//! Electrical Rules Check.
//!
//! ERC reads the [`Connectivity`] graph and the model's *data* (symbol pin types, references,
//! values, footprints). It never looks at pixels or at whether two drawn lines merely cross.
//!
//! Every finding has a stable **fingerprint** built from references, pin numbers and net names
//! (never object ids), so a documented exception ([`Waiver`]) keeps matching after edits.
//! Waived findings stay in the report, marked with their reason, and are excluded from the
//! counts. A waiver that no longer matches anything is reported as [`Rule::StaleWaiver`].
//!
//! The pin-compatibility matrix is documented in `docs/erc.md`.

use buturecad_geometry::Point;
use buturecad_schematic::{
    Component, Connectivity, Id, Issue, Net, Object, PinType, PlacedPin, Schematic, Waiver,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn id(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    UnconnectedPin,
    PinConflict,
    NoConnectPinConnected,
    InputNotDriven,
    PowerInputNotDriven,
    UnspecifiedPinType,
    DuplicateReference,
    Unannotated,
    MissingReference,
    UnknownSymbol,
    MissingValue,
    MissingFootprint,
    DanglingWire,
    MissingJunction,
    NetNameConflict,
    HierarchyMismatch,
    BusLabel,
    UnreachableSheet,
    SingleLabel,
    UnplacedUnit,
    StaleWaiver,
}

impl Rule {
    pub const ALL: [Rule; 21] = [
        Rule::UnconnectedPin,
        Rule::PinConflict,
        Rule::NoConnectPinConnected,
        Rule::InputNotDriven,
        Rule::PowerInputNotDriven,
        Rule::UnspecifiedPinType,
        Rule::DuplicateReference,
        Rule::Unannotated,
        Rule::MissingReference,
        Rule::UnknownSymbol,
        Rule::MissingValue,
        Rule::MissingFootprint,
        Rule::DanglingWire,
        Rule::MissingJunction,
        Rule::NetNameConflict,
        Rule::HierarchyMismatch,
        Rule::BusLabel,
        Rule::UnreachableSheet,
        Rule::SingleLabel,
        Rule::UnplacedUnit,
        Rule::StaleWaiver,
    ];

    /// Stable machine name (used in waivers and reports).
    pub fn code(self) -> &'static str {
        match self {
            Rule::UnconnectedPin => "unconnected_pin",
            Rule::PinConflict => "pin_conflict",
            Rule::NoConnectPinConnected => "no_connect_pin_connected",
            Rule::InputNotDriven => "input_not_driven",
            Rule::PowerInputNotDriven => "power_input_not_driven",
            Rule::UnspecifiedPinType => "unspecified_pin_type",
            Rule::DuplicateReference => "duplicate_reference",
            Rule::Unannotated => "unannotated",
            Rule::MissingReference => "missing_reference",
            Rule::UnknownSymbol => "unknown_symbol",
            Rule::MissingValue => "missing_value",
            Rule::MissingFootprint => "missing_footprint",
            Rule::DanglingWire => "dangling_wire",
            Rule::MissingJunction => "missing_junction",
            Rule::NetNameConflict => "net_name_conflict",
            Rule::HierarchyMismatch => "hierarchy_mismatch",
            Rule::BusLabel => "bus_label",
            Rule::UnreachableSheet => "unreachable_sheet",
            Rule::SingleLabel => "single_label",
            Rule::UnplacedUnit => "unplaced_unit",
            Rule::StaleWaiver => "stale_waiver",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Rule::UnconnectedPin => "Pin not connected",
            Rule::PinConflict => "Incompatible pins on one net",
            Rule::NoConnectPinConnected => "No-connect pin is connected",
            Rule::InputNotDriven => "Input pin not driven",
            Rule::PowerInputNotDriven => "Power input not driven",
            Rule::UnspecifiedPinType => "Pin type unspecified",
            Rule::DuplicateReference => "Duplicate reference",
            Rule::Unannotated => "Unannotated reference",
            Rule::MissingReference => "Missing reference",
            Rule::UnknownSymbol => "Unknown symbol",
            Rule::MissingValue => "Missing value",
            Rule::MissingFootprint => "Missing footprint",
            Rule::DanglingWire => "Dangling wire",
            Rule::MissingJunction => "Wire end without junction",
            Rule::NetNameConflict => "Conflicting net names",
            Rule::HierarchyMismatch => "Hierarchy mismatch",
            Rule::BusLabel => "Bad bus label",
            Rule::UnreachableSheet => "Unreachable sheet",
            Rule::SingleLabel => "Label used once",
            Rule::UnplacedUnit => "Unit not placed",
            Rule::StaleWaiver => "Waiver matches nothing",
        }
    }

    pub fn default_severity(self) -> Severity {
        match self {
            Rule::UnconnectedPin
            | Rule::NoConnectPinConnected
            | Rule::PowerInputNotDriven
            | Rule::DuplicateReference
            | Rule::MissingReference
            | Rule::UnknownSymbol
            | Rule::NetNameConflict
            | Rule::HierarchyMismatch
            | Rule::BusLabel => Severity::Error,
            // `PinConflict` carries its own severity per pin pair.
            Rule::PinConflict
            | Rule::InputNotDriven
            | Rule::Unannotated
            | Rule::MissingValue
            | Rule::MissingFootprint
            | Rule::DanglingWire
            | Rule::MissingJunction
            | Rule::SingleLabel => Severity::Warning,
            Rule::UnspecifiedPinType
            | Rule::UnreachableSheet
            | Rule::UnplacedUnit
            | Rule::StaleWaiver => Severity::Info,
        }
    }
}

