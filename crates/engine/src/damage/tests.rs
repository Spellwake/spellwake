use super::*;
use crate::{
    object_identity::{CardId, GameObjects, ObjectDestination},
    stack_window::StackWindow,
};

struct Fixture {
    creator: StackEntryId,
    sources: [DamageSource; 2],
    permanent: DamageEffectTarget,
    creature: DamageRecipient,
}
impl Fixture {
    fn new() -> Self {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut cards = GameObjects::new(0);
        for (id, owner, destination) in [
            (1, a, ObjectDestination::Battlefield { controller: a }),
            (2, b, ObjectDestination::Battlefield { controller: b }),
            (3, b, ObjectDestination::Battlefield { controller: b }),
            (4, a, ObjectDestination::Hand),
        ] {
            cards = cards
                .seed_card(cards.revision(), CardId(id), owner, destination)
                .unwrap()
                .next;
        }
        let source = |card| {
            let object = cards
                .get(cards.current_identity(CardId(card)).unwrap())
                .unwrap()
                .clone();
            DamageSource::Object {
                controller: object.controller().unwrap(),
                object,
            }
        };
        let sources = [source(1), source(2)];
        let object = cards.current_identity(CardId(3)).unwrap();
        let permanent = DamageEffectTarget::Permanent(object);
        let creature = DamageRecipient::Creature(object);
        let spell = cards.current_identity(CardId(4)).unwrap();
        let window = StackWindow::new(vec![a, b], a, cards).unwrap();
        let window = window.finish_checks(window.revision()).unwrap();
        let window = window
            .record_card_spell(window.revision(), a, spell)
            .unwrap();
        Self {
            creator: window.entries()[0].id(),
            sources,
            permanent,
            creature,
        }
    }
    fn origin(&self) -> DamageOrigin {
        DamageOrigin::Resolution {
            entry: self.creator,
            instruction: 2,
        }
    }
    fn input(&self, source: usize, target: DamageRecipient, amount: i64) -> DamageInput {
        DamageInput {
            source: self.sources[source].clone(),
            target,
            amount: amount.into(),
        }
    }
    fn pair(&self) -> Vec<DamageInput> {
        vec![
            self.input(0, self.creature, 3),
            self.input(1, self.creature, 3),
        ]
    }
    fn batch(&self, state: &mut DamageState, inputs: Vec<DamageInput>) -> ActiveDamageBatch {
        state
            .begin_batch(self.origin(), DamageKind::Noncombat, inputs)
            .unwrap()
            .0
    }
}
fn allocation(attempt: DamageAttemptId, amount: i64) -> DamageAllocation {
    DamageAllocation {
        attempt,
        amount: amount.into(),
    }
}

#[test]
fn one_batch_retains_original_sources_and_per_component_result_lineage() {
    let f = Fixture::new();
    let player = DamageRecipient::Player(SeatId(1));
    let mut state = DamageState::default();
    let (batch, journal) = state
        .begin_batch(
            f.origin(),
            DamageKind::Noncombat,
            vec![
                f.input(0, f.creature, 3),
                f.input(1, f.creature, 4),
                f.input(0, player, 5),
            ],
        )
        .unwrap();
    assert_eq!(journal.batches, vec![batch.started().clone()]);
    assert_eq!(journal.attempts.len(), 3);
    assert_eq!(
        batch.started().components,
        journal.attempts.iter().map(|a| a.id).collect::<Vec<_>>()
    );
    assert_eq!(batch.origin(), f.origin());
    assert_eq!(batch.kind(), DamageKind::Noncombat);
    assert_eq!(batch.components().count(), 3);
    assert_eq!(batch.stage(), 0);
    assert!(journal.applied.is_empty());
    let (applied, completions) = state.finish_batch(&batch).unwrap();
    assert_eq!(applied.len(), 3);
    assert_eq!(completions.len(), 3);
    for ((attempt, applied), completion) in journal.attempts.iter().zip(&applied).zip(&completions)
    {
        assert_eq!(attempt.batch, batch.id());
        assert_eq!(applied.batch, batch.id());
        assert_eq!(completion.batch, batch.id());
        assert_eq!(applied.attempt, attempt.id);
        assert_eq!(completion.attempt, attempt.id);
        assert_eq!(applied.source, attempt.source);
        assert_eq!(applied.target, attempt.target);
        assert_eq!(applied.amount, attempt.amount);
        assert_eq!(
            completion.outcome,
            DamageOutcome::Dealt(attempt.amount.clone())
        );
    }
    assert_eq!(
        journal.attempts[0].source.id(),
        journal.attempts[2].source.id()
    );
    assert_ne!(journal.attempts[0].id, journal.attempts[2].id);
    assert_eq!(state.finish_batch(&batch).unwrap(), (applied, completions));
}

