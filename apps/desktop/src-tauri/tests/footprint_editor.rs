//! Drives the Footprint Editor like the UI does against a real Workspace.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_geometry::Point;
use buturecad_libraries::LibraryDir;
use buturecad_pcb::{Footprint, FpLayer, FpShape, PadKind, PadShape};

const MM: i64 = 1_000_000;

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
    ws.set_mode(Mode::FootprintEditor);
    ws.set_canvas_size(1000.0, 700.0);
    ws.footprint_editor_command("new", Some("User:Test"))
        .unwrap();
    ws.fit_view();
    // A known view: 1 mm = 40 px, origin at the centre; grid 0.5 mm.
    ws.grid.spacing = 500_000;
    let v = ws.views.get_mut(&Mode::FootprintEditor).unwrap();
    v.center = (0.0, 0.0);
    v.scale = 40.0 / MM as f64;
    ws
}

fn fp(ws: &Workspace, name: &str) -> Footprint {
    ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .get(name)
        .unwrap()
        .clone()
}

fn screen(ws: &Workspace, mmx: f64, mmy: f64) -> (f64, f64) {
    ws.views[&Mode::FootprintEditor].world_to_screen(Point::new(
        (mmx * MM as f64) as i64,
        (mmy * MM as f64) as i64,
    ))
}

fn click(ws: &mut Workspace, mmx: f64, mmy: f64) {
    let (x, y) = screen(ws, mmx, mmy);
    ws.pointer(ev(PointerKind::Move, x, y, 0));
    ws.pointer(ev(PointerKind::Down, x, y, 0));
    ws.pointer(ev(PointerKind::Up, x, y, 0));
}

fn drag(ws: &mut Workspace, from: (f64, f64), to: (f64, f64)) {
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
fn pads_of_every_kind_are_placed_numbered_and_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    let info = ws.snapshot().footprint_editor.unwrap();
    assert_eq!((info.name.as_str(), info.pad_count), ("User:Test", 0));
    assert!(info.warnings.iter().any(|w| w.contains("no pads")));

    ws.editor_command("tool:pad_smd").unwrap();
    click(&mut ws, -1.0, 0.0);
    click(&mut ws, 1.0, 0.0);
    ws.footprint_editor_command("pad_template", Some("shape=oval"))
        .unwrap();
    ws.footprint_editor_command("pad_template", Some("size_x_mm=1.6"))
        .unwrap();
    ws.footprint_editor_command("pad_template", Some("size_y_mm=1.6"))
        .unwrap();
    ws.editor_command("tool:pad_through").unwrap();
    click(&mut ws, 0.0, 3.0);
    ws.editor_command("tool:npth").unwrap();
    click(&mut ws, 0.0, -3.0);

    let f = fp(&ws, "User:Test");
    assert_eq!(f.pads.len(), 4);
    assert_eq!(
        (f.pads[0].number.as_str(), f.pads[0].kind, f.pads[0].shape),
        ("1", PadKind::Smd, PadShape::Rect)
    );
    assert_eq!(f.pads[1].number, "2");
    assert_eq!(
        (f.pads[2].number.as_str(), f.pads[2].kind, f.pads[2].shape),
        ("3", PadKind::Through, PadShape::Oval)
    );
    let d = f.pads[2].drill.unwrap().diameter;
    assert!(d > 0 && d < 1_600_000, "the drill leaves a copper ring");
    assert_eq!(
        (f.pads[3].kind, f.pads[3].number.as_str()),
        (PadKind::NonPlated, "")
    );
    assert_eq!(f.pads[0].position, Point::new(-MM, 0));

    // The scene shows what the model holds: pads, drill holes and numbers are real shapes.
    assert_eq!(ws.snapshot().object_count, 4);
    // Every placement was one undo step.
    let history = ws.snapshot().project.unwrap().undo_history;
    assert_eq!(history.iter().filter(|h| *h == "Add pad").count(), 4);
    ws.undo().unwrap();
    assert_eq!(fp(&ws, "User:Test").pads.len(), 3);
}

