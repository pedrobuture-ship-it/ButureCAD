//! Footprint data model and validation. Pure data: no rendering, no tools.

use buturecad_geometry::{Arc, Circle, Coord, Point, Rectangle, Segment};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Largest number of pads a footprint may have (guards hostile library files).
pub const MAX_PADS: usize = 4096;
/// Largest custom-pad outline.
pub const MAX_CUSTOM_POINTS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    /// Surface-mount: copper, solder mask and paste on the top side, no hole.
    Smd,
    /// Plated through-hole: copper on every layer, drilled.
    Through,
    /// Non-plated hole (mounting hole): drilled, no copper.
    NonPlated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    Circle,
    Rect,
    /// A stadium: a rectangle with fully rounded ends.
    Oval,
    /// A rectangle with rounded corners; see [`Pad::roundrect_ratio`].
    RoundRect,
    /// An arbitrary polygon, [`Pad::custom`] (pad-local coordinates).
    Custom,
}

/// A drilled hole. A round hole has `slot: None`; an oval (slot) hole has `diameter` as its width
/// and `slot` as its length along the pad's long axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drill {
    pub diameter: Coord,
    #[serde(default)]
    pub slot: Option<Coord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pad {
    /// Pin number or name (`1`, `A1`, `EP`). Several plated pads may share one (they are one
    /// electrical pin). Empty for non-plated holes.
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    /// Pad centre, footprint coordinates.
    pub position: Point,
    pub size_x: Coord,
    pub size_y: Coord,
    /// Counter-clockwise quarter turns of the pad about its centre (`0..=3`).
    #[serde(default)]
    pub rotation: u8,
    #[serde(default)]
    pub drill: Option<Drill>,
    /// Corner radius of a [`PadShape::RoundRect`] pad, per mille of the shorter side (`0..=500`).
    #[serde(default)]
    pub roundrect_ratio: u16,
    /// Solder-mask opening margin around the pad; `None` = the board's default.
    #[serde(default)]
    pub mask_margin: Option<Coord>,
    /// Paste-aperture margin (negative shrinks); `None` = the board's default.
    #[serde(default)]
    pub paste_margin: Option<Coord>,
    /// SMD pads get a paste aperture unless this is `false` (fiducials, thermal vias).
    #[serde(default = "yes")]
    pub paste: bool,
    /// Outline of a [`PadShape::Custom`] pad, pad-local coordinates (implicitly closed).
    #[serde(default)]
    pub custom: Vec<Point>,
}

fn yes() -> bool {
    true
}

impl Pad {
    /// A plain SMD rectangle pad.
    pub fn smd(number: impl Into<String>, at: Point, w: Coord, h: Coord) -> Self {
        Self {
            number: number.into(),
            kind: PadKind::Smd,
            shape: PadShape::Rect,
            position: at,
            size_x: w,
            size_y: h,
            rotation: 0,
            drill: None,
            roundrect_ratio: 0,
            mask_margin: None,
            paste_margin: None,
            paste: true,
            custom: Vec::new(),
        }
    }

    /// A plated through-hole pad.
    pub fn through(
        number: impl Into<String>,
        shape: PadShape,
        at: Point,
        w: Coord,
        h: Coord,
        drill: Coord,
    ) -> Self {
        Self {
            kind: PadKind::Through,
            shape,
            drill: Some(Drill {
                diameter: drill,
                slot: None,
            }),
            ..Self::smd(number, at, w, h)
        }
    }

    /// A non-plated mounting hole of `drill` diameter.
    pub fn npth(at: Point, drill: Coord) -> Self {
        Self {
            number: String::new(),
            kind: PadKind::NonPlated,
            shape: PadShape::Circle,
            position: at,
            size_x: drill,
            size_y: drill,
            rotation: 0,
            drill: Some(Drill {
                diameter: drill,
                slot: None,
            }),
            roundrect_ratio: 0,
            mask_margin: None,
            paste_margin: None,
            paste: false,
            custom: Vec::new(),
        }
    }

    /// Extent after rotation: `(width, height)` as seen on the board.
    pub fn rotated_size(&self) -> (Coord, Coord) {
        if self.rotation % 2 == 1 {
            (self.size_y, self.size_x)
        } else {
            (self.size_x, self.size_y)
        }
    }

