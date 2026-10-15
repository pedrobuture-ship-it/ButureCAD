//! A structured diff between two project states. It compares the **states**, not the actions, so
//! it shows what would really change — including consequences the actions did not spell out.

use buturecad_eda_core::model::{Id, PcbDoc};
use buturecad_eda_core::Project;
use buturecad_schematic::Object;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffEntry {
    /// `added`, `removed` or `modified`.
    pub change: &'static str,
    /// `schematic`, `board`, `rules` or `library`.
    pub domain: &'static str,
    pub kind: String,
    pub id: Option<u64>,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub entries: Vec<DiffEntry>,
}

impl Diff {
    pub fn count(&self, change: &str) -> usize {
        self.entries.iter().filter(|e| e.change == change).count()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// `+ component C14 …` / `- …` / `~ …` lines.
    pub fn lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| {
                let sign = match e.change {
                    "added" => '+',
                    "removed" => '-',
                    _ => '~',
                };
                format!("{sign} [{}] {} {}", e.domain, e.kind, e.label)
            })
            .collect()
    }
}

fn mm(v: i64) -> String {
    format!("{:.3}", v as f64 / 1e6)
}

fn sch_label(o: &Object) -> String {
    match o {
        Object::Component(c) => format!(
            "{} ({}){}",
            c.reference,
            c.lib_id,
            if c.value.is_empty() {
                String::new()
            } else {
                format!(" = {}", c.value)
            }
        ),
        Object::GlobalLabel(l) => format!(
            "label {} at ({}, {})",
            l.name,
            mm(l.position.x),
            mm(l.position.y)
        ),
        Object::NetLabel(l) => format!(
            "label {} at ({}, {})",
            l.name,
            mm(l.position.x),
            mm(l.position.y)
        ),
        Object::PowerSymbol(p) => format!(
            "power {} at ({}, {})",
            p.net_name,
            mm(p.placement.position.x),
            mm(p.placement.position.y)
        ),
        other => other.kind().to_owned(),
    }
}

fn push<T: PartialEq>(
    out: &mut Vec<DiffEntry>,
    domain: &'static str,
    kind: &dyn Fn(&T) -> String,
    before: &BTreeMap<Id, T>,
    after: &BTreeMap<Id, T>,
    label: &dyn Fn(&T) -> String,
) {
    for (id, a) in after {
        match before.get(id) {
            None => out.push(DiffEntry {
                change: "added",
                domain,
                kind: kind(a),
                id: Some(id.0),
                label: label(a),
            }),
            Some(b) if b != a => out.push(DiffEntry {
                change: "modified",
                domain,
                kind: kind(a),
                id: Some(id.0),
                label: label(a),
            }),
            _ => {}
        }
    }
    for (id, b) in before {
        if !after.contains_key(id) {
            out.push(DiffEntry {
                change: "removed",
                domain,
                kind: kind(b),
                id: Some(id.0),
                label: label(b),
            });
        }
    }
}

