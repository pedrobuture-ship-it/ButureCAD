//! Creates a small demo project: `cargo run -p buturecad-desktop --example demo_project -- <dir>`.
//! A 3V3 supply feeding an LED through a resistor, with a decoupling capacitor and a connector.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::{LayerId, Track, Via};
use buturecad_eda_core::{CommandEngine, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_io::{engine, ProjectSession};
use buturecad_schematic::{
    builtin, Component, Direction, Junction, NetLabel, Object, Placement, PowerSymbol,
};

const G: i64 = 1_270_000;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: demo_project <dir>");
    let (session, project) = ProjectSession::create(&dir, "Demo").expect("create");
    let mut e = CommandEngine::new(project);
    e.execute(AddSymbolsCommand::new(builtin::symbols()))
        .unwrap();
    let pt = |x: i64, y: i64| Point::new(x * G, y * G);

    let part = |e: &mut CommandEngine, lib: &str, r: &str, v: &str, x: i64, y: i64, rot: u8| {
        let id = e.new_id();
        let c = Component {
            lib_id: lib.into(),
            reference: r.into(),
            value: v.into(),
            footprint: String::new(),
            properties: Default::default(),
            unit: 1,
            representation: 1,
            placement: Placement {
                position: pt(x, y),
                rotation: rot,
                mirror: false,
            },
        };
        e.execute(place_component(ROOT_SHEET, id, c)).unwrap();
    };
    // +3V3 → R1 → D1 → GND, a decoupling capacitor across the rail, and a power connector.
    part(&mut e, "Buture:R", "R1", "330", 20, 24, 0); // pins (20,27) / (20,21)
    part(&mut e, "Buture:LED", "D1", "GREEN", 20, 14, 1); // anode (20,17), cathode (20,11)
    part(&mut e, "Buture:C", "C1", "100n", 32, 19, 0); // pins (32,22) / (32,16)
    part(&mut e, "Buture:Conn_01x02", "J1", "PWR", 4, 19, 0); // pins (0,20) / (0,18)

    let wire = |e: &mut CommandEngine, a: (i64, i64), b: (i64, i64)| {
        let id = e.new_id();
        e.execute(create_wire(
            ROOT_SHEET,
            id,
            Segment::new(pt(a.0, a.1), pt(b.0, b.1)),
        ))
        .unwrap();
    };
    wire(&mut e, (20, 30), (20, 27)); // +3V3 → R1.1
    wire(&mut e, (20, 21), (20, 17)); // R1.2 → D1 anode
    wire(&mut e, (20, 11), (20, 8)); // D1 cathode → GND
    wire(&mut e, (0, 20), (0, 30)); // J1.1 up to the rail
    wire(&mut e, (0, 30), (20, 30));
    wire(&mut e, (0, 18), (0, 8)); // J1.2 down to ground
    wire(&mut e, (0, 8), (20, 8));
    wire(&mut e, (32, 22), (32, 29)); // C1 across the rail …
    wire(&mut e, (32, 29), (20, 29)); // … tapping the +3V3 wire (needs a junction)
    wire(&mut e, (32, 16), (32, 8)); // … and ground
    wire(&mut e, (32, 8), (20, 8));

    let put = |e: &mut CommandEngine, o: Object| {
        let id = e.new_id();
        e.execute(create_object("Add", ROOT_SHEET, id, o)).unwrap();
    };
    put(
        &mut e,
        Object::PowerSymbol(PowerSymbol {
            lib_id: "Buture:GND".into(),
            net_name: "GND".into(),
            placement: Placement::at(pt(20, 8)),
        }),
    );
    put(
        &mut e,
        Object::PowerSymbol(PowerSymbol {
            lib_id: "Buture:+3V3".into(),
            net_name: "+3V3".into(),
            placement: Placement::at(pt(20, 30)),
        }),
    );
    put(
        &mut e,
        Object::NetLabel(NetLabel {
            position: pt(8, 30),
            name: "VIN".into(),
            orientation: Direction::Right,
        }),
    );
    put(
        &mut e,
        Object::Junction(Junction {
            position: pt(20, 29),
        }),
    );
    put(
        &mut e,
        Object::Junction(Junction {
            position: pt(20, 8),
        }),
    );

    // A little copper so the PCB view has something real to show.
    e.execute(CreateTrackCommand::new(Track {
        layer: LayerId(0),
        segment: Segment::new(pt(0, 0), pt(30, 0)),
        width: 300_000,
        net: None,
    }))
    .unwrap();
    e.execute(CreateTrackCommand::new(Track {
        layer: LayerId(1),
        segment: Segment::new(pt(30, 0), pt(30, -20)),
        width: 300_000,
        net: None,
    }))
    .unwrap();
    e.execute(CreateViaCommand::new(Via {
        position: pt(30, 0),
        diameter: 800_000,
        drill: 400_000,
        net: None,
        kind: Default::default(),
    }))
    .unwrap();
    engine::save(&session, &mut e).expect("save");
    println!("demo project written to {dir}");
}
