//! The in-memory project. Mutated only through the Command Engine.

use crate::model::{Id, PcbDoc};
use buturecad_schematic::Schematic;
use thiserror::Error;

/// The root sheet always has this id; allocated ids start at 1, so it can never collide.
pub const ROOT_SHEET: Id = Id(0);

/// Maximum accepted project name length, in characters.
pub const MAX_NAME_LEN: usize = 128;

/// Invalid project data.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ProjectError {
    #[error("project name must not be empty")]
    EmptyName,
    #[error("project name is longer than {MAX_NAME_LEN} characters")]
    NameTooLong,
    #[error("project name contains control characters")]
    NameHasControlChars,
}

/// A ButureCAD project.
///
/// Fields are read through accessors. Mutation goes through commands (`crate::commands`) driven
/// by the [`crate::CommandEngine`]; this keeps undo/redo, dirty tracking and AI change review
/// reliable.
#[derive(Debug, Clone)]
pub struct Project {
    name: String,
    pub(crate) schematic: Schematic,
    pub(crate) pcb: PcbDoc,
    next_id: u64,
}

/// Equality compares design content (name and documents). The id counter is bookkeeping: it is
/// deliberately never rewound by undo, so ids are not reused within a session.
impl PartialEq for Project {
    fn eq(&self, o: &Self) -> bool {
        self.name == o.name && self.schematic == o.schematic && self.pcb == o.pcb
    }
}
impl Eq for Project {}

impl Project {
    /// Creates an empty project, validating the name.
    pub fn new(name: impl Into<String>) -> Result<Self, ProjectError> {
        let name = name.into();
        Self::validate_name(&name)?;
        Ok(Self {
            name,
            schematic: Schematic::new(ROOT_SHEET),
            pcb: PcbDoc::default(),
            next_id: 1,
        })
    }

    /// Rebuilds a project from loaded documents. The id counter resumes after the highest id.
    pub fn from_parts(
        name: impl Into<String>,
        schematic: Schematic,
        pcb: PcbDoc,
    ) -> Result<Self, ProjectError> {
        let mut p = Self::new(name)?;
        let max = schematic
            .sheets()
            .iter()
            .flat_map(|(sid, sheet)| std::iter::once(sid).chain(sheet.objects.keys()))
            .map(|id| id.0)
            .chain(pcb.ids().map(|id| id.0))
            .max()
            .unwrap_or(0);
        p.next_id = max.saturating_add(1).max(1);
        p.schematic = schematic;
        p.pcb = pcb;
        Ok(p)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn schematic(&self) -> &Schematic {
        &self.schematic
    }
    pub fn pcb(&self) -> &PcbDoc {
        &self.pcb
    }

    pub(crate) fn set_name(&mut self, name: String) -> Result<(), ProjectError> {
        Self::validate_name(&name)?;
        self.name = name;
        Ok(())
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Makes sure ids below `next` are never handed out again.
    pub(crate) fn reserve_ids_until(&mut self, next: u64) {
        self.next_id = self.next_id.max(next);
    }

    pub(crate) fn alloc_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn validate_name(name: &str) -> Result<(), ProjectError> {
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
        assert_eq!(Project::new("Drone_FC").unwrap().name(), "Drone_FC");
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

    #[test]
    fn from_parts_resumes_ids_after_the_highest() {
        use crate::model::{PcbDoc, Via};
        use buturecad_geometry::Point;
        let mut pcb = PcbDoc::default();
        pcb.vias.insert(
            Id(41),
            Via {
                position: Point::ORIGIN,
                diameter: 600_000,
                drill: 300_000,
                net: None,
                kind: Default::default(),
            },
        );
        let mut p = Project::from_parts("x", Schematic::new(ROOT_SHEET), pcb).unwrap();
        assert_eq!(p.alloc_id(), Id(42));
    }
}
