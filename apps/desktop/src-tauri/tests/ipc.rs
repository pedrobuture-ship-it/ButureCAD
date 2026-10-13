//! Drives the real Tauri commands through the mock runtime: argument decoding, error mapping and
//! the binary frame reply are exercised exactly as the webview would use them.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::commands::AppState;
use buturecad_desktop_lib::workspace::Workspace;
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

pub struct Harness {
    _app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
}

pub fn harness() -> Harness {
    let app = buturecad_desktop_lib::with_commands(mock_builder())
        .manage(AppState(Mutex::new(Workspace::new(None))))
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    Harness { _app: app, window }
}

impl Harness {
    pub fn invoke(&self, cmd: &str, body: Value) -> Result<InvokeResponseBody, Value> {
        tauri::test::get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
    }

    pub fn json(&self, cmd: &str, body: Value) -> Value {
        match self.invoke(cmd, body) {
            Ok(InvokeResponseBody::Json(s)) => serde_json::from_str(&s).unwrap(),
            Ok(_) => panic!("{cmd}: expected a JSON reply"),
            Err(e) => panic!("{cmd} failed: {e}"),
        }
    }
}

fn make_project(dir: &std::path::Path) {
    use buturecad_eda_core::commands::*;
    use buturecad_eda_core::{CommandEngine, ROOT_SHEET};
    use buturecad_geometry::{Point, Segment};
    use buturecad_io::{engine, ProjectSession};
    let (session, project) = ProjectSession::create(dir, "Ipc").unwrap();
    let mut e = CommandEngine::new(project);
    let id = e.new_id();
    e.execute(create_wire(
        ROOT_SHEET,
        id,
        Segment::new(Point::new(0, 0), Point::new(10_000_000, 0)),
    ))
    .unwrap();
    engine::save(&session, &mut e).unwrap();
}

const POINTER_MOVE: fn(f64, f64) -> Value = |x, y| json!({ "event": { "kind": "move", "x": x, "y": y, "button": 0, "shift": false, "ctrl": false, "alt": false, "delta": 0.0 } });

#[test]
fn core_info_and_snapshot_are_typed_json() {
    let h = harness();
    let info = h.json("core_info", json!({}));
    assert_eq!(info["name"], "ButureCAD");
    let snap = h.json("get_snapshot", json!({}));
    assert_eq!(snap["mode"], "schematic");
    assert_eq!(snap["tool"], "select");
    assert!(snap["project"].is_null());
}

#[test]
fn project_commands_roundtrip_through_ipc() {
    let dir = tempfile::tempdir().unwrap();
    make_project(&dir.path().join("p"));
    let h = harness();
    let snap = h.json(
        "project_open",
        json!({ "path": dir.path().join("p").to_string_lossy() }),
    );
    assert_eq!(snap["project"]["name"], "Ipc");
    assert_eq!(snap["object_count"], 1);

    let snap = h.json("set_mode", json!({ "mode": "pcb" }));
    assert_eq!(snap["mode"], "pcb");
    let snap = h.json(
        "set_grid",
        json!({ "spacingNm": 2_540_000, "visible": null, "snapGrid": false, "snapObjects": null }),
    );
    assert_eq!(
        (
            snap["grid"]["spacing_nm"].as_i64(),
            snap["grid"]["snap_grid"].as_bool()
        ),
        (Some(2_540_000), Some(false))
    );
    let snap = h.json("set_theme", json!({ "theme": "light" }));
    assert_eq!(snap["theme"], "light");
}

#[test]
fn errors_come_back_as_strings_and_invalid_arguments_are_rejected() {
    let h = harness();
    let err = h.invoke("project_save", json!({})).unwrap_err();
    assert_eq!(err, json!("no project is open"));
    let err = h
        .invoke(
            "set_grid",
            json!({ "spacingNm": 5, "visible": null, "snapGrid": null, "snapObjects": null }),
        )
        .unwrap_err();
    assert!(err.as_str().unwrap().contains("grid spacing"));
    assert!(h.invoke("set_mode", json!({ "mode": "bogus" })).is_err());
    assert!(h
        .invoke("editor_command", json!({ "name": "nope" }))
        .is_err());
    assert!(h
        .invoke("render_frame", json!({ "width": 0, "height": 10 }))
        .is_err());
    // The failed calls were logged for the Messages panel.
    let snap = h.json("get_snapshot", json!({}));
    assert!(snap["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["level"] == "error"));
}

#[test]
fn pointer_events_and_binary_frames() {
    let dir = tempfile::tempdir().unwrap();
    make_project(&dir.path().join("p"));
    let h = harness();
    h.json(
        "project_open",
        json!({ "path": dir.path().join("p").to_string_lossy() }),
    );

    let res = h.json("pointer", POINTER_MOVE(50.0, 50.0));
    assert!(res["redraw"].as_bool().unwrap());
    assert!(res["zoom_percent"].as_f64().unwrap() > 0.0);

    match h.invoke("render_frame", json!({ "width": 320, "height": 200 })) {
        Ok(InvokeResponseBody::Raw(bytes)) => {
            assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 320);
            assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 200);
            assert_eq!(bytes.len(), 8 + 320 * 200 * 4);
        }
        Err(e)
            if e.as_str()
                .is_some_and(|s| s.contains("GPU") || s.contains("adapter")) =>
        {
            eprintln!("SKIPPED: no GPU ({e})")
        }
        other => panic!("unexpected reply: {other:?}"),
    }
}

#[test]
fn editing_commands_work_over_ipc() {
    let dir = tempfile::tempdir().unwrap();
    make_project(&dir.path().join("p"));
    let h = harness();
    h.json(
        "project_open",
        json!({ "path": dir.path().join("p").to_string_lossy() }),
    );

    let lib = h.json("library_symbols", json!({}));
    assert!(lib
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["lib_id"] == "Buture:R" && s["is_power"] == false));
    assert!(lib
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["lib_id"] == "Buture:GND" && s["is_power"] == true));

    let snap = h.json(
        "editor_command",
        json!({ "name": "tool:place_component:Buture:R" }),
    );
    assert_eq!(
        (snap["tool"].as_str(), snap["tool_arg"].as_str()),
        (Some("place_component"), Some("Buture:R"))
    );
    assert!(
        h.invoke("editor_command", json!({ "name": "tool:place_component" }))
            .is_err(),
        "a symbol is required"
    );
    let snap = h.json("editor_command", json!({ "name": "cancel" }));
    assert_eq!(snap["tool"], "select");

    // Selecting and editing a property through the inspector API.
    h.json("editor_command", json!({ "name": "select_all" }));
    let snap = h.json("get_snapshot", json!({}));
    let wire = &snap["selection"][0];
    assert_eq!(wire["kind"], "wire");
    let err = h
        .invoke(
            "property_set",
            json!({ "id": wire["id"], "key": "value", "value": "x" }),
        )
        .unwrap_err();
    assert!(err.as_str().unwrap().contains("cannot be edited"));
}
