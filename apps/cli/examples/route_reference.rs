//! VALIDATION HARNESS (roadmap stage 47) — not a product feature.
//!
//! ButureCAD has no headless routing command: routing is interactive in the desktop app. To
//! validate DRC, 3-D and manufacturing on a populated board without a human at the mouse, this
//! tool drives the same router crate and the same Command Engine the GUI uses: for every
//! unrouted connection of the ratsnest it asks the router for a walk-around path on F.Cu and
//! commits it as a real Track; a few vias tie the front GND copper to the B.Cu pour. It never
//! touches a file other than the project it is given, and it reports what it could not route.
//!
//! `cargo run -p buturecad-cli --example route_reference -- <project-dir>`

use buturecad_eda_core::commands::PcbEditCommand;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track, Via};
use buturecad_eda_core::ratsnest::RatsnestEngine;
use buturecad_eda_core::routing::AngleMode;
use buturecad_eda_core::CommandEngine;
use buturecad_geometry::{Point, Segment};
use buturecad_io::ProjectSession;
use buturecad_pcb::layer;
use buturecad_router::{route_leg, ObstacleSet, RouteParams};
use std::collections::BTreeSet;

fn main() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage: route_reference <project-dir>")?;
    let opened = ProjectSession::open(&dir).map_err(|e| e.to_string())?;
    let session = opened.session;
    let mut engine = CommandEngine::new(opened.project);
    let mut failed: BTreeSet<(u64, i64, i64)> = BTreeSet::new();
    let (mut routed, mut gave_up) = (0usize, Vec::new());
    let mut via_routes = 0usize;
    let started = std::time::Instant::now();
    let plane_vias = plane_vias(&mut engine, &["GND", "+3V3"])?;
    let mut rat = RatsnestEngine::new(engine.project().pcb());
    loop {
        // Shortest remaining connection first.
        let next = rat
            .lines()
            .filter(|l| !failed.contains(&(l.net.0, l.a.x + l.b.x, l.a.y + l.b.y)))
            // Signals before supplies (supplies are wide and would wall the pins in), then shortest.
            .min_by(|a, b| {
                let power = |l: &buturecad_eda_core::ratsnest::RatLine| {
                    engine
                        .project()
                        .pcb()
                        .rules
                        .class_of(Some(l.net))
                        .track_width
                        > 250_000
                };
                power(a)
                    .cmp(&power(b))
                    .then(a.length().total_cmp(&b.length()))
            })
            .copied();
        let Some(line) = next else { break };
        let (class, net_name) = {
            let doc = engine.project().pcb();
            (
                doc.rules.class_of(Some(line.net)).clone(),
                doc.nets
                    .get(&line.net)
                    .map_or("?", |n| n.name.as_str())
                    .to_owned(),
            )
        };
        let mut done = false;
        // Prefer the class width; fall back to a thinner track where it does not fit.
        for width in [class.track_width, 250_000, 200_000, 150_000] {
            if width > class.track_width {
                continue;
            }
            let set = ObstacleSet::collect(
                engine.project().pcb(),
                RouteParams::new(Some(line.net), layer::F_CU, width),
            );
            let Ok(path) = route_leg(&set, line.a, line.b, AngleMode::Deg45, false) else {
                continue;
            };
            drop(set);
            let segments: Vec<_> = path
                .windows(2)
                .filter(|w| w[0] != w[1])
                .map(|w| Segment::new(w[0], w[1]))
                .collect();
            if segments.is_empty() {
                continue;
            }
            let mut changes = Vec::new();
            for segment in segments {
                changes.push(PcbChange::create(
                    engine.new_id(),
                    PcbObject::Track(Track {
                        layer: layer::F_CU,
                        segment,
                        width,
                        net: Some(line.net),
                    }),
                ));
            }
            engine
                .execute(PcbEditCommand::new("Route (validation harness)", changes))
                .map_err(|e| e.to_string())?;
            routed += 1;
            done = true;
            break;
        }
        if !done {
            done = route_with_vias(&mut engine, &line, &class);
            if done {
                via_routes += 1;
                routed += 1;
            }
        }
        if done {
            rat.refresh(engine.project().pcb());
        } else {
            gave_up.push(format!(
                "{net_name}: ({:.2},{:.2}) -> ({:.2},{:.2}) mm",
                line.a.x as f64 / 1e6,
                line.a.y as f64 / 1e6,
                line.b.x as f64 / 1e6,
                line.b.y as f64 / 1e6
            ));
            failed.insert((line.net.0, line.a.x + line.b.x, line.a.y + line.b.y));
        }
    }
    // Stitching vias from the front GND copper to the B.Cu pour, around U1's exposed pad.
    let doc = engine.project().pcb();
    let gnd = doc
        .nets
        .iter()
        .find(|(_, n)| n.name == "GND")
        .map(|(id, _)| *id);
    let u1 = doc
        .placed
        .values()
        .find(|f| f.reference == "U1")
        .map(|f| f.placement.position);
    let mut vias = 0;
    if let (Some(gnd), Some(c)) = (gnd, u1) {
        let doc = engine.project().pcb();
        let params = |l| RouteParams::new(Some(gnd), l, 600_000);
        let (front, back) = (
            ObstacleSet::collect(doc, params(layer::F_CU)),
            ObstacleSet::collect(doc, params(layer::B_CU)),
        );
        let mut spots: Vec<Point> = Vec::new();
        'search: for ring in 1..=8i64 {
            for k in 0..ring * 8 {
                let (dx, dy) = ring_point(ring, k);
                let q = Point::new(c.x + dx * 500_000, c.y + dy * 500_000);
                let free = front.containing(q).is_none() && back.containing(q).is_none();
                let apart = spots.iter().all(|o| o.distance(q) > 1_400_000.0);
                if free && apart {
                    spots.push(q);
                    if spots.len() == 4 {
                        break 'search;
                    }
                }
            }
        }
        let mut changes = Vec::new();
        for q in spots {
            changes.push(PcbChange::create(
                engine.new_id(),
                PcbObject::Via(Via {
                    position: q,
                    diameter: 600_000,
                    drill: 300_000,
                    net: Some(gnd),
                    kind: Default::default(),
                }),
            ));
            vias += 1;
        }
        engine
            .execute(PcbEditCommand::new(
                "Stitching vias (validation harness)",
                changes,
            ))
            .map_err(|e| e.to_string())?;
    }
    session.save(engine.project()).map_err(|e| e.to_string())?;
    println!(
        "routed {routed} connection(s) ({via_routes} with a layer change), {vias} stitching and \
         {plane_vias} plane via(s); could not route {} ({:.1} s)",
        gave_up.len(),
        started.elapsed().as_secs_f64()
    );
    for g in gave_up {
        println!("  unrouted: {g}");
    }
    Ok(())
}

