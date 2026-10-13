//! On-disk representation of `project.buture` and its schema versioning.
//!
//! See `docs/file-format/project.md`.

use crate::error::{IoError, Result};
use buturecad_eda_core::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Format tag stored in every project file.
pub const FORMAT_TAG: &str = "buturecad.project";
/// Schema version written by this build.
pub const CURRENT_SCHEMA: u32 = 1;

/// A migration step rewriting a document from version `from` to `from + 1`.
pub type MigrationFn = fn(&mut Value) -> std::result::Result<(), String>;

/// Ordered migration registry. `steps[i]` migrates schema `i + first_version`.
pub struct Migrations {
    first_version: u32,
    steps: Vec<MigrationFn>,
}

impl Migrations {
    /// Production registry. Schema 1 is the first released version, so no steps exist yet.
    pub fn builtin() -> Self {
        Self {
            first_version: CURRENT_SCHEMA,
            steps: Vec::new(),
        }
    }

    /// Custom registry (used by tests and future stages).
    pub fn new(first_version: u32, steps: Vec<MigrationFn>) -> Self {
        Self {
            first_version,
            steps,
        }
    }

    /// Latest version reachable with this registry.
    pub fn target(&self) -> u32 {
        self.first_version + self.steps.len() as u32
    }

    /// Migrates `doc` in place up to [`Self::target`]; returns the version it started at.
    pub fn apply(&self, path: &Path, doc: &mut Value) -> Result<u32> {
        let found = doc
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| IoError::InvalidSchema {
                path: path.into(),
                found: 0,
            })?;
        let version = u32::try_from(found)
            .ok()
            .filter(|v| *v >= self.first_version)
            .ok_or_else(|| IoError::InvalidSchema {
                path: path.into(),
                found,
            })?;
        if version > self.target() {
            return Err(IoError::NewerSchema {
                path: path.into(),
                found: version,
                supported: self.target(),
            });
        }
        for v in version..self.target() {
            let step = self.steps[(v - self.first_version) as usize];
            step(doc).map_err(|reason| IoError::Migration { from: v, reason })?;
            doc["schema_version"] = Value::from(v + 1);
        }
        Ok(version)
    }
}

/// Schema 1 of `project.buture`. Field order is the on-disk order (deterministic output).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFileV1 {
    format: String,
    schema_version: u32,
    name: String,
}

/// Serializes a project to deterministic, pretty-printed JSON (trailing newline).
pub fn serialize_project(project: &Project) -> Vec<u8> {
    let file = ProjectFileV1 {
        format: FORMAT_TAG.to_owned(),
        schema_version: CURRENT_SCHEMA,
        name: project.name().to_owned(),
    };
    // Serializing plain strings and integers cannot fail; fall back to an empty doc defensively.
    let mut out = serde_json::to_vec_pretty(&file).unwrap_or_default();
    out.push(b'\n');
    out
}

/// Parses and migrates a project file with the built-in migrations.
pub fn parse_project(path: &Path, bytes: &[u8]) -> Result<Project> {
    parse_project_with(path, bytes, &Migrations::builtin())
}

/// Parses and migrates a project file with an explicit registry.
pub fn parse_project_with(path: &Path, bytes: &[u8], migrations: &Migrations) -> Result<Project> {
    let parse_err = |source| IoError::Parse {
        path: path.into(),
        source,
    };
    let mut doc: Value = serde_json::from_slice(bytes).map_err(parse_err)?;
    let tag = doc
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if tag != FORMAT_TAG {
        return Err(IoError::WrongFormat {
            path: path.into(),
            found: tag.to_owned(),
        });
    }
    migrations.apply(path, &mut doc)?;
    let file: ProjectFileV1 = serde_json::from_value(doc).map_err(parse_err)?;
    Project::new(file.name).map_err(|source| IoError::Invalid {
        path: path.into(),
        source,
    })
}
