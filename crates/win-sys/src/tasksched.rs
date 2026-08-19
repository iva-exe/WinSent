//! Task Scheduler (SPEC kap. 7, backend 3): úlohy s triggerem při
//! přihlášení/bootu přes COM `ITaskService`. Čtení i přepnutí
//! `Enabled` — úloha se NIKDY nemaže.
//!
//! Vyžaduje COM na vlákně (`wic::init_com_for_thread`).

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler, TASK_TRIGGER_BOOT,
    TASK_TRIGGER_LOGON,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};

use crate::Error;

/// VARIANT s celým číslem (indexy kolekcí Task Scheduleru).
fn variant_i4(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    // SAFETY: zapisujeme do zero-inicializované unie platnou kombinaci
    // vt = VT_I4 + lVal, přesně jak API očekává.
    unsafe {
        let inner = &mut var.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = v;
    }
    var
}

/// Prázdný VARIANT (VT_EMPTY) pro Connect.
fn variant_empty() -> VARIANT {
    VARIANT::default()
}

/// Naplánovaná úloha spouštěná při startu/přihlášení.
#[derive(Debug, Clone)]
pub struct StartupTask {
    /// Celá cesta ve stromu úloh (např. `\Adobe\Updater`).
    pub path: String,
    pub name: String,
    pub enabled: bool,
    /// Spouštěná binárka (první akce), když jde zjistit.
    pub command: Option<String>,
}

/// Připojí se k Task Scheduleru.
fn connect() -> Result<ITaskService, Error> {
    // SAFETY: standardní COM sekvence; volající má COM inicializované.
    unsafe {
        let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| Error::Win32 {
                call: "CoCreateInstance(TaskScheduler)",
                code: e.code().0,
            })?;
        svc.Connect(
            &variant_empty(),
            &variant_empty(),
            &variant_empty(),
            &variant_empty(),
        )
        .map_err(|e| Error::Win32 {
            call: "ITaskService::Connect",
            code: e.code().0,
        })?;
        Ok(svc)
    }
}

/// Vyjmenuje úlohy s logon/boot triggerem (rekurzivně, max 4 úrovně).
pub fn startup_tasks() -> Result<Vec<StartupTask>, Error> {
    let svc = connect()?;
    let mut out = Vec::new();
    // SAFETY: COM objekty se uvolní přes Drop; chyby jednotlivých
    // složek jen přeskakujeme (některé nejsou čitelné ani pro SYSTEM).
    unsafe {
        let root = svc.GetFolder(&BSTR::from("\\")).map_err(|e| Error::Win32 {
            call: "ITaskService::GetFolder",
            code: e.code().0,
        })?;
        let mut stack: Vec<(ITaskFolder, u8)> = vec![(root, 0)];
        while let Some((folder, depth)) = stack.pop() {
            if let Ok(tasks) = folder.GetTasks(0) {
                let count = tasks.Count().unwrap_or(0);
                for i in 1..=count {
                    let Ok(task) = tasks.get_Item(&variant_i4(i)) else {
                        continue;
                    };
                    if let Some(item) = task_if_startup(&task) {
                        out.push(item);
                    }
                }
            }
            if depth < 4 {
                if let Ok(subs) = folder.GetFolders(0) {
                    let count = subs.Count().unwrap_or(0);
                    for i in 1..=count {
                        if let Ok(sub) = subs.get_Item(&variant_i4(i)) {
                            stack.push((sub, depth + 1));
                        }
                    }
                }
            }
        }
    }
    out.sort_by_key(|t| t.name.to_lowercase());
    Ok(out)
}

