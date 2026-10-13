//! The command abstraction.

use crate::model::{Id, ModelError};
use crate::project::ProjectError;
use crate::Project;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum CommandError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error("{kind} {id:?} does not exist")]
    MissingEntity { kind: &'static str, id: Id },
    #[error("{kind} {id:?} already exists")]
    IdInUse { kind: &'static str, id: Id },
    #[error("operation not allowed while a transaction is open")]
    TransactionOpen,
    #[error("no transaction is open")]
    NoTransaction,
    #[error("history is inconsistent with the project ({0}); history was cleared")]
    HistoryCorrupted(String),
}

/// A reversible change to the [`Project`].
///
/// Contract (relied on by the engine):
/// * `apply` is **atomic**: on `Err` the project is left unchanged.
/// * `revert` is only called after a successful `apply`, in strict LIFO order, and restores the
///   exact previous state (including ids).
/// * `apply` may be called again after `revert` (redo) and must reproduce the same result.
pub trait Command: std::fmt::Debug {
    /// Short human-readable description for history menus and AI change review.
    fn label(&self) -> String;
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError>;
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError>;
}
