# Working
- Cargo workspace builds; fmt/clippy/tests pass.
- Tauri shell starts and queries core version through a typed command.

- Project format (schema 1): create/open/save/save-as, atomic writes, rotating backups, autosave, crash recovery, migration framework (`buturecad-io`).
- Geometry engine (`buturecad-geometry`): Point/Vector/Line/Segment/Arc/Circle/Rectangle/Polygon/BoundingBox, exact predicates, nearest point, offset, rotation, R-tree index; unit + property-based tests.
- Command Engine (`buturecad-eda-core`): undo/redo, nested transactions, dirty state, history limit, autosave/recovery hooks; commands for components, wires, tracks, vias, rename; unit + property tests (undo-all/redo-all is lossless).
- Schematic/PCB document persistence (`schematic/main.bsch.json`, `pcb/main.bpcb.json`) through the engine, with backups/autosave/recovery covering documents.
- 2D renderer (`buturecad-renderer`): pan/zoom, adaptive grid, snap, layers, lines/arcs/circles/rects/polygons/text, picking, hover, selection, box selection, optional FPS overlay; wgpu backend verified on a real GPU (tests skip without an adapter).

# Partial
- Design model is a minimal skeleton (no pins, nets, footprints, connectivity); entities exist only to host commands.
- Multi-file save is not one atomic unit (backup snapshot is the recovery path).
- Project and command engine are not exposed in the desktop UI yet.
- Desktop UI is a static shell; navigation items are disabled.

# Broken

# Planned
- Everything in `docs/ROADMAP.md` beyond stage 04.

- Polygon offset: mitered only, no topology resolution; no polygon holes/boolean operations.

- Renderer is not wired into the desktop UI yet (Prompt 06).

# Technical Debt
- Reserved crates are empty on purpose.

# Known Bugs
