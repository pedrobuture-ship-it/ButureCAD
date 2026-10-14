//! Deterministic 2D software rasteriser for parsed Gerber/Excellon data: one coverage mask per
//! layer (dark sets, clear erases — so clear polarity works), composited in order. Independent
//! from the PCB model: it draws exactly what the reader parsed.

use crate::excellon_read::DrillFile;
use crate::gerber_read::{half_extent, ApShape, Image, Prim, P};
use std::f64::consts::PI;

pub enum Content<'a> {
    Gerber(&'a Image),
    Drill(&'a DrillFile),
}

pub struct LayerDraw<'a> {
    pub content: Content<'a>,
    pub color: [u8; 3],
    /// 0.0–1.0 opacity of the layer over what is below.
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// View centre, nm.
    pub center: [f64; 2],
    /// Pixels per nm.
    pub scale: f64,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// Viewport that frames `(lo, hi)` with a margin (fraction of the canvas).
    pub fn fit(lo: P, hi: P, width: u32, height: u32) -> Self {
        let w = ((hi[0] - lo[0]) as f64).max(1.0);
        let h = ((hi[1] - lo[1]) as f64).max(1.0);
        let scale = (f64::from(width) * 0.9 / w).min(f64::from(height) * 0.9 / h);
        Self {
            center: [(lo[0] + hi[0]) as f64 / 2.0, (lo[1] + hi[1]) as f64 / 2.0],
            scale,
            width,
            height,
        }
    }

    pub fn to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        [
            (p[0] - self.center[0]) * self.scale + f64::from(self.width) / 2.0,
            f64::from(self.height) / 2.0 - (p[1] - self.center[1]) * self.scale,
        ]
    }

    /// Zoom about a screen point.
    pub fn zoom_at(&mut self, sx: f64, sy: f64, factor: f64) {
        let before = [
            (sx - f64::from(self.width) / 2.0) / self.scale + self.center[0],
            (f64::from(self.height) / 2.0 - sy) / self.scale + self.center[1],
        ];
        self.scale = (self.scale * factor).clamp(1e-9, 1e-2);
        self.center = [
            before[0] - (sx - f64::from(self.width) / 2.0) / self.scale,
            before[1] - (f64::from(self.height) / 2.0 - sy) / self.scale,
        ];
    }

    pub fn pan_pixels(&mut self, dx: f64, dy: f64) {
        self.center[0] -= dx / self.scale;
        self.center[1] += dy / self.scale;
    }
}

fn circle_poly(c: [f64; 2], r: f64, px_r: f64) -> Vec<[f64; 2]> {
    let n = (px_r * 1.5).clamp(12.0, 128.0) as usize;
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Outline of an aperture centred at `c` (nm, f64). All aperture shapes here are convex.
fn ap_poly(ap: &ApShape, c: [f64; 2], scale: f64) -> Vec<[f64; 2]> {
    match ap {
        ApShape::Circle { d } => circle_poly(c, *d as f64 / 2.0, *d as f64 / 2.0 * scale),
        ApShape::Rect { w, h } => {
            let (hw, hh) = (*w as f64 / 2.0, *h as f64 / 2.0);
            vec![
                [c[0] - hw, c[1] - hh],
                [c[0] + hw, c[1] - hh],
                [c[0] + hw, c[1] + hh],
                [c[0] - hw, c[1] + hh],
            ]
        }
        ApShape::Obround { w, h } => {
            let (w, h) = (*w as f64, *h as f64);
            let r = w.min(h) / 2.0;
            let d = (w.max(h) - w.min(h)) / 2.0;
            let (a, b) = if w >= h {
                ([c[0] - d, c[1]], [c[0] + d, c[1]])
            } else {
                ([c[0], c[1] - d], [c[0], c[1] + d])
            };
            let mut pts = circle_poly(a, r, r * scale);
            pts.extend(circle_poly(b, r, r * scale));
            hull(pts)
        }
        ApShape::Polygon { d, n, rot_deg } => {
            let n = (*n).clamp(3, 12) as usize;
            (0..n)
                .map(|k| {
                    let a = rot_deg.to_radians() + 2.0 * PI * k as f64 / n as f64;
                    let r = *d as f64 / 2.0;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect()
        }
        ApShape::Unsupported(_) => Vec::new(),
    }
}

/// Convex hull (monotone chain).
fn hull(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut h: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for q in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], *q) <= 0.0 {
                h.pop();
            }
            h.push(*q);
        }
        h.pop();
    }
    h
}

