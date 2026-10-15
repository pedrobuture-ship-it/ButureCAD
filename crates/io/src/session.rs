//! Project folders on disk: create, open, save, save as, backups, autosave, crash recovery.
//!
//! Layout (see `docs/file-format/`):
//!
//! ```text
//! MyBoard/
//!   project.buture            manifest
//!   schematic/main.bsch.json  schematic document
//!   pcb/main.bpcb.json        PCB document
//!   libraries/ models/ scripts/ rules/ output/
//!   .buture/                  local state: backups/, autosave/, session.lock (not for VCS)
//! ```

use crate::atomic::atomic_write;
use crate::error::{IoError, Result};
use crate::format::{
    parse_manifest, parse_pcb, parse_schematic, serialize_manifest, serialize_pcb,
    serialize_schematic,
};
use buturecad_eda_core::Project;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_NAME: &str = "project.buture";
pub const SCHEMATIC_FILE: &str = "schematic/main.bsch.json";
pub const PCB_FILE: &str = "pcb/main.bpcb.json";
pub const STATE_DIR: &str = ".buture";
const AUTOSAVE_DIR: &str = "autosave";
const LOCK_FILE: &str = "session.lock";
/// Present while `save` is writing the project files; if it is found on open, the last save was
/// interrupted and the files may be a mix of old and new documents.
const SAVING_MARKER: &str = "saving";
const BACKUP_DIR: &str = "backups";
/// Number of backup snapshots kept.
pub const MAX_BACKUPS: usize = 5;
const SUBDIRS: [&str; 7] = [
    "schematic",
    "pcb",
    "libraries",
    "models",
    "scripts",
    "rules",
    "output",
];

/// An autosave left behind by a session that did not shut down cleanly.
#[derive(Debug)]
pub struct Recovery {
    pub autosave_dir: PathBuf,
    /// The recovered project, if the autosave was readable. Wrap with
    /// `CommandEngine::recovered` so it starts dirty.
    pub project: std::result::Result<Project, String>,
}

/// The previous save did not finish (power loss, crash, kill). The project folder may hold a mix
/// of old and new documents, so the project was loaded from the last complete backup instead.
#[derive(Debug)]
pub struct InterruptedSave {
    /// The backup snapshot the project was loaded from (`None`: there was no backup, the files on
    /// disk were used).
    pub restored_from: Option<PathBuf>,
    /// Why the files on disk were not used (a parse error), or `None` if they were readable but
    /// possibly inconsistent.
    pub disk_state_error: Option<String>,
}

