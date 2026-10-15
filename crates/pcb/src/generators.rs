//! Parametric footprint generators: DIP, SOIC, QFN, BGA and pin headers.
//!
//! Every generator is pure, integer-exact and returns a validated [`Footprint`] with pads,
//! silkscreen, fabrication outline, courtyard, reference and value texts and sensible
//! attributes. Dimensions are in nanometres; the `mm` helper converts the literals below.
//! Pin 1 is at the top-left with the usual conventions (counter-clockwise for dual-row and
//! quad packages, row-major for headers, letter/number for BGA). The default dimensions are the
//! published nominal package dimensions (JEDEC-style): no vendor library data is copied, and the
//! generators are a starting point: check them against the part's datasheet.

use crate::footprint::*;
use buturecad_geometry::{Circle, Coord, Point, Rectangle, Segment};
use thiserror::Error;

pub const MM: Coord = 1_000_000;

/// Millimetres → nm (rounded).
pub fn mm(v: f64) -> Coord {
    (v * MM as f64).round() as Coord
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum GeneratorError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Footprint(#[from] FootprintError),
}

type Result<T> = std::result::Result<T, GeneratorError>;

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(GeneratorError::Invalid(msg.into()))
}

const SILK: Coord = 120_000;
const FAB: Coord = 100_000;
const COURT: Coord = 50_000;
const TEXT_H: Coord = 1_000_000;
const COURT_MARGIN: Coord = 250_000;

fn line(layer: FpLayer, a: Point, b: Point, width: Coord) -> FpGraphic {
    FpGraphic {
        layer,
        shape: FpShape::Line {
            segment: Segment::new(a, b),
            width,
        },
    }
}

fn rect(layer: FpLayer, lo: Point, hi: Point, width: Coord) -> Option<FpGraphic> {
    Rectangle::new(lo, hi).ok().map(|rect| FpGraphic {
        layer,
        shape: FpShape::Rect {
            rect,
            filled: false,
            width,
        },
    })
}

/// Reference above, value below the courtyard.
fn texts(top: Coord, bottom: Coord) -> Vec<FpText> {
    vec![
        FpText {
            kind: TextKind::Reference,
            layer: FpLayer::Silkscreen,
            text: "REF**".into(),
            position: Point::new(0, top + TEXT_H),
            height: TEXT_H,
            rotation: 0,
        },
        FpText {
            kind: TextKind::Value,
            layer: FpLayer::Fabrication,
            text: "VAL**".into(),
            position: Point::new(0, bottom - TEXT_H),
            height: TEXT_H,
            rotation: 0,
        },
    ]
}

/// A pin-1 marker triangle just outside `corner` on the silkscreen.
fn pin1_marker(corner: Point, dx: Coord, dy: Coord) -> Vec<FpGraphic> {
    let s = 400_000;
    let a = Point::new(corner.x + dx * s, corner.y + dy * s);
    let b = Point::new(corner.x + dx * s, corner.y + dy * (s + 400_000));
    let c = Point::new(corner.x + dx * (s + 400_000), corner.y + dy * s);
    vec![
        line(FpLayer::Silkscreen, a, b, SILK),
        line(FpLayer::Silkscreen, b, c, SILK),
        line(FpLayer::Silkscreen, c, a, SILK),
    ]
}

/// Centre position of item `i` of `n` spaced `pitch` apart, symmetric about 0 (exact in nm for
/// even pitches; odd-nm pitches round half away from zero).
fn along(i: u32, n: u32, pitch: Coord) -> Coord {
    let twice = 2 * i128::from(i) * i128::from(pitch) - i128::from(n - 1) * i128::from(pitch);
    let half = if twice >= 0 {
        (twice + 1) / 2
    } else {
        (twice - 1) / 2
    };
    half as Coord
}

fn finish(fp: Footprint) -> Result<Footprint> {
    validate(&fp)?;
    Ok(fp)
}

fn rounded_step(v: Coord) -> Coord {
    // Courtyards sit on a 0.05 mm grid, rounding outwards.
    let g = 50_000;
    if v >= 0 {
        (v + g - 1) / g * g
    } else {
        -((-v + g - 1) / g * g)
    }
}

fn courtyard(lo: Point, hi: Point) -> Result<FpGraphic> {
    let lo = Point::new(
        rounded_step(lo.x - COURT_MARGIN),
        rounded_step(lo.y - COURT_MARGIN),
    );
    let hi = Point::new(
        rounded_step(hi.x + COURT_MARGIN),
        rounded_step(hi.y + COURT_MARGIN),
    );
    rect(FpLayer::Courtyard, lo, hi, COURT)
        .ok_or_else(|| GeneratorError::Invalid("the courtyard would be empty".into()))
}

// ---------------------------------------------------------------------------------------------
// DIP
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dip {
    /// Total pins, even, 4..=64.
    pub pins: u32,
    pub pitch: Coord,
    /// Distance between the two pin rows (centre to centre): 7.62 mm (300 mil) narrow,
    /// 15.24 mm (600 mil) wide.
    pub row_spacing: Coord,
    pub pad_diameter: Coord,
    pub drill: Coord,
}

