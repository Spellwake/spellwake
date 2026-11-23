//! Entry counters are installed on the new incarnation before its first battlefield observation.
use super::*;
use super::{occurrence_flow::RulesSnapshot, permanent_counters::controlled_counter_multiplier};
use crate::program::EntryCounterAbility;

/// A token template may need history from before the token was created.
pub(super) fn definition_or_token_matches(
    program: &Program,
    matches: fn(&Program) -> bool,
) -> bool {
    matches(program)
        || program
            .triggered
            .iter()
            .map(|a| &a.body)
            .chain(program.activated.iter().map(|a| &a.body))
            .chain(match &program.kind {
                ProgramKind::Spell(s) => Some(&s.body),
                ProgramKind::Permanent(_) => None,
            })
            .flat_map(|b| &b.instructions)
            .any(|op| match op {
                Instruction::CreateTokenAndAttach { definition } => {
                    definition_or_token_matches(definition.program(), matches)
                }
                Instruction::CreateTokens { creations } => creations
                    .iter()
                    .any(|c| definition_or_token_matches(c.definition.program(), matches)),
                _ => false,
            })
}
pub(super) fn needs_damage_history(program: &Program) -> bool {
    definition_or_token_matches(program, |p| {
        p.entry_counter_abilities.iter().any(|a| {
            matches!(
                a,
                EntryCounterAbility::Bloodthirst(_) | EntryCounterAbility::BloodthirstX
            )
        })
    })
}

