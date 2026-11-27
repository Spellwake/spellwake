use super::{
    Color, ColorOperation, StaticColorOperand, StaticContinuousAbility, ordering, types::*,
};
use crate::{
    object_identity::{GameObjects, ObjectId, ObjectZone},
    priority::SeatId,
    program::{CardType, Program},
};
use spellwake_contracts::{RulesInteger, names::NameRules};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum InstanceOrigin<'a> {
    Emblem(&'a crate::emblem::Emblem),
    EmblemStatic {
        emblem: &'a crate::emblem::Emblem,
        ability: &'a StaticContinuousAbility,
    },
    TemporaryStatic(&'a TemporaryBattlefieldStatic),
    Resolved(&'a ResolvedContinuousEffect),
    Static {
        source: ObjectId,
        ability: &'a StaticContinuousAbility,
    },
}
#[derive(Clone, Copy)]
struct Instance<'a> {
    key: EffectInstanceKey,
    timestamp: CharacteristicTimestamp,
    origin: InstanceOrigin<'a>,
}
impl Instance<'_> {
    fn contains_bucket(&self, bucket: CharacteristicBucket) -> bool {
        match self.origin {
            InstanceOrigin::Emblem(emblem) => {
                matches!(
                    emblem.definition,
                    crate::emblem::EmblemDefinition::ControlledCreatureBonus { .. }
                ) && matches!(
                    bucket,
                    CharacteristicBucket::Abilities | CharacteristicBucket::ModifyPT
                )
            }
            InstanceOrigin::EmblemStatic { ability, .. } => match ability {
                StaticContinuousAbility::Battlefield { parts, .. } => {
                    parts.0.iter().any(|part| part.bucket() == bucket)
                }
                _ => false,
            },
            InstanceOrigin::TemporaryStatic(temp) => match &temp.ability {
                StaticContinuousAbility::Battlefield { parts, .. } => {
                    parts.0.iter().any(|part| part.bucket() == bucket)
                }
                _ => false,
            },
            InstanceOrigin::Resolved(effect) => {
                effect.parts.0.iter().any(|part| part.bucket() == bucket)
            }
            InstanceOrigin::Static { ability, .. } => match ability {
                StaticContinuousAbility::Battlefield { parts, .. } => {
                    parts.0.iter().any(|part| part.bucket() == bucket)
                }
                _ => false,
            },
        }
    }
}
fn defined(program: &Program) -> DefinedCharacteristics {
    DefinedCharacteristics {
        mana_value: program.mana_value(),
        colored_mana_symbols: program.colored_mana_symbols(),
        printed_loyalty: program.printed_loyalty.clone(),
        printed_defense: program.printed_defense.clone(),
        names: program.names.clone(),
        supertypes: program.supertypes.clone(),
        types: program.card_types.clone(),
        creature_subtypes: program.creature_subtypes.clone(),
        land_subtypes: program.land_subtypes.clone(),
        spell_subtypes: program.spell_subtypes.clone(),
        artifact_subtypes: program.artifact_subtypes.clone(),
        enchantment_subtypes: program.enchantment_subtypes.clone(),
        planeswalker_subtypes: program.planeswalker_subtypes.clone(),
        colors: program.defined_colors(),
        printed_pt: program
            .permanent()
            .and_then(|value| value.printed_pt.clone()),
        has_adventure: program.adventure.is_some(),
        has_x_in_cost: program.x_mana_symbols > 0,
    }
}
fn initial_pt(object: ObjectId, defined: &DefinedCharacteristics) -> PowerToughnessEvaluation {
    let baseline = defined
        .printed_pt
        .as_ref()
        .map(PrintedPowerToughness::baseline)
        .unwrap_or_else(PowerToughness::zero);
    PowerToughnessEvaluation {
        object,
        copiable: defined.printed_pt.clone(),
        base: baseline.clone(),
        derived: baseline,
        applied_effects: vec![],
    }
}

