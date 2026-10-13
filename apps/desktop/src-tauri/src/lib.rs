//! Tauri shell. Contains no EDA logic: every command delegates to the Rust core.

use buturecad_eda_core as core;
use serde::Serialize;

/// Typed contract mirrored by `src/api.ts`.
#[derive(Serialize)]
struct CoreInfo {
    name: &'static str,
    version: &'static str,
}

#[tauri::command]
fn core_info() -> CoreInfo {
    CoreInfo {
        name: core::NAME,
        version: core::VERSION,
    }
}

pub fn run() {
    if let Err(err) = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![core_info])
        .run(tauri::generate_context!())
    {
        eprintln!("ButureCAD failed to start: {err}");
        std::process::exit(1);
    }
}
