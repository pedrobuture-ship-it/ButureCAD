//! Inspector summaries of model objects.

use crate::ui_types::{Mode, ObjectSummary, PropertyInfo};
use buturecad_eda_core::Project;
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{Connectivity, Id, Object, PinType};

fn mm(v: i64) -> String {
    format!("{:.3}", v as f64 / 1_000_000.0)
}

fn pos(p: Point) -> String {
    format!("{}, {} mm", mm(p.x), mm(p.y))
}

fn prop(key: &str, label: &str, value: impl Into<String>, editable: bool) -> PropertyInfo {
    PropertyInfo {
        key: key.into(),
        label: label.into(),
        value: value.into(),
        editable,
        options: Vec::new(),
    }
}

fn choice(key: &str, label: &str, value: String, options: &[(String, String)]) -> PropertyInfo {
    PropertyInfo {
        options: options
            .iter()
            .map(|(v, l)| crate::ui_types::OptionInfo {
                value: v.clone(),
                label: l.clone(),
            })
            .collect(),
        ..prop(key, label, value, true)
    }
}

fn segment_props(s: &Segment) -> Vec<PropertyInfo> {
    vec![
        prop("start", "Start", pos(s.a), false),
        prop("end", "End", pos(s.b), false),
        prop(
            "length",
            "Length",
            format!("{} mm", mm(s.length().round() as i64)),
            false,
        ),
    ]
}

fn pin_kind(k: PinType) -> &'static str {
    match k {
        PinType::Input => "input",
        PinType::Output => "output",
        PinType::Bidirectional => "bidirectional",
        PinType::TriState => "tri-state",
        PinType::Passive => "passive",
        PinType::PowerInput => "power input",
        PinType::PowerOutput => "power output",
        PinType::OpenCollector => "open collector",
        PinType::OpenEmitter => "open emitter",
        PinType::Unspecified => "unspecified",
        PinType::NoConnect => "no connect",
    }
}

pub fn summarize(
    project: &Project,
    mode: Mode,
    id: Id,
    conn: Option<&Connectivity>,
) -> Option<ObjectSummary> {
    let (kind, title, properties) = match mode {
        Mode::Schematic => {
            let sch = project.schematic();
            let (_, obj) = sch.object(id)?;
            let net_of = |pred: &dyn Fn(&buturecad_schematic::Net) -> bool| {
                conn.and_then(|c| c.nets().iter().find(|n| pred(n)))
                    .map(|n| n.name.clone())
            };
            let mut props = Vec::new();
            let title = match obj {
                Object::Component(c) => {
                    props.push(prop("reference", "Reference", &c.reference, true));
                    props.push(prop("value", "Value", &c.value, true));
                    props.push(prop("footprint", "Footprint", &c.footprint, true));
                    if let Some(sym) = sch.symbol(&c.lib_id) {
                        if sym.unit_count > 1 {
                            let units: Vec<(String, String)> = (1..=sym.unit_count)
                                .map(|u| {
                                    (u.to_string(), format!("Unit {}", (b'A' + u - 1) as char))
                                })
                                .collect();
                            props.push(choice("unit", "Unit", c.unit.to_string(), &units));
                        }
                        if sym.has_alternate() {
                            let reprs = vec![
                                ("1".to_owned(), "Normal".to_owned()),
                                ("2".to_owned(), "Alternate".to_owned()),
                            ];
                            props.push(choice(
                                "representation",
                                "Representation",
                                c.representation.to_string(),
                                &reprs,
                            ));
                        }
                    }
                    props.push(prop(
                        "lib_id",
                        "Symbol",
                        if c.lib_id.is_empty() {
                            "(none)"
                        } else {
                            &c.lib_id
                        },
                        false,
                    ));
                    props.push(prop(
                        "position",
                        "Position",
                        pos(c.placement.position),
                        false,
                    ));
                    props.push(prop(
                        "rotation",
                        "Rotation",
                        format!("{}°", c.placement.rotation as u32 * 90),
                        false,
                    ));
                    props.push(prop(
                        "mirror",
                        "Mirrored",
                        if c.placement.mirror { "yes" } else { "no" },
                        false,
                    ));
                    if let Some(conn) = conn {
                        for pin in sch.placed_pins(c) {
                            let net = conn
                                .net_of_pin(id, &pin.number)
                                .map_or("—".to_string(), |n| n.name.clone());
                            props.push(prop(
                                &format!("pin_{}", pin.number),
                                &format!(
                                    "Pin {} {} ({})",
                                    pin.number,
                                    pin.name,
                                    pin_kind(pin.kind)
                                ),
                                net,
                                false,
                            ));
                        }
                    }
                    c.reference.clone()
                }
                Object::Wire(w) => {
                    props.extend(segment_props(&w.segment));
                    if let Some(n) = net_of(&|n| n.wires.contains(&id)) {
                        props.push(prop("net", "Net", n, false));
                    }
                    "Wire".into()
                }
                Object::Bus(b) => {
                    props.extend(segment_props(&b.segment));
                    "Bus".into()
                }
                Object::Junction(j) => {
                    props.push(prop("position", "Position", pos(j.position), false));
                    "Junction".into()
                }
                Object::NoConnect(n) => {
                    props.push(prop("position", "Position", pos(n.position), false));
                    "No-connect".into()
                }
                Object::NetLabel(l) => {
                    props.push(prop("name", "Name", &l.name, true));
                    props.push(prop("position", "Position", pos(l.position), false));
                    format!("Label {}", l.name)
                }
                Object::GlobalLabel(l) => {
                    props.push(prop("name", "Name", &l.name, true));
                    props.push(prop("position", "Position", pos(l.position), false));
                    format!("Global label {}", l.name)
                }
                Object::HierLabel(l) => {
                    props.push(prop("name", "Name", &l.name, true));
                    props.push(prop("position", "Position", pos(l.position), false));
                    format!("Hierarchical label {}", l.name)
                }
                Object::PowerSymbol(p) => {
                    props.push(prop("net_name", "Net", &p.net_name, true));
                    props.push(prop("lib_id", "Symbol", &p.lib_id, false));
                    props.push(prop(
                        "position",
                        "Position",
                        pos(p.placement.position),
                        false,
                    ));
                    format!("Power {}", p.net_name)
                }
                Object::Sheet(s) => {
                    props.push(prop("name", "Name", &s.name, false));
                    props.push(prop("pins", "Pins", s.pins.len().to_string(), false));
                    format!("Sheet {}", s.name)
                }
            };
            (obj.kind().to_owned(), title, props)
        }
        Mode::Pcb => {
            if let Some(t) = project.pcb().tracks.get(&id) {
                let mut props = vec![
                    prop(
                        "layer",
                        "Layer",
                        crate::scene_builder::pcb_layer_name(t.layer),
                        false,
                    ),
                    prop("width", "Width", format!("{} mm", mm(t.width)), false),
                ];
                props.extend(segment_props(&t.segment));
                ("track".to_owned(), "Track".to_owned(), props)
            } else {
                let v = project.pcb().vias.get(&id)?;
                (
                    "via".to_owned(),
                    "Via".to_owned(),
                    vec![
                        prop("position", "Position", pos(v.position), false),
                        prop(
                            "diameter",
                            "Diameter",
                            format!("{} mm", mm(v.diameter)),
                            false,
                        ),
                        prop("drill", "Drill", format!("{} mm", mm(v.drill)), false),
                    ],
                )
            }
        }
        _ => return None,
    };
    Some(ObjectSummary {
        id: id.0,
        kind,
        title,
        properties,
    })
}
