//! The parametric generators produce real, valid, symmetric footprints.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_geometry::Point;
use buturecad_pcb::generators::*;
use buturecad_pcb::*;

fn pad<'a>(fp: &'a Footprint, n: &str) -> &'a Pad {
    fp.pads
        .iter()
        .find(|p| p.number == n)
        .unwrap_or_else(|| panic!("no pad {n}"))
}

fn pos(fp: &Footprint, n: &str) -> Point {
    pad(fp, n).position
}

/// Every generated footprint must validate with no warnings: courtyard encloses the pads, no
/// overlapping copper, reference and value present.
fn assert_sound(fp: &Footprint) {
    validate(fp).unwrap();
    assert_eq!(warnings(fp), Vec::<String>::new(), "{}", fp.name);
    // Round-trips through JSON.
    let back: Footprint = serde_json::from_str(&serde_json::to_string(fp).unwrap()).unwrap();
    assert_eq!(&back, fp);
    // Texts and graphics exist on the right layers.
    assert!(fp.graphics.iter().any(|g| g.layer == FpLayer::Silkscreen));
    assert!(fp.graphics.iter().any(|g| g.layer == FpLayer::Fabrication));
    assert!(fp.graphics.iter().any(|g| g.layer == FpLayer::Courtyard));
    assert!(fp.texts.iter().any(|t| t.kind == TextKind::Reference));
    assert!(fp.texts.iter().any(|t| t.kind == TextKind::Value));
}

#[test]
fn dip_numbers_counter_clockwise_from_top_left() {
    for pins in [4, 8, 14, 16, 28, 40] {
        let fp = Dip::standard(pins).generate().unwrap();
        assert_sound(&fp);
        assert_eq!(fp.pads.len() as u32, pins);
        assert!(fp
            .pads
            .iter()
            .all(|p| p.kind == PadKind::Through && p.drill.is_some()));
        let half = pins / 2;
        let (p1, ph, ph1, pn) = (
            pos(&fp, "1"),
            pos(&fp, &half.to_string()),
            pos(&fp, &(half + 1).to_string()),
            pos(&fp, &pins.to_string()),
        );
        assert!(p1.x < 0 && p1.y > 0, "pin 1 is top-left");
        assert_eq!((p1.x, ph.x), (-3_810_000, -3_810_000));
        assert_eq!((ph1.x, pn.x), (3_810_000, 3_810_000));
        assert_eq!(ph.y, ph1.y, "the numbering turns the corner at the bottom");
        assert_eq!(p1.y, pn.y, "and ends opposite pin 1");
        assert_eq!(p1.y - ph.y, i64::from(half - 1) * 2_540_000);
        assert_eq!(pad(&fp, "1").shape, PadShape::Rect, "pin 1 is square");
        // Symmetric about the origin.
        let (sx, sy): (i64, i64) = fp
            .pads
            .iter()
            .fold((0, 0), |a, p| (a.0 + p.position.x, a.1 + p.position.y));
        assert_eq!((sx, sy), (0, 0));
    }
    let wide = generate(
        "dip",
        &[("pins".into(), "28".into()), ("wide".into(), "true".into())],
    )
    .unwrap();
    assert_eq!(pos(&wide, "1").x, -7_620_000);
    assert_sound(&wide);
}

#[test]
fn soic_pads_are_on_the_pitch_and_pin_1_is_top_left() {
    for (pins, wide) in [(8, false), (14, false), (16, true), (28, true)] {
        let s = if wide {
            Soic::wide(pins)
        } else {
            Soic::narrow(pins)
        };
        let fp = s.generate().unwrap();
        assert_sound(&fp);
        assert_eq!(fp.pads.len() as u32, pins);
        assert!(fp
            .pads
            .iter()
            .all(|p| p.kind == PadKind::Smd && p.drill.is_none()));
        let (p1, pn) = (pos(&fp, "1"), pos(&fp, &pins.to_string()));
        assert!(p1.x < 0 && p1.y > 0 && pn.x > 0 && pn.y == p1.y);
        assert_eq!(pos(&fp, "2").y - p1.y, -1_270_000);
        assert_eq!(p1.x, -(s.row_spacing / 2));
    }
    let fp = Soic::narrow(8).generate().unwrap();
    assert_eq!(fp.name, "SOIC-8_3.9mm_P1.27mm");
    assert_eq!(pos(&fp, "1"), Point::new(-2_700_000, 1_905_000));
}

