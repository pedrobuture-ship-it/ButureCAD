//! Drives the schematic editor the way the UI does: pointer events and editor commands against a
//! real Workspace, asserting on the model, the undo history and connectivity.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::edit::topology_of;
use buturecad_schematic::{builtin, Component, Connectivity, Id, Object, Placement, Schematic};

const G: i64 = 1_270_000;

fn comp(reference: &str, lib: &str, x: i64, y: i64) -> Component {
    Component {
        lib_id: lib.into(),
        reference: reference.into(),
        value: String::new(),
        footprint: String::new(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x * G, y * G)),
    }
}

/// Empty project, canvas ready.
fn blank(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.fit_view();
    ws
}

/// R1 → R2 joined by a wire: R1 pin 2 (0,-3) to R2 pin 2 (12,-3).
fn with_two_resistors(dir: &std::path::Path) -> (Workspace, Id, Id) {
    let mut ws = blank(dir);
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let (r1, r2) = (e.new_id(), e.new_id());
    e.execute(place_component(
        ROOT_SHEET,
        r1,
        comp("R1", "Buture:R", 0, 0),
    ))
    .unwrap();
    e.execute(place_component(
        ROOT_SHEET,
        r2,
        comp("R2", "Buture:R", 12, 0),
    ))
    .unwrap();
    let w = e.new_id();
    e.execute(create_wire(
        ROOT_SHEET,
        w,
        Segment::new(Point::new(0, -3 * G), Point::new(12 * G, -3 * G)),
    ))
    .unwrap();
    ws.fit_view();
    (ws, r1, r2)
}

fn sch(ws: &Workspace) -> &Schematic {
    ws.open.as_ref().unwrap().engine.project().schematic()
}

fn objects(ws: &Workspace) -> usize {
    sch(ws).object_count()
}

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

fn screen(ws: &Workspace, x: i64, y: i64) -> (f64, f64) {
    ws.views[&Mode::Schematic].world_to_screen(Point::new(x, y))
}

/// Click at a world position given in grid units.
fn click(ws: &mut Workspace, gx: i64, gy: i64) -> PointerResult {
    let (x, y) = screen(ws, gx * G, gy * G);
    ws.pointer(ev(PointerKind::Move, x, y, 0));
    let down = ws.pointer(ev(PointerKind::Down, x, y, 0));
    let mut up = ws.pointer(ev(PointerKind::Up, x, y, 0));
    up.model_changed |= down.model_changed;
    up.selection_changed |= down.selection_changed;
    up
}

/// What a browser sends for a double click: two clicks, then the double-click event.
fn double_click(ws: &mut Workspace, gx: i64, gy: i64) {
    click(ws, gx, gy);
    click(ws, gx, gy);
    let (x, y) = screen(ws, gx * G, gy * G);
    ws.pointer(ev(PointerKind::DoubleClick, x, y, 0));
}

fn drag(ws: &mut Workspace, from: (i64, i64), to: (i64, i64)) -> PointerResult {
    let (x0, y0) = screen(ws, from.0 * G, from.1 * G);
    let (x1, y1) = screen(ws, to.0 * G, to.1 * G);
    ws.pointer(ev(PointerKind::Move, x0, y0, 0));
    ws.pointer(ev(PointerKind::Down, x0, y0, 0));
    for i in 1..=5 {
        let t = i as f64 / 5.0;
        ws.pointer(ev(
            PointerKind::Move,
            x0 + (x1 - x0) * t,
            y0 + (y1 - y0) * t,
            0,
        ));
    }
    ws.pointer(ev(PointerKind::Up, x1, y1, 0))
}

fn component(ws: &Workspace, id: Id) -> Component {
    match sch(ws).object(id).unwrap().1 {
        Object::Component(c) => c.clone(),
        other => panic!("not a component: {other:?}"),
    }
}

fn selected(ws: &Workspace) -> Vec<Id> {
    ws.selection
        .get(&Mode::Schematic)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default()
}

