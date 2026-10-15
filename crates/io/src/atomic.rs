//! Crash-safe file replacement.

use crate::error::{IoError, Result};
use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Crash simulation for tests. A real crash stops the process at an arbitrary point and cleans
/// nothing up; this hook reproduces that: when it fires, [`atomic_write`] returns an error at the
/// chosen point and leaves its temporary file behind exactly as a dead process would.
///
/// It is thread-local, costs one `Cell` read per write when unused, and is not meant for
/// application code.
pub mod fault {
    use super::Cell;

    /// Where in `atomic_write` the simulated crash happens.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Stage {
        /// Before the temporary file exists.
        BeforeWrite,
        /// Half of the content is in the temporary file.
        PartialWrite,
        /// The temporary file is complete and synced, but not renamed over the target.
        AfterWrite,
        /// Renamed over the target, directory not yet synced (the write is already visible).
        AfterRename,
    }

    thread_local! {
        static PLAN: Cell<Option<(Stage, usize)>> = const { Cell::new(None) };
        static CRASHED: Cell<bool> = const { Cell::new(false) };
    }

    /// The `(skip + 1)`-th write that reaches `stage` on this thread "crashes".
    pub fn inject(stage: Stage, skip: usize) {
        PLAN.with(|p| p.set(Some((stage, skip))));
        CRASHED.with(|c| c.set(false));
    }

    pub fn clear() {
        PLAN.with(|p| p.set(None));
        CRASHED.with(|c| c.set(false));
    }

    /// Has a simulated crash fired since the last `inject`?
    pub fn fired() -> bool {
        CRASHED.with(Cell::get)
    }

    pub(super) fn hit(stage: Stage) -> std::io::Result<()> {
        PLAN.with(|p| match p.get() {
            Some((s, 0)) if s == stage => {
                p.set(None);
                CRASHED.with(|c| c.set(true));
                Err(std::io::Error::other("simulated crash"))
            }
            Some((s, n)) if s == stage => {
                p.set(Some((s, n - 1)));
                Ok(())
            }
            _ => Ok(()),
        })
    }
}

/// Writes `bytes` to `path` so that readers (and a crash at any point) see either the
/// complete old content or the complete new content, never a mix.
///
/// Strategy: write a sibling temp file, fsync it, rename over the target, fsync the directory.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));

    let write = || -> std::io::Result<()> {
        fault::hit(fault::Stage::BeforeWrite)?;
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        if fault::hit(fault::Stage::PartialWrite).is_err() {
            f.write_all(&bytes[..bytes.len() / 2])?;
            f.sync_all()?;
            return Err(std::io::Error::other("simulated crash"));
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        fault::hit(fault::Stage::AfterWrite)?;
        fs::rename(&tmp, path)?;
        fault::hit(fault::Stage::AfterRename)?;
        sync_dir(dir)
    };
    write().map_err(|e| {
        // A simulated crash leaves its temporary file behind, like a dead process would.
        if !fault::fired() {
            let _ = fs::remove_file(&tmp);
        }
        IoError::fs("atomic write failed", path, e)
    })
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    // Directory handles cannot be fsynced portably on this platform.
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn replaces_content_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_write_keeps_old_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        atomic_write(&p, b"good").unwrap();
        // Target replaced by a directory: rename must fail and the temp file must be cleaned up.
        let blocked = dir.path().join("b");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("x"), b"x").unwrap();
        assert!(atomic_write(&blocked, b"bad").is_err());
        assert_eq!(fs::read(&p).unwrap(), b"good");
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
