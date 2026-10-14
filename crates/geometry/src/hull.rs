//! Convex hull.

use crate::point::Point;

/// Convex hull, counter-clockwise, without collinear points (Andrew's monotone chain; exact
/// integer predicates). Fewer than three distinct points are returned as they are.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut p: Vec<Point> = points.to_vec();
    p.sort();
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Point, a: Point, b: Point| -> i128 {
        i128::from(a.x - o.x) * i128::from(b.y - o.y)
            - i128::from(a.y - o.y) * i128::from(b.x - o.x)
    };
    let mut lower: Vec<Point> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<Point> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(h[0], Point::new(0, 0));
        assert!(h
            .windows(3)
            .all(|w| { (w[1] - w[0]).cross(w[2] - w[1]) > 0 }));
    }
}