fn undo_labels(ws: &mut Workspace) -> Vec<String> {
    ws.snapshot().project.unwrap().undo_history
}

#[test]
fn placing_components_embeds_the_symbol_numbers_references_and_undoes_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:place_component:Buture:R").unwrap();
    assert_eq!(ws.snapshot().tool, "place_component");
    let r = click(&mut ws, 4, 4);
    assert!(r.model_changed);
    click(&mut ws, 12, 4);
    assert_eq!(objects(&ws), 2);
    let refs: Vec<String> = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .filter_map(|o| {
            if let Object::Component(c) = o {
                Some(c.reference.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(refs, vec!["R1", "R2"]);
    assert!(
        sch(&ws).symbol("Buture:R").is_some(),
        "the used symbol is embedded in the project"
    );
    // Each placement is one undo step (the first also embeds the symbol, inside the same step).
    assert_eq!(
        undo_labels(&mut ws),
        vec!["Place component", "Place component"]
    );
    ws.undo().unwrap();
    ws.undo().unwrap();
    assert_eq!(objects(&ws), 0);
    assert!(
        sch(&ws).symbol("Buture:R").is_none(),
        "undo also removes the embedded symbol"
    );
    // Placement stays in the tool until Esc.
    ws.redo().unwrap();
    ws.editor_command("cancel").unwrap();
    assert_eq!(ws.snapshot().tool, "select");
}

#[test]
fn ghost_rotation_and_mirror_apply_to_the_placed_part() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:place_component:Buture:LED")
        .unwrap();
    ws.editor_command("rotate").unwrap();
    ws.editor_command("mirror_x").unwrap();
    click(&mut ws, 6, 6);
    let c = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .find_map(|o| {
            if let Object::Component(c) = o {
                Some(c.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!((c.placement.rotation, c.placement.mirror), (1, true));
    assert_eq!(c.value, "LED");
}

#[test]
fn power_tool_places_a_named_power_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    assert!(
        ws.editor_command("tool:power:Buture:R").is_err(),
        "a resistor is not a power symbol"
    );
    ws.editor_command("tool:power:Buture:GND").unwrap();
    click(&mut ws, 5, 5);
    let found = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .any(|o| matches!(o, Object::PowerSymbol(p) if p.net_name == "GND"));
    assert!(found);
}

#[test]
fn drawing_a_wire_with_corners_creates_connected_orthogonal_segments() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:place_component:Buture:R").unwrap();
    click(&mut ws, 0, 0); // R1: pins (0,3) (0,-3)
    click(&mut ws, 14, 0); // R2: pins (14,3) (14,-3)
    ws.editor_command("cancel").unwrap();

    ws.editor_command("tool:wire").unwrap();
    click(&mut ws, 0, -3); // start on R1 pin 2 (object snap)
    click(&mut ws, 7, -8); // a corner: the elbow is added
                           // Ending on a connection point (R2's pin 2) finishes the wire.
    click(&mut ws, 14, -3);
    let c = Connectivity::build(sch(&ws));
    let wires = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .filter(|o| matches!(o, Object::Wire(_)))
        .count();
    assert!(
        wires >= 2,
        "an L/Z-shaped wire has several segments, got {wires}"
    );
    let (r1, r2) = {
        let ids: Vec<Id> = sch(&ws).sheets()[&ROOT_SHEET]
            .objects
            .iter()
            .filter(|(_, o)| matches!(o, Object::Component(_)))
            .map(|(i, _)| *i)
            .collect();
        (ids[0], ids[1])
    };
    let net = c.net_of_pin(r1, "2").unwrap();
    assert!(
        net.pins.iter().any(|p| p.component == r2 && p.pin == "2"),
        "the drawn wire joins R1.2 and R2.2"
    );
    let all_orthogonal = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .all(|o| match o {
            Object::Wire(w) => w.segment.a.x == w.segment.b.x || w.segment.a.y == w.segment.b.y,
            _ => true,
        });
    assert!(all_orthogonal);
    assert!(undo_labels(&mut ws).contains(&"Draw wire".to_string()));
}

#[test]
fn wire_tool_can_be_cancelled_without_changing_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:wire").unwrap();
    click(&mut ws, 2, 2);
    let (x, y) = screen(&ws, 8 * G, 5 * G);
    ws.pointer(ev(PointerKind::Move, x, y, 0));
    ws.editor_command("cancel").unwrap(); // abandons the wire being drawn
    assert_eq!(objects(&ws), 0);
    assert_eq!(
        ws.snapshot().tool,
        "wire",
        "the first Esc only cancels the gesture"
    );
    ws.editor_command("cancel").unwrap();
    assert_eq!(ws.snapshot().tool, "select");
}

#[test]
fn a_t_branch_gets_an_automatic_junction() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:wire").unwrap();
    click(&mut ws, 0, 0);
    double_click(&mut ws, 10, 0);
    // Second wire starts on the interior of the first.
    ws.editor_command("tool:wire").unwrap();
    click(&mut ws, 5, 0);
    click(&mut ws, 5, 6);
    double_click(&mut ws, 5, 6);
    let junctions = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .filter(|o| matches!(o, Object::Junction(_)))
        .count();
    assert_eq!(junctions, 1);
}

#[test]
fn dragging_a_part_moves_it_and_its_wires_follow() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    let before = topology_of(sch(&ws));
    let rev_before = ws.open.as_ref().unwrap().engine.revision();

    // Press on R1's body and drag 4 grid steps up and 6 right.
    let r = drag(&mut ws, (0, 0), (6, 4));
    assert!(r.model_changed);
    assert_eq!(
        component(&ws, r1).placement.position,
        Point::new(6 * G, 4 * G)
    );
    assert_eq!(
        topology_of(sch(&ws)),
        before,
        "wires stretched with the part: nothing was disconnected"
    );
    assert_eq!(
        undo_labels(&mut ws).last().map(String::as_str),
        Some("Move")
    );
    assert!(ws.open.as_ref().unwrap().engine.revision() > rev_before);
    ws.undo().unwrap();
    assert_eq!(component(&ws, r1).placement.position, Point::new(0, 0));
    assert_eq!(topology_of(sch(&ws)), before);
}

#[test]
fn a_move_is_only_previewed_until_the_button_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    let (x0, y0) = screen(&ws, 0, 0);
    let (x1, y1) = screen(&ws, 5 * G, 5 * G);
    ws.pointer(ev(PointerKind::Move, x0, y0, 0));
    ws.pointer(ev(PointerKind::Down, x0, y0, 0));
    ws.pointer(ev(PointerKind::Move, x1, y1, 0));
    let history_during = undo_labels(&mut ws);
    assert_eq!(
        component(&ws, r1).placement.position,
        Point::new(0, 0),
        "the model is untouched mid-drag"
    );
    // Esc abandons the drag: no command was executed.
    ws.editor_command("cancel").unwrap();
    ws.pointer(ev(PointerKind::Up, x1, y1, 0));
    assert_eq!(undo_labels(&mut ws), history_during);
    assert_eq!(component(&ws, r1).placement.position, Point::new(0, 0));
    // The scene shows the original again.
    let hover = ws.pointer(ev(PointerKind::Move, x0, y0, 0)).hover;
    assert!(
        hover.is_some_and(|h| h.starts_with("R1")),
        "R1 is back under the cursor"
    );
}

