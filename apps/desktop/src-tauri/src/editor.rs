//! Editor commands that are not pointer gestures (select all, clear, …). Schematic editing tools
//! are added in the editor stage.

use crate::workspace::Workspace;

impl Workspace {
    /// Runs a named editor command. Unknown names are an error, never silently ignored.
    pub fn editor_command(&mut self, name: &str) -> Result<(), String> {
        match name {
            "select_all" => self.select_all(),
            "clear_selection" | "cancel" => self.clear_selection(),
            other => return Err(format!("unknown editor command: {other}")),
        }
        Ok(())
    }
}
