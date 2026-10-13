//! Infinite lines and finite segments.

use crate::bbox::{Bounded, BoundingBox};
use crate::error::GeometryError;
use crate::point::{Angle, Point, Vector};
use crate::predicates::{orient, round_div, round_f64, PointLocation, Side};
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// An infinite line through `origin` with non-zero direction `dir`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub origin: Point,
    pub dir: Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineIntersection {
    Parallel,
    Coincident,
    /// Intersection point rounded to the nearest nm.
    Point(Point),
}

impl Line {
    pub fn through(a: Point, b: Point) -> Result<Self, GeometryError> {
        let dir = b - a;
        if dir.is_zero() {
            return Err(GeometryError::DegenerateLine);
        }
        Ok(Self { origin: a, dir })
    }

    /// Exact side test.
    pub fn side_of(&self, p: Point) -> Side {
        orient(self.origin, self.origin + self.dir, p)
    }

    pub fn distance_to_point(&self, p: Point) -> f64 {
        self.dir.cross(p - self.origin).unsigned_abs() as f64 / self.dir.length()
    }

    /// Foot of the perpendicular from `p`, rounded to the nearest nm.
    pub fn nearest_point(&self, p: Point) -> Point {
        let len_sq = self.dir.length_sq();
        let t_num = self.dir.dot(p - self.origin);
        project(self.origin, self.dir, t_num, len_sq)
    }

    pub fn intersection(&self, other: &Line) -> LineIntersection {
        let denom = self.dir.cross(other.dir);
        let diff = other.origin - self.origin;
        if denom == 0 {
            return if self.dir.cross(diff) == 0 {
                LineIntersection::Coincident
            } else {
                LineIntersection::Parallel
            };
        }
        LineIntersection::Point(project(self.origin, self.dir, diff.cross(other.dir), denom))
    }
}

/// `origin + dir * num / den`, rounded per coordinate. `den` may be negative.
fn project(origin: Point, dir: Vector, num: i128, den: i128) -> Point {
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    Point::new(
        origin
            .x
            .saturating_add(round_div(dir.x as i128 * num, den) as Coord),
        origin
            .y
            .saturating_add(round_div(dir.y as i128 * num, den) as Coord),
    )
}

/// A finite segment between two endpoints (may be degenerate: `a == b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentIntersection {
    None,
    /// Single common point (rounded to the nearest nm when it is not on the integer grid).
    Point(Point),
    /// Collinear overlap (exact, endpoints are original endpoints).
    Overlap(Segment),
}

impl Segment {
    pub const fn new(a: Point, b: Point) -> Self {
        Self { a, b }
    }

    pub fn direction(&self) -> Vector {
        self.b - self.a
    }
    pub fn length(&self) -> f64 {
        self.direction().length()
    }
    pub fn translate(&self, v: Vector) -> Segment {
        Segment::new(self.a + v, self.b + v)
    }
    pub fn rotate_around(&self, center: Point, angle: Angle) -> Segment {
        Segment::new(
            self.a.rotate_around(center, angle),
            self.b.rotate_around(center, angle),
        )
    }

    /// Exact: is `p` on the segment (endpoints included)?
    pub fn contains_point(&self, p: Point) -> bool {
        orient(self.a, self.b, p) == Side::On && self.bounding_box().contains_point(p)
    }

    pub fn locate(&self, p: Point) -> PointLocation {
        if self.contains_point(p) {
            PointLocation::Boundary
        } else {
            PointLocation::Outside
        }
    }

    /// Exact segment/segment intersection.
    pub fn intersection(&self, o: &Segment) -> SegmentIntersection {
        let (d1, d2) = (self.direction(), o.direction());
        if d1.is_zero() {
            return if o.contains_point(self.a) {
                SegmentIntersection::Point(self.a)
            } else {
                SegmentIntersection::None
            };
        }
        if d2.is_zero() {
            return if self.contains_point(o.a) {
                SegmentIntersection::Point(o.a)
            } else {
                SegmentIntersection::None
            };
        }
        let denom = d1.cross(d2);
        let diff = o.a - self.a;
        if denom == 0 {
            if d1.cross(diff) != 0 {
                return SegmentIntersection::None;
            }
            // Collinear: lexicographic order is consistent along the common line.
            let lo = self.a.min(self.b).max(o.a.min(o.b));
            let hi = self.a.max(self.b).min(o.a.max(o.b));
            return match lo.cmp(&hi) {
                std::cmp::Ordering::Greater => SegmentIntersection::None,
                std::cmp::Ordering::Equal => SegmentIntersection::Point(lo),
                std::cmp::Ordering::Less => SegmentIntersection::Overlap(Segment::new(lo, hi)),
            };
        }
        let (mut t, mut u, mut den) = (diff.cross(d2), diff.cross(d1), denom);
        if den < 0 {
            (t, u, den) = (-t, -u, -den);
        }
        if t < 0 || t > den || u < 0 || u > den {
            return SegmentIntersection::None;
        }
        SegmentIntersection::Point(project(self.a, d1, t, den))
    }

