//! ButureScript end to end: source → diagnostics → EDA IR → project built through the Command
//! Engine (no GUI anywhere).
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_schematic::Connectivity;
use buturecad_scripting::{build, compile, CompileOptions, Severity};

fn opts() -> CompileOptions {
    CompileOptions::default()
}

fn errors(src: &str) -> Vec<String> {
    compile(src, &opts())
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}:{} {}", d.span.line, d.span.col, d.message))
        .collect()
}

const BASIC: &str = r#"
project "demo"
board { outline = rect(0mm, 0mm, 40mm, 30mm)  layers = 2 }

component C1 { value = "100nF"  footprint = "0402" }
component R1 { value = "10k"    footprint = "0402" }
component U1 { symbol = "Buture:C" value = "IC" footprint = "soic8" }   // 2-pin stand-in symbol

connect U1.1 -> C1.1
connect C1.2 -> GND
connect R1.1 -> C1.1
net VCC { R1.2, U1.2 }

place C1 at (10mm, 10mm)
place R1 at (20mm, 10mm) rotate 90
place U1 at (30mm, 15mm) side bottom
track VCC on F.Cu width 0.25mm path (20mm, 12mm) -> (30mm, 12mm)
via VCC at (25mm, 12mm)
zone GND on B.Cu { outline = rect(1mm, 1mm, 39mm, 29mm)  clearance = 0.3mm }
rule { clearance = 0.2mm  min_track_width = 0.15mm }
class Power { width = 0.5mm  nets = [VCC, GND] }
"#;

#[test]
fn a_script_builds_a_real_project_through_the_command_engine() {
    let mut b = build(BASIC, &opts(), None).unwrap_or_else(|e| panic!("{e:?}"));
    let p = b.engine.project();
    assert_eq!(p.name(), "demo");
    // Schematic: components and nets come from labels/power symbols on the pins.
    let conn = Connectivity::build(p.schematic());
    let net = |n: &str| conn.net_by_name(n).unwrap_or_else(|| panic!("net {n}"));
    let pins = |n: &str| {
        let mut v: Vec<String> = net(n)
            .pins
            .iter()
            .map(|p| format!("{}.{}", p.reference, p.pin))
            .collect();
        v.sort();
        v
    };
    assert_eq!(pins("GND"), ["C1.2"]);
    assert_eq!(pins("VCC"), ["R1.2", "U1.2"]);
    let auto = conn
        .nets()
        .iter()
        .find(|n| n.name.starts_with("N_"))
        .unwrap();
    let mut got: Vec<_> = auto
        .pins
        .iter()
        .map(|p| format!("{}.{}", p.reference, p.pin))
        .collect();
    got.sort();
    assert_eq!(
        got,
        ["C1.1", "R1.1", "U1.1"],
        "connect statements join one net"
    );
    // PCB: footprints, pad nets, placement, copper and zone.
    let pcb = p.pcb();
    assert_eq!(pcb.placed.len(), 3);
    let by_ref = |r: &str| pcb.placed.values().find(|f| f.reference == r).unwrap();
    assert_eq!(by_ref("C1").placement.position.x, 10_000_000);
    assert_eq!(by_ref("R1").placement.rotation, 1);
    assert!(by_ref("U1").placement.flipped);
    assert_eq!(
        by_ref("C1").pad_nets.len(),
        1,
        "only the pad of a net with two pads gets a net"
    );
    assert_eq!(pcb.tracks.len(), 1);
    assert_eq!(pcb.vias.len(), 1);
    assert_eq!(pcb.zones.len(), 1);
    assert!(
        pcb.zones.values().next().unwrap().fill.is_some(),
        "zones are filled"
    );
    assert_eq!(pcb.rules.min_track_width, 150_000);
    assert!(pcb.rules.classes.contains_key("Power"));
    assert_eq!(pcb.rules.assignments.len(), 2);
    assert!(
        buturecad_eda_core::outline::outline_loops(pcb).len() == 1,
        "closed board outline"
    );
    // Everything went through commands: undoing step by step leaves an empty project.
    let steps = b.engine.undo_history();
    assert!(
        steps.iter().any(|l| l.contains("ButureScript")) && steps.len() < 20,
        "{steps:?}"
    );
    while b.engine.can_undo() {
        b.engine.undo().unwrap();
    }
    assert!(
        b.engine.project().pcb().placed.is_empty()
            && b.engine.project().schematic().object_count() == 0
    );
}

