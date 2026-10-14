//! PCB editor: interaction layer over `buturecad_eda_core::pcb_ops` and the board model.
//!
//! Every edit is a `PcbEditCommand` (full before/after object states) through the Command
//! Engine; live previews (dragging, ghosts, the outline being drawn) only touch the scene.

use super::*;
use crate::pcb_scene::{self as ps, object_shapes};
use buturecad_eda_core::commands::{PcbEditCommand, SetFootprintCommand, SetStackupCommand};
use buturecad_eda_core::model::{PcbChange, PcbDoc, PcbObject};
use buturecad_eda_core::pcb_ops::{self, Align, Distribute};
use buturecad_eda_core::ratsnest::{self, RatLine, RatsnestEngine};
use buturecad_eda_core::sync::PlanKind;
use buturecad_geometry::{Rectangle, Segment, Vector};
use buturecad_pcb::{layer, BoardGraphic, Footprint, FpShape, PlacedFootprint, Placement, Stackup};
use buturecad_renderer::{Rgba, Shape};

const OUTLINE_WIDTH: i64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum PcbTool {
    #[default]
    Select,
    OutlineRect,
    OutlineLine,
    PlaceFootprint(String),
    Route,
    Via,
}

impl PcbTool {
    pub fn id(&self) -> &'static str {
        match self {
            PcbTool::Select => "select",
            PcbTool::OutlineRect => "outline_rect",
            PcbTool::OutlineLine => "outline_line",
            PcbTool::PlaceFootprint(_) => "place_footprint",
            PcbTool::Route => "route",
            PcbTool::Via => "via",
        }
    }
    pub fn arg(&self) -> Option<String> {
        match self {
            PcbTool::PlaceFootprint(f) => Some(f.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Drag {
    Pending {
        start_px: (f64, f64),
        start: Point,
        ids: Vec<Id>,
    },
    Moving {
        start: Point,
        ids: Vec<Id>,
    },
    /// Dragging one vertex of a track; coincident ends of same-net tracks on the same layer move
    /// with it. `ends` are (track, end index 0 = start / 1 = end).
    Vertex {
        from: Point,
        ends: Vec<(Id, u8)>,
    },
}

pub(crate) struct PcbEditor {
    pub tool: PcbTool,
    pub(crate) drag: Option<Drag>,
    /// Points of the outline being drawn.
    pub(crate) pts: Vec<Point>,
    pub(crate) pending_rotation: u8,
    pub(crate) pending_flipped: bool,
    /// The footprint of the active placement tool, resolved once (the ghost redraws every frame).
    pub(crate) pending_fp: Option<Footprint>,
    /// Objects whose scene shapes differ from the model (live preview).
    pub(crate) touched: BTreeSet<Id>,
    pub show_ratsnest: bool,
    /// Ratsnest lines of the nets affected by the move in progress.
    pub(crate) rat_preview: BTreeMap<Id, Vec<RatLine>>,
    pub(crate) route: Option<super::pcb_route::Route>,
    /// Manual track width; `None` follows the net class of the net being routed.
    pub(crate) route_width: Option<i64>,
    pub(crate) route_mode: buturecad_eda_core::routing::AngleMode,
    pub(crate) route_flip: bool,
    /// Manual via `(diameter, drill)`; `None` follows the net class.
    pub(crate) via_size: Option<(i64, i64)>,
}

impl Default for PcbEditor {
    fn default() -> Self {
        Self {
            tool: PcbTool::Select,
            drag: None,
            pts: Vec::new(),
            pending_rotation: 0,
            pending_flipped: false,
            pending_fp: None,
            touched: BTreeSet::new(),
            show_ratsnest: true,
            rat_preview: BTreeMap::new(),
            route: None,
            route_width: None,
            route_mode: Default::default(),
            route_flip: false,
            via_size: None,
        }
    }
}

pub(crate) fn mm_str(v: i64) -> String {
    let s = format!("{:.4}", v as f64 / 1e6);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s.to_owned()
    }
}

pub(crate) fn parse_mm(text: &str, what: &str) -> Result<i64, String> {
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
    options: Vec<(String, String)>,
) -> PropertyInfo {
    PropertyInfo {
        options: options
            .into_iter()
            .map(|(value, label)| OptionInfo { value, label })
            .collect(),
        ..prop(key, label, value, true)
    }
}

impl Workspace {
    // -- access ----------------------------------------------------------------------------------

    pub(crate) fn pcb_doc(&self) -> Option<&PcbDoc> {
        self.open.as_ref().map(|o| o.engine.project().pcb())
    }

    fn pcb_ids(&self) -> Vec<Id> {
        self.selection
            .get(&Mode::Pcb)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Runs `changes` through the Command Engine as one undoable step.
    pub(crate) fn pcb_commit(&mut self, label: &str, changes: Vec<PcbChange>) -> bool {
        if changes.is_empty() {
            return false;
        }
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let revision_before = open.engine.revision();
        let dirty = ratsnest::affected_nets(&changes);
        let result = open.engine.execute(PcbEditCommand::new(label, changes));
        self.built_for = None;
        self.pcb_ed.touched.clear();
        self.pcb_ed.rat_preview.clear();
        match result {
            Ok(()) => {
                self.ratsnest_committed(revision_before, &dirty);
                self.command_log.push(label.to_owned());
                true
            }
            Err(e) => {
                self.log(MessageLevel::Error, format!("{label} failed: {e}"));
                false
            }
        }
    }

    /// Runs `f` on the ratsnest of the open board, computing or refreshing it if the board
    /// changed in a way that was not tracked per net (undo, schematic update, placement…).
    pub(crate) fn ratsnest<R>(&self, f: impl FnOnce(&RatsnestEngine) -> R) -> Option<R> {
        let open = self.open.as_ref()?;
        let revision = open.engine.revision();
        let doc = open.engine.project().pcb();
        let mut cache = self.rats.borrow_mut();
        match cache.as_mut() {
            Some((rev, e)) if *rev == revision => {}
            Some((rev, e)) => {
                e.refresh(doc);
                *rev = revision;
            }
            None => *cache = Some((revision, RatsnestEngine::new(doc))),
        }
        cache.as_ref().map(|(_, e)| f(e))
    }

    /// Updates only the nets in `dirty` after a tracked change.
    fn ratsnest_committed(&mut self, revision_before: u64, dirty: &BTreeSet<Id>) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let revision = open.engine.revision();
        let doc = open.engine.project().pcb();
        if let Some((rev, e)) = self.rats.get_mut().as_mut() {
            if *rev == revision_before {
                e.update(doc, dirty);
                *rev = revision;
            }
        }
    }

    pub(crate) fn pcb_new_id(&mut self) -> Id {
        match self.open.as_mut() {
            Some(o) => o.engine.new_id(),
            None => Id(0),
        }
    }

    fn pcb_restore_scene(&mut self) {
        self.pcb_ed.rat_preview.clear();
        let touched = std::mem::take(&mut self.pcb_ed.touched);
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let doc = open.engine.project().pcb();
        for id in touched {
            let o = doc.object(id);
            self.pcb_scene.sync(doc, id, o.as_ref());
        }
    }

    // -- inspector -------------------------------------------------------------------------------

    pub(crate) fn pcb_describe(&self, id: Id) -> Option<String> {
        let doc = self.pcb_doc()?;
        Some(match doc.object(id)? {
            PcbObject::Footprint(f) => format!("{} ({})", f.reference, f.footprint),
            PcbObject::Track(t) => format!(
                "Track {} {:.3} mm{}",
                layer::name(t.layer),
                t.width as f64 / 1e6,
                self.pcb_net_suffix(t.net)
            ),
            PcbObject::Via(v) => format!(
                "Via Ø{:.3} mm{}",
                v.diameter as f64 / 1e6,
                self.pcb_net_suffix(v.net)
            ),
            PcbObject::Zone(z) => format!("Zone {}", layer::name(z.layer)),
            PcbObject::Keepout(_) => "Keepout".into(),
            PcbObject::Text(t) => format!("Text “{}”", t.text),
            PcbObject::Graphic(g) => format!("Graphic {}", layer::name(g.layer)),
            PcbObject::Net(n) => format!("Net {}", n.name),
        })
    }

    fn pcb_net_suffix(&self, n: Option<Id>) -> String {
        n.and_then(|id| self.pcb_doc()?.nets.get(&id))
            .map_or(String::new(), |n| format!(" [{}]", n.name))
    }

    pub(crate) fn pcb_summaries(&self) -> Vec<ObjectSummary> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let net_options: Vec<(String, String)> =
            std::iter::once((String::new(), "(none)".to_owned()))
                .chain(doc.nets.values().map(|n| (n.name.clone(), n.name.clone())))
                .collect();
        let net_name = |n: Option<Id>| {
            n.and_then(|id| doc.nets.get(&id))
                .map_or(String::new(), |n| n.name.clone())
        };
        let copper_options: Vec<(String, String)> = doc
            .stackup
            .copper()
            .into_iter()
            .map(|l| (l.0.to_string(), layer::name(l)))
            .collect();
        let mut out = Vec::new();
        for id in self.pcb_ids() {
            let Some(o) = doc.object(id) else { continue };
            let (kind, title, props) = match &o {
                PcbObject::Footprint(f) => {
                    let mut p = vec![
                        prop("reference", "Reference", &f.reference, true),
                        prop("value", "Value", &f.value, true),
                        prop("footprint", "Footprint", &f.footprint, false),
                        prop("x_mm", "X (mm)", mm_str(f.placement.position.x), true),
                        prop("y_mm", "Y (mm)", mm_str(f.placement.position.y), true),
                        choice(
                            "rotation",
                            "Rotation",
                            (u32::from(f.placement.rotation) * 90).to_string(),
                            ["0", "90", "180", "270"]
                                .iter()
                                .map(|d| ((*d).to_owned(), format!("{d}°")))
                                .collect(),
                        ),
                        choice(
                            "flipped",
                            "Side",
                            f.placement.flipped.to_string(),
                            vec![
                                ("false".into(), "Front".into()),
                                ("true".into(), "Back (flipped)".into()),
                            ],
                        ),
                        choice(
                            "locked",
                            "Locked",
                            f.locked.to_string(),
                            vec![("true".into(), "Yes".into()), ("false".into(), "No".into())],
                        ),
                    ];
                    for (pad, net) in &f.pad_nets {
                        p.push(prop(
                            &format!("pad_{pad}"),
                            &format!("Pad {pad}"),
                            net_name(Some(*net)),
                            false,
                        ));
                    }
                    ("footprint", f.reference.clone(), p)
                }
                PcbObject::Track(t) => (
                    "track",
                    "Track".to_owned(),
                    vec![
                        choice(
                            "layer",
                            "Layer",
                            t.layer.0.to_string(),
                            copper_options.clone(),
                        ),
                        prop("width_mm", "Width (mm)", mm_str(t.width), true),
                        choice("net", "Net", net_name(t.net), net_options.clone()),
                        prop("x1_mm", "Start X (mm)", mm_str(t.segment.a.x), true),
                        prop("y1_mm", "Start Y (mm)", mm_str(t.segment.a.y), true),
                        prop("x2_mm", "End X (mm)", mm_str(t.segment.b.x), true),
                        prop("y2_mm", "End Y (mm)", mm_str(t.segment.b.y), true),
                        prop(
                            "length",
                            "Length (mm)",
                            mm_str(t.segment.length().round() as i64),
                            false,
                        ),
                    ],
                ),
                PcbObject::Via(v) => (
                    "via",
                    "Via".to_owned(),
                    vec![
                        prop("diameter_mm", "Diameter (mm)", mm_str(v.diameter), true),
                        prop("drill_mm", "Drill (mm)", mm_str(v.drill), true),
                        choice("net", "Net", net_name(v.net), net_options.clone()),
                        prop("x_mm", "X (mm)", mm_str(v.position.x), true),
                        prop("y_mm", "Y (mm)", mm_str(v.position.y), true),
                        prop(
                            "via_kind",
                            "Type",
                            match v.kind {
                                buturecad_eda_core::model::ViaKind::Through => "Through",
                                buturecad_eda_core::model::ViaKind::Blind { .. } => "Blind",
                                buturecad_eda_core::model::ViaKind::Buried { .. } => "Buried",
                                buturecad_eda_core::model::ViaKind::Micro { .. } => "Micro",
                            },
                            false,
                        ),
                    ],
                ),
                PcbObject::Zone(z) => (
                    "zone",
                    "Zone".to_owned(),
                    vec![
                        prop("layer_name", "Layer", layer::name(z.layer), false),
                        choice("net", "Net", net_name(z.net), net_options.clone()),
                        prop(
                            "points",
                            "Outline points",
                            z.outline.len().to_string(),
                            false,
                        ),
                    ],
                ),
                PcbObject::Keepout(k) => (
                    "keepout",
                    "Keepout".to_owned(),
                    vec![prop(
                        "points",
                        "Outline points",
                        k.outline.len().to_string(),
                        false,
                    )],
                ),
                PcbObject::Text(t) => (
                    "text",
                    "Text".to_owned(),
                    vec![
                        prop("text", "Text", &t.text, true),
                        prop("height_mm", "Height (mm)", mm_str(t.height), true),
                        prop("x_mm", "X (mm)", mm_str(t.position.x), true),
                        prop("y_mm", "Y (mm)", mm_str(t.position.y), true),
                    ],
                ),
                PcbObject::Graphic(g) => (
                    "graphic",
                    format!("Graphic ({})", layer::name(g.layer)),
                    vec![prop("layer_name", "Layer", layer::name(g.layer), false)],
                ),
                PcbObject::Net(n) => (
                    "net",
                    n.name.clone(),
                    vec![prop("name", "Name", &n.name, false)],
                ),
            };
            out.push(ObjectSummary {
                id: id.0,
                kind: kind.into(),
                title,
                properties: props,
            });
        }
        out
    }

    /// Edits one property of a board object (Inspector).
    pub(crate) fn pcb_set_property(
        &mut self,
        id: u64,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let before = doc.object(Id(id)).ok_or("that object no longer exists")?;
        let net_by_name = |name: &str| -> Result<Option<Id>, String> {
            if name.trim().is_empty() {
                return Ok(None);
            }
            doc.nets
                .iter()
                .find(|(_, n)| n.name == name)
                .map(|(i, _)| Some(*i))
                .ok_or_else(|| format!("there is no net named {name}"))
        };
        let mut after = before.clone();
        match (&mut after, key) {
            (PcbObject::Footprint(f), "reference") => f.reference = value.trim().to_owned(),
            (PcbObject::Footprint(f), "value") => f.value = value.trim().to_owned(),
            (PcbObject::Footprint(f), "x_mm") => f.placement.position.x = parse_mm(value, "X")?,
            (PcbObject::Footprint(f), "y_mm") => f.placement.position.y = parse_mm(value, "Y")?,
            (PcbObject::Footprint(f), "rotation") => {
                let d: i32 = value
                    .trim()
                    .parse()
                    .map_err(|_| "rotation must be 0, 90, 180 or 270")?;
                if d % 90 != 0 {
                    return Err("rotation is a multiple of 90 degrees".into());
                }
                f.placement.rotation = (d / 90).rem_euclid(4) as u8;
            }
            (PcbObject::Footprint(f), "flipped") => f.placement.flipped = value == "true",
            (PcbObject::Footprint(f), "locked") => f.locked = value == "true",
            (PcbObject::Track(t), "layer") => {
                t.layer = layer::LayerId(value.trim().parse().map_err(|_| "unknown layer")?)
            }
            (PcbObject::Track(t), "width_mm") => t.width = parse_mm(value, "width")?,
            (PcbObject::Track(t), "net") => t.net = net_by_name(value)?,
            (PcbObject::Track(t), "x1_mm") => t.segment.a.x = parse_mm(value, "X")?,
            (PcbObject::Track(t), "y1_mm") => t.segment.a.y = parse_mm(value, "Y")?,
            (PcbObject::Track(t), "x2_mm") => t.segment.b.x = parse_mm(value, "X")?,
            (PcbObject::Track(t), "y2_mm") => t.segment.b.y = parse_mm(value, "Y")?,
            (PcbObject::Via(v), "diameter_mm") => v.diameter = parse_mm(value, "diameter")?,
            (PcbObject::Via(v), "drill_mm") => v.drill = parse_mm(value, "drill")?,
            (PcbObject::Via(v), "net") => v.net = net_by_name(value)?,
            (PcbObject::Via(v), "x_mm") => v.position.x = parse_mm(value, "X")?,
            (PcbObject::Via(v), "y_mm") => v.position.y = parse_mm(value, "Y")?,
            (PcbObject::Zone(z), "net") => z.net = net_by_name(value)?,
            (PcbObject::Text(t), "text") => t.text = value.to_owned(),
            (PcbObject::Text(t), "height_mm") => t.height = parse_mm(value, "height")?,
            (PcbObject::Text(t), "x_mm") => t.position.x = parse_mm(value, "X")?,
            (PcbObject::Text(t), "y_mm") => t.position.y = parse_mm(value, "Y")?,
            (o, k) => return Err(format!("property “{k}” cannot be edited on a {}", o.kind())),
        }
        if after == before {
            return Ok(());
        }
        if self.pcb_commit(
            &format!("Edit {key}"),
            vec![PcbChange::modify(Id(id), before, after)],
        ) {
            Ok(())
        } else {
            Err("the edit was rejected (see Messages)".into())
        }
    }

    // -- layers ----------------------------------------------------------------------------------

    pub(crate) fn pcb_layer_infos(&self) -> Vec<LayerInfo> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let hex = |c: Rgba| format!("#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]);
        let mut all = doc.stackup.all_layers();
        all.reverse();
        all.into_iter()
            .map(|l| LayerInfo {
                id: u32::from(l.0),
                name: layer::name(l),
                color: hex(ps::layer_color(l, self.theme_kind())),
                visible: !self.hidden_pcb_layers.contains(&l.0),
                active: self.active_pcb_layer == l.0,
                toggleable: true,
            })
            .collect()
    }

    pub(crate) fn pcb_info(&self) -> Option<PcbInfo> {
        let doc = self.pcb_doc()?;
        let mut warnings = pcb_ops::outline_problems(doc);
        if doc.graphics.values().all(|g| g.layer != layer::EDGE_CUTS) {
            warnings.push("There is no board outline yet (draw one on Edge.Cuts)".into());
        }
        Some(PcbInfo {
            copper_layers: u32::from(doc.stackup.copper_layers),
            active_layer: layer::name(layer::LayerId(self.active_pcb_layer)),
            footprints: doc.placed.len() as u32,
            nets: doc.nets.len() as u32,
            warnings,
            ratsnest_visible: self.pcb_ed.show_ratsnest,
            route_width_mm: mm_str(self.route_width_for(self.route_net())),
            route_width_auto: self.pcb_ed.route_width.is_none(),
            route_class: doc.rules.class_of(self.route_net()).name.clone(),
            route_mode: self.pcb_ed.route_mode.id().into(),
            via_size: {
                let (d, dr) = self.via_size_for(self.route_net());
                format!("{}/{}", mm_str(d), mm_str(dr))
            },
            via_size_auto: self.pcb_ed.via_size.is_none(),
            route: self.route_info(),
            unrouted: self.ratsnest(|e| e.count() as u32).unwrap_or(0),
        })
    }

    // -- commands --------------------------------------------------------------------------------

    pub(crate) fn pcb_editor_command(&mut self, name: &str) -> Result<(), String> {
        if let Some(rest) = name.strip_prefix("tool:") {
            let (id, arg) = match rest.split_once(':') {
                Some((id, arg)) => (id, Some(arg)),
                None => (rest, None),
            };
            return self.pcb_set_tool(id, arg);
        }
        match name {
            "select_all" => {
                let all: BTreeSet<Id> = self
                    .pcb_doc()
                    .map(|d| d.ids().filter(|i| !d.nets.contains_key(i)).collect())
                    .unwrap_or_default();
                self.selection.insert(Mode::Pcb, all);
            }
            "clear_selection" => {
                self.selection.insert(Mode::Pcb, BTreeSet::new());
            }
            "cancel" => self.pcb_cancel(),
            "rotate" => {
                if matches!(self.pcb_ed.tool, PcbTool::PlaceFootprint(_)) {
                    self.pcb_ed.pending_rotation = (self.pcb_ed.pending_rotation + 1) % 4;
                } else {
                    self.pcb_rotate()?;
                }
            }
            "flip_elbow" => self.pcb_ed.route_flip = !self.pcb_ed.route_flip,
            "route_via" => self.route_via()?,
            "route_finish" => {
                let mut out = Outcome::default();
                self.route_finish(&mut out);
            }
            "flip" => {
                if matches!(self.pcb_ed.tool, PcbTool::PlaceFootprint(_)) {
                    self.pcb_ed.pending_flipped = !self.pcb_ed.pending_flipped;
                } else {
                    self.pcb_flip()?;
                }
            }
            "delete" => self.pcb_delete()?,
            "duplicate" => self.pcb_duplicate()?,
            "lock" => self.pcb_lock(true)?,
            "unlock" => self.pcb_lock(false)?,
            "align_left" => self.pcb_align(Align::Left)?,
            "align_right" => self.pcb_align(Align::Right)?,
            "align_top" => self.pcb_align(Align::Top)?,
            "align_bottom" => self.pcb_align(Align::Bottom)?,
            "align_center_h" => self.pcb_align(Align::CenterH)?,
            "align_center_v" => self.pcb_align(Align::CenterV)?,
            "distribute_h" => self.pcb_distribute(Distribute::Horizontal)?,
            "distribute_v" => self.pcb_distribute(Distribute::Vertical)?,
            other => return Err(format!("“{other}” is not available in the PCB editor")),
        }
        Ok(())
    }

    fn pcb_set_tool(&mut self, id: &str, arg: Option<&str>) -> Result<(), String> {
        let tool = match id {
            "select" => PcbTool::Select,
            "outline_rect" => PcbTool::OutlineRect,
            "outline_line" => PcbTool::OutlineLine,
            "route" => PcbTool::Route,
            "via" => PcbTool::Via,
            "place_footprint" => {
                let f = arg
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .ok_or("choose a footprint to place")?;
                self.pcb_ed.pending_fp = Some(self.pcb_resolve_footprint(f)?);
                PcbTool::PlaceFootprint(f.to_owned())
            }
            other => return Err(format!("unknown PCB tool: {other}")),
        };
        if tool != PcbTool::Select && self.open.is_none() {
            return Err("no project is open".into());
        }
        self.pcb_ed.pts.clear();
        self.pcb_ed.drag = None;
        self.pcb_ed.pending_rotation = 0;
        self.pcb_ed.pending_flipped = false;
        self.pcb_restore_scene();
        self.pcb_ed.tool = tool;
        Ok(())
    }

    /// The footprint definition for `id`: the project's table, else a library.
    fn pcb_resolve_footprint(&self, id: &str) -> Result<Footprint, String> {
        self.pcb_doc()
            .and_then(|d| d.footprints.get(id).cloned())
            .or_else(|| self.find_library_footprint(id))
            .ok_or_else(|| format!("footprint {id} was not found in the project or any library"))
    }

    fn pcb_cancel(&mut self) {
        if self.route_cancel() {
            return;
        }
        if !self.pcb_ed.pts.is_empty() || self.pcb_ed.drag.is_some() {
            self.pcb_ed.pts.clear();
            self.pcb_ed.drag = None;
            self.pcb_restore_scene();
            self.gesture = Gesture::None;
        } else if self.pcb_ed.tool != PcbTool::Select {
            self.pcb_ed.tool = PcbTool::Select;
        } else {
            self.selection.insert(Mode::Pcb, BTreeSet::new());
        }
    }

    fn pcb_selection_pivot(&self) -> Point {
        let doc = match self.pcb_doc() {
            Some(d) => d,
            None => return Point::ORIGIN,
        };
        let boxes: Vec<(Point, Point)> = self
            .pcb_ids()
            .iter()
            .filter_map(|i| doc.object(*i).and_then(|o| pcb_ops::bounds(doc, &o)))
            .collect();
        if boxes.is_empty() {
            return Point::ORIGIN;
        }
        let g = self.grid.spacing.max(1);
        let snap = |v: i64| (v as f64 / g as f64).round() as i64 * g;
        let (lx, hx) = (
            boxes.iter().map(|b| b.0.x).min().unwrap_or(0),
            boxes.iter().map(|b| b.1.x).max().unwrap_or(0),
        );
        let (ly, hy) = (
            boxes.iter().map(|b| b.0.y).min().unwrap_or(0),
            boxes.iter().map(|b| b.1.y).max().unwrap_or(0),
        );
        Point::new(snap((lx + hx) / 2), snap((ly + hy) / 2))
    }

    /// Changes produced by `f` for every selected object, skipping those it declines (locked…).
    fn pcb_map_selection(&self, f: impl Fn(&PcbObject) -> Option<PcbObject>) -> Vec<PcbChange> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        self.pcb_ids()
            .into_iter()
            .filter_map(|id| {
                let o = doc.object(id)?;
                let n = f(&o)?;
                (n != o).then(|| PcbChange::modify(id, o, n))
            })
            .collect()
    }

    fn pcb_require_selection(&self) -> Result<(), String> {
        if self.pcb_ids().is_empty() {
            Err("select something first".into())
        } else {
            Ok(())
        }
    }

    fn pcb_rotate(&mut self) -> Result<(), String> {
        self.pcb_require_selection()?;
        let pivot = self.pcb_selection_pivot();
        let changes = self.pcb_map_selection(|o| pcb_ops::rotate(o, pivot));
        if changes.is_empty() {
            return Err("nothing can be rotated (locked footprints stay put)".into());
        }
        self.pcb_commit("Rotate", changes);
        Ok(())
    }

    fn pcb_flip(&mut self) -> Result<(), String> {
        self.pcb_require_selection()?;
        let changes = self.pcb_map_selection(pcb_ops::flip_footprint);
        if changes.is_empty() {
            return Err("only unlocked footprints can be flipped".into());
        }
        self.pcb_commit("Flip to the other side", changes);
        Ok(())
    }

    fn pcb_lock(&mut self, lock: bool) -> Result<(), String> {
        self.pcb_require_selection()?;
        let changes = self.pcb_map_selection(|o| match o {
            PcbObject::Footprint(f) => {
                let mut f = f.clone();
                f.locked = lock;
                Some(PcbObject::Footprint(f))
            }
            _ => None,
        });
        if changes.is_empty() {
            return Err("select footprints to lock or unlock".into());
        }
        self.pcb_commit(if lock { "Lock" } else { "Unlock" }, changes);
        Ok(())
    }

    fn pcb_delete(&mut self) -> Result<(), String> {
        self.pcb_require_selection()?;
        let Some(doc) = self.pcb_doc() else {
            return Ok(());
        };
        let mut skipped = 0;
        let changes: Vec<PcbChange> = self
            .pcb_ids()
            .into_iter()
            .filter_map(|id| {
                let o = doc.object(id)?;
                if matches!(&o, PcbObject::Footprint(f) if f.locked) {
                    skipped += 1;
                    return None;
                }
                Some(PcbChange::delete(id, o))
            })
            .collect();
        if changes.is_empty() {
            return Err("locked footprints cannot be deleted; unlock them first".into());
        }
        if skipped > 0 {
            self.log(
                MessageLevel::Warn,
                format!("{skipped} locked footprint(s) were not deleted"),
            );
        }
        self.pcb_commit("Delete", changes);
        self.selection.insert(Mode::Pcb, BTreeSet::new());
        Ok(())
    }

    fn pcb_duplicate(&mut self) -> Result<(), String> {
        self.pcb_require_selection()?;
        let ids = self.pcb_ids();
        let mut next = self.open.as_ref().map_or(1, |o| o.engine.peek_next_id());
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let off = Vector::new(self.grid.spacing.max(1) * 4, -self.grid.spacing.max(1) * 4);
        let changes = pcb_ops::duplicate(doc, &ids, off, || {
            let id = Id(next);
            next += 1;
            id
        });
        if changes.is_empty() {
            return Err("nothing to duplicate".into());
        }
        let created: BTreeSet<Id> = changes.iter().map(|c| c.id).collect();
        if let Some(open) = self.open.as_mut() {
            open.engine.reserve_ids_until(next);
        }
        if self.pcb_commit("Duplicate", changes) {
            self.selection.insert(Mode::Pcb, created);
        }
        Ok(())
    }

    fn pcb_align(&mut self, mode: Align) -> Result<(), String> {
        let ids = self.pcb_ids();
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let changes = pcb_ops::align(doc, &ids, mode).map_err(|e| e.to_string())?;
        self.pcb_commit("Align", changes);
        Ok(())
    }

    fn pcb_distribute(&mut self, mode: Distribute) -> Result<(), String> {
        let ids = self.pcb_ids();
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let changes = pcb_ops::distribute(doc, &ids, mode).map_err(|e| e.to_string())?;
        self.pcb_commit("Distribute", changes);
        Ok(())
    }

    /// `pcb_command` IPC: board-level commands. Names: `set_stackup` (arg: copper layer count),
    /// `active_layer` (arg: layer id).
    pub fn pcb_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        match name {
            "set_stackup" => {
                let n: u8 = arg
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .map_err(|_| "copper layers must be a number")?;
                let before = open.engine.project().pcb().stackup;
                open.engine
                    .execute(SetStackupCommand::new(before, Stackup { copper_layers: n }))
                    .map_err(|e| e.to_string())?;
                self.built_for = None;
                if !self
                    .pcb_doc()
                    .is_some_and(|d| d.stackup.has_copper(layer::LayerId(self.active_pcb_layer)))
                {
                    self.active_pcb_layer = 0;
                }
                Ok(())
            }
            other if self.pcb_route_command(other, arg)? => Ok(()),
            "toggle_ratsnest" => {
                self.pcb_ed.show_ratsnest = !self.pcb_ed.show_ratsnest;
                Ok(())
            }
            "active_layer" => {
                let id: u16 = arg
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .map_err(|_| "layer id must be a number")?;
                if !layer::is_known(layer::LayerId(id)) {
                    return Err("unknown layer".into());
                }
                self.active_pcb_layer = id;
                Ok(())
            }
            other => Err(format!("unknown PCB command: {other}")),
        }
    }

    /// Computes (without changing anything) what "Update PCB from Schematic" would do, and keeps
    /// the plan for [`Self::pcb_update_apply`].
    pub fn pcb_update_preview(&mut self) -> Result<UpdatePreview, String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let revision = open.engine.revision();
        let plan = buturecad_eda_core::sync::plan_update(open.engine.project(), &|name| {
            self.find_library_footprint(name)
        });
        let preview = UpdatePreview {
            lines: plan
                .lines
                .iter()
                .map(|l| PlanLineInfo {
                    kind: plan_kind_id(l.kind).into(),
                    text: l.text.clone(),
                })
                .collect(),
            can_apply: !plan.is_empty(),
        };
        self.pcb_plan = Some((revision, plan));
        Ok(preview)
    }

    /// Applies the previewed plan as one undo step. Refuses if the project changed since.
    pub fn pcb_update_apply(&mut self) -> Result<(), String> {
        let (revision, plan) = self.pcb_plan.take().ok_or("preview the update first")?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        if open.engine.revision() != revision {
            return Err("the project changed since the preview; preview the update again".into());
        }
        if plan.is_empty() {
            return Err("there is nothing to update".into());
        }
        plan.apply(&mut open.engine).map_err(|e| e.to_string())?;
        self.built_for = None;
        self.command_log.push("Update PCB from Schematic".into());
        self.log(
            MessageLevel::Info,
            format!(
                "Updated the PCB from the schematic: {} footprint(s) added, {} removed, {} net(s) added",
                plan.count(PlanKind::AddFootprint),
                plan.count(PlanKind::RemoveFootprint),
                plan.count(PlanKind::AddNet)
            ),
        );
        Ok(())
    }

    /// Footprints the placement tool can offer: the project's table plus library footprints.
    pub fn pcb_footprint_choices(&self) -> Vec<FootprintInfo> {
        self.library_footprints()
    }

    // -- pointer ---------------------------------------------------------------------------------

    pub(crate) fn pointer_pcb(&mut self, ev: PointerEvent, out: &mut Outcome) {
        if self.open.is_none() {
            return;
        }
        let at = self.cursor;
        match ev.kind {
            PointerKind::Move => {
                match self.pcb_ed.drag.clone() {
                    Some(Drag::Pending {
                        start_px,
                        start,
                        ids,
                    }) => {
                        if (ev.x - start_px.0).hypot(ev.y - start_px.1) >= DRAG_THRESHOLD_PX {
                            self.pcb_ed.drag = Some(Drag::Moving { start, ids });
                            self.pcb_preview_move(at);
                        }
                    }
                    Some(Drag::Moving { .. }) => self.pcb_preview_move(at),
                    Some(Drag::Vertex { .. }) => self.pcb_vertex_preview(at),
                    None => {
                        if !self.update_box(ev.x, ev.y) {
                            self.hover = self.pick_owner(ev.x, ev.y);
                        }
                    }
                }
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 0 => self.pcb_down(ev, at, out),
            PointerKind::Down if ev.button == 2 => {
                if !self.pcb_ed.pts.is_empty() {
                    self.pcb_ed.pts.clear();
                } else if self.pcb_ed.tool != PcbTool::Select {
                    self.pcb_ed.tool = PcbTool::Select;
                } else if let Some(owner) = self.pick_owner(ev.x, ev.y) {
                    self.select_owner(owner, false);
                    out.selection_changed = true;
                }
                out.redraw = true;
            }
            PointerKind::Up => {
                match self.pcb_ed.drag.take() {
                    Some(Drag::Moving { start, ids }) => {
                        let d = at - start;
                        if d.is_zero() {
                            self.pcb_restore_scene();
                        } else {
                            let changes = self.pcb_moved(&ids, d);
                            if self.pcb_commit("Move", changes) {
                                out.model_changed = true;
                            }
                        }
                    }
                    Some(v @ Drag::Vertex { .. }) => {
                        self.pcb_ed.drag = Some(v);
                        if self.pcb_vertex_commit(at) {
                            out.model_changed = true;
                        } else {
                            self.pcb_restore_scene();
                        }
                    }
                    Some(Drag::Pending { .. }) | None => {
                        if self.finish_box() {
                            out.selection_changed = true;
                        }
                    }
                }
                out.redraw = true;
            }
            PointerKind::DoubleClick => self.route_double_click(out),
            _ => {}
        }
    }

    fn pcb_moved(&self, ids: &[Id], d: Vector) -> Vec<PcbChange> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| {
                let o = doc.object(*id)?;
                let n = pcb_ops::translate(&o, d)?;
                Some(PcbChange::modify(*id, o, n))
            })
            .collect()
    }

    fn pcb_preview_move(&mut self, at: Point) {
        let Some(Drag::Moving { start, ids }) = self.pcb_ed.drag.clone() else {
            return;
        };
        let changes = self.pcb_moved(&ids, at - start);
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let doc = open.engine.project().pcb();
        for c in changes {
            self.pcb_scene.sync(doc, c.id, c.after.as_ref());
            self.pcb_ed.touched.insert(c.id);
        }
    }

    fn pcb_down(&mut self, ev: PointerEvent, at: Point, out: &mut Outcome) {
        match self.pcb_ed.tool.clone() {
            PcbTool::Select => {
                let additive = ev.shift || ev.ctrl;
                let raw = self
                    .views
                    .get(&Mode::Pcb)
                    .map_or(at, |v| v.screen_to_world(ev.x, ev.y));
                if !additive && self.pcb_try_vertex_drag(raw) {
                    out.redraw = true;
                    return;
                }
                match self.pick_owner(ev.x, ev.y) {
                    Some(owner) => {
                        self.select_owner(owner, additive);
                        out.selection_changed = true;
                        let ids = self.pcb_ids();
                        if !additive && ids.contains(&owner) {
                            self.pcb_ed.drag = Some(Drag::Pending {
                                start_px: (ev.x, ev.y),
                                start: at,
                                ids,
                            });
                        }
                    }
                    None => self.begin_box(ev.x, ev.y, additive),
                }
            }
            PcbTool::OutlineRect => self.pcb_outline_click(at, true, out),
            PcbTool::OutlineLine => self.pcb_outline_click(at, false, out),
            PcbTool::PlaceFootprint(id) => self.pcb_place(&id, at, out),
            PcbTool::Route => self.pcb_route_click(at, out),
            PcbTool::Via => self.pcb_place_via(at, out),
        }
        out.redraw = true;
    }

    fn pcb_outline_click(&mut self, at: Point, rect: bool, out: &mut Outcome) {
        if self.pcb_ed.pts.last() == Some(&at) {
            return;
        }
        self.pcb_ed.pts.push(at);
        let pts = self.pcb_ed.pts.clone();
        let line = |a: Point, b: Point| BoardGraphic {
            layer: layer::EDGE_CUTS,
            shape: FpShape::Line {
                segment: Segment::new(a, b),
                width: OUTLINE_WIDTH,
            },
        };
        let mut shapes: Vec<BoardGraphic> = Vec::new();
        if rect {
            if let [a, b] = pts.as_slice() {
                if Rectangle::new(*a, *b).is_err() {
                    self.log(
                        MessageLevel::Warn,
                        "A board outline rectangle needs two different corners",
                    );
                    self.pcb_ed.pts.pop();
                    return;
                }
                let c = [
                    Point::new(a.x, a.y),
                    Point::new(b.x, a.y),
                    Point::new(b.x, b.y),
                    Point::new(a.x, b.y),
                ];
                shapes = (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect();
                self.pcb_ed.pts.clear();
            }
        } else if let [.., a, b] = pts.as_slice() {
            shapes.push(line(*a, *b));
            // Clicking the first point again closes the loop and ends the outline.
            if pts.len() > 2 && *b == pts[0] {
                self.pcb_ed.pts.clear();
            }
        }
        if shapes.is_empty() {
            return;
        }
        let changes: Vec<PcbChange> = shapes
            .into_iter()
            .map(|g| {
                let id = self.pcb_new_id();
                PcbChange::create(id, PcbObject::Graphic(g))
            })
            .collect();
        if self.pcb_commit(
            if rect {
                "Draw board outline"
            } else {
                "Draw outline segment"
            },
            changes,
        ) {
            out.model_changed = true;
        }
    }

    fn pcb_place(&mut self, id: &str, at: Point, out: &mut Outcome) {
        let Ok(def) = self.pcb_resolve_footprint(id) else {
            return;
        };
        let embed = !self
            .pcb_doc()
            .is_some_and(|d| d.footprints.contains_key(id));
        let reference = def
            .texts
            .iter()
            .find(|t| t.kind == buturecad_pcb::TextKind::Reference)
            .map_or_else(|| "REF**".to_owned(), |t| t.text.clone());
        let placed = PlacedFootprint {
            footprint: id.to_owned(),
            reference,
            value: String::new(),
            placement: Placement {
                position: at,
                rotation: self.pcb_ed.pending_rotation,
                flipped: self.pcb_ed.pending_flipped,
            },
            locked: false,
            link: None,
            pad_nets: Default::default(),
        };
        let new_id = self.pcb_new_id();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let cmd = PcbEditCommand::new(
            "Place footprint",
            vec![PcbChange::create(new_id, PcbObject::Footprint(placed))],
        );
        let result = if embed {
            let name = id.to_owned();
            open.engine.transaction("Place footprint", |e| {
                e.execute(SetFootprintCommand::new(&name, None, Some(def)))?;
                e.execute(cmd)
            })
        } else {
            open.engine.execute(cmd)
        };
        self.built_for = None;
        match result {
            Ok(()) => {
                self.command_log.push("Place footprint".into());
                self.selection.insert(Mode::Pcb, BTreeSet::from([new_id]));
                out.model_changed = true;
                out.selection_changed = true;
            }
            Err(e) => self.log(MessageLevel::Error, format!("Place footprint failed: {e}")),
        }
    }

    // -- previews --------------------------------------------------------------------------------

    pub(crate) fn pcb_preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let colour = match self.theme {
            ThemeName::Dark => Theme::dark().selection,
            ThemeName::Light => Theme::light().selection,
        };
        let at = self.cursor;
        let mut out: Vec<(Shape, Rgba)> = self.ratsnest_shapes();
        out.extend(self.drc_marker_shapes(colour));
        out.extend(self.route_preview_shapes());
        match (&self.pcb_ed.tool, self.pcb_ed.pts.as_slice()) {
            (PcbTool::OutlineRect, [a]) => {
                if let Ok(rect) = Rectangle::new(*a, at) {
                    out.push((
                        Shape::Rect {
                            rect,
                            filled: false,
                            stroke: OUTLINE_WIDTH,
                        },
                        colour,
                    ));
                }
            }
            (PcbTool::OutlineLine, pts) if !pts.is_empty() => {
                if let Some(last) = pts.last() {
                    if *last != at {
                        out.push((
                            Shape::Line {
                                segment: Segment::new(*last, at),
                                width: OUTLINE_WIDTH,
                            },
                            colour,
                        ));
                    }
                }
            }
            (PcbTool::PlaceFootprint(id), _) => {
                if let Some(def) = &self.pcb_ed.pending_fp {
                    let mut ghost = PcbDoc {
                        stackup: doc.stackup,
                        ..PcbDoc::default()
                    };
                    ghost.footprints.insert(id.clone(), def.clone());
                    let f = PlacedFootprint {
                        footprint: id.clone(),
                        reference: String::new(),
                        value: String::new(),
                        placement: Placement {
                            position: at,
                            rotation: self.pcb_ed.pending_rotation,
                            flipped: self.pcb_ed.pending_flipped,
                        },
                        locked: false,
                        link: None,
                        pad_nets: Default::default(),
                    };
                    out.extend(
                        object_shapes(&ghost, &PcbObject::Footprint(f))
                            .into_iter()
                            .filter(|(l, _)| *l != ps::L_HIT && *l != ps::L_PADCENTER)
                            .map(|(_, s)| (s, colour)),
                    );
                }
            }
            _ => {}
        }
        out
    }

    /// The flying wires as overlay shapes (never pickable). Nets being previewed use their live
    /// lines instead of the cached ones.
    fn ratsnest_shapes(&self) -> Vec<(Shape, Rgba)> {
        if !self.pcb_ed.show_ratsnest {
            return Vec::new();
        }
        let colour = match self.theme {
            ThemeName::Dark => Rgba::rgb(240, 240, 120),
            ThemeName::Light => Rgba::rgb(150, 120, 0),
        };
        let line = |l: &RatLine| {
            (
                Shape::Line {
                    segment: Segment::new(l.a, l.b),
                    width: 0,
                },
                colour,
            )
        };
        let mut out: Vec<(Shape, Rgba)> = self
            .ratsnest(|e| {
                e.nets()
                    .filter(|n| !self.pcb_ed.rat_preview.contains_key(n))
                    .flat_map(|n| e.net_lines(n).iter().map(line).collect::<Vec<_>>())
                    .collect()
            })
            .unwrap_or_default();
        out.extend(self.pcb_ed.rat_preview.values().flatten().map(line));
        out
    }

    pub(crate) fn pcb_hint(&self) -> String {
        match (&self.pcb_ed.tool, self.pcb_ed.pts.len()) {
            (PcbTool::Select, _) => "Select: click or drag a box; drag to move. R rotates, F flips, Del deletes. Middle button pans.".into(),
            (PcbTool::OutlineRect, 0) => "Board outline: click the first corner.".into(),
            (PcbTool::OutlineRect, _) => "Board outline: click the opposite corner.".into(),
            (PcbTool::OutlineLine, 0) => "Board outline: click the start point.".into(),
            (PcbTool::OutlineLine, _) => "Board outline: click the next corner; click the first point to close. Esc ends.".into(),
            (PcbTool::PlaceFootprint(_), _) => "Place footprint: click to place. R rotates, F flips to the back, Esc stops.".into(),
            (PcbTool::Route, _) => self.route_hint(),
            (PcbTool::Via, _) => "Via: click to place a through via (it takes the net of the pad or track under it). Esc stops.".into(),
        }
    }
}

fn plan_kind_id(k: PlanKind) -> &'static str {
    match k {
        PlanKind::AddFootprint => "add_footprint",
        PlanKind::RemoveFootprint => "remove_footprint",
        PlanKind::ChangeFootprint => "change_footprint",
        PlanKind::Link => "link",
        PlanKind::UpdateField => "update_field",
        PlanKind::AddNet => "add_net",
        PlanKind::RemoveNet => "remove_net",
        PlanKind::RenameNet => "rename_net",
        PlanKind::PadNet => "pad_net",
        PlanKind::Warning => "warning",
    }
}
