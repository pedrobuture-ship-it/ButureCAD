//! The in-memory project model.

use thiserror::Error;

/// Maximum accepted project name length, in characters.
pub const MAX_NAME_LEN: usize = 128;

/// Invalid project data.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectError {
    #[error("project name must not be empty")]
    EmptyName,
    #[error("project name is longer than {MAX_NAME_LEN} characters")]
    NameTooLong,
    #[error("project name contains control characters")]
    NameHasControlChars,
}

/// A ButureCAD project. Design documents (schematic, PCB) are added by later stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    name: String,
}

impl Project {
    /// Creates a project, validating the name.
    pub fn new(name: impl Into<String>) -> Result<Self, ProjectError> {
        let name = name.into();
        Self::validate_name(&name)?;
        Ok(Self { name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Renames the project, validating the new name.
    pub fn set_name(&mut self, name: impl Into<String>) -> Result<(), ProjectError> {
        let name = name.into();
        Self::validate_name(&name)?;
        self.name = name;
        Ok(())
    }

    fn validate_name(name: &str) -> Result<(), ProjectError> {
        if name.trim().is_empty() {
            return Err(ProjectError::EmptyName);
        }
        if name.chars().count() > MAX_NAME_LEN {
            return Err(ProjectError::NameTooLong);
        }
        if name.chars().any(char::is_control) {
            return Err(ProjectError::NameHasControlChars);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_name() {
        assert_eq!(
            Project::new("Drone_FC").map(|p| p.name().to_owned()),
            Ok("Drone_FC".into())
        );
    }

    #[test]
    fn rejects_bad_names() {
        assert_eq!(Project::new("  "), Err(ProjectError::EmptyName));
        assert_eq!(Project::new("a\nb"), Err(ProjectError::NameHasControlChars));
        assert_eq!(
            Project::new("x".repeat(129)),
            Err(ProjectError::NameTooLong)
        );
    }
}
