//! The **EDA IR**: a language-independent, fully resolved description of a design.
//!
//! Front ends (ButureScript today; importers and agents later) produce a [`Design`]; the core
//! turns it into schematic and board objects through the Command Engine ([`crate::ir_apply`]).
//! Nothing here knows about syntax, rendering or the GUI. Lengths are integer nanometres, like
//! the rest of the core; angles are degrees.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Pt {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentIr {
    /// Reference designator (`R1`).
    pub reference: String,
    pub value: String,
    /// Footprint name as written (`0402`, `soic(pins=8)`); resolved when applied.
    pub footprint: String,
    /// Schematic symbol id (`Buture:R`); empty = chosen from the reference prefix.
    pub symbol: String,
    /// Extra properties (`part`, `mpn`, `Spice_Model`, …).
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRefIr {
    pub reference: String,
    /// Pin number or name.
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetIr {
    pub name: String,
    pub pins: Vec<PinRefIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementIr {
    pub reference: String,
    pub at: Pt,
    /// Counter-clockwise degrees; only multiples of 90 exist in the core.
    pub rotation_deg: i32,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackIr {
    pub net: String,
    /// Layer name (`F.Cu`).
    pub layer: String,
    /// At least two points; consecutive points are joined by straight segments.
    pub points: Vec<Pt>,
    pub width: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViaIr {
    pub net: String,
    pub at: Pt,
    pub diameter: i64,
    pub drill: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneIr {
    pub net: String,
    pub layer: String,
    pub outline: Vec<Pt>,
    pub clearance: Option<i64>,
    pub min_width: Option<i64>,
    pub priority: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardIr {
    /// Closed polygon of the board edge.
    pub outline: Vec<Pt>,
    /// Copper layer count (2, 4 … 32).
    pub layers: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetClassIr {
    pub name: String,
    pub track_width: Option<i64>,
    pub clearance: Option<i64>,
    pub via_diameter: Option<i64>,
    pub via_drill: Option<i64>,
    pub nets: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesIr {
    pub min_track_width: Option<i64>,
    pub min_via_diameter: Option<i64>,
    pub min_via_drill: Option<i64>,
    pub min_annular_ring: Option<i64>,
    pub copper_to_edge: Option<i64>,
    /// Clearance of the default net class.
    pub clearance: Option<i64>,
    pub classes: Vec<NetClassIr>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Design {
    pub name: String,
    pub board: Option<BoardIr>,
    pub components: Vec<ComponentIr>,
    pub nets: Vec<NetIr>,
    pub placements: Vec<PlacementIr>,
    pub tracks: Vec<TrackIr>,
    pub vias: Vec<ViaIr>,
    pub zones: Vec<ZoneIr>,
    pub rules: RulesIr,
}
