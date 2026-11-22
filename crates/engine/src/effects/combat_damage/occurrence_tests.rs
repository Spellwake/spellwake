//! Prepared combat-owner units; these are not ordinary/reference trajectory evidence.
use super::*;
use crate::{
    occurrence::{RulesRecord, TriggerError},
    program::ExecutableBody,
    turn::{Step, TurnStart},
};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);

fn object(state: &EffectWindow, id: u64) -> ObjectId {
    state.window.cards().current_identity(CardId(id)).unwrap()
}

fn prepared(power: i64, shield: bool) -> (EffectWindow, TurnScheduler) {
    let mut cards = [(1, A, power), (2, B, 1), (3, B, 1)]
        .into_iter()
        .map(|(id, owner, power)| SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program: Program::creature(
                None,
                power.into(),
                10.into(),
                std::collections::BTreeSet::new(),
            ),
        })
        .collect::<Vec<_>>();
    if shield {
        cards.push(SeedCard {
            card: CardId(4),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::CreatePreventionShield { amount: 1.into() }],
                ),
            ),
        });
    }
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
            vec![B, A],
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
    if shield {
        state = state.settle_checks(state.revision()).unwrap();
        state = state
            .cast(
                state.revision(),
                A,
                object(&state, 4),
                TargetRef::Object(object(&state, 1)),
            )
            .unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        for player in [A, B] {
            state = state.pass(state.revision(), player).unwrap();
            if matches!(
                state.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ) {
                state = state.settle_checks(state.revision()).unwrap();
            }
        }
        state = state.resolve_top(state.revision()).unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        for player in [A, B] {
            state = state.pass(state.revision(), player).unwrap();
            if matches!(
                state.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ) {
                state = state.settle_checks(state.revision()).unwrap();
            }
        }
    }
    state.window = state
        .window
        .enter_turn_step(state.window.revision(), A, false)
        .unwrap();
    let attacker =
        Combatant::from_object(state.window.cards().get(object(&state, 1)).unwrap()).unwrap();
    let blockers = [2, 3]
        .into_iter()
        .map(|id| {
            (
                Combatant::from_object(state.window.cards().get(object(&state, id)).unwrap())
                    .unwrap(),
                attacker.object(),
            )
        })
        .collect();
    let mut combat = CombatState::new(vec![(attacker, B)]).unwrap();
    combat.declare_blocks(B, blockers).unwrap();
    state.combat = Some(combat);
    let turn = TurnScheduler::new(
        TurnStart {
            number: 3,
            step: Step::CombatDamage,
        },
        false,
    )
    .unwrap();
    (state, turn)
}

fn assignment_frame(
    state: &EffectWindow,
    turn: &TurnScheduler,
) -> (EffectWindow, DamageAssignmentsDecision) {
    let (next, result, append) = state
        .begin_combat_damage(state.revision(), turn)
        .unwrap()
        .into_parts();
    assert!(append.records.is_empty());
    let RuleResult::Pending {
        frame: EffectDecision::Combat(CombatDecision::DamageAssignments(frame)),
        ..
    } = result
    else {
        panic!("active multi-recipient assignment")
    };
    (next, frame)
}

fn allocation(state: &EffectWindow) -> Vec<CombatDamageAssignment> {
    [2, 3]
        .into_iter()
        .map(|id| CombatDamageAssignment {
            source: object(state, 1),
            target: DamageRecipient::Creature(object(state, id)),
            amount: 2.into(),
        })
        .collect()
}

