//! Actual adoptable departures, with populated registries and surviving play.
mod checks_cleanup;
mod checks_legend;
mod checks_order;
mod checks_target;
mod chooser_replacement;
mod mana_color;
mod pending_optional;
mod pending_payment;
mod resolution_cursor;
mod selection;
mod token_entry;
use super::*;
use crate::{
    characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
    occurrence::{RulesOccurrence, ZoneReason},
    priority::PriorityPhase,
    stack_window::WindowPhase,
};

fn apply(state: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = state.clone();
    let issued = input(state, actor, command);
    let candidate = state.advance(issued.clone());
    assert_eq!(candidate, state.advance(issued));
    assert_eq!(*state, saved);
    candidate.unwrap_or_else(|error| {
        panic!(
            "{error:?} at {:?}, {:?}",
            state.turn(),
            state.execution().window().phase()
        )
    })
}
fn holder(state: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = state.execution().window().phase()
    else {
        panic!(
            "expected issued priority, got {:?}",
            state.execution().window().phase()
        );
    };
    holder
}
fn cast_and_resolve(mut state: GameState, caster: SeatId, id: u64, target: ObjectId) -> GameState {
    while holder(&state) != caster {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        caster,
        GameCommand::Cast {
            spell: object(&state, id),
            target: TargetRef::Object(target),
        },
    )
    .into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    state
}
fn creature() -> Program {
    Program::creature(None, 2.into(), 5.into(), BTreeSet::new())
}
fn control_spell(id: u64, owner: SeatId) -> SeedCard {
    card(
        id,
        owner,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::NoStatedDuration,
                }],
            ),
        ),
    )
}

#[test]
fn actual_departure_removes_owned_objects_exiles_default_control_and_keeps_survivors_playing() {
    let cards = vec![
        card(1, B, ObjectDestination::LibraryTop, creature()),
        card(2, B, ObjectDestination::Hand, creature()),
        card(3, B, ObjectDestination::Graveyard, creature()),
        card(
            4,
            B,
            ObjectDestination::Battlefield { controller: A },
            creature(),
        ),
        card(5, B, ObjectDestination::Exile, creature()),
        card(6, B, ObjectDestination::Command, creature()),
        card(
            7,
            C,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
        card(
            8,
            A,
            ObjectDestination::Battlefield { controller: C },
            creature(),
        ),
    ];
    let state = root(
        cards,
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let preserved = state
        .execution()
        .window()
        .cards()
        .get(object(&state, 8))
        .unwrap()
        .clone();
    let old_exile = object(&state, 7);
    let candidate = apply(&state, B, GameCommand::Concede);
    let RulesRecord::PlayersDeparted {
        owned_objects,
        ended_control,
        ..
    } = &candidate.rules_journal().records[0]
    else {
        panic!("missing departure receipt")
    };
    assert_eq!(owned_objects.len(), 6);
    assert!(ended_control.is_empty());
    assert!(
        candidate
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record,
        RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(&value.value,
            RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::PlayerDeparture
                && change.before.object.id() == old_exile))))
    );
    let next = candidate.into_state();
    assert_eq!(next.outcome(), None);
    assert_eq!(next.original_participants(), &[A, B, C]);
    assert_eq!(next.execution().window().participants(), &[A, C]);
    assert_eq!(
        next.execution().window().cards().get(preserved.id()),
        Some(&preserved)
    );
    assert_eq!(
        next.execution()
            .window()
            .cards()
            .get(object(&next, 7))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_ne!(object(&next, 7), old_exile);
    for id in 1..=6 {
        assert!(
            next.execution()
                .window()
                .cards()
                .current_identity(CardId(id))
                .is_none()
        );
    }
    assert_eq!(holder(&next), A);
    assert_eq!(
        next.advance(input(&next, B, GameCommand::Pass)),
        Err(GameError::UnknownActor)
    );
    let passed = apply(&next, A, GameCommand::Pass).into_state();
    assert_eq!(holder(&passed), C);
}

