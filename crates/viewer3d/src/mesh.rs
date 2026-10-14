//! Model → triangles.

use crate::model::{Model, Visibility, P};
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::int::triangulatable::IntTriangulatable;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub v: [[f32; 3]; 3],
    pub color: [u8; 3],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
}

const SUBSTRATE: [u8; 3] = [196, 170, 110];
const MASK: [u8; 3] = [24, 104, 58];
const MASK_OVER_COPPER: [u8; 3] = [40, 140, 80];
const COPPER: [u8; 3] = [184, 115, 51];
const GOLD: [u8; 3] = [222, 184, 70];
const SILK: [u8; 3] = [240, 240, 235];
const BODY: [u8; 3] = [44, 44, 48];
const BODY_MODEL: [u8; 3] = [52, 70, 120];
const SCALE: f64 = 1e6;

fn ip(p: P) -> IntPoint<i64> {
    IntPoint::new((p[0] * SCALE).round() as i64, (p[1] * SCALE).round() as i64)
}

fn area2(c: &[IntPoint<i64>]) -> i128 {
    c.iter()
        .zip(c.iter().cycle().skip(1))
        .map(|(a, b)| i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y))
        .sum()
}

fn contour(pts: &[P], ccw: bool) -> Vec<IntPoint<i64>> {
    let mut c: Vec<IntPoint<i64>> = pts.iter().map(|p| ip(*p)).collect();
    if (area2(&c) > 0) != ccw {
        c.reverse();
    }
    c
}

/// Triangulates a polygon with holes (exact integer triangulation; mm in, mm out).
pub fn triangulate(outer: &[P], holes: &[Vec<P>]) -> Vec<[P; 3]> {
    if outer.len() < 3 {
        return Vec::new();
    }
    let mut shape = vec![contour(outer, true)];
    shape.extend(
        holes
            .iter()
            .filter(|h| h.len() >= 3)
            .map(|h| contour(h, false)),
    );
    let t = shape.triangulate().into_triangulation::<u32>();
    let back = |i: u32| {
        t.points
            .get(i as usize)
            .map(|p| [p.x as f64 / SCALE, p.y as f64 / SCALE])
    };
    t.indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|tri| Some([back(tri[0])?, back(tri[1])?, back(tri[2])?]))
        .collect()
}

impl Mesh {
    fn tri(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], color: [u8; 3]) {
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
        self.triangles.push(Triangle {
            v: [f(a), f(b), f(c)],
            color,
        });
    }

    fn quad(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], color: [u8; 3]) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// A flat cap at height `z`.
    fn cap(&mut self, outer: &[P], holes: &[Vec<P>], z: f64, color: [u8; 3]) {
        for t in triangulate(outer, holes) {
            self.tri(
                [t[0][0], t[0][1], z],
                [t[1][0], t[1][1], z],
                [t[2][0], t[2][1], z],
                color,
            );
        }
    }

    /// Side walls of a contour between two heights.
    fn walls(&mut self, ring: &[P], z0: f64, z1: f64, color: [u8; 3]) {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            self.quad(
                [a[0], a[1], z0],
                [b[0], b[1], z0],
                [b[0], b[1], z1],
                [a[0], a[1], z1],
                color,
            );
        }
    }

    /// A prism over a simple polygon: top cap and walls (and a bottom cap when asked).
    fn prism(&mut self, outline: &[P], z0: f64, z1: f64, color: [u8; 3], bottom: bool) {
        self.cap(outline, &[], z1, color);
        if bottom {
            self.cap(outline, &[], z0, color);
        }
        self.walls(outline, z0, z1, color);
    }
}

