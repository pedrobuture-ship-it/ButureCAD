# Architecture

ButureCAD is a local-first, offline-first EDA. The Rust core is the source of truth.

```text
Desktop GUI (Tauri + React)   buturecad-cli (planned)
          └──────────┬──────────┘
              ButureCAD Core (Rust)
```

## Workspace

| Crate | Role | State |
|---|---|---|
| `geometry` | Integer-nanometre geometry: exact predicates, polygons, offsets, convex hull, R-tree | done (stage 03) |
| `schematic` | Schematic model, symbols, logical connectivity, ERC data, change sets | done (01–11) |
| `pcb` | Footprints, board model (layers, stackup, **physical layer stack**, zones, net classes, differential pairs, length targets), generators | done (12–26) |
| `eda-core` | Core: project, Command Engine, board commands, **copper shapes**, ratsnest, routing geometry, schematic→PCB sync, net length, **zone fill** (`i_overlay`) | done |
| `io` | Project format (schemas, migrations), atomic save, autosave/recovery | done |
| `libraries` | Symbol, footprint and layer-stack-preset files (local folders) | symbols, footprints, stackup presets |
| `rules` | ERC and **DRC** (incremental engine) | done (11, 19) |
| `router` | Walk around, push & shove, differential pair geometry, length tuning, DRC validation of what it produces | done (21–24) |
| `renderer` | wgpu renderer, picking, snapping | done (04) |
| `manufacturing`, `simulation`, `viewer3d`, `scripting`, `plugin-runtime`, `agent-api` | reserved slots | not implemented |
| `apps/desktop` | Tauri 2 shell + React UI; the Rust `Workspace` owns all state and tools | done through stage 26 |

## Rules

- React owns UI only; geometry, connectivity, DRC/ERC, routing, serialization stay in Rust.
- Dependencies are directional: `geometry` ← `pcb`/`schematic` ← `eda-core` ← `rules` ← `router`; `io` and `libraries` sit beside them; the desktop app depends on all and nothing depends on it; no cycles.
- No cloud, login, telemetry or mandatory network. AI is an optional client of the core.
- Tauri commands only delegate to `eda-core`; typed contracts live in `apps/desktop/src/api.ts`.

## Licensing

Proprietary, all rights reserved (see `LICENSE`). Crates use `LicenseRef-Proprietary` and `publish = false`.

## Geometry (`buturecad-geometry`)

- Internal unit: 1 nm, `i64`; valid range ±2^30 nm (±1.07 m) so all predicates run in exact `i128`.
- Axes: +X right, +Y up, angles counter-clockwise. mm/mil/inch only at the boundary.
- Yes/no predicates (intersects, contains, collides, within-distance) are exact. Distances and
  constructions (intersection points, rotation, offsets) are rounded to the nearest nm and are
  documented per function; arbitrary-angle rotation must always be applied to the original geometry.
- Spatial index: R-tree (STR bulk load + dynamic insert/remove) via `rstar`, chosen over a grid
  (bad on mixed object sizes) and a BVH (rebuild cost under edits). Bounding-box queries only.
- Known limits: polygons have no holes; polygon offset is mitered and rejects topology changes
  instead of resolving them; arc/circle intersection points are numerical (f64).

## Command Engine (`buturecad-eda-core`)

- `Project` exposes read-only accessors; mutation is `pub(crate)` and reachable only through
  `Command`s executed by `CommandEngine` (place/move/delete component, create/delete wire, track,
  via, rename project). Generic `CreateCommand<E>` / `DeleteCommand<E>` serve every `Entity`.
- Contract: `apply` is atomic; `revert` runs LIFO and restores exact state including ids (ids are
  allocated on first apply, reused on redo, never rewound).
- Transactions group commands into one undo step, nest, and roll back atomically; the closure form
  `transaction(label, |e| ...)` commits on `Ok` and rolls back on `Err`.
- Dirty state is exact w.r.t. the last save point (undoing back to it is clean; a save point lost
  to a discarded redo branch or the history limit is never clean again).
