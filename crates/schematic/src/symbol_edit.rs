//! Pure editing operations on a library [`Symbol`], used by the Symbol Editor.
//!
//! Every function mutates a symbol *in place* and is atomic: on `Err` the symbol is unchanged.
//! The desktop layer clones the stored symbol, applies operations to the clone and commits the
//! result through the Command Engine, so each edit is one undo step.
//!
//! Items are addressed by index ([`Item`]); operations that delete items renumber them, so
//! callers must re-derive their selection from the returned values.

use crate::model::*;
use buturecad_geometry::{Arc, Bounded, Circle, Coord, Point, Rectangle, Segment, Vector};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum SymbolError {
    #[error("symbol name must not be empty")]
    EmptyName,
    #[error("symbol name contains characters that are not allowed ({0})")]
    BadName(String),
    #[error("a symbol needs 1 to {MAX_UNITS} units")]
    BadUnitCount,
    #[error("unit {unit} does not exist (the symbol has {count})")]
    NoSuchUnit { unit: u8, count: u8 },
    #[error("representation must be 0 (both), 1 (normal) or 2 (alternate)")]
    BadRepresentation,
    #[error("item {0} does not exist")]
    NoSuchItem(usize),
    #[error("pin number must not be empty")]
    EmptyPinNumber,
    #[error("pin number “{0}” is already used in this unit")]
    DuplicatePinNumber(String),
    #[error("pin length must not be negative")]
    NegativePinLength,
    #[error("{0}")]
    Invalid(String),
    #[error("unknown field “{0}”")]
    UnknownField(String),
    #[error("units {0} still contain items; delete them or move them first")]
    UnitInUse(String),
}

/// A selectable part of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    Pin(usize),
    Graphic(usize),
}

/// Checks a symbol name (`Library:Name` ids are allowed): non-empty, no control characters and
/// none of the characters that are unsafe in file names or search patterns.
pub fn validate_name(name: &str) -> Result<(), SymbolError> {
    if name.trim().is_empty() {
        return Err(SymbolError::EmptyName);
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || matches!(c, '/' | '\\' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(SymbolError::BadName(format!("{c:?}")));
    }
    Ok(())
}

fn scopes_overlap(a: u8, b: u8) -> bool {
    a == 0 || b == 0 || a == b
}

/// Validates the invariants every stored symbol must satisfy.
pub fn validate(sym: &Symbol) -> Result<(), SymbolError> {
    validate_name(&sym.name)?;
    if sym.unit_count == 0 || sym.unit_count > MAX_UNITS {
        return Err(SymbolError::BadUnitCount);
    }
    let check_scope = |unit: u8, repr: u8| -> Result<(), SymbolError> {
        if unit > sym.unit_count {
            return Err(SymbolError::NoSuchUnit {
                unit,
                count: sym.unit_count,
            });
        }
        if repr > REPR_ALTERNATE {
            return Err(SymbolError::BadRepresentation);
        }
        Ok(())
    };
    for (i, p) in sym.pins.iter().enumerate() {
        check_scope(p.unit, p.representation)?;
        if p.number.trim().is_empty() {
            return Err(SymbolError::EmptyPinNumber);
        }
        if p.length < 0 {
            return Err(SymbolError::NegativePinLength);
        }
        for q in &sym.pins[..i] {
            if q.number == p.number
                && scopes_overlap(p.unit, q.unit)
                && scopes_overlap(p.representation, q.representation)
            {
                return Err(SymbolError::DuplicatePinNumber(p.number.clone()));
            }
        }
    }
    for g in &sym.graphics {
        check_scope(g.unit, g.representation)?;
    }
    Ok(())
}

/// Runs `f` on a clone and commits it only if the result still validates.
fn edit<T>(
    sym: &mut Symbol,
    f: impl FnOnce(&mut Symbol) -> Result<T, SymbolError>,
) -> Result<T, SymbolError> {
    let mut work = sym.clone();
    let out = f(&mut work)?;
    validate(&work)?;
    *sym = work;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Adding and removing
// ---------------------------------------------------------------------------------------------

/// Adds a pin; returns its index.
pub fn add_pin(sym: &mut Symbol, pin: Pin) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        s.pins.push(pin);
        Ok(s.pins.len() - 1)
    })
}

