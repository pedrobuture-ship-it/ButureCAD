//! Component libraries: records that tie a symbol, a footprint and a 3D model to manufacturer
//! data. One JSON file (`*.bcomplib.json`) holds any number of components.
//!
//! A component only *references* the symbol (`Library:Name`), footprint (`Library:Name`) and model
//! (a path or `ModelLibrary:Name`); whether the references resolve is checked by the
//! [`crate::index::Index`]. The datasheet is a stored string (path or URL): nothing here fetches it.

use crate::error::{LibraryError, Result};
use crate::store::{validate_library_name, LibraryDir, LibrarySummary};
use buturecad_io::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const COMPONENT_LIB_EXT: &str = "bcomplib.json";
const FORMAT_TAG: &str = "buturecad-component-library";
pub const SCHEMA: u32 = 1;
const MAX_FIELD: usize = 1024;
const MAX_PARAMS: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Symbol id `Library:Name` (empty = none).
    #[serde(default)]
    pub symbol: String,
    /// Footprint id `Library:Name` (empty = none).
    #[serde(default)]
    pub footprint: String,
    /// 3D model: a file path or `ModelLibrary:Name` (empty = none).
    #[serde(default)]
    pub model3d: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub mpn: String,
    /// Path or URL, stored only.
    #[serde(default)]
    pub datasheet: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn bad(name: &str, reason: impl ToString) -> LibraryError {
    LibraryError::InvalidItem {
        kind: "component",
        name: name.to_owned(),
        reason: reason.to_string(),
    }
}

