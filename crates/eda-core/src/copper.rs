//! Copper geometry for rule checking and routing: every piece of copper on a layer reduced to a
//! capsule (segment + radius: tracks, vias, round and oval pads) or a polygon (rectangular,
//! rounded and custom pads), with exact-enough edge-to-edge distances.

use crate::model::{Id, PcbDoc, PcbObject};
use buturecad_geometry::{Bounded, BoundingBox, Coord, Point, Polygon, Segment, SpatialIndex};
use buturecad_pcb::{LayerId, PadShape, WorldPad};
use std::f64::consts::PI;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum CShape {
    /// A segment thickened by `r` on every side (a point when `seg.a == seg.b`).
    Cap {
        seg: Segment,
        r: f64,
    },
    Poly(Polygon),
    /// A polygon with holes (a piece of a filled zone).
    Region(Arc<Region>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Track,
    Via,
    Pad,
    /// A piece of a zone fill.
    Zone,
}

/// A filled area: an outer contour and holes, with an edge index so distances to very large
/// fills (thousands of edges) stay local.
pub struct Region {
    pub outer: Polygon,
    pub holes: Vec<Polygon>,
    edges: SpatialIndex<Segment>,
    bbox: BoundingBox,
}

impl std::fmt::Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Region")
            .field("outer", &self.outer.vertices().len())
            .field("holes", &self.holes.len())
            .finish()
    }
}

impl PartialEq for Region {
    fn eq(&self, o: &Self) -> bool {
        self.outer == o.outer && self.holes == o.holes
    }
}

fn seg_box(s: &Segment) -> BoundingBox {
    BoundingBox::new(
        Point::new(s.a.x.min(s.b.x), s.a.y.min(s.b.y)),
        Point::new(s.a.x.max(s.b.x), s.a.y.max(s.b.y)),
    )
}

impl Region {
    pub fn new(outer: Vec<Point>, holes: Vec<Vec<Point>>) -> Option<Self> {
        let outer = Polygon::new(outer).ok()?;
        let holes: Vec<Polygon> = holes
            .into_iter()
            .filter_map(|h| Polygon::new(h).ok())
            .collect();
        let edges = SpatialIndex::bulk_load(
            outer
                .edges()
                .chain(holes.iter().flat_map(|h| h.edges()))
                .map(|e| (seg_box(&e), e)),
        );
        let bbox = outer.bounding_box();
        Some(Self {
            outer,
            holes,
            edges,
            bbox,
        })
    }

    pub fn bbox(&self) -> BoundingBox {
        self.bbox
    }

