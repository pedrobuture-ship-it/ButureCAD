use crate::bbox::{Bounded, BoundingBox};
use crate::error::GeometryError;
use crate::line::Segment;
use crate::point::{Angle, Point, Vector};
use crate::predicates::{round_f64, PointLocation};
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// A circle (disc) with an integer radius in nm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Circle {
    pub center: Point,
    pub radius: Coord,
}

impl Circle {
    pub fn new(center: Point, radius: Coord) -> Result<Self, GeometryError> {
        if radius < 0 {
            return Err(GeometryError::NegativeRadius);
        }
        Ok(Self { center, radius })
    }

    fn radius_sq(&self) -> i128 {
        self.radius as i128 * self.radius as i128
    }

    /// Exact location of `p` relative to the disc.
    pub fn locate(&self, p: Point) -> PointLocation {
        match self.center.distance_sq(p).cmp(&self.radius_sq()) {
            std::cmp::Ordering::Less => PointLocation::Inside,
            std::cmp::Ordering::Equal => PointLocation::Boundary,
            std::cmp::Ordering::Greater => PointLocation::Outside,
        }
    }

    pub fn contains_point(&self, p: Point) -> bool {
        self.locate(p).is_contained()
    }

    /// Exact: do the discs overlap or touch?
    pub fn collides_circle(&self, o: &Circle) -> bool {
        let r = self.radius as i128 + o.radius as i128;
        self.center.distance_sq(o.center) <= r * r
    }

    /// Exact: does the disc lie entirely inside `o`?
    pub fn is_inside_circle(&self, o: &Circle) -> bool {
        self.radius <= o.radius && {
            let r = (o.radius - self.radius) as i128;
            self.center.distance_sq(o.center) <= r * r
        }
    }

    /// Exact: does the disc touch the segment?
    pub fn collides_segment(&self, s: &Segment) -> bool {
        s.within_distance_of_point(self.center, self.radius)
    }

    /// Distance from `p` to the disc (0 inside).
    pub fn distance_to_point(&self, p: Point) -> f64 {
        (self.center.distance(p) - self.radius as f64).max(0.0)
    }

    /// Closest point on the circumference to `p`, rounded to the nearest nm.
    /// For `p == center` every point qualifies; the +X point is returned.
    pub fn nearest_boundary_point(&self, p: Point) -> Point {
        let v = p - self.center;
        if v.is_zero() {
            return self.center + Vector::new(self.radius, 0);
        }
        let k = self.radius as f64 / v.length();
        self.center + Vector::new(round_f64(v.x as f64 * k), round_f64(v.y as f64 * k))
    }

    /// Circumference intersection points with `o` (0–2 points). **Numerical (f64)**, rounded to
    /// the nearest nm; tangent configurations are reported when the discriminant is within
    /// floating-point noise of zero. Use [`Self::collides_circle`] for exact yes/no.
    pub fn intersections_with_circle(&self, o: &Circle) -> Vec<Point> {
        let d = self.center.distance(o.center);
        let (r0, r1) = (self.radius as f64, o.radius as f64);
        if d == 0.0 || d > r0 + r1 || d < (r0 - r1).abs() {
            return Vec::new();
        }
        let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
        let h = (r0 * r0 - a * a).max(0.0).sqrt();
        let u = o.center - self.center;
        let (ux, uy) = (u.x as f64 / d, u.y as f64 / d);
        let (mx, my) = (self.center.x as f64 + a * ux, self.center.y as f64 + a * uy);
        let p1 = Point::new(round_f64(mx - h * uy), round_f64(my + h * ux));
        let p2 = Point::new(round_f64(mx + h * uy), round_f64(my - h * ux));
        if p1 == p2 {
            vec![p1]
        } else {
            vec![p1, p2]
        }
    }