#[test]
fn qfn_numbers_counter_clockwise_and_adds_an_exposed_pad() {
    let mut q = Qfn::square(4, mm(4.0), mm(0.5));
    q.exposed = Some((mm(2.6), mm(2.6)));
    let fp = q.generate().unwrap();
    assert_sound(&fp);
    assert_eq!(fp.pads.len(), 17);
    assert_eq!(
        pad(&fp, "17").position,
        Point::ORIGIN,
        "exposed pad is last and centred"
    );
    // Left side top→bottom (1..4), bottom side left→right (5..8), right side bottom→top
    // (9..12), top side right→left (13..16).
    assert!(pos(&fp, "1").x < 0 && pos(&fp, "1").y > pos(&fp, "4").y);
    assert!(pos(&fp, "5").y < 0 && pos(&fp, "5").x < pos(&fp, "8").x);
    assert!(pos(&fp, "9").x > 0 && pos(&fp, "9").y < pos(&fp, "12").y);
    assert!(pos(&fp, "13").y > 0 && pos(&fp, "13").x > pos(&fp, "16").x);
    assert_eq!(pos(&fp, "2").y - pos(&fp, "1").y, -500_000);
    // The whole pad ring is symmetric.
    let ring: Vec<_> = fp.pads.iter().filter(|p| p.number != "17").collect();
    let (sx, sy): (i64, i64) = ring
        .iter()
        .fold((0, 0), |a, p| (a.0 + p.position.x, a.1 + p.position.y));
    assert_eq!((sx, sy), (0, 0));
    // Rotating the part by a quarter turn maps pad n to pad n+4 (the 4-fold symmetry).
    for n in 1..=12 {
        let a = pos(&fp, &n.to_string()).rotate_quarter_turns(Point::ORIGIN, 1);
        let b = pos(&fp, &(n + 4).to_string());
        assert_eq!(a, b, "pad {n}");
    }
    for pins in [2, 3, 5, 8, 12] {
        assert_sound(
            &Qfn::square(pins, mm(2.0 + pins as f64 * 0.5), mm(0.5))
                .generate()
                .unwrap(),
        );
    }
}

#[test]
fn bga_names_rows_with_jedec_letters() {
    let fp = Bga {
        rows: 10,
        columns: 6,
        pitch: mm(1.0),
        pad_diameter: mm(0.5),
        body: (mm(8.0), mm(12.0)),
    }
    .generate()
    .unwrap();
    assert_sound(&fp);
    assert_eq!(fp.pads.len(), 60);
    assert!(fp
        .pads
        .iter()
        .all(|p| p.shape == PadShape::Circle && p.kind == PadKind::Smd));
    let (a1, f1, k6) = (pos(&fp, "A1"), pos(&fp, "F1"), pos(&fp, "K6"));
    assert!(a1.x < 0 && a1.y > 0, "A1 is top-left");
    assert!(
        k6.x > 0 && k6.y < 0,
        "the last ball is bottom-right (row 10 = K, I skipped)"
    );
    assert_eq!(f1.y - a1.y, -5_000_000);
    assert!(fp.pads.iter().all(|p| !p.number.contains('I')));
    let names: std::collections::BTreeSet<_> = fp.pads.iter().map(|p| &p.number).collect();
    assert_eq!(names.len(), 60, "unique names");
    assert_sound(
        &generate(
            "bga",
            &[("rows".into(), "5".into()), ("columns".into(), "5".into())],
        )
        .unwrap(),
    );
}

#[test]
fn headers_number_row_major_from_pin_1() {
    let fp = Header::standard(4, 1).generate().unwrap();
    assert_sound(&fp);
    assert_eq!(pos(&fp, "1"), Point::ORIGIN);
    assert_eq!(pos(&fp, "4"), Point::new(0, -7_620_000));
    let fp = Header::standard(5, 2).generate().unwrap();
    assert_sound(&fp);
    assert_eq!(fp.pads.len(), 10);
    assert_eq!(
        (pos(&fp, "1"), pos(&fp, "2")),
        (Point::ORIGIN, Point::new(2_540_000, 0))
    );
    assert_eq!(pos(&fp, "3"), Point::new(0, -2_540_000));
    assert_eq!(pos(&fp, "10"), Point::new(2_540_000, -10_160_000));
    assert_eq!(pad(&fp, "1").shape, PadShape::Rect);
    assert_eq!(pad(&fp, "2").shape, PadShape::Circle);
    for n in [1, 2, 10, 40] {
        assert_sound(&Header::standard(n, 1).generate().unwrap());
    }
}

#[test]
fn generation_is_deterministic_and_dispatch_validates_every_key() {
    let a = generate("soic", &[("pins".into(), "16".into())]).unwrap();
    let b = generate("soic", &[("pins".into(), "16".into())]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.pads.len(), 16);
    let q = generate(
        "qfn",
        &[
            ("pins_per_side".into(), "6".into()),
            ("body".into(), "5".into()),
            ("exposed".into(), "3.2".into()),
        ],
    )
    .unwrap();
    assert_eq!(q.pads.len(), 25);
    let h = generate(
        "header",
        &[
            ("pins".into(), "3".into()),
            ("rows".into(), "2".into()),
            ("pitch".into(), "2.0".into()),
        ],
    )
    .unwrap();
    assert_eq!(pos(&h, "2").x, 2_000_000);
    assert!(generate("soic", &[("rows".into(), "2".into())]).is_err());
    for family in FAMILIES {
        let fp = generate(family, &[]).unwrap_or_else(|e| panic!("{family}: {e}"));
        assert_sound(&fp);
    }
}
