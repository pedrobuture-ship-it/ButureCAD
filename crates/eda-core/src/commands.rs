//! Concrete commands.

use crate::command::{Command, CommandError};
use crate::model::{Component, Entity, Id, Track, Via, Wire};
use crate::Project;
use buturecad_geometry::Point;

/// Creates an entity. The id is allocated on first apply and reused on redo.
#[derive(Debug)]
pub struct CreateCommand<E: Entity> {
    entity: E,
    id: Option<Id>,
}

impl<E: Entity> CreateCommand<E> {
    pub fn new(entity: E) -> Self {
        Self { entity, id: None }
    }
    /// Id of the created entity; `None` until the command has been applied.
    pub fn id(&self) -> Option<Id> {
        self.id
    }
}

impl<E: Entity> Command for CreateCommand<E> {
    fn label(&self) -> String {
        format!("Create {}", E::KIND)
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        self.entity.validate()?;
        let id = match self.id {
            Some(id) => id,
            None => project.alloc_id(),
        };
        if E::store(project).contains_key(&id) {
            return Err(CommandError::IdInUse { kind: E::KIND, id });
        }
        E::store(project).insert(id, self.entity.clone());
        self.id = Some(id);
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let id = self
            .id
            .ok_or_else(|| CommandError::HistoryCorrupted("create never applied".into()))?;
        E::store(project)
            .remove(&id)
            .map(|_| ())
            .ok_or(CommandError::MissingEntity { kind: E::KIND, id })
    }
}

/// Deletes an entity, remembering it so undo restores it under the same id.
#[derive(Debug)]
pub struct DeleteCommand<E: Entity> {
    id: Id,
    removed: Option<E>,
}

impl<E: Entity> DeleteCommand<E> {
    pub fn new(id: Id) -> Self {
        Self { id, removed: None }
    }
}

impl<E: Entity> Command for DeleteCommand<E> {
    fn label(&self) -> String {
        format!("Delete {}", E::KIND)
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let removed = E::store(project)
            .remove(&self.id)
            .ok_or(CommandError::MissingEntity {
                kind: E::KIND,
                id: self.id,
            })?;
        self.removed = Some(removed);
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let entity = self
            .removed
            .take()
            .ok_or_else(|| CommandError::HistoryCorrupted("delete never applied".into()))?;
        if E::store(project).insert(self.id, entity).is_some() {
            return Err(CommandError::IdInUse {
                kind: E::KIND,
                id: self.id,
            });
        }
        Ok(())
    }
}

pub type PlaceComponentCommand = CreateCommand<Component>;
pub type DeleteComponentCommand = DeleteCommand<Component>;
pub type CreateWireCommand = CreateCommand<Wire>;
pub type DeleteWireCommand = DeleteCommand<Wire>;
pub type CreateTrackCommand = CreateCommand<Track>;
pub type DeleteTrackCommand = DeleteCommand<Track>;
pub type CreateViaCommand = CreateCommand<Via>;
pub type DeleteViaCommand = DeleteCommand<Via>;

/// Moves a component to an absolute position.
#[derive(Debug)]
pub struct MoveComponentCommand {
    id: Id,
    to: Point,
    from: Option<Point>,
}

impl MoveComponentCommand {
    pub fn new(id: Id, to: Point) -> Self {
        Self { id, to, from: None }
    }
}

impl Command for MoveComponentCommand {
    fn label(&self) -> String {
        "Move component".into()
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        buturecad_geometry::Point::try_new(self.to.x, self.to.y)
            .map_err(|_| crate::model::ModelError::OutOfRange)?;
        let c =
            project
                .schematic
                .components
                .get_mut(&self.id)
                .ok_or(CommandError::MissingEntity {
                    kind: Component::KIND,
                    id: self.id,
                })?;
        self.from = Some(std::mem::replace(&mut c.position, self.to));
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let from = self
            .from
            .take()
            .ok_or_else(|| CommandError::HistoryCorrupted("move never applied".into()))?;
        let c =
            project
                .schematic
                .components
                .get_mut(&self.id)
                .ok_or(CommandError::MissingEntity {
                    kind: Component::KIND,
                    id: self.id,
                })?;
        c.position = from;
        Ok(())
    }
}

/// Renames the project.
#[derive(Debug)]
pub struct RenameProjectCommand {
    to: String,
    from: Option<String>,
}

impl RenameProjectCommand {
    pub fn new(to: impl Into<String>) -> Self {
        Self {
            to: to.into(),
            from: None,
        }
    }
}

impl Command for RenameProjectCommand {
    fn label(&self) -> String {
        "Rename project".into()
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let old = project.name().to_owned();
        project.set_name(self.to.clone())?;
        self.from = Some(old);
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let from = self
            .from
            .take()
            .ok_or_else(|| CommandError::HistoryCorrupted("rename never applied".into()))?;
        project.set_name(from)?;
        Ok(())
    }
}

/// Several commands applied as one atomic, single-undo unit.
#[derive(Debug)]
pub struct CompositeCommand {
    label: String,
    commands: Vec<Box<dyn Command>>,
    applied: usize,
}

impl CompositeCommand {
    pub fn new(label: impl Into<String>, commands: Vec<Box<dyn Command>>) -> Self {
        let applied = commands.len();
        Self {
            label: label.into(),
            commands,
            applied,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Command for CompositeCommand {
    fn label(&self) -> String {
        self.label.clone()
    }
    /// Re-applies every child in order; if one fails, the already-applied ones are reverted.
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        self.applied = 0;
        for i in 0..self.commands.len() {
            if let Err(e) = self.commands[i].apply(project) {
                for done in self.commands[..i].iter_mut().rev() {
                    // Best effort: children just applied successfully, so reverting cannot fail
                    // unless the contract was violated; surface the original error either way.
                    let _ = done.revert(project);
                }
                return Err(e);
            }
            self.applied = i + 1;
        }
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        for i in (0..self.applied).rev() {
            self.commands[i].revert(project)?;
            self.applied = i;
        }
        Ok(())
    }
}
