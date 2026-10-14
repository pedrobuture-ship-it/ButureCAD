//! The board model through the Command Engine.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::generators::Soic;
use buturecad_pcb::{
    layer, BoardGraphic, BoardText, FpShape, Keepout, Net, PlacedFootprint, Placement, Stackup,
    Zone,
};
use std::collections::BTreeMap;

fn engine() -> (CommandEngine, String) {
    let mut e = CommandEngine::new(Project::new("B").unwrap());
    let fp = Soic::narrow(8).generate().unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    (e, name)
}

fn placed(name: &str, x: i64) -> PlacedFootprint {
    PlacedFootprint {
        footprint: name.into(),
        reference: "U1".into(),
        value: "x".into(),
        placement: Placement::at(Point::new(x, 0)),
        locked: false,
        link: None,
        pad_nets: BTreeMap::new(),
    }
}

fn exec(
    e: &mut CommandEngine,
    label: &str,
    c: Vec<PcbChange>,
) -> Result<(), buturecad_eda_core::CommandError> {
    e.execute(PcbEditCommand::new(label, c))
}

#[test]
fn every_board_object_kind_can_be_created_edited_deleted_and_undone() {
    let (mut e, fp) = engine();
    let net = e.new_id();
    let (f, t, v, z, k, tx, g) = (
        e.new_id(),
        e.new_id(),
        e.new_id(),
        e.new_id(),
        e.new_id(),
        e.new_id(),
        e.new_id(),
    );
    let sq = vec![
        Point::new(0, 0),
        Point::new(10_000_000, 0),
        Point::new(10_000_000, 10_000_000),
        Point::new(0, 10_000_000),
    ];
    let start = e.project().pcb().clone();
    exec(
        &mut e,
        "Build board",
        vec![
            PcbChange::create(net, PcbObject::Net(Net { name: "GND".into() })),
            PcbChange::create(f, PcbObject::Footprint(placed(&fp, 5_000_000))),
            PcbChange::create(
                t,
                PcbObject::Track(Track {
                    layer: layer::F_CU,
                    segment: Segment::new(Point::new(0, 0), Point::new(1_000_000, 0)),
                    width: 200_000,
                    net: Some(net),
                }),
            ),
            PcbChange::create(
                v,
                PcbObject::Via(Via {
                    position: Point::new(1_000_000, 0),
                    diameter: 800_000,
                    drill: 400_000,
                    net: Some(net),
                    kind: ViaKind::Through,
                }),
            ),
            PcbChange::create(
                z,
                PcbObject::Zone(Zone::new(layer::B_CU, sq.clone(), Some(net))),
            ),
            PcbChange::create(
                k,
                PcbObject::Keepout(Keepout {
                    outline: sq.clone(),
                    layers: vec![],
                    no_tracks: true,
                    no_vias: true,
                    no_zones: false,
                }),
            ),
            PcbChange::create(
                tx,
                PcbObject::Text(BoardText {
                    layer: layer::F_SILK,
                    position: Point::ORIGIN,
                    text: "Rev A".into(),
                    height: 1_000_000,
                    rotation: 0,
                }),
            ),
            PcbChange::create(
                g,
                PcbObject::Graphic(BoardGraphic {
                    layer: layer::EDGE_CUTS,
                    shape: FpShape::Polygon {
                        points: sq,
                        filled: false,
                        width: 100_000,
                    },
                }),
            ),
        ],
    )
    .unwrap();
    assert_eq!(e.project().pcb().object_count(), 8);
    assert_eq!(e.project().pcb().object(f).unwrap().kind(), "footprint");

    // Modify and delete.
    let before = e.project().pcb().object(f).unwrap();
    let mut moved = placed(&fp, 7_000_000);
    moved.locked = true;
    exec(
        &mut e,
        "Move",
        vec![PcbChange::modify(f, before, PcbObject::Footprint(moved))],
    )
    .unwrap();
    assert!(e.project().pcb().placed[&f].locked);
    let tr = e.project().pcb().object(t).unwrap();
    exec(&mut e, "Delete", vec![PcbChange::delete(t, tr)]).unwrap();
    assert!(e.project().pcb().tracks.is_empty());

    // Undo everything: the document is exactly as it was.
    while e.can_undo() {
        e.undo().unwrap();
        if e.project().pcb().object_count() == 0 {
            break;
        }
    }
    let mut now = e.project().pcb().clone();
    now.footprints = start.footprints.clone();
    assert_eq!(now, start);
}

