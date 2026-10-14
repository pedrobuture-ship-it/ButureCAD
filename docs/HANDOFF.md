# Handoff — where the project stands and how to continue

*Written at the end of the session that implemented roadmap stages 19–34. Read this, then
`CLAUDE.md`, `docs/STATUS.md` and `docs/ROADMAP.md`, then ask for / read the next roadmap prompt
in `promptsbuturecad.md` (prompt 35, ButureScript, is next).*

## State

* Roadmap stages **01–34 are done**, tested and pushed to `https://github.com/pedrobuture-ship-it/ButureCAD`
  (branch `main`). Last checkpoint (full review, no new features) was after stage 26; stages 27–29 were
  added after it. Reasonable next step: implement 28–30, then do the next checkpoint.
* Verification commands (all green at the end of this session):
  ```bash
  eval "$(mise env)"                    # Rust toolchain via mise
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace                # ~530 tests
  cd apps/desktop && npm run check      # typecheck, eslint, prettier, vitest (21)
  npm run build && cargo build -p buturecad-desktop --examples
  DEMO_PROJECT=/tmp/fresh-dir node e2e/<scenario>.mjs   # route, pcb, zone, stackup, viewer3d, …
  ```
  (`apps/desktop/e2e/README.md` explains the headless-Chromium harness; use a **fresh**
  `DEMO_PROJECT` folder each time. Run the e2e after `npm run build` and building the examples.)
* Running the real app for manual tests: `npx tauri build --debug --no-bundle` in `apps/desktop`,
  then `WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/debug/buturecad-desktop <project folder>`
  (needed on this Wayland machine). `cargo run -p buturecad-desktop --example demo_project -- <dir>`
  generates a demo project.

## Rules of the engagement (from the user)

* Commits as `pedrobuture <pedrobuture@gmail.com>`, tags `[feat]` / `[fix]`, **no Claude or
  Co-Authored-By lines**, proprietary "all rights reserved" licence (not MIT). Push to `origin main`.
  (Pass identity with `git -c user.name=pedrobuture -c user.email=pedrobuture@gmail.com commit`.)
* Answer the user in Portuguese; keep replies short and honest about limits.
* CLAUDE.md applies: Rust core is the truth, every mutation goes through the Command Engine, never
  fake a feature, docs describe reality, update `docs/STATUS.md`, checkpoint every 3–5 prompts
  (no new features during a checkpoint).

## What exists (crates) — see `docs/ARCHITECTURE.md`

`geometry` · `schematic` · `pcb` (board model, rules, net classes, pairs, length targets, zones,
physical stack) · `eda-core` (commands, copper shapes, ratsnest, zone fill, net length, sync) ·
`io` (formats, schema **8**) · `libraries` · `rules` (ERC, incremental DRC) · `router` (walk around,
push & shove, pair geometry, tuning) · `renderer` (wgpu 2D) · `viewer3d` (software 3D) · desktop app
(`Workspace` owns state/tools; React is UI only; ts-rs generates `src/bindings`).

Stage docs: `docs/{symbol-editor,erc,footprint-editor,pcb-core,pcb-editor,schematic-to-pcb,ratsnest,
routing,drc,diff-pairs,length-tuning,zones,stackup,viewer3d}.md`, file formats in
`docs/file-format/documents.md`, dependency decisions in `docs/DEPENDENCIES.md`
(`i_overlay`/`i_triangle` were added for zones).

## What was done in the last session (stages 19–34)

19 DRC engine · 20 net classes · 21 walk around · 22 push & shove · 23 differential pairs · 24 length
tuning · 25 copper zones · 26 layer stack manager · 27 3D viewer · 28 STEP/MCAD layer (partial) · 29 manufacturing export. Each has tests (unit, property,
workspace IPC) and a doc; stages 25–27 also have an e2e scenario with screenshots in
`apps/desktop/e2e/out/`.

## Known limits to keep in mind (also in STATUS.md)

Router detours at any angle; push & shove never moves vias/pads and not for pairs; meanders only on
one straight track; zone fills are stored in the project file and thermal spokes are axis-aligned;
zones are not router obstacles; only through vias exist (HDI/blind/buried are modelled, not
routable); the layer stack is not yet used by Gerber/impedance; 3D components are boxes with
heuristic heights (no STEP yet); no e2e for the DRC panel / Net Classes dialog; only the root sheet
of a hierarchy is editable.

## Next: roadmap prompts 28–50

28 STEP/MCAD (analyse **licences** before choosing a library; OpenCascade is LGPL — record the
decision in `docs/DEPENDENCIES.md`; architectural support first) · 29 import/export of board
outline & mounting holes · 30 … (read `promptsbuturecad.md` from "PROMPT 28" on) · manufacturing
(Gerber/Excellon/BOM/pick&place — never fake Gerber; validate with DRC first), Gerber viewer,
libraries/plugins, simulation, ButureScript, CLI/headless/Agent API, importers, performance,
hardening, release.

Process for each stage: read CLAUDE.md §41–§43 and the prompt; inspect existing modules; implement
in Rust; tests; docs (`docs/<stage>.md`, ROADMAP, STATUS); fmt/clippy/test/npm check/e2e; commit
and push; checkpoint after 3–5 stages.
