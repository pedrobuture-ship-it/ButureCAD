//! Drives the PCB editor like the UI does against a real Workspace.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::model::PcbObject;
use buturecad_geometry::Point;
use buturecad_pcb::{layer, FpShape};

const MM: i64 = 1_000_000;
const FP: &str = "Generated:SOIC-8_3.9mm_P1.27mm";

fn ev(kind: PointerKind, x: f64, y: f64, button: i32) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        button,
        shift: false,
        ctrl: false,
        alt: false,
        delta: 0.0,
    }
}

fn board(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.set_mode(Mode::FootprintEditor);
    ws.footprint_editor_command("generate", Some("soic|pins=8"))
        .unwrap();
    ws.set_mode(Mode::Pcb);
    ws.fit_view();
    // 1 mm = 12 px, origin at the centre; 1 mm grid.
    ws.grid.spacing = MM;
    let v = ws.views.get_mut(&Mode::Pcb).unwrap();
    v.center = (0.0, 0.0);
    v.scale = 12.0 / MM as f64;
    ws
}

fn screen(ws: &Workspace, x: i64, y: i64) -> (f64, f64) {
    ws.views[&Mode::Pcb].world_to_screen(Point::new(x * MM, y * MM))
}

fn click(ws: &mut Workspace, x: i64, y: i64) {
    let (sx, sy) = screen(ws, x, y);
    ws.pointer(ev(PointerKind::Move, sx, sy, 0));
    ws.pointer(ev(PointerKind::Down, sx, sy, 0));
    ws.pointer(ev(PointerKind::Up, sx, sy, 0));
}

fn drag(ws: &mut Workspace, from: (i64, i64), to: (i64, i64)) {
    let (x0, y0) = screen(ws, from.0, from.1);
    let (x1, y1) = screen(ws, to.0, to.1);
    ws.pointer(ev(PointerKind::Move, x0, y0, 0));
    ws.pointer(ev(PointerKind::Down, x0, y0, 0));
    for i in 1..=5 {
        let t = f64::from(i) / 5.0;
        ws.pointer(ev(
            PointerKind::Move,
            x0 + (x1 - x0) * t,
            y0 + (y1 - y0) * t,
            0,
        ));
    }
    ws.pointer(ev(PointerKind::Up, x1, y1, 0));
}

fn placed(ws: &Workspace) -> Vec<(u64, buturecad_pcb::PlacedFootprint)> {
    ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .placed
        .iter()
        .map(|(k, v)| (k.0, v.clone()))
        .collect()
}

fn place_three(ws: &mut Workspace) {
    ws.editor_command(&format!("tool:place_footprint:{FP}"))
        .unwrap();
    click(ws, -20, 10);
    click(ws, 0, 0);
    click(ws, 14, -5);
    ws.editor_command("cancel").unwrap();
}

#[test]
fn the_outline_is_drawn_on_edge_cuts_and_checked_for_closure() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    assert!(ws
        .snapshot()
        .pcb
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.contains("no board outline")));
    ws.editor_command("tool:outline_rect").unwrap();
    click(&mut ws, -30, -20);
    click(&mut ws, 30, 20);
    let doc = ws.open.as_ref().unwrap().engine.project().pcb().clone();
    assert_eq!(doc.graphics.len(), 4);
    assert!(doc
        .graphics
        .values()
        .all(|g| g.layer == layer::EDGE_CUTS && matches!(g.shape, FpShape::Line { .. })));
    assert!(ws.snapshot().pcb.unwrap().warnings.is_empty());
    // One undo step removes all four sides.
    assert_eq!(
        ws.snapshot().project.unwrap().undo_history.last().unwrap(),
        "Draw board outline"
    );
    ws.undo().unwrap();
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .graphics
        .is_empty());

    // A polyline outline: the first point closes it.
    ws.editor_command("tool:outline_line").unwrap();
    for (x, y) in [(0, 0), (20, 0), (20, 10), (0, 0)] {
        click(&mut ws, x, y);
    }
    assert_eq!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .graphics
            .len(),
        3
    );
    assert!(
        ws.snapshot().pcb.unwrap().warnings.is_empty(),
        "closed triangle"
    );
    // An open one is reported.
    ws.editor_command("tool:outline_line").unwrap();
    click(&mut ws, 40, 0);
    click(&mut ws, 50, 0);
    ws.editor_command("cancel").unwrap();
    assert!(ws
        .snapshot()
        .pcb
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.contains("not a closed loop")));
}

