//! The incremental connectivity engine on complex, hierarchical designs.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_geometry::{Point, Rectangle, Segment};
use buturecad_schematic::builtin;
use buturecad_schematic::*;
use proptest::prelude::*;

const G: i64 = 1_270_000;

struct B {
    sch: Schematic,
    next: u64,
}

impl B {
    fn new() -> Self {
        let mut sch = Schematic::new(Id(1));
        builtin::install(&mut sch);
        Self { sch, next: 100 }
    }
    fn id(&mut self) -> Id {
        self.next += 1;
        Id(self.next)
    }
    fn put(&mut self, sheet: Id, o: Object) -> Id {
        let id = self.id();
        self.sch.insert(sheet, id, o).unwrap();
        id
    }
    fn part(&mut self, sheet: Id, lib: &str, reference: &str, x: i64, y: i64) -> Id {
        self.put(
            sheet,
            Object::Component(Component {
                lib_id: lib.into(),
                reference: reference.into(),
                value: String::new(),
                footprint: String::new(),
                properties: Default::default(),
                unit: 1,
                representation: 1,
                placement: Placement::at(Point::new(x * G, y * G)),
            }),
        )
    }
    /// Connection point of a pin of a placed component.
    fn pin(&self, comp: Id, number: &str) -> Point {
        let Some((_, Object::Component(c))) = self.sch.object(comp) else {
            panic!("not a component")
        };
        self.sch
            .placed_pins(c)
            .into_iter()
            .find(|p| p.number == number)
            .unwrap()
            .position
    }
    fn wire(&mut self, sheet: Id, a: Point, b: Point) -> Id {
        self.put(
            sheet,
            Object::Wire(Wire {
                segment: Segment::new(a, b),
            }),
        )
    }
    /// An L-shaped connection between two points (vertical leg first).
    fn route_v(&mut self, sheet: Id, a: Point, b: Point) {
        if a.x == b.x || a.y == b.y {
            self.wire(sheet, a, b);
        } else {
            let corner = Point::new(a.x, b.y);
            self.wire(sheet, a, corner);
            self.wire(sheet, corner, b);
        }
    }
    /// An L-shaped connection between two points (horizontal leg first).
    fn route(&mut self, sheet: Id, a: Point, b: Point) {
        if a.x == b.x || a.y == b.y {
            self.wire(sheet, a, b);
        } else {
            let corner = Point::new(b.x, a.y);
            self.wire(sheet, a, corner);
            self.wire(sheet, corner, b);
        }
    }
    fn label(&mut self, sheet: Id, at: Point, name: &str) -> Id {
        self.put(
            sheet,
            Object::NetLabel(NetLabel {
                position: at,
                name: name.into(),
                orientation: Direction::Right,
            }),
        )
    }
    fn global(&mut self, sheet: Id, at: Point, name: &str) -> Id {
        self.put(
            sheet,
            Object::GlobalLabel(GlobalLabel {
                position: at,
                name: name.into(),
                orientation: Direction::Right,
            }),
        )
    }
    fn hier(&mut self, sheet: Id, at: Point, name: &str) -> Id {
        self.put(
            sheet,
            Object::HierLabel(HierLabel {
                position: at,
                name: name.into(),
                orientation: Direction::Right,
            }),
        )
    }
    fn power(&mut self, sheet: Id, lib: &str, net: &str, at: Point) -> Id {
        self.put(
            sheet,
            Object::PowerSymbol(PowerSymbol {
                lib_id: lib.into(),
                net_name: net.into(),
                placement: Placement::at(at),
            }),
        )
    }
    fn sheet(&mut self, parent: Id, child: Id, name: &str, x: i64, y0: i64, pins: &[&str]) -> Id {
        self.sch.add_sheet(child, name).unwrap();
        let rect = Rectangle::new(
            Point::new(x * G, y0 * G),
            Point::new((x + 10) * G, (y0 + 10) * G),
        )
        .unwrap();
        let pins = pins
            .iter()
            .enumerate()
            .map(|(i, n)| SheetPin {
                name: (*n).into(),
                position: Point::new(x * G, (y0 + 2 + 2 * i as i64) * G),
            })
            .collect();
        self.put(
            parent,
            Object::Sheet(SheetInstance {
                name: name.into(),
                child,
                rect,
                pins,
            }),
        )
    }
}