impl Dip {
    /// DIP-`pins` with 2.54 mm pitch and 7.62 mm row spacing.
    pub fn standard(pins: u32) -> Self {
        Self {
            pins,
            pitch: mm(2.54),
            row_spacing: mm(7.62),
            pad_diameter: mm(1.6),
            drill: mm(0.8),
        }
    }

    pub fn generate(&self) -> Result<Footprint> {
        if self.pins < 4 || self.pins > 64 || self.pins & 1 == 1 {
            return bad("a DIP has an even number of pins, 4 to 64");
        }
        if self.pitch <= 0
            || self.row_spacing <= self.pad_diameter
            || self.drill >= self.pad_diameter
            || self.drill <= 0
        {
            return bad("DIP dimensions are inconsistent (row spacing > pad > drill > 0)");
        }
        let per_side = self.pins / 2;
        let x_left = -(self.row_spacing / 2);
        let x_right = self.row_spacing - self.row_spacing / 2;
        let mut fp = Footprint::new(format!(
            "DIP-{}_W{:.2}mm",
            self.pins,
            self.row_spacing as f64 / MM as f64
        ));
        fp.description = format!(
            "{}-pin dual in-line package, {:.2} mm pitch, {:.2} mm row spacing",
            self.pins,
            self.pitch as f64 / MM as f64,
            self.row_spacing as f64 / MM as f64
        );
        fp.tags = "dip through-hole".into();
        fp.attributes.mount = MountType::ThroughHole;
        // Pin 1 top-left, counter-clockwise: down the left row, up the right row.
        for i in 0..per_side {
            let y = -along(i, per_side, self.pitch);
            let shape = if i == 0 {
                PadShape::Rect
            } else {
                PadShape::Oval
            };
            let d = self.pad_diameter;
            fp.pads.push(Pad::through(
                (i + 1).to_string(),
                shape,
                Point::new(x_left, y),
                d,
                d,
                self.drill,
            ));
            fp.pads.push(Pad::through(
                (self.pins - i).to_string(),
                PadShape::Oval,
                Point::new(x_right, y),
                d,
                d,
                self.drill,
            ));
        }
        let half_span = -along(0, per_side, self.pitch);
        // Body between the rows, with the usual notch at the pin-1 end.
        let half_w = self.row_spacing / 2 - self.pad_diameter / 2 - 200_000;
        let body_top = half_span + self.pitch / 2;
        let body_bottom = -body_top;
        let (bx0, bx1) = (-half_w, half_w);
        let notch = 500_000;
        for (a, b) in [
            (Point::new(bx0, body_bottom), Point::new(bx1, body_bottom)),
            (Point::new(bx0, body_top), Point::new(bx0, body_bottom)),
            (Point::new(bx1, body_top), Point::new(bx1, body_bottom)),
            (Point::new(bx0, body_top), Point::new(-notch, body_top)),
            (Point::new(notch, body_top), Point::new(bx1, body_top)),
        ] {
            fp.graphics.push(line(FpLayer::Silkscreen, a, b, SILK));
        }
        fp.graphics.push(FpGraphic {
            layer: FpLayer::Silkscreen,
            shape: FpShape::Arc {
                arc: buturecad_geometry::Arc::new(
                    Point::new(0, body_top),
                    Point::new(-notch, body_top),
                    Point::new(notch, body_top),
                    false,
                )
                .map_err(|_| GeneratorError::Invalid("notch geometry".into()))?,
                width: SILK,
            },
        });
        fp.graphics.extend(rect(
            FpLayer::Fabrication,
            Point::new(bx0, body_bottom),
            Point::new(bx1, body_top),
            FAB,
        ));
        let ext_y = body_top.max(half_span + self.pad_diameter / 2);
        let ext_x = x_right + self.pad_diameter / 2;
        fp.graphics.push(courtyard(
            Point::new(-ext_x.max(-x_left + self.pad_diameter / 2), -ext_y),
            Point::new(ext_x, ext_y),
        )?);
        fp.texts = texts(ext_y + COURT_MARGIN, -ext_y - COURT_MARGIN);
        finish(fp)
    }
}

// ---------------------------------------------------------------------------------------------
// SOIC
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Soic {
    /// Total pins, even, 4..=64.
    pub pins: u32,
    pub pitch: Coord,
    /// Body width (3.9 mm narrow, 7.5 mm wide).
    pub body_width: Coord,
    /// Distance between the pad rows (centre to centre).
    pub row_spacing: Coord,
    pub pad_width: Coord,
    pub pad_length: Coord,
}

impl Soic {
    /// Narrow SOIC (3.9 mm body) at 1.27 mm pitch.
    pub fn narrow(pins: u32) -> Self {
        Self {
            pins,
            pitch: mm(1.27),
            body_width: mm(3.9),
            row_spacing: mm(5.4),
            pad_width: mm(0.6),
            pad_length: mm(1.55),
        }
    }
    /// Wide SOIC (7.5 mm body).
    pub fn wide(pins: u32) -> Self {
        Self {
            body_width: mm(7.5),
            row_spacing: mm(9.0),
            ..Self::narrow(pins)
        }
    }

