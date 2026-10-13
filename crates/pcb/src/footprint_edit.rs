//! Pure editing operations on a [`Footprint`], used by the Footprint Editor.
//!
//! Like `buturecad_schematic::symbol_edit`: every function mutates in place and is atomic (on
//! `Err` the footprint is unchanged); the desktop layer commits the result as one undoable
//! command. Items are addressed by index ([`Item`]); deleting renumbers them.

use crate::footprint::*;
use buturecad_geometry::{Arc, Circle, Coord, Point, Rectangle, Segment, Vector};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    Pad(usize),
    Graphic(usize),
    Text(usize),
}

/// Runs `f` on a clone and keeps the result only if it still validates.
fn edit<T>(
    fp: &mut Footprint,
    f: impl FnOnce(&mut Footprint) -> Result<T, FootprintError>,
) -> Result<T, FootprintError> {
    let mut work = fp.clone();
    let out = f(&mut work)?;
    validate(&work)?;
    *fp = work;
    Ok(out)
}

pub fn add_pad(fp: &mut Footprint, pad: Pad) -> Result<usize, FootprintError> {
    edit(fp, |f| {
        f.pads.push(pad);
        Ok(f.pads.len() - 1)
    })
}

pub fn add_graphic(fp: &mut Footprint, g: FpGraphic) -> Result<usize, FootprintError> {
    edit(fp, |f| {
        f.graphics.push(g);
        Ok(f.graphics.len() - 1)
    })
}

pub fn add_text(fp: &mut Footprint, t: FpText) -> Result<usize, FootprintError> {
    edit(fp, |f| {
        f.texts.push(t);
        Ok(f.texts.len() - 1)
    })
}

/// The next free numeric pin number (1, 2, 3…), ignoring non-numeric ones.
pub fn next_pad_number(fp: &Footprint) -> String {
    let used: BTreeSet<&str> = fp.pads.iter().map(|p| p.number.as_str()).collect();
    (1u32..)
        .map(|n| n.to_string())
        .find(|n| !used.contains(n.as_str()))
        .unwrap_or_else(|| "1".into())
}

fn idx(i: Item) -> usize {
    match i {
        Item::Pad(n) | Item::Graphic(n) | Item::Text(n) => n,
    }
}

/// Removes items; returns how many.
pub fn delete_items(fp: &mut Footprint, items: &[Item]) -> Result<usize, FootprintError> {
    edit(fp, |f| {
        let pick = |want: fn(&Item) -> bool| -> BTreeSet<usize> {
            items.iter().filter(|i| want(i)).map(|i| idx(*i)).collect()
        };
        let pads = pick(|i| matches!(i, Item::Pad(_)));
        let gfx = pick(|i| matches!(i, Item::Graphic(_)));
        let txt = pick(|i| matches!(i, Item::Text(_)));
        for (set, len) in [
            (&pads, f.pads.len()),
            (&gfx, f.graphics.len()),
            (&txt, f.texts.len()),
        ] {
            if let Some(bad) = set.iter().find(|i| **i >= len) {
                return Err(FootprintError::NoSuchItem(*bad));
            }
        }
        fn drop_idx<T>(v: &mut Vec<T>, set: &BTreeSet<usize>) {
            let mut i = 0;
            v.retain(|_| {
                i += 1;
                !set.contains(&(i - 1))
            });
        }
        drop_idx(&mut f.pads, &pads);
        drop_idx(&mut f.graphics, &gfx);
        drop_idx(&mut f.texts, &txt);
        Ok(pads.len() + gfx.len() + txt.len())
    })
}

// ---------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------

fn map_shape(s: &FpShape, f: &impl Fn(Point) -> Point, flips: bool) -> Option<FpShape> {
    Some(match s {
        FpShape::Line { segment, width } => FpShape::Line {
            segment: Segment::new(f(segment.a), f(segment.b)),
            width: *width,
        },
        FpShape::Rect {
            rect,
            filled,
            width,
        } => FpShape::Rect {
            rect: Rectangle::new(f(rect.min()), f(rect.max())).ok()?,
            filled: *filled,
            width: *width,
        },
        FpShape::Circle {
            circle,
            filled,
            width,
        } => FpShape::Circle {
            circle: Circle {
                center: f(circle.center),
                radius: circle.radius,
            },
            filled: *filled,
            width: *width,
        },
        FpShape::Arc { arc, width } => FpShape::Arc {
            arc: Arc::new(f(arc.center), f(arc.start), f(arc.end), arc.ccw != flips).ok()?,
            width: *width,
        },
        FpShape::Polygon {
            points,
            filled,
            width,
        } => FpShape::Polygon {
            points: points.iter().map(|p| f(*p)).collect(),
            filled: *filled,
            width: *width,
        },
    })
}

