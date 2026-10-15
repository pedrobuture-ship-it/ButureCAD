//! Manufacturing outputs: Gerber (RS-274X), Excellon drill, BOM, pick and place, assembly
//! information and `production.zip`, plus the manufacturing preflight.
//!
//! The crate is **decoupled from the PCB core** (like the 3D viewer): its input is [`FabInput`],
//! plain integer-nanometre geometry built from the real board by the desktop bridge. Output is
//! deterministic (no timestamps), so the same board always yields byte-identical files.
//!
//! Nothing here "fixes" a board: [`preflight`] reports errors (which block `production.zip`),
//! warnings and notes. A successful export is **not** a statement that the board is ready to be
//! fabricated.

pub mod board;
pub mod excellon;
pub mod excellon_read;
pub mod font;
pub mod gerber;
pub mod gerber_read;
pub mod model;
pub mod package;
pub mod preflight;
pub mod raster2d;
pub mod tables;
pub mod verify;
pub mod zip;

pub use board::{build_fab_input, build_fab_input_for_project, build_fab_input_with, BuildOptions};
pub use model::{
    Aperture, Capabilities, DrcSummary, FabInput, Feature, Hole, Layer, Mount, Part, PartFields, Pt,
};
pub use package::{generate, FilePreview, Generated, ProductionFile};
pub use preflight::{Issue, Report, Severity};

#[derive(Debug, thiserror::Error)]
pub enum FabError {
    #[error("invalid manufacturing data: {0}")]
    Invalid(String),
    #[error("export blocked by {0} preflight error(s)")]
    Blocked(usize),
}

/// Nanometres → `[-]mm.dddddd`, exact (integer arithmetic only).
pub fn fmt_mm(nm: i64) -> String {
    let sign = if nm < 0 { "-" } else { "" };
    let a = nm.unsigned_abs();
    format!("{sign}{}.{:06}", a / 1_000_000, a % 1_000_000)
}