#[test]
fn duplicate_nonpositive_and_empty_proposals_allocate_nothing() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    state.add_shield(f.creator, f.permanent, 7.into()).unwrap();
    let before = state.clone();
    assert_eq!(
        state.begin_batch(f.origin(), DamageKind::Noncombat, vec![]),
        Err(DamageError::EmptyBatch)
    );
    assert_eq!(state, before);
    assert_eq!(
        state.begin_batch(
            f.origin(),
            DamageKind::Noncombat,
            vec![f.input(0, f.creature, 3), f.input(0, f.creature, 4),]
        ),
        Err(DamageError::DuplicateSourceRecipient)
    );
    assert_eq!(state, before);
    for amount in [0, -1] {
        assert_eq!(
            state.begin_batch(
                f.origin(),
                DamageKind::Noncombat,
                vec![f.input(0, f.creature, 3), f.input(1, f.creature, amount),]
            ),
            Err(DamageError::InvalidAmount)
        );
        assert_eq!(
            state.add_shield(f.creator, f.permanent, amount.into()),
            Err(DamageError::InvalidAmount)
        );
        assert_eq!(
            state.add_multiplier(
                f.creator,
                f.permanent,
                amount.into(),
                DamageSourceFilter::Any
            ),
            Err(DamageError::InvalidFactor)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn one_shield_accepts_both_interior_partitions_without_early_debit() {
    let f = Fixture::new();
    for split in [(1, 2), (2, 1)] {
        let mut state = DamageState::default();
        let shield = state.add_shield(f.creator, f.permanent, 3.into()).unwrap();
        let mut batch = f.batch(&mut state, f.pair());
        let ids = batch.started().components.clone();
        let applications = state.applicable(&batch);
        assert_eq!(applications.len(), 1);
        let application = applications[0];
        assert_eq!(state.forced_allocation(&batch, application).unwrap(), None);
        let before_state = state.clone();
        let before_components = batch.components.clone();
        state.select_prevention(&mut batch, application).unwrap();
        assert_eq!(state, before_state);
        assert_eq!(batch.components, before_components);
        assert_eq!(batch.stage(), 0);
        assert_eq!(batch.phase(), &DamageBatchPhase::Allocating { application });
        assert_eq!(
            state.select_prevention(&mut batch, application),
            Err(DamageError::WrongPhase)
        );
        assert_eq!(state.finish_batch(&batch), Err(DamageError::WrongPhase));
        let fact = state
            .apply_prevention(
                &mut batch,
                application,
                vec![allocation(ids[1], split.1), allocation(ids[0], split.0)],
            )
            .unwrap();
        assert_eq!(fact.batch, batch.id());
        assert_eq!(fact.creator, f.creator);
        assert_eq!(fact.shield, shield.id);
        assert_eq!(fact.stage, 1);
        assert_eq!(fact.amount, 3.into());
        assert_eq!(
            fact.allocations,
            vec![allocation(ids[0], split.0), allocation(ids[1], split.1)]
        );
        assert_eq!(
            fact.damage_remaining,
            BTreeMap::from([
                (ids[0], (3 - split.0).into()),
                (ids[1], (3 - split.1).into()),
            ])
        );
        assert_eq!(fact.shield_remaining, 0.into());
        assert!(state.shield(shield.id).is_none());
        assert_eq!(batch.stage(), 1);
        assert_eq!(batch.phase(), &DamageBatchPhase::Selecting);
        assert!(batch.consumed_shields().contains(&application));
        let (applied, completions) = state.finish_batch(&batch).unwrap();
        assert_eq!(
            applied.iter().map(|a| a.amount.clone()).collect::<Vec<_>>(),
            vec![(3 - split.0).into(), (3 - split.1).into()]
        );
        assert_eq!(completions.len(), 2);
    }
}

#[test]
fn every_invalid_sparse_allocation_preserves_selected_phase_and_all_state() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    state.add_shield(f.creator, f.permanent, 3.into()).unwrap();
    let mut inputs = f.pair();
    inputs.push(f.input(0, DamageRecipient::Player(SeatId(0)), 9));
    let mut batch = f.batch(&mut state, inputs);
    let ids = batch.started().components.clone();
    let application = state.applicable(&batch)[0];
    state.select_prevention(&mut batch, application).unwrap();
    let before = (state.clone(), batch.clone());
    for rows in [
        vec![],
        vec![allocation(ids[0], 0), allocation(ids[1], 3)],
        vec![allocation(ids[0], -1), allocation(ids[1], 4)],
        vec![allocation(ids[0], 1), allocation(ids[0], 2)],
        vec![allocation(DamageAttemptId(999), 3)],
        vec![allocation(ids[2], 3)],
        vec![allocation(ids[0], 4)],
        vec![allocation(ids[0], 2)],
        vec![allocation(ids[0], 2), allocation(ids[1], 2)],
    ] {
        assert_eq!(
            state.apply_prevention(&mut batch, application, rows),
            Err(DamageError::InvalidAllocation)
        );
        assert_eq!((&state, &batch), (&before.0, &before.1));
    }
    for wrong in [
        DamageApplication {
            effect: PreventionShieldId(999).into(),
            ..application
        },
        DamageApplication {
            scope: DamageScope::Component(ids[0]),
            ..application
        },
    ] {
        assert_eq!(
            state.apply_prevention(&mut batch, wrong, vec![allocation(ids[0], 3)]),
            Err(DamageError::WrongPhase)
        );
        assert_eq!((&state, &batch), (&before.0, &before.1));
    }
    assert_eq!(
        state.apply_multiplier(
            &mut batch,
            DamageApplication {
                effect: DamageMultiplierId(1).into(),
                scope: DamageScope::Component(ids[0]),
            }
        ),
        Err(DamageError::WrongPhase)
    );
    assert_eq!((&state, &batch), (&before.0, &before.1));
}

#[test]
fn exact_huge_partition_does_not_narrow_or_enumerate_damage_points() {
    let f = Fixture::new();
    let huge: RulesInteger = "9007199254740993123456789012345678901234567890"
        .to_owned()
        .try_into()
        .unwrap();
    let one = RulesInteger::from(1);
    let mut state = DamageState::default();
    state
        .add_shield(f.creator, f.permanent, huge.clone())
        .unwrap();
    let mut batch = f.batch(
        &mut state,
        f.pair()
            .into_iter()
            .map(|mut input| {
                input.amount = huge.clone();
                input
            })
            .collect(),
    );
    let ids = batch.started().components.clone();
    let application = state.applicable(&batch)[0];
    assert_eq!(state.forced_allocation(&batch, application).unwrap(), None);
    state.select_prevention(&mut batch, application).unwrap();
    let fact = state
        .apply_prevention(
            &mut batch,
            application,
            vec![
                DamageAllocation {
                    attempt: ids[0],
                    amount: one.clone(),
                },
                DamageAllocation {
                    attempt: ids[1],
                    amount: huge.minus(&one),
                },
            ],
        )
        .unwrap();
    assert_eq!(fact.amount, huge);
    assert_eq!(
        batch.component(ids[0]).unwrap().remaining(),
        &huge.minus(&one)
    );
    assert_eq!(batch.component(ids[1]).unwrap().remaining(), &one);
}

#[test]
fn covering_and_single_component_allocations_are_forced_and_keep_residue() {
    let f = Fixture::new();
    for pair in [false, true] {
        let mut state = DamageState::default();
        let shield = state.add_shield(f.creator, f.permanent, 9.into()).unwrap();
        let inputs = if pair {
            f.pair()
        } else {
            vec![f.input(0, f.creature, 3)]
        };
        let mut batch = f.batch(&mut state, inputs);
        let application = state.applicable(&batch)[0];
        let allocations = state
            .forced_allocation(&batch, application)
            .unwrap()
            .unwrap();
        assert_eq!(allocations.len(), if pair { 2 } else { 1 });
        let fact = state
            .apply_prevention(&mut batch, application, allocations)
            .unwrap();
        assert_eq!(fact.amount, if pair { 6.into() } else { 3.into() });
        assert_eq!(
            state.shield(shield.id).unwrap().remaining,
            if pair { 3.into() } else { 6.into() }
        );
        assert!(
            fact.damage_remaining
                .values()
                .all(|amount| *amount == 0.into())
        );
        let (applied, completions) = state.finish_batch(&batch).unwrap();
        assert!(applied.is_empty());
        assert_eq!(completions.len(), if pair { 2 } else { 1 });
        assert!(
            completions
                .iter()
                .all(|c| c.outcome == DamageOutcome::FullyPrevented)
        );
        let mut later = f.batch(&mut state, vec![f.input(0, f.creature, 1)]);
        let later_application = state.applicable(&later)[0];
        let rows = state
            .forced_allocation(&later, later_application)
            .unwrap()
            .unwrap();
        state
            .apply_prevention(&mut later, later_application, rows)
            .unwrap();
        assert!(state.finish_batch(&later).unwrap().0.is_empty());
    }
    let mut state = DamageState::default();
    state.add_shield(f.creator, f.permanent, 1.into()).unwrap();
    let batch = f.batch(&mut state, vec![f.input(0, f.creature, 3)]);
    let rows = state
        .forced_allocation(&batch, state.applicable(&batch)[0])
        .unwrap()
        .unwrap();
    assert_eq!(rows, vec![allocation(batch.started().components[0], 1)]);
}

#[test]
fn multiplier_filters_and_use_are_component_local_and_reset_for_later_batch() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    let any = state
        .add_multiplier(f.creator, f.permanent, 1.into(), DamageSourceFilter::Any)
        .unwrap();
    let exact = state
        .add_multiplier(
            f.creator,
            f.permanent,
            2.into(),
            DamageSourceFilter::Exact(f.sources[0].id()),
        )
        .unwrap();
    let mut batch = f.batch(&mut state, f.pair());
    let ids = batch.started().components.clone();
    let exact_application = DamageApplication {
        effect: exact.id.into(),
        scope: DamageScope::Component(ids[0]),
    };
    assert_eq!(state.applicable(&batch).len(), 3);
    state
        .apply_multiplier(&mut batch, exact_application)
        .unwrap();
    assert_eq!(batch.component(ids[0]).unwrap().remaining(), &6.into());
    assert_eq!(batch.component(ids[1]).unwrap().remaining(), &3.into());
    let saved = batch.clone();
    assert_eq!(
        state.apply_multiplier(&mut batch, exact_application),
        Err(DamageError::InapplicableEffect)
    );
    assert_eq!(batch, saved);
    assert_eq!(
        state.apply_multiplier(
            &mut batch,
            DamageApplication {
                effect: exact.id.into(),
                scope: DamageScope::Component(ids[1]),
            }
        ),
        Err(DamageError::InapplicableEffect)
    );
    assert_eq!(batch, saved);
    for (index, id) in ids.iter().enumerate() {
        let application = DamageApplication {
            effect: any.id.into(),
            scope: DamageScope::Component(*id),
        };
        let fact = state.apply_multiplier(&mut batch, application).unwrap();
        assert_eq!(fact.stage, 2 + u64::try_from(index).unwrap());
        assert_eq!(fact.before, fact.after);
        assert!(
            batch
                .component(*id)
                .unwrap()
                .used_multipliers()
                .contains(&any.id)
        );
    }
    assert!(state.applicable(&batch).is_empty());
    assert_eq!(
        state
            .finish_batch(&batch)
            .unwrap()
            .0
            .iter()
            .map(|a| a.amount.clone())
            .collect::<Vec<_>>(),
        vec![6.into(), 3.into()]
    );
    let later = f.batch(&mut state, f.pair());
    assert_eq!(state.applicable(&later).len(), 3);
}

