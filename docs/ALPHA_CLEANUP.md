# Release cleanup for the first public alpha (roadmap stage 49)

No features were added. Searched, fixed or documented:

| Search | Result |
|---|---|
| `TODO` / `FIXME` / `XXX` / `HACK` | none in Rust, TypeScript, scripts or config |
| `mock` / `placeholder` / `fake` / `stub` / `temporary` | only documented items: the built-in `Buture:` bootstrap symbols (`crates/schematic/src/builtin.rs`), HTML input placeholders, the Tauri *mock runtime* used by tests and the e2e bridge, temp-file wording |
| dead code | removed `_extent` (`#[allow(dead_code)]`) in the 2-D rasteriser and an unused import; no other `dead_code` allowances; `unused` warnings: none |
| debug code | removed the `BUTURECAD_TRACE` stderr tracing from the core library (it printed from a library); remaining `eprintln!` are the CLI and the app's startup error |
| unused dependencies | `serde_json` removed from `buturecad-scripting`; every other Cargo and npm dependency is used (npm: tooling-only packages are build/lint tools) |
| `panic!` / `unwrap()` / `expect()` | `unsafe_code = "forbid"` and clippy `unwrap_used`/`expect_used`/`panic` are denied (`-D warnings`) for the whole workspace; every `allow` is on a `#[cfg(test)]` module or test file |
| memory leaks | no `Box::leak`, `mem::forget`, `static mut` or `unsafe` anywhere. **Fixed:** the ngspice runner left its temporary folder behind when it failed to start or wait; it now removes it on every path (guard), with a test |
| race conditions / hangs | the only threads are the scoped tessellation workers (joined before returning). **Fixed:** (1) the ngspice runner piped stdout/stderr and read them only after the process ended, so a chatty simulation could fill the pipe and hang until the 60 s timeout — output now goes to a file; (2) a panic inside one desktop command poisoned the workspace lock and made every later command fail — the lock is now recovered, a warning is logged, and a test covers it |
| error messages | syntax errors named internal tokens (`found Star`) — now `found “*”` for every token; internal ids appeared as `Id(5)` in three core errors — now `net #5` |
| Windows / macOS | the CLI and every crate it links **type-check** for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`; fixed a Windows-only unused import. **Not verified:** running the tests there, and building the desktop app (Tauri/WebView, wgpu). CI gained an experimental job for the core on both. See `docs/robustness.md` |

Verification after the changes: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
734 Rust tests, 36 frontend tests, `npm run build`, all 12 end-to-end scenarios (stage 48).

## Known and accepted for the alpha

* `NotImplemented.tsx` and `MODE_PLANNED` remain as a defensive fallback for a mode without a panel;
  every current mode has one, so it is never shown.
* Interior mutability (`RefCell`) in the desktop workspace is used for caches borrowed in short,
  non-nested scopes; a nested borrow would panic, none exists today (a lock-poisoning recovery now
  keeps the app usable if a command ever panics).
* Windows and macOS are unverified (above).
