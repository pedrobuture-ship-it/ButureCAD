//! Length tuning through the workspace, like the UI drives it.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::length::net_length;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, Net};

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
    a: buturecad_pcb::Id,
    b: buturecad_pcb::Id,
    ta: buturecad_pcb::Id,
    _dir: tempfile::TempDir,
}

/// Net A: one 40 mm track at y = 0. Net B: one 45 mm track at y = 12 mm.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.path().join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.set_mode(Mode::Pcb);
    let e = &mut ws.open.as_mut().unwrap().engine;
    let (a, b, ta, tb) = (e.new_id(), e.new_id(), e.new_id(), e.new_id());
    let track = |net, y: i64, len: i64| {
        PcbObject::Track(Track {
            layer: layer::F_CU,
            segment: Segment::new(Point::new(0, y * MM), Point::new(len * MM, y * MM)),
            width: 250_000,
            net: Some(net),
        })
    };
    e.execute(PcbEditCommand::new(
        "setup",
        vec![
            PcbChange::create(a, PcbObject::Net(Net { name: "A".into() })),
            PcbChange::create(b, PcbObject::Net(Net { name: "B".into() })),
            PcbChange::create(ta, track(a, 0, 40)),
            PcbChange::create(tb, track(b, 12, 45)),
        ],
    ))
    .unwrap();
    ws.fit_view();
    let v = ws.views.get_mut(&Mode::Pcb).unwrap();
    v.center = (22.0 * MM as f64, 6.0 * MM as f64);
    v.scale = 15.0 / MM as f64;
    Rig {
        ws,
        a,
        b,
        ta,
        _dir: dir,
    }
}

impl Rig {
    fn at(&self, p: Point) -> (f64, f64) {
        self.ws.views[&Mode::Pcb].world_to_screen(p)
    }
    fn hover(&mut self, p: Point) {
        let (x, y) = self.at(p);
        self.ws.pointer(ev(PointerKind::Move, x, y));
    }
    fn click(&mut self, p: Point) {
        self.hover(p);
        let (x, y) = self.at(p);
        self.ws.pointer(ev(PointerKind::Down, x, y));
        self.ws.pointer(ev(PointerKind::Up, x, y));
    }
    fn select(&mut self, id: buturecad_pcb::Id) {
        self.ws
            .selection
            .insert(Mode::Pcb, [id].into_iter().collect());
    }
    fn len(&self, net: buturecad_pcb::Id) -> f64 {
        net_length(self.ws.open.as_ref().unwrap().engine.project().pcb(), net)
    }
    fn info(&mut self) -> Option<LengthInfo> {
        self.ws.snapshot().pcb.unwrap().length
    }
}

#[test]
fn the_bar_shows_current_target_and_difference_for_the_selected_net() {
    let mut r = rig();
    r.select(r.ta);
    let info = r.info().unwrap();
    assert_eq!((info.net.as_str(), info.current_mm.as_str()), ("A", "40"));
    assert!(info.target_mm.is_none(), "no target yet");
    r.ws.pcb_command("length_target", Some("50/0.1")).unwrap();
    let info = r.info().unwrap();
    assert_eq!(info.target_mm.as_deref(), Some("50"));
    assert_eq!(info.tolerance_mm.as_deref(), Some("0.1"));
    assert_eq!(info.difference_mm.as_deref(), Some("-10"));
    assert!(!info.within && info.has_target);
    // DRC reports the missed target as a warning with a real object.
    r.ws.drc_run().unwrap();
    let drc = r.ws.snapshot().drc.unwrap();
    let v = drc
        .violations
        .iter()
        .find(|v| v.rule == "length_target")
        .unwrap();
    assert_eq!(v.severity, "warning");
    // One undo step removes the target again.
    r.ws.undo().unwrap();
    assert!(r.info().unwrap().target_mm.is_none());
}

