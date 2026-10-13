//! Ratsnest: islands, minimum spanning trees, layers, vias, incremental updates.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::model::*;
use buturecad_eda_core::ratsnest::*;
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Footprint, Pad, PlacedFootprint, Placement};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

const MM: i64 = 1_000_000;

fn fp() -> Footprint {
    let mut f = Footprint::new("T:TWO");
    f.pads.push(Pad::smd("1", Point::new(0, 0), MM, MM));
    f.pads.push(Pad::smd("2", Point::new(4 * MM, 0), MM, MM));
    f
}

struct Board {
    doc: PcbDoc,
    next: u64,
}

impl Board {
    fn new() -> Self {
        let mut doc = PcbDoc::default();
        doc.footprints.insert("T:TWO".into(), fp());
        doc.nets
            .insert(Id(1), buturecad_pcb::Net { name: "N".into() });
        doc.nets
            .insert(Id(2), buturecad_pcb::Net { name: "M".into() });
        Self { doc, next: 100 }
    }
    fn id(&mut self) -> Id {
        self.next += 1;
        Id(self.next)
    }
    /// A footprint at (x, y) mm whose pad 1 is on `net1` and pad 2 on `net2`.
    fn place(&mut self, x: i64, y: i64, flipped: bool, net1: Option<u64>, net2: Option<u64>) -> Id {
        let id = self.id();
        let mut pad_nets = BTreeMap::new();
        if let Some(n) = net1 {
            pad_nets.insert("1".to_string(), Id(n));
        }
        if let Some(n) = net2 {
            pad_nets.insert("2".to_string(), Id(n));
        }
        self.doc.placed.insert(
            id,
            PlacedFootprint {
                footprint: "T:TWO".into(),
                reference: format!("U{}", self.next),
                value: String::new(),
                placement: Placement {
                    position: Point::new(x * MM, y * MM),
                    rotation: 0,
                    flipped,
                },
                locked: false,
                link: None,
                pad_nets,
            },
        );
        id
    }
    fn track(&mut self, l: layer::LayerId, a: (i64, i64), b: (i64, i64), net: u64) -> Id {
        let id = self.id();
        self.doc.tracks.insert(
            id,
            Track {
                layer: l,
                segment: Segment::new(
                    Point::new(a.0 * MM, a.1 * MM),
                    Point::new(b.0 * MM, b.1 * MM),
                ),
                width: 200_000,
                net: Some(Id(net)),
            },
        );
        id
    }
    fn via(&mut self, at: (i64, i64), net: u64) -> Id {
        let id = self.id();
        self.doc.vias.insert(
            id,
            Via {
                position: Point::new(at.0 * MM, at.1 * MM),
                diameter: 800_000,
                drill: 400_000,
                net: Some(Id(net)),
                kind: ViaKind::Through,
            },
        );
        id
    }
}

fn lines(b: &Board, net: u64) -> Vec<RatLine> {
    net_ratsnest(&b.doc, Id(net))
}

#[test]
fn unrouted_pads_get_a_minimum_spanning_tree() {
    let mut b = Board::new();
    // Pad 1 of four footprints on net 1, at known spots.
    for (x, y) in [(0, 0), (10, 0), (10, 10), (30, 10)] {
        b.place(x, y, false, Some(1), None);
    }
    let l = lines(&b, 1);
    assert_eq!(l.len(), 3, "n pads need n-1 lines");
    let total: f64 = l.iter().map(RatLine::length).sum();
    // MST: 10 + 10 + 20 = 40 mm.
    assert!((total - 40.0 * MM as f64).abs() < 1.0, "{total}");
    // Lines join pad centres and form a connected tree.
    let mut pts: BTreeSet<Point> = BTreeSet::new();
    for r in &l {
        pts.insert(r.a);
        pts.insert(r.b);
    }
    assert_eq!(pts.len(), 4);
    // Other nets are untouched; a single pad has no ratsnest.
    assert!(lines(&b, 2).is_empty());
}

