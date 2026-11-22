//! Static noncombat prevention participates in ordinary replacement ordering.
use super::*;
use crate::damage::DamageAttemptId;

impl EffectWindow {
    pub(super) fn static_prevention_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> BTreeMap<DamageApplication, Vec<DamageAttemptId>> {
        let mut out = BTreeMap::<_, Vec<_>>::new();
        for source in evaluated
            .objects
            .values()
            .filter(|source| source.zone == ObjectZone::Battlefield)
        {
            for rule in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
                .filter(|rule| {
                    source.abilities.continuous == Some(rule.id())
                        || source
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&rule.id())
                })
            {
                let Some(controller) = source.controller else {
                    continue;
                };
                let exiles = matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::PreventIncomingCombatAndExile,
                        ..
                    }
                );
                let removes_counter = matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::PreventIncomingRemovePlusOne,
                        ..
                    }
                );
                let mills = matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::ReplaceSubtypeCombatWithMill(
                                _
                            ),
                        ..
                    }
                );
                for component in batch.components().filter(|component| {
                    component.remaining().is_positive()
                        && (exiles || removes_counter || mills || component.can_prevent_damage())
                }) {
                    let target = component.attempt().target;
                    let protected = match rule {
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype), .. }
                        if batch.kind() == DamageKind::Combat && matches!(target, DamageRecipient::Player(_)) =>
                        matches!(component.attempt().source.id(), crate::damage::DamageSourceId::Object(id)
                            if evaluated.get(id).is_some_and(|dealer|
                                dealer.zone == ObjectZone::Battlefield && dealer.controller == Some(controller)
                                && dealer.current_creature_subtypes.contains(subtype))),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventIncomingRemovePlusOne, .. } =>
                        matches!(target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if id == source.object),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventIncomingCombatAndExile, .. } if batch.kind() == DamageKind::Combat =>
                        matches!(target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if id == source.object),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventEnchantedCreature { combat_only: true }, .. } if batch.kind() == DamageKind::Combat =>
                        matches!(source.attached_to, Some(TargetRef::Object(host))
                            if evaluated.get(host).is_some_and(|object| object.zone == ObjectZone::Battlefield && object.is_creature())
                                && (component.attempt().source.id() == crate::damage::DamageSourceId::Object(host)
                                    || target == DamageRecipient::Creature(host))),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventEnchantedCreature { combat_only: false }, .. } =>
                        matches!(source.attached_to, Some(TargetRef::Object(host))
                            if component.attempt().source.id() == crate::damage::DamageSourceId::Object(host)
                                && evaluated.get(host).is_some_and(|object| object.zone == ObjectZone::Battlefield && object.is_creature())),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventSourceCombat, .. } if batch.kind() == DamageKind::Combat =>
                        component.attempt().source.id() == crate::damage::DamageSourceId::Object(source.object)
                            || matches!(target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if id == source.object),
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventNoncombat, .. } if batch.kind() == DamageKind::Noncombat => match target {
                        DamageRecipient::Player(player) => player == controller,
                        DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) => id != source.object
                            && evaluated.get(id).is_some_and(|view| view.zone == ObjectZone::Battlefield && view.controller == Some(controller)),
                    },
                    crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::PreventCombatToAttackingArtifacts, .. } if batch.kind() == DamageKind::Combat =>
                        matches!(target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if evaluated.get(id)
                            .is_some_and(|view| view.zone == ObjectZone::Battlefield && view.controller == Some(controller)
                                && view.is_creature() && view.has_type(CardType::Artifact) && view.attacking)),
                    _ => false,
                };
                    if protected {
                        let application = DamageApplication {
                            effect: if mills {
                                DamageEffectId::CombatDamageToMill(source.object, rule.id())
                            } else if exiles {
                                DamageEffectId::StaticPreventionWithExile(source.object, rule.id())
                            } else if removes_counter {
                                DamageEffectId::RemovePlusOneToPreventDamage(
                                    source.object,
                                    rule.id(),
                                )
                            } else {
                                DamageEffectId::StaticPrevention(source.object)
                            },
                            scope: DamageScope::Recipient(target),
                        };
                        if !batch.consumed_shields().contains(&application) {
                            out.entry(application)
                                .or_default()
                                .push(component.attempt().id);
                        }
                    }
                }
            }
        }
        out
    }

    pub(super) fn emblem_cap_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> BTreeMap<DamageApplication, Vec<DamageAttemptId>> {
        let mut groups = BTreeMap::<DamageApplication, Vec<DamageAttemptId>>::new();
        for emblem in self.characteristics.emblems.values().filter(|emblem| {
            matches!(
                emblem.definition,
                crate::emblem::EmblemDefinition::PreventAllButOne
            )
        }) {
            for component in batch
                .components()
                .filter(|component| component.can_prevent_damage())
            {
                let target = component.attempt().target;
                let protected = match target {
                    DamageRecipient::Player(player) => player == emblem.controller,
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => evaluated.get(id).is_some_and(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.controller == Some(emblem.controller)
                            && object.has_type(CardType::Planeswalker)
                    }),
                };
                if protected {
                    let application = DamageApplication {
                        effect: DamageEffectId::EmblemCap {
                            emblem: emblem.id,
                            source: component.attempt().source.id(),
                        },
                        scope: DamageScope::Recipient(target),
                    };
                    if !batch.consumed_shields().contains(&application) {
                        groups
                            .entry(application)
                            .or_default()
                            .push(component.attempt().id);
                    }
                }
            }
        }
        groups.retain(|_, ids| {
            ids.iter()
                .filter_map(|id| batch.component(*id))
                .fold(RulesInteger::from(0), |sum, component| {
                    sum.plus(component.remaining())
                })
                > 1.into()
        });
        groups
    }
    pub(super) fn apply_emblem_cap(
        &mut self,
        application: DamageApplication,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (DamageEffectId::EmblemCap { emblem, source }, DamageScope::Recipient(recipient)) =
            (application.effect, application.scope)
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        let before = self.rules_snapshot(None)?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .emblem_cap_applications(&before.view().evaluated, batch)
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let allocations = work.batch.prevent_all_but_one(application, eligible)?;
        let batch = work.batch.id();
        let stage = work.batch.stage();
        self.emit_from(
            EventCause::DamageAdjusted { batch },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::EmblemDamagePrevented {
                    emblem,
                    source,
                    batch,
                    stage,
                    recipient,
                    allocations,
                }])
            },
        )
    }

    pub(super) fn apply_static_prevention(
        &mut self,
        application: DamageApplication,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let source = match application.effect {
            DamageEffectId::StaticPrevention(source)
            | DamageEffectId::StaticPreventionWithExile(source, _)
            | DamageEffectId::CombatDamageToMill(source, _)
            | DamageEffectId::RemovePlusOneToPreventDamage(source, _) => source,
            _ => return Err(EffectError::WrongDamageFrame),
        };
        let DamageScope::Recipient(recipient) = application.scope else {
            return Err(EffectError::WrongDamageFrame);
        };
        let before = self.rules_snapshot(None)?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .static_prevention_applications(&before.view().evaluated, batch)
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let allocations = match application.effect {
            DamageEffectId::CombatDamageToMill(..) => {
                let DamageRecipient::Player(player) = recipient else {
                    return Err(EffectError::WrongDamageFrame);
                };
                let allocations = work
                    .batch
                    .replace_damage_with_library_move(recipient, eligible)?;
                let amount = allocations
                    .iter()
                    .fold(0.into(), |total: RulesInteger, row| total.plus(&row.amount));
                work.followups.push_back(DamageFollowup::LibraryTop {
                    player,
                    amount,
                    mill: true,
                });
                let batch = work.batch.id();
                let stage = work.batch.stage();
                return self.emit_from(
                    EventCause::DamageAdjusted { batch },
                    &before,
                    None,
                    append,
                    |_, _, _| {
                        Ok(vec![RulesOccurrence::DamageReplacedWithMill {
                            source,
                            batch,
                            stage,
                            player,
                            allocations,
                        }])
                    },
                );
            }
            DamageEffectId::StaticPreventionWithExile(..) => {
                let player = before
                    .view()
                    .evaluated
                    .get(source)
                    .and_then(|view| view.controller)
                    .ok_or(EffectError::WrongDamageFrame)?;
                let (allocations, amount) = work
                    .batch
                    .prevent_all_with_followup(application, &eligible)?;
                work.followups.push_back(DamageFollowup::LibraryTop {
                    player,
                    amount,
                    mill: false,
                });
                allocations
            }
            DamageEffectId::RemovePlusOneToPreventDamage(..) => {
                let (allocations, _) = work
                    .batch
                    .prevent_all_with_followup(application, &eligible)?;
                work.followups
                    .push_back(DamageFollowup::RemovePlusOneCounter(source));
                allocations
            }
            _ => work.batch.prevent_all(recipient, eligible)?,
        };
        let batch = work.batch.id();
        let stage = work.batch.stage();
        self.emit_from(
            EventCause::DamageAdjusted { batch },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::StaticDamagePrevented {
                    source,
                    batch,
                    stage,
                    recipient,
                    allocations,
                }])
            },
        )
    }
}

