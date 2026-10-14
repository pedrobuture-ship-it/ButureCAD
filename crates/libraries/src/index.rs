//! The local library index: every symbol, footprint, 3D model and component of the library
//! folders in one in-memory structure with an inverted token index, so searching is a prefix scan
//! over a sorted token map rather than a walk over files. Built from files on demand; nothing is
//! stored on disk and nothing needs a network.

use crate::store::LibraryDir;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Symbol,
    Footprint,
    Model3d,
    Component,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Symbol => "symbol",
            Kind::Footprint => "footprint",
            Kind::Model3d => "model",
            Kind::Component => "component",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// `builtin`, `project` or `user`.
    pub scope: String,
    pub library: String,
    pub name: String,
    /// `Library:Name`.
    pub id: String,
    pub description: String,
    pub manufacturer: String,
    pub mpn: String,
    pub keywords: String,
    /// Component references (empty for the other kinds).
    pub symbol: String,
    pub footprint: String,
    pub model3d: String,
    pub datasheet: String,
}

impl Entry {
    pub fn new(kind: Kind, scope: &str, library: &str, name: &str) -> Self {
        Self {
            kind,
            scope: scope.to_owned(),
            library: library.to_owned(),
            name: name.to_owned(),
            id: format!("{library}:{name}"),
            description: String::new(),
            manufacturer: String::new(),
            mpn: String::new(),
            keywords: String::new(),
            symbol: String::new(),
            footprint: String::new(),
            model3d: String::new(),
            datasheet: String::new(),
        }
    }

    /// Stable key used by favorites and recents.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.word(), self.id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub text: String,
    pub kinds: BTreeSet<Kind>,
    pub library: Option<String>,
    pub manufacturer: Option<String>,
    /// Only these keys (favorites or recents); `None` = no restriction.
    pub only_keys: Option<BTreeSet<String>>,
    pub limit: usize,
}

fn tokens(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
}

#[derive(Debug, Default)]
pub struct Index {
    entries: Vec<Entry>,
    /// token → entry positions (sorted, unique).
    postings: BTreeMap<String, Vec<u32>>,
    /// Name tokens per entry, for ranking.
    name_tokens: Vec<Vec<String>>,
    /// Problems met while loading (unreadable libraries), never silently dropped.
    pub problems: Vec<String>,
}

