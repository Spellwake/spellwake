//! Atomic activated costs that move cards, with zone-replacement continuations.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{PublicZoneDestination, ZoneActionKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectedActivationOwner {
    Permanent(Box<AbilityActivationRecord>),
    Emblem {
        entry: StackEntryId,
        controller: SeatId,
        emblem: crate::emblem::EmblemId,
    },
}
impl SelectedActivationOwner {
    fn entry(&self) -> StackEntryId {
        match self {
            Self::Permanent(a) => a.entry,
            Self::Emblem { entry, .. } => *entry,
        }
    }
    fn controller(&self) -> SeatId {
        match self {
            Self::Permanent(a) => a.controller,
            Self::Emblem { controller, .. } => *controller,
        }
    }
    fn ability(&self) -> crate::activated::ActivatedAbilityId {
        match self {
            Self::Permanent(a) => a.ability,
            Self::Emblem { .. } => crate::activated::ActivatedAbilityId(0),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectedActivationCostWork {
    pub(super) batch: ZoneReplacementBatch,
    pub(super) objects: Vec<ObjectId>,
    pub(super) graveyard_orders: BTreeMap<SeatId, Vec<ObjectId>>,
    pub(super) owner: SelectedActivationOwner,
    pub(super) receipt: ManaPaymentRecord,
    pub(super) action: ZoneActionKind,
}
impl EffectWindow {
    pub(super) fn finish_selected_activation(
        &mut self,
        owner: SelectedActivationOwner,
        receipt: ManaPaymentRecord,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.debit_mana_at_cut(&receipt, false, None, append)?;
        let entry = owner.entry();
        let ability = owner.ability();
        let complete = self.rules_snapshot(None)?;
        let occurrence = match &owner {
            SelectedActivationOwner::Permanent(activation) => RulesOccurrence::ActivatedAbility {
                entry,
                source: activation.source.clone(),
                ability,
            },
            SelectedActivationOwner::Emblem {
                emblem, controller, ..
            } => RulesOccurrence::EmblemActivated {
                entry,
                emblem: *emblem,
                controller: *controller,
            },
        };
        self.emit_from(
            EventCause::ActivatedAbilityCompleted { entry, ability },
            &complete,
            None,
            append,
            |_, _, _| Ok(vec![occurrence]),
        )?;
        self.completed_deferred_receipts = Some((
            receipt,
            match owner {
                SelectedActivationOwner::Permanent(activation) => Some(*activation),
                SelectedActivationOwner::Emblem { .. } => None,
            },
        ));
        Ok(())
    }
    pub(super) fn selected_activation_cost_frame(
        &self,
        work: &SelectedActivationCostWork,
    ) -> Result<Option<EffectDecision>, EffectError> {
        if let Some(frame) = work.batch.frame(self.revision) {
            return Ok(Some(EffectDecision::ZoneReplacement(frame)));
        }
        for player in self.window.apnap_players()? {
            let objects =
                work.objects
                    .iter()
                    .copied()
                    .filter(|id| {
                        work.batch.destination(*id).ok() == Some(PublicZoneDestination::Graveyard)
                            && self.window.cards().get(*id).is_some_and(|card| {
                                card.card_id().is_some() && card.owner() == player
                            })
                    })
                    .collect::<Vec<_>>();
            if objects.len() > 1 && !work.graveyard_orders.contains_key(&player) {
                return Ok(Some(EffectDecision::LinkedReturnOrder(
                    LinkedReturnOrderFrame {
                        revision: self.revision,
                        chooser: player,
                        objects,
                        destination: PublicZoneDestination::Graveyard,
                    },
                )));
            }
        }
        Ok(None)
    }
    pub(super) fn progress_selected_activation_cost(
        mut self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::SelectedActivationCost(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::SelectedActivationCost(work));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        if let Some(frame) = self.selected_activation_cost_frame(&work)? {
            self.pending_work = Some(PendingWork::SelectedActivationCost(work));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        let actor = work.owner.controller();
        let entry = work.owner.entry();
        let ability = work.owner.ability();
        let reason = match work.action {
            ZoneActionKind::MillCost => ZoneReason::Mill { player: actor },
            ZoneActionKind::DiscardCost => ZoneReason::Discard { player: actor },
            ZoneActionKind::SacrificeCost => ZoneReason::SacrificeCost,
            ZoneActionKind::ExileCost => ZoneReason::ExileCost,
            _ => return Err(EffectError::InconsistentProgram),
        };
        let cause = EventCause::ActivatedAbilityCost { entry, ability };
        let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: cause.clone(),
            action: work.action,
            intents: work
                .objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        })?;
        work.batch.verify_initial(fresh)?;
        let mut ordered = Vec::new();
        for player in self.window.apnap_players()? {
            if let Some(order) = work.graveyard_orders.get(&player) {
                ordered.extend(order.iter().rev().copied());
            }
            ordered.extend(work.objects.iter().copied().filter(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .is_some_and(|card| card.owner() == player)
                    && !work
                        .graveyard_orders
                        .get(&player)
                        .is_some_and(|order| order.contains(id))
            }));
        }
        if ordered.len() != work.objects.len() {
            return Err(EffectError::InconsistentProgram);
        }
        let moves = ordered
            .into_iter()
            .map(|id| {
                Ok((
                    id,
                    work.batch
                        .destination(id)?
                        .physical(None)
                        .ok_or(EffectError::InconsistentProgram)?,
                ))
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        // Capture last known power at the sacrifice cut, after replacement choices and
        // before any of the selected objects leave the battlefield.
        if work.action == ZoneActionKind::SacrificeCost && work.objects.len() == 1 {
            let mana_value = self.permanent_mana_value(work.objects[0]);
            let evaluated = self.evaluated_characteristics()?;
            let creature = evaluated
                .get(work.objects[0])
                .filter(|value| value.is_creature());
            let stats = creature.and_then(|value| value.power_toughness.as_ref());
            if let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) {
                payload.sacrificed_creature_power = stats.map(|pt| pt.derived.power.clone());
                payload.sacrificed_creature_toughness =
                    stats.map(|pt| pt.derived.toughness.clone());
                payload.sacrificed_creature_subtypes = creature
                    .map(|value| value.current_creature_subtypes.clone())
                    .unwrap_or_default();
                payload.sacrificed_lki = evaluated.get(work.objects[0]).cloned().map(Box::new);
                payload.sacrificed_permanent_mana_value = mana_value;
            }
        }
        if work.action == ZoneActionKind::ExileCost
            && matches!(
                self.stack_body(entry)
                    .map(|body| body.instructions.as_slice()),
                Some([Instruction::CopyCreatureExiledForAbility])
            )
        {
            let [object] = work.objects.as_slice() else {
                return Err(EffectError::InconsistentProgram);
            };
            let donor = self
                .copiable_program(*object)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.exiled_activation_copy = Some(Box::new(donor));
        }
        let exiled_cost_power =
            if work.action == ZoneActionKind::ExileCost && work.objects.len() == 1 {
                self.evaluated_characteristics()?
                    .get(work.objects[0])
                    .and_then(|value| value.power_toughness.as_ref())
                    .map(|stats| stats.derived.power.clone())
                    .unwrap_or_else(|| 0.into())
            } else {
                0.into()
            };
        let before = self.rules_snapshot(None)?;
        let (window, movements) = if moves.is_empty()
            && matches!(
                &work.owner,
                SelectedActivationOwner::Permanent(activation)
                    if matches!(
                        activation.source_cost,
                        crate::activated::SourceCost::Discard {
                            mode: crate::activated::DiscardCostMode::WholeHand,
                            ..
                        } | crate::activated::SourceCost::GraveyardDiscard {
                            mode: crate::activated::DiscardCostMode::WholeHand,
                        } | crate::activated::SourceCost::ExileGraveyard {
                            filter: crate::activated::GraveyardExileCost::AnyCountX,
                            ..
                        } | crate::activated::SourceCost::GraveyardExileOther {
                            filter: crate::activated::GraveyardExileCost::AnyCountX,
                        }
                    )
            ) {
            (self.window.clone(), vec![])
        } else {
            self.window
                .pay_selected_activation_costs(entry, actor, work.action, &moves)?
        };
        self.window = window;
        if work.action == ZoneActionKind::ExileCost
            && work.objects.len() == 1
            && let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
        {
            let exiled_id = movements
                .first()
                .map_or(work.objects[0], |movement| movement.after.id());
            payload.exiled_cost_creature = Some((exiled_id, exiled_cost_power));
        }
        if let SelectedActivationOwner::Permanent(activation) = &mut work.owner
            && let Some(movement) = movements
                .iter()
                .find(|movement| movement.before.id() == activation.source.id())
        {
            activation.resulting_source = movement.after.id();
        }
        let mut extra_source_sacrifice = None;
        if let SelectedActivationOwner::Permanent(activation) = &mut work.owner
            && activation.source_cost.sacrifices_source()
            && work.action != ZoneActionKind::SacrificeCost
        {
            let (win, src_after) = self.window.pay_announced_source_cost(
                entry,
                actor,
                activation.source.id(),
                crate::activated::SourceCost::Sacrifice,
                false,
            )?;
            self.window = win;
            activation.resulting_source = src_after.id();
            extra_source_sacrifice = Some(crate::object_identity::ObjectMovement {
                before: activation.source.clone(),
                after: src_after,
            });
        }
        let mut append = RulesJournal::default();
        self.emit_from(
            cause,
            &before,
            None,
            &mut append,
            |old, current, controllers| {
                let mut rows = movements
                    .iter()
                    .map(|movement| {
                        let row = zone_occurrence_at(old, current, movement, reason, controllers)?;
                        Ok(occurrence_flow::madness_replaced(
                            row,
                            work.batch.used_madness(movement.before.id()),
                        ))
                    })
                    .collect::<Result<Vec<_>, EffectError>>()?;
                if let Some(movement) = &extra_source_sacrifice {
                    rows.push(zone_occurrence_at(
                        old,
                        current,
                        movement,
                        ZoneReason::SacrificeCost,
                        controllers,
                    )?);
                }
                Ok(rows)
            },
        )?;
        if self.linked_return_work.is_some() {
            self.deferred_selected_activation = Some((work.owner, work.receipt));
            let (next, result, more) = self.continue_linked_returns(turn)?.into_parts();
            append.append(more);
            return Ok(RuleProgress::new(next, result, append));
        }
        self.finish_selected_activation(work.owner, work.receipt, &mut append)?;
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                completion: RuleCompletion::ActivationCost,
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                permanent_entry: None,
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
}
