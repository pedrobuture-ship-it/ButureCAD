# Simulation (roadmap stage 34)

Tab **Simulation** (`Ctrl+9`). The schematic's **connectivity graph** (not its drawing) is turned
into a flat netlist, run by a backend, and the result is plotted inside ButureCAD.

## Analyses

Operating point (table), DC sweep of a V/I source, AC (decade sweep → magnitude in dB and phase in
degrees per node, log axis), transient (fixed step). Values accept SPICE suffixes: `k`, `meg`
(mega), `m` (**milli**, as in SPICE), `u`, `n`, `p`; trailing units (`F`, `H`, `V`, `Ω`) are ignored.

## Backends (abstraction: `buturecad_simulation::Backend`)

| Backend | Supports | Notes |
|---|---|---|
| **Built-in linear engine** | R, C, L, independent V/I sources with DC, AC, `SIN(...)`, `PULSE(...)`; OP, DC, AC, transient | In-house modified nodal analysis, complex solver, trapezoidal transient (backward Euler across abrupt source edges), `gmin` 1e-12 S per node like SPICE. Verified in tests against analytic RC low-pass (AC, step response), RL current, dividers. Rejects diodes/transistors/subcircuits explicitly. Max 600 unknowns, 200 000 transient steps. |
| **ngspice (external process)** | everything ngspice supports | Runs the installed `ngspice -b` on the generated netlist, reads the ASCII raw file. Verified against **ngspice 47** (`crates/simulation/tests/ngspice_vs_builtin.rs`, skipped when ngspice is not installed): OP, AC and transient agree with the built-in engine and with the analytic RC response; a diode circuit (0.65 V drop) runs only in ngspice; a missing model is reported as an error. Reports "not installed" when absent. |

No SPICE solver is written from scratch beyond the linear subset above. Decision record in
`docs/DEPENDENCIES.md`.

## Associating SPICE data with components

Select a component in the schematic; the Inspector has: **SPICE model** (`Spice_Model`: a model
name, or a full `.model …` / `.subckt …` text that is added to the netlist — required for D, Q, M,
X), **SPICE value** (`Spice_Value`: overrides the component value, e.g. `2k` or `SIN(0 1 1k)`),
**SPICE pin order** (`Spice_Nodes`: pin numbers in SPICE node order, e.g. `2,1,3`; default is pin
number order) and **SPICE enable** (`0` = leave the part out). The element type comes from the
reference letter (R C L V I D Q M X); other references (J1, TP1, …) are listed as "left out".
Built-in `Buture:V` and `Buture:I` source symbols exist (pin 1 = +; value e.g. `DC 5`,
`AC 1`, `SIN(0 1 1k)`, `PULSE(0 5 0 1n 1n 5m 10m)`).
A net named `GND`, `AGND`, `DGND` or `0` is the ground node.

## Results

Plotted as SVG (one plot per unit: V, A, dB, deg) with per-curve toggles; curves are decimated to
1500 points for display (the full data stays in the core). An operating point is a table. The
generated SPICE netlist is shown and can be used in any other SPICE tool. A result is marked stale
when the schematic changes afterwards.

## Limits

* Built-in engine: linear only, no mutual inductors, controlled sources, switches, `.param`,
  `.step`, noise, `.tf`; initial conditions (`IC=`) are not supported; sources are independent.
* The netlist builder reads one flat schematic (hierarchy is flattened by connectivity); no
  `.include` of external files, no model library management beyond the property text.
* No cursors/measurements on the plot; no export of results to CSV yet.