#[derive(Clone, Copy)]
enum Xform {
    Move(Vector),
    Rotate(Point),
    MirrorX(Coord),
    MirrorY(Coord),
}

impl Xform {
    fn point(self, p: Point) -> Point {
        match self {
            Xform::Move(v) => p + v,
            Xform::Rotate(c) => p.rotate_quarter_turns(c, 1),
            Xform::MirrorX(a) => Point::new(a.saturating_mul(2).saturating_sub(p.x), p.y),
            Xform::MirrorY(a) => Point::new(p.x, a.saturating_mul(2).saturating_sub(p.y)),
        }
    }
    fn flips(self) -> bool {
        matches!(self, Xform::MirrorX(_) | Xform::MirrorY(_))
    }
}

fn transform_pad(p: &mut Pad, x: Xform) {
    p.position = x.point(p.position);
    match x {
        Xform::Move(_) => {}
        Xform::Rotate(_) => p.rotation = (p.rotation + 1) % 4,
        Xform::MirrorX(_) | Xform::MirrorY(_) => {
            if p.shape == PadShape::Custom {
                // Bake the rotation into the outline, then mirror it in world axes.
                let r = p.rotation % 4;
                p.custom = p
                    .custom
                    .iter()
                    .map(|q| {
                        let q = q.rotate_quarter_turns(Point::ORIGIN, r);
                        match x {
                            Xform::MirrorX(_) => Point::new(-q.x, q.y),
                            _ => Point::new(q.x, -q.y),
                        }
                    })
                    .collect();
                p.rotation = 0;
            } else {
                // Only the parity of the rotation matters for the symmetric shapes.
                p.rotation = match x {
                    Xform::MirrorX(_) => (4 - p.rotation % 4) % 4,
                    _ => (6 - p.rotation % 4) % 4,
                };
            }
        }
    }
}

fn apply_transform(fp: &mut Footprint, items: &[Item], x: Xform) -> Result<(), FootprintError> {
    edit(fp, |f| {
        for it in items {
            match *it {
                Item::Pad(i) => {
                    let p = f.pads.get_mut(i).ok_or(FootprintError::NoSuchItem(i))?;
                    transform_pad(p, x);
                }
                Item::Graphic(i) => {
                    let g = f.graphics.get_mut(i).ok_or(FootprintError::NoSuchItem(i))?;
                    g.shape = map_shape(&g.shape, &|p| x.point(p), x.flips()).ok_or_else(|| {
                        FootprintError::Invalid("the transform would collapse a shape".into())
                    })?;
                }
                Item::Text(i) => {
                    let t = f.texts.get_mut(i).ok_or(FootprintError::NoSuchItem(i))?;
                    t.position = x.point(t.position);
                    if matches!(x, Xform::Rotate(_)) {
                        t.rotation = (t.rotation + 1) % 4;
                    }
                }
            }
        }
        Ok(())
    })
}

pub fn move_items(fp: &mut Footprint, items: &[Item], delta: Vector) -> Result<(), FootprintError> {
    apply_transform(fp, items, Xform::Move(delta))
}

/// Rotates items 90° counter-clockwise about `center`.
pub fn rotate_items(
    fp: &mut Footprint,
    items: &[Item],
    center: Point,
) -> Result<(), FootprintError> {
    apply_transform(fp, items, Xform::Rotate(center))
}

/// Mirrors across the vertical line `x = axis.x` when `horizontal`, else across `y = axis.y`.
pub fn mirror_items(
    fp: &mut Footprint,
    items: &[Item],
    axis: Point,
    horizontal: bool,
) -> Result<(), FootprintError> {
    apply_transform(
        fp,
        items,
        if horizontal {
            Xform::MirrorX(axis.x)
        } else {
            Xform::MirrorY(axis.y)
        },
    )
}

