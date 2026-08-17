//! Lokální účty a kdo z nich má práva správce (v9E, SPEC kap. 14).
//!
//! Vědomě přes **netapi32**, ne přes WMI: `Win32_UserAccount` se na
//! stroji v doméně ptá řadiče a umí zatuhnout na desítky sekund —
//! přesně to, co brána v9 zakazuje. `NetUserEnum` se serverem `NULL`
//! čte jen lokální SAM a vrátí se okamžitě.
//!
//! Sekce jen čte. Zakládat účty ani měnit práva neumíme a nebudeme —
//! na to jsou nástroje Windows (SPEC kap. 14).

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_MORE_DATA;
use windows::Win32::NetworkManagement::NetManagement::{
    NetApiBufferFree, NetLocalGroupGetMembers, NetUserEnum, NetUserGetInfo, FILTER_NORMAL_ACCOUNT,
    LOCALGROUP_MEMBERS_INFO_2, MAX_PREFERRED_LENGTH, USER_INFO_24, USER_INFO_3,
    UF_ACCOUNTDISABLE, UF_LOCKOUT, UF_PASSWD_NOTREQD,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    CreateWellKnownSid, LookupAccountSidW, WinBuiltinAdministratorsSid, PSID, SID_NAME_USE,
};

use crate::Error;

/// Jeden lokální účet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct User {
    pub name: String,
    /// Celé jméno, když ho účet má („Jan Novák").
    pub full_name: String,
    pub comment: String,
    /// Textová podoba SID (`S-1-5-21-…-1001`).
    pub sid: String,
    /// Má práva správce (je členem vestavěné skupiny Administrators).
    pub admin: bool,
    pub disabled: bool,
    pub locked: bool,
    /// Windows u účtu heslo NEVYŽADUJÍ (`UF_PASSWD_NOTREQD`).
    ///
    /// Neznamená to, že účet žádné heslo nemá — u účtů propojených
    /// s Microsoft účtem bývá příznak nastavený běžně. Naměřeno na
    /// vývojovém stroji: účet s heslem i s dvoufaktorem ho má taky.
    /// Vydávat to za „účet bez hesla" by byl poplach z ničeho.
    pub password_not_required: bool,
    /// Propojený s účtem Microsoft (přihlášení e-mailem).
    pub microsoft: bool,
    /// Poslední přihlášení na TOMHLE počítači (unix), 0 = nikdy.
    /// Windows ho nesynchronizují mezi stroji — proto ta formulace.
    pub last_logon: i64,
    /// Kolikrát se přihlásil (počítadlo SAM na tomto stroji).
    pub logons: u32,
}

/// Člen skupiny Administrators, který není lokálním účtem —
/// doménová skupina, účet Microsoft Entra a podobně.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignAdmin {
    /// `DOMÉNA\jméno`, jak ho hlásí systém.
    pub name: String,
    pub sid: String,
    /// Uživatel / skupina / neznámé — podle SID_NAME_USE.
    pub kind: String,
}

/// Přehled účtů.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accounts {
    pub users: Vec<User>,
    /// Správci, kteří nejsou lokální účty. Bez nich by seznam tvrdil
    /// „nikdo jiný tu admina nemá", což na firemním stroji neplatí.
    pub foreign_admins: Vec<ForeignAdmin>,
    /// Lokalizované jméno skupiny správců („Správci" na české instalaci).
    pub admin_group: String,
}

/// Buffer od netapi32 — uvolní se vždy, i při časné návratu.
struct NetBuf(*mut core::ffi::c_void);
impl Drop for NetBuf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ukazatel pochází z Net*-volání a uvolňuje se právě jednou.
            unsafe { NetApiBufferFree(Some(self.0)) };
        }
    }
}

/// Řetězec z `PWSTR` ukončený nulou.
fn wstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: netapi32 vrací ukazatele na nulou ukončené UTF-16.
    unsafe { p.to_string().unwrap_or_default() }
}

/// SID do čitelné podoby `S-1-5-…`.
fn sid_to_string(sid: PSID) -> String {
    if sid.is_invalid() {
        return String::new();
    }
    let mut out = PWSTR::null();
    // SAFETY: řetězec alokuje systém a hned se uvolňuje LocalFree.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut out).is_err() {
            return String::new();
        }
        let s = out.to_string().unwrap_or_default();
        let _ = windows::Win32::Foundation::LocalFree(Some(
            windows::Win32::Foundation::HLOCAL(out.0 as *mut _),
        ));
        s
    }
}

