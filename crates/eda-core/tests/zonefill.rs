//! Copper zone fill on real boards.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::copper::{self, CShape, Region};
use buturecad_eda_core::model::*;
use buturecad_eda_core::zonefill::{fill_changes, stale_zones};
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::generators::Header;
use buturecad_pcb::{
    layer, BoardGraphic, FpShape, Net, PadConnection, PlacedFootprint, Placement, Zone,
};
use proptest::prelude::*;
use std::sync::Arc;

fn mm(v: f64) -> i64 {
    (v * 1e6).round() as i64
}
fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

struct Board {
    e: CommandEngine,
    gnd: Id,
    sig: Id,
}

fn board() -> Board {
    let mut e = CommandEngine::new(Project::new("Z").unwrap());
    let (gnd, sig) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(gnd, PcbObject::Net(Net { name: "GND".into() })),
        PcbChange::create(sig, PcbObject::Net(Net { name: "SIG".into() })),
    ];
    let c = [p(0., 0.), p(40., 0.), p(40., 30.), p(0., 30.), p(0., 0.)];
    for w in c.windows(2) {
        let id = e.new_id();
        ch.push(PcbChange::create(
            id,
            PcbObject::Graphic(BoardGraphic {
                layer: layer::EDGE_CUTS,
                shape: FpShape::Line {
                    segment: Segment::new(w[0], w[1]),
                    width: 100_000,
                },
            }),
        ));
    }
    e.execute(PcbEditCommand::new("setup", ch)).unwrap();
    Board { e, gnd, sig }
}

impl Board {
    fn doc(&self) -> &PcbDoc {
        self.e.project().pcb()
    }
    fn zone(&mut self, net: Option<Id>, f: impl FnOnce(&mut Zone)) -> Id {
        let id = self.e.new_id();
        let mut z = Zone::new(
            layer::F_CU,
            vec![p(0., 0.), p(40., 0.), p(40., 30.), p(0., 30.)],
            net,
        );
        f(&mut z);
        self.e
            .execute(PcbEditCommand::new(
                "zone",
                vec![PcbChange::create(id, PcbObject::Zone(z))],
            ))
            .unwrap();
        id
    }
    fn track(&mut self, net: Id, a: Point, b: Point) -> Id {
        let id = self.e.new_id();
        self.e
            .execute(PcbEditCommand::new(
                "t",
                vec![PcbChange::create(
                    id,
                    PcbObject::Track(Track {
                        layer: layer::F_CU,
                        segment: Segment::new(a, b),
                        width: mm(0.25),
                        net: Some(net),
                    }),
                )],
            ))
            .unwrap();
        id
    }
    fn fill(&mut self) {
        let ch = fill_changes(self.doc(), None, false);
        if !ch.is_empty() {
            self.e.execute(PcbEditCommand::new("fill", ch)).unwrap();
        }
    }
    fn regions(&self, z: Id) -> Vec<Region> {
        self.doc().zones[&z]
            .fill
            .as_ref()
            .unwrap()
            .regions
            .iter()
            .map(|r| Region::new(r.outer.clone(), r.holes.clone()).unwrap())
            .collect()
    }
    /// Smallest distance from the fill of `z` to copper of nets other than `net` (nm).
    fn min_gap_to_others(&self, z: Id, net: Id) -> f64 {
        let regions = self.regions(z);
        let mut best = f64::INFINITY;
        for id in self
            .doc()
            .tracks
            .keys()
            .chain(self.doc().vias.keys())
            .chain(self.doc().placed.keys())
        {
            for it in copper::items_of(self.doc(), *id) {
                if it.net == Some(net) || it.layer != layer::F_CU {
                    continue;
                }
                for r in &regions {
                    best = best.min(copper::distance(
                        &it.shape,
                        &CShape::Region(Arc::new(
                            Region::new(
                                r.outer.vertices().to_vec(),
                                r.holes.iter().map(|h| h.vertices().to_vec()).collect(),
                            )
                            .unwrap(),
                        )),
                    ));
                }
            }
        }
        best
    }
}

fn area(b: &Board, z: Id) -> f64 {
    b.doc().zones[&z]
        .fill
        .as_ref()
        .unwrap()
        .regions
        .iter()
        .map(|r| {
            let a = |pts: &[Point]| {
                buturecad_geometry::Polygon::new(pts.to_vec())
                    .unwrap()
                    .area()
            };
            a(&r.outer) - r.holes.iter().map(|h| a(h)).sum::<f64>()
        })
        .sum()
}

#[test]
fn an_empty_zone_fills_to_the_board_inset_by_the_edge_clearance() {
    let mut b = board();
    let (g, _) = (b.gnd, b.sig);
    let z = b.zone(Some(g), |_| {});
    b.fill();
    assert_eq!(
        b.doc().zones[&z].fill.as_ref().unwrap().regions.len(),
        0,
        "no copper of the net: island removed"
    );
    let z2 = b.zone(None, |_| {});
    b.fill();
    let a = area(&b, z2);
    // 40 x 30 minus a 0.3 mm margin all round (edge segments are centre lines).
    let expected = (40.0 - 0.6) * (30.0 - 0.6) * 1e12;
    assert!((a - expected).abs() / expected < 0.01, "{a} vs {expected}");
}

