#![allow(clippy::unwrap_used)]

use buturecad_geometry::{Point, Rectangle, Segment, Vector};
use buturecad_schematic::builtin;
use buturecad_schematic::*;
use proptest::prelude::*;

const G: i64 = 1_270_000; // 50 mil

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
    fn root(&self) -> Id {
        self.sch.root()
    }
    fn part(&mut self, sheet: Id, lib: &str, reference: &str, x: i64, y: i64) -> Id {
        self.part_placed(
            sheet,
            lib,
            reference,
            Placement::at(Point::new(x * G, y * G)),
        )
    }
    fn part_placed(&mut self, sheet: Id, lib: &str, reference: &str, placement: Placement) -> Id {
        self.put(
            sheet,
            Object::Component(Component {
                lib_id: lib.into(),
                reference: reference.into(),
                value: "".into(),
                footprint: "".into(),
                properties: Default::default(),
                placement,
            }),
        )
    }
    fn wire(&mut self, sheet: Id, a: (i64, i64), b: (i64, i64)) -> Id {
        self.put(
            sheet,
            Object::Wire(Wire {
                segment: Segment::new(Point::new(a.0 * G, a.1 * G), Point::new(b.0 * G, b.1 * G)),
            }),
        )
    }
    fn junction(&mut self, sheet: Id, at: (i64, i64)) -> Id {
        self.put(
            sheet,
            Object::Junction(Junction {
                position: Point::new(at.0 * G, at.1 * G),
            }),
        )
    }
    fn net_label(&mut self, sheet: Id, at: (i64, i64), name: &str) -> Id {
        self.put(
            sheet,
            Object::NetLabel(NetLabel {
                position: Point::new(at.0 * G, at.1 * G),
                name: name.into(),
                orientation: Direction::Right,
            }),
        )
    }
    fn global_label(&mut self, sheet: Id, at: (i64, i64), name: &str) -> Id {
        self.put(
            sheet,
            Object::GlobalLabel(GlobalLabel {
                position: Point::new(at.0 * G, at.1 * G),
                name: name.into(),
                orientation: Direction::Right,
            }),
        )
    }
    fn power(&mut self, sheet: Id, lib: &str, net: &str, at: (i64, i64)) -> Id {
        self.put(
            sheet,
            Object::PowerSymbol(PowerSymbol {
                lib_id: lib.into(),
                net_name: net.into(),
                placement: Placement::at(Point::new(at.0 * G, at.1 * G)),
            }),
        )
    }
}

/// Resistor pin positions in grid units: pin 1 at (x, y+3), pin 2 at (x, y-3).
fn names(c: &Connectivity) -> Vec<String> {
    c.nets().iter().map(|n| n.name.clone()).collect()
}

fn pin_set(n: &Net) -> Vec<String> {
    n.pins
        .iter()
        .map(|p| format!("{}.{}", p.reference, p.pin))
        .collect()
}

#[test]
fn wire_between_two_pins_makes_one_net_with_auto_name() {
    let mut b = B::new();
    b.part(b.root(), "Buture:R", "R1", 0, 0); // pins (0,3) and (0,-3)
    b.part(b.root(), "Buture:R", "R2", 10, 0);
    let w = b.wire(b.root(), (0, -3), (10, -3));
    let c = Connectivity::build(&b.sch);
    assert_eq!(
        names(&c),
        vec!["Net-(R1-Pad1)", "Net-(R1-Pad2)", "Net-(R2-Pad1)"]
    );
    let joined = c.net_by_name("Net-(R1-Pad2)").unwrap();
    assert_eq!(pin_set(joined), vec!["R1.2", "R2.2"]);
    assert_eq!(joined.wires, vec![w]);
    assert!(!joined.named);
    assert!(c.issues().is_empty());
}

#[test]
fn touching_pins_connect_without_a_wire() {
    let mut b = B::new();
    b.part(b.root(), "Buture:R", "R1", 0, 0);
    b.part(b.root(), "Buture:R", "R2", 0, -6); // its pin 1 is at (0,-3) = R1 pin 2
    let c = Connectivity::build(&b.sch);
    assert_eq!(
        pin_set(c.net_by_name("Net-(R1-Pad2)").unwrap()),
        vec!["R1.2", "R2.1"]
    );
}

