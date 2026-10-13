#![allow(clippy::unwrap_used)]

use buturecad_geometry::units::{from_mm, to_mm};
use buturecad_geometry::*;
use proptest::prelude::*;

const R: i64 = 1_000_000; // coordinates in ±1 mm keep brute-force checks cheap and dense

fn pt() -> impl Strategy<Value = Point> {
    (-R..=R, -R..=R).prop_map(|(x, y)| Point::new(x, y))
}
fn small_pt() -> impl Strategy<Value = Point> {
    (-20i64..=20, -20i64..=20).prop_map(|(x, y)| Point::new(x, y))
}
fn seg() -> impl Strategy<Value = Segment> {
    (pt(), pt()).prop_map(|(a, b)| Segment::new(a, b))
}
fn small_seg() -> impl Strategy<Value = Segment> {
    (small_pt(), small_pt()).prop_map(|(a, b)| Segment::new(a, b))
}

proptest! {
    #[test]
    fn segment_intersection_is_symmetric(a in seg(), b in seg()) {
        prop_assert_eq!(a.intersects(&b), b.intersects(&a));
    }

    #[test]
    fn intersection_point_lies_on_both_segments(a in seg(), b in seg()) {
        if let SegmentIntersection::Point(p) = a.intersection(&b) {
            // Rounded to the nearest nm, so allow ~1 nm of slack.
            prop_assert!(a.distance_to_point(p) <= 1.5);
            prop_assert!(b.distance_to_point(p) <= 1.5);
        }
    }

    /// Cross-check on a tiny grid: the exact predicate must agree with the numeric distance
    /// (disjoint segments on an integer grid are clearly above zero apart).
    #[test]
    fn intersects_matches_zero_distance(a in small_seg(), b in small_seg()) {
        prop_assert_eq!(a.intersects(&b), a.distance_to_segment(&b) < 1e-9);
    }

    #[test]
    fn within_distance_agrees_with_float_distance(s in seg(), p in pt(), d in 0i64..R) {
        let dist = s.distance_to_point(p);
        // Only assert away from the exact boundary, where f64 rounding is meaningful.
        if (dist - d as f64).abs() > 1e-3 {
            prop_assert_eq!(s.within_distance_of_point(p, d), dist <= d as f64);
        }
    }

    #[test]
    fn nearest_point_is_at_least_as_close_as_endpoints(s in seg(), p in pt()) {
        let q = s.nearest_point(p);
        prop_assert!(s.distance_to_point(q) <= 1.0);
        prop_assert!(p.distance(q) <= p.distance(s.a).min(p.distance(s.b)) + 1.0);
    }

    #[test]
    fn distance_between_segments_is_symmetric(a in seg(), b in seg()) {
        prop_assert!((a.distance_to_segment(&b) - b.distance_to_segment(&a)).abs() < 1e-6);
    }

    #[test]
    fn four_quarter_turns_are_identity(p in pt(), c in pt()) {
        let mut q = p;
        for _ in 0..4 { q = q.rotate_quarter_turns(c, 1); }
        prop_assert_eq!(q, p);
    }

    #[test]
    fn rotation_preserves_distance_within_rounding(p in pt(), c in pt(), deg in 0.0f64..360.0) {
        let a = Angle::from_degrees(deg).unwrap();
        let q = p.rotate_around(c, a);
        prop_assert!((q.distance(c) - p.distance(c)).abs() <= 1.5);
    }

    #[test]
    fn rotation_roundtrip_error_is_bounded(p in pt(), deg in 0.0f64..360.0) {
        let a = Angle::from_degrees(deg).unwrap();
        let back = p.rotate_around(Point::ORIGIN, a).rotate_around(Point::ORIGIN, Angle::from_degrees(-deg).unwrap());
        prop_assert!(back.distance(p) <= 2.0);
    }

    #[test]
    fn translation_preserves_area_and_collision(
        x0 in -50i64..50, y0 in -50i64..50, w in 1i64..40, h in 1i64..40,
        dx in -100i64..100, dy in -100i64..100,
    ) {
        let a = Rectangle::new(Point::new(x0, y0), Point::new(x0 + w, y0 + h)).unwrap().to_polygon();
        let b = Rectangle::new(Point::new(0, 0), Point::new(30, 30)).unwrap().to_polygon();
        let v = Vector::new(dx, dy);
        prop_assert_eq!(a.translate(v).signed_area2(), a.signed_area2());
        prop_assert_eq!(a.translate(v).collides(&b.translate(v)), a.collides(&b));
    }

    #[test]
    fn rect_polygon_agrees_with_rect(
        x0 in -50i64..50, y0 in -50i64..50, w in 1i64..40, h in 1i64..40, p in small_pt(),
    ) {
        let r = Rectangle::new(Point::new(x0, y0), Point::new(x0 + w, y0 + h)).unwrap();
        prop_assert_eq!(r.to_polygon().locate(p), r.locate(p));
    }

    #[test]
    fn polygon_collision_matches_rect_collision(
        a in (-30i64..30, -30i64..30, 1i64..30, 1i64..30),
        b in (-30i64..30, -30i64..30, 1i64..30, 1i64..30),
    ) {
        let ra = Rectangle::new(Point::new(a.0, a.1), Point::new(a.0 + a.2, a.1 + a.3)).unwrap();
        let rb = Rectangle::new(Point::new(b.0, b.1), Point::new(b.0 + b.2, b.1 + b.3)).unwrap();
        prop_assert_eq!(ra.to_polygon().collides(&rb.to_polygon()), ra.collides(&rb));
    }

    #[test]
    fn bounding_box_contains_all_vertices(pts in proptest::collection::vec(pt(), 3..8)) {
        if let Ok(poly) = Polygon::new(pts) {
            let bb = poly.bounding_box();
            prop_assert!(poly.vertices().iter().all(|v| bb.contains_point(*v)));
        }
    }

    #[test]
    fn convex_offset_grows_area(
        x0 in -50i64..50, y0 in -50i64..50, w in 20i64..80, h in 20i64..80, d in 1i64..10,
    ) {
        let poly = Rectangle::new(Point::new(x0, y0), Point::new(x0 + w, y0 + h)).unwrap().to_polygon();
        let grown = poly.offset(d).unwrap();
        prop_assert_eq!(grown.signed_area2(), ((w + 2 * d) as i128 * (h + 2 * d) as i128) * 2);
        prop_assert!(grown.collides(&poly));
    }

    #[test]
    fn circle_collision_matches_distance(
        c0 in small_pt(), r0 in 0i64..30, c1 in small_pt(), r1 in 0i64..30,
    ) {
        let a = Circle::new(c0, r0).unwrap();
        let b = Circle::new(c1, r1).unwrap();
        prop_assert_eq!(a.collides_circle(&b), c0.distance(c1) <= (r0 + r1) as f64 + 1e-9);
    }

    #[test]
    fn mm_roundtrip(v in -500.0f64..500.0) {
        let c = from_mm(v).unwrap();
        prop_assert!((to_mm(c) - v).abs() <= 0.5e-6 + 1e-12);
    }

    #[test]
    fn index_matches_brute_force(
        boxes in proptest::collection::vec((pt(), pt()), 0..60),
        q in (pt(), pt()),
    ) {
        let items: Vec<(BoundingBox, usize)> = boxes.iter().enumerate()
            .map(|(i, (a, b))| (BoundingBox::new(*a, *b), i)).collect();
        let bulk = SpatialIndex::bulk_load(items.clone());
        let mut dynamic = SpatialIndex::new();
        for (b, i) in &items { dynamic.insert(*b, *i); }
        let area = BoundingBox::new(q.0, q.1);
        let mut expected: Vec<usize> = items.iter().filter(|(b, _)| b.intersects(&area)).map(|(_, i)| *i).collect();
        expected.sort_unstable();
        for idx in [&bulk, &dynamic] {
            let mut got: Vec<usize> = idx.query_intersecting(&area).map(|(_, i)| *i).collect();
            got.sort_unstable();
            prop_assert_eq!(&got, &expected);
        }
        // Remove half and compare again.
        for (b, i) in items.iter().filter(|(_, i)| i % 2 == 0) {
            prop_assert!(dynamic.remove(b, *i));
        }
        let mut got: Vec<usize> = dynamic.query_intersecting(&area).map(|(_, i)| *i).collect();
        got.sort_unstable();
        expected.retain(|i| i % 2 == 1);
        prop_assert_eq!(got, expected);
    }
}