    /// Exact: do the segments share at least one point?
    pub fn intersects(&self, o: &Segment) -> bool {
        self.intersection(o) != SegmentIntersection::None
    }

    /// Closest point on the segment to `p`, rounded to the nearest nm.
    pub fn nearest_point(&self, p: Point) -> Point {
        let d = self.direction();
        let len_sq = d.length_sq();
        if len_sq == 0 {
            return self.a;
        }
        let t = d.dot(p - self.a);
        if t <= 0 {
            self.a
        } else if t >= len_sq {
            self.b
        } else {
            project(self.a, d, t, len_sq)
        }
    }

    pub fn distance_to_point(&self, p: Point) -> f64 {
        let d = self.direction();
        let len_sq = d.length_sq();
        if len_sq == 0 {
            return self.a.distance(p);
        }
        let t = d.dot(p - self.a);
        if t <= 0 {
            self.a.distance(p)
        } else if t >= len_sq {
            self.b.distance(p)
        } else {
            d.cross(p - self.a).unsigned_abs() as f64 / (len_sq as f64).sqrt()
        }
    }

    /// Exact: is `p` at distance `<= d` from the segment?
    pub fn within_distance_of_point(&self, p: Point, d: Coord) -> bool {
        if d < 0 {
            return false;
        }
        let d2 = d as i128 * d as i128;
        let dir = self.direction();
        let len_sq = dir.length_sq();
        let t = dir.dot(p - self.a);
        if len_sq == 0 || t <= 0 {
            self.a.distance_sq(p) <= d2
        } else if t >= len_sq {
            self.b.distance_sq(p) <= d2
        } else {
            let c = dir.cross(p - self.a);
            c * c <= d2 * len_sq
        }
    }

    /// Exact: are the segments at distance `<= d` from each other?
    pub fn within_distance_of_segment(&self, o: &Segment, d: Coord) -> bool {
        self.intersects(o)
            || self.within_distance_of_point(o.a, d)
            || self.within_distance_of_point(o.b, d)
            || o.within_distance_of_point(self.a, d)
            || o.within_distance_of_point(self.b, d)
    }

    pub fn distance_to_segment(&self, o: &Segment) -> f64 {
        if self.intersects(o) {
            return 0.0;
        }
        [
            self.distance_to_point(o.a),
            self.distance_to_point(o.b),
            o.distance_to_point(self.a),
            o.distance_to_point(self.b),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
    }

    /// Parallel segment shifted to the left of the direction by `d` (negative → right).
    /// Endpoints are rounded to the nearest nm. Errors on a degenerate segment.
    pub fn offset(&self, d: Coord) -> Result<Segment, GeometryError> {
        let dir = self.direction();
        if dir.is_zero() {
            return Err(GeometryError::DegenerateLine);
        }
        let n = dir.perp();
        let k = d as f64 / dir.length();
        let shift = Vector::new(round_f64(n.x as f64 * k), round_f64(n.y as f64 * k));
        Ok(self.translate(shift))
    }
}

impl Bounded for Segment {
    fn bounding_box(&self) -> BoundingBox {
        BoundingBox::new(self.a, self.b)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point::new(x, y)
    }
    fn s(a: (i64, i64), b: (i64, i64)) -> Segment {
        Segment::new(p(a.0, a.1), p(b.0, b.1))
    }

    #[test]
    fn crossing_segments() {
        assert_eq!(
            s((0, 0), (10, 10)).intersection(&s((0, 10), (10, 0))),
            SegmentIntersection::Point(p(5, 5))
        );
    }

    #[test]
    fn touching_at_endpoint_and_t_junction() {
        assert_eq!(
            s((0, 0), (5, 0)).intersection(&s((5, 0), (5, 5))),
            SegmentIntersection::Point(p(5, 0))
        );
        assert_eq!(
            s((0, 0), (10, 0)).intersection(&s((4, 0), (4, 7))),
            SegmentIntersection::Point(p(4, 0))
        );
    }

