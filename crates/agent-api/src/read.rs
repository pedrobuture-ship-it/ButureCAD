//! READ: structured JSON views of the project. Agents read these instead of files.

use buturecad_eda_core::model::PcbDoc;
use buturecad_eda_core::Project;
use buturecad_schematic::{Connectivity, Object};
use serde_json::{json, Value};

fn mm(v: i64) -> f64 {
    v as f64 / 1e6
}

pub fn get_project(p: &Project, revision: u64) -> Value {
    let comps = components_list(p);
    json!({
        "name": p.name(),
        "revision": revision,
        "units": {"length": "nm (integer); *_mm fields are for reading only", "angle": "degrees"},
        "schematic_components": comps.len(),
        "footprints": p.pcb().placed.len(),
        "nets": p.pcb().nets.len(),
        "tracks": p.pcb().tracks.len(),
        "vias": p.pcb().vias.len(),
        "zones": p.pcb().zones.len(),
        "copper_layers": p.pcb().stackup.copper_layers,
    })
}

fn components_list(p: &Project) -> Vec<Value> {
    let mut v = Vec::new();
    for sheet in p.schematic().sheets().values() {
        for (id, o) in &sheet.objects {
            if let Object::Component(c) = o {
                let placed = p.pcb().placed.values().find(|f| f.reference == c.reference);
                v.push(json!({
                    "id": id.0, "reference": c.reference, "value": c.value, "symbol": c.lib_id,
                    "footprint": c.footprint, "properties": c.properties,
                    "schematic_position_nm": [c.placement.position.x, c.placement.position.y],
                    "board": placed.map(|f| json!({
                        "x": f.placement.position.x, "y": f.placement.position.y,
                        "rotation_deg": u32::from(f.placement.rotation) * 90,
                        "side": if f.placement.flipped { "bottom" } else { "top" }, "locked": f.locked,
                    })),
                }));
            }
        }
    }
    v.sort_by(|a, b| a["reference"].as_str().cmp(&b["reference"].as_str()));
    v
}

pub fn get_components(p: &Project) -> Value {
    json!({"components": components_list(p)})
}

pub fn get_schematic(p: &Project) -> Value {
    let sch = p.schematic();
    let conn = Connectivity::build(sch);
    let mut objects = Vec::new();
    for (sid, sheet) in sch.sheets() {
        for (id, o) in &sheet.objects {
            let item = match o {
                Object::Component(c) => {
                    json!({"type": "component", "reference": c.reference, "symbol": c.lib_id})
                }
                Object::GlobalLabel(l) => {
                    json!({"type": "label", "name": l.name, "at": [l.position.x, l.position.y]})
                }
                Object::NetLabel(l) => {
                    json!({"type": "label", "name": l.name, "at": [l.position.x, l.position.y]})
                }
                Object::PowerSymbol(pw) => {
                    json!({"type": "power", "name": pw.net_name, "at": [pw.placement.position.x, pw.placement.position.y]})
                }
                Object::Wire(w) => {
                    json!({"type": "wire", "from": [w.segment.a.x, w.segment.a.y], "to": [w.segment.b.x, w.segment.b.y]})
                }
                other => json!({"type": other.kind()}),
            };
            objects.push(json!({"id": id.0, "sheet": sid.0, "object": item}));
        }
    }
    json!({"sheets": sch.sheets().len(), "objects": objects, "nets": nets_json(&conn)})
}

fn nets_json(conn: &Connectivity) -> Value {
    Value::Array(
        conn.nets()
            .iter()
            .map(|n| json!({"name": n.name, "named": n.named, "power": n.power, "pins": n.pins.iter().map(|p| format!("{}.{}", p.reference, p.pin)).collect::<Vec<_>>()}))
            .collect(),
    )
}

pub fn get_nets(p: &Project) -> Value {
    let conn = Connectivity::build(p.schematic());
    json!({"nets": nets_json(&conn), "board_nets": p.pcb().nets.values().map(|n| n.name.clone()).collect::<Vec<_>>()})
}

fn board_json(doc: &PcbDoc) -> Value {
    let loops = buturecad_eda_core::outline::outline_loops(doc);
    let outline: Vec<Vec<[i64; 2]>> = loops
        .iter()
        .map(|l| l.iter().map(|p| [p.x, p.y]).collect())
        .collect();
    let mut footprints = Vec::new();
    for (id, f) in &doc.placed {
        let pads: Vec<Value> = match doc.footprints.get(&f.footprint) {
            Some(def) => f
                .world_pads(def)
                .iter()
                .map(|w| {
                    let net = f.pad_nets.get(&w.number).and_then(|n| doc.nets.get(n)).map(|n| n.name.clone());
                    json!({"number": w.number, "x": w.center.x, "y": w.center.y, "width": w.width, "height": w.height, "net": net})
                })
                .collect(),
            None => Vec::new(),
        };
        footprints.push(json!({
            "id": id.0, "reference": f.reference, "footprint": f.footprint, "value": f.value,
            "x": f.placement.position.x, "y": f.placement.position.y,
            "rotation_deg": u32::from(f.placement.rotation) * 90,
            "side": if f.placement.flipped { "bottom" } else { "top" }, "locked": f.locked, "pads": pads,
        }));
    }
    json!({
        "copper_layers": doc.stackup.copper_layers,
        "layers": doc.stackup.copper().iter().map(|l| buturecad_pcb::layer::name(*l)).collect::<Vec<_>>(),
        "thickness_nm": doc.physical.thickness(),
        "outline": outline,
        "footprints": footprints,
        "tracks": doc.tracks.iter().map(|(id, t)| json!({
            "id": id.0, "layer": buturecad_pcb::layer::name(t.layer), "from": [t.segment.a.x, t.segment.a.y], "to": [t.segment.b.x, t.segment.b.y],
            "width": t.width, "net": t.net.and_then(|n| doc.nets.get(&n)).map(|n| n.name.clone()),
        })).collect::<Vec<_>>(),
        "vias": doc.vias.iter().map(|(id, v)| json!({
            "id": id.0, "x": v.position.x, "y": v.position.y, "diameter": v.diameter, "drill": v.drill,
            "net": v.net.and_then(|n| doc.nets.get(&n)).map(|n| n.name.clone()),
        })).collect::<Vec<_>>(),
        "zones": doc.zones.iter().map(|(id, z)| json!({
            "id": id.0, "layer": buturecad_pcb::layer::name(z.layer), "outline": z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            "net": z.net.and_then(|n| doc.nets.get(&n)).map(|n| n.name.clone()), "filled": z.fill.is_some(),
        })).collect::<Vec<_>>(),
    })
}

pub fn get_board(p: &Project) -> Value {
    board_json(p.pcb())
}

pub fn get_rules(p: &Project) -> Value {
    let r = &p.pcb().rules;
    json!({
        "min_track_width": r.min_track_width, "min_via_diameter": r.min_via_diameter, "min_via_drill": r.min_via_drill,
        "min_annular_ring": r.min_annular_ring, "copper_to_edge": r.copper_to_edge,
        "classes": r.classes.values().map(|c| json!({
            "name": c.name, "track_width": c.track_width, "clearance": c.clearance, "via_diameter": c.via_diameter, "via_drill": c.via_drill,
        })).collect::<Vec<_>>(),
        "assignments": r.assignments.iter().filter_map(|(n, c)| p.pcb().nets.get(n).map(|net| json!({"net": net.name, "class": c}))).collect::<Vec<_>>(),
        "note_mm": mm(r.min_track_width),
    })
}
