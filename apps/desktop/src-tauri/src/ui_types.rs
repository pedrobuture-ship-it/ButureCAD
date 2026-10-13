//! Typed contract between the Rust core and the React UI.
//!
//! These types are exported to TypeScript by `ts-rs` (`cargo test` regenerates
//! `apps/desktop/src/bindings/*.ts`), so the two sides cannot drift apart silently.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mode {
    Schematic,
    Pcb,
    SymbolEditor,
    FootprintEditor,
    Viewer3d,
}

impl Mode {
    /// Modes that have a 2D canvas backed by real data today.
    pub fn has_canvas(self) -> bool {
        matches!(self, Mode::Schematic | Mode::Pcb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Units {
    Mm,
    Mil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThemeName {
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_history: Vec<String>,
    pub redo_history: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LayerInfo {
    pub id: u32,
    pub name: String,
    /// CSS colour, e.g. `#dd4444`.
    pub color: String,
    pub visible: bool,
    pub active: bool,
    /// Whether toggling visibility / activating is supported for this layer today.
    pub toggleable: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PropertyInfo {
    pub key: String,
    pub label: String,
    pub value: String,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ObjectSummary {
    #[ts(type = "number")]
    pub id: u64,
    pub kind: String,
    pub title: String,
    pub properties: Vec<PropertyInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GridInfo {
    #[ts(type = "number")]
    pub spacing_nm: i64,
    pub visible: bool,
    pub snap_grid: bool,
    pub snap_objects: bool,
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Message {
    #[ts(type = "number")]
    pub seq: u64,
    pub level: MessageLevel,
    pub text: String,
}

/// Everything the UI needs to render its chrome. Returned by every state-changing command.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Snapshot {
    pub project: Option<ProjectInfo>,
    pub mode: Mode,
    pub tool: String,
    pub units: Units,
    pub theme: ThemeName,
    pub grid: GridInfo,
    pub layers: Vec<LayerInfo>,
    pub selection: Vec<ObjectSummary>,
    pub gpu_adapter: Option<String>,
    pub gpu_error: Option<String>,
    pub debug_overlay: bool,
    pub recent: Vec<RecentProject>,
    /// An autosave from a crashed session is waiting for a decision.
    pub recovery_pending: bool,
    pub messages: Vec<Message>,
    /// Executed commands this session, oldest first (the "Console" panel).
    pub command_log: Vec<String>,
    /// Model objects shown in the current mode (schematic objects, or PCB tracks + vias).
    #[ts(type = "number")]
    pub object_count: u64,
    pub zoom_percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PointerKind {
    Move,
    Down,
    Up,
    Wheel,
    DoubleClick,
    Leave,
}

/// A pointer event in canvas pixels (device pixels, origin top-left).
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub x: f64,
    pub y: f64,
    /// 0 = primary, 1 = middle, 2 = secondary.
    pub button: i32,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Wheel delta (positive = scroll down).
    pub delta: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PointerResult {
    pub redraw: bool,
    #[ts(type = "number")]
    pub cursor_x_nm: i64,
    #[ts(type = "number")]
    pub cursor_y_nm: i64,
    pub snap: String,
    pub hover: Option<String>,
    pub zoom_percent: f64,
    /// The selection changed: the UI should refresh the inspector.
    pub selection_changed: bool,
    /// The model changed (a command ran): the UI should refresh the snapshot.
    pub model_changed: bool,
    /// Short hint for the status bar describing what the active tool expects next.
    pub hint: String,
}
