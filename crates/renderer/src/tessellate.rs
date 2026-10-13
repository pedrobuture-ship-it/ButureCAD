//! Scene → triangle meshes (CPU side of rendering).
//!
//! Vertices are stored as `f32` offsets from a per-mesh `origin` (kept in `f64`), so large
//! board coordinates do not lose precision on the GPU. Tessellation density depends on the zoom
//! (`scale`, pixels per nm), hence meshes are rebuilt when the zoom bucket changes.

use crate::font;
use crate::scene::{Rgba, Scene, Shape};
use buturecad_geometry::{Arc, Circle, Coord, Point, Polygon, Segment};

/// GPU vertex: position relative to the mesh origin (nm) and colour.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: [u8; 4],
}

/// Indexed triangle list.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub origin: (f64, f64),
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Maximum allowed deviation of a flattened curve from the true curve, in pixels.
const CURVE_TOLERANCE_PX: f64 = 0.25;
/// Strokes at least this wide (px) get round caps/joins.
const ROUND_PX: f64 = 3.0;

pub struct MeshBuilder {
    mesh: Mesh,
    scale: f64,
}

impl MeshBuilder {
    pub fn new(origin: (f64, f64), scale: f64) -> Self {
        Self {
            mesh: Mesh {
                origin,
                ..Mesh::default()
            },
            scale: scale.max(1e-12),
        }
    }

    pub fn finish(self) -> Mesh {
        self.mesh
    }

    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    fn vertex(&mut self, x: f64, y: f64, color: Rgba) -> u32 {
        let i = self.mesh.vertices.len() as u32;
        self.mesh.vertices.push(Vertex {
            pos: [
                (x - self.mesh.origin.0) as f32,
                (y - self.mesh.origin.1) as f32,
            ],
            color: color.0,
        });
        i
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.mesh.indices.extend_from_slice(&[a, b, c]);
    }

    /// Wide enough to be visible: at least one pixel.
    fn min_width(&self, w: f64) -> f64 {
        w.max(1.0 / self.scale)
    }

    pub fn quad(&mut self, p: [(f64, f64); 4], color: Rgba) {
        let v: Vec<u32> = p.iter().map(|&(x, y)| self.vertex(x, y, color)).collect();
        self.tri(v[0], v[1], v[2]);
        self.tri(v[0], v[2], v[3]);
    }

