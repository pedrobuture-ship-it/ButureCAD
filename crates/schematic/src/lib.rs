//! Schematic model and logical connectivity.
//!
//! Strict separation (see `docs/ARCHITECTURE.md`):
//!
//! * [`model`] — plain data (symbols, components, wires, labels, sheets).
//! * [`schematic`] — the container, validation and reversible [`Change`] sets.
//! * [`connectivity`] — electrical connectivity derived from the model, never from pixels.
//! * [`builtin`] — a small bootstrap symbol library (to be superseded by the libraries crate).
//!
//! Rendering and tools live outside this crate.

pub mod builtin;
pub mod bus;
pub mod connectivity;
pub mod model;
pub mod schematic;
pub mod symbol_edit;

pub use connectivity::{
    Connectivity, ConnectivityEngine, DanglingEnd, Issue, Net, NetId, PinRef, UpdateStats,
};
pub use model::*;
pub use schematic::{Change, PlacedPin, Schematic, SchematicError};
pub mod edit;
