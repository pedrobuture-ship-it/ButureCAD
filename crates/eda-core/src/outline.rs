//! The board outline: the straight pieces of `Edge.Cuts`.

use crate::model::PcbDoc;
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, FpShape};

fn arc_points(arc: &buturecad_geometry::Arc) -> Vec<Point> {
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
