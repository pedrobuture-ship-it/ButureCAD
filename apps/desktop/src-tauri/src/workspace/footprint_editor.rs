//! Footprint Editor: the interaction layer over `buturecad_pcb::footprint_edit`.
//!
//! Mirrors the Symbol Editor: the footprint being edited lives in the project's embedded
//! footprint table, so every edit is one `SetFootprintCommand` through the Command Engine (undo,
//! redo, dirty state, autosave and crash recovery for free). Libraries are source and sink:
//! opening copies a library footprint into the project, "Save to library" writes it back.
//! Live previews (dragging, a primitive being drawn) never touch the model.

use super::*;
use crate::pcb_scene::rounded_rect;
use crate::scene_builder::{self as sb, ThemeKind};
use buturecad_eda_core::commands::SetFootprintCommand;
use buturecad_geometry::{Arc, Circle, Polygon, Rectangle, Segment};
use buturecad_libraries::{qualify, split_lib_id, LibraryDir};
use buturecad_pcb::footprint_edit::{self as fe, Item};
use buturecad_pcb::generators;
use buturecad_pcb::{
    Footprint, FpGraphic, FpLayer, FpShape, FpText, MountType, Pad, PadKind, PadShape, TextKind,
};
use buturecad_renderer::{Item as SceneItem, Layer, LayerId, Rgba, Shape};

/// Item ids: pads are `1 + index`, graphics `GFX_BASE + index`, texts `TXT_BASE + index`.
const GFX_BASE: u64 = 1 << 20;
const TXT_BASE: u64 = 1 << 21;

const L_HIT: LayerId = LayerId(1);
const L_COURT: LayerId = LayerId(20);
const L_FAB: LayerId = LayerId(22);
const L_MASK: LayerId = LayerId(24);
const L_PASTE: LayerId = LayerId(26);
const L_COPPER: LayerId = LayerId(30);
const L_SILK: LayerId = LayerId(34);
const L_DRILL: LayerId = LayerId(36);
const L_PADTEXT: LayerId = LayerId(38);

const DEFAULT_LINE: i64 = 120_000;

fn scene_layer(l: FpLayer) -> LayerId {
    match l {
        FpLayer::Silkscreen => L_SILK,
        FpLayer::Fabrication => L_FAB,
        FpLayer::Courtyard => L_COURT,
        FpLayer::Mask => L_MASK,
        FpLayer::Paste => L_PASTE,
    }
}

pub(crate) fn id_of(item: Item) -> Id {
    match item {
        Item::Pad(i) => Id(1 + i as u64),
        Item::Graphic(i) => Id(GFX_BASE + i as u64),
        Item::Text(i) => Id(TXT_BASE + i as u64),
    }
}