    #[test]
    fn near_miss_does_not_intersect() {
        assert!(!s((0, 0), (10, 0)).intersects(&s((5, 1), (5, 9))));
        assert!(!s((0, 0), (4, 4)).intersects(&s((0, 10), (10, 0)).translate(Vector::new(1, 1))));
    }

    #[test]
    fn collinear_cases() {
        assert_eq!(
            s((0, 0), (10, 0)).intersection(&s((5, 0), (20, 0))),
            SegmentIntersection::Overlap(s((5, 0), (10, 0)))
        );
        assert_eq!(
            s((0, 0), (5, 0)).intersection(&s((5, 0), (9, 0))),
            SegmentIntersection::Point(p(5, 0))
        );
        assert_eq!(
            s((0, 0), (4, 0)).intersection(&s((5, 0), (9, 0))),
            SegmentIntersection::None
        );
        // Vertical and diagonal collinear.
        assert_eq!(
            s((0, 0), (0, 10)).intersection(&s((0, 8), (0, 3))),
            SegmentIntersection::Overlap(s((0, 3), (0, 8)))
        );
        assert!(s((0, 0), (10, 10)).intersects(&s((12, 12), (5, 5))));
    }

    #[test]
    fn parallel_disjoint_and_degenerate() {
        assert_eq!(
            s((0, 0), (10, 0)).intersection(&s((0, 1), (10, 1))),
            SegmentIntersection::None
        );
        assert_eq!(
            s((3, 0), (3, 0)).intersection(&s((0, 0), (10, 0))),
            SegmentIntersection::Point(p(3, 0))
        );
        assert_eq!(
            s((3, 1), (3, 1)).intersection(&s((0, 0), (10, 0))),
            SegmentIntersection::None
        );
    }

    #[test]
    fn nearest_point_and_distance() {
        let seg = s((0, 0), (10, 0));
        assert_eq!(seg.nearest_point(p(4, 7)), p(4, 0));
        assert_eq!(seg.nearest_point(p(-5, 3)), p(0, 0));
        assert_eq!(seg.nearest_point(p(50, 3)), p(10, 0));
        assert_eq!(seg.distance_to_point(p(4, 7)), 7.0);
        assert_eq!(seg.distance_to_point(p(13, 4)), 5.0);
    }

    #[test]
    fn within_distance_is_exact_at_the_boundary() {
        let seg = s((0, 0), (10, 0));
        assert!(seg.within_distance_of_point(p(5, 3), 3));
        assert!(!seg.within_distance_of_point(p(5, 3), 2));
        assert!(seg.within_distance_of_point(p(13, 4), 5));
        assert!(!seg.within_distance_of_point(p(13, 4), 4));
        assert!(s((0, 5), (10, 5)).within_distance_of_segment(&seg, 5));
        assert!(!s((0, 5), (10, 5)).within_distance_of_segment(&seg, 4));
    }

    #[test]
    fn segment_distance() {
        assert_eq!(
            s((0, 0), (10, 0)).distance_to_segment(&s((0, 4), (10, 4))),
            4.0
        );
        assert_eq!(
            s((0, 0), (10, 10)).distance_to_segment(&s((0, 10), (10, 0))),
            0.0
        );
    }

    #[test]
    fn offset_moves_left() {
        assert_eq!(s((0, 0), (10, 0)).offset(3).unwrap(), s((0, 3), (10, 3)));
        assert_eq!(s((0, 0), (10, 0)).offset(-3).unwrap(), s((0, -3), (10, -3)));
        assert!(s((1, 1), (1, 1)).offset(1).is_err());
    }

    #[test]
    fn line_operations() {
        let l1 = Line::through(p(0, 0), p(10, 0)).unwrap();
        let l2 = Line::through(p(3, -5), p(3, 5)).unwrap();
        assert_eq!(l1.intersection(&l2), LineIntersection::Point(p(3, 0)));
        assert_eq!(
            l1.intersection(&Line::through(p(0, 1), p(1, 1)).unwrap()),
            LineIntersection::Parallel
        );
        assert_eq!(
            l1.intersection(&Line::through(p(2, 0), p(7, 0)).unwrap()),
            LineIntersection::Coincident
        );
        assert_eq!(l1.side_of(p(1, 1)), Side::Left);
        assert_eq!(l1.side_of(p(1, -1)), Side::Right);
        assert_eq!(l1.distance_to_point(p(4, -6)), 6.0);
        assert_eq!(l1.nearest_point(p(4, -6)), p(4, 0));
        assert!(Line::through(p(1, 1), p(1, 1)).is_err());
    }
}
