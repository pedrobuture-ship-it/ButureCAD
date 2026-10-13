#![allow(clippy::unwrap_used)]

use buturecad_eda_core::commands::*;
use buturecad_eda_core::model::*;
use buturecad_eda_core::{Command, CommandEngine, CommandError, Project, ROOT_SHEET};
use buturecad_geometry::{Point, Segment};
use buturecad_schematic::{builtin, Component, Object, Placement, SchematicError, Wire};
use proptest::prelude::*;

fn engine() -> CommandEngine {
    CommandEngine::new(Project::new("T").unwrap())
}
fn comp(r: &str, x: i64) -> Component {
    Component {
        lib_id: String::new(),
        reference: r.into(),
        value: "10k".into(),
        footprint: String::new(),
        properties: Default::default(),
        unit: 1,
        representation: 1,
        placement: Placement::at(Point::new(x, 0)),
    }
}
fn track(x: i64) -> Track {
    Track {
        layer: LayerId(0),
        segment: Segment::new(Point::new(0, 0), Point::new(x, 0)),
        width: 200_000,
    }
}
fn objects(e: &CommandEngine) -> usize {
    e.project().schematic().object_count()
}
fn place(e: &mut CommandEngine, r: &str) -> Id {
    let id = e.new_id();
    e.execute(place_component(ROOT_SHEET, id, comp(r, 0)))
        .unwrap();
    id
}
fn position(e: &CommandEngine, id: Id) -> Point {
    match e.project().schematic().object(id).unwrap().1 {
        Object::Component(c) => c.placement.position,
        _ => unreachable!(),
    }
}
fn move_to(e: &mut CommandEngine, id: Id, p: Point) -> Result<(), CommandError> {
    let cmd = move_component(e.project().schematic(), id, p)?;
    e.execute(cmd)
}
fn delete(e: &mut CommandEngine, id: Id) -> Result<(), CommandError> {
    let cmd = delete_objects(e.project().schematic(), &[id])?;
    e.execute(cmd)
}

#[test]
fn place_undo_redo_keeps_the_same_id() {
    let mut e = engine();
    let id = place(&mut e, "R1");
    assert_eq!(e.undo().unwrap().as_deref(), Some("Place component"));
    assert_eq!(objects(&e), 0);
    assert_eq!(e.redo().unwrap().as_deref(), Some("Place component"));
    assert!(e.project().schematic().object(id).is_some());
}

#[test]
fn move_and_delete_roundtrip() {
    let mut e = engine();
    let id = place(&mut e, "R1");
    move_to(&mut e, id, Point::new(500, 700)).unwrap();
    assert_eq!(position(&e, id), Point::new(500, 700));
    delete(&mut e, id).unwrap();
    assert_eq!(objects(&e), 0);
    e.undo().unwrap();
    assert_eq!(position(&e, id), Point::new(500, 700));
    e.undo().unwrap();
    assert_eq!(position(&e, id), Point::new(0, 0));
}

#[test]
fn wire_track_via_commands() {
    let mut e = engine();
    let wire_id = e.new_id();
    e.execute(create_wire(
        ROOT_SHEET,
        wire_id,
        Segment::new(Point::new(0, 0), Point::new(100, 0)),
    ))
    .unwrap();
    e.execute(CreateTrackCommand::new(track(1_000_000)))
        .unwrap();
    e.execute(CreateViaCommand::new(Via {
        position: Point::new(5, 5),
        diameter: 600_000,
        drill: 300_000,
    }))
    .unwrap();
    assert_eq!(
        (
            objects(&e),
            e.project().pcb().tracks.len(),
            e.project().pcb().vias.len()
        ),
        (1, 1, 1)
    );
    let track_id = *e.project().pcb().tracks.keys().next().unwrap();
    e.execute(DeleteTrackCommand::new(track_id)).unwrap();
    assert!(e.project().pcb().tracks.is_empty());
    for _ in 0..4 {
        e.undo().unwrap();
    }
    assert_eq!(e.project(), &Project::new("T").unwrap());
}

#[test]
fn symbols_are_added_through_a_command_and_undone_with_it() {
    let mut e = engine();
    e.transaction("Place resistor", |e| {
        e.execute(AddSymbolsCommand::new(builtin::symbols()))?;
        let id = e.new_id();
        let mut c = comp("R1", 0);
        c.lib_id = "Buture:R".into();
        e.execute(place_component(ROOT_SHEET, id, c))
    })
    .unwrap();
    assert!(e.project().schematic().symbol("Buture:R").is_some());
    e.undo().unwrap();
    assert!(e.project().schematic().symbol("Buture:R").is_none());
    assert_eq!(e.project(), &Project::new("T").unwrap());
    e.redo().unwrap();
    assert!(e.project().schematic().symbol("Buture:R").is_some());
}

