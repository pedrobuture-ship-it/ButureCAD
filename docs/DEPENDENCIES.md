# Dependencies

All must be compatible with proprietary commercial distribution (MIT/Apache-2.0/BSD class; no copyleft linked in).

| Dependency | Use | License |
|---|---|---|
| tauri 2 / tauri-build | desktop shell | MIT OR Apache-2.0 |
| serde | serialization | MIT OR Apache-2.0 |
| React 19, Vite, TypeScript | UI toolchain | MIT / Apache-2.0 |
| vitest, eslint, prettier | frontend tests/lint | MIT |

Run `cargo deny`/license audit before release (not yet configured).

| serde_json | project file JSON | MIT OR Apache-2.0 |
| thiserror | error types | MIT OR Apache-2.0 |
| tempfile (dev) | tests | MIT OR Apache-2.0 |
| rstar (geometry) | R-tree spatial index | MIT OR Apache-2.0 |
| proptest (dev) | property-based tests | MIT OR Apache-2.0 |
| wgpu 30 (renderer) | GPU rendering (Vulkan/Metal/DX12) | MIT OR Apache-2.0 |
| pollster (renderer) | block on wgpu async setup | Apache-2.0 OR MIT |
| bytemuck (renderer) | vertex buffer casting | Zlib OR Apache-2.0 OR MIT |
| ts-rs (desktop) | generate TypeScript contracts from Rust types | MIT |
| tauri-plugin-dialog (desktop) | native folder pickers | MIT OR Apache-2.0 |
| lucide-react (desktop UI) | icons | ISC |
| @tauri-apps/plugin-dialog, @tauri-apps/api | JS side of the Tauri bridge | MIT OR Apache-2.0 |

| i_overlay (eda-core) | exact integer (i64) polygon booleans and offsets for copper zone fill | MIT OR Apache-2.0 |
| i_triangle (eda-core) | triangulation of filled zone regions with holes (drawing, later 3D) | MIT OR Apache-2.0 |

Decision record — polygon booleans (roadmap 25): zone fill needs difference/intersection of polygons with holes, offsetting and triangulation. Writing robust boolean operations in-house is exactly the "fragile geometry hack" CLAUDE.md forbids, so `i_overlay`/`i_triangle` (same author, integer engines, no unsafe-by-default, no network, pure Rust, actively maintained, permissive licence) are used. Fill results are re-checked by our own exact copper distance code in the tests (the fill must keep the clearance from every other-net object), so the dependency is verified, not trusted.

Decision record — STEP/MCAD (roadmap 28): no STEP/geometry-kernel dependency is linked. `buturecad-mcad` uses only serde/serde_json/thiserror. Candidates (OpenCascade LGPL-2.1+exception, truck Apache-2.0, ruststep Apache-2.0) are analysed in `docs/mcad.md`; the choice is deferred behind the `McadBackend` trait.

| wasmi 2 (plugin-runtime) | WebAssembly interpreter for the plugin sandbox (features `std`, `validate`, `portable-dispatch`; no WAT parser in production) | MIT OR Apache-2.0 |
| wat (plugin-runtime, desktop; dev only) | assemble WAT test fixtures | Apache-2.0 WITH LLVM-exception |

Decision record — plugin sandbox (roadmap 32): WASM was chosen over native dynamic libraries
(no isolation). Between `wasmtime` (JIT, much larger, bigger attack surface, heavy build) and
`wasmi` (small pure-Rust interpreter, fuel metering and memory limits built in, offline, no
unsafe in our code) we use `wasmi`; plugins here are glue (searching catalogues, importing files),
not compute kernels, so interpreter speed is acceptable. **WASI is deliberately not provided**:
the only door out of the sandbox is `buturecad.host_request`, gated by user-granted permissions.
`portable-dispatch` avoids stack overflows of the tail-call dispatcher in debug builds.

Decision record — simulation (roadmap 34): the prompt asks for a mature solver with a compatible
licence rather than a new one. Candidates: **ngspice** (maintained for decades; BSD-style
licence for the core, but some optional components and build configurations carry other terms —
needs legal review before any redistribution), **Xyce** (GPL-3: incompatible with a proprietary
distribution), **LTspice** (proprietary, not redistributable), embedding **libngspice** through FFI
(needs `unsafe`, a shared library per platform, and ties the licence to our binary). Chosen: run
the *user's installed* `ngspice` as a separate process (no linking, nothing bundled, offline), plus a small in-house linear engine so basic
analyses work with no external tool. Nothing was added to `Cargo.toml` for this beyond `thiserror`.
