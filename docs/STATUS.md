# Working
- Cargo workspace builds; fmt/clippy/tests pass.
- Tauri shell starts and queries core version through a typed command.

- Project format (schema 1): create/open/save/save-as, atomic writes, rotating backups, autosave, crash recovery, migration framework (`buturecad-io`).
- Geometry engine (`buturecad-geometry`): Point/Vector/Line/Segment/Arc/Circle/Rectangle/Polygon/BoundingBox, exact predicates, nearest point, offset, rotation, R-tree index; unit + property-based tests.

# Partial
- Project only holds a name; schematic/PCB documents are not persisted yet.
- Not exposed in the desktop UI yet.
- Desktop UI is a static shell; navigation items are disabled.

# Broken

# Planned
- Everything in `docs/ROADMAP.md` beyond stage 04.

- Polygon offset: mitered only, no topology resolution; no polygon holes/boolean operations.

# Technical Debt
- Reserved crates are empty on purpose.

# Known Bugs
