#![allow(clippy::unwrap_used)]

use buturecad_geometry::{Point, Segment, Vector};
use buturecad_schematic::edit::*;
use buturecad_schematic::{
    builtin, Change, Component, Connectivity, Direction, Id, Junction, Object, Placement,
    PowerSymbol, Schematic, Wire,
};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

const G: i64 = 1_270_000;

struct World {
    sch: Schematic,
    gen: IdGen,
}

impl World {
    fn new() -> Self {
        let mut sch = Schematic::new(Id(0));
        builtin::install(&mut sch);
        Self {
            sch,
            gen: IdGen::new(1),
        }
    }
    fn root(&self) -> Id {
        self.sch.root()
    }
    fn put(&mut self, o: Object) -> Id {
        let id = self.gen.alloc();
        let root = self.root();
        self.sch.insert(root, id, o).unwrap();
        id
    }
    fn part(&mut self, lib: &str, reference: &str, x: i64, y: i64, rotation: u8) -> Id {
        self.put(Object::Component(Component {
            lib_id: lib.into(),
            reference: reference.into(),
            value: String::new(),
            footprint: String::new(),
            properties: Default::default(),
            placement: Placement {
                position: Point::new(x * G, y * G),
                rotation,
                mirror: false,
            },
        }))
    }
    fn wire(&mut self, a: (i64, i64), b: (i64, i64)) -> Id {
        self.put(Object::Wire(Wire {
            segment: Segment::new(Point::new(a.0 * G, a.1 * G), Point::new(b.0 * G, b.1 * G)),
        }))
    }
    fn junction(&mut self, at: (i64, i64)) -> Id {
        self.put(Object::Junction(Junction {
            position: Point::new(at.0 * G, at.1 * G),
        }))
    }
    fn power(&mut self, lib: &str, net: &str, at: (i64, i64)) -> Id {
        self.put(Object::PowerSymbol(PowerSymbol {
            lib_id: lib.into(),
            net_name: net.into(),
            placement: Placement::at(Point::new(at.0 * G, at.1 * G)),
        }))
    }
    /// Applies a verified edit and asserts that it kept the circuit's connectivity.
    fn apply_edit(&mut self, e: &Edited) {
        assert!(
            e.connectivity_preserved,
            "the edit changed the circuit's connectivity"
        );
        self.apply(&e.changes);
    }

    fn apply(&mut self, changes: &[Change]) {
        self.sch.apply(changes).unwrap();
        // Keep ids fresh for later edits.
        let max = changes.iter().map(|c| c.id.0).max().unwrap_or(0);
        self.gen.next = self.gen.next.max(max + 1);
    }
}

/// +3V3 → R1 → D1 → GND, a capacitor across, all wires orthogonal.
fn circuit() -> (World, Id, Id, Id) {
    let mut w = World::new();
    let r1 = w.part("Buture:R", "R1", 20, 24, 0); // pins (20,27) (20,21)
    let d1 = w.part("Buture:LED", "D1", 20, 14, 1); // anode (20,17) cathode (20,11)
    let c1 = w.part("Buture:C", "C1", 32, 19, 0); // pins (32,22) (32,16)
    w.wire((20, 30), (20, 27));
    w.wire((20, 21), (20, 17));
    w.wire((20, 11), (20, 8));
    w.wire((32, 22), (32, 29));
    w.wire((32, 29), (20, 29));
    w.wire((32, 16), (32, 8));
    w.wire((32, 8), (20, 8));
    w.junction((20, 29));
    w.junction((20, 8));
    w.power("Buture:+3V3", "+3V3", (20, 30));
    w.power("Buture:GND", "GND", (20, 8));
    (w, r1, d1, c1)
}

/// Which pins share a net, independent of net names: a set of sets of "REF.pin".
fn topology(sch: &Schematic) -> BTreeSet<BTreeSet<String>> {
    Connectivity::build(sch)
        .nets()
        .iter()
        .filter(|n| n.pins.len() > 1 || n.named)
        .map(|n| {
            n.pins
                .iter()
                .map(|p| format!("{}.{}", p.reference, p.pin))
                .collect()
        })
        .collect()
}

