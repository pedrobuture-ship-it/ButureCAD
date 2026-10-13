# PCB core (roadmap stage 13)

The board model lives in `crates/pcb` (`layer`, `board`, `footprint`) and the document that hosts
it, `PcbDoc`, in `crates/eda-core` so every change goes through the Command Engine.

## Layers (`pcb::layer`)

`LayerId` is a small integer. Copper keeps the original numbering: `0` F.Cu, `1` B.Cu, `2..=31`
inner copper 1…30. Fixed technical ids: F/B Mask 100/101, Paste 102/103, Silkscreen 104/105, Fab
106/107, Courtyard 108/109, **Edge.Cuts 110**, Mechanical 1–8 (120+), User 1–8 (130+).
`Stackup { copper_layers }` (2, 4 … 32) gives the physical order (F.Cu, In1…, B.Cu), wrapping
`next_copper`, and the list of layers a board shows. `SetStackupCommand` changes it and refuses
while a track or zone uses a layer that would disappear. Multilayer boards are therefore a
property of the document, not of any tool.

## Objects

| Object | Notes |
|---|---|
| Footprint definition | `PcbDoc.footprints` (stage 12) |
| `PlacedFootprint` | definition key, reference, value, `Placement {position, rotation (quarter turns), flipped}`, `locked`, `link` (schematic component), `pad_nets` (pad number → net id) |
| `Net` | name; objects refer to it by id |
| `Track` | layer, segment, width, `net` |
| `Via` | position, diameter, drill, `net`, `ViaKind` (`Through` today; `Blind`/`Buried`/`Micro` with from/to layers are part of the model and validated, but nothing creates them yet) |
| `Zone` | copper layer, outline polygon, net, priority — **outline only**: no fill (roadmap 25) |
| `Keepout` | outline, copper layers (empty = all), no tracks/vias/zones — stored, **not enforced** until DRC/routing (19+) |
| `BoardText`, `BoardGraphic` | any layer; graphics on `Edge.Cuts` are the board outline |

Placement order: flip (mirror about the footprint's vertical axis), rotate counter-clockwise by
quarter turns, translate. All integer-exact; arbitrary angles are not supported yet.
`PlacedFootprint::world_pads` / `world_graphics` give pads (with exact containment/distance
tests) and footprint graphics on their board layers (flipping moves silkscreen/fab/courtyard to
the back side, SMD copper to B.Cu).

## Changes and validation

`PcbObject` / `PcbChange {id, before, after}` / `PcbEditCommand` are the PCB twin of the schematic
change sets: tools compute full before/after states, the command applies them atomically (`before`
must match, `after` must validate against the document: layers exist on the stackup, referenced
footprints and nets exist), and undo/redo are exact. A failing change leaves the document
untouched.

## Files

`pcb/main.bpcb.json`, schema 3: `stackup`, `tracks`, `vias`, `footprints`, `placed`, `nets`,
`zones`, `keepouts`, `texts`, `graphics` (all but tracks/vias optional on read). Migration 2 → 3
needs no rewrite; loading validates every object and rejects duplicate ids across kinds.

## Known limits

Pad/zone/keepout geometry beyond containment (clearance, fills, DRC) arrives in stages 19–25.
