//! Minimal ButureCAD agent client in Rust (serde_json only).
//!
//!     cargo run -p buturecad-agent-api --example agent_client -- /path/to/buturecad /path/to/project
//!
//! Starts `buturecad agent serve <project>` and speaks JSON-RPC 2.0 over its stdin/stdout. It
//! inspects the project and PROPOSES a decoupling capacitor; it cannot apply anything.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

struct Agent {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Agent {
    fn start(binary: &str, project: &str) -> std::io::Result<Self> {
        let mut child = Command::new(binary)
            .args(["agent", "serve", project])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let out = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| std::io::Error::other("no stdout"))?,
        );
        Ok(Self {
            child,
            out,
            next: 0,
        })
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().ok_or("stdin closed")?;
        writeln!(stdin, "{msg}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.out.read_line(&mut line).map_err(|e| e.to_string())?;
        let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Some(e) = reply.get("error") {
            return Err(e["message"].as_str().unwrap_or("error").to_owned());
        }
        Ok(reply["result"].clone())
    }

    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let r = self.rpc("tools/call", json!({"name": name, "arguments": args}))?;
        if r["isError"] == true {
            return Err(r["content"][0]["text"]
                .as_str()
                .unwrap_or("tool error")
                .to_owned());
        }
        Ok(r["structuredContent"].clone())
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let (binary, project) = (
        args.get(1)
            .ok_or("usage: agent_client <buturecad> <project>")?,
        args.get(2)
            .ok_or("usage: agent_client <buturecad> <project>")?,
    );
    let mut agent = Agent::start(binary, project).map_err(|e| e.to_string())?;
    let info = agent.rpc(
        "initialize",
        json!({"clientInfo": {"name": "example-rust"}}),
    )?;
    let tools = agent.rpc("tools/list", json!({}))?;
    println!(
        "connected to {}, {} tools",
        info["serverInfo"]["name"].as_str().unwrap_or("?"),
        tools["tools"].as_array().map_or(0, Vec::len)
    );

    let comps = agent.tool("get_components", json!({}))?;
    let refs: Vec<&str> = comps["components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["reference"].as_str())
        .collect();
    println!("components: {}", refs.join(", "));

    let proposal = agent.tool(
        "propose_changes",
        json!({
            "title": "Add a bypass capacitor between nets A and B",
            "actions": [
                {"action": "create_component", "reference": "C1", "symbol": "Buture:C", "value": "100nF", "footprint": "0402"},
                {"action": "connect", "net": "A", "pins": [{"reference": "C1", "pin": "1"}]},
                {"action": "connect", "net": "B", "pins": [{"reference": "C1", "pin": "2"}]},
                {"action": "place", "reference": "C1", "x": 14000000, "y": 14000000}
            ]
        }),
    )?;
    let drc = &proposal["validation"]["drc"];
    println!(
        "proposal {} is {}: {} changes, DRC errors {} -> {}",
        proposal["id"].as_str().unwrap_or("?"),
        proposal["status"].as_str().unwrap_or("?"),
        proposal["validation"]["diff"]["entries"]
            .as_array()
            .map_or(0, Vec::len),
        drc["errors_before"],
        drc["errors_after"]
    );
    println!(
        "review it with: buturecad agent show {project} {}",
        proposal["id"].as_str().unwrap_or("?")
    );
    drop(agent.child.stdin.take());
    let _ = agent.child.wait();
    Ok(())
}
