# Document files (schema 1)

Each document is deterministic pretty-printed JSON with a `format` tag and `schema_version`.
Objects are stored in maps keyed by their stable numeric id (ascending); unknown fields are
rejected. Coordinates are integer **nanometres** (see `crates/geometry`).

## `schematic/main.bsch.json` (`buturecad.schematic`)

```json
{
  "format": "buturecad.schematic",
  "schema_version": 1,
  "components": { "1": { "reference": "C1", "value": "100n", "position": {"x": 1270000, "y": 2540000}, "rotation": 1 } },
  "wires": { "3": { "segment": { "a": {"x": 0, "y": 0}, "b": {"x": 5, "y": 0} } } }
}
```

`rotation` is counter-clockwise quarter turns (0–3).

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
- The model is a minimal skeleton (no pins, nets or footprints yet). Those arrive with later
  stages and will bump the document schema versions with migrations.