#[test]
fn n03_exact_source_filter_changes_only_two_to_four_and_retains_three() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    let effect = state
        .add_multiplier(
            f.creator,
            f.permanent,
            2.into(),
            DamageSourceFilter::Exact(f.sources[0].id()),
        )
        .unwrap();
    let mut batch = f.batch(
        &mut state,
        vec![f.input(0, f.creature, 2), f.input(1, f.creature, 3)],
    );
    let application = state.applicable(&batch)[0];
    assert_eq!(state.applicable(&batch).len(), 1);
    let fact = state.apply_multiplier(&mut batch, application).unwrap();
    assert_eq!(fact.before, 2.into());
    assert_eq!(fact.after, 4.into());
    let before = batch.clone();
    assert_eq!(
        state.apply_multiplier(&mut batch, application),
        Err(DamageError::InapplicableEffect)
    );
    assert_eq!(batch, before);
    assert_eq!(
        batch.component(fact.attempt).unwrap().used_multipliers(),
        &BTreeSet::from([effect.id])
    );
    let (applied, completed) = state.finish_batch(&batch).unwrap();
    assert_eq!(
        applied.iter().map(|a| a.amount.clone()).collect::<Vec<_>>(),
        vec![4.into(), 3.into()]
    );
    assert_eq!(completed.len(), 2);
}

