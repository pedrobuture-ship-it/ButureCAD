//! Exact collision predicates against convex obstacles.

use buturecad_geometry::Point;
use std::cmp::Ordering;

pub use buturecad_geometry::convex_hull;

/// `num/den` with `den > 0`.
#[derive(Clone, Copy)]
struct Frac(i128, i128);

impl Frac {
    fn cmp(self, o: Frac) -> Ordering {
        (self.0 * o.1).cmp(&(o.0 * self.1))
    }
}

fn cross(a: Point, b: Point, c: Point) -> i128 {
    i128::from(b.x - a.x) * i128::from(c.y - a.y) - i128::from(b.y - a.y) * i128::from(c.x - a.x)
}

/// Is `p` strictly inside the convex CCW polygon `hull` (the boundary does not count)?
pub fn point_inside(hull: &[Point], p: Point) -> bool {
    hull.len() >= 3 && (0..hull.len()).all(|i| cross(hull[i], hull[(i + 1) % hull.len()], p) > 0)
}

/// Does the segment `p → q` pass through the **interior** of the convex CCW polygon `hull`?
/// Touching the boundary (grazing a corner, running along an edge) is not a collision.
///
/// Exact Cyrus–Beck clipping: every edge gives a strict linear inequality in the segment
/// parameter `t`; the open set of solutions inside `[0, 1]` is empty or not, decided with
/// rational comparisons in `i128` (coordinates are bounded by ±2⁴⁰ nm, so products fit).
pub fn segment_crosses_interior(hull: &[Point], p: Point, q: Point) -> bool {
    if hull.len() < 3 {
        return false;
    }
    if p == q {
        return point_inside(hull, p);
    }
    let d = (i128::from(q.x - p.x), i128::from(q.y - p.y));
    // t > lo (strict, or ≥ 0 at the start), t < hi (strict, or ≤ 1 at the end).
    let mut lo = Frac(0, 1);
    let mut hi = Frac(1, 1);
    for i in 0..hull.len() {
        let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
        let e = (i128::from(b.x - a.x), i128::from(b.y - a.y));
        // f(t) = c + t·k > 0 with c = e × (p − a), k = e × d.
        let c = e.0 * i128::from(p.y - a.y) - e.1 * i128::from(p.x - a.x);
        let k = e.0 * d.1 - e.1 * d.0;
        if k == 0 {
            if c <= 0 {
                return false;
            }
        } else if k > 0 {
            // t > −c/k
            let f = Frac(-c, k);
            if f.cmp(lo) == Ordering::Greater {
                lo = f;
            }
        } else {
            // t < c/(−k)
            let f = Frac(c, -k);
            if f.cmp(hi) == Ordering::Less {
                hi = f;
            }
        }
    }
    lo.cmp(hi) == Ordering::Less
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sq(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Point> {
        convex_hull(&[
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    }

    #[test]
    fn hull_is_ccw_and_drops_collinear_points() {
        let h = convex_hull(&[
            Point::new(0, 0),
            Point::new(5, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
            Point::new(5, 5),
        ]);
        assert_eq!(h.len(), 4);
        assert!(point_inside(&h, Point::new(5, 5)));
    }

    #[test]
    fn crossing_touching_and_grazing_are_told_apart() {
        let s = sq(0, 0, 10, 10);
        let p = Point::new;
        assert!(segment_crosses_interior(&s, p(-5, 5), p(15, 5)), "through");
        assert!(
            segment_crosses_interior(&s, p(-5, -5), p(15, 15)),
            "through two corners"
        );
        assert!(
            segment_crosses_interior(&s, p(5, 5), p(20, 5)),
            "starts inside"
        );
        assert!(
            !segment_crosses_interior(&s, p(-5, 0), p(15, 0)),
            "runs along the bottom edge"
        );
        assert!(
            !segment_crosses_interior(&s, p(-5, 10), p(15, 10)),
            "runs along the top edge"
        );
        assert!(
            !segment_crosses_interior(&s, p(10, 10), p(20, 20)),
            "leaves from a corner"
        );
        assert!(
            segment_crosses_interior(&s, p(-5, 15), p(15, -5)),
            "the other diagonal"
        );
        assert!(
            !segment_crosses_interior(&s, p(5, 15), p(15, 5)),
            "grazes the corner (10,10)"
        );
        assert!(
            !segment_crosses_interior(&s, p(0, 0), p(10, 0)),
            "edge itself"
        );
        assert!(
            !segment_crosses_interior(&s, p(-10, 5), p(-1, 5)),
            "stops short"
        );
        assert!(
            !segment_crosses_interior(&s, p(11, -1), p(11, 20)),
            "beside"
        );
    }
}
