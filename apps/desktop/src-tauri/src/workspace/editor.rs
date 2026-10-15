//! Schematic editing: tools, interactions and editor commands.
//!
//! Division of labour: the pure operations live in `buturecad_schematic::edit` (they produce
//! reversible change sets); this module is the interaction layer that turns pointer events and
//! commands into those operations, shows live previews, and executes the result through the
//! Command Engine so every edit is undoable, dirty-tracked and autosaved.

use super::*;
use crate::scene_builder::{self as sb};
use buturecad_eda_core::commands::{AddSymbolsCommand, SchematicEditCommand};
use buturecad_geometry::{Segment, Vector};
use buturecad_renderer::{Rgba, Shape};
use buturecad_schematic::edit::{self, Clipboard, EditError, IdGen, Op};
use buturecad_schematic::{
    builtin, Change, Direction, GlobalLabel, NetLabel, Object, Placement,
    Schematic as SchematicModel, Symbol,
};

/// The active tool. Tools that need an argument carry it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    #[default]
    Select,
    PlaceComponent(String),
    PlacePower(String),
    Wire,
    Bus,
    Junction,
    NetLabel(String),
    GlobalLabel(String),
    Move,
    Drag,
    Rotate,
    Mirror,
    Delete,
    Duplicate,
}

impl Tool {
    pub fn id(&self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::PlaceComponent(_) => "place_component",
            Tool::PlacePower(_) => "power",
            Tool::Wire => "wire",
            Tool::Bus => "bus",
            Tool::Junction => "junction",
            Tool::NetLabel(_) => "net_label",
            Tool::GlobalLabel(_) => "global_label",
            Tool::Move => "move",
            Tool::Drag => "drag",
            Tool::Rotate => "rotate",
            Tool::Mirror => "mirror",
            Tool::Delete => "delete",
            Tool::Duplicate => "duplicate",
        }
    }

    pub fn arg(&self) -> Option<String> {
        match self {
            Tool::PlaceComponent(a)
            | Tool::PlacePower(a)
            | Tool::NetLabel(a)
            | Tool::GlobalLabel(a) => Some(a.clone()),
            _ => None,
        }
    }

    pub fn parse(id: &str, arg: Option<&str>) -> Result<Tool, String> {
        let need = |what: &str| -> Result<String, String> {
            match arg.map(str::trim) {
                Some(a) if !a.is_empty() => Ok(a.to_owned()),
                _ => Err(format!("the {id} tool needs {what}")),
            }
        };
        Ok(match id {
            "select" => Tool::Select,
            "place_component" => Tool::PlaceComponent(need("a symbol")?),
            "power" => Tool::PlacePower(need("a power symbol")?),
            "wire" => Tool::Wire,
            "bus" => Tool::Bus,
            "junction" => Tool::Junction,
            "net_label" => Tool::NetLabel(need("a label name")?),
            "global_label" => Tool::GlobalLabel(need("a label name")?),
            "move" => Tool::Move,
            "drag" => Tool::Drag,
            "rotate" => Tool::Rotate,
            "mirror" => Tool::Mirror,
            "delete" => Tool::Delete,
            "duplicate" => Tool::Duplicate,
            other => return Err(format!("unknown tool: {other}")),
        })
    }
}

/// A gesture in progress.
#[derive(Debug, Clone, Default)]
enum Interaction {
    #[default]
    Idle,
    /// Button pressed on an object; becomes `Moving` once the pointer travels far enough.
    PendingMove {
        start_px: (f64, f64),
        start: Point,
        ids: BTreeSet<Id>,
        stretch: bool,
    },
    /// Objects being carried. `click_mode`: started by the keyboard, committed by a click.
    Moving {
        start: Point,
        ids: BTreeSet<Id>,
        stretch: bool,
        delta: Vector,
        click_mode: bool,
    },
    /// A wire or bus being drawn: committed vertices so far.
    Path { points: Vec<Point> },
    /// A clipboard waiting to be dropped.
    Pasting,
}

#[derive(Debug, Default)]
pub(crate) struct EditorState {
    pub tool: Tool,
    interaction: Interaction,
    /// Rotation/mirror applied to the object about to be placed.
    pending_rotation: u8,
    pending_mirror: bool,
    /// Unit of a multi-unit symbol about to be placed (1-based).
    pending_unit: u8,
    /// The symbol of the active placement tool, resolved once when the tool is chosen (so the
    /// per-frame preview does not read library files).
    pending_symbol: Option<Symbol>,
    /// Footprint and manufacturer data of the library component being placed, with the symbol it
    /// belongs to (set by `library_place_symbol`, dropped when another tool is chosen).
    pub(crate) pending_part: Option<(String, edit::PartData)>,
    pub clipboard: Option<Clipboard>,
    /// Elbow orientation for new wires: horizontal leg first.
    horizontal_first: bool,
    /// Objects whose scene shapes currently differ from the model (live preview).
    touched: BTreeSet<Id>,
}

/// Distance a button-down must travel before it counts as a drag.
const DRAG_START_PX: f64 = 4.0;

fn direction_for(rotation: u8) -> Direction {
    [
        Direction::Right,
        Direction::Up,
        Direction::Left,
        Direction::Down,
    ][(rotation % 4) as usize]
}

impl Workspace {
    // -- model access ----------------------------------------------------------------------------

    fn sch(&self) -> Option<&SchematicModel> {
        self.open.as_ref().map(|o| o.engine.project().schematic())
    }