    pub fn generate(&self) -> Result<Footprint> {
        if self.pins < 4 || self.pins > 64 || self.pins & 1 == 1 {
            return bad("a SOIC has an even number of pins, 4 to 64");
        }
        if self.pitch <= 0
            || self.pad_width <= 0
            || self.pad_length <= 0
            || self.pad_width >= self.pitch
        {
            return bad("SOIC pads must be narrower than the pitch");
        }
        let per_side = self.pins / 2;
        let x = self.row_spacing / 2;
        let mut fp = Footprint::new(format!(
            "SOIC-{}_{:.1}mm_P{:.2}mm",
            self.pins,
            self.body_width as f64 / MM as f64,
            self.pitch as f64 / MM as f64
        ));
        fp.description = format!(
            "{}-pin small outline package, {:.1} mm body, {:.2} mm pitch",
            self.pins,
            self.body_width as f64 / MM as f64,
            self.pitch as f64 / MM as f64
        );
        fp.tags = "soic smd".into();
        fp.attributes.mount = MountType::Smd;
        for i in 0..per_side {
            let y = -along(i, per_side, self.pitch);
            fp.pads.push(Pad::smd(
                (i + 1).to_string(),
                Point::new(-x, y),
                self.pad_length,
                self.pad_width,
            ));
            fp.pads.push(Pad::smd(
                (self.pins - i).to_string(),
                Point::new(x, y),
                self.pad_length,
                self.pad_width,
            ));
        }
        let top = along(0, per_side, self.pitch).abs() + self.pitch / 2;
        let half_w = self.body_width / 2;
        let (bx0, bx1) = (-half_w, half_w);
        fp.graphics.extend(rect(
            FpLayer::Fabrication,
            Point::new(bx0, -top),
            Point::new(bx1, top),
            FAB,
        ));
        // Silkscreen along the long sides, clear of the pads, plus the pin-1 marker.
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(bx0, top),
            Point::new(bx1, top),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(bx0, -top),
            Point::new(bx1, -top),
            SILK,
        ));
        fp.graphics.extend(pin1_marker(
            Point::new(-x - self.pad_length / 2, top - self.pitch / 2),
            -1,
            1,
        ));
        let ext = x + self.pad_length / 2;
        fp.graphics
            .push(courtyard(Point::new(-ext, -top), Point::new(ext, top))?);
        fp.texts = texts(top + COURT_MARGIN, -top - COURT_MARGIN);
        finish(fp)
    }
}

// ---------------------------------------------------------------------------------------------
// QFN
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Qfn {
    /// Pins per side (2..=32); total pins = 4 × this.
    pub pins_per_side: u32,
    pub pitch: Coord,
    pub body_width: Coord,
    pub body_length: Coord,
    pub pad_width: Coord,
    pub pad_length: Coord,
    /// Exposed pad (width, height), numbered `pins + 1`; `None` for none.
    pub exposed: Option<(Coord, Coord)>,
}

impl Qfn {
    /// A square QFN of `body` size, 0.5 mm pitch by default.
    pub fn square(pins_per_side: u32, body: Coord, pitch: Coord) -> Self {
        let pad_width = pitch * 5 / 10 - pitch / 10;
        // The longest pad that keeps 0.2 mm between the corner pads of adjacent sides (a fixed
        // 0.8 mm made the corner pads of dense packages overlap, i.e. short each other).
        let along_max = i128::from(pins_per_side.saturating_sub(1)) * i128::from(pitch) / 2
            + i128::from(pad_width) / 2;
        let fit = i128::from(body) / 2 + 100_000 - along_max - 200_000;
        let pad_length = fit.clamp(300_000, i128::from(mm(0.8))) as Coord;
        Self {
            pins_per_side,
            pitch,
            body_width: body,
            body_length: body,
            pad_width,
            pad_length,
            exposed: None,
        }
    }

