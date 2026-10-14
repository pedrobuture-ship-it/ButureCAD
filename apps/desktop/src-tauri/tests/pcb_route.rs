//! Interactive routing and vias through the workspace, like the UI drives them.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_desktop_lib::ui_types::*;
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_eda_core::commands::*;
use buturecad_eda_core::ROOT_SHEET;
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::layer;
use buturecad_schematic::{builtin, Change, Component, Object, Placement, Wire};

const G: i64 = 1_270_000;
const MM: i64 = 1_000_000;
const FP: &str = "Generated:SOIC-8_3.9mm_P1.27mm";

fn part(reference: &str, x: i64) -> Object {
    Object::Component(Component {
        lib_id: "Buture:R".into(),
        reference: reference.into(),
        value: "10k".into(),
        footprint: FP.into(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x * G, 0)),
    })
}

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

/// R1.2 — R2.1 on net N; footprints updated from the schematic; view centred on the footprints.
fn board(dir: &std::path::Path, second_net: bool) -> Workspace {
    let mut ws = Workspace::new(None);
    ws.new_project(&dir.join("P"), "P").unwrap();
    ws.set_canvas_size(1000.0, 700.0);
    ws.set_mode(Mode::FootprintEditor);
    ws.footprint_editor_command("generate", Some("soic|pins=8"))
        .unwrap();
    ws.set_mode(Mode::Pcb);
    let e = &mut ws.open.as_mut().unwrap().engine;
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let (a, b, w) = (e.new_id(), e.new_id(), e.new_id());
    let mut changes = vec![
        Change::create(ROOT_SHEET, a, part("R1", 0)),
        Change::create(ROOT_SHEET, b, part("R2", 10)),
        Change::create(
            ROOT_SHEET,
            w,
            Object::Wire(Wire {
                segment: Segment::new(Point::new(0, -3 * G), Point::new(10 * G, 3 * G)),
            }),
        ),
    ];
    if second_net {
        let w2 = e.new_id();
        changes.push(Change::create(
            ROOT_SHEET,
            w2,
            Object::Wire(Wire {
                segment: Segment::new(Point::new(0, 3 * G), Point::new(10 * G, -3 * G)),
            }),
        ));
    }
    e.execute(SchematicEditCommand::new("build", changes))
        .unwrap();
    ws.pcb_update_preview().unwrap();
    ws.pcb_update_apply().unwrap();
    ws.fit_view();
    ws
}

fn pads(ws: &Workspace, reference: &str, pad: &str) -> Point {
    let doc = ws.open.as_ref().unwrap().engine.project().pcb();
    let f = doc
        .placed
        .values()
        .find(|f| f.reference == reference)
        .unwrap();
    f.world_pads(&doc.footprints[&f.footprint])
        .into_iter()
        .find(|p| p.number == pad)
        .unwrap()
        .center
}

fn view(ws: &mut Workspace) {
    // 25 px per mm, centred between the two footprints.
    let (a, b) = (pads(ws, "R1", "2"), pads(ws, "R2", "1"));
    let v = ws.views.get_mut(&Mode::Pcb).unwrap();
    v.center = (((a.x + b.x) / 2) as f64, ((a.y + b.y) / 2) as f64);
    v.scale = 25.0 / MM as f64;
}

fn at(ws: &Workspace, p: Point) -> (f64, f64) {
    ws.views[&Mode::Pcb].world_to_screen(p)
}

fn click(ws: &mut Workspace, p: Point) {
    let (x, y) = at(ws, p);
    ws.pointer(ev(PointerKind::Move, x, y));
    ws.pointer(ev(PointerKind::Down, x, y));
    ws.pointer(ev(PointerKind::Up, x, y));
}

fn hover(ws: &mut Workspace, p: Point) {
    let (x, y) = at(ws, p);
    ws.pointer(ev(PointerKind::Move, x, y));
}

fn tracks(ws: &Workspace) -> Vec<buturecad_eda_core::model::Track> {
    ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .tracks
        .values()
        .cloned()
        .collect()
}

fn vias(ws: &Workspace) -> usize {
    ws.open.as_ref().unwrap().engine.project().pcb().vias.len()
}

fn net_n(ws: &Workspace) -> buturecad_pcb::Id {
    *ws.open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .nets
        .iter()
        .find(|(_, n)| n.name.contains("R1-Pad2"))
        .unwrap()
        .0
}

