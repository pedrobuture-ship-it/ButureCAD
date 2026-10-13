# Working

*Checkpoint after roadmap stages 01–12. Everything below is implemented, tested and used by the
desktop app; limits are stated in the sections that follow.*

- **Foundation:** Cargo workspace, Tauri 2 + React + TypeScript shell, CI (fmt, clippy `-D warnings`, tests, generated TS bindings, frontend check/build), proprietary licence (all rights reserved).
- **Project format** (`buturecad-io`): create/open/save/save-as, atomic writes, rotating backups, autosave, crash recovery, migration framework. Schemas: manifest 1, schematic 3 (v1→v2→v3 migrations), PCB 2 (v1→v2). See `docs/file-format/`.
- **Geometry engine** (`buturecad-geometry`): integer nanometres, exact predicates, nearest point, offset, rotation, R-tree index; unit and property tests.
- **Command Engine** (`buturecad-eda-core`): undo/redo, nested transactions, exact dirty state, history limit, autosave/recovery hooks; commands for schematic edits, symbols, footprints, ERC waivers, tracks, vias, rename; property tests (undo-all/redo-all is lossless).
- **Renderer** (`buturecad-renderer`): wgpu offscreen MSAA renderer shown in a canvas; pan/zoom, adaptive grid, snap, layers, picking, selection; verified on a real GPU (GPU tests skip without an adapter).
- **Schematic** (`buturecad-schematic`): symbols/pins (multi-unit, alternate representation, metadata), components, wires, buses, junctions, labels, power symbols, sheets, no-connects, validation, change sets. **Editor:** placement, wiring with automatic junctions, move/drag/rotate/mirror with rubber-banding verified against net topology, clipboard, inspector editing. See `docs/schematic-editor.md`.
- **Connectivity** (roadmap 10): per-sheet graphs merged by `ConnectivityEngine`; incremental updates rebuild only touched sheets and are property-tested against full rebuilds; power/global nets, hierarchy, buses, dangling wire ends. See `docs/schematic-connectivity.md`.
- **Symbol Editor** (roadmap 09): all required pin types, graphics, units, alternate representation, metadata; undoable; local symbol libraries (`*.bsymlib.json`). See `docs/symbol-editor.md`.
- **ERC** (roadmap 11): 21 rules on the connectivity graph, clickable findings (select + centre), documented waivers stored in the project. See `docs/erc.md`.
- **Footprint Editor** (roadmap 12): SMD/through-hole/NPTH/custom pads, five shapes, mask/paste/silkscreen/fabrication/courtyard graphics, reference/value text, DIP/SOIC/QFN/BGA/header generators, footprint libraries (`*.bfplib.json`), 3D model references (stored only). See `docs/footprint-editor.md`.
- **Desktop shell:** menus, toolbar, tool palette, document tabs, inspector, bottom panel (ERC, Messages, Console), status bar, dark/light themes, command palette, shortcuts from one command registry, context menu, dirty guard, recent projects, crash-recovery prompt.
- **Verification:** 375 Rust tests in 28 test binaries (unit, integration, property, IPC through Tauri's mock runtime), 21 frontend tests, and an end-to-end harness (`apps/desktop/e2e`: real Rust commands, real GPU renderer, headless Chromium, screenshots) with scenarios for the shell, schematic editing, Symbol Editor, ERC and Footprint Editor.

- **PCB core** (roadmap 13): layer set and stackup (2–32 copper layers), placed footprints with exact flip/rotate transforms and pad geometry, nets, tracks/vias carrying nets, zones, keepouts, texts, graphics, via kinds modelled; generic atomic `PcbEditCommand`; PCB schema 3. See `docs/pcb-core.md`.

- **PCB editor** (roadmap 14): board outline (closure checked), footprint placement, move/rotate/flip/lock/delete/duplicate/align/distribute with multi-selection, layer manager with visibility and active layer, stackup selector; all rendered by wgpu and undoable. See `docs/pcb-editor.md`.

- **Schematic → PCB** (roadmap 15): preview-then-apply update of footprints, references, values and nets from the schematic; placement and routing preserved; renamed nets keep their tracks; one undo step. See `docs/schematic-to-pcb.md`.

- **Ratsnest** (roadmap 16): per-net minimum spanning trees over copper islands (layers, vias, back-side pads), incremental per-net updates (also live while dragging), show/hide; 33 ms full vs 1.9 ms per move on 4000 footprints. See `docs/ratsnest.md`.

- **Routing and vias** (roadmap 17–18): pad-to-pad routing with 45°/90°/free, width, active layer, preview with clearance halo, net highlight and targets, finish/cancel, net-aware (refuses shorts), through vias while routing and as a tool with automatic layer change, editing of width/net/layer/vertices, one undo step per route, ratsnest kept in step. See `docs/routing.md`.

# Partial

- **PCB:** the model, placement, routing and vias exist; routing does not avoid obstacles and clearance is only displayed (no DRC until roadmap 19); zones/keepouts have no tools (25).
- **Libraries:** one library per file; no component records (symbol + footprint + 3D + manufacturer data), no search index, no provider API (roadmap 31–33). The built-in `Buture` set is a small bootstrap library (R, C, L, D, LED, connectors, GND/+3V3/+5V/VCC, PWR_FLAG).
- **Hierarchy:** only the root sheet can be drawn and edited; hierarchical sheets exist in the model and connectivity but have no UI tools. ERC findings inside child sheets are listed but cannot be focused.
- **Symbol edits and placed parts:** changing a symbol's pins moves the pins but not the wires attached to them in the schematic; ERC shows what became unconnected. There is no "update symbol from library" command: the project copy is authoritative once opened.
- **Wire re-routing** is not a general router: dense layouts can leave no valid style (reported as a warning).
- **Multi-file save** is not one atomic unit (the backup snapshot is the recovery path).
- **Footprint Editor:** pad rotation in quarter turns only; custom pads are edited as polygons in the inspector (no on-canvas vertex editor); mask/paste are shown at each pad's own margin; 3D references are stored, not rendered.
- **ERC:** no per-rule severity overrides or disabling; no hidden/implicit power pins.
- **3D Viewer** and **DRC** are placeholders labelled as not implemented in the UI.

# Broken

- Nothing known to be broken.

# Planned

- Roadmap stages 13–50 in `docs/ROADMAP.md`: PCB core and editor, schematic→PCB, ratsnest, tracks/vias, DRC, net classes, routing (walk-around, push & shove, differential, length tuning), zones, stackup, 3D, STEP, manufacturing, Gerber viewer, libraries/plugins/providers, SPICE, ButureScript, CLI/headless/Agent API, importers, performance, hardening, release.
- Polygon offset: mitered only, no topology resolution; no polygon holes/boolean operations.

# Technical Debt

- Connectivity merge phase is O(nets) per update (net ids are positions in a global order): measured 2.5× faster than a full rebuild on a 63k-object design, ~1 ms on typical sheets (`cargo run --release -p buturecad-schematic --example connectivity_bench`).
- `buturecad-libraries` depends on `buturecad-io` only for `atomic_write`; the helper could move to a smaller shared crate.
- The desktop crate's `workspace/` module is ~6.4 k lines; the schematic, symbol and footprint editors each carry their own select/move/draw state machine with similar structure. A shared item-editor abstraction is worth extracting before the PCB editor adds a fourth copy.
- The end-to-end harness needs Chromium and a GPU, so it is not run in CI; React components are covered by it and by the command-registry tests only.
- Reserved crates (`router`, `manufacturing`, `simulation`, `viewer3d`, `scripting`, `plugin-runtime`, `agent-api`) are empty on purpose.
- `PinType::Unspecified` exists beyond the ten types the roadmap lists (kept for legacy and imported data).

# Known Bugs

- None open. Cosmetic: pin names and numbers are drawn horizontally even on vertical pins (no rotated text in the renderer yet).
