#![allow(clippy::unwrap_used, clippy::expect_used)]

use buturecad_eda_core::Project;
use buturecad_io::format::{parse_manifest_with, serialize_manifest, Migrations, CURRENT_SCHEMA};
use buturecad_io::session::{MANIFEST_NAME, MAX_BACKUPS};
use buturecad_io::{IoError, ProjectSession};
use serde_json::Value;
use std::fs;
use std::path::Path;

type Check = fn(&IoError) -> bool;

fn manifest(root: &Path) -> std::path::PathBuf {
    root.join(MANIFEST_NAME)
}

#[test]
fn create_makes_layout_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MyBoard");
    let (session, project) = ProjectSession::create(&root, "MyBoard").unwrap();
    for sub in [
        "schematic",
        "pcb",
        "libraries",
        "models",
        "scripts",
        "rules",
        "output",
    ] {
        assert!(root.join(sub).is_dir(), "{sub} missing");
    }
    drop(session);
    let opened = ProjectSession::open(&root).unwrap();
    assert_eq!(opened.project, project);
    assert!(opened.recovery.is_none());
}

#[test]
fn create_refuses_existing_project() {
    let dir = tempfile::tempdir().unwrap();
    let (_s, _) = ProjectSession::create(dir.path(), "A").unwrap();
    assert!(matches!(
        ProjectSession::create(dir.path(), "B"),
        Err(IoError::AlreadyExists(_))
    ));
}

#[test]
fn output_is_deterministic() {
    assert_eq!(serialize_manifest("Same"), serialize_manifest("Same"));
    let text = String::from_utf8(serialize_manifest("Same")).unwrap();
    assert_eq!(
        text,
        "{\n  \"format\": \"buturecad.project\",\n  \"schema_version\": 1,\n  \"name\": \"Same\"\n}\n"
    );
}

#[test]
fn save_persists_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = ProjectSession::create(dir.path(), "Old").unwrap();
    s.save(&Project::new("New").unwrap()).unwrap();
    drop(s);
    assert_eq!(
        ProjectSession::open(dir.path()).unwrap().project.name(),
        "New"
    );
}

#[test]
fn truncated_file_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = ProjectSession::create(dir.path(), "A").unwrap();
    drop(s);
    let full = fs::read(manifest(dir.path())).unwrap();
    fs::write(manifest(dir.path()), &full[..full.len() / 2]).unwrap();
    assert!(matches!(
        ProjectSession::open(dir.path()),
        Err(IoError::Parse { .. })
    ));
}

#[test]
fn invalid_files_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = manifest(dir.path());
    let cases: [(&str, Check); 5] = [
        ("", |e| matches!(e, IoError::Parse { .. })),
        ("[1,2]", |e| matches!(e, IoError::WrongFormat { .. })),
        (r#"{"format":"other","schema_version":1,"name":"x"}"#, |e| {
            matches!(e, IoError::WrongFormat { .. })
        }),
        (
            r#"{"format":"buturecad.project","schema_version":1,"name":""}"#,
            |e| matches!(e, IoError::Invalid { .. }),
        ),
        (
            r#"{"format":"buturecad.project","schema_version":1,"name":"x","bogus":1}"#,
            |e| matches!(e, IoError::Parse { .. }),
        ),
    ];
    for (text, check) in cases {
        fs::write(&path, text).unwrap();
        let err = ProjectSession::open(dir.path()).unwrap_err();
        assert!(check(&err), "unexpected error for {text:?}: {err}");
    }
    fs::write(&path, r#"{"format":"buturecad.project","name":"x"}"#).unwrap();
    assert!(matches!(
        ProjectSession::open(dir.path()),
        Err(IoError::InvalidSchema { .. })
    ));
}

#[test]
fn missing_project_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        ProjectSession::open(dir.path()),
        Err(IoError::NotFound(_))
    ));
}

#[test]
fn newer_schema_is_refused_with_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        r#"{{"format":"buturecad.project","schema_version":{},"name":"x"}}"#,
        CURRENT_SCHEMA + 1
    );
    let err = parse_manifest_with(dir.path(), text.as_bytes(), &Migrations::builtin()).unwrap_err();
    assert!(matches!(err, IoError::NewerSchema { .. }));
}

