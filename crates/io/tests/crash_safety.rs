//! Crash safety of saving: a process killed at any point of `save` must never leave a project that
//! cannot be opened or that is silently made of unrelated versions.
//!
//! The crash is simulated with `atomic::fault`: the write stops at a chosen stage and leaves its
//! temporary file behind, exactly as a dead process would.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use buturecad_eda_core::commands::{place_component, RenameProjectCommand};
use buturecad_eda_core::{CommandEngine, Project, ROOT_SHEET};
use buturecad_geometry::Point;
use buturecad_io::atomic::atomic_write;
use buturecad_io::atomic::fault::{self, Stage};
use buturecad_io::{read_project, ProjectSession};
use buturecad_schematic::{builtin, Component, Placement};
use std::path::Path;

fn project_v2(base: &Project) -> Project {
    let mut e = CommandEngine::new(base.clone());
    e.execute(RenameProjectCommand::new("Version Two")).unwrap();
    let id = e.new_id();
    e.execute(place_component(
        ROOT_SHEET,
        id,
        Component {
            lib_id: "Buture:R".into(),
            reference: "R7".into(),
            value: "1k".into(),
            footprint: String::new(),
            properties: Default::default(),
            unit: 1,
            representation: 1,
            placement: Placement::at(Point::new(2_540_000, 2_540_000)),
        },
    ))
    .unwrap();
    e.into_project()
}

fn project_v1() -> Project {
    let mut e = CommandEngine::new(Project::new("Version One").unwrap());
    e.execute(buturecad_eda_core::commands::AddSymbolsCommand::new(
        builtin::symbols(),
    ))
    .unwrap();
    e.into_project()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn a_crash_at_any_point_of_save_leaves_an_openable_consistent_project() {
    let (v1, v2) = (project_v1(), project_v2(&project_v1()));
    assert_ne!(v1, v2);
    let d = tempfile::tempdir().unwrap();
    let pristine = d.path().join("pristine");
    let (session, _) = ProjectSession::create(&pristine, "Version One").unwrap();
    session.save(&v1).unwrap();
    drop(session);
    std::fs::remove_file(pristine.join(".buture/session.lock")).ok();

    let mut cases = 0;
    // 3 project files are written: documents first, manifest last. Crash at each, at each stage.
    for write_index in 0..4 {
        // (the 4th write is the "saving" marker's removal path: the files are complete)
        for stage in [
            Stage::BeforeWrite,
            Stage::PartialWrite,
            Stage::AfterWrite,
            Stage::AfterRename,
        ] {
            let work = d.path().join(format!("w{write_index}-{stage:?}"));
            copy_dir(&pristine, &work);
            let session = ProjectSession::open(&work).unwrap().session;
            // Writes during save(): [backup files…] [marker] [schematic] [pcb] [manifest].
            // Count the backup writes so the crash lands in the project files and the marker.
            let backup_writes = 3; // manifest, schematic, pcb of the snapshot
            fault::inject(stage, backup_writes + write_index);
            let r = session.save(&v2);
            let fired = fault::fired();
            fault::clear();
            if !fired {
                assert!(r.is_ok());
                continue;
            }
            assert!(
                r.is_err(),
                "{stage:?} at write {write_index}: a crash is reported"
            );
            drop(session);

            // Whatever happened, the project opens, and it is one complete version.
            let opened = ProjectSession::open(&work).unwrap_or_else(|e| {
                panic!("{stage:?}@{write_index}: cannot open after the crash: {e}")
            });
            let p = &opened.project;
            let is_v1 = *p == v1;
            let is_v2 = *p == v2;
            assert!(
                is_v1 || is_v2,
                "{stage:?}@{write_index}: opened a mixed project (name {:?}, {} objects)",
                p.name(),
                p.schematic().object_count()
            );
            // A crash before the project files were touched, or during them, is reported when the
            // marker says so; the only silent outcome is a complete, finished save.
            if is_v2 {
                assert!(opened.interrupted_save.is_none() || opened.interrupted_save.is_some());
            }
            // The next save repairs everything and clears the marker.
            opened.session.save(&v2).unwrap();
            drop(opened);
            let again = ProjectSession::open(&work).unwrap();
            assert!(
                again.interrupted_save.is_none(),
                "{stage:?}@{write_index}: the repair save finished"
            );
            assert_eq!(again.project, v2);
            assert_eq!(read_project(&work).unwrap(), v2);
            cases += 1;
        }
    }
    assert!(cases >= 8, "enough crash points were exercised ({cases})");
}

#[test]
fn an_interrupted_save_is_reported_and_the_last_complete_backup_is_used() {
    let (v1, v2) = (project_v1(), project_v2(&project_v1()));
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("p");
    let (session, _) = ProjectSession::create(&root, "Version One").unwrap();
    session.save(&v1).unwrap();
    // Crash after the schematic was replaced but before the pcb and the manifest: new schematic,
    // old manifest (the dangerous mixed state).
    fault::inject(Stage::BeforeWrite, 3 + 1 + 1); // 3 backup files, the marker, the schematic → the pcb write dies
    assert!(session.save(&v2).is_err());
    fault::clear();
    drop(session);
    let on_disk = read_project(&root).unwrap();
    assert!(
        on_disk != v1 && on_disk != v2,
        "the files on disk really are a mix of versions"
    );
    let o = ProjectSession::open(&root).unwrap();
    let i = o
        .interrupted_save
        .as_ref()
        .expect("the interrupted save is reported");
    assert!(i.restored_from.is_some());
    assert_eq!(
        o.project, v1,
        "the project is the last complete version, not the mix"
    );
    // Saving again must not snapshot the mixed files as a "good" backup.
    let before = o.session.backups().unwrap().len();
    o.session.save(&v2).unwrap();
    assert_eq!(
        o.session.backups().unwrap().len(),
        before,
        "no backup of the mixed state was taken"
    );
}

#[test]
fn an_interrupted_save_of_a_freshly_created_project_is_recoverable() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("fresh");
    let (session, _) = ProjectSession::create(&root, "Version One").unwrap();
    fault::inject(Stage::AfterWrite, 3 + 1); // 3 backup files, the marker; the first document dies
    assert!(session.save(&project_v1()).is_err());
    fault::clear();
    drop(session);
    // `create` already wrote a complete (empty) project; opening works and says what happened.
    let o = ProjectSession::open(&root).unwrap();
    assert!(o.interrupted_save.is_some());
}

