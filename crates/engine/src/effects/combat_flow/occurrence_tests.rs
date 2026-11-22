//! Trusted prepared combat-work units, not new ordinary/reference trajectories.
use super::*;
use crate::{
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
    object_identity::ObjectIdentityError,
    occurrence::{RulesRecord, TriggerError, TriggerRelated},
    program::{ExecutableBody, TriggeredAbility, TriggeredAbilityId},
    turn::{Step, TurnStart},
};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(9);

fn creature(id: u64, owner: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program: Program::creature(None, 2.into(), 10.into(), std::collections::BTreeSet::new()),
    }
}

fn prepared(cards: Vec<SeedCard>, step: Step) -> (EffectWindow, TurnScheduler) {
    let mut state = EffectWindow::new({
        // Fixture premise: seeded BF timestamps follow this authored card sequence.
        let initial_battlefield_order = cards
            .iter()
            .filter(|card| {
                matches!(
                    card.destination,
                    crate::object_identity::ObjectDestination::Battlefield { .. }
                )
            })
            .map(|card| card.card)
            .collect();
        GameSeed::with_uniform_life(
            vec![C, A, B],
            A,
            cards,
            initial_battlefield_order,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    state.window = state
        .window
        .enter_turn_step(state.window.revision(), A, false)
        .unwrap();
    state.window = state
        .window
        .record_turn_start(state.window.revision())
        .unwrap();
    let turn = TurnScheduler::new(TurnStart { number: 3, step }, false).unwrap();
    (state, turn)
}

fn object(state: &EffectWindow, card: u64) -> ObjectId {
    state.window.cards().current_identity(CardId(card)).unwrap()
}

fn events(append: &RulesJournal) -> Vec<&crate::occurrence::RulesEvent> {
    append
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

fn attacker_pending(state: &EffectWindow, turn: &TurnScheduler) -> (EffectWindow, CombatDecision) {
    let (next, result, append) = state
        .begin_declare_attackers(state.revision(), turn)
        .unwrap()
        .into_parts();
    assert!(append.records.is_empty());
    let RuleResult::Pending {
        frame: EffectDecision::Combat(frame),
        ..
    } = result
    else {
        panic!("attacker choice required")
    };
    (next, frame)
}

#[test]
fn shared_tap_cut_discovers_distinct_related_objects_before_declaration_membership() {
    let mut observer = SeedCard {
        card: CardId(50),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::artifact(None, vec![]),
    };
    observer.program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(7),
        condition: crate::program::TriggerSpec::becomes_tapped(
            crate::program::ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    let (state, turn) = prepared(
        vec![creature(1, A), creature(2, A), observer],
        Step::DeclareAttackers,
    );
    let (state, frame) = attacker_pending(&state, &turn);
    let first = object(&state, 1);
    let second = object(&state, 2);
    let watcher = object(&state, 50);
    let revision = state.revision();
    let window_revision = state.window.revision();
    let card_revision = state.window.cards().revision();
    let selected = vec![
        AttackDeclaration {
            attacker: second,
            defender: C,
            defending_planeswalker: None,
        },
        AttackDeclaration {
            attacker: first,
            defender: B,
            defending_planeswalker: None,
        },
    ];
    let (next, _, append) = state
        .resume_combat(
            revision,
            &frame,
            A,
            DecisionReply::DeclareAttackers(selected),
            &turn,
        )
        .unwrap()
        .into_parts();
    let actual = events(&append);
    assert_eq!(actual.len(), 2);
    assert_eq!(
        actual[0].cause,
        EventCause::AttackTaps {
            position: turn.position(),
            actor: A
        }
    );
    assert_eq!(actual[0].occurrences.len(), 2);
    for (row, id) in actual[0].occurrences.iter().zip([first, second]) {
        let RulesOccurrence::StatusChange { change, reason } = &row.value else {
            panic!("real status relation")
        };
        assert_eq!(*reason, StatusReason::AttackDeclaration);
        assert_eq!(change.before.id(), id);
        assert_eq!(change.after.id(), id);
        assert_eq!(change.before.tapped(), Some(false));
        assert_eq!(change.after.tapped(), Some(true));
        assert_eq!(row.id.event, actual[0].id);
    }
    assert_ne!(actual[0].id, actual[1].id);
    let RulesOccurrence::AttackersDeclared {
        origin,
        selected,
        membership,
    } = &actual[1].occurrences[0].value
    else {
        panic!("one declaration")
    };
    assert_eq!(*origin, DeclarationOrigin::PlayerReply);
    assert_eq!(
        selected.iter().map(|row| row.attacker).collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(
        membership,
        &next
            .combat
            .as_ref()
            .unwrap()
            .attackers()
            .cloned()
            .collect::<Vec<_>>()
    );
    let waiting: Vec<_> = next.triggers.waiting().values().collect();
    assert_eq!(waiting.len(), 2);
    for (instance, id) in waiting.iter().zip([first, second]) {
        assert_eq!(instance.source.object.id(), watcher);
        assert_eq!(instance.causes.event(), actual[0].id);
        let TriggerRelated::StatusChange(change) = &instance.causes.members()[0].related else {
            panic!("owned related status pair")
        };
        assert_eq!(change.after.id(), id);
        assert_ne!(id, instance.source.object.id());
    }
    assert_eq!(next.revision(), revision + 1);
    assert_eq!(next.window.revision(), window_revision + 1);
    assert_eq!(next.window.cards().revision(), card_revision + 2);
    assert!(state.combat.is_none());
    assert_eq!(
        state.window.cards().get(first).unwrap().tapped(),
        Some(false)
    );
}

#[test]
fn forced_and_explicit_empty_attacks_each_publish_one_empty_declaration() {
    for forced in [true, false] {
        let cards = if forced {
            vec![creature(1, B)]
        } else {
            vec![creature(1, A)]
        };
        let (state, turn) = prepared(cards, Step::DeclareAttackers);
        let progress = if forced {
            state
                .begin_declare_attackers(state.revision(), &turn)
                .unwrap()
        } else {
            let (pending, frame) = attacker_pending(&state, &turn);
            pending
                .resume_combat(
                    pending.revision(),
                    &frame,
                    A,
                    DecisionReply::DeclareAttackers(vec![]),
                    &turn,
                )
                .unwrap()
        };
        let (next, result, append) = progress.into_parts();
        assert!(matches!(result, RuleResult::Complete { .. }));
        let actual = events(&append);
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].occurrences.len(), 1);
        assert_eq!(actual[0].occurrences[0].id.index, 0);
        let RulesOccurrence::AttackersDeclared {
            origin,
            selected,
            membership,
        } = &actual[0].occurrences[0].value
        else {
            panic!("empty declaration")
        };
        assert_eq!(
            *origin,
            if forced {
                DeclarationOrigin::ForcedEmpty
            } else {
                DeclarationOrigin::PlayerReply
            }
        );
        assert!(selected.is_empty());
        assert!(membership.is_empty());
        assert!(next.triggers.waiting().is_empty());
        assert_eq!(
            next.window.cards().revision(),
            state.window.cards().revision()
        );
    }
}

#[test]
fn late_second_tap_or_later_declaration_allocation_failure_keeps_the_accepted_frame() {
    let (state, turn) = prepared(vec![creature(1, A), creature(2, A)], Step::DeclareAttackers);
    let (state, frame) = attacker_pending(&state, &turn);
    let selected = vec![
        AttackDeclaration {
            attacker: object(&state, 1),
            defender: B,
            defending_planeswalker: None,
        },
        AttackDeclaration {
            attacker: object(&state, 2),
            defender: B,
            defending_planeswalker: None,
        },
    ];
    let mut physical = state.clone();
    physical.window.set_test_object_revision(u64::MAX - 1);
    let saved = physical.clone();
    assert_eq!(
        physical.resume_combat(
            physical.revision(),
            &frame,
            A,
            DecisionReply::DeclareAttackers(selected.clone()),
            &turn,
        ),
        Err(EffectError::Window(StackWindowError::Objects(
            ObjectIdentityError::RevisionExhausted
        )))
    );
    assert_eq!(physical, saved);
    let mut publication = state;
    publication.triggers.set_test_next_event(u64::MAX - 1);
    let saved = publication.clone();
    assert_eq!(
        publication.resume_combat(
            publication.revision(),
            &frame,
            A,
            DecisionReply::DeclareAttackers(selected),
            &turn,
        ),
        Err(EffectError::Trigger(TriggerError::EventIdentityExhausted))
    );
    assert_eq!(publication, saved);
}

fn blockers_prepared(with_b: bool) -> (EffectWindow, TurnScheduler) {
    let mut cards = vec![creature(1, A), creature(2, A), creature(5, C)];
    if with_b {
        cards.extend([creature(3, B), creature(4, B)]);
    }
    let (mut state, turn) = prepared(cards, Step::DeclareBlockers);
    let attackers = [(1, B), (2, C)]
        .into_iter()
        .map(|(id, defender)| {
            (
                Combatant::from_object(state.window.cards().get(object(&state, id)).unwrap())
                    .unwrap(),
                defender,
            )
        })
        .collect();
    state.combat = Some(CombatState::new(attackers).unwrap());
    (state, turn)
}

#[test]
fn accepted_defender_event_precedes_later_choice_and_records_one_final_block_status() {
    let (state, turn) = blockers_prepared(true);
    let (pending, result, first_append) = state
        .begin_declare_blockers(state.revision(), &turn)
        .unwrap()
        .into_parts();
    assert!(first_append.records.is_empty());
    let RuleResult::Pending {
        frame: EffectDecision::Combat(frame),
        ..
    } = result
    else {
        panic!("B choice")
    };
    assert_eq!(frame.chooser(), B);
    let target = object(&pending, 1);
    let selected = vec![3, 4]
        .into_iter()
        .map(|id| BlockDeclaration {
            blocker: object(&pending, id),
            attacker: target,
        })
        .collect();
    let (accepted, result, append) = pending
        .resume_combat(
            pending.revision(),
            &frame,
            B,
            DecisionReply::DeclareBlockers(selected),
            &turn,
        )
        .unwrap()
        .into_parts();
    let RuleResult::Pending {
        frame: EffectDecision::Combat(later),
        ..
    } = result
    else {
        panic!("C choice")
    };
    assert_eq!(later.chooser(), C);
    let actual = events(&append);
    assert_eq!(actual.len(), 1);
    let RulesOccurrence::BlockersDeclared {
        statuses,
        membership,
        ..
    } = &actual[0].occurrences[0].value
    else {
        panic!("block relation batch")
    };
    assert_eq!(
        statuses,
        &vec![(target, BlockStatus::NotDeclared, BlockStatus::Blocked)]
    );
    assert_eq!(membership.len(), 2);
    let saved = accepted.clone();
    let bad = BlockDeclaration {
        blocker: object(&accepted, 5),
        attacker: target,
    };
    assert!(
        accepted
            .resume_combat(
                accepted.revision(),
                &later,
                C,
                DecisionReply::DeclareBlockers(vec![bad]),
                &turn
            )
            .is_err()
    );
    assert_eq!(accepted, saved);
    let (complete, _, suffix) = accepted
        .resume_combat(
            accepted.revision(),
            &later,
            C,
            DecisionReply::DeclareBlockers(vec![]),
            &turn,
        )
        .unwrap()
        .into_parts();
    let later_events = events(&suffix);
    assert_eq!(later_events.len(), 1);
    assert_eq!(
        later_events[0].cause,
        EventCause::BlockersDeclared {
            position: turn.position(),
            actor: C
        }
    );
    let RulesOccurrence::BlockersDeclared {
        statuses,
        membership,
        ..
    } = &later_events[0].occurrences[0].value
    else {
        panic!("C declaration")
    };
    assert_eq!(
        statuses,
        &vec![(
            object(&complete, 2),
            BlockStatus::NotDeclared,
            BlockStatus::Unblocked
        )]
    );
    assert_eq!(membership.len(), 2);
    assert_eq!(complete.window.revision(), pending.window.revision());
    assert_eq!(
        complete.window.cards().revision(),
        pending.window.cards().revision()
    );
}

#[test]
fn forced_empty_defender_publishes_before_the_next_defenders_pending_choice() {
    let (state, turn) = blockers_prepared(false);
    let (pending, result, append) = state
        .begin_declare_blockers(state.revision(), &turn)
        .unwrap()
        .into_parts();
    let RuleResult::Pending {
        frame: EffectDecision::Combat(frame),
        ..
    } = result
    else {
        panic!("C choice")
    };
    assert_eq!(frame.chooser(), C);
    let actual = events(&append);
    assert_eq!(actual.len(), 1);
    let RulesOccurrence::BlockersDeclared {
        origin,
        selected,
        membership,
        statuses,
    } = &actual[0].occurrences[0].value
    else {
        panic!("B forced completion")
    };
    assert_eq!(*origin, DeclarationOrigin::ForcedEmpty);
    assert!(selected.is_empty());
    assert!(membership.is_empty());
    assert_eq!(
        statuses,
        &vec![(
            object(&pending, 1),
            BlockStatus::NotDeclared,
            BlockStatus::Unblocked
        )]
    );
}

#[path = "keyword_tests.rs"]
mod keyword_tests;
