#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, CommandError, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_io::format::{migrate_schematic_v1_to_v2, serialize_pcb, serialize_schematic};
use buturecad_io::session::{MANIFEST_NAME, PCB_FILE, SCHEMATIC_FILE};
use buturecad_io::{engine, IoError, ProjectSession};
use buturecad_schematic::{builtin, Component, Object, Placement, Waiver};
use std::fs;

fn comp(r: &str, lib: &str) -> Component {
    Component {
        lib_id: lib.into(),
        reference: r.into(),
        value: "100n".into(),
        footprint: String::new(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement {
            position: Point::new(1_270_000, 2_540_000),
            rotation: 1,
            mirror: false,
        },
    }
}

fn place(e: &mut CommandEngine, r: &str) {
    let id = e.new_id();
    e.execute(place_component(ROOT_SHEET, id, comp(r, "Buture:C")))
        .unwrap();
}

fn build_design(e: &mut CommandEngine) {
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    place(e, "C1");
    place(e, "R1");
    let w = e.new_id();
    e.execute(create_wire(
        ROOT_SHEET,
        w,
        Segment::new(Point::new(0, 0), Point::new(5, 0)),
    ))
    .unwrap();
    e.execute(CreateTrackCommand::new(Track {
        layer: LayerId(1),
        segment: Segment::new(Point::new(0, 0), Point::new(10_000_000, 0)),
        width: 250_000,
        net: None,
    }))
    .unwrap();
    e.execute(CreateViaCommand::new(Via {
        position: Point::new(7, 7),
        diameter: 600_000,
        drill: 300_000,
        net: None,
        kind: Default::default(),
    }))
    .unwrap();
}

fn count(e: &CommandEngine) -> usize {
    e.project().schematic().object_count()
}

#[test]
fn design_survives_save_and_reopen_through_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    build_design(&mut e);
    assert!(e.is_dirty());
    engine::save(&session, &mut e).unwrap();
    assert!(!e.is_dirty());
    let saved = e.into_project();
    drop(session);

    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(opened.project, saved);
    // Ids resume after the highest stored id, so new objects never collide with loaded ones.
    let mut e = CommandEngine::new(opened.project);
    place(&mut e, "L1");
    assert_eq!(count(&e), 4);
}

#[test]
fn saved_files_are_deterministic_and_human_readable() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    build_design(&mut e);
    engine::save(&session, &mut e).unwrap();
    let first = fs::read(dir.path().join(SCHEMATIC_FILE)).unwrap();
    engine::save(&session, &mut e).unwrap();
    assert_eq!(first, fs::read(dir.path().join(SCHEMATIC_FILE)).unwrap());
    assert_eq!(first, serialize_schematic(e.project().schematic()));
    assert_eq!(
        fs::read(dir.path().join(PCB_FILE)).unwrap(),
        serialize_pcb(e.project().pcb())
    );
    let text = String::from_utf8(first).unwrap();
    assert!(
        text.contains("\"reference\": \"C1\"")
            && text.contains("\"schema_version\": 3")
            && text.ends_with("}\n")
    );
}

#[test]
fn crash_recovery_restores_unsaved_design_as_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    build_design(&mut e);
    engine::save(&session, &mut e).unwrap();
    place(&mut e, "R9");
    assert!(engine::autosave_if_pending(&session, &mut e).unwrap());
    assert!(
        !engine::autosave_if_pending(&session, &mut e).unwrap(),
        "nothing new to autosave"
    );
    std::mem::forget(session); // crash

    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(
        opened.project.schematic().object_count(),
        3,
        "disk still has the last save"
    );
    let recovered = opened.recovery.unwrap().project.unwrap();
    assert_eq!(recovered.schematic().object_count(), 4);
    assert!(CommandEngine::recovered(recovered).is_dirty());
}

#[test]
fn saving_clears_the_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.execute(RenameProjectCommand::new("X")).unwrap();
    engine::autosave_if_pending(&session, &mut e).unwrap();
    assert!(dir.path().join(".buture/autosave").exists());
    engine::save(&session, &mut e).unwrap();
    assert!(!dir.path().join(".buture/autosave").exists());
}

#[test]
fn save_is_refused_inside_a_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.begin_transaction("t");
    e.execute(RenameProjectCommand::new("X")).unwrap();
    assert!(matches!(
        engine::save(&session, &mut e),
        Err(IoError::Command(CommandError::TransactionOpen))
    ));
    e.rollback_transaction().unwrap();
}

