//! ButureCAD Core: the headless source of truth.
//!
//! Must never depend on React, Tauri, a window system or any AI/network service.
//!
//! All user-level mutations of a [`Project`] go through the [`CommandEngine`].

pub mod command;
pub mod commands;
pub mod copper;
pub mod engine;
pub mod ir;
pub mod ir_apply;
pub mod length;
pub mod model;
pub mod outline;
pub mod pcb_ops;
pub mod project;
pub mod ratsnest;
pub mod routing;
pub mod sync;
pub mod zonefill;

pub use command::{Command, CommandError};
pub use engine::CommandEngine;
pub use project::{Project, ProjectError, ROOT_SHEET};

/// Product name.
pub const NAME: &str = "ButureCAD";

/// Core version (workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_set() {
        assert_eq!(NAME, "ButureCAD");
        assert!(!VERSION.is_empty());
    }
}