fn circle(c: P, r: f64, n: usize) -> Vec<P> {
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Rectangle around a segment, square-capped by half the width.
fn seg_poly(a: P, b: P, w: f64) -> Vec<P> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy);
    let (ux, uy) = if l > 0.0 {
        (dx / l, dy / l)
    } else {
        (1.0, 0.0)
    };
    let (nx, ny) = (-uy * w / 2.0, ux * w / 2.0);
    let (ex, ey) = (ux * w / 2.0, uy * w / 2.0);
    vec![
        [a[0] - ex + nx, a[1] - ey + ny],
        [b[0] + ex + nx, b[1] + ey + ny],
        [b[0] + ex - nx, b[1] + ey - ny],
        [a[0] - ex - nx, a[1] - ey - ny],
    ]
}

/// Builds the mesh of `model` for what `vis` shows.
pub fn build_mesh(model: &Model, vis: &Visibility) -> Mesh {
    let mut m = Mesh::default();
    if model.outline.len() < 3 {
        return m;
    }
    let (t, cu, mk) = (
        model.thickness,
        model.copper_thickness,
        model.mask_thickness,
    );
    // Everything on the top side sits above z = t, on the bottom side below z = 0.
    let zt = |h: f64| t + h;
    let zb = |h: f64| -h;
    let drills: Vec<Vec<P>> = model
        .vias
        .iter()
        .filter(|v| v.drill_radius > 0.0)
        .map(|v| circle(v.center, v.drill_radius, 16))
        .collect();
    let mut holes: Vec<Vec<P>> = model.cutouts.clone();
    holes.extend(drills.iter().cloned());

    if vis.substrate {
        m.cap(&model.outline, &holes, t, SUBSTRATE);
        m.cap(&model.outline, &holes, 0.0, SUBSTRATE);
        m.walls(&model.outline, 0.0, t, SUBSTRATE);
        for h in &holes {
            m.walls(h, 0.0, t, SUBSTRATE);
        }
    }
    if vis.mask {
        for top in [true, false] {
            // The plate lies on the substrate; over copper it rises by the copper thickness.
            let z = if top { zt(mk) } else { zb(mk) };
            let z_over = if top { zt(cu + mk) } else { zb(cu + mk) };
            // The mask plate: board outline minus pad openings and drills.
            let mut openings = holes.clone();
            openings.extend(
                model
                    .pads
                    .iter()
                    .filter(|p| if top { p.top } else { p.bottom })
                    .map(|p| p.outline.clone()),
            );
            if vis.substrate {
                m.cap(&model.outline, &openings, z, MASK);
            }
            if vis.copper {
                // The mask follows the copper it covers.
                for s in model.tracks.iter().filter(|s| s.top == top) {
                    let poly = seg_poly(s.a, s.b, s.width);
                    let (z0, z1) = if top {
                        (zt(0.0), z_over)
                    } else {
                        (z_over, zb(0.0))
                    };
                    m.prism(&poly, z0.min(z1), z0.max(z1), MASK_OVER_COPPER, false);
                }
                for r in model.zones.iter().filter(|r| r.top == top) {
                    m.cap(&r.outer, &r.holes, z_over, MASK_OVER_COPPER);
                }
            }
        }
    }
    if vis.copper {
        for top in [true, false] {
            let (z0, z1) = if top {
                (zt(0.0), zt(cu))
            } else {
                (zb(cu), zb(0.0))
            };
            if !vis.mask {
                for s in model.tracks.iter().filter(|s| s.top == top) {
                    m.prism(&seg_poly(s.a, s.b, s.width), z0, z1, COPPER, false);
                }
                for r in model.zones.iter().filter(|r| r.top == top) {
                    m.cap(&r.outer, &r.holes, if top { z1 } else { z0 }, COPPER);
                }
            }
            let lift = if vis.mask { mk * 0.5 } else { 0.0 };
            for p in model
                .pads
                .iter()
                .filter(|p| if top { p.top } else { p.bottom })
            {
                let (a, b) = if top {
                    (z0, z1 + lift)
                } else {
                    (z0 - lift, z1)
                };
                m.prism(&p.outline, a, b, GOLD, false);
            }
        }
        for v in &model.vias {
            if v.outer_radius > v.drill_radius {
                let ring_out = circle(v.center, v.outer_radius, 16);
                let ring_in = circle(v.center, v.drill_radius.max(0.0), 16);
                let hole = if v.drill_radius > 0.0 {
                    vec![ring_in]
                } else {
                    Vec::new()
                };
                let top_z = zt(cu + if vis.mask { mk * 0.5 } else { 0.0 });
                let bot_z = zb(cu + if vis.mask { mk * 0.5 } else { 0.0 });
                m.cap(&ring_out, &hole, top_z, GOLD);
                m.cap(&ring_out, &hole, bot_z, GOLD);
            }
        }
    }
    if vis.silkscreen {
        for s in &model.silk {
            let z = |top: bool| {
                if top {
                    zt(cu + mk + 0.002)
                } else {
                    zb(cu + mk + 0.002)
                }
            };
            m.cap(&seg_poly(s.a, s.b, s.width), &[], z(s.top), SILK);
        }
    }
    if vis.components {
        for b in &model.bodies {
            let base = zt(cu + mk);
            let (z0, z1) = if b.top {
                (base, base + b.height)
            } else {
                (-(cu + mk) - b.height, -(cu + mk))
            };
            m.prism(
                &b.outline,
                z0,
                z1,
                if b.has_model { BODY_MODEL } else { BODY },
                true,
            );
        }
    }
    m
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::model::{Model, Pad, Segment, ViaHole};

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    fn area(t: &[[P; 3]]) -> f64 {
        t.iter()
            .map(|t| {
                ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1])
                    - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]))
                    .abs()
                    / 2.0
            })
            .sum()
    }

    #[test]
    fn triangulation_respects_holes() {
        let t = triangulate(&square(0.0, 0.0, 10.0, 10.0), &[square(2.0, 2.0, 4.0, 4.0)]);
        assert!((area(&t) - 96.0).abs() < 1e-6, "{}", area(&t));
    }

    #[test]
    fn a_board_meshes_with_every_layer_and_visibility_removes_them() {
        let mut model = Model::empty();
        model.outline = square(0.0, 0.0, 20.0, 10.0);
        model.tracks.push(Segment {
            a: [2.0, 5.0],
            b: [12.0, 5.0],
            width: 0.25,
            top: true,
        });
        model.pads.push(Pad {
            outline: square(14.0, 4.0, 15.0, 6.0),
            top: true,
            bottom: false,
        });
        model.vias.push(ViaHole {
            center: [8.0, 2.0],
            outer_radius: 0.4,
            drill_radius: 0.2,
        });
        model.silk.push(Segment {
            a: [1.0, 1.0],
            b: [5.0, 1.0],
            width: 0.15,
            top: true,
        });
        let all = build_mesh(&model, &Visibility::default());
        assert!(all.triangles.len() > 50);
        let bare = build_mesh(
            &model,
            &Visibility {
                substrate: true,
                mask: false,
                copper: false,
                silkscreen: false,
                components: false,
            },
        );
        assert!(bare.triangles.len() < all.triangles.len());
        // Substrate only: top and bottom caps cover the board minus the via drill.
        let top_area: f64 = bare
            .triangles
            .iter()
            .filter(|t| {
                (t.v[0][2] - 1.5).abs() < 1e-6
                    && (t.v[1][2] - 1.5).abs() < 1e-6
                    && (t.v[2][2] - 1.5).abs() < 1e-6
            })
            .map(|t| {
                ((t.v[1][0] - t.v[0][0]) * (t.v[2][1] - t.v[0][1])
                    - (t.v[2][0] - t.v[0][0]) * (t.v[1][1] - t.v[0][1]))
                    .abs() as f64
                    / 2.0
            })
            .sum();
        assert!((top_area - 200.0).abs() < 0.5, "{top_area}");
    }

    #[test]
    fn no_outline_means_nothing_to_draw() {
        assert!(build_mesh(&Model::empty(), &Visibility::default())
            .triangles
            .is_empty());
    }
}
