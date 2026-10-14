//! Differential pair routing through the workspace, like the UI drives it.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{PcbChange, PcbObject, Via, ViaKind};
use buturecad_geometry::Point;
use buturecad_pcb::generators::Header;
use buturecad_pcb::{layer, Net, PlacedFootprint, Placement};

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
    dp: buturecad_pcb::Id,
    dn: buturecad_pcb::Id,
    _dir: tempfile::TempDir,
}

/// Two 1×2 headers 30 mm apart: J1 pads 1/2 are the pair D+ / D−, J2 the same.
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
    let (dp, dn, j1, j2) = (e.new_id(), e.new_id(), e.new_id(), e.new_id());
    let place = |x: i64, r: &str| PlacedFootprint {
        footprint: name.clone(),
        reference: r.into(),
        value: "x".into(),
        placement: Placement::at(Point::new(x * MM, 0)),
        locked: false,
        link: None,
        pad_nets: [("1".to_string(), dp), ("2".to_string(), dn)]
            .into_iter()
            .collect(),
    };
    e.execute(PcbEditCommand::new(
        "setup",
        vec![
            PcbChange::create(
                dp,
                PcbObject::Net(Net {
                    name: "USB_D+".into(),
                }),
            ),
            PcbChange::create(
                dn,
                PcbObject::Net(Net {
                    name: "USB_D-".into(),
                }),
            ),
            PcbChange::create(j1, PcbObject::Footprint(place(0, "J1"))),
            PcbChange::create(j2, PcbObject::Footprint(place(30, "J2"))),
        ],
    ))
    .unwrap();
    let mut rules = ws.pcb_rules_info().unwrap();
    assert_eq!(
        rules.suggested_pairs.len(),
        1,
        "USB_D+ / USB_D- is recognised"
    );
    let mut pair = rules.suggested_pairs[0].clone();
    pair.width = "0.25".into();
    pair.gap = "0.4".into();
    pair.max_skew = "0.5".into();
    rules.pairs.push(pair);
    ws.pcb_rules_apply(rules).unwrap();
    ws.editor_command("tool:route_pair").unwrap();
    ws.fit_view();
    let v = ws.views.get_mut(&Mode::Pcb).unwrap();
    v.center = (15.0 * MM as f64, -1.27 * MM as f64);
    v.scale = 25.0 / MM as f64;
    Rig {
        ws,
        dp,
        dn,
        _dir: dir,
    }
}

impl Rig {
    fn pad(&self, r: &str, n: &str) -> Point {
        let doc = self.ws.open.as_ref().unwrap().engine.project().pcb();
        let f = doc.placed.values().find(|f| f.reference == r).unwrap();
        f.world_pads(&doc.footprints[&f.footprint])
            .into_iter()
            .find(|p| p.number == n)
            .unwrap()
            .center
    }
    fn at(&self, p: Point) -> (f64, f64) {
        self.ws.views[&Mode::Pcb].world_to_screen(p)
    }
    fn click(&mut self, p: Point) {
        let (x, y) = self.at(p);
        self.ws.pointer(ev(PointerKind::Move, x, y));
        self.ws.pointer(ev(PointerKind::Down, x, y));
        self.ws.pointer(ev(PointerKind::Up, x, y));
    }
    fn hover(&mut self, p: Point) {
        let (x, y) = self.at(p);
        self.ws.pointer(ev(PointerKind::Move, x, y));
    }
    fn tracks_of(&self, net: buturecad_pcb::Id) -> Vec<buturecad_eda_core::model::Track> {
        let doc = self.ws.open.as_ref().unwrap().engine.project().pcb();
        doc.tracks
            .values()
            .filter(|t| t.net == Some(net))
            .cloned()
            .collect()
    }
}

#[test]
fn a_straight_pair_is_routed_on_both_nets_with_live_numbers_and_one_undo_step() {
    let mut r = rig();
    let (a1, a2) = (r.pad("J1", "1"), r.pad("J1", "2"));
    let (b1, b2) = (r.pad("J2", "1"), r.pad("J2", "2"));
    r.click(a1);
    let info = r.ws.snapshot().pcb.unwrap();
    assert!(info.route.is_some(), "the pair route started on a pad");
    r.hover(b1);
    let live = r.ws.snapshot().pcb.unwrap().route.unwrap().pair.unwrap();
    assert_eq!(live.name, "USB_D");
    assert_eq!(live.gap_target_mm, "0.4");
    assert_eq!(live.gap_mm, "0.4", "coupled part keeps the requested gap");
    assert!(!live.over_skew, "{live:?}");
    r.click(b1); // reaching the pair's pads finishes it

    let (tp, tn) = (r.tracks_of(r.dp), r.tracks_of(r.dn));
    assert!(!tp.is_empty() && !tn.is_empty(), "both nets got copper");
    assert!(tp
        .iter()
        .chain(&tn)
        .all(|t| t.width == 250_000 && t.layer == layer::F_CU));
    for (t, pad_a, pad_b) in [(&tp, a1, b1), (&tn, a2, b2)] {
        assert!(
            t.iter()
                .any(|t| t.segment.a == pad_a || t.segment.b == pad_a),
            "starts on its pad"
        );
        assert!(
            t.iter()
                .any(|t| t.segment.a == pad_b || t.segment.b == pad_b),
            "ends on its pad"
        );
    }
    r.ws.drc_run().unwrap();
    let drc = r.ws.snapshot().drc.unwrap();
    let bad: Vec<_> = drc
        .violations
        .iter()
        .map(|v| format!("{}: {}", v.rule, v.message))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    let history = r.ws.snapshot().project.unwrap().undo_history;
    assert_eq!(
        history
            .iter()
            .filter(|h| h.as_str() == "Route differential pair")
            .count(),
        1
    );
    r.ws.undo().unwrap();
    assert!(r.tracks_of(r.dp).is_empty() && r.tracks_of(r.dn).is_empty());
}