/// Evaluate one exact supplied world. No physical owner, resource, timestamp,
/// counter, mark, history, occurrence or persistent cache is mutated.
pub(crate) fn evaluate(
    world: CharacteristicWorld<'_>,
) -> Result<EvaluatedCharacteristics, CharacteristicInvariantError> {
    let mut objects = BTreeMap::new();
    let mut battlefield = BTreeSet::new();
    let mut used_timestamps = BTreeSet::new();
    let mut instances = BTreeMap::new();
    for physical in world.cards.iter() {
        let (program, copy_timestamp) = world
            .state
            .copiable_definition(physical, world.programs)
            .ok_or(CharacteristicInvariantError::MissingDefinition(
            physical.id(),
        ))?;
        let mut defined = defined(program);
        if physical.transformed()
            && physical.face_down().is_none()
            && copy_timestamp.is_none()
            && let Some(front) = physical.card_id().and_then(|id| world.programs.get(&id))
            && front.modal_back.is_none()
        {
            defined.mana_value = front.mana_value();
        }
        if copy_timestamp.is_none()
            && physical.face_down().is_none()
            && let Some(mana_value) = physical.melded_mana_value(world.programs)
        {
            defined.mana_value = mana_value;
        }
        let is_battlefield = physical.zone() == ObjectZone::Battlefield;
        if is_battlefield
            && copy_timestamp.is_none()
            && physical.face_down().is_none()
            && let Some(split) = program.split.as_deref()
            && split.is_room()
        {
            let doors = world
                .state
                .unlocked_doors
                .get(&physical.id())
                .copied()
                .unwrap_or_default();
            match (doors.left, doors.right) {
                (true, true) => {}
                (true, false) => {
                    defined.names = split.left.names.clone();
                    defined.mana_value = split.left.mana_value();
                    defined.colored_mana_symbols = split.left.colored_mana_symbols();
                    defined.colors = split.left.defined_colors();
                }
                (false, true) => {
                    defined.names = split.right.names.clone();
                    defined.mana_value = split.right.mana_value();
                    defined.colored_mana_symbols = split.right.colored_mana_symbols();
                    defined.colors = split.right.defined_colors();
                }
                (false, false) => {
                    defined.names = spellwake_contracts::names::ObjectNames::nameless();
                    defined.mana_value = 0.into();
                    defined.colored_mana_symbols.clear();
                    defined.colors.clear();
                }
            }
        }
        let power_toughness = (defined.types.contains(&CardType::Creature)
            || !is_battlefield && defined.printed_pt.is_some())
        .then(|| initial_pt(physical.id(), &defined));
        let paid = world.state.end_step_casts.get(&physical.id()).copied().filter(|mode| {
            copy_timestamp.is_none() && matches!(program.conditional_cost,Some(crate::program::ConditionalCastCost::EndStep{kind,..}) if *mode==kind)
        });
        let physical_id = physical.id();
        let pt_counters = world.state.counters(physical_id);
        let present_counter_kinds = world.state.present_permanent_counter_kinds(physical_id);
        let has_any_counter = !present_counter_kinds.is_empty();
        let mut object = ObjectCharacteristics {
            paired_with: world.state.paired_creatures.get(&physical_id).copied(),
            tapped: physical.tapped(),
            owner: physical.owner(),
            imprinted_cards: world
                .state
                .imprinted_cards
                .get(&physical_id)
                .cloned()
                .unwrap_or_default(),
            suppressed_keywords: BTreeSet::new(),
            entry_creature_type: world.entry_creature_types.get(&physical.id()).copied(),
            commander_owner: physical
                .card_id()
                .filter(|id| world.state.commander_cards.contains(id))
                .map(|_| physical.owner()),
            attachment_kind: if world.state.bestowed_auras.contains_key(&physical_id)
                || world.state.bestowed_spells.contains(&physical_id)
            {
                Some(crate::attachment::AttachmentKind::Aura(
                    crate::attachment::EnchantRestriction::Creature,
                ))
            } else {
                program
                    .permanent()
                    .and_then(|permanent| permanent.attachment)
            },
            is_token: physical.is_token(),
            attacking: world
                .combat
                .is_some_and(|c| c.attacker(physical.id()).is_some()),
            blocking: world
                .combat
                .is_some_and(|c| c.blocker(physical.id()).is_some()),
            has_plus_one_counter: pt_counters.plus_one.is_positive(),
            has_minus_one_counter: pt_counters.minus_one.is_positive(),
            plus_one_counters: pt_counters.plus_one.clone(),
            has_any_counter,
            present_counter_kinds,
            blessing_counters: world
                .state
                .blessing_counters
                .get(&physical_id)
                .cloned()
                .unwrap_or_else(|| 0.into()),
            charge_counters: world
                .state
                .charge
                .get(&physical.id())
                .cloned()
                .unwrap_or_else(|| 0.into()),
            controller_speed: physical
                .default_controller()
                .or(physical.controller())
                .and_then(|seat| world.state.speed.get(&seat).copied())
                .unwrap_or(0),
            saddled: world.state.saddled.contains(&physical.id()),
            is_face_down: physical.face_down().is_some(),
            entered_this_turn: world.state.entered_objects_this_turn.contains(&physical_id),
            entry_color: world.state.entry_color_choices.get(&physical_id).copied(),
            attacked_this_turn: world
                .state
                .attacked_objects_this_turn
                .contains(&physical_id),
            dealt_damage_this_turn: world
                .state
                .object_damage_taken_this_turn
                .get(&physical_id)
                .is_some_and(RulesInteger::is_positive),
            kicked: world.state.kicked.contains(&physical_id)
                || world
                    .state
                    .kicker_counts
                    .get(&physical_id)
                    .is_some_and(|counts| !counts.is_empty()),
            renowned: world.state.renowned.contains(&physical_id),
            monstrous: world.state.monstrous.contains(&physical_id),
            suspected: world
                .state
                .suspect_counters
                .get(&physical_id)
                .is_some_and(RulesInteger::is_positive),
            solved: world.state.solved_cases.contains(&physical_id),
            exerted: world.state.exerted_objects_this_turn.contains(&physical_id),
            attached_to: world.state.attachments.get(&physical.id()).copied(),
            object: physical.id(),
            zone: physical.zone(),
            current_types: defined.types.clone(),
            current_names: defined.names.clone(),
            current_supertypes: defined.supertypes.clone(),
            current_creature_subtypes: defined.creature_subtypes.clone(),
            current_land_subtypes: defined.land_subtypes.clone(),
            current_planeswalker_subtypes: defined.planeswalker_subtypes.clone(),
            current_spell_subtypes: defined.spell_subtypes.clone(),
            current_artifact_subtypes: defined.artifact_subtypes.clone(),
            current_enchantment_subtypes: defined.enchantment_subtypes.clone(),
            intrinsic_mana: BTreeSet::new(),
            current_colors: defined.colors.clone(),
            defined,
            controller: if is_battlefield {
                Some(physical.default_controller().ok_or(
                    CharacteristicInvariantError::MissingBattlefieldController(physical.id()),
                )?)
            } else {
                physical.controller()
            },
            abilities: PrintedAbilityRoster {
                entry_tap: program
                    .permanent()
                    .and_then(|permanent| permanent.entry_tap),
                attack_requirement: program.attack_requirement,
                additional_attack_requirements: program.additional_attack_requirements.clone(),
                combat_limits: program.combat_limits,
                entry_counter_abilities: !program.entry_counter_abilities.is_empty(),
                face_up: (0..program.face_up_abilities.len()).collect(),
                zone_replacements: program
                    .zone_replacements
                    .iter()
                    .map(|ability| ability.id)
                    .collect(),
                source: physical.id(),
                card: physical.card_id(),
                mana: program.mana.iter().map(|ability| ability.id).collect(),
                activated: program.activated.iter().map(|ability| ability.id).collect(),
                triggered: program
                    .triggered
                    .iter()
                    .filter(|a| {
                        !matches!(
                            a.condition.event,
                            crate::program::TriggerEvent::Dies {
                                mode: crate::program::DiesTriggerMode::Blitz,
                                ..
                            }
                        ) || paid == Some(crate::program::EndStepCast::Blitz)
                    })
                    .map(|ability| ability.id)
                    .collect(),
                continuous: program.continuous.as_ref().map(StaticContinuousAbility::id),
                additional_battlefield_abilities: program
                    .additional_battlefield_abilities
                    .iter()
                    .map(StaticContinuousAbility::id)
                    .collect(),
            },
            printed_text: PrintedText::from_program(program),
            keywords: program.printed_keywords.clone(),
            toxic_value: program.printed_toxic.clone(),
            granted_exalted: BTreeMap::new(),
            granted_any_color_mana: BTreeSet::new(),
            granted_fixed_mana: BTreeMap::new(),
            granted_triggers: BTreeMap::new(),
            granted_activated: BTreeMap::new(),
            inherited_activation_terms: BTreeMap::new(),
            inherited_mana: BTreeMap::new(),
            granted_combat_keywords: BTreeMap::new(),
            blocking_restrictions: Vec::new(),
            granted_restrictions: Vec::new(),
            granted_unblockable: false,
            cannot_be_sacrificed: false,
            cannot_be_untapped: false,
            doesnt_untap_during_untap_step: false,
            suppressed_activations: None,
            activate_as_though_haste: false,
            cannot_be_targeted_by_auras: false,
            cannot_be_targeted_by_colors: crate::program::ColorMask::EMPTY,
            cannot_be_targeted_by_spells_of_colors: crate::program::ColorMask::EMPTY,
            hexproof_ignored_by: BTreeSet::new(),
            power_toughness,
        };
        if matches!(
            paid,
            Some(crate::program::EndStepCast::Dash | crate::program::EndStepCast::Blitz)
        ) {
            object.keywords.insert(KeywordAbility::Haste);
        }
        if physical.zone() == ObjectZone::Stack
            && world
                .state
                .suspend_haste
                .get(&physical.id())
                .is_some_and(|(seat, _)| Some(*seat) == physical.controller())
        {
            object.keywords.insert(KeywordAbility::Haste);
        }
        if world.state.bestowed_spells.contains(&physical.id()) {
            object.current_types = BTreeSet::from([CardType::Enchantment]);
            object.current_creature_subtypes.clear();
            object.current_land_subtypes.clear();
            object.current_spell_subtypes.clear();
        }
        object.derive_intrinsic_mana();
        objects.insert(physical.id(), object);
        if is_battlefield {
            battlefield.insert(physical.id());
            let timestamp = world.state.object_timestamp(physical.id()).ok_or(
                CharacteristicInvariantError::MissingBattlefieldTimestamp(physical.id()),
            )?;
            if !used_timestamps.insert(timestamp) {
                return Err(CharacteristicInvariantError::DuplicateTimestamp(timestamp));
            }
            if physical.controller().is_none() {
                return Err(CharacteristicInvariantError::MissingBattlefieldController(
                    physical.id(),
                ));
            }
            for ability in program
                .continuous
                .iter()
                .chain(&program.additional_battlefield_abilities)
            {
                let StaticContinuousAbility::Battlefield { parts, .. } = ability else {
                    continue;
                };
                if validate_buckets(parts.0.iter().map(StaticPart::bucket)).is_err() {
                    return Err(CharacteristicInvariantError::InvalidStaticParts(
                        physical.id(),
                    ));
                }
                let key = EffectInstanceKey::Static {
                    source: physical.id(),
                    ability: ability.id(),
                };
                instances.insert(
                    key,
                    Instance {
                        key,
                        timestamp: copy_timestamp.unwrap_or(timestamp),
                        origin: InstanceOrigin::Static {
                            source: physical.id(),
                            ability,
                        },
                    },
                );
            }
        }
    }
    for effect in world.state.effects() {
        if effect.parts.validate().is_err() || effect.affected.is_empty() {
            return Err(CharacteristicInvariantError::InvalidResolvedParts(
                effect.id,
            ));
        }
        if !used_timestamps.insert(effect.timestamp) {
            return Err(CharacteristicInvariantError::DuplicateTimestamp(
                effect.timestamp,
            ));
        }
        // Fixed copy snapshots were selected before constructing any baseline
        // or static instance. They cannot depend on later layers or one another.
        if matches!(effect.parts.0.as_slice(), [ResolvedPart::Copy(_)]) {
            continue;
        }
        let duration_live = match effect.duration {
            ResolvedDuration::WhileSourceOnBattlefield { source } => {
                world.cards.get(source).is_some_and(|card| {
                    card.zone() == ObjectZone::Battlefield
                        && world.cards.phased_out_under(source).is_none()
                })
            }
            ResolvedDuration::WhileSourceTapped { source } => {
                world.cards.get(source).is_some_and(|card| {
                    card.zone() == ObjectZone::Battlefield
                        && world.cards.phased_out_under(source).is_none()
                        && card.tapped() == Some(true)
                })
            }
            ResolvedDuration::WhileSourceControlledAndTapped {
                source,
                controller,
                epoch,
            } => world.cards.get(source).is_some_and(|card| {
                card.zone() == ObjectZone::Battlefield
                    && world.cards.phased_out_under(source).is_none()
                    && card.control_epoch() == Some(epoch)
                    && card.controller() == Some(controller)
                    && card.tapped() == Some(true)
            }),
            _ => true,
        };
        if !duration_live {
            continue;
        }
        let key = EffectInstanceKey::Resolved(effect.id);
        instances.insert(
            key,
            Instance {
                key,
                timestamp: effect.timestamp,
                origin: InstanceOrigin::Resolved(effect),
            },
        );
    }
    let mut emblem_rules: Vec<(SeatId, StaticContinuousAbility)> = Vec::new();
    for emblem in world.state.emblems.values() {
        if !used_timestamps.insert(emblem.timestamp) {
            return Err(CharacteristicInvariantError::DuplicateTimestamp(
                emblem.timestamp,
            ));
        }
        match &emblem.definition {
            crate::emblem::EmblemDefinition::Authored { continuous, .. } => {
                for ability in continuous {
                    if matches!(ability, StaticContinuousAbility::Battlefield { .. }) {
                        let key = EffectInstanceKey::EmblemStatic {
                            emblem: emblem.id,
                            ability: ability.id(),
                        };
                        instances.insert(
                            key,
                            Instance {
                                key,
                                timestamp: emblem.timestamp,
                                origin: InstanceOrigin::EmblemStatic { emblem, ability },
                            },
                        );
                    } else {
                        emblem_rules.push((emblem.controller, ability.clone()));
                    }
                }
            }
            _ => {
                let key = EffectInstanceKey::Emblem(emblem.id);
                instances.insert(
                    key,
                    Instance {
                        key,
                        timestamp: emblem.timestamp,
                        origin: InstanceOrigin::Emblem(emblem),
                    },
                );
            }
        }
    }
    for temp in world.state.temporary_statics.values() {
        if !used_timestamps.insert(temp.timestamp) {
            return Err(CharacteristicInvariantError::DuplicateTimestamp(
                temp.timestamp,
            ));
        }
        if matches!(temp.ability, StaticContinuousAbility::Battlefield { .. }) {
            let key = EffectInstanceKey::TemporaryStatic(temp.id);
            instances.insert(
                key,
                Instance {
                    key,
                    timestamp: temp.timestamp,
                    origin: InstanceOrigin::TemporaryStatic(temp),
                },
            );
        }
    }
    let mut trace = EvaluationTrace::default();
    let mut copies: Vec<_> = world
        .state
        .effects()
        .filter(|effect| matches!(effect.parts.0.as_slice(), [ResolvedPart::Copy(_)]))
        .collect();
    copies.sort_by_key(|effect| effect.timestamp);
    for effect in copies {
        let affected: BTreeSet<_> = effect
            .affected
            .intersection(&battlefield)
            .copied()
            .collect();
        if !affected.is_empty() {
            trace.applied.push(AppliedComponent {
                key: EffectInstanceKey::Resolved(effect.id),
                bucket: CharacteristicBucket::Copy,
                generated: affected
                    .iter()
                    .map(|object| (*object, effect.parts.0[0].clone()))
                    .collect(),
                affected,
            });
        }
    }
    for instance in instances.values() {
        if matches!(
            instance.origin,
            InstanceOrigin::Static { .. } | InstanceOrigin::Emblem(_)
        ) {
            trace
                .bindings
                .insert(instance.key, PassBinding::NotProcessed);
        }
    }
    for current in objects.values() {
        if let Some(StaticContinuousAbility::CharacteristicDefining { id, .. }) =
            &current.printed_text.continuous
        {
            trace.bindings.insert(
                EffectInstanceKey::Static {
                    source: current.object,
                    ability: *id,
                },
                PassBinding::NotProcessed,
            );
        }
    }
    for bucket in CharacteristicBucket::ALL {
        if matches!(
            bucket,
            CharacteristicBucket::Type
                | CharacteristicBucket::Color
                | CharacteristicBucket::DefinePT
        ) {
            apply_definitions(world, bucket, &battlefield, &mut objects, &mut trace)?;
        }
        let mut remaining: BTreeMap<_, _> = instances
            .iter()
            .filter(|(_, instance)| instance.contains_bucket(bucket))
            .map(|(key, instance)| (*key, *instance))
            .collect();
        while !remaining.is_empty() {
            let generated: BTreeMap<_, _> = remaining
                .iter()
                .map(|(key, instance)| {
                    generate(
                        world,
                        *instance,
                        bucket,
                        &objects,
                        &battlefield,
                        &trace.bindings,
                        world.name_rules,
                    )
                    .map(|component| (*key, component))
                })
                .collect::<Result<_, _>>()?;
            let mut original_waits = BTreeSet::new();
            if remaining.len() > 1 {
                for b_key in remaining.keys() {
                    let b_comp = &generated[b_key];
                    if b_comp.generated.is_empty() {
                        continue;
                    }
                    let saved: Vec<(ObjectId, ObjectCharacteristics)> = b_comp
                        .generated
                        .keys()
                        .filter_map(|id| objects.get(id).cloned().map(|prev| (*id, prev)))
                        .collect();
                    apply(b_comp, &mut objects, &world.state.speed);
                    let mut err = None;
                    for (a_key, a) in &remaining {
                        if a_key == b_key {
                            continue;
                        }
                        match generate(
                            world,
                            *a,
                            bucket,
                            &objects,
                            &battlefield,
                            &trace.bindings,
                            world.name_rules,
                        ) {
                            Ok(after) => {
                                if !generated[a_key].same_semantics(&after) {
                                    original_waits.insert(ComponentWait {
                                        waiting: *a_key,
                                        prerequisite: *b_key,
                                    });
                                }
                            }
                            Err(e) => {
                                err = Some(e);
                                break;
                            }
                        }
                    }
                    for (id, prev) in saved {
                        objects.insert(id, prev);
                    }
                    if let Some(e) = err {
                        return Err(e);
                    }
                }
            }
            let timestamps: BTreeMap<_, _> = remaining
                .iter()
                .map(|(key, instance)| (*key, instance.timestamp))
                .collect();
            let selection = ordering::select(&timestamps, &original_waits)?;
            let component = &generated[&selection.chosen];
            let mut generation_order: Vec<_> = generated.values().cloned().collect();
            generation_order.sort_by_key(|component| timestamps[&component.key]);
            trace.selections.push(ComponentSelection {
                bucket,
                generated: generation_order,
                original_waits,
                strongly_connected: selection.strongly_connected,
                derived_timestamp_waits: selection.derived_timestamp_waits,
                retained_outside_waits: selection.retained_outside_waits,
                eligible: selection.eligible,
                chosen: selection.chosen,
            });
            if component.present {
                if let Some(binding) = trace.bindings.get_mut(&component.key)
                    && matches!(binding, PassBinding::NotProcessed)
                {
                    *binding = PassBinding::Processed(component.affected.clone());
                }
                apply(component, &mut objects, &world.state.speed);
                trace.applied.push(AppliedComponent {
                    key: component.key,
                    bucket,
                    affected: component.affected.clone(),
                    generated: component.generated.clone(),
                });
            }
            remaining.remove(&selection.chosen);
        }
        if bucket == CharacteristicBucket::ModifyPT {
            for object in &battlefield {
                if let Some(pt) = objects
                    .get_mut(object)
                    .and_then(|object| object.power_toughness.as_mut())
                {
                    let counters = world.state.counters(*object);
                    let before = pt.derived.clone();
                    let difference = counters.plus_one.minus(&counters.minus_one);
                    pt.derived.power = pt.derived.power.plus(&difference);
                    pt.derived.toughness = pt.derived.toughness.plus(&difference);
                    for (kind, counts) in &world.state.named_counters {
                        if let crate::effects::CounterKind::PtDelta { power, toughness } = *kind
                            && let Some(cnt) = counts.get(object)
                            && cnt.is_positive()
                        {
                            let dp = cnt.times(&i64::from(power).into());
                            let dt = cnt.times(&i64::from(toughness).into());
                            pt.derived.power = pt.derived.power.plus(&dp);
                            pt.derived.toughness = pt.derived.toughness.plus(&dt);
                        }
                    }
                    if counters.plus_one.is_positive() || counters.minus_one.is_positive() {
                        trace.counter_applications.push(CounterApplication {
                            object: *object,
                            after_components: trace.applied.len(),
                            counters,
                            before,
                            after: pt.derived.clone(),
                        });
                    }
                }
            }
        }
    }
    // Disable any conditional static battlefield rule whose StaticSourceCondition is not met.
    let mut disabled_statics: Vec<(ObjectId, StaticAbilityId)> = Vec::new();
    for o in objects.values() {
        if o.zone != ObjectZone::Battlefield || o.printed_text.static_conditions.is_empty() {
            continue;
        }
        for (id, cond) in &o.printed_text.static_conditions {
            let met = o.controller.is_some_and(|ctrl| {
                static_source_condition_met(cond, world, o.object, ctrl, &objects, &battlefield)
            });
            if !met {
                disabled_statics.push((o.object, *id));
            }
        }
    }
    for (obj_id, static_id) in disabled_statics {
        if let Some(o) = objects.get_mut(&obj_id) {
            if o.abilities.continuous == Some(static_id) {
                o.abilities.continuous = None;
            }
            o.abilities
                .additional_battlefield_abilities
                .retain(|id| *id != static_id);
        }
    }
    // Stamp selector-driven static restriction flags after conditional statics are resolved.
    {
        use crate::characteristics::StaticRestriction;
        enum RestrictionFlag {
            IgnoreHexproof(SeatId),
            ActivateAsHaste,
            CannotBeSacrificed,
            CannotBeUntapped,
            CantBeTargetedBy {
                auras_only: bool,
                colors: crate::program::ColorMask,
            },
        }
        let mut stamped: Vec<(ObjectId, RestrictionFlag)> = Vec::new();
        let collect_restriction =
            |source_id: ObjectId,
             ctrl: SeatId,
             rule: &StaticRestriction,
             stamped: &mut Vec<(ObjectId, RestrictionFlag)>| {
                let (selector, kind) = match rule {
                    StaticRestriction::IgnoreOpponentsHexproof(sel) => {
                        (sel.as_ref(), RestrictionFlag::IgnoreHexproof(ctrl))
                    }
                    StaticRestriction::ActivateAbilitiesAsThoughHaste(sel) => {
                        (sel.as_ref(), RestrictionFlag::ActivateAsHaste)
                    }
                    StaticRestriction::CannotBeSacrificed(sel) => {
                        (sel.as_ref(), RestrictionFlag::CannotBeSacrificed)
                    }
                    StaticRestriction::CannotBeUntapped(sel) => {
                        (sel.as_ref(), RestrictionFlag::CannotBeUntapped)
                    }
                    StaticRestriction::CantBeTargetedBy {
                        selector: sel,
                        auras_only,
                        colors,
                    } => (
                        sel.as_ref(),
                        RestrictionFlag::CantBeTargetedBy {
                            auras_only: *auras_only,
                            colors: *colors,
                        },
                    ),
                    _ => return,
                };
                let affected = select_affected(
                    selector,
                    source_id,
                    ctrl,
                    &objects,
                    &battlefield,
                    world.name_rules,
                    world.combat,
                    Some(world.state),
                );
                for target_id in affected {
                    let flag = match kind {
                        RestrictionFlag::IgnoreHexproof(s) => RestrictionFlag::IgnoreHexproof(s),
                        RestrictionFlag::ActivateAsHaste => RestrictionFlag::ActivateAsHaste,
                        RestrictionFlag::CannotBeSacrificed => RestrictionFlag::CannotBeSacrificed,
                        RestrictionFlag::CannotBeUntapped => RestrictionFlag::CannotBeUntapped,
                        RestrictionFlag::CantBeTargetedBy { auras_only, colors } => {
                            RestrictionFlag::CantBeTargetedBy { auras_only, colors }
                        }
                    };
                    stamped.push((target_id, flag));
                }
            };
        for source in objects.values() {
            if source.zone != ObjectZone::Battlefield {
                continue;
            }
            let Some(ctrl) = source.controller else {
                continue;
            };
            for ability in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
            {
                let active = source.abilities.continuous == Some(ability.id())
                    || source
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id());
                if !active {
                    continue;
                }
                if let StaticContinuousAbility::Restriction { rule, .. } = ability {
                    collect_restriction(source.object, ctrl, rule, &mut stamped);
                }
            }
            for rule in &source.granted_restrictions {
                collect_restriction(source.object, ctrl, rule, &mut stamped);
            }
        }
        for temp in world.state.temporary_statics.values() {
            if let StaticContinuousAbility::Restriction { rule, .. } = &temp.ability {
                let src_id = temp.source_object.unwrap_or(ObjectId::SENTINEL);
                collect_restriction(src_id, temp.controller, rule, &mut stamped);
            }
        }
        for (target_id, flag) in stamped {
            if let Some(target) = objects.get_mut(&target_id) {
                match flag {
                    RestrictionFlag::IgnoreHexproof(seat) => {
                        target.hexproof_ignored_by.insert(seat);
                    }
                    RestrictionFlag::ActivateAsHaste => {
                        target.activate_as_though_haste = true;
                    }
                    RestrictionFlag::CannotBeSacrificed => {
                        target.cannot_be_sacrificed = true;
                    }
                    RestrictionFlag::CannotBeUntapped => {
                        target.cannot_be_untapped = true;
                    }
                    RestrictionFlag::CantBeTargetedBy { auras_only, colors } => {
                        if auras_only {
                            target.cannot_be_targeted_by_auras = true;
                        }
                        target.cannot_be_targeted_by_colors =
                            target.cannot_be_targeted_by_colors.union(colors);
                    }
                }
            }
        }
    }
    // CR 701.60a: A suspected creature has menace and "This creature can't block."
    for (&suspect_id, count) in &world.state.suspect_counters {
        if count.is_positive()
            && let Some(creature) = objects.get_mut(&suspect_id)
            && creature.zone == ObjectZone::Battlefield
            && creature.is_creature()
        {
            if !creature
                .suppressed_keywords
                .contains(&KeywordAbility::Menace)
            {
                creature.keywords.insert(KeywordAbility::Menace);
            }
            creature
                .blocking_restrictions
                .push(BlockingRestriction::CannotBlock);
        }
    }
    // CR 701.52c Ring-bearer level 1: legendary and cannot be blocked by creatures with greater power.
    for (&seat, &bearer_id) in &world.state.ring_bearer {
        if world.state.ring_level.get(&seat).copied().unwrap_or(0) >= 1
            && let Some(bearer) = objects.get_mut(&bearer_id)
            && bearer.zone == ObjectZone::Battlefield
            && bearer.controller == Some(seat)
        {
            bearer
                .current_supertypes
                .insert(crate::program::Supertype::Legendary);
            bearer
                .blocking_restrictions
                .push(BlockingRestriction::CannotBeBlockedByGreaterPower);
        }
    }
    // Graveyard grants use each source's final controller and active ability roster.
    let encore_grants = objects
        .values()
        .filter(|o| o.zone == ObjectZone::Battlefield)
        .flat_map(|source| {
            source
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&source.printed_text.additional_battlefield_abilities)
                    .filter_map(move |rule| {
                        let crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardSubtypeEncore(
                                    subtype,
                                ),
                        } = rule
                        else {
                            return None;
                        };
                        (source.abilities.continuous == Some(*id)
                            || source
                                .abilities
                                .additional_battlefield_abilities
                                .contains(id))
                        .then(|| {
                            Some((
                                source.controller?,
                                *subtype,
                                EffectInstanceKey::Static {
                                    source: source.object,
                                    ability: *id,
                                },
                            ))
                        })
                        .flatten()
                    })
        })
        .collect::<Vec<_>>();
    for (controller, subtype, grant) in encore_grants {
        for recipient in objects.values_mut().filter(|o| {
            o.zone == ObjectZone::Graveyard(controller)
                && o.is_creature()
                && o.current_creature_subtypes.contains(&subtype)
        }) {
            let physical = world.cards.get(recipient.object).ok_or(
                CharacteristicInvariantError::MissingDefinition(recipient.object),
            )?;
            let mana_value = physical
                .base_program(world.programs)
                .ok_or(CharacteristicInvariantError::MissingDefinition(
                    recipient.object,
                ))?
                .mana_value();
            let mut price = crate::mana::ManaCost::free();
            price.generic = mana_value;
            recipient.granted_activated.insert(
                grant,
                crate::activated::ActivatedAbility {
                    id: crate::activated::ActivatedAbilityId(0),
                    mana_cost: price,
                    source_cost: crate::activated::SourceCost::Exile {
                        tap_source: false,
                        from_graveyard: true,
                    },
                    body: crate::program::ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::StackOp(Box::new(
                            crate::toolbox::StackOpSpec::encore(),
                        ))],
                    ),
                },
            );
        }
    }
    let mut evaluated = EvaluatedCharacteristics {
        objects,
        trace,
        player_color_protection: world.state.player_color_protection.clone(),
        player_granted_keywords: BTreeMap::new(),
        emblem_rules,
    };
    let improvised: Vec<_> = evaluated
        .objects
        .values()
        .filter(|spell| {
            spell.zone == ObjectZone::Stack
                && !spell.has_type(CardType::Artifact)
                && spell
                    .controller
                    .is_some_and(|player| evaluated.grants_improvise(player))
        })
        .map(|spell| spell.object)
        .collect();
    for spell in improvised {
        if let Some(spell) = evaluated.objects.get_mut(&spell) {
            spell.keywords.insert(KeywordAbility::Improvise);
        }
    }
    Ok(evaluated)
}

