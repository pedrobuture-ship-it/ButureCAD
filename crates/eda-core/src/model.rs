//! PCB design model hosting the command infrastructure.
//!
//! **Incomplete by design.** The schematic model lives in `buturecad-schematic`. The PCB entities
//! here carry only what the Command Engine needs today (identity, geometry, validated attributes);
//! footprints, pads, nets and zones arrive with the PCB core stage and will move into the `pcb`
//! crate; the command infrastructure does not change when they do.

use buturecad_geometry::{Coord, Point, Segment};
use buturecad_pcb::{
    layer, BoardGraphic, BoardText, Footprint, Keepout, Net, PlacedFootprint, Stackup, Zone,
};
pub use buturecad_schematic::Id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub use buturecad_pcb::LayerId;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ModelError {
    #[error("track width must be positive")]
    BadTrackWidth,
    #[error("track must have distinct endpoints")]
    ZeroLengthTrack,
    #[error("via drill must be positive and smaller than the via diameter")]
    BadViaSize,
    #[error("coordinate is outside the supported range")]
    OutOfRange,
    #[error("footprint {0:?} does not match the state the command was built from (stale edit)")]
    FootprintChanged(String),
    #[error("invalid footprint: {0}")]
    BadFootprint(String),
    #[error("invalid board object: {0}")]
    BadObject(String),
    #[error("layer {0:?} does not exist on this board")]
    NoSuchLayer(LayerId),
}

fn check_point(p: Point) -> Result<(), ModelError> {
    Point::try_new(p.x, p.y)
        .map(|_| ())
        .map_err(|_| ModelError::OutOfRange)
}

/// A PCB copper track segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub layer: LayerId,
    pub segment: Segment,
    pub width: Coord,
    /// The net the track carries (`None` = unassigned).
    #[serde(default)]
    pub net: Option<Id>,
}

/// A PCB via.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Via {
    pub position: Point,
    pub diameter: Coord,
    pub drill: Coord,
    #[serde(default)]
    pub net: Option<Id>,
    #[serde(default)]
    pub kind: ViaKind,
}

/// Via construction. Only through vias can be created today; the other kinds are part of the
/// model so boards and files do not need to change when they are implemented (roadmap 26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViaKind {
    /// Drilled through the whole board.
    #[default]
    Through,
    /// Connects an outer layer to an inner layer.
    Blind { from: LayerId, to: LayerId },
    /// Connects inner layers only.
    Buried { from: LayerId, to: LayerId },
    /// A laser-drilled via between adjacent layers.
    Micro { from: LayerId, to: LayerId },
}

impl ViaKind {
    /// The layers a via of this kind connects on a board with `stack`, in physical order.
    pub fn layers(&self, stack: &Stackup) -> Vec<LayerId> {
        let all = stack.copper();
        match self {
            ViaKind::Through => all,
            ViaKind::Blind { from, to }
            | ViaKind::Buried { from, to }
            | ViaKind::Micro { from, to } => {
                let pos = |l: &LayerId| all.iter().position(|x| x == l);
                match (pos(from), pos(to)) {
                    (Some(a), Some(b)) => all[a.min(b)..=a.max(b)].to_vec(),
                    _ => Vec::new(),
                }
            }
        }
    }
}

/// PCB document contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcbDoc {
    /// Copper layer count (2, 4 … 32).
    pub stackup: Stackup,
    pub tracks: BTreeMap<Id, Track>,
    pub vias: BTreeMap<Id, Via>,
    pub placed: BTreeMap<Id, PlacedFootprint>,
    pub nets: BTreeMap<Id, Net>,
    pub zones: BTreeMap<Id, Zone>,
    pub keepouts: BTreeMap<Id, Keepout>,
    pub texts: BTreeMap<Id, BoardText>,
    /// Graphics on any layer; those on `Edge.Cuts` form the board outline.
    pub graphics: BTreeMap<Id, BoardGraphic>,
    /// Footprint definitions embedded in the project (like the schematic's symbol table), keyed
    /// by `Library:Name`. Placing footprints on the board arrives with the PCB core stage.
    pub footprints: BTreeMap<String, Footprint>,
}

/// A design object that can be created and deleted through generic commands.
pub trait Entity: Clone + std::fmt::Debug + Send + 'static {
    /// Human-readable kind, used in labels and errors.
    const KIND: &'static str;
    fn validate(&self) -> Result<(), ModelError>;
    fn store(project: &mut crate::Project) -> &mut BTreeMap<Id, Self>;
}