fn p(x: i64, y: i64) -> Point {
    Point::new(x * G, y * G)
}

fn pins_of(n: &Net) -> Vec<String> {
    n.pins
        .iter()
        .map(|p| format!("{}.{}", p.reference, p.pin))
        .collect()
}

/// The design used by several tests:
///
/// ```text
/// root:   J1.1 ──▶ [Filter: IN,OUT] ──▶ R9.1 (+5V)          R9.2 (GND)
///         label SENSE (local) , global label RESET
/// Filter: IN ─ R1 ─ OUT ─ C1 ─ GND      [Stage: A=IN node, B=OUT node] (nested)
///         label SENSE (local, different net from the root one)
/// Stage:  A ─ D1 ─ B                    global label RESET, a GND power symbol
/// ```
struct Design {
    b: B,
    root: Id,
    filter: Id,
    stage: Id,
    j1: Id,
    r9: Id,
    r1: Id,
    c1: Id,
    d1: Id,
    filter_inst: Id,
}

fn design() -> Design {
    let mut b = B::new();
    let (root, filter, stage) = (Id(1), Id(2), Id(3));

    // Root sheet.
    let j1 = b.part(root, "Buture:Conn_01x02", "J1", -20, 0);
    let r9 = b.part(root, "Buture:R", "R9", 40, 0);
    let filter_inst = b.sheet(root, filter, "Filter", 10, 0, &["IN", "OUT"]);
    // J1.1 → Filter.IN (sheet pin at (10, 2))
    let j1_1 = b.pin(j1, "1");
    b.route(root, j1_1, p(10, 2));
    // Filter.OUT (10, 4) → R9.1
    let r9_1 = b.pin(r9, "1");
    b.route(root, p(10, 4), r9_1);
    let r9_2 = b.pin(r9, "2");
    b.power(root, "Buture:GND", "GND", r9_2);
    b.power(root, "Buture:+5V", "+5V", r9_1);
    b.label(root, p(-20, 10), "SENSE");
    b.global(root, p(-20, 14), "RESET");

    // Filter sheet.
    let r1 = b.part(filter, "Buture:R", "R1", 0, 0);
    let c1 = b.part(filter, "Buture:C", "C1", 20, -6);
    b.hier(filter, p(-10, 3), "IN");
    b.route(filter, p(-10, 3), b.pin(r1, "1"));
    b.hier(filter, p(30, -3), "OUT");
    let r1_2 = b.pin(r1, "2");
    let c1_1 = b.pin(c1, "1");
    b.route(filter, r1_2, c1_1);
    b.route(filter, c1_1, p(30, -3));
    let c1_2 = b.pin(c1, "2");
    b.power(filter, "Buture:GND", "GND", c1_2);
    b.label(filter, p(0, 20), "SENSE");
    let stage_inst = b.sheet(filter, stage, "Stage", 40, 10, &["A", "B"]);
    let _ = stage_inst;
    // A is wired to the input node (R1.1), B to the output node (C1.1). The two wires cross
    // without a junction, which must not connect them.
    b.route_v(filter, b.pin(r1, "1"), p(40, 12));
    b.route_v(filter, c1_1, p(40, 14));

    // Stage sheet.
    let d1 = b.part(stage, "Buture:D", "D1", 0, 0);
    b.hier(stage, p(-10, 0), "A");
    b.route(stage, p(-10, 0), b.pin(d1, "1"));
    b.hier(stage, p(10, 0), "B");
    b.route(stage, b.pin(d1, "2"), p(10, 0));
    b.global(stage, p(0, 10), "RESET");
    b.power(stage, "Buture:GND", "GND", p(0, -10));

    Design {
        b,
        root,
        filter,
        stage,
        j1,
        r9,
        r1,
        c1,
        d1,
        filter_inst,
    }
}

