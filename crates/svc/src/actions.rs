//! Orchestrátor mutujících akcí (v5, SPEC 17.4): plán → validace →
//! provedení → ověření. Jediné místo, které smí spustit exekutor —
//! a NIKDY bez `Verdict::Allow` z validate/.
//!
//! Audit se zapisuje synchronně vlastním spojením (mutace jsou vzácné,
//! id záznamu se vrací hned v odpovědi). Plány T1 mají expiraci —
//! zastaralý plán je při Execute zamítnut.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use core_types::action::{Action, ActionClass, ActionPlan, ActionResult};

/// Životnost T1 plánu (fáze 1 → potvrzení uživatelem).
const PLAN_TTL_S: i64 = 60;

/// Jak dlouho čeká schválené mazání na hlášení výsledku od UI. Pak se
/// cesty zapomenou — záznam v auditu zůstane „probíhá".
const DELETE_REPORT_TTL_S: i64 = 3600;

/// Plán, jak ho uživatel viděl, a co k němu služba drží stranou.
struct UlozenyPlan {
    plan: ActionPlan,
    /// Zmrazený strom ukončení (v11): přesně ty procesy (pid + čas
    /// vzniku), které uživatel v plánu potvrdil. Při provedení se znovu
    /// NEpočítá — jinak by padlo i to, co v plánu nebylo.
    obeti: Vec<actor_proc::Victim>,
}

pub struct Orchestrator {
    /// Vlastní zapisovací spojení pro audit (busy_timeout — writer
    /// vzorků má přednost, mutace jsou vzácné).
    audit_conn: Mutex<store::Connection>,
    plans: Mutex<HashMap<u64, UlozenyPlan>>,
    /// Schválená mazání čekající na hlášení z UI: audit_id → (ověřené
    /// cesty, kdy). Podle nich služba sama zkontroluje, že soubory
    /// zmizely — výsledek nerozhoduje UI.
    mazani: Mutex<HashMap<i64, (Vec<String>, i64)>>,
    next_id: AtomicU64,
    /// Striktní režim (SPEC 17.5): nevratná T1 → bod obnovení.
    strict: bool,
    /// Žádost na zapisovací vlákno, ať pustí zámek DB (viz audit_conn).
    uvolni_zapis: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Kanál, kterým se zapisovací vlákno hned probudí. Bez něj by si
    /// žádosti všimlo až po svém timeoutu (100 ms) a přepnutí po startu
    /// (T0, rozpočet pod 50 ms) trvalo přes sto milisekund.
    probud: Option<std::sync::mpsc::SyncSender<crate::incidents::StoreMsg>>,
}

impl Orchestrator {
    pub fn new(
        audit_conn: store::Connection,
        uvolni_zapis: std::sync::Arc<std::sync::atomic::AtomicBool>,
        probud: Option<std::sync::mpsc::SyncSender<crate::incidents::StoreMsg>>,
    ) -> Orchestrator {
        Orchestrator {
            audit_conn: Mutex::new(audit_conn),
            plans: Mutex::new(HashMap::new()),
            mazani: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            strict: true,
            uvolni_zapis,
            probud,
        }
    }