/// Adds a graphic; returns its index.
pub fn add_graphic(sym: &mut Symbol, graphic: SymbolGraphic) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        s.graphics.push(graphic);
        Ok(s.graphics.len() - 1)
    })
}

/// The smallest positive integer not yet used as a pin number in `representation`. Numbers are
/// unique across *all* units (each unit's pins are different physical pins); the alternate
/// representation deliberately reuses the numbers of the normal one.
pub fn next_pin_number(sym: &Symbol, representation: u8) -> String {
    let used: BTreeSet<&str> = sym
        .pins
        .iter()
        .filter(|p| scopes_overlap(p.representation, representation))
        .map(|p| p.number.as_str())
        .collect();
    (1u32..)
        .map(|n| n.to_string())
        .find(|n| !used.contains(n.as_str()))
        .unwrap_or_else(|| "1".into())
}

/// Removes items. Returns how many were removed.
pub fn delete_items(sym: &mut Symbol, items: &[Item]) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        let pins: BTreeSet<usize> = items
            .iter()
            .filter(|i| matches!(i, Item::Pin(_)))
            .map(|i| item_index(*i))
            .collect();
        let gfx: BTreeSet<usize> = items
            .iter()
            .filter(|i| matches!(i, Item::Graphic(_)))
            .map(|i| item_index(*i))
            .collect();
        if let Some(bad) = pins.iter().find(|i| **i >= s.pins.len()) {
            return Err(SymbolError::NoSuchItem(*bad));
        }
        if let Some(bad) = gfx.iter().find(|i| **i >= s.graphics.len()) {
            return Err(SymbolError::NoSuchItem(*bad));
        }
        let mut i = 0;
        s.pins.retain(|_| {
            i += 1;
            !pins.contains(&(i - 1))
        });
        let mut j = 0;
        s.graphics.retain(|_| {
            j += 1;
            !gfx.contains(&(j - 1))
        });
        Ok(pins.len() + gfx.len())
    })
}

fn item_index(i: Item) -> usize {
    match i {
        Item::Pin(n) | Item::Graphic(n) => n,
    }
}

// ---------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------

fn map_graphic(g: &Graphic, f: &impl Fn(Point) -> Point, flips: bool) -> Option<Graphic> {
    Some(match g {
        Graphic::Line { segment, width } => Graphic::Line {
            segment: Segment::new(f(segment.a), f(segment.b)),
            width: *width,
        },
        Graphic::Rect {
            rect,
            filled,
            width,
        } => Graphic::Rect {
            rect: Rectangle::new(f(rect.min()), f(rect.max())).ok()?,
            filled: *filled,
            width: *width,
        },
        Graphic::Circle {
            circle,
            filled,
            width,
        } => Graphic::Circle {
            circle: Circle {
                center: f(circle.center),
                radius: circle.radius,
            },
            filled: *filled,
            width: *width,
        },
        Graphic::Arc { arc, width } => Graphic::Arc {
            arc: Arc::new(f(arc.center), f(arc.start), f(arc.end), arc.ccw != flips).ok()?,
            width: *width,
        },
        Graphic::Text {
            origin,
            height,
            text,
        } => Graphic::Text {
            origin: f(*origin),
            height: *height,
            text: text.clone(),
        },
    })
}

fn rotate_direction(d: Direction) -> Direction {
    match d {
        Direction::Right => Direction::Up,
        Direction::Up => Direction::Left,
        Direction::Left => Direction::Down,
        Direction::Down => Direction::Right,
    }
}

fn mirror_direction_x(d: Direction) -> Direction {
    match d {
        Direction::Right => Direction::Left,
        Direction::Left => Direction::Right,
        other => other,
    }
}

fn mirror_direction_y(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        other => other,
    }
}