    pub fn generate(&self) -> Result<Footprint> {
        let n = self.pins_per_side;
        if !(2..=32).contains(&n) {
            return bad("a QFN has 2 to 32 pins per side");
        }
        if self.pitch <= 0
            || self.pad_width <= 0
            || self.pad_length <= 0
            || self.pad_width >= self.pitch
        {
            return bad("QFN pads must be narrower than the pitch");
        }
        let span = i128::from(n - 1) * i128::from(self.pitch) + i128::from(self.pad_width);
        if span >= i128::from(self.body_width.min(self.body_length)) + i128::from(self.pitch) {
            return bad("the pads do not fit along the body edge");
        }
        let (hw, hl) = (self.body_width / 2, self.body_length / 2);
        // Corner pads of adjacent sides must not touch: a footprint that shorts itself is invalid.
        let along_max =
            i128::from(n - 1) * i128::from(self.pitch) / 2 + i128::from(self.pad_width) / 2;
        let inner = |half: Coord| i128::from(half) + 100_000 - i128::from(self.pad_length);
        if along_max >= inner(hw).min(inner(hl)) {
            return bad("the corner pads of adjacent sides would overlap: use shorter pads, a larger body or fewer pins");
        }
        // Pad centres sit so the pad extends 0.1 mm outside the body and the rest underneath.
        let outside = 100_000;
        let cx = hw + outside - self.pad_length / 2;
        let cy = hl + outside - self.pad_length / 2;
        let total = 4 * n;
        let mut fp = Footprint::new(format!(
            "QFN-{}_{:.0}x{:.0}mm_P{:.2}mm",
            total,
            self.body_width as f64 / MM as f64,
            self.body_length as f64 / MM as f64,
            self.pitch as f64 / MM as f64
        ));
        fp.description = format!(
            "{total}-pin quad flat no-lead package, {:.1}×{:.1} mm body, {:.2} mm pitch",
            self.body_width as f64 / MM as f64,
            self.body_length as f64 / MM as f64,
            self.pitch as f64 / MM as f64
        );
        fp.tags = "qfn smd".into();
        fp.attributes.mount = MountType::Smd;
        // Counter-clockwise from the top of the left side.
        for i in 0..n {
            let y = -along(i, n, self.pitch);
            fp.pads.push(Pad::smd(
                (i + 1).to_string(),
                Point::new(-cx, y),
                self.pad_length,
                self.pad_width,
            ));
        }
        for i in 0..n {
            let x = along(i, n, self.pitch);
            fp.pads.push(Pad::smd(
                (n + i + 1).to_string(),
                Point::new(x, -cy),
                self.pad_width,
                self.pad_length,
            ));
        }
        for i in 0..n {
            let y = along(i, n, self.pitch);
            fp.pads.push(Pad::smd(
                (2 * n + i + 1).to_string(),
                Point::new(cx, y),
                self.pad_length,
                self.pad_width,
            ));
        }
        for i in 0..n {
            let x = -along(i, n, self.pitch);
            fp.pads.push(Pad::smd(
                (3 * n + i + 1).to_string(),
                Point::new(x, cy),
                self.pad_width,
                self.pad_length,
            ));
        }
        if let Some((w, h)) = self.exposed {
            if w <= 0 || h <= 0 || w >= self.body_width || h >= self.body_length {
                return bad("the exposed pad must fit inside the body");
            }
            let mut ep = Pad::smd((total + 1).to_string(), Point::ORIGIN, w, h);
            ep.shape = PadShape::RoundRect;
            ep.roundrect_ratio = 100;
            fp.pads.push(ep);
        }
        fp.graphics.extend(rect(
            FpLayer::Fabrication,
            Point::new(-hw, -hl),
            Point::new(hw, hl),
            FAB,
        ));
        // Corner silkscreen (clear of the pads) and the pin-1 marker.
        let c = 800_000.min(hw).min(hl);
        for (sx, sy) in [(1, 1), (1, -1), (-1, -1)] {
            let (px, py) = (sx * (hw + 150_000), sy * (hl + 150_000));
            fp.graphics.push(line(
                FpLayer::Silkscreen,
                Point::new(px, py),
                Point::new(px - sx * c, py),
                SILK,
            ));
            fp.graphics.push(line(
                FpLayer::Silkscreen,
                Point::new(px, py),
                Point::new(px, py - sy * c),
                SILK,
            ));
        }
        fp.graphics.extend(pin1_marker(
            Point::new(-hw - self.pad_length / 2 - 150_000, hl),
            1,
            1,
        ));
        let (ex, ey) = (hw + outside, hl + outside);
        fp.graphics
            .push(courtyard(Point::new(-ex, -ey), Point::new(ex, ey))?);
        fp.texts = texts(ey + COURT_MARGIN, -ey - COURT_MARGIN);
        finish(fp)
    }
}

// ---------------------------------------------------------------------------------------------
// BGA
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bga {
    pub rows: u32,
    pub columns: u32,
    pub pitch: Coord,
    /// Copper pad diameter (≈ 0.8 × the ball diameter for non-solder-mask-defined pads).
    pub pad_diameter: Coord,
    /// Body size (width, height).
    pub body: (Coord, Coord),
}

/// Row letters follow the JEDEC convention and skip I, O, Q, S, X and Z.
const ROW_LETTERS: &[u8] = b"ABCDEFGHJKLMNPRTUVWY";

/// Pin name for a BGA row index (0-based) and column index (0-based): `A1`, `B3`, `AA1`…
pub fn bga_name(row: u32, col: u32) -> String {
    let base = ROW_LETTERS.len() as u32;
    let mut letters = String::new();
    let mut r = row;
    loop {
        letters.insert(0, ROW_LETTERS[(r % base) as usize] as char);
        if r < base {
            break;
        }
        r = r / base - 1;
    }
    format!("{letters}{}", col + 1)
}

