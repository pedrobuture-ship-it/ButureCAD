//! Concrete commands.

use crate::command::{Command, CommandError};
use crate::model::{Entity, Id, Track, Via};
use crate::Project;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{
    Change, Component, Object, Placement, Schematic, SchematicError, Symbol, Waiver, Wire,
};

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

pub type CreateTrackCommand = CreateCommand<Track>;
pub type DeleteTrackCommand = DeleteCommand<Track>;
pub type CreateViaCommand = CreateCommand<Via>;
pub type DeleteViaCommand = DeleteCommand<Via>;

/// Applies a set of reversible schematic object changes as one atomic step.
///
/// Tools compute [`Change`]s (full before/after object states) and wrap them here, which keeps
/// undo/redo trivially exact and makes every edit reviewable as a diff.
#[derive(Debug)]
pub struct SchematicEditCommand {
    label: String,
    changes: Vec<Change>,
}

impl SchematicEditCommand {
    pub fn new(label: impl Into<String>, changes: Vec<Change>) -> Self {
        Self {
            label: label.into(),
            changes,
        }
    }
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

impl Command for SchematicEditCommand {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        Ok(project.schematic.apply(&self.changes)?)
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        Ok(project.schematic.revert(&self.changes)?)
    }
}

/// Adds library symbols to the project's embedded symbol table (symbols already present are left
/// untouched). Needed before placing parts that use them.
#[derive(Debug)]
pub struct AddSymbolsCommand {
    symbols: Vec<Symbol>,
    added: Vec<String>,
}

impl AddSymbolsCommand {
    pub fn new(symbols: Vec<Symbol>) -> Self {
        Self {
            symbols,
            added: Vec::new(),
        }
    }
}

impl Command for AddSymbolsCommand {
    fn label(&self) -> String {
        "Add symbols".into()
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        self.added.clear();
        for s in &self.symbols {
            if project.schematic.symbol(&s.name).is_none() {
                project.schematic.add_symbol(s.clone());
                self.added.push(s.name.clone());
            }
        }
        Ok(())
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        for name in self.added.drain(..).rev() {
            project.schematic.remove_symbol(&name)?;
        }
        Ok(())
    }
}

/// Creates, replaces or deletes one symbol in the project's embedded symbol table (the Symbol
/// Editor's unit of work). `before` / `after` are the complete states; `None` means absent.
/// Deleting a symbol that placed objects still use is refused.
#[derive(Debug)]
pub struct SetSymbolCommand {
    name: String,
    before: Option<Symbol>,
    after: Option<Symbol>,
    label: Option<String>,
}

impl SetSymbolCommand {
    pub fn new(name: impl Into<String>, before: Option<Symbol>, after: Option<Symbol>) -> Self {
        Self {
            name: name.into(),
            before,
            after,
            label: None,
        }
    }

    /// Overrides the history label ("Move pins", …).
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn set(project: &mut Project, name: &str, state: &Option<Symbol>) -> Result<(), CommandError> {
        match state {
            Some(sym) => {
                project.schematic.check_symbol_uses(sym)?;
                project.schematic.add_symbol(sym.clone());
                Ok(())
            }
            None => project
                .schematic
                .remove_symbol(name)
                .map(|_| ())
                .map_err(Into::into),
        }
    }
}

impl Command for SetSymbolCommand {
    fn label(&self) -> String {
        if let Some(l) = &self.label {
            return l.clone();
        }
        match (&self.before, &self.after) {
            (None, Some(_)) => format!("Create symbol {}", self.name),
            (Some(_), None) => format!("Delete symbol {}", self.name),
            _ => format!("Edit symbol {}", self.name),
        }
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        if project.schematic.symbol(&self.name) != self.before.as_ref() {
            return Err(SchematicError::UnknownSymbol(self.name.clone()).into());
        }
        if let Some(a) = &self.after {
            if a.name != self.name {
                return Err(SchematicError::UnknownSymbol(a.name.clone()).into());
            }
        }
        Self::set(project, &self.name, &self.after)
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        Self::set(project, &self.name, &self.before)
    }
}

/// Replaces the ERC waiver list (add, edit or remove a documented exception).
#[derive(Debug)]
pub struct SetWaiversCommand {
    before: Vec<Waiver>,
    after: Vec<Waiver>,
    label: String,
}

impl SetWaiversCommand {
    pub fn new(label: impl Into<String>, before: Vec<Waiver>, after: Vec<Waiver>) -> Self {
        Self {
            before,
            after,
            label: label.into(),
        }
    }
}

impl Command for SetWaiversCommand {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project
            .schematic
            .set_waivers(self.after.clone())
            .map_err(Into::into)
    }
    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project
            .schematic
            .set_waivers(self.before.clone())
            .map_err(Into::into)
    }
}

/// Creates one schematic object.
pub fn create_object(label: &str, sheet: Id, id: Id, object: Object) -> SchematicEditCommand {
    SchematicEditCommand::new(label, vec![Change::create(sheet, id, object)])
}

/// Places a component instance.
pub fn place_component(sheet: Id, id: Id, component: Component) -> SchematicEditCommand {
    create_object("Place component", sheet, id, Object::Component(component))
}

/// Draws a wire segment.
pub fn create_wire(sheet: Id, id: Id, segment: Segment) -> SchematicEditCommand {
    create_object("Create wire", sheet, id, Object::Wire(Wire { segment }))
}

/// Deletes objects (the current state is captured from `schematic` so undo restores it exactly).
pub fn delete_objects(
    schematic: &Schematic,
    ids: &[Id],
) -> Result<SchematicEditCommand, CommandError> {
    let mut changes = Vec::with_capacity(ids.len());
    for id in ids {
        let (sheet, obj) = schematic
            .object(*id)
            .ok_or(SchematicError::NoSuchObject(*id))?;
        changes.push(Change::delete(sheet, *id, obj.clone()));
    }
    Ok(SchematicEditCommand::new(
        if ids.len() == 1 {
            "Delete object"
        } else {
            "Delete objects"
        },
        changes,
    ))
}

/// Moves a component to an absolute position, keeping its rotation and mirror.
pub fn move_component(
    schematic: &Schematic,
    id: Id,
    to: Point,
) -> Result<SchematicEditCommand, CommandError> {
    match schematic.object(id) {
        Some((sheet, Object::Component(c))) => {
            let mut moved = c.clone();
            moved.placement = Placement {
                position: to,
                ..c.placement
            };
            Ok(SchematicEditCommand::new(
                "Move component",
                vec![Change::modify(
                    sheet,
                    id,
                    Object::Component(c.clone()),
                    Object::Component(moved),
                )],
            ))
        }
        _ => Err(SchematicError::NoSuchObject(id).into()),
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
