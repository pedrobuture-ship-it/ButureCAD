//! The JSON-RPC agent protocol: shape, tools, safety (no apply), errors.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_agent_api::{serve, tools, Server};
use serde_json::{json, Value};

const BASE: &str = r#"
board { outline = rect(0mm, 0mm, 30mm, 20mm) }
component R1 { value = "1k" footprint = "0603" }
component R2 { value = "2k" footprint = "0603" }
net MID { R1.2, R2.1 }
place R1 at (10mm, 10mm)
place R2 at (14mm, 10mm)
"#;

fn server() -> Server {
    let e = buturecad_scripting::build(BASE, &Default::default(), None)
        .unwrap()
        .engine;
    Server::new(
        e.into_project(),
        Box::new(buturecad_scripting::resolve_footprint),
    )
}

fn rpc(s: &mut Server, id: i64, method: &str, params: Value) -> Value {
    s.handle(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
        .unwrap()
}

fn call(s: &mut Server, name: &str, args: Value) -> Value {
    rpc(s, 1, "tools/call", json!({"name": name, "arguments": args}))
}

#[test]
fn handshake_and_tool_catalogue() {
    let mut s = server();
    let r = rpc(
        &mut s,
        1,
        "initialize",
        json!({"clientInfo": {"name": "my-agent"}}),
    );
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "buturecad-agent/1");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(
        s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .is_none(),
        "notifications get no reply"
    );
    assert_eq!(rpc(&mut s, 2, "ping", json!({}))["result"], json!({}));
    let list = rpc(&mut s, 3, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for want in [
        "get_project",
        "get_schematic",
        "get_board",
        "get_components",
        "get_nets",
        "get_rules",
        "get_violations",
        "run_erc",
        "run_drc",
        "measure_net",
        "measure_track",
        "search_components",
        "check_buturescript",
        "propose_changes",
        "get_proposal",
        "list_proposals",
    ] {
        assert!(names.contains(&want), "{want}");
    }
    // The agent surface has no way to apply, reject, write or execute.
    for forbidden in [
        "apply",
        "apply_proposal",
        "reject",
        "write_file",
        "read_file",
        "exec",
        "save",
        "approve",
    ] {
        assert!(!names.iter().any(|n| n.contains(forbidden)), "{forbidden}");
        let r = call(&mut s, forbidden, json!({}));
        assert_eq!(r["error"]["code"], -32602, "{forbidden}: {r}");
    }
    // Every tool has a JSON Schema object.
    assert!(tools()
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["inputSchema"]["type"] == "object"
            && t["description"].as_str().is_some_and(|d| !d.is_empty())));
}