impl Bga {
    pub fn generate(&self) -> Result<Footprint> {
        if !(2..=40).contains(&self.rows) || !(2..=40).contains(&self.columns) {
            return bad("a BGA grid is 2 to 40 balls in each direction");
        }
        if self.pitch <= 0 || self.pad_diameter <= 0 || self.pad_diameter >= self.pitch {
            return bad("BGA pads must be smaller than the pitch");
        }
        let width =
            i128::from(self.columns - 1) * i128::from(self.pitch) + i128::from(self.pad_diameter);
        let height =
            i128::from(self.rows - 1) * i128::from(self.pitch) + i128::from(self.pad_diameter);
        if i128::from(self.body.0) < width || i128::from(self.body.1) < height {
            return bad("the body is smaller than the ball array");
        }
        let mut fp = Footprint::new(format!(
            "BGA-{}_{}x{}_{:.0}x{:.0}mm_P{:.2}mm",
            self.rows * self.columns,
            self.columns,
            self.rows,
            self.body.0 as f64 / MM as f64,
            self.body.1 as f64 / MM as f64,
            self.pitch as f64 / MM as f64
        ));
        fp.description = format!(
            "{}×{} ball grid array, {:.2} mm pitch",
            self.columns,
            self.rows,
            self.pitch as f64 / MM as f64
        );
        fp.tags = "bga smd".into();
        fp.attributes.mount = MountType::Smd;
        for r in 0..self.rows {
            for c in 0..self.columns {
                let at = Point::new(
                    along(c, self.columns, self.pitch),
                    -along(r, self.rows, self.pitch),
                );
                let mut p = Pad::smd(bga_name(r, c), at, self.pad_diameter, self.pad_diameter);
                p.shape = PadShape::Circle;
                fp.pads.push(p);
            }
        }
        let (hw, hh) = (self.body.0 / 2, self.body.1 / 2);
        // Fabrication outline with the pin-1 corner chamfered.
        let ch = 1_000_000.min(hw).min(hh);
        let pts = [
            (Point::new(-hw + ch, hh), Point::new(hw, hh)),
            (Point::new(hw, hh), Point::new(hw, -hh)),
            (Point::new(hw, -hh), Point::new(-hw, -hh)),
            (Point::new(-hw, -hh), Point::new(-hw, hh - ch)),
            (Point::new(-hw, hh - ch), Point::new(-hw + ch, hh)),
        ];
        for (a, b) in pts {
            fp.graphics.push(line(FpLayer::Fabrication, a, b, FAB));
        }
        let m = 100_000;
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(-hw + ch, hh + m),
            Point::new(hw + m, hh + m),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(hw + m, hh + m),
            Point::new(hw + m, -hh - m),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(hw + m, -hh - m),
            Point::new(-hw - m, -hh - m),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(-hw - m, -hh - m),
            Point::new(-hw - m, hh - ch),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(-hw - m, hh - ch),
            Point::new(-hw + ch, hh + m),
            SILK,
        ));
        fp.graphics.push(courtyard(
            Point::new(-hw - m, -hh - m),
            Point::new(hw + m, hh + m),
        )?);
        fp.texts = texts(hh + m + COURT_MARGIN, -hh - m - COURT_MARGIN);
        finish(fp)
    }
}

// ---------------------------------------------------------------------------------------------
// Pin headers
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Pins per row, 1..=40.
    pub pins_per_row: u32,
    /// 1 or 2 rows.
    pub rows: u32,
    pub pitch: Coord,
    pub pad_diameter: Coord,
    pub drill: Coord,
}

impl Header {
    /// A 2.54 mm pin header.
    pub fn standard(pins_per_row: u32, rows: u32) -> Self {
        Self {
            pins_per_row,
            rows,
            pitch: mm(2.54),
            pad_diameter: mm(1.7),
            drill: mm(1.0),
        }
    }

    pub fn generate(&self) -> Result<Footprint> {
        if !(1..=40).contains(&self.pins_per_row) || !(1..=2).contains(&self.rows) {
            return bad("a header has 1 or 2 rows of 1 to 40 pins");
        }
        if self.pitch <= 0
            || self.drill <= 0
            || self.drill >= self.pad_diameter
            || self.pad_diameter >= self.pitch
        {
            return bad("header dimensions are inconsistent (pitch > pad > drill > 0)");
        }
        let (n, rows) = (self.pins_per_row, self.rows);
        let mut fp = Footprint::new(format!(
            "PinHeader_{rows}x{n:02}_P{:.2}mm",
            self.pitch as f64 / MM as f64
        ));
        fp.description = format!(
            "Pin header, {rows} row(s) of {n}, {:.2} mm pitch",
            self.pitch as f64 / MM as f64
        );
        fp.tags = "header pin through-hole".into();
        fp.attributes.mount = MountType::ThroughHole;
        // Pin 1 at the origin; rows go down (−Y), the second row to the right.
        for r in 0..n {
            for c in 0..rows {
                let number = r * rows + c + 1;
                let at = Point::new(
                    Coord::from(c as i32) * self.pitch,
                    -(Coord::from(r as i32) * self.pitch),
                );
                let shape = if number == 1 {
                    PadShape::Rect
                } else {
                    PadShape::Circle
                };
                fp.pads.push(Pad::through(
                    number.to_string(),
                    shape,
                    at,
                    self.pad_diameter,
                    self.pad_diameter,
                    self.drill,
                ));
            }
        }
        let half = self.pitch / 2;
        let (x0, x1) = (-half, Coord::from((rows - 1) as i32) * self.pitch + half);
        let (y1, y0) = (half, -(Coord::from((n - 1) as i32) * self.pitch) - half);
        fp.graphics.extend(rect(
            FpLayer::Fabrication,
            Point::new(x0, y0),
            Point::new(x1, y1),
            FAB,
        ));
        let s = 130_000;
        fp.graphics.extend(rect(
            FpLayer::Silkscreen,
            Point::new(x0 - s, y0 - s),
            Point::new(x1 + s, y1 + s),
            SILK,
        ));
        fp.graphics.push(FpGraphic {
            layer: FpLayer::Silkscreen,
            shape: FpShape::Circle {
                circle: Circle {
                    center: Point::new(0, y1 + s + 400_000),
                    radius: 150_000,
                },
                filled: true,
                width: 0,
            },
        });
        fp.graphics.push(courtyard(
            Point::new(x0 - s, y0 - s),
            Point::new(x1 + s, y1 + s + 500_000),
        )?);
        fp.texts = vec![
            FpText {
                kind: TextKind::Reference,
                layer: FpLayer::Silkscreen,
                text: "REF**".into(),
                position: Point::new((x0 + x1) / 2, y1 + COURT_MARGIN + 1_400_000),
                height: TEXT_H,
                rotation: 0,
            },
            FpText {
                kind: TextKind::Value,
                layer: FpLayer::Fabrication,
                text: "VAL**".into(),
                position: Point::new((x0 + x1) / 2, y0 - COURT_MARGIN - TEXT_H),
                height: TEXT_H,
                rotation: 0,
            },
        ];
        finish(fp)
    }
}