/// CR 613.3/613.4a/613.8a: the admitted self-only definitions precede
/// ordinary effects. Their numeric reads use earlier-layer traits, never another
/// 7a result; no same-sublayer dependency or invented timestamp is needed.
fn apply_definitions(
    world: CharacteristicWorld<'_>,
    bucket: CharacteristicBucket,
    battlefield: &BTreeSet<ObjectId>,
    objects: &mut BTreeMap<ObjectId, ObjectCharacteristics>,
    trace: &mut EvaluationTrace,
) -> Result<(), CharacteristicInvariantError> {
    let mut components = Vec::new();
    for (source, current) in objects.iter() {
        let keyword = match bucket {
            CharacteristicBucket::Type => Some((
                KeywordAbility::Changeling,
                DefiningPart::CreatureTypes(DefiningCreatureTypes::All),
            )),
            CharacteristicBucket::Color => Some((
                KeywordAbility::Devoid,
                DefiningPart::Colors(BTreeSet::new()),
            )),
            _ => None,
        };
        let keyword_part = keyword.as_ref().and_then(|(keyword, part)| {
            current.has_keyword(*keyword).then_some((
                EffectInstanceKey::PrintedKeyword {
                    source: *source,
                    keyword: *keyword,
                },
                part,
            ))
        });
        let authored_part = current
            .printed_text
            .continuous
            .as_ref()
            .and_then(|ability| {
                let StaticContinuousAbility::CharacteristicDefining { id, parts } = ability else {
                    return None;
                };
                let key = EffectInstanceKey::Static {
                    source: *source,
                    ability: *id,
                };
                let started = matches!(trace.bindings.get(&key), Some(PassBinding::Processed(_)));
                if !started && current.abilities.continuous != Some(*id) {
                    return None;
                }
                parts
                    .0
                    .iter()
                    .find(|part| part.bucket() == bucket)
                    .map(|part| (key, part))
            });
        for (key, part) in keyword_part.into_iter().chain(authored_part) {
            let operation = match part {
                DefiningPart::CommanderChosenColor(_) => {
                    let Some(card) = world.cards.get(*source).and_then(|object| object.card_id())
                    else {
                        continue;
                    };
                    let Some(&chosen) = world.state.commander_chosen_colors.get(&card) else {
                        continue;
                    };
                    if !world.state.commander_cards.contains(&card) {
                        continue;
                    }
                    ResolvedPart::Color(ColorOperation::Set([chosen].into()))
                }
                DefiningPart::Colors(colors) => {
                    ResolvedPart::Color(ColorOperation::Set(colors.clone()))
                }
                DefiningPart::CreatureTypes(types) => {
                    let values = match types {
                        DefiningCreatureTypes::Literal(values) => values.clone(),
                        DefiningCreatureTypes::All => world
                            .vocabulary
                            .members()
                            .iter()
                            .map(|name| {
                                world.vocabulary.resolve_label(name).ok_or(
                                    CharacteristicInvariantError::InvalidStaticParts(*source),
                                )
                            })
                            .collect::<Result<BTreeSet<_>, _>>()?,
                    };
                    ResolvedPart::Type(TypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(values),
                    })
                }
                DefiningPart::PowerToughness { power, toughness } => {
                    let physical = world
                        .cards
                        .get(*source)
                        .ok_or(CharacteristicInvariantError::InvalidStaticParts(*source))?;
                    let controller = current.controller.unwrap_or(physical.owner());
                    let baseline = current
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.clone())
                        .unwrap_or_else(|| initial_pt(*source, &current.defined).derived);
                    ResolvedPart::SetPT(PowerToughness {
                        power: power
                            .as_ref()
                            .map(|expression| {
                                numeric(
                                    world,
                                    expression,
                                    *source,
                                    controller,
                                    objects,
                                    battlefield,
                                    world.name_rules,
                                )
                            })
                            .unwrap_or(baseline.power),
                        toughness: toughness
                            .as_ref()
                            .map(|expression| {
                                numeric(
                                    world,
                                    expression,
                                    *source,
                                    controller,
                                    objects,
                                    battlefield,
                                    world.name_rules,
                                )
                            })
                            .unwrap_or(baseline.toughness),
                    })
                }
            };
            components.push(GeneratedComponent {
                key,
                bucket,
                present: true,
                affected: BTreeSet::from([*source]),
                generated: BTreeMap::from([(*source, operation)]),
            });
        }
    }
    for component in components {
        trace.bindings.insert(
            component.key,
            PassBinding::Processed(component.affected.clone()),
        );
        apply(&component, objects, &world.state.speed);
        trace.applied.push(AppliedComponent {
            key: component.key,
            bucket,
            affected: component.affected,
            generated: component.generated,
        });
    }
    Ok(())
}
fn selector_subtype_matches(sub: SelectorSubtype, object: &ObjectCharacteristics) -> bool {
    match sub {
        SelectorSubtype::Creature(kind) => object.current_creature_subtypes.contains(&kind),
        SelectorSubtype::Land(kind) => object.current_land_subtypes.contains(&kind),
        SelectorSubtype::Artifact(kind) => object.current_artifact_subtypes.contains(&kind),
        SelectorSubtype::Enchantment(kind) => object.current_enchantment_subtypes.contains(&kind),
        SelectorSubtype::Planeswalker(kind) => object.current_planeswalker_subtypes.contains(&kind),
        SelectorSubtype::Aura => matches!(
            object.attachment_kind,
            Some(crate::attachment::AttachmentKind::Aura(_))
        ),
    }
}

