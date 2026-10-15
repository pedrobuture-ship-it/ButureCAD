# Benchmarks (roadmap stage 44)

Reproducible, headless where possible. Sizes: **Small 100 · Medium 500 · Large 2000 · Extreme 5000**
components (0603 resistors in rows of 50 chained pad to pad with real tracks, a ground via per row
and a ground pour on the bottom layer, so the schematic, the board, DRC, zones and routing all have
work to do). Every size runs in a fresh process; times are medians.

```bash
benchmarks/run.sh <label> [sizes…]       # Core, headless (apps/cli/examples/bench.rs)
benchmarks/run_ui.sh <label> [sizes…]    # GUI side: scene, GPU frames, zoom, pan, selection
```

Output: `benchmarks/results/<label>-headless.{jsonl,md}` and `<label>-ui.{jsonl,md}`. Each row has
wall time, CPU time of the phase, RSS and peak RSS. UI rows also have p95 latency and frames per
second (a frame includes the pixel read-back the UI needs).

**Machine:** Intel Core i5-10600K (6 cores / 12 threads), NVIDIA GeForce RTX 2060, Linux, release
build. Numbers are a baseline for *this* machine, not promises.

## What the benchmarks found (and what was done about it)

Only changes proven by these numbers were made; one attempted optimisation was reverted because the
benchmark showed no gain.

| # | Finding | Evidence | Change | Before → after |
|---|---|---|---|---|
| 1 | **Bug:** designs of ≥ 500 schematic parts failed with `OutOfRange` (the automatic layout used a fixed 6 columns, reaching metres) | the 500-component run crashed | square-ish layout with adaptive spacing (`ir_apply`) | 500+ comps: crash → works |
| 2 | **Bug:** designs of ≥ 2000 footprints failed (the board staging grid had a fixed 8 columns) | the 2000-component run crashed | rows widen with the part count (`sync.rs`) | 2000+ comps: crash → works |
| 3 | **Bottleneck:** "Update PCB from Schematic" was quadratic (per-footprint scan of every net and every pin) — 95 % of building a design | `BUTURECAD_TRACE=1`: PCB sync plan 1624 ms of 1743 ms at 5000 | pin→net and part→pins maps built once | build @2000: 287 → 44 ms · **@5000: 1743 → 162 ms (10.8×)**; result byte-identical (project fingerprints compared) |
| 4 | **Bottleneck:** zoom re-tessellated the whole scene on one thread (82 ms at 5000 parts, PCB) | `RenderStats`: 40 of 40 zoom frames rebuilt the mesh; tessellation = 80 % of the frame | chunked, order-preserving parallel tessellation (std threads, no new dependency); mesh proven identical to the serial one by a test | PCB zoom frame @2000: 27 → 16 ms · **@5000: 84 → 27 ms (3.1×, 12 → 37 fps)**; PCB first frame @5000: 95 → 43 ms; schematic zoom @5000: 22 → 15 ms |
| – | Tried and **reverted:** a no-allocation fast path for triangle fills | tessellation 18.9–35 ms vs 24 ms: inside the noise | none | – |

Remaining observations (not optimised because nothing proves they matter yet): the first
schematic frame at 5000 parts is ~410 ms (GPU initialisation is most of it); DRC (166 ms), the
manufacturing export (202 ms) and a full project load (73 ms) at 5000 parts are all well under a
second; router walk-around is ~5 ms per route on a populated board (it is slower, ~26 ms, on the
nearly empty 100-part board because the search space is large); peak memory is 108 MB headless and
~500 MB in the GUI process at 5000 parts.

## Headless Core — after (all sizes)

| phase (median wall time) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| script compile (lex+parse+elaborate) | 1.2 ms | 5.8 ms | 25.6 ms | 65.5 ms |
| build project (apply IR + PCB sync + zone fill) | 2.1 ms | 9.8 ms | 43.8 ms | 161.8 ms |
| save project (atomic, with backup) | 2.7 ms | 11.9 ms | 6.4 ms | 16.9 ms |
| load project (parse + validate) | 1.2 ms | 5.2 ms | 27.0 ms | 72.7 ms |
| schematic connectivity | 0.2 ms | 1.1 ms | 5.8 ms | 20.2 ms |
| ERC | 0.2 ms | 0.9 ms | 4.6 ms | 18.8 ms |
| DRC | 0.8 ms | 5.2 ms | 37.0 ms | 166.2 ms |
| ratsnest (full) | 0.1 ms | 0.4 ms | 2.1 ms | 5.9 ms |
| ratsnest (move one footprint, incremental) | 0.0 ms | 0.0 ms | 0.2 ms | 0.4 ms |
| zone refill (all zones) | 1.0 ms | 3.8 ms | 19.7 ms | 90.6 ms |
| router obstacle collection | 0.9 ms | 4.2 ms | 17.5 ms | 45.2 ms |
| router walk-around (20 routes) | 520.2 ms | 91.7 ms | 91.3 ms | 103.4 ms |
| manufacturing (preflight + Gerber + drill + BOM + zip) | 1.4 ms | 8.0 ms | 49.4 ms | 202.2 ms |
| **peak memory (RSS high-water)** | 9 MB | 18 MB | 45 MB | 108 MB |

(`before-headless.md` is the same run before change 3.)

## GUI — after

| phase (median latency; FPS) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| startup (Workspace::new) | 0.0 ms | 0.0 ms | 0.0 ms | 0.0 ms |
| schematic: first frame (scene build + GPU init) | 210.6 ms | 175.5 ms | 228.7 ms | 412.1 ms |
| schematic: steady frame (cached scene) | 1.3 ms (p95 1.5 ms, 769 fps) | 1.5 ms (p95 1.9 ms, 671 fps) | 1.3 ms (p95 1.7 ms, 730 fps) | 1.7 ms (p95 3.7 ms, 544 fps) |
| schematic: zoom (wheel ± + frame) | 2.1 ms (p95 2.4 ms, 476 fps) | 1.4 ms (p95 1.8 ms, 704 fps) | 6.4 ms (p95 7.1 ms, 154 fps) | 15.5 ms (p95 18.5 ms, 64 fps) |

"steady frame" reuses the cached GPU mesh and is therefore fast at every size; "zoom" alternates a
wheel notch in and out around the centre so every frame draws the whole design and re-meshes it
(the worst case for zooming). "startup" is `Workspace::new` only; the window, the web view and
the first project load are not measured here. CPU and memory columns are in the `.jsonl` files.

## Not measured (and why)

* Real window FPS: the app draws through a web view; these numbers are the Rust side up to the
  pixel buffer that crosses the IPC boundary.
* Interactive routing latency beyond the walk-around above: Push & Shove has its own bench
  (`crates/router/examples/router_bench.rs`).
