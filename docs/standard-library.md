# The `Std` standard library (roadmap stage 51, first P0 item)

A read-only library built into the program, available in the Library Manager (scope *builtin*),
the symbol picker, the footprint lists, **Update PCB from Schematic** and in ButureScript / the CLI
(`symbol = "Std:Name"`, `footprint = "Std:Name"`) with no library folder.

| | Count | Contents |
|---|---:|---|
| Symbols | ~60 | polarized capacitor, ferrite bead, fuse, crystals (2 and 4 pad), push button, Schottky and Zener diodes, NPN/PNP/N-MOS/P-MOS (SOT-23 pinouts), op-amps (5-pin, dual 8-pin), linear regulators (SOT-23-5, SOT-223), 555 timer, USB Micro-B, connectors `Conn_01x01…20` and `Conn_02x02…20` |
| Footprints | ~75 | chip 0201–1206, SOIC-8/14/16, TSSOP-8…28, MSOP-8, LQFP-32/44/48/64/100/144, SOT-23-3/5/6, SOT-223, SOD-123, SMA, DIP-8/14/16/28, pin headers 1×1…20 and 2×2…20, mounting holes, test point |
| Components | 17 | symbol + footprint + manufacturer + MPN: LM358DR, NE555DR, AMS1117-3.3, AP2112K-3.3, 2N7002, BC847B, SS14, pin headers |

The `Buture:` bootstrap symbols (R, C, L, V, I, D, LED, GND, supplies…) are unchanged.

## What "verified" means here — and what it does not

**Nothing in this library has been verified against a vendor datasheet or a fabricator.** Be precise
about the three kinds of claim:

1. **Package outlines** — pitch, pin count, body size and lead span — are the published nominal
   JEDEC / EIA values. They are asserted by `crates/pcb/tests/standard_outlines.rs`, which measures
   the generated footprints against a table of those numbers with its source. Some rows were
   **cross-checked against manufacturer package drawings** while writing this stage (SOT-23-5:
   Renesas P5.064, Microchip, AOS; TSSOP-20: NXP SOT360-1/-2, Renesas MOA; LQFP-48: NXP SOT313-3
   and an LQFP-48 controller datasheet); the others are standard nominal values *not* cross-checked
   (the test table says which).
2. **Land patterns** (pad sizes and positions) are **computed** with a simplified IPC-7351B nominal
   rule (toe 0.35 mm, heel 0.35 mm, side 0.03 mm) from the outline. They are not copied from any
   library, and vendors recommend different patterns. The tests only guarantee geometric sanity:
   valid footprint, courtyard, no overlapping pads, at least 0.15 mm between pads, the right pad
   numbering. **They do not guarantee solderability.**
3. **Symbols and component pinouts** follow the common pinout of the named package (for example SOT-23
   transistors: BJT 1 = B, 2 = E, 3 = C; MOSFET 1 = G, 2 = S, 3 = D). Parts from other manufacturers
   can differ. Every component is checked mechanically: each pin of its symbol has a pad with the
   same number on its footprint.

**Before ordering a board**, compare every footprint with the datasheet of the actual part. Closing
this gap needs a person with the datasheets (P0 item 1 of `docs/PRODUCTION_READINESS.md` stays open
for that reason).

## Using it

* **Schematic:** `Ctrl+7` Library Manager → search ("lm358", "soic-8", "header") → *Place symbol*.
  Placing a **component** brings its footprint, manufacturer and MPN along (stage 51); placing a
  symbol brings the symbol's own defaults.
* **PCB:** *Update PCB from Schematic* finds `Std:` footprints.
* **Code:**

```text
component U1 { symbol = "Std:OpAmp_Dual_8" value = "LM358" footprint = "Std:SOIC-8_3.9mm_P1.27mm" mpn = "LM358DR" }
connect U1.OUT1 -> U1."-IN1"       // pins whose names are not identifiers are written quoted
connect U1."V+" -> "+5V"
```

Chip footprints are `Std:Chip_0603` etc.; headers `Std:PinHeader_1x04_P2.54mm`.

## Limits

* No QFN/DFN/BGA entries (generate them in the Footprint Editor with the family generators and
  check them), no USB-C, no SOT-89, no electrolytic can capacitors, no crystals as footprints
  (the reference board has nominal 3.2 × 2.5 mm patterns in its own library).
* The symbols are generic and pin-compatible with the common package, not copies of vendor symbols.
* Component data has no datasheet links (none were invented).
* The library is read-only; copy an item into a user library to change it.