// Private selector helper passes borrowed evaluation context slices directly to avoid allocating a wrapper struct on hot layer sweeps.
#[allow(clippy::too_many_arguments)]
fn select_affected(
    selector: &PermanentSelector,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    name_rules: &NameRules,
    combat: Option<&crate::combat::CombatState>,
    state: Option<&super::CharacteristicState>,
) -> BTreeSet<ObjectId> {
    if let Some(subtype) = selector.controlled_creatures_and_typed_tokens {
        return objects
            .values()
            .filter(|object| match object.zone {
                ObjectZone::Battlefield => {
                    object.controller == Some(controller)
                        && ((!object.is_token && object.is_creature())
                            || object.current_creature_subtypes.contains(&subtype))
                }
                ObjectZone::Stack => {
                    object.is_creature()
                        && (object.controller == Some(controller)
                            || !object.is_token && object.owner == controller)
                }
                _ => object.is_creature() && !object.is_token && object.owner == controller,
            })
            .map(|object| object.object)
            .collect();
    }
    if selector.source_has_imprinted_creature && last_imprinted_creature(source, objects).is_none()
    {
        return BTreeSet::new();
    }
    if selector.devotion_below.is_some_and(|(color, minimum)| {
        devotion(color, controller, objects, battlefield) >= i64::from(minimum).into()
    }) {
        return BTreeSet::new();
    }
    if selector.while_controlling_own_commander
        && !battlefield.iter().any(|id| {
            objects.get(id).is_some_and(|view| {
                view.commander_owner == Some(controller) && view.controller == Some(controller)
            })
        })
    {
        return BTreeSet::new();
    }
    if let Some((kind, minimum)) = selector.controller_type_threshold
        && battlefield
            .iter()
            .filter_map(|id| objects.get(id))
            .filter(|p| p.controller == Some(controller) && p.has_type(kind))
            .count()
            < usize::from(minimum.get())
    {
        return BTreeSet::new();
    }
    let creature_counts = selector.sole_controlled_creature.then(|| {
        let mut counts = BTreeMap::new();
        for id in battlefield {
            let creature = &objects[id];
            if creature.is_creature()
                && let Some(controller) = creature.controller
            {
                *counts.entry(controller).or_insert(0usize) += 1;
            }
        }
        counts
    });
    battlefield
        .iter()
        .copied()
        .filter(|id| {
            let object = &objects[id];
            (!selector.matches_source_entry_type
                || objects
                    .get(&source)
                    .and_then(|object| object.entry_creature_type)
                    .is_some_and(|kind| object.current_creature_subtypes.contains(&kind)))
                && (!selector.exclude_auras
                    || !matches!(
                        object.attachment_kind,
                        Some(crate::attachment::AttachmentKind::Aura(_))
                    ))
                && selector
                    .artifact_subtype
                    .is_none_or(|kind| object.current_artifact_subtypes.contains(&kind))
                && selector
                    .planeswalker_subtype
                    .is_none_or(|kind| object.current_planeswalker_subtypes.contains(&kind))
                && selector.combat_role.is_none_or(|role| match role {
                    CombatSelector::Attacking => object.attacking,
                    CombatSelector::Blocking => object.blocking,
                    CombatSelector::AttackingOrBlocking => object.attacking || object.blocking,
                    CombatSelector::OutsideCombat => !object.attacking && !object.blocking,
                    CombatSelector::Blocked => combat.is_some_and(|c| {
                        c.attacker(*id).is_some_and(|a| {
                            a.block_status() == crate::combat::BlockStatus::Blocked
                        })
                    }),
                    CombatSelector::Unblocked => combat.is_some_and(|c| {
                        c.attacker(*id).is_some_and(|a| {
                            a.block_status() == crate::combat::BlockStatus::Unblocked
                        })
                    }),
                    CombatSelector::BlockingSource => combat.is_some_and(|c| {
                        c.blocker(*id)
                            .is_some_and(|b| b.attackers().contains(&source))
                    }),
                    CombatSelector::BlockedBySource => combat.is_some_and(|c| {
                        c.blocker(source)
                            .is_some_and(|b| b.attackers().contains(id))
                    }),
                    CombatSelector::BlockingOrBlockedBySource => combat.is_some_and(|c| {
                        c.blocker(*id)
                            .is_some_and(|b| b.attackers().contains(&source))
                            || c.blocker(source)
                                .is_some_and(|b| b.attackers().contains(id))
                    }),
                    CombatSelector::NotAttacking => !object.attacking,
                })
                && (!selector.requires_untapped || object.tapped == Some(false))
                && (!selector.requires_tapped || object.tapped == Some(true))
                && creature_counts.as_ref().is_none_or(|counts| {
                    object.is_creature()
                        && object
                            .controller
                            .and_then(|controller| counts.get(&controller))
                            == Some(&1)
                })
                && (!selector.plus_one_counter || object.has_plus_one_counter)
                && (!selector.minus_one_counter || object.has_minus_one_counter)
                && (selector.minimum_plus_one_counters == 0
                    || object.plus_one_counters
                        >= i64::from(selector.minimum_plus_one_counters).into())
                && (!selector.requires_flying || object.has_keyword(KeywordAbility::Flying))
                && selector
                    .required_keyword
                    .is_none_or(|kw| object.has_keyword(kw))
                && selector
                    .extra_required_keyword
                    .is_none_or(|kw| object.has_keyword(kw))
                && (!selector.requires_toxic
                    || object
                        .toxic_value
                        .as_ref()
                        .is_some_and(RulesInteger::is_positive))
                && selector.required_counter_kind.is_none_or(|kind| {
                    if let Some(st) = state {
                        st.permanent_counter_count(*id, kind).is_positive()
                    } else {
                        object.present_counter_kinds.contains(&kind)
                    }
                })
                && selector
                    .forbidden_keyword
                    .is_none_or(|kw| !object.has_keyword(kw))
                && selector
                    .forbidden_keywords
                    .iter()
                    .all(|kw| !object.has_keyword(*kw))
                && (!selector.any_counter || object.has_any_counter)
                && (selector.min_power.is_none_or(|min| {
                    object
                        .power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.power >= min.into())
                }))
                && (selector.max_power.is_none_or(|max| {
                    object
                        .power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.power <= max.into())
                }))
                && (selector.min_toughness.is_none_or(|min| {
                    object
                        .power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.toughness >= min.into())
                }))
                && (selector.max_toughness.is_none_or(|max| {
                    object
                        .power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.toughness <= max.into())
                }))
                && (!selector.toughness_greater_than_power
                    || object
                        .power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.toughness > pt.derived.power))
                && selector
                    .min_mana_value
                    .is_none_or(|min| object.defined.mana_value >= i64::from(min).into())
                && selector
                    .max_mana_value
                    .is_none_or(|max| object.defined.mana_value <= i64::from(max).into())
                && (!selector.requires_enchanted
                    || battlefield
                        .iter()
                        .filter_map(|aid| objects.get(aid))
                        .any(|aura| {
                            aura.attached_to == Some(crate::targets::TargetRef::Object(*id))
                                && aura.has_type(CardType::Enchantment)
                        }))
                && (selector.equipped_by_at_least == 0
                    || battlefield
                        .iter()
                        .filter_map(|id| objects.get(id))
                        .filter(|equipment| {
                            equipment.attached_to == Some(crate::targets::TargetRef::Object(*id))
                                && equipment.has_type(CardType::Artifact)
                                && equipment
                                    .current_artifact_subtypes
                                    .contains(&crate::program::ArtifactSubtype::Equipment)
                        })
                        .count()
                        >= usize::from(selector.equipped_by_at_least))
                && selector.station_at.is_none_or(|minimum| {
                    *id == source && object.charge_counters >= i64::from(minimum.get()).into()
                })
                && (!selector.attached_to_source
                    || objects.get(&source).is_some_and(|source| {
                        source.attached_to == Some(crate::targets::TargetRef::Object(*id))
                    }))
                && selector.token.is_none_or(|token| token == object.is_token)
                && (!selector.exclude_source || *id != source)
                && selector
                    .required_supertypes
                    .is_subset(&object.current_supertypes)
                && selector
                    .forbidden_supertypes
                    .is_disjoint(&object.current_supertypes)
                && selector
                    .required_types
                    .iter()
                    .all(|kind| object.has_type(*kind))
                && (selector.any_types.is_empty()
                    || selector.any_types.iter().any(|kind| object.has_type(*kind)))
                && selector
                    .forbidden_types
                    .iter()
                    .all(|kind| !object.has_type(*kind))
                && selector
                    .creature_subtype
                    .is_none_or(|kind| object.current_creature_subtypes.contains(&kind))
                && (selector.any_creature_subtypes.is_empty()
                    || !selector
                        .any_creature_subtypes
                        .is_disjoint(&object.current_creature_subtypes))
                && selector
                    .forbidden_creature_subtype
                    .is_none_or(|kind| !object.current_creature_subtypes.contains(&kind))
                && selector
                    .land_subtype
                    .is_none_or(|kind| object.current_land_subtypes.contains(&kind))
                && (selector.any_land_subtypes.is_empty()
                    || !selector
                        .any_land_subtypes
                        .is_disjoint(&object.current_land_subtypes))
                && selector
                    .required_subtypes
                    .iter()
                    .all(|sub| selector_subtype_matches(*sub, object))
                && (selector.any_subtypes.is_empty()
                    || selector
                        .any_subtypes
                        .iter()
                        .any(|sub| selector_subtype_matches(*sub, object)))
                && selector
                    .forbidden_subtypes
                    .iter()
                    .all(|sub| !selector_subtype_matches(*sub, object))
                && selector
                    .commander
                    .is_none_or(|want| object.commander_owner.is_some() == want)
                && (!selector.requires_modified
                    || object.has_any_counter
                    || battlefield
                        .iter()
                        .filter_map(|aid| objects.get(aid))
                        .any(|att| {
                            att.attached_to == Some(crate::targets::TargetRef::Object(*id))
                                && ((att.has_type(CardType::Artifact)
                                    && att
                                        .current_artifact_subtypes
                                        .contains(&crate::program::ArtifactSubtype::Equipment))
                                    || (att.has_type(CardType::Enchantment)
                                        && matches!(
                                            att.attachment_kind,
                                            Some(crate::attachment::AttachmentKind::Aura(_))
                                        )
                                        && object.controller.is_some()
                                        && att.controller == object.controller))
                        }))
                && selector
                    .face_down
                    .is_none_or(|want| object.is_face_down == want)
                && (!selector.entered_this_turn || object.entered_this_turn)
                && (!selector.matches_source_entry_color
                    || objects
                        .get(&source)
                        .and_then(|src| src.entry_color)
                        .is_some_and(|c| object.current_colors.contains(&c)))
                && (!selector.without_counters || !object.has_any_counter)
                && (!selector.attached_to_source_permanent
                    || object.attached_to == Some(crate::targets::TargetRef::Object(source)))
                && (!selector.attached_to_source_host
                    || objects
                        .get(&source)
                        .and_then(|src| src.attached_to)
                        .is_some_and(|host| object.attached_to == Some(host)))
                && (selector.any_required_keywords.is_empty()
                    || selector
                        .any_required_keywords
                        .iter()
                        .any(|kw| object.has_keyword(*kw)))
                && selector.historic.is_none_or(|want| {
                    let is_historic = object.has_type(CardType::Artifact)
                        || object
                            .current_supertypes
                            .contains(&crate::program::Supertype::Legendary)
                        || object
                            .current_enchantment_subtypes
                            .contains(&crate::program::EnchantmentSubtype::Saga);
                    is_historic == want
                })
                && selector
                    .attacked_this_turn
                    .is_none_or(|want| object.attacked_this_turn == want)
                && (!selector.dealt_damage_this_turn || object.dealt_damage_this_turn)
                && (!selector.kicked || object.kicked)
                && (!selector.renowned || object.renowned)
                && (!selector.saddled || object.saddled)
                && (!selector.monstrous || object.monstrous)
                && (!selector.suspected || object.suspected)
                && (!selector.solved || object.solved)
                && (!selector.exerted || object.exerted)
                && selector.color.matches(&object.current_colors)
                && match &selector.name {
                    NameFilter::Any => true,
                    NameFilter::Named(name) => name_rules.has_name(&object.current_names, name),
                    NameFilter::SharesWithSource => objects.get(&source).is_some_and(|source| {
                        name_rules.shares_name(&object.current_names, &source.current_names)
                    }),
                    NameFilter::DifferentFromSource => objects.get(&source).is_some_and(|source| {
                        name_rules.different_from(&object.current_names, &[&source.current_names])
                    }),
                }
                && match selector.controller {
                    ControllerSelector::Any => true,
                    ControllerSelector::SourceController => object.controller == Some(controller),
                    ControllerSelector::Opponents => {
                        object.controller.is_some_and(|owner| owner != controller)
                    }
                    ControllerSelector::EnchantedPlayer => object.controller.is_some_and(|owner| {
                        objects.get(&source).is_some_and(|aura| {
                            aura.attached_to == Some(crate::targets::TargetRef::Player(owner))
                        })
                    }),

                    ControllerSelector::SourceItself => *id == source,
                    ControllerSelector::SourcePair => {
                        objects[&source].paired_with.is_some_and(|partner| {
                            (*id == source || *id == partner)
                                && objects[&source].is_creature()
                                && battlefield.contains(&partner)
                                && objects.get(&partner).is_some_and(|other| {
                                    other.is_creature() && other.controller == Some(controller)
                                })
                        })
                    }
                    ControllerSelector::SelfAtMaxSpeed => {
                        *id == source && object.controller_speed == 4
                    }
                    ControllerSelector::OwnerIsSourceController => object.owner == controller,
                    ControllerSelector::OwnerIsOpponent => object.owner != controller,
                }
                && selector.owner.is_none_or(|owner| match owner {
                    OwnerSelector::SourceController => object.owner == controller,
                    OwnerSelector::Opponents => object.owner != controller,
                })
        })
        .collect()
}
fn static_source_condition_met(
    condition: &StaticSourceCondition,
    world: CharacteristicWorld<'_>,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
) -> bool {
    use StaticSourceCondition::*;
    match condition {
        ControllerLifeAtMost(limit) => world
            .life
            .and_then(|life| life.get(controller))
            .is_some_and(|life| *life <= i64::from(*limit).into()),
        ControllerLifeAtLeast(limit) => world
            .life
            .and_then(|life| life.get(controller))
            .is_some_and(|life| *life >= i64::from(*limit).into()),
        ControllerControlsSubtype(kind) | OpponentControlsSubtype(kind) => {
            let opponents = matches!(condition, OpponentControlsSubtype(_));
            battlefield
                .iter()
                .filter_map(|id| objects.get(id))
                .any(|card| {
                    card.controller
                        .is_some_and(|owner| (owner != controller) == opponents)
                        && card.current_creature_subtypes.contains(kind)
                })
        }
        OpponentsTurn => world.active_player != controller,
        ControllersTurn => world.active_player == controller,
        HasKeyword(keyword) => objects
            .get(&source)
            .is_some_and(|o| o.has_keyword(*keyword)),
        Delirium => {
            let mut types = BTreeSet::new();
            for card in objects
                .values()
                .filter(|c| c.zone == ObjectZone::Graveyard(controller))
            {
                types.extend(card.current_types.iter().copied());
            }
            types.len() >= 4
        }
        Metalcraft => {
            battlefield
                .iter()
                .filter_map(|id| objects.get(id))
                .filter(|c| c.controller == Some(controller) && c.has_type(CardType::Artifact))
                .count()
                >= 3
        }
        Threshold => {
            objects
                .values()
                .filter(|c| c.zone == ObjectZone::Graveyard(controller))
                .count()
                >= 7
        }
        ControlsCardType { kind, minimum } => {
            battlefield
                .iter()
                .filter_map(|id| objects.get(id))
                .filter(|c| c.controller == Some(controller) && c.has_type(*kind))
                .count()
                >= usize::from(*minimum)
        }
        GraveyardCountAtLeast { types, minimum } => {
            objects
                .values()
                .filter(|c| {
                    c.zone == ObjectZone::Graveyard(controller)
                        && types.matches_any(&c.current_types)
                })
                .count()
                >= usize::from(*minimum)
        }
        HandCountAtMost(limit) => {
            objects
                .values()
                .filter(|c| c.zone == ObjectZone::Hand(controller))
                .count()
                <= usize::from(*limit)
        }
        HandCountAtLeast(limit) => {
            objects
                .values()
                .filter(|c| c.zone == ObjectZone::Hand(controller))
                .count()
                >= usize::from(*limit)
        }
        SourceCounterAtLeast { kind, minimum } => {
            world.state.permanent_counter_count(source, *kind) >= i64::from(*minimum).into()
        }
        SourceAnyCounterAtLeast(minimum) => {
            world.state.total_permanent_counters(source) >= i64::from(*minimum).into()
        }
        SourceIsEquipped => battlefield
            .iter()
            .filter_map(|id| objects.get(id))
            .any(|equipment| {
                equipment.attached_to == Some(crate::targets::TargetRef::Object(source))
                    && equipment.has_type(CardType::Artifact)
                    && equipment
                        .current_artifact_subtypes
                        .contains(&crate::program::ArtifactSubtype::Equipment)
            }),
        SourceIsEnchanted => battlefield
            .iter()
            .filter_map(|id| objects.get(id))
            .any(|aura| {
                aura.attached_to == Some(crate::targets::TargetRef::Object(source))
                    && aura.has_type(CardType::Enchantment)
            }),
        AttachedHostMatches(selector) => objects
            .get(&source)
            .and_then(|src| match src.attached_to {
                Some(crate::targets::TargetRef::Object(host)) => Some(host),
                _ => None,
            })
            .is_some_and(|host| {
                select_affected(
                    selector,
                    source,
                    controller,
                    objects,
                    battlefield,
                    world.name_rules,
                    world.combat,
                    Some(world.state),
                )
                .contains(&host)
            }),
        ControlsExactCount { selector, count } => {
            let mut sel = selector.as_ref().clone();
            if sel.controller == ControllerSelector::Any {
                sel.controller = ControllerSelector::SourceController;
            }
            count_matching(
                &sel,
                source,
                controller,
                objects,
                battlefield,
                world.name_rules,
                world.combat,
                Some(world.state),
            ) == i64::from(*count).into()
        }
        SourceIsTapped => objects.get(&source).is_some_and(|o| o.tapped == Some(true)),
        SourceIsUntapped => objects
            .get(&source)
            .is_some_and(|o| o.tapped == Some(false)),
        SourceClassLevelAtLeast(min) => {
            world.state.class_levels.get(&source).copied().unwrap_or(1) >= *min
        }
        SourceIsSolved => world.state.solved_cases.contains(&source),
        SourceEnteredThisTurn => world.state.entered_objects_this_turn.contains(&source),
        SourcePaired => world.state.paired_creatures.contains_key(&source),
        SourceIsPrepared => world.state.prepared.contains(&source),
        EntryOptionChosen(expected) => {
            world.state.entry_option_choices.get(&source) == Some(expected)
        }
        NonlandPermanentsEnteredThisTurnAtLeast(min) => {
            let total = world
                .state
                .permanents_entered_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0);
            let lands = world
                .state
                .entered_types_this_turn
                .get(&controller)
                .and_then(|by_type| by_type.get(&CardType::Land))
                .copied()
                .unwrap_or(0);
            total.saturating_sub(lands) >= *min
        }
        AnyLibraryAtMost(max) => world.participants.iter().any(|&seat| {
            objects
                .values()
                .filter(|c| c.zone == ObjectZone::Library(seat))
                .count()
                <= usize::from(*max)
        }),
        AnyPlayerHandCountAtMost(max) => world.participants.iter().any(|&seat| {
            objects
                .values()
                .filter(|c| c.zone == ObjectZone::Hand(seat))
                .count()
                <= usize::from(*max)
        }),
        MostCommonPermanentColor { color, or_tied } => {
            let mut counts = [0usize; 5];
            let idx = |c: Color| match c {
                Color::White => 0,
                Color::Blue => 1,
                Color::Black => 2,
                Color::Red => 3,
                Color::Green => 4,
            };
            for id in battlefield {
                if let Some(perm) = objects.get(id) {
                    for &c in &perm.current_colors {
                        counts[idx(c)] += 1;
                    }
                }
            }
            let target = counts[idx(*color)];
            let max_other = counts
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != idx(*color))
                .map(|(_, &n)| n)
                .max()
                .unwrap_or(0);
            if *or_tied {
                target >= max_other
            } else {
                target > max_other
            }
        }
        ControllerHasCityBlessing => world.state.city_blessing.contains(&controller),
        ControllerGainedLifeThisTurn => world
            .state
            .life_gained_this_turn
            .get(&controller)
            .is_some_and(RulesInteger::is_positive),
        ControllerLostLifeThisTurn => world
            .state
            .life_lost_this_turn
            .get(&controller)
            .is_some_and(RulesInteger::is_positive),
        OpponentLostLifeThisTurn => world
            .state
            .life_lost_this_turn
            .iter()
            .any(|(&seat, amount)| seat != controller && amount.is_positive()),
        ControllerAttackedThisTurn => world.state.attacked_this_turn_players.contains(&controller),
        ControllerCastSpellsThisTurn(min) => {
            world
                .state
                .spells_cast_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= *min
        }
        ControllerDiscardedThisTurn => {
            world
                .state
                .cards_discarded_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        ControllerSacrificedThisTurn => {
            world
                .state
                .permanents_sacrificed_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        PermanentLeftBattlefieldThisTurn => {
            world
                .state
                .permanents_left_battlefield_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        CreatureEnteredUnderControlThisTurn => {
            world
                .state
                .creatures_entered_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        DescendedThisTurn => world.state.descended_this_turn.contains(&controller),
        CommittedCrimeThisTurn => world.state.committed_crime_this_turn.contains(&controller),
        ControllerDrewCardsThisTurn(min) => {
            world
                .state
                .cards_drawn_this_turn_counts
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= *min
        }
        ControlsMatching { selector, minimum } => {
            let mut sel = selector.as_ref().clone();
            if sel.controller == ControllerSelector::Any {
                sel.controller = ControllerSelector::SourceController;
            }
            count_matching(
                &sel,
                source,
                controller,
                objects,
                battlefield,
                world.name_rules,
                world.combat,
                Some(world.state),
            ) >= i64::from(*minimum).into()
        }
        OpponentControlsMatching { selector, minimum } => {
            let mut sel = selector.as_ref().clone();
            if sel.controller == ControllerSelector::Any {
                sel.controller = ControllerSelector::Opponents;
            }
            count_matching(
                &sel,
                source,
                controller,
                objects,
                battlefield,
                world.name_rules,
                world.combat,
                Some(world.state),
            ) >= i64::from(*minimum).into()
        }
        Eval(cond) => {
            let ctx = super::WorldEvalCtx {
                active_player: world.active_player,
                step: world.step,
                participants: world.participants,
                life: world.life,
                combat: world.combat,
                cards: world.cards,
                programs: world.programs,
                state: world.state,
                objects,
                source,
                controller,
                name_rules: world.name_rules,
                trigger_subject: None,
                related_player: None,
                defending_player: defending_player_for_source(world.combat, objects, source),
            };
            super::eval_world_condition(&ctx, cond)
        }
        RoomDoorUnlocked(half) => world
            .state
            .unlocked_doors
            .get(&source)
            .is_some_and(|doors| doors.is_unlocked(*half)),
        RoomFullyUnlocked => world
            .state
            .unlocked_doors
            .get(&source)
            .is_some_and(|doors| doors.fully_unlocked()),
        Not(inner) => {
            !static_source_condition_met(inner, world, source, controller, objects, battlefield)
        }
        And(a, b) => {
            static_source_condition_met(a, world, source, controller, objects, battlefield)
                && static_source_condition_met(b, world, source, controller, objects, battlefield)
        }
        Or(a, b) => {
            static_source_condition_met(a, world, source, controller, objects, battlefield)
                || static_source_condition_met(b, world, source, controller, objects, battlefield)
        }
    }
}

fn devotion(
    color: Color,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
) -> RulesInteger {
    battlefield
        .iter()
        .filter_map(|id| objects.get(id))
        .filter(|view| view.controller == Some(controller))
        .filter_map(|view| view.defined.colored_mana_symbols.get(&color))
        .fold(RulesInteger::from(0), |total, n| total.plus(n))
}
fn numeric(
    world: CharacteristicWorld<'_>,
    expression: &NumericExpression,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    name_rules: &NameRules,
) -> RulesInteger {
    match expression {
        NumericExpression::Literal(value) => value.clone(),
        NumericExpression::Count {
            selector,
            multiplier,
            offset,
        } => count_matching(
            selector,
            source,
            controller,
            objects,
            battlefield,
            name_rules,
            world.combat,
            Some(world.state),
        )
        .times(multiplier)
        .plus(offset),
        NumericExpression::ZoneCount { zone, types } => match zone {
            StaticZone::ControlledBattlefield => RulesInteger::from_usize(
                battlefield
                    .iter()
                    .filter(|id| {
                        let object = &objects[id];
                        object.controller == Some(controller)
                            && types.matches_any(&object.current_types)
                    })
                    .count(),
            ),
            StaticZone::OwnGraveyard => RulesInteger::from_usize(
                objects
                    .values()
                    .filter(|card| {
                        card.zone == ObjectZone::Graveyard(controller)
                            && types.matches_any(&card.current_types)
                    })
                    .count(),
            ),
            StaticZone::OwnHand => RulesInteger::from_usize(
                objects
                    .values()
                    .filter(|card| {
                        card.zone == ObjectZone::Hand(controller)
                            && types.matches_any(&card.current_types)
                    })
                    .count(),
            ),
            StaticZone::AllGraveyards => RulesInteger::from_usize(
                objects
                    .values()
                    .filter(|card| {
                        matches!(card.zone, ObjectZone::Graveyard(_))
                            && types.matches_any(&card.current_types)
                    })
                    .count(),
            ),
            StaticZone::AnyBattlefield => RulesInteger::from_usize(
                battlefield
                    .iter()
                    .filter(|id| types.matches_any(&objects[id].current_types))
                    .count(),
            ),
            StaticZone::OwnExile => RulesInteger::from_usize(
                objects
                    .values()
                    .filter(|card| {
                        card.zone == ObjectZone::Exile
                            && card.owner == controller
                            && types.matches_any(&card.current_types)
                    })
                    .count(),
            ),
            StaticZone::LinkedExile => RulesInteger::from_usize(
                world.state.imprinted_cards.get(&source).map_or(0, |ids| {
                    ids.iter()
                        .filter(|id| {
                            objects.get(id).is_some_and(|card| {
                                card.zone == ObjectZone::Exile
                                    && types.matches_any(&card.current_types)
                            })
                        })
                        .count()
                }),
            ),
        },
        NumericExpression::Counters {
            source: linked,
            kind,
        } => match linked {
            Some(target) => world.state.permanent_counter_count(*target, *kind),
            None => match kind {
                crate::effects::CounterKind::Blessing => objects
                    .get(&source)
                    .map_or_else(|| 0.into(), |object| object.blessing_counters.clone()),
                crate::effects::CounterKind::Charge => objects
                    .get(&source)
                    .map_or_else(|| 0.into(), |object| object.charge_counters.clone()),
                other => world.state.permanent_counter_count(source, *other),
            },
        },
        NumericExpression::ImprintedStat { stat, total } => {
            if *total {
                imprinted_creatures(source, objects)
                    .filter_map(|card| card.power_toughness.as_ref())
                    .fold(0.into(), |acc, pt| {
                        let val = match stat {
                            crate::program::ObjectStatKind::Power => &pt.derived.power,
                            crate::program::ObjectStatKind::Toughness => &pt.derived.toughness,
                            crate::program::ObjectStatKind::ManaValue
                            | crate::program::ObjectStatKind::Loyalty => return acc,
                        };
                        acc.plus(val)
                    })
            } else {
                last_imprinted_creature(source, objects)
                    .and_then(|donor| donor.power_toughness.as_ref())
                    .map_or_else(
                        || 0.into(),
                        |pt| match stat {
                            crate::program::ObjectStatKind::Power => pt.derived.power.clone(),
                            crate::program::ObjectStatKind::Toughness => {
                                pt.derived.toughness.clone()
                            }
                            crate::program::ObjectStatKind::ManaValue
                            | crate::program::ObjectStatKind::Loyalty => 0.into(),
                        },
                    )
            }
        }
        NumericExpression::PlayerStat(stat) => match stat {
            StaticPlayerStat::Devotion(color) => devotion(*color, controller, objects, battlefield),
            StaticPlayerStat::ManaInPool(kind) => world
                .mana
                .get(&(controller, *kind))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            StaticPlayerStat::TotalUnspentMana => world
                .mana
                .iter()
                .filter(|((seat, _), _)| *seat == controller)
                .fold(RulesInteger::from(0), |acc, (_, amt)| acc.plus(amt)),
            StaticPlayerStat::HalfHighestOpponentLife => world
                .life
                .and_then(|life| {
                    world
                        .participants
                        .iter()
                        .copied()
                        .filter(|player| *player != controller)
                        .filter_map(|player| life.get(player))
                        .max()
                })
                .map(RulesInteger::nonnegative_half_rounded_up)
                .unwrap_or_else(|| 0.into()),
            StaticPlayerStat::OwnGraveyardGreatestCreaturePower => objects
                .values()
                .filter(|card| card.zone == ObjectZone::Graveyard(controller) && card.is_creature())
                .filter_map(|card| {
                    card.power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone())
                })
                .max()
                .unwrap_or_else(|| 0.into()),
            StaticPlayerStat::OtherControlledCreatureCounters(kind) => battlefield
                .iter()
                .filter(|id| **id != source)
                .filter(|id| {
                    let obj = &objects[id];
                    obj.controller == Some(controller) && obj.is_creature()
                })
                .fold(RulesInteger::from(0), |acc, id| {
                    acc.plus(&world.state.permanent_counter_count(*id, *kind))
                }),
            StaticPlayerStat::OwnGraveyardWithCycling => RulesInteger::from_usize(
                objects
                    .values()
                    .filter(|card| {
                        card.zone == ObjectZone::Graveyard(controller)
                            && card.printed_text.activated.iter().any(|ab| {
                                matches!(
                                    ab.source_cost,
                                    crate::activated::SourceCost::HandDiscardSelf(
                                        crate::activated::HandDiscardKeyword::Cycling
                                    )
                                )
                            })
                    })
                    .count(),
            ),
        },
        NumericExpression::AttachedAurasAndEquipment { multiplier } => RulesInteger::from_usize(
            battlefield
                .iter()
                .filter(|id| {
                    let object = &objects[id];
                    object.attached_to == Some(crate::targets::TargetRef::Object(source))
                        && match object.attachment_kind {
                            Some(crate::attachment::AttachmentKind::Equipment) => {
                                object.has_type(CardType::Artifact)
                            }
                            Some(crate::attachment::AttachmentKind::Aura(_)) => {
                                object.has_type(CardType::Enchantment)
                            }
                            _ => false,
                        }
                })
                .count(),
        )
        .times(multiplier),
        NumericExpression::ConditionalBonus { condition, bonus } => {
            let active = match condition {
                StaticBonusCondition::DuringControllersTurn => controller == world.active_player,
                StaticBonusCondition::ControlledLandsAtLeast(minimum) => {
                    let lands = battlefield
                        .iter()
                        .filter(|id| {
                            objects[id].controller == Some(controller)
                                && objects[id].has_type(CardType::Land)
                        })
                        .count();
                    lands >= usize::from(*minimum)
                }
            };
            if active { bonus.clone() } else { 0.into() }
        }
        NumericExpression::Eval(expr) => {
            let ctx = super::WorldEvalCtx {
                active_player: world.active_player,
                step: world.step,
                participants: world.participants,
                life: world.life,
                combat: world.combat,
                cards: world.cards,
                programs: world.programs,
                state: world.state,
                objects,
                source,
                controller,
                name_rules: world.name_rules,
                trigger_subject: None,
                related_player: None,
                defending_player: defending_player_for_source(world.combat, objects, source),
            };
            super::eval_world_value_expr(&ctx, expr)
        }
        NumericExpression::Add(lhs, rhs) => {
            let a = numeric(
                world,
                lhs,
                source,
                controller,
                objects,
                battlefield,
                name_rules,
            );
            let b = numeric(
                world,
                rhs,
                source,
                controller,
                objects,
                battlefield,
                name_rules,
            );
            a.plus(&b)
        }
        NumericExpression::Scale {
            expr,
            multiplier,
            offset,
        } => numeric(
            world,
            expr,
            source,
            controller,
            objects,
            battlefield,
            name_rules,
        )
        .times(multiplier)
        .plus(offset),
        NumericExpression::Cap { expr, max } => numeric(
            world,
            expr,
            source,
            controller,
            objects,
            battlefield,
            name_rules,
        )
        .min(max.clone()),
    }
}

fn defending_player_for_source(
    combat: Option<&crate::combat::CombatState>,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    source: ObjectId,
) -> Option<SeatId> {
    combat.and_then(|c| {
        c.attacker(source)
            .or_else(|| {
                objects.get(&source).and_then(|o| match o.attached_to {
                    Some(crate::targets::TargetRef::Object(host)) => c.attacker(host),
                    _ => None,
                })
            })
            .map(crate::combat::AttackingCreature::defender)
    })
}
// Private wrapper forwarding the same borrowed layer-evaluation context to select_affected.
#[allow(clippy::too_many_arguments)]
fn count_matching(
    selector: &PermanentSelector,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    name_rules: &NameRules,
    combat: Option<&crate::combat::CombatState>,
    state: Option<&super::CharacteristicState>,
) -> RulesInteger {
    select_affected(
        selector,
        source,
        controller,
        objects,
        battlefield,
        name_rules,
        combat,
        state,
    )
    .iter()
    .fold(0.into(), |count: RulesInteger, _| count.plus(&1.into()))
}
/// Query one already evaluated exact world using the same count semantics as
/// static numeric expressions. Callers supply its matching physical registry.
pub(crate) fn count_permanents(
    cards: &GameObjects,
    evaluated: &EvaluatedCharacteristics,
    source: ObjectId,
    controller: SeatId,
    selector: &PermanentSelector,
    name_rules: &NameRules,
) -> RulesInteger {
    let battlefield = cards
        .iter()
        .filter(|card| card.zone() == ObjectZone::Battlefield)
        .map(|card| card.id())
        .collect();
    count_matching(
        selector,
        source,
        controller,
        &evaluated.objects,
        &battlefield,
        name_rules,
        None,
        None,
    )
}
/// Whether a battlefield `selector` hosted by `source` (`controller`) matches `candidate`.
pub(crate) fn selector_matches_permanent(
    cards: &GameObjects,
    evaluated: &EvaluatedCharacteristics,
    source: ObjectId,
    controller: SeatId,
    selector: &PermanentSelector,
    candidate: ObjectId,
    name_rules: &NameRules,
) -> bool {
    let battlefield = cards
        .iter()
        .filter(|card| card.zone() == ObjectZone::Battlefield)
        .map(|card| card.id())
        .collect();
    select_affected(
        selector,
        source,
        controller,
        &evaluated.objects,
        &battlefield,
        name_rules,
        None,
        None,
    )
    .contains(&candidate)
}
fn last_imprinted_creature(
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
) -> Option<&ObjectCharacteristics> {
    imprinted_creatures(source, objects).next_back()
}

fn imprinted_creatures(
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
) -> impl DoubleEndedIterator<Item = &ObjectCharacteristics> {
    objects
        .get(&source)
        .into_iter()
        .flat_map(|source| &source.imprinted_cards)
        .filter_map(|id| objects.get(id))
        .filter(|card| card.zone == ObjectZone::Exile && card.is_creature())
}

fn static_subtype(
    operand: &StaticSubtypeOperand,
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> BTreeSet<spellwake_contracts::creature_types::CreatureTypeId> {
    match operand {
        StaticSubtypeOperand::ImprintedCreatureTypes { keep } => {
            let mut subtypes = keep.clone();
            if let Some(donor) = last_imprinted_creature(source, objects) {
                subtypes.extend(&donor.current_creature_subtypes);
            }
            subtypes
        }
        StaticSubtypeOperand::Literal(value) => value.clone(),
        StaticSubtypeOperand::SourceEntryCreatureType => objects
            .get(&source)
            .and_then(|object| object.entry_creature_type)
            .into_iter()
            .collect(),
        StaticSubtypeOperand::SourceCurrentSubtypes => {
            objects[&source].current_creature_subtypes.clone()
        }
        StaticSubtypeOperand::AllCreatureTypes => vocabulary
            .members()
            .iter()
            .filter_map(|name| vocabulary.resolve_label(name))
            .collect(),
    }
}
fn static_land_subtype(
    operand: &StaticLandSubtypeOperand,
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    state: &super::CharacteristicState,
) -> BTreeSet<crate::land::LandType> {
    match operand {
        StaticLandSubtypeOperand::Literal(value) => value.clone(),
        StaticLandSubtypeOperand::SourceCurrentLandSubtypes => {
            objects[&source].current_land_subtypes.clone()
        }
        StaticLandSubtypeOperand::SourceEntryBasicLandType => {
            let chosen = match state.entry_option_choices.get(&source).copied() {
                Some(0) => Some(crate::land::LandType::Plains),
                Some(1) => Some(crate::land::LandType::Island),
                Some(2) => Some(crate::land::LandType::Swamp),
                Some(3) => Some(crate::land::LandType::Mountain),
                Some(4) => Some(crate::land::LandType::Forest),
                _ => None,
            };
            chosen.into_iter().collect()
        }
    }
}
fn generate(
    world: CharacteristicWorld<'_>,
    instance: Instance<'_>,
    bucket: CharacteristicBucket,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    bindings: &BTreeMap<EffectInstanceKey, PassBinding>,
    name_rules: &NameRules,
) -> Result<GeneratedComponent, CharacteristicInvariantError> {
    let (affected, operation) = match instance.origin {
        InstanceOrigin::Emblem(emblem) => {
            let affected = match bindings.get(&instance.key) {
                Some(PassBinding::Processed(affected)) => affected.clone(),
                _ => battlefield
                    .iter()
                    .copied()
                    .filter(|id| {
                        objects.get(id).is_some_and(|object| {
                            object.is_creature() && object.controller == Some(emblem.controller)
                        })
                    })
                    .collect(),
            };
            let crate::emblem::EmblemDefinition::ControlledCreatureBonus {
                power_toughness,
                keywords,
            } = &emblem.definition
            else {
                return Err(CharacteristicInvariantError::NoEligibleComponent);
            };
            let operation = match bucket {
                CharacteristicBucket::Abilities => Some(ResolvedPart::Abilities(
                    AbilityChange::grant_keywords(keywords.iter().copied()),
                )),
                CharacteristicBucket::ModifyPT => {
                    Some(ResolvedPart::ModifyPT(power_toughness.clone()))
                }
                _ => None,
            };
            (affected, operation)
        }
        InstanceOrigin::EmblemStatic { emblem: _, ability }
        | InstanceOrigin::TemporaryStatic(TemporaryBattlefieldStatic { ability, .. }) => {
            let StaticContinuousAbility::Battlefield {
                selector, parts, ..
            } = ability
            else {
                return Err(CharacteristicInvariantError::NoEligibleComponent);
            };
            let (controller, sentinel, locked_objects) = match instance.origin {
                InstanceOrigin::EmblemStatic { emblem, .. } => {
                    (emblem.controller, ObjectId::SENTINEL, None)
                }
                InstanceOrigin::TemporaryStatic(temp) => (
                    temp.controller,
                    temp.source_object.unwrap_or(ObjectId::SENTINEL),
                    temp.locked_objects.as_ref(),
                ),
                _ => unreachable!(),
            };
            let affected = match bindings.get(&instance.key) {
                Some(PassBinding::Processed(affected)) => affected.clone(),
                _ if selector.source_condition.as_ref().is_some_and(|condition| {
                    !static_source_condition_met(
                        condition,
                        world,
                        sentinel,
                        controller,
                        objects,
                        battlefield,
                    )
                }) =>
                {
                    BTreeSet::new()
                }
                _ => {
                    let mut selected = select_affected(
                        selector,
                        sentinel,
                        controller,
                        objects,
                        battlefield,
                        name_rules,
                        world.combat,
                        Some(world.state),
                    );
                    if let Some(locked) = locked_objects {
                        selected.retain(|id| locked.contains(id));
                    }
                    selected
                }
            };
            let operation = if affected.is_empty() {
                None
            } else {
                let mut resolved_parts = Vec::new();
                for part in parts.0.iter().filter(|part| part.bucket() == bucket) {
                    resolved_parts.push(match part {
                        StaticPart::SetPTToOwnManaValue => {
                            let generated = affected
                                .iter()
                                .map(|id| {
                                    let value = &objects[id].defined.mana_value;
                                    (
                                        *id,
                                        ResolvedPart::SetPT(PowerToughness {
                                            power: value.clone(),
                                            toughness: value.clone(),
                                        }),
                                    )
                                })
                                .collect();
                            return Ok(GeneratedComponent {
                                key: instance.key,
                                bucket,
                                present: true,
                                affected,
                                generated,
                            });
                        }
                        StaticPart::ModifyPTPerOwnColor { power, toughness } => {
                            let generated = affected
                                .iter()
                                .map(|id| {
                                    let n =
                                        RulesInteger::from_usize(objects[id].current_colors.len());
                                    (
                                        *id,
                                        ResolvedPart::ModifyPT(PowerToughness {
                                            power: power.times(&n),
                                            toughness: toughness.times(&n),
                                        }),
                                    )
                                })
                                .collect();
                            return Ok(GeneratedComponent {
                                key: instance.key,
                                bucket,
                                present: true,
                                affected,
                                generated,
                            });
                        }
                        StaticPart::ControlSourceController => ResolvedPart::Control(controller),
                        StaticPart::Color(change) => {
                            ResolvedPart::Color(change.map(|operand| match operand {
                                StaticColorOperand::Literal(colors) => colors.clone(),
                                StaticColorOperand::SourceCurrentColors => BTreeSet::new(),
                            }))
                        }
                        StaticPart::Text(change) => ResolvedPart::Text(*change),
                        StaticPart::Names(change) => {
                            let resolve = |operand: &StaticNameOperand| match operand {
                                StaticNameOperand::Literal(names) => names.clone(),
                                StaticNameOperand::SourceCurrentNames => {
                                    spellwake_contracts::names::ObjectNames::nameless()
                                }
                            };
                            ResolvedPart::Names(match change {
                                NameOperation::Set(names) => NameOperation::Set(resolve(names)),
                                NameOperation::Add(names) => NameOperation::Add(resolve(names)),
                            })
                        }
                        StaticPart::Type(change) => ResolvedPart::Type(TypeChange {
                            land_subtypes: match &change.land_subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(values) => {
                                    SubtypeOperation::Remove(values.clone())
                                }
                                SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                    static_land_subtype(operand, sentinel, objects, world.state),
                                ),
                                SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                    static_land_subtype(operand, sentinel, objects, world.state),
                                ),
                            },
                            supertypes: change.supertypes.clone(),
                            types: change.types.clone(),
                            subtypes: match &change.subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(values) => {
                                    SubtypeOperation::Remove(values.clone())
                                }
                                SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                    static_subtype(operand, sentinel, objects, world.vocabulary),
                                ),
                                SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                    static_subtype(operand, sentinel, objects, world.vocabulary),
                                ),
                            },
                        }),
                        StaticPart::TypeWithSubtypes {
                            base: change,
                            artifact_subtypes,
                            enchantment_subtypes,
                        } => ResolvedPart::TypeWithSubtypes {
                            base: TypeChange {
                                land_subtypes: match &change.land_subtypes {
                                    SubtypeOperation::Keep => SubtypeOperation::Keep,
                                    SubtypeOperation::Remove(values) => {
                                        SubtypeOperation::Remove(values.clone())
                                    }
                                    SubtypeOperation::Add(operand) => {
                                        SubtypeOperation::Add(static_land_subtype(
                                            operand,
                                            sentinel,
                                            objects,
                                            world.state,
                                        ))
                                    }
                                    SubtypeOperation::Set(operand) => {
                                        SubtypeOperation::Set(static_land_subtype(
                                            operand,
                                            sentinel,
                                            objects,
                                            world.state,
                                        ))
                                    }
                                },
                                supertypes: change.supertypes.clone(),
                                types: change.types.clone(),
                                subtypes: match &change.subtypes {
                                    SubtypeOperation::Keep => SubtypeOperation::Keep,
                                    SubtypeOperation::Remove(values) => {
                                        SubtypeOperation::Remove(values.clone())
                                    }
                                    SubtypeOperation::Add(operand) => {
                                        SubtypeOperation::Add(static_subtype(
                                            operand,
                                            sentinel,
                                            objects,
                                            world.vocabulary,
                                        ))
                                    }
                                    SubtypeOperation::Set(operand) => {
                                        SubtypeOperation::Set(static_subtype(
                                            operand,
                                            sentinel,
                                            objects,
                                            world.vocabulary,
                                        ))
                                    }
                                },
                            },
                            artifact_subtypes: artifact_subtypes.clone(),
                            enchantment_subtypes: enchantment_subtypes.clone(),
                        },
                        StaticPart::SetPT(pair) => ResolvedPart::SetPT(PowerToughness {
                            power: numeric(
                                world,
                                &pair.power,
                                sentinel,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
                                world,
                                &pair.toughness,
                                sentinel,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                        }),
                        StaticPart::ModifyPT(pair) => ResolvedPart::ModifyPT(PowerToughness {
                            power: numeric(
                                world,
                                &pair.power,
                                sentinel,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
                                world,
                                &pair.toughness,
                                sentinel,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                        }),
                        StaticPart::SwitchPT => ResolvedPart::SwitchPT,
                        StaticPart::Abilities(change) => ResolvedPart::Abilities(change.clone()),
                        StaticPart::GrantActivated(ability) => {
                            ResolvedPart::GrantActivated(ability.clone())
                        }
                        StaticPart::GrantActivatedRestricted {
                            ability,
                            restriction,
                        } => ResolvedPart::GrantActivatedRestricted {
                            ability: ability.clone(),
                            restriction: restriction.clone(),
                        },
                        StaticPart::GrantLoyalty {
                            ability,
                            loyalty,
                            variable_loyalty,
                        } => ResolvedPart::GrantLoyalty {
                            ability: ability.clone(),
                            loyalty: *loyalty,
                            variable_loyalty: *variable_loyalty,
                        },
                        StaticPart::GrantTrigger(ability) => {
                            ResolvedPart::GrantTrigger(ability.clone())
                        }
                        StaticPart::GrantAnyColorMana => ResolvedPart::GrantAnyColorMana,
                        StaticPart::GrantSubtypeProtection {
                            types,
                            creatures_only,
                        } => {
                            let qualities =
                                static_subtype(types, sentinel, objects, world.vocabulary)
                                    .into_iter()
                                    .map(|subtype| {
                                        KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                                            subtype,
                                            exclude_subtype: false,
                                            creatures_only: *creatures_only,
                                        })
                                    });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(qualities))
                        }
                        StaticPart::GrantProtectionChosenColor => {
                            let kw =
                                world
                                    .state
                                    .entry_color_choices
                                    .get(&sentinel)
                                    .map(|c| match c {
                                        Color::White => KeywordAbility::ProtectionFromWhite,
                                        Color::Blue => KeywordAbility::ProtectionFromBlue,
                                        Color::Black => KeywordAbility::ProtectionFromBlack,
                                        Color::Red => KeywordAbility::ProtectionFromRed,
                                        Color::Green => KeywordAbility::ProtectionFromGreen,
                                    });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(kw))
                        }
                        StaticPart::GrantProtectionChosenPlayer => {
                            let kw = world.state.chosen_entry_players.get(&sentinel).map(|pair| {
                                KeywordAbility::ProtectionFrom(ProtectionFromQuality::Player(
                                    pair[0],
                                ))
                            });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(kw))
                        }
                        StaticPart::GrantFixedColorMana(color) => {
                            ResolvedPart::GrantFixedColorMana(*color)
                        }
                        StaticPart::GrantMana(output) => ResolvedPart::GrantMana(output.clone()),
                        StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackSourceController,
                        ) => ResolvedPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackPlayer(controller),
                        ),
                        StaticPart::BlockingRestriction(value) => {
                            ResolvedPart::BlockingRestriction(value.clone())
                        }
                        StaticPart::GrantRestriction(restriction) => {
                            ResolvedPart::GrantRestriction(restriction.clone())
                        }
                        StaticPart::InheritExiledCreatureAbilities
                        | StaticPart::InheritCounteredCreatureAbilities
                        | StaticPart::InheritZoneAbilities { .. } => {
                            return Err(CharacteristicInvariantError::NoEligibleComponent);
                        }
                    });
                }
                match resolved_parts.len() {
                    0 => None,
                    1 => resolved_parts.pop(),
                    _ => Some(ResolvedPart::CompoundAbilities(resolved_parts)),
                }
            };
            (affected, operation)
        }
        InstanceOrigin::Resolved(effect) => {
            let affected: BTreeSet<_> =
                effect.affected.intersection(battlefield).copied().collect();
            let mut matching: Vec<ResolvedPart> = effect
                .parts
                .0
                .iter()
                .filter(|part| part.bucket() == bucket)
                .cloned()
                .collect();
            let operation = match matching.len() {
                0 => None,
                1 => matching.pop(),
                _ => Some(ResolvedPart::CompoundAbilities(matching)),
            };
            (affected, operation)
        }
        InstanceOrigin::Static { source, ability } => {
            // CR 613.6: once this effect starts in an earlier layer it keeps
            // applying to that set even if its generating ability is removed.
            // Otherwise a removed ability cannot generate a later-layer effect.
            let started = matches!(bindings.get(&instance.key), Some(PassBinding::Processed(_)));
            let roster = &objects[&source].abilities;
            if !started
                && roster.continuous != Some(ability.id())
                && !roster
                    .additional_battlefield_abilities
                    .contains(&ability.id())
            {
                return Ok(GeneratedComponent {
                    key: instance.key,
                    bucket,
                    present: false,
                    affected: BTreeSet::new(),
                    generated: BTreeMap::new(),
                });
            }
            // Text substitution changes operands, never the identity or bucket
            // structure of this printed ability. Re-read its actual layer-3 text
            // in dependency probes and later layers, not the frozen definition.
            let text = &objects[&source].printed_text;
            let ability = text
                .continuous
                .iter()
                .chain(&text.additional_battlefield_abilities)
                .find(|current| current.id() == ability.id())
                .ok_or(CharacteristicInvariantError::InvalidStaticParts(source))?;
            let StaticContinuousAbility::Battlefield {
                selector, parts, ..
            } = ability
            else {
                return Err(CharacteristicInvariantError::InvalidStaticParts(source));
            };
            let controller = objects[&source].controller.ok_or(
                CharacteristicInvariantError::MissingBattlefieldController(source),
            )?;
            let affected = match bindings.get(&instance.key) {
                Some(PassBinding::Processed(affected)) => affected.clone(),
                _ if text
                    .static_conditions
                    .get(&ability.id())
                    .is_some_and(|condition| {
                        !static_source_condition_met(
                            condition,
                            world,
                            source,
                            controller,
                            objects,
                            battlefield,
                        )
                    })
                    || selector.source_condition.as_ref().is_some_and(|condition| {
                        !static_source_condition_met(
                            condition,
                            world,
                            source,
                            controller,
                            objects,
                            battlefield,
                        )
                    }) =>
                {
                    BTreeSet::new()
                }
                _ => select_affected(
                    selector,
                    source,
                    controller,
                    objects,
                    battlefield,
                    name_rules,
                    world.combat,
                    Some(world.state),
                ),
            };
            // Empty generated data is independent of unused numeric/source values.
            let operation = if affected.is_empty() {
                None
            } else {
                let mut resolved_parts = Vec::new();
                for part in parts.0.iter().filter(|part| part.bucket() == bucket) {
                    resolved_parts.push(match part {
                        StaticPart::SetPTToOwnManaValue => {
                            let generated = affected
                                .iter()
                                .map(|id| {
                                    let value = &objects[id].defined.mana_value;
                                    (
                                        *id,
                                        ResolvedPart::SetPT(PowerToughness {
                                            power: value.clone(),
                                            toughness: value.clone(),
                                        }),
                                    )
                                })
                                .collect();
                            return Ok(GeneratedComponent {
                                key: instance.key,
                                bucket,
                                present: true,
                                affected,
                                generated,
                            });
                        }
                        StaticPart::ModifyPTPerOwnColor { power, toughness } => {
                            let generated = affected
                                .iter()
                                .map(|id| {
                                    let n =
                                        RulesInteger::from_usize(objects[id].current_colors.len());
                                    (
                                        *id,
                                        ResolvedPart::ModifyPT(PowerToughness {
                                            power: power.times(&n),
                                            toughness: toughness.times(&n),
                                        }),
                                    )
                                })
                                .collect();
                            return Ok(GeneratedComponent {
                                key: instance.key,
                                bucket,
                                present: true,
                                affected,
                                generated,
                            });
                        }
                        StaticPart::ControlSourceController => ResolvedPart::Control(controller),
                        StaticPart::Color(change) => {
                            ResolvedPart::Color(change.map(|operand| match operand {
                                StaticColorOperand::Literal(colors) => colors.clone(),
                                StaticColorOperand::SourceCurrentColors => {
                                    objects[&source].current_colors.clone()
                                }
                            }))
                        }
                        StaticPart::Text(change) => ResolvedPart::Text(*change),
                        StaticPart::Names(change) => {
                            let resolve = |operand: &StaticNameOperand| match operand {
                                StaticNameOperand::Literal(names) => names.clone(),
                                StaticNameOperand::SourceCurrentNames => {
                                    objects[&source].current_names.clone()
                                }
                            };
                            ResolvedPart::Names(match change {
                                NameOperation::Set(names) => NameOperation::Set(resolve(names)),
                                NameOperation::Add(names) => NameOperation::Add(resolve(names)),
                            })
                        }
                        StaticPart::Type(change) => ResolvedPart::Type(TypeChange {
                            land_subtypes: match &change.land_subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(values) => {
                                    SubtypeOperation::Remove(values.clone())
                                }
                                SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                    static_land_subtype(operand, source, objects, world.state),
                                ),
                                SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                    static_land_subtype(operand, source, objects, world.state),
                                ),
                            },
                            supertypes: change.supertypes.clone(),
                            types: change.types.clone(),
                            subtypes: match &change.subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(values) => {
                                    SubtypeOperation::Remove(values.clone())
                                }
                                SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                    static_subtype(operand, source, objects, world.vocabulary),
                                ),
                                SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                    static_subtype(operand, source, objects, world.vocabulary),
                                ),
                            },
                        }),
                        StaticPart::TypeWithSubtypes {
                            base: change,
                            artifact_subtypes,
                            enchantment_subtypes,
                        } => ResolvedPart::TypeWithSubtypes {
                            base: TypeChange {
                                land_subtypes: match &change.land_subtypes {
                                    SubtypeOperation::Keep => SubtypeOperation::Keep,
                                    SubtypeOperation::Remove(values) => {
                                        SubtypeOperation::Remove(values.clone())
                                    }
                                    SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                        static_land_subtype(operand, source, objects, world.state),
                                    ),
                                    SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                        static_land_subtype(operand, source, objects, world.state),
                                    ),
                                },
                                supertypes: change.supertypes.clone(),
                                types: change.types.clone(),
                                subtypes: match &change.subtypes {
                                    SubtypeOperation::Keep => SubtypeOperation::Keep,
                                    SubtypeOperation::Remove(values) => {
                                        SubtypeOperation::Remove(values.clone())
                                    }
                                    SubtypeOperation::Add(operand) => SubtypeOperation::Add(
                                        static_subtype(operand, source, objects, world.vocabulary),
                                    ),
                                    SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                        static_subtype(operand, source, objects, world.vocabulary),
                                    ),
                                },
                            },
                            artifact_subtypes: artifact_subtypes.clone(),
                            enchantment_subtypes: enchantment_subtypes.clone(),
                        },
                        StaticPart::SetPT(pair) => ResolvedPart::SetPT(PowerToughness {
                            power: numeric(
                                world,
                                &pair.power,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
                                world,
                                &pair.toughness,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                        }),
                        StaticPart::ModifyPT(pair) => ResolvedPart::ModifyPT(PowerToughness {
                            power: numeric(
                                world,
                                &pair.power,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
                                world,
                                &pair.toughness,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                        }),
                        StaticPart::SwitchPT => ResolvedPart::SwitchPT,
                        StaticPart::Abilities(change) => ResolvedPart::Abilities(change.clone()),
                        StaticPart::InheritExiledCreatureAbilities => {
                            let donors = world
                                .state
                                .ability_donors_in_exile
                                .get(&source)
                                .into_iter()
                                .flatten()
                                .filter_map(|id| objects.get(id))
                                .filter(|card| {
                                    card.zone == ObjectZone::Exile && card.is_creature()
                                });
                            ResolvedPart::InheritedAbilities(Box::new(
                                super::inheritance::inherit_activated_abilities(
                                    source,
                                    ability.id(),
                                    donors,
                                ),
                            ))
                        }
                        StaticPart::InheritCounteredCreatureAbilities => {
                            ResolvedPart::InheritedAbilities(Box::new(
                                super::inheritance::countered_creature_abilities(
                                    source,
                                    ability.id(),
                                    objects,
                                    battlefield,
                                ),
                            ))
                        }
                        StaticPart::InheritZoneAbilities {
                            zone,
                            filter,
                            loyalty_only,
                        } => {
                            let ctx = super::WorldEvalCtx {
                                active_player: world.active_player,
                                step: world.step,
                                participants: world.participants,
                                life: world.life,
                                combat: world.combat,
                                cards: world.cards,
                                programs: world.programs,
                                state: world.state,
                                objects,
                                source,
                                controller,
                                name_rules: world.name_rules,
                                trigger_subject: None,
                                related_player: None,
                                defending_player: defending_player_for_source(
                                    world.combat,
                                    objects,
                                    source,
                                ),
                            };
                            let linked_ids: BTreeSet<ObjectId> = if *zone == StaticZone::LinkedExile
                            {
                                world
                                    .state
                                    .imprinted_cards
                                    .get(&source)
                                    .map(|v| v.iter().copied().collect())
                                    .unwrap_or_default()
                            } else {
                                BTreeSet::new()
                            };
                            let donors: Vec<&ObjectCharacteristics> = objects
                                .values()
                                .filter(|card| {
                                    card.object != source
                                        && match zone {
                                            StaticZone::ControlledBattlefield => {
                                                card.zone == ObjectZone::Battlefield
                                                    && card.controller == Some(controller)
                                            }
                                            StaticZone::AnyBattlefield => {
                                                card.zone == ObjectZone::Battlefield
                                            }
                                            StaticZone::OwnGraveyard => {
                                                card.zone == ObjectZone::Graveyard(controller)
                                            }
                                            StaticZone::AllGraveyards => {
                                                matches!(card.zone, ObjectZone::Graveyard(_))
                                            }
                                            StaticZone::OwnHand => {
                                                card.zone == ObjectZone::Hand(controller)
                                            }
                                            StaticZone::OwnExile => {
                                                card.zone == ObjectZone::Exile
                                                    && card.owner == controller
                                            }
                                            StaticZone::LinkedExile => {
                                                card.zone == ObjectZone::Exile
                                                    && linked_ids.contains(&card.object)
                                            }
                                        }
                                        && super::world_eval::matches_world_object_filter(
                                            &ctx, **filter, card,
                                        )
                                })
                                .collect();
                            let mut inherited = super::inheritance::inherit_activated_abilities(
                                source,
                                ability.id(),
                                donors,
                            );
                            if *loyalty_only {
                                inherited.activated.retain(|_, (_, terms)| {
                                    terms.loyalty.is_some() || terms.variable_loyalty
                                });
                                inherited.mana.clear();
                            }
                            ResolvedPart::InheritedAbilities(Box::new(inherited))
                        }
                        StaticPart::GrantActivated(ability) => {
                            ResolvedPart::GrantActivated(ability.clone())
                        }
                        StaticPart::GrantActivatedRestricted {
                            ability,
                            restriction,
                        } => ResolvedPart::GrantActivatedRestricted {
                            ability: ability.clone(),
                            restriction: restriction.clone(),
                        },
                        StaticPart::GrantLoyalty {
                            ability,
                            loyalty,
                            variable_loyalty,
                        } => ResolvedPart::GrantLoyalty {
                            ability: ability.clone(),
                            loyalty: *loyalty,
                            variable_loyalty: *variable_loyalty,
                        },
                        StaticPart::GrantTrigger(ability) => {
                            ResolvedPart::GrantTrigger(ability.clone())
                        }
                        StaticPart::GrantAnyColorMana => ResolvedPart::GrantAnyColorMana,
                        StaticPart::GrantSubtypeProtection {
                            types,
                            creatures_only,
                        } => {
                            let qualities =
                                static_subtype(types, source, objects, world.vocabulary)
                                    .into_iter()
                                    .map(|subtype| {
                                        KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                                            subtype,
                                            exclude_subtype: false,
                                            creatures_only: *creatures_only,
                                        })
                                    });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(qualities))
                        }
                        StaticPart::GrantProtectionChosenColor => {
                            let kw =
                                world
                                    .state
                                    .entry_color_choices
                                    .get(&source)
                                    .map(|c| match c {
                                        Color::White => KeywordAbility::ProtectionFromWhite,
                                        Color::Blue => KeywordAbility::ProtectionFromBlue,
                                        Color::Black => KeywordAbility::ProtectionFromBlack,
                                        Color::Red => KeywordAbility::ProtectionFromRed,
                                        Color::Green => KeywordAbility::ProtectionFromGreen,
                                    });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(kw))
                        }
                        StaticPart::GrantProtectionChosenPlayer => {
                            let kw = world.state.chosen_entry_players.get(&source).map(|pair| {
                                KeywordAbility::ProtectionFrom(ProtectionFromQuality::Player(
                                    pair[0],
                                ))
                            });
                            ResolvedPart::Abilities(AbilityChange::grant_keywords(kw))
                        }
                        StaticPart::GrantFixedColorMana(color) => {
                            ResolvedPart::GrantFixedColorMana(*color)
                        }
                        StaticPart::GrantMana(output) => ResolvedPart::GrantMana(output.clone()),
                        StaticPart::BlockingRestriction(
                            BlockingRestriction::BlockerPowerAtLeastSource,
                        ) => ResolvedPart::BlockingRestriction(
                            BlockingRestriction::BlockerPowerAtLeastPowerOf(source),
                        ),
                        StaticPart::BlockingRestriction(BlockingRestriction::CannotBlockSource) => {
                            ResolvedPart::BlockingRestriction(
                                BlockingRestriction::CannotBlockObject(source),
                            )
                        }
                        StaticPart::BlockingRestriction(BlockingRestriction::MustBlockSource) => {
                            ResolvedPart::BlockingRestriction(BlockingRestriction::MustBlockObject(
                                source,
                            ))
                        }
                        StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBeBlockedBySource,
                        ) => ResolvedPart::BlockingRestriction(
                            BlockingRestriction::CannotBeBlockedByObject(source),
                        ),
                        StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackSourceController,
                        ) => ResolvedPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackPlayer(controller),
                        ),
                        StaticPart::BlockingRestriction(value) => {
                            ResolvedPart::BlockingRestriction(value.clone())
                        }
                        StaticPart::GrantRestriction(restriction) => {
                            ResolvedPart::GrantRestriction(restriction.clone())
                        }
                    });
                }
                match resolved_parts.len() {
                    0 => None,
                    1 => resolved_parts.pop(),
                    _ => Some(ResolvedPart::CompoundAbilities(resolved_parts)),
                }
            };
            (affected, operation)
        }
    };
    let generated = match operation {
        // CR 800.4b: the control layer cannot grant control to a departed seat.
        Some(ResolvedPart::Control(controller))
            if !world.participants.is_empty() && !world.participants.contains(&controller) =>
        {
            BTreeMap::new()
        }
        Some(operation) => affected
            .iter()
            .map(|object| (*object, operation.clone()))
            .collect(),
        None => BTreeMap::new(),
    };
    Ok(GeneratedComponent {
        key: instance.key,
        bucket,
        present: true,
        affected,
        generated,
    })
}
fn apply_subtype_operation<T: Ord + Copy>(
    operation: &SubtypeOperation<BTreeSet<T>, T>,
    members: &mut BTreeSet<T>,
) {
    match operation {
        SubtypeOperation::Keep => {}
        SubtypeOperation::Add(values) => members.extend(values),
        SubtypeOperation::Remove(values) => members.retain(|value| !values.contains(value)),
        SubtypeOperation::Set(values) => *members = values.clone(),
    }
}
fn apply_type_operation<T: Copy + Ord>(operation: &TypeOperation<T>, values: &mut BTreeSet<T>) {
    match operation {
        TypeOperation::Keep => {}
        TypeOperation::Add(kinds) => values.extend(kinds.iter().copied()),
        TypeOperation::Remove(kinds) => {
            for kind in kinds {
                values.remove(kind);
            }
        }
        TypeOperation::Set(kinds) => *values = kinds.clone(),
    }
}

