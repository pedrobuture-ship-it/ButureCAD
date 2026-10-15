# Manufacturing export (roadmap stage 29)

## Using it

Bottom panel → **Manufacturing**. *Preview manufacturing* runs a fresh **DRC**, board validation and
the manufacturing **preflight**, generates every file in memory and lists the issues (errors,
warnings, notes) and the files with size and contents. *Export production.zip…* is enabled only
when the preflight has **no errors**; the preview is regenerated at export time so the zip always
matches the current board. Warnings never block but are written to `PREFLIGHT.txt` in the zip.

## What is generated

| File | Content |
|---|---|
| `<name>-F_Cu.gtl`, `-In<n>_Cu.g<n>`, `-B_Cu.gbl` | Gerber RS-274X copper (X2 attributes, 4.6 mm, positive). Tracks = strokes; round/oval pads and vias = flashes; rectangular/rounded/custom pads and zone fills = regions. Zone holes use clear polarity, zones are drawn first. |
| `-F/B_Mask.gts/gbs` | Openings at pads (pad margin or 0.05 mm default); vias tented; NPTH holes get an opening. |
| `-F/B_Paste.gtp/gbp` | SMD pad apertures (pad paste margin), only when non-empty. |
| `-F/B_Silkscreen.gto/gbo` | Footprint and board graphics on the silk layers, only when non-empty. |
| `-Edge_Cuts.gko` | Board outline and cut-outs, 0.1 mm strokes. |
| `-PTH.drl`, `-NPTH.drl` | Excellon metric, one tool per diameter, slots with `G85`; vias are in PTH. |
| `-bom.csv` | Grouped by value + footprint, natural reference order, honours *exclude from BOM*. |
| `-pos.csv` | Non through-hole parts, design-origin coordinates (same origin as Gerber), honours *exclude from position files*. |
| `assembly-info.txt`, `PREFLIGHT.txt` | Summary and the preflight report. |

Output is **deterministic** (no timestamps; the ZIP is "stored" with fixed dates) so the same board
gives byte-identical files. Coordinates are exact integer nanometres.

## Preflight

Errors (block the zip): no closed outline, DRC errors, no copper at all, empty profile, holes
outside the outline, duplicate or empty references, duplicate layer files, blind/buried/micro vias
(not exportable yet — they would be silently dropped). Warnings: DRC warnings, empty copper layer,
holes/tracks below generic limits (0.2 mm / 0.1 mm — **defaults, not your fabricator's rules**),
components without value, footprints without mount type, custom pads without mask expansion,
unfilled zones. Notes: silkscreen text, mask defaults, "not a sign-off". Nothing is ever fixed
silently.

## Architecture

`crates/manufacturing` is decoupled from the PCB core (like the 3D viewer): input `FabInput`
(plain nm geometry), modules `gerber`, `excellon`, `tables`, `zip`, `preflight`, `package`. The
only bridge is `apps/desktop/src-tauri/src/workspace/fab.rs` (board → `FabInput`, state, Tauri
commands `fab_preview`/`fab_export`). The CLI will call the same crate (roadmap).

## Limits (honest list)

* Silkscreen text uses ButureCAD's own single-stroke font (stage 52): references and user/board texts are exported, values only with `--silk-values`; texts are not checked against pads, the board edge or each other (overlaps happen: see the reference board). Filled silk shapes are outlines; no solder-paste on THT.
* Mask expansion for custom pads is zero; no mask bridges/sliver checks; no per-fabricator rules.
* Outline arcs are exported as polylines; no slot/route (milling) layer beyond Edge_Cuts.
* Zone regions nested inside another same-layer zone's hole could be erased by that zone's clear
  polarity if drawn first (not seen in practice; zones are ordered before all other copper).
* No inner-layer plane negatives, no ODB++/IPC-2581, no fiducials/panelisation, no per-fabricator rule sets.
* The generated Gerbers are read back by the independent Gerber/Excellon readers of the Gerber
  Viewer (stage 30) in tests (`apps/cli/tests/reference_board.rs`); that is still not a fabricator's
  CAM check.

## Stage 52: silkscreen text, BOM data, rotation offsets

* **Silkscreen text.** `crates/manufacturing/src/font.rs` is a single-stroke font (own glyphs: digits,
  upper and lower case, `- + . , _ / : ( ) = * %`, `° µ Ω`; anything else is drawn as a visible box and
  reported as `silk_text_glyph`). Stroke width is `max(0.15 mm, height / 8)`. Texts below 0.8 mm
  height are reported (`silk_text_small`). Footprint texts follow their part: moved, rotated and
  flipped with it; text of a part on the bottom goes to `B_Silkscreen` and is **mirrored** (a test
  proves the bottom text is the exact mirror of the top text). Board texts on `F.Silkscreen` /
  `B.Silkscreen` are exported too. Values live on the fabrication layer by default and are printed
  on the silkscreen only with `--silk-values`. The desktop preview draws text with a bitmap font, so
  its look differs from the Gerber (size and position agree).
* **BOM** columns: `Reference,Quantity,Value,Footprint,Manufacturer,MPN,Supplier,SPN`. The data comes
  from the schematic component's properties (`MPN`, `Manufacturer`, `Supplier`, `SPN`, matched without
  regard to case; a library component fills `MPN`/`Manufacturer`/`Datasheet` when placed). Parts that
  differ only in purchasing data are separate lines.
* **Pick-and-place rotation offsets.** Assembly houses expect per-footprint corrections. Pass a file
  with `footprint_pattern,degrees` lines (`*` wildcard, `#` comments; the first matching line wins):
  `buturecad export-gerber project --out fab --rotation-offsets offsets.csv`. The offset is added to
  the placement angle modulo 360 and `assembly-info.txt` says it was applied. ButureCAD ships **no**
  table of offsets (they are specific to each house), and does not mirror angles for bottom parts.
