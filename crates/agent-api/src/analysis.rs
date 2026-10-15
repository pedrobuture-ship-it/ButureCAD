//! ANALYSIS: ERC, DRC and measurements. All run on the real connectivity graph / geometry.

use buturecad_eda_core::model::Id;
use buturecad_eda_core::Project;
use buturecad_schematic::Connectivity;
use serde_json::{json, Value};

pub struct Finding {
    pub severity: String,
    pub rule: String,
    pub message: String,
}

fn sev(s: impl std::fmt::Debug) -> String {
    format!("{s:?}").to_lowercase()
}

pub fn erc_findings(p: &Project) -> Vec<Finding> {
    let sch = p.schematic();
    let conn = Connectivity::build(sch);
    buturecad_rules::run_erc(sch, &conn, sch.waivers())
        .violations
        .into_iter()
        .filter(|v| v.waived.is_none())
        .map(|v| Finding {
            severity: sev(v.severity),
            rule: v.rule.code().into(),
            message: v.message,
        })
        .collect()
}

pub fn drc_findings(p: &Project) -> Vec<Finding> {
    buturecad_rules::run_drc(p.pcb())
        .violations
        .into_iter()
        .map(|v| Finding {
            severity: sev(v.severity),
            rule: v.rule.code().into(),
            message: v.message,
        })
        .collect()
}

fn findings_json(f: &[Finding]) -> Value {
    json!({
        "errors": f.iter().filter(|x| x.severity == "error").count(),
        "warnings": f.iter().filter(|x| x.severity == "warning").count(),
        "findings": f.iter().map(|x| json!({"severity": x.severity, "rule": x.rule, "message": x.message})).collect::<Vec<_>>(),
    })
}

pub fn run_erc(p: &Project) -> Value {
    findings_json(&erc_findings(p))
}

pub fn run_drc(p: &Project) -> Value {
    findings_json(&drc_findings(p))
}

pub fn get_violations(p: &Project) -> Value {
    json!({"erc": run_erc(p), "drc": run_drc(p)})
}

fn net_id(p: &Project, name: &str) -> Option<Id> {
    p.pcb()
        .nets
        .iter()
        .find(|(_, n)| n.name == name)
        .map(|(i, _)| *i)
}

/// Routed length, vias, pads and unrouted connections of a board net.
pub fn measure_net(p: &Project, name: &str) -> Result<Value, String> {
    let doc = p.pcb();
    let id = net_id(p, name).ok_or_else(|| {
        format!(
            "no board net named “{name}” (nets: {})",
            doc.nets
                .values()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let tracks: Vec<_> = doc.tracks.values().filter(|t| t.net == Some(id)).collect();
    let mut per_layer: std::collections::BTreeMap<String, f64> = Default::default();
    for t in &tracks {
        *per_layer
            .entry(buturecad_pcb::layer::name(t.layer))
            .or_default() += t.segment.length();
    }
    let pads: usize = doc
        .placed
        .values()
        .map(|f| f.pad_nets.values().filter(|n| **n == id).count())
        .sum();
    let unrouted = buturecad_eda_core::ratsnest::net_ratsnest(doc, id).len();
    Ok(json!({
        "net": name,
        "track_segments": tracks.len(),
        "routed_length_nm": buturecad_eda_core::length::net_length(doc, id),
        "length_per_layer_nm": per_layer,
        "vias": doc.vias.values().filter(|v| v.net == Some(id)).count(),
        "pads": pads,
        "unrouted_connections": unrouted,
    }))
}

/// Length, width, layer and net of one track segment.
pub fn measure_track(p: &Project, id: u64) -> Result<Value, String> {
    let doc = p.pcb();
    let t = doc
        .tracks
        .get(&Id(id))
        .ok_or_else(|| format!("no track with id {id}"))?;
    Ok(json!({
        "id": id, "layer": buturecad_pcb::layer::name(t.layer),
        "from": [t.segment.a.x, t.segment.a.y], "to": [t.segment.b.x, t.segment.b.y],
        "length_nm": t.segment.length(), "width_nm": t.width,
        "net": t.net.and_then(|n| doc.nets.get(&n)).map(|n| n.name.clone()),
    }))
}
