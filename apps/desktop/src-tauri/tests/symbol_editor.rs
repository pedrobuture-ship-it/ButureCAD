//! Drives the Symbol Editor like the UI does: editor commands and pointer events against a real
//! Workspace, asserting on the stored symbol, undo history, libraries and placed instances.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_geometry::Point;
use buturecad_libraries::LibraryDir;
use buturecad_schematic::{
    Connectivity, Direction, Graphic, Object, PinType, Symbol, REPR_ALTERNATE,
};

const G: i64 = 1_270_000;

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

fn editor(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_mode(Mode::SymbolEditor);
    ws.set_canvas_size(1000.0, 700.0);
    ws.symbol_editor_command("new", Some("User:OpAmp")).unwrap();
    ws.fit_view();
    // A known view: 1 grid unit = 24 px, origin at the canvas centre.
    let v = ws.views.get_mut(&Mode::SymbolEditor).unwrap();
    v.center = (0.0, 0.0);
    v.scale = 24.0 / G as f64;
    ws
}

fn sym(ws: &Workspace) -> Symbol {
    ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .schematic()
        .symbol("User:OpAmp")
        .unwrap()
        .clone()
}

fn screen(ws: &Workspace, gx: i64, gy: i64) -> (f64, f64) {
    ws.views[&Mode::SymbolEditor].world_to_screen(Point::new(gx * G, gy * G))
}

fn click(ws: &mut Workspace, gx: i64, gy: i64) {
    let (x, y) = screen(ws, gx, gy);
    ws.pointer(ev(PointerKind::Move, x, y, 0));
    ws.pointer(ev(PointerKind::Down, x, y, 0));
    ws.pointer(ev(PointerKind::Up, x, y, 0));
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

#[test]
fn a_new_symbol_is_empty_and_the_editor_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    let s = sym(&ws);
    assert!(s.pins.is_empty() && s.graphics.is_empty());
    let snap = ws.snapshot();
    let info = snap.symbol_editor.unwrap();
    assert_eq!(info.lib_id, "User:OpAmp");
    assert_eq!(
        (info.unit, info.unit_count, info.has_alternate),
        (1, 1, false)
    );
    assert!(info.warnings.iter().any(|w| w.contains("no pins")));
    assert!(snap.project.unwrap().dirty);
}

#[test]
fn drawing_tools_create_real_graphics_and_pins_with_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());

    ws.editor_command("tool:rect").unwrap();
    click(&mut ws, -2, -4);
    click(&mut ws, 2, 4);
    ws.editor_command("tool:circle").unwrap();
    click(&mut ws, 0, 0);
    click(&mut ws, 1, 0);
    ws.editor_command("tool:line").unwrap();
    click(&mut ws, -2, 0);
    click(&mut ws, -1, 1);
    click(&mut ws, -1, 2); // chained second segment
    ws.editor_command("cancel").unwrap();
    ws.editor_command("tool:arc").unwrap();
    click(&mut ws, 0, -2);
    click(&mut ws, 1, -2);
    click(&mut ws, 0, -1);
    ws.editor_command("tool:text:OP").unwrap();
    click(&mut ws, 0, 3);

    let s = sym(&ws);
    let kinds: Vec<&str> = s
        .graphics
        .iter()
        .map(|g| match g.graphic {
            Graphic::Rect { .. } => "rect",
            Graphic::Circle { .. } => "circle",
            Graphic::Line { .. } => "line",
            Graphic::Arc { .. } => "arc",
            Graphic::Text { .. } => "text",
        })
        .collect();
    assert_eq!(kinds, ["rect", "circle", "line", "line", "arc", "text"]);
    assert!(s
        .graphics
        .iter()
        .all(|g| g.unit == 1 && g.representation == 1));

    // Pins use the template: set it to an output pointing left.
    ws.symbol_editor_command("pin_template", Some("type=output"))
        .unwrap();
    ws.symbol_editor_command("pin_template", Some("orientation=left"))
        .unwrap();
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    ws.symbol_editor_command("pin_template", Some("type=input"))
        .unwrap();
    ws.symbol_editor_command("pin_template", Some("orientation=right"))
        .unwrap();
    click(&mut ws, -4, 1);
    click(&mut ws, -4, -1);
    let s = sym(&ws);
    assert_eq!(s.pins.len(), 3);
    assert_eq!(
        (
            s.pins[0].number.as_str(),
            s.pins[0].kind,
            s.pins[0].orientation
        ),
        ("1", PinType::Output, Direction::Left)
    );
    assert_eq!(s.pins[1].number, "2");
    assert_eq!(s.pins[2].number, "3");
    assert_eq!(s.pins[0].position, Point::new(4 * G, 0));

    // Every edit is one undo step and undo restores the empty symbol exactly.
    let history = ws.snapshot().project.unwrap().undo_history;
    assert!(history.len() >= 10, "{history:?}");
    while ws.snapshot().project.unwrap().can_undo {
        if ws.undo().unwrap().as_deref() == Some("Create symbol User:OpAmp") {
            break;
        }
    }
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .schematic()
        .symbol("User:OpAmp")
        .is_none());
}

