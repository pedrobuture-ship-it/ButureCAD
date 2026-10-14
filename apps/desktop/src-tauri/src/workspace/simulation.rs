//! Simulation in the desktop app.
//!
//! The schematic's connectivity graph (not its drawing) becomes a flat `Circuit`; the chosen
//! backend of `buturecad-simulation` runs it; the result is plotted by the UI. SPICE models are
//! associated with components through component properties (see `docs/simulation.md`):
//!
//! * `Spice_Model` — model name (`D1N4148`) or a full `.model …`/`.subckt …` text that is added to
//!   the netlist; required for diodes, transistors, MOSFETs and subcircuits;
//! * `Spice_Value` — overrides the component value (resistance, source specification, …);
//! * `Spice_Nodes` — pin numbers in SPICE node order, e.g. `2,1,3` (default: pins sorted by number);
//! * `Spice_Enable` = `0` — leave the component out of the simulation.

use super::*;
use buturecad_schematic::connectivity::Connectivity;
use buturecad_schematic::{Object, Schematic};
use buturecad_simulation::circuit::parse_value;
use buturecad_simulation::{
    backends, Analysis, Circuit, Element, Kind, SimResult, SourceSpec, Value,
};
use std::collections::BTreeMap;

const GROUND_NAMES: [&str; 4] = ["GND", "0", "AGND", "DGND"];
const MAX_POINTS: usize = 1500;

#[derive(Default)]
pub(crate) struct SimState {
    analysis: String,
    backend: String,
    params: BTreeMap<String, String>,
    result: Option<(u64, SimResult)>,
    error: String,
    netlist: String,
    notes: Vec<String>,
    problems: Vec<String>,
    sources: Vec<String>,
    built_at: Option<u64>,
}

fn natural_key(s: &str) -> (u64, String) {
    (s.parse::<u64>().unwrap_or(u64::MAX), s.to_owned())
}

fn node_name(raw: &str, id: u32, taken: &mut BTreeMap<String, u32>) -> String {
    let mut n: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if n.is_empty() || n == "0" {
        n = format!("N{id}");
    }
    match taken.get(&n) {
        Some(other) if *other != id => n = format!("{n}_{id}"),
        _ => {}
    }
    taken.insert(n.clone(), id);
    n
}

/// Builds the circuit from the schematic. Returns the circuit plus notes and blocking problems.
pub(crate) fn build_circuit(
    sch: &Schematic,
    conn: &Connectivity,
    title: &str,
) -> (Circuit, Vec<String>, Vec<String>) {
    let mut notes = Vec::new();
    let mut problems = Vec::new();
    let mut taken = BTreeMap::new();
    // Node name of every net.
    let mut node_of = BTreeMap::new();
    for net in conn.nets() {
        let name = if GROUND_NAMES.contains(&net.name.to_ascii_uppercase().as_str()) {
            "0".to_owned()
        } else {
            node_name(&net.name, net.id.0, &mut taken)
        };
        node_of.insert(net.id, name);
    }
    // Pins by component.
    let mut by_comp: BTreeMap<buturecad_schematic::Id, (String, Vec<(String, String)>)> =
        BTreeMap::new();
    for net in conn.nets() {
        for p in &net.pins {
            by_comp
                .entry(p.component)
                .or_insert_with(|| (p.reference.clone(), Vec::new()))
                .1
                .push((p.pin.clone(), node_of[&net.id].clone()));
        }
    }
    let mut circuit = Circuit {
        title: title.to_owned(),
        ..Circuit::default()
    };
    for (id, (reference, mut pins)) in by_comp {
        let Some((_, Object::Component(comp))) = sch.object(id) else {
            continue;
        };
        if comp
            .properties
            .get("Spice_Enable")
            .is_some_and(|v| v.trim() == "0")
        {
            notes.push(format!("{reference}: left out (Spice_Enable = 0)"));
            continue;
        }
        let letter = reference.chars().find(|c| c.is_ascii_alphabetic());
        let Some(kind) = letter
            .and_then(Kind::from_prefix)
            .filter(|_| !reference.starts_with('#'))
        else {
            notes.push(format!(
                "{reference}: not a SPICE device (reference prefix), left out"
            ));
            continue;
        };
        pins.sort_by_key(|p| natural_key(&p.0));
        pins.dedup_by(|a, b| a.0 == b.0);
        let nodes: Vec<String> = if let Some(order) = comp.properties.get("Spice_Nodes") {
            let mut v = Vec::new();
            for num in order.split(',').map(str::trim) {
                match pins.iter().find(|p| p.0 == num) {
                    Some(p) => v.push(p.1.clone()),
                    None => {
                        v.push(format!("NC_{reference}_{num}"));
                        notes.push(format!("{reference}: pin {num} is not connected"));
                    }
                }
            }
            v
        } else {
            pins.iter().map(|p| p.1.clone()).collect()
        };
        if !kind.nodes_needed().contains(&nodes.len()) {
            problems.push(format!(
                "{reference}: {} connected pin(s), a {} needs {}–{}; connect all pins or set Spice_Nodes",
                nodes.len(),
                kind.letter(),
                kind.nodes_needed().start(),
                kind.nodes_needed().end()
            ));
            continue;
        }
        let text = comp
            .properties
            .get("Spice_Value")
            .unwrap_or(&comp.value)
            .trim()
            .to_owned();
        let value = match kind {
            Kind::R | Kind::C | Kind::L => match parse_value(&text) {
                Ok(v) => Value::Number(v),
                Err(e) => {
                    problems.push(format!("{reference}: value “{text}”: {e}"));
                    continue;
                }
            },
            Kind::V | Kind::I => match SourceSpec::parse(&text) {
                Ok(s) => Value::Source(s),
                Err(e) => {
                    problems.push(format!("{reference}: source “{text}”: {e}"));
                    continue;
                }
            },
            _ => match comp.properties.get("Spice_Model").map(|m| m.trim()) {
                None | Some("") => {
                    problems.push(format!(
                        "{reference}: no SPICE model (set the Spice_Model property)"
                    ));
                    continue;
                }
                Some(m) if m.starts_with('.') => {
                    let name = m.split_whitespace().nth(1).unwrap_or("").to_owned();
                    if name.is_empty() {
                        problems.push(format!("{reference}: Spice_Model has no model name"));
                        continue;
                    }
                    if !circuit.models.iter().any(|x| x == m) {
                        circuit.models.push(m.to_owned());
                    }
                    Value::Model(name)
                }
                Some(m) => Value::Model(m.to_owned()),
            },
        };
        circuit.elements.push(Element {
            name: reference.clone(),
            kind,
            nodes,
            value,
        });
    }
    if circuit.elements.is_empty() && problems.is_empty() {
        problems.push("the schematic has no simulatable components (R, C, L, V, I, D, Q, M, X) connected to nets".into());
    }
    if !circuit.elements.is_empty()
        && !circuit
            .elements
            .iter()
            .any(|e| e.nodes.iter().any(|n| n == "0"))
    {
        problems.push("no net is ground: name a net GND (power symbol or label) or 0".into());
    }
    (circuit, notes, problems)
}

