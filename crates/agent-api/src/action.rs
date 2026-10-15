//! The vocabulary of proposed changes. Lengths are integer nanometres, angles degrees; objects
//! are named the way engineers name them (references, net names), never by internal file paths.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinSpec {
    pub reference: String,
    /// Pin number or name.
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Adds a schematic component; the footprint appears on the board when it is synced.
    CreateComponent {
        /// Wanted reference; chosen from the symbol's prefix when absent.
        #[serde(default)]
        reference: Option<String>,
        /// Symbol id (`Buture:C`).
        symbol: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        footprint: String,
        #[serde(default)]
        properties: BTreeMap<String, String>,
    },
    RemoveComponent {
        reference: String,
    },
    /// Puts the pins on net `net` (a power rail name or any net name).
    Connect {
        net: String,
        pins: Vec<PinSpec>,
    },
    /// Removes the net label / power symbol sitting on the pin.
    Disconnect {
        reference: String,
        pin: String,
    },
    /// Absolute board placement.
    Place {
        reference: String,
        x: i64,
        y: i64,
        #[serde(default)]
        rotation_deg: i32,
        #[serde(default)]
        bottom: bool,
    },
    /// Relative board move.
    Move {
        reference: String,
        dx: i64,
        dy: i64,
    },
    Route {
        net: String,
        layer: String,
        points: Vec<[i64; 2]>,
        width: i64,
    },
    CreateVia {
        net: String,
        x: i64,
        y: i64,
        diameter: i64,
        drill: i64,
    },
    CreateZone {
        net: String,
        layer: String,
        outline: Vec<[i64; 2]>,
        #[serde(default)]
        clearance: Option<i64>,
        #[serde(default)]
        min_width: Option<i64>,
    },
    /// `rule`: `min_track_width`, `min_via_diameter`, `min_via_drill`, `min_annular_ring`,
    /// `copper_to_edge` or `clearance` (default net class), value in nm.
    ChangeRule {
        rule: String,
        value: i64,
    },
}

impl Action {
    /// One-line description for diffs and the audit log.
    pub fn describe(&self) -> String {
        match self {
            Action::CreateComponent {
                reference,
                symbol,
                value,
                ..
            } => {
                format!(
                    "create component {} ({symbol}{})",
                    reference.clone().unwrap_or_else(|| "auto".into()),
                    if value.is_empty() {
                        String::new()
                    } else {
                        format!(", {value}")
                    }
                )
            }
            Action::RemoveComponent { reference } => format!("remove component {reference}"),
            Action::Connect { net, pins } => format!("connect {} pin(s) to {net}", pins.len()),
            Action::Disconnect { reference, pin } => format!("disconnect {reference}.{pin}"),
            Action::Place {
                reference, x, y, ..
            } => format!(
                "place {reference} at ({:.3}, {:.3}) mm",
                *x as f64 / 1e6,
                *y as f64 / 1e6
            ),
            Action::Move { reference, dx, dy } => format!(
                "move {reference} by ({:.3}, {:.3}) mm",
                *dx as f64 / 1e6,
                *dy as f64 / 1e6
            ),
            Action::Route {
                net,
                layer,
                points,
                width,
            } => format!(
                "route {net} on {layer}, {} point(s), {:.3} mm wide",
                points.len(),
                *width as f64 / 1e6
            ),
            Action::CreateVia { net, x, y, .. } => format!(
                "create via on {net} at ({:.3}, {:.3}) mm",
                *x as f64 / 1e6,
                *y as f64 / 1e6
            ),
            Action::CreateZone { net, layer, .. } => format!("create zone {net} on {layer}"),
            Action::ChangeRule { rule, value } => {
                format!("set rule {rule} = {:.3} mm", *value as f64 / 1e6)
            }
        }
    }

    /// Does this action change the schematic (so the board must be synced afterwards)?
    pub fn touches_schematic(&self) -> bool {
        matches!(
            self,
            Action::CreateComponent { .. }
                | Action::RemoveComponent { .. }
                | Action::Connect { .. }
                | Action::Disconnect { .. }
        )
    }
}