#[test]
fn hierarchy_global_names_and_local_scopes_resolve() {
    let d = design();
    let c = Connectivity::build(&d.b.sch);
    assert!(c.issues().is_empty(), "{:?}", c.issues());

    // J1.1 → IN → R1.1 → A → D1.1 (sheet pins and hierarchical labels, two levels deep).
    let vin = c.net_of_pin(d.j1, "1").unwrap();
    assert_eq!(pins_of(vin), ["D1.1", "J1.1", "R1.1"]);
    assert_eq!(vin.sheets.len(), 3);
    assert!(!vin.global);

    // A is the input node, B the output node; D1 sits between them.
    // R1.2 ─ C1.1 ─ OUT ─ R9.1 ─ +5V, and D1.2 through B.
    let out = c.net_of_pin(d.r1, "2").unwrap();
    assert_ne!(
        out.id, vin.id,
        "crossing wires without a junction do not connect"
    );
    assert_eq!(out.id, c.net_of_pin(d.c1, "1").unwrap().id);
    assert_eq!(out.id, c.net_of_pin(d.d1, "2").unwrap().id);
    assert_eq!(out.id, c.net_of_pin(d.r9, "1").unwrap().id);
    assert_eq!(out.name, "+5V");
    assert!(out.power && out.global);

    // GND is one net across three sheets.
    let gnd = c.net_by_name("GND").unwrap();
    assert_eq!(pins_of(gnd), ["C1.2", "R9.2"]);
    assert_eq!(gnd.sheets.len(), 3);
    assert!(gnd.power);

    // Local labels with the same text are different nets in different sheets.
    let sense: Vec<_> = c
        .nets()
        .iter()
        .filter(|n| n.name.contains("SENSE"))
        .collect();
    assert_eq!(sense.len(), 2);
    assert!(c.net_by_name("/SENSE").is_some());
    assert!(c.net_by_name("/Filter/SENSE").is_some());

    // A global label spans sheets but, being alone on each sheet, carries no pins here.
    let reset = c.net_by_name("RESET").unwrap();
    assert!(reset.global && !reset.power);
    assert_eq!(reset.sheets.len(), 2);

    assert_eq!(c.connected_pins(d.j1, "1").len(), 2);
    assert!(c.connected_pins(d.j1, "2").is_empty());
}

#[test]
fn engine_matches_a_full_build_and_reports_what_it_rebuilt() {
    let mut d = design();
    let mut engine = ConnectivityEngine::new(&d.b.sch);
    assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));

    // Move R1's output wire in the Filter sheet only.
    let id = d.b.id();
    let change = Change::create(
        d.filter,
        id,
        Object::Wire(Wire {
            segment: Segment::new(p(60, 30), p(70, 30)),
        }),
    );
    d.b.sch.apply(std::slice::from_ref(&change)).unwrap();
    let stats = engine.update(&d.b.sch, &[change]);
    assert_eq!(
        stats,
        UpdateStats {
            sheets_rebuilt: 1,
            sheets_reused: 2
        }
    );
    assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
    // The new, isolated wire became its own net and a dangling-end pair.
    assert_eq!(engine.connectivity().dangling_ends().len(), 2);
}

#[test]
fn changes_in_a_child_sheet_propagate_to_nets_that_span_sheets() {
    let mut d = design();
    let mut engine = ConnectivityEngine::new(&d.b.sch);
    // Break the Stage's connection to D1 pin 1 by deleting the wire that ends on it.
    let target = {
        let sheet = d.b.sch.sheet(d.stage).unwrap();
        sheet
            .objects
            .iter()
            .find_map(|(id, o)| match o {
                Object::Wire(w)
                    if w.segment.b == d.b.pin(d.d1, "1") || w.segment.a == d.b.pin(d.d1, "1") =>
                {
                    Some((*id, o.clone()))
                }
                _ => None,
            })
            .unwrap()
    };
    let change = Change::delete(d.stage, target.0, target.1);
    d.b.sch.apply(std::slice::from_ref(&change)).unwrap();
    let stats = engine.update(&d.b.sch, std::slice::from_ref(&change));
    assert_eq!(stats.sheets_rebuilt, 1);
    let c = engine.connectivity();
    // D1.1 is now alone; the hierarchical net no longer reaches it.
    assert_ne!(
        c.net_of_pin(d.d1, "1").unwrap().id,
        c.net_of_pin(d.r1, "1").unwrap().id
    );
    assert_eq!(c, &Connectivity::build(&d.b.sch));
    // Undo.
    d.b.sch.revert(std::slice::from_ref(&change)).unwrap();
    engine.update(&d.b.sch, &[change]);
    assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
    assert_eq!(
        engine.connectivity().net_of_pin(d.d1, "1").unwrap().id,
        engine.connectivity().net_of_pin(d.r1, "1").unwrap().id
    );
}

