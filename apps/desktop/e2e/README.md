# End-to-end UI checks (dev tooling)

Drives the **built frontend** in headless Chromium against the **real Rust commands** (Tauri mock
runtime, real wgpu renderer) through a small HTTP bridge. It exists because the desktop webview
cannot be scripted directly on every machine. Not run in CI (needs Chromium and a GPU).

```bash
cd apps/desktop && npm run build
cargo run -p buturecad-desktop --example demo_project -- /tmp/buturecad-demo
node e2e/run.mjs            # screenshots land in e2e/out/
```

`shim.js` stands in for `window.__TAURI_INTERNALS__`: `invoke` is forwarded to the bridge, the
dialog plugin returns a canned folder, window APIs are no-ops.
