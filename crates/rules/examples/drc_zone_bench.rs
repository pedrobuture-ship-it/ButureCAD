//! DRC time on a large board with a filled pour: `cargo run --release -p buturecad-eda-core --example drc_zone_bench`.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use buturecad_eda_core::commands::PcbEditCommand;
use buturecad_eda_core::model::*;
use buturecad_eda_core::zonefill::fill_changes;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net, Zone};
use buturecad_rules::run_drc;
use std::time::Instant;

fn main() {
    let mut e = CommandEngine::new(Project::new("bench").unwrap());
    let (gnd, sig) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(gnd, PcbObject::Net(Net { name: "GND".into() })),
        PcbChange::create(sig, PcbObject::Net(Net { name: "SIG".into() })),
    ];
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let (w, h) = (200_000_000_i64, 150_000_000_i64);
    // 4000 tracks and 600 vias of SIG, 40 short GND stubs so the pour has copper to connect to.
    for _ in 0..4000 {
        let (x, y) = (
            (next() % 190_000) as i64 * 1000 + 5_000_000,
            (next() % 140_000) as i64 * 1000 + 5_000_000,
        );
        let (dx, dy) = (
            (next() % 8_000) as i64 * 1000 - 4_000_000,
            (next() % 8_000) as i64 * 1000 - 4_000_000,
        );
        let id = e.new_id();
        ch.push(PcbChange::create(
            id,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(Point::new(x, y), Point::new(x + dx, y + dy)),
                width: 250_000,
                net: Some(sig),
            }),
        ));
    }
    for _ in 0..600 {
        let id = e.new_id();
        ch.push(PcbChange::create(
            id,
            PcbObject::Via(Via {
                position: Point::new(
                    (next() % 190_000) as i64 * 1000 + 5_000_000,
                    (next() % 140_000) as i64 * 1000 + 5_000_000,
                ),
                diameter: 800_000,
                drill: 400_000,
                net: Some(sig),
                kind: ViaKind::Through,
            }),
        ));
    }
    for i in 0..40 {
        let id = e.new_id();
        let x = 10_000_000 + i * 4_000_000;
        ch.push(PcbChange::create(
            id,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(
                    Point::new(x, 1_000_000),
                    Point::new(x + 1_000_000, 1_000_000),
                ),
                width: 250_000,
                net: Some(gnd),
            }),
        ));
    }
    let zid = e.new_id();
    ch.push(PcbChange::create(
        zid,
        PcbObject::Zone(Zone::new(
            layer::F_CU,
            vec![
                Point::new(0, 0),
                Point::new(w, 0),
                Point::new(w, h),
                Point::new(0, h),
            ],
            Some(gnd),
        )),
    ));
    e.execute(PcbEditCommand::new("fill board", ch)).unwrap();
    let t = Instant::now();
    let changes = fill_changes(e.project().pcb(), None, false);
    println!(
        "first fill (200 x 150 mm, 4000 tracks, 600 vias): {:?}",
        t.elapsed()
    );
    e.execute(PcbEditCommand::new("fill", changes)).unwrap();
    let z = &e.project().pcb().zones[&zid];
    let f = z.fill.as_ref().unwrap();
    println!(
        "{} regions, {} contour points",
        f.regions.len(),
        f.regions
            .iter()
            .map(|r| r.outer.len() + r.holes.iter().map(Vec::len).sum::<usize>())
            .sum::<usize>()
    );
    let t = Instant::now();
    let report = run_drc(e.project().pcb());
    println!(
        "full DRC with the pour: {:?} ({} errors, {} warnings)",
        t.elapsed(),
        report.errors(),
        report.warnings()
    );
    let mut by_rule: std::collections::BTreeMap<&str, usize> = Default::default();
    for v in &report.violations {
        *by_rule.entry(v.rule.code()).or_default() += 1;
    }
    println!("{by_rule:?}");
    if let Some(v) = report.violations.first() {
        println!("first: {} — {}", v.rule.code(), v.message);
    }
}
