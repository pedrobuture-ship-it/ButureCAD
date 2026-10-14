//! Input of the manufacturing outputs. Coordinates are integer nanometres, +Y up.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pt {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Aperture {
    Circle { d: i64 },
    Rect { w: i64, h: i64 },
    Obround { w: i64, h: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feature {
    Flash {
        at: Pt,
        ap: Aperture,
    },
    /// A stroke with a round aperture of `width`.
    Draw {
        a: Pt,
        b: Pt,
        width: i64,
    },
    /// A filled polygon with holes (holes are cleared with clear polarity).
    Region {
        outer: Vec<Pt>,
        holes: Vec<Vec<Pt>>,
    },
}

/// One Gerber file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// File name inside the zip, e.g. `board-F_Cu.gtl`.
    pub file: String,
    /// X2 `TF.FileFunction` value, e.g. `Copper,L1,Top` or `Profile,NP`.
    pub function: String,
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mount {
    Smd,
    Through,
    Unspecified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub position: Pt,
    /// Counter-clockwise rotation of the placement, degrees.
    pub rotation_deg: u16,
    pub bottom: bool,
    pub mount: Mount,
    pub in_bom: bool,
    pub in_position_file: bool,
}

/// A drilled hole or slot (`slot_to` is the other end centre; `at` is one end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hole {
    pub at: Pt,
    pub diameter: i64,
    pub slot_to: Option<Pt>,
    pub plated: bool,
}

/// Generic process limits used for warnings. They are **defaults, not your fabricator's rules**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub min_track_width: i64,
    pub min_drill: i64,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            min_track_width: 100_000,
            min_drill: 200_000,
        }
    }
}

/// Result of the DRC run made right before the export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrcSummary {
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabInput {
    /// Base name of the files.
    pub name: String,
    /// Loop 0 is the board contour, the others are cut-outs.
    pub outline: Vec<Vec<Pt>>,
    pub copper_layers: u8,
    pub thickness_nm: i64,
    pub layers: Vec<Layer>,
    pub holes: Vec<Hole>,
    pub parts: Vec<Part>,
    pub drc: DrcSummary,
    pub capabilities: Capabilities,
    /// Smallest track width on the board (nm), if there are tracks.
    pub min_track_width: Option<i64>,
    /// Issues only the bridge can know (it reads the real board).
    pub extra_issues: Vec<crate::Issue>,
}

impl FabInput {
    pub fn layer_files(&self) -> BTreeSet<&str> {
        self.layers.iter().map(|l| l.file.as_str()).collect()
    }

    /// Bounding box of the outer contour, `(min, max)`.
    pub fn board_bounds(&self) -> Option<(Pt, Pt)> {
        let o = self.outline.first()?;
        let first = *o.first()?;
        Some(o.iter().fold((first, first), |(lo, hi), p| {
            (
                Pt {
                    x: lo.x.min(p.x),
                    y: lo.y.min(p.y),
                },
                Pt {
                    x: hi.x.max(p.x),
                    y: hi.y.max(p.y),
                },
            )
        }))
    }
}
