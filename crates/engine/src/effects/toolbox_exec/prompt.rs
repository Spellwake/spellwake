//! `PromptChoice` domain resolution, reply application, choice history, and `ChoiceMade` events.

use crate::{
    characteristics::Color,
    effects::{EffectError, EffectWindow},
    land::LandType,
    mana::ManaType,
    object_identity::ObjectId,
    occurrence::{EventCause, RulesJournal, RulesOccurrence},
    priority::SeatId,
    program::{CardType, ColorMask, PlaneswalkerSubtype},
    stack_window::StackEntry,
    toolbox::{
        ChoiceCategory, ChoiceRecord, ExecValue, PromptChoiceReply, PromptChoiceSpec, PromptDomain,
        ResolvedPromptDomain, SelectionConstraint, SubtypeChoiceKind, TagValue,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

fn color_mask_to_mana_set(mask: ColorMask) -> BTreeSet<ManaType> {
    let effective = if mask.is_empty() {
        ColorMask::ALL
    } else {
        mask
    };
    let mut out = BTreeSet::new();
    if effective.contains(Color::White) {
        out.insert(ManaType::White);
    }
    if effective.contains(Color::Blue) {
        out.insert(ManaType::Blue);
    }
    if effective.contains(Color::Black) {
        out.insert(ManaType::Black);
    }
    if effective.contains(Color::Red) {
        out.insert(ManaType::Red);
    }
    if effective.contains(Color::Green) {
        out.insert(ManaType::Green);
    }
    out
}

fn mana_to_color_mask(mana: ManaType) -> ColorMask {
    match mana {
        ManaType::White => ColorMask::WHITE,
        ManaType::Blue => ColorMask::BLUE,
        ManaType::Black => ColorMask::BLACK,
        ManaType::Red => ColorMask::RED,
        ManaType::Green => ColorMask::GREEN,
        ManaType::Colourless => ColorMask::EMPTY,
    }
}

fn try_match_card_type(
    obj_idx: usize,
    type_sets: &[&BTreeSet<CardType>],
    visited: &mut BTreeSet<CardType>,
    matched: &mut BTreeMap<CardType, usize>,
) -> bool {
    let Some(types) = type_sets.get(obj_idx) else {
        return false;
    };
    for &card_type in *types {
        if !visited.insert(card_type) {
            continue;
        }
        let can_claim = match matched.get(&card_type).copied() {
            None => true,
            Some(prev_idx) => try_match_card_type(prev_idx, type_sets, visited, matched),
        };
        if can_claim {
            matched.insert(card_type, obj_idx);
            return true;
        }
    }
    false
}

pub(super) struct ChoiceEventMeta {
    pub(super) chooser: SeatId,
    pub(super) category: ChoiceCategory,
    pub(super) option_indices: Vec<u8>,
    pub(super) chosen_count: usize,
}

impl EffectWindow {
    pub(super) fn satisfies_selection_constraint(
        &self,
        ids: &[ObjectId],
        constraint: SelectionConstraint,
    ) -> Result<bool, EffectError> {
        if ids.len() <= 1 {
            return Ok(true);
        }
        let evaluated = self.evaluated_characteristics()?;
        let mut objects = Vec::with_capacity(ids.len());
        for &id in ids {
            let Some(obj) = evaluated.get(id) else {
                return Ok(false);
            };
            objects.push(obj);
        }
        Ok(match constraint {
            SelectionConstraint::DistinctNames => {
                for (i, left) in objects.iter().enumerate() {
                    for right in &objects[i + 1..] {
                        if self
                            .name_rules
                            .shares_name(&left.current_names, &right.current_names)
                        {
                            return Ok(false);
                        }
                    }
                }
                true
            }
            SelectionConstraint::DistinctManaValues => {
                let mut seen = BTreeSet::new();
                for obj in &objects {
                    let mv = self
                        .permanent_mana_value(obj.object)
                        .unwrap_or_else(|| obj.defined.mana_value.clone());
                    if !seen.insert(mv) {
                        return Ok(false);
                    }
                }
                true
            }
            SelectionConstraint::DistinctPowers => {
                let mut seen = BTreeSet::new();
                for obj in &objects {
                    let Some(power) = obj
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone())
                        .or_else(|| {
                            obj.defined
                                .printed_pt
                                .as_ref()
                                .map(|pt| pt.baseline().power)
                        })
                    else {
                        return Ok(false);
                    };
                    if !seen.insert(power) {
                        return Ok(false);
                    }
                }
                true
            }
            SelectionConstraint::SharedCardType => {
                let mut shared = objects[0].current_types.clone();
                for obj in &objects[1..] {
                    shared.retain(|ty| obj.current_types.contains(ty));
                    if shared.is_empty() {
                        return Ok(false);
                    }
                }
                !shared.is_empty()
            }
            SelectionConstraint::DistinctCardTypes => {
                let type_sets: Vec<&BTreeSet<CardType>> =
                    objects.iter().map(|obj| &obj.current_types).collect();
                let mut matched = BTreeMap::new();
                for (obj_idx, types) in type_sets.iter().enumerate() {
                    if types.is_empty() {
                        return Ok(false);
                    }
                    let mut visited = BTreeSet::new();
                    if !try_match_card_type(obj_idx, &type_sets, &mut visited, &mut matched) {
                        return Ok(false);
                    }
                }
                true
            }
        })
    }

    pub(super) fn record_choice_and_emit(
        &mut self,
        entry: &StackEntry,
        index: usize,
        meta: ChoiceEventMeta,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let source = self.entry_source_object(Some(entry));
        self.choice_history.push(ChoiceRecord {
            turn: self.current_turn_number,
            chooser: meta.chooser,
            source,
            category: meta.category,
            option_indices: meta.option_indices.clone(),
            chosen_count: meta.chosen_count,
        });
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::ChoiceMade {
                    chooser: meta.chooser,
                    source,
                    category: meta.category,
                    option_indices: meta.option_indices.clone(),
                    chosen_count: meta.chosen_count,
                }])
            },
        )
    }

    pub(super) fn resolve_prompt_domain(
        &self,
        entry: &StackEntry,
        domain: &PromptDomain,
    ) -> Result<ResolvedPromptDomain, EffectError> {
        let controller = entry.controller();
        let cursor = self.resolution_cursor();
        let source_id = self.entry_source_object(Some(entry));
        Ok(match domain {
            PromptDomain::Objects {
                query,
                min,
                max,
                hidden,
                ..
            } => {
                let candidates =
                    self.eval_object_query_at(controller, Some(entry), query, cursor)?;
                let raw_min = self
                    .resolve_value_expr_with_source(controller, min, cursor, source_id)?
                    .to_usize()
                    .unwrap_or(0);
                let raw_max = self
                    .resolve_value_expr_with_source(controller, max, cursor, source_id)?
                    .to_usize()
                    .unwrap_or(candidates.len());
                let clamped_max = raw_max.min(candidates.len());
                let clamped_min = raw_min.min(clamped_max);
                ResolvedPromptDomain::Objects {
                    candidates,
                    min: clamped_min,
                    max: clamped_max,
                    hidden: *hidden,
                }
            }
            PromptDomain::Players { selector, min, max } => {
                let candidates =
                    self.eval_player_selector_at(controller, Some(entry), selector, cursor)?;
                let cap = u8::try_from(candidates.len()).unwrap_or(u8::MAX);
                let clamped_max = (*max).min(cap);
                let clamped_min = (*min).min(clamped_max);
                ResolvedPromptDomain::Players {
                    candidates,
                    min: clamped_min,
                    max: clamped_max,
                }
            }
            PromptDomain::Options {
                labels,
                min,
                max,
                allow_repeat,
            } => ResolvedPromptDomain::Options {
                labels: labels.clone(),
                min: *min,
                max: *max,
                allow_repeat: *allow_repeat,
            },
            PromptDomain::Number { min, max } => {
                let lo = self.resolve_value_expr_with_source(controller, min, cursor, source_id)?;
                let hi = self.resolve_value_expr_with_source(controller, max, cursor, source_id)?;
                let hi = if hi < lo { lo.clone() } else { hi };
                ResolvedPromptDomain::Number { min: lo, max: hi }
            }
            PromptDomain::Color { allowed } => ResolvedPromptDomain::Color {
                allowed: color_mask_to_mana_set(*allowed),
            },
            PromptDomain::CardType { allowed } => {
                let types = if allowed.is_empty() {
                    vec![
                        CardType::Artifact,
                        CardType::Battle,
                        CardType::Creature,
                        CardType::Enchantment,
                        CardType::Instant,
                        CardType::Kindred,
                        CardType::Land,
                        CardType::Planeswalker,
                        CardType::Sorcery,
                    ]
                } else {
                    allowed.clone()
                };
                ResolvedPromptDomain::CardType { allowed: types }
            }
            PromptDomain::Subtype {
                kind, excluding, ..
            } => match kind {
                SubtypeChoiceKind::Creature => ResolvedPromptDomain::CreatureType {
                    allowed: self
                        .creature_vocabulary
                        .members()
                        .iter()
                        .filter_map(|m| self.creature_vocabulary.resolve_label(m))
                        .filter(|id| !excluding.contains(id))
                        .collect(),
                },
                SubtypeChoiceKind::BasicLand => ResolvedPromptDomain::BasicLandType {
                    allowed: vec![
                        LandType::Plains,
                        LandType::Island,
                        LandType::Swamp,
                        LandType::Mountain,
                        LandType::Forest,
                    ],
                },
                SubtypeChoiceKind::Planeswalker => ResolvedPromptDomain::PlaneswalkerType {
                    allowed: PlaneswalkerSubtype::ALL.to_vec(),
                },
            },
            PromptDomain::CardName { nonland_only } => ResolvedPromptDomain::CardName {
                nonland_only: *nonland_only,
            },
            PromptDomain::Partition {
                query,
                pile_count,
                min_per_pile,
            } => {
                let candidates =
                    self.eval_object_query_at(controller, Some(entry), query, cursor)?;
                ResolvedPromptDomain::Partition {
                    candidates,
                    pile_count: *pile_count,
                    min_per_pile: *min_per_pile,
                }
            }
        })
    }

    pub(super) fn trivial_prompt_reply(domain: &ResolvedPromptDomain) -> Option<PromptChoiceReply> {
        match domain {
            ResolvedPromptDomain::Objects {
                candidates, min, ..
            } if candidates.is_empty() && *min == 0 => Some(PromptChoiceReply::Objects(Vec::new())),
            ResolvedPromptDomain::Objects {
                candidates,
                min,
                max,
                ..
            } if *min == candidates.len() && *max == candidates.len() => {
                Some(PromptChoiceReply::Objects(candidates.clone()))
            }
            ResolvedPromptDomain::Players {
                candidates, min, ..
            } if candidates.is_empty() && *min == 0 => Some(PromptChoiceReply::Players(Vec::new())),
            ResolvedPromptDomain::Players {
                candidates,
                min,
                max,
            } if usize::from(*min) == candidates.len() && usize::from(*max) == candidates.len() => {
                Some(PromptChoiceReply::Players(candidates.clone()))
            }
            ResolvedPromptDomain::Number { min, max } if min == max => {
                Some(PromptChoiceReply::Number(min.clone()))
            }
            ResolvedPromptDomain::Partition {
                candidates,
                pile_count,
                min_per_pile,
            } if candidates.is_empty() && *min_per_pile == 0 => {
                Some(PromptChoiceReply::Partition(vec![
                    Vec::new();
                    usize::from(*pile_count)
                ]))
            }
            _ => None,
        }
    }

    // Applies a validated PromptChoiceReply into ExecEnv, tags, and ChoiceMade history.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_prompt_reply(
        &mut self,
        entry: &StackEntry,
        index: usize,
        chooser: SeatId,
        spec: &PromptChoiceSpec,
        resolved_domain: &ResolvedPromptDomain,
        reply: PromptChoiceReply,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let mut option_indices = Vec::new();
        let chosen_count: usize;
        match reply {
            PromptChoiceReply::Objects(chosen) => {
                if let PromptDomain::Objects {
                    constraint: Some(constraint),
                    ..
                } = &spec.domain
                    && !self.satisfies_selection_constraint(&chosen, *constraint)?
                {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                chosen_count = chosen.len();
                if let Some(unchosen_slot) = spec.bind_unchosen
                    && let ResolvedPromptDomain::Objects { candidates, .. } = resolved_domain
                {
                    let chosen_set: BTreeSet<ObjectId> = chosen.iter().copied().collect();
                    let unchosen: Vec<ObjectId> = candidates
                        .iter()
                        .copied()
                        .filter(|id| !chosen_set.contains(id))
                        .collect();
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.env.bind(unchosen_slot, ExecValue::Objects(unchosen));
                    }
                }
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.env.bind(spec.bind_to, ExecValue::Objects(chosen));
                }
            }
            PromptChoiceReply::Players(seats) => {
                chosen_count = seats.len();
                if let Some(cur) = self.resolution_cursor_mut() {
                    if let Some(&first) = seats.first() {
                        cur.chosen_opponent = Some(first);
                    }
                    cur.env.bind(spec.bind_to, ExecValue::Players(seats));
                }
            }
            PromptChoiceReply::Options(indices) => {
                chosen_count = indices.len();
                option_indices = indices.clone();
                if let Some(cur) = self.resolution_cursor_mut() {
                    let first = indices.first().copied().unwrap_or(0);
                    cur.results.chosen_option_index = Some(first);
                    cur.env.bind(spec.bind_to, ExecValue::OptionIndex(first));
                }
            }
            PromptChoiceReply::Number(val) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.chosen_number = Some(val.clone());
                    cur.env.bind(spec.bind_to, ExecValue::Int(val));
                }
            }
            PromptChoiceReply::Color(color) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.chosen_effect_color = Some(color);
                    cur.env
                        .bind(spec.bind_to, ExecValue::Colors(mana_to_color_mask(color)));
                }
            }
            PromptChoiceReply::CardType(ty) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.chosen_card_type = Some(ty);
                    cur.env
                        .bind(spec.bind_to, ExecValue::CardTypes(BTreeSet::from([ty])));
                }
            }
            PromptChoiceReply::CreatureType(sub) => {
                if let ResolvedPromptDomain::CreatureType { allowed } = resolved_domain
                    && !allowed.contains(&sub)
                {
                    return Err(EffectError::IllegalCreatureType);
                }
                chosen_count = 1;
                let legacy_slot = match &spec.domain {
                    PromptDomain::Subtype { legacy_slot, .. } => *legacy_slot,
                    _ => None,
                };
                if let Some(slot) = legacy_slot {
                    let before = self.rules_snapshot(None)?;
                    if let Some(cur) = self.resolution_cursor_mut() {
                        if cur.values.insert(slot, sub).is_some() {
                            return Err(EffectError::InconsistentProgram);
                        }
                        cur.env
                            .bind(spec.bind_to, ExecValue::Subtypes(BTreeSet::from([sub])));
                    }
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        append,
                        |_, _, _| {
                            Ok(vec![RulesOccurrence::CreatureTypeChosen {
                                entry: entry.id(),
                                instruction: index,
                                slot,
                                chooser,
                                value: sub,
                            }])
                        },
                    )?;
                } else if let Some(cur) = self.resolution_cursor_mut() {
                    cur.env
                        .bind(spec.bind_to, ExecValue::Subtypes(BTreeSet::from([sub])));
                }
            }
            PromptChoiceReply::BasicLandType(lt) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.chosen_basic_land_type = Some(lt);
                    cur.env
                        .bind(spec.bind_to, ExecValue::CardName(format!("{lt:?}")));
                }
            }
            PromptChoiceReply::PlaneswalkerType(pw) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.env
                        .bind(spec.bind_to, ExecValue::CardName(pw.label().to_owned()));
                }
            }
            PromptChoiceReply::CardName(name) => {
                chosen_count = 1;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.chosen_card_name = Some(name.clone());
                    cur.env.bind(spec.bind_to, ExecValue::CardName(name));
                }
            }
            PromptChoiceReply::Partition(piles) => {
                chosen_count = piles.iter().map(Vec::len).sum();
                if let Some(tag_key) = &spec.tag_partition {
                    for (pile_idx, pile) in piles.iter().enumerate() {
                        let val = TagValue::Int(RulesInteger::from_usize(pile_idx));
                        for &id in pile {
                            self.object_tags.set(id, tag_key.clone(), val.clone());
                        }
                    }
                }
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.env.bind(spec.bind_to, ExecValue::Piles(piles));
                }
            }
        }
        self.record_choice_and_emit(
            entry,
            index,
            ChoiceEventMeta {
                chooser,
                category: spec.category,
                option_indices,
                chosen_count,
            },
            append,
        )
    }
}
