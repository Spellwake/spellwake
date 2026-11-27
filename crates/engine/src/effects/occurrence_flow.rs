//! Actual-cut snapshots and the single local occurrence/discovery publisher.
use super::*;
use crate::{
    combat::Combatant,
    object_identity::{ControllerChange, ObjectMovement, ObjectStatusChange},
    occurrence::{
        CharacteristicAppearanceChange, DrawOrigin, EventCause, RulesJournal, RulesOccurrence,
        RulesView, SupportedAppearance, ZoneOccurrence, ZoneReason,
    },
    stack_window::DrawBatchProgress,
};

type EmitFromBuild<'a> = dyn FnMut(
        &RulesView<'_>,
        &RulesView<'_>,
        &[ControllerChange],
    ) -> Result<Vec<RulesOccurrence>, EffectError>
    + 'a;

/// Short-lived relevant state before an operation; never an accepted history/outbox.
pub(super) struct RulesSnapshot {
    window: StackWindow,
    programs: Arc<BTreeMap<CardId, Program>>,
    vocabulary: Arc<CreatureTypeVocabulary>,
    name_rules: Arc<spellwake_contracts::names::NameRules>,
    characteristics: CharacteristicState,
    evaluated: Arc<EvaluatedCharacteristics>,
    marks: BTreeMap<ObjectId, RulesInteger>,
    mana: BTreeMap<(SeatId, ManaType), RulesInteger>,
    entry_creature_types: BTreeMap<ObjectId, spellwake_contracts::creature_types::CreatureTypeId>,
    life: crate::life::LifeState,
    combat: Option<CombatState>,
    step: Option<crate::turn::Step>,
    turn: Option<TurnScheduler>,
    previous_turn_casts: Option<crate::program::PreviousTurnCastCondition>,
    morbid: bool,
}
impl RulesSnapshot {
    pub(super) fn view(&self) -> RulesView<'_> {
        self.view_with_evaluated(self.window.cards(), self.evaluated.clone())
    }
    /// Evaluate an actual registry overlay using this captured mutable context,
    /// never the latest EffectWindow state or a reconstructed journal world.
    pub(super) fn view_with_cards<'a>(
        &'a self,
        cards: &'a GameObjects,
    ) -> Result<RulesView<'a>, EffectError> {
        let evaluated = Arc::new(evaluate(CharacteristicWorld {
            active_player: self.window.active_player(),
            participants: self.window.participants(),
            life: Some(&self.life),
            entry_creature_types: &self.entry_creature_types,
            mana: &self.mana,
            combat: self.combat.as_ref(),
            cards,
            programs: &self.programs,
            state: &self.characteristics,
            vocabulary: &self.vocabulary,
            name_rules: &self.name_rules,
            step: self.step,
        })?);
        Ok(self.view_with_evaluated(cards, evaluated))
    }
    fn view_with_evaluated<'a>(
        &'a self,
        cards: &'a GameObjects,
        evaluated: Arc<EvaluatedCharacteristics>,
    ) -> RulesView<'a> {
        RulesView {
            name_rules: &self.name_rules,
            window: &self.window,
            cards,
            programs: &self.programs,
            characteristics: &self.characteristics,
            evaluated,
            marks: &self.marks,
            mana: &self.mana,
            life: &self.life,
            combat: self.combat.as_ref(),
            step: self.step,
            turn: self.turn.as_ref(),
            previous_turn_casts: self.previous_turn_casts,
            morbid: self.morbid,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SagaSource {
    final_chapter: u8,
    controller: SeatId,
    mana_value: RulesInteger,
}

impl RulesView<'_> {
    fn saga_source(&self, object: ObjectId) -> Option<SagaSource> {
        let current = self.evaluated.get(object)?;
        let final_chapter = current.final_saga_chapter()?;
        let controller = current.controller?;
        let physical = self.cards.get(object)?;
        let mana_value = if physical.face_down().is_some() {
            0.into()
        } else if physical.transformed() && self.characteristics.copied_definition(object).is_none()
        {
            self.programs.get(&physical.card_id()?)?.mana_value()
        } else {
            self.characteristics
                .copiable_definition(physical, self.programs)?
                .0
                .mana_value()
        };
        Some(SagaSource {
            final_chapter,
            controller,
            mana_value,
        })
    }
}

impl EffectWindow {
    fn final_saga_chapter_completed(
        &self,
        entry: StackEntryId,
        before: &RulesSnapshot,
    ) -> Option<RulesOccurrence> {
        use crate::occurrence::TriggerRelated;
        let departed = self.departed_saga_sources.as_ref()?;
        let binding = self.execution_bindings(entry)?.trigger.as_ref()?;
        let [cause] = binding.causes.members() else {
            return None;
        };
        let TriggerRelated::SagaChapter { number } = cause.related else {
            return None;
        };
        let saga = binding.source.object.id();
        let source = if before.evaluated.get(saga).is_some() {
            before.view().saga_source(saga)?
        } else {
            departed.get(&saga)?.clone()
        };
        (number == source.final_chapter).then_some(RulesOccurrence::FinalSagaChapterCompleted {
            saga,
            controller: source.controller,
            mana_value: source.mana_value,
        })
    }

