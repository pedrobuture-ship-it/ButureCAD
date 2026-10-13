//! Benchmark: full connectivity build vs. incremental update on a hierarchical design.
//!
//! `cargo run --release -p buturecad-schematic --example connectivity_bench`
//! Optional arguments: `<sheets> <resistors per sheet>` (default 40 × 400).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use buturecad_geometry::{Point, Rectangle, Segment};
use buturecad_schematic::{
    builtin, Change, Component, ConnectivityEngine, Id, Object, Placement, Schematic,
    SheetInstance, Wire,
};
use std::time::Instant;

const G: i64 = 1_270_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let sheets: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(40);
    let per_sheet: i64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(400);

    let mut sch = Schematic::new(Id(1));
    builtin::install(&mut sch);
    let mut next = 1000u64;
    let mut id = || {
        next += 1;
        Id(next)
    };
    for s in 0..sheets {
        let sid = if s == 0 { Id(1) } else { Id(1 + s) };
        if s > 0 {
            sch.add_sheet(sid, format!("S{s}")).unwrap();
            let rect = Rectangle::new(
                Point::new(s as i64 * 12 * G, -40 * G),
                Point::new(s as i64 * 12 * G + 10 * G, -30 * G),
            )
            .unwrap();
            sch.insert(
                Id(1),
                id(),
                Object::Sheet(SheetInstance {
                    name: format!("S{s}"),
                    child: sid,
                    rect,
                    pins: vec![],
                }),
            )
            .unwrap();
        }
        // A chain of resistors joined by wires: R pin 2 → next R pin 1.
        for i in 0..per_sheet {
            let (col, row) = (i % 40, i / 40);
            let (x, y) = (col * 4 * G, row * 10 * G);
            sch.insert(
                sid,
                id(),
                Object::Component(Component {
                    lib_id: "Buture:R".into(),
                    reference: format!("R{s}_{i}"),
                    value: String::new(),
                    footprint: String::new(),
                    properties: Default::default(),
                    placement: Placement::at(Point::new(x, y)),
                    unit: 1,
                    representation: 1,
                }),
            )
            .unwrap();
            if col > 0 {
                // R(i-1).2 at (x-4G, -3G) → R(i).1 at (x, 3G): a Z route that touches no other pin.
                let (a, b) = (Point::new(x - 4 * G, y - 3 * G), Point::new(x, y + 3 * G));
                for seg in [
                    Segment::new(a, Point::new(x - 2 * G, y - 3 * G)),
                    Segment::new(
                        Point::new(x - 2 * G, y - 3 * G),
                        Point::new(x - 2 * G, y + 3 * G),
                    ),
                    Segment::new(Point::new(x - 2 * G, y + 3 * G), b),
                ] {
                    sch.insert(sid, id(), Object::Wire(Wire { segment: seg }))
                        .unwrap();
                }
            }
        }
    }
    let objects = sch.object_count();

    let t = Instant::now();
    let mut engine = ConnectivityEngine::new(&sch);
    let full = t.elapsed();
    let nets = engine.connectivity().nets().len();

    // One small edit in the last sheet.
    let last = Id(sheets.max(1));
    let change = Change::create(
        last,
        Id(9_000_000),
        Object::Wire(Wire {
            segment: Segment::new(Point::new(0, -30 * G), Point::new(5 * G, -30 * G)),
        }),
    );
    sch.apply(std::slice::from_ref(&change)).unwrap();
    let t = Instant::now();
    let stats = engine.update(&sch, std::slice::from_ref(&change));
    let incremental = t.elapsed();

    let t = Instant::now();
    let rebuilt = ConnectivityEngine::new(&sch);
    let full_again = t.elapsed();
    assert_eq!(engine.connectivity(), rebuilt.connectivity());

    println!("{sheets} sheets, {objects} objects, {nets} nets");
    println!("full build          {full:?}");
    println!("full build (again)  {full_again:?}");
    println!(
        "one-wire update     {incremental:?}  ({} sheet rebuilt, {} reused)",
        stats.sheets_rebuilt, stats.sheets_reused
    );
    println!(
        "speed-up            {:.1}x",
        full_again.as_secs_f64() / incremental.as_secs_f64().max(1e-9)
    );
}
