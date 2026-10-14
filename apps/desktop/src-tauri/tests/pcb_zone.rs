//! Copper zones through the workspace, like the UI drives them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::generators::Header;
use buturecad_pcb::{layer, BoardGraphic, FpShape, Net, PlacedFootprint, Placement};

const G: i64 = 1_270_000;
const MM: i64 = 1_000_000;

fn ev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        button: 0,
        shift: false,
        ctrl: false,
        alt: false,
        delta: 0.0,
    }
}

struct Rig {
    ws: Workspace,
    gnd: buturecad_pcb::Id,
    sig: buturecad_pcb::Id,
    _dir: tempfile::TempDir,
}

/// A 32 × 24 grid-step board with a 1×2 header J1 whose pad 1 is GND and pad 2 is SIG.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.set_mode(Mode::Pcb);
    let e = &mut ws.open.as_mut().unwrap().engine;
    let fp = Header::standard(2, 1).generate().unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let (gnd, sig, j1) = (e.new_id(), e.new_id(), e.new_id());
    let mut ch = vec![
        PcbChange::create(gnd, PcbObject::Net(Net { name: "GND".into() })),
        PcbChange::create(sig, PcbObject::Net(Net { name: "SIG".into() })),
        PcbChange::create(
            j1,
            PcbObject::Footprint(PlacedFootprint {
                footprint: name,
                reference: "J1".into(),
                value: "x".into(),
                placement: Placement::at(Point::new(10 * G, 10 * G)),
                locked: false,
                link: None,
                pad_nets: [("1".to_string(), gnd), ("2".to_string(), sig)]
                    .into_iter()
                    .collect(),
            }),
        ),
    ];
    let c = [(0, 0), (32, 0), (32, 24), (0, 24), (0, 0)].map(|(x, y)| Point::new(x * G, y * G));
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
    ws.fit_view();
    let v = ws.views.get_mut(&Mode::Pcb).unwrap();
    v.center = (16.0 * G as f64, 12.0 * G as f64);
    v.scale = 20.0 / MM as f64;
    Rig {
        ws,
        gnd,
        sig,
        _dir: dir,
    }
}

impl Rig {
    fn click(&mut self, p: Point) {
        let (x, y) = self.ws.views[&Mode::Pcb].world_to_screen(p);
        self.ws.pointer(ev(PointerKind::Move, x, y));
        self.ws.pointer(ev(PointerKind::Down, x, y));
        self.ws.pointer(ev(PointerKind::Up, x, y));
    }
    fn zones(&self) -> Vec<buturecad_pcb::Zone> {
        self.ws
            .open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .zones
            .values()
            .cloned()
            .collect()
    }
    fn draw_zone(&mut self) {
        self.ws
            .pcb_command("zone_net", Some(&self.gnd.0.to_string()))
            .unwrap();
        self.ws.editor_command("tool:zone").unwrap();
        for (x, y) in [(1, 1), (31, 1), (31, 23), (1, 23)] {
            self.click(Point::new(x * G, y * G));
        }
        self.click(Point::new(G, G)); // back to the first corner: closes
    }
}

#[test]
fn drawing_a_zone_creates_and_fills_it_in_one_undo_step() {
    let mut r = rig();
    assert!(r.zones().is_empty());
    r.draw_zone();
    let zones = r.zones();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].net, Some(r.gnd));
    let fill = zones[0].fill.as_ref().expect("filled right away");
    assert!(!fill.regions.is_empty(), "the GND pad keeps the pour alive");
    let snap = r.ws.snapshot();
    assert_eq!(snap.pcb.unwrap().stale_zones, 0);
    let history = snap.project.unwrap().undo_history;
    assert_eq!(
        history.iter().filter(|h| h.as_str() == "Add zone").count(),
        1
    );
    r.ws.undo().unwrap();
    assert!(r.zones().is_empty(), "creating and filling are one step");
}

