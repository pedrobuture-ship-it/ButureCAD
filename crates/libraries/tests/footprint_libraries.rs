#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_geometry::Point;
use buturecad_libraries::{LibraryDir, LibraryError};
use buturecad_pcb::generators::{Dip, Soic};
use buturecad_pcb::{Footprint, Pad};

#[test]
fn footprints_round_trip_through_a_library_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path().join("libs"));
    let fp = Soic::narrow(8).generate().unwrap();
    let id = store.save_footprint("Package_SO", fp.clone()).unwrap();
    assert_eq!(id, "Package_SO:SOIC-8_3.9mm_P1.27mm");
    store
        .save_footprint("Package_SO", Soic::wide(16).generate().unwrap())
        .unwrap();

    let lib = store.load_footprint_library("Package_SO").unwrap();
    assert_eq!(lib.len(), 2);
    assert_eq!(lib.get("SOIC-8_3.9mm_P1.27mm"), Some(&fp));
    let inst = lib.instantiate("SOIC-8_3.9mm_P1.27mm").unwrap();
    assert_eq!(inst.name, "Package_SO:SOIC-8_3.9mm_P1.27mm");
    assert_eq!(inst.pads, fp.pads);

    let listed = store.list_footprint_libraries();
    assert_eq!((listed.len(), listed[0].item_count), (1, 2));
    // Symbol and footprint libraries live side by side without seeing each other.
    assert!(store.list_symbol_libraries().is_empty());
}

#[test]
fn saving_is_deterministic_and_invalid_footprints_never_reach_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path());
    store
        .save_footprint("A", Dip::standard(8).generate().unwrap())
        .unwrap();
    let first = std::fs::read(dir.path().join("A.bfplib.json")).unwrap();
    store
        .save_footprint("A", Dip::standard(8).generate().unwrap())
        .unwrap();
    assert_eq!(
        first,
        std::fs::read(dir.path().join("A.bfplib.json")).unwrap()
    );

    let mut bad = Footprint::new("Bad");
    bad.pads.push(Pad::smd("1", Point::new(0, 0), 0, 100));
    assert!(store.save_footprint("B", bad).is_err());
    assert!(!dir.path().join("B.bfplib.json").exists());

    store.delete_footprint("A", "DIP-8_W7.62mm").unwrap();
    assert!(store.load_footprint_library("A").unwrap().is_empty());
    assert!(matches!(
        store.delete_footprint("A", "DIP-8_W7.62mm"),
        Err(LibraryError::NoSuchItem { .. })
    ));
}

#[test]
fn damaged_files_are_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path());
    std::fs::write(dir.path().join("Bad.bfplib.json"), b"nope").unwrap();
    std::fs::write(
        dir.path().join("Future.bfplib.json"),
        br#"{"format":"buturecad-footprint-library","schema":9,"name":"Future","footprints":[]}"#,
    )
    .unwrap();
    // A footprint whose pad has a zero size must be refused even inside an otherwise fine file.
    std::fs::write(
        dir.path().join("Evil.bfplib.json"),
        br#"{"format":"buturecad-footprint-library","schema":1,"name":"Evil","footprints":[{"name":"X","pads":[{"number":"1","kind":"smd","shape":"rect","position":{"x":0,"y":0},"size_x":0,"size_y":5}]}]}"#,
    )
    .unwrap();
    let listed = store.list_footprint_libraries();
    assert_eq!(listed.len(), 3);
    assert!(listed.iter().all(|l| l.error.is_some()));
    assert!(store.load_footprint_library("Evil").is_err());
}
