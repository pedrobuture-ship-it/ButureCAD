//! Copper zone fill (polygon pour).
//!
//! A zone's fill is its outline minus everything it must keep away from, computed with exact
//! integer polygon booleans (`i_overlay`, i64 engine, nanometres):
//!
//! * copper of other nets (and netless copper) inflated by the clearance — the larger of the
//!   zone's own clearance and the nets' rule;
//! * the board edge inflated by `copper_to_edge`, and keepouts that forbid zones;
//! * pads and vias of the zone's own net according to the zone's **connection**: solid (no
//!   cutout), thermal relief (a gap of `thermal_gap` crossed by four axis-aligned spokes of
//!   `thermal_spoke`) or none (isolated like another net);
//! * the fills of higher-priority zones of other nets on the same layer, inflated by the clearance.
//!
//! Then copper thinner than `min_width` is removed (a morphological opening, intersected with the
//! unopened fill so the opening can never add copper), and — when `remove_islands` is set — so is
//! every piece that touches no copper of the zone's own net.
//!
//! The result is stored in the zone ([`Zone::fill`]) together with a **signature** of everything it
//! was computed from, so [`stale_zones`] can tell when a change to the board makes a fill outdated.
//! Tracks of the zone's own net are not cut out: they merge with the pour.
//!
//! Obstacles are circumscribed polygons of the real shapes (a 16-gon around each corner or
//! centre), so clearance is only ever over-estimated.

use crate::copper::{self, CShape, CopperItem, ItemKind, Region};
use crate::model::{Id, PcbChange, PcbDoc, PcbObject};
use crate::outline::outline_segments;
use buturecad_geometry::{convex_hull, BoundingBox, Coord, Point, Polygon, SpatialIndex};
use buturecad_pcb::{FilledRegion, LayerId, PadConnection, Zone, ZoneFill};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay::{Overlay, ShapeType};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::i_float::int::angle::Angle;
use i_overlay::i_float::int::point::IntPoint;
use i_overlay::mesh::int::outline::offset::IntOutlineOffset;
use i_overlay::mesh::int::style::{IntLineJoin, IntOutlineStyle};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

type Ip = IntPoint<i64>;
type Contour = Vec<Ip>;
type Shapes = Vec<Vec<Contour>>;

const RING: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FillError {
    #[error("the zone does not exist")]
    NoSuchZone,
    #[error("the zone outline is not a valid polygon")]
    BadOutline,
}

fn ip(p: Point) -> Ip {
    IntPoint::new(p.x, p.y)
}
fn pt(p: Ip) -> Point {
    Point::new(p.x, p.y)
}
fn contour(pts: &[Point]) -> Contour {
    pts.iter().map(|p| ip(*p)).collect()
}

fn ccw(mut c: Contour) -> Contour {
    let area2: i128 = c
        .iter()
        .zip(c.iter().cycle().skip(1))
        .map(|(a, b)| i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y))
        .sum();
    if area2 < 0 {
        c.reverse();
    }
    c
}

/// Circumscribed polygon of everything within `d` of the `base` points (convex hull of a
/// regular `RING`-gon around each point).
fn inflate(base: &[Point], d: f64) -> Vec<Point> {
    let big = d / (PI / RING as f64).cos() + 2.0;
    let mut pts = Vec::with_capacity(base.len() * RING);
    for c in base {
        for k in 0..RING {
            let a = 2.0 * PI * k as f64 / RING as f64;
            pts.push(Point::new(
                (c.x as f64 + big * a.cos()).round() as i64,
                (c.y as f64 + big * a.sin()).round() as i64,
            ));
        }
    }
    convex_hull(&pts)
}

fn item_base(shape: &CShape) -> (Vec<Point>, f64) {
    match shape {
        CShape::Cap { seg, r } => (vec![seg.a, seg.b], *r),
        CShape::Poly(p) => (p.vertices().to_vec(), 0.0),
        CShape::Region(g) => (g.outer.vertices().to_vec(), 0.0),
    }
}

/// Convex polygon covering `shape` grown by `d`.
fn grown(shape: &CShape, d: f64) -> Vec<Point> {
    let (base, r) = item_base(shape);
    inflate(&base, r + d)
}