#[test]
fn crossing_wires_are_not_connected_without_a_junction() {
    let mut b = B::new();
    let a = b.wire(b.root(), (0, 0), (10, 0));
    let v = b.wire(b.root(), (5, -5), (5, 5));
    b.net_label(b.root(), (0, 0), "X");
    b.net_label(b.root(), (5, 5), "Y");
    let c = Connectivity::build(&b.sch);
    assert_eq!(
        names(&c),
        vec!["/X", "/Y"],
        "visual crossing is not a connection"
    );
    assert_eq!(c.net_by_name("/X").unwrap().wires, vec![a]);
    assert_eq!(c.net_by_name("/Y").unwrap().wires, vec![v]);

    b.junction(b.root(), (5, 0));
    let c = Connectivity::build(&b.sch);
    assert_eq!(c.nets().len(), 1, "an explicit junction connects them");
    assert!(
        matches!(c.issues(), [Issue::NameConflict { names, .. }] if names == &vec!["/X".to_string(), "/Y".to_string()])
    );
    assert_eq!(c.nets()[0].name, "/X", "ties resolve alphabetically");
}

#[test]
fn t_connection_needs_a_junction() {
    let mut b = B::new();
    b.wire(b.root(), (0, 0), (10, 0));
    b.wire(b.root(), (5, 0), (5, 5)); // ends on the interior of the first wire
    b.net_label(b.root(), (0, 0), "A");
    b.net_label(b.root(), (5, 5), "B");
    assert_eq!(Connectivity::build(&b.sch).nets().len(), 2);
    b.junction(b.root(), (5, 0));
    assert_eq!(Connectivity::build(&b.sch).nets().len(), 1);
}

#[test]
fn wires_ending_at_the_same_point_are_connected() {
    let mut b = B::new();
    b.wire(b.root(), (0, 0), (5, 0));
    b.wire(b.root(), (5, 0), (5, 5));
    b.wire(b.root(), (5, 0), (9, -3));
    let c = Connectivity::build(&b.sch);
    assert_eq!(c.nets().len(), 1);
    assert_eq!(c.nets()[0].wires.len(), 3);
}

#[test]
fn pin_on_the_interior_of_a_wire_attaches() {
    let mut b = B::new();
    b.part(b.root(), "Buture:R", "R1", 0, 0); // pin 1 at (0,3)
    b.wire(b.root(), (-5, 3), (5, 3));
    b.net_label(b.root(), (-5, 3), "SIG");
    let c = Connectivity::build(&b.sch);
    assert_eq!(pin_set(c.net_by_name("/SIG").unwrap()), vec!["R1.1"]);
}

#[test]
fn same_name_local_labels_join_nets_within_a_sheet() {
    let mut b = B::new();
    b.wire(b.root(), (0, 0), (5, 0));
    b.wire(b.root(), (0, 10), (5, 10));
    b.net_label(b.root(), (0, 0), "SDA");
    b.net_label(b.root(), (5, 10), "SDA");
    let c = Connectivity::build(&b.sch);
    assert_eq!(c.nets().len(), 1);
    assert_eq!(c.nets()[0].wires.len(), 2);
    assert!(c.nets()[0].named);
}

fn with_child(b: &mut B) -> Id {
    let child = Id(2);
    b.sch.add_sheet(child, "Child").unwrap();
    let rect = Rectangle::new(Point::new(30 * G, 0), Point::new(40 * G, 10 * G)).unwrap();
    b.put(
        b.root(),
        Object::Sheet(SheetInstance {
            name: "Child".into(),
            child,
            rect,
            pins: vec![],
        }),
    );
    child
}

#[test]
fn local_labels_are_scoped_per_sheet_but_globals_are_not() {
    let mut b = B::new();
    let child = with_child(&mut b);
    for sheet in [b.root(), child] {
        b.wire(sheet, (0, 0), (5, 0));
        b.net_label(sheet, (0, 0), "SDA");
        b.wire(sheet, (0, 10), (5, 10));
        b.global_label(sheet, (0, 10), "RST");
    }
    let c = Connectivity::build(&b.sch);
    assert_eq!(names(&c), vec!["/Child/SDA", "/SDA", "RST"]);
    assert_eq!(
        c.net_by_name("RST").unwrap().sheets.len(),
        2,
        "global net spans both sheets"
    );
}

#[test]
fn power_symbols_connect_by_name_across_sheets() {
    let mut b = B::new();
    let child = with_child(&mut b);
    b.part(b.root(), "Buture:R", "R1", 0, 0);
    b.part(child, "Buture:R", "R2", 0, 0);
    // GND ports sitting directly on pin 2 of each resistor.
    b.power(b.root(), "Buture:GND", "GND", (0, -3));
    b.power(child, "Buture:GND", "GND", (0, -3));
    let c = Connectivity::build(&b.sch);
    let gnd = c.net_by_name("GND").unwrap();
    assert_eq!(pin_set(gnd), vec!["R1.2", "R2.2"]);
    assert!(gnd.named);
}