fn ids(v: &[Id]) -> BTreeSet<Id> {
    v.iter().copied().collect()
}

fn all_wires_orthogonal(sch: &Schematic) -> bool {
    sch.sheets()
        .values()
        .flat_map(|s| s.objects.values())
        .all(|o| match o {
            Object::Wire(w) => w.segment.a.x == w.segment.b.x || w.segment.a.y == w.segment.b.y,
            _ => true,
        })
}

#[test]
fn moving_a_part_keeps_every_connection() {
    let (mut w, r1, _, _) = circuit();
    let before = topology(&w.sch);
    let edit = move_objects(
        &w.sch,
        &ids(&[r1]),
        Vector::new(6 * G, 3 * G),
        true,
        &mut w.gen,
    )
    .unwrap();
    w.apply_edit(&edit);
    assert_eq!(
        topology(&w.sch),
        before,
        "the stretched wires must keep R1 connected"
    );
    assert!(
        all_wires_orthogonal(&w.sch),
        "stretched wires stay orthogonal"
    );
    // R1 really moved.
    let Object::Component(c) = w.sch.object(r1).unwrap().1 else {
        unreachable!()
    };
    assert_eq!(c.placement.position, Point::new(26 * G, 27 * G));
}

#[test]
fn without_stretch_the_connection_is_lost() {
    let (mut w, r1, _, _) = circuit();
    let before = topology(&w.sch);
    let edit = move_objects(
        &w.sch,
        &ids(&[r1]),
        Vector::new(6 * G, 3 * G),
        false,
        &mut w.gen,
    )
    .unwrap();
    assert!(
        !edit.connectivity_preserved,
        "the verifier must notice the detached part"
    );
    w.apply(&edit.changes);
    assert_ne!(
        topology(&w.sch),
        before,
        "a rigid move without rubber-banding detaches the part"
    );
}

#[test]
fn rotate_and_mirror_keep_connections_for_every_part() {
    for pick in 0..3 {
        for op in 0..3 {
            let (mut w, r1, d1, c1) = circuit();
            let part = [r1, d1, c1][pick];
            let before = topology(&w.sch);
            let set = ids(&[part]);
            let edit = match op {
                0 => rotate_objects(&w.sch, &set, G, &mut w.gen),
                1 => mirror_objects(&w.sch, &set, true, G, &mut w.gen),
                _ => mirror_objects(&w.sch, &set, false, G, &mut w.gen),
            }
            .unwrap();
            w.apply_edit(&edit);
            assert_eq!(topology(&w.sch), before, "part {pick} op {op}");
            assert!(all_wires_orthogonal(&w.sch), "part {pick} op {op}");
        }
    }
}

#[test]
fn four_rotations_about_one_pivot_restore_the_parts_exactly() {
    let (mut w, r1, d1, _) = circuit();
    let original = w.sch.clone();
    let set = ids(&[r1, d1]);
    let pivot = pivot_for(&w.sch, &set, G).unwrap();
    for turn in 0..4 {
        let edit =
            transform_selection_checked(&w.sch, &set, Op::Rotate { pivot }, true, &mut w.gen)
                .unwrap();
        if turn == 0 {
            assert!(
                edit.connectivity_preserved,
                "a single quarter turn of this pair can be re-wired cleanly"
            );
        }
        // Later turns pile re-routed wires on top of each other; the placements stay exact
        // whether or not the verifier could keep every net intact.
        w.apply(&edit.changes);
    }
    for id in [r1, d1] {
        assert_eq!(
            w.sch.object(id).unwrap().1,
            original.object(id).unwrap().1,
            "exact integer rotation has no drift"
        );
    }
}

#[test]
fn group_move_carries_selected_wires_and_stretches_the_rest() {
    let (mut w, r1, d1, _) = circuit();
    let wire_between = w.sch.sheets()[&w.root()]
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            Object::Wire(x) if x.segment.a == Point::new(20 * G, 21 * G) => Some(*id),
            _ => None,
        });
    let before = topology(&w.sch);
    let sel = ids(&[r1, d1, wire_between.unwrap()]);
    let edit = move_objects(&w.sch, &sel, Vector::new(0, -2 * G), true, &mut w.gen).unwrap();
    w.apply_edit(&edit);
    assert_eq!(topology(&w.sch), before);
    let Object::Wire(x) = w.sch.object(wire_between.unwrap()).unwrap().1 else {
        unreachable!()
    };
    assert_eq!(
        x.segment,
        Segment::new(Point::new(20 * G, 19 * G), Point::new(20 * G, 15 * G)),
        "the selected wire moved rigidly"
    );
}

