//! The CLI and the headless pipeline: ButureScript → parser → core → schematic → PCB → ERC → DRC →
//! Gerber, with no GUI crate anywhere in the dependency tree.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
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
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}\n{err}")),
    )
}

fn build_chain(dir: &Path) -> String {
    let proj = dir.join("chain");
    let p = proj.to_str().unwrap().to_owned();
    let (c, out, err) = run(&[
        "build",
        example("chain.beda").to_str().unwrap(),
        "--out",
        &p,
    ]);
    assert_eq!(c, 0, "{out}{err}");
    assert!(out.contains("3 components, 4 nets, 2 tracks"), "{out}");
    p
}

#[test]
fn the_whole_flow_runs_headless_from_script_to_gerber() {
    let d = tempfile::tempdir().unwrap();
    let p = build_chain(d.path());
    assert!(Path::new(&p).join("design.beda").exists());
    // check (project and script), ERC and DRC.
    assert_eq!(run(&["check", &p]).0, 0);
    assert_eq!(
        run(&["check", example("chain.beda").to_str().unwrap()]).0,
        0
    );
    let (c, erc) = json(&["erc", &p]);
    assert_eq!(c, 0, "{erc}");
    let (c, drc) = json(&["drc", &p]);
    assert_eq!((c, drc["errors"].as_u64()), (0, Some(0)), "{drc}");
    // Gerber / drills / BOM / pick and place into a zip, which the independent readers parse back.
    let zip = d.path().join("production.zip");
    let (c, g) = json(&["export-gerber", &p, "--out", zip.to_str().unwrap()]);
    assert_eq!(c, 0, "{g}");
    let files = buturecad_manufacturing::zip::read_stored(&std::fs::read(&zip).unwrap()).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for want in [
        "chain-F_Cu.gtl",
        "chain-Edge_Cuts.gko",
        "chain-bom.csv",
        "chain-pos.csv",
        "PREFLIGHT.txt",
    ] {
        assert!(names.contains(&want), "{names:?}");
    }
    let cu =
        std::str::from_utf8(&files.iter().find(|(n, _)| n == "chain-F_Cu.gtl").unwrap().1).unwrap();
    let img = buturecad_manufacturing::gerber_read::parse(cu).unwrap();
    assert_eq!(img.count().1, 2, "two track strokes");
    assert!(
        img.count().0 + img.count().2 >= 6,
        "six pads (flashes or regions)"
    );
    // Same flow into a folder, then the BOM on its own.
    let dir = d.path().join("fab");
    assert_eq!(
        run(&["export-gerber", &p, "--out", dir.to_str().unwrap()]).0,
        0
    );
    assert!(dir.join("chain-F_Cu.gtl").exists());
    let (c, bom, _) = run(&["export-bom", &p]);
    assert_eq!(c, 0);
    assert!(
        bom.starts_with("Reference,Quantity,Value,Footprint") && bom.contains("R1,1,1k,0603"),
        "{bom}"
    );
    // Inspect.
    let (_, comps) = json(&["inspect", "components", &p]);
    assert_eq!(comps["components"].as_array().unwrap().len(), 3);
    assert_eq!(comps["components"][1]["placement"]["x_mm"], 14.0);
    let (_, nets) = json(&["inspect", "nets", &p]);
    assert!(nets["nets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "A" && n["pins"].as_array().unwrap().len() == 2));
    let (_, board) = json(&["inspect", "board", &p]);
    assert_eq!(
        (board["footprints"].as_u64(), board["tracks"].as_u64()),
        (Some(3), Some(2))
    );
    assert_eq!(board["outline_mm"]["width"], 30.0);
}

#[test]
fn findings_blocked_exports_and_usage_errors_have_distinct_exit_codes() {
    let d = tempfile::tempdir().unwrap();
    // A script with errors: exit 1, positions in the output.
    let bad = d.path().join("bad.beda");
    std::fs::write(&bad, "component C1 { value = \"1\" }\nconnect C1.9 -> GND\nboard { outline = rect(0mm,0mm,1mm + 1V,1mm) }\n").unwrap();
    let (c, v) = json(&["check", bad.to_str().unwrap()]);
    assert_eq!(c, 1);
    let diags = v["diagnostics"].as_array().unwrap();
    assert!(
        diags.iter().any(|x| x["line"] == 2
            && x["column"] == 9
            && x["message"].as_str().unwrap().contains("no pin")),
        "{v}"
    );
    assert!(
        diags.iter().any(|x| x["line"] == 3
            && x["message"]
                .as_str()
                .unwrap()
                .contains("incompatible dimensions")),
        "{v}"
    );
    let (c, out, _) = run(&[
        "build",
        bad.to_str().unwrap(),
        "--out",
        d.path().join("x").to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert!(out.contains("bad.beda:2:9: error[E-pin]"), "{out}");
    assert!(
        !d.path().join("x").exists(),
        "a failed build writes nothing"
    );
    // A project whose DRC fails: the Gerber export is blocked and writes nothing.
    // Net B's track is made to cross net A's track: a short circuit.
    let src = std::fs::read_to_string(example("chain.beda")).unwrap()
        + "\ntrack B on F.Cu width 0.25mm path (12mm, 9mm) -> (12mm, 11mm)\n";
    let script = d.path().join("broken.beda");
    std::fs::write(&script, src).unwrap();
    let p = d.path().join("broken");
    assert_eq!(
        run(&[
            "build",
            script.to_str().unwrap(),
            "--out",
            p.to_str().unwrap()
        ])
        .0,
        0
    );
    let (c, drc) = json(&["drc", p.to_str().unwrap()]);
    assert_eq!(c, 1);
    assert!(drc["errors"].as_u64().unwrap() > 0, "{drc}");
    let out_dir = d.path().join("nofab");
    let (c, g) = json(&[
        "export-gerber",
        p.to_str().unwrap(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!((c, g["blocked"].as_bool()), (1, Some(true)), "{g}");
    assert!(!out_dir.exists());
    // Usage and I/O problems are exit code 2 and go to stderr.
    for args in [
        vec![],
        vec!["frobnicate"],
        vec!["drc"],
        vec!["drc", "/no/such/project"],
        vec!["build", "x.beda"],
        vec!["erc", "--bogus"],
        vec!["inspect", "wires", "/tmp"],
    ] {
        let (c, _, e) = run(&args);
        assert_eq!(c, 2, "{args:?}");
        assert!(!e.is_empty(), "{args:?}");
    }
    assert_eq!(run(&["help"]).0, 0);
    assert!(run(&["version"]).1.starts_with("buturecad "));
    // --param overrides a script parameter.
    let ps = d.path().join("p.beda");
    std::fs::write(
        &ps,
        "param n = 2\nfor i in 0..n { component R[i] { value = \"1k\" } }\n",
    )
    .unwrap();
    let (_, v) = json(&["check", ps.to_str().unwrap(), "--param", "n=5"]);
    assert_eq!(v["design"]["components"], 5);
}

#[test]
fn the_real_binary_works_and_the_cli_links_no_gui_crate() {
    let bin = env!("CARGO_BIN_EXE_buturecad");
    let out = Command::new(bin)
        .args(["check", example("chain.beda").to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let out = Command::new(bin).args(["nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    // Headless guarantee: no Tauri, window system or GPU stack in the dependency tree.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let tree = Command::new(cargo)
        .args([
            "tree",
            "-p",
            "buturecad-cli",
            "--prefix",
            "none",
            "--edges",
            "normal",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&tree.stdout).to_lowercase();
    assert!(
        text.contains("buturecad-scripting"),
        "the tree was read: {text}"
    );
    for gui in [
        "tauri",
        "wgpu",
        "winit",
        "webkit",
        "buturecad-renderer",
        "buturecad-viewer3d",
        "buturecad-desktop",
        "gtk",
    ] {
        assert!(
            !text.lines().any(|l| l.starts_with(gui)),
            "the CLI must not depend on {gui}"
        );
    }
}
