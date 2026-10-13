//! Footprint libraries: one JSON file holding any number of footprints.
//!
//! Inside a library a footprint is stored under its *unqualified* name; once placed in a project
//! its name is the library-qualified id (`Package_SO:SOIC-8_3.9mm_P1.27mm`).

use crate::error::{LibraryError, Result};
use crate::symbol_lib::{qualify, split_lib_id};
use buturecad_pcb::{validate, Footprint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FOOTPRINT_LIB_EXT: &str = "bfplib.json";
const FORMAT_TAG: &str = "buturecad-footprint-library";
pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    schema: u32,
    name: String,
    footprints: Vec<Footprint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FootprintLibrary {
    pub name: String,
    footprints: BTreeMap<String, Footprint>,
}

fn invalid(name: &str, reason: impl ToString) -> LibraryError {
    LibraryError::InvalidItem {
        kind: "footprint",
        name: name.to_owned(),
        reason: reason.to_string(),
    }
}

impl FootprintLibrary {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        crate::store::validate_library_name(&name)?;
        Ok(Self {
            name,
            footprints: BTreeMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.footprints.len()
    }
    pub fn is_empty(&self) -> bool {
        self.footprints.is_empty()
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.footprints.keys().map(String::as_str)
    }
    pub fn get(&self, name: &str) -> Option<&Footprint> {
        self.footprints.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Footprint> {
        self.footprints.values()
    }

    /// Adds or replaces a footprint (stored unqualified). It must validate.
    pub fn insert(&mut self, mut fp: Footprint) -> Result<()> {
        if let Some((_, short)) = split_lib_id(&fp.name) {
            fp.name = short.to_owned();
        }
        if fp.name.contains(':') {
            return Err(invalid(
                &fp.name,
                "library footprint names cannot contain “:”",
            ));
        }
        validate(&fp).map_err(|e| invalid(&fp.name, e))?;
        self.footprints.insert(fp.name.clone(), fp);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Footprint> {
        self.footprints
            .remove(name)
            .ok_or_else(|| LibraryError::NoSuchItem {
                library: self.name.clone(),
                item: name.to_owned(),
            })
    }

    /// A copy ready to embed in a project (its name is the qualified id).
    pub fn instantiate(&self, name: &str) -> Option<Footprint> {
        let mut f = self.footprints.get(name)?.clone();
        f.name = qualify(&self.name, name);
        Some(f)
    }

    /// Deterministic file content (footprints sorted by name).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let file = File {
            format: FORMAT_TAG.into(),
            schema: SCHEMA,
            name: self.name.clone(),
            footprints: self.footprints.values().cloned().collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&file).map_err(|e| LibraryError::Invalid {
            path: Path::new(&self.name).to_path_buf(),
            reason: e.to_string(),
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self> {
        let bad = |reason: String| LibraryError::Invalid {
            path: path.to_path_buf(),
            reason,
        };
        let file: File = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
        if file.format != FORMAT_TAG {
            return Err(bad(format!("format tag is “{}”", file.format)));
        }
        if file.schema > SCHEMA {
            return Err(LibraryError::NewerSchema {
                path: path.to_path_buf(),
                found: file.schema,
                supported: SCHEMA,
            });
        }
        let mut lib = Self::new(file.name).map_err(|e| bad(e.to_string()))?;
        for fp in file.footprints {
            let before = lib.footprints.len();
            let name = fp.name.clone();
            lib.insert(fp).map_err(|e| bad(e.to_string()))?;
            if lib.footprints.len() == before {
                return Err(bad(format!("footprint “{name}” appears twice")));
            }
        }
        Ok(lib)
    }
}
