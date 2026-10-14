# Layer stack (roadmap stage 26)

`Stackup` (the logical copper layers: 2, 4 … 32) says *which* layers exist; the **physical stack**
(`buturecad_pcb::PhysicalStack`, stored in the PCB file, schema 8) says what the board is made of.

## The model

* **Copper layers**: thickness (nm) and role (signal / plane / mixed) per layer.
* **Dielectric layers** between every pair of adjacent copper layers — one to eight per gap, so
  `core + prepreg + prepreg…` is expressible: type (**core**, **prepreg**, adhesive, coverlay),
  material name, thickness, relative permittivity (stored ×1000) and loss tangent (stored in ppm),
  and a `flex` flag for flexible material.
* **Solder mask** on the outside (optional) and a surface finish name.
* Total thickness is derived. Validation (`PhysicalStack::validate`) requires exactly the board's
  copper layers, dielectric in every gap, thicknesses within sane limits (1 µm … 10 mm dielectric,
  1 µm … 1 mm copper), a named material and mask only outside.

Prepared for later stages: **HDI** (thin build-up dielectrics; the template uses 60 µm prepreg on
both outsides), **rigid-flex** (flex polyimide core + adhesive in the template, `flex` per
dielectric), any even layer count up to 32. The 3D viewer (27), impedance/length work and
manufacturing documentation will read this model; nothing reads it yet beyond the manager itself.

## Layer Stack Manager

**PCB bar → Layer stack…** or *Tools → Layer Stack Manager…*

* Edit name, finish, copper layers (the count selector gives the standard FR-4 stack for that
  count), every copper thickness and role, every dielectric (type, material, thickness, εr, tan δ,
  flex), add or remove dielectric layers in a gap.
* **Templates** (built in): 2/4/6/8 layers at 1.6 mm FR-4, HDI 1+4+1, rigid-flex 4 layers.
* **Presets**: *Save as preset* writes `<name>.bstack.json` to the user library folder ("for all my
  projects") or the project's `libraries/` folder; presets are listed with their scope, loaded into
  the dialog and deleted from there. Files are validated on load; a damaged file is listed with its
  error instead of being silently dropped.
* **Apply** changes the layer count and the whole stack as **one undo step** ("Edit layer stack");
  an invalid stack changes nothing and says why. Changing the count in the dialog or with the bar's
  layer selector refuses to remove a layer that tracks or zones use (existing rule).
* **Layer ordering**: *Exchange the contents of two inner layers* (`In1.Cu ⇄ In2.Cu` …) moves
  everything — tracks, zones (refilled), keepout layer lists — from one inner layer to the other in
  one undo step; the physical layers keep their thickness and role. Blind and buried vias make the
  exchange impossible (their spans depend on the order).

## Limits

* Only through vias exist today; blind/buried/micro vias are in the model but cannot be created, so
  the HDI template is a stack only. Rigid-flex regions (different stacks in different board areas)
  are not modelled — the `flex` flag marks the material.
* Odd copper layer counts (3, 5…) are not supported (the layer model is even-only).
* The stack is not yet used for impedance, 3D or Gerber stackup documentation (later stages).
* Presets are one stack per file; there is no import of other tools' stackup formats.
