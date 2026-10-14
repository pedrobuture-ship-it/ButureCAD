//! Thin IPC layer: every command locks the workspace and delegates. No EDA logic lives here.

use crate::ui_types::*;
use crate::workspace::Workspace;
use buturecad_eda_core as core;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use ts_rs::TS;

pub struct AppState(pub Mutex<Workspace>);

impl AppState {
    fn with<T>(&self, f: impl FnOnce(&mut Workspace) -> Result<T, String>) -> Result<T, String> {
        let mut ws = self
            .0
            .lock()
            .map_err(|_| "internal error: workspace lock poisoned".to_string())?;
        f(&mut ws)
    }

    /// Runs a state-changing action and returns the fresh snapshot. Failures are logged to the
    /// Messages panel as well as returned to the caller.
    fn act(
        &self,
        f: impl FnOnce(&mut Workspace) -> Result<(), String>,
    ) -> Result<Snapshot, String> {
        self.with(|ws| {
            if let Err(e) = f(ws) {
                ws.log(MessageLevel::Error, e.clone());
                return Err(e);
            }
            Ok(ws.snapshot())
        })
    }
}

/// Typed contract for `core_info`.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct CoreInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[tauri::command]
pub fn core_info() -> CoreInfo {
    CoreInfo {
        name: core::NAME,
        version: core::VERSION,
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.with(|ws| Ok(ws.snapshot()))
}

#[tauri::command]
pub fn project_new(
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.new_project(&PathBuf::from(parent).join(&name), &name))
}

#[tauri::command]
pub fn project_open(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.open_project(&PathBuf::from(path)))
}

#[tauri::command]
pub fn project_save(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.save())
}

#[tauri::command]
pub fn project_save_as(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.save_as(&PathBuf::from(path)))
}

#[tauri::command]
pub fn project_close(state: State<'_, AppState>, force: bool) -> Result<Snapshot, String> {
    state.act(|ws| ws.close_project_force(force))
}

#[tauri::command]
pub fn project_recovery(state: State<'_, AppState>, accept: bool) -> Result<Snapshot, String> {
    state.act(|ws| ws.resolve_recovery(accept))
}

#[tauri::command]
pub fn project_rename(state: State<'_, AppState>, name: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.rename_project(&name))
}

#[tauri::command]
pub fn autosave_tick(state: State<'_, AppState>) -> Result<bool, String> {
    state.with(|ws| ws.autosave_tick())
}

#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.undo().map(|_| ()))
}

#[tauri::command]
pub fn redo(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.redo().map(|_| ()))
}

#[tauri::command]
pub fn set_mode(state: State<'_, AppState>, mode: Mode) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.set_mode(mode);
        Ok(())
    })
}

#[tauri::command]
pub fn set_theme(state: State<'_, AppState>, theme: ThemeName) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.set_theme(theme);
        Ok(())
    })
}

#[tauri::command]
pub fn set_units(state: State<'_, AppState>, units: Units) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.units = units;
        Ok(())
    })
}

#[tauri::command]
pub fn set_grid(
    state: State<'_, AppState>,
    spacing_nm: Option<i64>,
    visible: Option<bool>,
    snap_grid: Option<bool>,
    snap_objects: Option<bool>,
) -> Result<Snapshot, String> {
    state.act(|ws| {
        if let Some(s) = spacing_nm {
            if !(1_000..=100_000_000).contains(&s) {
                return Err("grid spacing must be between 0.001 mm and 100 mm".into());
            }
            ws.grid.spacing = s;
        }
        if let Some(v) = visible {
            ws.grid.visible = v;
        }
        if let Some(v) = snap_grid {
            ws.snap.grid = v;
        }
        if let Some(v) = snap_objects {
            ws.snap.objects = v;
        }
        Ok(())
    })
}

#[tauri::command]
pub fn set_debug_overlay(state: State<'_, AppState>, enabled: bool) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.debug = enabled;
        Ok(())
    })
}

#[tauri::command]
pub fn set_pcb_layer_visible(
    state: State<'_, AppState>,
    layer: u16,
    visible: bool,
) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.set_pcb_layer_visible(layer, visible);
        Ok(())
    })
}

#[tauri::command]
pub fn set_active_layer(state: State<'_, AppState>, layer: u16) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.active_pcb_layer = layer;
        Ok(())
    })
}

