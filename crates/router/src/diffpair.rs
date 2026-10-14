//! Differential pair geometry: two parallel tracks derived from one centre path.
//!
//! The pair is routed as a single thick track (the centre path) and the two legs are its parallel
//! offsets at ±(gap + width)/2, joined with miters. At a corner the outer leg is longer than the
//! inner one — that difference is the *skew* reported live (length tuning to remove it is roadmap
//! stage 24).

use buturecad_geometry::{Coord, Point};

fn dedup(path: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(path.len());
    for p in path {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    out
}

fn unit(a: Point, b: Point) -> (f64, f64) {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let l = dx.hypot(dy);
    (dx / l, dy / l)
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x.round() as i64, y.round() as i64)
}

/// The parallel of `path` at distance `d` (positive = to the left of the travel direction).
/// Corners are mitered; a turn sharper than 120° is bevelled instead of spiking.
pub fn offset_polyline(path: &[Point], d: f64) -> Vec<Point> {
    let p = dedup(path);
    if p.len() < 2 {
        return p;
    }
    let normals: Vec<(f64, f64)> = p
        .windows(2)
        .map(|w| {
            let (ux, uy) = unit(w[0], w[1]);
            (-uy, ux)
        })
        .collect();
    let mut out = Vec::with_capacity(p.len());
    out.push(pt(
        p[0].x as f64 + d * normals[0].0,
        p[0].y as f64 + d * normals[0].1,
    ));
    for i in 1..p.len() - 1 {
        let (a, b) = (normals[i - 1], normals[i]);
        let dot = a.0 * b.0 + a.1 * b.1;
        let (px, py) = (p[i].x as f64, p[i].y as f64);
        if dot < -0.5 {
            out.push(pt(px + d * a.0, py + d * a.1));
            out.push(pt(px + d * b.0, py + d * b.1));
        } else {
            let k = d / (1.0 + dot);
            out.push(pt(px + k * (a.0 + b.0), py + k * (a.1 + b.1)));
        }
    }
    let n = normals[normals.len() - 1];
    let last = p[p.len() - 1];
    out.push(pt(last.x as f64 + d * n.0, last.y as f64 + d * n.1));
    dedup(&out)
}

/// The two legs of a pair around `path`: `(left, right)` of the travel direction, each at
/// `(gap + width) / 2` from it.
pub fn coupled(path: &[Point], width: Coord, gap: Coord) -> (Vec<Point>, Vec<Point>) {
    let half_pitch = (gap + width) as f64 / 2.0;
    (
        offset_polyline(path, half_pitch),
        offset_polyline(path, -half_pitch),
    )
}

/// Total length of a polyline in nm.
pub fn length(path: &[Point]) -> f64 {
    path.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Smallest edge-to-edge distance between two polylines of tracks of width `width`, in nm.
pub fn min_gap(a: &[Point], b: &[Point], width: Coord) -> Option<f64> {
    use buturecad_geometry::Segment;
    let segs = |p: &[Point]| -> Vec<Segment> {
        if p.len() == 1 {
            vec![Segment::new(p[0], p[0])]
        } else {
            p.windows(2).map(|w| Segment::new(w[0], w[1])).collect()
        }
    };
    let (sa, sb) = (segs(a), segs(b));
    if sa.is_empty() || sb.is_empty() {
        return None;
    }
    let mut best = f64::INFINITY;
    for x in &sa {
        for y in &sb {
            best = best.min(x.distance_to_segment(y));
        }
    }
    Some(best - width as f64)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const MM: i64 = 1_000_000;
    fn p(x: i64, y: i64) -> Point {
        Point::new(x * MM, y * MM)
    }

    #[test]
    fn a_straight_pair_is_parallel_with_the_requested_gap() {
        let (l, r) = coupled(&[p(0, 0), p(10, 0)], 200_000, 300_000);
        assert_eq!(
            l,
            vec![Point::new(0, 250_000), Point::new(10 * MM, 250_000)]
        );
        assert_eq!(
            r,
            vec![Point::new(0, -250_000), Point::new(10 * MM, -250_000)]
        );
        assert_eq!(min_gap(&l, &r, 200_000), Some(300_000.0));
        assert_eq!(length(&l), length(&r));
    }

    #[test]
    fn a_corner_keeps_the_gap_and_makes_the_outer_leg_longer() {
        // East then north: left turn, so the left leg is the inner one.
        let path = [p(0, 0), p(10, 0), p(10, 10)];
        let (l, r) = coupled(&path, 200_000, 300_000);
        assert_eq!(l.len(), 3);
        assert!(length(&l) < length(&r));
        // Difference of a 90° miter: 2 * half_pitch * 2 = 1.0 mm.
        let diff = length(&r) - length(&l);
        assert!((diff - 1_000_000.0).abs() < 4.0, "{diff}");
        // The gap along the whole pair stays at the target (to rounding).
        let g = min_gap(&l, &r, 200_000).unwrap();
        assert!((g - 300_000.0).abs() < 4.0, "{g}");
    }

    #[test]
    fn a_45_degree_corner_is_mitered_too() {
        let path = [p(0, 0), p(10, 0), p(15, 5)];
        let (l, r) = coupled(&path, 200_000, 200_000);
        assert_eq!(l.len(), 3);
        let g = min_gap(&l, &r, 200_000).unwrap();
        assert!((g - 200_000.0).abs() < 4.0, "{g}");
    }

    #[test]
    fn a_hairpin_is_bevelled_not_spiked() {
        let path = [p(0, 0), p(10, 0), p(0, 1)];
        let (l, _) = coupled(&path, 200_000, 200_000);
        assert!(l.len() >= 4, "{l:?}");
        assert!(l.iter().all(|q| q.x < 12 * MM));
    }
}
