use crate::point::{Point, Vector};
use crate::predicates::PointLocation;
use crate::units::Coord;
use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box, inclusive on all sides. Always non-empty (`min <= max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min: Point,
    pub max: Point,
}

/// Anything with a bounding box.
pub trait Bounded {
    fn bounding_box(&self) -> BoundingBox;
}

impl BoundingBox {
    /// Box spanning two arbitrary corners.
    pub fn new(a: Point, b: Point) -> Self {
        Self {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    pub fn from_point(p: Point) -> Self {
        Self { min: p, max: p }
    }

    /// Smallest box containing all points; `None` for an empty iterator.
    pub fn from_points(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        Some(it.fold(Self::from_point(first), |b, p| {
            b.union(&Self::from_point(p))
        }))
    }

    pub fn width(&self) -> Coord {
        self.max.x.saturating_sub(self.min.x)
    }
    pub fn height(&self) -> Coord {
        self.max.y.saturating_sub(self.min.y)
    }
    pub fn center(&self) -> Point {
        Point::new(
            ((self.min.x as i128 + self.max.x as i128).div_euclid(2)) as Coord,
            ((self.min.y as i128 + self.max.y as i128).div_euclid(2)) as Coord,
        )
    }

    pub fn union(&self, o: &BoundingBox) -> BoundingBox {
        BoundingBox {
            min: Point::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Point::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }

    /// Overlap region (touching boxes yield a degenerate box), or `None` when disjoint.
    pub fn intersection(&self, o: &BoundingBox) -> Option<BoundingBox> {
        let min = Point::new(self.min.x.max(o.min.x), self.min.y.max(o.min.y));
        let max = Point::new(self.max.x.min(o.max.x), self.max.y.min(o.max.y));
        (min.x <= max.x && min.y <= max.y).then_some(BoundingBox { min, max })
    }

    /// True when the boxes overlap or touch.
    pub fn intersects(&self, o: &BoundingBox) -> bool {
        self.intersection(o).is_some()
    }

    pub fn contains_point(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn locate(&self, p: Point) -> PointLocation {
        if !self.contains_point(p) {
            PointLocation::Outside
        } else if p.x == self.min.x || p.x == self.max.x || p.y == self.min.y || p.y == self.max.y {
            PointLocation::Boundary
        } else {
            PointLocation::Inside
        }
    }

    pub fn contains_box(&self, o: &BoundingBox) -> bool {
        self.contains_point(o.min) && self.contains_point(o.max)
    }

    /// Grows (or, for negative `d`, shrinks) every side by `d`. Shrinking past zero size yields `None`.
    pub fn inflate(&self, d: Coord) -> Option<BoundingBox> {
        let min = Point::new(self.min.x.saturating_sub(d), self.min.y.saturating_sub(d));
        let max = Point::new(self.max.x.saturating_add(d), self.max.y.saturating_add(d));
        (min.x <= max.x && min.y <= max.y).then_some(BoundingBox { min, max })
    }

    pub fn translate(&self, v: Vector) -> BoundingBox {
        BoundingBox {
            min: self.min + v,
            max: self.max + v,
        }
    }

    /// Squared distance from `p` to the box (0 inside), exact.
    pub fn distance_sq_to_point(&self, p: Point) -> i128 {
        let dx = (self.min.x - p.x).max(0).max(p.x - self.max.x) as i128;
        let dy = (self.min.y - p.y).max(0).max(p.y - self.max.y) as i128;
        dx * dx + dy * dy
    }
}

impl Bounded for BoundingBox {
    fn bounding_box(&self) -> BoundingBox {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x0: i64, y0: i64, x1: i64, y1: i64) -> BoundingBox {
        BoundingBox::new(Point::new(x0, y0), Point::new(x1, y1))
    }

    #[test]
    fn normalizes_corners() {
        assert_eq!(b(5, 5, 0, 0), b(0, 0, 5, 5));
    }

    #[test]
    fn touching_boxes_intersect_disjoint_do_not() {
        assert!(b(0, 0, 1, 1).intersects(&b(1, 1, 2, 2)));
        assert!(!b(0, 0, 1, 1).intersects(&b(2, 0, 3, 1)));
    }

    #[test]
    fn inflate_and_collapse() {
        assert_eq!(b(0, 0, 10, 10).inflate(2), Some(b(-2, -2, 12, 12)));
        assert_eq!(b(0, 0, 10, 10).inflate(-6), None);
    }

    #[test]
    fn locate_and_distance() {
        let r = b(0, 0, 10, 10);
        assert_eq!(r.locate(Point::new(5, 5)), PointLocation::Inside);
        assert_eq!(r.locate(Point::new(10, 5)), PointLocation::Boundary);
        assert_eq!(r.locate(Point::new(11, 5)), PointLocation::Outside);
        assert_eq!(r.distance_sq_to_point(Point::new(13, 14)), 25);
    }
}