#[test]
fn placing_a_part_without_its_symbol_is_rejected() {
    let mut e = engine();
    let id = e.new_id();
    let mut c = comp("R1", 0);
    c.lib_id = "Buture:R".into();
    assert_eq!(
        e.execute(place_component(ROOT_SHEET, id, c)),
        Err(CommandError::Schematic(SchematicError::UnknownSymbol(
            "Buture:R".into()
        )))
    );
}

#[test]
fn invalid_commands_fail_atomically_and_leave_no_history() {
    let mut e = engine();
    let before = e.project().clone();
    let id = e.new_id();
    let flat = Wire {
        segment: Segment::new(Point::ORIGIN, Point::ORIGIN),
    };
    let bad = [
        e.execute(place_component(
            ROOT_SHEET,
            id,
            Component {
                reference: " ".into(),
                ..comp("x", 0)
            },
        )),
        e.execute(place_component(
            ROOT_SHEET,
            id,
            Component {
                placement: Placement {
                    rotation: 4,
                    ..Placement::at(Point::ORIGIN)
                },
                ..comp("R", 0)
            },
        )),
        e.execute(create_object("w", ROOT_SHEET, id, Object::Wire(flat))),
        e.execute(CreateTrackCommand::new(Track {
            width: 0,
            ..track(10)
        })),
        e.execute(CreateTrackCommand::new(Track {
            segment: Segment::new(Point::ORIGIN, Point::ORIGIN),
            ..track(10)
        })),
        e.execute(CreateViaCommand::new(Via {
            position: Point::ORIGIN,
            diameter: 300,
            drill: 300,
        })),
        e.execute(DeleteViaCommand::new(Id(99))),
        e.execute(RenameProjectCommand::new("")),
    ];
    assert!(bad.iter().all(Result::is_err), "{bad:?}");
    assert!(move_component(e.project().schematic(), Id(99), Point::ORIGIN).is_err());
    assert!(delete_objects(e.project().schematic(), &[Id(99)]).is_err());
    assert_eq!(e.project(), &before);
    assert!(e.undo_history().is_empty() && !e.is_dirty());
}

#[test]
fn out_of_range_geometry_is_rejected() {
    let mut e = engine();
    let id = e.new_id();
    let far = Component {
        placement: Placement::at(Point::new(i64::MAX, 0)),
        ..comp("R1", 0)
    };
    assert_eq!(
        e.execute(place_component(ROOT_SHEET, id, far)),
        Err(CommandError::Schematic(SchematicError::OutOfRange))
    );
}

#[test]
fn new_command_clears_redo() {
    let mut e = engine();
    place(&mut e, "R1");
    e.undo().unwrap();
    assert!(e.can_redo());
    place(&mut e, "R2");
    assert!(!e.can_redo());
    assert_eq!(e.redo().unwrap(), None);
}

#[test]
fn undo_and_redo_on_empty_history_are_noops() {
    let mut e = engine();
    assert_eq!(e.undo().unwrap(), None);
    assert_eq!(e.redo().unwrap(), None);
}

#[test]
fn transaction_is_a_single_undo_step() {
    let mut e = engine();
    e.begin_transaction("Place resistor pair");
    place(&mut e, "R1");
    place(&mut e, "R2");
    e.commit_transaction().unwrap();
    assert_eq!(e.undo_history(), vec!["Place resistor pair"]);
    assert_eq!(e.undo().unwrap().as_deref(), Some("Place resistor pair"));
    assert_eq!(objects(&e), 0);
    e.redo().unwrap();
    assert_eq!(objects(&e), 2);
}

#[test]
fn rollback_discards_everything_in_the_group() {
    let mut e = engine();
    place(&mut e, "keep");
    e.begin_transaction("oops");
    place(&mut e, "R1");
    e.execute(CreateTrackCommand::new(track(10))).unwrap();
    e.rollback_transaction().unwrap();
    assert_eq!(objects(&e), 1);
    assert!(e.project().pcb().tracks.is_empty());
    assert_eq!(e.undo_history().len(), 1);
}

#[test]
fn closure_transaction_rolls_back_on_error() {
    let mut e = engine();
    let r = e.transaction("batch", |e| {
        let id = e.new_id();
        e.execute(place_component(ROOT_SHEET, id, comp("R1", 0)))?;
        e.execute(DeleteViaCommand::new(Id(999)))
    });
    assert!(matches!(r, Err(CommandError::MissingEntity { .. })));
    assert_eq!(objects(&e), 0);
    assert!(e.undo_history().is_empty());
    assert_eq!(e.transaction_depth(), 0);
}

