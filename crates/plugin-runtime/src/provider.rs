//! The **ComponentProvider** API (roadmap 33).
//!
//! A component provider is any plugin that declares the `ComponentProvider` category and answers
//! these methods (request `params` → response `{"result": …}` or `{"error": "…"}`):
//!
//! | method | params | result |
//! |---|---|---|
//! | `search` | `{"query": str, "limit": n}` | `[{id, name, description, manufacturer, mpn}]` |
//! | `get_component` | `{"id": str}` | component record (`name`, `description`, `symbol`, `footprint`, `model3d`, `manufacturer`, `mpn`, `datasheet`, `parameters`, `tags`) |
//! | `get_symbol` | `{"id": str}` | a ButureCAD symbol (JSON, validated by the host before use) |
//! | `get_footprint` | `{"id": str}` | a ButureCAD footprint (JSON, validated by the host) |
//! | `get_model` | `{"id": str}` | `{name, format, path?, url?}` — a **reference**; nothing is downloaded |
//! | `get_datasheet` | `{"id": str}` | `{path?, url?}` — a reference, never opened automatically |
//! | `get_metadata` | `{"id": str}` | `{key: value}` strings |
//!
//! The core knows no supplier: it only speaks this interface. A provider that needs the internet
//! must declare and be granted `network`; the user can revoke it any time (see `sandbox`: this
//! build has no network client behind the permission yet, so such plugins get an explicit error).