pub(crate) fn item_of(id: Id) -> Option<Item> {
    match id.0 {
        0 => None,
        n if n >= TXT_BASE => Some(Item::Text((n - TXT_BASE) as usize)),
        n if n >= GFX_BASE => Some(Item::Graphic((n - GFX_BASE) as usize)),
        n => Some(Item::Pad((n - 1) as usize)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum FpTool {
    #[default]
    Select,
    PadSmd,
    PadThrough,
    Npth,
    Line,
    Rect,
    Circle,
    Arc,
    Text(String),
}

impl FpTool {
    pub fn id(&self) -> &'static str {
        match self {
            FpTool::Select => "select",
            FpTool::PadSmd => "pad_smd",
            FpTool::PadThrough => "pad_through",
            FpTool::Npth => "npth",
            FpTool::Line => "line",
            FpTool::Rect => "rect",
            FpTool::Circle => "circle",
            FpTool::Arc => "arc",
            FpTool::Text(_) => "text",
        }
    }
    pub fn arg(&self) -> Option<String> {
        match self {
            FpTool::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
}

/// Attributes of the next pad placed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PadTemplate {
    pub shape: PadShape,
    pub size_x: i64,
    pub size_y: i64,
    pub drill: i64,
    pub ratio: u16,
}

impl Default for PadTemplate {
    fn default() -> Self {
        Self {
            shape: PadShape::Rect,
            size_x: 1_500_000,
            size_y: 600_000,
            drill: 800_000,
            ratio: 250,
        }
    }
}

#[derive(Debug, Clone)]
enum Drag {
    Pending {
        start_px: (f64, f64),
        start: Point,
        items: Vec<Item>,
    },
    Moving {
        start: Point,
        items: Vec<Item>,
    },
}

pub(crate) struct FootprintEditor {
    /// Key of the edited footprint in the project's footprint table.
    pub name: Option<String>,
    pub tool: FpTool,
    /// Layer new graphics and texts go on.
    pub layer: FpLayer,
    pub template: PadTemplate,
    hidden: BTreeSet<FpLayer>,
    pts: Vec<Point>,
    drag: Option<Drag>,
    preview: Option<Footprint>,
    pub scene: Scene,
    owners: BTreeMap<Id, Vec<ObjectId>>,
    built: Option<(u64, Option<String>, ThemeName, u64)>,
    version: u64,
}

impl Default for FootprintEditor {
    fn default() -> Self {
        Self {
            name: None,
            tool: FpTool::Select,
            layer: FpLayer::Silkscreen,
            template: PadTemplate::default(),
            hidden: BTreeSet::new(),
            pts: Vec::new(),
            drag: None,
            preview: None,
            scene: Scene::new(),
            owners: BTreeMap::new(),
            built: None,
            version: 0,
        }
    }
}

impl FootprintEditor {
    pub fn shapes_of(&self, owner: Id) -> &[ObjectId] {
        self.owners.get(&owner).map_or(&[], Vec::as_slice)
    }
}

// ---------------------------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------------------------

fn text_width(text: &str, h: i64) -> i64 {
    text.chars().count() as i64 * 6 * (h / 7)
}

fn fp_layers(theme: ThemeKind, hidden: &BTreeSet<FpLayer>) -> Vec<(LayerId, Layer)> {
    let dark = theme == ThemeKind::Dark;
    let c = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { l };
        Rgba::rgb(r, g, b)
    };
    let vis = |l: FpLayer| !hidden.contains(&l);
    vec![
        (
            L_HIT,
            Layer::new("Hit areas", Rgba::rgb(0, 0, 0)).hit_only(),
        ),
        (
            L_COURT,
            Layer::new("Courtyard", c((220, 90, 220), (160, 40, 160)))
                .with_visible(vis(FpLayer::Courtyard)),
        ),
        (
            L_FAB,
            Layer::new("Fabrication", c((140, 160, 200), (70, 90, 140)))
                .with_visible(vis(FpLayer::Fabrication)),
        ),
        (
            L_MASK,
            Layer::new("Solder mask", c((170, 110, 230), (110, 60, 180)))
                .with_visible(vis(FpLayer::Mask))
                .not_snappable(),
        ),
        (
            L_PASTE,
            Layer::new("Solder paste", c((150, 150, 150), (110, 110, 110)))
                .with_visible(vis(FpLayer::Paste))
                .not_snappable(),
        ),
        (
            L_COPPER,
            Layer::new("Pads (copper)", c((222, 70, 70), (190, 30, 30))),
        ),
        (
            L_SILK,
            Layer::new("Silkscreen", c((90, 220, 230), (0, 130, 150)))
                .with_visible(vis(FpLayer::Silkscreen)),
        ),
        (
            L_DRILL,
            Layer::new("Drill holes", c((235, 200, 70), (150, 110, 0))).not_snappable(),
        ),
        (
            L_PADTEXT,
            Layer::new("Pad numbers", c((250, 235, 235), (60, 10, 10))).not_snappable(),
        ),
    ]
}

/// Render shape of a pad (copper outline), optionally grown by `grow` nm on every side.
fn pad_shape(p: &Pad, grow: i64, filled: bool) -> Option<Shape> {
    let (w, h) = p.rotated_size();
    let (w, h) = (w + 2 * grow, h + 2 * grow);
    if w <= 0 || h <= 0 {
        return None;
    }
    let stroke = if filled { 0 } else { 50_000 };
    let poly = |pts: Vec<Point>| {
        Polygon::new(pts).ok().map(|polygon| Shape::Polygon {
            polygon,
            filled,
            stroke,
        })
    };
    match p.shape {
        PadShape::Circle => Some(Shape::Circle {
            circle: Circle {
                center: p.position,
                radius: w / 2,
            },
            filled,
            stroke,
        }),
        PadShape::Rect => Rectangle::from_center(p.position, w, h)
            .ok()
            .map(|rect| Shape::Rect {
                rect,
                filled,
                stroke,
            }),
        PadShape::Oval => poly(rounded_rect(p.position, w, h, w.min(h) / 2)),
        PadShape::RoundRect => {
            let r = i64::from(p.roundrect_ratio) * p.size_x.min(p.size_y) / 1000 + grow;
            poly(rounded_rect(p.position, w, h, r))
        }
        PadShape::Custom => {
            let pts: Vec<Point> = p
                .custom
                .iter()
                .map(|q| {
                    let q = q.rotate_quarter_turns(Point::ORIGIN, p.rotation % 4);
                    Point::new(p.position.x + q.x, p.position.y + q.y)
                })
                .collect();
            poly(pts)
        }
    }
}

fn pad_shapes(p: &Pad, with_mask_paste: bool) -> Vec<(LayerId, Shape)> {
    let mut out = Vec::new();
    if p.kind != PadKind::NonPlated {
        if let Some(s) = pad_shape(p, 0, true) {
            out.push((L_COPPER, s));
        }
    }
    if with_mask_paste {
        // Mask opening (outline) and paste aperture (outline), at the pad's own margins.
        if let Some(s) = pad_shape(p, p.mask_margin.unwrap_or(0), false) {
            out.push((L_MASK, s));
        }
        if p.kind == PadKind::Smd && p.paste {
            if let Some(s) = pad_shape(p, p.paste_margin.unwrap_or(0), false) {
                out.push((L_PASTE, s));
            }
        }
    }
    if let Some(d) = p.drill {
        // Round holes are circles; slots are drawn as a stadium along the pad's long axis.
        let long = d.slot.unwrap_or(d.diameter);
        let (w, h) = if p.rotated_size().0 >= p.rotated_size().1 {
            (long, d.diameter)
        } else {
            (d.diameter, long)
        };
        if d.slot.is_some() {
            if let Ok(polygon) = Polygon::new(rounded_rect(p.position, w, h, w.min(h) / 2)) {
                out.push((
                    L_DRILL,
                    Shape::Polygon {
                        polygon,
                        filled: true,
                        stroke: 0,
                    },
                ));
            }
        } else {
            out.push((
                L_DRILL,
                Shape::Circle {
                    circle: Circle {
                        center: p.position,
                        radius: d.diameter / 2,
                    },
                    filled: true,
                    stroke: 0,
                },
            ));
        }
    }
    if !p.number.is_empty() {
        let (w, h) = p.rotated_size();
        let th = (w.min(h) / 2).clamp(300_000, 900_000);
        let tw = text_width(&p.number, th);
        out.push((
            L_PADTEXT,
            Shape::Text {
                origin: Point::new(p.position.x - tw / 2, p.position.y - th / 2),
                height: th,
                text: p.number.clone(),
            },
        ));
    }
    out
}

fn graphic_shapes(g: &FpGraphic) -> Vec<(LayerId, Shape)> {
    let layer = scene_layer(g.layer);
    let mut out = Vec::new();
    match &g.shape {
        FpShape::Line { segment, width } => out.push((
            layer,
            Shape::Line {
                segment: *segment,
                width: *width,
            },
        )),
        FpShape::Rect {
            rect,
            filled,
            width,
        } => out.push((
            layer,
            Shape::Rect {
                rect: *rect,
                filled: *filled,
                stroke: *width,
            },
        )),
        FpShape::Circle {
            circle,
            filled,
            width,
        } => out.push((
            layer,
            Shape::Circle {
                circle: *circle,
                filled: *filled,
                stroke: *width,
            },
        )),
        FpShape::Arc { arc, width } => out.push((
            layer,
            Shape::Arc {
                arc: *arc,
                width: *width,
            },
        )),
        FpShape::Polygon {
            points,
            filled,
            width,
        } => {
            if let Ok(polygon) = Polygon::new(points.clone()) {
                out.push((
                    layer,
                    Shape::Polygon {
                        polygon,
                        filled: *filled,
                        stroke: *width,
                    },
                ));
            }
        }
    }
    out
}

fn text_shapes(t: &FpText) -> Vec<(LayerId, Shape)> {
    let w = text_width(&t.text, t.height);
    let mut out = vec![(
        scene_layer(t.layer),
        Shape::Text {
            origin: Point::new(t.position.x - w / 2, t.position.y - t.height / 2),
            height: t.height,
            text: t.text.clone(),
        },
    )];
    if let Ok(r) = Rectangle::from_center(t.position, w + 100_000, t.height + 100_000) {
        out.push((
            L_HIT,
            Shape::Rect {
                rect: r,
                filled: true,
                stroke: 0,
            },
        ));
    }
    out
}

fn build_scene(
    fp: &Footprint,
    theme: ThemeKind,
    hidden: &BTreeSet<FpLayer>,
) -> (Scene, BTreeMap<Id, Vec<ObjectId>>) {
    let mut scene = Scene::new();
    for (id, layer) in fp_layers(theme, hidden) {
        scene.set_layer(id, layer);
    }
    let mut owners: BTreeMap<Id, Vec<ObjectId>> = BTreeMap::new();
    let mut add = |scene: &mut Scene, owner: Id, shapes: Vec<(LayerId, Shape)>| {
        let mut ids = Vec::new();
        for (n, (layer, shape)) in shapes.into_iter().enumerate() {
            let sid = sb::shape_id(owner, n as u64);
            scene.insert(
                sid,
                SceneItem {
                    layer,
                    shape,
                    color: None,
                },
            );
            ids.push(sid);
        }
        owners.insert(owner, ids);
    };
    let show_mask = !hidden.contains(&FpLayer::Mask);
    for (i, p) in fp.pads.iter().enumerate() {
        add(&mut scene, id_of(Item::Pad(i)), pad_shapes(p, show_mask));
    }
    for (i, g) in fp.graphics.iter().enumerate() {
        add(&mut scene, id_of(Item::Graphic(i)), graphic_shapes(g));
    }
    for (i, t) in fp.texts.iter().enumerate() {
        add(&mut scene, id_of(Item::Text(i)), text_shapes(t));
    }
    (scene, owners)
}

/// The generator catalogue as the UI shows it (independent of any open footprint).
pub fn generator_infos() -> Vec<GeneratorInfo> {
    generators::catalog()
        .into_iter()
        .map(|g| GeneratorInfo {
            family: g.family.into(),
            title: g.title.into(),
            params: g
                .params
                .iter()
                .map(|p| GeneratorParamInfo {
                    key: p.key.into(),
                    label: p.label.into(),
                    default: p.default.into(),
                    kind: match p.kind {
                        generators::ParamKind::Integer => "integer",
                        generators::ParamKind::Millimetres => "millimetres",
                        generators::ParamKind::Flag => "flag",
                    }
                    .into(),
                })
                .collect(),
        })
        .collect()
}

fn mm_str(v: i64) -> String {
    let s = format!("{:.4}", v as f64 / 1e6);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s.to_owned()
    }
}

fn parse_mm(text: &str, what: &str) -> Result<i64, String> {
    let t = text.trim().trim_end_matches("mm").trim();
    let v: f64 = t
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("{what} must be a number in mm"))?;
    if !v.is_finite() || v.abs() > 1000.0 {
        return Err(format!("{what} is out of range"));
    }
    Ok((v * 1e6).round() as i64)
}

fn prop(key: &str, label: &str, value: impl Into<String>, editable: bool) -> PropertyInfo {
    PropertyInfo {
        key: key.into(),
        label: label.into(),
        value: value.into(),
        editable,
        options: Vec::new(),
    }
}