#[test]
fn copper_of_other_nets_is_cleared_and_the_net_own_tracks_merge() {
    let mut b = board();
    let (g, s) = (b.gnd, b.sig);
    b.track(g, p(5., 5.), p(35., 5.)); // gives the zone something of its net to connect to
    b.track(s, p(5., 15.), p(35., 15.));
    let z = b.zone(Some(g), |_| {});
    b.fill();
    let gap = b.min_gap_to_others(z, g);
    assert!(gap >= 200_000.0 - 2.0, "gap {gap}");
    assert!(gap < 260_000.0, "and not wasteful: {gap}");
    // The GND track lies inside the pour (merged), the SIG track has a channel.
    let regions = b.regions(z);
    let inside = |pt: Point| regions.iter().any(|r| r.contains(pt));
    assert!(inside(p(20., 5.)));
    assert!(!inside(p(20., 15.)));
    assert!(inside(p(20., 20.)));
}

#[test]
fn islands_without_the_nets_copper_are_removed_unless_asked_to_keep_them() {
    for keep in [false, true] {
        let mut b = board();
        let (g, s) = (b.gnd, b.sig);
        b.track(g, p(5., 5.), p(10., 5.));
        // A wall of SIG across the whole board separates the upper half from the track.
        b.track(s, p(0.4, 15.), p(39.6, 15.));
        let z = b.zone(Some(g), |z| z.remove_islands = !keep);
        b.fill();
        let regions = b.regions(z);
        let upper = regions.iter().any(|r| r.contains(p(20., 25.)));
        let lower = regions.iter().any(|r| r.contains(p(20., 8.)));
        assert!(lower, "the half with the track stays");
        assert_eq!(upper, keep, "keep_islands={keep}");
    }
}

fn header(b: &mut Board, nets: [Option<Id>; 2]) {
    let fp = Header::standard(2, 1).generate().unwrap();
    let name = fp.name.clone();
    b.e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let id = b.e.new_id();
    let mut pf = PlacedFootprint {
        footprint: name,
        reference: "J1".into(),
        value: "x".into(),
        placement: Placement::at(p(20., 10.)),
        locked: false,
        link: None,
        pad_nets: Default::default(),
    };
    for (n, net) in ["1", "2"].into_iter().zip(nets) {
        if let Some(net) = net {
            pf.pad_nets.insert(n.into(), net);
        }
    }
    b.e.execute(PcbEditCommand::new(
        "fp",
        vec![PcbChange::create(id, PcbObject::Footprint(pf))],
    ))
    .unwrap();
}

#[test]
fn pad_connections_solid_thermal_and_none() {
    let pad1 = p(20., 10.);
    let results: Vec<(PadConnection, bool, bool)> = [
        PadConnection::Solid,
        PadConnection::Thermal,
        PadConnection::None,
    ]
    .into_iter()
    .map(|conn| {
        let mut b = board();
        let (g, s) = (b.gnd, b.sig);
        header(&mut b, [Some(g), Some(s)]);
        let z = b.zone(Some(g), |z| z.connection = conn);
        b.fill();
        let regions = b.regions(z);
        let inside = |pt: Point| regions.iter().any(|r| r.contains(pt));
        // Solid: copper right next to the pad; thermal: a gap with spokes along the axes.
        let next_to_pad = inside(Point::new(pad1.x + mm(0.9), pad1.y + mm(0.9)));
        let spoke = inside(Point::new(pad1.x + mm(1.1), pad1.y));
        (conn, next_to_pad, spoke)
    })
    .collect();
    assert_eq!(
        results[0],
        (PadConnection::Solid, true, true),
        "solid pours up to the pad"
    );
    assert_eq!(results[1].0, PadConnection::Thermal);
    assert!(!results[1].1, "the corner of the gap is empty");
    assert!(results[1].2, "a spoke crosses the gap on the axis");
    assert!(!results[2].2, "no connection: the pad is isolated");
}

#[test]
fn higher_priority_zones_fill_first_and_lower_ones_keep_clear() {
    let mut b = board();
    let (g, s) = (b.gnd, b.sig);
    b.track(g, p(30., 25.), p(35., 25.));
    b.track(s, p(5., 5.), p(10., 5.));
    let hi = b.zone(Some(s), |z| {
        z.priority = 5;
        z.outline = vec![p(2., 2.), p(15., 2.), p(15., 12.), p(2., 12.)];
    });
    let lo = b.zone(Some(g), |_| {});
    b.fill();
    assert!(!b.regions(hi).is_empty());
    let regions_lo = b.regions(lo);
    assert!(
        !regions_lo.iter().any(|r| r.contains(p(8., 8.))),
        "the SIG pour is left alone"
    );
    // The two pours keep the clearance from each other.
    let hi_regions = b.regions(hi);
    let mut gap = f64::INFINITY;
    for a in &hi_regions {
        for c in &regions_lo {
            gap = gap.min(copper::distance(
                &CShape::Region(Arc::new(
                    Region::new(
                        a.outer.vertices().to_vec(),
                        a.holes.iter().map(|h| h.vertices().to_vec()).collect(),
                    )
                    .unwrap(),
                )),
                &CShape::Region(Arc::new(
                    Region::new(
                        c.outer.vertices().to_vec(),
                        c.holes.iter().map(|h| h.vertices().to_vec()).collect(),
                    )
                    .unwrap(),
                )),
            ));
        }
    }
    assert!(gap >= 200_000.0 - 2.0, "{gap}");
}

