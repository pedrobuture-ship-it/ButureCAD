//! DRC on real board geometry, and incremental == from scratch.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, BoardGraphic, FpShape, Net, NetClass};
use buturecad_rules::{run_drc, DrcEngine, DrcRule};

fn mm(v: f64) -> i64 {
    (v * 1e6) as i64
}
fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

fn board() -> (CommandEngine, Id, Id) {
    let mut e = CommandEngine::new(Project::new("D").unwrap());
    let (a, b) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(a, PcbObject::Net(Net { name: "A".into() })),
        PcbChange::create(b, PcbObject::Net(Net { name: "B".into() })),
    ];
    let corners = [p(0., 0.), p(40., 0.), p(40., 30.), p(0., 30.), p(0., 0.)];
    for w in corners.windows(2) {
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
    (e, a, b)
}

fn track(e: &mut CommandEngine, net: Option<Id>, a: Point, b: Point, w: i64) -> Id {
    let id = e.new_id();
    e.execute(PcbEditCommand::new(
        "t",
        vec![PcbChange::create(
            id,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(a, b),
                width: w,
                net,
            }),
        )],
    ))
    .unwrap();
    id
}

fn rules(e: &CommandEngine) -> Vec<DrcRule> {
    run_drc(e.project().pcb())
        .violations
        .iter()
        .map(|v| v.rule)
        .collect()
}

#[test]
fn clean_board_has_no_violations() {
    let (mut e, a, b) = board();
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.25));
    track(&mut e, Some(b), p(5., 6.), p(15., 6.), mm(0.25));
    assert!(rules(&e).is_empty(), "{:?}", rules(&e));
}

#[test]
fn clearance_short_and_same_net_are_distinguished() {
    let (mut e, a, b) = board();
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.25));
    // 0.3 mm edge-to-edge gap (0.55 pitch - 0.25) > 0.2: ok; 0.15 gap: clearance error.
    let t2 = track(&mut e, Some(b), p(5., 5.4), p(15., 5.4), mm(0.25));
    assert_eq!(rules(&e), vec![DrcRule::Clearance]);
    let v = &run_drc(e.project().pcb()).violations[0];
    assert!(v.objects.contains(&t2) && v.objects.len() == 2);
    assert_eq!(v.actual, Some(mm(0.15)));
    // Overlapping different nets: short.
    track(&mut e, Some(b), p(10., 4.), p(10., 6.), mm(0.25));
    assert!(rules(&e).contains(&DrcRule::Short));
    // Same net never conflicts with itself.
    let (mut e2, a2, _) = board();
    track(&mut e2, Some(a2), p(5., 5.), p(15., 5.), mm(0.25));
    track(&mut e2, Some(a2), p(10., 5.), p(10., 9.), mm(0.25));
    assert!(rules(&e2).is_empty());
}

#[test]
fn width_via_and_edge_rules() {
    let (mut e, a, _) = board();
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.1));
    let v = e.new_id();
    e.execute(PcbEditCommand::new(
        "v",
        vec![PcbChange::create(
            v,
            PcbObject::Via(Via {
                position: p(20., 10.),
                diameter: mm(0.4),
                drill: mm(0.25),
                net: Some(a),
                kind: ViaKind::default(),
            }),
        )],
    ))
    .unwrap();
    track(&mut e, Some(a), p(0.2, 15.), p(10., 15.), mm(0.25));
    let r = rules(&e);
    for want in [
        DrcRule::MinTrackWidth,
        DrcRule::ViaDiameter,
        DrcRule::ViaDrill,
        DrcRule::AnnularRing,
        DrcRule::CopperToEdge,
    ] {
        assert!(r.contains(&want), "missing {want:?} in {r:?}");
    }
    let (mut e2, a2, _) = board();
    track(&mut e2, Some(a2), p(-5., 5.), p(5., 5.), mm(0.25));
    assert!(rules(&e2).contains(&DrcRule::OutsideBoard));
}

#[test]
fn net_class_clearance_applies_to_the_pair() {
    let (mut e, a, b) = board();
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.25));
    track(&mut e, Some(b), p(5., 5.6), p(15., 5.6), mm(0.25)); // 0.35 gap
    assert!(rules(&e).is_empty());
    let before = e.project().pcb().rules.clone();
    let mut after = before.clone();
    let mut hv = NetClass::new("HV");
    hv.clearance = mm(0.5);
    after.classes.insert("HV".into(), hv);
    after.assignments.insert(a, "HV".into());
    e.execute(SetRulesCommand::new("rules", before, after))
        .unwrap();
    assert_eq!(rules(&e), vec![DrcRule::Clearance]);
    e.undo().unwrap();
    assert!(rules(&e).is_empty());
}