#[test]
fn power_name_outranks_labels() {
    let mut b = B::new();
    b.wire(b.root(), (0, 0), (5, 0));
    b.net_label(b.root(), (0, 0), "foo");
    b.power(b.root(), "Buture:+5V", "+5V", (5, 0));
    let c = Connectivity::build(&b.sch);
    assert_eq!(names(&c), vec!["+5V"]);
    assert!(
        c.issues().is_empty(),
        "different priorities are not a conflict"
    );
}

fn hierarchy(pin_name: &str, label_name: &str) -> (B, Id) {
    let mut b = B::new();
    let child = Id(2);
    b.sch.add_sheet(child, "Power").unwrap();
    b.part(b.root(), "Buture:R", "R1", 0, 0);
    b.part(child, "Buture:R", "R3", 0, 0);
    // Parent: sheet with a pin at (20,3) wired to R1 pin 1 at (0,3).
    let rect = Rectangle::new(Point::new(20 * G, 0), Point::new(30 * G, 10 * G)).unwrap();
    let pin = SheetPin {
        name: pin_name.into(),
        position: Point::new(20 * G, 3 * G),
    };
    b.put(
        b.root(),
        Object::Sheet(SheetInstance {
            name: "Power".into(),
            child,
            rect,
            pins: vec![pin],
        }),
    );
    b.wire(b.root(), (0, 3), (20, 3));
    // Child: hierarchical label wired to R3 pin 1.
    b.wire(child, (0, 3), (-8, 3));
    b.put(
        child,
        Object::HierLabel(HierLabel {
            position: Point::new(-8 * G, 3 * G),
            name: label_name.into(),
            orientation: Direction::Left,
        }),
    );
    (b, child)
}

#[test]
fn sheet_pin_links_to_hierarchical_label() {
    let (b, _) = hierarchy("IN", "IN");
    let c = Connectivity::build(&b.sch);
    let net = c.net_of_pin(find_component(&b, "R1"), "1").unwrap();
    assert_eq!(pin_set(net), vec!["R1.1", "R3.1"]);
    assert!(c.issues().is_empty(), "{:?}", c.issues());
    assert_eq!(
        net.name, "/Power/IN",
        "the label's (higher priority) name wins"
    );
}

#[test]
fn mismatched_hierarchy_names_are_reported_and_not_joined() {
    let (b, child) = hierarchy("IN", "OUT");
    let c = Connectivity::build(&b.sch);
    let r1 = c.net_of_pin(find_component(&b, "R1"), "1").unwrap();
    assert_eq!(pin_set(r1), vec!["R1.1"]);
    assert!(c
        .issues()
        .iter()
        .any(|i| matches!(i, Issue::SheetPinWithoutLabel { pin, .. } if pin == "IN")));
    assert!(c
        .issues()
        .iter()
        .any(|i| matches!(i, Issue::HierLabelWithoutPin { sheet, .. } if *sheet == child)));
}

fn find_component(b: &B, reference: &str) -> Id {
    b.sch
        .sheets()
        .values()
        .flat_map(|s| s.objects.iter())
        .find_map(|(id, o)| {
            matches!(o, Object::Component(c) if c.reference == reference).then_some(*id)
        })
        .unwrap()
}

#[test]
fn unreachable_sheets_are_reported_and_ignored() {
    let mut b = B::new();
    b.sch.add_sheet(Id(2), "Orphan").unwrap();
    b.part(Id(2), "Buture:R", "R9", 0, 0);
    let c = Connectivity::build(&b.sch);
    assert!(c.nets().is_empty());
    assert_eq!(c.issues(), &[Issue::UnreachableSheet { sheet: Id(2) }]);
}

#[test]
fn no_connect_markers_are_recorded() {
    let mut b = B::new();
    let r = b.part(b.root(), "Buture:R", "R1", 0, 0);
    b.put(
        b.root(),
        Object::NoConnect(NoConnect {
            position: Point::new(0, 3 * G),
        }),
    );
    let c = Connectivity::build(&b.sch);
    assert!(c.has_no_connect(r, "1") && !c.has_no_connect(r, "2"));
}

