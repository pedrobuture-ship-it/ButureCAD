//! The 3D viewer through the workspace: the board is drawn, edits show up, the camera works.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::generators::Header;
use buturecad_pcb::{layer, BoardGraphic, FpShape, Net, PlacedFootprint, Placement};

const MM: i64 = 1_000_000;
const BG: [u8; 3] = [11, 14, 20];

fn ev(kind: PointerKind, x: f64, y: f64, button: i32, delta: f64) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        button,
        shift: false,
        ctrl: false,
        alt: false,
        delta,
    }
}

fn rig() -> (Workspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_canvas_size(400.0, 300.0);
    let e = &mut ws.open.as_mut().unwrap().engine;
    let fp = Header::standard(2, 1).generate().unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let (net, j1) = (e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(net, PcbObject::Net(Net { name: "N".into() })),
        PcbChange::create(
            j1,
            PcbObject::Footprint(PlacedFootprint {
                footprint: name,
                reference: "J1".into(),
                value: "x".into(),
                placement: Placement::at(Point::new(10 * MM, 10 * MM)),
                locked: false,
                link: None,
                pad_nets: Default::default(),
            }),
        ),
    ];
    let c = [(0, 0), (40, 0), (40, 25), (0, 25), (0, 0)].map(|(x, y)| Point::new(x * MM, y * MM));
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
    e.execute(PcbEditCommand::new("setup", ch)).unwrap();
    ws.set_mode(Mode::Viewer3d);
    (ws, dir)
}

fn frame(ws: &mut Workspace) -> Vec<u8> {
    let (w, h, rgba) = ws.render(400, 300).unwrap();
    assert_eq!((w, h, rgba.len()), (400, 300, 400 * 300 * 4));
    rgba
}

fn px(img: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * 400 + x) * 4;
    [img[i], img[i + 1], img[i + 2]]
}

#[test]
fn the_board_is_rendered_in_3d_and_pcb_edits_show_up() {
    let (mut ws, _d) = rig();
    let a = frame(&mut ws);
    assert!(
        a.chunks(4).any(|p| [p[0], p[1], p[2]] != BG),
        "the board is visible"
    );
    assert_eq!(px(&a, 1, 1), BG, "background outside the board");
    let info = ws.snapshot().viewer3d.unwrap();
    assert!(info.has_outline && info.components_count == 1 && info.projection == "perspective");

    // A track added to the board changes the 3D image; undo restores it exactly.
    let e = &mut ws.open.as_mut().unwrap().engine;
    let (net, t) = (
        e.project().pcb().nets.keys().next().copied().unwrap(),
        e.new_id(),
    );
    e.execute(PcbEditCommand::new(
        "track",
        vec![PcbChange::create(
            t,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(Point::new(3 * MM, 20 * MM), Point::new(35 * MM, 20 * MM)),
                width: 1_500_000,
                net: Some(net),
            }),
        )],
    ))
    .unwrap();
    let b = frame(&mut ws);
    assert_ne!(a, b, "the new track is in the model");
    ws.undo().unwrap();
    assert_eq!(frame(&mut ws), a, "undo removes it from the 3D view too");
}

#[test]
fn camera_controls_projection_views_and_layer_toggles_change_the_picture() {
    let (mut ws, _d) = rig();
    let base = frame(&mut ws);
    // Orbit with a left drag.
    ws.pointer(ev(PointerKind::Down, 200.0, 150.0, 0, 0.0));
    ws.pointer(ev(PointerKind::Move, 260.0, 120.0, 0, 0.0));
    ws.pointer(ev(PointerKind::Up, 260.0, 120.0, 0, 0.0));
    let orbited = frame(&mut ws);
    assert_ne!(orbited, base);
    // Wheel zoom and right-drag pan.
    ws.pointer(ev(PointerKind::Wheel, 200.0, 150.0, 0, -300.0));
    let zoomed = frame(&mut ws);
    assert_ne!(zoomed, orbited);
    ws.pointer(ev(PointerKind::Down, 200.0, 150.0, 2, 0.0));
    ws.pointer(ev(PointerKind::Move, 230.0, 160.0, 2, 0.0));
    ws.pointer(ev(PointerKind::Up, 230.0, 160.0, 2, 0.0));
    assert_ne!(frame(&mut ws), zoomed);
    // Projection and preset views.
    ws.viewer3d_command("view", Some("top")).unwrap();
    let persp = frame(&mut ws);
    ws.viewer3d_command("projection", Some("orthographic"))
        .unwrap();
    assert_eq!(ws.snapshot().viewer3d.unwrap().projection, "orthographic");
    assert_ne!(frame(&mut ws), persp);
    ws.viewer3d_command("view", Some("bottom")).unwrap();
    ws.viewer3d_command("view", Some("iso")).unwrap();
    assert!(ws.viewer3d_command("view", Some("sideways")).is_err());
    assert!(ws.viewer3d_command("projection", Some("fisheye")).is_err());
    // Toggling the components removes their pixels.
    ws.viewer3d_command("frame", None).unwrap();
    let with = frame(&mut ws);
    ws.viewer3d_command("show", Some("components:0")).unwrap();
    assert!(!ws.snapshot().viewer3d.unwrap().components);
    assert_ne!(frame(&mut ws), with);
    assert!(ws.viewer3d_command("show", Some("magic:1")).is_err());
}
