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
