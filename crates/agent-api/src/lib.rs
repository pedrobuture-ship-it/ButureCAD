//! The ButureCAD **Agent API** (roadmap stage 40).
//!
//! It lets an AI agent — or any external tool — understand and propose changes to a project
//! **without ever modifying it**:
//!
//! ```text
//! Agent ─▶ AgentSurface (read · analyze · propose)
//!             └▶ Proposed Changes (structured Actions)
//!                  └▶ Validation on a COPY of the project (diff, ERC/DRC before→after)
//!                       └▶ Diff + findings shown to the user
//!                            └▶ User approval (host-only: `Reviewer`)
//!                                 └▶ Command Engine (one undoable transaction)
//! ```
//!
//! * [`AgentSurface`] is everything an agent may call. It has no method that changes a project.
//! * [`Reviewer`] (apply / reject) is host-side and is **not** part of the agent surface; the
//!   JSON-RPC protocol (stage 42) exposes only [`AgentSurface`].
//! * Every call — reads included — is recorded in an [`AuditLog`] as structured, serialisable
//!   entries.
//! * Applying re-checks that the project is still at the revision that was validated and that the
//!   result equals the validated copy exactly; otherwise nothing is kept.
//!
//! Agents never touch files: they see JSON views of the Core ([`read`]), run analyses
//! ([`analysis`]) and submit [`Action`]s. The API contains no AI model and no network code.

pub mod action;
pub mod analysis;
pub mod api;
pub mod apply;
pub mod audit;
pub mod diff;
pub mod protocol;
pub mod provider;
pub mod read;

pub use action::{Action, PinSpec};
pub use api::{AgentApi, AgentSurface, ApiError, Proposal, ProposalStatus, Reviewer, Validation};
pub use audit::{AuditEntry, AuditLog};
pub use diff::{Diff, DiffEntry};
pub use protocol::{serve, tools, Server, PROTOCOL};
pub use provider::{
    builtin_providers, AgentProvider, DecouplingHelper, JsonProvider, ProposalDraft, ProviderKind,
};
pub use read::fingerprint;

/// Resolves a footprint name (`0402`, `soic8`, a library id) to a definition, or `None`.
pub type FootprintResolver = Box<dyn Fn(&str) -> Option<buturecad_pcb::Footprint> + Send + Sync>;
