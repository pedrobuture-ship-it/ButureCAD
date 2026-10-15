//! The **agent protocol** (roadmap stage 42): JSON-RPC 2.0 over any line-based transport (stdio
//! today), exposing the [`AgentSurface`] as *tools*.
//!
//! The method set follows the shape used by the Model Context Protocol so that agent frameworks
//! that speak it can use ButureCAD with a thin adapter: `initialize`, `ping`, `tools/list`,
//! `tools/call`. (This is a compatible *shape*, not a certified MCP server.)
//!
//! What a client can do: inspect the project, search components, run ERC/DRC, measure, check
//! ButureScript, and **propose** changes. What it cannot do: apply or reject a proposal, read or
//! write files, run anything. Those are decisions of the human host (`Reviewer`) and no tool
//! exists for them.

use crate::api::{AgentApi, AgentSurface, ApiError};
use crate::{Action, FootprintResolver};
use buturecad_eda_core::Project;
use buturecad_libraries::{Entry, Index, Kind, LibraryDir, Query};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub const PROTOCOL: &str = "buturecad-agent/1";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub struct Server {
    api: AgentApi,
    project: Project,
    revision: u64,
    libs: Vec<(String, LibraryDir)>,
    index: Option<Index>,
    actor: String,
    shutdown: bool,
}

fn point() -> Value {
    json!({"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "[x, y] in nm"})
}

fn action_schemas() -> Value {
    let s = |name: &str, props: Value, required: Vec<&str>| {
        let mut p = props;
        p["action"] = json!({"const": name});
        let mut req = vec!["action"];
        req.extend(required);
        json!({"type": "object", "properties": p, "required": req, "additionalProperties": false})
    };
    let pins = json!({"type": "array", "items": {"type": "object", "properties": {"reference": {"type": "string"}, "pin": {"type": "string"}}, "required": ["reference", "pin"]}});
    json!([
        s(
            "create_component",
            json!({"reference": {"type": "string"}, "symbol": {"type": "string", "description": "e.g. Buture:C"}, "value": {"type": "string"}, "footprint": {"type": "string", "description": "e.g. 0402, soic8"}, "properties": {"type": "object", "additionalProperties": {"type": "string"}}}),
            vec!["symbol"]
        ),
        s(
            "remove_component",
            json!({"reference": {"type": "string"}}),
            vec!["reference"]
        ),
        s(
            "connect",
            json!({"net": {"type": "string"}, "pins": pins}),
            vec!["net", "pins"]
        ),
        s(
            "disconnect",
            json!({"reference": {"type": "string"}, "pin": {"type": "string"}}),
            vec!["reference", "pin"]
        ),
        s(
            "place",
            json!({"reference": {"type": "string"}, "x": {"type": "integer"}, "y": {"type": "integer"}, "rotation_deg": {"type": "integer"}, "bottom": {"type": "boolean"}}),
            vec!["reference", "x", "y"]
        ),
        s(
            "move",
            json!({"reference": {"type": "string"}, "dx": {"type": "integer"}, "dy": {"type": "integer"}}),
            vec!["reference", "dx", "dy"]
        ),
        s(
            "route",
            json!({"net": {"type": "string"}, "layer": {"type": "string"}, "points": {"type": "array", "items": point()}, "width": {"type": "integer"}}),
            vec!["net", "layer", "points", "width"]
        ),
        s(
            "create_via",
            json!({"net": {"type": "string"}, "x": {"type": "integer"}, "y": {"type": "integer"}, "diameter": {"type": "integer"}, "drill": {"type": "integer"}}),
            vec!["net", "x", "y", "diameter", "drill"]
        ),
        s(
            "create_zone",
            json!({"net": {"type": "string"}, "layer": {"type": "string"}, "outline": {"type": "array", "items": point()}, "clearance": {"type": "integer"}, "min_width": {"type": "integer"}}),
            vec!["net", "layer", "outline"]
        ),
        s(
            "change_rule",
            json!({"rule": {"type": "string", "enum": ["min_track_width", "min_via_diameter", "min_via_drill", "min_annular_ring", "copper_to_edge", "clearance"]}, "value": {"type": "integer"}}),
            vec!["rule", "value"]
        ),
    ])
}

/// The tool catalogue returned by `tools/list`.
pub fn tools() -> Value {
    let none = json!({"type": "object", "properties": {}, "additionalProperties": false});
    let t = |name: &str, desc: &str, schema: Value| json!({"name": name, "description": desc, "inputSchema": schema});
    json!([
        t("get_project", "Project summary: name, counts, copper layers.", none.clone()),
        t("get_schematic", "Schematic objects and nets.", none.clone()),
        t("get_board", "Board: outline, footprints with pad positions and nets, tracks, vias, zones. Lengths in nm.", none.clone()),
        t("get_components", "Components with value, symbol, footprint and board placement.", none.clone()),
        t("get_nets", "Schematic nets with their pins, and the board nets.", none.clone()),
        t("get_rules", "Design rules and net classes.", none.clone()),
        t("get_violations", "Current ERC and DRC findings.", none.clone()),
        t("run_erc", "Run the electrical rules check on the connectivity graph.", none.clone()),
        t("run_drc", "Run the design rules check on the board geometry.", none),
        t("measure_net", "Routed length, vias, pads and unrouted connections of a board net.", json!({"type": "object", "properties": {"net": {"type": "string"}}, "required": ["net"]})),
        t("measure_track", "Length, width, layer and net of one track segment.", json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]})),
        t("search_components", "Search the local libraries (symbols, footprints, 3D models, components).", json!({"type": "object", "properties": {"query": {"type": "string"}, "kinds": {"type": "array", "items": {"enum": ["symbol", "footprint", "model", "component"]}}, "limit": {"type": "integer", "maximum": 200}}, "required": ["query"]})),
        t("check_buturescript", "Compile ButureScript source and return diagnostics (severity, code, line, column, message) and a design summary. Nothing is created.", json!({"type": "object", "properties": {"source": {"type": "string"}, "params": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["source"]})),
        t("propose_changes", "Propose changes. They are validated on a copy (diff, ERC/DRC before/after) and NOT applied: only the human user can apply a proposal.", json!({"type": "object", "properties": {"title": {"type": "string"}, "actions": {"type": "array", "minItems": 1, "items": {"oneOf": action_schemas()}}}, "required": ["title", "actions"]})),
        t("get_proposal", "Status, diff and checks of a proposal made earlier.", json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]})),
        t("list_proposals", "All proposals of this session with their status.", json!({"type": "object", "properties": {}, "additionalProperties": false})),
    ])
}

