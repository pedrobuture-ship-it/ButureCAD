//! Glue between the Command Engine and project storage.

use crate::error::Result;
use crate::session::ProjectSession;
use buturecad_eda_core::{CommandEngine, CommandError};

/// Saves the engine's project and, only if the write succeeded, marks it clean.
/// Refused while a transaction is open (the state would be half-finished).
pub fn save(session: &ProjectSession, engine: &mut CommandEngine) -> Result<()> {
    if engine.transaction_depth() > 0 {
        return Err(CommandError::TransactionOpen.into());
    }
    session.save(engine.project())?;
    engine.mark_saved()?;
    Ok(())
}

/// Autosaves if there are unsaved, not-yet-autosaved changes. Returns whether a write happened.
pub fn autosave_if_pending(session: &ProjectSession, engine: &mut CommandEngine) -> Result<bool> {
    if !engine.autosave_pending() {
        return Ok(false);
    }
    let revision = engine.revision();
    session.autosave(engine.project())?;
    engine.mark_autosaved(revision);
    Ok(true)
}