/// One ERC finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: Rule,
    pub severity: Severity,
    pub message: String,
    pub sheet: Option<Id>,
    /// The offending objects; the first is the primary one. These are real model ids, so the UI
    /// can select them.
    pub objects: Vec<Id>,
    /// Where to look (sheet coordinates), when the finding has a location.
    pub at: Option<Point>,
    /// Stable identity used by waivers.
    pub fingerprint: String,
    /// Set when a documented waiver matches: the reason.
    pub waived: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErcCounts {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub waived: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErcReport {
    /// Sorted by severity, then rule, then fingerprint (deterministic).
    pub violations: Vec<Violation>,
}

impl ErcReport {
    /// Counts of *active* findings; waived ones are counted separately.
    pub fn counts(&self) -> ErcCounts {
        let mut c = ErcCounts::default();
        for v in &self.violations {
            if v.waived.is_some() {
                c.waived += 1;
                continue;
            }
            match v.severity {
                Severity::Error => c.errors += 1,
                Severity::Warning => c.warnings += 1,
                Severity::Info => c.infos += 1,
            }
        }
        c
    }

    /// No active errors. Warnings and infos do not fail a check.
    pub fn is_clean(&self) -> bool {
        self.counts().errors == 0
    }
}

// ---------------------------------------------------------------------------------------------
// Pin compatibility
// ---------------------------------------------------------------------------------------------

/// Severity of two pins of these types sharing a net, if they are incompatible.
///
/// The matrix is symmetric. Rationale (see `docs/erc.md`): two push-pull drivers fight; a
/// push-pull driver against a pull-only driver shorts when the latter conducts; drivers that can
/// be disabled (tri-state, bidirectional) may be fine, so only a warning; passives and inputs
/// never conflict. `NoConnect` pins are handled separately.
pub fn pin_conflict(a: PinType, b: PinType) -> Option<Severity> {
    use PinType::*;
    let pair = |x: PinType, y: PinType| (a == x && b == y) || (a == y && b == x);
    let drives_hard = |t: PinType| matches!(t, Output | PowerOutput);
    if drives_hard(a) && drives_hard(b) {
        return Some(Severity::Error);
    }
    if (drives_hard(a) && matches!(b, OpenCollector | OpenEmitter))
        || (drives_hard(b) && matches!(a, OpenCollector | OpenEmitter))
    {
        return Some(Severity::Error);
    }
    if (drives_hard(a) && matches!(b, TriState | Bidirectional))
        || (drives_hard(b) && matches!(a, TriState | Bidirectional))
    {
        return Some(Severity::Warning);
    }
    if pair(Output, PowerInput) {
        return Some(Severity::Warning);
    }
    if pair(OpenCollector, OpenEmitter) {
        return Some(Severity::Warning);
    }
    None
}

fn kind_name(t: PinType) -> &'static str {
    match t {
        PinType::Input => "input",
        PinType::Output => "output",
        PinType::Bidirectional => "bidirectional",
        PinType::TriState => "tri-state",
        PinType::Passive => "passive",
        PinType::PowerInput => "power input",
        PinType::PowerOutput => "power output",
        PinType::OpenCollector => "open collector",
        PinType::OpenEmitter => "open emitter",
        PinType::Unspecified => "unspecified",
        PinType::NoConnect => "no-connect",
    }
}

