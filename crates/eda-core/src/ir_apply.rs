//! Applying a [`Design`] to a project, **through the Command Engine**.
//!
//! Order: symbols → schematic (components, net labels / power symbols) → "Update PCB from
//! Schematic" (footprints and nets, via [`crate::sync`]) → board (outline, layers, placements,
//! tracks, vias, zones, rules). Every step is an undoable command. The target project must be
//! empty: this is generation, not merging (merging code with hand edits is a later stage).

use crate::commands::{
    AddSymbolsCommand, PcbEditCommand, SchematicEditCommand, SetRulesCommand, SetStackupCommand,
};
use crate::ir::{Design, Pt, Side};
use crate::model::{Id, PcbChange, PcbObject, Track, Via};
use crate::{CommandEngine, CommandError, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_pcb::{layer, BoardGraphic, Footprint, FpShape, LayerId, Net, Zone};
use buturecad_schematic::{
    builtin, Change, Component, Direction, GlobalLabel, Object, Placement, PowerSymbol,
};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Command(#[from] CommandError),
}

fn bad<T>(m: impl Into<String>) -> Result<T, ApplyError> {
    Err(ApplyError::Invalid(m.into()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub components: usize,
    pub nets: usize,
    pub tracks: usize,
    pub vias: usize,
    pub zones: usize,
    pub warnings: Vec<String>,
}

fn pt(p: Pt) -> Point {
    Point::new(p.x, p.y)
}

/// Default symbol for a reference prefix.
pub fn default_symbol(reference: &str) -> Option<&'static str> {
    let letters: String = reference
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    Some(match letters.to_ascii_uppercase().as_str() {
        "R" => "Buture:R",
        "C" => "Buture:C",
        "L" => "Buture:L",
        "V" => "Buture:V",
        "I" => "Buture:I",
        "D" => "Buture:D",
        "LED" => "Buture:LED",
        _ => return None,
    })
}

const POWER_SYMBOLS: [(&str, &str); 4] = [
    ("GND", "Buture:GND"),
    ("+3V3", "Buture:+3V3"),
    ("+5V", "Buture:+5V"),
    ("VCC", "Buture:VCC"),
];

fn layer_by_name(stack: &buturecad_pcb::Stackup, name: &str) -> Option<LayerId> {
    stack
        .all_layers()
        .into_iter()
        .find(|l| layer::name(*l).eq_ignore_ascii_case(name))
}

/// Applies `design` to the (empty) project of `engine`. `resolve_footprint` finds footprint
/// definitions by name (libraries, generators).
pub fn apply_design(
    engine: &mut CommandEngine,
    design: &Design,
    resolve_footprint: &dyn Fn(&str) -> Option<Footprint>,
) -> Result<ApplyReport, ApplyError> {
    let mut report = ApplyReport::default();
    {
        let p = engine.project();
        if p.schematic().object_count() != 0 || !p.pcb().placed.is_empty() {
            return bad("the target project is not empty; generate into a new project");
        }
    }

    // -- symbols ---------------------------------------------------------------------------------
    let all = builtin::symbols();
    let mut needed: Vec<String> = Vec::new();
    for c in &design.components {
        let sym = if c.symbol.is_empty() {
            match default_symbol(&c.reference) {
                Some(s) => s.to_owned(),
                None => {
                    return bad(format!(
                        "component {} needs a `symbol` (no default for this reference prefix)",
                        c.reference
                    ))
                }
            }
        } else {
            c.symbol.clone()
        };
        if !all.iter().any(|s| s.name == sym) {
            return bad(format!("component {}: unknown symbol “{sym}”", c.reference));
        }
        needed.push(sym);
    }
    for n in &design.nets {
        if let Some((_, s)) = POWER_SYMBOLS.iter().find(|(name, _)| *name == n.name) {
            needed.push((*s).to_owned());
        }
    }
    needed.sort();
    needed.dedup();
    let add: Vec<_> = all
        .into_iter()
        .filter(|s| needed.contains(&s.name))
        .collect();
    if !add.is_empty() {
        engine.execute(AddSymbolsCommand::new(add))?;
    }

    // -- schematic: components on a grid ---------------------------------------------------------
    const G: i64 = 1_270_000;
    let mut comps: BTreeMap<String, Component> = BTreeMap::new();
    let mut changes: Vec<Change> = Vec::new();
    let cols = 6usize;
    for (i, c) in design.components.iter().enumerate() {
        let sym = if c.symbol.is_empty() {
            default_symbol(&c.reference).unwrap_or_default().to_owned()
        } else {
            c.symbol.clone()
        };
        let comp = Component {
            lib_id: sym,
            reference: c.reference.clone(),
            value: c.value.clone(),
            footprint: c.footprint.clone(),
            properties: c.properties.clone(),
            unit: 1,
            representation: 1,
            placement: Placement::at(Point::new(
                ((i % cols) as i64 * 16 + 10) * G,
                ((i / cols) as i64 * 16 + 10) * G,
            )),
        };
        let id = engine.new_id();
        changes.push(Change::create(
            ROOT_SHEET,
            id,
            Object::Component(comp.clone()),
        ));
        comps.insert(c.reference.clone(), comp);
    }
    // -- schematic: nets as labels / power symbols at the pins -----------------------------------
    {
        let sch = engine.project().schematic();
        let mut labels: Vec<(Object, String)> = Vec::new();
        for n in &design.nets {
            for p in &n.pins {
                let comp = comps.get(&p.reference).ok_or_else(|| {
                    ApplyError::Invalid(format!("net {}: no component {}", n.name, p.reference))
                })?;
                let pins = sch.placed_pins(comp);
                let found: Vec<_> = pins.iter().filter(|x| x.number == p.pin).collect();
                let found = if found.is_empty() {
                    pins.iter().filter(|x| x.name == p.pin).collect::<Vec<_>>()
                } else {
                    found
                };
                let [pin] = found.as_slice() else {
                    return bad(format!(
                        "net {}: {}.{} {} (pins: {})",
                        n.name,
                        p.reference,
                        p.pin,
                        if found.is_empty() {
                            "does not exist"
                        } else {
                            "is ambiguous"
                        },
                        pins.iter()
                            .map(|x| format!("{}/{}", x.number, x.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                };
                let obj = match POWER_SYMBOLS.iter().find(|(name, _)| *name == n.name) {
                    Some((_, s)) => Object::PowerSymbol(PowerSymbol {
                        lib_id: (*s).to_owned(),
                        net_name: n.name.clone(),
                        placement: Placement::at(pin.position),
                    }),
                    None => Object::GlobalLabel(GlobalLabel {
                        position: pin.position,
                        name: n.name.clone(),
                        orientation: Direction::Right,
                    }),
                };
                labels.push((obj, n.name.clone()));
            }
        }
        for (obj, _) in labels {
            let id = engine.new_id();
            changes.push(Change::create(ROOT_SHEET, id, obj));
        }
    }
    if !changes.is_empty() {
        engine.execute(SchematicEditCommand::new(
            "ButureScript: schematic",
            changes,
        ))?;
    }
    report.components = design.components.len();
    report.nets = design.nets.len();

    // -- board: footprints and nets from the schematic -------------------------------------------
    let plan = crate::sync::plan_update(engine.project(), resolve_footprint);
    for w in plan.warnings() {
        report.warnings.push(w.text.clone());
    }
    plan.apply(engine)?;

    // -- board: layers, outline ------------------------------------------------------------------
    if let Some(layers) = design.board.as_ref().and_then(|b| b.layers) {
        let before = engine.project().pcb().stackup;
        let mut after = before;
        after.copper_layers = layers;
        if !after.is_valid() {
            return bad("board layers must be an even number from 2 to 32");
        }
        if before != after {
            engine.execute(SetStackupCommand::new(before, after))?;
        }
    }
    let mut pcb: Vec<PcbChange> = Vec::new();
    if let Some(b) = &design.board {
        if b.outline.len() < 3 {
            return bad("board outline needs at least 3 points");
        }
        for i in 0..b.outline.len() {
            let (a, c) = (pt(b.outline[i]), pt(b.outline[(i + 1) % b.outline.len()]));
            if a == c {
                return bad("board outline has a zero-length edge");
            }
            let id = engine.new_id();
            pcb.push(PcbChange::create(
                id,
                PcbObject::Graphic(BoardGraphic {
                    layer: layer::EDGE_CUTS,
                    shape: FpShape::Line {
                        segment: Segment::new(a, c),
                        width: 100_000,
                    },
                }),
            ));
        }
    }
    // -- board: placements -----------------------------------------------------------------------
    let placed: BTreeMap<String, Id> = engine
        .project()
        .pcb()
        .placed
        .iter()
        .map(|(id, f)| (f.reference.clone(), *id))
        .collect();
    for pl in &design.placements {
        let id = *placed.get(&pl.reference).ok_or_else(|| {
            ApplyError::Invalid(format!(
                "place: {} has no footprint on the board",
                pl.reference
            ))
        })?;
        if pl.rotation_deg.rem_euclid(90) != 0 {
            return bad(format!(
                "place {}: rotation must be a multiple of 90°",
                pl.reference
            ));
        }
        let before = engine.project().pcb().placed[&id].clone();
        let mut after = before.clone();
        after.placement.position = pt(pl.at);
        after.placement.rotation = (pl.rotation_deg.rem_euclid(360) / 90) as u8;
        after.placement.flipped = pl.side == Side::Bottom;
        pcb.push(PcbChange::modify(
            id,
            PcbObject::Footprint(before),
            PcbObject::Footprint(after),
        ));
    }
    // -- board: tracks, vias, zones --------------------------------------------------------------
    let net_ids: BTreeMap<String, Id> = engine
        .project()
        .pcb()
        .nets
        .iter()
        .map(|(id, n)| (n.name.clone(), *id))
        .collect();
    let mut extra_nets: BTreeMap<String, Id> = BTreeMap::new();
    let stack = engine.project().pcb().stackup;
    let mut net_for = |engine: &mut CommandEngine, name: &str, pcb: &mut Vec<PcbChange>| -> Id {
        if let Some(id) = net_ids.get(name).or_else(|| extra_nets.get(name)) {
            return *id;
        }
        // A net used only on the board (no pins in the schematic).
        let id = engine.new_id();
        pcb.push(PcbChange::create(
            id,
            PcbObject::Net(Net {
                name: name.to_owned(),
            }),
        ));
        extra_nets.insert(name.to_owned(), id);
        id
    };
    for t in &design.tracks {
        let l = layer_by_name(&stack, &t.layer).ok_or_else(|| {
            ApplyError::Invalid(format!("track on {}: unknown layer “{}”", t.net, t.layer))
        })?;
        if t.points.len() < 2 || t.width <= 0 {
            return bad(format!(
                "track on {}: needs two points and a positive width",
                t.net
            ));
        }
        let net = net_for(engine, &t.net, &mut pcb);
        for w in t.points.windows(2) {
            if w[0] == w[1] {
                continue;
            }
            let id = engine.new_id();
            pcb.push(PcbChange::create(
                id,
                PcbObject::Track(Track {
                    layer: l,
                    segment: Segment::new(pt(w[0]), pt(w[1])),
                    width: t.width,
                    net: Some(net),
                }),
            ));
        }
        report.tracks += 1;
    }
    for v in &design.vias {
        let net = net_for(engine, &v.net, &mut pcb);
        let id = engine.new_id();
        pcb.push(PcbChange::create(
            id,
            PcbObject::Via(Via {
                position: pt(v.at),
                diameter: v.diameter,
                drill: v.drill,
                net: Some(net),
                kind: Default::default(),
            }),
        ));
        report.vias += 1;
    }
    for z in &design.zones {
        let l = layer_by_name(&stack, &z.layer).ok_or_else(|| {
            ApplyError::Invalid(format!("zone {}: unknown layer “{}”", z.net, z.layer))
        })?;
        if z.outline.len() < 3 {
            return bad(format!("zone {}: outline needs at least 3 points", z.net));
        }
        let net = net_for(engine, &z.net, &mut pcb);
        let mut zone = Zone::new(l, z.outline.iter().map(|p| pt(*p)).collect(), Some(net));
        zone.clearance = z.clearance;
        if let Some(w) = z.min_width {
            zone.min_width = w;
        }
        zone.priority = z.priority;
        let id = engine.new_id();
        pcb.push(PcbChange::create(id, PcbObject::Zone(zone)));
        report.zones += 1;
    }
    if !pcb.is_empty() {
        engine.execute(PcbEditCommand::new("ButureScript: board", pcb))?;
    }
    if !design.zones.is_empty() {
        let fill = crate::zonefill::fill_changes(engine.project().pcb(), None, true);
        if !fill.is_empty() {
            engine.execute(PcbEditCommand::new("ButureScript: fill zones", fill))?;
        }
    }

    // -- rules -----------------------------------------------------------------------------------
    let r = &design.rules;
    let has_rules = r.min_track_width.is_some()
        || r.min_via_diameter.is_some()
        || r.min_via_drill.is_some()
        || r.min_annular_ring.is_some()
        || r.copper_to_edge.is_some()
        || r.clearance.is_some()
        || !r.classes.is_empty();
    if has_rules {
        let before = engine.project().pcb().rules.clone();
        let mut after = before.clone();
        macro_rules! set {
            ($field:ident) => {
                if let Some(v) = r.$field {
                    after.$field = v;
                }
            };
        }
        set!(min_track_width);
        set!(min_via_diameter);
        set!(min_via_drill);
        set!(min_annular_ring);
        set!(copper_to_edge);
        let nets_now: BTreeMap<String, Id> = engine
            .project()
            .pcb()
            .nets
            .iter()
            .map(|(id, n)| (n.name.clone(), *id))
            .collect();
        if let Some(c) = r.clearance {
            if let Some(d) = after.classes.get_mut(buturecad_pcb::DEFAULT_CLASS) {
                d.clearance = c;
            }
        }
        for class in &r.classes {
            let nc = after
                .classes
                .entry(class.name.clone())
                .or_insert_with(|| buturecad_pcb::NetClass::new(class.name.clone()));
            if let Some(v) = class.track_width {
                nc.track_width = v;
            }
            if let Some(v) = class.clearance {
                nc.clearance = v;
            }
            if let Some(v) = class.via_diameter {
                nc.via_diameter = v;
            }
            if let Some(v) = class.via_drill {
                nc.via_drill = v;
            }
            for n in &class.nets {
                let id = nets_now.get(n).ok_or_else(|| {
                    ApplyError::Invalid(format!("class {}: no net “{n}”", class.name))
                })?;
                after.assignments.insert(*id, class.name.clone());
            }
        }
        engine.execute(SetRulesCommand::new("ButureScript: rules", before, after))?;
    }
    Ok(report)
}
