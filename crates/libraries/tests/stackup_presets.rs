//! Layer stack presets as local files.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_libraries::{LibraryDir, StackupPreset};
use buturecad_pcb::PhysicalStack;

#[test]
fn presets_round_trip_list_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let lib = LibraryDir::new(dir.path().join("presets"));
    assert!(
        lib.list_stackup_presets().is_empty(),
        "a missing folder is an empty list"
    );
    let stack = PhysicalStack::template("HDI 1+4+1 (micro-via build-up)").unwrap();
    let preset = StackupPreset::new("My HDI", 6, stack).unwrap();
    lib.save_stackup_preset(&preset).unwrap();
    lib.save_stackup_preset(
        &StackupPreset::new("A 4-layer", 4, PhysicalStack::standard(4)).unwrap(),
    )
    .unwrap();
    let names: Vec<_> = lib
        .list_stackup_presets()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["A 4-layer", "My HDI"]);
    assert_eq!(lib.load_stackup_preset("My HDI").unwrap(), preset);
    // Deterministic bytes: saving twice gives the same file.
    let p = dir.path().join("presets/My HDI.bstack.json");
    let first = std::fs::read(&p).unwrap();
    lib.save_stackup_preset(&preset).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), first);
    lib.delete_stackup_preset("My HDI").unwrap();
    assert!(lib.load_stackup_preset("My HDI").is_err());
    assert!(lib.delete_stackup_preset("My HDI").is_err());
}

#[test]
fn bad_names_and_damaged_files_are_refused_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let lib = LibraryDir::new(dir.path());
    let preset = StackupPreset::new("ok", 2, PhysicalStack::standard(2)).unwrap();
    for bad in ["", "../x", "a/b", " lead"] {
        let mut p = preset.clone();
        p.name = bad.into();
        assert!(lib.save_stackup_preset(&p).is_err(), "{bad:?}");
    }
    // A stack that does not match its layer count cannot even be built.
    assert!(StackupPreset::new("x", 4, PhysicalStack::standard(2)).is_err());
    // A damaged file is listed with its error and cannot be loaded.
    lib.save_stackup_preset(&preset).unwrap();
    let path = dir.path().join("ok.bstack.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("\"thickness\": 1510000", "\"thickness\": 0"),
    )
    .unwrap();
    let listed = lib.list_stackup_presets();
    assert!(listed[0].error.is_some());
    assert!(lib.load_stackup_preset("ok").is_err());
    std::fs::write(dir.path().join("junk.bstack.json"), b"{not json").unwrap();
    assert_eq!(lib.list_stackup_presets().len(), 2);
}