#[tauri::command]
pub fn view_fit(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.fit_view();
        Ok(())
    })
}

#[tauri::command]
pub fn view_zoom(state: State<'_, AppState>, factor: f64) -> Result<Snapshot, String> {
    state.act(|ws| {
        ws.zoom_by(factor);
        Ok(())
    })
}

#[tauri::command]
pub fn editor_command(state: State<'_, AppState>, name: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.editor_command(&name))
}

#[tauri::command]
pub fn pointer(state: State<'_, AppState>, event: PointerEvent) -> Result<PointerResult, String> {
    state.with(|ws| Ok(ws.pointer(event)))
}

/// Renders the canvas. Binary reply: `width: u32 LE`, `height: u32 LE`, then RGBA8 pixels.
#[tauri::command]
pub fn render_frame(
    state: State<'_, AppState>,
    width: u32,
    height: u32,
) -> Result<tauri::ipc::Response, String> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("invalid canvas size".into());
    }
    let (w, h, rgba) = state.with(|ws| ws.render(width, height))?;
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&rgba);
    Ok(tauri::ipc::Response::new(out))
}

#[tauri::command]
pub fn library_symbols(state: State<'_, AppState>) -> Result<Vec<SymbolInfo>, String> {
    state.with(|ws| Ok(ws.library_symbols()))
}

#[tauri::command]
pub fn property_set(
    state: State<'_, AppState>,
    id: u64,
    key: String,
    value: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.set_property(id, &key, &value))
}

/// Symbol Editor lifecycle and view commands (see `Workspace::symbol_editor_command`).
#[tauri::command]
pub fn symbol_editor_command(
    state: State<'_, AppState>,
    name: String,
    arg: Option<String>,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.symbol_editor_command(&name, arg.as_deref()))
}

/// Sets a symbol-level field (name, prefix, units, metadata…) of the symbol being edited.
#[tauri::command]
pub fn symbol_editor_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.symbol_editor_set(&key, &value))
}

#[tauri::command]
pub fn symbol_libraries(state: State<'_, AppState>) -> Result<Vec<LibraryInfo>, String> {
    state.with(|ws| Ok(ws.symbol_libraries()))
}

/// Runs the Electrical Rules Check on the open project.
/// The board's design rules and net classes.
#[tauri::command]
pub fn pcb_rules(state: State<'_, AppState>) -> Result<RulesInfo, String> {
    state.with(|ws| ws.pcb_rules_info())
}

/// Applies edited rules as one undo step.
#[tauri::command]
pub fn pcb_rules_apply(state: State<'_, AppState>, rules: RulesInfo) -> Result<Snapshot, String> {
    state.act(|ws| ws.pcb_rules_apply(rules))
}

/// The layer stack with the templates and presets the manager offers.
#[tauri::command]
pub fn pcb_stackup(state: State<'_, AppState>) -> Result<StackupInfo, String> {
    state.with(|ws| ws.stackup_info())
}

/// A built-in template (`template` non-empty) or the standard stack for `copper_layers`.
#[tauri::command]
pub fn pcb_stackup_template(
    state: State<'_, AppState>,
    template: String,
    copper_layers: u32,
) -> Result<StackupInfo, String> {
    state.with(|ws| ws.stackup_template(&template, copper_layers))
}

#[tauri::command]
pub fn pcb_stackup_preset_load(
    state: State<'_, AppState>,
    scope: String,
    name: String,
) -> Result<StackupInfo, String> {
    state.with(|ws| ws.stackup_preset_load(&scope, &name))
}

#[tauri::command]
pub fn pcb_stackup_preset_save(
    state: State<'_, AppState>,
    scope: String,
    name: String,
    stackup: StackupInfo,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.stackup_preset_save(&scope, &name, stackup))
}

#[tauri::command]
pub fn pcb_stackup_preset_delete(
    state: State<'_, AppState>,
    scope: String,
    name: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.stackup_preset_delete(&scope, &name))
}

/// Applies the edited layer stack (and layer count) as one undo step.
#[tauri::command]
pub fn pcb_stackup_apply(
    state: State<'_, AppState>,
    stackup: StackupInfo,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.stackup_apply(stackup))
}

/// Exchanges the contents of two inner copper layers (one undo step).
#[tauri::command]
pub fn pcb_stackup_swap(state: State<'_, AppState>, a: u16, b: u16) -> Result<Snapshot, String> {
    state.act(|ws| ws.stackup_swap_inner(buturecad_pcb::LayerId(a), buturecad_pcb::LayerId(b)))
}

