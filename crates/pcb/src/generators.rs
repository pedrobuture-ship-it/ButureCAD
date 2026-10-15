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

// ---------------------------------------------------------------------------------------------
// Standard packages (stage 51): TSSOP, MSOP, LQFP, SOT-23, SOT-223, SOD-123, SMA, hole, test point
// ---------------------------------------------------------------------------------------------
//
// Package outlines (pitch, body, lead span, pin counts) are the published nominal JEDEC / EIA
// values. Land patterns follow a **simplified IPC-7351B** nominal rule for gull-wing and J-lead
// parts (toe 0.35 mm, heel 0.35 mm, side 0.03 mm): they are computed, not copied from any vendor
// or library, and vendors' recommended patterns differ. Always compare with the datasheet of the
// part you use.

/// Pad of a gull-wing style lead: `(length, width, |x| of the centre)` for a package whose lead
/// tips span `tip_span`, with leads `lead_w` wide and a flat foot of `foot`.
fn gull(pitch: Coord, tip_span: Coord, lead_w: Coord, foot: Coord) -> (Coord, Coord, Coord) {
    let (toe, heel, side) = (350_000, 350_000, 30_000);
    let length = toe + foot + heel;
    let outer = tip_span / 2 + toe;
    let width = (lead_w + 2 * side).min(pitch - 200_000).max(100_000);
    (length, width, outer - length / 2)
}

/// Common drawing of a two-row package: body outline, silkscreen along the long sides, pin-1
/// marker, courtyard and texts.
fn finish_two_row(
    mut fp: Footprint,
    half_w: Coord,
    half_l: Coord,
    pad_outer_x: Coord,
    marker_y: Coord,
) -> Result<Footprint> {
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-half_w, -half_l),
        Point::new(half_w, half_l),
        FAB,
    ));
    for y in [half_l, -half_l] {
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(-half_w, y),
            Point::new(half_w, y),
            SILK,
        ));
    }
    fp.graphics
        .extend(pin1_marker(Point::new(-pad_outer_x, marker_y), -1, 1));
    fp.graphics.push(courtyard(
        Point::new(-pad_outer_x, -half_l),
        Point::new(pad_outer_x, half_l),
    )?);
    fp.texts = texts(half_l + COURT_MARGIN, -half_l - COURT_MARGIN);
    finish(fp)
}

/// Body length of a TSSOP (JEDEC MO-153, 4.4 mm wide) by pin count.
fn tssop_body_length(pins: u32) -> Option<f64> {
    Some(match pins {
        8 => 3.0,
        14 | 16 => 5.0,
        20 => 6.5,
        24 => 7.8,
        28 => 9.7,
        _ => return None,
    })
}

/// A dual-row gull-wing package with pins numbered counter-clockwise from the top left.
struct Leads {
    pitch: Coord,
    tip_span: Coord,
    lead_w: Coord,
    foot: Coord,
}

fn dual_row(
    name: String,
    description: String,
    pins: u32,
    body: (Coord, Coord),
    leads: Leads,
) -> Result<Footprint> {
    let per_side = pins / 2;
    let pitch = leads.pitch;
    let (len, w, cx) = gull(pitch, leads.tip_span, leads.lead_w, leads.foot);
    let mut fp = Footprint::new(name);
    fp.description = description;
    fp.tags = "smd".into();
    fp.attributes.mount = MountType::Smd;
    for i in 0..per_side {
        let y = -along(i, per_side, pitch);
        fp.pads
            .push(Pad::smd((i + 1).to_string(), Point::new(-cx, y), len, w));
        fp.pads
            .push(Pad::smd((pins - i).to_string(), Point::new(cx, y), len, w));
    }
    let top = along(0, per_side, pitch).abs() + pitch / 2;
    let half_l = (body.1 / 2).max(top);
    finish_two_row(
        fp,
        body.0 / 2,
        half_l,
        cx + len / 2,
        along(0, per_side, pitch).abs(),
    )
}

