//! Fuzzing the file parsers: damaged project files and damaged foreign files must produce errors,
//! never panics, never changes to the files, and anything that *is* accepted must be consistent.
//!
//! Deterministic (seeded) mutation fuzzing; `FUZZ_ITERS=<n>` scales it (see `buturecad-testkit`).
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_eda_core::commands::{place_component, AddSymbolsCommand};
use buturecad_eda_core::{CommandEngine, Project, ROOT_SHEET};
use buturecad_geometry::Point;
use buturecad_io::import::{kicad_netlist, Registry};
use buturecad_io::{read_project, ProjectSession};
use buturecad_schematic::{builtin, Component, Placement};
use buturecad_testkit::{as_text, iterations, mutations};
use std::path::Path;

fn sample_project() -> Project {
    let mut e = CommandEngine::new(Project::new("Fuzz").unwrap());
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    for (i, r) in ["R1", "R2", "C1"].iter().enumerate() {
        let id = e.new_id();
        e.execute(place_component(
            ROOT_SHEET,
            id,
            Component {
                lib_id: if r.starts_with('R') {
                    "Buture:R"
                } else {
                    "Buture:C"
                }
                .into(),
                reference: (*r).into(),
                value: "1k".into(),
                footprint: "0603".into(),
                properties: Default::default(),
                unit: 1,
                representation: 1,
                placement: Placement::at(Point::new(i as i64 * 10_160_000, 5_080_000)),
            },
        ))
        .unwrap();
    }
    e.into_project()
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

#[test]
fn damaged_project_files_never_panic_never_change_and_accepted_ones_are_consistent() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("p");
    let project = sample_project();
    let (session, _) = ProjectSession::create(&root, "Fuzz").unwrap();
    session.save(&project).unwrap();
    drop(session);
    let files = [
        "project.buture.json",
        "schematic/schematic.json",
        "pcb/pcb.json",
    ];
    // Which names the format really uses.
    let present: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!present.is_empty());
    let mut docs: Vec<std::path::PathBuf> = Vec::new();
    for f in walk(&root) {
        if !f.to_string_lossy().contains("/.buture/") {
            docs.push(f);
        }
    }
    assert!(docs.len() >= 3, "{docs:?} / {files:?}");
    let n = iterations(150);
    let (mut accepted, mut rejected) = (0, 0);
    for (k, doc) in docs.iter().enumerate() {
        let original = read(doc);
        for (i, bad) in mutations(&original, 1000 + k as u64, n).enumerate() {
            std::fs::write(doc, &bad).unwrap();
            let before: Vec<Vec<u8>> = docs.iter().map(|p| read(p)).collect();
            match read_project(&root) {
                Ok(p) => {
                    accepted += 1;
                    // Accepted ⇒ consistent: it saves and reads back to the same project.
                    let copy = d.path().join(format!("copy-{k}-{i}"));
                    let (s, _) = ProjectSession::create(&copy, p.name()).unwrap();
                    s.save(&p).unwrap();
                    assert_eq!(
                        read_project(&copy).unwrap(),
                        p,
                        "an accepted project must round-trip"
                    );
                    let _ = std::fs::remove_dir_all(&copy);
                }
                Err(e) => {
                    rejected += 1;
                    assert!(!e.to_string().is_empty());
                }
            }
            let after: Vec<Vec<u8>> = docs.iter().map(|p| read(p)).collect();
            assert_eq!(before, after, "reading never modifies project files");
        }
        std::fs::write(doc, original).unwrap();
    }
    assert!(
        rejected > accepted,
        "most damage is detected ({rejected} rejected, {accepted} accepted)"
    );
    assert_eq!(read_project(&root).unwrap(), project);
    // Opening a damaged project through a session fails cleanly too and leaves files alone.
    let doc = &docs[1];
    std::fs::write(doc, b"{ not json").unwrap();
    let keep = read(doc);
    assert!(ProjectSession::open(&root).is_err());
    assert_eq!(read(doc), keep);
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[test]
fn schema_version_tampering_is_an_error_not_a_crash() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("p");
    let (s, _) = ProjectSession::create(&root, "Fuzz").unwrap();
    s.save(&sample_project()).unwrap();
    drop(s);
    for doc in walk(&root).into_iter().filter(|f| {
        f.extension().is_some_and(|e| e == "json") && !f.to_string_lossy().contains(".buture")
    }) {
        let original = read(&doc);
        let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        if value.get("schema_version").is_none() {
            continue;
        }
        for v in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(9999),
            serde_json::json!("x"),
            serde_json::json!(null),
            serde_json::json!(1e30),
            serde_json::json!([1]),
        ] {
            let mut t = value.clone();
            t["schema_version"] = v.clone();
            std::fs::write(&doc, serde_json::to_vec(&t).unwrap()).unwrap();
            assert!(
                read_project(&root).is_err(),
                "{}: schema_version {v} must be refused",
                doc.display()
            );
        }
        // A missing version and a wrong format tag are refused too.
        let mut t = value.clone();
        t.as_object_mut().unwrap().remove("schema_version");
        std::fs::write(&doc, serde_json::to_vec(&t).unwrap()).unwrap();
        assert!(read_project(&root).is_err());
        let mut t = value.clone();
        t["format"] = serde_json::json!("something.else");
        std::fs::write(&doc, serde_json::to_vec(&t).unwrap()).unwrap();
        assert!(read_project(&root).is_err());
        std::fs::write(&doc, original).unwrap();
        assert!(read_project(&root).is_ok(), "restored");
    }
}

