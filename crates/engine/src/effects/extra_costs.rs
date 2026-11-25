//! Atomic validation and payment of composable `ExtraActivationCost` atoms
//! shared by activated abilities and mandatory additional spell casting costs.
use super::*;
use crate::activated::{CostExileZone, CounterCostDelta, ExtraActivationCost, LifeCostAmount};
use crate::occurrence::{EventCause, RulesOccurrence, StatusReason, ZoneReason};

impl EffectWindow {
    /// Atomically validate and pay every cost atom in `extra_costs`.
    /// Callers run on a cloned `EffectWindow`, so any error rejects the whole transition.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pay_extra_costs_at_cut(
        &mut self,
        actor: SeatId,
        source: Option<ObjectId>,
        entry: StackEntryId,
        cause: EventCause,
        extra_costs: &[ExtraActivationCost],
        selected_objects: &[ObjectId],
        chosen_x: Option<&RulesInteger>,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let reserved_unrestricted = BTreeMap::new();
        self.pay_extra_costs_with_reserved_mana(
            actor,
            source,
            entry,
            cause,
            extra_costs,
            selected_objects,
            chosen_x,
            turn,
            &reserved_unrestricted,
            append,
        )
    }

    // Forwards activation context and base mana reservation without an extra wrapper allocation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pay_extra_costs_with_reserved_mana(
        &mut self,
        actor: SeatId,
        source: Option<ObjectId>,
        entry: StackEntryId,
        cause: EventCause,
        extra_costs: &[ExtraActivationCost],
        selected_objects: &[ObjectId],
        chosen_x: Option<&RulesInteger>,
        turn: Option<&TurnScheduler>,
        reserved_unrestricted: &BTreeMap<ManaType, RulesInteger>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if extra_costs.is_empty() {
            return Ok(());
        }
        let mut cursor = 0usize;
        for cost in extra_costs {
            self.pay_one_extra_cost(
                actor,
                source,
                entry,
                &cause,
                cost,
                selected_objects,
                &mut cursor,
                chosen_x,
                turn,
                reserved_unrestricted,
                append,
            )?;
        }
        if cursor != selected_objects.len() {
            return Err(EffectError::InvalidCastResource);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn pay_one_extra_cost(
        &mut self,
        actor: SeatId,
        source: Option<ObjectId>,
        entry: StackEntryId,
        cause: &EventCause,
        cost: &ExtraActivationCost,
        selected: &[ObjectId],
        cursor: &mut usize,
        chosen_x: Option<&RulesInteger>,
        turn: Option<&TurnScheduler>,
        reserved_unrestricted: &BTreeMap<ManaType, RulesInteger>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let take_n = |cursor: &mut usize, n: usize| -> Result<Vec<ObjectId>, EffectError> {
            let end = cursor
                .checked_add(n)
                .ok_or(EffectError::InvalidCastResource)?;
            if end > selected.len() {
                return Err(EffectError::InvalidCastResource);
            }
            let slice = selected[*cursor..end].to_vec();
            *cursor = end;
            Ok(slice)
        };
        let resolve_count = |opt: Option<std::num::NonZeroU8>| -> Result<usize, EffectError> {
            match opt {
                Some(nz) => Ok(usize::from(nz.get())),
                None => chosen_x
                    .and_then(|x| x.to_usize())
                    .ok_or(EffectError::InvalidCastResource),
            }
        };
        match cost {
            ExtraActivationCost::TapSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                if self.window.cards().get(id).and_then(GameObject::tapped) != Some(false) {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, changes) = self.window.tap_for_effect(entry, &[id])?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                    Ok(occurrence_flow::status_occurrences(
                        changes,
                        StatusReason::ActivatedAbilityCost,
                    ))
                })?;
            }
            ExtraActivationCost::UntapSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                if self.window.cards().get(id).and_then(GameObject::tapped) != Some(true) {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let occurrences = self.untap_for_effect_with_stun(entry, &[id])?;
                self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                    Ok(occurrences)
                })?;
            }
            ExtraActivationCost::ExertSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                if self
                    .window
                    .cards()
                    .get(id)
                    .is_none_or(|c| c.zone() != ObjectZone::Battlefield)
                {
                    return Err(EffectError::InvalidCastResource);
                }
                self.skip_next_controller_untap.insert(id);
            }
            ExtraActivationCost::SacrificeSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                let evaluated = self.evaluated_characteristics()?;
                if evaluated.sacrifice_prohibited(id)
                    || evaluated.get(id).is_none_or(|v| {
                        v.zone != ObjectZone::Battlefield || v.controller != Some(actor)
                    })
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::SacrificeCost,
                    &[(id, crate::object_identity::ObjectDestination::Graveyard)],
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::SacrificeCost, ctrls))
                        .collect()
                })?;
            }
            ExtraActivationCost::ExileSource {
                from_graveyard,
                from_hand,
            } => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                let expected = if *from_graveyard {
                    ObjectZone::Graveyard(actor)
                } else if *from_hand {
                    ObjectZone::Hand(actor)
                } else {
                    ObjectZone::Battlefield
                };
                if self
                    .window
                    .cards()
                    .get(id)
                    .is_none_or(|c| c.zone() != expected)
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::ExileCost,
                    &[(id, crate::object_identity::ObjectDestination::Exile)],
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                        .collect()
                })?;
            }
            ExtraActivationCost::ReturnSourceToHand => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                if self
                    .window
                    .cards()
                    .get(id)
                    .is_none_or(|c| c.zone() != ObjectZone::Battlefield)
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::SacrificeCost,
                    &[(id, crate::object_identity::ObjectDestination::Hand)],
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| {
                            zone_occurrence_at(old, cur, m, ZoneReason::ReturnToOwnersHand, ctrls)
                        })
                        .collect()
                })?;
            }
            ExtraActivationCost::DiscardSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                if self
                    .window
                    .cards()
                    .get(id)
                    .is_none_or(|c| c.zone() != ObjectZone::Hand(actor))
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::DiscardCost,
                    &[(id, crate::object_identity::ObjectDestination::Graveyard)],
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| {
                            zone_occurrence_at(
                                old,
                                cur,
                                m,
                                ZoneReason::Discard { player: actor },
                                ctrls,
                            )
                        })
                        .collect()
                })?;
            }
            ExtraActivationCost::PayLife(amount) => {
                let life = match amount {
                    LifeCostAmount::Fixed(n) => RulesInteger::from(i64::from(*n)),
                    LifeCostAmount::CommanderColors => {
                        RulesInteger::from_usize(self.commander_mana_colors(actor)?.len())
                    }
                    LifeCostAmount::ChosenX => chosen_x
                        .filter(|x| **x >= RulesInteger::from(0))
                        .cloned()
                        .ok_or(EffectError::InvalidCastResource)?,
                };
                if life.is_positive() {
                    let evaluated = self.evaluated_characteristics()?;
                    if evaluated.life_and_creature_costs_prohibited()
                        || self.life.get(actor).is_none_or(|total| *total < life)
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    self.pay_life_at_cut(actor, life, cause.clone(), turn, append)?;
                }
            }
            ExtraActivationCost::PayHalfLife => {
                let total = self
                    .life
                    .get(actor)
                    .cloned()
                    .ok_or(EffectError::InvalidCastResource)?;
                let half = if total.is_positive() {
                    total.nonnegative_half_rounded_up()
                } else {
                    0.into()
                };
                if half.is_positive() {
                    let evaluated = self.evaluated_characteristics()?;
                    if evaluated.life_and_creature_costs_prohibited() {
                        return Err(EffectError::InvalidCastResource);
                    }
                    self.pay_life_at_cut(actor, half, cause.clone(), turn, append)?;
                }
            }
            ExtraActivationCost::PayEnergy(n) => {
                let needed = RulesInteger::from(i64::from(*n));
                self.pay_energy_at_cut(actor, needed, cause.clone(), turn, append)?;
            }
            ExtraActivationCost::PayEnergyExpr(expr) => {
                let needed = if expr.is_chosen_x() {
                    chosen_x.cloned().ok_or(EffectError::InvalidCastResource)?
                } else {
                    self.resolve_value_expr(actor, expr)?
                }
                .max(0.into());
                self.pay_energy_at_cut(actor, needed, cause.clone(), turn, append)?;
            }
            ExtraActivationCost::Mill(count) => {
                let needed = usize::from(count.get());
                let library = self.window.cards().library(actor);
                if library.len() < needed {
                    return Err(EffectError::InvalidCastResource);
                }
                let moves: Vec<_> = library[..needed]
                    .iter()
                    .map(|&id| (id, crate::object_identity::ObjectDestination::Graveyard))
                    .collect();
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::MillCost,
                    &moves,
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| {
                            zone_occurrence_at(
                                old,
                                cur,
                                m,
                                ZoneReason::Mill { player: actor },
                                ctrls,
                            )
                        })
                        .collect()
                })?;
            }
            ExtraActivationCost::SourceCounter { kind, delta } => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                self.pay_counter_delta_at_cut(
                    id,
                    *kind,
                    *delta,
                    chosen_x,
                    cause.clone(),
                    turn,
                    append,
                )?;
            }
            ExtraActivationCost::RemovePermanentCounters {
                kind,
                count,
                all,
                filter,
            } => {
                let id = *take_n(cursor, 1)?
                    .first()
                    .ok_or(EffectError::InvalidCastResource)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let valid = evaluated.get(id).is_some_and(|view| {
                    view.zone == ObjectZone::Battlefield
                        && view.controller == Some(actor)
                        && self.matches_object_filter(
                            *filter, view, actor, source, &excluded, &evaluated,
                        )
                });
                if !valid {
                    return Err(EffectError::InvalidCastResource);
                }
                if let Some(chosen_kind) = kind {
                    let delta = if *all {
                        CounterCostDelta::RemoveAll
                    } else {
                        let needed = resolve_count(*count)?;
                        let u = u8::try_from(needed)
                            .ok()
                            .and_then(std::num::NonZeroU8::new)
                            .ok_or(EffectError::InvalidCastResource)?;
                        CounterCostDelta::Remove(u)
                    };
                    self.pay_counter_delta_at_cut(
                        id,
                        *chosen_kind,
                        delta,
                        None,
                        cause.clone(),
                        turn,
                        append,
                    )?;
                } else {
                    let present_kinds: Vec<(CounterKind, RulesInteger)> = self
                        .characteristics
                        .present_permanent_counter_kinds(id)
                        .into_iter()
                        .map(|ck| (ck, self.counters_of_kind(id, ck)))
                        .filter(|(_, cnt)| cnt.is_positive())
                        .collect();
                    if *all {
                        if present_kinds.is_empty() {
                            return Err(EffectError::InvalidCastResource);
                        }
                        for (ck, _) in present_kinds {
                            self.pay_counter_delta_at_cut(
                                id,
                                ck,
                                CounterCostDelta::RemoveAll,
                                None,
                                cause.clone(),
                                turn,
                                append,
                            )?;
                        }
                    } else {
                        let needed = resolve_count(*count)?;
                        let u = u8::try_from(needed)
                            .ok()
                            .and_then(std::num::NonZeroU8::new)
                            .ok_or(EffectError::InvalidCastResource)?;
                        let req = RulesInteger::from(i64::from(u.get()));
                        let chosen_kind = present_kinds
                            .iter()
                            .find(|(_, cnt)| *cnt >= req)
                            .map(|(ck, _)| *ck)
                            .ok_or(EffectError::InvalidCastResource)?;
                        self.pay_counter_delta_at_cut(
                            id,
                            chosen_kind,
                            CounterCostDelta::Remove(u),
                            None,
                            cause.clone(),
                            turn,
                            append,
                        )?;
                    }
                }
            }
            ExtraActivationCost::PutPermanentCounters {
                kind,
                count,
                filter,
            } => {
                let id = *take_n(cursor, 1)?
                    .first()
                    .ok_or(EffectError::InvalidCastResource)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let valid = evaluated.get(id).is_some_and(|view| {
                    view.zone == ObjectZone::Battlefield
                        && view.controller == Some(actor)
                        && self.matches_object_filter(
                            *filter, view, actor, source, &excluded, &evaluated,
                        )
                });
                if !valid {
                    return Err(EffectError::InvalidCastResource);
                }
                self.pay_counter_delta_at_cut(
                    id,
                    *kind,
                    CounterCostDelta::Add(*count),
                    None,
                    cause.clone(),
                    turn,
                    append,
                )?;
            }
            ExtraActivationCost::SacrificePermanent { filter, count } => {
                let needed = resolve_count(*count)?;
                let ids = take_n(cursor, needed)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if evaluated.sacrifice_prohibited(id)
                        || !evaluated.get(id).is_some_and(|view| {
                            view.zone == ObjectZone::Battlefield
                                && view.controller == Some(actor)
                                && self.matches_object_filter(
                                    *filter, view, actor, source, &excluded, &evaluated,
                                )
                        })
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Graveyard))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::SacrificeCost,
                        &moves,
                    )?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| {
                                zone_occurrence_at(old, cur, m, ZoneReason::SacrificeCost, ctrls)
                            })
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::TapPermanent {
                filter,
                count,
                exclude_source,
            } => {
                let needed = resolve_count(*count)?;
                let ids = take_n(cursor, needed)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if (*exclude_source && Some(id) == source)
                        || self.window.cards().get(id).and_then(GameObject::tapped) != Some(false)
                        || !evaluated.get(id).is_some_and(|view| {
                            view.zone == ObjectZone::Battlefield
                                && view.controller == Some(actor)
                                && self.matches_object_filter(
                                    *filter, view, actor, source, &excluded, &evaluated,
                                )
                        })
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if !ids.is_empty() {
                    let before = self.rules_snapshot(turn)?;
                    let (window, changes) = self.window.tap_for_effect(entry, &ids)?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                        Ok(occurrence_flow::status_occurrences(
                            changes,
                            StatusReason::ActivatedAbilityCost,
                        ))
                    })?;
                }
            }
            ExtraActivationCost::UntapPermanent {
                filter,
                count,
                exclude_source,
            } => {
                let ids = take_n(cursor, usize::from(count.get()))?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if (*exclude_source && Some(id) == source)
                        || self.window.cards().get(id).and_then(GameObject::tapped) != Some(true)
                        || !evaluated.get(id).is_some_and(|view| {
                            view.zone == ObjectZone::Battlefield
                                && view.controller == Some(actor)
                                && self.matches_object_filter(
                                    *filter, view, actor, source, &excluded, &evaluated,
                                )
                        })
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                let before = self.rules_snapshot(turn)?;
                let occurrences = self.untap_for_effect_with_stun(entry, &ids)?;
                self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                    Ok(occurrences)
                })?;
            }
            ExtraActivationCost::BouncePermanent {
                filter,
                count,
                exclude_source,
            } => {
                let ids = take_n(cursor, usize::from(count.get()))?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if (*exclude_source && Some(id) == source)
                        || !evaluated.get(id).is_some_and(|view| {
                            view.zone == ObjectZone::Battlefield
                                && view.controller == Some(actor)
                                && self.matches_object_filter(
                                    *filter, view, actor, source, &excluded, &evaluated,
                                )
                        })
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                let moves: Vec<_> = ids
                    .iter()
                    .map(|&id| (id, crate::object_identity::ObjectDestination::Hand))
                    .collect();
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::ReturnToHand,
                    &moves,
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| {
                            zone_occurrence_at(old, cur, m, ZoneReason::ReturnToOwnersHand, ctrls)
                        })
                        .collect()
                })?;
            }
            ExtraActivationCost::DiscardCards {
                filter,
                count,
                whole_hand,
                random,
            } => {
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let ids: Vec<ObjectId> = if *whole_hand {
                    self.window
                        .cards()
                        .iter()
                        .filter(|c| c.zone() == ObjectZone::Hand(actor) && c.card_id().is_some())
                        .map(GameObject::id)
                        .collect()
                } else if *random {
                    let needed = resolve_count(*count)?;
                    let matching: Vec<ObjectId> = self
                        .window
                        .cards()
                        .iter()
                        .filter(|c| {
                            c.zone() == ObjectZone::Hand(actor)
                                && c.card_id().is_some()
                                && Some(c.id()) != source
                                && evaluated.get(c.id()).is_some_and(|view| {
                                    self.matches_object_filter(
                                        *filter, view, actor, source, &excluded, &evaluated,
                                    )
                                })
                        })
                        .map(GameObject::id)
                        .collect();
                    if matching.len() < needed {
                        return Err(EffectError::InvalidCastResource);
                    }
                    matching.into_iter().take(needed).collect()
                } else {
                    let needed = resolve_count(*count)?;
                    take_n(cursor, needed)?
                };
                for &id in &ids {
                    if !evaluated.get(id).is_some_and(|view| {
                        view.zone == ObjectZone::Hand(actor)
                            && self.matches_object_filter(
                                *filter, view, actor, source, &excluded, &evaluated,
                            )
                    }) {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Graveyard))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::DiscardCost,
                        &moves,
                    )?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| {
                                zone_occurrence_at(
                                    old,
                                    cur,
                                    m,
                                    ZoneReason::Discard { player: actor },
                                    ctrls,
                                )
                            })
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::RevealHand {
                filter,
                count,
                allow_controlled,
            } => {
                let ids = take_n(cursor, usize::from(count.get()))?;
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() != ids.len() {
                    return Err(EffectError::InvalidCastResource);
                }
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let mut revealed = Vec::new();
                for &id in &ids {
                    let view = evaluated.get(id).ok_or(EffectError::InvalidCastResource)?;
                    let in_hand = view.zone == ObjectZone::Hand(actor);
                    let controlled_bf = *allow_controlled
                        && view.zone == ObjectZone::Battlefield
                        && view.controller == Some(actor);
                    if (!in_hand && !controlled_bf)
                        || !self.matches_object_filter(
                            *filter, view, actor, source, &excluded, &evaluated,
                        )
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                    if in_hand {
                        revealed.push(
                            self.window
                                .cards()
                                .get(id)
                                .cloned()
                                .ok_or(EffectError::InvalidCastResource)?,
                        );
                    }
                }
                if !revealed.is_empty() {
                    if let [only] = revealed.as_slice() {
                        let last_power = evaluated
                            .get(only.id())
                            .and_then(|o| o.power_toughness.as_ref())
                            .map(|stats| stats.derived.power.clone())
                            .unwrap_or_else(|| 0.into());
                        let mana_value = self
                            .copiable_program(only.id())
                            .map(Program::mana_value)
                            .unwrap_or_else(|| 0.into());
                        if let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get_mut(&entry)
                        {
                            payload.revealed_cost_card = Some(RevealedCostCard {
                                object: only.id(),
                                last_power,
                                mana_value,
                            });
                        }
                    }
                    let before = self.rules_snapshot(turn)?;
                    self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                        Ok(revealed
                            .iter()
                            .map(|object| RulesOccurrence::HandCardRevealed {
                                object: object.clone(),
                            })
                            .collect())
                    })?;
                }
            }
            ExtraActivationCost::ExileObjects {
                zone,
                filter,
                count,
                exclude_source,
            } => {
                let needed = resolve_count(*count)?;
                let ids = take_n(cursor, needed)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if *exclude_source && Some(id) == source {
                        return Err(EffectError::InvalidCastResource);
                    }
                    let view = evaluated.get(id).ok_or(EffectError::InvalidCastResource)?;
                    let zone_ok = match zone {
                        CostExileZone::Hand => view.zone == ObjectZone::Hand(actor),
                        CostExileZone::Graveyard => view.zone == ObjectZone::Graveyard(actor),
                        CostExileZone::Battlefield => {
                            view.zone == ObjectZone::Battlefield && view.controller == Some(actor)
                        }
                    };
                    if !zone_ok
                        || !self.matches_object_filter(
                            *filter, view, actor, source, &excluded, &evaluated,
                        )
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Exile))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::ExileCost,
                        &moves,
                    )?;
                    self.window = window;
                    if *zone == CostExileZone::Graveyard
                        && let [movement] = movements.as_slice()
                        && movement.after.zone() == ObjectZone::Exile
                    {
                        let exiled_id = movement.after.id();
                        let power = self
                            .current_characteristics(exiled_id)?
                            .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                            .unwrap_or_else(|| 0.into());
                        if let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get_mut(&entry)
                        {
                            payload.exiled_cost_creature = Some((exiled_id, power));
                        }
                    }
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::CollectEvidence(min_mv) => {
                let needed = RulesInteger::from(i64::from(*min_mv));
                let mut total = RulesInteger::from(0);
                let mut ids = Vec::new();
                while *cursor < selected.len() && total < needed {
                    let id = selected[*cursor];
                    if self.window.cards().get(id).is_none_or(|c| {
                        c.zone() != ObjectZone::Graveyard(actor) || c.card_id().is_none()
                    }) {
                        return Err(EffectError::InvalidCastResource);
                    }
                    total = total.plus(&self.permanent_mana_value(id).unwrap_or_else(|| 0.into()));
                    ids.push(id);
                    *cursor += 1;
                }
                if total < needed {
                    return Err(EffectError::InvalidCastResource);
                }
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() != ids.len() {
                    return Err(EffectError::InvalidCastResource);
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Exile))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::ExileCost,
                        &moves,
                    )?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::Forage => {
                let remaining = &selected[*cursor..];
                if let Some(&first) = remaining.first()
                    && let evaluated = self.evaluated_characteristics()?
                    && evaluated.get(first).is_some_and(|v| {
                        v.zone == ObjectZone::Battlefield
                            && v.controller == Some(actor)
                            && v.current_artifact_subtypes
                                .contains(&crate::program::ArtifactSubtype::Food)
                    })
                {
                    if evaluated.sacrifice_prohibited(first) {
                        return Err(EffectError::InvalidCastResource);
                    }
                    *cursor += 1;
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::SacrificeCost,
                        &[(first, crate::object_identity::ObjectDestination::Graveyard)],
                    )?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| {
                                zone_occurrence_at(old, cur, m, ZoneReason::SacrificeCost, ctrls)
                            })
                            .collect()
                    })?;
                } else {
                    let ids = take_n(cursor, 3)?;
                    let unique: BTreeSet<_> = ids.iter().copied().collect();
                    if unique.len() != ids.len() {
                        return Err(EffectError::InvalidCastResource);
                    }
                    for &id in &ids {
                        if self.window.cards().get(id).is_none_or(|c| {
                            c.zone() != ObjectZone::Graveyard(actor) || c.card_id().is_none()
                        }) {
                            return Err(EffectError::InvalidCastResource);
                        }
                    }
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Exile))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.pay_selected_activation_costs(
                        entry,
                        actor,
                        crate::program::ZoneActionKind::ExileCost,
                        &moves,
                    )?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::Waterbend(waterbend_mana) => {
                let mut tapped_helpers = Vec::new();
                let evaluated = self.evaluated_characteristics()?;
                while *cursor < selected.len() {
                    let id = selected[*cursor];
                    if self.window.cards().get(id).and_then(GameObject::tapped) == Some(false)
                        && evaluated.get(id).is_some_and(|v| {
                            v.zone == ObjectZone::Battlefield
                                && v.controller == Some(actor)
                                && (v.has_type(CardType::Artifact) || v.is_creature())
                        })
                    {
                        tapped_helpers.push(id);
                        *cursor += 1;
                    } else {
                        break;
                    }
                }
                if !tapped_helpers.is_empty() {
                    let before = self.rules_snapshot(turn)?;
                    let (window, changes) = self.window.tap_for_effect(entry, &tapped_helpers)?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                        Ok(occurrence_flow::status_occurrences(
                            changes,
                            StatusReason::ActivatedAbilityCost,
                        ))
                    })?;
                }
                let mut remaining_cost = waterbend_mana.clone();
                remaining_cost.generic = remaining_cost
                    .generic
                    .minus(&RulesInteger::from_usize(tapped_helpers.len()))
                    .max(0.into());
                if remaining_cost != ManaCost::free() {
                    let spend = self.prepare_mana_payment(&remaining_cost, None, actor)?;
                    let receipt = ManaPaymentRecord {
                        from_lots: Default::default(),
                        source: entry,
                        payer: actor,
                        cost: remaining_cost,
                        spend,
                        restricted_mana: Vec::new(),
                    };
                    self.debit_mana_at_cut(&receipt, false, turn, append)?;
                }
            }
            ExtraActivationCost::Blight(count) => {
                let id = *take_n(cursor, 1)?
                    .first()
                    .ok_or(EffectError::InvalidCastResource)?;
                let evaluated = self.evaluated_characteristics()?;
                if !evaluated.get(id).is_some_and(|v| {
                    v.zone == ObjectZone::Battlefield
                        && v.controller == Some(actor)
                        && v.is_creature()
                }) {
                    return Err(EffectError::InvalidCastResource);
                }
                self.pay_counter_delta_at_cut(
                    id,
                    CounterKind::MinusOne,
                    CounterCostDelta::Add(*count),
                    None,
                    cause.clone(),
                    turn,
                    append,
                )?;
            }
            ExtraActivationCost::ProcessExiledCard => {
                let id = *take_n(cursor, 1)?
                    .first()
                    .ok_or(EffectError::InvalidCastResource)?;
                let card = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::InvalidCastResource)?;
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), actor)?;
                if card.zone() != ObjectZone::Exile || !opponents.contains(&card.owner()) {
                    return Err(EffectError::InvalidCastResource);
                }
                let before = self.rules_snapshot(turn)?;
                let (window, movements) = self.window.pay_selected_activation_costs(
                    entry,
                    actor,
                    crate::program::ZoneActionKind::ExileCost,
                    &[(id, crate::object_identity::ObjectDestination::Graveyard)],
                )?;
                self.window = window;
                self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                    movements
                        .iter()
                        .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                        .collect()
                })?;
            }
            ExtraActivationCost::Unattach { self_source } => {
                let id = if *self_source {
                    source.ok_or(EffectError::InvalidCastResource)?
                } else {
                    *take_n(cursor, 1)?
                        .first()
                        .ok_or(EffectError::InvalidCastResource)?
                };
                if self.characteristics.attachments.remove(&id).is_none() {
                    return Err(EffectError::InvalidCastResource);
                }
            }
            ExtraActivationCost::PayMana(mana_cost) => {
                if *mana_cost != ManaCost::free() {
                    let spend = self.prepare_mana_payment(mana_cost, None, actor)?;
                    let receipt = ManaPaymentRecord {
                        from_lots: Default::default(),
                        source: entry,
                        payer: actor,
                        cost: mana_cost.clone(),
                        spend,
                        restricted_mana: Vec::new(),
                    };
                    self.debit_mana_at_cut(&receipt, false, turn, append)?;
                }
            }
            ExtraActivationCost::PaySnowMana(count) => {
                let mut needed = RulesInteger::from(i64::from(count.get()));
                let mut spend = Vec::new();
                for mana_type in ManaType::ALL {
                    if !needed.is_positive() {
                        break;
                    }
                    let reserved = reserved_unrestricted
                        .get(&mana_type)
                        .cloned()
                        .unwrap_or_else(|| 0.into());
                    let available_unrestricted = self
                        .unrestricted_mana(actor, mana_type)
                        .minus(&reserved)
                        .max(0.into());
                    let snow_in_pool = self
                        .floating_mana
                        .source_traits
                        .get(&(actor, mana_type))
                        .map_or_else(|| 0.into(), |traits| traits.snow.clone());
                    let available_snow = snow_in_pool.min(available_unrestricted);
                    if available_snow.is_positive() {
                        let take = needed.clone().min(available_snow);
                        spend.push(ManaSpend {
                            mana_type,
                            amount: take.clone(),
                        });
                        needed = needed.minus(&take);
                    }
                }
                if needed.is_positive() {
                    return Err(EffectError::InsufficientMana);
                }
                let receipt = ManaPaymentRecord {
                    from_lots: Default::default(),
                    source: entry,
                    payer: actor,
                    cost: ManaCost::generic(RulesInteger::from(i64::from(count.get()))),
                    spend,
                    restricted_mana: Vec::new(),
                };
                self.debit_mana_at_cut(&receipt, false, turn, append)?;
            }
            ExtraActivationCost::RemoveCountersAmong {
                kind,
                count,
                filter,
            } => {
                let needed = resolve_count(*count)?;
                if needed == 0 {
                    return Ok(());
                }
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let mut order: Vec<ObjectId> = Vec::new();
                let mut avail: BTreeMap<ObjectId, Vec<(CounterKind, RulesInteger)>> =
                    BTreeMap::new();
                let mut planned: BTreeMap<(ObjectId, CounterKind), RulesInteger> = BTreeMap::new();
                let mut taken: usize = 0;

                // Pass 1: take 1 counter from each matching selected entry up to `needed`.
                while *cursor < selected.len() && taken < needed {
                    let id = selected[*cursor];
                    let valid = evaluated.get(id).is_some_and(|view| {
                        view.zone == ObjectZone::Battlefield
                            && view.controller == Some(actor)
                            && self.matches_object_filter(
                                *filter, view, actor, source, &excluded, &evaluated,
                            )
                    });
                    if !valid {
                        break;
                    }
                    if let std::collections::btree_map::Entry::Vacant(slot) = avail.entry(id) {
                        let kinds: Vec<(CounterKind, RulesInteger)> = if let Some(ck) = kind {
                            let cnt = self.counters_of_kind(id, *ck);
                            if cnt.is_positive() {
                                vec![(*ck, cnt)]
                            } else {
                                Vec::new()
                            }
                        } else {
                            self.characteristics
                                .present_permanent_counter_kinds(id)
                                .into_iter()
                                .map(|ck| (ck, self.counters_of_kind(id, ck)))
                                .filter(|(_, cnt)| cnt.is_positive())
                                .collect()
                        };
                        if kinds.is_empty() {
                            break;
                        }
                        slot.insert(kinds);
                        order.push(id);
                    }
                    let taken_kind = avail.get_mut(&id).and_then(|kinds| {
                        kinds.iter_mut().find_map(|(ck, rem)| {
                            if rem.is_positive() {
                                *rem = rem.minus(&1.into());
                                Some(*ck)
                            } else {
                                None
                            }
                        })
                    });
                    let Some(ck) = taken_kind else {
                        break;
                    };
                    let slot = planned.entry((id, ck)).or_insert_with(|| 0.into());
                    *slot = slot.plus(&1.into());
                    taken += 1;
                    *cursor += 1;
                }

                // Pass 2: greedily take remaining counters across selected permanents in order.
                let mut remaining = RulesInteger::from_usize(needed.saturating_sub(taken));
                if remaining.is_positive() {
                    for &id in &order {
                        if !remaining.is_positive() {
                            break;
                        }
                        if let Some(kinds) = avail.get_mut(&id) {
                            for (ck, rem) in kinds.iter_mut() {
                                if !remaining.is_positive() {
                                    break;
                                }
                                if rem.is_positive() {
                                    let take = remaining.clone().min(rem.clone());
                                    *rem = rem.minus(&take);
                                    let slot = planned.entry((id, *ck)).or_insert_with(|| 0.into());
                                    *slot = slot.plus(&take);
                                    remaining = remaining.minus(&take);
                                }
                            }
                        }
                    }
                }
                if remaining.is_positive() {
                    return Err(EffectError::InvalidCastResource);
                }
                for id in order {
                    let removals: Vec<(CounterKind, RulesInteger)> = planned
                        .iter()
                        .filter(|((obj, _), _)| *obj == id)
                        .map(|((_, ck), amt)| (*ck, amt.clone()))
                        .collect();
                    for (ck, amt) in removals {
                        self.pay_counter_delta_at_cut(
                            id,
                            ck,
                            CounterCostDelta::RemoveX,
                            Some(&amt),
                            cause.clone(),
                            turn,
                            append,
                        )?;
                    }
                }
            }
            ExtraActivationCost::ExileTopLibrary { count, face_down } => {
                let needed = resolve_count(*count)?;
                let library = self.window.cards().library(actor);
                if library.len() < needed {
                    return Err(EffectError::InvalidCastResource);
                }
                if needed > 0 {
                    let moves: Vec<_> = library[..needed]
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Exile))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.record_linked_return_moves(&moves)?;
                    self.window = window;
                    if *face_down {
                        for movement in &movements {
                            if movement.after.zone() == ObjectZone::Exile {
                                self.face_down_exile.insert(movement.after.id(), actor);
                            }
                        }
                    }
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::RevealSource => {
                let id = source.ok_or(EffectError::InvalidCastResource)?;
                let card = self
                    .window
                    .cards()
                    .get(id)
                    .filter(|c| c.zone() == ObjectZone::Hand(actor))
                    .cloned()
                    .ok_or(EffectError::InvalidCastResource)?;
                let before = self.rules_snapshot(turn)?;
                self.emit_from(cause.clone(), &before, turn, append, |_, _, _| {
                    Ok(vec![RulesOccurrence::HandCardRevealed {
                        object: card.clone(),
                    }])
                })?;
            }
            ExtraActivationCost::SacrificeAnyNumber(filter) => {
                let ids = if let Some(x) = chosen_x {
                    let needed = x.to_usize().ok_or(EffectError::InvalidCastResource)?;
                    take_n(cursor, needed)?
                } else {
                    let remaining = selected.len().saturating_sub(*cursor);
                    take_n(cursor, remaining)?
                };
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() != ids.len() {
                    return Err(EffectError::InvalidCastResource);
                }
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    if evaluated.sacrifice_prohibited(id)
                        || !evaluated.get(id).is_some_and(|view| {
                            view.zone == ObjectZone::Battlefield
                                && view.controller == Some(actor)
                                && self.matches_object_filter(
                                    *filter, view, actor, source, &excluded, &evaluated,
                                )
                        })
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if chosen_x.is_none() {
                    self.window
                        .set_chosen_x(entry, RulesInteger::from_usize(ids.len()))?;
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Graveyard))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.record_linked_return_moves(&moves)?;
                    self.window = window;
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| {
                                zone_occurrence_at(old, cur, m, ZoneReason::SacrificeCost, ctrls)
                            })
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::ExileAnyNumber {
                zone,
                filter,
                min_one,
                face_down,
            } => {
                let ids = if let Some(x) = chosen_x {
                    let needed = x.to_usize().ok_or(EffectError::InvalidCastResource)?;
                    take_n(cursor, needed)?
                } else {
                    let remaining = selected.len().saturating_sub(*cursor);
                    take_n(cursor, remaining)?
                };
                if *min_one && ids.is_empty() {
                    return Err(EffectError::InvalidCastResource);
                }
                let unique: BTreeSet<_> = ids.iter().copied().collect();
                if unique.len() != ids.len() {
                    return Err(EffectError::InvalidCastResource);
                }
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                for &id in &ids {
                    let view = evaluated.get(id).ok_or(EffectError::InvalidCastResource)?;
                    let zone_ok = match zone {
                        CostExileZone::Hand => view.zone == ObjectZone::Hand(actor),
                        CostExileZone::Graveyard => view.zone == ObjectZone::Graveyard(actor),
                        CostExileZone::Battlefield => {
                            view.zone == ObjectZone::Battlefield && view.controller == Some(actor)
                        }
                    };
                    if !zone_ok
                        || !self.matches_object_filter(
                            *filter, view, actor, source, &excluded, &evaluated,
                        )
                    {
                        return Err(EffectError::InvalidCastResource);
                    }
                }
                if chosen_x.is_none() {
                    self.window
                        .set_chosen_x(entry, RulesInteger::from_usize(ids.len()))?;
                }
                if !ids.is_empty() {
                    let moves: Vec<_> = ids
                        .iter()
                        .map(|&id| (id, crate::object_identity::ObjectDestination::Exile))
                        .collect();
                    let before = self.rules_snapshot(turn)?;
                    let (window, movements) = self.window.record_linked_return_moves(&moves)?;
                    self.window = window;
                    if *face_down {
                        for movement in &movements {
                            if movement.after.zone() == ObjectZone::Exile {
                                self.face_down_exile.insert(movement.after.id(), actor);
                            }
                        }
                    } else if *zone == CostExileZone::Graveyard
                        && let [movement] = movements.as_slice()
                        && movement.after.zone() == ObjectZone::Exile
                    {
                        let exiled_id = movement.after.id();
                        let power = self
                            .current_characteristics(exiled_id)?
                            .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                            .unwrap_or_else(|| 0.into());
                        if let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get_mut(&entry)
                        {
                            payload.exiled_cost_creature = Some((exiled_id, power));
                        }
                    }
                    self.emit_from(cause.clone(), &before, turn, append, |old, cur, ctrls| {
                        movements
                            .iter()
                            .map(|m| zone_occurrence_at(old, cur, m, ZoneReason::ExileCost, ctrls))
                            .collect()
                    })?;
                }
            }
            ExtraActivationCost::Either(a, b) => {
                let has_selected = *cursor < selected.len();
                let (first, second) =
                    if !a.uses_selected_objects() && b.uses_selected_objects() && has_selected {
                        (b.as_ref(), a.as_ref())
                    } else {
                        (a.as_ref(), b.as_ref())
                    };
                let mut try_self = self.clone();
                let mut try_cursor = *cursor;
                let mut try_append = append.clone();
                if try_self
                    .pay_one_extra_cost(
                        actor,
                        source,
                        entry,
                        cause,
                        first,
                        selected,
                        &mut try_cursor,
                        chosen_x,
                        turn,
                        reserved_unrestricted,
                        &mut try_append,
                    )
                    .is_ok()
                {
                    *self = try_self;
                    *cursor = try_cursor;
                    *append = try_append;
                } else {
                    self.pay_one_extra_cost(
                        actor,
                        source,
                        entry,
                        cause,
                        second,
                        selected,
                        cursor,
                        chosen_x,
                        turn,
                        reserved_unrestricted,
                        append,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn pay_energy_at_cut(
        &mut self,
        actor: SeatId,
        needed: RulesInteger,
        cause: EventCause,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !needed.is_positive() {
            return Ok(());
        }
        let before = self.player_counters_of_kind(actor, CounterKind::Energy);
        if before < needed {
            return Err(EffectError::InvalidCastResource);
        }
        let after = before.minus(&needed);
        let snap = self.rules_snapshot(turn)?;
        let map = self
            .characteristics
            .player_counters
            .entry(CounterKind::Energy)
            .or_default();
        if after.is_positive() {
            map.insert(actor, after.clone());
        } else {
            map.remove(&actor);
        }
        self.emit_from(cause, &snap, turn, append, |_, _, _| {
            Ok(vec![RulesOccurrence::PlayerCounterChanged {
                player: actor,
                kind: CounterKind::Energy,
                before: before.clone(),
                after: after.clone(),
            }])
        })
    }

    // Internal helper takes counter target, delta, optional X amount, and journal context directly.
    #[allow(clippy::too_many_arguments)]
    fn pay_counter_delta_at_cut(
        &mut self,
        target: ObjectId,
        kind: CounterKind,
        delta: CounterCostDelta,
        chosen_x: Option<&RulesInteger>,
        cause: EventCause,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let current_count = self.counters_of_kind(target, kind);
        let change = match delta {
            CounterCostDelta::Add(n) => RulesInteger::from(i64::from(n.get())),
            CounterCostDelta::Remove(n) => {
                let amount = RulesInteger::from(i64::from(n.get()));
                if amount > current_count {
                    return Err(EffectError::InvalidCastResource);
                }
                RulesInteger::from(0).minus(&amount)
            }
            CounterCostDelta::RemoveX => {
                let amount = chosen_x
                    .filter(|x| **x >= RulesInteger::from(0))
                    .ok_or(EffectError::InvalidCastResource)?;
                if *amount > current_count {
                    return Err(EffectError::InvalidCastResource);
                }
                RulesInteger::from(0).minus(amount)
            }
            CounterCostDelta::RemoveAll => RulesInteger::from(0).minus(&current_count),
        };
        if change == 0.into() {
            return Ok(());
        }
        let snap = self.rules_snapshot(turn)?;
        match kind {
            CounterKind::PlusOne | CounterKind::MinusOne => {
                let before = self.characteristics.counters(target);
                let magnitude = if change.is_positive() {
                    change.clone()
                } else {
                    RulesInteger::from(0).minus(&change)
                };
                let delta_counters = if kind == CounterKind::PlusOne {
                    crate::characteristics::PowerToughnessCounters {
                        plus_one: magnitude,
                        minus_one: 0.into(),
                    }
                } else {
                    crate::characteristics::PowerToughnessCounters {
                        plus_one: 0.into(),
                        minus_one: magnitude,
                    }
                };
                if change.is_positive() {
                    self.characteristics
                        .place_counters(target, &delta_counters)?;
                    let after = self.characteristics.counters(target);
                    self.emit_from(cause, &snap, turn, append, |prev, cur, _| {
                        Ok(vec![RulesOccurrence::CountersPlaced {
                            object: target,
                            before: before.clone(),
                            added: delta_counters.clone(),
                            after: after.clone(),
                            before_appearance: Box::new(
                                prev.appearance(target)
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                            after_appearance: Box::new(
                                cur.appearance(target)
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                        }])
                    })?;
                } else {
                    let after = self
                        .characteristics
                        .take_power_toughness_counters(target, &delta_counters)
                        .map_err(|_| EffectError::InvalidCastResource)?;
                    self.emit_from(cause, &snap, turn, append, |_, _, _| {
                        Ok(vec![RulesOccurrence::PowerToughnessCountersRemoved {
                            object: target,
                            before: before.clone(),
                            after: after.clone(),
                        }])
                    })?;
                }
            }
            _ => {
                let counters = self
                    .characteristics
                    .named_permanent_counters_mut(kind)
                    .ok_or(EffectError::InconsistentProgram)?;
                let before = counters.get(&target).cloned().unwrap_or_else(|| 0.into());
                let after = before.plus(&change);
                if after < 0.into() {
                    return Err(EffectError::InvalidCastResource);
                }
                if after.is_positive() {
                    counters.insert(target, after.clone());
                } else {
                    counters.remove(&target);
                }
                self.emit_from(cause, &snap, turn, append, |_, _, _| {
                    Ok(vec![RulesOccurrence::PermanentCounterChanged {
                        object: target,
                        kind,
                        before: before.clone(),
                        after: after.clone(),
                    }])
                })?;
            }
        }
        Ok(())
    }
}
