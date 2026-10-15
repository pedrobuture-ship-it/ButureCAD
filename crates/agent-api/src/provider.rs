//! Agent **providers**: how requests written in words become proposals.
//!
//! The Core knows nothing about any model. A provider turns a request plus a read-only view of
//! the project into [`ProposalDraft`]s; the host then validates and shows them like any other
//! proposal. Plugging in a local or remote model means implementing [`AgentProvider`] outside the
//! Core (or talking to the host through the protocol of stage 42) — no Core change.
//!
//! Providers shipped here are deliberately **not AI**:
//!
//! * [`JsonProvider`] — the request is a JSON proposal written by anything (a model, a script, a
//!   person); it is parsed strictly.
//! * [`DecouplingHelper`] — a deterministic, rule-based generator for the common "add decoupling
//!   to U1" request. It reads the real power pins and nets and proposes capacitors.

use crate::action::{Action, PinSpec};
use buturecad_eda_core::Project;
use buturecad_schematic::{Connectivity, Object, PinType};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalDraft {
    pub title: String,
    pub actions: Vec<Action>,
    /// Why, in words (shown next to the diff).
    pub rationale: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Deterministic code, no model.
    RuleBased,
    /// The input is produced elsewhere (pasted JSON).
    External,
    /// A language model behind this provider.
    Model,
}

pub trait AgentProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn kind(&self) -> ProviderKind;
    fn description(&self) -> &'static str;
    /// Turns `request` into drafts. Read-only: the project is only borrowed.
    fn draft(&self, request: &str, project: &Project) -> Result<Vec<ProposalDraft>, String>;
}

// -- JSON ------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonInput {
    Many(Vec<ProposalJson>),
    One(ProposalJson),
    Actions(Vec<Action>),
}

#[derive(Deserialize)]
struct ProposalJson {
    #[serde(default)]
    title: String,
    #[serde(default)]
    rationale: Vec<String>,
    actions: Vec<Action>,
}

pub struct JsonProvider;

impl AgentProvider for JsonProvider {
    fn id(&self) -> &'static str {
        "json"
    }
    fn name(&self) -> &'static str {
        "Proposal JSON (from any agent)"
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::External
    }
    fn description(&self) -> &'static str {
        "Paste a proposal produced by any model, script or tool: a list of actions, or {title, rationale, actions}. It is validated like every other proposal."
    }
    fn draft(&self, request: &str, _project: &Project) -> Result<Vec<ProposalDraft>, String> {
        let input: JsonInput =
            serde_json::from_str(request).map_err(|e| format!("not a valid proposal: {e}"))?;
        let ps = match input {
            JsonInput::Many(v) => v,
            JsonInput::One(p) => vec![p],
            JsonInput::Actions(a) => vec![ProposalJson {
                title: "Proposal".into(),
                rationale: Vec::new(),
                actions: a,
            }],
        };
        if ps.is_empty() || ps.iter().any(|p| p.actions.is_empty()) {
            return Err("a proposal needs at least one action".into());
        }
        Ok(ps
            .into_iter()
            .map(|p| ProposalDraft {
                title: if p.title.is_empty() {
                    "Proposal".into()
                } else {
                    p.title
                },
                actions: p.actions,
                rationale: p.rationale,
            })
            .collect())
    }
}

// -- Decoupling ------------------------------------------------------------------------------------

/// Adds a 100 nF capacitor per power pin of an IC and one bulk capacitor per supply net, tied to
/// the pin's net and to ground, placed just outside the pad. Rule-based; see the rationale lines.
pub struct DecouplingHelper;

const SMALL_CAP: (&str, &str, &str) = ("100nF", "0402", "Buture:C");
const BULK_CAP: (&str, &str, &str) = ("4.7uF", "0805", "Buture:C");

fn snap(v: i64) -> i64 {
    let g = 50_000;
    (v + g / 2).div_euclid(g) * g
}

