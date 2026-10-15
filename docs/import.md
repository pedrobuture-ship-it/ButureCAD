# Importers (roadmap stage 43)

```text
file ─▶ Importer (crates/io/src/import/<format>.rs) ─▶ { Design (EDA IR), generated symbols, ImportReport }
                                                      └▶ same `ir_apply` path as ButureScript → project
```

CLI: `buturecad import <file> --out <project-dir> [--format auto|spice|kicad-netlist] [--report r.json] [--json]`.
The project is created only if the report has no errors; the report is saved as
`<project>/import-report.json`. Exit codes: `0` created, `1` not created (errors in the file),
`2` usage/I-O.

## Import Report

Every import produces a report with **converted**, **ignored**, **unsupported**, **warnings** and
**errors**. Each item has a kind, a name and the reason (`ignored`: why it carries no design
data; `unsupported`: information that was **not** imported).

**Nothing is silently dropped, and the code enforces it:** an importer calls `report.seen(n)` for
each record it reads and must file the record as converted, ignored or unsupported;
`ImportReport::finish()` compares the two counts and turns a mismatch into an error (tested with a
deliberately forgetful importer). Data that has no home in the model but must not be lost is kept
as component properties (e.g. SPICE `spice_extra`).

## Formats

| Id | File | What is imported | Basis |
|---|---|---|---|
| `spice` | `.cir .sp .spice .net` | R, C, L, V, I, D with their nodes (node `0`/`gnd` → `GND`), `.model` text attached to diodes (`Spice_Model`), continuation lines, title as project name | public SPICE3/ngspice syntax documentation |
| `kicad-netlist` | `.net` (S-expression `export`) | components (reference, value, properties), footprint names mapped to generators when recognisable (`R_0603_…` → `0603`, `SOIC-8…` → `soic8`, `PinHeader_1xN`), nets and nodes; symbols **generated** from the pins used in nets (types from `pintype`) when no built-in symbol fits | KiCad's public netlist file-format description |

Unsupported (listed in the report): SPICE Q/M/X/E/F/G/H/B…, `.subckt` bodies, `.param`, `.include`,
analysis commands, anything after `.end`; KiCad library tables are ignored (symbols are
regenerated), unknown sections are unsupported. Netlists carry **no placement, board or
footprints data**, so none is imported (the report says so).

## Licensing

Importers are written from published format descriptions only; no parser or source code of other
tools is copied (CLAUDE.md §31). Formats whose documentation or licence does not allow an
independent implementation are not implemented — in particular there is **no** importer for
proprietary binary/closed formats or for full KiCad project/board/schematic files in this stage
(the board and schematic file formats are larger, versioned and need their own review).

## Limits

* The KiCad importer was tested with fixtures written from the documented structure, **not** with
  files exported by a real KiCad (none was available).
* Generated symbols are simple rectangles with only the pins that appear in nets.
* No GUI entry point yet; use the CLI.
* Adding a format: implement `Importer` in a new file, register it in `Registry::builtin`, add
  fixtures that check the report's accounting.
