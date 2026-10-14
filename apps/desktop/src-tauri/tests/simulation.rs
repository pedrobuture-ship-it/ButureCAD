//! Simulation through the workspace: the schematic's connectivity becomes the netlist, the
//! built-in engine runs it, results come back plotted-ready, problems are reported.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::{Mode, SimulationInfo};
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{builtin, Component, Object, Placement, PowerSymbol};

const G: i64 = 1_270_000;

fn pt(x: i64, y: i64) -> Point {
    Point::new(x * G, y * G)
}

fn comp(reference: &str, lib: &str, value: &str, x: i64, y: i64, rotation: u8) -> Component {
    Component {
        lib_id: lib.into(),
        reference: reference.into(),
        value: value.into(),
        footprint: String::new(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement {
            position: pt(x, y),
            rotation,
            mirror: false,
        },
    }
}

/// V1 --- R1 --- C1 low-pass with GND under V1 and C1. R = 1k, C = 1u (fc ≈ 159 Hz).
fn rc_project(dir: &std::path::Path, with_ground: bool) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "RC").unwrap();
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    for c in [
        comp("V1", "Buture:V", "AC 1", 10, 10, 0),
        comp("R1", "Buture:R", "1k", 14, 13, 1),
        comp("C1", "Buture:C", "1u", 19, 10, 0),
    ] {
        let id = e.new_id();
        e.execute(place_component(ROOT_SHEET, id, c)).unwrap();
    }
    for (a, b) in [
        ((10, 13), (11, 13)),
        ((17, 13), (19, 13)),
        ((10, 7), (19, 7)),
    ] {
        let id = e.new_id();
        e.execute(create_wire(
            ROOT_SHEET,
            id,
            Segment::new(pt(a.0, a.1), pt(b.0, b.1)),
        ))
        .unwrap();
    }
    if with_ground {
        let id = e.new_id();
        e.execute(create_object(
            "ground",
            ROOT_SHEET,
            id,
            Object::PowerSymbol(PowerSymbol {
                lib_id: "Buture:GND".into(),
                net_name: "GND".into(),
                placement: Placement::at(pt(10, 7)),
            }),
        ))
        .unwrap();
    }
    ws.set_mode(Mode::Simulation);
    ws
}

fn info(ws: &mut Workspace) -> SimulationInfo {
    ws.snapshot().simulation.unwrap()
}

#[test]
fn ac_analysis_of_the_schematic_matches_the_analytic_lowpass() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = rc_project(dir.path(), true);
    let i = info(&mut ws);
    assert!(i.problems.is_empty(), "{:?}", i.problems);
    assert!(i.netlist.contains("V1") && i.netlist.contains("R1") && i.netlist.contains("C1"));
    assert!(i.backends.iter().any(|b| b.id == "builtin" && b.available));
    ws.simulation_command("set", Some("analysis=ac")).unwrap();
    ws.simulation_command("set", Some("fstart=10")).unwrap();
    ws.simulation_command("set", Some("fstop=10k")).unwrap();
    ws.simulation_command("run", None).unwrap();
    let r = info(&mut ws).result.unwrap();
    assert!(r.x_log && !r.stale);
    // The output node is the one that rolls off: last magnitude is far below the first.
    let rolled = r
        .series
        .iter()
        .filter(|s| s.name.starts_with("mag("))
        .any(|s| s.values[0] > -0.1 && *s.values.last().unwrap() < -30.0);
    assert!(
        rolled,
        "{:?}",
        r.series.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    // At the corner frequency (≈159.15 Hz) the response is −3.01 dB.
    let s = r
        .series
        .iter()
        .find(|s| {
            s.name.starts_with("mag(") && s.values[0] > -0.1 && *s.values.last().unwrap() < -30.0
        })
        .unwrap();
    let k = r.x.iter().position(|f| *f >= 158.0).unwrap();
    let w = r.x[k] / 159.1549;
    let want = -10.0 * (1.0 + w * w).log10();
    assert!(
        (s.values[k] - want).abs() < 0.05,
        "{} vs {want}",
        s.values[k]
    );
    // Editing the schematic marks the result stale.
    ws.rename_project("Other").unwrap();
    assert!(info(&mut ws).result.unwrap().stale);
}

#[test]
fn transient_and_op_of_the_schematic() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = rc_project(dir.path(), true);
    ws.simulation_command("run", None).unwrap();
    let r = info(&mut ws).result.unwrap();
    assert_eq!(r.analysis, "Operating point");
    assert!(r.series.iter().any(|s| s.name == "i(V1)"));
    ws.simulation_command("set", Some("analysis=tran")).unwrap();
    ws.simulation_command("set", Some("tstep=20u")).unwrap();
    ws.simulation_command("set", Some("tstop=2m")).unwrap();
    ws.simulation_command("run", None).unwrap();
    let r = info(&mut ws).result.unwrap();
    assert_eq!(r.x_label, "time");
    assert_eq!(r.points, 101);
    assert!(!r.decimated);
}

#[test]
fn problems_are_reported_and_block_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = rc_project(dir.path(), false);
    let i = info(&mut ws);
    assert!(
        i.problems.iter().any(|p| p.contains("ground")),
        "{:?}",
        i.problems
    );
    assert!(ws.simulation_command("run", None).is_err());
    // A diode without a model is a problem, with a reference to the property to set.
    let e = &mut ws.open.as_mut().unwrap().engine;
    let id = e.new_id();
    e.execute(place_component(
        ROOT_SHEET,
        id,
        comp("D1", "Buture:D", "", 25, 10, 0),
    ))
    .unwrap();
    let i = info(&mut ws);
    assert!(
        i.problems
            .iter()
            .any(|p| p.contains("D1") && p.contains("Spice_Model")),
        "{:?}",
        i.problems
    );
    assert!(ws
        .simulation_command("set", Some("analysis=bogus"))
        .is_err());
    assert!(ws.simulation_command("set", Some("backend=nope")).is_err());
    // ngspice is selectable but reports when it is not installed.
    ws.simulation_command("set", Some("backend=ngspice"))
        .unwrap();
}

#[test]
fn spice_models_are_associated_through_component_properties() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = rc_project(dir.path(), true);
    let e = &mut ws.open.as_mut().unwrap().engine;
    // Find R1 and give it a value override through the normal property editing path.
    let r1 = {
        let sch = e.project().schematic();
        sch.sheets().values().next().map(|_| ()).unwrap();
        let mut found = None;
        for id in 1..64u64 {
            if let Some((_, Object::Component(c))) = sch.object(buturecad_schematic::Id(id)) {
                if c.reference == "R1" {
                    found = Some(id);
                }
            }
        }
        found.unwrap()
    };
    ws.set_mode(Mode::Schematic);
    ws.set_property(r1, "Spice_Value", "2k").unwrap();
    ws.set_property(r1, "Spice_Model", ".model X D(IS=1e-14)")
        .unwrap();
    ws.set_mode(Mode::Simulation);
    let i = info(&mut ws);
    assert!(
        i.netlist.contains("R1") && i.netlist.contains("2e3"),
        "{}",
        i.netlist
    );
    // The Inspector shows the associations; clearing a property removes it.
    ws.set_mode(Mode::Schematic);
    ws.set_property(r1, "Spice_Value", "").unwrap();
    ws.set_mode(Mode::Simulation);
    assert!(info(&mut ws).netlist.contains("1e3"));
    assert!(ws.set_property(r1, "Spice_Value", "x\u{7}y").is_err());
}
