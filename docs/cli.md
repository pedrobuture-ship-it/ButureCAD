# buturecad-cli (roadmap stage 38) and headless core (stage 39)

`apps/cli` builds the `buturecad` binary. It links **the same crates as the desktop app** —
`buturecad-scripting`, `eda-core`, `schematic`, `pcb`, `rules`, `manufacturing`, `io` — and none of
the GUI: no Tauri, no window system, no renderer, no GPU stack. No logic is re-implemented in the
CLI; it only parses arguments and prints.

```text
buturecad build <script.beda> --out <project-dir> [--name N] [--param k=v]...
buturecad check <script.beda | project-dir>
buturecad erc <project-dir>
buturecad fill <project-dir>
buturecad drc <project-dir>
buturecad export-gerber <project-dir> --out <dir | file.zip> [--silk-values] [--rotation-offsets <file.csv>]
buturecad export-bom <project-dir> [--out file.csv]
buturecad inspect components|nets|board <project-dir>
buturecad agent serve|proposals|show|apply|reject <project-dir> …   (docs/agent-api/protocol.md)
buturecad import <file> --out <project-dir> [--format …] [--report …]   (docs/import.md)
buturecad help | version                      options: --json  --quiet
```

* `build` compiles the script and generates a saved project through the Command Engine (and copies
  the script to `design.beda`). Diagnostics are printed as `file:line:col: error[CODE]: message`.
* `export-gerber` / `export-bom` read purchasing data (`MPN`, `Manufacturer`, `Supplier`, `SPN`) from the schematic; `--silk-values` also prints values; `--rotation-offsets` applies pick-and-place rotation corrections (`docs/manufacturing.md`).
* `fill` refills every copper zone around the current copper (through the Command Engine) and saves; edits made outside the app do not refill zones, and DRC reports a stale fill.
* `build` and `check` read symbols/footprints from `--libs <folder>` or the `libraries/` folder next to the script.
* `erc` / `drc` use the real connectivity graph / board geometry; `export-gerber` runs a fresh DRC
  and the manufacturing preflight first and **writes nothing when the preflight has errors**
  (exit 1). With `--out x.zip` it writes `production.zip`; otherwise it writes the files into the
  folder. Success is not fabrication sign-off.
* `--json` prints one JSON document per command (stable keys: `errors`, `warnings`, `violations`,
  `diagnostics` with `line`/`column`, `components`, `nets`, `board`…).
* **Exit codes:** `0` ok · `1` design problems (script errors, ERC/DRC errors, blocked export) ·
  `2` usage or I/O problems (message on stderr).

## Headless guarantee (stage 39)

```text
ButureScript → lexer/parser → semantic analysis → EDA IR → Command Engine → schematic
            → "Update PCB from Schematic" → board → ERC → DRC → Gerber/Excellon/BOM/pick-and-place
```

runs with no React, no Tauri and no graphical window:

* `scripts/headless-ci.sh [script.beda]` runs the whole pipeline with the real binary
  (it was run with `DISPLAY` and `WAYLAND_DISPLAY` unset).
* `apps/cli/tests/cli.rs` runs the same flow in-process and through the compiled binary, parses
  the produced Gerber back with the independent reader, checks exit codes, and asserts with
  `cargo tree` that the CLI's dependency tree contains no `tauri`, `wgpu`, `winit`, `webkit`, `gtk`,
  `buturecad-renderer`, `buturecad-viewer3d` or `buturecad-desktop`.
* The board → manufacturing bridge (`buturecad_manufacturing::board`) moved out of the desktop
  app so the GUI and the CLI produce byte-identical production files.

The desktop app is a client of this core: its code editor, fab panel, library manager, plugins and
simulation panels call the same crate APIs.

## Limits

* No `simulate`, `library`, `plugin` or `agent` subcommands yet; no shell completion; no `watch`.
* Arguments are parsed by hand (no `clap`) to keep the dependency tree small.
* `inspect components` lists schematic components (and their placement when on the board).