/// Result of [`ProjectSession::open`].
#[derive(Debug)]
pub struct Opened {
    pub session: ProjectSession,
    pub project: Project,
    /// Present only when the previous session crashed and left an autosave.
    pub recovery: Option<Recovery>,
    /// Set when the previous save was interrupted (see [`InterruptedSave`]).
    pub interrupted_save: Option<InterruptedSave>,
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
        write_project(&root, &project)?;
        let session = Self::attach(root)?;
        Ok((session, project))
    }

    /// Opens an existing project. A leftover session lock marks the previous session as
    /// crashed; if an autosave exists it is offered through [`Opened::recovery`].
    pub fn open(root: impl Into<PathBuf>) -> Result<Opened> {
        let root = root.into();
        let state = root.join(STATE_DIR);
        let (project, interrupted_save) = if state.join(SAVING_MARKER).exists() {
            Self::open_after_interrupted_save(&root)?
        } else {
            (read_project(&root)?, None)
        };
        let autosave = state.join(AUTOSAVE_DIR);
        let crashed = state.join(LOCK_FILE).exists();
        let recovery = (crashed && autosave.join(MANIFEST_NAME).is_file()).then(|| Recovery {
            project: read_project(&autosave).map_err(|e| e.to_string()),
            autosave_dir: autosave,
        });
        let session = Self::attach(root)?;
        Ok(Opened {
            session,
            project,
            recovery,
            interrupted_save,
        })
    }

    /// The marker says a save was cut short: prefer the newest complete backup over the files on
    /// disk, which may mix documents of two versions.
    fn open_after_interrupted_save(root: &Path) -> Result<(Project, Option<InterruptedSave>)> {
        let disk = read_project(root);
        let backups = Self::list_backups(root)?;
        for b in backups.iter().rev() {
            if let Ok(p) = read_project(b) {
                return Ok((
                    p,
                    Some(InterruptedSave {
                        restored_from: Some(b.clone()),
                        disk_state_error: disk.err().map(|e| e.to_string()),
                    }),
                ));
            }
        }
        // No usable backup (the very first save was interrupted): the files are all there is.
        let p = disk?;
        Ok((
            p,
            Some(InterruptedSave {
                restored_from: None,
                disk_state_error: None,
            }),
        ))
    }

    /// Writes the project, keeping a rotating backup snapshot of the previous valid state.
    ///
    /// Each file is replaced atomically, documents first and the manifest last. The set of files
    /// is **not** a single atomic unit: a crash mid-save can leave new documents next to an old
    /// manifest. The backup snapshot taken beforehand is the recovery path for that case.
    pub fn save(&self, project: &Project) -> Result<()> {
        let marker = self.root.join(STATE_DIR).join(SAVING_MARKER);
        // If an earlier save was interrupted the files may be mixed: do not snapshot them.
        if !marker.exists() {
            self.backup_current()?;
        }
        ensure_parent(&marker)?;
        atomic_write(&marker, b"saving\n")?;
        write_project(&self.root, project)?;
        // Only after every file is in place is the save complete.
        match fs::remove_file(&marker) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(IoError::fs("cannot finish the save", &marker, e)),
        }
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
        write_project(&new_root, project)?;
        let new_session = Self::attach(new_root)?;
        // Dropping the old session releases the old lock.
        *self = new_session;
        Ok(())
    }

    /// Writes the working state to the autosave slot without touching the project files.
    pub fn autosave(&self, project: &Project) -> Result<()> {
        write_project(&self.root.join(STATE_DIR).join(AUTOSAVE_DIR), project)
    }

    /// Removes any autosave (after a successful save or when the user declines recovery).
    pub fn discard_autosave(&self) -> Result<()> {
        let dir = self.root.join(STATE_DIR).join(AUTOSAVE_DIR);
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(IoError::fs("cannot remove autosave", dir, e)),
        }
    }

    /// Backup snapshot directories, oldest first.
    pub fn backups(&self) -> Result<Vec<PathBuf>> {
        Self::list_backups(&self.root)
    }

    fn list_backups(root: &Path) -> Result<Vec<PathBuf>> {
        let dir = root.join(STATE_DIR).join(BACKUP_DIR);
        let mut snapshots: Vec<PathBuf> = match fs::read_dir(&dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(IoError::fs("cannot list backups", dir, e)),
        };
        snapshots.sort();
        Ok(snapshots)
    }

    fn attach(root: PathBuf) -> Result<Self> {
        let lock = root.join(STATE_DIR).join(LOCK_FILE);
        ensure_parent(&lock)?;
        fs::write(&lock, std::process::id().to_string())
            .map_err(|e| IoError::fs("cannot create session lock", &lock, e))?;
        Ok(Self { root })
    }

    /// Snapshots the current project files into `backups/NNNNNN/` — only when everything on
    /// disk parses, so a corrupt state never displaces a good backup — and prunes old ones.
    fn backup_current(&self) -> Result<()> {
        if read_project(&self.root).is_err() {
            return Ok(());
        }
        let existing = self.backups()?;
        let next = existing
            .last()
            .and_then(|p| backup_index(p))
            .map_or(1, |n| n + 1);
        let snapshot = self
            .root
            .join(STATE_DIR)
            .join(BACKUP_DIR)
            .join(format!("{next:06}"));
        for rel in [MANIFEST_NAME, SCHEMATIC_FILE, PCB_FILE] {
            let from = self.root.join(rel);
            if let Ok(bytes) = fs::read(&from) {
                let to = snapshot.join(rel);
                ensure_parent(&to)?;
                atomic_write(&to, &bytes)?;
            }
        }
        let all = self.backups()?;
        for old in all.iter().take(all.len().saturating_sub(MAX_BACKUPS)) {
            fs::remove_dir_all(old).map_err(|e| IoError::fs("cannot prune backup", old, e))?;
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

/// Reads a project laid out under `root`. The manifest is required; a missing document file
/// means an empty document (projects created before documents existed).
pub fn read_project(root: &Path) -> Result<Project> {
    let manifest_path = root.join(MANIFEST_NAME);
    let bytes =
        read_file(&manifest_path)?.ok_or_else(|| IoError::NotFound(manifest_path.clone()))?;
    let manifest = parse_manifest(&manifest_path, &bytes)?;

    let sch_path = root.join(SCHEMATIC_FILE);
    let schematic = match read_file(&sch_path)? {
        Some(b) => parse_schematic(&sch_path, &b)?,
        None => buturecad_schematic::Schematic::new(buturecad_eda_core::ROOT_SHEET),
    };
    let pcb_path = root.join(PCB_FILE);
    let pcb = match read_file(&pcb_path)? {
        Some(b) => parse_pcb(&pcb_path, &b)?,
        None => Default::default(),
    };
    Project::from_parts(manifest.name, schematic, pcb).map_err(|source| IoError::Invalid {
        path: manifest_path,
        source,
    })
}

/// Writes documents first and the manifest last.
fn write_project(root: &Path, project: &Project) -> Result<()> {
    for (rel, bytes) in [
        (SCHEMATIC_FILE, serialize_schematic(project.schematic())),
        (PCB_FILE, serialize_pcb(project.pcb())),
        (MANIFEST_NAME, serialize_manifest(project.name())),
    ] {
        let path = root.join(rel);
        ensure_parent(&path)?;
        atomic_write(&path, &bytes)?;
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(IoError::fs("cannot read project file", path, e)),
    }
}

fn backup_index(path: &Path) -> Option<u64> {
    path.file_name()?.to_str()?.parse().ok()
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