#[test]
fn alt_drag_moves_without_stretching_and_reports_the_detached_part() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    let before = topology_of(sch(&ws));
    let (x0, y0) = screen(&ws, 0, 0);
    let (x1, y1) = screen(&ws, 6 * G, 4 * G);
    let mut d = ev(PointerKind::Down, x0, y0, 0);
    d.alt = true;
    ws.pointer(ev(PointerKind::Move, x0, y0, 0));
    ws.pointer(d);
    ws.pointer(ev(PointerKind::Move, x1, y1, 0));
    ws.pointer(ev(PointerKind::Up, x1, y1, 0));
    assert_eq!(
        component(&ws, r1).placement.position,
        Point::new(6 * G, 4 * G)
    );
    assert_ne!(
        topology_of(sch(&ws)),
        before,
        "Alt disables rubber-banding, so the part detaches"
    );
}

#[test]
fn move_by_keyboard_commits_on_click() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    assert!(
        ws.editor_command("start_move").is_err(),
        "needs a selection"
    );
    click(&mut ws, 0, 0); // select R1
    assert_eq!(selected(&ws), vec![r1]);
    let (x0, y0) = screen(&ws, 0, 0);
    ws.pointer(ev(PointerKind::Move, x0, y0, 0));
    ws.editor_command("start_move").unwrap();
    let (x1, y1) = screen(&ws, 3 * G, -2 * G);
    ws.pointer(ev(PointerKind::Move, x1, y1, 0));
    let r = ws.pointer(ev(PointerKind::Down, x1, y1, 0));
    assert!(r.model_changed);
    assert_eq!(
        component(&ws, r1).placement.position,
        Point::new(3 * G, -2 * G)
    );
}

