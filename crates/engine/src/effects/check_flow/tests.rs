//! Private trusted prepared-boundary and operational-fault probes. These worlds
//! do not claim that lethal marks or opposing counters survived an earlier legal
//! priority check, or that the raw window copy setup executed an authored spell.
use super::*;
use crate::program::{
    BodyTarget, ExecutableBody, ObjectRelation, OrdinaryCondition, PlayerRelation,
    TriggeredAbility, TriggeredAbilityId,
};
use crate::{
    activated::SourceCost,
    object_identity::ObjectIdentityError,
    priority::PriorityError,
    turn::{Step, TurnStart},
};

const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
const C: SeatId = SeatId(9);

fn object(state: &EffectWindow, card: u64) -> ObjectId {
    state.window.cards().current_identity(CardId(card)).unwrap()
}

fn creature(card: u64, owner: SeatId, controller: SeatId, toughness: i64) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::creature(
            None,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn artifact(card: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program: Program::artifact(None, vec![]),
    }
}

fn calendar(step: Step) -> TurnScheduler {
    TurnScheduler::new(TurnStart { number: 4, step }, false).unwrap()
}

fn pending(progress: RuleProgress) -> (EffectWindow, CheckDecision, CheckJournal) {
    let (
        state,
        RuleResult::Pending {
            frame: EffectDecision::Checks(frame),
            checks,
            ..
        },
        _,
    ) = progress.into_parts()
    else {
        panic!("expected an actual pending check choice")
    };
    (state, frame, checks)
}

fn complete(progress: RuleProgress) -> (EffectWindow, CheckCompletion, CheckJournal) {
    let (
        state,
        RuleResult::Complete {
            completion: RuleCompletion::Checks(completion),
            checks,
            ..
        },
        _,
    ) = progress.into_parts()
    else {
        panic!("expected actual check completion")
    };
    (state, completion, checks)
}

fn resume(
    state: &EffectWindow,
    frame: &CheckDecision,
    actor: SeatId,
    reply: DecisionReply,
    turn: Option<&TurnScheduler>,
) -> Result<RuleProgress, EffectError> {
    state.resume_rule(
        state.revision(),
        &EffectDecision::Checks(frame.clone()),
        actor,
        reply,
        turn,
    )
}

fn raw_pass_round(mut window: StackWindow) -> StackWindow {
    window = window.finish_checks(window.revision()).unwrap();
    for actor in [A, C, B] {
        window = window.pass(window.revision(), actor).unwrap();
        if matches!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            window = window.finish_checks(window.revision()).unwrap();
        }
    }
    window
}

fn prepared_staged_world(losses: bool) -> EffectWindow {
    let mut cards = vec![
        creature(10, A, B, 2),
        creature(11, A, A, 2),
        creature(20, B, B, 2),
        creature(21, B, A, 2),
        creature(30, C, C, 5),
        artifact(60, A, ObjectDestination::Graveyard),
        artifact(61, B, ObjectDestination::Graveyard),
        artifact(62, C, ObjectDestination::LibraryBottom),
    ];
    for card in [90, 94] {
        cards.push(SeedCard {
            card: CardId(card),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Red),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            ),
        });
    }
    for (card, destination) in [
        (100, ObjectDestination::Battlefield { controller: A }),
        (102, ObjectDestination::Hand),
    ] {
        cards.push(SeedCard {
            card: CardId(card),
            owner: A,
            destination,
            program: Program::land(
                std::collections::BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(7),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 2.into())])),
                }],
            ),
        });
    }
    cards.push(SeedCard {
        card: CardId(101),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(8),
                mana_cost: ManaCost::one(ManaType::Red),
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            }],
        ),
    });
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
            vec![B, A, C],
            A,
            cards,
            initial_battlefield_order,
            vec![
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Red,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: B,
                    mana_type: ManaType::Blue,
                    amount: 5.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();

    // Trusted window-only setup creates and completes a real noncard copy. All
    // creatures are healthy here; adversarial pending SBA work is added below.
    let mut window = state.window.finish_checks(state.window.revision()).unwrap();
    window = window
        .record_card_spell(window.revision(), A, object(&state, 90))
        .unwrap();
    window = raw_pass_round(window);
    let original = window.resolving().unwrap().id();
    window = window
        .record_spell_copy(window.revision(), original, A, vec![])
        .unwrap();
    let copied = window.entries().last().unwrap().id();
    window = window
        .finish_resolution(
            window.revision(),
            original,
            Some(ObjectDestination::Graveyard),
        )
        .unwrap();
    window = raw_pass_round(window);
    assert_eq!(window.resolving().unwrap().id(), copied);
    state.window = window
        .finish_resolution(
            window.revision(),
            copied,
            Some(ObjectDestination::Graveyard),
        )
        .unwrap();
    assert_eq!(
        state.window.spell_copies()[&copied].zone,
        ObjectZone::Graveyard(A)
    );
    for card in [10, 11, 20, 21] {
        state.damage.insert(object(&state, card), 2.into());
    }
    state
        .characteristics
        .place_counters(
            object(&state, 30),
            &PowerToughnessCounters {
                plus_one: 3.into(),
                minus_one: 2.into(),
            },
        )
        .unwrap();
    if losses {
        state.failed_draws.insert(B);
        state
            .life
            .apply_losses(&BTreeMap::from([(C, 20.into())]))
            .unwrap();
    }
    state
}

fn accepted_first_owner(
    losses: bool,
) -> (EffectWindow, CheckDecision, CheckDecision, Vec<ObjectId>) {
    let state = prepared_staged_world(losses);
    let (state, first, _) = pending(state.begin_checks(state.revision()).unwrap());
    assert_eq!(first.chooser(), A);
    let first_order: Vec<_> = graveyard_choice(&first)
        .domain()
        .iter()
        .rev()
        .copied()
        .collect();
    let (state, second, checks) = pending(
        resume(
            &state,
            &first,
            A,
            DecisionReply::OrderGraveyard(first_order.clone()),
            None,
        )
        .unwrap(),
    );
    assert_eq!(second.chooser(), B);
    assert_eq!(checks.choices.len(), 1);
    assert!(checks.moves.is_empty());
    (state, first, second, first_order)
}

fn assert_first_order_retained(state: &EffectWindow, order: &[ObjectId]) {
    let CheckPhase::Actions(actions) = &state.check_process().unwrap().phase else {
        panic!()
    };
    assert_eq!(
        actions
            .movements
            .as_ref()
            .unwrap()
            .moves
            .groups()
            .iter()
            .find(|group| group.owner == A)
            .unwrap()
            .order
            .as_deref(),
        Some(order)
    );
}

