//! Symbol libraries: one JSON file holding any number of symbols.
//!
//! Inside a library a symbol is stored under its *unqualified* name (`LM358`); once placed in a
//! project its name is the library-qualified id (`Analog:LM358`).

use crate::error::{LibraryError, Result};
use buturecad_schematic::symbol_edit;
use buturecad_schematic::Symbol;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const SYMBOL_LIB_EXT: &str = "bsymlib.json";
const FORMAT_TAG: &str = "buturecad-symbol-library";
pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    schema: u32,
    name: String,
    symbols: Vec<Symbol>,
}

/// `Library:Name` for a symbol of a library.
pub fn qualify(library: &str, name: &str) -> String {
    format!("{library}:{name}")
}

/// Splits `Library:Name` at the first colon.
pub fn split_lib_id(lib_id: &str) -> Option<(&str, &str)> {
    lib_id
        .split_once(':')
        .filter(|(l, n)| !l.is_empty() && !n.is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolLibrary {
    pub name: String,
    symbols: BTreeMap<String, Symbol>,
}

fn check_name(name: &str) -> Result<()> {
    crate::store::validate_library_name(name)
}

fn invalid(name: &str, reason: impl ToString) -> LibraryError {
    LibraryError::InvalidItem {
        kind: "symbol",
        name: name.to_owned(),
        reason: reason.to_string(),
    }
}

impl SymbolLibrary {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        check_name(&name)?;
        Ok(Self {
            name,
            symbols: BTreeMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.symbols.keys().map(String::as_str)
    }
    pub fn get(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    /// Adds or replaces a symbol. Accepts a qualified or unqualified name; it is stored
    /// unqualified. The symbol must validate.
    pub fn insert(&mut self, mut symbol: Symbol) -> Result<()> {
        if let Some((_, short)) = split_lib_id(&symbol.name) {
            symbol.name = short.to_owned();
        }
        if symbol.name.contains(':') {
            return Err(invalid(
                &symbol.name,
                "library symbol names cannot contain “:”",
            ));
        }
        symbol_edit::validate(&symbol).map_err(|e| invalid(&symbol.name, e))?;
        self.symbols.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Symbol> {
        self.symbols
            .remove(name)
            .ok_or_else(|| LibraryError::NoSuchItem {
                library: self.name.clone(),
                item: name.to_owned(),
            })
    }

    /// A copy of a symbol ready to embed in a project (its name is the qualified id).
    pub fn instantiate(&self, name: &str) -> Option<Symbol> {
        let mut s = self.symbols.get(name)?.clone();
        s.name = qualify(&self.name, name);
        Some(s)
    }

    /// Deterministic file content (symbols sorted by name, pretty-printed).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let file = File {
            format: FORMAT_TAG.into(),
            schema: SCHEMA,
            name: self.name.clone(),
            symbols: self.symbols.values().cloned().collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&file).map_err(|e| LibraryError::Invalid {
            path: Path::new(&self.name).to_path_buf(),
            reason: e.to_string(),
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self> {
        let bad = |reason: String| LibraryError::Invalid {
            path: path.to_path_buf(),
            reason,
        };
        let file: File = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
        if file.format != FORMAT_TAG {
            return Err(bad(format!("format tag is “{}”", file.format)));
        }
        if file.schema > SCHEMA {
            return Err(LibraryError::NewerSchema {
                path: path.to_path_buf(),
                found: file.schema,
                supported: SCHEMA,
            });
        }
        let mut lib = Self::new(file.name).map_err(|e| bad(e.to_string()))?;
        for s in file.symbols {
            let before = lib.symbols.len();
            let name = s.name.clone();
            lib.insert(s).map_err(|e| bad(e.to_string()))?;
            if lib.symbols.len() == before {
                return Err(bad(format!("symbol “{name}” appears twice")));
            }
        }
        Ok(lib)
    }
}