fn choice(
    key: &str,
    label: &str,
    value: impl Into<String>,
    options: &[(&str, &str)],
) -> PropertyInfo {
    PropertyInfo {
        options: options
            .iter()
            .map(|(v, l)| OptionInfo {
                value: (*v).into(),
                label: (*l).into(),
            })
            .collect(),
        ..prop(key, label, value, true)
    }
}

fn kind_options() -> Vec<(&'static str, &'static str)> {
    fe::PAD_KINDS
        .iter()
        .map(|k| (fe::pad_kind_id(*k), fe::pad_kind_label(*k)))
        .collect()
}

fn shape_options() -> Vec<(&'static str, &'static str)> {
    fe::PAD_SHAPES
        .iter()
        .map(|s| (fe::pad_shape_id(*s), fe::pad_shape_label(*s)))
        .collect()
}

fn layer_options() -> Vec<(&'static str, &'static str)> {
    FpLayer::ALL.iter().map(|l| (l.id(), l.label())).collect()
}

const YES_NO: [(&str, &str); 2] = [("true", "Yes"), ("false", "No")];
const ROTATIONS: [(&str, &str); 4] = [("0", "0°"), ("90", "90°"), ("180", "180°"), ("270", "270°")];

impl Workspace {
    // -- access ----------------------------------------------------------------------------------

    pub(crate) fn fped_footprint(&self) -> Option<&Footprint> {
        let name = self.fped.name.as_ref()?;
        self.open
            .as_ref()?
            .engine
            .project()
            .pcb()
            .footprints
            .get(name)
    }

    fn fped_shown(&self) -> Option<&Footprint> {
        self.fped.preview.as_ref().or_else(|| self.fped_footprint())
    }

