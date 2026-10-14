//! The neutral mechanical model. Coordinates are integer **nanometres**, +Y up (as the PCB core);
//! rotations are millidegrees; scales parts-per-million.

use crate::backend::McadError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Version of the neutral exchange schema (`.bmech`).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pt {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountingHole {
    pub id: String,
    pub center: Pt,
    pub diameter: i64,
    pub plated: bool,
}

/// Reference to an external 3D model file (not loaded by this crate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub path: String,
    pub offset: [i64; 3],
    pub rotation_mdeg: [i32; 3],
    pub scale_ppm: [i32; 3],
}

/// Component geometry. An extruded footprint outline is the only geometry known to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    /// Outline (in board coordinates) extruded by `height` from the board surface.
    Prism { outline: Vec<Pt>, height: i64 },
    /// Geometry lives in an external model the footprint references.
    External(ModelRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub reference: String,
    pub footprint: String,
    pub position: Pt,
    pub rotation_mdeg: i32,
    pub side: Side,
    pub bodies: Vec<Body>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constraint {
    /// Nothing taller than `max` above the board surface inside `region` on `side`.
    MaxHeight {
        region: Vec<Pt>,
        side: Side,
        max: i64,
    },
    /// No component inside `region` on `side`.
    Keepout { region: Vec<Pt>, side: Side },
    /// The component must not move (mechanically locked).
    FixedComponent { reference: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MechanicalModel {
    pub schema_version: u32,
    pub name: String,
    pub board_thickness: i64,
    /// Loop 0 is the outer contour, the others are cut-outs.
    pub outline: Vec<Vec<Pt>>,
    pub holes: Vec<MountingHole>,
    pub components: Vec<Component>,
    pub constraints: Vec<Constraint>,
}

impl MechanicalModel {
    pub fn new(name: impl Into<String>, board_thickness: i64) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: name.into(),
            board_thickness,
            outline: Vec::new(),
            holes: Vec::new(),
            components: Vec::new(),
            constraints: Vec::new(),
        }
    }

    /// Structural validation (what an exchange must guarantee before anyone trusts the data).
    /// It does not check mechanical fit: holes are only required to lie within the bounding box of
    /// the outer contour.
    pub fn validate(&self) -> Result<(), McadError> {
        let bad = |m: String| Err(McadError::Invalid(m));
        if self.board_thickness <= 0 {
            return bad("board thickness must be positive".into());
        }
        let Some(outer) = self.outline.first() else {
            return bad("the board outline is empty".into());
        };
        for (i, l) in self.outline.iter().enumerate() {
            if l.len() < 3 {
                return bad(format!("outline loop {i} has fewer than 3 vertices"));
            }
        }
        let (lo, hi) = bbox(outer);
        let mut ids = BTreeSet::new();
        for h in &self.holes {
            if h.diameter <= 0 {
                return bad(format!("hole '{}' has a non-positive diameter", h.id));
            }
            if !ids.insert(h.id.as_str()) {
                return bad(format!("duplicate hole id '{}'", h.id));
            }
            let c = h.center;
            if c.x < lo.x || c.y < lo.y || c.x > hi.x || c.y > hi.y {
                return bad(format!(
                    "hole '{}' is outside the board outline bounds",
                    h.id
                ));
            }
        }
        let mut refs = BTreeSet::new();
        for c in &self.components {
            if !refs.insert(c.reference.as_str()) {
                return bad(format!("duplicate component reference '{}'", c.reference));
            }
            for b in &c.bodies {
                if let Body::Prism { outline, height } = b {
                    if outline.len() < 3 || *height <= 0 {
                        return bad(format!("component '{}' has a degenerate body", c.reference));
                    }
                }
            }
        }
        for k in &self.constraints {
            match k {
                Constraint::MaxHeight { region, max, .. } if region.len() < 3 || *max <= 0 => {
                    return bad("a max-height constraint is degenerate".into());
                }
                Constraint::Keepout { region, .. } if region.len() < 3 => {
                    return bad("a keepout constraint has fewer than 3 vertices".into());
                }
                Constraint::FixedComponent { reference } if !refs.contains(reference.as_str()) => {
                    return bad(format!(
                        "fixed constraint on unknown component '{reference}'"
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn bbox(pts: &[Pt]) -> (Pt, Pt) {
    let mut lo = pts[0];
    let mut hi = pts[0];
    for p in pts {
        lo = Pt {
            x: lo.x.min(p.x),
            y: lo.y.min(p.y),
        };
        hi = Pt {
            x: hi.x.max(p.x),
            y: hi.y.max(p.y),
        };
    }
    (lo, hi)
}

#[cfg(test)]
fn square() -> Vec<Pt> {
    [(0, 0), (10, 0), (10, 10), (0, 10)]
        .iter()
        .map(|&(x, y)| Pt { x, y })
        .collect()
}

#[cfg(test)]
pub(crate) fn sample_model() -> MechanicalModel {
    let mut m = MechanicalModel::new("demo", 1_600_000);
    m.outline.push(square());
    m.holes.push(MountingHole {
        id: "H1".into(),
        center: Pt { x: 5, y: 5 },
        diameter: 3,
        plated: false,
    });
    m.components.push(Component {
        reference: "U1".into(),
        footprint: "SOIC-8".into(),
        position: Pt { x: 5, y: 5 },
        rotation_mdeg: 90_000,
        side: Side::Top,
        bodies: vec![Body::Prism {
            outline: square(),
            height: 1_750_000,
        }],
    });
    m.constraints.push(Constraint::FixedComponent {
        reference: "U1".into(),
    });
    m
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::sample_model as sample;
    use super::*;

    #[test]
    fn sample_is_valid() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn rejects_bad_models() {
        let mut m = sample();
        m.holes[0].center = Pt { x: 99, y: 5 };
        assert!(m.validate().is_err());
        let mut m = sample();
        m.outline.clear();
        assert!(m.validate().is_err());
        let mut m = sample();
        m.constraints.push(Constraint::FixedComponent {
            reference: "X9".into(),
        });
        assert!(m.validate().is_err());
        let mut m = sample();
        m.components.push(m.components[0].clone());
        assert!(m.validate().is_err());
        let mut m = sample();
        m.board_thickness = 0;
        assert!(m.validate().is_err());
    }
}
