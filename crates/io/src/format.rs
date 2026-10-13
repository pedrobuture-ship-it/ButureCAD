//! On-disk representation of the project files and their schema versioning.
//!
//! See `docs/file-format/`.

use crate::error::{IoError, Result};
use buturecad_eda_core::model::{Id, PcbDoc, Track, Via};
use buturecad_pcb::{
    BoardGraphic, BoardText, Footprint, Keepout, Net, PlacedFootprint, Stackup, Zone,
};
use buturecad_schematic::{Schematic, Sheet, Symbol, Waiver};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const FORMAT_TAG: &str = "buturecad.project";
pub const SCHEMATIC_TAG: &str = "buturecad.schematic";
pub const PCB_TAG: &str = "buturecad.pcb";
/// Schema version of `project.buture` written by this build.
pub const CURRENT_SCHEMA: u32 = 1;
/// Schema version of the schematic document. v1 was the pre-release skeleton (components + wires
/// without symbols or sheets); v2 is the full schematic model (see
/// [`migrate_schematic_v1_to_v2`]); v3 adds symbol units, alternate representations and metadata
/// plus the ERC waiver list (see [`migrate_schematic_v2_to_v3`]).
pub const SCHEMATIC_SCHEMA: u32 = 3;
/// Schema version of the PCB document. v2 added embedded footprint definitions; v3 the board
/// (stackup, placed footprints, nets, zones, keepouts, texts, graphics, track/via nets).
pub const PCB_SCHEMA: u32 = 3;

/// A migration step rewriting a document from version `from` to `from + 1`.
pub type MigrationFn = fn(&mut Value) -> std::result::Result<(), String>;

/// Ordered migration registry. `steps[i]` migrates schema `i + first_version`.
pub struct Migrations {
    first_version: u32,
    steps: Vec<MigrationFn>,
}

impl Migrations {
    /// Registry for files whose schema 1 is still current (manifest, PCB): no steps yet.
    pub fn builtin() -> Self {
        Self {
            first_version: 1,
            steps: Vec::new(),
        }
    }

    /// Registry for the schematic document: v1 → v2 → v3.
    pub fn schematic() -> Self {
        Self {
            first_version: 1,
            steps: vec![migrate_schematic_v1_to_v2, migrate_schematic_v2_to_v3],
        }
    }

