//! Mechanical (MCAD / STEP) exchange: an **abstraction layer** between ButureCAD and whatever
//! library or format carries mechanical data.
//!
//! The PCB core never sees a STEP library. The desktop bridge converts the board into the neutral
//! [`MechanicalModel`] (board outline, mounting holes, component geometry, mechanical constraints;
//! integer nanometres like the core) and a [`McadBackend`] turns that model into bytes and back.
//! Backends are looked up in a [`Registry`] by file extension and declare honestly what they
//! support ([`Support`]). Today:
//!
//! * [`NeutralBackend`] (`.bmech`) — a real, deterministic JSON round trip of the neutral model.
//! * [`StepBackend`] (`.step`/`.stp`) — parses the ISO 10303-21 exchange structure
//!   ([`part21`]) and reports what the file contains, but does **not** interpret or write STEP
//!   geometry yet: import/export return [`McadError::Unsupported`]. No geometry kernel is linked;
//!   see `docs/mcad.md` for the licence analysis.

pub mod backend;
pub mod model;
pub mod neutral;
pub mod part21;
pub mod step;

pub use backend::{BackendInfo, McadBackend, McadError, Registry, Support};
pub use model::{
    Body, Component, Constraint, MechanicalModel, ModelRef, MountingHole, Pt, Side, SCHEMA_VERSION,
};
pub use neutral::NeutralBackend;
pub use step::{StepBackend, StepSummary};