#[test]
fn junctions_travel_with_the_connection_they_belong_to() {
    let (mut w, ..) = circuit();
    // The junction at (20,29) joins the +3V3 stub, the capacitor tap and R1's wire.
    let power = w.sch.sheets()[&w.root()]
        .objects
        .iter()
        .find_map(|(id, o)| {
            matches!(o, Object::PowerSymbol(p) if p.net_name == "+3V3").then_some(*id)
        })
        .unwrap();
    let before = topology(&w.sch);
    let edit = move_objects(
        &w.sch,
        &ids(&[power]),
        Vector::new(4 * G, 0),
        true,
        &mut w.gen,
    )
    .unwrap();
    w.apply_edit(&edit);
    assert_eq!(topology(&w.sch), before);
}

#[test]
fn delete_removes_orphan_junctions_but_keeps_needed_ones() {
    let mut w = World::new();
    let a = w.wire((0, 0), (10, 0));
    let b = w.wire((5, 0), (5, 5)); // T onto a
    let c = w.wire((5, 0), (5, -5)); // and a fourth end
    let j = w.junction((5, 0));
    // Deleting one branch leaves a + c: degree 2 + 1 = 3 → junction stays.
    let changes = delete_objects(&w.sch, &ids(&[b])).unwrap();
    assert!(changes.iter().all(|ch| ch.id != j));
    w.apply(&changes);
    // Deleting the other branch leaves only the through-wire: the junction is orphaned.
    let changes = delete_objects(&w.sch, &ids(&[c])).unwrap();
    assert!(
        changes.iter().any(|ch| ch.id == j),
        "orphan junction must go too"
    );
    w.apply(&changes);
    assert!(w.sch.object(j).is_none() && w.sch.object(a).is_some());
}

#[test]
fn delete_roundtrips_through_revert() {
    let (mut w, r1, ..) = circuit();
    let original = w.sch.clone();
    let changes = delete_objects(&w.sch, &ids(&[r1])).unwrap();
    w.apply(&changes);
    assert!(w.sch.object(r1).is_none());
    w.sch.revert(&changes).unwrap();
    assert_eq!(w.sch, original);
    assert_eq!(
        delete_objects(&w.sch, &BTreeSet::new()),
        Err(EditError::EmptySelection)
    );
}

fn draw(w: &mut World, pts: &[(i64, i64)]) -> Vec<Change> {
    let points: Vec<Point> = pts.iter().map(|p| Point::new(p.0 * G, p.1 * G)).collect();
    let root = w.root();
    draw_path(&w.sch, root, &points, false, &mut w.gen).unwrap()
}

fn count(changes: &[Change], pred: impl Fn(&Object) -> bool) -> usize {
    changes
        .iter()
        .filter(|c| c.after.as_ref().is_some_and(&pred))
        .count()
}

#[test]
fn drawn_wires_merge_collinear_steps_and_drop_zero_length_ones() {
    let mut w = World::new();
    let ch = draw(&mut w, &[(0, 0), (3, 0), (3, 0), (8, 0), (8, 4)]);
    assert_eq!(count(&ch, |o| matches!(o, Object::Wire(_))), 2);
    assert!(draw_path(
        &w.sch,
        Id(0),
        &[Point::ORIGIN, Point::ORIGIN],
        false,
        &mut w.gen
    )
    .is_err());
}

