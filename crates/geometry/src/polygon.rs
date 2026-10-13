use crate::bbox::{Bounded, BoundingBox};
use crate::error::GeometryError;
use crate::line::{Segment, SegmentIntersection};
use crate::point::{Angle, Point, Vector};
use crate::predicates::{round_f64, PointLocation};
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// Winding direction of a polygon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Ccw,
    Cw,
}

/// A closed polygon without holes, stored as its vertex ring (the closing edge is implicit).
///
/// Construction guarantees: at least 3 vertices, no repeated consecutive vertices, non-zero
/// area. It does **not** guarantee simplicity; call [`Polygon::is_simple`]. Containment
/// ([`Polygon::locate`]) is only meaningful for simple polygons. Holes and multi-polygons are
/// not supported yet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Polygon {
    vertices: Vec<Point>,
}

impl Polygon {
    pub fn new(vertices: Vec<Point>) -> Result<Self, GeometryError> {
        let n = vertices.len();
        if n < 3 || (0..n).any(|i| vertices[i] == vertices[(i + 1) % n]) {
            return Err(GeometryError::DegeneratePolygon);
        }
        let poly = Self { vertices };
        if poly.signed_area2() == 0 {
            return Err(GeometryError::DegeneratePolygon);
        }
        Ok(poly)
    }

    /// For callers that already guarantee the invariants (e.g. [`crate::Rectangle`]).
    pub(crate) fn from_valid_vertices(vertices: Vec<Point>) -> Self {
        Self { vertices }
    }

    pub fn vertices(&self) -> &[Point] {
        &self.vertices
    }