fn pnum(params: &BTreeMap<String, String>, key: &str, default: &str) -> Result<f64, String> {
    let t = params.get(key).map_or(default, String::as_str);
    parse_value(t).map_err(|e| format!("{key}: {e}"))
}

impl Workspace {
    fn sim_defaults(&mut self) {
        if self.sim.analysis.is_empty() {
            self.sim.analysis = "op".into();
        }
        if self.sim.backend.is_empty() {
            self.sim.backend = "builtin".into();
        }
    }

    fn sim_analysis(&self) -> Result<Analysis, String> {
        let p = &self.sim.params;
        Ok(match self.sim.analysis.as_str() {
            "op" => Analysis::Op,
            "dc" => Analysis::DcSweep {
                source: p.get("source").cloned().unwrap_or_default(),
                start: pnum(p, "start", "0")?,
                stop: pnum(p, "stop", "5")?,
                step: pnum(p, "step", "0.5")?,
            },
            "ac" => Analysis::Ac {
                points_per_decade: pnum(p, "ppd", "20")? as u32,
                start_hz: pnum(p, "fstart", "1")?,
                stop_hz: pnum(p, "fstop", "1meg")?,
            },
            "tran" => Analysis::Tran {
                step: pnum(p, "tstep", "10u")?,
                stop: pnum(p, "tstop", "10m")?,
            },
            other => return Err(format!("unknown analysis “{other}”")),
        })
    }

    /// Rebuilds the netlist when the schematic changed.
    pub(crate) fn sim_refresh(&mut self) {
        self.sim_defaults();
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let revision = open.engine.revision();
        if self.sim.built_at == Some(revision) {
            return;
        }
        let sch = open.engine.project().schematic();
        let title = open.engine.project().name().to_owned();
        let built = self.connectivity(|c| build_circuit(sch, c, &title));
        let Some((circuit, notes, problems)) = built else {
            return;
        };
        self.sim.sources = circuit
            .elements
            .iter()
            .filter(|e| matches!(e.kind, Kind::V | Kind::I))
            .map(|e| e.name.clone())
            .collect();
        self.sim.notes = notes;
        self.sim.problems = problems;
        let analysis = self.sim_analysis().unwrap_or(Analysis::Op);
        self.sim.netlist = circuit.to_spice(&analysis);
        self.sim.built_at = Some(revision);
    }