/// Bounds `(min, max)` of the selected items.
pub fn items_bounds(fp: &Footprint, items: &[Item]) -> Option<(Point, Point)> {
    let mut pts: Vec<Point> = Vec::new();
    for it in items {
        match *it {
            Item::Pad(i) => {
                if let Some(p) = fp.pads.get(i) {
                    let (lo, hi) = p.bounds();
                    pts.extend([lo, hi]);
                }
            }
            Item::Graphic(i) => {
                if let Some(g) = fp.graphics.get(i) {
                    match &g.shape {
                        FpShape::Line { segment, .. } => pts.extend([segment.a, segment.b]),
                        FpShape::Rect { rect, .. } => pts.extend([rect.min(), rect.max()]),
                        FpShape::Circle { circle, .. } => pts.extend([
                            Point::new(
                                circle.center.x - circle.radius,
                                circle.center.y - circle.radius,
                            ),
                            Point::new(
                                circle.center.x + circle.radius,
                                circle.center.y + circle.radius,
                            ),
                        ]),
                        FpShape::Arc { arc, .. } => pts.extend([arc.center, arc.start, arc.end]),
                        FpShape::Polygon { points, .. } => pts.extend(points.iter().copied()),
                    }
                }
            }
            Item::Text(i) => {
                if let Some(t) = fp.texts.get(i) {
                    pts.push(t.position);
                }
            }
        }
    }
    let lo = Point::new(
        pts.iter().map(|p| p.x).min()?,
        pts.iter().map(|p| p.y).min()?,
    );
    let hi = Point::new(
        pts.iter().map(|p| p.x).max()?,
        pts.iter().map(|p| p.y).max()?,
    );
    Some((lo, hi))
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

pub fn pad_kind_id(k: PadKind) -> &'static str {
    match k {
        PadKind::Smd => "smd",
        PadKind::Through => "through",
        PadKind::NonPlated => "non_plated",
    }
}
pub fn pad_kind_label(k: PadKind) -> &'static str {
    match k {
        PadKind::Smd => "SMD",
        PadKind::Through => "Through-hole",
        PadKind::NonPlated => "NPTH (mounting hole)",
    }
}
pub fn parse_pad_kind(s: &str) -> Option<PadKind> {
    Some(match s {
        "smd" => PadKind::Smd,
        "through" => PadKind::Through,
        "non_plated" => PadKind::NonPlated,
        _ => return None,
    })
}
pub fn pad_shape_id(s: PadShape) -> &'static str {
    match s {
        PadShape::Circle => "circle",
        PadShape::Rect => "rect",
        PadShape::Oval => "oval",
        PadShape::RoundRect => "round_rect",
        PadShape::Custom => "custom",
    }
}
pub fn pad_shape_label(s: PadShape) -> &'static str {
    match s {
        PadShape::Circle => "Circle",
        PadShape::Rect => "Rectangle",
        PadShape::Oval => "Oval",
        PadShape::RoundRect => "Rounded rectangle",
        PadShape::Custom => "Custom polygon",
    }
}
pub fn parse_pad_shape(s: &str) -> Option<PadShape> {
    Some(match s {
        "circle" => PadShape::Circle,
        "rect" => PadShape::Rect,
        "oval" => PadShape::Oval,
        "round_rect" => PadShape::RoundRect,
        "custom" => PadShape::Custom,
        _ => return None,
    })
}
pub const PAD_KINDS: [PadKind; 3] = [PadKind::Smd, PadKind::Through, PadKind::NonPlated];
pub const PAD_SHAPES: [PadShape; 5] = [
    PadShape::Circle,
    PadShape::Rect,
    PadShape::Oval,
    PadShape::RoundRect,
    PadShape::Custom,
];

fn parse<T: std::str::FromStr>(value: &str, what: &str) -> Result<T, FootprintError> {
    value
        .trim()
        .parse()
        .map_err(|_| FootprintError::Invalid(format!("{what} must be a whole number")))
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim(), "true" | "1" | "yes")
}