#[test]
fn singleton_mixed_ordering_and_zero_suppression_preserve_existing_arithmetic() {
    let f = Fixture::new();
    for prevention_first in [true, false] {
        let mut state = DamageState::default();
        let shield = state.add_shield(f.creator, f.permanent, 1.into()).unwrap();
        let multiplier = state
            .add_multiplier(f.creator, f.permanent, 2.into(), DamageSourceFilter::Any)
            .unwrap();
        let mut batch = f.batch(&mut state, vec![f.input(0, f.creature, 3)]);
        let prevention = DamageApplication {
            effect: shield.id.into(),
            scope: DamageScope::Recipient(f.creature),
        };
        let multiplication = DamageApplication {
            effect: multiplier.id.into(),
            scope: DamageScope::Component(batch.started().components[0]),
        };
        assert_eq!(
            state.finish_batch(&batch),
            Err(DamageError::EffectsStillApplicable)
        );
        if !prevention_first {
            state.apply_multiplier(&mut batch, multiplication).unwrap();
        }
        let rows = state
            .forced_allocation(&batch, prevention)
            .unwrap()
            .unwrap();
        state
            .apply_prevention(&mut batch, prevention, rows)
            .unwrap();
        if prevention_first {
            state.apply_multiplier(&mut batch, multiplication).unwrap();
        }
        assert_eq!(batch.stage(), 2);
        assert_eq!(
            state.finish_batch(&batch).unwrap().0[0].amount,
            if prevention_first { 4.into() } else { 5.into() }
        );
    }
    let mut state = DamageState::default();
    let shield = state.add_shield(f.creator, f.permanent, 9.into()).unwrap();
    state
        .add_multiplier(f.creator, f.permanent, 100.into(), DamageSourceFilter::Any)
        .unwrap();
    let mut batch = f.batch(&mut state, vec![f.input(0, f.creature, 3)]);
    let application = DamageApplication {
        effect: shield.id.into(),
        scope: DamageScope::Recipient(f.creature),
    };
    let rows = state
        .forced_allocation(&batch, application)
        .unwrap()
        .unwrap();
    state
        .apply_prevention(&mut batch, application, rows)
        .unwrap();
    assert!(state.applicable(&batch).is_empty());
    assert!(state.finish_batch(&batch).unwrap().0.is_empty());
    assert!(batch.components().all(|c| c.used_multipliers().is_empty()));
}

