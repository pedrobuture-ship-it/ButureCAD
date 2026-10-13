use crate::bbox::{Bounded, BoundingBox};
use crate::circle::Circle;
use crate::error::GeometryError;
use crate::line::Segment;
use crate::point::{Angle, Point, Vector};
use crate::predicates::round_f64;
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// A circular arc defined by its center, exact endpoints and sweep direction.
///
/// The arc runs from `start` to `end`, counter-clockwise when `ccw` is true, clockwise
/// otherwise. A full circle is a [`Circle`], not an arc. Because endpoints are integer points,
/// the two endpoint radii may differ by rounding; construction accepts up to 1 nm of difference.
///
/// Sweep-membership tests are exact (integer cross/dot products). Distances, nearest points,
/// offsets and intersections involve a square root or trigonometry and are **numerical (f64)**,
/// rounded to the nearest nm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Arc {
    pub center: Point,
    pub start: Point,
    pub end: Point,
    pub ccw: bool,
}

impl Arc {
    pub fn new(center: Point, start: Point, end: Point, ccw: bool) -> Result<Self, GeometryError> {
        let (u, v) = (start - center, end - center);
        let (rs, re) = (u.length(), v.length());
        let same_direction = u.cross(v) == 0 && u.dot(v) > 0;
        if rs == 0.0 || re == 0.0 || (rs - re).abs() > 1.0 || same_direction {
            return Err(GeometryError::DegenerateArc);
        }
        Ok(Self {
            center,
            start,
            end,
            ccw,
        })
    }

    /// Nominal radius (start radius).
    pub fn radius(&self) -> f64 {
        (self.start - self.center).length()
    }

    /// Counter-clockwise sweep as (from, to) direction vectors.
    fn ccw_span(&self) -> (Vector, Vector) {
        let (u, v) = (self.start - self.center, self.end - self.center);
        if self.ccw {
            (u, v)
        } else {
            (v, u)
        }
    }

    /// Exact: does direction `dir` (from the center) fall inside the arc's angular sweep?
    /// Endpoints are included.
    pub fn sweeps(&self, dir: Vector) -> bool {
        if dir.is_zero() {
            return false;
        }
        let (u, v) = self.ccw_span();
        let uv = u.cross(v);
        if uv > 0 {
            u.cross(dir) >= 0 && dir.cross(v) >= 0
        } else if uv < 0 {
            !(v.cross(dir) > 0 && dir.cross(u) > 0)
        } else {
            // Exactly 180° (the constructor rejects 0°).
            u.cross(dir) >= 0
        }
    }

    /// Sweep angle in radians, in `(0, 2π)`.
    pub fn sweep_angle(&self) -> f64 {
        let (u, v) = self.ccw_span();
        let a = (u.cross(v) as f64).atan2(u.dot(v) as f64);
        if a <= 0.0 {
            a + std::f64::consts::TAU
        } else {
            a
        }
    }

    pub fn length(&self) -> f64 {
        self.radius() * self.sweep_angle()
    }

    /// Distance from `p` to the arc curve.
    pub fn distance_to_point(&self, p: Point) -> f64 {
        let v = p - self.center;
        if self.sweeps(v) {
            (v.length() - self.radius()).abs()
        } else {
            p.distance(self.start).min(p.distance(self.end))
        }
    }

    /// Closest point on the arc to `p`, rounded to the nearest nm. For `p == center` the start
    /// point is returned (all arc points are equidistant).
    pub fn nearest_point(&self, p: Point) -> Point {
        let v = p - self.center;
        if v.is_zero() {
            return self.start;
        }
        if self.sweeps(v) {
            let k = self.radius() / v.length();
            return self.center + Vector::new(round_f64(v.x as f64 * k), round_f64(v.y as f64 * k));
        }
        if p.distance_sq(self.start) <= p.distance_sq(self.end) {
            self.start
        } else {
            self.end
        }
    }

    /// Intersection points with a segment. **Numerical**; the circle radius is rounded to the
    /// nearest nm before intersecting, then points outside the sweep are discarded.
    pub fn intersections_with_segment(&self, s: &Segment) -> Vec<Point> {
        let circle = Circle {
            center: self.center,
            radius: round_f64(self.radius()),
        };
        circle
            .intersections_with_segment(s)
            .into_iter()
            .filter(|p| self.sweeps(*p - self.center))
            .collect()
    }

    /// Concentric arc with radius changed by `d` (positive = away from the center).
    pub fn offset(&self, d: Coord) -> Result<Arc, GeometryError> {
        let r = self.radius();
        let nr = r + d as f64;
        if nr < 1.0 {
            return Err(GeometryError::OffsetTopology);
        }
        let k = nr / r;
        let scale = |p: Point| {
            let v = p - self.center;
            self.center + Vector::new(round_f64(v.x as f64 * k), round_f64(v.y as f64 * k))
        };
        Arc::new(self.center, scale(self.start), scale(self.end), self.ccw)
    }