#[test]
fn the_build_is_deterministic() {
    let a = build(BASIC, &opts(), None).unwrap();
    let b = build(BASIC, &opts(), None).unwrap();
    assert_eq!(a.engine.project(), b.engine.project());
    assert_eq!(a.design, b.design);
}

#[test]
fn diagnostics_carry_line_and_column() {
    let e = errors("component C1 { value = \"1\" }\nconnect C1.1 -> Z9.1\nconnect C1.7 -> GND\nplace Q1 at (1mm, 1mm)\n");
    assert!(
        e.iter()
            .any(|m| m.starts_with("2:") && m.contains("no component named “Z9”")),
        "{e:?}"
    );
    assert!(
        e.iter().any(|m| m.starts_with("3:")
            && m.contains("has no pin “7”")
            && m.contains("pins: 1, 2")),
        "{e:?}"
    );
    assert!(
        e.iter()
            .any(|m| m.starts_with("4:") && m.contains("no component named “Q1”")),
        "{e:?}"
    );
    let e = errors("component MCU { part = \"STM32\" }");
    assert!(e[0].contains("needs a `symbol`"), "{e:?}");
    let e = errors("net A { U1.1 }\nconnect A -> B\nnet B { C1.1 }\nconnect A -> B\ncomponent C1 { value = \"x\" }\ncomponent U1 { symbol = \"Buture:R\" }");
    assert!(e.iter().any(|m| m.contains("short circuit")), "{e:?}");
    // Warnings do not block.
    let out = compile("component R1 { value = \"1k\" }", &opts());
    assert!(out.design.is_some());
    assert!(out
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Warning && d.code == "W-unused"));
}

#[test]
fn dimensional_analysis_rejects_nonsense() {
    let e = errors("board { outline = rect(0mm, 0mm, 10mm + 5V, 10mm) }");
    assert!(
        e[0].contains("incompatible dimensions: length + voltage"),
        "{e:?}"
    );
    assert!(e[0].starts_with("1:"));
    // Lengths in other units add up exactly.
    let ok = compile(
        "board { outline = rect(0mm, 0mm, 1inch + 100mil + 1mm, 10um * 4) }",
        &opts(),
    );
    assert!(!ok.has_errors(), "{:?}", ok.diagnostics);
    let b = ok.design.unwrap().board.unwrap();
    assert_eq!(b.outline[2].x, 25_400_000 + 2_540_000 + 1_000_000);
    assert_eq!(b.outline[2].y, 40_000);
    for (src, want) in [
        (
            "let a = 1mm * 1mm\nboard { outline = rect(0mm,0mm,a,1mm) }",
            "needs lengths",
        ),
        ("let a = 5V < 3mA", "incompatible dimensions"),
        ("let a = 5furlong", "unknown unit"),
        ("let a = 1mm / 0", "division by zero"),
        (
            "board { outline = rect(0mm, 0mm, 0.0000001mm, 1mm) }",
            "whole number of nanometres",
        ),
        (
            "let r = 1kOhm\nlet v = 5V\nlet i = v / r\nlet bad = i + 1V",
            "incompatible dimensions: current + voltage",
        ),
    ] {
        let e = errors(src);
        assert!(e.iter().any(|m| m.contains(want)), "{src}: {e:?}");
    }
    // V / A is a resistance and can be compared with ohms.
    assert!(!compile("let ok = 5V / 2A > 1ohm\nlet r = 2kOhm + 500ohm", &opts()).has_errors());
}

