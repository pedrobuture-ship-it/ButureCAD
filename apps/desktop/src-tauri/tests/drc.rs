//! DRC through the workspace: run, list, jump to a violation.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net};

#[test]
fn drc_lists_real_violations_and_focus_selects_the_objects() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.set_mode(Mode::Pcb);
    assert!(ws.snapshot().drc.is_none(), "nothing is shown before a run");

    let e = &mut ws.open.as_mut().unwrap().engine;
    let (n1, n2, t1, t2) = (e.new_id(), e.new_id(), e.new_id(), e.new_id());
    let track = |y: i64, net| {
        PcbObject::Track(Track {
            layer: layer::F_CU,
            segment: Segment::new(Point::new(0, y), Point::new(5_000_000, y)),
            width: 250_000,
            net: Some(net),
        })
    };
    e.execute(PcbEditCommand::new(
        "t",
        vec![
            PcbChange::create(n1, PcbObject::Net(Net { name: "A".into() })),
            PcbChange::create(n2, PcbObject::Net(Net { name: "B".into() })),
            PcbChange::create(t1, track(0, n1)),
            PcbChange::create(t2, track(400_000, n2)),
        ],
    ))
    .unwrap();

    ws.drc_run().unwrap();
    let snap = ws.snapshot();
    let drc = snap.drc.unwrap();
    assert!(!drc.stale);
    let v = drc
        .violations
        .iter()
        .find(|v| v.rule == "clearance")
        .unwrap();
    assert_eq!(v.severity, "error");

    ws.set_mode(Mode::Schematic);
    ws.drc_focus(v.index as usize).unwrap();
    let snap = ws.snapshot();
    assert_eq!(snap.mode, Mode::Pcb);
    assert_eq!(snap.selection.len(), 2, "both tracks are highlighted");

    // Editing the board makes the report stale rather than silently wrong.
    ws.open.as_mut().unwrap().engine.new_id();
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(PcbEditCommand::new(
        "d",
        vec![PcbChange::delete(t2, e.project().pcb().object(t2).unwrap())],
    ))
    .unwrap();
    assert!(ws.snapshot().drc.unwrap().stale);
    ws.drc_run().unwrap();
    assert!(ws.snapshot().drc.unwrap().violations.is_empty());
    assert!(ws.drc_focus(0).is_err());
}