/// Sets a pad field. Keys: `number`, `kind`, `shape`, `x`, `y`, `size_x`, `size_y`, `rotation`
/// (degrees: 0, 90, 180, 270), `drill` (nm; empty or 0 removes it), `slot` (nm; empty removes),
/// `roundrect_ratio` (per mille), `mask_margin` / `paste_margin` (nm; empty = board default),
/// `paste` (bool). Lengths are nm.
pub fn set_pad_field(
    fp: &mut Footprint,
    index: usize,
    key: &str,
    value: &str,
) -> Result<(), FootprintError> {
    edit(fp, |f| {
        let p = f
            .pads
            .get_mut(index)
            .ok_or(FootprintError::NoSuchItem(index))?;
        let opt_len = |v: &str, what: &str| -> Result<Option<Coord>, FootprintError> {
            if v.trim().is_empty() {
                Ok(None)
            } else {
                parse(v, what).map(Some)
            }
        };
        match key {
            "number" => p.number = value.trim().to_owned(),
            "kind" => {
                let kind = parse_pad_kind(value).ok_or_else(|| {
                    FootprintError::Invalid(format!("unknown pad kind “{value}”"))
                })?;
                p.kind = kind;
                match kind {
                    PadKind::Smd => p.drill = None,
                    PadKind::Through | PadKind::NonPlated if p.drill.is_none() => {
                        let d = (p.size_x.min(p.size_y) * 5 / 10).max(100_000);
                        p.drill = Some(Drill {
                            diameter: d,
                            slot: None,
                        });
                    }
                    _ => {}
                }
                if kind == PadKind::NonPlated {
                    p.number.clear();
                    p.paste = false;
                }
            }
            "shape" => {
                let shape = parse_pad_shape(value).ok_or_else(|| {
                    FootprintError::Invalid(format!("unknown pad shape “{value}”"))
                })?;
                if shape == PadShape::Custom && p.custom.is_empty() {
                    let (hx, hy) = (p.size_x / 2, p.size_y / 2);
                    p.custom = vec![
                        Point::new(-hx, -hy),
                        Point::new(hx, -hy),
                        Point::new(hx, hy),
                        Point::new(-hx, hy),
                    ];
                }
                if shape != PadShape::Custom {
                    p.custom.clear();
                }
                if shape == PadShape::Circle {
                    let d = p.size_x.max(p.size_y);
                    p.size_x = d;
                    p.size_y = d;
                }
                if shape == PadShape::RoundRect && p.roundrect_ratio == 0 {
                    p.roundrect_ratio = 250;
                }
                p.shape = shape;
            }
            "x" => p.position.x = parse(value, "x")?,
            "y" => p.position.y = parse(value, "y")?,
            "size_x" => {
                p.size_x = parse(value, "width")?;
                if p.shape == PadShape::Circle {
                    p.size_y = p.size_x;
                }
            }
            "size_y" => {
                p.size_y = parse(value, "height")?;
                if p.shape == PadShape::Circle {
                    p.size_x = p.size_y;
                }
            }
            "rotation" => {
                let deg: i32 = parse(value, "rotation")?;
                if deg % 90 != 0 {
                    return Err(FootprintError::Invalid(
                        "pad rotation is a multiple of 90 degrees".into(),
                    ));
                }
                p.rotation = (deg / 90).rem_euclid(4) as u8;
            }
            "drill" => {
                p.drill = match opt_len(value, "drill")? {
                    Some(d) if d > 0 => Some(Drill {
                        diameter: d,
                        slot: p.drill.and_then(|x| x.slot),
                    }),
                    _ => None,
                }
            }
            "slot" => {
                let slot = opt_len(value, "slot length")?;
                match p.drill.as_mut() {
                    Some(d) => d.slot = slot.filter(|s| *s > 0),
                    None => return Err(FootprintError::Invalid("a slot needs a drill".into())),
                }
            }
            "roundrect_ratio" => p.roundrect_ratio = parse(value, "corner ratio")?,
            "mask_margin" => p.mask_margin = opt_len(value, "mask margin")?,
            "paste_margin" => p.paste_margin = opt_len(value, "paste margin")?,
            "paste" => p.paste = parse_bool(value),
            other => return Err(FootprintError::UnknownField(other.to_owned())),
        }
        Ok(())
    })
}

/// Sets a graphic field. Keys: `layer`, `width`, `filled`.
pub fn set_graphic_field(
    fp: &mut Footprint,
    index: usize,
    key: &str,
    value: &str,
) -> Result<(), FootprintError> {
    edit(fp, |f| {
        let g = f
            .graphics
            .get_mut(index)
            .ok_or(FootprintError::NoSuchItem(index))?;
        match key {
            "layer" => {
                g.layer = FpLayer::parse(value.trim())
                    .ok_or_else(|| FootprintError::Invalid(format!("unknown layer “{value}”")))?
            }
            "width" => {
                let w: Coord = parse(value, "line width")?;
                match &mut g.shape {
                    FpShape::Line { width, .. }
                    | FpShape::Rect { width, .. }
                    | FpShape::Circle { width, .. }
                    | FpShape::Arc { width, .. }
                    | FpShape::Polygon { width, .. } => *width = w,
                }
            }
            "filled" => match &mut g.shape {
                FpShape::Rect { filled, .. }
                | FpShape::Circle { filled, .. }
                | FpShape::Polygon { filled, .. } => *filled = parse_bool(value),
                _ => {
                    return Err(FootprintError::Invalid(
                        "only rectangles, circles and polygons can be filled".into(),
                    ))
                }
            },
            other => return Err(FootprintError::UnknownField(other.to_owned())),
        }
        Ok(())
    })
}