const SPICE: &str = "title\n* c\nV1 in 0 DC 5 AC 1\nR1 in out 1k tc=1\nC1 out 0 1u ; load\nD1 out 0 D1\n.model D1 D(IS=1e-9\n+ N=1.5)\nQ1 a b c m\nX1 a b s\n.subckt s a b\nR9 a b 1\n.ends\n.tran 1u 1m\n.end\n";

#[test]
fn damaged_foreign_files_are_reported_never_crash_and_always_account_for_every_record() {
    let reg = Registry::builtin();
    let kicad = include_str!("../src/import/kicad_netlist.rs");
    let net = kicad
        .split("pub(crate) const NET: &str = r#\"")
        .nth(1)
        .and_then(|s| s.split("\"#;").next())
        .unwrap()
        .to_owned();
    let n = iterations(400);
    for (name, seed_text, seed) in [("x.cir", SPICE.to_owned(), 1u64), ("x.net", net.clone(), 2)] {
        for bad in mutations(seed_text.as_bytes(), seed, n) {
            // Any bytes: either a clean error, or a report that accounts for everything it read.
            match reg.import(&bad, name, None) {
                Ok((o, _)) => {
                    if o.design.is_some() {
                        assert_eq!(
                            o.report.accounted(),
                            o.report.records_seen,
                            "{}",
                            o.report.text()
                        );
                    }
                }
                Err(e) => assert!(!e.to_string().is_empty()),
            }
            let forced = if name.ends_with(".cir") {
                "spice"
            } else {
                "kicad-netlist"
            };
            if let Ok((o, _)) = reg.import(&bad, name, Some(forced)) {
                assert!(
                    o.report.errors.iter().all(|e| !e.starts_with("internal:")),
                    "forgotten records: {:?}",
                    o.report.errors
                );
            }
        }
    }
    for bad in mutations(net.as_bytes(), 3, n) {
        let _ = kicad_netlist::parse_sx(&as_text(&bad));
    }
    // Pathological shapes.
    let deep = format!("{}{}", "(a ".repeat(100_000), ")".repeat(100_000));
    assert!(kicad_netlist::parse_sx(&deep).is_err());
    let long = format!("t\nR1 {} b 1\n", "n".repeat(1_000_000));
    assert!(reg.import(long.as_bytes(), "l.cir", Some("spice")).is_ok());
}
