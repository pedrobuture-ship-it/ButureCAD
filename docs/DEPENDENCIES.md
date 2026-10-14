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