/// TSSOP-`pins` (4.4 mm body, 0.65 mm pitch, 6.4 mm lead span; JEDEC MO-153).
pub fn tssop(pins: u32) -> Result<Footprint> {
    let Some(l) = tssop_body_length(pins) else {
        return bad("TSSOP is available with 8, 14, 16, 20, 24 or 28 pins");
    };
    dual_row(
        format!("TSSOP-{pins}_4.4x{l:.1}mm_P0.65mm"),
        format!("{pins}-pin thin shrink small outline package, 4.4 mm body, 0.65 mm pitch (JEDEC MO-153), nominal land pattern"),
        pins,
        (mm(4.4), mm(l)),
        Leads {
            pitch: mm(0.65),
            tip_span: mm(6.4),
            lead_w: mm(0.3),
            foot: mm(0.6),
        },
    )
}

/// MSOP-8 (3.0 x 3.0 mm body, 0.65 mm pitch, 4.9 mm lead span; JEDEC MO-187).
pub fn msop8() -> Result<Footprint> {
    dual_row(
        "MSOP-8_3x3mm_P0.65mm".into(),
        "8-pin mini small outline package, 3 x 3 mm body, 0.65 mm pitch (JEDEC MO-187), nominal land pattern".into(),
        8,
        (mm(3.0), mm(3.0)),
        Leads {
            pitch: mm(0.65),
            tip_span: mm(4.9),
            lead_w: mm(0.3),
            foot: mm(0.6),
        },
    )
}

/// Nominal outlines of the LQFP packages offered: `(pins per side, pitch, body, lead span)` in mm.
pub const LQFP_OUTLINES: [(u32, f64, f64, f64); 6] = [
    (8, 0.8, 7.0, 9.0),    // LQFP-32
    (11, 0.8, 10.0, 12.0), // LQFP-44
    (12, 0.5, 7.0, 9.0),   // LQFP-48
    (16, 0.5, 10.0, 12.0), // LQFP-64
    (25, 0.5, 14.0, 16.0), // LQFP-100
    (36, 0.5, 20.0, 22.0), // LQFP-144
];

/// LQFP-`4 × pins_per_side` (JEDEC MS-026): square body, gull-wing leads on four sides, pin 1 at the
/// top of the left side, counter-clockwise.
pub fn lqfp(pins_per_side: u32) -> Result<Footprint> {
    let Some(&(n, pitch_mm, body_mm, span_mm)) =
        LQFP_OUTLINES.iter().find(|o| o.0 == pins_per_side)
    else {
        return bad("LQFP is available with 32, 44, 48, 64, 100 or 144 pins");
    };
    let (pitch, body, span) = (mm(pitch_mm), mm(body_mm), mm(span_mm));
    let lead_w = if pitch_mm >= 0.8 { mm(0.37) } else { mm(0.22) };
    let (len, w, c) = gull(pitch, span, lead_w, mm(0.6));
    let total = 4 * n;
    let mut fp = Footprint::new(format!(
        "LQFP-{total}_{body_mm:.0}x{body_mm:.0}mm_P{pitch_mm:.2}mm"
    ));
    fp.description = format!(
        "{total}-pin low-profile quad flat package, {body_mm:.0} x {body_mm:.0} mm body, {pitch_mm:.2} mm pitch (JEDEC MS-026), nominal land pattern"
    );
    fp.tags = "lqfp qfp smd".into();
    fp.attributes.mount = MountType::Smd;
    for i in 0..n {
        fp.pads.push(Pad::smd(
            (i + 1).to_string(),
            Point::new(-c, -along(i, n, pitch)),
            len,
            w,
        ));
    }
    for i in 0..n {
        fp.pads.push(Pad::smd(
            (n + i + 1).to_string(),
            Point::new(along(i, n, pitch), -c),
            w,
            len,
        ));
    }
    for i in 0..n {
        fp.pads.push(Pad::smd(
            (2 * n + i + 1).to_string(),
            Point::new(c, along(i, n, pitch)),
            len,
            w,
        ));
    }
    for i in 0..n {
        fp.pads.push(Pad::smd(
            (3 * n + i + 1).to_string(),
            Point::new(-along(i, n, pitch), c),
            w,
            len,
        ));
    }
    let hb = body / 2;
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-hb, -hb),
        Point::new(hb, hb),
        FAB,
    ));
    let edge = c + len / 2;
    let k = 800_000.min(hb);
    for (sx, sy) in [(1, 1), (1, -1), (-1, -1)] {
        let (px, py) = (sx * (hb + 150_000), sy * (hb + 150_000));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(px, py),
            Point::new(px - sx * k, py),
            SILK,
        ));
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(px, py),
            Point::new(px, py - sy * k),
            SILK,
        ));
    }
    fp.graphics
        .extend(pin1_marker(Point::new(-edge - 100_000, hb), -1, 1));
    fp.graphics
        .push(courtyard(Point::new(-edge, -edge), Point::new(edge, edge))?);
    fp.texts = texts(edge + COURT_MARGIN, -edge - COURT_MARGIN);
    finish(fp)
}