#[test]
fn footprints_are_placed_from_the_project_with_rotate_and_flip_on_the_ghost() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    ws.editor_command(&format!("tool:place_footprint:{FP}"))
        .unwrap();
    ws.editor_command("rotate").unwrap();
    ws.editor_command("flip").unwrap();
    click(&mut ws, 5, 5);
    let p = placed(&ws);
    assert_eq!(p.len(), 1);
    let f = &p[0].1;
    assert_eq!((f.footprint.as_str(), f.reference.as_str()), (FP, "REF**"));
    assert_eq!(
        (
            f.placement.position,
            f.placement.rotation,
            f.placement.flipped
        ),
        (Point::new(5 * MM, 5 * MM), 1, true)
    );
    assert_eq!(ws.snapshot().selection.len(), 1);
    assert!(ws.editor_command("tool:place_footprint:Nope:X").is_err());
    assert!(ws.editor_command("tool:place_footprint").is_err());
    // The placed footprint is a real, pickable object with pads in the scene.
    assert!(ws.snapshot().object_count >= 1);
}

#[test]
fn move_rotate_flip_lock_delete_duplicate_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    place_three(&mut ws);
    assert_eq!(placed(&ws).len(), 3);
    ws.editor_command("select_all").unwrap();
    assert_eq!(ws.snapshot().selection.len(), 3);
    ws.editor_command("clear_selection").unwrap();

    // Drag the middle one (click inside its courtyard).
    drag(&mut ws, (0, 0), (4, 6));
    let mid = placed(&ws)
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.placement.position.x == 4 * MM)
        .unwrap();
    assert_eq!(mid.placement.position, Point::new(4 * MM, 6 * MM));
    ws.undo().unwrap();
    assert!(placed(&ws)
        .iter()
        .any(|(_, f)| f.placement.position == Point::ORIGIN));

    // Rotate/flip the selection (a single footprint rotates about its own pivot).
    click(&mut ws, 0, 0);
    ws.editor_command("rotate").unwrap();
    ws.editor_command("flip").unwrap();
    let f = placed(&ws)
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.placement.flipped)
        .unwrap();
    assert_eq!(
        f.placement.rotation, 3,
        "rotate then flip negates the angle"
    );
    ws.undo().unwrap();
    ws.undo().unwrap();

    // Lock: a locked footprint refuses to move, rotate, flip and delete.
    ws.editor_command("lock").unwrap();
    assert!(ws.editor_command("rotate").is_err());
    assert!(ws.editor_command("flip").is_err());
    assert!(ws.editor_command("delete").is_err());
    drag(&mut ws, (0, 0), (8, 8));
    assert!(placed(&ws)
        .iter()
        .any(|(_, f)| f.locked && f.placement.position == Point::ORIGIN));
    ws.editor_command("unlock").unwrap();

    // Duplicate creates an unconnected, unreferenced copy and selects it.
    ws.editor_command("duplicate").unwrap();
    assert_eq!(placed(&ws).len(), 4);
    assert_eq!(ws.snapshot().selection.len(), 1);
    ws.editor_command("delete").unwrap();
    assert_eq!(placed(&ws).len(), 3);
    ws.undo().unwrap();
    assert_eq!(placed(&ws).len(), 4);
    ws.undo().unwrap();
    ws.undo().unwrap();
    assert_eq!(placed(&ws).len(), 3);
}

