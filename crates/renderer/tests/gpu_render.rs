//! GPU integration tests. They skip (loudly) when the machine has no usable adapter.
#![allow(clippy::unwrap_used)]

use buturecad_geometry::{Point, Rectangle};
use buturecad_renderer::*;

fn renderer() -> Option<GpuRenderer> {
    match GpuRenderer::new_blocking() {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("SKIPPED: no GPU adapter available ({e})");
            None
        }
    }
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * f.width + x) * 4) as usize;
    [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
}

fn scene_with_square(layer_visible: bool) -> Scene {
    let mut s = Scene::new();
    s.set_layer(
        LayerId(1),
        Layer::new("L", Rgba::rgb(200, 30, 30)).with_visible(layer_visible),
    );
    let r = Rectangle::new(
        Point::new(-5_000_000, -5_000_000),
        Point::new(5_000_000, 5_000_000),
    )
    .unwrap();
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
    s
}

fn view() -> View {
    // 100 px = 10 mm: the 10 mm square is 100 px wide, centred.
    View {
        center: (0.0, 0.0),
        scale: 1.0e-5,
        width: 200.0,
        height: 200.0,
    }
}

fn request<'a>(
    scene: &'a Scene,
    view: &'a View,
    grid: &'a GridSettings,
    theme: &'a Theme,
    sel: &'a Selection,
) -> FrameRequest<'a> {
    FrameRequest {
        scene,
        view,
        grid,
        theme,
        selection: sel,
        box_select: None,
        preview: &[],
        debug: false,
    }
}

#[test]
fn draws_scene_geometry_where_the_view_says() {
    let Some(mut r) = renderer() else { return };
    let (scene, view, theme, sel) = (
        scene_with_square(true),
        view(),
        Theme::dark(),
        Selection::new(),
    );
    let grid = GridSettings {
        visible: false,
        ..GridSettings::new(1_270_000)
    };
    let f = r
        .render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert_eq!((f.width, f.height), (200, 200));
    assert_eq!(px(&f, 100, 100), [200, 30, 30], "inside the square");
    assert_eq!(
        px(&f, 20, 20),
        theme.background.0[..3],
        "outside is background"
    );
    assert_eq!(
        px(&f, 160, 100),
        theme.background.0[..3],
        "right of the 100px square"
    );
}

#[test]
fn hidden_layer_is_not_drawn() {
    let Some(mut r) = renderer() else { return };
    let (scene, view, theme, sel) = (
        scene_with_square(false),
        view(),
        Theme::dark(),
        Selection::new(),
    );
    let grid = GridSettings {
        visible: false,
        ..GridSettings::new(1_270_000)
    };
    let f = r
        .render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert_eq!(px(&f, 100, 100), theme.background.0[..3]);
}

#[test]
fn selection_and_hover_are_highlighted() {
    let Some(mut r) = renderer() else { return };
    let (scene, view, theme) = (scene_with_square(true), view(), Theme::dark());
    let grid = GridSettings {
        visible: false,
        ..GridSettings::new(1_270_000)
    };
    let mut sel = Selection::new();
    sel.set([ObjectId(1)]);
    let f = r
        .render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert_eq!(px(&f, 100, 100), theme.selection.0[..3]);
    sel.clear();
    sel.hover = Some(ObjectId(1));
    let f = r
        .render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert_eq!(px(&f, 100, 100), theme.hover.0[..3]);
}

#[test]
fn grid_is_drawn_when_enabled() {
    let Some(mut r) = renderer() else { return };
    let (scene, view, theme, sel) = (Scene::new(), view(), Theme::dark(), Selection::new());
    let on = GridSettings::new(1_270_000);
    let f = r
        .render(&request(&scene, &view, &on, &theme, &sel))
        .unwrap();
    let lit = f
        .rgba
        .chunks(4)
        .filter(|p| p[..3] != theme.background.0[..3])
        .count();
    assert!(lit > 200, "grid lines should cover many pixels, got {lit}");
    let off = GridSettings {
        visible: false,
        ..on
    };
    let f = r
        .render(&request(&scene, &view, &off, &theme, &sel))
        .unwrap();
    assert_eq!(
        f.rgba
            .chunks(4)
            .filter(|p| p[..3] != theme.background.0[..3])
            .count(),
        0
    );
}

#[test]
fn mesh_is_cached_until_scene_or_zoom_changes() {
    let Some(mut r) = renderer() else { return };
    let (mut scene, mut view, theme, sel) = (
        scene_with_square(true),
        view(),
        Theme::dark(),
        Selection::new(),
    );
    let grid = GridSettings::new(1_270_000);
    r.render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert!(r.stats().mesh_rebuilt);
    view.pan_pixels(10.0, 5.0);
    r.render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert!(!r.stats().mesh_rebuilt, "panning must not re-tessellate");
    view.zoom_at(100.0, 100.0, 3.0);
    r.render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert!(
        r.stats().mesh_rebuilt,
        "zooming changes the tessellation bucket"
    );
    scene.remove(ObjectId(1));
    r.render(&request(&scene, &view, &grid, &theme, &sel))
        .unwrap();
    assert!(r.stats().mesh_rebuilt);
}

#[test]
fn handles_resize_and_empty_scene() {
    let Some(mut r) = renderer() else { return };
    let (scene, theme, sel) = (Scene::new(), Theme::light(), Selection::new());
    let grid = GridSettings::new(1_270_000);
    for (w, h) in [(300.0, 100.0), (1.0, 1.0), (257.0, 33.0)] {
        let view = View::new(w, h);
        let f = r
            .render(&request(&scene, &view, &grid, &theme, &sel))
            .unwrap();
        assert_eq!(f.rgba.len(), (f.width * f.height * 4) as usize);
    }
}
