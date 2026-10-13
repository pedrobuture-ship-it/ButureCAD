# Ratsnest (roadmap stage 16)

Flying wires for connections that are not routed yet, drawn over the PCB (toggle: bar checkbox,
`Shift+R`, palette "Show / Hide Ratsnest"; the bar shows how many connections are left).
Implementation: `crates/eda-core/src/ratsnest.rs`.

## Model

For each net, the pads, tracks and vias **of that net** that touch on a shared copper layer form
*islands* (pad↔track end, track↔track, via↔pad/track on the layers the via spans, back-side
footprints on B.Cu). The ratsnest is a **minimum spanning tree over the islands** using the
shortest anchor-to-anchor distance (pad centres and vias; track ends only for islands that have
no pad or via), i.e. the cheapest way to finish the net. Copper of a *different* net never joins:
a short is not shown as a connection (DRC reports it, roadmap 19). Zones are ignored until zone
filling exists (roadmap 25).

Islands are found with an R-tree (no all-pairs scan). The spanning tree is exact up to 800
anchors per net; beyond that it uses k-nearest candidate edges (k = 6) completed to a spanning
tree — still a tree, near-minimal on real layouts, exact on regular grids (tested).

## Incremental updates

`RatsnestEngine` keeps one result per net. `affected_nets(&changes)` derives the nets a change
set can alter (pad nets of moved/placed/removed footprints, nets of tracks and vias, net
edits); `update(doc, nets)` recomputes **only those nets in one pass over the board**. The
desktop app uses it for every tracked edit; undo/redo and schematic updates refresh everything.
While a drag is in progress the affected nets are recomputed against the moved copy of the
objects (`net_ratsnest_with`, no board clone) so the flying wires follow the cursor.

Measured (`cargo run --release -p buturecad-eda-core --example ratsnest_bench`): 4000 footprints,
1500 nets, 30 500 lines → full ratsnest ≈ 33 ms, moving one footprint (8 nets) ≈ 1.9 ms.

## Limits

* Scanning for the dirty nets' copper is O(objects) per update (no per-net index yet); fine up
  to tens of thousands of objects, measured above.
* Ratsnest lines are overlay shapes recomputed each frame from the cache; they are not pickable.
* Net classes/priorities do not influence the tree.