/// How a generator parameter is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A whole number.
    Integer,
    /// A length in millimetres.
    Millimetres,
    Flag,
}

/// One parameter of a generator, with its default as the text `generate` accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorSpec {
    pub family: &'static str,
    pub title: &'static str,
    pub params: Vec<ParamSpec>,
}

/// Everything the UI needs to offer the generators: families and their parameters. The defaults
/// are exactly what `generate(family, &[])` uses, so the form and the engine cannot drift.
pub fn catalog() -> Vec<GeneratorSpec> {
    use ParamKind::*;
    let p = |key, label, default, kind| ParamSpec {
        key,
        label,
        default,
        kind,
    };
    vec![
        GeneratorSpec {
            family: "dip",
            title: "DIP (dual in-line, through-hole)",
            params: vec![
                p("pins", "Pins (even)", "8", Integer),
                p("pitch", "Pitch (mm)", "2.54", Millimetres),
                p("row_spacing", "Row spacing (mm)", "7.62", Millimetres),
            ],
        },
        GeneratorSpec {
            family: "soic",
            title: "SOIC (small outline, SMD)",
            params: vec![
                p("pins", "Pins (even)", "8", Integer),
                p("wide", "Wide body (7.5 mm)", "false", Flag),
                p("pitch", "Pitch (mm)", "1.27", Millimetres),
            ],
        },
        GeneratorSpec {
            family: "qfn",
            title: "QFN (quad flat no-lead, SMD)",
            params: vec![
                p("pins_per_side", "Pins per side", "4", Integer),
                p("body", "Body size (mm, square)", "4", Millimetres),
                p("pitch", "Pitch (mm)", "0.5", Millimetres),
                p("exposed", "Exposed pad (mm, 0 = none)", "0", Millimetres),
            ],
        },
        GeneratorSpec {
            family: "bga",
            title: "BGA (ball grid array, SMD)",
            params: vec![
                p("rows", "Rows", "8", Integer),
                p("columns", "Columns", "8", Integer),
                p("pitch", "Pitch (mm)", "0.8", Millimetres),
                p("pad", "Pad diameter (mm)", "0.4", Millimetres),
            ],
        },
        GeneratorSpec {
            family: "header",
            title: "Pin header (through-hole)",
            params: vec![
                p("pins", "Pins per row", "4", Integer),
                p("rows", "Rows (1 or 2)", "1", Integer),
                p("pitch", "Pitch (mm)", "2.54", Millimetres),
            ],
        },
    ]
}

/// The generator families offered by the UI.
pub const FAMILIES: [&str; 5] = ["dip", "soic", "qfn", "bga", "header"];

