//! Per-user favorites and recently used library items (keys from [`crate::Entry::key`]).
//! Stored in one small JSON file written atomically; a missing or broken file is an empty state.

use buturecad_io::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

pub const MAX_RECENT: usize = 20;
const FORMAT_TAG: &str = "buturecad-library-prefs";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub favorites: BTreeSet<String>,
    /// Most recent first.
    #[serde(default)]
    pub recents: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    #[serde(flatten)]
    prefs: Prefs,
}

impl Prefs {
    pub fn toggle_favorite(&mut self, key: &str) -> bool {
        if self.favorites.remove(key) {
            false
        } else {
            self.favorites.insert(key.to_owned());
            true
        }
    }

    pub fn touch_recent(&mut self, key: &str) {
        self.recents.retain(|k| k != key);
        self.recents.insert(0, key.to_owned());
        self.recents.truncate(MAX_RECENT);
    }

    /// Loads the file; anything unreadable gives an empty state (never an error to the user:
    /// favorites are a convenience, not project data).
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .filter(|f| f.format == FORMAT_TAG)
            .map(|f| f.prefs)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let f = File {
            format: FORMAT_TAG.into(),
            prefs: self.clone(),
        };
        let mut b = serde_json::to_vec_pretty(&f).map_err(|e| e.to_string())?;
        b.push(b'\n');
        atomic_write(path, &b).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn favorites_recents_and_persistence() {
        let mut p = Prefs::default();
        assert!(p.toggle_favorite("symbol:Buture:R"));
        assert!(!p.toggle_favorite("symbol:Buture:R"));
        p.toggle_favorite("symbol:Buture:C");
        for i in 0..30 {
            p.touch_recent(&format!("k{i}"));
        }
        p.touch_recent("k5");
        assert_eq!(p.recents.len(), MAX_RECENT);
        assert_eq!(p.recents[0], "k5");
        assert_eq!(p.recents.iter().filter(|k| *k == "k5").count(), 1);
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x/prefs.json");
        p.save(&f).unwrap();
        assert_eq!(Prefs::load(&f), p);
        std::fs::write(&f, b"junk").unwrap();
        assert_eq!(Prefs::load(&f), Prefs::default());
        assert_eq!(Prefs::load(&d.path().join("missing")), Prefs::default());
    }
}