    /// Circumference intersection points with a segment (0–2). **Numerical (f64)**, rounded.
    pub fn intersections_with_segment(&self, s: &Segment) -> Vec<Point> {
        let d = s.direction();
        let len_sq = d.length_sq() as f64;
        if len_sq == 0.0 {
            return Vec::new();
        }
        let f = s.a - self.center;
        let b = f.dot(d) as f64;
        let c = f.length_sq() as f64 - self.radius_sq() as f64;
        let disc = b * b - len_sq * c;
        if disc < 0.0 {
            return Vec::new();
        }
        let sq = disc.sqrt();
        let mut out = Vec::new();
        for t in [(-b - sq) / len_sq, (-b + sq) / len_sq] {
            if (0.0..=1.0).contains(&t) {
                let p = Point::new(
                    s.a.x + round_f64(d.x as f64 * t),
                    s.a.y + round_f64(d.y as f64 * t),
                );
                if out.last() != Some(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Grows (or shrinks, for negative `d`) the radius. Errors if the radius would go negative.
    pub fn offset(&self, d: Coord) -> Result<Circle, GeometryError> {
        let r = self
            .radius
            .checked_add(d)
            .ok_or(GeometryError::OutOfRange)?;
        if r < 0 {
            return Err(GeometryError::OffsetTopology);
        }
        Ok(Circle {
            center: self.center,
            radius: r,
        })
    }

    pub fn translate(&self, v: Vector) -> Circle {
        Circle {
            center: self.center + v,
            radius: self.radius,
        }
    }

    pub fn rotate_around(&self, c: Point, angle: Angle) -> Circle {
        Circle {
            center: self.center.rotate_around(c, angle),
            radius: self.radius,
        }
    }
}

impl Bounded for Circle {
    fn bounding_box(&self) -> BoundingBox {
        let r = Vector::new(self.radius, self.radius);
        BoundingBox::new(self.center - r, self.center + r)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn c(x: i64, y: i64, r: i64) -> Circle {
        Circle::new(Point::new(x, y), r).unwrap()
    }

    #[test]
    fn location_is_exact() {
        let k = c(0, 0, 5);
        assert_eq!(k.locate(Point::new(3, 4)), PointLocation::Boundary);
        assert_eq!(k.locate(Point::new(3, 3)), PointLocation::Inside);
        assert_eq!(k.locate(Point::new(4, 4)), PointLocation::Outside);
    }

    #[test]
    fn collisions() {
        assert!(c(0, 0, 5).collides_circle(&c(10, 0, 5)));
        assert!(!c(0, 0, 5).collides_circle(&c(11, 0, 5)));
        assert!(c(0, 0, 2).is_inside_circle(&c(1, 0, 3)));
        assert!(!c(0, 0, 2).is_inside_circle(&c(2, 0, 3)));
        let seg = Segment::new(Point::new(-10, 5), Point::new(10, 5));
        assert!(c(0, 0, 5).collides_segment(&seg));
        assert!(!c(0, 0, 4).collides_segment(&seg));
    }

    #[test]
    fn intersection_points() {
        let pts = c(0, 0, 5).intersections_with_circle(&c(6, 0, 5));
        assert_eq!(pts, vec![Point::new(3, 4), Point::new(3, -4)]);
        assert!(c(0, 0, 5)
            .intersections_with_circle(&c(20, 0, 5))
            .is_empty());
        let seg = Segment::new(Point::new(-10, 0), Point::new(10, 0));
        assert_eq!(
            c(0, 0, 5).intersections_with_segment(&seg),
            vec![Point::new(-5, 0), Point::new(5, 0)]
        );
        let short = Segment::new(Point::new(0, 0), Point::new(10, 0));
        assert_eq!(
            c(0, 0, 5).intersections_with_segment(&short),
            vec![Point::new(5, 0)]
        );
    }

    #[test]
    fn offset_and_bbox() {
        assert_eq!(c(0, 0, 5).offset(2).unwrap().radius, 7);
        assert!(c(0, 0, 5).offset(-6).is_err());
        assert!(Circle::new(Point::ORIGIN, -1).is_err());
        let b = c(1, 2, 3).bounding_box();
        assert_eq!((b.min, b.max), (Point::new(-2, -1), Point::new(4, 5)));
    }

    #[test]
    fn nearest_boundary() {
        assert_eq!(
            c(0, 0, 5).nearest_boundary_point(Point::new(0, 20)),
            Point::new(0, 5)
        );
        assert_eq!(c(0, 0, 5).distance_to_point(Point::new(0, 20)), 15.0);
    }
}
