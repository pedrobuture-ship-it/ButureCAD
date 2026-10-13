//! ButureCAD Core: the headless source of truth.
//!
//! Must never depend on React, Tauri, a window system or any AI/network service.

pub mod project;

pub use project::{Project, ProjectError};

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