impl Index {
    pub fn build(mut entries: Vec<Entry>, problems: Vec<String>) -> Self {
        entries.sort_by(|a, b| (a.kind, &a.id, &a.scope).cmp(&(b.kind, &b.id, &b.scope)));
        entries.dedup_by(|a, b| a.kind == b.kind && a.id == b.id && a.scope == b.scope);
        let mut postings: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        let mut name_tokens = Vec::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            let mut set: BTreeSet<String> = BTreeSet::new();
            for f in [
                e.name.as_str(),
                &e.library,
                &e.description,
                &e.manufacturer,
                &e.mpn,
                &e.keywords,
                e.kind.word(),
                &e.scope,
            ] {
                set.extend(tokens(f));
            }
            for t in set {
                postings.entry(t).or_default().push(i as u32);
            }
            name_tokens.push(tokens(&e.name).collect());
        }
        Self {
            entries,
            postings,
            name_tokens,
            problems,
        }
    }

    /// Indexes every library file of the given folders plus extra (built-in) entries.
    pub fn from_dirs(dirs: &[(&str, &LibraryDir)], extra: Vec<Entry>) -> Self {
        let mut entries = extra;
        let mut problems = Vec::new();
        for (scope, dir) in dirs {
            for s in dir.list_symbol_libraries() {
                match (&s.error, dir.load_symbol_library(&s.name)) {
                    (None, Ok(lib)) => {
                        for sym in lib.iter() {
                            let short = sym.name.rsplit(':').next().unwrap_or(&sym.name);
                            let mut e = Entry::new(Kind::Symbol, scope, &lib.name, short);
                            e.description = sym.meta.description.clone();
                            e.keywords = sym.meta.keywords.clone();
                            e.manufacturer = sym.meta.manufacturer.clone();
                            e.mpn = sym.meta.mpn.clone();
                            e.footprint = sym.meta.default_footprint.clone();
                            e.datasheet = sym.meta.datasheet.clone();
                            entries.push(e);
                        }
                    }
                    (Some(err), _) => problems.push(format!("symbol library {}: {err}", s.name)),
                    (None, Err(e)) => problems.push(format!("symbol library {}: {e}", s.name)),
                }
            }
            for s in dir.list_footprint_libraries() {
                match dir.load_footprint_library(&s.name) {
                    Ok(lib) => {
                        for n in lib.names() {
                            if let Some(f) = lib.get(n) {
                                let mut e = Entry::new(Kind::Footprint, scope, &lib.name, n);
                                e.description = f.description.clone();
                                e.keywords = f.tags.clone();
                                if let Some(m) = f.models.first() {
                                    e.model3d = m.path.clone();
                                }
                                entries.push(e);
                            }
                        }
                    }
                    Err(e) => problems.push(format!("footprint library {}: {e}", s.name)),
                }
            }
            for s in dir.list_model_libraries() {
                match dir.load_model_library(&s.name) {
                    Ok(lib) => {
                        for m in lib.iter() {
                            let mut e = Entry::new(Kind::Model3d, scope, &lib.name, &m.name);
                            e.description = m.description.clone();
                            e.keywords = m.format();
                            e.model3d = m.path.clone();
                            entries.push(e);
                        }
                    }
                    Err(e) => problems.push(format!("3D model library {}: {e}", s.name)),
                }
            }
            for s in dir.list_component_libraries() {
                match dir.load_component_library(&s.name) {
                    Ok(lib) => {
                        for c in lib.iter() {
                            let mut e = Entry::new(Kind::Component, scope, &lib.name, &c.name);
                            e.description = c.description.clone();
                            e.manufacturer = c.manufacturer.clone();
                            e.mpn = c.mpn.clone();
                            e.keywords = c.tags.join(" ");
                            e.symbol = c.symbol.clone();
                            e.footprint = c.footprint.clone();
                            e.model3d = c.model3d.clone();
                            e.datasheet = c.datasheet.clone();
                            for (k, v) in &c.parameters {
                                e.keywords.push(' ');
                                e.keywords.push_str(k);
                                e.keywords.push(' ');
                                e.keywords.push_str(v);
                            }
                            entries.push(e);
                        }
                    }
                    Err(e) => problems.push(format!("component library {}: {e}", s.name)),
                }
            }
        }
        Self::build(entries, problems)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn find(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key() == key)
    }

    /// Distinct manufacturers and libraries, for the filter drop-downs.
    pub fn manufacturers(&self) -> Vec<String> {
        let s: BTreeSet<&str> = self
            .entries
            .iter()
            .map(|e| e.manufacturer.as_str())
            .filter(|m| !m.is_empty())
            .collect();
        s.into_iter().map(str::to_owned).collect()
    }
    pub fn libraries(&self) -> Vec<String> {
        let s: BTreeSet<&str> = self.entries.iter().map(|e| e.library.as_str()).collect();
        s.into_iter().map(str::to_owned).collect()
    }

    /// Every query token must be a prefix of some token of the entry. Results are ranked: name
    /// tokens that start with the query tokens first, exact name before prefix, then by name.
    pub fn search(&self, q: &Query) -> Vec<&Entry> {
        let qt: Vec<String> = tokens(&q.text).collect();
        let mut hits: Option<Vec<u32>> = None;
        for t in &qt {
            let mut set: BTreeSet<u32> = BTreeSet::new();
            for (k, v) in self.postings.range(t.clone()..) {
                if !k.starts_with(t.as_str()) {
                    break;
                }
                set.extend(v.iter().copied());
            }
            hits = Some(match hits {
                None => set.into_iter().collect(),
                Some(prev) => prev.into_iter().filter(|i| set.contains(i)).collect(),
            });
            if hits.as_ref().is_some_and(Vec::is_empty) {
                return Vec::new();
            }
        }
        let ids: Vec<u32> = hits.unwrap_or_else(|| (0..self.entries.len() as u32).collect());
        let wanted = q.text.trim().to_lowercase();
        let mut out: Vec<(u32, &Entry)> = ids
            .into_iter()
            .map(|i| (i, &self.entries[i as usize]))
            .filter(|(_, e)| q.kinds.is_empty() || q.kinds.contains(&e.kind))
            .filter(|(_, e)| q.library.as_ref().is_none_or(|l| &e.library == l))
            .filter(|(_, e)| q.manufacturer.as_ref().is_none_or(|m| &e.manufacturer == m))
            .filter(|(_, e)| q.only_keys.as_ref().is_none_or(|k| k.contains(&e.key())))
            .map(|(i, e)| {
                let rank = if qt.is_empty() || e.name.to_lowercase() == wanted {
                    0
                } else if self.name_tokens[i as usize]
                    .iter()
                    .any(|n| qt.iter().all(|t| n.starts_with(t.as_str())))
                    || e.name.to_lowercase().starts_with(&wanted)
                {
                    1
                } else if qt.iter().all(|t| {
                    self.name_tokens[i as usize]
                        .iter()
                        .any(|n| n.starts_with(t.as_str()))
                }) {
                    2
                } else {
                    3
                };
                (rank, e)
            })
            .collect();
        out.sort_by(|a, b| {
            (a.0, a.1.name.to_lowercase(), &a.1.id).cmp(&(b.0, b.1.name.to_lowercase(), &b.1.id))
        });
        let limit = if q.limit == 0 { usize::MAX } else { q.limit };
        out.into_iter().take(limit).map(|(_, e)| e).collect()
    }

    /// References of a component that do not resolve to an indexed symbol/footprint/model.
    pub fn unresolved(&self, c: &Entry) -> Vec<String> {
        let mut v = Vec::new();
        let has = |kind: Kind, id: &str| self.entries.iter().any(|e| e.kind == kind && e.id == id);
        if !c.symbol.is_empty() && !has(Kind::Symbol, &c.symbol) {
            v.push(format!("symbol {} not found", c.symbol));
        }
        if !c.footprint.is_empty() && !has(Kind::Footprint, &c.footprint) {
            v.push(format!("footprint {} not found", c.footprint));
        }
        if !c.model3d.is_empty()
            && c.model3d.contains(':')
            && !c.model3d.contains(['/', '\\'])
            && !has(Kind::Model3d, &c.model3d)
        {
            v.push(format!("3D model {} not found", c.model3d));
        }
        v
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn e(kind: Kind, lib: &str, name: &str, desc: &str, mfr: &str) -> Entry {
        let mut x = Entry::new(kind, "user", lib, name);
        x.description = desc.into();
        x.manufacturer = mfr.into();
        x
    }

    fn sample() -> Index {
        Index::build(
            vec![
                e(Kind::Symbol, "Buture", "R", "Resistor", ""),
                e(Kind::Symbol, "Buture", "NE555", "Timer", "TI"),
                e(
                    Kind::Footprint,
                    "Package_SO",
                    "SOIC-8_3.9mm",
                    "8-pin SOIC",
                    "",
                ),
                e(
                    Kind::Footprint,
                    "Resistor_SMD",
                    "R_0603_1608Metric",
                    "Resistor 0603",
                    "",
                ),
                e(
                    Kind::Component,
                    "Parts",
                    "NE555DR",
                    "555 timer SOIC-8",
                    "Texas Instruments",
                ),
                e(Kind::Component, "Parts", "LM358", "dual op amp", "TI"),
            ],
            vec![],
        )
    }

    fn names(v: Vec<&Entry>) -> Vec<String> {
        v.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn prefix_and_multi_token_search_with_ranking() {
        let ix = sample();
        let q = |t: &str| Query {
            text: t.into(),
            ..Query::default()
        };
        assert_eq!(names(ix.search(&q("ne5"))), ["NE555", "NE555DR"]);
        assert_eq!(names(ix.search(&q("0603 resistor"))), ["R_0603_1608Metric"]);
        assert_eq!(names(ix.search(&q("timer soic"))), ["NE555DR"]);
        assert_eq!(names(ix.search(&q("R")))[0], "R", "exact name first");
        assert!(ix.search(&q("zzz")).is_empty());
        assert_eq!(ix.search(&q("")).len(), 6);
    }

    #[test]
    fn filters_favorites_and_limit() {
        let ix = sample();
        let mut q = Query {
            text: "".into(),
            ..Query::default()
        };
        q.kinds.insert(Kind::Footprint);
        assert_eq!(ix.search(&q).len(), 2);
        let q2 = Query {
            manufacturer: Some("TI".into()),
            ..Query::default()
        };
        assert_eq!(names(ix.search(&q2)), ["LM358", "NE555"]);
        let q3 = Query {
            library: Some("Parts".into()),
            limit: 1,
            ..Query::default()
        };
        assert_eq!(ix.search(&q3).len(), 1);
        let key = ix.find("symbol:Buture:R").unwrap().key();
        let q4 = Query {
            only_keys: Some([key].into()),
            ..Query::default()
        };
        assert_eq!(names(ix.search(&q4)), ["R"]);
        assert_eq!(ix.manufacturers(), ["TI", "Texas Instruments"]);
    }

    #[test]
    fn unresolved_references_are_reported() {
        let mut ix_entries = vec![
            e(Kind::Symbol, "Buture", "NE555", "", ""),
            e(Kind::Footprint, "Package_SO", "SOIC-8", "", ""),
        ];
        let mut c = e(Kind::Component, "Parts", "NE555DR", "", "");
        c.symbol = "Buture:NE555".into();
        c.footprint = "Package_SO:SOIC-16".into();
        c.model3d = "Models:SOIC-8".into();
        ix_entries.push(c.clone());
        let ix = Index::build(ix_entries, vec![]);
        let u = ix.unresolved(&c);
        assert_eq!(u.len(), 2, "{u:?}");
        assert!(u[0].contains("SOIC-16") && u[1].contains("Models:SOIC-8"));
    }

    #[test]
    fn a_large_index_searches_fast() {
        let entries: Vec<Entry> = (0..60_000)
            .map(|i| {
                e(
                    Kind::Component,
                    &format!("Lib{}", i % 40),
                    &format!("PART{i:06}_{}", ["0603", "0805", "SOIC8", "QFN32"][i % 4]),
                    &format!("generic part number {i} series {}", i % 97),
                    if i % 3 == 0 { "ACME" } else { "Globex" },
                )
            })
            .collect();
        let ix = Index::build(entries, vec![]);
        let start = std::time::Instant::now();
        let mut total = 0;
        for t in ["part00123", "0805 acme", "series 42", "qfn32 globex"] {
            total += ix
                .search(&Query {
                    text: t.into(),
                    limit: 50,
                    ..Query::default()
                })
                .len();
        }
        assert!(total > 0);
        // Generous bound (debug build, shared CI): 4 searches over 60k entries.
        assert!(start.elapsed().as_millis() < 1500, "{:?}", start.elapsed());
    }
}