/// Vrátí StartupTask, pokud úloha má logon/boot trigger.
/// SAFETY: `task` je platný COM objekt.
unsafe fn task_if_startup(task: &IRegisteredTask) -> Option<StartupTask> {
    let def = task.Definition().ok()?;
    let triggers = def.Triggers().ok()?;
    let mut count = 0i32;
    let _ = triggers.Count(&mut count);
    let mut is_startup = false;
    for i in 1..=count {
        if let Ok(t) = triggers.get_Item(i) {
            let mut kind = windows::Win32::System::TaskScheduler::TASK_TRIGGER_TYPE2::default();
            if t.Type(&mut kind).is_ok()
                && (kind == TASK_TRIGGER_LOGON || kind == TASK_TRIGGER_BOOT)
            {
                is_startup = true;
                break;
            }
        }
    }
    if !is_startup {
        return None;
    }
    // Příkaz z první akce (IExecAction).
    let command = def.Actions().ok().and_then(|acts| {
        let a = acts.get_Item(1).ok()?;
        let exec: IExecAction = a.cast().ok()?;
        let mut path = BSTR::default();
        exec.Path(&mut path).ok()?;
        let s = path.to_string();
        (!s.is_empty()).then_some(s)
    });
    Some(StartupTask {
        path: task.Path().ok()?.to_string(),
        name: task.Name().ok()?.to_string(),
        enabled: task.Enabled().map(|v| v.as_bool()).unwrap_or(true),
        command,
    })
}

/// Zapne/vypne úlohu (SPEC 7 — `IRegisteredTask.Enabled`, ne mazání).
pub fn set_task_enabled(path: &str, enabled: bool) -> Result<(), Error> {
    let svc = connect()?;
    // SAFETY: cesta úlohy se hledá od kořene; COM objekty se uvolní.
    unsafe {
        let (folder_path, name) = match path.rfind('\\') {
            Some(0) => ("\\".to_string(), path[1..].to_string()),
            Some(i) => (path[..i].to_string(), path[i + 1..].to_string()),
            None => ("\\".to_string(), path.to_string()),
        };
        let folder = svc
            .GetFolder(&BSTR::from(folder_path))
            .map_err(|e| Error::Win32 {
                call: "GetFolder(task)",
                code: e.code().0,
            })?;
        let task = folder
            .GetTask(&BSTR::from(name))
            .map_err(|e| Error::Win32 {
                call: "GetTask",
                code: e.code().0,
            })?;
        task.SetEnabled(enabled.into()).map_err(|e| Error::Win32 {
            call: "IRegisteredTask::SetEnabled",
            code: e.code().0,
        })
    }
}

/// Přečte aktuální stav úlohy (fáze ověření).
pub fn task_enabled(path: &str) -> Option<bool> {
    let svc = connect().ok()?;
    // SAFETY: jen čtení; objekty se uvolní.
    unsafe {
        let (folder_path, name) = match path.rfind('\\') {
            Some(0) => ("\\".to_string(), path[1..].to_string()),
            Some(i) => (path[..i].to_string(), path[i + 1..].to_string()),
            None => ("\\".to_string(), path.to_string()),
        };
        let folder = svc.GetFolder(&BSTR::from(folder_path)).ok()?;
        let task = folder.GetTask(&BSTR::from(name)).ok()?;
        task.Enabled().ok().map(|v| v.as_bool())
    }
}

/// Co úloha doopravdy spouští — cesta z první `IExecAction`.
/// U úloh s `IComHandlerAction` (jen CLSID, žádná cesta) vrací `None`;
/// volající si takovou úlohu má vyložit jako „nevíme", ne jako „nic".
pub fn task_payload(path: &str) -> Option<String> {
    let svc = connect().ok()?;
    // SAFETY: jen čtení; COM objekty uvolní Drop.
    unsafe {
        let (folder_path, name) = match path.rfind(char::from(92u8)) {
            Some(0) => ("\\".to_string(), path[1..].to_string()),
            Some(i) => (path[..i].to_string(), path[i + 1..].to_string()),
            None => ("\\".to_string(), path.to_string()),
        };
        let folder = svc.GetFolder(&BSTR::from(folder_path)).ok()?;
        let task = folder.GetTask(&BSTR::from(name)).ok()?;
        let def = task.Definition().ok()?;
        let acts = def.Actions().ok()?;
        let a = acts.get_Item(1).ok()?;
        let exec: IExecAction = a.cast().ok()?;
        let mut p = BSTR::default();
        exec.Path(&mut p).ok()?;
        let s = p.to_string();
        (!s.trim().is_empty()).then_some(s)
    }
}
