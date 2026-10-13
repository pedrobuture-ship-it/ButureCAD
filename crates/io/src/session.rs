//! Project folders on disk: create, open, save, save as, backups, autosave, crash recovery.
//!
//! Layout (see `docs/file-format/`):
//!
//! ```text
//! MyBoard/
//!   project.buture      manifest (source of truth)
//!   schematic/ pcb/ libraries/ models/ scripts/ rules/ output/
//!   .buture/            local state: backups/, autosave/, session.lock (not for version control)
//! ```

use crate::atomic::atomic_write;
use crate::error::{IoError, Result};
use crate::format::{parse_project, serialize_project};
use buturecad_eda_core::Project;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_NAME: &str = "project.buture";
pub const STATE_DIR: &str = ".buture";
const AUTOSAVE_FILE: &str = "autosave/project.buture";
const LOCK_FILE: &str = "session.lock";
const BACKUP_DIR: &str = "backups";
/// Number of manifest backups kept.
pub const MAX_BACKUPS: usize = 5;
const SUBDIRS: [&str; 6] = [
    "schematic",
    "pcb",
    "libraries",
    "models",
    "scripts",
    "rules",
];

/// An autosave left behind by a session that did not shut down cleanly.
#[derive(Debug)]
pub struct Recovery {
    pub autosave_path: PathBuf,
    /// The recovered project, if the autosave file was readable.
    pub project: std::result::Result<Project, String>,
}

/// Result of [`ProjectSession::open`].
#[derive(Debug)]
pub struct Opened {
    pub session: ProjectSession,
    pub project: Project,
    /// Present only when the previous session crashed and left a newer autosave.
    pub recovery: Option<Recovery>,
}

/// An open project folder. Holds the session lock until dropped.
///
/// The lock is advisory crash detection only: it does not prevent two live instances from
/// opening the same project (not implemented).
#[derive(Debug)]
pub struct ProjectSession {
    root: PathBuf,
}

impl ProjectSession {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Creates a new project folder at `root` (which may exist but must hold no project).
    pub fn create(root: impl Into<PathBuf>, name: &str) -> Result<(Self, Project)> {
        let root = root.into();
        if root.join(MANIFEST_NAME).exists() {
            return Err(IoError::AlreadyExists(root));
        }
        let project = Project::new(name).map_err(|source| IoError::Invalid {
            path: root.join(MANIFEST_NAME),
            source,
        })?;
        init_layout(&root)?;
        atomic_write(&root.join(MANIFEST_NAME), &serialize_project(&project))?;
        let session = Self::attach(root)?;
        Ok((session, project))
    }

