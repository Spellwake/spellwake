//! Trusted prepared-boundary tests of the shared executor, not ordinary combat.
use super::*;
use crate::program::{BodyTarget, ExecutableBody};
use crate::{
    object_identity::CardId,
    turn::{Step, TurnStart},
};

const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
fn prepared() -> (
    EffectWindow,
    TurnScheduler,
    ObjectId,
    ObjectId,
    ObjectId,
    StackEntryId,
) {
    let mut cards = vec![];
    for (id, owner) in [(1, A), (2, B), (3, B)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program: Program::creature(
                None,
                3.into(),
                12.into(),
                std::collections::BTreeSet::new(),
            ),
        });
    }
    cards.push(SeedCard {
        card: CardId(4),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreatePreventionShield { amount: 3.into() }],
            ),
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
            vec![A, B],
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
    let object = |s: &EffectWindow, id| s.window.cards().current_identity(CardId(id)).unwrap();
    let (target, s1, s2) = (object(&state, 1), object(&state, 2), object(&state, 3));
    state = state.settle_checks(state.revision()).unwrap();
    state = state
        .cast(
            state.revision(),
            A,
            object(&state, 4),
            TargetRef::Object(target),
        )
        .unwrap();
    let creator = state.window.entries()[0].id();
    state = state.settle_checks(state.revision()).unwrap();
    for actor in [A, B] {
        state = state.pass(state.revision(), actor).unwrap();
        if matches!(
            state.window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            state = state.settle_checks(state.revision()).unwrap();
        }
    }
    state = state.resolve_top(state.revision()).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    for actor in [A, B] {
        state = state.pass(state.revision(), actor).unwrap();
        if matches!(
            state.window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            state = state.settle_checks(state.revision()).unwrap();
        }
    }
    state = state
        .enter_turn_step(state.revision(), A, false, false)
        .unwrap()
        .into_state();
    state.combat = Some(CombatState::new(vec![]).unwrap());
    let turn = TurnScheduler::new(
        TurnStart {
            number: 1,
            step: Step::CombatDamage,
        },
        false,
    )
    .unwrap();
    (state, turn, target, s1, s2, creator)
}
fn inputs(state: &EffectWindow, target: ObjectId, s1: ObjectId, s2: ObjectId) -> Vec<DamageInput> {
    [
        (s1, DamageRecipient::Creature(target), 3),
        (s2, DamageRecipient::Creature(target), 3),
        (target, DamageRecipient::Player(B), 4),
    ]
    .into_iter()
    .map(|(id, target, amount)| DamageInput {
        source: state.combat_damage_source(id).unwrap(),
        target,
        amount: amount.into(),
    })
    .collect()
}
#[test]
fn sole_shield_requires_exact_partition_and_defers_every_physical_result() {
    let (state, turn, target, s1, s2, _) = prepared();
    let proposed = inputs(&state, target, s1, s2);
    let (
        pending,
        RuleResult::Pending {
            frame: EffectDecision::Damage(DamageDecision::Allocation(frame)),
            journal,
            ..
        },
        _,
    ) = state
        .clone()
        .begin_damage_batch(
            DamageOrigin::Combat {
                position: turn.position(),
            },
            DamageKind::Combat,
            proposed,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
        .unwrap()
        .into_parts()
    else {
        panic!("sole shield has an interior partition")
    };
    assert_eq!(journal.batches.len(), 1);
    assert_eq!(journal.attempts.len(), 3);
    assert!(journal.applied.is_empty());
    assert!(journal.preventions.is_empty());
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.required(), &3.into());
    assert_eq!(frame.eligible().len(), 2);
    assert_eq!(pending.life_total(B), Some(&20.into()));
    for id in [target, s1, s2] {
        assert_eq!(pending.creature(id).unwrap().unwrap().damage, 0.into());
    }
    assert_eq!(
        pending.prevention_shields().next().unwrap().remaining,
        3.into()
    );
    let allocations = vec![
        DamageAllocation {
            attempt: frame.eligible()[0].attempt(),
            amount: 1.into(),
        },
        DamageAllocation {
            attempt: frame.eligible()[1].attempt(),
            amount: 2.into(),
        },
    ];
    let saved = pending.clone();
    let decision = EffectDecision::Damage(DamageDecision::Allocation(frame.clone()));
    for rows in [
        vec![],
        vec![allocations[0].clone(), allocations[0].clone()],
        vec![DamageAllocation {
            attempt: allocations[0].attempt,
            amount: 4.into(),
        }],
    ] {
        assert!(
            pending
                .resume_rule(
                    pending.revision(),
                    &decision,
                    A,
                    DecisionReply::AllocatePrevention(rows),
                    Some(&turn)
                )
                .is_err()
        );
        assert_eq!(pending, saved);
    }
    assert_eq!(
        pending.resume_rule(
            pending.revision(),
            &decision,
            B,
            DecisionReply::AllocatePrevention(allocations.clone()),
            Some(&turn)
        ),
        Err(EffectError::WrongChooser)
    );
    assert_eq!(
        pending.resume_rule(
            pending.revision(),
            &decision,
            A,
            DecisionReply::AllocatePrevention(allocations.clone()),
            None
        ),
        Err(EffectError::WrongPhase)
    );
    let (
        done,
        RuleResult::Complete {
            completion,
            journal,
            ..
        },
        _,
    ) = pending
        .resume_rule(
            pending.revision(),
            &decision,
            A,
            DecisionReply::AllocatePrevention(allocations),
            Some(&turn),
        )
        .unwrap()
        .into_parts()
    else {
        panic!("one partition completes the prepared batch")
    };
    assert_eq!(pending, saved);
    assert_eq!(
        completion,
        RuleCompletion::TurnWork(WorkCompleted::CombatDamage {
            additional_step_required: false
        })
    );
    assert_eq!(done.life_total(B), Some(&16.into()));
    assert_eq!(done.creature(target).unwrap().unwrap().damage, 3.into());
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(journal.applied.len(), 3);
    assert_eq!(journal.completions.len(), 3);
    assert!(done.pending_damage().is_none());
}
#[test]
fn allocation_reconstruction_binds_all_current_fields_and_turn_position() {
    let (state, turn, target, s1, s2, _) = prepared();
    let proposed = inputs(&state, target, s1, s2);
    let (
        state,
        RuleResult::Pending {
            frame: EffectDecision::Damage(DamageDecision::Allocation(frame)),
            ..
        },
        _,
    ) = state
        .begin_damage_batch(
            DamageOrigin::Combat {
                position: turn.position(),
            },
            DamageKind::Combat,
            proposed,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
        .unwrap()
        .into_parts()
    else {
        panic!()
    };
    let saved = state.clone();
    let mut wrongs = vec![];
    let mut wrong = frame.clone();
    wrong.stage += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.revision += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.capacity = 4.into();
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.eligible.pop();
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.chooser = B;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    let DamageOrigin::Combat { position } = &mut wrong.origin else {
        panic!()
    };
    position.number += 1;
    wrongs.push(wrong);
    for wrong in wrongs {
        assert_eq!(
            state.resume_rule(
                state.revision(),
                &EffectDecision::Damage(DamageDecision::Allocation(wrong)),
                A,
                DecisionReply::AllocatePrevention(vec![]),
                Some(&turn)
            ),
            Err(EffectError::WrongDamageFrame)
        );
        assert_eq!(state, saved);
    }
    let wrong_turn = TurnScheduler::new(
        TurnStart {
            number: 2,
            step: Step::CombatDamage,
        },
        false,
    )
    .unwrap();
    assert_eq!(
        state.resume_rule(
            state.revision(),
            &EffectDecision::Damage(DamageDecision::Allocation(frame)),
            A,
            DecisionReply::AllocatePrevention(vec![]),
            Some(&wrong_turn)
        ),
        Err(EffectError::WrongDamageFrame)
    );
    assert_eq!(state, saved);
}
#[test]
fn shared_player_result_path_is_simultaneous_and_uses_existing_loss_checks() {
    let (mut state, turn, target, s1, _, creator) = prepared();
    state.life = crate::life::LifeState::new(
        &[A, B],
        vec![
            crate::setup::StartingLife {
                player: A,
                total: 2.into(),
            },
            crate::setup::StartingLife {
                player: B,
                total: 3.into(),
            },
        ],
    )
    .unwrap();
    state
        .damage_state
        .add_multiplier(
            creator,
            crate::damage::DamageEffectTarget::Player(B),
            1.into(),
            DamageSourceFilter::Any,
        )
        .unwrap();
    let proposed = vec![
        DamageInput {
            source: state.combat_damage_source(s1).unwrap(),
            target: DamageRecipient::Player(A),
            amount: 2.into(),
        },
        DamageInput {
            source: state.combat_damage_source(target).unwrap(),
            target: DamageRecipient::Player(B),
            amount: 3.into(),
        },
    ];
    let (mut state, RuleResult::Complete { journal, .. }, _) = state
        .begin_damage_batch(
            DamageOrigin::Combat {
                position: turn.position(),
            },
            DamageKind::Combat,
            proposed,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
        .unwrap()
        .into_parts()
    else {
        panic!()
    };
    assert_eq!(state.life_total(A), Some(&0.into()));
    assert_eq!(state.life_total(B), Some(&0.into()));
    assert!(state.state_based_losses.is_empty());
    assert_eq!(journal.applied.len(), 2);
    assert_eq!(journal.multiplications.len(), 1);
    // Primitive damage input, followed by the real completed-work/check boundary.
    // This does not claim an ordinary 1v1 attack can hit the attacking player.
    let schedule = turn
        .complete_work(WorkCompleted::CombatDamage {
            additional_step_required: false,
        })
        .unwrap();
    state = state
        .enter_turn_step(
            state.revision(),
            A,
            schedule.stage() == crate::turn::TurnStage::Priority,
            true,
        )
        .unwrap()
        .into_state();
    state = state.settle_checks(state.revision()).unwrap();
    assert_eq!(state.state_based_losses, BTreeSet::from([A, B]));
    assert_eq!(state.damage_multipliers().count(), 1);
}

/// Returns actual shared-processor progress from a prepared resolution cursor.
/// The test-only input is not an authored player-target spell or an ordinary
/// combat claim. Physical sources, the target gate and parent completion are real.
pub(crate) fn n04_player_batch_fixture() -> (EffectWindow, RuleProgress) {
    let mut cards = vec![];
    for (id, owner) in [(1, A), (2, B)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program: Program::creature(
                None,
                1.into(),
                10.into(),
                std::collections::BTreeSet::new(),
            ),
        });
    }
    cards.push(SeedCard {
        card: CardId(4),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![crate::program::Instruction::DealDamage(Box::new(
                    crate::toolbox::DealDamageSpec {
                        source: crate::toolbox::DamageSourceQuery::EntrySource,
                        recipients: crate::toolbox::DamageTargetQuery::Targets,
                        amount: crate::program::DamageAmountSpec::Expr(
                            crate::program::ValueExpr::Literal(1.into()),
                        ),
                        exile_if_dies: false,
                    },
                ))],
            ),
        ),
    });
    let mut seed = {
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
            vec![],
            2.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    };
    seed.life.iter_mut().find(|l| l.player == B).unwrap().total = 3.into();
    let mut state = EffectWindow::new(seed).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    let a = state.window.cards().current_identity(CardId(1)).unwrap();
    let b = state.window.cards().current_identity(CardId(2)).unwrap();
    let spell = state.window.cards().current_identity(CardId(4)).unwrap();
    state = state
        .cast(state.revision(), A, spell, TargetRef::Object(b))
        .unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    for actor in [A, B] {
        state = state.pass(state.revision(), actor).unwrap();
        if matches!(
            state.window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            state = state.settle_checks(state.revision()).unwrap();
        }
    }
    let entry = state.window.checking_targets().unwrap().clone();
    let target = entry.targets().targets()[0].choice().id;
    state.window = state
        .window
        .check_resolution_targets(
            state.window.revision(),
            entry.id(),
            vec![TargetCheck {
                target,
                legal: true,
            }],
        )
        .unwrap();
    state.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
        revealed_piles: None,
        chosen_opponent: None,
        chosen_damage_recipient: None,
        chosen_damage_source: None,
        damage_offer_opponents: None,
        vowed_creatures: Default::default(),
        meld_pair: None,
        manifest_remaining: None,
        random_selection: None,
        results: ResolutionResults::default(),
        chosen_effect_color: None,
        chosen_option_index: None,
        chosen_distinct_colors: Vec::new(),
        chosen_mana_split: None,
        tempting_token_offer: None,
        destroyed_permanents: None,
        chosen_loyalty_removals: None,
        discarded_nonland_mana_value: None,
        last_created_tokens: Vec::new(),
        chosen_token_equipment: None,
        found_spells: Vec::new(),
        largest_discard: None,
        env: crate::toolbox::ExecEnv::new(),
        pending_prompt_frame: None,
        pending_prompt_reply: None,
        impulse: ImpulseState::default(),
        entry: entry.id(),
        instruction: 0,
        exiled_target: None,
        remembered_permanent: None,
        values: BTreeMap::new(),
    }));
    let inputs = vec![
        DamageInput {
            source: state.combat_damage_source(b).unwrap(),
            target: DamageRecipient::Player(A),
            amount: 2.into(),
        },
        DamageInput {
            source: state.combat_damage_source(a).unwrap(),
            target: DamageRecipient::Player(B),
            amount: 3.into(),
        },
    ];
    let progress = state
        .clone()
        .begin_damage_batch(
            DamageOrigin::Resolution {
                entry: entry.id(),
                instruction: 0,
            },
            DamageKind::Noncombat,
            inputs,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
        .unwrap();
    (state, progress)
}