#[test]
fn buses_expose_members_and_report_bad_labels() {
    let mut b = B::new();
    let s1 = b.put(
        b.root(),
        Object::Bus(Bus {
            segment: Segment::new(Point::new(0, 0), Point::new(10 * G, 0)),
        }),
    );
    let s2 = b.put(
        b.root(),
        Object::Bus(Bus {
            segment: Segment::new(Point::new(10 * G, 0), Point::new(10 * G, 8 * G)),
        }),
    );
    let label = b.net_label(b.root(), (5, 0), "D[0..3]");
    let c = Connectivity::build(&b.sch);
    assert_eq!(c.buses().len(), 1);
    let bus = &c.buses()[0];
    assert_eq!(
        (bus.label, bus.def.members()),
        (
            label,
            vec!["D0", "D1", "D2", "D3"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        bus.segments,
        vec![s1, s2],
        "connected bus segments form one bus"
    );
    assert!(c.nets().is_empty(), "buses make no electrical connection");

    b.net_label(b.root(), (50, 50), "A[0..1]"); // not on a bus
    b.net_label(b.root(), (2, 0), "X[3..1]"); // on the bus but invalid
    let c = Connectivity::build(&b.sch);
    assert_eq!(
        c.issues()
            .iter()
            .filter(|i| matches!(i, Issue::BadBusLabel { .. }))
            .count(),
        2
    );
}

#[test]
fn rotation_and_mirror_move_pins_exactly() {
    let mut b = B::new();
    // Diode: K at (-3,0), A at (3,0) grid units.
    let plain = b.part_placed(
        b.root(),
        "Buture:D",
        "D1",
        Placement::at(Point::new(10 * G, 10 * G)),
    );
    let mirrored = b.part_placed(
        b.root(),
        "Buture:D",
        "D2",
        Placement {
            position: Point::new(10 * G, 10 * G),
            rotation: 0,
            mirror: true,
        },
    );
    let rotated = b.part_placed(
        b.root(),
        "Buture:D",
        "D3",
        Placement {
            position: Point::new(10 * G, 10 * G),
            rotation: 1,
            mirror: false,
        },
    );
    let pins = |id: Id| match b.sch.object(id).unwrap().1 {
        Object::Component(c) => b.sch.placed_pins(c),
        _ => unreachable!(),
    };
    let k = |v: Vec<PlacedPin>| v.iter().find(|p| p.number == "1").unwrap().clone();
    assert_eq!(k(pins(plain)).position, Point::new(7 * G, 10 * G));
    assert_eq!(k(pins(plain)).orientation, Direction::Right);
    assert_eq!(k(pins(mirrored)).position, Point::new(13 * G, 10 * G));
    assert_eq!(k(pins(mirrored)).orientation, Direction::Left);
    assert_eq!(
        k(pins(rotated)).position,
        Point::new(10 * G, 7 * G),
        "(-3,0) turned 90° CCW is (0,-3)"
    );
    assert_eq!(k(pins(rotated)).orientation, Direction::Up);
}

#[test]
fn validation_rejects_bad_objects() {
    let mut b = B::new();
    let root = b.root();
    let comp = |lib: &str, rotation: u8| {
        Object::Component(Component {
            lib_id: lib.into(),
            reference: "R1".into(),
            value: "".into(),
            footprint: "".into(),
            properties: Default::default(),
            placement: Placement {
                position: Point::ORIGIN,
                rotation,
                mirror: false,
            },
        })
    };
    assert_eq!(
        b.sch.insert(root, Id(500), comp("Nope:X", 0)),
        Err(SchematicError::UnknownSymbol("Nope:X".into()))
    );
    assert_eq!(
        b.sch.insert(root, Id(500), comp("Buture:R", 4)),
        Err(SchematicError::BadRotation(4))
    );
    let flat = Object::Wire(Wire {
        segment: Segment::new(Point::ORIGIN, Point::ORIGIN),
    });
    assert_eq!(
        b.sch.insert(root, Id(500), flat),
        Err(SchematicError::ZeroLength("wire"))
    );
    let far = Object::Junction(Junction {
        position: Point::new(i64::MAX, 0),
    });
    assert_eq!(
        b.sch.insert(root, Id(500), far),
        Err(SchematicError::OutOfRange)
    );
    let not_power = Object::PowerSymbol(PowerSymbol {
        lib_id: "Buture:R".into(),
        net_name: "X".into(),
        placement: Placement::at(Point::ORIGIN),
    });
    assert!(matches!(
        b.sch.insert(root, Id(500), not_power),
        Err(SchematicError::NotPowerSymbol(_))
    ));
    let blank = Object::NetLabel(NetLabel {
        position: Point::ORIGIN,
        name: " ".into(),
        orientation: Direction::Right,
    });
    assert!(matches!(
        b.sch.insert(root, Id(500), blank),
        Err(SchematicError::EmptyName(_))
    ));
    assert_eq!(
        b.sch.insert(Id(77), Id(500), comp("Buture:R", 0)),
        Err(SchematicError::NoSuchSheet(Id(77)))
    );
    let id = b.part(root, "Buture:R", "R1", 0, 0);
    assert_eq!(
        b.sch.insert(root, id, comp("Buture:R", 0)),
        Err(SchematicError::DuplicateId(id))
    );
    assert_eq!(
        b.sch.insert(root, root, comp("Buture:R", 0)),
        Err(SchematicError::DuplicateId(root)),
        "sheet ids are in the same id space"
    );
}

#[test]
fn sheet_hierarchy_rules() {
    let mut b = B::new();
    let rect = Rectangle::new(Point::ORIGIN, Point::new(5 * G, 5 * G)).unwrap();
    let inst = |child| {
        Object::Sheet(SheetInstance {
            name: "S".into(),
            child,
            rect,
            pins: vec![],
        })
    };
    b.sch.add_sheet(Id(2), "A").unwrap();
    b.sch.add_sheet(Id(3), "B").unwrap();
    b.sch.insert(Id(1), Id(10), inst(Id(2))).unwrap();
    b.sch.insert(Id(2), Id(11), inst(Id(3))).unwrap();
    assert_eq!(
        b.sch.insert(Id(3), Id(12), inst(Id(2))),
        Err(SchematicError::SheetCycle(Id(2))),
        "B → A → B would be a cycle"
    );
    assert_eq!(
        b.sch.insert(Id(3), Id(13), inst(Id(1))),
        Err(SchematicError::SheetCycle(Id(1))),
        "the root cannot be a child"
    );
    assert_eq!(
        b.sch.insert(Id(1), Id(14), inst(Id(3))),
        Err(SchematicError::SheetReused(Id(3)))
    );
    assert_eq!(
        b.sch.insert(Id(2), Id(15), inst(Id(2))),
        Err(SchematicError::SheetCycle(Id(2)))
    );
    assert_eq!(
        b.sch.insert(Id(1), Id(16), inst(Id(99))),
        Err(SchematicError::NoSuchSheet(Id(99)))
    );
}

#[test]
fn from_parts_validates_untrusted_input() {
    let b = B::new();
    let mut sheets = b.sch.sheets().clone();
    let symbols = b.sch.symbols().clone();
    assert!(Schematic::from_parts(Id(1), symbols.clone(), sheets.clone()).is_ok());
    assert!(matches!(
        Schematic::from_parts(Id(9), symbols.clone(), sheets.clone()),
        Err(SchematicError::NoRoot(Id(9)))
    ));
    // The same object id on two sheets.
    sheets.insert(
        Id(2),
        Sheet {
            name: "S".into(),
            objects: Default::default(),
        },
    );
    let j = Object::Junction(Junction {
        position: Point::ORIGIN,
    });
    for s in [Id(1), Id(2)] {
        if let Some(sh) = sheets.get_mut(&s) {
            sh.objects.insert(Id(50), j.clone());
        }
    }
    assert!(matches!(
        Schematic::from_parts(Id(1), symbols, sheets),
        Err(SchematicError::DuplicateId(Id(50)))
    ));
}

#[test]
fn change_sets_apply_and_revert_atomically() {
    let mut b = B::new();
    let root = b.root();
    let j = |x| {
        Object::Junction(Junction {
            position: Point::new(x, 0),
        })
    };
    let id = b.put(root, j(0));
    let before = b.sch.clone();
    let changes = vec![
        Change::create(root, Id(900), j(5)),
        Change::modify(root, id, j(0), j(7)),
        Change::delete(root, Id(900), j(5)),
        Change::create(root, Id(901), j(9)),
    ];
    b.sch.apply(&changes).unwrap();
    assert!(b.sch.object(Id(900)).is_none());
    assert_eq!(b.sch.object(id).unwrap().1, &j(7));
    assert!(b.sch.object(Id(901)).is_some());
    b.sch.revert(&changes).unwrap();
    assert_eq!(b.sch, before);

    // A stale `before` fails the whole set and leaves the model untouched.
    let bad = vec![
        Change::create(root, Id(902), j(1)),
        Change::modify(root, id, j(123), j(8)),
    ];
    assert_eq!(b.sch.apply(&bad), Err(SchematicError::StaleChange(id)));
    assert_eq!(b.sch, before);
    // An invalid new state fails the whole set too.
    let invalid = vec![
        Change::create(root, Id(903), j(1)),
        Change::create(
            root,
            Id(904),
            Object::Wire(Wire {
                segment: Segment::new(Point::ORIGIN, Point::ORIGIN),
            }),
        ),
    ];
    assert!(b.sch.apply(&invalid).is_err());
    assert_eq!(b.sch, before);
}

#[test]
fn next_reference_fills_the_lowest_gap() {
    let mut b = B::new();
    assert_eq!(b.sch.next_reference("R"), "R1");
    b.part(b.root(), "Buture:R", "R1", 0, 0);
    b.part(b.root(), "Buture:R", "R3", 5, 0);
    b.part(b.root(), "Buture:C", "C1", 9, 0);
    assert_eq!(b.sch.next_reference("R"), "R2");
    assert_eq!(b.sch.next_reference("C"), "C2");
    assert_eq!(b.sch.next_reference("U"), "U1");
}

#[test]
fn object_bounds_cover_symbol_pins() {
    let mut b = B::new();
    let id = b.part(b.root(), "Buture:R", "R1", 10, 10);
    let bb = b.sch.object_bounds(b.sch.object(id).unwrap().1);
    assert!(
        bb.contains_point(Point::new(10 * G, 13 * G))
            && bb.contains_point(Point::new(10 * G, 7 * G))
    );
}

/// A small circuit; every coordinate is shifted by `(dx, dy)` grid units.
fn circuit(dx: i64, dy: i64) -> B {
    let mut b = B::new();
    let root = b.root();
    b.part(root, "Buture:R", "R1", dx, dy);
    b.part(root, "Buture:C", "C1", 10 + dx, dy);
    b.part(root, "Buture:LED", "D1", 20 + dx, dy);
    b.wire(root, (dx, dy - 3), (10 + dx, dy - 3));
    b.wire(root, (10 + dx, dy - 3), (17 + dx, dy - 3));
    b.junction(root, (10 + dx, dy - 3));
    b.wire(root, (dx, dy + 3), (dx, dy + 8));
    b.net_label(root, (dx, dy + 8), "VIN");
    b.power(root, "Buture:GND", "GND", (10 + dx, dy + 3));
    b
}

fn summary(c: &Connectivity) -> Vec<(String, Vec<String>)> {
    c.nets()
        .iter()
        .map(|n| (n.name.clone(), pin_set(n)))
        .collect()
}

proptest! {
    /// Connectivity depends on topology, not on where the circuit sits on the sheet.
    #[test]
    fn connectivity_is_translation_invariant(dx in -200i64..200, dy in -200i64..200) {
        let base = summary(&Connectivity::build(&circuit(0, 0).sch));
        let moved = summary(&Connectivity::build(&circuit(dx, dy).sch));
        prop_assert_eq!(base, moved);
    }

    /// Moving all objects through change sets and reverting restores the schematic exactly.
    #[test]
    fn translate_changes_roundtrip(dx in -50i64..50, dy in -50i64..50) {
        let b = circuit(0, 0);
        let v = Vector::new(dx * G, dy * G);
        let mut changes = Vec::new();
        for (sid, sheet) in b.sch.sheets() {
            for (id, obj) in &sheet.objects {
                if let Object::Junction(j) = obj {
                    changes.push(Change::modify(*sid, *id, obj.clone(), Object::Junction(Junction { position: j.position + v })));
                }
            }
        }
        let mut sch = b.sch.clone();
        sch.apply(&changes).unwrap();
        sch.revert(&changes).unwrap();
        prop_assert_eq!(sch, b.sch);
    }
}

#[test]
fn circuit_has_the_expected_topology() {
    let c = Connectivity::build(&circuit(0, 0).sch);
    assert_eq!(
        c.net_by_name("GND").map(pin_set),
        Some(vec!["C1.1".to_string()])
    );
    assert_eq!(
        c.net_by_name("/VIN").map(pin_set),
        Some(vec!["R1.1".to_string()])
    );
    let joined = c.nets().iter().find(|n| n.pins.len() == 2).unwrap();
    assert_eq!(pin_set(joined), vec!["C1.2", "R1.2"]);
}