fn apply(
    component: &GeneratedComponent,
    objects: &mut BTreeMap<ObjectId, ObjectCharacteristics>,
    speeds: &BTreeMap<SeatId, u8>,
) {
    for (object, operation) in &component.generated {
        let Some(object) = objects.get_mut(object) else {
            continue;
        };
        apply_part(component.key, operation, object, speeds);
    }
}

fn apply_part(
    key: EffectInstanceKey,
    operation: &ResolvedPart,
    object: &mut ObjectCharacteristics,
    speeds: &BTreeMap<SeatId, u8>,
) {
    match operation {
        // Fixed copy snapshots initialize the entire definition before
        // static instances are collected. No later-layer generator emits one.
        ResolvedPart::Copy(_) => unreachable!("copy part reached a later characteristic layer"),
        ResolvedPart::CompoundAbilities(parts) => {
            for part in parts {
                apply_part(key, part, object, speeds);
            }
        }
        ResolvedPart::Control(controller) => {
            object.controller = Some(*controller);
            object.controller_speed = speeds.get(controller).copied().unwrap_or(0);
        }
        ResolvedPart::Color(change) => match change {
            ColorOperation::Set(colors) => object.current_colors = colors.clone(),
            ColorOperation::Add(colors) => object.current_colors.extend(colors),
            ColorOperation::Remove(colors) => object
                .current_colors
                .retain(|color| !colors.contains(color)),
        },
        ResolvedPart::Text(change) => super::text::apply(*change, object),
        ResolvedPart::Names(change) => match change {
            NameOperation::Set(names) => object.current_names = names.clone(),
            NameOperation::Add(names) => object.current_names.add(names),
        },
        ResolvedPart::InheritedAbilities(abilities) => {
            for (k, (definition, terms)) in &abilities.activated {
                object.granted_activated.insert(*k, definition.clone());
                object.inherited_activation_terms.insert(*k, terms.clone());
            }
            object.inherited_mana.extend(abilities.mana.clone());
        }
        ResolvedPart::GrantActivated(ability) => {
            object.granted_activated.insert(key, (**ability).clone());
        }
        ResolvedPart::GrantActivatedRestricted {
            ability,
            restriction,
        } => {
            object.granted_activated.insert(key, (**ability).clone());
            object.inherited_activation_terms.insert(
                key,
                super::ActivationTerms {
                    restriction: Some((**restriction).clone()),
                    loyalty: None,
                    variable_loyalty: false,
                    origin: super::ActivationOriginPath::direct(ability.id, Some(key)),
                },
            );
        }
        ResolvedPart::GrantLoyalty {
            ability,
            loyalty,
            variable_loyalty,
        } => {
            object.granted_activated.insert(key, (**ability).clone());
            object.inherited_activation_terms.insert(
                key,
                super::ActivationTerms {
                    restriction: None,
                    loyalty: *loyalty,
                    variable_loyalty: *variable_loyalty,
                    origin: super::ActivationOriginPath::direct(ability.id, Some(key)),
                },
            );
        }
        ResolvedPart::GrantTrigger(ability) => {
            object
                .granted_triggers
                .entry(key)
                .or_default()
                .push((**ability).clone());
        }
        ResolvedPart::GrantAnyColorMana => {
            object.granted_any_color_mana.insert(key);
        }
        ResolvedPart::GrantFixedColorMana(color) => {
            object.granted_fixed_mana.insert(key, *color);
        }
        ResolvedPart::GrantMana(output) => {
            object.inherited_mana.insert(key, (**output).clone());
        }
        ResolvedPart::BlockingRestriction(value) => {
            object.blocking_restrictions.push(value.clone())
        }
        ResolvedPart::CannotBeTargetedByColors(colors) => {
            object.cannot_be_targeted_by_colors =
                object.cannot_be_targeted_by_colors.union(*colors);
        }
        ResolvedPart::CannotBeTargetedBySpellsOfColors(colors) => {
            object.cannot_be_targeted_by_spells_of_colors =
                object.cannot_be_targeted_by_spells_of_colors.union(*colors);
        }
        ResolvedPart::GrantRestriction(restriction) => {
            object.granted_restrictions.push((**restriction).clone());
        }
        ResolvedPart::Abilities(change) => {
            if change.removal == Some(AbilityRemoval::Flying) {
                object.keywords.remove(&KeywordAbility::Flying);
            } else if let Some(
                removal @ (AbilityRemoval::Banding | AbilityRemoval::BandsWithOther),
            ) = change.removal
            {
                object.keywords.retain(|keyword| {
                    !matches!(keyword, KeywordAbility::BandsWithOther(_))
                        && !(removal == AbilityRemoval::Banding
                            && *keyword == KeywordAbility::Banding)
                });
            } else if let Some(removal) = change.removal {
                object.abilities.remove(removal);
                if removal == AbilityRemoval::All {
                    object.intrinsic_mana.clear();
                    object.granted_any_color_mana.clear();
                    object.granted_fixed_mana.clear();
                    object.inherited_mana.clear();
                }
                object.keywords.clear();
                object.toxic_value = None;
                object.granted_exalted.clear();
                object.granted_unblockable = false;
                object.granted_combat_keywords.clear();
                object.granted_triggers.clear();
                object.granted_activated.clear();
                object.inherited_activation_terms.clear();
                object.granted_restrictions.clear();
            }
            object.granted_unblockable |= change.grants_unblockable;
            object.doesnt_untap_during_untap_step |= change.doesnt_untap;
            if let Some(suppression) = change.suppressed_activations {
                object.suppressed_activations = match (object.suppressed_activations, suppression) {
                    (Some(crate::characteristics::ActivationSuppression::All), _)
                    | (_, crate::characteristics::ActivationSuppression::All) => {
                        Some(crate::characteristics::ActivationSuppression::All)
                    }
                    _ => Some(crate::characteristics::ActivationSuppression::NonMana),
                };
            }
            if !change.granted_triggers.is_empty() {
                object
                    .granted_triggers
                    .entry(key)
                    .or_default()
                    .extend(change.granted_triggers.iter().cloned());
            }
            object
                .suppressed_keywords
                .extend(&change.suppressed_keywords);
            object.keywords.extend(&change.granted_keywords);
            object
                .keywords
                .retain(|keyword| !object.suppressed_keywords.contains(keyword));
            if let Some(amount) = &change.granted_toxic {
                object.toxic_value = Some(
                    object
                        .toxic_value
                        .as_ref()
                        .map_or_else(|| amount.clone(), |total| total.plus(amount)),
                );
            }
            if !change.granted_combat_keywords.is_empty() {
                object
                    .granted_combat_keywords
                    .insert(key, change.granted_combat_keywords.clone());
            }
            if change.granted_exalted != 0 {
                object.granted_exalted.insert(key, change.granted_exalted);
            }
        }
        ResolvedPart::Type(change) | ResolvedPart::TypeWithSubtypes { base: change, .. } => {
            let extra_subtypes = match operation {
                ResolvedPart::TypeWithSubtypes {
                    artifact_subtypes,
                    enchantment_subtypes,
                    ..
                } => Some((artifact_subtypes, enchantment_subtypes)),
                _ => None,
            };
            apply_type_operation(&change.supertypes, &mut object.current_supertypes);
            apply_type_operation(&change.types, &mut object.current_types);
            if !object.has_type(CardType::Instant) && !object.has_type(CardType::Sorcery) {
                object.current_spell_subtypes.clear();
            }
            if !object.has_type(CardType::Artifact) {
                object.current_artifact_subtypes.clear();
            }
            if !object.has_type(CardType::Enchantment) {
                object.current_enchantment_subtypes.clear();
            }
            if !object.carries_creature_subtypes() {
                object.current_creature_subtypes.clear();
            }
            if !object.has_type(CardType::Land) {
                object.current_land_subtypes.clear();
            }
            if !object.has_type(CardType::Planeswalker) {
                object.current_planeswalker_subtypes.clear();
            }
            apply_subtype_operation(&change.subtypes, &mut object.current_creature_subtypes);
            if let Some((art_sub, ench_sub)) = extra_subtypes {
                if object.has_type(CardType::Artifact) {
                    apply_subtype_operation(art_sub, &mut object.current_artifact_subtypes);
                }
                if object.has_type(CardType::Enchantment) {
                    apply_subtype_operation(ench_sub, &mut object.current_enchantment_subtypes);
                }
            }
            // CR 305.7 removes availability, not the retained semantic text
            // used by an already-started static effect (CR 613.6).
            if object.has_type(CardType::Land)
                && matches!(&change.land_subtypes, SubtypeOperation::Set(kinds) if kinds.iter().any(|kind|kind.basic().is_some()))
            {
                object.abilities.remove(AbilityRemoval::All);
                object.granted_any_color_mana.clear();
                object.granted_fixed_mana.clear();
                object.inherited_mana.clear();
                object.keywords.clear();
                object.toxic_value = None;
                object.granted_exalted.clear();
                object.granted_unblockable = false;
                object.granted_combat_keywords.clear();
                object.granted_triggers.clear();
                object.granted_activated.clear();
                object.inherited_activation_terms.clear();
                object.granted_restrictions.clear();
            }
            apply_subtype_operation(&change.land_subtypes, &mut object.current_land_subtypes);
            if !object.has_type(CardType::Land) {
                object.current_land_subtypes.clear();
            }
            object.derive_intrinsic_mana();
            // CR 205.3d is independent of whether a nonbattlefield object
            // retains its printed PT box under CR 208.3.
            if !object.carries_creature_subtypes() {
                object.current_creature_subtypes.clear();
            }
            if object.is_creature()
                || object.zone != ObjectZone::Battlefield && object.defined.printed_pt.is_some()
            {
                if object.power_toughness.is_none() {
                    object.power_toughness = Some(initial_pt(object.object, &object.defined));
                }
            } else {
                object.power_toughness = None;
            }
        }
        ResolvedPart::SetPT(pair) => {
            if let Some(pt) = object.power_toughness.as_mut() {
                pt.base = pair.clone();
                pt.derived = pair.clone();
                pt.applied_effects.push(key);
            }
        }
        ResolvedPart::SetToughness(toughness) => {
            if let Some(pt) = object.power_toughness.as_mut() {
                pt.base.toughness = toughness.clone();
                pt.derived.toughness = toughness.clone();
                pt.applied_effects.push(key);
            }
        }
        ResolvedPart::ModifyPT(pair) => {
            if let Some(pt) = object.power_toughness.as_mut() {
                pt.derived.power = pt.derived.power.plus(&pair.power);
                pt.derived.toughness = pt.derived.toughness.plus(&pair.toughness);
                pt.applied_effects.push(key);
            }
        }
        ResolvedPart::SwitchPT => {
            if let Some(pt) = object.power_toughness.as_mut() {
                std::mem::swap(&mut pt.derived.power, &mut pt.derived.toughness);
                pt.applied_effects.push(key);
            }
        }
    }
}

