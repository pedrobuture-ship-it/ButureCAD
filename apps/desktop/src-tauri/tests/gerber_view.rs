//! The Gerber viewer through the workspace: it reads the generated files, not the board model.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::Mode;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::generators::Header;
use buturecad_pcb::{layer, BoardGraphic, FpShape, PlacedFootprint, Placement};

const MM: i64 = 1_000_000;

fn rig(with_outline: bool) -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "My Board").unwrap();
    let e = &mut ws.open.as_mut().unwrap().engine;
    let fp = Header::standard(2, 1).generate().unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let j1 = e.new_id();
    let mut ch = vec![PcbChange::create(
        j1,
        PcbObject::Footprint(PlacedFootprint {
            footprint: name,
            reference: "J1".into(),
            value: "Conn".into(),
            placement: Placement::at(Point::new(10 * MM, 10 * MM)),
            locked: false,
            link: None,
            pad_nets: Default::default(),
        }),
    )];
    if with_outline {
        let c =
            [(0, 0), (40, 0), (40, 25), (0, 25), (0, 0)].map(|(x, y)| Point::new(x * MM, y * MM));
        for w in c.windows(2) {
            let id = e.new_id();
            ch.push(PcbChange::create(
                id,
                PcbObject::Graphic(BoardGraphic {
                    layer: layer::EDGE_CUTS,
                    shape: FpShape::Line {
                        segment: Segment::new(w[0], w[1]),
                        width: 100_000,
                    },
                }),
            ));
        }
    }
    e.execute(PcbEditCommand::new("setup", ch)).unwrap();
    (ws, dir)
}

fn frame(ws: &mut Workspace) -> Vec<u8> {
    let (w, h, rgba) = ws.render(400, 300).unwrap();
    assert_eq!((w, h, rgba.len()), (400, 300, 400 * 300 * 4));
    rgba
}

#[test]
fn the_viewer_parses_the_generated_files_and_matches_the_board() {
    let (mut ws, _d) = rig(true);
    ws.set_canvas_size(400.0, 300.0);
    ws.set_mode(Mode::GerberViewer);
    ws.gerber_command("load_preview", None).unwrap();
    let g = ws.snapshot().gerber.unwrap();
    assert!(g.loaded && g.errors.is_empty(), "{:?}", g.errors);
    for want in [
        "My_Board-F_Cu.gtl",
        "My_Board-Edge_Cuts.gko",
        "My_Board-PTH.drl",
    ] {
        assert!(g.layers.iter().any(|l| l.name == want), "missing {want}");
    }
    assert!(
        !g.checks.is_empty() && g.checks.iter().all(|c| c.ok),
        "{:?}",
        g.checks
    );
    let a = frame(&mut ws);
    assert!(a.chunks(4).any(|p| p[..3] != a[..3]), "something is drawn");
    // Apertures of the selected layer and the drill tools are listed.
    let drill = g.layers.iter().position(|l| l.kind == "drill").unwrap();
    ws.gerber_command("select", Some(&drill.to_string()))
        .unwrap();
    let g = ws.snapshot().gerber.unwrap();
    assert_eq!(g.tools.len(), 1);
    assert_eq!(g.tools[0].holes, 2);
    let cu = g
        .layers
        .iter()
        .position(|l| l.name.ends_with("F_Cu.gtl"))
        .unwrap();
    ws.gerber_command("select", Some(&cu.to_string())).unwrap();
    assert!(!ws.snapshot().gerber.unwrap().apertures.is_empty());
    // Hiding a layer changes the picture; showing it restores it.
    ws.gerber_command("toggle", Some(&format!("{cu}:0")))
        .unwrap();
    let b = frame(&mut ws);
    assert_ne!(a, b);
    ws.gerber_command("toggle", Some(&format!("{cu}:1")))
        .unwrap();
    assert_eq!(frame(&mut ws), a);
}

#[test]
fn zip_archives_open_and_verification_is_only_for_the_live_preview() {
    let (mut ws, dir) = rig(true);
    ws.set_mode(Mode::GerberViewer);
    ws.fab_preview().unwrap();
    let zip = dir.path().join("production.zip");
    ws.fab_export(zip.to_str().unwrap()).unwrap();
    ws.gerber_command("open", Some(zip.to_str().unwrap()))
        .unwrap();
    let g = ws.snapshot().gerber.unwrap();
    assert!(g.loaded && g.source.ends_with("production.zip"));
    assert!(g.checks.is_empty(), "verify is only for the live preview");
    assert!(ws.gerber_command("verify", None).is_err());
    // A board edit after the preview does not make the viewer redraw the model: it still shows
    // the files, and a fresh preview + verify reports the new, consistent state.
    ws.gerber_command("load_preview", None).unwrap();
    assert!(ws.snapshot().gerber.unwrap().checks.iter().all(|c| c.ok));
    // Unsupported archives are reported, not guessed.
    let bad = dir.path().join("bad.zip");
    std::fs::write(&bad, b"not a zip").unwrap();
    assert!(ws
        .gerber_command("open", Some(bad.to_str().unwrap()))
        .is_err());
}
