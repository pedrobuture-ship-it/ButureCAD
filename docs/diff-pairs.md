# Differential pairs (roadmap stage 23)

## Defining pairs

**PCB bar → Net classes… → Differential pairs.** A pair has a name, a positive and a negative net,
an optional **width** and **gap** (empty = the `Differential pair width/gap` of the *positive
net's net class*, so pairs integrate with Net Classes) and a **maximum skew**. Nets named like
`USB_D+`/`USB_D-`, `X_P`/`X_N`, `X_DP`/`X_DN`, `X_DP`/`X_DM` are offered as one-click suggestions.
Pairs are stored in the PCB file's `rules` (schema 5), edited as one undo step and validated whole
(distinct nets, a net in at most one pair, nets exist, sane dimensions).

## Routing a pair — `Shift+X` (tool "Route Differential Pair")

1. Click a **pad** of a pair net. The partner pad (nearest pad of the other net on that layer)
   is found automatically; the pair starts between the two pads.
2. Click corners as with a single track (45°/90°/free, `Space` flips the elbow). `V` places
   **two vias** side by side and continues on the next layer.
3. Reach the pair's pads (either of them) to finish, or `Enter` to end in free space.

The pair is routed as **one thick track** (`2 × width + gap`), so Walk Around keeps both legs clear
of everything; the two legs are the centre path's parallel offsets with mitered corners
(`crates/router/src/diffpair.rs`), joined to the pads by short stubs. Push & shove is not available
for pairs (the bar says it walks around instead).

**Live numbers** in the bar, from the real geometry: length of + and − (stubs included), their
difference against the maximum, and the smallest edge-to-edge gap of the coupled part against the
target. A corner makes the outer leg longer (a 90° corner adds `2 × (gap + width)` to the
difference); the number turns into a warning when it exceeds the maximum.

Finishing creates tracks and vias of **both nets** as one `PcbEditCommand`
("Route differential pair"; one undo). In walk-around mode the whole thing is validated with the
DRC rules first — each leg against the board and the legs against each other — and refused with
the reason if it would break a rule.

## Rules

* **Clearance between the two legs** of a pair is the pair's gap (when smaller than the class
  clearance); every other pair of nets keeps the class clearance. This applies to DRC, walk around
  and push & shove alike (`DesignRules::clearance_between`).
* DRC `pair_skew` (warning): the summed track lengths of the two legs differ by more than the
  maximum skew. A pair is judged once both legs have copper.
* DRC and the router allow **1 nm** of slack in clearance checks (coordinates are rounded).

## Limits

* The partner pad is the nearest one; start the route on the pad you want to leave from.
* Legs are offsets of one centre path: they cannot change gap, swap polarity or fan out
  independently. When the pads are not symmetric about the first segment the stubs absorb the
  difference; if they cross, the validation refuses the route.
* Skew is **reported**, not removed — length tuning is roadmap stage 24. Via lengths are not part
  of the length; DRC skew sums track lengths only.
* Single-layer or through-via pairs only; the layer-change geometry joins each leg to its via by a
  straight stub.