/// Generates a footprint from a family name and `key=value` parameters (lengths in mm).
///
/// * `dip`: `pins`, `wide` (bool), `pitch`, `row_spacing`
/// * `soic`: `pins`, `wide` (bool), `pitch`
/// * `qfn`: `pins_per_side`, `body`, `pitch`, `exposed` (mm, square; 0 = none)
/// * `bga`: `rows`, `columns`, `pitch`, `pad`
/// * `header`: `pins`, `rows`, `pitch`
///
/// Unknown keys and unparsable values are errors: nothing is silently ignored.
pub fn generate(family: &str, params: &[(String, String)]) -> Result<Footprint> {
    let get = |key: &str| {
        params
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let num = |key: &str, default: f64| -> Result<f64> {
        match get(key) {
            None => Ok(default),
            Some(v) => v
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .ok_or_else(|| GeneratorError::Invalid(format!("{key} must be a number"))),
        }
    };
    let int = |key: &str, default: u32| -> Result<u32> {
        let v = num(key, f64::from(default))?;
        if v < 0.0 || v.fract() != 0.0 || v > 1e6 {
            return bad(format!("{key} must be a whole number"));
        }
        Ok(v as u32)
    };
    let flag = |key: &str| matches!(get(key), Some("true" | "1" | "yes"));
    let allowed: &[&str] = match family {
        "dip" => &["pins", "wide", "pitch", "row_spacing"],
        "soic" => &["pins", "wide", "pitch"],
        "qfn" => &["pins_per_side", "body", "pitch", "exposed"],
        "bga" => &["rows", "columns", "pitch", "pad"],
        "header" => &["pins", "rows", "pitch"],
        other => {
            return bad(format!(
                "unknown generator “{other}” (try dip, soic, qfn, bga, header)"
            ))
        }
    };
    if let Some((k, _)) = params.iter().find(|(k, _)| !allowed.contains(&k.as_str())) {
        return bad(format!("{family} has no parameter “{k}”"));
    }
    match family {
        "dip" => {
            let mut d = Dip::standard(int("pins", 8)?);
            if flag("wide") {
                d.row_spacing = mm(15.24);
            }
            d.pitch = mm(num("pitch", d.pitch as f64 / MM as f64)?);
            d.row_spacing = mm(num("row_spacing", d.row_spacing as f64 / MM as f64)?);
            d.generate()
        }
        "soic" => {
            let mut s = if flag("wide") {
                Soic::wide(int("pins", 8)?)
            } else {
                Soic::narrow(int("pins", 8)?)
            };
            s.pitch = mm(num("pitch", s.pitch as f64 / MM as f64)?);
            s.generate()
        }
        "qfn" => {
            let pitch = mm(num("pitch", 0.5)?);
            let mut q = Qfn::square(int("pins_per_side", 4)?, mm(num("body", 4.0)?), pitch);
            let ep = num("exposed", 0.0)?;
            if ep > 0.0 {
                q.exposed = Some((mm(ep), mm(ep)));
            }
            q.generate()
        }
        "bga" => {
            let pitch = mm(num("pitch", 0.8)?);
            let (rows, columns) = (int("rows", 8)?, int("columns", 8)?);
            let pad = mm(num("pad", 0.4)?);
            let w = i128::from(columns.saturating_sub(1)) * i128::from(pitch)
                + i128::from(pad)
                + 2 * 500_000;
            let h = i128::from(rows.saturating_sub(1)) * i128::from(pitch)
                + i128::from(pad)
                + 2 * 500_000;
            Bga {
                rows,
                columns,
                pitch,
                pad_diameter: pad,
                body: (w as Coord, h as Coord),
            }
            .generate()
        }
        _ => {
            let mut h = Header::standard(int("pins", 4)?, int("rows", 1)?);
            h.pitch = mm(num("pitch", 2.54)?);
            h.generate()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_matches_the_engine() {
        for spec in catalog() {
            assert!(FAMILIES.contains(&spec.family));
            // The defaults in the catalog are exactly the engine's defaults.
            let from_catalog: Vec<(String, String)> = spec
                .params
                .iter()
                .map(|p| (p.key.to_owned(), p.default.to_owned()))
                .collect();
            let a = generate(spec.family, &from_catalog).unwrap();
            let b = generate(spec.family, &[]).unwrap();
            assert_eq!(a, b, "{}", spec.family);
        }
    }

    #[test]
    fn bga_names_follow_the_jedec_letters() {
        assert_eq!(bga_name(0, 0), "A1");
        assert_eq!(bga_name(7, 2), "H3");
        assert_eq!(bga_name(8, 0), "J1", "I is skipped");
        assert_eq!(bga_name(19, 9), "Y10");
        assert_eq!(bga_name(20, 0), "AA1");
        assert_eq!(bga_name(21, 4), "AB5");
    }

    #[test]
    fn spacing_is_symmetric_and_exact() {
        for n in 1..=9u32 {
            let xs: Vec<Coord> = (0..n).map(|i| along(i, n, 1_270_000)).collect();
            assert_eq!(xs.iter().sum::<Coord>(), 0, "n={n}");
            for w in xs.windows(2) {
                assert_eq!(w[1] - w[0], 1_270_000);
            }
        }
    }

    #[test]
    fn every_generator_validates_and_rejects_bad_parameters() {
        assert!(Dip::standard(2).generate().is_err());
        assert!(Dip::standard(7).generate().is_err());
        assert!(Soic::narrow(3).generate().is_err());
        assert!(Qfn::square(1, mm(4.0), mm(0.5)).generate().is_err());
        assert!(Bga {
            rows: 1,
            columns: 8,
            pitch: mm(0.8),
            pad_diameter: mm(0.4),
            body: (mm(9.0), mm(9.0))
        }
        .generate()
        .is_err());
        assert!(Header::standard(0, 1).generate().is_err());
        assert!(Header::standard(4, 3).generate().is_err());
        assert!(generate("nope", &[]).is_err());
        assert!(generate("dip", &[("bogus".into(), "1".into())]).is_err());
        assert!(generate("dip", &[("pins".into(), "abc".into())]).is_err());
    }
}

// ---------------------------------------------------------------------------------------------
// Two-terminal chip (resistors, capacitors, inductors, LEDs)
// ---------------------------------------------------------------------------------------------

/// Imperial chip sizes: `(code, pad width along the axis, pad height, centre-to-centre of pads,
/// body length, body width)` in mm. Nominal values for general-purpose land patterns; check them
/// against the part's datasheet and your fabricator's capabilities.
const CHIP_SIZES: [(&str, f64, f64, f64, f64, f64); 5] = [
    ("0201", 0.40, 0.40, 0.66, 0.60, 0.30),
    ("0402", 0.55, 0.60, 1.02, 1.00, 0.50),
    ("0603", 0.80, 0.95, 1.65, 1.60, 0.80),
    ("0805", 1.00, 1.45, 1.90, 2.00, 1.25),
    ("1206", 1.15, 1.80, 2.95, 3.20, 1.60),
];

/// Imperial chip codes the generator knows.
pub const CHIP_CODES: [&str; 5] = ["0201", "0402", "0603", "0805", "1206"];

/// A two-pad chip footprint (`0402`, `0603`, …) named `Chip_<code>`. Pin 1 is on the left.
pub fn chip(code: &str) -> Result<Footprint> {
    let Some(&(_, pw, ph, cc, bl, bw)) = CHIP_SIZES.iter().find(|s| s.0 == code) else {
        return bad(format!(
            "unknown chip size “{code}” (known: {})",
            CHIP_CODES.join(", ")
        ));
    };
    let mut fp = Footprint::new(format!("Chip_{code}"));
    fp.description = format!("{code} (imperial) two-terminal chip component");
    fp.tags = format!("chip {code} smd");
    fp.attributes.mount = MountType::Smd;
    let x = mm(cc / 2.0);
    fp.pads
        .push(Pad::smd("1", Point::new(-x, 0), mm(pw), mm(ph)));
    fp.pads
        .push(Pad::smd("2", Point::new(x, 0), mm(pw), mm(ph)));
    let (hl, hw) = (mm(bl / 2.0), mm(bw / 2.0));
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-hl, -hw),
        Point::new(hl, hw),
        FAB,
    ));
    // Silkscreen ticks above and below the body, clear of the pads.
    let sx = mm((cc - pw) / 2.0 - 0.1).max(0);
    let sy = hw.max(mm(ph / 2.0)) + mm(0.2);
    fp.graphics.push(line(
        FpLayer::Silkscreen,
        Point::new(-sx, -sy),
        Point::new(sx, -sy),
        SILK,
    ));
    fp.graphics.push(line(
        FpLayer::Silkscreen,
        Point::new(-sx, sy),
        Point::new(sx, sy),
        SILK,
    ));
    let ext = x + mm(pw / 2.0);
    let top = sy + SILK / 2;
    fp.graphics
        .push(courtyard(Point::new(-ext, -top), Point::new(ext, top))?);
    fp.texts = texts(top + COURT_MARGIN, -top - COURT_MARGIN);
    finish(fp)
}

