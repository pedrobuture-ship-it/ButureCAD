# Length matching and tuning (roadmap stage 24)

## Targets

A net can carry a **length target** (`target ± tolerance`, stored in the board rules, PCB schema 6,
one undo step). Select a track of the net (or point at one with the tuner) and type
`Length target/tol (mm)` in the PCB bar, e.g. `50/0.1`; empty clears it. A leg of a **differential
pair** without its own target aims at its partner's length within the pair's maximum skew.

The **length of a net** is the sum of its track lengths (centre lines): vias add nothing and pad
geometry inside footprints is ignored. It is computed from the real tracks on every change
(`eda-core::length`).

## The bar (live)

For the net under the tuner or of the selected tracks: `Length <net>: <current> mm · target <T> ±
<tol> · Δ <current − target>` and, while the tuner hovers a valid spot, `→ <predicted> mm` — the
length after the previewed meander is placed. The text turns into a warning when the net is outside
the tolerance.

## The tuner — `T`

Point at a track of a net that is too short: the track is hidden and the meander that would replace
it is drawn with its clearance halo. A click places it (the old track is deleted and the meander
tracks are created: one `PcbEditCommand` "Tune length", one undo). `Tune Length of the Selected
Track` does the same at the middle of the selected track from the keyboard or palette.

Settings in the bar: **Style**, **Amplitude** (largest bump height) and **Pitch** (centre distance of
parallel runs, never less than `width + max(clearance, width)`).

* **Serpentine** — rectangular wave; each period adds `2h`.
* **Accordion** — 45° trapezoids (no 90° corners); each period adds `2h(√2 − 1)`.

`h` is chosen so the addition equals the missing length exactly (to a few nm), never exceeds the
amplitude and is at least one pitch (so the runs of a bump keep their spacing); the meander is
centred on the pointer and kept one pitch away from the segment's ends. If the segment is too short
the preview is marked **partial** (it adds what fits; place it and tune another segment), and a
segment that cannot hold one period is refused. Both sides of the track are tried; the plan must pass
the DRC rules (clearance, shorts, board edge) against everything but the track it replaces, and
otherwise the bar says why and nothing can be placed. A net that is too long cannot be shortened by
tuning ("too long; shorten it by rerouting").

Tests: geometry (`crates/router/src/tuning.rs`: exact added length for both styles, amplitude and
side, parallel spacing, partial and error cases), net length/status, workspace scenarios
(`apps/desktop/src-tauri/tests/pcb_tune.rs`: live numbers, preview → click → target reached → DRC
clean → one undo, pair partner target, blocked meander refused).

## DRC

`length_target` (warning): a net with a set target is outside `target ± tolerance` (only once it has
tracks). Pair-partner goals are reported by `pair_skew` instead.

## Limits

* Meanders are placed on a single **straight** track; arcs, tuning across several segments, tuning
  through vias, and automatic hugging of other traces are not implemented.
* The meander of a pair leg is not coupled with its partner (each leg is tuned on its own track, on
  the side that passes validation first: left of the track direction, then right).
* Tolerance is symmetric; there is no separate minimum/maximum, no length groups (matching N nets to
  the longest) and no interactive dragging of the amplitude yet.
