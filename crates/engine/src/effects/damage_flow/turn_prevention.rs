//! Unlimited prevention effects compete with other replacement effects for each recipient.
use super::*;
use crate::combat::BlockStatus;
use crate::damage::{
    ActiveTurnPrevention, ActiveTurnPreventionKind, DamageAttemptId, DamageSource,
    DamageSourceChoiceFilter, DamageSourceId, PreventionSourceFilter, ResolvedPreventionRecipient,
    ResolvedPreventionSource, TurnPreventionFilter,
};
use crate::program::{CardType, ControllerFilter};

impl EffectWindow {
    /// Legal public damage sources choosable under CR 609.7a.
    ///
    /// Includes battlefield permanents, spells and spell copies on the stack, public sources
    /// referenced by abilities currently on the stack (including departed battlefield sources),
    /// face-up command-zone objects, and command-zone emblems. Never exposes hidden cards in
    /// hands or libraries, and never peeks behind face-down characteristics.
    pub(crate) fn legal_damage_sources(
        &self,
        filter: DamageSourceChoiceFilter,
    ) -> Result<Vec<DamageSourceId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let mut sources = BTreeSet::new();

        let matches_object = |id: ObjectId| -> bool {
            if let Some(current) = evaluated.get(id) {
                match filter {
                    DamageSourceChoiceFilter::Any => true,
                    DamageSourceChoiceFilter::Creature => current.is_creature(),
                    DamageSourceChoiceFilter::Colors(mask) => {
                        !mask.is_empty() && mask.matches_any(&current.current_colors)
                    }
                }
            } else if let Some(departed) = self.departed_damage_sources.get(&id) {
                match filter {
                    DamageSourceChoiceFilter::Any => true,
                    DamageSourceChoiceFilter::Creature => departed.was_creature,
                    DamageSourceChoiceFilter::Colors(mask) => {
                        !mask.is_empty() && mask.matches_any(&departed.colors)
                    }
                }
            } else {
                false
            }
        };

        // 1. Battlefield permanents and face-up command-zone cards.
        for card in self.window.cards().iter() {
            let public_zone = match card.zone() {
                ObjectZone::Battlefield => true,
                ObjectZone::Command => card.face_down().is_none(),
                ObjectZone::Hand(_)
                | ObjectZone::Library(_)
                | ObjectZone::Graveyard(_)
                | ObjectZone::Exile
                | ObjectZone::Stack => false,
            };
            if public_zone && matches_object(card.id()) {
                sources.insert(DamageSourceId::Object(card.id()));
            }
        }