#[test]
fn old_versions_are_migrated_in_order() {
    // Synthetic registry: schema 1 stored `title`, schema 2 renamed it to `name`.
    fn rename(doc: &mut Value) -> Result<(), String> {
        let title = doc.get("title").cloned().ok_or("missing title")?;
        let obj = doc.as_object_mut().ok_or("not an object")?;
        obj.remove("title");
        obj.insert("name".into(), title);
        Ok(())
    }
    let registry = Migrations::new(1, vec![rename]);
    assert_eq!(registry.target(), 2);
    let old = br#"{"format":"buturecad.project","schema_version":1,"title":"Legacy"}"#;
    let p = parse_manifest_with(Path::new("x"), old, &registry).unwrap();
    assert_eq!(p.name, "Legacy");

    let broken = br#"{"format":"buturecad.project","schema_version":1}"#;
    assert!(matches!(
        parse_manifest_with(Path::new("x"), broken, &registry),
        Err(IoError::Migration { from: 1, .. })
    ));
}

#[test]
fn backups_rotate_and_skip_corrupt_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = ProjectSession::create(dir.path(), "v0").unwrap();
    let mut p = Project::new("v0").unwrap();
    for i in 1..=MAX_BACKUPS + 3 {
        p = Project::new(format!("v{i}")).unwrap();
        s.save(&p).unwrap();
    }
    let backups = s.backups().unwrap();
    assert_eq!(backups.len(), MAX_BACKUPS);
    // Newest backup holds the state before the last save.
    let newest = fs::read_to_string(backups.last().unwrap().join(MANIFEST_NAME)).unwrap();
    assert!(newest.contains(&format!("\"v{}\"", MAX_BACKUPS + 2)));

    // A corrupt manifest must not become a backup.
    fs::write(manifest(dir.path()), "garbage").unwrap();
    s.save(&p).unwrap();
    for b in s.backups().unwrap() {
        assert!(!fs::read_to_string(b.join(MANIFEST_NAME))
            .unwrap()
            .contains("garbage"));
    }
    assert!(ProjectSession::open(dir.path()).is_ok());
}

#[test]
fn crash_with_autosave_offers_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = ProjectSession::create(dir.path(), "Saved").unwrap();
    s.autosave(&Project::new("Unsaved work").unwrap()).unwrap();
    std::mem::forget(s); // simulate a crash: Drop never releases the lock

    let opened = ProjectSession::open(dir.path()).unwrap();
    assert_eq!(opened.project.name(), "Saved");
    let recovery = opened.recovery.expect("recovery offered");
    assert_eq!(recovery.project.unwrap().name(), "Unsaved work");

    // Saving discards the autosave.
    opened.session.save(&opened.project).unwrap();
    assert!(!recovery.autosave_dir.exists());
}

#[test]
fn clean_shutdown_does_not_offer_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let (s, p) = ProjectSession::create(dir.path(), "A").unwrap();
    s.autosave(&p).unwrap();
    drop(s);
    assert!(ProjectSession::open(dir.path()).unwrap().recovery.is_none());
}

#[test]
fn corrupt_autosave_is_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let (s, p) = ProjectSession::create(dir.path(), "A").unwrap();
    s.autosave(&p).unwrap();
    std::mem::forget(s);
    fs::write(dir.path().join(".buture/autosave/project.buture"), "{").unwrap();
    let opened = ProjectSession::open(dir.path()).unwrap();
    assert!(opened.recovery.unwrap().project.is_err());
}

#[test]
fn save_as_copies_files_and_switches_session() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let (mut s, _) = ProjectSession::create(&a, "A").unwrap();
    fs::write(a.join("scripts/x.beda"), "hello").unwrap();
    let p = Project::new("B").unwrap();
    s.save_as(&b, &p).unwrap();
    assert_eq!(s.root(), b);
    assert_eq!(
        fs::read_to_string(b.join("scripts/x.beda")).unwrap(),
        "hello"
    );
    assert!(!b.join(".buture/backups").exists());
    // Original is untouched.
    assert_eq!(ProjectSession::open(&a).unwrap().project.name(), "A");
    assert!(matches!(s.save_as(&a, &p), Err(IoError::AlreadyExists(_))));
}
