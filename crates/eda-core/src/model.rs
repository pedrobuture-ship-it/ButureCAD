//! Minimal design model hosting the command infrastructure.
//!
//! **Incomplete by design.** These entities carry only what the Command Engine needs today
//! (identity, geometry, a few validated attributes). Pins, nets, connectivity, footprints and
//! padstacks arrive with the schematic/PCB core stages and will move into the `schematic` and
//! `pcb` crates; the command infrastructure does not change when they do.

use buturecad_geometry::{Coord, Point, Segment};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Stable identifier of a design object. Never reused within a session, so undo/redo and
/// references (e.g. from an Agent API diff) stay valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

/// Copper/board layer index (stackup semantics arrive with the stackup stage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u16);

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ModelError {
    #[error("component reference must not be empty")]
    EmptyReference,
    #[error("component rotation must be 0..=3 quarter turns, got {0}")]
    BadRotation(u8),
    #[error("track width must be positive")]
    BadTrackWidth,
    #[error("track must have distinct endpoints")]
    ZeroLengthTrack,
    #[error("wire must have distinct endpoints")]
    ZeroLengthWire,
    #[error("via drill must be positive and smaller than the via diameter")]
    BadViaSize,
    #[error("coordinate is outside the supported range")]
    OutOfRange,
}

fn check_point(p: Point) -> Result<(), ModelError> {
    Point::try_new(p.x, p.y)
        .map(|_| ())
        .map_err(|_| ModelError::OutOfRange)
}

/// A placed schematic component instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub position: Point,
    /// Counter-clockwise quarter turns, `0..=3`.
    pub rotation: u8,
}

/// A schematic wire segment. Electrical connectivity is *not* inferred from geometry; the
/// connectivity engine will derive it logically in a later stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub segment: Segment,
}

/// A PCB copper track segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub layer: LayerId,
    pub segment: Segment,
    pub width: Coord,
}

/// A PCB via.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Via {
    pub position: Point,
    pub diameter: Coord,
    pub drill: Coord,
}

/// Schematic document contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchematicDoc {
    pub components: BTreeMap<Id, Component>,
    pub wires: BTreeMap<Id, Wire>,
}

/// PCB document contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcbDoc {
    pub tracks: BTreeMap<Id, Track>,
    pub vias: BTreeMap<Id, Via>,
}

/// A design object that can be created and deleted through generic commands.
pub trait Entity: Clone + std::fmt::Debug + 'static {
    /// Human-readable kind, used in labels and errors.
    const KIND: &'static str;
    fn validate(&self) -> Result<(), ModelError>;
    fn store(project: &mut crate::Project) -> &mut BTreeMap<Id, Self>;
}

impl Entity for Component {
    const KIND: &'static str = "component";
    fn validate(&self) -> Result<(), ModelError> {
        if self.reference.trim().is_empty() {
            return Err(ModelError::EmptyReference);
        }
        if self.rotation > 3 {
            return Err(ModelError::BadRotation(self.rotation));
        }
        check_point(self.position)
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.schematic.components
    }
}

impl Entity for Wire {
    const KIND: &'static str = "wire";
    fn validate(&self) -> Result<(), ModelError> {
        check_point(self.segment.a)?;
        check_point(self.segment.b)?;
        if self.segment.a == self.segment.b {
            return Err(ModelError::ZeroLengthWire);
        }
        Ok(())
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.schematic.wires
    }
}

impl Entity for Track {
    const KIND: &'static str = "track";
    fn validate(&self) -> Result<(), ModelError> {
        check_point(self.segment.a)?;
        check_point(self.segment.b)?;
        if self.width <= 0 {
            return Err(ModelError::BadTrackWidth);
        }
        if self.segment.a == self.segment.b {
            return Err(ModelError::ZeroLengthTrack);
        }
        Ok(())
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.pcb.tracks
    }
}

impl Entity for Via {
    const KIND: &'static str = "via";
    fn validate(&self) -> Result<(), ModelError> {
        check_point(self.position)?;
        if self.drill <= 0 || self.drill >= self.diameter {
            return Err(ModelError::BadViaSize);
        }
        Ok(())
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.pcb.vias
    }
}
