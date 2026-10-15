//! Writes the symbol and footprint libraries of the reference board (`examples/reference-board`).
//!
//! `cargo run -p buturecad-libraries --example reference_libs -- examples/reference-board/libraries`
//!
//! The pin numbers and names of the microcontroller follow the public STM32F103x8 datasheet
//! (UFQFPN48 / LQFP48 share one numbering). Symbol drawings and land patterns are our own and
//! **nominal**: a validation design for ButureCAD, not a fabrication-approved library.

use buturecad_geometry::{Point, Rectangle, Segment};
use buturecad_libraries::{FootprintLibrary, LibraryDir, SymbolLibrary};
use buturecad_pcb::footprint::{FpGraphic, FpLayer, FpShape, FpText, MountType, Pad, TextKind};
use buturecad_pcb::Footprint;
use buturecad_schematic::{Direction, Graphic, Pin, PinType, Symbol, SymbolMeta};
use std::process::ExitCode;

const MM: f64 = 1_000_000.0;
fn u(v: f64) -> i64 {
    (v * MM).round() as i64
}
fn p(x: f64, y: f64) -> Point {
    Point::new(u(x), u(y))
}

// -- symbols -----------------------------------------------------------------------------------

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

fn body(a: (f64, f64), b: (f64, f64)) -> Option<Graphic> {
    Some(Graphic::Rect {
        rect: Rectangle::new(p(a.0, a.1), p(b.0, b.1)).ok()?,
        filled: false,
        width: 150_000,
    })
}

fn symbol(
    name: &str,
    prefix: &str,
    value: &str,
    pins: Vec<Pin>,
    g: Vec<Graphic>,
    d: &str,
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
            description: d.into(),
            ..SymbolMeta::default()
        },
    }
}

/// (number, name, type, side) of the STM32F103C8 in 48 pins; side: L R T B.
const MCU: &[(&str, &str, char, char)] = &[
    ("1", "VBAT", 'P', 'T'),
    ("9", "VDDA", 'P', 'T'),
    ("24", "VDD_1", 'P', 'T'),
    ("36", "VDD_2", 'P', 'T'),
    ("48", "VDD_3", 'P', 'T'),
    ("7", "NRST", 'I', 'T'),
    ("44", "BOOT0", 'I', 'T'),
    ("8", "VSSA", 'P', 'B'),
    ("23", "VSS_1", 'P', 'B'),
    ("35", "VSS_2", 'P', 'B'),
    ("47", "VSS_3", 'P', 'B'),
    ("49", "EP", 'P', 'B'),
    ("5", "PD0_OSC_IN", 'B', 'B'),
    ("6", "PD1_OSC_OUT", 'B', 'B'),
    ("2", "PC13", 'B', 'B'),
    ("3", "PC14_OSC32_IN", 'B', 'B'),
    ("4", "PC15_OSC32_OUT", 'B', 'B'),
    ("18", "PB0", 'B', 'L'),
    ("19", "PB1", 'B', 'L'),
    ("20", "PB2_BOOT1", 'B', 'L'),
    ("39", "PB3", 'B', 'L'),
    ("40", "PB4", 'B', 'L'),
    ("41", "PB5", 'B', 'L'),
    ("42", "PB6", 'B', 'L'),
    ("43", "PB7", 'B', 'L'),
    ("45", "PB8", 'B', 'L'),
    ("46", "PB9", 'B', 'L'),
    ("21", "PB10", 'B', 'L'),
    ("22", "PB11", 'B', 'L'),
    ("25", "PB12", 'B', 'L'),
    ("26", "PB13", 'B', 'L'),
    ("27", "PB14", 'B', 'L'),
    ("28", "PB15", 'B', 'L'),
    ("10", "PA0", 'B', 'R'),
    ("11", "PA1", 'B', 'R'),
    ("12", "PA2", 'B', 'R'),
    ("13", "PA3", 'B', 'R'),
    ("14", "PA4", 'B', 'R'),
    ("15", "PA5", 'B', 'R'),
    ("16", "PA6", 'B', 'R'),
    ("17", "PA7", 'B', 'R'),
    ("29", "PA8", 'B', 'R'),
    ("30", "PA9", 'B', 'R'),
    ("31", "PA10", 'B', 'R'),
    ("32", "PA11_USB_DM", 'B', 'R'),
    ("33", "PA12_USB_DP", 'B', 'R'),
    ("34", "PA13_SWDIO", 'B', 'R'),
    ("37", "PA14_SWCLK", 'B', 'R'),
    ("38", "PA15", 'B', 'R'),
];

