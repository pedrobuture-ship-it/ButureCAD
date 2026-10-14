//! Manufacturing export through the workspace: preview, preflight blocking and production.zip.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject};
use buturecad_geometry::{Point, Segment};
use buturecad_manufacturing::zip::read_stored;
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

#[test]
fn production_zip_contains_the_real_board() {
    let (mut ws, dir) = rig(true);
    ws.fab_preview().unwrap();
    let info = ws.snapshot().fab.unwrap();
    assert!(!info.blocked, "{:?}", info.issues);
    assert!(info.files.iter().any(|f| f.name == "My_Board-F_Cu.gtl"));
    let out = dir.path().join("production.zip");
    ws.fab_export(out.to_str().unwrap()).unwrap();
    let files = read_stored(&std::fs::read(&out).unwrap()).unwrap();
    let get =
        |n: &str| String::from_utf8(files.iter().find(|(x, _)| x == n).unwrap().1.clone()).unwrap();
    // Two through-hole pads on each outer copper layer (round pads are flashes, rectangular ones
    // are regions), two plated holes.
    let pads = |g: &str| g.matches("D03*").count() + g.matches("G36*").count();
    assert_eq!(pads(&get("My_Board-F_Cu.gtl")), 2);
    assert_eq!(pads(&get("My_Board-B_Cu.gbl")), 2);
    let pth = get("My_Board-PTH.drl");
    assert!(
        pth.contains("METRIC") && pth.matches("\nX").count() == 2,
        "{pth}"
    );
    // The outline: 4 strokes of the profile.
    assert_eq!(get("My_Board-Edge_Cuts.gko").matches("D01*").count(), 4);
    assert!(get("My_Board-bom.csv").contains("J1,1,Conn,"));
    assert!(get("PREFLIGHT.txt").contains("not fabrication sign-off"));
    // Same board, same bytes.
    let out2 = dir.path().join("again.zip");
    ws.fab_export(out2.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&out2).unwrap());
}

#[test]
fn a_board_with_errors_is_previewed_but_not_exported() {
    let (mut ws, dir) = rig(false);
    ws.fab_preview().unwrap();
    let info = ws.snapshot().fab.unwrap();
    assert!(info.blocked && info.errors > 0);
    assert!(info.issues.iter().any(|i| i.code == "outline"));
    let out = dir.path().join("production.zip");
    let err = ws.fab_export(out.to_str().unwrap()).unwrap_err();
    assert!(err.contains("blocked"), "{err}");
    assert!(!out.exists(), "nothing is written when blocked");
}