#[test]
fn parametric_design_loops_functions_conditions_and_modules() {
    let src = r#"
param n = 4
param pitch = 2.54mm
fn x(i) = i * pitch + 5mm
module Decouple(port VDD, port GND, value = "100nF") {
  component C { value = value  footprint = "0402" }
  connect VDD -> C.1
  connect C.2 -> GND
}
board { outline = rect(0mm, 0mm, 50mm, 30mm) }
for i in 0..n {
  component LED[i] { value = "red" footprint = "0603" }
  component R[i] { value = "330" footprint = "0402" }
  connect R[i].2 -> LED[i].A
  connect LED[i].K -> GND
  connect R[i].1 -> "+3V3"
  place LED[i] at (x(i), 20mm)
  place R[i] at (x(i), 10mm)
  if i % 2 == 0 { use dec[i] = Decouple(VDD = "+3V3", GND = GND) } else { use dec[i] = Decouple(VDD = "+3V3", GND = GND, value = "1uF") }
}
"#;
    let b = build(src, &opts(), None).unwrap_or_else(|e| panic!("{e:?}"));
    let d = &b.design;
    assert_eq!(d.components.len(), 4 * 2 + 4);
    let c = |r: &str| d.components.iter().find(|c| c.reference == r).unwrap();
    assert_eq!(c("LED3").symbol, "Buture:LED");
    assert_eq!(c("dec1_C").value, "1uF");
    assert_eq!(c("dec0_C").value, "100nF");
    assert_eq!(
        d.placements
            .iter()
            .find(|p| p.reference == "LED3")
            .unwrap()
            .at
            .x,
        3 * 2_540_000 + 5_000_000
    );
    let gnd = d.nets.iter().find(|n| n.name == "GND").unwrap();
    assert_eq!(
        gnd.pins.len(),
        8,
        "4 LED cathodes + 4 module capacitors share GND through the port"
    );
    // Overriding a parameter changes the design.
    let mut o = opts();
    o.params.insert("n".into(), "2".into());
    o.params.insert("pitch".into(), "5mm".into());
    let b2 = build(src, &o, None).unwrap();
    assert_eq!(b2.design.components.len(), 2 * 2 + 2);
    assert_eq!(
        b2.design
            .placements
            .iter()
            .find(|p| p.reference == "LED1")
            .unwrap()
            .at
            .x,
        10_000_000
    );
    o.params.insert("n".into(), "5V".into());
    assert!(matches!(
        build(src, &o, None),
        Err(buturecad_scripting::BuildError::Script(_))
    ));
}

#[test]
fn runaway_scripts_are_stopped_and_bad_structure_is_reported() {
    let e = errors("fn f(a) = f(a)\nlet x = f(1)");
    assert!(e.iter().any(|m| m.contains("nest too deeply")), "{e:?}");
    let e = errors("module M() { use a = M() }\nuse m = M()");
    assert!(e.iter().any(|m| m.contains("nest too deeply")), "{e:?}");
    let e = errors("for i in 0..1000000 { let a = 1 }");
    assert!(e.iter().any(|m| m.contains("limit")), "{e:?}");
    let e = errors("for i in 0..100000 { for j in 0..100000 { let a = i } }");
    assert!(e.iter().any(|m| m.contains("evaluation steps")), "{e:?}");
    assert!(errors("module M(port A) { }\nuse m = M()")[0].contains("needs the port"));
    assert!(errors("use m = Nope()")[0].contains("unknown module"));
    assert!(errors("if 1 { }")[0].contains("must be a boolean"));
    assert!(
        errors("component R1 { value = \"1\" }\ncomponent R1 { value = \"2\" }")[0]
            .contains("already defined")
    );
    assert!(
        errors("track X on Z.Cu width 1mm path (0mm,0mm) -> (1mm,0mm)")[0]
            .contains("not a copper layer")
    );
    assert!(errors("component R1 { value = \"1\" }\nboard { outline = rect(0mm,0mm,9mm,9mm) }\nplace R1 at (1mm,1mm) rotate 45")
        .iter()
        .any(|m| m.contains("multiple of 90")));
}

#[test]
fn applying_to_a_used_project_or_with_unknown_pieces_fails_cleanly() {
    let e = build("component X1 { symbol = \"Buture:Nope\" }", &opts(), None);
    assert!(matches!(e, Err(buturecad_scripting::BuildError::Script(_))));
    // A footprint name the resolver cannot make is reported as a warning from the PCB sync, not a crash.
    let b = build("component R1 { value = \"1\" footprint = \"mystery\" }\ncomponent R2 { value = \"1\" footprint = \"0402\" }\nconnect R1.1 -> R2.1", &opts(), None).unwrap();
    assert!(!b.report.warnings.is_empty(), "{:?}", b.report);
}

