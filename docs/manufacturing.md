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

* Silkscreen **text** is not exported; filled silk shapes are outlines; no solder-paste on THT.
* Mask expansion for custom pads is zero; no mask bridges/sliver checks; no per-fabricator rules.
* Outline arcs are exported as polylines; no slot/route (milling) layer beyond Edge_Cuts.
* Zone regions nested inside another same-layer zone's hole could be erased by that zone's clear
  polarity if drawn first (not seen in practice; zones are ordered before all other copper).
* No inner-layer plane negatives, no ODB++/IPC-2581, no fiducials/panelisation, no centroid
  rotation offsets per footprint, no MPN/supplier data in the BOM (the library model has none yet).
* The Gerbers are not independently viewed yet: the Gerber Viewer is roadmap stage 30. Tests check
  the structure and exact coordinates of what is written, and Python's `zipfile` opens the archive.