fn board(out: &mut Vec<DiffEntry>, a: &PcbDoc, b: &PcbDoc) {
    let pad_label = |f: &buturecad_pcb::PlacedFootprint| {
        format!(
            "{} ({}) at ({}, {}) mm rot {}°{}",
            f.reference,
            f.footprint,
            mm(f.placement.position.x),
            mm(f.placement.position.y),
            u32::from(f.placement.rotation) * 90,
            if f.placement.flipped { " bottom" } else { "" }
        )
    };
    push(
        out,
        "board",
        &|_| "footprint".to_string(),
        &a.placed,
        &b.placed,
        &pad_label,
    );
    push(
        out,
        "board",
        &|_| "net".to_string(),
        &a.nets,
        &b.nets,
        &|n| n.name.clone(),
    );
    push(
        out,
        "board",
        &|_| "track".to_string(),
        &a.tracks,
        &b.tracks,
        &|t| {
            format!(
                "{:.3} mm from ({}, {}) to ({}, {}) on layer {}",
                t.segment.length() / 1e6,
                mm(t.segment.a.x),
                mm(t.segment.a.y),
                mm(t.segment.b.x),
                mm(t.segment.b.y),
                buturecad_pcb::layer::name(t.layer)
            )
        },
    );
    push(
        out,
        "board",
        &|_| "via".to_string(),
        &a.vias,
        &b.vias,
        &|v| {
            format!(
                "at ({}, {}) Ø{} / drill {}",
                mm(v.position.x),
                mm(v.position.y),
                mm(v.diameter),
                mm(v.drill)
            )
        },
    );
    push(
        out,
        "board",
        &|_| "zone".to_string(),
        &a.zones,
        &b.zones,
        &|z| {
            format!(
                "on {} ({} points)",
                buturecad_pcb::layer::name(z.layer),
                z.outline.len()
            )
        },
    );
    push(
        out,
        "board",
        &|_| "graphic".to_string(),
        &a.graphics,
        &b.graphics,
        &|g| format!("on {}", buturecad_pcb::layer::name(g.layer)),
    );
    if a.stackup != b.stackup {
        out.push(DiffEntry {
            change: "modified",
            domain: "board",
            kind: "stackup".into(),
            id: None,
            label: format!(
                "{} → {} copper layers",
                a.stackup.copper_layers, b.stackup.copper_layers
            ),
        });
    }
    for (name, x, y) in [
        (
            "min_track_width",
            a.rules.min_track_width,
            b.rules.min_track_width,
        ),
        (
            "min_via_diameter",
            a.rules.min_via_diameter,
            b.rules.min_via_diameter,
        ),
        (
            "min_via_drill",
            a.rules.min_via_drill,
            b.rules.min_via_drill,
        ),
        (
            "min_annular_ring",
            a.rules.min_annular_ring,
            b.rules.min_annular_ring,
        ),
        (
            "copper_to_edge",
            a.rules.copper_to_edge,
            b.rules.copper_to_edge,
        ),
    ] {
        if x != y {
            out.push(DiffEntry {
                change: "modified",
                domain: "rules",
                kind: "rule".into(),
                id: None,
                label: format!("{name}: {} → {} mm", mm(x), mm(y)),
            });
        }
    }
    if a.rules != b.rules && !out.iter().any(|e| e.domain == "rules") {
        out.push(DiffEntry {
            change: "modified",
            domain: "rules",
            kind: "rule".into(),
            id: None,
            label: "net classes or assignments".into(),
        });
    }
}

/// Everything that differs between `a` (before) and `b` (after).
pub fn diff(a: &Project, b: &Project) -> Diff {
    let mut out = Vec::new();
    let flat = |p: &Project| -> BTreeMap<Id, Object> {
        p.schematic()
            .sheets()
            .values()
            .flat_map(|s| s.objects.iter().map(|(i, o)| (*i, o.clone())))
            .collect()
    };
    push(
        &mut out,
        "schematic",
        &|o: &Object| match o {
            Object::Component(_) => "component".into(),
            Object::PowerSymbol(_) => "power symbol".into(),
            Object::GlobalLabel(_) | Object::NetLabel(_) => "label".into(),
            other => other.kind().to_owned(),
        },
        &flat(a),
        &flat(b),
        &sch_label,
    );
    for name in b.schematic().symbols().keys() {
        if !a.schematic().symbols().contains_key(name) {
            out.push(DiffEntry {
                change: "added",
                domain: "library",
                kind: "symbol".into(),
                id: None,
                label: name.clone(),
            });
        }
    }
    for name in b.pcb().footprints.keys() {
        if !a.pcb().footprints.contains_key(name) {
            out.push(DiffEntry {
                change: "added",
                domain: "library",
                kind: "footprint definition".into(),
                id: None,
                label: name.clone(),
            });
        }
    }
    board(&mut out, a.pcb(), b.pcb());
    Diff { entries: out }
}
