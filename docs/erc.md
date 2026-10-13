# Electrical Rules Check (roadmap stage 11)

ERC lives in `crates/rules` (`buturecad_rules::erc`) and reads the **connectivity graph**
(`buturecad-schematic::Connectivity`) plus the model's data (pin types, references, values,
footprints). It never looks at drawn geometry: two lines that merely cross are not a connection
and ERC does not treat them as one. The desktop app only runs it, lists the result and routes
waivers through the Command Engine.

`run_erc(&schematic, &connectivity, waivers) -> ErcReport` is pure and deterministic: same input,
same report, sorted by severity, rule, fingerprint. It checks only sheets reachable from the root.

## Findings

Every finding has a rule, a severity (**Error**, **Warning**, **Info**), a message, the offending
object ids (real model ids; the first is primary), a location, and a stable **fingerprint**.

| Rule (`code`) | Severity | Detects |
|---|---|---|
| `unconnected_pin` | Error | A pin that shares its net with no other pin, power symbol, global label or sheet pin, and has no no-connect marker. All units of one part are one physical pin. |
| `pin_conflict` | Error/Warning | Incompatible pin types on one net (matrix below), per pair, capped at 100 pairs per net. |
| `no_connect_pin_connected` | Error | A pin whose type is *no connect* but that is connected. |
| `input_not_driven` | Warning | A net with input pins and nothing that can drive it (passives count as possible drivers, power symbols too). |
| `power_input_not_driven` | Error | A net with power-input pins and no power-output pin. Add a `PWR_FLAG`, a power-output pin, or waive. |
| `unspecified_pin_type` | Info | Pins of unspecified type cannot be checked. |
| `duplicate_reference` | Error | Same reference on different parts (same symbol and same unit twice, or different symbols). Several units of one symbol sharing a reference is normal. |
| `unannotated` | Warning | References ending in `?` (`R?`). |
| `missing_reference` / `unknown_symbol` | Error | A component without reference, or whose symbol is not in the project. |
| `missing_value` / `missing_footprint` | Warning | Parts without value or footprint (virtual `#…` parts such as power flags are exempt). |
| `dangling_wire` | Warning | A wire end attached to nothing. |
| `missing_junction` | Warning | A wire end on the *body* of another wire without a junction: it looks connected but is not. |
| `net_name_conflict` | Error | Different power-symbol, global-label or local-label names on one net. |
| `hierarchy_mismatch` | Error | A sheet pin without a hierarchical label inside, or a hierarchical label without a sheet pin. |
| `bus_label` | Error | A bracketed label that is not on a bus, or an invalid bus range. |
| `single_label` | Warning | A label name used once in its scope whose net connects fewer than two pins. |
| `unplaced_unit` | Info | A unit of a multi-unit symbol that is not placed. |
| `unreachable_sheet` | Info | A sheet that is not in the hierarchy (not checked). |
| `stale_waiver` | Info | A waiver that matches no current finding. |

## Pin compatibility matrix

Symmetric (`pin_conflict(a, b)`); anything not listed is compatible.

| Pair | Result | Why |
|---|---|---|
| output / power output × output / power output | Error | two push-pull drivers fight |
| output / power output × open collector / open emitter | Error | a pull-only driver conducting against a push-pull one is a short |
| output / power output × tri-state / bidirectional | Warning | fine only while the other side is disabled |
| output × power input | Warning | a signal driving a supply pin |
| open collector × open emitter | Warning | mixing pull directions |
| *no connect* × anything | `no_connect_pin_connected` | the pin was declared unconnected |

Passive, input, bidirectional×bidirectional, tri-state×tri-state, open-collector×open-collector
and power output×power input are compatible. The matrix is original engineering reasoning, not
copied from another EDA; it is covered by a test (`pin_compatibility_matrix_is_symmetric_and_as_documented`).

## Power flags

A power-input pin needs a power-output pin on its net. Power *symbols* (`GND`, `+5V`) name a
net but are not sources. For supplies that come from outside the schematic place the built-in
`Buture:PWR_FLAG` (a virtual `#FLG` part with a power-output pin), or waive the finding.

## Waivers

A waiver documents why a finding is acceptable. It is stored in the project
(`erc_waivers` in `schematic/main.bsch.json`, schema 3), added and removed through commands
(`SetWaiversCommand`: undoable, dirty-tracked, autosaved) and **requires a non-empty reason**.

* Matched by the finding's fingerprint, built from references, pin numbers and net names, e.g.
  `unconnected_pin|R1.1`, `pin_conflict|U1.3|U2.5`, `duplicate_reference|R1` — never from object
  ids — so it survives deleting and re-creating the part.
* A waived finding stays in the list, marked with its reason, and is excluded from the counts.
* A waiver that stops matching is reported as `stale_waiver` (Info) so it can be removed.

## UI

*ERC* tab of the bottom panel (also `Ctrl+Shift+E` / command palette "Run Electrical Rules
Check"): run, filter by severity, **click a finding to select its objects and centre the view on
it** (a ring marks the spot until the next click), waive with a reason, remove a waiver. The list
is marked stale as soon as the design changes.

## Known limits

* Only the root sheet can be shown, so findings inside child sheets are listed but cannot be
  focused yet (the button says so).
* Per-rule severity overrides and rule disabling are not implemented.
* Hidden power pins and implicit power connections are not modelled.
* No ERC for buses beyond label validity, and no cross-probing to the PCB (no PCB yet).