    pub fn edges(&self) -> impl Iterator<Item = Segment> + '_ {
        let n = self.vertices.len();
        (0..n).map(move |i| Segment::new(self.vertices[i], self.vertices[(i + 1) % n]))
    }

    /// Twice the signed area (positive for counter-clockwise), exact, in nm².
    pub fn signed_area2(&self) -> i128 {
        self.edges()
            .map(|e| e.a.x as i128 * e.b.y as i128 - e.b.x as i128 * e.a.y as i128)
            .sum()
    }

    pub fn area(&self) -> f64 {
        self.signed_area2().unsigned_abs() as f64 / 2.0
    }

    pub fn orientation(&self) -> Orientation {
        if self.signed_area2() > 0 {
            Orientation::Ccw
        } else {
            Orientation::Cw
        }
    }

    /// The same polygon with counter-clockwise winding.
    pub fn into_ccw(mut self) -> Polygon {
        if self.orientation() == Orientation::Cw {
            self.vertices.reverse();
        }
        self
    }

    /// Exact: no two edges cross, touch (except adjacent edges at their shared vertex) or overlap.
    /// O(n²).
    pub fn is_simple(&self) -> bool {
        let edges: Vec<Segment> = self.edges().collect();
        let n = edges.len();
        for i in 0..n {
            for j in i + 1..n {
                let adjacent = j == i + 1 || (i == 0 && j == n - 1);
                let hit = edges[i].intersection(&edges[j]);
                if adjacent {
                    let shared = if j == i + 1 { edges[i].b } else { edges[i].a };
                    if hit != SegmentIntersection::Point(shared) {
                        return false;
                    }
                } else if hit != SegmentIntersection::None {
                    return false;
                }
            }
        }
        true
    }

    pub fn is_convex(&self) -> bool {
        let n = self.vertices.len();
        let mut sign = 0i128;
        for i in 0..n {
            let (a, b, c) = (
                self.vertices[i],
                self.vertices[(i + 1) % n],
                self.vertices[(i + 2) % n],
            );
            let cr = (b - a).cross(c - b).signum();
            if cr != 0 {
                if sign != 0 && sign != cr {
                    return false;
                }
                sign = cr;
            }
        }
        true
    }

    /// Exact point location (boundary inclusive). Valid for simple polygons.
    pub fn locate(&self, p: Point) -> PointLocation {
        if !self.bounding_box().contains_point(p) {
            return PointLocation::Outside;
        }
        let mut inside = false;
        for e in self.edges() {
            if e.contains_point(p) {
                return PointLocation::Boundary;
            }
            // Half-open crossing rule for a ray towards +x.
            if (e.a.y > p.y) != (e.b.y > p.y) {
                let side = (e.b - e.a).cross(p - e.a);
                if (e.b.y > e.a.y && side > 0) || (e.b.y < e.a.y && side < 0) {
                    inside = !inside;
                }
            }
        }
        if inside {
            PointLocation::Inside
        } else {
            PointLocation::Outside
        }
    }

    pub fn contains_point(&self, p: Point) -> bool {
        self.locate(p).is_contained()
    }

    /// Exact: do the two (simple) polygons overlap or touch?
    pub fn collides(&self, o: &Polygon) -> bool {
        if !self.bounding_box().intersects(&o.bounding_box()) {
            return false;
        }
        let oe: Vec<Segment> = o.edges().collect();
        if self.edges().any(|a| oe.iter().any(|b| a.intersects(b))) {
            return true;
        }
        // No edge contact: either disjoint or one contains the other.
        o.locate(self.vertices[0]).is_contained() || self.locate(o.vertices[0]).is_contained()
    }

    /// Exact: does the polygon touch the segment?
    pub fn collides_segment(&self, s: &Segment) -> bool {
        self.edges().any(|e| e.intersects(s)) || self.contains_point(s.a)
    }

    /// Distance from `p` to the polygon region (0 inside or on the boundary).
    pub fn distance_to_point(&self, p: Point) -> f64 {
        if self.contains_point(p) {
            return 0.0;
        }
        self.edges()
            .map(|e| e.distance_to_point(p))
            .fold(f64::INFINITY, f64::min)
    }

    /// Distance between the polygon regions (0 when they collide). O(n·m).
    pub fn distance_to_polygon(&self, o: &Polygon) -> f64 {
        if self.collides(o) {
            return 0.0;
        }
        let oe: Vec<Segment> = o.edges().collect();
        self.edges()
            .flat_map(|a| oe.iter().map(move |b| a.distance_to_segment(b)))
            .fold(f64::INFINITY, f64::min)
    }

    /// Closest point on the polygon boundary to `p`, rounded to the nearest nm.
    pub fn nearest_boundary_point(&self, p: Point) -> Point {
        let mut best = self.vertices[0];
        let mut best_d = i128::MAX;
        for e in self.edges() {
            let q = e.nearest_point(p);
            let d = q.distance_sq(p);
            if d < best_d {
                best_d = d;
                best = q;
            }
        }
        best
    }

    pub fn translate(&self, v: Vector) -> Polygon {
        Polygon {
            vertices: self.vertices.iter().map(|p| *p + v).collect(),
        }
    }

    /// Exact rotation by `n` quarter turns counter-clockwise.
    pub fn rotate_quarter_turns(&self, center: Point, n: u8) -> Polygon {
        Polygon {
            vertices: self
                .vertices
                .iter()
                .map(|p| p.rotate_quarter_turns(center, n))
                .collect(),
        }
    }

    /// Rotation around `center`; vertices are rounded to the nearest nm (exact for multiples of 90°).
    pub fn rotate_around(&self, center: Point, angle: Angle) -> Polygon {
        Polygon {
            vertices: self
                .vertices
                .iter()
                .map(|p| p.rotate_around(center, angle))
                .collect(),
        }
    }

    /// Mitered offset: positive `d` grows the polygon, negative shrinks it.
    ///
    /// **Incomplete.** Uses mitered joins computed in `f64` and rounds to the nearest nm. It does
    /// not resolve topology changes: if edges collapse, vanish, or the result self-intersects or
    /// inverts, it returns [`GeometryError::OffsetTopology`] rather than a wrong polygon. Corners
    /// sharper than ~150° of turn return [`GeometryError::MiterTooSharp`]. Round/bevel joins and
    /// splitting into several polygons are not implemented.
    pub fn offset(&self, d: Coord) -> Result<Polygon, GeometryError> {
        if d == 0 {
            return Ok(self.clone());
        }
        let ccw = self.orientation() == Orientation::Ccw;
        let n = self.vertices.len();
        let unit_normal = |a: Point, b: Point| {
            let e = b - a;
            let len = e.length();
            let (nx, ny) = (e.y as f64 / len, -(e.x as f64) / len);
            if ccw {
                (nx, ny)
            } else {
                (-nx, -ny)
            }
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (prev, v, next) = (
                self.vertices[(i + n - 1) % n],
                self.vertices[i],
                self.vertices[(i + 1) % n],
            );
            let (n1, n2) = (unit_normal(prev, v), unit_normal(v, next));
            let denom = 1.0 + n1.0 * n2.0 + n1.1 * n2.1;
            if denom < 0.05 {
                return Err(GeometryError::MiterTooSharp);
            }
            let k = d as f64 / denom;
            out.push(Point::new(
                v.x.saturating_add(round_f64((n1.0 + n2.0) * k)),
                v.y.saturating_add(round_f64((n1.1 + n2.1) * k)),
            ));
        }
        let result = Polygon::new(out).map_err(|_| GeometryError::OffsetTopology)?;
        let same_winding = (result.signed_area2() > 0) == ccw;
        // Every offset edge must keep the direction of its source edge; an inverted (collapsed
        // then re-expanded) result flips some of them.
        let directions_kept = self
            .edges()
            .zip(result.edges())
            .all(|(a, b)| a.direction().dot(b.direction()) > 0);
        let area_ok = if d > 0 {
            result.signed_area2().abs() > self.signed_area2().abs()
        } else {
            result.signed_area2().abs() < self.signed_area2().abs()
        };
        if !same_winding || !directions_kept || !area_ok || !result.is_simple() {
            return Err(GeometryError::OffsetTopology);
        }
        Ok(result)
    }
}

