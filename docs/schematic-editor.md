# Schematic editor

The editor is an interaction layer over pure operations: `buturecad_schematic::edit` turns intent
into reversible `Change` sets; `apps/desktop/src-tauri/src/workspace/editor.rs` turns pointer
events and commands into those operations, previews them live, and executes the result through the
Command Engine. Every edit is therefore undoable, dirty-tracked, autosaved and reviewable as a diff.

## Tools and shortcuts

| Tool | Key | Behaviour |
|---|---|---|
| Select | `V` / `Esc` | Click, Shift/Ctrl-click to toggle, drag a box (left→right = window, right→left = crossing). Dragging an object moves it. |
| Place component | `A` | Pick a symbol, click to place; `R` rotates, `X`/`Y` mirror the ghost; stays armed; right-click or `Esc` stops. References count up (`R1`, `R2`, …). |
| Wire / Bus | `W` / `B` | Click to start, click for corners (orthogonal elbows; `Space` flips the elbow), click on a pin/wire end or double-click to finish. |
| Junction | `J` | Click to place (never duplicated). |
| Net label / Global label | `L` / `Ctrl+L` | Name first; a trailing number counts up on each placement (`D0`, `D1`…); `R` turns the label. |
| Power symbol | `P` | Pick a power symbol (net name = its default, editable later). |
| Move | `M` | With a selection: carries it with the pointer, click to drop. Without: press and drag an object. |
| Drag | `G` | Grabs exactly the object or wire segment under the pointer, ignoring the selection. |
| Rotate / Mirror / Delete / Duplicate | `R`, `X`/`Y`, `Del`, `Ctrl+D` | Apply to the selection immediately; with nothing selected the palette button waits for a click. |
| Copy / Cut / Paste | `Ctrl+C/X/V` | In-app clipboard (objects plus the symbols they need, so pasting works in any project); paste shows a ghost, click drops it. |

`Alt` while pressing an object disables rubber-banding for that move. `Esc` cancels the gesture in
progress, then leaves the tool, then clears the selection. The inspector edits reference, value,
footprint, label names and power-net names through commands.

## Connectivity during edits

- Visual crossings never connect. A junction is added only where **three or more connection ends
  meet** (a wire end counts 1, a wire passing through 2, a pin 1).
- **Move, rotate and mirror keep connections.** Wires that are not selected but end on a moved
  connection point follow it (orthogonal elbows are inserted so wires stay axis-aligned);
  junctions, power symbols, labels and no-connect marks sitting on a moved connection point travel
  with it. A wire carrying a junction/pin/label in its middle keeps that part of its path.
- Re-routing tries several styles (two L shapes, two Z shapes, detours) and **verifies the result**
  against the circuit's net topology (which pins share a net, plus net names). If no style keeps
  every net intact, the edit is still applied but `connectivity_preserved = false` is reported as a
  warning in Messages ("nets were merged or split — undo and place elsewhere"). Nothing pretends
  that connectivity was preserved when it was not.
- Deleting wires also removes junctions that no longer join three ends.

## Limits (documented, not hidden)

- Only the root sheet is editable in the UI; hierarchical sheets exist in the model and connectivity
  but have no editing tools yet.
- Re-routing is not a general autorouter: dense layouts can leave no valid style, which is reported.
  Labels sitting in the middle of a wire that is re-routed away from them are not moved.
- Symbol text is not mirrored or rotated with the symbol; no multi-unit symbols; no bus entries.
- The clipboard is in-app only (not the system clipboard).
- The bootstrap symbol library is small (see `docs/schematic-connectivity.md`).

## Verification

- `crates/schematic/tests/editing.rs`: unit + property tests (reversibility, truthful reporting of
  connectivity, free-space moves always preserve topology, junction rules, paste, properties).
- `apps/desktop/src-tauri/tests/editor.rs`: the editor driven by pointer events and commands.
- `apps/desktop/e2e/edit.mjs`: the real UI building and editing a circuit (screenshots in `e2e/out`).
