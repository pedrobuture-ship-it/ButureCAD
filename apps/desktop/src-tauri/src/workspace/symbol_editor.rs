//! Symbol Editor: the interaction layer over `buturecad_schematic::symbol_edit`.
//!
//! The symbol being edited lives in the project's embedded symbol table, so every edit is one
//! `SetSymbolCommand` through the Command Engine (undo/redo, dirty state, autosave, crash
//! recovery all work like for any other edit). Libraries are the *source and sink*: a symbol is
//! copied from a library into the project when opened, and "Save to library" writes it back.
//! Live previews (dragging items, a primitive being drawn) never touch the model.

use super::*;
use crate::scene_builder::{self as sb, ThemeKind};
use buturecad_eda_core::commands::{SchematicEditCommand, SetSymbolCommand};
use buturecad_geometry::{Arc, Circle, Rectangle, Segment, Vector};
use buturecad_libraries::{qualify, split_lib_id, LibraryDir};
use buturecad_renderer::{Item as SceneItem, LayerId, Rgba, Shape};
use buturecad_schematic::symbol_edit::{self as se, Item};
use buturecad_schematic::{
    builtin, Change, Direction, Graphic, Object, Pin, PinType, Placement, Symbol, SymbolGraphic,
    REPR_ALTERNATE, REPR_NORMAL,
};

/// Item ids: graphics are `1 + index`, pins are `PIN_BASE + index`.
pub(crate) const PIN_BASE: u64 = 1 << 24;
const PIN_TEXT_H: i64 = 1_000_000;
const DEFAULT_PIN_LENGTH: i64 = 2_540_000;

pub(crate) fn id_of(item: Item) -> Id {
    match item {
        Item::Graphic(i) => Id(1 + i as u64),
        Item::Pin(i) => Id(PIN_BASE + i as u64),
    }
}