/// Can this pin drive an input?
fn can_drive(t: PinType) -> bool {
    !matches!(t, PinType::Input | PinType::PowerInput | PinType::NoConnect)
}

// ---------------------------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------------------------

/// A physical pin: all instances of (reference, number) across the units of one part.
struct PhysPin {
    reference: String,
    number: String,
    name: String,
    kind: PinType,
    /// First instance (the one findings point at).
    component: Id,
    sheet: Id,
    at: Point,
    /// Indices of the nets of every instance.
    nets: Vec<usize>,
}

impl PhysPin {
    fn key(&self) -> String {
        format!("{}.{}", self.reference, self.number)
    }
}

fn is_virtual(reference: &str) -> bool {
    reference.starts_with('#')
}

fn is_unannotated(reference: &str) -> bool {
    reference.ends_with('?')
}

fn unit_letter(u: u8) -> char {
    (b'A' + u.saturating_sub(1).min(25)) as char
}

fn component_at(c: &Component) -> Point {
    c.placement.position
}

/// Runs the Electrical Rules Check.
///
/// `conn` must have been built from `sch` (use the same revision). `waivers` are the project's
/// documented exceptions.
pub fn run_erc<'a>(
    sch: &Schematic,
    conn: &Connectivity,
    waivers: impl IntoIterator<Item = &'a Waiver>,
) -> ErcReport {
    let mut out: Vec<Violation> = Vec::new();
    let sheets: BTreeSet<Id> = conn.reachable_sheets().iter().copied().collect();

    // Components of reachable sheets, in id order.
    let mut parts: Vec<(Id, Id, &Component)> = Vec::new();
    for sid in conn.reachable_sheets() {
        if let Some(sheet) = sch.sheet(*sid) {
            for (id, o) in &sheet.objects {
                if let Object::Component(c) = o {
                    parts.push((*sid, *id, c));
                }
            }
        }
    }

    check_components(sch, &parts, &mut out);
    check_units(sch, &parts, &mut out);

    // Physical pins and their nets.
    let mut placed: HashMap<Id, Vec<PlacedPin>> = HashMap::new();
    for (_, id, c) in &parts {
        placed.insert(*id, sch.placed_pins(c));
    }
    let mut phys: BTreeMap<(String, String), PhysPin> = BTreeMap::new();
    for (ni, net) in conn.nets().iter().enumerate() {
        for pr in &net.pins {
            let Some(pin) = placed
                .get(&pr.component)
                .and_then(|v| v.iter().find(|p| p.number == pr.pin))
            else {
                continue;
            };
            let entry = phys
                .entry((pr.reference.clone(), pr.pin.clone()))
                .or_insert_with(|| PhysPin {
                    reference: pr.reference.clone(),
                    number: pr.pin.clone(),
                    name: pin.name.clone(),
                    kind: pin.kind,
                    component: pr.component,
                    sheet: pr.sheet,
                    at: pin.position,
                    nets: Vec::new(),
                });
            if !entry.nets.contains(&ni) {
                entry.nets.push(ni);
            }
        }
    }
    check_pins(conn, &phys, &mut out);
    check_nets(conn, &phys, &mut out);
    check_wires(conn, &mut out);
    check_structure(sch, conn, &sheets, &mut out);
    check_labels(sch, conn, &mut out);

    // Waivers.
    let waivers: BTreeMap<&str, &Waiver> = waivers
        .into_iter()
        .map(|w| (w.fingerprint.as_str(), w))
        .collect();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for v in &mut out {
        if let Some(w) = waivers.get(v.fingerprint.as_str()) {
            v.waived = Some(w.reason.clone());
            used.insert(w.fingerprint.as_str());
        }
    }
    for (fp, w) in &waivers {
        if !used.contains(fp) {
            let fingerprint = format!("stale_waiver|{fp}");
            out.push(Violation {
                rule: Rule::StaleWaiver,
                severity: Severity::Info,
                message: format!(
                    "The waiver for “{}” ({}) no longer matches any finding; you can remove it",
                    w.subject, w.rule
                ),
                sheet: None,
                objects: Vec::new(),
                at: None,
                waived: None,
                fingerprint,
            });
        }
    }

    let rule_index = |r: Rule| Rule::ALL.iter().position(|x| *x == r).unwrap_or(usize::MAX);
    out.sort_by(|a, b| {
        (a.severity, rule_index(a.rule), &a.fingerprint, &a.message).cmp(&(
            b.severity,
            rule_index(b.rule),
            &b.fingerprint,
            &b.message,
        ))
    });
    ErcReport { violations: out }
}

