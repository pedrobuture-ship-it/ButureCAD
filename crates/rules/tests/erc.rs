//! ERC behaviour on small, readable circuits.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_geometry::{Point, Rectangle, Segment};
use buturecad_rules::{pin_conflict, run_erc, ErcReport, Rule, Severity, Violation};
use buturecad_schematic::{
    builtin, Component, Connectivity, Direction, GlobalLabel, HierLabel, Id, Junction, NetLabel,
    NoConnect, Object, Pin, PinType, Placement, PowerSymbol, Schematic, SheetInstance, SheetPin,
    Symbol, Waiver, Wire,
};

const G: i64 = 1_270_000;

fn p(x: i64, y: i64) -> Point {
    Point::new(x * G, y * G)
}

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
    /// A fully specified component (value and footprint filled in).
    fn part(&mut self, lib: &str, reference: &str, x: i64, y: i64) -> Id {
        self.part_unit(lib, reference, 1, x, y)
    }
    fn part_unit(&mut self, lib: &str, reference: &str, unit: u8, x: i64, y: i64) -> Id {
        let root = self.root();
        self.put(
            root,
            Object::Component(Component {
                lib_id: lib.into(),
                reference: reference.into(),
                value: "v".into(),
                footprint: "FP".into(),
                properties: Default::default(),
                placement: Placement::at(p(x, y)),
                unit,
                representation: 1,
            }),
        )
    }
    fn pin(&self, comp: Id, number: &str) -> Point {
        let Some((_, Object::Component(c))) = self.sch.object(comp) else {
            panic!("not a component")
        };
        self.sch
            .placed_pins(c)
            .into_iter()
            .find(|q| q.number == number)
            .unwrap()
            .position
    }
    fn wire(&mut self, a: Point, b: Point) -> Id {
        let root = self.root();
        self.put(
            root,
            Object::Wire(Wire {
                segment: Segment::new(a, b),
            }),
        )
    }
    fn power(&mut self, lib: &str, net: &str, at: Point) -> Id {
        let root = self.root();
        self.put(
            root,
            Object::PowerSymbol(PowerSymbol {
                lib_id: lib.into(),
                net_name: net.into(),
                placement: Placement::at(at),
            }),
        )
    }
    fn symbol(&mut self, name: &str, kinds: &[(&str, PinType)]) {
        let mut s = Symbol::new(name, "U");
        for (i, (num, kind)) in kinds.iter().enumerate() {
            s.pins.push(Pin {
                number: (*num).into(),
                name: (*num).into(),
                kind: *kind,
                position: Point::new(0, -(i as i64) * 2 * G),
                orientation: Direction::Right,
                length: 0,
                unit: 0,
                representation: 0,
            });
        }
        self.sch.add_symbol(s);
    }
    fn erc(&self) -> ErcReport {
        let conn = Connectivity::build(&self.sch);
        run_erc(&self.sch, &conn, self.sch.waivers())
    }
}

fn of(r: &ErcReport, rule: Rule) -> Vec<&Violation> {
    r.violations.iter().filter(|v| v.rule == rule).collect()
}

/// R1 — R2 in a chain whose ends are tied to a connector.
fn simple_good() -> B {
    let mut b = B::new();
    let j1 = b.part("Buture:Conn_01x02", "J1", -20, 0);
    let r1 = b.part("Buture:R", "R1", 0, 0);
    let (j_1, j_2) = (b.pin(j1, "1"), b.pin(j1, "2"));
    let (r_1, r_2) = (b.pin(r1, "1"), b.pin(r1, "2"));
    // J1.1 → R1.1 and R1.2 → J1.2 with L-shaped wires that touch no other pin.
    b.wire(j_1, Point::new(r_1.x, j_1.y));
    b.wire(Point::new(r_1.x, j_1.y), r_1);
    b.wire(r_2, Point::new(r_2.x, j_2.y - 3 * G));
    b.wire(
        Point::new(r_2.x, j_2.y - 3 * G),
        Point::new(j_2.x, j_2.y - 3 * G),
    );
    b.wire(Point::new(j_2.x, j_2.y - 3 * G), j_2);
    b
}

#[test]
fn a_fully_connected_circuit_is_clean() {
    let b = simple_good();
    let r = b.erc();
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
    assert!(r.is_clean());
}

