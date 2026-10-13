//! The schematic container: sheets, objects, the embedded symbol table, validation and
//! reversible change sets.

use crate::model::*;
use buturecad_geometry::{Bounded, BoundingBox, Point, Vector};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum SchematicError {
    #[error("id {0:?} is already in use")]
    DuplicateId(Id),
    #[error("sheet {0:?} does not exist")]
    NoSuchSheet(Id),
    #[error("object {0:?} does not exist")]
    NoSuchObject(Id),
    #[error("symbol {0:?} is not in the schematic's symbol table")]
    UnknownSymbol(String),
    #[error("{0} must have distinct endpoints")]
    ZeroLength(&'static str),
    #[error("{0} must have a non-empty name")]
    EmptyName(&'static str),
    #[error("component rotation must be 0..=3 quarter turns, got {0}")]
    BadRotation(u8),
    #[error("a coordinate is outside the supported range")]
    OutOfRange,
    #[error("symbol {0:?} is not a power symbol")]
    NotPowerSymbol(String),
    #[error("sheet {0:?} is already instantiated; each sheet may be placed once")]
    SheetReused(Id),
    #[error("sheet hierarchy would contain a cycle through sheet {0:?}")]
    SheetCycle(Id),
    #[error("root sheet {0:?} is missing")]
    NoRoot(Id),
    #[error("symbol {0:?} is still used by placed objects")]
    SymbolInUse(String),
    #[error("change does not match the current state of object {0:?} (stale change)")]
    StaleChange(Id),
}

/// A pin of a component resolved to sheet coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedPin {
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub position: Point,
    pub orientation: Direction,
}

/// One reversible edit of one object: `before`/`after` are the full object states
/// (`None` = absent). Tools produce change sets; the Command Engine applies and reverts them.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub sheet: Id,
    pub id: Id,
    pub before: Option<Object>,
    pub after: Option<Object>,
}

impl Change {
    pub fn create(sheet: Id, id: Id, object: Object) -> Self {
        Self {
            sheet,
            id,
            before: None,
            after: Some(object),
        }
    }
    pub fn delete(sheet: Id, id: Id, object: Object) -> Self {
        Self {
            sheet,
            id,
            before: Some(object),
            after: None,
        }
    }
    pub fn modify(sheet: Id, id: Id, before: Object, after: Object) -> Self {
        Self {
            sheet,
            id,
            before: Some(before),
            after: Some(after),
        }
    }
}

/// The schematic of a project.
#[derive(Debug, Clone)]
pub struct Schematic {
    symbols: BTreeMap<String, Symbol>,
    sheets: BTreeMap<Id, Sheet>,
    root: Id,
    /// object id → sheet id
    index: BTreeMap<Id, Id>,
}

impl PartialEq for Schematic {
    fn eq(&self, o: &Self) -> bool {
        self.symbols == o.symbols && self.sheets == o.sheets && self.root == o.root
    }
}

impl Schematic {
    /// An empty schematic with one root sheet.
    pub fn new(root: Id) -> Self {
        let mut sheets = BTreeMap::new();
        sheets.insert(
            root,
            Sheet {
                name: "Root".into(),
                objects: BTreeMap::new(),
            },
        );
        Self {
            symbols: BTreeMap::new(),
            sheets,
            root,
            index: BTreeMap::new(),
        }
    }

    /// Builds from loaded parts and validates everything (untrusted input).
    pub fn from_parts(
        root: Id,
        symbols: BTreeMap<String, Symbol>,
        sheets: BTreeMap<Id, Sheet>,
    ) -> Result<Self, SchematicError> {
        let mut s = Self {
            symbols,
            sheets,
            root,
            index: BTreeMap::new(),
        };
        let mut seen: BTreeSet<Id> = s.sheets.keys().copied().collect();
        if seen.len() != s.sheets.len() || !s.sheets.contains_key(&root) {
            return Err(SchematicError::NoRoot(root));
        }
        for (sid, sheet) in &s.sheets {
            for oid in sheet.objects.keys() {
                if !seen.insert(*oid) {
                    return Err(SchematicError::DuplicateId(*oid));
                }
                s.index.insert(*oid, *sid);
            }
        }
        s.validate()?;
        Ok(s)
    }

