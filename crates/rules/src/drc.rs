//! Design Rule Checking on the real board geometry.
//!
//! Rules (all use the net class and board limits of `PcbDoc::rules`):
//!
//! | rule | severity | meaning |
//! |---|---|---|
//! | `clearance` | Error | copper of different nets closer than the larger of their classes' clearances (tracks, vias, pads) |
//! | `short` | Error | copper of different (or unassigned) nets overlapping |
//! | `min_track_width` | Error | a track thinner than the board minimum |
//! | `via_diameter` / `via_drill` / `annular_ring` | Error | via smaller than the minimum, or too little copper around its drill |
//! | `copper_to_edge` | Error | copper closer to `Edge.Cuts` than allowed |
//! | `outside_board` | Error | copper outside the closed board outline |
//! | `unconnected` | Error | a ratsnest connection that is not routed |
//! | `courtyard_overlap` | Warning | courtyards of footprints on the same side overlap |
//! | `pair_skew` | Warning | the routed lengths of a differential pair's legs differ by more than its maximum skew (track length per net; vias and stubs of other topologies are not modelled) |
//!
//! The engine is **incremental**: [`DrcEngine::update`] re-checks only the objects a change set
//! touched (against a per-layer R-tree) and the nets it affected; [`run_drc`] is the from-scratch
//! reference and the two are property-tested to agree. Zones are not copper yet (roadmap 25).