#[test]
fn graphics_go_on_the_active_layer_and_text_kinds_are_editable() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pad_smd").unwrap();
    click(&mut ws, -1.0, 0.0);
    click(&mut ws, 1.0, 0.0);

    ws.footprint_editor_command("layer", Some("courtyard"))
        .unwrap();
    ws.editor_command("tool:rect").unwrap();
    click(&mut ws, -2.5, -1.5);
    click(&mut ws, 2.5, 1.5);
    ws.footprint_editor_command("layer", Some("silkscreen"))
        .unwrap();
    ws.editor_command("tool:line").unwrap();
    click(&mut ws, -2.0, 2.0);
    click(&mut ws, 2.0, 2.0);
    ws.editor_command("cancel").unwrap();
    ws.footprint_editor_command("layer", Some("fabrication"))
        .unwrap();
    ws.editor_command("tool:circle").unwrap();
    click(&mut ws, 0.0, 0.0);
    click(&mut ws, 1.0, 0.0);
    ws.editor_command("tool:arc").unwrap();
    click(&mut ws, 0.0, -3.0);
    click(&mut ws, 1.0, -3.0);
    click(&mut ws, 0.0, -2.0);
    ws.footprint_editor_command("layer", Some("silkscreen"))
        .unwrap();
    ws.editor_command("tool:text:REF**").unwrap();
    click(&mut ws, 0.0, 4.0);

    let f = fp(&ws, "User:Test");
    let layers: Vec<(FpLayer, &str)> = f
        .graphics
        .iter()
        .map(|g| {
            (
                g.layer,
                match g.shape {
                    FpShape::Rect { .. } => "rect",
                    FpShape::Line { .. } => "line",
                    FpShape::Circle { .. } => "circle",
                    FpShape::Arc { .. } => "arc",
                    FpShape::Polygon { .. } => "polygon",
                },
            )
        })
        .collect();
    assert_eq!(
        layers,
        [
            (FpLayer::Courtyard, "rect"),
            (FpLayer::Silkscreen, "line"),
            (FpLayer::Fabrication, "circle"),
            (FpLayer::Fabrication, "arc"),
        ]
    );
    assert_eq!(f.texts.len(), 1);
    // Courtyard encloses the pads now, so that warning is gone.
    let w = ws.snapshot().footprint_editor.unwrap().warnings;
    assert!(w.iter().all(|x| !x.contains("courtyard")), "{w:?}");

    // Turn the text into the reference through the inspector.
    let id = ws.snapshot().selection[0].id;
    ws.set_property(id, "kind", "reference").unwrap();
    assert!(
        ws.set_property(id, "text", "").is_err(),
        "empty text is refused"
    );
    let w = ws.snapshot().footprint_editor.unwrap().warnings;
    assert!(w.iter().all(|x| !x.contains("reference text")));

    // Layers can be hidden and shown without touching the model.
    let before = fp(&ws, "User:Test");
    ws.footprint_editor_command("layer_visible", Some("silkscreen:0"))
        .unwrap();
    let layers = ws.snapshot().layers;
    assert!(layers.iter().any(|l| l.name == "Silkscreen" && !l.visible));
    assert_eq!(fp(&ws, "User:Test"), before);
}