#[test]
fn corners_create_skew_that_is_reported_live_and_by_drc() {
    let mut r = rig();
    r.ws.pcb_command("route_mode", Some("90")).unwrap();
    let a1 = r.pad("J1", "1");
    let mid = Point::new(a1.x, a1.y - 1_270_000);
    r.click(a1);
    r.click(Point::new(mid.x + 12 * MM, mid.y));
    r.hover(Point::new(mid.x + 12 * MM, mid.y + 8 * MM));
    let live = r.ws.snapshot().pcb.unwrap().route.unwrap().pair.unwrap();
    assert!(live.over_skew, "one 90° corner exceeds 0.5 mm: {live:?}");
    r.click(Point::new(mid.x + 12 * MM, mid.y + 8 * MM));
    r.ws.pcb_command("route_finish", None).unwrap();
    assert!(!r.tracks_of(r.dp).is_empty());
    r.ws.drc_run().unwrap();
    let drc = r.ws.snapshot().drc.unwrap();
    assert!(
        drc.violations
            .iter()
            .any(|v| v.rule == "pair_skew" && v.severity == "warning"),
        "{:?}",
        drc.violations
    );
    assert!(
        !drc.violations
            .iter()
            .any(|v| ["clearance", "short"].contains(&v.rule.as_str())),
        "the legs themselves keep their gap: {:?}",
        drc.violations
    );
}

#[test]
fn walk_around_takes_the_whole_pair_around_an_obstacle() {
    let mut r = rig();
    let a1 = r.pad("J1", "1");
    let y = a1.y - 1_270_000;
    // A via of no net on the centre line, between the headers.
    let e = &mut r.ws.open.as_mut().unwrap().engine;
    let id = e.new_id();
    e.execute(PcbEditCommand::new(
        "via",
        vec![PcbChange::create(
            id,
            PcbObject::Via(Via {
                position: Point::new(15 * MM, y),
                diameter: 800_000,
                drill: 400_000,
                net: None,
                kind: ViaKind::Through,
            }),
        )],
    ))
    .unwrap();
    let b1 = r.pad("J2", "1");
    r.click(a1);
    r.hover(b1);
    let route = r.ws.snapshot().pcb.unwrap().route.unwrap();
    assert!(route.problem.is_none(), "{route:?}");
    r.click(b1);
    assert!(
        r.tracks_of(r.dp).len() > 3 && r.tracks_of(r.dn).len() > 3,
        "both legs detour around the via"
    );
    r.ws.drc_run().unwrap();
    let bad: Vec<_> =
        r.ws.snapshot()
            .drc
            .unwrap()
            .violations
            .into_iter()
            .filter(|v| ["clearance", "short", "copper_to_edge"].contains(&v.rule.as_str()))
            .map(|v| v.message)
            .collect();
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn only_pair_nets_start_a_pair_and_rules_validate() {
    let mut r = rig();
    // Not on a pad: refused, no route.
    r.click(Point::new(15 * MM, 15 * MM));
    assert!(r.ws.snapshot().pcb.unwrap().route.is_none());

    let mut rules = r.ws.pcb_rules_info().unwrap();
    // The same net twice, a net in two pairs, and an unknown net are refused as a whole.
    let mut p = rules.pairs[0].clone();
    p.negative = p.positive;
    let mut bad = rules.clone();
    bad.pairs = vec![p];
    assert!(r.ws.pcb_rules_apply(bad).is_err());
    let mut dup = rules.pairs[0].clone();
    dup.name = "Other".into();
    let mut bad = rules.clone();
    bad.pairs.push(dup);
    assert!(r.ws.pcb_rules_apply(bad).is_err());
    rules.pairs[0].positive = 9999;
    assert!(r.ws.pcb_rules_apply(rules).is_err());
    // The good pair survives and one undo removes the definition.
    assert_eq!(r.ws.pcb_rules_info().unwrap().pairs.len(), 1);
    r.ws.undo().unwrap();
    assert!(r.ws.pcb_rules_info().unwrap().pairs.is_empty());
}