#[test]
fn routed_copper_removes_lines_and_layers_matter() {
    let mut b = Board::new();
    let (a, _, _) = (
        b.place(0, 0, false, Some(1), None),
        b.place(20, 0, false, Some(1), None),
        0,
    );
    let _ = a;
    assert_eq!(lines(&b, 1).len(), 1);
    // A track on the wrong layer does not connect (pads are SMD on the front).
    b.track(layer::B_CU, (0, 0), (20, 0), 1);
    // The stray back track is its own island (it touches nothing): the net needs 2 lines.
    assert_eq!(
        lines(&b, 1).len(),
        2,
        "back copper does not touch front SMD pads"
    );
    // A front track pad to pad connects them.
    b.track(layer::F_CU, (0, 0), (10, 0), 1);
    assert_eq!(lines(&b, 1).len(), 2, "half way is not enough");
    b.track(layer::F_CU, (10, 0), (20, 0), 1);
    // Pads connected; only the stray back-layer track is left to join.
    assert_eq!(lines(&b, 1).len(), 1);
    let stray: Vec<Id> = b
        .doc
        .tracks
        .iter()
        .filter(|(_, t)| t.layer == layer::B_CU)
        .map(|(i, _)| *i)
        .collect();
    for id in stray {
        b.doc.tracks.remove(&id);
    }
    assert!(lines(&b, 1).is_empty(), "complete");
}

#[test]
fn vias_connect_layers_and_back_side_footprints_use_the_back_copper() {
    let mut b = Board::new();
    b.place(0, 0, false, Some(1), None); // front pad
    b.place(20, 0, true, Some(1), None); // flipped: pad on B.Cu (mirrored: pad 1 at x = 20)
    assert_eq!(lines(&b, 1).len(), 1);
    b.track(layer::F_CU, (0, 0), (10, 0), 1);
    b.via((10, 0), 1);
    b.track(layer::B_CU, (10, 0), (20, 0), 1);
    assert!(
        lines(&b, 1).is_empty(),
        "front track, via, back track reach the back pad"
    );
}

#[test]
fn copper_of_another_net_never_connects() {
    let mut b = Board::new();
    b.place(0, 0, false, Some(1), None);
    b.place(20, 0, false, Some(1), None);
    b.track(layer::F_CU, (0, 0), (20, 0), 2); // net M runs over both pads
    assert_eq!(lines(&b, 1).len(), 1, "a short is not a connection");
}

#[test]
fn vias_without_pads_and_islands_of_tracks_are_handled() {
    let mut b = Board::new();
    b.place(0, 0, false, Some(1), None);
    b.via((10, 0), 1);
    let l = lines(&b, 1);
    assert_eq!(l.len(), 1);
    assert!(l[0].b == Point::new(10 * MM, 0) || l[0].a == Point::new(10 * MM, 0));
    b.track(layer::F_CU, (0, 0), (10, 0), 1);
    assert!(lines(&b, 1).is_empty());
}

#[test]
fn large_nets_use_the_approximate_tree_and_still_span() {
    let mut b = Board::new();
    // 30 × 30 = 900 footprints' pad 1 on one net: more than EXACT_LIMIT anchors.
    for i in 0..30 {
        for j in 0..30 {
            b.place(i * 6, j * 6, false, Some(1), None);
        }
    }
    let l = lines(&b, 1);
    assert_eq!(l.len(), 899);
    // Every pad is reachable: union-find over the lines.
    let mut idx: BTreeMap<Point, usize> = BTreeMap::new();
    for r in &l {
        let n = idx.len();
        idx.entry(r.a).or_insert(n);
        let n = idx.len();
        idx.entry(r.b).or_insert(n);
    }
    assert_eq!(idx.len(), 900);
    let mut parent: Vec<usize> = (0..900).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for r in &l {
        let (a, c) = (find(&mut parent, idx[&r.a]), find(&mut parent, idx[&r.b]));
        parent[a] = c;
    }
    let roots: BTreeSet<usize> = (0..900).map(|i| find(&mut parent, i)).collect();
    assert_eq!(roots.len(), 1);
    // On a grid the approximation finds the exact length (all nearest edges are 6 mm).
    let total: f64 = l.iter().map(RatLine::length).sum();
    assert!((total - 899.0 * 6.0 * MM as f64).abs() < 1.0);
}

