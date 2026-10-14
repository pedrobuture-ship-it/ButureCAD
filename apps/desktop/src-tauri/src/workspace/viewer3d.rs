//! The 3D viewer in the desktop app.
//!
//! `buturecad-viewer3d` is decoupled from the PCB core: this module is the **bridge** that turns
//! the board (`PcbDoc`) into the viewer's plain millimetre `Model`, keeps the camera and the
//! visibility toggles, rebuilds the mesh when the board changes (keyed by the engine revision, so
//! every edit — including undo — shows up in 3D) and renders frames with the software rasteriser.

use super::*;
use buturecad_eda_core::copper::{self, CShape, ItemKind};
use buturecad_eda_core::model::PcbDoc;
use buturecad_eda_core::outline::{arc_points, outline_loops};
use buturecad_geometry::Point;
use buturecad_pcb::{layer, FpShape, LayerId};
use buturecad_viewer3d as v3d;
use buturecad_viewer3d::{Camera, Mesh, Projection, Visibility};
use std::f64::consts::PI;

fn mm(p: Point) -> [f64; 2] {
    [p.x as f64 / 1e6, p.y as f64 / 1e6]
}

fn circle_pts(c: [f64; 2], r: f64, n: usize) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Stadium (capsule) outline of a segment thickened by `r`.
fn capsule_pts(a: [f64; 2], b: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    let ang = (b[1] - a[1]).atan2(b[0] - a[0]);
    let mut v = Vec::new();
    for k in 0..=8 {
        let t = ang - PI / 2.0 + PI * f64::from(k) / 8.0;
        v.push([b[0] + r * t.cos(), b[1] + r * t.sin()]);
    }
    for k in 0..=8 {
        let t = ang + PI / 2.0 + PI * f64::from(k) / 8.0;
        v.push([a[0] + r * t.cos(), a[1] + r * t.sin()]);
    }
    v
}

/// A typical body height for a footprint (mm). **A heuristic**: footprints carry no body height
/// until 3D models are parsed (roadmap 28).
fn body_height(name: &str) -> f64 {
    let n = name.to_ascii_lowercase();
    if n.contains("header") || n.contains("pin") {
        8.5
    } else if n.contains("dip") {
        4.5
    } else if n.contains("soic") {
        1.75
    } else if n.contains("bga") {
        1.2
    } else if n.contains("qfn") {
        0.9
    } else {
        1.0
    }
}

/// Builds the viewer model from the board.
pub(crate) fn build_model(doc: &PcbDoc) -> v3d::Model {
    let mut m = v3d::Model::empty();
    let loops = outline_loops(doc);
    if let Some((outer, cut)) = loops.split_first() {
        m.outline = outer.iter().map(|p| mm(*p)).collect();
        m.cutouts = cut
            .iter()
            .map(|r| r.iter().map(|p| mm(*p)).collect())
            .collect();
    }
    let phys = &doc.physical;
    let cu = |l: LayerId| phys.copper.get(&l).map_or(35_000, |c| c.thickness);
    let (top_cu, bot_cu) = (cu(layer::F_CU), cu(layer::B_CU));
    let mask_t = phys.top_mask.as_ref().map_or(0, |d| d.thickness);
    m.copper_thickness = top_cu as f64 / 1e6;
    m.mask_thickness = mask_t as f64 / 1e6;
    m.thickness = ((phys.thickness()
        - top_cu
        - bot_cu
        - mask_t
        - phys.bottom_mask.as_ref().map_or(0, |d| d.thickness)) as f64
        / 1e6)
        .max(0.1);
    let _ = bot_cu;
    for t in doc.tracks.values() {
        let top = t.layer == layer::F_CU;
        if top || t.layer == layer::B_CU {
            m.tracks.push(v3d::Segment {
                a: mm(t.segment.a),
                b: mm(t.segment.b),
                width: t.width as f64 / 1e6,
                top,
            });
        }
    }
    for v in doc.vias.values() {
        m.vias.push(v3d::ViaHole {
            center: mm(v.position),
            outer_radius: v.diameter as f64 / 2e6,
            drill_radius: v.drill as f64 / 2e6,
        });
    }
    for z in doc.zones.values() {
        let top = z.layer == layer::F_CU;
        if !(top || z.layer == layer::B_CU) {
            continue;
        }
        if let Some(fill) = &z.fill {
            for r in &fill.regions {
                m.zones.push(v3d::Region {
                    outer: r.outer.iter().map(|p| mm(*p)).collect(),
                    holes: r
                        .holes
                        .iter()
                        .map(|h| h.iter().map(|p| mm(*p)).collect())
                        .collect(),
                    top,
                });
            }
        }
    }
    for g in doc.graphics.values() {
        if g.layer == layer::F_SILK || g.layer == layer::B_SILK {
            silk_shape(&g.shape, g.layer == layer::F_SILK, &mut m.silk);
        }
    }
    for (id, f) in &doc.placed {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        // Pads (copper on the outer layers) and the drills of through pads.
        for item in copper::items_of(doc, *id) {
            if item.kind != ItemKind::Pad
                || !(item.layer == layer::F_CU || item.layer == layer::B_CU)
            {
                continue;
            }
            let outline: Vec<[f64; 2]> = match &item.shape {
                CShape::Cap { seg, r } => {
                    if seg.a == seg.b {
                        circle_pts(mm(seg.a), r / 1e6, 20)
                    } else {
                        capsule_pts(mm(seg.a), mm(seg.b), r / 1e6)
                    }
                }
                CShape::Poly(p) => p.vertices().iter().map(|v| mm(*v)).collect(),
                CShape::Region(_) => continue,
            };
            let top = item.layer == layer::F_CU;
            m.pads.push(v3d::Pad {
                outline,
                top,
                bottom: !top,
            });
        }
        for (wp, dp) in f.world_pads(def).iter().zip(&def.pads) {
            if let Some(d) = dp.drill {
                m.vias.push(v3d::ViaHole {
                    center: mm(wp.center),
                    outer_radius: 0.0,
                    drill_radius: d.diameter as f64 / 2e6,
                });
            }
        }
        for g in f.world_graphics(def) {
            if g.layer == layer::F_SILK || g.layer == layer::B_SILK {
                silk_shape(&g.shape, g.layer == layer::F_SILK, &mut m.silk);
            }
        }
        // Body: the courtyard (else the pad extent) as a box.
        let mut outline: Vec<[f64; 2]> = Vec::new();
        for g in f.world_graphics(def) {
            if layer::kind(g.layer) == Some(layer::LayerKind::Courtyard) {
                if let FpShape::Polygon { points, .. } = &g.shape {
                    outline = points.iter().map(|p| mm(*p)).collect();
                    break;
                }
            }
        }
        if outline.len() < 3 {
            let pads = f.world_pads(def);
            let (Some(lo), Some(hi)) = (
                pads.iter()
                    .map(|p| p.bounds().0)
                    .reduce(|a, b| Point::new(a.x.min(b.x), a.y.min(b.y))),
                pads.iter()
                    .map(|p| p.bounds().1)
                    .reduce(|a, b| Point::new(a.x.max(b.x), a.y.max(b.y))),
            ) else {
                continue;
            };
            outline = vec![
                mm(lo),
                mm(Point::new(hi.x, lo.y)),
                mm(hi),
                mm(Point::new(lo.x, hi.y)),
            ];
        }
        m.bodies.push(v3d::Body {
            outline,
            height: body_height(&f.footprint),
            top: !f.placement.flipped,
            has_model: !def.models.is_empty(),
            label: f.reference.clone(),
        });
    }
    m
}

