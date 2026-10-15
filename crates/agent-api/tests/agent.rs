//! The Agent API end to end: read, analyze, propose (validated on a copy), review, apply as one
//! undoable step, audit.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_agent_api::*;
use buturecad_eda_core::CommandEngine;
use serde_json::json;

fn api() -> AgentApi {
    AgentApi::new(Box::new(buturecad_scripting::resolve_footprint))
}

const BASE: &str = r#"
project "base"
board { outline = rect(0mm, 0mm, 30mm, 20mm) }
component J1 { symbol = "Buture:Conn_01x02" value = "PWR" footprint = "header2" }
component R1 { value = "1k" footprint = "0603" }
connect J1.1 -> VDD -> R1.1
connect J1.2 -> GND
connect R1.2 -> GND
place J1 at (5mm, 5mm)
place R1 at (15mm, 10mm)
"#;

fn engine() -> CommandEngine {
    buturecad_scripting::build(BASE, &Default::default(), None)
        .unwrap()
        .engine
}

fn add_cap() -> Vec<Action> {
    vec![
        Action::CreateComponent {
            reference: None,
            symbol: "Buture:C".into(),
            value: "100nF".into(),
            footprint: "0402".into(),
            properties: Default::default(),
        },
        Action::Connect {
            net: "VDD".into(),
            pins: vec![PinSpec {
                reference: "C1".into(),
                pin: "1".into(),
            }],
        },
        Action::Connect {
            net: "GND".into(),
            pins: vec![PinSpec {
                reference: "C1".into(),
                pin: "2".into(),
            }],
        },
        Action::Place {
            reference: "C1".into(),
            x: 17_000_000,
            y: 14_000_000,
            rotation_deg: 0,
            bottom: false,
        },
    ]
}

#[test]
fn reads_and_analyses_are_structured_and_audited() {
    let e = engine();
    let mut a = api();
    let p = e.project();
    let proj = a.read(p, e.revision(), "get_project", "tester").unwrap();
    assert_eq!(
        (proj["name"].clone(), proj["footprints"].clone()),
        (json!("base"), json!(2))
    );
    let comps = a.read(p, 0, "get_components", "tester").unwrap();
    assert_eq!(comps["components"].as_array().unwrap().len(), 2);
    assert_eq!(comps["components"][0]["board"]["x"], 5_000_000);
    let board = a.read(p, 0, "get_board", "tester").unwrap();
    let j1 = board["footprints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["reference"] == "J1")
        .unwrap();
    assert_eq!(
        j1["pads"].as_array().unwrap().len(),
        2,
        "pad positions are visible to the agent"
    );
    for q in ["get_schematic", "get_nets", "get_rules", "get_violations"] {
        assert!(a.read(p, 0, q, "tester").unwrap().is_object(), "{q}");
    }
    assert!(matches!(
        a.read(p, 0, "get_secrets", "tester"),
        Err(ApiError::UnknownQuery(_))
    ));
    assert!(a.analyze(p, "run_erc", &json!({}), "tester").unwrap()["errors"].is_number());
    assert!(a.analyze(p, "run_drc", &json!({}), "tester").unwrap()["warnings"].is_number());
    let m = a
        .analyze(p, "measure_net", &json!({"net": "VDD"}), "tester")
        .unwrap();
    assert_eq!(
        (m["pads"].clone(), m["unrouted_connections"].clone()),
        (json!(2), json!(1))
    );
    assert!(a
        .analyze(p, "measure_net", &json!({"net": "nope"}), "tester")
        .is_err());
    assert!(a
        .analyze(p, "measure_track", &json!({"id": 999999}), "tester")
        .is_err());
    // Every call is in the audit log, with the actor.
    let kinds: Vec<_> = a
        .audit
        .entries()
        .iter()
        .map(|x| (x.kind.as_str(), x.actor.as_str()))
        .collect();
    assert_eq!(kinds.len(), 12 - 2, "{kinds:?}");
    assert!(kinds.iter().all(|k| k.1 == "tester"));
    assert!(a
        .audit
        .to_jsonl()
        .lines()
        .all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()));
}

