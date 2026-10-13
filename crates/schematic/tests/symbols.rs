//! Multi-unit / alternate-representation symbols: compatibility and connectivity behaviour.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{
    Component, Connectivity, Direction, Graphic, Id, Object, Pin, PinType, Placement, Schematic,
    Symbol, SymbolGraphic, Wire, REPR_ALTERNATE, REPR_NORMAL,
};

/// Files written before units existed (schema 2 without the new fields) must still load, with
/// every item shared by all units and representations.
#[test]
fn symbols_written_before_units_still_load() {
    let json = r#"{
        "name": "Old:R", "reference_prefix": "R", "default_value": "1k", "is_power": false,
        "pins": [{"number":"1","name":"~","kind":"passive","position":{"x":0,"y":0},
                  "orientation":"down","length":1270000}],
        "graphics": [{"type":"line","segment":{"a":{"x":0,"y":0},"b":{"x":0,"y":5}},"width":150000}]
    }"#;
    let sym: Symbol = serde_json::from_str(json).unwrap();
    assert_eq!(sym.unit_count, 1);
    assert_eq!((sym.pins[0].unit, sym.pins[0].representation), (0, 0));
    assert_eq!(
        (sym.graphics[0].unit, sym.graphics[0].representation),
        (0, 0)
    );
    assert!(sym.meta.description.is_empty());
    assert_eq!(sym.pins_for(1, REPR_NORMAL).count(), 1);

    let comp: Component = serde_json::from_str(
        r#"{"lib_id":"Old:R","reference":"R1","value":"","footprint":"","properties":{},
            "placement":{"position":{"x":0,"y":0},"rotation":0,"mirror":false}}"#,
    )
    .unwrap();
    assert_eq!((comp.unit, comp.representation), (1, 1));
}

#[test]
fn symbols_round_trip_with_units_and_metadata() {
    let mut s = Symbol::new("Lib:Gate", "U");
    s.unit_count = 2;
    s.meta.description = "Quad NAND".into();
    s.meta.properties.insert("family".into(), "74HC".into());
    s.pins.push(Pin {
        number: "1".into(),
        name: "A".into(),
        kind: PinType::Input,
        position: Point::new(0, 0),
        orientation: Direction::Right,
        length: 1,
        unit: 2,
        representation: REPR_ALTERNATE,
    });
    s.graphics.push(SymbolGraphic {
        graphic: Graphic::Line {
            segment: Segment::new(Point::new(0, 0), Point::new(1, 1)),
            width: 1,
        },
        unit: 2,
        representation: 0,
    });
    let back: Symbol = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

fn gate() -> Symbol {
    let mut s = Symbol::new("Lib:Dual", "U");
    s.unit_count = 2;
    let pin = |n: &str, unit: u8, x: i64| Pin {
        number: n.into(),
        name: n.into(),
        kind: PinType::Passive,
        position: Point::new(x, 0),
        orientation: Direction::Right,
        length: 0,
        unit,
        representation: 0,
    };
    s.pins = vec![pin("1", 1, 0), pin("2", 2, 0), pin("3", 0, 10)]; // 3 is shared (power)
    s
}

fn placed(sch: &mut Schematic, id: u64, unit: u8, x: i64) {
    sch.insert(
        Id(0),
        Id(id),
        Object::Component(Component {
            lib_id: "Lib:Dual".into(),
            reference: "U1".into(),
            value: String::new(),
            footprint: String::new(),
            properties: Default::default(),
            placement: Placement::at(Point::new(x, 0)),
            unit,
            representation: REPR_NORMAL,
        }),
    )
    .unwrap();
}

#[test]
fn each_unit_instance_exposes_its_own_and_the_shared_pins() {
    let mut sch = Schematic::new(Id(0));
    sch.add_symbol(gate());
    placed(&mut sch, 1, 1, 0);
    placed(&mut sch, 2, 2, 100);
    let pins = |id| {
        let Some((_, Object::Component(c))) = sch.object(Id(id)) else {
            panic!()
        };
        sch.placed_pins(c)
            .into_iter()
            .map(|p| p.number)
            .collect::<Vec<_>>()
    };
    assert_eq!(pins(1), ["1", "3"]);
    assert_eq!(pins(2), ["2", "3"]);
}

#[test]
fn units_must_exist_and_alternate_needs_an_alternate_body() {
    let mut sch = Schematic::new(Id(0));
    sch.add_symbol(gate());
    placed(&mut sch, 1, 2, 0);
    let bad_unit = Object::Component(Component {
        unit: 3,
        ..match sch.object(Id(1)).unwrap().1 {
            Object::Component(c) => c.clone(),
            _ => panic!(),
        }
    });
    assert!(sch.replace(Id(1), bad_unit).is_err());
    let bad_repr = Object::Component(Component {
        representation: REPR_ALTERNATE,
        ..match sch.object(Id(1)).unwrap().1 {
            Object::Component(c) => c.clone(),
            _ => panic!(),
        }
    });
    assert!(sch.replace(Id(1), bad_repr).is_err());
}

#[test]
fn a_wire_to_a_shared_pin_connects_both_units() {
    let mut sch = Schematic::new(Id(0));
    sch.add_symbol(gate());
    placed(&mut sch, 1, 1, 0);
    placed(&mut sch, 2, 2, 100);
    // Shared pin 3 of unit instance 1 is at x=10, of instance 2 at x=110.
    sch.insert(
        Id(0),
        Id(3),
        Object::Wire(Wire {
            segment: Segment::new(Point::new(10, 0), Point::new(110, 0)),
        }),
    )
    .unwrap();
    let conn = Connectivity::build(&sch);
    let n1 = conn.net_of_pin(Id(1), "3").unwrap();
    let n2 = conn.net_of_pin(Id(2), "3").unwrap();
    assert_eq!(
        n1.id, n2.id,
        "the shared pin is one physical pin on one net"
    );
}
