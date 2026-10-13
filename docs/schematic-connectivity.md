# Schematic model and connectivity

`buturecad-schematic` separates **data model** (`model`, `schematic`), **connectivity**
(`connectivity`, `bus`) and a temporary **bootstrap library** (`builtin`). Rendering and tools
live outside the crate. Connectivity is computed from the model; pixels never matter.

## Entities

Component, Symbol, Pin, Wire, Bus, Junction, NetLabel, GlobalLabel, PowerSymbol, Sheet (a sheet
instance placed on a parent), plus `HierLabel` (needed to make hierarchy real), `NoConnect` and
`SheetPin`. Objects live in a sheet; sheets form a tree rooted at the reserved root id `0`.
A sheet may be instantiated **once** (multi-instance reuse is not supported). Symbols are
embedded in the schematic (self-contained projects); pins are placed through
`Placement { position, rotation (CCW quarter turns), mirror }` with exact integer math.

## Connection rules

1. Wires that **end at the same point** are connected.
2. A wire crossing another, or ending on its interior, is **not** connected unless a **Junction**
   is at that point.
3. Pins, labels, power symbols, sheet pins and hierarchical labels attach to every wire that passes
   through or ends at their point, and to each other when they coincide. (A wire passing over a
   pin therefore connects to it, as in other EDA tools.)
4. Net labels merge same-named nets **within a sheet**; global labels and power symbols merge by
   name **across the design**; a sheet pin merges with the hierarchical label of the same name in
   its child sheet.
5. Buses name groups of signals (`D[0..7]` → `D0`…`D7`) and make **no** electrical connection.

## Net naming

Priority: power symbol > global label > local/hierarchical label > sheet pin > generated name
(`Net-(R1-Pad2)`, from the smallest reference/pin). Ties at the same priority resolve
alphabetically and are reported as `Issue::NameConflict`. Local names are qualified by sheet
(`/NAME` on the root sheet, `/Sheet/NAME` below it); global and power names are bare. Net order
and ids are deterministic.

## Structural issues (not ERC)

`Connectivity::issues()` reports name conflicts, sheet pins without a hierarchical label (and
vice versa), bad bus labels and unreachable sheets. Electrical rule checking proper (pin
compatibility, power sources, dangling wires, …) is a later stage and will consume this graph.

## Changes

Edits are expressed as `Change { sheet, id, before, after }` sets, applied atomically
(`Schematic::apply`) and reverted exactly (`revert`); stale or invalid changes fail the whole set.
The Command Engine wraps them in `SchematicEditCommand`.

## Limits

- `builtin` is a small bootstrap library, to be replaced by the library system (stage 31).
- No multi-unit symbols, no multi-instance sheets, no bus entries/bus-to-net electrical mapping.