#[test]
fn read_analysis_search_and_buturescript_tools() {
    let mut s = server();
    let r = call(&mut s, "get_components", json!({}));
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(
        r["result"]["structuredContent"]["components"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("\"R1\""));
    let m = call(&mut s, "measure_net", json!({"net": "MID"}));
    assert_eq!(m["result"]["structuredContent"]["pads"], 2);
    let bad = call(&mut s, "measure_net", json!({"net": "NOPE"}));
    assert_eq!(
        bad["result"]["isError"], true,
        "tool failures are results, not protocol errors"
    );
    assert!(
        call(&mut s, "run_drc", json!({}))["result"]["structuredContent"]["errors"].is_number()
    );
    let found = call(
        &mut s,
        "search_components",
        json!({"query": "r", "kinds": ["symbol"], "limit": 5}),
    );
    let results = found["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap();
    assert!(results.iter().any(|x| x["id"] == "Buture:R"), "{found}");
    // ButureScript check returns positions.
    let c = call(
        &mut s,
        "check_buturescript",
        json!({"source": "component C1 { value = \"1\" }\nconnect C1.9 -> GND"}),
    );
    let d = &c["result"]["structuredContent"];
    assert_eq!(d["ok"], false);
    assert_eq!(
        (
            d["diagnostics"][0]["line"].clone(),
            d["diagnostics"][0]["column"].clone()
        ),
        (json!(2), json!(9))
    );
    let ok = call(
        &mut s,
        "check_buturescript",
        json!({"source": "param n = 2\nfor i in 0..n { component R[i] { value = \"1k\" } }", "params": {"n": "4"}}),
    );
    assert_eq!(ok["result"]["structuredContent"]["design"]["components"], 4);
}

#[test]
fn proposals_are_validated_not_applied_and_stay_inspectable() {
    let mut s = server();
    let before = s.project().clone();
    let actions = json!([
        {"action": "create_component", "reference": "C1", "symbol": "Buture:C", "value": "100nF", "footprint": "0402"},
        {"action": "connect", "net": "MID", "pins": [{"reference": "C1", "pin": "1"}]},
        {"action": "connect", "net": "GND", "pins": [{"reference": "C1", "pin": "2"}]},
        {"action": "place", "reference": "C1", "x": 12000000, "y": 14000000}
    ]);
    let r = call(
        &mut s,
        "propose_changes",
        json!({"title": "Cap on MID", "actions": actions}),
    );
    let p = &r["result"]["structuredContent"];
    assert_eq!(
        (p["id"].clone(), p["status"].clone()),
        (json!("P1"), json!("pending")),
        "{r}"
    );
    assert!(p["validation"]["diff"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "component"));
    assert!(p["validation"]["drc"]["errors_after"].is_number());
    assert_eq!(*s.project(), before, "a proposal never changes the project");
    let got = call(&mut s, "get_proposal", json!({"id": "P1"}));
    assert_eq!(got["result"]["structuredContent"]["title"], "Cap on MID");
    assert_eq!(
        call(&mut s, "get_proposal", json!({"id": "P9"}))["result"]["isError"],
        true
    );
    let list = call(&mut s, "list_proposals", json!({}));
    assert_eq!(
        list["result"]["structuredContent"]["proposals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Invalid proposals are results with the failing action; malformed ones are protocol errors.
    let inv = call(
        &mut s,
        "propose_changes",
        json!({"title": "bad", "actions": [{"action": "route", "net": "NOPE", "layer": "F.Cu", "points": [[0, 0], [1, 1]], "width": 250000}]}),
    );
    assert_eq!(inv["result"]["structuredContent"]["status"], "invalid");
    for bad in [
        json!({"title": "x", "actions": []}),
        json!({"title": "x", "actions": [{"action": "launch_missiles"}]}),
        json!({"title": "x"}),
    ] {
        assert_eq!(
            call(&mut s, "propose_changes", bad)["error"]["code"],
            -32602
        );
    }
    // Everything is in the audit log under the client's name.
    let init = rpc(
        &mut s,
        9,
        "initialize",
        json!({"clientInfo": {"name": "agent-z"}}),
    );
    assert!(init["result"].is_object());
    call(&mut s, "get_project", json!({}));
    assert_eq!(s.api().audit.entries().last().unwrap().actor, "agent-z");
}

#[test]
fn jsonrpc_errors_and_the_line_transport() {
    let mut s = server();
    assert_eq!(
        s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "nope"}))
            .unwrap()["error"]["code"],
        -32601
    );
    assert_eq!(
        s.handle(&json!({"id": 1, "method": "ping"})).unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(s.handle(&json!([1, 2])).unwrap()["error"]["code"], -32600);
    assert_eq!(
        s.handle(&json!({"jsonrpc": "2.0", "id": 4})).unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(
        s.handle(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {}}))
            .unwrap()["error"]["code"],
        -32602
    );
    let parse: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(
        (parse["error"]["code"].clone(), parse["id"].clone()),
        (json!(-32700), Value::Null)
    );
    // A scripted session over the line transport: responses in order, notifications silent, EOF or exit ends it.
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_project","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
    ]
    .join("\n");
    let mut out = Vec::new();
    let mut seen = 0;
    serve(&mut s, input.as_bytes(), &mut out, |_| seen += 1).unwrap();
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines.iter().map(|l| l["id"].clone()).collect::<Vec<_>>(),
        [json!(1), json!(2), json!(3)]
    );
    assert_eq!(seen, 4, "after_each runs per handled line (blank lines are skipped, ping after shutdown is not read)");
    assert!(s.is_shut_down());
}
