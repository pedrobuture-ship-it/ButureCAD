#![allow(clippy::unwrap_used)]

use buturecad_geometry::{Point, Polygon};
use buturecad_renderer::tessellate::triangulate;
use proptest::prelude::*;

proptest! {
    /// Star-shaped polygons (vertices sorted by angle around the origin) are always simple, so
    /// the triangulation must cover exactly their area.
    #[test]
    fn ear_clipping_covers_exact_area(radii in proptest::collection::vec(100i64..10_000, 3..24)) {
        let n = radii.len();
        let pts: Vec<Point> = radii.iter().enumerate().map(|(i, r)| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new((*r as f64 * a.cos()).round() as i64, (*r as f64 * a.sin()).round() as i64)
        }).collect();
        if let Ok(poly) = Polygon::new(pts) {
            if poly.is_simple() {
                let poly = poly.into_ccw();
                let tris = triangulate(poly.vertices());
                let v = poly.vertices();
                let area2: i128 = tris.iter().map(|t| (v[t[1]] - v[t[0]]).cross(v[t[2]] - v[t[0]])).sum();
                prop_assert_eq!(area2, poly.signed_area2());
            }
        }
    }
}