fn violation(
    rule: Rule,
    severity: Severity,
    message: String,
    sheet: Option<Id>,
    objects: Vec<Id>,
    at: Option<Point>,
    fingerprint: String,
) -> Violation {
    Violation {
        rule,
        severity,
        message,
        sheet,
        objects,
        at,
        fingerprint,
        waived: None,
    }
}

/// References, symbols and data of every part.
fn check_components(sch: &Schematic, parts: &[(Id, Id, &Component)], out: &mut Vec<Violation>) {
    let mut by_ref: BTreeMap<&str, Vec<(Id, Id, &Component)>> = BTreeMap::new();
    for &(sid, id, c) in parts {
        let at = Some(component_at(c));
        if c.reference.trim().is_empty() {
            out.push(violation(
                Rule::MissingReference,
                Severity::Error,
                "A component has no reference".into(),
                Some(sid),
                vec![id],
                at,
                format!("missing_reference|{}", id.0),
            ));
            continue;
        }
        let virt = is_virtual(&c.reference);
        if c.lib_id.is_empty() || sch.symbol(&c.lib_id).is_none() {
            out.push(violation(
                Rule::UnknownSymbol,
                Severity::Error,
                format!(
                    "{} uses the symbol “{}”, which is not in the project",
                    c.reference, c.lib_id
                ),
                Some(sid),
                vec![id],
                at,
                format!("unknown_symbol|{}", c.reference),
            ));
        }
        if virt {
            continue;
        }
        if is_unannotated(&c.reference) {
            out.push(violation(
                Rule::Unannotated,
                Severity::Warning,
                format!("{} has not been annotated", c.reference),
                Some(sid),
                vec![id],
                at,
                format!("unannotated|{}", id.0),
            ));
        } else {
            by_ref
                .entry(c.reference.as_str())
                .or_default()
                .push((sid, id, c));
        }
        if c.value.trim().is_empty() {
            out.push(violation(
                Rule::MissingValue,
                Severity::Warning,
                format!("{} has no value", c.reference),
                Some(sid),
                vec![id],
                at,
                format!("missing_value|{}|{}", c.reference, c.unit),
            ));
        }
        if c.footprint.trim().is_empty() {
            out.push(violation(
                Rule::MissingFootprint,
                Severity::Warning,
                format!("{} has no footprint assigned", c.reference),
                Some(sid),
                vec![id],
                at,
                format!("missing_footprint|{}|{}", c.reference, c.unit),
            ));
        }
    }
    // Duplicates: the same reference used by different parts. Several units of one symbol share
    // a reference by design; the same unit twice, or different symbols, do not.
    for (reference, group) in by_ref {
        if group.len() < 2 {
            continue;
        }
        let mut conflict: Vec<(Id, Id, &Component)> = Vec::new();
        for (i, a) in group.iter().enumerate() {
            let clash = group
                .iter()
                .enumerate()
                .any(|(j, b)| i != j && (a.2.lib_id != b.2.lib_id || a.2.unit == b.2.unit));
            if clash {
                conflict.push(*a);
            }
        }
        if conflict.is_empty() {
            continue;
        }
        let first = conflict[0];
        out.push(violation(
            Rule::DuplicateReference,
            Severity::Error,
            format!(
                "Reference {reference} is used by {} different parts",
                conflict.len()
            ),
            Some(first.0),
            conflict.iter().map(|c| c.1).collect(),
            Some(component_at(first.2)),
            format!("duplicate_reference|{reference}"),
        ));
    }
}

/// Units of one part seen so far, with the first instance (for locating findings).
struct PlacedUnits {
    units: BTreeSet<u8>,
    sheet: Id,
    object: Id,
    at: Point,
}

