//! Plugin Manager and the component-provider flow through the workspace.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::{Mode, PluginManagerInfo};
use buturecad_desktop_lib::workspace::Workspace;
use std::path::Path;

fn provider_wasm() -> Vec<u8> {
    let search = r#"{"result":[{"id":"p1","name":"NE555","description":"timer","manufacturer":"TI","mpn":"NE555DR"}]}"#;
    let comp = r#"{"result":{"name":"NE555","description":"timer","symbol":"Buture:R","footprint":"Package_SO:SOIC-8","manufacturer":"TI","mpn":"NE555DR","parameters":{"Vcc":"5V"}}}"#;
    let esc = |s: &str| s.replace('"', "\\\"");
    wat::parse_str(format!(
        r#"(module
  (memory (export "memory") 2)
  (global $heap (mut i32) (i32.const 1024))
  (data (i32.const 8192) "{}")
  (data (i32.const 16384) "{}")
  (func $r (param $a i32) (param $l i32) (result i64)
    (i64.or (i64.shl (i64.extend_i32_u (local.get $a)) (i64.const 32)) (i64.extend_i32_u (local.get $l))))
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32) (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n))) (local.get $p))
  (func (export "call") (param $p i32) (param $n i32) (result i64)
    (if (i32.eq (i32.load8_u (i32.add (local.get $p) (i32.const 11))) (i32.const 115))
      (then (return (call $r (i32.const 8192) (i32.const {})))))
    (call $r (i32.const 16384) (i32.const {}))))"#,
        esc(search),
        esc(comp),
        search.len(),
        comp.len()
    ))
    .unwrap()
}

fn write_plugin(dir: &Path, id: &str, perms: &str, wasm: &[u8]) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"format":"buturecad-plugin","api":1,"id":"{id}","name":"Fixture parts","version":"1.0","description":"test","categories":["ComponentProvider"],"permissions":[{perms}],"wasm":"plugin.wasm"}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("plugin.wasm"), wasm).unwrap();
    dir.join("plugin.json")
}

fn rig() -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None)
        .with_user_libraries(dir.path().join("user-libs"))
        .with_plugin_dir(dir.path().join("plugins"));
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_mode(Mode::Plugins);
    (ws, dir)
}

fn info(ws: &mut Workspace) -> PluginManagerInfo {
    ws.snapshot().plugins.unwrap()
}

#[test]
fn install_enable_permissions_and_remove() {
    let (mut ws, dir) = rig();
    let src = write_plugin(
        &dir.path().join("src"),
        "fixture.parts",
        r#""network","project-read""#,
        &provider_wasm(),
    );
    ws.plugin_command("install", Some(src.to_str().unwrap()))
        .unwrap();
    let p = &info(&mut ws).plugins[0];
    assert!(
        !p.enabled && p.granted.is_empty(),
        "installed disabled, nothing granted"
    );
    assert_eq!(p.requested, ["network", "project-read"]);
    assert!(
        ws.plugin_command("grant", Some("fixture.parts|filesystem"))
            .is_err(),
        "not requested"
    );
    ws.plugin_command("enable", Some("fixture.parts")).unwrap();
    ws.plugin_command("grant", Some("fixture.parts|network"))
        .unwrap();
    assert_eq!(info(&mut ws).plugins[0].granted, ["network"]);
    ws.plugin_command("revoke", Some("fixture.parts|network"))
        .unwrap();
    assert!(info(&mut ws).plugins[0].granted.is_empty());
    assert_eq!(info(&mut ws).providers, ["fixture.parts"]);
    ws.plugin_command("reload", Some("fixture.parts")).unwrap();
    ws.plugin_command("disable", Some("fixture.parts")).unwrap();
    assert!(info(&mut ws).providers.is_empty());
    // A module that is not WebAssembly is refused at install.
    let bad = write_plugin(&dir.path().join("bad"), "fixture.bad", "", b"nope");
    assert!(ws
        .plugin_command("install", Some(bad.to_str().unwrap()))
        .is_err());
    ws.plugin_command("remove", Some("fixture.parts")).unwrap();
    assert!(info(&mut ws).plugins.is_empty());
}

#[test]
fn a_provider_plugin_feeds_the_local_component_library() {
    let (mut ws, dir) = rig();
    let src = write_plugin(
        &dir.path().join("src"),
        "fixture.parts",
        "",
        &provider_wasm(),
    );
    ws.plugin_command("install", Some(src.to_str().unwrap()))
        .unwrap();
    ws.plugin_command("enable", Some("fixture.parts")).unwrap();
    ws.plugin_command("provider", Some("fixture.parts"))
        .unwrap();
    ws.plugin_command("search", Some("555")).unwrap();
    let i = info(&mut ws);
    assert_eq!(i.hits.len(), 1, "{:?}", i.error);
    assert_eq!(i.hits[0].mpn, "NE555DR");
    // Import into a user component library; the (invalid) symbol/footprint payloads are noted, not stored.
    ws.plugin_command("import", Some("p1|Supplier")).unwrap();
    assert!(info(&mut ws).last_import.contains("Imported “NE555”"));
    ws.set_mode(Mode::LibraryManager);
    ws.library_command("query", Some("ne555")).unwrap();
    let lm = ws.snapshot().library_manager.unwrap();
    let r = lm.results.iter().find(|r| r.kind == "component").unwrap();
    assert_eq!(
        (r.library.as_str(), r.mpn.as_str()),
        ("Supplier", "NE555DR")
    );
    assert!(ws.plugin_command("import", Some("p1|Buture")).is_err());
}

#[test]
fn without_a_plugin_folder_the_manager_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_mode(Mode::Plugins);
    assert!(!info(&mut ws).available);
    assert!(ws.plugin_command("enable", Some("x")).is_err());
}
