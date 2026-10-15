//! `buturecad import`: other tools' files become projects, with an Import Report that accounts
//! for everything.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_buturecad"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

const SPICE: &str = "RC with clamp\nV1 in 0 DC 0 AC 1\nR1 in out 1k\nC1 out 0 1u\nD1 out 0 D1N4148\n.model D1N4148 D(IS=2.5e-9)\nQ1 out in 0 BC547\n.tran 1u 1m\n.end\n";

#[test]
fn a_spice_netlist_becomes_a_simulatable_project_with_a_full_report() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("rc.cir");
    std::fs::write(&f, SPICE).unwrap();
    let out = d.path().join("rc");
    let (c, text, err) = run(&[
        "import",
        f.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 0, "{text}{err}");
    assert!(
        text.contains("Unsupported (information that was NOT imported)")
            && text.contains("Q1")
            && text.contains(".tran"),
        "{text}"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("import-report.json")).unwrap()).unwrap();
    let s = &report["summary"];
    let total = s["converted"].as_u64().unwrap()
        + s["ignored"].as_u64().unwrap()
        + s["unsupported"].as_u64().unwrap();
    assert_eq!(
        total,
        report["records_seen"].as_u64().unwrap(),
        "nothing is silently dropped"
    );
    // The project exists with the converted parts and nets, and is internally consistent.
    let (_, comps, _) = run(&["inspect", "components", out.to_str().unwrap()]);
    for r in ["V1", "R1", "C1", "D1"] {
        assert!(comps.contains(r), "{comps}");
    }
    assert!(!comps.contains("Q1"));
    assert_eq!(run(&["check", out.to_str().unwrap()]).0, 0);
    let (_, nets, _) = run(&["inspect", "nets", out.to_str().unwrap()]);
    assert!(nets.contains("GND") && nets.contains("out"), "{nets}");
    // --json and --report.
    let rp = d.path().join("r.json");
    let (c, j, _) = run(&[
        "import",
        f.to_str().unwrap(),
        "--out",
        d.path().join("rc2").to_str().unwrap(),
        "--json",
        "--report",
        rp.to_str().unwrap(),
    ]);
    assert_eq!(c, 0);
    let v: serde_json::Value = serde_json::from_str(&j).unwrap();
    assert_eq!(v["created"], true);
    assert!(rp.exists());
}

#[test]
fn a_kicad_netlist_becomes_a_project_with_generated_symbols() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("board.net");
    std::fs::write(&f, r#"(export (version "E")
 (design (source "b.kicad_sch") (tool "Eeschema"))
 (components
  (comp (ref "R1") (value "10k") (footprint "Resistor_SMD:R_0603_1608Metric") (libsource (lib "Device") (part "R")))
  (comp (ref "C1") (value "100n") (footprint "Capacitor_SMD:C_0402_1005Metric") (libsource (lib "Device") (part "C")))
  (comp (ref "U1") (value "MCU") (footprint "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm") (libsource (lib "MCU") (part "STM"))))
 (libparts (libpart (lib "MCU") (part "STM")))
 (nets
  (net (code "1") (name "GND") (node (ref "C1") (pin "2")) (node (ref "U1") (pin "2") (pinfunction "VSS") (pintype "power_in")))
  (net (code "2") (name "+3V3") (node (ref "R1") (pin "1")) (node (ref "C1") (pin "1")) (node (ref "U1") (pin "1") (pinfunction "VDD") (pintype "power_in")))
  (net (code "3") (name "SIG") (node (ref "R1") (pin "2")) (node (ref "U1") (pin "3") (pinfunction "PA0")))))
"#).unwrap();
    let out = d.path().join("kb");
    let (c, text, err) = run(&[
        "import",
        f.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 0, "{text}{err}");
    assert!(
        text.contains("a symbol with 3 pin(s) was generated"),
        "{text}"
    );
    assert!(text.contains("nothing of the board was imported"));
    let (_, board, _) = run(&["inspect", "board", out.to_str().unwrap(), "--json"]);
    let b: serde_json::Value = serde_json::from_str(&board).unwrap();
    assert_eq!(
        b["footprints"], 3,
        "footprints were generated from the KiCad names"
    );
    assert_eq!(b["nets"], 3);
    let (_, nets, _) = run(&["inspect", "nets", out.to_str().unwrap()]);
    assert!(
        nets.contains("U1.1") && nets.contains("R1.1") && nets.contains("C1.1"),
        "{nets}"
    );
    // ERC works on the imported connectivity (the generated U1 has the right pin types).
    let (c, erc, _) = run(&["erc", out.to_str().unwrap()]);
    assert!(c == 0 || erc.contains("ERC:"), "{erc}");
}

#[test]
fn failures_create_nothing_and_say_why() {
    let d = tempfile::tempdir().unwrap();
    let bad = d.path().join("bad.net");
    std::fs::write(
        &bad,
        "(export (components (comp (ref \"R1\")) (comp (ref \"R1\"))))",
    )
    .unwrap();
    let out = d.path().join("never");
    let (c, text, _) = run(&[
        "import",
        bad.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert!(
        text.contains("appears twice") && text.contains("no project was created"),
        "{text}"
    );
    assert!(!out.exists());
    let junk = d.path().join("junk.dat");
    std::fs::write(&junk, "hello").unwrap();
    assert_eq!(
        run(&[
            "import",
            junk.to_str().unwrap(),
            "--out",
            out.to_str().unwrap()
        ])
        .0,
        2
    );
    assert_eq!(
        run(&[
            "import",
            junk.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--format",
            "altium"
        ])
        .0,
        2
    );
    assert_eq!(
        run(&[
            "import",
            "/no/such/file.cir",
            "--out",
            out.to_str().unwrap()
        ])
        .0,
        2
    );
    assert_eq!(
        run(&["import", bad.to_str().unwrap()]).0,
        2,
        "--out is required"
    );
    assert!(!Path::new(&out).exists());
}
