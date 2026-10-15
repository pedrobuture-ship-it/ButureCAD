//! The Library Manager through the workspace: components, search, favorites and recents.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use buturecad_desktop_lib::ui_types::Mode;
use buturecad_desktop_lib::workspace::Workspace;

fn rig() -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None).with_user_libraries(dir.path().join("user-libs"));
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_mode(Mode::LibraryManager);
    (ws, dir)
}

fn info(ws: &mut Workspace) -> buturecad_desktop_lib::ui_types::LibraryManagerInfo {
    ws.snapshot().library_manager.unwrap()
}

const NE555: &str = r#"{"scope":"user","library":"Parts","name":"NE555DR","description":"555 timer","symbol":"Buture:R","footprint":"Package_SO:SOIC-8","model3d":"","manufacturer":"Texas Instruments","mpn":"NE555DR","datasheet":"https://example.com/ne555.pdf","parameters":{"Vcc":"4.5-16 V"},"tags":["timer"]}"#;

#[test]
fn components_are_stored_indexed_searched_and_survive_restart() {
    let (mut ws, dir) = rig();
    let i = info(&mut ws);
    assert!(i.total > 0, "built-in symbols are indexed");
    ws.library_command("component_save", Some(NE555)).unwrap();
    ws.library_command("query", Some("555 texas")).unwrap();
    let i = info(&mut ws);
    // The built-in Std library has an NE555DR too; the user's own is the one we saved.
    let user: Vec<_> = i.results.iter().filter(|r| r.scope == "user").collect();
    assert_eq!(user.len(), 1, "{:?}", i.results);
    let r = user[0];
    assert_eq!(
        (r.kind.as_str(), r.name.as_str(), r.scope.as_str()),
        ("component", "NE555DR", "user")
    );
    // Saved component is selected; its footprint does not resolve (no such library) and says so.
    let d = i.detail.unwrap();
    assert_eq!(d.parameters[0].key, "Vcc");
    assert!(
        d.unresolved.iter().any(|u| u.contains("Package_SO:SOIC-8")),
        "{:?}",
        d.unresolved
    );
    assert!(d.placeable);
    // Filters.
    ws.library_command("query", Some("")).unwrap();
    ws.library_command("kind", Some("component:1")).unwrap();
    let comps = info(&mut ws).results;
    assert!(comps.iter().all(|r| r.kind == "component"));
    assert_eq!(comps.iter().filter(|r| r.scope == "user").count(), 1);
    ws.library_command("kind", Some("component:0")).unwrap();
    ws.library_command("manufacturer", Some("Texas Instruments"))
        .unwrap();
    // The user's NE555DR and the built-in Std LM358DR / NE555DR.
    let ti = info(&mut ws).results;
    assert!(ti.iter().all(|r| r.manufacturer == "Texas Instruments"));
    assert_eq!(ti.iter().filter(|r| r.scope == "user").count(), 1);
    ws.library_command("manufacturer", Some("")).unwrap();
    // Favorites and recents are saved in the user folder and restored by a new session.
    let key = "component:Parts:NE555DR";
    ws.library_command("favorite", Some(key)).unwrap();
    ws.library_command("select", Some("symbol:Buture:R"))
        .unwrap();
    ws.library_command("scope", Some("favorites")).unwrap();
    let favs = info(&mut ws).results;
    assert_eq!(favs.len(), 1);
    assert!(favs[0].favorite);
    let mut ws2 = Workspace::new(None).with_user_libraries(dir.path().join("user-libs"));
    ws2.new_project(&dir.path().join("Q"), "Q").unwrap();
    ws2.set_mode(Mode::LibraryManager);
    ws2.library_command("scope", Some("recents")).unwrap();
    let rec = info(&mut ws2).results;
    assert_eq!(rec[0].key, "symbol:Buture:R", "most recent first");
    assert_eq!(rec.len(), 1);
    ws2.library_command("scope", Some("favorites")).unwrap();
    assert_eq!(info(&mut ws2).results[0].key, key);
    // Delete.
    ws2.library_command("component_delete", Some("user|Parts|NE555DR"))
        .unwrap();
    ws2.library_command("scope", Some("all")).unwrap();
    ws2.library_command("query", Some("555")).unwrap();
    // Only the built-in Std NE555 is left.
    assert!(info(&mut ws2).results.iter().all(|r| r.scope == "builtin"));
}

