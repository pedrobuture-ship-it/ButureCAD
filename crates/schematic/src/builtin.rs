//! Bootstrap symbol library ("Buture:"), embedded in code.
//!
//! **Temporary by design:** this exists so the schematic editor can place real symbols before the
//! library system lands (roadmap stage 31). It is small on purpose; do not grow it (the power flag
//! was added because ERC needs one, see `docs/erc.md`). Geometry
//! follows a 1.27 mm (50 mil) grid with 2.54 mm (100 mil) pin pitch, in symbol-local coordinates.

use crate::model::*;
use crate::schematic::Schematic;
use buturecad_geometry::{Arc, Point, Rectangle, Segment};

const MM: f64 = 1_000_000.0;
const LINE: i64 = 150_000;

fn u(v: f64) -> i64 {
    (v * MM).round() as i64
}
fn p(x: f64, y: f64) -> Point {
    Point::new(u(x), u(y))
}
fn line(a: (f64, f64), b: (f64, f64)) -> Graphic {
    Graphic::Line {
        segment: Segment::new(p(a.0, a.1), p(b.0, b.1)),
        width: LINE,
    }
}
fn rect(a: (f64, f64), b: (f64, f64)) -> Graphic {
    match Rectangle::new(p(a.0, a.1), p(b.0, b.1)) {
        Ok(rect) => Graphic::Rect {
            rect,
            filled: false,
            width: LINE,
        },
        // Constants above are non-degenerate; keep an inert graphic rather than panic.
        Err(_) => line(a, b),
    }
}
fn pin(
    number: &str,
    name: &str,
    kind: PinType,
    at: (f64, f64),
    orientation: Direction,
    length: f64,
) -> Pin {
    Pin {
        number: number.into(),
        name: name.into(),
        kind,
        position: p(at.0, at.1),
        orientation,
        length: u(length),
        unit: 0,
        representation: 0,
    }
}
fn text(at: (f64, f64), s: &str) -> Graphic {
    Graphic::Text {
        origin: p(at.0, at.1),
        height: u(1.27),
        text: s.into(),
    }
}

