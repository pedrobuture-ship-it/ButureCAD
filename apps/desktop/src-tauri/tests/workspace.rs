#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{LayerId, Track, Via};
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{builtin, Component, Id, Placement};

const G: i64 = 1_270_000;

fn comp(reference: &str, lib: &str, x: i64, y: i64) -> Component {
    Component {
        lib_id: lib.into(),
        reference: reference.into(),
        value: "10k".into(),
        footprint: String::new(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x * G, y * G)),
    }
}

/// Two resistors joined by a wire, a label and a PCB track + via.
fn workspace_with_design(dir: &std::path::Path) -> (Workspace, Vec<Id>) {
    let mut ws = Workspace::new(Some(dir.join("recent.json")));
    ws.new_project(&dir.join("Board"), "Board").unwrap();
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let mut ids = Vec::new();
    for (r, x) in [("R1", 0), ("R2", 20)] {
        let id = e.new_id();
        e.execute(place_component(ROOT_SHEET, id, comp(r, "Buture:R", x, 0)))
            .unwrap();
        ids.push(id);
    }
    let w = e.new_id();
    e.execute(create_wire(
        ROOT_SHEET,
        w,
        Segment::new(Point::new(0, -3 * G), Point::new(20 * G, -3 * G)),
    ))
    .unwrap();
    ids.push(w);
    e.execute(CreateTrackCommand::new(Track {
        layer: LayerId(0),
        segment: Segment::new(Point::ORIGIN, Point::new(5 * G, 0)),
        width: 250_000,
        net: None,
    }))
    .unwrap();
    e.execute(CreateViaCommand::new(Via {
        position: Point::new(5 * G, 0),
        diameter: 600_000,
        drill: 300_000,
        net: None,
        kind: Default::default(),
    }))
    .unwrap();
    (ws, ids)
}

fn ev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        button: 0,
        shift: false,
        ctrl: false,
        alt: false,
        delta: 0.0,
    }
}

/// Screen position of a world point in the current schematic view.
fn screen(ws: &Workspace, x: i64, y: i64) -> (f64, f64) {
    ws.views[&Mode::Schematic].world_to_screen(Point::new(x, y))
}

fn selected(ws: &Workspace) -> Vec<u64> {
    ws.selection
        .get(&Mode::Schematic)
        .map(|s| s.iter().map(|i| i.0).collect())
        .unwrap_or_default()
}

fn prepared(ws: &mut Workspace) {
    ws.set_canvas_size(1000.0, 700.0);
    ws.fit_view();
}

#[test]
fn click_selects_hover_describes_and_snapshot_summarizes_with_nets() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, ids) = workspace_with_design(dir.path());
    prepared(&mut ws);

    let (sx, sy) = screen(&ws, 0, 0); // centre of R1's body
    let r = ws.pointer(ev(PointerKind::Move, sx, sy));
    assert_eq!(r.hover.as_deref(), Some("R1 (10k)"));
    ws.pointer(ev(PointerKind::Down, sx, sy));
    ws.pointer(ev(PointerKind::Up, sx, sy));
    assert_eq!(selected(&ws), vec![ids[0].0]);

    let snap = ws.snapshot();
    assert_eq!(snap.selection.len(), 1);
    let s = &snap.selection[0];
    assert_eq!((s.kind.as_str(), s.title.as_str()), ("component", "R1"));
    let prop = |k: &str| {
        s.properties
            .iter()
            .find(|p| p.key == k)
            .map(|p| p.value.clone())
    };
    assert_eq!(prop("reference").as_deref(), Some("R1"));
    // Real connectivity: pin 2 of R1 is on the wire shared with R2.
    assert_eq!(prop("pin_2").as_deref(), Some("Net-(R1-Pad2)"));
    assert_eq!(prop("pin_1").as_deref(), Some("Net-(R1-Pad1)"));
}

#[test]
fn clicking_empty_space_clears_and_shift_click_toggles() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, ids) = workspace_with_design(dir.path());
    prepared(&mut ws);
    let click = |ws: &mut Workspace, x: f64, y: f64, shift: bool| {
        let mut d = ev(PointerKind::Down, x, y);
        d.shift = shift;
        ws.pointer(d);
        ws.pointer(PointerEvent {
            kind: PointerKind::Up,
            ..d
        });
    };
    let (r1x, r1y) = screen(&ws, 0, 0);
    let (r2x, r2y) = screen(&ws, 20 * G, 0);
    click(&mut ws, r1x, r1y, false);
    click(&mut ws, r2x, r2y, true);
    assert_eq!(selected(&ws), vec![ids[0].0, ids[1].0]);
    click(&mut ws, r1x, r1y, true);
    assert_eq!(selected(&ws), vec![ids[1].0]);
    click(&mut ws, 5.0, 5.0, false);
    assert!(selected(&ws).is_empty());
}

