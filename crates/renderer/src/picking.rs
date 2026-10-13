//! Picking, hover, selection and box selection. Pure CPU, spatial-index accelerated.

use crate::scene::{Item, ObjectId, Scene, Shape};
use crate::view::View;
use buturecad_geometry::{Bounded, BoundingBox, Coord, Point, PointLocation};
use std::collections::BTreeSet;

/// Default pick tolerance in pixels.
pub const PICK_TOLERANCE_PX: f64 = 4.0;

/// Distance (nm, f64) from `p` to the drawn shape; 0 when `p` is on it. Filled shapes are hit
/// anywhere inside.
pub fn distance_to_shape(shape: &Shape, p: Point) -> f64 {
    let outline = |d_boundary: f64, stroke: Coord| (d_boundary - stroke as f64 / 2.0).max(0.0);
    match shape {
        Shape::Line { segment, width } => outline(segment.distance_to_point(p), *width),
        Shape::Arc { arc, width } => outline(arc.distance_to_point(p), *width),
        Shape::Circle {
            circle,
            filled,
            stroke,
        } => {
            if *filled && circle.contains_point(p) {
                0.0
            } else {
                outline(
                    (circle.center.distance(p) - circle.radius as f64).abs(),
                    *stroke,
                )
            }
        }
        Shape::Rect {
            rect,
            filled,
            stroke,
        } => {
            if *filled && rect.contains_point(p) {
                return 0.0;
            }
            let polygon = rect.to_polygon();
            let d = polygon
                .edges()
                .map(|e| e.distance_to_point(p))
                .fold(f64::INFINITY, f64::min);
            outline(d, *stroke)
        }
        Shape::Polygon {
            polygon,
            filled,
            stroke,
        } => {
            if *filled && polygon.locate(p) != PointLocation::Outside {
                return 0.0;
            }
            let d = polygon
                .edges()
                .map(|e| e.distance_to_point(p))
                .fold(f64::INFINITY, f64::min);
            outline(d, *stroke)
        }
        Shape::Text { .. } => {
            let bb = shape.bounding_box();
            (bb.distance_sq_to_point(p) as f64).sqrt()
        }
    }
}

fn bbox_area(bb: &BoundingBox) -> i128 {
    bb.width() as i128 * bb.height() as i128
}

/// All objects within `tolerance_px` of the screen point, best first.
///
/// Order: exact hits before near misses, then smaller objects before larger ones (so a pad on
/// top of a zone is picked first), then topmost layer, then highest id.
pub fn pick_all(scene: &Scene, view: &View, sx: f64, sy: f64, tolerance_px: f64) -> Vec<ObjectId> {
    let p = view.screen_to_world(sx, sy);
    let tol = view.px_to_nm(tolerance_px);
    let reach = tol.ceil() as Coord;
    let area = BoundingBox::from_point(p)
        .inflate(reach)
        .unwrap_or(BoundingBox::from_point(p));
    let mut hits: Vec<(bool, i128, u16, ObjectId)> = scene
        .query(&area)
        .filter_map(|id| {
            let item = scene.get(id)?;
            let d = distance_to_shape(&item.shape, p);
            if d > tol {
                return None;
            }
            let area = bbox_area(&scene.bounding_box_of(id)?);
            Some((d > 0.0, area, item.layer.0, id))
        })
        .collect();
    hits.sort_by(|a, b| {
        (a.0, a.1, std::cmp::Reverse(a.2), std::cmp::Reverse(a.3)).cmp(&(
            b.0,
            b.1,
            std::cmp::Reverse(b.2),
            std::cmp::Reverse(b.3),
        ))
    });
    hits.into_iter().map(|h| h.3).collect()
}