- Autosave: the engine exposes `revision()` / `autosave_pending()`; `buturecad-io::engine` performs
  the writes, so the core stays I/O free. A recovered autosave is wrapped with
  `CommandEngine::recovered` (starts dirty). A failing `revert` clears history rather than continue.
- Schematic edits are `SchematicEditCommand`s wrapping `Change` sets (full before/after object
  states) from `buturecad-schematic`; `AddSymbolsCommand` embeds library symbols. Ids are reserved
  with `CommandEngine::new_id()`. The PCB entities in `eda-core::model` remain a minimal skeleton.

## Renderer (`buturecad-renderer`)

- **CPU side (no GPU needed, fully unit-tested):** `View` (pan/zoom, world↔screen, +Y up),
  adaptive grid (1-2-5 multiples of the user grid, so drawn lines coincide with snap nodes), grid
  and object snapping, `Scene` (layers + shapes + R-tree), picking (exact distance tests, smaller
  objects first), hover, selection, window/crossing box selection, tessellation (curves flattened
  to 0.25 px, ear-clipped polygons, built-in 5×7 bitmap font).
- **GPU side:** `GpuRenderer` (wgpu, 4× MSAA) renders offscreen and reads pixels back. The webview
  displays the RGBA frame in a `<canvas>`: the DOM never holds per-object elements. wgpu cannot
  present directly into a webview on all platforms, so readback is the portable transport; the cost
  is one memcpy per frame (≈1.5 ms at 1400×900 here). Frames are produced on demand.
- **Caching:** the scene mesh is rebuilt only when the scene revision or the zoom bucket (≈19 %
  steps) changes; panning re-uses it. Selection/hover/previews and the pixel-space overlay (grid,
  rubber band, debug text) are small per-frame meshes. Meshes store `f32` offsets from a per-mesh
  origin kept in `f64`.
- **Measured (RTX 2060, release):** 200 000 tracks: pick 18 µs, tessellation 35 ms (only on
  rebuild), pan frame ≈1.7 ms. See `examples/stress.rs`.
- **Limits:** text is a basic ASCII bitmap font; strokes overlap with simple joins (opaque colours
  only, translucent strokes would double-blend); arcs/text use bounding boxes for crossing
  selection; no GPU instancing yet; no direct swapchain presentation.

## Desktop shell (`apps/desktop`)

- **Rust owns state.** `Workspace` (open project + `CommandEngine`, views, selection, scenes, GPU
  renderer) is Tauri-independent and tested headlessly. `commands.rs` is a thin IPC layer; every
  state-changing command returns a `Snapshot` the UI renders. Types are exported to TypeScript by
  `ts-rs` (`src/bindings/`, checked in CI), so the contract cannot drift.
- **Canvas.** React only hosts a `<canvas>`. Pointer input is forwarded to Rust (`pointer`), which
  handles pan/zoom/hover/selection/box selection and picking; `render_frame` returns RGBA bytes
  that React paints with `putImageData`. No per-object DOM.
- **Scene building** (`scene_builder.rs`) turns the schematic/PCB model into renderer shapes
  (shape id = owner id << 12 | n), keeping model crates free of rendering concerns.
- **Single command registry** (`commands.ts`): menus, toolbar, command palette and shortcuts are
  generated from it. Unavailable commands stay visible, disabled with the reason. Tools and modes
  that are not implemented are shown disabled/labelled with the roadmap stage; ERC/DRC panels say
  "not implemented" instead of showing fake results.
- **Verification.** `tests/ipc.rs` drives the real commands through Tauri's mock runtime;
  `apps/desktop/e2e` drives the built frontend in headless Chromium against the real Rust commands
  and real GPU renderer (dev tool, not run in CI).

## Schematic editor

See `docs/schematic-editor.md`. Pure operations (`buturecad_schematic::edit`) → `Change` sets →
`SchematicEditCommand` in the Command Engine. The desktop `editor.rs` owns the tool state machine
(placement ghosts, wire drawing, live move preview that mutates only the scene, never the model).
