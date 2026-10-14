//! The backend contract and the registry.

use crate::model::MechanicalModel;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum McadError {
    #[error("invalid mechanical model: {0}")]
    Invalid(String),
    #[error("malformed {format} data: {detail}")]
    Malformed {
        format: &'static str,
        detail: String,
    },
    #[error("{backend}: {what} is not supported yet ({detail})")]
    Unsupported {
        backend: &'static str,
        what: &'static str,
        detail: String,
    },
    #[error("unsupported schema version {found} (this build reads {supported})")]
    SchemaVersion { found: u32, supported: u32 },
    #[error("no backend handles the extension '{0}'")]
    NoBackend(String),
}

/// How much of a direction a backend implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Full,
    Partial,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Lower-case extensions without the dot.
    pub extensions: &'static [&'static str],
    /// Licence of the code that does the work (for the dependency record).
    pub license: &'static str,
    pub import: Support,
    pub export: Support,
    pub notes: &'static str,
}

pub trait McadBackend {
    fn info(&self) -> BackendInfo;
    fn import(&self, bytes: &[u8]) -> Result<MechanicalModel, McadError>;
    fn export(&self, model: &MechanicalModel) -> Result<Vec<u8>, McadError>;
}

#[derive(Default)]
pub struct Registry {
    backends: Vec<Box<dyn McadBackend>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The backends shipped with ButureCAD.
    pub fn with_builtin() -> Self {
        let mut r = Self::new();
        r.register(Box::new(crate::NeutralBackend));
        r.register(Box::new(crate::StepBackend));
        r
    }

    pub fn register(&mut self, backend: Box<dyn McadBackend>) {
        self.backends.push(backend);
    }

    pub fn infos(&self) -> Vec<BackendInfo> {
        self.backends.iter().map(|b| b.info()).collect()
    }

    pub fn by_extension(&self, ext: &str) -> Result<&dyn McadBackend, McadError> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        self.backends
            .iter()
            .find(|b| b.info().extensions.contains(&ext.as_str()))
            .map(|b| b.as_ref())
            .ok_or(McadError::NoBackend(ext))
    }

    /// Extension → backend id, for UI listings.
    pub fn extension_map(&self) -> BTreeMap<&'static str, &'static str> {
        let mut m = BTreeMap::new();
        for b in &self.backends {
            let i = b.info();
            for e in i.extensions {
                m.insert(*e, i.id);
            }
        }
        m
    }
}