#[test]
fn unsupported_scopes_reject_before_any_counter_or_effect_mutation() {
    let f = Fixture::new();
    for mixed in [false, true] {
        let mut state = DamageState::default();
        state.add_shield(f.creator, f.permanent, 5.into()).unwrap();
        if mixed {
            state
                .add_multiplier(
                    f.creator,
                    f.permanent,
                    2.into(),
                    DamageSourceFilter::Exact(f.sources[0].id()),
                )
                .unwrap();
        } else {
            state.add_shield(f.creator, f.permanent, 5.into()).unwrap();
        }
        state.next_batch = u64::MAX;
        state.next_attempt = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.begin_batch(f.origin(), DamageKind::Combat, f.pair()),
            Err(DamageError::UnsupportedSimultaneousScope)
        );
        assert_eq!(state, saved);
    }
    // Effects on distinct recipients do not acquire a fictitious shared scope.
    let mut state = DamageState::default();
    state.add_shield(f.creator, f.permanent, 2.into()).unwrap();
    state.add_shield(f.creator, f.permanent, 3.into()).unwrap();
    let player = DamageRecipient::Player(SeatId(1));
    state
        .add_multiplier(
            f.creator,
            DamageEffectTarget::Player(SeatId(1)),
            2.into(),
            DamageSourceFilter::Any,
        )
        .unwrap();
    assert!(
        state
            .begin_batch(
                f.origin(),
                DamageKind::Combat,
                vec![f.input(0, f.creature, 3), f.input(1, player, 3)]
            )
            .is_ok()
    );
    // A source-filtered effect with no matching component does not create competition.
    let mut state = DamageState::default();
    state.add_shield(f.creator, f.permanent, 3.into()).unwrap();
    state
        .add_multiplier(
            f.creator,
            f.permanent,
            2.into(),
            DamageSourceFilter::Exact(DamageSourceId::SpellCopy(f.creator)),
        )
        .unwrap();
    assert!(
        state
            .begin_batch(f.origin(), DamageKind::Combat, f.pair())
            .is_ok()
    );
}