fn apply_transform(
    sym: &mut Symbol,
    items: &[Item],
    f: impl Fn(Point) -> Point,
    flips: bool,
    pin_dir: impl Fn(Direction) -> Direction,
) -> Result<(), SymbolError> {
    edit(sym, |s| {
        for it in items {
            match *it {
                Item::Pin(i) => {
                    let p = s.pins.get_mut(i).ok_or(SymbolError::NoSuchItem(i))?;
                    p.position = f(p.position);
                    p.orientation = pin_dir(p.orientation);
                }
                Item::Graphic(i) => {
                    let g = s.graphics.get_mut(i).ok_or(SymbolError::NoSuchItem(i))?;
                    g.graphic = map_graphic(&g.graphic, &f, flips).ok_or_else(|| {
                        SymbolError::Invalid("the transform would collapse a shape".into())
                    })?;
                }
            }
        }
        Ok(())
    })
}

/// Translates items.
pub fn move_items(sym: &mut Symbol, items: &[Item], delta: Vector) -> Result<(), SymbolError> {
    apply_transform(sym, items, |p| p + delta, false, |d| d)
}

/// Rotates items 90° counter-clockwise about `center`.
pub fn rotate_items(sym: &mut Symbol, items: &[Item], center: Point) -> Result<(), SymbolError> {
    apply_transform(
        sym,
        items,
        |p| p.rotate_quarter_turns(center, 1),
        false,
        rotate_direction,
    )
}

/// Mirrors items across the vertical line `x = axis_x` (left ↔ right) when `horizontal`,
/// otherwise across the horizontal line `y = axis_y`.
pub fn mirror_items(
    sym: &mut Symbol,
    items: &[Item],
    axis: Point,
    horizontal: bool,
) -> Result<(), SymbolError> {
    if horizontal {
        apply_transform(
            sym,
            items,
            |p| Point::new(axis.x.saturating_mul(2).saturating_sub(p.x), p.y),
            true,
            mirror_direction_x,
        )
    } else {
        apply_transform(
            sym,
            items,
            |p| Point::new(p.x, axis.y.saturating_mul(2).saturating_sub(p.y)),
            true,
            mirror_direction_y,
        )
    }
}

/// Bounding points of the selected items (for rotate / mirror pivots).
pub fn items_bounds(sym: &Symbol, items: &[Item]) -> Option<(Point, Point)> {
    let mut pts: Vec<Point> = Vec::new();
    for it in items {
        match *it {
            Item::Pin(i) => {
                if let Some(p) = sym.pins.get(i) {
                    pts.push(p.position);
                }
            }
            Item::Graphic(i) => {
                if let Some(g) = sym.graphics.get(i) {
                    match &g.graphic {
                        Graphic::Line { segment, .. } => pts.extend([segment.a, segment.b]),
                        Graphic::Rect { rect, .. } => pts.extend([rect.min(), rect.max()]),
                        Graphic::Circle { circle, .. } => {
                            let bb = circle.bounding_box();
                            pts.extend([bb.min, bb.max]);
                        }
                        Graphic::Arc { arc, .. } => {
                            let bb = arc.bounding_box();
                            pts.extend([bb.min, bb.max]);
                        }
                        Graphic::Text { origin, .. } => pts.push(*origin),
                    }
                }
            }
        }
    }
    let min_x = pts.iter().map(|p| p.x).min()?;
    let min_y = pts.iter().map(|p| p.y).min()?;
    let max_x = pts.iter().map(|p| p.x).max()?;
    let max_y = pts.iter().map(|p| p.y).max()?;
    Some((Point::new(min_x, min_y), Point::new(max_x, max_y)))
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

pub fn parse_pin_type(s: &str) -> Option<PinType> {
    Some(match s {
        "input" => PinType::Input,
        "output" => PinType::Output,
        "bidirectional" => PinType::Bidirectional,
        "tri_state" => PinType::TriState,
        "passive" => PinType::Passive,
        "power_input" => PinType::PowerInput,
        "power_output" => PinType::PowerOutput,
        "open_collector" => PinType::OpenCollector,
        "open_emitter" => PinType::OpenEmitter,
        "unspecified" => PinType::Unspecified,
        "no_connect" => PinType::NoConnect,
        _ => return None,
    })
}

pub fn pin_type_id(t: PinType) -> &'static str {
    match t {
        PinType::Input => "input",
        PinType::Output => "output",
        PinType::Bidirectional => "bidirectional",
        PinType::TriState => "tri_state",
        PinType::Passive => "passive",
        PinType::PowerInput => "power_input",
        PinType::PowerOutput => "power_output",
        PinType::OpenCollector => "open_collector",
        PinType::OpenEmitter => "open_emitter",
        PinType::Unspecified => "unspecified",
        PinType::NoConnect => "no_connect",
    }
}

