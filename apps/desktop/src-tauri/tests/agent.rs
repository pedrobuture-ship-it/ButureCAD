//! The AI Workspace through the workspace: providers → proposals → review → apply/reject.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::AddSymbolsCommand;
use buturecad_eda_core::ir_apply::apply_design;
use buturecad_geometry::Point;
use buturecad_schematic::{Direction, Pin, PinType, Symbol};

fn rig() -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    let pin = |n: &str, name: &str, kind, x: i64, y: i64| Pin {
        number: n.into(),
        name: name.into(),
        kind,
        position: Point::new(x * 1_270_000, y * 1_270_000),
        orientation: Direction::Right,
        length: 2_540_000,
        unit: 0,
        representation: 0,
    };
    let mut ic = Symbol::new("Test:MCU", "U");
    ic.pins = vec![
        pin("1", "VDD", PinType::PowerInput, -5, 2),
        pin("2", "VSS", PinType::PowerInput, -5, -2),
        pin("3", "PA0", PinType::Bidirectional, 5, 0),
    ];
    let src = "board { outline = rect(0mm,0mm,40mm,30mm) }\ncomponent U1 { symbol = \"Test:MCU\" value = \"MCU\" footprint = \"soic(pins=8)\" }\nconnect U1.VDD -> \"+3V3\"\nconnect U1.VSS -> GND\nplace U1 at (20mm, 15mm)\n";
    let out = buturecad_scripting::compile(src, &Default::default());
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(vec![ic])).unwrap();
    apply_design(e, &out.design.unwrap(), &|n| {
        buturecad_scripting::resolve_footprint(n)
    })
    .unwrap();
    ws.set_mode(Mode::Agent);
    (ws, dir)
}

fn info(ws: &mut Workspace) -> AgentInfo {
    ws.snapshot().agent.unwrap()
}

#[test]
fn a_request_becomes_a_reviewable_proposal_and_nothing_changes_until_apply() {
    let (mut ws, dir) = rig();
    let i = info(&mut ws);
    assert!(
        i.providers.iter().any(|p| p.kind == "rule-based")
            && i.providers.iter().any(|p| p.kind == "external input")
    );
    let before = ws.open.as_ref().unwrap().engine.project().clone();
    ws.agent_command("request", Some("Add decoupling to U1"))
        .unwrap();
    let i = info(&mut ws);
    let p = &i.proposals[0];
    assert_eq!((p.status.as_str(), p.stale), ("pending", false), "{p:?}");
    assert!(
        p.actions.iter().any(|a| a.contains("create component C1")),
        "{:?}",
        p.actions
    );
    assert!(p.rationale.iter().any(|r| r.contains("U1.1 (VDD)")));
    assert!(p
        .diff
        .iter()
        .any(|d| d.change == "added" && d.kind == "component" && d.label.contains("C1")));
    assert!(p
        .diff
        .iter()
        .any(|d| d.domain == "board" && d.kind == "footprint"));
    assert!(
        p.erc.is_some() && p.drc.is_some(),
        "ERC/DRC are run on the proposed result"
    );
    assert!(
        p.json.contains("\"create_component\""),
        "Inspect shows the structured proposal"
    );
    assert_eq!(
        *ws.open.as_ref().unwrap().engine.project(),
        before,
        "a pending proposal changes nothing"
    );
    // Apply: one undo step, status updated, audit file written.
    let id = p.id.clone();
    let steps = ws.open.as_ref().unwrap().engine.undo_history().len();
    ws.agent_command("apply", Some(&id)).unwrap();
    let e = &ws.open.as_ref().unwrap().engine;
    assert_eq!(e.undo_history().len(), steps + 1);
    assert!(e
        .project()
        .pcb()
        .placed
        .values()
        .any(|f| f.reference == "C1"));
    assert_eq!(info(&mut ws).proposals[0].status, "applied");
    assert!(ws.agent_command("apply", Some(&id)).is_err());
    ws.undo().unwrap();
    assert_eq!(
        *ws.open.as_ref().unwrap().engine.project(),
        before,
        "undo reverts the whole proposal"
    );
    let log = std::fs::read_to_string(dir.path().join("P/.buture/agent-audit.jsonl")).unwrap();
    let kinds: Vec<String> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(kinds, ["propose", "validated", "approved", "applied"]);
    assert!(info(&mut ws).audit.len() >= 4);
}

#[test]
fn reject_stale_invalid_and_pasted_proposals() {
    let (mut ws, _d) = rig();
    // A pasted proposal (what any external agent would produce) goes through the same pipeline.
    ws.agent_command("provider", Some("json")).unwrap();
    let good =
        r#"{"title":"Move U1","actions":[{"action":"move","reference":"U1","dx":1000000,"dy":0}]}"#;
    ws.agent_command("request", Some(good)).unwrap();
    let p1 = info(&mut ws).proposals[0].id.clone();
    ws.agent_command("reject", Some(&p1)).unwrap();
    assert_eq!(info(&mut ws).proposals[0].status, "rejected");
    assert!(ws.agent_command("apply", Some(&p1)).is_err());
    // Stale after any other edit.
    ws.agent_command("request", Some(good)).unwrap();
    let p2 = info(&mut ws).proposals[0].id.clone();
    ws.rename_project("Other").unwrap();
    let i = info(&mut ws);
    assert!(i.proposals[0].stale);
    assert!(ws
        .agent_command("apply", Some(&p2))
        .unwrap_err()
        .contains("changed after"));
    // An invalid action names the action and the reason; garbage input is an error, not a proposal.
    ws.agent_command("request", Some(r#"[{"action":"route","net":"NOPE","layer":"F.Cu","points":[[0,0],[1,1]],"width":250000}]"#)).unwrap();
    let p = &info(&mut ws).proposals[0];
    assert_eq!(p.status, "invalid");
    assert!(
        p.failed_action
            .as_ref()
            .unwrap()
            .contains("no board net named"),
        "{:?}",
        p.failed_action
    );
    assert!(ws.agent_command("request", Some("hello")).is_err());
    assert!(!info(&mut ws).error.is_empty());
    ws.agent_command("provider", Some("decoupling")).unwrap();
    assert!(ws.agent_command("request", Some("paint it blue")).is_err());
    assert!(
        ws.agent_command("provider", Some("gpt")).is_err(),
        "no model provider is configured"
    );
}
