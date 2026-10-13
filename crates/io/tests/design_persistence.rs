#![allow(clippy::unwrap_used)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, CommandError};
use buturecad_geometry::{Point, Segment};
use buturecad_io::format::{serialize_pcb, serialize_schematic};
use buturecad_io::session::{MANIFEST_NAME, PCB_FILE, SCHEMATIC_FILE};
use buturecad_io::{engine, IoError, ProjectSession};
use std::fs;

fn comp(r: &str) -> Component {
    Component {
        reference: r.into(),
        value: "100n".into(),
        position: Point::new(1_270_000, 2_540_000),
        rotation: 1,
    }
}

fn build_design(e: &mut CommandEngine) {
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
    e.execute(PlaceComponentCommand::new(comp("R1"))).unwrap();
    e.execute(CreateWireCommand::new(Wire {
        segment: Segment::new(Point::new(0, 0), Point::new(5, 0)),
    }))
    .unwrap();
    e.execute(CreateTrackCommand::new(Track {
        layer: LayerId(1),
        segment: Segment::new(Point::new(0, 0), Point::new(10_000_000, 0)),
        width: 250_000,
    }))
    .unwrap();
    e.execute(CreateViaCommand::new(Via {
        position: Point::new(7, 7),
        diameter: 600_000,
        drill: 300_000,
    }))
    .unwrap();
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
    e.execute(PlaceComponentCommand::new(comp("L1"))).unwrap();
    assert_eq!(e.project().schematic().components.len(), 3);
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
    assert!(text.contains("\"reference\": \"C1\"") && text.ends_with("}\n"));
}

#[test]
fn crash_recovery_restores_unsaved_design_as_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
    engine::save(&session, &mut e).unwrap();
    e.execute(PlaceComponentCommand::new(comp("R1"))).unwrap();
    assert!(engine::autosave_if_pending(&session, &mut e).unwrap());
    assert!(
        !engine::autosave_if_pending(&session, &mut e).unwrap(),
        "nothing new to autosave"
    );
    std::mem::forget(session); // crash

    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(
        opened.project.schematic().components.len(),
        1,
        "disk still has the last save"
    );
    let recovered = opened.recovery.unwrap().project.unwrap();
    assert_eq!(recovered.schematic().components.len(), 2);
    let e = CommandEngine::recovered(recovered);
    assert!(e.is_dirty());
}

#[test]
fn saving_clears_the_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let (session, project) = ProjectSession::create(dir.path(), "Board").unwrap();
    let mut e = CommandEngine::new(project);
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
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
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
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
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
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
    assert!(opened.project.schematic().components.is_empty());
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
        r#"{"format":"buturecad.pcb","schema_version":1,"components":{},"wires":{}}"#,
        r#"{"format":"buturecad.schematic","schema_version":9,"components":{},"wires":{}}"#,
        r#"{"format":"buturecad.schematic","schema_version":1,"components":{"x":{}},"wires":{}}"#,
        r#"{"format":"buturecad.schematic","schema_version":1,"components":{},"wires":{},"extra":1}"#,
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
    e.execute(PlaceComponentCommand::new(comp("C1"))).unwrap();
    engine::save(&session, &mut e).unwrap(); // backs up the empty initial state
    e.execute(PlaceComponentCommand::new(comp("R1"))).unwrap();
    engine::save(&session, &mut e).unwrap(); // backs up the 1-component state
    let backups = session.backups().unwrap();
    assert_eq!(backups.len(), 2);
    let latest = fs::read_to_string(backups[1].join(SCHEMATIC_FILE)).unwrap();
    assert!(latest.contains("C1") && !latest.contains("R1"));
}
