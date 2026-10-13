//! Logical connectivity, derived from the model; never from rendering.
//!
//! # Rules (documented in `docs/schematic-connectivity.md`)
//!
//! * Wires that **end at the same point** are connected.
//! * A wire **crossing** another, or one wire ending on the **interior** of another, is *not*
//!   connected unless a [`Junction`](crate::model::Junction) sits at that point.
//! * Pins, labels, power symbols, sheet pins and hierarchical labels attach to every wire that
//!   passes through or ends at their point, and to each other when they coincide.
//! * Net labels join nets with the same name **within a sheet**; global labels and power symbols
//!   join nets with the same name **across the design**; a sheet pin joins the hierarchical
//!   label of the same name in the child sheet.
//! * Buses name groups of signals but make no electrical connection themselves.

use crate::bus::{looks_like_bus, BusDef, BusNameError};
use crate::model::*;
use crate::schematic::Schematic;
use buturecad_geometry::{Bounded, Point, SpatialIndex};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetId(pub u32);

/// A component pin that belongs to a net.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PinRef {
    pub reference: String,
    pub pin: String,
    pub component: Id,
    pub sheet: Id,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub id: NetId,
    pub name: String,
    /// `true` when the name comes from a label or power symbol; `false` for generated names.
    pub named: bool,
    pub pins: Vec<PinRef>,
    pub wires: Vec<Id>,
    /// Labels, power symbols and sheet pins attached to the net.
    pub labels: Vec<Id>,
    pub sheets: BTreeSet<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// Several different names compete for one net; the highest-priority, then smallest, wins.
    NameConflict { net: NetId, names: Vec<String> },
    /// A sheet pin has no hierarchical label of the same name in its child sheet.
    SheetPinWithoutLabel { sheet: Id, pin: String },
    /// A hierarchical label has no sheet pin of the same name in the parent.
    HierLabelWithoutPin { sheet: Id, label: Id },
    /// A bracketed label that is not on a bus, or a plain label on a bus, or an invalid bus name.
    BadBusLabel { label: Id, reason: String },
    /// The sheet is not reachable from the root and was ignored.
    UnreachableSheet { sheet: Id },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusInfo {
    pub label: Id,
    pub sheet: Id,
    pub name: String,
    pub def: BusDef,
    pub segments: Vec<Id>,
}

#[derive(Debug, Clone, Default)]
pub struct Connectivity {
    nets: Vec<Net>,
    pin_nets: BTreeMap<(Id, String), NetId>,
    no_connects: BTreeSet<(Id, String)>,
    buses: Vec<BusInfo>,
    issues: Vec<Issue>,
}

