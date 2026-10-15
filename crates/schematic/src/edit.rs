//! Pure editing operations: each function turns "what the user wants" into a list of reversible
//! [`Change`]s. Nothing here touches the model, rendering or any UI state, so the same operations
//! serve the GUI, scripts and agents, and the Command Engine applies them.
//!
//! Connectivity rules honoured here (see `docs/schematic-connectivity.md`): wires that end on a
//! moved connection point **stretch** with it (orthogonal elbows are inserted so wires stay
//! axis-aligned), junctions that sit on a moved connection point travel with it, and a visual
//! crossing never creates a connection — junctions are added only where three or more
//! connection ends meet.

use crate::model::*;
use crate::schematic::{Change, Schematic, SchematicError};
use buturecad_geometry::{BoundingBox, Coord, Point, Rectangle, Segment, Vector};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum EditError {
    #[error("nothing is selected")]
    EmptySelection,
    #[error(transparent)]
    Schematic(#[from] SchematicError),
    #[error("{0}")]
    Invalid(String),
}

/// Sequential id source. The caller reserves ids from the engine afterwards (`next`).
#[derive(Debug, Clone, Copy)]
pub struct IdGen {
    pub next: u64,
}

impl IdGen {
    pub fn new(next: u64) -> Self {
        Self { next }
    }
    pub fn alloc(&mut self) -> Id {
        let id = Id(self.next);
        self.next += 1;
        id
    }
}

/// A geometric operation on schematic objects. All integer-exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Translate(Vector),
    /// 90° counter-clockwise about `pivot`.
    Rotate {
        pivot: Point,
    },
    /// Left-right flip about the vertical line `x = axis`.
    MirrorX {
        axis: Coord,
    },
    /// Top-bottom flip about the horizontal line `y = axis`.
    MirrorY {
        axis: Coord,
    },
}

impl Op {
    pub fn point(self, p: Point) -> Point {
        match self {
            Op::Translate(v) => p + v,
            Op::Rotate { pivot } => p.rotate_quarter_turns(pivot, 1),
            Op::MirrorX { axis } => Point::new(axis.saturating_mul(2).saturating_sub(p.x), p.y),
            Op::MirrorY { axis } => Point::new(p.x, axis.saturating_mul(2).saturating_sub(p.y)),
        }
    }

    pub fn direction(self, d: Direction) -> Direction {
        match (self, d) {
            (Op::Translate(_), d) => d,
            (Op::Rotate { .. }, Direction::Right) => Direction::Up,
            (Op::Rotate { .. }, Direction::Up) => Direction::Left,
            (Op::Rotate { .. }, Direction::Left) => Direction::Down,
            (Op::Rotate { .. }, Direction::Down) => Direction::Right,
            (Op::MirrorX { .. }, Direction::Left) => Direction::Right,
            (Op::MirrorX { .. }, Direction::Right) => Direction::Left,
            (Op::MirrorY { .. }, Direction::Up) => Direction::Down,
            (Op::MirrorY { .. }, Direction::Down) => Direction::Up,
            (_, d) => d,
        }
    }

    /// New placement such that `new.apply(p) == op.point(old.apply(p))` for every local `p`.
    pub fn placement(self, pl: Placement) -> Placement {
        let k = pl.rotation % 4;
        let position = self.point(pl.position);
        match self {
            Op::Translate(_) => Placement { position, ..pl },
            Op::Rotate { .. } => Placement {
                position,
                rotation: (k + 1) % 4,
                mirror: pl.mirror,
            },
            // flipX∘R_k∘M = R_{-k}∘M' with M' = !M ; flipY = R180∘flipX adds a half turn.
            Op::MirrorX { .. } => Placement {
                position,
                rotation: (4 - k) % 4,
                mirror: !pl.mirror,
            },
            Op::MirrorY { .. } => Placement {
                position,
                rotation: (6 - k) % 4,
                mirror: !pl.mirror,
            },
        }
    }

    fn segment(self, s: Segment) -> Segment {
        Segment::new(self.point(s.a), self.point(s.b))
    }

    fn rect(self, r: Rectangle) -> Rectangle {
        let (a, b) = (self.point(r.min()), self.point(r.max()));
        // Quarter turns and flips keep rectangles axis-aligned and non-degenerate.
        Rectangle::new(a, b).unwrap_or(r)
    }
}