impl AgentProvider for DecouplingHelper {
    fn id(&self) -> &'static str {
        "decoupling"
    }
    fn name(&self) -> &'static str {
        "Decoupling helper (rule-based, not AI)"
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::RuleBased
    }
    fn description(&self) -> &'static str {
        "Understands requests like “add decoupling to U1”: a 100 nF capacitor per power pin plus a 4.7 µF per supply net, connected and placed next to the pins."
    }

    fn draft(&self, request: &str, project: &Project) -> Result<Vec<ProposalDraft>, String> {
        let low = request.to_lowercase();
        if !["decoup", "desacopl", "bypass"]
            .iter()
            .any(|k| low.contains(k))
        {
            return Err(
                "this helper only handles decoupling requests, e.g. “Add decoupling to U1”".into(),
            );
        }
        let sch = project.schematic();
        let conn = Connectivity::build(sch);
        // Components by reference.
        let mut comps = Vec::new();
        for sheet in sch.sheets().values() {
            for (id, o) in &sheet.objects {
                if let Object::Component(c) = o {
                    comps.push((*id, c.clone()));
                }
            }
        }
        let words: Vec<String> = request
            .split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        let power_pins =
            |c: &buturecad_schematic::Component| -> Vec<buturecad_schematic::PlacedPin> {
                sch.placed_pins(c)
                    .into_iter()
                    .filter(|p| p.kind == PinType::PowerInput)
                    .collect()
            };
        let target = comps
            .iter()
            .find(|(_, c)| words.contains(&c.reference.to_lowercase()))
            .or_else(|| {
                let ics: Vec<_> = comps
                    .iter()
                    .filter(|(_, c)| !power_pins(c).is_empty())
                    .collect();
                if ics.len() == 1 {
                    Some(ics[0])
                } else {
                    None
                }
            })
            .ok_or("name the component to decouple (e.g. U1); none was found in the request")?;
        let (tid, ic) = (target.0, &target.1);
        let pins = power_pins(ic);
        if pins.is_empty() {
            return Err(format!(
                "{} has no power-input pins in its symbol, so there is nothing to decouple",
                ic.reference
            ));
        }
        // Ground net.
        let gnd = conn
            .nets()
            .iter()
            .find(|n| n.power && n.name.to_uppercase().contains("GND"))
            .or_else(|| {
                conn.nets()
                    .iter()
                    .find(|n| n.name.eq_ignore_ascii_case("GND"))
            })
            .map(|n| n.name.clone())
            .ok_or("no ground net exists yet; connect a GND power symbol first")?;
        // Free capacitor references, continuing after the highest used number.
        let mut used: BTreeSet<u64> = BTreeSet::new();
        for (_, c) in &comps {
            if let Some(n) = c
                .reference
                .strip_prefix('C')
                .and_then(|r| r.parse::<u64>().ok())
            {
                used.insert(n);
            }
        }
        let mut next = used.iter().next_back().copied().unwrap_or(0) + 1;
        let mut take = || {
            let r = format!("C{next}");
            next += 1;
            r
        };
        // Board geometry of the IC, if it is on the board.
        let fp = project
            .pcb()
            .placed
            .values()
            .find(|f| f.reference == ic.reference);
        let pads = fp.and_then(|f| {
            project
                .pcb()
                .footprints
                .get(&f.footprint)
                .map(|d| (f, f.world_pads(d)))
        });
        let mut actions = Vec::new();
        let mut why = Vec::new();
        let mut placed_at: Vec<(i64, i64)> = Vec::new();
        let mut supplies: Vec<String> = Vec::new();
        for pin in &pins {
            let net = conn
                .net_of_pin(tid, &pin.number)
                .map(|n| n.name.clone())
                .ok_or_else(|| {
                    format!(
                        "{}.{} is not connected to a net yet",
                        ic.reference, pin.number
                    )
                })?;
            if net == gnd {
                continue;
            }
            let r = take();
            actions.push(Action::CreateComponent {
                reference: Some(r.clone()),
                symbol: SMALL_CAP.2.into(),
                value: SMALL_CAP.0.into(),
                footprint: SMALL_CAP.1.into(),
                properties: Default::default(),
            });
            actions.push(Action::Connect {
                net: net.clone(),
                pins: vec![PinSpec {
                    reference: r.clone(),
                    pin: "1".into(),
                }],
            });
            actions.push(Action::Connect {
                net: gnd.clone(),
                pins: vec![PinSpec {
                    reference: r.clone(),
                    pin: "2".into(),
                }],
            });
            why.push(format!(
                "{r} {} between {net} and {gnd} for {}.{} ({})",
                SMALL_CAP.0, ic.reference, pin.number, pin.name
            ));
            if let Some((f, ps)) = &pads {
                if let Some(p) = ps.iter().find(|p| p.number == pin.number) {
                    let (dx, dy) = (
                        p.center.x - f.placement.position.x,
                        p.center.y - f.placement.position.y,
                    );
                    let (ux, uy, rot) = if dx.abs() >= dy.abs() {
                        (dx.signum(), 0, 0)
                    } else {
                        (0, dy.signum(), 90)
                    };
                    let (mut x, mut y) = (
                        snap(p.center.x + ux * 2_000_000),
                        snap(p.center.y + uy * 2_000_000),
                    );
                    let mut tries = 0;
                    while placed_at
                        .iter()
                        .any(|(px, py)| (px - x).abs() < 1_300_000 && (py - y).abs() < 1_300_000)
                        && tries < 8
                    {
                        x += if ux == 0 { 1_400_000 } else { 0 };
                        y += if uy == 0 { 1_400_000 } else { 0 };
                        tries += 1;
                    }
                    placed_at.push((x, y));
                    actions.push(Action::Place {
                        reference: r,
                        x,
                        y,
                        rotation_deg: rot,
                        bottom: f.placement.flipped,
                    });
                }
            }
            if !supplies.contains(&net) {
                supplies.push(net);
            }
        }
        if actions.is_empty() {
            return Err(format!(
                "every power pin of {} is already on {gnd}; nothing to decouple",
                ic.reference
            ));
        }
        for net in supplies {
            let r = take();
            actions.push(Action::CreateComponent {
                reference: Some(r.clone()),
                symbol: BULK_CAP.2.into(),
                value: BULK_CAP.0.into(),
                footprint: BULK_CAP.1.into(),
                properties: Default::default(),
            });
            actions.push(Action::Connect {
                net: net.clone(),
                pins: vec![PinSpec {
                    reference: r.clone(),
                    pin: "1".into(),
                }],
            });
            actions.push(Action::Connect {
                net: gnd.clone(),
                pins: vec![PinSpec {
                    reference: r.clone(),
                    pin: "2".into(),
                }],
            });
            why.push(format!(
                "{r} {} bulk capacitor between {net} and {gnd}",
                BULK_CAP.0
            ));
            if let Some((f, ps)) = &pads {
                // Next to the first pad of this supply, a little further out than the small caps.
                if let Some(p) = pins
                    .iter()
                    .find(|p| {
                        conn.net_of_pin(tid, &p.number)
                            .is_some_and(|n| n.name == net)
                    })
                    .and_then(|pin| ps.iter().find(|w| w.number == pin.number))
                {
                    let (dx, dy) = (
                        p.center.x - f.placement.position.x,
                        p.center.y - f.placement.position.y,
                    );
                    let (ux, uy, rot) = if dx.abs() >= dy.abs() {
                        (dx.signum(), 0, 0)
                    } else {
                        (0, dy.signum(), 90)
                    };
                    let (mut x, mut y) = (
                        snap(p.center.x + ux * 5_000_000),
                        snap(p.center.y + uy * 5_000_000),
                    );
                    let mut tries = 0;
                    while placed_at
                        .iter()
                        .any(|(px, py)| (px - x).abs() < 2_300_000 && (py - y).abs() < 2_300_000)
                        && tries < 8
                    {
                        x += if ux == 0 { 2_400_000 } else { 0 };
                        y += if uy == 0 { 2_400_000 } else { 0 };
                        tries += 1;
                    }
                    placed_at.push((x, y));
                    actions.push(Action::Place {
                        reference: r,
                        x,
                        y,
                        rotation_deg: rot,
                        bottom: f.placement.flipped,
                    });
                }
            }
        }
        if pads.is_none() {
            why.push(format!(
                "{} is not on the board yet, so no placement is proposed",
                ic.reference
            ));
        } else {
            why.push("placement: just outside each power pad; check the DRC result for courtyard overlaps".into());
        }
        Ok(vec![ProposalDraft {
            title: format!("Decoupling for {}", ic.reference),
            actions,
            rationale: why,
        }])
    }
}

/// All providers shipped with the Core.
pub fn builtin_providers() -> Vec<Box<dyn AgentProvider>> {
    vec![Box::new(DecouplingHelper), Box::new(JsonProvider)]
}
