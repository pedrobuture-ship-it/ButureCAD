//! Interactive PCB routing engine.
//!
//! Layers, kept separate on purpose (roadmap 21):
//!
//! * [`collision`] — exact predicates: does a segment pass through the interior of a convex
//!   obstacle, is a point inside one (integer arithmetic, no tolerance);
//! * [`obstacle`] — the copper, keepouts and board edge a route of a given net/width must avoid,
//!   each *inflated* by clearance + half the track width into a convex polygon;
//! * [`pathfind`] — A* over the visibility graph of the obstacle corners;
//! * [`walkaround`] — the public routing entry points that combine them;
//! * [`validate`] — the DRC check of the finished route (the router never trusts its own
//!   approximation: what it returns is re-checked with the same rules as the DRC panel).
//!
//! Status: **Walk Around** is implemented. Push & Shove is roadmap stage 22; results between
//! obstacles are straight segments at any angle (no 45° clean-up yet).

pub mod collision;
pub mod obstacle;
pub mod pathfind;
pub mod validate;
pub mod walkaround;

pub use obstacle::{ObstacleSet, RouteParams};
pub use validate::{validate_route, RouteCandidate};
pub use walkaround::{route_leg, walk_around, RouteError};