    fn sch_ids(&self) -> BTreeSet<Id> {
        self.selection
            .get(&Mode::Schematic)
            .cloned()
            .unwrap_or_default()
    }

    fn new_gen(&self) -> IdGen {
        IdGen::new(self.open.as_ref().map_or(1, |o| o.engine.peek_next_id()))
    }

    fn root(&self) -> Id {
        self.sch().map_or(Id(0), SchematicModel::root)
    }

    fn symbol_by_id(&self, lib_id: &str) -> Option<Symbol> {
        self.sch()
            .and_then(|s| s.symbol(lib_id).cloned())
            .or_else(|| {
                self.editor
                    .pending_symbol
                    .clone()
                    .filter(|s| s.name == lib_id)
            })
            .or_else(|| self.find_library_symbol(lib_id))
    }

    /// Runs `change` through the Command Engine as one undoable step. Symbols the schematic lacks
    /// are embedded in the same step.
    fn commit(
        &mut self,
        label: &str,
        changes: Vec<Change>,
        symbols: Vec<Symbol>,
        gen: IdGen,
    ) -> bool {
        if changes.is_empty() {
            return false;
        }
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let missing: Vec<Symbol> = symbols
            .into_iter()
            .filter(|s| open.engine.project().schematic().symbol(&s.name).is_none())
            .collect();
        open.engine.reserve_ids_until(gen.next);
        let revision_before = open.engine.revision();
        let applied = changes.clone();
        let cmd = SchematicEditCommand::new(label, changes);
        let result = if missing.is_empty() {
            open.engine.execute(cmd)
        } else {
            open.engine.transaction(label, |e| {
                e.execute(AddSymbolsCommand::new(missing))?;
                e.execute(cmd)
            })
        };
        self.built_for = None; // the model changed: rebuild the scene (also drops any preview)
        self.editor.touched.clear();
        match result {
            Ok(()) => {
                self.connectivity_committed(revision_before, &applied);
                self.command_log.push(label.to_owned());
                true
            }
            Err(e) => {
                self.log(MessageLevel::Error, format!("{label} failed: {e}"));
                false
            }
        }
    }

    fn report(&mut self, label: &str, e: EditError) {
        self.log(MessageLevel::Error, format!("{label}: {e}"));
    }

    // -- previews ------------------------------------------------------------------------------

    /// Puts the scene back in sync with the model after a live preview.
    fn restore_scene(&mut self) {
        let touched = std::mem::take(&mut self.editor.touched);
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let sch = open.engine.project().schematic();
        for id in touched {
            self.sch_scene.sync(sch, id, sch.object(id).map(|(_, o)| o));
        }
    }

    /// Shows `changes` in the scene without touching the model.
    fn show_changes(&mut self, changes: &[Change]) {
        self.restore_scene();
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let sch = open.engine.project().schematic();
        for c in changes {
            self.sch_scene.sync(sch, c.id, c.after.as_ref());
            self.editor.touched.insert(c.id);
        }
    }

    fn preview_move(&mut self, ids: &BTreeSet<Id>, delta: Vector, stretch: bool) {
        let mut gen = self.new_gen();
        let changes = match self.sch() {
            Some(sch) => {
                edit::transform_selection(sch, ids, Op::Translate(delta), stretch, &mut gen)
            }
            None => return,
        };
        if let Ok(changes) = changes {
            self.show_changes(&changes);
        }
    }

