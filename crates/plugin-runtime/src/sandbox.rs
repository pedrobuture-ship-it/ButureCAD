//! One plugin instance: a `wasmi` store with fuel and memory limits, the `buturecad` host import
//! module and the `JSON in → JSON out` call convention.
//!
//! Guest ABI (API 1), also described in `docs/plugin-sdk/README.md`:
//!
//! * export `memory`;
//! * export `alloc(len: i32) -> i32` — returns a guest address the host may write `len` bytes to;
//! * export `call(ptr: i32, len: i32) -> i64` — request JSON `{"method":…,"params":…}` at
//!   `ptr..ptr+len`; returns `(out_ptr << 32) | out_len` of the response JSON (0 = no response);
//! * imports (module `buturecad`): `log(ptr, len)` and `host_request(ptr, len) -> i64` (same
//!   packed return; the host allocates the reply with the guest's `alloc`).

use crate::manifest::Permission;
use crate::PluginError;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use wasmi::{
    Caller, Engine, Extern, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

/// Instructions a single call may execute.
pub const FUEL_PER_CALL: u64 = 200_000_000;
/// Linear memory cap.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Largest request/response JSON accepted in either direction.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const MAX_LOG_LINES: usize = 200;

/// Data the embedder puts at the plugin's disposal for **one call**. Which parts the plugin may
/// actually see is decided by the granted permissions.
#[derive(Debug, Clone, Default)]
pub struct CallContext {
    /// Returned to `project.info` (needs `project-read`).
    pub project_info: Option<Value>,
    /// Folders `fs.read` may read from (needs `filesystem`). Everything else is refused.
    pub fs_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CallOutput {
    pub value: Value,
    pub logs: Vec<String>,
    /// Host requests refused for lack of permission (audit trail).
    pub denied: Vec<String>,
    /// Host requests that were answered.
    pub host_requests: Vec<String>,
}

struct HostState {
    limits: StoreLimits,
    granted: BTreeSet<Permission>,
    ctx: CallContext,
    out: CallOutput,
}

pub struct Sandbox {
    fuel: u64,
    engine: Engine,
    module: Module,
    linker: Linker<HostState>,
}

fn guest_memory(caller: &Caller<'_, HostState>) -> Option<Memory> {
    caller.get_export("memory").and_then(Extern::into_memory)
}

fn read_guest(caller: &Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let (ptr, len) = (usize::try_from(ptr).ok()?, usize::try_from(len).ok()?);
    if len > MAX_MESSAGE {
        return None;
    }
    let mem = guest_memory(caller)?;
    let data = mem.data(caller);
    data.get(ptr..ptr.checked_add(len)?).map(<[u8]>::to_vec)
}

/// Copies `bytes` into guest memory (via the guest's `alloc`) and returns the packed pointer.
fn write_guest(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> i64 {
    let Some(alloc) = caller
        .get_export("alloc")
        .and_then(Extern::into_func)
        .and_then(|f| f.typed::<i32, i32>(&*caller).ok())
    else {
        return 0;
    };
    let Ok(len) = i32::try_from(bytes.len()) else {
        return 0;
    };
    let Ok(ptr) = alloc.call(&mut *caller, len) else {
        return 0;
    };
    let Some(mem) = guest_memory(caller) else {
        return 0;
    };
    let Ok(start) = usize::try_from(ptr) else {
        return 0;
    };
    if mem.write(&mut *caller, start, bytes).is_err() {
        return 0;
    }
    (i64::from(ptr) << 32) | i64::from(len)
}

/// Handles one host request. The permission check is the first thing every operation does.
fn handle_request(st: &mut HostState, req: &Value) -> Value {
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    let need = match op {
        "project.info" => Permission::ProjectRead,
        "project.propose" => Permission::ProjectWrite,
        "fs.read" => Permission::Filesystem,
        "net.get" => Permission::Network,
        _ => return json!({"error": format!("unknown host operation “{op}”")}),
    };
    if !st.granted.contains(&need) {
        st.out
            .denied
            .push(format!("{op}: needs the {} permission", need.name()));
        return json!({"error": format!("permission denied: {}", need.name())});
    }
    st.out.host_requests.push(op.to_owned());
    match op {
        "project.info" => match &st.ctx.project_info {
            Some(v) => json!({"ok": v}),
            None => json!({"error": "no project is open"}),
        },
        "project.propose" => json!({
            "error": "plugins cannot modify the project directly; the proposal flow (Agent API) is not implemented yet"
        }),
        "fs.read" => {
            let Some(p) = req.get("path").and_then(Value::as_str) else {
                return json!({"error": "fs.read needs a path"});
            };
            read_allowed(&st.ctx.fs_roots, p)
        }
        _ => {
            json!({"error": "network access is not available in this build (the permission exists, no client is wired)"})
        }
    }
}

fn read_allowed(roots: &[PathBuf], path: &str) -> Value {
    let Ok(real) = std::fs::canonicalize(path) else {
        return json!({"error": "file not found"});
    };
    let allowed = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|r| real.starts_with(r));
    if !allowed {
        return json!({"error": "path is outside the folders this plugin may read"});
    }
    match std::fs::metadata(&real) {
        Ok(m) if m.is_file() && m.len() <= MAX_MESSAGE as u64 => {
            match std::fs::read_to_string(&real) {
                Ok(text) => json!({"ok": {"text": text}}),
                Err(_) => json!({"error": "only UTF-8 text files can be read"}),
            }
        }
        _ => json!({"error": "not a readable file of at most 8 MiB"}),
    }
}

impl Sandbox {
    /// Compiles and validates the module and checks that it only imports what the host offers.
    pub fn new(wasm: &[u8]) -> Result<Self, PluginError> {
        let mut cfg = wasmi::Config::default();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg);
        let module = Module::new(&engine, wasm).map_err(|e| PluginError::Load(e.to_string()))?;
        let mut linker = <Linker<HostState>>::new(&engine);
        linker
            .func_wrap(
                "buturecad",
                "log",
                |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                    if let Some(b) = read_guest(&caller, ptr, len) {
                        let out = &mut caller.data_mut().out;
                        if out.logs.len() < MAX_LOG_LINES {
                            out.logs
                                .push(String::from_utf8_lossy(&b).chars().take(500).collect());
                        }
                    }
                },
            )
            .map_err(|e| PluginError::Load(e.to_string()))?;
        linker
            .func_wrap(
                "buturecad",
                "host_request",
                |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i64 {
                    let Some(bytes) = read_guest(&caller, ptr, len) else {
                        return 0;
                    };
                    let reply = match serde_json::from_slice::<Value>(&bytes) {
                        Ok(req) => handle_request(caller.data_mut(), &req),
                        Err(e) => json!({"error": format!("bad request JSON: {e}")}),
                    };
                    match serde_json::to_vec(&reply) {
                        Ok(b) if b.len() <= MAX_MESSAGE => write_guest(&mut caller, &b),
                        _ => 0,
                    }
                },
            )
            .map_err(|e| PluginError::Load(e.to_string()))?;
        // Instantiate once to reject unknown imports and missing exports up front.
        let s = Self {
            fuel: FUEL_PER_CALL,
            engine,
            module,
            linker,
        };
        let mut store = s.store(BTreeSet::new(), CallContext::default())?;
        let inst = s
            .linker
            .instantiate_and_start(&mut store, &s.module)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        inst.get_memory(&store, "memory")
            .ok_or_else(|| PluginError::Load("the module does not export `memory`".into()))?;
        inst.get_typed_func::<i32, i32>(&store, "alloc")
            .map_err(|e| PluginError::Load(format!("export `alloc(i32)->i32`: {e}")))?;
        inst.get_typed_func::<(i32, i32), i64>(&store, "call")
            .map_err(|e| PluginError::Load(format!("export `call(i32,i32)->i64`: {e}")))?;
        Ok(s)
    }

    /// Lowers the per-call instruction budget (tests, stricter hosts).
    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.fuel = fuel;
        self
    }

    fn store(
        &self,
        granted: BTreeSet<Permission>,
        ctx: CallContext,
    ) -> Result<Store<HostState>, PluginError> {
        let mut store = Store::new(
            &self.engine,
            HostState {
                limits: StoreLimitsBuilder::new()
                    .memory_size(MEMORY_LIMIT)
                    .instances(1)
                    .memories(1)
                    .tables(4)
                    .build(),
                granted,
                ctx,
                out: CallOutput::default(),
            },
        );
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(self.fuel)
            .map_err(|e| PluginError::Runtime(e.to_string()))?;
        Ok(store)
    }

    /// Calls `method` with `params`. A fresh instance is used for every call, so plugins keep no
    /// hidden state between calls and revoked permissions take effect immediately.
    pub fn call(
        &self,
        method: &str,
        params: &Value,
        granted: &BTreeSet<Permission>,
        ctx: CallContext,
    ) -> Result<CallOutput, PluginError> {
        let req = serde_json::to_vec(&json!({"method": method, "params": params}))
            .map_err(|e| PluginError::Runtime(e.to_string()))?;
        if req.len() > MAX_MESSAGE {
            return Err(PluginError::Runtime("request is too large".into()));
        }
        let mut store = self.store(granted.clone(), ctx)?;
        let rt = |e: wasmi::Error| {
            let s = e.to_string();
            if s.to_lowercase().contains("fuel") {
                PluginError::OutOfFuel
            } else {
                PluginError::Runtime(s)
            }
        };
        let inst = self
            .linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(rt)?;
        let alloc: TypedFunc<i32, i32> = inst.get_typed_func(&store, "alloc").map_err(rt)?;
        let call: TypedFunc<(i32, i32), i64> = inst.get_typed_func(&store, "call").map_err(rt)?;
        let mem = inst
            .get_memory(&store, "memory")
            .ok_or_else(|| PluginError::Runtime("no memory export".into()))?;
        let len = i32::try_from(req.len())
            .map_err(|_| PluginError::Runtime("request too large".into()))?;
        let ptr = alloc.call(&mut store, len).map_err(rt)?;
        let start = usize::try_from(ptr)
            .map_err(|_| PluginError::Runtime("alloc returned a bad pointer".into()))?;
        mem.write(&mut store, start, &req)
            .map_err(|e| PluginError::Runtime(format!("cannot write the request: {e}")))?;
        let packed = call.call(&mut store, (ptr, len)).map_err(rt)?;
        let mut out = std::mem::take(&mut store.data_mut().out);
        out.value = Value::Null;
        if packed != 0 {
            let (rp, rl) = (
                (packed >> 32) as u32 as usize,
                (packed & 0xFFFF_FFFF) as usize,
            );
            if rl > MAX_MESSAGE {
                return Err(PluginError::Runtime("response is larger than 8 MiB".into()));
            }
            let data = mem.data(&store);
            let bytes = data
                .get(
                    rp..rp
                        .checked_add(rl)
                        .ok_or_else(|| PluginError::Runtime("bad response range".into()))?,
                )
                .ok_or_else(|| {
                    PluginError::Runtime("the response points outside guest memory".into())
                })?;
            out.value = serde_json::from_slice(bytes)
                .map_err(|e| PluginError::Runtime(format!("the response is not JSON: {e}")))?;
        }
        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
pub(crate) mod tests {
    use super::*;

    /// A guest that answers every call with the reply to a fixed host request.
    pub(crate) fn host_probe(op_json: &str) -> Vec<u8> {
        let esc = op_json.replace('\\', "\\\\").replace('"', "\\\"");
        wat::parse_str(format!(
            r#"(module
  (import "buturecad" "host_request" (func $req (param i32 i32) (result i64)))
  (import "buturecad" "log" (func $log (param i32 i32)))
  (memory (export "memory") 2)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 16) "{esc}")
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n)))
    (local.get $p))
  (func (export "call") (param $p i32) (param $n i32) (result i64)
    (call $log (i32.const 16) (i32.const {len}))
    (call $req (i32.const 16) (i32.const {len}))))"#,
            len = op_json.len()
        ))
        .unwrap()
    }

    pub(crate) fn echo() -> Vec<u8> {
        // Returns the request it received.
        wat::parse_str(
            r#"(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n)))
    (local.get $p))
  (func (export "call") (param $p i32) (param $n i32) (result i64)
    (i64.or (i64.shl (i64.extend_i32_u (local.get $p)) (i64.const 32)) (i64.extend_i32_u (local.get $n)))))"#,
        )
        .unwrap()
    }

    fn none() -> BTreeSet<Permission> {
        BTreeSet::new()
    }

    #[test]
    fn json_round_trip_through_the_guest() {
        let s = Sandbox::new(&echo()).unwrap();
        let out = s
            .call("hello", &json!({"a": 1}), &none(), CallContext::default())
            .unwrap();
        assert_eq!(out.value, json!({"method": "hello", "params": {"a": 1}}));
    }

    #[test]
    fn host_requests_are_gated_by_permission_and_audited() {
        let s = Sandbox::new(&host_probe(r#"{"op":"project.info"}"#)).unwrap();
        let ctx = CallContext {
            project_info: Some(json!({"name": "P"})),
            ..CallContext::default()
        };
        let denied = s.call("x", &Value::Null, &none(), ctx.clone()).unwrap();
        assert_eq!(
            denied.value,
            json!({"error": "permission denied: project-read"})
        );
        assert_eq!(denied.denied.len(), 1);
        assert!(denied.host_requests.is_empty());
        assert_eq!(denied.logs.len(), 1);
        let ok = s
            .call("x", &Value::Null, &[Permission::ProjectRead].into(), ctx)
            .unwrap();
        assert_eq!(ok.value, json!({"ok": {"name": "P"}}));
        assert_eq!(ok.host_requests, ["project.info"]);
        // Network is gated too, and even when granted this build has no client.
        let n = Sandbox::new(&host_probe(r#"{"op":"net.get","url":"https://x"}"#)).unwrap();
        let r = n
            .call("x", &Value::Null, &none(), CallContext::default())
            .unwrap();
        assert_eq!(r.value["error"], "permission denied: network");
        let r = n
            .call(
                "x",
                &Value::Null,
                &[Permission::Network].into(),
                CallContext::default(),
            )
            .unwrap();
        assert!(r.value["error"]
            .as_str()
            .unwrap()
            .contains("not available in this build"));
        // project-write never modifies anything.
        let w = Sandbox::new(&host_probe(r#"{"op":"project.propose"}"#)).unwrap();
        let r = w
            .call(
                "x",
                &Value::Null,
                &[Permission::ProjectWrite].into(),
                CallContext::default(),
            )
            .unwrap();
        assert!(r.value["error"]
            .as_str()
            .unwrap()
            .contains("cannot modify the project"));
        let u = Sandbox::new(&host_probe(r#"{"op":"exec"}"#)).unwrap();
        assert!(u
            .call("x", &Value::Null, &none(), CallContext::default())
            .unwrap()
            .value["error"]
            .as_str()
            .unwrap()
            .contains("unknown host operation"));
    }

    #[test]
    fn filesystem_reads_stay_inside_the_granted_roots() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("in")).unwrap();
        std::fs::write(d.path().join("in/a.txt"), "hello").unwrap();
        std::fs::write(d.path().join("secret.txt"), "no").unwrap();
        let probe = |p: &std::path::Path| {
            let op = format!(r#"{{"op":"fs.read","path":"{}"}}"#, p.display());
            Sandbox::new(&host_probe(&op)).unwrap()
        };
        let ctx = CallContext {
            fs_roots: vec![d.path().join("in")],
            ..CallContext::default()
        };
        let g: BTreeSet<_> = [Permission::Filesystem].into();
        let ok = probe(&d.path().join("in/a.txt"))
            .call("x", &Value::Null, &g, ctx.clone())
            .unwrap();
        assert_eq!(ok.value, json!({"ok": {"text": "hello"}}));
        let esc = d.path().join("in/../secret.txt");
        let r = probe(&esc)
            .call("x", &Value::Null, &g, ctx.clone())
            .unwrap();
        assert!(r.value["error"].as_str().unwrap().contains("outside"));
        let r = probe(&d.path().join("in/a.txt"))
            .call("x", &Value::Null, &none(), ctx)
            .unwrap();
        assert_eq!(r.value["error"], "permission denied: filesystem");
    }

    fn module(call_body: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module (memory (export "memory") 1) (func (export "alloc") (param i32) (result i32) i32.const 64) (func (export "call") (param i32 i32) (result i64) {call_body}))"#
        ))
        .unwrap()
    }

    #[test]
    fn unloadable_modules_are_rejected() {
        assert!(matches!(
            Sandbox::new(b"not wasm"),
            Err(PluginError::Load(_))
        ));
        // Imports outside the host surface (e.g. WASI) cannot be satisfied.
        let wasi = wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))) (memory (export "memory") 1) (func (export "alloc") (param i32) (result i32) i32.const 0) (func (export "call") (param i32 i32) (result i64) i64.const 0))"#).unwrap();
        assert!(matches!(Sandbox::new(&wasi), Err(PluginError::Load(_))));
        let no_exports = wat::parse_str("(module)").unwrap();
        assert!(matches!(
            Sandbox::new(&no_exports),
            Err(PluginError::Load(_))
        ));
    }

    #[test]
    fn runaway_plugins_run_out_of_fuel() {
        let s = Sandbox::new(&module("(loop $l (br $l)) i64.const 0"))
            .unwrap()
            .with_fuel(2_000_000);
        assert!(matches!(
            s.call("x", &Value::Null, &none(), CallContext::default()),
            Err(PluginError::OutOfFuel)
        ));
    }

    #[test]
    fn traps_and_wild_pointers_are_errors_not_panics() {
        let s = Sandbox::new(&module("unreachable")).unwrap();
        assert!(matches!(
            s.call("x", &Value::Null, &none(), CallContext::default()),
            Err(PluginError::Runtime(_))
        ));
        let s = Sandbox::new(&module("i64.const 0x7FFFFFFF00000010")).unwrap();
        assert!(s
            .call("x", &Value::Null, &none(), CallContext::default())
            .is_err());
    }
}
