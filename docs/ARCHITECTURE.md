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