#[test]
fn bad_input_is_rejected_and_builtin_is_read_only_and_broken_files_are_reported() {
    let (mut ws, dir) = rig();
    assert!(ws
        .library_command(
            "component_save",
            Some(&NE555.replace("Package_SO:SOIC-8", "NoColon"))
        )
        .is_err());
    assert!(ws
        .library_command(
            "component_save",
            Some(&NE555.replace("\"Parts\"", "\"Buture\""))
        )
        .is_err());
    assert!(ws.library_command("component_save", Some("{nope")).is_err());
    assert!(ws.library_command("scope", Some("bogus")).is_err());
    assert!(ws.library_command("select", Some("symbol:Nope:X")).is_err());
    std::fs::create_dir_all(dir.path().join("user-libs")).unwrap();
    std::fs::write(
        dir.path().join("user-libs/Broken.bcomplib.json"),
        b"{not json",
    )
    .unwrap();
    ws.library_command("refresh", None).unwrap();
    let i = info(&mut ws);
    assert!(
        i.problems.iter().any(|p| p.contains("Broken")),
        "{:?}",
        i.problems
    );
    // Models.
    ws.library_command(
        "model_save",
        Some(r#"{"scope":"user","library":"M","name":"SOIC-8","path":"m/soic8.step"}"#),
    )
    .unwrap();
    ws.library_command("query", Some("soic")).unwrap();
    assert!(info(&mut ws).results.iter().any(|r| r.kind == "model"));
    // Placing: a built-in symbol resolves; a model cannot be placed.
    assert_eq!(
        ws.library_place_symbol("symbol:Buture:R").unwrap(),
        "Buture:R"
    );
    assert!(ws.library_place_symbol("model:M:SOIC-8").is_err());
}

/// Stage 51: placing a library component brings its footprint and manufacturer data, not only
/// its symbol; placing the plain symbol afterwards does not inherit them.
#[test]
fn placing_a_library_component_brings_footprint_and_manufacturer_data() {
    use buturecad_desktop_lib::ui_types::{PointerEvent, PointerKind};
    use buturecad_geometry::Point;
    use buturecad_schematic::Object;
    let (mut ws, _dir) = rig();
    ws.library_command("component_save", Some(NE555)).unwrap();
    let sym = ws.library_place_symbol("component:Parts:NE555DR").unwrap();
    assert_eq!(sym, "Buture:R");
    ws.set_mode(Mode::Schematic);
    ws.set_canvas_size(800.0, 600.0);
    ws.render(800, 600).unwrap(); // creates the schematic view
    ws.editor_command(&format!("tool:place_component:{sym}"))
        .unwrap();
    let ev = |kind, x, y| PointerEvent {
        kind,
        x,
        y,
        button: 0,
        shift: false,
        ctrl: false,
        alt: false,
        delta: 0.0,
    };
    let place = |ws: &mut Workspace, gx: i64| {
        let (x, y) = ws.views[&Mode::Schematic].world_to_screen(Point::new(gx * 1_270_000, 0));
        ws.pointer(ev(PointerKind::Move, x, y));
        ws.pointer(ev(PointerKind::Down, x, y));
        ws.pointer(ev(PointerKind::Up, x, y));
    };
    place(&mut ws, 0);
    let parts = |ws: &Workspace| -> Vec<buturecad_schematic::Component> {
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .schematic()
            .sheets()
            .values()
            .flat_map(|s| s.objects.values())
            .filter_map(|o| match o {
                Object::Component(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    };
    let first = parts(&ws);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].footprint, "Package_SO:SOIC-8");
    assert_eq!(first[0].properties["MPN"], "NE555DR");
    assert_eq!(first[0].properties["Manufacturer"], "Texas Instruments");
    assert_eq!(
        first[0].properties["Datasheet"],
        "https://example.com/ne555.pdf"
    );

    // The plain symbol (a different tool activation) carries none of it.
    ws.editor_command("tool:select").unwrap();
    ws.library_place_symbol("symbol:Buture:R").unwrap();
    ws.editor_command(&format!("tool:place_component:{sym}"))
        .unwrap();
    place(&mut ws, 20);
    let all = parts(&ws);
    assert_eq!(all.len(), 2);
    let second = all
        .iter()
        .find(|c| c.reference != first[0].reference)
        .unwrap();
    assert!(
        second.footprint.is_empty() && !second.properties.contains_key("MPN"),
        "{second:?}"
    );
}

/// Stage 51: the built-in Std library is searchable, placeable, and its footprint reaches the board.
#[test]
fn the_standard_library_is_searchable_and_its_footprint_reaches_the_board() {
    use buturecad_desktop_lib::ui_types::{PointerEvent, PointerKind};
    use buturecad_geometry::Point;
    let (mut ws, _dir) = rig();
    ws.library_command("query", Some("lm358")).unwrap();
    let results = info(&mut ws).results;
    let comp = results
        .iter()
        .find(|r| r.kind == "component" && r.name == "LM358DR")
        .expect("LM358DR is indexed");
    assert_eq!(
        (comp.scope.as_str(), comp.library.as_str()),
        ("builtin", "Std")
    );
    ws.library_command("query", Some("opamp dual")).unwrap();
    assert!(info(&mut ws)
        .results
        .iter()
        .any(|r| r.kind == "symbol" && r.name == "OpAmp_Dual_8"));
    ws.library_command("query", Some("soic-8")).unwrap();
    assert!(
        info(&mut ws).results.iter().any(|r| r.kind == "footprint"),
        "Std footprints are indexed"
    );

    let sym = ws.library_place_symbol("component:Std:LM358DR").unwrap();
    assert_eq!(sym, "Std:OpAmp_Dual_8");
    ws.set_mode(Mode::Schematic);
    ws.set_canvas_size(800.0, 600.0);
    ws.render(800, 600).unwrap();
    ws.editor_command(&format!("tool:place_component:{sym}"))
        .unwrap();
    let (x, y) = ws.views[&Mode::Schematic].world_to_screen(Point::new(0, 0));
    for kind in [PointerKind::Move, PointerKind::Down, PointerKind::Up] {
        ws.pointer(PointerEvent {
            kind,
            x,
            y,
            button: 0,
            shift: false,
            ctrl: false,
            alt: false,
            delta: 0.0,
        });
    }
    let preview = ws.pcb_update_preview().unwrap();
    assert!(
        preview.can_apply,
        "{:?}",
        preview.lines.iter().map(|l| &l.text).collect::<Vec<_>>()
    );
    ws.pcb_update_apply().unwrap();
    let doc = ws.open.as_ref().unwrap().engine.project().pcb();
    let f = doc
        .placed
        .values()
        .next()
        .expect("the footprint was placed on the board");
    assert_eq!(
        doc.footprints[&f.footprint].pads.len(),
        8,
        "SOIC-8 from the Std library"
    );
}
