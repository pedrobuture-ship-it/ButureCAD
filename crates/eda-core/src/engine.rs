//! The Command Engine: the only way user-level changes reach the [`Project`].

use crate::command::{Command, CommandError};
use crate::commands::CompositeCommand;
use crate::Project;

/// Default maximum number of undoable entries.
pub const DEFAULT_HISTORY_LIMIT: usize = 1000;

struct Group {
    label: String,
    done: Vec<Box<dyn Command>>,
}

/// Owns the project and applies, undoes and redoes [`Command`]s.
///
/// * **Transactions** group several commands into one undo step; they nest.
/// * **Dirty state** is relative to the last [`Self::mark_saved`] point and is exact: undoing back
///   to the saved state makes the project clean again.
/// * **Autosave** is driven by [`Self::revision`] (bumped by every state change); I/O lives in
///   `buturecad-io`, which keeps this crate headless and dependency-free.
/// * **Recovery**: a project restored from an autosave is wrapped with [`Self::recovered`] so it
///   starts dirty.
pub struct CommandEngine {
    project: Project,
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    open: Vec<Group>,
    /// `undo.len()` at the last save, or `None` if that state is no longer reachable.
    clean_at: Option<usize>,
    revision: u64,
    autosaved_revision: u64,
    limit: usize,
}

impl CommandEngine {
    /// Wraps a freshly created or loaded project (clean).
    pub fn new(project: Project) -> Self {
        Self {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            open: Vec::new(),
            clean_at: Some(0),
            revision: 0,
            autosaved_revision: 0,
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    /// Wraps a project restored from an autosave: it differs from disk, so it starts dirty.
    pub fn recovered(project: Project) -> Self {
        let mut e = Self::new(project);
        e.clean_at = None;
        e.revision = 1;
        e
    }

    pub fn with_history_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self.trim();
        self
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Reserves a fresh object id. Ids are never reused, so reserving one is not a model change
    /// and is not undoable; tools use this to build commands that create several objects.
    pub fn new_id(&mut self) -> crate::model::Id {
        self.project.alloc_id()
    }

    pub fn into_project(self) -> Project {
        self.project
    }

    /// Applies a command. On error nothing changes. Clears the redo stack.
    pub fn execute<C: Command + 'static>(&mut self, cmd: C) -> Result<(), CommandError> {
        self.execute_boxed(Box::new(cmd))
    }

    pub fn execute_boxed(&mut self, mut cmd: Box<dyn Command>) -> Result<(), CommandError> {
        cmd.apply(&mut self.project)?;
        self.revision += 1;
        self.discard_redo();
        match self.open.last_mut() {
            Some(group) => group.done.push(cmd),
            None => self.push_undo(cmd),
        }
        Ok(())
    }

    /// Opens a (possibly nested) transaction group.
    pub fn begin_transaction(&mut self, label: impl Into<String>) {
        self.open.push(Group {
            label: label.into(),
            done: Vec::new(),
        });
    }

    /// Closes the innermost group; it becomes one undo step (or part of the parent group).
    /// An empty group leaves no history entry.
    pub fn commit_transaction(&mut self) -> Result<(), CommandError> {
        let group = self.open.pop().ok_or(CommandError::NoTransaction)?;
        if group.done.is_empty() {
            return Ok(());
        }
        let composite: Box<dyn Command> = Box::new(CompositeCommand::new(group.label, group.done));
        match self.open.last_mut() {
            Some(parent) => parent.done.push(composite),
            None => self.push_undo(composite),
        }
        Ok(())
    }

    /// Reverts and discards everything done in the innermost group.
    pub fn rollback_transaction(&mut self) -> Result<(), CommandError> {
        let mut group = self.open.pop().ok_or(CommandError::NoTransaction)?;
        while let Some(mut cmd) = group.done.pop() {
            if let Err(e) = cmd.revert(&mut self.project) {
                return Err(self.corrupted(e));
            }
            self.revision += 1;
        }
        Ok(())
    }

    /// Runs `f` inside a transaction: committed on `Ok`, rolled back on `Err`.
    pub fn transaction<T>(
        &mut self,
        label: impl Into<String>,
        f: impl FnOnce(&mut CommandEngine) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        self.begin_transaction(label);
        match f(self) {
            Ok(v) => self.commit_transaction().map(|()| v),
            Err(e) => {
                self.rollback_transaction()?;
                Err(e)
            }
        }
    }

    pub fn transaction_depth(&self) -> usize {
        self.open.len()
    }

    pub fn can_undo(&self) -> bool {
        self.open.is_empty() && !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.open.is_empty() && !self.redo.is_empty()
    }

    /// Undoes the last step; returns its label, or `None` if there is nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>, CommandError> {
        self.require_no_transaction()?;
        let Some(mut cmd) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(e) = cmd.revert(&mut self.project) {
            return Err(self.corrupted(e));
        }
        self.revision += 1;
        let label = cmd.label();
        self.redo.push(cmd);
        Ok(Some(label))
    }