/// Sets a text field. Keys: `text`, `layer`, `height`, `kind`, `x`, `y`.
pub fn set_text_field(
    fp: &mut Footprint,
    index: usize,
    key: &str,
    value: &str,
) -> Result<(), FootprintError> {
    edit(fp, |f| {
        let t = f
            .texts
            .get_mut(index)
            .ok_or(FootprintError::NoSuchItem(index))?;
        match key {
            "text" => t.text = value.to_owned(),
            "layer" => {
                t.layer = FpLayer::parse(value.trim())
                    .ok_or_else(|| FootprintError::Invalid(format!("unknown layer “{value}”")))?
            }
            "height" => t.height = parse(value, "text height")?,
            "x" => t.position.x = parse(value, "x")?,
            "y" => t.position.y = parse(value, "y")?,
            "kind" => {
                t.kind = match value.trim() {
                    "reference" => TextKind::Reference,
                    "value" => TextKind::Value,
                    "user" => TextKind::User,
                    other => {
                        return Err(FootprintError::Invalid(format!(
                            "unknown text kind “{other}”"
                        )))
                    }
                }
            }
            other => return Err(FootprintError::UnknownField(other.to_owned())),
        }
        Ok(())
    })
}

/// Sets a footprint-level field. Keys: `name`, `description`, `tags`, `mount`
/// (`unspecified|smd|through_hole`), `exclude_from_bom`, `exclude_from_position_files`, and
/// `model:<n>:path|x|y|z|rx|ry|rz|sx|sy|sz` for 3D model reference *n* (0-based; x/y/z nm,
/// rx/ry/rz millidegrees, sx/sy/sz ppm).
pub fn set_footprint_field(
    fp: &mut Footprint,
    key: &str,
    value: &str,
) -> Result<(), FootprintError> {
    edit(fp, |f| {
        match key {
            "name" => f.name = value.trim().to_owned(),
            "description" => f.description = value.to_owned(),
            "tags" => f.tags = value.to_owned(),
            "mount" => {
                f.attributes.mount = match value.trim() {
                    "unspecified" => MountType::Unspecified,
                    "smd" => MountType::Smd,
                    "through_hole" => MountType::ThroughHole,
                    other => {
                        return Err(FootprintError::Invalid(format!(
                            "unknown mount type “{other}”"
                        )))
                    }
                }
            }
            "exclude_from_bom" => f.attributes.exclude_from_bom = parse_bool(value),
            "exclude_from_position_files" => {
                f.attributes.exclude_from_position_files = parse_bool(value)
            }
            other => {
                let Some(rest) = other.strip_prefix("model:") else {
                    return Err(FootprintError::UnknownField(other.to_owned()));
                };
                let (n, field) = rest
                    .split_once(':')
                    .ok_or_else(|| FootprintError::UnknownField(other.to_owned()))?;
                let n: usize = parse(n, "model number")?;
                let m = f.models.get_mut(n).ok_or(FootprintError::NoSuchItem(n))?;
                let axis = |c: &str| match c {
                    "x" | "rx" | "sx" => Some(0),
                    "y" | "ry" | "sy" => Some(1),
                    "z" | "rz" | "sz" => Some(2),
                    _ => None,
                };
                match field {
                    "path" => m.path = value.trim().to_owned(),
                    "x" | "y" | "z" => m.offset[axis(field).unwrap_or(0)] = parse(value, "offset")?,
                    "rx" | "ry" | "rz" => {
                        m.rotation_mdeg[axis(field).unwrap_or(0)] = parse(value, "rotation")?
                    }
                    "sx" | "sy" | "sz" => {
                        m.scale_ppm[axis(field).unwrap_or(0)] = parse(value, "scale")?
                    }
                    _ => return Err(FootprintError::UnknownField(other.to_owned())),
                }
            }
        }
        Ok(())
    })
}

