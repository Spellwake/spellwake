//! Unlimited prevention effects compete with other replacement effects for each recipient.
use super::*;
use crate::damage::{DamageAttemptId, TurnPreventionFilter};

impl EffectWindow {
    pub(super) fn turn_prevention_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> BTreeMap<DamageApplication, Vec<DamageAttemptId>> {
        let mut applications = BTreeMap::<_, Vec<_>>::new();
        for (&(creator, instruction), &(controller, filter, protected_source)) in
            &self.damage_state.turn_preventions
        {
            for component in batch.components().filter(|c| c.can_prevent_damage()) {
                let attempt = component.attempt();
                let eligible = match filter {
                    TurnPreventionFilter::ToAbilitySource | TurnPreventionFilter::ToTarget => {
                        matches!(
                            attempt.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if protected_source == Some(id)
                        )
                    }
                    TurnPreventionFilter::CombatToOrFromAbilitySource => {
                        batch.kind() == DamageKind::Combat
                            && (matches!(attempt.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id)
                            if protected_source == Some(id))
                                || matches!(attempt.source.id(), crate::damage::DamageSourceId::Object(id) if protected_source == Some(id)))
                    }
                    TurnPreventionFilter::CombatFromCreaturesOtherThanTarget => {
                        batch.kind() == DamageKind::Combat
                            && matches!(attempt.source.id(), crate::damage::DamageSourceId::Object(id) if Some(id) != protected_source
                            && evaluated.get(id).map_or_else(|| self.departed_damage_sources.get(&id).is_some_and(|source| source.was_creature),
                                |source| source.zone == ObjectZone::Battlefield && source.is_creature()))
                    }
                    TurnPreventionFilter::CombatToControlledCreatures => {
                        batch.kind() == DamageKind::Combat
                            && match attempt.target {
                                DamageRecipient::Creature(id)
                                | DamageRecipient::Planeswalker(id)
                                | DamageRecipient::Battle(id) => {
                                    evaluated.get(id).is_some_and(|p| {
                                        p.zone == ObjectZone::Battlefield
                                            && p.is_creature()
                                            && p.controller == Some(controller)
                                    })
                                }
                                DamageRecipient::Player(_) => false,
                            }
                    }
                    TurnPreventionFilter::Combat => batch.kind() == DamageKind::Combat,
                    TurnPreventionFilter::CombatFromCreaturesWithoutSubtype(first)
                    | TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(first, _) => {
                        let excluded = |types: &BTreeSet<_>| {
                            types.contains(&first)
                                || matches!(filter, TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(_, second)
                                    if types.contains(&second))
                        };
                        batch.kind() == DamageKind::Combat
                            && match attempt.source.id() {
                                crate::damage::DamageSourceId::Object(id) => {
                                    evaluated.get(id).map_or_else(
                                        || {
                                            self.departed_damage_sources.get(&id).is_some_and(
                                                |source| {
                                                    source.was_creature
                                                        && !excluded(&source.creature_subtypes)
                                                },
                                            )
                                        },
                                        |current| {
                                            current.zone == ObjectZone::Battlefield
                                                && current.is_creature()
                                                && !excluded(&current.current_creature_subtypes)
                                        },
                                    )
                                }
                                _ => false,
                            }
                    }
                    TurnPreventionFilter::CreaturesControlledByOpponents => {
                        let DamageSource::Object { object, .. } = &attempt.source else {
                            continue;
                        };
                        let source_controller = match evaluated.get(object.id()) {
                            Some(current)
                                if current.zone == ObjectZone::Battlefield
                                    && current.is_creature() =>
                            {
                                current.controller
                            }
                            Some(_) => None,
                            None => self
                                .departed_damage_sources
                                .get(&object.id())
                                .filter(|source| source.was_creature)
                                .and_then(|source| source.object.controller()),
                        };
                        source_controller.is_some_and(|player| {
                            player != controller && self.window.participants().contains(&player)
                        })
                    }
                };
                if eligible {
                    applications
                        .entry(DamageApplication {
                            effect: DamageEffectId::UntilEndOfTurn {
                                creator,
                                instruction,
                            },
                            scope: DamageScope::Recipient(attempt.target),
                        })
                        .or_default()
                        .push(attempt.id);
                }
            }
        }
        applications
    }

    pub(super) fn apply_turn_prevention(
        &mut self,
        application: DamageApplication,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (
            DamageEffectId::UntilEndOfTurn {
                creator,
                instruction,
            },
            DamageScope::Recipient(recipient),
        ) = (application.effect, application.scope)
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        let before = self.rules_snapshot(None)?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .turn_prevention_applications(&before.view().evaluated, batch)
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let allocations = work.batch.prevent_all(recipient, eligible)?;
        let batch = work.batch.id();
        let stage = work.batch.stage();
        self.emit_from(
            EventCause::DamageAdjusted { batch },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::TurnDamagePrevented {
                    creator,
                    instruction,
                    batch,
                    stage,
                    recipient,
                    allocations,
                }])
            },
        )
    }
}