/// Lokalizované jméno vestavěné skupiny správců.
///
/// Natvrdo „Administrators" nestačí: na české instalaci se skupina
/// jmenuje „Správci" a dotaz by tiše vrátil prázdno — tedy „nikdo tu
/// nemá admin práva", což je ta nejhorší možná odpověď. Jméno se proto
/// odvozuje od neměnného SID S-1-5-32-544.
pub fn admin_group_name() -> Result<String, Error> {
    let mut sid_buf = [0u8; 68]; // SECURITY_MAX_SID_SIZE
    let mut len = sid_buf.len() as u32;
    let sid = PSID(sid_buf.as_mut_ptr() as *mut _);
    // SAFETY: buffer má velikost dle kontraktu API a předává se i s délkou.
    unsafe {
        CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(sid), &mut len).map_err(
            |e| Error::Win32 {
                call: "CreateWellKnownSid(BuiltinAdministrators)",
                code: e.code().0,
            },
        )?;
    }

    let mut name = [0u16; 256];
    let mut name_len = name.len() as u32;
    let mut domain = [0u16; 256];
    let mut domain_len = domain.len() as u32;
    let mut kind = SID_NAME_USE::default();
    // SAFETY: oba buffery mají hlášené velikosti.
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut kind,
        )
        .map_err(|e| Error::Win32 {
            call: "LookupAccountSidW(BuiltinAdministrators)",
            code: e.code().0,
        })?;
    }
    Ok(String::from_utf16_lossy(&name[..name_len as usize]))
}

/// Přečte lokální účty a jejich členství ve skupině správců.
pub fn accounts() -> Result<Accounts, Error> {
    let admin_group = admin_group_name()?;
    let (admin_sids, foreign_admins) = administrators(&admin_group)?;

    let mut users = Vec::new();
    // Pozor: NetUserEnum bere resume_handle jako u32, kdežto
    // NetLocalGroupGetMembers níž jako usize. Není to překlep.
    let mut resume = 0u32;
    loop {
        let mut buf: *mut u8 = std::ptr::null_mut();
        let mut read = 0u32;
        let mut total = 0u32;
        // SAFETY: buffer alokuje systém, uvolní ho NetBuf; resume_handle
        // se předává zpět beze změny podle kontraktu API.
        let rc = unsafe {
            NetUserEnum(
                PCWSTR::null(),
                3,
                FILTER_NORMAL_ACCOUNT,
                &mut buf,
                MAX_PREFERRED_LENGTH,
                &mut read,
                &mut total,
                Some(&mut resume),
            )
        };
        let _guard = NetBuf(buf as *mut _);
        // Net* API vrací stav jako NÁVRATOVOU hodnotu, ne přes
        // GetLastError — proto se porovnává rovnou.
        if rc != 0 && rc != ERROR_MORE_DATA.0 {
            return Err(Error::Win32 {
                call: "NetUserEnum",
                code: rc as i32,
            });
        }
        if buf.is_null() {
            break;
        }
        // SAFETY: systém vrátil `read` položek USER_INFO_3 za sebou.
        let rows =
            unsafe { std::slice::from_raw_parts(buf as *const USER_INFO_3, read as usize) };
        for r in rows {
            let name = wstr(r.usri3_name);
            if name.is_empty() {
                continue;
            }
            let (sid, microsoft) = account_extras(&name);
            users.push(User {
                admin: admin_sids.iter().any(|s| s.eq_ignore_ascii_case(&sid)),
                full_name: wstr(r.usri3_full_name),
                comment: wstr(r.usri3_comment),
                disabled: r.usri3_flags.0 & UF_ACCOUNTDISABLE.0 != 0,
                locked: r.usri3_flags.0 & UF_LOCKOUT.0 != 0,
                password_not_required: r.usri3_flags.0 & UF_PASSWD_NOTREQD.0 != 0,
                last_logon: r.usri3_last_logon as i64,
                logons: r.usri3_num_logons,
                microsoft,
                sid,
                name,
            });
        }
        if rc != ERROR_MORE_DATA.0 {
            break;
        }
    }

    users.sort_by(|a, b| b.admin.cmp(&a.admin).then_with(|| a.name.cmp(&b.name)));
    Ok(Accounts {
        users,
        foreign_admins,
        admin_group,
    })
}

