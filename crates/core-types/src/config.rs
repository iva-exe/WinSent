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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            heartbeat_ms: 1000,
            retention_interval_s: 60,
        }
    }
}
