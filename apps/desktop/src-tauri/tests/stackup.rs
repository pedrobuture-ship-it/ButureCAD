//! The Layer Stack Manager through the workspace.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net, Zone};

fn ws(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::new(None).with_user_libraries(dir.join("user-libs"));
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_mode(Mode::Pcb);
    ws
}

fn thickness(ws: &Workspace) -> i64 {
    ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .physical
        .thickness()
}

#[test]
fn the_manager_shows_the_real_stack_and_offers_templates() {
    let dir = tempfile::tempdir().unwrap();
    let w = ws(dir.path());
    let info = w.stackup_info().unwrap();
    assert_eq!(
        (info.copper_layers, info.copper.len(), info.gaps.len()),
        (2, 2, 1)
    );
    assert_eq!(info.copper[0].layer, "F.Cu");
    assert_eq!(info.copper[0].thickness, "0.035");
    assert_eq!(info.gaps[0][0].kind, "core");
    assert!(info.top_mask.is_some() && info.bottom_mask.is_some());
    let total: f64 = info.total_thickness.parse().unwrap();
    assert!((total - 1.6).abs() < 0.1, "{total}");
    assert!(info.templates.iter().any(|t| t.name.contains("HDI")));
    assert!(info.templates.iter().any(|t| t.name.contains("Rigid-flex")));
    let four = w.stackup_template("", 4).unwrap();
    assert_eq!((four.copper.len(), four.gaps.len()), (4, 3));
    assert_eq!(four.copper[1].layer, "In1.Cu");
    assert!(w.stackup_template("", 3).is_err());
    let hdi = w
        .stackup_template("HDI 1+4+1 (micro-via build-up)", 0)
        .unwrap();
    assert_eq!(hdi.copper_layers, 6);
    assert_eq!(hdi.gaps[0][0].thickness, "0.06");
}

#[test]
fn editing_the_stack_is_one_undo_step_and_invalid_edits_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = ws(dir.path());
    let before = thickness(&w);
    let mut info = w.stackup_info().unwrap();
    info.copper[0].thickness = "0.07".into();
    info.gaps[0][0].thickness = "1.2".into();
    info.gaps[0][0].material = "Isola 370HR".into();
    info.gaps[0][0].er = "4.04".into();
    info.finish = "ENIG".into();
    w.stackup_apply(info.clone()).unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.physical.copper[&layer::F_CU].thickness, 70_000);
    assert_eq!(doc.physical.gaps[0][0].thickness, 1_200_000);
    assert_eq!(doc.physical.gaps[0][0].er_milli, 4_040);
    assert_eq!(doc.physical.finish, "ENIG");
    assert!(w
        .snapshot()
        .project
        .unwrap()
        .undo_history
        .iter()
        .any(|h| h == "Edit layer stack"));
    w.undo().unwrap();
    assert_eq!(thickness(&w), before);

    for bad in [
        |i: &mut StackupInfo| i.gaps[0].clear(),
        |i: &mut StackupInfo| i.gaps[0][0].thickness = "0".into(),
        |i: &mut StackupInfo| i.copper[0].thickness = "abc".into(),
        |i: &mut StackupInfo| i.gaps[0][0].kind = "wood".into(),
        |i: &mut StackupInfo| i.copper[1].role = "magic".into(),
        |i: &mut StackupInfo| i.gaps[0][0].material = " ".into(),
    ] {
        let mut b = info.clone();
        bad(&mut b);
        assert!(w.stackup_apply(b).is_err());
        assert_eq!(
            thickness(&w),
            before,
            "a refused edit leaves the stack alone"
        );
    }
}

