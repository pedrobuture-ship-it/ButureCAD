//! Application state: the open project, its editors and the renderer. Tauri-independent so it can
//! be tested headlessly; `commands.rs` is a thin IPC layer over it.

use crate::scene_builder::{self as sb, SchematicScene, ThemeKind};
use crate::ui_types::*;
use buturecad_eda_core::commands::RenameProjectCommand;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::{BoundingBox, Point};
use buturecad_io::{engine as io_engine, ProjectSession};
use buturecad_renderer::{
    box_select, pick, BoxMode, FrameRequest, GpuRenderer, GridSettings, ObjectId, Rgba, Scene,
    Selection, SnapSettings, Theme, View,
};
use buturecad_schematic::{Connectivity, Id, Object};

mod editor;
use editor::EditorState;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const DEFAULT_GRID_NM: i64 = 1_270_000; // 50 mil
/// 1 mm at 96 dpi is 3.7795 px; used to express zoom as a percentage.
const PX_PER_NM_AT_100: f64 = 3.7795e-6;
const MAX_MESSAGES: usize = 500;
const PICK_TOLERANCE_PX: f64 = 5.0;
const DRAG_THRESHOLD_PX: f64 = 4.0;

pub struct OpenProject {
    pub session: ProjectSession,
    pub engine: CommandEngine,
    pub root: PathBuf,
}

/// What a pointer event changed.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Outcome {
    pub redraw: bool,
    pub selection_changed: bool,
    pub model_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    None,
    Pan {
        last: (f64, f64),
    },
    Box {
        start: (f64, f64),
        end: (f64, f64),
        additive: bool,
    },
}

pub struct Workspace {
    pub open: Option<OpenProject>,
    recovery: Option<Project>,
    pub mode: Mode,
    pub units: Units,
    pub theme: ThemeName,
    pub grid: GridSettings,
    pub snap: SnapSettings,
    pub debug: bool,
    pub(crate) editor: EditorState,

    sch_scene: SchematicScene,
    pcb_scene: sb::PcbScene,
    built_for: Option<(u64, ThemeName, BTreeSet<u16>)>,
    pub views: BTreeMap<Mode, View>,
    fitted: BTreeSet<Mode>,
    pub selection: BTreeMap<Mode, BTreeSet<Id>>,
    hover: Option<Id>,
    gesture: Gesture,
    pub hidden_pcb_layers: BTreeSet<u16>,
    pub active_pcb_layer: u16,
    canvas: (f64, f64),
    cursor: Point,
    snap_label: &'static str,
    /// The last snap landed on an object's connection point (pin, wire end, junction…).
    pub(crate) snap_on_object: bool,
    /// Screen position of the last pointer event.
    pub(crate) last_pointer: (f64, f64),

    gpu: Option<GpuRenderer>,
    gpu_error: Option<String>,
    messages: Vec<Message>,
    seq: u64,
    command_log: Vec<String>,
    recent: Vec<RecentProject>,
    recent_path: Option<PathBuf>,
}

impl Workspace {
    pub fn new(recent_path: Option<PathBuf>) -> Self {
        let mut ws = Self {
            open: None,
            recovery: None,
            mode: Mode::Schematic,
            units: Units::Mm,
            theme: ThemeName::Dark,
            grid: GridSettings::new(DEFAULT_GRID_NM),
            snap: SnapSettings::default(),
            debug: false,
            editor: EditorState::default(),
            sch_scene: SchematicScene::default(),
            pcb_scene: sb::PcbScene::default(),
            built_for: None,
            views: BTreeMap::new(),
            fitted: BTreeSet::new(),
            selection: BTreeMap::new(),
            hover: None,
            gesture: Gesture::None,
            hidden_pcb_layers: BTreeSet::new(),
            active_pcb_layer: 0,
            canvas: (800.0, 600.0),
            cursor: Point::ORIGIN,
            snap_label: "none",
            snap_on_object: false,
            last_pointer: (0.0, 0.0),
            gpu: None,
            gpu_error: None,
            messages: Vec::new(),
            seq: 0,
            command_log: Vec::new(),
            recent: Vec::new(),
            recent_path,
        };
        ws.load_recent();
        ws
    }

