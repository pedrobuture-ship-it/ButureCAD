//! Walk Around: route a leg, detouring around obstacles instead of through them.

use crate::obstacle::ObstacleSet;
use crate::pathfind::{self, Search};
use buturecad_eda_core::model::Id;
use buturecad_eda_core::routing::{self, AngleMode};
use buturecad_geometry::{BoundingBox, Point};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// The start already violates clearance against this board object (`Id(0)` = board edge or
    /// keepout).
    StartBlocked(Id),
    EndBlocked(Id),
    /// Every way is closed.
    NoPath,
    /// Too many obstacle corners to search interactively.
    TooComplex,
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::StartBlocked(_) => write!(f, "the start is too close to other copper"),
            RouteError::EndBlocked(_) => write!(f, "the target is too close to other copper"),
            RouteError::NoPath => write!(f, "no path around the obstacles"),
            RouteError::TooComplex => write!(f, "too many obstacles to route around here"),
        }
    }
}

impl std::error::Error for RouteError {}

fn corridor(a: Point, b: Point, margin: i64) -> BoundingBox {
    let bb = BoundingBox::new(
        Point::new(a.x.min(b.x), a.y.min(b.y)),
        Point::new(a.x.max(b.x), a.y.max(b.y)),
    );
    bb.inflate(margin).unwrap_or(bb)
}

/// The shortest obstacle-free polyline from `from` to `to`: the straight segment when nothing is
/// in the way, otherwise a path through the corners of the inflated obstacles. Segments between
/// obstacles run at any angle; an inflated obstacle is a circumscribed polygon, so the result
/// keeps at least the required clearance.
pub fn walk_around(set: &ObstacleSet, from: Point, to: Point) -> Result<Vec<Point>, RouteError> {
    if let Some(o) = set.containing(from) {
        return Err(RouteError::StartBlocked(o.owner));
    }
    if let Some(o) = set.containing(to) {
        return Err(RouteError::EndBlocked(o.owner));
    }
    if from == to {
        return Ok(vec![from]);
    }
    if set.blocking(from, to).is_none() {
        return Ok(vec![from, to]);
    }
    let span = from.distance(to) as i64;
    // Widen the corridor only when a narrower one has no way through.
    for margin in [
        (span / 2).max(2_000_000),
        (span * 2).max(10_000_000),
        i64::MAX / 4,
    ] {
        match pathfind::shortest(set, from, to, &corridor(from, to, margin)) {
            Search::Found(p) => return Ok(routing::simplify(&p)),
            Search::TooComplex => return Err(RouteError::TooComplex),
            Search::NoPath => {}
        }
    }
    Err(RouteError::NoPath)
}

/// One leg of an interactive route. The preferred shape is the usual 45°/90°/free elbow; the
/// other elbow is tried next; only when both cross an obstacle does the leg walk around.
pub fn route_leg(
    set: &ObstacleSet,
    from: Point,
    to: Point,
    mode: AngleMode,
    flip: bool,
) -> Result<Vec<Point>, RouteError> {
    if let Some(o) = set.containing(from) {
        return Err(RouteError::StartBlocked(o.owner));
    }
    if let Some(o) = set.containing(to) {
        return Err(RouteError::EndBlocked(o.owner));
    }
    for f in [flip, !flip] {
        let direct = routing::path(from, to, mode, f);
        if set.blocking_path(&direct).is_none() {
            return Ok(direct);
        }
    }
    walk_around(set, from, to)
}
