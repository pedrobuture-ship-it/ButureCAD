//! Push & Shove: a new route may displace existing tracks of other nets.
//!
//! Model (kept deliberately small and checked exactly):
//!
//! * every track of another net on a copper layer is *movable*; pads, existing vias, the board
//!   edge, keepouts and tracks of the routed net are *fixed*;
//! * a *pusher* (the new copper, then every track that has been moved) closer than the required
//!   clearance to a movable track translates that track along the line joining their closest
//!   points by exactly the missing distance (plus 2 µm). Endpoints that meet another track of the
//!   same net move together, so connected copper stretches instead of tearing;
//! * an endpoint that sits on a pad or via stays; a track anchored at one end pivots about it, a
//!   track anchored at both ends cannot be pushed;
//! * pushed tracks push others in turn until nothing moves (capped);
//! * the final state is **re-verified with the DRC rules**, independently of the heuristic above.
//!   Any failure is an error — the router never returns geometry that breaks a rule.
//!
//! Limits (documented in `docs/routing.md`): vias and pads are never pushed, tracks are never
//! re-routed around a fixed obstacle (no new vertices are added), and a track that crosses the new
//! route cannot be slid out of the way.

use buturecad_eda_core::model::{Id, PcbDoc};
use buturecad_geometry::{BoundingBox, Coord, Point, Segment, SpatialIndex};
use buturecad_pcb::LayerId;
use buturecad_rules::copper::{self, CShape, CopperItem, ItemKind};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Hard cap on push operations per call (keeps the interactive preview responsive).
pub const MAX_PUSHES: usize = 600;
/// Extra distance added to every push so rounding to nanometres never leaves a violation.
const MARGIN: f64 = 2_000.0;

/// New copper that pushes.
pub struct NewCopper<'a> {
    pub net: Option<Id>,
    pub width: Coord,
    pub tracks: &'a [(LayerId, Vec<Point>)],
    pub vias: &'a [Point],
    pub via_diameter: Coord,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShoveResult {
    /// Tracks whose geometry changed, with the new segment.
    pub moved: Vec<(Id, Segment)>,
    /// Tracks that collapsed to a point and are removed.
    pub removed: Vec<Id>,
    /// Number of push operations.
    pub pushes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShoveError {
    /// Pushing would break a rule or hit something fixed; the text says what.
    Unresolvable(String),
    TooComplex,
}

impl fmt::Display for ShoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShoveError::Unresolvable(s) => write!(f, "{s}"),
            ShoveError::TooComplex => write!(f, "too many tracks would have to move"),
        }
    }
}

impl std::error::Error for ShoveError {}

fn unresolvable<T>(s: impl Into<String>) -> Result<T, ShoveError> {
    Err(ShoveError::Unresolvable(s.into()))
}

#[derive(Clone)]
struct TrackState {
    layer: LayerId,
    net: Option<Id>,
    width: Coord,
    seg: Segment,
    orig: Segment,
}

impl TrackState {
    fn shape(&self) -> CShape {
        CShape::Cap {
            seg: self.seg,
            r: self.width as f64 / 2.0,
        }
    }
}

struct Pusher {
    /// The track this pusher is (so it does not push itself); `None` for the new copper.
    id: Option<Id>,
    layer: LayerId,
    net: Option<Id>,
    seg: Segment,
    r: f64,
}

fn seg_bbox(s: &Segment, grow: Coord) -> BoundingBox {
    let b = BoundingBox::new(
        Point::new(s.a.x.min(s.b.x), s.a.y.min(s.b.y)),
        Point::new(s.a.x.max(s.b.x), s.a.y.max(s.b.y)),
    );
    b.inflate(grow).unwrap_or(b)
}

type P2 = (f64, f64);

fn project(p: P2, s: &Segment) -> (P2, f64) {
    let (ax, ay) = (s.a.x as f64, s.a.y as f64);
    let (dx, dy) = ((s.b.x - s.a.x) as f64, (s.b.y - s.a.y) as f64);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - ax) * dx + (p.1 - ay) * dy) / l2).clamp(0.0, 1.0)
    };
    ((ax + t * dx, ay + t * dy), t)
}

