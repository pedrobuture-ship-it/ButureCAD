//! `buturecad agent`: the protocol server on a real process, the human review commands, and the
//! three documented client examples (Rust, TypeScript, Python) actually run.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_buturecad")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build_project(dir: &Path) -> String {
    let p = dir.join("chain").to_str().unwrap().to_owned();
    let out = Command::new(bin())
        .args([
            "build",
            root().join("examples/chain.beda").to_str().unwrap(),
            "--out",
            &p,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(bin()).args(args).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn session(project: &str, requests: &[&str]) -> Vec<serde_json::Value> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(bin())
        .args(["agent", "serve", project])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

const PROPOSE: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"propose_changes","arguments":{"title":"Cap","actions":[
 {"action":"create_component","reference":"C1","symbol":"Buture:C","value":"100nF","footprint":"0402"},
 {"action":"connect","net":"A","pins":[{"reference":"C1","pin":"1"}]},
 {"action":"connect","net":"B","pins":[{"reference":"C1","pin":"2"}]},
 {"action":"place","reference":"C1","x":14000000,"y":14000000}]}}}"#;

#[test]
fn an_agent_proposes_over_stdio_and_only_the_human_can_apply() {
    let d = tempfile::tempdir().unwrap();
    let p = build_project(d.path());
    let before = std::fs::read_to_string(Path::new(&p).join("pcb.json")).ok();
    let replies = session(
        &p,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test-agent"}}}"#,
            &PROPOSE.replace('\n', ""),
        ],
    );
    assert_eq!(
        replies[1]["result"]["structuredContent"]["status"], "pending",
        "{}",
        replies[1]
    );
    // The project on disk is untouched by the agent session; the proposal and audit are stored.
    assert_eq!(
        std::fs::read_to_string(Path::new(&p).join("pcb.json")).ok(),
        before
    );
    let (c, out, _) = run(&["agent", "proposals", &p]);
    assert!(
        c == 0 && out.contains("P1") && out.contains("pending") && out.contains("test-agent"),
        "{out}"
    );
    let audit = std::fs::read_to_string(Path::new(&p).join(".buture/agent-audit.jsonl")).unwrap();
    assert!(audit
        .lines()
        .any(|l| l.contains("\"propose\"") && l.contains("test-agent")));
    // Review shows the diff and the checks; applying needs an explicit --yes.
    let (c, out, _) = run(&["agent", "show", &p, "P1"]);
    assert_eq!(c, 0, "{out}");
    assert!(
        out.contains("component C1") && out.contains("DRC: errors"),
        "{out}"
    );
    let (c, _, err) = run(&["agent", "apply", &p, "P1"]);
    assert_eq!(c, 2);
    assert!(err.contains("--yes"));
    let (c, out, err) = run(&["agent", "apply", &p, "P1", "--yes"]);
    assert_eq!(c, 0, "{out}{err}");
    let (_, comps, _) = run(&["inspect", "components", &p]);
    assert!(comps.contains("C1"), "{comps}");
    assert!(run(&["agent", "proposals", &p]).1.contains("applied"));
    // A second apply, and a stale proposal, are refused.
    assert_eq!(run(&["agent", "apply", &p, "P1", "--yes"]).0, 2);
    let r2 = session(
        &p,
        &[&PROPOSE.replace('\n', "").replace("\"C1\"", "\"C9\"")],
    );
    assert_eq!(
        r2[0]["result"]["structuredContent"]["id"], "P2",
        "ids continue across sessions"
    );
    assert_eq!(run(&["agent", "reject", &p, "P2", "--reason", "no"]).0, 0);
    assert!(run(&["agent", "proposals", &p]).1.contains("rejected"));
    assert_eq!(run(&["agent", "apply", &p, "P2", "--yes"]).0, 2);
    // Garbage ids are rejected before touching any file.
    assert_eq!(run(&["agent", "show", &p, "../../etc/passwd"]).0, 2);
}

#[test]
fn a_stale_stored_proposal_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = build_project(d.path());
    session(&p, &[&PROPOSE.replace('\n', "")]);
    // The project changes (another proposal is applied) after P1 was made.
    session(
        &p,
        &[&PROPOSE
            .replace('\n', "")
            .replace("\"C1\"", "\"C2\"")
            .replace("14000000", "15000000")],
    );
    assert_eq!(run(&["agent", "apply", &p, "P2", "--yes"]).0, 0);
    let (c, _, err) = run(&["agent", "apply", &p, "P1", "--yes"]);
    assert_eq!(c, 2);
    assert!(err.contains("stale"), "{err}");
}

fn expect_example_output(out: &std::process::Output, project: &str) {
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("connected to buturecad, 16 tools"), "{text}");
    assert!(text.contains("components: R1, R2, R3"), "{text}");
    assert!(text.contains("is pending"), "{text}");
    assert!(
        text.contains(&format!("buturecad agent show {project} P")),
        "{text}"
    );
}

#[test]
fn the_python_typescript_and_rust_examples_run_against_the_real_server() {
    let ex = root().join("docs/agent-api/examples");
    let d = tempfile::tempdir().unwrap();
    if Command::new("python3").arg("--version").output().is_ok() {
        let p = build_project(&d.path().join("py"));
        let out = Command::new("python3")
            .args(["-I", ex.join("client.py").to_str().unwrap(), bin(), &p])
            .output()
            .unwrap();
        expect_example_output(&out, &p);
    } else {
        eprintln!("SKIPPED python example: python3 not found");
    }
    let node_ok = Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| {
            v.trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|m| m.parse::<u32>().ok())
        })
        .is_some_and(|m| m >= 24);
    if node_ok {
        let p = build_project(&d.path().join("ts"));
        let out = Command::new("node")
            .args([ex.join("client.ts").to_str().unwrap(), bin(), &p])
            .output()
            .unwrap();
        expect_example_output(&out, &p);
    } else {
        eprintln!("SKIPPED typescript example: node >= 24 not found");
    }
    let p = build_project(&d.path().join("rs"));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "run",
            "-q",
            "-p",
            "buturecad-agent-api",
            "--example",
            "agent_client",
            "--",
            bin(),
            &p,
        ])
        .current_dir(root())
        .output()
        .unwrap();
    expect_example_output(&out, &p);
}