fn prepared_cleanup(hand_count: u64) -> (EffectWindow, TurnScheduler, ObjectId) {
    let mut cards = vec![
        creature(1, A, A, 2),
        artifact(2, A, ObjectDestination::Graveyard),
    ];
    cards.extend((200..200 + hand_count).map(|id| artifact(id, A, ObjectDestination::Hand)));
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
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            vec![FloatingMana {
                player: A,
                mana_type: ManaType::Green,
                amount: 4.into(),
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    state = state
        .enter_turn_step(state.revision(), A, false, false)
        .unwrap()
        .into_state();
    let marked = object(&state, 1);
    // Deliberately prepared private cleanup entry: normal earlier priority would
    // already remove this lethally marked creature.
    state.damage.insert(marked, 2.into());
    (state, calendar(Step::Cleanup), marked)
}

#[test]
fn prepared_cleanup_discard_precedes_damage_clear_and_clear_precedes_sba() {
    let (state, turn, marked) = prepared_cleanup(9);
    let before = state.clone();
    let old_pile = state.window.cards().graveyard(A).to_vec();
    let (pending, frame, checks) = pending(state.begin_cleanup(state.revision(), &turn).unwrap());
    assert_eq!(state, before);
    assert!(matches!(frame, CheckDecision::CleanupDiscard(_)));
    assert_eq!(discard_choice(&frame).required(), 2);
    assert_eq!(frame.event().pass(), 0);
    assert_eq!(pending.creature(marked).unwrap().unwrap().damage, 2.into());
    assert_eq!(pending.window.cards(), state.window.cards());
    assert!(checks.moves.is_empty());
    let selected = vec![
        discard_choice(&frame).domain()[7],
        discard_choice(&frame).domain()[1],
    ];
    let (done, completion, checks) = complete(
        resume(
            &pending,
            &frame,
            A,
            DecisionReply::Discard(selected.clone()),
            Some(&turn),
        )
        .unwrap(),
    );
    assert!(!completion.performed());
    assert_eq!(
        completion.boundary(),
        CheckBoundary::Cleanup(turn.position())
    );
    assert_eq!(done.creature(marked).unwrap().unwrap().damage, 0.into());
    assert_eq!(
        done.window.cards().get(marked).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(&done.window.cards().graveyard(A)[2..], old_pile);
    assert_eq!(checks.choices.len(), 1);
    assert_eq!(
        checks.choices[0].choice,
        CheckChoice::CleanupDiscard {
            ordered: selected.clone()
        }
    );
    assert_eq!(checks.moves.len(), 2);
    for (movement, selected) in checks.moves.iter().zip(selected) {
        assert_eq!(movement.cause, CheckMoveCause::CleanupDiscard);
        assert_eq!(movement.before.id(), selected);
        assert_eq!(movement.before.zone(), ObjectZone::Hand(A));
        assert_eq!(movement.after.zone(), ObjectZone::Graveyard(A));
    }
    assert_eq!(done.mana(A, ManaType::Green), 4.into());
    assert_eq!(turn.stage(), TurnStage::Work(TurnWork::Cleanup));
}

#[test]
fn no_excess_prepared_cleanup_clears_lethal_marks_without_fabricating_discard() {
    let (state, turn, marked) = prepared_cleanup(7);
    let (done, completion, checks) =
        complete(state.begin_cleanup(state.revision(), &turn).unwrap());
    assert!(!completion.performed());
    assert_eq!(done.creature(marked).unwrap().unwrap().damage, 0.into());
    assert!(checks.choices.is_empty());
    assert!(checks.moves.is_empty());
    assert_eq!(done.window.cards(), state.window.cards());
}

#[test]
fn whole_prepared_sba_waits_for_every_owner_before_deaths_counters_copy_and_losses() {
    for mixed_death_causes in [false, true] {
        let mut state = prepared_staged_world(true);
        if mixed_death_causes {
            // Trusted private pre-check world, not a public card-update action.
            // Each owner has one unmarked zero-toughness creature and one
            // positive-toughness creature with lethal marked damage.
            for card in [10, 20] {
                let ProgramKind::Permanent(permanent) =
                    &mut std::sync::Arc::make_mut(&mut state.programs)
                        .get_mut(&CardId(card))
                        .unwrap()
                        .kind
                else {
                    panic!()
                };
                permanent.printed_pt.as_mut().unwrap().toughness =
                    crate::characteristics::PrintedStat::Number(0.into());
                let id = object(&state, card);
                state.damage.remove(&id);
                let zero = state.creature(id).unwrap().unwrap();
                assert_eq!(zero.toughness, 0.into());
                assert_eq!(zero.damage, 0.into());
            }
            for card in [11, 21] {
                let lethal = state.creature(object(&state, card)).unwrap().unwrap();
                assert!(lethal.toughness > 0.into());
                assert!(lethal.damage >= lethal.toughness);
            }
        }
        let physical = state.window.cards().clone();
        let survivor = object(&state, 30);
        let (state, first, checks) = pending(state.begin_checks(state.revision()).unwrap());
        assert!(checks.moves.is_empty());
        assert_eq!(first.chooser(), A);
        assert_eq!(
            graveyard_choice(&first).domain(),
            [object(&state, 10), object(&state, 11)]
        );
        let CheckPhase::Actions(actions) = &state.check_process().unwrap().phase else {
            panic!()
        };
        assert_eq!(actions.dead.len(), 4);
        assert_eq!(actions.cancellations.len(), 1);
        assert!(!actions.departed_copies.is_empty());
        assert_eq!(actions.losses, BTreeSet::from([B, C]));
        let first_order: Vec<_> = graveyard_choice(&first)
            .domain()
            .iter()
            .rev()
            .copied()
            .collect();
        let (state, second, checks) = pending(
            resume(
                &state,
                &first,
                A,
                DecisionReply::OrderGraveyard(first_order.clone()),
                None,
            )
            .unwrap(),
        );
        assert_eq!(second.chooser(), B);
        assert_eq!(second.event(), first.event());
        assert_eq!(state.window.cards(), &physical);
        assert_eq!(
            state.power_toughness_counters(survivor),
            Some(PowerToughnessCounters {
                plus_one: 3.into(),
                minus_one: 2.into()
            })
        );
        assert_eq!(state.window.spell_copies().len(), 1);
        assert_eq!(state.failed_draws, BTreeSet::from([B]));
        assert!(state.state_based_losses.is_empty());
        assert!(checks.moves.is_empty());
        assert_first_order_retained(&state, &first_order);
        let accepted = state.clone();
        let second_order: Vec<_> = graveyard_choice(&second)
            .domain()
            .iter()
            .rev()
            .copied()
            .collect();
        let progress = resume(
            &state,
            &second,
            B,
            DecisionReply::OrderGraveyard(second_order.clone()),
            None,
        )
        .unwrap();
        for record in &progress.append().records {
            let RulesRecord::Event(event) = record else {
                continue;
            };
            if !matches!(event.cause, EventCause::StateBasedActions { .. }) {
                continue;
            }
            for occurrence in &event.occurrences {
                let RulesOccurrence::ZoneChange(change) = &occurrence.value else {
                    continue;
                };
                let zero = mixed_death_causes
                    && matches!(
                        change
                            .before
                            .object
                            .card_id()
                            .expect("physical fixture card"),
                        CardId(10 | 20)
                    );
                assert_eq!(
                    change.reason,
                    if zero {
                        ZoneReason::StateBased(
                            spellwake_engine::occurrence::StateBasedMoveReasons::new(
                                spellwake_engine::occurrence::StateBasedMoveReason::ZeroToughness,
                            ),
                        )
                    } else {
                        ZoneReason::StateBased(spellwake_engine::occurrence::StateBasedMoveReasons::new(spellwake_engine::occurrence::StateBasedMoveReason::LethalDestruction))
                    }
                );
            }
        }
        let (done, completion, checks) = complete(progress);
        assert_eq!(state, accepted);
        assert!(completion.performed());
        assert_eq!(done.state_based_losses, BTreeSet::from([B, C]));
        assert!(done.failed_draws.is_empty());
        assert!(done.window.spell_copies().is_empty());
        assert_eq!(
            done.power_toughness_counters(survivor),
            Some(PowerToughnessCounters {
                plus_one: 1.into(),
                minus_one: 0.into()
            })
        );
        assert!(matches!(
            done.window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ));
        assert_eq!(checks.choices.len(), 1);
        assert_eq!(checks.choices[0].player, B);
        assert_eq!(
            checks
                .moves
                .iter()
                .map(|movement| movement.before.id())
                .collect::<Vec<_>>(),
            [first_order, second_order].concat()
        );
        assert!(
            checks
                .moves
                .iter()
                .all(|movement| movement.event == first.event()
                    && movement.cause == CheckMoveCause::StateBasedAction)
        );
        for owner in [A, B] {
            assert_eq!(
                &done.window.cards().graveyard(owner)[2..],
                physical.graveyard(owner)
            );
        }
    }
}

#[test]
fn later_owner_rejections_preserve_accepted_choice_and_the_whole_pre_action_world() {
    let (state, first, second, first_order) = accepted_first_owner(true);
    let saved = state.clone();
    let valid = graveyard_choice(&second).domain().to_vec();
    for (frame, actor, reply, error) in [
        (
            first,
            A,
            DecisionReply::OrderGraveyard(first_order.clone()),
            EffectError::Check(CheckError::WrongFrame),
        ),
        (
            second.clone(),
            A,
            DecisionReply::OrderGraveyard(valid.clone()),
            EffectError::WrongChooser,
        ),
        (
            second.clone(),
            B,
            DecisionReply::Discard(valid.clone()),
            EffectError::WrongDecisionKind,
        ),
        (
            second.clone(),
            B,
            DecisionReply::OrderGraveyard(vec![valid[0], valid[0]]),
            EffectError::Check(CheckError::InvalidGraveyardOrder),
        ),
        (
            second.clone(),
            B,
            DecisionReply::OrderGraveyard(vec![valid[0], first_order[0]]),
            EffectError::Check(CheckError::InvalidGraveyardOrder),
        ),
        (
            second.clone(),
            B,
            DecisionReply::OrderGraveyard(vec![]),
            EffectError::Check(CheckError::InvalidGraveyardOrder),
        ),
    ] {
        assert_eq!(resume(&state, &frame, actor, reply, None), Err(error));
        assert_eq!(state, saved);
        assert_first_order_retained(&state, &first_order);
    }
    assert_eq!(
        state.resume_rule(
            state.revision() - 1,
            &EffectDecision::Checks(second),
            B,
            DecisionReply::OrderGraveyard(valid),
            None
        ),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(state, saved);
}

#[test]
fn cleanup_reconstruction_binds_process_domain_count_actor_and_exact_calendar() {
    let (state, turn, _) = prepared_cleanup(9);
    let (state, frame, _) = pending(state.begin_cleanup(state.revision(), &turn).unwrap());
    let CheckDecision::CleanupDiscard(bound) = &frame else {
        panic!()
    };
    let selected = vec![bound.domain()[0], bound.domain()[1]];
    let mut wrongs = vec![];
    let mut wrong = bound.clone();
    wrong.event.pass += 1;
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.event.started_revision += 1;
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.revision += 1;
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.player = B;
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.required += 1;
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.domain.reverse();
    wrongs.push(wrong);
    let mut wrong = bound.clone();
    wrong.domain.pop();
    wrongs.push(wrong);
    let saved = state.clone();
    for wrong in wrongs {
        assert_eq!(
            resume(
                &state,
                &CheckDecision::CleanupDiscard(wrong),
                A,
                DecisionReply::Discard(selected.clone()),
                Some(&turn)
            ),
            Err(EffectError::Check(CheckError::WrongFrame))
        );
        assert_eq!(state, saved);
    }
    for rows in [
        vec![],
        vec![selected[0]],
        vec![selected[0], selected[0]],
        vec![selected[0], object(&state, 1)],
    ] {
        assert_eq!(
            resume(&state, &frame, A, DecisionReply::Discard(rows), Some(&turn)),
            Err(EffectError::Check(CheckError::InvalidDiscard))
        );
        assert_eq!(state, saved);
    }
    assert_eq!(
        resume(
            &state,
            &frame,
            B,
            DecisionReply::Discard(selected.clone()),
            Some(&turn)
        ),
        Err(EffectError::WrongChooser)
    );
    assert_eq!(
        resume(
            &state,
            &frame,
            A,
            DecisionReply::OrderGraveyard(selected.clone()),
            Some(&turn)
        ),
        Err(EffectError::WrongDecisionKind)
    );
    assert_eq!(
        resume(
            &state,
            &frame,
            A,
            DecisionReply::Discard(selected.clone()),
            None
        ),
        Err(EffectError::WrongPhase)
    );
    let wrong_turn = turn
        .complete_work(WorkCompleted::Cleanup {
            priority_required: true,
        })
        .unwrap()
        .complete_priority(crate::turn::PriorityCompleted::Ordinary)
        .unwrap();
    assert_eq!(
        resume(
            &state,
            &frame,
            A,
            DecisionReply::Discard(selected),
            Some(&wrong_turn)
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(state, saved);
}

#[test]
fn pending_checks_block_lifecycle_actions_cast_copy_and_mana_bypasses() {
    let (state, _, _, _) = accepted_first_owner(false);
    let saved = state.clone();
    let r = state.revision();
    let land = object(&state, 100);
    let target = object(&state, 30);
    let entry = *state.window.spell_copies().keys().next().unwrap();
    let copy = CopyResolution {
        revision: r,
        entry,
        original: entry,
        chooser: A,
        instruction: 0,
    };
    let turn = calendar(Step::Cleanup);
    let entropy = EntropyTape::new(vec![5, 7]);
    let results = [
        state.begin_opening(r).map(|_| ()),
        state.opening_shuffle(r, A, &entropy).map(|_| ()),
        state
            .opening_move_cohort(
                r,
                crate::occurrence::OpeningMovePhase::ChargedBottom,
                &[A],
                &[],
            )
            .map(|_| ()),
        state.draw_cards(r, A, 1.into()).map(|_| ()),
        state.enter_turn_step(r, B, false, true).map(|_| ()),
        state.record_turn_start(r).map(|_| ()),
        state.untap_turn(r).map(|_| ()),
        state.pass(r, A).map(|_| ()),
        state
            .cast(r, A, object(&state, 94), TargetRef::Object(target))
            .map(|_| ()),
        state
            .activate_ability(
                r,
                A,
                object(&state, 101),
                ActivatedAbilityId(8),
                TargetRef::Object(target),
                None,
            )
            .map(|_| ()),
        state
            .begin_mana_activation(
                r,
                A,
                land,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .map(|_| ()),
        state
            .activate_mana(
                r,
                A,
                land,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .map(|_| ()),
        state
            .resume_copy_resolution(r, &copy, A, CopyTargetChoice::Retain)
            .map(|_| ()),
        state.begin_resolution(r).map(|_| ()),
        state.begin_checks(r).map(|_| ()),
        state.begin_cleanup(r, &turn).map(|_| ()),
        state
            .begin_declare_attackers(r, &calendar(Step::DeclareAttackers))
            .map(|_| ()),
        state
            .begin_declare_blockers(r, &calendar(Step::DeclareBlockers))
            .map(|_| ()),
        state
            .begin_combat_damage(r, &calendar(Step::CombatDamage))
            .map(|_| ()),
        state.finish_combat(r).map(|_| ()),
        state
            .play_land(r, A, object(&state, 102), &calendar(Step::PrecombatMain))
            .map(|_| ()),
    ];
    for result in results {
        assert_eq!(result, Err(EffectError::WrongPhase));
        assert_eq!(state, saved);
    }
    let color = ManaColorChoice {
        revision: r,
        source: land,
        ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        chooser: A,
        amount: 2.into(),
        colors: BTreeSet::from([ManaType::Red]),
    };
    assert_eq!(
        state.resume_mana_color(r, &color, A, ManaType::Red),
        Err(EffectError::WrongManaColorFrame)
    );
    assert_eq!(state, saved);
    assert_eq!(state.mana(A, ManaType::Red), 7.into());
    assert_eq!(state.mana(B, ManaType::Blue), 5.into());
    assert_eq!(
        state.window.cards().get(land).unwrap().tapped(),
        Some(false)
    );
    assert_eq!(
        state
            .window
            .cards()
            .get(object(&state, 101))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}

#[test]
fn cleanup_pending_work_blocks_other_mandatory_work_even_with_matching_window_phase() {
    let (state, turn, _) = prepared_cleanup(9);
    let (mut state, _, _) = pending(state.begin_cleanup(state.revision(), &turn).unwrap());
    // Supply an empty private combat world so the damage-work probe cannot fail
    // merely because combat state is absent. The pending check owner still wins.
    state.combat = Some(CombatState::new(vec![]).unwrap());
    let saved = state.clone();
    assert_eq!(state.window.phase(), WindowPhase::TurnWork);
    for result in [
        state.begin_declare_attackers(state.revision(), &calendar(Step::DeclareAttackers)),
        state.begin_declare_blockers(state.revision(), &calendar(Step::DeclareBlockers)),
        state.begin_combat_damage(state.revision(), &calendar(Step::CombatDamage)),
    ] {
        assert_eq!(result, Err(EffectError::WrongPhase));
        assert_eq!(state, saved);
    }
    assert_eq!(
        state.begin_cleanup(state.revision(), &turn),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(state, saved);
}

#[test]
fn cleanup_pass_overflow_after_local_discards_preserves_hand_marks_and_old_pile() {
    let (state, turn, marked) = prepared_cleanup(9);
    let (mut state, _, _) = pending(state.begin_cleanup(state.revision(), &turn).unwrap());
    state.check_process_mut().unwrap().event.pass = u64::MAX;
    let frame = state.current_check_decision().unwrap();
    let selected = discard_choice(&frame).domain()[..2].to_vec();
    let saved = state.clone();
    assert_eq!(
        resume(
            &state,
            &frame,
            A,
            DecisionReply::Discard(selected.clone()),
            Some(&turn)
        ),
        Err(EffectError::Check(CheckError::IterationExhausted))
    );
    assert_eq!(state, saved);
    assert_eq!(state.creature(marked).unwrap().unwrap().damage, 2.into());
    for object in selected {
        assert_eq!(
            state.window.cards().get(object).unwrap().zone(),
            ObjectZone::Hand(A)
        );
    }
}

#[test]
fn late_event_commit_faults_preserve_the_previously_accepted_owner_choice() {
    let (state, _, frame, first_order) = accepted_first_owner(false);
    let reply = graveyard_choice(&frame).domain().to_vec();
    // A successful control proves these exact choices commit all four physical
    // moves, cancellation and departed-copy cessation before granting priority.
    let (done, completion, checks) = complete(
        resume(
            &state,
            &frame,
            B,
            DecisionReply::OrderGraveyard(reply.clone()),
            None,
        )
        .unwrap(),
    );
    assert!(completion.performed());
    assert_eq!(checks.moves.len(), 4);
    assert!(done.window.spell_copies().is_empty());
    assert!(matches!(
        done.window.phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: A })
    ));
    for fault in 0..6 {
        let mut faulted = state.clone();
        let error = match fault {
            0 => {
                faulted.check_process_mut().unwrap().event.pass = u64::MAX;
                EffectError::Check(CheckError::IterationExhausted)
            }
            1 => {
                faulted.window.set_test_object_revision(u64::MAX - 3);
                EffectError::Window(StackWindowError::Objects(
                    ObjectIdentityError::RevisionExhausted,
                ))
            }
            2 => {
                faulted.window.set_test_next_object_identity(u64::MAX - 3);
                EffectError::Window(StackWindowError::Objects(
                    ObjectIdentityError::IdentityExhausted,
                ))
            }
            3 => {
                faulted.window.set_test_revision(u64::MAX - 1);
                EffectError::Window(StackWindowError::RevisionExhausted)
            }
            4 => {
                faulted.window.set_test_priority_revision(u64::MAX);
                EffectError::Window(StackWindowError::Priority(PriorityError::RevisionExhausted))
            }
            5 => {
                faulted.revision = u64::MAX;
                EffectError::Window(StackWindowError::RevisionExhausted)
            }
            _ => unreachable!(),
        };
        rebind_prepared_fault_cut(&mut faulted);
        let exact = faulted.current_check_decision().unwrap();
        let saved = faulted.clone();
        assert_eq!(
            resume(
                &faulted,
                &exact,
                B,
                DecisionReply::OrderGraveyard(reply.clone()),
                None
            ),
            Err(error),
            "fault {fault}"
        );
        assert_eq!(faulted, saved, "fault {fault}");
        assert_first_order_retained(&faulted, &first_order);
        assert!(faulted.state_based_losses.is_empty());
        assert_eq!(faulted.window.spell_copies().len(), 1);
        assert_eq!(faulted.mana(A, ManaType::Red), 7.into());
        assert_eq!(faulted.mana(B, ManaType::Blue), 5.into());
    }
}

#[test]
fn late_finish_checks_window_failure_rolls_back_the_already_moved_local_event() {
    let (mut state, _, _, first_order) = accepted_first_owner(false);
    // Remove only the prepared noncard-copy action on a private local world, then
    // reconstruct its staged plan. One window revision remains for all deaths;
    // the subsequent finish_checks call is the operation that must fail.
    state.window = state
        .window
        .cease_departed_copies(state.window.revision())
        .unwrap();
    let retained = {
        let CheckPhase::Actions(prior) = &state.check_process().unwrap().phase else {
            panic!()
        };
        prior
            .movements
            .as_ref()
            .unwrap()
            .moves
            .groups()
            .iter()
            .find(|g| g.owner == A)
            .unwrap()
            .order
            .clone()
    };
    let actions = state
        .collect_state_actions(state.check_process().unwrap().event)
        .unwrap();
    // Disclosed prepared fixture: install the newly declared whole action cut
    // after its manual copy teardown before calling the stricter shared authorizer.
    state.check_process_mut().unwrap().phase = CheckPhase::Actions(actions.clone());
    let movements = state.determine_state_movements(&actions).unwrap();
    let CheckPhase::Actions(prior) = &mut state.check_process_mut().unwrap().phase else {
        panic!()
    };
    prior.movements = Some(movements);
    prior
        .movements
        .as_mut()
        .unwrap()
        .moves
        .groups_mut()
        .iter_mut()
        .find(|g| g.owner == A)
        .unwrap()
        .order = retained;
    state.window.set_test_revision(u64::MAX - 1);
    rebind_prepared_fault_cut(&mut state);
    let frame = state.current_check_decision().unwrap();
    let saved = state.clone();
    assert_eq!(
        resume(
            &state,
            &frame,
            B,
            DecisionReply::OrderGraveyard(graveyard_choice(&frame).domain().to_vec()),
            None
        ),
        Err(EffectError::Window(StackWindowError::RevisionExhausted))
    );
    assert_eq!(state, saved);
    assert_first_order_retained(&state, &first_order);
    for card in [10, 11, 20, 21] {
        assert_eq!(
            state
                .window
                .cards()
                .get(object(&state, card))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
    }
}

fn add_trigger(seed: &mut SeedCard, condition: OrdinaryCondition) {
    seed.program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::InstructionAmount::Literal(1.into()),
            }],
        ),
    });
}

/// Trusted prepared SBA entry. Actual checks, capture, choices and allocation run;
/// this does not assert that zero-toughness seeds survived an earlier priority.
fn prepared_trigger_cohort(singleton_active: bool) -> EffectWindow {
    let mut first = creature(10, A, A, 0);
    if singleton_active {
        add_trigger(
            &mut first,
            OrdinaryCondition::Dies(ObjectRelation::SelfObject),
        );
    }
    let mut cards = vec![first, creature(20, B, B, 0), creature(30, C, C, 3)];
    for (card, controller) in [(100, A), (101, C), (102, B)] {
        if controller == A && singleton_active {
            continue;
        }
        // Deliberately different owner/controller for A's observing source.
        let mut observer = artifact(card, B, ObjectDestination::Battlefield { controller });
        add_trigger(
            &mut observer,
            OrdinaryCondition::Dies(ObjectRelation::AnyCreature),
        );
        cards.push(observer);
    }
    EffectWindow::new({
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
            vec![B, A, C],
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
    .unwrap()
}

fn trigger_pending(progress: RuleProgress) -> (EffectWindow, TriggerOrder, RulesJournal) {
    let (state, result, append) = progress.into_parts();
    let RuleResult::Pending {
        frame: EffectDecision::Triggers(frame),
        draws,
        ..
    } = result
    else {
        panic!("expected a genuinely issued trigger order");
    };
    assert!(draws.events.is_empty());
    (state, frame, append)
}

fn order_triggers(
    state: &EffectWindow,
    frame: &TriggerOrder,
    ordered: Vec<TriggerInstanceId>,
) -> Result<RuleProgress, EffectError> {
    state.resume_rule(
        state.revision(),
        &EffectDecision::Triggers(frame.clone()),
        frame.chooser(),
        DecisionReply::OrderTriggers(ordered),
        None,
    )
}

fn placements(append: &RulesJournal) -> Vec<TriggerPlacement> {
    append
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn actual_sba_cohort_places_each_owner_before_the_next_apnap_choice_and_preserves_bindings() {
    let original = prepared_trigger_cohort(false);
    let (state, first, initial_append) =
        trigger_pending(original.begin_checks(original.revision()).unwrap());
    assert_eq!(first.chooser(), A);
    assert!(first.instances().windows(2).all(|ids| ids[0] < ids[1]));
    assert_eq!(first.instances().len(), 2);
    assert!(state.window.entries().is_empty());
    assert_eq!(
        initial_append
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::Event(_)))
            .count(),
        1
    );
    let captured = state.triggers.waiting().clone();
    assert_eq!(captured.len(), 6);
    let first_order = first.instances().iter().rev().copied().collect::<Vec<_>>();
    let (state, second, append_a) =
        trigger_pending(order_triggers(&state, &first, first_order.clone()).unwrap());
    assert_eq!(second.chooser(), C);
    assert_eq!(second.event(), first.event());
    assert_eq!(second.wave(), first.wave());
    assert_eq!(state.window.entries().len(), 2);
    assert!(
        state
            .window
            .entries()
            .iter()
            .all(|entry| entry.controller() == A)
    );
    assert_eq!(
        placements(&append_a)
            .iter()
            .map(|p| p.instance)
            .collect::<Vec<_>>(),
        first_order
    );
    assert_eq!(append_a.records.len(), 4);
    for pair in append_a.records.as_chunks::<2>().0 {
        let [
            RulesRecord::TriggerPlacementStarted(started),
            RulesRecord::TriggerPlaced(placed),
        ] = pair
        else {
            panic!("each targetless placement starts and completes before the next instance")
        };
        assert_eq!(started, placed);
    }
    let (state, third, append_c) =
        trigger_pending(order_triggers(&state, &second, second.instances().to_vec()).unwrap());
    assert_eq!(third.chooser(), B);
    assert_eq!(third.wave(), first.wave());
    assert_eq!(state.window.entries().len(), 4);
    let third_order = third.instances().iter().rev().copied().collect::<Vec<_>>();
    let accepted = state.clone();
    let (done, result, append_b) = order_triggers(&state, &third, third_order)
        .unwrap()
        .into_parts();
    assert_eq!(state, accepted);
    let RuleResult::Complete {
        completion: RuleCompletion::Checks(completion),
        draws,
        ..
    } = result
    else {
        panic!()
    };
    assert!(completion.performed());
    assert!(!completion.waiting_triggers());
    assert!(draws.events.is_empty());
    assert!(done.triggers.waiting().is_empty());
    assert!(matches!(
        done.window.phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: A })
    ));
    let all = [
        placements(&append_a),
        placements(&append_c),
        placements(&append_b),
    ]
    .concat();
    assert_eq!(all.len(), 6);
    assert_eq!(
        all.iter().map(|p| p.controller).collect::<Vec<_>>(),
        vec![A, A, C, C, B, B]
    );
    assert_eq!(
        all.iter().map(|p| p.position).collect::<Vec<_>>(),
        vec![0, 1, 0, 1, 0, 1]
    );
    assert_eq!(
        all.iter().map(|p| p.entry).collect::<Vec<_>>(),
        done.window
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>()
    );
    for placed in all {
        let waiting = &captured[&placed.instance];
        assert_eq!(done.stack_body(placed.entry), Some(&waiting.body));
        let binding = done
            .execution_bindings(placed.entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap();
        assert_eq!(binding.instance, waiting.id);
        assert_eq!(binding.definition, waiting.definition);
        assert_eq!(binding.causes, waiting.causes);
        assert_eq!(binding.source, waiting.source);
    }
}

#[test]
fn singleton_departed_source_places_without_a_choice_and_is_visible_to_later_players() {
    let original = prepared_trigger_cohort(true);
    let departed = object(&original, 10);
    let (state, frame, append) =
        trigger_pending(original.begin_checks(original.revision()).unwrap());
    assert_eq!(frame.chooser(), C);
    assert_eq!(state.window.entries().len(), 1);
    assert!(state.window.cards().get(departed).is_none());
    let placed = placements(&append);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].controller, A);
    let binding = state
        .execution_bindings(placed[0].entry)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source.object.id(), departed);
    assert_eq!(binding.source.object.zone(), ObjectZone::Battlefield);
    assert!(
        matches!(state.window.entries()[0].kind(), StackKind::TriggeredAbility { source } if source.id() == departed)
    );
}

#[test]
fn later_trigger_order_rejections_preserve_accepted_placements_and_exact_current_domain() {
    let original = prepared_trigger_cohort(false);
    let (state, first, _) = trigger_pending(original.begin_checks(original.revision()).unwrap());
    let foreign = first.instances()[0];
    let (state, frame, _) =
        trigger_pending(order_triggers(&state, &first, first.instances().to_vec()).unwrap());
    let accepted = state.clone();
    let valid = frame.instances().to_vec();
    for (actor, reply, error) in [
        (
            A,
            DecisionReply::OrderTriggers(valid.clone()),
            EffectError::WrongChooser,
        ),
        (
            C,
            DecisionReply::Discard(vec![]),
            EffectError::WrongDecisionKind,
        ),
        (
            C,
            DecisionReply::OrderTriggers(vec![]),
            EffectError::Trigger(TriggerError::InvalidTriggerOrder),
        ),
        (
            C,
            DecisionReply::OrderTriggers(vec![valid[0], valid[0]]),
            EffectError::Trigger(TriggerError::InvalidTriggerOrder),
        ),
        (
            C,
            DecisionReply::OrderTriggers(vec![valid[0], foreign]),
            EffectError::Trigger(TriggerError::InvalidTriggerOrder),
        ),
    ] {
        assert_eq!(
            state.resume_rule(
                state.revision(),
                &EffectDecision::Triggers(frame.clone()),
                actor,
                reply,
                None
            ),
            Err(error)
        );
        assert_eq!(state, accepted);
    }
    let mut wrongs = Vec::new();
    let mut wrong = frame.clone();
    wrong.revision += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.event.pass += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.event.started_revision += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.wave = state.triggers.clone().start_wave().unwrap().1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.instances.reverse();
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.chooser = B;
    wrongs.push(wrong);
    wrongs.push(first);
    for wrong in wrongs {
        assert_eq!(
            state.resume_rule(
                state.revision(),
                &EffectDecision::Triggers(wrong),
                C,
                DecisionReply::OrderTriggers(valid.clone()),
                None
            ),
            Err(EffectError::Trigger(TriggerError::WrongOrderFrame))
        );
        assert_eq!(state, accepted);
    }
    assert_eq!(
        state.resume_rule(
            state.revision() - 1,
            &EffectDecision::Triggers(frame),
            C,
            DecisionReply::OrderTriggers(valid),
            None
        ),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(state, accepted);
}

#[test]
fn late_trigger_entry_exhaustion_drops_only_the_new_owner_candidate() {
    let original = prepared_trigger_cohort(false);
    let (state, first, _) = trigger_pending(original.begin_checks(original.revision()).unwrap());
    let (mut state, frame, _) =
        trigger_pending(order_triggers(&state, &first, first.instances().to_vec()).unwrap());
    state.window.set_test_next_entry(u64::MAX - 1);
    let accepted = state.clone();
    assert_eq!(
        order_triggers(&state, &frame, frame.instances().to_vec()),
        Err(EffectError::Window(
            StackWindowError::EntryIdentityExhausted
        ))
    );
    assert_eq!(state, accepted);
    assert_eq!(state.window.entries().len(), 2);
    assert_eq!(state.payloads.len(), 2);
    assert_eq!(state.triggers.waiting().len(), 4);
}

#[test]
fn late_payload_installation_failure_preserves_earlier_accepted_owner_and_waiting_bodies() {
    let original = prepared_trigger_cohort(false);
    let (state, first, _) = trigger_pending(original.begin_checks(original.revision()).unwrap());
    let (mut state, frame, _) =
        trigger_pending(order_triggers(&state, &first, first.instances().to_vec()).unwrap());
    // Derive the second actual allocation identity from a private trial receipt,
    // then inject an occupied payload slot. No public operation can create it.
    let sources = frame
        .instances()
        .iter()
        .map(|id| OrderedTrigger {
            controller: C,
            source: state.triggers.waiting()[id].source.object.clone(),
        })
        .collect();
    let (_, entries) = state
        .window
        .record_ordered_triggers_with_receipt(state.window.revision(), sources)
        .unwrap()
        .into_parts();
    state.payloads.insert(entries[1], StackPayload::Permanent);
    let accepted = state.clone();
    assert_eq!(
        order_triggers(&state, &frame, frame.instances().to_vec()),
        Err(EffectError::InconsistentProgram)
    );
    assert_eq!(state, accepted);
    assert_eq!(state.window.entries().len(), 2);
    assert_eq!(state.triggers.waiting().len(), 4);
    assert!(!state.payloads.contains_key(&entries[0]));
}

#[test]
fn completed_wave_reobserves_actual_sbas_and_places_the_new_wave_before_ready() {
    let original = prepared_trigger_cohort(false);
    let (state, first, _) = trigger_pending(original.begin_checks(original.revision()).unwrap());
    let (state, second, _) =
        trigger_pending(order_triggers(&state, &first, first.instances().to_vec()).unwrap());
    let (mut state, third, _) =
        trigger_pending(order_triggers(&state, &second, second.instances().to_vec()).unwrap());
    // Private adversarial pending-boundary probe, not an intervening player
    // action. It proves placement completion really returns through Observe.
    let newly_lethal = object(&state, 30);
    state.damage.insert(newly_lethal, 3.into());
    let (done, result, append) = order_triggers(&state, &third, third.instances().to_vec())
        .unwrap()
        .into_parts();
    assert!(matches!(result, RuleResult::Complete { .. }));
    assert!(done.window.cards().get(newly_lethal).is_none());
    assert!(done.triggers.waiting().is_empty());
    assert!(matches!(
        done.window.phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: A })
    ));
    let placed = placements(&append);
    assert_eq!(placed.len(), 5);
    assert_eq!(placed[0].wave, third.wave());
    assert_eq!(placed[1].wave, third.wave());
    assert_ne!(placed[2].wave, third.wave());
    assert!(placed[2..].iter().all(|value| value.wave == placed[2].wave));
    assert_eq!(
        placed[2..]
            .iter()
            .map(|value| value.controller)
            .collect::<Vec<_>>(),
        vec![A, C, B]
    );
    assert_eq!(done.window.entries().len(), 9);
    assert!(
        matches!(&append.records[4], RulesRecord::Event(event) if matches!(event.cause, EventCause::StateBasedActions { .. }))
    );
}

