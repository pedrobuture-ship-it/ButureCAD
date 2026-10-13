use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("{context} ({path}): {source}")]
    Fs {
        context: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid JSON or has an unexpected structure: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{path} is not a ButureCAD project file (format tag {found:?})")]
    WrongFormat { path: PathBuf, found: String },
    #[error("{path} uses schema version {found}, newer than the supported version {supported}; update ButureCAD")]
    NewerSchema {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    #[error("{path} has invalid schema version {found}")]
    InvalidSchema { path: PathBuf, found: u64 },
    #[error("migration from schema version {from} failed: {reason}")]
    Migration { from: u32, reason: String },
    #[error("a project already exists at {0}")]
    AlreadyExists(PathBuf),
    #[error("no project file found at {0}")]
    NotFound(PathBuf),
    #[error("invalid project data in {path}: {source}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: buturecad_eda_core::ProjectError,
    },
    #[error(transparent)]
    Command(#[from] buturecad_eda_core::CommandError),
    #[error("refusing to copy symbolic link {0}")]
    Symlink(PathBuf),
}

impl IoError {
    pub(crate) fn fs(
        context: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Fs {
            context,
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, IoError>;
