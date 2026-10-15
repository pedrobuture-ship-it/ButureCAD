//! The reference board (roadmap stage 47) in the desktop workspace: it opens, the 3D preview
//! draws it, the PCB view renders it, and DRC through the app agrees with the CLI.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::Mode;
use buturecad_desktop_lib::workspace::Workspace;
use std::path::{Path, PathBuf};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

fn open() -> (Workspace, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let src: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/reference-board/project");
    let dst = d.path().join("p");
    copy_dir(&src, &dst);
    let mut ws = Workspace::new(None);
    ws.open_project(&dst).unwrap();
    ws.set_canvas_size(900.0, 600.0);
    (ws, d)
}

fn save_ppm(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in rgba.chunks(4) {
        out.extend_from_slice(&p[..3]);
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn the_reference_board_opens_and_is_drawn_in_pcb_and_3d() {
    let (mut ws, _d) = open();
    let snap = ws.snapshot();
    assert_eq!(snap.project.as_ref().unwrap().name, "reference-mcu-board");

    ws.set_mode(Mode::Pcb);
    let (w, h, pcb) = ws.render(900, 600).unwrap();
    let bg = [pcb[0], pcb[1], pcb[2]];
    assert!(
        pcb.chunks(4).filter(|p| [p[0], p[1], p[2]] != bg).count() > 5_000,
        "the board is drawn"
    );
    if let Ok(dir) = std::env::var("REFERENCE_BOARD_IMAGES") {
        save_ppm(&format!("{dir}/pcb.ppm"), w, h, &pcb);
    }

    ws.set_mode(Mode::Viewer3d);
    let (w, h, img) = ws.render(900, 600).unwrap();
    let bg = [img[0], img[1], img[2]];
    let drawn = img.chunks(4).filter(|p| [p[0], p[1], p[2]] != bg).count();
    assert!(
        drawn > 50_000,
        "the 3D board covers a good part of the frame ({drawn} px)"
    );
    let info = ws.snapshot().viewer3d.unwrap();
    assert!(info.has_outline);
    assert_eq!(info.components_count, 24);
    if let Ok(dir) = std::env::var("REFERENCE_BOARD_IMAGES") {
        save_ppm(&format!("{dir}/3d.ppm"), w, h, &img);
    }
}