/// Routes `line` on F.Cu up to an escape via next to each pad, across B.Cu between the vias.
fn route_with_vias(
    engine: &mut CommandEngine,
    line: &buturecad_eda_core::ratsnest::RatLine,
    class: &buturecad_pcb::NetClass,
) -> bool {
    let width = class.track_width.min(250_000);
    let (dia, drill) = (500_000, 250_000); // the board's minimum via
    let net = Some(line.net);
    let doc = engine.project().pcb();
    let front = ObstacleSet::collect(doc, RouteParams::new(net, layer::F_CU, width));
    let front_via = ObstacleSet::collect(doc, RouteParams::new(net, layer::F_CU, dia));
    let back = ObstacleSet::collect(doc, RouteParams::new(net, layer::B_CU, width));
    let back_via = ObstacleSet::collect(doc, RouteParams::new(net, layer::B_CU, dia));
    let escape = |p: Point, toward: Point| -> Option<(Vec<Point>, Point)> {
        // Candidate via spots around the pad, nearest first, preferring the way to the other end.
        let mut cands = Vec::new();
        for d in (3..=20).map(|k| k * 200_000i64) {
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let q = Point::new(p.x + dx * d, p.y + dy * d);
                let score = q.distance(toward);
                cands.push((d, score as i64, q));
            }
        }
        cands.sort_by_key(|c| (c.0, c.1));
        for (_, _, q) in cands {
            if front_via.containing(q).is_some() || back_via.containing(q).is_some() {
                continue;
            }
            if let Ok(path) = route_leg(&front, p, q, AngleMode::Deg45, false) {
                return Some((path, q));
            }
        }
        None
    };
    let (Some((pa, qa)), Some((pb, qb))) = (escape(line.a, line.b), escape(line.b, line.a)) else {
        return false;
    };
    let Ok(mid) = route_leg(&back, qa, qb, AngleMode::Deg45, false) else {
        return false;
    };
    let mut parts: Vec<(buturecad_pcb::LayerId, Vec<Point>)> =
        vec![(layer::F_CU, pa), (layer::B_CU, mid), (layer::F_CU, pb)];
    parts.retain(|(_, p)| p.len() > 1);
    let mut changes = Vec::new();
    for (l, path) in parts {
        for w in path.windows(2).filter(|w| w[0] != w[1]) {
            changes.push(PcbChange::create(
                engine.new_id(),
                PcbObject::Track(Track {
                    layer: l,
                    segment: Segment::new(w[0], w[1]),
                    width,
                    net,
                }),
            ));
        }
    }
    for q in [qa, qb] {
        changes.push(PcbChange::create(
            engine.new_id(),
            PcbObject::Via(Via {
                position: q,
                diameter: dia,
                drill,
                net,
                kind: Default::default(),
            }),
        ));
    }
    engine
        .execute(PcbEditCommand::new(
            "Route with layer change (validation harness)",
            changes,
        ))
        .is_ok()
}