#[test]
fn unconnected_net_is_reported_until_routed() {
    let (mut e, a, _) = board();
    let fp = buturecad_pcb::generators::Soic::narrow(8)
        .generate()
        .unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let (f1, f2) = (e.new_id(), e.new_id());
    let mk = |r: &str, x: f64| {
        let mut pf = buturecad_pcb::PlacedFootprint {
            footprint: name.clone(),
            reference: r.into(),
            value: "x".into(),
            placement: buturecad_pcb::Placement::at(p(x, 15.)),
            locked: false,
            link: None,
            pad_nets: Default::default(),
        };
        pf.pad_nets.insert("1".into(), a);
        pf
    };
    e.execute(PcbEditCommand::new(
        "fp",
        vec![
            PcbChange::create(f1, PcbObject::Footprint(mk("U1", 8.))),
            PcbChange::create(f2, PcbObject::Footprint(mk("U2", 30.))),
        ],
    ))
    .unwrap();
    assert!(rules(&e).contains(&DrcRule::Unconnected));
}

#[test]
fn incremental_matches_from_scratch_over_an_edit_sequence() {
    let (mut e, a, b) = board();
    let mut eng = DrcEngine::new(e.project().pcb());
    let mut ids = Vec::new();
    for i in 0..12 {
        let y = 3.0 + f64::from(i) * 0.45;
        let id = e.new_id();
        let ch = PcbChange::create(
            id,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(p(5., y), p(25., y)),
                width: mm(0.25),
                net: Some(if i % 2 == 0 { a } else { b }),
            }),
        );
        e.execute(PcbEditCommand::new("t", vec![ch.clone()]))
            .unwrap();
        eng.update(e.project().pcb(), &[ch]);
        ids.push(id);
        assert_eq!(eng.report(), run_drc(e.project().pcb()), "after add {i}");
    }
    for id in ids.iter().step_by(3) {
        let before = e.project().pcb().object(*id);
        let ch = PcbChange {
            id: *id,
            before,
            after: None,
        };
        e.execute(PcbEditCommand::new("d", vec![ch.clone()]))
            .unwrap();
        eng.update(e.project().pcb(), &[ch]);
        assert_eq!(eng.report(), run_drc(e.project().pcb()), "after delete");
    }
}

#[test]
fn the_legs_of_a_differential_pair_keep_only_the_pair_gap() {
    let (mut e, a, b) = board();
    // 0.1 mm edge to edge: closer than the 0.2 mm class clearance.
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.25));
    track(&mut e, Some(b), p(5., 5.35), p(15., 5.35), mm(0.25));
    assert_eq!(rules(&e), vec![DrcRule::Clearance]);
    // Declared a pair with a 0.1 mm gap: the same copper is now fine.
    let before = e.project().pcb().rules.clone();
    let mut after = before.clone();
    after.diff_pairs.insert(
        "D".into(),
        buturecad_pcb::DiffPair {
            name: "D".into(),
            positive: a,
            negative: b,
            width: Some(mm(0.25)),
            gap: Some(mm(0.1)),
            max_skew: mm(0.5),
        },
    );
    e.execute(SetRulesCommand::new("pair", before, after))
        .unwrap();
    assert!(rules(&e).is_empty(), "{:?}", rules(&e));
    // A third net next to the pair still needs the full clearance.
    let c = {
        let id = e.new_id();
        e.execute(PcbEditCommand::new(
            "n",
            vec![PcbChange::create(
                id,
                PcbObject::Net(Net { name: "C".into() }),
            )],
        ))
        .unwrap();
        id
    };
    track(&mut e, Some(c), p(5., 4.65), p(15., 4.65), mm(0.25));
    assert!(rules(&e).contains(&DrcRule::Clearance));
}

#[test]
fn skew_between_the_legs_of_a_pair_is_a_warning_once_both_are_routed() {
    let (mut e, a, b) = board();
    let before = e.project().pcb().rules.clone();
    let mut after = before.clone();
    after.diff_pairs.insert(
        "D".into(),
        buturecad_pcb::DiffPair {
            name: "D".into(),
            positive: a,
            negative: b,
            width: None,
            gap: None,
            max_skew: mm(0.5),
        },
    );
    e.execute(SetRulesCommand::new("pair", before, after))
        .unwrap();
    track(&mut e, Some(a), p(5., 5.), p(15., 5.), mm(0.25));
    assert!(rules(&e).is_empty(), "an unrouted leg is not judged");
    track(&mut e, Some(b), p(5., 8.), p(14., 8.), mm(0.25));
    let report = run_drc(e.project().pcb());
    let v = report
        .violations
        .iter()
        .find(|v| v.rule == DrcRule::PairSkew)
        .unwrap();
    assert_eq!(v.severity, buturecad_rules::DrcSeverity::Warning);
    assert_eq!(v.actual, Some(mm(1.0)));
    assert_eq!(v.objects.len(), 2);
}
