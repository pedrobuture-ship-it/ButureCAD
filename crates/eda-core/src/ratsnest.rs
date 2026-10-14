//! Ratsnest: the "flying wires" of connections that are not routed yet.
//!
//! For each net the copper that *is* connected (pads, tracks and vias of that net that touch on
//! a shared copper layer) forms **islands**; the ratsnest is a minimum spanning tree over the
//! islands using the shortest pad-to-pad (or via) distance, so it shows the cheapest way to
//! finish the net. Different nets never join, so a short between nets is not hidden (DRC will
//! report it, roadmap 19).
//!
//! [`RatsnestEngine`] keeps one result per net and recomputes **only the nets that changed**
//! ([`affected_nets`] derives them from a change set). The filled copper of a **zone** is copper
//! of its net like any other: pads, vias and tracks that touch a piece of the fill are in the same
//! island, so a ground pour completes the connections it really makes.
//!
//! Complexity: islands are found with a spatial index (no all-pairs scan of copper); the
//! spanning tree is exact for nets with up to [`EXACT_LIMIT`] anchors and approximated with
//! k-nearest candidate edges (always completed to a spanning tree) beyond that.

use crate::copper::{self, CShape, Region};
use crate::model::{Id, PcbChange, PcbDoc, PcbObject};
use buturecad_geometry::{BoundingBox, Coord, Point, Segment, SpatialIndex};
use buturecad_pcb::{LayerId, WorldPad};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Nets with more anchors than this use the approximate spanning tree.
pub const EXACT_LIMIT: usize = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatLine {
    pub net: Id,
    pub a: Point,
    pub b: Point,
}

impl RatLine {
    pub fn length(&self) -> f64 {
        self.a.distance(self.b)
    }
}

#[derive(Debug, Clone)]
enum Item {
    Pad(WorldPad),
    Track {
        layer: LayerId,
        seg: Segment,
        width: Coord,
    },
    Via {
        at: Point,
        radius: Coord,
        layers: Vec<LayerId>,
    },
    /// A piece of a zone fill.
    Zone {
        layer: LayerId,
        region: Arc<Region>,
    },
}

impl Item {
    /// The copper shape, for exact overlap tests against a zone.
    fn shape(&self) -> Option<CShape> {
        Some(match self {
            Item::Pad(p) => copper::pad_shape(p)?,
            Item::Track { seg, width, .. } => CShape::Cap {
                seg: *seg,
                r: *width as f64 / 2.0,
            },
            Item::Via { at, radius, .. } => CShape::Cap {
                seg: Segment::new(*at, *at),
                r: *radius as f64,
            },
            Item::Zone { region, .. } => CShape::Region(region.clone()),
        })
    }

    fn bbox(&self) -> BoundingBox {
        let pad = |a: Point, r: Coord| {
            BoundingBox::new(Point::new(a.x - r, a.y - r), Point::new(a.x + r, a.y + r))
        };
        match self {
            Item::Pad(p) => {
                let (lo, hi) = p.bounds();
                BoundingBox::new(lo, hi)
            }
            Item::Track { seg, width, .. } => {
                let b = BoundingBox::new(
                    Point::new(seg.a.x.min(seg.b.x), seg.a.y.min(seg.b.y)),
                    Point::new(seg.a.x.max(seg.b.x), seg.a.y.max(seg.b.y)),
                );
                b.inflate(*width / 2 + 1).unwrap_or(b)
            }
            Item::Via { at, radius, .. } => pad(*at, *radius),
            Item::Zone { region, .. } => region.bbox(),
        }
    }

    fn layers(&self, stack: &buturecad_pcb::Stackup) -> Vec<LayerId> {
        match self {
            Item::Pad(p) => p.copper_layers(stack),
            Item::Track { layer, .. } => vec![*layer],
            Item::Via { layers, .. } => layers.clone(),
            Item::Zone { layer, .. } => vec![*layer],
        }
    }

    /// Distance from the item's copper to a point (0 when covered).
    fn distance(&self, p: Point) -> f64 {
        match self {
            Item::Pad(w) => w.distance_to_point(p),
            Item::Track { seg, width, .. } => {
                (seg.distance_to_point(p) - *width as f64 / 2.0).max(0.0)
            }
            Item::Via { at, radius, .. } => (at.distance(p) - *radius as f64).max(0.0),
            Item::Zone { region, .. } => {
                if region.contains(p) {
                    0.0
                } else {
                    region.boundary_distance(&Segment::new(p, p))
                }
            }
        }
    }

    /// Representative points: where a ratsnest line may end.
    fn anchors(&self) -> Vec<Point> {
        match self {
            Item::Pad(p) => vec![p.center],
            Item::Via { at, .. } => vec![*at],
            Item::Track { seg, .. } => vec![seg.a, seg.b],
            // Pieces of a pour are never the end of a flying wire.
            Item::Zone { .. } => Vec::new(),
        }
    }