/// Multi-unit symbols whose units are not all placed.
fn check_units(sch: &Schematic, parts: &[(Id, Id, &Component)], out: &mut Vec<Violation>) {
    let mut placed: BTreeMap<(&str, &str), PlacedUnits> = BTreeMap::new();
    for &(sid, id, c) in parts {
        if is_virtual(&c.reference) || c.reference.is_empty() || is_unannotated(&c.reference) {
            continue;
        }
        let e = placed
            .entry((c.reference.as_str(), c.lib_id.as_str()))
            .or_insert_with(|| PlacedUnits {
                units: BTreeSet::new(),
                sheet: sid,
                object: id,
                at: component_at(c),
            });
        e.units.insert(c.unit);
    }
    for ((reference, lib), found) in placed {
        let Some(sym) = sch.symbol(lib) else { continue };
        for u in 1..=sym.unit_count {
            if !found.units.contains(&u) {
                out.push(violation(
                    Rule::UnplacedUnit,
                    Severity::Info,
                    format!(
                        "{reference}: unit {} of {} is not placed",
                        unit_letter(u),
                        sym.unit_count
                    ),
                    Some(found.sheet),
                    vec![found.object],
                    Some(found.at),
                    format!("unplaced_unit|{reference}|{u}"),
                ));
            }
        }
    }
}

/// Per-pin rules.
fn check_pins(
    conn: &Connectivity,
    phys: &BTreeMap<(String, String), PhysPin>,
    out: &mut Vec<Violation>,
) {
    for p in phys.values() {
        if p.kind == PinType::Unspecified {
            out.push(violation(
                Rule::UnspecifiedPinType,
                Severity::Info,
                format!(
                    "Pin {} ({}) of {} has an unspecified electrical type; ERC cannot check it",
                    p.number, p.name, p.reference
                ),
                Some(p.sheet),
                vec![p.component],
                Some(p.at),
                format!("unspecified_pin_type|{}", p.key()),
            ));
        }
        // "Connected" = shares a net with another physical pin, or the net leaves the pin through
        // a power symbol, a global label or a sheet pin.
        let connected = p.nets.iter().any(|ni| {
            let net = &conn.nets()[*ni];
            let others = net
                .pins
                .iter()
                .any(|q| !(q.reference == p.reference && q.pin == p.number));
            others || net.power || net.global || net.sheets.len() > 1
        });
        let marked = conn.has_no_connect(p.component, &p.number);
        if p.kind == PinType::NoConnect {
            if connected {
                out.push(violation(
                    Rule::NoConnectPinConnected,
                    Severity::Error,
                    format!(
                        "Pin {} ({}) of {} is declared no-connect but is connected",
                        p.number, p.name, p.reference
                    ),
                    Some(p.sheet),
                    vec![p.component],
                    Some(p.at),
                    format!("no_connect_pin_connected|{}", p.key()),
                ));
            }
            continue;
        }
        if !connected && !marked && !is_virtual(&p.reference) {
            out.push(violation(
                Rule::UnconnectedPin,
                Severity::Error,
                format!(
                    "Pin {} ({}, {}) of {} is not connected",
                    p.number,
                    p.name,
                    kind_name(p.kind),
                    p.reference
                ),
                Some(p.sheet),
                vec![p.component],
                Some(p.at),
                format!("unconnected_pin|{}", p.key()),
            ));
        }
    }
}