fn clean(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl Component {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        let n = &self.name;
        if n.trim().is_empty() || n.len() > 128 || !clean(n) || n.contains(':') {
            return Err(bad(
                n,
                "name must be 1–128 printable characters without ':'",
            ));
        }
        for (what, v) in [
            ("description", &self.description),
            ("symbol", &self.symbol),
            ("footprint", &self.footprint),
            ("model3d", &self.model3d),
            ("manufacturer", &self.manufacturer),
            ("mpn", &self.mpn),
            ("datasheet", &self.datasheet),
        ] {
            if v.len() > MAX_FIELD || !clean(v) {
                return Err(bad(
                    n,
                    format!("{what} is too long or has control characters"),
                ));
            }
        }
        for (what, v) in [("symbol", &self.symbol), ("footprint", &self.footprint)] {
            if !v.is_empty() && !v.contains(':') {
                return Err(bad(
                    n,
                    format!("{what} must be a qualified id (Library:Name)"),
                ));
            }
        }
        if self.parameters.len() > MAX_PARAMS {
            return Err(bad(n, format!("more than {MAX_PARAMS} parameters")));
        }
        for (k, v) in &self.parameters {
            if k.trim().is_empty() || k.len() > 64 || v.len() > MAX_FIELD || !clean(k) || !clean(v)
            {
                return Err(bad(n, format!("parameter “{k}” is not valid")));
            }
        }
        if self.tags.len() > 64 || self.tags.iter().any(|t| t.len() > 64 || !clean(t)) {
            return Err(bad(n, "tags are not valid"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    schema: u32,
    name: String,
    components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentLibrary {
    pub name: String,
    components: BTreeMap<String, Component>,
}

impl ComponentLibrary {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        validate_library_name(&name)?;
        Ok(Self {
            name,
            components: BTreeMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.components.len()
    }
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }
    pub fn get(&self, name: &str) -> Option<&Component> {
        self.components.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Component> {
        self.components.values()
    }

    /// Inserts or replaces (validated).
    pub fn insert(&mut self, c: Component) -> Result<()> {
        c.validate()?;
        self.components.insert(c.name.clone(), c);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Component> {
        self.components
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
            components: self.components.values().cloned().collect(),
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
        for c in f.components {
            lib.insert(c)?;
        }
        Ok(lib)
    }
}

impl LibraryDir {
    fn component_path(&self, name: &str) -> Result<std::path::PathBuf> {
        validate_library_name(name)?;
        Ok(self.root().join(format!("{name}.{COMPONENT_LIB_EXT}")))
    }

    pub fn list_component_libraries(&self) -> Vec<LibrarySummary> {
        crate::store::list_by_suffix(self.root(), COMPONENT_LIB_EXT, |p, b| {
            ComponentLibrary::from_bytes(p, b).map(|l| l.len())
        })
    }

    pub fn load_component_library(&self, name: &str) -> Result<ComponentLibrary> {
        let path = self.component_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        let lib = ComponentLibrary::from_bytes(&path, &crate::store::read_limited(&path)?)?;
        if lib.name != name {
            return Err(LibraryError::Invalid {
                path,
                reason: format!("file declares library “{}”", lib.name),
            });
        }
        Ok(lib)
    }

    pub fn save_component_library(&self, lib: &ComponentLibrary) -> Result<()> {
        let path = self.component_path(&lib.name)?;
        crate::store::ensure_dir(self.root())?;
        Ok(atomic_write(&path, &lib.to_bytes()?)?)
    }

    pub fn create_component_library(&self, name: &str) -> Result<ComponentLibrary> {
        if self.component_path(name)?.exists() {
            return Err(LibraryError::LibraryExists(name.to_owned()));
        }
        let lib = ComponentLibrary::new(name)?;
        self.save_component_library(&lib)?;
        Ok(lib)
    }

    /// Adds or replaces a component, creating the library when it does not exist.
    pub fn save_component(&self, library: &str, c: Component) -> Result<()> {
        let mut lib = match self.load_component_library(library) {
            Ok(l) => l,
            Err(LibraryError::NoSuchLibrary(_)) => ComponentLibrary::new(library)?,
            Err(e) => return Err(e),
        };
        lib.insert(c)?;
        self.save_component_library(&lib)
    }

    pub fn delete_component(&self, library: &str, name: &str) -> Result<()> {
        let mut lib = self.load_component_library(library)?;
        lib.remove(name)?;
        self.save_component_library(&lib)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample() -> Component {
        let mut c = Component::new("NE555");
        c.symbol = "Buture:NE555".into();
        c.footprint = "Package_SO:SOIC-8".into();
        c.manufacturer = "TI".into();
        c.mpn = "NE555DR".into();
        c.datasheet = "https://example.com/ne555.pdf".into();
        c.parameters.insert("Vcc".into(), "4.5–16 V".into());
        c.tags = vec!["timer".into()];
        c
    }

    #[test]
    fn round_trip_and_atomic_save() {
        let d = tempfile::tempdir().unwrap();
        let dir = LibraryDir::new(d.path().join("libs"));
        dir.save_component("Parts", sample()).unwrap();
        let lib = dir.load_component_library("Parts").unwrap();
        assert_eq!(lib.get("NE555"), Some(&sample()));
        assert_eq!(lib.to_bytes().unwrap(), lib.to_bytes().unwrap());
        let list = dir.list_component_libraries();
        assert_eq!(
            (list.len(), list[0].item_count, list[0].error.is_none()),
            (1, 1, true)
        );
        dir.delete_component("Parts", "NE555").unwrap();
        assert!(dir.load_component_library("Parts").unwrap().is_empty());
        assert!(dir.create_component_library("Parts").is_err());
    }

    #[test]
    fn invalid_components_and_files_are_rejected() {
        let mut c = sample();
        c.footprint = "NoLibrary".into();
        assert!(c.validate().is_err());
        let mut c = sample();
        c.name = "a:b".into();
        assert!(c.validate().is_err());
        let mut c = sample();
        c.mpn = "x\u{7}".into();
        assert!(c.validate().is_err());
        let mut c = sample();
        for i in 0..70 {
            c.parameters.insert(format!("p{i}"), "1".into());
        }
        assert!(c.validate().is_err());
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Bad.bcomplib.json"), b"{nope").unwrap();
        let dir = LibraryDir::new(d.path());
        let l = dir.list_component_libraries();
        assert!(
            l[0].error.is_some(),
            "a broken library is listed with its error"
        );
        assert!(dir.load_component_library("Bad").is_err());
        assert!(dir.load_component_library("../x").is_err());
    }
}