#[test]
fn changing_the_layer_count_with_a_template_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = ws(dir.path());
    let original = w.stackup_info().unwrap();
    let hdi = w
        .stackup_template("HDI 1+4+1 (micro-via build-up)", 0)
        .unwrap();
    w.stackup_apply(hdi).unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.stackup.copper_layers, 6);
    assert_eq!(doc.physical.name, "HDI 1+4+1 (micro-via build-up)");
    assert_eq!(doc.physical.gaps.len(), 5);
    let history = w.snapshot().project.unwrap().undo_history;
    assert_eq!(
        history
            .iter()
            .filter(|h| h.as_str() == "Edit layer stack")
            .count(),
        1
    );
    w.undo().unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.stackup.copper_layers, 2);
    assert_eq!(
        w.stackup_info().unwrap().total_thickness,
        original.total_thickness
    );
    // The bar's plain layer-count selector also gives a matching standard stack.
    w.pcb_command("set_stackup", Some("4")).unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.physical.copper.len(), 4);
    doc.physical.validate(&doc.stackup).unwrap();
    w.undo().unwrap();
    assert_eq!(
        w.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .physical
            .copper
            .len(),
        2
    );
}

#[test]
fn presets_are_saved_listed_loaded_and_deleted_locally() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = ws(dir.path());
    let mut info = w.stackup_template("", 6).unwrap();
    info.gaps[0][0].material = "Megtron 6".into();
    w.stackup_preset_save("user", "Fast boards", info.clone())
        .unwrap();
    w.stackup_preset_save("project", "Project 6L", info)
        .unwrap();
    let listed = w.stackup_info().unwrap().presets;
    let names: Vec<_> = listed
        .iter()
        .map(|p| (p.scope.as_str(), p.name.as_str(), p.copper_layers))
        .collect();
    assert!(
        names.contains(&("user", "Fast boards", 6))
            && names.contains(&("project", "Project 6L", 6)),
        "{names:?}"
    );
    let loaded = w.stackup_preset_load("user", "Fast boards").unwrap();
    assert_eq!(loaded.copper_layers, 6);
    assert_eq!(loaded.gaps[0][0].material, "Megtron 6");
    w.stackup_apply(loaded).unwrap();
    assert_eq!(
        w.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .stackup
            .copper_layers,
        6
    );
    assert!(w
        .stackup_preset_save("user", "../escape", w.stackup_info().unwrap())
        .is_err());
    w.stackup_preset_delete("user", "Fast boards").unwrap();
    assert!(w.stackup_preset_load("user", "Fast boards").is_err());
    assert!(dir
        .path()
        .join("P/libraries/Project 6L.bstack.json")
        .exists());
}

#[test]
fn swapping_inner_layers_moves_the_design_and_is_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = ws(dir.path());
    w.pcb_command("set_stackup", Some("4")).unwrap();
    let (a, b) = (layer::inner_cu(1), layer::inner_cu(2));
    let e = &mut w.open.as_mut().unwrap().engine;
    let (net, t, z) = (e.new_id(), e.new_id(), e.new_id());
    e.execute(PcbEditCommand::new(
        "x",
        vec![
            PcbChange::create(net, PcbObject::Net(Net { name: "N".into() })),
            PcbChange::create(
                t,
                PcbObject::Track(Track {
                    layer: a,
                    segment: Segment::new(Point::ORIGIN, Point::new(5_000_000, 0)),
                    width: 250_000,
                    net: Some(net),
                }),
            ),
            PcbChange::create(
                z,
                PcbObject::Zone(Zone::new(
                    b,
                    vec![
                        Point::ORIGIN,
                        Point::new(9_000_000, 0),
                        Point::new(9_000_000, 9_000_000),
                    ],
                    Some(net),
                )),
            ),
        ],
    ))
    .unwrap();
    w.stackup_swap_inner(a, b).unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.tracks[&t].layer, b);
    assert_eq!(doc.zones[&z].layer, a);
    assert!(w.stackup_swap_inner(a, a).is_err());
    assert!(
        w.stackup_swap_inner(layer::F_CU, a).is_err(),
        "outer layers cannot be swapped"
    );
    w.undo().unwrap();
    let doc = w.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!((doc.tracks[&t].layer, doc.zones[&z].layer), (a, b));
}