#[test]
fn actual_departure_ends_masked_control_and_preserves_independent_survivor_control() {
    let state = root(
        vec![
            control_spell(1, B),
            control_spell(2, B),
            control_spell(3, C),
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            card(
                11,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let first = object(&state, 10);
    let second = object(&state, 11);
    let state = cast_and_resolve(state, B, 1, first);
    let state = cast_and_resolve(state, B, 2, second);
    let state = cast_and_resolve(state, C, 3, second);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(first)
            .unwrap()
            .controller(),
        Some(B)
    );
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .controller(),
        Some(C)
    );
    let candidate = apply(&state, B, GameCommand::Concede);
    let RulesRecord::PlayersDeparted { ended_control, .. } = &candidate.rules_journal().records[0]
    else {
        panic!("missing receipt")
    };
    assert_eq!(
        ended_control.len(),
        2,
        "masked departed-recipient record also ends"
    );
    assert!(candidate.rules_journal().records.iter().any(|record| matches!(record,
        RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(&value.value,
            RulesOccurrence::ControllerChanged(change) if change.before.id() == first
                && change.before.controller() == Some(B) && change.after.controller() == Some(A))))));
    let next = candidate.into_state();
    assert_eq!(
        next.execution()
            .window()
            .cards()
            .get(first)
            .unwrap()
            .controller(),
        Some(A)
    );
    assert_eq!(
        next.execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .controller(),
        Some(C)
    );
    assert_eq!(next.execution().window().turn_owner(), A);
}

#[test]
fn later_control_expiry_exiles_departed_default_instead_of_blocking_cleanup() {
    let spell = card(
        1,
        C,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
    );
    let state = root(
        vec![
            spell,
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
        &[20, 20, 20],
        Some(Step::PostcombatMain),
        vec![A, B, C],
    );
    let target = object(&state, 10);
    let state = cast_and_resolve(state, C, 1, target);
    let mut state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .controller(),
        Some(C)
    );
    let mut exiled = false;
    for _ in 0..4 {
        let candidate = apply(&state, holder(&state), GameCommand::Pass);
        exiled |= candidate.rules_journal().records.iter().any(|record| matches!(record,
            RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(&value.value,
                RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::DefaultControllerDeparted
                    && change.before.object.id() == target))));
        state = candidate.into_state();
    }
    assert!(exiled);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 10))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(state.outcome(), None);
    assert_eq!(state.execution().window().turn_owner(), C);
}

#[test]
fn owned_leaving_game_triggers_survivors_without_inventing_a_zone_or_death() {
    let mut watcher = Program::artifact(None, vec![]);
    for (id, condition) in [
        (
            1,
            OrdinaryCondition::LeavesBattlefield(crate::program::ObjectRelation::AnyCreature),
        ),
        (
            2,
            OrdinaryCondition::Dies(crate::program::ObjectRelation::AnyCreature),
        ),
    ] {
        watcher.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
    }
    let state = root(
        vec![
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            ),
            card(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let candidate = apply(&state, B, GameCommand::Concede);
    let discoveries: Vec<_> = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::TriggerDiscovered(value) = record {
                Some(value)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].definition, TriggeredAbilityId(1));
    assert_eq!(discoveries[0].controller, A);
    assert!(matches!(&discoveries[0].causes.members()[0].related,
        crate::occurrence::TriggerRelated::LeftGame { before } if before.object.owner() == B));
    let mut state = candidate.into_state();
    assert_eq!(state.execution().window().entries().len(), 1);
    state = apply(&state, A, GameCommand::Pass).into_state();
    state = apply(&state, C, GameCommand::Pass).into_state();
    assert_eq!(state.execution().life_total(A), Some(&21.into()));
}

#[test]
fn whole_mixed_control_record_ends_but_independent_effect_from_same_entry_survives() {
    use crate::characteristics::PowerToughness;
    let spell = card(
        1,
        B,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![
                            ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                            ResolvedPartSpec::SetPT(PowerToughness {
                                power: 10.into(),
                                toughness: 10.into(),
                            }),
                        ]),
                        duration: ResolvedDuration::NoStatedDuration,
                    },
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(
                            PowerToughness {
                                power: 1.into(),
                                toughness: 2.into(),
                            },
                        )]),
                        duration: ResolvedDuration::NoStatedDuration,
                    },
                ],
            ),
        ),
    );
    let state = root(
        vec![
            spell,
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let target = object(&state, 10);
    let state = cast_and_resolve(state, B, 1, target);
    assert_eq!(
        state
            .execution()
            .power_toughness(target)
            .unwrap()
            .unwrap()
            .derived,
        PowerToughness {
            power: 11.into(),
            toughness: 12.into()
        }
    );
    let candidate = apply(&state, B, GameCommand::Concede);
    let RulesRecord::PlayersDeparted { ended_control, .. } = &candidate.rules_journal().records[0]
    else {
        panic!("missing receipt")
    };
    assert_eq!(ended_control.len(), 1);
    assert_eq!(ended_control[0].parts.0.len(), 2);
    assert_eq!(
        candidate
            .state()
            .execution()
            .power_toughness(target)
            .unwrap()
            .unwrap()
            .derived,
        PowerToughness {
            power: 3.into(),
            toughness: 7.into()
        }
    );
}

#[test]
fn successive_departures_exile_after_control_returns_to_an_earlier_departed_default() {
    let d = SeatId(77);
    let state = root(
        vec![
            control_spell(1, C),
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
        &[20, 20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C, d],
    );
    let target = object(&state, 10);
    let state = cast_and_resolve(state, C, 1, target);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .controller(),
        Some(C)
    );
    let state = apply(&state, C, GameCommand::Concede).into_state();
    assert_eq!(state.execution().window().participants(), &[A, d]);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 10))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(state.original_participants(), &[A, B, C, d]);
}

