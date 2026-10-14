//! Integer-coordinate geometry engine for ButureCAD.
//!
//! # Coordinate system
//!
//! * Internal unit: **1 nanometre** (`Coord = i64`). Conversions to mm / mil / inch happen only
//!   at the boundary (see [`units`]). 1 mil = 25 400 nm and 1 inch = 25 400 000 nm exactly, so
//!   integer mil/µm/mm values convert without any loss.
//! * Axes: +X to the right, +Y **up** (mathematical orientation). Angles are counter-clockwise.
//! * Valid range: `|coord| <= MAX_COORD = 2^30 nm` (about ±1.07 m). Inside this range every
//!   predicate (orientation, containment, collision, `within_distance`) is evaluated with exact
//!   `i128` integer arithmetic and cannot overflow. Constructors that accept raw values never
//!   panic; callers should validate user input with [`Point::try_new`].
//! * Results that are not representable (an intersection point, a rotated vertex, an offset)
//!   are rounded to the nearest nanometre, and this is stated on each function. Rotations should
//!   always be applied to the original geometry, never accumulated, to avoid drift.
//!
//! # Exact vs. numerical
//!
//! Yes/no questions (intersects, contains, collides, `within_distance`) are exact. Real-valued
//! results (`distance`, `length`) are `f64` computed from exact integer intermediates.
//! Anything numerical beyond that is called out as such in its documentation.
//!
//! # Spatial indexing
//!
//! [`index::SpatialIndex`] is an R-tree (STR bulk load + dynamic insert/remove). Rationale: PCB
//! objects are rectangles of very uneven size, and the editor mutates the design continuously,
//! so a dynamic bounding-box hierarchy beats a uniform grid (degenerates on mixed object sizes)
//! and a BVH rebuilt on change. The implementation is delegated to the maintained `rstar`
//! crate; only bounding-box queries are used, with exact filtering left to the geometry types.

pub mod arc;
pub mod bbox;
pub mod circle;
pub mod error;
pub mod hull;
pub mod index;
pub mod line;
pub mod point;
pub mod polygon;
pub mod predicates;
pub mod rectangle;
pub mod units;

pub use arc::Arc;
pub use bbox::{Bounded, BoundingBox};
pub use circle::Circle;
pub use error::GeometryError;
pub use hull::convex_hull;
pub use index::SpatialIndex;
pub use line::{Line, LineIntersection, Segment, SegmentIntersection};
pub use point::{Angle, Point, Vector};
pub use polygon::{Orientation, Polygon};
pub use predicates::{orient, PointLocation, Side};
pub use rectangle::Rectangle;
pub use units::{Coord, MAX_COORD};