/// Best object under the cursor.
pub fn pick(scene: &Scene, view: &View, sx: f64, sy: f64, tolerance_px: f64) -> Option<ObjectId> {
    pick_all(scene, view, sx, sy, tolerance_px)
        .into_iter()
        .next()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxMode {
    /// Objects entirely inside the box (drag left → right).
    Window,
    /// Objects touching the box (drag right → left).
    Crossing,
}

impl BoxMode {
    /// Window when dragging to the right, crossing when dragging to the left.
    pub fn from_drag(start_x: f64, end_x: f64) -> Self {
        if end_x >= start_x {
            BoxMode::Window
        } else {
            BoxMode::Crossing
        }
    }
}

fn shape_touches_box(item: &Item, area: &BoundingBox) -> bool {
    use buturecad_geometry::Rectangle;
    let Ok(rect) = Rectangle::new(area.min, area.max) else {
        // Degenerate (zero-area) box: fall back to bounding-box overlap.
        return item.shape.bounding_box().intersects(area);
    };
    let poly = rect.to_polygon();
    match &item.shape {
        Shape::Line { segment, width } => {
            poly.collides_segment(segment)
                || segment.within_distance_of_point(area.center(), 0)
                || {
                    // Stroke width: compare against the inflated box.
                    let w = width / 2;
                    w > 0
                        && area.inflate(w).is_some_and(|b| {
                            Rectangle::new(b.min, b.max)
                                .is_ok_and(|r| r.to_polygon().collides_segment(segment))
                        })
                }
        }
        Shape::Circle { circle, filled, .. } => {
            let d = rect.distance_to_point(circle.center);
            if *filled {
                d <= circle.radius as f64
            } else {
                // Touches the outline: box reaches the ring.
                let far = [
                    rect.min(),
                    rect.max(),
                    Point::new(rect.min().x, rect.max().y),
                    Point::new(rect.max().x, rect.min().y),
                ]
                .iter()
                .map(|c| c.distance(circle.center))
                .fold(0.0, f64::max);
                d <= circle.radius as f64 && far >= circle.radius as f64
            }
        }
        Shape::Rect {
            rect: r, filled, ..
        } => {
            if *filled {
                r.collides(&rect)
            } else {
                r.to_polygon().edges().any(|e| poly.collides_segment(&e))
                    && !(rect_inside(&rect, r))
            }
        }
        Shape::Polygon {
            polygon, filled, ..
        } => {
            if *filled {
                polygon.collides(&poly)
            } else {
                polygon.edges().any(|e| poly.collides_segment(&e))
            }
        }
        // Arcs and text: conservative bounding-box test (documented in `box_select`).
        Shape::Arc { .. } | Shape::Text { .. } => item.shape.bounding_box().intersects(area),
    }
}

fn rect_inside(
    inner: &buturecad_geometry::Rectangle,
    outer: &buturecad_geometry::Rectangle,
) -> bool {
    outer.bounding_box().contains_box(&inner.bounding_box())
}

/// Objects selected by a world-space box.
///
/// `Window` is exact (bounding box containment). `Crossing` is exact for lines, circles,
/// rectangles and polygons; arcs and text use their bounding box (may over-select near them).
pub fn box_select(scene: &Scene, area: &BoundingBox, mode: BoxMode) -> Vec<ObjectId> {
    let mut out: Vec<ObjectId> = scene
        .query(area)
        .filter(|id| match (mode, scene.get(*id)) {
            (_, None) => false,
            (BoxMode::Window, Some(_)) => scene
                .bounding_box_of(*id)
                .is_some_and(|b| area.contains_box(&b)),
            (BoxMode::Crossing, Some(item)) => shape_touches_box(item, area),
        })
        .collect();
    out.sort();
    out
}

/// Selection and hover state.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Selection {
    selected: BTreeSet<ObjectId>,
    pub hover: Option<ObjectId>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn selected(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.selected.iter().copied()
    }
    pub fn contains(&self, id: ObjectId) -> bool {
        self.selected.contains(&id)
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    pub fn clear(&mut self) {
        self.selected.clear();
    }
    pub fn set(&mut self, ids: impl IntoIterator<Item = ObjectId>) {
        self.selected = ids.into_iter().collect();
    }
    pub fn add(&mut self, ids: impl IntoIterator<Item = ObjectId>) {
        self.selected.extend(ids);
    }
    pub fn toggle(&mut self, id: ObjectId) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
    }
    /// Drops ids that no longer exist in the scene.
    pub fn retain_existing(&mut self, scene: &Scene) {
        self.selected.retain(|id| scene.get(*id).is_some());
        if self.hover.is_some_and(|h| scene.get(h).is_none()) {
            self.hover = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::scene::{Layer, LayerId, Rgba};
    use buturecad_geometry::{Circle, Polygon, Rectangle, Segment};

    const L: LayerId = LayerId(1);

    fn scene() -> Scene {
        let mut s = Scene::new();
        s.set_layer(L, Layer::new("L", Rgba::rgb(1, 2, 3)));
        s
    }
    fn item(shape: Shape) -> Item {
        Item {
            layer: L,
            shape,
            color: None,
        }
    }
    fn view() -> View {
        // 1 px = 1000 nm, centre at origin, 800x600.
        View {
            center: (0.0, 0.0),
            scale: 1.0e-3,
            width: 800.0,
            height: 600.0,
        }
    }
    fn at(v: &View, x: i64, y: i64) -> (f64, f64) {
        v.world_to_screen(Point::new(x, y))
    }

    #[test]
    fn picks_line_within_pixel_tolerance() {
        let mut s = scene();
        let seg = Segment::new(Point::new(-100_000, 0), Point::new(100_000, 0));
        s.insert(
            ObjectId(1),
            item(Shape::Line {
                segment: seg,
                width: 0,
            }),
        );
        let v = view();
        let (sx, sy) = at(&v, 0, 0);
        assert_eq!(pick(&s, &v, sx, sy + 3.0, 4.0), Some(ObjectId(1)));
        assert_eq!(pick(&s, &v, sx, sy + 6.0, 4.0), None);
    }

    #[test]
    fn exact_hit_beats_near_miss_and_small_beats_large() {
        let mut s = scene();
        let big = Rectangle::new(Point::new(-50_000, -50_000), Point::new(50_000, 50_000)).unwrap();
        s.insert(
            ObjectId(1),
            item(Shape::Rect {
                rect: big,
                filled: true,
                stroke: 0,
            }),
        );
        let small = Circle::new(Point::new(0, 0), 5_000).unwrap();
        s.insert(
            ObjectId(2),
            item(Shape::Circle {
                circle: small,
                filled: true,
                stroke: 0,
            }),
        );
        let v = view();
        let (sx, sy) = at(&v, 0, 0);
        assert_eq!(
            pick_all(&s, &v, sx, sy, 4.0),
            vec![ObjectId(2), ObjectId(1)]
        );
    }

    #[test]
    fn hidden_layers_are_not_pickable() {
        let mut s = scene();
        let c = Circle::new(Point::ORIGIN, 5_000).unwrap();
        s.insert(
            ObjectId(1),
            item(Shape::Circle {
                circle: c,
                filled: true,
                stroke: 0,
            }),
        );
        s.set_layer_visible(L, false);
        let v = view();
        let (sx, sy) = at(&v, 0, 0);
        assert_eq!(pick(&s, &v, sx, sy, 4.0), None);
    }

    #[test]
    fn unfilled_shapes_are_hit_on_the_outline_only() {
        let mut s = scene();
        let c = Circle::new(Point::ORIGIN, 50_000).unwrap();
        s.insert(
            ObjectId(1),
            item(Shape::Circle {
                circle: c,
                filled: false,
                stroke: 2_000,
            }),
        );
        let v = view();
        let (cx, cy) = at(&v, 0, 0);
        assert_eq!(pick(&s, &v, cx, cy, 4.0), None, "centre of a ring");
        let (ox, oy) = at(&v, 50_000, 0);
        assert_eq!(pick(&s, &v, ox, oy, 4.0), Some(ObjectId(1)));
    }

    #[test]
    fn window_vs_crossing() {
        let mut s = scene();
        s.insert(
            ObjectId(1),
            item(Shape::Line {
                segment: Segment::new(Point::new(0, 0), Point::new(10, 0)),
                width: 0,
            }),
        );
        s.insert(
            ObjectId(2),
            item(Shape::Line {
                segment: Segment::new(Point::new(5, -5), Point::new(500, -5)),
                width: 0,
            }),
        );
        let poly = Polygon::new(vec![
            Point::new(100, 100),
            Point::new(200, 100),
            Point::new(150, 200),
        ])
        .unwrap();
        s.insert(
            ObjectId(3),
            item(Shape::Polygon {
                polygon: poly,
                filled: true,
                stroke: 0,
            }),
        );
        let area = BoundingBox::new(Point::new(-20, -20), Point::new(20, 20));
        assert_eq!(box_select(&s, &area, BoxMode::Window), vec![ObjectId(1)]);
        assert_eq!(
            box_select(&s, &area, BoxMode::Crossing),
            vec![ObjectId(1), ObjectId(2)]
        );
        let big = BoundingBox::new(Point::new(0, 0), Point::new(1000, 1000));
        assert!(box_select(&s, &big, BoxMode::Crossing).contains(&ObjectId(3)));
        assert_eq!(BoxMode::from_drag(10.0, 50.0), BoxMode::Window);
        assert_eq!(BoxMode::from_drag(50.0, 10.0), BoxMode::Crossing);
    }

    #[test]
    fn crossing_touches_without_vertex_inside() {
        let mut s = scene();
        // A long line passing through the box without endpoints inside it.
        s.insert(
            ObjectId(1),
            item(Shape::Line {
                segment: Segment::new(Point::new(-1000, 0), Point::new(1000, 0)),
                width: 0,
            }),
        );
        let area = BoundingBox::new(Point::new(-10, -10), Point::new(10, 10));
        assert_eq!(box_select(&s, &area, BoxMode::Crossing), vec![ObjectId(1)]);
        assert!(box_select(&s, &area, BoxMode::Window).is_empty());
    }

    #[test]
    fn selection_ops() {
        let mut sel = Selection::new();
        sel.add([ObjectId(1), ObjectId(2)]);
        sel.toggle(ObjectId(2));
        sel.toggle(ObjectId(3));
        assert_eq!(
            sel.selected().collect::<Vec<_>>(),
            vec![ObjectId(1), ObjectId(3)]
        );
        let s = scene();
        sel.hover = Some(ObjectId(1));
        sel.retain_existing(&s);
        assert!(sel.is_empty() && sel.hover.is_none());
    }
}