/// Per-net rules.
fn check_nets(
    conn: &Connectivity,
    phys: &BTreeMap<(String, String), PhysPin>,
    out: &mut Vec<Violation>,
) {
    const MAX_PAIRS_PER_NET: usize = 100;
    for (ni, net) in conn.nets().iter().enumerate() {
        let mut pins: Vec<&PhysPin> = Vec::new();
        let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
        for pr in &net.pins {
            if !seen.insert((pr.reference.as_str(), pr.pin.as_str())) {
                continue;
            }
            if let Some(p) = phys.get(&(pr.reference.clone(), pr.pin.clone())) {
                if p.nets.contains(&ni) {
                    pins.push(p);
                }
            }
        }
        if pins.is_empty() {
            continue;
        }
        let at_of = |p: &PhysPin| Some(p.at);

        // Conflicting pin pairs.
        let mut reported = 0;
        'pairs: for (i, a) in pins.iter().enumerate() {
            for b in &pins[i + 1..] {
                if a.kind == PinType::NoConnect || b.kind == PinType::NoConnect {
                    continue;
                }
                let Some(sev) = pin_conflict(a.kind, b.kind) else {
                    continue;
                };
                if reported == MAX_PAIRS_PER_NET {
                    break 'pairs;
                }
                reported += 1;
                let (ka, kb) = if a.key() <= b.key() {
                    (a.key(), b.key())
                } else {
                    (b.key(), a.key())
                };
                out.push(violation(
                    Rule::PinConflict,
                    sev,
                    format!(
                        "{} pin {} is connected to {} pin {} (net {})",
                        kind_name(a.kind),
                        a.key(),
                        kind_name(b.kind),
                        b.key(),
                        net.name
                    ),
                    Some(a.sheet),
                    vec![a.component, b.component],
                    at_of(a),
                    format!("pin_conflict|{ka}|{kb}"),
                ));
            }
        }

        // Inputs with nothing to drive them.
        let inputs: Vec<&&PhysPin> = pins.iter().filter(|p| p.kind == PinType::Input).collect();
        if !inputs.is_empty()
            && pins.len() > 1
            && !net.power
            && !pins.iter().any(|p| can_drive(p.kind))
        {
            let mut keys: Vec<String> = inputs.iter().map(|p| p.key()).collect();
            keys.sort();
            out.push(violation(
                Rule::InputNotDriven,
                Severity::Warning,
                format!(
                    "Net {} has input pins ({}) but nothing that can drive it",
                    net.name,
                    keys.join(", ")
                ),
                Some(inputs[0].sheet),
                inputs.iter().map(|p| p.component).collect(),
                at_of(inputs[0]),
                format!("input_not_driven|{}", keys.join(",")),
            ));
        }

        // Power inputs with no power output on the net.
        let power_in: Vec<&&PhysPin> = pins
            .iter()
            .filter(|p| p.kind == PinType::PowerInput)
            .collect();
        if !power_in.is_empty() && !pins.iter().any(|p| p.kind == PinType::PowerOutput) {
            let mut keys: Vec<String> = power_in.iter().map(|p| p.key()).collect();
            keys.sort();
            out.push(violation(
                Rule::PowerInputNotDriven,
                Severity::Error,
                format!(
                    "Net {} powers pin(s) {} but no power output drives it; add a PWR_FLAG, a power output pin, or waive this",
                    net.name,
                    keys.join(", ")
                ),
                Some(power_in[0].sheet),
                power_in.iter().map(|p| p.component).collect(),
                at_of(power_in[0]),
                format!("power_input_not_driven|{}", keys.join(",")),
            ));
        }
    }
}

/// Wire ends.
fn check_wires(conn: &Connectivity, out: &mut Vec<Violation>) {
    for d in conn.dangling_ends() {
        let at = format!("{},{}", d.at.x, d.at.y);
        match d.touches {
            Some(other) => out.push(violation(
                Rule::MissingJunction,
                Severity::Warning,
                "A wire ends on the middle of another wire without a junction, so they are not connected"
                    .into(),
                Some(d.sheet),
                vec![d.wire, other],
                Some(d.at),
                format!("missing_junction|{}|{at}", d.sheet.0),
            )),
            None => out.push(violation(
                Rule::DanglingWire,
                Severity::Warning,
                "A wire end is not connected to anything".into(),
                Some(d.sheet),
                vec![d.wire],
                Some(d.at),
                format!("dangling_wire|{}|{at}", d.sheet.0),
            )),
        }
    }
}

fn position_of(sch: &Schematic, id: Id) -> Option<Point> {
    let (_, o) = sch.object(id)?;
    Some(match o {
        Object::Wire(w) => w.segment.a,
        Object::Bus(b) => b.segment.a,
        Object::Junction(j) => j.position,
        Object::NoConnect(n) => n.position,
        Object::NetLabel(l) => l.position,
        Object::GlobalLabel(l) => l.position,
        Object::HierLabel(l) => l.position,
        Object::PowerSymbol(p) => p.placement.position,
        Object::Component(c) => c.placement.position,
        Object::Sheet(s) => s.rect.min(),
    })
}

