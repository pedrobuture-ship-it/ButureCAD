# Robustness (roadmap stage 45)

Goal: invalid files never corrupt anything, nothing crashes the application, and a crash during
Save never loses the last valid project.

## Fuzzing and property tests

No nightly toolchain or external fuzzer is needed: `crates/testkit` provides deterministic
**mutation fuzzing** (bit flips, truncation, duplicated/removed chunks, junk and huge numbers
injected, token soup) that runs inside `cargo test`. The same seed gives the same inputs, so every
failure is reproducible. `FUZZ_ITERS=<n>` scales every target (default: hundreds to thousands of
cases each; the suite was also run with `FUZZ_ITERS=20000`):

```bash
FUZZ_ITERS=200000 cargo test --release -- fuzz damaged mutated hostile tampering
```

| Area | Test | What must hold |
|---|---|---|
| Project files | `io/tests/fuzz_files.rs` | damaged `project`/`schematic`/`pcb` files → error, never a panic; reading **never modifies** files; anything accepted round-trips (save → load equal); tampered `schema_version` (0, −1, 9999, text, null, missing) and wrong format tags are refused |
| Migrations | `io/tests/project_files.rs` (v1→current chain), `fuzz_files.rs` | older schemas migrate in order; newer schemas are refused with a clear error and the file is untouched |
| Importers | `io/tests/fuzz_files.rs` | damaged SPICE / KiCad / S-expression input → clean errors; every record the importer read is accounted for (converted/ignored/unsupported) |
| ButureScript | `scripting/tests/fuzz.rs` | mutated programs, random token soup and hostile shapes (100 000-deep nesting, 2 MB strings, runaway loops/recursion/modules) → diagnostics, no panic, no hang |
| Gerber / Excellon / ZIP / rasteriser | `manufacturing/tests/fuzz.rs` | damaged files → errors, absurd coordinates refused, rasteriser never panics; **property:** the Gerber writer and reader agree exactly on random features; ZIP round-trips |
| Agent protocol / actions | `agent-api/tests/fuzz.rs` | every reply is valid JSON-RPC; no request, however broken, modifies the project; out-of-range numbers are refused |
| Plugins | `plugin-runtime/tests/fuzz.rs` | damaged manifests and WebAssembly are rejected, loaded ones run with a fuel budget |
| DRC / ERC | `rules/tests/properties.rs` | **properties** on random boards/schematics: deterministic, translation-invariant, violations only point at existing objects |
| Geometry, router, connectivity, zones, ratsnest, command engine | existing `proptest` suites (`geometry/tests/properties.rs`, `router/tests/walkaround.rs`, `router/tests/shove.rs`, `schematic/tests/connectivity*.rs`, `eda-core/tests/*.rs`) | unchanged by this stage; they already check geometric invariants and equality of incremental vs full computation |

### Defects the fuzzers found (all fixed, each with a regression test)

* Agent `move`/`place`/`route`/… accepted any 64-bit coordinate → arithmetic overflow panics. All
  coordinates from agents, ButureScript and the IR are now validated against the core range (±1073 mm).
* ButureScript: very deep nesting (`((((…`, long `-----…` chains, 100 000-term sums) overflowed the
  stack of the parser, evaluator and AST drop. Parser nesting (100) and expression height (400) are limited.
* SPICE importer: a `.model` defined twice silently lost the first one (broke the "nothing is
  dropped" rule).
* Gerber reader: coordinates near `i64::MAX` overflowed the bounds and viewport arithmetic →
  refused beyond ±1000 m; arithmetic saturates.
* (Found earlier by the benchmarks: designs of ≥ 500 parts / ≥ 2000 footprints ran out of coordinate range.)

## Atomic writes and crash recovery

* `atomic_write` writes a sibling temporary file, fsyncs it, renames it over the target and fsyncs
  the directory: readers (and a crash at any point) see the old or the new content, never a mix.
* **Saving a project writes several files**, so it is not atomic as a set. New: `save` takes a
  snapshot of the last good state, writes a `saving` marker, writes the documents (manifest last),
  then removes the marker. If the marker is found when the project is opened, the previous save
  was interrupted: the project is loaded from the **last complete backup** (never from files that
  may mix two versions), `Opened::interrupted_save` reports it (the app logs a warning), and the
  next save repairs the files without snapshotting the mixed state.
* Crashed sessions with unsaved work: the autosave + session-lock mechanism (earlier stages)
  offers recovery of the autosaved state.
* **Crash simulation:** `io::atomic::fault` makes a write "die" at a chosen stage (before the
  temporary file, half-written, complete but not renamed, renamed) and leaves its temporary file
  behind like a dead process. `io/tests/crash_safety.rs` crashes **every** write of a save
  (backup files, marker, documents, manifest) at every stage, then opens the folder and requires
  a project that is exactly the old or the new version, followed by a repairing save that clears
  the marker. It also checks that the target of a failed atomic write is untouched and that stale
  temporary files do not matter.

## Limits

* The fault hook simulates process death; it cannot simulate power loss with unsynced data or
  filesystem bugs.
* If the very first save of a project is interrupted and no backup exists, the files on disk are
  opened as they are (`restored_from: None`).
* Fuzzing is mutation-based and time-boxed; it is not coverage-guided. A libFuzzer harness
  (`cargo fuzz`) can be added later on top of the same entry points.
* Router and geometry were covered by earlier property tests; this stage did not extend them.
