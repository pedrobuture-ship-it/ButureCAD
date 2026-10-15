//! The standard package footprints against the **published nominal package outlines**
//! (pitch, pin count, body size, lead span). This checks the geometry we generate against numbers
//! written down independently of the generator, with the source of each number.
//!
//! What it does NOT prove: that the *land pattern* (pad sizes) is what a given vendor recommends.
//! That needs the datasheet of the part (see `docs/standard-library.md`).
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_pcb::generators::{lqfp, msop8, sma, sod123, sot223, sot23, tssop};
use buturecad_pcb::{Footprint, FpLayer, FpShape};

const MM: f64 = 1_000_000.0;
const TOE: f64 = 0.35; // the land-pattern rule's toe: pads extend this far beyond the lead tips

struct Outline {
    name: &'static str,
    fp: Footprint,
    pins: usize,
    pitch: f64,
    body: (f64, f64),
    /// Lead tip to lead tip across the package.
    span: Option<f64>,
    source: &'static str,
}

fn body_of(f: &Footprint) -> (f64, f64) {
    let r = f
        .graphics
        .iter()
        .find_map(|g| match (&g.layer, &g.shape) {
            (FpLayer::Fabrication, FpShape::Rect { rect, .. }) => Some(*rect),
            _ => None,
        })
        .unwrap();
    (
        (r.max().x - r.min().x) as f64 / MM,
        (r.max().y - r.min().y) as f64 / MM,
    )
}

/// Largest |x| (or |y|) edge of the pads, minus the toe, doubled: the lead tip span the land
/// pattern was built for.
fn span_x(f: &Footprint) -> f64 {
    let edge = f
        .pads
        .iter()
        .map(|p| p.bounds().1.x.max(-p.bounds().0.x))
        .max()
        .unwrap();
    2.0 * (edge as f64 / MM - TOE)
}
fn span_y(f: &Footprint) -> f64 {
    let edge = f
        .pads
        .iter()
        .map(|p| p.bounds().1.y.max(-p.bounds().0.y))
        .max()
        .unwrap();
    2.0 * (edge as f64 / MM - TOE)
}

/// Smallest centre-to-centre distance between two pads of one row.
fn min_pitch(f: &Footprint) -> f64 {
    let mut best = f64::MAX;
    for (i, a) in f.pads.iter().enumerate() {
        for b in &f.pads[i + 1..] {
            let d = ((a.position.x - b.position.x) as f64)
                .hypot((a.position.y - b.position.y) as f64)
                / MM;
            if d > 0.0 {
                best = best.min(d);
            }
        }
    }
    best
}

