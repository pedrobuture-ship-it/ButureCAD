# Production readiness: final audit (roadmap stage 50)

Written as a senior EDA engineer, Rust engineer, PCB designer and desktop architect would write it,
and **without equivalence claims**. No feature was implemented for this audit.

> **Conflict of interest.** The code, the tests and this audit come from the same development
> process. Scores below are conservative on purpose, but an independent review (another engineer,
> a real fabrication run, real users) is still required before anyone relies on ButureCAD.

**Evidence used:** 734 Rust tests and 36 frontend tests (all pass), 12 end-to-end scenarios in a real
browser against the real Rust commands and GPU renderer, the benchmark suite up to 5 000 parts, fuzzing
of every parser, the stage-47 reference board taken from script to Gerber, and the stage-48 feature
audit (`docs/FEATURE_AUDIT.md`). Size: about 70 000 lines of Rust in 19 crates, 8 000 lines of
TypeScript. Never exercised: a real fabrication run, Windows and macOS builds of the app, any user
other than the developer, boards beyond the synthetic benchmarks and one small reference design.

## Verdict

**ButureCAD is a working, honest alpha.** Its engineering foundations (integer geometry, command
engine, deterministic crash-safe files, a verified headless pipeline, a safe agent model) are good.
Its **design-flow completeness** is not: libraries are nearly empty, a handful of everyday EDA
workflows are missing, and nothing has been through a fabricator. **A small hobby board can be
completed today with workarounds** (the reference board proves it). **A commercial PCB should not
be designed with it yet.**

## Scores (0–10)

Scale: 10 = comparable to a mature professional tool; 5 = usable for simple work with real gaps;
0 = absent. "Professional tools" means the class of KiCad / Altium / OrCAD / Allegro.

