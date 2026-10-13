//! Internal unit (nanometre) and boundary conversions.

use crate::error::GeometryError;

/// Internal coordinate: nanometres.
pub type Coord = i64;

pub const NM_PER_UM: Coord = 1_000;
pub const NM_PER_MM: Coord = 1_000_000;
pub const NM_PER_MIL: Coord = 25_400;
pub const NM_PER_INCH: Coord = 25_400_000;

/// Largest supported absolute coordinate (2^30 nm ≈ 1.07 m).
pub const MAX_COORD: Coord = 1 << 30;

fn from_unit(value: f64, nm_per_unit: Coord) -> Result<Coord, GeometryError> {
    if !value.is_finite() {
        return Err(GeometryError::NotFinite);
    }
    let nm = (value * nm_per_unit as f64).round();
    if nm.abs() > MAX_COORD as f64 {
        return Err(GeometryError::OutOfRange);
    }
    Ok(nm as Coord)
}

/// Millimetres → nm, rounded to the nearest nanometre. Errors on NaN/∞/out-of-range.
pub fn from_mm(v: f64) -> Result<Coord, GeometryError> {
    from_unit(v, NM_PER_MM)
}
/// Mils (1/1000 inch) → nm, rounded to the nearest nanometre.
pub fn from_mil(v: f64) -> Result<Coord, GeometryError> {
    from_unit(v, NM_PER_MIL)
}
/// Inches → nm, rounded to the nearest nanometre.
pub fn from_inch(v: f64) -> Result<Coord, GeometryError> {
    from_unit(v, NM_PER_INCH)
}
pub fn to_mm(c: Coord) -> f64 {
    c as f64 / NM_PER_MM as f64
}
pub fn to_mil(c: Coord) -> f64 {
    c as f64 / NM_PER_MIL as f64
}
pub fn to_inch(c: Coord) -> f64 {
    c as f64 / NM_PER_INCH as f64
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn integer_units_are_exact() {
        assert_eq!(from_mil(1.0).unwrap(), 25_400);
        assert_eq!(from_inch(0.1).unwrap(), 2_540_000);
        assert_eq!(from_mm(0.254).unwrap(), from_mil(10.0).unwrap());
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(from_mm(f64::NAN), Err(GeometryError::NotFinite));
        assert_eq!(from_mm(f64::INFINITY), Err(GeometryError::NotFinite));
        assert_eq!(from_mm(5000.0), Err(GeometryError::OutOfRange));
    }

    #[test]
    fn roundtrip_is_within_half_nm() {
        let c = from_mm(1.234_567_89).unwrap();
        assert!((to_mm(c) - 1.234_567_89).abs() <= 0.5e-6);
    }
}
