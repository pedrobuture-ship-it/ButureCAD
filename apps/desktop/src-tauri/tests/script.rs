//! ButureScript in the editor: code → diagnostics → preview, and generation of a real project.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;

fn rig() -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_canvas_size(400.0, 300.0);
    ws.set_mode(Mode::Code);
    (ws, dir)
}

fn code(ws: &mut Workspace) -> CodeInfo {
    ws.snapshot().code.unwrap()
}

fn frame(ws: &mut Workspace) -> Vec<u8> {
    let (w, h, rgba) = ws.render(400, 300).unwrap();
    assert_eq!((w, h, rgba.len()), (400, 300, 400 * 300 * 4));
    rgba
}

fn drawn(img: &[u8]) -> bool {
    img.chunks(4).any(|p| p[..3] != img[..3])
}

#[test]
fn the_template_builds_a_preview_in_every_view() {
    let (mut ws, _d) = rig();
    let src = ws.script_source().unwrap();
    assert!(src.contains("project \"blinky\""));
    let c = code(&mut ws);
    assert!(
        c.has_preview && c.diagnostics.iter().all(|d| d.severity != "error"),
        "{:?}",
        c.diagnostics
    );
    assert!(c.summary.contains("6 components"), "{}", c.summary);
    for p in ["pcb", "schematic", "3d"] {
        ws.script_command("preview", Some(p)).unwrap();
        assert!(drawn(&frame(&mut ws)), "{p} preview is drawn");
    }
    // The user's own project was not touched.
    assert_eq!(
        ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .placed
            .len(),
        0
    );
}

#[test]
fn errors_have_positions_and_keep_the_last_good_preview() {
    let (mut ws, _d) = rig();
    ws.script_source().unwrap();
    ws.script_command("preview", Some("pcb")).unwrap();
    let before = frame(&mut ws);
    let bad = "component C1 { value = \"1\" footprint = \"0402\" }\nconnect C1.1 -> C9.1\nboard { outline = rect(0mm,0mm,10mm + 5V,5mm) }\n";
    ws.script_command("update", Some(bad)).unwrap();
    let c = code(&mut ws);
    let errs: Vec<_> = c
        .diagnostics
        .iter()
        .filter(|d| d.severity == "error")
        .collect();
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert_eq!((errs[0].line, errs[0].col), (2, 17));
    assert!(errs[1].message.contains("incompatible dimensions"));
    assert_eq!(errs[1].line, 3);
    assert!(c.has_preview, "the last good preview stays");
    assert_eq!(frame(&mut ws), before);
    // A change that builds updates the preview.
    ws.script_command("update", Some("board { outline = rect(0mm,0mm,10mm,5mm) }\ncomponent R1 { value=\"1k\" footprint=\"1206\" }\nplace R1 at (5mm, 2mm)")).unwrap();
    let c = code(&mut ws);
    assert!(c.diagnostics.iter().all(|d| d.severity != "error"));
    assert_ne!(frame(&mut ws), before);
}

#[test]
fn the_preview_is_read_only_but_can_be_panned_and_zoomed() {
    let (mut ws, _d) = rig();
    ws.script_source().unwrap();
    ws.script_command("preview", Some("pcb")).unwrap();
    let a = frame(&mut ws);
    let ev = |kind, x, y, button, delta| PointerEvent {
        kind,
        x,
        y,
        button,
        shift: false,
        ctrl: false,
        alt: false,
        delta,
    };
    ws.pointer(ev(PointerKind::Wheel, 200.0, 150.0, 0, -300.0));
    let zoomed = frame(&mut ws);
    assert_ne!(zoomed, a);
    ws.pointer(ev(PointerKind::Down, 200.0, 150.0, 0, 0.0));
    ws.pointer(ev(PointerKind::Move, 260.0, 190.0, 0, 0.0));
    ws.pointer(ev(PointerKind::Up, 260.0, 190.0, 0, 0.0));
    assert_ne!(frame(&mut ws), zoomed, "a left drag pans");
    // Nothing was edited anywhere.
    let pw_changed = ws.snapshot().code.unwrap().generation;
    assert_eq!(pw_changed, 1);
    // The view survives a rebuild (typing does not reset the zoom).
    let before = frame(&mut ws);
    let src = ws.script_source().unwrap();
    ws.script_command("update", Some(&(src + "\n// edit\n")))
        .unwrap();
    assert_eq!(frame(&mut ws), before);
}

#[test]
fn save_view_switches_and_generating_a_real_project() {
    let (mut ws, d) = rig();
    let src = ws.script_source().unwrap();
    assert!(code(&mut ws).dirty || !code(&mut ws).saved_file);
    ws.script_command("save", Some(&src)).unwrap();
    let c = code(&mut ws);
    assert!(c.saved_file && !c.dirty);
    assert_eq!(
        std::fs::read_to_string(d.path().join("P/design.beda")).unwrap(),
        src
    );
    ws.script_command("update", Some(&(src.clone() + "\n")))
        .unwrap();
    assert!(code(&mut ws).dirty);
    for v in ["code", "visual", "split"] {
        ws.script_command("view", Some(v)).unwrap();
        assert_eq!(code(&mut ws).view, v);
    }
    assert!(ws.script_command("view", Some("zoomed")).is_err());
    // Generate a real project (the current one is clean so it can be replaced).
    let out = d.path().join("Generated");
    ws.script_command("generate", Some(&format!("{}|{src}", out.display())))
        .unwrap();
    let p = ws.open.as_ref().unwrap();
    assert_eq!(p.engine.project().name(), "blinky");
    assert_eq!(p.engine.project().pcb().placed.len(), 6);
    assert!(out.join("design.beda").exists());
    // Broken scripts never create projects.
    assert!(ws
        .script_command(
            "generate",
            Some(&format!(
                "{}|{}",
                d.path().join("X").display(),
                "connect A -> "
            ))
        )
        .is_err());
}
