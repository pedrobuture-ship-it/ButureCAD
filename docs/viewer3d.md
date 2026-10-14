# 3D viewer (roadmap stage 27)

## Using it

Tab **3D Viewer** (`Ctrl+5`). Left drag orbits, Shift/right/middle drag pans, the wheel zooms. Bar:
projection (**perspective / orthographic**), preset views (Top, Bottom, Front, Iso), *Frame*, and
toggles for substrate, solder mask, copper, silkscreen and components.

What is drawn: the **substrate** (outline extruded to the stack thickness, cut-outs and drilled
holes for vias and through pads), **solder mask** on both sides (a plate with openings at the pads
that rises over tracks and zone fills), **copper** (outer-layer tracks, pads, via annuli, zone
fills; tracks show through the mask as raised areas, and as bare copper when the mask is hidden),
**silkscreen** (footprint and board graphics on `F/B.Silkscreen`) and **components** as boxes.
Thicknesses come from the board's **physical layer stack** (stage 26): substrate = total − outer
copper − masks.

**PCB changes are reflected**: the mesh is rebuilt whenever the project revision changes (edits,
undo/redo, zone refills, stack changes) and the 3D tab renders the same frame again.

## Architecture (decoupled from the PCB core)

* `crates/viewer3d` depends on nothing of ButureCAD. Its input is `Model`: plain millimetre
  polygons/segments (outline, cut-outs, tracks, pads, via holes, zone regions with holes, silk,
  bodies). It builds a triangle `Mesh` (exact integer triangulation of polygons with holes via
  `i_triangle`) and draws it with a deterministic **software rasteriser** (z-buffer, flat shading,
  `Camera` with orbit/pan/zoom and perspective or orthographic projection). No GPU or window
  system is needed, so it is unit-tested headless.
* `apps/desktop/src-tauri/src/workspace/viewer3d.rs` is the only bridge: `build_model(PcbDoc)`
  converts the board to the viewer's `Model`, keeps the camera/toggles, caches the mesh per
  `(revision, toggles)` and handles pointer input in the 3D mode. The UI (`Viewer3dBar`) only sends
  actions; the image is a Rust-rendered frame shown in the same canvas component as the 2D editors.

## Component models — what is and is not supported

Footprints can **store references** to 3D models (Footprint Editor, stage 12). The viewer does not
parse model files yet (STEP/MCAD is roadmap stage 28): components are drawn as **boxes from the
courtyard** (else the pad extent), with a **heuristic height** by footprint name (header 8.5 mm, DIP
4.5, SOIC 1.75, BGA 1.2, QFN 0.9, otherwise 1.0 mm), and are **tinted blue** when the footprint
references a model. The bar says this explicitly. Real model rendering plugs into `Model::bodies`
(or a mesh input) when stage 28 lands.

## Limits

* Software rasteriser: no anti-aliasing, lighting is flat shading; fine for boards of tens of
  thousands of triangles (a large pour with many holes is slower, especially in debug builds).
  A wgpu path can replace it behind the same `Mesh`/`Camera` types.
* Only the outer copper layers are shown; inner layers are not visible (there is no section view).
* Pads of through-hole parts are drawn at the outer layers with their drill; slots are drawn as
  round holes; no plating barrel is modelled inside holes.
* Mask openings only at pads (vias are tented); no paste layer, no text rendering in silkscreen
  (only graphics), no board-edge plating, no component colours.
* No picking/selection in 3D and no measurement.