#[test]
fn late_batch_component_allocation_failure_preserves_all_nonzero_state() {
    let f = Fixture::new();
    for batch_fault in [false, true] {
        let mut state = DamageState::default();
        state.add_shield(f.creator, f.permanent, 7.into()).unwrap();
        state
            .add_multiplier(
                f.creator,
                DamageEffectTarget::Player(SeatId(0)),
                2.into(),
                DamageSourceFilter::Any,
            )
            .unwrap();
        f.batch(
            &mut state,
            vec![f.input(0, DamageRecipient::Player(SeatId(1)), 1)],
        );
        if batch_fault {
            state.next_batch = u64::MAX;
        } else {
            state.next_attempt = u64::MAX - 1;
        }
        let saved = state.clone();
        assert_eq!(
            state.begin_batch(f.origin(), DamageKind::Noncombat, f.pair()),
            Err(if batch_fault {
                DamageError::BatchIdentityExhausted
            } else {
                DamageError::AttemptIdentityExhausted
            })
        );
        assert_eq!(state, saved);
    }
    let mut state = DamageState {
        next_shield: u64::MAX,
        next_multiplier: u64::MAX,
        ..Default::default()
    };
    let saved = state.clone();
    assert_eq!(
        state.add_shield(f.creator, f.permanent, 3.into()),
        Err(DamageError::ShieldIdentityExhausted)
    );
    assert_eq!(
        state.add_multiplier(f.creator, f.permanent, 2.into(), DamageSourceFilter::Any),
        Err(DamageError::MultiplierIdentityExhausted)
    );
    assert_eq!(state, saved);
}

