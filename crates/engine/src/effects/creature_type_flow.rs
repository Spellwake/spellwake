//! Resolution-local choices and once-current controlled-permanent counts.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

impl EffectWindow {
    pub(super) fn creature_type_frame(&self) -> Result<CreatureTypeChoice, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        let (into, chooser, excluding, previously_named) = match self.current_instruction()? {
            Instruction::ChooseCreatureType { into, excluding } => {
                (*into, entry.controller(), excluding.clone(), Vec::new())
            }
            Instruction::ChooseCreatureTypeForPlayer { player, into } => (
                *into,
                *player,
                BTreeSet::new(),
                cursor.values.values().copied().collect(),
            ),
            _ => return Err(EffectError::WrongCreatureTypeFrame),
        };
        if cursor.entry != entry.id() || cursor.values.contains_key(&into) {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(CreatureTypeChoice {
            revision: self.revision,
            entry: entry.id(),
            instruction: cursor.instruction,
            slot: into,
            chooser,
            vocabulary: self.creature_vocabulary.id(),
            excluding,
            previously_named,
        })
    }

    pub(super) fn expand_type_choices_for_table(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
    ) -> Result<(), EffectError> {
        let mut operations = self
            .window
            .apnap_players()?
            .enumerate()
            .map(|(index, player)| {
                let slot = u16::try_from(index).map_err(|_| EffectError::InconsistentProgram)?;
                Ok(Instruction::ChooseCreatureTypeForPlayer {
                    player,
                    into: CreatureTypeSlot::new(slot),
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        operations.push(Instruction::DestroyGroupWithoutRegeneration(Box::new(
            crate::program::ObjectFilter::creature().outside_named_slots(),
        )));
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) else {
            return Err(EffectError::InconsistentProgram);
        };
        payload.expand_instruction(instruction, false, operations);
        Ok(())
    }

    /// The production one-instruction acceptance step. The caller then runs the
    /// existing iterative driver; no priority or checks occur between these steps.
    pub(crate) fn accept_creature_type(
        &self,
        expected: u64,
        bound: &CreatureTypeChoice,
        actor: SeatId,
        selected: CreatureTypeId,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        self.ensure_resolution_or_idle()?;
        let revision = self.next_revision(expected)?;
        if &self.creature_type_frame()? != bound {
            return Err(EffectError::WrongCreatureTypeFrame);
        }
        if actor != bound.chooser {
            return Err(EffectError::WrongChooser);
        }
        if self.creature_vocabulary.label(selected).is_none() || bound.excluding.contains(&selected)
        {
            return Err(EffectError::IllegalCreatureType);
        }
        let before = self.rules_snapshot(None)?;
        let mut next = self.clone();
        next.revision = revision;
        let cursor = next
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        if cursor.values.insert(bound.slot, selected).is_some() {
            return Err(EffectError::InconsistentProgram);
        }
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::Instruction {
                entry: bound.entry,
                instruction: bound.instruction,
            },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::CreatureTypeChosen {
                    entry: bound.entry,
                    instruction: bound.instruction,
                    slot: bound.slot,
                    chooser: actor,
                    value: selected,
                }])
            },
        )?;
        next.advance_instruction()?;
        Ok(RulesTransition::new(next, (), append))
    }

    pub(super) fn resolve_value_expr(
        &self,
        controller: SeatId,
        expr: &crate::program::ValueExpr,
    ) -> Result<RulesInteger, EffectError> {
        self.resolve_value_expr_at(controller, expr, self.resolution_cursor())
    }
    pub(super) fn resolve_value_expr_at(
        &self,
        controller: SeatId,
        expr: &crate::program::ValueExpr,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<RulesInteger, EffectError> {
        use crate::program::{
            AttachedCountKind, CounterSubjectRef, EventValueKind, MatchObjectRef, ObjectStatKind,
            PlayerStatExpr, StatAggregate, SubtypeFilter, ValueExpr, ValueObjectRef, ValueZone,
        };
        let resolve_ability_source = || -> Option<ObjectId> {
            if let Some(binding) = cursor
                .and_then(|c| self.execution_bindings(c.entry))
                .and_then(|b| b.trigger.as_ref())
            {
                return Some(binding.source.object.id());
            }
            match self
                .window
                .resolving()
                .map(crate::stack_window::StackEntry::kind)
            {
                Some(
                    StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source },
                ) => Some(source.id()),
                _ => None,
            }
        };
        match expr {
            ValueExpr::Literal(value) => Ok(value.clone()),
            ValueExpr::Scale { expr, factor } => Ok(self
                .resolve_value_expr_at(controller, expr, cursor)?
                .times(factor)),
            ValueExpr::DivConst {
                expr,
                divisor,
                round_up,
            } => {
                let value = self.resolve_value_expr_at(controller, expr, cursor)?;
                let d = usize::from(divisor.get());
                if d == 2 {
                    Ok(if *round_up {
                        value.nonnegative_half_rounded_up()
                    } else {
                        value.nonnegative_half_rounded_down()
                    })
                } else if let Some(n) = value.to_usize() {
                    let result = if *round_up { n.div_ceil(d) } else { n / d };
                    Ok(RulesInteger::from_usize(result))
                } else {
                    Err(EffectError::InconsistentProgram)
                }
            }
            ValueExpr::Conditional {
                left,
                comparison,
                right,
                then_value,
                else_value,
            } => {
                let lhs = self.resolve_value_expr_at(controller, left, cursor)?;
                let rhs = self.resolve_value_expr_at(controller, right, cursor)?;
                if comparison.evaluate(&lhs, &rhs) {
                    self.resolve_value_expr_at(controller, then_value, cursor)
                } else {
                    self.resolve_value_expr_at(controller, else_value, cursor)
                }
            }
            ValueExpr::Count {
                zone,
                filter,
                controller_scope,
            } => {
                let mut bound_filter = **filter;
                if let Some(slot) = bound_filter.chosen_type_slot() {
                    let selected = cursor
                        .and_then(|c| c.values.get(&slot))
                        .copied()
                        .ok_or(EffectError::InconsistentProgram)?;
                    if self.creature_vocabulary.label(selected).is_none() {
                        return Err(EffectError::InconsistentProgram);
                    }
                    bound_filter = bound_filter.bind_matching_chosen_slot(slot, selected);
                }
                let ability_source = if bound_filter.excludes_ability_source() {
                    let source =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    Some(source)
                } else {
                    None
                };
                let excluded = BTreeSet::new();
                match *zone {
                    ValueZone::Battlefield => {
                        if let Some(role) = controller_scope {
                            let seat = self.resolving_player_at(*role, cursor)?;
                            bound_filter = bound_filter.of_player(seat);
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|object| {
                                object.zone == ObjectZone::Battlefield
                                    && self.matches_object_filter(
                                        bound_filter,
                                        object,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Graveyard(player_role) => {
                        let seat = self.resolving_player_at(player_role, cursor)?;
                        if bound_filter == crate::program::ObjectFilter::any() {
                            return Ok(RulesInteger::from_usize(
                                self.window.cards().graveyard(seat).len(),
                            ));
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Graveyard(seat)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Hand(player_role) => {
                        let seat = self.resolving_player_at(player_role, cursor)?;
                        if bound_filter == crate::program::ObjectFilter::any() {
                            return Ok(RulesInteger::from_usize(
                                self.window
                                    .cards()
                                    .iter()
                                    .filter(|object| object.zone() == ObjectZone::Hand(seat))
                                    .count(),
                            ));
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Hand(seat)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                }
            }
            ValueExpr::CountOtherSharingSourceSubtype => {
                let trigger = cursor
                    .and_then(|c| self.execution_bindings(c.entry))
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                let source = trigger.source.object.id();
                let evaluated = self.evaluated_characteristics()?;
                let subtypes = evaluated
                    .get(source)
                    .filter(|object| object.zone == ObjectZone::Battlefield)
                    .map(|object| &object.current_creature_subtypes)
                    .or_else(|| {
                        self.departed_damage_sources
                            .get(&source)
                            .map(|source| &source.creature_subtypes)
                    });
                let count = evaluated
                    .objects
                    .values()
                    .filter(|object| {
                        object.object != source
                            && object.zone == ObjectZone::Battlefield
                            && object.controller == Some(controller)
                            && object.is_creature()
                            && subtypes.is_some_and(|types| {
                                !object.current_creature_subtypes.is_disjoint(types)
                            })
                    })
                    .count();
                Ok(RulesInteger::from_usize(count))
            }
            ValueExpr::CountAttackingController => Ok(RulesInteger::from_usize(
                self.combat.as_ref().map_or(0, |combat| {
                    combat
                        .attackers()
                        .filter(|attacker| {
                            attacker.defender() == controller
                                && attacker.defending_planeswalker().is_none()
                        })
                        .count()
                }),
            )),
            ValueExpr::Aggregate { filter, stat, op } => {
                let ability_source = if filter.excludes_ability_source() {
                    let source =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    Some(source)
                } else {
                    None
                };
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let values = evaluated.objects.values().filter_map(|object| {
                    if object.zone != ObjectZone::Battlefield
                        || !self.matches_object_filter(
                            **filter,
                            object,
                            controller,
                            ability_source,
                            &excluded,
                            &evaluated,
                        )
                    {
                        return None;
                    }
                    match stat {
                        ObjectStatKind::Power => object
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone()),
                        ObjectStatKind::Toughness => object
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.toughness.clone()),
                        ObjectStatKind::ManaValue => self.permanent_mana_value(object.object),
                    }
                });
                match op {
                    StatAggregate::Max => Ok(values.max().unwrap_or_else(|| 0.into())),
                    StatAggregate::Sum => {
                        Ok(values.fold(0.into(), |total: RulesInteger, v| total.plus(&v)))
                    }
                }
            }
            ValueExpr::ObjectStat { subject, stat } => match (subject, stat) {
                (ValueObjectRef::AbilitySource, ObjectStatKind::Power) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let (StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source }) = entry.kind()
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(self
                        .current_characteristics(source.id())?
                        .and_then(|value| value.power_toughness)
                        .map_or_else(|| 0.into(), |pt| pt.derived.power))
                }
                (ValueObjectRef::TriggerSource, ObjectStatKind::Power) => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let id = trigger.source.object.id();
                    let current = self.evaluated_characteristics()?;
                    let power = if let Some(live) = current
                        .get(id)
                        .filter(|o| o.zone == ObjectZone::Battlefield)
                    {
                        live.power_toughness.as_ref().map(|pt| &pt.derived.power)
                    } else {
                        self.departed_reflexive_sources
                            .get(&id)
                            .unwrap_or(&trigger.source)
                            .current_power_toughness
                            .as_ref()
                            .map(|pt| &pt.power)
                    };
                    Ok(power.cloned().unwrap_or_else(|| 0.into()).max(0.into()))
                }
                (
                    ValueObjectRef::TargetObject,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let stats = self
                        .current_characteristics(id)?
                        .and_then(|value| value.power_toughness)
                        .ok_or(EffectError::InconsistentProgram)?
                        .derived;
                    Ok(if *stat == ObjectStatKind::Power {
                        stats.power
                    } else {
                        stats.toughness
                    })
                }
                (ValueObjectRef::TargetObject, ObjectStatKind::ManaValue) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    self.copiable_program(id)
                        .map(Program::mana_value)
                        .ok_or(EffectError::InconsistentProgram)
                }
                (
                    ValueObjectRef::DyingSource | ValueObjectRef::DyingCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &cause.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let raw = before.current_power_toughness.as_ref().map_or_else(
                        || 0.into(),
                        |pt| {
                            if *stat == ObjectStatKind::Power {
                                pt.power.clone()
                            } else {
                                pt.toughness.clone()
                            }
                        },
                    );
                    Ok(raw.max(0.into()))
                }
                (ValueObjectRef::DestructionTarget, ObjectStatKind::Toughness) => Ok(cursor
                    .and_then(|c| c.results.destruction_target_toughness.clone())
                    .unwrap_or_else(|| 0.into())
                    .max(0.into())),
                (
                    ValueObjectRef::LastSacrificedCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => Ok(cursor
                    .and_then(|c| c.results.last_sacrificed_creature.as_ref())
                    .map_or_else(
                        || 0.into(),
                        |stats| {
                            let value = if *stat == ObjectStatKind::Power {
                                &stats.power
                            } else {
                                &stats.toughness
                            };
                            value.clone().max(0.into())
                        },
                    )),
                (
                    ValueObjectRef::SacrificedCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let value = if *stat == ObjectStatKind::Power {
                        &payload.sacrificed_creature_power
                    } else {
                        &payload.sacrificed_creature_toughness
                    };
                    Ok(value
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone()
                        .max(0.into()))
                }
                (ValueObjectRef::SacrificedPermanent, ObjectStatKind::ManaValue) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(payload
                        .sacrificed_permanent_mana_value
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone()
                        .max(0.into()))
                }
                (ValueObjectRef::ExiledCostCreature, ObjectStatKind::Power) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let Some((object, last_power)) = &payload.exiled_cost_creature else {
                        return Ok(0.into());
                    };
                    let power = self
                        .current_characteristics(*object)?
                        .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                        .unwrap_or_else(|| last_power.clone());
                    Ok(power.max(0.into()))
                }
                (ValueObjectRef::TriggeringSpell, ObjectStatKind::ManaValue) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::SpellCast(spell) = &cause.related else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(spell.mana_value().clone())
                }
                (ValueObjectRef::FinalSaga, ObjectStatKind::ManaValue) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::FinalSagaChapter { mana_value, .. } =
                        &cause.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(mana_value.clone())
                }
                _ => Err(EffectError::InconsistentProgram),
            },
            ValueExpr::CounterCount { subject, kind } => match subject {
                CounterSubjectRef::AbilitySource => {
                    let kind = (*kind).ok_or(EffectError::InconsistentProgram)?;
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    self.source_counter_value_at_resolution(entry, kind)
                }
                CounterSubjectRef::TriggerSource => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let id = trigger.source.object.id();
                    if self
                        .evaluated_characteristics()?
                        .get(id)
                        .is_some_and(|object| object.zone == ObjectZone::Battlefield)
                    {
                        return Ok(match kind {
                            Some(k) => self.counters_of_kind(id, *k),
                            None => self.characteristics.total_permanent_counters(id),
                        });
                    }
                    let appearance = self
                        .departed_reflexive_sources
                        .get(&id)
                        .unwrap_or(&trigger.source);
                    Ok(match kind {
                        Some(CounterKind::PlusOne) => appearance
                            .battlefield_counters
                            .as_ref()
                            .map_or_else(|| 0.into(), |c| c.plus_one.clone()),
                        None => appearance
                            .battlefield_total_counters
                            .clone()
                            .unwrap_or_else(|| 0.into()),
                        _ => 0.into(),
                    })
                }
                CounterSubjectRef::DyingSource => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    match binding.causes.members() {
                        [member] => match &member.related {
                            crate::occurrence::TriggerRelated::ZoneChange { before, .. }
                                if before.object.id() == binding.source.object.id() =>
                            {
                                Ok(match kind {
                                    None => before
                                        .battlefield_total_counters
                                        .clone()
                                        .unwrap_or_else(|| 0.into()),
                                    Some(CounterKind::PlusOne) => before
                                        .battlefield_counters
                                        .as_ref()
                                        .map_or_else(|| 0.into(), |c| c.plus_one.clone()),
                                    _ => 0.into(),
                                })
                            }
                            _ => Err(EffectError::InconsistentProgram),
                        },
                        _ => Err(EffectError::InconsistentProgram),
                    }
                }
                CounterSubjectRef::ControllerExperience => Ok(self.experience_counters(controller)),
            },
            ValueExpr::AttachedCount(kind) => match kind {
                AttachedCountKind::EquipmentOnSource
                | AttachedCountKind::AurasAndEquipmentOnSource => {
                    let both = matches!(kind, AttachedCountKind::AurasAndEquipmentOnSource);
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let source = binding.source.object.id();
                    let current = self.evaluated_characteristics()?;
                    if current
                        .get(source)
                        .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        Ok(if both {
                            current.auras_and_equipment_attached_to(source)
                        } else {
                            current.equipment_attached_to(source)
                        })
                    } else {
                        Ok(self.departed_damage_sources.get(&source).map_or_else(
                            || 0.into(),
                            |p| {
                                if both {
                                    p.attached_auras_and_equipment.clone()
                                } else {
                                    p.attached_equipment.clone()
                                }
                            },
                        ))
                    }
                }
                AttachedCountKind::CursesOnRelatedPlayer => {
                    let player = self.resolving_player_at(
                        crate::program::ResolvingPlayer::UniqueRelatedPlayer,
                        cursor,
                    )?;
                    let world = self.evaluated_characteristics()?;
                    let count = world
                        .objects
                        .values()
                        .filter(|object| {
                            object.zone == ObjectZone::Battlefield
                                && object.attached_to == Some(TargetRef::Player(player))
                                && object
                                    .current_enchantment_subtypes
                                    .contains(&crate::program::EnchantmentSubtype::Curse)
                        })
                        .count();
                    Ok(RulesInteger::from_usize(count))
                }
            },
            ValueExpr::PlayerStat(stat) => match stat {
                PlayerStatExpr::ControllerLife => self
                    .life
                    .get(controller)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram),
                PlayerStatExpr::ControllerLifeAboveStarting => {
                    let starting = self
                        .life
                        .starting_total(controller)
                        .ok_or(EffectError::InvalidResolvingPlayer)?;
                    let current = self
                        .life
                        .get(controller)
                        .ok_or(EffectError::InvalidResolvingPlayer)?;
                    Ok(current.minus(starting))
                }
                PlayerStatExpr::OpponentsLifeLostThisTurn => Ok(self
                    .characteristics
                    .life_lost_this_turn
                    .iter()
                    .filter(|(player, _)| **player != controller)
                    .fold(0.into(), |total: RulesInteger, (_, loss)| total.plus(loss))),
                PlayerStatExpr::CardsDrawnThisTurn => Ok(self
                    .cards_drawn_this_turn
                    .get(&controller)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::CardsShortOfHandSize(required) => {
                    let count = self
                        .window
                        .cards()
                        .iter()
                        .filter(|o| o.zone() == ObjectZone::Hand(controller))
                        .count();
                    Ok(RulesInteger::from_usize(
                        usize::from(*required).saturating_sub(count),
                    ))
                }
                PlayerStatExpr::LargestHandSize => {
                    let mut sizes = BTreeMap::new();
                    for object in self.window.cards().iter() {
                        if let ObjectZone::Hand(player) = object.zone() {
                            *sizes.entry(player).or_insert(0usize) += 1;
                        }
                    }
                    Ok(RulesInteger::from_usize(
                        sizes.into_values().max().unwrap_or(0),
                    ))
                }
                PlayerStatExpr::ControllerIsMonarch => {
                    Ok(i64::from(self.characteristics.monarch == Some(controller)).into())
                }
                PlayerStatExpr::ControlsGreatestCreature => {
                    let evaluated = self.evaluated_characteristics()?;
                    let creatures = evaluated
                        .objects
                        .values()
                        .filter(|p| p.zone == ObjectZone::Battlefield && p.is_creature())
                        .filter_map(|p| {
                            p.power_toughness
                                .as_ref()
                                .map(|pt| (&pt.derived.power, p.controller == Some(controller)))
                        });
                    Ok(i64::from(creatures.max().is_some_and(|(_, c)| c)).into())
                }
            },
            ValueExpr::ObjectMatches { subject, filter } => {
                let (clause_a, _) = filter.clauses();
                let matched = match subject {
                    MatchObjectRef::RevealedTop => cursor.is_some_and(|c| {
                        clause_a
                            .any_types
                            .matches_any(&c.results.revealed_top_types)
                            && !c.results.revealed_top_types.is_empty()
                    }),
                    MatchObjectRef::DiscardedCard => cursor.is_some_and(|c| {
                        clause_a.any_types.matches_any(&c.results.discarded_types)
                            && !c.results.discarded_types.is_empty()
                    }),
                    MatchObjectRef::MovedLibraryCard => {
                        let c = cursor.ok_or(EffectError::WrongResolutionFrame)?;
                        clause_a
                            .any_types
                            .matches_any(&c.results.moved_library_types)
                            && !c.results.moved_library_types.is_empty()
                    }
                    MatchObjectRef::DestructionTarget => {
                        let SubtypeFilter::Has(kind) = clause_a.subtypes else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        cursor
                            .is_some_and(|c| c.results.destruction_target_subtypes.contains(&kind))
                    }
                    MatchObjectRef::DyingCreature => {
                        let SubtypeFilter::Has(kind) = clause_a.subtypes else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let binding = self
                            .window
                            .resolving()
                            .and_then(|entry| self.execution_bindings(entry.id()))
                            .and_then(|bindings| bindings.trigger.as_ref())
                            .ok_or(EffectError::InconsistentProgram)?;
                        let [member] = binding.causes.members() else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                            &member.related
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        before.is_creature() && before.creature_subtypes.contains(&kind)
                    }
                    MatchObjectRef::SacrificedCreature => {
                        let SubtypeFilter::Has(kind) = clause_a.subtypes else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let entry = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get(&entry.id())
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        payload.sacrificed_creature_subtypes.contains(&kind)
                    }
                    MatchObjectRef::TargetObject => {
                        let entry = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let TargetRef::Object(object) =
                            self.instruction_target(entry)?.choice().target
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        evaluated.get(object).is_some_and(|permanent| {
                            permanent.zone == ObjectZone::Battlefield
                                && self.matches_object_filter(
                                    **filter, permanent, controller, None, &excluded, &evaluated,
                                )
                        })
                    }
                };
                Ok(i64::from(matched).into())
            }
            ValueExpr::EventValue(kind) => match kind {
                EventValueKind::ChosenX => {
                    let entry = self
                        .window
                        .resolving()
                        .filter(|entry| cursor.is_none_or(|c| c.entry == entry.id()))
                        .ok_or(EffectError::InconsistentProgram)?;
                    Ok(entry.chosen_x().cloned().unwrap_or_else(|| 0.into()))
                }
                EventValueKind::EnteredKickerX => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(before.spell_x.clone().unwrap_or_else(|| 0.into()))
                }
                EventValueKind::KickerCount(index) => Ok(self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?
                    .kicker_choices()
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::PaidAlternative => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .paid_conditional_cost(),
                )
                .into()),
                EventValueKind::Bargained => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .bargained(),
                )
                .into()),
                EventValueKind::CastDuringOwnMain => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .controller_cast_during_own_main(),
                )
                .into()),
                EventValueKind::CastFromGraveyard => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .cast_from_graveyard,
                )
                .into()),
                EventValueKind::MatchedOccurrences => {
                    self.resolution_trigger_count_at(controller, cursor)
                }
                EventValueKind::PaidCreatureTaps => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(RulesInteger::from_usize(payload.paid_creature_taps))
                }
                EventValueKind::PaidNamedCounters => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    payload
                        .paid_named_counters
                        .clone()
                        .ok_or(EffectError::InconsistentProgram)
                }
                EventValueKind::LargestDiscard => cursor
                    .and_then(|c| c.largest_discard)
                    .map(RulesInteger::from_usize)
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::DestroyedPermanents => cursor
                    .and_then(|c| c.destroyed_permanents)
                    .map(RulesInteger::from_usize)
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::ExiledMilledCreaturePower => cursor
                    .and_then(|c| c.impulse.exiled_milled_power.clone())
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::MilledCardsManaValueSum => {
                    let values = &cursor
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .results
                        .milled_mana_values;
                    Ok(values
                        .iter()
                        .fold(0.into(), |total: RulesInteger, value| total.plus(value)))
                }
                EventValueKind::MilledCardsManaValueMax => {
                    let values = &cursor
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .results
                        .milled_mana_values;
                    Ok(values.iter().max().cloned().unwrap_or_else(|| 0.into()))
                }
                EventValueKind::DeclaredSubtypeAttackers => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::DeclaredSubtypeAttackers(attackers) =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(RulesInteger::from_usize(attackers.len()))
                }
                EventValueKind::DeclaredAttackersAtController => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .id();
                    let binding = self
                        .execution_bindings(entry)
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::AttackGroup { creatures, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let count = creatures
                        .iter()
                        .filter(|object| {
                            let current = self
                                .combat
                                .as_ref()
                                .and_then(|combat| combat.attacker(**object))
                                .map(|attacker| attacker.defender());
                            current.or_else(|| {
                                self.departed_attack_defenders
                                    .as_ref()
                                    .and_then(|values| values.get(*object).map(|(seat, _)| *seat))
                            }) == Some(controller)
                        })
                        .count();
                    Ok(RulesInteger::from_usize(count))
                }
                EventValueKind::TriggeringLifeChange => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = trigger.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::LifeChanged(change) = &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(change.amount.clone())
                }
                EventValueKind::CombatDamageToPlayer => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    match binding.causes.members() {
                        [member] => match &member.related {
                            crate::occurrence::TriggerRelated::CombatDamageToPlayer {
                                amount,
                                ..
                            } => Ok(amount.clone()),
                            _ => Err(EffectError::InconsistentProgram),
                        },
                        _ => Err(EffectError::InconsistentProgram),
                    }
                }
                EventValueKind::DamageReceived => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    binding.causes.members().iter().try_fold(
                        RulesInteger::from(0),
                        |total, member| match &member.related {
                            crate::occurrence::TriggerRelated::DamageReceived {
                                amount, ..
                            } => Ok(total.plus(amount)),
                            _ => Err(EffectError::InconsistentProgram),
                        },
                    )
                }
                EventValueKind::CreaturesDiedThisTurn => Ok(self.creatures_died_this_turn.clone()),
                EventValueKind::LastSacrificeSucceeded => {
                    Ok(i64::from(cursor.is_some_and(|c| c.results.last_sacrifice)).into())
                }
                EventValueKind::SelectedGraveyardExileCreatures => {
                    Ok(RulesInteger::from_usize(cursor.map_or(0, |c| {
                        c.results.selected_graveyard_exile_creatures
                    })))
                }
                EventValueKind::SourceAttackedThisTurn => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let binding = self
                        .execution_bindings(entry.id())
                        .and_then(|binding| binding.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    Ok(
                        i64::from(self.triggers.attacked_this_turn(binding.source.object.id()))
                            .into(),
                    )
                }
                EventValueKind::TriggerSourceOnBattlefield => {
                    let Some(StackKind::TriggeredAbility { source }) = self
                        .window
                        .resolving()
                        .map(crate::stack_window::StackEntry::kind)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(i64::from(
                        self.evaluated_characteristics()?
                            .get(source.id())
                            .is_some_and(|object| object.zone == ObjectZone::Battlefield),
                    )
                    .into())
                }
            },
        }
    }
}