/// Issues the connectivity builder found.
fn check_structure(
    sch: &Schematic,
    conn: &Connectivity,
    _sheets: &BTreeSet<Id>,
    out: &mut Vec<Violation>,
) {
    for issue in conn.issues() {
        match issue {
            Issue::NameConflict { net, names } => {
                let Some(n) = conn.net(*net) else { continue };
                let first = n.labels.first().copied();
                out.push(violation(
                    Rule::NetNameConflict,
                    Severity::Error,
                    format!("One net carries several names: {}", names.join(", ")),
                    first.and_then(|id| sch.sheet_of(id)),
                    n.labels.clone(),
                    first.and_then(|id| position_of(sch, id)),
                    format!("net_name_conflict|{}", names.join("|")),
                ));
            }
            Issue::SheetPinWithoutLabel { sheet, pin } => out.push(violation(
                Rule::HierarchyMismatch,
                Severity::Error,
                format!(
                    "Sheet pin “{pin}” has no hierarchical label of that name inside the sheet"
                ),
                Some(*sheet),
                Vec::new(),
                None,
                format!("hierarchy_mismatch|pin|{}|{pin}", sheet.0),
            )),
            Issue::HierLabelWithoutPin { sheet, label } => out.push(violation(
                Rule::HierarchyMismatch,
                Severity::Error,
                "A hierarchical label has no sheet pin of that name in the parent sheet".into(),
                Some(*sheet),
                vec![*label],
                position_of(sch, *label),
                format!("hierarchy_mismatch|label|{}", label.0),
            )),
            Issue::BadBusLabel { label, reason } => out.push(violation(
                Rule::BusLabel,
                Severity::Error,
                format!("Bus label: {reason}"),
                sch.sheet_of(*label),
                vec![*label],
                position_of(sch, *label),
                format!("bus_label|{}", label.0),
            )),
            Issue::UnreachableSheet { sheet } => out.push(violation(
                Rule::UnreachableSheet,
                Severity::Info,
                format!(
                    "Sheet “{}” is not placed anywhere in the hierarchy and was not checked",
                    sch.sheet(*sheet).map_or("?", |s| s.name.as_str())
                ),
                Some(*sheet),
                Vec::new(),
                None,
                format!("unreachable_sheet|{}", sheet.0),
            )),
        }
    }
}

/// Labels that name a net nobody else uses.
fn check_labels(sch: &Schematic, conn: &Connectivity, out: &mut Vec<Violation>) {
    let mut local: HashMap<(Id, &str), usize> = HashMap::new();
    let mut global: HashMap<&str, usize> = HashMap::new();
    for sid in conn.reachable_sheets() {
        let Some(sheet) = sch.sheet(*sid) else {
            continue;
        };
        for o in sheet.objects.values() {
            match o {
                Object::NetLabel(l) => *local.entry((*sid, l.name.as_str())).or_default() += 1,
                Object::GlobalLabel(l) => *global.entry(l.name.as_str()).or_default() += 1,
                _ => {}
            }
        }
    }
    let net_of_label: HashMap<Id, &Net> = conn
        .nets()
        .iter()
        .flat_map(|n| n.labels.iter().map(move |l| (*l, n)))
        .collect();
    for sid in conn.reachable_sheets() {
        let Some(sheet) = sch.sheet(*sid) else {
            continue;
        };
        for (id, o) in &sheet.objects {
            let (name, count, at) = match o {
                Object::NetLabel(l) => (
                    l.name.as_str(),
                    local.get(&(*sid, l.name.as_str())).copied().unwrap_or(0),
                    l.position,
                ),
                Object::GlobalLabel(l) => (
                    l.name.as_str(),
                    global.get(l.name.as_str()).copied().unwrap_or(0),
                    l.position,
                ),
                _ => continue,
            };
            if buturecad_schematic::bus::looks_like_bus(name) || count != 1 {
                continue;
            }
            let pins = net_of_label.get(id).map_or(0, |n| n.pins.len());
            if pins < 2 {
                out.push(violation(
                    Rule::SingleLabel,
                    Severity::Warning,
                    format!("Label “{name}” is used only once and connects fewer than two pins"),
                    Some(*sid),
                    vec![*id],
                    Some(at),
                    format!("single_label|{}|{name}", sid.0),
                ));
            }
        }
    }
}
