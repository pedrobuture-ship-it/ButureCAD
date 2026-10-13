#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Rough scale check: 200k tracks. `cargo run --release -p buturecad-renderer --example stress`

use buturecad_geometry::{Point, Segment};
use buturecad_renderer::*;
use std::time::Instant;

fn main() {
    let mut scene = Scene::new();
    scene.set_layer(LayerId(1), Layer::new("Top", Rgba::rgb(220, 60, 60)));
    let t = Instant::now();
    let n = 200_000u64;
    for i in 0..n {
        let (x, y) = ((i % 500) as i64 * 400_000, (i / 500) as i64 * 400_000);
        let seg = Segment::new(
            Point::new(x, y),
            Point::new(x + 300_000, y + (i % 7) as i64 * 40_000),
        );
        scene.insert(
            ObjectId(i),
            Item {
                layer: LayerId(1),
                shape: Shape::Line {
                    segment: seg,
                    width: 120_000,
                },
                color: None,
            },
        );
    }
    eprintln!("insert {n} objects: {:?}", t.elapsed());
    let mut view = View::new(1400.0, 900.0);
    view.fit(&scene.bounds().unwrap(), 20.0);
    let t = Instant::now();
    let hit = pick(&scene, &view, 700.0, 450.0, 4.0);
    eprintln!("pick: {:?} -> {:?}", t.elapsed(), hit);
    let Ok(mut r) = GpuRenderer::new_blocking() else {
        return eprintln!("no GPU");
    };
    let (theme, grid, sel) = (
        Theme::dark(),
        GridSettings::new(1_270_000),
        Selection::new(),
    );
    for i in 0..3 {
        let req = FrameRequest {
            scene: &scene,
            view: &view,
            grid: &grid,
            theme: &theme,
            selection: &sel,
            box_select: None,
            preview: &[],
            debug: false,
        };
        r.render(&req).unwrap();
        eprintln!("frame {i}: {:?}", r.stats());
        view.pan_pixels(5.0, 0.0);
    }
}