    /// Spojení pro zápis auditu. Napřed požádá zapisovací vlákno, ať
    /// pustí zámek databáze — dávka vzorků ho jinak drží déle, než
    /// auditní spojení čeká (viz `Davka::zadost` v daemonu).
    fn audit_conn(&self) -> std::sync::MutexGuard<'_, store::Connection> {
        self.uvolni_zapis.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(tx) = &self.probud {
            // Plný kanál nevadí: vlákno pak stejně zpracovává zprávy
            // a žádost si přečte před každou z nich.
            let _ = tx.try_send(crate::incidents::StoreMsg::UvolniZamek);
        }
        self.audit_conn.lock().expect("audit conn lock")
    }

    /// Audit zápis (allow i deny — každá akce nechává stopu).
    fn audit(
        &self,
        action: &Action,
        verdict: &str,
        deny_reason: Option<&str>,
        outcome: Option<&str>,
        detail: Option<&str>,
    ) -> i64 {
        let conn = self.audit_conn();
        // U mazání se cesta zpět NEPÍŠE tady: při schválení se ještě nic
        // nepřesunulo a uživatel může přesun zrušit nebo položku, která
        // se do koše nevejde, smazat natrvalo. Doplní ji `report_delete`
        // podle toho, co doopravdy skončilo v koši.
        let reversible = actor_toggle::reversible_hint(action);
        store::audit::insert(
            &conn,
            unix_now(),
            action.name(),
            &action.target(),
            action.class().as_str(),
            verdict,
            deny_reason,
            outcome,
            reversible.as_deref(),
            detail,
        )
        .unwrap_or_else(|e| {
            // Audit selhal → akce se stejně hlásí, ale hlasitě do logu.
            tracing::error!(error = %e, "zápis auditu selhal");
            -1
        })
    }

    /// T0: validace + provedení + ověření v jednom volání (< 50 ms).
    /// Bez potvrzení — akce je vratná z definice.
    pub fn toggle(&self, action: Action) -> ActionResult {
        let t0 = Instant::now();
        if action.class() != ActionClass::T0 {
            let reason = "akce není třídy T0 — vyžaduje plán a potvrzení".to_string();
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Startup položky sahají na Task Scheduler přes COM — obslužné
        // vlákno pipe ho potřebuje inicializované (idempotentní).
        win_sys::wic::init_com_for_thread();
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Fáze 3 + 4: ověření proti stavu — nikdy mlčky (SPEC 17.4).
        let (verified, rolled_back, detail) = execute_for(&action);
        let outcome = if verified {
            "ok"
        } else if rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&detail));
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id: id,
        }
    }

    /// T1 fáze 1 — PLÁN (+ časná validace pro UX; rozhodná validace
    /// běží znovu ČERSTVĚ při Execute).
    pub fn plan(&self, action: Action) -> Result<ActionPlan, ActionResult> {
        let t0 = Instant::now();
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Strom ukončení se počítá TADY a jen tady — uživatel potvrzuje
        // přesně tenhle seznam (v11).
        let (steps, obeti) = match &action {
            Action::KillProc { .. } => {
                let (tree, steps) = actor_proc::plan(&action);
                (steps, tree.victims)
            }
            _ => (plan_for(&action), Vec::new()),
        };
        let plan = ActionPlan {
            plan_id: self.next_id.fetch_add(1, Ordering::SeqCst),
            class: action.class(),
            action,
            steps,
            expires_ts: unix_now() + PLAN_TTL_S,
        };
        let mut plans = self.plans.lock().expect("plans lock");
        // Úklid při vkládání: plán, který uživatel nikdy nepotvrdil, by
        // jinak v mapě zůstal navždy. Malé, ale bez stropu — a data
        // nástroje strop mít musí (SPEC 2.3).
        let now = unix_now();
        plans.retain(|_, p| p.plan.expires_ts >= now);
        plans.insert(
            plan.plan_id,
            UlozenyPlan {
                plan: plan.clone(),
                obeti,
            },
        );
        drop(plans);
        Ok(plan)
    }

    /// T1 fáze 2–4 — po potvrzení uživatelem. Plán je jednorázový.
    pub fn execute(&self, plan_id: u64) -> ActionResult {
        let t0 = Instant::now();
        let ulozeny = {
            let mut plans = self.plans.lock().expect("plans lock");
            // Mazání služba NEPROVÁDÍ (v11): jako SYSTEM by soubory
            // odklidila do koše SYSTEMu, odkud je uživatel nevrátí. Plán
            // zůstává k dispozici pro AuthorizeDelete — přesun do koše
            // udělá okno Winsentu pod účtem uživatele.
            if plans
                .get(&plan_id)
                .is_some_and(|p| matches!(p.plan.action, Action::DeleteFiles { .. }))
            {
                return deny_result(
                    "do koše maže okno Winsentu pod tvým účtem — služba sama nemaže",
                    t0,
                    -1,
                );
            }
            plans.remove(&plan_id)
        };
        let Some(UlozenyPlan { plan, obeti }) = ulozeny else {
            return deny_result("plán neexistuje (už proveden, nebo nikdy nevznikl)", t0, -1);
        };
        let action = plan.action;
        if unix_now() > plan.expires_ts {
            let reason = "plán vypršel — sestav ho znovu (stav systému se mohl změnit)";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Fáze 2: validace proti ŽIVÉMU stavu teď, ne při plánu (17.3).
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return deny_result(reason, t0, id);
        }
        // Striktní režim: nevratný krok → bod obnovení PŘED provedením.
        if self.strict && plan.steps.iter().any(|s| !s.reversible) {
            if let Err(e) = win_sys::restore::create_restore_point("Winsent: před akcí") {
                let reason = format!("bod obnovení se nepodařil ({e}) — akce zastavena");
                let id = self.audit(&action, "deny", Some(&reason), None, None);
                return deny_result(reason, t0, id);
            }
        }
        // Fáze 3 + 4. Ukončení procesů jde podle zmrazeného stromu.
        let (verified, rolled_back, detail) = match &action {
            Action::KillProc { .. } => {
                let out = actor_proc::execute_frozen(&obeti);
                (out.ok && actor_proc::verify(&action), false, out.detail)
            }
            _ => execute_for(&action),
        };
        let outcome = if verified {
            "ok"
        } else if rolled_back {
            "rolled_back"
        } else {
            "failed"
        };
        let id = self.audit(&action, "allow", None, Some(outcome), Some(&detail));
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id: id,
        }
    }

    /// Mazání do koše, fáze 2 — SCHVÁLENÍ místo provedení (v11).
    ///
    /// Služba běží jako SYSTEM a koš patří účtu, který maže: soubory od
    /// služby končily v `$Recycle.Bin\S-1-5-18`, kam uživatel nevidí,
    /// a UI přitom slibovalo „jde vrátit z koše". Proto se tu jen znovu
    /// validuje živý stav, cesty se převedou na kanonický tvar a zapíše
    /// se audit; přesun do koše udělá UI pod tokenem uživatele. Vrací
    /// přesně ty cesty, které vrstva ověřila — jiné UI mazat nemá.
    pub fn authorize_delete(&self, plan_id: u64) -> Result<(Vec<String>, i64), ActionResult> {
        let t0 = Instant::now();
        let ulozeny = self.plans.lock().expect("plans lock").remove(&plan_id);
        let Some(UlozenyPlan { plan, .. }) = ulozeny else {
            return Err(deny_result(
                "plán neexistuje (už použit, nebo nikdy nevznikl)",
                t0,
                -1,
            ));
        };
        let action = plan.action;
        let Action::DeleteFiles { paths } = &action else {
            return Err(deny_result("plán není mazání", t0, -1));
        };
        if unix_now() > plan.expires_ts {
            let reason = "plán vypršel — sestav ho znovu (stav systému se mohl změnit)";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Validace proti ŽIVÉMU stavu teď, ne při plánu (SPEC 17.3).
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Kanonický tvar cest: co se ověřilo, to se smaže (ne `PROGRA~1`
        // ani jiný zápis téhož, který by UI mohlo poslat dál).
        let canon = match validate::delete_targets(paths) {
            Ok(c) => c,
            Err(reason) => {
                let id = self.audit(&action, "deny", Some(&reason), None, None);
                return Err(deny_result(reason, t0, id));
            }
        };
        // Výsledek zatím neznáme — doplní ho ReportDelete (fáze 4).
        let id = self.audit(
            &action,
            "allow",
            None,
            Some("running"),
            Some(&format!(
                "do koše uživatele přesouvá okno Winsentu: {} položek",
                canon.len()
            )),
        );
        let now = unix_now();
        let mut mazani = self.mazani.lock().expect("mazani lock");
        // Úklid při vkládání, ať mapa nemá jen vstup (SPEC 2.3).
        mazani.retain(|_, (_, ts)| now - *ts < DELETE_REPORT_TTL_S);
        mazani.insert(id, (canon.clone(), now));
        drop(mazani);
        Ok((canon, id))
    }

    /// Mazání do koše, fáze 4 — OVĚŘENÍ po přesunu (v11).
    ///
    /// UI hlásí, jak přesun dopadl; rozhoduje ale disk: cesty, které
    /// služba schválila, už nesmí existovat. Výsledek se smí doplnit jen
    /// k rozpracovanému mazání — ne k libovolnému záznamu auditu.
    ///
    /// `recycled` = kolik položek podle Windows skončilo v koši. Jen na
    /// ně se smí historie odvolat „obnovit z koše" — a nikdy na víc, než
    /// kolik jich z disku opravdu zmizelo.
    pub fn report_delete(
        &self,
        audit_id: i64,
        ok: bool,
        recycled: u32,
        detail: &str,
    ) -> ActionResult {
        let t0 = Instant::now();
        let conn = self.audit_conn();
        if audit_id <= 0 || !rozpracovane_mazani(&conn, audit_id) {
            return deny_result("auditní záznam nepatří rozpracovanému mazání", t0, audit_id);
        }
        let cesty = self
            .mazani
            .lock()
            .expect("mazani lock")
            .remove(&audit_id)
            .map(|(c, _)| c);
        let recycled = recycled as usize;
        let (outcome, detail, v_kosi) = match cesty {
            Some(c) => {
                let zbylo = c
                    .iter()
                    .filter(|p| std::path::Path::new(p.as_str()).exists())
                    .count();
                let outcome = if zbylo == 0 { "ok" } else { "failed" };
                (
                    outcome,
                    format!("{detail}; ověřeno službou: zbylo {zbylo} z {}", c.len()),
                    recycled.min(c.len() - zbylo),
                )
            }
            // Služba mezitím restartovala a cesty nezná — zbývá slovo UI.
            None => (
                if ok { "ok" } else { "failed" },
                format!("{detail}; neověřeno (služba mezitím restartovala)"),
                recycled,
            ),
        };
        if let Err(e) = store::audit::set_outcome(&conn, audit_id, outcome, &detail) {
            tracing::error!(error = %e, "doplnění výsledku mazání do auditu selhalo");
        }
        let zpet = actor_file::undo_hint(v_kosi);
        if let Err(e) = store::audit::set_reversible(&conn, audit_id, zpet.as_deref()) {
            tracing::error!(error = %e, "doplnění cesty zpět k mazání selhalo");
        }
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id,
        }
    }

    /// Odinstalace, fáze 2 — SCHVÁLENÍ místo provedení.
    ///
    /// Služba běží jako SYSTEM v session 0. Odinstalátor spuštěný odtud
    /// nemá viditelnou plochu (uživatel neodklikne dialogy), vidí HKCU
    /// SYSTEMu místo uživatelova a dostane práva, se kterými nepočítá —
    /// pozorovaný následek byl zásek systému a rozbité audio. Proto se
    /// tady jen znovu validuje živý stav, přečte se ČERSTVÝ příkaz
    /// z registru a zapíše audit; spuštění dělá UI ve své relaci.
    pub fn authorize_uninstall(&self, plan_id: u64) -> Result<(String, i64), ActionResult> {
        let t0 = Instant::now();
        let ulozeny = self.plans.lock().expect("plans lock").remove(&plan_id);
        let Some(UlozenyPlan { plan, .. }) = ulozeny else {
            return Err(deny_result(
                "plán neexistuje (už použit, nebo nikdy nevznikl)",
                t0,
                -1,
            ));
        };
        let action = plan.action;
        let Action::UninstallApp { identity_key } = &action else {
            return Err(deny_result("plán není odinstalace", t0, -1));
        };
        if unix_now() > plan.expires_ts {
            let reason = "plán vypršel — sestav ho znovu (stav systému se mohl změnit)";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Validace proti ŽIVÉMU stavu teď, ne při plánu (SPEC 17.3).
        let mut ctx = validate::LiveContext::new();
        if let validate::Verdict::Deny { reason } = validate::validate(&action, &mut ctx) {
            let id = self.audit(&action, "deny", Some(&reason), None, None);
            return Err(deny_result(reason, t0, id));
        }
        // Příkaz čte vrstva sama z registru — UI ho nemůže podvrhnout.
        let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
        let Some(command) = validate::uninstall_command(name) else {
            let reason = "odinstalační příkaz se v registru nenašel";
            let id = self.audit(&action, "deny", Some(reason), None, None);
            return Err(deny_result(reason, t0, id));
        };
        // Bod obnovení se tady VĚDOMĚ nedělá. Snímek VSS trvá desítky
        // sekund a uživatel by mezitím jen koukal na nic — odinstalátor
        // by se spustil až dlouho po kliknutí. Windows navíc u MSI
        // instalací dělají vlastní bod samy. Bod obnovení zůstává
        // u ostatních nevratných T1 akcí (viz `execute`).
        // Výsledek zatím neznáme — doplní ho ReportUninstall (fáze 4).
        let id = self.audit(
            &action,
            "allow",
            None,
            Some("running"),
            Some(&format!("spouští UI v relaci uživatele: {command}")),
        );
        Ok((command, id))
    }

    /// Odinstalace, fáze 4 — OVĚŘENÍ po doběhnutí odinstalátoru.
    /// Rozhoduje registr, ne návratový kód odinstalátoru ani UI.
    pub fn report_uninstall(
        &self,
        audit_id: i64,
        identity_key: &str,
        detail: &str,
    ) -> ActionResult {
        let t0 = Instant::now();
        // Prázdný klíč posílá UI jen tehdy, když odinstalátor vůbec nešel
        // spustit (zrušené UAC, chybějící soubor). Pak se nic nikomu
        // nepředalo — i aplikace ze Steamu musí skončit jako failed.
        let start_selhal = identity_key.is_empty();
        // Výsledek se smí doplnit JEN k rozpracované odinstalaci, a to
        // k té aplikaci, kterou zapsal audit — ne té, kterou jmenuje
        // klient. Dřív se `UPDATE … WHERE id = ?` provedl pro libovolné
        // id: přes pipe šlo přepsat výsledek kteréhokoli záznamu
        // (i odmítnutí nebo ukončení procesu) a UI po zrušeném UAC
        // posílalo prázdný klíč, pro který „ověření" vyšlo jako
        // úspěch — historie pak tvrdila, že se odinstalace povedla,
        // přestože se odinstalátor vůbec nespustil.
        let identity_key = if audit_id > 0 {
            let conn = self.audit_conn.lock().expect("audit conn lock");
            match rozpracovana_odinstalace(&conn, audit_id) {
                Some(target) => target,
                None => {
                    return deny_result(
                        "auditní záznam nepatří rozpracované odinstalaci",
                        t0,
                        audit_id,
                    )
                }
            }
        } else {
            identity_key.to_string()
        };
        let identity_key = identity_key.as_str();
        let action = Action::UninstallApp {
            identity_key: identity_key.to_string(),
        };
        let verified = actor_app::verify(&action);
        // Odinstalátor, který práci jen předal svému launcheru, se vrací
        // hned a v registru je aplikace pořád — to není selhání, to je
        // stav „čeká se na tebe v okně Steamu". Hlásit `failed` znamenalo
        // posílat uživatele opakovaně dělat něco, co se ve skutečnosti
        // nikdy nepokazilo.
        let name = identity_key.strip_prefix("app:").unwrap_or(identity_key);
        let handoff = (!verified && !start_selhal)
            .then(|| validate::uninstall_command(name))
            .flatten()
            .and_then(|c| actor_app::hands_off(&c));
        let outcome = match (verified, handoff) {
            (true, _) => "ok",
            (false, Some(_)) => "handed",
            (false, None) => "failed",
        };
        let detail = match handoff {
            Some(who) => format!("předáno {who} — dokonči odinstalaci v jeho okně; {detail}"),
            None => detail.to_string(),
        };
        if audit_id > 0 {
            let conn = self.audit_conn();
            // Znovu pod zámkem: mezi čtením a zápisem běželo ověření
            // registru a záznam mezitím mohl uzavřít souběžný report.
            if rozpracovana_odinstalace(&conn, audit_id).is_none() {
                return deny_result(
                    "auditní záznam nepatří rozpracované odinstalaci",
                    t0,
                    audit_id,
                );
            }
            if let Err(e) = store::audit::set_outcome(&conn, audit_id, outcome, &detail) {
                tracing::error!(error = %e, "doplnění výsledku do auditu selhalo");
            }
        }
        ActionResult {
            verdict: "allow".into(),
            deny_reason: None,
            outcome: Some(outcome.into()),
            duration_ms: t0.elapsed().as_millis() as u64,
            audit_id,
        }
    }
}

/// Plán podle typu akce — exekutor si vybírá orchestrátor, ne UI.
fn plan_for(action: &Action) -> Vec<core_types::action::PlanStep> {
    match action {
        Action::KillProc { .. } => actor_proc::plan(action).1,
        Action::DeleteFiles { .. } => actor_file::plan(action),

        Action::UninstallApp { .. } => actor_app::plan(action),
        Action::PurgeGhost { .. } => actor_app::purge_plan(action),
        _ => actor_toggle::plan(action),
    }
}

/// Provedení + ověření podle typu akce (fáze 3 a 4).
/// Vrací (ok, rolled_back, detail).
fn execute_for(action: &Action) -> (bool, bool, String) {
    match action {
        // Pojistka: ukončuje se jen zmrazený strom z potvrzeného plánu
        // (`Orchestrator::execute`). Sem by se kill dostal jen novou
        // cestou v kódu — a ta by strom počítala znovu.
        Action::KillProc { .. } => (
            false,
            false,
            "ukončení jde jen přes potvrzený plán se zmrazeným stromem".into(),
        ),
        // Pojistka: služba jako SYSTEM nemaže (koš SYSTEMu uživatel
        // nevidí). Do koše přesouvá okno Winsentu pod účtem uživatele.
        Action::DeleteFiles { .. } => (
            false,
            false,
            "služba nemaže — do koše přesouvá okno Winsentu pod účtem uživatele".into(),
        ),

        // Úklid záznamu v registru běží ve službě: sahá se na registr
        // a prázdné složky, žádné okno se neotvírá, takže session 0
        // nevadí — a naopak jsou tu práva, která uživatel mít nemusí.
        Action::PurgeGhost { .. } => {
            let (ok, detail) = actor_app::purge_execute(action);
            (ok && actor_app::purge_verify(action), false, detail)
        }
        // Pojistka: odinstalátor se ze služby NIKDY nespouští. Kdyby
        // sem akce přesto dorazila (nová cesta v kódu), skončí tady —
        // ne na neviditelné ploše session 0 pod účtem SYSTEM.
        Action::UninstallApp { .. } => (
            false,
            false,
            "odinstalátor spouští UI v relaci uživatele — služba ho nespouští".into(),
        ),
        _ => {
            let out = actor_toggle::execute(action);
            let verified = out.ok && actor_toggle::verify(action);
            (verified, out.rolled_back, out.detail)
        }
    }
}

/// Cíl (identity_key) auditního záznamu, který je rozpracovanou
/// odinstalací — jinak None.
///
/// 'handed' zůstává otevřené: launcher (Steam) dokončí odinstalaci až po
/// potvrzení ve svém okně a pozdější report je legitimní. Uzavřený
/// výsledek ('ok', 'failed') ani záznamy jiných akcí se nepřepisují.
fn rozpracovana_odinstalace(conn: &store::Connection, audit_id: i64) -> Option<String> {
    let uninstall = Action::UninstallApp {
        identity_key: String::new(),
    }
    .name();
    conn.query_row(
        "SELECT target FROM audit
         WHERE id = ?1 AND action = ?2 AND verdict = 'allow'
           AND outcome IN ('running', 'handed')",
        (audit_id, uninstall),
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Je auditní záznam rozpracovaným (schváleným, ještě neohlášeným)
/// mazáním? Jen k takovému smí ReportDelete doplnit výsledek — jinak by
/// šlo přes pipe přepsat výsledek kteréhokoli záznamu.
fn rozpracovane_mazani(conn: &store::Connection, audit_id: i64) -> bool {
    let delete = Action::DeleteFiles { paths: Vec::new() }.name();
    conn.query_row(
        "SELECT 1 FROM audit
         WHERE id = ?1 AND action = ?2 AND verdict = 'allow' AND outcome = 'running'",
        (audit_id, delete),
        |_| Ok(()),
    )
    .is_ok()
}

fn deny_result(reason: impl Into<String>, t0: Instant, audit_id: i64) -> ActionResult {
    ActionResult {
        verdict: "deny".into(),
        deny_reason: Some(reason.into()),
        outcome: None,
        duration_ms: t0.elapsed().as_millis() as u64,
        audit_id,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orch() -> Orchestrator {
        let conn = store::Connection::open_in_memory().expect("in-memory DB");
        store::migrations::run(&conn).expect("migrace");
        Orchestrator::new(conn, Default::default(), None)
    }

    // Brána v5: expirovaný plán je při Execute odmítnut.
    #[test]
    fn expired_plan_rejected() {
        let o = orch();
        let plan = o
            .plan(Action::TestOp {
                target: "demo".into(),
                fail_at: None,
            })
            .expect("plán projde");
        o.plans
            .lock()
            .unwrap()
            .get_mut(&plan.plan_id)
            .unwrap()
            .plan
            .expires_ts = 1;
        let r = o.execute(plan.plan_id);
        assert_eq!(r.verdict, "deny");
        assert!(r.deny_reason.unwrap().contains("vypršel"));
    }

    // T1 akce nesmí projít T0 (toggle) cestou — vyžaduje plán+potvrzení.
    #[test]
    fn t1_via_toggle_denied() {
        let o = orch();
        let r = o.toggle(Action::TestOp {
            target: "demo".into(),
            fail_at: None,
        });
        assert_eq!(r.verdict, "deny");
    }

    // Deny při toggle nechává auditní stopu.
    #[test]
    fn deny_leaves_audit_trail() {
        let o = orch();
        let r = o.toggle(Action::TestToggle {
            key: "spatny:klic".into(),
            on: true,
        });
        assert_eq!(r.verdict, "deny");
        assert!(r.audit_id > 0, "deny musí mít auditní záznam");
        let conn = o.audit_conn.lock().unwrap();
        let rows = store::audit::recent(&conn, 5).unwrap();
        assert_eq!(rows[0].verdict, "deny");
    }

    fn outcome_of(o: &Orchestrator, id: i64) -> Option<String> {
        let conn = o.audit_conn.lock().unwrap();
        store::audit::recent(&conn, 50)
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .and_then(|r| r.outcome)
    }

    fn reversible_of(o: &Orchestrator, id: i64) -> Option<String> {
        let conn = o.audit_conn.lock().unwrap();
        store::audit::recent(&conn, 50)
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .and_then(|r| r.reversible)
    }

    // ReportUninstall nesmí přepsat záznam jiné akce ani už uzavřenou
    // odinstalaci; ověřuje se aplikace ze záznamu, ne z požadavku.
    #[test]
    fn report_uninstall_jen_pro_rozpracovanou_odinstalaci() {
        let o = orch();
        let kill = o.audit(
            &Action::TestOp {
                target: "demo".into(),
                fail_at: None,
            },
            "allow",
            None,
            Some("ok"),
            None,
        );
        let r = o.report_uninstall(kill, "app:cokoli", "podvrh");
        assert_eq!(r.verdict, "deny");
        assert_eq!(outcome_of(&o, kill).as_deref(), Some("ok"));

        // Aplikace, která v registru není → ověření „odinstalováno" = ok.
        let uninst = o.audit(
            &Action::UninstallApp {
                identity_key: "app:Winsent test neexistujici aplikace 7f3c".into(),
            },
            "allow",
            None,
            Some("running"),
            None,
        );
        // Prázdný klíč od klienta (UI po zrušeném UAC) se ignoruje.
        let r = o.report_uninstall(uninst, "", "spuštění selhalo");
        assert_eq!(r.verdict, "allow");
        assert_eq!(outcome_of(&o, uninst).as_deref(), Some("ok"));

        // Podruhé už je záznam uzavřený.
        let r = o.report_uninstall(uninst, "", "znovu");
        assert_eq!(r.verdict, "deny");
    }

    // Mazání (v11): služba sama nemaže, jen schválí a vrátí kanonické
    // cesty; výsledek rozhoduje disk a jde doplnit jen jednou.
    #[test]
    fn mazani_schvaleni_a_hlaseni() {
        let o = orch();
        let f = std::env::temp_dir().join(format!("winsent-svc-mazani-{}.tmp", std::process::id()));
        std::fs::write(&f, b"x").expect("zapsat");
        let p = f.to_string_lossy().into_owned();
        let plan = o
            .plan(Action::DeleteFiles {
                paths: vec![p.clone()],
            })
            .expect("temp soubor projde plánem");

        // ExecuteAction mazání odmítne a plán NEspotřebuje.
        let r = o.execute(plan.plan_id);
        assert_eq!(r.verdict, "deny");
        assert!(f.exists(), "služba nesmí mazat");

        let (cesty, audit_id) = o.authorize_delete(plan.plan_id).expect("schváleno");
        assert_eq!(cesty.len(), 1);
        assert!(cesty[0].eq_ignore_ascii_case(&p));
        assert!(f.exists(), "schválení samo nic nemaže");
        assert_eq!(outcome_of(&o, audit_id).as_deref(), Some("running"));
        // Při schválení se ještě nic nepřesunulo — „obnovit z koše" by
        // byl slib, který zrušení nebo smazání natrvalo nedodrží.
        assert_eq!(reversible_of(&o, audit_id), None);
        // Plán je jednorázový.
        assert!(o.authorize_delete(plan.plan_id).is_err());

        // UI tvrdí úspěch, ale soubor tu pořád je → rozhodne disk;
        // a v koši nic není, ať UI hlásí cokoli.
        let r = o.report_delete(audit_id, true, 1, "podvrh");
        assert_eq!(r.outcome.as_deref(), Some("failed"));
        assert_eq!(reversible_of(&o, audit_id), None);
        // Uzavřený záznam se znovu nepřepisuje.
        let r = o.report_delete(audit_id, true, 1, "znovu");
        assert_eq!(r.verdict, "deny");
        let _ = std::fs::remove_file(&f);

        // Hlášení k cizímu záznamu (jiná akce) se odmítne.
        let jiny = o.audit(
            &Action::TestOp {
                target: "demo".into(),
                fail_at: None,
            },
            "allow",
            None,
            Some("running"),
            None,
        );
        assert_eq!(o.report_delete(jiny, true, 1, "podvrh").verdict, "deny");
    }

    // Mazání úspěšné: soubor zmizí (tady ho odklidí test sám, jako by
    // ho přesunulo UI) → ok. „Obnovit z koše" jen podle toho, co podle
    // Windows skončilo v koši.
    #[test]
    fn mazani_overeno_diskem() {
        let o = orch();
        let pryc = |jmeno: &str, v_kosi: u32| {
            let f = std::env::temp_dir().join(format!(
                "winsent-svc-mazani-{jmeno}-{}.tmp",
                std::process::id()
            ));
            std::fs::write(&f, b"x").expect("zapsat");
            let plan = o
                .plan(Action::DeleteFiles {
                    paths: vec![f.to_string_lossy().into_owned()],
                })
                .expect("plán");
            let (_, audit_id) = o.authorize_delete(plan.plan_id).expect("schváleno");
            let _ = std::fs::remove_file(&f);
            let r = o.report_delete(audit_id, false, v_kosi, "ui nevěděl");
            assert_eq!(r.outcome.as_deref(), Some("ok"));
            reversible_of(&o, audit_id)
        };
        assert_eq!(pryc("kos", 1).as_deref(), Some("obnovit z koše (1 položek)"));
        // Smazáno natrvalo (do koše se nevešlo, uživatel potvrdil).
        assert_eq!(pryc("natrvalo", 0), None);
        // UI nesmí slíbit víc položek, než kolik jich zmizelo.
        assert_eq!(pryc("vic", 5).as_deref(), Some("obnovit z koše (1 položek)"));
    }

    // Kill (v11): strom se zmrazí při plánu; cizí instance = prázdný plán.
    #[test]
    fn kill_plan_zmrazi_strom() {
        let o = orch();
        let mut buf = Vec::new();
        let ct = win_sys::proc::snapshot_processes(&mut buf)
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .unwrap()
            .create_time;
        // Vlastní proces validace odmítne (Winsent sám sebe) — plán nevznikne.
        let r = o.plan(Action::KillProc {
            pid: std::process::id(),
            create_time: ct,
            tree: true,
        });
        assert!(r.is_err());
    }
}
