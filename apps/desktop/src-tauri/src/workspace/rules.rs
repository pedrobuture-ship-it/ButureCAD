//! Design rules and net classes: reading them for the Net Classes dialog and applying an edited
//! set as one undo step. Validation lives in `buturecad_pcb::DesignRules`; the router and DRC read
//! the same values.

use super::pcb_editor::{mm_str, parse_mm};
use super::*;
use buturecad_eda_core::commands::SetRulesCommand;
use buturecad_pcb::{DesignRules, DiffPair, NetClass};

impl Workspace {
    pub fn pcb_rules_info(&self) -> Result<RulesInfo, String> {
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let r = &doc.rules;
        Ok(RulesInfo {
            min_track_width: mm_str(r.min_track_width),
            min_via_diameter: mm_str(r.min_via_diameter),
            min_via_drill: mm_str(r.min_via_drill),
            min_annular_ring: mm_str(r.min_annular_ring),
            copper_to_edge: mm_str(r.copper_to_edge),
            classes: r
                .classes
                .values()
                .map(|c| NetClassInfo {
                    name: c.name.clone(),
                    track_width: mm_str(c.track_width),
                    clearance: mm_str(c.clearance),
                    via_diameter: mm_str(c.via_diameter),
                    via_drill: mm_str(c.via_drill),
                    diff_pair_width: mm_str(c.diff_pair_width),
                    diff_pair_gap: mm_str(c.diff_pair_gap),
                })
                .collect(),
            nets: doc
                .nets
                .iter()
                .map(|(id, n)| NetAssignInfo {
                    id: id.0,
                    name: n.name.clone(),
                    class: r
                        .assignments
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| buturecad_pcb::DEFAULT_CLASS.to_owned()),
                })
                .collect(),
            presets: NetClass::presets().into_iter().map(|c| c.name).collect(),
            pairs: r
                .diff_pairs
                .values()
                .map(|p| PairInfo {
                    name: p.name.clone(),
                    positive: p.positive.0,
                    negative: p.negative.0,
                    width: p.width.map(mm_str).unwrap_or_default(),
                    gap: p.gap.map(mm_str).unwrap_or_default(),
                    max_skew: mm_str(p.max_skew),
                })
                .collect(),
            suggested_pairs: {
                let nets: Vec<(Id, String)> =
                    doc.nets.iter().map(|(i, n)| (*i, n.name.clone())).collect();
                buturecad_pcb::detect_pairs(&nets, &r.diff_pairs)
                    .into_iter()
                    .map(|(name, p, n)| PairInfo {
                        name,
                        positive: p.0,
                        negative: n.0,
                        width: String::new(),
                        gap: String::new(),
                        max_skew: "0.25".into(),
                    })
                    .collect()
            },
        })
    }

    /// Replaces the rules with the edited set (one undo step). Everything is validated first.
    pub fn pcb_rules_apply(&mut self, info: RulesInfo) -> Result<(), String> {
        let before = self.pcb_doc().ok_or("no project is open")?.rules.clone();
        let mut after = DesignRules {
            min_track_width: parse_mm(&info.min_track_width, "minimum track width")?,
            min_via_diameter: parse_mm(&info.min_via_diameter, "minimum via diameter")?,
            min_via_drill: parse_mm(&info.min_via_drill, "minimum via drill")?,
            min_annular_ring: parse_mm(&info.min_annular_ring, "minimum annular ring")?,
            copper_to_edge: parse_mm(&info.copper_to_edge, "copper to edge")?,
            classes: Default::default(),
            assignments: Default::default(),
            diff_pairs: Default::default(),
            length_targets: before.length_targets.clone(),
        };
        for c in &info.classes {
            let name = c.name.trim().to_owned();
            let class = NetClass {
                name: name.clone(),
                track_width: parse_mm(&c.track_width, "track width")?,
                clearance: parse_mm(&c.clearance, "clearance")?,
                via_diameter: parse_mm(&c.via_diameter, "via diameter")?,
                via_drill: parse_mm(&c.via_drill, "via drill")?,
                diff_pair_width: parse_mm(&c.diff_pair_width, "differential pair width")?,
                diff_pair_gap: parse_mm(&c.diff_pair_gap, "differential pair gap")?,
            };
            if after.classes.insert(name.clone(), class).is_some() {
                return Err(format!("two net classes are named “{name}”"));
            }
        }
        for n in &info.nets {
            if n.class != buturecad_pcb::DEFAULT_CLASS {
                after.assignments.insert(Id(n.id), n.class.clone());
            }
        }
        for p in &info.pairs {
            let name = p.name.trim().to_owned();
            let opt = |s: &str, what: &str| -> Result<Option<i64>, String> {
                if s.trim().is_empty() {
                    Ok(None)
                } else {
                    parse_mm(s, what).map(Some)
                }
            };
            let pair = DiffPair {
                name: name.clone(),
                positive: Id(p.positive),
                negative: Id(p.negative),
                width: opt(&p.width, "pair width")?,
                gap: opt(&p.gap, "pair gap")?,
                max_skew: parse_mm(&p.max_skew, "maximum skew")?,
            };
            if after.diff_pairs.insert(name.clone(), pair).is_some() {
                return Err(format!("two differential pairs are named “{name}”"));
            }
        }
        after.validate().map_err(|e| e.to_string())?;
        if after == before {
            return Ok(());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        open.engine
            .execute(SetRulesCommand::new("Edit net classes", before, after))
            .map_err(|e| e.to_string())?;
        self.command_log.push("Edit net classes".into());
        Ok(())
    }
}
