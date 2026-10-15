//! The built-in `Std` library is available to scripts without any library folder.
#![allow(clippy::unwrap_used, clippy::panic)]

fn run(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = buturecad_cli::run(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

const SCRIPT: &str = r#"
project "std-demo"
board { outline = rect(0mm, 0mm, 40mm, 30mm) layers = 2 }
component U1 { symbol = "Std:OpAmp_Dual_8" value = "LM358" footprint = "Std:SOIC-8_3.9mm_P1.27mm" mpn = "LM358DR" }
component U2 { symbol = "Std:Regulator_Linear_3" value = "AMS1117-3.3" footprint = "Std:SOT-223-3" }
component Q1 { symbol = "Std:Q_NMOS_GSD" value = "2N7002" footprint = "Std:SOT-23-3" }
component J1 { symbol = "Std:Conn_01x04" value = "Header" footprint = "Std:PinHeader_1x04_P2.54mm" }
connect U1.OUT1 -> U1."-IN1"
connect U1."V+" -> "+5V"
connect U2.VIN -> "+5V"
connect U2.VOUT -> "+3V3"
connect GND -> U1."V-", U2.GND, Q1.S
connect Q1.G -> J1.1
connect Q1.D -> J1.2
place U1 at (10mm, 15mm)
place U2 at (25mm, 15mm)
place Q1 at (32mm, 8mm)
place J1 at (32mm, 22mm)
"#;

#[test]
fn std_symbols_and_footprints_work_in_a_script_with_no_library_folder() {
    let d = tempfile::tempdir().unwrap();
    let script = d.path().join("demo.beda");
    std::fs::write(&script, SCRIPT).unwrap();
    let (c, out, err) = run(&["check", script.to_str().unwrap()]);
    assert_eq!(c, 0, "{out}{err}");
    let proj = d.path().join("p");
    let (c, out, err) = run(&[
        "build",
        script.to_str().unwrap(),
        "--out",
        proj.to_str().unwrap(),
    ]);
    assert_eq!(c, 0, "{out}{err}");
    // The footprints landed on the board with the right pad counts.
    let o = buturecad_io::ProjectSession::open(&proj).unwrap();
    let doc = o.project.pcb();
    let pads = |reference: &str| -> usize {
        let f = doc
            .placed
            .values()
            .find(|f| f.reference == reference)
            .unwrap();
        doc.footprints[&f.footprint].pads.len()
    };
    assert_eq!(
        (pads("U1"), pads("U2"), pads("Q1"), pads("J1")),
        (8, 4, 3, 4)
    );
    // An unknown Std symbol is a clear error, not a silent fallback.
    std::fs::write(&script, SCRIPT.replace("Std:Q_NMOS_GSD", "Std:Q_Nope")).unwrap();
    let (c, out, err) = run(&[
        "build",
        script.to_str().unwrap(),
        "--out",
        d.path().join("q").to_str().unwrap(),
    ]);
    assert_eq!(c, 1, "{out}{err}");
    assert!(format!("{out}{err}").contains("Std:Q_Nope"), "{out}{err}");
}
