# Feature audit (roadmap stage 48)

Compared with the flow expected of a modern EDA. **No feature was added** in the first step; the
only changes made while auditing are corrections of documentation that had drifted from reality and
of one stale end-to-end assertion and one command shown as available that the core refuses (listed under "What is actually broken").

**Method.** "Implemented" means something was *run*, not that code exists:

* the whole Rust workspace test suite (730+ tests, `cargo test --workspace`, including the real
  `ngspice 47` backend tests) and the frontend suite (36 tests);
* the **end-to-end harness** against the real Rust commands and real GPU renderer: all 12 scenarios
  pass (`apps/desktop/e2e`: shell, schematic editing, Symbol Editor, ERC, Footprint Editor, PCB,
  schematic→PCB, routing, zones, layer stack, 3D viewer, keyboard workflow);
* the **reference board** of stage 47, taken through script → ERC → DRC → Gerber/drill/BOM/pick-and-place
  → 3D with the real CLI and library code (`docs/validation-project.md`);
* reading each feature's own "limits" section and checking it against behaviour (several were stale).

Not exercised in this audit: interactive pointer-driven routing (push & shove, pairs, tuning) beyond
what the Rust workspace tests drive, any real fabrication run, Windows/macOS builds.

**Legend.** Test coverage: **Strong** (unit + integration/property, behaviour asserted), **Medium**
(integration tests of the main path), **Weak** (little beyond a smoke test), **None**.
Priority is for the *gap* named in the row: **High** blocks a real design flow, **Medium** costs a
lot of time, **Low** nice to have.

## Matrix