use crate::copper::{self, CShape, CopperItem};
use buturecad_eda_core::model::{Id, PcbChange, PcbDoc, PcbObject};
use buturecad_eda_core::ratsnest::{affected_nets, RatsnestEngine};
use buturecad_geometry::Bounded;
use buturecad_geometry::{
    BoundingBox, Coord, Point, PointLocation, Polygon, Segment, SpatialIndex,
};
use buturecad_pcb::{layer, FpShape, LayerId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DrcSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DrcRule {
    Short,
    Clearance,
    MinTrackWidth,
    ViaDiameter,
    ViaDrill,
    AnnularRing,
    CopperToEdge,
    OutsideBoard,
    Unconnected,
    CourtyardOverlap,
    PairSkew,
}

impl DrcRule {
    pub fn code(self) -> &'static str {
        match self {
            DrcRule::Short => "short",
            DrcRule::Clearance => "clearance",
            DrcRule::MinTrackWidth => "min_track_width",
            DrcRule::ViaDiameter => "via_diameter",
            DrcRule::ViaDrill => "via_drill",
            DrcRule::AnnularRing => "annular_ring",
            DrcRule::CopperToEdge => "copper_to_edge",
            DrcRule::OutsideBoard => "outside_board",
            DrcRule::Unconnected => "unconnected",
            DrcRule::CourtyardOverlap => "courtyard_overlap",
            DrcRule::PairSkew => "pair_skew",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            DrcRule::Short => "Short circuit",
            DrcRule::Clearance => "Clearance",
            DrcRule::MinTrackWidth => "Track too thin",
            DrcRule::ViaDiameter => "Via too small",
            DrcRule::ViaDrill => "Via drill too small",
            DrcRule::AnnularRing => "Annular ring too small",
            DrcRule::CopperToEdge => "Copper too close to the edge",
            DrcRule::OutsideBoard => "Copper outside the board",
            DrcRule::Unconnected => "Unconnected net",
            DrcRule::CourtyardOverlap => "Courtyards overlap",
            DrcRule::PairSkew => "Differential pair skew",
        }
    }
    pub fn severity(self) -> DrcSeverity {
        match self {
            DrcRule::CourtyardOverlap | DrcRule::PairSkew => DrcSeverity::Warning,
            _ => DrcSeverity::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrcViolation {
    pub rule: DrcRule,
    pub severity: DrcSeverity,
    pub message: String,
    /// The offending board objects (real ids): select them to see the problem.
    pub objects: Vec<Id>,
    pub at: Point,
    pub layer: Option<LayerId>,
    /// Actual and required value in nm, when the rule has one.
    pub actual: Option<Coord>,
    pub required: Option<Coord>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrcReport {
    pub violations: Vec<DrcViolation>,
}

impl DrcReport {
    pub fn errors(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == DrcSeverity::Error)
            .count()
    }
    pub fn warnings(&self) -> usize {
        self.violations.len() - self.errors()
    }
    pub fn is_clean(&self) -> bool {
        self.errors() == 0
    }
}

fn mm(v: f64) -> f64 {
    v / 1e6
}

// ---------------------------------------------------------------------------------------------
// Board outline
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Outline {
    segs: SpatialIndex<Segment>,
    count: usize,
    /// Closed loops: the largest is the board, loops inside it are cut-outs.
    boards: Vec<Polygon>,
    holes: Vec<Polygon>,
}

fn arc_points(arc: &buturecad_geometry::Arc) -> Vec<Point> {
    let (c, s, e) = (arc.center, arc.start, arc.end);
    let r = arc.radius();
    let a0 = ((s.y - c.y) as f64).atan2((s.x - c.x) as f64);
    let mut a1 = ((e.y - c.y) as f64).atan2((e.x - c.x) as f64);
    if arc.ccw {
        while a1 <= a0 {
            a1 += std::f64::consts::TAU;
        }
    } else {
        while a1 >= a0 {
            a1 -= std::f64::consts::TAU;
        }
    }
    let n = (((a1 - a0).abs() / std::f64::consts::TAU) * 48.0)
        .ceil()
        .max(4.0) as usize;
    let mut pts = vec![s];
    for i in 1..n {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        pts.push(Point::new(
            (c.x as f64 + r * a.cos()).round() as i64,
            (c.y as f64 + r * a.sin()).round() as i64,
        ));
    }
    pts.push(e);
    pts
}

/// The straight pieces of the board outline on `Edge.Cuts` (arcs and circles approximated by
/// polylines of at most 48 segments).
pub fn outline_segments(doc: &PcbDoc) -> Vec<Segment> {
    let mut polylines: Vec<Vec<Point>> = Vec::new();
    for g in doc
        .graphics
        .values()
        .filter(|g| g.layer == layer::EDGE_CUTS)
    {
        match &g.shape {
            FpShape::Line { segment, .. } => polylines.push(vec![segment.a, segment.b]),
            FpShape::Arc { arc, .. } => polylines.push(arc_points(arc)),
            FpShape::Polygon { points, .. } => {
                let mut p = points.clone();
                p.push(points[0]);
                polylines.push(p);
            }
            FpShape::Rect { rect, .. } => {
                let (a, b) = (rect.min(), rect.max());
                polylines.push(vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y), a]);
            }
            FpShape::Circle { circle, .. } => {
                let pts: Vec<Point> = (0..=48)
                    .map(|i| {
                        let a = i as f64 / 48.0 * std::f64::consts::TAU;
                        Point::new(
                            (circle.center.x as f64 + circle.radius as f64 * a.cos()).round()
                                as i64,
                            (circle.center.y as f64 + circle.radius as f64 * a.sin()).round()
                                as i64,
                        )
                    })
                    .collect();
                polylines.push(pts);
            }
        }
    }
    polylines
        .iter()
        .flat_map(|p| p.windows(2).map(|w| Segment::new(w[0], w[1])))
        .filter(|s| s.a != s.b)
        .collect()
}

impl Outline {
    fn build(doc: &PcbDoc) -> Self {
        let mut o = Outline::default();
        let segs = outline_segments(doc);
        o.count = segs.len();
        o.segs = SpatialIndex::bulk_load(segs.iter().map(|s| {
            (
                BoundingBox::new(
                    Point::new(s.a.x.min(s.b.x), s.a.y.min(s.b.y)),
                    Point::new(s.a.x.max(s.b.x), s.a.y.max(s.b.y)),
                ),
                *s,
            )
        }));
        // Chain segments into closed loops (every end must have exactly one partner).
        let mut by_end: BTreeMap<Point, Vec<usize>> = BTreeMap::new();
        for (i, s) in segs.iter().enumerate() {
            by_end.entry(s.a).or_default().push(i);
            by_end.entry(s.b).or_default().push(i);
        }
        if by_end.values().all(|v| v.len() == 2) {
            let mut used = vec![false; segs.len()];
            for start in 0..segs.len() {
                if used[start] {
                    continue;
                }
                let mut pts = vec![segs[start].a];
                let mut cur = segs[start].b;
                used[start] = true;
                while cur != pts[0] {
                    pts.push(cur);
                    let next = by_end[&cur].iter().copied().find(|i| !used[*i]);
                    let Some(i) = next else { break };
                    used[i] = true;
                    cur = if segs[i].a == cur {
                        segs[i].b
                    } else {
                        segs[i].a
                    };
                }
                if cur == pts[0] && pts.len() >= 3 {
                    if let Ok(p) = Polygon::new(pts) {
                        o.holes.push(p);
                    }
                }
            }
            o.holes.sort_by(|a, b| {
                b.area()
                    .partial_cmp(&a.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if !o.holes.is_empty() {
                let outer = o.holes.remove(0);
                o.boards.push(outer);
            }
        }
        o
    }

    /// Is `p` on the board (inside the outer loop and not in a cut-out)? `None` without a closed
    /// outline.
    fn contains(&self, p: Point) -> Option<bool> {
        let outer = self.boards.first()?;
        Some(
            outer.locate(p) != PointLocation::Outside
                && !self
                    .holes
                    .iter()
                    .any(|h| h.locate(p) == PointLocation::Inside),
        )
    }

    fn nearest(&self, shape: &CShape, within: Coord) -> Option<(f64, Point)> {
        let bb = match shape {
            CShape::Cap { seg, r } => {
                let b = BoundingBox::new(
                    Point::new(seg.a.x.min(seg.b.x), seg.a.y.min(seg.b.y)),
                    Point::new(seg.a.x.max(seg.b.x), seg.a.y.max(seg.b.y)),
                );
                b.inflate(*r as Coord + within)?
            }
            CShape::Poly(p) => p.bounding_box().inflate(within)?,
        };
        self.segs
            .query_intersecting(&bb)
            .map(|(_, s)| (copper::distance_to_segment(shape, s), s.a))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ---------------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------------

struct Courtyard {
    back: bool,
    polys: Vec<Polygon>,
    bbox: BoundingBox,
}

/// Incremental DRC state.
pub struct DrcEngine {
    items: BTreeMap<Id, Vec<CopperItem>>,
    index: BTreeMap<LayerId, SpatialIndex<(Id, usize)>>,
    pair_v: Vec<DrcViolation>,
    single_v: Vec<DrcViolation>,
    unconnected: BTreeMap<Id, Vec<DrcViolation>>,
    court: BTreeMap<Id, Courtyard>,
    court_v: Vec<DrcViolation>,
    skew_v: Vec<DrcViolation>,
    rats: RatsnestEngine,
    outline: Outline,
    max_clearance: Coord,
}

fn vio(
    rule: DrcRule,
    message: String,
    mut objects: Vec<Id>,
    at: Point,
    layer: Option<LayerId>,
    actual: Option<Coord>,
    required: Option<Coord>,
) -> DrcViolation {
    objects.sort();
    objects.dedup();
    let fingerprint = format!(
        "{}|{}|{}",
        rule.code(),
        objects
            .iter()
            .map(|i| i.0.to_string())
            .collect::<Vec<_>>()
            .join(","),
        layer.map_or(String::new(), |l| l.0.to_string())
    );
    DrcViolation {
        rule,
        severity: rule.severity(),
        message,
        objects,
        at,
        layer,
        actual,
        required,
        fingerprint,
    }
}

fn mid(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) / 2, (a.y + b.y) / 2)
}

fn center(b: &BoundingBox) -> Point {
    mid(b.min, b.max)
}

impl DrcEngine {
    pub fn new(doc: &PcbDoc) -> Self {
        let mut e = Self {
            items: BTreeMap::new(),
            index: BTreeMap::new(),
            pair_v: Vec::new(),
            single_v: Vec::new(),
            unconnected: BTreeMap::new(),
            court: BTreeMap::new(),
            court_v: Vec::new(),
            skew_v: Vec::new(),
            rats: RatsnestEngine::default(),
            outline: Outline::default(),
            max_clearance: 0,
        };
        e.refresh(doc);
        e
    }

    /// Re-checks everything.
    pub fn refresh(&mut self, doc: &PcbDoc) {
        self.items.clear();
        self.index.clear();
        self.pair_v.clear();
        self.single_v.clear();
        self.unconnected.clear();
        self.court.clear();
        self.court_v.clear();
        self.skew_v = skew_violations(doc);
        self.outline = Outline::build(doc);
        self.max_clearance = doc
            .rules
            .classes
            .values()
            .map(|c| c.clearance)
            .max()
            .unwrap_or(0);
        let ids: Vec<Id> = doc.ids().collect();
        for id in &ids {
            self.add_items(doc, *id);
        }
        let mut seen = BTreeSet::new();
        for id in &ids {
            self.check_owner(doc, *id, &mut seen);
        }
        self.rats = RatsnestEngine::new(doc);
        for net in self.rats.nets().collect::<Vec<_>>() {
            self.unconnected.insert(net, self.unconnected_for(doc, net));
        }
        for id in doc.placed.keys() {
            self.add_courtyard(doc, *id);
        }
        let mut seen = BTreeSet::new();
        for id in doc.placed.keys().copied().collect::<Vec<_>>() {
            self.check_courtyard(id, &mut seen);
        }
    }

    fn add_items(&mut self, doc: &PcbDoc, id: Id) {
        let items = copper::items_of(doc, id);
        if items.is_empty() {
            return;
        }
        for (i, it) in items.iter().enumerate() {
            self.index
                .entry(it.layer)
                .or_default()
                .insert(it.bbox(), (id, i));
        }
        self.items.insert(id, items);
    }

    fn remove_items(&mut self, id: Id) {
        if let Some(items) = self.items.remove(&id) {
            for (i, it) in items.iter().enumerate() {
                if let Some(ix) = self.index.get_mut(&it.layer) {
                    ix.remove(&it.bbox(), (id, i));
                }
            }
        }
    }

    /// Single-object rules and pair rules for every item of `id` against the index.
    fn check_owner(&mut self, doc: &PcbDoc, id: Id, seen: &mut BTreeSet<(Id, usize, Id, usize)>) {
        self.check_single(doc, id);
        let Some(items) = self.items.get(&id).cloned() else {
            return;
        };
        for (i, a) in items.iter().enumerate() {
            let reach = self.max_clearance + 1;
            let Some(ix) = self.index.get(&a.layer) else {
                continue;
            };
            let Some(area) = a.bbox().inflate(reach) else {
                continue;
            };
            let neighbours: Vec<(Id, usize)> =
                ix.query_intersecting(&area).map(|(_, v)| *v).collect();
            for (oid, oi) in neighbours {
                if (oid, oi) == (id, i) {
                    continue;
                }
                let key = if (id, i) < (oid, oi) {
                    (id, i, oid, oi)
                } else {
                    (oid, oi, id, i)
                };
                if !seen.insert(key) {
                    continue;
                }
                let Some(b) = self.items.get(&oid).and_then(|v| v.get(oi)) else {
                    continue;
                };
                if oid == id && a.kind != copper::ItemKind::Pad {
                    continue;
                }
                if a.net.is_some() && a.net == b.net {
                    continue;
                }
                if let Some(v) = pair_violation(doc, a, b) {
                    self.pair_v.push(v);
                }
            }
        }
    }

    fn check_single(&mut self, doc: &PcbDoc, id: Id) {
        let rules = &doc.rules;
        match doc.object(id) {
            Some(PcbObject::Track(t)) if t.width < rules.min_track_width => {
                self.single_v.push(vio(
                    DrcRule::MinTrackWidth,
                    format!(
                        "Track is {:.3} mm wide; the minimum is {:.3} mm",
                        mm(t.width as f64),
                        mm(rules.min_track_width as f64)
                    ),
                    vec![id],
                    mid(t.segment.a, t.segment.b),
                    Some(t.layer),
                    Some(t.width),
                    Some(rules.min_track_width),
                ))
            }
            Some(PcbObject::Via(v)) => {
                if v.diameter < rules.min_via_diameter {
                    self.single_v.push(vio(
                        DrcRule::ViaDiameter,
                        format!(
                            "Via is {:.3} mm; the minimum is {:.3} mm",
                            mm(v.diameter as f64),
                            mm(rules.min_via_diameter as f64)
                        ),
                        vec![id],
                        v.position,
                        None,
                        Some(v.diameter),
                        Some(rules.min_via_diameter),
                    ));
                }
                if v.drill < rules.min_via_drill {
                    self.single_v.push(vio(
                        DrcRule::ViaDrill,
                        format!(
                            "Via drill is {:.3} mm; the minimum is {:.3} mm",
                            mm(v.drill as f64),
                            mm(rules.min_via_drill as f64)
                        ),
                        vec![id],
                        v.position,
                        None,
                        Some(v.drill),
                        Some(rules.min_via_drill),
                    ));
                }
                let ring = (v.diameter - v.drill) / 2;
                if ring < rules.min_annular_ring {
                    self.single_v.push(vio(
                        DrcRule::AnnularRing,
                        format!(
                            "Via annular ring is {:.3} mm; the minimum is {:.3} mm",
                            mm(ring as f64),
                            mm(rules.min_annular_ring as f64)
                        ),
                        vec![id],
                        v.position,
                        None,
                        Some(ring),
                        Some(rules.min_annular_ring),
                    ));
                }
            }
            _ => {}
        }
        // Board edge.
        let Some(items) = self.items.get(&id) else {
            return;
        };
        if self.outline.count == 0 {
            return;
        }
        let mut found: Vec<DrcViolation> = Vec::new();
        let mut reported_layers: BTreeSet<LayerId> = BTreeSet::new();
        for it in items {
            if reported_layers.contains(&it.layer) {
                continue;
            }
            let at = center(&it.bbox());
            if self.outline.contains(it.anchor()) == Some(false) {
                found.push(vio(
                    DrcRule::OutsideBoard,
                    format!("{} is outside the board outline", it.label),
                    vec![id],
                    at,
                    Some(it.layer),
                    None,
                    None,
                ));
                reported_layers.insert(it.layer);
                continue;
            }
            if let Some((d, _)) = self.outline.nearest(&it.shape, rules.copper_to_edge) {
                if d < rules.copper_to_edge as f64 {
                    found.push(vio(
                        DrcRule::CopperToEdge,
                        format!(
                            "{} is {:.3} mm from the board edge; {:.3} mm required",
                            it.label,
                            mm(d.max(0.0)),
                            mm(rules.copper_to_edge as f64)
                        ),
                        vec![id],
                        at,
                        Some(it.layer),
                        Some(d.max(0.0).round() as Coord),
                        Some(rules.copper_to_edge),
                    ));
                    reported_layers.insert(it.layer);
                }
            }
        }
        self.single_v.extend(found);
    }

    fn unconnected_for(&self, doc: &PcbDoc, net: Id) -> Vec<DrcViolation> {
        let name = doc.nets.get(&net).map_or("?", |n| n.name.as_str());
        let owner_at = |p: Point| -> Option<Id> {
            self.items.iter().find_map(|(id, its)| {
                its.iter()
                    .find(|it| {
                        it.net == Some(net)
                            && copper::distance(
                                &it.shape,
                                &CShape::Cap {
                                    seg: Segment::new(p, p),
                                    r: 0.0,
                                },
                            ) <= 0.0
                    })
                    .map(|_| *id)
            })
        };
        self.rats
            .net_lines(net)
            .iter()
            .map(|l| {
                let objs: Vec<Id> = [owner_at(l.a), owner_at(l.b)]
                    .into_iter()
                    .flatten()
                    .collect();
                vio(
                    DrcRule::Unconnected,
                    format!(
                        "Net {name} is not fully routed ({:.2} mm still to connect)",
                        mm(l.length())
                    ),
                    if objs.is_empty() { vec![net] } else { objs },
                    mid(l.a, l.b),
                    None,
                    None,
                    None,
                )
            })
            .collect()
    }

    fn add_courtyard(&mut self, doc: &PcbDoc, id: Id) {
        let Some(f) = doc.placed.get(&id) else { return };
        let Some(def) = doc.footprints.get(&f.footprint) else {
            return;
        };
        let polys: Vec<Polygon> = f
            .world_graphics(def)
            .into_iter()
            .filter(|g| layer::kind(g.layer) == Some(layer::LayerKind::Courtyard))
            .filter_map(|g| match g.shape {
                FpShape::Polygon { points, .. } => Polygon::new(points).ok(),
                _ => None,
            })
            .collect();
        if polys.is_empty() {
            return;
        }
        let mut bbox = polys[0].bounding_box();
        for p in &polys[1..] {
            bbox = bbox.union(&p.bounding_box());
        }
        self.court.insert(
            id,
            Courtyard {
                back: f.placement.flipped,
                polys,
                bbox,
            },
        );
    }

    fn check_courtyard(&mut self, id: Id, seen: &mut BTreeSet<(Id, Id)>) {
        let Some(a) = self.court.get(&id) else { return };
        let mut found = Vec::new();
        for (oid, b) in &self.court {
            if *oid == id || b.back != a.back || !a.bbox.intersects(&b.bbox) {
                continue;
            }
            let key = if id < *oid { (id, *oid) } else { (*oid, id) };
            if !seen.insert(key) {
                continue;
            }
            // Touching courtyards are fine; only real overlap counts.
            let overlap = a.polys.iter().any(|p| {
                b.polys.iter().any(|q| {
                    p.offset(-1000)
                        .map_or_else(|_| p.collides(q), |s| s.collides(q))
                })
            });
            if overlap {
                found.push(vio(
                    DrcRule::CourtyardOverlap,
                    "The courtyards of two footprints overlap".into(),
                    vec![id, *oid],
                    center(&a.bbox.intersection(&b.bbox).unwrap_or(a.bbox)),
                    None,
                    None,
                    None,
                ));
            }
        }
        self.court_v.extend(found);
    }

    /// Re-checks what `changes` touched. Changes to the board outline or anything that is not a
    /// board object (rules) need [`Self::refresh`]; this falls back to it automatically for
    /// outline edits.
    pub fn update(&mut self, doc: &PcbDoc, changes: &[PcbChange]) {
        let touches_edge = changes.iter().any(|c| {
            let is_edge = |o: &Option<PcbObject>| matches!(o, Some(PcbObject::Graphic(g)) if g.layer == layer::EDGE_CUTS);
            is_edge(&c.before) || is_edge(&c.after)
        });
        if touches_edge {
            self.refresh(doc);
            return;
        }
        let ids: BTreeSet<Id> = changes.iter().map(|c| c.id).collect();
        for id in &ids {
            self.remove_items(*id);
            self.court.remove(id);
        }
        let mentions = |v: &DrcViolation| v.objects.iter().any(|o| ids.contains(o));
        self.pair_v.retain(|v| !mentions(v));
        self.single_v.retain(|v| !mentions(v));
        self.court_v.retain(|v| !mentions(v));
        for id in &ids {
            self.add_items(doc, *id);
            self.add_courtyard(doc, *id);
        }
        let mut seen = BTreeSet::new();
        for id in &ids {
            if doc.object(*id).is_some() {
                self.check_owner(doc, *id, &mut seen);
            }
        }
        let mut seen = BTreeSet::new();
        for id in &ids {
            self.check_courtyard(*id, &mut seen);
        }
        self.skew_v = skew_violations(doc);
        let dirty = affected_nets(changes);
        self.rats.update(doc, &dirty);
        for net in &dirty {
            self.unconnected.remove(net);
        }
        // Items moved: other nets' violations that mention changed copper were dropped with the
        // generic filter above only if they listed an id; unconnected ones list pad owners.
        self.unconnected.retain(|_, vs| !vs.iter().any(&mentions));
        let nets: Vec<Id> = self
            .rats
            .nets()
            .filter(|n| !self.unconnected.contains_key(n))
            .collect();
        for net in nets {
            let v = self.unconnected_for(doc, net);
            self.unconnected.insert(net, v);
        }
        self.unconnected.retain(|n, _| doc.nets.contains_key(n));
    }

    pub fn report(&self) -> DrcReport {
        let mut v: Vec<DrcViolation> = self
            .pair_v
            .iter()
            .chain(&self.single_v)
            .chain(self.unconnected.values().flatten())
            .chain(&self.court_v)
            .chain(&self.skew_v)
            .cloned()
            .collect();
        v.sort_by(|a, b| {
            (a.severity, a.rule, &a.fingerprint, &a.message).cmp(&(
                b.severity,
                b.rule,
                &b.fingerprint,
                &b.message,
            ))
        });
        v.dedup();
        DrcReport { violations: v }
    }
}

/// The clearance/short verdict for two copper items on one layer.
fn pair_violation(doc: &PcbDoc, a: &CopperItem, b: &CopperItem) -> Option<DrcViolation> {
    let required = doc.rules.clearance_between(a.net, b.net);
    let d = copper::distance(&a.shape, &b.shape);
    // 1 nm of slack: coordinates are rounded to nanometres.
    if d + 1.0 >= required as f64 {
        return None;
    }
    let at = mid(center(&a.bbox()), center(&b.bbox()));
    let objs = vec![a.owner, b.owner];
    let lname = layer::name(a.layer);
    if d <= 0.0 {
        return Some(vio(
            DrcRule::Short,
            format!("{} and {} overlap on {lname}", a.label, b.label),
            objs,
            at,
            Some(a.layer),
            Some(0),
            Some(required),
        ));
    }
    Some(vio(
        DrcRule::Clearance,
        format!(
            "{} and {} are {:.3} mm apart on {lname}; {:.3} mm required",
            a.label,
            b.label,
            mm(d),
            mm(required as f64)
        ),
        objs,
        at,
        Some(a.layer),
        Some(d.round() as Coord),
        Some(required),
    ))
}

/// Checks a whole board from scratch.
pub fn run_drc(doc: &PcbDoc) -> DrcReport {
    DrcEngine::new(doc).report()
}

/// Would adding/replacing the copper of these items violate clearance or short rules against the
/// rest of the board? Used by the router to validate a candidate route before committing it.
pub fn check_candidate(
    doc: &PcbDoc,
    candidate: &[CopperItem],
    ignore: &BTreeSet<Id>,
) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    for id in doc
        .tracks
        .keys()
        .chain(doc.vias.keys())
        .chain(doc.placed.keys())
        .copied()
    {
        if ignore.contains(&id) {
            continue;
        }
        for other in copper::items_of(doc, id) {
            for c in candidate.iter().filter(|c| c.layer == other.layer) {
                if c.net.is_some() && c.net == other.net {
                    continue;
                }
                if let Some(v) = pair_violation(doc, c, &other) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// Edge-related violations (copper-to-edge, outside board) of candidate copper, for the router.
pub fn candidate_edge_violations(doc: &PcbDoc, candidate: &[CopperItem]) -> Vec<DrcViolation> {
    let outline = Outline::build(doc);
    if outline.count == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for it in candidate {
        let at = center(&it.bbox());
        if outline.contains(it.anchor()) == Some(false) {
            out.push(vio(
                DrcRule::OutsideBoard,
                format!("{} is outside the board outline", it.label),
                vec![it.owner],
                at,
                Some(it.layer),
                None,
                None,
            ));
        } else if let Some((d, _)) = outline.nearest(&it.shape, doc.rules.copper_to_edge) {
            if d < doc.rules.copper_to_edge as f64 {
                out.push(vio(
                    DrcRule::CopperToEdge,
                    format!(
                        "{} is {:.3} mm from the board edge; {:.3} mm required",
                        it.label,
                        mm(d.max(0.0)),
                        mm(doc.rules.copper_to_edge as f64)
                    ),
                    vec![it.owner],
                    at,
                    Some(it.layer),
                    Some(d.max(0.0).round() as Coord),
                    Some(doc.rules.copper_to_edge),
                ));
            }
        }
    }
    out
}

/// `pair_skew`: routed length per leg (sum of the net's track lengths) compared with the pair's
/// maximum skew. Pairs where a leg has no track yet are not judged (the unrouted connection is
/// already reported).
fn skew_violations(doc: &PcbDoc) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    for pair in doc.rules.diff_pairs.values() {
        let stats = |net: Id| -> Option<(f64, Id, Point)> {
            let mut total = 0.0;
            let mut longest: Option<(f64, Id, Point)> = None;
            for (id, t) in doc.tracks.iter().filter(|(_, t)| t.net == Some(net)) {
                let l = t.segment.length();
                total += l;
                if longest.as_ref().is_none_or(|b| l > b.0) {
                    longest = Some((l, *id, mid(t.segment.a, t.segment.b)));
                }
            }
            longest.map(|(_, id, at)| (total, id, at))
        };
        let (Some((lp, ip, at)), Some((ln, in_, _))) = (stats(pair.positive), stats(pair.negative))
        else {
            continue;
        };
        let skew = (lp - ln).abs();
        if skew > pair.max_skew as f64 {
            out.push(vio(
                DrcRule::PairSkew,
                format!(
                    "Pair {}: legs are {:.3} mm and {:.3} mm long, {:.3} mm apart; maximum {:.3} mm",
                    pair.name,
                    mm(lp),
                    mm(ln),
                    mm(skew),
                    mm(pair.max_skew as f64)
                ),
                vec![ip, in_],
                at,
                None,
                Some(skew.round() as Coord),
                Some(pair.max_skew),
            ));
        }
    }
    out
}
