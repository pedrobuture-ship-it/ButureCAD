//! The **`Std` standard library**: read-only, built into the program, deterministic.
//!
//! * symbols: generic parts with the pinouts of common packages (diodes, transistors, op-amps,
//!   regulators, crystals, switches, headers, a 555, USB Micro-B…);
//! * footprints: chip sizes, SOIC, TSSOP, MSOP, LQFP, SOT-23, SOT-223, SOD-123, SMA, DIP, headers,
//!   mounting holes and test points;
//! * components: parts with a symbol, a footprint and a manufacturer part number.
//!
//! **Provenance and limits (read `docs/standard-library.md`).** Nothing here is copied from another
//! library. Package outlines (pitch, body, lead span, pin counts) are the published nominal
//! JEDEC / EIA values and are checked by tests (`crates/pcb/tests/standard_outlines.rs`). Land
//! patterns follow a simplified IPC-7351B nominal rule and are **not verified against vendor
//! datasheets or a fabricator**. Symbol pin numbers follow the common pinout of the named package;
//! manufacturer pinouts can differ: check the datasheet of the part you order.

use crate::component_lib::{Component, ComponentLibrary};
use crate::error::Result;
use crate::footprint_lib::FootprintLibrary;
use crate::symbol_lib::SymbolLibrary;
use buturecad_geometry::{Circle, Point, Rectangle, Segment};
use buturecad_pcb::generators::{self, Dip, Header, Soic};
use buturecad_pcb::Footprint;
use buturecad_schematic::{Direction, Graphic, Pin, PinType, Symbol, SymbolMeta};

/// Name of the library.
pub const NAME: &str = "Std";

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
fn rect(a: (f64, f64), b: (f64, f64)) -> Vec<Graphic> {
    Rectangle::new(p(a.0, a.1), p(b.0, b.1))
        .map(|rect| {
            vec![Graphic::Rect {
                rect,
                filled: false,
                width: LINE,
            }]
        })
        .unwrap_or_default()
}
fn circle(c: (f64, f64), r: f64) -> Vec<Graphic> {
    Circle::new(p(c.0, c.1), u(r))
        .map(|circle| {
            vec![Graphic::Circle {
                circle,
                filled: false,
                width: LINE,
            }]
        })
        .unwrap_or_default()
}
fn text(at: (f64, f64), s: &str) -> Graphic {
    Graphic::Text {
        origin: p(at.0, at.1),
        height: u(1.27),
        text: s.into(),
    }
}
fn pin(num: &str, name: &str, kind: PinType, at: (f64, f64), dir: Direction) -> Pin {
    Pin {
        number: num.into(),
        name: name.into(),
        kind,
        position: p(at.0, at.1),
        orientation: dir,
        length: u(2.54),
        unit: 0,
        representation: 0,
    }
}

fn symbol(
    name: &str,
    prefix: &str,
    value: &str,
    pins: Vec<Pin>,
    g: Vec<Graphic>,
    desc: &str,
    keywords: &str,
) -> Symbol {
    Symbol {
        name: name.into(),
        reference_prefix: prefix.into(),
        default_value: value.into(),
        is_power: false,
        pins,
        graphics: g.into_iter().map(Into::into).collect(),
        unit_count: 1,
        meta: SymbolMeta {
            description: desc.into(),
            keywords: keywords.into(),
            ..SymbolMeta::default()
        },
    }
}

/// Two pins on the vertical axis (1 on top), 3.81 mm from the centre.
fn vertical_two_pin(
    name: &str,
    prefix: &str,
    value: &str,
    names: (&str, &str),
    g: Vec<Graphic>,
    desc: &str,
    kw: &str,
) -> Symbol {
    let pins = vec![
        pin("1", names.0, PinType::Passive, (0.0, 3.81), Direction::Down),
        pin("2", names.1, PinType::Passive, (0.0, -3.81), Direction::Up),
    ];
    symbol(name, prefix, value, pins, g, desc, kw)
}