pub fn pin_type_label(t: PinType) -> &'static str {
    match t {
        PinType::Input => "Input",
        PinType::Output => "Output",
        PinType::Bidirectional => "Bidirectional",
        PinType::TriState => "Tri-state",
        PinType::Passive => "Passive",
        PinType::PowerInput => "Power input",
        PinType::PowerOutput => "Power output",
        PinType::OpenCollector => "Open collector",
        PinType::OpenEmitter => "Open emitter",
        PinType::Unspecified => "Unspecified",
        PinType::NoConnect => "No connect",
    }
}

/// All pin types, in the order the editor lists them.
pub const PIN_TYPES: [PinType; 11] = [
    PinType::Input,
    PinType::Output,
    PinType::Bidirectional,
    PinType::TriState,
    PinType::Passive,
    PinType::PowerInput,
    PinType::PowerOutput,
    PinType::OpenCollector,
    PinType::OpenEmitter,
    PinType::NoConnect,
    PinType::Unspecified,
];

pub fn parse_direction(s: &str) -> Option<Direction> {
    Some(match s {
        "right" => Direction::Right,
        "up" => Direction::Up,
        "left" => Direction::Left,
        "down" => Direction::Down,
        _ => return None,
    })
}

pub fn direction_id(d: Direction) -> &'static str {
    match d {
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Left => "left",
        Direction::Down => "down",
    }
}

fn parse_u8(value: &str, what: &str) -> Result<u8, SymbolError> {
    value
        .trim()
        .parse()
        .map_err(|_| SymbolError::Invalid(format!("{what} must be a whole number")))
}

fn parse_coord(value: &str, what: &str) -> Result<Coord, SymbolError> {
    value
        .trim()
        .parse()
        .map_err(|_| SymbolError::Invalid(format!("{what} must be a whole number of nanometres")))
}

/// Sets one field of a pin. Keys: `number`, `name`, `type`, `orientation`, `length`, `unit`,
/// `representation`, `x`, `y` (the last four in nm for `length`/`x`/`y`).
pub fn set_pin_field(
    sym: &mut Symbol,
    index: usize,
    key: &str,
    value: &str,
) -> Result<(), SymbolError> {
    edit(sym, |s| {
        let p = s
            .pins
            .get_mut(index)
            .ok_or(SymbolError::NoSuchItem(index))?;
        match key {
            "number" => p.number = value.trim().to_owned(),
            "name" => p.name = value.to_owned(),
            "type" => {
                p.kind = parse_pin_type(value)
                    .ok_or_else(|| SymbolError::Invalid(format!("unknown pin type “{value}”")))?
            }
            "orientation" => {
                p.orientation = parse_direction(value)
                    .ok_or_else(|| SymbolError::Invalid(format!("unknown direction “{value}”")))?
            }
            "length" => p.length = parse_coord(value, "pin length")?,
            "unit" => p.unit = parse_u8(value, "unit")?,
            "representation" => p.representation = parse_u8(value, "representation")?,
            "x" => p.position.x = parse_coord(value, "x")?,
            "y" => p.position.y = parse_coord(value, "y")?,
            other => return Err(SymbolError::UnknownField(other.to_owned())),
        }
        Ok(())
    })
}

