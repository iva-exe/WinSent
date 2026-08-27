//! Načtení `config.toml` + hot-reload (SPEC kap. 22: vše konfigurovatelné
//! v jednom TOML, hot-reload).
//!
//! Soubor žije v `%ProgramData%\syswatch\config.toml`. Když chybí,
//! založí se s defaulty, aby měl uživatel co editovat. Změna souboru se
//! propíše do sdíleného `Arc<RwLock<Config>>` — bez restartu služby.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use core_types::config::Config;
use notify::Watcher;

/// Chyby konfigurace. Vadný soubor při startu je fatální (radši
/// nespustit než běžet s něčím jiným, než si uživatel myslí).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("nelze číst/zapsat {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("vadné TOML v {path}: {source}")]
    Parse {
        path: PathBuf,
        // Boxed: toml chyba je velká a nafukovala by každý Result v cestě
        // startu démona (clippy::result_large_err).
        source: Box<toml::de::Error>,
    },
    #[error("nelze spustit watcher configu: {0}")]
    Watch(#[from] notify::Error),
}

/// Vzor config.toml zapisovaný při prvním startu.
const DEFAULT_CONFIG_TOML: &str = "\
# syswatch — konfigurace nástroje. Změny se projeví za běhu (hot-reload).

# Interval logu \u{201e}žiju\u{201c} v milisekundách (v0 heartbeat).
heartbeat_ms = 1000

# Interval retenční smyčky v sekundách.
retention_interval_s = 60
";

/// Zapíše do configu nové umístění databáze.
///
/// Prázdný `dir` znamená návrat na výchozí místo. Ověří se, že se do
/// adresáře dá opravdu zapsat — přání, které by při startu služby
/// selhalo, nemá smysl ukládat: uživatel by se o problému dozvěděl až
/// z logu, a to nikdo nečte.
///
/// Soubor se přepisuje celý ze současné podoby configu, protože ostatní
/// hodnoty musí zůstat. Hot-reload watcher si změnu přečte sám.
pub fn set_db_dir(cfg_path: &Path, dir: &str) -> Result<(), Error> {
    let d = dir.trim();
    if !d.is_empty() {
        let p = PathBuf::from(d);
        std::fs::create_dir_all(&p).map_err(|source| Error::Io {
            path: p.clone(),
            source,
        })?;
        // Zápis nanečisto: práva na adresář se z metadat spolehlivě
        // nepoznají, jediné jisté ověření je zkusit to.
        let zkouska = p.join(".winsent-zkouska");
        std::fs::write(&zkouska, b"x").map_err(|source| Error::Io {
            path: zkouska.clone(),
            source,
        })?;
        let _ = std::fs::remove_file(&zkouska);
    }
    let mut cfg = load(cfg_path)?;
    cfg.db_dir = d.to_string();
    let text = format!(
        "\
# syswatch — konfigurace nástroje. Změny se projeví za běhu (hot-reload).

# Interval logu \u{201e}žiju\u{201c} v milisekundách.
heartbeat_ms = {}

# Interval retenční smyčky v sekundách.
retention_interval_s = {}

# Adresář databáze. Prázdno = výchozí %ProgramData%\\syswatch.
# Projeví se až při příštím startu služby.
db_dir = {}
",
        cfg.heartbeat_ms,
        cfg.retention_interval_s,
        toml_string(&cfg.db_dir)
    );
    std::fs::write(cfg_path, text).map_err(|source| Error::Io {
        path: cfg_path.to_path_buf(),
        source,
    })
}

/// Řetězec do TOML. Cesty na Windows nesou zpětná lomítka, takže se
/// použije doslovný literál v apostrofech — jinak by se z `C:\data`
/// stal tabulátor a nová řádka.
fn toml_string(s: &str) -> String {
    if s.contains('\'') {
        // Apostrof v cestě je vzácný, ale doslovný literál ho neumí
        // odescapovat — pak se jde přes základní řetězec.
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{s}'")
    }
}

/// Načte config; když soubor neexistuje, založí ho s defaulty.
pub fn load_or_create(path: &Path) -> Result<Config, Error> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(path, DEFAULT_CONFIG_TOML).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        tracing::info!(path = %path.display(), "založen výchozí config.toml");
    }
    load(path)
}

/// Načte a naparsuje config ze souboru.
fn load(path: &Path) -> Result<Config, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Spustí watcher na config soubor. Vrácený watcher je nutné držet —
/// drop ho zastaví. Vadný soubor při reloadu se jen zaloguje a dál
/// platí poslední dobrá konfigurace (běžící služba se neshodí).
pub fn watch(
    path: &Path,
    shared: Arc<RwLock<Config>>,
) -> Result<notify::RecommendedWatcher, Error> {
    let path_buf = path.to_path_buf();

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if !matches!(
            event.kind,
            notify::EventKind::Modify(_) | notify::EventKind::Create(_)
        ) {
            return;
        }
        // Watch běží na celém adresáři — zajímá nás jen config.toml.
        let is_config = event
            .paths
            .iter()
            .any(|p| p.file_name() == path_buf.file_name());
        if !is_config {
            return;
        }
        // Editory zapisují nadvakrát — krátká pauza, ať čteme celý soubor.
        std::thread::sleep(Duration::from_millis(100));
        match load(&path_buf) {
            Ok(new_cfg) => {
                let mut cfg = shared.write().expect("config lock poisoned");
                if *cfg != new_cfg {
                    tracing::info!(?new_cfg, "config.toml změněn, aplikuji hot-reload");
                    *cfg = new_cfg;
                }
            }
            Err(e) => tracing::error!(error = %e, "reload configu selhal, platí předchozí"),
        }
    })?;

    // Sledujeme celý adresář — editory často soubor nahrazují (rename),
    // watch na samotný soubor by se utrhl.
    let dir = path.parent().unwrap_or(path);
    watcher.watch(dir, notify::RecursiveMode::NonRecursive)?;
    Ok(watcher)
}