#[test]
fn unconnected_pins_are_errors_until_connected_or_marked_no_connect() {
    let mut b = B::new();
    let r1 = b.part("Buture:R", "R1", 0, 0);
    let r = b.erc();
    let un = of(&r, Rule::UnconnectedPin);
    assert_eq!(un.len(), 2);
    assert!(un.iter().all(|v| v.severity == Severity::Error));
    assert!(un.iter().all(|v| v.objects == vec![r1] && v.at.is_some()));
    assert_eq!(un[0].fingerprint, "unconnected_pin|R1.1");

    // A no-connect marker on pin 2 accepts it.
    let at = b.pin(r1, "2");
    let root = b.root();
    b.put(root, Object::NoConnect(NoConnect { position: at }));
    let r = b.erc();
    let un = of(&r, Rule::UnconnectedPin);
    assert_eq!(un.len(), 1);
    assert_eq!(un[0].fingerprint, "unconnected_pin|R1.1");
    assert!(!r.is_clean());
}

#[test]
fn a_pin_wired_to_nothing_is_still_unconnected_and_the_wire_dangles() {
    let mut b = B::new();
    let r1 = b.part("Buture:R", "R1", 0, 0);
    let pin = b.pin(r1, "1");
    b.wire(pin, Point::new(pin.x, pin.y + 4 * G));
    let r = b.erc();
    assert!(of(&r, Rule::UnconnectedPin)
        .iter()
        .any(|v| v.fingerprint == "unconnected_pin|R1.1"));
    let d = of(&r, Rule::DanglingWire);
    assert_eq!(
        d.len(),
        1,
        "only the free end dangles; the other ends on the pin"
    );
    assert_eq!(d[0].severity, Severity::Warning);
}

#[test]
fn pin_compatibility_matrix_is_symmetric_and_as_documented() {
    use PinType::*;
    use Severity::*;
    let all = [
        Input,
        Output,
        Bidirectional,
        TriState,
        Passive,
        PowerInput,
        PowerOutput,
        OpenCollector,
        OpenEmitter,
        Unspecified,
        NoConnect,
    ];
    for a in all {
        for b in all {
            assert_eq!(pin_conflict(a, b), pin_conflict(b, a), "{a:?} / {b:?}");
        }
    }
    assert_eq!(pin_conflict(Output, Output), Some(Error));
    assert_eq!(pin_conflict(Output, PowerOutput), Some(Error));
    assert_eq!(pin_conflict(PowerOutput, PowerOutput), Some(Error));
    assert_eq!(pin_conflict(Output, OpenCollector), Some(Error));
    assert_eq!(pin_conflict(PowerOutput, OpenEmitter), Some(Error));
    assert_eq!(pin_conflict(Output, TriState), Some(Warning));
    assert_eq!(pin_conflict(Output, Bidirectional), Some(Warning));
    assert_eq!(pin_conflict(Output, PowerInput), Some(Warning));
    assert_eq!(pin_conflict(OpenCollector, OpenEmitter), Some(Warning));
    // Things that are fine.
    for ok in [
        (Output, Input),
        (Output, Passive),
        (Bidirectional, Bidirectional),
        (TriState, TriState),
        (TriState, Bidirectional),
        (OpenCollector, OpenCollector),
        (OpenEmitter, OpenEmitter),
        (Passive, Passive),
        (PowerOutput, PowerInput),
        (PowerOutput, Input),
        (Input, Input),
        (Passive, Unspecified),
    ] {
        assert_eq!(pin_conflict(ok.0, ok.1), None, "{ok:?}");
    }
}

fn two_chips_on_one_net(kind_a: PinType, kind_b: PinType) -> B {
    let mut b = B::new();
    b.symbol("T:A", &[("1", kind_a)]);
    b.symbol("T:B", &[("1", kind_b)]);
    let u1 = b.part("T:A", "U1", 0, 0);
    let u2 = b.part("T:B", "U2", 20, 0);
    let (a, c) = (b.pin(u1, "1"), b.pin(u2, "1"));
    b.wire(a, c);
    b
}

#[test]
fn output_output_conflict_is_an_error_and_output_tristate_a_warning() {
    let r = two_chips_on_one_net(PinType::Output, PinType::Output).erc();
    let c = of(&r, Rule::PinConflict);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].severity, Severity::Error);
    assert_eq!(c[0].fingerprint, "pin_conflict|U1.1|U2.1");
    assert_eq!(c[0].objects.len(), 2, "both offending parts are clickable");
    assert!(c[0].message.contains("output pin U1.1") && c[0].message.contains("U2.1"));

    let r = two_chips_on_one_net(PinType::Output, PinType::TriState).erc();
    assert_eq!(of(&r, Rule::PinConflict)[0].severity, Severity::Warning);
    let r = two_chips_on_one_net(PinType::Output, PinType::Input).erc();
    assert!(of(&r, Rule::PinConflict).is_empty());
}

