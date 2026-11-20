use super::{ColorOperation, StaticColorOperand, ordering, types::*};
use crate::{
    object_identity::{GameObjects, ObjectId, ObjectZone},
    priority::SeatId,
    program::{CardType, Program},
};
use spellwake_contracts::{RulesInteger, names::NameRules};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum InstanceOrigin<'a> {
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
            InstanceOrigin::Resolved(effect) => {
                effect.parts.0.iter().any(|part| part.bucket() == bucket)
            }
            InstanceOrigin::Static { ability, .. } => match ability {
                StaticContinuousAbility::Battlefield { parts, .. } => {
                    parts.0.iter().any(|part| part.bucket() == bucket)
                }
                StaticContinuousAbility::CharacteristicDefining { .. } => false,
            },
        }
    }
}
fn defined(program: &Program) -> DefinedCharacteristics {
    DefinedCharacteristics {
        names: program.names.clone(),
        supertypes: program.supertypes.clone(),
        types: program.card_types.clone(),
        creature_subtypes: program.creature_subtypes.clone(),
        land_subtypes: program.land_subtypes.clone(),
        colors: program.defined_colors(),
        printed_pt: program
            .permanent()
            .and_then(|value| value.printed_pt.clone()),
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
        let defined = defined(program);
        let is_battlefield = physical.zone() == ObjectZone::Battlefield;
        let power_toughness = (defined.types.contains(&CardType::Creature)
            || !is_battlefield && defined.printed_pt.is_some())
        .then(|| initial_pt(physical.id(), &defined));
        let mut object = ObjectCharacteristics {
            object: physical.id(),
            zone: physical.zone(),
            current_types: defined.types.clone(),
            current_names: defined.names.clone(),
            current_supertypes: defined.supertypes.clone(),
            current_creature_subtypes: defined.creature_subtypes.clone(),
            current_land_subtypes: defined.land_subtypes.clone(),
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
                zone_replacements: program
                    .zone_replacements
                    .iter()
                    .map(|ability| ability.id)
                    .collect(),
                source: physical.id(),
                card: physical.card_id(),
                mana: program.mana.iter().map(|ability| ability.id).collect(),
                activated: program.activated.iter().map(|ability| ability.id).collect(),
                triggered: program.triggered.iter().map(|ability| ability.id).collect(),
                continuous: program.continuous.as_ref().map(StaticContinuousAbility::id),
            },
            printed_text: PrintedText::from_program(program),
            keywords: BTreeSet::new(),
            power_toughness,
        };
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
            if let Some(ability @ StaticContinuousAbility::Battlefield { parts, .. }) =
                &program.continuous
            {
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
        if matches!(instance.origin, InstanceOrigin::Static { .. }) {
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
                        *instance,
                        bucket,
                        &objects,
                        &battlefield,
                        &trace.bindings,
                        world,
                    )
                    .map(|component| (*key, component))
                })
                .collect::<Result<_, _>>()?;
            let mut original_waits = BTreeSet::new();
            for (a_key, a) in &remaining {
                for b_key in remaining.keys().filter(|key| *key != a_key) {
                    let mut probe = objects.clone();
                    apply(&generated[b_key], &mut probe);
                    let after = generate(*a, bucket, &probe, &battlefield, &trace.bindings, world)?;
                    if !generated[a_key].same_semantics(&after) {
                        original_waits.insert(ComponentWait {
                            waiting: *a_key,
                            prerequisite: *b_key,
                        });
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
                apply(component, &mut objects);
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
    Ok(EvaluatedCharacteristics { objects, trace })
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
        let Some(StaticContinuousAbility::CharacteristicDefining { id, parts }) =
            &current.printed_text.continuous
        else {
            continue;
        };
        let Some(part) = parts.0.iter().find(|part| part.bucket() == bucket) else {
            continue;
        };
        let key = EffectInstanceKey::Static {
            source: *source,
            ability: *id,
        };
        let started = matches!(trace.bindings.get(&key), Some(PassBinding::Processed(_)));
        if !started && current.abilities.continuous != Some(*id) {
            continue;
        }
        let operation = match part {
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
                            world
                                .vocabulary
                                .resolve_label(name)
                                .ok_or(CharacteristicInvariantError::InvalidStaticParts(*source))
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
    for component in components {
        trace.bindings.insert(
            component.key,
            PassBinding::Processed(component.affected.clone()),
        );
        apply(&component, objects);
        trace.applied.push(AppliedComponent {
            key: component.key,
            bucket,
            affected: component.affected,
            generated: component.generated,
        });
    }
    Ok(())
}
fn select_affected(
    selector: &PermanentSelector,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    name_rules: &NameRules,
) -> BTreeSet<ObjectId> {
    battlefield
        .iter()
        .copied()
        .filter(|id| {
            let object = &objects[id];
            (!selector.exclude_source || *id != source)
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
                && selector
                    .forbidden_types
                    .iter()
                    .all(|kind| !object.has_type(*kind))
                && selector
                    .creature_subtype
                    .is_none_or(|kind| object.current_creature_subtypes.contains(&kind))
                && selector
                    .land_subtype
                    .is_none_or(|kind| object.current_land_subtypes.contains(&kind))
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
                && (selector.controller == ControllerSelector::Any
                    || object.controller == Some(controller))
        })
        .collect()
}
fn numeric(
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
        )
        .times(multiplier)
        .plus(offset),
    }
}
fn count_matching(
    selector: &PermanentSelector,
    source: ObjectId,
    controller: SeatId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    name_rules: &NameRules,
) -> RulesInteger {
    select_affected(
        selector,
        source,
        controller,
        objects,
        battlefield,
        name_rules,
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
    )
}
fn static_subtype(
    operand: &StaticSubtypeOperand,
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
) -> BTreeSet<spellwake_contracts::creature_types::CreatureTypeId> {
    match operand {
        StaticSubtypeOperand::Literal(value) => value.clone(),
        StaticSubtypeOperand::SourceCurrentSubtypes => {
            objects[&source].current_creature_subtypes.clone()
        }
    }
}
fn static_land_subtype(
    operand: &StaticLandSubtypeOperand,
    source: ObjectId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
) -> BTreeSet<crate::land::LandType> {
    match operand {
        StaticLandSubtypeOperand::Literal(value) => value.clone(),
        StaticLandSubtypeOperand::SourceCurrentLandSubtypes => {
            objects[&source].current_land_subtypes.clone()
        }
    }
}
fn generate(
    instance: Instance<'_>,
    bucket: CharacteristicBucket,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
    bindings: &BTreeMap<EffectInstanceKey, PassBinding>,
    world: CharacteristicWorld<'_>,
) -> Result<GeneratedComponent, CharacteristicInvariantError> {
    let name_rules = world.name_rules;
    let (affected, operation) = match instance.origin {
        InstanceOrigin::Resolved(effect) => {
            let affected: BTreeSet<_> =
                effect.affected.intersection(battlefield).copied().collect();
            let operation = effect
                .parts
                .0
                .iter()
                .find(|part| part.bucket() == bucket)
                .cloned();
            (affected, operation)
        }
        InstanceOrigin::Static { source, ability } => {
            // CR 613.6: once this effect starts in an earlier layer it keeps
            // applying to that set even if its generating ability is removed.
            // Otherwise a removed ability cannot generate a later-layer effect.
            let started = matches!(bindings.get(&instance.key), Some(PassBinding::Processed(_)));
            // Layer dependency probes must see the same control prohibition as
            // the final application. A change in the source's own controller
            // can make its static effect applicable again during this evaluation.
            let grants_departed_control = matches!(ability, StaticContinuousAbility::Battlefield { parts, .. }
                if parts.0.contains(&StaticPart::ControlSourceController))
                && objects[&source]
                    .controller
                    .is_some_and(|seat| world.state.departed_controllers.contains(&seat));
            if grants_departed_control
                || (!started && objects[&source].abilities.continuous != Some(ability.id()))
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
            let ability = objects[&source]
                .printed_text
                .continuous
                .as_ref()
                .filter(|current| current.id() == ability.id())
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
                _ => select_affected(
                    selector,
                    source,
                    controller,
                    objects,
                    battlefield,
                    name_rules,
                ),
            };
            // Empty generated data is independent of unused numeric/source values.
            let operation = if affected.is_empty() {
                None
            } else {
                parts
                    .0
                    .iter()
                    .find(|part| part.bucket() == bucket)
                    .map(|part| match part {
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
                                    static_land_subtype(operand, source, objects),
                                ),
                                SubtypeOperation::Set(operand) => SubtypeOperation::Set(
                                    static_land_subtype(operand, source, objects),
                                ),
                            },
                            supertypes: change.supertypes.clone(),
                            types: change.types.clone(),
                            subtypes: match &change.subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(values) => {
                                    SubtypeOperation::Remove(values.clone())
                                }
                                SubtypeOperation::Add(operand) => {
                                    SubtypeOperation::Add(static_subtype(operand, source, objects))
                                }
                                SubtypeOperation::Set(operand) => {
                                    SubtypeOperation::Set(static_subtype(operand, source, objects))
                                }
                            },
                        }),
                        StaticPart::SetPT(pair) => ResolvedPart::SetPT(PowerToughness {
                            power: numeric(
                                &pair.power,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
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
                                &pair.power,
                                source,
                                controller,
                                objects,
                                battlefield,
                                name_rules,
                            ),
                            toughness: numeric(
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
                    })
            };
            (affected, operation)
        }
    };
    let generated = match operation {
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

fn apply(component: &GeneratedComponent, objects: &mut BTreeMap<ObjectId, ObjectCharacteristics>) {
    for (object, operation) in &component.generated {
        let Some(object) = objects.get_mut(object) else {
            continue;
        };
        match operation {
            // Fixed copy snapshots initialize the entire definition before
            // static instances are collected. No later-layer generator emits one.
            ResolvedPart::Copy(_) => unreachable!("copy part reached a later characteristic layer"),
            ResolvedPart::Control(controller) => object.controller = Some(*controller),
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
            ResolvedPart::Abilities(change) => {
                if let Some(removal) = change.removal {
                    object.abilities.remove(removal);
                    if removal == AbilityRemoval::All {
                        object.intrinsic_mana.clear();
                    }
                    object.keywords.clear();
                }
                object.keywords.extend(&change.granted_keywords);
            }
            ResolvedPart::Type(change) => {
                apply_type_operation(&change.supertypes, &mut object.current_supertypes);
                apply_type_operation(&change.types, &mut object.current_types);
                if !object.carries_creature_subtypes() {
                    object.current_creature_subtypes.clear();
                }
                if !object.has_type(CardType::Land) {
                    object.current_land_subtypes.clear();
                }
                apply_subtype_operation(&change.subtypes, &mut object.current_creature_subtypes);
                // CR 305.7 removes availability, not the retained semantic text
                // used by an already-started static effect (CR 613.6).
                if object.has_type(CardType::Land)
                    && matches!(&change.land_subtypes, SubtypeOperation::Set(kinds) if kinds.iter().any(|kind|kind.basic().is_some()))
                {
                    object.abilities.remove(AbilityRemoval::All);
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
                    pt.applied_effects.push(component.key);
                }
            }
            ResolvedPart::ModifyPT(pair) => {
                if let Some(pt) = object.power_toughness.as_mut() {
                    pt.derived.power = pt.derived.power.plus(&pair.power);
                    pt.derived.toughness = pt.derived.toughness.plus(&pair.toughness);
                    pt.applied_effects.push(component.key);
                }
            }
            ResolvedPart::SwitchPT => {
                if let Some(pt) = object.power_toughness.as_mut() {
                    std::mem::swap(&mut pt.derived.power, &mut pt.derived.toughness);
                    pt.applied_effects.push(component.key);
                }
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
        instance,
        bucket,
        objects,
        &battlefield,
        &BTreeMap::from([(key, binding)]),
        world,
    )
}