#[test]
fn t_connections_get_a_junction_but_crossings_do_not() {
    let mut w = World::new();
    w.wire((0, 0), (10, 0));
    // Ends on the interior of the existing wire: junction.
    let ch = draw(&mut w, &[(5, 0), (5, 6)]);
    assert_eq!(
        count(
            &ch,
            |o| matches!(o, Object::Junction(j) if j.position == Point::new(5 * G, 0))
        ),
        1
    );
    // Passes straight across it: no junction, no connection.
    let ch = draw(&mut w, &[(7, -4), (7, 4)]);
    assert_eq!(count(&ch, |o| matches!(o, Object::Junction(_))), 0);
    // An existing wire end lands on the interior of the new wire: junction.
    let mut w = World::new();
    w.wire((4, 0), (4, 5));
    let ch = draw(&mut w, &[(0, 0), (9, 0)]);
    assert_eq!(
        count(
            &ch,
            |o| matches!(o, Object::Junction(j) if j.position == Point::new(4 * G, 0))
        ),
        1
    );
}

#[test]
fn three_ends_meeting_get_one_junction_and_two_do_not() {
    let mut w = World::new();
    w.wire((0, 0), (5, 0));
    let ch = draw(&mut w, &[(5, 0), (5, 5)]);
    assert_eq!(
        count(&ch, |o| matches!(o, Object::Junction(_))),
        0,
        "two ends make a plain corner"
    );
    w.apply(&ch);
    let ch = draw(&mut w, &[(5, 0), (9, 0)]);
    assert_eq!(
        count(&ch, |o| matches!(o, Object::Junction(_))),
        1,
        "the third end needs a junction"
    );
    w.apply(&ch);
    let ch = draw(&mut w, &[(5, 0), (5, -4)]);
    assert_eq!(
        count(&ch, |o| matches!(o, Object::Junction(_))),
        0,
        "a junction already exists there"
    );
}

#[test]
fn a_wire_ending_on_a_pin_needs_no_junction() {
    let mut w = World::new();
    w.part("Buture:R", "R1", 0, 0, 0); // pin 1 at (0,3)
    let ch = draw(&mut w, &[(0, 3), (0, 8)]);
    assert_eq!(count(&ch, |o| matches!(o, Object::Junction(_))), 0);
    // Buses never get junctions.
    let root = w.root();
    let bus = draw_path(
        &w.sch,
        root,
        &[
            Point::ORIGIN,
            Point::new(5 * G, 0),
            Point::new(5 * G, 5 * G),
        ],
        true,
        &mut w.gen,
    )
    .unwrap();
    assert_eq!(count(&bus, |o| matches!(o, Object::Bus(_))), 2);
}

#[test]
fn drawing_connects_what_it_touches() {
    let mut w = World::new();
    let r1 = w.part("Buture:R", "R1", 0, 0, 0); // pins (0,3) (0,-3)
    let r2 = w.part("Buture:R", "R2", 12, 0, 0);
    let path = elbow(Point::new(0, -3 * G), Point::new(12 * G, 3 * G), true);
    let root = w.root();
    let ch = draw_path(&w.sch, root, &path, false, &mut w.gen).unwrap();
    w.apply(&ch);
    let c = Connectivity::build(&w.sch);
    let net = c.net_of_pin(r1, "2").unwrap();
    assert!(
        net.pins.iter().any(|p| p.component == r2 && p.pin == "1"),
        "the L-shaped wire joins R1.2 to R2.1"
    );
}

