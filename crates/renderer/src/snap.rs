//! Snapping to objects and the grid.

use crate::grid::snap_to_grid;
use crate::scene::{ObjectId, Scene, Shape};
use crate::view::View;
use buturecad_geometry::{BoundingBox, Coord, Point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapSettings {
    pub grid: bool,
    pub objects: bool,
    /// Capture radius for object snap, in pixels.
    pub radius_px: f64,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            grid: true,
            objects: true,
            radius_px: 10.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    None,
    Grid,
    /// Snapped to a characteristic point (end point, centre, corner, vertex) of an object.
    Object(ObjectId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapResult {
    pub point: Point,
    pub kind: SnapKind,
}

/// Characteristic points of a shape.
pub fn snap_points(shape: &Shape) -> Vec<Point> {
    match shape {
        Shape::Line { segment, .. } => vec![segment.a, segment.b],
        Shape::Arc { arc, .. } => vec![arc.start, arc.end, arc.center],
        Shape::Circle { circle, .. } => vec![circle.center],
        Shape::Rect { rect, .. } => {
            let (a, b) = (rect.min(), rect.max());
            vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]
        }
        Shape::Polygon { polygon, .. } => polygon.vertices().to_vec(),
        Shape::Text { origin, .. } => vec![*origin],
    }
}

/// Snaps the cursor at screen `(sx, sy)`. Object points win over the grid when within the
/// capture radius; otherwise the grid node (if enabled); otherwise the raw world point.
pub fn snap(
    scene: &Scene,
    view: &View,
    settings: &SnapSettings,
    grid_spacing: Coord,
    sx: f64,
    sy: f64,
) -> SnapResult {
    let raw = view.screen_to_world(sx, sy);
    if settings.objects {
        let reach = view.px_to_nm(settings.radius_px);
        let area = BoundingBox::from_point(raw)
            .inflate(reach.ceil() as Coord)
            .unwrap_or(BoundingBox::from_point(raw));
        let mut best: Option<(f64, Point, ObjectId)> = None;
        for id in scene.query(&area) {
            let Some(item) = scene.get(id) else { continue };
            if !scene.layer(item.layer).is_some_and(|l| l.snappable) {
                continue;
            }
            for p in snap_points(&item.shape) {
                let d = raw.distance(p);
                if d <= reach && best.is_none_or(|(bd, _, bid)| d < bd || (d == bd && id < bid)) {
                    best = Some((d, p, id));
                }
            }
        }
        if let Some((_, point, id)) = best {
            return SnapResult {
                point,
                kind: SnapKind::Object(id),
            };
        }
    }
    if settings.grid {
        return SnapResult {
            point: snap_to_grid(raw, grid_spacing),
            kind: SnapKind::Grid,
        };
    }
    SnapResult {
        point: raw,
        kind: SnapKind::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Item, Layer, LayerId, Rgba};
    use buturecad_geometry::Segment;

    fn setup() -> (Scene, View) {
        let mut s = Scene::new();
        s.set_layer(LayerId(1), Layer::new("L", Rgba::rgb(0, 0, 0)));
        let seg = Segment::new(Point::new(1234, 5678), Point::new(100_000, 5678));
        s.insert(
            ObjectId(7),
            Item {
                layer: LayerId(1),
                shape: Shape::Line {
                    segment: seg,
                    width: 0,
                },
                color: None,
            },
        );
        (
            s,
            View {
                center: (0.0, 0.0),
                scale: 1.0e-3,
                width: 800.0,
                height: 600.0,
            },
        )
    }

    #[test]
    fn object_snap_beats_grid() {
        let (s, v) = setup();
        let (sx, sy) = v.world_to_screen(Point::new(1234, 5678));
        let r = snap(&s, &v, &SnapSettings::default(), 10_000, sx + 3.0, sy + 2.0);
        assert_eq!(
            r,
            SnapResult {
                point: Point::new(1234, 5678),
                kind: SnapKind::Object(ObjectId(7))
            }
        );
    }

    #[test]
    fn falls_back_to_grid_then_raw() {
        let (s, v) = setup();
        let (sx, sy) = v.world_to_screen(Point::new(-300_000, 200_000));
        let r = snap(&s, &v, &SnapSettings::default(), 10_000, sx, sy);
        assert_eq!(r.kind, SnapKind::Grid);
        assert_eq!(r.point, Point::new(-300_000, 200_000));
        let off = SnapSettings {
            grid: false,
            objects: false,
            radius_px: 10.0,
        };
        assert_eq!(snap(&s, &v, &off, 10_000, sx, sy).kind, SnapKind::None);
    }

    #[test]
    fn outside_capture_radius_does_not_snap_to_objects() {
        let (s, v) = setup();
        let (sx, sy) = v.world_to_screen(Point::new(1234, 5678));
        let r = snap(&s, &v, &SnapSettings::default(), 10_000, sx + 30.0, sy);
        assert_eq!(r.kind, SnapKind::Grid);
    }
}