impl Entity for Track {
    const KIND: &'static str = "track";
    fn validate(&self) -> Result<(), ModelError> {
        check_point(self.segment.a)?;
        check_point(self.segment.b)?;
        if self.width <= 0 {
            return Err(ModelError::BadTrackWidth);
        }
        if self.segment.a == self.segment.b {
            return Err(ModelError::ZeroLengthTrack);
        }
        Ok(())
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.pcb.tracks
    }
}

impl Entity for Via {
    const KIND: &'static str = "via";
    fn validate(&self) -> Result<(), ModelError> {
        check_point(self.position)?;
        if self.drill <= 0 || self.drill >= self.diameter {
            return Err(ModelError::BadViaSize);
        }
        Ok(())
    }
    fn store(p: &mut crate::Project) -> &mut BTreeMap<Id, Self> {
        &mut p.pcb.vias
    }
}

/// Any object of the PCB document, for generic reversible changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcbObject {
    Track(Track),
    Via(Via),
    Footprint(PlacedFootprint),
    Net(Net),
    Zone(Zone),
    Keepout(Keepout),
    Text(BoardText),
    Graphic(BoardGraphic),
}

impl PcbObject {
    pub fn kind(&self) -> &'static str {
        match self {
            PcbObject::Track(_) => "track",
            PcbObject::Via(_) => "via",
            PcbObject::Footprint(_) => "footprint",
            PcbObject::Net(_) => "net",
            PcbObject::Zone(_) => "zone",
            PcbObject::Keepout(_) => "keepout",
            PcbObject::Text(_) => "text",
            PcbObject::Graphic(_) => "graphic",
        }
    }
}

impl PcbDoc {
    pub fn object(&self, id: Id) -> Option<PcbObject> {
        self.tracks
            .get(&id)
            .cloned()
            .map(PcbObject::Track)
            .or_else(|| self.vias.get(&id).cloned().map(PcbObject::Via))
            .or_else(|| self.placed.get(&id).cloned().map(PcbObject::Footprint))
            .or_else(|| self.nets.get(&id).cloned().map(PcbObject::Net))
            .or_else(|| self.zones.get(&id).cloned().map(PcbObject::Zone))
            .or_else(|| self.keepouts.get(&id).cloned().map(PcbObject::Keepout))
            .or_else(|| self.texts.get(&id).cloned().map(PcbObject::Text))
            .or_else(|| self.graphics.get(&id).cloned().map(PcbObject::Graphic))
    }

