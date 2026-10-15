//! The agent protocol and the action vocabulary under fire: arbitrary input must give a JSON-RPC
//! error or a tool error, never a panic, and must never change the project.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use buturecad_agent_api::{Action, Server};
use buturecad_testkit::{as_text, iterations, mutations};

const BASE: &str = "board { outline = rect(0mm, 0mm, 30mm, 20mm) }\ncomponent R1 { value = \"1k\" footprint = \"0603\" }\ncomponent R2 { value = \"2k\" footprint = \"0603\" }\nnet MID { R1.2, R2.1 }\nplace R1 at (10mm, 10mm)\n";

const REQUESTS: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"fuzz"}}}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_board","arguments":{}}}"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"measure_net","arguments":{"net":"MID"}}}"#,
    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"check_buturescript","arguments":{"source":"component R1 { value = \"1\" }"}}}"#,
    r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"propose_changes","arguments":{"title":"t","actions":[{"action":"create_component","reference":"C1","symbol":"Buture:C","value":"1n","footprint":"0402"},{"action":"connect","net":"MID","pins":[{"reference":"C1","pin":"1"}]},{"action":"place","reference":"C1","x":12000000,"y":12000000},{"action":"route","net":"MID","layer":"F.Cu","points":[[0,0],[1000000,0]],"width":250000},{"action":"change_rule","rule":"min_track_width","value":150000}]}}}"#,
    r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"search_components","arguments":{"query":"r","limit":5}}}"#,
];

#[test]
fn damaged_requests_never_panic_and_never_change_the_project() {
    let e = buturecad_scripting::build(BASE, &Default::default(), None)
        .unwrap()
        .engine;
    let project = e.into_project();
    let mut s = Server::new(
        project.clone(),
        Box::new(buturecad_scripting::resolve_footprint),
    );
    let n = iterations(500);
    for (k, req) in REQUESTS.iter().enumerate() {
        for bad in mutations(req.as_bytes(), 50 + k as u64, n) {
            let line = as_text(&bad).replace('\n', " ");
            if let Some(resp) = s.handle_line(&line) {
                let v: serde_json::Value =
                    serde_json::from_str(&resp).expect("every reply is valid JSON");
                assert_eq!(v["jsonrpc"], "2.0");
                assert!(v.get("result").is_some() || v.get("error").is_some());
            }
        }
    }
    assert_eq!(
        *s.project(),
        project,
        "no request, however broken, modifies the project"
    );
}

#[test]
fn damaged_actions_are_rejected_by_the_types_or_by_validation() {
    let actions = r#"[{"action":"create_component","reference":"C1","symbol":"Buture:C"},{"action":"move","reference":"R1","dx":1,"dy":2},{"action":"create_zone","net":"MID","layer":"B.Cu","outline":[[0,0],[1,0],[1,1]]}]"#;
    let e = buturecad_scripting::build(BASE, &Default::default(), None)
        .unwrap()
        .engine;
    let mut api =
        buturecad_agent_api::AgentApi::new(Box::new(buturecad_scripting::resolve_footprint));
    let mut parsed = 0;
    for bad in mutations(actions.as_bytes(), 77, iterations(1000)) {
        if let Ok(list) = serde_json::from_slice::<Vec<Action>>(&bad) {
            parsed += 1;
            use buturecad_agent_api::AgentSurface;
            // Whatever parses is validated on a copy; the real project is never touched.
            let p = api.propose(e.project(), e.revision(), "fuzz", list, "fuzz");
            assert!(matches!(
                p.status,
                buturecad_agent_api::ProposalStatus::Pending
                    | buturecad_agent_api::ProposalStatus::Invalid
            ));
        }
    }
    assert!(
        parsed > 0,
        "some mutations still parse, so validation was exercised"
    );
}