/// 3D view actions (projection, preset views, layer toggles).
#[tauri::command]
pub fn viewer3d_command(
    state: State<'_, AppState>,
    name: String,
    arg: Option<String>,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.viewer3d_command(&name, arg.as_deref()))
}

/// Exports the board's mechanical model (`.bmech`; STEP is reported as unsupported).
#[tauri::command]
pub fn mcad_export(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.mcad_export(&path))
}

/// Manufacturing preview: DRC, preflight and the list of production files (nothing is written).
#[tauri::command]
pub fn fab_preview(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.fab_preview())
}

/// Writes `production.zip` (refused while the preflight has errors).
#[tauri::command]
pub fn fab_export(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.fab_export(&path))
}

/// Gerber viewer actions (load the preview, open a folder/zip, layer toggles, verify).
#[tauri::command]
pub fn gerber_command(
    state: State<'_, AppState>,
    name: String,
    arg: Option<String>,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.gerber_command(&name, arg.as_deref()))
}

#[tauri::command]
pub fn drc_run(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.drc_run())
}

#[tauri::command]
pub fn drc_focus(state: State<'_, AppState>, index: u32) -> Result<Snapshot, String> {
    state.act(|ws| ws.drc_focus(index as usize))
}

#[tauri::command]
pub fn erc_run(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.erc_run())
}

/// Selects the finding's objects and centres the schematic view on it.
#[tauri::command]
pub fn erc_focus(state: State<'_, AppState>, index: u32) -> Result<Snapshot, String> {
    state.act(|ws| ws.erc_focus(index as usize))
}

/// Documents an exception for an ERC finding (a reason is required).
#[tauri::command]
pub fn erc_waive(
    state: State<'_, AppState>,
    fingerprint: String,
    reason: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.erc_waive(&fingerprint, &reason))
}

#[tauri::command]
pub fn erc_unwaive(state: State<'_, AppState>, fingerprint: String) -> Result<Snapshot, String> {
    state.act(|ws| ws.erc_unwaive(&fingerprint))
}

/// Footprint Editor lifecycle, view and generator commands
/// (see `Workspace::footprint_editor_command`).
#[tauri::command]
pub fn footprint_editor_command(
    state: State<'_, AppState>,
    name: String,
    arg: Option<String>,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.footprint_editor_command(&name, arg.as_deref()))
}

/// Sets a footprint-level field (name, description, mounting, 3D model reference…).
#[tauri::command]
pub fn footprint_editor_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.footprint_editor_set(&key, &value))
}

#[tauri::command]
pub fn footprint_libraries(state: State<'_, AppState>) -> Result<Vec<LibraryInfo>, String> {
    state.with(|ws| Ok(ws.footprint_libraries()))
}

#[tauri::command]
pub fn library_footprints(state: State<'_, AppState>) -> Result<Vec<FootprintInfo>, String> {
    state.with(|ws| Ok(ws.library_footprints()))
}

/// The footprint generators and their parameters (static; no project needed).
#[tauri::command]
pub fn footprint_generators() -> Vec<GeneratorInfo> {
    crate::workspace::generator_infos()
}

/// Board-level commands: stackup, active layer (see `Workspace::pcb_command`).
#[tauri::command]
pub fn pcb_command(
    state: State<'_, AppState>,
    name: String,
    arg: Option<String>,
) -> Result<Snapshot, String> {
    state.act(|ws| ws.pcb_command(&name, arg.as_deref()))
}

/// Footprints the PCB placement tool can offer.
#[tauri::command]
pub fn pcb_footprints(state: State<'_, AppState>) -> Result<Vec<FootprintInfo>, String> {
    state.with(|ws| Ok(ws.pcb_footprint_choices()))
}

/// Previews "Update PCB from Schematic" without changing anything.
#[tauri::command]
pub fn pcb_update_preview(state: State<'_, AppState>) -> Result<UpdatePreview, String> {
    state.with(|ws| ws.pcb_update_preview())
}

/// Applies the previewed update as one undo step.
#[tauri::command]
pub fn pcb_update_apply(state: State<'_, AppState>) -> Result<Snapshot, String> {
    state.act(|ws| ws.pcb_update_apply())
}
