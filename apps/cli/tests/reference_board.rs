//! The validation board of roadmap stage 47: a 4-layer STM32 USB board (examples/reference-board)
//! taken through the whole headless flow with the committed, routed project.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

fn board_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/reference-board")
}

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

fn json(args: &[&str]) -> (i32, serde_json::Value) {
    let mut a = args.to_vec();
    a.push("--json");
    let (c, out, err) = run(&a);
    (
        c,
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}{err}")),
    )
}

/// A private copy of the committed project (opening a project writes session state).
fn project_copy(d: &Path) -> String {
    let dst = d.join("project");
    copy_dir(&board_dir().join("project"), &dst);
    dst.to_str().unwrap().to_owned()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

#[test]
fn the_script_builds_with_its_libraries_and_the_committed_project_is_that_script() {
    let b = board_dir();
    let script = b.join("board.beda");
    let (c, out, err) = run(&["check", script.to_str().unwrap()]);
    assert_eq!(c, 0, "{out}{err}");
    // Libraries next to the script are found automatically.
    let d = tempfile::tempdir().unwrap();
    let out_dir = d.path().join("p");
    let (c, out, err) = run(&[
        "build",
        script.to_str().unwrap(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(c, 0, "{out}{err}");
    assert!(out.contains("25 components, 14 nets"), "{out}");
    // Connectivity is exactly what the script says (found by this board: an automatic layout once
    // put pins of the big MCU symbol on the net markers of neighbouring parts).
    let (_, nets) = json(&["inspect", "nets", out_dir.to_str().unwrap()]);
    let pins_of = |name: &str| -> Vec<String> {
        let n = nets["nets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == name)
            .unwrap();
        let mut v: Vec<String> = n["pins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().to_owned())
            .collect();
        v.sort();
        v
    };
    let gnd = pins_of("GND");
    for want in [
        "U1.8", "U1.23", "U1.35", "U1.47", "U1.49", "U2.2", "J1.5", "J1.6", "J2.4", "FLG1.1",
    ] {
        assert!(
            gnd.iter().any(|p| p == want),
            "{want} missing from GND: {gnd:?}"
        );
    }
    let u1_on_gnd = gnd.iter().filter(|p| p.starts_with("U1.")).count();
    assert_eq!(
        u1_on_gnd, 5,
        "only the five VSS pins of U1 are ground: {gnd:?}"
    );
    assert_eq!(
        pins_of("+3V3")
            .iter()
            .filter(|p| p.starts_with("U1."))
            .count(),
        5
    );
    // The committed project was generated from the same script.
    assert_eq!(
        std::fs::read(b.join("project/design.beda")).unwrap(),
        std::fs::read(&script).unwrap()
    );
}

#[test]
fn the_routed_board_passes_drc_and_erc_reports_only_the_known_findings() {
    let d = tempfile::tempdir().unwrap();
    let p = project_copy(d.path());
    let (c, drc) = json(&["drc", &p]);
    assert_eq!(
        (c, drc["errors"].as_i64(), drc["warnings"].as_i64()),
        (0, Some(0), Some(0)),
        "{drc}"
    );
    // Zones are current (nothing to refill).
    let (_, fill) = json(&["fill", &p]);
    assert_eq!(fill["zones"], 2);

    // ERC: every error is an unused I/O pin (no way to mark a pin "no connect" from a script yet,
    // see docs/validation-project.md, MAJOR-2), plus the PWR_FLAG without a footprint.
    let (c, erc) = json(&["erc", &p]);
    assert_eq!(c, 1);
    let v = erc["violations"].as_array().unwrap();
    let errors: Vec<_> = v.iter().filter(|x| x["severity"] == "error").collect();
    assert!(
        errors.iter().all(|x| x["rule"] == "unconnected_pin"),
        "{errors:?}"
    );
    assert_eq!(errors.len(), 31);
    assert!(v.iter().any(
        |x| x["rule"] == "missing_footprint" && x["message"].as_str().unwrap().contains("FLG1")
    ));
}

#[test]
fn production_files_are_complete_and_read_back_consistently() {
    let d = tempfile::tempdir().unwrap();
    let p = project_copy(d.path());
    let fab = d.path().join("fab");
    let (c, out, err) = run(&["export-gerber", &p, "--out", fab.to_str().unwrap()]);
    assert_eq!(c, 0, "{out}{err}");
    let name = |s: &str| fab.join(format!("reference-mcu-board-{s}"));
    for f in [
        "F_Cu.gtl",
        "In1_Cu.g1",
        "In2_Cu.g2",
        "B_Cu.gbl",
        "F_Mask.gts",
        "B_Mask.gbs",
        "F_Paste.gtp",
        "F_Silkscreen.gto",
        "Edge_Cuts.gko",
        "PTH.drl",
        "bom.csv",
        "pos.csv",
    ] {
        assert!(name(f).exists(), "{f}");
    }
    // Every Gerber parses with the independent reader and has no warnings.
    for f in [
        "F_Cu.gtl",
        "In1_Cu.g1",
        "In2_Cu.g2",
        "B_Cu.gbl",
        "F_Mask.gts",
        "B_Mask.gbs",
        "F_Paste.gtp",
        "Edge_Cuts.gko",
    ] {
        let text = std::fs::read_to_string(name(f)).unwrap();
        let img = buturecad_manufacturing::gerber_read::parse(&text)
            .unwrap_or_else(|e| panic!("{f}: {e:?}"));
        assert!(img.warnings.is_empty(), "{f}: {:?}", img.warnings);
        assert!(!img.prims.is_empty(), "{f} is empty");
    }
    // Drill hits = vias + plated pads of the board.
    let drill = buturecad_manufacturing::excellon_read::parse(
        &std::fs::read_to_string(name("PTH.drl")).unwrap(),
    )
    .unwrap();
    let (_, project) = {
        let o = buturecad_io::ProjectSession::open(&p).unwrap();
        (o.session, o.project)
    };
    let doc = project.pcb();
    let through_pads: usize = doc
        .placed
        .values()
        .map(|f| {
            let def = &doc.footprints[&f.footprint];
            f.world_pads(def)
                .iter()
                .filter(|w| w.kind == buturecad_pcb::PadKind::Through)
                .count()
        })
        .sum();
    assert_eq!(drill.hits.len(), doc.vias.len() + through_pads);
    // BOM: 24 parts on 16 lines, FLG1 (no footprint) is not in it; the position file has the 23 SMD parts.
    let bom = std::fs::read_to_string(name("bom.csv")).unwrap();
    assert_eq!(bom.lines().count(), 1 + 16);
    assert!(!bom.contains("FLG1"));
    let pos = std::fs::read_to_string(name("pos.csv")).unwrap();
    assert_eq!(pos.lines().count(), 1 + 23);
    assert!(
        pos.contains("U1,") && !pos.contains("J2,"),
        "through-hole J2 is not placed by machine"
    );
}