#[test]
fn whole_sba_event_captures_pre_cancellation_dying_sources_and_stops_before_loss_placement() {
    let mut state = prepared_staged_world(true);
    state.failed_draws.insert(C);
    std::sync::Arc::make_mut(&mut state.programs)
        .get_mut(&CardId(10))
        .unwrap()
        .triggered
        .push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::Dies(ObjectRelation::SelfObject),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(1.into()),
                }],
            ),
        });
    let source = object(&state, 10);
    state
        .characteristics
        .place_counters(
            source,
            &PowerToughnessCounters {
                plus_one: 2.into(),
                minus_one: 2.into(),
            },
        )
        .unwrap();
    let (state, first, _) = pending(state.begin_checks(state.revision()).unwrap());
    let (state, second, _) = pending(
        resume(
            &state,
            &first,
            A,
            DecisionReply::OrderGraveyard(
                graveyard_choice(&first)
                    .domain()
                    .iter()
                    .rev()
                    .copied()
                    .collect(),
            ),
            None,
        )
        .unwrap(),
    );
    let (done, result, append) = resume(
        &state,
        &second,
        B,
        DecisionReply::OrderGraveyard(
            graveyard_choice(&second)
                .domain()
                .iter()
                .rev()
                .copied()
                .collect(),
        ),
        None,
    )
    .unwrap()
    .into_parts();
    let RuleResult::Complete {
        completion: RuleCompletion::Checks(completion),
        ..
    } = result
    else {
        panic!()
    };
    assert!(completion.performed());
    assert!(completion.waiting_triggers());
    assert!(done.window.entries().is_empty());
    assert_eq!(placements(&append).len(), 0);
    let events = append
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    let event = events[0];
    assert!(matches!(event.cause, EventCause::StateBasedActions { .. }));
    assert_eq!(event.occurrences.len(), 9); // four deaths, two cancellations, one copy, two losses
    for (index, occurrence) in event.occurrences.iter().enumerate() {
        assert_eq!(occurrence.id.event, event.id);
        assert_eq!(occurrence.id.index, index as u64);
    }
    assert!(
        event.occurrences[..4]
            .iter()
            .all(|value| matches!(value.value, RulesOccurrence::ZoneChange(_)))
    );
    for occurrence in &event.occurrences[..4] {
        let RulesOccurrence::ZoneChange(change) = &occurrence.value else {
            panic!("death move");
        };
        // This fixture marks four positive-toughness creatures. The separate
        // mixed_death_causes loop above supplies the zero-toughness controls.
        assert_eq!(
            change.reason,
            ZoneReason::StateBased(spellwake_engine::occurrence::StateBasedMoveReasons::new(
                spellwake_engine::occurrence::StateBasedMoveReason::LethalDestruction
            ))
        );
    }
    assert!(
        matches!(event.occurrences[4].value, RulesOccurrence::CountersCancelled { object, .. } if object == source)
    );
    assert!(matches!(
        event.occurrences[6].value,
        RulesOccurrence::CopyCeased { .. }
    ));
    assert!(matches!(
        event.occurrences[7].value,
        RulesOccurrence::PlayerLost { player: C, .. }
    ));
    let RulesOccurrence::PlayerLost { reasons, .. } = &event.occurrences[7].value else {
        panic!()
    };
    assert_eq!(
        *reasons,
        BTreeSet::from([
            PlayerLossReason::NonpositiveLife,
            PlayerLossReason::FailedDraw
        ])
    );
    assert!(matches!(
        event.occurrences[8].value,
        RulesOccurrence::PlayerLost { player: B, .. }
    ));
    let waiting = done.waiting_triggers().next().unwrap();
    let counters = waiting.source.battlefield_counters.as_ref().unwrap();
    assert_eq!(
        counters,
        &PowerToughnessCounters {
            plus_one: 2.into(),
            minus_one: 2.into()
        }
    );
    assert_eq!(waiting.source.battlefield_marked_damage, Some(2.into()));
    assert_eq!(waiting.source.object.id(), source);
    assert!(done.window.cards().get(source).is_none());
    assert_eq!(done.power_toughness_counters(source), None);
    let change = event
        .occurrences
        .iter()
        .find_map(|value| match &value.value {
            RulesOccurrence::ZoneChange(change) if change.before.object.id() == source => {
                Some(change)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        change.before.battlefield_counters.as_ref().unwrap(),
        counters
    );
    assert_eq!(
        change.after.current_power_toughness,
        Some(PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        })
    );
    assert!(change.after.copiable_power_toughness.is_some());
}

