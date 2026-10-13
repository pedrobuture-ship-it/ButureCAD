# Working
- Cargo workspace builds; fmt/clippy/tests pass.
- Tauri shell starts and queries core version through a typed command.

- Project format (schema 1): create/open/save/save-as, atomic writes, rotating backups, autosave, crash recovery, migration framework (`buturecad-io`).
- Geometry engine (`buturecad-geometry`): Point/Vector/Line/Segment/Arc/Circle/Rectangle/Polygon/BoundingBox, exact predicates, nearest point, offset, rotation, R-tree index; unit + property-based tests.
- Command Engine (`buturecad-eda-core`): undo/redo, nested transactions, dirty state, history limit, autosave/recovery hooks; commands for components, wires, tracks, vias, rename; unit + property tests (undo-all/redo-all is lossless).
- Schematic/PCB document persistence (`schematic/main.bsch.json`, `pcb/main.bpcb.json`) through the engine, with backups/autosave/recovery covering documents.
- 2D renderer (`buturecad-renderer`): pan/zoom, adaptive grid, snap, layers, lines/arcs/circles/rects/polygons/text, picking, hover, selection, box selection, optional FPS overlay; wgpu backend verified on a real GPU (tests skip without an adapter).
- Schematic core (`buturecad-schematic`): symbols/pins, components, wires, buses, junctions, net/global/hierarchical labels, power symbols, sheets, no-connects; validation; logical connectivity (junction rules, labels, hierarchy, buses) with auto-naming and structural issues; schema v2 file format with a real v1→v2 migration.
- Desktop shell: menus/toolbar/tool palette/document tabs/inspector/bottom panel/status bar, dark and light themes, command palette, shortcuts, context menu, project New/Open/Save/Save As/Close, dirty-guard, autosave + crash-recovery prompt, recent projects, GPU canvas with pan/zoom/hover/selection/box selection, real connectivity in the inspector, PCB layer visibility. Verified end to end (e2e harness) with screenshots.

# Partial
- PCB model is a minimal skeleton (tracks, vias) hosting the commands.
- Built-in symbol library is a small bootstrap set (R, C, L, D, LED, connectors, GND/+3V3/+5V/VCC).
- Multi-file save is not one atomic unit (backup snapshot is the recovery path).
- No editing tools yet: the canvas supports navigation and selection only (editor stage).
- Symbol Editor, Footprint Editor and 3D Viewer tabs are placeholders labelled as not implemented; ERC/DRC panels likewise.
- Inspector is read-only for now.
- Desktop UI is a static shell; navigation items are disabled.

# Broken

# Planned
- Everything in `docs/ROADMAP.md` beyond stage 04.

- Polygon offset: mitered only, no topology resolution; no polygon holes/boolean operations.


# Technical Debt
- Reserved crates are empty on purpose.

# Known Bugs