/// SOT-23 with 3, 5 or 6 pins (JEDEC TO-236 / MO-178): 0.95 mm pitch, 2.4 mm lead span. Pins
/// 1, 2, 3 run left to right along the bottom; the others continue right to left along the top.
pub fn sot23(pins: u32) -> Result<Footprint> {
    if ![3, 5, 6].contains(&pins) {
        return bad("SOT-23 has 3, 5 or 6 pins");
    }
    let pitch = mm(0.95);
    let (len, w, c) = gull(pitch, mm(2.4), mm(0.4), mm(0.45));
    let mut fp = Footprint::new(format!("SOT-23-{pins}"));
    fp.description = format!(
        "SOT-23 with {pins} pins, 0.95 mm pitch, 2.9 mm body (JEDEC {}), nominal land pattern",
        if pins == 3 { "TO-236" } else { "MO-178" }
    );
    fp.tags = "sot23 smd".into();
    fp.attributes.mount = MountType::Smd;
    let xs = [-pitch, 0, pitch];
    // (number, x, y): bottom row first.
    let mut spots: Vec<(u32, Coord, Coord)> = Vec::new();
    match pins {
        3 => {
            spots.push((1, xs[0], -c));
            spots.push((2, xs[2], -c));
            spots.push((3, 0, c));
        }
        5 => {
            for (i, x) in xs.iter().enumerate() {
                spots.push((i as u32 + 1, *x, -c));
            }
            spots.push((4, xs[2], c));
            spots.push((5, xs[0], c));
        }
        _ => {
            for (i, x) in xs.iter().enumerate() {
                spots.push((i as u32 + 1, *x, -c));
            }
            for (i, x) in xs.iter().rev().enumerate() {
                spots.push((i as u32 + 4, *x, c));
            }
        }
    }
    for (n, x, y) in spots {
        fp.pads
            .push(Pad::smd(n.to_string(), Point::new(x, y), w, len));
    }
    let (hw, hl) = (mm(1.45), if pins == 3 { mm(0.65) } else { mm(0.8) });
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-hw, -hl),
        Point::new(hw, hl),
        FAB,
    ));
    for y in [hl + 100_000, -hl - 100_000] {
        fp.graphics.push(line(
            FpLayer::Silkscreen,
            Point::new(-hw, y),
            Point::new(hw, y),
            SILK,
        ));
    }
    let ey = c + len / 2;
    fp.graphics.extend(pin1_marker(
        Point::new(-pitch - w / 2 - 100_000, -ey),
        -1,
        -1,
    ));
    fp.graphics.push(courtyard(
        Point::new(-pitch - w / 2, -ey),
        Point::new(pitch + w / 2, ey),
    )?);
    fp.texts = texts(ey + COURT_MARGIN, -ey - COURT_MARGIN);
    finish(fp)
}