/// Replaces the courtyard with one rectangle around all pads plus `margin`, snapped outwards to a
/// 0.05 mm grid. Existing courtyard graphics are removed. Errors if there are no pads.
pub fn set_courtyard_from_pads(fp: &mut Footprint, margin: Coord) -> Result<(), FootprintError> {
    edit(fp, |f| {
        let (lo, hi) = f
            .pad_bounds()
            .ok_or_else(|| FootprintError::Invalid("the footprint has no pads".into()))?;
        let g: Coord = 50_000;
        let down = |v: Coord| v.div_euclid(g) * g;
        let up = |v: Coord| -((-v).div_euclid(g) * g);
        let rect = Rectangle::new(
            Point::new(down(lo.x - margin), down(lo.y - margin)),
            Point::new(up(hi.x + margin), up(hi.y + margin)),
        )
        .map_err(|_| FootprintError::Invalid("the courtyard would be empty".into()))?;
        f.graphics.retain(|g| g.layer != FpLayer::Courtyard);
        f.graphics.push(FpGraphic {
            layer: FpLayer::Courtyard,
            shape: FpShape::Rect {
                rect,
                filled: false,
                width: 50_000,
            },
        });
        Ok(())
    })
}

/// Adds a 3D model reference (stored only; nothing renders it yet).
pub fn add_model(fp: &mut Footprint, path: &str) -> Result<usize, FootprintError> {
    edit(fp, |f| {
        f.models.push(Model3d::new(path.trim()));
        Ok(f.models.len() - 1)
    })
}

