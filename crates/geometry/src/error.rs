use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum GeometryError {
    #[error("value is not a finite number")]
    NotFinite,
    #[error(
        "coordinate is outside the supported range of ±{} nm",
        crate::units::MAX_COORD
    )]
    OutOfRange,
    #[error("a line needs two distinct points")]
    DegenerateLine,
    #[error("radius must not be negative")]
    NegativeRadius,
    #[error("arc is degenerate (zero radius, coincident ends, or start/end radii differ by more than 1 nm)")]
    DegenerateArc,
    #[error("polygon needs at least 3 distinct, non-collinear vertices")]
    DegeneratePolygon,
    #[error("offset collapses or changes the topology of the shape")]
    OffsetTopology,
    #[error("offset produces a miter that is too sharp")]
    MiterTooSharp,
}
