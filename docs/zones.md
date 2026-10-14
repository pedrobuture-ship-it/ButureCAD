# Copper zones (roadmap stage 25)

A zone is a polygon on a copper layer, assigned to a net, that is **filled** with copper everywhere
the rules allow. Zones are real board objects (`PcbObject::Zone`) edited through the Command
Engine; the fill is stored inside the zone so the board, DRC, ratsnest and (later) manufacturing
all use exactly the same copper.

## Using it

| Key / action | Effect |
|---|---|
| `Z` | Zone tool. Click the corners of the outline; click the first corner (or `Enter`, or double click) to close it. `Esc` cancels |
| bar → **Zone net** | the net assigned to new zones (the zone is created on the active copper layer) |
| `B` / bar → **Fill zones** | recompute every fill from scratch |
| `Ctrl+B` | clear all fills |
| bar → **Auto fill** | refill zones after every edit that changes copper near them |
| inspector (zone selected) | net, priority, clearance (empty = net class), minimum width, pad connection (thermal relief / solid / none), thermal gap and spoke width, remove islands |

Creating a zone fills it immediately and the creation and the fill are **one undo step**. With
*Auto fill* on (default), every edit — routing, moving a footprint, adding a via — refills the
zones it affects **inside the same undo step**, so undo returns both the edit and the pour.

## What the fill is

Outline minus (see `crates/eda-core/src/zonefill.rs`):

* copper of other nets and netless copper, grown by the **clearance** (the zone's own clearance, or
  the net rule — the larger one);
* the board edge grown by `copper_to_edge`, and keepouts that forbid zones;
* pads and vias of the zone's net according to the connection: **solid** (copper flows into the
  pad), **thermal relief** (a gap of `thermal_gap` crossed by four axis-aligned spokes of
  `thermal_spoke`) or **none** (isolated like another net). Tracks of the zone's own net are
  merged with the pour;
* the fills of **higher-priority** zones of other nets on the same layer, grown by the clearance
  (higher priority fills first; equal priority: older zone first).

Then copper narrower than **minimum width** is removed (a morphological opening intersected with
the unopened fill, so it can never add copper), and with **remove islands** every piece that
touches no copper of the zone's net is dropped.

All geometry is exact integer arithmetic in nanometres (`i_overlay`, i64 engine). Obstacles are
circumscribed polygons of the real shapes (a 16-gon around each corner or centre), so the clearance
is only ever over-estimated. Output is deterministic: the same board gives bit-identical fills.

## Staleness

Each fill stores a **signature** of its inputs (zone parameters, the copper, keepouts and edge near
it, the rule values, and the fills of the zones it yields to). `stale_zones` recomputes the
signatures (5 ms on a 4600-object board) and a fill that no longer matches is reported by DRC
(`zone_stale`, warning) and in the bar. Edits far from a zone do not make it stale.

## Connectivity and DRC

The fill is copper of the zone's net: the ratsnest joins everything that touches a piece of it (a
ground pour completes its connections), and the DRC checks every piece against other copper with
exact distances to the filled area (holes respected). The router does not treat zones as obstacles —
routing over a pour of another net is normal, and the pour is refilled around the new track.

## Performance

`cargo run --release -p buturecad-eda-core --example zonefill_bench`: a 200 × 150 mm pour with
4000 tracks and 600 vias fills in ~0.7 s (81 k contour points); a full DRC with that pour takes
~0.7 s (`-p buturecad-rules --example drc_zone_bench`). A fill is computed per zone from the
objects near it (spatial index), not from the whole board.

## Limits

* Fills are **stored in the project file** (they make the file larger and change with every
  refill); a zone with no stored fill is simply unfilled until `B`.
* Thermal spokes are axis-aligned only; pads whose size exceeds the gap are handled by the
  circumscribed polygon, not an exact annulus.
* Zones are not obstacles for the router and not yet modelled as zone-to-zone connection (two
  zones of one net on one layer are separate pours); no hatched fills, no per-layer zone
  duplication, no outline editing after creation except in the inspector's net/rule fields.
* Keepouts have no drawing tool yet (they can be stored and are honoured by the fill).
* Zone fills are drawn from a triangulation of the stored regions; they cannot be picked (select a
  zone by its outline).
* Manufacturing (Gerber) from fills arrives with stage 33.