#[test]
fn selecting_and_dragging_moves_items_through_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    ws.editor_command("tool:select").unwrap();
    ws.editor_command("clear_selection").unwrap();
    // Grab the pin by its body and move it two grid units up.
    drag(&mut ws, (4, 0), (4, 2));
    let s = sym(&ws);
    assert_eq!(s.pins[0].position, Point::new(4 * G, 2 * G));
    assert_eq!(
        ws.snapshot().project.unwrap().undo_history.last().unwrap(),
        "Move items"
    );
    ws.undo().unwrap();
    assert_eq!(sym(&ws).pins[0].position, Point::new(4 * G, 0));
}

#[test]
fn inspector_edits_validate_and_report_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    click(&mut ws, 4, -2);
    let snap = ws.snapshot();
    let sel = &snap.selection;
    assert_eq!(sel.len(), 1);
    let id = sel[0].id;
    ws.set_property(id, "name", "OUT").unwrap();
    ws.set_property(id, "type", "tri_state").unwrap();
    ws.set_property(id, "length_mm", "5.08").unwrap();
    let p = sym(&ws).pins[1].clone();
    assert_eq!(
        (p.name.as_str(), p.kind, p.length),
        ("OUT", PinType::TriState, 5_080_000)
    );
    // Duplicate numbers and bad values are rejected and leave the symbol unchanged.
    assert!(ws.set_property(id, "number", "1").is_err());
    assert!(ws.set_property(id, "type", "nonsense").is_err());
    assert!(ws.set_property(id, "length_mm", "abc").is_err());
    assert_eq!(sym(&ws).pins[1], p);
}

#[test]
fn multiple_units_and_alternate_representation() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    ws.symbol_editor_command("add_unit", None).unwrap();
    assert_eq!(ws.snapshot().symbol_editor.unwrap().unit, 2);
    // Unit 2 starts empty. Its pins are different physical pins, so numbering continues.
    click(&mut ws, 4, 2);
    let s = sym(&ws);
    assert_eq!(s.unit_count, 2);
    assert_eq!(s.pins.len(), 2);
    assert_eq!((s.pins[1].unit, s.pins[1].number.as_str()), (2, "2"));
    ws.symbol_editor_command("unit", Some("1")).unwrap();
    assert_eq!(s.pins_for(1, 1).count(), 1);
    assert_eq!(s.pins_for(2, 1).count(), 1);
    assert!(ws.symbol_editor_command("unit", Some("3")).is_err());

    ws.symbol_editor_command("alternate_create", None).unwrap();
    let s = sym(&ws);
    assert!(s.has_alternate());
    assert_eq!(
        s.pins
            .iter()
            .filter(|p| p.representation == REPR_ALTERNATE)
            .count(),
        2
    );
    assert_eq!(ws.snapshot().symbol_editor.unwrap().representation, 2);
    ws.symbol_editor_command("alternate_remove", None).unwrap();
    assert!(!sym(&ws).has_alternate());
    // Units in use cannot be dropped silently.
    assert!(ws.symbol_editor_set("unit_count", "1").is_err());
}

#[test]
fn saving_to_a_library_and_placing_parts_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    ws.symbol_editor_set("description", "Op-amp").unwrap();
    ws.symbol_editor_set("reference_prefix", "U").unwrap();
    ws.symbol_editor_command("save_to_library", Some("project:User"))
        .unwrap();

    let libs = LibraryDir::new(dir.path().join("P/libraries"));
    let lib = libs.load_symbol_library("User").unwrap();
    assert_eq!(lib.get("OpAmp").unwrap().meta.description, "Op-amp");
    assert_eq!(lib.get("OpAmp").unwrap().pins.len(), 1);
    assert!(ws
        .symbol_libraries()
        .iter()
        .any(|l| l.name == "User" && l.scope == "project"));
    assert!(ws
        .library_symbols()
        .iter()
        .any(|s| s.lib_id == "User:OpAmp"));

    // The built-in library is read-only.
    assert!(ws
        .symbol_editor_command("save_to_library", Some("project:Buture"))
        .is_err());
    assert!(ws
        .symbol_editor_command("save_to_library", Some("nowhere:X"))
        .is_err());
}