#[test]
fn edits_near_the_pour_refill_it_in_the_same_step_and_drc_stays_clean() {
    let mut r = rig();
    r.draw_zone();
    let before = r.zones()[0].fill.clone();
    // Route a SIG track across the pour with the Route tool.
    r.ws.editor_command("tool:route").unwrap();
    r.ws.pcb_command("route_obstacles", Some("ignore")).unwrap();
    r.click(Point::new(20 * G, 3 * G));
    r.click(Point::new(20 * G, 20 * G));
    r.ws.pcb_command("route_finish", None).unwrap();
    let after = r.zones()[0].fill.clone();
    assert_ne!(after, before, "the pour was refilled around the new track");
    assert_eq!(r.ws.snapshot().pcb.unwrap().stale_zones, 0);
    r.ws.drc_run().unwrap();
    let drc = r.ws.snapshot().drc.unwrap();
    let bad: Vec<_> = drc
        .violations
        .iter()
        .filter(|v| {
            ["clearance", "short", "zone_stale", "copper_to_edge"].contains(&v.rule.as_str())
        })
        .map(|v| format!("{}: {}", v.rule, v.message))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    // The track and the refill are one undo step.
    r.ws.undo().unwrap();
    assert_eq!(r.zones()[0].fill, before);
    let doc = r.ws.open.as_ref().unwrap().engine.project().pcb();
    assert!(doc.tracks.is_empty());
}

#[test]
fn without_auto_fill_the_zone_goes_stale_and_the_command_refills_it() {
    let mut r = rig();
    r.draw_zone();
    r.ws.pcb_command("auto_fill", None).unwrap();
    assert!(!r.ws.snapshot().pcb.unwrap().auto_fill);
    r.ws.editor_command("tool:route").unwrap();
    r.ws.pcb_command("route_obstacles", Some("ignore")).unwrap();
    r.click(Point::new(20 * G, 3 * G));
    r.click(Point::new(20 * G, 20 * G));
    r.ws.pcb_command("route_finish", None).unwrap();
    assert_eq!(r.ws.snapshot().pcb.unwrap().stale_zones, 1);
    r.ws.drc_run().unwrap();
    assert!(r
        .ws
        .snapshot()
        .drc
        .unwrap()
        .violations
        .iter()
        .any(|v| v.rule == "zone_stale"));
    r.ws.pcb_command("fill_zones", None).unwrap();
    assert_eq!(r.ws.snapshot().pcb.unwrap().stale_zones, 0);
    // Clearing removes the copper (and the ratsnest sees it).
    r.ws.pcb_command("unfill_zones", None).unwrap();
    assert!(r.zones()[0].fill.is_none());
    assert_eq!(r.ws.snapshot().pcb.unwrap().stale_zones, 1);
}

#[test]
fn zone_properties_are_editable_and_change_the_fill() {
    let mut r = rig();
    r.draw_zone();
    let id =
        r.ws.open
            .as_ref()
            .unwrap()
            .engine
            .project()
            .pcb()
            .zones
            .keys()
            .next()
            .unwrap()
            .0;
    let solid_before = r.zones()[0].fill.clone();
    r.ws.set_property(id, "connection", "solid").unwrap();
    assert_eq!(r.zones()[0].connection, buturecad_pcb::PadConnection::Solid);
    assert_ne!(
        r.zones()[0].fill,
        solid_before,
        "thermal → solid changes the copper around the pad"
    );
    r.ws.set_property(id, "clearance_mm", "0.5").unwrap();
    assert_eq!(r.zones()[0].clearance, Some(500_000));
    r.ws.set_property(id, "priority", "3").unwrap();
    assert_eq!(r.zones()[0].priority, 3);
    assert!(
        r.ws.set_property(id, "min_width_mm", "0").is_err(),
        "validation refuses a zero width"
    );
}

#[test]
fn a_zone_connects_its_pads_in_the_ratsnest_after_filling() {
    let mut r = rig();
    // A second GND pad far away: unrouted until the pour joins them.
    let e = &mut r.ws.open.as_mut().unwrap().engine;
    let (gnd, via) = (r.gnd, e.new_id());
    e.execute(PcbEditCommand::new(
        "via",
        vec![PcbChange::create(
            via,
            PcbObject::Via(buturecad_eda_core::model::Via {
                position: Point::new(25 * G, 18 * G),
                diameter: 800_000,
                drill: 400_000,
                net: Some(gnd),
                kind: buturecad_eda_core::model::ViaKind::Through,
            }),
        )],
    ))
    .unwrap();
    assert_eq!(r.ws.snapshot().pcb.unwrap().unrouted, 1);
    r.draw_zone();
    assert_eq!(
        r.ws.snapshot().pcb.unwrap().unrouted,
        0,
        "the pour connects the pad and the via"
    );
    let _ = r.sig;
}
