//! ERC and DRC rule engines.
//!
//! * [`erc`] — Electrical Rules Check on the schematic connectivity graph (roadmap 11);
//! * [`drc`] — Design Rule Checking on the real board geometry (roadmap 19), built on
//!   [`copper`], the shared copper-shape geometry.

pub use buturecad_eda_core::copper;
pub mod drc;
pub mod erc;

pub use buturecad_eda_core::outline::outline_segments;
pub use drc::{
    candidate_edge_violations, check_candidate, run_drc, DrcEngine, DrcReport, DrcRule,
    DrcSeverity, DrcViolation,
};
pub use erc::{pin_conflict, run_erc, ErcCounts, ErcReport, Rule, Severity, Violation};