#[test]
fn zero_mark_bookkeeping_and_no_discard_do_not_create_cleanup_events() {
    let (mut state, turn, marked) = prepared_cleanup(7);
    state.damage.insert(marked, 0.into());
    let accepted = state.clone();
    let (done, result, append) = state
        .begin_cleanup(state.revision(), &turn)
        .unwrap()
        .into_parts();
    assert_eq!(state, accepted);
    let RuleResult::Complete {
        completion: RuleCompletion::Checks(completion),
        checks,
        ..
    } = result
    else {
        panic!()
    };
    assert!(!completion.performed());
    assert!(!completion.waiting_triggers());
    assert!(checks.moves.is_empty());
    assert!(append.records.is_empty());
    assert!(done.damage.is_empty());
}

#[test]
fn cleanup_discard_discovery_precedes_mark_clear_and_waiting_only_completion_keeps_stack_empty() {
    let (mut state, turn, marked) = prepared_cleanup(8);
    add_trigger_to_program_for_cleanup(&mut state, marked);
    let (state, frame, _) = pending(state.begin_cleanup(state.revision(), &turn).unwrap());
    let (done, result, append) = resume(
        &state,
        &frame,
        A,
        DecisionReply::Discard(vec![discard_choice(&frame).domain()[0]]),
        Some(&turn),
    )
    .unwrap()
    .into_parts();
    let RuleResult::Complete {
        completion: RuleCompletion::Checks(completion),
        ..
    } = result
    else {
        panic!()
    };
    assert!(!completion.performed());
    assert!(completion.waiting_triggers());
    assert!(done.window.entries().is_empty());
    assert_eq!(done.creature(marked).unwrap().unwrap().damage, 0.into());
    assert!(
        matches!(&append.records[0], RulesRecord::Event(event) if matches!(event.cause, EventCause::CleanupDiscard { .. }))
    );
    let RulesRecord::TriggerDiscovered(waiting) = &append.records[1] else {
        panic!()
    };
    assert_eq!(
        waiting.source.battlefield_marked_damage.as_ref().unwrap(),
        &2.into()
    );
    let RulesRecord::Event(cleanup) = &append.records[2] else {
        panic!("actual cleanup characteristic action")
    };
    assert_eq!(
        cleanup.cause,
        EventCause::CleanupCharacteristics {
            position: turn.position()
        }
    );
    let [occurrence] = cleanup.occurrences.as_slice() else {
        panic!("one grouped cleanup occurrence")
    };
    let RulesOccurrence::CleanupCharacteristics {
        expired_effects,
        marks_removed,
        appearances,
    } = &occurrence.value
    else {
        panic!("combined cleanup characteristic receipt")
    };
    assert!(expired_effects.is_empty());
    assert_eq!(
        marks_removed,
        &[crate::occurrence::MarkedDamageRemoval {
            object: marked,
            previous: 2.into()
        }]
    );
    assert_eq!(appearances.len(), 1);
    assert_eq!(appearances[0].object, marked);
    assert_eq!(
        appearances[0].before.battlefield_marked_damage,
        Some(2.into())
    );
    assert_eq!(
        appearances[0].after.battlefield_marked_damage,
        Some(0.into())
    );
    assert_eq!(append.records.len(), 3);
    let priority = done
        .enter_turn_step(done.revision(), A, true, false)
        .unwrap()
        .into_state();
    let mut exhausted = priority.clone();
    exhausted.triggers.set_test_next_wave(u64::MAX);
    let accepted_waiting = exhausted.clone();
    assert_eq!(
        exhausted.begin_checks(exhausted.revision()),
        Err(EffectError::Trigger(TriggerError::WaveIdentityExhausted))
    );
    assert_eq!(exhausted, accepted_waiting);
    assert_eq!(exhausted.triggers.waiting().len(), 1);
    assert!(exhausted.window.entries().is_empty());
    let (placed, result, new_append) = priority
        .begin_checks(priority.revision())
        .unwrap()
        .into_parts();
    assert!(matches!(result, RuleResult::Complete { .. }));
    assert_eq!(placed.window.entries().len(), 1);
    let [
        RulesRecord::TriggerPlacementStarted(started),
        RulesRecord::TriggerPlaced(placed),
    ] = new_append.records.as_slice()
    else {
        panic!("cleanup trigger must start and complete placement")
    };
    assert_eq!(started, placed);
}