fn silk_shape(shape: &FpShape, top: bool, out: &mut Vec<v3d::Segment>) {
    let seg = |a: Point, b: Point, w: i64, out: &mut Vec<v3d::Segment>| {
        out.push(v3d::Segment {
            a: mm(a),
            b: mm(b),
            width: (w as f64 / 1e6).max(0.1),
            top,
        });
    };
    match shape {
        FpShape::Line { segment, width } => seg(segment.a, segment.b, *width, out),
        FpShape::Polygon { points, width, .. } => {
            for i in 0..points.len() {
                seg(points[i], points[(i + 1) % points.len()], *width, out);
            }
        }
        FpShape::Rect { rect, width, .. } => {
            let (a, b) = (rect.min(), rect.max());
            let c = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
            for i in 0..4 {
                seg(c[i], c[(i + 1) % 4], *width, out);
            }
        }
        FpShape::Circle { circle, width, .. } => {
            let pts = circle_pts(mm(circle.center), circle.radius as f64 / 1e6, 24);
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                out.push(v3d::Segment {
                    a,
                    b,
                    width: (*width as f64 / 1e6).max(0.1),
                    top,
                });
            }
        }
        FpShape::Arc { arc, width } => {
            let pts = arc_points(arc);
            for w in pts.windows(2) {
                seg(w[0], w[1], *width, out);
            }
        }
    }
}

/// Camera, toggles and the cached mesh of the 3D view.
pub(crate) struct Viewer3d {
    pub camera: Camera,
    pub vis: Visibility,
    /// Framed once per opened project.
    pub framed: bool,
    cache: Option<(u64, Visibility, Mesh)>,
    model_info: (usize, usize, usize, usize),
    drag: Option<u8>,
    last: (f64, f64),
}

impl Default for Viewer3d {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            vis: Visibility::default(),
            framed: false,
            cache: None,
            model_info: (0, 0, 0, 0),
            drag: None,
            last: (0.0, 0.0),
        }
    }
}