    /// Transient shapes drawn above the scene: placement ghosts, the wire being drawn, pasted
    /// objects.
    pub(crate) fn preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        if self.mode == Mode::SymbolEditor {
            return self.symed_preview_shapes();
        }
        if self.mode == Mode::FootprintEditor {
            return self.fped_preview_shapes();
        }
        if self.mode == Mode::Pcb {
            return self.pcb_preview_shapes();
        }
        if self.mode != Mode::Schematic || self.open.is_none() {
            return Vec::new();
        }
        let colour = match self.theme {
            ThemeName::Dark => Theme::dark().selection,
            ThemeName::Light => Theme::light().selection,
        };
        let Some(sch) = self.sch() else {
            return Vec::new();
        };
        let at = self.cursor;
        let placement = Placement {
            position: at,
            rotation: self.editor.pending_rotation,
            mirror: self.editor.pending_mirror,
        };
        let mut out: Vec<(Shape, Rgba)> = Vec::new();
        out.extend(self.erc_marker_shapes(colour));
        let mut push = |shapes: Vec<(buturecad_renderer::LayerId, Shape)>| {
            out.extend(
                shapes
                    .into_iter()
                    .filter(|(l, _)| *l != sb::L_HIT)
                    .map(|(_, s)| (s, colour)),
            )
        };
        match (&self.editor.tool, &self.editor.interaction) {
            (_, Interaction::Path { points }) => {
                let bus = self.editor.tool == Tool::Bus;
                let mut path = points.clone();
                if let Some(last) = path.last().copied() {
                    path.pop();
                    path.extend(edit::elbow(last, at, self.editor.horizontal_first));
                }
                let width = if bus { 300_000 } else { 150_000 };
                for w in path.windows(2) {
                    out.push((
                        Shape::Line {
                            segment: Segment::new(w[0], w[1]),
                            width,
                        },
                        colour,
                    ));
                }
            }
            (_, Interaction::Pasting) => {
                if let Some(clip) = &self.editor.clipboard {
                    let delta = at - clip.anchor;
                    for o in &clip.objects {
                        let moved = edit::transform_object(o, Op::Translate(delta));
                        let sym = match &moved {
                            Object::Component(c) => {
                                clip.symbols.iter().find(|s| s.name == c.lib_id)
                            }
                            Object::PowerSymbol(p) => {
                                clip.symbols.iter().find(|s| s.name == p.lib_id)
                            }
                            _ => None,
                        };
                        match (&moved, sym) {
                            (Object::Component(c), Some(s)) => {
                                push(sb::symbol_shapes(s, &c.placement, c.unit, c.representation))
                            }
                            (Object::PowerSymbol(p), Some(s)) => {
                                push(sb::symbol_shapes(s, &p.placement, 1, 1))
                            }
                            _ => push(sb::object_shapes(sch, &moved)),
                        }
                    }
                }
            }
            (Tool::PlaceComponent(lib), Interaction::Idle)
            | (Tool::PlacePower(lib), Interaction::Idle) => {
                if let Some(sym) = self.symbol_by_id(lib) {
                    push(sb::symbol_shapes(
                        &sym,
                        &placement,
                        self.editor.pending_unit.clamp(1, sym.unit_count.max(1)),
                        1,
                    ));
                }
            }
            (Tool::NetLabel(name), Interaction::Idle) => {
                let obj = Object::NetLabel(NetLabel {
                    position: at,
                    name: name.clone(),
                    orientation: direction_for(self.editor.pending_rotation),
                });
                push(sb::object_shapes(sch, &obj));
            }
            (Tool::GlobalLabel(name), Interaction::Idle) => {
                let obj = Object::GlobalLabel(GlobalLabel {
                    position: at,
                    name: name.clone(),
                    orientation: direction_for(self.editor.pending_rotation),
                });
                push(sb::object_shapes(sch, &obj));
            }
            (Tool::Wire | Tool::Bus, Interaction::Idle) => {
                // A small marker where the wire would start.
                out.push((
                    Shape::Circle {
                        circle: buturecad_geometry::Circle {
                            center: at,
                            radius: 300_000,
                        },
                        filled: true,
                        stroke: 0,
                    },
                    colour,
                ));
            }
            _ => {}
        }
        out
    }

    pub(crate) fn tool_hint(&self) -> String {
        match (&self.editor.tool, &self.editor.interaction) {
            (_, Interaction::Moving { click_mode: true, .. }) => "Moving: click to drop, Esc to cancel.".into(),
            (_, Interaction::Moving { .. }) => "Moving: release to drop, Esc to cancel.".into(),
            (_, Interaction::Path { .. }) => "Wire: click to add corners, click a pin/wire end or double-click to finish, Space flips the elbow, Esc cancels.".into(),
            (_, Interaction::Pasting) => "Paste: click to drop, R rotates, Esc cancels.".into(),
            (Tool::Select, _) => "Select: click or drag a box; drag objects to move them (wires follow). Middle button or Space+drag pans.".into(),
            (Tool::PlaceComponent(_) | Tool::PlacePower(_), _) => "Place: click to place, R rotates, X/Y mirror, Esc to stop.".into(),
            (Tool::Wire, _) => "Wire: click to start.".into(),
            (Tool::Bus, _) => "Bus: click to start.".into(),
            (Tool::Junction, _) => "Junction: click to place.".into(),
            (Tool::NetLabel(_) | Tool::GlobalLabel(_), _) => "Label: click to place (a trailing number auto-increments), R rotates.".into(),
            (Tool::Move, _) => "Move: press on an object and drag.".into(),
            (Tool::Drag, _) => "Drag: grab the object or wire segment under the cursor; connected wires follow.".into(),
            (Tool::Rotate, _) => "Rotate: click an object.".into(),
            (Tool::Mirror, _) => "Mirror: click an object (X mirrors left-right, Y top-bottom).".into(),
            (Tool::Delete, _) => "Delete: click an object.".into(),
            (Tool::Duplicate, _) => "Duplicate: click an object.".into(),
        }
    }

    // -- commands --------------------------------------------------------------------------------

    /// Runs a named editor command. Unknown names are an error, never silently ignored.
    ///
    /// Names: `select_all`, `clear_selection`, `cancel`, `rotate`, `mirror_x`, `mirror_y`,
    /// `delete`, `duplicate`, `copy`, `cut`, `paste`, `start_move`, `flip_elbow`, and
    /// `tool:<id>[:<argument>]`.
    pub fn editor_command(&mut self, name: &str) -> Result<(), String> {
        if self.mode == Mode::SymbolEditor {
            return self.symed_editor_command(name);
        }
        if self.mode == Mode::FootprintEditor {
            return self.fped_editor_command(name);
        }
        if self.mode == Mode::Pcb {
            return self.pcb_editor_command(name);
        }
        if let Some(rest) = name.strip_prefix("tool:") {
            let (id, arg) = match rest.split_once(':') {
                Some((id, arg)) => (id, Some(arg)),
                None => (rest, None),
            };
            return self.set_tool(Tool::parse(id, arg)?);
        }
        match name {
            "select_all" => self.select_all(),
            "clear_selection" => self.clear_selection(),
            "cancel" => {
                self.erc_clear_marker();
                self.cancel()
            }
            "flip_elbow" => self.editor.horizontal_first = !self.editor.horizontal_first,
            "rotate" => self.rotate_selection_or_pending(),
            "mirror_x" => self.mirror_selection_or_pending(true),
            "mirror_y" => self.mirror_selection_or_pending(false),
            "delete" => self.delete_selection()?,
            "duplicate" => self.duplicate_selection()?,
            "copy" => self.copy_selection(false)?,
            "cut" => self.copy_selection(true)?,
            "paste" => self.begin_paste()?,
            "start_move" => self.start_move_from_keyboard()?,
            "unit_next" => self.cycle_pending_unit(1),
            "unit_prev" => self.cycle_pending_unit(-1),
            other => return Err(format!("unknown editor command: {other}")),
        }
        Ok(())
    }

    fn require_schematic(&self) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no project is open".into());
        }
        if self.mode != Mode::Schematic {
            return Err("switch to the Schematic view first".into());
        }
        Ok(())
    }

    pub(crate) fn set_tool(&mut self, tool: Tool) -> Result<(), String> {
        if tool != Tool::Select {
            self.require_schematic()?;
        }
        // Part data only survives for the placement tool of its own symbol.
        if !matches!(&tool, Tool::PlaceComponent(lib) if self.editor.pending_part.as_ref().is_some_and(|(s, _)| s == lib))
        {
            self.editor.pending_part = None;
        }
        if let Tool::PlaceComponent(lib) | Tool::PlacePower(lib) = &tool {
            let sym = self
                .symbol_by_id(lib)
                .ok_or_else(|| format!("unknown symbol {lib}"))?;
            self.editor.pending_symbol = Some(sym.clone());
            if matches!(tool, Tool::PlacePower(_)) != sym.is_power {
                return Err(format!(
                    "{lib} is {}a power symbol",
                    if sym.is_power { "" } else { "not " }
                ));
            }
        }
        if let Tool::NetLabel(n) | Tool::GlobalLabel(n) = &tool {
            if n.trim().is_empty() {
                return Err("a label needs a name".into());
            }
        }
        self.abort_interaction();
        self.editor.pending_rotation = 0;
        self.editor.pending_mirror = false;
        self.editor.pending_unit = 1;
        // Tools that act on objects apply immediately when something is already selected.
        let acts_on_selection = matches!(
            tool,
            Tool::Rotate | Tool::Mirror | Tool::Delete | Tool::Duplicate
        );
        if acts_on_selection && !self.sch_ids().is_empty() {
            self.editor.tool = Tool::Select;
            match tool {
                Tool::Rotate => self.rotate_selection_or_pending(),
                Tool::Mirror => self.mirror_selection_or_pending(true),
                Tool::Delete => self.delete_selection()?,
                _ => self.duplicate_selection()?,
            }
            return Ok(());
        }
        self.editor.tool = tool;
        Ok(())
    }

    /// Cycles the unit of the multi-unit symbol being placed.
    fn cycle_pending_unit(&mut self, step: i32) {
        let Tool::PlaceComponent(lib) = &self.editor.tool else {
            return;
        };
        let Some(sym) = self.symbol_by_id(lib) else {
            return;
        };
        let n = i32::from(sym.unit_count.max(1));
        let cur = i32::from(self.editor.pending_unit.max(1)) - 1;
        self.editor.pending_unit = ((cur + step).rem_euclid(n) + 1) as u8;
    }

    fn abort_interaction(&mut self) {
        self.editor.interaction = Interaction::Idle;
        self.gesture = Gesture::None;
        self.restore_scene();
    }

    fn cancel(&mut self) {
        if !matches!(self.editor.interaction, Interaction::Idle)
            || !matches!(self.gesture, Gesture::None)
        {
            self.abort_interaction();
        } else if self.editor.tool != Tool::Select {
            self.editor.tool = Tool::Select;
        } else {
            self.clear_selection();
        }
    }

    fn rotate_selection_or_pending(&mut self) {
        let placing = matches!(
            self.editor.tool,
            Tool::PlaceComponent(_)
                | Tool::PlacePower(_)
                | Tool::NetLabel(_)
                | Tool::GlobalLabel(_)
        );
        if placing || matches!(self.editor.interaction, Interaction::Pasting) {
            self.editor.pending_rotation = (self.editor.pending_rotation + 1) % 4;
            return;
        }
        let ids = self.sch_ids();
        self.transform_ids(&ids, "Rotate", |sch, ids, grid, gen| {
            edit::rotate_objects(sch, ids, grid, gen)
        });
    }

    fn mirror_selection_or_pending(&mut self, horizontal: bool) {
        if matches!(
            self.editor.tool,
            Tool::PlaceComponent(_) | Tool::PlacePower(_)
        ) {
            if horizontal {
                self.editor.pending_mirror = !self.editor.pending_mirror;
            } else {
                // Top-bottom flip = mirror + half turn.
                self.editor.pending_mirror = !self.editor.pending_mirror;
                self.editor.pending_rotation = (self.editor.pending_rotation + 2) % 4;
            }
            return;
        }
        let ids = self.sch_ids();
        let label = if horizontal {
            "Mirror left-right"
        } else {
            "Mirror top-bottom"
        };
        self.transform_ids(&ids, label, move |sch, ids, grid, gen| {
            edit::mirror_objects(sch, ids, horizontal, grid, gen)
        });
    }

    fn transform_ids(
        &mut self,
        ids: &BTreeSet<Id>,
        label: &str,
        f: impl FnOnce(
            &SchematicModel,
            &BTreeSet<Id>,
            i64,
            &mut IdGen,
        ) -> Result<edit::Edited, EditError>,
    ) {
        if ids.is_empty() {
            self.log(MessageLevel::Warn, format!("{label}: nothing is selected"));
            return;
        }
        let mut gen = self.new_gen();
        let grid = self.grid.spacing;
        let Some(sch) = self.sch() else { return };
        match f(sch, ids, grid, &mut gen) {
            Ok(edited) => {
                let preserved = edited.connectivity_preserved;
                if self.commit(label, edited.changes, Vec::new(), gen) && !preserved {
                    self.warn_connectivity(label);
                }
            }
            Err(e) => self.report(label, e),
        }
    }

    fn warn_connectivity(&mut self, label: &str) {
        self.log(
            MessageLevel::Warn,
            format!("{label}: the re-routed wires could not keep every net intact (nets were merged or split). Undo (Ctrl+Z) and place the part elsewhere, or fix the wiring by hand."),
        );
    }

    fn delete_selection(&mut self) -> Result<(), String> {
        self.require_schematic()?;
        let ids = self.sch_ids();
        self.delete_ids(&ids);
        Ok(())
    }

    fn delete_ids(&mut self, ids: &BTreeSet<Id>) {
        if ids.is_empty() {
            return;
        }
        let Some(sch) = self.sch() else { return };
        match edit::delete_objects(sch, ids) {
            Ok(changes) => {
                let gen = self.new_gen();
                if self.commit(
                    if ids.len() == 1 {
                        "Delete object"
                    } else {
                        "Delete objects"
                    },
                    changes,
                    Vec::new(),
                    gen,
                ) {
                    self.selection
                        .entry(Mode::Schematic)
                        .or_default()
                        .retain(|i| !ids.contains(i));
                }
            }
            Err(e) => self.report("Delete", e),
        }
    }

    fn duplicate_selection(&mut self) -> Result<(), String> {
        self.require_schematic()?;
        let ids = self.sch_ids();
        self.duplicate_ids(&ids);
        Ok(())
    }

    fn duplicate_ids(&mut self, ids: &BTreeSet<Id>) {
        if ids.is_empty() {
            return;
        }
        let step = self.grid.spacing * 2;
        let mut gen = self.new_gen();
        let Some(sch) = self.sch() else { return };
        match edit::duplicate_objects(sch, ids, Vector::new(step, -step), &mut gen) {
            Ok((changes, new_ids)) => {
                if self.commit("Duplicate", changes, Vec::new(), gen) {
                    self.selection
                        .insert(Mode::Schematic, new_ids.into_iter().collect());
                }
            }
            Err(e) => self.report("Duplicate", e),
        }
    }

    fn copy_selection(&mut self, cut: bool) -> Result<(), String> {
        self.require_schematic()?;
        let ids = self.sch_ids();
        if ids.is_empty() {
            return Err("nothing is selected".into());
        }
        let Some(sch) = self.sch() else { return Ok(()) };
        match edit::copy_objects(sch, &ids, self.grid.spacing) {
            Ok(clip) => {
                let n = clip.objects.len();
                self.editor.clipboard = Some(clip);
                self.log(MessageLevel::Info, format!("Copied {n} object(s)"));
            }
            Err(e) => return Err(e.to_string()),
        }
        if cut {
            self.delete_ids(&ids);
        }
        Ok(())
    }

    fn begin_paste(&mut self) -> Result<(), String> {
        self.require_schematic()?;
        if self.editor.clipboard.is_none() {
            return Err("the clipboard is empty".into());
        }
        self.abort_interaction();
        self.editor.interaction = Interaction::Pasting;
        Ok(())
    }

    fn start_move_from_keyboard(&mut self) -> Result<(), String> {
        self.require_schematic()?;
        let ids = self.sch_ids();
        if ids.is_empty() {
            return Err("select objects first, then press M".into());
        }
        let Some(view) = self.views.get(&Mode::Schematic).copied() else {
            return Ok(());
        };
        self.abort_interaction();
        let start = view.screen_to_world(self.last_pointer.0, self.last_pointer.1);
        self.editor.interaction = Interaction::Moving {
            start,
            ids,
            stretch: true,
            delta: Vector::new(0, 0),
            click_mode: true,
        };
        Ok(())
    }

    /// Edits one property of an object (Inspector).
    pub fn set_property(&mut self, id: u64, key: &str, value: &str) -> Result<(), String> {
        if self.mode == Mode::SymbolEditor {
            return self.symed_set_item_property(id, key, value);
        }
        if self.mode == Mode::FootprintEditor {
            return self.fped_set_item_property(id, key, value);
        }
        if self.mode == Mode::Pcb {
            return self.pcb_set_property(id, key, value);
        }
        self.require_schematic()?;
        let sch = self.sch().ok_or("no project is open")?;
        let change = edit::set_property(sch, Id(id), key, value).map_err(|e| e.to_string())?;
        let gen = self.new_gen();
        if self.commit(&format!("Edit {key}"), vec![change], Vec::new(), gen) {
            Ok(())
        } else {
            Err("the edit was rejected (see Messages)".into())
        }
    }

    /// Symbols the placement tools and the Symbol Editor offer: the built-in set, symbols embedded
    /// in the project, and everything in the library folders. A project copy wins over a library
    /// entry with the same id (that is the definition placed parts actually use).
    pub fn library_symbols(&self) -> Vec<SymbolInfo> {
        let mut all: BTreeMap<String, (Symbol, String)> = BTreeMap::new();
        for s in builtin::symbols() {
            all.insert(s.name.clone(), (s, "builtin".into()));
        }
        for s in buturecad_libraries::standard::symbols() {
            all.insert(s.name.clone(), (s, "builtin".into()));
        }
        for (scope, dir) in self.library_dirs() {
            for l in dir.list_symbol_libraries() {
                if let Ok(lib) = dir.load_symbol_library(&l.name) {
                    for name in lib.names() {
                        if let Some(s) = lib.instantiate(name) {
                            all.insert(s.name.clone(), (s, format!("{scope}-library")));
                        }
                    }
                }
            }
        }
        if let Some(sch) = self.sch() {
            for (k, v) in sch.symbols() {
                all.insert(k.clone(), (v.clone(), "project".into()));
            }
        }
        all.into_values()
            .map(|(s, source)| SymbolInfo {
                lib_id: s.name.clone(),
                name: s.name.rsplit(':').next().unwrap_or(&s.name).to_owned(),
                library: s.name.split_once(':').map_or("", |(l, _)| l).to_owned(),
                source,
                description: s.meta.description.clone(),
                unit_count: u32::from(s.unit_count),
                has_alternate: s.has_alternate(),
                reference_prefix: s.reference_prefix.clone(),
                is_power: s.is_power,
                pin_count: s.pins.len() as u32,
            })
            .collect()
    }

    // -- pointer ---------------------------------------------------------------------------------

    pub(crate) fn pointer_schematic(&mut self, ev: PointerEvent, out: &mut Outcome) {
        if self.open.is_none() {
            return;
        }
        let Some(view) = self.views.get(&Mode::Schematic).copied() else {
            return;
        };
        let raw = view.screen_to_world(ev.x, ev.y);
        let snapped = self.cursor;
        let grid = self.grid.spacing;

        // Interactions in progress take precedence over the tool.
        match self.editor.interaction.clone() {
            Interaction::PendingMove {
                start_px,
                start,
                ids,
                stretch,
            } => match ev.kind {
                PointerKind::Move => {
                    if (ev.x - start_px.0).hypot(ev.y - start_px.1) >= DRAG_START_PX {
                        self.editor.interaction = Interaction::Moving {
                            start,
                            ids: ids.clone(),
                            stretch,
                            delta: Vector::new(0, 0),
                            click_mode: false,
                        };
                        self.update_move(raw, out);
                    }
                    return;
                }
                PointerKind::Up => {
                    self.editor.interaction = Interaction::Idle;
                    out.redraw = true;
                    return;
                }
                _ => {}
            },
            Interaction::Moving { click_mode, .. } => match ev.kind {
                PointerKind::Move => {
                    self.update_move(raw, out);
                    return;
                }
                PointerKind::Up if !click_mode => {
                    self.finish_move(out);
                    return;
                }
                PointerKind::Down if click_mode && ev.button == 0 => {
                    self.finish_move(out);
                    return;
                }
                PointerKind::Down if ev.button == 2 => {
                    self.abort_interaction();
                    out.redraw = true;
                    return;
                }
                _ => return,
            },
            Interaction::Pasting => match ev.kind {
                PointerKind::Move => {
                    out.redraw = true;
                    return;
                }
                PointerKind::Down if ev.button == 0 => {
                    self.drop_paste(snapped, out);
                    return;
                }
                PointerKind::Down if ev.button == 2 => {
                    self.abort_interaction();
                    out.redraw = true;
                    return;
                }
                _ => return,
            },
            Interaction::Path { points } => {
                self.path_event(ev, points, snapped, out);
                return;
            }
            Interaction::Idle => {}
        }

        match self.editor.tool.clone() {
            Tool::Select | Tool::Move | Tool::Drag => self.pick_tool_event(ev, raw, out),
            Tool::Rotate | Tool::Mirror | Tool::Delete | Tool::Duplicate => {
                self.click_action_event(ev, out)
            }
            Tool::PlaceComponent(lib) => self.place_event(ev, &lib, false, snapped, out),
            Tool::PlacePower(lib) => self.place_event(ev, &lib, true, snapped, out),
            Tool::Wire | Tool::Bus => match ev.kind {
                PointerKind::Move => out.redraw = true,
                PointerKind::Down if ev.button == 0 => {
                    self.editor.interaction = Interaction::Path {
                        points: vec![snapped],
                    };
                    out.redraw = true;
                }
                PointerKind::Down if ev.button == 2 => {
                    self.editor.tool = Tool::Select;
                    out.redraw = true;
                }
                _ => {}
            },
            Tool::Junction => match ev.kind {
                PointerKind::Move => out.redraw = true,
                PointerKind::Down if ev.button == 0 => {
                    let mut gen = self.new_gen();
                    let root = self.root();
                    let changes = self
                        .sch()
                        .map(|s| edit::add_junction(s, root, snapped, &mut gen))
                        .unwrap_or_default();
                    out.model_changed |= self.commit("Add junction", changes, Vec::new(), gen);
                    out.redraw = true;
                }
                PointerKind::Down if ev.button == 2 => self.editor.tool = Tool::Select,
                _ => {}
            },
            Tool::NetLabel(name) => self.label_event(ev, &name, false, snapped, out),
            Tool::GlobalLabel(name) => self.label_event(ev, &name, true, snapped, out),
        }
        let _ = grid;
    }

    fn update_move(&mut self, raw: Point, out: &mut Outcome) {
        let Interaction::Moving {
            start,
            ids,
            stretch,
            delta,
            click_mode,
        } = self.editor.interaction.clone()
        else {
            return;
        };
        let g = self.grid.spacing.max(1);
        let snap = |v: i64| (v + g / 2).div_euclid(g) * g;
        let d = raw - start;
        let new_delta = Vector::new(snap(d.x), snap(d.y));
        if new_delta != delta || self.editor.touched.is_empty() {
            self.preview_move(&ids, new_delta, stretch);
        }
        self.editor.interaction = Interaction::Moving {
            start,
            ids,
            stretch,
            delta: new_delta,
            click_mode,
        };
        out.redraw = true;
    }

    fn finish_move(&mut self, out: &mut Outcome) {
        let Interaction::Moving {
            ids,
            stretch,
            delta,
            ..
        } = std::mem::take(&mut self.editor.interaction)
        else {
            return;
        };
        self.restore_scene();
        out.redraw = true;
        if delta == Vector::new(0, 0) {
            return;
        }
        let mut gen = self.new_gen();
        let Some(sch) = self.sch() else { return };
        match edit::move_objects(sch, &ids, delta, stretch, &mut gen) {
            Ok(edited) => {
                let preserved = edited.connectivity_preserved;
                if self.commit("Move", edited.changes, Vec::new(), gen) {
                    out.model_changed = true;
                    if !preserved {
                        self.warn_connectivity("Move");
                    }
                }
            }
            Err(e) => self.report("Move", e),
        }
    }

    fn drop_paste(&mut self, at: Point, out: &mut Outcome) {
        let Some(clip) = self.editor.clipboard.clone() else {
            return;
        };
        self.editor.interaction = Interaction::Idle;
        let root = self.root();
        let mut gen = self.new_gen();
        let Some(sch) = self.sch() else { return };
        // Symbols are embedded first so the pasted parts validate.
        let mut with_symbols = sch.clone();
        for s in &clip.symbols {
            if with_symbols.symbol(&s.name).is_none() {
                with_symbols.add_symbol(s.clone());
            }
        }
        match edit::paste_objects(&with_symbols, &clip, root, at, &mut gen) {
            Ok((changes, ids)) => {
                if self.commit("Paste", changes, clip.symbols.clone(), gen) {
                    self.selection
                        .insert(Mode::Schematic, ids.into_iter().collect());
                    out.model_changed = true;
                    out.selection_changed = true;
                }
            }
            Err(e) => self.report("Paste", e),
        }
        out.redraw = true;
    }

    /// Select / Move / Drag: press on an object, or start a box on empty space.
    fn pick_tool_event(&mut self, ev: PointerEvent, raw: Point, out: &mut Outcome) {
        match ev.kind {
            PointerKind::Move => {
                if self.update_box(ev.x, ev.y) {
                    out.redraw = true;
                } else {
                    self.hover = self.pick_owner(ev.x, ev.y);
                    out.redraw = true;
                }
            }
            PointerKind::Down if ev.button == 0 || ev.button == 2 => {
                let hit = self.pick_owner(ev.x, ev.y);
                let additive = ev.shift || ev.ctrl;
                let Some(owner) = hit else {
                    if ev.button == 0 {
                        self.begin_box(ev.x, ev.y, additive);
                    }
                    return;
                };
                if self.editor.tool == Tool::Drag {
                    // Drag grabs exactly the object under the cursor (a wire segment, a part…).
                    self.selection
                        .insert(Mode::Schematic, [owner].into_iter().collect());
                } else {
                    self.select_owner(owner, additive && ev.button == 0);
                }
                out.selection_changed = true;
                out.redraw = true;
                if ev.button == 0 && !additive {
                    let ids = self.sch_ids();
                    if self.editor.tool == Tool::Drag {
                        let drag_ids: BTreeSet<Id> = [owner].into_iter().collect();
                        self.editor.interaction = Interaction::PendingMove {
                            start_px: (ev.x, ev.y),
                            start: raw,
                            ids: drag_ids,
                            stretch: !ev.alt,
                        };
                    } else if !ids.is_empty() {
                        self.editor.interaction = Interaction::PendingMove {
                            start_px: (ev.x, ev.y),
                            start: raw,
                            ids,
                            stretch: !ev.alt,
                        };
                    }
                }
            }
            PointerKind::Up => {
                if self.finish_box() {
                    out.selection_changed = true;
                }
                out.redraw = true;
            }
            _ => {}
        }
    }

    /// Rotate / Mirror / Delete / Duplicate tools: click an object.
    fn click_action_event(&mut self, ev: PointerEvent, out: &mut Outcome) {
        match ev.kind {
            PointerKind::Move => {
                self.hover = self.pick_owner(ev.x, ev.y);
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 0 => {
                let Some(owner) = self.pick_owner(ev.x, ev.y) else {
                    return;
                };
                let ids: BTreeSet<Id> = [owner].into_iter().collect();
                self.selection.insert(Mode::Schematic, ids.clone());
                out.selection_changed = true;
                match self.editor.tool.clone() {
                    Tool::Rotate => self.transform_ids(&ids, "Rotate", |s, i, g, gen| {
                        edit::rotate_objects(s, i, g, gen)
                    }),
                    Tool::Mirror => {
                        self.transform_ids(&ids, "Mirror left-right", |s, i, g, gen| {
                            edit::mirror_objects(s, i, true, g, gen)
                        })
                    }
                    Tool::Delete => self.delete_ids(&ids),
                    _ => self.duplicate_ids(&ids),
                }
                out.model_changed = true;
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 2 => {
                self.editor.tool = Tool::Select;
                out.redraw = true;
            }
            _ => {}
        }
    }

    fn place_event(
        &mut self,
        ev: PointerEvent,
        lib: &str,
        power: bool,
        at: Point,
        out: &mut Outcome,
    ) {
        match ev.kind {
            PointerKind::Move => out.redraw = true,
            PointerKind::Down if ev.button == 0 => {
                let Some(sym) = self.symbol_by_id(lib) else {
                    return;
                };
                let placement = Placement {
                    position: at,
                    rotation: self.editor.pending_rotation,
                    mirror: self.editor.pending_mirror,
                };
                let root = self.root();
                let mut gen = self.new_gen();
                let id = gen.alloc();
                let Some(sch) = self.sch() else { return };
                let object = if power {
                    Object::PowerSymbol(edit::new_power_symbol(&sym, placement))
                } else {
                    // The reference must account for the symbol even if it is not embedded yet.
                    let mut c = edit::new_component_unit(
                        sch,
                        &sym,
                        placement,
                        self.editor.pending_unit.max(1),
                    );
                    if let Some((s, data)) = &self.editor.pending_part {
                        if *s == sym.name && c.unit == 1 {
                            data.apply(&mut c);
                        }
                    }
                    Object::Component(c)
                };
                let label = if power {
                    "Place power symbol"
                } else {
                    "Place component"
                };
                if self.commit(
                    label,
                    vec![Change::create(root, id, object)],
                    vec![sym],
                    gen,
                ) {
                    out.model_changed = true;
                }
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 2 => {
                self.editor.tool = Tool::Select;
                out.redraw = true;
            }
            _ => {}
        }
    }

    fn label_event(
        &mut self,
        ev: PointerEvent,
        name: &str,
        global: bool,
        at: Point,
        out: &mut Outcome,
    ) {
        match ev.kind {
            PointerKind::Move => out.redraw = true,
            PointerKind::Down if ev.button == 0 => {
                let root = self.root();
                let mut gen = self.new_gen();
                let id = gen.alloc();
                let orientation = direction_for(self.editor.pending_rotation);
                let object = if global {
                    Object::GlobalLabel(GlobalLabel {
                        position: at,
                        name: name.to_owned(),
                        orientation,
                    })
                } else {
                    Object::NetLabel(NetLabel {
                        position: at,
                        name: name.to_owned(),
                        orientation,
                    })
                };
                if self.commit(
                    "Add label",
                    vec![Change::create(root, id, object)],
                    Vec::new(),
                    gen,
                ) {
                    out.model_changed = true;
                    // Repeated placement counts up: D3 → D4.
                    let next = edit::next_label_name(name);
                    self.editor.tool = if global {
                        Tool::GlobalLabel(next)
                    } else {
                        Tool::NetLabel(next)
                    };
                }
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 2 => {
                self.editor.tool = Tool::Select;
                out.redraw = true;
            }
            _ => {}
        }
    }

    /// Events while a wire/bus is being drawn.
    fn path_event(
        &mut self,
        ev: PointerEvent,
        mut points: Vec<Point>,
        snapped: Point,
        out: &mut Outcome,
    ) {
        let bus = self.editor.tool == Tool::Bus;
        match ev.kind {
            PointerKind::Move => out.redraw = true,
            PointerKind::Down if ev.button == 0 => {
                let Some(last) = points.last().copied() else {
                    return;
                };
                if snapped == last {
                    return;
                }
                let corner = edit::elbow(last, snapped, self.editor.horizontal_first);
                points.extend(corner.into_iter().skip(1));
                // Ending on a connection point (pin, wire end, junction…) finishes the wire.
                let finish = self.snap_on_object && points.len() >= 2;
                self.editor.interaction = Interaction::Path {
                    points: points.clone(),
                };
                if finish {
                    self.finish_path(points, bus, out);
                }
                out.redraw = true;
            }
            PointerKind::DoubleClick => {
                self.finish_path(points, bus, out);
                out.redraw = true;
            }
            PointerKind::Down if ev.button == 2 => {
                // Right click: finish what exists, otherwise cancel.
                if points.len() >= 2 {
                    self.finish_path(points, bus, out);
                } else {
                    self.editor.interaction = Interaction::Idle;
                }
                out.redraw = true;
            }
            _ => {}
        }
    }

    fn finish_path(&mut self, points: Vec<Point>, bus: bool, out: &mut Outcome) {
        self.editor.interaction = Interaction::Idle;
        if points.len() < 2 {
            return;
        }
        let root = self.root();
        let mut gen = self.new_gen();
        let Some(sch) = self.sch() else { return };
        match edit::draw_path(sch, root, &points, bus, &mut gen) {
            Ok(changes) => {
                if self.commit(
                    if bus { "Draw bus" } else { "Draw wire" },
                    changes,
                    Vec::new(),
                    gen,
                ) {
                    out.model_changed = true;
                }
            }
            Err(e) => self.report("Draw wire", e),
        }
    }
}