fn diode_body(extra: Vec<Graphic>) -> Vec<Graphic> {
    let mut g = vec![
        line((1.27, 1.27), (1.27, -1.27)),
        line((1.27, 1.27), (-1.27, 0.0)),
        line((1.27, -1.27), (-1.27, 0.0)),
        line((-1.27, 1.27), (-1.27, -1.27)),
    ];
    g.extend(extra);
    g
}

fn diode_pins() -> Vec<Pin> {
    vec![
        pin("1", "K", PinType::Passive, (-3.81, 0.0), Direction::Right),
        pin("2", "A", PinType::Passive, (3.81, 0.0), Direction::Left),
    ]
}

fn transistor(
    name: &str,
    value: &str,
    pins: [(&str, &str, PinType); 3],
    mos: bool,
    p_type: bool,
    desc: &str,
) -> Symbol {
    // Left pin = base/gate; top = collector/drain; bottom = emitter/source. Pin numbers are given
    // in the order (base/gate, collector/drain, emitter/source).
    let mut g = circle((1.27, 0.0), 3.81);
    g.push(line((0.0, 1.905), (0.0, -1.905)));
    g.push(line((-2.54, 0.0), (0.0, 0.0)));
    g.push(line((0.0, 0.635), (2.54, 2.54)));
    g.push(line((0.0, -0.635), (2.54, -2.54)));
    if mos {
        g.push(line((-0.635, 1.27), (-0.635, -1.27)));
    }
    if p_type {
        g.push(text((1.0, -0.3), "P"));
    }
    let pins = vec![
        pin(
            pins[0].0,
            pins[0].1,
            pins[0].2,
            (-5.08, 0.0),
            Direction::Right,
        ),
        pin(
            pins[1].0,
            pins[1].1,
            pins[1].2,
            (2.54, 5.08),
            Direction::Down,
        ),
        pin(
            pins[2].0,
            pins[2].1,
            pins[2].2,
            (2.54, -5.08),
            Direction::Up,
        ),
    ];
    symbol(name, "Q", value, pins, g, desc, "transistor")
}

fn connector(rows: usize, n: usize) -> Symbol {
    let per = n as f64;
    let top = (per - 1.0) / 2.0 * 2.54;
    let mut pins = Vec::new();
    if rows == 1 {
        for i in 0..n {
            let y = top - i as f64 * 2.54;
            pins.push(pin(
                &(i + 1).to_string(),
                &format!("Pin_{}", i + 1),
                PinType::Passive,
                (-5.08, y),
                Direction::Right,
            ));
        }
    } else {
        for i in 0..n {
            let y = top - i as f64 * 2.54;
            pins.push(pin(
                &(2 * i + 1).to_string(),
                &format!("Pin_{}", 2 * i + 1),
                PinType::Passive,
                (-7.62, y),
                Direction::Right,
            ));
            pins.push(pin(
                &(2 * i + 2).to_string(),
                &format!("Pin_{}", 2 * i + 2),
                PinType::Passive,
                (7.62, y),
                Direction::Left,
            ));
        }
    }
    let half = if rows == 1 { 2.54 } else { 5.08 };
    let g = rect(
        (-half, -top - 1.27),
        (if rows == 1 { 0.0 } else { half }, top + 1.27),
    );
    let name = format!("Conn_{rows:02}x{n:02}");
    symbol(
        &name,
        "J",
        &name,
        pins,
        g,
        &format!("Generic connector, {rows} row(s) of {n} pins"),
        "connector header",
    )
}

