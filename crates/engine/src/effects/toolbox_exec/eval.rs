//! Query, condition, state-track, and `ValueExpr` evaluation for the Universal Rules Toolbox.

use crate::{
    characteristics::{Color, EvaluatedCharacteristics, ObjectCharacteristics},
    effects::{CounterKind, EffectError, EffectWindow, ResolutionCursor},
    object_identity::{GameObject, ObjectId, ObjectZone},
    priority::SeatId,
    program::{
        AffectedPlayers, ColorMask, Comparison, ObjectFilter, ObjectStatKind, StatAggregate,
        ValueExpr,
    },
    stack_window::{StackEntry, StackKind},
    targets::TargetRef,
    toolbox::{
        ConditionExpr, ExecValue, ObjectQuery, ObjectSnapshot, PlayerSelector, QuerySlice,
        QueryZone, StateTrackKey, StateTrackKeySpec, StateTrackScope,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

fn colors_to_mask(colors: &BTreeSet<Color>) -> ColorMask {
    colors
        .iter()
        .copied()
        .fold(ColorMask::EMPTY, ColorMask::with)
}

fn compare_values(lhs: &RulesInteger, op: Comparison, rhs: &RulesInteger) -> bool {
    match op {
        Comparison::Equal => lhs == rhs,
        Comparison::NotEqual => lhs != rhs,
        Comparison::Less => lhs < rhs,
        Comparison::LessOrEqual => lhs <= rhs,
        Comparison::Greater => lhs > rhs,
        Comparison::GreaterOrEqual => lhs >= rhs,
    }
}

fn snapshot_stat(snap: &ObjectSnapshot, stat: ObjectStatKind) -> Option<RulesInteger> {
    match stat {
        ObjectStatKind::Power => snap.power.clone(),
        ObjectStatKind::Toughness => snap.toughness.clone(),
        ObjectStatKind::ManaValue => Some(snap.mana_value.clone()),
        ObjectStatKind::Loyalty => snap.counters.get(&CounterKind::Loyalty).cloned(),
    }
}

fn live_stat(card: &ObjectCharacteristics, stat: ObjectStatKind) -> Option<RulesInteger> {
    match stat {
        ObjectStatKind::Power => card
            .power_toughness
            .as_ref()
            .map(|pt| pt.derived.power.clone()),
        ObjectStatKind::Toughness => card
            .power_toughness
            .as_ref()
            .map(|pt| pt.derived.toughness.clone()),
        ObjectStatKind::ManaValue => Some(card.defined.mana_value.clone()),
        ObjectStatKind::Loyalty => Some(
            card.defined
                .printed_loyalty
                .clone()
                .unwrap_or_else(|| RulesInteger::from(0)),
        ),
    }
}

fn aggregate_stats(values: Vec<RulesInteger>, op: StatAggregate) -> RulesInteger {
    if values.is_empty() {
        return RulesInteger::from(0);
    }
    match op {
        StatAggregate::Sum => values
            .into_iter()
            .fold(RulesInteger::from(0), |acc, v| acc.plus(&v)),
        StatAggregate::Max => values
            .into_iter()
            .max()
            .unwrap_or_else(|| RulesInteger::from(0)),
        StatAggregate::Min => values
            .into_iter()
            .min()
            .unwrap_or_else(|| RulesInteger::from(0)),
        StatAggregate::CountDistinct => {
            let distinct: BTreeSet<RulesInteger> = values.into_iter().collect();
            RulesInteger::from_usize(distinct.len())
        }
    }
}

impl EffectWindow {
    /// Capture an immutable LKI [`ObjectSnapshot`] for `id` from `evaluated`.
    pub(super) fn capture_object_snapshot(
        &self,
        id: ObjectId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<ObjectSnapshot> {
        let obj = self.window.cards().get(id)?;
        let chars = evaluated.get(id);
        let mut counters = BTreeMap::new();
        if let Some(c) = chars {
            if c.plus_one_counters.is_positive() {
                counters.insert(CounterKind::PlusOne, c.plus_one_counters.clone());
            }
            if c.blessing_counters.is_positive() {
                counters.insert(CounterKind::Blessing, c.blessing_counters.clone());
            }
            if c.charge_counters.is_positive() {
                counters.insert(CounterKind::Charge, c.charge_counters.clone());
            }
        }
        Some(ObjectSnapshot {
            object: id,
            card: obj.card_id(),
            owner: obj.owner(),
            controller: chars
                .and_then(|c| c.controller)
                .or_else(|| obj.controller())
                .unwrap_or_else(|| obj.owner()),
            zone: obj.zone(),
            power: chars
                .and_then(|c| c.power_toughness.as_ref())
                .map(|pt| pt.derived.power.clone()),
            toughness: chars
                .and_then(|c| c.power_toughness.as_ref())
                .map(|pt| pt.derived.toughness.clone()),
            mana_value: chars
                .map(|c| c.defined.mana_value.clone())
                .unwrap_or_else(|| RulesInteger::from(0)),
            card_types: chars.map(|c| c.current_types.clone()).unwrap_or_default(),
            colors: chars
                .map(|c| colors_to_mask(&c.current_colors))
                .unwrap_or(ColorMask::EMPTY),
            subtypes: chars
                .map(|c| c.current_creature_subtypes.clone())
                .unwrap_or_default(),
            counters,
        })
    }

    pub(super) fn entry_source_object(&self, entry: Option<&StackEntry>) -> Option<ObjectId> {
        let entry = entry.or_else(|| self.window.resolving())?;
        match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(id) => Some(*id),
            _ => None,
        }
    }

    /// Evaluate a [`PlayerSelector`] into an ordered, deduplicated list of seats.
    pub(in crate::effects) fn eval_player_selector_at(
        &self,
        controller: SeatId,
        entry: Option<&StackEntry>,
        selector: &PlayerSelector,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<Vec<SeatId>, EffectError> {
        let mut out = Vec::new();
        match selector {
            PlayerSelector::Fixed(seat) => {
                if self.window.participants().contains(seat) {
                    out.push(*seat);
                }
            }
            PlayerSelector::Resolving(role) | PlayerSelector::Group(AffectedPlayers::One(role)) => {
                if let Ok(seat) = self.resolving_player_at(*role, cursor) {
                    out.push(seat);
                }
            }
            PlayerSelector::Group(AffectedPlayers::EachPlayer) => {
                out.extend(self.window.participants().iter().copied());
            }
            PlayerSelector::Group(AffectedPlayers::EachOtherPlayer) => {
                out.extend(
                    self.window
                        .participants()
                        .iter()
                        .copied()
                        .filter(|&s| s != controller),
                );
            }
            PlayerSelector::Group(AffectedPlayers::EachOpponent) => {
                out.extend(
                    self.combat_rules
                        .opponents(self.window.participants(), controller)?,
                );
            }
            PlayerSelector::Bound(binding) => {
                if let Some(cur) = cursor {
                    match cur.env.get(*binding) {
                        Some(ExecValue::Players(seats)) => out.extend(seats.iter().copied()),
                        Some(ExecValue::Objects(ids)) => {
                            let eval = self.evaluated_characteristics()?;
                            for id in ids {
                                if let Some(seat) = eval
                                    .get(*id)
                                    .and_then(|c| c.controller)
                                    .or_else(|| self.window.cards().get(*id).map(GameObject::owner))
                                {
                                    out.push(seat);
                                }
                            }
                        }
                        Some(ExecValue::ObjectSnapshots(snaps)) => {
                            for snap in snaps {
                                out.push(snap.controller);
                            }
                        }
                        _ => {}
                    }
                }
            }
            PlayerSelector::OwnersOf(query) => {
                for id in self.eval_object_query_at(controller, entry, query, cursor)? {
                    if let Some(obj) = self.window.cards().get(id) {
                        out.push(obj.owner());
                    }
                }
            }
            PlayerSelector::ControllersOf(query) => {
                let eval = self.evaluated_characteristics()?;
                for id in self.eval_object_query_at(controller, entry, query, cursor)? {
                    if let Some(seat) = eval
                        .get(id)
                        .and_then(|c| c.controller)
                        .or_else(|| self.window.cards().get(id).map(GameObject::owner))
                    {
                        out.push(seat);
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        out.retain(|seat| seen.insert(*seat));
        Ok(out)
    }

    pub(super) fn collect_zone_objects(
        &self,
        controller: SeatId,
        entry: Option<&StackEntry>,
        zone: &QueryZone,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<Vec<ObjectId>, EffectError> {
        let cards = self.window.cards();
        match zone {
            QueryZone::Battlefield => Ok(cards
                .iter()
                .filter(|obj| obj.zone() == ObjectZone::Battlefield)
                .map(GameObject::id)
                .collect()),
            QueryZone::BattlefieldControlledBy(players) => {
                let seats: BTreeSet<SeatId> = self
                    .eval_player_selector_at(controller, entry, players, cursor)?
                    .into_iter()
                    .collect();
                let eval = self.evaluated_characteristics()?;
                Ok(cards
                    .iter()
                    .filter(|obj| {
                        obj.zone() == ObjectZone::Battlefield
                            && eval
                                .get(obj.id())
                                .and_then(|c| c.controller)
                                .or_else(|| obj.controller())
                                .is_some_and(|c| seats.contains(&c))
                    })
                    .map(GameObject::id)
                    .collect())
            }
            QueryZone::Exile => Ok(cards
                .iter()
                .filter(|obj| obj.zone() == ObjectZone::Exile)
                .map(GameObject::id)
                .collect()),
            QueryZone::ExileOwnedBy(players) => {
                let seats: BTreeSet<SeatId> = self
                    .eval_player_selector_at(controller, entry, players, cursor)?
                    .into_iter()
                    .collect();
                Ok(cards
                    .iter()
                    .filter(|obj| obj.zone() == ObjectZone::Exile && seats.contains(&obj.owner()))
                    .map(GameObject::id)
                    .collect())
            }
            QueryZone::Stack => Ok(cards
                .iter()
                .filter(|obj| obj.zone() == ObjectZone::Stack)
                .map(GameObject::id)
                .collect()),
            QueryZone::Command => Ok(cards
                .iter()
                .filter(|obj| obj.zone() == ObjectZone::Command)
                .map(GameObject::id)
                .collect()),
            QueryZone::AllZones => Ok(cards.iter().map(GameObject::id).collect()),
            QueryZone::Hand(players) => {
                let seats: BTreeSet<SeatId> = self
                    .eval_player_selector_at(controller, entry, players, cursor)?
                    .into_iter()
                    .collect();
                Ok(cards
                    .iter()
                    .filter(|obj| {
                        matches!(obj.zone(), ObjectZone::Hand(_)) && seats.contains(&obj.owner())
                    })
                    .map(GameObject::id)
                    .collect())
            }
            QueryZone::Library(players) => {
                let seats = self.eval_player_selector_at(controller, entry, players, cursor)?;
                let mut out = Vec::new();
                for seat in seats {
                    out.extend(cards.library(seat).iter().copied());
                }
                Ok(out)
            }
            QueryZone::Graveyard(players) => {
                let seats = self.eval_player_selector_at(controller, entry, players, cursor)?;
                let mut out = Vec::new();
                for seat in seats {
                    out.extend(cards.graveyard(seat).iter().copied());
                }
                Ok(out)
            }
        }
    }

    pub(in crate::effects) fn object_passes_filter(
        &self,
        id: ObjectId,
        filter: ObjectFilter,
        controller: SeatId,
        source_id: Option<ObjectId>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        let Some(chars) = evaluated.get(id) else {
            return false;
        };
        let excluded = BTreeSet::new();
        self.matches_object_filter(filter, chars, controller, source_id, &excluded, evaluated)
    }

    /// Evaluate a composable [`ObjectQuery`] into an ordered, deduplicated list of `ObjectId`s.
    pub(in crate::effects) fn eval_object_query_at(
        &self,
        controller: SeatId,
        entry: Option<&StackEntry>,
        query: &ObjectQuery,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<Vec<ObjectId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let source_id = self.entry_source_object(entry);
        let mut out = match query {
            ObjectQuery::ZoneScan {
                zone,
                filter,
                slice,
            } => match (zone, slice) {
                (QueryZone::Library(players), QuerySlice::Top(expr)) => {
                    let count = self
                        .resolve_value_expr_with_source(controller, expr, cursor, source_id)?
                        .to_usize()
                        .unwrap_or(0);
                    let seats = self.eval_player_selector_at(controller, entry, players, cursor)?;
                    let cards = self.window.cards();
                    let mut collected = Vec::new();
                    for seat in seats {
                        for &id in cards.library(seat).iter().take(count) {
                            if self.object_passes_filter(
                                id, **filter, controller, source_id, &evaluated,
                            ) {
                                collected.push(id);
                            }
                        }
                    }
                    collected
                }
                (QueryZone::Library(players), QuerySlice::Bottom(expr)) => {
                    let count = self
                        .resolve_value_expr_with_source(controller, expr, cursor, source_id)?
                        .to_usize()
                        .unwrap_or(0);
                    let seats = self.eval_player_selector_at(controller, entry, players, cursor)?;
                    let cards = self.window.cards();
                    let mut collected = Vec::new();
                    for seat in seats {
                        let lib = cards.library(seat);
                        let skip = lib.len().saturating_sub(count);
                        for &id in lib.iter().skip(skip) {
                            if self.object_passes_filter(
                                id, **filter, controller, source_id, &evaluated,
                            ) {
                                collected.push(id);
                            }
                        }
                    }
                    collected
                }
                (
                    QueryZone::Library(players),
                    QuerySlice::UntilMatch {
                        filter: match_filter,
                        count,
                        include_non_matches,
                    },
                ) => {
                    let need = self
                        .resolve_value_expr_with_source(controller, count, cursor, source_id)?
                        .to_usize()
                        .unwrap_or(0);
                    let seats = self.eval_player_selector_at(controller, entry, players, cursor)?;
                    let cards = self.window.cards();
                    let mut collected = Vec::new();
                    for seat in seats {
                        let mut matched = 0usize;
                        for &id in cards.library(seat) {
                            if matched >= need {
                                break;
                            }
                            let is_hit = self.object_passes_filter(
                                id,
                                **match_filter,
                                controller,
                                source_id,
                                &evaluated,
                            );
                            if is_hit {
                                matched += 1;
                                if self.object_passes_filter(
                                    id, **filter, controller, source_id, &evaluated,
                                ) {
                                    collected.push(id);
                                }
                            } else if *include_non_matches
                                && self.object_passes_filter(
                                    id, **filter, controller, source_id, &evaluated,
                                )
                            {
                                collected.push(id);
                            }
                        }
                    }
                    collected
                }
                (_, slice) => {
                    let raw = self.collect_zone_objects(controller, entry, zone, cursor)?;
                    match slice {
                        QuerySlice::All => raw
                            .into_iter()
                            .filter(|&id| {
                                self.object_passes_filter(
                                    id, **filter, controller, source_id, &evaluated,
                                )
                            })
                            .collect(),
                        QuerySlice::Top(expr) => {
                            let count = self
                                .resolve_value_expr_with_source(
                                    controller, expr, cursor, source_id,
                                )?
                                .to_usize()
                                .unwrap_or(0);
                            raw.into_iter()
                                .take(count)
                                .filter(|&id| {
                                    self.object_passes_filter(
                                        id, **filter, controller, source_id, &evaluated,
                                    )
                                })
                                .collect()
                        }
                        QuerySlice::Bottom(expr) => {
                            let count = self
                                .resolve_value_expr_with_source(
                                    controller, expr, cursor, source_id,
                                )?
                                .to_usize()
                                .unwrap_or(0);
                            let skip = raw.len().saturating_sub(count);
                            raw.into_iter()
                                .skip(skip)
                                .filter(|&id| {
                                    self.object_passes_filter(
                                        id, **filter, controller, source_id, &evaluated,
                                    )
                                })
                                .collect()
                        }
                        QuerySlice::UntilMatch {
                            filter: match_filter,
                            count,
                            include_non_matches,
                        } => {
                            let need = self
                                .resolve_value_expr_with_source(
                                    controller, count, cursor, source_id,
                                )?
                                .to_usize()
                                .unwrap_or(0);
                            let mut matched = 0usize;
                            let mut collected = Vec::new();
                            for id in raw {
                                if matched >= need {
                                    break;
                                }
                                let is_hit = self.object_passes_filter(
                                    id,
                                    **match_filter,
                                    controller,
                                    source_id,
                                    &evaluated,
                                );
                                if is_hit {
                                    matched += 1;
                                    if self.object_passes_filter(
                                        id, **filter, controller, source_id, &evaluated,
                                    ) {
                                        collected.push(id);
                                    }
                                } else if *include_non_matches
                                    && self.object_passes_filter(
                                        id, **filter, controller, source_id, &evaluated,
                                    )
                                {
                                    collected.push(id);
                                }
                            }
                            collected
                        }
                    }
                }
            },
            ObjectQuery::Empty => Vec::new(),
            ObjectQuery::Fixed(ids) => ids
                .iter()
                .copied()
                .filter(|id| self.window.cards().get(*id).is_some())
                .collect(),
            ObjectQuery::Take { source, count } => {
                let n = self
                    .resolve_value_expr_with_source(controller, count, cursor, source_id)?
                    .to_usize()
                    .unwrap_or(0);
                self.eval_object_query_at(controller, entry, source, cursor)?
                    .into_iter()
                    .take(n)
                    .collect()
            }
            ObjectQuery::Filtered { source, filter } => self
                .eval_object_query_at(controller, entry, source, cursor)?
                .into_iter()
                .filter(|&id| {
                    self.object_passes_filter(id, **filter, controller, source_id, &evaluated)
                })
                .collect(),
            ObjectQuery::LastMoved => cursor
                .map(|c| {
                    if !c.results.last_moved_objects.is_empty() {
                        c.results.last_moved_objects.clone()
                    } else {
                        c.results.returned_target_permanent.into_iter().collect()
                    }
                })
                .unwrap_or_default(),
            ObjectQuery::DestroyedThisWay => cursor
                .map(|c| c.results.destroyed_cards_to_graveyard.clone())
                .unwrap_or_default(),
            ObjectQuery::CreatedTokens => cursor
                .map(|c| c.last_created_tokens.clone())
                .unwrap_or_default(),
            ObjectQuery::Revealed => cursor
                .and_then(|c| c.results.revealed_top_card)
                .or_else(|| self.window.cards().library(controller).first().copied())
                .into_iter()
                .collect(),
            ObjectQuery::LinkedTarget { index } => self
                .legal_linked_targets()
                .ok()
                .flatten()
                .map(|(a, b)| if *index == 0 { a } else { b })
                .into_iter()
                .collect(),
            ObjectQuery::LinkedExile { group } => {
                let mut out = Vec::new();
                if let Some(src) = source_id {
                    match group {
                        None => {
                            if let Some(list) = self.characteristics.linked_exiles.get(&src) {
                                out.extend(list.iter().copied().filter(|id| {
                                    self.window
                                        .cards()
                                        .get(*id)
                                        .is_some_and(|c| c.zone() == ObjectZone::Exile)
                                }));
                            }
                        }
                        Some(g) => {
                            if let Some(list) = self.independent_exile_links.get(&(src, *g)) {
                                out.extend(list.iter().copied().filter(|id| {
                                    self.window
                                        .cards()
                                        .get(*id)
                                        .is_some_and(|c| c.zone() == ObjectZone::Exile)
                                }));
                            }
                        }
                    }
                }
                out
            }
            ObjectQuery::EffectSubject(subject) => {
                if let Some(e) = entry.or_else(|| self.window.resolving()) {
                    self.effect_group(e, **subject)?.into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            ObjectQuery::Bound(binding) => cursor
                .map(|c| c.env.objects(*binding).to_vec())
                .unwrap_or_default(),
            ObjectQuery::BoundFiltered { binding, filter } => cursor
                .map(|c| {
                    c.env
                        .objects(*binding)
                        .iter()
                        .copied()
                        .filter(|&id| {
                            self.object_passes_filter(
                                id, **filter, controller, source_id, &evaluated,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            ObjectQuery::Targets => {
                if let Some(assessment) = self.window.target_assessment()
                    && (entry.is_none() || self.window.resolving() == entry)
                {
                    let mode = cursor.and_then(|c| self.instruction_mode(c.entry, c.instruction));
                    assessment
                        .results()
                        .iter()
                        .filter(|row| {
                            row.legality == crate::targets::TargetLegality::Legal
                                && mode
                                    .is_none_or(|m| m.targets().any(|(id, _)| id == row.choice.id))
                        })
                        .filter_map(|row| match row.choice.target {
                            TargetRef::Object(id) if self.window.cards().get(id).is_some() => {
                                Some(id)
                            }
                            _ => None,
                        })
                        .collect()
                } else {
                    entry
                        .or_else(|| self.window.resolving())
                        .map(|e| {
                            e.targets()
                                .targets()
                                .iter()
                                .filter_map(|t| match t.choice().target {
                                    TargetRef::Object(id)
                                        if self.window.cards().get(id).is_some() =>
                                    {
                                        Some(id)
                                    }
                                    _ => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                }
            }
            ObjectQuery::TargetsWithPairedPartner => {
                let mut targets: Vec<ObjectId> = if let Some(assessment) =
                    self.window.target_assessment()
                    && (entry.is_none() || self.window.resolving() == entry)
                {
                    let mode = cursor.and_then(|c| self.instruction_mode(c.entry, c.instruction));
                    assessment
                        .results()
                        .iter()
                        .filter(|row| {
                            row.legality == crate::targets::TargetLegality::Legal
                                && mode
                                    .is_none_or(|m| m.targets().any(|(id, _)| id == row.choice.id))
                        })
                        .filter_map(|row| match row.choice.target {
                            TargetRef::Object(id) if self.window.cards().get(id).is_some() => {
                                Some(id)
                            }
                            _ => None,
                        })
                        .collect()
                } else {
                    entry
                        .or_else(|| self.window.resolving())
                        .map(|e| {
                            e.targets()
                                .targets()
                                .iter()
                                .filter_map(|t| match t.choice().target {
                                    TargetRef::Object(id)
                                        if self.window.cards().get(id).is_some() =>
                                    {
                                        Some(id)
                                    }
                                    _ => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let partners: Vec<ObjectId> = targets
                    .iter()
                    .filter_map(|t| self.characteristics.paired_creatures.get(t).copied())
                    .filter(|p| self.window.cards().get(*p).is_some())
                    .collect();
                targets.extend(partners);
                targets
            }
            ObjectQuery::Source => source_id
                .filter(|id| self.window.cards().get(*id).is_some())
                .into_iter()
                .collect(),
            ObjectQuery::TriggerSubject => entry
                .or_else(|| self.window.resolving())
                .map(StackEntry::id)
                .and_then(|eid| self.execution_bindings(eid))
                .and_then(|b| b.trigger.as_ref())
                .map(|t| {
                    t.causes
                        .members()
                        .iter()
                        .filter_map(|cause| cause.subject_object_id())
                        .collect()
                })
                .unwrap_or_default(),
            ObjectQuery::AttachedHost => source_id
                .and_then(|src| self.attachment_host_for_ability(src))
                .and_then(|target| match target {
                    TargetRef::Object(host) => Some(host),
                    _ => None,
                })
                .into_iter()
                .collect(),
            ObjectQuery::Tagged {
                key,
                value,
                zone,
                filter,
            } => {
                let mut tagged = self.object_tags.objects_with_tag(key, value.as_ref());
                if let Some(z) = zone {
                    let allowed: BTreeSet<ObjectId> = self
                        .collect_zone_objects(controller, entry, z, cursor)?
                        .into_iter()
                        .collect();
                    tagged.retain(|id| allowed.contains(id));
                }
                if let Some(f) = filter {
                    tagged.retain(|&id| {
                        self.object_passes_filter(id, **f, controller, source_id, &evaluated)
                    });
                }
                tagged
            }
            ObjectQuery::Union(qs) => {
                let mut acc = Vec::new();
                for q in qs {
                    acc.extend(self.eval_object_query_at(controller, entry, q, cursor)?);
                }
                acc
            }
            ObjectQuery::Difference(left, right) => {
                let lhs = self.eval_object_query_at(controller, entry, left, cursor)?;
                let rhs: BTreeSet<ObjectId> = self
                    .eval_object_query_at(controller, entry, right, cursor)?
                    .into_iter()
                    .collect();
                lhs.into_iter().filter(|id| !rhs.contains(id)).collect()
            }
            ObjectQuery::Intersect(left, right) => {
                let lhs = self.eval_object_query_at(controller, entry, left, cursor)?;
                let rhs: BTreeSet<ObjectId> = self
                    .eval_object_query_at(controller, entry, right, cursor)?
                    .into_iter()
                    .collect();
                lhs.into_iter().filter(|id| rhs.contains(id)).collect()
            }
        };
        let mut seen = BTreeSet::new();
        out.retain(|id| seen.insert(*id));
        Ok(out)
    }

    pub(super) fn eval_state_track_keys(
        &self,
        controller: SeatId,
        entry: Option<&StackEntry>,
        spec: &StateTrackKeySpec,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<Vec<StateTrackKey>, EffectError> {
        match &spec.scope {
            StateTrackScope::Global => Ok(vec![StateTrackKey::Global(spec.name.clone())]),
            StateTrackScope::Player(sel) => Ok(self
                .eval_player_selector_at(controller, entry, sel, cursor)?
                .into_iter()
                .map(|seat| StateTrackKey::Player(seat, spec.name.clone()))
                .collect()),
            StateTrackScope::Object(query) => Ok(self
                .eval_object_query_at(controller, entry, query, cursor)?
                .into_iter()
                .map(|id| StateTrackKey::Object(id, spec.name.clone()))
                .collect()),
        }
    }

    /// Evaluate a universal [`ConditionExpr`] against live game state and `ExecEnv`.
    pub(super) fn eval_condition_expr_at(
        &self,
        controller: SeatId,
        entry: Option<&StackEntry>,
        cond: &ConditionExpr,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<bool, EffectError> {
        let source_id = self.entry_source_object(entry);
        match cond {
            ConditionExpr::Always(v) => Ok(*v),
            ConditionExpr::Compare { left, op, right } => {
                let lhs =
                    self.resolve_value_expr_with_source(controller, left, cursor, source_id)?;
                let rhs =
                    self.resolve_value_expr_with_source(controller, right, cursor, source_id)?;
                Ok(compare_values(&lhs, *op, &rhs))
            }
            ConditionExpr::BoundBool(binding) => Ok(cursor
                .and_then(|c| c.env.bool_flag(*binding))
                .unwrap_or(false)),
            ConditionExpr::NonEmpty(query) => Ok(!self
                .eval_object_query_at(controller, entry, query, cursor)?
                .is_empty()),
            ConditionExpr::AllMatch {
                query,
                filter,
                require_non_empty,
            } => {
                let ids = self.eval_object_query_at(controller, entry, query, cursor)?;
                if ids.is_empty() {
                    return Ok(!require_non_empty);
                }
                let evaluated = self.evaluated_characteristics()?;
                Ok(ids.into_iter().all(|id| {
                    self.object_passes_filter(id, **filter, controller, source_id, &evaluated)
                }))
            }
            ConditionExpr::AnyMatch { query, filter } => {
                let ids = self.eval_object_query_at(controller, entry, query, cursor)?;
                let evaluated = self.evaluated_characteristics()?;
                Ok(ids.into_iter().any(|id| {
                    self.object_passes_filter(id, **filter, controller, source_id, &evaluated)
                }))
            }
            ConditionExpr::HasTag { query, key, value } => {
                let ids = self.eval_object_query_at(controller, entry, query, cursor)?;
                Ok(ids
                    .into_iter()
                    .any(|id| self.object_tags.has(id, key, value.as_ref())))
            }
            ConditionExpr::And(cs) => {
                for c in cs {
                    if !self.eval_condition_expr_at(controller, entry, c, cursor)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ConditionExpr::Or(cs) => {
                for c in cs {
                    if self.eval_condition_expr_at(controller, entry, c, cursor)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ConditionExpr::Not(c) => {
                Ok(!self.eval_condition_expr_at(controller, entry, c, cursor)?)
            }
            ConditionExpr::Resolution(res_cond) => {
                let lhs = self.resolve_value_expr_with_source(
                    controller,
                    &res_cond.left,
                    cursor,
                    source_id,
                )?;
                let rhs = self.resolve_value_expr_with_source(
                    controller,
                    &res_cond.right,
                    cursor,
                    source_id,
                )?;
                Ok(res_cond.comparison.evaluate(&lhs, &rhs))
            }
        }
    }

    /// Evaluate toolbox-backed [`ValueExpr`] variants.
    pub(in crate::effects) fn resolve_toolbox_value_expr(
        &self,
        controller: SeatId,
        expr: &ValueExpr,
        cursor: Option<&ResolutionCursor>,
        explicit_source: Option<ObjectId>,
    ) -> Result<Option<RulesInteger>, EffectError> {
        let entry = self.window.resolving();
        match expr {
            ValueExpr::BoundInt(binding) => Ok(Some(
                cursor
                    .and_then(|c| c.env.int(*binding))
                    .cloned()
                    .unwrap_or_else(|| RulesInteger::from(0)),
            )),
            ValueExpr::BoundCount { binding, filter } => {
                let Some(cur) = cursor else {
                    return Ok(Some(RulesInteger::from(0)));
                };
                let count = match cur.env.get(*binding) {
                    Some(ExecValue::Objects(ids)) => match filter {
                        None => ids.len(),
                        Some(f) => {
                            let eval = self.evaluated_characteristics()?;
                            let src = explicit_source.or_else(|| self.entry_source_object(entry));
                            ids.iter()
                                .filter(|&&id| {
                                    self.object_passes_filter(id, **f, controller, src, &eval)
                                })
                                .count()
                        }
                    },
                    Some(ExecValue::ObjectSnapshots(snaps)) => match filter {
                        None => snaps.len(),
                        Some(f) => snaps
                            .iter()
                            .filter(|s| f.matches_card_types_only(&s.card_types))
                            .count(),
                    },
                    Some(ExecValue::Players(seats)) => seats.len(),
                    Some(ExecValue::Piles(piles)) => piles.len(),
                    _ => 0,
                };
                Ok(Some(RulesInteger::from_usize(count)))
            }
            ValueExpr::BoundStat { binding, stat, op } => {
                let Some(cur) = cursor else {
                    return Ok(Some(RulesInteger::from(0)));
                };
                let values: Vec<RulesInteger> = match cur.env.get(*binding) {
                    Some(ExecValue::ObjectSnapshots(snaps)) => snaps
                        .iter()
                        .filter_map(|s| snapshot_stat(s, *stat))
                        .collect(),
                    Some(ExecValue::Objects(ids)) => {
                        let eval = self.evaluated_characteristics()?;
                        ids.iter()
                            .filter_map(|id| eval.get(*id).and_then(|c| live_stat(c, *stat)))
                            .collect()
                    }
                    _ => Vec::new(),
                };
                Ok(Some(aggregate_stats(values, *op)))
            }
            ValueExpr::QueryCount(query) => {
                let ids = self.eval_object_query_at(controller, entry, query, cursor)?;
                Ok(Some(RulesInteger::from_usize(ids.len())))
            }
            ValueExpr::QueryAggregate { query, stat, op } => {
                let ids = self.eval_object_query_at(controller, entry, query, cursor)?;
                let eval = self.evaluated_characteristics()?;
                let values: Vec<RulesInteger> = ids
                    .into_iter()
                    .filter_map(|id| eval.get(id).and_then(|c| live_stat(c, *stat)))
                    .collect();
                Ok(Some(aggregate_stats(values, *op)))
            }
            ValueExpr::StateTrackInt(spec) => {
                let keys = self.eval_state_track_keys(controller, entry, spec, cursor)?;
                let total = keys.into_iter().fold(RulesInteger::from(0), |acc, k| {
                    acc.plus(&self.state_tracks.int_or_zero(&k))
                });
                Ok(Some(total))
            }
            ValueExpr::ChoiceHistoryCount {
                category,
                chooser,
                this_turn_only,
            } => {
                let seats: Option<BTreeSet<SeatId>> = match chooser {
                    Some(sel) => Some(
                        self.eval_player_selector_at(controller, entry, sel, cursor)?
                            .into_iter()
                            .collect(),
                    ),
                    None => None,
                };
                let count = self
                    .choice_history
                    .iter()
                    .filter(|rec| {
                        (!*this_turn_only || rec.turn == self.current_turn_number)
                            && category.is_none_or(|c| rec.category == c)
                            && seats.as_ref().is_none_or(|s| s.contains(&rec.chooser))
                    })
                    .count();
                Ok(Some(RulesInteger::from_usize(count)))
            }
            _ => Ok(None),
        }
    }
}