#[test]
fn nested_transactions_merge_into_the_outer_step() {
    let mut e = engine();
    e.begin_transaction("outer");
    place(&mut e, "R1");
    e.begin_transaction("inner");
    place(&mut e, "R2");
    e.commit_transaction().unwrap();
    assert_eq!(e.transaction_depth(), 1);
    e.commit_transaction().unwrap();
    assert_eq!(e.undo_history(), vec!["outer"]);
    e.undo().unwrap();
    assert_eq!(objects(&e), 0);
    e.redo().unwrap();
    assert_eq!(objects(&e), 2);
}

#[test]
fn inner_rollback_keeps_outer_work() {
    let mut e = engine();
    e.begin_transaction("outer");
    place(&mut e, "R1");
    e.begin_transaction("inner");
    place(&mut e, "R2");
    e.rollback_transaction().unwrap();
    e.commit_transaction().unwrap();
    assert_eq!(objects(&e), 1);
}

#[test]
fn empty_transaction_leaves_no_history() {
    let mut e = engine();
    e.begin_transaction("nothing");
    e.commit_transaction().unwrap();
    assert!(e.undo_history().is_empty() && !e.is_dirty());
}

#[test]
fn undo_redo_are_refused_inside_a_transaction() {
    let mut e = engine();
    place(&mut e, "R1");
    e.begin_transaction("t");
    assert_eq!(e.undo(), Err(CommandError::TransactionOpen));
    assert_eq!(e.redo(), Err(CommandError::TransactionOpen));
    assert_eq!(e.mark_saved(), Err(CommandError::TransactionOpen));
    assert!(!e.can_undo());
    e.rollback_transaction().unwrap();
    assert_eq!(e.commit_transaction(), Err(CommandError::NoTransaction));
    assert_eq!(e.rollback_transaction(), Err(CommandError::NoTransaction));
}

#[test]
fn dirty_state_follows_the_saved_point() {
    let mut e = engine();
    assert!(!e.is_dirty());
    place(&mut e, "R1");
    assert!(e.is_dirty());
    e.mark_saved().unwrap();
    assert!(!e.is_dirty());
    place(&mut e, "R2");
    assert!(e.is_dirty());
    e.undo().unwrap();
    assert!(!e.is_dirty(), "undoing back to the saved state is clean");
    e.undo().unwrap();
    assert!(e.is_dirty());
    e.redo().unwrap();
    assert!(!e.is_dirty());
}

#[test]
fn saved_state_in_a_discarded_redo_branch_is_never_clean_again() {
    let mut e = engine();
    place(&mut e, "R1");
    place(&mut e, "R2");
    e.mark_saved().unwrap(); // saved with 2 steps
    e.undo().unwrap();
    e.undo().unwrap();
    place(&mut e, "other"); // discards the branch that contained the saved state
    place(&mut e, "other2");
    assert!(
        e.is_dirty(),
        "same undo depth as the save point but different content"
    );
}

#[test]
fn open_transaction_with_work_is_dirty() {
    let mut e = engine();
    e.begin_transaction("t");
    assert!(!e.is_dirty());
    place(&mut e, "R1");
    assert!(e.is_dirty());
    e.rollback_transaction().unwrap();
    assert!(!e.is_dirty());
}

#[test]
fn history_limit_drops_oldest_and_tracks_clean_point() {
    let mut e = engine().with_history_limit(3);
    place(&mut e, "R1");
    e.mark_saved().unwrap();
    for i in 2..=5 {
        place(&mut e, &format!("R{i}"));
    }
    assert_eq!(e.undo_history().len(), 3);
    while e.undo().unwrap().is_some() {}
    assert!(e.is_dirty(), "the saved state fell off the history");
    assert_eq!(objects(&e), 2, "R1 and R2 are not undoable anymore");
}

#[test]
fn recovered_engine_starts_dirty_and_autosave_pending() {
    let e = CommandEngine::recovered(Project::new("R").unwrap());
    assert!(e.is_dirty());
    assert!(e.autosave_pending());
}

#[test]
fn autosave_tracking() {
    let mut e = engine();
    assert!(!e.autosave_pending());
    place(&mut e, "R1");
    assert!(e.autosave_pending());
    let rev = e.revision();
    e.mark_autosaved(rev);
    assert!(!e.autosave_pending());
    place(&mut e, "R2");
    assert!(e.autosave_pending());
    e.mark_autosaved(rev); // stale revision: still pending
    assert!(e.autosave_pending());
    e.mark_saved().unwrap();
    assert!(!e.autosave_pending(), "clean projects need no autosave");
}

#[test]
fn rename_project_roundtrip() {
    let mut e = engine();
    e.execute(RenameProjectCommand::new("Renamed")).unwrap();
    assert_eq!(e.project().name(), "Renamed");
    e.undo().unwrap();
    assert_eq!(e.project().name(), "T");
}