#[test]
fn survivor_spell_resolves_and_restarts_priority_after_the_active_owner_leaves() {
    let mut state = root(
        vec![
            card(
                1,
                C,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: InstructionAmount::Literal(1.into()),
                        }],
                    ),
                ),
            ),
            card(2, C, ObjectDestination::LibraryTop, creature()),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    for actor in [A, B] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        C,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    state = apply(&state, A, GameCommand::Concede).into_state();
    assert_eq!(state.execution().window().active_player(), None);
    assert_eq!(holder(&state), C);
    for actor in [C, B] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert_eq!(holder(&state), B);
    assert_eq!(state.execution().window().active_player(), None);
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(C)
    );
}

#[test]
fn departing_stack_spell_is_removed_without_erasing_the_surviving_lower_spell() {
    let spell = || {
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: InstructionAmount::Literal(1.into()),
                }],
            ),
        )
    };
    let mut state = root(
        vec![
            card(1, C, ObjectDestination::Hand, spell()),
            card(2, B, ObjectDestination::Hand, spell()),
            card(3, C, ObjectDestination::LibraryTop, creature()),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    for actor in [A, B] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        C,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    let retained = state.execution().window().entries()[0].clone();
    for actor in [C, A] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        B,
        GameCommand::CastUntargeted {
            spell: object(&state, 2),
            payment: None,
        },
    )
    .into_state();
    let candidate = apply(&state, B, GameCommand::Concede);
    let mut state = candidate.into_state();
    assert_eq!(state.execution().window().entries(), &[retained]);
    assert_eq!(holder(&state), C);
    for actor in [C, A] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 3))
            .unwrap()
            .zone(),
        ObjectZone::Hand(C)
    );
}