#[test]
fn no_connect_pins_must_stay_unconnected() {
    let r = two_chips_on_one_net(PinType::NoConnect, PinType::Input).erc();
    let nc = of(&r, Rule::NoConnectPinConnected);
    assert_eq!(nc.len(), 1);
    assert_eq!(nc[0].severity, Severity::Error);
    // Unconnected NC pins are fine.
    let mut b = B::new();
    b.symbol("T:N", &[("1", PinType::NoConnect)]);
    b.part("T:N", "U1", 0, 0);
    assert!(of(&b.erc(), Rule::UnconnectedPin).is_empty());
}

#[test]
fn power_inputs_need_a_source_a_flag_a_power_output_or_a_waiver() {
    let mut b = B::new();
    b.symbol(
        "T:IC",
        &[("1", PinType::PowerInput), ("2", PinType::PowerInput)],
    );
    let u1 = b.part("T:IC", "U1", 0, 0);
    let (v, g) = (b.pin(u1, "1"), b.pin(u1, "2"));
    b.power("Buture:+5V", "+5V", v);
    b.power("Buture:GND", "GND", g);
    let r = b.erc();
    let np = of(&r, Rule::PowerInputNotDriven);
    assert_eq!(np.len(), 2, "+5V and GND are not driven by anything");
    assert!(np.iter().all(|x| x.severity == Severity::Error));
    assert!(
        of(&r, Rule::UnconnectedPin).is_empty(),
        "power symbols connect the pins"
    );

    // A PWR_FLAG on the +5V net satisfies it.
    let flag = b.part("Buture:PWR_FLAG", "#FLG1", 30, 10);
    let at = b.pin(flag, "1");
    b.wire(at, Point::new(at.x, v.y));
    b.wire(Point::new(at.x, v.y), v);
    let r = b.erc();
    let np = of(&r, Rule::PowerInputNotDriven);
    assert_eq!(np.len(), 1);
    assert!(np[0].message.contains("GND"));
    // Power flags are virtual parts: no reference/value/footprint noise.
    assert!(of(&r, Rule::MissingFootprint)
        .iter()
        .all(|v| !v.message.contains("#FLG")));
}

#[test]
fn a_power_output_pin_drives_power_inputs() {
    let mut b = B::new();
    b.symbol("T:REG", &[("1", PinType::PowerOutput)]);
    b.symbol("T:IC", &[("1", PinType::PowerInput)]);
    let reg = b.part("T:REG", "U1", 0, 0);
    let ic = b.part("T:IC", "U2", 20, 0);
    let (a, c) = (b.pin(reg, "1"), b.pin(ic, "1"));
    b.wire(a, c);
    let r = b.erc();
    assert!(of(&r, Rule::PowerInputNotDriven).is_empty());
    assert!(of(&r, Rule::PinConflict).is_empty());
}

#[test]
fn inputs_with_no_driver_warn() {
    let r = two_chips_on_one_net(PinType::Input, PinType::Input).erc();
    let w = of(&r, Rule::InputNotDriven);
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].severity, Severity::Warning);
    // A passive on the net may be a pull-up: no warning.
    let r = two_chips_on_one_net(PinType::Input, PinType::Passive).erc();
    assert!(of(&r, Rule::InputNotDriven).is_empty());
}

#[test]
fn duplicate_references_units_and_annotation() {
    let mut b = B::new();
    b.part("Buture:R", "R1", 0, 0);
    b.part("Buture:R", "R1", 10, 0);
    b.part("Buture:C", "R1", 20, 0); // same reference, different symbol
    b.part("Buture:R", "R?", 30, 0);
    let r = b.erc();
    let d = of(&r, Rule::DuplicateReference);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].severity, Severity::Error);
    assert_eq!(d[0].objects.len(), 3);
    assert_eq!(d[0].fingerprint, "duplicate_reference|R1");
    let u = of(&r, Rule::Unannotated);
    assert_eq!(u.len(), 1);
    assert!(u[0].message.contains("R?"));

    // Two units of one symbol sharing a reference are fine; the same unit twice is not.
    let mut b = B::new();
    let mut gate = Symbol::new("T:Gate", "U");
    gate.unit_count = 2;
    b.sch.add_symbol(gate);
    b.part_unit("T:Gate", "U1", 1, 0, 0);
    b.part_unit("T:Gate", "U1", 2, 10, 0);
    assert!(of(&b.erc(), Rule::DuplicateReference).is_empty());
    b.part_unit("T:Gate", "U1", 2, 20, 0);
    assert_eq!(of(&b.erc(), Rule::DuplicateReference).len(), 1);
}