impl Bounded for Polygon {
    fn bounding_box(&self) -> BoundingBox {
        BoundingBox::from_points(self.vertices.iter().copied())
            .unwrap_or(BoundingBox::from_point(Point::ORIGIN))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn poly(pts: &[(i64, i64)]) -> Polygon {
        Polygon::new(pts.iter().map(|&(x, y)| Point::new(x, y)).collect()).unwrap()
    }
    fn p(x: i64, y: i64) -> Point {
        Point::new(x, y)
    }
    fn square(x: i64, y: i64, s: i64) -> Polygon {
        poly(&[(x, y), (x + s, y), (x + s, y + s), (x, y + s)])
    }
    /// Concave "L" shape.
    fn ell() -> Polygon {
        poly(&[(0, 0), (10, 0), (10, 4), (4, 4), (4, 10), (0, 10)])
    }

    #[test]
    fn rejects_degenerate() {
        assert!(Polygon::new(vec![p(0, 0), p(1, 1)]).is_err());
        assert!(Polygon::new(vec![p(0, 0), p(5, 5), p(10, 10)]).is_err());
        assert!(Polygon::new(vec![p(0, 0), p(0, 0), p(5, 5), p(0, 5)]).is_err());
    }

    #[test]
    fn area_and_orientation() {
        assert_eq!(square(0, 0, 10).signed_area2(), 200);
        assert_eq!(square(0, 0, 10).orientation(), Orientation::Ccw);
        let cw = poly(&[(0, 0), (0, 10), (10, 10), (10, 0)]);
        assert_eq!(cw.orientation(), Orientation::Cw);
        assert_eq!(cw.into_ccw().orientation(), Orientation::Ccw);
        assert_eq!(ell().area(), 64.0);
    }

    #[test]
    fn simplicity_and_convexity() {
        assert!(ell().is_simple() && !ell().is_convex());
        assert!(square(0, 0, 5).is_simple() && square(0, 0, 5).is_convex());
        let bowtie = poly(&[(0, 0), (10, 10), (10, 0), (0, 6)]);
        assert!(!bowtie.is_simple());
    }

    #[test]
    fn point_location_concave() {
        let l = ell();
        assert_eq!(l.locate(p(2, 8)), PointLocation::Inside);
        assert_eq!(l.locate(p(8, 2)), PointLocation::Inside);
        assert_eq!(l.locate(p(8, 8)), PointLocation::Outside);
        assert_eq!(l.locate(p(4, 7)), PointLocation::Boundary);
        assert_eq!(l.locate(p(10, 4)), PointLocation::Boundary);
        // Ray passes exactly through vertices.
        assert_eq!(l.locate(p(-5, 4)), PointLocation::Outside);
        assert_eq!(l.locate(p(5, 4)), PointLocation::Boundary);
        assert_eq!(l.locate(p(6, 4)), PointLocation::Boundary);
    }

    #[test]
    fn polygon_collision() {
        assert!(square(0, 0, 10).collides(&square(10, 10, 5))); // corner touch
        assert!(square(0, 0, 10).collides(&square(2, 2, 3))); // containment
        assert!(square(2, 2, 3).collides(&square(0, 0, 10)));
        assert!(!square(0, 0, 10).collides(&square(11, 0, 5)));
        // The concave notch of the L does not touch a square placed inside it.
        assert!(!ell().collides(&square(6, 6, 3)));
        assert!(ell().collides(&square(3, 3, 3)));
    }

    #[test]
    fn distances() {
        assert_eq!(square(0, 0, 10).distance_to_point(p(5, 5)), 0.0);
        assert_eq!(square(0, 0, 10).distance_to_point(p(13, 14)), 5.0);
        assert_eq!(square(0, 0, 10).distance_to_polygon(&square(14, 0, 2)), 4.0);
        assert_eq!(square(0, 0, 10).nearest_boundary_point(p(5, 20)), p(5, 10));
    }

    #[test]
    fn transforms() {
        let s = square(0, 0, 10);
        assert_eq!(s.translate(Vector::new(5, 5)).bounding_box().min, p(5, 5));
        let r = s.rotate_quarter_turns(Point::ORIGIN, 1);
        assert_eq!(r.bounding_box().min, p(-10, 0));
        assert_eq!(r.signed_area2(), s.signed_area2());
    }

    #[test]
    fn offset_square_grows_and_shrinks() {
        let g = square(0, 0, 10).offset(2).unwrap();
        assert_eq!(
            (g.bounding_box().min, g.bounding_box().max),
            (p(-2, -2), p(12, 12))
        );
        let s = square(0, 0, 10).offset(-2).unwrap();
        assert_eq!(
            (s.bounding_box().min, s.bounding_box().max),
            (p(2, 2), p(8, 8))
        );
        // Clockwise input offsets the same way.
        let cw = poly(&[(0, 0), (0, 10), (10, 10), (10, 0)])
            .offset(2)
            .unwrap();
        assert_eq!(cw.bounding_box().min, p(-2, -2));
    }

    #[test]
    fn offset_concave_and_failures() {
        let g = ell().offset(1).unwrap();
        assert!(g.collides(&ell()) && g.area() > ell().area());
        assert_eq!(
            square(0, 0, 10).offset(-6),
            Err(GeometryError::OffsetTopology)
        );
        // Shrinking the L past its arm width must not return garbage.
        assert!(ell().offset(-3).is_err());
        let spike = poly(&[(0, 0), (100, 1), (0, 2)]);
        assert_eq!(spike.offset(1), Err(GeometryError::MiterTooSharp));
    }
}
