//! The Library Manager through the workspace: components, search, favorites and recents.
#![allow(clippy::unwrap_used, clippy::panic)]

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
    assert_eq!(i.results.len(), 1, "{:?}", i.results);
    let r = &i.results[0];
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
    assert_eq!(info(&mut ws).results.len(), 1);
    ws.library_command("kind", Some("component:0")).unwrap();
    ws.library_command("manufacturer", Some("Texas Instruments"))
        .unwrap();
    assert_eq!(info(&mut ws).results.len(), 1);
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
    assert!(info(&mut ws2).results.is_empty());
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