impl EffectWindow {
    pub(super) fn static_damage_modifications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> Result<BTreeMap<DamageApplication, Vec<DamageAttemptId>>, EffectError> {
        let mut applications = BTreeMap::<_, Vec<_>>::new();
        for permanent in evaluated.objects.values() {
            let Some(controller) = permanent.controller else {
                continue;
            };
            let doubles = permanent.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule: crate::characteristics::StaticDamageRule::DoubleToOpponents,
                        ..
                    }
                )
            });
            let halves = permanent.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule: crate::characteristics::StaticDamageRule::PreventHalfIncoming,
                        ..
                    }
                )
            });
            let enchanted = match permanent.attached_to {
                Some(TargetRef::Player(player)) if permanent.has_battlefield_rule(|rule| {
                    matches!(rule, crate::characteristics::StaticContinuousAbility::DamageRule { rule: crate::characteristics::StaticDamageRule::DoubleToEnchantedPlayer, .. })
                }) => Some(player),
                _ => None,
            };
            if batch.kind() == DamageKind::Combat
                && permanent.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticContinuousAbility::DamageRule {
                            rule: crate::characteristics::StaticDamageRule::DoubleEquippedCombat,
                            ..
                        }
                    )
                })
                && let Some(TargetRef::Object(host)) = permanent.attached_to
                && evaluated
                    .get(host)
                    .is_some_and(|card| card.zone == ObjectZone::Battlefield && card.is_creature())
            {
                use crate::damage::DamageSourceId;
                for component in batch
                    .components()
                    .filter(|component| component.remaining().is_positive())
                {
                    let attempt = component.attempt();
                    let outgoing = attempt.source.id() == DamageSourceId::Object(host);
                    let incoming = matches!(attempt.source.id(), DamageSourceId::Object(source) if source != host)
                        && matches!(attempt.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if id == host);
                    for (matches, effect) in [
                        (
                            outgoing,
                            DamageEffectId::DoubleDamageFromEquippedCreature(permanent.object),
                        ),
                        (
                            incoming,
                            DamageEffectId::DoubleDamageToEquippedCreature(permanent.object),
                        ),
                    ] {
                        let application = DamageApplication {
                            effect,
                            scope: DamageScope::Component(attempt.id),
                        };
                        if matches && !batch.consumed_shields().contains(&application) {
                            applications
                                .entry(application)
                                .or_default()
                                .push(attempt.id);
                        }
                    }
                }
            }
            if !doubles && !halves && enchanted.is_none() {
                continue;
            }
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), controller)?;
            for component in batch
                .components()
                .filter(|component| component.remaining().is_positive())
            {
                let attempt = component.attempt();
                let affected = self.damage_recipient_controller_at(evaluated, attempt.target)?;
                let doubles_to_player = enchanted
                    .is_some_and(|player| attempt.target == DamageRecipient::Player(player));
                let application = if doubles_to_player || (doubles && opponents.contains(&affected))
                {
                    DamageApplication {
                        effect: DamageEffectId::StaticDouble(permanent.object),
                        scope: DamageScope::Component(attempt.id),
                    }
                } else if halves && affected == controller && component.can_prevent_damage() {
                    DamageApplication {
                        effect: DamageEffectId::StaticHalfPrevention {
                            permanent: permanent.object,
                            source: attempt.source.id(),
                        },
                        scope: DamageScope::Recipient(attempt.target),
                    }
                } else {
                    continue;
                };
                if !batch.consumed_shields().contains(&application) {
                    applications
                        .entry(application)
                        .or_default()
                        .push(attempt.id);
                }
            }
        }
        Ok(applications)
    }

    pub(super) fn apply_static_damage_modification(
        &mut self,
        application: DamageApplication,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let pending = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .static_damage_modifications(&before.view().evaluated, pending)?
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let batch = work.batch.id();
        let result = match (application.effect, application.scope) {
            (
                DamageEffectId::StaticDouble(source)
                | DamageEffectId::DoubleDamageFromEquippedCreature(source)
                | DamageEffectId::DoubleDamageToEquippedCreature(source),
                DamageScope::Component(attempt),
            ) => {
                let (before, after) = work.batch.double_static_component(application)?;
                RulesOccurrence::StaticDamageMultiplied {
                    source,
                    batch,
                    attempt,
                    stage: work.batch.stage(),
                    before,
                    after,
                }
            }
            (
                DamageEffectId::StaticHalfPrevention { permanent, .. },
                DamageScope::Recipient(recipient),
            ) => {
                let total = eligible
                    .iter()
                    .filter_map(|id| work.batch.component(*id))
                    .fold(RulesInteger::from(0), |sum, component| {
                        sum.plus(component.remaining())
                    });
                let allocations = work.batch.limit_damage(
                    application,
                    eligible,
                    total.nonnegative_half_rounded_down(),
                )?;
                RulesOccurrence::StaticDamagePrevented {
                    source: permanent,
                    batch,
                    recipient,
                    stage: work.batch.stage(),
                    allocations,
                }
            }
            _ => return Err(EffectError::WrongDamageFrame),
        };
        self.emit_from(
            EventCause::DamageAdjusted { batch },
            &before,
            None,
            append,
            |_, _, _| Ok(vec![result]),
        )
    }
}
