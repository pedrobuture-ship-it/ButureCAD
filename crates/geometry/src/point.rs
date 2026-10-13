use crate::error::GeometryError;
use crate::units::{Coord, MAX_COORD};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Neg, Sub};

/// A position, in nanometres.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct Point {
    pub x: Coord,
    pub y: Coord,
}

/// A displacement, in nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Vector {
    pub x: Coord,
    pub y: Coord,
}

/// An angle in degrees, counter-clockwise, normalized to `[0, 360)`.
///
/// Multiples of 90° are detected and rotate exactly (no rounding).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle(f64);

impl Angle {
    pub const ZERO: Angle = Angle(0.0);

    pub fn from_degrees(deg: f64) -> Result<Self, GeometryError> {
        if !deg.is_finite() {
            return Err(GeometryError::NotFinite);
        }
        Ok(Angle(deg.rem_euclid(360.0)))
    }
    pub fn degrees(self) -> f64 {
        self.0
    }
    /// `Some(n)` (0..4) when the angle is exactly `n * 90°`.
    pub fn quarter_turns(self) -> Option<u8> {
        let q = self.0 / 90.0;
        (q.fract() == 0.0).then_some((q as u8) % 4)
    }
}

impl Point {
    pub const ORIGIN: Point = Point { x: 0, y: 0 };

    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }

    /// Validating constructor: rejects coordinates outside `±MAX_COORD`.
    pub fn try_new(x: Coord, y: Coord) -> Result<Self, GeometryError> {
        if x.abs() > MAX_COORD || y.abs() > MAX_COORD {
            return Err(GeometryError::OutOfRange);
        }
        Ok(Self { x, y })
    }

    pub fn distance_sq(self, other: Point) -> i128 {
        (other - self).length_sq()
    }

    pub fn distance(self, other: Point) -> f64 {
        (self.distance_sq(other) as f64).sqrt()
    }

    pub fn translate(self, v: Vector) -> Point {
        self + v
    }

    /// Exact rotation by `n` quarter turns (counter-clockwise) around `center`.
    pub fn rotate_quarter_turns(self, center: Point, n: u8) -> Point {
        let v = self - center;
        let r = match n % 4 {
            0 => v,
            1 => Vector {
                x: v.y.saturating_neg(),
                y: v.x,
            },
            2 => Vector {
                x: v.x.saturating_neg(),
                y: v.y.saturating_neg(),
            },
            _ => Vector {
                x: v.y,
                y: v.x.saturating_neg(),
            },
        };
        center + r
    }

    /// Rotation around `center`. Exact for multiples of 90°, otherwise rounded to the nearest nm.
    pub fn rotate_around(self, center: Point, angle: Angle) -> Point {
        if let Some(q) = angle.quarter_turns() {
            return self.rotate_quarter_turns(center, q);
        }
        let (s, c) = angle.0.to_radians().sin_cos();
        let v = self - center;
        let (vx, vy) = (v.x as f64, v.y as f64);
        Point::new(
            center.x.saturating_add((c * vx - s * vy).round() as Coord),
            center.y.saturating_add((s * vx + c * vy).round() as Coord),
        )
    }
}

impl Vector {
    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }
    pub fn dot(self, o: Vector) -> i128 {
        self.x as i128 * o.x as i128 + self.y as i128 * o.y as i128
    }
    /// 2D cross product (z component), exact.
    pub fn cross(self, o: Vector) -> i128 {
        self.x as i128 * o.y as i128 - self.y as i128 * o.x as i128
    }
    pub fn length_sq(self) -> i128 {
        self.dot(self)
    }
    pub fn length(self) -> f64 {
        (self.length_sq() as f64).sqrt()
    }
    pub fn is_zero(self) -> bool {
        self.x == 0 && self.y == 0
    }
    /// Rotated 90° counter-clockwise.
    pub fn perp(self) -> Vector {
        Vector {
            x: self.y.saturating_neg(),
            y: self.x,
        }
    }
}

// Arithmetic saturates instead of wrapping so out-of-range input degrades visibly, never silently.
impl Sub for Point {
    type Output = Vector;
    fn sub(self, o: Point) -> Vector {
        Vector {
            x: self.x.saturating_sub(o.x),
            y: self.y.saturating_sub(o.y),
        }
    }
}
impl Add<Vector> for Point {
    type Output = Point;
    fn add(self, v: Vector) -> Point {
        Point {
            x: self.x.saturating_add(v.x),
            y: self.y.saturating_add(v.y),
        }
    }
}
impl Sub<Vector> for Point {
    type Output = Point;
    fn sub(self, v: Vector) -> Point {
        Point {
            x: self.x.saturating_sub(v.x),
            y: self.y.saturating_sub(v.y),
        }
    }
}
impl Add for Vector {
    type Output = Vector;
    fn add(self, o: Vector) -> Vector {
        Vector {
            x: self.x.saturating_add(o.x),
            y: self.y.saturating_add(o.y),
        }
    }
}
impl Sub for Vector {
    type Output = Vector;
    fn sub(self, o: Vector) -> Vector {
        Vector {
            x: self.x.saturating_sub(o.x),
            y: self.y.saturating_sub(o.y),
        }
    }
}
impl Neg for Vector {
    type Output = Vector;
    fn neg(self) -> Vector {
        Vector {
            x: self.x.saturating_neg(),
            y: self.y.saturating_neg(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_are_exact() {
        let p = Point::new(10, 0);
        assert_eq!(
            p.rotate_around(Point::ORIGIN, Angle::from_degrees(90.0).unwrap()),
            Point::new(0, 10)
        );
        assert_eq!(
            p.rotate_around(Point::ORIGIN, Angle::from_degrees(-90.0).unwrap()),
            Point::new(0, -10)
        );
        assert_eq!(
            p.rotate_around(Point::ORIGIN, Angle::from_degrees(180.0).unwrap()),
            Point::new(-10, 0)
        );
    }

    #[test]
    fn arbitrary_rotation_rounds_to_nearest() {
        let p = Point::new(1_000_000, 0);
        let r = p.rotate_around(Point::ORIGIN, Angle::from_degrees(45.0).unwrap());
        assert_eq!(r, Point::new(707_107, 707_107));
    }

    #[test]
    fn cross_and_dot_do_not_overflow() {
        let a = Vector::new(MAX_COORD, MAX_COORD);
        let b = Vector::new(-MAX_COORD, MAX_COORD);
        assert_eq!(a.cross(b), 2 * (MAX_COORD as i128).pow(2));
    }

    #[test]
    fn try_new_validates() {
        assert!(Point::try_new(MAX_COORD + 1, 0).is_err());
        assert!(Point::try_new(MAX_COORD, -MAX_COORD).is_ok());
    }
}