/// Closest points of two non-crossing segments: `(on a, on b, parameter of the point on b)`.
fn closest(a: &Segment, b: &Segment) -> (P2, P2, f64) {
    let pt = |p: Point| (p.x as f64, p.y as f64);
    let mut best: Option<(f64, P2, P2, f64)> = None;
    let mut consider = |pa: P2, pb: P2, tb: f64| {
        let d = (pa.0 - pb.0).hypot(pa.1 - pb.1);
        if best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, pa, pb, tb));
        }
    };
    for e in [a.a, a.b] {
        let (q, t) = project(pt(e), b);
        consider(pt(e), q, t);
    }
    for e in [b.a, b.b] {
        let (q, _) = project(pt(e), a);
        let tb = if e == b.a { 0.0 } else { 1.0 };
        consider(q, pt(e), tb);
    }
    match best {
        Some((_, pa, pb, tb)) => (pa, pb, tb),
        None => (pt(a.a), pt(b.a), 0.0),
    }
}

struct Fixed {
    layer: Option<LayerId>,
    shape: CShape,
    net: Option<Id>,
    /// Required distance when it does not depend on the nets (board edge, keepout).
    required: Option<Coord>,
    anchor: bool,
    what: String,
}

/// `(layer, net, point)` → the track ends that meet there.
type Joints = BTreeMap<(LayerId, Option<Id>, Point), Vec<(Id, u8)>>;

struct Shover<'a> {
    doc: &'a PcbDoc,
    new: &'a NewCopper<'a>,
    tracks: BTreeMap<Id, TrackState>,
    tindex: BTreeMap<LayerId, SpatialIndex<Id>>,
    fixed: Vec<Fixed>,
    findex: SpatialIndex<usize>,
    /// (layer, net, point) → track ends that meet there.
    joints: Joints,
    queue: VecDeque<Pusher>,
    pushes: usize,
    max_clearance: Coord,
}

impl<'a> Shover<'a> {
    fn new(doc: &'a PcbDoc, new: &'a NewCopper<'a>) -> Self {
        let mut tracks = BTreeMap::new();
        let mut tindex: BTreeMap<LayerId, SpatialIndex<Id>> = BTreeMap::new();
        let mut joints = Joints::new();
        let mut fixed: Vec<Fixed> = Vec::new();
        for (id, t) in &doc.tracks {
            let movable = !(new.net.is_some() && t.net == new.net);
            if movable {
                tindex
                    .entry(t.layer)
                    .or_default()
                    .insert(seg_bbox(&t.segment, t.width / 2 + 1), *id);
                joints
                    .entry((t.layer, t.net, t.segment.a))
                    .or_default()
                    .push((*id, 0));
                joints
                    .entry((t.layer, t.net, t.segment.b))
                    .or_default()
                    .push((*id, 1));
                tracks.insert(
                    *id,
                    TrackState {
                        layer: t.layer,
                        net: t.net,
                        width: t.width,
                        seg: t.segment,
                        orig: t.segment,
                    },
                );
            } else {
                fixed.push(Fixed {
                    layer: Some(t.layer),
                    shape: CShape::Cap {
                        seg: t.segment,
                        r: t.width as f64 / 2.0,
                    },
                    net: t.net,
                    required: None,
                    anchor: false,
                    what: "a track of the routed net".into(),
                });
            }
        }
        for id in doc.vias.keys().chain(doc.placed.keys()) {
            for it in copper::items_of(doc, *id) {
                fixed.push(Fixed {
                    layer: Some(it.layer),
                    shape: it.shape,
                    net: it.net,
                    required: None,
                    anchor: true,
                    what: it.label,
                });
            }
        }
        for s in buturecad_rules::outline_segments(doc) {
            fixed.push(Fixed {
                layer: None,
                shape: CShape::Cap { seg: s, r: 0.0 },
                net: None,
                required: Some(doc.rules.copper_to_edge),
                anchor: false,
                what: "the board edge".into(),
            });
        }
        for k in doc.keepouts.values() {
            if k.no_tracks && k.outline.len() >= 3 {
                if let Ok(p) = buturecad_geometry::Polygon::new(k.outline.clone()) {
                    for l in doc.stackup.copper() {
                        if k.layers.is_empty() || k.layers.contains(&l) {
                            fixed.push(Fixed {
                                layer: Some(l),
                                shape: CShape::Poly(p.clone()),
                                net: None,
                                required: Some(0),
                                anchor: false,
                                what: "a keepout".into(),
                            });
                        }
                    }
                }
            }
        }
        let findex = SpatialIndex::bulk_load(fixed.iter().enumerate().map(|(i, f)| {
            let bb = match &f.shape {
                CShape::Cap { seg, r } => seg_bbox(seg, r.ceil() as Coord + 1),
                CShape::Poly(p) => buturecad_geometry::Bounded::bounding_box(p),
            };
            (bb, i)
        }));
        let max_clearance = doc
            .rules
            .classes
            .values()
            .map(|c| c.clearance)
            .max()
            .unwrap_or(0)
            .max(doc.rules.copper_to_edge);
        Self {
            doc,
            new,
            tracks,
            tindex,
            fixed,
            findex,
            joints,
            queue: VecDeque::new(),
            pushes: 0,
            max_clearance,
        }
    }

