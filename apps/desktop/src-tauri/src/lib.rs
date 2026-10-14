//! ButureCAD desktop shell (Tauri 2). Contains no EDA logic: it owns the application state
//! (`workspace`), translates it to scenes (`scene_builder`) and exposes it over typed IPC.

pub mod commands;
pub mod pcb_scene;
pub mod scene_builder;
pub mod summary;
pub mod ui_types;
pub mod workspace;

use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;

/// Registers every IPC command. Runtime-generic so the same set serves the real app and the
/// mock runtime used by the IPC tests and the e2e bridge.
pub fn with_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::core_info,
        commands::get_snapshot,
        commands::project_new,
        commands::project_open,
        commands::project_save,
        commands::project_save_as,
        commands::project_close,
        commands::project_recovery,
        commands::project_rename,
        commands::autosave_tick,
        commands::undo,
        commands::redo,
        commands::set_mode,
        commands::set_theme,
        commands::set_units,
        commands::set_grid,
        commands::set_debug_overlay,
        commands::set_pcb_layer_visible,
        commands::set_active_layer,
        commands::view_fit,
        commands::view_zoom,
        commands::editor_command,
        commands::pointer,
        commands::render_frame,
        commands::library_symbols,
        commands::property_set,
        commands::symbol_editor_command,
        commands::symbol_editor_set,
        commands::symbol_libraries,
        commands::pcb_command,
        commands::pcb_footprints,
        commands::pcb_update_preview,
        commands::pcb_update_apply,
        commands::pcb_rules,
        commands::pcb_rules_apply,
        commands::pcb_stackup,
        commands::pcb_stackup_template,
        commands::pcb_stackup_preset_load,
        commands::pcb_stackup_preset_save,
        commands::pcb_stackup_preset_delete,
        commands::pcb_stackup_apply,
        commands::pcb_stackup_swap,
        commands::viewer3d_command,
        commands::mcad_export,
        commands::fab_preview,
        commands::fab_export,
        commands::gerber_command,
        commands::library_command,
        commands::plugin_command,
        commands::simulation_command,
        commands::library_place_symbol,
        commands::drc_run,
        commands::drc_focus,
        commands::erc_run,
        commands::erc_focus,
        commands::erc_waive,
        commands::erc_unwaive,
        commands::footprint_editor_command,
        commands::footprint_editor_set,
        commands::footprint_libraries,
        commands::library_footprints,
        commands::footprint_generators,
    ])
}

pub fn run() {
    let builder = with_commands(
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .setup(|app| {
                let recent = app
                    .path()
                    .app_config_dir()
                    .ok()
                    .map(|d| d.join("recent-projects.json"));
                let mut ws = workspace::Workspace::new(recent);
                if let Ok(dir) = app.path().app_data_dir() {
                    ws = ws
                        .with_user_libraries(dir.join("libraries"))
                        .with_plugin_dir(dir.join("plugins"));
                }
                // `buturecad /path/to/project` opens that project at startup.
                if let Some(path) = std::env::args().nth(1) {
                    if let Err(e) = ws.open_project(std::path::Path::new(&path)) {
                        ws.log(
                            ui_types::MessageLevel::Error,
                            format!("Could not open {path}: {e}"),
                        );
                    }
                }
                app.manage(AppState(Mutex::new(ws)));
                Ok(())
            }),
    );
    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("ButureCAD failed to start: {err}");
        std::process::exit(1);
    }
}