#[test]
fn routing_from_pad_to_pad_creates_tracks_on_the_net_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    assert_eq!(ws.snapshot().pcb.unwrap().unrouted, 1);
    let (from, to) = (pads(&ws, "R1", "2"), pads(&ws, "R2", "1"));
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    let info = ws.snapshot().pcb.unwrap().route.unwrap();
    assert!(info.net.contains("R1-Pad2") && info.layer == "F.Cu");
    // A corner, then the target pad: the route ends by itself.
    click(&mut ws, Point::new(from.x, to.y));
    assert!(
        tracks(&ws).is_empty(),
        "nothing is created until the route ends"
    );
    click(&mut ws, to);
    let t = tracks(&ws);
    assert!(!t.is_empty());
    let net = net_n(&ws);
    assert!(t
        .iter()
        .all(|t| t.net == Some(net) && t.layer == layer::F_CU && t.width == 250_000));
    assert!(
        t.iter().any(|t| t.segment.a == from || t.segment.b == from),
        "starts exactly on the pad centre"
    );
    assert!(
        t.iter().any(|t| t.segment.a == to || t.segment.b == to),
        "ends exactly on the pad centre"
    );
    assert!(ws.snapshot().pcb.unwrap().route.is_none());
    assert_eq!(
        ws.snapshot().pcb.unwrap().unrouted,
        0,
        "the ratsnest line is gone"
    );
    let history = ws.snapshot().project.unwrap().undo_history;
    assert_eq!(
        history
            .iter()
            .filter(|h| h.starts_with("Route track"))
            .count(),
        1
    );
    ws.undo().unwrap();
    assert!(tracks(&ws).is_empty());
    assert_eq!(ws.snapshot().pcb.unwrap().unrouted, 1);
}

#[test]
fn angle_modes_width_and_elbow_flip() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    let (from, to) = (pads(&ws, "R1", "2"), pads(&ws, "R2", "1"));
    ws.pcb_command("route_width", Some("0.4")).unwrap();
    assert!(ws.pcb_command("route_width", Some("0")).is_err());
    assert!(ws.pcb_command("route_mode", Some("30")).is_err());
    for (mode, ok) in [
        (
            "90",
            (|dx: i64, dy: i64| dx == 0 || dy == 0) as fn(i64, i64) -> bool,
        ),
        ("45", |dx: i64, dy: i64| {
            dx == 0 || dy == 0 || dx.abs() == dy.abs()
        }),
    ] {
        ws.pcb_command("route_mode", Some(mode)).unwrap();
        ws.editor_command("tool:route").unwrap();
        click(&mut ws, from);
        hover(&mut ws, to);
        ws.editor_command("flip_elbow").unwrap();
        click(&mut ws, to);
        let t = tracks(&ws);
        assert!(!t.is_empty());
        for s in &t {
            assert!(
                ok(s.segment.b.x - s.segment.a.x, s.segment.b.y - s.segment.a.y),
                "{mode}: {s:?}"
            );
            assert_eq!(s.width, 400_000);
        }
        ws.undo().unwrap();
    }
    ws.pcb_command("route_mode", Some("free")).unwrap();
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    click(&mut ws, to);
    assert_eq!(tracks(&ws).len(), 1, "free angle is one straight segment");
}

#[test]
fn escape_cancels_and_empty_space_starts_a_netless_route() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    let from = pads(&ws, "R1", "2");
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    click(&mut ws, Point::new(from.x - 3 * MM, from.y - 5 * MM));
    ws.editor_command("cancel").unwrap();
    assert!(ws.snapshot().pcb.unwrap().route.is_none());
    assert!(tracks(&ws).is_empty(), "cancel discards the route");
    assert_eq!(
        ws.snapshot().tool,
        "route",
        "the first Esc only cancels the route"
    );
    // Free start: no net; Enter finishes where the pointer is.
    let empty = Point::new(from.x - 6 * MM, from.y - 8 * MM);
    click(&mut ws, empty);
    assert_eq!(ws.snapshot().pcb.unwrap().route.unwrap().net, "(no net)");
    click(&mut ws, Point::new(empty.x - 4 * MM, empty.y));
    ws.pcb_command("route_finish", None).unwrap();
    let t = tracks(&ws);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].net, None);
}

#[test]
fn a_route_never_shorts_another_net() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), true);
    view(&mut ws);
    let doc_nets = ws.open.as_ref().unwrap().engine.project().pcb().nets.len();
    assert_eq!(doc_nets, 2);
    let from = pads(&ws, "R1", "2"); // net A
    let other = pads(&ws, "R1", "1"); // net B
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    click(&mut ws, other);
    assert!(
        ws.snapshot()
            .messages
            .iter()
            .any(|m| m.text.contains("belongs to net")),
        "{:?}",
        ws.snapshot().messages.last()
    );
    assert!(
        ws.snapshot().pcb.unwrap().route.is_some(),
        "the route continues; the click was refused"
    );
    assert!(tracks(&ws).is_empty());
}