impl Connectivity {
    pub fn nets(&self) -> &[Net] {
        &self.nets
    }
    pub fn net(&self, id: NetId) -> Option<&Net> {
        self.nets.get(id.0 as usize)
    }
    pub fn net_by_name(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }
    /// Net of a component pin (by component id and pin number).
    pub fn net_of_pin(&self, component: Id, pin: &str) -> Option<&Net> {
        self.pin_nets
            .get(&(component, pin.to_owned()))
            .and_then(|id| self.net(*id))
    }
    /// Is the pin covered by a no-connect marker?
    pub fn has_no_connect(&self, component: Id, pin: &str) -> bool {
        self.no_connects.contains(&(component, pin.to_owned()))
    }
    pub fn buses(&self) -> &[BusInfo] {
        &self.buses
    }
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// Builds connectivity for the whole design (all sheets reachable from the root).
    pub fn build(sch: &Schematic) -> Connectivity {
        Builder::new(sch).run()
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Kind {
    Wire(Id),
    Pin {
        component: Id,
        reference: String,
        number: String,
    },
    Junction,
    NetLabel(Id),
    GlobalLabel(Id),
    HierLabel(Id),
    Power(Id),
    SheetPin {
        name: String,
        instance: Id,
    },
}

struct Node {
    sheet: Id,
    kind: Kind,
}

struct Uf {
    parent: Vec<usize>,
}

impl Uf {
    fn new() -> Self {
        Self { parent: Vec::new() }
    }
    fn add(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Keep the smaller index as the root so results do not depend on union order.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}

struct Builder<'a> {
    sch: &'a Schematic,
    nodes: Vec<Node>,
    uf: Uf,
    issues: Vec<Issue>,
    no_connects: BTreeSet<(Id, String)>,
    /// (sheet id, bus-label object, bus name, anchor point)
    bus_labels: Vec<(Id, Id, String, Point)>,
    bus_segments: Vec<(Id, Id, buturecad_geometry::Segment)>,
}

impl<'a> Builder<'a> {
    fn new(sch: &'a Schematic) -> Self {
        Self {
            sch,
            nodes: Vec::new(),
            uf: Uf::new(),
            issues: Vec::new(),
            no_connects: BTreeSet::new(),
            bus_labels: Vec::new(),
            bus_segments: Vec::new(),
        }
    }

    fn node(&mut self, sheet: Id, kind: Kind) -> usize {
        self.nodes.push(Node { sheet, kind });
        self.uf.add()
    }

    fn reachable_sheets(&self) -> Vec<Id> {
        let mut order = vec![self.sch.root()];
        let mut seen: BTreeSet<Id> = order.iter().copied().collect();
        let mut i = 0;
        while i < order.len() {
            let sheet = order[i];
            i += 1;
            if let Some(s) = self.sch.sheet(sheet) {
                for obj in s.objects.values() {
                    if let Object::Sheet(inst) = obj {
                        if seen.insert(inst.child) {
                            order.push(inst.child);
                        }
                    }
                }
            }
        }
        order
    }

    fn run(mut self) -> Connectivity {
        let sheets = self.reachable_sheets();
        for sid in self.sch.sheets().keys() {
            if !sheets.contains(sid) {
                self.issues.push(Issue::UnreachableSheet { sheet: *sid });
            }
        }
        // Global/local name registries: name -> first node, to merge same-named nets.
        let mut globals: HashMap<String, usize> = HashMap::new();
        let mut locals: HashMap<(Id, String), usize> = HashMap::new();
        let mut hier_labels: HashMap<(Id, String), usize> = HashMap::new();
        let mut sheet_pins: Vec<(usize, Id, String)> = Vec::new(); // node, child sheet, pin name

        for &sid in &sheets {
            let Some(sheet) = self.sch.sheet(sid) else {
                continue;
            };
            let mut wires: Vec<(usize, buturecad_geometry::Segment)> = Vec::new();
            let mut points: Vec<(usize, Point)> = Vec::new();

            for (oid, obj) in &sheet.objects {
                match obj {
                    Object::Wire(w) => {
                        let n = self.node(sid, Kind::Wire(*oid));
                        wires.push((n, w.segment));
                    }
                    Object::Bus(b) => self.bus_segments.push((sid, *oid, b.segment)),
                    Object::Junction(j) => {
                        let n = self.node(sid, Kind::Junction);
                        points.push((n, j.position));
                    }
                    Object::NoConnect(nc) => {
                        // Not a union element; mark the pins at this point after pins are known.
                        let _ = nc;
                    }
                    Object::Component(c) => {
                        let mut same_number: HashMap<String, usize> = HashMap::new();
                        for pin in self.sch.placed_pins(c) {
                            let n = self.node(
                                sid,
                                Kind::Pin {
                                    component: *oid,
                                    reference: c.reference.clone(),
                                    number: pin.number.clone(),
                                },
                            );
                            points.push((n, pin.position));
                            // Duplicate pin numbers on one symbol are the same electrical pin.
                            if let Some(first) = same_number.get(&pin.number) {
                                self.uf.union(*first, n);
                            } else {
                                same_number.insert(pin.number, n);
                            }
                        }
                    }
                    Object::PowerSymbol(p) => {
                        if let Some(at) = self.sch.power_pin_position(p) {
                            let n = self.node(sid, Kind::Power(*oid));
                            points.push((n, at));
                            if let Some(first) = globals.get(&p.net_name) {
                                self.uf.union(*first, n);
                            } else {
                                globals.insert(p.net_name.clone(), n);
                            }
                        }
                    }
                    Object::GlobalLabel(l) => {
                        let n = self.node(sid, Kind::GlobalLabel(*oid));
                        points.push((n, l.position));
                        if let Some(first) = globals.get(&l.name) {
                            self.uf.union(*first, n);
                        } else {
                            globals.insert(l.name.clone(), n);
                        }
                    }
                    Object::NetLabel(l) => {
                        if looks_like_bus(&l.name) {
                            self.bus_labels
                                .push((sid, *oid, l.name.clone(), l.position));
                        } else {
                            let n = self.node(sid, Kind::NetLabel(*oid));
                            points.push((n, l.position));
                            Self::register(&mut self.uf, &mut locals, (sid, l.name.clone()), n);
                        }
                    }
                    Object::HierLabel(l) => {
                        let n = self.node(sid, Kind::HierLabel(*oid));
                        points.push((n, l.position));
                        Self::register(&mut self.uf, &mut locals, (sid, l.name.clone()), n);
                        hier_labels.insert((sid, l.name.clone()), n);
                    }
                    Object::Sheet(inst) => {
                        for pin in &inst.pins {
                            let n = self.node(
                                sid,
                                Kind::SheetPin {
                                    name: pin.name.clone(),
                                    instance: *oid,
                                },
                            );
                            points.push((n, pin.position));
                            sheet_pins.push((n, inst.child, pin.name.clone()));
                        }
                    }
                }
            }

            // 1. Wires sharing an endpoint.
            let mut endpoints: HashMap<Point, usize> = HashMap::new();
            for (n, seg) in &wires {
                for p in [seg.a, seg.b] {
                    match endpoints.get(&p) {
                        Some(first) => self.uf.union(*first, *n),
                        None => {
                            endpoints.insert(p, *n);
                        }
                    }
                }
            }
            // 2. Point elements attach to every wire through their point.
            let index =
                SpatialIndex::bulk_load(wires.iter().map(|(n, s)| (s.bounding_box(), (*n, *s))));
            for (n, p) in &points {
                for (_, (wn, seg)) in index.query_point(*p) {
                    if seg.contains_point(*p) {
                        self.uf.union(*n, *wn);
                    }
                }
            }
            // 3. Point elements at the same point are connected.
            let mut at_point: HashMap<Point, usize> = HashMap::new();
            for (n, p) in &points {
                match at_point.get(p) {
                    Some(first) => self.uf.union(*first, *n),
                    None => {
                        at_point.insert(*p, *n);
                    }
                }
            }
            // 4. No-connect markers.
            for (oid, obj) in &sheet.objects {
                if let Object::NoConnect(nc) = obj {
                    let _ = oid;
                    for (n, p) in &points {
                        if *p == nc.position {
                            if let Kind::Pin {
                                component, number, ..
                            } = &self.nodes[*n].kind
                            {
                                self.no_connects.insert((*component, number.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Hierarchy: sheet pin ↔ hierarchical label of the same name in the child sheet.
        let mut linked_labels: BTreeSet<(Id, String)> = BTreeSet::new();
        for (n, child, name) in &sheet_pins {
            match hier_labels.get(&(*child, name.clone())) {
                Some(label) => {
                    self.uf.union(*n, *label);
                    linked_labels.insert((*child, name.clone()));
                }
                None => {
                    let parent = self.nodes[*n].sheet;
                    self.issues.push(Issue::SheetPinWithoutLabel {
                        sheet: parent,
                        pin: name.clone(),
                    });
                }
            }
        }
        for (i, node) in self.nodes.iter().enumerate() {
            if let Kind::HierLabel(oid) = node.kind {
                let name = match self.sch.object(oid) {
                    Some((_, Object::HierLabel(l))) => l.name.clone(),
                    _ => continue,
                };
                if !linked_labels.contains(&(node.sheet, name)) {
                    let _ = i;
                    self.issues.push(Issue::HierLabelWithoutPin {
                        sheet: node.sheet,
                        label: oid,
                    });
                }
            }
        }

        let buses = self.collect_buses();
        self.collect(buses)
    }

    fn register(uf: &mut Uf, map: &mut HashMap<(Id, String), usize>, key: (Id, String), n: usize) {
        match map.get(&key) {
            Some(first) => uf.union(*first, n),
            None => {
                map.insert(key, n);
            }
        }
    }

    fn collect_buses(&mut self) -> Vec<BusInfo> {
        // Union bus segments sharing an endpoint, per sheet.
        let mut uf = Uf::new();
        let ids: Vec<usize> = self.bus_segments.iter().map(|_| uf.add()).collect();
        let mut ends: HashMap<(Id, Point), usize> = HashMap::new();
        for (i, (sid, _, seg)) in self.bus_segments.iter().enumerate() {
            for p in [seg.a, seg.b] {
                match ends.get(&(*sid, p)) {
                    Some(first) => uf.union(*first, ids[i]),
                    None => {
                        ends.insert((*sid, p), ids[i]);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let labels = std::mem::take(&mut self.bus_labels);
        for (sid, label, name, at) in labels {
            let on: Vec<usize> = self
                .bus_segments
                .iter()
                .enumerate()
                .filter(|(_, (s, _, seg))| *s == sid && seg.contains_point(at))
                .map(|(i, _)| i)
                .collect();
            let Some(&first) = on.first() else {
                self.issues.push(Issue::BadBusLabel {
                    label,
                    reason: "bracketed label is not on a bus".into(),
                });
                continue;
            };
            let def = match BusDef::parse(&name) {
                Ok(d) => d,
                Err(
                    e @ (BusNameError::NotABus | BusNameError::BadRange | BusNameError::TooWide),
                ) => {
                    self.issues.push(Issue::BadBusLabel {
                        label,
                        reason: e.to_string(),
                    });
                    continue;
                }
            };
            let root = uf.find(ids[first]);
            let segments = self
                .bus_segments
                .iter()
                .enumerate()
                .filter(|(i, _)| uf.find(ids[*i]) == root)
                .map(|(_, (_, oid, _))| *oid)
                .collect();
            out.push(BusInfo {
                label,
                sheet: sid,
                name,
                def,
                segments,
            });
        }
        out.sort_by_key(|b| b.label);
        out
    }

    fn collect(mut self, buses: Vec<BusInfo>) -> Connectivity {
        // Group nodes by root.
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..self.nodes.len() {
            let r = self.uf.find(i);
            groups.entry(r).or_default().push(i);
        }

        struct Draft {
            name: String,
            named: bool,
            conflict: Option<Vec<String>>,
            pins: Vec<PinRef>,
            wires: Vec<Id>,
            labels: Vec<Id>,
            sheets: BTreeSet<Id>,
            first_object: Id,
        }
        let mut drafts: Vec<Draft> = Vec::new();

        for members in groups.values() {
            if members
                .iter()
                .all(|m| matches!(self.nodes[*m].kind, Kind::Junction))
            {
                continue;
            }
            let mut pins = Vec::new();
            let mut wires = Vec::new();
            let mut labels = Vec::new();
            let mut sheets = BTreeSet::new();
            // name candidates by priority: 3 power, 2 global, 1 local label, 0 sheet pin
            let mut candidates: Vec<(u8, String)> = Vec::new();
            let mut first_object = Id(u64::MAX);
            for &m in members {
                let node = &self.nodes[m];
                sheets.insert(node.sheet);
                match &node.kind {
                    Kind::Wire(id) => {
                        wires.push(*id);
                        first_object = first_object.min(*id);
                    }
                    Kind::Pin {
                        component,
                        reference,
                        number,
                    } => {
                        pins.push(PinRef {
                            reference: reference.clone(),
                            pin: number.clone(),
                            component: *component,
                            sheet: node.sheet,
                        });
                        first_object = first_object.min(*component);
                    }
                    Kind::Junction => {}
                    Kind::Power(id) => {
                        labels.push(*id);
                        first_object = first_object.min(*id);
                        if let Some((_, Object::PowerSymbol(p))) = self.sch.object(*id) {
                            candidates.push((3, p.net_name.clone()));
                        }
                    }
                    Kind::GlobalLabel(id) => {
                        labels.push(*id);
                        first_object = first_object.min(*id);
                        if let Some((_, Object::GlobalLabel(l))) = self.sch.object(*id) {
                            candidates.push((2, l.name.clone()));
                        }
                    }
                    Kind::NetLabel(id) | Kind::HierLabel(id) => {
                        labels.push(*id);
                        first_object = first_object.min(*id);
                        let name = match self.sch.object(*id) {
                            Some((_, Object::NetLabel(l))) => Some(l.name.clone()),
                            Some((_, Object::HierLabel(l))) => Some(l.name.clone()),
                            _ => None,
                        };
                        if let Some(n) = name {
                            candidates.push((1, self.qualify_local(node.sheet, &n)));
                        }
                    }
                    Kind::SheetPin { name, instance } => {
                        labels.push(*instance);
                        first_object = first_object.min(*instance);
                        candidates.push((0, self.qualify_local(node.sheet, name)));
                    }
                }
            }
            pins.sort();
            pins.dedup();
            wires.sort();
            labels.sort();
            labels.dedup();

            let best = candidates.iter().map(|c| c.0).max();
            let (name, named, conflict) = match best {
                Some(prio) => {
                    let names: BTreeSet<String> = candidates
                        .iter()
                        .filter(|c| c.0 == prio)
                        .map(|c| c.1.clone())
                        .collect();
                    let chosen = names.iter().next().cloned().unwrap_or_default();
                    let conflict = (names.len() > 1).then(|| names.into_iter().collect::<Vec<_>>());
                    (chosen, true, conflict)
                }
                None => match pins.first() {
                    Some(p) => (format!("Net-({}-Pad{})", p.reference, p.pin), false, None),
                    None => (format!("Net-(unnamed-{})", first_object.0), false, None),
                },
            };
            drafts.push(Draft {
                name,
                named,
                conflict,
                pins,
                wires,
                labels,
                sheets,
                first_object,
            });
        }

        // Deterministic order and unique names.
        drafts.sort_by(|a, b| {
            (a.name.as_str(), a.first_object).cmp(&(b.name.as_str(), b.first_object))
        });
        let mut seen: HashMap<String, u32> = HashMap::new();
        for d in &mut drafts {
            let count = seen.entry(d.name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                d.name = format!("{}_{}", d.name, count);
            }
        }

        let mut nets = Vec::new();
        let mut pin_nets = BTreeMap::new();
        for (i, d) in drafts.into_iter().enumerate() {
            let id = NetId(i as u32);
            for p in &d.pins {
                pin_nets.insert((p.component, p.pin.clone()), id);
            }
            if let Some(names) = d.conflict {
                self.issues.push(Issue::NameConflict { net: id, names });
            }
            nets.push(Net {
                id,
                name: d.name,
                named: d.named,
                pins: d.pins,
                wires: d.wires,
                labels: d.labels,
                sheets: d.sheets,
            });
        }
        Connectivity {
            nets,
            pin_nets,
            no_connects: self.no_connects,
            buses,
            issues: self.issues,
        }
    }

    /// Local names are scoped to their sheet: `/NAME` on the root sheet, `/Sheet/NAME` below it.
    fn qualify_local(&self, sheet: Id, name: &str) -> String {
        if sheet == self.sch.root() {
            format!("/{name}")
        } else {
            let sheet_name = self.sch.sheet(sheet).map_or("?", |s| s.name.as_str());
            format!("/{sheet_name}/{name}")
        }
    }
}