    /// Registry for the PCB document: v1 → v2 → v3.
    pub fn pcb() -> Self {
        Self {
            first_version: 1,
            steps: vec![migrate_pcb_v1_to_v2, migrate_pcb_v2_to_v3],
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

/// Schematic v1 → v2. v1 held `components` (reference/value/position/rotation) and `wires`
/// directly; v2 nests objects in sheets and gives components a placement and a (here empty)
/// symbol reference. The single sheet gets the reserved root id `0`, which cannot collide.
pub fn migrate_schematic_v1_to_v2(doc: &mut Value) -> std::result::Result<(), String> {
    let obj = doc.as_object_mut().ok_or("document is not an object")?;
    let components = obj.remove("components").ok_or("missing components")?;
    let wires = obj.remove("wires").ok_or("missing wires")?;
    let mut objects = serde_json::Map::new();
    for (id, c) in components
        .as_object()
        .ok_or("components is not an object")?
    {
        let field = |name: &str| {
            c.get(name)
                .cloned()
                .ok_or_else(|| format!("component {id} lacks {name}"))
        };
        let mut o = serde_json::Map::new();
        o.insert("type".into(), "component".into());
        o.insert("lib_id".into(), "".into());
        o.insert("reference".into(), field("reference")?);
        o.insert("value".into(), field("value")?);
        o.insert("footprint".into(), "".into());
        o.insert("properties".into(), Value::Object(Default::default()));
        let mut placement = serde_json::Map::new();
        placement.insert("position".into(), field("position")?);
        placement.insert("rotation".into(), field("rotation")?);
        placement.insert("mirror".into(), false.into());
        o.insert("placement".into(), Value::Object(placement));
        objects.insert(id.clone(), Value::Object(o));
    }
    for (id, w) in wires.as_object().ok_or("wires is not an object")? {
        let mut o = serde_json::Map::new();
        o.insert("type".into(), "wire".into());
        o.insert(
            "segment".into(),
            w.get("segment")
                .cloned()
                .ok_or_else(|| format!("wire {id} lacks segment"))?,
        );
        if objects.insert(id.clone(), Value::Object(o)).is_some() {
            return Err(format!("object id {id} used twice"));
        }
    }
    let mut sheet = serde_json::Map::new();
    sheet.insert("name".into(), "Root".into());
    sheet.insert("objects".into(), Value::Object(objects));
    let mut sheets = serde_json::Map::new();
    sheets.insert("0".into(), Value::Object(sheet));
    obj.insert("root".into(), 0.into());
    obj.insert("symbols".into(), Value::Object(Default::default()));
    obj.insert("sheets".into(), Value::Object(sheets));
    Ok(())
}

/// Schema 2 → 3. Nothing in the stored data has to change: symbol units, representations and
/// metadata and component units all default when absent, and the waiver list starts empty. The
/// step exists so older builds refuse a v3 file instead of silently dropping what they do not
/// know.
pub fn migrate_schematic_v2_to_v3(doc: &mut Value) -> std::result::Result<(), String> {
    let obj = doc.as_object_mut().ok_or("document is not an object")?;
    obj.entry("erc_waivers")
        .or_insert_with(|| Value::Array(Vec::new()));
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchematicDoc {
    format: String,
    schema_version: u32,
    root: Id,
    symbols: BTreeMap<String, Symbol>,
    sheets: BTreeMap<Id, Sheet>,
    #[serde(default)]
    erc_waivers: Vec<Waiver>,
}

pub fn serialize_schematic(doc: &Schematic) -> Vec<u8> {
    to_bytes(&SchematicDoc {
        format: SCHEMATIC_TAG.into(),
        schema_version: SCHEMATIC_SCHEMA,
        root: doc.root(),
        symbols: doc.symbols().clone(),
        sheets: doc.sheets().clone(),
        erc_waivers: doc.waivers().cloned().collect(),
    })
}

pub fn parse_schematic(path: &Path, bytes: &[u8]) -> Result<Schematic> {
    parse_schematic_with(path, bytes, &Migrations::schematic())
}

pub fn parse_schematic_with(
    path: &Path,
    bytes: &[u8],
    migrations: &Migrations,
) -> Result<Schematic> {
    let f: SchematicDoc = parse_migrated(path, bytes, SCHEMATIC_TAG, migrations)?;
    let invalid = |source: buturecad_schematic::SchematicError| IoError::InvalidDocument {
        path: path.into(),
        reason: source.to_string(),
    };
    let mut sch = Schematic::from_parts(f.root, f.symbols, f.sheets).map_err(invalid)?;
    sch.set_waivers(f.erc_waivers).map_err(invalid)?;
    Ok(sch)
}

/// PCB 1 → 2: adds the (empty) footprint table.
pub fn migrate_pcb_v1_to_v2(doc: &mut Value) -> std::result::Result<(), String> {
    let obj = doc.as_object_mut().ok_or("document is not an object")?;
    obj.entry("footprints")
        .or_insert_with(|| Value::Object(Default::default()));
    Ok(())
}

/// PCB 2 → 3: every board field is optional on read (defaults: 2 copper layers, nothing placed),
/// so nothing has to be rewritten; the bump makes older builds refuse the file.
pub fn migrate_pcb_v2_to_v3(doc: &mut Value) -> std::result::Result<(), String> {
    doc.as_object()
        .map(|_| ())
        .ok_or_else(|| "document is not an object".to_string())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PcbFile {
    format: String,
    schema_version: u32,
    #[serde(default)]
    stackup: Stackup,
    tracks: BTreeMap<Id, Track>,
    vias: BTreeMap<Id, Via>,
    #[serde(default)]
    footprints: BTreeMap<String, Footprint>,
    #[serde(default)]
    placed: BTreeMap<Id, PlacedFootprint>,
    #[serde(default)]
    nets: BTreeMap<Id, Net>,
    #[serde(default)]
    zones: BTreeMap<Id, Zone>,
    #[serde(default)]
    keepouts: BTreeMap<Id, Keepout>,
    #[serde(default)]
    texts: BTreeMap<Id, BoardText>,
    #[serde(default)]
    graphics: BTreeMap<Id, BoardGraphic>,
}

pub fn serialize_pcb(doc: &PcbDoc) -> Vec<u8> {
    to_bytes(&PcbFile {
        format: PCB_TAG.into(),
        schema_version: PCB_SCHEMA,
        stackup: doc.stackup,
        tracks: doc.tracks.clone(),
        vias: doc.vias.clone(),
        footprints: doc.footprints.clone(),
        placed: doc.placed.clone(),
        nets: doc.nets.clone(),
        zones: doc.zones.clone(),
        keepouts: doc.keepouts.clone(),
        texts: doc.texts.clone(),
        graphics: doc.graphics.clone(),
    })
}

pub fn parse_pcb(path: &Path, bytes: &[u8]) -> Result<PcbDoc> {
    let f: PcbFile = parse_migrated(path, bytes, PCB_TAG, &Migrations::pcb())?;
    let invalid = |reason: String| IoError::InvalidDocument {
        path: path.into(),
        reason,
    };
    for (name, fp) in &f.footprints {
        buturecad_pcb::validate(fp).map_err(|e| invalid(format!("footprint {name}: {e}")))?;
        if &fp.name != name {
            return Err(invalid(format!(
                "footprint stored as {name} is named {}",
                fp.name
            )));
        }
    }
    if !f.stackup.is_valid() {
        return Err(invalid(
            "copper layer count must be an even number from 2 to 32".into(),
        ));
    }
    let doc = PcbDoc {
        stackup: f.stackup,
        tracks: f.tracks,
        vias: f.vias,
        footprints: f.footprints,
        placed: f.placed,
        nets: f.nets,
        zones: f.zones,
        keepouts: f.keepouts,
        texts: f.texts,
        graphics: f.graphics,
    };
    // Duplicate ids across kinds and every object's own invariants.
    let mut seen = std::collections::BTreeSet::new();
    for id in doc.ids() {
        if !seen.insert(id) {
            return Err(invalid(format!("object id {} is used twice", id.0)));
        }
    }
    for id in doc.ids().collect::<Vec<_>>() {
        if let Some(o) = doc.object(id) {
            doc.validate_object(&o)
                .map_err(|e| invalid(format!("object {}: {e}", id.0)))?;
        }
    }
    Ok(doc)
}