    /// Number of remembered recent projects (used by tests and the start screen).
    pub fn snapshot_recent_len(&self) -> usize {
        self.recent.len()
    }

    // -- messages ------------------------------------------------------------------------------

    pub fn log(&mut self, level: MessageLevel, text: impl Into<String>) {
        self.seq += 1;
        self.messages.push(Message {
            seq: self.seq,
            level,
            text: text.into(),
        });
        if self.messages.len() > MAX_MESSAGES {
            self.messages.remove(0);
        }
    }

    // -- recent projects -----------------------------------------------------------------------

    fn load_recent(&mut self) {
        let Some(path) = &self.recent_path else {
            return;
        };
        if let Ok(bytes) = std::fs::read(path) {
            // A damaged recent list is not worth failing for; start empty.
            self.recent = serde_json::from_slice(&bytes).unwrap_or_default();
        }
    }

    fn remember(&mut self, name: &str, path: &Path) {
        let path_s = path.to_string_lossy().into_owned();
        self.recent.retain(|r| r.path != path_s);
        self.recent.insert(
            0,
            RecentProject {
                name: name.to_owned(),
                path: path_s,
            },
        );
        self.recent.truncate(10);
        if let Some(file) = &self.recent_path {
            if let Ok(bytes) = serde_json::to_vec_pretty(&self.recent) {
                if let Some(dir) = file.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = buturecad_io::atomic::atomic_write(file, &bytes);
            }
        }
    }

    // -- project lifecycle ---------------------------------------------------------------------

    fn install(&mut self, session: ProjectSession, engine: CommandEngine, root: PathBuf) {
        self.remember(engine.project().name(), &root);
        self.open = Some(OpenProject {
            session,
            engine,
            root,
        });
        self.built_for = None;
        self.fitted.clear();
        self.selection.clear();
        self.hover = None;
        self.gesture = Gesture::None;
        self.command_log.clear();
        self.editor = EditorState::default();
    }

    pub fn new_project(&mut self, root: &Path, name: &str) -> Result<(), String> {
        self.close_project()?;
        let (session, project) = ProjectSession::create(root, name).map_err(|e| e.to_string())?;
        self.install(session, CommandEngine::new(project), root.to_path_buf());
        self.log(
            MessageLevel::Info,
            format!("Created project “{name}” in {}", root.display()),
        );
        Ok(())
    }

    pub fn open_project(&mut self, root: &Path) -> Result<(), String> {
        self.close_project()?;
        let opened = ProjectSession::open(root).map_err(|e| e.to_string())?;
        let name = opened.project.name().to_owned();
        if let Some(rec) = opened.recovery {
            match rec.project {
                Ok(p) => {
                    self.recovery = Some(p);
                    self.log(
                        MessageLevel::Warn,
                        "The previous session did not close cleanly; an autosave is available.",
                    );
                }
                Err(e) => {
                    self.log(MessageLevel::Warn, format!("The previous session crashed but its autosave is unreadable ({e}); using the last saved version."));
                    let _ = opened.session.discard_autosave();
                }
            }
        }
        self.install(
            opened.session,
            CommandEngine::new(opened.project),
            root.to_path_buf(),
        );
        self.log(MessageLevel::Info, format!("Opened project “{name}”"));
        Ok(())
    }

