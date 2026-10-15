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
    GerberViewer,
    LibraryManager,
    Plugins,
    Simulation,
    Code,
    Agent,
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
    /// The last DRC run.
    pub drc: Option<DrcInfo>,
    /// 3D view state (only in the 3D mode).
    pub viewer3d: Option<Viewer3dInfo>,
    /// The last manufacturing preview.
    pub fab: Option<FabInfo>,
    /// Gerber viewer state (only in the Gerber viewer mode).
    pub gerber: Option<GerberViewerInfo>,
    /// Library Manager state (only in the Library Manager mode).
    pub library_manager: Option<LibraryManagerInfo>,
    /// Plugin Manager state (only in the Plugins mode).
    pub plugins: Option<PluginManagerInfo>,
    /// Simulation state (only in the Simulation mode).
    pub simulation: Option<SimulationInfo>,
    /// ButureScript editor state (only in the Code mode).
    pub code: Option<CodeInfo>,
    /// AI Workspace state (only in the Agent mode).
    pub agent: Option<AgentInfo>,
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

/// A footprint suggested for a placed part (stage 53).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FootprintSuggestion {
    /// `Library:Name`: the value to put in the part's Footprint field.
    pub lib_id: String,
    pub name: String,
    pub library: String,
    pub description: String,
    pub pad_count: u32,
    /// Why it is suggested (`filter` or `pads`) and the sentence for the user.
    pub reason: String,
    pub reason_text: String,
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
    pub ratsnest_visible: bool,
    /// Track width (mm) and angle mode (`45`, `90`, `free`) of the Route tool, via size `d/drill`.
    pub route_width_mm: String,
    /// The width follows the net class (no manual override).
    pub route_width_auto: bool,
    /// `walk` (walk around obstacles) or `ignore`.
    pub route_obstacles: String,
    /// Class of the net being routed (or Default).
    pub route_class: String,
    pub route_mode: String,
    pub via_size: String,
    pub via_size_auto: bool,
    /// The route in progress.
    pub route: Option<RouteInfo>,
    /// Length against the goal of the net under the tuner / of the selected tracks.
    pub length: Option<LengthInfo>,
    /// A zone outline is being drawn.
    pub zone_drawing: bool,
    /// Net the Zone tool assigns.
    #[ts(type = "number | null")]
    pub zone_net: Option<u64>,
    /// Zones are refilled automatically after edits.
    pub auto_fill: bool,
    /// Zones whose fill is missing or out of date.
    pub stale_zones: u32,
    pub zones: u32,
    pub net_choices: Vec<NetChoice>,
    /// Connections still to route (ratsnest lines).
    pub unrouted: u32,
}

/// A route in progress.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RouteInfo {
    /// Why the leg to the pointer cannot be routed (walk-around found no way), if so.
    pub problem: Option<String>,
    /// Extra information about the leg (e.g. “pushing 2 track(s)”, “cannot push…; walking around”).
    pub note: Option<String>,
    /// Tracks the current route would displace (Push & Shove).
    pub pushed: u32,
    /// Set while a differential pair is being routed.
    pub pair: Option<PairLive>,
    pub net: String,
    pub layer: String,
    pub length_mm: String,
    pub vias: u32,
}

/// One line of the "Update PCB from Schematic" preview.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PlanLineInfo {
    /// `add_footprint`, `remove_footprint`, `change_footprint`, `link`, `update_field`,
    /// `add_net`, `remove_net`, `rename_net`, `pad_net` or `warning`.
    pub kind: String,
    pub text: String,
}

/// What "Update PCB from Schematic" would do, computed without changing anything.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UpdatePreview {
    pub lines: Vec<PlanLineInfo>,
    /// There is something to apply (warnings alone do not count).
    pub can_apply: bool,
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

/// One DRC violation as the UI shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DrcViolationInfo {
    /// Position in the report; pass it to `drc_focus`.
    pub index: u32,
    pub rule: String,
    pub title: String,
    /// `error` or `warning`.
    pub severity: String,
    pub message: String,
    pub layer: Option<String>,
}

