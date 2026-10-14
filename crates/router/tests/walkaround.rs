//! Walk Around on real boards: detours, refusals and the guarantee that whatever the router
//! returns passes the same DRC the panel runs.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::routing::AngleMode;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, BoardGraphic, FpShape, Net};
use buturecad_router::{
    route_leg, validate_route, walk_around, ObstacleSet, RouteCandidate, RouteError, RouteParams,
};
use proptest::prelude::*;

fn mm(v: f64) -> i64 {
    (v * 1e6).round() as i64
}
fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

struct Board {
    e: CommandEngine,
    a: Id,
    b: Id,
}

fn board() -> Board {
    let mut e = CommandEngine::new(Project::new("R").unwrap());
    let (a, b) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(a, PcbObject::Net(Net { name: "A".into() })),
        PcbChange::create(b, PcbObject::Net(Net { name: "B".into() })),
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
    Board { e, a, b }
}

impl Board {
    fn track(&mut self, net: Id, a: Point, b: Point) {
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
    }
    fn doc(&self) -> &PcbDoc {
        self.e.project().pcb()
    }
    fn set(&self, net: Id) -> ObstacleSet {
        ObstacleSet::collect(
            self.doc(),
            RouteParams::new(Some(net), layer::F_CU, mm(0.25)),
        )
    }
    fn violations(&self, net: Id, poly: &[Point]) -> usize {
        let tracks = vec![(layer::F_CU, poly.to_vec())];
        validate_route(
            self.doc(),
            &RouteCandidate {
                net: Some(net),
                width: mm(0.25),
                tracks: &tracks,
                vias: &[],
                via_diameter: mm(0.8),
            },
        )
        .len()
    }
}

fn len(poly: &[Point]) -> f64 {
    poly.windows(2).map(|w| w[0].distance(w[1])).sum()
}

#[test]
fn free_space_is_a_straight_line() {
    let b = board();
    let set = b.set(b.a);
    let r = walk_around(&set, p(5., 15.), p(35., 15.)).unwrap();
    assert_eq!(r, vec![p(5., 15.), p(35., 15.)]);
}

#[test]
fn a_track_of_another_net_is_walked_around_with_clearance() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    b.track(nb, p(20., 8.), p(20., 22.));
    let set = b.set(na);
    let r = walk_around(&set, p(5., 15.), p(35., 15.)).unwrap();
    assert!(r.len() >= 3, "{r:?}");
    assert!(len(&r) > mm(30.0) as f64);
    assert_eq!((r[0], *r.last().unwrap()), (p(5., 15.), p(35., 15.)));
    assert_eq!(b.violations(na, &r), 0, "{r:?}");
    // The shortest way is around the nearer end, not a wide berth.
    assert!(len(&r) < mm(40.0) as f64, "{}", len(&r) / 1e6);
}

#[test]
fn copper_of_the_same_net_is_not_an_obstacle() {
    let mut b = board();
    let na = b.a;
    b.track(na, p(20., 8.), p(20., 22.));
    let set = b.set(na);
    assert_eq!(walk_around(&set, p(5., 15.), p(35., 15.)).unwrap().len(), 2);
}

#[test]
fn the_preferred_elbow_is_kept_when_it_is_free() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    b.track(nb, p(20., 8.), p(20., 12.));
    let set = b.set(na);
    let r = route_leg(&set, p(5., 15.), p(35., 20.), AngleMode::Deg45, false).unwrap();
    assert!(b.violations(na, &r) == 0);
    // Only 45°/axis legs, exactly like the unobstructed elbow.
    for w in r.windows(2) {
        let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
        assert!(dx == 0 || dy == 0 || dx == dy, "{r:?}");
    }
}

#[test]
fn closed_in_or_blocked_endpoints_are_refused_not_faked() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    // A closed ring of the other net around (20, 15).
    for (a, c) in [
        (p(17., 12.), p(23., 12.)),
        (p(23., 12.), p(23., 18.)),
        (p(23., 18.), p(17., 18.)),
        (p(17., 18.), p(17., 12.)),
    ] {
        b.track(nb, a, c);
    }
    let set = b.set(na);
    assert_eq!(
        walk_around(&set, p(20., 15.), p(35., 15.)),
        Err(RouteError::NoPath)
    );
    // On top of the other net's copper.
    assert!(matches!(
        walk_around(&set, p(20., 12.), p(35., 15.)),
        Err(RouteError::StartBlocked(_))
    ));
    assert!(matches!(
        walk_around(&set, p(5., 15.), p(23., 15.)),
        Err(RouteError::EndBlocked(_))
    ));
}

#[test]
fn the_board_edge_is_a_wall() {
    let b = board();
    let na = b.a;
    let set = b.set(na);
    // Too close to the edge: refused.
    assert!(walk_around(&set, p(0.2, 15.), p(10., 15.)).is_err());
    let r = walk_around(&set, p(1., 1.), p(39., 29.)).unwrap();
    assert_eq!(b.violations(na, &r), 0);
}

#[test]
fn a_gap_wider_than_the_track_plus_clearances_is_used_and_a_narrow_one_is_not() {
    for (gap, passes) in [(1.2, true), (0.6, false)] {
        let mut b = board();
        let (na, nb) = (b.a, b.b);
        // A wall from edge to edge with a gap at y = 15.
        b.track(nb, p(20., 0.2), p(20., 15. - gap / 2.0));
        b.track(nb, p(20., 15. + gap / 2.0), p(20., 29.8));
        let set = b.set(na);
        let r = walk_around(&set, p(5., 15.), p(35., 15.));
        assert_eq!(r.is_ok(), passes, "gap {gap}: {r:?}");
        if let Ok(r) = r {
            assert_eq!(b.violations(na, &r), 0, "{r:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// Whatever the router returns on a random board passes the DRC rules.
    #[test]
    fn results_always_pass_the_drc(
        tracks in proptest::collection::vec((2.0f64..38.0, 2.0f64..28.0, 2.0f64..38.0, 2.0f64..28.0), 0..12),
        s in (3.0f64..37.0, 3.0f64..27.0),
        t in (3.0f64..37.0, 3.0f64..27.0),
    ) {
        let mut b = board();
        let (na, nb) = (b.a, b.b);
        for (x0, y0, x1, y1) in tracks {
            b.track(nb, p(x0, y0), p(x1, y1));
        }
        let set = b.set(na);
        let (from, to) = (p(s.0, s.1), p(t.0, t.1));
        if let Ok(r) = walk_around(&set, from, to) {
            prop_assert_eq!(r[0], from);
            prop_assert_eq!(*r.last().unwrap(), to);
            prop_assert_eq!(b.violations(na, &r), 0, "{:?}", r);
        }
    }
}
