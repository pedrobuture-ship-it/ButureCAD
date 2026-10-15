# ButureScript (roadmap stages 35–36)

A declarative, unit-aware language for describing electronics as code. Files use the extension
`.beda`. The language knows nothing about rendering: it compiles to the **EDA IR**
(`buturecad_eda_core::ir::Design`), which the core applies through the Command Engine.

```text
source ─ lexer ─▶ tokens ─ parser ─▶ AST ─ elaboration + semantic analysis ─▶ EDA IR
                                      (diagnostics: severity, code, line:column, message)
EDA IR ─ ir_apply (Command Engine) ─▶ schematic (components, global labels / power symbols)
                                       ─ "Update PCB from Schematic" ─▶ footprints + nets
                                       ─▶ outline, layers, placement, tracks, vias, zones (filled), rules
```

Crates: `buturecad-scripting` (lexer, parser, AST, evaluator, diagnostics, footprint-name resolver),
`buturecad-eda-core` (`ir`, `ir_apply`). Source files are untrusted: the evaluator has step
(2 000 000), loop (100 000), recursion (64) and module-depth (16) limits and no access to files,
network or native code.

## Grammar (informal)

Statements start with a keyword, so they need no terminators. Comments: `// …`, `# …`, `/* … */`.

```text
project "name"
board { outline = rect(0mm,0mm,50mm,30mm)  layers = 2 }
component NAME { value = "100nF"  footprint = "0402"  symbol = "Buture:C"  part = "…"  <any> = "…" }
net NAME { U1.1, R1.2 }                      # named net with pins
connect A.1 -> B.2 -> GND                    # one net; endpoints are pins (REF.PIN) or net names
connect U1.VDD -> C1.1, C2.1                 # fan-out
place REF at (x, y) [rotate 90] [side top|bottom]
track NET on F.Cu width 0.25mm path (x,y) -> (x,y) -> …
via NET at (x, y) [diameter 0.8mm] [drill 0.4mm]
zone NET on B.Cu { outline = rect(…)  clearance = 0.3mm  min_width = 0.2mm  priority = 1 }
rule { clearance=… min_track_width=… min_via_diameter=… min_via_drill=… min_annular_ring=… copper_to_edge=… }
class Power { width = 0.5mm  clearance=…  via_diameter=…  via_drill=…  nets = [VCC, GND] }

let x = expr            param p = expr          # `param` can be overridden: --param p=value
fn name(a, b) = expr                            # pure functions (see only globals + parameters)
module Name(port VDD, port GND, value = "100nF") { …statements… }
use inst = Name(VDD = VCC, GND = GND, value = "1uF")
for i in 0..8 { … }                             # end exclusive
if cond { … } else if cond { … } else { … }
```

* `LED[i]` appends the index to the name: `LED3`. `U1.(expr)` computes a pin number.
* Net names with characters outside identifiers are quoted: `"+3V3"`.
* Pins are matched by number or name against the symbol (built-in symbols are checked at compile
  time with line:column; other symbols are checked when applied).
* Default symbols by reference prefix: R C L V I D LED (`Buture:R` …); anything else needs
  `symbol = "Library:Name"`.
* Footprint names: `0201 0402 0603 0805 1206` (also `R_0603`), `soic8`, `dip14`, `header2x5`, or a
  generator call such as `soic(pins=14, wide=true)` / `qfn(pins_per_side=8)`. Generated land
  patterns are nominal starting points: verify against the datasheet.
* A component declared inside a module gets the reference `<instance>_<name>` (`dec1_C`); nets
  inside a module are local (`<instance>_<name>`) unless bound through a `port`.
* Unconnected nets/components, missing footprints, unplaced parts are **warnings**; syntax,
  dimension, name, pin, short-circuit and range problems are **errors** and stop the build.

## Libraries (stage 47)

`symbol = "Library:Name"` and `footprint = "Library:Name"` can name parts of user libraries
(`*.bsymlib.json`, `*.bfplib.json`). `buturecad build/check` read the `libraries/` folder next to the
script, or `--libs <folder>`; the desktop app and library files are the same format (see
`docs/library-manager.md`). Library symbols are embedded in the generated project and their pins are
checked at compile time. Net names that are not identifiers are written quoted in every statement
(`net "+3V3" {…}`, `track "+3V3" …`, `via "+3V3" …`, `zone "+3V3" on In2.Cu {…}`).

## Units and dimensions (stage 36)

Every number is a quantity with a dimension: `nm um mm mil inch` (length), `V mV`, `A mA`,
`ohm kOhm MOhm` (resistance = V/A), `Hz kHz MHz GHz`. `+ - %` and comparisons need equal
dimensions; `*` and `/` combine them (`5V / 2A` is a resistance and compares with `ohm`).
`10mm + 5V` is an error: `incompatible dimensions: length + voltage`. Lengths become whole
nanometres; a length that is not (e.g. `0.0000001mm`) is an error, never silently rounded.
Component values can be quantities (`value = 4.7kOhm` → `"4.7k"`). Built-ins: `rect abs sqrt min
max floor round str`.

## Diagnostics

`line:col: error[E-dimension]: message` — codes: `E-syntax E-unit E-dimension E-type E-undefined
E-duplicate E-symbol E-pin E-short E-layer E-footprint E-value E-property E-limit E-precision
E-scope E-argument E-arity E-param`, warnings `W-unused W-footprint W-unplaced W-net W-global`.

## Limits

* Generation, not merging: the target project must be empty. Re-running a script produces a new
  project (deterministically); applying it on top of hand edits is stage 37's design work.
* The schematic is auto-laid-out in rows by symbol size, with global labels at pins (no wires); every
  build verifies on the connectivity graph that it connects exactly what the design says. It is
  correct electrically but not pretty. There is no way to mark a pin "no connect" yet. Net classes, zones and tracks are supported; differential pairs,
  length targets, keepouts, silkscreen text and board text are not yet.
* Rotation is a multiple of 90°. Board thickness / physical stack-up is not settable yet.
* No `import`/`include` of other `.beda` files yet.