    fn required(&self, a: Option<Id>, b: Option<Id>) -> Coord {
        self.doc.rules.clearance_between(a, b)
    }

    fn anchored(&self, layer: LayerId, p: Point) -> bool {
        let probe = CShape::Cap {
            seg: Segment::new(p, p),
            r: 0.0,
        };
        self.findex
            .query_intersecting(&BoundingBox::new(p, p))
            .map(|(_, i)| &self.fixed[*i])
            .any(|f| {
                f.anchor && f.layer == Some(layer) && copper::distance(&f.shape, &probe) <= 0.0
            })
    }

    fn seed(&mut self) {
        let half = self.new.width as f64 / 2.0;
        for (layer, poly) in self.new.tracks {
            for w in poly.windows(2) {
                self.queue.push_back(Pusher {
                    id: None,
                    layer: *layer,
                    net: self.new.net,
                    seg: Segment::new(w[0], w[1]),
                    r: half,
                });
            }
        }
        for at in self.new.vias {
            for l in self.doc.stackup.copper() {
                self.queue.push_back(Pusher {
                    id: None,
                    layer: l,
                    net: self.new.net,
                    seg: Segment::new(*at, *at),
                    r: self.new.via_diameter as f64 / 2.0,
                });
            }
        }
    }

    fn run(&mut self) -> Result<(), ShoveError> {
        self.seed();
        while let Some(p) = self.queue.pop_front() {
            let area = seg_bbox(&p.seg, p.r.ceil() as Coord + self.max_clearance + 4_000_000);
            let near: Vec<Id> = match self.tindex.get(&p.layer) {
                Some(ix) => ix.query_intersecting(&area).map(|(_, id)| *id).collect(),
                None => continue,
            };
            let pshape = CShape::Cap { seg: p.seg, r: p.r };
            for id in near {
                if Some(id) == p.id {
                    continue;
                }
                let Some(t) = self.tracks.get(&id) else {
                    continue;
                };
                if p.net.is_some() && p.net == t.net {
                    continue;
                }
                let d = copper::distance(&pshape, &t.shape());
                let required = self.required(p.net, t.net) as f64;
                if d + 0.5 >= required {
                    continue;
                }
                self.push(id, &p, d, required)?;
            }
        }
        Ok(())
    }