#[test]
fn rotate_and_mirror_keep_connections_and_are_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    let before = topology_of(sch(&ws));
    click(&mut ws, 0, 0);
    ws.editor_command("rotate").unwrap();
    assert_eq!(component(&ws, r1).placement.rotation, 1);
    assert_eq!(topology_of(sch(&ws)), before);
    ws.editor_command("mirror_x").unwrap();
    assert!(component(&ws, r1).placement.mirror);
    assert_eq!(topology_of(sch(&ws)), before);
    ws.editor_command("mirror_y").unwrap();
    assert_eq!(topology_of(sch(&ws)), before);
    assert_eq!(
        undo_labels(&mut ws)
            .iter()
            .filter(|l| l.starts_with("Rotate") || l.starts_with("Mirror"))
            .count(),
        3
    );
    for _ in 0..3 {
        ws.undo().unwrap();
    }
    assert_eq!(component(&ws, r1).placement, Placement::at(Point::ORIGIN));
}

#[test]
fn rotate_tool_acts_on_the_clicked_object() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _, r2) = with_two_resistors(dir.path());
    ws.editor_command("tool:rotate").unwrap();
    assert_eq!(
        ws.snapshot().tool,
        "rotate",
        "nothing selected: the tool waits for a click"
    );
    click(&mut ws, 12, 0);
    assert_eq!(component(&ws, r2).placement.rotation, 1);
}

#[test]
fn delete_removes_the_selection_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    click(&mut ws, 0, 0);
    ws.editor_command("delete").unwrap();
    assert!(sch(&ws).object(r1).is_none());
    assert!(selected(&ws).is_empty());
    ws.undo().unwrap();
    assert!(sch(&ws).object(r1).is_some());
}

#[test]
fn duplicate_selects_the_copies_with_new_references() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    click(&mut ws, 0, 0);
    ws.editor_command("duplicate").unwrap();
    let new = selected(&ws);
    assert_eq!(new.len(), 1);
    assert_ne!(new[0], r1);
    assert_eq!(component(&ws, new[0]).reference, "R3");
}

#[test]
fn copy_cut_and_paste_roundtrip_with_a_ghost() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    assert!(ws.editor_command("paste").is_err(), "empty clipboard");
    click(&mut ws, 0, 0);
    ws.editor_command("copy").unwrap();
    assert!(ws.snapshot().can_paste);
    ws.editor_command("paste").unwrap();
    // The ghost follows the pointer; a click drops it.
    let before = objects(&ws);
    click(&mut ws, 10, 12);
    assert_eq!(objects(&ws), before + 1);
    let pasted = selected(&ws);
    assert_eq!(component(&ws, pasted[0]).reference, "R3");
    // Cut removes and keeps it on the clipboard.
    ws.selection
        .insert(Mode::Schematic, [r1].into_iter().collect());
    ws.editor_command("cut").unwrap();
    assert!(sch(&ws).object(r1).is_none());
    ws.editor_command("paste").unwrap();
    click(&mut ws, -10, 12);
    assert_eq!(
        component(&ws, selected(&ws)[0]).reference,
        "R1",
        "the freed reference is reused"
    );
}

