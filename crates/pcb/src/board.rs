//! Board-level objects: placed footprints, nets, zones, keepouts, text and graphics, plus the
//! exact geometry of placed pads. Pure data and geometry; no rendering, no tools.
//!
//! Orientation: a placement first **flips** (mirror about the footprint's vertical axis, for the
//! back side), then rotates counter-clockwise by quarter turns, then translates. Arbitrary
//! rotation angles are not supported yet; every operation stays integer-exact.

use crate::footprint::{Footprint, FpGraphic, FpLayer, FpShape, Pad, PadKind, PadShape};
use crate::layer::{self, LayerId};
use buturecad_geometry::{Coord, Point, Polygon, Segment};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Identifier of any board object (shares the project's id counter).
pub use buturecad_schematic::Id;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum BoardError {
    #[error("{0}")]
    Invalid(String),
}

fn bad<T>(msg: impl Into<String>) -> Result<T, BoardError> {
    Err(BoardError::Invalid(msg.into()))
}

fn in_range(p: Point) -> bool {
    Point::try_new(p.x, p.y).is_ok()
}

/// Where and how a footprint sits on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub position: Point,
    /// Counter-clockwise quarter turns, `0..=3`.
    pub rotation: u8,
    /// Placed on the back side (mirrored).
    #[serde(default)]
    pub flipped: bool,
}

impl Placement {
    pub fn at(position: Point) -> Self {
        Self {
            position,
            rotation: 0,
            flipped: false,
        }
    }

    /// Footprint-local point → board coordinates.
    pub fn apply(&self, local: Point) -> Point {
        let p = if self.flipped {
            Point::new(local.x.saturating_neg(), local.y)
        } else {
            local
        };
        let r = p.rotate_quarter_turns(Point::ORIGIN, self.rotation % 4);
        Point::new(
            self.position.x.saturating_add(r.x),
            self.position.y.saturating_add(r.y),
        )
    }

    /// Board layer a footprint-defined layer lands on.
    pub fn board_layer(&self, front: LayerId) -> LayerId {
        if self.flipped {
            layer::flipped(front)
        } else {
            front
        }
    }
}