#[test]
fn failed_write_does_not_mark_the_engine_clean() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.execute(RenameProjectCommand::new("X")).unwrap();
    // Block the schematic file path with a non-empty directory so the atomic rename fails.
    let target = dir.path().join(SCHEMATIC_FILE);
    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("x"), b"x").unwrap();
    assert!(engine::save(&session, &mut e).is_err());
    assert!(e.is_dirty());
}

#[test]
fn legacy_project_without_documents_opens_empty() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(MANIFEST_NAME),
        "{\n  \"format\": \"buturecad.project\",\n  \"schema_version\": 1,\n  \"name\": \"Old\"\n}\n",
    )
    .unwrap();
    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(opened.project.schematic().object_count(), 0);
}

/// A schematic exactly as written by the pre-release (schema 1) builds.
const V1_SCHEMATIC: &str = r#"{
  "format": "buturecad.schematic",
  "schema_version": 1,
  "components": {
    "1": { "reference": "C1", "value": "100n", "position": { "x": 1270000, "y": 2540000 }, "rotation": 1 }
  },
  "wires": {
    "3": { "segment": { "a": { "x": 0, "y": 0 }, "b": { "x": 5, "y": 0 } } }
  }
}
"#;

#[test]
fn schema_v1_schematic_is_migrated_on_open_and_rewritten_as_v2() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = ProjectSession::create(dir.path(), "Old").unwrap();
    drop(session);
    fs::write(dir.path().join(SCHEMATIC_FILE), V1_SCHEMATIC).unwrap();

    let opened = ProjectSession::open(dir.path()).unwrap();
    let sch = opened.project.schematic();
    assert_eq!(sch.object_count(), 2);
    match sch.object(Id(1)).unwrap().1 {
        Object::Component(c) => {
            assert_eq!(
                (c.reference.as_str(), c.value.as_str(), c.lib_id.as_str()),
                ("C1", "100n", "")
            );
            assert_eq!(
                c.placement,
                Placement {
                    position: Point::new(1_270_000, 2_540_000),
                    rotation: 1,
                    mirror: false
                }
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(sch.object(Id(3)).unwrap().1, Object::Wire(_)));

    // Ids continue after the migrated ones, and saving upgrades the file.
    let mut e = CommandEngine::new(opened.project);
    assert_eq!(e.new_id(), Id(4));
    engine::save(&opened.session, &mut e).unwrap();
    let text = fs::read_to_string(dir.path().join(SCHEMATIC_FILE)).unwrap();
    assert!(text.contains("\"schema_version\": 3"));
    assert_eq!(
        ProjectSession::open(dir.path()).unwrap().project,
        e.into_project()
    );
}

#[test]
fn v1_migration_rejects_malformed_documents() {
    for bad in [
        r#"{"schema_version":1}"#,
        r#"{"components":{"1":{"reference":"R"}},"wires":{}}"#,
        r#"{"components":{"1":{"reference":"R","value":"","position":{"x":0,"y":0},"rotation":0}},"wires":{"1":{"segment":{"a":{"x":0,"y":0},"b":{"x":1,"y":0}}}}}"#,
        r#"[1]"#,
    ] {
        let mut v: serde_json::Value = serde_json::from_str(bad).unwrap();
        assert!(
            migrate_schematic_v1_to_v2(&mut v).is_err(),
            "accepted {bad}"
        );
    }
}

#[test]
fn corrupt_or_foreign_documents_are_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    drop(session);
    drop(project);
    let path = dir.path().join(SCHEMATIC_FILE);
    let cases = [
        "{",
        r#"{"format":"buturecad.pcb","schema_version":2,"root":0,"symbols":{},"sheets":{}}"#,
        r#"{"format":"buturecad.schematic","schema_version":9,"root":0,"symbols":{},"sheets":{}}"#,
        r#"{"format":"buturecad.schematic","schema_version":2,"root":0,"symbols":{},"sheets":{},"extra":1}"#,
        // Root sheet missing.
        r#"{"format":"buturecad.schematic","schema_version":2,"root":0,"symbols":{},"sheets":{}}"#,
        // Component referencing a symbol that is not in the table.
        r#"{"format":"buturecad.schematic","schema_version":2,"root":0,"symbols":{},"sheets":{"0":{"name":"Root","objects":{"5":{"type":"component","lib_id":"Nope:X","reference":"R1","value":"","footprint":"","properties":{},"placement":{"position":{"x":0,"y":0},"rotation":0,"mirror":false}}}}}}"#,
        // Unknown object type.
        r#"{"format":"buturecad.schematic","schema_version":2,"root":0,"symbols":{},"sheets":{"0":{"name":"Root","objects":{"5":{"type":"teapot"}}}}}"#,
    ];
    for text in cases {
        fs::write(&path, text).unwrap();
        assert!(ProjectSession::open(dir.path()).is_err(), "accepted {text}");
    }
}

