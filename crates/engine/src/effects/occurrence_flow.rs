//! Actual-cut snapshots and the single local occurrence/discovery publisher.
use super::*;
use crate::{
    combat::Combatant,
    object_identity::{ControllerChange, ObjectMovement, ObjectStatusChange},
    occurrence::{
        CharacteristicAppearanceChange, DrawOrigin, EventCause, RulesJournal, RulesOccurrence,
        RulesView, ZoneOccurrence, ZoneReason,
    },
    stack_window::DrawBatchProgress,
};

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
    life: crate::life::LifeState,
    combat: Option<CombatState>,
    turn: Option<TurnScheduler>,
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
            cards,
            programs: &self.programs,
            state: &self.characteristics,
            vocabulary: &self.vocabulary,
            name_rules: &self.name_rules,
        })?);
        Ok(self.view_with_evaluated(cards, evaluated))
    }
    fn view_with_evaluated<'a>(
        &'a self,
        cards: &'a GameObjects,
        evaluated: Arc<EvaluatedCharacteristics>,
    ) -> RulesView<'a> {
        RulesView {
            window: &self.window,
            cards,
            characteristics: &self.characteristics,
            evaluated,
            marks: &self.marks,
            mana: &self.mana,
            life: &self.life,
            combat: self.combat.as_ref(),
            turn: self.turn.as_ref(),
        }
    }
}
impl EffectWindow {
    /// Complete through the sole stack mutator and consume its actual exit receipt.
    pub(super) fn complete_at_cut(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        completion: crate::occurrence::EntryCompletion,
        destination: Option<ObjectDestination>,
        append: &mut RulesJournal,
    ) -> Result<Option<ObjectMovement>, EffectError> {
        let before = self.rules_snapshot(None)?;
        let (window, exit) = match completion {
            crate::occurrence::EntryCompletion::Resolved => self
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
        if let Some(movement) = &movement
            && movement.after.zone() == ObjectZone::Battlefield
            && movement.before.zone() != ObjectZone::Battlefield
        {
            self.characteristics
                .register_battlefield_entry(movement.after.id())?;
        }
        let reason = if matches!(destination, Some(ObjectDestination::Battlefield { .. })) {
            ZoneReason::PermanentResolution
        } else if completion == crate::occurrence::EntryCompletion::AllTargetsIllegal {
            ZoneReason::AllTargetsIllegalExit
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
                Ok(vec![value])
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
    pub(super) fn credit_mana_at_cut(
        &mut self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        controller: SeatId,
        produced: BTreeMap<ManaType, RulesInteger>,
        append: &mut RulesJournal,
    ) -> Result<ManaProductionRecord, EffectError> {
        // A counted mana ability may legally resolve producing nothing. Its tap
        // and activation facts already exist; CR 106.12a requires real produced
        // mana before a tapped-for-mana occurrence can arise.
        if produced.is_empty() {
            return Ok(self.credit_mana(source, ability, controller, produced));
        }
        let before = self.rules_snapshot(None)?;
        let production = self.credit_mana(source, ability, controller, produced);
        self.emit_from(
            EventCause::ManaProduced { source, ability },
            &before,
            None,
            append,
            |previous, current, _| {
                Ok(vec![RulesOccurrence::ManaProduced {
                    production: production.clone(),
                    before: previous
                        .mana_pool(controller)
                        .ok_or(EffectError::InconsistentProgram)?,
                    after: current
                        .mana_pool(controller)
                        .ok_or(EffectError::InconsistentProgram)?,
                }])
            },
        )?;
        Ok(production)
    }
    pub(super) fn debit_mana_at_cut(
        &mut self,
        payment: &ManaPaymentRecord,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if payment.spend.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(turn)?;
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
                        let triggers = std::mem::take(&mut self.triggers);
                        let (state, _, records) = triggers
                            .publish(
                                EventCause::Draw { player, origin },
                                vec![value],
                                &previous,
                                &current,
                            )?
                            .into_parts();
                        (current.evaluated.clone(), state, records)
                    };
                    self.triggers = state;
                    append.append(records);
                    before_evaluated = after_evaluated;
                    batch = next;
                }
                DrawBatchProgress::Complete { window, journal } => {
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
                        self.triggers = state;
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
            characteristics: self.characteristics.clone(),
            marks: self.damage.clone(),
            mana: self.mana.clone(),
            life: self.life.clone(),
            combat: self.combat.clone(),
            turn: turn.cloned(),
        })
    }
    fn rules_view_with_evaluated<'a>(
        &'a self,
        turn: Option<&'a TurnScheduler>,
        evaluated: Arc<EvaluatedCharacteristics>,
    ) -> RulesView<'a> {
        RulesView {
            window: &self.window,
            cards: self.window.cards(),
            characteristics: &self.characteristics,
            evaluated,
            marks: &self.damage,
            mana: &self.mana,
            life: &self.life,
            combat: self.combat.as_ref(),
            turn,
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
            combat.reconcile(|participant| {
                let retained = cards.get(participant.object()).is_some_and(|card| {
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
        self.prune_non_battlefield_state();
        let mut evaluated = self.evaluated_characteristics()?;
        let mut default_exiles = Vec::new();
        loop {
            let exiles = self
                .window
                .cards()
                .iter()
                .filter(|object| {
                    object.zone() == ObjectZone::Battlefield
                        && object
                            .default_controller()
                            .is_some_and(|player| !self.window.participants().contains(&player))
                        && evaluated
                            .get(object.id())
                            .and_then(|current| current.controller)
                            == object.default_controller()
                })
                .map(GameObject::id)
                .collect::<Vec<_>>();
            if exiles.is_empty() {
                break;
            }
            // CR 800.4c exile is outside the admitted action-specific replacement predicates.
            let (window, movements) = self.window.exile_departed_defaults(&exiles)?;
            self.window = window;
            default_exiles.extend(movements);
            self.prune_non_battlefield_state();
            evaluated = self.evaluated_characteristics()?;
        }
        let evaluated = Arc::new(evaluated);
        let world_changes = self.characteristics.reconcile_world_tenures(&evaluated)?;
        let controllers = self.synchronize_controllers(&evaluated)?;
        let removed = self.reconcile_characteristic_boundary(&evaluated);
        let mut values = build(
            &before.view(),
            &self.rules_view_with_evaluated(turn, evaluated.clone()),
            &controllers,
        )?;
        for movement in default_exiles {
            values.push(zone_occurrence_at(
                &before.view(),
                &self.rules_view_with_evaluated(turn, evaluated.clone()),
                &movement,
                ZoneReason::DefaultControllerDeparted,
                &controllers,
            )?);
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
        if values.is_empty() {
            return Ok(());
        }
        let triggers = std::mem::take(&mut self.triggers);
        let updated = triggers.publish(
            cause,
            values,
            &before.view(),
            &self.rules_view_with_evaluated(turn, evaluated),
        )?;
        let (state, _, records) = updated.into_parts();
        self.triggers = state;
        #[cfg(test)]
        super::characteristic_observations::published(self, &records);
        append.append(records);
        Ok(())
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
    Ok(RulesOccurrence::ZoneChange(Box::new(ZoneOccurrence {
        before: previous,
        after: current,
        reason,
    })))
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