fn passive_two_pin(
    name: &str,
    prefix: &str,
    value: &str,
    pin_len: f64,
    graphics: Vec<Graphic>,
) -> Symbol {
    Symbol {
        name: format!("Buture:{name}"),
        reference_prefix: prefix.into(),
        default_value: value.into(),
        is_power: false,
        pins: vec![
            pin(
                "1",
                "~",
                PinType::Passive,
                (0.0, 3.81),
                Direction::Down,
                pin_len,
            ),
            pin(
                "2",
                "~",
                PinType::Passive,
                (0.0, -3.81),
                Direction::Up,
                pin_len,
            ),
        ],
        graphics: graphics.into_iter().map(Into::into).collect(),
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

fn resistor() -> Symbol {
    passive_two_pin(
        "R",
        "R",
        "10k",
        1.27,
        vec![rect((-1.016, -2.54), (1.016, 2.54))],
    )
}

fn capacitor() -> Symbol {
    passive_two_pin(
        "C",
        "C",
        "100n",
        3.175,
        vec![
            line((-1.905, 0.635), (1.905, 0.635)),
            line((-1.905, -0.635), (1.905, -0.635)),
        ],
    )
}

fn inductor() -> Symbol {
    let mut g = Vec::new();
    for c in [1.905, 0.635, -0.635, -1.905] {
        // Bumps on the +x side, drawn top to bottom (clockwise).
        if let Ok(arc) = Arc::new(p(0.0, c), p(0.0, c + 0.635), p(0.0, c - 0.635), false) {
            g.push(Graphic::Arc { arc, width: LINE });
        }
    }
    passive_two_pin("L", "L", "10u", 1.27, g)
}

fn diode_graphics() -> Vec<Graphic> {
    vec![
        line((1.27, 1.27), (1.27, -1.27)),
        line((1.27, 1.27), (-1.27, 0.0)),
        line((1.27, -1.27), (-1.27, 0.0)),
        line((-1.27, 1.27), (-1.27, -1.27)),
    ]
}

fn diode_pins() -> Vec<Pin> {
    vec![
        pin(
            "1",
            "K",
            PinType::Passive,
            (-3.81, 0.0),
            Direction::Right,
            2.54,
        ),
        pin(
            "2",
            "A",
            PinType::Passive,
            (3.81, 0.0),
            Direction::Left,
            2.54,
        ),
    ]
}

fn diode() -> Symbol {
    Symbol {
        name: "Buture:D".into(),
        reference_prefix: "D".into(),
        default_value: "1N4148".into(),
        is_power: false,
        pins: diode_pins(),
        graphics: diode_graphics().into_iter().map(Into::into).collect(),
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

fn led() -> Symbol {
    let mut g = diode_graphics();
    g.push(line((-0.254, 1.778), (1.016, 3.048)));
    g.push(line((0.762, 1.778), (2.032, 3.048)));
    Symbol {
        name: "Buture:LED".into(),
        reference_prefix: "D".into(),
        default_value: "LED".into(),
        is_power: false,
        pins: diode_pins(),
        graphics: g.into_iter().map(Into::into).collect(),
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

fn connector(n: u32) -> Symbol {
    let top = (n as f64 - 1.0) * 1.27;
    let mut pins = Vec::new();
    for i in 0..n {
        let y = top - i as f64 * 2.54;
        pins.push(pin(
            &(i + 1).to_string(),
            &format!("Pin_{}", i + 1),
            PinType::Passive,
            (-5.08, y),
            Direction::Right,
            2.54,
        ));
    }
    Symbol {
        name: format!("Buture:Conn_01x{n:02}"),
        reference_prefix: "J".into(),
        default_value: format!("Conn_01x{n:02}"),
        is_power: false,
        pins,
        graphics: vec![rect((-2.54, -top - 1.27), (0.0, top + 1.27)).into()],
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

fn ground() -> Symbol {
    Symbol {
        name: "Buture:GND".into(),
        reference_prefix: String::new(),
        default_value: "GND".into(),
        is_power: true,
        pins: vec![pin(
            "1",
            "GND",
            PinType::PowerInput,
            (0.0, 0.0),
            Direction::Down,
            0.0,
        )],
        graphics: vec![
            line((0.0, 0.0), (0.0, -1.27)),
            line((-1.27, -1.27), (1.27, -1.27)),
            line((-0.762, -1.778), (0.762, -1.778)),
            line((-0.254, -2.286), (0.254, -2.286)),
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

fn supply(name: &str, net: &str) -> Symbol {
    Symbol {
        name: format!("Buture:{name}"),
        reference_prefix: String::new(),
        default_value: net.into(),
        is_power: true,
        pins: vec![pin(
            "1",
            net,
            PinType::PowerInput,
            (0.0, 0.0),
            Direction::Up,
            0.0,
        )],
        graphics: vec![
            line((0.0, 0.0), (0.0, 1.27)),
            line((-0.762, 1.27), (0.762, 1.27)),
            text((-1.2, 2.0), net),
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
        unit_count: 1,
        meta: SymbolMeta::default(),
    }
}

/// A power flag: tells ERC that the net it touches is fed from outside the schematic (a
/// connector, a battery, a bench supply). Its pin is a power *output*.
fn power_flag() -> Symbol {
    Symbol {
        name: "Buture:PWR_FLAG".into(),
        reference_prefix: "#FLG".into(),
        default_value: "PWR_FLAG".into(),
        is_power: false,
        pins: vec![pin(
            "1",
            "pwr",
            PinType::PowerOutput,
            (0.0, 0.0),
            Direction::Up,
            0.0,
        )],
        graphics: vec![
            line((0.0, 0.0), (0.0, 1.27)),
            line((0.0, 1.27), (1.27, 2.54)),
            line((1.27, 2.54), (0.0, 3.81)),
            line((0.0, 3.81), (-1.27, 2.54)),
            line((-1.27, 2.54), (0.0, 1.27)),
            text((1.9, 2.0), "PWR_FLAG"),
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
        unit_count: 1,
        meta: SymbolMeta {
            description: "Marks a net as supplied from outside the schematic (satisfies the ERC \"power input not driven\" check)".into(),
            ..SymbolMeta::default()
        },
    }
}

/// All built-in symbols.
pub fn symbols() -> Vec<Symbol> {
    let mut v = vec![
        resistor(),
        capacitor(),
        inductor(),
        diode(),
        led(),
        connector(2),
        connector(4),
        ground(),
    ];
    v.push(supply("+3V3", "+3V3"));
    v.push(supply("+5V", "+5V"));
    v.push(supply("VCC", "VCC"));
    v.push(power_flag());
    v
}

/// Copies every built-in symbol into the schematic's symbol table (project files are
/// self-contained: they embed the symbols they use).
pub fn install(schematic: &mut Schematic) {
    for s in symbols() {
        schematic.add_symbol(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn symbols_are_well_formed() {
        for s in symbols() {
            assert!(!s.pins.is_empty(), "{}", s.name);
            let numbers: BTreeSet<_> = s.pins.iter().map(|p| &p.number).collect();
            assert_eq!(
                numbers.len(),
                s.pins.len(),
                "{} has duplicate pin numbers",
                s.name
            );
            assert_eq!(s.is_power, s.reference_prefix.is_empty(), "{}", s.name);
            for pin in &s.pins {
                // Pin connection points sit on the 1.27 mm grid so wires can reach them.
                assert_eq!(
                    pin.position.x % 1_270_000,
                    0,
                    "{} pin {}",
                    s.name,
                    pin.number
                );
                assert_eq!(
                    pin.position.y % 1_270_000,
                    0,
                    "{} pin {}",
                    s.name,
                    pin.number
                );
            }
        }
    }

    #[test]
    fn two_pin_passives_pins_are_on_the_body_axis() {
        let r = resistor();
        assert_eq!(
            (r.pins[0].position, r.pins[1].position),
            (p(0.0, 3.81), p(0.0, -3.81))
        );
    }
}
