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
                        rule: crate::characteristics::StaticDamageRule::PreventIncomingRemovePlusOne
                            | crate::characteristics::StaticDamageRule::PreventIncomingRemoveNPlusOne,
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
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::PreventIncomingRemovePlusOne
                            | crate::characteristics::StaticDamageRule::PreventIncomingRemoveNPlusOne,
                        ..
                    } => matches!(
                        target,
                        DamageRecipient::Creature(id)
                            | DamageRecipient::Planeswalker(id)
                            | DamageRecipient::Battle(id)
                            if id == source.object
                    ),
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::PreventIncomingCombatToSource,
                        ..
                    } if batch.kind() == DamageKind::Combat => matches!(
                        target,
                        DamageRecipient::Creature(id)
                            | DamageRecipient::Planeswalker(id)
                            | DamageRecipient::Battle(id)
                            if id == source.object
                    ),
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
        // CR 122.1c: If a permanent with a shield counter on it would be dealt damage,
        // prevent that damage and remove a shield counter from it.
        for component in batch
            .components()
            .filter(|c| c.remaining().is_positive() && c.can_prevent_damage())
        {
            let target = component.attempt().target;
            let (DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id)) = target
            else {
                continue;
            };
            if evaluated
                .get(id)
                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                && self.counters_of_kind(id, CounterKind::Shield).is_positive()
            {
                let application = DamageApplication {
                    effect: DamageEffectId::ShieldCounter(id),
                    scope: DamageScope::Recipient(target),
                };
                if !batch.consumed_shields().contains(&application) {
                    out.entry(application)
                        .or_default()
                        .push(component.attempt().id);
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
            | DamageEffectId::RemovePlusOneToPreventDamage(source, _)
            | DamageEffectId::ShieldCounter(source) => source,
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
            DamageEffectId::RemovePlusOneToPreventDamage(_, rule_id) => {
                let (allocations, amount) = work
                    .batch
                    .prevent_all_with_followup(application, &eligible)?;
                let remove_n = before
                    .view()
                    .evaluated
                    .get(source)
                    .is_some_and(|perm| {
                        perm.printed_text
                            .continuous
                            .iter()
                            .chain(&perm.printed_text.additional_battlefield_abilities)
                            .any(|ab| {
                                matches!(
                                    ab,
                                    crate::characteristics::StaticContinuousAbility::DamageRule {
                                        id,
                                        rule: crate::characteristics::StaticDamageRule::PreventIncomingRemoveNPlusOne,
                                    } if *id == rule_id
                                )
                            })
                    });
                work.followups
                    .push_back(DamageFollowup::RemovePlusOneCounter {
                        object: source,
                        count: if remove_n { amount } else { 1.into() },
                    });
                allocations
            }
            DamageEffectId::ShieldCounter(..) => {
                let (allocations, _) = work
                    .batch
                    .prevent_all_with_followup(application, &eligible)?;
                work.followups
                    .push_back(DamageFollowup::RemoveShieldCounter(source));
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
            for ability in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active = permanent.abilities.continuous == Some(ability.id())
                    || permanent
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id());
                if !active {
                    continue;
                }
                match ability {
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        id: rule_id,
                        rule:
                            crate::characteristics::StaticDamageRule::RedirectPlayerDamageToSource {
                                include_other_permanents,
                            },
                    } => {
                        let dest_ok = permanent.is_creature()
                            || permanent.has_type(CardType::Planeswalker)
                            || permanent.has_type(CardType::Battle);
                        if !dest_ok {
                            continue;
                        }
                        for component in batch
                            .components()
                            .filter(|component| component.remaining().is_positive())
                        {
                            let attempt = component.attempt();
                            let matches_target = match attempt.target {
                                DamageRecipient::Player(p) => p == controller,
                                DamageRecipient::Creature(id)
                                | DamageRecipient::Planeswalker(id)
                                | DamageRecipient::Battle(id) => {
                                    *include_other_permanents
                                        && id != permanent.object
                                        && evaluated.get(id).is_some_and(|v| {
                                            v.zone == ObjectZone::Battlefield
                                                && v.controller == Some(controller)
                                        })
                                }
                            };
                            if !matches_target {
                                continue;
                            }
                            let application = DamageApplication {
                                effect: DamageEffectId::StaticModify(permanent.object, *rule_id),
                                scope: DamageScope::Component(attempt.id),
                            };
                            if !batch.consumed_shields().contains(&application) {
                                applications
                                    .entry(application)
                                    .or_default()
                                    .push(attempt.id);
                            }
                        }
                        continue;
                    }
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        id: rule_id,
                        rule:
                            crate::characteristics::StaticDamageRule::RedirectPlayerDamageToEnchanted,
                    } => {
                        let Some(TargetRef::Object(host)) = permanent.attached_to else {
                            continue;
                        };
                        if !evaluated
                            .get(host)
                            .is_some_and(|v| v.zone == ObjectZone::Battlefield && v.is_creature())
                        {
                            continue;
                        }
                        for component in batch
                            .components()
                            .filter(|component| component.remaining().is_positive())
                        {
                            let attempt = component.attempt();
                            if attempt.target != DamageRecipient::Player(controller) {
                                continue;
                            }
                            let application = DamageApplication {
                                effect: DamageEffectId::StaticModify(permanent.object, *rule_id),
                                scope: DamageScope::Component(attempt.id),
                            };
                            if !batch.consumed_shields().contains(&application) {
                                applications
                                    .entry(application)
                                    .or_default()
                                    .push(attempt.id);
                            }
                        }
                        continue;
                    }
                    crate::characteristics::StaticContinuousAbility::DamageRule {
                        id: rule_id,
                        rule:
                            crate::characteristics::StaticDamageRule::RedirectDamage {
                                source: source_filter,
                                from,
                                to,
                                combat_only,
                            },
                    } => {
                        if *combat_only && batch.kind() != DamageKind::Combat {
                            continue;
                        }
                        let opponents = self
                            .combat_rules
                            .opponents(self.window.participants(), controller)?;
                        let excluded = BTreeSet::new();
                        for component in batch
                            .components()
                            .filter(|component| component.remaining().is_positive())
                        {
                            let attempt = component.attempt();
                            if let Some(filter) = source_filter {
                                let source_ok = match &attempt.source {
                                    crate::damage::DamageSource::Object { object, .. } => {
                                        evaluated.get(object.id()).is_some_and(|view| {
                                            self.matches_object_filter(
                                                *filter,
                                                view,
                                                controller,
                                                Some(permanent.object),
                                                &excluded,
                                                evaluated,
                                            )
                                        })
                                    }
                                    _ => false,
                                };
                                if !source_ok {
                                    continue;
                                }
                            }
                            let affected =
                                self.damage_recipient_controller_at(evaluated, attempt.target)?;
                            let from_ok = self.damage_recipient_matches_static(
                                *from,
                                attempt.target,
                                affected,
                                controller,
                                Some(permanent.object),
                                &opponents,
                                evaluated,
                            );
                            if !from_ok {
                                continue;
                            }
                            let Some(dest) = Self::resolve_redirect_target(
                                *to,
                                controller,
                                Some(permanent.object),
                                None,
                                attempt.source.controller(),
                                evaluated,
                            ) else {
                                continue;
                            };
                            if dest == attempt.target {
                                continue;
                            }
                            let application = DamageApplication {
                                effect: DamageEffectId::StaticModify(permanent.object, *rule_id),
                                scope: DamageScope::Component(attempt.id),
                            };
                            if !batch.consumed_shields().contains(&application) {
                                applications
                                    .entry(application)
                                    .or_default()
                                    .push(attempt.id);
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
                let crate::characteristics::StaticContinuousAbility::DamageRule {
                    id: rule_id,
                    rule:
                        crate::characteristics::StaticDamageRule::ModifyDamage {
                            source: source_filter,
                            source_controller,
                            recipient,
                            kind,
                            op,
                        },
                } = ability
                else {
                    continue;
                };
                let kind_matches = match kind {
                    crate::characteristics::StaticDamageKind::Any => true,
                    crate::characteristics::StaticDamageKind::CombatOnly => {
                        batch.kind() == DamageKind::Combat
                    }
                    crate::characteristics::StaticDamageKind::NoncombatOnly => {
                        batch.kind() == DamageKind::Noncombat
                    }
                };
                if !kind_matches {
                    continue;
                }
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                let excluded = BTreeSet::new();
                for component in batch
                    .components()
                    .filter(|component| component.remaining().is_positive())
                {
                    if matches!(
                        op,
                        crate::characteristics::StaticDamageOp::PreventAll
                            | crate::characteristics::StaticDamageOp::PreventUpTo(_)
                    ) && !component.can_prevent_damage()
                    {
                        continue;
                    }
                    let attempt = component.attempt();
                    if matches!(
                        op,
                        crate::characteristics::StaticDamageOp::ReplaceWithMinusOneCounters
                    ) && !matches!(attempt.target, DamageRecipient::Creature(_))
                    {
                        continue;
                    }
                    let src_ctrl = attempt.source.controller();
                    let src_ctrl_ok = match source_controller {
                        crate::program::PlayerRelation::Controller => src_ctrl == controller,
                        crate::program::PlayerRelation::Opponent => opponents.contains(&src_ctrl),
                        crate::program::PlayerRelation::AnyParticipant => true,
                    };
                    if !src_ctrl_ok {
                        continue;
                    }
                    if let Some(filter) = source_filter {
                        let source_ok = match &attempt.source {
                            crate::damage::DamageSource::Object { object, .. } => {
                                evaluated.get(object.id()).is_some_and(|view| {
                                    self.matches_object_filter(
                                        *filter,
                                        view,
                                        controller,
                                        Some(permanent.object),
                                        &excluded,
                                        evaluated,
                                    )
                                })
                            }
                            crate::damage::DamageSource::Emblem { .. }
                            | crate::damage::DamageSource::SpellCopy { .. } => false,
                        };
                        if !source_ok {
                            continue;
                        }
                    }
                    let affected =
                        self.damage_recipient_controller_at(evaluated, attempt.target)?;
                    let recipient_ok = match recipient {
                        crate::characteristics::StaticDamageRecipient::Any => true,
                        crate::characteristics::StaticDamageRecipient::Opponents => {
                            matches!(attempt.target, DamageRecipient::Player(p) if opponents.contains(&p))
                        }
                        crate::characteristics::StaticDamageRecipient::OpponentOrOpponentPermanent => {
                            opponents.contains(&affected)
                        }
                        crate::characteristics::StaticDamageRecipient::Controller => {
                            attempt.target == DamageRecipient::Player(controller)
                        }
                        crate::characteristics::StaticDamageRecipient::ControllerOrControlledPermanent => {
                            affected == controller
                        }
                        crate::characteristics::StaticDamageRecipient::ControllerOrControlledCreatures => {
                            attempt.target == DamageRecipient::Player(controller)
                                || (matches!(attempt.target, DamageRecipient::Creature(_))
                                    && affected == controller)
                        }
                        crate::characteristics::StaticDamageRecipient::ControlledCreatures => {
                            matches!(attempt.target, DamageRecipient::Creature(_))
                                && affected == controller
                        }
                        crate::characteristics::StaticDamageRecipient::Creatures => {
                            matches!(attempt.target, DamageRecipient::Creature(_))
                        }
                        crate::characteristics::StaticDamageRecipient::OpponentCreatures => {
                            matches!(attempt.target, DamageRecipient::Creature(_))
                                && opponents.contains(&affected)
                        }
                        crate::characteristics::StaticDamageRecipient::Players => {
                            matches!(attempt.target, DamageRecipient::Player(_))
                        }
                        crate::characteristics::StaticDamageRecipient::SelfObject => {
                            matches!(
                                attempt.target,
                                DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id)
                                    if id == permanent.object
                            )
                        }
                        crate::characteristics::StaticDamageRecipient::AttachedHost => {
                            let target_obj = match attempt.target {
                                DamageRecipient::Creature(id)
                                | DamageRecipient::Planeswalker(id)
                                | DamageRecipient::Battle(id) => Some(id),
                                DamageRecipient::Player(_) => None,
                            };
                            matches!(
                                (permanent.attached_to, target_obj),
                                (Some(TargetRef::Object(host)), Some(obj)) if host == obj
                            )
                        }
                        crate::characteristics::StaticDamageRecipient::MatchingPermanent(filter) => {
                            let target_obj = match attempt.target {
                                DamageRecipient::Creature(id)
                                | DamageRecipient::Planeswalker(id)
                                | DamageRecipient::Battle(id) => Some(id),
                                DamageRecipient::Player(_) => None,
                            };
                            target_obj
                                .and_then(|obj| evaluated.get(obj))
                                .is_some_and(|view| {
                                    self.matches_object_filter(
                                        *filter,
                                        view,
                                        controller,
                                        Some(permanent.object),
                                        &excluded,
                                        evaluated,
                                    )
                                })
                        }
                    };
                    if !recipient_ok {
                        continue;
                    }
                    let application = DamageApplication {
                        effect: DamageEffectId::StaticModify(permanent.object, *rule_id),
                        scope: DamageScope::Component(attempt.id),
                    };
                    if !batch.consumed_shields().contains(&application) {
                        applications
                            .entry(application)
                            .or_default()
                            .push(attempt.id);
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
        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            let controller = active.controller;
            let excluded = BTreeSet::new();
            match &active.rule {
                crate::program::TemporaryRuleKind::TemporaryStatic(ability) => {
                    let crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::ModifyDamage {
                                source: source_filter,
                                source_controller,
                                recipient,
                                kind,
                                op,
                            },
                        ..
                    } = ability.as_ref()
                    else {
                        continue;
                    };
                    let kind_matches = match kind {
                        crate::characteristics::StaticDamageKind::Any => true,
                        crate::characteristics::StaticDamageKind::CombatOnly => {
                            batch.kind() == DamageKind::Combat
                        }
                        crate::characteristics::StaticDamageKind::NoncombatOnly => {
                            batch.kind() == DamageKind::Noncombat
                        }
                    };
                    if !kind_matches {
                        continue;
                    }
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    for component in batch
                        .components()
                        .filter(|component| component.remaining().is_positive())
                    {
                        if matches!(
                            op,
                            crate::characteristics::StaticDamageOp::PreventAll
                                | crate::characteristics::StaticDamageOp::PreventUpTo(_)
                        ) && !component.can_prevent_damage()
                        {
                            continue;
                        }
                        let attempt = component.attempt();
                        if matches!(
                            op,
                            crate::characteristics::StaticDamageOp::ReplaceWithMinusOneCounters
                        ) && !matches!(attempt.target, DamageRecipient::Creature(_))
                        {
                            continue;
                        }
                        let src_ctrl = attempt.source.controller();
                        let src_ctrl_ok = match source_controller {
                            crate::program::PlayerRelation::Controller => src_ctrl == controller,
                            crate::program::PlayerRelation::Opponent => {
                                opponents.contains(&src_ctrl)
                            }
                            crate::program::PlayerRelation::AnyParticipant => true,
                        };
                        if !src_ctrl_ok {
                            continue;
                        }
                        if let Some(filter) = source_filter {
                            let source_ok = match &attempt.source {
                                crate::damage::DamageSource::Object { object, .. } => {
                                    evaluated.get(object.id()).is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                                }
                                _ => false,
                            };
                            if !source_ok {
                                continue;
                            }
                        }
                        let affected =
                            self.damage_recipient_controller_at(evaluated, attempt.target)?;
                        let recipient_ok = match recipient {
                            crate::characteristics::StaticDamageRecipient::Any => true,
                            crate::characteristics::StaticDamageRecipient::Opponents => {
                                matches!(attempt.target, DamageRecipient::Player(p) if opponents.contains(&p))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentOrOpponentPermanent => {
                                opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Controller => {
                                attempt.target == DamageRecipient::Player(controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledPermanent => {
                                affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledCreatures => {
                                attempt.target == DamageRecipient::Player(controller)
                                    || (matches!(attempt.target, DamageRecipient::Creature(_))
                                        && affected == controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControlledCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::Creatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Players => {
                                matches!(attempt.target, DamageRecipient::Player(_))
                            }
                            crate::characteristics::StaticDamageRecipient::SelfObject => {
                                active.source_object.is_some_and(|src| {
                                    matches!(
                                        attempt.target,
                                        DamageRecipient::Creature(id)
                                            | DamageRecipient::Planeswalker(id)
                                            | DamageRecipient::Battle(id)
                                            if id == src
                                    )
                                })
                            }
                            crate::characteristics::StaticDamageRecipient::AttachedHost => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                active
                                    .source_object
                                    .and_then(|src| evaluated.get(src))
                                    .and_then(|perm| perm.attached_to)
                                    .is_some_and(|att| {
                                        matches!(
                                            (att, target_obj),
                                            (TargetRef::Object(host), Some(obj)) if host == obj
                                        )
                                    })
                            }
                            crate::characteristics::StaticDamageRecipient::MatchingPermanent(
                                filter,
                            ) => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                target_obj
                                    .and_then(|obj| evaluated.get(obj))
                                    .is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                            }
                        };
                        if !recipient_ok {
                            continue;
                        }
                        let application = DamageApplication {
                            effect: DamageEffectId::TemporaryStaticModify(active.id),
                            scope: DamageScope::Component(attempt.id),
                        };
                        if !batch.consumed_shields().contains(&application) {
                            applications
                                .entry(application)
                                .or_default()
                                .push(attempt.id);
                        }
                    }
                }
                crate::program::TemporaryRuleKind::PreventNextDamage {
                    amount,
                    source: source_filter,
                    recipient,
                    combat_only,
                } => {
                    if self.damage_state.prevention_disallowed_this_turn
                        || !amount.is_positive()
                        || (*combat_only && batch.kind() != DamageKind::Combat)
                    {
                        continue;
                    }
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    for component in batch
                        .components()
                        .filter(|component| component.remaining().is_positive())
                    {
                        if !component.can_prevent_damage() {
                            continue;
                        }
                        let attempt = component.attempt();
                        if let Some(filter) = source_filter {
                            let source_ok = match &attempt.source {
                                crate::damage::DamageSource::Object { object, .. } => {
                                    evaluated.get(object.id()).is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                                }
                                _ => false,
                            };
                            if !source_ok {
                                continue;
                            }
                        }
                        let affected =
                            self.damage_recipient_controller_at(evaluated, attempt.target)?;
                        let recipient_ok = match recipient {
                            crate::characteristics::StaticDamageRecipient::Any => true,
                            crate::characteristics::StaticDamageRecipient::Opponents => {
                                matches!(attempt.target, DamageRecipient::Player(p) if opponents.contains(&p))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentOrOpponentPermanent => {
                                opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Controller => {
                                attempt.target == DamageRecipient::Player(controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledPermanent => {
                                affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledCreatures => {
                                attempt.target == DamageRecipient::Player(controller)
                                    || (matches!(attempt.target, DamageRecipient::Creature(_))
                                        && affected == controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControlledCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::Creatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Players => {
                                matches!(attempt.target, DamageRecipient::Player(_))
                            }
                            crate::characteristics::StaticDamageRecipient::SelfObject => {
                                active.source_object.is_some_and(|src| {
                                    matches!(
                                        attempt.target,
                                        DamageRecipient::Creature(id)
                                            | DamageRecipient::Planeswalker(id)
                                            | DamageRecipient::Battle(id)
                                            if id == src
                                    )
                                })
                            }
                            crate::characteristics::StaticDamageRecipient::AttachedHost => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                active
                                    .source_object
                                    .and_then(|src| evaluated.get(src))
                                    .and_then(|perm| perm.attached_to)
                                    .is_some_and(|att| {
                                        matches!(
                                            (att, target_obj),
                                            (TargetRef::Object(host), Some(obj)) if host == obj
                                        )
                                    })
                            }
                            crate::characteristics::StaticDamageRecipient::MatchingPermanent(
                                filter,
                            ) => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                target_obj
                                    .and_then(|obj| evaluated.get(obj))
                                    .is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                            }
                        };
                        if !recipient_ok {
                            continue;
                        }
                        let application = DamageApplication {
                            effect: DamageEffectId::TemporaryPrevention(active.id),
                            scope: DamageScope::Component(attempt.id),
                        };
                        if !batch.consumed_shields().contains(&application) {
                            applications
                                .entry(application)
                                .or_default()
                                .push(attempt.id);
                        }
                    }
                }
                crate::program::TemporaryRuleKind::PreventNextTimeDamage {
                    source: source_filter,
                    source_chosen_color,
                    recipient,
                    combat_only,
                } => {
                    if self.damage_state.prevention_disallowed_this_turn
                        || (*combat_only && batch.kind() != DamageKind::Combat)
                    {
                        continue;
                    }
                    let chosen_color = if *source_chosen_color {
                        let Some(c) = active
                            .source_object
                            .and_then(|src| self.entry_color_choices.get(&src).copied())
                            .and_then(|m| match m {
                                crate::mana::ManaType::White => {
                                    Some(crate::characteristics::Color::White)
                                }
                                crate::mana::ManaType::Blue => {
                                    Some(crate::characteristics::Color::Blue)
                                }
                                crate::mana::ManaType::Black => {
                                    Some(crate::characteristics::Color::Black)
                                }
                                crate::mana::ManaType::Red => {
                                    Some(crate::characteristics::Color::Red)
                                }
                                crate::mana::ManaType::Green => {
                                    Some(crate::characteristics::Color::Green)
                                }
                                crate::mana::ManaType::Colourless => None,
                            })
                        else {
                            continue;
                        };
                        Some(c)
                    } else {
                        None
                    };
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    for component in batch
                        .components()
                        .filter(|component| component.remaining().is_positive())
                    {
                        if !component.can_prevent_damage() {
                            continue;
                        }
                        let attempt = component.attempt();
                        if let Some(color) = chosen_color {
                            let color_ok = match &attempt.source {
                                crate::damage::DamageSource::Object { object, .. } => evaluated
                                    .get(object.id())
                                    .is_some_and(|view| view.current_colors.contains(&color)),
                                _ => false,
                            };
                            if !color_ok {
                                continue;
                            }
                        }
                        if let Some(filter) = source_filter {
                            let source_ok = match &attempt.source {
                                crate::damage::DamageSource::Object { object, .. } => {
                                    evaluated.get(object.id()).is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                                }
                                _ => false,
                            };
                            if !source_ok {
                                continue;
                            }
                        }
                        let affected =
                            self.damage_recipient_controller_at(evaluated, attempt.target)?;
                        let recipient_ok = match recipient {
                            crate::characteristics::StaticDamageRecipient::Any => true,
                            crate::characteristics::StaticDamageRecipient::Opponents => {
                                matches!(attempt.target, DamageRecipient::Player(p) if opponents.contains(&p))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentOrOpponentPermanent => {
                                opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Controller => {
                                attempt.target == DamageRecipient::Player(controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledPermanent => {
                                affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::ControllerOrControlledCreatures => {
                                attempt.target == DamageRecipient::Player(controller)
                                    || (matches!(attempt.target, DamageRecipient::Creature(_))
                                        && affected == controller)
                            }
                            crate::characteristics::StaticDamageRecipient::ControlledCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && affected == controller
                            }
                            crate::characteristics::StaticDamageRecipient::Creatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                            }
                            crate::characteristics::StaticDamageRecipient::OpponentCreatures => {
                                matches!(attempt.target, DamageRecipient::Creature(_))
                                    && opponents.contains(&affected)
                            }
                            crate::characteristics::StaticDamageRecipient::Players => {
                                matches!(attempt.target, DamageRecipient::Player(_))
                            }
                            crate::characteristics::StaticDamageRecipient::SelfObject => {
                                active.source_object.is_some_and(|src| {
                                    matches!(
                                        attempt.target,
                                        DamageRecipient::Creature(id)
                                            | DamageRecipient::Planeswalker(id)
                                            | DamageRecipient::Battle(id)
                                            if id == src
                                    )
                                })
                            }
                            crate::characteristics::StaticDamageRecipient::AttachedHost => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                active
                                    .source_object
                                    .and_then(|src| evaluated.get(src))
                                    .and_then(|perm| perm.attached_to)
                                    .is_some_and(|att| {
                                        matches!(
                                            (att, target_obj),
                                            (TargetRef::Object(host), Some(obj)) if host == obj
                                        )
                                    })
                            }
                            crate::characteristics::StaticDamageRecipient::MatchingPermanent(
                                filter,
                            ) => {
                                let target_obj = match attempt.target {
                                    DamageRecipient::Creature(id)
                                    | DamageRecipient::Planeswalker(id)
                                    | DamageRecipient::Battle(id) => Some(id),
                                    DamageRecipient::Player(_) => None,
                                };
                                target_obj
                                    .and_then(|obj| evaluated.get(obj))
                                    .is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                            }
                        };
                        if !recipient_ok {
                            continue;
                        }
                        let application = DamageApplication {
                            effect: DamageEffectId::TemporaryPrevention(active.id),
                            scope: DamageScope::Component(attempt.id),
                        };
                        if !batch.consumed_shields().contains(&application) {
                            applications
                                .entry(application)
                                .or_default()
                                .push(attempt.id);
                        }
                    }
                }
                crate::program::TemporaryRuleKind::RedirectNextDamage {
                    source: source_filter,
                    from,
                    to,
                    combat_only,
                    amount,
                    ..
                } => {
                    if amount.as_ref().is_some_and(|a| !a.is_positive())
                        || (*combat_only && batch.kind() != DamageKind::Combat)
                    {
                        continue;
                    }
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let resolved_target_obj = active.resolved_objects.iter().next().copied();
                    for component in batch
                        .components()
                        .filter(|component| component.remaining().is_positive())
                    {
                        let attempt = component.attempt();
                        if let Some(filter) = source_filter {
                            let source_ok = match &attempt.source {
                                crate::damage::DamageSource::Object { object, .. } => {
                                    evaluated.get(object.id()).is_some_and(|view| {
                                        self.matches_object_filter(
                                            *filter,
                                            view,
                                            controller,
                                            active.source_object,
                                            &excluded,
                                            evaluated,
                                        )
                                    })
                                }
                                _ => false,
                            };
                            if !source_ok {
                                continue;
                            }
                        }
                        let affected =
                            self.damage_recipient_controller_at(evaluated, attempt.target)?;
                        let from_ok = self.damage_recipient_matches_static(
                            *from,
                            attempt.target,
                            affected,
                            controller,
                            active.source_object,
                            &opponents,
                            evaluated,
                        );
                        if !from_ok {
                            continue;
                        }
                        let Some(dest) = Self::resolve_redirect_target(
                            *to,
                            controller,
                            active.source_object,
                            resolved_target_obj,
                            attempt.source.controller(),
                            evaluated,
                        ) else {
                            continue;
                        };
                        if dest == attempt.target {
                            continue;
                        }
                        let application = DamageApplication {
                            effect: DamageEffectId::TemporaryStaticModify(active.id),
                            scope: DamageScope::Component(attempt.id),
                        };
                        if !batch.consumed_shields().contains(&application) {
                            applications
                                .entry(application)
                                .or_default()
                                .push(attempt.id);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(applications)
    }

    #[allow(clippy::too_many_arguments)]
    fn damage_recipient_matches_static(
        &self,
        recipient: crate::characteristics::StaticDamageRecipient,
        target: DamageRecipient,
        affected: SeatId,
        controller: SeatId,
        source_object: Option<ObjectId>,
        opponents: &[SeatId],
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        let excluded = BTreeSet::new();
        match recipient {
            crate::characteristics::StaticDamageRecipient::Any => true,
            crate::characteristics::StaticDamageRecipient::Opponents => {
                matches!(target, DamageRecipient::Player(p) if opponents.contains(&p))
            }
            crate::characteristics::StaticDamageRecipient::OpponentOrOpponentPermanent => {
                opponents.contains(&affected)
            }
            crate::characteristics::StaticDamageRecipient::Controller => {
                target == DamageRecipient::Player(controller)
            }
            crate::characteristics::StaticDamageRecipient::ControllerOrControlledPermanent => {
                affected == controller
            }
            crate::characteristics::StaticDamageRecipient::ControllerOrControlledCreatures => {
                target == DamageRecipient::Player(controller)
                    || (matches!(target, DamageRecipient::Creature(_)) && affected == controller)
            }
            crate::characteristics::StaticDamageRecipient::ControlledCreatures => {
                matches!(target, DamageRecipient::Creature(_)) && affected == controller
            }
            crate::characteristics::StaticDamageRecipient::Creatures => {
                matches!(target, DamageRecipient::Creature(_))
            }
            crate::characteristics::StaticDamageRecipient::OpponentCreatures => {
                matches!(target, DamageRecipient::Creature(_)) && opponents.contains(&affected)
            }
            crate::characteristics::StaticDamageRecipient::Players => {
                matches!(target, DamageRecipient::Player(_))
            }
            crate::characteristics::StaticDamageRecipient::SelfObject => {
                source_object.is_some_and(|src| {
                    matches!(
                        target,
                        DamageRecipient::Creature(id)
                            | DamageRecipient::Planeswalker(id)
                            | DamageRecipient::Battle(id)
                            if id == src
                    )
                })
            }
            crate::characteristics::StaticDamageRecipient::AttachedHost => {
                let target_obj = match target {
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => Some(id),
                    DamageRecipient::Player(_) => None,
                };
                source_object
                    .and_then(|src| evaluated.get(src))
                    .and_then(|perm| perm.attached_to)
                    .is_some_and(|att| {
                        matches!(
                            (att, target_obj),
                            (TargetRef::Object(host), Some(obj)) if host == obj
                        )
                    })
            }
            crate::characteristics::StaticDamageRecipient::MatchingPermanent(filter) => {
                let target_obj = match target {
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => Some(id),
                    DamageRecipient::Player(_) => None,
                };
                target_obj
                    .and_then(|obj| evaluated.get(obj))
                    .is_some_and(|view| {
                        self.matches_object_filter(
                            filter,
                            view,
                            controller,
                            source_object,
                            &excluded,
                            evaluated,
                        )
                    })
            }
        }
    }

    fn resolve_redirect_target(
        to: crate::characteristics::DamageRedirectTarget,
        controller: SeatId,
        source_object: Option<ObjectId>,
        target_object: Option<ObjectId>,
        damage_source_controller: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<DamageRecipient> {
        let obj_recipient = |id: ObjectId| -> Option<DamageRecipient> {
            let perm = evaluated.get(id)?;
            if perm.zone != ObjectZone::Battlefield {
                return None;
            }
            if perm.is_creature() {
                Some(DamageRecipient::Creature(id))
            } else if perm.has_type(CardType::Planeswalker) {
                Some(DamageRecipient::Planeswalker(id))
            } else if perm.has_type(CardType::Battle) {
                Some(DamageRecipient::Battle(id))
            } else {
                None
            }
        };
        match to {
            crate::characteristics::DamageRedirectTarget::AbilitySource => {
                source_object.and_then(obj_recipient)
            }
            crate::characteristics::DamageRedirectTarget::AttachedHost => source_object
                .and_then(|src| evaluated.get(src))
                .and_then(|perm| perm.attached_to)
                .and_then(|att| match att {
                    TargetRef::Object(host) => obj_recipient(host),
                    TargetRef::Player(p) => Some(DamageRecipient::Player(p)),
                    TargetRef::Stack(_) => None,
                }),
            crate::characteristics::DamageRedirectTarget::Controller => {
                Some(DamageRecipient::Player(controller))
            }
            crate::characteristics::DamageRedirectTarget::TargetObject => {
                target_object.and_then(obj_recipient)
            }
            crate::characteristics::DamageRedirectTarget::DamageSourceController => {
                Some(DamageRecipient::Player(damage_source_controller))
            }
        }
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
            (DamageEffectId::StaticModify(source, rule_id), DamageScope::Component(attempt)) => {
                let damage_rule = before
                    .view()
                    .evaluated
                    .get(source)
                    .and_then(|perm| {
                        perm.printed_text
                            .continuous
                            .iter()
                            .chain(&perm.printed_text.additional_battlefield_abilities)
                            .find_map(|ab| match ab {
                                crate::characteristics::StaticContinuousAbility::DamageRule {
                                    id,
                                    rule,
                                } if *id == rule_id => Some(rule.clone()),
                                _ => None,
                            })
                    })
                    .ok_or(DamageError::InapplicableEffect)?;
                if let crate::characteristics::StaticDamageRule::RedirectDamage { to, .. } =
                    &damage_rule
                {
                    let view = before.view();
                    let source_ctrl = view
                        .evaluated
                        .get(source)
                        .and_then(|p| p.controller)
                        .ok_or(DamageError::InapplicableEffect)?;
                    let src_ctrl = work
                        .batch
                        .component(attempt)
                        .ok_or(DamageError::InapplicableEffect)?
                        .attempt()
                        .source
                        .controller();
                    let new_target = Self::resolve_redirect_target(
                        *to,
                        source_ctrl,
                        Some(source),
                        None,
                        src_ctrl,
                        &view.evaluated,
                    )
                    .ok_or(DamageError::InapplicableEffect)?;
                    work.batch
                        .redirect_static_component(application, new_target)?;
                    return Ok(());
                }
                if let crate::characteristics::StaticDamageRule::RedirectPlayerDamageToSource {
                    ..
                } = damage_rule
                {
                    let view = before.view();
                    let perm = view
                        .evaluated
                        .get(source)
                        .ok_or(DamageError::InapplicableEffect)?;
                    let new_target = if perm.is_creature() {
                        DamageRecipient::Creature(source)
                    } else if perm.has_type(CardType::Planeswalker) {
                        DamageRecipient::Planeswalker(source)
                    } else {
                        DamageRecipient::Battle(source)
                    };
                    work.batch
                        .redirect_static_component(application, new_target)?;
                    return Ok(());
                }
                if let crate::characteristics::StaticDamageRule::RedirectPlayerDamageToEnchanted =
                    damage_rule
                {
                    let Some(TargetRef::Object(host)) = before
                        .view()
                        .evaluated
                        .get(source)
                        .and_then(|perm| perm.attached_to)
                    else {
                        return Err(DamageError::InapplicableEffect.into());
                    };
                    work.batch
                        .redirect_static_component(application, DamageRecipient::Creature(host))?;
                    return Ok(());
                }
                let crate::characteristics::StaticDamageRule::ModifyDamage { op, .. } = damage_rule
                else {
                    return Err(DamageError::InapplicableEffect.into());
                };
                let target_recipient = work
                    .batch
                    .component(attempt)
                    .ok_or(DamageError::InapplicableEffect)?
                    .attempt()
                    .target;
                match op {
                    crate::characteristics::StaticDamageOp::Add(delta) => {
                        let (before_amt, after_amt) =
                            work.batch
                                .modify_static_component(application, i64::from(delta), 1)?;
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::AddExpr(expr) => {
                        let view = before.view();
                        let ctrl = view
                            .evaluated
                            .get(source)
                            .and_then(|p| p.controller)
                            .ok_or(DamageError::InapplicableEffect)?;
                        let ctx = crate::characteristics::WorldEvalCtx {
                            active_player: self.window.active_player(),
                            participants: self.window.participants(),
                            life: Some(&self.life),
                            combat: self.combat.as_ref(),
                            cards: self.window.cards(),
                            programs: &self.programs,
                            state: &self.characteristics,
                            objects: &view.evaluated.objects,
                            source,
                            controller: ctrl,
                            name_rules: &self.name_rules,
                            trigger_subject: None,
                            related_player: None,
                            defending_player: None,
                        };
                        let val = crate::characteristics::eval_world_value_expr(&ctx, &expr);
                        let delta = if val.is_positive() {
                            i64::try_from(val.to_usize().unwrap_or(usize::MAX)).unwrap_or(i64::MAX)
                        } else {
                            0
                        };
                        let (before_amt, after_amt) =
                            work.batch.modify_static_component(application, delta, 1)?;
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::Subtract(delta) => {
                        let (before_amt, after_amt) = work.batch.modify_static_component(
                            application,
                            -i64::from(delta),
                            1,
                        )?;
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::CapAt(cap) => {
                        let rem = work
                            .batch
                            .component(attempt)
                            .ok_or(DamageError::InapplicableEffect)?
                            .remaining()
                            .clone();
                        let cap_ri = RulesInteger::from(i64::from(cap));
                        let diff = if rem > cap_ri {
                            rem.minus(&cap_ri).to_usize().unwrap_or(usize::MAX)
                        } else {
                            0
                        };
                        let delta = -i64::try_from(diff).unwrap_or(i64::MAX);
                        let (before_amt, after_amt) =
                            work.batch.modify_static_component(application, delta, 1)?;
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::Multiply(factor) => {
                        let (before_amt, after_amt) = work.batch.modify_static_component(
                            application,
                            0,
                            i64::from(factor),
                        )?;
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::PreventAll => {
                        let allocations =
                            work.batch.limit_damage(application, eligible, 0.into())?;
                        RulesOccurrence::StaticDamagePrevented {
                            source,
                            batch,
                            recipient: target_recipient,
                            stage: work.batch.stage(),
                            allocations,
                        }
                    }
                    crate::characteristics::StaticDamageOp::PreventUpTo(delta) => {
                        let rem = work
                            .batch
                            .component(attempt)
                            .ok_or(DamageError::InapplicableEffect)?
                            .remaining()
                            .clone();
                        let leave = rem
                            .minus(&RulesInteger::from(i64::from(delta)))
                            .max(0.into());
                        let allocations = work.batch.limit_damage(application, eligible, leave)?;
                        RulesOccurrence::StaticDamagePrevented {
                            source,
                            batch,
                            recipient: target_recipient,
                            stage: work.batch.stage(),
                            allocations,
                        }
                    }
                    crate::characteristics::StaticDamageOp::ReplaceWithMinusOneCounters => {
                        let DamageRecipient::Creature(creature_id) = target_recipient else {
                            return Err(DamageError::InapplicableEffect.into());
                        };
                        let replaced = work.batch.replace_static_component_all(application)?;
                        let raw = crate::characteristics::PowerToughnessCounters {
                            plus_one: 0.into(),
                            minus_one: replaced,
                        };
                        let added = self.adjusted_pt_counters(
                            &before.view().evaluated,
                            creature_id,
                            raw,
                            None,
                        )?;
                        let prev = self.characteristics.counters(creature_id);
                        self.characteristics.place_counters(creature_id, &added)?;
                        let after_c = self.characteristics.counters(creature_id);
                        return self.emit_from(
                            EventCause::DamageAdjusted { batch },
                            &before,
                            None,
                            append,
                            |old, new, _| {
                                Ok(vec![RulesOccurrence::CountersPlaced {
                                    object: creature_id,
                                    before_appearance: Box::new(
                                        old.appearance(creature_id)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                    after_appearance: Box::new(
                                        new.appearance(creature_id)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                    before: prev.clone(),
                                    added: added.clone(),
                                    after: after_c.clone(),
                                }])
                            },
                        );
                    }
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
            (DamageEffectId::TemporaryStaticModify(rule_id), DamageScope::Component(attempt)) => {
                enum TempDamageAction {
                    Modify(
                        Option<ObjectId>,
                        SeatId,
                        crate::characteristics::StaticDamageOp,
                    ),
                    Redirect {
                        controller: SeatId,
                        source_object: Option<ObjectId>,
                        target_object: Option<ObjectId>,
                        to: crate::characteristics::DamageRedirectTarget,
                        one_shot: bool,
                    },
                }
                let action = self
                    .temporary_rules
                    .iter()
                    .find_map(|active| {
                        if active.id != rule_id || active.consumed {
                            return None;
                        }
                        match &active.rule {
                            crate::program::TemporaryRuleKind::TemporaryStatic(ability) => {
                                if let crate::characteristics::StaticContinuousAbility::DamageRule {
                                    rule:
                                        crate::characteristics::StaticDamageRule::ModifyDamage {
                                            op,
                                            ..
                                        },
                                    ..
                                } = ability.as_ref()
                                {
                                    Some(TempDamageAction::Modify(
                                        active.source_object,
                                        active.controller,
                                        op.clone(),
                                    ))
                                } else {
                                    None
                                }
                            }
                            crate::program::TemporaryRuleKind::RedirectNextDamage {
                                to,
                                one_shot,
                                ..
                            } => Some(TempDamageAction::Redirect {
                                controller: active.controller,
                                source_object: active.source_object,
                                target_object: active.resolved_objects.iter().next().copied(),
                                to: *to,
                                one_shot: *one_shot,
                            }),
                            _ => None,
                        }
                    })
                    .ok_or(DamageError::InapplicableEffect)?;
                let (source_obj, rule_ctrl, op) = match action {
                    TempDamageAction::Redirect {
                        controller: r_ctrl,
                        source_object,
                        target_object,
                        to,
                        one_shot,
                    } => {
                        let view = before.view();
                        let src_ctrl = work
                            .batch
                            .component(attempt)
                            .ok_or(DamageError::InapplicableEffect)?
                            .attempt()
                            .source
                            .controller();
                        let new_target = Self::resolve_redirect_target(
                            to,
                            r_ctrl,
                            source_object,
                            target_object,
                            src_ctrl,
                            &view.evaluated,
                        )
                        .ok_or(DamageError::InapplicableEffect)?;
                        work.batch
                            .redirect_static_component(application, new_target)?;
                        if one_shot
                            && let Some(active) =
                                self.temporary_rules.iter_mut().find(|a| a.id == rule_id)
                        {
                            active.consumed = true;
                        }
                        return Ok(());
                    }
                    TempDamageAction::Modify(s, c, op) => (s, c, op),
                };
                let target_recipient = work
                    .batch
                    .component(attempt)
                    .ok_or(DamageError::InapplicableEffect)?
                    .attempt()
                    .target;
                match op {
                    crate::characteristics::StaticDamageOp::Add(delta) => {
                        let (before_amt, after_amt) =
                            work.batch
                                .modify_static_component(application, i64::from(delta), 1)?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::AddExpr(expr) => {
                        let delta = if let Some(src) = source_obj {
                            let view = before.view();
                            let ctx = crate::characteristics::WorldEvalCtx {
                                active_player: self.window.active_player(),
                                participants: self.window.participants(),
                                life: Some(&self.life),
                                combat: self.combat.as_ref(),
                                cards: self.window.cards(),
                                programs: &self.programs,
                                state: &self.characteristics,
                                objects: &view.evaluated.objects,
                                source: src,
                                controller: rule_ctrl,
                                name_rules: &self.name_rules,
                                trigger_subject: None,
                                related_player: None,
                                defending_player: None,
                            };
                            let val = crate::characteristics::eval_world_value_expr(&ctx, &expr);
                            if val.is_positive() {
                                i64::try_from(val.to_usize().unwrap_or(usize::MAX))
                                    .unwrap_or(i64::MAX)
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        let (before_amt, after_amt) =
                            work.batch.modify_static_component(application, delta, 1)?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::Subtract(delta) => {
                        let (before_amt, after_amt) = work.batch.modify_static_component(
                            application,
                            -i64::from(delta),
                            1,
                        )?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::CapAt(cap) => {
                        let rem = work
                            .batch
                            .component(attempt)
                            .ok_or(DamageError::InapplicableEffect)?
                            .remaining()
                            .clone();
                        let cap_ri = RulesInteger::from(i64::from(cap));
                        let diff = if rem > cap_ri {
                            rem.minus(&cap_ri).to_usize().unwrap_or(usize::MAX)
                        } else {
                            0
                        };
                        let delta = -i64::try_from(diff).unwrap_or(i64::MAX);
                        let (before_amt, after_amt) =
                            work.batch.modify_static_component(application, delta, 1)?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::Multiply(factor) => {
                        let (before_amt, after_amt) = work.batch.modify_static_component(
                            application,
                            0,
                            i64::from(factor),
                        )?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamageMultiplied {
                            source,
                            batch,
                            attempt,
                            stage: work.batch.stage(),
                            before: before_amt,
                            after: after_amt,
                        }
                    }
                    crate::characteristics::StaticDamageOp::PreventAll => {
                        let allocations =
                            work.batch.limit_damage(application, eligible, 0.into())?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamagePrevented {
                            source,
                            batch,
                            recipient: target_recipient,
                            stage: work.batch.stage(),
                            allocations,
                        }
                    }
                    crate::characteristics::StaticDamageOp::PreventUpTo(delta) => {
                        let rem = work
                            .batch
                            .component(attempt)
                            .ok_or(DamageError::InapplicableEffect)?
                            .remaining()
                            .clone();
                        let leave = rem
                            .minus(&RulesInteger::from(i64::from(delta)))
                            .max(0.into());
                        let allocations = work.batch.limit_damage(application, eligible, leave)?;
                        let Some(source) = source_obj else {
                            return Ok(());
                        };
                        RulesOccurrence::StaticDamagePrevented {
                            source,
                            batch,
                            recipient: target_recipient,
                            stage: work.batch.stage(),
                            allocations,
                        }
                    }
                    crate::characteristics::StaticDamageOp::ReplaceWithMinusOneCounters => {
                        let DamageRecipient::Creature(creature_id) = target_recipient else {
                            return Err(DamageError::InapplicableEffect.into());
                        };
                        let replaced = work.batch.replace_static_component_all(application)?;
                        let raw = crate::characteristics::PowerToughnessCounters {
                            plus_one: 0.into(),
                            minus_one: replaced,
                        };
                        let added = self.adjusted_pt_counters(
                            &before.view().evaluated,
                            creature_id,
                            raw,
                            None,
                        )?;
                        let prev = self.characteristics.counters(creature_id);
                        self.characteristics.place_counters(creature_id, &added)?;
                        let after_c = self.characteristics.counters(creature_id);
                        return self.emit_from(
                            EventCause::DamageAdjusted { batch },
                            &before,
                            None,
                            append,
                            |old, new, _| {
                                Ok(vec![RulesOccurrence::CountersPlaced {
                                    object: creature_id,
                                    before_appearance: Box::new(
                                        old.appearance(creature_id)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                    after_appearance: Box::new(
                                        new.appearance(creature_id)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                    before: prev.clone(),
                                    added: added.clone(),
                                    after: after_c.clone(),
                                }])
                            },
                        );
                    }
                }
            }
            (DamageEffectId::TemporaryPrevention(rule_id), DamageScope::Component(attempt)) => {
                let target_recipient = work
                    .batch
                    .component(attempt)
                    .ok_or(DamageError::InapplicableEffect)?
                    .attempt()
                    .target;
                let rem = work
                    .batch
                    .component(attempt)
                    .ok_or(DamageError::InapplicableEffect)?
                    .remaining()
                    .clone();
                let (source_obj, shield_amount) = self
                    .temporary_rules
                    .iter()
                    .find_map(|active| {
                        if active.id != rule_id || active.consumed {
                            return None;
                        }
                        match &active.rule {
                            crate::program::TemporaryRuleKind::PreventNextDamage {
                                amount, ..
                            } if amount.is_positive() => {
                                Some((active.source_object, Some(amount.clone())))
                            }
                            crate::program::TemporaryRuleKind::PreventNextTimeDamage { .. } => {
                                Some((active.source_object, None))
                            }
                            _ => None,
                        }
                    })
                    .ok_or(DamageError::InapplicableEffect)?;
                let prevented = match shield_amount {
                    Some(cap) => rem.clone().min(cap),
                    None => rem.clone(),
                };
                let leave = rem.minus(&prevented);
                let allocations = work.batch.limit_damage(application, eligible, leave)?;
                let stage = work.batch.stage();
                if let Some(active) = self
                    .temporary_rules
                    .iter_mut()
                    .find(|active| active.id == rule_id)
                {
                    match &mut active.rule {
                        crate::program::TemporaryRuleKind::PreventNextDamage { amount, .. } => {
                            *amount = amount.minus(&prevented).max(0.into());
                            if !amount.is_positive() {
                                active.consumed = true;
                            }
                        }
                        crate::program::TemporaryRuleKind::PreventNextTimeDamage { .. } => {
                            active.consumed = true;
                        }
                        _ => {}
                    }
                }
                let Some(source) = source_obj else {
                    return Ok(());
                };
                RulesOccurrence::StaticDamagePrevented {
                    source,
                    batch,
                    recipient: target_recipient,
                    stage,
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