/// Regressions found by the stage-44 benchmarks: automatic layouts used to run out of coordinate
/// range at about 500 schematic parts and about 2000 board footprints.
#[test]
fn large_designs_stay_inside_the_coordinate_range() {
    for n in [520usize, 2100] {
        let src = format!(
            "for i in 0..{n} {{ component R[i] {{ value = \"1k\" footprint = \"0603\" }} }}"
        );
        let b = build(&src, &opts(), None).unwrap_or_else(|e| panic!("{n} components: {e:?}"));
        assert_eq!(b.engine.project().pcb().placed.len(), n);
        let max = b
            .engine
            .project()
            .pcb()
            .placed
            .values()
            .map(|f| {
                f.placement
                    .position
                    .x
                    .abs()
                    .max(f.placement.position.y.abs())
            })
            .max()
            .unwrap();
        assert!(max < 1 << 30, "footprints stay inside ±1.07 m ({max})");
    }
}

#[test]
fn absurd_coordinates_are_refused_not_overflowed() {
    for bad in [
        "board { outline = rect(0mm, 0mm, 5000mm, 10mm) }",
        "component R1 { value = \"1\" footprint = \"0603\" }\nplace R1 at (2000mm, 0mm)",
    ] {
        assert!(
            errors(bad)
                .iter()
                .any(|m| m.contains("out of range") || m.contains("range")),
            "{bad}: {:?}",
            errors(bad)
        );
    }
}

/// Found by the stage-47 validation board: `zone "+3V3" …` (a power plane) was a syntax error
/// although `connect "+3V3" -> …` worked.
#[test]
fn quoted_net_names_work_in_zone_track_via_and_net_statements() {
    let src = r#"
project "p"
board { outline = rect(0mm, 0mm, 20mm, 20mm) layers = 4 }
component R1 { value = "1k" footprint = "0603" }
component R2 { value = "1k" footprint = "0603" }
net "+3V3" { R1.1, R2.1 }
connect R1.2 -> GND
connect R2.2 -> GND
place R1 at (5mm, 10mm)
place R2 at (9mm, 10mm)
via "+3V3" at (7mm, 6mm)
track "+3V3" on F.Cu width 0.25mm path (5mm, 10mm) -> (6mm, 10mm)
zone "+3V3" on In2.Cu { outline = rect(1mm, 1mm, 19mm, 19mm) }
"#;
    let b = build(src, &opts(), None).unwrap_or_else(|e| panic!("{e:?}"));
    let pcb = b.engine.project().pcb();
    assert_eq!(
        (pcb.vias.len(), pcb.tracks.len(), pcb.zones.len()),
        (1, 1, 1)
    );
}

#[test]
fn syntax_errors_name_the_symbol_found_not_an_internal_token() {
    for (src, want) in [
        ("project \"x\"\nlet a = 1 + * 2\n", "found “*”"),
        ("project \"x\"\nconnect A.1 -> * B.2\n", "found “*”"),
        ("project \"x\"\nlet b = (1 }\n", "found “}”"),
    ] {
        let e = errors(src);
        assert!(e.iter().any(|m| m.contains(want)), "{src}: {e:?}");
        assert!(
            e.iter()
                .all(|m| !m.contains("Star") && !m.contains("RBrace")),
            "{e:?}"
        );
    }
}

/// Pins named like `V+`, `D-` or `-IN1` are written quoted after the dot (found by the stage-47
/// reference board and the stage-51 standard library).
#[test]
fn quoted_pin_names_connect_pins_whose_names_are_not_identifiers() {
    let src = r#"
project "p"
component J1 { symbol = "Std:USB_Micro_B" value = "usb" }
component R1 { value = "1k" }
connect J1."D+" -> USB_DP
connect USB_DP -> R1.1
connect J1."D-" -> USB_DM
connect J1.GND -> GND
connect R1.2 -> GND
"#;
    let std_syms = buturecad_libraries::standard::symbols();
    let o = CompileOptions {
        symbols: std_syms,
        ..Default::default()
    };
    let out = compile(src, &o);
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let d = out.design.unwrap();
    let net = |n: &str| d.nets.iter().find(|x| x.name == n).unwrap();
    assert!(net("USB_DP")
        .pins
        .iter()
        .any(|p| p.reference == "J1" && p.pin == "D+"));
    assert!(net("USB_DM")
        .pins
        .iter()
        .any(|p| p.reference == "J1" && p.pin == "D-"));
    // A quoted name that does not exist is still an error with the symbol's pins listed.
    let bad = src.replace("J1.\"D+\"", "J1.\"X+\"");
    assert!(compile(&bad, &o).has_errors());
}