    /// Points that decide whether two items touch.
    fn probes(&self) -> Vec<Point> {
        match self {
            Item::Pad(p) => {
                let (lo, hi) = p.bounds();
                vec![
                    p.center,
                    lo,
                    hi,
                    Point::new(lo.x, hi.y),
                    Point::new(hi.x, lo.y),
                ]
            }
            Item::Via { at, .. } => vec![*at],
            Item::Track { seg, .. } => vec![seg.a, seg.b],
            Item::Zone { .. } => Vec::new(),
        }
    }
}

fn touches(a: &Item, b: &Item) -> bool {
    // A zone piece touches whatever overlaps it (exact shape test, not just probe points).
    if matches!(a, Item::Zone { .. }) || matches!(b, Item::Zone { .. }) {
        return match (a.shape(), b.shape()) {
            (Some(x), Some(y)) => copper::distance(&x, &y) <= 0.0,
            _ => false,
        };
    }
    // Two copper items touch if a probe point of one is covered by the other.
    a.probes().into_iter().any(|p| b.distance(p) <= 0.0)
        || b.probes().into_iter().any(|p| a.distance(p) <= 0.0)
        || match (a, b) {
            (
                Item::Track {
                    seg: s, width: w1, ..
                },
                Item::Track {
                    seg: t, width: w2, ..
                },
            ) => s.intersects(t) || s.distance_to_segment(t) <= (*w1 + *w2) as f64 / 2.0,
            _ => false,
        }
}

struct Uf(Vec<usize>);
impl Uf {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// Collects the copper of `net`. `overrides` replaces (or removes, with `None`) objects of the
/// document: used to preview a move without cloning the board.
fn gather(doc: &PcbDoc, net: Id, overrides: &BTreeMap<Id, Option<PcbObject>>) -> Vec<Item> {
    let mut items = Vec::new();
    let stack = &doc.stackup;
    let get = |id: Id| -> Option<PcbObject> {
        match overrides.get(&id) {
            Some(o) => o.clone(),
            None => doc.object(id),
        }
    };
    let mut ids: BTreeSet<Id> = doc.ids().collect();
    ids.extend(overrides.keys().copied());
    for id in ids {
        let Some(o) = get(id) else { continue };
        match o {
            PcbObject::Footprint(f) => {
                let Some(def) = doc.footprints.get(&f.footprint) else {
                    continue;
                };
                for p in f.world_pads(def) {
                    if f.pad_nets.get(&p.number) == Some(&net) {
                        items.push(Item::Pad(p));
                    }
                }
            }
            PcbObject::Track(t) if t.net == Some(net) => items.push(Item::Track {
                layer: t.layer,
                seg: t.segment,
                width: t.width,
            }),
            PcbObject::Via(v) if v.net == Some(net) => items.push(Item::Via {
                at: v.position,
                radius: v.diameter / 2,
                layers: v.kind.layers(stack),
            }),
            PcbObject::Zone(z) if z.net == Some(net) => zone_items(&z, &mut items),
            _ => {}
        }
    }
    items
}

fn zone_items(z: &buturecad_pcb::Zone, items: &mut Vec<Item>) {
    if let Some(fill) = &z.fill {
        for r in &fill.regions {
            if let Some(region) = Region::new(r.outer.clone(), r.holes.clone()) {
                items.push(Item::Zone {
                    layer: z.layer,
                    region: Arc::new(region),
                });
            }
        }
    }
}

/// The ratsnest of one net.
pub fn net_ratsnest(doc: &PcbDoc, net: Id) -> Vec<RatLine> {
    net_ratsnest_with(doc, net, &BTreeMap::new())
}

/// The ratsnest of one net as it would be if `overrides` were applied (live move previews).
pub fn net_ratsnest_with(
    doc: &PcbDoc,
    net: Id,
    overrides: &BTreeMap<Id, Option<PcbObject>>,
) -> Vec<RatLine> {
    solve(doc, net, gather(doc, net, overrides))
}

/// All nets' copper in one pass over the board (used by full refreshes).
fn gather_all(doc: &PcbDoc, only: Option<&BTreeSet<Id>>) -> BTreeMap<Id, Vec<Item>> {
    let wanted = |n: &Id| only.is_none_or(|s| s.contains(n));
    let mut out: BTreeMap<Id, Vec<Item>> = BTreeMap::new();
    let stack = &doc.stackup;
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        for p in f.world_pads(def) {
            if let Some(net) = f.pad_nets.get(&p.number).filter(|n| wanted(n)) {
                out.entry(*net).or_default().push(Item::Pad(p));
            }
        }
    }
    for t in doc.tracks.values() {
        if let Some(net) = t.net.filter(wanted) {
            out.entry(net).or_default().push(Item::Track {
                layer: t.layer,
                seg: t.segment,
                width: t.width,
            });
        }
    }
    for v in doc.vias.values() {
        if let Some(net) = v.net.filter(wanted) {
            out.entry(net).or_default().push(Item::Via {
                at: v.position,
                radius: v.diameter / 2,
                layers: v.kind.layers(stack),
            });
        }
    }
    for z in doc.zones.values() {
        if let Some(net) = z.net.filter(wanted) {
            zone_items(z, out.entry(net).or_default());
        }
    }
    out
}

