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
//! * [`shove`] — displaces existing tracks of other nets so a new route fits, then re-verifies the
//!   result with the DRC rules;
//! * [`tuning`] — length tuning: serpentine/accordion meanders that add an exact length;
//! * [`validate`] — the DRC check of the finished route (the router never trusts its own
//!   approximation: what it returns is re-checked with the same rules as the DRC panel).
//!
//! Status: **Walk Around** (roadmap 21) and **Push & Shove** ([`shove`], roadmap 22) are implemented; results between
//! obstacles are straight segments at any angle (no 45° clean-up yet).

pub mod collision;
pub mod diffpair;
pub mod obstacle;
pub mod pathfind;
pub mod shove;
pub mod tuning;
pub mod validate;
pub mod walkaround;

pub use obstacle::{ObstacleSet, RouteParams};
pub use shove::{shove, NewCopper, ShoveError, ShoveResult};
pub use tuning::{plan_meander, TuneError, TuneParams, TunePlan, TuneStyle};
pub use validate::{validate_pair, validate_replacement, validate_route, RouteCandidate};
pub use walkaround::{route_leg, walk_around, RouteError};
