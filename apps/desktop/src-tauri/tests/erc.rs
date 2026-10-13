//! ERC through the desktop workspace: running, jumping to findings, documented waivers, undo and
//! persistence.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::Point;
use buturecad_schematic::{builtin, Component, Id, Object, Placement};

const G: i64 = 1_270_000;

fn comp(reference: &str, lib: &str, x: i64, y: i64) -> Component {
    Component {
        lib_id: lib.into(),
        reference: reference.into(),
        value: "v".into(),
        footprint: "FP".into(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x * G, y * G)),
    }
}

/// R1 placed with nothing connected: two unconnected pins.
fn project(dir: &std::path::Path) -> (Workspace, Id) {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let r1 = e.new_id();
    e.execute(place_component(
        ROOT_SHEET,
        r1,
        comp("R1", "Buture:R", 10, 10),
    ))
    .unwrap();
    ws.fit_view();
    (ws, r1)
}

fn erc(ws: &mut Workspace) -> ErcInfo {
    ws.erc_run().unwrap();
    ws.snapshot().erc.unwrap()
}

#[test]
fn no_report_until_the_check_is_run_and_it_goes_stale_after_edits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = project(dir.path());
    assert!(ws.snapshot().erc.is_none(), "nothing has been checked yet");
    let info = erc(&mut ws);
    assert!(!info.stale);
    assert_eq!((info.errors, info.warnings), (2, 0));
    assert!(info
        .violations
        .iter()
        .all(|v| v.severity == "error" && v.can_focus));
    assert_eq!(info.violations[0].rule, "unconnected_pin");

    // Any edit makes the list stale.
    ws.rename_project("Other").unwrap();
    assert!(ws.snapshot().erc.unwrap().stale);
    assert!(!erc(&mut ws).stale);
}

#[test]
fn clicking_a_finding_selects_the_part_and_centres_the_view_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1) = project(dir.path());
    ws.set_mode(Mode::Pcb);
    ws.set_mode(Mode::Schematic);
    // Scroll the view far away first.
    ws.views.get_mut(&Mode::Schematic).unwrap().center = (5.0e7, 5.0e7);
    let info = erc(&mut ws);
    let idx = info.violations[0].index as usize;
    ws.set_mode(Mode::Pcb); // the finding must bring us back to the schematic
    ws.erc_focus(idx).unwrap();
    let snap = ws.snapshot();
    assert_eq!(snap.mode, Mode::Schematic);
    assert_eq!(snap.selection.len(), 1);
    assert_eq!(snap.selection[0].id, r1.0);
    // The view is centred on the pin the finding is about (R1 pin 1 at (10, 13) grid units).
    let c = ws.views[&Mode::Schematic].center;
    assert_eq!(c, (10.0 * G as f64, 13.0 * G as f64));

    // Out-of-range indices are errors, not panics.
    assert!(ws.erc_focus(9999).is_err());
}

#[test]
fn waivers_need_a_reason_are_undoable_persist_and_never_hide_the_finding() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, _) = project(dir.path());
    let info = erc(&mut ws);
    let fp = info.violations[0].fingerprint.clone();

    assert!(ws.erc_waive(&fp, "   ").is_err(), "a reason is mandatory");
    assert!(ws.erc_waive("nonsense", "because").is_err());
    ws.erc_waive(&fp, "reserved for the production test jig")
        .unwrap();

    let info = ws.snapshot().erc.unwrap();
    assert_eq!((info.errors, info.waived), (1, 1));
    let w = info
        .violations
        .iter()
        .find(|v| v.fingerprint == fp)
        .unwrap();
    assert_eq!(
        w.waived.as_deref(),
        Some("reserved for the production test jig")
    );

    // It is a command: undo brings the finding back.
    assert!(ws
        .snapshot()
        .project
        .unwrap()
        .undo_history
        .contains(&"Waive ERC finding".to_string()));
    ws.undo().unwrap();
    assert_eq!(erc(&mut ws).errors, 2);
    ws.redo().unwrap();
    assert_eq!(erc(&mut ws).waived, 1);

    // The waiver is part of the saved project.
    ws.save().unwrap();
    ws.close_project().unwrap();
    ws.open_project(&dir.path().join("P")).unwrap();
    let info = erc(&mut ws);
    assert_eq!((info.errors, info.waived), (1, 1));
    let text = std::fs::read_to_string(dir.path().join("P/schematic/main.bsch.json")).unwrap();
    assert!(text.contains("reserved for the production test jig"));

    // Removing it makes the finding active again.
    ws.erc_unwaive(&fp).unwrap();
    assert_eq!(ws.snapshot().erc.unwrap().errors, 2);
    assert!(ws.erc_unwaive(&fp).is_err());
}

#[test]
fn fixing_the_circuit_clears_findings_and_stale_waivers_are_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let (mut ws, r1) = project(dir.path());
    let info = erc(&mut ws);
    ws.erc_waive(&info.violations[0].fingerprint, "known")
        .unwrap();
    // Delete R1: both findings and the waiver's target disappear.
    let e = &mut ws.open.as_mut().unwrap().engine;
    let cmd = delete_objects(e.project().schematic(), &[r1]).unwrap();
    e.execute(cmd).unwrap();
    let info = erc(&mut ws);
    assert_eq!((info.errors, info.warnings), (0, 0));
    assert_eq!(info.infos, 1);
    assert_eq!(info.violations[0].rule, "stale_waiver");
    assert!(!matches!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .schematic()
            .object(r1),
        Some((_, Object::Component(_)))
    ));
}