fn add_trigger_to_program_for_cleanup(state: &mut EffectWindow, object: ObjectId) {
    let card = state
        .window
        .cards()
        .get(object)
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    std::sync::Arc::make_mut(&mut state.programs)
        .get_mut(&card)
        .unwrap()
        .triggered
        .push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::Discarded(PlayerRelation::Controller),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(1.into()),
                }],
            ),
        });
}

#[test]
fn late_sba_publisher_and_wave_faults_preserve_the_entire_pre_action_world() {
    for fault in 0..3 {
        let mut original = prepared_trigger_cohort(false);
        let expected = match fault {
            0 => {
                original.triggers.set_test_next_event(u64::MAX);
                TriggerError::EventIdentityExhausted
            }
            1 => {
                original.triggers.set_test_next_instance(u64::MAX - 1);
                TriggerError::InstanceIdentityExhausted
            }
            _ => {
                original.triggers.set_test_next_wave(u64::MAX);
                TriggerError::WaveIdentityExhausted
            }
        };
        let accepted = original.clone();
        assert_eq!(
            original.begin_checks(original.revision()),
            Err(EffectError::Trigger(expected))
        );
        assert_eq!(original, accepted);
        assert!(original.window.entries().is_empty());
        for card in [10, 20] {
            assert_eq!(
                original
                    .window
                    .cards()
                    .get(object(&original, card))
                    .unwrap()
                    .zone(),
                ObjectZone::Battlefield
            );
        }
    }
}