    pub fn root(&self) -> Id {
        self.root
    }
    pub fn symbols(&self) -> &BTreeMap<String, Symbol> {
        &self.symbols
    }
    pub fn sheets(&self) -> &BTreeMap<Id, Sheet> {
        &self.sheets
    }
    pub fn sheet(&self, id: Id) -> Option<&Sheet> {
        self.sheets.get(&id)
    }
    pub fn symbol(&self, lib_id: &str) -> Option<&Symbol> {
        self.symbols.get(lib_id)
    }

    /// Adds or replaces a library symbol in the embedded symbol table.
    pub fn add_symbol(&mut self, symbol: Symbol) {
        self.symbols.insert(symbol.name.clone(), symbol);
    }

    /// Removes a symbol that no placed object uses.
    pub fn remove_symbol(&mut self, lib_id: &str) -> Result<Symbol, SchematicError> {
        let used = self
            .sheets
            .values()
            .flat_map(|s| s.objects.values())
            .any(|o| match o {
                Object::Component(c) => c.lib_id == lib_id,
                Object::PowerSymbol(p) => p.lib_id == lib_id,
                _ => false,
            });
        if used {
            return Err(SchematicError::SymbolInUse(lib_id.to_owned()));
        }
        self.symbols
            .remove(lib_id)
            .ok_or_else(|| SchematicError::UnknownSymbol(lib_id.to_owned()))
    }

    /// Adds an empty child sheet.
    pub fn add_sheet(&mut self, id: Id, name: impl Into<String>) -> Result<(), SchematicError> {
        if self.sheets.contains_key(&id) || self.index.contains_key(&id) {
            return Err(SchematicError::DuplicateId(id));
        }
        self.sheets.insert(
            id,
            Sheet {
                name: name.into(),
                objects: BTreeMap::new(),
            },
        );
        Ok(())
    }

    /// Looks an object up: `(sheet id, object)`.
    pub fn object(&self, id: Id) -> Option<(Id, &Object)> {
        let sheet = *self.index.get(&id)?;
        self.sheets
            .get(&sheet)?
            .objects
            .get(&id)
            .map(|o| (sheet, o))
    }

    pub fn sheet_of(&self, id: Id) -> Option<Id> {
        self.index.get(&id).copied()
    }

    pub fn object_count(&self) -> usize {
        self.index.len()
    }

    /// Inserts a new object after validating it.
    pub fn insert(&mut self, sheet: Id, id: Id, object: Object) -> Result<(), SchematicError> {
        if self.index.contains_key(&id) || self.sheets.contains_key(&id) {
            return Err(SchematicError::DuplicateId(id));
        }
        if !self.sheets.contains_key(&sheet) {
            return Err(SchematicError::NoSuchSheet(sheet));
        }
        self.validate_object(sheet, id, &object)?;
        if let Some(s) = self.sheets.get_mut(&sheet) {
            s.objects.insert(id, object);
        }
        self.index.insert(id, sheet);
        Ok(())
    }

    pub fn remove(&mut self, id: Id) -> Result<Object, SchematicError> {
        let sheet = self
            .index
            .remove(&id)
            .ok_or(SchematicError::NoSuchObject(id))?;
        self.sheets
            .get_mut(&sheet)
            .and_then(|s| s.objects.remove(&id))
            .ok_or(SchematicError::NoSuchObject(id))
    }

    /// Replaces an existing object after validating the new state.
    pub fn replace(&mut self, id: Id, object: Object) -> Result<Object, SchematicError> {
        let sheet = self.sheet_of(id).ok_or(SchematicError::NoSuchObject(id))?;
        self.validate_object(sheet, id, &object)?;
        let slot = self
            .sheets
            .get_mut(&sheet)
            .and_then(|s| s.objects.get_mut(&id))
            .ok_or(SchematicError::NoSuchObject(id))?;
        Ok(std::mem::replace(slot, object))
    }

