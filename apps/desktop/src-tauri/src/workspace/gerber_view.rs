//! The integrated Gerber viewer.
//!
//! It shows what the **files** say: the generated Gerber/Excellon bytes (from the manufacturing
//! preview, a folder or a stored `.zip`) are parsed by the independent readers of
//! `buturecad-manufacturing` and drawn by its 2D rasteriser. The PCB model is never drawn here; it
//! is only used by *Verify*, which compares the parsed files with the board.

use super::*;
use buturecad_manufacturing::excellon_read::{self, DrillFile};
use buturecad_manufacturing::gerber_read::{self, ApShape, Image};
use buturecad_manufacturing::raster2d::{self, Content, LayerDraw, Viewport};
use buturecad_manufacturing::{verify, zip};

pub(crate) enum GvData {
    Gerber(Image),
    Drill(DrillFile),
}

pub(crate) struct GvLayer {
    name: String,
    data: GvData,
    visible: bool,
    color: [u8; 3],
}

#[derive(Default)]
pub(crate) struct GerberViewer {
    layers: Vec<GvLayer>,
    errors: Vec<String>,
    source: String,
    selected: Option<usize>,
    view: Option<([f64; 2], f64)>,
    drag: bool,
    last: (f64, f64),
    checks: Vec<verify::Check>,
}

fn color_for(name: &str, function: Option<&str>) -> [u8; 3] {
    let f = function.unwrap_or("");
    let n = name.to_ascii_lowercase();
    if f.starts_with("Copper") && f.ends_with("Top") {
        [214, 70, 60]
    } else if f.starts_with("Copper") && f.ends_with("Bot") {
        [70, 120, 220]
    } else if f.starts_with("Copper") {
        [200, 160, 60]
    } else if f.starts_with("Soldermask") {
        [40, 160, 90]
    } else if f.starts_with("Paste") {
        [160, 160, 170]
    } else if f.starts_with("Legend") {
        [235, 235, 210]
    } else if f.starts_with("Profile") {
        [235, 200, 60]
    } else if n.ends_with(".drl") {
        [250, 250, 250]
    } else {
        [180, 120, 220]
    }
}

fn is_gerber(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "gbr"
            | "ger"
            | "gtl"
            | "gbl"
            | "gts"
            | "gbs"
            | "gtp"
            | "gbp"
            | "gto"
            | "gbo"
            | "gko"
            | "gm1"
    ) || (ext.len() == 2 && ext.starts_with('g') && ext[1..].chars().all(|c| c.is_ascii_digit()))
}

fn is_drill(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "drl" | "xln" | "exc")
}

fn shape_text(s: &ApShape) -> String {
    let mm = |v: i64| format!("{:.4}", v as f64 / 1e6);
    match s {
        ApShape::Circle { d } => format!("circle Ø{} mm", mm(*d)),
        ApShape::Rect { w, h } => format!("rect {}×{} mm", mm(*w), mm(*h)),
        ApShape::Obround { w, h } => format!("obround {}×{} mm", mm(*w), mm(*h)),
        ApShape::Polygon { d, n, .. } => format!("polygon {n}-gon Ø{} mm", mm(*d)),
        ApShape::Unsupported(n) => format!("unsupported ({n})"),
    }
}

impl GerberViewer {
    fn load(&mut self, mut files: Vec<(String, Vec<u8>)>, source: String) {
        files.sort_by(|a, b| a.0.cmp(&b.0));
        self.layers.clear();
        self.errors.clear();
        self.checks.clear();
        self.selected = None;
        self.view = None;
        self.source = source;
        // Draw order: copper bottom→top is not known from names alone, so use the file function.
        for (name, bytes) in files {
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if is_gerber(&name) {
                match gerber_read::parse(text) {
                    Ok(img) => {
                        let color = color_for(&name, img.function.as_deref());
                        self.layers.push(GvLayer {
                            name,
                            data: GvData::Gerber(img),
                            visible: true,
                            color,
                        });
                    }
                    Err(e) => self.errors.push(format!("{name}: {e}")),
                }
            } else if is_drill(&name) {
                match excellon_read::parse(text) {
                    Ok(d) => {
                        let color = color_for(&name, d.function.as_deref());
                        self.layers.push(GvLayer {
                            name,
                            data: GvData::Drill(d),
                            visible: true,
                            color,
                        });
                    }
                    Err(e) => self.errors.push(format!("{name}: {e}")),
                }
            }
        }
        let rank = |l: &GvLayer| match &l.data {
            GvData::Drill(_) => 9,
            GvData::Gerber(i) => match i.function.as_deref().unwrap_or("") {
                f if f.starts_with("Copper") && f.ends_with("Bot") => 0,
                f if f.starts_with("Copper") => 1,
                f if f.starts_with("Soldermask") => 2,
                f if f.starts_with("Paste") => 3,
                f if f.starts_with("Legend") => 4,
                f if f.starts_with("Profile") => 5,
                _ => 6,
            },
        };
        self.layers.sort_by_key(|l| (rank(l), l.name.clone()));
        if !self.layers.is_empty() {
            self.selected = Some(0);
        }
    }

