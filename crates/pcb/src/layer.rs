//! Board layers and the layer stack.
//!
//! A [`LayerId`] is a small integer. Copper layers keep the numbering the project already used:
//! `0` = front copper, `1` = back copper, `2..=31` = inner copper 1…30. Technical layers (mask,
//! paste, silkscreen, fabrication, courtyard, edge cuts, mechanical, user) have fixed ids from
//! 100. The id says nothing about physical order: the [`Stackup`] does.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u16);

pub const F_CU: LayerId = LayerId(0);
pub const B_CU: LayerId = LayerId(1);
pub const F_MASK: LayerId = LayerId(100);
pub const B_MASK: LayerId = LayerId(101);
pub const F_PASTE: LayerId = LayerId(102);
pub const B_PASTE: LayerId = LayerId(103);
pub const F_SILK: LayerId = LayerId(104);
pub const B_SILK: LayerId = LayerId(105);
pub const F_FAB: LayerId = LayerId(106);
pub const B_FAB: LayerId = LayerId(107);
pub const F_CRTYD: LayerId = LayerId(108);
pub const B_CRTYD: LayerId = LayerId(109);
pub const EDGE_CUTS: LayerId = LayerId(110);
/// First of 8 mechanical layers (`Mechanical 1…8`).
pub const MECH_BASE: u16 = 120;
/// First of 8 user layers (`User 1…8`).
pub const USER_BASE: u16 = 130;

/// Most copper layers a board may have.
pub const MAX_COPPER: u8 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayerKind {
    Copper,
    Mask,
    Paste,
    Silkscreen,
    Fabrication,
    Courtyard,
    EdgeCuts,
    Mechanical,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
    /// Inner copper, or a layer without a side.
    None,
}

/// Inner copper layer `n` (1-based).
pub fn inner_cu(n: u8) -> LayerId {
    LayerId(1 + u16::from(n))
}

pub fn mechanical(n: u8) -> LayerId {
    LayerId(MECH_BASE + u16::from(n.saturating_sub(1)))
}

pub fn user(n: u8) -> LayerId {
    LayerId(USER_BASE + u16::from(n.saturating_sub(1)))
}

/// Is `id` one of the defined layers?
pub fn is_known(id: LayerId) -> bool {
    kind(id).is_some()
}

pub fn kind(id: LayerId) -> Option<LayerKind> {
    Some(match id.0 {
        0..=31 => LayerKind::Copper,
        100 | 101 => LayerKind::Mask,
        102 | 103 => LayerKind::Paste,
        104 | 105 => LayerKind::Silkscreen,
        106 | 107 => LayerKind::Fabrication,
        108 | 109 => LayerKind::Courtyard,
        110 => LayerKind::EdgeCuts,
        120..=127 => LayerKind::Mechanical,
        130..=137 => LayerKind::User,
        _ => return None,
    })
}

pub fn is_copper(id: LayerId) -> bool {
    id.0 <= 31
}

pub fn side(id: LayerId) -> Side {
    match id.0 {
        0 | 100 | 102 | 104 | 106 | 108 => Side::Front,
        1 | 101 | 103 | 105 | 107 | 109 => Side::Back,
        _ => Side::None,
    }
}

pub fn name(id: LayerId) -> String {
    match id.0 {
        0 => "F.Cu".into(),
        1 => "B.Cu".into(),
        n @ 2..=31 => format!("In{}.Cu", n - 1),
        100 => "F.Mask".into(),
        101 => "B.Mask".into(),
        102 => "F.Paste".into(),
        103 => "B.Paste".into(),
        104 => "F.Silkscreen".into(),
        105 => "B.Silkscreen".into(),
        106 => "F.Fab".into(),
        107 => "B.Fab".into(),
        108 => "F.Courtyard".into(),
        109 => "B.Courtyard".into(),
        110 => "Edge.Cuts".into(),
        n @ 120..=127 => format!("Mechanical {}", n - MECH_BASE + 1),
        n @ 130..=137 => format!("User {}", n - USER_BASE + 1),
        n => format!("Layer {n}"),
    }
}

