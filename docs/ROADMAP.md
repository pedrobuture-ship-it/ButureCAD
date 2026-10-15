# Roadmap

The detailed prompt roadmap is `promptsbuturecad.md` (50 stages). Status is tracked in
`docs/STATUS.md`; this page only says which stages exist in the code.

| Stage | Topic | State |
|---|---|---|
| 01 | Foundation | done |
| 02 | Project format | done |
| 03 | Command Engine | done |
| 04 | Geometry engine | done |
| 05 | GPU renderer | done |
| 06 | Professional shell | done |
| 07 | Schematic core | done |
| 08 | Schematic editor | done |
| 09 | Symbol Editor | done (`docs/symbol-editor.md`) |
| 10 | Connectivity engine | done (`docs/schematic-connectivity.md`) |
| 11 | ERC | done (`docs/erc.md`) |
| 12 | Footprint Editor | done (`docs/footprint-editor.md`) |
| 13 | PCB core | done (`docs/pcb-core.md`) |
| 14 | PCB editor | done (`docs/pcb-editor.md`) |
| 15 | Schematic → PCB | done (`docs/schematic-to-pcb.md`) |
| 16 | Ratsnest | done (`docs/ratsnest.md`) |
| 17–18 | Tracks, vias and layer change | done (`docs/routing.md`) |
| 19–20 | DRC engine, net classes | done (`docs/drc.md`) |
| 21 | Router walk-around | done (`docs/routing.md`) |
| 22 | Push & shove | done (`docs/routing.md`) |
| 23 | Differential pairs | done (`docs/diff-pairs.md`) |
| 24 | Length tuning | done (`docs/length-tuning.md`) |
| 25 | Copper zones | done (`docs/zones.md`) |
| 26 | Layer stack manager | done (`docs/stackup.md`) |
| 27 | PCB 3D viewer | done (`docs/viewer3d.md`) |
| 28 | STEP/MCAD abstraction layer | partial (`docs/mcad.md`): architecture, `.bmech` and Part 21 inspection; STEP geometry import/export not implemented |
| 29 | Manufacturing export | done (`docs/manufacturing.md`) |
| 30 | Gerber viewer | done (`docs/gerber-viewer.md`) |
| 31 | Local libraries / Library Manager | done (`docs/library-manager.md`) |
| 32 | Plugin system (WASM sandbox, permissions, manager) | done (`docs/plugin-sdk/README.md`); network/project-write permissions have no backend yet |
| 33 | Component Provider API | done (`docs/plugin-sdk/README.md`) |
| 34 | Simulation architecture (SPICE) | done (`docs/simulation.md`): built-in linear engine + ngspice backend verified against ngspice 47 |
| 35 | ButureScript (lexer, parser, AST, semantic analysis, diagnostics, compiler → EDA IR → core) | done (`docs/buturescript/README.md`) |
| 36 | Parametric ButureScript (variables, functions, modules, loops, conditions, units) | done |
| 37 | Code ↔ Visual | partial (`docs/buturescript/code-visual.md`): Code → Visual (CODE/SPLIT/VISUAL, live preview, diagnostics) done; Visual → Code designed, not implemented |
| 38 | CLI (`buturecad`) | done (`docs/cli.md`) |
| 39 | Headless core | done (`docs/cli.md`): script → gerber with no GUI crate, checked by test and `scripts/headless-ci.sh` |
| 40 | Agent API | done (`docs/agent-api/README.md`) |
| 41 | AI Workspace | done: panel, provider interface, rule-based decoupling helper and JSON provider; no model provider ships |
| 42 | Plugin/Agent protocol | planned |
| 43–50 | Import, performance, robustness, UX, validation project, audits, release | planned |
