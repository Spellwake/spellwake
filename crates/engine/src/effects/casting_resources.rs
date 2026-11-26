//! Lock the total cost before paying it. These mechanics pay individual mana
//! symbols and do not change the spell's mana value (CR 601.2f–h).
use super::*;
use crate::{
    characteristics::{Color, ObjectCharacteristics},
    mana::CastResource,
    object_identity::{ObjectMovement, ObjectStatusChange},
    occurrence::{EventCause, RulesOccurrence, StatusReason, ZoneReason},
};

impl EffectWindow {
    /// Reveal-cost faces stay visible while the original spell remains on the stack.
    pub(crate) fn revealed_cast_cost_cards(&self) -> impl Iterator<Item = &GameObject> {
        self.window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .filter_map(|entry| {
                if !matches!(entry.kind(), StackKind::CardSpell(_)) {
                    return None;
                }
                let StackPayload::Instructions(payload) = self.payloads.get(&entry.id())? else {
                    return None;
                };
                let card = self
                    .window
                    .cards()
                    .get(payload.revealed_cost_card.as_ref()?.object)?;
                matches!(card.zone(), ObjectZone::Hand(_)).then_some(card)
            })
    }

    pub(super) fn revealed_exile_subjects(
        &self,
        instruction: &Instruction,
    ) -> Result<BTreeSet<ObjectId>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
            return Err(EffectError::InconsistentProgram);
        };
        let cost = payload
            .revealed_cost_card
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        let current = self.evaluated_characteristics()?;
        match instruction {
            Instruction::ExileCreatureAtMostRevealedPower => {
                let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let power = current
                    .get(cost.object)
                    .and_then(|o| o.power_toughness.as_ref())
                    .map_or(&cost.last_power, |stats| &stats.derived.power);
                Ok(current
                    .get(id)
                    .filter(|o| {
                        o.is_creature()
                            && o.power_toughness
                                .as_ref()
                                .is_some_and(|stats| stats.derived.power <= *power)
                    })
                    .map(|o| o.object)
                    .into_iter()
                    .collect())
            }
            Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue => Ok(current
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && (o.is_creature() || o.has_type(CardType::Planeswalker))
                        && self
                            .permanent_mana_value(o.object)
                            .is_some_and(|mv| mv < cost.mana_value)
                })
                .map(|o| o.object)
                .collect()),
            _ => Err(EffectError::InconsistentProgram),
        }
    }

    pub(crate) fn pending_cast_cost_decision(&self) -> Option<EffectDecision> {
        let work = self.cast_cost_replacement.as_ref()?;
        if let Some(frame) = work.batch.frame(self.revision) {
            return Some(EffectDecision::ZoneReplacement(frame));
        }
        for player in self.window.apnap_players().ok()? {
            let objects =
                work.objects
                    .iter()
                    .copied()
                    .filter(|id| {
                        work.batch.destination(*id).ok()
                            == Some(crate::program::PublicZoneDestination::Graveyard)
                            && self.window.cards().get(*id).is_some_and(|card| {
                                card.card_id().is_some() && card.owner() == player
                            })
                    })
                    .collect::<Vec<_>>();
            if objects.len() > 1 && !work.graveyard_orders.contains_key(&player) {
                return Some(EffectDecision::LinkedReturnOrder(LinkedReturnOrderFrame {
                    revision: self.revision,
                    chooser: player,
                    objects,
                    destination: crate::program::PublicZoneDestination::Graveyard,
                }));
            }
        }
        None
    }
    fn cast_cost_zone_batch(
        &self,
        entry: StackEntryId,
        resource: CastResource,
        objects: &[ObjectId],
    ) -> Result<zone_replacement::AuthorizedZoneBatch, EffectError> {
        let (action, reason) = if matches!(
            resource,
            CastResource::ExileCreatureForReduction(_)
                | CastResource::ExileGraveyardCreatureForSpell(_)
                | CastResource::ExileGraveyardCardForSpell(_)
        ) {
            (
                crate::program::ZoneActionKind::ExileCost,
                ZoneReason::ExileCost,
            )
        } else if matches!(
            resource,
            CastResource::DiscardForSpell(_)
                | CastResource::Retrace(_)
                | CastResource::JumpStart(_)
        ) {
            let actor = self
                .window
                .entries()
                .iter()
                .find(|e| e.id() == entry)
                .ok_or(EffectError::InconsistentProgram)?
                .controller();
            (
                crate::program::ZoneActionKind::DiscardCost,
                ZoneReason::Discard { player: actor },
            )
        } else {
            (
                crate::program::ZoneActionKind::SacrificeCost,
                ZoneReason::SacrificeCost,
            )
        };
        Ok(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::CastObjectPayment { entry, resource },
            action,
            intents: objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        })
    }
    fn begin_cast_cost_replacement(
        &mut self,
        entry: StackEntryId,
        resource: CastResource,
        objects: &[ObjectId],
    ) -> Result<bool, EffectError> {
        if objects
            .iter()
            .all(|id| self.settled_cast_cost_destinations.contains_key(id))
        {
            return Ok(false);
        }
        let mut batch =
            self.begin_zone_replacements(self.cast_cost_zone_batch(entry, resource, objects)?)?;
        batch.progress(self.revision)?;
        self.cast_cost_replacement = Some(CastCostReplacementWork {
            batch,
            objects: objects.to_vec(),
            graveyard_orders: BTreeMap::new(),
        });
        if self.pending_cast_cost_decision().is_some() {
            return Ok(true);
        }
        self.finish_cast_cost_replacement()?;
        Ok(false)
    }
    pub(super) fn finish_cast_cost_replacement(&mut self) -> Result<(), EffectError> {
        let work = self
            .cast_cost_replacement
            .take()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let EventCause::CastObjectPayment { entry, resource } = work.batch.cause() else {
            return Err(EffectError::InconsistentProgram);
        };
        let fresh = self.begin_zone_replacements(self.cast_cost_zone_batch(
            *entry,
            *resource,
            &work.objects,
        )?)?;
        work.batch.verify_initial(fresh)?;
        for id in &work.objects {
            if work.batch.used_madness(*id) {
                self.settled_cast_cost_madness.insert(*id);
            }
            self.settled_cast_cost_destinations
                .insert(*id, work.batch.destination(*id)?);
        }
        self.settled_cast_cost_order.clear();
        for player in self.window.apnap_players()? {
            if let Some(order) = work.graveyard_orders.get(&player) {
                self.settled_cast_cost_order
                    .extend(order.iter().rev().copied());
            } else {
                self.settled_cast_cost_order
                    .extend(work.objects.iter().copied().filter(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|card| card.owner() == player)
                    }));
            }
        }
        Ok(())
    }
    pub(super) fn counted_cast_discount(
        &self,
        count: crate::program::CastCostCount,
        spell: ObjectId,
        actor: SeatId,
        current: &EvaluatedCharacteristics,
    ) -> RulesInteger {
        use crate::program::CastCostCount;
        let total = current
            .objects
            .values()
            .filter(|o| {
                if o.object == spell {
                    // CR 601.2a moves this spell to the stack before its cost is determined.
                    return false;
                }
                match count {
                    CastCostCount::BattlefieldCreatures => {
                        o.zone == ObjectZone::Battlefield && o.is_creature()
                    }
                    CastCostCount::AttackingCreatures => {
                        o.zone == ObjectZone::Battlefield
                            && o.is_creature()
                            && self
                                .combat
                                .as_ref()
                                .is_some_and(|combat| combat.attacker(o.object).is_some())
                    }
                    CastCostCount::ControlledCreaturesWithPlusOneCounters => {
                        o.zone == ObjectZone::Battlefield
                            && o.controller == Some(actor)
                            && o.is_creature()
                            && self
                                .characteristics
                                .counters(o.object)
                                .plus_one
                                .is_positive()
                    }
                    CastCostCount::OwnGraveyardCreatures
                    | CastCostCount::OwnGraveyardInstantsOrSorceries => {
                        o.zone == ObjectZone::Graveyard(actor)
                            && self
                                .window
                                .cards()
                                .get(o.object)
                                .is_some_and(|card| card.card_id().is_some())
                            && match count {
                                CastCostCount::OwnGraveyardCreatures => o.is_creature(),
                                _ => o.has_type(CardType::Instant) || o.has_type(CardType::Sorcery),
                            }
                    }
                }
            })
            .count();
        RulesInteger::from_usize(total)
    }

    pub(super) fn battlefield_cost_value(
        &self,
        value: crate::program::CastCostValue,
        caster: SeatId,
        view: &EvaluatedCharacteristics,
    ) -> Result<RulesInteger, EffectError> {
        use crate::program::CastCostValue;
        let mut result = RulesInteger::from(0);
        for object in view.objects.values().filter(|object| {
            object.zone == ObjectZone::Battlefield && object.controller == Some(caster)
        }) {
            match value {
                CastCostValue::TotalCreaturePower | CastCostValue::GreatestCreaturePower
                    if object.is_creature() =>
                {
                    let power = &object
                        .power_toughness
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?
                        .derived
                        .power;
                    result = if value == CastCostValue::TotalCreaturePower {
                        result.plus(power)
                    } else {
                        result.max(power.clone())
                    };
                }
                CastCostValue::NoncreatureArtifactManaValue
                | CastCostValue::NoncreatureEnchantmentManaValue
                    if !object.is_creature()
                        && object.has_type(
                            if value == CastCostValue::NoncreatureArtifactManaValue {
                                CardType::Artifact
                            } else {
                                CardType::Enchantment
                            },
                        ) =>
                {
                    let amount = self
                        .permanent_mana_value(object.object)
                        .ok_or(EffectError::InconsistentProgram)?;
                    result = result.plus(&amount);
                }
                _ => {}
            }
        }
        // A negative total power cannot increase a cost.
        Ok(result.max(0.into()))
    }

    pub(super) fn allocate_cast_resources(
        &self,
        cost: &mut ManaCost,
        resources: &[CastResource],
        evaluated: &EvaluatedCharacteristics,
        spell: &ObjectCharacteristics,
        actor: SeatId,
    ) -> Result<(), EffectError> {
        let mut used = BTreeSet::new();
        let mut bargained = false;
        for &resource in resources {
            let id = resource.object();
            if id == spell.object || !used.insert(id) {
                return Err(EffectError::InvalidCastResource);
            }
            let physical = self
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::InvalidCastResource)?;
            let subject = evaluated.get(id).ok_or(EffectError::InvalidCastResource)?;
            if matches!(
                resource,
                CastResource::Bargain(_)
                    | CastResource::Emerge(_)
                    | CastResource::SacrificeForSpell(_)
                    | CastResource::FlashbackSacrifice(_)
            ) && evaluated.sacrifice_prohibited(id)
            {
                return Err(EffectError::InvalidCastResource);
            }
            let untapped = subject.zone == ObjectZone::Battlefield
                && subject.controller == Some(actor)
                && physical.tapped() == Some(false);
            let symbol = match resource {
                CastResource::SacrificeForSpell(_) => {
                    let matches_target =
                        if let Some(filter) = spell.printed_text.sacrifice_cast_cost_filter {
                            let excluded = BTreeSet::new();
                            self.matches_object_filter(
                                filter, subject, actor, None, &excluded, evaluated,
                            )
                        } else {
                            subject.is_creature()
                        };
                    if subject.zone != ObjectZone::Battlefield
                        || subject.controller != Some(actor)
                        || !matches_target
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::FlashbackSacrifice(_) => {
                    if subject.zone != ObjectZone::Battlefield
                        || subject.controller != Some(actor)
                        || !subject.is_creature()
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::RevealColorlessCreature(_) => {
                    if subject.zone != ObjectZone::Hand(actor)
                        || physical.card_id().is_none()
                        || !subject.is_creature()
                        || !subject.current_colors.is_empty()
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::ExileHandForAlternative(_) => {
                    let Some(crate::program::ConditionalCastCost::ExileHandCards { color, .. }) =
                        &spell.printed_text.conditional_cost
                    else {
                        return Err(EffectError::InvalidCastResource);
                    };
                    if physical.zone() != ObjectZone::Hand(actor)
                        || physical.card_id().is_none()
                        || !subject.current_colors.contains(color)
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::TapCreatureForSpell(_) => {
                    if !spell.printed_text.tap_creatures_cast_cost
                        || !untapped
                        || !subject.is_creature()
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::TapForFlashback(_) => {
                    if !untapped
                        || !subject.is_creature()
                        || !subject.current_colors.contains(&Color::White)
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::TapForAlternative(_) => {
                    let Some(crate::program::ConditionalCastCost::TapPermanents {
                        quality, ..
                    }) = &spell.printed_text.conditional_cost
                    else {
                        return Err(EffectError::InvalidCastResource);
                    };
                    if !untapped
                        || !match quality {
                            crate::program::CastTapQuality::Artifact => {
                                subject.has_type(CardType::Artifact)
                            }
                            crate::program::CastTapQuality::FlyingCreature => {
                                subject.is_creature() && subject.has_keyword(KeywordAbility::Flying)
                            }
                        }
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::WebSlinging(_) | CastResource::Sneak(_) => continue, // The announcement already checked the returned creature for this price.
                CastResource::Emerge(_) => continue, // Chosen and priced when the total cost was locked.
                CastResource::Bargain(_) => {
                    if bargained
                        || !(spell.has_keyword(KeywordAbility::Bargain)
                            || self.spell_has_granted_keyword(
                                evaluated,
                                spell,
                                actor,
                                KeywordAbility::Bargain,
                            ))
                        || subject.zone != ObjectZone::Battlefield
                        || subject.controller != Some(actor)
                        || !(physical.is_token()
                            || subject.has_type(CardType::Artifact)
                            || subject.has_type(CardType::Enchantment))
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    bargained = true;
                    continue;
                }
                CastResource::DiscardForSpell(_)
                | CastResource::Retrace(_)
                | CastResource::JumpStart(_) => {
                    let legal = match resource {
                        CastResource::DiscardForSpell(_) => {
                            spell.printed_text.discard_cast_cost
                                && spell.printed_text.discard_cast_cost_filter.is_none_or(|f| {
                                    let excluded = BTreeSet::new();
                                    f.matches_static_characteristics(subject, &excluded, || {
                                        self.copiable_program(id).map(Program::mana_value)
                                    })
                                })
                        }
                        CastResource::Retrace(_) => {
                            (spell.has_keyword(KeywordAbility::Retrace)
                                || self.spell_has_granted_keyword(
                                    evaluated,
                                    spell,
                                    actor,
                                    KeywordAbility::Retrace,
                                ))
                                && subject.has_type(CardType::Land)
                        }
                        _ => {
                            (spell.has_keyword(KeywordAbility::JumpStart)
                                || self.spell_has_granted_keyword(
                                    evaluated,
                                    spell,
                                    actor,
                                    KeywordAbility::JumpStart,
                                ))
                                && (spell.has_type(CardType::Instant)
                                    || spell.has_type(CardType::Sorcery))
                        }
                    };
                    if !legal
                        || physical.zone() != ObjectZone::Hand(actor)
                        || physical.card_id().is_none()
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    continue;
                }
                CastResource::Escape(_)
                    if physical.zone() == ObjectZone::Graveyard(actor)
                        && physical.card_id().is_some() =>
                {
                    continue;
                }
                CastResource::ExileCreatureForReduction(_)
                    if spell.printed_text.cast_reductions.iter().any(|r| {
                        matches!(
                            r,
                            crate::program::CastReduction::ExileGraveyardCreatures { .. }
                        )
                    }) && physical.zone() == ObjectZone::Graveyard(actor)
                        && physical.card_id().is_some()
                        && subject.is_creature() =>
                {
                    continue;
                }
                CastResource::ExileGraveyardCardForSpell(_)
                    if spell.printed_text.graveyard_exile_x_cost
                        && physical.zone() == ObjectZone::Graveyard(actor)
                        && physical.card_id().is_some() =>
                {
                    continue;
                }
                CastResource::ExileGraveyardCreatureForSpell(_)
                    if spell.printed_text.graveyard_creature_exile_cost > 0
                        && physical.zone() == ObjectZone::Graveyard(actor)
                        && physical.card_id().is_some()
                        && subject.is_creature() =>
                {
                    continue;
                }
                CastResource::ExtraCost(_)
                    if !spell.printed_text.extra_cast_costs.is_empty()
                        || matches!(
                            spell.printed_text.graveyard_self_cast,
                            Some(
                                crate::program::GraveyardSelfCast::WithExtraCost(_)
                                    | crate::program::GraveyardSelfCast::WithAlternativeCost { .. }
                            )
                        )
                        || spell.printed_text.additional_cast_costs.iter().any(|cost| {
                            matches!(
                                cost,
                                crate::program::AdditionalCastCost::KickerExtra { extra, .. }
                                    if !extra.is_empty()
                            )
                        })
                        || matches!(
                            &spell.printed_text.conditional_cost,
                            Some(crate::program::ConditionalCastCost::Alternative { extra, .. })
                                if !extra.is_empty()
                        ) =>
                {
                    continue;
                }
                CastResource::Delve(_)
                    if (spell.has_keyword(KeywordAbility::Delve)
                        || self.spell_has_granted_keyword(
                            evaluated,
                            spell,
                            actor,
                            KeywordAbility::Delve,
                        ))
                        && physical.card_id().is_some()
                        && physical.zone() == ObjectZone::Graveyard(actor) =>
                {
                    &mut cost.generic
                }
                CastResource::Improvise(_)
                    if (spell.has_keyword(KeywordAbility::Improvise)
                        || (!spell.has_type(CardType::Artifact)
                            && evaluated.grants_improvise(actor))
                        || self.spell_has_granted_keyword(
                            evaluated,
                            spell,
                            actor,
                            KeywordAbility::Improvise,
                        ))
                        && untapped
                        && subject.has_type(CardType::Artifact) =>
                {
                    &mut cost.generic
                }
                CastResource::Convoke { color, .. }
                    if (spell.has_keyword(KeywordAbility::Convoke)
                        || self.spell_has_granted_keyword(
                            evaluated,
                            spell,
                            actor,
                            KeywordAbility::Convoke,
                        ))
                        && untapped
                        && subject.is_creature() =>
                {
                    if let Some(color) = color {
                        if !subject.current_colors.contains(&color) {
                            return Err(EffectError::InvalidCastResource);
                        }
                        let mana = match color {
                            Color::White => ManaType::White,
                            Color::Blue => ManaType::Blue,
                            Color::Black => ManaType::Black,
                            Color::Red => ManaType::Red,
                            Color::Green => ManaType::Green,
                        };
                        cost.required
                            .get_mut(&mana)
                            .ok_or(EffectError::InvalidCastResource)?
                    } else {
                        &mut cost.generic
                    }
                }
                _ => return Err(EffectError::InvalidCastResource),
            };
            if !symbol.is_positive() {
                return Err(EffectError::InvalidCastResource);
            }
            *symbol = symbol.minus(&1.into());
        }
        Ok(())
    }

    pub(super) fn pay_cast_resources(
        &mut self,
        resources: &[CastResource],
        entry: StackEntryId,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<Vec<CastResource>, EffectError> {
        // One printed tap cost creates one event involving all of the chosen permanents.
        let selected: Vec<_> = resources
            .iter()
            .copied()
            .filter(|resource| {
                matches!(
                    resource,
                    CastResource::TapForAlternative(_)
                        | CastResource::TapForFlashback(_)
                        | CastResource::TapCreatureForSpell(_)
                )
            })
            .collect();
        let remaining;
        let resources = if let Some(first) = selected.first() {
            let before = self.rules_snapshot(turn)?;
            let ids = selected
                .iter()
                .map(|resource| resource.object())
                .collect::<Vec<_>>();
            let (window, changes) = self.window.tap_for_cast_cost(entry, &ids)?;
            self.window = window;
            let paid_creatures = selected
                .iter()
                .filter(|cost| matches!(cost, CastResource::TapCreatureForSpell(_)))
                .count();
            if paid_creatures > 0 {
                let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                payload.paid_creature_taps = paid_creatures;
            }
            self.emit_from(
                EventCause::CastObjectPayment {
                    entry,
                    resource: *first,
                },
                &before,
                turn,
                append,
                |_, _, _| {
                    Ok(occurrence_flow::status_occurrences(
                        changes,
                        StatusReason::CastObjectCost,
                    ))
                },
            )?;
            remaining = resources
                .iter()
                .copied()
                .filter(|resource| {
                    !matches!(
                        resource,
                        CastResource::TapForAlternative(_)
                            | CastResource::TapForFlashback(_)
                            | CastResource::TapCreatureForSpell(_)
                    )
                })
                .collect::<Vec<_>>();
            remaining.as_slice()
        } else {
            resources
        };
        // All exiles making up one printed escape cost occur together.
        let mut escape_paid = false;
        let mut flashback_sacrifice_paid = false;
        let mut hand_exiles_paid = false;
        for (index, &resource) in resources.iter().enumerate() {
            if matches!(resource, CastResource::ExtraCost(_)) {
                continue;
            }
            if matches!(resource, CastResource::ExileHandForAlternative(_)) {
                if !hand_exiles_paid {
                    let before = self.rules_snapshot(turn)?;
                    let mut moves = Vec::new();
                    for &selected in resources
                        .iter()
                        .filter(|r| matches!(r, CastResource::ExileHandForAlternative(_)))
                    {
                        let (window, original, after) =
                            self.window.pay_cast_resource(entry, selected, false)?;
                        self.window = window;
                        moves.push(ObjectMovement {
                            before: original,
                            after,
                        });
                    }
                    self.emit_from(
                        EventCause::CastObjectPayment { entry, resource },
                        &before,
                        turn,
                        append,
                        |previous, current, controllers| {
                            moves
                                .iter()
                                .map(|movement| {
                                    zone_occurrence_at(
                                        previous,
                                        current,
                                        movement,
                                        ZoneReason::AlternativeHandExileCost,
                                        controllers,
                                    )
                                })
                                .collect()
                        },
                    )?;
                    hand_exiles_paid = true;
                }
                continue;
            }
            if let CastResource::RevealColorlessCreature(id) = resource {
                let before = self.rules_snapshot(turn)?;
                let card = self
                    .window
                    .cards()
                    .get(id)
                    .cloned()
                    .ok_or(EffectError::InvalidCastResource)?;
                let last_power = before
                    .view()
                    .evaluated
                    .get(id)
                    .and_then(|o| o.power_toughness.as_ref())
                    .ok_or(EffectError::InvalidCastResource)?
                    .derived
                    .power
                    .clone();
                let mana_value = self
                    .copiable_program(id)
                    .ok_or(EffectError::InvalidCastResource)?
                    .mana_value();
                let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                payload.revealed_cost_card = Some(RevealedCostCard {
                    object: id,
                    last_power,
                    mana_value,
                });
                self.emit_from(
                    EventCause::CastObjectPayment { entry, resource },
                    &before,
                    turn,
                    append,
                    |_, _, _| Ok(vec![RulesOccurrence::HandCardRevealed { object: card }]),
                )?;
                continue;
            }
            if matches!(resource, CastResource::FlashbackSacrifice(_)) {
                if !flashback_sacrifice_paid {
                    let objects = resources
                        .iter()
                        .filter_map(|r| {
                            if let CastResource::FlashbackSacrifice(id) = r {
                                Some(*id)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    if self.begin_cast_cost_replacement(entry, resource, &objects)? {
                        return Ok(Self::preserve_earlier_extra_costs(
                            &resources[..index],
                            resources[index..]
                                .iter()
                                .copied()
                                .filter(|r| {
                                    !hand_exiles_paid
                                        || !matches!(r, CastResource::ExileHandForAlternative(_))
                                })
                                .filter(|r| !escape_paid || !matches!(r, CastResource::Escape(_)))
                                .collect(),
                        ));
                    }
                    let before = self.rules_snapshot(turn)?;
                    let mut moves = Vec::new();
                    let mut sacrifices = resources
                        .iter()
                        .copied()
                        .filter(|r| matches!(r, CastResource::FlashbackSacrifice(_)))
                        .collect::<Vec<_>>();
                    sacrifices.sort_by_key(|resource| {
                        self.settled_cast_cost_order
                            .iter()
                            .position(|id| *id == resource.object())
                            .unwrap_or(self.settled_cast_cost_order.len())
                    });
                    for card in sacrifices {
                        let destination = self
                            .settled_cast_cost_destinations
                            .remove(&card.object())
                            .ok_or(EffectError::InconsistentProgram)?
                            .physical(None)
                            .ok_or(EffectError::InconsistentProgram)?;
                        let (window, original, after) = self.window.pay_cast_resource_replaced(
                            entry,
                            card,
                            false,
                            Some(destination),
                        )?;
                        self.window = window;
                        moves.push(ObjectMovement {
                            before: original,
                            after,
                        });
                    }
                    self.settled_cast_cost_order.clear();
                    self.emit_from(
                        EventCause::CastObjectPayment { entry, resource },
                        &before,
                        turn,
                        append,
                        |previous, current, controllers| {
                            moves
                                .iter()
                                .map(|m| {
                                    zone_occurrence_at(
                                        previous,
                                        current,
                                        m,
                                        ZoneReason::SacrificeCost,
                                        controllers,
                                    )
                                })
                                .collect()
                        },
                    )?;
                    flashback_sacrifice_paid = true;
                    if self.linked_return_work.is_some() {
                        return Ok(Self::preserve_earlier_extra_costs(
                            &resources[..index],
                            resources[index + 1..]
                                .iter()
                                .copied()
                                .filter(|r| {
                                    !hand_exiles_paid
                                        || !matches!(r, CastResource::ExileHandForAlternative(_))
                                })
                                .filter(|r| {
                                    !matches!(r, CastResource::FlashbackSacrifice(_))
                                        && (!escape_paid || !matches!(r, CastResource::Escape(_)))
                                })
                                .collect(),
                        ));
                    }
                }
                continue;
            }
            if matches!(
                resource,
                CastResource::ExileCreatureForReduction(_)
                    | CastResource::ExileGraveyardCreatureForSpell(_)
                    | CastResource::ExileGraveyardCardForSpell(_)
            ) {
                let still_unpaid = |other: &CastResource| {
                    !(hand_exiles_paid && matches!(other, CastResource::ExileHandForAlternative(_))
                        || escape_paid && matches!(other, CastResource::Escape(_))
                        || flashback_sacrifice_paid
                            && matches!(other, CastResource::FlashbackSacrifice(_)))
                };
                let same_cost = |candidate: &CastResource| {
                    std::mem::discriminant(candidate) == std::mem::discriminant(&resource)
                };
                let selected = resources[index..]
                    .iter()
                    .copied()
                    .filter(same_cost)
                    .collect::<Vec<_>>();
                let ids = selected.iter().map(|r| r.object()).collect::<Vec<_>>();
                if self.begin_cast_cost_replacement(entry, resource, &ids)? {
                    return Ok(Self::preserve_earlier_extra_costs(
                        &resources[..index],
                        resources[index..]
                            .iter()
                            .copied()
                            .filter(still_unpaid)
                            .collect(),
                    ));
                }
                let before = self.rules_snapshot(turn)?;
                let Some(StackKind::CardSpell(spell)) = self
                    .window
                    .entries()
                    .iter()
                    .find(|s| s.id() == entry)
                    .map(|s| s.kind())
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let spell = *spell;
                let mut moves = Vec::new();
                for cost in selected {
                    let destination = self
                        .settled_cast_cost_destinations
                        .remove(&cost.object())
                        .ok_or(EffectError::InconsistentProgram)?
                        .physical(None)
                        .ok_or(EffectError::InconsistentProgram)?;
                    if destination == ObjectDestination::Graveyard {
                        continue;
                    }
                    let (window, original, after) = self.window.pay_cast_resource_replaced(
                        entry,
                        cost,
                        false,
                        Some(destination),
                    )?;
                    self.window = window;
                    if matches!(cost, CastResource::ExileCreatureForReduction(_))
                        && after.zone() == ObjectZone::Exile
                    {
                        self.linked_search_results
                            .entry(spell)
                            .or_default()
                            .push(after.id());
                    }
                    moves.push(ObjectMovement {
                        before: original,
                        after,
                    });
                }
                self.settled_cast_cost_order.clear();
                if matches!(
                    resource,
                    CastResource::ExileGraveyardCreatureForSpell(_)
                        | CastResource::ExileGraveyardCardForSpell(_)
                ) && let [movement] = moves.as_slice()
                    && movement.after.zone() == ObjectZone::Exile
                {
                    let object = movement.after.id();
                    let power = self
                        .current_characteristics(object)?
                        .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                        .unwrap_or_else(|| 0.into());
                    if let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
                    {
                        payload.exiled_cost_creature = Some((object, power));
                    }
                }
                if !moves.is_empty() {
                    self.emit_from(
                        EventCause::CastObjectPayment { entry, resource },
                        &before,
                        turn,
                        append,
                        |old, current, controllers| {
                            moves
                                .iter()
                                .map(|movement| {
                                    zone_occurrence_at(
                                        old,
                                        current,
                                        movement,
                                        ZoneReason::ExileCost,
                                        controllers,
                                    )
                                })
                                .collect()
                        },
                    )?;
                }
                let remaining = Self::preserve_earlier_extra_costs(
                    &resources[..index],
                    resources[index + 1..]
                        .iter()
                        .copied()
                        .filter(still_unpaid)
                        .filter(|resource| !same_cost(resource))
                        .collect::<Vec<_>>(),
                );
                return if self.linked_return_work.is_some() {
                    Ok(remaining)
                } else {
                    self.pay_cast_resources(&remaining, entry, turn, append)
                };
            }
            if matches!(resource, CastResource::Escape(_)) {
                if !escape_paid {
                    let before = self.rules_snapshot(turn)?;
                    let mut moves = Vec::new();
                    for &card in resources
                        .iter()
                        .filter(|r| matches!(r, CastResource::Escape(_)))
                    {
                        let (window, original, after) =
                            self.window.pay_cast_resource(entry, card, false)?;
                        self.window = window;
                        moves.push(ObjectMovement {
                            before: original,
                            after,
                        });
                    }
                    self.emit_from(
                        EventCause::EscapePayment { entry },
                        &before,
                        turn,
                        append,
                        |previous, current, controllers| {
                            moves
                                .iter()
                                .map(|m| {
                                    zone_occurrence_at(
                                        previous,
                                        current,
                                        m,
                                        ZoneReason::EscapeCost,
                                        controllers,
                                    )
                                })
                                .collect()
                        },
                    )?;
                    escape_paid = true;
                }
                continue;
            }

            let before = self.rules_snapshot(turn)?;
            if matches!(
                resource,
                CastResource::DiscardForSpell(_)
                    | CastResource::Retrace(_)
                    | CastResource::JumpStart(_)
                    | CastResource::Emerge(_)
                    | CastResource::Bargain(_)
                    | CastResource::SacrificeForSpell(_)
            ) && self.begin_cast_cost_replacement(entry, resource, &[resource.object()])?
            {
                return Ok(Self::preserve_earlier_extra_costs(
                    &resources[..index],
                    resources[index..]
                        .iter()
                        .copied()
                        .filter(|r| {
                            !hand_exiles_paid
                                || !matches!(r, CastResource::ExileHandForAlternative(_))
                        })
                        .filter(|r| {
                            (!escape_paid || !matches!(r, CastResource::Escape(_)))
                                && (!flashback_sacrifice_paid
                                    || !matches!(r, CastResource::FlashbackSacrifice(_)))
                        })
                        .collect(),
                ));
            }
            let madness = self.settled_cast_cost_madness.remove(&resource.object());
            let replacement = if matches!(
                resource,
                CastResource::DiscardForSpell(_)
                    | CastResource::Retrace(_)
                    | CastResource::JumpStart(_)
                    | CastResource::Emerge(_)
                    | CastResource::Bargain(_)
                    | CastResource::SacrificeForSpell(_)
            ) {
                Some(
                    self.settled_cast_cost_destinations
                        .remove(&resource.object())
                        .ok_or(EffectError::InconsistentProgram)?
                        .physical(None)
                        .ok_or(EffectError::InconsistentProgram)?,
                )
            } else {
                None
            };
            if let CastResource::SacrificeForSpell(object) = resource {
                let mana_value = self.permanent_mana_value(object);
                let evaluated = before.view().evaluated;
                let creature = evaluated.get(object).filter(|value| value.is_creature());
                let stats = creature.and_then(|value| value.power_toughness.as_ref());
                if let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) {
                    payload.sacrificed_creature_power = stats.map(|pt| pt.derived.power.clone());
                    payload.sacrificed_creature_toughness =
                        stats.map(|pt| pt.derived.toughness.clone());
                    payload.sacrificed_creature_subtypes = creature
                        .map(|value| value.current_creature_subtypes.clone())
                        .unwrap_or_default();
                    payload.sacrificed_lki = evaluated.get(object).cloned().map(Box::new);
                    payload.sacrificed_permanent_mana_value = mana_value;
                }
            }
            let (window, original, after) =
                self.window
                    .pay_cast_resource_replaced(entry, resource, madness, replacement)?;
            self.window = window;
            self.emit_from(
                EventCause::CastObjectPayment { entry, resource },
                &before,
                turn,
                append,
                |previous, current, controllers| {
                    let occurrence = match resource {
                        CastResource::WebSlinging(_) | CastResource::Sneak(_) => {
                            zone_occurrence_at(
                                previous,
                                current,
                                &ObjectMovement {
                                    before: original,
                                    after,
                                },
                                ZoneReason::ReturnToOwnersHand,
                                controllers,
                            )?
                        }
                        CastResource::SacrificeForSpell(_)
                        | CastResource::Emerge(_)
                        | CastResource::Bargain(_)
                        | CastResource::Delve(_)
                        | CastResource::Retrace(_)
                        | CastResource::DiscardForSpell(_)
                        | CastResource::JumpStart(_) => {
                            let reason = if matches!(
                                resource,
                                CastResource::SacrificeForSpell(_)
                                    | CastResource::Bargain(_)
                                    | CastResource::Emerge(_)
                            ) {
                                ZoneReason::SacrificeCost
                            } else if matches!(resource, CastResource::Delve(_)) {
                                ZoneReason::DelveCost
                            } else {
                                ZoneReason::Discard {
                                    player: original.owner(),
                                }
                            };
                            zone_occurrence_at(
                                previous,
                                current,
                                &ObjectMovement {
                                    before: original,
                                    after,
                                },
                                reason,
                                controllers,
                            )?
                        }
                        _ => RulesOccurrence::StatusChange {
                            change: ObjectStatusChange {
                                before: original,
                                after,
                            },
                            reason: StatusReason::CastObjectCost,
                        },
                    };
                    Ok(vec![occurrence_flow::madness_replaced(occurrence, madness)])
                },
            )?;
            if self.linked_return_work.is_some() {
                return Ok(Self::preserve_earlier_extra_costs(
                    &resources[..index],
                    resources[index + 1..]
                        .iter()
                        .copied()
                        .filter(|r| {
                            !hand_exiles_paid
                                || !matches!(r, CastResource::ExileHandForAlternative(_))
                        })
                        .filter(|r| {
                            (!escape_paid || !matches!(r, CastResource::Escape(_)))
                                && (!flashback_sacrifice_paid
                                    || !matches!(r, CastResource::FlashbackSacrifice(_)))
                        })
                        .collect(),
                ));
            }
        }
        self.pay_supplementary_cast_extra_costs(resources, entry, turn, append)?;
        Ok(Vec::new())
    }

    fn preserve_earlier_extra_costs(
        prefix: &[CastResource],
        tail: Vec<CastResource>,
    ) -> Vec<CastResource> {
        let mut preserved: Vec<CastResource> = prefix
            .iter()
            .copied()
            .filter(|r| matches!(r, CastResource::ExtraCost(_)))
            .collect();
        preserved.extend(tail);
        preserved
    }

    fn pay_supplementary_cast_extra_costs(
        &mut self,
        resources: &[CastResource],
        entry: StackEntryId,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let Some(stack_entry) = self
            .window
            .entries()
            .iter()
            .find(|e| e.id() == entry)
            .cloned()
        else {
            return Ok(());
        };
        let Some(spell_id) = self.window.spell_object(&stack_entry) else {
            return Ok(());
        };
        let Some(program) = self.copiable_program(spell_id).cloned() else {
            return Ok(());
        };
        let mut supplementary = Vec::new();
        if stack_entry.paid_conditional_cost()
            && let Some(crate::program::ConditionalCastCost::Alternative { extra, .. }) =
                &program.conditional_cost
        {
            supplementary.extend(extra.iter().cloned());
        }
        for &index in stack_entry.kicker_choices().keys() {
            if let Some(crate::program::AdditionalCastCost::KickerExtra { extra, .. }) =
                program.additional_cast_costs.get(index)
            {
                supplementary.extend(extra.iter().cloned());
            }
        }
        let paid_other_gy_alt = stack_entry.paid_conditional_cost()
            && matches!(
                &program.conditional_cost,
                Some(
                    crate::program::ConditionalCastCost::Escape { .. }
                        | crate::program::ConditionalCastCost::Mayhem(_)
                        | crate::program::ConditionalCastCost::Harmonize(_)
                )
            );
        if stack_entry.cast_from_graveyard
            && !stack_entry.cast_with_flashback()
            && !stack_entry.cast_with_jump_start()
            && !stack_entry.exiles_if_put_into_graveyard()
            && !paid_other_gy_alt
            && !self.source_cast_pending.contains_key(&entry)
            && !resources
                .iter()
                .any(|r| matches!(r, CastResource::Retrace(_)))
            && let Some(
                crate::program::GraveyardSelfCast::WithExtraCost(extra)
                | crate::program::GraveyardSelfCast::WithAlternativeCost { extra, .. },
            ) = &program.graveyard_self_cast
        {
            supplementary.extend(extra.iter().cloned());
        }
        let extra_objects: Vec<ObjectId> = resources
            .iter()
            .filter_map(|r| match r {
                CastResource::ExtraCost(id) => Some(*id),
                _ => None,
            })
            .collect();
        if supplementary.is_empty() {
            if program.extra_cast_costs.is_empty() && !extra_objects.is_empty() {
                return Err(EffectError::InvalidCastResource);
            }
            return Ok(());
        }
        let selected_objects: &[ObjectId] = if program.extra_cast_costs.is_empty() {
            &extra_objects
        } else {
            &[]
        };
        self.pay_extra_costs_at_cut(
            stack_entry.controller(),
            None,
            entry,
            EventCause::CastLifePayment { entry },
            &supplementary,
            selected_objects,
            stack_entry.chosen_x(),
            turn,
            append,
        )
    }
}