#[test]
fn labels_auto_increment_and_junctions_are_placed_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = blank(dir.path());
    ws.editor_command("tool:net_label:D0").unwrap();
    click(&mut ws, 2, 2);
    click(&mut ws, 2, 4);
    assert_eq!(ws.snapshot().tool_arg.as_deref(), Some("D2"));
    let names: Vec<String> = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .filter_map(|o| {
            if let Object::NetLabel(l) = o {
                Some(l.name.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(names, vec!["D0", "D1"]);
    assert!(
        ws.editor_command("tool:net_label").is_err(),
        "a label needs a name"
    );
    ws.editor_command("tool:junction").unwrap();
    click(&mut ws, 8, 8);
    click(&mut ws, 8, 8);
    let junctions = sch(&ws).sheets()[&ROOT_SHEET]
        .objects
        .values()
        .filter(|o| matches!(o, Object::Junction(_)))
        .count();
    assert_eq!(junctions, 1);
}

#[test]
fn box_selection_then_group_move_keeps_connectivity() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, r2) = with_two_resistors(dir.path());
    let before = topology_of(sch(&ws));
    // Window around both resistors (and the wire between them).
    let (x0, y0) = screen(&ws, -4 * G, 6 * G);
    let (x1, y1) = screen(&ws, 16 * G, -6 * G);
    ws.pointer(ev(PointerKind::Down, x0, y0, 0));
    ws.pointer(ev(PointerKind::Move, x1, y1, 0));
    ws.pointer(ev(PointerKind::Up, x1, y1, 0));
    let sel = selected(&ws);
    assert!(sel.contains(&r1) && sel.contains(&r2) && sel.len() == 3);
    drag(&mut ws, (0, 0), (0, 5));
    assert_eq!(component(&ws, r1).placement.position, Point::new(0, 5 * G));
    assert_eq!(
        component(&ws, r2).placement.position,
        Point::new(12 * G, 5 * G)
    );
    assert_eq!(topology_of(sch(&ws)), before);
}

#[test]
fn inspector_edits_go_through_commands_and_validate() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    ws.set_property(r1.0, "value", "4k7").unwrap();
    assert_eq!(component(&ws, r1).value, "4k7");
    assert_eq!(
        undo_labels(&mut ws).last().map(String::as_str),
        Some("Edit value")
    );
    assert!(
        ws.set_property(r1.0, "reference", "  ").is_err(),
        "an empty reference is rejected"
    );
    assert_eq!(component(&ws, r1).reference, "R1");
    assert!(ws.set_property(r1.0, "position", "1,2").is_err());
    assert!(ws.set_property(9999, "value", "x").is_err());
    ws.undo().unwrap();
    assert_eq!(component(&ws, r1).value, "");
}

#[test]
fn editing_is_refused_outside_the_schematic_and_without_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, ..) = with_two_resistors(dir.path());
    ws.set_mode(Mode::Pcb);
    assert!(ws.editor_command("tool:wire").is_err());
    assert!(ws.editor_command("delete").is_err());
    assert!(ws.editor_command("tool:bogus").is_err());
    let mut empty = Workspace::new(None);
    assert!(empty.editor_command("tool:wire").is_err());
    assert!(
        empty.library_symbols().len() >= 8,
        "the built-in library is always offered"
    );
}

#[test]
fn edits_survive_save_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1, _) = with_two_resistors(dir.path());
    drag(&mut ws, (0, 0), (2, 2));
    ws.save().unwrap();
    ws.close_project().unwrap();
    ws.open_project(&dir.path().join("P")).unwrap();
    assert_eq!(
        component(&ws, r1).placement.position,
        Point::new(2 * G, 2 * G)
    );
    assert!(sch(&ws).validate().is_ok());
}