    /// Axis-aligned rectangle in world coordinates (nm, f64).
    pub fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba) {
        self.quad([(x0, y0), (x1, y0), (x1, y1), (x0, y1)], color);
    }

    fn segments_for_radius(&self, r_world: f64) -> usize {
        let r_px = (r_world * self.scale).max(0.5);
        let step = 2.0 * (1.0 - CURVE_TOLERANCE_PX / r_px).clamp(-1.0, 1.0).acos();
        if step <= 0.0 || !step.is_finite() {
            return 8;
        }
        ((std::f64::consts::TAU / step).ceil() as usize).clamp(8, 256)
    }

    pub fn disc(&mut self, cx: f64, cy: f64, r: f64, color: Rgba) {
        let n = self.segments_for_radius(r);
        let c = self.vertex(cx, cy, color);
        let ring: Vec<u32> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                self.vertex(cx + r * a.cos(), cy + r * a.sin(), color)
            })
            .collect();
        for i in 0..n {
            self.tri(c, ring[i], ring[(i + 1) % n]);
        }
    }

    pub fn ring(&mut self, cx: f64, cy: f64, r_in: f64, r_out: f64, color: Rgba) {
        if r_in <= 0.0 {
            return self.disc(cx, cy, r_out, color);
        }
        let n = self.segments_for_radius(r_out);
        let pts: Vec<(u32, u32)> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                (
                    self.vertex(cx + r_in * a.cos(), cy + r_in * a.sin(), color),
                    self.vertex(cx + r_out * a.cos(), cy + r_out * a.sin(), color),
                )
            })
            .collect();
        for i in 0..n {
            let (a, b) = pts[i];
            let (c, d) = pts[(i + 1) % n];
            self.tri(a, b, d);
            self.tri(a, d, c);
        }
    }

    /// Thick line with flat ends; round caps when the stroke is wide on screen.
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), width: f64, color: Rgba) {
        let w = self.min_width(width);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let hw = w / 2.0;
        if len > 0.0 {
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            self.quad(
                [
                    (a.0 + nx, a.1 + ny),
                    (a.0 - nx, a.1 - ny),
                    (b.0 - nx, b.1 - ny),
                    (b.0 + nx, b.1 + ny),
                ],
                color,
            );
        }
        if w * self.scale >= ROUND_PX {
            self.disc(a.0, a.1, hw, color);
            self.disc(b.0, b.1, hw, color);
        }
    }

    pub fn polyline(&mut self, pts: &[(f64, f64)], width: f64, color: Rgba) {
        for w in pts.windows(2) {
            self.line(w[0], w[1], width, color);
        }
    }

    pub fn fill_polygon(&mut self, polygon: &Polygon, color: Rgba) {
        let ccw = polygon.clone().into_ccw();
        let v = ccw.vertices();
        let idx: Vec<u32> = v
            .iter()
            .map(|p| self.vertex(p.x as f64, p.y as f64, color))
            .collect();
        for [a, b, c] in triangulate(v) {
            self.tri(idx[a], idx[b], idx[c]);
        }
    }

    pub fn arc_points(&self, arc: &Arc) -> Vec<(f64, f64)> {
        let r = arc.radius();
        let c = (arc.center.x as f64, arc.center.y as f64);
        let a0 = ((arc.start.y - arc.center.y) as f64).atan2((arc.start.x - arc.center.x) as f64);
        let sweep = arc.sweep_angle() * if arc.ccw { 1.0 } else { -1.0 };
        let n = ((self.segments_for_radius(r) as f64 * sweep.abs() / std::f64::consts::TAU).ceil()
            as usize)
            .max(2);
        let mut pts: Vec<(f64, f64)> = (0..=n)
            .map(|i| {
                let a = a0 + sweep * i as f64 / n as f64;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect();
        // Use the exact integer endpoints so connected geometry meets without gaps.
        pts[0] = (arc.start.x as f64, arc.start.y as f64);
        pts[n] = (arc.end.x as f64, arc.end.y as f64);
        pts
    }

    pub fn text(&mut self, origin: Point, height: Coord, text: &str, color: Rgba) {
        let px = (height as f64 / font::GLYPH_H as f64).max(1.0);
        if px * self.scale < 0.5 {
            return; // glyphs would be sub-pixel: level-of-detail cut
        }
        let (ox, oy) = (origin.x as f64, origin.y as f64);
        for (i, ch) in text.chars().enumerate() {
            let gx = ox + (i * font::ADVANCE) as f64 * px;
            for col in 0..font::GLYPH_W {
                let mut row = 0;
                while row < font::GLYPH_H {
                    if font::lit(ch, col, row) {
                        let start = row;
                        while row < font::GLYPH_H && font::lit(ch, col, row) {
                            row += 1;
                        }
                        let x0 = gx + col as f64 * px;
                        // Row 0 is the glyph top; world +Y is up.
                        let y1 = oy + (font::GLYPH_H - start) as f64 * px;
                        let y0 = oy + (font::GLYPH_H - row) as f64 * px;
                        self.rect(x0, y0, x0 + px, y1, color);
                    } else {
                        row += 1;
                    }
                }
            }
        }
    }

    /// Adds one shape.
    pub fn shape(&mut self, shape: &Shape, color: Rgba) {
        let pt = |p: Point| (p.x as f64, p.y as f64);
        match shape {
            Shape::Line {
                segment: Segment { a, b },
                width,
            } => self.line(pt(*a), pt(*b), *width as f64, color),
            Shape::Arc { arc, width } => {
                let pts = self.arc_points(arc);
                self.polyline(&pts, *width as f64, color);
            }
            Shape::Circle {
                circle: Circle { center, radius },
                filled,
                stroke,
            } => {
                let (cx, cy) = pt(*center);
                if *filled {
                    self.disc(cx, cy, *radius as f64 + *stroke as f64 / 2.0, color);
                } else {
                    let hw = self.min_width(*stroke as f64) / 2.0;
                    self.ring(cx, cy, *radius as f64 - hw, *radius as f64 + hw, color);
                }
            }
            Shape::Rect {
                rect,
                filled,
                stroke,
            } => {
                let (a, b) = (pt(rect.min()), pt(rect.max()));
                if *filled {
                    let hw = *stroke as f64 / 2.0;
                    self.rect(a.0 - hw, a.1 - hw, b.0 + hw, b.1 + hw, color);
                } else {
                    let corners = [a, (b.0, a.1), b, (a.0, b.1)];
                    self.stroke_loop(&corners, *stroke as f64, color);
                }
            }
            Shape::Polygon {
                polygon,
                filled,
                stroke,
            } => {
                if *filled {
                    self.fill_polygon(polygon, color);
                }
                if !*filled || *stroke > 0 {
                    let pts: Vec<(f64, f64)> = polygon.vertices().iter().map(|p| pt(*p)).collect();
                    self.stroke_loop(&pts, *stroke as f64, color);
                }
            }
            Shape::Text {
                origin,
                height,
                text,
            } => self.text(*origin, *height, text, color),
        }
    }

    /// Closed outline; corners are covered by extending each edge by half the stroke width.
    fn stroke_loop(&mut self, pts: &[(f64, f64)], width: f64, color: Rgba) {
        let hw = self.min_width(width) / 2.0;
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dy);
            if len == 0.0 {
                continue;
            }
            let (ux, uy) = (dx / len * hw, dy / len * hw);
            self.line((a.0 - ux, a.1 - uy), (b.0 + ux, b.1 + uy), hw * 2.0, color);
        }
    }
}

