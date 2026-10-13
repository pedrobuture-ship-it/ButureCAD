use crate::bbox::{Bounded, BoundingBox};
use crate::error::GeometryError;
use crate::point::{Point, Vector};
use crate::polygon::Polygon;
use crate::predicates::PointLocation;
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// An axis-aligned rectangle with positive width and height.
///
/// Rotation by non-multiples of 90° does not yield a rectangle; convert with
/// [`Rectangle::to_polygon`] first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rectangle {
    min: Point,
    max: Point,
}

impl Rectangle {
    /// Rectangle spanning two opposite corners. Errors when it has zero width or height.
    pub fn new(a: Point, b: Point) -> Result<Self, GeometryError> {
        let bb = BoundingBox::new(a, b);
        if bb.width() == 0 || bb.height() == 0 {
            return Err(GeometryError::DegeneratePolygon);
        }
        Ok(Self {
            min: bb.min,
            max: bb.max,
        })
    }

    /// Rectangle from its center and full size.
    pub fn from_center(center: Point, width: Coord, height: Coord) -> Result<Self, GeometryError> {
        let half = Vector::new(width / 2, height / 2);
        let other = Vector::new(width - width / 2, height - height / 2);
        Self::new(center - half, center + other)
    }

    pub fn min(&self) -> Point {
        self.min
    }
    pub fn max(&self) -> Point {
        self.max
    }
    pub fn width(&self) -> Coord {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> Coord {
        self.max.y - self.min.y
    }
    /// Exact area in nm².
    pub fn area(&self) -> i128 {
        self.width() as i128 * self.height() as i128
    }

    pub fn locate(&self, p: Point) -> PointLocation {
        self.bounding_box().locate(p)
    }
    pub fn contains_point(&self, p: Point) -> bool {
        self.locate(p).is_contained()
    }
    /// Exact: overlap or touch.
    pub fn collides(&self, o: &Rectangle) -> bool {
        self.bounding_box().intersects(&o.bounding_box())
    }
    pub fn distance_to_point(&self, p: Point) -> f64 {
        (self.bounding_box().distance_sq_to_point(p) as f64).sqrt()
    }

    /// Grows (or shrinks, for negative `d`) every side by `d`; errors if it collapses.
    pub fn offset(&self, d: Coord) -> Result<Rectangle, GeometryError> {
        let bb = self
            .bounding_box()
            .inflate(d)
            .ok_or(GeometryError::OffsetTopology)?;
        Rectangle::new(bb.min, bb.max).map_err(|_| GeometryError::OffsetTopology)
    }

    pub fn translate(&self, v: Vector) -> Rectangle {
        Rectangle {
            min: self.min + v,
            max: self.max + v,
        }
    }

    /// Exact rotation by `n` quarter turns counter-clockwise around `center`.
    pub fn rotate_quarter_turns(&self, center: Point, n: u8) -> Rectangle {
        let a = self.min.rotate_quarter_turns(center, n);
        let b = self.max.rotate_quarter_turns(center, n);
        let bb = BoundingBox::new(a, b);
        Rectangle {
            min: bb.min,
            max: bb.max,
        }
    }

    /// Counter-clockwise polygon of the four corners.
    pub fn to_polygon(&self) -> Polygon {
        let (a, c) = (self.min, self.max);
        Polygon::from_valid_vertices(vec![a, Point::new(c.x, a.y), c, Point::new(a.x, c.y)])
    }
}

impl Bounded for Rectangle {
    fn bounding_box(&self) -> BoundingBox {
        BoundingBox {
            min: self.min,
            max: self.max,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn r(x0: i64, y0: i64, x1: i64, y1: i64) -> Rectangle {
        Rectangle::new(Point::new(x0, y0), Point::new(x1, y1)).unwrap()
    }

    #[test]
    fn construction() {
        assert_eq!(r(10, 10, 0, 0), r(0, 0, 10, 10));
        assert!(Rectangle::new(Point::new(0, 0), Point::new(0, 5)).is_err());
        assert_eq!(
            Rectangle::from_center(Point::ORIGIN, 10, 4).unwrap(),
            r(-5, -2, 5, 2)
        );
    }

    #[test]
    fn collisions_and_containment() {
        assert!(r(0, 0, 10, 10).collides(&r(10, 10, 20, 20)));
        assert!(!r(0, 0, 10, 10).collides(&r(11, 0, 20, 10)));
        assert_eq!(
            r(0, 0, 10, 10).locate(Point::new(0, 5)),
            PointLocation::Boundary
        );
        assert_eq!(r(0, 0, 10, 10).area(), 100);
        assert_eq!(r(0, 0, 10, 10).distance_to_point(Point::new(13, 14)), 5.0);
    }

    #[test]
    fn offset_and_rotate() {
        assert_eq!(r(0, 0, 10, 10).offset(1).unwrap(), r(-1, -1, 11, 11));
        assert!(r(0, 0, 10, 10).offset(-5).is_err());
        assert_eq!(
            r(0, 0, 10, 4).rotate_quarter_turns(Point::ORIGIN, 1),
            r(-4, 0, 0, 10)
        );
        assert_eq!(r(0, 0, 10, 4).to_polygon().signed_area2(), 80);
    }
}