#[test]
fn the_tuner_previews_live_and_a_click_reaches_the_target_in_one_undo_step() {
    for style in ["serpentine", "accordion"] {
        let mut r = rig();
        r.select(r.ta);
        r.ws.pcb_command("length_target", Some("50/0.1")).unwrap();
        r.ws.pcb_command("tune_style", Some(style)).unwrap();
        r.ws.editor_command("tool:tune").unwrap();
        r.hover(Point::new(20 * MM, 0));
        let info = r.info().unwrap();
        assert_eq!(info.style, style);
        assert!(info.problem.is_none(), "{info:?}");
        let predicted: f64 = info.predicted_mm.as_deref().unwrap().parse().unwrap();
        assert!(
            (predicted - 50.0).abs() < 0.01,
            "{style}: predicted {predicted}"
        );
        r.click(Point::new(20 * MM, 0));
        let len = r.len(r.a);
        assert!((len - 50.0e6).abs() <= 0.1e6, "{style}: {len}");
        r.ws.drc_run().unwrap();
        let drc = r.ws.snapshot().drc.unwrap();
        let bad: Vec<_> = drc
            .violations
            .iter()
            .map(|v| format!("{}: {}", v.rule, v.message))
            .collect();
        assert!(bad.is_empty(), "{style}: {bad:?}");
        let history = r.ws.snapshot().project.unwrap().undo_history;
        assert_eq!(
            history
                .iter()
                .filter(|h| h.as_str() == "Tune length")
                .count(),
            1
        );
        r.ws.undo().unwrap();
        assert!(
            (r.len(r.a) - 40.0e6).abs() < 1.0,
            "one undo restores the straight track"
        );
    }
}

#[test]
fn a_pair_leg_without_a_target_aims_at_its_partner() {
    let mut r = rig();
    let mut rules = r.ws.pcb_rules_info().unwrap();
    rules.pairs.push(PairInfo {
        name: "P".into(),
        positive: r.a.0,
        negative: r.b.0,
        width: String::new(),
        gap: String::new(),
        max_skew: "0.2".into(),
    });
    r.ws.pcb_rules_apply(rules).unwrap();
    r.select(r.ta);
    let info = r.info().unwrap();
    assert!(info.from_pair && !info.has_target);
    assert_eq!(info.target_mm.as_deref(), Some("45"));
    r.ws.editor_command("tune_selected").unwrap();
    let len = r.len(r.a);
    assert!((len - 45.0e6).abs() <= 0.2e6, "{len}");
}

#[test]
fn a_blocked_meander_is_refused_with_the_reason_and_nothing_changes() {
    let mut r = rig();
    r.select(r.ta);
    r.ws.pcb_command("length_target", Some("50/0.1")).unwrap();
    // Copper of another net 1 mm above and below the track blocks both sides.
    let e = &mut r.ws.open.as_mut().unwrap().engine;
    let ch: Vec<PcbChange> = [1_i64, -1]
        .into_iter()
        .map(|s| {
            let id = e.new_id();
            PcbChange::create(
                id,
                PcbObject::Track(Track {
                    layer: layer::F_CU,
                    segment: Segment::new(Point::new(0, s * MM), Point::new(40 * MM, s * MM)),
                    width: 250_000,
                    net: None,
                }),
            )
        })
        .collect();
    e.execute(PcbEditCommand::new("blockers", ch)).unwrap();
    r.ws.editor_command("tool:tune").unwrap();
    r.hover(Point::new(20 * MM, 0));
    let info = r.info().unwrap();
    assert!(
        info.problem.as_deref().is_some_and(|p| p.contains("rule")),
        "{info:?}"
    );
    assert!(info.predicted_mm.is_none());
    let before = r.len(r.a);
    r.click(Point::new(20 * MM, 0));
    assert_eq!(r.len(r.a), before, "nothing was placed");
}

#[test]
fn a_target_below_the_current_length_cannot_be_reached_by_adding() {
    let mut r = rig();
    r.select(r.ta);
    r.ws.pcb_command("length_target", Some("30/0.1")).unwrap();
    r.ws.editor_command("tool:tune").unwrap();
    r.hover(Point::new(20 * MM, 0));
    let info = r.info().unwrap();
    assert!(
        info.problem
            .as_deref()
            .is_some_and(|p| p.contains("too long")),
        "{info:?}"
    );
}
