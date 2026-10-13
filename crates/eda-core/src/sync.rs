//! "Update PCB from Schematic": plans the changes that bring the board in line with the
//! schematic, without touching anything.
//!
//! The plan is **pure data**: a list of human-readable lines for the preview, warnings, the new
//! footprint definitions to embed, and ordinary [`PcbChange`]s that the caller runs through the
//! Command Engine as one undo step ([`UpdatePlan::apply`]). Nothing here mutates the project.
//!
//! Matching rules (so placement and routing survive):
//!
//! * a placed footprint is tied to its schematic part by `link` (the lowest object id among the
//!   part's units); unlinked footprints with the same reference are adopted and linked;
//! * footprints are **moved, not recreated** when the part's footprint, reference or value
//!   changes; only the definition key, text and pad nets change;
//! * a footprint whose part disappeared is removed unless it is locked (then it is kept and
//!   reported); tracks and vias stay where they are;
//! * nets are matched by name; a net whose name changed but whose pads are identical is
//!   *renamed* (same id), so its tracks keep their net; a net that disappears is deleted and the
//!   tracks, vias and zones that used it lose their net (reported, never deleted).

use crate::model::{Id, PcbChange, PcbDoc, PcbObject};
use crate::Project;
use buturecad_pcb::{Footprint, Net, PlacedFootprint, Placement};
use buturecad_schematic::{Component, Connectivity, Object};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanKind {
    AddFootprint,
    RemoveFootprint,
    ChangeFootprint,
    Link,
    UpdateField,
    AddNet,
    RemoveNet,
    RenameNet,
    PadNet,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLine {
    pub kind: PlanKind,
    pub text: String,
}

/// What "Update PCB from Schematic" would do.
#[derive(Debug, Clone, Default)]
pub struct UpdatePlan {
    pub lines: Vec<PlanLine>,
    /// Footprint definitions to add to the project before the changes apply.
    pub new_footprints: Vec<Footprint>,
    pub changes: Vec<PcbChange>,
    /// Ids the plan uses; reserve them before applying.
    pub next_id: u64,
}

impl UpdatePlan {
    /// Applies the plan as **one** undo step: new footprint definitions first, then the board
    /// changes. Fails (changing nothing) if the board changed since the plan was made.
    pub fn apply(&self, engine: &mut crate::CommandEngine) -> Result<(), crate::CommandError> {
        engine.reserve_ids_until(self.next_id);
        engine.transaction("Update PCB from Schematic", |e| {
            for f in &self.new_footprints {
                e.execute(crate::commands::SetFootprintCommand::new(
                    f.name.clone(),
                    None,
                    Some(f.clone()),
                ))?;
            }
            if !self.changes.is_empty() {
                e.execute(crate::commands::PcbEditCommand::new(
                    "Update PCB from Schematic",
                    self.changes.clone(),
                ))?;
            }
            Ok(())
        })
    }

    /// Nothing to change (warnings may still exist).
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.new_footprints.is_empty()
    }

    pub fn count(&self, kind: PlanKind) -> usize {
        self.lines.iter().filter(|l| l.kind == kind).count()
    }

    pub fn warnings(&self) -> impl Iterator<Item = &PlanLine> {
        self.lines.iter().filter(|l| l.kind == PlanKind::Warning)
    }
}

/// A schematic part (all units of one reference).
struct Part<'a> {
    reference: String,
    /// Lowest object id among the units: the stable link.
    link: Id,
    comp: &'a Component,
}