    /// Complete through the sole stack mutator and consume its actual exit receipt.
    pub(super) fn complete_at_cut(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        completion: crate::occurrence::EntryCompletion,
        destination: Option<ObjectDestination>,
        append: &mut RulesJournal,
    ) -> Result<Option<ObjectMovement>, EffectError> {
        let before = self.rules_snapshot(None)?;
        let saga_completion = (completion == crate::occurrence::EntryCompletion::Resolved)
            .then(|| self.final_saga_chapter_completed(entry.id(), &before))
            .flatten();
        let rebound_source = match entry.kind() {
            StackKind::CardSpell(object)
                if completion == crate::occurrence::EntryCompletion::Resolved
                    && destination == Some(ObjectDestination::Graveyard)
                    && entry.cast_from_controllers_hand()
                    && before.evaluated.get(*object).is_some_and(|current| {
                        current.has_keyword(KeywordAbility::Rebound)
                            || self.spell_has_granted_keyword(
                                &before.evaluated,
                                current,
                                entry.controller(),
                                KeywordAbility::Rebound,
                            )
                    }) =>
            {
                before.view().appearance(*object)
            }
            _ => None,
        };
        let adventure = completion == crate::occurrence::EntryCompletion::Resolved
            && destination == Some(ObjectDestination::Graveyard)
            && self
                .stack_copiable_spell_subtypes(entry.id())
                .is_some_and(|types| types.contains(&crate::program::SpellSubtype::Adventure));
        let destination = if rebound_source.is_some() || adventure {
            Some(ObjectDestination::Exile)
        } else {
            destination
        };
        let (window, mut exit) = match completion {
            crate::occurrence::EntryCompletion::Resolved
            | crate::occurrence::EntryCompletion::InterveningIfFalse => self
                .window
                .finish_resolution_with_exit(self.window.revision(), entry.id(), destination)?,
            crate::occurrence::EntryCompletion::AllTargetsIllegal => self
                .window
                .finish_unresolved_with_exit(self.window.revision(), entry.id(), destination)?,
        };
        self.window = window;
        let mut movement = match &exit {
            Some(crate::stack_window::StackExit::Object(movement)) => Some(movement.clone()),
            _ => None,
        };
        if adventure
            && let Some(moved) = movement.as_ref()
            && moved.after.zone() == ObjectZone::Exile
            && moved.after.card_id().is_some()
        {
            self.adventured
                .insert((moved.after.id(), entry.controller()));
        }
        if let (Some(source), Some(moved)) = (rebound_source, movement.as_ref())
            && moved.after.zone() == ObjectZone::Exile
        {
            self.triggers
                .delay_rebound(source, moved.after.id(), entry.controller());
        }
        let mut entry_counters = None;
        let mut entry_door_unlocked = None;
        if let Some(movement) = &mut movement
            && movement.after.zone() == ObjectZone::Battlefield
            && movement.before.zone() != ObjectZone::Battlefield
        {
            if let Some(half) = movement.before.split_half() {
                let doors = self
                    .characteristics
                    .unlocked_doors
                    .entry(movement.after.id())
                    .or_default();
                match half {
                    crate::program::SplitHalf::Left => doors.left = true,
                    crate::program::SplitHalf::Right => doors.right = true,
                }
                let fully_unlocked = doors.fully_unlocked();
                entry_door_unlocked = Some((
                    movement.after.id(),
                    entry.controller(),
                    half,
                    fully_unlocked,
                ));
            }
            self.stage_resolution_entry_choice(movement.before.id(), movement.after.id())?;
            self.characteristics
                .register_battlefield_entry(movement.after.id())?;
            if self
                .characteristics
                .bestowed_spells
                .remove(&movement.before.id())
            {
                self.characteristics
                    .enter_bestowed(entry.id(), movement.after.id())?;
            }
            if !entry.kicker_choices().is_empty() {
                self.characteristics.kicked.insert(movement.after.id());
                self.characteristics
                    .kicker_counts
                    .insert(movement.after.id(), entry.kicker_choices().clone());
            }
            self.characteristics
                .kicker_counts
                .remove(&movement.before.id());
            if entry.paid_conditional_cost() || entry.cast_summary().paid_alternative {
                self.characteristics
                    .paid_alternative_objects
                    .insert(movement.after.id());
            }
            self.characteristics
                .cast_summaries
                .insert(movement.after.id(), entry.cast_summary().clone());
            if entry.paid_conditional_cost()
                && self.copiable_program(movement.after.id()).is_some_and(|p| {
                    matches!(
                        p.conditional_cost,
                        Some(crate::program::ConditionalCastCost::Evoke(_))
                    )
                })
            {
                self.characteristics.evoked.insert(movement.after.id());
            }
            if entry.paid_conditional_cost()
                && let Some(crate::program::ConditionalCastCost::EndStep { kind, .. }) = self
                    .copiable_program(movement.after.id())
                    .and_then(|p| p.conditional_cost.as_ref())
            {
                let kind = *kind;
                let spell = before
                    .view()
                    .appearance(movement.before.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                self.characteristics
                    .end_step_casts
                    .insert(movement.after.id(), kind);
                self.triggers
                    .delay_cast(spell, movement.after.id(), kind, entry.controller());
            }
            let mut extra = PowerToughnessCounters::default();
            if let Some(card) = movement.before.card_id() {
                extra.plus_one =
                    self.take_commander_mana_entry_counters(movement.before.id(), card);
            }
            if let Some((controller, _)) = self
                .characteristics
                .suspend_haste
                .remove(&movement.before.id())
            {
                let parts = crate::characteristics::ResolvedParts(vec![
                    crate::characteristics::ResolvedPart::Abilities(
                        crate::characteristics::AbilityChange::grant_keywords([
                            KeywordAbility::Haste,
                        ]),
                    ),
                ]);
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    movement.after.id(),
                    parts,
                    crate::characteristics::ResolvedDuration::NoStatedDuration,
                )?;
                self.characteristics
                    .suspend_haste
                    .insert(movement.after.id(), (controller, Some(effect.id)));
            }
            if entry.paid_conditional_cost()
                && matches!(entry.kind(), StackKind::CardSpell(_))
                && let Some(crate::program::ConditionalCastCost::Escape { entry_counters, .. }) =
                    movement
                        .after
                        .card_id()
                        .and_then(|id| self.programs.get(&id))
                        .and_then(|p| p.conditional_cost.as_ref())
            {
                extra.plus_one = extra.plus_one.plus(&i64::from(*entry_counters).into());
            }
            let added = self.apply_spell_entry_counters(
                &before,
                movement.after.id(),
                extra,
                entry.sunburst_colors(),
                Some(entry),
            )?;
            if let Some(defender) = entry.attacking_defender() {
                movement.after = self.put_attacking(movement.after.id(), defender)?;
            } else {
                movement.after = self
                    .window
                    .cards()
                    .get(movement.after.id())
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
            }
            if let Some(crate::stack_window::StackExit::Object(moved)) = &mut exit {
                moved.after = movement.after.clone();
            }
            if added.valid_placement() {
                entry_counters = Some((movement.before.id(), movement.after.id(), added));
            }
            if let Some(host) = self.resolution_aura_choices.remove(&movement.before.id()) {
                self.characteristics
                    .attach(movement.after.id(), host, true)?;
            } else if let [bound] = entry.targets().targets()
                && matches!(
                    self.attachment_kind(movement.after.id()),
                    Some(crate::attachment::AttachmentKind::Aura(_))
                )
            {
                self.characteristics
                    .attach(movement.after.id(), bound.choice().target, true)?;
            }
        }
        if completion == crate::occurrence::EntryCompletion::Resolved
            && let StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } =
                entry.kind()
        {
            let count = self
                .characteristics
                .ability_resolutions_by_source_this_turn
                .entry(source.id())
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        let reason = if matches!(destination, Some(ObjectDestination::Battlefield { .. })) {
            ZoneReason::PermanentResolution
        } else if completion == crate::occurrence::EntryCompletion::AllTargetsIllegal {
            ZoneReason::AllTargetsIllegalExit
        } else if matches!(self.payloads.get(&entry.id()), Some(StackPayload::Instructions(payload)) if payload.exile_on_resolve)
        {
            ZoneReason::ExiledByEffect
        } else {
            ZoneReason::ResolutionExit
        };
        self.emit_from(
            EventCause::ResolutionCompleted {
                entry: entry.id(),
                completion,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                let value = if let Some(exit) = exit {
                    exit_occurrence_at(previous, current, exit, reason, controllers)?
                } else {
                    RulesOccurrence::EntryCompleted {
                        entry: entry.id(),
                        controller: entry.controller(),
                        kind: entry.kind().clone(),
                        completion,
                    }
                };
                let mut rows = vec![value];
                rows.extend(saga_completion);
                if let Some((object, player, half, fully_unlocked)) = entry_door_unlocked {
                    rows.push(RulesOccurrence::DoorUnlocked {
                        object,
                        player,
                        half,
                        fully_unlocked,
                    });
                }
                if let Some((source, object, added)) = entry_counters {
                    rows.push(RulesOccurrence::CountersPlaced {
                        object,
                        before: previous.characteristics.counters(object),
                        added,
                        after: current.characteristics.counters(object),
                        before_appearance: Box::new(
                            previous
                                .appearance(source)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )?;
        if let Some(movement) = &mut movement {
            movement.after = self
                .window
                .cards()
                .get(movement.after.id())
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
        }
        Ok(movement)
    }

    pub(crate) fn begin_retained_mana(&mut self, issued: &[ManaProductionRecord]) {
        self.retained_mana = Some(issued.iter().cloned().collect());
    }
    pub(crate) fn finish_retained_mana(&mut self) -> Result<(), EffectError> {
        if self
            .retained_mana
            .take()
            .is_some_and(|records| !records.is_empty())
        {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(())
    }
    pub(super) fn retained_mana_amount(
        &self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    ) -> Option<RulesInteger> {
        self.retained_mana
            .as_ref()?
            .front()
            .filter(|p| p.source == source && p.ability == ability)
            .map(|p| {
                p.produced
                    .values()
                    .fold(0.into(), |sum: RulesInteger, count| sum.plus(count))
            })
    }
    // Internal mana-credit helper passing source, restriction, and journal context explicitly.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn credit_mana_at_cut(
        &mut self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        players: (SeatId, SeatId),
        produced: BTreeMap<ManaType, RulesInteger>,
        restriction: Option<crate::mana::ManaRestriction>,
        triggered: bool,
        sacrificed_source: Option<SacrificedManaSource>,
        append: &mut RulesJournal,
    ) -> Result<ManaProductionRecord, EffectError> {
        let (controller, recipient) = players;
        let produced = if let Some(records) = &mut self.retained_mana {
            let original = records
                .pop_front()
                .filter(|p| {
                    p.source == source
                        && p.ability == ability
                        && p.controller == controller
                        && p.recipient.unwrap_or(controller) == recipient
                        && p.restriction == restriction
                })
                .ok_or(EffectError::InconsistentProgram)?;
            original.produced
        } else {
            produced
        };

        // A counted mana ability may legally resolve producing nothing. Its tap
        // and activation facts already exist; CR 106.12a requires real produced
        // mana before a tapped-for-mana occurrence can arise.
        if produced.is_empty() {
            return Ok(self.credit_mana(
                source,
                ability,
                (controller, recipient),
                produced,
                restriction,
            ));
        }
        let before = self.rules_snapshot(None)?;
        let output = before
            .evaluated
            .get(source)
            .and_then(|p| p.mana_output(ability));
        let tap_mana = !triggered
            && sacrificed_source
                .as_ref()
                .is_none_or(|source| source.tapped)
            && !output
                .as_deref()
                .is_some_and(crate::mana::ManaOutput::skips_source_tap);
        let was_artifact = sacrificed_source
            .as_ref()
            .map(|source| source.was_artifact)
            .unwrap_or_else(|| {
                before.evaluated.get(source).is_some_and(|object| {
                    object.zone == ObjectZone::Battlefield && object.has_type(CardType::Artifact)
                })
            });
        if !triggered
            && let Some(effect) = output
                .as_deref()
                .and_then(crate::mana::ManaOutput::spend_effect_rule)
        {
            let appearance = before
                .view()
                .appearance(source)
                .ok_or(EffectError::InconsistentProgram)?;
            self.record_mana_lots(appearance, recipient, &produced, effect)?;
        }
        let was_snow = sacrificed_source
            .as_ref()
            .map(|source| source.was_snow)
            .unwrap_or_else(|| {
                before.evaluated.get(source).is_some_and(|object| {
                    object
                        .current_supertypes
                        .contains(&crate::program::Supertype::Snow)
                })
            });
        let was_treasure = sacrificed_source
            .as_ref()
            .map(|source| source.was_treasure)
            .unwrap_or_else(|| {
                before.evaluated.get(source).is_some_and(|object| {
                    object
                        .current_artifact_subtypes
                        .contains(&crate::program::ArtifactSubtype::Treasure)
                })
            });
        let was_creature = sacrificed_source
            .as_ref()
            .map(|source| source.was_creature)
            .unwrap_or_else(|| {
                before
                    .evaluated
                    .get(source)
                    .is_some_and(|object| object.is_creature())
            });
        self.record_mana_source_traits(recipient, &produced, was_snow, was_treasure, was_creature);
        let production = self.credit_mana(
            source,
            ability,
            (controller, recipient),
            produced,
            restriction,
        );
        let record_artifact_tap = tap_mana && was_artifact && self.track_artifact_tapped_for_mana;
        let record_perm_tap = tap_mana && self.track_artifact_tapped_for_mana;
        self.emit_from(
            EventCause::ManaProduced { source, ability },
            &before,
            None,
            append,
            |previous, current, _| {
                let mut values = vec![RulesOccurrence::ManaProduced {
                    production: production.clone(),
                    before: previous
                        .mana_pool(recipient)
                        .ok_or(EffectError::InconsistentProgram)?,
                    after: current
                        .mana_pool(recipient)
                        .ok_or(EffectError::InconsistentProgram)?,
                }];
                if record_artifact_tap {
                    values.push(RulesOccurrence::ArtifactTappedForMana {
                        source,
                        actor: controller,
                    });
                }
                if record_perm_tap && let Some(appearance) = previous.appearance(source) {
                    values.push(RulesOccurrence::PermanentTappedForMana {
                        source,
                        actor: controller,
                        appearance: Box::new(appearance),
                    });
                }
                Ok(values)
            },
        )?;
        // Triggered mana abilities resolve immediately after the land ability,
        // without using the stack. Capture each surviving instance at this cut.
        if tap_mana {
            let current = self.evaluated_characteristics()?;
            let was_land = sacrificed_source
                .as_ref()
                .map(|source| source.was_land)
                .unwrap_or_else(|| {
                    current.get(source).is_some_and(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.has_type(crate::program::CardType::Land)
                    })
                });
            let names = sacrificed_source
                .as_ref()
                .map(|source| &source.names)
                .or_else(|| before.evaluated.get(source).map(|land| &land.current_names));
            let mut immediate_triggered_mana: Vec<(
                ObjectId,
                crate::mana::ManaAbilityId,
                SeatId,
                BTreeMap<ManaType, RulesInteger>,
            )> = Vec::new();
            for object in current
                .objects
                .values()
                .filter(|object| object.zone == ObjectZone::Battlefield)
            {
                let Some(mana_controller) = object.controller else {
                    continue;
                };
                for mana in &object.printed_text.mana {
                    if !object.abilities.mana.contains(&mana.id) {
                        continue;
                    }
                    let crate::mana::ManaOutput::Triggered(rule) = &mana.output else {
                        continue;
                    };
                    let allow_any_controller = matches!(
                        rule,
                        crate::mana::TriggeredManaRule::ImprintedLandTappedSameType
                            | crate::mana::TriggeredManaRule::LandTappedChosenColor {
                                controller_only: false
                            }
                            | crate::mana::TriggeredManaRule::EnchantedLandTapped { .. }
                    );
                    if !allow_any_controller && mana_controller != controller {
                        continue;
                    }
                    if let crate::mana::TriggeredManaRule::EnchantedLandTapped { extra } = rule {
                        let attached_to_source = object.attached_to
                            == Some(crate::targets::TargetRef::Object(source))
                            || before.evaluated.get(object.object).is_some_and(|prev| {
                                prev.attached_to == Some(crate::targets::TargetRef::Object(source))
                            });
                        if !was_land || !attached_to_source {
                            continue;
                        }
                        if self.retained_mana.as_ref().is_some_and(|records| {
                            !records.iter().any(|record| {
                                record.source == object.object
                                    && record.ability
                                        == crate::mana::ManaAbilityRef::Printed(mana.id)
                            })
                        }) {
                            continue;
                        }
                        match extra {
                            crate::mana::EnchantedLandManaBonus::Fixed(pool) => {
                                immediate_triggered_mana.push((
                                    object.object,
                                    mana.id,
                                    mana_controller,
                                    pool.clone(),
                                ));
                            }
                            crate::mana::EnchantedLandManaBonus::ChosenColor => {
                                let Some(chosen) =
                                    self.entry_color_choices.get(&object.object).copied()
                                else {
                                    continue;
                                };
                                immediate_triggered_mana.push((
                                    object.object,
                                    mana.id,
                                    mana_controller,
                                    BTreeMap::from([(chosen, 1.into())]),
                                ));
                            }
                            crate::mana::EnchantedLandManaBonus::AnyColor { count }
                            | crate::mana::EnchantedLandManaBonus::AnyCombination { count } => {
                                let colors: BTreeSet<ManaType> = ManaType::ALL
                                    .into_iter()
                                    .filter(|kind| *kind != ManaType::Colourless)
                                    .collect();
                                for _ in 0..*count {
                                    self.triggered_mana_queue.push((
                                        object.object,
                                        mana.id,
                                        mana_controller,
                                        controller,
                                        colors.clone(),
                                    ));
                                }
                            }
                        }
                        continue;
                    }
                    let colors = match rule {
                        crate::mana::TriggeredManaRule::ImprintedLandTappedSameType
                            if was_land
                                && names.is_some_and(|names| {
                                    self.characteristics
                                        .imprinted_cards
                                        .get(&object.object)
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|id| current.get(*id))
                                        .any(|card| {
                                            card.zone == ObjectZone::Exile
                                                && self
                                                    .name_rules
                                                    .shares_name(names, &card.current_names)
                                        })
                                }) =>
                        {
                            production.produced.keys().copied().collect()
                        }
                        crate::mana::TriggeredManaRule::LandTappedWhileMonarch
                            if was_land && self.characteristics.monarch == Some(controller) =>
                        {
                            ManaType::ALL
                                .into_iter()
                                .filter(|kind| *kind != ManaType::Colourless)
                                .collect()
                        }
                        crate::mana::TriggeredManaRule::LandTappedSameType if was_land => {
                            production.produced.keys().copied().collect()
                        }
                        crate::mana::TriggeredManaRule::LandTappedChosenColor { .. }
                            if was_land =>
                        {
                            let Some(chosen) =
                                self.entry_color_choices.get(&object.object).copied()
                            else {
                                continue;
                            };
                            if !production.produced.contains_key(&chosen) {
                                continue;
                            }
                            BTreeSet::from([chosen])
                        }
                        crate::mana::TriggeredManaRule::PermanentTappedForColorless
                            if production.produced.contains_key(&ManaType::Colourless) =>
                        {
                            BTreeSet::from([ManaType::Colourless])
                        }
                        _ => continue,
                    };
                    if self.retained_mana.as_ref().is_some_and(|records| {
                        !records.iter().any(|record| {
                            record.source == object.object
                                && record.ability == crate::mana::ManaAbilityRef::Printed(mana.id)
                        })
                    }) {
                        continue;
                    }
                    self.triggered_mana_queue.push((
                        object.object,
                        mana.id,
                        mana_controller,
                        controller,
                        colors,
                    ));
                }
            }
            for (trig_source, trig_id, trig_controller, pool) in immediate_triggered_mana {
                self.credit_mana_at_cut(
                    trig_source,
                    crate::mana::ManaAbilityRef::Printed(trig_id),
                    (trig_controller, controller),
                    pool,
                    None,
                    true,
                    None,
                    append,
                )?;
            }
        }
        Ok(production)
    }
    pub(super) fn debit_mana_at_cut(
        &mut self,
        payment: &ManaPaymentRecord,
        casting: bool,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if payment.spend.is_empty() {
            return if payment.from_lots.is_empty() {
                Ok(())
            } else {
                Err(EffectError::InvalidManaPayment(
                    ManaPaymentError::WrongTotal,
                ))
            };
        }
        let before = self.rules_snapshot(turn)?;
        self.debit_restricted_mana(payment.payer, &payment.restricted_mana)?;
        let spent = self.spend_mana_lots(
            payment.payer,
            &payment.spend,
            &payment.restricted_mana,
            Some(&payment.from_lots),
        )?;
        let (spent_snow, spent_treasure, spent_creature) =
            self.consume_mana_source_traits(payment.payer, &payment.spend);
        if casting {
            self.window.record_cast_mana_summary(
                payment.source,
                &payment.spend,
                spent_snow,
                spent_treasure,
                spent_creature,
            )?;
            self.floating_mana.remember_cast(payment.source, spent);
        }
        self.debit_mana(payment.payer, &payment.spend);
        self.emit_from(
            EventCause::ManaDebited {
                entry: payment.source,
            },
            &before,
            turn,
            append,
            |previous, current, _| {
                let before_pool = previous
                    .mana_pool(payment.payer)
                    .ok_or(EffectError::InconsistentProgram)?;
                let after_pool = current
                    .mana_pool(payment.payer)
                    .ok_or(EffectError::InconsistentProgram)?;
                Ok(payment
                    .spend
                    .iter()
                    .map(|spend| RulesOccurrence::ManaDebited {
                        payment: payment.clone(),
                        mana_type: spend.mana_type,
                        amount: spend.amount.clone(),
                        before: before_pool[&spend.mana_type].clone(),
                        after: after_pool[&spend.mana_type].clone(),
                    })
                    .collect())
            },
        )
    }
    /// Draw inside the enclosing tentative call without allocating another executor revision.
    pub(super) fn draw_at_cuts(
        &mut self,
        player: SeatId,
        amount: RulesInteger,
        origin: DrawOrigin,
        append: &mut RulesJournal,
    ) -> Result<DrawJournal, EffectError> {
        #[cfg(test)]
        super::characteristic_observations::requested(self, player, origin, &amount);
        // This batch changes only the physical registry and failed-draw memory.
        // Its preceding after-result remains exact for the next unchanged before-cut.
        let context = self.rules_snapshot(None)?;
        let mut amount = amount;
        if amount.is_positive()
            && matches!(origin, DrawOrigin::Turn { .. })
            && self.should_skip_step(player, crate::turn::Step::Draw)?
        {
            amount = 0.into();
        }
        if amount.is_positive() && !matches!(origin, DrawOrigin::Opening) {
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), player)?;
            let draw_prohibited = context.evaluated.objects.values().any(|perm| {
                perm.zone == ObjectZone::Battlefield
                    && perm.has_battlefield_rule(|rule| match rule {
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::PlayersCannotDraw,
                            ..
                        } => true,
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::OpponentsCannotDraw,
                            ..
                        } => perm.controller.is_some_and(|c| opponents.contains(&c)),
                        _ => false,
                    })
            });
            if draw_prohibited {
                amount = 0.into();
            }
        }
        if amount.is_positive() && !matches!(origin, DrawOrigin::Opening) {
            // Apply StaticDrawRule::ModifyDraw (Teferi's Ageless Insight, Thought Reflection, Phial of Galadriel).
            let hand_empty = self
                .window
                .cards()
                .iter()
                .all(|obj| obj.zone() != ObjectZone::Hand(player));
            let is_first_draw_step = matches!(origin, DrawOrigin::Turn { .. })
                && !self.cards_drawn_this_turn.contains_key(&player);
            let mut total_add = 0i64;
            let mut factors: Vec<u8> = Vec::new();
            let mut skip_first_any = false;
            let mut skip_draw = false;
            let mut body_replacements: Vec<super::event_replacement::EventBodyReplacement> =
                Vec::new();
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), player)?;
            for perm in context
                .evaluated
                .objects
                .values()
                .filter(|p| p.zone == ObjectZone::Battlefield)
            {
                let Some(perm_ctrl) = perm.controller else {
                    continue;
                };
                for ability in perm
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&perm.printed_text.additional_battlefield_abilities)
                {
                    let active = perm.abilities.continuous == Some(ability.id())
                        || perm
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id());
                    if !active {
                        continue;
                    }
                    match ability {
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule:
                                crate::characteristics::StaticDrawRule::ModifyDraw {
                                    add,
                                    multiply,
                                    skip_first_draw_step,
                                    require_empty_hand,
                                },
                            ..
                        } if perm_ctrl == player => {
                            if *require_empty_hand && !hand_empty {
                                continue;
                            }
                            if *skip_first_draw_step && is_first_draw_step {
                                if amount <= 1.into() {
                                    continue;
                                }
                                skip_first_any = true;
                            }
                            total_add = total_add.saturating_add(i64::from(*add));
                            if *multiply > 1 {
                                factors.push(*multiply);
                            }
                        }
                        crate::characteristics::StaticContinuousAbility::EventReplacement {
                            id,
                            event: crate::characteristics::ReplaceableEvent::Draw { player: rel },
                            condition,
                            action,
                        } => {
                            let applies = match rel {
                                crate::program::AffectedPlayers::One(
                                    crate::program::ResolvingPlayer::Controller,
                                ) => player == perm_ctrl,
                                crate::program::AffectedPlayers::EachOpponent
                                | crate::program::AffectedPlayers::EachOtherPlayer => {
                                    opponents.contains(&perm_ctrl)
                                }
                                crate::program::AffectedPlayers::EachPlayer
                                | crate::program::AffectedPlayers::One(_) => true,
                            };
                            if !applies {
                                continue;
                            }
                            let ctx = crate::characteristics::WorldEvalCtx {
                                active_player: self.window.active_player(),
                                step: self.current_step,
                                participants: self.window.participants(),
                                life: Some(&self.life),
                                combat: self.combat.as_ref(),
                                cards: self.window.cards(),
                                programs: &self.programs,
                                state: &self.characteristics,
                                objects: &context.evaluated.objects,
                                source: perm.object,
                                controller: perm_ctrl,
                                name_rules: &self.name_rules,
                                trigger_subject: None,
                                related_player: None,
                                defending_player: None,
                            };
                            let cond_ok = condition.as_deref().is_none_or(|cond| match cond {
                                crate::characteristics::StaticSourceCondition::Eval(rc) => {
                                    crate::characteristics::eval_world_condition(&ctx, rc)
                                }
                                _ => true,
                            });
                            if !cond_ok {
                                continue;
                            }
                            match action {
                                crate::characteristics::StaticReplacementAction::ModifyAmount {
                                    add,
                                    multiply,
                                } => {
                                    let val =
                                        crate::characteristics::eval_world_value_expr(&ctx, add);
                                    if val.is_positive()
                                        && let Some(u) = val.to_usize()
                                    {
                                        total_add = total_add
                                            .saturating_add(i64::try_from(u).unwrap_or(i64::MAX));
                                    }
                                    if *multiply > 1 {
                                        factors.push(*multiply);
                                    }
                                }
                                crate::characteristics::StaticReplacementAction::Skip => {
                                    skip_draw = true;
                                }
                                crate::characteristics::StaticReplacementAction::ReplaceWithBody {
                                    optional,
                                    body,
                                } => {
                                    let key = (Some(perm.object), *id);
                                    if !optional && !self.active_event_replacements.contains(&key) {
                                        skip_draw = true;
                                        body_replacements.push(
                                            super::event_replacement::EventBodyReplacement {
                                                source: Some(perm.object),
                                                ability: *id,
                                                controller: perm_ctrl,
                                                affected_player: player,
                                                optional: false,
                                                body: (**body).clone(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            for active in &self.temporary_rules {
                if let Some(ab) = self.active_temporary_static(active, &context.evaluated)
                    && let crate::characteristics::StaticContinuousAbility::EventReplacement {
                        id,
                        event: crate::characteristics::ReplaceableEvent::Draw { player: rel },
                        action,
                        ..
                    } = ab
                {
                    let applies = match rel {
                        crate::program::AffectedPlayers::One(
                            crate::program::ResolvingPlayer::Controller,
                        ) => player == active.controller,
                        crate::program::AffectedPlayers::EachOpponent
                        | crate::program::AffectedPlayers::EachOtherPlayer => {
                            opponents.contains(&active.controller)
                        }
                        crate::program::AffectedPlayers::EachPlayer
                        | crate::program::AffectedPlayers::One(_) => true,
                    };
                    if !applies {
                        continue;
                    }
                    match action {
                        crate::characteristics::StaticReplacementAction::ModifyAmount {
                            add,
                            multiply,
                        } => {
                            if let crate::program::ValueExpr::Literal(v) = add.as_ref()
                                && let Some(u) = v.to_usize()
                            {
                                total_add =
                                    total_add.saturating_add(i64::try_from(u).unwrap_or(i64::MAX));
                            }
                            if *multiply > 1 {
                                factors.push(*multiply);
                            }
                        }
                        crate::characteristics::StaticReplacementAction::Skip => {
                            skip_draw = true;
                        }
                        crate::characteristics::StaticReplacementAction::ReplaceWithBody {
                            optional,
                            body,
                        } => {
                            let key = (active.source_object, *id);
                            if !optional && !self.active_event_replacements.contains(&key) {
                                skip_draw = true;
                                body_replacements.push(
                                    super::event_replacement::EventBodyReplacement {
                                        source: active.source_object,
                                        ability: *id,
                                        controller: active.controller,
                                        affected_player: player,
                                        optional: false,
                                        body: (**body).clone(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
            if !body_replacements.is_empty() {
                let draws = amount.to_usize().unwrap_or(1);
                let cause = EventCause::Draw { player, origin };
                if let Some(rep) = body_replacements.into_iter().next() {
                    for _ in 0..draws {
                        self.execute_inline_replacement_body(&rep, cause.clone(), append)?;
                    }
                }
                return Ok(DrawJournal::default());
            }
            if skip_draw {
                amount = 0.into();
            } else if total_add != 0 || !factors.is_empty() {
                if skip_first_any {
                    let mut rest = amount.minus(&1.into());
                    if total_add != 0 {
                        rest = rest.plus(&RulesInteger::from(total_add)).max(0.into());
                    }
                    for f in factors {
                        rest = rest.times(&RulesInteger::from(i64::from(f)));
                    }
                    amount = rest.plus(&1.into());
                } else {
                    if total_add != 0 {
                        amount = amount.plus(&RulesInteger::from(total_add)).max(0.into());
                    }
                    for f in factors {
                        amount = amount.times(&RulesInteger::from(i64::from(f)));
                    }
                }
            }
        }
        if amount.is_positive() {
            let capped = self
                .combat_rules
                .opponents(self.window.participants(), player)?
                .into_iter()
                .any(|controller| {
                    context.evaluated.controlled_rules(controller, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::DrawRule {
                                rule: crate::characteristics::StaticDrawRule::OpponentsAtMostOne,
                                ..
                            }
                        )
                    }) > 0
                });
            if capped {
                if self.cards_drawn_this_turn.contains_key(&player) {
                    amount = 0.into();
                } else if amount > 1.into() {
                    amount = 1.into();
                }
            }
        }
        let mut before_evaluated = context.evaluated.clone();
        let mut batch =
            context
                .window
                .begin_draw_batch(context.window.revision(), player, amount)?;
        loop {
            match batch.next_draw()? {
                DrawBatchProgress::Drawn {
                    before,
                    next,
                    movement,
                } => {
                    #[cfg(test)]
                    super::characteristic_observations::drawn(
                        self,
                        player,
                        origin,
                        &before,
                        next.cards(),
                        &movement,
                    );
                    let ordinal = self
                        .cards_drawn_this_turn
                        .get(&player)
                        .map_or_else(|| 1.into(), |count| count.plus(&1.into()));
                    let (after_evaluated, state, records) = {
                        let previous = context.view_with_evaluated(&before, before_evaluated);
                        let current = context.view_with_cards(next.cards())?;
                        let value = zone_occurrence_at(
                            &previous,
                            &current,
                            &movement,
                            ZoneReason::Draw { player },
                            &[],
                        )?;
                        let mut values = vec![value];
                        if current.evaluated.objects.values().any(|source| {
                            source.zone == ObjectZone::Battlefield
                                && source.controller.is_some_and(|ctrl| {
                                    source.printed_text.triggered.iter().any(|ability| {
                                        source.abilities.triggered.contains(&ability.id)
                                            && match ability.condition.event {
                                                crate::program::TriggerEvent::PlayerAction {
                                                    player: rel,
                                                    action:
                                                        crate::program::PlayerTriggerAction::DrewCard {
                                                            nth_each_turn: Some(number),
                                                        },
                                                } if ordinal == i64::from(number.get()).into() => {
                                                    match rel {
                                                        crate::program::PlayerRelation::Controller => {
                                                            ctrl == player
                                                        }
                                                        crate::program::PlayerRelation::Opponent => {
                                                            ctrl != player
                                                        }
                                                        crate::program::PlayerRelation::AnyParticipant => {
                                                            true
                                                        }
                                                    }
                                                }
                                                _ => false,
                                            }
                                    })
                                })
                        }) {
                            values.push(RulesOccurrence::CardDrawOrdinal {
                                player,
                                number: ordinal.clone(),
                            });
                        }
                        let triggers = std::mem::take(&mut self.triggers);
                        let (state, _, records) = triggers
                            .publish(
                                EventCause::Draw { player, origin },
                                values,
                                &previous,
                                &current,
                            )?
                            .into_parts();
                        (current.evaluated.clone(), state, records)
                    };
                    self.cards_drawn_this_turn.insert(player, ordinal);
                    *self.triggers = state;
                    append.append(records);
                    before_evaluated = after_evaluated;
                    batch = next;
                }
                DrawBatchProgress::Complete {
                    window,
                    mut journal,
                } => {
                    if matches!(journal.events.last(), Some(DrawEvent::EmptyLibrary { .. }))
                        && let Some(source) = before_evaluated
                            .controlled_rule_sources(player, |rule| {
                                matches!(
                                    rule,
                                    crate::characteristics::StaticContinuousAbility::DrawRule {
                                        rule: crate::characteristics::StaticDrawRule::EmptyDrawWins,
                                        ..
                                    }
                                )
                            })
                            .next()
                    {
                        // The first impossible draw is replaced. Later draws never occur.
                        journal.events.pop();
                        if !self.player_cannot_win(&before_evaluated, player)? {
                            self.won_by_effect = Some((player, source));
                        }
                    } else if matches!(journal.events.last(), Some(DrawEvent::EmptyLibrary { .. }))
                        && let Some((source, kind, count)) = before_evaluated
                            .objects
                            .values()
                            .filter(|p| {
                                p.zone == ObjectZone::Battlefield && p.controller == Some(player)
                            })
                            .find_map(|p| {
                                p.printed_text
                                    .continuous
                                    .iter()
                                    .chain(&p.printed_text.additional_battlefield_abilities)
                                    .find_map(|ab| {
                                        let active = p.abilities.continuous == Some(ab.id())
                                            || p.abilities
                                                .additional_battlefield_abilities
                                                .contains(&ab.id());
                                        if !active {
                                            return None;
                                        }
                                        if let crate::characteristics::StaticContinuousAbility::DrawRule {
                                            rule:
                                                crate::characteristics::StaticDrawRule::EmptyDrawPutCounters {
                                                    kind,
                                                    amount,
                                                },
                                            ..
                                        } = ab
                                        {
                                            Some((p.object, *kind, *amount))
                                        } else {
                                            None
                                        }
                                    })
                            })
                    {
                        journal.events.pop();
                        self.window = window;
                        if kind == CounterKind::PlusOne {
                            let added = PowerToughnessCounters {
                                plus_one: i64::from(count).into(),
                                minus_one: 0.into(),
                            };
                            let _ = self.characteristics.place_counters(source, &added);
                        } else if let Some(map) =
                            self.characteristics.named_permanent_counters_mut(kind)
                        {
                            let entry = map.entry(source).or_insert_with(|| 0.into());
                            *entry = entry.plus(&i64::from(count).into());
                        }
                        return Ok(journal);
                    }
                    if let Some(DrawEvent::EmptyLibrary { player, attempts }) =
                        journal.events.last()
                    {
                        self.failed_draws.insert(*player);
                        let view = context.view_with_evaluated(window.cards(), before_evaluated);
                        let triggers = std::mem::take(&mut self.triggers);
                        let (state, _, records) = triggers
                            .publish(
                                EventCause::Draw {
                                    player: *player,
                                    origin,
                                },
                                vec![RulesOccurrence::FailedDraw {
                                    player: *player,
                                    attempts: attempts.clone(),
                                }],
                                &view,
                                &view,
                            )?
                            .into_parts();
                        *self.triggers = state;
                        append.append(records);
                    }
                    self.window = window;
                    #[cfg(test)]
                    super::characteristic_observations::completed(self, player, origin, &journal);
                    return Ok(journal);
                }
            }
        }
    }
    pub(super) fn rules_snapshot(
        &self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RulesSnapshot, EffectError> {
        Ok(RulesSnapshot {
            evaluated: Arc::new(self.evaluated_characteristics()?),
            window: self.window.clone(),
            programs: self.programs.clone(),
            vocabulary: self.creature_vocabulary.clone(),
            name_rules: self.name_rules.clone(),
            characteristics: (*self.characteristics).clone(),
            marks: self.damage.clone(),
            mana: self.mana.clone(),
            entry_creature_types: self.entry_creature_type_choices.clone(),
            life: self.life.clone(),
            combat: self.combat.clone(),
            step: turn.map(|turn| turn.position().step).or(self.current_step),
            turn: turn.cloned(),
            previous_turn_casts: self.previous_turn_casts,
            morbid: self.creatures_died_this_turn.is_positive(),
        })
    }
    fn rules_view_with_evaluated<'a>(
        &'a self,
        turn: Option<&'a TurnScheduler>,
        evaluated: Arc<EvaluatedCharacteristics>,
    ) -> RulesView<'a> {
        RulesView {
            name_rules: &self.name_rules,
            window: &self.window,
            cards: self.window.cards(),
            programs: &self.programs,
            characteristics: &self.characteristics,
            evaluated,
            marks: &self.damage,
            mana: &self.mana,
            life: &self.life,
            combat: self.combat.as_ref(),
            step: turn.map(|turn| turn.position().step).or(self.current_step),
            turn,
            previous_turn_casts: self.previous_turn_casts,
            morbid: self.creatures_died_this_turn.is_positive(),
        }
    }
    /// Actual operations alone call this seam. Scratch layer/probe changes never
    /// alter canonical membership or emit removal evidence.
    /// Commit the layer-2 result once per actual cut. Pure evaluation never
    /// changes epochs, taps, timestamps or the physical controller baseline.
    pub(super) fn synchronize_controllers(
        &mut self,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<Vec<ControllerChange>, EffectError> {
        let desired = desired_controllers(self.window.cards(), evaluated)?;
        if desired.is_empty() {
            return Ok(vec![]);
        }
        let (window, changes) = self
            .window
            .reconcile_controllers(self.window.revision(), &desired)?;
        self.window = window;
        Ok(changes)
    }
    fn reconcile_characteristic_boundary(
        &mut self,
        evaluated: &EvaluatedCharacteristics,
    ) -> Vec<Combatant> {
        let mut removed = Vec::new();
        if let Some(combat) = self.combat.as_mut() {
            let cards = self.window.cards();
            combat.reconcile(|participant, defender| {
                let defender_current = defender.is_none_or(|(id, controller)| {
                    cards.get(id).is_some_and(|card| {
                        card.zone() == ObjectZone::Battlefield
                            && evaluated.get(id).is_some_and(|view| {
                                if view.has_type(CardType::Battle) {
                                    self.characteristics.battle_protectors.get(&id)
                                        == Some(&controller)
                                } else {
                                    view.has_type(CardType::Planeswalker)
                                        && view.controller == Some(controller)
                                }
                            })
                    })
                });
                let retained = defender_current
                    && cards.get(participant.object()).is_some_and(|card| {
                        card.zone() == ObjectZone::Battlefield
                            && card.controller() == Some(participant.controller())
                            && card.control_epoch() == Some(participant.control_epoch())
                            && evaluated
                                .get(card.id())
                                .is_some_and(ObjectCharacteristics::is_creature)
                    });
                if !retained {
                    removed.push(participant);
                }
                retained
            });
        }
        removed
    }
    fn needs_departed_copy_source(&self, object: ObjectId) -> bool {
        let refers_to = |source: ObjectId,
                         causes: &crate::occurrence::TriggerCauseSet,
                         body: &ExecutableBody| {
            (source == object
                && body.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(spec.op, crate::toolbox::ObjectMutation::Myriad)
                    )
                }))
                || (body.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::DealDamage(spec)
                            if matches!(
                                spec.recipients,
                                crate::toolbox::DamageTargetQuery::FromGrantingPermanent
                            )
                    )
                }) && causes.members().iter().any(|member| {
                    matches!(&member.related, crate::occurrence::TriggerRelated::ZoneChange { after, .. }
                        | crate::occurrence::TriggerRelated::TokenCreated { after, .. } if after.object.id() == object)
                }))
        };
        self.triggers.waiting().values().any(|waiting| refers_to(waiting.source.object.id(), &waiting.causes, &waiting.body))
            || self.payloads.values().any(|payload| matches!(payload, StackPayload::Instructions(p)
                if p.bindings.trigger.as_ref().is_some_and(|b| refers_to(b.source.object.id(), &b.causes, &p.body))))
    }

    fn needs_departed_ability_source(&self, object: ObjectId) -> bool {
        let needs_source = |op: &Instruction| {
            matches!(
            op,
            Instruction::CreateTokens { creations }
                    if creations.iter().any(|creation| {
                        creation.amount.references_trigger_source_stat()
                            || match &creation.base_size {
                                Some(crate::token::TokenSize::AtCreation(expr)) => {
                                    expr.references_trigger_source_stat()
                                }
                                Some(crate::token::TokenSize::AtCreationPair {
                                    power,
                                    toughness,
                                }) => {
                                    power.references_trigger_source_stat()
                                        || toughness.references_trigger_source_stat()
                                }
                                _ => false,
                            }
                    })
            )
        };
        if self.triggers.waiting().values().any(|waiting| waiting.source.object.id() == object && waiting.body.instructions.iter().any(needs_source))
            || self.payloads.values().any(|payload| matches!(payload, StackPayload::Instructions(p) if p.bindings.trigger.as_ref().is_some_and(|b|b.source.object.id()==object) && p.body.instructions.iter().any(needs_source))) {
            return true;
        }
        self.window.entries().iter().chain(self.window.resolving()).chain(self.window.checking_targets())
            .any(|entry| matches!(entry.kind(), StackKind::ActivatedAbility { source } if source.id() == object)
                && self.payloads.get(&entry.id()).and_then(StackPayload::instructions).is_some_and(|body| body.iter().any(|op| matches!(op,
                    Instruction::ScheduleDelayedStepTrigger { .. }
                    | Instruction::EmitReflexiveTrigger { .. }
                ) || matches!(
                    op,
                    Instruction::AdjustLife(spec)
                        if matches!(
                            &spec.players,
                            crate::toolbox::PlayerSelector::ControllersOf(q)
                                if matches!(q.as_ref(), crate::toolbox::ObjectQuery::Source)
                        )
                ) || matches!(op, Instruction::SearchLibrary { filter } if filter.is_haste_and_return()))))
    }

    // Phased-out sources and departed sources both need their last known characteristics.
    fn retain_source_appearance(
        &mut self,
        appearance: &SupportedAppearance,
        before: &RulesSnapshot,
    ) {
        let source_id = appearance.object.id();
        if let Some(departed) = &mut self.departed_saga_sources
            && let Some(source) = before.view().saga_source(source_id)
        {
            departed.insert(source_id, source);
        }
        let is_source_quest_require = |op: &Instruction| {
            matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(
                        spec.op,
                        crate::toolbox::PlayerMutation::RequireCondition(
                            crate::program::InterveningCondition::SourceQuestCounters(_)
                        )
                    )
            )
        };
        if let Some(count) = before.characteristics.quest_counters.get(&source_id)
            && (self.triggers.waiting().values().any(|w| {
                w.source.object.id() == source_id
                    && w.body.instructions.iter().any(is_source_quest_require)
            }) || self
                .window
                .entries()
                .iter()
                .chain(self.window.resolving())
                .chain(self.window.checking_targets())
                .any(|entry| {
                    matches!(entry.kind(), StackKind::TriggeredAbility { source } if source.id() == source_id)
                        && self
                            .stack_body(entry.id())
                            .is_some_and(|body| body.instructions.iter().any(is_source_quest_require))
                }))
        {
            self.departed_quest_counts.insert(source_id, count.clone());
        }
        for entry in self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .chain(self.window.checking_targets())
        {
            if matches!(entry.kind(), StackKind::ActivatedAbility {source} | StackKind::TriggeredAbility {source} if source.id()==source_id)
                && let Some(StackPayload::Instructions(payload)) =
                    self.payloads.get_mut(&entry.id())
            {
                for op in &payload.body.instructions {
                    let kind = match op {
                        Instruction::SearchLibrary { filter } => filter.source_counter_kind(),
                        Instruction::DrawCards(spec) => spec.amount.ability_source_counter_kind(),
                        Instruction::DealDamage(spec) => match &spec.amount {
                            crate::program::DamageAmountSpec::Expr(e) => {
                                e.ability_source_counter_kind()
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        payload.departed_source_counters.insert(
                            kind,
                            before
                                .characteristics
                                .permanent_counter_count(source_id, kind),
                        );
                    }
                }
            }
        }
        if self.needs_departed_copy_source(source_id)
            && let Some((definition, _)) = before
                .characteristics
                .copiable_definition(&appearance.object, &before.programs)
        {
            self.departed_copy_sources
                .insert(source_id, definition.clone());
        }
        if self.needs_departed_ability_source(source_id) {
            self.departed_reflexive_sources
                .insert(source_id, appearance.clone());
        }
        let keywords: BTreeSet<_> = [
            KeywordAbility::Deathtouch,
            KeywordAbility::Lifelink,
            KeywordAbility::Infect,
            KeywordAbility::Wither,
        ]
        .into_iter()
        .filter(|key| appearance.keywords.contains(key))
        .collect();
        // If current and captured colors equal the immutable card/token baseline,
        // future abilities can recover the exact value without retaining another object.
        let source = appearance.object.id();
        let colors = &appearance.colors;
        let different_capture = self
            .triggers
            .waiting()
            .values()
            .any(|t| t.source.object.id() == source && t.source.colors != *colors)
            || self
                .window
                .entries()
                .iter()
                .chain(self.window.resolving())
                .chain(self.window.checking_targets())
                .any(|entry| {
                    self.execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .is_some_and(|t| {
                            t.source.object.id() == source && t.source.colors != *colors
                        })
                });
        let refers_to_combatant = |causes: &crate::occurrence::TriggerCauseSet| {
            causes.members().iter().any(|member|
            matches!(member.related,crate::occurrence::TriggerRelated::Combatant(id) if id==source) || matches!(&member.related,crate::occurrence::TriggerRelated::ZoneChange {after,..} | crate::occurrence::TriggerRelated::TokenCreated {after,..} if after.object.id()==source))
        };
        let held_as_combatant=self.triggers.waiting().values().any(|trigger|refers_to_combatant(&trigger.causes))
            || self.payloads.values().any(|payload|matches!(payload,StackPayload::Instructions(p) if
                p.bindings.trigger.as_ref().is_some_and(|trigger|refers_to_combatant(&trigger.causes))));
        let attached_equipment = before.view().evaluated.equipment_attached_to(source);
        let attached_auras_and_equipment = before
            .view()
            .evaluated
            .auras_and_equipment_attached_to(source);
        let granting_damage_source = self.payloads.values().any(|payload| {
            matches!(payload,
            StackPayload::Instructions(p) if p.granting_damage_source.as_ref().is_some_and(|grant|
                grant.id() == crate::damage::DamageSourceId::Object(source)))
        });
        if granting_damage_source
            || held_as_combatant
            || attached_auras_and_equipment.is_positive()
            || appearance.types.contains(&CardType::Creature)
            || !keywords.is_empty()
            || !appearance.creature_subtypes.is_empty()
            || *colors != self.printed_source_colors(&appearance.object)
            || different_capture
        {
            self.departed_damage_sources.insert(
                appearance.object.id(),
                crate::damage::DepartedDamageSource {
                    attached_equipment,
                    attached_auras_and_equipment,
                    power: appearance
                        .current_power_toughness
                        .as_ref()
                        .map(|pt| pt.power.clone()),
                    was_creature: appearance.types.contains(&CardType::Creature),
                    creature_subtypes: appearance.creature_subtypes.clone(),
                    card_types: appearance.types.clone(),
                    colors: appearance.colors.clone(),
                    object: appearance.object.clone(),
                    keywords,
                },
            );
        }
    }

    /// Call only on the enclosing transaction's owned tentative successor.
    /// Complete evaluation and combat reconciliation precede all after appearances
    /// and discovery. A propagated failure drops the entire tentative prefix.
    pub(super) fn emit_from(
        &mut self,
        cause: EventCause,
        before: &RulesSnapshot,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
        build: impl FnOnce(
            &RulesView<'_>,
            &RulesView<'_>,
            &[ControllerChange],
        ) -> Result<Vec<RulesOccurrence>, EffectError>,
    ) -> Result<(), EffectError> {
        let mut build = Some(build);
        self.emit_from_inner(
            cause,
            before,
            turn,
            append,
            &mut |before, current, controllers| {
                let Some(build) = build.take() else {
                    return Err(EffectError::InconsistentProgram);
                };
                build(before, current, controllers)
            },
        )
    }

    fn emit_from_inner(
        &mut self,
        cause: EventCause,
        before: &RulesSnapshot,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
        build: &mut EmitFromBuild<'_>,
    ) -> Result<(), EffectError> {
        // If a revealed cost card leaves hand, its characteristics immediately before
        // that move supply the resolving spell’s last known information.
        for payload in self.payloads.values_mut() {
            if let StackPayload::Instructions(payload) = payload
                && let Some(revealed) = &mut payload.revealed_cost_card
                && self.window.cards().get(revealed.object).is_none()
                && let Some(stats) = before
                    .evaluated
                    .get(revealed.object)
                    .and_then(|o| o.power_toughness.as_ref())
            {
                revealed.last_power = stats.derived.power.clone();
            }
        }
        for payload in self.payloads.values_mut() {
            if let StackPayload::Instructions(payload) = payload
                && let Some((object, power)) = &mut payload.exiled_cost_creature
                && self.window.cards().get(*object).is_none()
                && let Some(stats) = before
                    .evaluated
                    .get(*object)
                    .and_then(|card| card.power_toughness.as_ref())
            {
                *power = stats.derived.power.clone();
            }
        }
        self.independent_exile_links.retain(|_, objects| {
            objects.retain(|id| {
                self.window.cards().get(*id).is_some_and(|card| {
                    card.zone() == ObjectZone::Exile && card.card_id().is_some()
                })
            });
            !objects.is_empty()
        });
        self.granted_flashback.retain(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|card| matches!(card.zone(), ObjectZone::Graveyard(_)))
        });
        self.characteristics.foretold.retain(|id, _| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|o| o.zone() == ObjectZone::Exile)
        });
        self.any_type_exile_cast.retain(|(id, _)| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Exile)
        });
        self.card_play_permissions.retain(|grant| {
            self.window
                .cards()
                .get(grant.object)
                .is_some_and(|object| grant.zone.allows(object.zone()))
        });
        self.exile_play_until_next.retain(|_, (id, _)| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Exile)
        });
        self.face_down_exile.retain(|id, _| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Exile)
        });
        self.warped.retain(|id, _| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Exile)
        });
        self.plotted.retain(|id, _| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|o| o.zone() == ObjectZone::Exile)
        });
        self.characteristics.bestowed_spells.retain(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|o| o.zone() == ObjectZone::Stack)
        });
        self.characteristics.kicker_counts.retain(|id, _| {
            self.characteristics.entered_objects_this_turn.contains(id)
                || self.window.cards().get(*id).is_some_and(|o| {
                    matches!(o.zone(), ObjectZone::Battlefield | ObjectZone::Stack)
                })
                || self.window.cards().phased_out_under(*id).is_some()
        });
        self.prune_non_battlefield_state();
        let old_clock = self.day_night();
        let mut evaluated = Arc::new(self.evaluated_characteristics()?);
        let (clock, flipped) = self.reconcile_day_night(None, &evaluated)?;
        if !flipped.is_empty() {
            evaluated = Arc::new(self.evaluated_characteristics()?);
        }
        let mut ended_control_effects = Vec::new();
        let mut control_exiles = Vec::new();
        loop {
            let expired = self
                .characteristics
                .expire_source_control(self.window.cards(), &evaluated);
            if !expired.is_empty() {
                ended_control_effects.extend(expired);
                evaluated = Arc::new(self.evaluated_characteristics()?);
                continue;
            }
            if self.window.participants().len() == self.window.original_participants().len() {
                break;
            }
            // CR 800.4c: a surviving object cannot revert to a departed default controller.
            let exile = evaluated
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller.is_some_and(|seat| {
                            !self.window.participants().contains(&seat)
                                && self
                                    .window
                                    .cards()
                                    .get(object.object)
                                    .and_then(GameObject::default_controller)
                                    == Some(seat)
                        })
                })
                .map(|object| (object.object, ObjectDestination::Exile))
                .collect::<Vec<_>>();
            if exile.is_empty() {
                break;
            }
            let (window, changes) = self.window.record_linked_return_moves(&exile)?;
            self.window = window;
            control_exiles.extend(changes);
            self.prune_non_battlefield_state();
            evaluated = Arc::new(self.evaluated_characteristics()?);
        }
        let world_changes = self.characteristics.reconcile_world_tenures(&evaluated)?;
        let controllers = self.synchronize_controllers(&evaluated)?;
        self.untap_locks.retain(|&(target, source, epoch)| {
            (self
                .window
                .cards()
                .get(target)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                || self.window.cards().phased_out_under(target).is_some())
                && self.window.cards().get(source).is_some_and(|card| {
                    card.zone() == ObjectZone::Battlefield && card.control_epoch() == Some(epoch)
                })
        });
        let expired_haste = self
            .characteristics
            .expire_suspend_haste(self.window.cards());
        let broke_pair = self.reconcile_pairs(&evaluated, &controllers);
        let evaluated = if expired_haste || broke_pair {
            Arc::new(self.evaluated_characteristics()?)
        } else {
            evaluated
        };
        let removed = self.reconcile_characteristic_boundary(&evaluated);
        let blessing = self.update_city_blessing(&evaluated);
        let current = self.rules_view_with_evaluated(turn, evaluated.clone());
        let mut values = build(&before.view(), &current, &controllers)?;
        for change in &control_exiles {
            values.push(zone_occurrence_at(
                &before.view(),
                &current,
                change,
                ZoneReason::PlayerDeparture,
                &controllers,
            )?);
        }
        let mut cast_spend_events = Vec::new();
        for occurrence in &values {
            if let Some(permanent) = occurrence.departing_permanent() {
                self.source_created_tokens.retain(|_, tokens| {
                    tokens.remove(&permanent.object.id());
                    !tokens.is_empty()
                });
            }
            if let RulesOccurrence::CastCompleted(cast) = occurrence {
                if self
                    .window
                    .entries()
                    .iter()
                    .any(|entry| entry.id() == cast.entry() && entry.cast_from_command)
                    && let Some(record) = cast
                        .spell()
                        .object
                        .card_id()
                        .and_then(|id| self.commanders.get_mut(&id))
                {
                    record.record_cast();
                }
                cast_spend_events.extend(self.mana_spend_effects(
                    cast.entry(),
                    cast.controller(),
                    &evaluated,
                )?);
            }
            if let RulesOccurrence::ZoneChange(change) = occurrence
                && change.before.object.zone() == ObjectZone::Stack
            {
                self.forget_departed_commander_mana(change.before.object.id());
            }
        }
        values.extend(cast_spend_events);
        values.extend(
            blessing
                .into_iter()
                .map(|player| RulesOccurrence::CityBlessingGained { player }),
        );
        for (&object, after) in &self.characteristics.lore {
            let old = before
                .characteristics
                .lore
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into());
            if old != *after {
                values.push(RulesOccurrence::LoreCountersChanged {
                    object,
                    before: old,
                    after: after.clone(),
                    exact_chapter_only: object >= self.turn_start_incarnation
                        && evaluated.get(object).is_some_and(|view| {
                            matches!(
                                view.abilities.entry_tap,
                                Some(crate::land::EntryTap::ReadAhead { .. })
                            )
                        }),
                });
            }
        }
        if !self.pending_entry_reveals.is_empty() {
            let mut reveals = std::mem::take(&mut self.pending_entry_reveals)
                .into_iter()
                .map(|object| RulesOccurrence::HandCardRevealed { object })
                .collect::<Vec<_>>();
            reveals.append(&mut values);
            values = reveals;
        }
        if old_clock != clock || !flipped.is_empty() {
            values.extend(Self::day_change_occurrences(
                old_clock,
                clock,
                &flipped,
                &before.view(),
                &self.rules_view_with_evaluated(turn, evaluated.clone()),
            )?);
        }
        for value in &values {
            if let RulesOccurrence::CastCompleted(cast) = value {
                let count = self
                    .spells_cast_by_player
                    .entry(cast.controller())
                    .or_default();
                *count = (*count + 1).min(2);
                if let Some(stack_entry) = self
                    .window
                    .entries()
                    .iter()
                    .find(|e| e.id() == cast.entry())
                {
                    self.characteristics
                        .cast_summaries
                        .insert(cast.spell().object.id(), stack_entry.cast_summary().clone());
                    if stack_entry.paid_conditional_cost()
                        && self
                            .copiable_program(cast.spell().object.id())
                            .is_some_and(|prog| {
                                matches!(
                                    prog.conditional_cost,
                                    Some(crate::program::ConditionalCastCost::EndStep {
                                        kind: crate::program::EndStepCast::Warp,
                                        ..
                                    })
                                )
                            })
                    {
                        self.characteristics.any_spell_warped_this_turn = true;
                    }
                }
            }
        }
        for event in &values {
            if let RulesOccurrence::ZoneChange(change) = event {
                let after = &change.after.object;
                if let Some(record) = after.card_id().and_then(|id| self.commanders.get_mut(&id)) {
                    record.return_eligible =
                        matches!(after.zone(), ObjectZone::Exile | ObjectZone::Graveyard(_))
                            .then_some(after.id());
                }
            }
        }
        let reveals = values
            .iter()
            .filter_map(|value| {
                let RulesOccurrence::ZoneChange(change) = value else {
                    return None;
                };
                let object = &change.before.object;
                ((before.characteristics.foretold.contains_key(&object.id())
                    && change.reason == ZoneReason::CastAnnouncement)
                    || object.face_down().is_some()
                        && (object.zone() == ObjectZone::Battlefield
                            || (object.zone() == ObjectZone::Stack
                                && change.after.object.zone() != ObjectZone::Battlefield)))
                    .then(|| RulesOccurrence::FaceDownRevealed {
                        object: object.clone(),
                    })
            })
            .collect::<Vec<_>>();
        values.extend(reveals);
        let mut target_events = Vec::new();
        if let EventCause::CastAnnouncement { entry }
        | EventCause::ActivatedAbilityCompleted { entry, .. }
        | EventCause::TriggerTargetsBound { entry } = cause
        {
            target_events.extend(self.permanent_target_events(entry, &evaluated)?);
            let extra = self.extra_targeted_by_controller_events(entry, &evaluated, &target_events);
            target_events.extend(extra);
        }
        for occurrence in &values {
            match occurrence {
                RulesOccurrence::AbilityCopied { entry, .. }
                | RulesOccurrence::CopyCreated { entry, .. } => {
                    target_events.extend(self.permanent_target_events(*entry, &evaluated)?);
                    let extra = self.extra_targeted_by_controller_events(
                        *entry,
                        &evaluated,
                        &target_events,
                    );
                    target_events.extend(extra);
                }
                RulesOccurrence::StackTargetsChanged { entry, before, .. } => {
                    let previous: BTreeSet<_> = before.iter().map(|bound| bound.target).collect();
                    target_events.extend(
                        self.permanent_target_events(*entry, &evaluated)?
                            .into_iter()
                            .filter(|event| match event {
                                RulesOccurrence::PermanentTargeted { object, .. } => {
                                    !previous.contains(&TargetRef::Object(*object))
                                }
                                RulesOccurrence::PlayerTargeted { player, .. } => {
                                    !previous.contains(&TargetRef::Player(*player))
                                }
                                _ => false,
                            }),
                    );
                    let extra: Vec<_> = self
                        .extra_targeted_by_controller_events(*entry, &evaluated, &target_events)
                        .into_iter()
                        .filter(|event| {
                            matches!(
                                event,
                                RulesOccurrence::PermanentTargeted { object, .. }
                                    if !previous.contains(&TargetRef::Object(*object))
                            )
                        })
                        .collect();
                    target_events.extend(extra);
                }
                _ => {}
            }
        }
        values.extend(target_events);
        if !ended_control_effects.is_empty() {
            values.push(RulesOccurrence::SourceControlEffectsEnded {
                effects: ended_control_effects,
            });
        }
        values.extend(
            world_changes
                .into_iter()
                .map(RulesOccurrence::WorldTenureChanged),
        );
        values.extend(
            controllers
                .into_iter()
                .map(RulesOccurrence::ControllerChanged),
        );
        if !removed.is_empty() {
            values.push(RulesOccurrence::CombatantsRemoved {
                participants: removed,
            });
        }
        // A state trigger can trigger again as soon as its previous instance
        // leaves the stack. Freeze each relevant controller/subtype count once.
        use crate::program::{SpellTargetShape, StateThresholdKind, TriggerEvent};
        let mut subtype_counts = BTreeMap::new();
        for current in evaluated
            .objects
            .values()
            .filter(|object| object.zone == ObjectZone::Battlefield)
        {
            let object = current.object;
            let Some(controller) = current.controller else {
                continue;
            };
            let printed = current
                .printed_text
                .triggered
                .iter()
                .filter(|definition| current.abilities.triggered.contains(&definition.id))
                .map(|definition| (None, definition));
            let granted = current.granted_triggers.iter().flat_map(|(grant, defs)| {
                defs.iter()
                    .map(move |definition| (Some(*grant), definition))
            });
            for (grant, definition) in printed.chain(granted) {
                let candidate = match definition.condition.event {
                    TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                        kind,
                        comparison,
                        amount,
                    }) if comparison.evaluate(
                        &self.counters_of_kind(object, kind),
                        &i64::from(amount).into(),
                    ) =>
                    {
                        RulesOccurrence::CounterThresholdMet {
                            object,
                            definition: definition.id,
                            grant,
                            kind,
                        }
                    }
                    TriggerEvent::StateThreshold(StateThresholdKind::ControlledSubtype {
                        subtype,
                        minimum,
                    }) => {
                        let count =
                            subtype_counts
                                .entry((controller, subtype))
                                .or_insert_with(|| {
                                    evaluated
                                        .objects
                                        .values()
                                        .filter(|object| {
                                            object.zone == ObjectZone::Battlefield
                                                && object.controller == Some(controller)
                                                && object
                                                    .current_creature_subtypes
                                                    .contains(&subtype)
                                        })
                                        .count()
                                });
                        if *count < usize::from(minimum) {
                            continue;
                        }
                        RulesOccurrence::SubtypeThresholdMet {
                            object,
                            definition: definition.id,
                            grant,
                        }
                    }
                    TriggerEvent::StateThreshold(StateThresholdKind::Eval(ref cond)) => {
                        let ctx = crate::characteristics::WorldEvalCtx {
                            active_player: self.window.active_player(),
                            step: self.current_step,
                            participants: self.window.participants(),
                            life: Some(&self.life),
                            combat: self.combat.as_ref(),
                            cards: self.window.cards(),
                            programs: &self.programs,
                            state: &self.characteristics,
                            objects: &evaluated.objects,
                            source: object,
                            controller,
                            name_rules: &self.name_rules,
                            trigger_subject: None,
                            related_player: None,
                            defending_player: None,
                        };
                        if !crate::characteristics::eval_world_condition(&ctx, cond) {
                            continue;
                        }
                        RulesOccurrence::StateConditionMet {
                            object,
                            definition: definition.id,
                            grant,
                        }
                    }
                    _ => continue,
                };
                let pending = self.triggers.waiting().values().any(|waiting| {
                    waiting.source.object.id() == object
                        && waiting.definition == definition.id
                        && waiting.granting_effect == grant
                }) || self
                    .window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .chain(self.window.checking_targets())
                    .filter_map(|entry| {
                        self.execution_bindings(entry.id())
                            .and_then(|bindings| bindings.trigger.as_ref())
                    })
                    .any(|bound| {
                        bound.source.object.id() == object
                            && bound.definition == definition.id
                            && bound.granting_effect == grant
                    });
                if !pending {
                    values.push(candidate);
                }
            }
        }
        if values.is_empty() {
            return Ok(());
        }
        if !self.adventured.is_empty() {
            for event in &values {
                if let RulesOccurrence::ZoneChange(change) = event
                    && change.before.object.zone() == ObjectZone::Exile
                {
                    self.adventured
                        .retain(|(id, _)| *id != change.before.object.id());
                }
            }
        }
        if !self.linked_search_results.is_empty()
            || !self.characteristics.imprinted_cards.is_empty()
        {
            for event in &values {
                if let RulesOccurrence::ZoneChange(change) = event
                    && change.before.object.zone() == ObjectZone::Stack
                    && let Some(linked) = self
                        .linked_search_results
                        .remove(&change.before.object.id())
                    && change.after.object.zone() == ObjectZone::Battlefield
                    && change.reason == ZoneReason::PermanentResolution
                {
                    self.linked_search_results
                        .insert(change.after.object.id(), linked);
                }
            }
            let departed: BTreeSet<_> = values
                .iter()
                .filter_map(|value| match value {
                    RulesOccurrence::ZoneChange(change)
                        if change.before.object.zone() == ObjectZone::Exile =>
                    {
                        Some(change.before.object.id())
                    }
                    _ => None,
                })
                .collect();
            if !departed.is_empty() {
                self.linked_search_results.retain(|_, results| {
                    results.retain(|id| !departed.contains(id));
                    !results.is_empty()
                });
                self.characteristics.imprinted_cards.retain(|_, cards| {
                    cards.retain(|id| !departed.contains(id));
                    !cards.is_empty()
                });
            }
        }
        // A source might regain its cast trigger during payment. Inspect the completed
        // spell's sole target before freezing a copy for that trigger's later resolution.
        for event in &mut values {
            let RulesOccurrence::CastCompleted(cast) = event else {
                continue;
            };
            if cast.copy_source.is_some()
                || !crate::program::SpellFilter::instant_or_sorcery()
                    .matches_spell_appearance(cast.spell())
            {
                continue;
            }
            let Some(entry) = self
                .window
                .entries()
                .iter()
                .find(|entry| entry.id() == cast.entry())
            else {
                continue;
            };
            let targets = entry.targets().targets();
            let Some(TargetRef::Object(id)) = targets.first().map(|bound| bound.choice().target)
            else {
                continue;
            };
            let is_targeting_only_source = |trigger: &crate::program::TriggeredAbility| {
                matches!(
                    trigger.condition.event,
                    TriggerEvent::SpellCast {
                        caster: crate::program::TriggerPlayerScope::Relation(crate::program::PlayerRelation::Controller),
                        filter,
                        self_cast: false,
                        include_copy: false,
                        min_mana_value: None,
                    } if filter.target_shape == SpellTargetShape::TargetingOnlySource
                )
            };
            if targets
                .iter()
                .all(|bound| bound.choice().target == TargetRef::Object(id))
                && evaluated.get(id).is_some_and(|view| {
                    view.zone == ObjectZone::Battlefield
                        && view.controller == Some(cast.controller())
                        && (view.printed_text.triggered.iter().any(|trigger| {
                            view.abilities.triggered.contains(&trigger.id)
                                && is_targeting_only_source(trigger)
                        }) || view
                            .granted_triggers
                            .values()
                            .flatten()
                            .any(is_targeting_only_source))
                })
            {
                cast.copy_source =
                    Some(Box::new(self.capture_copy_source(cast.entry(), 0.into())?));
            }
        }
        if let Some(history) = &mut self.spell_cast_positions {
            for event in &values {
                if let RulesOccurrence::CastCompleted(cast) = event {
                    history.count = history
                        .count
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                    history.positions.insert(cast.entry(), history.count);
                }
            }
        }
        if let Some(ordinals) = self.spell_ordinals.as_mut() {
            for event in &mut values {
                if let RulesOccurrence::CastCompleted(cast) = event {
                    let count = ordinals
                        .entry(cast.controller())
                        .or_insert_with(|| 0.into());
                    *count = count.plus(&1.into());
                    cast.ordinal_this_turn = Some(count.clone());
                }
            }
        }
        let cause_actor = match &cause {
            EventCause::TurnFaceUp { actor, .. } => Some(*actor),
            EventCause::Instruction { entry, .. }
            | EventCause::ActivatedAbilityCost { entry, .. } => self
                .window
                .entries()
                .iter()
                .chain(self.window.resolving())
                .find(|item| item.id() == *entry)
                .map(crate::stack_window::StackEntry::controller),
            _ => None,
        };
        let departing_link_sources = values
            .iter()
            .filter_map(RulesOccurrence::departing_permanent)
            .map(|previous| previous.object.id())
            .collect::<Vec<_>>();
        let mut batch_creature_damage: BTreeMap<ObjectId, RulesInteger> = BTreeMap::new();
        let mut batch_pw_damage: BTreeMap<ObjectId, RulesInteger> = BTreeMap::new();
        let mut batch_battle_damage: BTreeMap<ObjectId, RulesInteger> = BTreeMap::new();
        for value in &values {
            if let Some(permanent) = value.departing_permanent()
                && let Some(defenders) = self.departed_attack_defenders.as_mut()
                && let Some(attacker) = before
                    .combat
                    .as_ref()
                    .and_then(|combat| combat.attacker(permanent.object.id()))
            {
                defenders.insert(
                    permanent.object.id(),
                    (attacker.defender(), attacker.defending_planeswalker()),
                );
            }
            match value {
                RulesOccurrence::ZoneChange(change) => {
                    if matches!(change.after.object.zone(), ObjectZone::Graveyard(_)) {
                        self.characteristics
                            .put_into_gy_this_turn
                            .insert(change.after.object.id());
                        if matches!(change.before.object.zone(), ObjectZone::Library(_)) {
                            self.characteristics
                                .put_into_gy_from_library_this_turn
                                .insert(change.after.object.id());
                        }
                    }
                    if change.before.object.zone() == ObjectZone::Battlefield
                        && matches!(change.after.object.zone(), ObjectZone::Graveyard(_))
                    {
                        self.characteristics
                            .died_this_turn
                            .insert(change.after.object.id());
                    }
                    if change.before.object.zone() == ObjectZone::Battlefield
                        && change.before.is_creature()
                        && matches!(change.after.object.zone(), ObjectZone::Graveyard(_))
                    {
                        self.creatures_died_this_turn =
                            self.creatures_died_this_turn.plus(&1.into());
                        self.characteristics.creatures_died_this_turn_count = self
                            .characteristics
                            .creatures_died_this_turn_count
                            .saturating_add(1);
                        if let Some(ctrl) = change.before.object.controller() {
                            let count = self
                                .characteristics
                                .controlled_creatures_died_this_turn
                                .entry(ctrl)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                        }
                    }
                    if change.after.object.zone() == ObjectZone::Battlefield
                        && change.before.object.zone() != ObjectZone::Battlefield
                    {
                        self.characteristics
                            .entered_objects_this_turn
                            .insert(change.after.object.id());
                        if matches!(change.before.object.zone(), ObjectZone::Graveyard(_)) {
                            self.characteristics
                                .entered_from_graveyard
                                .insert(change.after.object.id());
                        }
                        let origin_zone = match change.before.object.zone() {
                            ObjectZone::Hand(_) => Some(crate::program::SourceZoneFilter::Hand),
                            ObjectZone::Graveyard(owner) => {
                                Some(if Some(owner) == change.after.object.controller() {
                                    crate::program::SourceZoneFilter::Graveyard
                                } else {
                                    crate::program::SourceZoneFilter::OpponentGraveyard
                                })
                            }
                            ObjectZone::Exile => Some(
                                if Some(change.before.object.owner())
                                    == change.after.object.controller()
                                {
                                    crate::program::SourceZoneFilter::Exile
                                } else {
                                    crate::program::SourceZoneFilter::OpponentExile
                                },
                            ),
                            ObjectZone::Library(_) => {
                                Some(crate::program::SourceZoneFilter::YourLibrary)
                            }
                            _ => None,
                        };
                        if let Some(zone_filter) = origin_zone {
                            self.characteristics
                                .entered_from_zone
                                .insert(change.after.object.id(), zone_filter);
                        }
                        if change.before.object.zone() == ObjectZone::Stack
                            && let Some((caster, from_hand)) = change.before.cast_origin
                        {
                            if change.after.object.controller() == Some(caster) {
                                self.characteristics
                                    .cast_by_controller_objects
                                    .insert(change.after.object.id());
                            }
                            if from_hand {
                                self.characteristics
                                    .cast_from_hand_objects
                                    .insert(change.after.object.id());
                            }
                        }
                        if let Some(ctrl) = change.after.object.controller() {
                            let entered = self
                                .characteristics
                                .permanents_entered_this_turn
                                .entry(ctrl)
                                .or_insert(0);
                            *entered = entered.saturating_add(1);
                            let by_type = self
                                .characteristics
                                .entered_types_this_turn
                                .entry(ctrl)
                                .or_default();
                            for &kind in &change.after.types {
                                let entry = by_type.entry(kind).or_insert(0);
                                *entry = entry.saturating_add(1);
                            }
                            if change.after.is_creature() {
                                let count = self
                                    .characteristics
                                    .creatures_entered_this_turn
                                    .entry(ctrl)
                                    .or_insert(0);
                                *count = count.saturating_add(1);
                            }
                            if change.after.types.contains(&CardType::Land) {
                                let count = self
                                    .characteristics
                                    .lands_entered_this_turn
                                    .entry(ctrl)
                                    .or_insert(0);
                                *count = count.saturating_add(1);
                            }
                            if change.reason == crate::occurrence::ZoneReason::LandPlay {
                                let count = self
                                    .characteristics
                                    .lands_played_this_turn
                                    .entry(ctrl)
                                    .or_insert(0);
                                *count = count.saturating_add(1);
                            }
                        }
                    }
                    if change.before.object.zone() == ObjectZone::Battlefield
                        && change.after.object.zone() != ObjectZone::Battlefield
                        && let Some(ctrl) = change.before.object.controller()
                    {
                        let count = self
                            .characteristics
                            .permanents_left_battlefield_this_turn
                            .entry(ctrl)
                            .or_insert(0);
                        *count = count.saturating_add(1);
                        if !change.before.types.contains(&CardType::Land) {
                            let nonland = self
                                .characteristics
                                .nonland_permanents_left_battlefield_this_turn
                                .entry(ctrl)
                                .or_insert(0);
                            *nonland = nonland.saturating_add(1);
                        }
                    }
                    if let ObjectZone::Graveyard(owner) = change.before.object.zone()
                        && change.after.object.zone() != ObjectZone::Graveyard(owner)
                        && change.before.object.card_id().is_some()
                    {
                        let count = self
                            .characteristics
                            .cards_left_graveyard_this_turn
                            .entry(owner)
                            .or_insert(0);
                        *count = count.saturating_add(1);
                    }
                    match change.reason {
                        crate::occurrence::ZoneReason::Discard { player }
                        | crate::occurrence::ZoneReason::CycleDiscard { player }
                        | crate::occurrence::ZoneReason::TransmuteDiscard { player } => {
                            let count = self
                                .characteristics
                                .cards_discarded_this_turn
                                .entry(player)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                        }
                        crate::occurrence::ZoneReason::Sacrifice { player } => {
                            let count = self
                                .characteristics
                                .permanents_sacrificed_this_turn
                                .entry(player)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                            self.characteristics
                                .sacrificed_this_turn
                                .entry(player)
                                .or_default()
                                .push(appearance_to_zone_move_record(change, &change.before));
                        }
                        crate::occurrence::ZoneReason::SacrificeCost => {
                            if let Some(ctrl) = change.before.object.controller() {
                                let count = self
                                    .characteristics
                                    .permanents_sacrificed_this_turn
                                    .entry(ctrl)
                                    .or_insert(0);
                                *count = count.saturating_add(1);
                                self.characteristics
                                    .sacrificed_this_turn
                                    .entry(ctrl)
                                    .or_default()
                                    .push(appearance_to_zone_move_record(change, &change.before));
                            }
                        }
                        crate::occurrence::ZoneReason::StateBased(reasons)
                            if reasons.contains(
                                crate::occurrence::StateBasedMoveReason::CompletedSaga,
                            ) =>
                        {
                            if let Some(ctrl) = change.before.object.controller() {
                                self.characteristics
                                    .sacrificed_this_turn
                                    .entry(ctrl)
                                    .or_default()
                                    .push(appearance_to_zone_move_record(change, &change.before));
                            }
                        }
                        _ => {}
                    }
                    if change.after.object.zone() == ObjectZone::Exile
                        && change.before.object.zone() != ObjectZone::Exile
                        && change.after.object.card_id().is_some()
                    {
                        self.characteristics.cards_exiled_this_turn = self
                            .characteristics
                            .cards_exiled_this_turn
                            .saturating_add(1);
                    }
                    for extra in &change.additional_arrivals {
                        if extra.object.zone() == ObjectZone::Exile
                            && extra.object.card_id().is_some()
                        {
                            self.characteristics.cards_exiled_this_turn = self
                                .characteristics
                                .cards_exiled_this_turn
                                .saturating_add(1);
                        }
                    }
                    if let ObjectZone::Graveyard(owner) = change.after.object.zone()
                        && change.before.object.zone() != ObjectZone::Graveyard(owner)
                    {
                        if change.after.object.card_id().is_some() {
                            let count = self
                                .characteristics
                                .cards_entered_graveyard_this_turn
                                .entry(owner)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                        }
                        let from_battlefield =
                            change.before.object.zone() == ObjectZone::Battlefield;
                        if from_battlefield || change.after.object.card_id().is_some() {
                            let app = if from_battlefield {
                                &change.before
                            } else {
                                &change.after
                            };
                            self.characteristics
                                .cards_put_into_graveyard_this_turn
                                .push(appearance_to_zone_move_record(change, app));
                        }
                    }
                    for extra in &change.additional_arrivals {
                        if matches!(extra.object.zone(), ObjectZone::Graveyard(_))
                            && extra.object.card_id().is_some()
                        {
                            self.characteristics
                                .cards_put_into_graveyard_this_turn
                                .push(appearance_to_zone_move_record(change, extra));
                        }
                    }
                    if let ObjectZone::Graveyard(owner) = change.after.object.zone()
                        && change.before.object.card_id().is_some()
                        && change.before.types.iter().any(|t| {
                            matches!(
                                t,
                                CardType::Artifact
                                    | CardType::Battle
                                    | CardType::Creature
                                    | CardType::Enchantment
                                    | CardType::Land
                                    | CardType::Planeswalker
                            )
                        })
                    {
                        self.characteristics.descended_this_turn.insert(owner);
                    }
                }
                RulesOccurrence::LifeChanged(change) => match change.kind {
                    crate::life::LifeChangeKind::Loss => {
                        let lost = self
                            .characteristics
                            .life_lost_this_turn
                            .entry(change.player)
                            .or_insert_with(|| 0.into());
                        *lost = lost.plus(&change.amount);
                    }
                    crate::life::LifeChangeKind::Gain => {
                        let gained = self
                            .characteristics
                            .life_gained_this_turn
                            .entry(change.player)
                            .or_insert_with(|| 0.into());
                        *gained = gained.plus(&change.amount);
                    }
                },
                RulesOccurrence::CastCompleted(cast) => {
                    let ctrl = cast.controller();
                    let count = self
                        .characteristics
                        .spells_cast_this_turn
                        .entry(ctrl)
                        .or_insert(0);
                    *count = count.saturating_add(1);
                    let by_type = self
                        .characteristics
                        .cast_spell_types_this_turn
                        .entry(ctrl)
                        .or_default();
                    let mut type_mask = crate::program::CardTypeMask::EMPTY;
                    for &kind in &cast.spell().types {
                        type_mask = type_mask.with(kind);
                        let entry = by_type.entry(kind).or_insert(0);
                        *entry = entry.saturating_add(1);
                    }
                    let mut color_mask = crate::program::ColorMask::EMPTY;
                    for &color in &cast.spell().colors {
                        color_mask = color_mask.with(color);
                    }
                    let mut supertype_mask = crate::program::SupertypeMask::EMPTY;
                    for &sup in &cast.spell().current_supertypes {
                        supertype_mask = supertype_mask
                            .union(crate::program::SupertypeMask::from_supertype(sup));
                    }
                    let from_hand = cast
                        .spell()
                        .cast_origin
                        .is_some_and(|(_, from_hand)| from_hand);
                    let summary = self
                        .characteristics
                        .cast_summaries
                        .get(&cast.spell().object.id());
                    let from_graveyard = summary.is_some_and(|s| s.cast_from_graveyard);
                    let from_exile = summary.is_some_and(|s| s.cast_from_exile);
                    self.characteristics
                        .cast_spells_this_turn
                        .entry(ctrl)
                        .or_default()
                        .push(crate::characteristics::CastSpellTurnRecord {
                            types: type_mask,
                            colors: color_mask,
                            supertypes: supertype_mask,
                            subtypes: cast.spell().creature_subtypes.clone(),
                            from_hand,
                            from_graveyard,
                            from_exile,
                        });
                    if !cast.spell().is_creature() {
                        let noncreature = self
                            .characteristics
                            .noncreature_spells_cast_this_turn
                            .entry(ctrl)
                            .or_insert(0);
                        *noncreature = noncreature.saturating_add(1);
                    }
                    if from_hand {
                        let from_hand_count = self
                            .characteristics
                            .spells_cast_from_hand_this_turn
                            .entry(ctrl)
                            .or_insert(0);
                        *from_hand_count = from_hand_count.saturating_add(1);
                    }
                    let mv = cast
                        .mana_value()
                        .to_usize()
                        .map_or(0u32, |n| u32::try_from(n).unwrap_or(u32::MAX));
                    let total_mv = self
                        .characteristics
                        .other_spells_mv_this_turn
                        .entry(ctrl)
                        .or_insert(0);
                    *total_mv = total_mv.saturating_add(mv);
                    if !cast.spell().kicker_choices.is_empty() {
                        self.characteristics.kicker_counts.insert(
                            cast.spell().object.id(),
                            cast.spell().kicker_choices.clone(),
                        );
                    }
                }
                RulesOccurrence::ActivatedAbility { entry, source, .. } => {
                    if matches!(source.zone(), ObjectZone::Graveyard(_)) {
                        let actor = self
                            .window
                            .entries()
                            .iter()
                            .chain(self.window.resolving())
                            .find(|item| item.id() == *entry)
                            .map(crate::stack_window::StackEntry::controller)
                            .unwrap_or_else(|| source.owner());
                        self.characteristics
                            .activated_graveyard_ability_this_turn
                            .insert(actor);
                    }
                }
                RulesOccurrence::AttackersDeclared { selected, .. } => {
                    for attack in selected {
                        self.characteristics
                            .attacked_objects_this_turn
                            .insert(attack.attacker);
                        if let Some(view) = before.evaluated.get(attack.attacker)
                            && let Some(ctrl) = view.controller
                        {
                            self.characteristics.attacked_this_turn_players.insert(ctrl);
                            self.characteristics
                                .attacking_creatures_this_turn
                                .entry(ctrl)
                                .or_default()
                                .insert(attack.attacker);
                            let mut types = crate::program::CardTypeMask::EMPTY;
                            for &kind in &view.current_types {
                                types = types.with(kind);
                            }
                            let mut colors = crate::program::ColorMask::EMPTY;
                            for &color in &view.current_colors {
                                colors = colors.with(color);
                            }
                            let mut supertypes = crate::program::SupertypeMask::EMPTY;
                            for &sup in &view.current_supertypes {
                                supertypes = supertypes
                                    .union(crate::program::SupertypeMask::from_supertype(sup));
                            }
                            self.characteristics
                                .attacked_records_this_turn
                                .entry(ctrl)
                                .or_default()
                                .push(crate::characteristics::ZoneMoveTurnRecord {
                                    owner: view.owner,
                                    previous_controller: Some(ctrl),
                                    from_battlefield: true,
                                    types,
                                    colors,
                                    supertypes,
                                    subtypes: view.current_creature_subtypes.clone(),
                                    artifact_subtypes: view.current_artifact_subtypes.clone(),
                                });
                        }
                    }
                }
                RulesOccurrence::BlockersDeclared { selected, .. } => {
                    for pair in selected {
                        self.characteristics
                            .blocked_objects_this_turn
                            .insert(pair.blocker);
                    }
                }
                RulesOccurrence::CardDrawOrdinal { player, .. } => {
                    let count = self
                        .characteristics
                        .cards_drawn_this_turn_counts
                        .entry(*player)
                        .or_insert(0);
                    *count = count.saturating_add(1);
                }
                RulesOccurrence::PermanentTargeted {
                    object, controller, ..
                } => {
                    if before
                        .evaluated
                        .get(*object)
                        .and_then(|o| o.controller)
                        .is_some_and(|owner| owner != *controller)
                    {
                        self.characteristics
                            .committed_crime_this_turn
                            .insert(*controller);
                    }
                }
                RulesOccurrence::PlayerTargeted {
                    player, controller, ..
                } => {
                    if *player != *controller {
                        self.characteristics
                            .committed_crime_this_turn
                            .insert(*controller);
                    }
                }
                RulesOccurrence::TokenCreated { creator, after } => {
                    self.token_creators_this_turn.insert(*creator);
                    let tok_count = self
                        .characteristics
                        .tokens_created_this_turn
                        .entry(*creator)
                        .or_insert(0);
                    *tok_count = tok_count.saturating_add(1);
                    self.characteristics
                        .entered_objects_this_turn
                        .insert(after.object.id());
                    if let Some(ctrl) = after.object.controller() {
                        let entered = self
                            .characteristics
                            .permanents_entered_this_turn
                            .entry(ctrl)
                            .or_insert(0);
                        *entered = entered.saturating_add(1);
                        let by_type = self
                            .characteristics
                            .entered_types_this_turn
                            .entry(ctrl)
                            .or_default();
                        for &kind in &after.types {
                            let entry = by_type.entry(kind).or_insert(0);
                            *entry = entry.saturating_add(1);
                        }
                        if after.is_creature() {
                            let count = self
                                .characteristics
                                .creatures_entered_this_turn
                                .entry(ctrl)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                        }
                        if after.types.contains(&CardType::Land) {
                            let count = self
                                .characteristics
                                .lands_entered_this_turn
                                .entry(ctrl)
                                .or_insert(0);
                            *count = count.saturating_add(1);
                        }
                    }
                    if after.is_creature()
                        && let Some(total) = &after.battlefield_total_counters
                        && total.is_positive()
                    {
                        let amount = total
                            .to_usize()
                            .map_or(0u16, |n| u16::try_from(n).unwrap_or(u16::MAX));
                        if amount > 0 {
                            let seat = after.object.controller().unwrap_or(*creator);
                            let count = self
                                .characteristics
                                .counters_placed_on_creatures_this_turn
                                .entry(seat)
                                .or_insert(0);
                            *count = count.saturating_add(amount);
                        }
                    }
                }
                RulesOccurrence::DieRolled { player, .. }
                | RulesOccurrence::KeywordActionPerformed {
                    player,
                    action: crate::program::PlayerTriggerAction::RolledDie { .. },
                } => {
                    let count = self
                        .characteristics
                        .dice_rolled_this_turn
                        .entry(*player)
                        .or_insert(0);
                    *count = count.saturating_add(1);
                }
                RulesOccurrence::CountersPlaced {
                    before_appearance,
                    after_appearance,
                    added,
                    ..
                } => {
                    if after_appearance.is_creature() {
                        let placed = added.plus_one.plus(&added.minus_one);
                        let amount = placed
                            .to_usize()
                            .map_or(0u16, |n| u16::try_from(n).unwrap_or(u16::MAX));
                        if amount > 0 {
                            let actor =
                                if before_appearance.object.zone() != ObjectZone::Battlefield {
                                    after_appearance.object.controller()
                                } else {
                                    cause_actor.or_else(|| after_appearance.object.controller())
                                };
                            if let Some(seat) = actor {
                                let count = self
                                    .characteristics
                                    .counters_placed_on_creatures_this_turn
                                    .entry(seat)
                                    .or_insert(0);
                                *count = count.saturating_add(amount);
                            }
                        }
                    }
                }
                RulesOccurrence::PermanentCounterChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                    ..
                }
                | RulesOccurrence::ChargeCountersChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                }
                | RulesOccurrence::LoyaltyCountersChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                }
                | RulesOccurrence::SuspectCountersChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                }
                | RulesOccurrence::StorageCountersChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                }
                | RulesOccurrence::LoreCountersChanged {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                    ..
                }
                | RulesOccurrence::AgeCounterPlaced {
                    object,
                    before: old_cnt,
                    after: new_cnt,
                } => {
                    if new_cnt > old_cnt
                        && let Some(perm) = evaluated
                            .get(*object)
                            .filter(|o| o.zone == ObjectZone::Battlefield && o.is_creature())
                    {
                        let delta = new_cnt.minus(old_cnt);
                        let amount = delta
                            .to_usize()
                            .map_or(0u16, |n| u16::try_from(n).unwrap_or(u16::MAX));
                        if amount > 0
                            && let Some(seat) = cause_actor.or(perm.controller)
                        {
                            let count = self
                                .characteristics
                                .counters_placed_on_creatures_this_turn
                                .entry(seat)
                                .or_insert(0);
                            *count = count.saturating_add(amount);
                        }
                    }
                }
                RulesOccurrence::DamageDealt { damage, kind, .. }
                    if damage.amount.is_positive() =>
                {
                    if self.characteristics.track_damage_dealt_by
                        && let crate::damage::DamageSourceId::Object(dealer) = damage.source.id()
                    {
                        let dealt = self
                            .characteristics
                            .damage_dealt_this_turn_by
                            .entry(dealer)
                            .or_insert_with(|| 0.into());
                        *dealt = dealt.plus(&damage.amount);
                        if let crate::damage::DamageRecipient::Player(player) = damage.target {
                            let dealer_ctrl = before
                                .evaluated
                                .get(dealer)
                                .and_then(|o| o.controller)
                                .or_else(|| {
                                    self.departed_damage_sources
                                        .get(&dealer)
                                        .and_then(|s| s.object.controller())
                                });
                            if dealer_ctrl.is_some_and(|ctrl| ctrl != player) {
                                self.characteristics
                                    .damage_dealt_to_opponents_this_turn
                                    .entry(dealer)
                                    .or_default()
                                    .insert(player);
                            }
                            if *kind == crate::damage::DamageKind::Combat {
                                self.characteristics
                                    .combat_damage_dealt_to_player_this_turn
                                    .insert(dealer);
                            }
                        }
                    }
                    match damage.target {
                        crate::damage::DamageRecipient::Player(player) => {
                            self.characteristics
                                .players_dealt_damage_this_turn
                                .insert(player);
                            if *kind == crate::damage::DamageKind::Noncombat {
                                self.characteristics
                                    .players_dealt_noncombat_damage_this_turn
                                    .insert(player);
                            }
                        }
                        crate::damage::DamageRecipient::Creature(id) => {
                            let total = self
                                .characteristics
                                .object_damage_taken_this_turn
                                .entry(id)
                                .or_insert_with(|| 0.into());
                            *total = total.plus(&damage.amount);
                            let batch_total =
                                batch_creature_damage.entry(id).or_insert_with(|| 0.into());
                            *batch_total = batch_total.plus(&damage.amount);
                            let before_marks =
                                before.marks.get(&id).cloned().unwrap_or_else(|| 0.into());
                            let total_marked = before_marks.plus(batch_total);
                            let deathtouch =
                                match damage.source.id() {
                                    crate::damage::DamageSourceId::Object(dealer) => {
                                        before.evaluated.get(dealer).is_some_and(|s| {
                                            s.has_keyword(KeywordAbility::Deathtouch)
                                        }) || self.departed_damage_sources.get(&dealer).is_some_and(
                                            |s| s.keywords.contains(&KeywordAbility::Deathtouch),
                                        )
                                    }
                                    _ => false,
                                };
                            let is_excess = (deathtouch
                                && (damage.amount > 1.into() || before_marks.is_positive()))
                                || before
                                    .evaluated
                                    .get(id)
                                    .and_then(|o| o.power_toughness.as_ref())
                                    .is_some_and(|pt| total_marked > pt.derived.toughness);
                            if is_excess {
                                self.characteristics
                                    .excess_damage_received_this_turn
                                    .insert(id);
                            }
                            if let Some(pairs) = &mut self.characteristics.creature_damage_pairs
                                && let crate::damage::DamageSourceId::Object(dealer) =
                                    damage.source.id()
                            {
                                pairs.insert((dealer, id));
                            }
                        }
                        crate::damage::DamageRecipient::Planeswalker(id) => {
                            let total = self
                                .characteristics
                                .object_damage_taken_this_turn
                                .entry(id)
                                .or_insert_with(|| 0.into());
                            *total = total.plus(&damage.amount);
                            let batch_total = batch_pw_damage.entry(id).or_insert_with(|| 0.into());
                            *batch_total = batch_total.plus(&damage.amount);
                            let before_loyalty = before
                                .characteristics
                                .loyalty
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| 0.into());
                            if *batch_total > before_loyalty {
                                self.characteristics
                                    .excess_damage_received_this_turn
                                    .insert(id);
                            }
                        }
                        crate::damage::DamageRecipient::Battle(id) => {
                            let total = self
                                .characteristics
                                .object_damage_taken_this_turn
                                .entry(id)
                                .or_insert_with(|| 0.into());
                            *total = total.plus(&damage.amount);
                            let batch_total =
                                batch_battle_damage.entry(id).or_insert_with(|| 0.into());
                            *batch_total = batch_total.plus(&damage.amount);
                            let before_defense = before
                                .characteristics
                                .defense
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| 0.into());
                            if *batch_total > before_defense {
                                self.characteristics
                                    .excess_damage_received_this_turn
                                    .insert(id);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.update_growth_last_known(&values);
        for occurrence in &values {
            if let RulesOccurrence::DamageDealt {
                damage,
                kind: crate::damage::DamageKind::Combat,
                ..
            } = occurrence
                && let crate::damage::DamageSourceId::Object(source) = damage.source.id()
                && let crate::damage::DamageRecipient::Player(target) = damage.target
            {
                self.combat_damaged_players_by_source
                    .entry(source)
                    .or_default()
                    .insert(target);
            }
        }
        if let Some(history) = &mut self.damage_to_players_this_turn {
            for occurrence in &values {
                if let RulesOccurrence::DamageDealt { damage, .. } = occurrence
                    && let crate::damage::DamageRecipient::Player(player) = damage.target
                {
                    let total = history.entry(player).or_insert_with(|| 0.into());
                    *total = total.plus(&damage.amount);
                }
            }
        }
        for value in &values {
            if let RulesOccurrence::CastCompleted(cast) = value
                && let Some((grant, kind)) = self.typed_exile_pending.remove(&cast.entry())
                && let Some(cohort) = self.typed_exile_cohorts.get_mut(&grant)
            {
                cohort.used.insert(kind);
            }
            if let RulesOccurrence::CastCompleted(cast) = value
                && let Some(permissions) = self.source_cast_pending.remove(&cast.entry())
            {
                self.source_cast_uses.extend(permissions);
            }
        }
        if let Some(recent) = self.recent_graveyard_cards.as_mut() {
            for value in &values {
                let RulesOccurrence::ZoneChange(change) = value else {
                    continue;
                };
                let old = &change.before.object;
                let new = &change.after.object;
                let known = recent.remove(&old.id());
                let discarded = matches!(
                    change.reason,
                    ZoneReason::Discard { .. }
                        | ZoneReason::CycleDiscard { .. }
                        | ZoneReason::TransmuteDiscard { .. }
                );
                if new.card_id().is_some()
                    && (matches!(new.zone(), ObjectZone::Graveyard(_))
                        && (matches!(old.zone(), ObjectZone::Library(_))
                            || discarded
                            || known && change.reason == ZoneReason::MadnessDeclined)
                        || discarded && new.zone() == ObjectZone::Exile)
                {
                    recent.insert(new.id());
                }
            }
        }
        if let Some(history) = &mut self.cast_history {
            for value in &values {
                match value {
                    RulesOccurrence::ZoneChange(change) if self.track_discard_prices => {
                        let old = &change.before.object;
                        let new = &change.after.object;
                        let discarded_by = match change.reason {
                            ZoneReason::CycleDiscard { player }
                            | ZoneReason::TransmuteDiscard { player }
                            | ZoneReason::Discard { player }
                                if old.zone() == ObjectZone::Hand(player) =>
                            {
                                Some(player)
                            }
                            ZoneReason::MadnessDeclined => {
                                history.discarded.get(&old.id()).copied()
                            }
                            _ => None,
                        };
                        history.discarded.remove(&old.id());
                        if let Some(player) = discarded_by
                            && matches!(new.zone(), ObjectZone::Exile | ObjectZone::Graveyard(_))
                        {
                            history.discarded.insert(new.id(), player);
                        }
                    }
                    RulesOccurrence::CastCompleted(cast) => {
                        history.casters.insert(cast.controller());
                        history.spells = history.spells.plus(&1.into());
                    }
                    RulesOccurrence::ZoneChange(change)
                        if change.before.object.zone() == ObjectZone::Battlefield
                            && matches!(change.after.object.zone(), ObjectZone::Graveyard(_)) =>
                    {
                        history.grave_permanents = history.grave_permanents.plus(&1.into());
                    }
                    RulesOccurrence::LifeChanged(loss)
                        if loss.kind == crate::life::LifeChangeKind::Loss =>
                    {
                        history.lost_life.insert(loss.player);
                    }
                    _ => {}
                }
            }
        }
        for value in &values {
            if let Some(permanent) = value.departing_permanent() {
                self.retain_source_appearance(permanent, before);
            }
            if let RulesOccurrence::Phased { objects, out: true } = value {
                for &object in objects {
                    let appearance = before
                        .view()
                        .appearance(object)
                        .ok_or(EffectError::InconsistentProgram)?;
                    self.retain_source_appearance(&appearance, before);
                }
            }
        }
        let triggers = std::mem::take(&mut self.triggers);
        let updated = triggers.publish(
            cause,
            values,
            &before.view(),
            &self.rules_view_with_evaluated(turn, evaluated),
        )?;
        let (state, _, mut records) = updated.into_parts();
        *self.triggers = state;
        for &departed in &departing_link_sources {
            self.characteristics.entered_from_zone.remove(&departed);
            self.characteristics
                .cast_by_controller_objects
                .remove(&departed);
            self.characteristics
                .cast_from_hand_objects
                .remove(&departed);
        }
        if self.triggers.watches_ability_copies() {
            let activations = records
                .records
                .iter()
                .filter_map(|record| match record {
                    crate::occurrence::RulesRecord::Event(event) => Some(&event.occurrences),
                    _ => None,
                })
                .flatten()
                .filter_map(|row| match &row.value {
                    RulesOccurrence::ActivatedAbility { entry, source, .. } => {
                        Some((row.id, *entry, source.id()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            for (occurrence, id, source) in activations {
                let current = self.evaluated_characteristics()?;
                let captured = Arc::new(self.capture_ability(id, &current)?);
                let types = match current.get(source) {
                    Some(view) if view.zone == ObjectZone::Battlefield => {
                        &view.current_planeswalker_subtypes
                    }
                    _ => &captured.payload.activated_planeswalker_types,
                };
                self.triggers.discover_ability_copies(
                    occurrence,
                    captured.entry.controller(),
                    captured.clone(),
                    captured.payload.activated_as_loyalty,
                    types,
                    &mut records,
                )?;
            }
        }
        self.departed_quest_counts.retain(|object,_|self.triggers.waiting().values().any(|w|w.source.object.id()==*object)
            || self.window.entries().iter().chain(self.window.resolving()).chain(self.window.checking_targets())
                .any(|entry|matches!(entry.kind(),StackKind::TriggeredAbility {source} if source.id()==*object)));
        let captured_power_cohort = match &self.pending_work {
            Some(PendingWork::Damage(work)) => match &work.parent {
                DamageParent::Resolution(cursor) => cursor.results.power_damage_cohort.as_ref(),
                _ => None,
            },
            _ => self
                .resolution_cursor()
                .and_then(|cursor| cursor.results.power_damage_cohort.as_ref()),
        }
        .cloned();
        self.departed_damage_sources.retain(|source, _| {
            let refers_to_source = |causes: &crate::occurrence::TriggerCauseSet| causes.members().iter().any(|member|
                matches!(member.related, crate::occurrence::TriggerRelated::SubtypeCombatDamage {creature,..} | crate::occurrence::TriggerRelated::AttachedCreatureDamaged {creature, ..} | crate::occurrence::TriggerRelated::Combatant(creature) if creature == *source) || matches!(&member.related,crate::occurrence::TriggerRelated::ZoneChange {after,..} | crate::occurrence::TriggerRelated::TokenCreated {after,..} if after.object.id()==*source));
            captured_power_cohort.as_ref().is_some_and(|(_, subjects)| subjects.contains(source))
                || self.payloads.values().any(|payload| matches!(payload, StackPayload::Instructions(p)
                    if p.granting_damage_source.as_ref().is_some_and(|grant|
                        grant.id() == crate::damage::DamageSourceId::Object(*source))))
                || self.triggers.waiting().values().any(|trigger| trigger.source.object.id() == *source || refers_to_source(&trigger.causes))
                || self.payloads.values().any(|payload| matches!(payload, StackPayload::Instructions(p) if p.bindings.trigger.as_ref().is_some_and(|trigger| refers_to_source(&trigger.causes))))
                || self.window.entries().iter().chain(self.window.resolving()).chain(self.window.checking_targets())
                    .any(|entry| matches!(entry.kind(), StackKind::ActivatedAbility { source: object } | StackKind::TriggeredAbility { source: object } if object.id() == *source))
        });
        if let Some(mut departed) = self.departed_saga_sources.take() {
            use crate::occurrence::{TriggerCauseSet, TriggerRelated};
            let chapter = |causes: &TriggerCauseSet| {
                causes
                    .members()
                    .iter()
                    .any(|cause| matches!(cause.related, TriggerRelated::SagaChapter { .. }))
            };
            departed.retain(|&saga, _| {
                self.triggers
                    .waiting()
                    .values()
                    .any(|waiting| waiting.source.object.id() == saga && chapter(&waiting.causes))
                    || self
                        .window
                        .entries()
                        .iter()
                        .chain(self.window.resolving())
                        .chain(self.window.checking_targets())
                        .filter_map(|entry| {
                            self.execution_bindings(entry.id())
                                .and_then(|bindings| bindings.trigger.as_ref())
                        })
                        .any(|bound| bound.source.object.id() == saga && chapter(&bound.causes))
            });
            self.departed_saga_sources = Some(departed);
        }
        let live_copy_sources = self
            .departed_copy_sources
            .keys()
            .copied()
            .filter(|id| self.needs_departed_copy_source(*id))
            .collect::<BTreeSet<_>>();
        self.departed_copy_sources
            .retain(|id, _| live_copy_sources.contains(id));
        let live_ability_sources = self
            .departed_reflexive_sources
            .keys()
            .copied()
            .filter(|id| self.needs_departed_ability_source(*id))
            .collect::<BTreeSet<_>>();
        self.departed_reflexive_sources
            .retain(|id, _| live_ability_sources.contains(id));
        #[cfg(test)]
        super::characteristic_observations::published(self, &records);
        append.append(records);
        let mut linked = Vec::new();
        for source in departing_link_sources {
            if let Some(objects) = self.characteristics.linked_exiles.remove(&source) {
                linked.extend(objects);
            }
        }
        if !linked.is_empty() {
            if let Some(work) = &mut self.linked_return_work {
                if work.origin != LinkedReturnOrigin::Exile {
                    return Err(EffectError::InconsistentProgram);
                }
                work.objects.extend(linked);
                work.orders.clear();
                work.graveyard_orders.clear();
                work.replacements = None;
            } else {
                self.linked_return_work = Some(LinkedReturnWork {
                    library_bottom: None,
                    origin: LinkedReturnOrigin::Exile,
                    wake_entry: None,
                    objects: linked,
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
            }
            self.finish_linked_return_if_ready(append)?;
        }
        Ok(())
    }
    fn trigger_watches_targeted_by_controller(
        event: &crate::program::TriggerEvent,
        is_spell: bool,
        instant_or_sorcery: bool,
    ) -> bool {
        match event {
            crate::program::TriggerEvent::TargetedByController {
                spell_only,
                instant_or_sorcery_only,
                ..
            } => (!*spell_only || is_spell) && (!*instant_or_sorcery_only || instant_or_sorcery),
            crate::program::TriggerEvent::AnyOf(events) => events.iter().any(|sub| {
                Self::trigger_watches_targeted_by_controller(sub, is_spell, instant_or_sorcery)
            }),
            _ => false,
        }
    }

    fn extra_targeted_by_controller_events(
        &self,
        id: StackEntryId,
        current: &EvaluatedCharacteristics,
        existing: &[RulesOccurrence],
    ) -> Vec<RulesOccurrence> {
        let Some(entry) = self.window.entries().iter().find(|s| s.id() == id) else {
            return Vec::new();
        };
        let is_spell = matches!(
            entry.kind(),
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
        );
        let instant_or_sorcery = self.stack_copiable_card_types(id).is_some_and(|types| {
            crate::program::SpellFilter::instant_or_sorcery().matches_card_types(types)
        });
        let has_watcher = current.objects.values().any(|watcher| {
            watcher.zone == ObjectZone::Battlefield
                && watcher.controller == Some(entry.controller())
                && (watcher.printed_text.triggered.iter().any(|ability| {
                    watcher.abilities.triggered.contains(&ability.id)
                        && Self::trigger_watches_targeted_by_controller(
                            &ability.condition.event,
                            is_spell,
                            instant_or_sorcery,
                        )
                }) || watcher.granted_triggers.values().flatten().any(|ability| {
                    Self::trigger_watches_targeted_by_controller(
                        &ability.condition.event,
                        is_spell,
                        instant_or_sorcery,
                    )
                }))
        });
        if !has_watcher {
            return Vec::new();
        }
        let mut objects = BTreeSet::new();
        for target in entry.targets().targets() {
            let TargetRef::Object(object) = target.choice().target else {
                continue;
            };
            let already_emitted = existing.iter().any(|occ| {
                matches!(
                    occ,
                    RulesOccurrence::PermanentTargeted {
                        object: seen,
                        entry: seen_entry,
                        ..
                    } if *seen == object && *seen_entry == id
                )
            });
            if !already_emitted
                && current
                    .get(object)
                    .is_some_and(|sheet| sheet.zone == ObjectZone::Battlefield)
            {
                objects.insert(object);
            }
        }
        objects
            .into_iter()
            .map(|object| RulesOccurrence::PermanentTargeted {
                object,
                entry: id,
                controller: entry.controller(),
                instant_or_sorcery,
            })
            .collect()
    }

    pub(super) fn face_down_entry_for(
        &self,
        id: ObjectId,
    ) -> Option<crate::face_down::FaceDownKind> {
        self.linked_return_work
            .as_ref()
            .filter(|work| work.objects.contains(&id))?
            .origin
            .face_down_entry()
    }
    pub(super) fn linked_aura_domain(
        &self,
        id: ObjectId,
        owner: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<Vec<TargetRef>> {
        if self.face_down_entry_for(id).is_some() {
            return None;
        }
        let copy = self
            .linked_return_work
            .as_ref()
            .and_then(|work| work.replacements.as_ref())
            .and_then(|batch| batch.copied_definition(id))
            .or_else(|| {
                self.prospective_entry_copies
                    .get(&id)
                    .map(|copy| copy.definition.as_ref())
            });
        self.prospective_aura_domain(id, owner, copy, None, evaluated)
    }

    pub(super) fn prospective_aura_domain(
        &self,
        id: ObjectId,
        owner: SeatId,
        copy: Option<&Program>,
        forced_host: Option<TargetRef>,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<Vec<TargetRef>> {
        let apply_host = |mut candidates: Vec<TargetRef>| {
            if let Some(host) = forced_host {
                candidates.retain(|candidate| *candidate == host);
            }
            candidates
        };
        if let Some(definition) = copy {
            return self
                .token_aura_domain(definition, owner, evaluated)
                .map(apply_host);
        }
        let crate::attachment::AttachmentKind::Aura(enchant) =
            self.prospective_entry_face(id)?.permanent()?.attachment?
        else {
            return None;
        };
        let aura = evaluated.get(id)?;
        if aura.is_creature() {
            return Some(Vec::new());
        }
        Some(apply_host(self.aura_entry_domain(
            enchant,
            owner,
            &super::protection::ProtectionSource::from(aura),
            evaluated,
        )))
    }

    pub(super) fn aura_entry_domain(
        &self,
        enchant: crate::attachment::EnchantRestriction,
        owner: SeatId,
        source: &super::protection::ProtectionSource,
        evaluated: &EvaluatedCharacteristics,
    ) -> Vec<TargetRef> {
        self.window
            .participants()
            .iter()
            .copied()
            .map(TargetRef::Player)
            .chain(
                self.window
                    .cards()
                    .iter()
                    .filter(|card| card.zone() == ObjectZone::Battlefield)
                    .map(|card| TargetRef::Object(card.id())),
            )
            .filter(|target| {
                self.enchant_permits(enchant, *target, owner, evaluated)
                    && !Self::protected_target(*target, source, evaluated)
            })
            .collect()
    }
    pub(crate) fn linked_return_decision(&self) -> Result<Option<EffectDecision>, EffectError> {
        let Some(work) = &self.linked_return_work else {
            return Ok(None);
        };
        if let Some(batch) = &work.replacements
            && let Some(frame) = batch.frame(self.revision)
        {
            return Ok(Some(EffectDecision::ZoneReplacement(frame)));
        }
        let players = self.window.apnap_players()?.collect::<Vec<_>>();
        let evaluated = self.evaluated_characteristics()?;
        for chooser in &players {
            let objects = work
                .objects
                .iter()
                .copied()
                .filter(|id| {
                    work.replacements.as_ref().is_some_and(|batch| {
                        batch.destination(*id).ok()
                            == Some(crate::program::PublicZoneDestination::Battlefield)
                    }) && self.window.cards().get(*id).is_some_and(|card| {
                        work.origin.accepts(card)
                            && work
                                .replacements
                                .as_ref()
                                .and_then(|batch| batch.entry_controller(card.id()))
                                .unwrap_or_else(|| work.origin.entry_controller(card))
                                == *chooser
                    }) && self
                        .linked_aura_domain(*id, *chooser, &evaluated)
                        .is_none_or(|domain| !domain.is_empty())
                })
                .collect::<Vec<_>>();
            if objects.len() > 1 && !work.orders.contains_key(chooser) {
                return Ok(Some(EffectDecision::LinkedReturnOrder(
                    LinkedReturnOrderFrame {
                        revision: self.revision,
                        chooser: *chooser,
                        objects,
                        destination: crate::program::PublicZoneDestination::Battlefield,
                    },
                )));
            }
            let graveyard =
                work.objects
                    .iter()
                    .filter(|_| {
                        matches!(
                            work.origin,
                            LinkedReturnOrigin::Manifest { .. }
                                | LinkedReturnOrigin::ChosenExile { .. }
                                | LinkedReturnOrigin::ExileWithCounters { .. }
                                | LinkedReturnOrigin::Exile
                                | LinkedReturnOrigin::Hand { .. }
                        )
                    })
                    .copied()
                    .filter(|id| {
                        work.replacements.as_ref().is_some_and(|batch| {
                            batch.destination(*id).ok()
                                == Some(crate::program::PublicZoneDestination::Graveyard)
                        }) && self.window.cards().get(*id).is_some_and(|card| {
                            work.origin.accepts(card) && card.owner() == *chooser
                        })
                    })
                    .collect::<Vec<_>>();
            if graveyard.len() > 1 && !work.graveyard_orders.contains_key(chooser) {
                return Ok(Some(EffectDecision::LinkedReturnOrder(
                    LinkedReturnOrderFrame {
                        revision: self.revision,
                        chooser: *chooser,
                        objects: graveyard,
                        destination: crate::program::PublicZoneDestination::Graveyard,
                    },
                )));
            }
        }
        for chooser in players {
            for id in &work.objects {
                let Some(card) = self.window.cards().get(*id) else {
                    continue;
                };
                if !work.origin.accepts(card)
                    || work
                        .replacements
                        .as_ref()
                        .and_then(|batch| batch.entry_controller(card.id()))
                        .unwrap_or_else(|| work.origin.entry_controller(card))
                        != chooser
                {
                    continue;
                }
                if work.replacements.as_ref().is_none_or(|batch| {
                    batch.destination(*id).ok()
                        != Some(crate::program::PublicZoneDestination::Battlefield)
                }) {
                    continue;
                }
                if let Some(domain) = self.linked_aura_domain(*id, chooser, &evaluated) {
                    if domain.is_empty() {
                        continue;
                    }
                    if !work.auras.contains_key(id) {
                        return Ok(Some(EffectDecision::LinkedAuraChoice(
                            LinkedAuraChoiceFrame {
                                revision: self.revision,
                                chooser,
                                object: *id,
                                domain,
                            },
                        )));
                    }
                }
                if work.choices.contains_key(id) || work.origin.face_down_entry().is_some() {
                    continue;
                }
                let Some(definition) = work
                    .replacements
                    .as_ref()
                    .and_then(|batch| batch.copied_definition(*id))
                    .or_else(|| self.copiable_program(*id))
                else {
                    continue;
                };
                if let Some(frame) =
                    self.entry_choice_for_definition(chooser, *id, definition, false)?
                {
                    return Ok(Some(EffectDecision::EntryChoice(frame)));
                }
            }
        }
        Ok(None)
    }
    pub(super) fn begin_graveyard_returns(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        player: SeatId,
        objects: Vec<ObjectId>,
        tapped: bool,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.linked_return_work.is_some() {
            return Err(EffectError::InconsistentProgram);
        }
        self.linked_return_work = Some(LinkedReturnWork {
            origin: LinkedReturnOrigin::Graveyard {
                player: Some(player),
                controller: None,
                subtype_counters: None,
                entry_additions: crate::program::EntryAdditions::default(),
                entry: entry.id(),
                instruction,
                tapped,
                forest: false,
                corpse: matches!(
                    self.payloads
                        .get(&entry.id())
                        .and_then(StackPayload::instructions)
                        .and_then(|ops| ops.get(instruction)),
                    Some(Instruction::ZoneOp(spec))
                        if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Delayed(
                                crate::toolbox::DelayedZoneAction::ReturnGraveyardCreatureWithCorpse(
                                    _
                                )
                            )
                        )
                ),
            },
            library_bottom: None,
            wake_entry: None,
            objects,
            orders: BTreeMap::new(),
            graveyard_orders: BTreeMap::new(),
            choices: BTreeMap::new(),
            auras: BTreeMap::new(),
            replacements: None,
        });
        self.finish_linked_return_if_ready(append)
    }

    pub(super) fn begin_linked_cohort_return(
        &mut self,
        origin: LinkedReturnOrigin,
        mut objects: Vec<ObjectId>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.linked_return_work.is_some() {
            return Err(EffectError::InconsistentProgram);
        }
        objects.retain(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|card| origin.accepts(card))
        });
        if objects.is_empty() {
            return Ok(());
        }
        let auras = BTreeMap::new();
        self.linked_return_work = Some(LinkedReturnWork {
            origin,
            objects,
            library_bottom: None,
            wake_entry: None,
            orders: BTreeMap::new(),
            graveyard_orders: BTreeMap::new(),
            choices: BTreeMap::new(),
            auras,
            replacements: None,
        });
        self.finish_linked_return_if_ready(append)
    }

    pub(super) fn finish_linked_return_if_ready(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.linked_return_work.is_none() {
            return Ok(());
        }
        if self
            .linked_return_work
            .as_ref()
            .is_some_and(|work| work.replacements.is_none())
        {
            let work = self
                .linked_return_work
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?;
            let objects = work.objects.clone();
            let origin = work.origin;
            let controllers = objects
                .iter()
                .filter_map(|id| {
                    self.window
                        .cards()
                        .get(*id)
                        .filter(|card| work.origin.accepts(card))
                        .map(|card| (*id, work.origin.entry_controller(card)))
                })
                .collect::<BTreeMap<_, _>>();
            let intents = controllers
                .keys()
                .map(|id| {
                    let before = self
                        .window
                        .cards()
                        .get(*id)
                        .ok_or(EffectError::InconsistentProgram)?;
                    Ok(zone_replacement::ZoneMoveIntent {
                        before: *id,
                        reason: origin.reason(before.zone()),
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()?;
            let batch = self.begin_zone_replacements_with_entry(
                zone_replacement::AuthorizedZoneBatch {
                    cause: origin.cause(),
                    action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                    intents,
                },
                Some(&controllers),
            )?;
            self.linked_return_work
                .as_mut()
                .ok_or(EffectError::InconsistentProgram)?
                .replacements = Some(batch);
        }
        if let Some(work) = &mut self.linked_return_work
            && let Some(batch) = &mut work.replacements
            && batch.progress(self.revision)?.is_some()
        {
            return Ok(());
        }
        if self.linked_return_decision()?.is_some() {
            return Ok(());
        }
        let held = self
            .linked_return_work
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        let origin = held.origin;
        let controllers = held
            .objects
            .iter()
            .filter_map(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .filter(|card| origin.accepts(card))
                    .map(|card| (*id, origin.entry_controller(card)))
            })
            .collect::<BTreeMap<_, _>>();
        let intents = controllers
            .keys()
            .map(|id| {
                let before = self
                    .window
                    .cards()
                    .get(*id)
                    .ok_or(EffectError::InconsistentProgram)?;
                Ok(zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason: origin.reason(before.zone()),
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let fresh = self.begin_zone_replacements_with_entry(
            zone_replacement::AuthorizedZoneBatch {
                cause: origin.cause(),
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                intents,
            },
            Some(&controllers),
        )?;
        held.replacements
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?
            .verify_initial(fresh)?;
        let work = self
            .linked_return_work
            .take()
            .ok_or(EffectError::InconsistentProgram)?;
        let evaluated = self.evaluated_characteristics()?;
        let destinations = work
            .objects
            .iter()
            .filter_map(|id| {
                work.replacements
                    .as_ref()
                    .and_then(|batch| batch.destination(*id).ok())
                    .map(|destination| (*id, destination))
            })
            .collect::<BTreeMap<_, _>>();
        let mut ordered = Vec::new();
        for player in self.window.apnap_players()? {
            if let Some(order) = work.orders.get(&player) {
                ordered.extend(order.iter().copied());
            } else {
                ordered.extend(work.objects.iter().copied().filter(|id| {
                    destinations.get(id)
                        == Some(&crate::program::PublicZoneDestination::Battlefield)
                        && self.window.cards().get(*id).is_some_and(|card| {
                            work.origin.accepts(card)
                                && work
                                    .replacements
                                    .as_ref()
                                    .and_then(|batch| batch.entry_controller(card.id()))
                                    .unwrap_or_else(|| work.origin.entry_controller(card))
                                    == player
                        })
                        && self
                            .linked_aura_domain(*id, player, &evaluated)
                            .is_none_or(|domain| !domain.is_empty())
                }));
            }
            if let Some(order) = work.graveyard_orders.get(&player) {
                ordered.extend(order.iter().rev().copied());
            } else {
                ordered.extend(work.objects.iter().copied().filter(|id| {
                    destinations.get(id) == Some(&crate::program::PublicZoneDestination::Graveyard)
                        && self
                            .window
                            .cards()
                            .get(*id)
                            .is_some_and(|card| work.origin.accepts(card) && card.owner() == player)
                }));
            }
            ordered.extend(work.objects.iter().copied().filter(|id| {
                destinations.get(id).is_some_and(|destination| {
                    !matches!(
                        destination,
                        crate::program::PublicZoneDestination::Battlefield
                            | crate::program::PublicZoneDestination::Graveyard
                    )
                }) && self
                    .window
                    .cards()
                    .get(*id)
                    .is_some_and(|card| work.origin.accepts(card) && card.owner() == player)
            }));
        }
        work.replacements
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?
            .commit_devour(self, append)?;
        self.prospective_entry_copies.extend(
            work.replacements
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?
                .entry_copies()
                .filter(|(id, _)| ordered.contains(id)),
        );
        self.prospective_entry_controllers.extend(
            work.replacements
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?
                .entry_controllers()
                .filter(|(id, _)| ordered.contains(id)),
        );
        self.resolution_entry_choices.extend(work.choices);
        self.return_linked_exiles(
            &ordered,
            work.auras,
            &destinations,
            work.origin,
            work.wake_entry,
            append,
        )?;
        Ok(())
    }
    fn return_linked_exiles(
        &mut self,
        objects: &[ObjectId],
        auras: BTreeMap<ObjectId, TargetRef>,
        destinations: &BTreeMap<ObjectId, crate::program::PublicZoneDestination>,
        origin: LinkedReturnOrigin,
        wake_entry: Option<StackEntryId>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let mut moves = objects
            .iter()
            .filter_map(|id| {
                let card = self.window.cards().get(*id)?;
                if !origin.accepts(card) {
                    return None;
                }
                let destination = destinations
                    .get(id)?
                    .physical(Some(origin.entry_controller(card)))?;
                (!matches!(
                    (origin, destination),
                    (LinkedReturnOrigin::Hand { .. }, ObjectDestination::Hand)
                        | (
                            LinkedReturnOrigin::Exile
                                | LinkedReturnOrigin::ExileWithCounters { .. }
                                | LinkedReturnOrigin::ChosenExile { .. },
                            ObjectDestination::Exile
                        )
                        | (
                            LinkedReturnOrigin::Graveyard { .. },
                            ObjectDestination::Graveyard
                        )
                ))
                .then_some((*id, destination))
            })
            .collect::<Vec<_>>();
        let apnap = self.window.apnap_players()?.collect::<Vec<_>>();
        moves.sort_by_key(|(_, destination)| match destination {
            ObjectDestination::Battlefield { controller } => apnap
                .iter()
                .position(|player| player == controller)
                .unwrap_or(apnap.len()),
            _ => apnap.len(),
        });
        if moves.is_empty() {
            if let Some(entry) = wake_entry {
                let controller = self
                    .window
                    .resolving()
                    .filter(|actual| actual.id() == entry)
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                self.delay_spell_sacrifice(entry, controller, vec![])?;
            }
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let mut face_up_origins = BTreeMap::new();
        if let Some(kind) = origin.face_down_entry() {
            for (id, destination) in &moves {
                if matches!(destination, ObjectDestination::Battlefield { .. }) {
                    let card = self
                        .window
                        .cards()
                        .get(*id)
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone();
                    face_up_origins.insert(*id, card);
                    self.window.set_orientation(*id, Some(kind))?;
                }
            }
        }
        let (window, mut movements) = self.window.record_linked_return_moves(&moves)?;
        for movement in &mut movements {
            if let Some(original) = face_up_origins.remove(&movement.before.id()) {
                movement.before = original;
            }
        }
        self.window = window;
        let ids = movements
            .iter()
            .filter(|m| m.after.zone() == ObjectZone::Battlefield)
            .map(|m| m.after.id())
            .collect::<Vec<_>>();
        self.characteristics.register_battlefield_entries(&ids)?;
        if matches!(origin, LinkedReturnOrigin::Manifest { .. })
            && !ids.is_empty()
            && let Some(cursor) = self.resolution_cursor_mut()
        {
            cursor
                .results
                .last_moved_objects
                .extend(ids.iter().copied());
            cursor.results.last_action_succeeded = true;
            cursor.results.last_action_count = Some(RulesInteger::from_usize(
                cursor.results.last_moved_objects.len(),
            ));
        }
        if let Some(entry) = wake_entry {
            let controller = self
                .window
                .resolving()
                .filter(|actual| actual.id() == entry)
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            self.delay_spell_sacrifice(entry, controller, ids.clone())?;
        }
        let mut counters = Vec::new();
        let mut entry_effects = Vec::new();
        for movement in &mut movements {
            if movement.after.zone() != ObjectZone::Battlefield {
                continue;
            }
            if matches!(movement.before.zone(), ObjectZone::Graveyard(_)) {
                self.characteristics
                    .entered_from_graveyard
                    .insert(movement.after.id());
            }
            if let LinkedReturnOrigin::Graveyard {
                entry,
                entry_additions,
                ..
            } = origin
            {
                use crate::characteristics::{
                    ColorOperation, ResolvedDuration, ResolvedPart, ResolvedParts,
                    SubtypeOperation, TypeChange, TypeOperation,
                };
                let mut parts = Vec::new();
                if let Some(subtype) = entry_additions.creature_subtype {
                    parts.push(ResolvedPart::Type(TypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        types: TypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Add([subtype].into()),
                    }));
                }
                if let Some(color) = entry_additions.color {
                    parts.push(ResolvedPart::Color(ColorOperation::Add([color].into())));
                }
                if !parts.is_empty() {
                    entry_effects.push(self.characteristics.add_effect(
                        entry,
                        movement.after.id(),
                        ResolvedParts(parts),
                        ResolvedDuration::NoStatedDuration,
                    )?);
                }
            }
            self.stage_resolution_entry_choice(movement.before.id(), movement.after.id())?;
            if matches!(
                origin,
                LinkedReturnOrigin::Hand { tapped: true, .. }
                    | LinkedReturnOrigin::Graveyard { tapped: true, .. }
                    | LinkedReturnOrigin::ChosenExile { tapped: true, .. }
            ) {
                self.window.enter_tapped(movement.after.id())?;
            }
            if let Some(host) = auras.get(&movement.before.id()) {
                self.characteristics
                    .attach(movement.after.id(), *host, true)?;
            }
            if matches!(origin, LinkedReturnOrigin::Graveyard { corpse: true, .. }) {
                let object = movement.after.id();
                let controller = self
                    .current_characteristics(object)?
                    .and_then(|o| o.controller)
                    .ok_or(EffectError::InconsistentProgram)?;
                let amount = super::permanent_counters::controlled_counter_multiplier(
                    &before.view().evaluated,
                    controller,
                );
                self.characteristics.corpse_counters.insert(object, amount);
                self.window.mark_corpse_return(object)?;
            }
            let added = if matches!(origin, LinkedReturnOrigin::ExileWithCounters { .. }) {
                self.apply_return_entry_counters(&before, movement.after.id())?
            } else {
                self.apply_entry_counters(
                    &before,
                    movement.after.id(),
                    PowerToughnessCounters::default(),
                    0,
                )?
            };
            movement.after = self
                .window
                .cards()
                .get(movement.after.id())
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            if added.valid_placement() {
                counters.push((movement.before.id(), movement.after.id(), added));
            }
        }
        self.emit_from(
            origin.cause(),
            &before,
            None,
            append,
            |old, current, controllers| {
                let mut values = movements
                    .iter()
                    .map(|movement| {
                        zone_occurrence_at(
                            old,
                            current,
                            movement,
                            origin.reason(movement.before.zone()),
                            controllers,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if matches!(origin, LinkedReturnOrigin::Graveyard { corpse: true, .. }) {
                    for movement in movements
                        .iter()
                        .filter(|m| m.after.zone() == ObjectZone::Battlefield)
                    {
                        let id = movement.after.id();
                        let after = current
                            .characteristics
                            .permanent_counter_count(id, CounterKind::Corpse);
                        values.push(RulesOccurrence::PermanentCounterChanged {
                            object: id,
                            kind: CounterKind::Corpse,
                            before: 0.into(),
                            after,
                        });
                    }
                }
                for effect in entry_effects {
                    // The zone change already records the entrant with its new characteristics.
                    values.push(RulesOccurrence::ContinuousEffectCreated {
                        effect,
                        appearances: vec![],
                    });
                }
                for (old_id, id, added) in counters {
                    values.push(RulesOccurrence::CountersPlaced {
                        object: id,
                        before: old.characteristics.counters(id),
                        after: current.characteristics.counters(id),
                        added,
                        before_appearance: Box::new(
                            old.appearance(old_id)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(id)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(values)
            },
        )?;
        if let LinkedReturnOrigin::Graveyard {
            subtype_counters: Some((subtype, amount)),
            ..
        } = origin
        {
            let current = self.evaluated_characteristics()?;
            let recipients = ids
                .into_iter()
                .filter(|id| {
                    current.get(*id).is_some_and(|permanent| {
                        permanent.zone == ObjectZone::Battlefield
                            && permanent.current_creature_subtypes.contains(&subtype)
                    })
                })
                .collect();
            self.place_plus_one_counter_amount(
                recipients,
                i64::from(amount).into(),
                origin.cause(),
                append,
            )?;
        }
        Ok(())
    }
}
fn appearance_to_zone_move_record(
    change: &ZoneOccurrence,
    appearance: &SupportedAppearance,
) -> crate::characteristics::ZoneMoveTurnRecord {
    let mut types = crate::program::CardTypeMask::EMPTY;
    for &kind in &appearance.types {
        types = types.with(kind);
    }
    let mut colors = crate::program::ColorMask::EMPTY;
    for &color in &appearance.colors {
        colors = colors.with(color);
    }
    let mut supertypes = crate::program::SupertypeMask::EMPTY;
    for &sup in &appearance.current_supertypes {
        supertypes = supertypes.union(crate::program::SupertypeMask::from_supertype(sup));
    }
    crate::characteristics::ZoneMoveTurnRecord {
        owner: appearance.object.owner(),
        previous_controller: change.before.object.controller(),
        from_battlefield: change.before.object.zone() == ObjectZone::Battlefield,
        types,
        colors,
        supertypes,
        subtypes: appearance.creature_subtypes.clone(),
        artifact_subtypes: appearance.artifact_subtypes.clone(),
    }
}

/// Scratch computation shared by trusted initial setup and actual control cuts.
pub(super) fn desired_controllers(
    cards: &GameObjects,
    evaluated: &EvaluatedCharacteristics,
) -> Result<BTreeMap<ObjectId, SeatId>, EffectError> {
    let mut desired = BTreeMap::new();
    for card in cards
        .iter()
        .filter(|card| card.zone() == ObjectZone::Battlefield)
    {
        let controller = evaluated
            .get(card.id())
            .and_then(|current| current.controller)
            .ok_or(EffectError::InconsistentProgram)?;
        if card.controller() != Some(controller) {
            desired.insert(card.id(), controller);
        }
    }
    Ok(desired)
}
/// These are real predecessor/successor registries, including draw-batch working cuts.
pub(super) fn zone_occurrence_at(
    before: &RulesView<'_>,
    after: &RulesView<'_>,
    movement: &ObjectMovement,
    reason: ZoneReason,
    controllers: &[ControllerChange],
) -> Result<RulesOccurrence, EffectError> {
    let previous = before
        .appearance(movement.before.id())
        .ok_or(EffectError::InconsistentProgram)?;
    let current = after
        .appearance(movement.after.id())
        .ok_or(EffectError::InconsistentProgram)?;
    let after_matches = current.object == movement.after
        || controllers
            .iter()
            .any(|change| change.before == movement.after && change.after == current.object);
    if previous.object != movement.before || !after_matches {
        return Err(EffectError::InconsistentProgram);
    }
    let additional_arrivals = movement
        .arrivals()
        .skip(1)
        .map(|id| after.appearance(id).ok_or(EffectError::InconsistentProgram))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RulesOccurrence::ZoneChange(Box::new(ZoneOccurrence {
        additional_arrivals,
        madness: false,
        before: previous,
        after: current,
        reason,
    })))
}
pub(super) fn madness_replaced(mut value: RulesOccurrence, applied: bool) -> RulesOccurrence {
    if let RulesOccurrence::ZoneChange(change) = &mut value {
        change.madness = applied && change.after.object.zone() == ObjectZone::Exile;
    }
    value
}
pub(super) fn exit_occurrence_at(
    before: &RulesView<'_>,
    after: &RulesView<'_>,
    exit: crate::stack_window::StackExit,
    reason: ZoneReason,
    controllers: &[ControllerChange],
) -> Result<RulesOccurrence, EffectError> {
    match exit {
        crate::stack_window::StackExit::Object(movement) => {
            zone_occurrence_at(before, after, &movement, reason, controllers)
        }
        crate::stack_window::StackExit::Copy { before, after } => {
            Ok(RulesOccurrence::CopyMoved { before, after })
        }
    }
}
pub(super) fn characteristic_appearance_changes(
    before: &RulesView<'_>,
    after: &RulesView<'_>,
    objects: impl IntoIterator<Item = ObjectId>,
) -> Result<Vec<CharacteristicAppearanceChange>, EffectError> {
    objects
        .into_iter()
        .map(|object| {
            Ok(CharacteristicAppearanceChange {
                object,
                before: before
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?,
                after: after
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?,
            })
        })
        .collect()
}
pub(super) fn status_occurrences(
    changes: Vec<ObjectStatusChange>,
    reason: crate::occurrence::StatusReason,
) -> Vec<RulesOccurrence> {
    changes
        .into_iter()
        .map(|change| RulesOccurrence::StatusChange { change, reason })
        .collect()
}

#[cfg(test)]
#[path = "trigger_grouping_tests.rs"]
mod trigger_grouping_tests;

#[cfg(test)]
#[path = "cast_trigger_tests.rs"]
mod cast_trigger_tests;