    /// Applies a change set atomically: every `before` must match the current state, and on any
    /// failure the already-applied changes are undone.
    pub fn apply(&mut self, changes: &[Change]) -> Result<(), SchematicError> {
        for (i, c) in changes.iter().enumerate() {
            if let Err(e) = self.apply_one(c) {
                for done in changes[..i].iter().rev() {
                    // Reverting a change that was just applied cannot fail unless the model
                    // was mutated concurrently, which `&mut self` rules out.
                    let _ = self.revert_one(done);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Undoes a previously applied change set.
    pub fn revert(&mut self, changes: &[Change]) -> Result<(), SchematicError> {
        for (i, c) in changes.iter().enumerate().rev() {
            if let Err(e) = self.revert_one(c) {
                for done in changes[i + 1..].iter() {
                    let _ = self.apply_one(done);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    fn apply_one(&mut self, c: &Change) -> Result<(), SchematicError> {
        self.transition(c.sheet, c.id, c.before.as_ref(), c.after.as_ref())
    }

    fn revert_one(&mut self, c: &Change) -> Result<(), SchematicError> {
        self.transition(c.sheet, c.id, c.after.as_ref(), c.before.as_ref())
    }

    fn transition(
        &mut self,
        sheet: Id,
        id: Id,
        from: Option<&Object>,
        to: Option<&Object>,
    ) -> Result<(), SchematicError> {
        let current = self.object(id).map(|(_, o)| o);
        if current != from {
            return Err(SchematicError::StaleChange(id));
        }
        match (from, to) {
            (None, Some(o)) => self.insert(sheet, id, o.clone()),
            (Some(_), None) => self.remove(id).map(|_| ()),
            (Some(_), Some(o)) => self.replace(id, o.clone()).map(|_| ()),
            (None, None) => Ok(()),
        }
    }

    /// Validates a single object in the context of `sheet`.
    pub fn validate_object(
        &self,
        sheet: Id,
        id: Id,
        object: &Object,
    ) -> Result<(), SchematicError> {
        let pt = |p: Point| {
            Point::try_new(p.x, p.y)
                .map(|_| ())
                .map_err(|_| SchematicError::OutOfRange)
        };
        let seg = |s: &buturecad_geometry::Segment, what: &'static str| {
            pt(s.a)?;
            pt(s.b)?;
            if s.a == s.b {
                return Err(SchematicError::ZeroLength(what));
            }
            Ok(())
        };
        let name = |n: &str, what: &'static str| {
            if n.trim().is_empty() {
                Err(SchematicError::EmptyName(what))
            } else {
                Ok(())
            }
        };
        match object {
            Object::Component(c) => {
                if c.placement.rotation > 3 {
                    return Err(SchematicError::BadRotation(c.placement.rotation));
                }
                if !c.lib_id.is_empty() && !self.symbols.contains_key(&c.lib_id) {
                    return Err(SchematicError::UnknownSymbol(c.lib_id.clone()));
                }
                name(&c.reference, "component reference")?;
                pt(c.placement.position)
            }
            Object::Wire(w) => seg(&w.segment, "wire"),
            Object::Bus(b) => seg(&b.segment, "bus"),
            Object::Junction(j) => pt(j.position),
            Object::NoConnect(n) => pt(n.position),
            Object::NetLabel(l) => name(&l.name, "net label").and_then(|()| pt(l.position)),
            Object::GlobalLabel(l) => name(&l.name, "global label").and_then(|()| pt(l.position)),
            Object::HierLabel(l) => {
                name(&l.name, "hierarchical label").and_then(|()| pt(l.position))
            }
            Object::PowerSymbol(p) => {
                if p.placement.rotation > 3 {
                    return Err(SchematicError::BadRotation(p.placement.rotation));
                }
                let sym = self
                    .symbols
                    .get(&p.lib_id)
                    .ok_or_else(|| SchematicError::UnknownSymbol(p.lib_id.clone()))?;
                if !sym.is_power {
                    return Err(SchematicError::NotPowerSymbol(p.lib_id.clone()));
                }
                name(&p.net_name, "power symbol net")?;
                pt(p.placement.position)
            }
            Object::Sheet(s) => {
                for p in &s.pins {
                    name(&p.name, "sheet pin")?;
                    pt(p.position)?;
                }
                if !self.sheets.contains_key(&s.child) {
                    return Err(SchematicError::NoSuchSheet(s.child));
                }
                if s.child == sheet
                    || self.descendants(s.child).contains(&sheet)
                    || s.child == self.root
                {
                    return Err(SchematicError::SheetCycle(s.child));
                }
                let reused =
                    self.sheets
                        .values()
                        .flat_map(|sh| sh.objects.iter())
                        .any(|(oid, o)| {
                            *oid != id
                                && matches!(o, Object::Sheet(other) if other.child == s.child)
                        });
                if reused {
                    return Err(SchematicError::SheetReused(s.child));
                }
                Ok(())
            }
        }
    }

    /// Whole-model validation (used when loading untrusted files).
    pub fn validate(&self) -> Result<(), SchematicError> {
        if !self.sheets.contains_key(&self.root) {
            return Err(SchematicError::NoRoot(self.root));
        }
        for (sid, sheet) in &self.sheets {
            for (oid, obj) in &sheet.objects {
                self.validate_object(*sid, *oid, obj)?;
            }
        }
        Ok(())
    }

    /// Sheet ids reachable below `sheet` (excluding itself), following sheet instances.
    pub fn descendants(&self, sheet: Id) -> BTreeSet<Id> {
        let mut out = BTreeSet::new();
        let mut stack = vec![sheet];
        while let Some(s) = stack.pop() {
            for obj in self
                .sheets
                .get(&s)
                .into_iter()
                .flat_map(|sh| sh.objects.values())
            {
                if let Object::Sheet(inst) = obj {
                    if out.insert(inst.child) {
                        stack.push(inst.child);
                    }
                }
            }
        }
        out
    }

    /// Pins of `component` in sheet coordinates. A component without a symbol has no pins.
    pub fn placed_pins(&self, component: &Component) -> Vec<PlacedPin> {
        let Some(sym) = self.symbols.get(&component.lib_id) else {
            return Vec::new();
        };
        sym.pins
            .iter()
            .map(|p| PlacedPin {
                number: p.number.clone(),
                name: p.name.clone(),
                kind: p.kind,
                position: component.placement.apply(p.position),
                orientation: component.placement.apply_direction(p.orientation),
            })
            .collect()
    }

    /// Connection point of a power symbol (its first pin).
    pub fn power_pin_position(&self, ps: &PowerSymbol) -> Option<Point> {
        let sym = self.symbols.get(&ps.lib_id)?;
        sym.pins.first().map(|p| ps.placement.apply(p.position))
    }

    /// Next free reference for `prefix` (`R` → `R1`, `R2`, … filling the lowest unused number).
    pub fn next_reference(&self, prefix: &str) -> String {
        let used: BTreeSet<u64> = self
            .sheets
            .values()
            .flat_map(|s| s.objects.values())
            .filter_map(|o| match o {
                Object::Component(c) => c.reference.strip_prefix(prefix)?.parse::<u64>().ok(),
                _ => None,
            })
            .collect();
        let n = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        format!("{prefix}{n}")
    }

    /// Bounding box of an object (sheet coordinates), including symbol graphics and pins.
    pub fn object_bounds(&self, object: &Object) -> BoundingBox {
        let around =
            |p: Point, r: i64| BoundingBox::new(p - Vector::new(r, r), p + Vector::new(r, r));
        match object {
            Object::Wire(w) => w.segment.bounding_box(),
            Object::Bus(b) => b.segment.bounding_box(),
            Object::Junction(j) => around(j.position, 400_000),
            Object::NoConnect(n) => around(n.position, 600_000),
            Object::NetLabel(l) => text_box(l.position, &l.name),
            Object::GlobalLabel(l) => text_box(l.position, &l.name),
            Object::HierLabel(l) => text_box(l.position, &l.name),
            Object::Sheet(s) => s.rect.bounding_box(),
            Object::Component(c) => self
                .symbol_bounds(&c.lib_id, &c.placement)
                .unwrap_or_else(|| around(c.placement.position, 5_080_000)),
            Object::PowerSymbol(p) => self
                .symbol_bounds(&p.lib_id, &p.placement)
                .unwrap_or_else(|| around(p.placement.position, 2_540_000)),
        }
    }

    fn symbol_bounds(&self, lib_id: &str, placement: &Placement) -> Option<BoundingBox> {
        let sym = self.symbols.get(lib_id)?;
        let mut pts: Vec<Point> = sym.pins.iter().map(|p| p.position).collect();
        for g in &sym.graphics {
            match g {
                Graphic::Line { segment, .. } => pts.extend([segment.a, segment.b]),
                Graphic::Rect { rect, .. } => pts.extend([rect.min(), rect.max()]),
                Graphic::Circle { circle, .. } => {
                    let bb = circle.bounding_box();
                    pts.extend([bb.min, bb.max]);
                }
                Graphic::Arc { arc, .. } => {
                    let bb = arc.bounding_box();
                    pts.extend([bb.min, bb.max]);
                }
                Graphic::Text { origin, .. } => pts.push(*origin),
            }
        }
        BoundingBox::from_points(pts.into_iter().map(|p| placement.apply(p)))
    }
}

fn text_box(at: Point, text: &str) -> BoundingBox {
    // 1.27 mm text: 6 font pixels of 0.18 mm per character.
    let w = text.chars().count() as i64 * 1_000_000;
    BoundingBox::new(at, at + Vector::new(w, 1_270_000))
}
