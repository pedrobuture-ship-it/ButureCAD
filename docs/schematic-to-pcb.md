# Update PCB from Schematic (roadmap stage 15)

`Update PCB from Schematic…` (PCB bar, command palette, `F8`) compares the schematic with the
board, **shows a preview**, and only then applies the changes as **one undo step**. The planner
is `buturecad_eda_core::sync::plan_update`: pure (it reads the project and returns data), so the
preview can never change anything, and the plan is the exact change set that runs through the
Command Engine (`UpdatePlan::apply`). A plan is single-use and is refused if the project changed
after the preview.

## What is detected

| Finding | Effect |
|---|---|
| Component without a board footprint | **Add** a footprint on a staging grid to the right of the board (nothing is dropped on top of existing work) |
| Footprint whose part disappeared | **Remove** it (never if locked: kept and reported, its pads leave their nets) |
| Changed footprint assignment | footprint key changes, **placement kept** |
| Changed reference / value | text updated in place |
| Unlinked footprint with the same reference | **linked** (adopted) instead of duplicated |
| New / removed / renamed nets, changed pad nets | listed per pad |
| Part without footprint, unknown footprint, pins without a pad, unannotated or conflicting references | **warnings** (never silently fixed) |

Footprint definitions the project lacks are looked up in the library folders and embedded in
the same step.

## How placement and routing are preserved

* Parts are tied to footprints by `PlacedFootprint.link` (the lowest object id among a part's
  units), so renaming a reference does not recreate the footprint.
* Nets are matched by name. A net whose name changed but whose pads are identical is *renamed*
  with the same id, so its tracks, vias and zones keep their net.
* A net that no longer exists is deleted; the tracks, vias and zones that used it stay exactly
  where they are with **no net** and the preview says how many.
* Only nets with at least two pads become board nets.

Multi-unit parts are one footprint. Pad↔pin matching is by number: schematic pin `3` is the pad
numbered `3`.

## Known limits

* A footprint is added/updated from the component's `footprint` field (`Library:Name`); there is
  no footprint-assignment tool yet other than editing that field in the Inspector.
* Pins whose number has no pad are warned about, not mapped.
* Only the root sheet's hierarchy is flattened through the connectivity engine; nothing here is
  sheet-aware beyond that.