#[derive(Debug, Clone)]
enum Op {
    Place(i64, i64, bool, u8),
    Track(u8, i64, i64, i64, i64, u8),
    Via(i64, i64, u8),
    Delete(u8),
}

fn op() -> impl Strategy<Value = Op> {
    let c = -30i64..30;
    prop_oneof![
        3 => (c.clone(), c.clone(), any::<bool>(), 0u8..3).prop_map(|(x, y, f, n)| Op::Place(x, y, f, n)),
        3 => (0u8..2, c.clone(), c.clone(), c.clone(), c.clone(), 1u8..3).prop_map(|(l, a, b, d, e, n)| Op::Track(l, a, b, d, e, n)),
        1 => (c.clone(), c.clone(), 1u8..3).prop_map(|(x, y, n)| Op::Via(x, y, n)),
        1 => any::<u8>().prop_map(Op::Delete),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Updating only the affected nets after each change gives the same result as recomputing
    /// everything, and unrelated nets are not recomputed.
    #[test]
    fn incremental_equals_full(ops in prop::collection::vec(op(), 1..40)) {
        let mut b = Board::new();
        let mut engine = RatsnestEngine::new(&b.doc);
        for o in ops {
            let change = match o {
                Op::Place(x, y, f, n) => {
                    let before = b.doc.clone();
                    let id = b.place(x, y, f, Some(u64::from(n % 2) + 1), None);
                    let c = PcbChange::create(id, b.doc.object(id).unwrap());
                    b.doc = before;
                    c
                }
                Op::Track(l, x1, y1, x2, y2, n) => {
                    if (x1, y1) == (x2, y2) { continue; }
                    let id = b.id();
                    PcbChange::create(id, PcbObject::Track(Track {
                        layer: if l == 0 { layer::F_CU } else { layer::B_CU },
                        segment: Segment::new(Point::new(x1 * MM, y1 * MM), Point::new(x2 * MM, y2 * MM)),
                        width: 200_000,
                        net: Some(Id(u64::from(n))),
                    }))
                }
                Op::Via(x, y, n) => {
                    let id = b.id();
                    PcbChange::create(id, PcbObject::Via(Via { position: Point::new(x * MM, y * MM), diameter: 800_000, drill: 400_000, net: Some(Id(u64::from(n))), kind: ViaKind::Through }))
                }
                Op::Delete(k) => {
                    let ids: Vec<Id> = b.doc.placed.keys().chain(b.doc.tracks.keys()).chain(b.doc.vias.keys()).copied().collect();
                    if ids.is_empty() { continue; }
                    let id = ids[k as usize % ids.len()];
                    PcbChange::delete(id, b.doc.object(id).unwrap())
                }
            };
            b.doc.apply_changes(std::slice::from_ref(&change)).unwrap();
            let dirty = affected_nets(std::slice::from_ref(&change));
            let n = engine.update(&b.doc, &dirty);
            prop_assert!(n <= 2);
            prop_assert_eq!(&engine, &RatsnestEngine::new(&b.doc));
        }
    }
}

#[test]
fn affected_nets_cover_moves_and_net_edits() {
    let mut b = Board::new();
    let id = b.place(0, 0, false, Some(1), Some(2));
    let before = b.doc.object(id).unwrap();
    let PcbObject::Footprint(mut f) = before.clone() else {
        panic!()
    };
    f.placement.position = Point::new(MM, 0);
    let c = PcbChange::modify(id, before, PcbObject::Footprint(f));
    assert_eq!(affected_nets(&[c]), [Id(1), Id(2)].into_iter().collect());
    let n = PcbChange::delete(
        Id(1),
        PcbObject::Net(buturecad_pcb::Net { name: "N".into() }),
    );
    assert!(affected_nets(&[n]).contains(&Id(1)));
}
