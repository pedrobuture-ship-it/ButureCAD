# Symbol Editor and symbol libraries (roadmap stage 09)

## What it is

A real editor for library symbols, hosted in the **Symbol Editor** tab. Every edit is a
`SetSymbolCommand` through the Command Engine, so undo/redo, dirty state, autosave and crash
recovery behave exactly like schematic edits. Pure editing operations live in
`crates/schematic/src/symbol_edit.rs`; the interaction layer is
`apps/desktop/src-tauri/src/workspace/symbol_editor.rs`; the canvas is the same wgpu renderer.

## Model (`buturecad-schematic`)

| Concept | Representation |
|---|---|
| Pin | number, name, electrical type, connection point, orientation, length, `unit`, `representation` |
| Pin types | input, output, bidirectional, tri-state, passive, power input, power output, open collector, open emitter, no connect (plus *unspecified*, kept for imported/legacy data) |
| Graphics | line, rectangle, circle, arc, text; each with `unit`, `representation`, line width, fill |
| Units | `unit_count` 1–26. Item `unit` `0` = shared by every unit (typical for power pins), `n` = unit n |
| Alternate representation | item `representation` `0` = both, `1` = normal, `2` = alternate (De Morgan style) |
| Metadata | description, keywords, datasheet, manufacturer, MPN, default footprint, footprint filters, free key/value properties |

A placed `Component` records its own `unit` and `representation`. `Schematic::placed_pins` returns
only the pins of that unit/representation (plus shared pins), so connectivity sees exactly the
pins that are drawn. Shared pins on several placed units of one reference are the same physical
pin (they get connected by the user's wiring like any other pin; ERC treats them as one pin).

Rules enforced by `symbol_edit::validate` (on every edit, on library load and save):

- name is non-empty and file-name safe; unit count 1–26; unit/representation in range;
- pin numbers are non-empty and unique among pins whose unit and representation overlap;
- pin length is not negative.

Placing unit *n > 1* of a multi-unit symbol joins the most recently placed part of the same
symbol that does not have that unit yet (`U1A`, `U1B` share `U1`); unit 1 always starts a new
reference. Changing a symbol so that a placed part would reference a missing unit or alternate
representation is refused (`SetSymbolCommand` checks every use).

## Workflow

1. **Open** a symbol (from the project, a library folder or the built-in set) or **New**
   (`Library:Name`). Opening a library symbol *copies* it into the project's embedded symbol table;
   the project copy is what placed parts use.
2. Draw with the tools: Select `V`, Pin `P`, Line `L` (chains, Esc ends), Rectangle `B`, Circle `C`,
   Arc `A` (center, start, end direction), Text `T`. `R` rotates / `X`,`Y` mirror the selection
   (or the pin about to be placed), `Del` deletes, drag moves. Everything snaps to the grid;
   pin connection points should stay on the 1.27 mm grid (the editor warns otherwise).
3. Edit properties in the Inspector (pin type/number/name/length/orientation/unit/representation;
   symbol name, prefix, default value, units, metadata, custom properties).
4. **Save to library** writes the symbol to a library folder. If the library name differs from the
   symbol's, the project copy is renamed (placed parts follow, one undo step).

Renaming a symbol retargets all instances in one undo step. Deleting a symbol that placed parts
use is refused.

## Library files (`buturecad-libraries`)

Folders:

- **project**: `<project>/libraries/` (travels with the project, Git-friendly);
- **user**: `<app data dir>/libraries/` (shared between projects).

File: `<Library>.bsymlib.json`

```json
{
  "format": "buturecad-symbol-library",
  "schema": 1,
  "name": "Analog",
  "symbols": [ { "name": "LM358", "reference_prefix": "U", "unit_count": 3, "pins": [...], "graphics": [...], "meta": {...} } ]
}
```

- Symbols are stored under their **unqualified** name; in a project they are `Library:Name`.
- Deterministic output (symbols sorted by name), atomic writes (`buturecad-io`).
- Files are untrusted input: size limit (64 MiB), format tag, newer-schema refusal, every symbol
  validated. A damaged file is listed with its error instead of being hidden, and never crashes
  the app.
- `Buture` is the built-in bootstrap library (read-only, embedded in code); it is not a file.

## Compatibility

Files written before units existed load unchanged (`unit`/`representation` default to `0`,
`unit_count` to `1`, `meta` empty; a component's `unit`/`representation` default to `1`).
Covered by `crates/schematic/tests/symbols.rs`.

## Known limits

- One library per file; no component records (symbol + footprint + 3D + manufacturer data), no
  library search index, no plugin providers: roadmap stages 31–33.
- Pin names and numbers are drawn horizontally (no rotated text in the renderer yet).
- Reload-from-library ("update the project copy") is not implemented: the project copy is
  authoritative once opened.
- The text tool places fixed 1.27 mm text; text height is editable in the Inspector.
- No symbol clipboard (copy/paste) inside the editor yet.
