//! A folder of library files.

use crate::error::{LibraryError, Result};
use crate::footprint_lib::{FootprintLibrary, FOOTPRINT_LIB_EXT};
use crate::stackup_lib::{StackupPreset, STACKUP_PRESET_EXT};
use crate::symbol_lib::{SymbolLibrary, SYMBOL_LIB_EXT};
use buturecad_io::atomic::atomic_write;
use buturecad_pcb::Footprint;
use buturecad_schematic::Symbol;
use std::path::{Path, PathBuf};

/// Libraries larger than this are refused (they are untrusted input).
const MAX_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn validate_library_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(LibraryError::BadLibraryName(name.to_owned()))
    }
}

/// A library found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySummary {
    pub name: String,
    pub path: PathBuf,
    pub item_count: usize,
    /// Set when the file exists but cannot be read; the library is listed so the user can see
    /// that something is wrong instead of silently losing it.
    pub error: Option<String>,
}

/// A folder holding library files. The folder is created on first write.
#[derive(Debug, Clone)]
pub struct LibraryDir {
    root: PathBuf,
}

impl LibraryDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn symbol_path(&self, name: &str) -> Result<PathBuf> {
        validate_library_name(name)?;
        Ok(self.root.join(format!("{name}.{SYMBOL_LIB_EXT}")))
    }

    fn read(path: &Path) -> Result<Vec<u8>> {
        let fs_err = |source| LibraryError::Fs {
            context: "cannot read library",
            path: path.to_path_buf(),
            source,
        };
        let len = std::fs::metadata(path).map_err(fs_err)?.len();
        if len > MAX_LIBRARY_BYTES {
            return Err(LibraryError::Invalid {
                path: path.to_path_buf(),
                reason: format!("file is {len} bytes, over the {MAX_LIBRARY_BYTES} byte limit"),
            });
        }
        std::fs::read(path).map_err(fs_err)
    }

    /// Symbol libraries in this folder, sorted by name. A missing folder is an empty list.
    pub fn list_symbol_libraries(&self) -> Vec<LibrarySummary> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let suffix = format!(".{SYMBOL_LIB_EXT}");
        let mut out: Vec<LibrarySummary> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                let name = file.strip_suffix(&suffix)?.to_owned();
                (!file.starts_with('.')).then_some((name, e.path()))
            })
            .map(|(name, path)| {
                match Self::read(&path).and_then(|b| SymbolLibrary::from_bytes(&path, &b)) {
                    Ok(lib) => LibrarySummary {
                        name,
                        path,
                        item_count: lib.len(),
                        error: None,
                    },
                    Err(e) => LibrarySummary {
                        name,
                        path,
                        item_count: 0,
                        error: Some(e.to_string()),
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load_symbol_library(&self, name: &str) -> Result<SymbolLibrary> {
        let path = self.symbol_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        let lib = SymbolLibrary::from_bytes(&path, &Self::read(&path)?)?;
        if lib.name != name {
            return Err(LibraryError::Invalid {
                path,
                reason: format!("file declares library “{}”", lib.name),
            });
        }
        Ok(lib)
    }

    /// Writes a library atomically (a crash never leaves a half-written file).
    pub fn save_symbol_library(&self, lib: &SymbolLibrary) -> Result<()> {
        let path = self.symbol_path(&lib.name)?;
        std::fs::create_dir_all(&self.root).map_err(|source| LibraryError::Fs {
            context: "cannot create library folder",
            path: self.root.clone(),
            source,
        })?;
        Ok(atomic_write(&path, &lib.to_bytes()?)?)
    }

    pub fn create_symbol_library(&self, name: &str) -> Result<SymbolLibrary> {
        if self.symbol_path(name)?.exists() {
            return Err(LibraryError::LibraryExists(name.to_owned()));
        }
        let lib = SymbolLibrary::new(name)?;
        self.save_symbol_library(&lib)?;
        Ok(lib)
    }

    /// Adds or replaces `symbol` in library `library`, creating the library when absent.
    /// Returns the library-qualified id.
    pub fn save_symbol(&self, library: &str, symbol: Symbol) -> Result<String> {
        let mut lib = match self.load_symbol_library(library) {
            Ok(l) => l,
            Err(LibraryError::NoSuchLibrary(_)) => SymbolLibrary::new(library)?,
            Err(e) => return Err(e),
        };
        lib.insert(symbol.clone())?;
        self.save_symbol_library(&lib)?;
        let short = symbol.name.rsplit(':').next().unwrap_or(&symbol.name);
        Ok(crate::symbol_lib::qualify(library, short))
    }

    pub fn delete_symbol(&self, library: &str, name: &str) -> Result<()> {
        let mut lib = self.load_symbol_library(library)?;
        lib.remove(name)?;
        self.save_symbol_library(&lib)
    }

    // -- layer stack presets -------------------------------------------------------------------

    fn stackup_path(&self, name: &str) -> Result<PathBuf> {
        validate_library_name(name)?;
        Ok(self.root.join(format!("{name}.{STACKUP_PRESET_EXT}")))
    }

    /// Layer stack presets in this folder, sorted by name (unreadable ones are listed with their
    /// error). A missing folder is an empty list.
    pub fn list_stackup_presets(&self) -> Vec<LibrarySummary> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let suffix = format!(".{STACKUP_PRESET_EXT}");
        let mut out: Vec<LibrarySummary> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                let name = file.strip_suffix(&suffix)?.to_owned();
                (!file.starts_with('.')).then_some((name, e.path()))
            })
            .map(|(name, path)| {
                match Self::read(&path).and_then(|b| StackupPreset::from_bytes(&path, &b)) {
                    Ok(p) => LibrarySummary {
                        name,
                        path,
                        item_count: usize::from(p.copper_layers),
                        error: None,
                    },
                    Err(e) => LibrarySummary {
                        name,
                        path,
                        item_count: 0,
                        error: Some(e.to_string()),
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load_stackup_preset(&self, name: &str) -> Result<StackupPreset> {
        let path = self.stackup_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        StackupPreset::from_bytes(&path, &Self::read(&path)?)
    }

    /// Writes (or replaces) a preset atomically.
    pub fn save_stackup_preset(&self, preset: &StackupPreset) -> Result<()> {
        let path = self.stackup_path(&preset.name)?;
        std::fs::create_dir_all(&self.root).map_err(|source| LibraryError::Fs {
            context: "cannot create library folder",
            path: self.root.clone(),
            source,
        })?;
        Ok(atomic_write(&path, &preset.to_bytes()?)?)
    }

    pub fn delete_stackup_preset(&self, name: &str) -> Result<()> {
        let path = self.stackup_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        std::fs::remove_file(&path).map_err(|source| LibraryError::Fs {
            context: "cannot delete the preset",
            path,
            source,
        })
    }

    // -- footprint libraries -------------------------------------------------------------------

    fn footprint_path(&self, name: &str) -> Result<PathBuf> {
        validate_library_name(name)?;
        Ok(self.root.join(format!("{name}.{FOOTPRINT_LIB_EXT}")))
    }

    /// Footprint libraries in this folder, sorted by name. A missing folder is an empty list.
    pub fn list_footprint_libraries(&self) -> Vec<LibrarySummary> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let suffix = format!(".{FOOTPRINT_LIB_EXT}");
        let mut out: Vec<LibrarySummary> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                let name = file.strip_suffix(&suffix)?.to_owned();
                (!file.starts_with('.')).then_some((name, e.path()))
            })
            .map(|(name, path)| {
                match Self::read(&path).and_then(|b| FootprintLibrary::from_bytes(&path, &b)) {
                    Ok(lib) => LibrarySummary {
                        name,
                        path,
                        item_count: lib.len(),
                        error: None,
                    },
                    Err(e) => LibrarySummary {
                        name,
                        path,
                        item_count: 0,
                        error: Some(e.to_string()),
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load_footprint_library(&self, name: &str) -> Result<FootprintLibrary> {
        let path = self.footprint_path(name)?;
        if !path.exists() {
            return Err(LibraryError::NoSuchLibrary(name.to_owned()));
        }
        let lib = FootprintLibrary::from_bytes(&path, &Self::read(&path)?)?;
        if lib.name != name {
            return Err(LibraryError::Invalid {
                path,
                reason: format!("file declares library “{}”", lib.name),
            });
        }
        Ok(lib)
    }

    /// Writes a footprint library atomically.
    pub fn save_footprint_library(&self, lib: &FootprintLibrary) -> Result<()> {
        let path = self.footprint_path(&lib.name)?;
        std::fs::create_dir_all(&self.root).map_err(|source| LibraryError::Fs {
            context: "cannot create library folder",
            path: self.root.clone(),
            source,
        })?;
        Ok(atomic_write(&path, &lib.to_bytes()?)?)
    }

    /// Adds or replaces `footprint` in `library`, creating the library when absent. Returns the
    /// library-qualified id.
    pub fn save_footprint(&self, library: &str, footprint: Footprint) -> Result<String> {
        let mut lib = match self.load_footprint_library(library) {
            Ok(l) => l,
            Err(LibraryError::NoSuchLibrary(_)) => FootprintLibrary::new(library)?,
            Err(e) => return Err(e),
        };
        lib.insert(footprint.clone())?;
        self.save_footprint_library(&lib)?;
        let short = footprint.name.rsplit(':').next().unwrap_or(&footprint.name);
        Ok(crate::symbol_lib::qualify(library, short))
    }

    pub fn delete_footprint(&self, library: &str, name: &str) -> Result<()> {
        let mut lib = self.load_footprint_library(library)?;
        lib.remove(name)?;
        self.save_footprint_library(&lib)
    }
}