/// The same layer on the other side (copper, mask, paste, silkscreen, fab, courtyard); other
/// layers map to themselves. Inner copper maps to itself.
pub fn flipped(id: LayerId) -> LayerId {
    match id.0 {
        0 => B_CU,
        1 => F_CU,
        n @ (100 | 102 | 104 | 106 | 108) => LayerId(n + 1),
        n @ (101 | 103 | 105 | 107 | 109) => LayerId(n - 1),
        _ => id,
    }
}

/// The copper layers of a board, front to back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stackup {
    /// Total copper layers: 2, 4, 6 … 32.
    pub copper_layers: u8,
}

impl Default for Stackup {
    fn default() -> Self {
        Self { copper_layers: 2 }
    }
}

impl Stackup {
    pub fn is_valid(&self) -> bool {
        (2..=MAX_COPPER).contains(&self.copper_layers) && self.copper_layers & 1 == 0
    }

    /// Copper layers in physical order: F.Cu, In1.Cu … InN.Cu, B.Cu.
    pub fn copper(&self) -> Vec<LayerId> {
        let inner = self.copper_layers.saturating_sub(2);
        let mut v = vec![F_CU];
        v.extend((1..=inner).map(inner_cu));
        v.push(B_CU);
        v
    }

    pub fn has_copper(&self, id: LayerId) -> bool {
        self.copper().contains(&id)
    }

    /// The copper layer after (`step` = 1) or before (`-1`) `id` in physical order, wrapping.
    pub fn next_copper(&self, id: LayerId, step: i32) -> LayerId {
        let c = self.copper();
        let i = c.iter().position(|l| *l == id).unwrap_or(0) as i32;
        c[(i + step).rem_euclid(c.len() as i32) as usize]
    }

    /// Every layer a board of this stackup shows, in drawing order (bottom to top).
    pub fn all_layers(&self) -> Vec<LayerId> {
        let mut v: Vec<LayerId> = (1..=8).map(user).collect();
        v.extend((1..=8).map(mechanical));
        v.extend([
            EDGE_CUTS, B_CRTYD, F_CRTYD, B_FAB, F_FAB, B_PASTE, F_PASTE, B_MASK, F_MASK,
        ]);
        v.extend(self.copper().into_iter().rev());
        v.extend([B_SILK, F_SILK]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stackup_orders_and_wraps() {
        let s = Stackup { copper_layers: 4 };
        assert!(s.is_valid());
        assert_eq!(s.copper(), [F_CU, inner_cu(1), inner_cu(2), B_CU]);
        assert_eq!(s.next_copper(F_CU, 1), inner_cu(1));
        assert_eq!(s.next_copper(B_CU, 1), F_CU);
        assert_eq!(s.next_copper(F_CU, -1), B_CU);
        assert!(!Stackup { copper_layers: 3 }.is_valid());
        assert!(!Stackup { copper_layers: 34 }.is_valid());
        assert_eq!(Stackup::default().copper(), [F_CU, B_CU]);
    }

    #[test]
    fn names_kinds_and_flipping() {
        assert_eq!(name(F_CU), "F.Cu");
        assert_eq!(name(inner_cu(3)), "In3.Cu");
        assert_eq!(name(user(2)), "User 2");
        assert_eq!(kind(EDGE_CUTS), Some(LayerKind::EdgeCuts));
        assert_eq!(kind(LayerId(50)), None);
        for l in [F_CU, F_MASK, F_PASTE, F_SILK, F_FAB, F_CRTYD, B_SILK] {
            assert_eq!(flipped(flipped(l)), l);
        }
        assert_eq!(flipped(EDGE_CUTS), EDGE_CUTS);
        assert_eq!(side(F_SILK), Side::Front);
        assert_eq!(side(B_CU), Side::Back);
        assert_eq!(side(inner_cu(1)), Side::None);
        let all = Stackup { copper_layers: 6 }.all_layers();
        assert!(all.iter().all(|l| is_known(*l)));
        assert_eq!(all.iter().filter(|l| is_copper(**l)).count(), 6);
    }
}