    /// Opens an existing project. A leftover session lock marks the previous session as
    /// crashed; if an autosave exists it is offered through [`Opened::recovery`].
    pub fn open(root: impl Into<PathBuf>) -> Result<Opened> {
        let root = root.into();
        let manifest = root.join(MANIFEST_NAME);
        let bytes = fs::read(&manifest).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound(manifest.clone()),
            _ => IoError::fs("cannot read project file", &manifest, e),
        })?;
        let project = parse_project(&manifest, &bytes)?;

        let state = root.join(STATE_DIR);
        let crashed = state.join(LOCK_FILE).exists();
        let autosave = state.join(AUTOSAVE_FILE);
        let recovery = (crashed && autosave.is_file()).then(|| Recovery {
            project: fs::read(&autosave)
                .map_err(|e| e.to_string())
                .and_then(|b| parse_project(&autosave, &b).map_err(|e| e.to_string())),
            autosave_path: autosave,
        });
        let session = Self::attach(root)?;
        Ok(Opened {
            session,
            project,
            recovery,
        })
    }

    /// Atomically writes the manifest, keeping a rotating backup of the previous valid one.
    pub fn save(&self, project: &Project) -> Result<()> {
        let manifest = self.root.join(MANIFEST_NAME);
        self.backup_current()?;
        atomic_write(&manifest, &serialize_project(project))?;
        self.discard_autosave()
    }

    /// Copies the project folder to `new_root` (excluding `.buture/`), saves there, and
    /// switches this session to the new location. The original folder is left untouched.
    pub fn save_as(&mut self, new_root: impl Into<PathBuf>, project: &Project) -> Result<()> {
        let new_root = new_root.into();
        if new_root.join(MANIFEST_NAME).exists() {
            return Err(IoError::AlreadyExists(new_root));
        }
        init_layout(&new_root)?;
        copy_tree(&self.root, &new_root)?;
        atomic_write(&new_root.join(MANIFEST_NAME), &serialize_project(project))?;
        let new_session = Self::attach(new_root)?;
        // Dropping the old session releases the old lock.
        *self = new_session;
        Ok(())
    }

    /// Writes the working state to the autosave slot without touching `project.buture`.
    pub fn autosave(&self, project: &Project) -> Result<()> {
        let path = self.root.join(STATE_DIR).join(AUTOSAVE_FILE);
        ensure_parent(&path)?;
        atomic_write(&path, &serialize_project(project))
    }

    /// Removes any autosave (after a successful save or when the user declines recovery).
    pub fn discard_autosave(&self) -> Result<()> {
        let path = self.root.join(STATE_DIR).join(AUTOSAVE_FILE);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(IoError::fs("cannot remove autosave", path, e)),
        }
    }

    /// Backup files, oldest first.
    pub fn backups(&self) -> Result<Vec<PathBuf>> {
        let dir = self.root.join(STATE_DIR).join(BACKUP_DIR);
        let mut files: Vec<PathBuf> = match fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(IoError::fs("cannot list backups", dir, e)),
        };
        files.sort();
        Ok(files)
    }

    fn attach(root: PathBuf) -> Result<Self> {
        let lock = root.join(STATE_DIR).join(LOCK_FILE);
        ensure_parent(&lock)?;
        fs::write(&lock, std::process::id().to_string())
            .map_err(|e| IoError::fs("cannot create session lock", &lock, e))?;
        Ok(Self { root })
    }

    /// Copies the current manifest to `backups/project.NNNNNN.buture` (only if it is valid,
    /// so a corrupt file never displaces a good backup) and prunes old ones.
    fn backup_current(&self) -> Result<()> {
        let manifest = self.root.join(MANIFEST_NAME);
        let Ok(bytes) = fs::read(&manifest) else {
            return Ok(());
        };
        if parse_project(&manifest, &bytes).is_err() {
            return Ok(());
        }
        let existing = self.backups()?;
        let next = existing
            .last()
            .and_then(|p| backup_index(p))
            .map_or(1, |n| n + 1);
        let dir = self.root.join(STATE_DIR).join(BACKUP_DIR);
        fs::create_dir_all(&dir).map_err(|e| IoError::fs("cannot create directory", &dir, e))?;
        atomic_write(&dir.join(format!("project.{next:06}.buture")), &bytes)?;
        let all = self.backups()?;
        for old in all.iter().take(all.len().saturating_sub(MAX_BACKUPS)) {
            fs::remove_file(old).map_err(|e| IoError::fs("cannot prune backup", old, e))?;
        }
        Ok(())
    }
}

impl Drop for ProjectSession {
    /// Clean shutdown: releases the lock so the next open does not offer recovery.
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.join(STATE_DIR).join(LOCK_FILE));
    }
}

fn backup_index(path: &Path) -> Option<u64> {
    path.file_name()?
        .to_str()?
        .strip_prefix("project.")?
        .strip_suffix(".buture")?
        .parse()
        .ok()
}

fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(p) => fs::create_dir_all(p).map_err(|e| IoError::fs("cannot create directory", p, e)),
        None => Ok(()),
    }
}

fn init_layout(root: &Path) -> Result<()> {
    for sub in SUBDIRS {
        let dir = root.join(sub);
        fs::create_dir_all(&dir).map_err(|e| IoError::fs("cannot create directory", &dir, e))?;
    }
    let out = root.join("output");
    fs::create_dir_all(&out).map_err(|e| IoError::fs("cannot create directory", &out, e))?;
    Ok(())
}

/// Recursively copies `src` into `dst`, skipping `.buture/` and the manifest. Symlinks are refused.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    let rd = fs::read_dir(src).map_err(|e| IoError::fs("cannot read directory", src, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| IoError::fs("cannot read directory", src, e))?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        let kind = entry
            .file_type()
            .map_err(|e| IoError::fs("cannot stat", &from, e))?;
        if kind.is_symlink() {
            return Err(IoError::Symlink(from));
        }
        if name == STATE_DIR || name == MANIFEST_NAME {
            continue;
        }
        if kind.is_dir() {
            fs::create_dir_all(&to).map_err(|e| IoError::fs("cannot create directory", &to, e))?;
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| IoError::fs("cannot copy file", &from, e))?;
        }
    }
    Ok(())
}