    /// Every id used by a board object.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.tracks
            .keys()
            .chain(self.vias.keys())
            .chain(self.placed.keys())
            .chain(self.nets.keys())
            .chain(self.zones.keys())
            .chain(self.keepouts.keys())
            .chain(self.texts.keys())
            .chain(self.graphics.keys())
            .copied()
    }

    pub fn object_count(&self) -> usize {
        self.ids().count()
    }

    fn remove_object(&mut self, id: Id) -> Option<PcbObject> {
        let o = self.object(id)?;
        match o {
            PcbObject::Track(_) => self.tracks.remove(&id).map(drop),
            PcbObject::Via(_) => self.vias.remove(&id).map(drop),
            PcbObject::Footprint(_) => self.placed.remove(&id).map(drop),
            PcbObject::Net(_) => self.nets.remove(&id).map(drop),
            PcbObject::Zone(_) => self.zones.remove(&id).map(drop),
            PcbObject::Keepout(_) => self.keepouts.remove(&id).map(drop),
            PcbObject::Text(_) => self.texts.remove(&id).map(drop),
            PcbObject::Graphic(_) => self.graphics.remove(&id).map(drop),
        };
        Some(o)
    }

    fn put_object(&mut self, id: Id, o: PcbObject) {
        match o {
            PcbObject::Track(v) => self.tracks.insert(id, v).map(drop),
            PcbObject::Via(v) => self.vias.insert(id, v).map(drop),
            PcbObject::Footprint(v) => self.placed.insert(id, v).map(drop),
            PcbObject::Net(v) => self.nets.insert(id, v).map(drop),
            PcbObject::Zone(v) => self.zones.insert(id, v).map(drop),
            PcbObject::Keepout(v) => self.keepouts.insert(id, v).map(drop),
            PcbObject::Text(v) => self.texts.insert(id, v).map(drop),
            PcbObject::Graphic(v) => self.graphics.insert(id, v).map(drop),
        };
    }

    /// Validates one object against the document it would live in (layers exist, referenced
    /// footprints and nets exist).
    pub fn validate_object(&self, o: &PcbObject) -> Result<(), ModelError> {
        let bad = |e: buturecad_pcb::BoardError| ModelError::BadObject(e.to_string());
        let layer_ok = |l: LayerId, copper: bool| -> Result<(), ModelError> {
            let ok = if copper {
                self.stackup.has_copper(l)
            } else {
                layer::is_known(l) && (!layer::is_copper(l) || self.stackup.has_copper(l))
            };
            if ok {
                Ok(())
            } else {
                Err(ModelError::NoSuchLayer(l))
            }
        };
        let net_ok = |n: Option<Id>| -> Result<(), ModelError> {
            match n {
                Some(id) if !self.nets.contains_key(&id) => {
                    Err(ModelError::BadObject(format!("net {id:?} does not exist")))
                }
                _ => Ok(()),
            }
        };
        match o {
            PcbObject::Track(t) => {
                t.validate()?;
                layer_ok(t.layer, true)?;
                net_ok(t.net)
            }
            PcbObject::Via(v) => {
                v.validate()?;
                net_ok(v.net)?;
                for l in v.kind.layers(&self.stackup) {
                    layer_ok(l, true)?;
                }
                if !matches!(v.kind, ViaKind::Through) && v.kind.layers(&self.stackup).len() < 2 {
                    return Err(ModelError::BadObject(
                        "via layers are not on this board".into(),
                    ));
                }
                Ok(())
            }
            PcbObject::Footprint(f) => {
                f.validate().map_err(bad)?;
                if !self.footprints.contains_key(&f.footprint) {
                    return Err(ModelError::BadObject(format!(
                        "footprint {} is not in the project",
                        f.footprint
                    )));
                }
                for n in f.pad_nets.values() {
                    net_ok(Some(*n))?;
                }
                Ok(())
            }
            PcbObject::Net(n) => {
                if n.name.trim().is_empty() {
                    return Err(ModelError::BadObject("a net needs a name".into()));
                }
                Ok(())
            }
            PcbObject::Zone(z) => {
                z.validate().map_err(bad)?;
                layer_ok(z.layer, true)?;
                net_ok(z.net)
            }
            PcbObject::Keepout(k) => {
                k.validate().map_err(bad)?;
                k.layers.iter().try_for_each(|l| layer_ok(*l, true))
            }
            PcbObject::Text(t) => {
                t.validate().map_err(bad)?;
                layer_ok(t.layer, false)
            }
            PcbObject::Graphic(g) => {
                g.validate().map_err(bad)?;
                layer_ok(g.layer, false)
            }
        }
    }
}

/// One reversible change to a board object: its complete state before and after (`None` =
/// absent). Tools compute these; [`crate::commands::PcbEditCommand`] applies them atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcbChange {
    pub id: Id,
    pub before: Option<PcbObject>,
    pub after: Option<PcbObject>,
}

impl PcbChange {
    pub fn create(id: Id, o: PcbObject) -> Self {
        Self {
            id,
            before: None,
            after: Some(o),
        }
    }
    pub fn delete(id: Id, o: PcbObject) -> Self {
        Self {
            id,
            before: Some(o),
            after: None,
        }
    }
    pub fn modify(id: Id, before: PcbObject, after: PcbObject) -> Self {
        Self {
            id,
            before: Some(before),
            after: Some(after),
        }
    }
}

impl PcbDoc {
    /// Applies changes atomically: every `before` must match the current state, every `after`
    /// must validate; on error nothing changes.
    pub fn apply_changes(&mut self, changes: &[PcbChange]) -> Result<(), ModelError> {
        let snapshot_ids: Vec<(Id, Option<PcbObject>)> =
            changes.iter().map(|c| (c.id, self.object(c.id))).collect();
        let result = (|| {
            for c in changes {
                if self.object(c.id) != c.before {
                    return Err(ModelError::BadObject(format!(
                        "object {:?} changed since the edit was prepared",
                        c.id
                    )));
                }
                self.remove_object(c.id);
                if let Some(a) = &c.after {
                    self.validate_object(a)?;
                    self.put_object(c.id, a.clone());
                }
            }
            Ok(())
        })();
        if result.is_err() {
            // Restore the original state of every touched id (first snapshot wins).
            let mut seen = std::collections::BTreeSet::new();
            for (id, o) in snapshot_ids {
                if seen.insert(id) {
                    self.remove_object(id);
                    if let Some(o) = o {
                        self.put_object(id, o);
                    }
                }
            }
        }
        result
    }

    pub fn revert_changes(&mut self, changes: &[PcbChange]) -> Result<(), ModelError> {
        let inverse: Vec<PcbChange> = changes
            .iter()
            .rev()
            .map(|c| PcbChange {
                id: c.id,
                before: c.after.clone(),
                after: c.before.clone(),
            })
            .collect();
        self.apply_changes(&inverse)
    }
}