    /// Axis-aligned bounds `(min, max)` of the pad copper in footprint coordinates.
    pub fn bounds(&self) -> (Point, Point) {
        if self.shape == PadShape::Custom && !self.custom.is_empty() {
            let pts = self
                .custom
                .iter()
                .map(|p| p.rotate_quarter_turns(Point::ORIGIN, self.rotation % 4));
            let (mut lo, mut hi) = (
                Point::new(Coord::MAX, Coord::MAX),
                Point::new(Coord::MIN, Coord::MIN),
            );
            for p in pts {
                lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            return (
                Point::new(self.position.x + lo.x, self.position.y + lo.y),
                Point::new(self.position.x + hi.x, self.position.y + hi.y),
            );
        }
        let (w, h) = self.rotated_size();
        (
            Point::new(self.position.x - w / 2, self.position.y - h / 2),
            Point::new(self.position.x + w - w / 2, self.position.y + h - h / 2),
        )
    }
}

/// Footprint layers a graphic can live on. (Copper belongs to pads; board-level layers come with
/// the PCB core.) All are *top* side; flipping happens at placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpLayer {
    Silkscreen,
    Fabrication,
    Courtyard,
    /// Extra solder-mask openings.
    Mask,
    /// Extra paste apertures.
    Paste,
}

impl FpLayer {
    pub const ALL: [FpLayer; 5] = [
        FpLayer::Silkscreen,
        FpLayer::Fabrication,
        FpLayer::Courtyard,
        FpLayer::Mask,
        FpLayer::Paste,
    ];

    pub fn id(self) -> &'static str {
        match self {
            FpLayer::Silkscreen => "silkscreen",
            FpLayer::Fabrication => "fabrication",
            FpLayer::Courtyard => "courtyard",
            FpLayer::Mask => "mask",
            FpLayer::Paste => "paste",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FpLayer::Silkscreen => "Silkscreen",
            FpLayer::Fabrication => "Fabrication",
            FpLayer::Courtyard => "Courtyard",
            FpLayer::Mask => "Solder mask",
            FpLayer::Paste => "Solder paste",
        }
    }

    pub fn parse(s: &str) -> Option<FpLayer> {
        FpLayer::ALL.into_iter().find(|l| l.id() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FpShape {
    Line {
        segment: Segment,
        width: Coord,
    },
    Rect {
        rect: Rectangle,
        filled: bool,
        width: Coord,
    },
    Circle {
        circle: Circle,
        filled: bool,
        width: Coord,
    },
    Arc {
        arc: Arc,
        width: Coord,
    },
    Polygon {
        points: Vec<Point>,
        filled: bool,
        width: Coord,
    },
}

/// A graphic on a footprint layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpGraphic {
    pub layer: FpLayer,
    #[serde(flatten)]
    pub shape: FpShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    /// Shows the part's reference (`R1`); the text content is only a placeholder (`REF**`).
    Reference,
    /// Shows the part's value.
    Value,
    /// Fixed text.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpText {
    pub kind: TextKind,
    pub layer: FpLayer,
    pub text: String,
    pub position: Point,
    pub height: Coord,
    #[serde(default)]
    pub rotation: u8,
}

/// A reference to a 3D model. **Stored only**: nothing renders it yet (roadmap stage 27/28).
/// Offsets are nm, rotations millidegrees, scales parts-per-million (1 000 000 = ×1), so the file
/// stays integer-exact like the rest of the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model3d {
    /// Path as the user wrote it (relative to the project/library or an absolute path).
    pub path: String,
    #[serde(default)]
    pub offset: [Coord; 3],
    #[serde(default)]
    pub rotation_mdeg: [i32; 3],
    #[serde(default = "unit_scale")]
    pub scale_ppm: [i32; 3],
}

fn unit_scale() -> [i32; 3] {
    [1_000_000; 3]
}