/// The result of the last DRC run.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DrcInfo {
    /// The board changed after the run: the list may be out of date.
    pub stale: bool,
    pub errors: u32,
    pub warnings: u32,
    pub violations: Vec<DrcViolationInfo>,
    pub truncated: u32,
}

/// Manufacturing preview: preflight issues and the files `production.zip` would contain.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FabInfo {
    /// The board changed after the preview.
    pub stale: bool,
    /// Preflight errors block `production.zip`.
    pub blocked: bool,
    pub errors: u32,
    pub warnings: u32,
    pub issues: Vec<FabIssueInfo>,
    pub files: Vec<FabFileInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FabIssueInfo {
    /// `error`, `warning` or `note`.
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FabFileInfo {
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub summary: String,
}

/// One net class, dimensions in mm (text, so the UI shows exactly what is stored).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NetClassInfo {
    pub name: String,
    pub track_width: String,
    pub clearance: String,
    pub via_diameter: String,
    pub via_drill: String,
    pub diff_pair_width: String,
    pub diff_pair_gap: String,
}

/// A net and its class.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NetAssignInfo {
    #[ts(type = "number")]
    pub id: u64,
    pub name: String,
    pub class: String,
}

/// The board's design rules as the Net Classes dialog edits them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RulesInfo {
    pub min_track_width: String,
    pub min_via_diameter: String,
    pub min_via_drill: String,
    pub min_annular_ring: String,
    pub copper_to_edge: String,
    pub classes: Vec<NetClassInfo>,
    pub nets: Vec<NetAssignInfo>,
    /// Names of the preset classes the dialog can add.
    pub presets: Vec<String>,
    pub pairs: Vec<PairInfo>,
    /// Pairs found from net names that are not defined yet.
    pub suggested_pairs: Vec<PairInfo>,
}

/// A differential pair; empty width/gap mean “follow the net class”.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairInfo {
    pub name: String,
    #[ts(type = "number")]
    pub positive: u64,
    #[ts(type = "number")]
    pub negative: u64,
    pub width: String,
    pub gap: String,
    pub max_skew: String,
}

/// Live numbers of a differential pair route.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PairLive {
    pub name: String,
    pub length_p_mm: String,
    pub length_n_mm: String,
    pub skew_mm: String,
    pub max_skew_mm: String,
    /// Smallest edge-to-edge gap of the coupled part (empty before there is one).
    pub gap_mm: String,
    pub gap_target_mm: String,
    pub over_skew: bool,
}

/// Length of the net under the tuner or of the selected tracks, against its goal.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LengthInfo {
    pub net: String,
    pub current_mm: String,
    pub target_mm: Option<String>,
    pub tolerance_mm: Option<String>,
    /// Current − target (positive: too long).
    pub difference_mm: Option<String>,
    /// Length after the previewed meander is placed.
    pub predicted_mm: Option<String>,
    /// The goal is the partner leg of a differential pair, not a set target.
    pub from_pair: bool,
    pub within: bool,
    /// The previewed meander does not reach the target (segment too short).
    pub partial: bool,
    pub problem: Option<String>,
    /// The net has its own target (so it can be cleared).
    pub has_target: bool,
    pub style: String,
    pub amplitude_mm: String,
    pub pitch_mm: String,
}

/// A net the UI can pick.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NetChoice {
    #[ts(type = "number")]
    pub id: u64,
    pub name: String,
}

/// A dielectric layer row of the Layer Stack Manager (numbers as text: mm, relative
/// permittivity, loss tangent).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DielRow {
    /// `core`, `prepreg`, `solder_mask`, `coverlay` or `adhesive`.
    pub kind: String,
    pub material: String,
    pub thickness: String,
    pub er: String,
    pub loss_tangent: String,
    pub flex: bool,
}

/// A copper layer row.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CopperRow {
    pub layer: String,
    pub layer_id: u32,
    pub thickness: String,
    /// `signal`, `plane` or `mixed`.
    pub role: String,
}

