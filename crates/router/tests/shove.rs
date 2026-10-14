//! Push & Shove scenarios on real boards. Every accepted result is applied to the board through
//! the Command Engine and checked with the DRC.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, BoardGraphic, FpShape, Net};
use buturecad_router::{shove, NewCopper, ShoveError};
use buturecad_rules::{run_drc, DrcRule};
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
    let mut e = CommandEngine::new(Project::new("S").unwrap());
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
    fn via(&mut self, net: Id, at: Point) {
        let id = self.e.new_id();
        self.e
            .execute(PcbEditCommand::new(
                "v",
                vec![PcbChange::create(
                    id,
                    PcbObject::Via(Via {
                        position: at,
                        diameter: mm(0.8),
                        drill: mm(0.4),
                        net: Some(net),
                        kind: ViaKind::Through,
                    }),
                )],
            ))
            .unwrap();
    }
    fn doc(&self) -> &PcbDoc {
        self.e.project().pcb()
    }
    /// Applies the new route and the shove result, then returns the DRC rules violated.
    fn apply(
        &mut self,
        net: Id,
        poly: &[Point],
        r: &buturecad_router::ShoveResult,
    ) -> Vec<DrcRule> {
        let mut ch = Vec::new();
        for (id, seg) in &r.moved {
            let before = self.doc().object(*id).unwrap();
            let mut t = self.doc().tracks[id].clone();
            t.segment = *seg;
            ch.push(PcbChange::modify(*id, before, PcbObject::Track(t)));
        }
        for id in &r.removed {
            ch.push(PcbChange::delete(*id, self.doc().object(*id).unwrap()));
        }
        for w in poly.windows(2) {
            let id = self.e.new_id();
            ch.push(PcbChange::create(
                id,
                PcbObject::Track(Track {
                    layer: layer::F_CU,
                    segment: Segment::new(w[0], w[1]),
                    width: mm(0.25),
                    net: Some(net),
                }),
            ));
        }
        self.e.execute(PcbEditCommand::new("route", ch)).unwrap();
        run_drc(self.doc())
            .violations
            .iter()
            .map(|v| v.rule)
            .filter(|r| {
                matches!(
                    r,
                    DrcRule::Clearance
                        | DrcRule::Short
                        | DrcRule::CopperToEdge
                        | DrcRule::OutsideBoard
                )
            })
            .collect()
    }
}

fn new(net: Id, poly: &[(f64, f64)]) -> (Vec<(buturecad_pcb::LayerId, Vec<Point>)>, Id) {
    (
        vec![(layer::F_CU, poly.iter().map(|(x, y)| p(*x, *y)).collect())],
        net,
    )
}

fn run(
    b: &Board,
    net: Id,
    poly: &[(f64, f64)],
) -> Result<buturecad_router::ShoveResult, ShoveError> {
    let (tracks, _) = new(net, poly);
    shove(
        b.doc(),
        &NewCopper {
            net: Some(net),
            width: mm(0.25),
            tracks: &tracks,
            vias: &[],
            via_diameter: mm(0.8),
        },
    )
}