#[test]
fn affected_players_choose_apnap_and_later_replies_preserve_accepted_modifications() {
    let (mut state, turn, target, s1, s2, creator) = prepared();
    state
        .damage_state
        .add_shield(
            creator,
            crate::damage::DamageEffectTarget::Permanent(s1),
            3.into(),
        )
        .unwrap();
    let rows = [
        (s1, target, 3),
        (s2, target, 3),
        (target, s1, 2),
        (s2, s1, 4),
    ]
    .into_iter()
    .map(|(source, target, amount)| DamageInput {
        source: state.combat_damage_source(source).unwrap(),
        target: DamageRecipient::Creature(target),
        amount: amount.into(),
    })
    .collect();
    let (
        state,
        RuleResult::Pending {
            frame: EffectDecision::Damage(DamageDecision::Allocation(first)),
            journal,
            ..
        },
        _,
    ) = state
        .begin_damage_batch(
            DamageOrigin::Combat {
                position: turn.position(),
            },
            DamageKind::Combat,
            rows,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
        .unwrap()
        .into_parts()
    else {
        panic!("active affected player chooses first")
    };
    assert_eq!(first.chooser(), A);
    assert_eq!(journal.batches.len(), 1);
    assert_eq!(journal.attempts.len(), 4);
    let first_reply = vec![
        DamageAllocation {
            attempt: first.eligible()[0].attempt(),
            amount: 1.into(),
        },
        DamageAllocation {
            attempt: first.eligible()[1].attempt(),
            amount: 2.into(),
        },
    ];
    let (
        after_a,
        RuleResult::Pending {
            frame: EffectDecision::Damage(DamageDecision::Allocation(second)),
            journal,
            ..
        },
        _,
    ) = state
        .resume_rule(
            state.revision(),
            &EffectDecision::Damage(DamageDecision::Allocation(first.clone())),
            A,
            DecisionReply::AllocatePrevention(first_reply.clone()),
            Some(&turn),
        )
        .unwrap()
        .into_parts()
    else {
        panic!("nonactive affected player chooses after accepted active allocation")
    };
    assert_eq!(second.chooser(), B);
    assert_eq!(second.stage(), first.stage() + 1);
    assert_eq!(second.batch(), first.batch());
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(journal.preventions[0].allocations, first_reply);
    assert!(journal.batches.is_empty());
    assert!(journal.attempts.is_empty());
    assert!(journal.applied.is_empty());
    assert!(journal.completions.is_empty());
    assert_eq!(after_a.life_total(A), Some(&20.into()));
    assert_eq!(after_a.life_total(B), Some(&20.into()));
    for id in [target, s1, s2] {
        assert_eq!(after_a.creature(id).unwrap().unwrap().damage, 0.into());
    }
    let accepted = after_a.clone();
    assert_eq!(
        after_a.resume_rule(
            after_a.revision(),
            &EffectDecision::Damage(DamageDecision::Allocation(first)),
            A,
            DecisionReply::AllocatePrevention(first_reply),
            Some(&turn)
        ),
        Err(EffectError::WrongDamageFrame)
    );
    assert!(
        after_a
            .resume_rule(
                after_a.revision(),
                &EffectDecision::Damage(DamageDecision::Allocation(second.clone())),
                B,
                DecisionReply::AllocatePrevention(vec![]),
                Some(&turn)
            )
            .is_err()
    );
    assert_eq!(after_a, accepted);
    let second_reply = vec![
        DamageAllocation {
            attempt: second.eligible()[0].attempt(),
            amount: 1.into(),
        },
        DamageAllocation {
            attempt: second.eligible()[1].attempt(),
            amount: 2.into(),
        },
    ];
    let (done, RuleResult::Complete { journal, .. }, _) = after_a
        .resume_rule(
            after_a.revision(),
            &EffectDecision::Damage(DamageDecision::Allocation(second)),
            B,
            DecisionReply::AllocatePrevention(second_reply),
            Some(&turn),
        )
        .unwrap()
        .into_parts()
    else {
        panic!("all components commit together")
    };
    assert_eq!(after_a, accepted);
    assert_eq!(journal.preventions.len(), 1);
    assert!(journal.batches.is_empty());
    assert!(journal.attempts.is_empty());
    assert_eq!(journal.applied.len(), 4);
    assert_eq!(journal.completions.len(), 4);
    assert_eq!(done.creature(target).unwrap().unwrap().damage, 3.into());
    assert_eq!(done.creature(s1).unwrap().unwrap().damage, 3.into());
    assert_eq!(done.creature(s2).unwrap().unwrap().damage, 0.into());
}