#[test]
fn late_stage_failure_keeps_accepted_effect_use_and_allocation_phase() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    let shield = state.add_shield(f.creator, f.permanent, 1.into()).unwrap();
    let multiplier = state
        .add_multiplier(f.creator, f.permanent, 2.into(), DamageSourceFilter::Any)
        .unwrap();
    let mut batch = f.batch(&mut state, vec![f.input(0, f.creature, 3)]);
    let multiplication = DamageApplication {
        effect: multiplier.id.into(),
        scope: DamageScope::Component(batch.started().components[0]),
    };
    state.apply_multiplier(&mut batch, multiplication).unwrap();
    let prevention = DamageApplication {
        effect: shield.id.into(),
        scope: DamageScope::Recipient(f.creature),
    };
    state.select_prevention(&mut batch, prevention).unwrap();
    let rows = state
        .forced_allocation(&batch, prevention)
        .unwrap()
        .unwrap();
    batch.stage = u64::MAX;
    let saved = (state.clone(), batch.clone());
    assert_eq!(
        state.apply_prevention(&mut batch, prevention, rows),
        Err(DamageError::StageExhausted)
    );
    assert_eq!((&state, &batch), (&saved.0, &saved.1));
    let mut state = DamageState::default();
    state
        .add_multiplier(f.creator, f.permanent, 2.into(), DamageSourceFilter::Any)
        .unwrap();
    let mut batch = f.batch(&mut state, vec![f.input(0, f.creature, 3)]);
    let application = state.applicable(&batch)[0];
    batch.stage = u64::MAX;
    let saved = (state.clone(), batch.clone());
    assert_eq!(
        state.apply_multiplier(&mut batch, application),
        Err(DamageError::StageExhausted)
    );
    assert_eq!((&state, &batch), (&saved.0, &saved.1));
}

#[test]
fn player_effects_survive_creature_pruning_and_creator_exit_is_not_a_lifetime() {
    let f = Fixture::new();
    let mut state = DamageState::default();
    let player = DamageEffectTarget::Player(SeatId(1));
    let creature_shield = state.add_shield(f.creator, f.permanent, 2.into()).unwrap();
    let player_shield = state.add_shield(f.creator, player, 3.into()).unwrap();
    let creature_multiplier = state
        .add_multiplier(f.creator, f.permanent, 2.into(), DamageSourceFilter::Any)
        .unwrap();
    let player_multiplier = state
        .add_multiplier(f.creator, player, 3.into(), DamageSourceFilter::Any)
        .unwrap();
    let DamageRecipient::Creature(creature) = f.creature else {
        unreachable!()
    };
    state.retain_recipients(&BTreeSet::from([creature]));
    assert_eq!(state.shields().count(), 2);
    assert_eq!(state.multipliers().count(), 2);
    state.retain_recipients(&BTreeSet::new());
    assert!(state.shield(creature_shield.id).is_none());
    assert!(state.multiplier(creature_multiplier.id).is_none());
    assert_eq!(state.shield(player_shield.id), Some(&player_shield));
    assert_eq!(
        state.multiplier(player_multiplier.id),
        Some(&player_multiplier)
    );
}