#[test]
fn backups_include_documents() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    place(&mut e, "C1");
    engine::save(&session, &mut e).unwrap(); // backs up the empty initial state
    place(&mut e, "R1");
    engine::save(&session, &mut e).unwrap(); // backs up the 1-component state
    let backups = session.backups().unwrap();
    assert_eq!(backups.len(), 2);
    let latest = fs::read_to_string(backups[1].join(SCHEMATIC_FILE)).unwrap();
    assert!(latest.contains("C1") && !latest.contains("R1"));
}

#[test]
fn schema_2_files_load_and_waivers_round_trip_through_save_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    // A schema-2 file as written before units and waivers existed.
    fs::write(
        dir.path().join(SCHEMATIC_FILE),
        r#"{"format":"buturecad.schematic","schema_version":2,"root":0,"symbols":{},"sheets":{"0":{"name":"Root","objects":{}}}}"#,
    )
    .unwrap();
    drop((session, project));
    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(opened.project.schematic().waivers().count(), 0);

    let waiver = Waiver {
        fingerprint: "unconnected_pin|J1|2".into(),
        rule: "unconnected_pin".into(),
        subject: "J1 pin 2".into(),
        reason: "reserved for the programming header".into(),
    };
    let mut e = CommandEngine::new(opened.project);
    e.execute(SetWaiversCommand::new(
        "Waive",
        vec![],
        vec![waiver.clone()],
    ))
    .unwrap();
    engine::save(&opened.session, &mut e).unwrap();
    let text = fs::read_to_string(dir.path().join(SCHEMATIC_FILE)).unwrap();
    assert!(
        text.contains("\"schema_version\": 3")
            && text.contains("reserved for the programming header")
    );
    let reopened = ProjectSession::open(dir.path()).unwrap().project;
    assert_eq!(
        reopened.schematic().waiver(&waiver.fingerprint),
        Some(&waiver)
    );

    // Undo removes it again; a waiver without a reason is refused and changes nothing.
    e.undo().unwrap();
    assert_eq!(e.project().schematic().waivers().count(), 0);
    let mut empty_reason = waiver.clone();
    empty_reason.reason = "  ".into();
    assert!(e
        .execute(SetWaiversCommand::new("Waive", vec![], vec![empty_reason]))
        .is_err());
    assert_eq!(e.project().schematic().waivers().count(), 0);
}