impl EffectWindow {
    /// Actual damage dealt to each player this turn in games with a printed bloodthirst ability.
    pub fn damage_to_players_this_turn(&self) -> Option<&BTreeMap<SeatId, RulesInteger>> {
        self.damage_to_players_this_turn.as_ref()
    }
    /// Loyalty counters on current permanent incarnations.
    pub fn loyalty_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.loyalty
    }
    /// Player designated to protect this current battle incarnation.
    pub fn battle_protector(&self, battle: ObjectId) -> Option<SeatId> {
        self.characteristics.battle_protectors.get(&battle).copied()
    }
    /// Lore counters on current permanent incarnations.
    pub fn lore_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.lore
    }
    /// Storage counters on live objects.
    pub fn storage_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.storage
    }
    /// Suspect counters by current incarnation.
    pub fn suspect_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.suspect_counters
    }
    /// Charge counters by current incarnation.
    pub fn charge_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.charge
    }

    fn apply_entry_status(
        &mut self,
        object: ObjectId,
        before: &RulesSnapshot,
    ) -> Result<(), EffectError> {
        use crate::land::EntryTap;
        let has_entry_tap = self
            .copiable_program(object)
            .and_then(Program::permanent)
            .and_then(|p| p.entry_tap)
            .is_some();
        let mut tapped = false;
        if has_entry_tap {
            let current = self
                .current_characteristics(object)?
                .ok_or(EffectError::InconsistentProgram)?;
            if let Some(rule) = current.abilities.entry_tap {
                let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
                let previous = before.view();
                let lands =
                    previous.evaluated.objects.values().filter(|p| {
                        p.zone == ObjectZone::Battlefield && p.has_type(CardType::Land)
                    });
                let mut controlled = lands.clone().filter(|p| p.controller == Some(controller));
                let opponents = || {
                    self.combat_rules
                        .opponents(previous.window.participants(), controller)
                };
                tapped = match rule {
                    EntryTap::ReadAhead { .. } => false,
                    EntryTap::Always => true,
                    EntryTap::AlwaysChooseColorExcept(excluded) => {
                        let chosen = self
                            .entry_color_choices
                            .get(&object)
                            .ok_or(EffectError::EntryColorChoiceRequired)?;
                        if *chosen == excluded || *chosen == ManaType::Colourless {
                            return Err(EffectError::IllegalEntryColor);
                        }
                        true
                    }
                    EntryTap::ChosenOpponentControls => {
                        if let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.remove(&object)
                        {
                            self.window.set_entering_controller(object, player)?;
                        } else if !opponents()?.is_empty() {
                            return Err(EffectError::SelectionRequired);
                        }
                        false
                    }
                    EntryTap::ChooseNonlandCardName => {
                        if !matches!(
                            self.entry_choices.get(&object),
                            Some(crate::land::EntryChoice::CardName(_))
                        ) {
                            return Err(EffectError::SelectionRequired);
                        }
                        false
                    }
                    EntryTap::ChooseCreatureType => {
                        let chosen = self
                            .entry_creature_type_choices
                            .get(&object)
                            .ok_or(EffectError::SelectionRequired)?;
                        if self.creature_vocabulary.label(*chosen).is_none() {
                            return Err(EffectError::IllegalTarget);
                        }
                        false
                    }
                    EntryTap::SiegeProtector => {
                        let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !opponents()?.contains(&player) {
                            return Err(EffectError::IllegalTarget);
                        }
                        self.characteristics
                            .battle_protectors
                            .insert(object, player);
                        false
                    }
                    EntryTap::ChooseTwoPlayers => {
                        let Some(crate::land::EntryChoice::Players(players)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !self.valid_distinct_players(players) {
                            return Err(EffectError::IllegalTarget);
                        }
                        self.characteristics
                            .chosen_entry_players
                            .insert(object, players);
                        false
                    }
                    EntryTap::UnlessTwoOtherLands => controlled.count() < 2,
                    EntryTap::UnlessTwoOrFewerOtherLands => controlled.count() > 2,
                    EntryTap::UnlessTwoBasicLands => {
                        controlled
                            .filter(|p| {
                                p.current_supertypes
                                    .contains(&crate::program::Supertype::Basic)
                            })
                            .count()
                            < 2
                    }
                    EntryTap::UnlessLandType(a, b) => !controlled.any(|p| {
                        p.current_land_subtypes.contains(&a.land_type())
                            || p.current_land_subtypes.contains(&b.land_type())
                    }),
                    EntryTap::UnlessRevealHandLandType(a, b) => {
                        let Some(crate::land::EntryChoice::Reveal(revealed)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if let Some(revealed) = revealed {
                            let card = previous
                                .cards
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            let current_card = self
                                .window
                                .cards()
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            let characteristics = previous
                                .evaluated
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            if card.card_id().is_none()
                                || card.zone() != ObjectZone::Hand(controller)
                                || current_card.zone() != ObjectZone::Hand(controller)
                                || !characteristics.has_type(CardType::Land)
                                || !(characteristics
                                    .current_land_subtypes
                                    .contains(&a.land_type())
                                    || characteristics
                                        .current_land_subtypes
                                        .contains(&b.land_type()))
                            {
                                return Err(EffectError::IllegalTarget);
                            }
                            self.pending_entry_reveals.push(card.clone());
                            false
                        } else {
                            true
                        }
                    }
                    EntryTap::UnlessTwoOpponents => opponents()?.len() < 2,
                    EntryTap::UnlessOpponentLandsEight => {
                        let opponents = opponents()?;
                        lands
                            .filter(|p| p.controller.is_some_and(|seat| opponents.contains(&seat)))
                            .count()
                            < 8
                    }
                    EntryTap::UnlessPlayerLifeThirteenOrLess => {
                        !previous.window.participants().iter().any(|seat| {
                            previous
                                .life
                                .get(*seat)
                                .is_some_and(|life| *life <= 13.into())
                        })
                    }
                    EntryTap::UnlessPayLife(amount) => {
                        let Some(crate::land::EntryChoice::PayLife(pay)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if pay {
                            let cost: RulesInteger = i64::from(amount).into();
                            if !cost.is_positive()
                                || self.life.get(controller).is_none_or(|total| total < &cost)
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                            self.life
                                .apply_adjustments(&BTreeMap::from([(
                                    controller,
                                    crate::life::LifeAdjustment::Lose(cost.clone()),
                                )]))
                                .map_err(|_| EffectError::InconsistentProgram)?;
                            let lost = self
                                .characteristics
                                .life_lost_this_turn
                                .entry(controller)
                                .or_insert_with(|| 0.into());
                            *lost = lost.plus(&cost);
                            false
                        } else {
                            true
                        }
                    }
                };
            }
        }
        if !tapped {
            let previous = before.view();
            let has_others_tapped_rule = previous.evaluated.objects.values().any(|perm| {
                perm.zone == ObjectZone::Battlefield
                    && perm.object != object
                    && perm.has_battlefield_rule(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::OthersEnterTapped(
                                    _
                                ),
                                ..
                            }
                        )
                    })
            });
            if has_others_tapped_rule {
                let evaluated_now = self.evaluated_characteristics()?;
                tapped = previous.evaluated.objects.values().any(|perm| {
                    if perm.zone != ObjectZone::Battlefield || perm.object == object {
                        return false;
                    }
                    let Some(perm_ctrl) = perm.controller else {
                        return false;
                    };
                    perm.printed_text
                        .continuous
                        .iter()
                        .chain(&perm.printed_text.additional_battlefield_abilities)
                        .any(|ability| {
                            let active = perm.abilities.continuous == Some(ability.id())
                                || perm
                                    .abilities
                                    .additional_battlefield_abilities
                                    .contains(&ability.id());
                            if !active {
                                return false;
                            }
                            if let crate::characteristics::StaticContinuousAbility::Restriction {
                                rule:
                                    crate::characteristics::StaticRestriction::OthersEnterTapped(
                                        selector,
                                    ),
                                ..
                            } = ability
                            {
                                crate::characteristics::selector_matches_permanent(
                                    self.window.cards(),
                                    &evaluated_now,
                                    perm.object,
                                    perm_ctrl,
                                    selector,
                                    object,
                                    &self.name_rules,
                                )
                            } else {
                                false
                            }
                        })
                });
            }
        }
        if tapped {
            self.window.enter_tapped(object)?;
        }
        Ok(())
    }

    pub(super) fn apply_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        placement: PowerToughnessCounters,
        mana_colors: u8,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_spell_entry_counters(before, object, placement, mana_colors, None)
    }
    pub(super) fn apply_return_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_entry_counter_batch(
            before,
            object,
            PowerToughnessCounters::default(),
            0,
            None,
            true,
        )
    }
    pub(super) fn apply_spell_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        placement: PowerToughnessCounters,
        mana_colors: u8,
        spell: Option<&crate::stack_window::StackEntry>,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_entry_counter_batch(before, object, placement, mana_colors, spell, false)
    }
    fn apply_entry_counter_batch(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        mut placement: PowerToughnessCounters,
        mana_colors: u8,
        spell: Option<&crate::stack_window::StackEntry>,
        return_with_counters: bool,
    ) -> Result<PowerToughnessCounters, EffectError> {
        if let Some(count) = self.prospective_devour_counters.remove(&object) {
            placement.plus_one = placement.plus_one.plus(&RulesInteger::from_usize(count));
        }
        self.apply_entry_status(object, before)?;
        let controller = self
            .current_characteristics(object)?
            .and_then(|current| current.controller)
            .ok_or(EffectError::InconsistentProgram)?;
        let extra_counters = self.copy_entry_counters.remove(&object).unwrap_or(0)
            + usize::from(return_with_counters);
        if extra_counters > 0
            && self
                .current_characteristics(object)?
                .is_some_and(|current| current.is_creature())
        {
            placement.plus_one = placement
                .plus_one
                .plus(&RulesInteger::from_usize(extra_counters));
        }
        let grant_entry = spell
            .map(crate::stack_window::StackEntry::id)
            .or_else(|| {
                self.window
                    .resolving()
                    .map(crate::stack_window::StackEntry::id)
            })
            .unwrap_or_else(StackEntryId::entry_replacement_creator);
        let previous = before.view();
        if let Some(entrant) = self.current_characteristics(object)? {
            let current_eval = self.evaluated_characteristics()?;
            for source in previous.evaluated.objects.values() {
                if source.object == object || source.zone != ObjectZone::Battlefield {
                    continue;
                }
                let Some(source_controller) = source.controller else {
                    continue;
                };
                if entrant.is_creature()
                    && source_controller == controller
                    && source.has_battlefield_rule(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::Modifier {
                                rule: crate::characteristics::StaticModifierRule::OtherCreaturesEnterWithSourceCounters,
                                ..
                            }
                        )
                    })
                {
                    placement.plus_one = placement.plus_one.plus(&source.plus_one_counters);
                }
                for ability in source.active_battlefield_rules() {
                    if let crate::characteristics::StaticContinuousAbility::Modifier {
                        rule:
                            crate::characteristics::StaticModifierRule::OthersEnterWithCounters {
                                selector,
                                kind,
                                count,
                            },
                        ..
                    } = ability
                        && crate::characteristics::selector_matches_permanent(
                            self.window.cards(),
                            &current_eval,
                            source.object,
                            source_controller,
                            selector,
                            object,
                            &self.name_rules,
                        )
                    {
                        let n: RulesInteger = i64::from(*count).into();
                        match *kind {
                            CounterKind::PlusOne => {
                                placement.plus_one = placement.plus_one.plus(&n);
                            }
                            CounterKind::MinusOne => {
                                placement.minus_one = placement.minus_one.plus(&n);
                            }
                            other => {
                                let mult =
                                    controlled_counter_multiplier(&previous.evaluated, controller);
                                let added = n.times(&mult);
                                if let Some(map) =
                                    self.characteristics.named_permanent_counters_mut(other)
                                {
                                    let entry = map.entry(object).or_insert_with(|| 0.into());
                                    *entry = entry.plus(&added);
                                }
                                self.characteristics.refresh_keyword_counter_grant(
                                    grant_entry,
                                    object,
                                    other,
                                )?;
                            }
                        }
                    }
                }
            }
            if entrant.is_creature() {
                let extra = previous
                    .evaluated
                    .objects
                    .values()
                    .filter(|source| {
                        source.zone == ObjectZone::Graveyard(controller)
                            && source.abilities.entry_counter_abilities
                    })
                    .flat_map(|source| &source.printed_text.entry_counter_abilities)
                    .filter(|ability| {
                        matches!(ability,
                            EntryCounterAbility::GraveyardControlledSubtypePlusOne(subtype)
                                if entrant.current_creature_subtypes.contains(subtype)
                        )
                    })
                    .count();
                placement.plus_one = placement.plus_one.plus(&RulesInteger::from_usize(extra));
            }
        }
        let multiplier = controlled_counter_multiplier(&previous.evaluated, controller);
        if let Some(current) = self.current_characteristics(object)?
            && let Some(last) = current.final_saga_chapter()
        {
            let number = if matches!(
                current.abilities.entry_tap,
                Some(crate::land::EntryTap::ReadAhead { .. })
            ) {
                let Some(crate::land::EntryChoice::Chapter(value)) =
                    self.entry_choices.remove(&object)
                else {
                    return Err(EffectError::SelectionRequired);
                };
                if !(1..=last).contains(&value) {
                    return Err(EffectError::IllegalTarget);
                }
                value
            } else {
                1
            };
            self.characteristics.lore.insert(
                object,
                RulesInteger::from(i64::from(number)).times(&multiplier),
            );
        }
        if self
            .current_characteristics(object)?
            .is_some_and(|view| view.has_type(CardType::Battle))
        {
            self.characteristics
                .battle_protectors
                .entry(object)
                .or_insert(controller);
            if let Some(printed) = self
                .copiable_program(object)
                .and_then(|program| program.printed_defense.as_ref())
            {
                self.characteristics
                    .defense
                    .insert(object, printed.times(&multiplier));
            }
        }
        if let Some(current) = self.current_characteristics(object)?
            && current.has_type(CardType::Planeswalker)
        {
            let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
            let extra = before
                .view()
                .evaluated
                .controlled_rules(controller, |ability| {
                    matches!(
                        ability,
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule: crate::characteristics::StaticModifierRule::ExtraEntryLoyalty,
                            ..
                        }
                    )
                });
            let loyalty = current
                .defined
                .printed_loyalty
                .unwrap_or_else(|| 0.into())
                .plus(&RulesInteger::from_usize(extra + extra_counters));
            if loyalty.is_positive() {
                self.characteristics
                    .loyalty
                    .insert(object, loyalty.times(&multiplier));
            }
        }
        let has_entry_ability = !self
            .copiable_program(object)
            .ok_or(EffectError::InconsistentProgram)?
            .entry_counter_abilities
            .is_empty();
        let mut charge = RulesInteger::from(0);
        let mut suspect = RulesInteger::from(0);
        let mut extra_loyalty = RulesInteger::from(0);
        if has_entry_ability
            && let Some(current) = self.current_characteristics(object)?
            && current.abilities.entry_counter_abilities
        {
            let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
            let total_damage = self
                .damage_to_players_this_turn
                .iter()
                .flat_map(|map| map.iter())
                .filter(|(player, _)| **player != controller)
                .fold(RulesInteger::from(0), |total, (_, damage)| {
                    total.plus(damage)
                });
            for ability in &current.printed_text.entry_counter_abilities {
                match ability {
                    EntryCounterAbility::KickedOpposingNonbasicLands(index) => {
                        if spell
                            .and_then(|entry| entry.kicker_choices().get(index))
                            .is_some_and(RulesInteger::is_positive)
                        {
                            let opponents = self
                                .combat_rules
                                .opponents(previous.window.participants(), controller)?;
                            let count = previous
                                .evaluated
                                .objects
                                .values()
                                .filter(|object| {
                                    object.zone == ObjectZone::Battlefield
                                        && object.has_type(CardType::Land)
                                        && !object
                                            .current_supertypes
                                            .contains(&crate::program::Supertype::Basic)
                                        && object
                                            .controller
                                            .is_some_and(|seat| opponents.contains(&seat))
                                })
                                .count();
                            placement.plus_one =
                                placement.plus_one.plus(&RulesInteger::from_usize(count));
                        }
                    }
                    EntryCounterAbility::ChargePerKicker(index) => {
                        if let Some(times) =
                            spell.and_then(|entry| entry.kicker_choices().get(index))
                        {
                            charge = charge.plus(times);
                        }
                    }
                    EntryCounterAbility::Devour(_) => {} // Counters bound to the completed sacrifices are staged before movement.
                    EntryCounterAbility::ChosenXPlusOne => {
                        if let Some(x) = spell.and_then(|entry| entry.chosen_x()) {
                            placement.plus_one = placement.plus_one.plus(x);
                        }
                    }
                    EntryCounterAbility::SuspectForGreatestCreatureCount => {
                        let mut counts = BTreeMap::<SeatId, usize>::new();
                        for permanent in before
                            .view()
                            .evaluated
                            .objects
                            .values()
                            .filter(|o| o.zone == ObjectZone::Battlefield && o.is_creature())
                        {
                            if let Some(player) = permanent.controller {
                                *counts.entry(player).or_default() += 1;
                            }
                        }
                        suspect = suspect.plus(&RulesInteger::from_usize(
                            counts.values().copied().max().unwrap_or(0),
                        ));
                    }
                    EntryCounterAbility::FixedCharge(n) => charge = charge.plus(n),
                    EntryCounterAbility::FixedLoyalty(n) => extra_loyalty = extra_loyalty.plus(n),
                    EntryCounterAbility::FixedNamed { kind, amount } => {
                        if amount.is_positive() {
                            let added = amount.times(&multiplier);
                            if let Some(map) =
                                self.characteristics.named_permanent_counters_mut(*kind)
                            {
                                let entry = map.entry(object).or_insert_with(|| 0.into());
                                *entry = entry.plus(&added);
                            }
                            self.characteristics.refresh_keyword_counter_grant(
                                grant_entry,
                                object,
                                *kind,
                            )?;
                        }
                    }
                    EntryCounterAbility::FixedPlusOne(n) => {
                        placement.plus_one = placement.plus_one.plus(n)
                    }
                    EntryCounterAbility::FixedMinusOne(n) => {
                        placement.minus_one = placement.minus_one.plus(n)
                    }
                    EntryCounterAbility::DynamicCounter { kind, amount }
                    | EntryCounterAbility::ConditionalCounter { kind, amount, .. } => {
                        if let EntryCounterAbility::ConditionalCounter { condition, .. } = ability {
                            let left =
                                self.resolve_value_expr_at(controller, &condition.left, None)?;
                            let right =
                                self.resolve_value_expr_at(controller, &condition.right, None)?;
                            if !condition.comparison.evaluate(&left, &right) {
                                continue;
                            }
                        }
                        let resolved = if amount.is_chosen_x() {
                            spell
                                .and_then(|entry| entry.chosen_x().cloned())
                                .unwrap_or_else(|| 0.into())
                        } else {
                            self.resolve_value_expr_at(controller, amount, None)?
                        };
                        if resolved.is_positive() {
                            match *kind {
                                CounterKind::PlusOne => {
                                    placement.plus_one = placement.plus_one.plus(&resolved);
                                }
                                CounterKind::MinusOne => {
                                    placement.minus_one = placement.minus_one.plus(&resolved);
                                }
                                CounterKind::Charge => {
                                    charge = charge.plus(&resolved);
                                }
                                CounterKind::Loyalty => {
                                    extra_loyalty = extra_loyalty.plus(&resolved);
                                }
                                other => {
                                    let added = resolved.times(&multiplier);
                                    if let Some(map) =
                                        self.characteristics.named_permanent_counters_mut(other)
                                    {
                                        let entry = map.entry(object).or_insert_with(|| 0.into());
                                        *entry = entry.plus(&added);
                                    }
                                    self.characteristics.refresh_keyword_counter_grant(
                                        grant_entry,
                                        object,
                                        other,
                                    )?;
                                }
                            }
                        }
                    }
                    EntryCounterAbility::Bloodthirst(n) if total_damage.is_positive() => {
                        placement.plus_one = placement.plus_one.plus(n)
                    }
                    EntryCounterAbility::Bloodthirst(_) => {}
                    EntryCounterAbility::MorbidPlusOne(n) => {
                        if self.creatures_died_this_turn.is_positive() {
                            placement.plus_one = placement.plus_one.plus(n);
                        }
                    }
                    EntryCounterAbility::GraveyardControlledSubtypePlusOne(_) => {}
                    EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(subtype) => {
                        let count = previous
                            .evaluated
                            .objects
                            .values()
                            .filter(|object| {
                                object.current_creature_subtypes.contains(subtype)
                                    && (object.zone == ObjectZone::Graveyard(controller)
                                        || object.zone == ObjectZone::Battlefield
                                            && object.controller == Some(controller))
                            })
                            .count();
                        placement.plus_one =
                            placement.plus_one.plus(&RulesInteger::from_usize(count));
                    }
                    EntryCounterAbility::BloodthirstX => {
                        placement.plus_one = placement.plus_one.plus(&total_damage)
                    }
                    EntryCounterAbility::Sunburst => {
                        let color_count = i64::from(mana_colors).into();
                        if current.has_type(CardType::Creature) {
                            placement.plus_one = placement.plus_one.plus(&color_count);
                        } else {
                            charge = charge.plus(&color_count);
                        }
                    }
                }
            }
        }
        self.apply_fading_vanishing_entry(object, &multiplier)?;
        if placement.valid_placement() {
            let entering = self
                .current_characteristics(object)?
                .ok_or(EffectError::InconsistentProgram)?;
            placement =
                self.adjusted_pt_counters(&previous.evaluated, object, placement, Some(&entering))?;
            self.characteristics.place_counters(object, &placement)?;
        }
        if suspect.is_positive() {
            self.characteristics
                .suspect_counters
                .insert(object, suspect.times(&multiplier));
        }
        if charge.is_positive() {
            self.characteristics
                .charge
                .insert(object, charge.times(&multiplier));
        }
        if extra_loyalty.is_positive() {
            let loyalty = self
                .characteristics
                .loyalty
                .entry(object)
                .or_insert_with(|| 0.into());
            *loyalty = loyalty.plus(&extra_loyalty.times(&multiplier));
        }
        Ok(placement)
    }
}