#[test]
fn departed_related_payer_cannot_pay_even_a_free_cost_and_uses_authored_unpaid_branch() {
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::LeavesBattlefield(
            crate::program::ObjectRelation::AnyCreature,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::UniqueRelatedPlayer,
                    cost: ManaCost::free(),
                    paid_destination: 1,
                    declined_destination: 3,
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(100.into()),
                },
                Instruction::Jump { destination: 4 },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
            ],
        ),
    });
    let state = root(
        vec![
            card(
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            ),
            card(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let state = apply(&state, B, GameCommand::Concede).into_state();
    let state = apply(&state, A, GameCommand::Pass).into_state();
    let candidate = apply(&state, C, GameCommand::Pass);
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(
        candidate.state().execution().life_total(A),
        Some(&21.into())
    );
    assert_eq!(candidate.rules_journal().records.iter().filter(|record| matches!(record,
        RulesRecord::ResolutionPaymentOutcome { payer, outcome: crate::effects::ResolutionPaymentOutcome::UnpaidDeparted,
            next: 3, .. } if *payer == B)).count(), 1);
}

#[test]
fn departure_faults_after_owned_removal_or_control_ending_publish_no_prefix() {
    use crate::effects::private_qualification::QualificationFault;
    let state = root(
        vec![
            control_spell(1, B),
            card(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            card(3, B, ObjectDestination::Hand, creature()),
        ],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let target = object(&state, 2);
    let state = cast_and_resolve(state, B, 1, target);
    for fault in [
        QualificationFault::Revision(u64::MAX),
        QualificationFault::WindowRevision(u64::MAX),
        QualificationFault::PriorityRevision(u64::MAX),
        QualificationFault::ObjectRevision(u64::MAX - 1),
        QualificationFault::Event(u64::MAX),
    ] {
        let mut faulted = state.clone();
        faulted.set_test_check_effect_fault(fault);
        let before = faulted.clone();
        let result = faulted.advance(input(&faulted, B, GameCommand::Concede));
        assert!(result.is_err(), "fault {fault:?} unexpectedly succeeded");
        assert_eq!(faulted, before);
        assert_eq!(
            result,
            faulted.advance(input(&faulted, B, GameCommand::Concede))
        );
    }
}

#[test]
fn departed_active_turn_continues_without_active_player_then_next_survivor_begins() {
    for start in [Step::Upkeep, Step::PrecombatMain] {
        let mut state = root(
            vec![
                card(
                    1,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    creature(),
                ),
                card(
                    2,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    creature(),
                ),
                card(
                    3,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                    creature(),
                ),
                card(4, B, ObjectDestination::LibraryTop, creature()),
            ],
            &[20, 20, 20],
            Some(start),
            vec![A, B, C],
        );
        let position = state.turn().unwrap().position();
        state = apply(&state, A, GameCommand::Concede).into_state();
        assert_eq!(
            state.turn().unwrap().position(),
            position,
            "departure does not begin another turn"
        );
        assert_eq!(state.execution().window().active_player(), None);
        assert_eq!(state.execution().window().turn_owner(), A);
        assert_eq!(holder(&state), B);
        let mut absent_step = false;
        let mut finished = false;
        for _ in 0..24 {
            assert!(
                state.pending_decision().is_none(),
                "no departed attack or cleanup chooser"
            );
            let candidate = apply(&state, holder(&state), GameCommand::Pass);
            assert!(
                candidate.draw_journal().events.is_empty(),
                "departed active player does not draw"
            );
            for record in &candidate.rules_journal().records {
                if let RulesRecord::Event(event) = record {
                    for occurrence in &event.occurrences {
                        if let RulesOccurrence::StepBegan { position, active } = occurrence.value
                            && position.number == 1
                        {
                            assert_eq!(active, None);
                            absent_step = true;
                        }
                    }
                }
            }
            state = candidate.into_state();
            if state.turn().unwrap().position().number == 2 {
                finished = true;
                break;
            }
            assert_eq!(state.execution().window().active_player(), None);
        }
        assert!(finished && absent_step);
        assert_eq!(state.execution().window().active_player(), Some(B));
        assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
        assert_eq!(holder(&state), B);
        for actor in [B, C] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        assert_eq!(state.turn().unwrap().position().step, Step::Draw);
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(object(&state, 4))
                .unwrap()
                .zone(),
            ObjectZone::Hand(B)
        );
        assert_eq!(state.original_participants(), &[A, B, C]);
    }
}

#[test]
fn final_multiplayer_concession_concludes_with_all_earlier_departures() {
    for first in [A, B, C] {
        let initial = root(
            vec![],
            &[20, 20, 20],
            Some(Step::PostcombatMain),
            vec![A, B, C],
        );
        let state = apply(&initial, first, GameCommand::Concede).into_state();
        for last in state.execution().window().participants() {
            let winner = *state
                .execution()
                .window()
                .participants()
                .iter()
                .find(|seat| *seat != last)
                .unwrap();
            let candidate = apply(&state, *last, GameCommand::Concede);
            assert_eq!(
                candidate.state().outcome(),
                Some(GameOutcome::Winner(winner))
            );
            let conclusion = candidate.state().conclusion().unwrap();
            assert_eq!(
                conclusion.cause(),
                &GameConclusionCause::ConcededLastSurvivor {
                    player: *last,
                    earlier_departures: BTreeSet::from([first]),
                }
            );
            assert_eq!(
                conclusion.losing_players().collect::<BTreeSet<_>>(),
                BTreeSet::from([first, *last])
            );
            assert!(matches!(
                candidate.rules_journal().records.as_slice(),
                [RulesRecord::GameConcluded { .. }]
            ));
            assert_eq!(
                candidate
                    .state()
                    .advance(input(candidate.state(), winner, GameCommand::Pass)),
                Err(GameError::GameEnded)
            );
        }
    }
}

#[test]
fn response_timeout_checks_waiter_and_basis_before_removing_a_seat() {
    let state = root(
        vec![card(11, A, ObjectDestination::Hand, creature())],
        &[20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    let current = holder(&state);
    let other = [A, B, C].into_iter().find(|p| *p != current).unwrap();
    let before = state.clone();
    assert_eq!(
        state.expire_response(state.basis(), other),
        Err(GameError::ResponsePlayerNotWaiting)
    );
    let candidate = state.expire_response(state.basis(), current).unwrap();
    assert_eq!(
        state.expire_response(state.basis(), current).unwrap(),
        candidate
    );
    assert_eq!(state, before);
    let next = candidate.state();
    assert_eq!(
        next.expire_response(state.basis(), current),
        Err(GameError::StaleRevision)
    );
    assert_eq!(
        next.expire_response(next.basis(), current),
        Err(GameError::UnknownActor)
    );
    assert!(next.outcome().is_none());
    assert_eq!(next.execution().window().participants().len(), 2);
    let after_pass = apply(next, holder(next), GameCommand::Pass).into_state();
    let last_loser = holder(&after_pass);
    let terminal = after_pass
        .expire_response(after_pass.basis(), last_loser)
        .unwrap();
    let result = terminal.state().conclusion().unwrap();
    assert_eq!(
        result.cause(),
        &GameConclusionCause::ResponseExpiredLastSurvivor {
            player: last_loser,
            earlier_departures: BTreeSet::from([current])
        }
    );
    assert_eq!(
        result.losing_players().collect::<BTreeSet<_>>(),
        BTreeSet::from([current, last_loser])
    );
    assert_eq!(
        terminal
            .state()
            .expire_response(terminal.state().basis(), last_loser),
        Err(GameError::GameEnded)
    );
    let pair = root(vec![], &[20, 20], None, vec![A, B]);
    assert_eq!(
        pair.expire_response(pair.basis(), A),
        Err(GameError::UnsupportedResponseExpiryScope)
    );
}

#[test]
fn opening_departure_preserves_surviving_mulligans_and_first_turn() {
    let cards = [A, B, C]
        .into_iter()
        .flat_map(|owner| {
            (0..15).map(move |n| {
                card(
                    100 + u64::from(owner.0) * 100 + n,
                    owner,
                    ObjectDestination::LibraryBottom,
                    creature(),
                )
            })
        })
        .collect();
    let (context, seed) = seed(cards, vec![A, B, C], &[20, 20, 20]);
    let start = GameState::initialize_opening(
        GameId(1046),
        context,
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 300]),
        },
    )
    .unwrap()
    .into_state();
    let respond = |s: &GameState, choice: DecisionReply| {
        let pending = s.pending_decision().unwrap();
        apply(
            s,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        )
        .into_state()
    };
    let keep = |s: &GameState| respond(s, DecisionReply::Mulligan(MulliganChoice::Keep));
    let mull = |s: &GameState| respond(s, DecisionReply::Mulligan(MulliganChoice::Mulligan));
    let expire = |s: &GameState| {
        s.expire_response(s.basis(), s.pending_decision().unwrap().chooser())
            .unwrap()
            .into_state()
    };

    let next = expire(&start);
    assert_eq!(next.pending_decision().unwrap().chooser(), B);
    assert_eq!(next.execution().window().turn_owner(), B);
    assert_eq!(next.original_participants(), &[A, B, C]);
    assert!(
        next.execution()
            .window()
            .cards()
            .iter()
            .all(|object| object.owner() != A)
    );
    let active = keep(&keep(&next));
    assert_eq!(holder(&active), B);
    assert_eq!(active.turn().unwrap().position().number, 1);

    // A and B have both requested a mulligan. C's expiry must apply the
    // surviving requests, including their original multiplayer free mulligan.
    let wait = mull(&mull(&start));
    let survivors = expire(&wait);
    assert_eq!(survivors.pending_decision().unwrap().chooser(), A);
    for player in [A, B] {
        assert_eq!(survivors.opening().unwrap().mulligans_taken(player), 1);
        assert_eq!(survivors.opening().unwrap().charged(player), 0);
    }
    // Get to an actual charged simultaneous bottom round in the original
    // three-seat game. A's accepted bottom stays selected when B times out.
    let round = keep(&wait);
    let bottom = mull(&mull(&round));
    let RuleFrame::Opening(OpeningDecision::Bottom { hand, .. }) =
        bottom.pending_decision().unwrap().frame()
    else {
        panic!("expected charged bottom")
    };
    let selected = hand[0];
    let staged = respond(&bottom, DecisionReply::Bottom(vec![selected]));
    assert_eq!(staged.pending_decision().unwrap().chooser(), B);
    let after = expire(&staged);
    assert_eq!(after.pending_decision().unwrap().chooser(), A);
    assert_eq!(after.opening().unwrap().charged(A), 1);
    assert_eq!(
        after
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.zone() == crate::object_identity::ObjectZone::Hand(A))
            .count(),
        6
    );
    assert!(after.opening().unwrap().kept(C));
}
