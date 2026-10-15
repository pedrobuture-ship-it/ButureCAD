//! Property tests for ERC and DRC on random designs: they never panic, give the same answer twice,
//! do not depend on where on the sheet/board the design is (translation invariance), and every
//! violation points at objects that exist.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::{
    create_object, create_wire, place_component, AddSymbolsCommand, PcbEditCommand,
};
use buturecad_eda_core::model::{PcbChange, PcbObject, Track, Via};
use buturecad_eda_core::{CommandEngine, Project, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net};
use buturecad_rules::{run_drc, run_erc};
use buturecad_schematic::{builtin, Component, Connectivity, GlobalLabel, Object, Placement};
use proptest::prelude::*;
use std::collections::BTreeMap;

const MM: i64 = 1_000_000;
const G: i64 = 1_270_000;

#[derive(Debug, Clone)]
enum PcbItem {
    Track {
        f: bool,
        a: (i64, i64),
        b: (i64, i64),
        w: i64,
        net: usize,
    },
    Via {
        at: (i64, i64),
        d: i64,
        drill: i64,
        net: usize,
    },
}

fn pcb_item() -> impl Strategy<Value = PcbItem> {
    prop_oneof![
        (
            any::<bool>(),
            (0i64..40, 0i64..40),
            (0i64..40, 0i64..40),
            100_000i64..800_000,
            0usize..3
        )
            .prop_map(|(f, a, b, w, net)| PcbItem::Track {
                f,
                a: (a.0 * MM, a.1 * MM),
                b: (b.0 * MM, b.1 * MM),
                w,
                net
            }),
        (
            (0i64..40, 0i64..40),
            500_000i64..1_200_000,
            100_000i64..500_000,
            0usize..3
        )
            .prop_map(|(at, d, drill, net)| PcbItem::Via {
                at: (at.0 * MM, at.1 * MM),
                d,
                drill,
                net
            }),
    ]
}

fn board(items: &[PcbItem], dx: i64, dy: i64) -> Project {
    let mut e = CommandEngine::new(Project::new("prop").unwrap());
    let nets: Vec<_> = (0..3).map(|_| e.new_id()).collect();
    let mut ch: Vec<PcbChange> = nets
        .iter()
        .enumerate()
        .map(|(i, id)| {
            PcbChange::create(
                *id,
                PcbObject::Net(Net {
                    name: format!("N{i}"),
                }),
            )
        })
        .collect();
    for it in items {
        let id = e.new_id();
        match it {
            PcbItem::Track { f, a, b, w, net } => {
                if a == b {
                    continue;
                }
                ch.push(PcbChange::create(
                    id,
                    PcbObject::Track(Track {
                        layer: if *f { layer::F_CU } else { layer::B_CU },
                        segment: Segment::new(
                            Point::new(a.0 + dx, a.1 + dy),
                            Point::new(b.0 + dx, b.1 + dy),
                        ),
                        width: *w,
                        net: Some(nets[*net]),
                    }),
                ));
            }
            PcbItem::Via { at, d, drill, net } => {
                if drill >= d {
                    continue;
                }
                ch.push(PcbChange::create(
                    id,
                    PcbObject::Via(Via {
                        position: Point::new(at.0 + dx, at.1 + dy),
                        diameter: *d,
                        drill: *drill,
                        net: Some(nets[*net]),
                        kind: Default::default(),
                    }),
                ));
            }
        }
    }
    e.execute(PcbEditCommand::new("random board", ch)).unwrap();
    e.into_project()
}

