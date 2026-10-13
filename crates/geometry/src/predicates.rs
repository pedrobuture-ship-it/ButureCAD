//! Exact integer predicates.

use crate::point::Point;
use serde::{Deserialize, Serialize};

/// Which side of a directed line a point lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    On,
    Right,
}

/// Where a point lies relative to a closed region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointLocation {
    Inside,
    Boundary,
    Outside,
}

impl PointLocation {
    /// Inside or on the boundary.
    pub fn is_contained(self) -> bool {
        self != PointLocation::Outside
    }
}

/// Exact orientation of `c` relative to the directed line `a → b`.
pub fn orient(a: Point, b: Point, c: Point) -> Side {
    match (b - a).cross(c - a).signum() {
        1 => Side::Left,
        -1 => Side::Right,
        _ => Side::On,
    }
}

/// Rounded integer division `num / den` for `den > 0` (halves round up).
pub(crate) fn round_div(num: i128, den: i128) -> i128 {
    (2 * num + den).div_euclid(2 * den)
}

/// Rounds an `f64` to the nearest coordinate (saturating; NaN → 0).
pub(crate) fn round_f64(v: f64) -> i64 {
    v.round() as i64
}