/// A footprint placed on the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedFootprint {
    /// Key of the definition in the board's footprint table (`Library:Name`).
    pub footprint: String,
    pub reference: String,
    pub value: String,
    pub placement: Placement,
    #[serde(default)]
    pub locked: bool,
    /// Schematic component this footprint was created from (set by "Update PCB from Schematic").
    #[serde(default)]
    pub link: Option<Id>,
    /// Net of each pad, by pad number.
    #[serde(default)]
    pub pad_nets: BTreeMap<String, Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Net {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub layer: LayerId,
    pub outline: Vec<Point>,
    #[serde(default)]
    pub net: Option<Id>,
    /// Higher priority zones fill first.
    #[serde(default)]
    pub priority: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keepout {
    pub outline: Vec<Point>,
    /// Copper layers affected; empty = all.
    #[serde(default)]
    pub layers: Vec<LayerId>,
    #[serde(default = "yes")]
    pub no_tracks: bool,
    #[serde(default = "yes")]
    pub no_vias: bool,
    #[serde(default = "yes")]
    pub no_zones: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardText {
    pub layer: LayerId,
    pub position: Point,
    pub text: String,
    pub height: Coord,
    #[serde(default)]
    pub rotation: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardGraphic {
    pub layer: LayerId,
    #[serde(flatten)]
    pub shape: FpShape,
}

fn poly_ok(pts: &[Point], what: &str) -> Result<(), BoardError> {
    if pts.len() < 3 || pts.len() > 4096 {
        return bad(format!("{what} needs 3 to 4096 points"));
    }
    if pts.iter().any(|p| !in_range(*p)) {
        return bad(format!("{what} is outside the supported range"));
    }
    if Polygon::new(pts.to_vec()).is_err() {
        return bad(format!("{what} is degenerate"));
    }
    Ok(())
}

impl PlacedFootprint {
    pub fn validate(&self) -> Result<(), BoardError> {
        if self.footprint.trim().is_empty() {
            return bad("a placed footprint needs a footprint");
        }
        if self.placement.rotation > 3 {
            return bad("rotation must be 0..=3 quarter turns");
        }
        if !in_range(self.placement.position) {
            return bad("position is outside the supported range");
        }
        if self.reference.chars().any(char::is_control) || self.value.chars().any(char::is_control)
        {
            return bad("reference and value must not contain control characters");
        }
        Ok(())
    }
}

impl Zone {
    pub fn validate(&self) -> Result<(), BoardError> {
        if !layer::is_copper(self.layer) {
            return bad("a zone lives on a copper layer");
        }
        poly_ok(&self.outline, "zone outline")
    }
}

impl Keepout {
    pub fn validate(&self) -> Result<(), BoardError> {
        if self.layers.iter().any(|l| !layer::is_copper(*l)) {
            return bad("a keepout restricts copper layers");
        }
        if !(self.no_tracks || self.no_vias || self.no_zones) {
            return bad("a keepout must forbid something");
        }
        poly_ok(&self.outline, "keepout outline")
    }
}

impl BoardText {
    pub fn validate(&self) -> Result<(), BoardError> {
        if !layer::is_known(self.layer) {
            return bad("unknown layer");
        }
        if self.height <= 0 || self.text.trim().is_empty() || self.rotation > 3 {
            return bad("text needs content, a positive height and a rotation of 0..=3");
        }
        if !in_range(self.position) {
            return bad("position is outside the supported range");
        }
        Ok(())
    }
}

impl BoardGraphic {
    pub fn validate(&self) -> Result<(), BoardError> {
        if !layer::is_known(self.layer) {
            return bad("unknown layer");
        }
        let (width, pts): (Coord, Vec<Point>) = match &self.shape {
            FpShape::Line { segment, width } => (*width, vec![segment.a, segment.b]),
            FpShape::Rect { rect, width, .. } => (*width, vec![rect.min(), rect.max()]),
            FpShape::Circle { circle, width, .. } => (*width, vec![circle.center]),
            FpShape::Arc { arc, width } => (*width, vec![arc.center, arc.start, arc.end]),
            FpShape::Polygon { points, width, .. } => {
                poly_ok(points, "polygon")?;
                (*width, points.clone())
            }
        };
        if width < 0 || pts.iter().any(|p| !in_range(*p)) {
            return bad("graphic has a negative width or is outside the supported range");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Placed pad geometry
// ---------------------------------------------------------------------------------------------

/// A pad in board coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    pub center: Point,
    /// Extent along the board axes after flipping and rotation.
    pub width: Coord,
    pub height: Coord,
    pub roundrect_ratio: u16,
    /// Outline of a custom pad in board coordinates.
    pub outline: Vec<Point>,
    pub front_copper: bool,
    pub back_copper: bool,
}

impl WorldPad {
    /// Copper layers the pad is on.
    pub fn copper_layers(&self, stack: &layer::Stackup) -> Vec<LayerId> {
        match (self.front_copper, self.back_copper) {
            (true, true) => stack.copper(),
            (true, false) => vec![layer::F_CU],
            (false, true) => vec![layer::B_CU],
            _ => Vec::new(),
        }
    }

    pub fn on_layer(&self, l: LayerId, stack: &layer::Stackup) -> bool {
        self.copper_layers(stack).contains(&l)
    }

    pub fn bounds(&self) -> (Point, Point) {
        if self.shape == PadShape::Custom && !self.outline.is_empty() {
            let lo = Point::new(
                self.outline.iter().map(|p| p.x).min().unwrap_or(0),
                self.outline.iter().map(|p| p.y).min().unwrap_or(0),
            );
            let hi = Point::new(
                self.outline.iter().map(|p| p.x).max().unwrap_or(0),
                self.outline.iter().map(|p| p.y).max().unwrap_or(0),
            );
            return (lo, hi);
        }
        (
            Point::new(
                self.center.x - self.width / 2,
                self.center.y - self.height / 2,
            ),
            Point::new(
                self.center.x + self.width - self.width / 2,
                self.center.y + self.height - self.height / 2,
            ),
        )
    }

    /// Does copper of this pad cover `p`? Exact for rectangles and circles, geometric for
    /// ovals, rounded rectangles and custom pads (boundary points count as covered).
    pub fn contains(&self, p: Point) -> bool {
        let (dx, dy) = ((p.x - self.center.x) as f64, (p.y - self.center.y) as f64);
        let (hw, hh) = (self.width as f64 / 2.0, self.height as f64 / 2.0);
        match self.shape {
            PadShape::Circle => dx.hypot(dy) <= hw.max(hh),
            PadShape::Rect => dx.abs() <= hw && dy.abs() <= hh,
            PadShape::Oval => capsule(dx, dy, hw, hh, hw.min(hh)),
            PadShape::RoundRect => {
                let r =
                    f64::from(self.roundrect_ratio) * (self.width.min(self.height) as f64) / 1000.0;
                capsule(dx, dy, hw, hh, r.min(hw.min(hh)))
            }
            PadShape::Custom => Polygon::new(self.outline.clone())
                .map(|poly| poly.locate(p) != buturecad_geometry::PointLocation::Outside)
                .unwrap_or(false),
        }
    }

    /// Smallest distance from `p` to the pad copper (0 inside).
    pub fn distance_to_point(&self, p: Point) -> f64 {
        if self.contains(p) {
            return 0.0;
        }
        let (dx, dy) = ((p.x - self.center.x) as f64, (p.y - self.center.y) as f64);
        let (hw, hh) = (self.width as f64 / 2.0, self.height as f64 / 2.0);
        match self.shape {
            PadShape::Circle => (dx.hypot(dy) - hw.max(hh)).max(0.0),
            PadShape::Custom => Polygon::new(self.outline.clone())
                .map(|poly| poly.distance_to_point(p))
                .unwrap_or(f64::INFINITY),
            _ => {
                let r = match self.shape {
                    PadShape::Oval => hw.min(hh),
                    _ => {
                        f64::from(self.roundrect_ratio) * (self.width.min(self.height) as f64)
                            / 1000.0
                    }
                }
                .min(hw.min(hh));
                // Distance to a rounded box = distance to the inner box minus the corner radius.
                let (ix, iy) = (
                    (dx.abs() - (hw - r)).max(0.0),
                    (dy.abs() - (hh - r)).max(0.0),
                );
                (ix.hypot(iy) - r).max(0.0)
            }
        }
    }
}

/// Is (dx, dy) inside a box of half-extents (hw, hh) with corner radius r?
fn capsule(dx: f64, dy: f64, hw: f64, hh: f64, r: f64) -> bool {
    let (ix, iy) = (
        (dx.abs() - (hw - r)).max(0.0),
        (dy.abs() - (hh - r)).max(0.0),
    );
    dx.abs() <= hw && dy.abs() <= hh && ix.hypot(iy) <= r + 1e-9
}

/// The pad of `fp` as placed by `pl`.
pub fn world_pad(pl: &Placement, pad: &Pad) -> WorldPad {
    let center = pl.apply(pad.position);
    let quarter = (u32::from(pad.rotation) + u32::from(pl.rotation)) % 2 == 1;
    let (w, h) = if quarter {
        (pad.size_y, pad.size_x)
    } else {
        (pad.size_x, pad.size_y)
    };
    let outline = if pad.shape == PadShape::Custom {
        pad.custom
            .iter()
            .map(|q| {
                let q = q.rotate_quarter_turns(Point::ORIGIN, pad.rotation % 4);
                pl.apply(Point::new(pad.position.x + q.x, pad.position.y + q.y))
            })
            .collect()
    } else {
        Vec::new()
    };
    let (front, back) = match pad.kind {
        PadKind::Smd => (!pl.flipped, pl.flipped),
        PadKind::Through => (true, true),
        PadKind::NonPlated => (false, false),
    };
    WorldPad {
        number: pad.number.clone(),
        kind: pad.kind,
        shape: pad.shape,
        center,
        width: w,
        height: h,
        roundrect_ratio: pad.roundrect_ratio,
        outline,
        front_copper: front,
        back_copper: back,
    }
}

impl PlacedFootprint {
    /// All pads in board coordinates.
    pub fn world_pads(&self, def: &Footprint) -> Vec<WorldPad> {
        def.pads
            .iter()
            .map(|p| world_pad(&self.placement, p))
            .collect()
    }

    /// Footprint graphics on their board layers, with endpoints transformed.
    pub fn world_graphics(&self, def: &Footprint) -> Vec<BoardGraphic> {
        def.graphics
            .iter()
            .filter_map(|g| self.world_graphic(g))
            .collect()
    }

    fn world_graphic(&self, g: &FpGraphic) -> Option<BoardGraphic> {
        let layer = self.placement.board_layer(fp_layer_front(g.layer));
        let f = |p: Point| self.placement.apply(p);
        let flips = self.placement.flipped;
        let shape = match &g.shape {
            FpShape::Line { segment, width } => FpShape::Line {
                segment: Segment::new(f(segment.a), f(segment.b)),
                width: *width,
            },
            FpShape::Rect {
                rect,
                filled,
                width,
            } => FpShape::Polygon {
                points: {
                    let (a, b) = (rect.min(), rect.max());
                    vec![
                        f(Point::new(a.x, a.y)),
                        f(Point::new(b.x, a.y)),
                        f(Point::new(b.x, b.y)),
                        f(Point::new(a.x, b.y)),
                    ]
                },
                filled: *filled,
                width: *width,
            },
            FpShape::Circle {
                circle,
                filled,
                width,
            } => FpShape::Circle {
                circle: buturecad_geometry::Circle {
                    center: f(circle.center),
                    radius: circle.radius,
                },
                filled: *filled,
                width: *width,
            },
            FpShape::Arc { arc, width } => FpShape::Arc {
                arc: buturecad_geometry::Arc::new(
                    f(arc.center),
                    f(arc.start),
                    f(arc.end),
                    arc.ccw != flips,
                )
                .ok()?,
                width: *width,
            },
            FpShape::Polygon {
                points,
                filled,
                width,
            } => FpShape::Polygon {
                points: points.iter().map(|p| f(*p)).collect(),
                filled: *filled,
                width: *width,
            },
        };
        Some(BoardGraphic { layer, shape })
    }

    /// Bounding box of the pads and courtyard in board coordinates.
    pub fn bounds(&self, def: &Footprint) -> Option<(Point, Point)> {
        let mut pts: Vec<Point> = Vec::new();
        for p in self.world_pads(def) {
            let (lo, hi) = p.bounds();
            pts.extend([lo, hi]);
        }
        for g in self.world_graphics(def) {
            if layer::kind(g.layer) == Some(layer::LayerKind::Courtyard) {
                match &g.shape {
                    FpShape::Polygon { points, .. } => pts.extend(points.iter().copied()),
                    FpShape::Line { segment, .. } => pts.extend([segment.a, segment.b]),
                    FpShape::Circle { circle, .. } => pts.extend([
                        Point::new(
                            circle.center.x - circle.radius,
                            circle.center.y - circle.radius,
                        ),
                        Point::new(
                            circle.center.x + circle.radius,
                            circle.center.y + circle.radius,
                        ),
                    ]),
                    _ => {}
                }
            }
        }
        if pts.is_empty() {
            pts.push(self.placement.position);
        }
        let lo = Point::new(
            pts.iter().map(|p| p.x).min()?,
            pts.iter().map(|p| p.y).min()?,
        );
        let hi = Point::new(
            pts.iter().map(|p| p.x).max()?,
            pts.iter().map(|p| p.y).max()?,
        );
        Some((lo, hi))
    }
}

/// Footprint layer → the front-side board layer it corresponds to.
pub fn fp_layer_front(l: FpLayer) -> LayerId {
    match l {
        FpLayer::Silkscreen => layer::F_SILK,
        FpLayer::Fabrication => layer::F_FAB,
        FpLayer::Courtyard => layer::F_CRTYD,
        FpLayer::Mask => layer::F_MASK,
        FpLayer::Paste => layer::F_PASTE,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::generators::Soic;

    #[test]
    fn placement_flips_then_rotates_then_translates() {
        let pl = Placement {
            position: Point::new(10, 20),
            rotation: 1,
            flipped: true,
        };
        // local (3, 0): flip → (-3, 0); rotate 90° CCW → (0, -3); translate → (10, 17).
        assert_eq!(pl.apply(Point::new(3, 0)), Point::new(10, 17));
        assert_eq!(
            Placement::at(Point::new(1, 2)).apply(Point::new(3, 4)),
            Point::new(4, 6)
        );
        assert_eq!(pl.board_layer(layer::F_SILK), layer::B_SILK);
    }

    #[test]
    fn world_pads_follow_the_placement() {
        let def = Soic::narrow(8).generate().unwrap();
        let pl = PlacedFootprint {
            footprint: def.name.clone(),
            reference: "U1".into(),
            value: "x".into(),
            placement: Placement {
                position: Point::new(50_000_000, 20_000_000),
                rotation: 1,
                flipped: false,
            },
            locked: false,
            link: None,
            pad_nets: BTreeMap::new(),
        };
        pl.validate().unwrap();
        let pads = pl.world_pads(&def);
        assert_eq!(pads.len(), 8);
        // Pin 1 at local (-2.7, 1.905) rotates to (-1.905, -2.7).
        assert_eq!(
            pads[0].center,
            Point::new(50_000_000 - 1_905_000, 20_000_000 - 2_700_000)
        );
        // The pad was 1.55 × 0.6 mm; one quarter turn swaps the extents.
        assert_eq!((pads[0].width, pads[0].height), (600_000, 1_550_000));
        assert!(pads[0].front_copper && !pads[0].back_copper);
        assert!(pads[0].contains(pads[0].center));
        assert!(!pads[0].contains(Point::new(pads[0].center.x + 400_000, pads[0].center.y)));
        // Flipping puts SMD copper on the back and mirrors the layout.
        let mut back = pl.clone();
        back.placement.flipped = true;
        back.placement.rotation = 0;
        let bp = back.world_pads(&def);
        assert!(!bp[0].front_copper && bp[0].back_copper);
        assert!(bp[0].center.x > 50_000_000, "pin 1 moved to the right side");
        let g = back.world_graphics(&def);
        assert!(g.iter().all(|g| layer::side(g.layer) == layer::Side::Back));
    }

    #[test]
    fn pad_shapes_cover_what_they_should() {
        let mk = |shape, w, h| WorldPad {
            number: "1".into(),
            kind: PadKind::Smd,
            shape,
            center: Point::ORIGIN,
            width: w,
            height: h,
            roundrect_ratio: 250,
            outline: vec![],
            front_copper: true,
            back_copper: false,
        };
        let rect = mk(PadShape::Rect, 2_000, 1_000);
        assert!(rect.contains(Point::new(1_000, 500)));
        assert!(!rect.contains(Point::new(1_001, 0)));
        let oval = mk(PadShape::Oval, 2_000, 1_000);
        assert!(oval.contains(Point::new(900, 0)));
        assert!(
            !oval.contains(Point::new(990, 490)),
            "the corner of the box is rounded off"
        );
        let circle = mk(PadShape::Circle, 1_000, 1_000);
        assert!(circle.contains(Point::new(500, 0)) && !circle.contains(Point::new(400, 400)));
        assert_eq!(rect.distance_to_point(Point::new(1_500, 0)), 500.0);
        assert_eq!(circle.distance_to_point(Point::new(1_500, 0)), 1_000.0);
        assert_eq!(rect.distance_to_point(Point::ORIGIN), 0.0);
    }

    #[test]
    fn board_objects_validate() {
        let sq = vec![Point::new(0, 0), Point::new(10, 0), Point::new(10, 10)];
        assert!(Zone {
            layer: layer::F_CU,
            outline: sq.clone(),
            net: None,
            priority: 0
        }
        .validate()
        .is_ok());
        assert!(Zone {
            layer: layer::F_SILK,
            outline: sq.clone(),
            net: None,
            priority: 0
        }
        .validate()
        .is_err());
        assert!(Zone {
            layer: layer::F_CU,
            outline: sq[..2].to_vec(),
            net: None,
            priority: 0
        }
        .validate()
        .is_err());
        assert!(Keepout {
            outline: sq.clone(),
            layers: vec![],
            no_tracks: false,
            no_vias: false,
            no_zones: false
        }
        .validate()
        .is_err());
        assert!(BoardText {
            layer: layer::F_SILK,
            position: Point::ORIGIN,
            text: "".into(),
            height: 1,
            rotation: 0
        }
        .validate()
        .is_err());
    }
}