/// Pin positions on the 1.27 mm grid.
fn snap(a: (f64, f64)) -> (f64, f64) {
    let g = 1.27;
    ((a.0 / g).round() * g, (a.1 / g).round() * g)
}

fn mcu() -> Symbol {
    let (w, h) = (17.78, 20.32);
    let mut pins = Vec::new();
    for side in ['L', 'R', 'T', 'B'] {
        let list: Vec<_> = MCU.iter().filter(|m| m.3 == side).collect();
        let n = list.len() as f64;
        for (i, (num, name, t, _)) in list.iter().enumerate() {
            let kind = match t {
                'P' => PinType::PowerInput,
                'I' => PinType::Input,
                _ => PinType::Bidirectional,
            };
            let off = (i as f64 - (n - 1.0) / 2.0) * 2.54;
            let (at, dir) = match side {
                'L' => ((-w - 2.54, -off), Direction::Right),
                'R' => ((w + 2.54, -off), Direction::Left),
                'T' => ((off, h + 2.54), Direction::Down),
                _ => ((off, -h - 2.54), Direction::Up),
            };
            pins.push(pin(num, name, kind, snap(at), dir));
        }
    }
    symbol(
        "Ref:STM32F103C8U6",
        "U",
        "STM32F103C8U6",
        pins,
        body((-w, -h), (w, h)).into_iter().collect(),
        "ARM Cortex-M3 microcontroller, 64 KB flash, USB FS, UFQFPN48 (reference symbol)",
    )
}

