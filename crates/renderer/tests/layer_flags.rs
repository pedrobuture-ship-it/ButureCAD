#![allow(clippy::unwrap_used)]

use buturecad_geometry::{Point, Rectangle, Segment};
use buturecad_renderer::tessellate::tessellate_scene;
use buturecad_renderer::*;

fn scene() -> Scene {
    let mut s = Scene::new();
    s.set_layer(LayerId(1), Layer::new("hit", Rgba::rgb(1, 1, 1)).hit_only());
    s.set_layer(
        LayerId(2),
        Layer::new("lines", Rgba::rgb(2, 2, 2)).not_snappable(),
    );
    let r = Rectangle::new(Point::new(-10_000, -10_000), Point::new(10_000, 10_000)).unwrap();
    s.insert(
        ObjectId(1),
        Item {
            layer: LayerId(1),
            shape: Shape::Rect {
                rect: r,
                filled: true,
                stroke: 0,
            },
            color: None,
        },
    );
    let seg = Segment::new(Point::new(50_000, 0), Point::new(60_000, 0));
    s.insert(
        ObjectId(2),
        Item {
            layer: LayerId(2),
            shape: Shape::Line {
                segment: seg,
                width: 0,
            },
            color: None,
        },
    );
    s
}

#[test]
fn hit_only_layers_are_pickable_but_never_drawn() {
    let s = scene();
    let v = View {
        center: (0.0, 0.0),
        scale: 1.0e-3,
        width: 400.0,
        height: 300.0,
    };
    let (sx, sy) = v.world_to_screen(Point::ORIGIN);
    assert_eq!(pick(&s, &v, sx, sy, 4.0), Some(ObjectId(1)));
    let mesh = tessellate_scene(&s, 1.0e-3, (0.0, 0.0));
    assert!(
        mesh.vertices.iter().all(|vtx| vtx.color[0] != 1),
        "the hit layer produced geometry"
    );
}

#[test]
fn non_snappable_layers_do_not_attract_the_cursor() {
    let s = scene();
    let v = View {
        center: (0.0, 0.0),
        scale: 1.0e-3,
        width: 400.0,
        height: 300.0,
    };
    let (sx, sy) = v.world_to_screen(Point::new(50_000, 0));
    let r = snap(
        &s,
        &v,
        &SnapSettings {
            grid: false,
            objects: true,
            radius_px: 10.0,
        },
        1_000,
        sx + 1.0,
        sy,
    );
    assert_eq!(r.kind, SnapKind::None);
}

#[test]
fn distinct_scenes_never_share_a_revision() {
    // Same number of operations on two scenes: a revision-keyed cache must still tell them apart.
    let build = || {
        let mut s = Scene::new();
        s.set_layer(LayerId(1), Layer::new("a", Rgba::rgb(1, 1, 1)));
        s
    };
    let (a, b) = (build(), build());
    assert_ne!(a.revision(), b.revision());
    let mut c = build();
    let before = c.revision();
    c.set_layer_visible(LayerId(1), false);
    assert!(c.revision() > before);
}