fn drc_summary(p: &Project) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in run_drc(p.pcb()).violations {
        *m.entry(format!("{}:{:?}", v.rule.code(), v.severity))
            .or_default() += 1;
    }
    m
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, ..ProptestConfig::default() })]

    #[test]
    fn drc_is_deterministic_translation_invariant_and_points_at_real_objects(
        items in proptest::collection::vec(pcb_item(), 0..40),
        dx in -20i64..20, dy in -20i64..20,
    ) {
        let p = board(&items, 0, 0);
        let (a, b) = (run_drc(p.pcb()), run_drc(p.pcb()));
        prop_assert_eq!(a.violations.len(), b.violations.len());
        for (x, y) in a.violations.iter().zip(&b.violations) {
            prop_assert_eq!((&x.message, x.rule.code()), (&y.message, y.rule.code()));
        }
        for v in &a.violations {
            for id in &v.objects {
                prop_assert!(p.pcb().object(*id).is_some(), "violation {:?} refers to missing object {:?}", v.rule.code(), id);
            }
        }
        let moved = board(&items, dx * 7_000 + 13, dy * 5_000 - 7);
        prop_assert_eq!(drc_summary(&p), drc_summary(&moved), "moving the whole design must not change the findings");
    }
}

#[derive(Debug, Clone)]
enum SchItem {
    Part { c: bool, at: (i64, i64) },
    Wire { a: (i64, i64), b: (i64, i64) },
    Label { at: (i64, i64), name: u8 },
}

fn sch_item() -> impl Strategy<Value = SchItem> {
    let pt = (0i64..12, 0i64..12);
    prop_oneof![
        (any::<bool>(), pt.clone()).prop_map(|(c, at)| SchItem::Part { c, at }),
        (pt.clone(), pt.clone()).prop_map(|(a, b)| SchItem::Wire { a, b }),
        (pt, 0u8..4).prop_map(|(at, name)| SchItem::Label { at, name }),
    ]
}

fn schematic(items: &[SchItem], shift: i64) -> Project {
    let mut e = CommandEngine::new(Project::new("prop").unwrap());
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let p = |(x, y): (i64, i64)| Point::new((x * 3 + shift) * G, (y * 3 + shift) * G);
    let mut n = 0;
    for it in items {
        let id = e.new_id();
        let r = match it {
            SchItem::Part { c, at } => {
                n += 1;
                e.execute(place_component(
                    ROOT_SHEET,
                    id,
                    Component {
                        lib_id: if *c { "Buture:C" } else { "Buture:R" }.into(),
                        reference: format!("{}{n}", if *c { "C" } else { "R" }),
                        value: "1".into(),
                        footprint: String::new(),
                        properties: Default::default(),
                        unit: 1,
                        representation: 1,
                        placement: Placement::at(p(*at)),
                    },
                ))
            }
            SchItem::Wire { a, b } if a != b => {
                e.execute(create_wire(ROOT_SHEET, id, Segment::new(p(*a), p(*b))))
            }
            SchItem::Wire { .. } => continue,
            SchItem::Label { at, name } => e.execute(create_object(
                "label",
                ROOT_SHEET,
                id,
                Object::GlobalLabel(GlobalLabel {
                    position: p(*at),
                    name: format!("L{name}"),
                    orientation: buturecad_schematic::Direction::Right,
                }),
            )),
        };
        // Random edits may be refused (overlaps, degenerate shapes): that must be an error, not a panic.
        let _ = r;
    }
    e.into_project()
}

fn erc_summary(p: &Project) -> (usize, BTreeMap<String, usize>) {
    let sch = p.schematic();
    let conn = Connectivity::build(sch);
    let mut m = BTreeMap::new();
    for v in run_erc(sch, &conn, sch.waivers()).violations {
        *m.entry(format!("{}:{:?}", v.rule.code(), v.severity))
            .or_default() += 1;
    }
    (conn.nets().len(), m)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, ..ProptestConfig::default() })]

    #[test]
    fn erc_and_connectivity_are_deterministic_and_translation_invariant(items in proptest::collection::vec(sch_item(), 0..40), shift in 0i64..30) {
        let p = schematic(&items, 0);
        prop_assert_eq!(erc_summary(&p), erc_summary(&p));
        let moved = schematic(&items, shift);
        prop_assert_eq!(erc_summary(&p), erc_summary(&moved), "moving the sheet content must not change connectivity or findings");
        for v in run_erc(p.schematic(), &Connectivity::build(p.schematic()), p.schematic().waivers()).violations {
            for id in &v.objects {
                prop_assert!(p.schematic().object(*id).is_some(), "ERC points at a missing object");
            }
        }
    }
}
