//! PCB design model hosting the command infrastructure.
//!
//! **Incomplete by design.** The schematic model lives in `buturecad-schematic`. The PCB entities
//! here carry only what the Command Engine needs today (identity, geometry, validated attributes);
//! footprints, pads, nets and zones arrive with the PCB core stage and will move into the `pcb`
//! crate; the command infrastructure does not change when they do.

use buturecad_geometry::{Coord, Point, Segment};
pub use buturecad_schematic::Id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Copper/board layer index (stackup semantics arrive with the stackup stage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u16);

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ModelError {
    #[error("track width must be positive")]
    BadTrackWidth,
    #[error("track must have distinct endpoints")]
    ZeroLengthTrack,
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