/// Sets one field of a graphic. Keys: `unit`, `representation`, `width`, `filled`, `text`,
/// `height` (text height).
pub fn set_graphic_field(
    sym: &mut Symbol,
    index: usize,
    key: &str,
    value: &str,
) -> Result<(), SymbolError> {
    edit(sym, |s| {
        let g = s
            .graphics
            .get_mut(index)
            .ok_or(SymbolError::NoSuchItem(index))?;
        match key {
            "unit" => g.unit = parse_u8(value, "unit")?,
            "representation" => g.representation = parse_u8(value, "representation")?,
            "width" => {
                let w = parse_coord(value, "line width")?;
                if w < 0 {
                    return Err(SymbolError::Invalid(
                        "line width must not be negative".into(),
                    ));
                }
                match &mut g.graphic {
                    Graphic::Line { width, .. }
                    | Graphic::Rect { width, .. }
                    | Graphic::Circle { width, .. }
                    | Graphic::Arc { width, .. } => *width = w,
                    Graphic::Text { .. } => {
                        return Err(SymbolError::Invalid("text has no line width".into()))
                    }
                }
            }
            "filled" => {
                let v = matches!(value, "true" | "1" | "yes");
                match &mut g.graphic {
                    Graphic::Rect { filled, .. } | Graphic::Circle { filled, .. } => *filled = v,
                    _ => {
                        return Err(SymbolError::Invalid(
                            "only rectangles and circles can be filled".into(),
                        ))
                    }
                }
            }
            "text" => match &mut g.graphic {
                Graphic::Text { text, .. } => {
                    if value.is_empty() {
                        return Err(SymbolError::Invalid("text must not be empty".into()));
                    }
                    *text = value.to_owned()
                }
                _ => return Err(SymbolError::Invalid("only text items have text".into())),
            },
            "height" => match &mut g.graphic {
                Graphic::Text { height, .. } => {
                    let h = parse_coord(value, "text height")?;
                    if h <= 0 {
                        return Err(SymbolError::Invalid("text height must be positive".into()));
                    }
                    *height = h
                }
                _ => return Err(SymbolError::Invalid("only text items have a height".into())),
            },
            other => return Err(SymbolError::UnknownField(other.to_owned())),
        }
        Ok(())
    })
}

