//! Actual priority-check trigger ordering and departure normalized originals.
use super::chooser_replacement::{bound, finish_original_stack, four_seat_root, mechanism_step};
use super::*;
use spellwake_engine::program::{
    ObjectRelation, OrdinaryCondition, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn prepare() -> GameState {
    let mut cards = vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            ),
        ),
        card(
            20,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        ),
    ];
    for (id, owner, controller) in [(2, B, A), (3, A, A), (4, C, C), (5, C, C), (6, D, D)] {
        let mut program = Program::artifact(None, vec![]);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::LeavesBattlefield(ObjectRelation::AnyCreature),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller },
            program,
        ));
    }
    let mut state = four_seat_root(cards);
    state = mechanism_step(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Object(object(&state, 20)),
        },
    );
    for actor in [A, B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    state
}
pub(super) fn app(source: &GameState, actor: SeatId) -> Uuid {
    bound(source, viewer_a())
        .seats
        .iter()
        .find(|seat| seat.native_seat == actor)
        .unwrap()
        .seat_id
}
pub(super) fn project(view: &mut NativeScope<'_>) -> NativeProjectedSnapshot {
    let snapshot = view
        .project_with_facts(&metadata(), &Display::default())
        .unwrap();
    let facts = snapshot
        .facts(view.source.execution().creature_vocabulary())
        .unwrap();
    snapshot.snapshot().validate(&facts).unwrap();
    let decoded: PlayerSnapshot =
        crate::decode_wire_object(&serde_json::to_vec(snapshot.snapshot()).unwrap()).unwrap();
    decoded.validate(&facts).unwrap();
    capture(snapshot.snapshot());
    snapshot
}
pub(super) fn departure(
    source: &GameState,
    actor: SeatId,
) -> spellwake_engine::game::NativeCandidate {
    let scope = NativeScope::new(source, bound(source, app(source, actor))).unwrap();
    let prepared = scope
        .propose(&command(&scope, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    prepared.candidate().clone()
}
fn depart(source: &GameState, actor: SeatId) -> GameState {
    departure(source, actor).into_state()
}

pub(super) fn order(source: &GameState) -> GameState {
    let actor = source.pending_decision().unwrap().chooser();
    let viewer = app(source, actor);
    let mut scope = NativeScope::new(source, bound(source, viewer)).unwrap();
    let projected = project(&mut scope);
    let descriptor = &projected.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::OrderTriggers);
    assert!(
        matches!(crate::native_game::clocks::waiting(source,&scope.binding).unwrap(),crate::native_game::clocks::Waiting::Decision(seat) if seat==viewer)
    );
    let PlayerDomain::Ordering { objects, .. } = &descriptor.domain else {
        panic!("trigger order")
    };
    let original = command(
        &scope,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: PlayerChoice::Ordering {
                ordered: objects.iter().rev().cloned().collect(),
            },
        },
    );
    let roster = projected
        .snapshot()
        .players
        .iter()
        .map(|seat| seat.seat_id)
        .collect::<Vec<_>>();
    descriptor
        .validate_response(&original, viewer, &roster, &[])
        .unwrap();
    let prepared = scope.propose(&original).unwrap();
    input_codec::verify(&scope, &prepared);
    let observer = if actor == D { A } else { D };
    let mut other = NativeScope::new(source, bound(source, app(source, observer))).unwrap();
    assert!(project(&mut other).snapshot().decisions.is_empty());
    assert!(other.propose(&original).is_err());
    for (handle, reference) in &scope.references {
        if matches!(reference, NativeReference::Trigger(_)) {
            assert!(other.reference(handle).is_err());
        }
    }
    prepared.candidate().state().clone()
}
#[test]
#[ignore = "requires genuine vocabulary; priority Checks trigger-order departure normalized originals"]
fn checks_order_departure_originals_keep_source_capture_skip_gone_chooser_and_continue_stack() {
    let source = prepare();
    let old = source.pending_decision().unwrap().id();
    let source = depart(&source, B);
    assert_ne!(source.pending_decision().unwrap().id(), old);
    let source = order(&source);
    assert_eq!(source.pending_decision().unwrap().chooser(), C);
    let source = depart(&source, C);
    assert!(source.pending_decision().is_none());
    assert_eq!(source.execution().window().entries().len(), 3);
    let source = finish_original_stack(source);
    assert_eq!(source.execution().life_total(A), Some(&22.into()));
    assert_eq!(source.execution().life_total(D), Some(&21.into()));
    let mut scope = NativeScope::new(&source, bound(&source, viewer_a())).unwrap();
    project(&mut scope);
}

#[test]
#[ignore = "requires genuine vocabulary; departed priority recipient and surviving trigger chooser originals"]
fn departed_priority_recipient_originals_leave_surviving_chooser_and_real_singleton_placement() {
    let source = prepare();
    let source = depart(&source, A);
    assert_eq!(source.pending_decision().unwrap().chooser(), C);
    let source = order(&source);
    assert!(source.pending_decision().is_none());
    assert_eq!(source.execution().window().entries().len(), 3);
    let source = finish_original_stack(source);
    assert_eq!(source.execution().life_total(A), Some(&20.into()));
    assert_eq!(source.execution().life_total(C), Some(&22.into()));
    assert_eq!(source.execution().life_total(D), Some(&21.into()));
    let mut scope = NativeScope::new(&source, bound(&source, app(&source, C))).unwrap();
    project(&mut scope);
}
