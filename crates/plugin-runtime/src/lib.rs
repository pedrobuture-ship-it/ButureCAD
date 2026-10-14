//! Sandboxed plugin runtime (roadmap stages 32–33).
//!
//! Plugins are **WebAssembly modules run by `wasmi`**, a pure-Rust interpreter: a plugin has no
//! access to the file system, network, environment or the project except through the single
//! host import `buturecad.host_request`, and every operation behind it is checked against the
//! permissions the **user** granted (`filesystem`, `network`, `project-read`, `project-write`).
//! Calls run with a fuel limit (no infinite loops), a memory cap and size caps on messages.
//! No WASI is provided at all.
//!
//! * [`manifest`] — `plugin.json` and its validation.
//! * [`sandbox`] — instantiating a module and calling it (`JSON in → JSON out`).
//! * [`manager`] — install / enable / disable / remove / reload, grants and revocation.
//! * [`provider`] — the ComponentProvider API over the sandbox (roadmap 33).
//!
//! What is *not* implemented is stated where it matters: `network` can be requested, granted and
//! revoked, but this build has no network client behind it; `project-write` is granted but the
//! host refuses direct modification (changes must go through the Command Engine / Agent API
//! proposal flow, which does not exist yet).

pub mod manager;
pub mod manifest;
pub mod provider;
pub mod sandbox;

pub use manager::{PluginManager, PluginStatus};
pub use manifest::{Category, Manifest, Permission};
pub use sandbox::{CallContext, CallOutput, Sandbox};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("invalid plugin manifest: {0}")]
    Manifest(String),
    #[error("cannot load plugin: {0}")]
    Load(String),
    #[error("plugin trapped or misbehaved: {0}")]
    Runtime(String),
    #[error("plugin ran out of its execution budget (possible infinite loop)")]
    OutOfFuel,
    #[error("plugin “{0}” is not installed")]
    NotInstalled(String),
    #[error("plugin “{0}” is disabled")]
    Disabled(String),
    #[error("plugin “{id}” does not declare the {category:?} category")]
    WrongCategory { id: String, category: Category },
    #[error("{0}")]
    Io(String),
}