/// Ear-clipping triangulation of a simple, counter-clockwise polygon. Returns index triples.
///
/// Exact integer predicates; O(n²). For degenerate input (collinear runs) it still terminates
/// by discarding zero-area ears. Holes are not supported.
pub fn triangulate(v: &[Point]) -> Vec<[usize; 3]> {
    let n = v.len();
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let cross = |a: Point, b: Point, c: Point| (b - a).cross(c - a);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n + 8 {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (v[ia], v[ib], v[ic]);
            let area = cross(a, b, c);
            if area == 0 {
                // Collinear vertex: drop it without emitting a degenerate triangle.
                idx.remove(i);
                clipped = true;
                break;
            }
            if area < 0 {
                continue; // reflex
            }
            let blocked = idx.iter().any(|&k| {
                k != ia && k != ib && k != ic && {
                    let p = v[k];
                    // Strictly inside or on the boundary of the candidate ear (excluding its corners).
                    p != a
                        && p != b
                        && p != c
                        && cross(a, b, p) >= 0
                        && cross(b, c, p) >= 0
                        && cross(c, a, p) >= 0
                }
            });
            if !blocked {
                out.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // No ear found (should not happen for simple polygons): force progress.
            let (ia, ib, ic) = (idx[idx.len() - 1], idx[0], idx[1]);
            if cross(v[ia], v[ib], v[ic]) > 0 {
                out.push([ia, ib, ic]);
            }
            idx.remove(0);
        }
    }
    if idx.len() == 3 && cross(v[idx[0]], v[idx[1]], v[idx[2]]) > 0 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// Meshes for every visible object, in layer order (higher layer ids last = on top).
pub fn tessellate_scene(scene: &Scene, scale: f64, origin: (f64, f64)) -> Mesh {
    let mut b = MeshBuilder::new(origin, scale);
    for (layer_id, layer) in scene.layers() {
        if !layer.visible {
            continue;
        }
        for (_, item) in scene.items().filter(|(_, i)| i.layer == layer_id) {
            b.shape(&item.shape, scene.color_of(item));
        }
    }
    b.finish()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::scene::{Item, Layer, LayerId, ObjectId};
    use buturecad_geometry::Rectangle;

    fn tri_area(m: &Mesh) -> f64 {
        m.indices
            .chunks(3)
            .map(|t| {
                let p: Vec<[f32; 2]> = t.iter().map(|&i| m.vertices[i as usize].pos).collect();
                ((p[1][0] - p[0][0]) as f64 * (p[2][1] - p[0][1]) as f64
                    - (p[2][0] - p[0][0]) as f64 * (p[1][1] - p[0][1]) as f64)
                    .abs()
                    / 2.0
            })
            .sum()
    }

    fn tri_area_exact(v: &[Point], tris: &[[usize; 3]]) -> f64 {
        tris.iter()
            .map(|t| ((v[t[1]] - v[t[0]]).cross(v[t[2]] - v[t[0]]).abs() as f64) / 2.0)
            .sum()
    }

    #[test]
    fn triangulates_concave_polygon_exactly() {
        let l = [(0, 0), (10, 0), (10, 4), (4, 4), (4, 10), (0, 10)].map(|(x, y)| Point::new(x, y));
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        assert_eq!(tri_area_exact(&l, &t), 64.0);
    }

    #[test]
    fn triangulation_handles_collinear_vertices() {
        let v = [(0, 0), (5, 0), (10, 0), (10, 10), (0, 10)].map(|(x, y)| Point::new(x, y));
        let t = triangulate(&v);
        assert_eq!(tri_area_exact(&v, &t), 100.0);
    }

    #[test]
    fn line_quad_has_expected_area() {
        let mut b = MeshBuilder::new((0.0, 0.0), 0.1);
        b.line((0.0, 0.0), (100.0, 0.0), 10.0, Rgba::rgb(1, 1, 1));
        assert!((tri_area(&b.finish()) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn hairline_is_one_pixel_wide() {
        let scale = 0.01; // 1 px = 100 nm
        let mut b = MeshBuilder::new((0.0, 0.0), scale);
        b.line((0.0, 0.0), (1000.0, 0.0), 0.0, Rgba::rgb(1, 1, 1));
        let area = tri_area(&b.finish());
        assert!((area - 1000.0 * 100.0).abs() < 1.0);
    }

    #[test]
    fn circle_density_adapts_to_screen_size() {
        let mut small = MeshBuilder::new((0.0, 0.0), 1.0);
        small.disc(0.0, 0.0, 4.0, Rgba::rgb(0, 0, 0));
        let mut big = MeshBuilder::new((0.0, 0.0), 1.0);
        big.disc(0.0, 0.0, 400.0, Rgba::rgb(0, 0, 0));
        let (s, g) = (small.finish(), big.finish());
        assert!(g.triangle_count() > s.triangle_count());
        assert!(g.triangle_count() <= 256);
        let expected = std::f64::consts::PI * 400.0 * 400.0;
        assert!((tri_area(&g) - expected).abs() / expected < 0.01);
    }

    #[test]
    fn ring_area() {
        let mut b = MeshBuilder::new((0.0, 0.0), 1.0);
        b.ring(0.0, 0.0, 90.0, 100.0, Rgba::rgb(0, 0, 0));
        let expected = std::f64::consts::PI * (100.0 * 100.0 - 90.0 * 90.0);
        assert!((tri_area(&b.finish()) - expected).abs() / expected < 0.02);
    }

    #[test]
    fn arc_polyline_keeps_exact_endpoints_and_radius() {
        let arc = Arc::new(
            Point::ORIGIN,
            Point::new(10_000, 0),
            Point::new(0, 10_000),
            true,
        )
        .unwrap();
        let b = MeshBuilder::new((0.0, 0.0), 0.01);
        let pts = b.arc_points(&arc);
        assert_eq!(pts.first(), Some(&(10_000.0, 0.0)));
        assert_eq!(pts.last(), Some(&(0.0, 10_000.0)));
        assert!(pts.iter().all(|p| (p.0.hypot(p.1) - 10_000.0).abs() < 1.0));
        // The CW arc between the same ends is the long way round.
        let cw = Arc::new(
            Point::ORIGIN,
            Point::new(10_000, 0),
            Point::new(0, 10_000),
            false,
        )
        .unwrap();
        assert!(b.arc_points(&cw).len() > pts.len());
    }

    #[test]
    fn text_is_drawn_and_culled_by_size() {
        let mut b = MeshBuilder::new((0.0, 0.0), 1.0);
        b.text(Point::ORIGIN, 70, "I", Rgba::rgb(1, 1, 1));
        let m = b.finish();
        assert_eq!(
            m.triangle_count(),
            2 * 5,
            "'I' is three vertical bars of runs: top/bottom serifs + stem"
        );
        let mut tiny = MeshBuilder::new((0.0, 0.0), 1.0e-4);
        tiny.text(Point::ORIGIN, 70, "Hello", Rgba::rgb(1, 1, 1));
        assert!(tiny.finish().vertices.is_empty());
    }

    #[test]
    fn scene_tessellation_respects_visibility_and_layer_order() {
        let mut s = Scene::new();
        s.set_layer(
            LayerId(1),
            Layer {
                name: "bottom".into(),
                color: Rgba::rgb(10, 0, 0),
                visible: true,
            },
        );
        s.set_layer(
            LayerId(2),
            Layer {
                name: "top".into(),
                color: Rgba::rgb(20, 0, 0),
                visible: true,
            },
        );
        s.set_layer(
            LayerId(3),
            Layer {
                name: "hidden".into(),
                color: Rgba::rgb(30, 0, 0),
                visible: false,
            },
        );
        let r = Rectangle::new(Point::ORIGIN, Point::new(10, 10)).unwrap();
        for (id, layer) in [(1, 2), (2, 1), (3, 3)] {
            s.insert(
                ObjectId(id),
                Item {
                    layer: LayerId(layer),
                    shape: Shape::Rect {
                        rect: r,
                        filled: true,
                        stroke: 0,
                    },
                    color: None,
                },
            );
        }
        let m = tessellate_scene(&s, 1.0, (0.0, 0.0));
        let reds: Vec<u8> = m.vertices.iter().map(|v| v.color[0]).collect();
        assert_eq!(reds.first(), Some(&10), "lower layer first");
        assert_eq!(reds.last(), Some(&20));
        assert!(!reds.contains(&30));
    }

    #[test]
    fn origin_keeps_far_coordinates_precise() {
        let far = 900_000_000.0;
        let mut b = MeshBuilder::new((far, far), 1.0);
        b.rect(far, far, far + 100.0, far + 100.0, Rgba::rgb(0, 0, 0));
        assert!(b.finish().vertices.iter().all(|v| v.pos[0].abs() <= 100.0));
    }
}
