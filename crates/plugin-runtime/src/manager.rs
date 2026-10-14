//! Plugin Manager: install, enable, disable, remove, reload, grant and revoke permissions.
//! Everything is local. Plugins live in `<root>/<id>/{plugin.json,<wasm>}`; the state (enabled
//! flag, granted permissions) is `<root>/state.json`, written atomically.

use crate::manifest::{Category, Manifest, Permission, MAX_WASM_BYTES};
use crate::sandbox::{CallContext, CallOutput, Sandbox};
use crate::PluginError;
use buturecad_io::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PluginState {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    granted: BTreeSet<Permission>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    plugins: BTreeMap<String, PluginState>,
}

/// What the UI shows for an installed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    pub manifest: Manifest,
    pub enabled: bool,
    pub granted: BTreeSet<Permission>,
    /// Loaded in the sandbox right now.
    pub loaded: bool,
    /// Why it is not usable (broken files, failed load).
    pub error: Option<String>,
}

pub struct PluginManager {
    root: PathBuf,
    state: StateFile,
    loaded: BTreeMap<String, Sandbox>,
    errors: BTreeMap<String, String>,
}

fn io(context: &str, e: impl std::fmt::Display) -> PluginError {
    PluginError::Io(format!("{context}: {e}"))
}

impl PluginManager {
    /// Opens the plugin folder (created on first install). Enabled plugins are loaded; the ones
    /// that fail to load stay listed with their error.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let state = std::fs::read(root.join("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<StateFile>(&b).ok())
            .unwrap_or_default();
        let mut m = Self {
            root,
            state,
            loaded: BTreeMap::new(),
            errors: BTreeMap::new(),
        };
        let ids: Vec<String> = m
            .state
            .plugins
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(i, _)| i.clone())
            .collect();
        for id in ids {
            if let Err(e) = m.load(&id) {
                m.errors.insert(id, e.to_string());
            }
        }
        m
    }

    fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    fn save_state(&self) -> Result<(), PluginError> {
        std::fs::create_dir_all(&self.root)
            .map_err(|e| io("cannot create the plugin folder", e))?;
        let mut b = serde_json::to_vec_pretty(&self.state).map_err(|e| io("state", e))?;
        b.push(b'\n');
        atomic_write(&self.root.join("state.json"), &b)
            .map_err(|e| io("cannot save plugin state", e))
    }

    fn read_manifest(dir: &Path) -> Result<Manifest, PluginError> {
        let bytes =
            std::fs::read(dir.join("plugin.json")).map_err(|e| io("cannot read plugin.json", e))?;
        Manifest::parse(&bytes)
    }

    fn read_wasm(dir: &Path, m: &Manifest) -> Result<Vec<u8>, PluginError> {
        let p = dir.join(&m.wasm);
        let len = std::fs::metadata(&p)
            .map_err(|e| io("cannot read the module", e))?
            .len();
        if len > MAX_WASM_BYTES {
            return Err(PluginError::Load(format!(
                "the module is {len} bytes, over the {MAX_WASM_BYTES} limit"
            )));
        }
        std::fs::read(&p).map_err(|e| io("cannot read the module", e))
    }

    fn load(&mut self, id: &str) -> Result<(), PluginError> {
        let dir = self.dir(id);
        let m = Self::read_manifest(&dir)?;
        if m.id != id {
            return Err(PluginError::Manifest(
                "the manifest id does not match its folder".into(),
            ));
        }
        let sb = Sandbox::new(&Self::read_wasm(&dir, &m)?)?;
        self.loaded.insert(id.to_owned(), sb);
        self.errors.remove(id);
        Ok(())
    }

    pub fn list(&self) -> Vec<PluginStatus> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let id = e.file_name().to_string_lossy().into_owned();
            match Self::read_manifest(&e.path()) {
                Ok(manifest) => {
                    let st = self.state.plugins.get(&id).cloned().unwrap_or_default();
                    out.push(PluginStatus {
                        enabled: st.enabled,
                        granted: st
                            .granted
                            .intersection(&manifest.permissions)
                            .copied()
                            .collect(),
                        loaded: self.loaded.contains_key(&id),
                        error: self.errors.get(&id).cloned(),
                        manifest,
                    });
                }
                Err(err) => {
                    // A broken plugin folder is listed, not hidden.
                    out.push(PluginStatus {
                        manifest: Manifest {
                            format: crate::manifest::FORMAT_TAG.into(),
                            api: crate::manifest::API_VERSION,
                            id: id.clone(),
                            name: id.clone(),
                            version: "?".into(),
                            description: String::new(),
                            categories: BTreeSet::new(),
                            permissions: BTreeSet::new(),
                            wasm: String::new(),
                        },
                        enabled: false,
                        granted: BTreeSet::new(),
                        loaded: false,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        out
    }

    /// Installs from a `plugin.json` file (the module must be next to it). The plugin starts
    /// **disabled with no permission granted**. Reinstalling the same id replaces the files and
    /// keeps only the grants the new manifest still asks for.
    pub fn install(&mut self, manifest_path: &Path) -> Result<String, PluginError> {
        let src = manifest_path
            .parent()
            .ok_or_else(|| io("manifest path", "has no folder"))?;
        let m = Self::read_manifest(src)?;
        let wasm = Self::read_wasm(src, &m)?;
        // Reject modules that do not load before touching the installed copy.
        Sandbox::new(&wasm)?;
        let dest = self.dir(&m.id);
        let tmp = self.root.join(format!(".installing-{}", m.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| io("cannot create the plugin folder", e))?;
        let write = || -> Result<(), PluginError> {
            atomic_write(
                &tmp.join("plugin.json"),
                &serde_json::to_vec_pretty(&m).map_err(|e| io("manifest", e))?,
            )
            .map_err(|e| io("cannot write plugin.json", e))?;
            atomic_write(&tmp.join(&m.wasm), &wasm).map_err(|e| io("cannot write the module", e))
        };
        if let Err(e) = write() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest).map_err(|e| io("cannot finish the install", e))?;
        self.loaded.remove(&m.id);
        self.errors.remove(&m.id);
        let st = self.state.plugins.entry(m.id.clone()).or_default();
        st.granted = st.granted.intersection(&m.permissions).copied().collect();
        let was_enabled = st.enabled;
        self.save_state()?;
        if was_enabled {
            if let Err(e) = self.load(&m.id) {
                self.errors.insert(m.id.clone(), e.to_string());
            }
        }
        Ok(m.id)
    }

    fn manifest_of(&self, id: &str) -> Result<Manifest, PluginError> {
        if !self.dir(id).join("plugin.json").exists() {
            return Err(PluginError::NotInstalled(id.to_owned()));
        }
        Self::read_manifest(&self.dir(id))
    }

    /// Enables a plugin: it must load in the sandbox first.
    pub fn enable(&mut self, id: &str) -> Result<(), PluginError> {
        self.manifest_of(id)?;
        self.load(id).inspect_err(|e| {
            self.errors.insert(id.to_owned(), e.to_string());
        })?;
        self.state.plugins.entry(id.to_owned()).or_default().enabled = true;
        self.save_state()
    }

    pub fn disable(&mut self, id: &str) -> Result<(), PluginError> {
        self.manifest_of(id)?;
        self.state.plugins.entry(id.to_owned()).or_default().enabled = false;
        self.loaded.remove(id);
        self.save_state()
    }

    /// Reloads the module from disk (an enabled plugin stays enabled if it loads).
    pub fn reload(&mut self, id: &str) -> Result<(), PluginError> {
        self.manifest_of(id)?;
        self.loaded.remove(id);
        let enabled = self.state.plugins.get(id).is_some_and(|s| s.enabled);
        if enabled {
            self.load(id).inspect_err(|e| {
                self.errors.insert(id.to_owned(), e.to_string());
            })?;
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), PluginError> {
        self.manifest_of(id).map(|_| ())?;
        std::fs::remove_dir_all(self.dir(id)).map_err(|e| io("cannot remove the plugin", e))?;
        self.loaded.remove(id);
        self.errors.remove(id);
        self.state.plugins.remove(id);
        self.save_state()
    }

    /// Grants a permission the manifest declares. Takes effect on the next call.
    pub fn grant(&mut self, id: &str, p: Permission) -> Result<(), PluginError> {
        let m = self.manifest_of(id)?;
        if !m.permissions.contains(&p) {
            return Err(PluginError::Manifest(format!(
                "{id} does not request the {} permission",
                p.name()
            )));
        }
        self.state
            .plugins
            .entry(id.to_owned())
            .or_default()
            .granted
            .insert(p);
        self.save_state()
    }

    /// Revokes a permission. Takes effect on the next call (plugins keep no state between calls).
    pub fn revoke(&mut self, id: &str, p: Permission) -> Result<(), PluginError> {
        self.manifest_of(id)?;
        self.state
            .plugins
            .entry(id.to_owned())
            .or_default()
            .granted
            .remove(&p);
        self.save_state()
    }

    pub fn granted(&self, id: &str) -> BTreeSet<Permission> {
        self.state
            .plugins
            .get(id)
            .map(|s| s.granted.clone())
            .unwrap_or_default()
    }

    /// Ids of enabled, loaded plugins that declare `category`.
    pub fn with_category(&self, category: Category) -> Vec<String> {
        self.list()
            .into_iter()
            .filter(|p| p.enabled && p.loaded && p.manifest.categories.contains(&category))
            .map(|p| p.manifest.id)
            .collect()
    }

    /// Calls a method of an enabled plugin. `category` must be one the plugin declares.
    pub fn call(
        &self,
        id: &str,
        category: Category,
        method: &str,
        params: &Value,
        ctx: CallContext,
    ) -> Result<CallOutput, PluginError> {
        let m = self.manifest_of(id)?;
        if !m.categories.contains(&category) {
            return Err(PluginError::WrongCategory {
                id: id.to_owned(),
                category,
            });
        }
        if !self.state.plugins.get(id).is_some_and(|s| s.enabled) {
            return Err(PluginError::Disabled(id.to_owned()));
        }
        let sb = self
            .loaded
            .get(id)
            .ok_or_else(|| PluginError::Disabled(id.to_owned()))?;
        let granted: BTreeSet<Permission> = self
            .granted(id)
            .intersection(&m.permissions)
            .copied()
            .collect();
        sb.call(method, params, &granted, ctx)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::sandbox::tests::{echo, host_probe};
    use serde_json::json;

    fn package(dir: &Path, id: &str, perms: &str, wasm: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let m = format!(
            r#"{{"format":"buturecad-plugin","api":1,"id":"{id}","name":"T","version":"1","categories":["ComponentProvider"],"permissions":[{perms}],"wasm":"plugin.wasm"}}"#
        );
        std::fs::write(dir.join("plugin.json"), m).unwrap();
        std::fs::write(dir.join("plugin.wasm"), wasm).unwrap();
        dir.join("plugin.json")
    }

    #[test]
    fn install_enable_call_disable_remove_and_persistence() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("plugins");
        let mut m = PluginManager::open(&root);
        let src = package(&d.path().join("src"), "t.echo", "", &echo());
        assert_eq!(m.install(&src).unwrap(), "t.echo");
        let st = m.list();
        assert!(!st[0].enabled && !st[0].loaded && st[0].granted.is_empty());
        let call = |m: &PluginManager| {
            m.call(
                "t.echo",
                Category::ComponentProvider,
                "ping",
                &json!(1),
                CallContext::default(),
            )
        };
        assert!(matches!(call(&m), Err(PluginError::Disabled(_))));
        m.enable("t.echo").unwrap();
        assert_eq!(call(&m).unwrap().value["method"], "ping");
        assert!(matches!(
            m.call(
                "t.echo",
                Category::Importer,
                "x",
                &json!(1),
                CallContext::default()
            ),
            Err(PluginError::WrongCategory { .. })
        ));
        assert_eq!(m.with_category(Category::ComponentProvider), ["t.echo"]);
        // State survives a restart.
        let m2 = PluginManager::open(&root);
        assert!(m2.list()[0].enabled && m2.list()[0].loaded);
        assert!(call(&m2).is_ok());
        m.disable("t.echo").unwrap();
        assert!(matches!(call(&m), Err(PluginError::Disabled(_))));
        m.remove("t.echo").unwrap();
        assert!(m.list().is_empty());
        assert!(matches!(
            m.enable("t.echo"),
            Err(PluginError::NotInstalled(_))
        ));
    }

    #[test]
    fn permissions_are_explicit_revocable_and_limited_to_the_manifest() {
        let d = tempfile::tempdir().unwrap();
        let mut m = PluginManager::open(d.path().join("plugins"));
        let src = package(
            &d.path().join("src"),
            "t.probe",
            r#""project-read""#,
            &host_probe(r#"{"op":"project.info"}"#),
        );
        m.install(&src).unwrap();
        m.enable("t.probe").unwrap();
        let ctx = || CallContext {
            project_info: Some(json!({"name": "P"})),
            ..CallContext::default()
        };
        let call = |m: &PluginManager| {
            m.call(
                "t.probe",
                Category::ComponentProvider,
                "x",
                &Value::Null,
                ctx(),
            )
            .unwrap()
        };
        assert_eq!(
            call(&m).value["error"],
            "permission denied: project-read",
            "nothing is granted by default"
        );
        assert!(
            m.grant("t.probe", Permission::Network).is_err(),
            "cannot grant what was not requested"
        );
        m.grant("t.probe", Permission::ProjectRead).unwrap();
        assert_eq!(call(&m).value["ok"]["name"], "P");
        // Persisted, and revocation applies at once (no reload needed).
        assert!(PluginManager::open(d.path().join("plugins"))
            .granted("t.probe")
            .contains(&Permission::ProjectRead));
        m.revoke("t.probe", Permission::ProjectRead).unwrap();
        assert_eq!(call(&m).value["error"], "permission denied: project-read");
        // Reinstalling with fewer requested permissions drops the stale grant.
        m.grant("t.probe", Permission::ProjectRead).unwrap();
        let src2 = package(
            &d.path().join("src2"),
            "t.probe",
            "",
            &host_probe(r#"{"op":"project.info"}"#),
        );
        m.install(&src2).unwrap();
        assert!(m.granted("t.probe").is_empty());
        assert_eq!(call(&m).value["error"], "permission denied: project-read");
    }

    #[test]
    fn broken_plugins_are_rejected_or_listed_with_their_error() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("plugins");
        let mut m = PluginManager::open(&root);
        let bad = package(&d.path().join("bad"), "t.bad", "", b"not wasm");
        assert!(m.install(&bad).is_err());
        assert!(
            m.list().is_empty(),
            "a rejected install leaves nothing behind"
        );
        let ok = package(&d.path().join("ok"), "t.ok", "", &echo());
        m.install(&ok).unwrap();
        m.enable("t.ok").unwrap();
        // The module is corrupted on disk: reload fails and says so; the old instance is dropped.
        std::fs::write(root.join("t.ok/plugin.wasm"), b"broken").unwrap();
        assert!(m.reload("t.ok").is_err());
        let st = m.list();
        assert!(st[0].enabled && !st[0].loaded && st[0].error.is_some());
        let m2 = PluginManager::open(&root);
        assert!(
            m2.list()[0].error.is_some(),
            "a broken enabled plugin does not stop the app"
        );
        // A folder without a valid manifest is listed too.
        std::fs::create_dir_all(root.join("junk")).unwrap();
        assert!(m
            .list()
            .iter()
            .any(|p| p.manifest.id == "junk" && p.error.is_some()));
        assert!(m.install(&d.path().join("missing/plugin.json")).is_err());
    }
}
