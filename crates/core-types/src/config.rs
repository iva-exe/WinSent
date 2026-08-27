//! Konfigurace nástroje — jediný `config.toml`, hot-reload (SPEC kap. 22).
//!
//! v0 drží jen minimum: interval heartbeatu a interval retenční smyčky.
//! Další pole přibývají s kolektory (v1+). Všechna pole mají defaulty,
//! takže prázdný nebo chybějící soubor je validní konfigurace.

use serde::Deserialize;

/// Kořen `config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Interval logu „žiju“ v milisekundách (v0 heartbeat).
    pub heartbeat_ms: u64,
    /// Interval retenční smyčky v sekundách (v0 běží naprázdno).
    pub retention_interval_s: u64,
    /// Adresář databáze. Prázdno = výchozí `%ProgramData%\syswatch`.
    ///
    /// Existuje kvůli systémovým SSD: databáze roste do stovek megabajtů
    /// a na stroji, kde je systémový disk malý nebo opotřebovaný, ji jde
    /// odsunout jinam. Výchozí umístění se nemění — přesouvá se jen
    /// tehdy, když si o to uživatel řekne.
    ///
    /// Projeví se až při příštím startu služby; databáze je otevřená
    /// a stěhovat ji pod rukama by znamenalo přijít o rozepsaný WAL.
    pub db_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            heartbeat_ms: 1000,
            retention_interval_s: 60,
            db_dir: String::new(),
        }
    }
}