/// Frozen private later-bucket probe: the caller supplies an explicitly prepared
/// scratch map and binding, without asserting whole-pass reachability.
#[cfg(test)]
pub(crate) fn qualification_static_probe(
    world: CharacteristicWorld<'_>,
    source: ObjectId,
    bucket: CharacteristicBucket,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    binding: PassBinding,
) -> Result<GeneratedComponent, CharacteristicInvariantError> {
    let physical = world
        .cards
        .get(source)
        .filter(|object| object.zone() == ObjectZone::Battlefield)
        .ok_or(CharacteristicInvariantError::InvalidStaticParts(source))?;
    let (program, copy_timestamp) = world
        .state
        .copiable_definition(physical, world.programs)
        .ok_or(CharacteristicInvariantError::MissingDefinition(
            physical.id(),
        ))?;
    let ability = program
        .continuous
        .as_ref()
        .ok_or(CharacteristicInvariantError::InvalidStaticParts(source))?;
    let timestamp = world.state.object_timestamp(source).ok_or(
        CharacteristicInvariantError::MissingBattlefieldTimestamp(source),
    )?;
    let key = EffectInstanceKey::Static {
        source,
        ability: ability.id(),
    };
    let instance = Instance {
        key,
        timestamp: copy_timestamp.unwrap_or(timestamp),
        origin: InstanceOrigin::Static { source, ability },
    };
    let battlefield = world
        .cards
        .iter()
        .filter(|object| object.zone() == ObjectZone::Battlefield)
        .map(|object| object.id())
        .collect();
    generate(
        world,
        instance,
        bucket,
        objects,
        &battlefield,
        &BTreeMap::from([(key, binding)]),
        world.name_rules,
    )
}