| Feature | Implemented | Partial | Missing | Broken | Test coverage | Priority |
|---|---|---|---|---|---|---|
| **Schematic capture** | components, wires, buses, junctions, labels, global labels, power symbols, no-connects, move/drag with rubber-banding, rotate/mirror, copy/paste, inspector; undo/redo | wire/junction editing is basic (no break/join tools listed); generated schematics (script) have no drawn wires | auto-annotation of references, find/replace, multiple-unit placement helpers, sheet/page borders and title block, PDF/SVG/print export | — | **Strong** (`schematic` 79, `eda-core` 65, desktop editor tests, e2e) | Medium (annotate, print) |
| **Hierarchical schematic** | sheet objects, sheet pins and hierarchical labels in the model; connectivity across sheets (tested) | sheets are drawn | **no tool or script statement to create or edit sheets**; ERC/PCB sync for hierarchy only exercised in tests | — | Medium (`connectivity.rs`) | High |
| **Symbols** | Symbol Editor: pins (all types), graphics, units, alternates, metadata; libraries | built-in sets: 14 bootstrap symbols plus the ~60-symbol `Std` library (stage 51) | no standard IC/connector library shipped; no import of other tools' symbols | — | **Strong** (symbol editor tests, e2e) | High (library content) |
| **ERC** | 21 rules on the connectivity graph, waivers, clickable findings | unused pins on scripted designs cannot be marked no-connect (found by the reference board) | pin-type matrix is fixed (not editable) | — | **Strong** (`rules/tests/erc.rs`, properties) | High (no-connect in scripts / CLI waivers) |
| **PCB core / editor** | outline, footprints, tracks, vias, zones, texts, graphics, layers, flip/rotate/lock/duplicate/align/distribute, 2–32 layers | board text and graphics tools minimal; keepouts have no drawing tool | PCB copy/paste, arc tracks, teardrops, curved outline editing, rule areas | — | **Strong** (`pcb` 30, desktop pcb tests, e2e) | Medium |
| **Footprints** | Footprint Editor; DIP/SOIC/QFN/BGA/header/chip generators; libraries; 3D model references | generators are nominal (verify vs datasheet); QFN had self-overlapping corners until stage 47 | LQFP/SOT/crystal/connector generators; IPC calculator; footprint wizard for gull-wing quads | — | Strong (generators + property-style overlap test) | High (more families) |
| **Routing** | manual routing with 45°/90°/free, walk-around, **push & shove**, net-aware, via while routing, one undo per route | router output between obstacles is free-angle (no 45° clean-up); failing searches can be slow (not profiled) | **autorouter**, headless route command, interactive tuning of routes by dragging corners with shove | — | Strong (`router` 26 + desktop `pcb_route`) | High (autorouter / 45° clean-up) |
| **Vias** | through vias, net-aware, sizes from net class, layer change while routing, DRC (diameter, drill, annular ring) | via kinds blind/buried/micro exist **in the model only** | creating blind/buried/micro vias, via-in-pad rules, via stitching/fence tools | — | Strong | Medium |
| **Multilayer** | 2–32 even copper layers, inner layers in zones/tracks/Gerber, plane vias (4-layer reference board works) | 3D view shows outer copper only | odd layer counts; layer-pair vias | — | Strong (stackup + reference board) | Low |
| **DRC** | clearance, short, track width, via diameter/drill, annular ring, copper-to-edge, outside board, unrouted, courtyard overlap, pair skew, length target, stale zone fill; reproducible, incremental == full (property-tested) | clearance only per net class | **keepout violations, hole-to-hole, silkscreen/mask rules, custom rules / rule areas, per-layer clearance** | — | **Strong** (`rules` 27 + properties, fuzz) | High (hole/keepout rules) |
| **Net classes** | presets, per-net assignment, limits, undo, persisted, used by router and DRC | five fixed presets plus editing | rule inheritance, per-class layer rules | — | Medium (desktop tests) | Low |
| **Differential pairs** | coupled routing, gap/width, skew reported and checked | single-layer or through-via pairs; partner pad is "nearest" | fan-out, polarity swap, phase tuning, impedance | — | Medium (`pcb_pair.rs`) | Medium |
| **Length matching** | per-net target, serpentine/accordion tuning, exact length, DRC check | meanders on one straight track only | length groups (match N nets), tuning through vias/arcs, interactive amplitude | — | Medium (`pcb_tune.rs`) | Medium |
| **Zones** | polygons, priority, clearance, thermal/solid/none, min width, island removal, exact fill, auto-refill, inner-layer planes (reference board) | stored fills enlarge files; zone-to-zone connection not modelled | hatched fills, keepout drawing tool, zone outline editing, per-layer duplication | — | Strong (zonefill + property tests) | Medium |
| **Stackup** | physical stack model, templates, presets, Layer Stack Manager | not used for impedance/3D/Gerber docs | impedance calculator, rigid-flex regions | — | Medium | Low |
| **3D** | software 3D view: substrate, mask, copper, pads, vias, zone fills, silkscreen graphics, component boxes; orbit/pan/zoom, presets; follows edits | component bodies are **courtyard boxes**; outer copper only | STEP/3D model rendering, section view, picking, anti-aliasing | — | Medium (`viewer3d` 9, desktop 3D, e2e, reference board) | Medium |
| **Mechanical exchange** | neutral model + backend trait; `.bmech` round trip; board → model | no real backend | STEP import/export, clearance checks against enclosure | — | Medium (`mcad` 9) | Medium |
| **Manufacturing** | Gerber (copper, mask, paste, silk, profile, inner layers), Excellon PTH/NPTH, BOM, pick-and-place, assembly info, preflight that blocks on errors, deterministic zip | silkscreen text (stage 52) is not checked against pads/edges | assembly drawings, ODB++/IPC-2581, panelisation, fiducials, per-fabricator rules, offset tables for assembly houses | — | **Strong** (`manufacturing` 29, reference board, fuzz + round-trip property) | Medium (silk checks, drawings) |
| **Gerber viewer** | independent Gerber/Excellon readers, 2D rasteriser, layer toggles, folder/zip loading, verification against the board | no anti-aliasing | measuring, net highlight, X2 attributes, mirrored bottom view | — | Strong (reader fuzz, round-trip property) | Low |
| **Libraries** | symbol/footprint/3D/component libraries, local inverted-index search, favourites, component editor; **scripts can use them** (stage 47) | placing a library component copies the **symbol only** (not footprint/value/MPN); library content is tiny | third-party library import, versioning, previews | — | Medium (`libraries` 18, desktop) | High (content, component placement) |
| **Simulation** | netlist from connectivity; OP/DC/AC/transient; built-in linear engine; real **ngspice** backend (47) for diodes/transistors/models | built-in engine linear only | cursors/measurements, CSV export, `.step`/`.param`, parameter sweeps UI | — | Medium (`simulation` 16, runs against ngspice) | Medium |
| **Plugins** | WASM sandbox (wasmi) with fuel/memory limits, no WASI, manifest, install/enable/remove, grant/revoke permissions, provider panel | no sample plugin set or registry; Importer/Exporter/Tool/Rule/Simulation plugin categories are not wired | plugin SDK crates/templates, signing | — | Medium (`plugin-runtime` 15 + fuzz) | Medium |
| **ButureScript** | lexer/parser/semantic analysis with units, modules, loops, functions, params, tracks/vias/zones/rules/classes, libraries, quoted net names, depth limits, fuzzed; Code/Split/Visual in the app | no sheets, no `noconnect`, pin names like `D+` need numbers, no `import` of other files, no diff-pairs/length targets/keepouts/text | round-trip merge of hand edits | — | **Strong** (`scripting` 22 incl. fuzz) | High (`noconnect`, sheets) |
| **CLI** | `build check erc drc fill export-gerber export-bom inspect import agent`, `--json`, exit codes, no GUI crates linked (tested) | `--json` position rule | `route`, `simulate`, `library`, `plugin`, `report` commands, shell completion | — | Medium (`cli` 12 incl. reference board) | Medium |
| **Agent API** | read/analyse/propose, validation on a copy, diff + ERC/DRC delta, human-only apply, audit log, JSON-RPC over stdio, examples run | no model ships; rule-based helper only | streaming progress, more action types | — | Strong (`agent-api` 13 + fuzz) | Low |
| **Project format / safety** | deterministic files, schema versions + migrations, atomic saves, backups, autosave, crash recovery, interrupted-save marker (fault-injected) | — | merge tooling for Git conflicts | — | **Strong** (`io` 47 + fuzz) | Low |
| **UX / keyboard** | one command registry, palette, repeat, previous tool, shortcut sheet, per-view context menus, conflict test | schematic align/distribute, PCB copy/paste, arrow nudge/pan missing (`docs/ux.md`) | keymap editor | — | Medium (35 frontend + e2e `ux.mjs`) | Medium |
| **Cross-probing / back-annotation** | ratsnest and net highlight inside the PCB; ERC/DRC findings select objects | — | **select in PCB from the schematic and vice versa; push PCB changes (swaps, re-annotation) back to the schematic** | — | None | High |
| **Import of other formats** | SPICE netlist, KiCad netlist with enforced accounting | — | KiCad/Altium/Eagle schematics and boards, Gerber import into the PCB | — | Medium (`io` importer tests + fuzz) | Medium |
| **Reports and documentation output** | ERC/DRC lists, assembly info, preflight | — | PDF/SVG schematic and fab drawings, design report | — | None | Medium |

