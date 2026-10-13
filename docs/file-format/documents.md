# Document files

Each document is deterministic pretty-printed JSON with a `format` tag and `schema_version`.
Objects are stored in maps keyed by their stable numeric id (ascending); unknown fields are
rejected. Coordinates are integer **nanometres** (see `crates/geometry`).

## `schematic/main.bsch.json` (`buturecad.schematic`, schema 2)

```json
{
  "format": "buturecad.schematic",
  "schema_version": 2,
  "root": 0,
  "symbols": { "Buture:R": { "name": "Buture:R", "pins": [ ... ], "graphics": [ ... ] } },
  "sheets": {
    "0": { "name": "Root", "objects": {
      "1": { "type": "component", "lib_id": "Buture:R", "reference": "R1", "value": "10k",
             "footprint": "", "properties": {},
             "placement": { "position": {"x": 1270000, "y": 2540000}, "rotation": 1, "mirror": false } },
      "2": { "type": "wire", "segment": { "a": {"x": 0, "y": 0}, "b": {"x": 5, "y": 0} } }
    } }
  }
}
```

Object types: `component`, `wire`, `bus`, `junction`, `net_label`, `global_label`, `hier_label`,
`power_symbol`, `no_connect`, `sheet`. The model is validated on load (unknown symbols, duplicate
ids, hierarchy cycles, bad rotations, … are errors). See `docs/schematic-connectivity.md`.

**Migration 1 → 2** (implemented, tested with a verbatim v1 file): the pre-release skeleton stored
`components` and `wires` directly. They become objects of a single root sheet (id `0`);
components get `lib_id: ""` (no symbol), `placement` from `position`/`rotation`, empty
properties. Files are rewritten as v2 on the next save.

## `pcb/main.bpcb.json` (`buturecad.pcb`)

`tracks` (`layer`, `segment`, `width`) and `vias` (`position`, `diameter`, `drill`).

## Rules

- A missing document file means an empty document (projects created before documents existed).
- A present but malformed, foreign or newer-schema file is an error; the project is not opened.
- The id counter is not stored: it resumes after the highest id on load.
- Saves write documents first and the manifest last, each file atomically. The three files are
  **not** one atomic unit; a rotating backup snapshot (`.buture/backups/NNNNNN/`, last 5, taken
  only when the current files all parse) is the recovery path. Autosave mirrors the layout in
  `.buture/autosave/`.
- The PCB document is still a minimal skeleton (tracks and vias); footprints, pads and nets will
  bump its schema version with a migration.