| Area | Score | Why |
|---|---:|---|
| **Architecture** | **7.5** | Rust core as source of truth, directional crate graph, Command Engine for every mutation, EDA IR shared by script/agent/plugins, headless CLI links no GUI crate (tested). Weak points: the interactive editing logic (tools, pointer handling, routing UX, 23 k lines) lives in the desktop crate, so the CLI and agents cannot drive interactive tools; the IR can only *generate into an empty project* (no merge of hand edits); file format is JSON per document. |
| **Performance** | **7** | Measured, not assumed: 5 000-part design builds in 0.16 s, DRC 0.17 s, load 0.07 s, Gerber export 0.2 s, 108 MB; GPU renderer at 25–40 fps zooming a 5 000-part board; two real bottlenecks found and fixed. Limits: only synthetic large boards; zoom re-tessellates per zoom bucket; software 3D; failing router searches can take seconds (not profiled). |
| **Schematic** | **5** | Real connectivity graph, editing, symbol editor, hierarchy in the model. Missing: auto-annotation, creating sheets, print/PDF, find/replace; the built-in symbol set is 14 parts; generated schematics have no drawn wires. |
| **PCB** | **5.5** | Real geometry for every object, 2–32 layers, planes, flip/lock/align, 3D follow. Missing: copy/paste, arc tracks, teardrops, rule areas, curved outlines, keepout drawing, panelisation; land patterns are generated or hand-made and unverified. |
| **Routing** | **5** | Walk-around, push & shove, differential pairs, length tuning are real and re-validated with the DRC rules. Missing: autorouter, 45° clean-up (diagonals everywhere), multi-segment tuning, fan-out, impedance control; no headless routing; speed on failure unknown. |
| **ERC** | **6.5** | 21 rules on the connectivity graph, waivers, tested and property-checked. Gap: scripted designs cannot mark no-connects (31 false errors on the reference board); pin matrix not editable. |
| **DRC** | **6** | Reproducible, incremental engine proven equal to the full run, exact integer geometry, zones as copper. Missing: keepout, hole-to-hole, silkscreen, mask sliver, drill-to-copper rules, per-layer/area/custom rules, fabricator presets. A passing DRC is *not* a manufacturability check. |
| **Libraries** | **3.5** | Local, fast, modular infrastructure (symbol/footprint/3D/component, search, scripts can use them). Content is essentially absent; placing a component copies only its symbol; no import of other libraries; no verified land patterns. |
| **3D** | **3.5** | Deterministic software viewer: substrate, mask, copper, silk graphics, zones. Components are courtyard boxes; no STEP/models, no inner layers, no picking. Useful as a sanity view only. |
| **Manufacturing** | **5.5** | Correct, deterministic Gerber/Excellon/BOM/pick-and-place; inner layers; preflight blocks on errors; outputs re-read by independent parsers. Missing: silkscreen text, BOM MPN/supplier, rotation offsets, ODB++/IPC-2581, fiducials, panels. Never checked by a fabricator's CAM. |
| **Simulation** | **5** | OP/DC/AC/transient from the real netlist; verified against analytic circuits and a real ngspice. Built-in engine is linear only (nonlinear needs the user's ngspice); no measurements/cursors/export; no model library management. |
| **Plugins** | **5** | Real WASM sandbox (fuel, memory, no WASI), explicit permissions with audit, provider panel, fuzzed. Only component providers are wired; no SDK templates, signing or registry; ecosystem is zero. |
| **ButureScript** | **6.5** | A real language with dimensional analysis, modules, loops, functions, libraries, depth/step limits, fuzzed, shared model with the GUI. Gaps: no sheets, no no-connect, no pairs/length/keepouts/text, no merge with hand edits, a few syntax limits (`D+`). |
| **Agent API** | **7** | Strong safety design: read/analyse/propose, validation on a copy, diff and ERC/DRC delta, human-only apply, audit log, JSON-RPC over stdio, fuzzed. No model ships; action vocabulary is small. |
| **UX** | **5** | Keyboard-first design: one command registry, palette, repeat/previous tool, shortcut sheet, per-view menus, conflict-tested. But: several everyday ops missing (PCB copy/paste, schematic align, nudge), never used by anyone else, no in-app help/onboarding, Windows/macOS look unverified. |
| **Reliability** | **7** | Atomic saves with backups and an interrupted-save marker (fault-injected at every stage), crash recovery, fuzzing of every parser with defects found and fixed, ngspice hang and leak fixed, `unsafe` forbidden, no unwrap/panic in production code. Unproven at scale and in the field; Windows/macOS untested; durability of saves on non-Unix is weaker. |

## Comparison with professional EDA (conceptual, no equivalence claimed)

| Workflow step | Professional tools | ButureCAD today |
|---|---|---|
| Libraries | Large curated, verified libraries; supplier integration | Infrastructure only; nominal generators; **far behind** |
| Schematic | Hierarchy, annotation, variants, cross-probing, ERC with configurable matrix, PDF output | Core editing and ERC; **no hierarchy creation, annotation, cross-probing, output** |
| Schematic → PCB | Bidirectional ECO, back-annotation, pin/gate swap | One-way "Update PCB" with preview; **no back-annotation** |
| Placement | Rooms, alignment, interactive placement tools, 3D clearance | Move/align/distribute/flip/lock; adequate for small boards |
| Routing | Autorouter or strong interactive router, 45°, length/impedance/diff-pair constraints, fan-out | Real but basic interactive routing; **no autorouter, rough geometry** |
| Rules | Constraint manager, rule areas, SQL-like custom rules, fab presets | Net-class rules; **no custom rules or fab presets** |
| DRC | Full manufacturing rule set | Core electrical/copper rules; **missing hole/mask/silk/keepout** |
| Zones | Hatched, thermal tuning, zone manager | Exact solid fills; adequate for planes |
| 3D | STEP models, collision with enclosure | Boxes; **far behind** |
| Output | Gerber X2, ODB++, IPC-2581, drill, BOM with supplier data, assembly drawings, PDF | Gerber/Excellon/BOM/pos; **no text on silk, BOM data, drawings** |
| Simulation | Integrated SPICE, SI/PI tools | OP/AC/tran + external ngspice; **no SI/PI** |
| Scripting | Python APIs | **ButureScript (unique strength)**, CLI, agent API |
| Reliability | Decades of field use | Strong engineering, **no field use** |

Differentiators that are real: local-first single binary, deterministic Git-friendly files,
unit-checked code-first design, a headless pipeline identical to the GUI, and a safe agent model.

## 1. Production-ready (for a Linux alpha, with the caveats named)

* **Project files:** deterministic, schema-versioned with migrations, atomic saves, rotating backups,
  interrupted-save recovery (fault-injected), autosave and crash recovery; parsers fuzzed.
* **Geometry kernel:** integer nanometres, exact predicates, property-tested.
* **Command Engine:** undo/redo, transactions, lossless history (property-tested).
* **Headless pipeline and CLI:** script → schematic → PCB → ERC → DRC → Gerber/BOM with no GUI crate
  (tested), exit codes, JSON output.
* **Safety model of the Agent API:** nothing but the human's Apply changes a project.

"Production-ready" here means *reliable and tested for what it does*, not *complete*.

## 2. Works but experimental

Interactive routing (walk-around, push & shove, pairs, tuning), copper-zone fill, DRC/ERC as a
sign-off (rules are sound but incomplete), ButureScript, Gerber/Excellon output (not CAM-verified),
Gerber viewer, simulation (and its ngspice backend), plugins and component providers, importers
(SPICE and KiCad netlists), the 3D viewer, Symbol/Footprint/Library editors, the AI Workspace, the
benchmark numbers (synthetic boards only).

## 3. Partially implemented

Hierarchical schematics (model + connectivity, no way to create sheets); library placement (symbol
only); via kinds (blind/buried in the model only); net classes and rules (per class only);
manufacturing outputs (no silk text, no BOM supplier data); 3D (boxes); mechanical exchange (neutral
model, no STEP); UX (several operations missing in some views); platform support (Linux verified;
Windows/macOS type-checked only); no-connects (editor yes, scripts no); schematic generation (no wires).

## 4. Missing

Standard component/footprint libraries and datasheet-verified land patterns; auto-annotation;
cross-probing and back-annotation; hierarchical sheet creation; autorouter and 45° clean-up; DRC for
keepouts, hole-to-hole, silkscreen, mask, custom rules, fab presets; silkscreen text and BOM supplier
data in outputs; ODB++/IPC-2581; STEP import/export and 3D models; PDF/SVG/print output; PCB
copy/paste, teardrops, arc tracks, panelisation, fiducials; impedance and SI tools; design variants;
import of third-party schematics/boards/libraries; installers, signing, auto-update; plugin SDK and
registry; documentation inside the app.

## 5. What would stop an engineer from using ButureCAD for a commercial PCB

1. **No trustworthy parts.** There is no verified library; every land pattern must be hand-checked
   against a datasheet, and the generators are explicitly nominal.
2. **Outputs a fab/assembly house expects are incomplete:** no reference designators on silkscreen,
   BOM without MPN/supplier, no rotation offsets for pick-and-place, no assembly drawings.
3. **The DRC cannot be trusted as the last gate:** missing hole-to-hole, mask, silkscreen, keepout and
   custom/fab rules mean a clean DRC can still be a board the fabricator rejects.
4. **Design iteration is unsafe:** no annotation, cross-probing or back-annotation; "Update PCB" is
   one-way; scripts regenerate into an empty project and cannot merge hand edits. A real design
   changes for weeks.
5. **Routing is not yet productive:** no autorouter, diagonal geometry, little constraint support
   for length/impedance/fan-out — acceptable for a dozen nets, painful for hundreds.
6. **Nothing has met reality:** no fabrication run, no CAM cross-check of the Gerbers, no second
   EDA tool opening the outputs, no users, no field time. Data-loss and correctness risks at scale
   are unknown.
7. **Platform and distribution:** Windows/macOS unverified, no installer or signing, no crash
   reporting that respects the local-first rule, no migration story across alpha releases yet in practice.
8. **Single-author bus factor:** all knowledge sits in one development process; review by other
   engineers has not happened.

## Prioritised roadmap

Priorities reflect "what blocks a real board first". Nothing here is started.

### P0 — before any real board
1. Curated libraries with **datasheet-verified footprints and symbols** (passives, common ICs,
   connectors, crystals, regulators); component placement that brings symbol **and** footprint,
   value and MPN.
2. Manufacturing completeness: **silkscreen text** (references/values), **BOM MPN/supplier columns**,
   pick-and-place rotation offsets, assembly info.
3. DRC rules: **hole-to-hole, drill-to-copper, silkscreen-over-pad, mask sliver, keepout**, fabricator
   rule presets and custom rules; DRC fixtures built from published fab capabilities.
4. **Independent output verification:** compare Gerbers/drill in at least two third-party viewers and
   one fabricator's online CAM; make one physical test board.
5. Design iteration: **auto-annotation, cross-probing, back-annotation**, no-connect in scripts, and
   a safe way to re-apply a script to an existing project.
6. Verify (build + test + run) on **Windows and macOS**, with an installer and signed builds.

### P1 — productivity
7. Router: 45° clean-up, speed on failure, fan-out, multi-segment tuning, length groups; optional
   autorouter. Headless `route` command.
8. Hierarchical sheet creation (UI and ButureScript); schematic print/PDF/SVG and fabrication drawings.
9. STEP import/export and 3D component models; 3D with inner layers.
10. Missing editing operations: PCB copy/paste, schematic align/distribute, nudge/pan with keys,
    arc tracks, keepout drawing tool.
11. In-app help and onboarding; user-visible error log export.

### P2 — breadth
12. Plugin SDK (templates, signing, registry) and wiring of Importer/Exporter/Rule/Tool plugins.
13. Importers for third-party schematics/boards/libraries.
14. Simulation UX: measurements, cursors, export, parameter sweeps; model-library management.
15. Impedance calculator and stack-up driven rules; keymap editor; design variants.

### P3 — long term
16. ODB++ / IPC-2581; panelisation, fiducials, teardrops; HDI/blind/buried vias; rigid-flex; odd layer counts.
17. SI/PI analysis; collaborative workflows that stay local-first.
18. An external security and robustness review of the plugin sandbox and file parsers.

## How to use this document

Re-run the matrix of `docs/FEATURE_AUDIT.md` and the reference board test
(`apps/cli/tests/reference_board.rs`) after each P0 item; update the scores only with new evidence.
