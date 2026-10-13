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