fn overlay(subject: &[Contour], clip: &[Contour], rule: OverlayRule) -> Shapes {
    let mut o = Overlay::<i64>::new(subject.len() + clip.len() + 4);
    for c in subject {
        o.add_contour(c, ShapeType::Subject);
    }
    for c in clip {
        o.add_contour(c, ShapeType::Clip);
    }
    o.overlay(rule, FillRule::NonZero)
}

fn flatten(shapes: &Shapes) -> Vec<Contour> {
    shapes.iter().flat_map(|s| s.iter().cloned()).collect()
}

fn offset(shapes: &Shapes, d: i64) -> Shapes {
    if shapes.is_empty() {
        return Vec::new();
    }
    // Mitered joins keep sharp corners, so an inflated obstacle never loses a corner (bevelled
    // joins would let the pour come closer than the clearance there).
    let style = match Angle::from_radians(0.2_f64) {
        Some(a) => IntOutlineStyle::<i64>::new(d).line_join(IntLineJoin::Miter(a)),
        None => IntOutlineStyle::<i64>::new(d),
    };
    shapes.outline(&style).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

/// The non-zone copper of a board, indexed per layer.
pub struct CopperIndex {
    items: Vec<CopperItem>,
    by_layer: BTreeMap<LayerId, SpatialIndex<usize>>,
}

impl CopperIndex {
    pub fn new(doc: &PcbDoc) -> Self {
        let mut items = Vec::new();
        for id in doc
            .tracks
            .keys()
            .chain(doc.vias.keys())
            .chain(doc.placed.keys())
        {
            items.extend(copper::items_of(doc, *id));
        }
        let mut by_layer: BTreeMap<LayerId, SpatialIndex<usize>> = BTreeMap::new();
        for (i, it) in items.iter().enumerate() {
            by_layer.entry(it.layer).or_default().insert(it.bbox(), i);
        }
        Self { items, by_layer }
    }

    fn near(&self, layer: LayerId, area: &BoundingBox) -> Vec<&CopperItem> {
        let mut v: Vec<&CopperItem> = match self.by_layer.get(&layer) {
            Some(ix) => ix
                .query_intersecting(area)
                .map(|(_, i)| &self.items[*i])
                .collect(),
            None => Vec::new(),
        };
        // Index order is not stable across builds; make the result deterministic.
        v.sort_by_key(|it| (it.owner, it.label.clone()));
        v
    }
}

fn zone_bbox(z: &Zone) -> BoundingBox {
    let mut lo = z.outline[0];
    let mut hi = z.outline[0];
    for p in &z.outline {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    BoundingBox::new(lo, hi)
}

fn zone_clearance(doc: &PcbDoc, z: &Zone, other: Option<Id>) -> Coord {
    z.clearance
        .unwrap_or(0)
        .max(doc.rules.clearance_between(z.net, other))
}

/// How far from the zone anything can influence its fill.
fn reach(doc: &PcbDoc, z: &Zone) -> Coord {
    let max_class = doc
        .rules
        .classes
        .values()
        .map(|c| c.clearance)
        .max()
        .unwrap_or(0);
    z.clearance
        .unwrap_or(0)
        .max(max_class)
        .max(doc.rules.copper_to_edge)
        .max(z.thermal_gap)
        + 2_000_000
}

// ---------------------------------------------------------------------------------------------
// Signature
// ---------------------------------------------------------------------------------------------

struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
    fn pt(&mut self, p: Point) {
        self.i64(p.x);
        self.i64(p.y);
    }
}

fn hash_shape(h: &mut Fnv, s: &CShape) {
    match s {
        CShape::Cap { seg, r } => {
            h.u64(1);
            h.pt(seg.a);
            h.pt(seg.b);
            h.u64(r.to_bits());
        }
        CShape::Poly(p) => {
            h.u64(2);
            p.vertices().iter().for_each(|v| h.pt(*v));
        }
        CShape::Region(g) => {
            h.u64(3);
            g.outer.vertices().iter().for_each(|v| h.pt(*v));
        }
    }
}

/// Fingerprint of everything the fill of `zone` depends on: its own parameters, the copper,
/// keepouts and board edge near it, the rule values it uses and the fills of the zones it yields
/// to. Equal signature ⇒ the stored fill is what a refill would produce.
pub fn zone_signature(
    doc: &PcbDoc,
    index: &CopperIndex,
    zone: &Zone,
    upstream: &[(Id, u64)],
) -> u64 {
    let mut h = Fnv::new();
    h.u64(0xB07);
    h.u64(u64::from(zone.layer.0));
    h.u64(zone.net.map_or(u64::MAX, |n| n.0));
    h.u64(u64::from(zone.priority));
    h.i64(zone.clearance.unwrap_or(-1));
    h.i64(zone.min_width);
    h.u64(match zone.connection {
        PadConnection::Solid => 1,
        PadConnection::Thermal => 2,
        PadConnection::None => 3,
    });
    h.i64(zone.thermal_gap);
    h.i64(zone.thermal_spoke);
    h.u64(u64::from(zone.remove_islands));
    zone.outline.iter().for_each(|p| h.pt(*p));
    h.i64(doc.rules.copper_to_edge);
    let area = zone_bbox(zone)
        .inflate(reach(doc, zone))
        .unwrap_or(zone_bbox(zone));
    for it in index.near(zone.layer, &area) {
        h.u64(it.owner.0);
        h.u64(match it.kind {
            ItemKind::Track => 1,
            ItemKind::Via => 2,
            ItemKind::Pad => 3,
            ItemKind::Zone => 4,
        });
        h.u64(it.net.map_or(u64::MAX, |n| n.0));
        hash_shape(&mut h, &it.shape);
        // The clearance this copper demands from the zone.
        h.i64(zone_clearance(doc, zone, it.net));
    }
    for s in outline_segments(doc) {
        if seg_near(&s, &area) {
            h.pt(s.a);
            h.pt(s.b);
        }
    }
    for k in doc.keepouts.values() {
        if k.no_zones && (k.layers.is_empty() || k.layers.contains(&zone.layer)) {
            k.outline.iter().for_each(|p| h.pt(*p));
            h.u64(0xEE);
        }
    }
    for (id, sig) in upstream {
        h.u64(id.0);
        h.u64(*sig);
    }
    h.0
}

fn seg_near(s: &buturecad_geometry::Segment, area: &BoundingBox) -> bool {
    BoundingBox::new(
        Point::new(s.a.x.min(s.b.x), s.a.y.min(s.b.y)),
        Point::new(s.a.x.max(s.b.x), s.a.y.max(s.b.y)),
    )
    .intersects(area)
}

// ---------------------------------------------------------------------------------------------
// Fill
// ---------------------------------------------------------------------------------------------

/// Spokes of a thermal relief: four axis-aligned bars from the centre of `item` across the gap.
fn spokes(item: &CopperItem, gap: Coord, width: Coord) -> Vec<Contour> {
    let bb = item.bbox();
    let c = Point::new((bb.min.x + bb.max.x) / 2, (bb.min.y + bb.max.y) / 2);
    let ext = ((bb.max.x - bb.min.x).max(bb.max.y - bb.min.y) / 2) as f64;
    let len = (ext * 1.1 + gap as f64 + 20_000.0).round() as i64;
    let hw = (width / 2).max(1);
    [(len, hw, 0), (hw, len, 1)]
        .into_iter()
        .map(|(dx, dy, _)| {
            ccw(contour(&[
                Point::new(c.x - dx, c.y - dy),
                Point::new(c.x + dx, c.y - dy),
                Point::new(c.x + dx, c.y + dy),
                Point::new(c.x - dx, c.y + dy),
            ]))
        })
        .collect()
}

/// Regions of `shapes` as stored fill data.
fn to_regions(shapes: &Shapes) -> Vec<FilledRegion> {
    let mut out: Vec<FilledRegion> = shapes
        .iter()
        .filter_map(|s| {
            let (outer, holes) = s.split_first()?;
            if outer.len() < 3 {
                return None;
            }
            Some(FilledRegion {
                outer: outer.iter().map(|p| pt(*p)).collect(),
                holes: holes
                    .iter()
                    .filter(|h| h.len() >= 3)
                    .map(|h| h.iter().map(|p| pt(*p)).collect())
                    .collect(),
            })
        })
        .collect();
    out.sort_by_key(|r| {
        let m = r.outer.iter().min().copied().unwrap_or(Point::ORIGIN);
        (m.x, m.y, r.outer.len())
    });
    out
}

fn from_regions(regions: &[FilledRegion]) -> Shapes {
    regions
        .iter()
        .map(|r| {
            let mut s = vec![ccw(contour(&r.outer))];
            s.extend(r.holes.iter().map(|h| {
                let mut c = ccw(contour(h));
                c.reverse();
                c
            }));
            s
        })
        .collect()
}

/// Computes the fill of zone `id`. `upstream` are the fills of the higher-priority zones on the
/// same layer (already up to date).
pub fn compute_fill(
    doc: &PcbDoc,
    index: &CopperIndex,
    id: Id,
    upstream: &[(Id, &Zone)],
) -> Result<ZoneFill, FillError> {
    let z = doc.zones.get(&id).ok_or(FillError::NoSuchZone)?;
    if Polygon::new(z.outline.clone()).is_err() {
        return Err(FillError::BadOutline);
    }
    let area = zone_bbox(z).inflate(reach(doc, z)).unwrap_or(zone_bbox(z));
    let mut clip: Vec<Contour> = Vec::new();

    // Board edge and keepouts.
    let edge = doc.rules.copper_to_edge as f64;
    for s in outline_segments(doc) {
        if seg_near(&s, &area) {
            clip.push(ccw(contour(&inflate(&[s.a, s.b], edge))));
        }
    }
    for k in doc.keepouts.values() {
        if k.no_zones
            && (k.layers.is_empty() || k.layers.contains(&z.layer))
            && k.outline.len() >= 3
        {
            clip.push(ccw(contour(&k.outline)));
        }
    }
    // Copper on the layer.
    for it in index.near(z.layer, &area) {
        let own = z.net.is_some() && it.net == z.net;
        if own && it.kind == ItemKind::Track {
            continue; // merges with the pour
        }
        if own && matches!(it.kind, ItemKind::Pad | ItemKind::Via) {
            match z.connection {
                PadConnection::Solid => continue,
                PadConnection::None => {
                    let d = zone_clearance(doc, z, it.net) as f64;
                    clip.push(ccw(contour(&grown(&it.shape, d))));
                }
                PadConnection::Thermal => {
                    let outer = vec![ccw(contour(&grown(&it.shape, z.thermal_gap as f64)))];
                    let inner = vec![ccw(contour(&grown(&it.shape, 0.0)))];
                    let ring = overlay(&outer, &inner, OverlayRule::Difference);
                    let cut = overlay(
                        &flatten(&ring),
                        &spokes(it, z.thermal_gap, z.thermal_spoke),
                        OverlayRule::Difference,
                    );
                    clip.extend(flatten(&cut));
                }
            }
            continue;
        }
        let d = zone_clearance(doc, z, it.net) as f64;
        clip.push(ccw(contour(&grown(&it.shape, d))));
    }
    // Zones that fill first.
    let mut higher: Shapes = Vec::new();
    for (_, u) in upstream {
        if u.layer != z.layer || (z.net.is_some() && u.net == z.net) {
            continue;
        }
        if let Some(f) = &u.fill {
            higher.extend(from_regions(&f.regions));
        }
    }
    if !higher.is_empty() {
        let d = zone_clearance(doc, z, None).max(z.clearance.unwrap_or(0));
        clip.extend(flatten(&offset(&higher, d)));
    }

    let subject = vec![ccw(contour(&z.outline))];
    let mut fill = overlay(&subject, &clip, OverlayRule::Difference);

    // Minimum width: opening, never adding copper.
    let open = if z.connection == PadConnection::Thermal {
        z.min_width.min(z.thermal_spoke)
    } else {
        z.min_width
    };
    if open >= 2 && !fill.is_empty() {
        let opened = offset(&offset(&fill, -(open / 2)), open / 2);
        fill = overlay(&flatten(&opened), &flatten(&fill), OverlayRule::Intersect);
    }

    // Islands.
    if z.remove_islands && z.net.is_some() {
        let own: Vec<CopperItem> = index
            .near(z.layer, &area)
            .into_iter()
            .filter(|it| it.net == z.net && it.kind != ItemKind::Zone)
            .cloned()
            .collect();
        fill.retain(|s| {
            let regions = to_regions(&vec![s.clone()]);
            let Some(r) = regions.first() else {
                return false;
            };
            let Some(region) = Region::new(r.outer.clone(), r.holes.clone()) else {
                return false;
            };
            let shape = CShape::Region(std::sync::Arc::new(region));
            own.iter()
                .any(|it| copper::distance(&it.shape, &shape) <= 0.0)
        });
    }

    let regions = to_regions(&fill);
    let ups: Vec<(Id, u64)> = upstream
        .iter()
        .filter(|(_, u)| u.layer == z.layer)
        .map(|(i, u)| (*i, u.fill.as_ref().map_or(0, |f| f.signature)))
        .collect();
    Ok(ZoneFill {
        regions,
        signature: zone_signature(doc, index, z, &ups),
    })
}

/// Zones in fill order: higher priority first, then by id.
fn fill_order(doc: &PcbDoc) -> Vec<Id> {
    let mut ids: Vec<Id> = doc.zones.keys().copied().collect();
    ids.sort_by_key(|i| (std::cmp::Reverse(doc.zones[i].priority), *i));
    ids
}

/// Recomputes zones and returns the changes that store the new fills. With `only = None` every
/// zone is refilled; otherwise only those listed **and** the zones that yield to them.
pub fn fill_changes(doc: &PcbDoc, only: Option<&BTreeSet<Id>>, force: bool) -> Vec<PcbChange> {
    let index = CopperIndex::new(doc);
    let mut done: Vec<(Id, Zone)> = Vec::new();
    let mut changes = Vec::new();
    let mut changed_layers: BTreeSet<(LayerId, u8)> = BTreeSet::new();
    for id in fill_order(doc) {
        let Some(z) = doc.zones.get(&id) else {
            continue;
        };
        let upstream: Vec<(Id, &Zone)> = done.iter().map(|(i, z)| (*i, z)).collect();
        let ups: Vec<(Id, u64)> = upstream
            .iter()
            .filter(|(_, u)| u.layer == z.layer)
            .map(|(i, u)| (*i, u.fill.as_ref().map_or(0, |f| f.signature)))
            .collect();
        let want = only.is_none_or(|s| s.contains(&id))
            || changed_layers
                .iter()
                .any(|(l, p)| *l == z.layer && *p >= z.priority);
        let fresh = z
            .fill
            .as_ref()
            .is_some_and(|f| f.signature == zone_signature(doc, &index, z, &ups));
        if !(want && (force || !fresh)) {
            done.push((id, z.clone()));
            continue;
        }
        match compute_fill(doc, &index, id, &upstream) {
            Ok(fill) => {
                let mut nz = z.clone();
                let same = nz.fill.as_ref().is_some_and(|f| f.regions == fill.regions);
                nz.fill = Some(fill);
                if !same {
                    changed_layers.insert((z.layer, z.priority));
                }
                if nz != *z {
                    changes.push(PcbChange::modify(
                        id,
                        PcbObject::Zone(z.clone()),
                        PcbObject::Zone(nz.clone()),
                    ));
                }
                done.push((id, nz));
            }
            Err(_) => done.push((id, z.clone())),
        }
    }
    changes
}

/// Zones whose stored fill is missing or no longer matches the board.
pub fn stale_zones(doc: &PcbDoc) -> Vec<Id> {
    if doc.zones.is_empty() {
        return Vec::new();
    }
    let index = CopperIndex::new(doc);
    let mut done: Vec<(Id, &Zone)> = Vec::new();
    let mut out = Vec::new();
    for id in fill_order(doc) {
        let Some(z) = doc.zones.get(&id) else {
            continue;
        };
        let ups: Vec<(Id, u64)> = done
            .iter()
            .filter(|(_, u)| u.layer == z.layer)
            .map(|(i, u)| (*i, u.fill.as_ref().map_or(0, |f| f.signature)))
            .collect();
        let fresh = z
            .fill
            .as_ref()
            .is_some_and(|f| f.signature == zone_signature(doc, &index, z, &ups));
        if !fresh {
            out.push(id);
        }
        done.push((id, z));
    }
    out
}

/// Triangles covering exactly the filled area (holes respected), for drawing and for the 3D view.
/// Deterministic: same regions, same triangles.
pub fn triangulate(regions: &[FilledRegion]) -> Vec<[Point; 3]> {
    use i_triangle::int::triangulatable::IntTriangulatable;
    let mut out = Vec::new();
    for r in regions {
        let mut shape: Vec<Contour> = vec![ccw(contour(&r.outer))];
        shape.extend(r.holes.iter().map(|h| {
            let mut c = ccw(contour(h));
            c.reverse();
            c
        }));
        let t = shape.triangulate().into_triangulation::<u32>();
        for tri in t.indices.as_chunks::<3>().0 {
            let p = |i: u32| t.points.get(i as usize).map(|p| pt(*p));
            if let (Some(a), Some(b), Some(c)) = (p(tri[0]), p(tri[1]), p(tri[2])) {
                out.push([a, b, c]);
            }
        }
    }
    out
}