    fn push(&mut self, id: Id, p: &Pusher, d: f64, required: f64) -> Result<(), ShoveError> {
        self.pushes += 1;
        if self.pushes > MAX_PUSHES {
            return Err(ShoveError::TooComplex);
        }
        let Some(t) = self.tracks.get(&id).cloned() else {
            return Ok(());
        };
        if p.seg.intersects(&t.seg) {
            return unresolvable(
                "a track of another net crosses the new route and cannot be pushed aside",
            );
        }
        let (pp, qq, tb) = closest(&p.seg, &t.seg);
        let (mut ux, mut uy) = (qq.0 - pp.0, qq.1 - pp.1);
        let len = ux.hypot(uy);
        if len < 1.0 {
            // Touching centre lines (d ≤ 0 without crossing): push along the pusher's normal.
            let (dx, dy) = (
                (p.seg.b.x - p.seg.a.x) as f64,
                (p.seg.b.y - p.seg.a.y) as f64,
            );
            let nl = dx.hypot(dy);
            if nl == 0.0 {
                return unresolvable("a track of another net lies exactly on the new route");
            }
            let (nx, ny) = (-dy / nl, dx / nl);
            let mid = (
                (t.seg.a.x + t.seg.b.x) as f64 / 2.0,
                (t.seg.a.y + t.seg.b.y) as f64 / 2.0,
            );
            let s = (mid.0 - pp.0) * nx + (mid.1 - pp.1) * ny;
            (ux, uy) = if s >= 0.0 { (nx, ny) } else { (-nx, -ny) };
        } else {
            ux /= len;
            uy /= len;
        }
        let need = required - d + MARGIN;
        let (a_fixed, b_fixed) = (
            self.anchored(t.layer, t.seg.a),
            self.anchored(t.layer, t.seg.b),
        );
        let (move_a, move_b, scale) = match (a_fixed, b_fixed) {
            (true, true) => {
                return unresolvable(
                    "a track anchored on pads or vias at both ends cannot be pushed",
                )
            }
            (false, false) => (true, true, 1.0),
            (true, false) => (false, true, tb.max(0.0)),
            (false, true) => (true, false, (1.0 - tb).max(0.0)),
        };
        if scale < 0.2 {
            return unresolvable("a track would have to pivot too close to the pad it starts on");
        }
        let (tx, ty) = (ux * need / scale, uy * need / scale);
        let shift = |p: Point| {
            Point::new(
                (p.x as f64 + tx).round() as i64,
                (p.y as f64 + ty).round() as i64,
            )
        };
        let mut changed: BTreeSet<Id> = BTreeSet::new();
        if move_a {
            self.move_joint(id, 0, shift(t.seg.a), &mut changed);
        }
        if move_b {
            self.move_joint(id, 1, shift(t.seg.b), &mut changed);
        }
        for cid in changed {
            self.after_move(cid)?;
        }
        Ok(())
    }

    /// Moves the joint that track end `(id, end)` belongs to (every end meeting there) to `to`.
    fn move_joint(&mut self, id: Id, end: u8, to: Point, changed: &mut BTreeSet<Id>) {
        let Some(t) = self.tracks.get(&id) else {
            return;
        };
        let from = if end == 0 { t.seg.a } else { t.seg.b };
        let key = (t.layer, t.net, from);
        let members = self.joints.remove(&key).unwrap_or_else(|| vec![(id, end)]);
        for (mid, mend) in &members {
            let Some(m) = self.tracks.get_mut(mid) else {
                continue;
            };
            if let Some(ix) = self.tindex.get_mut(&m.layer) {
                ix.remove(&seg_bbox(&m.seg, m.width / 2 + 1), *mid);
            }
            if *mend == 0 {
                m.seg.a = to;
            } else {
                m.seg.b = to;
            }
            if let Some(ix) = self.tindex.get_mut(&m.layer) {
                ix.insert(seg_bbox(&m.seg, m.width / 2 + 1), *mid);
            }
            changed.insert(*mid);
        }
        self.joints
            .entry((key.0, key.1, to))
            .or_default()
            .extend(members);
    }

