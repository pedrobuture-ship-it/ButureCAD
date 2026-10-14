# Track routing, vias, Walk Around and Push & Shove (roadmap stages 17–18, 21–22)

Basic interactive routing in the **PCB** tab. Everything created by one route (tracks **and**
vias) is a single `PcbEditCommand`, so one undo removes the whole route; tracks carry their net;
the ratsnest updates as soon as the route is committed.

## Using it

| Key / action | Effect |
|---|---|
| `X` | Route tool. Click a pad, via or track end to start: the **net comes from it** (empty space starts a net-less route) |
| click | adds a corner; the pointer leg follows the angle mode (Angle selector in the bar) |
| `Space` | the other of the two possible elbows |
| `V` (while routing) | **through via** at the pointer; routing continues on the next copper layer (the active layer follows) |
| click a pad/via/track of the **same net** | the route ends exactly on its centre/end |
| `Enter` / double click | finish at the pointer |
| `Esc` | cancel the route (second `Esc` leaves the tool); nothing is created |
| `Shift+V` | Via tool: one via at the click, on the net of the copper under it |
| bar | track width (mm), angle (45° / 90° / free), via diameter/drill, active layer |

Angle modes (`eda-core/src/routing.rs`, exact integer geometry): **45°** = one straight leg and
one 45° leg; **90°** = two orthogonal legs; **free** = one straight segment. Collinear vertices
are merged and zero-length steps dropped.

While routing the preview shows: the track in its layer colour, a translucent **clearance halo**
(width + 2 × 0.2 mm), vias, **the whole net highlighted**, **target rings** on the ends of the
net's remaining ratsnest lines, and a ring on the pad it will snap to. The status bar shows the
net, layer, length and number of vias.

A route refuses to end on copper of another net (it says so and continues) and on a pad that is
not on the current layer (place a via first).

## Editing afterwards

Inspector: track layer, width, net and both endpoints; via diameter, drill and net (validated:
the drill must stay inside the via). Dragging an end of a selected track moves it **and the ends
of same-net tracks on the same layer that meet there** (one undo step). Delete works on any
segment.

## Vias

Through vias only are created. The model already has `ViaKind::{Through, Blind, Buried, Micro}`
with from/to layers, validated against the stackup and used by the ratsnest, so blind, buried and
micro vias need only tools (roadmap 26) — files and boards will not change.

## Known limits (stated, not hidden)

* **Clearance is displayed, not enforced**: routes do not avoid other copper and nothing checks
  clearance yet (DRC is roadmap 19, net classes 20, walk-around and push & shove 21–22).
  The width, clearance and via size are global defaults until net classes exist.
* A route does not snap to the *exact* centre of a pad of another layer, and cannot start on a
  zone.
* Track vertices are edited by dragging ends or via the Inspector; there is no segment-splitting
  tool yet.

## Walk Around (roadmap 21)

The bar's **Obstacles** selector picks how the router treats other copper:

* **Walk around** (default) — every leg avoids the copper of other nets (and netless copper),
  keepouts that forbid tracks, and the board edge, keeping the **net class clearance** (larger of
  the two nets' classes, as DRC applies) plus half the track width. The preferred 45°/90°/free
  elbow is kept when it is free; the other elbow is tried next; only then does the leg detour.
* **Push & shove** — see below.
* **Ignore obstacles** — the old behaviour: exactly what you click. DRC reports what it violates.

If no way exists the preview turns red, the bar says why ("the target is too close to other
copper", "no path around the obstacles", "too many obstacles…") and a click adds nothing.
Finishing a route is also **validated with the DRC rules** (clearance, shorts, copper-to-edge,
including vias); a route that would break one is refused with the DRC message and nothing is
created. Obstacle mode "Ignore" skips both checks.

### Architecture (`crates/router`, kept separate on purpose)

| Layer | Module | Role |
|---|---|---|
| geometry / collision | `collision` | exact integer predicates: convex hull, point strictly inside a convex polygon, segment through a convex polygon's interior (touching a boundary is allowed) |
| obstacles | `obstacle` | copper/keepouts/edge → inflated convex polygons (a circumscribed 16-gon around every corner, so clearance is only ever over-estimated), R-tree indexed; cached per `(revision, net, layer, width)` |
| path finding | `pathfind` | A* over the visibility graph of obstacle corners inside a corridor that widens only if needed (2500-corner cap) |
| entry points | `walkaround` | `route_leg` (elbow → other elbow → detour) and `walk_around` |
| DRC validation | `validate` | the finished route is checked with `buturecad-rules` (`check_candidate` + edge rules) — the router never trusts its own approximation |

Property test: on random boards, every route the router returns has **zero** DRC violations.
Measured (`cargo run --release -p buturecad-router --example router_bench`, 3000 tracks on
200 × 150 mm): building the obstacles 6 ms, a 22 mm leg 30 ms on average, worst 175 ms.

### Limits

* Detours follow obstacle corners, so they run at **any angle**; there is no 45° clean-up yet.
* Obstacles are convex over-approximations (a concave keepout is replaced by its hull, a
  circumscribed polygon is up to 2 % larger than the circle), so a very tight gap that DRC would
  accept can be refused.
* Zones are not obstacles yet (they are not copper until stage 25). Vias placed with `V` are
  checked when the route finishes, not at the moment of placing.
* Walk Around never moves existing tracks — Push & Shove does.

## Push & Shove (roadmap 22)

Select **Push & shove** in the bar's *Obstacles* menu. The new route may displace existing
**tracks of other nets**; pads, existing vias, the board edge, keepouts and tracks of the routed
net are fixed. While you move the pointer the pushed tracks are drawn at their new place and the
bar says "pushing N track(s)"; nothing in the model changes until the route ends, and then the
route **and every displaced track** are one `PcbEditCommand` ("Route track (push and shove)"),
so a single undo restores everything.

How it works (`crates/router/src/shove.rs`):

1. A *pusher* (the new copper, then every track that has just moved) closer than the required
   clearance to a movable track translates it along the line between their closest points by
   exactly the missing distance (+ 2 µm). Ends that meet another track of the same net move
   together, so connected copper **stretches** instead of tearing.
2. An end that sits on a pad or via stays; a track anchored at one end **pivots** about it
   (refused when the clash is within 20 % of its length from the anchor); a track anchored at both
   ends cannot be pushed.
3. Moved tracks push others in turn until nothing moves (at most 600 pushes).
4. The final state is **re-verified with the DRC rules** (new copper and every moved track at their
   final positions against everything else, plus the board edge) — independent of the heuristic.

If pushing is impossible — a track crosses the route, something fixed is in the way, or the
result would break a rule — the leg **falls back to Walk Around** and the bar says why ("cannot
push (…); walking around"). If that fails too the leg is refused like in Walk Around. The router
never commits geometry that breaks a rule.

Tests: unit and scenario tests in `crates/router/tests/shove.rs` (single push, cascade through
three tracks, stretching an L, anchored tracks, crossing, board edge) and a property test (every
accepted result is DRC-clean after being applied through the Command Engine); workspace scenarios
in `apps/desktop/src-tauri/tests/pcb_route.rs` (push is one undo step; fallback to walk around).
Cost: ~3.5 ms per shove on a board with 3000 tracks (the fixed-obstacle index is rebuilt each time).

Limits: vias and pads are never pushed; pushed tracks keep their vertices (no new corners are
added, so a pushed track can end up at any angle); a track crossing the new route cannot be slid
out of the way; only the active copper layer's legs and vias are shoved (new vias push on every
layer); no "hug" / "shove with corner optimisation" polish yet.