    fn draws(&self) -> Vec<LayerDraw<'_>> {
        self.layers
            .iter()
            .filter(|l| l.visible)
            .map(|l| LayerDraw {
                content: match &l.data {
                    GvData::Gerber(i) => Content::Gerber(i),
                    GvData::Drill(d) => Content::Drill(d),
                },
                color: l.color,
                alpha: 0.78,
            })
            .collect()
    }
}

fn read_dir_files(dir: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        if e.file_type().map_err(|e| e.to_string())?.is_file() {
            let meta = e.metadata().map_err(|e| e.to_string())?;
            if meta.len() > 64 * 1024 * 1024 {
                continue;
            }
            out.push((
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).map_err(|e| e.to_string())?,
            ));
        }
    }
    Ok(out)
}

impl Workspace {
    /// `load_preview`, `open <path>` (folder or stored zip), `toggle <i>:<0|1>`, `select <i>`,
    /// `fit`, `all <0|1>`, `verify`.
    pub fn gerber_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("").trim();
        match name {
            "load_preview" => {
                self.fab_preview()?;
                let files: Vec<(String, Vec<u8>)> = {
                    let (_, g) = self.fab.generated.as_ref().ok_or("no preview")?;
                    g.files
                        .iter()
                        .map(|f| (f.name.clone(), f.bytes.clone()))
                        .collect()
                };
                self.gv.load(files, "current manufacturing preview".into());
                self.gerber_verify()?;
            }
            "open" => {
                let p = std::path::Path::new(a);
                let files = if p.is_dir() {
                    read_dir_files(p)?
                } else {
                    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
                    zip::read_stored(&bytes).ok_or(
                        "only stored (uncompressed) ZIP archives such as ButureCAD's production.zip can be read; unzip other archives and open the folder",
                    )?
                };
                self.gv.load(files, a.to_string());
            }
            "toggle" => {
                let (i, on) = a.split_once(':').ok_or("toggle takes <index>:<0|1>")?;
                let i: usize = i.parse().map_err(|_| "bad layer index")?;
                self.gv.layers.get_mut(i).ok_or("no such layer")?.visible = on == "1";
            }
            "select" => {
                let i: usize = a.parse().map_err(|_| "bad layer index")?;
                if i >= self.gv.layers.len() {
                    return Err("no such layer".into());
                }
                self.gv.selected = Some(i);
            }
            "all" => {
                for l in &mut self.gv.layers {
                    l.visible = a == "1";
                }
            }
            "fit" => self.gv.view = None,
            "verify" => self.gerber_verify()?,
            other => return Err(format!("unknown Gerber viewer action: {other}")),
        }
        Ok(())
    }

    /// Parses the *files currently shown* against the board: positions, counts, profile, drills.
    fn gerber_verify(&mut self) -> Result<(), String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let input = buturecad_manufacturing::board::build_fab_input_for_project(
            open.engine.project(),
            open.engine.project().name(),
            &Default::default(),
        );
        let Some((_, g)) = self.fab.generated.as_ref() else {
            return Err(
                "verification compares the current manufacturing preview; run it first".into(),
            );
        };
        if self.gv.source != "current manufacturing preview" {
            return Err(
                "verification is only available for the current manufacturing preview".into(),
            );
        }
        let files: Vec<(String, Vec<u8>)> = g
            .files
            .iter()
            .map(|f| (f.name.clone(), f.bytes.clone()))
            .collect();
        self.gv.checks = verify::check(&input, &files);
        let bad = self.gv.checks.iter().filter(|c| !c.ok).count();
        self.log(
            if bad > 0 {
                MessageLevel::Warn
            } else {
                MessageLevel::Info
            },
            format!(
                "Gerber verification: {} check(s), {bad} mismatch(es)",
                self.gv.checks.len()
            ),
        );
        Ok(())
    }

    pub(crate) fn render_gerber(&mut self, w: u32, h: u32) -> Result<(u32, u32, Vec<u8>), String> {
        let bg = match self.theme {
            ThemeName::Dark => [11, 14, 20],
            ThemeName::Light => [235, 238, 243],
        };
        if self.gv.view.is_none() {
            if let Some((lo, hi)) = raster2d::bounds(&self.gv.draws()) {
                let vp = Viewport::fit(lo, hi, w, h);
                self.gv.view = Some((vp.center, vp.scale));
            }
        }
        let draws = self.gv.draws();
        let (center, scale) = self.gv.view.unwrap_or(([0.0, 0.0], 1e-5));
        let vp = Viewport {
            center,
            scale,
            width: w,
            height: h,
        };
        Ok((w, h, raster2d::render(&draws, &vp, bg)))
    }

    pub(crate) fn pointer_gerber(&mut self, ev: PointerEvent, out: &mut Outcome) {
        let (w, h) = (self.canvas.0.max(1.0) as u32, self.canvas.1.max(1.0) as u32);
        let Some((center, scale)) = self.gv.view else {
            return;
        };
        let mut vp = Viewport {
            center,
            scale,
            width: w,
            height: h,
        };
        match ev.kind {
            PointerKind::Down => {
                self.gv.drag = true;
                self.gv.last = (ev.x, ev.y);
            }
            PointerKind::Up | PointerKind::Leave => self.gv.drag = false,
            PointerKind::Move if self.gv.drag => {
                vp.pan_pixels(ev.x - self.gv.last.0, ev.y - self.gv.last.1);
                self.gv.last = (ev.x, ev.y);
                out.redraw = true;
            }
            PointerKind::Wheel => {
                vp.zoom_at(ev.x, ev.y, (-ev.delta * 0.0015).exp());
                out.redraw = true;
            }
            _ => {}
        }
        self.gv.view = Some((vp.center, vp.scale));
    }

    pub(crate) fn gerber_info(&self) -> GerberViewerInfo {
        let g = &self.gv;
        let layers: Vec<GerberLayerInfo> = g
            .layers
            .iter()
            .map(|l| {
                let (kind, summary, warnings) = match &l.data {
                    GvData::Gerber(i) => {
                        let (f, s, r) = i.count();
                        (
                            "gerber",
                            format!(
                                "{} — {f} flashes, {s} strokes, {r} contours",
                                i.function.as_deref().unwrap_or("no FileFunction")
                            ),
                            i.warnings.clone(),
                        )
                    }
                    GvData::Drill(d) => (
                        "drill",
                        format!(
                            "{} — {} holes, {} tools",
                            d.function.as_deref().unwrap_or("no FileFunction"),
                            d.hits.len(),
                            d.tools.len()
                        ),
                        d.warnings.clone(),
                    ),
                };
                GerberLayerInfo {
                    name: l.name.clone(),
                    kind: kind.into(),
                    visible: l.visible,
                    color: format!("#{:02x}{:02x}{:02x}", l.color[0], l.color[1], l.color[2]),
                    summary,
                    warnings,
                }
            })
            .collect();
        let mut apertures = Vec::new();
        let mut tools = Vec::new();
        if let Some(l) = g.selected.and_then(|i| g.layers.get(i)) {
            match &l.data {
                GvData::Gerber(i) => {
                    for a in &i.apertures {
                        apertures.push(GerberApertureInfo {
                            code: format!("D{}", a.code),
                            shape: shape_text(&a.shape),
                            uses: a.uses as u32,
                        });
                    }
                }
                GvData::Drill(d) => {
                    for (n, dia) in &d.tools {
                        tools.push(GerberToolInfo {
                            tool: format!("T{n}"),
                            diameter_mm: format!("{:.4}", *dia as f64 / 1e6),
                            holes: d.hits.iter().filter(|h| h.tool == *n).count() as u32,
                        });
                    }
                }
            }
        }
        GerberViewerInfo {
            loaded: !g.layers.is_empty(),
            source: g.source.clone(),
            layers,
            selected: g.selected.map(|i| i as u32),
            apertures,
            tools,
            errors: g.errors.clone(),
            checks: g
                .checks
                .iter()
                .map(|c| GerberCheckInfo {
                    ok: c.ok,
                    text: c.text.clone(),
                })
                .collect(),
        }
    }
}