/// A template or a saved preset the manager can load.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TemplateInfo {
    pub name: String,
    pub copper_layers: u32,
    /// `built-in`, `project` or `user`.
    pub scope: String,
    pub error: Option<String>,
}

/// The layer stack as the manager edits it. `templates` and `presets` are only read.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StackupInfo {
    pub name: String,
    pub copper_layers: u32,
    pub finish: String,
    /// Total thickness (mm), derived.
    pub total_thickness: String,
    /// Copper layers, front to back.
    pub copper: Vec<CopperRow>,
    /// `gaps[i]` lies between copper layer `i` and `i + 1`, top to bottom.
    pub gaps: Vec<Vec<DielRow>>,
    pub top_mask: Option<DielRow>,
    pub bottom_mask: Option<DielRow>,
    #[serde(default)]
    pub templates: Vec<TemplateInfo>,
    #[serde(default)]
    pub presets: Vec<TemplateInfo>,
}

/// State of the 3D view.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Viewer3dInfo {
    /// `perspective` or `orthographic`.
    pub projection: String,
    pub substrate: bool,
    pub mask: bool,
    pub copper: bool,
    pub silkscreen: bool,
    pub components: bool,
    pub components_count: u32,
    /// Components whose footprint references a 3D model (drawn tinted; models are not parsed yet).
    pub with_model_refs: u32,
    pub has_outline: bool,
    pub tracks: u32,
    pub zone_pieces: u32,
}