    fn fped_require(&self) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no project is open".into());
        }
        if self.fped.name.is_none() {
            return Err("no footprint is open in the Footprint Editor".into());
        }
        Ok(())
    }

    pub(crate) fn fped_ensure_scene(&mut self) {
        let Some(open) = &self.open else { return };
        let key = (
            open.engine.revision(),
            self.fped.name.clone(),
            self.theme,
            self.fped.version,
        );
        if self.fped.built.as_ref() == Some(&key) {
            return;
        }
        let theme = self.theme_kind();
        let (scene, owners) = match self.fped_shown() {
            Some(fp) => build_scene(fp, theme, &self.fped.hidden),
            None => (Scene::new(), BTreeMap::new()),
        };
        self.fped.scene = scene;
        self.fped.owners = owners;
        self.fped.built = Some(key);
    }

    fn fped_items(&self) -> Vec<Item> {
        let Some(fp) = self.fped_footprint() else {
            return Vec::new();
        };
        self.selection
            .get(&Mode::FootprintEditor)
            .into_iter()
            .flatten()
            .filter_map(|id| item_of(*id))
            .filter(|it| match it {
                Item::Pad(i) => *i < fp.pads.len(),
                Item::Graphic(i) => *i < fp.graphics.len(),
                Item::Text(i) => *i < fp.texts.len(),
            })
            .collect()
    }

    fn fped_select(&mut self, items: &[Item]) {
        self.selection.insert(
            Mode::FootprintEditor,
            items.iter().map(|i| id_of(*i)).collect(),
        );
    }

    // -- editing ---------------------------------------------------------------------------------

    fn fped_edit<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Footprint) -> Result<R, buturecad_pcb::FootprintError>,
    ) -> Result<R, String> {
        let name = self.fped.name.clone().ok_or("no footprint is open")?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before = open
            .engine
            .project()
            .pcb()
            .footprints
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("footprint {name} is no longer in the project"))?;
        let mut after = before.clone();
        let result = f(&mut after).map_err(|e| e.to_string())?;
        if after != before {
            open.engine
                .execute(
                    SetFootprintCommand::new(&name, Some(before), Some(after)).with_label(label),
                )
                .map_err(|e| e.to_string())?;
            self.fped.version += 1;
        }
        self.command_log.push(label.to_owned());
        Ok(result)
    }

    fn fped_log_err(&mut self, label: &str, e: String) {
        self.log(MessageLevel::Error, format!("{label}: {e}"));
    }

    fn fped_add_graphic(&mut self, label: &str, shape: FpShape) {
        let g = FpGraphic {
            layer: self.fped.layer,
            shape,
        };
        match self.fped_edit(label, |f| fe::add_graphic(f, g)) {
            Ok(i) => self.fped_select(&[Item::Graphic(i)]),
            Err(e) => self.fped_log_err(label, e),
        }
    }

    fn fped_add_pad(&mut self, at: Point, kind: PadKind) {
        let t = self.fped.template;
        let result = self.fped_edit("Add pad", |f| {
            let mut pad = match kind {
                PadKind::Smd => {
                    let mut p = Pad::smd(fe::next_pad_number(f), at, t.size_x, t.size_y);
                    p.shape = t.shape;
                    p
                }
                PadKind::Through => {
                    let d = t.drill.min(t.size_x.min(t.size_y) - 100_000).max(100_000);
                    let shape = if t.shape == PadShape::Custom {
                        PadShape::Circle
                    } else {
                        t.shape
                    };
                    Pad::through(fe::next_pad_number(f), shape, at, t.size_x, t.size_y, d)
                }
                PadKind::NonPlated => Pad::npth(at, t.drill),
            };
            if pad.shape == PadShape::Circle {
                let d = pad.size_x.max(pad.size_y);
                pad.size_x = d;
                pad.size_y = d;
            }
            if pad.shape == PadShape::RoundRect {
                pad.roundrect_ratio = t.ratio;
            }
            if pad.shape == PadShape::Custom {
                let (hx, hy) = (pad.size_x / 2, pad.size_y / 2);
                pad.custom = vec![
                    Point::new(-hx, -hy),
                    Point::new(hx, -hy),
                    Point::new(hx, hy),
                    Point::new(-hx, hy),
                ];
            }
            fe::add_pad(f, pad)
        });
        match result {
            Ok(i) => self.fped_select(&[Item::Pad(i)]),
            Err(e) => self.fped_log_err("Add pad", e),
        }
    }

    fn fped_delete(&mut self) -> Result<(), String> {
        let items = self.fped_items();
        if items.is_empty() {
            return Err("select something to delete".into());
        }
        let n = self.fped_edit("Delete items", |f| fe::delete_items(f, &items))?;
        self.selection
            .insert(Mode::FootprintEditor, BTreeSet::new());
        self.log(MessageLevel::Info, format!("Deleted {n} item(s)"));
        Ok(())
    }

    fn fped_transform(&mut self, rotate: bool, horizontal: bool) -> Result<(), String> {
        let items = self.fped_items();
        if items.is_empty() {
            return Err("select something first".into());
        }
        let fp = self.fped_footprint().ok_or("no footprint is open")?;
        let (lo, hi) = fe::items_bounds(fp, &items).ok_or("nothing to transform")?;
        let g = self.grid.spacing.max(1);
        let snap = |v: i64| (v as f64 / g as f64).round() as i64 * g;
        let pivot = Point::new(snap((lo.x + hi.x) / 2), snap((lo.y + hi.y) / 2));
        if rotate {
            self.fped_edit("Rotate items", |f| fe::rotate_items(f, &items, pivot))
        } else if horizontal {
            self.fped_edit("Mirror left-right", |f| {
                fe::mirror_items(f, &items, pivot, true)
            })
        } else {
            self.fped_edit("Mirror top-bottom", |f| {
                fe::mirror_items(f, &items, pivot, false)
            })
        }
    }

    // -- item properties -------------------------------------------------------------------------

    /// The Inspector edits lengths in mm; the model stores nm.
    fn fped_normalise(key: &str, value: &str) -> Result<(String, String), String> {
        let mm_keys = [
            ("x_mm", "x"),
            ("y_mm", "y"),
            ("size_x_mm", "size_x"),
            ("size_y_mm", "size_y"),
            ("drill_mm", "drill"),
            ("slot_mm", "slot"),
            ("mask_margin_mm", "mask_margin"),
            ("paste_margin_mm", "paste_margin"),
            ("width_mm", "width"),
            ("height_mm", "height"),
        ];
        for (k, to) in mm_keys {
            if key == k {
                // Empty stays empty: it clears optional values (drill, slot, margins).
                let v = if value.trim().is_empty() {
                    String::new()
                } else {
                    parse_mm(value, to)?.to_string()
                };
                return Ok((to.to_owned(), v));
            }
        }
        Ok((key.to_owned(), value.to_owned()))
    }

    pub(crate) fn fped_set_item_property(
        &mut self,
        id: u64,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        self.fped_require()?;
        let item = item_of(Id(id)).ok_or("that is not a footprint item")?;
        let (key, value) = Self::fped_normalise(key, value)?;
        let label = format!("Edit {key}");
        match item {
            Item::Pad(i) => self.fped_edit(&label, |f| fe::set_pad_field(f, i, &key, &value)),
            Item::Graphic(i) => {
                self.fped_edit(&label, |f| fe::set_graphic_field(f, i, &key, &value))
            }
            Item::Text(i) => self.fped_edit(&label, |f| fe::set_text_field(f, i, &key, &value)),
        }
    }

    pub(crate) fn fped_summaries(&self) -> Vec<ObjectSummary> {
        let Some(fp) = self.fped_footprint() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in self.fped_items() {
            let id = id_of(item);
            match item {
                Item::Pad(i) => {
                    let Some(p) = fp.pads.get(i) else { continue };
                    let mut props = vec![
                        prop("number", "Number", &p.number, p.kind != PadKind::NonPlated),
                        choice("kind", "Type", fe::pad_kind_id(p.kind), &kind_options()),
                        choice(
                            "shape",
                            "Shape",
                            fe::pad_shape_id(p.shape),
                            &shape_options(),
                        ),
                        prop("x_mm", "X (mm)", mm_str(p.position.x), true),
                        prop("y_mm", "Y (mm)", mm_str(p.position.y), true),
                        prop("size_x_mm", "Width (mm)", mm_str(p.size_x), true),
                        prop("size_y_mm", "Height (mm)", mm_str(p.size_y), true),
                        choice(
                            "rotation",
                            "Rotation",
                            (u32::from(p.rotation) * 90).to_string(),
                            &ROTATIONS,
                        ),
                    ];
                    if p.shape == PadShape::RoundRect {
                        props.push(prop(
                            "roundrect_ratio",
                            "Corner radius (‰ of the short side)",
                            p.roundrect_ratio.to_string(),
                            true,
                        ));
                    }
                    if p.kind != PadKind::Smd {
                        props.push(prop(
                            "drill_mm",
                            "Drill (mm)",
                            p.drill.map_or(String::new(), |d| mm_str(d.diameter)),
                            true,
                        ));
                        props.push(prop(
                            "slot_mm",
                            "Slot length (mm, empty = round)",
                            p.drill.and_then(|d| d.slot).map_or(String::new(), mm_str),
                            true,
                        ));
                    }
                    props.push(prop(
                        "mask_margin_mm",
                        "Mask margin (mm, empty = default)",
                        p.mask_margin.map_or(String::new(), mm_str),
                        true,
                    ));
                    if p.kind == PadKind::Smd {
                        props.push(prop(
                            "paste_margin_mm",
                            "Paste margin (mm, empty = default)",
                            p.paste_margin.map_or(String::new(), mm_str),
                            true,
                        ));
                        props.push(choice(
                            "paste",
                            "Paste aperture",
                            p.paste.to_string(),
                            &YES_NO,
                        ));
                    }
                    if p.shape == PadShape::Custom {
                        props.push(prop(
                            "outline",
                            "Outline",
                            format!("{} points", p.custom.len()),
                            false,
                        ));
                    }
                    let title = if p.number.is_empty() {
                        format!("{} hole", fe::pad_kind_label(p.kind))
                    } else {
                        format!("Pad {}", p.number)
                    };
                    out.push(ObjectSummary {
                        id: id.0,
                        kind: "pad".into(),
                        title,
                        properties: props,
                    });
                }
                Item::Graphic(i) => {
                    let Some(g) = fp.graphics.get(i) else {
                        continue;
                    };
                    let (kind, mut props) = match &g.shape {
                        FpShape::Line { width, .. } => (
                            "line",
                            vec![prop("width_mm", "Line width (mm)", mm_str(*width), true)],
                        ),
                        FpShape::Rect { width, filled, .. } => (
                            "rectangle",
                            vec![
                                prop("width_mm", "Line width (mm)", mm_str(*width), true),
                                choice("filled", "Filled", filled.to_string(), &YES_NO),
                            ],
                        ),
                        FpShape::Circle { width, filled, .. } => (
                            "circle",
                            vec![
                                prop("width_mm", "Line width (mm)", mm_str(*width), true),
                                choice("filled", "Filled", filled.to_string(), &YES_NO),
                            ],
                        ),
                        FpShape::Arc { width, .. } => (
                            "arc",
                            vec![prop("width_mm", "Line width (mm)", mm_str(*width), true)],
                        ),
                        FpShape::Polygon { width, filled, .. } => (
                            "polygon",
                            vec![
                                prop("width_mm", "Line width (mm)", mm_str(*width), true),
                                choice("filled", "Filled", filled.to_string(), &YES_NO),
                            ],
                        ),
                    };
                    props.insert(0, choice("layer", "Layer", g.layer.id(), &layer_options()));
                    out.push(ObjectSummary {
                        id: id.0,
                        kind: kind.into(),
                        title: format!(
                            "{} ({})",
                            kind[..1].to_uppercase() + &kind[1..],
                            g.layer.label()
                        ),
                        properties: props,
                    });
                }
                Item::Text(i) => {
                    let Some(t) = fp.texts.get(i) else { continue };
                    out.push(ObjectSummary {
                        id: id.0,
                        kind: "text".into(),
                        title: match t.kind {
                            TextKind::Reference => "Reference text".into(),
                            TextKind::Value => "Value text".into(),
                            TextKind::User => "Text".into(),
                        },
                        properties: vec![
                            prop("text", "Text", &t.text, true),
                            choice(
                                "kind",
                                "Shows",
                                match t.kind {
                                    TextKind::Reference => "reference",
                                    TextKind::Value => "value",
                                    TextKind::User => "user",
                                },
                                &[
                                    ("reference", "Reference (R1…)"),
                                    ("value", "Value"),
                                    ("user", "Fixed text"),
                                ],
                            ),
                            choice("layer", "Layer", t.layer.id(), &layer_options()),
                            prop("height_mm", "Height (mm)", mm_str(t.height), true),
                            prop("x_mm", "X (mm)", mm_str(t.position.x), true),
                            prop("y_mm", "Y (mm)", mm_str(t.position.y), true),
                        ],
                    });
                }
            }
        }
        out
    }

    pub(crate) fn fped_describe(&self, id: Id) -> Option<String> {
        let fp = self.fped_footprint()?;
        match item_of(id)? {
            Item::Pad(i) => {
                let p = fp.pads.get(i)?;
                Some(format!(
                    "{} {} {}",
                    fe::pad_kind_label(p.kind),
                    fe::pad_shape_label(p.shape),
                    if p.number.is_empty() {
                        String::new()
                    } else {
                        format!("pad {}", p.number)
                    }
                ))
            }
            Item::Graphic(i) => Some(format!("{} graphic", fp.graphics.get(i)?.layer.label())),
            Item::Text(i) => Some(format!("Text “{}”", fp.texts.get(i)?.text)),
        }
    }

    pub(crate) fn fped_prune_selection(&mut self) {
        let valid: BTreeSet<Id> = {
            let Some(fp) = self.fped_footprint() else {
                self.selection.remove(&Mode::FootprintEditor);
                return;
            };
            (0..fp.pads.len())
                .map(|i| id_of(Item::Pad(i)))
                .chain((0..fp.graphics.len()).map(|i| id_of(Item::Graphic(i))))
                .chain((0..fp.texts.len()).map(|i| id_of(Item::Text(i))))
                .collect()
        };
        if let Some(sel) = self.selection.get_mut(&Mode::FootprintEditor) {
            sel.retain(|id| valid.contains(id));
        }
    }

    pub(crate) fn fped_select_all(&mut self) {
        let Some(fp) = self.fped_footprint() else {
            return;
        };
        let all: BTreeSet<Id> = (0..fp.pads.len())
            .map(|i| id_of(Item::Pad(i)))
            .chain((0..fp.graphics.len()).map(|i| id_of(Item::Graphic(i))))
            .chain((0..fp.texts.len()).map(|i| id_of(Item::Text(i))))
            .collect();
        self.selection.insert(Mode::FootprintEditor, all);
    }

    // -- info for the UI -------------------------------------------------------------------------

    pub(crate) fn fped_layer_infos(&self) -> Vec<LayerInfo> {
        let hex = |c: Rgba| format!("#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]);
        let theme = self.theme_kind();
        let layers = fp_layers(theme, &BTreeSet::new());
        FpLayer::ALL
            .iter()
            .enumerate()
            .map(|(i, l)| LayerInfo {
                id: i as u32,
                name: l.label().to_owned(),
                color: layers
                    .iter()
                    .find(|(id, _)| *id == scene_layer(*l))
                    .map(|(_, ly)| hex(ly.color))
                    .unwrap_or_default(),
                visible: !self.fped.hidden.contains(l),
                active: self.fped.layer == *l,
                toggleable: true,
            })
            .collect()
    }

    pub(crate) fn footprint_editor_info(&self) -> Option<FootprintEditorInfo> {
        let fp = self.fped_footprint()?;
        let t = self.fped.template;
        let mount = match fp.attributes.mount {
            MountType::Unspecified => "unspecified",
            MountType::Smd => "smd",
            MountType::ThroughHole => "through_hole",
        };
        let fields = vec![
            prop("name", "Name (Library:Footprint)", &fp.name, true),
            prop("description", "Description", &fp.description, true),
            prop("tags", "Tags", &fp.tags, true),
            choice(
                "mount",
                "Mounting",
                mount,
                &[
                    ("unspecified", "Unspecified"),
                    ("smd", "SMD"),
                    ("through_hole", "Through-hole"),
                ],
            ),
            choice(
                "exclude_from_bom",
                "Exclude from BOM",
                fp.attributes.exclude_from_bom.to_string(),
                &YES_NO,
            ),
            choice(
                "exclude_from_position_files",
                "Exclude from position files",
                fp.attributes.exclude_from_position_files.to_string(),
                &YES_NO,
            ),
        ];
        let models = fp
            .models
            .iter()
            .enumerate()
            .map(|(i, m)| ModelInfo {
                index: i as u32,
                fields: vec![
                    prop(&format!("model:{i}:path"), "File", &m.path, true),
                    prop(
                        &format!("model:{i}:x"),
                        "Offset X (nm)",
                        m.offset[0].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:y"),
                        "Offset Y (nm)",
                        m.offset[1].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:z"),
                        "Offset Z (nm)",
                        m.offset[2].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:rx"),
                        "Rotation X (m°)",
                        m.rotation_mdeg[0].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:ry"),
                        "Rotation Y (m°)",
                        m.rotation_mdeg[1].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:rz"),
                        "Rotation Z (m°)",
                        m.rotation_mdeg[2].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:sx"),
                        "Scale X (ppm)",
                        m.scale_ppm[0].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:sy"),
                        "Scale Y (ppm)",
                        m.scale_ppm[1].to_string(),
                        true,
                    ),
                    prop(
                        &format!("model:{i}:sz"),
                        "Scale Z (ppm)",
                        m.scale_ppm[2].to_string(),
                        true,
                    ),
                ],
            })
            .collect();
        let generators = generator_infos();
        Some(FootprintEditorInfo {
            name: fp.name.clone(),
            layer: self.fped.layer.id().into(),
            pad_count: fp.pads.len() as u32,
            pad_template: vec![
                choice(
                    "shape",
                    "Shape",
                    fe::pad_shape_id(t.shape),
                    &shape_options(),
                ),
                prop("size_x_mm", "Width (mm)", mm_str(t.size_x), true),
                prop("size_y_mm", "Height (mm)", mm_str(t.size_y), true),
                prop("drill_mm", "Drill (mm)", mm_str(t.drill), true),
            ],
            fields,
            models,
            warnings: buturecad_pcb::warnings(fp),
            generators,
        })
    }

    // -- commands --------------------------------------------------------------------------------

    pub(crate) fn fped_editor_command(&mut self, name: &str) -> Result<(), String> {
        if let Some(rest) = name.strip_prefix("tool:") {
            let (id, arg) = match rest.split_once(':') {
                Some((id, arg)) => (id, Some(arg)),
                None => (rest, None),
            };
            return self.fped_set_tool(id, arg);
        }
        match name {
            "select_all" => self.fped_select_all(),
            "clear_selection" => {
                self.selection
                    .insert(Mode::FootprintEditor, BTreeSet::new());
            }
            "cancel" => self.fped_cancel(),
            "delete" => self.fped_delete()?,
            "rotate" => self.fped_transform(true, true)?,
            "mirror_x" => self.fped_transform(false, true)?,
            "mirror_y" => self.fped_transform(false, false)?,
            other => {
                return Err(format!(
                    "“{other}” is not available in the Footprint Editor"
                ))
            }
        }
        Ok(())
    }

    fn fped_set_tool(&mut self, id: &str, arg: Option<&str>) -> Result<(), String> {
        let tool = match id {
            "select" => FpTool::Select,
            "pad_smd" => FpTool::PadSmd,
            "pad_through" => FpTool::PadThrough,
            "npth" => FpTool::Npth,
            "line" => FpTool::Line,
            "rect" => FpTool::Rect,
            "circle" => FpTool::Circle,
            "arc" => FpTool::Arc,
            "text" => match arg.map(str::trim) {
                Some(t) if !t.is_empty() => FpTool::Text(t.to_owned()),
                _ => return Err("the text tool needs the text to place".into()),
            },
            other => return Err(format!("unknown Footprint Editor tool: {other}")),
        };
        if tool != FpTool::Select {
            self.fped_require()?;
        }
        self.fped.pts.clear();
        self.fped.drag = None;
        self.fped.tool = tool;
        Ok(())
    }

    fn fped_cancel(&mut self) {
        if !self.fped.pts.is_empty() || self.fped.drag.is_some() {
            self.fped.pts.clear();
            self.fped.drag = None;
            self.fped.preview = None;
            self.fped.version += 1;
            self.gesture = Gesture::None;
        } else if self.fped.tool != FpTool::Select {
            self.fped.tool = FpTool::Select;
        } else {
            self.selection
                .insert(Mode::FootprintEditor, BTreeSet::new());
        }
    }

    /// `footprint_editor_command` IPC: lifecycle, view and generator commands.
    ///
    /// Names: `open` (arg: id), `new` (arg: `Library:Name`), `close`, `layer` (arg: layer id),
    /// `layer_visible` (arg: `layer:0|1`), `pad_template` (arg: `key=value`),
    /// `generate` (arg: `family|key=value|…`), `auto_courtyard`, `add_model` (arg: path),
    /// `remove_model` (arg: index), `save_to_library` (arg: `scope:Library`), `delete_footprint`.
    pub fn footprint_editor_command(
        &mut self,
        name: &str,
        arg: Option<&str>,
    ) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no project is open".into());
        }
        let need = |what: &str| -> Result<&str, String> {
            arg.map(str::trim)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| format!("{name} needs {what}"))
        };
        match name {
            "open" => self.fped_open(need("a footprint")?),
            "new" => self.fped_new(need("a name like Library:Footprint")?),
            "close" => {
                self.fped.name = None;
                self.fped.preview = None;
                self.fped.pts.clear();
                self.fped.drag = None;
                self.selection.remove(&Mode::FootprintEditor);
                self.fped.version += 1;
                Ok(())
            }
            "layer" => {
                self.fped.layer = FpLayer::parse(need("a layer")?).ok_or("unknown layer")?;
                Ok(())
            }
            "layer_visible" => {
                let (l, v) = need("layer:0|1")?
                    .split_once(':')
                    .ok_or("expected layer:0|1")?;
                let layer = FpLayer::parse(l).ok_or("unknown layer")?;
                if v == "1" {
                    self.fped.hidden.remove(&layer);
                } else {
                    self.fped.hidden.insert(layer);
                }
                self.fped.version += 1;
                Ok(())
            }
            "pad_template" => {
                let (k, v) = need("key=value")?
                    .split_once('=')
                    .ok_or("pad_template needs key=value")?;
                let t = &mut self.fped.template;
                match k {
                    "shape" => t.shape = fe::parse_pad_shape(v).ok_or("unknown pad shape")?,
                    "size_x_mm" | "size_y_mm" | "drill_mm" => {
                        let nm = parse_mm(v, k)?;
                        if nm <= 0 {
                            return Err("sizes must be positive".into());
                        }
                        match k {
                            "size_x_mm" => t.size_x = nm,
                            "size_y_mm" => t.size_y = nm,
                            _ => t.drill = nm,
                        }
                    }
                    other => return Err(format!("unknown pad template field {other}")),
                }
                Ok(())
            }
            "generate" => self.fped_generate(need("family|key=value|…")?),
            "auto_courtyard" => {
                self.fped_require()?;
                self.fped_edit("Courtyard from pads", |f| {
                    fe::set_courtyard_from_pads(f, 250_000)
                })
            }
            "add_model" => {
                self.fped_require()?;
                let path = need("a model path")?.to_owned();
                self.fped_edit("Add 3D model reference", |f| fe::add_model(f, &path))
                    .map(|_| ())
            }
            "remove_model" => {
                self.fped_require()?;
                let i: usize = need("a model number")?
                    .parse()
                    .map_err(|_| "model number must be a number")?;
                self.fped_edit("Remove 3D model reference", |f| fe::remove_model(f, i))
            }
            "save_to_library" => self.fped_save_to_library(need("scope:Library")?),
            "delete_footprint" => self.fped_delete_footprint(),
            other => Err(format!("unknown Footprint Editor command: {other}")),
        }
    }

    /// Sets a footprint-level field (`footprint_editor_set` IPC).
    pub fn footprint_editor_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.fped_require()?;
        if key == "name" {
            return self.fped_rename(value.trim());
        }
        let label = format!("Edit footprint {key}");
        self.fped_edit(&label, |f| fe::set_footprint_field(f, key, value))
    }

    fn fped_rename(&mut self, new_name: &str) -> Result<(), String> {
        buturecad_pcb::validate_name(new_name).map_err(|e| e.to_string())?;
        if split_lib_id(new_name).is_none() {
            return Err("footprint names look like Library:Name".into());
        }
        let old = self.fped.name.clone().ok_or("no footprint is open")?;
        if old == new_name {
            return Ok(());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        let fps = &open.engine.project().pcb().footprints;
        if fps.contains_key(new_name) {
            return Err(format!(
                "a footprint named {new_name} already exists in this project"
            ));
        }
        let before = fps.get(&old).cloned().ok_or("footprint is missing")?;
        let mut renamed = before.clone();
        renamed.name = new_name.to_owned();
        let new_key = new_name.to_owned();
        open.engine
            .transaction(format!("Rename footprint to {new_name}"), |e| {
                e.execute(SetFootprintCommand::new(&new_key, None, Some(renamed)))?;
                e.execute(SetFootprintCommand::new(&old, Some(before), None))
            })
            .map_err(|e| e.to_string())?;
        self.fped.name = Some(new_key);
        self.fped.version += 1;
        Ok(())
    }

    fn fped_open(&mut self, id: &str) -> Result<(), String> {
        let in_project = self
            .open
            .as_ref()
            .is_some_and(|o| o.engine.project().pcb().footprints.contains_key(id));
        if !in_project {
            let fp = self
                .find_library_footprint(id)
                .ok_or_else(|| format!("footprint {id} was not found in any library"))?;
            let open = self.open.as_mut().ok_or("no project is open")?;
            open.engine
                .execute(
                    SetFootprintCommand::new(id, None, Some(fp))
                        .with_label(format!("Open footprint {id}")),
                )
                .map_err(|e| e.to_string())?;
        }
        self.fped.name = Some(id.to_owned());
        self.fped.tool = FpTool::Select;
        self.fped.pts.clear();
        self.fped.drag = None;
        self.fped.preview = None;
        self.fped.version += 1;
        self.selection.remove(&Mode::FootprintEditor);
        self.fitted.remove(&Mode::FootprintEditor);
        Ok(())
    }

    fn fped_new(&mut self, id: &str) -> Result<(), String> {
        buturecad_pcb::validate_name(id).map_err(|e| e.to_string())?;
        if split_lib_id(id).is_none() {
            return Err("footprint names look like Library:Name".into());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        if open.engine.project().pcb().footprints.contains_key(id) {
            return Err(format!("{id} already exists in this project"));
        }
        open.engine
            .execute(SetFootprintCommand::new(id, None, Some(Footprint::new(id))))
            .map_err(|e| e.to_string())?;
        self.fped_open(id)
    }

    fn fped_generate(&mut self, spec: &str) -> Result<(), String> {
        let mut parts = spec.split('|');
        let family = parts.next().unwrap_or_default().trim();
        let params: Vec<(String, String)> = parts
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                p.split_once('=')
                    .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                    .ok_or_else(|| format!("parameter “{p}” needs key=value"))
            })
            .collect::<Result<_, _>>()?;
        let mut fp = generators::generate(family, &params).map_err(|e| e.to_string())?;
        // Generated names are bare; give them a library so the project can tell them apart.
        let short = fp.name.clone();
        let id = qualify("Generated", &short);
        fp.name = id.clone();
        let open = self.open.as_mut().ok_or("no project is open")?;
        let exists = open.engine.project().pcb().footprints.contains_key(&id);
        if !exists {
            open.engine
                .execute(
                    SetFootprintCommand::new(&id, None, Some(fp))
                        .with_label(format!("Generate {short}")),
                )
                .map_err(|e| e.to_string())?;
        }
        self.command_log.push(format!("Generate {short}"));
        self.fped_open(&id)?;
        self.log(
            MessageLevel::Info,
            if exists {
                format!("{short} already exists in this project; opened it")
            } else {
                format!("Generated {short}. Check the dimensions against the part's datasheet.")
            },
        );
        Ok(())
    }

    fn fped_delete_footprint(&mut self) -> Result<(), String> {
        let name = self.fped.name.clone().ok_or("no footprint is open")?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before = open
            .engine
            .project()
            .pcb()
            .footprints
            .get(&name)
            .cloned()
            .ok_or("footprint is missing")?;
        open.engine
            .execute(SetFootprintCommand::new(&name, Some(before), None))
            .map_err(|e| e.to_string())?;
        self.fped.name = None;
        self.fped.version += 1;
        self.selection.remove(&Mode::FootprintEditor);
        Ok(())
    }

    // -- libraries -------------------------------------------------------------------------------

    pub(crate) fn find_library_footprint(&self, id: &str) -> Option<Footprint> {
        let (lib, name) = split_lib_id(id)?;
        if lib == buturecad_libraries::standard::NAME {
            return buturecad_libraries::standard::footprint_library()
                .ok()
                .and_then(|l| l.instantiate(name));
        }
        self.library_dirs().into_iter().find_map(|(_, dir)| {
            dir.load_footprint_library(lib)
                .ok()
                .and_then(|l| l.instantiate(name))
        })
    }

    pub fn footprint_libraries(&self) -> Vec<LibraryInfo> {
        let mut out = vec![LibraryInfo {
            name: buturecad_libraries::standard::NAME.into(),
            scope: "builtin".into(),
            item_count: buturecad_libraries::standard::footprints().len() as u32,
            error: None,
        }];
        for (scope, dir) in self.library_dirs() {
            for l in dir.list_footprint_libraries() {
                out.push(LibraryInfo {
                    name: l.name,
                    scope: scope.into(),
                    item_count: l.item_count as u32,
                    error: l.error,
                });
            }
        }
        out
    }

    /// Every footprint definition the app can offer, by `Library:Name`, with where it comes from:
    /// the built-in `Std` library, the library folders, then the project (a project copy wins
    /// over a library entry with the same id).
    pub(crate) fn all_footprint_defs(&self) -> BTreeMap<String, (Footprint, String)> {
        let mut all: BTreeMap<String, (Footprint, String)> = BTreeMap::new();
        for f in buturecad_libraries::standard::footprints() {
            all.insert(f.name.clone(), (f, "builtin".into()));
        }
        for (scope, dir) in self.library_dirs() {
            for l in dir.list_footprint_libraries() {
                if let Ok(lib) = LibraryDir::load_footprint_library(&dir, &l.name) {
                    for n in lib.names() {
                        if let Some(f) = lib.instantiate(n) {
                            all.insert(f.name.clone(), (f, format!("{scope}-library")));
                        }
                    }
                }
            }
        }
        if let Some(open) = &self.open {
            for (k, v) in &open.engine.project().pcb().footprints {
                all.insert(k.clone(), (v.clone(), "project".into()));
            }
        }
        all
    }

    /// Footprints in the project and in the library folders.
    pub fn library_footprints(&self) -> Vec<FootprintInfo> {
        self.all_footprint_defs()
            .into_values()
            .map(|(f, source)| FootprintInfo {
                lib_id: f.name.clone(),
                name: f.name.rsplit(':').next().unwrap_or(&f.name).to_owned(),
                library: f.name.split_once(':').map_or("", |(l, _)| l).to_owned(),
                source,
                description: f.description.clone(),
                pad_count: f.pads.len() as u32,
                tags: f.tags.clone(),
            })
            .collect()
    }

    /// Footprints that fit the schematic part `id` (its symbol's filters, and a pad for every
    /// pin), best first: what the user would pick in KiCad's footprint chooser.
    pub fn footprint_suggestions(&self, id: u64) -> Result<Vec<FootprintSuggestion>, String> {
        let sch = self.sch().ok_or("no project is open")?;
        let comp = match sch.object(Id(id)) {
            Some((_, Object::Component(c))) => c.clone(),
            _ => return Err("select a schematic part first".into()),
        };
        let sym = self
            .symbol_by_id(&comp.lib_id)
            .ok_or_else(|| format!("symbol {} was not found", comp.lib_id))?;
        let defs = self.all_footprint_defs();
        let fps: Vec<Footprint> = defs.values().map(|(f, _)| f.clone()).collect();
        Ok(buturecad_libraries::suggest::suggest(&sym, &fps, 40)
            .into_iter()
            .filter_map(|s| {
                let (f, _) = defs.get(&s.id)?;
                Some(FootprintSuggestion {
                    name: f.name.rsplit(':').next().unwrap_or(&f.name).to_owned(),
                    library: f.name.split_once(':').map_or("", |(l, _)| l).to_owned(),
                    description: f.description.clone(),
                    pad_count: f.pads.len() as u32,
                    reason: match s.reason {
                        buturecad_libraries::suggest::Reason::Filter => "filter",
                        buturecad_libraries::suggest::Reason::ExactPads => "pads",
                    }
                    .into(),
                    reason_text: s.reason.label().into(),
                    lib_id: s.id,
                })
            })
            .collect())
    }

    fn fped_save_to_library(&mut self, arg: &str) -> Result<(), String> {
        let (scope, library) = arg.split_once(':').ok_or("expected scope:Library")?;
        let dir = self
            .library_dirs()
            .into_iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, d)| d)
            .ok_or_else(|| format!("the {scope} library folder is not available"))?;
        let fp = self
            .fped_footprint()
            .cloned()
            .ok_or("no footprint is open")?;
        let short = fp.name.rsplit(':').next().unwrap_or(&fp.name).to_owned();
        let wanted = qualify(library, &short);
        let id = dir
            .save_footprint(library, fp.clone())
            .map_err(|e| e.to_string())?;
        if wanted != fp.name {
            self.fped_rename(&wanted)?;
        }
        self.log(
            MessageLevel::Info,
            format!(
                "Saved {id} to the {scope} library folder ({})",
                dir.root().display()
            ),
        );
        Ok(())
    }

    // -- pointer ---------------------------------------------------------------------------------

    pub(crate) fn pointer_footprint(&mut self, ev: PointerEvent, out: &mut Outcome) {
        if self.fped_footprint().is_none() {
            return;
        }
        let at = self.cursor;
        match ev.kind {
            PointerKind::Move => {
                match self.fped.drag.clone() {
                    Some(Drag::Pending {
                        start_px,
                        start,
                        items,
                    }) => {
                        if (ev.x - start_px.0).hypot(ev.y - start_px.1) >= DRAG_THRESHOLD_PX {
                            self.fped.drag = Some(Drag::Moving { start, items });
                            self.fped_update_move(at);
                        }
                    }
                    Some(Drag::Moving { .. }) => self.fped_update_move(at),
                    None => {
                        if !self.update_box(ev.x, ev.y) {
                            self.hover = self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0);
                        }
                    }
                }
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 0 => self.fped_down(ev, at, out),
            PointerKind::Down if ev.button == 2 => {
                if !self.fped.pts.is_empty() {
                    self.fped.pts.clear();
                } else if self.fped.tool != FpTool::Select {
                    self.fped.tool = FpTool::Select;
                } else if let Some(owner) = self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0) {
                    self.select_owner(owner, false);
                    out.selection_changed = true;
                }
                out.redraw = true;
            }
            PointerKind::Up => {
                match self.fped.drag.take() {
                    Some(Drag::Moving { start, items }) => {
                        self.fped.preview = None;
                        self.fped.version += 1;
                        let delta = at - start;
                        if !delta.is_zero() {
                            match self.fped_edit("Move items", |f| fe::move_items(f, &items, delta))
                            {
                                Ok(()) => out.model_changed = true,
                                Err(e) => self.fped_log_err("Move items", e),
                            }
                        }
                    }
                    Some(Drag::Pending { .. }) | None => {
                        if self.finish_box() {
                            self.selection
                                .entry(Mode::FootprintEditor)
                                .or_default()
                                .remove(&Id(0));
                            out.selection_changed = true;
                        }
                    }
                }
                out.redraw = true;
            }
            _ => {}
        }
    }

    fn fped_down(&mut self, ev: PointerEvent, at: Point, out: &mut Outcome) {
        match self.fped.tool.clone() {
            FpTool::Select => {
                let additive = ev.shift || ev.ctrl;
                match self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0) {
                    Some(owner) => {
                        self.select_owner(owner, additive);
                        out.selection_changed = true;
                        let items = self.fped_items();
                        if !additive && items.iter().any(|i| id_of(*i) == owner) {
                            self.fped.drag = Some(Drag::Pending {
                                start_px: (ev.x, ev.y),
                                start: at,
                                items,
                            });
                        }
                    }
                    None => self.begin_box(ev.x, ev.y, additive),
                }
            }
            FpTool::PadSmd => {
                self.fped_add_pad(at, PadKind::Smd);
                out.model_changed = true;
                out.selection_changed = true;
            }
            FpTool::PadThrough => {
                self.fped_add_pad(at, PadKind::Through);
                out.model_changed = true;
                out.selection_changed = true;
            }
            FpTool::Npth => {
                self.fped_add_pad(at, PadKind::NonPlated);
                out.model_changed = true;
                out.selection_changed = true;
            }
            FpTool::Text(text) => {
                let (unit_h, layer) = (1_000_000, self.fped.layer);
                let t = FpText {
                    kind: TextKind::User,
                    layer,
                    text,
                    position: at,
                    height: unit_h,
                    rotation: 0,
                };
                match self.fped_edit("Add text", |f| fe::add_text(f, t)) {
                    Ok(i) => self.fped_select(&[Item::Text(i)]),
                    Err(e) => self.fped_log_err("Add text", e),
                }
                out.model_changed = true;
                out.selection_changed = true;
            }
            FpTool::Line | FpTool::Rect | FpTool::Circle | FpTool::Arc => {
                self.fped_draw_click(at, out);
            }
        }
        out.redraw = true;
    }

    fn fped_draw_click(&mut self, at: Point, out: &mut Outcome) {
        if self.fped.pts.last() == Some(&at) {
            return;
        }
        self.fped.pts.push(at);
        let pts = self.fped.pts.clone();
        let width = if self.fped.layer == FpLayer::Courtyard {
            50_000
        } else {
            DEFAULT_LINE
        };
        let done = match (&self.fped.tool, pts.as_slice()) {
            (FpTool::Line, [a, b]) => Some((
                "Add line",
                FpShape::Line {
                    segment: Segment::new(*a, *b),
                    width,
                },
            )),
            (FpTool::Rect, [a, b]) => match Rectangle::new(*a, *b) {
                Ok(rect) => Some((
                    "Add rectangle",
                    FpShape::Rect {
                        rect,
                        filled: false,
                        width,
                    },
                )),
                Err(_) => {
                    self.log(
                        MessageLevel::Warn,
                        "A rectangle needs two different corners",
                    );
                    self.fped.pts.pop();
                    return;
                }
            },
            (FpTool::Circle, [c, r]) => {
                let radius = c.distance(*r).round() as i64;
                if radius <= 0 {
                    self.fped.pts.pop();
                    return;
                }
                Some((
                    "Add circle",
                    FpShape::Circle {
                        circle: Circle { center: *c, radius },
                        filled: false,
                        width,
                    },
                ))
            }
            (FpTool::Arc, [c, s, e]) => match arc_through(*c, *s, *e) {
                Some(arc) => Some(("Add arc", FpShape::Arc { arc, width })),
                None => {
                    self.log(
                        MessageLevel::Warn,
                        "That arc is degenerate; pick a different end point",
                    );
                    self.fped.pts.pop();
                    return;
                }
            },
            _ => None,
        };
        if let Some((label, shape)) = done {
            let chain = matches!(self.fped.tool, FpTool::Line);
            self.fped.pts.clear();
            if chain {
                self.fped.pts.push(at);
            }
            self.fped_add_graphic(label, shape);
            out.model_changed = true;
            out.selection_changed = true;
        }
    }

    fn fped_update_move(&mut self, at: Point) {
        let Some(Drag::Moving { start, items }) = self.fped.drag.clone() else {
            return;
        };
        let Some(base) = self.fped_footprint().cloned() else {
            return;
        };
        let mut moved = base;
        if fe::move_items(&mut moved, &items, at - start).is_ok() {
            self.fped.preview = Some(moved);
            self.fped.version += 1;
        }
    }

    pub(crate) fn fped_preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        if self.fped_footprint().is_none() {
            return Vec::new();
        }
        let colour = match self.theme {
            ThemeName::Dark => Theme::dark().selection,
            ThemeName::Light => Theme::light().selection,
        };
        let at = self.cursor;
        let pts = &self.fped.pts;
        let line = |a: Point, b: Point| Shape::Line {
            segment: Segment::new(a, b),
            width: DEFAULT_LINE,
        };
        let mut out: Vec<(Shape, Rgba)> = super::symbol_editor::origin_marks(self.theme, 2_000_000);
        match (&self.fped.tool, pts.as_slice()) {
            (FpTool::Line, [a]) if *a != at => out.push((line(*a, at), colour)),
            (FpTool::Rect, [a]) => {
                if let Ok(rect) = Rectangle::new(*a, at) {
                    out.push((
                        Shape::Rect {
                            rect,
                            filled: false,
                            stroke: DEFAULT_LINE,
                        },
                        colour,
                    ));
                }
            }
            (FpTool::Circle, [c]) => {
                let radius = c.distance(at).round() as i64;
                if radius > 0 {
                    out.push((
                        Shape::Circle {
                            circle: Circle { center: *c, radius },
                            filled: false,
                            stroke: DEFAULT_LINE,
                        },
                        colour,
                    ));
                }
            }
            (FpTool::Arc, [c]) => out.push((line(*c, at), colour)),
            (FpTool::Arc, [c, s]) => {
                out.push((line(*c, *s), colour));
                if let Some(arc) = arc_through(*c, *s, at) {
                    out.push((
                        Shape::Arc {
                            arc,
                            width: DEFAULT_LINE,
                        },
                        colour,
                    ));
                }
            }
            (FpTool::PadSmd | FpTool::PadThrough | FpTool::Npth, _) => {
                let t = self.fped.template;
                let kind = match self.fped.tool {
                    FpTool::PadSmd => PadKind::Smd,
                    FpTool::PadThrough => PadKind::Through,
                    _ => PadKind::NonPlated,
                };
                let mut pad = match kind {
                    PadKind::Smd => Pad::smd("", at, t.size_x, t.size_y),
                    PadKind::Through => {
                        let d = t.drill.min(t.size_x.min(t.size_y) - 100_000).max(100_000);
                        Pad::through("", PadShape::Circle, at, t.size_x, t.size_y, d)
                    }
                    PadKind::NonPlated => Pad::npth(at, t.drill),
                };
                pad.shape = if kind == PadKind::NonPlated || t.shape == PadShape::Custom {
                    PadShape::Circle
                } else {
                    t.shape
                };
                if pad.shape == PadShape::Circle {
                    let d = pad.size_x.max(pad.size_y);
                    pad.size_x = d;
                    pad.size_y = d;
                }
                pad.roundrect_ratio = t.ratio;
                let outline = if kind == PadKind::NonPlated {
                    Some(Shape::Circle {
                        circle: Circle {
                            center: at,
                            radius: t.drill / 2,
                        },
                        filled: false,
                        stroke: 50_000,
                    })
                } else {
                    pad_shape(&pad, 0, false)
                };
                out.extend(outline.map(|s| (s, colour)));
            }
            (FpTool::Text(text), _) => out.push((
                Shape::Text {
                    origin: at,
                    height: 1_000_000,
                    text: text.clone(),
                },
                colour,
            )),
            _ => {}
        }
        out
    }

    pub(crate) fn fped_hint(&self) -> String {
        if self.fped_footprint().is_none() {
            return "Open, create or generate a footprint to start editing.".into();
        }
        match (&self.fped.tool, self.fped.pts.len()) {
            (FpTool::Select, _) => {
                "Select: click, drag a box, drag items to move. R rotates, X/Y mirror, Del deletes."
                    .into()
            }
            (FpTool::PadSmd, _) => {
                "SMD pad: click to place (number is automatic). Esc stops.".into()
            }
            (FpTool::PadThrough, _) => "Through-hole pad: click to place. Esc stops.".into(),
            (FpTool::Npth, _) => "Non-plated hole: click to place. Esc stops.".into(),
            (FpTool::Line, 0) => "Line: click the start point.".into(),
            (FpTool::Line, _) => {
                "Line: click the next point (chains). Right click or Esc ends.".into()
            }
            (FpTool::Rect, 0) => "Rectangle: click the first corner.".into(),
            (FpTool::Rect, _) => "Rectangle: click the opposite corner.".into(),
            (FpTool::Circle, 0) => "Circle: click the center.".into(),
            (FpTool::Circle, _) => "Circle: click a point on the circle.".into(),
            (FpTool::Arc, 0) => "Arc: click the center.".into(),
            (FpTool::Arc, 1) => "Arc: click the start point.".into(),
            (FpTool::Arc, _) => "Arc: click the end direction (counter-clockwise).".into(),
            (FpTool::Text(_), _) => "Text: click to place.".into(),
        }
    }
}

/// Counter-clockwise arc about `center` from `start` towards `towards` (end projected onto the
/// circle through `start`).
fn arc_through(center: Point, start: Point, towards: Point) -> Option<Arc> {
    let r = center.distance(start);
    let d = towards - center;
    let len = d.length();
    if r < 1.0 || len < 1.0 {
        return None;
    }
    let (ex, ey) = (
        center.x as f64 + d.x as f64 / len * r,
        center.y as f64 + d.y as f64 / len * r,
    );
    let mut best: Option<(f64, Point)> = None;
    for dx in -1..=1 {
        for dy in -1..=1 {
            let p = Point::new(ex.round() as i64 + dx, ey.round() as i64 + dy);
            let err = (center.distance(p) - r).abs();
            if best.is_none_or(|(b, _)| err < b) {
                best = Some((err, p));
            }
        }
    }
    let (_, end) = best?;
    Arc::new(center, start, end, true).ok()
}