#[test]
fn duplicate_assigns_new_ids_and_references() {
    let (mut w, r1, d1, _) = circuit();
    let original = w.sch.clone();
    let (changes, new_ids) = duplicate_objects(
        &w.sch,
        &ids(&[r1, d1]),
        Vector::new(2 * G, -2 * G),
        &mut w.gen,
    )
    .unwrap();
    assert_eq!(new_ids.len(), 2);
    w.apply(&changes);
    let refs: Vec<String> = new_ids
        .iter()
        .map(|id| match w.sch.object(*id).unwrap().1 {
            Object::Component(c) => c.reference.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(refs, vec!["R2", "D2"]);
    let Object::Component(c) = w.sch.object(new_ids[0]).unwrap().1 else {
        unreachable!()
    };
    assert_eq!(c.placement.position, Point::new(22 * G, 22 * G));
    w.sch.revert(&changes).unwrap();
    assert_eq!(w.sch, original);
}

#[test]
fn paste_brings_its_symbols_and_never_duplicates_references() {
    let (w, r1, d1, c1) = circuit();
    let clip = copy_objects(&w.sch, &ids(&[r1, d1, c1]), G).unwrap();
    assert_eq!(
        clip.symbols.len(),
        3,
        "R, LED and C travel with the clipboard"
    );
    // A fresh schematic without any symbols.
    let mut other = World::new();
    other.sch = Schematic::new(Id(0));
    assert!(paste_objects(
        &other.sch,
        &clip,
        Id(0),
        Point::new(10 * G, 10 * G),
        &mut other.gen
    )
    .is_ok());
    for s in &clip.symbols {
        other.sch.add_symbol(s.clone());
    }
    let (changes, _) = paste_objects(
        &other.sch,
        &clip,
        Id(0),
        Point::new(10 * G, 10 * G),
        &mut other.gen,
    )
    .unwrap();
    other.apply(&changes);
    // Pasting again into the same schematic must not reuse references.
    let (again, _) = paste_objects(
        &other.sch,
        &clip,
        Id(0),
        Point::new(30 * G, 10 * G),
        &mut other.gen,
    )
    .unwrap();
    other.apply(&again);
    let refs: BTreeSet<String> = other
        .sch
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
    assert_eq!(refs.len(), 6, "{refs:?}");
    // The clipboard's anchor lands at the paste position.
    let bounds = selection_bounds(
        &other.sch,
        &other.sch.sheets()[&Id(0)]
            .objects
            .keys()
            .copied()
            .take(3)
            .collect(),
    )
    .unwrap();
    assert!(bounds.min.x >= 10 * G - 6 * G);
}

#[test]
fn property_edits_validate_and_roundtrip() {
    let (mut w, r1, ..) = circuit();
    let ch = set_property(&w.sch, r1, "value", "4k7").unwrap();
    w.apply(std::slice::from_ref(&ch));
    let Object::Component(c) = w.sch.object(r1).unwrap().1 else {
        unreachable!()
    };
    assert_eq!(c.value, "4k7");
    w.sch.revert(&[ch]).unwrap();
    assert!(set_property(&w.sch, r1, "bogus", "x").is_err());
    assert!(set_property(&w.sch, Id(9999), "value", "x").is_err());
    let blank = set_property(&w.sch, r1, "reference", "  ").unwrap();
    assert!(
        w.sch.apply(&[blank]).is_err(),
        "an empty reference is rejected by validation"
    );
}

#[test]
fn new_parts_get_the_next_free_reference_and_defaults() {
    let (w, ..) = circuit();
    let r = w.sch.symbol("Buture:R").unwrap();
    let c = new_component(&w.sch, r, Placement::at(Point::ORIGIN));
    assert_eq!(
        (c.reference.as_str(), c.value.as_str(), c.lib_id.as_str()),
        ("R2", "10k", "Buture:R")
    );
    let gnd = new_power_symbol(
        w.sch.symbol("Buture:GND").unwrap(),
        Placement::at(Point::ORIGIN),
    );
    assert_eq!(gnd.net_name, "GND");
}

#[test]
fn objects_of_different_sheets_cannot_be_edited_together() {
    let (mut w, r1, ..) = circuit();
    w.sch.add_sheet(Id(900), "Child").unwrap();
    let other = Id(901);
    w.sch
        .insert(
            Id(900),
            other,
            Object::Junction(Junction {
                position: Point::ORIGIN,
            }),
        )
        .unwrap();
    assert!(move_objects(
        &w.sch,
        &ids(&[r1, other]),
        Vector::new(G, 0),
        true,
        &mut w.gen
    )
    .is_err());
}

fn orientation_of(sch: &Schematic, id: Id) -> Direction {
    match sch.object(id).unwrap().1 {
        Object::NetLabel(l) => l.orientation,
        _ => unreachable!(),
    }
}

#[test]
fn labels_turn_and_mirror_with_the_selection() {
    let mut w = World::new();
    let l = w.put(Object::NetLabel(buturecad_schematic::NetLabel {
        position: Point::new(5 * G, 5 * G),
        name: "SDA".into(),
        orientation: Direction::Right,
    }));
    let set = ids(&[l]);
    let ch = rotate_objects(&w.sch, &set, G, &mut w.gen).unwrap();
    w.apply(&ch.changes);
    assert_eq!(orientation_of(&w.sch, l), Direction::Up);
    let ch = mirror_objects(&w.sch, &set, false, G, &mut w.gen).unwrap();
    w.apply(&ch.changes);
    assert_eq!(orientation_of(&w.sch, l), Direction::Down);
    let ch = mirror_objects(&w.sch, &set, true, G, &mut w.gen).unwrap();
    w.apply(&ch.changes);
    assert_eq!(
        orientation_of(&w.sch, l),
        Direction::Down,
        "a horizontal mirror leaves vertical labels alone"
    );
}

proptest! {
    /// For any rigid move of any single part: wires stay orthogonal, applying and reverting
    /// restores the schematic exactly, and the preservation report is truthful (it says
    /// "preserved" exactly when the pin topology really is unchanged).
    #[test]
    fn moving_any_part_anywhere_is_reversible_and_honest(pick in 0usize..3, dx in -40i64..40, dy in -40i64..40) {
        let (mut w, r1, d1, c1) = circuit();
        let part = [r1, d1, c1][pick];
        let before = topology(&w.sch);
        let original = w.sch.clone();
        let edit = move_objects(&w.sch, &ids(&[part]), Vector::new(dx * G, dy * G), true, &mut w.gen).unwrap();
        w.apply(&edit.changes);
        prop_assert!(all_wires_orthogonal(&w.sch));
        prop_assert_eq!(edit.connectivity_preserved, topology(&w.sch) == before);
        w.sch.revert(&edit.changes).unwrap();
        prop_assert_eq!(w.sch, original);
    }

    /// Moves that land in free space (to the left of everything, where no other net can be hit)
    /// must always preserve connectivity.
    #[test]
    fn moves_into_free_space_always_preserve_connectivity(pick in 0usize..3, dx in 1i64..40, dy in -20i64..20) {
        let (mut w, r1, d1, c1) = circuit();
        let part = [r1, d1, c1][pick];
        let before = topology(&w.sch);
        // Move far to the right of the drawing (x ≥ 60), clear of every existing wire.
        let edit = move_objects(&w.sch, &ids(&[part]), Vector::new((40 + dx) * G, dy * G), true, &mut w.gen).unwrap();
        prop_assert!(edit.connectivity_preserved);
        w.apply(&edit.changes);
        prop_assert_eq!(topology(&w.sch), before);
    }

    /// Random sequences of rotations/mirrors never leave a report that contradicts reality.
    #[test]
    fn transform_sequences_report_connectivity_truthfully(ops in proptest::collection::vec((0usize..3, 0usize..3), 1..8)) {
        let (mut w, r1, d1, c1) = circuit();
        for (pick, op) in ops {
            let set = ids(&[[r1, d1, c1][pick]]);
            let before = topology(&w.sch);
            let edit = match op {
                0 => rotate_objects(&w.sch, &set, G, &mut w.gen),
                1 => mirror_objects(&w.sch, &set, true, G, &mut w.gen),
                _ => mirror_objects(&w.sch, &set, false, G, &mut w.gen),
            }.unwrap();
            w.apply(&edit.changes);
            prop_assert_eq!(edit.connectivity_preserved, topology(&w.sch) == before);
            prop_assert!(w.sch.validate().is_ok());
            if !edit.connectivity_preserved {
                break; // the circuit changed; later steps would compare against a different baseline
            }
        }
    }
}

#[test]
fn unused_helpers_stay_consistent() {
    let (w, r1, ..) = circuit();
    let m: BTreeMap<_, _> = [(r1, ())].into_iter().collect();
    assert_eq!(m.len(), 1);
    let area = selection_bounds(&w.sch, &ids(&[r1])).unwrap();
    assert!(objects_in(&w.sch, w.root(), &area).contains(&r1));
    assert!(next_label_name("D9") == "D10");
    assert!(add_junction(
        &w.sch,
        w.root(),
        Point::new(20 * G, 29 * G),
        &mut IdGen::new(5000)
    )
    .is_empty());
    assert_eq!(
        add_junction(
            &w.sch,
            w.root(),
            Point::new(50 * G, 29 * G),
            &mut IdGen::new(5000)
        )
        .len(),
        1
    );
}
