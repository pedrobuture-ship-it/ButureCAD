//! Plugin Manager and the Component Provider browser.
//!
//! Plugins run in the WebAssembly sandbox of `buturecad-plugin-runtime` with only the permissions
//! the user granted here. The core knows no supplier: provider results are imported through the
//! normal library code paths (validated, written atomically), never straight into the project.

use super::*;
use buturecad_libraries::Component;
use buturecad_plugin_runtime::provider::{ComponentProvider, PluginProvider};
use buturecad_plugin_runtime::{CallContext, Category, Permission, PluginManager};
use buturecad_schematic::Symbol;

#[derive(Default)]
pub(crate) struct Plugins {
    manager: Option<PluginManager>,
    provider: String,
    query: String,
    hits: Vec<PluginHit>,
    error: String,
    audit: Vec<String>,
    last_import: String,
}

impl Workspace {
    /// Sets the per-user plugin folder.
    pub fn with_plugin_dir(mut self, dir: PathBuf) -> Self {
        self.plugin_dir = Some(dir);
        self
    }

    fn pl_manager(&mut self) -> Result<&mut PluginManager, String> {
        if self.plugins.manager.is_none() {
            let dir = self
                .plugin_dir
                .clone()
                .ok_or("the plugin folder is not available in this session")?;
            self.plugins.manager = Some(PluginManager::open(dir));
        }
        self.plugins
            .manager
            .as_mut()
            .ok_or_else(|| "no plugin manager".into())
    }

    fn pl_ctx(&self) -> CallContext {
        let open = self.open.as_ref();
        CallContext {
            project_info: open.map(|o| serde_json::json!({"name": o.engine.project().name()})),
            fs_roots: open.map(|o| vec![o.root.clone()]).unwrap_or_default(),
        }
    }

    fn pl_perm(s: &str) -> Result<Permission, String> {
        Permission::parse(s).ok_or_else(|| format!("unknown permission “{s}”"))
    }

