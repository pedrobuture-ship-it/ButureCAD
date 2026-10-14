//! Circuit simulation architecture (roadmap stage 34).
//!
//! The schematic is turned into a plain [`Circuit`]; a [`Backend`] simulates it for one
//! [`Analysis`] (operating point, DC sweep, AC, transient) and returns a [`SimResult`] that the UI
//! plots. Two backends exist:
//!
//! * [`BuiltinBackend`] — an in-house modified-nodal-analysis engine for **linear** circuits
//!   (R, C, L, independent V/I sources with DC, AC, SIN and PULSE). Deterministic, offline, no
//!   dependency. Diodes, transistors and subcircuits are rejected explicitly, never approximated.
//! * [`NgspiceBackend`] — runs the user's installed `ngspice` as a **separate process** (batch
//!   mode, ASCII raw file) for everything else. Process separation keeps ngspice's licence out of
//!   ButureCAD; nothing is bundled. See `docs/simulation.md`.
//!
//! No SPICE solver is written from scratch: the built-in engine covers the linear subset only and
//! says so.

pub mod builtin;
pub mod circuit;
pub mod ngspice;
pub mod result;

pub use builtin::BuiltinBackend;
pub use circuit::{Analysis, Circuit, Element, Kind, SourceSpec, Value, Wave};
pub use ngspice::NgspiceBackend;
pub use result::{Series, SimResult};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SimError {
    #[error("invalid circuit: {0}")]
    Circuit(String),
    #[error("{backend} cannot simulate this: {reason}")]
    Unsupported {
        backend: &'static str,
        reason: String,
    },
    #[error("simulation failed: {0}")]
    Failed(String),
    #[error("{0} is not available on this machine")]
    Unavailable(&'static str),
}

pub trait Backend {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Can this backend run on this machine right now?
    fn available(&self) -> bool;
    fn run(&self, circuit: &Circuit, analysis: &Analysis) -> Result<SimResult, SimError>;
}

/// All backends in order of preference.
pub fn backends() -> Vec<Box<dyn Backend>> {
    vec![Box::new(BuiltinBackend), Box::new(NgspiceBackend::detect())]
}