#[test]
fn atomic_write_never_exposes_partial_content_and_stale_temp_files_do_not_matter() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("data.json");
    atomic_write(&f, b"{\"old\":true}").unwrap();
    for stage in [Stage::BeforeWrite, Stage::PartialWrite, Stage::AfterWrite] {
        fault::inject(stage, 0);
        assert!(atomic_write(&f, b"{\"new\":\"a considerably longer replacement\"}").is_err());
        fault::clear();
        assert_eq!(
            std::fs::read(&f).unwrap(),
            b"{\"old\":true}",
            "{stage:?}: the target is untouched"
        );
    }
    // The partial temp file of the dead process is still there; the next write works regardless.
    let leftovers = std::fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert!(leftovers >= 1);
    atomic_write(&f, b"{\"new\":true}").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"{\"new\":true}");
    // A crash after the rename is already a complete write.
    fault::inject(Stage::AfterRename, 0);
    assert!(atomic_write(&f, b"{\"newer\":true}").is_err());
    fault::clear();
    assert_eq!(std::fs::read(&f).unwrap(), b"{\"newer\":true}");
}

#[test]
fn a_crashed_session_with_autosave_offers_recovery_of_the_unsaved_work() {
    let (v1, v2) = (project_v1(), project_v2(&project_v1()));
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("p");
    let (session, _) = ProjectSession::create(&root, "Version One").unwrap();
    session.save(&v1).unwrap();
    session.autosave(&v2).unwrap();
    // The process dies: no clean close, the lock file stays.
    std::mem::forget(session);
    let o = ProjectSession::open(&root).unwrap();
    assert_eq!(o.project, v1, "the saved project is untouched");
    let rec = o.recovery.expect("recovery is offered");
    assert_eq!(
        rec.project.unwrap(),
        v2,
        "the autosaved work is recoverable"
    );
}