#[test]
fn missing_component_data_and_unknown_symbols() {
    let mut b = B::new();
    let root = b.root();
    b.put(
        root,
        Object::Component(Component {
            lib_id: "Buture:R".into(),
            reference: "R1".into(),
            value: String::new(),
            footprint: String::new(),
            properties: Default::default(),
            placement: Placement::at(p(0, 0)),
            unit: 1,
            representation: 1,
        }),
    );
    let r = b.erc();
    assert_eq!(of(&r, Rule::MissingValue).len(), 1);
    assert_eq!(of(&r, Rule::MissingFootprint).len(), 1);
    assert_eq!(
        of(&r, Rule::MissingFootprint)[0].severity,
        Severity::Warning
    );

    // A component whose symbol is not in the project (legacy data).
    b.put(
        root,
        Object::Component(Component {
            lib_id: String::new(),
            reference: "X1".into(),
            value: "v".into(),
            footprint: "f".into(),
            properties: Default::default(),
            placement: Placement::at(p(40, 0)),
            unit: 1,
            representation: 1,
        }),
    );
    let r = b.erc();
    let u = of(&r, Rule::UnknownSymbol);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].severity, Severity::Error);
}

#[test]
fn dangling_wires_and_missing_junctions_are_reported_with_locations() {
    let mut b = B::new();
    let free = b.wire(p(0, 0), p(10, 0));
    let body = b.wire(p(0, 5), p(20, 5));
    let t = b.wire(p(10, 5), p(10, 12)); // starts on the body of `body`
    let r = b.erc();
    let d = of(&r, Rule::DanglingWire);
    assert!(d.iter().any(|v| v.objects == vec![free] && v.at.is_some()));
    let mj = of(&r, Rule::MissingJunction);
    assert_eq!(mj.len(), 1);
    assert_eq!(mj[0].objects, vec![t, body]);
    assert_eq!(mj[0].at, Some(p(10, 5)));
    // A junction fixes it.
    let root = b.root();
    b.put(root, Object::Junction(Junction { position: p(10, 5) }));
    assert!(of(&b.erc(), Rule::MissingJunction).is_empty());
}

#[test]
fn net_name_conflicts_hierarchy_and_single_labels() {
    let mut b = B::new();
    let root = b.root();
    // Two different local labels on one wire.
    b.wire(p(0, 0), p(10, 0));
    b.put(
        root,
        Object::NetLabel(NetLabel {
            position: p(2, 0),
            name: "A".into(),
            orientation: Direction::Right,
        }),
    );
    b.put(
        root,
        Object::NetLabel(NetLabel {
            position: p(6, 0),
            name: "B".into(),
            orientation: Direction::Right,
        }),
    );
    // A global label alone in the design.
    b.put(
        root,
        Object::GlobalLabel(GlobalLabel {
            position: p(0, 20),
            name: "LONELY".into(),
            orientation: Direction::Right,
        }),
    );
    // A sheet whose pin has no label inside, and a hierarchical label with no pin.
    let child = Id(50);
    b.sch.add_sheet(child, "Child").unwrap();
    b.put(
        root,
        Object::Sheet(SheetInstance {
            name: "Child".into(),
            child,
            rect: Rectangle::new(p(30, 0), p(40, 10)).unwrap(),
            pins: vec![SheetPin {
                name: "IN".into(),
                position: p(30, 2),
            }],
        }),
    );
    b.put(
        child,
        Object::HierLabel(HierLabel {
            position: p(0, 0),
            name: "OTHER".into(),
            orientation: Direction::Right,
        }),
    );
    let r = b.erc();
    let nc = of(&r, Rule::NetNameConflict);
    assert_eq!(nc.len(), 1);
    assert_eq!(nc[0].severity, Severity::Error);
    assert_eq!(nc[0].objects.len(), 2);
    assert_eq!(of(&r, Rule::HierarchyMismatch).len(), 2);
    assert!(of(&r, Rule::SingleLabel)
        .iter()
        .any(|v| v.message.contains("LONELY")));
}