/// SID účtu a jestli je propojený s účtem Microsoft.
///
/// `USER_INFO_3` nese jen RID, ne celý SID, takže párovat se členy
/// skupiny by znamenalo skládat SID ručně. Level 24 vrátí SID rovnou
/// a k tomu příznak internetové identity — podle SID by se přihlášení
/// e-mailem poznat nedalo.
fn account_extras(name: &str) -> (String, bool) {
    let wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf: *mut u8 = std::ptr::null_mut();
    // SAFETY: jméno je nulou ukončené, buffer uvolní NetBuf.
    let rc = unsafe { NetUserGetInfo(PCWSTR::null(), PCWSTR(wname.as_ptr()), 24, &mut buf) };
    let _guard = NetBuf(buf as *mut _);
    if rc != 0 || buf.is_null() {
        return (String::new(), false);
    }
    // SAFETY: při rc == 0 ukazuje buffer na jeden USER_INFO_24.
    let info = unsafe { &*(buf as *const USER_INFO_24) };
    (
        sid_to_string(info.usri24_user_sid),
        info.usri24_internet_identity.as_bool(),
    )
}

/// SIDy členů skupiny správců + ti členové, kteří nejsou lokální účty.
fn administrators(group: &str) -> Result<(Vec<String>, Vec<ForeignAdmin>), Error> {
    let wgroup: Vec<u16> = group.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sids = Vec::new();
    let mut foreign = Vec::new();
    let mut resume = 0usize;
    loop {
        let mut buf: *mut u8 = std::ptr::null_mut();
        let mut read = 0u32;
        let mut total = 0u32;
        // SAFETY: viz NetUserEnum výše — stejný kontrakt.
        let rc = unsafe {
            NetLocalGroupGetMembers(
                PCWSTR::null(),
                PCWSTR(wgroup.as_ptr()),
                2,
                &mut buf,
                MAX_PREFERRED_LENGTH,
                &mut read,
                &mut total,
                Some(&mut resume),
            )
        };
        let _guard = NetBuf(buf as *mut _);
        if rc != 0 && rc != ERROR_MORE_DATA.0 {
            return Err(Error::Win32 {
                call: "NetLocalGroupGetMembers",
                code: rc as i32,
            });
        }
        if buf.is_null() {
            break;
        }
        // SAFETY: `read` položek LOCALGROUP_MEMBERS_INFO_2 za sebou.
        let rows = unsafe {
            std::slice::from_raw_parts(buf as *const LOCALGROUP_MEMBERS_INFO_2, read as usize)
        };
        for m in rows {
            let sid = sid_to_string(m.lgrmi2_sid);
            if sid.is_empty() {
                continue;
            }
            // Doménové skupiny a účty Microsoft Entra tu jsou taky —
            // zahodit je by znamenalo tvrdit, že admina má jen ten,
            // koho vidíme v SAM.
            let name = wstr(m.lgrmi2_domainandname);
            if !is_local_account(&sid) {
                foreign.push(ForeignAdmin {
                    kind: sid_kind(m.lgrmi2_sidusage.0),
                    name,
                    sid: sid.clone(),
                });
            }
            sids.push(sid);
        }
        if rc != ERROR_MORE_DATA.0 {
            break;
        }
    }
    Ok((sids, foreign))
}

/// Je to SID lokálního (nebo doménového) uživatelského účtu?
/// Účty v SAM mají autoritu 21; skupiny Windows 32, Entra 12.
fn is_local_account(sid: &str) -> bool {
    sid.starts_with("S-1-5-21-")
}

fn sid_kind(usage: i32) -> String {
    match usage {
        1 => "uživatel".into(),
        2 => "skupina".into(),
        4 => "alias".into(),
        5 => "známá skupina".into(),
        _ => "jiné".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Skupina správců se musí najít i na jinak než anglicky nastavených
    // Windows — jinak by sekce tvrdila, že admina nemá nikdo.
    #[test]
    fn admin_group_is_found_in_system_language() {
        let g = admin_group_name().expect("skupina správců");
        assert!(!g.is_empty());
    }

    // Na každém stroji je aspoň jeden účet a aspoň jeden správce —
    // jinak by se do Windows nedalo přihlásit ani je spravovat.
    #[test]
    fn there_is_at_least_one_account_and_one_admin() {
        let a = accounts().expect("účty");
        assert!(!a.users.is_empty(), "žádný lokální účet");
        let admins = a.users.iter().filter(|u| u.admin).count() + a.foreign_admins.len();
        assert!(admins > 0, "nikdo nemá práva správce");
        // SID musí být čitelný u všech, jinak by párování s adminy
        // fungovalo jen náhodou.
        for u in &a.users {
            assert!(u.sid.starts_with("S-1-"), "účet {} bez SID", u.name);
        }
    }
}