impl Model3d {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            offset: [0; 3],
            rotation_mdeg: [0; 3],
            scale_ppm: unit_scale(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountType {
    #[default]
    Unspecified,
    Smd,
    ThroughHole,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FootprintAttributes {
    #[serde(default)]
    pub mount: MountType,
    #[serde(default)]
    pub exclude_from_bom: bool,
    #[serde(default)]
    pub exclude_from_position_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    /// `Library:Name` inside a project, the bare name inside a library file.
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub attributes: FootprintAttributes,
    pub pads: Vec<Pad>,
    #[serde(default)]
    pub graphics: Vec<FpGraphic>,
    #[serde(default)]
    pub texts: Vec<FpText>,
    #[serde(default)]
    pub models: Vec<Model3d>,
}

impl Footprint {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            tags: String::new(),
            attributes: FootprintAttributes::default(),
            pads: Vec::new(),
            graphics: Vec::new(),
            texts: Vec::new(),
            models: Vec::new(),
        }
    }

    /// Bounds of everything that has copper or a courtyard, if the footprint has any geometry.
    pub fn pad_bounds(&self) -> Option<(Point, Point)> {
        let mut it = self.pads.iter().map(Pad::bounds);
        let first = it.next()?;
        Some(it.fold(first, |(lo, hi), (a, b)| {
            (
                Point::new(lo.x.min(a.x), lo.y.min(a.y)),
                Point::new(hi.x.max(b.x), hi.y.max(b.y)),
            )
        }))
    }
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum FootprintError {
    #[error("footprint name must not be empty")]
    EmptyName,
    #[error("footprint name contains characters that are not allowed ({0})")]
    BadName(String),
    #[error("a footprint may have at most {MAX_PADS} pads")]
    TooManyPads,
    #[error("pad {index}: {reason}")]
    Pad { index: usize, reason: String },
    #[error("graphic {index}: {reason}")]
    Graphic { index: usize, reason: String },
    #[error("text {index}: {reason}")]
    Text { index: usize, reason: String },
    #[error("a footprint has at most one reference text and one value text")]
    DuplicateReferenceOrValue,
    #[error("3D model {index}: {reason}")]
    Model { index: usize, reason: String },
    #[error("{0}")]
    Invalid(String),
    #[error("item {0} does not exist")]
    NoSuchItem(usize),
    #[error("unknown field “{0}”")]
    UnknownField(String),
}

/// Checks a footprint name (`Library:Name` is allowed).
pub fn validate_name(name: &str) -> Result<(), FootprintError> {
    if name.trim().is_empty() {
        return Err(FootprintError::EmptyName);
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || matches!(c, '/' | '\\' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(FootprintError::BadName(format!("{c:?}")));
    }
    Ok(())
}

fn in_range(p: Point) -> bool {
    Point::try_new(p.x, p.y).is_ok()
}

fn pad_err(index: usize, reason: impl Into<String>) -> FootprintError {
    FootprintError::Pad {
        index,
        reason: reason.into(),
    }
}

/// Validates every invariant a stored footprint must satisfy (also run on library load/save).
pub fn validate(fp: &Footprint) -> Result<(), FootprintError> {
    validate_name(&fp.name)?;
    if fp.pads.len() > MAX_PADS {
        return Err(FootprintError::TooManyPads);
    }
    for (i, p) in fp.pads.iter().enumerate() {
        if !in_range(p.position) {
            return Err(pad_err(i, "position is outside the supported range"));
        }
        if p.size_x <= 0 || p.size_y <= 0 {
            return Err(pad_err(i, "size must be positive"));
        }
        if p.size_x > 1 << 30 || p.size_y > 1 << 30 {
            return Err(pad_err(i, "size is outside the supported range"));
        }
        if p.rotation > 3 {
            return Err(pad_err(i, "rotation must be 0..=3 quarter turns"));
        }
        if p.shape == PadShape::Circle && p.size_x != p.size_y {
            return Err(pad_err(i, "a circular pad needs equal width and height"));
        }
        if p.roundrect_ratio > 500 {
            return Err(pad_err(i, "corner ratio must be 0..=500 per mille"));
        }
        if p.shape == PadShape::Custom {
            if p.custom.len() < 3 {
                return Err(pad_err(i, "a custom pad needs at least 3 outline points"));
            }
            if p.custom.len() > MAX_CUSTOM_POINTS {
                return Err(pad_err(i, "custom pad outline is too long"));
            }
            if p.custom.iter().any(|q| !in_range(*q)) {
                return Err(pad_err(i, "custom outline is outside the supported range"));
            }
        } else if !p.custom.is_empty() {
            return Err(pad_err(i, "only custom pads have an outline"));
        }
        match (p.kind, p.drill) {
            (PadKind::Smd, Some(_)) => return Err(pad_err(i, "an SMD pad has no drill")),
            (PadKind::Through | PadKind::NonPlated, None) => {
                return Err(pad_err(i, "a through-hole or non-plated pad needs a drill"))
            }
            _ => {}
        }
        if let Some(d) = p.drill {
            if d.diameter <= 0 {
                return Err(pad_err(i, "drill diameter must be positive"));
            }
            if let Some(slot) = d.slot {
                if slot < d.diameter {
                    return Err(pad_err(i, "a slot is at least as long as it is wide"));
                }
            }
            if p.kind == PadKind::Through {
                let min = p.size_x.min(p.size_y);
                if d.diameter >= min {
                    return Err(pad_err(
                        i,
                        "the drill leaves no copper ring (drill must be smaller than the pad)",
                    ));
                }
            }
        }
        if p.kind == PadKind::NonPlated && !p.number.is_empty() {
            return Err(pad_err(i, "a non-plated hole has no pin number"));
        }
        if p.number.chars().any(char::is_control) {
            return Err(pad_err(i, "pin number contains control characters"));
        }
    }
    for (i, g) in fp.graphics.iter().enumerate() {
        let bad = |reason: &str| FootprintError::Graphic {
            index: i,
            reason: reason.into(),
        };
        let (width, pts): (Coord, Vec<Point>) = match &g.shape {
            FpShape::Line { segment, width } => (*width, vec![segment.a, segment.b]),
            FpShape::Rect { rect, width, .. } => (*width, vec![rect.min(), rect.max()]),
            FpShape::Circle { circle, width, .. } => (*width, vec![circle.center]),
            FpShape::Arc { arc, width } => (*width, vec![arc.center, arc.start, arc.end]),
            FpShape::Polygon { points, width, .. } => {
                if points.len() < 3 {
                    return Err(bad("a polygon needs at least 3 points"));
                }
                if points.len() > MAX_CUSTOM_POINTS {
                    return Err(bad("polygon is too long"));
                }
                (*width, points.clone())
            }
        };
        if width < 0 {
            return Err(bad("line width must not be negative"));
        }
        if pts.iter().any(|p| !in_range(*p)) {
            return Err(bad("outside the supported range"));
        }
    }
    let mut refs = 0;
    let mut vals = 0;
    for (i, t) in fp.texts.iter().enumerate() {
        let bad = |reason: &str| FootprintError::Text {
            index: i,
            reason: reason.into(),
        };
        if t.height <= 0 {
            return Err(bad("height must be positive"));
        }
        if t.text.trim().is_empty() {
            return Err(bad("text must not be empty"));
        }
        if t.rotation > 3 {
            return Err(bad("rotation must be 0..=3 quarter turns"));
        }
        if !in_range(t.position) {
            return Err(bad("outside the supported range"));
        }
        match t.kind {
            TextKind::Reference => refs += 1,
            TextKind::Value => vals += 1,
            TextKind::User => {}
        }
    }
    if refs > 1 || vals > 1 {
        return Err(FootprintError::DuplicateReferenceOrValue);
    }
    for (i, m) in fp.models.iter().enumerate() {
        if m.path.trim().is_empty() || m.path.chars().any(char::is_control) {
            return Err(FootprintError::Model {
                index: i,
                reason: "path must be non-empty and free of control characters".into(),
            });
        }
        if m.scale_ppm.iter().any(|s| *s <= 0) {
            return Err(FootprintError::Model {
                index: i,
                reason: "scale must be positive".into(),
            });
        }
    }
    Ok(())
}

/// Things that make a footprint hard to use or risky to manufacture. They do not block saving.
pub fn warnings(fp: &Footprint) -> Vec<String> {
    let mut w = Vec::new();
    if fp.pads.is_empty() {
        w.push("The footprint has no pads".to_owned());
    }
    let has_court = fp.graphics.iter().any(|g| g.layer == FpLayer::Courtyard);
    if !has_court {
        w.push("There is no courtyard: placement checks need one".to_owned());
    }
    if !fp.texts.iter().any(|t| t.kind == TextKind::Reference) {
        w.push("There is no reference text".to_owned());
    }
    if !fp.texts.iter().any(|t| t.kind == TextKind::Value) {
        w.push("There is no value text".to_owned());
    }
    // Plated pads of different numbers whose copper overlaps would short on the board.
    for (i, a) in fp.pads.iter().enumerate() {
        if a.kind == PadKind::NonPlated {
            continue;
        }
        let (alo, ahi) = a.bounds();
        for b in &fp.pads[i + 1..] {
            if b.kind == PadKind::NonPlated || a.number == b.number {
                continue;
            }
            let (blo, bhi) = b.bounds();
            if alo.x < bhi.x && blo.x < ahi.x && alo.y < bhi.y && blo.y < ahi.y {
                w.push(format!(
                    "Pads {} and {} overlap (their bounding boxes intersect)",
                    a.number, b.number
                ));
            }
        }
    }
    // The courtyard should enclose the pads.
    if let (true, Some((plo, phi))) = (has_court, fp.pad_bounds()) {
        let mut lo = Point::new(Coord::MAX, Coord::MAX);
        let mut hi = Point::new(Coord::MIN, Coord::MIN);
        for g in fp.graphics.iter().filter(|g| g.layer == FpLayer::Courtyard) {
            let pts: Vec<Point> = match &g.shape {
                FpShape::Line { segment, .. } => vec![segment.a, segment.b],
                FpShape::Rect { rect, .. } => vec![rect.min(), rect.max()],
                FpShape::Circle { circle, .. } => vec![
                    Point::new(
                        circle.center.x - circle.radius,
                        circle.center.y - circle.radius,
                    ),
                    Point::new(
                        circle.center.x + circle.radius,
                        circle.center.y + circle.radius,
                    ),
                ],
                FpShape::Arc { arc, .. } => vec![arc.start, arc.end],
                FpShape::Polygon { points, .. } => points.clone(),
            };
            for p in pts {
                lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        if plo.x < lo.x || plo.y < lo.y || phi.x > hi.x || phi.y > hi.y {
            w.push("The courtyard does not enclose all pads".to_owned());
        }
    }
    for p in &fp.pads {
        if p.kind == PadKind::Smd && p.number.is_empty() {
            w.push("An SMD pad has no number".to_owned());
        }
    }
    w
}
