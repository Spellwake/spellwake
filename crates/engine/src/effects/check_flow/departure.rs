//! Keep the authoritative state-check process through a multiplayer departure.
use super::*;

impl EffectWindow {
    pub(in crate::effects) fn check_departure_admitted(
        &self,
        issued: Option<&EffectDecision>,
        turn: Option<&TurnScheduler>,
    ) -> Result<bool, EffectError> {
        let process = self.check_process()?;
        self.require_check_boundary(process.boundary, turn)?;
        Ok(match (&process.phase, issued) {
            (
                CheckPhase::Discard { .. } | CheckPhase::DiscardSelection { .. },
                Some(EffectDecision::Checks(frame)),
            ) => self.current_check_decision()? == *frame,
            (CheckPhase::Actions(actions), Some(EffectDecision::Checks(frame)))
                if actions.movements.is_none() =>
            {
                self.validate_state_actions(actions, process.event)?;
                self.current_check_decision()? == *frame
            }
            (CheckPhase::Placement(cursor), Some(EffectDecision::Triggers(frame)))
                if cursor.stage == PlacementStage::Ordering =>
            {
                self.current_trigger_order()? == *frame
            }
            (CheckPhase::Placement(cursor), Some(EffectDecision::TriggerTarget(frame)))
                if matches!(cursor.stage, PlacementStage::Targets { .. }) =>
            {
                self.current_trigger_target()? == *frame
            }
            _ => false,
        })
    }

    pub(in crate::effects) fn refresh_checks_after_departure(
        &mut self,
        before: &Self,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let original = before.check_process()?;
        self.require_check_boundary(original.boundary, turn)?;
        if self.check_process()? != original {
            return Err(EffectError::InconsistentProgram);
        }
        match &original.phase {
            CheckPhase::Discard { .. } | CheckPhase::DiscardSelection { .. } => {
                if let Some(phase) = self.cleanup_discard_phase()? {
                    self.check_process_mut()?.phase = phase;
                } else {
                    self.cleanup_characteristics(turn.ok_or(EffectError::WrongPhase)?, append)?;
                    let process = self.check_process_mut()?;
                    // Slot zero is reserved for the discard cut (CR 514.1).
                    process.event.pass = process
                        .event
                        .pass
                        .checked_add(1)
                        .ok_or(CheckError::IterationExhausted)?;
                    process.phase = CheckPhase::Observe;
                }
                Ok(())
            }
            CheckPhase::Actions(actions) if actions.movements.is_none() => {
                self.refresh_departed_action_cohorts(original.event, actions)
            }
            CheckPhase::Placement(_) => self.refresh_departed_trigger_cohorts(before, append),
            _ => Err(EffectError::WrongPhase),
        }
    }

    fn refresh_departed_action_cohorts(
        &mut self,
        event: CheckEventId,
        original: &StateActions,
    ) -> Result<(), EffectError> {
        let mut fresh = self.collect_state_actions(event)?;
        // Accepted simultaneous choices survive while their exact obligation survives.
        for old in &original.legends {
            let Some(kept) = old.keep else { continue };
            if !self
                .window
                .participants()
                .contains(&old.conflict.id.controller)
            {
                continue;
            }
            let group = fresh
                .legends
                .iter_mut()
                .find(|new| new.conflict == old.conflict)
                .ok_or(EffectError::WrongPhase)?;
            group.keep = Some(kept);
        }
        for group in &mut fresh.commanders {
            group.selected = original
                .commanders
                .iter()
                .find(|old| old.owner == group.owner && old.domain == group.domain)
                .and_then(|old| old.selected.clone());
        }
        for change in &mut fresh.protectors {
            change.selected = original
                .protectors
                .iter()
                .find(|old| old.battle == change.battle && old.controller == change.controller)
                .and_then(|old| old.selected)
                .filter(|selected| change.domain.contains(selected));
        }
        self.validate_state_actions(&fresh, event)?;
        self.check_process_mut()?.phase = if fresh.is_empty() {
            CheckPhase::Observe
        } else {
            CheckPhase::Actions(Box::new(fresh))
        };
        Ok(())
    }

    fn refresh_departed_trigger_cohorts(
        &mut self,
        before: &Self,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let provisional = before.pending_trigger_placement()?;
        let participants = self.window.participants();
        let Some(PendingWork::Checks(process)) = &mut self.pending_work else {
            return Err(EffectError::InconsistentProgram);
        };
        let CheckPhase::Placement(cursor) = &mut process.phase else {
            return Err(EffectError::InconsistentProgram);
        };
        for (seat, group) in cursor.groups.iter_mut().skip(cursor.player) {
            if !participants.contains(seat) {
                group.clear();
            }
        }
        if let Some(placement) = provisional {
            if !participants.contains(&placement.controller) {
                cursor.stage = PlacementStage::Ordering;
            } else if self.available_trigger_target(&placement)?.is_none() {
                self.window = self
                    .window
                    .remove_unbound_trigger(self.window.revision(), placement.entry)?;
                self.finish_trigger_placement(
                    placement,
                    Some(TriggerRemovalReason::NoLegalTargets),
                    append,
                )?;
            }
        }
        Ok(())
    }
}