#[test]
fn vias_change_layers_during_a_route_and_everything_is_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    ws.pcb_command("route_mode", Some("90")).unwrap();
    let (from, to) = (pads(&ws, "R1", "2"), pads(&ws, "R2", "1"));
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    let a = Point::new(from.x, from.y + 4 * MM);
    click(&mut ws, a);
    // Via here: continue on the back copper.
    hover(&mut ws, a);
    ws.editor_command("route_via").unwrap();
    assert_eq!(ws.snapshot().pcb.unwrap().route.unwrap().layer, "B.Cu");
    assert_eq!(
        ws.snapshot().pcb.unwrap().active_layer,
        "B.Cu",
        "the active layer follows the route"
    );
    let c = Point::new(to.x, a.y);
    click(&mut ws, c);
    hover(&mut ws, c);
    ws.editor_command("route_via").unwrap();
    assert_eq!(ws.snapshot().pcb.unwrap().route.unwrap().layer, "F.Cu");
    assert_eq!(ws.snapshot().pcb.unwrap().route.unwrap().vias, 2);
    click(&mut ws, to);
    let t = tracks(&ws);
    let layers: std::collections::BTreeSet<_> = t.iter().map(|t| t.layer).collect();
    assert_eq!(layers, [layer::F_CU, layer::B_CU].into_iter().collect());
    assert_eq!(vias(&ws), 2);
    let doc = ws.open.as_ref().unwrap().engine.project().pcb();
    assert!(doc.vias.values().all(|v| v.net == Some(net_n(&ws))
        && v.diameter == 800_000
        && v.drill == 400_000
        && v.kind == buturecad_eda_core::model::ViaKind::Through));
    assert_eq!(
        ws.snapshot().pcb.unwrap().unrouted,
        0,
        "front track, via, back track, via, front track connect the pads"
    );
    let history = ws.snapshot().project.unwrap().undo_history;
    assert_eq!(
        history
            .iter()
            .filter(|h| *h == "Route track with vias")
            .count(),
        1
    );
    ws.undo().unwrap();
    assert!((tracks(&ws).is_empty(), vias(&ws)) == (true, 0));
    assert_eq!(ws.snapshot().pcb.unwrap().unrouted, 1);
    // Via sizes: validated.
    ws.pcb_command("via_size", Some("1.0/0.5")).unwrap();
    assert!(ws.pcb_command("via_size", Some("0.5/0.6")).is_err());
    assert!(ws.pcb_command("via_size", Some("nonsense")).is_err());
    assert!(
        ws.editor_command("route_via").is_err(),
        "no route in progress"
    );
}

#[test]
fn tracks_can_be_edited_afterwards_width_net_layer_and_vertices() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    ws.pcb_command("route_mode", Some("90")).unwrap();
    let (from, to) = (pads(&ws, "R1", "2"), pads(&ws, "R2", "1"));
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    click(&mut ws, Point::new(from.x, to.y));
    click(&mut ws, to);
    ws.editor_command("tool:select").unwrap();
    ws.editor_command("select_all").unwrap();
    assert!(ws.snapshot().selection.iter().any(|s| s.kind == "track"));
    ws.editor_command("clear_selection").unwrap();

    // Inspector: width, layer, endpoints.
    let first = *ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .tracks
        .keys()
        .next()
        .unwrap();
    ws.set_property(first.0, "width_mm", "0.5").unwrap();
    assert_eq!(tracks(&ws)[0].width, 500_000);
    ws.set_property(first.0, "layer", "1").unwrap();
    assert_eq!(tracks(&ws)[0].layer, layer::B_CU);
    assert!(
        ws.set_property(first.0, "layer", "7").is_err(),
        "no such layer on a 2-layer board"
    );
    assert!(ws.set_property(first.0, "net", "NoSuchNet").is_err());
    ws.set_property(first.0, "net", "").unwrap();
    assert_eq!(tracks(&ws)[0].net, None);
    ws.undo().unwrap();
    ws.undo().unwrap();
    ws.undo().unwrap();

    // Vertex drag: select the corner's track, grab its far end and move it; the track that meets
    // it follows.
    let corner = Point::new(from.x, to.y);
    let doc_tracks: Vec<_> = ws
        .open
        .as_ref()
        .unwrap()
        .engine
        .project()
        .pcb()
        .tracks
        .iter()
        .map(|(i, t)| (*i, t.clone()))
        .collect();
    let (tid, _) = doc_tracks
        .iter()
        .find(|(_, t)| t.segment.a == corner || t.segment.b == corner)
        .unwrap();
    ws.selection.insert(Mode::Pcb, [*tid].into_iter().collect());
    let (x0, y0) = at(&ws, corner);
    let target = Point::new(corner.x + 2 * MM, corner.y);
    let (x1, y1) = at(&ws, target);
    ws.pointer(ev(PointerKind::Move, x0, y0));
    ws.pointer(ev(PointerKind::Down, x0, y0));
    for i in 1..=4 {
        let t = f64::from(i) / 4.0;
        ws.pointer(ev(
            PointerKind::Move,
            x0 + (x1 - x0) * t,
            y0 + (y1 - y0) * t,
        ));
    }
    ws.pointer(ev(PointerKind::Up, x1, y1));
    let moved = tracks(&ws);
    assert!(
        moved
            .iter()
            .all(|t| t.segment.a != corner && t.segment.b != corner),
        "nothing is left at the old corner"
    );
    // The two tracks still meet at one common point (the new corner, snapped to the grid).
    let mut ends: Vec<Point> = moved
        .iter()
        .flat_map(|t| [t.segment.a, t.segment.b])
        .collect();
    ends.sort_by_key(|p| (p.x, p.y));
    let shared: Vec<&Point> = ends
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| &w[0])
        .collect();
    assert_eq!(shared.len(), 1, "{ends:?}");
    assert_ne!(*shared[0], corner);
    assert_eq!(
        ws.snapshot().project.unwrap().undo_history.last().unwrap(),
        "Move track vertex"
    );
    ws.undo().unwrap();
    assert!(
        tracks(&ws)
            .iter()
            .filter(|t| t.segment.a == corner || t.segment.b == corner)
            .count()
            == 2
    );
}

