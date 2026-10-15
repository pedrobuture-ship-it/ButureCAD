<h1 align="center">ButureCAD</h1>

<p align="center">
  <strong>Design visually. Design with code. Automate when useful.<br>Keep the engineer in control.</strong>
</p>

<p align="center">
  A local-first, offline-first electronic design automation (EDA) application:<br>
  schematic capture, PCB layout, interactive routing, ERC/DRC, libraries, simulation, 3D and manufacturing output,<br>
  on a deterministic Rust core with a Tauri 2 + React desktop app and a headless CLI.
</p>

---

## Why ButureCAD

- **Local-first.** Your projects are plain files on your disk. No account, no cloud, no telemetry, no server. It works with the network unplugged.
- **One core, many clients.** The Rust core is the single source of truth. The desktop app, the `buturecad` CLI, the ButureScript language and optional AI agents are all clients of the same code, so a design behaves the same everywhere.
- **Real engineering, not a mock-up.** A track on screen is a real `Track` object. DRC runs on the actual copper geometry, ERC on the connectivity graph, Gerber is written from the board model and read back by an independent parser. If something is incomplete, the docs say so.
- **Deterministic and exact.** Integer nanometre coordinates, deterministic project files (Git-friendly), atomic saves with backups and crash recovery.
- **Scriptable.** Describe a board in **ButureScript** (`.beda`), a unit-aware language where `10mm + 5V` is a compile error, and get the same model the visual editor edits.
- **Safe with AI.** Agents can read, analyse and *propose*; only you can apply, in one undo step, after seeing a diff and the ERC/DRC delta. No model ships with the app.

## What is in the box

| Area | What works today |
|---|---|
| **Schematic** | Symbols (multi-unit, alternates), wires, buses, junctions, labels, power symbols, hierarchy, no-connects, rubber-banding, clipboard, Symbol Editor |
| **ERC** | 21 rules on the connectivity graph, clickable findings, documented waivers |
| **PCB** | Outline, footprints (DIP/SOIC/QFN/BGA/header generators + libraries), tracks, vias, copper zones with exact polygon fill, 2–32 layer stack-up, net classes, ratsnest |
| **Routing** | Walk-around and Push & Shove, differential pairs, length tuning, all validated with the DRC rules before they are committed |
| **DRC** | Clearance, shorts, widths, vias, annular ring, copper-to-edge, courtyards, unrouted nets, pair skew, length targets |
| **Manufacturing** | Gerber, Excellon, BOM, pick-and-place, preflight that blocks export on errors, an independent Gerber viewer |
| **3D** | Board viewer with substrate, mask, copper, silkscreen and component bodies; mechanical exchange layer |
| **Libraries** | Symbol / footprint / 3D / component libraries, fast local search, favourites |
| **Simulation** | Netlist extraction and OP / DC / AC / transient analysis |
| **ButureScript** | Lexer → parser → semantic analysis → EDA IR → core; modules, loops, parameters, units, Code / Split / Visual views |
| **CLI** | `build`, `check`, `erc`, `drc`, `fill`, `export-gerber`, `export-bom`, `inspect`, `import`, `agent` — no GUI code linked |
| **Plugins** | WASM sandbox with fuel and memory limits, explicit permission grants, component providers |
| **Agent API** | JSON-RPC 2.0 over stdio, MCP-shaped tools, proposals with diff and audit log |
| **Quality** | Fuzzing of every parser, property tests for geometry/DRC/ERC, crash-safety tests with fault injection, benchmark suite up to 5000 parts |

See [`docs/STATUS.md`](docs/STATUS.md) for the honest list of what is **Working**, **Partial**, **Broken**, **Planned** and **Known bugs**.

> **Status: working alpha.** See [`docs/PRODUCTION_READINESS.md`](docs/PRODUCTION_READINESS.md): good engineering foundations, but libraries are nearly empty and nothing has been through a fabricator — not yet suitable for commercial boards. ButureCAD is under active development. A successful export is **not** a fabrication sign-off: always review the DRC report and the manufacturing preflight, and check land patterns against the datasheets of the parts you use.

## Quick start

Requirements: a recent stable Rust toolchain (`mise install` reads `mise.toml`), Node.js and npm, and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/).

```bash
# run the whole Rust test suite
cargo test --workspace

# run the desktop app
cd apps/desktop
npm install
npm run tauri dev
```