pub fn remove_model(fp: &mut Footprint, index: usize) -> Result<(), FootprintError> {
    edit(fp, |f| {
        if index >= f.models.len() {
            return Err(FootprintError::NoSuchItem(index));
        }
        f.models.remove(index);
        Ok(())
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn fp() -> Footprint {
        let mut f = Footprint::new("T:X");
        add_pad(&mut f, Pad::smd("1", Point::new(0, 0), 1_000_000, 500_000)).unwrap();
        f
    }

    #[test]
    fn failed_edits_leave_the_footprint_unchanged() {
        let mut f = fp();
        let before = f.clone();
        assert!(set_pad_field(&mut f, 0, "size_x", "0").is_err());
        assert!(set_pad_field(&mut f, 0, "kind", "through").is_ok());
        let _ = before;
        let mut g = fp();
        let before = g.clone();
        assert!(set_pad_field(&mut g, 0, "bogus", "1").is_err());
        assert!(set_pad_field(&mut g, 5, "number", "1").is_err());
        assert!(set_pad_field(&mut g, 0, "rotation", "45").is_err());
        assert_eq!(g, before);
    }

    #[test]
    fn changing_kind_and_shape_keeps_the_pad_valid() {
        let mut f = fp();
        set_pad_field(&mut f, 0, "kind", "through").unwrap();
        let d = f.pads[0].drill.unwrap().diameter;
        assert!(
            d > 0 && d < 500_000,
            "a default drill that leaves a ring: {d}"
        );
        set_pad_field(&mut f, 0, "shape", "circle").unwrap();
        assert_eq!(f.pads[0].size_x, f.pads[0].size_y);
        set_pad_field(&mut f, 0, "shape", "custom").unwrap();
        assert!(f.pads[0].custom.len() >= 3);
        set_pad_field(&mut f, 0, "shape", "round_rect").unwrap();
        assert!(f.pads[0].custom.is_empty() && f.pads[0].roundrect_ratio == 250);
        set_pad_field(&mut f, 0, "kind", "non_plated").unwrap();
        assert!(f.pads[0].number.is_empty() && !f.pads[0].paste);
        set_pad_field(&mut f, 0, "kind", "smd").unwrap();
        assert!(f.pads[0].drill.is_none());
    }

    #[test]
    fn rotate_and_mirror_are_exact_and_reversible() {
        let mut f = fp();
        add_graphic(
            &mut f,
            FpGraphic {
                layer: FpLayer::Silkscreen,
                shape: FpShape::Line {
                    segment: Segment::new(Point::new(0, 0), Point::new(2_000_000, 1_000_000)),
                    width: 120_000,
                },
            },
        )
        .unwrap();
        let items = [Item::Pad(0), Item::Graphic(0)];
        let original = f.clone();
        for _ in 0..4 {
            rotate_items(&mut f, &items, Point::new(500_000, 0)).unwrap();
        }
        assert_eq!(f, original);
        mirror_items(&mut f, &items, Point::new(1_000_000, 0), true).unwrap();
        mirror_items(&mut f, &items, Point::new(1_000_000, 0), true).unwrap();
        assert_eq!(f, original);
        mirror_items(&mut f, &items, Point::new(0, 0), false).unwrap();
        mirror_items(&mut f, &items, Point::new(0, 0), false).unwrap();
        assert_eq!(f, original);
        // A quarter turn swaps the apparent pad size.
        rotate_items(&mut f, &[Item::Pad(0)], Point::ORIGIN).unwrap();
        assert_eq!(f.pads[0].rotated_size(), (500_000, 1_000_000));
    }

    #[test]
    fn mirroring_a_custom_pad_mirrors_its_outline() {
        let mut f = Footprint::new("T:C");
        let mut p = Pad::smd("1", Point::new(0, 0), 1_000_000, 1_000_000);
        p.shape = PadShape::Custom;
        p.custom = vec![
            Point::new(0, 0),
            Point::new(2_000_000, 0),
            Point::new(0, 1_000_000),
        ];
        p.rotation = 1;
        add_pad(&mut f, p).unwrap();
        let before = f.pads[0].bounds();
        mirror_items(&mut f, &[Item::Pad(0)], Point::new(0, 0), true).unwrap();
        let after = f.pads[0].bounds();
        // Bounds mirror about x = 0 (the pad was rotated 90°, so its extent is 1 mm × 2 mm).
        assert_eq!((after.0.x, after.1.x), (-before.1.x, -before.0.x));
        assert_eq!((after.0.y, after.1.y), (before.0.y, before.1.y));
    }

    #[test]
    fn delete_and_numbering() {
        let mut f = fp();
        add_pad(&mut f, Pad::smd("2", Point::new(1, 0), 1_000_000, 500_000)).unwrap();
        add_pad(&mut f, Pad::smd("4", Point::new(2, 0), 1_000_000, 500_000)).unwrap();
        assert_eq!(next_pad_number(&f), "3");
        assert_eq!(delete_items(&mut f, &[Item::Pad(1)]), Ok(1));
        assert_eq!(f.pads.len(), 2);
        assert!(delete_items(&mut f, &[Item::Pad(9)]).is_err());
    }

    #[test]
    fn auto_courtyard_encloses_the_pads_on_the_grid() {
        let mut f = fp();
        add_pad(
            &mut f,
            Pad::smd("2", Point::new(2_030_000, 0), 1_000_000, 500_000),
        )
        .unwrap();
        set_courtyard_from_pads(&mut f, 250_000).unwrap();
        set_courtyard_from_pads(&mut f, 250_000).unwrap(); // idempotent: replaces, never stacks
        let courts: Vec<_> = f
            .graphics
            .iter()
            .filter(|g| g.layer == FpLayer::Courtyard)
            .collect();
        assert_eq!(courts.len(), 1);
        let FpShape::Rect { rect, .. } = &courts[0].shape else {
            panic!("not a rectangle")
        };
        assert_eq!((rect.min().x % 50_000, rect.max().y % 50_000), (0, 0));
        assert!(rect.min().x <= -750_000 && rect.max().x >= 2_780_000);
        assert!(warnings(&f).iter().all(|w| !w.contains("courtyard")));
        assert!(set_courtyard_from_pads(&mut Footprint::new("E"), 0).is_err());
    }

    #[test]
    fn model_references_are_stored_and_validated() {
        let mut f = fp();
        let n = add_model(&mut f, "models/r0603.step").unwrap();
        set_footprint_field(&mut f, &format!("model:{n}:z"), "250000").unwrap();
        set_footprint_field(&mut f, &format!("model:{n}:rz"), "90000").unwrap();
        assert_eq!(
            (f.models[0].offset[2], f.models[0].rotation_mdeg[2]),
            (250_000, 90_000)
        );
        assert!(set_footprint_field(&mut f, "model:0:sx", "0").is_err());
        assert!(set_footprint_field(&mut f, "model:5:path", "x").is_err());
        assert!(add_model(&mut f, "  ").is_err());
        remove_model(&mut f, 0).unwrap();
        assert!(f.models.is_empty());
    }
}
