use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("{context} ({path}): {source}")]
    Fs {
        context: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a valid library file: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("{path} uses library schema {found}, newer than the supported {supported}")]
    NewerSchema {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    #[error("library name “{0}” is not valid (use letters, digits, space, - _ . and no path separators)")]
    BadLibraryName(String),
    #[error("library “{0}” does not exist")]
    NoSuchLibrary(String),
    #[error("library “{0}” already exists")]
    LibraryExists(String),
    #[error("“{item}” does not exist in library “{library}”")]
    NoSuchItem { library: String, item: String },
    #[error("invalid {kind} “{name}”: {reason}")]
    InvalidItem {
        kind: &'static str,
        name: String,
        reason: String,
    },
    #[error(transparent)]
    Write(#[from] buturecad_io::IoError),
}

pub type Result<T> = std::result::Result<T, LibraryError>;