#[test]
fn renaming_retargets_placed_instances_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    ws.set_mode(Mode::Schematic);
    ws.set_canvas_size(1000.0, 700.0);
    ws.fit_view();
    ws.editor_command("tool:place_component:User:OpAmp")
        .unwrap();
    {
        let v = ws.views.get_mut(&Mode::Schematic).unwrap();
        v.center = (0.0, 0.0);
    }
    let (x, y) = ws.views[&Mode::Schematic].world_to_screen(Point::new(0, 0));
    ws.pointer(ev(PointerKind::Move, x, y, 0));
    ws.pointer(ev(PointerKind::Down, x, y, 0));
    ws.pointer(ev(PointerKind::Up, x, y, 0));
    let sch = ws.open.as_ref().unwrap().engine.project().schematic();
    assert_eq!(sch.object_count(), 1);

    // The symbol in use cannot be deleted, but it can be renamed.
    ws.set_mode(Mode::SymbolEditor);
    assert!(ws.symbol_editor_command("delete_symbol", None).is_err());
    ws.symbol_editor_set("name", "Analog:OpAmp").unwrap();
    let sch = ws.open.as_ref().unwrap().engine.project().schematic();
    assert!(sch.symbol("User:OpAmp").is_none());
    assert!(sch.symbol("Analog:OpAmp").is_some());
    let used = sch
        .sheets()
        .values()
        .flat_map(|s| s.objects.values())
        .filter(|o| matches!(o, Object::Component(c) if c.lib_id == "Analog:OpAmp"))
        .count();
    assert_eq!(used, 1);
    assert!(sch.validate().is_ok());

    ws.undo().unwrap();
    let sch = ws.open.as_ref().unwrap().engine.project().schematic();
    assert!(sch.symbol("User:OpAmp").is_some() && sch.symbol("Analog:OpAmp").is_none());
    assert!(sch.validate().is_ok());
    let _ = Connectivity::build(sch);
}

#[test]
fn multi_unit_parts_share_a_reference_and_expose_only_their_unit_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    // Unit 1: pins 1 and 2; unit 2: pin 3.
    ws.editor_command("tool:pin").unwrap();
    click(&mut ws, 4, 0);
    click(&mut ws, 4, -2);
    ws.symbol_editor_command("add_unit", None).unwrap();
    click(&mut ws, 4, 0);
    assert_eq!(sym(&ws).pins.len(), 3);
    assert_eq!(sym(&ws).pins[2].number, "3");

    ws.set_mode(Mode::Schematic);
    ws.set_canvas_size(1000.0, 700.0);
    ws.fit_view();
    let place = |ws: &mut Workspace, gx: i64, gy: i64, unit_steps: usize| {
        ws.editor_command("tool:place_component:User:OpAmp")
            .unwrap();
        for _ in 0..unit_steps {
            ws.editor_command("unit_next").unwrap();
        }
        let (x, y) = ws.views[&Mode::Schematic].world_to_screen(Point::new(gx * G, gy * G));
        ws.pointer(ev(PointerKind::Move, x, y, 0));
        ws.pointer(ev(PointerKind::Down, x, y, 0));
        ws.pointer(ev(PointerKind::Up, x, y, 0));
    };
    place(&mut ws, 0, 0, 0); // unit A
    place(&mut ws, 20, 0, 1); // unit B joins the same reference
    place(&mut ws, 40, 0, 0); // a second unit A is a new part
    let sch = ws.open.as_ref().unwrap().engine.project().schematic();
    let mut parts: Vec<(String, u8)> = sch
        .sheets()
        .values()
        .flat_map(|s| s.objects.values())
        .filter_map(|o| match o {
            Object::Component(c) => Some((c.reference.clone(), c.unit)),
            _ => None,
        })
        .collect();
    parts.sort();
    assert_eq!(
        parts,
        [("U1".into(), 1), ("U1".into(), 2), ("U2".into(), 1)]
    );
    for o in sch.sheets().values().flat_map(|s| s.objects.values()) {
        if let Object::Component(c) = o {
            let pins = sch.placed_pins(c).len();
            assert_eq!(pins, if c.unit == 1 { 2 } else { 1 });
        }
    }
    // A symbol edit that would strand a placed unit is refused.
    ws.set_mode(Mode::SymbolEditor);
    assert!(ws.symbol_editor_set("unit_count", "1").is_err());
}