## What is actually broken

No feature was found **broken** (producing wrong results) during this audit; everything exercised ran
and produced correct results. Small defects found and fixed in this stage:

| Defect | Kind | Fix |
|---|---|---|
| `apps/desktop/e2e/run.mjs` asserted that the 3D viewer "is not implemented yet" (true before stage 27) | stale test | now asserts the 3D controls are shown |
| `docs/drc.md` said zones are not checked and the router does not avoid obstacles | docs drift | corrected (zones are checked; keepouts are not) |
| `docs/manufacturing.md` said the Gerbers are never read back | docs drift | corrected |
| Copy / Cut were shown as available in the PCB editor but the core refuses them ("not available in the PCB editor") | UI honesty | now unavailable with a reason; unit test |

Known defects from earlier stages remain listed in `docs/STATUS.md` and `docs/validation-project.md`.

## Top gaps, in order

1. **Library content and component placement** — a real design needs ICs, connectors, regulators;
   placing a library component copies only the symbol.
2. **`noconnect` and waivers for scripted designs** — every unused pin is an ERC error today.
3. **Hierarchical sheets can be modelled but not created** (no tool, no script statement).
4. **Cross-probing and back-annotation** between schematic and PCB.
5. **Routing quality**: 45° clean-up and an automatic router; DRC rules for keepouts, hole-to-hole
   and silkscreen.
6. **Manufacturing completeness**: silkscreen text, BOM MPN/manufacturer, rotation offsets.
7. **Footprint families** (LQFP, SOT, crystals, connectors) and datasheet-verified land patterns.
8. **3D component models** (STEP) and PDF/SVG output.
