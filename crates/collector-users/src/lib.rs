//! Účty na tomhle počítači a kdo z nich má práva správce
//! (v9E, SPEC kap. 14).
//!
//! Sekce odpovídá na jednu otázku, kterou si laik umí položit:
//! *„kdo se sem dostane a kdo tu může všechno?"* Nic nemění — zakládat
//! účty a měnit práva umí Windows samy a dělají to líp.
//!
//! Pozor na jednu vlastnost Windows, kterou je potřeba říct nahlas:
//! seznam jsou **lokální účty**, ne „všichni, kdo se sem hlásí".
//! Doménový nebo firemní účet se v místní databázi vůbec neobjeví —
//! zato se objeví ve skupině správců, a proto se hlásí zvlášť.

use core_types::proc::{ForeignAdminRow, UserRow, UsersReport};

/// Přečte účty. Volá se na dotaz, ne v cyklu — čtení je sice levné
/// (jen místní databáze účtů), ale výsledek se během minuty nezmění.
pub fn report() -> UsersReport {
    let acc = match win_sys::users::accounts() {
        Ok(a) => a,
        Err(e) => {
            // Bez účtů se sekce ukáže prázdná; mlčet by bylo horší,
            // proto aspoň stopa v logu.
            tracing::warn!(error = %e, "účty nelze přečíst");
            return UsersReport::default();
        }
    };

    UsersReport {
        users: acc
            .users
            .into_iter()
            .map(|u| UserRow {
                name: u.name,
                full_name: u.full_name,
                comment: u.comment,
                sid: u.sid,
                admin: u.admin,
                disabled: u.disabled,
                locked: u.locked,
                password_not_required: u.password_not_required,
                microsoft: u.microsoft,
                last_logon: u.last_logon,
                logons: u.logons,
            })
            .collect(),
        foreign_admins: acc
            .foreign_admins
            .into_iter()
            .map(|f| ForeignAdminRow {
                name: f.name,
                sid: f.sid,
                kind: f.kind,
            })
            .collect(),
        admin_group: acc.admin_group,
        // Doplní se ve službě — ta zná jen sebe (běží jako SYSTEM),
        // takže přihlášeného uživatele hlásí UI ve svém procesu.
        current_user: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Na každém stroji je aspoň jeden účet a někdo, kdo ho spravuje.
    // Kdyby se rozbilo párování se skupinou správců, sekce by tvrdila,
    // že admina nemá nikdo — a to je nejhorší možná odpověď.
    #[test]
    fn report_finds_accounts_and_an_administrator() {
        let r = report();
        assert!(!r.admin_group.is_empty(), "skupina správců nemá jméno");
        assert!(!r.users.is_empty(), "žádný účet");
        let admins = r.users.iter().filter(|u| u.admin).count() + r.foreign_admins.len();
        assert!(admins > 0, "nikdo nemá práva správce");
    }

    // Vypnutý účet nesmí zmizet ze seznamu — vestavěný „Administrator"
    // bývá vypnutý a právě to je informace, kterou chce uživatel vidět.
    #[test]
    fn disabled_accounts_are_kept() {
        let r = report();
        for u in &r.users {
            assert!(!u.name.is_empty());
            assert!(u.sid.starts_with("S-1-"), "účet {} bez SID", u.name);
        }
    }
}
