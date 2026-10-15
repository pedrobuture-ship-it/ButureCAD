# Working

*Checkpoint note (after stage 34): workspace compiles, `fmt`/`clippy -D warnings` clean, 637 Rust tests and 21 frontend tests pass, a search for TODO/FIXME/mock/placeholder/temporary/fake/stub found only documented items (`builtin.rs` bootstrap symbols, UI input placeholders). The e2e screenshot scenarios (`apps/desktop/e2e`) were not re-run for stages 28–34.*

*Checkpoint after roadmap stages 01–26; stages 27–52 added after it. Everything below is implemented, tested and used by the
desktop app; limits are stated in the sections that follow.*

- **Foundation:** Cargo workspace, Tauri 2 + React + TypeScript shell, CI (fmt, clippy `-D warnings`, tests, generated TS bindings, frontend check/build), proprietary licence (all rights reserved).
- **Project format** (`buturecad-io`): create/open/save/save-as, atomic writes, rotating backups, autosave, crash recovery, migration framework. Schemas: manifest 1, schematic 3 (v1→v2→v3 migrations), PCB 8 (v1→…→v8). See `docs/file-format/`.
- **Geometry engine** (`buturecad-geometry`): integer nanometres, exact predicates, nearest point, offset, rotation, R-tree index; unit and property tests.
- **Command Engine** (`buturecad-eda-core`): undo/redo, nested transactions, exact dirty state, history limit, autosave/recovery hooks; commands for schematic edits, symbols, footprints, ERC waivers, tracks, vias, rename; property tests (undo-all/redo-all is lossless).
- **Renderer** (`buturecad-renderer`): wgpu offscreen MSAA renderer shown in a canvas; pan/zoom, adaptive grid, snap, layers, picking, selection; verified on a real GPU (GPU tests skip without an adapter).
- **Schematic** (`buturecad-schematic`): symbols/pins (multi-unit, alternate representation, metadata), components, wires, buses, junctions, labels, power symbols, sheets, no-connects, validation, change sets. **Editor:** placement, wiring with automatic junctions, move/drag/rotate/mirror with rubber-banding verified against net topology, clipboard, inspector editing. See `docs/schematic-editor.md`.
- **Connectivity** (roadmap 10): per-sheet graphs merged by `ConnectivityEngine`; incremental updates rebuild only touched sheets and are property-tested against full rebuilds; power/global nets, hierarchy, buses, dangling wire ends. See `docs/schematic-connectivity.md`.
- **Symbol Editor** (roadmap 09): all required pin types, graphics, units, alternate representation, metadata; undoable; local symbol libraries (`*.bsymlib.json`). See `docs/symbol-editor.md`.
- **ERC** (roadmap 11): 21 rules on the connectivity graph, clickable findings (select + centre), documented waivers stored in the project. See `docs/erc.md`.
- **Footprint Editor** (roadmap 12): SMD/through-hole/NPTH/custom pads, five shapes, mask/paste/silkscreen/fabrication/courtyard graphics, reference/value text, DIP/SOIC/QFN/BGA/header generators, footprint libraries (`*.bfplib.json`), 3D model references (stored only). See `docs/footprint-editor.md`.
- **Desktop shell:** menus, toolbar, tool palette, document tabs, inspector, bottom panel (ERC, Messages, Console), status bar, dark/light themes, command palette, shortcuts from one command registry, context menu, dirty guard, recent projects, crash-recovery prompt.
- **Verification:** 522 Rust tests in 70 test binaries (unit, integration, property, IPC through Tauri's mock runtime), 21 frontend tests, and an end-to-end harness (`apps/desktop/e2e`: real Rust commands, real GPU renderer, headless Chromium, screenshots) with scenarios for the shell, schematic editing, Symbol Editor, ERC, Footprint Editor, PCB editor, schematic→PCB update, routing, copper zones and the layer stack manager. Checkpoint 26: workspace builds, `clippy -D warnings` clean, no TODO/FIXME/stub/mock in production code (matches are the documented temporary `Buture` bootstrap library and the 3D placeholder), all features of stages 19–26 exercised by tests.

- **PCB core** (roadmap 13): layer set and stackup (2–32 copper layers), placed footprints with exact flip/rotate transforms and pad geometry, nets, tracks/vias carrying nets, zones, keepouts, texts, graphics, via kinds modelled; generic atomic `PcbEditCommand`; PCB schema 3. See `docs/pcb-core.md`.

- **PCB editor** (roadmap 14): board outline (closure checked), footprint placement, move/rotate/flip/lock/delete/duplicate/align/distribute with multi-selection, layer manager with visibility and active layer, stackup selector; all rendered by wgpu and undoable. See `docs/pcb-editor.md`.

- **Schematic → PCB** (roadmap 15): preview-then-apply update of footprints, references, values and nets from the schematic; placement and routing preserved; renamed nets keep their tracks; one undo step. See `docs/schematic-to-pcb.md`.

- **Ratsnest** (roadmap 16): per-net minimum spanning trees over copper islands (layers, vias, back-side pads), incremental per-net updates (also live while dragging), show/hide; 33 ms full vs 1.9 ms per move on 4000 footprints. See `docs/ratsnest.md`.

- **Routing and vias** (roadmap 17–18): pad-to-pad routing with 45°/90°/free, width, active layer, preview with clearance halo, net highlight and targets, finish/cancel, net-aware (refuses shorts), through vias while routing and as a tool with automatic layer change, editing of width/net/layer/vertices, one undo step per route, ratsnest kept in step. See `docs/routing.md`.

- **DRC** (roadmap 19): clearance, shorts, track width, via diameter/drill/annular ring, copper-to-edge, outside board, unrouted connections, courtyard overlap on real geometry; incremental engine proven equal to a full run; clickable violations select the objects and zoom. See `docs/drc.md`.

- **Net classes** (roadmap 20): Default/Power/USB/Differential/HighCurrent presets, per-net assignment, board limits, undoable, persisted (PCB schema 4); the router takes width and via size from the net class and DRC its clearance.

- **Router walk-around** (roadmap 21): `buturecad-router` (collision, obstacles, A* path finding, DRC validation of the result); Walk around / Ignore obstacles modes, live preview with refusal reason, routes validated with the DRC rules before they are committed. See `docs/routing.md`.

- **Push & shove** (roadmap 22): existing tracks of other nets are displaced (with connected copper stretching and anchors pivoting), verified with the DRC rules, one undo step, automatic fallback to walk around; mode selectable in the bar (walk around / push & shove / ignore obstacles). See `docs/routing.md`.

- **Differential pairs** (roadmap 23): pair definitions integrated with net classes, coupled routing of both nets as one thick track with mitered legs, live length+/length−/difference/gap, two-via layer changes, one-undo commit validated with the DRC rules, pair gap used as the clearance between the legs, DRC `pair_skew`. PCB schema 5. See `docs/diff-pairs.md`.

- **Length tuning** (roadmap 24): per-net length targets (or the partner leg of a pair), live current/target/difference, serpentine and accordion meanders added exactly to the missing length, validated with the DRC rules, one undo step; DRC `length_target`. PCB schema 6. See `docs/length-tuning.md`.

- **Copper zones** (roadmap 25): zone tool, net, priority, clearance, thermal relief / solid / none, minimum width, island removal, keepouts; exact integer polygon fill (`i_overlay`), stored with a staleness signature, auto-refilled in the same undo step, ratsnest and DRC aware, filled/drawn by the renderer; 0.7 s for a 200×150 mm pour with 4600 obstacles. PCB schema 7. See `docs/zones.md`.

- **Layer stack** (roadmap 26): physical stack model (copper/dielectric thickness, material, core/prepreg, εr/tan δ, solder mask, flex flag), Layer Stack Manager with built-in templates (2/4/6/8 layers, HDI, rigid-flex) and local presets (`*.bstack.json`), inner-layer exchange, one undo step; PCB schema 8. See `docs/stackup.md`.

- **3D viewer** (roadmap 27): decoupled `buturecad-viewer3d` (model → mesh → software rasteriser, orbit/pan/zoom, perspective/orthographic); substrate, solder mask, copper, pads, vias, zone fills, silkscreen and component boxes; follows every PCB edit/undo; layer toggles and preset views. Component 3D models are referenced but not parsed (stage 28). See `docs/viewer3d.md`.
- **Manufacturing completeness** (stage 52, P0-2): silkscreen text (own stroke font, mirrored on the bottom, tested by read-back), BOM with MPN/Manufacturer/Supplier/SPN, pick-and-place rotation offsets from a file. Not done: silk-text collision checks, assembly drawings. See `docs/manufacturing.md`.
- **Standard library** (stage 51, P0-1): built-in `Std` library (symbols, footprints, components with MPN) usable in the app and in scripts; component placement brings footprint/MPN; quoted pin names in ButureScript. Land patterns are computed, **not datasheet-verified** (`docs/standard-library.md`).
- **Final audit** (roadmap 50): `docs/PRODUCTION_READINESS.md` — scores 3.5–7.5, verdict "working alpha, not for commercial boards", prioritised P0–P3 roadmap.
- **Release cleanup** (roadmap 49): see `docs/ALPHA_CLEANUP.md`. Windows and macOS are type-checked only, not tested.
- **Feature audit** (roadmap 48): `docs/FEATURE_AUDIT.md` compares ButureCAD with the expected modern EDA flow; nothing found broken, top gaps are library content, script no-connects, hierarchical sheet creation, cross-probing, router quality/autorouter, silkscreen text and BOM data.
- **Validation board** (roadmap 47): a 4-layer STM32 USB board runs script → ERC → DRC → Gerber/drill/BOM/pick-and-place; scripts can use user libraries, `buturecad fill`, size-aware schematic layout verified against connectivity, QFN generator fixed. Open findings (no-connect in scripts, no headless router, silkscreen text, BOM MPN) in `docs/validation-project.md`.
- **Performance and robustness** (roadmap 44–45): benchmark suite and measured fixes (build of a 5000-part design 1.7 s → 0.16 s, PCB zoom at 5000 parts 84 → 27 ms; see `benchmarks/README.md`); fuzzing of every parser, property tests for DRC/ERC, crash-safe project saving with an interrupted-save marker and fault injection (`docs/robustness.md`).
- **Agent protocol and importers** (roadmap 42–43): JSON-RPC 2.0 over stdio (`buturecad agent serve`, MCP-shaped tools; Rust/TypeScript/Python examples run in tests) with human-only apply; modular importers (SPICE netlist, KiCad netlist) with an Import Report whose accounting is enforced. See `docs/agent-api/protocol.md`, `docs/import.md`.
- **Agent API and AI Workspace** (roadmap 40–41): agents read, analyse and propose; proposals are validated on a copy with a state diff and ERC/DRC comparison; only the user's Apply (one undo step) changes the project; everything is audited. No model ships. See `docs/agent-api/README.md`.
- **CLI and headless core** (roadmap 38–39): `buturecad build/check/erc/drc/export-gerber/export-bom/inspect` with `--json` and exit codes; the whole script → Gerber flow runs with no GUI crate (tested, `scripts/headless-ci.sh`). See `docs/cli.md`.
- **ButureScript** (roadmap 35–37): `.beda` language (lexer, parser, AST, unit-aware semantic analysis, diagnostics, modules/functions/loops/conditions/parameters) → EDA IR → core through the Command Engine; CODE/SPLIT/VISUAL editor with live read-only preview and project generation. See `docs/buturescript/`.
- **Simulation** (roadmap 34): schematic connectivity → netlist; OP, DC sweep, AC, transient with the built-in linear engine (verified against analytic circuits); SPICE properties per component (Inspector); built-in V/I source symbols; plots in the UI. See `docs/simulation.md`.
- **Plugins and Component Provider API** (roadmap 32–33): WASM sandbox (wasmi) with fuel/memory limits and no WASI; manifest; install/enable/disable/reload/remove; explicit grant/revoke of `filesystem`, `project-read`, `network`, `project-write` with audit of refusals; provider panel with search and import into user libraries. See `docs/plugin-sdk/README.md`.
- **Library Manager** (roadmap 31): symbol, footprint, 3D model (manifest) and component libraries; local inverted-index search with filters, favorites and recents; component editor; place symbol. See `docs/library-manager.md`.
- **Gerber viewer** (roadmap 30): independent Gerber/Excellon readers + 2D rasteriser; layers, apertures, drill tools, toggles, zip/folder loading, verification of the generated files against the board. See `docs/gerber-viewer.md`.
- **Manufacturing export** (roadmap 29): Gerber (copper, mask, paste, silk, profile), Excellon PTH/NPTH, BOM, pick and place, assembly info, preflight with DRC, preview panel, deterministic `production.zip`; preflight errors block the zip. See `docs/manufacturing.md`.
- **Mechanical exchange layer** (roadmap 28): `buturecad-mcad` neutral model + backend trait + registry; `.bmech` round trip; board → model bridge and `mcad_export` command. See `docs/mcad.md`.

# Partial

- **Importers:** only netlist-level formats (no placement/board); the KiCad importer is untested against real KiCad exports; no GUI entry point. **Agent protocol:** stdio only, one client per process, not tested with a third-party MCP client.
- **Agent API:** `connect`/`disconnect` use labels/power symbols only; no model provider, no JSON-RPC protocol yet (roadmap 42); proposals live in memory; the decoupling helper's placement is simple.
- **ButureScript:** generation only (target project must be empty); no Visual → Code (designed in `docs/buturescript/code-visual.md`); no includes, no differential pairs/keepouts/text in the language; the schematic is auto-laid-out with global labels and no wires; no syntax highlighting.
- **Simulation:** the built-in engine is linear only; nonlinear devices need ngspice (backend verified against ngspice 47 for OP/AC/transient and a diode); no cursors/measurements/CSV export.
- **Plugins:** `network` can be granted/revoked but there is no network client (requests fail explicitly); `project-write` never edits the project (no proposal flow yet); only `ComponentProvider` has a defined method set; no plugin SDK crate/examples beyond test fixtures; no signature/trust verification of plugin files.
- **Manufacturing:** silkscreen text is not exported, no per-fabricator rules, no inner-plane negatives, no BOM supplier/MPN data; the Gerber viewer verifies positions/counts/drills but is not a third-party CAM check.
- **STEP/MCAD:** abstraction, neutral `.bmech` and Part 21 inspection work; STEP geometry import/export is not implemented (no kernel chosen, licence review pending); mechanical constraints are stored but not evaluated; no UI button for `mcad_export`; no import back into the board.
- **PCB:** the model, placement, routing and vias exist; routing walks around or pushes obstacles; keepouts have no drawing tool yet.
- **Libraries:** components and search exist (stage 31) but placing a component only places its symbol; no provider API yet (roadmap 33). The built-in `Buture` set is a small bootstrap library (R, C, L, D, LED, connectors, GND/+3V3/+5V/VCC, PWR_FLAG).
- **Router:** Walk Around avoids copper, keepouts and the board edge with the class clearance; Push & Shove displaces tracks of other nets (never vias or pads) and falls back to walking around. Detours and pushed tracks run at any angle (no 45° clean-up) and zones are not obstacles yet. Width and via size follow the net class.
- **Differential pairs:** skew is reported but not removed (length tuning is stage 24); legs are offsets of a single centre path (no independent gap changes or polarity swaps); push & shove does not apply to pairs.
- **Length tuning:** meanders only on a single straight track; no length groups, no coupled pair tuning, no interactive amplitude.
- **Layer stack:** not yet read by any other feature (3D, impedance, Gerber stackup are later stages); only through vias exist, so HDI/blind/buried/micro structures are modelled but not routable; no rigid-flex regions; even layer counts only.
- **Zones:** fills are stored in the file; thermal spokes are axis-aligned; zones are not router obstacles; no hatched fills; no tool to edit a zone outline after creation.
- **DRC:** the app runs a full check on demand (and marks it stale after edits); the incremental `DrcEngine::update` is not yet driven per edit. No hole-to-hole, silkscreen or mask rules.
- **Hierarchy:** only the root sheet can be drawn and edited; hierarchical sheets exist in the model and connectivity but have no UI tools. ERC findings inside child sheets are listed but cannot be focused.
- **Symbol edits and placed parts:** changing a symbol's pins moves the pins but not the wires attached to them in the schematic; ERC shows what became unconnected. There is no "update symbol from library" command: the project copy is authoritative once opened.
- **Wire re-routing** is not a general router: dense layouts can leave no valid style (reported as a warning).
- **Multi-file save** is not one atomic unit (the backup snapshot is the recovery path).
- **Footprint Editor:** pad rotation in quarter turns only; custom pads are edited as polygons in the inspector (no on-canvas vertex editor); mask/paste are shown at each pad's own margin; 3D references are stored, not rendered.
- **ERC:** no per-rule severity overrides or disabling; no hidden/implicit power pins.
- **3D viewer:** components are boxes with heuristic heights (no model parsing), outer copper only, software rasteriser without anti-aliasing, no picking or measuring.

# Broken

- Nothing known to be broken.

# Planned

- Roadmap stages 13–50 in `docs/ROADMAP.md`: PCB core and editor, schematic→PCB, ratsnest, tracks/vias, DRC, net classes, routing (walk-around, push & shove, differential, length tuning), zones, stackup, 3D, STEP, manufacturing, Gerber viewer, libraries/plugins/providers, SPICE, ButureScript, CLI/headless/Agent API, importers, performance, hardening, release.
- Polygon offset: mitered only, no topology resolution; no polygon holes/boolean operations.

# Technical Debt

- Connectivity merge phase is O(nets) per update (net ids are positions in a global order): measured 2.5× faster than a full rebuild on a 63k-object design, ~1 ms on typical sheets (`cargo run --release -p buturecad-schematic --example connectivity_bench`).
- `buturecad-libraries` depends on `buturecad-io` only for `atomic_write`; the helper could move to a smaller shared crate.
- The desktop crate's `workspace/` module is ~8.3 k lines; the schematic, symbol, footprint and PCB editors each carry their own select/move/draw state machine with similar structure. A shared item-editor abstraction is worth extracting before more editors (zones, 3D) add copies.
- The DRC panel and the Net Classes dialog are covered by IPC tests but have no end-to-end browser scenario yet.
- The end-to-end harness needs Chromium and a GPU, so it is not run in CI; React components are covered by it and by the command-registry tests only.
- Reserved crates (`manufacturing`, `simulation`, `viewer3d`, `scripting`, `plugin-runtime`, `agent-api`) are empty on purpose.
- `PinType::Unspecified` exists beyond the ten types the roadmap lists (kept for legacy and imported data).

# Known Bugs

- None open. Cosmetic: pin names and numbers are drawn horizontally even on vertical pins (no rotated text in the renderer yet).