    /// A track moved: it becomes a pusher and must not hit anything fixed.
    fn after_move(&mut self, id: Id) -> Result<(), ShoveError> {
        let Some(t) = self.tracks.get(&id) else {
            return Ok(());
        };
        let shape = t.shape();
        let bb = seg_bbox(&t.seg, t.width / 2 + self.max_clearance + 1);
        let (layer, net, seg, r) = (t.layer, t.net, t.seg, t.width as f64 / 2.0);
        for (_, i) in self.findex.query_intersecting(&bb) {
            let f = &self.fixed[*i];
            if f.layer.is_some_and(|l| l != layer) {
                continue;
            }
            if net.is_some() && f.net == net {
                continue;
            }
            let required = f.required.unwrap_or_else(|| self.required(net, f.net)) as f64;
            if copper::distance(&f.shape, &shape) < required - 0.5 {
                // A track that merely stays where it was is not the pusher's fault, but a moved
                // track hitting something fixed means this push has no valid result.
                return unresolvable(format!("pushing would move a track into {}", f.what));
            }
        }
        self.queue.push_back(Pusher {
            id: Some(id),
            layer,
            net,
            seg,
            r,
        });
        Ok(())
    }

    fn result(self) -> ShoveResult {
        let mut moved = Vec::new();
        let mut removed = Vec::new();
        for (id, t) in &self.tracks {
            if t.seg == t.orig {
                continue;
            }
            if t.seg.a == t.seg.b {
                removed.push(*id);
            } else {
                moved.push((*id, t.seg));
            }
        }
        ShoveResult {
            moved,
            removed,
            pushes: self.pushes,
        }
    }
}

/// Computes the displacement of existing tracks needed for `new` to fit, or why it cannot.
pub fn shove(doc: &PcbDoc, new: &NewCopper<'_>) -> Result<ShoveResult, ShoveError> {
    let mut s = Shover::new(doc, new);
    s.run()?;
    let result = s.result();
    verify(doc, new, &result)?;
    Ok(result)
}

/// Independent check of the final state with the DRC rules: the new copper and every moved track,
/// at their final positions, against everything that did not move and against each other.
fn verify(doc: &PcbDoc, new: &NewCopper<'_>, r: &ShoveResult) -> Result<(), ShoveError> {
    let gone: BTreeSet<Id> = r
        .moved
        .iter()
        .map(|(i, _)| *i)
        .chain(r.removed.iter().copied())
        .collect();
    let mut cands: Vec<CopperItem> = Vec::new();
    let item = |owner: Id,
                layer: LayerId,
                net: Option<Id>,
                seg: Segment,
                r: f64,
                kind: ItemKind,
                label: &str| CopperItem {
        owner,
        kind,
        net,
        layer,
        shape: CShape::Cap { seg, r },
        label: label.to_owned(),
    };
    let new_owner = Id(u64::MAX);
    for (layer, poly) in new.tracks {
        for w in poly.windows(2) {
            cands.push(item(
                new_owner,
                *layer,
                new.net,
                Segment::new(w[0], w[1]),
                new.width as f64 / 2.0,
                ItemKind::Track,
                "the new track",
            ));
        }
    }
    for at in new.vias {
        for l in doc.stackup.copper() {
            cands.push(item(
                new_owner,
                l,
                new.net,
                Segment::new(*at, *at),
                new.via_diameter as f64 / 2.0,
                ItemKind::Via,
                "the new via",
            ));
        }
    }
    for (id, seg) in &r.moved {
        if let Some(t) = doc.tracks.get(id) {
            cands.push(item(
                *id,
                t.layer,
                t.net,
                *seg,
                t.width as f64 / 2.0,
                ItemKind::Track,
                "a pushed track",
            ));
        }
    }
    let mut bad = buturecad_rules::check_candidate(doc, &cands, &gone);
    bad.extend(buturecad_rules::candidate_edge_violations(doc, &cands));
    if let Some(v) = bad.first() {
        return unresolvable(format!("pushing would break a rule: {}", v.message));
    }
    for (i, a) in cands.iter().enumerate() {
        for b in &cands[i + 1..] {
            if a.layer != b.layer || a.owner == b.owner || (a.net.is_some() && a.net == b.net) {
                continue;
            }
            let required = doc.rules.clearance_between(a.net, b.net) as f64;
            if copper::distance(&a.shape, &b.shape) < required - 0.5 {
                return unresolvable("pushed tracks would end up too close to the new route");
            }
        }
    }
    Ok(())
}