fn outlines() -> Vec<Outline> {
    let mut v = vec![
        Outline { name: "SOT-23-5", fp: sot23(5).unwrap(), pins: 5, pitch: 0.95, body: (2.9, 1.6), span: Some(2.4),
            source: "JEDEC MO-178; body 2.90x1.60, pitch 0.95 (cross-checked: Renesas P5.064, Microchip, AOS SOT23-5 drawings)" },
        Outline { name: "SOT-23-6", fp: sot23(6).unwrap(), pins: 6, pitch: 0.95, body: (2.9, 1.6), span: Some(2.4), source: "JEDEC MO-178" },
        Outline { name: "SOT-23-3", fp: sot23(3).unwrap(), pins: 3, pitch: 1.9, body: (2.9, 1.3), span: Some(2.4), source: "JEDEC TO-236 (pins 1 and 2 are 1.9 mm apart)" },
        Outline { name: "TSSOP-20", fp: tssop(20).unwrap(), pins: 20, pitch: 0.65, body: (4.4, 6.5), span: Some(6.4),
            source: "JEDEC MO-153 AD (cross-checked: NXP SOT360-1/-2, Renesas MOA: 6.5x4.4 body, 0.65 pitch, tips 6.2-6.6)" },
        Outline { name: "TSSOP-14", fp: tssop(14).unwrap(), pins: 14, pitch: 0.65, body: (4.4, 5.0), span: Some(6.4), source: "JEDEC MO-153 AB" },
        Outline { name: "TSSOP-8", fp: tssop(8).unwrap(), pins: 8, pitch: 0.65, body: (4.4, 3.0), span: Some(6.4), source: "JEDEC MO-153 AA" },
        Outline { name: "MSOP-8", fp: msop8().unwrap(), pins: 8, pitch: 0.65, body: (3.0, 3.0), span: Some(4.9), source: "JEDEC MO-187" },
        Outline { name: "SOT-223", fp: sot223().unwrap(), pins: 4, pitch: 2.3, body: (6.5, 3.5), span: Some(7.0), source: "JEDEC TO-261 (6.5 x 3.5 body, 2.3 pitch)" },
        Outline { name: "SOD-123", fp: sod123().unwrap(), pins: 2, pitch: 0.0, body: (2.7, 1.6), span: Some(3.7), source: "JEDEC MO-219" },
        Outline { name: "SMA", fp: sma().unwrap(), pins: 2, pitch: 0.0, body: (4.3, 2.6), span: Some(5.3), source: "JEDEC DO-214AC" },
    ];
    for (n, pitch, body, span, name) in [
        (8, 0.8, 7.0, 9.0, "LQFP-32"),
        (11, 0.8, 10.0, 12.0, "LQFP-44"),
        (12, 0.5, 7.0, 9.0, "LQFP-48"),
        (16, 0.5, 10.0, 12.0, "LQFP-64"),
        (25, 0.5, 14.0, 16.0, "LQFP-100"),
        (36, 0.5, 20.0, 22.0, "LQFP-144"),
    ] {
        v.push(Outline {
            name,
            fp: lqfp(n).unwrap(),
            pins: 4 * n as usize,
            pitch,
            body: (body, body),
            span: Some(span),
            source: if name == "LQFP-48" {
                "JEDEC MS-026 (cross-checked: NXP SOT313-3 7x7 body, 0.5 pitch; Holtek 48LQFP tips 8.9-9.1)"
            } else {
                "JEDEC MS-026 nominal (not cross-checked against a drawing)"
            },
        });
    }
    v
}

#[test]
fn generated_footprints_match_the_published_nominal_outlines() {
    for o in outlines() {
        assert_eq!(
            o.fp.pads.len(),
            o.pins,
            "{}: pin count ({})",
            o.name,
            o.source
        );
        let (bw, bl) = body_of(&o.fp);
        // The fabrication outline is the body (a long body may be widened to cover the pad row).
        assert!(
            (bw - o.body.0).abs() < 0.011,
            "{}: body width {bw} vs {} ({})",
            o.name,
            o.body.0,
            o.source
        );
        assert!(
            bl + 0.011 >= o.body.1,
            "{}: body length {bl} vs {} ({})",
            o.name,
            o.body.1,
            o.source
        );
        if o.pitch > 0.0 && o.pins > 5 {
            let p = min_pitch(&o.fp);
            assert!(
                (p - o.pitch).abs() < 0.011,
                "{}: pitch {p} vs {} ({})",
                o.name,
                o.pitch,
                o.source
            );
        }
        if let Some(span) = o.span {
            // Lead tips run across x for the dual-row and two-pad packages and across both axes
            // for quads; SOT-23 / SOT-223 pads sit along y.
            let measured = if o.name.starts_with("SOT") {
                span_y(&o.fp)
            } else {
                span_x(&o.fp)
            };
            assert!(
                (measured - span).abs() < 0.02,
                "{}: lead span {measured} vs {span} ({})",
                o.name,
                o.source
            );
            if o.name.starts_with("LQFP") {
                assert!(
                    (span_y(&o.fp) - span).abs() < 0.02,
                    "{}: quad is not square",
                    o.name
                );
            }
        }
    }
}

#[test]
fn dense_packages_keep_a_manufacturable_gap_between_pads() {
    // Copper gap between neighbouring pads of the same row: at least 0.15 mm.
    for o in outlines() {
        if o.pins < 6 {
            continue;
        }
        let f = &o.fp;
        for (i, a) in f.pads.iter().enumerate() {
            for b in &f.pads[i + 1..] {
                let (al, ah) = a.bounds();
                let (bl, bh) = b.bounds();
                let gap_x = (bl.x - ah.x).max(al.x - bh.x) as f64 / MM;
                let gap_y = (bl.y - ah.y).max(al.y - bh.y) as f64 / MM;
                assert!(
                    gap_x.max(gap_y) >= 0.149,
                    "{}: pads {} and {} are {:.3} mm apart",
                    o.name,
                    a.number,
                    b.number,
                    gap_x.max(gap_y)
                );
            }
        }
    }
}
