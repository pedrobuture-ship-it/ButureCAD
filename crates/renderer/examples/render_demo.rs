#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Renders a demo scene to `render_demo.ppm` (dev tool: `cargo run -p buturecad-renderer --example render_demo`).

use buturecad_geometry::{Arc, Circle, Point, Polygon, Rectangle, Segment};
use buturecad_renderer::*;

fn main() {
    let mut renderer = match GpuRenderer::new_blocking() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GPU: {e}");
            std::process::exit(2);
        }
    };
    let mut scene = Scene::new();
    scene.set_layer(
        LayerId(1),
        Layer {
            name: "Bottom".into(),
            color: Rgba::rgb(60, 110, 230),
            visible: true,
        },
    );
    scene.set_layer(
        LayerId(2),
        Layer {
            name: "Top".into(),
            color: Rgba::rgb(230, 60, 60),
            visible: true,
        },
    );
    scene.set_layer(
        LayerId(3),
        Layer {
            name: "Silk".into(),
            color: Rgba::rgb(240, 240, 240),
            visible: true,
        },
    );
    let mm = 1_000_000;
    let pt = |x: i64, y: i64| Point::new(x * mm, y * mm);
    let mut id = 0;
    let mut add = |s: &mut Scene, layer, shape| {
        id += 1;
        s.insert(
            ObjectId(id),
            Item {
                layer: LayerId(layer),
                shape,
                color: None,
            },
        );
    };
    add(
        &mut scene,
        2,
        Shape::Line {
            segment: Segment::new(pt(-30, -10), pt(10, -10)),
            width: 250_000,
        },
    );
    add(
        &mut scene,
        2,
        Shape::Line {
            segment: Segment::new(pt(10, -10), pt(25, 5)),
            width: 250_000,
        },
    );
    add(
        &mut scene,
        1,
        Shape::Line {
            segment: Segment::new(pt(-30, 10), pt(20, 10)),
            width: 400_000,
        },
    );
    add(
        &mut scene,
        2,
        Shape::Circle {
            circle: Circle::new(pt(-30, -10), 800_000).unwrap(),
            filled: true,
            stroke: 0,
        },
    );
    add(
        &mut scene,
        1,
        Shape::Circle {
            circle: Circle::new(pt(0, 0), 6 * mm).unwrap(),
            filled: false,
            stroke: 300_000,
        },
    );
    add(
        &mut scene,
        3,
        Shape::Arc {
            arc: Arc::new(pt(0, 0), pt(10, 0), pt(0, 10), true).unwrap(),
            width: 200_000,
        },
    );
    add(
        &mut scene,
        2,
        Shape::Rect {
            rect: Rectangle::new(pt(14, -4), pt(20, 2)).unwrap(),
            filled: true,
            stroke: 0,
        },
    );
    add(
        &mut scene,
        3,
        Shape::Rect {
            rect: Rectangle::new(pt(-35, -15), pt(30, 15)).unwrap(),
            filled: false,
            stroke: 150_000,
        },
    );
    let poly = Polygon::new(vec![
        pt(-20, -4),
        pt(-10, -4),
        pt(-10, 2),
        pt(-14, 2),
        pt(-14, 6),
        pt(-20, 6),
    ])
    .unwrap();
    add(
        &mut scene,
        1,
        Shape::Polygon {
            polygon: poly,
            filled: true,
            stroke: 0,
        },
    );
    add(
        &mut scene,
        3,
        Shape::Text {
            origin: pt(-34, 11),
            height: 1_400_000,
            text: "BUTURECAD R1 10K 0123".into(),
        },
    );

    let mut view = View::new(900.0, 560.0);
    view.fit(
        &scene
            .bounds()
            .unwrap_or(buturecad_geometry::BoundingBox::from_point(Point::ORIGIN)),
        30.0,
    );
    let mut selection = Selection::new();
    selection.set([ObjectId(5)]);
    selection.hover = Some(ObjectId(1));
    let theme = Theme::dark();
    let grid = GridSettings::new(1_270_000);
    let req = FrameRequest {
        scene: &scene,
        view: &view,
        grid: &grid,
        theme: &theme,
        selection: &selection,
        box_select: Some(((600.0, 380.0), (800.0, 480.0))),
        preview: &[],
        debug: true,
    };
    let frame = renderer.render(&req).expect("render");
    let frame = renderer.render(&req).unwrap_or(frame);
    let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
    for px in frame.rgba.chunks(4) {
        out.extend_from_slice(&px[..3]);
    }
    std::fs::write("render_demo.ppm", out).expect("write");
    eprintln!(
        "adapter: {}  stats: {:?}",
        renderer.adapter_name(),
        renderer.stats()
    );
}
