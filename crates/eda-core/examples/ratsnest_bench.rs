//! Benchmark: full ratsnest vs. updating the nets a move affects.
//! `cargo run --release -p buturecad-eda-core --example ratsnest_bench [footprints] [nets]`
#![allow(clippy::unwrap_used)]

use buturecad_eda_core::model::*;
use buturecad_eda_core::ratsnest::*;
use buturecad_geometry::Point;
use buturecad_pcb::{Footprint, Net, Pad, PlacedFootprint, Placement};
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let n: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4000);
    let nets: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1500);
    let mm = 1_000_000;
    let mut doc = PcbDoc::default();
    let mut f = Footprint::new("T:SOIC");
    for i in 0..8 {
        f.pads.push(Pad::smd(
            (i + 1).to_string(),
            Point::new(0, i * 1_270_000),
            600_000,
            600_000,
        ));
    }
    doc.footprints.insert("T:SOIC".into(), f);
    for k in 0..nets {
        doc.nets.insert(
            Id(1 + k),
            Net {
                name: format!("N{k}"),
            },
        );
    }
    let cols = 80;
    for i in 0..n {
        let mut pad_nets = BTreeMap::new();
        for p in 0..8u64 {
            // Pseudo-random but deterministic net assignment.
            let net = 1 + (i * 7919 + p * 104_729) % nets;
            pad_nets.insert((p + 1).to_string(), Id(net));
        }
        doc.placed.insert(
            Id(10_000 + i),
            PlacedFootprint {
                footprint: "T:SOIC".into(),
                reference: format!("U{i}"),
                value: String::new(),
                placement: Placement::at(Point::new(
                    (i % cols) as i64 * 15 * mm,
                    (i / cols) as i64 * 20 * mm,
                )),
                locked: false,
                link: None,
                pad_nets,
            },
        );
    }
    let t = Instant::now();
    let mut e = RatsnestEngine::new(&doc);
    let full = t.elapsed();
    let lines = e.count();

    // Move one footprint: only its ≤ 8 nets are recomputed.
    let id = Id(10_000);
    let before = doc.object(id).unwrap();
    let PcbObject::Footprint(mut moved) = before.clone() else {
        return;
    };
    moved.placement.position = Point::new(3 * mm, 3 * mm);
    let change = PcbChange::modify(id, before, PcbObject::Footprint(moved));
    doc.apply_changes(std::slice::from_ref(&change)).unwrap();
    let t = Instant::now();
    let dirty = affected_nets(std::slice::from_ref(&change));
    let k = e.update(&doc, &dirty);
    let inc = t.elapsed();
    assert_eq!(e, RatsnestEngine::new(&doc));
    println!("{n} footprints, {nets} nets, {lines} ratsnest lines");
    println!("full ratsnest        {full:?}");
    println!("move one footprint   {inc:?}  ({k} nets recomputed)");
    println!(
        "speed-up             {:.0}x",
        full.as_secs_f64() / inc.as_secs_f64().max(1e-9)
    );
}