    /// Resolves the pending crash-recovery offer.
    pub fn resolve_recovery(&mut self, accept: bool) -> Result<(), String> {
        let Some(project) = self.recovery.take() else {
            return Ok(());
        };
        let Some(open) = self.open.as_mut() else {
            return Ok(());
        };
        if accept {
            open.engine = CommandEngine::recovered(project);
            self.built_for = None;
            self.log(
                MessageLevel::Info,
                "Recovered unsaved changes from the autosave. Save to keep them.",
            );
        } else {
            open.session.discard_autosave().map_err(|e| e.to_string())?;
            self.log(MessageLevel::Info, "Discarded the autosave.");
        }
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        io_engine::save(&open.session, &mut open.engine).map_err(|e| e.to_string())?;
        let name = open.engine.project().name().to_owned();
        self.log(MessageLevel::Info, format!("Saved “{name}”"));
        Ok(())
    }

    pub fn save_as(&mut self, new_root: &Path) -> Result<(), String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        if open.engine.transaction_depth() > 0 {
            return Err("cannot save while an operation is in progress".into());
        }
        open.session
            .save_as(new_root, open.engine.project())
            .map_err(|e| e.to_string())?;
        open.root = new_root.to_path_buf();
        open.engine.mark_saved().map_err(|e| e.to_string())?;
        let name = open.engine.project().name().to_owned();
        self.remember(&name, new_root);
        self.log(
            MessageLevel::Info,
            format!("Saved a copy to {}", new_root.display()),
        );
        Ok(())
    }

    /// Closes the project. Refuses when there are unsaved changes unless `force`.
    pub fn close_project(&mut self) -> Result<(), String> {
        self.close_project_force(false)
    }

    pub fn close_project_force(&mut self, force: bool) -> Result<(), String> {
        if let Some(open) = &self.open {
            if open.engine.is_dirty() && !force {
                return Err("the project has unsaved changes".into());
            }
        }
        self.open = None;
        self.recovery = None;
        self.built_for = None;
        self.selection.clear();
        Ok(())
    }

    pub fn rename_project(&mut self, name: &str) -> Result<(), String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        open.engine
            .execute(RenameProjectCommand::new(name))
            .map_err(|e| e.to_string())?;
        self.command_log.push("Rename project".into());
        Ok(())
    }

    /// Writes an autosave when there are unsaved, not yet autosaved changes.
    pub fn autosave_tick(&mut self) -> Result<bool, String> {
        let Some(open) = self.open.as_mut() else {
            return Ok(false);
        };
        io_engine::autosave_if_pending(&open.session, &mut open.engine).map_err(|e| e.to_string())
    }

    // -- undo / redo -----------------------------------------------------------------------------

    pub fn undo(&mut self) -> Result<Option<String>, String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        let label = open.engine.undo().map_err(|e| e.to_string())?;
        if let Some(l) = &label {
            self.command_log.push(format!("Undo: {l}"));
            self.prune_selection();
        }
        Ok(label)
    }

    pub fn redo(&mut self) -> Result<Option<String>, String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        let label = open.engine.redo().map_err(|e| e.to_string())?;
        if let Some(l) = &label {
            self.command_log.push(format!("Redo: {l}"));
            self.prune_selection();
        }
        Ok(label)
    }

    fn prune_selection(&mut self) {
        let Some(open) = &self.open else { return };
        let project = open.engine.project();
        if let Some(sel) = self.selection.get_mut(&Mode::Schematic) {
            sel.retain(|id| project.schematic().object(*id).is_some());
        }
        if let Some(sel) = self.selection.get_mut(&Mode::Pcb) {
            sel.retain(|id| {
                project.pcb().tracks.contains_key(id) || project.pcb().vias.contains_key(id)
            });
        }
        if self.hover.is_some_and(|h| !self.owner_exists(h)) {
            self.hover = None;
        }
    }

    fn owner_exists(&self, id: Id) -> bool {
        let Some(open) = &self.open else { return false };
        let p = open.engine.project();
        p.schematic().object(id).is_some()
            || p.pcb().tracks.contains_key(&id)
            || p.pcb().vias.contains_key(&id)
    }

    // -- view settings ---------------------------------------------------------------------------

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.gesture = Gesture::None;
        self.hover = None;
    }

    pub fn set_theme(&mut self, theme: ThemeName) {
        self.theme = theme;
        self.built_for = None;
    }

    pub fn set_canvas_size(&mut self, w: f64, h: f64) {
        self.canvas = (w.max(1.0), h.max(1.0));
        for v in self.views.values_mut() {
            v.resize(self.canvas.0, self.canvas.1);
        }
    }

    pub fn set_pcb_layer_visible(&mut self, layer: u16, visible: bool) {
        if visible {
            self.hidden_pcb_layers.remove(&layer);
        } else {
            self.hidden_pcb_layers.insert(layer);
        }
        self.built_for = None;
    }

    fn theme_kind(&self) -> ThemeKind {
        match self.theme {
            ThemeName::Dark => ThemeKind::Dark,
            ThemeName::Light => ThemeKind::Light,
        }
    }

    // -- scenes ----------------------------------------------------------------------------------

    /// Rebuilds scenes when the model, theme or layer visibility changed.
    pub(crate) fn ensure_scene(&mut self) {
        let Some(open) = &self.open else { return };
        let key = (
            open.engine.revision(),
            self.theme,
            self.hidden_pcb_layers.clone(),
        );
        if self.built_for.as_ref() == Some(&key) {
            return;
        }
        let theme = self.theme_kind();
        let project = open.engine.project();
        self.sch_scene.rebuild(project.schematic(), theme);
        self.pcb_scene
            .rebuild(project.pcb(), theme, &self.hidden_pcb_layers);
        self.built_for = Some(key);
    }

    pub(crate) fn scene(&self) -> Option<&Scene> {
        match self.mode {
            Mode::Schematic => Some(&self.sch_scene.scene),
            Mode::Pcb => Some(&self.pcb_scene.scene),
            _ => None,
        }
    }

    /// Shape ids belonging to `owners` in the active scene.
    fn shapes_for(&self, owners: impl IntoIterator<Item = Id>) -> Vec<ObjectId> {
        match self.mode {
            Mode::Schematic => owners
                .into_iter()
                .flat_map(|o| self.sch_scene.shapes_of(o).to_vec())
                .collect(),
            Mode::Pcb => {
                let Some(scene) = self.scene() else {
                    return Vec::new();
                };
                owners
                    .into_iter()
                    .flat_map(|o| (0..2).map(move |n| sb::shape_id(o, n)))
                    .filter(|s| scene.get(*s).is_some())
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn view_mut(&mut self) -> &mut View {
        let canvas = self.canvas;
        self.views.entry(self.mode).or_insert_with(|| {
            let mut v = View::new(canvas.0, canvas.1);
            v.scale = 12.0 / DEFAULT_GRID_NM as f64;
            v
        })
    }

    /// Fits the view to the content (or resets it for an empty scene).
    pub fn fit_view(&mut self) {
        self.ensure_scene();
        let bounds: Option<BoundingBox> = self.scene().and_then(|s| s.bounds());
        let v = self.view_mut();
        match bounds {
            Some(b) => v.fit(&b, 40.0),
            None => {
                v.center = (0.0, 0.0);
                v.scale = 12.0 / DEFAULT_GRID_NM as f64;
            }
        }
        self.fitted.insert(self.mode);
    }

    pub fn zoom_by(&mut self, factor: f64) {
        let (w, h) = self.canvas;
        self.view_mut().zoom_at(w / 2.0, h / 2.0, factor);
    }

    // -- pointer input ---------------------------------------------------------------------------

    pub fn pointer(&mut self, ev: PointerEvent) -> PointerResult {
        self.ensure_scene();
        if self.scene().is_some() && !self.fitted.contains(&self.mode) {
            self.fit_view();
        }
        // Cursor position & snap.
        if let Some(scene) = self.scene() {
            let view = self
                .views
                .get(&self.mode)
                .copied()
                .unwrap_or_else(|| View::new(self.canvas.0, self.canvas.1));
            let s =
                buturecad_renderer::snap(scene, &view, &self.snap, self.grid.spacing, ev.x, ev.y);
            self.cursor = s.point;
            self.snap_on_object = matches!(s.kind, buturecad_renderer::SnapKind::Object(_));
            self.last_pointer = (ev.x, ev.y);
            self.snap_label = match s.kind {
                buturecad_renderer::SnapKind::None => "none",
                buturecad_renderer::SnapKind::Grid => "grid",
                buturecad_renderer::SnapKind::Object(_) => "object",
            };
        }

        let mut out = Outcome::default();
        if !self.pointer_common(ev, &mut out) {
            if self.mode == Mode::Schematic {
                self.pointer_schematic(ev, &mut out);
            } else {
                self.pointer_select(ev, &mut out);
            }
        }
        let Outcome {
            redraw,
            selection_changed,
            model_changed,
        } = out;

        PointerResult {
            redraw,
            cursor_x_nm: self.cursor.x,
            cursor_y_nm: self.cursor.y,
            snap: self.snap_label.into(),
            hover: self.hover.and_then(|h| self.describe(h)),
            zoom_percent: self.zoom_percent(),
            selection_changed,
            model_changed,
            hint: self.hint(),
        }
    }

    /// Events every mode handles the same way: zoom, pan, leaving the canvas. Returns whether the
    /// event was consumed.
    fn pointer_common(&mut self, ev: PointerEvent, out: &mut Outcome) -> bool {
        match ev.kind {
            PointerKind::Wheel => {
                let factor = (-ev.delta * 0.0015).exp();
                self.view_mut().zoom_at(ev.x, ev.y, factor);
                out.redraw = true;
                true
            }
            PointerKind::Leave => {
                if self.hover.take().is_some() {
                    out.redraw = true;
                }
                true
            }
            PointerKind::Down if ev.button == 1 => {
                self.gesture = Gesture::Pan { last: (ev.x, ev.y) };
                true
            }
            PointerKind::Move => match self.gesture {
                Gesture::Pan { last } => {
                    self.view_mut().pan_pixels(ev.x - last.0, ev.y - last.1);
                    self.gesture = Gesture::Pan { last: (ev.x, ev.y) };
                    out.redraw = true;
                    true
                }
                _ => false,
            },
            PointerKind::Up if matches!(self.gesture, Gesture::Pan { .. }) => {
                self.gesture = Gesture::None;
                out.redraw = true;
                true
            }
            _ => false,
        }
    }

    /// Selection-only pointer behaviour (PCB mode, and the base of the schematic Select tool).
    fn pointer_select(&mut self, ev: PointerEvent, out: &mut Outcome) {
        match ev.kind {
            PointerKind::Move => match self.gesture {
                Gesture::Box {
                    start, additive, ..
                } => {
                    self.gesture = Gesture::Box {
                        start,
                        end: (ev.x, ev.y),
                        additive,
                    };
                    out.redraw = true;
                }
                _ => {
                    self.hover = self.pick_owner(ev.x, ev.y);
                    out.redraw = true;
                }
            },
            PointerKind::Down if ev.button == 0 || ev.button == 2 => {
                let hit = self.pick_owner(ev.x, ev.y);
                let additive = ev.shift || ev.ctrl;
                match hit {
                    Some(owner) => {
                        self.select_owner(owner, additive && ev.button == 0);
                        out.selection_changed = true;
                        out.redraw = true;
                    }
                    None if ev.button == 0 => self.begin_box(ev.x, ev.y, additive),
                    None => {}
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

    /// Click-selection rules: toggle when additive, otherwise select only (unless already selected,
    /// so a drag can start on any member of a multi-selection).
    pub(crate) fn select_owner(&mut self, owner: Id, additive: bool) {
        let sel = self.selection.entry(self.mode).or_default();
        if additive {
            if !sel.remove(&owner) {
                sel.insert(owner);
            }
        } else if !sel.contains(&owner) {
            sel.clear();
            sel.insert(owner);
        }
    }

    pub(crate) fn begin_box(&mut self, x: f64, y: f64, additive: bool) {
        self.gesture = Gesture::Box {
            start: (x, y),
            end: (x, y),
            additive,
        };
    }

    /// Ends a rubber-band selection; returns whether the selection may have changed.
    pub(crate) fn finish_box(&mut self) -> bool {
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Box {
                start,
                end,
                additive,
            } => {
                let moved = (end.0 - start.0).abs() > DRAG_THRESHOLD_PX
                    || (end.1 - start.1).abs() > DRAG_THRESHOLD_PX;
                if moved {
                    self.apply_box_selection(start, end, additive);
                } else if !additive {
                    self.selection.entry(self.mode).or_default().clear();
                }
                true
            }
            other => {
                self.gesture = other;
                false
            }
        }
    }

    pub(crate) fn update_box(&mut self, x: f64, y: f64) -> bool {
        if let Gesture::Box {
            start, additive, ..
        } = self.gesture
        {
            self.gesture = Gesture::Box {
                start,
                end: (x, y),
                additive,
            };
            true
        } else {
            false
        }
    }

    fn hint(&self) -> String {
        match self.mode {
            Mode::Schematic => self.tool_hint(),
            Mode::Pcb => {
                "Select: click or drag a box. Middle button or Space+drag pans, wheel zooms.".into()
            }
            _ => String::new(),
        }
    }

    pub fn zoom_percent(&self) -> f64 {
        self.views
            .get(&self.mode)
            .map_or(100.0, |v| v.scale / PX_PER_NM_AT_100 * 100.0)
    }

    fn pick_owner(&self, x: f64, y: f64) -> Option<Id> {
        let scene = self.scene()?;
        let view = self.views.get(&self.mode)?;
        pick(scene, view, x, y, PICK_TOLERANCE_PX).map(sb::owner_of)
    }

    fn owner_bounds(&self, owner: Id) -> Option<BoundingBox> {
        let scene = self.scene()?;
        let mut boxes = self
            .shapes_for([owner])
            .into_iter()
            .filter_map(|s| scene.bounding_box_of(s));
        let first = boxes.next()?;
        Some(boxes.fold(first, |a, b| a.union(&b)))
    }

    /// Window: objects entirely inside the box. Crossing: objects touching it. Both work on whole
    /// objects, not on their individual shapes.
    fn apply_box_selection(&mut self, start: (f64, f64), end: (f64, f64), additive: bool) {
        let Some(view) = self.views.get(&self.mode).copied() else {
            return;
        };
        let area = BoundingBox::new(
            view.screen_to_world(start.0, start.1),
            view.screen_to_world(end.0, end.1),
        );
        let mode = BoxMode::from_drag(start.0, end.0);
        let Some(scene) = self.scene() else { return };
        let touching: BTreeSet<Id> = box_select(scene, &area, BoxMode::Crossing)
            .into_iter()
            .map(sb::owner_of)
            .collect();
        let hit: BTreeSet<Id> = match mode {
            BoxMode::Crossing => touching,
            BoxMode::Window => touching
                .into_iter()
                .filter(|o| self.owner_bounds(*o).is_some_and(|b| area.contains_box(&b)))
                .collect(),
        };
        let sel = self.selection.entry(self.mode).or_default();
        if !additive {
            sel.clear();
        }
        sel.extend(hit);
    }

    pub fn select_all(&mut self) {
        self.ensure_scene();
        let Some(open) = &self.open else { return };
        let project = open.engine.project();
        let all: BTreeSet<Id> = match self.mode {
            Mode::Schematic => project
                .schematic()
                .sheet(project.schematic().root())
                .map(|s| s.objects.keys().copied().collect())
                .unwrap_or_default(),
            Mode::Pcb => project
                .pcb()
                .tracks
                .keys()
                .chain(project.pcb().vias.keys())
                .copied()
                .collect(),
            _ => BTreeSet::new(),
        };
        self.selection.insert(self.mode, all);
    }

    pub fn clear_selection(&mut self) {
        self.selection.entry(self.mode).or_default().clear();
    }

    // -- descriptions & summaries --------------------------------------------------------------

    pub(crate) fn describe(&self, id: Id) -> Option<String> {
        let open = self.open.as_ref()?;
        let p = open.engine.project();
        match self.mode {
            Mode::Schematic => {
                let (_, obj) = p.schematic().object(id)?;
                Some(match obj {
                    Object::Component(c) => format!(
                        "{} ({})",
                        c.reference,
                        if c.value.is_empty() {
                            &c.lib_id
                        } else {
                            &c.value
                        }
                    ),
                    Object::PowerSymbol(ps) => format!("Power {}", ps.net_name),
                    Object::NetLabel(l) => format!("Label {}", l.name),
                    Object::GlobalLabel(l) => format!("Global label {}", l.name),
                    Object::HierLabel(l) => format!("Hierarchical label {}", l.name),
                    other => other.kind().to_owned(),
                })
            }
            Mode::Pcb => {
                if let Some(t) = p.pcb().tracks.get(&id) {
                    Some(format!(
                        "Track {} {:.3} mm",
                        sb::pcb_layer_name(t.layer),
                        t.width as f64 / 1e6
                    ))
                } else {
                    p.pcb()
                        .vias
                        .get(&id)
                        .map(|v| format!("Via Ø{:.3} mm", v.diameter as f64 / 1e6))
                }
            }
            _ => None,
        }
    }

    fn summaries(&self) -> Vec<ObjectSummary> {
        let Some(open) = &self.open else {
            return Vec::new();
        };
        let project = open.engine.project();
        let ids: Vec<Id> = self
            .selection
            .get(&self.mode)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        // Connectivity is only needed for a single selected schematic object.
        let conn = (self.mode == Mode::Schematic && ids.len() == 1)
            .then(|| Connectivity::build(project.schematic()));
        ids.into_iter()
            .filter_map(|id| crate::summary::summarize(project, self.mode, id, conn.as_ref()))
            .collect()
    }

    // -- snapshot --------------------------------------------------------------------------------

    fn layer_infos(&self) -> Vec<LayerInfo> {
        let hex = |c: Rgba| format!("#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]);
        match (&self.open, self.mode) {
            (Some(open), Mode::Pcb) => {
                let mut v: Vec<LayerInfo> = sb::pcb_copper_layers(open.engine.project().pcb())
                    .into_iter()
                    .map(|l| LayerInfo {
                        id: l.0 as u32,
                        name: sb::pcb_layer_name(l),
                        color: self
                            .pcb_scene
                            .scene
                            .layer(sb::pcb_layer_id(l))
                            .map(|x| hex(x.color))
                            .unwrap_or_default(),
                        visible: !self.hidden_pcb_layers.contains(&l.0),
                        active: self.active_pcb_layer == l.0,
                        toggleable: true,
                    })
                    .collect();
                v.sort_by_key(|l| l.id);
                v
            }
            (Some(_), Mode::Schematic) => sb::schematic_layers(self.theme_kind())
                .into_iter()
                .filter(|(_, l)| !l.hit_only)
                .rev()
                .map(|(id, l)| LayerInfo {
                    id: id.0 as u32,
                    name: l.name,
                    color: hex(l.color),
                    visible: true,
                    active: false,
                    toggleable: false,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn model_object_count(&self) -> u64 {
        let Some(open) = &self.open else { return 0 };
        let p = open.engine.project();
        match self.mode {
            Mode::Schematic => p.schematic().object_count() as u64,
            Mode::Pcb => (p.pcb().tracks.len() + p.pcb().vias.len()) as u64,
            _ => 0,
        }
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.ensure_scene();
        let project = self.open.as_ref().map(|o| ProjectInfo {
            name: o.engine.project().name().to_owned(),
            path: o.root.to_string_lossy().into_owned(),
            dirty: o.engine.is_dirty(),
            can_undo: o.engine.can_undo(),
            can_redo: o.engine.can_redo(),
            undo_history: o.engine.undo_history(),
            redo_history: o.engine.redo_history(),
        });
        Snapshot {
            project,
            mode: self.mode,
            tool: self.editor.tool.id().to_owned(),
            tool_arg: self.editor.tool.arg(),
            can_paste: self.editor.clipboard.is_some(),
            units: self.units,
            theme: self.theme,
            grid: GridInfo {
                spacing_nm: self.grid.spacing,
                visible: self.grid.visible,
                snap_grid: self.snap.grid,
                snap_objects: self.snap.objects,
            },
            layers: self.layer_infos(),
            selection: self.summaries(),
            gpu_adapter: self.gpu.as_ref().map(|g| g.adapter_name().to_owned()),
            gpu_error: self.gpu_error.clone(),
            debug_overlay: self.debug,
            recent: self.recent.clone(),
            recovery_pending: self.recovery.is_some(),
            messages: self.messages.clone(),
            command_log: self.command_log.clone(),
            object_count: self.model_object_count(),
            zoom_percent: self.zoom_percent(),
            hint: self.hint(),
        }
    }

    // -- rendering -------------------------------------------------------------------------------

    /// Renders the active canvas. Returns `(width, height, rgba)`.
    pub fn render(&mut self, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>), String> {
        self.set_canvas_size(width as f64, height as f64);
        self.ensure_scene();
        if self.scene().is_some() && !self.fitted.contains(&self.mode) {
            self.fit_view();
        }
        if self.gpu.is_none() && self.gpu_error.is_none() {
            match GpuRenderer::new_blocking() {
                Ok(r) => {
                    let name = r.adapter_name().to_owned();
                    self.gpu = Some(r);
                    self.log(MessageLevel::Info, format!("GPU renderer ready: {name}"));
                }
                Err(e) => {
                    self.gpu_error = Some(e.to_string());
                    self.log(
                        MessageLevel::Error,
                        format!("GPU renderer unavailable: {e}"),
                    );
                }
            }
        }
        let theme = match self.theme {
            ThemeName::Dark => Theme::dark(),
            ThemeName::Light => Theme::light(),
        };
        let mut selection = Selection::new();
        let owners: Vec<Id> = self
            .selection
            .get(&self.mode)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        selection.set(self.shapes_for(owners));
        selection.hover = self
            .hover
            .and_then(|h| self.shapes_for([h]).first().copied());
        let box_rect = match self.gesture {
            Gesture::Box { start, end, .. } => Some((start, end)),
            _ => None,
        };
        let preview = self.preview_shapes();
        let Some(gpu) = self.gpu.as_mut() else {
            return Err(self.gpu_error.clone().unwrap_or_else(|| "no GPU".into()));
        };
        let view = *self
            .views
            .entry(self.mode)
            .or_insert_with(|| View::new(width as f64, height as f64));
        let scene = match self.mode {
            Mode::Schematic => &self.sch_scene.scene,
            Mode::Pcb => &self.pcb_scene.scene,
            _ => return Err("this mode has no canvas".into()),
        };
        let req = FrameRequest {
            scene,
            view: &view,
            grid: &self.grid,
            theme: &theme,
            selection: &selection,
            box_select: box_rect,
            preview: &preview,
            debug: self.debug,
        };
        let frame = gpu.render(&req).map_err(|e| e.to_string())?;
        Ok((frame.width, frame.height, frame.rgba))
    }
}
