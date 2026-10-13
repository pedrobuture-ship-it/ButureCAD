# PCB editor (roadmap stage 14)

The **PCB** tab edits the board model of `docs/pcb-core.md`. Everything is drawn by the wgpu
renderer (pads, footprints, outline, graphics, tracks, vias: no DOM/SVG), and every edit is a
`PcbEditCommand` through the Command Engine (undo/redo, dirty state, autosave, recovery).
Pure geometric operations live in `crates/eda-core/src/pcb_ops.rs`; the scene in
`apps/desktop/src-tauri/src/pcb_scene.rs`; the interaction layer in `workspace/pcb_editor.rs`.

## Tools and commands

| Action | How |
|---|---|
| Board outline | `O` two corners (4 `Edge.Cuts` lines in one undo step); polyline tool: click corners, click the first point to close. The bar warns while the outline is missing or not a closed loop. |
| Place footprint | `A`, pick from the project's footprints and the libraries (a library footprint is embedded in the same undo step); `R` rotates and `F` flips the ghost; unlinked footprints get the placeholder reference `REF**` until "Update PCB from Schematic" (stage 15) |
| Select | click, shift/ctrl toggle, box (window left→right, crossing right→left), `Ctrl+A`; middle button pans |
| Move | drag (live preview; one undo step) |
| Rotate | `R` 90° counter-clockwise about the (grid-snapped) selection centre |
| Flip | `F` to the other side about the footprint origin: mirrors the pads, moves SMD copper to B.Cu and silkscreen/fab/courtyard/mask/paste to the back layers |
| Lock | `K` / `Shift+K`; locked footprints refuse move, rotate, flip, align and delete |
| Align / distribute | left, right, top, bottom, centres; distribute evenly by centre (≥ 3 objects). Menu/palette commands |
| Duplicate | `Ctrl+D`: a new unconnected, unlinked part (`U?`); tracks/vias/zones copy without a net |
| Delete | `Del` (locked footprints are kept and reported) |
| Inspector | reference, value, position, rotation, side, lock; track layer/width/net/ends; via diameter/drill/net; zone net; text |

## Layer manager and active layer

The Inspector's **Layers** list shows every layer of the stackup — copper (F.Cu, In1…, B.Cu), mask,
paste, silkscreen, fab, courtyard, Edge.Cuts, 8 mechanical and 8 user layers — with a visibility
checkbox (hidden layers are neither drawn nor pickable) and **set active**. The bar changes the
copper layer count (2–16 in the UI, 32 in the model; `SetStackupCommand` refuses while a track or
zone uses a layer that would disappear).

## Rendering

Pads are real shapes in board coordinates: rectangles, circles, ovals and rounded rectangles as
exact/approximated polygons, custom pads as polygons, one shape per copper layer they occupy;
drills, pad numbers, mask and paste outlines at each pad's margins, footprint graphics on their
board layers, reference/value text, and a small pad-centre dot that routing snaps to.

## Known limits

- Rotation is in quarter turns; no arbitrary angles.
- Zones and keepouts can be stored and loaded but have no creation tools yet (roadmap 25).
- No per-footprint 3D rendering; mask/paste are outlines at the pad's own margin.
- No copy/paste between boards; no arrays; no "move to layer".
