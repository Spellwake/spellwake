//! Activated ability validation, cost payment, and stack placement.
use super::*;

impl EffectWindow {
    #[inline(never)]
    pub(crate) fn activate_ability_on_turn_restricted(
        &self,
        expected: u64,
        actor: SeatId,
        request: ActivationRequest,
        payment: Option<ManaPayment>,
        restricted: &[crate::mana::RestrictedManaSpend],
        turn: Option<&TurnScheduler>,
    ) -> Result<AbilityActivationTransition, EffectError> {
        let ActivationRequest {
            granting_effect,
            chosen_x,
            source,
            ability,
            origin,
        } = request;
        let target = match &origin {
            crate::activated::ActivationOrigin::Targeted(t) => Some(*t),
            crate::activated::ActivationOrigin::SelectedCost { target, .. } => *target,
            _ => None,
        };
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let card = self
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::UnknownObject)?;
        let evaluated = self.evaluated_characteristics()?;
        let current = evaluated
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?;
        self.window.validate_action_actor(actor)?;
        self.check_opponents_turn_restriction(actor, &evaluated, Some(current))?;
        Self::check_artifact_activation_restriction(&evaluated, current)?;
        let selected = if let Some(effect) = granting_effect {
            current
                .granted_activated
                .get(&effect)
                .filter(|selected| selected.id == ability)
                .ok_or(EffectError::UnknownActivatedAbility)?
        } else {
            if current.abilities.activated.is_empty() {
                return Err(EffectError::NotAbilitySource);
            }
            if !current.abilities.activated.contains(&ability) {
                return Err(EffectError::UnknownActivatedAbility);
            }
            current
                .printed_text
                .activated
                .iter()
                .find(|entry| entry.id == ability)
                .ok_or(EffectError::UnknownActivatedAbility)?
        };
        use crate::activated::{
            ActivationOrigin, CounterCostDelta, DiscardCostMode, GraveyardExileCost,
            HandDiscardKeyword, LifeCostAmount, SelectedSacrificeFilter, SourceCost, TapCostCount,
            TapCostFilter,
        };
        let matches_sacrifice_filter =
            |id: ObjectId, filter: SelectedSacrificeFilter| -> Result<bool, EffectError> {
                let candidate = evaluated.get(id).ok_or(EffectError::InvalidCastResource)?;
                Ok(candidate.zone == ObjectZone::Battlefield
                    && candidate.controller == Some(actor)
                    && match filter {
                        SelectedSacrificeFilter::Subtype(kind) => {
                            candidate.current_creature_subtypes.contains(&kind)
                        }
                        SelectedSacrificeFilter::Artifact => candidate.has_type(CardType::Artifact),
                        SelectedSacrificeFilter::OtherEnchantment => {
                            id != source && candidate.has_type(CardType::Enchantment)
                        }
                        SelectedSacrificeFilter::OtherCreature => {
                            id != source && candidate.is_creature()
                        }
                        SelectedSacrificeFilter::Creature => candidate.is_creature(),
                        SelectedSacrificeFilter::CreatureOrEnchantment => {
                            candidate.is_creature() || candidate.has_type(CardType::Enchantment)
                        }
                        SelectedSacrificeFilter::ArtifactOrCreature => {
                            candidate.is_creature() || candidate.has_type(CardType::Artifact)
                        }
                        SelectedSacrificeFilter::Desert => {
                            candidate.has_type(CardType::Land)
                                && candidate
                                    .current_land_subtypes
                                    .contains(&crate::land::LandType::Desert)
                        }
                        SelectedSacrificeFilter::GrantingPermanent => false,
                    })
            };
        let matches_tap_filter =
            |id: ObjectId, filter: TapCostFilter, exclude_source: bool| -> bool {
                if exclude_source && id == source {
                    return false;
                }
                if self.window.cards().get(id).and_then(GameObject::tapped) != Some(false) {
                    return false;
                }
                evaluated.get(id).is_some_and(|candidate| {
                    candidate.zone == ObjectZone::Battlefield
                        && candidate.controller == Some(actor)
                        && match filter {
                            TapCostFilter::Creature => candidate.is_creature(),
                            TapCostFilter::Artifact => candidate.has_type(CardType::Artifact),
                            TapCostFilter::Legendary => candidate
                                .current_supertypes
                                .contains(&crate::program::Supertype::Legendary),
                            TapCostFilter::Subtype(kind) => {
                                candidate.current_creature_subtypes.contains(&kind)
                            }
                        }
                })
            };
        let granting_damage_source = if matches!(
            selected.source_cost,
            SourceCost::SacrificeSelected {
                filter: SelectedSacrificeFilter::GrantingPermanent,
                ..
            }
        ) {
            let Some(crate::characteristics::EffectInstanceKey::Static {
                source: grantor, ..
            }) = granting_effect
            else {
                return Err(EffectError::InvalidCastResource);
            };
            let object = self
                .window
                .cards()
                .get(grantor)
                .filter(|object| {
                    object.zone() == ObjectZone::Battlefield
                        && object.controller() == Some(actor)
                        && self.attached_to(grantor) == Some(TargetRef::Object(source))
                })
                .ok_or(EffectError::InvalidCastResource)?;
            if evaluated.sacrifice_prohibited(grantor) {
                return Err(EffectError::InvalidCastResource);
            }
            Some(DamageSource::Object {
                object: object.clone(),
                controller: actor,
            })
        } else {
            None
        };
        let origin = if let Some(DamageSource::Object { object, .. }) = &granting_damage_source {
            let ActivationOrigin::Targeted(target) = origin else {
                return Err(EffectError::TargetRequired);
            };
            ActivationOrigin::SelectedCost {
                target: Some(target),
                objects: vec![object.id()],
            }
        } else if matches!(
            selected.source_cost,
            SourceCost::Discard {
                mode: DiscardCostMode::WholeHand,
                ..
            }
        ) {
            match origin {
                ActivationOrigin::Untargeted | ActivationOrigin::Targeted(_) => {
                    let target = if let ActivationOrigin::Targeted(target) = origin {
                        Some(target)
                    } else {
                        None
                    };
                    let objects = self
                        .window
                        .cards()
                        .iter()
                        .filter(|card| card.zone() == ObjectZone::Hand(actor))
                        .map(GameObject::id)
                        .collect();
                    ActivationOrigin::SelectedCost { objects, target }
                }
                other => other,
            }
        } else {
            origin
        };
        let terms = current.activation_terms(ability, granting_effect);
        let (restriction, loyalty_cost, variable_loyalty) =
            (terms.restriction, terms.loyalty, terms.variable_loyalty);
        let counter_cost_plan = if let Some((kind, delta)) = selected.source_cost.counter_cost() {
            let current_count = self.counters_of_kind(source, kind);
            let (change, paid) = match delta {
                CounterCostDelta::Add(count) => (RulesInteger::from(i64::from(count.get())), None),
                CounterCostDelta::Remove(count) => {
                    let amount = RulesInteger::from(i64::from(count.get()));
                    if amount > current_count {
                        return Err(EffectError::InvalidCastResource);
                    }
                    (RulesInteger::from(0).minus(&amount), Some(amount))
                }
                CounterCostDelta::RemoveAll => (
                    RulesInteger::from(0).minus(&current_count),
                    Some(current_count),
                ),
            };
            Some((kind, change, paid))
        } else {
            None
        };
        let mut bound_ability;
        let mut selected_tap_count = if matches!(
            selected.source_cost,
            SourceCost::ExileGraveyard {
                filter: GraveyardExileCost::AnyCountX,
                ..
            }
        ) {
            let ActivationOrigin::SelectedCost { objects, .. } = &origin else {
                return Err(EffectError::InvalidCastResource);
            };
            Some(RulesInteger::from_usize(objects.len()))
        } else {
            None
        };
        let selected = if matches!(
            selected.source_cost,
            SourceCost::TapPermanents {
                count: TapCostCount::ChosenX,
                ..
            }
        ) {
            let ActivationOrigin::SelectedCost { objects, .. } = &origin else {
                return Err(EffectError::InvalidCastResource);
            };
            let BodyTarget::FilteredPermanent(filter) = selected.body.target else {
                return Err(EffectError::InconsistentProgram);
            };
            if !filter.has_power_at_most_x() {
                return Err(EffectError::InconsistentProgram);
            }
            let count = objects.len();
            selected_tap_count = Some(RulesInteger::from_usize(count));
            bound_ability = selected.clone();
            bound_ability.body.target = BodyTarget::FilteredPermanent(
                filter.bind_power_at_most_x(u16::try_from(count).unwrap_or(u16::MAX)),
            );
            &bound_ability
        } else {
            selected
        };
        let fixed_activation = if selected
            .body
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
        {
            let ActivationOrigin::PermanentTargets(targets) = &origin else {
                return Err(EffectError::TargetRequired);
            };
            Some(self.expand_fixed_clauses(&selected.body, targets, actor, &evaluated, current)?)
        } else {
            None
        };
        if (variable_loyalty || selected.source_cost.generic_x_symbols().is_some())
            != chosen_x.is_some()
            || chosen_x.as_ref().is_some_and(|x| x < &0.into())
        {
            return Err(EffectError::InvalidCastResource);
        }
        if loyalty_cost.is_some() && current.has_type(CardType::Planeswalker) && evaluated.objects.values().any(|permanent| {
            permanent.zone == ObjectZone::Battlefield && permanent.printed_text.continuous.iter().chain(&permanent.printed_text.additional_battlefield_abilities).any(|effect| {
                matches!(effect, crate::characteristics::StaticContinuousAbility::Restriction { rule: crate::characteristics::StaticRestriction::PreventLoyaltyActivations, .. })
                    && (permanent.abilities.continuous == Some(effect.id()) || permanent.abilities.additional_battlefield_abilities.contains(&effect.id()))
            })
        }) { return Err(EffectError::ActivationConditionNotMet); }
        if evaluated.objects.values().any(|permanent| {
            if permanent.zone != ObjectZone::Battlefield {
                return false;
            }
            let Some(perm_ctrl) = permanent.controller else {
                return false;
            };
            permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
                .any(|effect| {
                    let active = permanent.abilities.continuous == Some(effect.id())
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&effect.id());
                    if !active {
                        return false;
                    }
                    if let crate::characteristics::StaticContinuousAbility::Restriction {
                        rule:
                            crate::characteristics::StaticRestriction::PreventMatchingActivations(
                                selector,
                            ),
                        ..
                    } = effect
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
            return Err(EffectError::ActivationConditionNotMet);
        }
        let mut uses_extra_loyalty = false;
        if let Some(cost) = loyalty_cost {
            let instant_timing = self.characteristics.emblems.values().any(|emblem| {
                emblem.controller == actor
                    && matches!(
                        emblem.definition,
                        crate::emblem::EmblemDefinition::LoyaltyAnyTurn
                    )
            });
            if !instant_timing {
                validate_main_phase(&self.window, turn, actor)?;
            }
            if selected.source_cost != crate::activated::SourceCost::None {
                return Err(EffectError::InconsistentProgram);
            }
            let walker = current.has_type(CardType::Planeswalker);
            let normal_limit = if walker
                && evaluated.controlled_rules(actor, |ability| {
                    matches!(
                        ability,
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule: crate::characteristics::StaticModifierRule::TwiceLoyaltyPerTurn,
                            ..
                        }
                    )
                }) > 0
            {
                2.into()
            } else {
                1.into()
            };
            uses_extra_loyalty = self
                .characteristics
                .loyalty_used
                .get(&source)
                .is_some_and(|count| count >= &normal_limit);
            if uses_extra_loyalty {
                let available = self.characteristics.extra_loyalty_activations.get(&actor);
                let used = self
                    .characteristics
                    .extra_loyalty_used
                    .get(&(source, actor))
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                if !walker || available.is_none_or(|limit| &used >= limit) {
                    return Err(EffectError::LoyaltyAlreadyActivated);
                }
            }
            let before = self
                .characteristics
                .loyalty
                .get(&source)
                .cloned()
                .unwrap_or_else(|| 0.into());
            if chosen_x.as_ref().is_some_and(|x| before < *x)
                || (cost < 0 && before < (-i64::from(cost)).into())
            {
                return Err(EffectError::InsufficientLoyalty);
            }
        }
        let exhausted_id = (
            source,
            self.characteristics
                .copied_definition(source)
                .map(|(_, stamp)| stamp),
            ability,
            terms.origin.clone(),
        );
        use crate::activated::ActivationRestriction;
        let controlled = || {
            evaluated
                .objects
                .values()
                .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(actor))
        };
        match restriction {
            Some(ActivationRestriction::HasCityBlessing) if !self.has_city_blessing(actor) => {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::CreatedTokenThisTurn)
                if !self.token_creators_this_turn.contains(&actor) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::CreatureDiedThisTurn)
                if !self.creatures_died_this_turn.is_positive() =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::EmptyHand)
                if self
                    .window
                    .cards()
                    .iter()
                    .any(|card| card.zone() == ObjectZone::Hand(actor)) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::ControlsCreaturePowerFourOrGreater)
                if !controlled().any(|p| {
                    p.is_creature()
                        && p.power_toughness
                            .as_ref()
                            .is_some_and(|pt| pt.derived.power >= 4.into())
                }) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::ControlsSubtypeCount { kind, minimum })
                if controlled()
                    .filter(|permanent| permanent.current_creature_subtypes.contains(&kind))
                    .take(usize::from(minimum.get()))
                    .count()
                    < usize::from(minimum.get()) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(ActivationRestriction::ControlsThreeLandsWithSameName) => {
                let mut counts = BTreeMap::new();
                let legal = controlled()
                    .filter(|p| p.has_type(CardType::Land))
                    .any(|p| {
                        self.name_rules
                            .comparison_forms(&p.current_names)
                            .into_iter()
                            .any(|name| {
                                let count = counts.entry(name).or_insert(0usize);
                                *count += 1;
                                *count >= 3
                            })
                    });
                if !legal {
                    return Err(EffectError::ActivationConditionNotMet);
                }
            }
            Some(crate::activated::ActivationRestriction::DuringUpkeep)
                if turn.map(|t| t.position().step).or(self.current_step)
                    != Some(crate::turn::Step::Upkeep) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::OwnTurnBeforeAttackers) => {
                let turn = turn.ok_or(TimingError::TurnRequired)?;
                if self.window.active_player() != actor {
                    return Err(TimingError::NotActivePlayer.into());
                }
                if !matches!(
                    turn.position().step,
                    crate::turn::Step::Untap
                        | crate::turn::Step::Upkeep
                        | crate::turn::Step::Draw
                        | crate::turn::Step::PrecombatMain
                        | crate::turn::Step::BeginningCombat
                ) {
                    return Err(TimingError::BeforeAttackersRequired.into());
                }
            }
            Some(crate::activated::ActivationRestriction::OnceEachTurn)
                if self.activated_this_turn.contains(&exhausted_id) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::Sorcery) => {
                validate_main_phase(&self.window, turn, actor)?
            }
            Some(crate::activated::ActivationRestriction::SorceryOnceEachTurn) => {
                if self.activated_this_turn.contains(&exhausted_id) {
                    return Err(EffectError::ActivationConditionNotMet);
                }
                validate_main_phase(&self.window, turn, actor)?;
            }
            Some(crate::activated::ActivationRestriction::DuringYourTurn)
                if self.window.active_player() != actor =>
            {
                return Err(TimingError::NotActivePlayer.into());
            }
            Some(crate::activated::ActivationRestriction::DuringYourTurnOnceEachTurn) => {
                if self.window.active_player() != actor {
                    return Err(TimingError::NotActivePlayer.into());
                }
                if self.activated_this_turn.contains(&exhausted_id) {
                    return Err(EffectError::ActivationConditionNotMet);
                }
            }
            Some(crate::activated::ActivationRestriction::DuringOpponentsTurn)
                if self.window.active_player() == actor =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::DuringCombat)
                if !turn
                    .map(|t| t.position().step)
                    .or(self.current_step)
                    .is_some_and(|s| s.phase() == crate::turn::Phase::Combat) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::Threshold)
                if self
                    .window
                    .cards()
                    .iter()
                    .filter(|c| c.zone() == ObjectZone::Graveyard(actor))
                    .count()
                    < 7 =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::Delirium) => {
                let mut types = BTreeSet::new();
                for c in evaluated
                    .objects
                    .values()
                    .filter(|o| o.zone == ObjectZone::Graveyard(actor))
                {
                    types.extend(c.current_types.iter().copied());
                }
                if types.len() < 4 {
                    return Err(EffectError::ActivationConditionNotMet);
                }
            }
            Some(crate::activated::ActivationRestriction::Metalcraft)
                if controlled()
                    .filter(|p| p.has_type(CardType::Artifact))
                    .count()
                    < 3 =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::SourceIsTapped)
                if card.tapped() != Some(true) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::SourceIsUntapped)
                if card.tapped() != Some(false) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::ControllerLifeAtLeast(min))
                if !self
                    .life
                    .get(actor)
                    .is_some_and(|life| *life >= i64::from(min).into()) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::ControllerLifeAtMost(max))
                if !self
                    .life
                    .get(actor)
                    .is_some_and(|life| *life <= i64::from(max).into()) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::ControlsCardType { kind, minimum })
                if controlled().filter(|p| p.has_type(kind)).count() < usize::from(minimum) =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(crate::activated::ActivationRestriction::ControlsMatching { filter, minimum })
                if {
                    let empty = BTreeSet::new();
                    controlled()
                        .filter(|p| {
                            let same = p.object == source;
                            let (c1, c2) = filter.clauses();
                            let src_ok = |c: crate::program::ObjectClause| match c.source_relation {
                                crate::program::SourceRelation::Any => true,
                                crate::program::SourceRelation::SelfObject => same,
                                crate::program::SourceRelation::OtherThanAbilitySource
                                | crate::program::SourceRelation::OtherThanTriggerSource => !same,
                                _ => false,
                            };
                            (src_ok(c1) || c2.is_some_and(src_ok))
                                && filter.matches_static_characteristics(p, &empty, || {
                                    self.window
                                        .cards()
                                        .get(p.object)
                                        .and_then(GameObject::card_id)
                                        .and_then(|cid| self.programs.get(&cid))
                                        .map(Program::mana_value)
                                })
                        })
                        .count()
                        < usize::from(minimum)
                } =>
            {
                return Err(EffectError::ActivationConditionNotMet);
            }
            Some(
                crate::activated::ActivationRestriction::Exhaust
                | crate::activated::ActivationRestriction::PowerUp,
            ) if self.characteristics.exhausted.contains(&exhausted_id) => {
                return Err(EffectError::AlreadyExhausted);
            }
            _ => {}
        }
        let returned_attacker = if selected.source_cost.is_ninjutsu() {
            if !selected.source_cost.can_activate_ninjutsu(card, actor) {
                return Err(EffectError::NotOwnHand);
            }
            let crate::activated::ActivationOrigin::Ninjutsu(attacker) = origin else {
                return Err(EffectError::InvalidCastResource);
            };
            Some((
                attacker,
                self.unblocked_defender(attacker, actor, &evaluated)?,
            ))
        } else {
            None
        };
        if let SourceCost::SacrificeSelected {
            filter,
            count,
            from_graveyard: true,
            ..
        } = selected.source_cost
        {
            if card.zone() != ObjectZone::Graveyard(actor) {
                return Err(EffectError::NotAbilitySource);
            }
            let ActivationOrigin::SelectedCost {
                target: None,
                objects,
            } = &origin
            else {
                return Err(EffectError::InvalidCastResource);
            };
            if count == 0
                || objects.len() != usize::from(count)
                || objects.iter().copied().collect::<BTreeSet<_>>().len() != objects.len()
            {
                return Err(EffectError::InvalidCastResource);
            }
            for id in objects {
                if !matches_sacrifice_filter(*id, filter)? {
                    return Err(EffectError::InvalidCastResource);
                }
            }
        } else if matches!(
            selected.source_cost,
            SourceCost::Exile {
                from_graveyard: true,
                ..
            }
        ) {
            if card.zone() != ObjectZone::Graveyard(actor) {
                return Err(EffectError::NotAbilitySource);
            }
            match (&origin, selected.body.target) {
                (ActivationOrigin::Untargeted, BodyTarget::None) => {}
                (ActivationOrigin::Targeted(_), target) if target != BodyTarget::None => {}
                _ => return Err(EffectError::InvalidCastResource),
            }
        } else if let SourceCost::GraveyardSelf { unearth } = selected.source_cost {
            if card.zone() != ObjectZone::Graveyard(actor) {
                return Err(EffectError::NotAbilitySource);
            }
            if !matches!(origin, ActivationOrigin::Untargeted) {
                return Err(EffectError::UnexpectedTarget);
            }
            if unearth {
                validate_main_phase(&self.window, turn, actor)?;
            }
        } else if let SourceCost::HandDiscardSelf(keyword) = selected.source_cost {
            if card.zone() != ObjectZone::Hand(actor) {
                return Err(EffectError::NotOwnHand);
            }
            match (keyword, &origin) {
                (HandDiscardKeyword::Reinforce, ActivationOrigin::Targeted(_)) => {}
                (HandDiscardKeyword::Reinforce, _) => return Err(EffectError::TargetRequired),
                (HandDiscardKeyword::Cycling, ActivationOrigin::Cycling) => {}
                (HandDiscardKeyword::Channel, ActivationOrigin::Untargeted)
                    if selected.body.target == BodyTarget::None => {}
                (HandDiscardKeyword::Channel, ActivationOrigin::Targeted(_))
                    if selected.body.target != BodyTarget::None
                        && selected.body.permanent_target_bounds().is_none() => {}
                (HandDiscardKeyword::Channel, ActivationOrigin::Untargeted) => {
                    return Err(EffectError::TargetRequired);
                }
                (HandDiscardKeyword::Transmute, ActivationOrigin::Transmute) => {
                    validate_main_phase(&self.window, turn, actor)?;
                }
                (_, ActivationOrigin::Targeted(_)) => return Err(EffectError::UnexpectedTarget),
                _ => return Err(EffectError::UnknownActivatedAbility),
            }
        } else if returned_attacker.is_none() {
            if restriction != Some(ActivationRestriction::AnyPlayer)
                || card.zone() != ObjectZone::Battlefield
            {
                Self::validate_current_source(actor, card)?;
            }
            match (&origin, selected.body.target) {
                (ActivationOrigin::SelectedCost { target, objects }, body_target) => {
                    if selected.body.permanent_target_bounds().is_some() {
                        return Err(EffectError::InconsistentProgram);
                    }
                    if (body_target == BodyTarget::None) != target.is_none() {
                        return Err(if target.is_some() {
                            EffectError::UnexpectedTarget
                        } else {
                            EffectError::TargetRequired
                        });
                    }
                    let unique = objects.iter().copied().collect::<BTreeSet<_>>();
                    if unique.len() != objects.len() {
                        return Err(EffectError::InvalidCastResource);
                    }
                    match selected.source_cost {
                        SourceCost::SacrificeSelected {
                            filter: SelectedSacrificeFilter::GrantingPermanent,
                            ..
                        } => {
                            if !matches!(granting_damage_source.as_ref(), Some(DamageSource::Object { object, .. }) if objects.as_slice() == [object.id()])
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::SacrificeSelected {
                            filter,
                            count,
                            from_graveyard: false,
                            ..
                        } => {
                            if objects.len() != usize::from(count) || count == 0 {
                                return Err(EffectError::InvalidCastResource);
                            }
                            for id in objects {
                                if !matches_sacrifice_filter(*id, filter)? {
                                    return Err(EffectError::InvalidCastResource);
                                }
                            }
                        }
                        SourceCost::TapPermanents {
                            filter,
                            count,
                            tap_source,
                            exclude_source,
                        } if selected.source_cost.uses_selected_cost_origin() => {
                            if let TapCostCount::Fixed(required) = count
                                && objects.len() != usize::from(required.get())
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                            if (tap_source && card.tapped() != Some(false))
                                || objects
                                    .iter()
                                    .any(|id| !matches_tap_filter(*id, filter, exclude_source))
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::ExileGraveyard {
                            filter: GraveyardExileCost::AnyCountX,
                            ..
                        } => {
                            if objects.iter().any(|id| {
                                self.window.cards().get(*id).is_none_or(|card| {
                                    card.zone() != ObjectZone::Graveyard(actor)
                                        || card.card_id().is_none()
                                })
                            }) {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::ExileGraveyard {
                            filter: GraveyardExileCost::Creature { .. },
                            ..
                        } => {
                            let [object] = objects.as_slice() else {
                                return Err(EffectError::InvalidCastResource);
                            };
                            if evaluated.get(*object).is_none_or(|card| {
                                card.zone != ObjectZone::Graveyard(actor) || !card.is_creature()
                            }) || self
                                .window
                                .cards()
                                .get(*object)
                                .is_none_or(|card| card.card_id().is_none())
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::ExileGraveyard {
                            filter: GraveyardExileCost::Filtered { filter, count },
                            ..
                        } => {
                            let excluded = BTreeSet::new();
                            if objects.len() != usize::from(count.get())
                                || objects.iter().any(|object| {
                                    evaluated.get(*object).is_none_or(|card| {
                                        card.zone != ObjectZone::Graveyard(actor)
                                            || !self.matches_object_filter(
                                                filter,
                                                card,
                                                actor,
                                                Some(source),
                                                &excluded,
                                                &evaluated,
                                            )
                                    }) || self
                                        .window
                                        .cards()
                                        .get(*object)
                                        .is_none_or(|card| card.card_id().is_none())
                                })
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::PayLife {
                            amount: LifeCostAmount::Fixed(life),
                            sacrifice: Some(filter),
                            ..
                        } => {
                            if objects.len() != 1
                                || self.life.get(actor).is_none_or(|total| {
                                    total < &RulesInteger::from(i64::from(life))
                                })
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                            if !matches_sacrifice_filter(objects[0], filter)? {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::Discard {
                            mode: DiscardCostMode::WholeHand,
                            ..
                        } => {
                            let hand = self
                                .window
                                .cards()
                                .iter()
                                .filter(|c| c.zone() == ObjectZone::Hand(actor))
                                .map(GameObject::id)
                                .collect::<BTreeSet<_>>();
                            if unique != hand {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::Discard {
                            mode:
                                DiscardCostMode::SelectedOne
                                | DiscardCostMode::MatchingTargetSpellManaValue,
                            ..
                        } => {
                            if objects.len() != 1
                                || self.window.cards().get(objects[0]).is_none_or(|object| {
                                    object.zone() != ObjectZone::Hand(actor)
                                        || object.card_id().is_none()
                                })
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        SourceCost::Discard {
                            mode: DiscardCostMode::Filtered { filter, count },
                            ..
                        } => {
                            let excluded = BTreeSet::new();
                            if objects.len() != usize::from(count.get())
                                || objects.iter().any(|object| {
                                    evaluated.get(*object).is_none_or(|card| {
                                        card.zone != ObjectZone::Hand(actor)
                                            || !self.matches_object_filter(
                                                filter,
                                                card,
                                                actor,
                                                Some(source),
                                                &excluded,
                                                &evaluated,
                                            )
                                    }) || self
                                        .window
                                        .cards()
                                        .get(*object)
                                        .is_none_or(|card| card.card_id().is_none())
                                })
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                        }
                        _ => return Err(EffectError::InvalidCastResource),
                    }
                }
                (_, _) if selected.source_cost.uses_selected_cost_origin() => {
                    return Err(EffectError::InvalidCastResource);
                }
                (ActivationOrigin::Contributors(objects), BodyTarget::None)
                    if selected.source_cost.uses_contributors_origin()
                        && selected.source_cost.power_cost().is_none() =>
                {
                    let SourceCost::TapPermanents {
                        filter,
                        count: TapCostCount::Fixed(required),
                        exclude_source,
                        ..
                    } = selected.source_cost
                    else {
                        return Err(EffectError::InvalidPowerContributors);
                    };
                    let unique = objects.iter().copied().collect::<BTreeSet<_>>();
                    if objects.len() != usize::from(required.get())
                        || unique.len() != objects.len()
                        || objects
                            .iter()
                            .any(|id| !matches_tap_filter(*id, filter, exclude_source))
                    {
                        return Err(EffectError::InvalidPowerContributors);
                    }
                }
                (_, _)
                    if selected.source_cost.uses_contributors_origin()
                        && selected.source_cost.power_cost().is_none() =>
                {
                    return Err(EffectError::InvalidPowerContributors);
                }
                (ActivationOrigin::Contributors(objects), BodyTarget::None)
                    if selected.source_cost.power_cost().is_some() =>
                {
                    self.validate_power_contributors(
                        selected.source_cost,
                        source,
                        actor,
                        objects,
                        &evaluated,
                        turn,
                    )?
                }
                (_, _) if selected.source_cost.power_cost().is_some() => {
                    return Err(EffectError::InvalidPowerContributors);
                }
                (ActivationOrigin::PermanentTargets(_), _) if fixed_activation.is_some() => {}
                (ActivationOrigin::PermanentTargets(targets), _)
                    if selected.body.permanent_target_bounds().is_some() =>
                {
                    let (min, max) = selected
                        .body
                        .permanent_target_bounds()
                        .ok_or(EffectError::InconsistentProgram)?;
                    let bound_target = selected.body.target.with_ability_source(source);
                    let mut controllers = BTreeSet::new();
                    if selected.body.instructions == [Instruction::ReturnOneNonlandPerOpponent] && targets.iter().any(|target| {
                        !matches!(target,TargetRef::Object(id) if evaluated.get(*id).and_then(|value|value.controller).is_some_and(|player|controllers.insert(player)))
                    }) {return Err(EffectError::IllegalTarget);}
                    if !(min..=max).contains(&targets.len()) || targets.iter().any(|target|
                        (matches!(selected.body.target, BodyTarget::FilteredPermanent(filter) if filter.excludes_ability_source()) && *target == TargetRef::Object(source)) ||
                        Self::protected_target(*target,&protection::ProtectionSource::from(current),&evaluated)
                        || if bound_target == selected.body.target {
                            !self.legal_body_target_in(&selected.body, *target, actor, &evaluated)
                        } else {
                            !self.legal_target_kind_in(bound_target, *target, actor, &evaluated)
                        }) {
                        return Err(EffectError::IllegalTarget);
                    }
                }
                (_, _) if selected.body.permanent_target_bounds().is_some() => {
                    return Err(EffectError::TargetRequired);
                }
                (ActivationOrigin::Untargeted, BodyTarget::None) => {}
                (ActivationOrigin::Targeted(_), BodyTarget::None) => {
                    return Err(EffectError::UnexpectedTarget);
                }
                (ActivationOrigin::Targeted(_), _) => {}
                _ => return Err(EffectError::TargetRequired),
            }
        }
        let spell_target_mana_value = if matches!(
            selected.source_cost,
            SourceCost::Discard {
                mode: DiscardCostMode::MatchingTargetSpellManaValue,
                ..
            }
        ) {
            let ActivationOrigin::SelectedCost {
                objects,
                target: Some(TargetRef::Stack(spell)),
            } = &origin
            else {
                return Err(EffectError::TargetRequired);
            };
            let [card] = objects.as_slice() else {
                return Err(EffectError::InvalidCastResource);
            };
            let value = self
                .base_program(*card)
                .ok_or(EffectError::InvalidCastResource)?
                .mana_value();
            if self.entry_spell_mana_value(*spell).as_ref() != Some(&value) {
                return Err(EffectError::IllegalTarget);
            }
            Some(value)
        } else {
            None
        };
        self.validate_attach_activation(&selected.body, source, actor, turn, target, &evaluated)?;
        if selected.source_cost.taps_source() || selected.source_cost.untaps_source() {
            Self::validate_creature_tap_history(card, current)?;
        }
        if let Some(energy_cost) = selected.source_cost.energy_cost() {
            let available = self.player_counters_of_kind(actor, CounterKind::Energy);
            if available < i64::from(energy_cost).into() {
                return Err(EffectError::InvalidCastResource);
            }
        }
        self.check_split_second(&evaluated)?;
        if selected.body.target == same_name_ability_target()
            && !target.is_some_and(|target| {
                self.target_shares_ability_source_name(source, target, &evaluated)
            })
        {
            return Err(EffectError::IllegalTarget);
        }
        if let Some(target) = target
            && ((matches!(selected.body.target, BodyTarget::FilteredPermanent(filter) if filter.excludes_ability_source())
                && target == TargetRef::Object(source))
                || Self::protected_target(
                    target,
                    &protection::ProtectionSource::from(current),
                    &evaluated,
                )
                || !self.legal_body_target_in(&selected.body, target, actor, &evaluated))
        {
            return Err(EffectError::IllegalTarget);
        }
        let quoted = if let Some(symbols) = selected.source_cost.generic_x_symbols() {
            let mut cost = selected.mana_cost.clone();
            cost.generic = cost.generic.plus(
                &chosen_x
                    .as_ref()
                    .ok_or(EffectError::InvalidCastResource)?
                    .times(&i64::from(symbols).into()),
            );
            cost
        } else if restriction
            == Some(
                crate::activated::ActivationRestriction::DiscountLegendaryCreaturesAndPlaneswalkers,
            )
        {
            let mut price = selected.mana_cost.clone();
            let count = controlled()
                .filter(|p| {
                    p.current_supertypes
                        .contains(&crate::program::Supertype::Legendary)
                        && (p.is_creature() || p.has_type(CardType::Planeswalker))
                })
                .count();
            price.generic = price
                .generic
                .minus(&RulesInteger::from_usize(count))
                .max(0.into());
            price
        } else if restriction == Some(crate::activated::ActivationRestriction::PowerUp) {
            self.power_up_mana_cost(source, &selected.mana_cost)
        } else {
            selected.mana_cost.clone()
        };
        let pay = |cost: &ManaCost| {
            self.prepare_restricted_mana_payment(
                cost,
                payment.as_ref(),
                actor,
                restricted,
                |rule| self.restriction_allows_ability(rule, current),
            )
        };
        let (quoted, spend, selected_life) = if matches!(
            selected.source_cost,
            SourceCost::ExileGraveyard {
                filter: GraveyardExileCost::Creature {
                    pay_mana_cost: true
                },
                ..
            }
        ) {
            let ActivationOrigin::SelectedCost { objects, .. } = &origin else {
                return Err(EffectError::InvalidCastResource);
            };
            let [object] = objects.as_slice() else {
                return Err(EffectError::InvalidCastResource);
            };
            let definition = self
                .copiable_program(*object)
                .ok_or(EffectError::InvalidCastResource)?;
            let price = definition
                .permanent()
                .and_then(|permanent| permanent.casting_cost.as_ref())
                .ok_or(EffectError::InvalidCastResource)?;
            price
                .hybrid_prices(&definition.hybrid_symbols)
                .into_iter()
                .find_map(|(mana, life)| {
                    if life.is_positive()
                        && (evaluated.life_and_creature_costs_prohibited()
                            || self.life.get(actor).is_none_or(|total| total < &life))
                    {
                        return None;
                    }
                    let spend = pay(&mana).ok()?;
                    Some((mana, spend, life.is_positive().then_some(life)))
                })
                .ok_or(EffectError::InvalidCastResource)?
        } else {
            let spend = pay(&quoted)?;
            (quoted, spend, None)
        };
        let announced = if let Some((_, _, choices)) = &fixed_activation {
            let clauses = choices
                .iter()
                .map(|choice| TargetClause {
                    id: choice.clause,
                    minimum: 1,
                    maximum: 1,
                })
                .collect();
            self.window.record_targeted_activation(
                self.window.revision(),
                actor,
                card.clone(),
                clauses,
                choices.clone(),
            )?
        } else if let crate::activated::ActivationOrigin::PermanentTargets(targets) = &origin {
            let (minimum, maximum) = selected
                .body
                .permanent_target_bounds()
                .ok_or(EffectError::InconsistentProgram)?;
            let choices = targets
                .iter()
                .enumerate()
                .map(|(n, target)| {
                    Ok(TargetChoice {
                        id: TargetId(u32::try_from(n).map_err(|_| EffectError::IllegalTarget)?),
                        clause: TargetClauseId(0),
                        target: *target,
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()?;
            self.window.record_targeted_activation(
                self.window.revision(),
                actor,
                card.clone(),
                vec![TargetClause {
                    id: TargetClauseId(0),
                    minimum,
                    maximum,
                }],
                choices,
            )?
        } else if let Some(target) = target {
            self.window.record_targeted_activation(
                self.window.revision(),
                actor,
                card.clone(),
                vec![TargetClause {
                    id: TargetClauseId(0),
                    minimum: 1,
                    maximum: 1,
                }],
                vec![TargetChoice {
                    id: TargetId(0),
                    clause: TargetClauseId(0),
                    target,
                }],
            )?
        } else {
            self.window
                .record_activation(self.window.revision(), actor, card.clone())?
        };
        let entry = announced
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .id();
        let mut next = self.clone();
        next.revision = revision;
        next.window = announced;
        if matches!(
            restriction,
            Some(
                crate::activated::ActivationRestriction::OnceEachTurn
                    | crate::activated::ActivationRestriction::SorceryOnceEachTurn
                    | crate::activated::ActivationRestriction::DuringYourTurnOnceEachTurn
            )
        ) {
            next.activated_this_turn.insert(exhausted_id.clone());
        }
        if matches!(
            restriction,
            Some(
                crate::activated::ActivationRestriction::Exhaust
                    | crate::activated::ActivationRestriction::PowerUp
            )
        ) {
            next.characteristics.exhausted.insert(exhausted_id);
        }
        let (mut body, fixed_segments) = match fixed_activation {
            Some((body, segments, _)) => (body, segments),
            None => (selected.body.clone(), Vec::new()),
        };
        body.target = body.target.with_ability_source(source);
        for instruction in &mut body.instructions {
            if let Instruction::DamageFromGrantingPermanent { amount } = instruction {
                if granting_damage_source.is_none() {
                    return Err(EffectError::InconsistentProgram);
                }
                *instruction = Instruction::Damage {
                    amount: amount.clone(),
                };
            }
            if let Instruction::SearchLibrary { filter } = instruction
                && filter.is_transfigure()
                && let Some(crate::program::SearchManaValue::Exact(value)) = &mut filter.mana_value
            {
                if selected.source_cost != crate::activated::SourceCost::Sacrifice {
                    return Err(EffectError::InconsistentProgram);
                }
                *value = self
                    .permanent_mana_value(source)
                    .ok_or(EffectError::InconsistentProgram)?;
            }
        }
        if let Some(x) = chosen_x.as_ref().or(selected_tap_count.as_ref()) {
            next.window.set_chosen_x(entry, x.clone())?;
        }
        let mut payload = StackPayload::from_body(body, None);
        if let StackPayload::Instructions(value) = &mut payload {
            value.modes = fixed_segments;
            value.activated_ability = Some(terms.origin);
            value.granting_damage_source = granting_damage_source.map(Box::new);
            value.paid_named_counters = counter_cost_plan
                .as_ref()
                .and_then(|(_, _, paid)| paid.clone());
            value.activated_as_loyalty = loyalty_cost.is_some();
            if current.has_type(CardType::Planeswalker) {
                value
                    .activated_planeswalker_types
                    .clone_from(&current.current_planeswalker_subtypes);
            }
            value.spell_target_mana_value = spell_target_mana_value;
            if value.body.instructions == [Instruction::ReturnOneNonlandPerOpponent]
                && let crate::activated::ActivationOrigin::PermanentTargets(targets) = &origin
            {
                for (index, target) in targets.iter().enumerate() {
                    let TargetRef::Object(object) = target else {
                        return Err(EffectError::IllegalTarget);
                    };
                    value.target_controllers.insert(
                        TargetId(index.try_into().map_err(|_| EffectError::IllegalTarget)?),
                        evaluated
                            .get(*object)
                            .and_then(|p| p.controller)
                            .ok_or(EffectError::IllegalTarget)?,
                    );
                }
            }
        }
        if let StackPayload::Instructions(value) = &mut payload
            && value.body.instructions == [Instruction::DestroyNonlandsAtSourceChargeCount]
        {
            value.source_charge_count = Some(
                next.characteristics
                    .charge
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| 0.into()),
            );
        }
        if let StackPayload::Instructions(value) = &mut payload
            && value.body.instructions == [Instruction::Encore]
        {
            value.encore_source = Some(Box::new(
                self.rules_snapshot(None)?
                    .view()
                    .appearance(source)
                    .ok_or(EffectError::InconsistentProgram)?,
            ));
        }
        next.payloads.insert(entry, payload);
        // The actual entry and captured body exist before any source-cost write.
        let mut cost_before = next.rules_snapshot(None)?;
        let mut append = RulesJournal::default();
        let life_cost = match selected.source_cost {
            SourceCost::PayLife {
                amount: LifeCostAmount::Fixed(life),
                ..
            } => Some(RulesInteger::from(i64::from(life))),
            SourceCost::PayLife {
                amount: LifeCostAmount::CommanderColors,
                ..
            } => Some(RulesInteger::from_usize(
                next.commander_mana_colors(actor)?.len(),
            )),
            _ => None,
        }
        .or(selected_life);
        if let Some(life) = life_cost {
            next.pay_life_at_cut(
                actor,
                life,
                crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                turn,
                &mut append,
            )?;
            cost_before = next.rules_snapshot(None)?;
        }
        if let Some(energy_cost) = selected.source_cost.energy_cost()
            && energy_cost > 0
        {
            let before = next.player_counters_of_kind(actor, CounterKind::Energy);
            let after = before.minus(&i64::from(energy_cost).into());
            let map = next
                .characteristics
                .player_counters
                .entry(CounterKind::Energy)
                .or_default();
            if after.is_positive() {
                map.insert(actor, after.clone());
            } else {
                map.remove(&actor);
            }
            next.emit_from(
                crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                &cost_before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![
                        crate::occurrence::RulesOccurrence::PlayerCounterChanged {
                            player: actor,
                            kind: CounterKind::Energy,
                            before: before.clone(),
                            after: after.clone(),
                        },
                    ])
                },
            )?;
            cost_before = next.rules_snapshot(None)?;
        }
        let mut inline_counter_occurrence: Option<crate::occurrence::RulesOccurrence> = None;
        if let Some((kind, change, _)) =
            counter_cost_plan.filter(|(_, change, _)| *change != 0.into())
        {
            let inline_with_tap = matches!(
                selected.source_cost,
                SourceCost::Counter {
                    tap_source: true,
                    sacrifice_source: false,
                    ..
                }
            ) && matches!(
                kind,
                CounterKind::PlusOne | CounterKind::Charge | CounterKind::Suspect
            );
            match kind {
                CounterKind::PlusOne | CounterKind::MinusOne => {
                    let before = next.characteristics.counters(source);
                    let delta_counters = if kind == CounterKind::PlusOne {
                        crate::characteristics::PowerToughnessCounters {
                            plus_one: if change.is_positive() {
                                change.clone()
                            } else {
                                RulesInteger::from(0).minus(&change)
                            },
                            minus_one: 0.into(),
                        }
                    } else {
                        crate::characteristics::PowerToughnessCounters {
                            plus_one: 0.into(),
                            minus_one: if change.is_positive() {
                                change.clone()
                            } else {
                                RulesInteger::from(0).minus(&change)
                            },
                        }
                    };
                    if change.is_positive() {
                        next.characteristics
                            .place_counters(source, &delta_counters)?;
                        let after = next.characteristics.counters(source);
                        next.emit_from(
                            crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                            &cost_before,
                            None,
                            &mut append,
                            |previous, current, _| {
                                Ok(vec![crate::occurrence::RulesOccurrence::CountersPlaced {
                                    object: source,
                                    before: before.clone(),
                                    added: delta_counters.clone(),
                                    after: after.clone(),
                                    before_appearance: Box::new(
                                        previous
                                            .appearance(source)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                    after_appearance: Box::new(
                                        current
                                            .appearance(source)
                                            .ok_or(EffectError::InconsistentProgram)?,
                                    ),
                                }])
                            },
                        )?;
                        cost_before = next.rules_snapshot(None)?;
                    } else {
                        let after = next
                            .characteristics
                            .take_power_toughness_counters(source, &delta_counters)
                            .map_err(|_| EffectError::InvalidCastResource)?;
                        let occurrence =
                            crate::occurrence::RulesOccurrence::PowerToughnessCountersRemoved {
                                object: source,
                                before,
                                after,
                            };
                        if inline_with_tap {
                            inline_counter_occurrence = Some(occurrence);
                        } else {
                            next.emit_from(
                                crate::occurrence::EventCause::ActivatedAbilityCost {
                                    entry,
                                    ability,
                                },
                                &cost_before,
                                None,
                                &mut append,
                                |_, _, _| Ok(vec![occurrence.clone()]),
                            )?;
                            cost_before = next.rules_snapshot(None)?;
                        }
                    }
                }
                _ => {
                    let counters = next
                        .characteristics
                        .named_permanent_counters_mut(kind)
                        .ok_or(EffectError::InconsistentProgram)?;
                    let before = counters.get(&source).cloned().unwrap_or_else(|| 0.into());
                    let after = before.plus(&change);
                    if after < 0.into() {
                        return Err(EffectError::InvalidCastResource);
                    }
                    if after.is_positive() {
                        counters.insert(source, after.clone());
                    } else {
                        counters.remove(&source);
                    }
                    let occurrence = match kind {
                        CounterKind::Charge => {
                            crate::occurrence::RulesOccurrence::ChargeCountersChanged {
                                object: source,
                                before,
                                after,
                            }
                        }
                        CounterKind::Suspect => {
                            crate::occurrence::RulesOccurrence::SuspectCountersChanged {
                                object: source,
                                before,
                                after,
                            }
                        }
                        _ => crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                            object: source,
                            kind,
                            before,
                            after,
                        },
                    };
                    if inline_with_tap {
                        inline_counter_occurrence = Some(occurrence);
                    } else {
                        next.emit_from(
                            crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                            &cost_before,
                            None,
                            &mut append,
                            |_, _, _| Ok(vec![occurrence.clone()]),
                        )?;
                        cost_before = next.rules_snapshot(None)?;
                    }
                }
            }
        }
        let loyalty_change = loyalty_cost.map(|cost| {
            let before = next
                .characteristics
                .loyalty
                .get(&source)
                .cloned()
                .unwrap_or_else(|| 0.into());
            let after = if let Some(x) = chosen_x.as_ref() {
                before.minus(x)
            } else {
                before.plus(&i64::from(cost).into())
            };
            let value = if uses_extra_loyalty {
                next.characteristics
                    .extra_loyalty_used
                    .entry((source, actor))
                    .or_insert_with(|| 0.into())
            } else {
                next.characteristics
                    .loyalty_used
                    .entry(source)
                    .or_insert_with(|| 0.into())
            };
            *value = value.plus(&1.into());
            if current.has_type(CardType::Planeswalker) {
                next.characteristics.loyalty_activators.insert(actor);
            }
            if after.is_positive() {
                next.characteristics.loyalty.insert(source, after.clone());
            } else {
                next.characteristics.loyalty.remove(&source);
            }
            (before, after)
        });
        let tapped_source = if selected.source_cost.taps_source_before_move() {
            let (window, tapped) = next.window.pay_announced_source_cost(
                entry,
                actor,
                source,
                SourceCost::Tap,
                false,
            )?;
            next.window = window;
            next.emit_from(
                crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                &cost_before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![crate::occurrence::RulesOccurrence::StatusChange {
                        change: crate::object_identity::ObjectStatusChange {
                            before: card.clone(),
                            after: tapped.clone(),
                        },
                        reason: crate::occurrence::StatusReason::ActivatedAbilityCost,
                    }])
                },
            )?;
            cost_before = next.rules_snapshot(None)?;
            Some(tapped)
        } else {
            None
        };
        let selected_objects = match &origin {
            _ if matches!(selected.source_cost, SourceCost::Mill { .. }) => {
                let SourceCost::Mill { count, .. } = selected.source_cost else {
                    return Err(EffectError::InconsistentProgram);
                };
                let library = next.window.cards().library(actor);
                let needed = usize::from(count.get());
                if library.len() < needed {
                    return Err(EffectError::InvalidCastResource);
                }
                Some(library[..needed].to_vec())
            }
            ActivationOrigin::SelectedCost { objects, .. }
                if !matches!(selected.source_cost, SourceCost::TapPermanents { .. }) =>
            {
                Some(objects.clone())
            }
            _ => None,
        };
        if let Some(objects) = selected_objects {
            let (action, reason) = match selected.source_cost {
                SourceCost::Mill { .. } => (
                    crate::program::ZoneActionKind::MillCost,
                    crate::occurrence::ZoneReason::Mill { player: actor },
                ),
                SourceCost::Discard { .. } => (
                    crate::program::ZoneActionKind::DiscardCost,
                    crate::occurrence::ZoneReason::Discard { player: actor },
                ),
                SourceCost::ExileGraveyard { .. } => (
                    crate::program::ZoneActionKind::ExileCost,
                    crate::occurrence::ZoneReason::ExileCost,
                ),
                _ => (
                    crate::program::ZoneActionKind::SacrificeCost,
                    crate::occurrence::ZoneReason::SacrificeCost,
                ),
            };
            let mut batch =
                next.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
                    cause: crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
                    action,
                    intents: objects
                        .iter()
                        .map(|id| zone_replacement::ZoneMoveIntent {
                            before: *id,
                            reason,
                        })
                        .collect(),
                })?;
            batch.progress(next.revision)?;
            let activation = AbilityActivationRecord {
                granting_effect,
                entry,
                ability,
                controller: actor,
                source: card.clone(),
                source_cost: selected.source_cost,
                resulting_source: source,
            };
            let receipt = ManaPaymentRecord {
                from_lots: payment
                    .as_ref()
                    .map_or_else(Default::default, |allocation| allocation.from_lots.clone()),
                source: entry,
                payer: actor,
                cost: quoted.clone(),
                spend: spend.clone(),
                restricted_mana: restricted.to_vec(),
            };
            next.pending_work = Some(PendingWork::SelectedActivationCost(Box::new(
                selected_activation_cost::SelectedActivationCostWork {
                    batch,
                    objects,
                    graveyard_orders: BTreeMap::new(),
                    owner: selected_activation_cost::SelectedActivationOwner::Permanent(Box::new(
                        activation.clone(),
                    )),
                    receipt: receipt.clone(),
                    action,
                },
            )));
            let (mut state, _, more) = next.progress_selected_activation_cost(turn)?.into_parts();
            append.append(more);
            let pending = state.has_deferred_payment();
            let (receipt, activation) = match state.take_completed_deferred_receipts() {
                Some((receipt, Some(activation))) => (receipt, activation),
                _ => (receipt, activation),
            };
            return Ok(AbilityActivationTransition::new(
                state,
                crate::activated::AbilityActivationResult {
                    pending,
                    activation,
                    payment: receipt,
                },
                append,
            ));
        }
        let mut sacrifice_destination = None;
        if (selected.source_cost.sacrifices_source() || selected.source_cost.exiles_source())
            && !next.zone_replacement_sources()?.sources.is_empty()
        {
            let cause = crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability };
            let exiling = selected.source_cost.exiles_source();
            let action = if exiling {
                crate::program::ZoneActionKind::ExileCost
            } else {
                crate::program::ZoneActionKind::SacrificeCost
            };
            let reason = if exiling {
                crate::occurrence::ZoneReason::ExileCost
            } else {
                crate::occurrence::ZoneReason::SacrificeCost
            };
            let mut batch =
                next.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
                    cause,
                    action,
                    intents: vec![zone_replacement::ZoneMoveIntent {
                        before: source,
                        reason,
                    }],
                })?;
            if batch.progress(next.revision)?.is_some() {
                let activation = AbilityActivationRecord {
                    granting_effect,
                    entry,
                    ability,
                    controller: actor,
                    source: card.clone(),
                    source_cost: selected.source_cost,
                    resulting_source: source,
                };
                let receipt = ManaPaymentRecord {
                    from_lots: payment
                        .as_ref()
                        .map_or_else(Default::default, |allocation| allocation.from_lots.clone()),
                    source: entry,
                    payer: actor,
                    cost: quoted.clone(),
                    spend: spend.clone(),
                    restricted_mana: restricted.to_vec(),
                };
                next.pending_work =
                    Some(PendingWork::ActivationCost(Box::new(ActivationCostWork {
                        batch,
                        activation: activation.clone(),
                        movement_before: tapped_source.clone().unwrap_or_else(|| card.clone()),
                        receipt: receipt.clone(),
                    })));
                return Ok(AbilityActivationTransition::new(
                    next,
                    crate::activated::AbilityActivationResult {
                        pending: true,
                        activation,
                        payment: receipt,
                    },
                    append,
                ));
            }
            sacrifice_destination = Some(batch.destination(source)?);
        }
        let madness = selected.source_cost.discards_source()
            && next
                .copiable_program(source)
                .is_some_and(|p| p.madness.is_some());
        let (window, source_after) = if selected.source_cost.power_cost().is_some()
            || selected.source_cost == SourceCost::None
        {
            (next.window.clone(), card.clone())
        } else if let Some(destination) = sacrifice_destination {
            next.window.pay_announced_source_to(
                entry,
                actor,
                source,
                destination
                    .physical(None)
                    .ok_or(EffectError::InconsistentProgram)?,
            )?
        } else {
            let step_cost = if selected.source_cost.sacrifices_source() {
                SourceCost::Sacrifice
            } else if selected.source_cost.exiles_source() {
                SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: selected.source_cost.is_from_graveyard(),
                }
            } else if selected.source_cost.returns_source_to_hand() {
                SourceCost::ReturnSelfToHand { tap_source: false }
            } else {
                selected.source_cost
            };
            next.window
                .pay_announced_source_cost(entry, actor, source, step_cost, madness)?
        };
        next.window = window;
        let returned = if let Some((object, defender)) = returned_attacker {
            next.window.note_attacking_defender(entry, defender)?;
            let (changed, before, after) = next.window.pay_cast_resource(
                entry,
                crate::mana::CastResource::Sneak(object),
                false,
            )?;
            next.window = changed;
            Some(crate::object_identity::ObjectMovement { before, after })
        } else {
            None
        };
        let cost_taps = if let ActivationOrigin::Contributors(mut objects)
        | ActivationOrigin::SelectedCost { mut objects, .. } = origin
        {
            if matches!(
                selected.source_cost,
                SourceCost::TapPermanents {
                    tap_source: true,
                    ..
                }
            ) {
                objects.push(source);
            }
            let (window, taps) = next.window.pay_power_contributors(entry, &objects)?;
            next.window = window;
            taps
        } else {
            Vec::new()
        };
        let activation = AbilityActivationRecord {
            granting_effect,
            entry,
            ability,
            controller: actor,
            source: card.clone(),
            source_cost: selected.source_cost,
            resulting_source: source_after.id(),
        };
        let receipt = ManaPaymentRecord {
            from_lots: payment
                .as_ref()
                .map_or_else(Default::default, |allocation| allocation.from_lots.clone()),
            source: entry,
            payer: actor,
            cost: quoted,
            spend,
            restricted_mana: restricted.to_vec(),
        };
        next.emit_from(
            crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
            &cost_before,
            None,
            &mut append,
            |previous, current, controllers| {
                let cost = selected.source_cost;
                if matches!(
                    cost,
                    SourceCost::SacrificeSelected { .. }
                        | SourceCost::Discard { .. }
                        | SourceCost::ExileGraveyard { .. }
                        | SourceCost::Mill { .. }
                        | SourceCost::PayLife {
                            sacrifice: Some(_),
                            ..
                        }
                ) {
                    return Err(EffectError::InconsistentProgram);
                }
                if cost.is_ninjutsu() {
                    return Ok(vec![zone_occurrence_at(
                        previous,
                        current,
                        returned.as_ref().ok_or(EffectError::InconsistentProgram)?,
                        crate::occurrence::ZoneReason::ReturnToOwnersHand,
                        controllers,
                    )?]);
                }
                if cost.power_cost().is_some() || matches!(cost, SourceCost::TapPermanents { .. }) {
                    return Ok(occurrence_flow::status_occurrences(
                        cost_taps,
                        crate::occurrence::StatusReason::ActivatedAbilityCost,
                    ));
                }
                if cost.sacrifices_source() || cost.exiles_source() || cost.discards_source() {
                    let reason = match cost {
                        SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling) => {
                            crate::occurrence::ZoneReason::CycleDiscard { player: actor }
                        }
                        SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute) => {
                            crate::occurrence::ZoneReason::TransmuteDiscard { player: actor }
                        }
                        SourceCost::HandDiscardSelf(_) => {
                            crate::occurrence::ZoneReason::Discard { player: actor }
                        }
                        SourceCost::Exile { .. } => crate::occurrence::ZoneReason::ExileCost,
                        _ => crate::occurrence::ZoneReason::SacrificeCost,
                    };
                    return Ok(vec![occurrence_flow::madness_replaced(
                        zone_occurrence_at(
                            previous,
                            current,
                            &crate::object_identity::ObjectMovement {
                                before: tapped_source
                                    .clone()
                                    .unwrap_or_else(|| activation.source.clone()),
                                after: source_after.clone(),
                            },
                            reason,
                            controllers,
                        )?,
                        madness,
                    )]);
                }
                if cost.returns_source_to_hand() {
                    return Ok(vec![zone_occurrence_at(
                        previous,
                        current,
                        &crate::object_identity::ObjectMovement {
                            before: tapped_source
                                .clone()
                                .unwrap_or_else(|| activation.source.clone()),
                            after: source_after.clone(),
                        },
                        crate::occurrence::ZoneReason::ReturnToOwnersHand,
                        controllers,
                    )?]);
                }
                if cost.taps_source() || cost.untaps_source() {
                    let mut out = vec![crate::occurrence::RulesOccurrence::StatusChange {
                        change: crate::object_identity::ObjectStatusChange {
                            before: activation.source.clone(),
                            after: source_after.clone(),
                        },
                        reason: crate::occurrence::StatusReason::ActivatedAbilityCost,
                    }];
                    if let Some(occurrence) = inline_counter_occurrence.clone() {
                        out.push(occurrence);
                    }
                    return Ok(out);
                }
                Ok(loyalty_change
                    .filter(|(before, after)| before != after)
                    .map(|(before, after)| {
                        crate::occurrence::RulesOccurrence::LoyaltyCountersChanged {
                            object: source,
                            before,
                            after,
                        }
                    })
                    .into_iter()
                    .collect())
            },
        )?;
        if next.linked_return_work.is_some() {
            next.deferred_activation = Some(DeferredActivation {
                activation: activation.clone(),
                receipt: receipt.clone(),
            });
            return Ok(AbilityActivationTransition::new(
                next,
                crate::activated::AbilityActivationResult {
                    pending: true,
                    activation,
                    payment: receipt,
                },
                append,
            ));
        }
        next.debit_mana_at_cut(&receipt, false, None, &mut append)?;
        let complete = next.rules_snapshot(None)?;
        next.emit_from(
            crate::occurrence::EventCause::ActivatedAbilityCompleted { entry, ability },
            &complete,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::ActivatedAbility {
                    entry,
                    source: activation.source.clone(),
                    ability,
                }])
            },
        )?;
        Ok(AbilityActivationTransition::new(
            next,
            crate::activated::AbilityActivationResult {
                pending: false,
                activation,
                payment: receipt,
            },
            append,
        ))
    }
}