/// Sets a symbol-level field. Keys: `name`, `reference_prefix`, `default_value`, `is_power`,
/// `unit_count`, and the metadata fields `description`, `keywords`, `datasheet`,
/// `manufacturer`, `mpn`, `default_footprint`, `footprint_filters` (comma separated), and
/// `property:<key>` (an empty value removes the property).
pub fn set_symbol_field(sym: &mut Symbol, key: &str, value: &str) -> Result<(), SymbolError> {
    edit(sym, |s| {
        match key {
            "name" => s.name = value.trim().to_owned(),
            "reference_prefix" => s.reference_prefix = value.trim().to_owned(),
            "default_value" => s.default_value = value.to_owned(),
            "is_power" => s.is_power = matches!(value, "true" | "1" | "yes"),
            "unit_count" => {
                let n = parse_u8(value, "unit count")?;
                if n == 0 || n > MAX_UNITS {
                    return Err(SymbolError::BadUnitCount);
                }
                let orphaned: BTreeSet<u8> = s
                    .pins
                    .iter()
                    .map(|p| p.unit)
                    .chain(s.graphics.iter().map(|g| g.unit))
                    .filter(|u| *u > n)
                    .collect();
                if !orphaned.is_empty() {
                    return Err(SymbolError::UnitInUse(
                        orphaned
                            .iter()
                            .map(u8::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                s.unit_count = n;
            }
            "description" => s.meta.description = value.to_owned(),
            "keywords" => s.meta.keywords = value.to_owned(),
            "datasheet" => s.meta.datasheet = value.to_owned(),
            "manufacturer" => s.meta.manufacturer = value.to_owned(),
            "mpn" => s.meta.mpn = value.to_owned(),
            "default_footprint" => s.meta.default_footprint = value.trim().to_owned(),
            "footprint_filters" => {
                s.meta.footprint_filters = value
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            other => match other.strip_prefix("property:") {
                Some(k) if !k.trim().is_empty() => {
                    if value.is_empty() {
                        s.meta.properties.remove(k.trim());
                    } else {
                        s.meta
                            .properties
                            .insert(k.trim().to_owned(), value.to_owned());
                    }
                }
                _ => return Err(SymbolError::UnknownField(other.to_owned())),
            },
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------------------------
// Units and representations
// ---------------------------------------------------------------------------------------------

/// Copies the items of `from_repr` (graphics and pins that are not shared) to the alternate
/// representation, so it can be edited as a variation. Existing alternate items are replaced.
/// Returns the number of items created.
pub fn create_alternate_from_normal(sym: &mut Symbol) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        s.pins.retain(|p| p.representation != REPR_ALTERNATE);
        s.graphics.retain(|g| g.representation != REPR_ALTERNATE);
        let mut n = 0;
        let pins: Vec<Pin> = s
            .pins
            .iter()
            .filter(|p| p.representation == REPR_NORMAL)
            .cloned()
            .map(|mut p| {
                p.representation = REPR_ALTERNATE;
                p
            })
            .collect();
        n += pins.len();
        s.pins.extend(pins);
        let gfx: Vec<SymbolGraphic> = s
            .graphics
            .iter()
            .filter(|g| g.representation == REPR_NORMAL)
            .cloned()
            .map(|mut g| {
                g.representation = REPR_ALTERNATE;
                g
            })
            .collect();
        n += gfx.len();
        s.graphics.extend(gfx);
        Ok(n)
    })
}

/// Removes every item that belongs to the alternate representation.
pub fn remove_alternate(sym: &mut Symbol) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        let before = s.pins.len() + s.graphics.len();
        s.pins.retain(|p| p.representation != REPR_ALTERNATE);
        s.graphics.retain(|g| g.representation != REPR_ALTERNATE);
        Ok(before - s.pins.len() - s.graphics.len())
    })
}

/// Copies the items of unit `from` into unit `to`. Copied pins get fresh numbers (the next
/// free integers) so the physical pins stay distinct; edit them afterwards.
pub fn copy_unit(sym: &mut Symbol, from: u8, to: u8) -> Result<usize, SymbolError> {
    edit(sym, |s| {
        if from == 0 || from > s.unit_count {
            return Err(SymbolError::NoSuchUnit {
                unit: from,
                count: s.unit_count,
            });
        }
        if to == 0 || to > s.unit_count {
            return Err(SymbolError::NoSuchUnit {
                unit: to,
                count: s.unit_count,
            });
        }
        if from == to {
            return Err(SymbolError::Invalid(
                "source and target unit are the same".into(),
            ));
        }
        let gfx: Vec<SymbolGraphic> = s
            .graphics
            .iter()
            .filter(|g| g.unit == from)
            .cloned()
            .map(|mut g| {
                g.unit = to;
                g
            })
            .collect();
        let pins: Vec<Pin> = s.pins.iter().filter(|p| p.unit == from).cloned().collect();
        let n = gfx.len() + pins.len();
        s.graphics.extend(gfx);
        for mut p in pins {
            p.unit = to;
            p.number = next_pin_number(s, p.representation);
            s.pins.push(p);
        }
        Ok(n)
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn pin(n: &str, x: i64) -> Pin {
        Pin {
            number: n.into(),
            name: "P".into(),
            kind: PinType::Passive,
            position: Point::new(x, 0),
            orientation: Direction::Right,
            length: 2_540_000,
            unit: 0,
            representation: 0,
        }
    }

    #[test]
    fn duplicate_numbers_are_rejected_only_inside_the_same_scope() {
        let mut s = Symbol::new("Test", "U");
        s.unit_count = 2;
        add_pin(&mut s, pin("1", 0)).unwrap();
        assert_eq!(
            add_pin(&mut s, pin("1", 10)),
            Err(SymbolError::DuplicatePinNumber("1".into()))
        );
        let mut a = pin("2", 0);
        a.unit = 1;
        let mut b = pin("2", 0);
        b.unit = 2;
        add_pin(&mut s, a).unwrap();
        // Same number in another unit would be a second physical pin 2 only if it were shared;
        // different units never overlap, so this is accepted.
        add_pin(&mut s, b).unwrap();
        // A shared (unit 0) pin clashes with a unit-specific one.
        assert!(add_pin(&mut s, pin("2", 5)).is_err());
    }

    #[test]
    fn failed_edit_leaves_the_symbol_unchanged() {
        let mut s = Symbol::new("Test", "U");
        add_pin(&mut s, pin("1", 0)).unwrap();
        let before = s.clone();
        assert!(set_pin_field(&mut s, 0, "number", "").is_err());
        assert!(set_pin_field(&mut s, 0, "type", "bogus").is_err());
        assert!(set_pin_field(&mut s, 9, "name", "x").is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn unit_count_cannot_drop_units_in_use() {
        let mut s = Symbol::new("Test", "U");
        s.unit_count = 3;
        let mut p = pin("1", 0);
        p.unit = 3;
        add_pin(&mut s, p).unwrap();
        assert!(matches!(
            set_symbol_field(&mut s, "unit_count", "2"),
            Err(SymbolError::UnitInUse(_))
        ));
        set_symbol_field(&mut s, "unit_count", "4").unwrap();
        assert_eq!(s.unit_count, 4);
    }

    #[test]
    fn move_rotate_mirror_are_exact_and_reversible() {
        let mut s = Symbol::new("Test", "U");
        add_pin(&mut s, pin("1", 2_540_000)).unwrap();
        add_graphic(
            &mut s,
            Graphic::Line {
                segment: Segment::new(Point::new(0, 0), Point::new(2_540_000, 1_270_000)),
                width: 150_000,
            }
            .into(),
        )
        .unwrap();
        let items = [Item::Pin(0), Item::Graphic(0)];
        let original = s.clone();
        for _ in 0..4 {
            rotate_items(&mut s, &items, Point::ORIGIN).unwrap();
        }
        assert_eq!(s, original, "four quarter turns are the identity");
        mirror_items(&mut s, &items, Point::ORIGIN, true).unwrap();
        assert_eq!(s.pins[0].orientation, Direction::Left);
        assert_eq!(s.pins[0].position, Point::new(-2_540_000, 0));
        mirror_items(&mut s, &items, Point::ORIGIN, true).unwrap();
        assert_eq!(s, original);
        move_items(&mut s, &items, Vector::new(1_270_000, -1_270_000)).unwrap();
        move_items(&mut s, &items, Vector::new(-1_270_000, 1_270_000)).unwrap();
        assert_eq!(s, original);
    }

    #[test]
    fn delete_renumbers_and_validates_indices() {
        let mut s = Symbol::new("Test", "U");
        for i in 0..4 {
            add_pin(&mut s, pin(&(i + 1).to_string(), i * 1_270_000)).unwrap();
        }
        assert_eq!(delete_items(&mut s, &[Item::Pin(1), Item::Pin(3)]), Ok(2));
        let numbers: Vec<_> = s.pins.iter().map(|p| p.number.as_str()).collect();
        assert_eq!(numbers, ["1", "3"]);
        assert!(delete_items(&mut s, &[Item::Pin(5)]).is_err());
        assert_eq!(s.pins.len(), 2);
    }

    #[test]
    fn alternate_representation_copies_and_removes() {
        let mut s = Symbol::new("Test", "U");
        let mut p = pin("1", 0);
        p.representation = REPR_NORMAL;
        add_pin(&mut s, p).unwrap();
        assert!(!s.has_alternate());
        assert_eq!(create_alternate_from_normal(&mut s), Ok(1));
        assert!(s.has_alternate());
        assert_eq!(s.pins_for(1, REPR_NORMAL).count(), 1);
        assert_eq!(s.pins_for(1, REPR_ALTERNATE).count(), 1);
        assert_eq!(remove_alternate(&mut s), Ok(1));
        assert!(!s.has_alternate());
    }

    #[test]
    fn copy_unit_gives_new_pin_numbers() {
        let mut s = Symbol::new("Gate", "U");
        s.unit_count = 2;
        let mut p = pin("1", 0);
        p.unit = 1;
        add_pin(&mut s, p).unwrap();
        assert_eq!(copy_unit(&mut s, 1, 2), Ok(1));
        assert_eq!(s.pins.len(), 2);
        assert_eq!(s.pins[1].unit, 2);
        assert_ne!(s.pins[0].number, s.pins[1].number);
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("LM358").is_ok());
        assert_eq!(validate_name("  "), Err(SymbolError::EmptyName));
        assert!(validate_name("a/b").is_err());
    }
}