#[test]
fn sheets_added_after_the_engine_was_built_are_picked_up() {
    let mut d = design();
    let mut engine = ConnectivityEngine::new(&d.b.sch);
    let extra = Id(50);
    d.b.sch.add_sheet(extra, "Extra").unwrap();
    let inst = Object::Sheet(SheetInstance {
        name: "Extra".into(),
        child: extra,
        rect: Rectangle::new(p(100, 0), p(110, 10)).unwrap(),
        pins: vec![],
    });
    let id = d.b.id();
    let change = Change::create(d.root, id, inst);
    d.b.sch.apply(std::slice::from_ref(&change)).unwrap();
    let stats = engine.update(&d.b.sch, &[change]);
    assert_eq!(stats.sheets_rebuilt, 2, "the root and the new sheet");
    assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
}

#[test]
fn symbol_changes_are_not_change_sets_and_need_a_refresh() {
    let mut d = design();
    let mut engine = ConnectivityEngine::new(&d.b.sch);
    // Redefine the resistor with a third, shared pin.
    let mut r = d.b.sch.symbol("Buture:R").unwrap().clone();
    let mut extra = r.pins[0].clone();
    extra.number = "3".into();
    r.pins.push(extra);
    d.b.sch.add_symbol(r);
    assert_ne!(
        engine.connectivity(),
        &Connectivity::build(&d.b.sch),
        "stale until refreshed"
    );
    let stats = engine.refresh(&d.b.sch);
    assert_eq!(stats.sheets_rebuilt, 3);
    assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
}

#[test]
fn dangling_wire_ends_are_reported_with_their_reason() {
    let mut b = B::new();
    let root = Id(1);
    let r1 = b.part(root, "Buture:R", "R1", 0, 0);
    let r2 = b.part(root, "Buture:R", "R2", 20, 0);
    // R1.2 → free end.
    let pin = b.pin(r1, "2");
    let dangling = b.wire(root, pin, Point::new(pin.x, pin.y - 5 * G));
    // R2.1 → a T-end on the body of another wire without a junction.
    let body = b.wire(root, p(40, 30), p(60, 30));
    let pin2 = b.pin(r2, "1");
    let to_body = b.wire(root, pin2, p(50, 3));
    let to_body2 = b.wire(root, p(50, 3), p(50, 30));
    let c = Connectivity::build(&b.sch);
    let ends = c.dangling_ends();
    // Free end of the first wire.
    assert!(ends
        .iter()
        .any(|e| e.wire == dangling && e.touches.is_none()));
    // The wire that runs onto the body of `body` is flagged and names the wire it touches.
    let t = ends.iter().find(|e| e.wire == to_body2).unwrap();
    assert_eq!(t.touches, Some(body));
    // Ends on pins or on other wire ends are not dangling.
    assert!(!ends.iter().any(|e| e.wire == to_body));
    // With a junction the T becomes a connection.
    let _ = b.put(
        root,
        Object::Junction(Junction {
            position: p(50, 30),
        }),
    );
    let c2 = Connectivity::build(&b.sch);
    assert!(!c2.dangling_ends().iter().any(|e| e.wire == to_body2));
}

// -- randomised edit sequences ----------------------------------------------------------------

