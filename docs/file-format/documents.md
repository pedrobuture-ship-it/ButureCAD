# Document files

Each document is deterministic pretty-printed JSON with a `format` tag and `schema_version`.
Objects are stored in maps keyed by their stable numeric id (ascending); unknown fields are
rejected. Coordinates are integer **nanometres** (see `crates/geometry`).

## `schematic/main.bsch.json` (`buturecad.schematic`, schema 3)

```json
{
  "format": "buturecad.schematic",
  "schema_version": 3,
  "root": 0,
  "erc_waivers": [ { "fingerprint": "unconnected_pin|J1.2", "rule": "unconnected_pin", "subject": "...", "reason": "..." } ],
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

Symbols carry `unit_count`, optional `meta`, and every pin and graphic has `unit` and
`representation` (see `docs/symbol-editor.md`); components have `unit` and `representation`.
All of these are optional on read (defaults keep older files valid).

Object types: `component`, `wire`, `bus`, `junction`, `net_label`, `global_label`, `hier_label`,
`power_symbol`, `no_connect`, `sheet`. The model is validated on load (unknown symbols, duplicate
ids, hierarchy cycles, bad rotations, … are errors). See `docs/schematic-connectivity.md`.

**Migration 2 → 3** (implemented): adds the empty `erc_waivers` list. Symbol units, alternate
representations, metadata and component units default when absent, so nothing else changes; the
bump makes older builds refuse the file instead of silently dropping data.

**Migration 1 → 2** (implemented, tested with a verbatim v1 file): the pre-release skeleton stored
`components` and `wires` directly. They become objects of a single root sheet (id `0`);
components get `lib_id: ""` (no symbol), `placement` from `position`/`rotation`, empty
properties. Files are rewritten as v2 on the next save.

## `pcb/main.bpcb.json` (`buturecad.pcb`)

Schema 2: `tracks` (`layer`, `segment`, `width`), `vias` (`position`, `diameter`, `drill`) and
`footprints` (`Library:Name` → footprint, see `docs/footprint-editor.md`). Migration 1 → 2 adds an
empty `footprints` map. Schema 3 adds the stackup, placed footprints, nets, zones, keepouts, texts
and graphics (`docs/pcb-core.md`). Schema 4 adds `rules`: board limits, net `classes` and
net → class `assignments` (`docs/drc.md`); older files load with the default rules (migration 3 → 4
rewrites nothing). Schema 5 adds `rules.diff_pairs` (name, positive/negative net, optional
width/gap, max skew; `docs/diff-pairs.md`); migration 4 → 5 rewrites nothing. Schema 6 adds `rules.length_targets` (net → target, tolerance;
`docs/length-tuning.md`); migration 5 → 6 rewrites nothing. Schema 7 adds zone fill parameters (`clearance`, `min_width`,
`connection`, `thermal_gap`, `thermal_spoke`, `remove_islands`) and the computed `fill` (regions with
holes + signature; `docs/zones.md`); migration 6 → 7 rewrites nothing. Schema 8 adds the physical layer stack (`physical`: per-layer
copper thickness/role, dielectric gaps, masks, finish; `docs/stackup.md`); files without it get the
standard stack for their layer count, migration 7 → 8 rewrites nothing.

## Rules

- A missing document file means an empty document (projects created before documents existed).
- A present but malformed, foreign or newer-schema file is an error; the project is not opened.
- The id counter is not stored: it resumes after the highest id on load.
- Saves write documents first and the manifest last, each file atomically. The three files are
  **not** one atomic unit; a rotating backup snapshot (`.buture/backups/NNNNNN/`, last 5, taken
  only when the current files all parse) is the recovery path. Autosave mirrors the layout in
  `.buture/autosave/`.
- Any change to a document's shape bumps its schema version and adds a migration step.