#[test]
fn the_board_round_trips_and_older_pcb_files_still_load() {
    use buturecad_eda_core::model::{PcbChange, PcbObject};
    use buturecad_pcb::{generators::Dip, layer, Net, PlacedFootprint, Placement, Stackup};
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "B").unwrap();
    // A schema-2 PCB file as written before the board existed.
    fs::write(
        dir.path().join(PCB_FILE),
        r#"{"format":"buturecad.pcb","schema_version":2,"tracks":{},"vias":{},"footprints":{}}"#,
    )
    .unwrap();
    drop((session, project));
    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(opened.project.pcb().stackup.copper_layers, 2);

    let mut e = CommandEngine::new(opened.project);
    let fp = Dip::standard(8).generate().unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    e.execute(SetStackupCommand::new(
        Stackup { copper_layers: 2 },
        Stackup { copper_layers: 4 },
    ))
    .unwrap();
    let (net, f, t) = (e.new_id(), e.new_id(), e.new_id());
    e.execute(PcbEditCommand::new(
        "board",
        vec![
            PcbChange::create(net, PcbObject::Net(Net { name: "VCC".into() })),
            PcbChange::create(
                f,
                PcbObject::Footprint(PlacedFootprint {
                    footprint: name,
                    reference: "U1".into(),
                    value: "NE555".into(),
                    placement: Placement {
                        position: Point::new(10_000_000, 5_000_000),
                        rotation: 1,
                        flipped: true,
                    },
                    locked: true,
                    link: None,
                    pad_nets: [("8".to_string(), net)].into_iter().collect(),
                }),
            ),
            PcbChange::create(
                t,
                PcbObject::Track(Track {
                    layer: layer::inner_cu(1),
                    segment: Segment::new(Point::ORIGIN, Point::new(1_000_000, 0)),
                    width: 200_000,
                    net: Some(net),
                }),
            ),
        ],
    ))
    .unwrap();
    let before = e.project().pcb().rules.clone();
    let mut after = before.clone();
    after
        .classes
        .insert("Power".into(), buturecad_pcb::NetClass::new("Power"));
    after.assignments.insert(net, "Power".into());
    let other = e.new_id();
    e.execute(PcbEditCommand::new(
        "n",
        vec![PcbChange::create(
            other,
            PcbObject::Net(Net { name: "X-".into() }),
        )],
    ))
    .unwrap();
    after.length_targets.insert(
        net,
        buturecad_pcb::LengthTarget {
            target: 50_000_000,
            tolerance: 100_000,
        },
    );
    after.diff_pairs.insert(
        "X".into(),
        buturecad_pcb::DiffPair {
            name: "X".into(),
            positive: net,
            negative: other,
            width: None,
            gap: Some(150_000),
            max_skew: 250_000,
        },
    );
    e.execute(buturecad_eda_core::commands::SetRulesCommand::new(
        "rules", before, after,
    ))
    .unwrap();
    engine::save(&opened.session, &mut e).unwrap();
    let text = fs::read_to_string(dir.path().join(PCB_FILE)).unwrap();
    assert!(text.contains("\"schema_version\": 7") && text.contains("NE555"));
    let net_field = format!("\"net\": {}", net.0);
    assert!(text.contains(&net_field));
    assert_eq!(
        ProjectSession::open(dir.path()).unwrap().project,
        e.into_project()
    );

    // A hand-damaged board is refused, not half-loaded.
    assert!(text.contains("\"Power\""));
    for bad in [
        text.replace("\"copper_layers\": 4", "\"copper_layers\": 3"),
        text.replace("\"Power\": \"Power\"", "\"Power\": \"Gone\"")
            .replace(
                &format!("\"{}\": \"Power\"", net.0),
                &format!("\"{}\": \"Gone\"", net.0),
            ),
        text.replace(&net_field, "\"net\": 999"),
    ] {
        fs::write(dir.path().join(PCB_FILE), bad).unwrap();
        assert!(ProjectSession::open(dir.path()).is_err());
    }
}

#[test]
fn zone_fills_round_trip_and_malformed_fills_are_refused() {
    use buturecad_pcb::{layer, FilledRegion, Zone, ZoneFill};
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Z").unwrap();
    let mut e = CommandEngine::new(project);
    let id = e.new_id();
    let sq = |x0: i64, x1: i64| {
        vec![
            Point::new(x0, x0),
            Point::new(x1, x0),
            Point::new(x1, x1),
            Point::new(x0, x1),
        ]
    };
    let mut zone = Zone::new(layer::F_CU, sq(0, 10_000_000), None);
    zone.fill = Some(ZoneFill {
        regions: vec![FilledRegion {
            outer: sq(0, 10_000_000),
            holes: vec![sq(3_000_000, 4_000_000)],
        }],
        signature: 0xDEAD_BEEF,
    });
    zone.connection = buturecad_pcb::PadConnection::Solid;
    zone.priority = 4;
    e.execute(PcbEditCommand::new(
        "zone",
        vec![PcbChange::create(id, PcbObject::Zone(zone))],
    ))
    .unwrap();
    engine::save(&session, &mut e).unwrap();
    let text = fs::read_to_string(dir.path().join(PCB_FILE)).unwrap();
    assert!(text.contains("\"solid\"") && text.contains("\"holes\""));
    assert_eq!(
        ProjectSession::open(dir.path()).unwrap().project,
        e.into_project()
    );
    // A fill with a two-point ring is refused as a whole.
    let bad = text.replace(
        "\"holes\": [",
        "\"holes\": [[{\"x\": 1, \"y\": 1}, {\"x\": 2, \"y\": 2}],",
    );
    assert_ne!(bad, text);
    fs::write(dir.path().join(PCB_FILE), bad).unwrap();
    assert!(ProjectSession::open(dir.path()).is_err());
}
