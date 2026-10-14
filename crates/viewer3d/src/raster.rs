//! A small deterministic software rasteriser: z-buffer, flat shading, two-sided triangles.

use crate::camera::{Camera, Projection};
use crate::mesh::Mesh;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    if l == 0.0 {
        a
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// Renders `mesh` to an RGBA image of `w × h` pixels over the `background` colour.
pub fn render(mesh: &Mesh, camera: &Camera, w: u32, h: u32, background: [u8; 3]) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let mut rgba = vec![255u8; wu * hu * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        px[0] = background[0];
        px[1] = background[1];
        px[2] = background[2];
    }
    let mut depth = vec![f32::INFINITY; wu * hu];
    let (wf, hf) = (f64::from(w), f64::from(h));
    let (_, _, fwd) = camera.axes();
    let light = norm([0.3, 0.2, 0.9]);
    for t in &mesh.triangles {
        let p: [[f64; 3]; 3] = [0, 1, 2].map(|i| {
            [
                f64::from(t.v[i][0]),
                f64::from(t.v[i][1]),
                f64::from(t.v[i][2]),
            ]
        });
        let Some(a) = camera.project(p[0], wf, hf) else {
            continue;
        };
        let Some(b) = camera.project(p[1], wf, hf) else {
            continue;
        };
        let Some(c) = camera.project(p[2], wf, hf) else {
            continue;
        };
        let (min_x, max_x) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
        let (min_y, max_y) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
        if max_x < 0.0 || max_y < 0.0 || min_x >= wf || min_y >= hf {
            continue;
        }
        let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
        if area.abs() < 1e-9 {
            continue;
        }
        // Flat shading from the triangle's world normal (two-sided).
        let n = norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        // One view vector for the whole image keeps large flat faces evenly shaded.
        let view = [-fwd[0], -fwd[1], -fwd[2]];
        let shade = 0.30 + 0.50 * dot(n, view).abs() + 0.20 * dot(n, light).abs();
        let col = [
            (f64::from(t.color[0]) * shade).min(255.0) as u8,
            (f64::from(t.color[1]) * shade).min(255.0) as u8,
            (f64::from(t.color[2]) * shade).min(255.0) as u8,
        ];
        let x0 = min_x.floor().max(0.0) as usize;
        let x1 = (max_x.ceil() as usize).min(wu.saturating_sub(1));
        let y0 = min_y.floor().max(0.0) as usize;
        let y1 = (max_y.ceil() as usize).min(hu.saturating_sub(1));
        // 1/z is linear in screen space under perspective.
        let inv = |z: f64| {
            if camera.projection == Projection::Perspective {
                1.0 / z
            } else {
                z
            }
        };
        let (ia, ib, ic) = (inv(a.2), inv(b.2), inv(c.2));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((b.0 - px) * (c.1 - py) - (c.0 - px) * (b.1 - py)) / area;
                let w1 = ((c.0 - px) * (a.1 - py) - (a.0 - px) * (c.1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let v = w0 * ia + w1 * ib + w2 * ic;
                // Smaller = nearer: under perspective store -1/z.
                let d = if camera.projection == Projection::Perspective {
                    -v
                } else {
                    v
                } as f32;
                let i = y * wu + x;
                if d < depth[i] {
                    depth[i] = d;
                    rgba[i * 4] = col[0];
                    rgba[i * 4 + 1] = col[1];
                    rgba[i * 4 + 2] = col[2];
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::mesh::build_mesh;
    use crate::model::{Model, Visibility};

    fn board() -> Model {
        let mut m = Model::empty();
        m.outline = vec![[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
        m
    }

    fn pixel(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2]]
    }

    #[test]
    fn the_board_is_drawn_in_the_middle_and_the_background_outside() {
        let mesh = build_mesh(&board(), &Visibility::default());
        let cam = Camera::framing([0.0, 0.0], [40.0, 20.0]);
        let img = render(&mesh, &cam, 320, 240, [10, 12, 18]);
        assert_ne!(
            pixel(&img, 320, 160, 120),
            [10, 12, 18],
            "the board covers the centre"
        );
        assert_eq!(
            pixel(&img, 320, 2, 2),
            [10, 12, 18],
            "the corner is background"
        );
        // Deterministic.
        assert_eq!(img, render(&mesh, &cam, 320, 240, [10, 12, 18]));
    }

    #[test]
    fn nearer_geometry_hides_farther_geometry() {
        let mut m = board();
        m.bodies.push(crate::model::Body {
            outline: vec![[15.0, 5.0], [25.0, 5.0], [25.0, 15.0], [15.0, 15.0]],
            height: 8.0,
            top: true,
            has_model: false,
            label: "U1".into(),
        });
        let mesh = build_mesh(&m, &Visibility::default());
        let mut cam = Camera::framing([0.0, 0.0], [40.0, 20.0]);
        cam.set_view(
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2 - 0.001,
        );
        let with = render(&mesh, &cam, 320, 240, [0, 0, 0]);
        let bare = render(
            &build_mesh(&board(), &Visibility::default()),
            &cam,
            320,
            240,
            [0, 0, 0],
        );
        assert_ne!(
            pixel(&with, 320, 160, 120),
            pixel(&bare, 320, 160, 120),
            "the body is in front of the board"
        );
        // Hiding components restores the view.
        let hidden = build_mesh(
            &m,
            &Visibility {
                components: false,
                ..Visibility::default()
            },
        );
        assert_eq!(render(&hidden, &cam, 320, 240, [0, 0, 0]), bare);
    }

    #[test]
    fn orthographic_and_perspective_both_render() {
        let mesh = build_mesh(&board(), &Visibility::default());
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let cam = Camera {
                projection,
                ..Camera::framing([0.0, 0.0], [40.0, 20.0])
            };
            let img = render(&mesh, &cam, 200, 150, [0, 0, 0]);
            assert!(img.chunks(4).any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0));
        }
    }
}