#[test]
fn staleness_follows_the_copper_near_the_zone_and_fills_are_deterministic() {
    let mut b = board();
    let (g, s) = (b.gnd, b.sig);
    b.track(g, p(2., 2.), p(8., 2.));
    let z = b.zone(Some(g), |z| {
        z.outline = vec![p(1., 1.), p(20., 1.), p(20., 29.), p(1., 29.)]
    });
    assert_eq!(stale_zones(b.doc()), vec![z], "never filled");
    b.fill();
    assert!(stale_zones(b.doc()).is_empty());
    let first = b.doc().zones[&z].fill.clone();
    // Far from the zone: still fresh.
    b.track(s, p(30., 5.), p(38., 5.));
    assert!(stale_zones(b.doc()).is_empty());
    // Next to it: stale, and a refill fixes it.
    b.track(s, p(10., 10.), p(15., 10.));
    assert_eq!(stale_zones(b.doc()), vec![z]);
    b.fill();
    assert!(stale_zones(b.doc()).is_empty());
    assert_ne!(b.doc().zones[&z].fill, first);
    // Same inputs, same fill, bit for bit.
    let again = fill_changes(b.doc(), None, true);
    assert!(
        again.is_empty(),
        "a forced refill reproduces the stored fill exactly"
    );
    // Two undos (the refill, then the track near the zone) restore the earlier, still fresh, state.
    b.e.undo().unwrap();
    b.e.undo().unwrap();
    assert_eq!(
        b.doc().tracks.len(),
        2,
        "the GND track and the far SIG track remain"
    );
    assert!(
        stale_zones(b.doc()).is_empty(),
        "the fill belongs to the board state it was computed on"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// Whatever the other-net copper is, the fill keeps the clearance from it.
    #[test]
    fn the_fill_always_keeps_the_clearance(
        tracks in proptest::collection::vec((3.0f64..37.0, 3.0f64..27.0, -8.0f64..8.0, -8.0f64..8.0), 0..10),
    ) {
        let mut b = board();
        let (g, s) = (b.gnd, b.sig);
        b.track(g, p(1., 1.), p(2., 1.));
        for (x, y, dx, dy) in tracks {
            b.track(s, p(x, y), p(x + dx, y + dy));
        }
        let z = b.zone(Some(g), |_| {});
        b.fill();
        if !b.doc().zones[&z].fill.as_ref().unwrap().regions.is_empty() {
            let gap = b.min_gap_to_others(z, g);
            prop_assert!(gap >= 200_000.0 - 2.0, "gap {}", gap);
        }
    }
}

#[test]
fn a_filled_zone_connects_the_pads_it_touches_in_the_ratsnest() {
    use buturecad_eda_core::ratsnest::net_ratsnest;
    let mut b = board();
    let g = b.gnd;
    header(&mut b, [Some(g), Some(g)]);
    b.track(g, p(5., 25.), p(8., 25.)); // a GND island far from the header
    assert_eq!(
        net_ratsnest(b.doc(), g).len(),
        2,
        "three separate islands: the two header pads and the track"
    );
    let _z = b.zone(Some(g), |z| z.connection = PadConnection::Solid);
    assert_eq!(
        net_ratsnest(b.doc(), g).len(),
        2,
        "an unfilled zone connects nothing"
    );
    b.fill();
    assert!(
        net_ratsnest(b.doc(), g).is_empty(),
        "the pour joins them: {:?}",
        net_ratsnest(b.doc(), g)
    );
}

#[test]
fn triangulation_covers_exactly_the_filled_area() {
    use buturecad_eda_core::zonefill::triangulate;
    let mut b = board();
    let (g, s) = (b.gnd, b.sig);
    b.track(g, p(2., 2.), p(4., 2.));
    b.track(s, p(10., 10.), p(30., 20.));
    header(&mut b, [Some(s), Some(s)]);
    let z = b.zone(Some(g), |_| {});
    b.fill();
    let fill = b.doc().zones[&z].fill.clone().unwrap();
    let tri: f64 = triangulate(&fill.regions)
        .iter()
        .map(|t| buturecad_geometry::Polygon::new(t.to_vec()).unwrap().area())
        .sum();
    assert!(
        (tri - area(&b, z)).abs() / area(&b, z) < 1e-6,
        "{tri} vs {}",
        area(&b, z)
    );
}