#[test]
fn window_and_crossing_box_selection_pick_whole_objects() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, ids) = workspace_with_design(dir.path());
    prepared(&mut ws);
    let drag = |ws: &mut Workspace, a: (f64, f64), b: (f64, f64)| {
        ws.selection.clear();
        ws.pointer(ev(PointerKind::Down, a.0, a.1));
        ws.pointer(ev(PointerKind::Move, b.0, b.1));
        ws.pointer(ev(PointerKind::Up, b.0, b.1));
        selected(ws)
    };
    let (x0, y0) = screen(&ws, -4 * G, 6 * G); // top-left of the world area around R1
    let (x1, y1) = screen(&ws, 4 * G, -8 * G); // covers R1 fully, wire only partially
                                               // Left → right = window: R1 is fully inside; the wire (running to R2) is not.
    assert_eq!(drag(&mut ws, (x0, y0), (x1, y1)), vec![ids[0].0]);
    // Right → left = crossing: R1 and the wire touched by the box.
    assert_eq!(drag(&mut ws, (x1, y1), (x0, y0)), vec![ids[0].0, ids[2].0]);
    // A window that only covers part of R1 selects nothing.
    let (px, py) = screen(&ws, 2 * G, 4 * G);
    assert!(drag(&mut ws, (x0, y0), (px, py)).is_empty());
}

#[test]
fn wheel_zoom_keeps_the_point_under_the_cursor_and_middle_drag_pans() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    prepared(&mut ws);
    let (sx, sy) = (300.0, 200.0);
    let before = ws.views[&Mode::Schematic].screen_to_world_f(sx, sy);
    let mut w = ev(PointerKind::Wheel, sx, sy);
    w.delta = -200.0; // scroll up = zoom in
    let r = ws.pointer(w);
    assert!(r.redraw);
    let after = ws.views[&Mode::Schematic].screen_to_world_f(sx, sy);
    assert!((before.0 - after.0).abs() < 1.0 && (before.1 - after.1).abs() < 1.0);
    let zoom_in = r.zoom_percent;
    w.delta = 400.0;
    assert!(ws.pointer(w).zoom_percent < zoom_in);

    let anchor = screen(&ws, 0, 0);
    let mut d = ev(PointerKind::Down, 100.0, 100.0);
    d.button = 1;
    ws.pointer(d);
    ws.pointer(PointerEvent {
        kind: PointerKind::Move,
        x: 140.0,
        y: 90.0,
        ..d
    });
    ws.pointer(PointerEvent {
        kind: PointerKind::Up,
        x: 140.0,
        y: 90.0,
        ..d
    });
    let moved = screen(&ws, 0, 0);
    assert!((moved.0 - anchor.0 - 40.0).abs() < 1e-6 && (moved.1 - anchor.1 + 10.0).abs() < 1e-6);
}

#[test]
fn cursor_reports_snap_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    prepared(&mut ws);
    // Near R1's pin 1 connection point (0, 3 grid): object snap.
    let (sx, sy) = screen(&ws, 0, 3 * G);
    let r = ws.pointer(ev(PointerKind::Move, sx + 2.0, sy + 1.0));
    assert_eq!(
        (r.snap.as_str(), r.cursor_x_nm, r.cursor_y_nm),
        ("object", 0, 3 * G)
    );
    // Far from anything: grid.
    let (fx, fy) = screen(&ws, 60 * G, 30 * G);
    let r = ws.pointer(ev(PointerKind::Move, fx, fy));
    assert_eq!(r.snap, "grid");
    assert_eq!(r.cursor_x_nm % G, 0);
}

#[test]
fn pcb_mode_shows_tracks_and_vias_and_layer_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    ws.set_mode(Mode::Pcb);
    let snap = ws.snapshot();
    assert_eq!(snap.object_count, 2, "one track and one via");
    let names: Vec<_> = snap.layers.iter().map(|l| l.name.as_str()).collect();
    for want in ["F.Cu", "B.Cu", "Edge.Cuts", "F.Silkscreen", "User 1"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    assert_eq!(snap.pcb.as_ref().unwrap().copper_layers, 2);
    ws.set_pcb_layer_visible(0, false);
    let snap = ws.snapshot();
    let f_cu = |s: &Snapshot| s.layers.iter().find(|l| l.name == "F.Cu").unwrap().visible;
    assert!(!f_cu(&snap));
    // Hidden layers are not pickable.
    ws.set_canvas_size(800.0, 600.0);
    ws.fit_view();
    let v = ws.views[&Mode::Pcb];
    let (sx, sy) = v.world_to_screen(Point::new(2 * G, 0));
    assert!(ws.pointer(ev(PointerKind::Move, sx, sy)).hover.is_none());
    ws.set_pcb_layer_visible(0, true);
    assert!(ws
        .pointer(ev(PointerKind::Move, sx, sy))
        .hover
        .unwrap()
        .starts_with("Track F.Cu"));
}

