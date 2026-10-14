# Design Rules Check and net classes (roadmap stages 19–20)

DRC runs in Rust (`crates/rules/src/drc.rs`) on the **real board geometry**: every track, via and
pad becomes a capsule or polygon (`crates/rules/src/copper.rs`) with exact-enough edge-to-edge
distances. The UI only lists what the core reports.

## Rules

| Rule | Severity | Meaning |
|---|---|---|
| `short` | Error | copper of different (or unassigned) nets overlaps on a layer |
| `clearance` | Error | edge-to-edge distance below the **larger** clearance of the two nets' classes |
| `min_track_width` | Error | track thinner than the board minimum |
| `via_diameter`, `via_drill`, `annular_ring` | Error | via smaller than the minimums, or ring `(Ø − drill)/2` too small |
| `copper_to_edge` | Error | copper closer to `Edge.Cuts` than the board limit |
| `outside_board` | Error | copper outside the closed outline (or inside a cut-out) |
| `unconnected` | Error | a ratsnest connection that is not routed |
| `courtyard_overlap` | Warning | courtyards of footprints on the same side overlap (touching is fine) |

Pads of the **same footprint** are checked against each other (different pads, different nets);
copper of the same net never conflicts with itself. Every violation lists the offending **real
object ids**, a location, the layer and actual/required values.

## Incremental architecture

`DrcEngine` keeps per-layer R-trees of copper items. `update(doc, changes)` removes the changed
objects' items and violations, re-adds them and re-checks only them against their neighbours
(within the largest clearance), plus the ratsnest of the affected nets. Edge-cut edits and rule
changes do a full `refresh`. `tests/drc.rs` checks that the incremental report equals a
from-scratch `run_drc` after every edit of a sequence.

## In the app

* `Ctrl+Shift+D` / bottom panel **DRC** → *Run DRC*. The list shows severity, rule and message.
* Click a violation: the PCB tab opens, the offending objects are **selected** and the view is
  centred on the problem with a ring marker (dismissed by the next click).
* The report is marked **stale** when the board changes after a run. The panel re-runs on
  demand; incremental updating is implemented in the engine but the app does not yet call
  `update` per edit (a run is a full check).

## Net classes

`DesignRules` (stored in the PCB file, schema 4, `rules`) hold board limits and named
`NetClass`es (track width, clearance, via Ø, via drill, differential pair width and gap). The
`Default` class always exists; presets Power, USB, Differential and HighCurrent can be added in
**PCB bar → Net classes…**. Nets are assigned to classes in that dialog; editing is one undo step
(`SetRulesCommand`) and validated whole (drill < Ø, positive dimensions, assigned classes exist).

* The **router** takes track width and via size from the class of the net being routed.
  Typing a width / via size in the PCB bar overrides it; clearing the field (empty or `auto`)
  returns to the class.
* **DRC** uses the class clearances as above.
* Differential pair values are stored and validated but nothing uses them until stage 23.

## Limits

* Zones are not copper yet (stage 25), so they are not checked; keepouts are not checked.
* No hole-to-hole, silkscreen, mask or drill-to-copper rules; no per-layer or per-pair clearance
  overrides (only per net class); arcs on `Edge.Cuts` are approximated by 48-segment polylines.
* Clearance between footprints' own graphics is not checked; courtyards use polygon courtyards.
* The router does **not** yet avoid obstacles: it can draw tracks that DRC then reports (stage 21).
* DRC passing does not mean the board is fabrication-ready.