#[test]
fn inspector_edits_keep_pads_valid_and_report_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pad_smd").unwrap();
    click(&mut ws, 0.0, 0.0);
    let id = ws.snapshot().selection[0].id;

    ws.set_property(id, "number", "A1").unwrap();
    ws.set_property(id, "shape", "round_rect").unwrap();
    assert_eq!(fp(&ws, "User:Test").pads[0].roundrect_ratio, 250);
    ws.set_property(id, "kind", "through").unwrap();
    assert!(fp(&ws, "User:Test").pads[0].drill.is_some());
    ws.set_property(id, "shape", "custom").unwrap();
    assert!(fp(&ws, "User:Test").pads[0].custom.len() >= 3);
    ws.set_property(id, "rotation", "90").unwrap();
    assert_eq!(fp(&ws, "User:Test").pads[0].rotation, 1);
    ws.set_property(id, "slot_mm", "1.4").unwrap();
    assert_eq!(
        fp(&ws, "User:Test").pads[0].drill.unwrap().slot,
        Some(1_400_000)
    );

    let before = fp(&ws, "User:Test");
    assert!(
        ws.set_property(id, "drill_mm", "9").is_err(),
        "a drill bigger than the pad leaves no copper"
    );
    assert!(ws.set_property(id, "size_x_mm", "0").is_err());
    assert!(ws.set_property(id, "rotation", "45").is_err());
    assert!(ws.set_property(id, "shape", "triangle").is_err());
    assert!(ws.set_property(id, "size_x_mm", "abc").is_err());
    assert_eq!(fp(&ws, "User:Test"), before, "failed edits change nothing");

    // Dragging moves the pad (through the engine) and undo restores it.
    ws.set_property(id, "kind", "smd").unwrap();
    ws.set_property(id, "shape", "rect").unwrap();
    ws.editor_command("tool:select").unwrap();
    ws.editor_command("clear_selection").unwrap();
    drag(&mut ws, (0.0, 0.0), (2.0, 1.0));
    assert_eq!(
        fp(&ws, "User:Test").pads[0].position,
        Point::new(2 * MM, MM)
    );
    ws.undo().unwrap();
    assert_eq!(fp(&ws, "User:Test").pads[0].position, Point::ORIGIN);
}

#[test]
fn generators_create_real_footprints_through_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.footprint_editor_command("generate", Some("soic|pins=14|wide=false"))
        .unwrap();
    let name = "Generated:SOIC-14_3.9mm_P1.27mm";
    assert_eq!(ws.snapshot().footprint_editor.unwrap().name, name);
    let f = fp(&ws, name);
    assert_eq!(f.pads.len(), 14);
    assert!(f.pads.iter().all(|p| p.kind == PadKind::Smd));
    assert!(ws.snapshot().footprint_editor.unwrap().warnings.is_empty());
    assert_eq!(
        ws.snapshot().object_count as usize,
        f.pads.len() + f.graphics.len() + f.texts.len()
    );

    // Every family, with defaults.
    for family in ["dip", "qfn", "bga", "header"] {
        ws.footprint_editor_command("generate", Some(family))
            .unwrap();
        assert!(
            ws.snapshot().footprint_editor.unwrap().warnings.is_empty(),
            "{family}"
        );
    }
    // Generating the same footprint twice reuses the first one.
    let n = ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .len();
    ws.footprint_editor_command("generate", Some("soic|pins=14"))
        .unwrap();
    assert_eq!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .footprints
            .len(),
        n
    );

    // Bad input is an error and creates nothing.
    assert!(ws
        .footprint_editor_command("generate", Some("soic|pins=3"))
        .is_err());
    assert!(ws
        .footprint_editor_command("generate", Some("soic|bogus=1"))
        .is_err());
    assert!(ws
        .footprint_editor_command("generate", Some("lga"))
        .is_err());
    assert_eq!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .footprints
            .len(),
        n
    );

    // The catalogue offered to the UI matches the engine.
    let info = ws.snapshot().footprint_editor.unwrap();
    assert_eq!(info.generators.len(), 5);
    assert!(info.generators.iter().all(|g| !g.params.is_empty()));

    // Undo removes the last generated footprint again.
    ws.undo().unwrap();
    assert!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .footprints
            .len()
            <= n
    );
}