#[test]
fn undo_redo_prune_selection_and_report_history() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, ids) = workspace_with_design(dir.path());
    prepared(&mut ws);
    ws.selection
        .insert(Mode::Schematic, [ids[2]].into_iter().collect());
    assert_eq!(ws.undo().unwrap().as_deref(), Some("Create via")); // PCB step: selection untouched
    assert_eq!(ws.undo().unwrap().as_deref(), Some("Create track"));
    assert_eq!(ws.undo().unwrap().as_deref(), Some("Create wire"));
    assert!(
        selected(&ws).is_empty(),
        "the selected wire no longer exists"
    );
    let snap = ws.snapshot();
    let p = snap.project.unwrap();
    assert!(p.can_redo && p.dirty);
    assert_eq!(p.redo_history[0], "Create wire");
    assert!(snap.command_log.iter().any(|l| l == "Undo: Create wire"));
    assert_eq!(ws.redo().unwrap().as_deref(), Some("Create wire"));
}

#[test]
fn project_lifecycle_save_open_recent_and_dirty_close() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    assert!(ws.snapshot().project.unwrap().dirty);
    assert!(
        ws.close_project().is_err(),
        "unsaved changes block a plain close"
    );
    ws.save().unwrap();
    assert!(!ws.snapshot().project.unwrap().dirty);
    ws.close_project().unwrap();
    assert!(ws.snapshot().project.is_none());

    ws.open_project(&dir.path().join("Board")).unwrap();
    assert_eq!(ws.snapshot().object_count, 3, "two components and a wire");
    let recent = ws.snapshot().recent;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].name, "Board");

    // The recent list survives a restart.
    let again = Workspace::new(Some(dir.path().join("recent.json")));
    assert_eq!(again.snapshot_recent_len(), 1);

    // Rename goes through the command engine: undoable and marks the project dirty.
    ws.rename_project("Renamed").unwrap();
    let p = ws.snapshot().project.unwrap();
    assert_eq!((p.name.as_str(), p.dirty), ("Renamed", true));
    ws.undo().unwrap();
    assert_eq!(ws.snapshot().project.unwrap().name, "Board");
    assert!(ws.rename_project("").is_err());
}

#[test]
fn crash_recovery_is_offered_and_can_be_accepted_or_declined() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    ws.save().unwrap();
    ws.rename_project("Unsaved").unwrap();
    assert!(ws.autosave_tick().unwrap());
    assert!(!ws.autosave_tick().unwrap(), "nothing new to autosave");
    std::mem::forget(ws); // simulate a crash: the session lock is never released

    let mut ws2 = Workspace::new(None);
    ws2.open_project(&dir.path().join("Board")).unwrap();
    let snap = ws2.snapshot();
    assert!(snap.recovery_pending);
    assert_eq!(snap.project.unwrap().name, "Board");
    ws2.resolve_recovery(true).unwrap();
    let snap = ws2.snapshot();
    assert!(!snap.recovery_pending);
    let p = snap.project.unwrap();
    assert_eq!((p.name.as_str(), p.dirty), ("Unsaved", true));
}

#[test]
fn declining_recovery_discards_the_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    ws.save().unwrap();
    ws.rename_project("Unsaved").unwrap();
    ws.autosave_tick().unwrap();
    std::mem::forget(ws);
    let mut ws2 = Workspace::new(None);
    ws2.open_project(&dir.path().join("Board")).unwrap();
    ws2.resolve_recovery(false).unwrap();
    assert!(!dir.path().join("Board/.buture/autosave").exists());
    assert_eq!(ws2.snapshot().project.unwrap().name, "Board");
}

#[test]
fn errors_are_reported_not_panics() {
    let mut ws = Workspace::new(None);
    assert!(ws.save().is_err());
    assert!(ws.undo().is_err());
    assert!(ws
        .open_project(std::path::Path::new("/definitely/not/a/project"))
        .is_err());
    assert!(ws.editor_command("bogus").is_err());
    // No project: pointer events and snapshots are harmless.
    let r = ws.pointer(ev(PointerKind::Move, 10.0, 10.0));
    assert!(!r.redraw || r.hover.is_none());
    assert!(ws.snapshot().project.is_none());
}

#[test]
fn modes_without_a_canvas_have_no_scene() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    for mode in [Mode::SymbolEditor, Mode::FootprintEditor, Mode::Viewer3d] {
        ws.set_mode(mode);
        let snap = ws.snapshot();
        assert_eq!((snap.object_count, snap.layers.len()), (0, 0));
        assert!(ws.render(100, 100).is_err());
    }
}

#[test]
fn rendering_produces_pixels_when_a_gpu_exists() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = workspace_with_design(dir.path());
    let Ok((w, h, rgba)) = ws.render(640, 480) else {
        eprintln!("SKIPPED: no GPU adapter ({:?})", ws.snapshot().gpu_error);
        return;
    };
    assert_eq!((w, h, rgba.len()), (640, 480, 640 * 480 * 4));
    let bg = [11, 14, 20];
    assert!(
        rgba.chunks(4).any(|p| p[..3] != bg),
        "the schematic must draw something"
    );
    assert!(ws.snapshot().gpu_adapter.is_some());
    ws.set_theme(ThemeName::Light);
    let (_, _, light) = ws.render(640, 480).unwrap();
    assert_ne!(rgba, light, "the theme changes the picture");
}
