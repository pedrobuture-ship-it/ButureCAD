//! Local component, symbol and footprint libraries.
//!
//! Libraries are plain JSON files in folders the user controls (a project's `libraries/`
//! folder, or a per-user folder). Nothing here talks to a network. Files are treated as
//! untrusted input: every symbol is validated on load and on save.
//!
//! Library types (roadmap 09, 12, 31): symbol, footprint, 3D model (manifest) and component
//! libraries, one JSON file per library, plus a local search [`index::Index`] and per-user
//! favorites/recents ([`prefs::Prefs`]). The provider API arrives in roadmap 33.

pub mod component_lib;
pub mod error;
pub mod footprint_lib;
pub mod index;
pub mod model_lib;
pub mod prefs;
pub mod stackup_lib;
pub mod store;
pub mod symbol_lib;

pub use component_lib::{Component, ComponentLibrary, COMPONENT_LIB_EXT};
pub use error::{LibraryError, Result};
pub use footprint_lib::{FootprintLibrary, FOOTPRINT_LIB_EXT};
pub use index::{Entry, Index, Kind, Query};
pub use model_lib::{ModelEntry, ModelLibrary, MODEL_LIB_EXT};
pub use prefs::Prefs;
pub use stackup_lib::{StackupPreset, STACKUP_PRESET_EXT};
pub use store::{LibraryDir, LibrarySummary};
pub use symbol_lib::{qualify, split_lib_id, SymbolLibrary, SYMBOL_LIB_EXT};