fn symbols_list() -> Vec<Symbol> {
    let mut v = Vec::new();
    // -- passives ------------------------------------------------------------------------------
    let mut cp = vec![line((-1.905, 0.635), (1.905, 0.635)), text((1.4, 1.9), "+")];
    cp.extend(rect((-1.905, -0.254), (1.905, -0.889)));
    v.push(vertical_two_pin(
        "C_Polarized",
        "C",
        "10u",
        ("+", "-"),
        cp,
        "Polarized capacitor (pin 1 = +)",
        "capacitor electrolytic tantalum",
    ));
    let mut fb = rect((-1.016, -1.905), (1.016, 1.905));
    fb.push(line((-1.016, -1.905), (1.016, 1.905)));
    v.push(vertical_two_pin(
        "Ferrite_Bead",
        "FB",
        "600R@100MHz",
        ("~", "~"),
        fb,
        "Ferrite bead",
        "ferrite bead filter",
    ));
    let mut fuse = rect((-0.508, -2.54), (0.508, 2.54));
    fuse.push(line((0.0, 2.54), (0.0, -2.54)));
    v.push(vertical_two_pin(
        "Fuse",
        "F",
        "500mA",
        ("~", "~"),
        fuse,
        "Fuse",
        "fuse protection",
    ));
    let mut xt = rect((-0.635, -1.524), (0.635, 1.524));
    xt.push(line((-1.905, 1.27), (-1.905, -1.27)));
    xt.push(line((1.905, 1.27), (1.905, -1.27)));
    let xt_pins = vec![
        pin("1", "1", PinType::Passive, (-5.08, 0.0), Direction::Right),
        pin("2", "2", PinType::Passive, (5.08, 0.0), Direction::Left),
    ];
    v.push(symbol(
        "Crystal_2Pin",
        "Y",
        "8MHz",
        xt_pins,
        xt,
        "Two-terminal crystal",
        "crystal oscillator",
    ));
    let mut xt4 = rect((-5.08, -5.08), (5.08, 5.08));
    xt4.push(text((-1.5, -0.4), "XTAL"));
    let xt4_pins = vec![
        pin("1", "XIN", PinType::Passive, (-7.62, 0.0), Direction::Right),
        pin("3", "XOUT", PinType::Passive, (7.62, 0.0), Direction::Left),
        pin(
            "2",
            "GND_1",
            PinType::Passive,
            (-2.54, -7.62),
            Direction::Up,
        ),
        pin("4", "GND_2", PinType::Passive, (2.54, -7.62), Direction::Up),
    ];
    v.push(symbol(
        "Crystal_4Pad",
        "Y",
        "8MHz",
        xt4_pins,
        xt4,
        "Crystal with four pads (2 and 4 are ground), pads 1-2-3-4 counter-clockwise",
        "crystal oscillator",
    ));
    let mut sw = vec![
        line((-2.54, 0.0), (-2.54, 1.27)),
        line((2.54, 0.0), (2.54, 1.27)),
        line((-2.54, 1.27), (2.54, 1.27)),
    ];
    sw.push(line((0.0, 1.27), (0.0, 2.54)));
    let sw_pins = vec![
        pin("1", "A", PinType::Passive, (-5.08, 0.0), Direction::Right),
        pin("2", "B", PinType::Passive, (5.08, 0.0), Direction::Left),
    ];
    v.push(symbol(
        "SW_Push",
        "SW",
        "SW_Push",
        sw_pins,
        sw,
        "Tactile push button (pads 1 and 2 on the footprint may be doubled)",
        "switch button tactile",
    ));
    // -- diodes ----------------------------------------------------------------------------------
    let mut schottky = diode_body(vec![]);
    schottky.push(line((-1.27, 1.27), (-1.905, 1.27)));
    schottky.push(line((-1.905, 1.27), (-1.905, 0.635)));
    schottky.push(line((-1.27, -1.27), (-0.635, -1.27)));
    schottky.push(line((-0.635, -1.27), (-0.635, -0.635)));
    v.push(symbol(
        "D_Schottky",
        "D",
        "SS14",
        diode_pins(),
        schottky,
        "Schottky diode (pin 1 = cathode)",
        "diode schottky",
    ));
    let mut zener = diode_body(vec![]);
    zener.push(line((-1.27, 1.27), (-1.905, 1.905)));
    zener.push(line((-1.27, -1.27), (-0.635, -1.905)));
    v.push(symbol(
        "D_Zener",
        "D",
        "5V1",
        diode_pins(),
        zener,
        "Zener diode (pin 1 = cathode)",
        "diode zener",
    ));
    // -- transistors (SOT-23 numbering: BJT 1=B 2=E 3=C, MOSFET 1=G 2=S 3=D) ------------------------
    v.push(transistor(
        "Q_NPN_BEC",
        "BC847B",
        [
            ("1", "B", PinType::Input),
            ("3", "C", PinType::Passive),
            ("2", "E", PinType::Passive),
        ],
        false,
        false,
        "NPN transistor, SOT-23 pinout 1=B 2=E 3=C",
    ));
    v.push(transistor(
        "Q_PNP_BEC",
        "BC857B",
        [
            ("1", "B", PinType::Input),
            ("3", "C", PinType::Passive),
            ("2", "E", PinType::Passive),
        ],
        false,
        true,
        "PNP transistor, SOT-23 pinout 1=B 2=E 3=C",
    ));
    v.push(transistor(
        "Q_NMOS_GSD",
        "2N7002",
        [
            ("1", "G", PinType::Input),
            ("3", "D", PinType::Passive),
            ("2", "S", PinType::Passive),
        ],
        true,
        false,
        "N-channel MOSFET, SOT-23 pinout 1=G 2=S 3=D",
    ));
    v.push(transistor(
        "Q_PMOS_GSD",
        "SI2301",
        [
            ("1", "G", PinType::Input),
            ("3", "D", PinType::Passive),
            ("2", "S", PinType::Passive),
        ],
        true,
        true,
        "P-channel MOSFET, SOT-23 pinout 1=G 2=S 3=D",
    ));
    // -- op-amps, regulators, 555 ------------------------------------------------------------------
    let oa5 = vec![
        pin("3", "+", PinType::Input, (-7.62, 2.54), Direction::Right),
        pin("4", "-", PinType::Input, (-7.62, -2.54), Direction::Right),
        pin("1", "OUT", PinType::Output, (7.62, 0.0), Direction::Left),
        pin(
            "5",
            "V+",
            PinType::PowerInput,
            (-2.54, 6.35),
            Direction::Down,
        ),
        pin(
            "2",
            "V-",
            PinType::PowerInput,
            (-2.54, -6.35),
            Direction::Up,
        ),
    ];
    let tri = |dy: f64| {
        vec![
            line((-5.08, dy + 5.08), (-5.08, dy - 5.08)),
            line((-5.08, dy + 5.08), (5.08, dy)),
            line((-5.08, dy - 5.08), (5.08, dy)),
        ]
    };
    v.push(symbol(
        "OpAmp_5pin",
        "U",
        "OPA",
        oa5,
        tri(0.0),
        "Single op-amp, SOT-23-5 pinout (1 OUT, 2 V-, 3 +IN, 4 -IN, 5 V+)",
        "opamp amplifier",
    ));
    let oa8 = vec![
        pin("1", "OUT1", PinType::Output, (7.62, 7.62), Direction::Left),
        pin("2", "-IN1", PinType::Input, (-7.62, 5.08), Direction::Right),
        pin(
            "3",
            "+IN1",
            PinType::Input,
            (-7.62, 10.16),
            Direction::Right,
        ),
        pin("7", "OUT2", PinType::Output, (7.62, -7.62), Direction::Left),
        pin(
            "6",
            "-IN2",
            PinType::Input,
            (-7.62, -10.16),
            Direction::Right,
        ),
        pin(
            "5",
            "+IN2",
            PinType::Input,
            (-7.62, -5.08),
            Direction::Right,
        ),
        pin(
            "8",
            "V+",
            PinType::PowerInput,
            (0.0, 15.24),
            Direction::Down,
        ),
        pin("4", "V-", PinType::PowerInput, (0.0, -15.24), Direction::Up),
    ];
    let mut oa8g = tri(7.62);
    oa8g.extend(tri(-7.62));
    v.push(symbol(
        "OpAmp_Dual_8",
        "U",
        "LM358",
        oa8,
        oa8g,
        "Dual op-amp, 8-pin pinout (1 OUT1, 2 -IN1, 3 +IN1, 4 V-, 5 +IN2, 6 -IN2, 7 OUT2, 8 V+)",
        "opamp amplifier dual",
    ));
    let ldo5 = vec![
        pin(
            "1",
            "VIN",
            PinType::PowerInput,
            (-7.62, 2.54),
            Direction::Right,
        ),
        pin("3", "EN", PinType::Input, (-7.62, -2.54), Direction::Right),
        pin(
            "5",
            "VOUT",
            PinType::PowerOutput,
            (7.62, 2.54),
            Direction::Left,
        ),
        pin(
            "4",
            "NC",
            PinType::NoConnect,
            (7.62, -2.54),
            Direction::Left,
        ),
        pin("2", "GND", PinType::PowerInput, (0.0, -7.62), Direction::Up),
    ];
    v.push(symbol(
        "Regulator_LDO_5",
        "U",
        "LDO",
        ldo5,
        rect((-5.08, -5.08), (5.08, 5.08)),
        "Linear regulator, SOT-23-5 pinout (1 VIN, 2 GND, 3 EN, 4 NC, 5 VOUT)",
        "regulator ldo",
    ));
    let lin3 = vec![
        pin(
            "3",
            "VIN",
            PinType::PowerInput,
            (-7.62, 2.54),
            Direction::Right,
        ),
        pin(
            "2",
            "VOUT",
            PinType::PowerOutput,
            (7.62, 2.54),
            Direction::Left,
        ),
        pin(
            "4",
            "VOUT_TAB",
            PinType::PowerOutput,
            (7.62, -2.54),
            Direction::Left,
        ),
        pin("1", "GND", PinType::PowerInput, (0.0, -7.62), Direction::Up),
    ];
    v.push(symbol(
        "Regulator_Linear_3",
        "U",
        "AMS1117",
        lin3,
        rect((-5.08, -5.08), (5.08, 5.08)),
        "Linear regulator, SOT-223 pinout (1 GND, 2 VOUT, 3 VIN, tab = VOUT)",
        "regulator linear ldo",
    ));
    let t555 = vec![
        pin(
            "8",
            "VCC",
            PinType::PowerInput,
            (0.0, 10.16),
            Direction::Down,
        ),
        pin(
            "1",
            "GND",
            PinType::PowerInput,
            (0.0, -10.16),
            Direction::Up,
        ),
        pin(
            "2",
            "TRIG",
            PinType::Input,
            (-10.16, 2.54),
            Direction::Right,
        ),
        pin("6", "THR", PinType::Input, (-10.16, 0.0), Direction::Right),
        pin(
            "5",
            "CTRL",
            PinType::Input,
            (-10.16, -2.54),
            Direction::Right,
        ),
        pin(
            "4",
            "RESET",
            PinType::Input,
            (-10.16, 5.08),
            Direction::Right,
        ),
        pin("3", "OUT", PinType::Output, (10.16, 2.54), Direction::Left),
        pin(
            "7",
            "DISCH",
            PinType::OpenCollector,
            (10.16, -2.54),
            Direction::Left,
        ),
    ];
    v.push(symbol(
        "NE555",
        "U",
        "NE555",
        t555,
        rect((-7.62, -7.62), (7.62, 7.62)),
        "555 timer, 8-pin pinout",
        "timer 555",
    ));
    // -- connectors ------------------------------------------------------------------------------
    for n in 1..=20 {
        v.push(connector(1, n));
    }
    for n in 2..=20 {
        v.push(connector(2, n));
    }
    let usb = vec![
        pin(
            "1",
            "VBUS",
            PinType::PowerOutput,
            (-7.62, 5.08),
            Direction::Right,
        ),
        pin(
            "2",
            "D-",
            PinType::Bidirectional,
            (-7.62, 2.54),
            Direction::Right,
        ),
        pin(
            "3",
            "D+",
            PinType::Bidirectional,
            (-7.62, 0.0),
            Direction::Right,
        ),
        pin(
            "4",
            "ID",
            PinType::Passive,
            (-7.62, -2.54),
            Direction::Right,
        ),
        pin(
            "5",
            "GND",
            PinType::Passive,
            (-7.62, -5.08),
            Direction::Right,
        ),
        pin(
            "6",
            "SHIELD",
            PinType::Passive,
            (0.0, -10.16),
            Direction::Up,
        ),
    ];
    v.push(symbol(
        "USB_Micro_B",
        "J",
        "USB_Micro_B",
        usb,
        rect((-5.08, -7.62), (2.54, 7.62)),
        "USB 2.0 Micro-B receptacle (1 VBUS, 2 D-, 3 D+, 4 ID, 5 GND, 6 shield)",
        "usb connector",
    ));
    v
}