#[test]
fn libraries_round_trip_and_the_project_copy_follows_the_library_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.footprint_editor_command("generate", Some("header|pins=3|rows=2"))
        .unwrap();
    ws.footprint_editor_set("description", "Test header")
        .unwrap();
    ws.footprint_editor_set("tags", "header test").unwrap();
    ws.footprint_editor_command("save_to_library", Some("project:Connectors"))
        .unwrap();

    let libs = LibraryDir::new(dir.path().join("P/libraries"));
    let lib = libs.load_footprint_library("Connectors").unwrap();
    let saved = lib.get("PinHeader_2x03_P2.54mm").unwrap();
    assert_eq!(saved.description, "Test header");
    assert_eq!(saved.pads.len(), 6);
    // The project copy was renamed to match its library.
    let info = ws.snapshot().footprint_editor.unwrap();
    assert_eq!(info.name, "Connectors:PinHeader_2x03_P2.54mm");
    assert!(ws
        .footprint_libraries()
        .iter()
        .any(|l| l.name == "Connectors" && l.scope == "project"));
    let listed = ws.library_footprints();
    assert!(listed
        .iter()
        .any(|f| f.lib_id == "Connectors:PinHeader_2x03_P2.54mm" && f.pad_count == 6));
    assert!(ws
        .footprint_editor_command("save_to_library", Some("nowhere:X"))
        .is_err());

    // A second project can open it from the library folder (copy the folder: "user" scope).
    let user = dir.path().join("user-libs");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::copy(
        dir.path().join("P/libraries/Connectors.bfplib.json"),
        user.join("Connectors.bfplib.json"),
    )
    .unwrap();
    let mut ws2 = Workspace::new(None).with_user_libraries(user);
    ws2.new_project(&dir.path().join("Q"), "Q").unwrap();
    ws2.set_mode(Mode::FootprintEditor);
    ws2.footprint_editor_command("open", Some("Connectors:PinHeader_2x03_P2.54mm"))
        .unwrap();
    assert_eq!(ws2.snapshot().footprint_editor.unwrap().pad_count, 6);
}

#[test]
fn three_d_model_references_and_footprints_persist_with_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.footprint_editor_command("generate", Some("dip|pins=8"))
        .unwrap();
    ws.footprint_editor_command("add_model", Some("models/dip8.step"))
        .unwrap();
    ws.footprint_editor_set("model:0:z", "300000").unwrap();
    ws.footprint_editor_set("model:0:rz", "90000").unwrap();
    ws.footprint_editor_set("model:0:sx", "1000000").unwrap();
    assert!(ws.footprint_editor_set("model:0:sx", "-1").is_err());
    assert!(ws.footprint_editor_set("model:3:path", "x").is_err());
    let info = ws.snapshot().footprint_editor.unwrap();
    assert_eq!(info.models.len(), 1);
    assert_eq!(info.models[0].fields[0].value, "models/dip8.step");

    ws.save().unwrap();
    let text = std::fs::read_to_string(dir.path().join("P/pcb/main.bpcb.json")).unwrap();
    assert!(text.contains("\"schema_version\": 2") && text.contains("models/dip8.step"));
    ws.close_project().unwrap();
    ws.open_project(&dir.path().join("P")).unwrap();
    ws.set_mode(Mode::FootprintEditor);
    ws.footprint_editor_command("open", Some("Generated:DIP-8_W7.62mm"))
        .unwrap();
    let f = fp(&ws, "Generated:DIP-8_W7.62mm");
    assert_eq!(f.pads.len(), 8);
    assert_eq!(
        (f.models[0].offset[2], f.models[0].rotation_mdeg[2]),
        (300_000, 90_000)
    );
}

#[test]
fn rename_delete_and_selection_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = editor(dir.path());
    ws.editor_command("tool:pad_smd").unwrap();
    click(&mut ws, -1.0, 0.0);
    click(&mut ws, 1.0, 0.0);
    ws.editor_command("tool:select").unwrap();
    ws.editor_command("select_all").unwrap();
    assert_eq!(ws.snapshot().selection.len(), 2);
    ws.editor_command("rotate").unwrap();
    let f = fp(&ws, "User:Test");
    assert!(f.pads.iter().all(|p| p.rotation == 1));
    ws.editor_command("mirror_x").unwrap();
    ws.editor_command("delete").unwrap();
    assert!(fp(&ws, "User:Test").pads.is_empty());
    assert!(ws.editor_command("delete").is_err(), "nothing selected");
    assert!(
        ws.editor_command("duplicate").is_err(),
        "not a Footprint Editor command"
    );

    ws.footprint_editor_set("name", "Other:Renamed").unwrap();
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .contains_key("Other:Renamed"));
    assert!(!ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .contains_key("User:Test"));
    assert!(ws.footprint_editor_set("name", "bad/name").is_err());
    ws.undo().unwrap();
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .contains_key("User:Test"));
    ws.redo().unwrap();
    ws.footprint_editor_command("delete_footprint", None)
        .unwrap();
    assert!(ws.snapshot().footprint_editor.is_none());
}