On some Wayland + NVIDIA systems the web view aborts with `Gdk-Message: Error 71 (Protocol error)`; start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev`.

### From code to Gerber, without a window

`examples/chain.beda` describes three resistors in a chain, routed on the top layer:

```text
project "chain"
board { outline = rect(0mm, 0mm, 30mm, 20mm)  layers = 2 }

component R1 { value = "1k"  footprint = "0603" }
component R2 { value = "2k2" footprint = "0603" }
component R3 { value = "4k7" footprint = "0603" }

net A { R1.2, R2.1 }
net B { R2.2, R3.1 }
connect R1.1 -> IN
connect R3.2 -> OUT

place R1 at (10mm, 10mm)
place R2 at (14mm, 10mm)
place R3 at (18mm, 10mm)

track A on F.Cu width 0.25mm path (10.825mm, 10mm) -> (13.175mm, 10mm)
track B on F.Cu width 0.25mm path (14.825mm, 10mm) -> (17.175mm, 10mm)
```

```bash
cargo build -p buturecad-cli
buturecad build examples/chain.beda --out /tmp/chain     # script -> project, through the Command Engine
buturecad erc /tmp/chain                                 # electrical rules on the connectivity graph
buturecad drc /tmp/chain                                 # design rules on the real copper geometry
buturecad export-gerber /tmp/chain --out /tmp/chain.zip  # refuses to write when preflight has errors
```

Every command supports `--json`. Exit codes: `0` ok, `1` design problems, `2` usage or I/O problems.

## Architecture

```text
                  ButureCAD
                      │
          ┌───────────┴───────────┐
          │                       │
     Desktop GUI             buturecad-cli
          │                       │
          └───────────┬───────────┘
                      │
              ButureCAD Core (Rust)
                      │
      ┌───────────────┼────────────────┐
 Schematic           PCB             Rules
      │            Router          ERC / DRC
      └───────────────┼────────────────┘
                    EDA IR
        ┌─────────────┼─────────────┐
   ButureScript    Plugins      Agent API
```

- **The GUI is not the core.** React owns windows, panels, menus and UI state. Geometry, connectivity, routing, ERC/DRC, serialization and manufacturing live in Rust.
- **Everything goes through the Command Engine**, which gives undo/redo, transactions, history and AI change review.
- **Rendering** is done with wgpu, not thousands of DOM nodes.

More in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Repository layout

```text
apps/desktop     Tauri 2 + React + TypeScript app (the Rust side is src-tauri)
apps/cli         the `buturecad` command line
crates/          geometry, eda-core, schematic, pcb, router, rules, renderer,
                 libraries, manufacturing, simulation, viewer3d, io, mcad,
                 scripting, plugin-runtime, agent-api, testkit
docs/            architecture, status, roadmap, file format, per-feature guides
examples/        ButureScript examples
benchmarks/      reproducible performance suite and results
scripts/         headless CI script
```

## Documentation

| | |
|---|---|
| [`docs/STATUS.md`](docs/STATUS.md) | What works, what is partial, known bugs |
| [`docs/PRODUCTION_READINESS.md`](docs/PRODUCTION_READINESS.md) | Final audit: scores, what is ready, what is missing, prioritised roadmap |
| [`docs/FEATURE_AUDIT.md`](docs/FEATURE_AUDIT.md) | Feature matrix compared with a modern EDA flow |
| [`docs/validation-project.md`](docs/validation-project.md) | A real 4-layer board taken from script to Gerber, with findings |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Where the project is going |
| [`docs/buturescript/`](docs/buturescript/README.md) | The language |
| [`docs/cli.md`](docs/cli.md) | The command line and the headless guarantee |
| [`docs/file-format/`](docs/file-format) | Project files, schemas and migrations |
| [`docs/ux.md`](docs/ux.md) | Keyboard workflow, command palette, shortcuts |
| [`docs/robustness.md`](docs/robustness.md) | Fuzzing, crash safety, atomic writes |
| [`benchmarks/README.md`](benchmarks/README.md) | Performance numbers and methodology |
| [`docs/agent-api/`](docs/agent-api/README.md) | Agent protocol and safety model |
| [`docs/plugin-sdk/`](docs/plugin-sdk/README.md) | Writing plugins |

## Principles

Never fake a feature. Never let a placeholder become architecture. Prefer *small, correct, tested* over *large, fake, fragile*. The full rules are in [`CLAUDE.md`](CLAUDE.md).

## License

Proprietary software, all rights reserved. See [`LICENSE`](LICENSE).