#[test]
fn waivers_are_documented_stable_and_never_hide_findings() {
    let mut b = B::new();
    let r1 = b.part("Buture:R", "R1", 0, 0);
    let before = b.erc();
    assert_eq!(before.counts().errors, 2);
    let target = of(&before, Rule::UnconnectedPin)[0].clone();
    b.sch
        .set_waivers(vec![Waiver {
            fingerprint: target.fingerprint.clone(),
            rule: Rule::UnconnectedPin.code().into(),
            subject: "R1 pin 1".into(),
            reason: "reserved for the test fixture".into(),
        }])
        .unwrap();
    let after = b.erc();
    // The finding is still listed, marked waived with its reason, and no longer counted.
    let w = after
        .violations
        .iter()
        .find(|v| v.fingerprint == target.fingerprint)
        .unwrap();
    assert_eq!(w.waived.as_deref(), Some("reserved for the test fixture"));
    assert_eq!(after.counts().errors, 1);
    assert_eq!(after.counts().waived, 1);

    // The waiver is keyed by reference and pin, so it survives re-creating the part with new ids.
    let root = b.root();
    b.sch.remove(r1).unwrap();
    b.put(
        root,
        Object::Component(Component {
            lib_id: "Buture:R".into(),
            reference: "R1".into(),
            value: "v".into(),
            footprint: "FP".into(),
            properties: Default::default(),
            placement: Placement::at(p(30, 30)),
            unit: 1,
            representation: 1,
        }),
    );
    let again = b.erc();
    assert_eq!(again.counts().waived, 1);

    // Once the finding is fixed, the waiver is reported as stale (an Info, not an error).
    let r1 = b
        .sch
        .sheets()
        .values()
        .flat_map(|s| s.objects.iter())
        .find_map(|(id, o)| matches!(o, Object::Component(_)).then_some(*id))
        .unwrap();
    let Some((_, Object::Component(c))) = b.sch.object(r1) else {
        panic!()
    };
    let pin1 = b
        .sch
        .placed_pins(c)
        .into_iter()
        .find(|q| q.number == "1")
        .unwrap()
        .position;
    b.put(root, Object::NoConnect(NoConnect { position: pin1 }));
    let fixed = b.erc();
    let stale = of(&fixed, Rule::StaleWaiver);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].severity, Severity::Info);
    assert!(stale[0].message.contains("R1 pin 1"));
}

#[test]
fn unplaced_units_are_informational() {
    let mut b = B::new();
    let mut gate = Symbol::new("T:Gate", "U");
    gate.unit_count = 3;
    b.sch.add_symbol(gate);
    b.part_unit("T:Gate", "U1", 2, 0, 0);
    let r = b.erc();
    let u = of(&r, Rule::UnplacedUnit);
    assert_eq!(u.len(), 2);
    assert!(u.iter().all(|v| v.severity == Severity::Info));
    assert!(u.iter().any(|v| v.message.contains("unit A")));
    assert!(u.iter().any(|v| v.message.contains("unit C")));
}

#[test]
fn reports_are_deterministic_sorted_and_point_at_real_objects() {
    let mut b = simple_good();
    b.part("Buture:R", "R1", 40, 0); // duplicate + unconnected pins
    b.wire(p(60, 0), p(70, 0));
    let r1 = b.erc();
    let r2 = b.erc();
    assert_eq!(r1, r2);
    let sev: Vec<Severity> = r1.violations.iter().map(|v| v.severity).collect();
    let mut sorted = sev.clone();
    sorted.sort();
    assert_eq!(sev, sorted, "errors first, then warnings, then infos");
    for v in &r1.violations {
        for id in &v.objects {
            assert!(
                b.sch.object(*id).is_some(),
                "{v:?} references a missing object"
            );
        }
        if !v.objects.is_empty() {
            assert!(v.at.is_some(), "{v:?} has objects but no location");
        }
        assert!(!v.fingerprint.is_empty() && !v.message.is_empty());
    }
}

#[test]
fn unreachable_sheets_are_not_checked() {
    let mut b = B::new();
    let orphan = Id(60);
    b.sch.add_sheet(orphan, "Orphan").unwrap();
    b.put(
        orphan,
        Object::Component(Component {
            lib_id: "Buture:R".into(),
            reference: "R9".into(),
            value: "v".into(),
            footprint: "f".into(),
            properties: Default::default(),
            placement: Placement::at(p(0, 0)),
            unit: 1,
            representation: 1,
        }),
    );
    let r = b.erc();
    assert!(of(&r, Rule::UnconnectedPin).is_empty());
    let u = of(&r, Rule::UnreachableSheet);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].severity, Severity::Info);
}
