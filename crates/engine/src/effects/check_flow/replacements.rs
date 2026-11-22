//! Checks retains its complete parent; only neutral zone intents enter the shared reducer.
use super::*;
use crate::effects::zone_replacement::AuthorizedZoneBatch;
use crate::program::{ZoneActionKind, ZoneActionPredicate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckMoves {
    intents: Vec<ZoneMoveIntent>,
    window_revision: u64,
    stage: CheckMoveStage,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CheckMoveStage {
    Replacing(Box<ZoneReplacementBatch>),
    Ordering {
        replacements: Option<Box<ZoneReplacementBatch>>,
        groups: Vec<GraveyardGroup>,
    },
}
impl CheckMoves {
    pub(super) fn order_group(&self) -> Option<&GraveyardGroup> {
        match &self.stage {
            CheckMoveStage::Ordering { groups, .. } => groups.iter().find(|g| g.order.is_none()),
            _ => None,
        }
    }
    pub(super) fn order_group_mut(&mut self) -> Option<&mut GraveyardGroup> {
        match &mut self.stage {
            CheckMoveStage::Ordering { groups, .. } => {
                groups.iter_mut().find(|g| g.order.is_none())
            }
            _ => None,
        }
    }
    /// Private prepared fault fixture only: this is not a player-reachable cut.
    #[cfg(test)]
    pub(super) fn qualification_rebind_cut(&mut self, revision: u64) {
        assert!(self.replacements().is_none());
        self.window_revision = revision;
    }
    #[cfg(test)]
    pub(super) fn groups(&self) -> &[GraveyardGroup] {
        let CheckMoveStage::Ordering { groups, .. } = &self.stage else {
            panic!("test ordering stage")
        };
        groups
    }
    #[cfg(test)]
    pub(super) fn groups_mut(&mut self) -> &mut [GraveyardGroup] {
        let CheckMoveStage::Ordering { groups, .. } = &mut self.stage else {
            panic!("test ordering stage")
        };
        groups
    }
    pub(super) fn objects(&self) -> Vec<ObjectId> {
        self.intents.iter().map(|i| i.before).collect()
    }
    pub(super) fn orders(&self) -> Result<Vec<ZoneOwnerOrder>, EffectError> {
        let CheckMoveStage::Ordering { groups, .. } = &self.stage else {
            return Err(CheckError::WrongFrame.into());
        };
        groups
            .iter()
            .map(|g| {
                Ok(ZoneOwnerOrder {
                    owner: g.owner,
                    top_to_bottom: g.order.clone().ok_or(CheckError::WrongFrame)?,
                })
            })
            .collect()
    }
    pub(super) fn replacements(&self) -> Option<&ZoneReplacementBatch> {
        match &self.stage {
            CheckMoveStage::Replacing(batch) => Some(batch),
            CheckMoveStage::Ordering { replacements, .. } => replacements.as_deref(),
        }
    }
}
impl EffectWindow {
    /// Whether the actual owned Checks stage has semantics excluded by historical observers.
    /// This predicate is independent of live Programs, definitions and currently issued frames.
    pub fn has_check_replacement_work(&self) -> bool {
        let Some(PendingWork::Checks(process)) = &self.pending_work else {
            return false;
        };
        match &process.phase {
            CheckPhase::DiscardSelection { .. } | CheckPhase::CleanupMoves { .. } => true,
            CheckPhase::Actions(actions) => actions
                .movements
                .as_ref()
                .is_some_and(|m| m.moves.replacements().is_some()),
            _ => false,
        }
    }
    /// Borrow only the complete generic replacement work, not a complete Checks archive.
    pub fn check_zone_replacement(&self) -> Option<&ZoneReplacementBatch> {
        let Some(PendingWork::Checks(process)) = &self.pending_work else {
            return None;
        };
        match &process.phase {
            CheckPhase::CleanupMoves { moves, .. } => moves.replacements(),
            CheckPhase::Actions(actions) => actions.movements.as_ref()?.moves.replacements(),
            _ => None,
        }
    }
    pub(super) fn has_cleanup_replacement_source(&self) -> Result<bool, EffectError> {
        Ok(self.zone_replacement_sources()?.sources.values().any(|s|matches!(&s.definition.actions,ZoneActionPredicate::OneOf(actions) if actions.contains(&ZoneActionKind::CleanupDiscard))))
    }
    pub(super) fn new_check_moves(
        &self,
        intents: Vec<ZoneMoveIntent>,
        cause: EventCause,
        action: ZoneActionKind,
    ) -> Result<CheckMoves, EffectError> {
        let replacements = self.begin_check_replacements(&intents, cause, action)?;
        let stage = if let Some(batch) = replacements {
            CheckMoveStage::Replacing(batch)
        } else {
            CheckMoveStage::Ordering {
                replacements: None,
                groups: self.check_arrival_groups(&intents, None)?,
            }
        };
        Ok(CheckMoves {
            intents,
            window_revision: self.window.revision(),
            stage,
        })
    }
    /// The Check owner alone binds a causal-shaped input to an actual phase/cohort.
    pub(in crate::effects) fn authorize_check_zone_input(
        &self,
        cause: &EventCause,
        action: ZoneActionKind,
        intents: &[ZoneMoveIntent],
    ) -> Result<(), EffectError> {
        let process = self.check_process()?;
        let valid_boundary = match process.boundary {
            CheckBoundary::Priority => matches!(
                self.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ),
            CheckBoundary::Cleanup(_) => {
                self.window.phase() == WindowPhase::TurnWork && self.window.entries().is_empty()
            }
        };
        if !valid_boundary {
            return Err(CheckError::WrongFrame.into());
        }
        if action == ZoneActionKind::CleanupDiscard
            && (!matches!(process.boundary, CheckBoundary::Cleanup(_)) || process.event.pass != 0)
        {
            return Err(CheckError::WrongFrame.into());
        }
        if action == ZoneActionKind::StateBased
            && matches!(process.boundary, CheckBoundary::Cleanup(_))
            && process.event.pass == 0
        {
            return Err(CheckError::WrongFrame.into());
        }
        let expected = match action {
            ZoneActionKind::CleanupDiscard => EventCause::CleanupDiscard {
                event: process.event,
                boundary: process.boundary,
            },
            ZoneActionKind::StateBased => EventCause::StateBasedActions {
                event: process.event,
                boundary: process.boundary,
            },
            _ => return Err(CheckError::WrongFrame.into()),
        };
        if *cause != expected {
            return Err(CheckError::WrongFrame.into());
        }
        match (action, &process.phase) {
            (ZoneActionKind::CleanupDiscard, CheckPhase::DiscardSelection { domain, required }) => {
                let objects = intents.iter().map(|i| i.before).collect::<Vec<_>>();
                if self.cleanup_hand() != *domain
                    || *required != self.cleanup_discard_count(domain.len())?
                    || objects.len() != *required
                    || !valid_subset(&objects, domain)
                {
                    return Err(CheckError::WrongFrame.into());
                }
                if intents.iter().any(|i| {
                    i.reason
                        != ZoneReason::Discard {
                            player: self.window.active_player(),
                        }
                }) {
                    return Err(CheckError::WrongFrame.into());
                }
            }
            (
                ZoneActionKind::CleanupDiscard,
                CheckPhase::CleanupMoves {
                    domain,
                    required,
                    selected,
                    ..
                },
            ) => {
                if self.cleanup_hand() != *domain
                    || *required != self.cleanup_discard_count(domain.len())?
                    || selected.len() != *required
                    || !valid_subset(selected, domain)
                {
                    return Err(CheckError::WrongFrame.into());
                }
                let expected = selected
                    .iter()
                    .map(|before| ZoneMoveIntent {
                        before: *before,
                        reason: ZoneReason::Discard {
                            player: self.window.active_player(),
                        },
                    })
                    .collect::<Vec<_>>();
                if expected != intents {
                    return Err(CheckError::WrongFrame.into());
                }
            }
            (ZoneActionKind::StateBased, CheckPhase::Actions(actions)) => {
                self.validate_state_actions(actions, process.event)?;
                let expected = self
                    .state_dead_union(actions)?
                    .into_iter()
                    .map(|(before, reasons)| ZoneMoveIntent {
                        before,
                        reason: ZoneReason::StateBased(reasons),
                    })
                    .collect::<Vec<_>>();
                if expected != intents {
                    return Err(CheckError::WrongFrame.into());
                }
            }
            _ => return Err(CheckError::WrongFrame.into()),
        }
        Ok(())
    }
    fn begin_check_replacements(
        &self,
        intents: &[ZoneMoveIntent],
        cause: EventCause,
        action: ZoneActionKind,
    ) -> Result<Option<Box<ZoneReplacementBatch>>, EffectError> {
        if intents.is_empty() {
            self.authorize_check_zone_input(&cause, action, intents)?;
            return Ok(None);
        }
        let batch = self.begin_zone_replacements(AuthorizedZoneBatch {
            cause,
            action,
            intents: intents.to_vec(),
        })?;
        if batch.sources().is_empty() {
            return Ok(None);
        }
        if batch.candidates().len() != intents.len() {
            return Err(CheckError::WrongFrame.into());
        }
        Ok(Some(Box::new(batch)))
    }
    fn check_arrival_groups(
        &self,
        intents: &[ZoneMoveIntent],
        batch: Option<&ZoneReplacementBatch>,
    ) -> Result<Vec<GraveyardGroup>, EffectError> {
        Ok(self
            .zone_arrival_groups(&intents.iter().map(|i| i.before).collect::<Vec<_>>(), batch)?
            .into_iter()
            .map(|g| GraveyardGroup {
                owner: g.owner,
                order: (g.entrants.len() == 1).then(|| g.entrants.clone()),
                entrants: g.entrants,
            })
            .collect())
    }
    pub(super) fn advance_check_moves(&self, moves: &mut CheckMoves) -> Result<bool, EffectError> {
        if let CheckMoveStage::Replacing(batch) = &mut moves.stage {
            if batch.progress(self.revision)?.is_some() {
                return Ok(true);
            }
            let groups = self.check_arrival_groups(&moves.intents, Some(batch))?;
            let CheckMoveStage::Replacing(batch) = std::mem::replace(
                &mut moves.stage,
                CheckMoveStage::Ordering {
                    replacements: None,
                    groups: vec![],
                },
            ) else {
                return Err(CheckError::WrongFrame.into());
            };
            moves.stage = CheckMoveStage::Ordering {
                replacements: Some(batch),
                groups,
            };
        }
        Ok(moves.order_group().is_some())
    }
    pub(super) fn validate_state_actions(
        &self,
        actions: &StateActions,
        event: CheckEventId,
    ) -> Result<(), EffectError> {
        let actual = self.collect_state_actions(event)?;
        if actions.protectors.len() != actual.protectors.len()
            || actions
                .protectors
                .iter()
                .zip(&actual.protectors)
                .any(|(staged, actual)| {
                    staged.battle != actual.battle
                        || staged.controller != actual.controller
                        || staged.domain != actual.domain
                })
            || actions
                .protectors
                .iter()
                .any(|p| p.selected.is_some_and(|seat| !p.domain.contains(&seat)))
            || actions.start_speed != actual.start_speed
            || !actions
                .commanders
                .iter()
                .map(|g| (g.owner, &g.domain))
                .eq(actual.commanders.iter().map(|g| (g.owner, &g.domain)))
            || actions.commanders.iter().any(|g| {
                g.selected
                    .as_ref()
                    .is_some_and(|s| !valid_subset(s, &g.domain))
            })
            || actions.detach != actual.detach
            || actions.dead != actual.dead
            || actions.cancellations != actual.cancellations
            || actions.departed_copies != actual.departed_copies
            || actions.departed_tokens != actual.departed_tokens
            || actions.losses != actual.losses
            || !actions
                .legends
                .iter()
                .map(|s| &s.conflict)
                .eq(actual.legends.iter().map(|s| &s.conflict))
        {
            return Err(CheckError::WrongFrame.into());
        }
        for legend in &actions.legends {
            if legend
                .keep
                .is_some_and(|keep| !legend.conflict.domain.contains(&keep))
            {
                return Err(CheckError::WrongFrame.into());
            }
        }
        Ok(())
    }
    fn authorize_moves(
        &self,
        moves: &CheckMoves,
        intents: Vec<ZoneMoveIntent>,
        cause: EventCause,
        action: ZoneActionKind,
    ) -> Result<(), EffectError> {
        if self.window.revision() != moves.window_revision || moves.intents != intents {
            return Err(CheckError::WrongFrame.into());
        }
        let fresh = self.begin_check_replacements(&intents, cause, action)?;
        match (moves.replacements(), fresh) {
            (None, None) => {}
            (Some(held), Some(initial)) => held.verify_cut(&initial)?,
            _ => return Err(CheckError::WrongFrame.into()),
        }
        if let CheckMoveStage::Ordering { groups, .. } = &moves.stage {
            let actual = self.check_arrival_groups(&intents, moves.replacements())?;
            if !groups
                .iter()
                .map(|g| (g.owner, &g.entrants))
                .eq(actual.iter().map(|g| (g.owner, &g.entrants)))
            {
                return Err(CheckError::WrongFrame.into());
            }
            for group in groups {
                if let Some(order) = &group.order
                    && (order.len() != group.entrants.len()
                        || order.iter().copied().collect::<BTreeSet<_>>()
                            != group.entrants.iter().copied().collect())
                {
                    return Err(CheckError::InvalidGraveyardOrder.into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn authorize_state_movements(
        &self,
        actions: &StateActions,
        movements: &StateMovements,
        process: &CheckProcess,
    ) -> Result<(), EffectError> {
        self.validate_state_actions(actions, process.event)?;
        if self.state_dead_union(actions)? != movements.dead {
            return Err(CheckError::WrongFrame.into());
        }
        self.authorize_moves(
            &movements.moves,
            movements
                .dead
                .iter()
                .map(|(before, reasons)| ZoneMoveIntent {
                    before: *before,
                    reason: ZoneReason::StateBased(*reasons),
                })
                .collect(),
            EventCause::StateBasedActions {
                event: process.event,
                boundary: process.boundary,
            },
            ZoneActionKind::StateBased,
        )
    }
    pub(super) fn authorize_cleanup_moves(
        &self,
        domain: &[ObjectId],
        required: usize,
        selected: &[ObjectId],
        moves: &CheckMoves,
        process: &CheckProcess,
    ) -> Result<(), EffectError> {
        if self.cleanup_hand() != domain
            || self.cleanup_discard_count(domain.len())? != required
            || selected.len() != required
            || !valid_subset(selected, domain)
            || !matches!(process.boundary, CheckBoundary::Cleanup(_))
        {
            return Err(CheckError::WrongFrame.into());
        }
        let player = self.window.active_player();
        self.authorize_moves(
            moves,
            selected
                .iter()
                .map(|before| ZoneMoveIntent {
                    before: *before,
                    reason: ZoneReason::Discard { player },
                })
                .collect(),
            EventCause::CleanupDiscard {
                event: process.event,
                boundary: process.boundary,
            },
            ZoneActionKind::CleanupDiscard,
        )
    }
    pub(super) fn verify_check_moves(&self, moves: &CheckMoves) -> Result<(), EffectError> {
        if let Some(batch) = moves.replacements() {
            let action = batch
                .candidates()
                .values()
                .next()
                .ok_or(CheckError::WrongFrame)?
                .action;
            let fresh = self
                .begin_check_replacements(&moves.intents, batch.cause().clone(), action)?
                .ok_or(CheckError::WrongFrame)?;
            batch.verify_initial(*fresh)?;
        }
        Ok(())
    }
    pub(super) fn check_replacement_frame(
        &self,
    ) -> Result<Option<ZoneReplacementFrame>, EffectError> {
        let process = self.check_process()?;
        let moves = match &process.phase {
            CheckPhase::CleanupMoves {
                domain,
                required,
                selected,
                moves,
            } => {
                self.authorize_cleanup_moves(domain, *required, selected, moves, process)?;
                moves
            }
            CheckPhase::Actions(actions) => {
                let Some(movements) = &actions.movements else {
                    return Ok(None);
                };
                self.authorize_state_movements(actions, movements, process)?;
                &movements.moves
            }
            _ => return Ok(None),
        };
        Ok(match &moves.stage {
            CheckMoveStage::Replacing(batch) => batch.frame(self.revision),
            _ => None,
        })
    }
    /// Resume the same reducer through its real Checks parent and boundary.
    ///
    /// # Errors
    /// Rejects wrong parent/cause/frame/actor/reply or altered frozen work; final
    /// movement, publication and continuation errors return no partial successor.
    pub(in crate::effects) fn resume_check_replacement(
        &self,
        expected: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let process = self.check_process()?;
        self.require_check_boundary(process.boundary, turn)?;
        if self.check_replacement_frame()?.as_ref() != Some(frame) {
            return Err(CheckError::WrongFrame.into());
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ZoneReplacement(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        next.revision = revision;
        let moves = match &mut next.check_process_mut()?.phase {
            CheckPhase::CleanupMoves { moves, .. } => moves,
            CheckPhase::Actions(actions) => {
                &mut actions
                    .movements
                    .as_mut()
                    .ok_or(CheckError::WrongFrame)?
                    .moves
            }
            _ => return Err(CheckError::WrongFrame.into()),
        };
        let CheckMoveStage::Replacing(batch) = &mut moves.stage else {
            return Err(CheckError::WrongFrame.into());
        };
        batch.accept(self.revision, frame, actor, reply)?;
        next.progress_checks(CheckJournal::default(), RulesJournal::default(), turn)
    }
    pub(super) fn commit_cleanup_moves(
        &mut self,
        journal: &mut CheckJournal,
        append: &mut RulesJournal,
        turn: Option<&TurnScheduler>,
    ) -> Result<(), EffectError> {
        let process = self.check_process()?.clone();
        let CheckPhase::CleanupMoves {
            domain,
            required,
            selected,
            moves,
        } = &process.phase
        else {
            return Err(CheckError::WrongFrame.into());
        };
        self.authorize_cleanup_moves(domain, *required, selected, moves, &process)?;
        self.verify_check_moves(moves)?;
        let plan = self.zone_move_plan(&moves.objects(), &moves.orders()?, moves.replacements())?;
        let before = self.rules_snapshot(turn)?;
        let player = self.window.active_player();
        let (window, receipts) =
            self.window
                .record_cleanup_moves(self.window.revision(), player, &plan.application)?;
        let mut by_before = receipts
            .into_iter()
            .map(|r| (r.before.id(), r))
            .collect::<BTreeMap<_, _>>();
        if by_before.len() != selected.len() {
            return Err(CheckError::WrongFrame.into());
        }
        let logical = plan
            .logical
            .iter()
            .map(|id| by_before.remove(id).ok_or(CheckError::WrongFrame))
            .collect::<Result<Vec<_>, _>>()?;
        if !by_before.is_empty() {
            return Err(CheckError::WrongFrame.into());
        }
        self.window = window;
        self.emit_from(
            EventCause::CleanupDiscard {
                event: process.event,
                boundary: process.boundary,
            },
            &before,
            turn,
            append,
            |previous, current, controllers| {
                logical
                    .iter()
                    .map(|movement| {
                        zone_occurrence_at(
                            previous,
                            current,
                            movement,
                            ZoneReason::Discard { player },
                            controllers,
                        )
                        .map(|value| {
                            occurrence_flow::madness_replaced(
                                value,
                                moves
                                    .replacements()
                                    .is_some_and(|b| b.used_madness(movement.before.id())),
                            )
                        })
                    })
                    .collect()
            },
        )?;
        journal
            .moves
            .extend(logical.into_iter().map(|r| CheckMoveRecord {
                event: process.event,
                boundary: process.boundary,
                cause: CheckMoveCause::CleanupDiscard,
                before: r.before,
                after: r.after,
            }));
        self.cleanup_characteristics(turn.ok_or(EffectError::WrongPhase)?, append)?;
        let process = self.check_process_mut()?;
        process.event.pass = process
            .event
            .pass
            .checked_add(1)
            .ok_or(CheckError::IterationExhausted)?;
        process.phase = CheckPhase::Observe;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