#[test]
fn the_via_tool_places_through_vias_on_the_net_under_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    let pad = pads(&ws, "R1", "2");
    ws.editor_command("tool:via").unwrap();
    click(&mut ws, pad);
    click(&mut ws, Point::new(pad.x - 5 * MM, pad.y - 6 * MM));
    let doc = ws.open.as_ref().unwrap().engine.project().pcb();
    assert_eq!(doc.vias.len(), 2);
    let on_pad = doc.vias.values().find(|v| v.position == pad).unwrap();
    assert_eq!(
        on_pad.net,
        Some(net_n(&ws)),
        "a via on a pad takes its net and sits on its centre"
    );
    assert!(doc.vias.values().any(|v| v.net.is_none()));
    // Vias are editable afterwards.
    let id = *doc.vias.iter().find(|(_, v)| v.position == pad).unwrap().0;
    ws.set_property(id.0, "diameter_mm", "1.2").unwrap();
    ws.set_property(id.0, "drill_mm", "0.6").unwrap();
    assert!(
        ws.set_property(id.0, "drill_mm", "1.5").is_err(),
        "the drill must stay inside the annulus"
    );
    let v = ws.open.as_ref().unwrap().engine.project().pcb().vias[&id].clone();
    assert_eq!((v.diameter, v.drill), (1_200_000, 600_000));
    // The via is a copper object of the net: it adds an island, so the ratsnest still needs it joined.
    assert_eq!(ws.snapshot().pcb.unwrap().unrouted, 1);
}

#[test]
fn the_router_uses_the_net_class_and_a_manual_width_overrides_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = board(dir.path(), false);
    view(&mut ws);
    let net = net_n(&ws);
    let mut rules = ws.pcb_rules_info().unwrap();
    let mut power = rules.classes[0].clone();
    power.name = "Power".into();
    power.track_width = "0.6".into();
    power.via_diameter = "1.2".into();
    power.via_drill = "0.6".into();
    rules.classes.push(power);
    rules.nets.iter_mut().find(|n| n.id == net.0).unwrap().class = "Power".into();
    ws.pcb_rules_apply(rules.clone()).unwrap();

    let (from, to) = (pads(&ws, "R1", "2"), pads(&ws, "R2", "1"));
    ws.editor_command("tool:route").unwrap();
    click(&mut ws, from);
    let info = ws.snapshot().pcb.unwrap();
    assert_eq!(info.route_class, "Power");
    assert_eq!(info.route_width_mm, "0.6");
    assert_eq!(info.via_size, "1.2/0.6");
    click(&mut ws, to);
    assert!(tracks(&ws).iter().all(|t| t.width == 600_000));

    // A manual width wins until it is cleared.
    ws.undo().unwrap();
    ws.pcb_command("route_width", Some("0.3")).unwrap();
    click(&mut ws, from);
    click(&mut ws, to);
    assert!(tracks(&ws).iter().all(|t| t.width == 300_000));

    // Editing the rules is one undo step and a bad class is refused whole.
    let mut bad = rules.clone();
    bad.classes[1].via_drill = "2".into();
    assert!(ws.pcb_rules_apply(bad).is_err());
    ws.undo().unwrap();
    ws.undo().unwrap();
    assert_eq!(
        ws.pcb_rules_info().unwrap().classes.len(),
        1,
        "undo removed the class"
    );
}