    /// Is `p` covered by the area (inside the outer contour, outside every hole; boundary counts)?
    pub fn contains(&self, p: Point) -> bool {
        if !self.bbox.contains_point(p) {
            return false;
        }
        // Even-odd crossings of a ray towards +x over the edges of every contour (an outer
        // contour and its holes together), through the edge index: O(log n + crossings).
        let ray = BoundingBox::new(p, Point::new(self.bbox.max.x, p.y));
        let mut inside = false;
        for (_, e) in self.edges.query_intersecting(&ray) {
            if e.contains_point(p) {
                return true; // on the boundary
            }
            if (e.a.y > p.y) != (e.b.y > p.y) {
                let side = (e.b - e.a).cross(p - e.a);
                if (e.b.y > e.a.y && side > 0) || (e.b.y < e.a.y && side < 0) {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Smallest distance from `s` to the boundary of the area.
    pub fn boundary_distance(&self, s: &Segment) -> f64 {
        let base = seg_box(s);
        let mut window: Coord = 1_000_000;
        loop {
            let area = base.inflate(window).unwrap_or(base);
            let best = self
                .edges
                .query_intersecting(&area)
                .map(|(_, e)| e.distance_to_segment(s))
                .fold(f64::INFINITY, f64::min);
            if best <= window as f64 {
                return best;
            }
            if window > 1 << 40 {
                return best;
            }
            window = window.saturating_mul(4);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopperItem {
    /// The board object that owns the copper (track, via or footprint id).
    pub owner: Id,
    pub kind: ItemKind,
    pub net: Option<Id>,
    pub layer: LayerId,
    pub shape: CShape,
    /// Human description (`U1 pad 3`, `track`, `via`).
    pub label: String,
}

impl CopperItem {
    pub fn bbox(&self) -> BoundingBox {
        match &self.shape {
            CShape::Cap { seg, r } => {
                let lo = Point::new(seg.a.x.min(seg.b.x), seg.a.y.min(seg.b.y));
                let hi = Point::new(seg.a.x.max(seg.b.x), seg.a.y.max(seg.b.y));
                let b = BoundingBox::new(lo, hi);
                b.inflate(r.ceil() as Coord + 1).unwrap_or(b)
            }
            CShape::Poly(p) => p.bounding_box(),
            CShape::Region(r) => r.bbox(),
        }
    }

    /// A point inside (or on) the item, used for containment tests.
    pub fn anchor(&self) -> Point {
        match &self.shape {
            CShape::Cap { seg, .. } => seg.a,
            CShape::Poly(p) => p.vertices()[0],
            CShape::Region(r) => r.outer.vertices()[0],
        }
    }
}

pub fn rounded_rect_points(c: Point, w: i64, h: i64, r: i64) -> Vec<Point> {
    let r = r.clamp(0, w.min(h) / 2);
    if r < 1 {
        let (hw, hh) = (w / 2, h / 2);
        return vec![
            Point::new(c.x - hw, c.y - hh),
            Point::new(c.x + hw, c.y - hh),
            Point::new(c.x + hw, c.y + hh),
            Point::new(c.x - hw, c.y + hh),
        ];
    }
    let (hw, hh, rf) = (w as f64 / 2.0, h as f64 / 2.0, r as f64);
    let steps = 8;
    let mut pts = Vec::new();
    for (k, (sx, sy)) in [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
        .into_iter()
        .enumerate()
    {
        let (cx, cy) = (c.x as f64 + sx * (hw - rf), c.y as f64 + sy * (hh - rf));
        for i in 0..=steps {
            let a = -PI / 2.0 + (k as f64) * PI / 2.0 + PI / 2.0 * (i as f64) / (steps as f64);
            pts.push(Point::new(
                (cx + rf * a.cos()).round() as i64,
                (cy + rf * a.sin()).round() as i64,
            ));
        }
    }
    pts.dedup();
    pts
}

/// The shape of a placed pad's copper.
pub fn pad_shape(p: &WorldPad) -> Option<CShape> {
    let (w, h) = (p.width, p.height);
    Some(match p.shape {
        PadShape::Circle => CShape::Cap {
            seg: Segment::new(p.center, p.center),
            r: w.max(h) as f64 / 2.0,
        },
        PadShape::Oval => {
            let r = w.min(h) as f64 / 2.0;
            let d = (w.max(h) - w.min(h)) / 2;
            let seg = if w >= h {
                Segment::new(
                    Point::new(p.center.x - d, p.center.y),
                    Point::new(p.center.x + d, p.center.y),
                )
            } else {
                Segment::new(
                    Point::new(p.center.x, p.center.y - d),
                    Point::new(p.center.x, p.center.y + d),
                )
            };
            CShape::Cap { seg, r }
        }
        PadShape::Rect => CShape::Poly(Polygon::new(rounded_rect_points(p.center, w, h, 0)).ok()?),
        PadShape::RoundRect => {
            let r = i64::from(p.roundrect_ratio) * w.min(h) / 1000;
            CShape::Poly(Polygon::new(rounded_rect_points(p.center, w, h, r)).ok()?)
        }
        PadShape::Custom => CShape::Poly(Polygon::new(p.outline.clone()).ok()?),
    })
}

/// All copper of one board object, per layer.
pub fn items_of(doc: &PcbDoc, id: Id) -> Vec<CopperItem> {
    let Some(o) = doc.object(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match o {
        PcbObject::Track(t) => out.push(CopperItem {
            owner: id,
            kind: ItemKind::Track,
            net: t.net,
            layer: t.layer,
            shape: CShape::Cap {
                seg: t.segment,
                r: t.width as f64 / 2.0,
            },
            label: "track".into(),
        }),
        PcbObject::Via(v) => {
            for l in v.kind.layers(&doc.stackup) {
                out.push(CopperItem {
                    owner: id,
                    kind: ItemKind::Via,
                    net: v.net,
                    layer: l,
                    shape: CShape::Cap {
                        seg: Segment::new(v.position, v.position),
                        r: v.diameter as f64 / 2.0,
                    },
                    label: "via".into(),
                });
            }
        }
        PcbObject::Footprint(f) => {
            let Some(def) = doc.footprints.get(&f.footprint) else {
                return out;
            };
            for p in f.world_pads(def) {
                let Some(shape) = pad_shape(&p) else { continue };
                for l in p.copper_layers(&doc.stackup) {
                    out.push(CopperItem {
                        owner: id,
                        kind: ItemKind::Pad,
                        net: f.pad_nets.get(&p.number).copied(),
                        layer: l,
                        shape: shape.clone(),
                        label: format!("{} pad {}", f.reference, p.number),
                    });
                }
            }
        }
        PcbObject::Zone(z) => {
            if let Some(fill) = &z.fill {
                for r in &fill.regions {
                    if let Some(region) = Region::new(r.outer.clone(), r.holes.clone()) {
                        out.push(CopperItem {
                            owner: id,
                            kind: ItemKind::Zone,
                            net: z.net,
                            layer: z.layer,
                            shape: CShape::Region(Arc::new(region)),
                            label: "zone".into(),
                        });
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn seg_poly(poly: &Polygon, s: &Segment) -> f64 {
    if poly.collides_segment(s) {
        return 0.0;
    }
    poly.edges()
        .map(|e| e.distance_to_segment(s))
        .fold(f64::INFINITY, f64::min)
}

fn poly_region(p: &Polygon, r: &Region) -> f64 {
    if p.vertices().iter().any(|v| r.contains(*v)) || p.contains_point(r.outer.vertices()[0]) {
        return 0.0;
    }
    p.edges()
        .map(|e| r.boundary_distance(&e))
        .fold(f64::INFINITY, f64::min)
}

/// Edge-to-edge distance of two shapes; `<= 0` means they overlap or touch.
pub fn distance(a: &CShape, b: &CShape) -> f64 {
    match (a, b) {
        (CShape::Cap { seg: s, r: ra }, CShape::Cap { seg: t, r: rb }) => {
            s.distance_to_segment(t) - ra - rb
        }
        (CShape::Cap { seg, r }, CShape::Poly(p)) | (CShape::Poly(p), CShape::Cap { seg, r }) => {
            seg_poly(p, seg) - r
        }
        (CShape::Poly(p), CShape::Poly(q)) => p.distance_to_polygon(q),
        (CShape::Cap { seg, r }, CShape::Region(g))
        | (CShape::Region(g), CShape::Cap { seg, r }) => {
            if g.contains(seg.a) || g.contains(seg.b) {
                -r
            } else {
                g.boundary_distance(seg) - r
            }
        }
        (CShape::Poly(p), CShape::Region(g)) | (CShape::Region(g), CShape::Poly(p)) => {
            poly_region(p, g)
        }
        (CShape::Region(g), CShape::Region(h)) => {
            if h.outer.vertices().iter().any(|v| g.contains(*v))
                || g.outer.vertices().iter().any(|v| h.contains(*v))
            {
                return 0.0;
            }
            h.outer
                .edges()
                .chain(h.holes.iter().flat_map(|x| x.edges()))
                .map(|e| g.boundary_distance(&e))
                .fold(f64::INFINITY, f64::min)
        }
    }
}

/// Distance from a shape to a plain segment (board edge).
pub fn distance_to_segment(a: &CShape, s: &Segment) -> f64 {
    match a {
        CShape::Cap { seg, r } => seg.distance_to_segment(s) - r,
        CShape::Poly(p) => seg_poly(p, s),
        CShape::Region(g) => {
            if g.contains(s.a) || g.contains(s.b) {
                0.0
            } else {
                g.boundary_distance(s)
            }
        }
    }
}
