//! The board outline: the straight pieces of `Edge.Cuts`.

use crate::model::PcbDoc;
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, FpShape};

pub fn arc_points(arc: &buturecad_geometry::Arc) -> Vec<Point> {
    let (c, s, e) = (arc.center, arc.start, arc.end);
    let r = arc.radius();
    let a0 = ((s.y - c.y) as f64).atan2((s.x - c.x) as f64);
    let mut a1 = ((e.y - c.y) as f64).atan2((e.x - c.x) as f64);
    if arc.ccw {
        while a1 <= a0 {
            a1 += std::f64::consts::TAU;
        }
    } else {
        while a1 >= a0 {
            a1 -= std::f64::consts::TAU;
        }
    }
    let n = (((a1 - a0).abs() / std::f64::consts::TAU) * 48.0)
        .ceil()
        .max(4.0) as usize;
    let mut pts = vec![s];
    for i in 1..n {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        pts.push(Point::new(
            (c.x as f64 + r * a.cos()).round() as i64,
            (c.y as f64 + r * a.sin()).round() as i64,
        ));
    }
    pts.push(e);
    pts
}

/// The straight pieces of the board outline on `Edge.Cuts` (arcs and circles approximated by
/// polylines of at most 48 segments).
pub fn outline_segments(doc: &PcbDoc) -> Vec<Segment> {
    let mut polylines: Vec<Vec<Point>> = Vec::new();
    for g in doc
        .graphics
        .values()
        .filter(|g| g.layer == layer::EDGE_CUTS)
    {
        match &g.shape {
            FpShape::Line { segment, .. } => polylines.push(vec![segment.a, segment.b]),
            FpShape::Arc { arc, .. } => polylines.push(arc_points(arc)),
            FpShape::Polygon { points, .. } => {
                let mut p = points.clone();
                p.push(points[0]);
                polylines.push(p);
            }
            FpShape::Rect { rect, .. } => {
                let (a, b) = (rect.min(), rect.max());
                polylines.push(vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y), a]);
            }
            FpShape::Circle { circle, .. } => {
                let pts: Vec<Point> = (0..=48)
                    .map(|i| {
                        let a = i as f64 / 48.0 * std::f64::consts::TAU;
                        Point::new(
                            (circle.center.x as f64 + circle.radius as f64 * a.cos()).round()
                                as i64,
                            (circle.center.y as f64 + circle.radius as f64 * a.sin()).round()
                                as i64,
                        )
                    })
                    .collect();
                polylines.push(pts);
            }
        }
    }
    polylines
        .iter()
        .flat_map(|p| p.windows(2).map(|w| Segment::new(w[0], w[1])))
        .filter(|s| s.a != s.b)
        .collect()
}

/// The closed loops formed by the outline segments (each as a vertex ring), largest first: the
/// first is the board, the others are cut-outs. Empty when the outline is not closed (every end
/// must meet exactly one other end).
pub fn outline_loops(doc: &PcbDoc) -> Vec<Vec<Point>> {
    let segs = outline_segments(doc);
    let mut by_end: std::collections::BTreeMap<Point, Vec<usize>> = Default::default();
    for (i, s) in segs.iter().enumerate() {
        by_end.entry(s.a).or_default().push(i);
        by_end.entry(s.b).or_default().push(i);
    }
    if segs.is_empty() || by_end.values().any(|v| v.len() != 2) {
        return Vec::new();
    }
    let mut used = vec![false; segs.len()];
    let mut loops: Vec<Vec<Point>> = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        let mut ring = vec![segs[start].a];
        let mut cur = segs[start].b;
        used[start] = true;
        while cur != ring[0] {
            ring.push(cur);
            let Some(i) = by_end[&cur].iter().copied().find(|i| !used[*i]) else {
                break;
            };
            used[i] = true;
            cur = if segs[i].a == cur {
                segs[i].b
            } else {
                segs[i].a
            };
        }
        if cur == ring[0] && ring.len() >= 3 {
            loops.push(ring);
        }
    }
    let area = |r: &[Point]| -> i128 {
        r.iter()
            .zip(r.iter().cycle().skip(1))
            .map(|(a, b)| i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y))
            .sum::<i128>()
            .abs()
    };
    loops.sort_by_key(|r| std::cmp::Reverse(area(r)));
    loops
}