    /// Redoes the last undone step; returns its label, or `None` if there is nothing to redo.
    pub fn redo(&mut self) -> Result<Option<String>, CommandError> {
        self.require_no_transaction()?;
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(e) = cmd.apply(&mut self.project) {
            return Err(self.corrupted(e));
        }
        self.revision += 1;
        let label = cmd.label();
        self.undo.push(cmd);
        Ok(Some(label))
    }

    /// Undo history, oldest first.
    pub fn undo_history(&self) -> Vec<String> {
        self.undo.iter().map(|c| c.label()).collect()
    }

    /// Redo history, next-to-redo first.
    pub fn redo_history(&self) -> Vec<String> {
        self.redo.iter().rev().map(|c| c.label()).collect()
    }

    /// Does the project differ from the last saved state?
    pub fn is_dirty(&self) -> bool {
        self.clean_at != Some(self.undo.len()) || self.open.iter().any(|g| !g.done.is_empty())
    }

    /// Records that the current state has been written to disk.
    pub fn mark_saved(&mut self) -> Result<(), CommandError> {
        self.require_no_transaction()?;
        self.clean_at = Some(self.undo.len());
        Ok(())
    }

    /// Monotonic counter bumped by every state change (apply, undo, redo, rollback).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Is there an unsaved change that has not been autosaved yet?
    pub fn autosave_pending(&self) -> bool {
        self.is_dirty() && self.revision != self.autosaved_revision
    }

    /// Records that the state at `revision` has been autosaved. Pass the value returned by
    /// [`Self::revision`] from *before* the autosave started so concurrent edits are not lost.
    pub fn mark_autosaved(&mut self, revision: u64) {
        self.autosaved_revision = revision;
    }

    fn require_no_transaction(&self) -> Result<(), CommandError> {
        if self.open.is_empty() {
            Ok(())
        } else {
            Err(CommandError::TransactionOpen)
        }
    }

    fn push_undo(&mut self, cmd: Box<dyn Command>) {
        self.undo.push(cmd);
        self.trim();
    }

    fn discard_redo(&mut self) {
        if !self.redo.is_empty() {
            // The saved state lived in the discarded branch if it was beyond the undo stack.
            if self.clean_at.is_some_and(|c| c > self.undo.len()) {
                self.clean_at = None;
            }
            self.redo.clear();
        }
    }

    fn trim(&mut self) {
        if self.undo.len() > self.limit {
            let excess = self.undo.len() - self.limit;
            self.undo.drain(..excess);
            self.clean_at = self.clean_at.and_then(|c| c.checked_sub(excess));
        }
    }

    /// A revert/redo failure means history no longer matches the project (a command broke its
    /// contract). Drop history so no further corrupting step can run.
    fn corrupted(&mut self, cause: CommandError) -> CommandError {
        self.undo.clear();
        self.redo.clear();
        self.open.clear();
        self.clean_at = None;
        CommandError::HistoryCorrupted(cause.to_string())
    }
}