fn parts<'a>(
    project: &'a Project,
    conn: &Connectivity,
    lines: &mut Vec<PlanLine>,
) -> Vec<Part<'a>> {
    let sch = project.schematic();
    let mut by_ref: BTreeMap<String, Vec<(Id, &Component)>> = BTreeMap::new();
    for sid in conn.reachable_sheets() {
        let Some(sheet) = sch.sheet(*sid) else {
            continue;
        };
        for (id, o) in &sheet.objects {
            if let Object::Component(c) = o {
                if c.reference.starts_with('#') || c.reference.trim().is_empty() {
                    continue;
                }
                if c.reference.ends_with('?') {
                    lines.push(PlanLine {
                        kind: PlanKind::Warning,
                        text: format!("{} is not annotated and was skipped", c.reference),
                    });
                    continue;
                }
                by_ref
                    .entry(c.reference.clone())
                    .or_default()
                    .push((*id, c));
            }
        }
    }
    let mut out = Vec::new();
    for (reference, mut units) in by_ref {
        units.sort_by_key(|u| u.0);
        let distinct: BTreeSet<&str> = units.iter().map(|u| u.1.lib_id.as_str()).collect();
        if distinct.len() > 1 {
            lines.push(PlanLine {
                kind: PlanKind::Warning,
                text: format!(
                    "{reference} is used by different symbols; only the first part is used"
                ),
            });
        }
        out.push(Part {
            reference,
            link: units[0].0,
            comp: units[0].1,
        });
    }
    out
}

/// Accumulates object edits so every id is changed at most once.
#[derive(Default)]
struct Edits {
    map: BTreeMap<Id, (Option<PcbObject>, Option<PcbObject>)>,
}

impl Edits {
    fn current(&self, doc: &PcbDoc, id: Id) -> Option<PcbObject> {
        match self.map.get(&id) {
            Some((_, after)) => after.clone(),
            None => doc.object(id),
        }
    }
    fn set(&mut self, doc: &PcbDoc, id: Id, after: Option<PcbObject>) {
        let before = self
            .map
            .get(&id)
            .map_or_else(|| doc.object(id), |(b, _)| b.clone());
        self.map.insert(id, (before, after));
    }
    fn into_changes(self) -> Vec<PcbChange> {
        self.map
            .into_iter()
            .filter(|(_, (b, a))| b != a)
            .map(|(id, (before, after))| PcbChange { id, before, after })
            .collect()
    }
}