/// The object after applying `op`.
pub fn transform_object(obj: &Object, op: Op) -> Object {
    let mut out = obj.clone();
    match &mut out {
        Object::Component(c) => c.placement = op.placement(c.placement),
        Object::PowerSymbol(p) => p.placement = op.placement(p.placement),
        Object::Wire(w) => w.segment = op.segment(w.segment),
        Object::Bus(b) => b.segment = op.segment(b.segment),
        Object::Junction(j) => j.position = op.point(j.position),
        Object::NoConnect(n) => n.position = op.point(n.position),
        Object::NetLabel(l) => {
            l.position = op.point(l.position);
            l.orientation = op.direction(l.orientation);
        }
        Object::GlobalLabel(l) => {
            l.position = op.point(l.position);
            l.orientation = op.direction(l.orientation);
        }
        Object::HierLabel(l) => {
            l.position = op.point(l.position);
            l.orientation = op.direction(l.orientation);
        }
        Object::Sheet(s) => {
            s.rect = op.rect(s.rect);
            for p in &mut s.pins {
                p.position = op.point(p.position);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Connection points
// ---------------------------------------------------------------------------------------------

/// Points where other objects can attach to `obj`.
pub fn connection_points(sch: &Schematic, obj: &Object) -> Vec<Point> {
    match obj {
        Object::Component(c) => sch.placed_pins(c).into_iter().map(|p| p.position).collect(),
        Object::PowerSymbol(p) => sch.power_pin_position(p).into_iter().collect(),
        Object::Wire(w) => vec![w.segment.a, w.segment.b],
        Object::Bus(b) => vec![b.segment.a, b.segment.b],
        Object::Junction(j) => vec![j.position],
        Object::NoConnect(n) => vec![n.position],
        Object::NetLabel(l) => vec![l.position],
        Object::GlobalLabel(l) => vec![l.position],
        Object::HierLabel(l) => vec![l.position],
        Object::Sheet(s) => s.pins.iter().map(|p| p.position).collect(),
    }
}

/// All objects on the sheet containing `id`, as (id, object).
fn sheet_objects(sch: &Schematic, sheet: Id) -> Vec<(Id, &Object)> {
    sch.sheet(sheet)
        .map(|s| s.objects.iter().map(|(i, o)| (*i, o)).collect())
        .unwrap_or_default()
}

fn single_sheet(sch: &Schematic, ids: &BTreeSet<Id>) -> Result<Id, EditError> {
    let mut sheet = None;
    for id in ids {
        let s = sch.sheet_of(*id).ok_or(SchematicError::NoSuchObject(*id))?;
        if sheet.is_some_and(|x| x != s) {
            return Err(EditError::Invalid(
                "the selection spans several sheets".into(),
            ));
        }
        sheet = Some(s);
    }
    sheet.ok_or(EditError::EmptySelection)
}

/// Bounding box of the selected objects.
pub fn selection_bounds(sch: &Schematic, ids: &BTreeSet<Id>) -> Option<BoundingBox> {
    let mut boxes = ids
        .iter()
        .filter_map(|id| sch.object(*id))
        .map(|(_, o)| sch.object_bounds(o));
    let first = boxes.next()?;
    Some(boxes.fold(first, |a, b| a.union(&b)))
}

/// Pivot for rotating or mirroring a selection: a lone component or label turns in place; a group
/// turns about the centre of its bounds, snapped to `grid` so everything stays on the grid.
pub fn pivot_for(sch: &Schematic, ids: &BTreeSet<Id>, grid: Coord) -> Option<Point> {
    if ids.len() == 1 {
        let (_, obj) = sch.object(*ids.iter().next()?)?;
        match obj {
            Object::Component(c) => return Some(c.placement.position),
            Object::PowerSymbol(p) => return Some(p.placement.position),
            Object::NetLabel(l) => return Some(l.position),
            Object::GlobalLabel(l) => return Some(l.position),
            Object::HierLabel(l) => return Some(l.position),
            Object::Junction(j) => return Some(j.position),
            _ => {}
        }
    }
    let c = selection_bounds(sch, ids)?.center();
    let g = grid.max(1);
    let snap = |v: Coord| (v + g / 2).div_euclid(g) * g;
    Some(Point::new(snap(c.x), snap(c.y)))
}

// ---------------------------------------------------------------------------------------------
// Move / rotate / mirror with connectivity-preserving stretch
// ---------------------------------------------------------------------------------------------

/// A wire that has to follow a moved connection point.
#[derive(Debug, Clone)]
struct Stretch {
    sheet: Id,
    id: Id,
    original: Object,
    fixed: Point,
    old_end: Point,
    new_end: Point,
    /// Where the new route leaves the old wire: the interior contact (junction, pin, label…)
    /// nearest the moving end, or `fixed` when the wire has no interior contacts.
    anchor: Point,
}

/// Grid the Z-shaped re-routes are snapped to (50 mil, the schematic grid).
const ROUTE_GRID: Coord = 1_270_000;

/// How a stretched wire is re-routed. All orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Horizontal, then vertical.
    Hv,
    /// Vertical, then horizontal.
    Vh,
    /// Horizontal, vertical, horizontal: the vertical leg half way across.
    HvhMid,
    /// Same, with the vertical leg 3 grid steps to the right of the midpoint.
    HvhRight,
    /// … and to the left.
    HvhLeft,
    /// Vertical, horizontal, vertical: the horizontal leg half way across.
    VhvMid,
    /// Same, 3 grid steps above the midpoint.
    VhvUp,
    /// … and below.
    VhvDown,
}

/// Number of routing styles tried per stretched wire.
const STYLES: usize = 8;
/// Detour used by the offset styles, in nm (3 grid steps): clears a part's pins and body.
const DETOUR: Coord = 3 * ROUTE_GRID;

impl Stretch {
    /// Styles in order of preference: the wire keeps running the way it ran (or, leaving a
    /// mid-wire contact, turns away from it), L-shapes before Z-shapes before detours.
    fn preference(&self) -> [Style; STYLES] {
        let horizontal = self.fixed.y == self.old_end.y && self.fixed.x != self.old_end.x;
        let leaving_end = self.anchor == self.fixed;
        if horizontal == leaving_end {
            [
                Style::Hv,
                Style::Vh,
                Style::HvhMid,
                Style::VhvMid,
                Style::HvhRight,
                Style::HvhLeft,
                Style::VhvUp,
                Style::VhvDown,
            ]
        } else {
            [
                Style::Vh,
                Style::Hv,
                Style::VhvMid,
                Style::HvhMid,
                Style::VhvUp,
                Style::VhvDown,
                Style::HvhRight,
                Style::HvhLeft,
            ]
        }
    }

    /// Legs replacing the wire. `choice` indexes [`Self::preference`].
    fn legs(&self, choice: usize) -> Vec<Segment> {
        let mut pts: Vec<Point> = vec![self.fixed];
        if self.anchor != self.fixed {
            pts.push(self.anchor);
        }
        let (from, to) = (self.anchor, self.new_end);
        let diagonal = self.fixed.x != self.old_end.x && self.fixed.y != self.old_end.y;
        let mid = |a: Coord, b: Coord| {
            let m = a + (b - a) / 2;
            let snapped = (m + ROUTE_GRID / 2).div_euclid(ROUTE_GRID) * ROUTE_GRID;
            if snapped != a && snapped != b && (snapped - a).signum() == (b - a).signum() {
                snapped
            } else {
                m
            }
        };
        if !diagonal {
            let hvh = |x: Coord| [Point::new(x, from.y), Point::new(x, to.y)];
            let vhv = |y: Coord| [Point::new(from.x, y), Point::new(to.x, y)];
            match self.preference()[choice % STYLES] {
                Style::Hv => pts.push(Point::new(to.x, from.y)),
                Style::Vh => pts.push(Point::new(from.x, to.y)),
                Style::HvhMid => pts.extend(hvh(mid(from.x, to.x))),
                Style::HvhRight => pts.extend(hvh(mid(from.x, to.x) + DETOUR)),
                Style::HvhLeft => pts.extend(hvh(mid(from.x, to.x) - DETOUR)),
                Style::VhvMid => pts.extend(vhv(mid(from.y, to.y))),
                Style::VhvUp => pts.extend(vhv(mid(from.y, to.y) + DETOUR)),
                Style::VhvDown => pts.extend(vhv(mid(from.y, to.y) - DETOUR)),
            }
        }
        pts.push(to);
        pts.dedup();
        pts.windows(2).map(|w| Segment::new(w[0], w[1])).collect()
    }
}

/// The changes for a transform, before choosing elbow orientations.
struct Plan {
    base: Vec<Change>,
    stretches: Vec<Stretch>,
}

fn plan(sch: &Schematic, ids: &BTreeSet<Id>, op: Op, stretch: bool) -> Result<Plan, EditError> {
    let sheet = single_sheet(sch, ids)?;
    let mut moved: BTreeSet<Id> = ids.clone();

    // Connection points of the moved objects, before the edit.
    let mut points: BTreeSet<Point> = BTreeSet::new();
    for id in &moved {
        if let Some((_, o)) = sch.object(*id) {
            points.extend(connection_points(sch, o));
        }
    }
    if stretch {
        // Junctions, power symbols, labels and no-connect marks sitting on a moved connection
        // point belong to that connection and travel with it (they cannot stretch like wires).
        for (id, o) in sheet_objects(sch, sheet) {
            let attached = matches!(
                o,
                Object::Junction(_)
                    | Object::PowerSymbol(_)
                    | Object::NetLabel(_)
                    | Object::GlobalLabel(_)
                    | Object::HierLabel(_)
                    | Object::NoConnect(_)
            );
            if attached && connection_points(sch, o).iter().any(|p| points.contains(p)) {
                moved.insert(id);
            }
        }
        // Wires with both ends on moved points move as a whole.
        for (id, o) in sheet_objects(sch, sheet) {
            if let Object::Wire(w) = o {
                if points.contains(&w.segment.a) && points.contains(&w.segment.b) {
                    moved.insert(id);
                }
            }
        }
    }

    let base: Vec<Change> = moved
        .iter()
        .filter_map(|id| {
            sch.object(*id)
                .map(|(s, obj)| Change::modify(s, *id, obj.clone(), transform_object(obj, op)))
        })
        .collect();

    let mut stretches = Vec::new();
    if stretch {
        // Attachment points of everything that stays put: contacts a stretched wire may carry.
        let contacts: Vec<Point> = sheet_objects(sch, sheet)
            .into_iter()
            .filter(|(id, _)| !moved.contains(id))
            .flat_map(|(_, o)| connection_points(sch, o))
            .collect();
        for (id, o) in sheet_objects(sch, sheet) {
            if moved.contains(&id) {
                continue;
            }
            let Object::Wire(w) = o else { continue };
            let (fixed, old_end) =
                match (points.contains(&w.segment.a), points.contains(&w.segment.b)) {
                    (true, false) => (w.segment.b, w.segment.a),
                    (false, true) => (w.segment.a, w.segment.b),
                    _ => continue,
                };
            let along = Segment::new(fixed, old_end);
            let anchor = contacts
                .iter()
                .filter(|p| **p != fixed && **p != old_end && along.contains_point(**p))
                .max_by_key(|p| (**p - fixed).length_sq())
                .copied()
                .unwrap_or(fixed);
            stretches.push(Stretch {
                sheet,
                id,
                original: o.clone(),
                fixed,
                old_end,
                new_end: op.point(old_end),
                anchor,
            });
        }
    }
    Ok(Plan { base, stretches })
}

/// `choices[i]` is the style choice (index into the preference list) of stretched wire `i`;
/// missing entries mean the preferred style.
fn realize(plan: &Plan, choices: &[usize], gen: &mut IdGen) -> Vec<Change> {
    let mut changes = plan.base.clone();
    for (i, st) in plan.stretches.iter().enumerate() {
        let choice = choices.get(i).copied().unwrap_or(0);
        let mut legs = st.legs(choice).into_iter();
        match legs.next() {
            Some(first) => changes.push(Change::modify(
                st.sheet,
                st.id,
                st.original.clone(),
                Object::Wire(Wire { segment: first }),
            )),
            None => changes.push(Change::delete(st.sheet, st.id, st.original.clone())),
        }
        for seg in legs {
            changes.push(Change::create(
                st.sheet,
                gen.alloc(),
                Object::Wire(Wire { segment: seg }),
            ));
        }
    }
    changes
}

/// Applies `op` to the selection (fast, unverified: used for live previews). With `stretch`,
/// wires that are **not** selected but end on a moved connection point follow it, and junctions
/// on moved points travel along. See [`transform_selection_checked`] for the verified version.
pub fn transform_selection(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    op: Op,
    stretch: bool,
    gen: &mut IdGen,
) -> Result<Vec<Change>, EditError> {
    Ok(realize(&plan(sch, ids, op, stretch)?, &[], gen))
}

/// Which pins share a net (as a set of sets of `REF.pin`), plus named nets. Independent of net
/// names and ids, so it can be compared before and after an edit.
pub type Topology = BTreeSet<BTreeSet<String>>;

pub fn topology_of(sch: &Schematic) -> Topology {
    crate::Connectivity::build(sch)
        .nets()
        .iter()
        .filter(|n| n.pins.len() > 1 || n.named)
        .map(|n| {
            let mut set: BTreeSet<String> = n
                .pins
                .iter()
                .map(|p| format!("{}.{}", p.reference, p.pin))
                .collect();
            if n.named {
                set.insert(format!("#{}", n.name));
            }
            set
        })
        .collect()
}

/// Result of a verified transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    pub changes: Vec<Change>,
    /// `true` when every net keeps exactly the same pins and names after the edit. `false` means
    /// the edit merged or split nets (for example a re-routed wire crossed another net); the
    /// changes are still returned so the caller can warn the user, who can undo.
    pub connectivity_preserved: bool,
}

/// Like [`transform_selection`], but tries several routing styles (L shapes, Z shapes, detours) for
/// the stretched wires and returns the first combination that leaves the circuit's topology
/// unchanged.
pub fn transform_selection_checked(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    op: Op,
    stretch: bool,
    gen: &mut IdGen,
) -> Result<Edited, EditError> {
    let plan = plan(sch, ids, op, stretch)?;
    let before = topology_of(sch);
    let n = plan.stretches.len();
    // Number of topology differences a style combination causes (0 = connectivity preserved).
    let cost = |choices: &[usize]| -> usize {
        let mut trial = sch.clone();
        let mut probe = *gen;
        if trial.apply(&realize(&plan, choices, &mut probe)).is_err() {
            return usize::MAX;
        }
        topology_of(&trial).symmetric_difference(&before).count()
    };
    let mut best: Vec<usize> = vec![0; n];
    let mut best_cost = cost(&best);
    if best_cost > 0 && n > 0 {
        if n <= 3 {
            // Small: try every combination, preferred styles first.
            let total = STYLES.pow(n as u32);
            for code in 1..total {
                let choices: Vec<usize> = (0..n)
                    .map(|i| code / STYLES.pow(i as u32) % STYLES)
                    .collect();
                let c = cost(&choices);
                if c < best_cost {
                    (best, best_cost) = (choices, c);
                    if best_cost == 0 {
                        break;
                    }
                }
            }
        } else {
            // Many wires: improve one at a time, keeping whatever lowers the damage.
            for _round in 0..2 {
                for i in 0..n {
                    for style in 0..STYLES {
                        let mut choices = best.clone();
                        choices[i] = style;
                        let c = cost(&choices);
                        if c < best_cost {
                            (best, best_cost) = (choices, c);
                        }
                    }
                    if best_cost == 0 {
                        break;
                    }
                }
                if best_cost == 0 {
                    break;
                }
            }
        }
    }
    Ok(Edited {
        changes: realize(&plan, &best, gen),
        connectivity_preserved: best_cost == 0,
    })
}

pub fn move_objects(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    delta: Vector,
    stretch: bool,
    gen: &mut IdGen,
) -> Result<Edited, EditError> {
    transform_selection_checked(sch, ids, Op::Translate(delta), stretch, gen)
}

pub fn rotate_objects(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    grid: Coord,
    gen: &mut IdGen,
) -> Result<Edited, EditError> {
    let pivot = pivot_for(sch, ids, grid).ok_or(EditError::EmptySelection)?;
    transform_selection_checked(sch, ids, Op::Rotate { pivot }, true, gen)
}

pub fn mirror_objects(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    horizontal: bool,
    grid: Coord,
    gen: &mut IdGen,
) -> Result<Edited, EditError> {
    let pivot = pivot_for(sch, ids, grid).ok_or(EditError::EmptySelection)?;
    let op = if horizontal {
        Op::MirrorX { axis: pivot.x }
    } else {
        Op::MirrorY { axis: pivot.y }
    };
    transform_selection_checked(sch, ids, op, true, gen)
}

// ---------------------------------------------------------------------------------------------
// Junction bookkeeping
// ---------------------------------------------------------------------------------------------

/// How many connection "ends" meet at `p`: a wire end counts 1, a wire passing through counts 2,
/// a component/power pin counts 1. Three or more need an explicit junction.
fn degree_at(sch: &Schematic, sheet: Id, p: Point, skip: &BTreeSet<Id>, extra: &[Segment]) -> u32 {
    let weight = |s: &Segment| -> u32 {
        if s.a == p || s.b == p {
            1
        } else if s.contains_point(p) {
            2
        } else {
            0
        }
    };
    let mut d = 0;
    for (id, o) in sheet_objects(sch, sheet) {
        if skip.contains(&id) {
            continue;
        }
        match o {
            Object::Wire(w) => d += weight(&w.segment),
            Object::Component(_) | Object::PowerSymbol(_) => {
                d += connection_points(sch, o)
                    .iter()
                    .filter(|q| **q == p)
                    .count() as u32;
            }
            _ => {}
        }
    }
    d + extra.iter().map(weight).sum::<u32>()
}

fn junction_at(sch: &Schematic, sheet: Id, p: Point, skip: &BTreeSet<Id>) -> Option<Id> {
    sheet_objects(sch, sheet)
        .into_iter()
        .find_map(|(id, o)| match o {
            Object::Junction(j) if j.position == p && !skip.contains(&id) => Some(id),
            _ => None,
        })
}

// ---------------------------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------------------------

/// Deletes the selection. Junctions that no longer join three or more ends are removed too.
pub fn delete_objects(sch: &Schematic, ids: &BTreeSet<Id>) -> Result<Vec<Change>, EditError> {
    let sheet = single_sheet(sch, ids)?;
    let mut doomed = ids.clone();
    // Orphan junctions: those at an end of a deleted wire whose remaining degree drops below 3.
    let mut candidates: BTreeSet<Point> = BTreeSet::new();
    for id in ids {
        if let Some((_, Object::Wire(w))) = sch.object(*id) {
            candidates.extend([w.segment.a, w.segment.b]);
        }
    }
    for p in candidates {
        if let Some(j) = junction_at(sch, sheet, p, &doomed) {
            if degree_at(sch, sheet, p, &doomed, &[]) < 3 {
                doomed.insert(j);
            }
        }
    }
    Ok(doomed
        .into_iter()
        .filter_map(|id| {
            sch.object(id)
                .map(|(s, o)| Change::delete(s, id, o.clone()))
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Clipboard, duplicate, paste
// ---------------------------------------------------------------------------------------------

/// Copied objects plus the library symbols they need, so a paste works in any project.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipboard {
    pub objects: Vec<Object>,
    pub symbols: Vec<Symbol>,
    /// Reference point: the top-left corner of the copied bounds, on the grid.
    pub anchor: Point,
}

pub fn copy_objects(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    grid: Coord,
) -> Result<Clipboard, EditError> {
    single_sheet(sch, ids)?;
    let objects: Vec<Object> = ids
        .iter()
        .filter_map(|id| sch.object(*id).map(|(_, o)| o.clone()))
        .collect();
    let mut libs: BTreeSet<&str> = BTreeSet::new();
    for o in &objects {
        match o {
            Object::Component(c) if !c.lib_id.is_empty() => libs.insert(&c.lib_id),
            Object::PowerSymbol(p) => libs.insert(&p.lib_id),
            _ => false,
        };
    }
    let symbols = libs
        .into_iter()
        .filter_map(|l| sch.symbol(l).cloned())
        .collect();
    let b = selection_bounds(sch, ids).ok_or(EditError::EmptySelection)?;
    let g = grid.max(1);
    let anchor = Point::new(
        b.min.x.div_euclid(g) * g,
        b.max.y.div_euclid(g) * g + if b.max.y.rem_euclid(g) == 0 { 0 } else { g },
    );
    Ok(Clipboard {
        objects,
        symbols,
        anchor,
    })
}

/// Hands out fresh references (`R1`, `R2` …) without clashing with the schematic or with each other.
struct References<'a> {
    sch: &'a Schematic,
    taken: BTreeSet<String>,
}

impl<'a> References<'a> {
    fn new(sch: &'a Schematic) -> Self {
        let taken = sch
            .sheets()
            .values()
            .flat_map(|s| s.objects.values())
            .filter_map(|o| {
                if let Object::Component(c) = o {
                    Some(c.reference.clone())
                } else {
                    None
                }
            })
            .collect();
        Self { sch, taken }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let prefix = if prefix.is_empty() { "U" } else { prefix };
        let n = (1..)
            .find(|n| !self.taken.contains(&format!("{prefix}{n}")))
            .unwrap_or(1);
        let r = format!("{prefix}{n}");
        self.taken.insert(r.clone());
        r
    }

    fn prefix_of(&self, c: &Component) -> String {
        self.sch
            .symbol(&c.lib_id)
            .map(|s| s.reference_prefix.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| {
                c.reference
                    .trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '?')
                    .to_owned()
            })
    }
}

/// Creates copies of `objects` translated by `delta`; components get new references.
fn instantiate(
    sch: &Schematic,
    sheet: Id,
    objects: &[Object],
    delta: Vector,
    gen: &mut IdGen,
) -> (Vec<Change>, Vec<Id>) {
    let mut refs = References::new(sch);
    let mut changes = Vec::new();
    let mut ids = Vec::new();
    for o in objects {
        let mut copy = transform_object(o, Op::Translate(delta));
        if let Object::Component(c) = &mut copy {
            let prefix = refs.prefix_of(c);
            c.reference = refs.fresh(&prefix);
        }
        let id = gen.alloc();
        ids.push(id);
        changes.push(Change::create(sheet, id, copy));
    }
    (changes, ids)
}

/// Duplicates the selection, offset by `offset`. Returns the changes and the new ids.
pub fn duplicate_objects(
    sch: &Schematic,
    ids: &BTreeSet<Id>,
    offset: Vector,
    gen: &mut IdGen,
) -> Result<(Vec<Change>, Vec<Id>), EditError> {
    let sheet = single_sheet(sch, ids)?;
    let objects: Vec<Object> = ids
        .iter()
        .filter_map(|id| sch.object(*id).map(|(_, o)| o.clone()))
        .collect();
    Ok(instantiate(sch, sheet, &objects, offset, gen))
}

/// Pastes a clipboard with its anchor at `at`. Symbols the schematic lacks must be added first
/// (the caller does that through `AddSymbolsCommand`).
pub fn paste_objects(
    sch: &Schematic,
    clip: &Clipboard,
    sheet: Id,
    at: Point,
    gen: &mut IdGen,
) -> Result<(Vec<Change>, Vec<Id>), EditError> {
    if clip.objects.is_empty() {
        return Err(EditError::EmptySelection);
    }
    Ok(instantiate(
        sch,
        sheet,
        &clip.objects,
        at - clip.anchor,
        gen,
    ))
}

// ---------------------------------------------------------------------------------------------
// Creation helpers
// ---------------------------------------------------------------------------------------------

/// A new component for `symbol` with the next free reference.
pub fn new_component(sch: &Schematic, symbol: &Symbol, placement: Placement) -> Component {
    let prefix = if symbol.reference_prefix.is_empty() {
        "U"
    } else {
        symbol.reference_prefix.as_str()
    };
    // The part carries what the symbol knows about it: free-form properties first, then the
    // standard fields (so a symbol's own `MPN` property cannot be shadowed by an empty field).
    let mut properties = symbol.meta.properties.clone();
    for (key, v) in [
        ("MPN", &symbol.meta.mpn),
        ("Manufacturer", &symbol.meta.manufacturer),
        ("Datasheet", &symbol.meta.datasheet),
    ] {
        if !v.is_empty() {
            properties.insert(key.to_owned(), v.clone());
        }
    }
    Component {
        lib_id: symbol.name.clone(),
        reference: sch.next_reference(prefix),
        value: symbol.default_value.clone(),
        footprint: symbol.meta.default_footprint.clone(),
        properties,
        placement,
        unit: 1,
        representation: REPR_NORMAL,
    }
}

/// What a library component adds to a part placed from it (stage 51: placing a component brings
/// its footprint and manufacturer data, not only the symbol).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartData {
    pub footprint: String,
    pub mpn: String,
    pub manufacturer: String,
    pub datasheet: String,
}

impl PartData {
    /// Copies the non-empty fields onto `c`.
    pub fn apply(&self, c: &mut Component) {
        if !self.footprint.is_empty() {
            c.footprint = self.footprint.clone();
        }
        for (key, v) in [
            ("MPN", &self.mpn),
            ("Manufacturer", &self.manufacturer),
            ("Datasheet", &self.datasheet),
        ] {
            if !v.is_empty() {
                c.properties.insert(key.to_owned(), v.clone());
            }
        }
    }
}

/// A new component for one `unit` of a multi-unit `symbol`. Unit 1 always gets a fresh
/// reference. Any other unit joins the most recently placed part of the same symbol that does
/// not have that unit yet (so `U1A`, `U1B` share `U1`); with none, it gets a fresh reference.
pub fn new_component_unit(
    sch: &Schematic,
    symbol: &Symbol,
    placement: Placement,
    unit: u8,
) -> Component {
    let mut c = new_component(sch, symbol, placement);
    c.unit = unit.clamp(1, symbol.unit_count.max(1));
    if c.unit == 1 {
        return c;
    }
    let parts: Vec<(Id, &Component)> = sch
        .sheets()
        .values()
        .flat_map(|s| s.objects.iter())
        .filter_map(|(id, o)| match o {
            Object::Component(p) if p.lib_id == symbol.name => Some((*id, p)),
            _ => None,
        })
        .collect();
    let host = parts
        .iter()
        .filter(|(_, p)| {
            !parts
                .iter()
                .any(|(_, q)| q.reference == p.reference && q.unit == c.unit)
        })
        .max_by_key(|(id, _)| *id);
    if let Some((_, p)) = host {
        c.reference = p.reference.clone();
        c.value = p.value.clone();
        c.footprint = p.footprint.clone();
    }
    c
}

pub fn new_power_symbol(symbol: &Symbol, placement: Placement) -> PowerSymbol {
    PowerSymbol {
        lib_id: symbol.name.clone(),
        net_name: symbol.default_value.clone(),
        placement,
    }
}

/// Adds a junction at `p` unless one exists.
pub fn add_junction(sch: &Schematic, sheet: Id, p: Point, gen: &mut IdGen) -> Vec<Change> {
    if junction_at(sch, sheet, p, &BTreeSet::new()).is_some() {
        return Vec::new();
    }
    vec![Change::create(
        sheet,
        gen.alloc(),
        Object::Junction(Junction { position: p }),
    )]
}

/// Turns a path of points into wire (or bus) segments: zero-length steps are dropped and
/// collinear consecutive steps are merged. Wires also get automatic junctions where three or more
/// connection ends meet; crossings alone never do.
pub fn draw_path(
    sch: &Schematic,
    sheet: Id,
    points: &[Point],
    bus: bool,
    gen: &mut IdGen,
) -> Result<Vec<Change>, EditError> {
    let mut pts: Vec<Point> = Vec::new();
    for p in points {
        if pts.last() != Some(p) {
            pts.push(*p);
        }
    }
    // Merge collinear runs.
    let mut merged: Vec<Point> = Vec::new();
    for p in pts {
        while merged.len() >= 2 {
            let (a, b) = (merged[merged.len() - 2], merged[merged.len() - 1]);
            if (b - a).cross(p - b) == 0 && (b - a).dot(p - b) > 0 {
                merged.pop();
            } else {
                break;
            }
        }
        merged.push(p);
    }
    if merged.len() < 2 {
        return Err(EditError::Invalid(
            "a wire needs two different points".into(),
        ));
    }
    let segments: Vec<Segment> = merged
        .windows(2)
        .map(|w| Segment::new(w[0], w[1]))
        .collect();
    let mut changes: Vec<Change> = segments
        .iter()
        .map(|s| {
            let id = gen.alloc();
            let obj = if bus {
                Object::Bus(Bus { segment: *s })
            } else {
                Object::Wire(Wire { segment: *s })
            };
            Change::create(sheet, id, obj)
        })
        .collect();

    if !bus {
        // Candidate junction points: ends of the new wires, and ends of existing wires that now
        // lie on the interior of a new one.
        let mut candidates: BTreeSet<Point> = BTreeSet::new();
        for s in &segments {
            candidates.extend([s.a, s.b]);
        }
        for (_, o) in sheet_objects(sch, sheet) {
            if let Object::Wire(w) = o {
                for p in [w.segment.a, w.segment.b] {
                    if segments
                        .iter()
                        .any(|s| s.a != p && s.b != p && s.contains_point(p))
                    {
                        candidates.insert(p);
                    }
                }
            }
        }
        for p in candidates {
            if junction_at(sch, sheet, p, &BTreeSet::new()).is_some() {
                continue;
            }
            if degree_at(sch, sheet, p, &BTreeSet::new(), &segments) >= 3 {
                changes.push(Change::create(
                    sheet,
                    gen.alloc(),
                    Object::Junction(Junction { position: p }),
                ));
            }
        }
    }
    Ok(changes)
}

/// Orthogonal two-leg path from `from` to `to`: horizontal first, or vertical first.
pub fn elbow(from: Point, to: Point, horizontal_first: bool) -> Vec<Point> {
    if from.x == to.x || from.y == to.y {
        return vec![from, to];
    }
    let corner = if horizontal_first {
        Point::new(to.x, from.y)
    } else {
        Point::new(from.x, to.y)
    };
    vec![from, corner, to]
}

/// Increments the trailing number of a label (`D3` → `D4`), for repeated label placement.
pub fn next_label_name(name: &str) -> String {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if digits.is_empty() {
        return name.to_owned();
    }
    let stem = &name[..name.len() - digits.len()];
    match digits.parse::<u64>() {
        Ok(n) => format!("{stem}{:0width$}", n + 1, width = digits.len()),
        Err(_) => name.to_owned(),
    }
}

/// Sets a property of an object; returns the single change. Supported keys: `reference`, `value`,
/// `footprint` (components), `name` (labels, sheets' names are not editable here) and `net_name`
/// (power symbols).
pub fn set_property(sch: &Schematic, id: Id, key: &str, value: &str) -> Result<Change, EditError> {
    let (sheet, obj) = sch.object(id).ok_or(SchematicError::NoSuchObject(id))?;
    let mut new = obj.clone();
    match (&mut new, key) {
        (Object::Component(c), "reference") => c.reference = value.trim().to_owned(),
        (Object::Component(c), "value") => c.value = value.to_owned(),
        (Object::Component(c), "footprint") => c.footprint = value.to_owned(),
        (Object::Component(c), "unit") => {
            c.unit = value
                .trim()
                .parse()
                .map_err(|_| EditError::Invalid("unit must be a whole number".into()))?
        }
        (Object::Component(c), "representation") => {
            c.representation = match value.trim() {
                "1" | "normal" => REPR_NORMAL,
                "2" | "alternate" => REPR_ALTERNATE,
                other => {
                    return Err(EditError::Invalid(format!(
                        "representation must be normal or alternate, got {other:?}"
                    )))
                }
            }
        }
        // SPICE associations (`Spice_Model`, `Spice_Value`, `Spice_Nodes`, `Spice_Enable`).
        (Object::Component(c), k) if k.starts_with("Spice_") => {
            let v = value.trim();
            if v.len() > 4000 || v.chars().any(|ch| ch.is_control() && ch != '\n') {
                return Err(EditError::Invalid(
                    "a SPICE property is too long or has control characters".into(),
                ));
            }
            if v.is_empty() {
                c.properties.remove(k);
            } else {
                c.properties.insert(k.to_owned(), v.to_owned());
            }
        }
        (Object::NetLabel(l), "name") => l.name = value.trim().to_owned(),
        (Object::GlobalLabel(l), "name") => l.name = value.trim().to_owned(),
        (Object::HierLabel(l), "name") => l.name = value.trim().to_owned(),
        (Object::PowerSymbol(p), "net_name") => p.net_name = value.trim().to_owned(),
        _ => {
            return Err(EditError::Invalid(format!(
                "property {key:?} cannot be edited on a {}",
                obj.kind()
            )))
        }
    }
    Ok(Change::modify(sheet, id, obj.clone(), new))
}

/// Ids of objects on `sheet` whose bounds intersect `area` (used by tools for hit-testing).
pub fn objects_in(sch: &Schematic, sheet: Id, area: &BoundingBox) -> Vec<Id> {
    sheet_objects(sch, sheet)
        .into_iter()
        .filter(|(_, o)| sch.object_bounds(o).intersects(area))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_increment_keeping_width() {
        assert_eq!(next_label_name("D3"), "D4");
        assert_eq!(next_label_name("A09"), "A10");
        assert_eq!(next_label_name("A099"), "A100");
        assert_eq!(next_label_name("CLK"), "CLK");
        assert_eq!(next_label_name("9"), "10");
    }

    #[test]
    fn elbows_are_orthogonal() {
        let (a, b) = (Point::new(0, 0), Point::new(5, 3));
        assert_eq!(elbow(a, b, true), vec![a, Point::new(5, 0), b]);
        assert_eq!(elbow(a, b, false), vec![a, Point::new(0, 3), b]);
        assert_eq!(elbow(a, Point::new(5, 0), true), vec![a, Point::new(5, 0)]);
    }

    fn stretch(
        fixed: (i64, i64),
        old: (i64, i64),
        new: (i64, i64),
        anchor: Option<(i64, i64)>,
    ) -> Stretch {
        let p = |t: (i64, i64)| Point::new(t.0, t.1);
        Stretch {
            sheet: Id(0),
            id: Id(1),
            original: Object::Wire(Wire {
                segment: Segment::new(p(fixed), p(old)),
            }),
            fixed: p(fixed),
            old_end: p(old),
            new_end: p(new),
            anchor: p(anchor.unwrap_or(fixed)),
        }
    }

    fn seg(a: (i64, i64), b: (i64, i64)) -> Segment {
        Segment::new(Point::new(a.0, a.1), Point::new(b.0, b.1))
    }

    #[test]
    fn stretched_wires_keep_their_direction_first() {
        // Horizontal wire, end moves up and right: stay horizontal, then turn.
        let s = stretch((0, 0), (10, 0), (14, 6), None);
        assert_eq!(s.legs(0), vec![seg((0, 0), (14, 0)), seg((14, 0), (14, 6))]);
        // The next choice turns first.
        assert_eq!(s.legs(1), vec![seg((0, 0), (0, 6)), seg((0, 6), (14, 6))]);
        // Vertical wire: vertical first.
        let s = stretch((0, 0), (0, 10), (4, 14), None);
        assert_eq!(s.legs(0), vec![seg((0, 0), (0, 14)), seg((0, 14), (4, 14))]);
        // Pure extension needs one leg; collapsing onto the fixed end removes the wire.
        assert_eq!(stretch((0, 0), (10, 0), (20, 0), None).legs(0).len(), 1);
        assert!(stretch((0, 0), (10, 0), (0, 0), None).legs(0).is_empty());
    }

    #[test]
    fn z_routes_cross_over_half_way() {
        let g = ROUTE_GRID;
        // A vertical wire's third choice is vertical-horizontal-vertical.
        let s = stretch((0, 0), (0, 4 * g), (6 * g, 8 * g), None);
        assert_eq!(
            s.legs(2),
            vec![
                seg((0, 0), (0, 4 * g)),
                seg((0, 4 * g), (6 * g, 4 * g)),
                seg((6 * g, 4 * g), (6 * g, 8 * g))
            ]
        );
        // ...and the fourth is horizontal-vertical-horizontal.
        assert_eq!(
            s.legs(3),
            vec![
                seg((0, 0), (3 * g, 0)),
                seg((3 * g, 0), (3 * g, 8 * g)),
                seg((3 * g, 8 * g), (6 * g, 8 * g))
            ]
        );
        // Detours leave the straight corridor: 3 grid steps above the midpoint.
        assert_eq!(
            s.legs(4),
            vec![
                seg((0, 0), (0, 7 * g)),
                seg((0, 7 * g), (6 * g, 7 * g)),
                seg((6 * g, 7 * g), (6 * g, 8 * g))
            ]
        );
    }

    #[test]
    fn a_mid_wire_contact_is_kept_and_the_new_route_turns_away_from_it() {
        // Vertical wire (0,0)-(0,10) with a junction at (0,7): keep (0,0)-(0,7), then leave sideways.
        let s = stretch((0, 0), (0, 10), (6, 12), Some((0, 7)));
        assert_eq!(
            s.legs(0),
            vec![
                seg((0, 0), (0, 7)),
                seg((0, 7), (6, 7)),
                seg((6, 7), (6, 12))
            ]
        );
    }

    #[test]
    fn placement_ops_match_geometry_for_every_orientation() {
        let local = Point::new(3_810_000, 1_270_000);
        for rotation in 0..4u8 {
            for mirror in [false, true] {
                let pl = Placement {
                    position: Point::new(10_160_000, -5_080_000),
                    rotation,
                    mirror,
                };
                let pivot = Point::new(2_540_000, 1_270_000);
                for op in [
                    Op::Translate(Vector::new(1_270_000, -2_540_000)),
                    Op::Rotate { pivot },
                    Op::MirrorX { axis: pivot.x },
                    Op::MirrorY { axis: pivot.y },
                ] {
                    let expected = op.point(pl.apply(local));
                    let got = op.placement(pl).apply(local);
                    assert_eq!(
                        got, expected,
                        "{op:?} on rotation {rotation} mirror {mirror}"
                    );
                    for d in [
                        Direction::Right,
                        Direction::Up,
                        Direction::Left,
                        Direction::Down,
                    ] {
                        assert_eq!(
                            op.placement(pl).apply_direction(d),
                            {
                                // Direction transform of the placed direction.
                                op.direction(pl.apply_direction(d))
                            },
                            "{op:?} direction {d:?}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod part_data_tests {
    use super::*;

    #[test]
    fn a_new_part_carries_what_its_symbol_knows() {
        let mut s = crate::builtin::symbols().into_iter().next().unwrap();
        s.meta.mpn = "RC0603FR-0710KL".into();
        s.meta.manufacturer = "Yageo".into();
        s.meta.default_footprint = "Chip_0603".into();
        s.meta.properties.insert("Tolerance".into(), "1%".into());
        let c = new_component(
            &Schematic::new(Id(0)),
            &s,
            Placement::at(buturecad_geometry::Point::new(0, 0)),
        );
        assert_eq!(c.footprint, "Chip_0603");
        assert_eq!(c.properties["MPN"], "RC0603FR-0710KL");
        assert_eq!(c.properties["Manufacturer"], "Yageo");
        assert_eq!(c.properties["Tolerance"], "1%");
        assert!(
            !c.properties.contains_key("Datasheet"),
            "empty fields are not written"
        );
    }

    #[test]
    fn part_data_overrides_only_what_it_has() {
        let s = crate::builtin::symbols().into_iter().next().unwrap();
        let mut c = new_component(
            &Schematic::new(Id(0)),
            &s,
            Placement::at(buturecad_geometry::Point::new(0, 0)),
        );
        c.footprint = "Keep".into();
        PartData {
            mpn: "X1".into(),
            ..PartData::default()
        }
        .apply(&mut c);
        assert_eq!(
            (c.footprint.as_str(), c.properties["MPN"].as_str()),
            ("Keep", "X1")
        );
        PartData {
            footprint: "New".into(),
            ..PartData::default()
        }
        .apply(&mut c);
        assert_eq!(c.footprint, "New");
    }
}