// -- footprints ------------------------------------------------------------------------------------

fn gen<T>(r: std::result::Result<T, generators::GeneratorError>) -> Option<T> {
    r.ok()
}

fn footprints_list() -> Vec<Footprint> {
    let mut v: Vec<Footprint> = Vec::new();
    for code in generators::CHIP_CODES {
        v.extend(gen(generators::chip(code)));
    }
    for n in [8, 14, 16] {
        v.extend(gen(Soic::narrow(n).generate()));
    }
    for n in [8, 14, 16, 20, 24, 28] {
        v.extend(gen(generators::tssop(n)));
    }
    v.extend(gen(generators::msop8()));
    for o in generators::LQFP_OUTLINES {
        v.extend(gen(generators::lqfp(o.0)));
    }
    for n in [3, 5, 6] {
        v.extend(gen(generators::sot23(n)));
    }
    v.extend(gen(generators::sot223()));
    v.extend(gen(generators::sod123()));
    v.extend(gen(generators::sma()));
    for n in [8, 14, 16, 28] {
        v.extend(gen(Dip::standard(n).generate()));
    }
    for n in 1..=20 {
        v.extend(gen(Header::standard(n, 1).generate()));
    }
    for n in 2..=20 {
        v.extend(gen(Header::standard(n, 2).generate()));
    }
    for d in [2.2, 3.2] {
        v.extend(gen(generators::mounting_hole(d)));
    }
    v.extend(gen(generators::test_point(1.5)));
    v
}

