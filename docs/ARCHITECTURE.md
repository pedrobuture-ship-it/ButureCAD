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
| `eda-core` | Core: project model, Command Engine | foundation |
| `geometry` | Integer-coordinate geometry | foundation |
| `io` | Project format, atomic save | foundation |
| `schematic`, `pcb`, `router`, `rules`, `libraries`, `manufacturing`, `simulation`, `viewer3d`, `renderer`, `scripting`, `plugin-runtime`, `agent-api` | reserved slots | not implemented |
| `apps/desktop` | Tauri 2 shell + React UI | shell only |

## Rules

- React owns UI only; geometry, connectivity, DRC/ERC, routing, serialization stay in Rust.
- Dependencies are directional: `geometry` ← `pcb`/`schematic` ← `rules`/`router` ← `eda-core`; no cycles.
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
