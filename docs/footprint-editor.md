# Footprint Editor and footprint libraries (roadmap stage 12)

A real editor for footprints in the **Footprint Editor** tab. Like the Symbol Editor, the
footprint being edited lives in the project (`pcb/main.bpcb.json`, `footprints` table) and every
edit is one `SetFootprintCommand` through the Command Engine: undo/redo, dirty state, autosave and
crash recovery work as for any other edit. The model, editing operations and generators are in
`crates/pcb` (`footprint`, `footprint_edit`, `generators`); the interaction layer is
`apps/desktop/src-tauri/src/workspace/footprint_editor.rs`.

## Model (`buturecad-pcb`)

Footprints are defined for the **top** side, in integer nanometres (+Y up); flipping happens when
a footprint is placed on the board (roadmap 14–15).

| Concept | Representation |
|---|---|
| Pad kinds | **SMD** (copper + mask + paste, no hole), **through-hole** (plated, drilled), **NPTH** (non-plated hole, no copper) |
| Pad shapes | circle, rectangle, oval (stadium), rounded rectangle (corner ratio ‰ of the short side), **custom polygon** (pad-local outline) |
| Pad fields | number, position, size, rotation (quarter turns), drill (round or slot), mask margin, paste margin, paste on/off |
| Graphics | line, rectangle, circle, arc, polygon on the layers **silkscreen, fabrication, courtyard, mask, paste** |
| Texts | **reference** (`REF**`), **value**, or fixed user text, on silkscreen or fabrication |
| Attributes | mounting (SMD / through-hole), exclude from BOM / position files, description, tags |
| 3D models | list of references `{path, offset nm, rotation m°, scale ppm}` — **stored only; nothing renders them yet** (roadmap 27–28) |

`footprint::validate` is enforced on every edit, on library load/save and on project load:
positive sizes, circular pads square, drill leaves a copper ring (through-hole), slots at least as
long as wide, custom outlines of ≥ 3 points, non-plated holes have no number, at most one
reference and one value text, finite positive 3D scales, size/count limits. `footprint::warnings`
reports problems that do not block saving: no pads, no courtyard, courtyard not enclosing the
pads, missing reference/value text, overlapping copper of different pin numbers.

## Editing

Tools: Select `V`, SMD pad `S`, through-hole pad `H`, non-plated hole `N`, Line `L`, Rectangle `B`,
Circle `C`, Arc `A`, Text `T`. `R` rotates / `X`,`Y` mirror the selection about its grid-snapped
centre (custom pad outlines are mirrored exactly), `Del` deletes, dragging moves. New graphics go
on the layer chosen in the bar (the Inspector's layer list also toggles visibility). The Inspector
edits every pad/graphic/text field (kind and shape changes keep the pad valid: switching to
through-hole adds a drill, to circle makes it square, to custom creates an outline, to NPTH clears
the number). **Courtyard from pads** draws a rectangle around all pads + 0.25 mm on a 0.05 mm
grid, replacing any existing courtyard.

## Generators (`pcb::generators`)

DIP, SOIC (narrow/wide), QFN (with optional exposed pad), BGA and pin headers (1–2 rows). Each is
a pure, integer-exact function returning a validated footprint with pads, silkscreen (with a
pin-1 marker), fabrication outline, courtyard, reference and value text and mounting attributes.
Pin 1 is top-left; dual-row and quad packages number counter-clockwise, headers row-major, BGAs
`A1…` with the JEDEC row letters (no I, O, Q, S, X, Z; `AA…` after `Y`). Tests check numbering,
exact pitch, symmetry, 4-fold QFN symmetry, courtyard enclosure and zero warnings for every
family, plus a catalogue test that the UI's parameter defaults equal the engine's.

The default dimensions are nominal package dimensions (JEDEC-style): **no vendor library data is
copied, and generated footprints must be checked against the part's datasheet.** Generated names
look like `Generated:SOIC-8_3.9mm_P1.27mm`; generating the same package twice reuses the first.

## Library files

`<Library>.bfplib.json` in the project's `libraries/` folder or the user folder, next to symbol
libraries:

```json
{ "format": "buturecad-footprint-library", "schema": 1, "name": "Package_SO",
  "footprints": [ { "name": "SOIC-8_3.9mm_P1.27mm", "pads": [...], "graphics": [...], "texts": [...], "models": [...] } ] }
```

Footprints are stored under their unqualified name (`Library:Name` inside a project); output is
deterministic and written atomically; files are untrusted input (size limit, format tag, schema,
every footprint validated). Opening a library footprint copies it into the project; **Save to
library** writes it back and renames the project copy to match the library.

## PCB document schema 2

`pcb/main.bpcb.json` gained a `footprints` map (schema 2; migration 1 → 2 adds an empty map).
The board itself (outline, placed footprints, nets, zones) arrives in roadmap 13–15.

## Known limits

- Pad rotation is in quarter turns (exact); arbitrary angles come with the PCB stage.
- Custom pads are edited as polygons through the Inspector (a rectangle outline is created
  first); there is no on-canvas vertex editor yet.
- Mask and paste are shown as outlines at each pad's own margin (the board default margin is a
  PCB-stage setting); mask/paste graphics are stored but have no effect until manufacturing.
- Pad numbers are drawn horizontally; there is no rotated text yet.
- No footprint clipboard, no array/duplicate tool, no pad renumbering tool.
- 3D model references are stored only.