    /// `set <key>=<value>` (`analysis`, `backend`, `source`, `start`, `stop`, `step`, `ppd`,
    /// `fstart`, `fstop`, `tstep`, `tstop`), `run`.
    pub fn simulation_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        self.sim_defaults();
        match name {
            "set" => {
                let (k, v) = arg
                    .unwrap_or("")
                    .split_once('=')
                    .ok_or("expected key=value")?;
                match k {
                    "analysis" => {
                        if !matches!(v, "op" | "dc" | "ac" | "tran") {
                            return Err("analysis is op, dc, ac or tran".into());
                        }
                        self.sim.analysis = v.to_owned();
                    }
                    "backend" => {
                        if !backends().iter().any(|b| b.id() == v) {
                            return Err("unknown backend".into());
                        }
                        self.sim.backend = v.to_owned();
                    }
                    _ => {
                        self.sim.params.insert(k.to_owned(), v.to_owned());
                    }
                }
                self.sim.built_at = None;
                self.sim_refresh();
                Ok(())
            }
            "run" => self.sim_run(),
            other => Err(format!("unknown simulation action: {other}")),
        }
    }

    fn sim_run(&mut self) -> Result<(), String> {
        self.sim.built_at = None;
        self.sim_refresh();
        self.sim.error.clear();
        let open = self.open.as_ref().ok_or("no project is open")?;
        let revision = open.engine.revision();
        if !self.sim.problems.is_empty() {
            return Err(format!("fix the circuit first: {}", self.sim.problems[0]));
        }
        let analysis = self.sim_analysis()?;
        let sch = open.engine.project().schematic();
        let title = open.engine.project().name().to_owned();
        let (circuit, _, _) = self
            .connectivity(|c| build_circuit(sch, c, &title))
            .ok_or("no project is open")?;
        let backend = backends()
            .into_iter()
            .find(|b| b.id() == self.sim.backend)
            .ok_or("unknown backend")?;
        if !backend.available() {
            self.sim.error = format!("{} is not available on this machine", backend.name());
            return Err(self.sim.error.clone());
        }
        let started = std::time::Instant::now();
        match backend.run(&circuit, &analysis) {
            Ok(r) => {
                let points = r.x.len().max(1);
                self.sim.result = Some((revision, r));
                self.log(
                    MessageLevel::Info,
                    format!(
                        "Simulation ({}): {points} point(s) in {} ms",
                        analysis.name(),
                        started.elapsed().as_millis()
                    ),
                );
                Ok(())
            }
            Err(e) => {
                self.sim.result = None;
                self.sim.error = e.to_string();
                Err(self.sim.error.clone())
            }
        }
    }

    pub(crate) fn simulation_info(&mut self) -> SimulationInfo {
        self.sim_refresh();
        let revision = self.open.as_ref().map(|o| o.engine.revision());
        let decimate =
            |v: &[f64], keep: &[usize]| -> Vec<f64> { keep.iter().map(|&i| v[i]).collect() };
        let result = self.sim.result.as_ref().map(|(rev, r)| {
            let n =
                r.x.len()
                    .max(r.series.first().map_or(0, |s| s.values.len()));
            let keep: Vec<usize> = if n <= MAX_POINTS {
                (0..n).collect()
            } else {
                let mut k: Vec<usize> = (0..MAX_POINTS)
                    .map(|i| i * (n - 1) / (MAX_POINTS - 1))
                    .collect();
                k.dedup();
                k
            };
            SimulationResultInfo {
                stale: Some(*rev) != revision,
                analysis: r.analysis.clone(),
                backend: r.backend.clone(),
                x_label: r.x_label.clone(),
                x_unit: r.x_unit.to_owned(),
                x_log: r.x_log,
                x: decimate(
                    &r.x,
                    &keep
                        .iter()
                        .copied()
                        .filter(|i| *i < r.x.len())
                        .collect::<Vec<_>>(),
                ),
                decimated: n > MAX_POINTS,
                points: n as u32,
                series: r
                    .series
                    .iter()
                    .map(|s| SimulationSeriesInfo {
                        name: s.name.clone(),
                        unit: s.unit.to_owned(),
                        values: decimate(
                            &s.values,
                            &keep
                                .iter()
                                .copied()
                                .filter(|i| *i < s.values.len())
                                .collect::<Vec<_>>(),
                        ),
                    })
                    .collect(),
            }
        });
        let p = &self.sim.params;
        let get = |k: &str, d: &str| p.get(k).cloned().unwrap_or_else(|| d.to_owned());
        SimulationInfo {
            backends: backends()
                .iter()
                .map(|b| SimulationBackendInfo {
                    id: b.id().into(),
                    name: b.name().into(),
                    available: b.available(),
                })
                .collect(),
            backend: self.sim.backend.clone(),
            analysis: self.sim.analysis.clone(),
            source: get(
                "source",
                self.sim.sources.first().map_or("", String::as_str),
            ),
            sources: self.sim.sources.clone(),
            start: get("start", "0"),
            stop: get("stop", "5"),
            step: get("step", "0.5"),
            ppd: get("ppd", "20"),
            fstart: get("fstart", "1"),
            fstop: get("fstop", "1meg"),
            tstep: get("tstep", "10u"),
            tstop: get("tstop", "10m"),
            netlist: self.sim.netlist.clone(),
            notes: self.sim.notes.clone(),
            problems: self.sim.problems.clone(),
            error: self.sim.error.clone(),
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_names_are_sanitised_and_unique() {
        let mut taken = BTreeMap::new();
        assert_eq!(node_name("OUT", 1, &mut taken), "OUT");
        assert_eq!(node_name("A/B", 2, &mut taken), "A_B");
        assert_eq!(node_name("A_B", 3, &mut taken), "A_B_3");
        assert_eq!(node_name("", 4, &mut taken), "N4");
        assert_eq!(node_name("0", 5, &mut taken), "N5");
    }
}