// -- components --------------------------------------------------------------------------------------

fn part(
    name: &str,
    symbol: &str,
    footprint: &str,
    manufacturer: &str,
    mpn: &str,
    description: &str,
    tags: &[&str],
) -> Component {
    Component {
        name: name.into(),
        description: description.into(),
        symbol: format!("{NAME}:{symbol}"),
        footprint: format!("{NAME}:{footprint}"),
        model3d: String::new(),
        manufacturer: manufacturer.into(),
        mpn: mpn.into(),
        datasheet: String::new(),
        parameters: Default::default(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
    }
}

fn components_list() -> Vec<Component> {
    let soic8 = Soic::narrow(8)
        .generate()
        .map(|f| f.name)
        .unwrap_or_default();
    let mut v = vec![
        part(
            "LM358DR",
            "OpAmp_Dual_8",
            &soic8,
            "Texas Instruments",
            "LM358DR",
            "Dual op-amp, SOIC-8",
            &["opamp", "dual"],
        ),
        part(
            "NE555DR",
            "NE555",
            &soic8,
            "Texas Instruments",
            "NE555DR",
            "555 timer, SOIC-8",
            &["timer", "555"],
        ),
        part(
            "AMS1117-3.3",
            "Regulator_Linear_3",
            "SOT-223-3",
            "Advanced Monolithic Systems",
            "AMS1117-3.3",
            "3.3 V 1 A linear regulator, SOT-223",
            &["regulator", "3v3"],
        ),
        part(
            "AP2112K-3.3",
            "Regulator_LDO_5",
            "SOT-23-5",
            "Diodes Incorporated",
            "AP2112K-3.3TRG1",
            "3.3 V 600 mA LDO, SOT-23-5",
            &["regulator", "ldo", "3v3"],
        ),
        part(
            "2N7002",
            "Q_NMOS_GSD",
            "SOT-23-3",
            "Various",
            "2N7002",
            "N-channel MOSFET 60 V, SOT-23",
            &["mosfet", "nmos"],
        ),
        part(
            "BC847B",
            "Q_NPN_BEC",
            "SOT-23-3",
            "Nexperia",
            "BC847B",
            "NPN transistor 45 V, SOT-23",
            &["bjt", "npn"],
        ),
        part(
            "SS14",
            "D_Schottky",
            "SMA_DO-214AC",
            "Various",
            "SS14",
            "Schottky diode 40 V 1 A, SMA",
            &["diode", "schottky"],
        ),
    ];
    for n in 2..=10 {
        let name = format!("Conn_01x{n:02}_P2.54mm");
        let fp = Header::standard(n, 1)
            .generate()
            .map(|f| f.name)
            .unwrap_or_default();
        v.push(part(
            &name,
            &format!("Conn_01x{n:02}"),
            &fp,
            "",
            "",
            &format!("Pin header 1x{n}, 2.54 mm"),
            &["header", "connector"],
        ));
    }
    v
}

// -- public API ----------------------------------------------------------------------------------

/// The standard symbols as a library (validated).
pub fn symbol_library() -> Result<SymbolLibrary> {
    let mut l = SymbolLibrary::new(NAME)?;
    for s in symbols_list() {
        l.insert(s)?;
    }
    Ok(l)
}

/// The standard footprints as a library (validated).
pub fn footprint_library() -> Result<FootprintLibrary> {
    let mut l = FootprintLibrary::new(NAME)?;
    for f in footprints_list() {
        l.insert(f)?;
    }
    Ok(l)
}

/// The standard components as a library (validated).
pub fn component_library() -> Result<ComponentLibrary> {
    let mut l = ComponentLibrary::new(NAME)?;
    for c in components_list() {
        l.insert(c)?;
    }
    Ok(l)
}

/// Standard symbols with qualified names (`Std:Name`).
pub fn symbols() -> Vec<Symbol> {
    symbol_library()
        .map(|l| l.names().filter_map(|n| l.instantiate(n)).collect())
        .unwrap_or_default()
}

/// Standard footprints with qualified names (`Std:Name`).
pub fn footprints() -> Vec<Footprint> {
    footprint_library()
        .map(|l| l.names().filter_map(|n| l.instantiate(n)).collect())
        .unwrap_or_default()
}

/// Standard components.
pub fn components() -> Vec<Component> {
    component_library()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_library_is_valid_and_complete() {
        let (s, f, c) = (
            symbol_library().unwrap(),
            footprint_library().unwrap(),
            component_library().unwrap(),
        );
        assert!(
            s.len() >= 50 && f.len() >= 70 && c.len() >= 15,
            "{} {} {}",
            s.len(),
            f.len(),
            c.len()
        );
        // Every component refers to a symbol and a footprint that exist, and the symbol's pins
        // are all present on the footprint (same numbers) — except no-connect and tab handling.
        for comp in components() {
            let (_, sym) = comp.symbol.split_once(':').unwrap();
            let (_, fp) = comp.footprint.split_once(':').unwrap();
            let sym = s
                .get(sym)
                .unwrap_or_else(|| panic!("{}: symbol {}", comp.name, comp.symbol));
            let fp = f
                .get(fp)
                .unwrap_or_else(|| panic!("{}: footprint {}", comp.name, comp.footprint));
            for pin in &sym.pins {
                assert!(
                    fp.pads.iter().any(|pad| pad.number == pin.number),
                    "{}: pin {} ({}) of {} has no pad on {}",
                    comp.name,
                    pin.number,
                    pin.name,
                    comp.symbol,
                    comp.footprint
                );
            }
        }
    }

    #[test]
    fn the_library_is_deterministic() {
        assert_eq!(
            symbol_library().unwrap().to_bytes().unwrap(),
            symbol_library().unwrap().to_bytes().unwrap()
        );
        assert_eq!(
            footprint_library().unwrap().to_bytes().unwrap(),
            footprint_library().unwrap().to_bytes().unwrap()
        );
    }

    #[test]
    fn symbol_pins_sit_on_the_grid_and_names_are_unique() {
        let g = 1_270_000;
        for sym in symbols() {
            let mut seen = std::collections::BTreeSet::new();
            for pin in &sym.pins {
                assert!(
                    seen.insert(pin.number.clone()),
                    "{}: duplicate pin {}",
                    sym.name,
                    pin.number
                );
                assert_eq!(
                    (pin.position.x % g, pin.position.y % g),
                    (0, 0),
                    "{} pin {} is off the 1.27 mm grid",
                    sym.name,
                    pin.number
                );
            }
        }
    }
}