fn solve(doc: &PcbDoc, net: Id, mut items: Vec<Item>) -> Vec<RatLine> {
    if items.len() < 2 {
        return Vec::new();
    }
    // A canonical order makes the result independent of how the copper was gathered.
    items.sort_by_key(|i| {
        let a = i.anchors();
        let k = match i {
            Item::Pad(_) => 0u8,
            Item::Via { .. } => 1,
            Item::Track { .. } => 2,
            Item::Zone { .. } => 3,
        };
        let lo = i.bbox().min;
        let first = a.first().copied().unwrap_or(lo);
        (
            first.x,
            first.y,
            a.last().map_or(lo.x, |p| p.x),
            a.last().map_or(lo.y, |p| p.y),
            k,
        )
    });
    // Islands via a spatial index of the items.
    let stack = &doc.stackup;
    let layers: Vec<Vec<LayerId>> = items.iter().map(|i| i.layers(stack)).collect();
    let index = SpatialIndex::bulk_load(items.iter().enumerate().map(|(i, it)| (it.bbox(), i)));
    let mut uf = Uf((0..items.len()).collect());
    for (i, it) in items.iter().enumerate() {
        for (_, j) in index.query_intersecting(&it.bbox()) {
            let j = *j;
            if j <= i {
                continue;
            }
            if layers[i].iter().any(|l| layers[j].contains(l)) && touches(it, &items[j]) {
                uf.union(i, j);
            }
        }
    }
    // Anchors per island: pad centres and vias; track ends only when an island has no pad or via.
    let mut islands: BTreeMap<usize, Vec<Point>> = BTreeMap::new();
    let mut fallback: BTreeMap<usize, Vec<Point>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        let r = uf.find(i);
        match it {
            Item::Track { .. } => fallback.entry(r).or_default().extend(it.anchors()),
            Item::Zone { .. } => {}
            _ => islands.entry(r).or_default().extend(it.anchors()),
        }
    }
    let roots: BTreeSet<usize> = (0..items.len()).map(|i| uf.find(i)).collect();
    if roots.len() < 2 {
        return Vec::new();
    }
    let groups: Vec<Vec<Point>> = roots
        .iter()
        .map(|r| {
            islands
                .remove(r)
                .filter(|v| !v.is_empty())
                .or_else(|| fallback.remove(r))
                .unwrap_or_default()
        })
        // A piece of pour that touches nothing with an end to draw to is not a place to connect.
        .filter(|g| !g.is_empty())
        .collect();
    if groups.len() < 2 {
        return Vec::new();
    }
    spanning_tree(net, &groups)
}

fn d2(a: Point, b: Point) -> i128 {
    a.distance_sq(b)
}

