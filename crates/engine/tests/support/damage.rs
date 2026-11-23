//! Shared assertions for current native damage evidence across fixture families.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    damage::{
        ActiveDamageBatch, DamageApplication, DamageComponent, DamageEffectId, DamageJournal,
        DamageKind, DamageOrigin, DamageScope, DamageSource, PreventionApplication,
    },
    effects::DamageOrderChoice,
    game::{GameState, NativeCandidate},
    stack_window::StackKind,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn singleton(batch: &ActiveDamageBatch) -> &DamageComponent {
    assert_eq!(batch.components().count(), 1);
    batch.components().next().unwrap()
}

pub fn prevention_remaining(application: &PreventionApplication) -> &RulesInteger {
    assert_eq!(application.damage_remaining.len(), 1);
    application.damage_remaining.values().next().unwrap()
}

pub fn resolution_origin(
    frame: &DamageOrderChoice,
) -> (spellwake_engine::stack_window::StackEntryId, usize) {
    let DamageOrigin::Resolution { entry, instruction } = frame.origin() else {
        panic!("expected a resolution origin")
    };
    (*entry, *instruction)
}

/// Build either a current or deliberately inapplicable scoped reply. Eligibility
/// remains a kernel check, so negative tests can supply an unrelated effect ID.
pub fn application(game: &GameState, effect: impl Into<DamageEffectId>) -> DamageApplication {
    let batch = game.execution().pending_damage().unwrap();
    let component = singleton(batch);
    let effect = effect.into();
    let scope = match effect {
        DamageEffectId::EmblemCap { .. }
        | DamageEffectId::StaticPrevention(_)
        | DamageEffectId::StaticPreventionWithExile(_, _)
        | DamageEffectId::CombatDamageToMill(_, _)
        | DamageEffectId::RemovePlusOneToPreventDamage(_, _)
        | DamageEffectId::ShieldCounter(_)
        | DamageEffectId::StaticDouble(_)
        | DamageEffectId::DoubleDamageFromEquippedCreature(_)
        | DamageEffectId::DoubleDamageToEquippedCreature(_)
        | DamageEffectId::StaticHalfPrevention { .. }
        | DamageEffectId::UntilEndOfTurn { .. }
        | DamageEffectId::ProtectionMulticolored
        | DamageEffectId::ProtectionSubtype(_)
        | DamageEffectId::Protection(_)
        | DamageEffectId::Prevention(_) => DamageScope::Recipient(component.attempt().target),
        DamageEffectId::Multiplier(_) => DamageScope::Component(component.attempt().id),
    };
    DamageApplication { effect, scope }
}

pub fn used_effects(batch: &ActiveDamageBatch) -> Vec<DamageEffectId> {
    let component = singleton(batch);
    let mut used: BTreeSet<_> = component
        .used_multipliers()
        .iter()
        .copied()
        .map(DamageEffectId::Multiplier)
        .collect();
    for consumed in batch.consumed_shields() {
        assert_eq!(
            consumed.scope,
            DamageScope::Recipient(component.attempt().target)
        );
        used.insert(consumed.effect);
    }
    used.into_iter().collect()
}

/// Verify actual source and parent independently from the returned damage facts.
/// A resumed append joins its components to the accepted pending batch, since
/// its start/attempt records belong to the earlier successful candidate.
pub fn assert_transition_damage(before: &GameState, candidate: &NativeCandidate) {
    let journal = candidate.damage_journal();
    let mut attempts = BTreeMap::new();
    if let Some(batch) = before.execution().pending_damage() {
        for component in batch.components() {
            attempts.insert(component.attempt().id, component.attempt());
        }
    }
    for batch in &journal.batches {
        let DamageOrigin::Resolution { entry, .. } = batch.origin else {
            panic!("these fixture families execute noncombat instructions")
        };
        assert_eq!(batch.kind, DamageKind::Noncombat);
        let actual = before
            .execution()
            .window()
            .entries()
            .iter()
            .find(|actual| actual.id() == entry)
            .unwrap();
        assert_eq!(
            before.execution().window().entries().last().unwrap().id(),
            entry
        );
        let expected = match actual.kind() {
            StackKind::Speed | StackKind::Monarch | StackKind::DelayedCopy { .. } => {
                panic!("a source-free rules trigger cannot deal damage")
            }
            StackKind::Emblem { emblem } => DamageSource::Emblem {
                emblem: *emblem,
                controller: actual.controller(),
            },
            StackKind::CardSpell(object) => DamageSource::Object {
                object: before
                    .execution()
                    .window()
                    .cards()
                    .get(*object)
                    .unwrap()
                    .clone(),
                controller: actual.controller(),
            },
            StackKind::SpellCopy { .. } => DamageSource::SpellCopy {
                entry,
                controller: actual.controller(),
            },
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let object = before
                    .execution()
                    .window()
                    .cards()
                    .get(source.id())
                    .filter(|object| {
                        object.zone() == spellwake_engine::object_identity::ObjectZone::Battlefield
                    })
                    .unwrap_or(source)
                    .clone();
                let controller = object.controller().unwrap();
                DamageSource::Object { object, controller }
            }
        };
        let current: Vec<_> = journal
            .attempts
            .iter()
            .filter(|attempt| attempt.batch == batch.id)
            .collect();
        assert!(!current.is_empty());
        assert_eq!(
            batch.components,
            current.iter().map(|attempt| attempt.id).collect::<Vec<_>>()
        );
        let mut pairs = BTreeSet::new();
        for attempt in current {
            assert_eq!(attempt.source, expected);
            assert!(attempt.amount.is_positive());
            assert!(pairs.insert((attempt.source.id(), attempt.target)));
        }
    }
    for attempt in &journal.attempts {
        assert!(
            journal
                .batches
                .iter()
                .any(|batch| batch.id == attempt.batch)
        );
        assert!(attempts.insert(attempt.id, attempt).is_none());
    }
    for fact in &journal.applied {
        let original = attempts[&fact.attempt];
        assert_eq!(fact.batch, original.batch);
        assert_eq!(fact.source, original.source);
        assert_eq!(fact.target, original.target);
        assert!(fact.amount.is_positive());
    }
    for fact in &journal.completions {
        assert_eq!(fact.batch, attempts[&fact.attempt].batch);
    }
    for fact in &journal.preventions {
        let mut total = RulesInteger::from(0);
        let mut selected = BTreeSet::new();
        for allocation in &fact.allocations {
            let original = attempts[&allocation.attempt];
            assert_eq!(fact.batch, original.batch);
            assert_eq!(fact.target, original.target);
            assert!(allocation.amount.is_positive());
            assert!(selected.insert(allocation.attempt));
            total = total.plus(&allocation.amount);
        }
        assert_eq!(total, fact.amount);
    }
    for fact in &journal.multiplications {
        let original = attempts[&fact.attempt];
        assert_eq!(fact.batch, original.batch);
        assert_eq!(fact.target, original.target);
    }
}

/// A start record is emitted once; resumed appends are checked against the
/// accepted batch by assert_transition_damage instead of re-emitting that record.
pub fn assert_started_origin(
    journal: &DamageJournal,
    entry: spellwake_engine::stack_window::StackEntryId,
) {
    for batch in &journal.batches {
        assert!(
            matches!(batch.origin, DamageOrigin::Resolution { entry: actual, .. } if actual == entry)
        );
    }
}