/// SOT-223 (JEDEC TO-261): 2.3 mm pitch, three leads on the bottom and the tab (pad 4) on top.
pub fn sot223() -> Result<Footprint> {
    let (len, w, c) = gull(mm(2.3), mm(7.0), mm(0.75), mm(1.0));
    let mut fp = Footprint::new("SOT-223-3");
    fp.description =
        "SOT-223 with 3 leads and a tab (pad 4), 2.3 mm pitch (JEDEC TO-261), nominal land pattern"
            .into();
    fp.tags = "sot223 smd".into();
    fp.attributes.mount = MountType::Smd;
    for (i, x) in [-mm(2.3), 0, mm(2.3)].into_iter().enumerate() {
        fp.pads
            .push(Pad::smd((i + 1).to_string(), Point::new(x, -c), w, len));
    }
    fp.pads.push(Pad::smd("4", Point::new(0, c), mm(3.4), len));
    let (hw, hl) = (mm(3.25), mm(1.75));
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-hw, -hl),
        Point::new(hw, hl),
        FAB,
    ));
    let ey = c + len / 2;
    fp.graphics.extend(pin1_marker(
        Point::new(-mm(2.3) - w / 2 - 100_000, -ey),
        -1,
        -1,
    ));
    fp.graphics.push(courtyard(
        Point::new(-mm(2.3) - w / 2, -ey),
        Point::new(mm(2.3) + w / 2, ey),
    )?);
    fp.texts = texts(ey + COURT_MARGIN, -ey - COURT_MARGIN);
    finish(fp)
}

/// Two-terminal diode packages. Pad 1 is the cathode (left), pad 2 the anode.
fn two_pad(
    name: &str,
    description: &str,
    body: (f64, f64),
    tip_span: f64,
    lead_w: f64,
    foot: f64,
) -> Result<Footprint> {
    let (len, w, c) = gull(mm(10.0), mm(tip_span), mm(lead_w), mm(foot));
    let mut fp = Footprint::new(name);
    fp.description = description.into();
    fp.tags = "diode smd".into();
    fp.attributes.mount = MountType::Smd;
    fp.pads.push(Pad::smd("1", Point::new(-c, 0), len, w));
    fp.pads.push(Pad::smd("2", Point::new(c, 0), len, w));
    let (hw, hl) = (mm(body.0 / 2.0), mm(body.1 / 2.0));
    fp.graphics.extend(rect(
        FpLayer::Fabrication,
        Point::new(-hw, -hl),
        Point::new(hw, hl),
        FAB,
    ));
    let ey = (hl.max(w / 2)) + 150_000;
    // The cathode bar on the silkscreen, beside the body.
    fp.graphics.push(line(
        FpLayer::Silkscreen,
        Point::new(-c - len / 2, ey),
        Point::new(c + len / 2, ey),
        SILK,
    ));
    fp.graphics.push(line(
        FpLayer::Silkscreen,
        Point::new(-c - len / 2, -ey),
        Point::new(c + len / 2, -ey),
        SILK,
    ));
    fp.graphics.push(line(
        FpLayer::Silkscreen,
        Point::new(-c - len / 2 - 150_000, -ey),
        Point::new(-c - len / 2 - 150_000, ey),
        SILK,
    ));
    fp.graphics.push(courtyard(
        Point::new(-c - len / 2, -ey),
        Point::new(c + len / 2, ey),
    )?);
    fp.texts = texts(ey + COURT_MARGIN, -ey - COURT_MARGIN);
    finish(fp)
}

/// SOD-123 (JEDEC MO-219): 2.7 x 1.6 mm body, 3.7 mm lead span.
pub fn sod123() -> Result<Footprint> {
    two_pad(
        "SOD-123",
        "SOD-123 diode package, 2.7 x 1.6 mm body, pad 1 = cathode, nominal land pattern",
        (2.7, 1.6),
        3.7,
        1.0,
        0.9,
    )
}

/// SMA / DO-214AC: 4.3 x 2.6 mm body, 5.3 mm lead span.
pub fn sma() -> Result<Footprint> {
    two_pad(
        "SMA_DO-214AC",
        "SMA (DO-214AC) diode package, 4.3 x 2.6 mm body, pad 1 = cathode, nominal land pattern",
        (4.3, 2.6),
        5.3,
        1.6,
        1.0,
    )
}