impl Workspace {
    /// The mesh of the open board, rebuilt only when the project revision or toggles changed.
    fn view3d_mesh(&mut self) -> Option<&Mesh> {
        let open = self.open.as_ref()?;
        let revision = open.engine.revision();
        let vis = self.view3d.vis;
        let stale = self
            .view3d
            .cache
            .as_ref()
            .is_none_or(|(r, v, _)| *r != revision || *v != vis);
        if stale {
            let doc = open.engine.project().pcb();
            let model = build_model(doc);
            if !self.view3d.framed {
                if let Some((lo, hi)) = model.bounds() {
                    self.view3d.camera = Camera::framing(lo, hi);
                    self.view3d.framed = true;
                }
            }
            self.view3d.model_info = (
                model.bodies.len(),
                model.bodies.iter().filter(|b| b.has_model).count(),
                model.tracks.len(),
                model.zones.len(),
            );
            let mesh = v3d::build_mesh(&model, &vis);
            self.view3d.cache = Some((revision, vis, mesh));
        }
        self.view3d.cache.as_ref().map(|(_, _, m)| m)
    }

    /// Makes the cached mesh (and the counts shown in the bar) current.
    pub(crate) fn view3d_refresh(&mut self) {
        self.view3d_mesh();
    }

    pub(crate) fn render_3d(&mut self, w: u32, h: u32) -> Result<(u32, u32, Vec<u8>), String> {
        let bg = match self.theme {
            ThemeName::Dark => [11, 14, 20],
            ThemeName::Light => [225, 229, 236],
        };
        self.view3d_mesh();
        let cam = self.view3d.camera;
        let Some((_, _, mesh)) = self.view3d.cache.as_ref() else {
            return Err("no project is open".into());
        };
        Ok((w, h, v3d::render(mesh, &cam, w, h, bg)))
    }

    /// Camera control: left drag orbits, right/middle or Shift+left drag pans, the wheel zooms.
    pub(crate) fn pointer_3d(&mut self, ev: PointerEvent, out: &mut Outcome) {
        let (hh, ww) = (self.canvas.1.max(1.0), self.canvas.0.max(1.0));
        match ev.kind {
            PointerKind::Down => {
                self.view3d.drag = Some(if ev.button == 0 && !ev.shift { 0 } else { 1 });
                self.view3d.last = (ev.x, ev.y);
            }
            PointerKind::Up | PointerKind::Leave => self.view3d.drag = None,
            PointerKind::Move => {
                if let Some(kind) = self.view3d.drag {
                    let (dx, dy) = (ev.x - self.view3d.last.0, ev.y - self.view3d.last.1);
                    self.view3d.last = (ev.x, ev.y);
                    if kind == 0 {
                        self.view3d.camera.orbit(-dx / ww * 3.0, dy / hh * 3.0);
                    } else {
                        self.view3d.camera.pan(dx / hh, dy / hh);
                    }
                    out.redraw = true;
                }
            }
            PointerKind::Wheel => {
                self.view3d.camera.zoom((ev.delta * 0.0015).exp());
                out.redraw = true;
            }
            PointerKind::DoubleClick => {}
        }
    }

    /// 3D view actions: `projection` (`perspective`/`orthographic`), `view`
    /// (`top`/`bottom`/`front`/`iso`), `show` (`<layer>:<0|1>` for substrate, mask, copper,
    /// silkscreen, components), `frame`.
    pub fn viewer3d_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("").trim();
        let cam = &mut self.view3d.camera;
        match name {
            "projection" => {
                cam.projection = match a {
                    "perspective" => Projection::Perspective,
                    "orthographic" => Projection::Orthographic,
                    _ => return Err("projection is perspective or orthographic".into()),
                }
            }
            "view" => match a {
                "top" => cam.set_view(-PI / 2.0, PI / 2.0),
                "bottom" => cam.set_view(-PI / 2.0, -PI / 2.0),
                "front" => cam.set_view(-PI / 2.0, 0.0),
                "iso" => cam.set_view(-PI / 4.0, 0.6),
                _ => return Err("view is top, bottom, front or iso".into()),
            },
            "frame" => self.view3d.framed = false,
            "show" => {
                let (what, on) = a.split_once(':').ok_or("show takes <layer>:<0|1>")?;
                let on = on == "1";
                let v = &mut self.view3d.vis;
                match what {
                    "substrate" => v.substrate = on,
                    "mask" => v.mask = on,
                    "copper" => v.copper = on,
                    "silkscreen" => v.silkscreen = on,
                    "components" => v.components = on,
                    _ => return Err("unknown 3D layer".into()),
                }
            }
            other => return Err(format!("unknown 3D action: {other}")),
        }
        Ok(())
    }

    pub(crate) fn viewer3d_info(&self) -> Viewer3dInfo {
        let v = self.view3d.vis;
        let (bodies, with_model, tracks, zones) = self.view3d.model_info;
        Viewer3dInfo {
            projection: match self.view3d.camera.projection {
                Projection::Perspective => "perspective",
                Projection::Orthographic => "orthographic",
            }
            .into(),
            substrate: v.substrate,
            mask: v.mask,
            copper: v.copper,
            silkscreen: v.silkscreen,
            components: v.components,
            components_count: bodies as u32,
            with_model_refs: with_model as u32,
            has_outline: self.pcb_doc().is_some_and(|d| !outline_loops(d).is_empty()),
            tracks: tracks as u32,
            zone_pieces: zones as u32,
        }
    }
}
