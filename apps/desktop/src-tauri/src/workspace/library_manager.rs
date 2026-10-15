//! Library Manager: a local search index over symbol, footprint, 3D model and component
//! libraries, with filters, favorites and recents. Everything is read from the library folders
//! (project and user); nothing leaves the machine.

use super::*;
use buturecad_libraries::{Component, Entry, Index, Kind, ModelEntry, Prefs, Query};
use buturecad_schematic::builtin;
use std::collections::BTreeSet;

const MAX_LISTED: usize = 300;

#[derive(Default)]
pub(crate) struct LibraryManager {
    index: Option<Index>,
    text: String,
    kinds: BTreeSet<Kind>,
    library: Option<String>,
    manufacturer: Option<String>,
    /// `all`, `favorites` or `recents`.
    scope: String,
    selected: Option<String>,
    prefs: Prefs,
    prefs_loaded: bool,
}

fn kind_of(s: &str) -> Option<Kind> {
    Some(match s {
        "symbol" => Kind::Symbol,
        "footprint" => Kind::Footprint,
        "model" => Kind::Model3d,
        "component" => Kind::Component,
        _ => return None,
    })
}

#[derive(serde::Deserialize)]
struct ComponentArgs {
    scope: String,
    library: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    footprint: String,
    #[serde(default)]
    model3d: String,
    #[serde(default)]
    manufacturer: String,
    #[serde(default)]
    mpn: String,
    #[serde(default)]
    datasheet: String,
    #[serde(default)]
    parameters: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ModelArgs {
    scope: String,
    library: String,
    name: String,
    path: String,
    #[serde(default)]
    description: String,
}

impl Workspace {
    fn prefs_path(&self) -> Option<std::path::PathBuf> {
        self.user_libs
            .as_ref()
            .map(|d| d.join(".library-prefs.json"))
    }

    fn lm_prefs(&mut self) {
        if !self.libm.prefs_loaded {
            self.libm.prefs = self
                .prefs_path()
                .map(|p| Prefs::load(&p))
                .unwrap_or_default();
            self.libm.prefs_loaded = true;
        }
    }

    fn lm_save_prefs(&mut self) {
        if let Some(p) = self.prefs_path() {
            if let Err(e) = self.libm.prefs.save(&p) {
                self.log(
                    MessageLevel::Warn,
                    format!("Cannot save library favorites: {e}"),
                );
            }
        }
    }

    pub(crate) fn lm_rebuild(&mut self) {
        let mut extra = Vec::new();
        for s in builtin::symbols() {
            let short = s.name.rsplit(':').next().unwrap_or(&s.name).to_owned();
            let mut e = Entry::new(Kind::Symbol, "builtin", "Buture", &short);
            e.description = s.meta.description.clone();
            e.keywords = s.meta.keywords.clone();
            e.footprint = s.meta.default_footprint.clone();
            extra.push(e);
        }
        // The built-in Std library: symbols, footprints and components.
        for s in buturecad_libraries::standard::symbols() {
            let short = s.name.rsplit(':').next().unwrap_or(&s.name).to_owned();
            let mut e = Entry::new(Kind::Symbol, "builtin", "Std", &short);
            e.description = s.meta.description.clone();
            e.keywords = s.meta.keywords.clone();
            extra.push(e);
        }
        for f in buturecad_libraries::standard::footprints() {
            let short = f.name.rsplit(':').next().unwrap_or(&f.name).to_owned();
            let mut e = Entry::new(Kind::Footprint, "builtin", "Std", &short);
            e.description = f.description.clone();
            e.keywords = f.tags.clone();
            extra.push(e);
        }
        for c in buturecad_libraries::standard::components() {
            let mut e = Entry::new(Kind::Component, "builtin", "Std", &c.name);
            e.description = c.description.clone();
            e.keywords = c.tags.join(" ");
            e.manufacturer = c.manufacturer.clone();
            e.mpn = c.mpn.clone();
            e.symbol = c.symbol.clone();
            e.footprint = c.footprint.clone();
            e.datasheet = c.datasheet.clone();
            extra.push(e);
        }
        let dirs = self.library_dirs();
        let refs: Vec<(&str, &buturecad_libraries::LibraryDir)> =
            dirs.iter().map(|(s, d)| (*s, d)).collect();
        let started = std::time::Instant::now();
        let ix = Index::from_dirs(&refs, extra);
        let (n, ms) = (ix.len(), started.elapsed().as_millis());
        self.libm.index = Some(ix);
        self.log(
            MessageLevel::Info,
            format!("Library index: {n} item(s) in {ms} ms"),
        );
    }

