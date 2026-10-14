# STEP / MCAD exchange (roadmap stage 28)

**Status: architecture done; STEP geometry import/export NOT implemented.**

## Layers

```text
PcbDoc (core) ──bridge──▶ MechanicalModel ──McadBackend──▶ bytes (.bmech / .step …)
                         (crates/mcad, no dependency on the PCB core or on any STEP library)
```

* `crates/mcad` (`buturecad-mcad`): the neutral `MechanicalModel` (integer nm; board outline loops,
  mounting holes, components with prism or external-model bodies, constraints: max height, keepout,
  fixed component), `validate()`, the `McadBackend` trait, `BackendInfo`/`Support` (each backend
  declares honestly what it does) and a `Registry` that routes by file extension.
* `apps/desktop/.../workspace/mcad.rs`: the only bridge. `build_mechanical(PcbDoc)` and the Tauri
  command `mcad_export(path)`. Export direction only; importing mechanical data into the board must
  later go through the Command Engine (undo, agent review).

## Backends today

| Backend | Ext | Import | Export |
|---|---|---|---|
| `neutral` | `.bmech` | full (versioned JSON, validated) | full (deterministic) |
| `step` | `.step` `.stp` | **none** — parses ISO 10303-21 structure (`StepBackend::inspect`: protocol AP203/214/242, entity/solid/product counts) and returns `Unsupported` | **none** |

The Part 21 reader (`part21.rs`) is in-house syntax only (header, entity list, comments, quoted
strings); malformed input returns errors. It does not interpret B-rep or assemblies.

## What the bridge exports (and what it does not invent)

Outline loops, drilled non-plated pads as mounting holes, placed components (position, quarter-turn
rotation, side), `External` bodies from the 3D models the footprint references, locked components
as `FixedComponent`. The board stores **no body heights**, so no `Prism` bodies are emitted.
A board without a closed outline is refused (`validate`). No UI button yet.

## Licence analysis (why no kernel is linked)

| Candidate | Licence | Verdict |
|---|---|---|
| OpenCascade (OCCT) via `opencascade-rs` | LGPL-2.1 + OCCT exception; C++ build | Linking in a proprietary product is possible but needs legal review, dynamic linking and a large C++ toolchain; deferred. |
| `truck` (Rust) | Apache-2.0 | Permissive, pure Rust; STEP support is limited — evaluate when real import is scheduled. |
| `ruststep` | Apache-2.0 | Parses EXPRESS/Part 21 generically; maintenance is slow — evaluate. |
| Writing AP214 in-house | n/a | Possible for prismatic solids; not started. |

Decision: none selected. A future backend implements `McadBackend` and is registered; the core and
the neutral model do not change. Record the choice in `docs/DEPENDENCIES.md` when made.

## Limits / next steps

STEP geometry import/export, STEP assemblies, mechanical constraint checking (nothing evaluates
`MaxHeight`/`Keepout` yet), importing a `.bmech` back into the board, a UI entry point.
