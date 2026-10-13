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
        matches!(
            self,
            Mode::Schematic | Mode::Pcb | Mode::SymbolEditor | Mode::FootprintEditor
        )
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
    /// Fixed choices (drop-down) for enumerated properties; empty for free text.
    pub options: Vec<OptionInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OptionInfo {
    pub value: String,
    pub label: String,
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
    /// Argument of the active tool (symbol id for placement tools, text for label tools).
    pub tool_arg: Option<String>,
    /// The clipboard holds something to paste.
    pub can_paste: bool,
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
    /// What the active tool expects next (status bar).
    pub hint: String,
    /// Present while the Symbol Editor has a symbol open.
    pub symbol_editor: Option<SymbolEditorInfo>,
    /// The last ERC run, if any.
    pub erc: Option<ErcInfo>,
    /// Present while the Footprint Editor has a footprint open.
    pub footprint_editor: Option<FootprintEditorInfo>,
    /// Board state (stackup, outline warnings…).
    pub pcb: Option<PcbInfo>,
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

/// A library symbol offered by the placement tools and the Symbol Editor.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SymbolInfo {
    pub lib_id: String,
    pub name: String,
    pub reference_prefix: String,
    pub is_power: bool,
    pub pin_count: u32,
    /// Library the symbol belongs to (`Buture` is the built-in bootstrap set).
    pub library: String,
    /// Where the definition came from: `builtin`, `project` (embedded in the project) or a
    /// library scope (`project-library`, `user-library`).
    pub source: String,
    pub description: String,
    pub unit_count: u32,
    pub has_alternate: bool,
}

/// A library file found on disk.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LibraryInfo {
    pub name: String,
    /// `project` (the project's `libraries/` folder) or `user`.
    pub scope: String,
    pub item_count: u32,
    pub error: Option<String>,
}

/// One ERC finding as the UI shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ViolationInfo {
    /// Position in the report; pass it to `erc_focus`.
    pub index: u32,
    /// Machine name of the rule (`unconnected_pin`, …).
    pub rule: String,
    pub title: String,
    /// `error`, `warning` or `info`.
    pub severity: String,
    pub message: String,
    pub sheet: String,
    /// Selecting objects and centring the view is possible (the finding has objects or a place
    /// on the visible sheet).
    pub can_focus: bool,
    pub fingerprint: String,
    /// The documented reason when the finding is waived.
    pub waived: Option<String>,
}

/// The result of the last ERC run.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ErcInfo {
    /// The model changed after the run: the list may be out of date.
    pub stale: bool,
    pub errors: u32,
    pub warnings: u32,
    pub infos: u32,
    pub waived: u32,
    pub violations: Vec<ViolationInfo>,
    /// Findings not listed because the list is capped.
    pub truncated: u32,
}

/// A footprint offered by the Footprint Editor's open dialog.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FootprintInfo {
    pub lib_id: String,
    pub name: String,
    pub library: String,
    /// `project` (embedded in the project), `project-library` or `user-library`.
    pub source: String,
    pub description: String,
    pub pad_count: u32,
    pub tags: String,
}

/// One parameter of a footprint generator.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GeneratorParamInfo {
    pub key: String,
    pub label: String,
    pub default: String,
    /// `integer`, `millimetres` or `flag`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GeneratorInfo {
    pub family: String,
    pub title: String,
    pub params: Vec<GeneratorParamInfo>,
}

/// The editable fields of one 3D model reference.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ModelInfo {
    pub index: u32,
    pub fields: Vec<PropertyInfo>,
}

/// State of the Footprint Editor for the footprint being edited.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FootprintEditorInfo {
    pub name: String,
    /// Layer new graphics are drawn on (`silkscreen`, `fabrication`, …).
    pub layer: String,
    pub pad_count: u32,
    /// Type / shape / size / drill applied to the next pad placed.
    pub pad_template: Vec<PropertyInfo>,
    pub fields: Vec<PropertyInfo>,
    /// 3D model references (stored only; nothing renders them yet).
    pub models: Vec<ModelInfo>,
    pub warnings: Vec<String>,
    pub generators: Vec<GeneratorInfo>,
}

/// Board-level state for the PCB view.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PcbInfo {
    pub copper_layers: u32,
    pub active_layer: String,
    pub footprints: u32,
    pub nets: u32,
    pub warnings: Vec<String>,
}

/// State of the Symbol Editor for the symbol being edited.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SymbolEditorInfo {
    pub lib_id: String,
    /// Currently shown unit (1-based) and representation (1 normal, 2 alternate).
    pub unit: u32,
    pub unit_count: u32,
    pub representation: u32,
    pub has_alternate: bool,
    /// Placed instances that use this symbol in the open project.
    pub used_by: u32,
    /// Pin type / orientation / length applied to the next pin placed.
    pub pin_template: Vec<PropertyInfo>,
    /// Symbol-level fields (name, prefix, metadata, …).
    pub fields: Vec<PropertyInfo>,
    /// Custom `property:<key>` entries.
    pub custom: Vec<PropertyInfo>,
    /// Problems that make the symbol hard to use (does not block editing).
    pub warnings: Vec<String>,
}