/// A non-plated mounting hole of `drill_mm` with a keep-out courtyard.
pub fn mounting_hole(drill_mm: f64) -> Result<Footprint> {
    let d = mm(drill_mm);
    let mut fp = Footprint::new(format!("MountingHole_{drill_mm:.1}mm"));
    fp.description = format!("Non-plated mounting hole, {drill_mm:.1} mm drill");
    fp.tags = "mounting hole".into();
    fp.attributes.mount = MountType::Unspecified;
    fp.pads.push(Pad::npth(Point::ORIGIN, d));
    let r = d / 2 + mm(1.0);
    fp.graphics.push(courtyard(
        Point::new(-r + COURT_MARGIN, -r + COURT_MARGIN),
        Point::new(r - COURT_MARGIN, r - COURT_MARGIN),
    )?);
    fp.texts = texts(r + COURT_MARGIN, -r - COURT_MARGIN);
    finish(fp)
}

/// A round SMD test point pad of `dia_mm`.
pub fn test_point(dia_mm: f64) -> Result<Footprint> {
    let d = mm(dia_mm);
    let mut fp = Footprint::new(format!("TestPoint_{dia_mm:.1}mm"));
    fp.description = format!("Round SMD test point pad, {dia_mm:.1} mm");
    fp.tags = "test point".into();
    fp.attributes.mount = MountType::Smd;
    let mut p = Pad::smd("1", Point::ORIGIN, d, d);
    p.shape = PadShape::Circle;
    p.paste = false;
    fp.pads.push(p);
    let r = d / 2;
    fp.graphics
        .push(courtyard(Point::new(-r, -r), Point::new(r, r))?);
    fp.texts = texts(r + COURT_MARGIN, -r - COURT_MARGIN);
    finish(fp)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod standard_package_tests {
    use super::*;

    fn overlaps(f: &Footprint) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (i, a) in f.pads.iter().enumerate() {
            for b in &f.pads[i + 1..] {
                let (al, ah) = a.bounds();
                let (bl, bh) = b.bounds();
                if al.x < bh.x && bl.x < ah.x && al.y < bh.y && bl.y < ah.y && a.number != b.number
                {
                    out.push((a.number.clone(), b.number.clone()));
                }
            }
        }
        out
    }

    #[test]
    fn every_standard_package_is_valid_with_distinct_non_overlapping_pads() {
        let mut all = vec![
            msop8().unwrap(),
            sot223().unwrap(),
            sod123().unwrap(),
            sma().unwrap(),
            mounting_hole(3.2).unwrap(),
            test_point(1.5).unwrap(),
        ];
        for p in [8, 14, 16, 20, 24, 28] {
            all.push(tssop(p).unwrap());
        }
        for p in [3, 5, 6] {
            all.push(sot23(p).unwrap());
        }
        for o in LQFP_OUTLINES {
            all.push(lqfp(o.0).unwrap());
        }
        for f in &all {
            assert!(overlaps(f).is_empty(), "{}: {:?}", f.name, overlaps(f));
            assert!(
                f.graphics.iter().any(|g| g.layer == FpLayer::Courtyard),
                "{} has no courtyard",
                f.name
            );
        }
    }

    #[test]
    fn pin_counts_and_numbering_follow_the_packages() {
        assert_eq!(tssop(20).unwrap().pads.len(), 20);
        assert_eq!(lqfp(12).unwrap().pads.len(), 48);
        assert_eq!(lqfp(8).unwrap().pads.len(), 32);
        assert_eq!(sot23(5).unwrap().pads.len(), 5);
        assert_eq!(sot223().unwrap().pads.len(), 4);
        // SOT-23-5: pins 1..3 along the bottom, left to right; 4 and 5 on top, right to left.
        let f = sot23(5).unwrap();
        let at = |n: &str| f.pads.iter().find(|p| p.number == n).unwrap().position;
        assert!(
            at("1").x < at("2").x
                && at("2").x < at("3").x
                && at("1").y == at("3").y
                && at("1").y < 0
        );
        assert!(at("4").x > at("5").x && at("4").y > 0);
        assert!(tssop(7).is_err() && sot23(4).is_err() && lqfp(9).is_err());
    }
}