fn ldo() -> Symbol {
    let pins = vec![
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
    symbol(
        "Ref:AP2112K-3.3",
        "U",
        "AP2112K-3.3",
        pins,
        body((-5.08, -5.08), (5.08, 5.08)).into_iter().collect(),
        "600 mA 3.3 V LDO regulator, SOT-23-5 (reference symbol)",
    )
}

fn usb_micro_b() -> Symbol {
    let pins = vec![
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
    symbol(
        "Ref:USB_Micro_B",
        "J",
        "USB_Micro_B",
        pins,
        body((-5.08, -7.62), (2.54, 7.62)).into_iter().collect(),
        "USB 2.0 Micro-B receptacle, SMD (reference symbol)",
    )
}

fn crystal() -> Symbol {
    let pins = vec![
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
    symbol(
        "Ref:Crystal_4Pad",
        "Y",
        "8MHz",
        pins,
        body((-5.08, -5.08), (5.08, 5.08)).into_iter().collect(),
        "Crystal, 4 pads (2 are ground), 3.2 x 2.5 mm (reference symbol)",
    )
}

fn push_switch() -> Symbol {
    let pins = vec![
        pin("1", "A", PinType::Passive, (-5.08, 0.0), Direction::Right),
        pin("2", "B", PinType::Passive, (5.08, 0.0), Direction::Left),
    ];
    symbol(
        "Ref:SW_Push",
        "SW",
        "SW_Push",
        pins,
        body((-2.54, -1.27), (2.54, 1.27)).into_iter().collect(),
        "Tactile push button (reference symbol)",
    )
}

// -- footprints --------------------------------------------------------------------------------

fn line(layer: FpLayer, a: Point, b: Point, w: i64) -> FpGraphic {
    FpGraphic {
        layer,
        shape: FpShape::Line {
            segment: Segment::new(a, b),
            width: w,
        },
    }
}

fn rect(layer: FpLayer, a: Point, b: Point, w: i64) -> Option<FpGraphic> {
    Some(FpGraphic {
        layer,
        shape: FpShape::Rect {
            rect: Rectangle::new(a, b).ok()?,
            filled: false,
            width: w,
        },
    })
}

fn footprint(
    name: &str,
    desc: &str,
    pads: Vec<Pad>,
    body: (f64, f64),
    court: (f64, f64),
) -> Footprint {
    let mut f = Footprint::new(name);
    f.description = desc.into();
    f.attributes.mount = MountType::Smd;
    f.pads = pads;
    f.graphics.extend(rect(
        FpLayer::Fabrication,
        p(-body.0, -body.1),
        p(body.0, body.1),
        100_000,
    ));
    f.graphics.extend(rect(
        FpLayer::Courtyard,
        p(-court.0, -court.1),
        p(court.0, court.1),
        50_000,
    ));
    f.graphics.push(line(
        FpLayer::Silkscreen,
        p(-court.0, -court.1 - 0.4),
        p(-court.0 + 0.6, -court.1 - 0.4),
        120_000,
    ));
    for (kind, text, y) in [
        (TextKind::Reference, "REF**", -court.1 - 1.2),
        (TextKind::Value, "VAL**", court.1 + 1.2),
    ] {
        f.texts.push(FpText {
            kind,
            layer: FpLayer::Silkscreen,
            text: text.into(),
            position: p(0.0, y),
            height: u(1.0),
            rotation: 0,
        });
    }
    f
}

fn smd(num: &str, x: f64, y: f64, w: f64, h: f64) -> Pad {
    Pad::smd(num, p(x, y), u(w), u(h))
}

fn sot23_5() -> Footprint {
    footprint(
        "Ref:SOT-23-5",
        "SOT-23-5 (SC-74A), 0.95 mm pitch, nominal land pattern",
        vec![
            smd("1", -0.95, 1.2, 0.6, 1.1),
            smd("2", 0.0, 1.2, 0.6, 1.1),
            smd("3", 0.95, 1.2, 0.6, 1.1),
            smd("4", 0.95, -1.2, 0.6, 1.1),
            smd("5", -0.95, -1.2, 0.6, 1.1),
        ],
        (1.5, 0.8),
        (2.1, 2.1),
    )
}

fn usb_micro_b_fp() -> Footprint {
    let mut pads = Vec::new();
    for (i, x) in [-1.3, -0.65, 0.0, 0.65, 1.3].into_iter().enumerate() {
        pads.push(smd(&(i + 1).to_string(), x, 0.0, 0.4, 1.35));
    }
    for x in [-3.6, 3.6] {
        pads.push(smd("6", x, -0.2, 1.8, 1.9));
        pads.push(smd("6", x * 0.55, 2.6, 1.2, 1.9));
    }
    footprint(
        "Ref:USB_Micro_B",
        "USB Micro-B receptacle, SMD, nominal land pattern",
        pads,
        (3.8, 2.5),
        (4.9, 3.8),
    )
}

fn crystal_fp() -> Footprint {
    footprint(
        "Ref:Crystal_3225",
        "Crystal 3.2 x 2.5 mm, 4 pads, nominal land pattern",
        vec![
            smd("1", -1.1, 0.95, 1.4, 1.15),
            smd("2", 1.1, 0.95, 1.4, 1.15),
            smd("3", 1.1, -0.95, 1.4, 1.15),
            smd("4", -1.1, -0.95, 1.4, 1.15),
        ],
        (1.6, 1.25),
        (2.2, 1.9),
    )
}

fn tact_fp() -> Footprint {
    footprint(
        "Ref:SW_Tact_6x6",
        "Tactile switch 6 x 6 mm, 4 legs (pairs shorted), nominal land pattern",
        vec![
            smd("1", -3.1, -2.25, 1.6, 1.6),
            smd("1", -3.1, 2.25, 1.6, 1.6),
            smd("2", 3.1, -2.25, 1.6, 1.6),
            smd("2", 3.1, 2.25, 1.6, 1.6),
        ],
        (3.0, 3.0),
        (4.2, 3.4),
    )
}

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: reference_libs <libraries-dir>");
        return ExitCode::from(2);
    };
    match write(&dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write(dir: &str) -> Result<(), String> {
    let store = LibraryDir::new(dir);
    let mut syms = SymbolLibrary::new("Ref").map_err(|e| e.to_string())?;
    for s in [mcu(), ldo(), usb_micro_b(), crystal(), push_switch()] {
        syms.insert(s).map_err(|e| e.to_string())?;
    }
    store
        .save_symbol_library(&syms)
        .map_err(|e| e.to_string())?;
    let mut fps = FootprintLibrary::new("Ref").map_err(|e| e.to_string())?;
    for f in [sot23_5(), usb_micro_b_fp(), crystal_fp(), tact_fp()] {
        fps.insert(f).map_err(|e| e.to_string())?;
    }
    store
        .save_footprint_library(&fps)
        .map_err(|e| e.to_string())?;
    Ok(())
}
