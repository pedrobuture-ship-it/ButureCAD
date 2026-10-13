//! Schematic data model. Pure data: no rendering, no tools, no connectivity.
//!
//! Coordinates are integer nanometres (+Y up), like the rest of the core.

use buturecad_geometry::{Arc, Circle, Coord, Point, Rectangle, Segment, Vector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier of a schematic object or sheet, unique within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

// ---------------------------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------------------------

/// Direction a pin extends in, or any axis-aligned direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Right,
    Up,
    Left,
    Down,
}

impl Direction {
    pub fn vector(self) -> Vector {
        match self {
            Direction::Right => Vector::new(1, 0),
            Direction::Up => Vector::new(0, 1),
            Direction::Left => Vector::new(-1, 0),
            Direction::Down => Vector::new(0, -1),
        }
    }
    fn from_vector(v: Vector) -> Direction {
        match (v.x.signum(), v.y.signum()) {
            (1, _) => Direction::Right,
            (-1, _) => Direction::Left,
            (_, 1) => Direction::Up,
            _ => Direction::Down,
        }
    }
}

/// Where and how a symbol instance sits: optional mirror (x → −x, applied first), then
/// counter-clockwise quarter turns, then translation. All exact integer operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub position: Point,
    /// Counter-clockwise quarter turns, `0..=3`.
    pub rotation: u8,
    pub mirror: bool,
}

impl Placement {
    pub fn at(position: Point) -> Self {
        Self {
            position,
            rotation: 0,
            mirror: false,
        }
    }

    /// Symbol-local point → sheet coordinates.
    pub fn apply(&self, local: Point) -> Point {
        let mut p = local;
        if self.mirror {
            p.x = p.x.saturating_neg();
        }
        let r = p.rotate_quarter_turns(Point::ORIGIN, self.rotation % 4);
        self.position + Vector::new(r.x, r.y)
    }

    /// Symbol-local direction → sheet direction.
    pub fn apply_direction(&self, d: Direction) -> Direction {
        let mut v = d.vector();
        if self.mirror {
            v.x = -v.x;
        }
        let r = Point::new(v.x, v.y).rotate_quarter_turns(Point::ORIGIN, self.rotation % 4);
        Direction::from_vector(Vector::new(r.x, r.y))
    }

    pub fn rotated(mut self) -> Self {
        self.rotation = (self.rotation + 1) % 4;
        self
    }

    pub fn mirrored(mut self) -> Self {
        self.mirror = !self.mirror;
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    PowerInput,
    PowerOutput,
    OpenCollector,
    OpenEmitter,
    Unspecified,
    NoConnect,
}

/// A symbol pin. `position` is the *connection point* (where a wire attaches), in symbol-local
/// coordinates; the pin body extends from there towards the symbol along `orientation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub position: Point,
    /// Direction from the connection point towards the symbol body.
    pub orientation: Direction,
    pub length: Coord,
    /// Unit the pin belongs to: `0` = every unit (shared pins such as power), `1..` = that unit.
    #[serde(default)]
    pub unit: u8,
    /// Representation the pin belongs to: `0` = both, [`REPR_NORMAL`] or [`REPR_ALTERNATE`].
    #[serde(default)]
    pub representation: u8,
}

/// Representation numbers. `0` always means "shared by every representation".
pub const REPR_NORMAL: u8 = 1;
pub const REPR_ALTERNATE: u8 = 2;
/// Largest number of units a symbol may declare.
pub const MAX_UNITS: u8 = 26;

impl Pin {
    /// Is the pin part of `unit` (1-based) drawn in `representation`?
    pub fn is_visible_in(&self, unit: u8, representation: u8) -> bool {
        (self.unit == 0 || self.unit == unit)
            && (self.representation == 0 || self.representation == representation)
    }
}

/// Graphic primitives of a symbol body, in symbol-local coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Graphic {
    Line {
        segment: Segment,
        width: Coord,
    },
    Rect {
        rect: Rectangle,
        filled: bool,
        width: Coord,
    },
    Circle {
        circle: Circle,
        filled: bool,
        width: Coord,
    },
    Arc {
        arc: Arc,
        width: Coord,
    },
    Text {
        origin: Point,
        height: Coord,
        text: String,
    },
}

/// A graphic primitive together with the unit / representation it belongs to (same meaning as
/// on [`Pin`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolGraphic {
    #[serde(flatten)]
    pub graphic: Graphic,
    #[serde(default)]
    pub unit: u8,
    #[serde(default)]
    pub representation: u8,
}

impl SymbolGraphic {
    pub fn is_visible_in(&self, unit: u8, representation: u8) -> bool {
        (self.unit == 0 || self.unit == unit)
            && (self.representation == 0 || self.representation == representation)
    }
}

impl From<Graphic> for SymbolGraphic {
    fn from(graphic: Graphic) -> Self {
        Self {
            graphic,
            unit: 0,
            representation: 0,
        }
    }
}

