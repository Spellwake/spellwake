//! Owned replacement-order continuation for an announced source cost.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};

impl EffectWindow {
    pub(super) fn progress_activation_cost(
        mut self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::ActivationCost(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::ActivationCost(work));
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
        let source = work.movement_before.id();
        let cause = EventCause::ActivatedAbilityCost {
            entry: work.activation.entry,
            ability: work.activation.ability,
        };
        let exiling = work.activation.source_cost.exiles_source();
        let action = if exiling {
            crate::program::ZoneActionKind::ExileCost
        } else {
            crate::program::ZoneActionKind::SacrificeCost
        };
        let reason = if exiling {
            ZoneReason::ExileCost
        } else {
            ZoneReason::SacrificeCost
        };
        let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: cause.clone(),
            action,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: source,
                reason,
            }],
        })?;
        work.batch.verify_initial(fresh)?;
        let destination = work
            .batch
            .destination(source)?
            .physical(None)
            .ok_or(EffectError::InconsistentProgram)?;
        let before = self.rules_snapshot(None)?;
        let (window, after) = self.window.pay_announced_source_to(
            work.activation.entry,
            work.activation.controller,
            source,
            destination,
        )?;
        self.window = window;
        work.activation.resulting_source = after.id();
        let movement = crate::object_identity::ObjectMovement {
            before: work.movement_before,
            after,
        };
        let mut append = RulesJournal::default();
        self.emit_from(
            cause,
            &before,
            None,
            &mut append,
            |previous, current, controllers| {
                Ok(vec![zone_occurrence_at(
                    previous,
                    current,
                    &movement,
                    reason,
                    controllers,
                )?])
            },
        )?;
        if self.linked_return_work.is_some() {
            self.deferred_activation = Some(DeferredActivation {
                activation: work.activation,
                receipt: work.receipt,
            });
            let (next, result, more) = self.continue_linked_returns(turn)?.into_parts();
            append.append(more);
            return Ok(RuleProgress::new(next, result, append));
        }
        self.debit_mana_at_cut(&work.receipt, false, None, &mut append)?;
        let complete = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::ActivatedAbilityCompleted {
                entry: work.activation.entry,
                ability: work.activation.ability,
            },
            &complete,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::ActivatedAbility {
                    entry: work.activation.entry,
                    source: work.activation.source.clone(),
                    ability: work.activation.ability,
                }])
            },
        )?;
        self.completed_deferred_receipts = Some((work.receipt, Some(work.activation)));
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