use crate::manager::PluginManager;
use crate::manifest::Category;
use crate::sandbox::{CallContext, CallOutput};
use crate::PluginError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_RESULTS: usize = 200;
const MAX_TEXT: usize = 2000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub mpn: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderComponent {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub footprint: String,
    #[serde(default)]
    pub model3d: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub mpn: String,
    #[serde(default)]
    pub datasheet: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelReference {
    pub name: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasheetReference {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// What a component source offers. Implemented here by [`PluginProvider`]; the core and the UI
/// only use this trait.
pub trait ComponentProvider {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, PluginError>;
    fn get_component(&self, id: &str) -> Result<ProviderComponent, PluginError>;
    fn get_symbol(&self, id: &str) -> Result<Value, PluginError>;
    fn get_footprint(&self, id: &str) -> Result<Value, PluginError>;
    fn get_model(&self, id: &str) -> Result<ModelReference, PluginError>;
    fn get_datasheet(&self, id: &str) -> Result<DatasheetReference, PluginError>;
    fn get_metadata(&self, id: &str) -> Result<BTreeMap<String, String>, PluginError>;
}

/// A provider backed by an installed, enabled plugin.
pub struct PluginProvider<'a> {
    pub manager: &'a PluginManager,
    pub plugin: String,
    pub ctx: CallContext,
}

/// The result of one provider call plus what the host observed (for the audit panel).
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderReply<T> {
    pub value: T,
    pub denied: Vec<String>,
    pub logs: Vec<String>,
}

fn unwrap_envelope(out: CallOutput) -> Result<(Value, CallOutput), PluginError> {
    match out.value.clone() {
        Value::Object(mut m) => {
            if let Some(e) = m.remove("error") {
                let msg = e
                    .as_str()
                    .unwrap_or("the plugin reported an error")
                    .chars()
                    .take(MAX_TEXT)
                    .collect::<String>();
                let mut full = msg;
                if !out.denied.is_empty() {
                    full = format!("{full} (denied: {})", out.denied.join("; "));
                }
                return Err(PluginError::Runtime(full));
            }
            let v = m.remove("result").ok_or_else(|| {
                PluginError::Runtime("the response has neither `result` nor `error`".into())
            })?;
            Ok((v, out))
        }
        _ => Err(PluginError::Runtime(
            "the response is not a JSON object".into(),
        )),
    }
}

fn cap(s: &mut String) {
    if s.len() > MAX_TEXT {
        *s = s.chars().take(MAX_TEXT).collect();
    }
    s.retain(|c| !c.is_control());
}

impl PluginProvider<'_> {
    fn raw(&self, method: &str, params: Value) -> Result<(Value, CallOutput), PluginError> {
        let out = self.manager.call(
            &self.plugin,
            Category::ComponentProvider,
            method,
            &params,
            self.ctx.clone(),
        )?;
        unwrap_envelope(out)
    }

    fn typed<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, PluginError> {
        let (v, _) = self.raw(method, params)?;
        serde_json::from_value(v).map_err(|e| {
            PluginError::Runtime(format!("`{method}` returned an unexpected shape: {e}"))
        })
    }

    /// `search` plus the audit data (denied requests, logs).
    pub fn search_audited(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<ProviderReply<Vec<SearchHit>>, PluginError> {
        let (v, out) = self.raw(
            "search",
            json!({"query": query, "limit": limit.min(MAX_RESULTS)}),
        )?;
        let mut hits: Vec<SearchHit> = serde_json::from_value(v).map_err(|e| {
            PluginError::Runtime(format!("`search` returned an unexpected shape: {e}"))
        })?;
        hits.truncate(limit.min(MAX_RESULTS));
        for h in &mut hits {
            for f in [
                &mut h.id,
                &mut h.name,
                &mut h.description,
                &mut h.manufacturer,
                &mut h.mpn,
            ] {
                cap(f);
            }
        }
        hits.retain(|h| !h.id.is_empty() && !h.name.is_empty());
        Ok(ProviderReply {
            value: hits,
            denied: out.denied,
            logs: out.logs,
        })
    }
}

impl ComponentProvider for PluginProvider<'_> {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, PluginError> {
        self.search_audited(query, limit).map(|r| r.value)
    }

    fn get_component(&self, id: &str) -> Result<ProviderComponent, PluginError> {
        let mut c: ProviderComponent = self.typed("get_component", json!({"id": id}))?;
        for f in [
            &mut c.name,
            &mut c.description,
            &mut c.symbol,
            &mut c.footprint,
            &mut c.model3d,
            &mut c.manufacturer,
            &mut c.mpn,
            &mut c.datasheet,
        ] {
            cap(f);
        }
        Ok(c)
    }

    fn get_symbol(&self, id: &str) -> Result<Value, PluginError> {
        Ok(self.raw("get_symbol", json!({"id": id}))?.0)
    }

    fn get_footprint(&self, id: &str) -> Result<Value, PluginError> {
        Ok(self.raw("get_footprint", json!({"id": id}))?.0)
    }

    fn get_model(&self, id: &str) -> Result<ModelReference, PluginError> {
        self.typed("get_model", json!({"id": id}))
    }

    fn get_datasheet(&self, id: &str) -> Result<DatasheetReference, PluginError> {
        self.typed("get_datasheet", json!({"id": id}))
    }

    fn get_metadata(&self, id: &str) -> Result<BTreeMap<String, String>, PluginError> {
        self.typed("get_metadata", json!({"id": id}))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use crate::manifest::Permission;
    use std::path::Path;

    /// A provider plugin whose answers are fixed JSON texts. The request is
    /// `{"method":"<name>",…` so the byte at offset 11 / 15 / 16 tells the method apart.
    pub(crate) fn provider_wasm() -> Vec<u8> {
        // (discriminating byte offset, byte, response)
        let table: Vec<(usize, u8, &str)> = vec![
            (
                11,
                b's',
                r#"{"result":[{"id":"p1","name":"NE555","description":"timer","manufacturer":"TI","mpn":"NE555DR"},{"id":"p2","name":"LM358"},{"id":"","name":"dropped"}]}"#,
            ),
            (
                15,
                b'c',
                r#"{"result":{"name":"NE555","description":"timer","symbol":"Buture:R","footprint":"Package_SO:SOIC-8","manufacturer":"TI","mpn":"NE555DR","datasheet":"https://x.example/ds.pdf","parameters":{"Vcc":"5V"},"tags":["timer"]}}"#,
            ),
            (
                15,
                b's',
                r#"{"result":{"name":"NE555","reference_prefix":"U"}}"#,
            ),
            (15, b'f', r#"{"result":{"name":"SOIC-8"}}"#),
            (15, b'd', r#"{"result":{"url":"https://x.example/ds.pdf"}}"#),
            (
                16,
                b'o',
                r#"{"result":{"name":"SOIC-8","format":"step","url":"https://x.example/m.step"}}"#,
            ),
            (16, b'e', r#"{"result":{"family":"timer"}}"#),
        ];
        let mut data = String::new();
        let mut offs = Vec::new();
        let mut at = 8192usize;
        for (_, _, r) in &table {
            data.push_str(&format!(
                "(data (i32.const {at}) \"{}\")\n",
                r.replace('\\', "\\\\").replace('"', "\\\"")
            ));
            offs.push((at, r.len()));
            at += r.len() + 8;
        }
        // method `search` has no byte at 15 conflict; others share byte 11 = 'g'.
        let mut body = String::new();
        for (i, (off, ch, _)) in table.iter().enumerate() {
            let (a, l) = offs[i];
            let cond = if *off == 11 {
                format!("(i32.eq (i32.load8_u (i32.add (local.get $p) (i32.const 11))) (i32.const {ch}))")
            } else {
                format!(
                    "(i32.and (i32.eq (i32.load8_u (i32.add (local.get $p) (i32.const 11))) (i32.const 103)) (i32.eq (i32.load8_u (i32.add (local.get $p) (i32.const {off}))) (i32.const {ch})))"
                )
            };
            body.push_str(&format!(
                "(if {cond} (then (return (call $r (i32.const {a}) (i32.const {l})))))\n"
            ));
        }
        wat::parse_str(format!(
            r#"(module
  (memory (export "memory") 4)
  (global $heap (mut i32) (i32.const 1024))
  {data}
  (func $r (param $a i32) (param $l i32) (result i64)
    (i64.or (i64.shl (i64.extend_i32_u (local.get $a)) (i64.const 32)) (i64.extend_i32_u (local.get $l))))
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n)))
    (local.get $p))
  (func (export "call") (param $p i32) (param $n i32) (result i64)
    {body}
    (i64.const 0)))"#
        ))
        .unwrap()
    }

    pub(crate) fn install_provider(
        m: &mut PluginManager,
        work: &Path,
        id: &str,
        perms: &str,
        wasm: &[u8],
    ) {
        std::fs::create_dir_all(work).unwrap();
        std::fs::write(
            work.join("plugin.json"),
            format!(
                r#"{{"format":"buturecad-plugin","api":1,"id":"{id}","name":"Fixture","version":"1","categories":["ComponentProvider"],"permissions":[{perms}],"wasm":"plugin.wasm"}}"#
            ),
        )
        .unwrap();
        std::fs::write(work.join("plugin.wasm"), wasm).unwrap();
        m.install(&work.join("plugin.json")).unwrap();
        m.enable(id).unwrap();
    }

    #[test]
    fn the_whole_provider_interface_works_through_the_sandbox() {
        let d = tempfile::tempdir().unwrap();
        let mut m = PluginManager::open(d.path().join("plugins"));
        install_provider(
            &mut m,
            &d.path().join("w"),
            "fixture.parts",
            "",
            &provider_wasm(),
        );
        let p = PluginProvider {
            manager: &m,
            plugin: "fixture.parts".into(),
            ctx: CallContext::default(),
        };
        let hits = p.search("555", 10).unwrap();
        assert_eq!(hits.len(), 2, "hits without an id are dropped");
        assert_eq!(
            (hits[0].id.as_str(), hits[0].mpn.as_str()),
            ("p1", "NE555DR")
        );
        assert_eq!(p.search("555", 1).unwrap().len(), 1, "limit is honoured");
        let c = p.get_component("p1").unwrap();
        assert_eq!(
            (c.footprint.as_str(), c.parameters["Vcc"].as_str()),
            ("Package_SO:SOIC-8", "5V")
        );
        assert_eq!(p.get_symbol("p1").unwrap()["reference_prefix"], "U");
        assert_eq!(p.get_footprint("p1").unwrap()["name"], "SOIC-8");
        assert_eq!(
            p.get_datasheet("p1").unwrap().url.as_deref(),
            Some("https://x.example/ds.pdf")
        );
        let model = p.get_model("p1").unwrap();
        assert_eq!((model.format.as_str(), model.path), ("step", None));
        assert_eq!(p.get_metadata("p1").unwrap()["family"], "timer");
    }

    #[test]
    fn errors_and_wrong_shapes_are_reported() {
        let d = tempfile::tempdir().unwrap();
        let mut m = PluginManager::open(d.path().join("plugins"));
        // A plugin that always answers with an error.
        let wasm = wat::parse_str(
            r#"(module (memory (export "memory") 1)
  (data (i32.const 2048) "{\"error\":\"no such part\"}")
  (func (export "alloc") (param i32) (result i32) i32.const 64)
  (func (export "call") (param i32 i32) (result i64) (i64.or (i64.shl (i64.const 2048) (i64.const 32)) (i64.const 24))))"#,
        )
        .unwrap();
        install_provider(&mut m, &d.path().join("w"), "fixture.err", "", &wasm);
        let p = PluginProvider {
            manager: &m,
            plugin: "fixture.err".into(),
            ctx: CallContext::default(),
        };
        let e = p.get_component("x").unwrap_err().to_string();
        assert!(e.contains("no such part"), "{e}");
        assert!(p.search("x", 5).is_err());
        let missing = PluginProvider {
            manager: &m,
            plugin: "nope".into(),
            ctx: CallContext::default(),
        };
        assert!(matches!(
            missing.search("x", 5),
            Err(PluginError::NotInstalled(_))
        ));
        let _ = Permission::Network;
    }

    #[test]
    fn a_provider_that_wants_the_network_is_denied_until_granted_and_can_be_revoked() {
        let d = tempfile::tempdir().unwrap();
        let mut m = PluginManager::open(d.path().join("plugins"));
        // Searches by asking the host for the network; returns whatever the host says.
        let wasm = crate::sandbox::tests::host_probe(
            r#"{"op":"net.get","url":"https://parts.example/q"}"#,
        );
        install_provider(
            &mut m,
            &d.path().join("w"),
            "fixture.net",
            r#""network""#,
            &wasm,
        );
        let p = || PluginProvider {
            manager: &m,
            plugin: "fixture.net".into(),
            ctx: CallContext::default(),
        };
        // The probe answers with {"error":"permission denied: network"} which the provider layer
        // turns into an error that carries the audit trail.
        let e = p().search("x", 5).unwrap_err().to_string();
        assert!(e.contains("permission denied: network"), "{e}");
        let mut m2 = PluginManager::open(d.path().join("plugins"));
        m2.grant("fixture.net", Permission::Network).unwrap();
        let p2 = PluginProvider {
            manager: &m2,
            plugin: "fixture.net".into(),
            ctx: CallContext::default(),
        };
        assert!(p2
            .search("x", 5)
            .unwrap_err()
            .to_string()
            .contains("not available in this build"));
        m2.revoke("fixture.net", Permission::Network).unwrap();
        let p3 = PluginProvider {
            manager: &m2,
            plugin: "fixture.net".into(),
            ctx: CallContext::default(),
        };
        assert!(p3
            .search("x", 5)
            .unwrap_err()
            .to_string()
            .contains("permission denied: network"));
    }
}