fn err(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn tool_ok(id: &Value, v: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}],
        "structuredContent": v, "isError": false,
    }})
}

fn tool_fail(id: &Value, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {
        "content": [{"type": "text", "text": message.into()}], "isError": true,
    }})
}

impl Server {
    pub fn new(project: Project, resolve: FootprintResolver) -> Self {
        Self {
            api: AgentApi::new(resolve),
            project,
            revision: 0,
            libs: Vec::new(),
            index: None,
            actor: "external-agent".into(),
            shutdown: false,
        }
    }

    /// Library folders searched by `search_components`.
    pub fn with_libraries(mut self, dirs: Vec<(String, LibraryDir)>) -> Self {
        self.libs = dirs;
        self
    }

    /// The Agent API behind the server (the host reads proposals and the audit log from it).
    pub fn api(&self) -> &AgentApi {
        &self.api
    }

    pub fn api_mut(&mut self) -> &mut AgentApi {
        &mut self.api
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn is_shut_down(&self) -> bool {
        self.shutdown
    }

    /// Handles one parsed message. `None` for notifications (no response is due).
    pub fn handle(&mut self, req: &Value) -> Option<Value> {
        let Some(obj) = req.as_object() else {
            return Some(err(
                &Value::Null,
                INVALID_REQUEST,
                "a request is a JSON object",
            ));
        };
        let id = obj.get("id").cloned();
        let rid = id.clone().unwrap_or(Value::Null);
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return id.map(|_| err(&rid, INVALID_REQUEST, "\"jsonrpc\" must be \"2.0\""));
        }
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return Some(err(&rid, INVALID_REQUEST, "missing \"method\""));
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "initialize" => {
                if let Some(n) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                    self.actor = n.chars().take(64).collect();
                }
                Some(json!({"jsonrpc": "2.0", "id": rid, "result": {
                    "protocolVersion": PROTOCOL,
                    "serverInfo": {"name": "buturecad", "version": buturecad_eda_core::VERSION},
                    "capabilities": {"tools": {"listChanged": false}},
                    "instructions": "Read the project, then call propose_changes. Proposals are never applied by tools: the user reviews the diff and applies or rejects. Lengths are integer nanometres.",
                }}))
            }
            "ping" => Some(json!({"jsonrpc": "2.0", "id": rid, "result": {}})),
            "tools/list" => Some(json!({"jsonrpc": "2.0", "id": rid, "result": {"tools": tools()}})),
            "tools/call" => Some(self.call_tool(&rid, &params)),
            "shutdown" => {
                self.shutdown = true;
                Some(json!({"jsonrpc": "2.0", "id": rid, "result": null}))
            }
            "exit" | "notifications/initialized" | "notifications/cancelled" => {
                if method == "exit" {
                    self.shutdown = true;
                }
                None
            }
            other => Some(err(&rid, METHOD_NOT_FOUND, format!("unknown method “{other}” (methods: initialize, ping, tools/list, tools/call, shutdown)"))),
        };
        // Notifications never get a response.
        if id.is_none() {
            None
        } else {
            reply
        }
    }

    /// One line in, one line out (`None` for notifications). Malformed JSON gets a parse error.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => self.handle(&v).map(|r| r.to_string()),
            Err(e) => Some(err(&Value::Null, PARSE_ERROR, format!("parse error: {e}")).to_string()),
        }
    }

    fn index(&mut self) -> &Index {
        let libs = &self.libs;
        self.index.get_or_insert_with(|| {
            let mut extra = Vec::new();
            for s in buturecad_schematic::builtin::symbols() {
                let short = s.name.rsplit(':').next().unwrap_or(&s.name).to_owned();
                let mut e = Entry::new(Kind::Symbol, "builtin", "Buture", &short);
                e.description = s.meta.description.clone();
                e.keywords = s.meta.keywords.clone();
                extra.push(e);
            }
            let refs: Vec<(&str, &LibraryDir)> =
                libs.iter().map(|(s, d)| (s.as_str(), d)).collect();
            Index::from_dirs(&refs, extra)
        })
    }

    fn call_tool(&mut self, id: &Value, params: &Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return err(
                id,
                INVALID_PARAMS,
                "tools/call needs {\"name\": …, \"arguments\": …}",
            );
        };
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !tools()
            .as_array()
            .is_some_and(|t| t.iter().any(|x| x["name"] == name))
        {
            return err(
                id,
                INVALID_PARAMS,
                format!("unknown tool “{name}”; see tools/list"),
            );
        }
        let actor = self.actor.clone();
        let (p, rev) = (self.project.clone(), self.revision);
        let result: Result<Value, String> = match name {
            "get_project" | "get_schematic" | "get_board" | "get_components" | "get_nets"
            | "get_rules" | "get_violations" => self
                .api
                .read(&p, rev, name, &actor)
                .map_err(|e: ApiError| e.to_string()),
            "run_erc" | "run_drc" | "measure_net" | "measure_track" => self
                .api
                .analyze(&p, name, &args, &actor)
                .map_err(|e| e.to_string()),
            "search_components" => {
                let Some(q) = args.get("query").and_then(Value::as_str) else {
                    return err(
                        id,
                        INVALID_PARAMS,
                        "search_components needs {\"query\": text}",
                    );
                };
                let kinds = args
                    .get("kinds")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .filter_map(|k| match k {
                                "symbol" => Some(Kind::Symbol),
                                "footprint" => Some(Kind::Footprint),
                                "model" => Some(Kind::Model3d),
                                "component" => Some(Kind::Component),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .min(200) as usize;
                let query = Query {
                    text: q.to_owned(),
                    kinds,
                    limit,
                    ..Query::default()
                };
                let hits: Vec<Value> = self
                    .index()
                    .search(&query)
                    .into_iter()
                    .map(|e| json!({"kind": e.kind.word(), "id": e.id, "name": e.name, "library": e.library, "description": e.description, "manufacturer": e.manufacturer, "mpn": e.mpn, "symbol": e.symbol, "footprint": e.footprint}))
                    .collect();
                self.api.audit.record(
                    "analyze",
                    &actor,
                    None,
                    json!({"analysis": "search_components", "query": q}),
                );
                Ok(json!({"results": hits}))
            }
            "check_buturescript" => {
                let Some(src) = args.get("source").and_then(Value::as_str) else {
                    return err(
                        id,
                        INVALID_PARAMS,
                        "check_buturescript needs {\"source\": text}",
                    );
                };
                let mut opts = buturecad_scripting::CompileOptions::default();
                if let Some(m) = args.get("params").and_then(Value::as_object) {
                    for (k, v) in m {
                        if let Some(v) = v.as_str() {
                            opts.params.insert(k.clone(), v.to_owned());
                        }
                    }
                }
                let out = buturecad_scripting::compile(src, &opts);
                self.api.audit.record(
                    "analyze",
                    &actor,
                    None,
                    json!({"analysis": "check_buturescript", "bytes": src.len()}),
                );
                Ok(json!({
                    "ok": !out.has_errors(),
                    "diagnostics": out.diagnostics.iter().map(|d| json!({
                        "severity": format!("{:?}", d.severity).to_lowercase(), "code": d.code, "message": d.message,
                        "line": d.span.line, "column": d.span.col, "length": d.span.len,
                    })).collect::<Vec<_>>(),
                    "design": out.design.as_ref().map(|d| json!({"name": d.name, "components": d.components.len(), "nets": d.nets.len(), "tracks": d.tracks.len(), "vias": d.vias.len(), "zones": d.zones.len()})),
                }))
            }
            "propose_changes" => {
                let title = args
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Proposal");
                let actions: Result<Vec<Action>, _> =
                    serde_json::from_value(args.get("actions").cloned().unwrap_or(Value::Null));
                match actions {
                    Ok(a) if !a.is_empty() => {
                        let prop = self.api.propose(&p, rev, title, a, &actor);
                        Ok(prop.to_json())
                    }
                    Ok(_) => {
                        return err(
                            id,
                            INVALID_PARAMS,
                            "propose_changes needs at least one action",
                        )
                    }
                    Err(e) => return err(id, INVALID_PARAMS, format!("invalid actions: {e}")),
                }
            }
            "get_proposal" => {
                let pid = args.get("id").and_then(Value::as_str).unwrap_or("");
                self.api
                    .proposal(pid)
                    .map(|p| p.to_json())
                    .ok_or_else(|| format!("no proposal “{pid}”"))
            }
            _ => Ok(
                json!({"proposals": self.api.proposals().iter().map(|p| json!({"id": p.id, "title": p.title, "status": p.status})).collect::<Vec<_>>()}),
            ),
        };
        match result {
            Ok(v) => tool_ok(id, v),
            Err(m) => tool_fail(id, m),
        }
    }
}

/// Serves requests line by line until EOF, `exit` or after `shutdown`. `after_each` runs after every
/// handled line (the host persists proposals and the audit log there).
pub fn serve<R: BufRead, W: Write>(
    server: &mut Server,
    input: R,
    mut output: W,
    mut after_each: impl FnMut(&Server),
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = server.handle_line(&line) {
            writeln!(output, "{resp}")?;
            output.flush()?;
        }
        after_each(server);
        if server.is_shut_down() {
            break;
        }
    }
    Ok(())
}