#[derive(Debug, Clone)]
enum Edit {
    Wire {
        sheet: u8,
        x1: i8,
        y1: i8,
        x2: i8,
        y2: i8,
    },
    Label {
        sheet: u8,
        x: i8,
        y: i8,
        name: u8,
        global: bool,
    },
    Junction {
        sheet: u8,
        x: i8,
        y: i8,
    },
    Delete {
        sheet: u8,
        nth: u8,
    },
    Undo,
}

fn edit_strategy() -> impl Strategy<Value = Edit> {
    let c = -6i8..6;
    prop_oneof![
        4 => (0u8..3, c.clone(), c.clone(), c.clone(), c.clone())
            .prop_map(|(sheet, x1, y1, x2, y2)| Edit::Wire { sheet, x1, y1, x2, y2 }),
        2 => (0u8..3, c.clone(), c.clone(), 0u8..3, any::<bool>())
            .prop_map(|(sheet, x, y, name, global)| Edit::Label { sheet, x, y, name, global }),
        1 => (0u8..3, c.clone(), c.clone()).prop_map(|(sheet, x, y)| Edit::Junction { sheet, x, y }),
        2 => (0u8..3, any::<u8>()).prop_map(|(sheet, nth)| Edit::Delete { sheet, nth }),
        1 => Just(Edit::Undo),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// After any sequence of edits and undos, the incrementally maintained result equals a build
    /// from scratch.
    #[test]
    fn incremental_updates_always_equal_a_full_rebuild(edits in prop::collection::vec(edit_strategy(), 1..40)) {
        let mut d = design();
        let sheets = [d.root, d.filter, d.stage];
        let mut engine = ConnectivityEngine::new(&d.b.sch);
        let mut undo: Vec<Change> = Vec::new();
        for e in edits {
            let change = match e {
                Edit::Wire { sheet, x1, y1, x2, y2 } => {
                    let (a, b) = (p(i64::from(x1), i64::from(y1)), p(i64::from(x2), i64::from(y2)));
                    if a == b { continue; }
                    Change::create(sheets[sheet as usize % 3], d.b.id(), Object::Wire(Wire { segment: Segment::new(a, b) }))
                }
                Edit::Label { sheet, x, y, name, global } => {
                    let n = ["A", "B", "RESET"][name as usize % 3];
                    let at = p(i64::from(x), i64::from(y));
                    let o = if global {
                        Object::GlobalLabel(GlobalLabel { position: at, name: n.into(), orientation: Direction::Right })
                    } else {
                        Object::NetLabel(NetLabel { position: at, name: n.into(), orientation: Direction::Right })
                    };
                    Change::create(sheets[sheet as usize % 3], d.b.id(), o)
                }
                Edit::Junction { sheet, x, y } => Change::create(
                    sheets[sheet as usize % 3],
                    d.b.id(),
                    Object::Junction(Junction { position: p(i64::from(x), i64::from(y)) }),
                ),
                Edit::Delete { sheet, nth } => {
                    let sid = sheets[sheet as usize % 3];
                    let objs: Vec<_> = d.b.sch.sheet(sid).unwrap().objects.iter()
                        .filter(|(_, o)| !matches!(o, Object::Sheet(_) | Object::Component(_)))
                        .map(|(i, o)| (*i, o.clone())).collect();
                    if objs.is_empty() { continue; }
                    let (id, o) = objs[nth as usize % objs.len()].clone();
                    Change::delete(sid, id, o)
                }
                Edit::Undo => {
                    let Some(c) = undo.pop() else { continue };
                    d.b.sch.revert(std::slice::from_ref(&c)).unwrap();
                    engine.update(&d.b.sch, &[c]);
                    prop_assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
                    continue;
                }
            };
            d.b.sch.apply(std::slice::from_ref(&change)).unwrap();
            let stats = engine.update(&d.b.sch, std::slice::from_ref(&change));
            prop_assert!(stats.sheets_rebuilt == 1);
            prop_assert_eq!(engine.connectivity(), &Connectivity::build(&d.b.sch));
            undo.push(change);
        }
        let _ = d.filter_inst;
    }
}