#[test]
fn a_proposal_is_validated_on_a_copy_then_applied_only_after_approval() {
    let mut e = engine();
    let mut a = api();
    let before = e.project().clone();
    let rev = e.revision();
    let p = a.propose(e.project(), rev, "Add decoupling", add_cap(), "agent-x");
    assert_eq!(p.status, ProposalStatus::Pending, "{:?}", p.validation);
    assert_eq!(*e.project(), before, "proposing changes nothing");
    assert_eq!(e.revision(), rev);
    // The diff shows the component, its labels, the footprint and the placement; ERC/DRC deltas exist.
    let lines = p.validation.diff.lines().join("\n");
    assert!(lines.contains("[schematic] component C1"), "{lines}");
    assert!(
        lines.contains("label VDD") && lines.contains("power GND"),
        "{lines}"
    );
    assert!(
        lines.contains("[board] footprint C1") && lines.contains("(17.000, 14.000) mm"),
        "{lines}"
    );
    assert!(p.validation.erc.is_some() && p.validation.drc.is_some());
    // Approving applies exactly the validated result, as ONE undo step.
    let history_before = e.undo_history().len();
    a.apply(&mut e, &p.id, "user").unwrap();
    assert_eq!(e.undo_history().len(), history_before + 1);
    assert_eq!(e.undo_history().last().unwrap(), "Agent: Add decoupling");
    assert_eq!(a.proposal(&p.id).unwrap().status, ProposalStatus::Applied);
    assert!(e
        .project()
        .pcb()
        .placed
        .values()
        .any(|f| f.reference == "C1" && f.placement.position.x == 17_000_000));
    let vdd = e
        .project()
        .pcb()
        .nets
        .iter()
        .find(|(_, n)| n.name == "VDD")
        .map(|(i, _)| *i)
        .unwrap();
    let c1 = e
        .project()
        .pcb()
        .placed
        .values()
        .find(|f| f.reference == "C1")
        .unwrap();
    assert_eq!(
        c1.pad_nets.get("1"),
        Some(&vdd),
        "the new pad is on the VDD net"
    );
    // It cannot be applied twice, and undo restores the project exactly.
    assert!(matches!(
        a.apply(&mut e, &p.id, "user"),
        Err(ApiError::NotApplicable(..))
    ));
    e.undo().unwrap();
    assert_eq!(*e.project(), before);
    // The audit trail tells the story in order.
    let story: Vec<_> = a.audit.entries().iter().map(|x| x.kind.as_str()).collect();
    assert_eq!(story, ["propose", "validated", "approved", "applied"]);
}

#[test]
fn invalid_stale_and_rejected_proposals_never_change_the_project() {
    let mut e = engine();
    let mut a = api();
    let before = e.project().clone();
    // A bad action in the middle: the whole proposal is invalid and says which action.
    let mut acts = add_cap();
    acts.insert(
        2,
        Action::Route {
            net: "NOPE".into(),
            layer: "F.Cu".into(),
            points: vec![[0, 0], [1_000_000, 0]],
            width: 250_000,
        },
    );
    let bad = a.propose(e.project(), e.revision(), "Broken", acts, "agent-x");
    assert_eq!(bad.status, ProposalStatus::Invalid);
    let (idx, msg) = bad.validation.failed_action.clone().unwrap();
    assert_eq!(idx, 2);
    assert!(msg.contains("no board net named “NOPE”"), "{msg}");
    assert!(matches!(
        a.apply(&mut e, &bad.id, "user"),
        Err(ApiError::NotApplicable(..))
    ));
    // Stale: the project moved on after validation.
    let good = a.propose(e.project(), e.revision(), "Cap", add_cap(), "agent-x");
    e.execute(buturecad_eda_core::commands::RenameProjectCommand::new(
        "renamed",
    ))
    .unwrap();
    assert!(matches!(
        a.apply(&mut e, &good.id, "user"),
        Err(ApiError::Stale(..))
    ));
    assert!(!e
        .project()
        .pcb()
        .placed
        .values()
        .any(|f| f.reference == "C1"));
    // Rejection is final and recorded with the reason.
    let p3 = a.propose(e.project(), e.revision(), "Cap again", add_cap(), "agent-x");
    a.reject(&p3.id, "user", "not wanted").unwrap();
    assert_eq!(a.proposal(&p3.id).unwrap().status, ProposalStatus::Rejected);
    assert!(a.apply(&mut e, &p3.id, "user").is_err());
    let last = a.audit.entries().last().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.detail["reason"].clone()),
        ("rejected", json!("not wanted"))
    );
    assert_ne!(e.project().name(), before.name());
    assert!(matches!(
        a.apply(&mut e, "P99", "user"),
        Err(ApiError::NoProposal(_))
    ));
}

