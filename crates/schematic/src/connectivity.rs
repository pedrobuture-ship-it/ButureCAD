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
//!
//! # Incremental updates
//!
//! [`ConnectivityEngine`] keeps one cached graph per sheet (geometry attachment, local labels,
//! buses, dangling ends) and a cheap global phase that merges them (global names, power symbols,
//! hierarchy, net naming). [`ConnectivityEngine::update`] rebuilds only the sheets a change set
//! touched and re-runs the global phase, whose cost is proportional to the number of *nets*, not
//! the number of objects. The result is always identical to a from-scratch build (checked by
//! property tests). Granularity is one sheet: a very large flat sheet is rebuilt as a whole.
//! Edits that do not go through a change set (symbol definitions, sheet renames) need
//! [`ConnectivityEngine::refresh`].

use crate::bus::{looks_like_bus, BusDef, BusNameError};
use crate::model::*;
use crate::schematic::Change;
use crate::schematic::Schematic;
use buturecad_geometry::{Bounded, Point, Segment, SpatialIndex};
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
    /// A power symbol is attached (the net is a supply rail by definition).
    pub power: bool,
    /// Named across the whole design (power symbol or global label).
    pub global: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// Several different names of the same kind (power symbols, global labels or local labels)
    /// compete for one net; the smallest wins. Hierarchical labels and sheet pins are aliases and
    /// never conflict.
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