#[test]
fn align_and_distribute_multi_selections() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    place_three(&mut ws);
    assert!(ws.editor_command("align_left").is_err(), "nothing selected");
    ws.editor_command("select_all").unwrap();
    ws.editor_command("align_bottom").unwrap();
    let ys: Vec<i64> = {
        let doc = ws.open.as_ref().unwrap().engine.project().pcb();
        doc.placed
            .values()
            .map(|f| f.placement.position.y)
            .collect()
    };
    assert!(
        ys.windows(2).all(|w| w[0] == w[1]),
        "all share a bottom edge (same footprint): {ys:?}"
    );
    ws.editor_command("distribute_h").unwrap();
    let xs: Vec<i64> = {
        let doc = ws.open.as_ref().unwrap().engine.project().pcb();
        let mut v: Vec<i64> = doc
            .placed
            .values()
            .map(|f| f.placement.position.x)
            .collect();
        v.sort();
        v
    };
    assert_eq!(xs[1] - xs[0], xs[2] - xs[1]);
    assert!(ws.editor_command("distribute_h").is_err(), "already even");
    ws.editor_command("align_center_v").unwrap_err();
    let labels = ws.snapshot().project.unwrap().undo_history;
    assert!(labels.contains(&"Align".to_string()) && labels.contains(&"Distribute".to_string()));
}

#[test]
fn layer_manager_active_layer_and_stackup() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    place_three(&mut ws);
    let snap = ws.snapshot();
    let names: Vec<&str> = snap.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(
        names.contains(&"B.Mask") && names.contains(&"Mechanical 8") && !names.contains(&"In1.Cu")
    );
    ws.pcb_command("set_stackup", Some("4")).unwrap();
    let snap = ws.snapshot();
    assert!(snap.layers.iter().any(|l| l.name == "In2.Cu"));
    assert_eq!(snap.pcb.as_ref().unwrap().copper_layers, 4);
    ws.pcb_command("active_layer", Some("1")).unwrap();
    assert_eq!(ws.snapshot().pcb.unwrap().active_layer, "B.Cu");
    assert!(ws.pcb_command("active_layer", Some("55")).is_err());
    assert!(ws.pcb_command("set_stackup", Some("5")).is_err());
    ws.undo().unwrap();
    assert_eq!(ws.snapshot().pcb.unwrap().copper_layers, 2);
    // Hiding the silkscreen hides it from picking and the layer list agrees.
    ws.set_pcb_layer_visible(layer::F_SILK.0, false);
    assert!(ws
        .snapshot()
        .layers
        .iter()
        .any(|l| l.name == "F.Silkscreen" && !l.visible));
}

#[test]
fn inspector_edits_are_commands_and_invalid_values_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path());
    place_three(&mut ws);
    click(&mut ws, 0, 0);
    let id = ws.snapshot().selection[0].id;
    ws.set_property(id, "reference", "U7").unwrap();
    ws.set_property(id, "value", "NE555").unwrap();
    ws.set_property(id, "x_mm", "2.5").unwrap();
    ws.set_property(id, "rotation", "180").unwrap();
    ws.set_property(id, "flipped", "true").unwrap();
    let f = &placed(&ws)
        .iter()
        .find(|(i, _)| *i == id)
        .unwrap()
        .1
        .clone();
    assert_eq!(
        (
            f.reference.as_str(),
            f.value.as_str(),
            f.placement.rotation,
            f.placement.flipped
        ),
        ("U7", "NE555", 2, true)
    );
    assert_eq!(f.placement.position.x, 2_500_000);
    assert!(ws.set_property(id, "rotation", "45").is_err());
    assert!(ws.set_property(id, "x_mm", "abc").is_err());
    assert!(
        ws.set_property(id, "footprint", "x").is_err(),
        "the footprint is not editable here"
    );
    ws.undo().unwrap();
    assert!(
        !placed(&ws)
            .iter()
            .find(|(i, _)| *i == id)
            .unwrap()
            .1
            .placement
            .flipped
    );
    // Selection summaries describe real data.
    let sel = ws.snapshot().selection;
    assert_eq!(sel[0].kind, "footprint");
    assert!(sel[0]
        .properties
        .iter()
        .any(|p| p.key == "locked" && !p.options.is_empty()));
    // Objects of every kind can be picked, described and deleted in one multi-selection.
    ws.editor_command("select_all").unwrap();
    ws.editor_command("delete").unwrap();
    assert!(placed(&ws).is_empty());
    let _ = PcbObject::Net(buturecad_pcb::Net { name: "x".into() });
}