    /// `install <plugin.json>`, `enable|disable|remove|reload <id>`, `grant|revoke <id>|<perm>`,
    /// `provider <id>`, `search <query>`, `import <hit id>|<library>`.
    pub fn plugin_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("").trim().to_owned();
        match name {
            "install" => {
                let id = self
                    .pl_manager()?
                    .install(std::path::Path::new(&a))
                    .map_err(|e| e.to_string())?;
                self.log(
                    MessageLevel::Info,
                    format!("Installed plugin {id} (disabled, no permission granted)"),
                );
            }
            "enable" => self.pl_manager()?.enable(&a).map_err(|e| e.to_string())?,
            "disable" => {
                self.pl_manager()?.disable(&a).map_err(|e| e.to_string())?;
                if self.plugins.provider == a {
                    self.plugins.provider.clear();
                    self.plugins.hits.clear();
                }
            }
            "remove" => {
                self.pl_manager()?.remove(&a).map_err(|e| e.to_string())?;
                if self.plugins.provider == a {
                    self.plugins.provider.clear();
                    self.plugins.hits.clear();
                }
            }
            "reload" => self.pl_manager()?.reload(&a).map_err(|e| e.to_string())?,
            "grant" | "revoke" => {
                let (id, perm) = a.split_once('|').ok_or("expected <id>|<permission>")?;
                let p = Self::pl_perm(perm)?;
                let m = self.pl_manager()?;
                if name == "grant" {
                    m.grant(id, p).map_err(|e| e.to_string())?;
                } else {
                    m.revoke(id, p).map_err(|e| e.to_string())?;
                }
                self.log(
                    MessageLevel::Info,
                    format!(
                        "Plugin {id}: {} {}",
                        if name == "grant" {
                            "granted"
                        } else {
                            "revoked"
                        },
                        p.name()
                    ),
                );
            }
            "provider" => {
                self.plugins.provider = a;
                self.plugins.hits.clear();
                self.plugins.error.clear();
                self.plugins.audit.clear();
            }
            "search" => self.pl_search(&a),
            "import" => self.pl_import(&a)?,
            other => return Err(format!("unknown plugin action: {other}")),
        }
        Ok(())
    }

    fn pl_search(&mut self, query: &str) {
        self.plugins.query = query.to_owned();
        self.plugins.hits.clear();
        self.plugins.error.clear();
        self.plugins.audit.clear();
        let id = self.plugins.provider.clone();
        if id.is_empty() {
            self.plugins.error = "choose a provider first".into();
            return;
        }
        let ctx = self.pl_ctx();
        let Ok(m) = self.pl_manager() else {
            self.plugins.error = "the plugin folder is not available".into();
            return;
        };
        let p = PluginProvider {
            manager: &*m,
            plugin: id,
            ctx,
        };
        match p.search_audited(query, 100) {
            Ok(r) => {
                self.plugins.audit = r.denied;
                self.plugins.hits = r
                    .value
                    .into_iter()
                    .map(|h| PluginHit {
                        id: h.id,
                        name: h.name,
                        description: h.description,
                        manufacturer: h.manufacturer,
                        mpn: h.mpn,
                    })
                    .collect();
            }
            Err(e) => self.plugins.error = e.to_string(),
        }
    }

    /// Imports a provider component into the user component library `library`: the component
    /// record, and — when the provider supplies them and they validate — its symbol and footprint.
    fn pl_import(&mut self, arg: &str) -> Result<(), String> {
        let (hit, library) = arg.split_once('|').ok_or("expected <hit id>|<library>")?;
        if library == "Buture" {
            return Err("“Buture” is the built-in library and is read-only".into());
        }
        let dir = self
            .library_dirs()
            .into_iter()
            .find(|(s, _)| *s == "user")
            .map(|(_, d)| d)
            .ok_or("the user library folder is not available")?;
        let id = self.plugins.provider.clone();
        let ctx = self.pl_ctx();
        let m = self.pl_manager()?;
        let p = PluginProvider {
            manager: &*m,
            plugin: id.clone(),
            ctx,
        };
        let pc = p.get_component(hit).map_err(|e| e.to_string())?;
        let mut notes = Vec::new();
        let mut comp = Component {
            name: pc.name,
            description: pc.description,
            symbol: pc.symbol,
            footprint: pc.footprint,
            model3d: pc.model3d,
            manufacturer: pc.manufacturer,
            mpn: pc.mpn,
            datasheet: pc.datasheet,
            parameters: pc.parameters,
            tags: pc.tags,
        };
        // Optional extras: errors are noted, never fatal, and nothing unvalidated is stored.
        match p.get_symbol(hit).map(serde_json::from_value::<Symbol>) {
            Ok(Ok(sym)) => match dir.save_symbol(library, sym) {
                Ok(sid) => {
                    comp.symbol = sid;
                    notes.push("symbol imported".to_owned());
                }
                Err(e) => notes.push(format!("symbol not imported: {e}")),
            },
            Ok(Err(e)) => notes.push(format!("symbol not imported: {e}")),
            Err(e) => notes.push(format!("no symbol: {e}")),
        }
        match p
            .get_footprint(hit)
            .map(serde_json::from_value::<buturecad_pcb::Footprint>)
        {
            Ok(Ok(fp)) => match dir.save_footprint(library, fp) {
                Ok(fid) => {
                    comp.footprint = fid;
                    notes.push("footprint imported".to_owned());
                }
                Err(e) => notes.push(format!("footprint not imported: {e}")),
            },
            Ok(Err(e)) => notes.push(format!("footprint not imported: {e}")),
            Err(e) => notes.push(format!("no footprint: {e}")),
        }
        let name = comp.name.clone();
        dir.save_component(library, comp)
            .map_err(|e| e.to_string())?;
        self.plugins.last_import =
            format!("Imported “{name}” into {library} ({})", notes.join("; "));
        let msg = self.plugins.last_import.clone();
        self.log(MessageLevel::Info, msg);
        self.lm_rebuild();
        Ok(())
    }

    pub(crate) fn plugins_info(&mut self) -> PluginManagerInfo {
        let available = self.plugin_dir.is_some();
        let list = self.pl_manager().map(|m| m.list()).unwrap_or_default();
        let providers: Vec<String> = list
            .iter()
            .filter(|p| {
                p.enabled
                    && p.loaded
                    && p.manifest.categories.contains(&Category::ComponentProvider)
            })
            .map(|p| p.manifest.id.clone())
            .collect();
        PluginManagerInfo {
            available,
            plugins: list
                .into_iter()
                .map(|p| PluginRow {
                    id: p.manifest.id,
                    name: p.manifest.name,
                    version: p.manifest.version,
                    description: p.manifest.description,
                    categories: p
                        .manifest
                        .categories
                        .iter()
                        .map(|c| format!("{c:?}"))
                        .collect(),
                    requested: p
                        .manifest
                        .permissions
                        .iter()
                        .map(|x| x.name().to_owned())
                        .collect(),
                    granted: p.granted.iter().map(|x| x.name().to_owned()).collect(),
                    enabled: p.enabled,
                    loaded: p.loaded,
                    error: p.error,
                })
                .collect(),
            providers,
            provider: self.plugins.provider.clone(),
            query: self.plugins.query.clone(),
            hits: self.plugins.hits.clone(),
            error: self.plugins.error.clone(),
            audit: self.plugins.audit.clone(),
            last_import: self.plugins.last_import.clone(),
        }
    }
}