/// Minimum spanning tree over groups of anchor points (exact, or k-nearest approximation for
/// large nets). Deterministic: ties break by group index and point order.
fn spanning_tree(net: Id, groups: &[Vec<Point>]) -> Vec<RatLine> {
    let n = groups.len();
    let total: usize = groups.iter().map(Vec::len).sum();
    // Candidate edges: (distance², group i, group j, point a, point b).
    let mut edges: Vec<(i128, usize, usize, Point, Point)> = Vec::new();
    if total <= EXACT_LIMIT {
        for i in 0..n {
            for j in i + 1..n {
                let best = groups[i]
                    .iter()
                    .flat_map(|a| groups[j].iter().map(move |b| (d2(*a, *b), *a, *b)))
                    .min_by_key(|(d, a, b)| (*d, a.x, a.y, b.x, b.y));
                if let Some((d, a, b)) = best {
                    edges.push((d, i, j, a, b));
                }
            }
        }
    } else {
        let index = SpatialIndex::bulk_load(groups.iter().enumerate().flat_map(|(g, pts)| {
            pts.iter()
                .map(move |p| (BoundingBox::from_point(*p), (g, *p)))
        }));
        for (g, pts) in groups.iter().enumerate() {
            for p in pts {
                let mut radius: Coord = 2_000_000;
                loop {
                    let mut near: Vec<(i128, usize, Point)> = index
                        .query_near(*p, radius)
                        .filter(|(_, (h, _))| *h != g)
                        .map(|(_, (h, q))| (d2(*p, *q), *h, *q))
                        .collect();
                    near.sort_by_key(|(d, h, q)| (*d, *h, q.x, q.y));
                    if !near.is_empty() || radius > 1 << 30 {
                        for (d, h, q) in near.into_iter().take(6) {
                            let (i, j, a, b) = if g < h { (g, h, *p, q) } else { (h, g, q, *p) };
                            edges.push((d, i, j, a, b));
                        }
                        break;
                    }
                    radius *= 4;
                }
            }
        }
    }
    edges.sort_by_key(|(d, i, j, a, b)| (*d, *i, *j, a.x, a.y, b.x, b.y));
    let mut uf = Uf((0..n).collect());
    let mut out = Vec::new();
    for (_, i, j, a, b) in &edges {
        if uf.find(*i) != uf.find(*j) {
            uf.union(*i, *j);
            out.push(RatLine { net, a: *a, b: *b });
        }
    }
    // The approximation may leave islands unconnected: join them through their closest anchors.
    let mut roots: BTreeMap<usize, usize> = BTreeMap::new();
    for g in 0..n {
        let r = uf.find(g);
        roots.entry(r).or_insert(g);
    }
    let reps: Vec<usize> = roots.values().copied().collect();
    for w in reps.windows(2) {
        if uf.find(w[0]) == uf.find(w[1]) {
            continue;
        }
        let best = groups[w[0]]
            .iter()
            .flat_map(|a| groups[w[1]].iter().map(move |b| (d2(*a, *b), *a, *b)))
            .min_by_key(|(d, a, b)| (*d, a.x, a.y, b.x, b.y));
        if let Some((_, a, b)) = best {
            out.push(RatLine { net, a, b });
            uf.union(w[0], w[1]);
        }
    }
    out
}

/// Nets whose ratsnest may change because of `changes`.
pub fn affected_nets(changes: &[PcbChange]) -> BTreeSet<Id> {
    fn nets_of(o: &Option<PcbObject>) -> Vec<Id> {
        match o {
            Some(PcbObject::Footprint(f)) => f.pad_nets.values().copied().collect(),
            Some(PcbObject::Track(t)) => t.net.into_iter().collect(),
            Some(PcbObject::Via(v)) => v.net.into_iter().collect(),
            Some(PcbObject::Zone(z)) => z.net.into_iter().collect(),
            _ => Vec::new(),
        }
    }
    let mut out = BTreeSet::new();
    for c in changes {
        out.extend(nets_of(&c.before));
        out.extend(nets_of(&c.after));
        if matches!(&c.before, Some(PcbObject::Net(_)))
            || matches!(&c.after, Some(PcbObject::Net(_)))
        {
            out.insert(c.id);
        }
    }
    out
}

/// Per-net ratsnest results kept up to date incrementally.
#[derive(Debug, Clone, Default)]
pub struct RatsnestEngine {
    per_net: BTreeMap<Id, Vec<RatLine>>,
}

impl RatsnestEngine {
    pub fn new(doc: &PcbDoc) -> Self {
        let mut e = Self::default();
        e.refresh(doc);
        e
    }

    /// Recomputes everything.
    pub fn refresh(&mut self, doc: &PcbDoc) {
        self.per_net.clear();
        for (net, items) in gather_all(doc, None) {
            if !doc.nets.contains_key(&net) {
                continue;
            }
            let lines = solve(doc, net, items);
            if !lines.is_empty() {
                self.per_net.insert(net, lines);
            }
        }
    }

    /// Recomputes only `nets` (use [`affected_nets`]). Returns how many nets were recomputed.
    pub fn update(&mut self, doc: &PcbDoc, nets: &BTreeSet<Id>) -> usize {
        for net in nets {
            self.per_net.remove(net);
        }
        // One pass over the board gathers the copper of every dirty net.
        for (net, items) in gather_all(doc, Some(nets)) {
            if !doc.nets.contains_key(&net) {
                continue;
            }
            let lines = solve(doc, net, items);
            if !lines.is_empty() {
                self.per_net.insert(net, lines);
            }
        }
        // Nets deleted from the document disappear.
        self.per_net.retain(|n, _| doc.nets.contains_key(n));
        nets.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = &RatLine> {
        self.per_net.values().flatten()
    }

    pub fn net_lines(&self, net: Id) -> &[RatLine] {
        self.per_net.get(&net).map_or(&[], Vec::as_slice)
    }

    pub fn nets(&self) -> impl Iterator<Item = Id> + '_ {
        self.per_net.keys().copied()
    }

    /// Number of connections still to route.
    pub fn count(&self) -> usize {
        self.per_net.values().map(Vec::len).sum()
    }
}

impl PartialEq for RatsnestEngine {
    fn eq(&self, o: &Self) -> bool {
        self.per_net == o.per_net
    }
}
