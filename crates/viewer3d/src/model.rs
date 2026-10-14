//! The viewer's input: plain geometry in millimetres.

/// A point `[x, y]` in mm (+Y up, like the PCB).
pub type P = [f64; 2];

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub a: P,
    pub b: P,
    pub width: f64,
    /// Front (top) side when true.
    pub top: bool,
}

/// A copper pad: a simple polygon, on the top side, the bottom side, or through (`top` and
/// `bottom`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub outline: Vec<P>,
    pub top: bool,
    pub bottom: bool,
}

/// A drilled via or through-hole pad: a hole in the substrate with a copper barrel ring.
#[derive(Debug, Clone, PartialEq)]
pub struct ViaHole {
    pub center: P,
    pub outer_radius: f64,
    pub drill_radius: f64,
}

/// A filled area: outer contour and holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Vec<P>,
    pub holes: Vec<Vec<P>>,
    pub top: bool,
}

/// A component body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub outline: Vec<P>,
    pub height: f64,
    pub top: bool,
    /// The footprint references a 3D model that is not rendered yet.
    pub has_model: bool,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub outline: Vec<P>,
    pub cutouts: Vec<Vec<P>>,
    /// Substrate thickness in mm (the whole stack without outer copper and mask).
    pub thickness: f64,
    pub copper_thickness: f64,
    pub mask_thickness: f64,
    pub tracks: Vec<Segment>,
    pub pads: Vec<Pad>,
    pub vias: Vec<ViaHole>,
    pub zones: Vec<Region>,
    pub silk: Vec<Segment>,
    pub bodies: Vec<Body>,
}

impl Model {
    pub fn empty() -> Self {
        Self {
            outline: Vec::new(),
            cutouts: Vec::new(),
            thickness: 1.5,
            copper_thickness: 0.035,
            mask_thickness: 0.02,
            tracks: Vec::new(),
            pads: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            silk: Vec::new(),
            bodies: Vec::new(),
        }
    }

    /// Bounding box `(min, max)` of the board outline (or of everything when there is none).
    pub fn bounds(&self) -> Option<(P, P)> {
        let mut pts: Vec<P> = self.outline.clone();
        if pts.is_empty() {
            pts.extend(self.tracks.iter().flat_map(|s| [s.a, s.b]));
            pts.extend(self.pads.iter().flat_map(|p| p.outline.iter().copied()));
            pts.extend(self.bodies.iter().flat_map(|b| b.outline.iter().copied()));
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        }))
    }
}

/// What is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    pub substrate: bool,
    pub mask: bool,
    pub copper: bool,
    pub silkscreen: bool,
    pub components: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            substrate: true,
            mask: true,
            copper: true,
            silkscreen: true,
            components: true,
        }
    }
}
