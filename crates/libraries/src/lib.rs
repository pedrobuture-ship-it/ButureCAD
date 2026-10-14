//! Local component, symbol and footprint libraries.
//!
//! Libraries are plain JSON files in folders the user controls (a project's `libraries/`
//! folder, or a per-user folder). Nothing here talks to a network. Files are treated as
//! untrusted input: every symbol is validated on load and on save.
//!
//! **Scope today (roadmap stages 09 and 12):** symbol and footprint libraries, read and written one
//! library per file. Component records tying symbol + footprint + 3D model +
//! manufacturer data, library search and the provider API arrive in roadmap stages 31–33.

pub mod error;
pub mod footprint_lib;
pub mod stackup_lib;
pub mod store;
pub mod symbol_lib;

pub use error::{LibraryError, Result};
pub use footprint_lib::{FootprintLibrary, FOOTPRINT_LIB_EXT};
pub use stackup_lib::{StackupPreset, STACKUP_PRESET_EXT};
pub use store::{LibraryDir, LibrarySummary};
pub use symbol_lib::{qualify, split_lib_id, SymbolLibrary, SYMBOL_LIB_EXT};