/// Plans the update. `resolve` finds footprint definitions that are not yet in the project
/// (library lookup); it is only called for names the project lacks.
pub fn plan_update(project: &Project, resolve: &dyn Fn(&str) -> Option<Footprint>) -> UpdatePlan {
    let sch = project.schematic();
    let doc = project.pcb();
    let conn = Connectivity::build(sch);
    let mut plan = UpdatePlan::default();
    let mut lines: Vec<PlanLine> = Vec::new();
    let parts = parts(project, &conn, &mut lines);
    let mut next = project.next_id();
    let mut alloc = || {
        let id = Id(next);
        next += 1;
        id
    };
    let mut edits = Edits::default();

    // -- footprint definitions -------------------------------------------------------------------
    let mut defs: BTreeMap<String, Footprint> = BTreeMap::new();
    let mut def_of = |name: &str, new: &mut Vec<Footprint>| -> Option<Footprint> {
        if let Some(d) = defs.get(name) {
            return Some(d.clone());
        }
        let d = doc.footprints.get(name).cloned().or_else(|| {
            let d = resolve(name)?;
            new.push(d.clone());
            Some(d)
        })?;
        defs.insert(name.to_owned(), d.clone());
        Some(d)
    };

    // -- match parts to placed footprints --------------------------------------------------------
    let mut by_link: BTreeMap<Id, Id> = BTreeMap::new();
    let mut unlinked: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    for (id, f) in &doc.placed {
        match f.link {
            Some(l) => {
                by_link.insert(l, *id);
            }
            None => unlinked.entry(f.reference.clone()).or_default().push(*id),
        }
    }
    let live_links: BTreeSet<Id> = parts.iter().map(|p| p.link).collect();
    // Footprints keep their link even when the lowest unit changes: match by link, then by
    // reference among all placed footprints that no part claims by link.
    let mut claimed: BTreeSet<Id> = BTreeSet::new();
    struct Matched<'a> {
        part: &'a Part<'a>,
        placed: Option<Id>,
    }
    let mut matched: Vec<Matched> = Vec::new();
    for p in &parts {
        let mut hit = by_link.get(&p.link).copied();
        if hit.is_none() {
            hit = unlinked
                .get_mut(&p.reference)
                .and_then(|v| (!v.is_empty()).then(|| v.remove(0)));
            if let Some(id) = hit {
                lines.push(PlanLine {
                    kind: PlanKind::Link,
                    text: format!(
                        "Link existing footprint {} to the schematic part",
                        p.reference
                    ),
                });
                let _ = id;
            }
        }
        if hit.is_none() {
            // A linked footprint whose link moved to another unit (the lowest unit was deleted).
            hit = doc
                .placed
                .iter()
                .find(|(id, f)| {
                    !claimed.contains(id)
                        && f.link.is_some_and(|l| !live_links.contains(&l))
                        && f.reference == p.reference
                })
                .map(|(id, _)| *id);
        }
        if let Some(id) = hit {
            claimed.insert(id);
        }
        matched.push(Matched {
            part: p,
            placed: hit,
        });
    }

    // -- desired nets (≥ 2 pads) -----------------------------------------------------------------
    let included: BTreeSet<&str> = matched
        .iter()
        .filter(|m| !m.part.comp.footprint.trim().is_empty())
        .map(|m| m.part.reference.as_str())
        .collect();
    let mut desired: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for net in conn.nets() {
        let pins: BTreeSet<(String, String)> = net
            .pins
            .iter()
            .filter(|p| included.contains(p.reference.as_str()))
            .map(|p| (p.reference.clone(), p.pin.clone()))
            .collect();
        if pins.len() >= 2 {
            desired.insert(net.name.clone(), pins);
        }
    }

    // -- resolve net ids (reuse by name, rename by identical pad set, else create) ---------------
    let mut old_sets: BTreeMap<Id, BTreeSet<(String, String)>> = BTreeMap::new();
    for f in doc.placed.values() {
        for (pad, net) in &f.pad_nets {
            old_sets
                .entry(*net)
                .or_default()
                .insert((f.reference.clone(), pad.clone()));
        }
    }
    let mut net_id: BTreeMap<String, Id> = BTreeMap::new();
    let existing_by_name: BTreeMap<&str, Id> = doc
        .nets
        .iter()
        .map(|(i, n)| (n.name.as_str(), *i))
        .collect();
    let mut kept: BTreeSet<Id> = BTreeSet::new();
    for name in desired.keys() {
        if let Some(id) = existing_by_name.get(name.as_str()) {
            net_id.insert(name.clone(), *id);
            kept.insert(*id);
        }
    }
    for (name, pins) in &desired {
        if net_id.contains_key(name) {
            continue;
        }
        let renamed = doc.nets.iter().find(|(id, n)| {
            !kept.contains(id)
                && !desired.contains_key(&n.name)
                && old_sets.get(id).is_some_and(|s| s == pins)
        });
        match renamed {
            Some((id, old)) => {
                lines.push(PlanLine {
                    kind: PlanKind::RenameNet,
                    text: format!(
                        "Rename net {} → {name} (same pads; its tracks keep their net)",
                        old.name
                    ),
                });
                edits.set(doc, *id, Some(PcbObject::Net(Net { name: name.clone() })));
                kept.insert(*id);
                net_id.insert(name.clone(), *id);
            }
            None => {
                let id = alloc();
                lines.push(PlanLine {
                    kind: PlanKind::AddNet,
                    text: format!("Add net {name} ({} pads)", pins.len()),
                });
                edits.set(doc, id, Some(PcbObject::Net(Net { name: name.clone() })));
                kept.insert(id);
                net_id.insert(name.clone(), id);
            }
        }
    }
    // Nets that no longer exist.
    for (id, n) in &doc.nets {
        if kept.contains(id) {
            continue;
        }
        let mut stripped = 0;
        for (tid, t) in &doc.tracks {
            if t.net == Some(*id) {
                let mut t = t.clone();
                t.net = None;
                edits.set(doc, *tid, Some(PcbObject::Track(t)));
                stripped += 1;
            }
        }
        for (vid, v) in &doc.vias {
            if v.net == Some(*id) {
                let mut v = v.clone();
                v.net = None;
                edits.set(doc, *vid, Some(PcbObject::Via(v)));
                stripped += 1;
            }
        }
        for (zid, z) in &doc.zones {
            if z.net == Some(*id) {
                let mut z = z.clone();
                z.net = None;
                edits.set(doc, *zid, Some(PcbObject::Zone(z)));
                stripped += 1;
            }
        }
        lines.push(PlanLine {
            kind: PlanKind::RemoveNet,
            text: if stripped > 0 {
                format!("Remove net {} ({stripped} track/via/zone object(s) lose their net but stay on the board)", n.name)
            } else {
                format!("Remove net {}", n.name)
            },
        });
        edits.set(doc, *id, None);
    }

    // -- footprints ------------------------------------------------------------------------------
    let pad_net_for = |reference: &str, def: &Footprint| -> BTreeMap<String, Id> {
        let mut m = BTreeMap::new();
        for pad in &def.pads {
            for (name, pins) in &desired {
                if pins.contains(&(reference.to_owned(), pad.number.clone())) {
                    if let Some(id) = net_id.get(name) {
                        m.insert(pad.number.clone(), *id);
                    }
                }
            }
        }
        m
    };
    let mut staged: Vec<(Id, PlacedFootprint, Footprint)> = Vec::new();
    for m in &matched {
        let c = m.part.comp;
        let fp_name = c.footprint.trim();
        if fp_name.is_empty() {
            lines.push(PlanLine {
                kind: PlanKind::Warning,
                text: format!(
                    "{} has no footprint assigned in the schematic",
                    m.part.reference
                ),
            });
            continue;
        }
        let Some(def) = def_of(fp_name, &mut plan.new_footprints) else {
            lines.push(PlanLine {
                kind: PlanKind::Warning,
                text: format!(
                    "{}: footprint {fp_name} was not found in the project or any library",
                    m.part.reference
                ),
            });
            continue;
        };
        // Pins of the part that have no pad.
        let missing: BTreeSet<String> = conn
            .nets()
            .iter()
            .flat_map(|n| n.pins.iter())
            .filter(|p| {
                p.reference == m.part.reference && !def.pads.iter().any(|d| d.number == p.pin)
            })
            .map(|p| p.pin.clone())
            .collect();
        if !missing.is_empty() {
            lines.push(PlanLine {
                kind: PlanKind::Warning,
                text: format!(
                    "{}: pin(s) {} have no pad in {fp_name}",
                    m.part.reference,
                    missing.into_iter().collect::<Vec<_>>().join(", ")
                ),
            });
        }
        let pad_nets = pad_net_for(&m.part.reference, &def);
        match m.placed {
            Some(id) => {
                let Some(old) = doc.placed.get(&id) else {
                    continue;
                };
                let mut new = old.clone();
                new.link = Some(m.part.link);
                if old.footprint != fp_name {
                    lines.push(PlanLine {
                        kind: PlanKind::ChangeFootprint,
                        text: format!(
                            "{}: footprint {} → {fp_name} (placement kept)",
                            m.part.reference, old.footprint
                        ),
                    });
                    new.footprint = fp_name.to_owned();
                }
                if old.reference != m.part.reference {
                    lines.push(PlanLine {
                        kind: PlanKind::UpdateField,
                        text: format!("{} → {}: reference", old.reference, m.part.reference),
                    });
                    new.reference = m.part.reference.clone();
                }
                if old.value != c.value {
                    lines.push(PlanLine {
                        kind: PlanKind::UpdateField,
                        text: format!(
                            "{}: value “{}” → “{}”",
                            m.part.reference, old.value, c.value
                        ),
                    });
                    new.value = c.value.clone();
                }
                for (pad, net) in &pad_nets {
                    if old.pad_nets.get(pad) != Some(net) {
                        let name = |id: Option<&Id>| {
                            id.and_then(|i| edits.current(doc, *i))
                                .and_then(|o| match o {
                                    PcbObject::Net(n) => Some(n.name),
                                    _ => None,
                                })
                        };
                        lines.push(PlanLine {
                            kind: PlanKind::PadNet,
                            text: format!(
                                "{} pad {pad}: {} → {}",
                                m.part.reference,
                                name(old.pad_nets.get(pad)).unwrap_or_else(|| "(no net)".into()),
                                name(Some(net)).unwrap_or_default()
                            ),
                        });
                    }
                }
                for pad in old.pad_nets.keys() {
                    if !pad_nets.contains_key(pad) {
                        lines.push(PlanLine {
                            kind: PlanKind::PadNet,
                            text: format!("{} pad {pad}: no longer connected", m.part.reference),
                        });
                    }
                }
                new.pad_nets = pad_nets;
                edits.set(doc, id, Some(PcbObject::Footprint(new)));
            }
            None => {
                let id = alloc();
                let f = PlacedFootprint {
                    footprint: fp_name.to_owned(),
                    reference: m.part.reference.clone(),
                    value: c.value.clone(),
                    placement: Placement::at(buturecad_geometry::Point::ORIGIN),
                    locked: false,
                    link: Some(m.part.link),
                    pad_nets,
                };
                staged.push((id, f, def));
            }
        }
    }

    // New footprints go to a staging grid to the right of everything already on the board.
    if !staged.is_empty() {
        let mut max_x = doc
            .placed
            .values()
            .filter_map(|f| f.bounds(doc.footprints.get(&f.footprint)?))
            .map(|b| b.1.x)
            .max()
            .unwrap_or(0);
        for g in doc.graphics.values() {
            if let Some(b) = crate::pcb_ops::bounds(doc, &PcbObject::Graphic(g.clone())) {
                max_x = max_x.max(b.1.x);
            }
        }
        let (x0, mut x, mut y, mut row_h) = (max_x + 10_000_000, max_x + 10_000_000, 0i64, 0i64);
        for (i, (id, mut f, def)) in staged.into_iter().enumerate() {
            let probe = f.bounds(&def).map_or((5_000_000, 5_000_000), |(lo, hi)| {
                (hi.x - lo.x, hi.y - lo.y)
            });
            if i > 0 && i % 8 == 0 {
                x = x0;
                y -= row_h + 3_000_000;
                row_h = 0;
            }
            f.placement.position = buturecad_geometry::Point::new(x - probe.0 / 2 + probe.0 / 2, y);
            x += probe.0 + 3_000_000;
            row_h = row_h.max(probe.1);
            lines.push(PlanLine {
                kind: PlanKind::AddFootprint,
                text: format!("Add {} ({})", f.reference, f.footprint),
            });
            edits.set(doc, id, Some(PcbObject::Footprint(f)));
        }
    }

    // -- footprints whose part is gone -----------------------------------------------------------
    for (id, f) in &doc.placed {
        if claimed.contains(id) {
            continue;
        }
        let part_gone = match f.link {
            Some(l) => !live_links.contains(&l),
            None => !parts.iter().any(|p| p.reference == f.reference),
        };
        // Unlinked footprints are the user's own (manual placement) unless a part claims them.
        if f.link.is_none() {
            continue;
        }
        if !part_gone {
            continue;
        }
        if f.locked {
            lines.push(PlanLine {
                kind: PlanKind::Warning,
                text: format!(
                    "{} is no longer in the schematic but is locked; it was kept",
                    f.reference
                ),
            });
            // Its pads no longer belong to any net.
            if !f.pad_nets.is_empty() {
                let mut kept_fp = f.clone();
                kept_fp.pad_nets.clear();
                edits.set(doc, *id, Some(PcbObject::Footprint(kept_fp)));
            }
        } else {
            lines.push(PlanLine {
                kind: PlanKind::RemoveFootprint,
                text: format!("Remove {} (no longer in the schematic)", f.reference),
            });
            edits.set(doc, *id, None);
        }
    }

    plan.lines = lines;
    plan.changes = edits.into_changes();
    plan.next_id = next;
    plan
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::commands::AddSymbolsCommand;
    use crate::{CommandEngine, ROOT_SHEET};
    use buturecad_geometry::{Point, Segment};
    use buturecad_pcb::generators::{Header, Soic};
    use buturecad_schematic::{builtin, Change, Placement as SPlacement, Wire};

    const G: i64 = 1_270_000;

    fn comp(reference: &str, lib: &str, fp: &str, x: i64) -> Component {
        Component {
            lib_id: lib.into(),
            reference: reference.into(),
            value: "10k".into(),
            footprint: fp.into(),
            properties: Default::default(),
            unit: 1,
            representation: 1,
            placement: SPlacement::at(Point::new(x * G, 0)),
        }
    }

    /// R1.2 — R2.1 wired together, R1.1 and R2.2 free.
    fn schematic_engine() -> CommandEngine {
        let mut e = CommandEngine::new(Project::new("T").unwrap());
        e.execute(AddSymbolsCommand::new(builtin::symbols()))
            .unwrap();
        let (a, b, w) = (e.new_id(), e.new_id(), e.new_id());
        let pin = |x: i64, y: i64| Point::new(x * G, y * G);
        e.execute(crate::commands::SchematicEditCommand::new(
            "build",
            vec![
                Change::create(
                    ROOT_SHEET,
                    a,
                    Object::Component(comp("R1", "Buture:R", "Lib:RES", 0)),
                ),
                Change::create(
                    ROOT_SHEET,
                    b,
                    Object::Component(comp("R2", "Buture:R", "Lib:RES", 10)),
                ),
                Change::create(
                    ROOT_SHEET,
                    w,
                    Object::Wire(Wire {
                        segment: Segment::new(pin(0, -3), pin(10, 3)),
                    }),
                ),
            ],
        ))
        .unwrap();
        e
    }

    fn res() -> Footprint {
        let mut f = Header::standard(2, 1).generate().unwrap();
        f.name = "Lib:RES".into();
        f
    }

    fn resolver(name: &str) -> Option<Footprint> {
        (name == "Lib:RES").then(res)
    }

    fn run(e: &mut CommandEngine) -> UpdatePlan {
        let plan = plan_update(e.project(), &resolver);
        plan.apply(e).unwrap();
        plan
    }

    #[test]
    fn first_update_adds_footprints_and_nets_and_a_second_is_empty() {
        let mut e = schematic_engine();
        let plan = plan_update(e.project(), &resolver);
        assert_eq!(plan.count(PlanKind::AddFootprint), 2);
        assert_eq!(
            plan.count(PlanKind::AddNet),
            1,
            "only the R1.2-R2.1 net has two pads"
        );
        assert_eq!(
            plan.new_footprints.len(),
            1,
            "the library footprint is embedded once"
        );
        // The preview changed nothing.
        assert!(e.project().pcb().placed.is_empty());
        run(&mut e);
        let doc = e.project().pcb();
        assert_eq!(doc.placed.len(), 2);
        assert_eq!(doc.nets.len(), 1);
        let net = *doc.nets.keys().next().unwrap();
        let r1 = doc.placed.values().find(|f| f.reference == "R1").unwrap();
        let r2 = doc.placed.values().find(|f| f.reference == "R2").unwrap();
        assert_eq!(r1.pad_nets.get("2"), Some(&net));
        assert_eq!(r2.pad_nets.get("1"), Some(&net));
        assert!(!r1.pad_nets.contains_key("1"));
        assert!(r1.link.is_some() && r1.placement.position != r2.placement.position);
        let again = plan_update(e.project(), &resolver);
        assert!(again.is_empty(), "{:?}", again.lines);
        assert_eq!(again.warnings().count(), 0);
        // One undo removes everything the update did.
        e.undo().unwrap();
        assert!(e.project().pcb().placed.is_empty() && e.project().pcb().nets.is_empty());
    }

    #[test]
    fn value_and_footprint_changes_keep_the_placement() {
        let mut e = schematic_engine();
        run(&mut e);
        // Move R1 on the board, then change the schematic.
        let id = *e
            .project()
            .pcb()
            .placed
            .iter()
            .find(|(_, f)| f.reference == "R1")
            .unwrap()
            .0;
        let before = e.project().pcb().object(id).unwrap();
        let PcbObject::Footprint(mut f) = before.clone() else {
            panic!()
        };
        f.placement.position = Point::new(33_000_000, 44_000_000);
        f.placement.rotation = 1;
        e.execute(crate::commands::PcbEditCommand::new(
            "move",
            vec![PcbChange::modify(id, before, PcbObject::Footprint(f))],
        ))
        .unwrap();
        let (sid, obj) = {
            let (sid, o) = e.project().schematic().object(Id(2)).unwrap();
            (sid, o.clone())
        };
        let mut c = match &obj {
            Object::Component(c) => c.clone(),
            _ => panic!(),
        };
        c.value = "22k".into();
        // The new footprint comes from a library the project does not have yet.
        c.footprint = "Lib:SOIC".into();
        e.execute(crate::commands::SchematicEditCommand::new(
            "edit",
            vec![Change::modify(sid, Id(2), obj, Object::Component(c))],
        ))
        .unwrap();
        let soic = {
            let mut s = Soic::narrow(8).generate().unwrap();
            s.name = "Lib:SOIC".into();
            s
        };
        let plan = plan_update(e.project(), &|n| match n {
            "Lib:RES" => Some(res()),
            "Lib:SOIC" => Some(soic.clone()),
            _ => None,
        });
        assert_eq!(plan.count(PlanKind::ChangeFootprint), 1);
        assert!(plan
            .lines
            .iter()
            .any(|l| l.kind == PlanKind::UpdateField && l.text.contains("22k")));
        // R1 (which was moved) is not mentioned: nothing about it changed.
        assert!(
            plan.lines.iter().all(|l| !l.text.starts_with("R1")),
            "{:?}",
            plan.lines
        );
    }

    #[test]
    fn removed_parts_unconnected_nets_and_locked_footprints() {
        let mut e = schematic_engine();
        run(&mut e);
        // A track on the net, a locked R2, and the wire deleted from the schematic.
        let net = *e.project().pcb().nets.keys().next().unwrap();
        let tid = e.new_id();
        e.execute(crate::commands::PcbEditCommand::new(
            "track",
            vec![PcbChange::create(
                tid,
                PcbObject::Track(crate::model::Track {
                    layer: buturecad_pcb::layer::F_CU,
                    segment: Segment::new(Point::ORIGIN, Point::new(1_000_000, 0)),
                    width: 200_000,
                    net: Some(net),
                }),
            )],
        ))
        .unwrap();
        let r2 = *e
            .project()
            .pcb()
            .placed
            .iter()
            .find(|(_, f)| f.reference == "R2")
            .unwrap()
            .0;
        let before = e.project().pcb().object(r2).unwrap();
        let PcbObject::Footprint(mut f) = before.clone() else {
            panic!()
        };
        f.locked = true;
        e.execute(crate::commands::PcbEditCommand::new(
            "lock",
            vec![PcbChange::modify(r2, before, PcbObject::Footprint(f))],
        ))
        .unwrap();
        // Delete R2 (id 2) and the wire (id 3) from the schematic.
        let objs: Vec<(Id, Object)> = [Id(2), Id(3)]
            .iter()
            .map(|i| (*i, e.project().schematic().object(*i).unwrap().1.clone()))
            .collect();
        e.execute(crate::commands::SchematicEditCommand::new(
            "delete",
            objs.into_iter()
                .map(|(i, o)| Change::delete(ROOT_SHEET, i, o))
                .collect(),
        ))
        .unwrap();
        let plan = plan_update(e.project(), &resolver);
        assert_eq!(
            plan.count(PlanKind::RemoveFootprint),
            0,
            "R2 is locked and stays"
        );
        assert!(plan
            .warnings()
            .any(|w| w.text.contains("R2") && w.text.contains("locked")));
        assert_eq!(plan.count(PlanKind::RemoveNet), 1);
        let removed = plan
            .lines
            .iter()
            .find(|l| l.kind == PlanKind::RemoveNet)
            .unwrap();
        assert!(removed.text.contains("1 track"), "{}", removed.text);
        // Applying keeps the track, now without a net.
        run(&mut e);
        assert_eq!(e.project().pcb().tracks[&tid].net, None);
        assert!(e.project().pcb().nets.is_empty());
        // Unlock and update again: R2 goes away.
        let before = e.project().pcb().object(r2).unwrap();
        let PcbObject::Footprint(mut f) = before.clone() else {
            panic!()
        };
        f.locked = false;
        e.execute(crate::commands::PcbEditCommand::new(
            "unlock",
            vec![PcbChange::modify(r2, before, PcbObject::Footprint(f))],
        ))
        .unwrap();
        run(&mut e);
        assert!(e
            .project()
            .pcb()
            .placed
            .values()
            .all(|f| f.reference != "R2"));
    }

    #[test]
    fn renamed_nets_keep_their_id_and_missing_data_is_reported() {
        let mut e = schematic_engine();
        run(&mut e);
        let net = *e.project().pcb().nets.keys().next().unwrap();
        // Name the net with a label: same pads, new name.
        let lid = e.new_id();
        e.execute(crate::commands::SchematicEditCommand::new(
            "label",
            vec![Change::create(
                ROOT_SHEET,
                lid,
                Object::NetLabel(buturecad_schematic::NetLabel {
                    position: Point::new(5 * G, 3 * G),
                    name: "SIG".into(),
                    orientation: buturecad_schematic::Direction::Right,
                }),
            )],
        ))
        .unwrap();
        // The label must sit on the wire to attach (wire from (0,-3) to (10,3) is diagonal);
        // use the wire end instead.
        let lbl = e.project().schematic().object(lid).unwrap().1.clone();
        let Object::NetLabel(mut l) = lbl.clone() else {
            panic!()
        };
        l.position = Point::new(10 * G, 3 * G);
        e.execute(crate::commands::SchematicEditCommand::new(
            "move",
            vec![Change::modify(ROOT_SHEET, lid, lbl, Object::NetLabel(l))],
        ))
        .unwrap();
        let plan = plan_update(e.project(), &resolver);
        assert_eq!(plan.count(PlanKind::RenameNet), 1, "{:?}", plan.lines);
        assert_eq!(plan.count(PlanKind::AddNet), 0);
        run(&mut e);
        assert_eq!(e.project().pcb().nets.len(), 1);
        assert_eq!(e.project().pcb().nets[&net].name, "/SIG");

        // A part without footprint, an unknown footprint and an unannotated part are reported.
        let mut e2 = schematic_engine();
        let (sid, obj) = (
            ROOT_SHEET,
            e2.project().schematic().object(Id(1)).unwrap().1.clone(),
        );
        let Object::Component(mut c) = obj.clone() else {
            panic!()
        };
        c.footprint = String::new();
        e2.execute(crate::commands::SchematicEditCommand::new(
            "e",
            vec![Change::modify(sid, Id(1), obj, Object::Component(c))],
        ))
        .unwrap();
        let obj2 = e2.project().schematic().object(Id(2)).unwrap().1.clone();
        let Object::Component(mut c2) = obj2.clone() else {
            panic!()
        };
        c2.footprint = "Nope:X".into();
        e2.execute(crate::commands::SchematicEditCommand::new(
            "e",
            vec![Change::modify(sid, Id(2), obj2, Object::Component(c2))],
        ))
        .unwrap();
        let plan = plan_update(e2.project(), &resolver);
        let w: Vec<&str> = plan.warnings().map(|l| l.text.as_str()).collect();
        assert!(
            w.iter()
                .any(|t| t.contains("R1") && t.contains("no footprint")),
            "{w:?}"
        );
        assert!(
            w.iter().any(|t| t.contains("R2") && t.contains("Nope:X")),
            "{w:?}"
        );
        assert!(plan.is_empty(), "nothing can be placed");
    }
}