#[cfg(test)]
mod chip_tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used)]
    fn every_chip_size_is_valid_and_symmetric() {
        for c in CHIP_CODES {
            let f = chip(c).unwrap();
            assert_eq!(f.name, format!("Chip_{c}"));
            assert_eq!(f.pads.len(), 2);
            assert_eq!(f.pads[0].position.x, -f.pads[1].position.x);
            assert_eq!(f.pads[0].number, "1");
        }
        assert!(chip("9999").is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod qfn_overlap_tests {
    use super::*;

    fn overlaps(f: &Footprint) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (i, a) in f.pads.iter().enumerate() {
            for b in &f.pads[i + 1..] {
                let (al, ah) = a.bounds();
                let (bl, bh) = b.bounds();
                if al.x < bh.x && bl.x < ah.x && al.y < bh.y && bl.y < ah.y {
                    out.push((a.number.clone(), b.number.clone()));
                }
            }
        }
        out
    }

    /// Found by the stage-47 validation board: a 48-pin 7 mm QFN had overlapping corner pads.
    #[test]
    fn dense_qfn_pads_never_overlap() {
        for (n, body, pitch, ep) in [
            (12, 7.0, 0.5, 5.0),
            (8, 5.0, 0.5, 3.0),
            (4, 4.0, 0.5, 2.0),
            (16, 7.0, 0.4, 0.0),
        ] {
            let mut q = Qfn::square(n, mm(body), mm(pitch));
            if ep > 0.0 {
                q.exposed = Some((mm(ep), mm(ep)));
            }
            match q.generate() {
                Ok(f) => assert!(
                    overlaps(&f).is_empty(),
                    "{n}/{body}/{pitch}: {:?}",
                    overlaps(&f)
                ),
                Err(e) => panic!("{n}/{body}/{pitch}: {e}"),
            }
        }
    }

    #[test]
    fn an_impossible_qfn_is_refused_instead_of_shorting() {
        let mut q = Qfn::square(12, mm(7.0), mm(0.5));
        q.pad_length = mm(0.8); // the old default
        assert!(q.generate().is_err());
    }
}
