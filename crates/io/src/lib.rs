//! Project file I/O: format, atomic save, migrations, recovery.
//!
//! Treats project files as untrusted input: malformed files yield [`IoError`], never a panic.

pub mod atomic;
pub mod engine;
pub mod error;
pub mod format;
pub mod import;
pub mod session;

pub use error::{IoError, Result};
pub use session::{Opened, ProjectSession, Recovery};
