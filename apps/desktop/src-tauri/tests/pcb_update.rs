//! "Update PCB from Schematic" through the workspace: preview, apply, undo.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{builtin, Change, Component, Object, Placement, Wire};

const G: i64 = 1_270_000;
const FP: &str = "Generated:SOIC-8_3.9mm_P1.27mm";

fn part(reference: &str, fp: &str, x: i64) -> Object {
    Object::Component(Component {
        lib_id: "Buture:R".into(),
        reference: reference.into(),
        value: "10k".into(),
        footprint: fp.into(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x * G, 0)),
    })
}

fn project(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_mode(Mode::FootprintEditor);
    ws.footprint_editor_command("generate", Some("soic|pins=8"))
        .unwrap();
    ws.set_mode(Mode::Pcb);
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let (a, b, w) = (e.new_id(), e.new_id(), e.new_id());
    e.execute(SchematicEditCommand::new(
        "build",
        vec![
            Change::create(ROOT_SHEET, a, part("R1", FP, 0)),
            Change::create(ROOT_SHEET, b, part("R2", FP, 10)),
            Change::create(
                ROOT_SHEET,
                w,
                Object::Wire(Wire {
                    segment: Segment::new(Point::new(0, -3 * G), Point::new(10 * G, 3 * G)),
                }),
            ),
        ],
    ))
    .unwrap();
    ws
}

fn kinds(p: &UpdatePreview) -> Vec<&str> {
    p.lines.iter().map(|l| l.kind.as_str()).collect()
}

#[test]
fn preview_changes_nothing_and_apply_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    let history = ws.snapshot().project.unwrap().undo_history.len();
    let preview = ws.pcb_update_preview().unwrap();
    assert!(preview.can_apply);
    assert_eq!(
        kinds(&preview)
            .iter()
            .filter(|k| **k == "add_footprint")
            .count(),
        2
    );
    assert!(kinds(&preview).contains(&"add_net"));
    assert!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .placed
            .is_empty(),
        "preview is read-only"
    );
    assert_eq!(ws.snapshot().project.unwrap().undo_history.len(), history);

    ws.pcb_update_apply().unwrap();
    let doc = ws.open.as_ref().unwrap().engine.project().pcb().clone();
    assert_eq!((doc.placed.len(), doc.nets.len()), (2, 1));
    assert_eq!(
        ws.snapshot().project.unwrap().undo_history.len(),
        history + 1
    );
    // A second preview has nothing to do; applying it is refused.
    let again = ws.pcb_update_preview().unwrap();
    assert!(!again.can_apply);
    assert!(ws.pcb_update_apply().is_err());
    // The previewed plan is single-use and goes stale after any other edit.
    ws.undo().unwrap();
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .placed
        .is_empty());
    ws.redo().unwrap();
    assert_eq!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .placed
            .len(),
        2
    );
}

#[test]
fn a_stale_preview_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    ws.pcb_update_preview().unwrap();
    ws.rename_project("Other").unwrap();
    let err = ws.pcb_update_apply().unwrap_err();
    assert!(err.contains("preview the update again"), "{err}");
    assert!(
        ws.pcb_update_apply().is_err(),
        "the stale plan was discarded"
    );
}

#[test]
fn board_work_survives_a_schematic_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    ws.pcb_update_preview().unwrap();
    ws.pcb_update_apply().unwrap();

    // Move R1 on the board, then change its value in the schematic.
    let id = ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .placed
        .iter()
        .find(|(_, f)| f.reference == "R1")
        .map(|(i, _)| *i)
        .unwrap();
    ws.selection.insert(Mode::Pcb, [id].into_iter().collect());
    ws.set_property(id.0, "x_mm", "40").unwrap();
    ws.set_mode(Mode::Schematic);
    let comp = ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .schematic()
        .sheet(ROOT_SHEET)
        .unwrap()
        .objects
        .iter()
        .find_map(|(i, o)| matches!(o, Object::Component(c) if c.reference == "R1").then_some(*i))
        .unwrap();
    ws.set_property(comp.0, "value", "47k").unwrap();
    ws.set_mode(Mode::Pcb);

    let preview = ws.pcb_update_preview().unwrap();
    let texts: Vec<&str> = preview.lines.iter().map(|l| l.text.as_str()).collect();
    assert!(texts.iter().any(|t| t.contains("47k")), "{texts:?}");
    assert!(
        !kinds(&preview).contains(&"add_footprint")
            && !kinds(&preview).contains(&"remove_footprint")
    );
    ws.pcb_update_apply().unwrap();
    let f = ws.open.as_ref().unwrap().engine.project().pcb().placed[&id].clone();
    assert_eq!(
        (f.value.as_str(), f.placement.position.x),
        ("47k", 40_000_000),
        "value updated, placement kept"
    );
}

#[test]
fn warnings_are_listed_but_do_not_block_and_nothing_is_applied_for_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_mode(Mode::Pcb);
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let a = e.new_id();
    e.execute(SchematicEditCommand::new(
        "p",
        vec![Change::create(ROOT_SHEET, a, part("R1", "Nope:Missing", 0))],
    ))
    .unwrap();
    let preview = ws.pcb_update_preview().unwrap();
    assert!(!preview.can_apply);
    assert!(preview
        .lines
        .iter()
        .any(|l| l.kind == "warning" && l.text.contains("Nope:Missing")));
}

#[test]
fn footprints_come_from_libraries_when_the_project_lacks_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = project(dir.path());
    // Save the footprint into the project library, then drop it from the project's table.
    ws.set_mode(Mode::FootprintEditor);
    ws.footprint_editor_command("save_to_library", Some("project:Generated"))
        .unwrap();
    ws.footprint_editor_command("delete_footprint", None)
        .unwrap();
    assert!(ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .footprints
        .is_empty());
    ws.set_mode(Mode::Pcb);
    let preview = ws.pcb_update_preview().unwrap();
    assert!(preview.can_apply);
    ws.pcb_update_apply().unwrap();
    let doc = ws.open.as_ref().unwrap().engine.project().pcb();
    assert!(
        doc.footprints.contains_key(FP),
        "the library footprint was embedded"
    );
    assert_eq!(doc.placed.len(), 2);
}