/// The Gerber viewer: parsed files, not the board model.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GerberViewerInfo {
    pub loaded: bool,
    pub source: String,
    pub layers: Vec<GerberLayerInfo>,
    pub selected: Option<u32>,
    /// Apertures of the selected Gerber layer.
    pub apertures: Vec<GerberApertureInfo>,
    /// Tools of the selected drill layer.
    pub tools: Vec<GerberToolInfo>,
    /// Files that could not be parsed.
    pub errors: Vec<String>,
    /// Parsed-files-vs-board comparison (only for the current preview).
    pub checks: Vec<GerberCheckInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GerberLayerInfo {
    pub name: String,
    /// `gerber` or `drill`.
    pub kind: String,
    pub visible: bool,
    pub color: String,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GerberApertureInfo {
    pub code: String,
    pub shape: String,
    pub uses: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GerberToolInfo {
    pub tool: String,
    pub diameter_mm: String,
    pub holes: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GerberCheckInfo {
    pub ok: bool,
    pub text: String,
}

/// One row of the Library Manager result list.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LibraryRow {
    /// Stable key (`kind:Library:Name`) for favorites, recents and selection.
    pub key: String,
    /// `symbol`, `footprint`, `model` or `component`.
    pub kind: String,
    /// `builtin`, `project` or `user`.
    pub scope: String,
    pub library: String,
    pub name: String,
    pub id: String,
    pub description: String,
    pub manufacturer: String,
    pub mpn: String,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LibraryParam {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LibraryDetail {
    pub row: LibraryRow,
    pub symbol: String,
    pub footprint: String,
    /// Footprint proposed by a symbol.
    pub default_footprint: String,
    pub model3d: String,
    /// Stored path or URL; ButureCAD never fetches it.
    pub datasheet: String,
    pub keywords: String,
    pub parameters: Vec<LibraryParam>,
    /// Component references that do not resolve to an indexed item.
    pub unresolved: Vec<String>,
    pub placeable: bool,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct LibraryManagerInfo {
    pub total: u32,
    pub text: String,
    pub kinds: Vec<String>,
    pub library: String,
    pub manufacturer: String,
    /// `all`, `favorites` or `recents`.
    pub scope: String,
    pub libraries: Vec<String>,
    pub manufacturers: Vec<String>,
    pub results: Vec<LibraryRow>,
    pub truncated: bool,
    /// Library files that could not be read.
    pub problems: Vec<String>,
    /// Favorites/recents are saved (a user library folder exists).
    pub prefs_persisted: bool,
    pub detail: Option<LibraryDetail>,
    /// Folders that can receive new components: `project`, `user`.
    pub writable_scopes: Vec<String>,
}

/// An installed plugin.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub categories: Vec<String>,
    /// Permissions the manifest asks for.
    pub requested: Vec<String>,
    /// Permissions the user granted (a subset of `requested`).
    pub granted: Vec<String>,
    pub enabled: bool,
    pub loaded: bool,
    pub error: Option<String>,
}

/// A search hit returned by a component provider plugin.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PluginHit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub manufacturer: String,
    pub mpn: String,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct PluginManagerInfo {
    /// A plugin folder exists in this session.
    pub available: bool,
    pub plugins: Vec<PluginRow>,
    /// Enabled component providers.
    pub providers: Vec<String>,
    pub provider: String,
    pub query: String,
    pub hits: Vec<PluginHit>,
    pub error: String,
    /// Host requests the sandbox refused during the last search.
    pub audit: Vec<String>,
    pub last_import: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SimulationBackendInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SimulationSeriesInfo {
    pub name: String,
    pub unit: String,
    pub values: Vec<f64>,
}

/// A simulation result, decimated to at most 1500 points per curve for plotting.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SimulationResultInfo {
    /// The schematic changed after the run.
    pub stale: bool,
    pub analysis: String,
    pub backend: String,
    pub x_label: String,
    pub x_unit: String,
    pub x_log: bool,
    pub x: Vec<f64>,
    /// The data had more points than shown.
    pub decimated: bool,
    pub points: u32,
    pub series: Vec<SimulationSeriesInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SimulationInfo {
    pub backends: Vec<SimulationBackendInfo>,
    pub backend: String,
    /// `op`, `dc`, `ac` or `tran`.
    pub analysis: String,
    pub source: String,
    pub sources: Vec<String>,
    pub start: String,
    pub stop: String,
    pub step: String,
    pub ppd: String,
    pub fstart: String,
    pub fstop: String,
    pub tstep: String,
    pub tstop: String,
    /// The generated SPICE netlist.
    pub netlist: String,
    pub notes: Vec<String>,
    /// Problems that stop the simulation.
    pub problems: Vec<String>,
    pub error: String,
    pub result: Option<SimulationResultInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CodeDiagnostic {
    /// `error` or `warning`.
    pub severity: String,
    pub code: String,
    pub message: String,
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

/// The ButureScript editor: the source text lives in the UI, everything derived from it here.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CodeInfo {
    /// `code`, `split` or `visual`.
    pub view: String,
    /// `schematic`, `pcb` or `3d`.
    pub preview: String,
    pub has_preview: bool,
    /// The text differs from the saved `design.beda`.
    pub dirty: bool,
    pub saved_file: bool,
    pub diagnostics: Vec<CodeDiagnostic>,
    pub summary: String,
    /// Increments each time a new preview was built.
    pub generation: u32,
    pub has_project: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentProviderInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `rule-based`, `external input` or `model`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentDiffInfo {
    /// `added`, `removed` or `modified`.
    pub change: String,
    pub domain: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentDeltaInfo {
    pub errors_before: u32,
    pub errors_after: u32,
    pub warnings_before: u32,
    pub warnings_after: u32,
    pub resolved: u32,
    /// Findings that exist only after the change.
    pub new: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentProposalInfo {
    pub id: String,
    pub title: String,
    pub origin: String,
    /// `pending`, `invalid`, `applied` or `rejected`.
    pub status: String,
    /// The project changed since validation: it cannot be applied any more.
    pub stale: bool,
    pub actions: Vec<String>,
    pub rationale: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub failed_action: Option<String>,
    pub diff: Vec<AgentDiffInfo>,
    pub erc: Option<AgentDeltaInfo>,
    pub drc: Option<AgentDeltaInfo>,
    /// The raw structured proposal (only for the selected one).
    pub json: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentInfo {
    pub providers: Vec<AgentProviderInfo>,
    pub provider: String,
    pub proposals: Vec<AgentProposalInfo>,
    pub selected: Option<String>,
    pub error: String,
    pub audit: Vec<String>,
}
