//! Turns one [`Action`] into Command Engine commands. Used twice with the same code: on the scratch
//! copy during validation and on the real engine when the user approves, so what was reviewed is
//! what is applied.

use crate::action::Action;
use crate::FootprintResolver;
use buturecad_eda_core::commands::{
    delete_objects, place_component, PcbEditCommand, SchematicEditCommand, SetRulesCommand,
};
use buturecad_eda_core::ir_apply::{ensure_symbols, find_pin, layer_by_name, net_marker};
use buturecad_eda_core::model::{Id, PcbChange, PcbObject, Track, Via};
use buturecad_eda_core::{CommandEngine, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::Zone;
use buturecad_schematic::{Change, Component, Connectivity, Object, Placement};

const G: i64 = 1_270_000;

fn find_component(engine: &CommandEngine, reference: &str) -> Result<(Id, Component), String> {
    for sheet in engine.project().schematic().sheets().values() {
        for (id, o) in &sheet.objects {
            if let Object::Component(c) = o {
                if c.reference == reference {
                    return Ok((*id, c.clone()));
                }
            }
        }
    }
    Err(format!("no component with reference “{reference}”"))
}

fn footprint_id(engine: &CommandEngine, reference: &str) -> Result<Id, String> {
    engine
        .project()
        .pcb()
        .placed
        .iter()
        .find(|(_, f)| f.reference == reference)
        .map(|(i, _)| *i)
        .ok_or_else(|| format!("{reference} has no footprint on the board yet (does the component have a footprint?)"))
}

fn net_id(engine: &CommandEngine, name: &str) -> Result<Id, String> {
    let doc = engine.project().pcb();
    doc.nets
        .iter()
        .find(|(_, n)| n.name == name)
        .map(|(i, _)| *i)
        .ok_or_else(|| {
            format!(
                "no board net named “{name}” (nets: {})",
                doc.nets
                    .values()
                    .map(|n| n.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn pt(p: [i64; 2]) -> Point {
    Point::new(p[0], p[1])
}

/// Brings footprints and nets in line with the schematic ("Update PCB from Schematic").
pub fn sync_board(engine: &mut CommandEngine, resolve: &FootprintResolver) -> Result<(), String> {
    let plan = buturecad_eda_core::sync::plan_update(engine.project(), &|n| resolve(n));
    if plan.is_empty() {
        return Ok(());
    }
    plan.apply(engine).map_err(|e| e.to_string())
}

/// Executes `action`. `Ok(note)` describes the result (e.g. the reference that was chosen).
pub fn perform(engine: &mut CommandEngine, action: &Action) -> Result<String, String> {
    match action {
        Action::CreateComponent {
            reference,
            symbol,
            value,
            footprint,
            properties,
        } => {
            ensure_symbols(engine, std::slice::from_ref(symbol)).map_err(|e| e.to_string())?;
            let sch = engine.project().schematic();
            let sym = sch.symbol(symbol).ok_or("symbol missing after install")?;
            let prefix = if sym.reference_prefix.is_empty() {
                "U".to_owned()
            } else {
                sym.reference_prefix.clone()
            };
            let reference = match reference {
                Some(r) => {
                    if find_component(engine, r).is_ok() {
                        return Err(format!("a component {r} already exists"));
                    }
                    r.clone()
                }
                None => sch.next_reference(&prefix),
            };
            // A free spot below everything already placed.
            let (mut max_y, mut count) = (0i64, 0i64);
            for s in sch.sheets().values() {
                for o in s.objects.values() {
                    if let Object::Component(c) = o {
                        max_y = max_y.max(c.placement.position.y);
                        count += 1;
                    }
                }
            }
            let (col, row) = (count % 6, if count == 0 { 0 } else { 1 });
            let y = if count == 0 {
                10 * G
            } else {
                max_y + (16 + row - 1) * G
            };
            let comp = Component {
                lib_id: symbol.clone(),
                reference: reference.clone(),
                value: value.clone(),
                footprint: footprint.clone(),
                properties: properties.clone(),
                unit: 1,
                representation: 1,
                placement: Placement::at(Point::new((col * 16 + 10) * G, y)),
            };
            let id = engine.new_id();
            engine
                .execute(place_component(ROOT_SHEET, id, comp))
                .map_err(|e| e.to_string())?;
            Ok(format!("created {reference}"))
        }
        Action::RemoveComponent { reference } => {
            let (id, comp) = find_component(engine, reference)?;
            let sch = engine.project().schematic();
            let pins: Vec<Point> = sch.placed_pins(&comp).iter().map(|p| p.position).collect();
            let mut ids = vec![id];
            for s in sch.sheets().values() {
                for (oid, o) in &s.objects {
                    let at = match o {
                        Object::GlobalLabel(l) => Some(l.position),
                        Object::NetLabel(l) => Some(l.position),
                        Object::PowerSymbol(p) => Some(p.placement.position),
                        _ => None,
                    };
                    if at.is_some_and(|a| pins.contains(&a)) {
                        ids.push(*oid);
                    }
                }
            }
            let cmd = delete_objects(sch, &ids).map_err(|e| e.to_string())?;
            engine.execute(cmd).map_err(|e| e.to_string())?;
            Ok(format!(
                "removed {reference} and {} label(s) on its pins",
                ids.len() - 1
            ))
        }
        Action::Connect { net, pins } => {
            if net.trim().is_empty() {
                return Err("the net needs a name".into());
            }
            let mut needed: Vec<String> = Vec::new();
            if let Some(s) = buturecad_eda_core::ir_apply::power_symbol_for(net) {
                needed.push(s.to_owned());
            }
            ensure_symbols(engine, &needed).map_err(|e| e.to_string())?;
            let conn = Connectivity::build(engine.project().schematic());
            let mut changes = Vec::new();
            let mut positions: Vec<Point> = Vec::new();
            for p in pins {
                let (cid, comp) = find_component(engine, &p.reference)?;
                let pin = find_pin(engine.project().schematic(), &comp, &p.pin)?;
                if let Some(n) = conn.net_of_pin(cid, &pin.number) {
                    if n.name == *net {
                        continue; // already there
                    }
                    if n.named {
                        return Err(format!(
                            "{}.{} is already on net {}; disconnect it first",
                            p.reference, p.pin, n.name
                        ));
                    }
                }
                if positions.contains(&pin.position) {
                    continue;
                }
                positions.push(pin.position);
                let marker = net_marker(net, pin.position);
                changes.push((marker, pin.position));
            }
            if changes.is_empty() {
                return Ok(format!("{net}: pins were already connected"));
            }
            let n = changes.len();
            let mut edits = Vec::new();
            for (obj, _) in changes {
                let id = engine.new_id();
                edits.push(Change::create(ROOT_SHEET, id, obj));
            }
            engine
                .execute(SchematicEditCommand::new(
                    format!("Agent: connect to {net}"),
                    edits,
                ))
                .map_err(|e| e.to_string())?;
            Ok(format!("connected {n} pin(s) to {net}"))
        }
        Action::Disconnect { reference, pin } => {
            let (_, comp) = find_component(engine, reference)?;
            let sch = engine.project().schematic();
            let at = find_pin(sch, &comp, pin)?.position;
            let mut ids = Vec::new();
            for s in sch.sheets().values() {
                for (oid, o) in &s.objects {
                    let p = match o {
                        Object::GlobalLabel(l) => Some(l.position),
                        Object::NetLabel(l) => Some(l.position),
                        Object::PowerSymbol(p) => Some(p.placement.position),
                        _ => None,
                    };
                    if p == Some(at) {
                        ids.push(*oid);
                    }
                }
            }
            if ids.is_empty() {
                return Err(format!("{reference}.{pin} has no label or power symbol on it; if it is connected by wires, edit them in the visual editor"));
            }
            let cmd = delete_objects(sch, &ids).map_err(|e| e.to_string())?;
            engine.execute(cmd).map_err(|e| e.to_string())?;
            Ok(format!("disconnected {reference}.{pin}"))
        }
        Action::Place {
            reference,
            x,
            y,
            rotation_deg,
            bottom,
        } => {
            if rotation_deg.rem_euclid(90) != 0 {
                return Err("rotation must be a multiple of 90°".into());
            }
            let id = footprint_id(engine, reference)?;
            let before = engine.project().pcb().placed[&id].clone();
            if before.locked {
                return Err(format!("{reference} is locked"));
            }
            let mut after = before.clone();
            after.placement.position = Point::new(*x, *y);
            after.placement.rotation = (rotation_deg.rem_euclid(360) / 90) as u8;
            after.placement.flipped = *bottom;
            engine
                .execute(PcbEditCommand::new(
                    format!("Agent: place {reference}"),
                    vec![PcbChange::modify(
                        id,
                        PcbObject::Footprint(before),
                        PcbObject::Footprint(after),
                    )],
                ))
                .map_err(|e| e.to_string())?;
            Ok(format!("placed {reference}"))
        }
        Action::Move { reference, dx, dy } => {
            let id = footprint_id(engine, reference)?;
            let before = engine.project().pcb().placed[&id].clone();
            if before.locked {
                return Err(format!("{reference} is locked"));
            }
            let mut after = before.clone();
            after.placement.position = Point::new(
                before.placement.position.x + dx,
                before.placement.position.y + dy,
            );
            engine
                .execute(PcbEditCommand::new(
                    format!("Agent: move {reference}"),
                    vec![PcbChange::modify(
                        id,
                        PcbObject::Footprint(before),
                        PcbObject::Footprint(after),
                    )],
                ))
                .map_err(|e| e.to_string())?;
            Ok(format!("moved {reference}"))
        }
        Action::Route {
            net,
            layer,
            points,
            width,
        } => {
            if points.len() < 2 || *width <= 0 {
                return Err("a route needs at least two points and a positive width".into());
            }
            let nid = net_id(engine, net)?;
            let l = layer_by_name(&engine.project().pcb().stackup, layer)
                .ok_or_else(|| format!("unknown layer “{layer}”"))?;
            if !engine.project().pcb().stackup.has_copper(l) {
                return Err(format!("{layer} is not a copper layer of this board"));
            }
            let mut changes = Vec::new();
            for w in points.windows(2) {
                if w[0] == w[1] {
                    continue;
                }
                let id = engine.new_id();
                changes.push(PcbChange::create(
                    id,
                    PcbObject::Track(Track {
                        layer: l,
                        segment: Segment::new(pt(w[0]), pt(w[1])),
                        width: *width,
                        net: Some(nid),
                    }),
                ));
            }
            let n = changes.len();
            engine
                .execute(PcbEditCommand::new(format!("Agent: route {net}"), changes))
                .map_err(|e| e.to_string())?;
            Ok(format!("routed {net}: {n} segment(s)"))
        }
        Action::CreateVia {
            net,
            x,
            y,
            diameter,
            drill,
        } => {
            if *drill <= 0 || diameter <= drill {
                return Err("a via needs 0 < drill < diameter".into());
            }
            let nid = net_id(engine, net)?;
            let id = engine.new_id();
            engine
                .execute(PcbEditCommand::new(
                    format!("Agent: via on {net}"),
                    vec![PcbChange::create(
                        id,
                        PcbObject::Via(Via {
                            position: Point::new(*x, *y),
                            diameter: *diameter,
                            drill: *drill,
                            net: Some(nid),
                            kind: Default::default(),
                        }),
                    )],
                ))
                .map_err(|e| e.to_string())?;
            Ok(format!("via on {net}"))
        }
        Action::CreateZone {
            net,
            layer,
            outline,
            clearance,
            min_width,
        } => {
            if outline.len() < 3 {
                return Err("a zone outline needs at least 3 points".into());
            }
            let nid = net_id(engine, net)?;
            let l = layer_by_name(&engine.project().pcb().stackup, layer)
                .ok_or_else(|| format!("unknown layer “{layer}”"))?;
            if !engine.project().pcb().stackup.has_copper(l) {
                return Err(format!("{layer} is not a copper layer of this board"));
            }
            let mut z = Zone::new(l, outline.iter().map(|p| pt(*p)).collect(), Some(nid));
            z.clearance = *clearance;
            if let Some(w) = min_width {
                z.min_width = *w;
            }
            let id = engine.new_id();
            engine
                .execute(PcbEditCommand::new(
                    format!("Agent: zone {net}"),
                    vec![PcbChange::create(id, PcbObject::Zone(z))],
                ))
                .map_err(|e| e.to_string())?;
            let fill =
                buturecad_eda_core::zonefill::fill_changes(engine.project().pcb(), None, true);
            if !fill.is_empty() {
                engine
                    .execute(PcbEditCommand::new("Agent: fill zones", fill))
                    .map_err(|e| e.to_string())?;
            }
            Ok(format!("zone {net} on {layer}"))
        }
        Action::ChangeRule { rule, value } => {
            if *value <= 0 {
                return Err("a rule value must be positive".into());
            }
            let before = engine.project().pcb().rules.clone();
            let mut after = before.clone();
            match rule.as_str() {
                "min_track_width" => after.min_track_width = *value,
                "min_via_diameter" => after.min_via_diameter = *value,
                "min_via_drill" => after.min_via_drill = *value,
                "min_annular_ring" => after.min_annular_ring = *value,
                "copper_to_edge" => after.copper_to_edge = *value,
                "clearance" => {
                    after
                        .classes
                        .get_mut(buturecad_pcb::DEFAULT_CLASS)
                        .ok_or("the default net class is missing")?
                        .clearance = *value
                }
                other => return Err(format!("unknown rule “{other}” (min_track_width, min_via_diameter, min_via_drill, min_annular_ring, copper_to_edge, clearance)")),
            }
            engine
                .execute(SetRulesCommand::new(
                    format!("Agent: rule {rule}"),
                    before,
                    after,
                ))
                .map_err(|e| e.to_string())?;
            Ok(format!("rule {rule} changed"))
        }
    }
}

/// Runs all actions in order, syncing the board whenever a PCB action follows schematic changes
/// and once more at the end. Returns one note per action.
pub fn perform_all(
    engine: &mut CommandEngine,
    actions: &[Action],
    resolve: &FootprintResolver,
) -> Result<Vec<String>, (usize, String)> {
    let mut notes = Vec::new();
    let mut dirty = false;
    for (i, a) in actions.iter().enumerate() {
        if dirty && !a.touches_schematic() {
            sync_board(engine, resolve).map_err(|e| {
                (
                    i,
                    format!("updating the board from the schematic failed: {e}"),
                )
            })?;
            dirty = false;
        }
        notes.push(perform(engine, a).map_err(|e| (i, e))?);
        dirty |= a.touches_schematic();
    }
    if dirty {
        sync_board(engine, resolve).map_err(|e| {
            (
                actions.len(),
                format!("updating the board from the schematic failed: {e}"),
            )
        })?;
    }
    Ok(notes)
}
