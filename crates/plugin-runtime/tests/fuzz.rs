//! Plugins are untrusted: damaged manifests and damaged WebAssembly must be rejected with errors,
//! never crash the host.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_plugin_runtime::{CallContext, Manifest, Sandbox};
use buturecad_testkit::{iterations, mutations};
use std::collections::BTreeSet;

const MANIFEST: &str = r#"{"format":"buturecad-plugin","api":1,"id":"acme.parts","name":"Acme","version":"1.0.0","categories":["ComponentProvider"],"permissions":["network","project-read"],"wasm":"plugin.wasm"}"#;

fn wasm() -> Vec<u8> {
    wat::parse_str(
        r#"(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32) (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n))) (local.get $p))
  (func (export "call") (param $p i32) (param $n i32) (result i64)
    (i64.or (i64.shl (i64.extend_i32_u (local.get $p)) (i64.const 32)) (i64.extend_i32_u (local.get $n)))))"#,
    )
    .unwrap()
}

#[test]
fn damaged_manifests_are_errors() {
    for bad in mutations(MANIFEST.as_bytes(), 5, iterations(2000)) {
        if let Ok(m) = Manifest::parse(&bad) {
            // Accepted manifests are valid by the same rules.
            m.validate().unwrap();
            assert!(!m.id.is_empty() && m.wasm.ends_with(".wasm") && !m.wasm.contains('/'));
        }
    }
}

#[test]
fn damaged_wasm_never_crashes_the_host() {
    let good = wasm();
    let (mut loaded, mut rejected) = (0, 0);
    for bad in mutations(&good, 6, iterations(600)) {
        match Sandbox::new(&bad) {
            Ok(s) => {
                loaded += 1;
                // A module that still loads can be called safely, with a tiny budget.
                let s = s.with_fuel(100_000);
                let _ = s.call(
                    "x",
                    &serde_json::json!({"a": 1}),
                    &BTreeSet::new(),
                    CallContext::default(),
                );
            }
            Err(_) => rejected += 1,
        }
    }
    assert!(
        rejected > loaded,
        "most damage is rejected ({rejected} rejected, {loaded} loaded)"
    );
}