#[test]
fn a_free_track_is_pushed_aside_and_the_board_stays_drc_clean() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    let t = b.track(nb, p(10., 15.2), p(30., 15.2));
    let poly = [(5., 15.), (35., 15.)];
    let r = run(&b, na, &poly).unwrap();
    assert_eq!(r.moved.len(), 1);
    assert_eq!(r.moved[0].0, t);
    // Pushed along +y (away from the new route), not sideways.
    assert!(r.moved[0].1.a.y > p(0., 15.2).y && r.moved[0].1.a.x == p(10., 0.).x);
    let bad = b.apply(na, &[p(5., 15.), p(35., 15.)], &r);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn pushes_cascade_through_a_stack_of_parallel_tracks() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    let nc = {
        let id = b.e.new_id();
        b.e.execute(PcbEditCommand::new(
            "n",
            vec![PcbChange::create(
                id,
                PcbObject::Net(Net { name: "C".into() }),
            )],
        ))
        .unwrap();
        id
    };
    b.track(nb, p(10., 15.3), p(30., 15.3));
    b.track(nc, p(10., 15.8), p(30., 15.8));
    b.track(nb, p(10., 16.3), p(30., 16.3));
    let r = run(&b, na, &[(5., 15.), (35., 15.)]).unwrap();
    assert_eq!(r.moved.len(), 3, "all three had to move");
    assert!(r.pushes >= 3);
    let bad = b.apply(na, &[p(5., 15.), p(35., 15.)], &r);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn connected_copper_stretches_instead_of_tearing() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    // An L of net B: the horizontal leg is in the way, the vertical leg hangs off its end.
    let h = b.track(nb, p(10., 15.2), p(30., 15.2));
    let v = b.track(nb, p(30., 15.2), p(30., 25.));
    let r = run(&b, na, &[(5., 15.), (35., 15.)]).unwrap();
    let ids: Vec<Id> = r.moved.iter().map(|(i, _)| *i).collect();
    assert!(ids.contains(&h) && ids.contains(&v));
    let seg = |id: Id| r.moved.iter().find(|(i, _)| *i == id).unwrap().1;
    assert_eq!(seg(h).b, seg(v).a, "the joint stays closed");
    let bad = b.apply(na, &[p(5., 15.), p(35., 15.)], &r);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn a_track_anchored_at_both_ends_cannot_be_pushed() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    b.via(nb, p(10., 15.2));
    b.via(nb, p(30., 15.2));
    b.track(nb, p(10., 15.2), p(30., 15.2));
    let e = run(&b, na, &[(5., 15.), (35., 15.)]).unwrap_err();
    assert!(matches!(e, ShoveError::Unresolvable(_)), "{e:?}");
}

#[test]
fn a_track_that_crosses_the_route_is_refused_and_so_is_pushing_into_the_edge() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    b.track(nb, p(20., 8.), p(20., 22.));
    assert!(matches!(
        run(&b, na, &[(5., 15.), (35., 15.)]),
        Err(ShoveError::Unresolvable(_))
    ));

    let mut c = board();
    let (na, nb) = (c.a, c.b);
    // Hard against the top edge: there is no room to push it.
    c.track(nb, p(10., 29.5), p(30., 29.5));
    let e = run(&c, na, &[(5., 29.2), (35., 29.2)]).unwrap_err();
    assert!(matches!(e, ShoveError::Unresolvable(_)), "{e:?}");
}

#[test]
fn nothing_to_push_is_an_empty_result() {
    let mut b = board();
    let (na, nb) = (b.a, b.b);
    b.track(nb, p(10., 25.), p(30., 25.));
    let r = run(&b, na, &[(5., 15.), (35., 15.)]).unwrap();
    assert!(r.moved.is_empty() && r.removed.is_empty() && r.pushes == 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    /// Whenever the shover accepts, the resulting board has no clearance, short or edge errors.
    #[test]
    fn accepted_results_are_always_drc_clean(
        tracks in proptest::collection::vec((4.0f64..36.0, 4.0f64..26.0, -6.0f64..6.0, -6.0f64..6.0), 1..8),
        route in ((3.0f64..37.0), (3.0f64..27.0), (3.0f64..37.0), (3.0f64..27.0)),
    ) {
        let mut b = board();
        let (na, nb) = (b.a, b.b);
        for (x, y, dx, dy) in tracks {
            b.track(nb, p(x, y), p(x + dx, y + dy));
        }
        let poly = [p(route.0, route.1), p(route.2, route.3)];
        let tracks_new = vec![(layer::F_CU, poly.to_vec())];
        let res = shove(b.doc(), &NewCopper { net: Some(na), width: mm(0.25), tracks: &tracks_new, vias: &[], via_diameter: mm(0.8) });
        if let Ok(r) = res {
            let before = run_drc(b.doc()).violations.len();
            let bad = b.apply(na, &poly, &r);
            prop_assert!(bad.is_empty() || before > 0, "{:?}", bad);
        }
    }
}