struct Mask {
    w: usize,
    h: usize,
    v: Vec<u8>,
}

impl Mask {
    /// Even-odd scanline fill of a polygon given in screen coordinates.
    fn fill(&mut self, poly: &[[f64; 2]], value: u8) {
        if poly.len() < 3 {
            return;
        }
        let (mut y0, mut y1) = (f64::MAX, f64::MIN);
        for p in poly {
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        let ys = (y0 - 0.5).ceil().max(0.0) as i64;
        let ye = ((y1 - 0.5).floor() as i64).min(self.h as i64 - 1);
        let mut xs: Vec<f64> = Vec::new();
        for y in ys..=ye {
            let yc = y as f64 + 0.5;
            xs.clear();
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a[1] > yc) != (b[1] > yc) {
                    xs.push(a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                let x0 = (pair[0] - 0.5).ceil().max(0.0) as i64;
                let x1 = ((pair[1] - 0.5).floor() as i64).min(self.w as i64 - 1);
                for x in x0..=x1 {
                    self.v[y as usize * self.w + x as usize] = value;
                }
            }
        }
    }
}

fn nm(p: P) -> [f64; 2] {
    [p[0] as f64, p[1] as f64]
}

fn draw_gerber(m: &mut Mask, img: &Image, vp: &Viewport) {
    let to_s = |pts: Vec<[f64; 2]>| -> Vec<[f64; 2]> {
        pts.into_iter().map(|p| vp.to_screen(p)).collect()
    };
    for p in &img.prims {
        match p {
            Prim::Flash { ap, at, dark } => {
                m.fill(
                    &to_s(ap_poly(ap, nm(*at), vp.scale)),
                    if *dark { 255 } else { 0 },
                );
            }
            Prim::Stroke { a, b, ap, dark } => {
                let mut pts = ap_poly(ap, nm(*a), vp.scale);
                pts.extend(ap_poly(ap, nm(*b), vp.scale));
                let h = hull(pts);
                m.fill(&to_s(h), if *dark { 255 } else { 0 });
            }
            Prim::Region { contours, dark } => {
                for c in contours {
                    m.fill(
                        &to_s(c.iter().map(|q| nm(*q)).collect()),
                        if *dark { 255 } else { 0 },
                    );
                }
            }
        }
    }
}

fn draw_drill(m: &mut Mask, d: &DrillFile, vp: &Viewport) {
    for h in &d.hits {
        let Some(dia) = d.diameter(h.tool) else {
            continue;
        };
        let r = dia as f64 / 2.0;
        let mut pts = circle_poly(nm(h.at), r, r * vp.scale);
        if let Some(b) = h.slot_to {
            pts.extend(circle_poly(nm(b), r, r * vp.scale));
        }
        let poly: Vec<[f64; 2]> = hull(pts).into_iter().map(|p| vp.to_screen(p)).collect();
        m.fill(&poly, 255);
    }
}

/// Renders RGBA (opaque) pixels: `bg`, then each layer in order.
pub fn render(layers: &[LayerDraw<'_>], vp: &Viewport, bg: [u8; 3]) -> Vec<u8> {
    let (w, h) = (vp.width as usize, vp.height as usize);
    let mut px: Vec<f32> = Vec::with_capacity(w * h * 3);
    for _ in 0..w * h {
        px.extend(bg.iter().map(|c| f32::from(*c)));
    }
    for l in layers {
        let mut m = Mask {
            w,
            h,
            v: vec![0; w * h],
        };
        match &l.content {
            Content::Gerber(i) => draw_gerber(&mut m, i, vp),
            Content::Drill(d) => draw_drill(&mut m, d, vp),
        }
        for (i, c) in m.v.iter().enumerate() {
            if *c != 0 {
                for k in 0..3 {
                    px[i * 3 + k] += (f32::from(l.color[k]) - px[i * 3 + k]) * l.alpha;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for c in px.as_chunks::<3>().0 {
        out.extend_from_slice(&[
            c[0].round() as u8,
            c[1].round() as u8,
            c[2].round() as u8,
            255,
        ]);
    }
    out
}

/// Union bounds of everything drawn.
pub fn bounds(layers: &[LayerDraw<'_>]) -> Option<(P, P)> {
    let mut b: Option<(P, P)> = None;
    for l in layers {
        let lb = match &l.content {
            Content::Gerber(i) => i.bounds(),
            Content::Drill(d) => d.bounds().map(|(lo, hi)| {
                let r = d.tools.iter().map(|t| t.1).max().unwrap_or(0) / 2;
                ([lo[0] - r, lo[1] - r], [hi[0] + r, hi[1] + r])
            }),
        };
        if let Some((lo, hi)) = lb {
            b = Some(match b {
                None => (lo, hi),
                Some((a, c)) => (
                    [a[0].min(lo[0]), a[1].min(lo[1])],
                    [c[0].max(hi[0]), c[1].max(hi[1])],
                ),
            });
        }
    }
    b
}

#[allow(dead_code)]
fn _extent(ap: &ApShape) -> (i64, i64) {
    half_extent(ap)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::gerber_read::parse;

    fn px(img: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let i = (y * w + x) * 4;
        [img[i], img[i + 1], img[i + 2]]
    }

    #[test]
    fn flash_stroke_region_and_clear_polarity_are_drawn() {
        // 10 mm x 10 mm area: a 6 mm dark square region with a 2 mm clear circle in its middle.
        let g = parse("%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,2.000000*%\n%LPD*%\nG36*\nX2000000Y2000000D02*\nX8000000Y2000000D01*\nX8000000Y8000000D01*\nX2000000Y8000000D01*\nX2000000Y2000000D01*\nG37*\n%LPC*%\nD10*\nX5000000Y5000000D03*\nM02*\n").unwrap();
        let vp = Viewport {
            center: [5e6, 5e6],
            scale: 100.0 / 10e6,
            width: 100,
            height: 100,
        };
        let l = [LayerDraw {
            content: Content::Gerber(&g),
            color: [200, 0, 0],
            alpha: 1.0,
        }];
        let img = render(&l, &vp, [0, 0, 0]);
        assert_eq!(
            px(&img, 100, 50, 50),
            [0, 0, 0],
            "clear circle erased the middle"
        );
        assert_eq!(px(&img, 100, 30, 50), [200, 0, 0], "region is filled");
        assert_eq!(px(&img, 100, 5, 5), [0, 0, 0], "outside is background");
        // +Y is up: the point (3 mm, 7 mm) is near the top-left of the square.
        assert_eq!(px(&img, 100, 30, 30), [200, 0, 0]);
    }

    #[test]
    fn strokes_use_the_aperture_width() {
        let g = parse("%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,2.000000*%\nD10*\nX1000000Y5000000D02*\nX9000000Y5000000D01*\nM02*\n").unwrap();
        let vp = Viewport {
            center: [5e6, 5e6],
            scale: 100.0 / 10e6,
            width: 100,
            height: 100,
        };
        let l = [LayerDraw {
            content: Content::Gerber(&g),
            color: [9, 9, 9],
            alpha: 1.0,
        }];
        let img = render(&l, &vp, [0, 0, 0]);
        assert_eq!(px(&img, 100, 50, 41), [9, 9, 9]);
        assert_eq!(px(&img, 100, 50, 59), [9, 9, 9]);
        assert_eq!(px(&img, 100, 50, 38), [0, 0, 0]);
        assert_eq!(px(&img, 100, 50, 62), [0, 0, 0]);
        assert_eq!(
            bounds(&l).unwrap(),
            ([0, 4_000_000], [10_000_000, 6_000_000])
        );
    }

    #[test]
    fn drills_and_fit() {
        let d = crate::excellon_read::parse(
            "M48\nMETRIC\nT1C2.000000\n%\nT1\nX5.000000Y5.000000\nM30\n",
        )
        .unwrap();
        let l = [LayerDraw {
            content: Content::Drill(&d),
            color: [255, 255, 255],
            alpha: 1.0,
        }];
        let (lo, hi) = bounds(&l).unwrap();
        let vp = Viewport::fit(lo, hi, 100, 100);
        let img = render(&l, &vp, [0, 0, 0]);
        assert_eq!(px(&img, 100, 50, 50), [255, 255, 255]);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut vp = Viewport {
            center: [0.0, 0.0],
            scale: 1e-5,
            width: 200,
            height: 100,
        };
        let before = vp.to_screen([3e6, 2e6]);
        vp.zoom_at(before[0], before[1], 2.0);
        let after = vp.to_screen([3e6, 2e6]);
        assert!((before[0] - after[0]).abs() < 1e-6 && (before[1] - after[1]).abs() < 1e-6);
    }
}
