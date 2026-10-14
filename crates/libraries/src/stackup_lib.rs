//! Layer stack presets: a named [`PhysicalStack`] saved as a file the user controls
//! (`<name>.bstack.json`) so the same stackup can be applied to any board.

use crate::error::{LibraryError, Result};
use buturecad_pcb::{PhysicalStack, Stackup};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const STACKUP_PRESET_EXT: &str = "bstack.json";
const FORMAT_TAG: &str = "buturecad.stackup-preset";
const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    schema_version: u32,
    name: String,
    copper_layers: u8,
    stack: PhysicalStack,
}

/// A saved layer stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackupPreset {
    pub name: String,
    pub copper_layers: u8,
    pub stack: PhysicalStack,
}

impl StackupPreset {
    pub fn new(name: impl Into<String>, copper_layers: u8, stack: PhysicalStack) -> Result<Self> {
        let name = name.into();
        let p = Self {
            name,
            copper_layers,
            stack,
        };
        p.validate(Path::new(&p.name))?;
        Ok(p)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        let bad = |reason: String| LibraryError::Invalid {
            path: path.to_path_buf(),
            reason,
        };
        let st = Stackup {
            copper_layers: self.copper_layers,
        };
        if !st.is_valid() {
            return Err(bad(
                "copper layer count must be an even number from 2 to 32".into(),
            ));
        }
        self.stack.validate(&st).map_err(|e| bad(e.to_string()))
    }

    /// Deterministic file content.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let file = File {
            format: FORMAT_TAG.into(),
            schema_version: SCHEMA,
            name: self.name.clone(),
            copper_layers: self.copper_layers,
            stack: self.stack.clone(),
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
        if file.schema_version > SCHEMA {
            return Err(LibraryError::NewerSchema {
                path: path.to_path_buf(),
                found: file.schema_version,
                supported: SCHEMA,
            });
        }
        let p = Self {
            name: file.name,
            copper_layers: file.copper_layers,
            stack: file.stack,
        };
        p.validate(path)?;
        Ok(p)
    }
}