/// A command that violates the contract, to prove the engine contains the damage.
#[derive(Debug)]
struct Broken;
impl Command for Broken {
    fn label(&self) -> String {
        "broken".into()
    }
    fn apply(&mut self, _: &mut Project) -> Result<(), CommandError> {
        Ok(())
    }
    fn revert(&mut self, _: &mut Project) -> Result<(), CommandError> {
        Err(CommandError::MissingEntity {
            kind: "x",
            id: Id(1),
        })
    }
}

#[test]
fn failing_revert_clears_history_instead_of_continuing() {
    let mut e = engine();
    place(&mut e, "R1");
    e.execute(Broken).unwrap();
    assert!(matches!(e.undo(), Err(CommandError::HistoryCorrupted(_))));
    assert!(e.undo_history().is_empty() && e.redo_history().is_empty());
    assert!(e.is_dirty());
}

#[test]
fn composite_apply_failure_reverts_applied_children() {
    let mut project = Project::new("C").unwrap();
    let children: Vec<Box<dyn Command>> = vec![
        Box::new(place_component(ROOT_SHEET, Id(1), comp("R1", 0))),
        Box::new(DeleteViaCommand::new(Id(999))),
    ];
    let mut c = CompositeCommand::new("c", children);
    assert!(c.apply(&mut project).is_err());
    assert_eq!(project.schematic().object_count(), 0);
}

#[derive(Debug, Clone)]
enum Op {
    Place(i64),
    Move(usize, i64),
    Delete(usize),
    Track(i64),
    Via,
    Rename(u8),
    Undo,
    Redo,
    Txn(Vec<Op>),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let leaf = prop_oneof![
        (-1000i64..1000).prop_map(Op::Place),
        (0usize..8, -1000i64..1000).prop_map(|(i, x)| Op::Move(i, x)),
        (0usize..8).prop_map(Op::Delete),
        (1i64..1000).prop_map(Op::Track),
        Just(Op::Via),
        any::<u8>().prop_map(Op::Rename),
        Just(Op::Undo),
        Just(Op::Redo),
    ];
    leaf.prop_recursive(2, 12, 4, |inner| {
        proptest::collection::vec(inner, 0..4).prop_map(Op::Txn)
    })
}

fn run(e: &mut CommandEngine, op: &Op) {
    let pick = |e: &CommandEngine, i: usize| {
        let ids: Vec<Id> = e.project().schematic().sheets()[&ROOT_SHEET]
            .objects
            .keys()
            .copied()
            .collect();
        (!ids.is_empty()).then(|| ids[i % ids.len()])
    };
    match op {
        Op::Place(x) => {
            let n = objects(e);
            let id = e.new_id();
            let _ = e.execute(place_component(ROOT_SHEET, id, comp(&format!("R{n}"), *x)));
        }
        Op::Move(i, x) => {
            if let Some(id) = pick(e, *i) {
                let _ = move_to(e, id, Point::new(*x, *x));
            }
        }
        Op::Delete(i) => {
            if let Some(id) = pick(e, *i) {
                let _ = delete(e, id);
            }
        }
        Op::Track(x) => {
            let _ = e.execute(CreateTrackCommand::new(track(*x)));
        }
        Op::Via => {
            let _ = e.execute(CreateViaCommand::new(Via {
                position: Point::ORIGIN,
                diameter: 600_000,
                drill: 300_000,
            }));
        }
        Op::Rename(n) => {
            let _ = e.execute(RenameProjectCommand::new(format!("P{n}")));
        }
        Op::Undo => {
            let _ = e.undo();
        }
        Op::Redo => {
            let _ = e.redo();
        }
        Op::Txn(ops) => {
            // Undo/redo inside a transaction is refused by design, which is itself part of the property.
            let _ = e.transaction("txn", |e| {
                for o in ops {
                    run(e, o);
                }
                Ok::<(), CommandError>(())
            });
        }
    }
}

proptest! {
    /// Undoing everything restores the initial project; redoing everything restores the final one.
    #[test]
    fn undo_all_then_redo_all_is_lossless(ops in proptest::collection::vec(op_strategy(), 0..40)) {
        let mut e = engine();
        let initial = e.project().clone();
        for op in &ops {
            run(&mut e, op);
        }
        let last = e.project().clone();
        let mut steps = 0;
        while e.undo().unwrap().is_some() { steps += 1; }
        prop_assert_eq!(e.project(), &initial);
        prop_assert!(!e.is_dirty());
        for _ in 0..steps { prop_assert!(e.redo().unwrap().is_some()); }
        prop_assert_eq!(e.project(), &last);
    }
}