#[test]
fn actions_cover_the_board_and_rules_and_findings_are_compared() {
    let mut e = engine();
    let mut a = api();
    // Connect R1's far end by routing, add a via and a zone, change a rule, then remove a part.
    let board = a.read(e.project(), 0, "get_board", "t").unwrap();
    let pad = |r: &str, n: &str| {
        let f = board["footprints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["reference"] == r)
            .unwrap();
        let p = f["pads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["number"] == n)
            .unwrap();
        [p["x"].as_i64().unwrap(), p["y"].as_i64().unwrap()]
    };
    let (from, to) = (pad("J1", "1"), pad("R1", "1"));
    let acts = vec![
        Action::Route {
            net: "VDD".into(),
            layer: "F.Cu".into(),
            points: vec![from, [to[0], from[1]], to],
            width: 250_000,
        },
        Action::CreateVia {
            net: "GND".into(),
            x: 25_000_000,
            y: 15_000_000,
            diameter: 800_000,
            drill: 400_000,
        },
        Action::CreateZone {
            net: "GND".into(),
            layer: "B.Cu".into(),
            outline: vec![
                [1_000_000, 1_000_000],
                [29_000_000, 1_000_000],
                [29_000_000, 19_000_000],
                [1_000_000, 19_000_000],
            ],
            clearance: Some(300_000),
            min_width: None,
        },
        Action::ChangeRule {
            rule: "min_track_width".into(),
            value: 120_000,
        },
        Action::Move {
            reference: "R1".into(),
            dx: 1_000_000,
            dy: 0,
        },
    ];
    let p = a.propose(
        e.project(),
        e.revision(),
        "Route and rules",
        acts,
        "agent-x",
    );
    assert_eq!(p.status, ProposalStatus::Pending, "{:?}", p.validation);
    let lines = p.validation.diff.lines().join("\n");
    for want in [
        "[board] track",
        "[board] via",
        "[board] zone",
        "→ 0.120 mm",
        "[board] footprint R1",
    ] {
        assert!(lines.contains(want), "{want} in\n{lines}");
    }
    a.apply(&mut e, &p.id, "user").unwrap();
    assert_eq!(e.project().pcb().rules.min_track_width, 120_000);
    let m = a
        .analyze(e.project(), "measure_net", &json!({"net": "VDD"}), "t")
        .unwrap();
    assert!(
        m["routed_length_nm"].as_f64().unwrap() > 5_000_000.0 && m["track_segments"] == 2,
        "{m}"
    );
    // Removing a part also removes its footprint on sync, and DRC/ERC deltas are reported.
    let p2 = a.propose(
        e.project(),
        e.revision(),
        "Remove R1",
        vec![Action::RemoveComponent {
            reference: "R1".into(),
        }],
        "agent-x",
    );
    assert_eq!(p2.status, ProposalStatus::Pending, "{:?}", p2.validation);
    assert!(p2
        .validation
        .diff
        .lines()
        .iter()
        .any(|l| l.starts_with("- [board] footprint R1")));
    // Locked parts are refused.
    let mut e2 = engine();
    let id = e2
        .project()
        .pcb()
        .placed
        .iter()
        .find(|(_, f)| f.reference == "J1")
        .map(|(i, _)| *i)
        .unwrap();
    let before = e2.project().pcb().placed[&id].clone();
    let mut locked = before.clone();
    locked.locked = true;
    e2.execute(buturecad_eda_core::commands::PcbEditCommand::new(
        "lock",
        vec![buturecad_eda_core::model::PcbChange::modify(
            id,
            buturecad_eda_core::model::PcbObject::Footprint(before),
            buturecad_eda_core::model::PcbObject::Footprint(locked),
        )],
    ))
    .unwrap();
    let r = a.propose(
        e2.project(),
        e2.revision(),
        "Move locked",
        vec![Action::Move {
            reference: "J1".into(),
            dx: 1,
            dy: 1,
        }],
        "agent-x",
    );
    assert_eq!(r.status, ProposalStatus::Invalid);
    assert!(r.validation.failed_action.unwrap().1.contains("locked"));
    // Disconnect works for label-made connections and says why when it cannot.
    let d = a.propose(
        e.project(),
        e.revision(),
        "Disconnect",
        vec![Action::Disconnect {
            reference: "J1".into(),
            pin: "2".into(),
        }],
        "agent-x",
    );
    assert_eq!(d.status, ProposalStatus::Pending, "{:?}", d.validation);
    let bad = a.propose(
        e.project(),
        e.revision(),
        "Disconnect none",
        vec![Action::Disconnect {
            reference: "J1".into(),
            pin: "9".into(),
        }],
        "agent-x",
    );
    assert!(bad
        .validation
        .failed_action
        .unwrap()
        .1
        .contains("does not exist"));
}

#[test]
fn actions_round_trip_through_json_for_the_protocol() {
    let acts = add_cap();
    let text = serde_json::to_string(&acts).unwrap();
    assert!(text.contains("\"action\":\"create_component\""));
    let back: Vec<Action> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, acts);
    assert!(serde_json::from_str::<Action>(r#"{"action":"format_disk"}"#).is_err());
    assert!(
        serde_json::from_str::<Action>(r#"{"action":"place","reference":"U1","x":"a","y":1}"#)
            .is_err()
    );
}