fn events(append: &RulesJournal) -> Vec<&crate::occurrence::RulesEvent> {
    append
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

fn assignment_actors(append: &RulesJournal) -> Vec<(SeatId, AssignmentOrigin)> {
    events(append)
        .into_iter()
        .filter_map(|event| match event.cause {
            EventCause::CombatDamageAssigned { actor, origin, .. } => Some((actor, origin)),
            _ => None,
        })
        .collect()
}

#[test]
fn actor_allocation_rejects_atomically_then_precedes_actual_forced_later_actor_and_damage() {
    let (state, turn) = prepared(4, false);
    let (pending, frame) = assignment_frame(&state, &turn);
    let saved = pending.clone();
    let mut invalid = allocation(&pending);
    invalid[1].amount = 1.into();
    assert_eq!(
        pending.resume_combat_damage_assignments(pending.revision(), &frame, A, invalid, &turn),
        Err(EffectError::Combat(CombatError::IncorrectDamageTotal))
    );
    assert_eq!(pending, saved);
    let window_revision = pending.window.revision();
    let card_revision = pending.window.cards().revision();
    let (next, result, append) = pending
        .resume_combat_damage_assignments(
            pending.revision(),
            &frame,
            A,
            allocation(&pending),
            &turn,
        )
        .unwrap()
        .into_parts();
    assert_eq!(
        assignment_actors(&append),
        vec![
            (A, AssignmentOrigin::PlayerReply),
            (B, AssignmentOrigin::Forced)
        ]
    );
    let actual = events(&append);
    for event in actual.iter().take(2) {
        assert_eq!(event.occurrences.len(), 2);
        assert!(
            event
                .occurrences
                .iter()
                .all(|row| matches!(row.value, RulesOccurrence::CombatDamageAssigned(_)))
        );
    }
    let RuleResult::Complete {
        journal,
        assignments,
        ..
    } = result
    else {
        panic!("shared damage complete")
    };
    assert_eq!(journal.batches.len(), 1);
    assert_eq!(journal.applied.len(), 4);
    assert_eq!(assignments.len(), 2);
    assert_eq!(
        next.creature(object(&next, 1)).unwrap().unwrap().damage,
        2.into()
    );
    assert_eq!(
        next.creature(object(&next, 2)).unwrap().unwrap().damage,
        2.into()
    );
    assert_eq!(
        next.creature(object(&next, 3)).unwrap().unwrap().damage,
        2.into()
    );
    assert_eq!(next.window.revision(), window_revision);
    assert_eq!(next.window.cards().revision(), card_revision);
    assert_eq!(next.revision(), pending.revision() + 1);
}

#[test]
fn assignment_prefix_survives_damage_allocation_pending_and_reply_emits_only_suffix() {
    let (state, turn) = prepared(4, true);
    let (pending, frame) = assignment_frame(&state, &turn);
    let (accepted, result, append) = pending
        .resume_combat_damage_assignments(
            pending.revision(),
            &frame,
            A,
            allocation(&pending),
            &turn,
        )
        .unwrap()
        .into_parts();
    assert_eq!(
        assignment_actors(&append),
        vec![
            (A, AssignmentOrigin::PlayerReply),
            (B, AssignmentOrigin::Forced)
        ]
    );
    let RuleResult::Pending {
        frame: EffectDecision::Damage(DamageDecision::Allocation(frame)),
        journal,
        ..
    } = result
    else {
        panic!("one shield must allocate among two actual blocker sources")
    };
    assert!(journal.applied.is_empty());
    assert_eq!(
        accepted
            .creature(object(&accepted, 1))
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    let saved = accepted.clone();
    let decision = EffectDecision::Damage(DamageDecision::Allocation(frame.clone()));
    assert!(
        accepted
            .resume_rule(
                accepted.revision(),
                &decision,
                A,
                DecisionReply::AllocatePrevention(vec![]),
                Some(&turn)
            )
            .is_err()
    );
    assert_eq!(accepted, saved);
    let (_, result, suffix) = accepted
        .resume_rule(
            accepted.revision(),
            &decision,
            A,
            DecisionReply::AllocatePrevention(vec![DamageAllocation {
                attempt: frame.eligible()[0].attempt(),
                amount: 1.into(),
            }]),
            Some(&turn),
        )
        .unwrap()
        .into_parts();
    assert!(matches!(result, RuleResult::Complete { .. }));
    assert!(assignment_actors(&suffix).is_empty());
}

#[test]
fn late_assignment_publication_failure_retains_the_whole_accepted_choice() {
    let (state, turn) = prepared(4, false);
    let (mut pending, frame) = assignment_frame(&state, &turn);
    pending.triggers.set_test_next_event(u64::MAX - 1);
    let saved = pending.clone();
    assert_eq!(
        pending.resume_combat_damage_assignments(
            pending.revision(),
            &frame,
            A,
            allocation(&pending),
            &turn,
        ),
        Err(EffectError::Trigger(TriggerError::EventIdentityExhausted))
    );
    assert_eq!(pending, saved);
    assert!(matches!(pending.pending_work, Some(PendingWork::Combat(_))));
}

#[test]
fn zero_power_or_no_recipients_complete_without_assignment_or_batch() {
    for power in [0, 4] {
        let (mut state, turn) = prepared(power, false);
        if power == 0 {
            let attacker =
                Combatant::from_object(state.window.cards().get(object(&state, 1)).unwrap())
                    .unwrap();
            let mut combat = CombatState::new(vec![(attacker, B)]).unwrap();
            combat.declare_blocks(B, vec![]).unwrap();
            state.combat = Some(combat);
        } else {
            // Trusted membership unit: opposite departures leave this positive
            // attacker Blocked, with no current recipients or blocker sources.
            state
                .combat
                .as_mut()
                .unwrap()
                .reconcile(|combatant, _| combatant.controller() == A);
        }
        let (_, result, append) = state
            .begin_combat_damage(state.revision(), &turn)
            .unwrap()
            .into_parts();
        assert!(append.records.is_empty());
        let RuleResult::Complete {
            assignments,
            journal,
            ..
        } = result
        else {
            panic!("empty damage work")
        };
        assert!(assignments.is_empty());
        assert!(journal.batches.is_empty());
        assert!(journal.applied.is_empty());
    }
}