        // 2. Spells, spell copies, and public sources of abilities currently on the stack.
        for entry in self.window.entries() {
            match entry.kind() {
                StackKind::CardSpell(id) => {
                    if matches_object(*id) {
                        sources.insert(DamageSourceId::Object(*id));
                    }
                }
                StackKind::SpellCopy { .. } => {
                    let ok = match filter {
                        DamageSourceChoiceFilter::Any => true,
                        DamageSourceChoiceFilter::Creature => self
                            .stack_copiable_card_types(entry.id())
                            .is_some_and(|types| types.contains(&CardType::Creature)),
                        DamageSourceChoiceFilter::Colors(mask) => {
                            !mask.is_empty()
                                && self
                                    .stack_copy_colors(entry.id())
                                    .is_some_and(|colors| mask.matches_any(colors))
                        }
                    };
                    if ok {
                        sources.insert(DamageSourceId::SpellCopy(entry.id()));
                    }
                }
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    let hidden =
                        matches!(source.zone(), ObjectZone::Library(_) | ObjectZone::Hand(_))
                            || (source.zone() != ObjectZone::Battlefield
                                && source.face_down().is_some());
                    if !hidden && matches_object(source.id()) {
                        sources.insert(DamageSourceId::Object(source.id()));
                    }
                }
                StackKind::Emblem { emblem, .. } => {
                    if filter == DamageSourceChoiceFilter::Any {
                        sources.insert(DamageSourceId::Emblem(*emblem));
                    }
                }
                StackKind::Speed | StackKind::Monarch | StackKind::DelayedCopy { .. } => {}
            }
        }

        // 3. Face-up emblems in the command zone.
        if filter == DamageSourceChoiceFilter::Any {
            for &emblem in self.characteristics.emblems.keys() {
                sources.insert(DamageSourceId::Emblem(emblem));
            }
        }

        Ok(sources.into_iter().collect())
    }

    fn source_matches_color(
        &self,
        evaluated: &EvaluatedCharacteristics,
        source: &DamageSource,
        color: crate::characteristics::Color,
    ) -> bool {
        match source {
            DamageSource::Object { object, .. } => {
                if let Some(current) = evaluated.get(object.id()) {
                    current.current_colors.contains(&color)
                } else if let Some(departed) = self.departed_damage_sources.get(&object.id()) {
                    departed.colors.contains(&color)
                } else if let Some(program) = self
                    .copiable_program(object.id())
                    .or_else(|| object.card_id().and_then(|cid| self.programs.get(&cid)))
                {
                    program.defined_colors().contains(&color)
                } else {
                    false
                }
            }
            DamageSource::SpellCopy { entry, .. } => self
                .stack_copy_colors(*entry)
                .is_some_and(|colors| colors.contains(&color)),
            DamageSource::Emblem { .. } => false,
        }
    }

    fn source_matches_filter(
        &self,
        evaluated: &EvaluatedCharacteristics,
        source: &DamageSource,
        prevention_controller: SeatId,
        filter: PreventionSourceFilter,
    ) -> bool {
        let (is_creature, colors, keywords, source_controller, object_id) = match source {
            DamageSource::Object { object, controller } => {
                if let Some(current) = evaluated.get(object.id()) {
                    (
                        current.is_creature(),
                        current.current_colors.clone(),
                        current.keywords.clone(),
                        current.controller.or(Some(*controller)),
                        Some(object.id()),
                    )
                } else if let Some(departed) = self.departed_damage_sources.get(&object.id()) {
                    (
                        departed.was_creature,
                        departed.colors.clone(),
                        departed.keywords.clone(),
                        departed.object.controller().or(Some(*controller)),
                        Some(object.id()),
                    )
                } else if let Some(program) = self
                    .copiable_program(object.id())
                    .or_else(|| object.card_id().and_then(|cid| self.programs.get(&cid)))
                {
                    (
                        program.has_type(CardType::Creature),
                        program.defined_colors(),
                        program.printed_keywords.clone(),
                        Some(*controller),
                        Some(object.id()),
                    )
                } else {
                    (
                        false,
                        BTreeSet::new(),
                        BTreeSet::new(),
                        Some(*controller),
                        Some(object.id()),
                    )
                }
            }
            DamageSource::SpellCopy { entry, controller } => (
                self.stack_copiable_card_types(*entry)
                    .is_some_and(|types| types.contains(&CardType::Creature)),
                self.stack_copy_colors(*entry).cloned().unwrap_or_default(),
                self.stack_copiable_keywords(*entry)
                    .cloned()
                    .unwrap_or_default(),
                Some(*controller),
                None,
            ),
            DamageSource::Emblem { controller, .. } => (
                false,
                BTreeSet::new(),
                BTreeSet::new(),
                Some(*controller),
                None,
            ),
        };

        if filter.creatures_only && !is_creature {
            return false;
        }
        if filter.colorless_only && !colors.is_empty() {
            return false;
        }
        if !filter.any_colors.is_empty() && !filter.any_colors.matches_any(&colors) {
            return false;
        }
        if !filter.forbidden_colors.is_empty() && !filter.forbidden_colors.matches_none(&colors) {
            return false;
        }
        if let Some(kw) = filter.required_keyword
            && !keywords.contains(&kw)
        {
            return false;
        }
        if let Some(kw) = filter.forbidden_keyword
            && keywords.contains(&kw)
        {
            return false;
        }
        match filter.controller {
            ControllerFilter::Any => {}
            ControllerFilter::You => {
                if source_controller != Some(prevention_controller) {
                    return false;
                }
            }
            ControllerFilter::Opponent | ControllerFilter::NotYou => {
                if !source_controller.is_some_and(|p| {
                    p != prevention_controller && self.window.participants().contains(&p)
                }) {
                    return false;
                }
            }
            _ => return false,
        }
        if filter.attacking_only || filter.blocking_only || filter.unblocked_only {
            let Some(id) = object_id else {
                return false;
            };
            let Some(combat) = self.combat.as_ref() else {
                return false;
            };
            if filter.attacking_only && combat.attackers().all(|a| a.combatant().object() != id) {
                return false;
            }
            if filter.blocking_only && combat.blockers().all(|b| b.combatant().object() != id) {
                return false;
            }
            if filter.unblocked_only
                && !combat.attackers().any(|a| {
                    a.combatant().object() == id && a.block_status() == BlockStatus::Unblocked
                })
            {
                return false;
            }
        }
        true
    }

    fn recipient_matches_spec(
        &self,
        evaluated: &EvaluatedCharacteristics,
        recipient: DamageRecipient,
        controller: SeatId,
        spec: &ResolvedPreventionRecipient,
    ) -> bool {
        match spec {
            ResolvedPreventionRecipient::Any => true,
            ResolvedPreventionRecipient::Controller => {
                recipient == DamageRecipient::Player(controller)
            }
            ResolvedPreventionRecipient::ControllerAndControlledCreatures => match recipient {
                DamageRecipient::Player(p) => p == controller,
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => evaluated.get(id).is_some_and(|p| {
                    p.zone == ObjectZone::Battlefield
                        && p.is_creature()
                        && p.controller == Some(controller)
                }),
            },
            ResolvedPreventionRecipient::ControllerAndControlledPermanents => match recipient {
                DamageRecipient::Player(p) => p == controller,
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => evaluated.get(id).is_some_and(|p| {
                    p.zone == ObjectZone::Battlefield && p.controller == Some(controller)
                }),
            },
            ResolvedPreventionRecipient::ControlledCreatures => match recipient {
                DamageRecipient::Player(_) => false,
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => evaluated.get(id).is_some_and(|p| {
                    p.zone == ObjectZone::Battlefield
                        && p.is_creature()
                        && p.controller == Some(controller)
                }),
            },
            ResolvedPreventionRecipient::AllPlayers => {
                matches!(recipient, DamageRecipient::Player(_))
            }
            ResolvedPreventionRecipient::AllCreatures => match recipient {
                DamageRecipient::Player(_) => false,
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => evaluated
                    .get(id)
                    .is_some_and(|p| p.zone == ObjectZone::Battlefield && p.is_creature()),
            },
            ResolvedPreventionRecipient::ExactTarget(target) => match (*target, recipient) {
                (TargetRef::Player(expected), DamageRecipient::Player(actual)) => {
                    expected == actual
                }
                (
                    TargetRef::Object(expected),
                    DamageRecipient::Creature(actual)
                    | DamageRecipient::Planeswalker(actual)
                    | DamageRecipient::Battle(actual),
                ) => expected == actual,
                _ => false,
            },
        }
    }

    pub(super) fn turn_prevention_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> BTreeMap<DamageApplication, Vec<DamageAttemptId>> {
        let mut applications = BTreeMap::<_, Vec<_>>::new();
        for (&(creator, instruction), active) in &self.damage_state.turn_preventions {
            let ActiveTurnPrevention { controller, kind } = active;
            let controller = *controller;
            for component in batch.components().filter(|c| c.can_prevent_damage()) {
                let attempt = component.attempt();
                let eligible = match kind {
                    ActiveTurnPreventionKind::Legacy {
                        filter,
                        protected_source,
                    } => {
                        let filter = *filter;
                        let protected_source = *protected_source;
                        match filter {
                            TurnPreventionFilter::ToAbilitySource
                            | TurnPreventionFilter::ToTarget => {
                                matches!(
                                    attempt.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if protected_source == Some(id)
                                )
                            }
                            TurnPreventionFilter::FromAbilitySource
                            | TurnPreventionFilter::FromTarget => {
                                matches!(attempt.source.id(), DamageSourceId::Object(id) if protected_source == Some(id))
                            }
                            TurnPreventionFilter::CombatFromAbilitySource
                            | TurnPreventionFilter::CombatFromTarget => {
                                batch.kind() == DamageKind::Combat
                                    && matches!(attempt.source.id(), DamageSourceId::Object(id) if protected_source == Some(id))
                            }
                            TurnPreventionFilter::Creatures
                            | TurnPreventionFilter::CombatFromCreatures => {
                                (filter == TurnPreventionFilter::Creatures
                                    || batch.kind() == DamageKind::Combat)
                                    && matches!(attempt.source.id(), DamageSourceId::Object(id)
                                    if evaluated.get(id).map_or_else(
                                        || self.departed_damage_sources.get(&id).is_some_and(|source| source.was_creature),
                                        |source| source.zone == ObjectZone::Battlefield && source.is_creature()))
                            }
                            TurnPreventionFilter::CombatToOrFromAbilitySource
                            | TurnPreventionFilter::CombatToOrFromTarget => {
                                batch.kind() == DamageKind::Combat
                                    && (matches!(attempt.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id)
                                    if protected_source == Some(id))
                                        || matches!(attempt.source.id(), DamageSourceId::Object(id) if protected_source == Some(id)))
                            }
                            TurnPreventionFilter::CombatFromCreaturesOtherThanTarget => {
                                batch.kind() == DamageKind::Combat
                                    && matches!(attempt.source.id(), DamageSourceId::Object(id) if Some(id) != protected_source
                                    && evaluated.get(id).map_or_else(|| self.departed_damage_sources.get(&id).is_some_and(|source| source.was_creature),
                                        |source| source.zone == ObjectZone::Battlefield && source.is_creature()))
                            }
                            TurnPreventionFilter::CombatToControlledCreatures => {
                                batch.kind() == DamageKind::Combat
                                    && match attempt.target {
                                        DamageRecipient::Creature(id)
                                        | DamageRecipient::Planeswalker(id)
                                        | DamageRecipient::Battle(id) => {
                                            evaluated.get(id).is_some_and(|p| {
                                                p.zone == ObjectZone::Battlefield
                                                    && p.is_creature()
                                                    && p.controller == Some(controller)
                                            })
                                        }
                                        DamageRecipient::Player(_) => false,
                                    }
                            }
                            TurnPreventionFilter::Combat => batch.kind() == DamageKind::Combat,
                            TurnPreventionFilter::All => true,
                            TurnPreventionFilter::CombatFromCreaturesWithoutSubtype(first)
                            | TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(
                                first,
                                _,
                            ) => {
                                let excluded = |types: &BTreeSet<_>| {
                                    types.contains(&first)
                                        || matches!(filter, TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(_, second)
                                            if types.contains(&second))
                                };
                                batch.kind() == DamageKind::Combat
                                    && match attempt.source.id() {
                                        DamageSourceId::Object(id) => {
                                            evaluated.get(id).map_or_else(
                                                || {
                                                    self.departed_damage_sources
                                                        .get(&id)
                                                        .is_some_and(|source| {
                                                            source.was_creature
                                                                && !excluded(
                                                                    &source.creature_subtypes,
                                                                )
                                                        })
                                                },
                                                |current| {
                                                    current.zone == ObjectZone::Battlefield
                                                        && current.is_creature()
                                                        && !excluded(
                                                            &current.current_creature_subtypes,
                                                        )
                                                },
                                            )
                                        }
                                        _ => false,
                                    }
                            }
                            TurnPreventionFilter::CreaturesControlledByOpponents => {
                                let DamageSource::Object { object, .. } = &attempt.source else {
                                    continue;
                                };
                                let source_controller = match evaluated.get(object.id()) {
                                    Some(current)
                                        if current.zone == ObjectZone::Battlefield
                                            && current.is_creature() =>
                                    {
                                        current.controller
                                    }
                                    Some(_) => None,
                                    None => self
                                        .departed_damage_sources
                                        .get(&object.id())
                                        .filter(|source| source.was_creature)
                                        .and_then(|source| source.object.controller()),
                                };
                                source_controller.is_some_and(|player| {
                                    player != controller
                                        && self.window.participants().contains(&player)
                                })
                            }
                        }
                    }
                    ActiveTurnPreventionKind::Spec {
                        combat_only,
                        source,
                        recipient,
                        both_directions,
                    } => {
                        if *combat_only && batch.kind() != DamageKind::Combat {
                            false
                        } else if *both_directions {
                            match source {
                                ResolvedPreventionSource::ExactSources(sources) => {
                                    sources.contains(&attempt.source.id())
                                        || matches!(
                                            attempt.target,
                                            DamageRecipient::Creature(id)
                                                | DamageRecipient::Planeswalker(id)
                                                | DamageRecipient::Battle(id)
                                                if sources.contains(&DamageSourceId::Object(id))
                                        )
                                }
                                _ => false,
                            }
                        } else {
                            let source_ok = match source {
                                ResolvedPreventionSource::Any => true,
                                ResolvedPreventionSource::ExactSources(sources) => {
                                    sources.contains(&attempt.source.id())
                                }
                                ResolvedPreventionSource::Color(color) => {
                                    self.source_matches_color(evaluated, &attempt.source, *color)
                                }
                                ResolvedPreventionSource::Filter(filter) => self
                                    .source_matches_filter(
                                        evaluated,
                                        &attempt.source,
                                        controller,
                                        *filter,
                                    ),
                                ResolvedPreventionSource::CreaturesOtherThan(excluded) => {
                                    matches!(
                                        attempt.source.id(),
                                        DamageSourceId::Object(id) if id != *excluded
                                            && evaluated.get(id).map_or_else(
                                                || self
                                                    .departed_damage_sources
                                                    .get(&id)
                                                    .is_some_and(|s| s.was_creature),
                                                |s| s.zone == ObjectZone::Battlefield
                                                    && s.is_creature(),
                                            )
                                    )
                                }
                            };
                            source_ok
                                && self.recipient_matches_spec(
                                    evaluated,
                                    attempt.target,
                                    controller,
                                    recipient,
                                )
                        }
                    }
                };
                if eligible {
                    applications
                        .entry(DamageApplication {
                            effect: DamageEffectId::UntilEndOfTurn {
                                creator,
                                instruction,
                            },
                            scope: DamageScope::Recipient(attempt.target),
                        })
                        .or_default()
                        .push(attempt.id);
                }
            }
        }
        applications
    }

    pub(super) fn apply_turn_prevention(
        &mut self,
        application: DamageApplication,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (
            DamageEffectId::UntilEndOfTurn {
                creator,
                instruction,
            },
            DamageScope::Recipient(recipient),
        ) = (application.effect, application.scope)
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        let before = self.rules_snapshot(None)?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .turn_prevention_applications(&before.view().evaluated, batch)
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let allocations = work.batch.prevent_all(recipient, eligible)?;
        let batch = work.batch.id();
        let stage = work.batch.stage();
        self.emit_from(
            EventCause::DamageAdjusted { batch },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::TurnDamagePrevented {
                    creator,
                    instruction,
                    batch,
                    stage,
                    recipient,
                    allocations,
                }])
            },
        )
    }
}