    pub fn translate(&self, v: Vector) -> Arc {
        Arc {
            center: self.center + v,
            start: self.start + v,
            end: self.end + v,
            ccw: self.ccw,
        }
    }

    /// Rotation preserves the sweep direction. Endpoints are rounded individually.
    pub fn rotate_around(&self, c: Point, angle: Angle) -> Arc {
        Arc {
            center: self.center.rotate_around(c, angle),
            start: self.start.rotate_around(c, angle),
            end: self.end.rotate_around(c, angle),
            ccw: self.ccw,
        }
    }
}

impl Bounded for Arc {
    /// Conservative: axis extremes use the radius rounded up, so the box may exceed the exact
    /// extent by at most 1 nm on a side.
    fn bounding_box(&self) -> BoundingBox {
        let mut b = BoundingBox::new(self.start, self.end);
        let r = self.radius().ceil() as Coord;
        for dir in [
            Vector::new(1, 0),
            Vector::new(0, 1),
            Vector::new(-1, 0),
            Vector::new(0, -1),
        ] {
            if self.sweeps(dir) {
                b = b.union(&BoundingBox::from_point(
                    self.center + Vector::new(dir.x * r, dir.y * r),
                ));
            }
        }
        b
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point::new(x, y)
    }
    /// Quarter arc from (10,0) to (0,10) around the origin.
    fn quarter(ccw: bool) -> Arc {
        Arc::new(Point::ORIGIN, p(10, 0), p(0, 10), ccw).unwrap()
    }

    #[test]
    fn rejects_degenerate() {
        assert!(Arc::new(Point::ORIGIN, p(10, 0), p(10, 0), true).is_err());
        assert!(Arc::new(Point::ORIGIN, p(10, 0), p(0, 20), true).is_err());
        assert!(Arc::new(Point::ORIGIN, p(0, 0), p(0, 10), true).is_err());
    }

    #[test]
    fn sweep_membership_ccw_and_cw() {
        let a = quarter(true);
        assert!(a.sweeps(Vector::new(1, 1)));
        assert!(a.sweeps(Vector::new(1, 0)) && a.sweeps(Vector::new(0, 1)));
        assert!(!a.sweeps(Vector::new(-1, 1)) && !a.sweeps(Vector::new(1, -1)));
        // The clockwise arc between the same ends is the long way round (270°).
        let c = quarter(false);
        assert!(!c.sweeps(Vector::new(1, 1)));
        assert!(c.sweeps(Vector::new(-1, -1)) && c.sweeps(Vector::new(-1, 1)));
    }

    #[test]
    fn half_circle() {
        let a = Arc::new(Point::ORIGIN, p(10, 0), p(-10, 0), true).unwrap();
        assert!(a.sweeps(Vector::new(0, 1)));
        assert!(!a.sweeps(Vector::new(0, -1)));
        assert!((a.sweep_angle() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn length_and_bbox() {
        let a = quarter(true);
        assert!((a.length() - 10.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let b = a.bounding_box();
        assert_eq!((b.min, b.max), (p(0, 0), p(10, 10)));
        // 270° arc includes the -x and -y extremes.
        let b = quarter(false).bounding_box();
        assert_eq!((b.min, b.max), (p(-10, -10), p(10, 10)));
    }

    #[test]
    fn nearest_and_distance() {
        let a = quarter(true);
        assert_eq!(a.nearest_point(p(20, 20)), p(7, 7));
        assert_eq!(a.nearest_point(p(20, -5)), p(10, 0));
        assert!((a.distance_to_point(p(30, 0)) - 20.0).abs() < 1e-9);
        assert!(
            (a.distance_to_point(p(-3, -4))
                - Point::new(-3, -4)
                    .distance(p(10, 0))
                    .min(Point::new(-3, -4).distance(p(0, 10))))
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn segment_intersections_respect_sweep() {
        let a = quarter(true);
        let s = Segment::new(p(-20, 0), p(20, 0));
        assert_eq!(a.intersections_with_segment(&s), vec![p(10, 0)]);
        let s = Segment::new(p(5, -20), p(5, 20));
        assert_eq!(a.intersections_with_segment(&s).len(), 1);
    }

    #[test]
    fn offset_and_transforms() {
        let a = quarter(true).offset(10).unwrap();
        assert_eq!((a.start, a.end), (p(20, 0), p(0, 20)));
        assert!(quarter(true).offset(-10).is_err());
        let r = quarter(true).rotate_around(Point::ORIGIN, Angle::from_degrees(90.0).unwrap());
        assert_eq!((r.start, r.end), (p(0, 10), p(-10, 0)));
        assert_eq!(quarter(true).translate(Vector::new(5, 5)).center, p(5, 5));
    }
}
