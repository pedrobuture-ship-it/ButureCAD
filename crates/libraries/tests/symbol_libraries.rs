#![allow(clippy::unwrap_used, clippy::panic)]
use buturecad_geometry::Point;
use buturecad_libraries::{qualify, split_lib_id, LibraryDir, LibraryError, SymbolLibrary};
use buturecad_schematic::{Direction, Pin, PinType, Symbol};

fn opamp() -> Symbol {
    let mut s = Symbol::new("LM358", "U");
    s.unit_count = 3;
    s.meta.description = "Dual op-amp".into();
    s.meta.footprint_filters = vec!["SOIC-8*".into(), "DIP-8*".into()];
    for (n, name, unit, kind, y) in [
        ("1", "OUT", 1, PinType::Output, 0),
        ("2", "-", 1, PinType::Input, -1_270_000),
        ("3", "+", 1, PinType::Input, 1_270_000),
        ("8", "V+", 3, PinType::PowerInput, 0),
        ("4", "V-", 3, PinType::PowerInput, -2_540_000),
    ] {
        s.pins.push(Pin {
            number: n.into(),
            name: name.into(),
            kind,
            position: Point::new(0, y),
            orientation: Direction::Right,
            length: 2_540_000,
            unit,
            representation: 0,
        });
    }
    s
}

#[test]
fn symbols_round_trip_through_a_library_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path().join("libs"));
    let id = store.save_symbol("Analog", opamp()).unwrap();
    assert_eq!(id, "Analog:LM358");

    let lib = store.load_symbol_library("Analog").unwrap();
    assert_eq!(lib.len(), 1);
    assert_eq!(lib.get("LM358"), Some(&opamp()));
    let inst = lib.instantiate("LM358").unwrap();
    assert_eq!(inst.name, "Analog:LM358");
    assert_eq!(inst.unit_count, 3);

    let listed = store.list_symbol_libraries();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].name.as_str(), listed[0].item_count),
        ("Analog", 1)
    );
    assert!(listed[0].error.is_none());
}

#[test]
fn saving_is_deterministic_and_replaces_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path());
    store.save_symbol("A", opamp()).unwrap();
    let first = std::fs::read(dir.path().join("A.bsymlib.json")).unwrap();
    store.save_symbol("A", opamp()).unwrap();
    assert_eq!(
        first,
        std::fs::read(dir.path().join("A.bsymlib.json")).unwrap()
    );

    let mut changed = opamp();
    changed.meta.description = "Changed".into();
    store.save_symbol("A", changed).unwrap();
    let lib = store.load_symbol_library("A").unwrap();
    assert_eq!(lib.len(), 1);
    assert_eq!(lib.get("LM358").unwrap().meta.description, "Changed");

    store.delete_symbol("A", "LM358").unwrap();
    assert!(store.load_symbol_library("A").unwrap().is_empty());
    assert!(matches!(
        store.delete_symbol("A", "LM358"),
        Err(LibraryError::NoSuchItem { .. })
    ));
}

#[test]
fn invalid_symbols_and_names_are_rejected_before_touching_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path());
    let mut dup = opamp();
    dup.pins[1].number = "1".into();
    assert!(store.save_symbol("A", dup).is_err());
    assert!(
        store.list_symbol_libraries().is_empty(),
        "nothing was written"
    );
    for bad in ["", "..", "a/b", "a\\b", " x", "x "] {
        assert!(
            matches!(
                store.create_symbol_library(bad),
                Err(LibraryError::BadLibraryName(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn damaged_and_foreign_files_are_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryDir::new(dir.path());
    store.save_symbol("Good", opamp()).unwrap();
    std::fs::write(dir.path().join("Bad.bsymlib.json"), b"{ not json").unwrap();
    std::fs::write(
        dir.path().join("Foreign.bsymlib.json"),
        br#"{"format":"something-else","schema":1,"name":"Foreign","symbols":[]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Future.bsymlib.json"),
        br#"{"format":"buturecad-symbol-library","schema":99,"name":"Future","symbols":[]}"#,
    )
    .unwrap();
    let listed = store.list_symbol_libraries();
    assert_eq!(listed.len(), 4);
    let by = |n: &str| listed.iter().find(|l| l.name == n).unwrap();
    assert!(by("Good").error.is_none());
    for n in ["Bad", "Foreign", "Future"] {
        assert!(by(n).error.is_some(), "{n}");
        assert!(store.load_symbol_library(n).is_err());
    }
    assert!(matches!(
        store.load_symbol_library("Missing"),
        Err(LibraryError::NoSuchLibrary(_))
    ));
}

#[test]
fn qualified_ids() {
    assert_eq!(qualify("L", "N"), "L:N");
    assert_eq!(split_lib_id("L:N:x"), Some(("L", "N:x")));
    assert_eq!(split_lib_id("nolib"), None);
    assert_eq!(split_lib_id(":x"), None);
    let mut lib = SymbolLibrary::new("L").unwrap();
    let mut s = opamp();
    s.name = "L:LM358".into();
    lib.insert(s).unwrap();
    assert!(lib.get("LM358").is_some(), "stored unqualified");
}