#[test]
fn invalid_changes_are_atomic_and_precisely_refused() {
    let (mut e, fp) = engine();
    let (a, b) = (e.new_id(), e.new_id());
    let snapshot = e.project().pcb().clone();
    // The second object is invalid, so the first must not be created either.
    let r = exec(
        &mut e,
        "x",
        vec![
            PcbChange::create(a, PcbObject::Footprint(placed(&fp, 0))),
            PcbChange::create(
                b,
                PcbObject::Track(Track {
                    layer: layer::LayerId(5),
                    segment: Segment::new(Point::ORIGIN, Point::new(1, 0)),
                    width: 100,
                    net: None,
                }),
            ),
        ],
    );
    assert!(r.is_err(), "layer 5 does not exist on a 2-layer board");
    assert_eq!(e.project().pcb(), &snapshot);

    // More refusals: unknown footprint, unknown net, inner layer on a 2-layer board, stale before.
    assert!(exec(
        &mut e,
        "x",
        vec![PcbChange::create(
            a,
            PcbObject::Footprint(placed("Nope:X", 0))
        )]
    )
    .is_err());
    let mut p = placed(&fp, 0);
    p.pad_nets.insert("1".into(), b);
    assert!(exec(
        &mut e,
        "x",
        vec![PcbChange::create(a, PcbObject::Footprint(p))]
    )
    .is_err());
    assert!(exec(
        &mut e,
        "x",
        vec![PcbChange::create(
            a,
            PcbObject::Track(Track {
                layer: layer::inner_cu(1),
                segment: Segment::new(Point::ORIGIN, Point::new(1, 0)),
                width: 100,
                net: None
            })
        )]
    )
    .is_err());
    assert!(exec(
        &mut e,
        "x",
        vec![PcbChange::delete(
            a,
            PcbObject::Net(Net {
                name: "ghost".into()
            })
        )]
    )
    .is_err());
    assert_eq!(e.project().pcb(), &snapshot);
    assert!(!e.is_dirty() || e.can_undo());
}

#[test]
fn the_stackup_gates_which_layers_exist() {
    let (mut e, _) = engine();
    let t = e.new_id();
    let inner = Track {
        layer: layer::inner_cu(1),
        segment: Segment::new(Point::ORIGIN, Point::new(1_000_000, 0)),
        width: 200_000,
        net: None,
    };
    assert!(exec(
        &mut e,
        "x",
        vec![PcbChange::create(t, PcbObject::Track(inner.clone()))]
    )
    .is_err());
    e.execute(SetStackupCommand::new(
        Stackup { copper_layers: 2 },
        Stackup { copper_layers: 4 },
    ))
    .unwrap();
    exec(
        &mut e,
        "Route",
        vec![PcbChange::create(t, PcbObject::Track(inner))],
    )
    .unwrap();
    // A layer in use cannot disappear; invalid counts are refused.
    assert!(e
        .execute(SetStackupCommand::new(
            Stackup { copper_layers: 4 },
            Stackup { copper_layers: 2 }
        ))
        .is_err());
    assert!(e
        .execute(SetStackupCommand::new(
            Stackup { copper_layers: 4 },
            Stackup { copper_layers: 5 }
        ))
        .is_err());
    assert_eq!(e.project().pcb().stackup.copper_layers, 4);
    e.undo().unwrap();
    e.undo().unwrap();
    assert_eq!(e.project().pcb().stackup.copper_layers, 2);
}

#[test]
fn via_kinds_describe_the_layers_they_connect() {
    let s = Stackup { copper_layers: 6 };
    assert_eq!(ViaKind::Through.layers(&s).len(), 6);
    let blind = ViaKind::Blind {
        from: layer::F_CU,
        to: layer::inner_cu(2),
    };
    assert_eq!(
        blind.layers(&s),
        [layer::F_CU, layer::inner_cu(1), layer::inner_cu(2)]
    );
    let buried = ViaKind::Buried {
        from: layer::inner_cu(3),
        to: layer::inner_cu(1),
    };
    assert_eq!(buried.layers(&s).len(), 3);
    assert!(ViaKind::Micro {
        from: layer::F_CU,
        to: layer::LayerId(99)
    }
    .layers(&s)
    .is_empty());
}

#[test]
fn ids_continue_after_every_board_object() {
    let mut pcb = PcbDoc::default();
    pcb.nets
        .insert(buturecad_pcb::Id(77), Net { name: "N".into() });
    let mut p = Project::from_parts(
        "x",
        buturecad_schematic::Schematic::new(buturecad_eda_core::ROOT_SHEET),
        pcb,
    )
    .unwrap();
    let mut e = CommandEngine::new(p.clone());
    assert_eq!(e.new_id().0, 78);
    let _ = &mut p;
}
