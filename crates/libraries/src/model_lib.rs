//! 3D model libraries: a manifest (`*.bmodlib.json`) listing model files. The files themselves
//! stay where the user keeps them (paths are relative to the manifest's folder or absolute);
//! ButureCAD does not parse or copy them (STEP geometry is not implemented, see `docs/mcad.md`).

use crate::error::{LibraryError, Result};
use crate::store::{validate_library_name, LibraryDir, LibrarySummary};
use buturecad_io::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const MODEL_LIB_EXT: &str = "bmodlib.json";
const FORMAT_TAG: &str = "buturecad-model-library";
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    /// File path, relative to the manifest folder or absolute.
    pub path: String,
    #[serde(default)]
    pub description: String,
}

impl ModelEntry {
    /// File format from the extension (`step`, `stp`, `wrl`, …), lower-case; empty if none.
    pub fn format(&self) -> String {
        Path::new(&self.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |r: &str| LibraryError::InvalidItem {
            kind: "3D model",
            name: self.name.clone(),
            reason: r.to_owned(),
        };
        let ctl = |s: &str| s.chars().any(char::is_control);
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || ctl(&self.name)
            || self.name.contains(':')
        {
            return Err(bad("name must be 1–128 printable characters without ':'"));
        }
        if self.path.trim().is_empty() || self.path.len() > 1024 || ctl(&self.path) {
            return Err(bad("the path is empty, too long or has control characters"));
        }
        if self.description.len() > 1024 || ctl(&self.description) {
            return Err(bad("the description is too long or has control characters"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    schema: u32,
    name: String,
    models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelLibrary {
    pub name: String,
    models: BTreeMap<String, ModelEntry>,
}

impl ModelLibrary {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        validate_library_name(&name)?;
        Ok(Self {
            name,
            models: BTreeMap::new(),
        })
    }
    pub fn len(&self) -> usize {
        self.models.len()
    }
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &ModelEntry> {
        self.models.values()
    }
    pub fn insert(&mut self, m: ModelEntry) -> Result<()> {
        m.validate()?;
        self.models.insert(m.name.clone(), m);
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> Result<ModelEntry> {
        self.models
            .remove(name)
            .ok_or_else(|| LibraryError::NoSuchItem {
                library: self.name.clone(),
                item: name.to_owned(),
            })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let f = File {
            format: FORMAT_TAG.into(),
            schema: SCHEMA,
            name: self.name.clone(),
            models: self.models.values().cloned().collect(),
        };
        let mut b = serde_json::to_vec_pretty(&f).map_err(|e| LibraryError::Invalid {
            path: Path::new(&self.name).to_path_buf(),
            reason: e.to_string(),
        })?;
        b.push(b'\n');
        Ok(b)
    }

    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self> {
        let invalid = |reason: String| LibraryError::Invalid {
            path: path.to_path_buf(),
            reason,
        };
        let f: File = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        if f.format != FORMAT_TAG {
            return Err(invalid(format!("format tag is “{}”", f.format)));
        }
        if f.schema > SCHEMA {
            return Err(LibraryError::NewerSchema {
                path: path.to_path_buf(),
                found: f.schema,
                supported: SCHEMA,
            });
        }
        let mut lib = Self::new(f.name).map_err(|e| invalid(e.to_string()))?;
        for m in f.models {
            lib.insert(m)?;
        }
        Ok(lib)
    }
}

impl LibraryDir {
    fn model_path(&self, name: &str) -> Result<std::path::PathBuf> {
        validate_library_name(name)?;
        Ok(self.root().join(format!("{name}.{MODEL_LIB_EXT}")))
    }

    pub fn list_model_libraries(&self) -> Vec<LibrarySummary> {
        crate::store::list_by_suffix(self.root(), MODEL_LIB_EXT, |p, b| {
            ModelLibrary::from_bytes(p, b).map(|l| l.len())
        })
    }

    pub fn load_model_library(&self, name: &str) -> Result<ModelLibrary> {
        let path = self.model_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        let lib = ModelLibrary::from_bytes(&path, &crate::store::read_limited(&path)?)?;
        if lib.name != name {
            return Err(LibraryError::Invalid {
                path,
                reason: format!("file declares library “{}”", lib.name),
            });
        }
        Ok(lib)
    }

    pub fn save_model_library(&self, lib: &ModelLibrary) -> Result<()> {
        let path = self.model_path(&lib.name)?;
        crate::store::ensure_dir(self.root())?;
        Ok(atomic_write(&path, &lib.to_bytes()?)?)
    }

    pub fn save_model(&self, library: &str, m: ModelEntry) -> Result<()> {
        let mut lib = match self.load_model_library(library) {
            Ok(l) => l,
            Err(LibraryError::NoSuchLibrary(_)) => ModelLibrary::new(library)?,
            Err(e) => return Err(e),
        };
        lib.insert(m)?;
        self.save_model_library(&lib)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_format_and_validation() {
        let d = tempfile::tempdir().unwrap();
        let dir = LibraryDir::new(d.path());
        let m = ModelEntry {
            name: "SOIC-8".into(),
            path: "models/SOIC8.STEP".into(),
            description: "x".into(),
        };
        assert_eq!(m.format(), "step");
        dir.save_model("Models", m.clone()).unwrap();
        let lib = dir.load_model_library("Models").unwrap();
        assert_eq!(lib.iter().next(), Some(&m));
        assert_eq!(dir.list_model_libraries()[0].item_count, 1);
        assert!(dir
            .save_model(
                "Models",
                ModelEntry {
                    name: "".into(),
                    ..m.clone()
                }
            )
            .is_err());
        assert!(dir
            .save_model(
                "Models",
                ModelEntry {
                    path: "a\nb".into(),
                    ..m
                }
            )
            .is_err());
    }
}
