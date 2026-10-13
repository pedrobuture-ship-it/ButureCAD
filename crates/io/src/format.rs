//! On-disk representation of the project files and their schema versioning.
//!
//! See `docs/file-format/`.

use crate::error::{IoError, Result};
use buturecad_eda_core::model::{Component, Id, Track, Via, Wire};
use buturecad_eda_core::model::{PcbDoc, SchematicDoc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const FORMAT_TAG: &str = "buturecad.project";
pub const SCHEMATIC_TAG: &str = "buturecad.schematic";
pub const PCB_TAG: &str = "buturecad.pcb";
/// Schema version written by this build (all file kinds start at 1).
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

/// Parses JSON, checks the format tag and migrates to the registry's target version.
fn parse_migrated<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
    tag: &str,
    migrations: &Migrations,
) -> Result<T> {
    let parse_err = |source| IoError::Parse {
        path: path.into(),
        source,
    };
    let mut doc: Value = serde_json::from_slice(bytes).map_err(parse_err)?;
    let found = doc
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if found != tag {
        return Err(IoError::WrongFormat {
            path: path.into(),
            found: found.to_owned(),
        });
    }
    migrations.apply(path, &mut doc)?;
    serde_json::from_value(doc).map_err(parse_err)
}

/// Deterministic pretty JSON with a trailing newline. Serializing plain data cannot fail; an
/// empty buffer (which would fail validation on load) is the defensive fallback.
fn to_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = serde_json::to_vec_pretty(value).unwrap_or_default();
    out.push(b'\n');
    out
}

/// Field order is the on-disk order (deterministic output).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestV1 {
    format: String,
    schema_version: u32,
    name: String,
}

/// Contents of `project.buture`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
}

pub fn serialize_manifest(name: &str) -> Vec<u8> {
    to_bytes(&ManifestV1 {
        format: FORMAT_TAG.into(),
        schema_version: CURRENT_SCHEMA,
        name: name.into(),
    })
}

pub fn parse_manifest(path: &Path, bytes: &[u8]) -> Result<Manifest> {
    parse_manifest_with(path, bytes, &Migrations::builtin())
}

pub fn parse_manifest_with(path: &Path, bytes: &[u8], migrations: &Migrations) -> Result<Manifest> {
    let m: ManifestV1 = parse_migrated(path, bytes, FORMAT_TAG, migrations)?;
    buturecad_eda_core::Project::validate_name(&m.name).map_err(|source| IoError::Invalid {
        path: path.into(),
        source,
    })?;
    Ok(Manifest { name: m.name })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchematicV1 {
    format: String,
    schema_version: u32,
    components: BTreeMap<Id, Component>,
    wires: BTreeMap<Id, Wire>,
}

pub fn serialize_schematic(doc: &SchematicDoc) -> Vec<u8> {
    to_bytes(&SchematicV1 {
        format: SCHEMATIC_TAG.into(),
        schema_version: CURRENT_SCHEMA,
        components: doc.components.clone(),
        wires: doc.wires.clone(),
    })
}

pub fn parse_schematic(path: &Path, bytes: &[u8]) -> Result<SchematicDoc> {
    let f: SchematicV1 = parse_migrated(path, bytes, SCHEMATIC_TAG, &Migrations::builtin())?;
    Ok(SchematicDoc {
        components: f.components,
        wires: f.wires,
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PcbV1 {
    format: String,
    schema_version: u32,
    tracks: BTreeMap<Id, Track>,
    vias: BTreeMap<Id, Via>,
}

pub fn serialize_pcb(doc: &PcbDoc) -> Vec<u8> {
    to_bytes(&PcbV1 {
        format: PCB_TAG.into(),
        schema_version: CURRENT_SCHEMA,
        tracks: doc.tracks.clone(),
        vias: doc.vias.clone(),
    })
}

pub fn parse_pcb(path: &Path, bytes: &[u8]) -> Result<PcbDoc> {
    let f: PcbV1 = parse_migrated(path, bytes, PCB_TAG, &Migrations::builtin())?;
    Ok(PcbDoc {
        tracks: f.tracks,
        vias: f.vias,
    })
}