pub(crate) fn item_of(id: Id) -> Option<Item> {
    match id.0 {
        0 => None,
        n if n >= PIN_BASE => Some(Item::Pin((n - PIN_BASE) as usize)),
        n => Some(Item::Graphic((n - 1) as usize)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum SymTool {
    #[default]
    Select,
    Pin,
    Line,
    Rect,
    Circle,
    Arc,
    Text(String),
}

impl SymTool {
    pub fn id(&self) -> &'static str {
        match self {
            SymTool::Select => "select",
            SymTool::Pin => "pin",
            SymTool::Line => "line",
            SymTool::Rect => "rect",
            SymTool::Circle => "circle",
            SymTool::Arc => "arc",
            SymTool::Text(_) => "text",
        }
    }
    pub fn arg(&self) -> Option<String> {
        match self {
            SymTool::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
}

/// Attributes of the next pin placed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PinTemplate {
    pub kind: PinType,
    pub orientation: Direction,
    pub length: i64,
}

impl Default for PinTemplate {
    fn default() -> Self {
        Self {
            kind: PinType::Passive,
            orientation: Direction::Right,
            length: DEFAULT_PIN_LENGTH,
        }
    }
}

#[derive(Debug, Clone)]
enum Drag {
    /// Button down on an item; becomes `Moving` after the pointer travels far enough.
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

pub(crate) struct SymbolEditor {
    /// Key of the edited symbol in the project's symbol table.
    pub lib_id: Option<String>,
    pub unit: u8,
    pub repr: u8,
    pub tool: SymTool,
    pub template: PinTemplate,
    /// Points of the primitive being drawn.
    pts: Vec<Point>,
    drag: Option<Drag>,
    /// A modified copy shown while an item is being dragged.
    preview: Option<Symbol>,
    pub scene: Scene,
    owners: BTreeMap<Id, Vec<ObjectId>>,
    built: Option<(u64, Option<String>, u8, u8, ThemeName, u64)>,
    /// Bumped by anything that changes what the scene shows without a model revision.
    version: u64,
}

impl Default for SymbolEditor {
    fn default() -> Self {
        Self {
            lib_id: None,
            unit: 1,
            repr: REPR_NORMAL,
            tool: SymTool::Select,
            template: PinTemplate::default(),
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

impl SymbolEditor {
    pub fn shapes_of(&self, owner: Id) -> &[ObjectId] {
        self.owners.get(&owner).map_or(&[], Vec::as_slice)
    }
}

// ---------------------------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------------------------

/// The origin cross. Drawn as an overlay (not as scene items) so it can never be picked or get
/// in the way of the item sitting on the origin.
pub(crate) fn origin_marks(theme: ThemeName, arm: i64) -> Vec<(Shape, Rgba)> {
    let grey = match theme {
        ThemeName::Dark => Rgba::rgb(110, 110, 120),
        ThemeName::Light => Rgba::rgb(150, 150, 160),
    };
    let line = |a: Point, b: Point| {
        (
            Shape::Line {
                segment: Segment::new(a, b),
                width: 30_000,
            },
            grey,
        )
    };
    vec![
        line(Point::new(-arm, 0), Point::new(arm, 0)),
        line(Point::new(0, -arm), Point::new(0, arm)),
    ]
}

fn text_width(text: &str, h: i64) -> i64 {
    text.chars().count() as i64 * 6 * (h / 7)
}

fn bbox_rect(a: Point, b: Point, pad: i64) -> Option<Shape> {
    let (lo, hi) = (
        Point::new(a.x.min(b.x) - pad, a.y.min(b.y) - pad),
        Point::new(a.x.max(b.x) + pad, a.y.max(b.y) + pad),
    );
    Rectangle::new(lo, hi).ok().map(|rect| Shape::Rect {
        rect,
        filled: true,
        stroke: 0,
    })
}

fn pin_shapes(pin: &Pin) -> Vec<(LayerId, Shape)> {
    let ident = Placement::at(Point::ORIGIN);
    let mut out = vec![sb::pin_shape(pin, &ident)];
    let dir = pin.orientation.vector();
    let end = pin.position + Vector::new(dir.x * pin.length, dir.y * pin.length);
    out.push((
        sb::L_PIN,
        Shape::Circle {
            circle: Circle {
                center: pin.position,
                radius: 220_000,
            },
            filled: false,
            stroke: 100_000,
        },
    ));
    let h = PIN_TEXT_H;
    if !pin.number.is_empty() && pin.length > 0 {
        let mid = Point::new((pin.position.x + end.x) / 2, (pin.position.y + end.y) / 2);
        let w = text_width(&pin.number, h);
        let origin = match pin.orientation {
            Direction::Right | Direction::Left => Point::new(mid.x - w / 2, mid.y + 200_000),
            Direction::Up | Direction::Down => Point::new(mid.x + 250_000, mid.y - h / 2),
        };
        out.push((
            sb::L_TEXT,
            Shape::Text {
                origin,
                height: h,
                text: pin.number.clone(),
            },
        ));
    }
    if !pin.name.is_empty() && pin.name != "~" {
        let w = text_width(&pin.name, h);
        let origin = match pin.orientation {
            Direction::Right => Point::new(end.x + 400_000, end.y - h / 2),
            Direction::Left => Point::new(end.x - 400_000 - w, end.y - h / 2),
            Direction::Up => Point::new(end.x - w / 2, end.y + 400_000),
            Direction::Down => Point::new(end.x - w / 2, end.y - 400_000 - h),
        };
        out.push((
            sb::L_TEXT,
            Shape::Text {
                origin,
                height: h,
                text: pin.name.clone(),
            },
        ));
    }
    if let Some(hit) = bbox_rect(pin.position, end, 450_000) {
        out.push((sb::L_HIT, hit));
    }
    out
}

fn graphic_item_shapes(g: &Graphic) -> Vec<(LayerId, Shape)> {
    let mut out = sb::graphic_shapes(g, &Placement::at(Point::ORIGIN));
    if let Graphic::Text {
        origin,
        height,
        text,
    } = g
    {
        let end = *origin + Vector::new(text_width(text, *height), *height);
        if let Some(hit) = bbox_rect(*origin, end, 100_000) {
            out.push((sb::L_HIT, hit));
        }
    }
    out
}

fn build_scene(
    sym: &Symbol,
    unit: u8,
    repr: u8,
    theme: ThemeKind,
) -> (Scene, BTreeMap<Id, Vec<ObjectId>>) {
    let mut scene = Scene::new();
    for (id, layer) in sb::schematic_layers(theme) {
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
    for (i, g) in sym.graphics.iter().enumerate() {
        if g.is_visible_in(unit, repr) {
            add(
                &mut scene,
                id_of(Item::Graphic(i)),
                graphic_item_shapes(&g.graphic),
            );
        }
    }
    for (i, p) in sym.pins.iter().enumerate() {
        if p.is_visible_in(unit, repr) {
            add(&mut scene, id_of(Item::Pin(i)), pin_shapes(p));
        }
    }
    (scene, owners)
}

fn mm_str(v: i64) -> String {
    let s = format!("{:.4}", v as f64 / 1e6);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
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

fn pin_type_options() -> Vec<(&'static str, &'static str)> {
    se::PIN_TYPES
        .iter()
        .map(|t| (se::pin_type_id(*t), se::pin_type_label(*t)))
        .collect()
}

const DIRECTIONS: [(&str, &str); 4] = [
    ("right", "Right"),
    ("up", "Up"),
    ("left", "Left"),
    ("down", "Down"),
];

impl Workspace {
    // -- access ----------------------------------------------------------------------------------

    /// The symbol being edited, as stored in the project.
    pub(crate) fn symed_symbol(&self) -> Option<&Symbol> {
        let name = self.symed.lib_id.as_ref()?;
        self.open
            .as_ref()?
            .engine
            .project()
            .schematic()
            .symbol(name)
    }

    /// What the canvas shows: the live preview while dragging, else the stored symbol.
    fn symed_shown(&self) -> Option<&Symbol> {
        self.symed.preview.as_ref().or_else(|| self.symed_symbol())
    }

    fn symed_require(&self) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no project is open".into());
        }
        if self.symed.lib_id.is_none() {
            return Err("no symbol is open in the Symbol Editor".into());
        }
        Ok(())
    }

    pub(crate) fn symed_ensure_scene(&mut self) {
        let Some(open) = &self.open else { return };
        let key = (
            open.engine.revision(),
            self.symed.lib_id.clone(),
            self.symed.unit,
            self.symed.repr,
            self.theme,
            self.symed.version,
        );
        if self.symed.built.as_ref() == Some(&key) {
            return;
        }
        let theme = self.theme_kind();
        let (scene, owners) = match self.symed_shown() {
            Some(sym) => build_scene(sym, self.symed.unit, self.symed.repr, theme),
            None => (Scene::new(), BTreeMap::new()),
        };
        self.symed.scene = scene;
        self.symed.owners = owners;
        self.symed.built = Some(key);
    }

    /// The selection restricted to items that still exist in the shown symbol.
    fn symed_items(&self) -> Vec<Item> {
        let Some(sym) = self.symed_symbol() else {
            return Vec::new();
        };
        self.selection
            .get(&Mode::SymbolEditor)
            .into_iter()
            .flatten()
            .filter_map(|id| item_of(*id))
            .filter(|it| match it {
                Item::Pin(i) => *i < sym.pins.len(),
                Item::Graphic(i) => *i < sym.graphics.len(),
            })
            .collect()
    }

    fn symed_select(&mut self, items: &[Item]) {
        self.selection.insert(
            Mode::SymbolEditor,
            items.iter().map(|i| id_of(*i)).collect(),
        );
    }

    // -- editing ---------------------------------------------------------------------------------

    /// Applies `f` to a copy of the edited symbol and commits the result as one undo step.
    fn symed_edit<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Symbol) -> Result<R, se::SymbolError>,
    ) -> Result<R, String> {
        let name = self.symed.lib_id.clone().ok_or("no symbol is open")?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before = open
            .engine
            .project()
            .schematic()
            .symbol(&name)
            .cloned()
            .ok_or_else(|| format!("symbol {name} is no longer in the project"))?;
        let mut after = before.clone();
        let result = f(&mut after).map_err(|e| e.to_string())?;
        if after != before {
            open.engine
                .execute(SetSymbolCommand::new(&name, Some(before), Some(after)).with_label(label))
                .map_err(|e| e.to_string())?;
            self.symed.version += 1;
        }
        self.command_log.push(label.to_owned());
        Ok(result)
    }

    fn symed_log_err(&mut self, label: &str, e: String) {
        self.log(MessageLevel::Error, format!("{label}: {e}"));
    }

    fn symed_add_graphic(&mut self, label: &str, graphic: Graphic) {
        let (unit, repr) = (self.symed.unit, self.symed.repr);
        let item = SymbolGraphic {
            graphic,
            unit,
            representation: repr,
        };
        match self.symed_edit(label, |s| se::add_graphic(s, item)) {
            Ok(i) => self.symed_select(&[Item::Graphic(i)]),
            Err(e) => self.symed_log_err(label, e),
        }
    }

    fn symed_add_pin(&mut self, at: Point) {
        let (unit, repr, t) = (self.symed.unit, self.symed.repr, self.symed.template);
        let result = self.symed_edit("Add pin", |s| {
            let number = se::next_pin_number(s, repr);
            se::add_pin(
                s,
                Pin {
                    number,
                    name: "~".into(),
                    kind: t.kind,
                    position: at,
                    orientation: t.orientation,
                    length: t.length,
                    unit,
                    representation: repr,
                },
            )
        });
        match result {
            Ok(i) => self.symed_select(&[Item::Pin(i)]),
            Err(e) => self.symed_log_err("Add pin", e),
        }
    }

    fn symed_delete(&mut self) -> Result<(), String> {
        let items = self.symed_items();
        if items.is_empty() {
            return Err("select something to delete".into());
        }
        let n = self.symed_edit("Delete items", |s| se::delete_items(s, &items))?;
        self.selection.insert(Mode::SymbolEditor, BTreeSet::new());
        self.log(MessageLevel::Info, format!("Deleted {n} item(s)"));
        Ok(())
    }

    fn symed_transform(&mut self, rotate: bool, horizontal: bool) -> Result<(), String> {
        let items = self.symed_items();
        if items.is_empty() {
            return Err("select something first".into());
        }
        let sym = self.symed_symbol().ok_or("no symbol is open")?;
        let (lo, hi) = se::items_bounds(sym, &items).ok_or("nothing to transform")?;
        // Pivot on the grid so pins stay on the grid.
        let g = self.grid.spacing.max(1);
        let snap = |v: i64| (v as f64 / g as f64).round() as i64 * g;
        let pivot = Point::new(snap((lo.x + hi.x) / 2), snap((lo.y + hi.y) / 2));
        if rotate {
            self.symed_edit("Rotate items", |s| se::rotate_items(s, &items, pivot))
        } else if horizontal {
            self.symed_edit("Mirror left-right", |s| {
                se::mirror_items(s, &items, pivot, true)
            })
        } else {
            self.symed_edit("Mirror top-bottom", |s| {
                se::mirror_items(s, &items, pivot, false)
            })
        }
    }

    // -- item properties (Inspector) -----------------------------------------------------------

    pub(crate) fn symed_set_item_property(
        &mut self,
        id: u64,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        self.symed_require()?;
        let item = item_of(Id(id)).ok_or("that is not a symbol item")?;
        let (key, value) = Self::symed_normalise_length(key, value)?;
        let label = format!("Edit {key}");
        match item {
            Item::Pin(i) => self.symed_edit(&label, |s| se::set_pin_field(s, i, &key, &value)),
            Item::Graphic(i) => {
                self.symed_edit(&label, |s| se::set_graphic_field(s, i, &key, &value))
            }
        }
    }

    /// The Inspector edits lengths in mm; the model stores nm.
    fn symed_normalise_length(key: &str, value: &str) -> Result<(String, String), String> {
        Ok(match key {
            "length_mm" => ("length".into(), parse_mm(value, "length")?.to_string()),
            "x_mm" => ("x".into(), parse_mm(value, "x")?.to_string()),
            "y_mm" => ("y".into(), parse_mm(value, "y")?.to_string()),
            "width_mm" => ("width".into(), parse_mm(value, "line width")?.to_string()),
            "height_mm" => ("height".into(), parse_mm(value, "text height")?.to_string()),
            other => (other.to_owned(), value.to_owned()),
        })
    }

    pub(crate) fn symed_summaries(&self) -> Vec<ObjectSummary> {
        let Some(sym) = self.symed_symbol() else {
            return Vec::new();
        };
        let unit_opts: Vec<(String, String)> =
            std::iter::once(("0".to_owned(), "All units".to_owned()))
                .chain(
                    (1..=sym.unit_count)
                        .map(|u| (u.to_string(), format!("Unit {}", unit_letter(u)))),
                )
                .collect();
        let unit_opts: Vec<(&str, &str)> = unit_opts
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let repr_opts = [("0", "Both"), ("1", "Normal"), ("2", "Alternate")];
        let mut out = Vec::new();
        for item in self.symed_items() {
            let id = id_of(item);
            match item {
                Item::Pin(i) => {
                    let Some(p) = sym.pins.get(i) else { continue };
                    out.push(ObjectSummary {
                        id: id.0,
                        kind: "pin".into(),
                        title: format!("Pin {} {}", p.number, p.name),
                        properties: vec![
                            prop("number", "Number", &p.number, true),
                            prop("name", "Name", &p.name, true),
                            choice(
                                "type",
                                "Electrical type",
                                se::pin_type_id(p.kind),
                                &pin_type_options(),
                            ),
                            choice(
                                "orientation",
                                "Orientation",
                                se::direction_id(p.orientation),
                                &DIRECTIONS,
                            ),
                            prop("length_mm", "Length (mm)", mm_str(p.length), true),
                            prop("x_mm", "X (mm)", mm_str(p.position.x), true),
                            prop("y_mm", "Y (mm)", mm_str(p.position.y), true),
                            choice("unit", "Unit", p.unit.to_string(), &unit_opts),
                            choice(
                                "representation",
                                "Representation",
                                p.representation.to_string(),
                                &repr_opts,
                            ),
                        ],
                    });
                }
                Item::Graphic(i) => {
                    let Some(g) = sym.graphics.get(i) else {
                        continue;
                    };
                    let mut props = Vec::new();
                    let kind = match &g.graphic {
                        Graphic::Line { width, .. } => {
                            props.push(prop("width_mm", "Line width (mm)", mm_str(*width), true));
                            "line"
                        }
                        Graphic::Rect { width, filled, .. } => {
                            props.push(prop("width_mm", "Line width (mm)", mm_str(*width), true));
                            props.push(choice(
                                "filled",
                                "Filled",
                                filled.to_string(),
                                &[("true", "Yes"), ("false", "No")],
                            ));
                            "rectangle"
                        }
                        Graphic::Circle { width, filled, .. } => {
                            props.push(prop("width_mm", "Line width (mm)", mm_str(*width), true));
                            props.push(choice(
                                "filled",
                                "Filled",
                                filled.to_string(),
                                &[("true", "Yes"), ("false", "No")],
                            ));
                            "circle"
                        }
                        Graphic::Arc { width, .. } => {
                            props.push(prop("width_mm", "Line width (mm)", mm_str(*width), true));
                            "arc"
                        }
                        Graphic::Text { text, height, .. } => {
                            props.push(prop("text", "Text", text, true));
                            props.push(prop("height_mm", "Height (mm)", mm_str(*height), true));
                            "text"
                        }
                    };
                    props.push(choice("unit", "Unit", g.unit.to_string(), &unit_opts));
                    props.push(choice(
                        "representation",
                        "Representation",
                        g.representation.to_string(),
                        &repr_opts,
                    ));
                    out.push(ObjectSummary {
                        id: id.0,
                        kind: kind.into(),
                        title: kind[..1].to_uppercase() + &kind[1..],
                        properties: props,
                    });
                }
            }
        }
        out
    }

    pub(crate) fn symed_describe(&self, id: Id) -> Option<String> {
        let sym = self.symed_symbol()?;
        match item_of(id)? {
            Item::Pin(i) => {
                let p = sym.pins.get(i)?;
                Some(format!(
                    "Pin {} {} ({})",
                    p.number,
                    p.name,
                    se::pin_type_label(p.kind)
                ))
            }
            Item::Graphic(i) => Some(
                match &sym.graphics.get(i)?.graphic {
                    Graphic::Line { .. } => "Line",
                    Graphic::Rect { .. } => "Rectangle",
                    Graphic::Circle { .. } => "Circle",
                    Graphic::Arc { .. } => "Arc",
                    Graphic::Text { .. } => "Text",
                }
                .to_owned(),
            ),
        }
    }

    pub(crate) fn symed_prune_selection(&mut self) {
        let valid: BTreeSet<Id> = {
            let Some(sym) = self.symed_symbol() else {
                self.selection.remove(&Mode::SymbolEditor);
                return;
            };
            (0..sym.graphics.len())
                .map(|i| id_of(Item::Graphic(i)))
                .chain((0..sym.pins.len()).map(|i| id_of(Item::Pin(i))))
                .collect()
        };
        if let Some(sel) = self.selection.get_mut(&Mode::SymbolEditor) {
            sel.retain(|id| valid.contains(id));
        }
    }

    pub(crate) fn symed_select_all(&mut self) {
        let (unit, repr) = (self.symed.unit, self.symed.repr);
        let Some(sym) = self.symed_symbol() else {
            return;
        };
        let all: BTreeSet<Id> = sym
            .graphics
            .iter()
            .enumerate()
            .filter(|(_, g)| g.is_visible_in(unit, repr))
            .map(|(i, _)| id_of(Item::Graphic(i)))
            .chain(
                sym.pins
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_visible_in(unit, repr))
                    .map(|(i, _)| id_of(Item::Pin(i))),
            )
            .collect();
        self.selection.insert(Mode::SymbolEditor, all);
    }

    // -- editor info for the UI ------------------------------------------------------------------

    pub(crate) fn symbol_editor_info(&self) -> Option<SymbolEditorInfo> {
        let sym = self.symed_symbol()?;
        let open = self.open.as_ref()?;
        let used_by = open
            .engine
            .project()
            .schematic()
            .sheets()
            .values()
            .flat_map(|s| s.objects.values())
            .filter(|o| match o {
                Object::Component(c) => c.lib_id == sym.name,
                Object::PowerSymbol(p) => p.lib_id == sym.name,
                _ => false,
            })
            .count() as u32;
        let t = self.symed.template;
        let yes_no = [("true", "Yes"), ("false", "No")];
        let fields = vec![
            prop("name", "Name (Library:Symbol)", &sym.name, true),
            prop(
                "reference_prefix",
                "Reference prefix",
                &sym.reference_prefix,
                true,
            ),
            prop("default_value", "Default value", &sym.default_value, true),
            choice(
                "is_power",
                "Power symbol",
                sym.is_power.to_string(),
                &yes_no,
            ),
            prop("unit_count", "Units", sym.unit_count.to_string(), true),
            prop("description", "Description", &sym.meta.description, true),
            prop("keywords", "Keywords", &sym.meta.keywords, true),
            prop("datasheet", "Datasheet", &sym.meta.datasheet, true),
            prop("manufacturer", "Manufacturer", &sym.meta.manufacturer, true),
            prop("mpn", "MPN", &sym.meta.mpn, true),
            prop(
                "default_footprint",
                "Default footprint",
                &sym.meta.default_footprint,
                true,
            ),
            prop(
                "footprint_filters",
                "Footprint filters",
                sym.meta.footprint_filters.join(", "),
                true,
            ),
        ];
        let custom = sym
            .meta
            .properties
            .iter()
            .map(|(k, v)| prop(&format!("property:{k}"), k, v, true))
            .collect();
        Some(SymbolEditorInfo {
            lib_id: sym.name.clone(),
            unit: u32::from(self.symed.unit),
            unit_count: u32::from(sym.unit_count),
            representation: u32::from(self.symed.repr),
            has_alternate: sym.has_alternate(),
            used_by,
            pin_template: vec![
                choice(
                    "type",
                    "Electrical type",
                    se::pin_type_id(t.kind),
                    &pin_type_options(),
                ),
                choice(
                    "orientation",
                    "Orientation",
                    se::direction_id(t.orientation),
                    &DIRECTIONS,
                ),
                prop("length_mm", "Length (mm)", mm_str(t.length), true),
            ],
            fields,
            custom,
            warnings: symbol_warnings(sym),
        })
    }

    // -- commands --------------------------------------------------------------------------------

    /// Editor commands issued while the Symbol Editor is the active mode.
    pub(crate) fn symed_editor_command(&mut self, name: &str) -> Result<(), String> {
        if let Some(rest) = name.strip_prefix("tool:") {
            let (id, arg) = match rest.split_once(':') {
                Some((id, arg)) => (id, Some(arg)),
                None => (rest, None),
            };
            return self.symed_set_tool(id, arg);
        }
        match name {
            "select_all" => self.symed_select_all(),
            "clear_selection" => {
                self.selection.insert(Mode::SymbolEditor, BTreeSet::new());
            }
            "cancel" => self.symed_cancel(),
            "delete" => self.symed_delete()?,
            "rotate" => {
                if self.symed.tool == SymTool::Pin {
                    let o = self.symed.template.orientation;
                    self.symed.template.orientation = match o {
                        Direction::Right => Direction::Up,
                        Direction::Up => Direction::Left,
                        Direction::Left => Direction::Down,
                        Direction::Down => Direction::Right,
                    };
                } else {
                    self.symed_transform(true, true)?;
                }
            }
            "mirror_x" => self.symed_transform(false, true)?,
            "mirror_y" => self.symed_transform(false, false)?,
            other => return Err(format!("“{other}” is not available in the Symbol Editor")),
        }
        Ok(())
    }

    fn symed_set_tool(&mut self, id: &str, arg: Option<&str>) -> Result<(), String> {
        let tool = match id {
            "select" => SymTool::Select,
            "pin" => SymTool::Pin,
            "line" => SymTool::Line,
            "rect" => SymTool::Rect,
            "circle" => SymTool::Circle,
            "arc" => SymTool::Arc,
            "text" => match arg.map(str::trim) {
                Some(t) if !t.is_empty() => SymTool::Text(t.to_owned()),
                _ => return Err("the text tool needs the text to place".into()),
            },
            other => return Err(format!("unknown Symbol Editor tool: {other}")),
        };
        if tool != SymTool::Select {
            self.symed_require()?;
        }
        self.symed.pts.clear();
        self.symed.drag = None;
        self.symed.tool = tool;
        Ok(())
    }

    fn symed_cancel(&mut self) {
        if !self.symed.pts.is_empty() || self.symed.drag.is_some() {
            self.symed.pts.clear();
            self.symed.drag = None;
            self.symed.preview = None;
            self.symed.version += 1;
            self.gesture = Gesture::None;
        } else if self.symed.tool != SymTool::Select {
            self.symed.tool = SymTool::Select;
        } else {
            self.selection.insert(Mode::SymbolEditor, BTreeSet::new());
        }
    }

    /// `symbol_editor_command` IPC: lifecycle and view commands.
    ///
    /// Names: `open` (arg: lib id), `new` (arg: `Library:Name`), `close`, `unit` (arg: n),
    /// `repr` (arg: 1|2), `add_unit`, `remove_unit`, `copy_unit` (arg: `from:to`),
    /// `alternate_create`, `alternate_remove`, `pin_template` (arg: `key=value`),
    /// `save_to_library` (arg: `scope:Library`), `delete_symbol`.
    pub fn symbol_editor_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no project is open".into());
        }
        let need = |what: &str| -> Result<&str, String> {
            arg.map(str::trim)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| format!("{name} needs {what}"))
        };
        match name {
            "open" => self.symed_open(need("a symbol")?),
            "new" => self.symed_new(need("a name like Library:Symbol")?),
            "close" => {
                self.symed.lib_id = None;
                self.symed.preview = None;
                self.symed.pts.clear();
                self.symed.drag = None;
                self.selection.remove(&Mode::SymbolEditor);
                self.symed.version += 1;
                Ok(())
            }
            "unit" => {
                self.symed_require()?;
                let n: u8 = need("a unit number")?
                    .parse()
                    .map_err(|_| "unit must be a number")?;
                let count = self.symed_symbol().map_or(1, |s| s.unit_count);
                if n == 0 || n > count {
                    return Err(format!("the symbol has {count} unit(s)"));
                }
                self.symed.unit = n;
                self.selection.insert(Mode::SymbolEditor, BTreeSet::new());
                Ok(())
            }
            "repr" => {
                self.symed_require()?;
                let n: u8 = need("1 or 2")?
                    .parse()
                    .map_err(|_| "representation must be 1 or 2")?;
                if n == REPR_ALTERNATE && !self.symed_symbol().is_some_and(Symbol::has_alternate) {
                    return Err(
                        "this symbol has no alternate representation yet; create one first".into(),
                    );
                }
                if n != REPR_NORMAL && n != REPR_ALTERNATE {
                    return Err("representation must be 1 (normal) or 2 (alternate)".into());
                }
                self.symed.repr = n;
                self.selection.insert(Mode::SymbolEditor, BTreeSet::new());
                Ok(())
            }
            "add_unit" => {
                self.symed_require()?;
                let next = self.symed_symbol().map_or(1, |s| s.unit_count) + 1;
                self.symed_edit("Add unit", |s| {
                    se::set_symbol_field(s, "unit_count", &next.to_string())
                })?;
                self.symed.unit = next;
                Ok(())
            }
            "remove_unit" => {
                self.symed_require()?;
                let count = self.symed_symbol().map_or(1, |s| s.unit_count);
                if count <= 1 {
                    return Err("a symbol needs at least one unit".into());
                }
                self.symed_edit("Remove unit", |s| {
                    se::set_symbol_field(s, "unit_count", &(count - 1).to_string())
                })?;
                self.symed.unit = self.symed.unit.min(count - 1);
                Ok(())
            }
            "copy_unit" => {
                self.symed_require()?;
                let (a, b) = need("from:to")?
                    .split_once(':')
                    .ok_or("copy_unit needs from:to")?;
                let from: u8 = a.parse().map_err(|_| "unit numbers must be numbers")?;
                let to: u8 = b.parse().map_err(|_| "unit numbers must be numbers")?;
                self.symed_edit("Copy unit", |s| se::copy_unit(s, from, to))?;
                Ok(())
            }
            "alternate_create" => {
                self.symed_require()?;
                let n = self.symed_edit(
                    "Create alternate representation",
                    se::create_alternate_from_normal,
                )?;
                self.symed.repr = REPR_ALTERNATE;
                self.log(
                    MessageLevel::Info,
                    format!("Created the alternate representation from {n} item(s)"),
                );
                Ok(())
            }
            "alternate_remove" => {
                self.symed_require()?;
                self.symed_symbol_uses_alternate_check()?;
                self.symed_edit("Remove alternate representation", se::remove_alternate)?;
                self.symed.repr = REPR_NORMAL;
                Ok(())
            }
            "pin_template" => {
                let (k, v) = need("key=value")?
                    .split_once('=')
                    .ok_or("pin_template needs key=value")?;
                let t = &mut self.symed.template;
                match k {
                    "type" => t.kind = se::parse_pin_type(v).ok_or("unknown pin type")?,
                    "orientation" => {
                        t.orientation = se::parse_direction(v).ok_or("unknown direction")?
                    }
                    "length_mm" => {
                        let l = parse_mm(v, "length")?;
                        if l < 0 {
                            return Err("pin length must not be negative".into());
                        }
                        t.length = l;
                    }
                    other => return Err(format!("unknown pin template field {other}")),
                }
                Ok(())
            }
            "save_to_library" => self.symed_save_to_library(need("scope:Library")?),
            "delete_symbol" => self.symed_delete_symbol(),
            other => Err(format!("unknown Symbol Editor command: {other}")),
        }
    }

    fn symed_symbol_uses_alternate_check(&self) -> Result<(), String> {
        let (Some(sym), Some(open)) = (self.symed_symbol(), self.open.as_ref()) else {
            return Ok(());
        };
        let used = open
            .engine
            .project()
            .schematic()
            .sheets()
            .values()
            .flat_map(|s| s.objects.values())
            .any(|o| matches!(o, Object::Component(c) if c.lib_id == sym.name && c.representation == REPR_ALTERNATE));
        if used {
            Err("placed components use the alternate representation; change them first".into())
        } else {
            Ok(())
        }
    }

    /// Sets a symbol-level field (`symbol_editor_set` IPC). Renaming is a project-wide operation.
    pub fn symbol_editor_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.symed_require()?;
        if key == "name" {
            return self.symed_rename(value.trim());
        }
        let label = format!("Edit symbol {key}");
        self.symed_edit(&label, |s| se::set_symbol_field(s, key, value))
    }

    fn symed_rename(&mut self, new_name: &str) -> Result<(), String> {
        se::validate_name(new_name).map_err(|e| e.to_string())?;
        if split_lib_id(new_name).is_none() {
            return Err("symbol names look like Library:Name".into());
        }
        let old = self.symed.lib_id.clone().ok_or("no symbol is open")?;
        if old == new_name {
            return Ok(());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        let sch = open.engine.project().schematic();
        if sch.symbol(new_name).is_some() {
            return Err(format!(
                "a symbol named {new_name} already exists in this project"
            ));
        }
        let before = sch.symbol(&old).cloned().ok_or("symbol is missing")?;
        let mut renamed = before.clone();
        renamed.name = new_name.to_owned();
        // Retarget every instance in the same undo step.
        let mut changes = Vec::new();
        for (sid, sheet) in sch.sheets() {
            for (oid, obj) in &sheet.objects {
                let mut new = obj.clone();
                let hit = match &mut new {
                    Object::Component(c) if c.lib_id == old => {
                        c.lib_id = new_name.to_owned();
                        true
                    }
                    Object::PowerSymbol(p) if p.lib_id == old => {
                        p.lib_id = new_name.to_owned();
                        true
                    }
                    _ => false,
                };
                if hit {
                    changes.push(Change::modify(*sid, *oid, obj.clone(), new));
                }
            }
        }
        let new_key = new_name.to_owned();
        open.engine
            .transaction(format!("Rename symbol to {new_name}"), |e| {
                e.execute(SetSymbolCommand::new(&new_key, None, Some(renamed)))?;
                if !changes.is_empty() {
                    e.execute(SchematicEditCommand::new("Retarget instances", changes))?;
                }
                e.execute(SetSymbolCommand::new(&old, Some(before), None))
            })
            .map_err(|e| e.to_string())?;
        self.symed.lib_id = Some(new_key);
        self.symed.version += 1;
        Ok(())
    }

    fn symed_open(&mut self, lib_id: &str) -> Result<(), String> {
        let in_project = self
            .open
            .as_ref()
            .is_some_and(|o| o.engine.project().schematic().symbol(lib_id).is_some());
        if !in_project {
            let sym = self
                .find_library_symbol(lib_id)
                .ok_or_else(|| format!("symbol {lib_id} was not found in any library"))?;
            let open = self.open.as_mut().ok_or("no project is open")?;
            open.engine
                .execute(
                    SetSymbolCommand::new(lib_id, None, Some(sym))
                        .with_label(format!("Open symbol {lib_id}")),
                )
                .map_err(|e| e.to_string())?;
        }
        self.symed.lib_id = Some(lib_id.to_owned());
        self.symed.unit = 1;
        self.symed.repr = REPR_NORMAL;
        self.symed.tool = SymTool::Select;
        self.symed.pts.clear();
        self.symed.drag = None;
        self.symed.preview = None;
        self.symed.version += 1;
        self.selection.remove(&Mode::SymbolEditor);
        self.fitted.remove(&Mode::SymbolEditor);
        Ok(())
    }

    fn symed_new(&mut self, lib_id: &str) -> Result<(), String> {
        se::validate_name(lib_id).map_err(|e| e.to_string())?;
        if split_lib_id(lib_id).is_none() {
            return Err("symbol names look like Library:Name".into());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        if open.engine.project().schematic().symbol(lib_id).is_some() {
            return Err(format!("{lib_id} already exists in this project"));
        }
        let mut sym = Symbol::new(lib_id, "U");
        sym.default_value = lib_id.rsplit(':').next().unwrap_or(lib_id).to_owned();
        open.engine
            .execute(SetSymbolCommand::new(lib_id, None, Some(sym)))
            .map_err(|e| e.to_string())?;
        self.symed_open(lib_id)
    }

    fn symed_delete_symbol(&mut self) -> Result<(), String> {
        let name = self.symed.lib_id.clone().ok_or("no symbol is open")?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before = open
            .engine
            .project()
            .schematic()
            .symbol(&name)
            .cloned()
            .ok_or("symbol is missing")?;
        open.engine
            .execute(SetSymbolCommand::new(&name, Some(before), None))
            .map_err(|e| e.to_string())?;
        self.symed.lib_id = None;
        self.symed.version += 1;
        self.selection.remove(&Mode::SymbolEditor);
        Ok(())
    }

    // -- libraries -------------------------------------------------------------------------------

    /// Library folders, project first.
    pub(crate) fn library_dirs(&self) -> Vec<(&'static str, LibraryDir)> {
        let mut v = Vec::new();
        if let Some(open) = &self.open {
            v.push(("project", LibraryDir::new(open.root.join("libraries"))));
        }
        if let Some(dir) = &self.user_libs {
            v.push(("user", LibraryDir::new(dir)));
        }
        v
    }

    /// Looks `lib_id` up in the built-in set and the library folders.
    pub(crate) fn find_library_symbol(&self, lib_id: &str) -> Option<Symbol> {
        let (lib, name) = split_lib_id(lib_id)?;
        if lib == "Buture" {
            return builtin::symbols().into_iter().find(|s| s.name == lib_id);
        }
        if lib == buturecad_libraries::standard::NAME {
            return buturecad_libraries::standard::symbol_library()
                .ok()
                .and_then(|l| l.instantiate(name));
        }
        self.library_dirs().into_iter().find_map(|(_, dir)| {
            dir.load_symbol_library(lib)
                .ok()
                .and_then(|l| l.instantiate(name))
        })
    }

    pub fn symbol_libraries(&self) -> Vec<LibraryInfo> {
        let mut out = vec![LibraryInfo {
            name: "Buture".into(),
            scope: "builtin".into(),
            item_count: builtin::symbols().len() as u32,
            error: None,
        }];
        out.push(LibraryInfo {
            name: buturecad_libraries::standard::NAME.into(),
            scope: "builtin".into(),
            item_count: buturecad_libraries::standard::symbols().len() as u32,
            error: None,
        });
        for (scope, dir) in self.library_dirs() {
            for l in dir.list_symbol_libraries() {
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

    /// Writes the edited symbol to a library. Argument: `scope:Library` (`project` or `user`).
    fn symed_save_to_library(&mut self, arg: &str) -> Result<(), String> {
        let (scope, library) = arg.split_once(':').ok_or("expected scope:Library")?;
        if library == "Buture" {
            return Err("“Buture” is the built-in bootstrap library and is read-only; pick another library name".into());
        }
        let dir = self
            .library_dirs()
            .into_iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, d)| d)
            .ok_or_else(|| format!("the {scope} library folder is not available"))?;
        let sym = self.symed_symbol().cloned().ok_or("no symbol is open")?;
        let short = sym.name.rsplit(':').next().unwrap_or(&sym.name).to_owned();
        let wanted = qualify(library, &short);
        let id = dir
            .save_symbol(library, sym.clone())
            .map_err(|e| e.to_string())?;
        // Keep the project copy and the library entry under one name.
        if wanted != sym.name {
            self.symed_rename(&wanted)?;
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

    pub(crate) fn pointer_symbol(&mut self, ev: PointerEvent, out: &mut Outcome) {
        if self.symed_symbol().is_none() {
            return;
        }
        let at = self.cursor;
        match ev.kind {
            PointerKind::Move => {
                match self.symed.drag.clone() {
                    Some(Drag::Pending {
                        start_px,
                        start,
                        items,
                    }) => {
                        let moved =
                            (ev.x - start_px.0).hypot(ev.y - start_px.1) >= DRAG_THRESHOLD_PX;
                        if moved {
                            self.symed.drag = Some(Drag::Moving { start, items });
                            self.symed_update_move(at);
                        }
                    }
                    Some(Drag::Moving { .. }) => self.symed_update_move(at),
                    None => {
                        if !self.update_box(ev.x, ev.y) {
                            self.hover = self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0);
                        }
                    }
                }
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 0 => self.symed_down(ev, at, out),
            PointerKind::Down if ev.button == 2 => {
                if !self.symed.pts.is_empty() {
                    self.symed.pts.clear();
                } else if self.symed.tool != SymTool::Select {
                    self.symed.tool = SymTool::Select;
                } else if let Some(owner) = self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0) {
                    self.select_owner(owner, false);
                    out.selection_changed = true;
                }
                out.redraw = true;
            }
            PointerKind::Up => {
                match self.symed.drag.take() {
                    Some(Drag::Moving { start, items }) => {
                        self.symed_finish_move(start, at, &items, out);
                    }
                    Some(Drag::Pending { .. }) | None => {
                        if self.finish_box() {
                            self.selection
                                .entry(Mode::SymbolEditor)
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

    fn symed_down(&mut self, ev: PointerEvent, at: Point, out: &mut Outcome) {
        match self.symed.tool.clone() {
            SymTool::Select => {
                let additive = ev.shift || ev.ctrl;
                match self.pick_owner(ev.x, ev.y).filter(|o| o.0 != 0) {
                    Some(owner) => {
                        self.select_owner(owner, additive);
                        out.selection_changed = true;
                        let items = self.symed_items();
                        if !additive && items.iter().any(|i| id_of(*i) == owner) {
                            self.symed.drag = Some(Drag::Pending {
                                start_px: (ev.x, ev.y),
                                start: at,
                                items,
                            });
                        }
                    }
                    None => self.begin_box(ev.x, ev.y, additive),
                }
            }
            SymTool::Pin => {
                self.symed_add_pin(at);
                out.model_changed = true;
                out.selection_changed = true;
            }
            SymTool::Text(text) => {
                let g = Graphic::Text {
                    origin: at,
                    height: 1_270_000,
                    text,
                };
                self.symed_add_graphic("Add text", g);
                out.model_changed = true;
                out.selection_changed = true;
            }
            SymTool::Line | SymTool::Rect | SymTool::Circle | SymTool::Arc => {
                self.symed_draw_click(at, out);
            }
        }
        out.redraw = true;
    }

    /// One click of a multi-click primitive (line, rectangle, circle, arc).
    fn symed_draw_click(&mut self, at: Point, out: &mut Outcome) {
        if self.symed.pts.last() == Some(&at) {
            return; // a double click on the same point adds nothing
        }
        self.symed.pts.push(at);
        let pts = self.symed.pts.clone();
        let width = 150_000;
        let done = match (&self.symed.tool, pts.as_slice()) {
            (SymTool::Line, [a, b]) => Some((
                "Add line",
                Graphic::Line {
                    segment: Segment::new(*a, *b),
                    width,
                },
            )),
            (SymTool::Rect, [a, b]) => match Rectangle::new(*a, *b) {
                Ok(rect) => Some((
                    "Add rectangle",
                    Graphic::Rect {
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
                    self.symed.pts.pop();
                    return;
                }
            },
            (SymTool::Circle, [c, r]) => {
                let radius = c.distance(*r).round() as i64;
                if radius <= 0 {
                    self.symed.pts.pop();
                    return;
                }
                Some((
                    "Add circle",
                    Graphic::Circle {
                        circle: Circle { center: *c, radius },
                        filled: false,
                        width,
                    },
                ))
            }
            (SymTool::Arc, [c, s, e]) => match arc_through(*c, *s, *e) {
                Some(arc) => Some(("Add arc", Graphic::Arc { arc, width })),
                None => {
                    self.log(
                        MessageLevel::Warn,
                        "That arc is degenerate; pick a different end point",
                    );
                    self.symed.pts.pop();
                    return;
                }
            },
            _ => None,
        };
        if let Some((label, graphic)) = done {
            let chain = matches!(self.symed.tool, SymTool::Line);
            self.symed.pts.clear();
            if chain {
                self.symed.pts.push(at);
            }
            self.symed_add_graphic(label, graphic);
            out.model_changed = true;
            out.selection_changed = true;
        }
    }

    fn symed_update_move(&mut self, at: Point) {
        let Some(Drag::Moving { start, items }) = self.symed.drag.clone() else {
            return;
        };
        let Some(base) = self.symed_symbol().cloned() else {
            return;
        };
        let mut moved = base;
        if se::move_items(&mut moved, &items, at - start).is_ok() {
            self.symed.preview = Some(moved);
            self.symed.version += 1;
        }
    }

    fn symed_finish_move(&mut self, start: Point, at: Point, items: &[Item], out: &mut Outcome) {
        self.symed.preview = None;
        self.symed.version += 1;
        let delta = at - start;
        if delta.is_zero() {
            return;
        }
        match self.symed_edit("Move items", |s| se::move_items(s, items, delta)) {
            Ok(()) => out.model_changed = true,
            Err(e) => self.symed_log_err("Move items", e),
        }
    }

    /// Transient shapes: the primitive being drawn and the pin about to be placed.
    pub(crate) fn symed_preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        if self.symed_symbol().is_none() {
            return Vec::new();
        }
        let colour = match self.theme {
            ThemeName::Dark => Theme::dark().selection,
            ThemeName::Light => Theme::light().selection,
        };
        let at = self.cursor;
        let pts = &self.symed.pts;
        let mut out: Vec<(Shape, Rgba)> = origin_marks(self.theme, 2_540_000);
        let line = |a: Point, b: Point| Shape::Line {
            segment: Segment::new(a, b),
            width: 150_000,
        };
        match (&self.symed.tool, pts.as_slice()) {
            (SymTool::Line, [a]) if *a != at => out.push((line(*a, at), colour)),
            (SymTool::Rect, [a]) => {
                if let Ok(rect) = Rectangle::new(*a, at) {
                    out.push((
                        Shape::Rect {
                            rect,
                            filled: false,
                            stroke: 150_000,
                        },
                        colour,
                    ));
                }
            }
            (SymTool::Circle, [c]) => {
                let radius = c.distance(at).round() as i64;
                if radius > 0 {
                    out.push((
                        Shape::Circle {
                            circle: Circle { center: *c, radius },
                            filled: false,
                            stroke: 150_000,
                        },
                        colour,
                    ));
                }
            }
            (SymTool::Arc, [c]) => out.push((line(*c, at), colour)),
            (SymTool::Arc, [c, s]) => {
                out.push((line(*c, *s), colour));
                if let Some(arc) = arc_through(*c, *s, at) {
                    out.push((
                        Shape::Arc {
                            arc,
                            width: 150_000,
                        },
                        colour,
                    ));
                }
            }
            (SymTool::Pin, _) => {
                let t = self.symed.template;
                let pin = Pin {
                    number: se::next_pin_number(
                        self.symed_symbol().unwrap_or(&Symbol::new("x", "U")),
                        self.symed.repr,
                    ),
                    name: "~".into(),
                    kind: t.kind,
                    position: at,
                    orientation: t.orientation,
                    length: t.length,
                    unit: self.symed.unit,
                    representation: self.symed.repr,
                };
                out.extend(
                    pin_shapes(&pin)
                        .into_iter()
                        .filter(|(l, _)| *l != sb::L_HIT)
                        .map(|(_, s)| (s, colour)),
                );
            }
            (SymTool::Text(text), _) => out.push((
                Shape::Text {
                    origin: at,
                    height: 1_270_000,
                    text: text.clone(),
                },
                colour,
            )),
            _ => {}
        }
        out
    }

    pub(crate) fn symed_hint(&self) -> String {
        if self.symed_symbol().is_none() {
            return "Open or create a symbol to start editing.".into();
        }
        match (&self.symed.tool, self.symed.pts.len()) {
            (SymTool::Select, _) => {
                "Select: click, drag a box, drag items to move. R rotates, X/Y mirror, Del deletes."
                    .into()
            }
            (SymTool::Pin, _) => {
                "Pin: click to place at the connection point. R rotates the pin, Esc stops.".into()
            }
            (SymTool::Line, 0) => "Line: click the start point.".into(),
            (SymTool::Line, _) => {
                "Line: click the next point (chains). Right click or Esc ends.".into()
            }
            (SymTool::Rect, 0) => "Rectangle: click the first corner.".into(),
            (SymTool::Rect, _) => "Rectangle: click the opposite corner.".into(),
            (SymTool::Circle, 0) => "Circle: click the center.".into(),
            (SymTool::Circle, _) => "Circle: click a point on the circle.".into(),
            (SymTool::Arc, 0) => "Arc: click the center.".into(),
            (SymTool::Arc, 1) => "Arc: click the start point.".into(),
            (SymTool::Arc, _) => "Arc: click the end direction (counter-clockwise).".into(),
            (SymTool::Text(_), _) => "Text: click to place.".into(),
        }
    }
}

/// Counter-clockwise arc about `center` from `start` towards the direction of `towards`; the end
/// point is projected onto the circle through `start`.
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
    // Rounding can put the end point a nanometre off the circle; look at the neighbours too.
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

fn unit_letter(u: u8) -> char {
    (b'A' + (u.saturating_sub(1)).min(25)) as char
}

/// Things that make a symbol hard to use, shown as warnings (they do not block saving).
fn symbol_warnings(sym: &Symbol) -> Vec<String> {
    let mut w = Vec::new();
    for u in 1..=sym.unit_count {
        if sym.pins_for(u, REPR_NORMAL).next().is_none() {
            w.push(format!("Unit {} has no pins", unit_letter(u)));
        }
    }
    if sym.has_alternate() {
        for u in 1..=sym.unit_count {
            if sym.pins_for(u, REPR_ALTERNATE).next().is_none() {
                w.push(format!(
                    "Unit {} has no pins in the alternate representation",
                    unit_letter(u)
                ));
            }
        }
    }
    let g = 1_270_000;
    for p in &sym.pins {
        if p.position.x % g != 0 || p.position.y % g != 0 {
            w.push(format!(
                "Pin {} is off the 1.27 mm grid; wires may not be able to connect to it",
                p.number
            ));
        }
        if p.name.trim().is_empty() {
            w.push(format!(
                "Pin {} has no name (use ~ for an unnamed pin)",
                p.number
            ));
        }
    }
    if sym.is_power && sym.pins.len() != 1 {
        w.push("A power symbol should have exactly one pin".into());
    }
    if !sym.is_power && sym.reference_prefix.trim().is_empty() {
        w.push("The reference prefix is empty; placed parts will be named U1, U2, …".into());
    }
    w
}
