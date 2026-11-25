//! Mana ability activation, payment, and production.
use super::*;

impl EffectWindow {
    #[inline(never)]
    pub(super) fn begin_mana_with_parent_inner(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        authorized_parent: Option<ManaParent>,
        choices: ManaActivationChoices<'_>,
    ) -> Result<ManaActivationProgress, EffectError> {
        let ManaActivationChoices {
            payment,
            bundle,
            cost_objects,
            recipient,
            restricted,
        } = choices;
        let restricted = restricted.unwrap_or(&[]);
        let revision = self.next_revision(expected)?;
        if !self.is_paying_attack_cost()
            && !matches!(
                self.pending_work,
                Some(PendingWork::ResolutionPayment(_) | PendingWork::CastRecovery(_))
            )
        {
            self.ensure_resolution_or_idle()?;
        }
        let card = self
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::UnknownObject)?;
        let evaluated = self.evaluated_characteristics()?;
        let current = evaluated
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?;
        self.check_opponents_turn_restriction(actor, &evaluated, Some(current))?;
        Self::check_artifact_activation_restriction(&evaluated, current)?;
        if current.suppressed_activations
            == Some(crate::characteristics::ActivationSuppression::All)
            || self.temporary_rules.iter().any(|active| {
                if active.consumed {
                    return false;
                }
                if let crate::program::TemporaryRuleKind::CantActivateAbilities {
                    nonmana_only: false,
                    ..
                } = &active.rule
                {
                    active.resolved_players.contains(&actor)
                } else if let Some(crate::characteristics::StaticContinuousAbility::Restriction {
                    rule:
                        crate::characteristics::StaticRestriction::PreventMatchingActivations(
                            selector,
                        ),
                    ..
                }) = self.active_temporary_static(active, &evaluated)
                {
                    crate::characteristics::selector_matches_permanent(
                        self.window.cards(),
                        &evaluated,
                        active.source_object.unwrap_or(ObjectId::SENTINEL),
                        active.controller,
                        selector,
                        source,
                        &self.name_rules,
                    )
                } else {
                    false
                }
            })
            || evaluated.objects.values().any(|permanent| {
                if permanent.zone != ObjectZone::Battlefield {
                    return false;
                }
                let Some(perm_ctrl) = permanent.controller else {
                    return false;
                };
                Self::permanent_has_restriction(permanent, |rule| {
                    if let crate::characteristics::StaticRestriction::PreventMatchingActivations(
                        selector,
                    ) = rule
                    {
                        crate::characteristics::selector_matches_permanent(
                            self.window.cards(),
                            &evaluated,
                            permanent.object,
                            perm_ctrl,
                            selector,
                            source,
                            &self.name_rules,
                        )
                    } else {
                        false
                    }
                })
            }) {
            return Err(EffectError::ManaActivationConditionNotMet);
        }
        if current.abilities.mana.is_empty()
            && current.intrinsic_mana.is_empty()
            && current.granted_any_color_mana.is_empty()
            && current.granted_fixed_mana.is_empty()
            && current.inherited_mana.is_empty()
        {
            return Err(EffectError::NotManaSource);
        }
        // Availability and semantic output come from the same evaluated cut.
        // Intrinsic output is temporary query data, never an authored definition.
        use crate::mana::{
            ChosenTypeManaScope, CounterManaYield, DynamicManaYield, ManaOutputModifier,
            ManaOutputRestriction, ManaSacrificeCost, ManaSideEffect, SpecialColorYield,
        };
        let raw_selected = current
            .mana_output(ability)
            .ok_or(EffectError::UnknownManaAbility)?;
        let mut cursor = raw_selected.into_owned();
        let mut mana_cost: Option<ManaCost> = None;
        let mut sacrifice: Option<ManaSacrificeCost> = None;
        let mut tap_controlled_artifact = false;
        let mut restriction: Option<ManaOutputRestriction> = None;
        let mut side_effect: Option<ManaSideEffect> = None;
        let mut activation_conditions: Vec<crate::activated::ActivationRestriction> = Vec::new();
        while let ManaOutput::Modified { modifier, output } = cursor {
            match modifier {
                ManaOutputModifier::RequirePermanents { selector, minimum } => {
                    let count = crate::characteristics::count_permanents(
                        self.window.cards(),
                        &evaluated,
                        source,
                        actor,
                        &selector,
                        &self.name_rules,
                    );
                    if count < minimum {
                        return Err(EffectError::ManaActivationConditionNotMet);
                    }
                    cursor = *output;
                }
                ManaOutputModifier::PayMana(cost) => {
                    mana_cost = Some(cost);
                    cursor = *output;
                }
                ManaOutputModifier::Sacrifice(cost) => {
                    sacrifice = Some(cost);
                    cursor = *output;
                    break;
                }
                ManaOutputModifier::TapControlledArtifact => {
                    tap_controlled_artifact = true;
                    cursor = *output;
                }
                ManaOutputModifier::Restrict(rule) => {
                    restriction = Some(rule);
                    cursor = *output;
                }
                ManaOutputModifier::SpendEffect(_) => {
                    cursor = *output;
                }
                ManaOutputModifier::SideEffect(effect) => {
                    side_effect = Some(effect);
                    cursor = *output;
                }
                ManaOutputModifier::ActivationCondition(cond) => {
                    activation_conditions.push(*cond);
                    cursor = *output;
                }
            }
        }
        let (mana_ordinal, mana_origin) = match ability {
            crate::mana::ManaAbilityRef::Printed(id) => (id.0, None),
            crate::mana::ManaAbilityRef::BasicLand(land) => (10_000 + u64::from(land as u8), None),
            crate::mana::ManaAbilityRef::GrantedAnyColor(effect) => (20_000, Some(effect)),
            crate::mana::ManaAbilityRef::GrantedFixedColor(effect, color) => {
                (30_000 + u64::from(color as u8), Some(effect))
            }
            crate::mana::ManaAbilityRef::Inherited(effect) => (40_000, Some(effect)),
        };
        let mana_synth_id =
            crate::activated::ActivatedAbilityId(u64::MAX.saturating_sub(mana_ordinal));
        let mana_exhaust_key = (
            source,
            None,
            mana_synth_id,
            crate::characteristics::ActivationOriginPath::direct(mana_synth_id, mana_origin),
        );
        for cond in &activation_conditions {
            if matches!(
                cond,
                crate::activated::ActivationRestriction::Sorcery
                    | crate::activated::ActivationRestriction::SorceryOnceEachTurn
            ) {
                if self.window.active_player() != actor
                    || !self.window.entries().is_empty()
                    || !matches!(
                        self.current_step,
                        Some(crate::turn::Step::PrecombatMain | crate::turn::Step::PostcombatMain)
                    )
                {
                    return Err(EffectError::ManaActivationConditionNotMet);
                }
                if let Some(limit) = cond.max_per_turn()
                    && self
                        .activated_this_turn
                        .get(&mana_exhaust_key)
                        .copied()
                        .unwrap_or(0)
                        >= limit
                {
                    return Err(EffectError::ManaActivationConditionNotMet);
                }
            } else {
                self.check_activation_restriction(
                    cond,
                    actor,
                    source,
                    card,
                    &evaluated,
                    None,
                    &mana_exhaust_key,
                )
                .map_err(|_| EffectError::ManaActivationConditionNotMet)?;
            }
        }
        if let Some(sac_cost) = sacrifice {
            let inner = cursor;
            let parent = match authorized_parent {
                Some(parent) => parent,
                None => self.mana_activation_parent(actor)?,
            };
            Self::validate_current_source(actor, card)?;
            if recipient.is_some() {
                return Err(EffectError::InvalidCastResource);
            }
            if payment.is_some() || bundle.is_some() || !restricted.is_empty() {
                return Err(EffectError::InvalidManaPayment(
                    ManaPaymentError::WrongTotal,
                ));
            }
            return match sac_cost {
                ManaSacrificeCost::Source { tap } => {
                    if cost_objects.is_some_and(|objects| !objects.is_empty()) {
                        return Err(EffectError::InvalidManaPayment(
                            ManaPaymentError::WrongTotal,
                        ));
                    }
                    if tap {
                        Self::validate_creature_tap_history(card, current)?;
                    }
                    self.begin_sacrifice_mana(
                        revision,
                        actor,
                        card.clone(),
                        card.clone(),
                        ability,
                        parent,
                        tap,
                        current.has_type(CardType::Land),
                        inner,
                    )
                }
                ManaSacrificeCost::ControlledCreature | ManaSacrificeCost::ControlledSubtype(_) => {
                    let Some([object]) = cost_objects else {
                        return Err(EffectError::InvalidCastResource);
                    };
                    let candidate = evaluated
                        .get(*object)
                        .ok_or(EffectError::InvalidCastResource)?;
                    let matches = match sac_cost {
                        ManaSacrificeCost::ControlledSubtype(subtype) => {
                            candidate.current_creature_subtypes.contains(&subtype)
                        }
                        _ => candidate.is_creature(),
                    };
                    if candidate.zone != ObjectZone::Battlefield
                        || candidate.controller != Some(actor)
                        || !matches
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    let sacrificed = self
                        .window
                        .cards()
                        .get(*object)
                        .ok_or(EffectError::InvalidCastResource)?
                        .clone();
                    self.begin_sacrifice_mana(
                        revision,
                        actor,
                        card.clone(),
                        sacrificed,
                        ability,
                        parent,
                        false,
                        current.has_type(CardType::Land),
                        inner,
                    )
                }
            };
        }
        let selected = match cursor {
            ManaOutput::Triggered(_) => return Err(EffectError::UnknownManaAbility),
            ManaOutput::SpecialColor(SpecialColorYield::EntryChosenColor { fixed }) => {
                // A permanent can acquire this ability after it entered. In that case
                // no color was chosen for it, but its printed fixed color still works.
                match self.entry_color_choices.get(&source).copied() {
                    Some(chosen) => ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: BTreeSet::from([fixed, chosen]),
                    },
                    None => ManaOutput::Fixed(BTreeMap::from([(fixed, 1.into())])),
                }
            }
            other => other,
        };
        if matches!(
            selected,
            ManaOutput::SpecialColor(SpecialColorYield::ChosenPlayerColor)
        ) {
            if !recipient.is_some_and(|player| self.window.participants().contains(&player)) {
                return Err(EffectError::InvalidCastResource);
            }
        } else if recipient.is_some() {
            return Err(EffectError::InvalidCastResource);
        }
        let commander_colors = if matches!(
            selected,
            ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
        ) {
            Some(self.commander_mana_colors(actor)?)
        } else {
            None
        };
        let parent = match authorized_parent {
            Some(parent) => parent,
            None => self.mana_activation_parent(actor)?,
        };
        Self::validate_current_source(actor, card)?;
        if selected == ManaOutput::CounterBased(CounterManaYield::RemoveChargeForColor) {
            if payment.is_some()
                || bundle.is_some()
                || !restricted.is_empty()
                || cost_objects.is_some_and(|objects| !objects.is_empty())
            {
                return Err(EffectError::InvalidCastResource);
            }
            let count = self
                .characteristics
                .charge
                .get(&source)
                .cloned()
                .filter(RulesInteger::is_positive)
                .ok_or(EffectError::InvalidCastResource)?;
            let remaining = count.minus(&1.into());
            let mut next = self.clone();
            next.revision = revision;
            if remaining.is_positive() {
                next.characteristics
                    .charge
                    .insert(source, remaining.clone());
            } else {
                next.characteristics.charge.remove(&source);
            }
            let mut append = RulesJournal::default();
            let before = self.rules_snapshot(None)?;
            next.emit_from(
                crate::occurrence::EventCause::ManaAbilityCost { source, ability },
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![
                        crate::occurrence::RulesOccurrence::ChargeCountersChanged {
                            object: source,
                            before: count,
                            after: remaining,
                        },
                    ])
                },
            )?;
            let before = next.rules_snapshot(None)?;
            next.emit_from(
                crate::occurrence::EventCause::ManaAbilityActivated { source, ability },
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![crate::occurrence::RulesOccurrence::ManaAbility {
                        source: card.clone(),
                        ability,
                        controller: actor,
                    }])
                },
            )?;
            let frame = ManaColorChoice {
                revision,
                source,
                ability,
                chooser: actor,
                amount: 1.into(),
                colors: ManaOutput::all_colors(),
            };
            next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                frame: frame.clone(),
                parent,
                controller_damage: None,
                triggered: false,
                activation_controller: None,
                restriction: None,
                sacrificed_source: None,
            })));
            return Ok(ManaActivationProgress::new(
                next,
                ManaActivationResult::Pending { frame },
                append,
            ));
        }
        let paid_object = if tap_controlled_artifact {
            let Some([object]) = cost_objects else {
                return Err(EffectError::InvalidCastResource);
            };
            let target = evaluated
                .get(*object)
                .ok_or(EffectError::InvalidCastResource)?;
            if target.zone != ObjectZone::Battlefield
                || target.controller != Some(actor)
                || !target.has_type(CardType::Artifact)
            {
                return Err(EffectError::InvalidCastResource);
            }
            self.window
                .cards()
                .get(*object)
                .ok_or(EffectError::InvalidCastResource)?
        } else {
            if cost_objects.is_some_and(|objects| !objects.is_empty()) {
                return Err(EffectError::InvalidCastResource);
            }
            Self::validate_creature_tap_history(card, current)?;
            card
        };
        let pay = |cost: &ManaCost| {
            self.prepare_restricted_mana_payment(cost, payment, actor, restricted, |rule| {
                self.restriction_allows_ability(rule, current)
            })
        };
        let spend =
            match (&mana_cost, &selected) {
                (_, ManaOutput::SpecialColor(SpecialColorYield::HybridFilter { colors })) => {
                    let spend = pay(&ManaCost::generic(1.into()))?;
                    if spend.len() != 1 || !colors.contains(&spend[0].mana_type) {
                        return Err(EffectError::IllegalHybridManaPayment);
                    }
                    let values = bundle.ok_or(EffectError::IllegalManaBundle)?;
                    if values.is_empty()
                        || values
                            .iter()
                            .any(|(kind, amount)| !colors.contains(kind) || !amount.is_positive())
                        || values
                            .values()
                            .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount))
                            != 2.into()
                    {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    spend
                }
                (Some(cost), ManaOutput::Dynamic(DynamicManaYield::GreatestCreaturePower)) => {
                    pay(cost)?
                }
                (Some(cost), ManaOutput::TwoColors { distinct }) => {
                    let spend = pay(cost)?;
                    let values = bundle.ok_or(EffectError::IllegalManaBundle)?;
                    if *distinct {
                        if values.len() != 2
                            || values.iter().any(|(kind, amount)| {
                                *kind == ManaType::Colourless || *amount != 1.into()
                            })
                        {
                            return Err(EffectError::IllegalManaBundle);
                        }
                    } else if values.iter().any(|(kind, amount)| {
                        *kind == ManaType::Colourless || !amount.is_positive()
                    }) || values
                        .values()
                        .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount))
                        != 2.into()
                    {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    spend
                }
                (Some(cost), _) => {
                    if bundle.is_some() {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    pay(cost)?
                }
                (None, ManaOutput::CounterBased(CounterManaYield::RemoveStorageCounters)) => {
                    if payment.is_some() || !restricted.is_empty() {
                        return Err(EffectError::InvalidManaPayment(
                            ManaPaymentError::WrongTotal,
                        ));
                    }
                    let values = bundle.ok_or(EffectError::IllegalManaBundle)?;
                    if values.iter().any(|(kind, amount)| {
                        *kind != ManaType::Colourless || !amount.is_positive()
                    }) {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    let amount = values
                        .get(&ManaType::Colourless)
                        .cloned()
                        .unwrap_or_else(|| 0.into());
                    let available = self
                        .characteristics
                        .storage
                        .get(&source)
                        .cloned()
                        .unwrap_or_else(|| 0.into());
                    if amount > available {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    Vec::new()
                }
                (None, ManaOutput::TwoColors { distinct }) => {
                    if payment.is_some() || !restricted.is_empty() {
                        return Err(EffectError::InvalidManaPayment(
                            ManaPaymentError::WrongTotal,
                        ));
                    }
                    let values = bundle.ok_or(EffectError::IllegalManaBundle)?;
                    if *distinct {
                        if values.len() != 2
                            || values.iter().any(|(kind, amount)| {
                                *kind == ManaType::Colourless || *amount != 1.into()
                            })
                        {
                            return Err(EffectError::IllegalManaBundle);
                        }
                    } else if values.iter().any(|(kind, amount)| {
                        *kind == ManaType::Colourless || !amount.is_positive()
                    }) || values
                        .values()
                        .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount))
                        != 2.into()
                    {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    Vec::new()
                }
                (None, _) => {
                    if bundle.is_some() {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    if payment.is_some() || !restricted.is_empty() {
                        return Err(EffectError::InvalidManaPayment(
                            ManaPaymentError::WrongTotal,
                        ));
                    }
                    Vec::new()
                }
            };
        let window = match &parent {
            ManaParent::Priority => {
                self.window
                    .record_tap_action(self.window.revision(), actor, paid_object.id())?
            }
            ManaParent::CombatPayment(_) => {
                self.window
                    .record_declaration_taps(self.window.revision(), actor, &[paid_object.id()])?
                    .0
            }
            ManaParent::Casting(entry) => self.window.record_resolution_tap(
                self.window.revision(),
                *entry,
                actor,
                paid_object.id(),
            )?,
            ManaParent::Recovery(_, entry) => self.window.record_resolution_tap(
                self.window.revision(),
                *entry,
                actor,
                paid_object.id(),
            )?,
            ManaParent::Payment(work) => self.window.record_resolution_tap(
                self.window.revision(),
                work.cursor.entry,
                actor,
                paid_object.id(),
            )?,
        };
        let mut next = self.clone();
        next.revision = revision;
        next.window = window;
        if activation_conditions
            .iter()
            .any(crate::activated::ActivationRestriction::has_turn_activation_cap)
        {
            let count = next
                .activated_this_turn
                .entry(mana_exhaust_key.clone())
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        if activation_conditions
            .iter()
            .any(crate::activated::ActivationRestriction::has_exhaust)
        {
            next.characteristics
                .exhausted
                .insert(mana_exhaust_key.clone());
        }
        let storage_change =
            if selected == ManaOutput::CounterBased(CounterManaYield::RemoveStorageCounters) {
                let amount = bundle
                    .and_then(|values| values.get(&ManaType::Colourless))
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                let old = next
                    .characteristics
                    .storage
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                let remaining = old.minus(&amount);
                if remaining.is_positive() {
                    next.characteristics
                        .storage
                        .insert(source, remaining.clone());
                } else {
                    next.characteristics.storage.remove(&source);
                }
                Some((old, remaining))
            } else {
                None
            };
        next.debit_restricted_mana(actor, restricted)?;
        next.spend_mana_lots(
            actor,
            &spend,
            restricted,
            payment.map(|allocation| &allocation.from_lots),
        )?;
        next.debit_mana(actor, &spend);
        let mut append = RulesJournal::default();
        let before = self.rules_snapshot(None)?;
        let after = next
            .window
            .cards()
            .get(paid_object.id())
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        next.emit_from(
            crate::occurrence::EventCause::ManaAbilityCost { source, ability },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(occurrence_flow::status_occurrences(
                    vec![crate::object_identity::ObjectStatusChange {
                        before: paid_object.clone(),
                        after: after.clone(),
                    }],
                    crate::occurrence::StatusReason::ManaAbilityCost,
                ))
            },
        )?;
        if let Some((before_count, after_count)) = storage_change
            && before_count != after_count
        {
            next.emit_from(
                crate::occurrence::EventCause::ManaAbilityCost { source, ability },
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![
                        crate::occurrence::RulesOccurrence::StorageCountersChanged {
                            object: source,
                            before: before_count,
                            after: after_count,
                        },
                    ])
                },
            )?;
        }
        let activated = next.rules_snapshot(None)?;
        let ability_source = next
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        next.emit_from(
            crate::occurrence::EventCause::ManaAbilityActivated { source, ability },
            &activated,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::ManaAbility {
                    source: ability_source,
                    ability,
                    controller: actor,
                }])
            },
        )?;
        let resolved_restriction = restriction.map(|rule| match rule {
            ManaOutputRestriction::Fixed(r) => r,
            ManaOutputRestriction::ChosenCreatureType(scope) => {
                let kind = next.entry_creature_type_choices.get(&source).copied();
                match scope {
                    ChosenTypeManaScope::CreatureSpellOrAbility => {
                        crate::mana::ManaRestriction::CreatureSpellOrAbilityOfType(kind)
                    }
                    ChosenTypeManaScope::CreatureSpellUncounterable => {
                        crate::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(kind)
                    }
                    ChosenTypeManaScope::CreatureSpell => {
                        crate::mana::ManaRestriction::CreatureSpellOfType(kind)
                    }
                }
            }
        });
        let (life_gain, controller_damage) = match side_effect {
            Some(ManaSideEffect::GainLife(life)) => (Some(life), None),
            Some(ManaSideEffect::DamageController(damage)) => (None, Some(damage)),
            None => (None, None),
        };
        match selected {
            ManaOutput::Fixed(produced) => {
                let production = next.credit_mana_at_cut(
                    source,
                    ability,
                    (actor, actor),
                    produced,
                    resolved_restriction,
                    false,
                    None,
                    &mut append,
                )?;
                if let Some(life) = life_gain {
                    next.gain_life_from_mana(source, ability, actor, &life, &mut append)?;
                }
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Complete { production },
                    append,
                ))
            }
            ManaOutput::ChooseColor { amount, colors } => {
                let frame = ManaColorChoice {
                    revision,
                    source,
                    ability,
                    chooser: actor,
                    amount,
                    colors,
                };
                next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                    frame: frame.clone(),
                    parent,
                    controller_damage,
                    triggered: false,
                    activation_controller: None,
                    restriction: resolved_restriction,
                    sacrificed_source: None,
                })));
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Pending { frame },
                    append,
                ))
            }
            ManaOutput::CounterBased(kind) => match kind {
                CounterManaYield::SourceChargeCounters => {
                    let produced = next
                        .characteristics
                        .charge
                        .get(&source)
                        .filter(|n| n.is_positive())
                        .map(|n| (ManaType::Colourless, n.clone()))
                        .into_iter()
                        .collect();
                    let production = next.credit_mana_at_cut(
                        source,
                        ability,
                        (actor, actor),
                        produced,
                        resolved_restriction,
                        false,
                        None,
                        &mut append,
                    )?;
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Complete { production },
                        append,
                    ))
                }
                CounterManaYield::RemoveStorageCounters => {
                    let production = next.credit_mana_at_cut(
                        source,
                        ability,
                        (actor, actor),
                        bundle.ok_or(EffectError::IllegalManaBundle)?.clone(),
                        resolved_restriction,
                        false,
                        None,
                        &mut append,
                    )?;
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Complete { production },
                        append,
                    ))
                }
                CounterManaYield::RemoveChargeForColor => Err(EffectError::InconsistentProgram),
            },
            ManaOutput::TwoColors { .. } => {
                let production = next.credit_mana_at_cut(
                    source,
                    ability,
                    (actor, actor),
                    bundle.ok_or(EffectError::IllegalManaBundle)?.clone(),
                    resolved_restriction,
                    false,
                    None,
                    &mut append,
                )?;
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Complete { production },
                    append,
                ))
            }
            ManaOutput::Counted {
                mana_type,
                permanents,
            } => {
                let evaluated = next.evaluated_characteristics()?;
                let amount = crate::characteristics::count_permanents(
                    next.window.cards(),
                    &evaluated,
                    source,
                    actor,
                    &permanents,
                    &next.name_rules,
                );
                match mana_type {
                    Some(kind) => {
                        let produced = if amount.is_positive() {
                            BTreeMap::from([(kind, amount)])
                        } else {
                            BTreeMap::new()
                        };
                        let production = next.credit_mana_at_cut(
                            source,
                            ability,
                            (actor, actor),
                            produced,
                            resolved_restriction,
                            false,
                            None,
                            &mut append,
                        )?;
                        Ok(ManaActivationProgress::new(
                            next,
                            ManaActivationResult::Complete { production },
                            append,
                        ))
                    }
                    None => {
                        let amount = next.retained_mana_amount(source, ability).unwrap_or(amount);
                        if !amount.is_positive() {
                            let production = next.credit_mana_at_cut(
                                source,
                                ability,
                                (actor, actor),
                                BTreeMap::new(),
                                resolved_restriction,
                                false,
                                None,
                                &mut append,
                            )?;
                            return Ok(ManaActivationProgress::new(
                                next,
                                ManaActivationResult::Complete { production },
                                append,
                            ));
                        }
                        let frame = ManaColorChoice {
                            revision,
                            source,
                            ability,
                            chooser: actor,
                            amount,
                            colors: ManaOutput::all_colors(),
                        };
                        next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                            frame: frame.clone(),
                            parent,
                            controller_damage,
                            triggered: false,
                            activation_controller: None,
                            restriction: resolved_restriction,
                            sacrificed_source: None,
                        })));
                        Ok(ManaActivationProgress::new(
                            next,
                            ManaActivationResult::Pending { frame },
                            append,
                        ))
                    }
                }
            }
            ManaOutput::SpecialColor(kind) => match kind {
                SpecialColorYield::ChosenPlayerColor => {
                    let frame = ManaColorChoice {
                        revision,
                        source,
                        ability,
                        chooser: recipient.ok_or(EffectError::InvalidCastResource)?,
                        amount: 1.into(),
                        colors: ManaOutput::all_colors(),
                    };
                    next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                        frame: frame.clone(),
                        parent,
                        controller_damage: None,
                        triggered: false,
                        activation_controller: Some(actor),
                        restriction: resolved_restriction,
                        sacrificed_source: None,
                    })));
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Pending { frame },
                        append,
                    ))
                }
                SpecialColorYield::OpponentsLandColors => {
                    let colors = next.opponents_land_colors(actor)?;
                    if colors.is_empty() {
                        let production = next.credit_mana_at_cut(
                            source,
                            ability,
                            (actor, actor),
                            BTreeMap::new(),
                            resolved_restriction,
                            false,
                            None,
                            &mut append,
                        )?;
                        return Ok(ManaActivationProgress::new(
                            next,
                            ManaActivationResult::Complete { production },
                            append,
                        ));
                    }
                    let frame = ManaColorChoice {
                        revision,
                        source,
                        ability,
                        chooser: actor,
                        amount: 1.into(),
                        colors,
                    };
                    next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                        frame: frame.clone(),
                        parent,
                        controller_damage: None,
                        triggered: false,
                        activation_controller: None,
                        restriction: resolved_restriction,
                        sacrificed_source: None,
                    })));
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Pending { frame },
                        append,
                    ))
                }
                SpecialColorYield::CommanderColorIdentity => {
                    if commander_colors.as_ref().is_some_and(BTreeSet::is_empty) {
                        let production = next.credit_mana_at_cut(
                            source,
                            ability,
                            (actor, actor),
                            BTreeMap::new(),
                            resolved_restriction,
                            false,
                            None,
                            &mut append,
                        )?;
                        return Ok(ManaActivationProgress::new(
                            next,
                            ManaActivationResult::Complete { production },
                            append,
                        ));
                    }
                    let colors =
                        commander_colors.ok_or(EffectError::MissingCommanderColorIdentity)?;
                    let frame = ManaColorChoice {
                        revision,
                        source,
                        ability,
                        chooser: actor,
                        amount: 1.into(),
                        colors,
                    };
                    next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                        frame: frame.clone(),
                        parent,
                        controller_damage: None,
                        triggered: false,
                        activation_controller: None,
                        restriction: resolved_restriction,
                        sacrificed_source: None,
                    })));
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Pending { frame },
                        append,
                    ))
                }
                SpecialColorYield::HybridFilter { .. } => {
                    let production = next.credit_mana_at_cut(
                        source,
                        ability,
                        (actor, actor),
                        bundle.ok_or(EffectError::IllegalManaBundle)?.clone(),
                        resolved_restriction,
                        false,
                        None,
                        &mut append,
                    )?;
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Complete { production },
                        append,
                    ))
                }
                SpecialColorYield::EntryChosenColor { .. } => Err(EffectError::InconsistentProgram),
            },
            ManaOutput::Dynamic(kind) => match kind {
                DynamicManaYield::GreatestCreaturePower => {
                    // Paying the activation cost can change continuous effects that depend on the mana pool or tapped state.
                    let amount = if let Some(issued) = next.retained_mana_amount(source, ability) {
                        issued
                    } else {
                        next.battlefield_cost_value(
                            crate::program::CastCostValue::GreatestCreaturePower,
                            actor,
                            &next.evaluated_characteristics()?,
                        )?
                    };
                    let produced = bundle.cloned().unwrap_or_default();
                    if produced
                        .iter()
                        .any(|(kind, n)| *kind == ManaType::Colourless || !n.is_positive())
                        || produced
                            .values()
                            .fold(RulesInteger::from(0), |sum, n| sum.plus(n))
                            != amount
                    {
                        return Err(EffectError::IllegalManaBundle);
                    }
                    let production = next.credit_mana_at_cut(
                        source,
                        ability,
                        (actor, actor),
                        produced,
                        resolved_restriction,
                        false,
                        None,
                        &mut append,
                    )?;
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Complete { production },
                        append,
                    ))
                }
                DynamicManaYield::ControlledCreaturePowerColor {
                    minimum_power,
                    otherwise,
                    matching,
                } => {
                    let evaluated = next.evaluated_characteristics()?;
                    let qualifies = evaluated.objects.values().any(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.controller == Some(actor)
                            && object.is_creature()
                            && object
                                .power_toughness
                                .as_ref()
                                .is_some_and(|pt| pt.derived.power >= minimum_power)
                    });
                    let frame = ManaColorChoice {
                        revision,
                        source,
                        ability,
                        chooser: actor,
                        amount: if qualifies { matching } else { otherwise },
                        colors: ManaOutput::all_colors(),
                    };
                    next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                        frame: frame.clone(),
                        parent,
                        controller_damage,
                        triggered: false,
                        activation_controller: None,
                        restriction: resolved_restriction,
                        sacrificed_source: None,
                    })));
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Pending { frame },
                        append,
                    ))
                }
                DynamicManaYield::ConditionalControlledLandTypes {
                    requirements,
                    normal,
                    fulfilled,
                } => {
                    let evaluated = next.evaluated_characteristics()?;
                    let qualifies = requirements.iter().all(|required| {
                        evaluated.objects.values().any(|object| {
                            object.zone == ObjectZone::Battlefield
                                && object.controller == Some(actor)
                                && object.has_type(CardType::Land)
                                && required.is_subset(&object.current_land_subtypes)
                        })
                    });
                    let produced = if qualifies { fulfilled } else { normal };
                    let production = next.credit_mana_at_cut(
                        source,
                        ability,
                        (actor, actor),
                        produced,
                        resolved_restriction,
                        false,
                        None,
                        &mut append,
                    )?;
                    Ok(ManaActivationProgress::new(
                        next,
                        ManaActivationResult::Complete { production },
                        append,
                    ))
                }
            },
            ManaOutput::Triggered(_) | ManaOutput::Modified { .. } => {
                Err(EffectError::InconsistentProgram)
            }
        }
    }
}
