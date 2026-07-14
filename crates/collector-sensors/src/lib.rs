//! collector-sensors — GPU teploty (NVML/ADLX/IGCL), CPU kaskáda, FPS/frame time (SPEC kap. 15.2–15.3). Naplní se ve v3/v9.
//!
//! v0: prázdný stub, jen tvar rozhraní a závislosti.

use core_types::config::Config;

/// Chyby této crate. Varianty přibudou s implementací.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

/// Stav kolektoru mezi ticky. v0: prázdný nosič.
pub struct State;

/// Inicializace kolektoru při startu služby.
pub fn init(_cfg: &Config) -> Result<State, Error> {
    Ok(State)
}

/// Jeden krok sběru. Ve v1+ dostane i RingWriter pro výstup vzorků.
pub fn tick(_state: &mut State) -> Result<(), Error> {
    Ok(())
}

/// Korektní ukončení kolektoru.
pub fn shutdown(_state: State) {}
