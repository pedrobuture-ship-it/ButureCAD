//! Walk-around latency on a dense board: `cargo run --release -p buturecad-router --example router_bench`.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use buturecad_eda_core::commands::PcbEditCommand;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net};
use buturecad_router::{shove, walk_around, NewCopper, ObstacleSet, RouteParams};
use std::time::Instant;

fn main() {
    let mut e = CommandEngine::new(Project::new("bench").unwrap());
    let (a, b) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(a, PcbObject::Net(Net { name: "A".into() })),
        PcbChange::create(b, PcbObject::Net(Net { name: "B".into() })),
    ];
    // 3000 short tracks of net B on a 200 x 150 mm board, pseudo-random but deterministic.
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for _ in 0..3000 {
        let (x, y) = (
            (next() % 200_000) as i64 * 1000,
            (next() % 150_000) as i64 * 1000,
        );
        let (dx, dy) = (
            (next() % 4_000) as i64 * 1000 - 2_000_000,
            (next() % 4_000) as i64 * 1000 - 2_000_000,
        );
        let id = e.new_id();
        ch.push(PcbChange::create(
            id,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(Point::new(x, y), Point::new(x + dx, y + dy)),
                width: 250_000,
                net: Some(b),
            }),
        ));
    }
    e.execute(PcbEditCommand::new("fill", ch)).unwrap();
    let params = RouteParams {
        net: Some(a),
        layer: layer::F_CU,
        width: 250_000,
    };
    let t = Instant::now();
    let set = ObstacleSet::collect(e.project().pcb(), params);
    println!("collect {} obstacles: {:?}", set.len(), t.elapsed());
    let mut ok = 0;
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    for _ in 0..50 {
        let from = Point::new(
            (next() % 190_000) as i64 * 1000 + 5_000_000,
            (next() % 140_000) as i64 * 1000 + 5_000_000,
        );
        let to = Point::new(from.x + 20_000_000, from.y + 10_000_000);
        let t = Instant::now();
        let r = walk_around(&set, from, to);
        let d = t.elapsed();
        total += d;
        worst = worst.max(d);
        ok += usize::from(r.is_ok());
    }
    println!(
        "50 legs of ~22 mm: {ok} routed, mean {:?}, worst {:?}",
        total / 50,
        worst
    );

    let mut total = std::time::Duration::ZERO;
    let mut worst = std::time::Duration::ZERO;
    let mut ok = 0;
    for _ in 0..20 {
        let from = Point::new(
            (next() % 190_000) as i64 * 1000 + 5_000_000,
            (next() % 140_000) as i64 * 1000 + 5_000_000,
        );
        let to = Point::new(from.x + 15_000_000, from.y);
        let tracks = vec![(layer::F_CU, vec![from, to])];
        let t = Instant::now();
        let r = shove(
            e.project().pcb(),
            &NewCopper {
                net: Some(a),
                width: 250_000,
                tracks: &tracks,
                vias: &[],
                via_diameter: 800_000,
            },
        );
        let d = t.elapsed();
        total += d;
        worst = worst.max(d);
        ok += usize::from(r.is_ok());
    }
    println!(
        "20 shoves of 15 mm: {ok} accepted, mean {:?}, worst {:?}",
        total / 20,
        worst
    );
}
