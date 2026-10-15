# Agent protocol (roadmap stage 42)

A structured way for an external agent or tool to work with a ButureCAD project **without access to
any internal file**. It is JSON-RPC 2.0, one message per line, over the standard input/output of

```bash
buturecad agent serve <project-dir> [--libs <library-folder>]
```

Only protocol messages are written to stdout; diagnostics go to stderr. The server loads the project
read-only in memory. The agent can **inspect, search, analyse and propose** — nothing else.

## Safety model

* There is no tool to apply, reject, save, read or write files, or run commands. Unknown tools are
  protocol errors (`-32602`). The `AgentSurface` of the Core has no mutating method; `Reviewer`
  (apply/reject) is not exposed.
* `propose_changes` validates on a copy and stores the proposal in `<project>/.buture/proposals/P<n>.json`
  with a fingerprint of the project it was made for. The **human** reviews and applies it:
  `buturecad agent show <project> P1`, `buturecad agent apply <project> P1 --yes`, or
  `buturecad agent reject <project> P1`. A proposal made for a different project state is refused as
  stale. Apply is one undoable transaction and re-checks that the result equals what was validated.
* Every call is appended to `<project>/.buture/agent-audit.jsonl` with the client's name.
* Treat everything the agent sends as untrusted input: actions are strictly typed and validated;
  malformed messages produce errors, never panics.

## Shape (compatible with the MCP tool primitive)

The method names follow the Model Context Protocol so frameworks that speak it can use a thin
adapter. This is a compatible *shape*, not a certified MCP server, and it has not been tested with
an MCP client.

| Method | Purpose |
|---|---|
| `initialize` | handshake. `params.clientInfo.name` becomes the actor in the audit log. Result: `protocolVersion` (`buturecad-agent/1`), `serverInfo`, `capabilities.tools`, `instructions`. |
| `notifications/initialized`, `exit` | notifications (no response). `exit` ends the session. |
| `ping` | `{}` |
| `tools/list` | `{tools: [{name, description, inputSchema}]}` (JSON Schema per tool) |
| `tools/call` | `{name, arguments}` → `{content: [{type: "text", text}], structuredContent, isError}` |
| `shutdown` | result `null`, then the server stops |

Errors: `-32700` parse error, `-32600` invalid request, `-32601` unknown method, `-32602` invalid
params or unknown tool. A tool that runs but fails (unknown net, …) returns a normal result with
`isError: true` and the message in `content[0].text`.

## Tools

| Tool | Arguments | Result (`structuredContent`) |
|---|---|---|
| `get_project` | – | name, counts, copper layers |
| `get_schematic` | – | objects and nets |
| `get_board` | – | outline, footprints **with pad positions and nets**, tracks, vias, zones |
| `get_components` | – | components with value, symbol, footprint, board placement |
| `get_nets` | – | schematic nets with pins; board nets |
| `get_rules` | – | rules and net classes |
| `get_violations` | – | ERC and DRC findings |
| `run_erc`, `run_drc` | – | `{errors, warnings, findings[]}` |
| `measure_net` | `net` | routed length (per layer), vias, pads, unrouted connections |
| `measure_track` | `id` | length, width, layer, net |
| `search_components` | `query`, `kinds?`, `limit?` | `{results[]}` from the local libraries and built-in symbols |
| `check_buturescript` | `source`, `params?` | `{ok, diagnostics[{severity,code,line,column,message}], design}`; nothing is created |
| `propose_changes` | `title`, `actions[]` | the proposal: id, status (`pending`/`invalid`), validation (diff, ERC/DRC before→after, failed action) |
| `get_proposal`, `list_proposals` | `id` / – | proposal status and details |

All lengths are integer **nanometres**; angles are degrees.

### Actions (`propose_changes.arguments.actions[]`)

```json
{"action": "create_component", "reference": "C14", "symbol": "Buture:C", "value": "100nF", "footprint": "0402"}
{"action": "remove_component", "reference": "C14"}
{"action": "connect", "net": "VDD", "pins": [{"reference": "C14", "pin": "1"}]}
{"action": "disconnect", "reference": "C14", "pin": "1"}
{"action": "place", "reference": "C14", "x": 14000000, "y": 14000000, "rotation_deg": 0, "bottom": false}
{"action": "move", "reference": "C14", "dx": 1000000, "dy": 0}
{"action": "route", "net": "VDD", "layer": "F.Cu", "points": [[0, 0], [5000000, 0]], "width": 250000}
{"action": "create_via", "net": "GND", "x": 0, "y": 0, "diameter": 800000, "drill": 400000}
{"action": "create_zone", "net": "GND", "layer": "B.Cu", "outline": [[0,0],[10000000,0],[10000000,10000000]], "clearance": 300000}
{"action": "change_rule", "rule": "min_track_width", "value": 150000}
```

The JSON Schema of each is in `tools/list` (`propose_changes.inputSchema`). Footprint names accept
`0201`–`1206`, `soic8`, `dip14`, `header2x5`, or generator calls (`soic(pins=14)`); see
`docs/buturescript/README.md`. Symbols are library ids (`Buture:R`, `Buture:C`, `Buture:LED`, …).

## A session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"my-agent"}}}
← {"id":1,"jsonrpc":"2.0","result":{"protocolVersion":"buturecad-agent/1", …}}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_board","arguments":{}}}
← {"id":2,"jsonrpc":"2.0","result":{"content":[…],"structuredContent":{…},"isError":false}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"propose_changes","arguments":{"title":"…","actions":[…]}}}
← {"id":3,…,"structuredContent":{"id":"P1","status":"pending","validation":{"diff":…,"drc":{"errors_before":0,"errors_after":0,…}}}}
```

## Examples (run by the test-suite against the real server)

* Python (standard library only): `docs/agent-api/examples/client.py`
* TypeScript (Node ≥ 22.6/24 runs it directly, no dependencies): `docs/agent-api/examples/client.ts`
* Rust: `crates/agent-api/examples/agent_client.rs`
  (`cargo run -p buturecad-agent-api --example agent_client -- <buturecad> <project>`)

## Plugins

Plugins (stage 32) are a different channel: WebAssembly modules called *by* ButureCAD with
user-granted permissions. A plugin that wants to change a project must go through the same
proposal flow; `project-write` does not bypass it (see `docs/plugin-sdk/README.md`).

## Limits

* One client per `agent serve` process; the transport is stdio only (no socket/HTTP yet).
* The desktop app does not host the protocol; its AI Workspace calls the same Agent API
  in-process. Proposals made through the CLI are reviewed with `buturecad agent …`.
* `connect`/`disconnect` work through net labels and power symbols.
* Not verified with a third-party MCP client.