/// A wire end that is attached to nothing (no other wire end, pin, label, junction, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingEnd {
    pub sheet: Id,
    pub wire: Id,
    pub at: Point,
    /// The end lies on the *body* of this other wire without a junction, so it looks connected
    /// but is not.
    pub touches: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusInfo {
    pub label: Id,
    pub sheet: Id,
    pub name: String,
    pub def: BusDef,
    pub segments: Vec<Id>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Connectivity {
    nets: Vec<Net>,
    pin_nets: BTreeMap<(Id, String), NetId>,
    no_connects: BTreeSet<(Id, String)>,
    buses: Vec<BusInfo>,
    issues: Vec<Issue>,
    dangling: Vec<DanglingEnd>,
    sheets: Vec<Id>,
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
    /// Sheets reachable from the root, breadth first (the sheets the nets were built from).
    pub fn reachable_sheets(&self) -> &[Id] {
        &self.sheets
    }
    /// Wire ends that attach to nothing, in sheet and object order.
    pub fn dangling_ends(&self) -> &[DanglingEnd] {
        &self.dangling
    }
    /// Every pin electrically connected to `pin` (excluding itself), sorted.
    pub fn connected_pins(&self, component: Id, pin: &str) -> Vec<&PinRef> {
        self.net_of_pin(component, pin)
            .map(|n| {
                n.pins
                    .iter()
                    .filter(|p| !(p.component == component && p.pin == pin))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Builds connectivity for the whole design (all sheets reachable from the root).
    ///
    /// For repeated queries on a changing design keep a [`ConnectivityEngine`] instead.
    pub fn build(sch: &Schematic) -> Connectivity {
        ConnectivityEngine::new(sch).into_connectivity()
    }
}

// ---------------------------------------------------------------------------------------------
// Per-sheet graph
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
    NetLabel(Id, String),
    GlobalLabel(Id, String),
    HierLabel(Id, String),
    Power(Id, String),
    SheetPin {
        name: String,
        instance: Id,
    },
}

struct Uf {
    parent: Vec<usize>,
}

impl Uf {
    fn new() -> Self {
        Self { parent: Vec::new() }
    }
    fn with_len(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
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

const PRIO_POWER: u8 = 4;
const PRIO_GLOBAL: u8 = 3;
const PRIO_LOCAL: u8 = 2;
/// Hierarchical labels and sheet pins are *aliases* of one signal across hierarchy levels; they
/// never conflict with each other. The name closest to the root wins.
const PRIO_HIER: u8 = 1;

/// One connected group of objects inside a single sheet.
#[derive(Debug, Clone)]
struct Comp {
    pins: Vec<PinRef>,
    wires: Vec<Id>,
    labels: Vec<Id>,
    /// Name candidates `(priority, name)`: 4 power symbol, 3 global label, 2 local net label,
    /// 1 hierarchical label or sheet pin.
    candidates: Vec<(u8, String)>,
    first_object: Id,
    junction_only: bool,
    power: bool,
    global: bool,
}

/// Everything derived from one sheet alone.
#[derive(Debug, Clone, Default)]
struct SheetGraph {
    comps: Vec<Comp>,
    /// Global names (power symbols, global labels) and the component that carries them.
    globals: Vec<(String, usize)>,
    hier_labels: BTreeMap<String, usize>,
    /// Hierarchical label objects in object order, for the "label without sheet pin" check.
    hier_label_objs: Vec<(Id, String)>,
    /// Sheet pins in object order: (component, child sheet, pin name).
    sheet_pins: Vec<(usize, Id, String)>,
    children: Vec<Id>,
    no_connects: BTreeSet<(Id, String)>,
    buses: Vec<BusInfo>,
    bus_issues: Vec<Issue>,
    dangling: Vec<DanglingEnd>,
}

/// Local names are scoped to their sheet: `/NAME` on the root sheet, `/Sheet/NAME` below it.
fn qualify_local(sch: &Schematic, sheet: Id, name: &str) -> String {
    if sheet == sch.root() {
        format!("/{name}")
    } else {
        let sheet_name = sch.sheet(sheet).map_or("?", |s| s.name.as_str());
        format!("/{sheet_name}/{name}")
    }
}

fn register(uf: &mut Uf, map: &mut HashMap<String, usize>, key: &str, n: usize) {
    match map.get(key) {
        Some(first) => uf.union(*first, n),
        None => {
            map.insert(key.to_owned(), n);
        }
    }
}

impl SheetGraph {
    fn build(sch: &Schematic, sid: Id) -> SheetGraph {
        let mut g = SheetGraph::default();
        let Some(sheet) = sch.sheet(sid) else {
            return g;
        };
        let mut kinds: Vec<Kind> = Vec::new();
        let mut uf = Uf::new();
        let node = |kinds: &mut Vec<Kind>, uf: &mut Uf, kind: Kind| {
            kinds.push(kind);
            uf.add()
        };
        let mut wires: Vec<(usize, Id, Segment)> = Vec::new();
        let mut points: Vec<(usize, Point)> = Vec::new();
        let mut globals: HashMap<String, usize> = HashMap::new();
        let mut locals: HashMap<String, usize> = HashMap::new();
        let mut hier: BTreeMap<String, usize> = BTreeMap::new();
        let mut sheet_pins: Vec<(usize, Id, String)> = Vec::new();
        let mut bus_labels: Vec<(Id, String, Point)> = Vec::new();
        let mut bus_segments: Vec<(Id, Segment)> = Vec::new();

        for (oid, obj) in &sheet.objects {
            match obj {
                Object::Wire(w) => {
                    let n = node(&mut kinds, &mut uf, Kind::Wire(*oid));
                    wires.push((n, *oid, w.segment));
                }
                Object::Bus(b) => bus_segments.push((*oid, b.segment)),
                Object::Junction(j) => {
                    let n = node(&mut kinds, &mut uf, Kind::Junction);
                    points.push((n, j.position));
                }
                // Not a union element; the pins at its point are marked below.
                Object::NoConnect(_) => {}
                Object::Component(c) => {
                    let mut same_number: HashMap<String, usize> = HashMap::new();
                    for pin in sch.placed_pins(c) {
                        let n = node(
                            &mut kinds,
                            &mut uf,
                            Kind::Pin {
                                component: *oid,
                                reference: c.reference.clone(),
                                number: pin.number.clone(),
                            },
                        );
                        points.push((n, pin.position));
                        // Duplicate pin numbers on one symbol are the same electrical pin.
                        match same_number.get(&pin.number) {
                            Some(first) => uf.union(*first, n),
                            None => {
                                same_number.insert(pin.number, n);
                            }
                        }
                    }
                }
                Object::PowerSymbol(p) => {
                    if let Some(at) = sch.power_pin_position(p) {
                        let n = node(&mut kinds, &mut uf, Kind::Power(*oid, p.net_name.clone()));
                        points.push((n, at));
                        register(&mut uf, &mut globals, &p.net_name, n);
                    }
                }
                Object::GlobalLabel(l) => {
                    let n = node(&mut kinds, &mut uf, Kind::GlobalLabel(*oid, l.name.clone()));
                    points.push((n, l.position));
                    register(&mut uf, &mut globals, &l.name, n);
                }
                Object::NetLabel(l) => {
                    if looks_like_bus(&l.name) {
                        bus_labels.push((*oid, l.name.clone(), l.position));
                    } else {
                        let n = node(&mut kinds, &mut uf, Kind::NetLabel(*oid, l.name.clone()));
                        points.push((n, l.position));
                        register(&mut uf, &mut locals, &l.name, n);
                    }
                }
                Object::HierLabel(l) => {
                    let n = node(&mut kinds, &mut uf, Kind::HierLabel(*oid, l.name.clone()));
                    points.push((n, l.position));
                    register(&mut uf, &mut locals, &l.name, n);
                    hier.insert(l.name.clone(), n);
                    g.hier_label_objs.push((*oid, l.name.clone()));
                }
                Object::Sheet(inst) => {
                    g.children.push(inst.child);
                    for pin in &inst.pins {
                        let n = node(
                            &mut kinds,
                            &mut uf,
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
        let mut end_count: HashMap<Point, u32> = HashMap::new();
        for (n, _, seg) in &wires {
            for p in [seg.a, seg.b] {
                *end_count.entry(p).or_insert(0) += 1;
                match endpoints.get(&p) {
                    Some(first) => uf.union(*first, *n),
                    None => {
                        endpoints.insert(p, *n);
                    }
                }
            }
        }
        // 2. Point elements attach to every wire through their point.
        let index = SpatialIndex::bulk_load(
            wires
                .iter()
                .map(|(n, id, s)| (s.bounding_box(), (*n, *id, *s))),
        );
        for (n, p) in &points {
            for (_, (wn, _, seg)) in index.query_point(*p) {
                if seg.contains_point(*p) {
                    uf.union(*n, *wn);
                }
            }
        }
        // 3. Point elements at the same point are connected.
        let mut at_point: HashMap<Point, usize> = HashMap::new();
        for (n, p) in &points {
            match at_point.get(p) {
                Some(first) => uf.union(*first, *n),
                None => {
                    at_point.insert(*p, *n);
                }
            }
        }
        // 4. No-connect markers.
        for obj in sheet.objects.values() {
            if let Object::NoConnect(nc) = obj {
                for (n, p) in &points {
                    if *p == nc.position {
                        if let Kind::Pin {
                            component, number, ..
                        } = &kinds[*n]
                        {
                            g.no_connects.insert((*component, number.clone()));
                        }
                    }
                }
            }
        }
        // 5. Dangling wire ends: attached to no other wire end and no point element.
        for (_, id, seg) in &wires {
            for p in [seg.a, seg.b] {
                let others =
                    end_count.get(&p).copied().unwrap_or(0) > 1 || at_point.contains_key(&p);
                if others {
                    continue;
                }
                let touches = index
                    .query_point(p)
                    .into_iter()
                    .find(|(_, (_, other, s))| other != id && s.contains_point(p))
                    .map(|(_, (_, other, _))| *other);
                g.dangling.push(DanglingEnd {
                    sheet: sid,
                    wire: *id,
                    at: p,
                    touches,
                });
            }
        }

        g.collect_buses(sid, bus_labels, bus_segments);
        g.collect_comps(sch, sid, &kinds, &mut uf, &globals, &hier, &sheet_pins);
        g
    }

    fn collect_buses(
        &mut self,
        sid: Id,
        labels: Vec<(Id, String, Point)>,
        segments: Vec<(Id, Segment)>,
    ) {
        // Union bus segments sharing an endpoint.
        let mut uf = Uf::new();
        let ids: Vec<usize> = segments.iter().map(|_| uf.add()).collect();
        let mut ends: HashMap<Point, usize> = HashMap::new();
        for (i, (_, seg)) in segments.iter().enumerate() {
            for p in [seg.a, seg.b] {
                match ends.get(&p) {
                    Some(first) => uf.union(*first, ids[i]),
                    None => {
                        ends.insert(p, ids[i]);
                    }
                }
            }
        }
        for (label, name, at) in labels {
            let on: Vec<usize> = segments
                .iter()
                .enumerate()
                .filter(|(_, (_, seg))| seg.contains_point(at))
                .map(|(i, _)| i)
                .collect();
            let Some(&first) = on.first() else {
                self.bus_issues.push(Issue::BadBusLabel {
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
                    self.bus_issues.push(Issue::BadBusLabel {
                        label,
                        reason: e.to_string(),
                    });
                    continue;
                }
            };
            let root = uf.find(ids[first]);
            let members = segments
                .iter()
                .enumerate()
                .filter(|(i, _)| uf.find(ids[*i]) == root)
                .map(|(_, (oid, _))| *oid)
                .collect();
            self.buses.push(BusInfo {
                label,
                sheet: sid,
                name,
                def,
                segments: members,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_comps(
        &mut self,
        sch: &Schematic,
        sid: Id,
        kinds: &[Kind],
        uf: &mut Uf,
        globals: &HashMap<String, usize>,
        hier: &BTreeMap<String, usize>,
        sheet_pins: &[(usize, Id, String)],
    ) {
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..kinds.len() {
            groups.entry(uf.find(i)).or_default().push(i);
        }
        let mut comp_of_root: HashMap<usize, usize> = HashMap::new();
        for (ci, (root, members)) in groups.iter().enumerate() {
            comp_of_root.insert(*root, ci);
            let mut c = Comp {
                pins: Vec::new(),
                wires: Vec::new(),
                labels: Vec::new(),
                candidates: Vec::new(),
                first_object: Id(u64::MAX),
                junction_only: members.iter().all(|m| matches!(kinds[*m], Kind::Junction)),
                power: false,
                global: false,
            };
            for &m in members {
                match &kinds[m] {
                    Kind::Wire(id) => {
                        c.wires.push(*id);
                        c.first_object = c.first_object.min(*id);
                    }
                    Kind::Pin {
                        component,
                        reference,
                        number,
                    } => {
                        c.pins.push(PinRef {
                            reference: reference.clone(),
                            pin: number.clone(),
                            component: *component,
                            sheet: sid,
                        });
                        c.first_object = c.first_object.min(*component);
                    }
                    Kind::Junction => {}
                    Kind::Power(id, name) => {
                        c.labels.push(*id);
                        c.first_object = c.first_object.min(*id);
                        c.candidates.push((PRIO_POWER, name.clone()));
                        c.power = true;
                        c.global = true;
                    }
                    Kind::GlobalLabel(id, name) => {
                        c.labels.push(*id);
                        c.first_object = c.first_object.min(*id);
                        c.candidates.push((PRIO_GLOBAL, name.clone()));
                        c.global = true;
                    }
                    Kind::NetLabel(id, name) => {
                        c.labels.push(*id);
                        c.first_object = c.first_object.min(*id);
                        c.candidates
                            .push((PRIO_LOCAL, qualify_local(sch, sid, name)));
                    }
                    Kind::HierLabel(id, name) => {
                        c.labels.push(*id);
                        c.first_object = c.first_object.min(*id);
                        c.candidates
                            .push((PRIO_HIER, qualify_local(sch, sid, name)));
                    }
                    Kind::SheetPin { name, instance } => {
                        c.labels.push(*instance);
                        c.first_object = c.first_object.min(*instance);
                        c.candidates
                            .push((PRIO_HIER, qualify_local(sch, sid, name)));
                    }
                }
            }
            self.comps.push(c);
        }
        let comp_of = |uf: &mut Uf, node: usize| comp_of_root[&uf.find(node)];
        let mut seen: BTreeSet<(String, usize)> = BTreeSet::new();
        let mut global_nodes: Vec<(String, usize)> = globals
            .iter()
            .map(|(name, node)| (name.clone(), *node))
            .collect();
        global_nodes.sort();
        for (name, node) in global_nodes {
            let ci = comp_of(uf, node);
            if seen.insert((name.clone(), ci)) {
                self.globals.push((name, ci));
            }
        }
        for (name, node) in hier {
            let ci = comp_of(uf, *node);
            self.hier_labels.insert(name.clone(), ci);
        }
        for (node, child, name) in sheet_pins {
            let ci = comp_of(uf, *node);
            self.sheet_pins.push((ci, *child, name.clone()));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------------

/// What an update rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdateStats {
    /// Sheet graphs rebuilt from the model.
    pub sheets_rebuilt: usize,
    /// Sheet graphs reused unchanged.
    pub sheets_reused: usize,
}

/// A connectivity result that can be kept up to date cheaply. See the module documentation.
#[derive(Debug, Clone)]
pub struct ConnectivityEngine {
    graphs: BTreeMap<Id, SheetGraph>,
    result: Connectivity,
}

impl ConnectivityEngine {
    /// Builds every sheet and the merged result.
    pub fn new(sch: &Schematic) -> Self {
        let graphs = sch
            .sheets()
            .keys()
            .map(|sid| (*sid, SheetGraph::build(sch, *sid)))
            .collect();
        let mut e = Self {
            graphs,
            result: Connectivity::default(),
        };
        e.result = compose(sch, &e.graphs);
        e
    }

    pub fn connectivity(&self) -> &Connectivity {
        &self.result
    }

    pub fn into_connectivity(self) -> Connectivity {
        self.result
    }

    /// Brings the result up to date after `changes` were applied to (or reverted from) `sch`.
    /// Only sheets named by the changes (and sheets that appeared or disappeared) are rebuilt.
    pub fn update(&mut self, sch: &Schematic, changes: &[Change]) -> UpdateStats {
        let mut dirty: BTreeSet<Id> = changes.iter().map(|c| c.sheet).collect();
        self.graphs.retain(|sid, _| sch.sheet(*sid).is_some());
        for sid in sch.sheets().keys() {
            if !self.graphs.contains_key(sid) {
                dirty.insert(*sid);
            }
        }
        self.rebuild(sch, dirty)
    }

    /// Rebuilds the listed sheets (for changes that are not change sets, such as a renamed
    /// sheet) and the merged result.
    pub fn invalidate(&mut self, sch: &Schematic, sheets: &[Id]) -> UpdateStats {
        self.graphs.retain(|sid, _| sch.sheet(*sid).is_some());
        let mut dirty: BTreeSet<Id> = sheets.iter().copied().collect();
        for sid in sch.sheets().keys() {
            if !self.graphs.contains_key(sid) {
                dirty.insert(*sid);
            }
        }
        self.rebuild(sch, dirty)
    }

    /// Rebuilds everything (symbol definitions changed, unknown edits, …).
    pub fn refresh(&mut self, sch: &Schematic) -> UpdateStats {
        let all: BTreeSet<Id> = sch.sheets().keys().copied().collect();
        self.graphs.clear();
        self.rebuild(sch, all)
    }

    fn rebuild(&mut self, sch: &Schematic, dirty: BTreeSet<Id>) -> UpdateStats {
        let mut rebuilt = 0;
        for sid in dirty {
            if sch.sheet(sid).is_some() {
                self.graphs.insert(sid, SheetGraph::build(sch, sid));
                rebuilt += 1;
            }
        }
        self.result = compose(sch, &self.graphs);
        UpdateStats {
            sheets_rebuilt: rebuilt,
            sheets_reused: self.graphs.len().saturating_sub(rebuilt),
        }
    }
}

/// The global phase: merges sheet graphs into nets.
fn compose(sch: &Schematic, graphs: &BTreeMap<Id, SheetGraph>) -> Connectivity {
    // Sheets reachable from the root, breadth first.
    let mut order = vec![sch.root()];
    let mut seen: BTreeSet<Id> = order.iter().copied().collect();
    let mut depth: BTreeMap<Id, u32> = BTreeMap::from([(sch.root(), 0)]);
    let mut i = 0;
    while i < order.len() {
        let sid = order[i];
        i += 1;
        if let Some(g) = graphs.get(&sid) {
            for child in &g.children {
                if seen.insert(*child) {
                    depth.insert(*child, depth[&sid] + 1);
                    order.push(*child);
                }
            }
        }
    }
    let empty = SheetGraph::default();
    let graph = |sid: Id| graphs.get(&sid).unwrap_or(&empty);

    let mut issues: Vec<Issue> = Vec::new();
    for sid in sch.sheets().keys() {
        if !seen.contains(sid) {
            issues.push(Issue::UnreachableSheet { sheet: *sid });
        }
    }

    // Global component indices.
    let mut base: BTreeMap<Id, usize> = BTreeMap::new();
    let mut total = 0;
    for sid in &order {
        base.insert(*sid, total);
        total += graph(*sid).comps.len();
    }
    let mut uf = Uf::with_len(total);

    // Global names join across sheets.
    let mut globals: HashMap<&str, usize> = HashMap::new();
    for sid in &order {
        for (name, ci) in &graph(*sid).globals {
            let at = base[sid] + ci;
            match globals.get(name.as_str()) {
                Some(first) => uf.union(*first, at),
                None => {
                    globals.insert(name.as_str(), at);
                }
            }
        }
    }

    // Hierarchy: sheet pin <-> hierarchical label of the same name in the child sheet.
    let mut linked: BTreeSet<(Id, &str)> = BTreeSet::new();
    for sid in &order {
        for (ci, child, name) in &graph(*sid).sheet_pins {
            let target = graphs
                .get(child)
                .filter(|_| base.contains_key(child))
                .and_then(|g| g.hier_labels.get(name));
            match target {
                Some(label_ci) => {
                    uf.union(base[sid] + ci, base[child] + label_ci);
                    linked.insert((*child, name.as_str()));
                }
                None => issues.push(Issue::SheetPinWithoutLabel {
                    sheet: *sid,
                    pin: name.clone(),
                }),
            }
        }
    }
    for sid in &order {
        for (oid, name) in &graph(*sid).hier_label_objs {
            if !linked.contains(&(*sid, name.as_str())) {
                issues.push(Issue::HierLabelWithoutPin {
                    sheet: *sid,
                    label: *oid,
                });
            }
        }
    }
    for sid in &order {
        issues.extend(graph(*sid).bus_issues.iter().cloned());
    }

    // Group components into nets.
    struct Draft {
        name: String,
        named: bool,
        conflict: Option<Vec<String>>,
        pins: Vec<PinRef>,
        wires: Vec<Id>,
        labels: Vec<Id>,
        sheets: BTreeSet<Id>,
        first_object: Id,
        power: bool,
        global: bool,
    }
    let mut groups: BTreeMap<usize, Vec<(Id, usize)>> = BTreeMap::new();
    for sid in &order {
        for ci in 0..graph(*sid).comps.len() {
            groups
                .entry(uf.find(base[sid] + ci))
                .or_default()
                .push((*sid, ci));
        }
    }
    let mut drafts: Vec<Draft> = Vec::new();
    for members in groups.values() {
        let comps: Vec<(Id, &Comp)> = members
            .iter()
            .map(|(sid, ci)| (*sid, &graph(*sid).comps[*ci]))
            .collect();
        if comps.iter().all(|(_, c)| c.junction_only) {
            continue;
        }
        let mut pins = Vec::new();
        let mut wires = Vec::new();
        let mut labels = Vec::new();
        let mut sheets = BTreeSet::new();
        // (priority, depth of the sheet that carries the name, name)
        let mut candidates: Vec<(u8, u32, &str)> = Vec::new();
        let mut first_object = Id(u64::MAX);
        let (mut power, mut global) = (false, false);
        for (sid, c) in &comps {
            sheets.insert(*sid);
            pins.extend(c.pins.iter().cloned());
            wires.extend(c.wires.iter().copied());
            labels.extend(c.labels.iter().copied());
            candidates.extend(
                c.candidates
                    .iter()
                    .map(|(p, n)| (*p, depth[sid], n.as_str())),
            );
            first_object = first_object.min(c.first_object);
            power |= c.power;
            global |= c.global;
        }
        pins.sort();
        pins.dedup();
        wires.sort();
        labels.sort();
        labels.dedup();

        let best = candidates.iter().map(|c| c.0).max();
        let (name, named, conflict) = match best {
            Some(PRIO_HIER) => {
                // Aliases of one signal: the name nearest the root wins, never a conflict.
                let chosen = candidates
                    .iter()
                    .filter(|c| c.0 == PRIO_HIER)
                    .map(|c| (c.1, c.2))
                    .min()
                    .map(|(_, n)| n.to_owned())
                    .unwrap_or_default();
                (chosen, true, None)
            }
            Some(prio) => {
                let names: BTreeSet<&str> = candidates
                    .iter()
                    .filter(|c| c.0 == prio)
                    .map(|c| c.2)
                    .collect();
                let chosen = names
                    .iter()
                    .next()
                    .map(|s| (*s).to_owned())
                    .unwrap_or_default();
                let conflict = (names.len() > 1)
                    .then(|| names.into_iter().map(str::to_owned).collect::<Vec<_>>());
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
            power,
            global,
        });
    }

    // Deterministic order and unique names.
    drafts
        .sort_by(|a, b| (a.name.as_str(), a.first_object).cmp(&(b.name.as_str(), b.first_object)));
    let mut counts: HashMap<String, u32> = HashMap::new();
    for d in &mut drafts {
        let count = counts.entry(d.name.clone()).or_insert(0);
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
            issues.push(Issue::NameConflict { net: id, names });
        }
        nets.push(Net {
            id,
            name: d.name,
            named: d.named,
            pins: d.pins,
            wires: d.wires,
            labels: d.labels,
            sheets: d.sheets,
            power: d.power,
            global: d.global,
        });
    }

    let mut no_connects = BTreeSet::new();
    let mut buses: Vec<BusInfo> = Vec::new();
    let mut dangling = Vec::new();
    for sid in &order {
        let g = graph(*sid);
        no_connects.extend(g.no_connects.iter().cloned());
        buses.extend(g.buses.iter().cloned());
        dangling.extend(g.dangling.iter().cloned());
    }
    buses.sort_by_key(|b| b.label);

    Connectivity {
        nets,
        pin_nets,
        no_connects,
        buses,
        issues,
        dangling,
        sheets: order,
    }
}