/// The `k`-th lattice point on the square ring of radius `ring`.
fn ring_point(ring: i64, k: i64) -> (i64, i64) {
    let side = 2 * ring;
    match k / side {
        0 => (-ring + k % side, -ring),
        1 => (ring, -ring + k % side),
        2 => (ring - k % side, ring),
        _ => (-ring, ring - k % side),
    }
}

/// Ties every pad of the plane nets to the inner planes with a via: in the pad when it is big
/// enough, otherwise at the end of a short F.Cu stub. Fills the zones afterwards so the
/// ratsnest sees the planes.
fn plane_vias(engine: &mut CommandEngine, names: &[&str]) -> Result<usize, String> {
    let (dia, drill) = (500_000, 250_000);
    let doc = engine.project().pcb();
    let wanted: Vec<_> = doc
        .nets
        .iter()
        .filter(|(_, n)| names.contains(&n.name.as_str()))
        .map(|(id, _)| *id)
        .collect();
    let mut pads = Vec::new();
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        for wp in f.world_pads(def) {
            if let Some(net) = f.pad_nets.get(&wp.number).filter(|n| wanted.contains(n)) {
                pads.push((*net, wp.center, wp.width.min(wp.height)));
            }
        }
    }
    let mut vias: Vec<(buturecad_eda_core::model::Id, Point)> = Vec::new();
    let mut stubs: Vec<(buturecad_eda_core::model::Id, Vec<Point>)> = Vec::new();
    for (net, c, small) in pads {
        if small >= 700_000 {
            vias.push((net, c));
            continue;
        }
        let doc = engine.project().pcb();
        let front = ObstacleSet::collect(doc, RouteParams::new(Some(net), layer::F_CU, 200_000));
        let front_via = ObstacleSet::collect(doc, RouteParams::new(Some(net), layer::F_CU, dia));
        let back_via = ObstacleSet::collect(doc, RouteParams::new(Some(net), layer::B_CU, dia));
        let mut cands = Vec::new();
        for d in (2..=20).map(|k| k * 200_000i64) {
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                cands.push((d, Point::new(c.x + dx * d, c.y + dy * d)));
            }
        }
        for (_, q) in cands {
            let clear_of_new = vias
                .iter()
                .all(|(n, v)| (*n == net && v.distance(q) < 1.0) || v.distance(q) > 1_000_000.0)
                && stubs.iter().all(|(n, path)| {
                    *n == net
                        || path
                            .windows(2)
                            .all(|w| Segment::new(w[0], w[1]).distance_to_point(q) > 600_000.0)
                });
            if !clear_of_new
                || front_via.containing(q).is_some()
                || back_via.containing(q).is_some()
            {
                continue;
            }
            if let Ok(path) = route_leg(&front, c, q, AngleMode::Deg45, false) {
                if path.windows(2).all(|w| w[0] != w[1]) {
                    vias.push((net, q));
                    stubs.push((net, path));
                    break;
                }
            }
        }
    }
    let n = vias.len();
    let mut changes = Vec::new();
    for (net, path) in stubs {
        for w in path.windows(2) {
            changes.push(PcbChange::create(
                engine.new_id(),
                PcbObject::Track(Track {
                    layer: layer::F_CU,
                    segment: Segment::new(w[0], w[1]),
                    width: 200_000,
                    net: Some(net),
                }),
            ));
        }
    }
    for (net, at) in vias {
        changes.push(PcbChange::create(
            engine.new_id(),
            PcbObject::Via(Via {
                position: at,
                diameter: dia,
                drill,
                net: Some(net),
                kind: Default::default(),
            }),
        ));
    }
    engine
        .execute(PcbEditCommand::new(
            "Plane vias (validation harness)",
            changes,
        ))
        .map_err(|e| e.to_string())?;
    let fill = buturecad_eda_core::zonefill::fill_changes(engine.project().pcb(), None, true);
    if !fill.is_empty() {
        engine
            .execute(PcbEditCommand::new("Fill zones (validation harness)", fill))
            .map_err(|e| e.to_string())?;
    }
    Ok(n)
}
