//! Preserve one already begun priority-check placement wave through departure.
use super::*;
impl EffectWindow {
    #[cfg(test)]
    pub(crate) fn qualification_corrupt_legend(
        &mut self,
        fault: private_qualification::QualificationLegendFault,
    ) {
        use private_qualification::QualificationLegendFault as Fault;
        let CheckPhase::Actions(actions) =
            &mut self.check_process_mut().expect("actual checks").phase
        else {
            panic!("actual actions")
        };
        let other = actions
            .legends
            .iter()
            .find(|legend| legend.accepted.is_none())
            .expect("actual unanswered other cohort")
            .conflict
            .clone();
        let accepted = actions
            .legends
            .iter_mut()
            .find_map(|legend| legend.accepted.as_mut())
            .expect("actual accepted certificate");
        match fault {
            Fault::Event => accepted.conflict.id.event.pass += 1,
            Fault::Controller => accepted.conflict.id.controller = other.id.controller,
            Fault::Domain => accepted.conflict.domain.push(other.domain[0]),
            Fault::Names => accepted.conflict.comparison_names = other.comparison_names,
            Fault::Keep => accepted.keep = other.domain[0],
        }
    }

    #[cfg(test)]
    pub(crate) fn qualification_legend_certificates(
        &self,
    ) -> Vec<(LegendConflict, LegendConflict, ObjectId)> {
        let CheckPhase::Actions(actions) = &self.check_process().expect("actual checks").phase
        else {
            panic!("actual actions")
        };
        actions
            .legends
            .iter()
            .filter_map(|legend| {
                legend.accepted.as_ref().map(|accepted| {
                    (
                        legend.conflict.clone(),
                        accepted.conflict.clone(),
                        accepted.keep,
                    )
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn qualification_corrupt_check_target(
        &mut self,
        fault: private_qualification::QualificationCheckTargetFault,
    ) {
        use private_qualification::QualificationCheckTargetFault as Fault;
        let placement = self
            .pending_trigger_placement()
            .expect("valid placement")
            .expect("actual target stage");
        match fault {
            Fault::MissingCapture => {
                self.triggers
                    .remove_departed(&BTreeSet::from([placement.controller]));
                return;
            }
            Fault::TargetlessCapture => {
                self.triggers.qualification_corrupt_capture(
                    placement.instance,
                    crate::occurrence::QualificationCaptureFault::Targetless,
                );
                return;
            }
            Fault::DifferentSource(source) => {
                self.triggers.qualification_corrupt_capture(
                    placement.instance,
                    crate::occurrence::QualificationCaptureFault::Source(
                        self.window
                            .cards()
                            .get(source)
                            .expect("actual alternate source object")
                            .clone(),
                    ),
                );
                return;
            }
            _ => {}
        }
        let other = self
            .window
            .entries()
            .iter()
            .find(|entry| entry.id() != placement.entry)
            .expect("actual earlier prefix entry")
            .id();
        let CheckPhase::Placement(cursor) =
            &mut self.check_process_mut().expect("actual checks").phase
        else {
            panic!("placement")
        };
        let PlacementStage::Targets {
            order,
            position,
            entry,
        } = &mut cursor.stage
        else {
            panic!("target stage")
        };
        match fault {
            Fault::DuplicateOrder => order[0] = order[*position],
            Fault::ExtendedOrder => order.push(order[*position]),
            Fault::PositionPastEnd => *position = order.len(),
            Fault::WrongEntry => *entry = other,
            _ => unreachable!(),
        }
    }

    #[cfg(test)]
    pub(crate) fn qualification_corrupt_check_order(
        &mut self,
        target: SeatId,
        fault: private_qualification::QualificationCheckOrderFault,
    ) {
        use private_qualification::QualificationCheckOrderFault as Fault;
        if matches!(fault, Fault::MissingFutureCapture) {
            self.triggers.remove_departed(&BTreeSet::from([target]));
            return;
        }
        if matches!(fault, Fault::UnallocatedWave) {
            self.triggers.set_test_next_wave(0);
            return;
        }
        let process = self.check_process_mut().expect("actual checks process");
        if matches!(fault, Fault::UnsupportedStage) {
            process.phase = CheckPhase::Observe;
            return;
        }
        let CheckPhase::Placement(cursor) = &mut process.phase else {
            panic!("actual placement")
        };
        let (current, instances) = &cursor.groups[cursor.player];
        let controller = *current;
        let foreign = instances[0];
        let (_, future) = cursor
            .groups
            .iter_mut()
            .skip(cursor.player + 1)
            .find(|(controller, _)| *controller == target)
            .expect("actual future chooser");
        match fault {
            Fault::DuplicateFutureMember => future.push(future[0]),
            Fault::ForeignFutureMember => future[0] = foreign,
            Fault::DuplicateFutureController => {
                cursor
                    .groups
                    .iter_mut()
                    .skip(cursor.player + 1)
                    .find(|(owner, _)| *owner == target)
                    .expect("actual future chooser")
                    .0 = controller;
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn validate_check_placement(&self) -> Result<(), EffectError> {
        let process = self.check_process()?;
        let CheckPhase::Placement(cursor) = &process.phase else {
            return Err(EffectError::UnsupportedDepartureState);
        };
        if process.boundary != CheckBoundary::Priority
            || cursor.player >= cursor.groups.len()
            || cursor.wave.sequence() >= self.triggers.counters().2
            || !matches!(
                self.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            )
        {
            return Err(EffectError::UnsupportedDepartureState);
        }
        let (controller, current_group) = &cursor.groups[cursor.player];
        if !self.window.participants().contains(controller) {
            return Err(EffectError::InconsistentProgram);
        }
        let current_suffix = match &cursor.stage {
            PlacementStage::Ordering => {
                self.current_trigger_order()?;
                current_group.as_slice()
            }
            PlacementStage::Targets {
                order, position, ..
            } => {
                if *position >= order.len()
                    || order.len() != current_group.len()
                    || order.iter().copied().collect::<BTreeSet<_>>()
                        != current_group.iter().copied().collect()
                {
                    return Err(EffectError::InconsistentProgram);
                }
                self.current_trigger_target()?;
                &order[*position..]
            }
            PlacementStage::Placing { .. } => return Err(EffectError::UnsupportedDepartureState),
        };
        let mut controllers = BTreeSet::new();
        let mut instances = BTreeSet::new();
        for (index, (controller, group)) in cursor.groups.iter().enumerate() {
            if !self.window.known_player(*controller)
                || !controllers.insert(*controller)
                || group.windows(2).any(|pair| pair[0] >= pair[1])
                || group.iter().any(|instance| !instances.insert(*instance))
            {
                return Err(EffectError::InconsistentProgram);
            }
            // Historical placed prefixes no longer own waiting captures.
            let suffix = if index == cursor.player {
                current_suffix
            } else {
                group
            };
            if index >= cursor.player
                && ((!suffix.is_empty() && !self.window.participants().contains(controller))
                    || suffix.iter().any(|id| {
                        self.triggers
                            .waiting()
                            .get(id)
                            .is_none_or(|waiting| waiting.controller != *controller)
                    }))
            {
                return Err(EffectError::InconsistentProgram);
            }
        }
        Ok(())
    }
    pub(in crate::effects) fn check_departure_admitted(
        &self,
        turn: Option<&TurnScheduler>,
        issued: Option<&EffectDecision>,
    ) -> Result<bool, EffectError> {
        let Some(PendingWork::Checks(process)) = &self.pending_work else {
            return Ok(false);
        };
        if matches!(process.boundary, CheckBoundary::Cleanup(_))
            && !matches!(process.phase, CheckPhase::Actions(_))
        {
            if matches!(process.phase, CheckPhase::CleanupMoves { .. }) {
                self.require_check_boundary(process.boundary, turn)?;
                return Ok(match issued {
                    Some(EffectDecision::ZoneReplacement(frame)) => {
                        self.check_replacement_frame()?.as_ref() == Some(frame)
                    }
                    Some(EffectDecision::Checks(frame @ CheckDecision::GraveyardOrder(_))) => {
                        self.current_check_decision()? == *frame
                    }
                    _ => false,
                });
            }
            if !matches!(
                process.phase,
                CheckPhase::Discard { .. } | CheckPhase::DiscardSelection { .. }
            ) {
                return Ok(false);
            }
            self.require_check_boundary(process.boundary, turn)?;
            return Ok(matches!(issued, Some(EffectDecision::Checks(frame))
                if self.current_check_decision()?.eq(frame)));
        }
        if let CheckPhase::Actions(actions) = &process.phase {
            self.validate_state_actions(actions, process.event)?;
            return Ok(match issued {
                Some(EffectDecision::Checks(frame @ CheckDecision::Legend(_)))
                    if actions.movements.is_none() =>
                {
                    self.current_check_decision()? == *frame
                }
                Some(EffectDecision::Checks(frame @ CheckDecision::GraveyardOrder(_))) => {
                    self.current_check_decision()? == *frame
                }
                Some(EffectDecision::ZoneReplacement(frame)) => {
                    self.check_replacement_frame()?.as_ref() == Some(frame)
                }
                _ => false,
            });
        }
        let CheckPhase::Placement(cursor) = &process.phase else {
            return Ok(false);
        };
        let matches = match (&cursor.stage, issued) {
            (PlacementStage::Ordering, Some(EffectDecision::Triggers(frame))) => {
                &self.current_trigger_order()? == frame
            }
            (PlacementStage::Targets { .. }, Some(EffectDecision::TriggerTarget(frame))) => {
                &self.current_trigger_target()? == frame
            }
            _ => false,
        };
        if !matches {
            return Ok(false);
        }
        self.validate_check_placement()?;
        Ok(true)
    }
    pub(in crate::effects) fn refresh_checks_after_departure(
        &mut self,
        before: &Self,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let process = before.check_process()?;
        if matches!(process.boundary, CheckBoundary::Cleanup(_))
            && !matches!(process.phase, CheckPhase::Actions(_))
        {
            if matches!(process.phase, CheckPhase::CleanupMoves { .. }) {
                self.require_check_boundary(process.boundary, turn)?;
                if self.window.active_player().is_none() && before.window.active_player().is_some()
                {
                    // Departure removed the active player's entire hand before this uncommitted discard.
                    if let CheckPhase::CleanupMoves {
                        domain,
                        required,
                        selected,
                        moves,
                    } = &process.phase
                    {
                        before
                            .authorize_cleanup_moves(domain, *required, selected, moves, process)?;
                    }
                    return self
                        .finish_cleanup_discard(turn.ok_or(EffectError::WrongPhase)?, append);
                }
                return self.refresh_cleanup_replacement(before);
            }
            return self.refresh_cleanup_discard_departure(process, turn, append);
        }
        if let CheckPhase::Actions(actions) = &before.check_process()?.phase {
            return self.refresh_state_actions_after_departure(before, actions);
        }
        before.validate_check_placement()?;
        if self.check_process()? != before.check_process()? {
            return Err(EffectError::InconsistentProgram);
        }
        let original_placement = before.pending_trigger_placement()?;
        let current_controller = original_placement
            .as_ref()
            .map(|placement| placement.controller);
        let controller_departed = current_controller
            .is_some_and(|controller| !self.window.participants().contains(&controller));
        if controller_departed
            && original_placement.as_ref().is_some_and(|placement| {
                self.window
                    .entries()
                    .iter()
                    .any(|entry| entry.id() == placement.entry)
            })
        {
            return Err(EffectError::InconsistentProgram);
        }
        let retained = self.triggers.waiting();
        let Some(PendingWork::Checks(process)) = &mut self.pending_work else {
            return Err(EffectError::InconsistentProgram);
        };
        let CheckPhase::Placement(cursor) = &mut process.phase else {
            return Err(EffectError::InconsistentProgram);
        };
        let first_future = if original_placement.is_some() {
            if controller_departed {
                cursor.groups[cursor.player].1.clear();
                cursor.stage = PlacementStage::Ordering;
            }
            cursor.player + 1
        } else {
            cursor.player
        };
        for (controller, group) in cursor.groups.iter_mut().skip(first_future) {
            group.retain(|id| retained.contains_key(id));
            if group
                .iter()
                .any(|id| retained[id].controller != *controller)
            {
                return Err(EffectError::InconsistentProgram);
            }
        }
        if original_placement.is_some() && !controller_departed {
            let placement = self
                .pending_trigger_placement()?
                .ok_or(EffectError::InconsistentProgram)?;
            let waiting = self
                .triggers
                .waiting()
                .get(&placement.instance)
                .ok_or(EffectError::InconsistentProgram)?;
            if self.trigger_target_domain(&waiting.body)?.is_empty() {
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
    fn refresh_cleanup_discard_departure(
        &mut self,
        original: &CheckProcess,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.require_check_boundary(original.boundary, turn)?;
        if self.check_process()? != original
            || !matches!(
                original.phase,
                CheckPhase::Discard { .. } | CheckPhase::DiscardSelection { .. }
            )
        {
            return Err(EffectError::UnsupportedDepartureState);
        }
        // An ownerless turn has no active hand to discard (CR 800.4j, 514.1).
        // Finish the remaining cleanup work without inventing a discard reply.
        if let Some(phase) = self.cleanup_discard_phase()? {
            self.check_process_mut()?.phase = phase;
            Ok(())
        } else {
            self.finish_cleanup_discard(turn.ok_or(EffectError::WrongPhase)?, append)
        }
    }

    fn refresh_state_actions_after_departure(
        &mut self,
        before: &Self,
        original: &StateActions,
    ) -> Result<(), EffectError> {
        let process = before.check_process()?;
        if self.check_process()? != process {
            return Err(EffectError::UnsupportedDepartureState);
        }
        before.validate_state_actions(original, process.event)?;
        let mut fresh = self.collect_state_actions(process.event)?;
        for accepted in original
            .legends
            .iter()
            .filter_map(|legend| legend.accepted.as_ref())
        {
            if !self
                .window
                .participants()
                .contains(&accepted.conflict.id.controller)
            {
                continue;
            }
            // A changed conflict is a new choice. Preserve the prior decision
            // in its original journal and retain every still-identical cohort.
            let Some(retained) = fresh.legends.iter_mut().find(|legend| {
                legend.conflict.id.controller == accepted.conflict.id.controller
                    && legend.conflict.domain == accepted.conflict.domain
                    && legend.conflict.comparison_names == accepted.conflict.comparison_names
            }) else {
                continue;
            };
            if retained.accepted.is_some() || !retained.conflict.domain.contains(&accepted.keep) {
                return Err(EffectError::InconsistentProgram);
            }
            retained.accepted = Some(accepted.clone());
        }
        self.validate_state_actions(&fresh, process.event)?;
        if let Some(previous) = &original.movements {
            before.authorize_state_movements(original, previous, process)?;
            if !fresh.is_empty() && fresh.legends.iter().all(|legend| legend.accepted.is_some()) {
                // Replacement authorization reconstructs the event from the
                // current cursor. Install the surviving simultaneous obligations first.
                self.check_process_mut()?.phase = CheckPhase::Actions(fresh.clone());
                let mut movements = self.determine_state_movements(&fresh)?;
                self.rebase_check_moves(
                    before,
                    &previous.moves,
                    &mut movements.moves,
                    crate::program::ZoneActionKind::StateBased,
                )?;
                self.authorize_state_movements(&fresh, &movements, process)?;
                fresh.movements = Some(movements);
            }
        }
        self.check_process_mut()?.phase = if fresh.is_empty() {
            CheckPhase::Observe
        } else {
            CheckPhase::Actions(fresh)
        };
        Ok(())
    }

    pub(in crate::effects) fn progress_checks_after_departure(
        &self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        self.require_check_boundary(self.check_process()?.boundary, turn)?;
        self.clone()
            .progress_checks(CheckJournal::default(), RulesJournal::default(), turn)
    }
}