/// Descriptive data of a symbol; never affects connectivity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolMeta {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default)]
    pub datasheet: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub mpn: String,
    /// Footprint proposed when the part is placed (`Library:Name`), may be empty.
    #[serde(default)]
    pub default_footprint: String,
    /// Glob patterns of footprints that fit this symbol (`R_*`, `SOIC-*`).
    #[serde(default)]
    pub footprint_filters: Vec<String>,
    /// Free-form key/value pairs.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

fn one() -> u8 {
    1
}

/// A library symbol definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    /// Prefix for auto-generated references ("R", "C", "U"). Empty for power symbols.
    pub reference_prefix: String,
    pub default_value: String,
    /// Power symbols name a global net instead of being a part.
    pub is_power: bool,
    pub pins: Vec<Pin>,
    pub graphics: Vec<SymbolGraphic>,
    /// Number of units (`1..=`[`MAX_UNITS`]). Units are drawn and placed separately but share
    /// one reference (`U1A`, `U1B`).
    #[serde(default = "one")]
    pub unit_count: u8,
    #[serde(default)]
    pub meta: SymbolMeta,
}

impl Symbol {
    /// An empty single-unit symbol.
    pub fn new(name: impl Into<String>, reference_prefix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            reference_prefix: reference_prefix.into(),
            default_value: String::new(),
            is_power: false,
            pins: Vec::new(),
            graphics: Vec::new(),
            unit_count: 1,
            meta: SymbolMeta::default(),
        }
    }

    /// Does any item use the alternate (De Morgan style) representation?
    pub fn has_alternate(&self) -> bool {
        self.pins.iter().any(|p| p.representation == REPR_ALTERNATE)
            || self
                .graphics
                .iter()
                .any(|g| g.representation == REPR_ALTERNATE)
    }

    /// Pins drawn for `unit` in `representation`.
    pub fn pins_for(&self, unit: u8, representation: u8) -> impl Iterator<Item = &Pin> {
        self.pins
            .iter()
            .filter(move |p| p.is_visible_in(unit, representation))
    }

    /// Graphics drawn for `unit` in `representation`.
    pub fn graphics_for(
        &self,
        unit: u8,
        representation: u8,
    ) -> impl Iterator<Item = &SymbolGraphic> {
        self.graphics
            .iter()
            .filter(move |g| g.is_visible_in(unit, representation))
    }
}

// ---------------------------------------------------------------------------------------------
// Sheet objects
// ---------------------------------------------------------------------------------------------

/// A placed symbol instance (a part).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// Library symbol id; empty means "no symbol" (migrated legacy data).
    pub lib_id: String,
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub properties: BTreeMap<String, String>,
    pub placement: Placement,
    /// Which unit of a multi-unit symbol this instance is (1-based).
    #[serde(default = "one")]
    pub unit: u8,
    /// Which representation is shown ([`REPR_NORMAL`] or [`REPR_ALTERNATE`]).
    #[serde(default = "one")]
    pub representation: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub segment: Segment,
}

/// A bus segment. Buses group signals; they do not make electrical connections themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bus {
    pub segment: Segment,
}

/// Explicit electrical junction: connects every wire passing through or ending at its position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Junction {
    pub position: Point,
}

/// Names the net it is attached to; visible within its sheet only. When attached to a bus it
/// names the bus (`D[0..7]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetLabel {
    pub position: Point,
    pub name: String,
    pub orientation: Direction,
}

/// Names a net across the whole design.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalLabel {
    pub position: Point,
    pub name: String,
    pub orientation: Direction,
}

/// Label inside a child sheet that links to the sheet pin of the same name in the parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierLabel {
    pub position: Point,
    pub name: String,
    pub orientation: Direction,
}

/// A power port: ties its pin to the global net `net_name` (e.g. `GND`, `+3V3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerSymbol {
    pub lib_id: String,
    pub net_name: String,
    pub placement: Placement,
}

/// Marks a pin as intentionally unconnected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoConnect {
    pub position: Point,
}

/// A pin on the border of a sheet instance, linking to the hierarchical label of the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetPin {
    pub name: String,
    /// Absolute position on the sheet-instance border.
    pub position: Point,
}

/// A hierarchical sheet symbol placed on a parent sheet, referring to a child sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetInstance {
    pub name: String,
    pub child: Id,
    pub rect: Rectangle,
    pub pins: Vec<SheetPin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Object {
    Component(Component),
    Wire(Wire),
    Bus(Bus),
    Junction(Junction),
    NetLabel(NetLabel),
    GlobalLabel(GlobalLabel),
    HierLabel(HierLabel),
    PowerSymbol(PowerSymbol),
    NoConnect(NoConnect),
    Sheet(SheetInstance),
}

impl Object {
    pub fn kind(&self) -> &'static str {
        match self {
            Object::Component(_) => "component",
            Object::Wire(_) => "wire",
            Object::Bus(_) => "bus",
            Object::Junction(_) => "junction",
            Object::NetLabel(_) => "net label",
            Object::GlobalLabel(_) => "global label",
            Object::HierLabel(_) => "hierarchical label",
            Object::PowerSymbol(_) => "power symbol",
            Object::NoConnect(_) => "no-connect",
            Object::Sheet(_) => "sheet",
        }
    }
}

/// One drawing sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub objects: BTreeMap<Id, Object>,
}