    fn lm_dir(&self, scope: &str) -> Result<buturecad_libraries::LibraryDir, String> {
        self.library_dirs()
            .into_iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, d)| d)
            .ok_or_else(|| format!("the {scope} library folder is not available (open a project, or start the app with a user library folder)"))
    }

    /// `refresh`, `query <text>`, `kind <kind>:<0|1>`, `library <name|''>`, `manufacturer <name|''>`,
    /// `scope all|favorites|recents`, `select <key>`, `favorite <key>`, `component_save <json>`,
    /// `component_delete <scope>|<library>|<name>`, `model_save <json>`, `library_create
    /// <scope>|<kind>|<name>`, `place <key>`.
    pub fn library_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("");
        self.lm_prefs();
        match name {
            "refresh" => self.lm_rebuild(),
            "query" => self.libm.text = a.to_owned(),
            "kind" => {
                let (k, on) = a.split_once(':').ok_or("kind takes <kind>:<0|1>")?;
                let k = kind_of(k).ok_or("unknown kind")?;
                if on == "1" {
                    self.libm.kinds.insert(k);
                } else {
                    self.libm.kinds.remove(&k);
                }
            }
            "library" => self.libm.library = (!a.is_empty()).then(|| a.to_owned()),
            "manufacturer" => self.libm.manufacturer = (!a.is_empty()).then(|| a.to_owned()),
            "scope" => {
                if !matches!(a, "all" | "favorites" | "recents") {
                    return Err("scope is all, favorites or recents".into());
                }
                self.libm.scope = a.to_owned();
            }
            "select" => {
                self.lm_ensure();
                let found = self.libm.index.as_ref().and_then(|i| i.find(a)).is_some();
                if !found {
                    return Err("that item is not in the index (refresh?)".into());
                }
                self.libm.selected = Some(a.to_owned());
                self.libm.prefs.touch_recent(a);
                self.lm_save_prefs();
            }
            "favorite" => {
                self.libm.prefs.toggle_favorite(a);
                self.lm_save_prefs();
            }
            "component_save" => {
                let c: ComponentArgs = serde_json::from_str(a).map_err(|e| e.to_string())?;
                if c.library == "Buture" {
                    return Err("“Buture” is the built-in library and is read-only".into());
                }
                let dir = self.lm_dir(&c.scope)?;
                let comp = Component {
                    name: c.name.trim().to_owned(),
                    description: c.description,
                    symbol: c.symbol,
                    footprint: c.footprint,
                    model3d: c.model3d,
                    manufacturer: c.manufacturer,
                    mpn: c.mpn,
                    datasheet: c.datasheet,
                    parameters: c.parameters,
                    tags: c.tags,
                };
                let key = format!("component:{}:{}", c.library, comp.name);
                dir.save_component(&c.library, comp)
                    .map_err(|e| e.to_string())?;
                self.lm_rebuild();
                self.libm.selected = Some(key);
            }
            "component_delete" => {
                let mut p = a.splitn(3, '|');
                let (scope, lib, n) = (
                    p.next().unwrap_or(""),
                    p.next().unwrap_or(""),
                    p.next().unwrap_or(""),
                );
                self.lm_dir(scope)?
                    .delete_component(lib, n)
                    .map_err(|e| e.to_string())?;
                self.libm.selected = None;
                self.lm_rebuild();
            }
            "model_save" => {
                let m: ModelArgs = serde_json::from_str(a).map_err(|e| e.to_string())?;
                let dir = self.lm_dir(&m.scope)?;
                dir.save_model(
                    &m.library,
                    ModelEntry {
                        name: m.name.trim().to_owned(),
                        path: m.path,
                        description: m.description,
                    },
                )
                .map_err(|e| e.to_string())?;
                self.lm_rebuild();
            }
            "library_create" => {
                let mut p = a.splitn(3, '|');
                let (scope, kind, n) = (
                    p.next().unwrap_or(""),
                    p.next().unwrap_or(""),
                    p.next().unwrap_or(""),
                );
                let dir = self.lm_dir(scope)?;
                match kind {
                    "component" => dir.create_component_library(n).map(|_| ()),
                    "model" => dir.save_model_library(
                        &buturecad_libraries::ModelLibrary::new(n).map_err(|e| e.to_string())?,
                    ),
                    _ => return Err("kind is component or model".into()),
                }
                .map_err(|e| e.to_string())?;
                self.lm_rebuild();
            }
            other => return Err(format!("unknown library action: {other}")),
        }
        Ok(())
    }

    fn lm_ensure(&mut self) {
        if self.libm.index.is_none() {
            self.lm_rebuild();
        }
    }

    /// The symbol id to arm the placement tool with for an indexed item (symbols and components).
    pub fn library_place_symbol(&mut self, key: &str) -> Result<String, String> {
        self.lm_ensure();
        let ix = self.libm.index.as_ref().ok_or("no index")?;
        let e = ix.find(key).ok_or("that item is not in the index")?;
        let id = match e.kind {
            Kind::Symbol => e.id.clone(),
            Kind::Component if !e.symbol.is_empty() => e.symbol.clone(),
            _ => return Err("only symbols and components with a symbol can be placed".into()),
        };
        if self.find_library_symbol(&id).is_none() {
            return Err(format!("symbol {id} was not found in any library"));
        }
        // A library component brings its footprint and manufacturer data, not only its symbol.
        self.editor.pending_part = match (
            &e.kind,
            self.libm.index.as_ref().and_then(|ix| ix.find(key)),
        ) {
            (Kind::Component, Some(c)) => Some((
                id.clone(),
                buturecad_schematic::edit::PartData {
                    footprint: c.footprint.clone(),
                    mpn: c.mpn.clone(),
                    manufacturer: c.manufacturer.clone(),
                    datasheet: c.datasheet.clone(),
                },
            )),
            _ => None,
        };
        self.libm.prefs.touch_recent(key);
        self.lm_save_prefs();
        Ok(id)
    }

    pub(crate) fn library_info(&mut self) -> LibraryManagerInfo {
        self.lm_prefs();
        self.lm_ensure();
        let persisted = self.user_libs.is_some();
        let Some(ix) = self.libm.index.as_ref() else {
            return LibraryManagerInfo::default();
        };
        let prefs = &self.libm.prefs;
        let only = match self.libm.scope.as_str() {
            "favorites" => Some(prefs.favorites.clone()),
            "recents" => Some(prefs.recents.iter().cloned().collect()),
            _ => None,
        };
        let q = Query {
            text: self.libm.text.clone(),
            kinds: self.libm.kinds.clone(),
            library: self.libm.library.clone(),
            manufacturer: self.libm.manufacturer.clone(),
            only_keys: only,
            limit: MAX_LISTED + 1,
        };
        let mut hits = ix.search(&q);
        let truncated = hits.len() > MAX_LISTED;
        hits.truncate(MAX_LISTED);
        if self.libm.scope == "recents" {
            hits.sort_by_key(|e| {
                prefs
                    .recents
                    .iter()
                    .position(|k| *k == e.key())
                    .unwrap_or(usize::MAX)
            });
        }
        let row = |e: &Entry| LibraryRow {
            key: e.key(),
            kind: e.kind.word().into(),
            scope: e.scope.clone(),
            library: e.library.clone(),
            name: e.name.clone(),
            id: e.id.clone(),
            description: e.description.clone(),
            manufacturer: e.manufacturer.clone(),
            mpn: e.mpn.clone(),
            favorite: prefs.favorites.contains(&e.key()),
        };
        let detail = self
            .libm
            .selected
            .as_ref()
            .and_then(|k| ix.find(k))
            .map(|e| {
                let mut parameters = Vec::new();
                if e.kind == Kind::Component {
                    if let Ok(dir) = self.lm_dir(&e.scope) {
                        if let Ok(lib) = dir.load_component_library(&e.library) {
                            if let Some(c) = lib.get(&e.name) {
                                parameters = c
                                    .parameters
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.clone()))
                                    .collect();
                            }
                        }
                    }
                }
                LibraryDetail {
                    row: row(e),
                    symbol: e.symbol.clone(),
                    footprint: if e.kind == Kind::Symbol {
                        String::new()
                    } else {
                        e.footprint.clone()
                    },
                    default_footprint: if e.kind == Kind::Symbol {
                        e.footprint.clone()
                    } else {
                        String::new()
                    },
                    model3d: e.model3d.clone(),
                    datasheet: e.datasheet.clone(),
                    keywords: e.keywords.clone(),
                    parameters: parameters
                        .into_iter()
                        .map(|(k, v)| LibraryParam { key: k, value: v })
                        .collect(),
                    unresolved: if e.kind == Kind::Component {
                        ix.unresolved(e)
                    } else {
                        Vec::new()
                    },
                    placeable: matches!(e.kind, Kind::Symbol)
                        || (e.kind == Kind::Component && !e.symbol.is_empty()),
                }
            });
        LibraryManagerInfo {
            total: ix.len() as u32,
            text: self.libm.text.clone(),
            kinds: self
                .libm
                .kinds
                .iter()
                .map(|k| k.word().to_owned())
                .collect(),
            library: self.libm.library.clone().unwrap_or_default(),
            manufacturer: self.libm.manufacturer.clone().unwrap_or_default(),
            scope: if self.libm.scope.is_empty() {
                "all".into()
            } else {
                self.libm.scope.clone()
            },
            libraries: ix.libraries(),
            manufacturers: ix.manufacturers(),
            results: hits.into_iter().map(row).collect(),
            truncated,
            problems: ix.problems.clone(),
            prefs_persisted: persisted,
            detail,
            writable_scopes: self
                .library_dirs()
                .into_iter()
                .map(|(s, _)| s.to_owned())
                .collect(),
        }
    }
}
