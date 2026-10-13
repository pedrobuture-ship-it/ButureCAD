//! ERC and DRC rule engines.
//!
//! **Scope today (roadmap stage 11):** [`erc`], the Electrical Rules Check, which works on the
//! logical connectivity graph (never on drawn geometry). The DRC engine arrives with the PCB
//! stages (roadmap 19–20) and is not implemented.

pub mod erc;

pub use erc::{pin_conflict, run_erc, ErcCounts, ErcReport, Rule, Severity, Violation};
