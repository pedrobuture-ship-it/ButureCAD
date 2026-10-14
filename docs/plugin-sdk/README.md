# Plugin SDK (roadmap stages 32–33)

Plugins are **WebAssembly modules** run by [`wasmi`](https://github.com/wasmi-labs/wasmi) (pure Rust
interpreter) inside `buturecad-plugin-runtime`. They work completely offline. This document
describes API version **1**.

## Package

A folder with two files; users install it with *Plugins → Install from file…* (choose `plugin.json`):

```json
{
  "format": "buturecad-plugin",
  "api": 1,
  "id": "acme.parts",
  "name": "Acme Parts",
  "version": "1.0.0",
  "description": "optional",
  "categories": ["ComponentProvider"],
  "permissions": ["project-read"],
  "wasm": "plugin.wasm"
}
```

`id`: `a-z 0-9 . - _`, ≤ 64. `categories`: `ComponentProvider`, `SymbolProvider`, `FootprintProvider`,
`Model3dProvider`, `Importer`, `Exporter`, `RulePlugin`, `ToolPlugin`, `SimulationPlugin` (only
`ComponentProvider` has a defined method set today; the others can be declared and called through
the generic `call` but the host does not drive them yet). `permissions` is what the plugin *asks
for*; nothing is granted at install.

## Module ABI

Exports: `memory`; `alloc(len: i32) -> i32`; `call(ptr: i32, len: i32) -> i64`.
`call` receives `{"method": "...", "params": ...}` at `ptr..ptr+len` and returns
`(out_ptr << 32) | out_len` pointing to a JSON response in its memory (`0` = no response).
Imports (module `buturecad`, nothing else — in particular no WASI):

* `log(ptr, len)` — a log line (shown nowhere by default, kept for auditing; max 200 lines).
* `host_request(ptr, len) -> i64` — JSON `{"op": ..}` request, JSON reply allocated with the
  module's own `alloc`, same packed return (`0` on failure).

A **fresh instance is created for every call**: plugins keep no state between calls, and a revoked
permission takes effect on the next call. Limits per call: 200 M wasm instructions (fuel), 64 MiB
memory, 8 MiB per message. A plugin that traps, runs out of fuel, returns invalid JSON or points
outside its memory yields an error, never a crash of the application.

## Host operations and permissions

| `op` | needs | behaviour |
|---|---|---|
| `project.info` | `project-read` | `{"ok": {"name": "<project name>"}}` |
| `fs.read` `{path}` | `filesystem` | reads a UTF-8 text file ≤ 8 MiB, only inside the open project folder |
| `net.get` `{url}` | `network` | **not implemented in this build**: replies with an explicit error even when granted |
| `project.propose` | `project-write` | **refused**: plugins never edit the project directly (the proposal/diff/approval flow of CLAUDE.md §27 does not exist yet) |

Without the permission the reply is `{"error": "permission denied: <perm>"}` and the request is
recorded in the call's audit trail (shown in the provider panel).

## ComponentProvider (stage 33)

Responses are `{"result": ...}` or `{"error": "message"}`.

| method | params | result |
|---|---|---|
| `search` | `{query, limit}` | `[{id, name, description?, manufacturer?, mpn?}]` (≤ 200) |
| `get_component` | `{id}` | `{name, description?, symbol?, footprint?, model3d?, manufacturer?, mpn?, datasheet?, parameters?, tags?}` |
| `get_symbol` / `get_footprint` | `{id}` | a ButureCAD symbol / footprint JSON; validated by the host library code, stored only if valid |
| `get_model` | `{id}` | `{name, format?, path?, url?}` — a reference; the host never downloads it |
| `get_datasheet` | `{id}` | `{path?, url?}` — a reference, never opened automatically |
| `get_metadata` | `{id}` | `{key: value}` |

The core does not know any supplier. *Import* in the provider panel writes the component (and the
symbol/footprint when valid) into a **user library** through the normal library code, so the
result is a plain local file that works offline from then on.

## Manager

*Install from file*, *Enable / Disable*, *Reload* (re-reads the module from disk), *Remove*, and
a checkbox per requested permission (grant / revoke). A plugin must load in the sandbox before it
can be installed or enabled; broken installed plugins are listed with their error and never stop
the application. State is `<app data>/plugins/state.json`.

## Writing a plugin

Any language that compiles to `wasm32-unknown-unknown` works (Rust: `cdylib`, export the three
functions, `#[link(wasm_import_module = "buturecad")]` for the imports). There is no example SDK
crate yet; the runtime tests contain complete WAT fixtures (`crates/plugin-runtime/src/*`).