fn discard_choice(frame: &CheckDecision) -> &crate::checks::CleanupDiscardChoice {
    let CheckDecision::CleanupDiscard(choice) = frame else {
        panic!("expected cleanup discard")
    };
    choice
}
fn graveyard_choice(frame: &CheckDecision) -> &crate::checks::GraveyardOrderChoice {
    let CheckDecision::GraveyardOrder(choice) = frame else {
        panic!("expected graveyard arrangement")
    };
    choice
}

/// Explicit trusted pre-check premise, built on the existing raw-copy fixture.
/// Legend choices, arrangements and all component writes execute normally.
fn prepared_legend_components() -> EffectWindow {
    use spellwake_contracts::names::{EnglishCardName, ObjectNames};
    let mut state = prepared_staged_world(true);
    for n in [10, 11, 20, 21] {
        let program = std::sync::Arc::make_mut(&mut state.programs)
            .get_mut(&CardId(n))
            .unwrap();
        program
            .supertypes
            .insert(crate::program::Supertype::Legendary);
        program.names = ObjectNames::single(EnglishCardName::new("Shared Legend".into()).unwrap());
    }
    add_trigger_to_existing(&mut state, 10);
    state
        .characteristics
        .place_counters(
            object(&state, 10),
            &PowerToughnessCounters {
                plus_one: 2.into(),
                minus_one: 2.into(),
            },
        )
        .unwrap();
    state.failed_draws.insert(C);
    state
}
fn add_trigger_to_existing(state: &mut EffectWindow, n: u64) {
    std::sync::Arc::make_mut(&mut state.programs)
        .get_mut(&CardId(n))
        .unwrap()
        .triggered
        .push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::Dies(ObjectRelation::SelfObject),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(1.into()),
                }],
            ),
        });
}
fn assert_legend_components_unwritten(state: &EffectWindow) {
    for n in [10, 11, 20, 21] {
        assert_eq!(
            state.window.cards().get(object(state, n)).unwrap().zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(state.damage.get(&object(state, n)), Some(&2.into()));
    }
    assert_eq!(
        state.power_toughness_counters(object(state, 10)),
        Some(PowerToughnessCounters {
            plus_one: 2.into(),
            minus_one: 2.into()
        })
    );
    assert_eq!(
        state.power_toughness_counters(object(state, 30)),
        Some(PowerToughnessCounters {
            plus_one: 3.into(),
            minus_one: 2.into()
        })
    );
    assert_eq!(state.window.spell_copies().len(), 1);
    assert!(state.state_based_losses.is_empty());
    assert!(state.waiting_triggers().next().is_none());
}
fn choose_legend_component(
    state: &EffectWindow,
    frame: &CheckDecision,
    actor: SeatId,
    keep: u64,
) -> RuleProgress {
    let CheckDecision::Legend(f) = frame else {
        panic!("legend choices precede owner arrangements")
    };
    assert_eq!(f.chooser(), actor);
    assert_eq!(f.conflicts().len(), 1);
    resume(
        state,
        frame,
        actor,
        DecisionReply::KeepLegend {
            conflict: f.conflicts()[0].id(),
            keep: object(state, keep),
        },
        None,
    )
    .unwrap()
}

#[test]
fn legend_keeps_and_all_owner_orders_precede_the_same_counter_copy_loss_and_capture_event() {
    for keep_a in [11, 21] {
        for keep_b in [10, 20] {
            for reverse_a in [false, true] {
                for reverse_b in [false, true] {
                    let source = prepared_legend_components();
                    let old_source = object(&source, 10);
                    let (state, first, _) =
                        pending(source.begin_checks(source.revision()).unwrap());
                    assert_legend_components_unwritten(&state);
                    let (state, second, checks) =
                        pending(choose_legend_component(&state, &first, A, keep_a));
                    assert!(checks.moves.is_empty());
                    assert_legend_components_unwritten(&state);
                    let (state, owner_a, checks) =
                        pending(choose_legend_component(&state, &second, B, keep_b));
                    assert!(checks.moves.is_empty());
                    assert_legend_components_unwritten(&state);
                    assert_eq!(owner_a.chooser(), A);
                    let mut order_a = vec![object(&state, 10), object(&state, 11)];
                    if reverse_a {
                        order_a.reverse();
                    }
                    let (state, owner_b, _) = pending(
                        resume(
                            &state,
                            &owner_a,
                            A,
                            DecisionReply::OrderGraveyard(order_a.clone()),
                            None,
                        )
                        .unwrap(),
                    );
                    assert_legend_components_unwritten(&state);
                    assert_first_order_retained(&state, &order_a);
                    let mut order_b = vec![object(&state, 20), object(&state, 21)];
                    if reverse_b {
                        order_b.reverse();
                    }
                    for (actor, reply) in [
                        (C, DecisionReply::OrderGraveyard(order_b.clone())),
                        (B, DecisionReply::OrderGraveyard(vec![])),
                        (B, DecisionReply::OrderGraveyard(vec![order_b[0]; 2])),
                        (
                            B,
                            DecisionReply::OrderGraveyard(vec![order_b[0], order_a[0]]),
                        ),
                        (
                            B,
                            DecisionReply::KeepLegend {
                                conflict: match &first {
                                    CheckDecision::Legend(f) => f.conflicts()[0].id(),
                                    _ => panic!(),
                                },
                                keep: old_source,
                            },
                        ),
                    ] {
                        let saved = state.clone();
                        assert!(resume(&state, &owner_b, actor, reply, None).is_err());
                        assert_eq!(state, saved);
                        assert_first_order_retained(&state, &order_a);
                        assert_legend_components_unwritten(&state);
                    }
                    let (done, result, append) = resume(
                        &state,
                        &owner_b,
                        B,
                        DecisionReply::OrderGraveyard(order_b),
                        None,
                    )
                    .unwrap()
                    .into_parts();
                    let RuleResult::Complete {
                        completion: RuleCompletion::Checks(completion),
                        checks,
                        ..
                    } = result
                    else {
                        panic!()
                    };
                    assert!(completion.performed() && completion.waiting_triggers());
                    assert_eq!(checks.moves.len(), 4);
                    assert!(done.window.spell_copies().is_empty());
                    assert_eq!(done.state_based_losses, BTreeSet::from([C, B]));
                    assert_eq!(done.waiting_triggers().count(), 1);
                    assert!(done.window.entries().is_empty());
                    let events = append
                        .records
                        .iter()
                        .filter_map(|r| match r {
                            RulesRecord::Event(e) => Some(e),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(events.len(), 1);
                    let event = events[0];
                    assert_eq!(event.occurrences.len(), 9);
                    let moves = event
                        .occurrences
                        .iter()
                        .filter_map(|r| match &r.value {
                            RulesOccurrence::ZoneChange(z) => Some(z),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(moves.len(), 4);
                    assert_eq!(
                        moves
                            .iter()
                            .map(|m| m.before.object.id())
                            .collect::<BTreeSet<_>>()
                            .len(),
                        4
                    );
                    for movement in moves {
                        let ZoneReason::StateBased(causes) = movement.reason else {
                            panic!()
                        };
                        assert!(causes.contains(StateBasedMoveReason::LethalDestruction));
                        let n = movement
                            .before
                            .object
                            .card_id()
                            .expect("physical fixture card")
                            .0;
                        assert_eq!(
                            causes.contains(StateBasedMoveReason::Legend),
                            if [11, 21].contains(&n) {
                                n != keep_a
                            } else {
                                n != keep_b
                            }
                        );
                    }
                    let waiting = done.waiting_triggers().next().unwrap();
                    assert_eq!(waiting.source.object.id(), old_source);
                    assert_eq!(waiting.source.battlefield_marked_damage, Some(2.into()));
                    assert_eq!(
                        waiting.source.battlefield_counters,
                        Some(PowerToughnessCounters {
                            plus_one: 2.into(),
                            minus_one: 2.into()
                        })
                    );
                    assert!(
                        waiting
                            .source
                            .current_supertypes
                            .contains(&crate::program::Supertype::Legendary)
                    );
                    assert_eq!(waiting.controller, B);
                    assert!(done.window.cards().get(old_source).is_none());
                    assert_eq!(
                        done.power_toughness_counters(object(&done, 30)),
                        Some(PowerToughnessCounters {
                            plus_one: 1.into(),
                            minus_one: 0.into()
                        })
                    );
                }
            }
        }
    }
}

#[test]
fn late_combined_legend_event_faults_preserve_both_controller_keeps_and_the_first_owner_order() {
    let source = prepared_legend_components();
    let (state, first, _) = pending(source.begin_checks(source.revision()).unwrap());
    let (state, second, _) = pending(choose_legend_component(&state, &first, A, 11));
    let (state, owner_a, _) = pending(choose_legend_component(&state, &second, B, 10));
    let order_a = graveyard_choice(&owner_a).domain().to_vec();
    let (state, _, _) = pending(
        resume(
            &state,
            &owner_a,
            A,
            DecisionReply::OrderGraveyard(order_a.clone()),
            None,
        )
        .unwrap(),
    );
    for fault in [
        super::super::private_qualification::QualificationFault::Event(u64::MAX),
        super::super::private_qualification::QualificationFault::ObjectIdentity(u64::MAX),
        super::super::private_qualification::QualificationFault::Instance(u64::MAX),
        super::super::private_qualification::QualificationFault::WindowRevision(u64::MAX - 1),
    ] {
        let mut faulted = state.clone();
        faulted.qualification_set_fault(fault);
        rebind_prepared_fault_cut(&mut faulted);
        let current = faulted.current_check_decision().unwrap();
        let saved = faulted.clone();
        assert!(
            resume(
                &faulted,
                &current,
                B,
                DecisionReply::OrderGraveyard(graveyard_choice(&current).domain().to_vec()),
                None
            )
            .is_err(),
            "{fault:?}"
        );
        assert_eq!(faulted, saved);
        assert_first_order_retained(&faulted, &order_a);
        assert_legend_components_unwritten(&faulted);
        let CheckPhase::Actions(actions) = &faulted.check_process().unwrap().phase else {
            panic!()
        };
        assert!(actions.legends.iter().all(|s| s.accepted.is_some()));
    }
}

fn pending_targeted_cohort() -> (EffectWindow, TriggerTargetChoice) {
    let mut state = prepared_trigger_cohort(false);
    for program in std::sync::Arc::make_mut(&mut state.programs).values_mut() {
        for ability in &mut program.triggered {
            ability.body = ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::AdjustTargetLife {
                    adjustment: crate::life::LifeAdjustment::Gain(1.into()),
                }],
            );
        }
    }
    let (state, order, _) = trigger_pending(state.begin_checks(state.revision()).unwrap());
    let (state, result, _) = order_triggers(&state, &order, order.instances().to_vec())
        .unwrap()
        .into_parts();
    let RuleResult::Pending {
        frame: EffectDecision::TriggerTarget(frame),
        ..
    } = result
    else {
        panic!("actual target choice")
    };
    (state, frame)
}
#[test]
fn failure_after_target_binding_and_payload_install_drops_only_the_tentative_suffix() {
    let (pending, frame) = pending_targeted_cohort();
    for fault in [0, 1] {
        let mut state = pending.clone();
        if fault == 0 {
            state.window.set_test_next_entry(u64::MAX);
        } else {
            state.window.set_test_revision(u64::MAX - 1);
        }
        let saved = state.clone();
        let before = crate::test_support::projection::effect(
            &state,
            &crate::test_support::packet::AliasContext::default(),
        )
        .unwrap();
        let expected = if fault == 0 {
            StackWindowError::EntryIdentityExhausted
        } else {
            StackWindowError::RevisionExhausted
        };
        assert_eq!(
            state.resume_rule(
                state.revision(),
                &EffectDecision::TriggerTarget(frame.clone()),
                frame.chooser(),
                DecisionReply::TriggerTarget(TargetRef::Player(B)),
                None
            ),
            Err(EffectError::Window(expected))
        );
        assert_eq!(state, saved);
        assert_eq!(state.triggers.waiting().len(), 6);
        assert!(state.stack_body(frame.entry()).is_none());
        assert!(
            state
                .window
                .entries()
                .last()
                .unwrap()
                .targets()
                .targets()
                .is_empty()
        );
        assert_eq!(
            crate::test_support::projection::effect(
                &state,
                &crate::test_support::packet::AliasContext::default()
            )
            .unwrap(),
            before
        );
    }
}
#[test]
fn factored_legality_preserves_initial_fallible_evaluation_for_every_wrapper_family() {
    let mut state = prepared_trigger_cohort(false);
    let source = object(&state, 100);
    state
        .characteristics
        .qualification_remove_object_timestamp(source);
    let expected = EffectError::CharacteristicInvariant(
        crate::characteristics::CharacteristicInvariantError::MissingBattlefieldTimestamp(source),
    );
    let saved = state.clone();
    for target in [
        BodyTarget::None,
        BodyTarget::SingleCreature,
        BodyTarget::SinglePermanent,
        BodyTarget::SinglePlayer,
        BodyTarget::SingleCreatureOrPlayer,
        BodyTarget::SingleSpell(crate::program::SpellTargetFilter::AnySpell),
        BodyTarget::SingleSpell(crate::program::SpellTargetFilter::CopyableInstructionSpell),
    ] {
        // Body instructions are irrelevant to this trusted predicate probe.
        let body = ExecutableBody::new(target, vec![]);
        assert_eq!(
            state.legal_body_target(&body, TargetRef::Player(B)),
            Err(expected.clone())
        );
        assert_eq!(state.trigger_target_domain(&body), Err(expected.clone()));
        assert_eq!(state, saved);
    }
}

/// Disclosed prepared source; the production cut guard precedes a requested late window fault.
#[test]
fn stale_prepared_window_cut_rejects_before_late_window_counter_fault() {
    let (mut state, _, frame, _) = accepted_first_owner(false);
    let reply = DecisionReply::OrderGraveyard(graveyard_choice(&frame).domain().to_vec());
    state.window.set_test_revision(u64::MAX - 1);
    let saved = state.clone();
    assert_eq!(
        state.current_check_decision(),
        Err(EffectError::Check(CheckError::WrongFrame))
    );
    assert_eq!(
        resume(&state, &frame, B, reply, None),
        Err(EffectError::Check(CheckError::WrongFrame))
    );
    assert_eq!(state, saved);
}
fn rebind_prepared_fault_cut(state: &mut EffectWindow) {
    // Only a private fault-preparation adjustment. It does not bypass the actual
    // production authority guard or establish a genuinely issued transition.
    let revision = state.window.revision();
    let CheckPhase::Actions(actions) = &mut state.check_process_mut().unwrap().phase else {
        panic!("prepared actions")
    };
    actions
        .movements
        .as_mut()
        .unwrap()
        .moves
        .qualification_rebind_cut(revision);
}
