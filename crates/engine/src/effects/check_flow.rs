//! One suspendable supported state-check process and ordinary cleanup sequence.
mod departure;
mod legend;
mod replacements;
use replacements::CheckMoves;
#[cfg(test)]
mod tests;
use super::*;
use crate::{
    characteristics::ResolvedDuration,
    checks::{
        CheckBoundary, CheckChoice, CheckChoiceRecord, CheckEventId, CheckMoveCause,
        CheckMoveRecord, CleanupDiscardChoice, GraveyardOrderChoice, LegendChoice, LegendConflict,
        LegendConflictId,
    },
    object_identity::GameObject,
    occurrence::{
        EventCause, MarkedDamageRemoval, PlacementWaveId, PlayerLossReason, RulesOccurrence,
        RulesRecord, StateBasedMoveReason, StateBasedMoveReasons, TriggerError, TriggerPlacement,
        TriggerRemovalReason, ZoneReason,
    },
    stack_window::OrderedTrigger,
    turn::TurnStage,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckProcess {
    boundary: CheckBoundary,
    event: CheckEventId,
    performed: bool,
    phase: CheckPhase,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckPhase {
    Discard {
        domain: Vec<ObjectId>,
        required: usize,
    },
    DiscardSelection {
        domain: Vec<ObjectId>,
        required: usize,
    },
    CleanupMoves {
        domain: Vec<ObjectId>,
        required: usize,
        selected: Vec<ObjectId>,
        moves: CheckMoves,
    },
    Observe,
    Actions(StateActions),
    Placement(PlacementCursor),
}
/// Frozen cohort references only. WaitingInstance remains the sole body/capture owner.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlacementCursor {
    wave: PlacementWaveId,
    groups: Vec<(SeatId, Vec<TriggerInstanceId>)>,
    player: usize,
    stage: PlacementStage,
}
/// The accepted order contains references; waiting retains the body and causes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PlacementStage {
    Ordering,
    Placing {
        order: Vec<TriggerInstanceId>,
        position: usize,
    },
    Targets {
        order: Vec<TriggerInstanceId>,
        position: usize,
        entry: StackEntryId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct GraveyardGroup {
    owner: SeatId,
    entrants: Vec<ObjectId>,
    order: Option<Vec<ObjectId>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct StagedLegend {
    conflict: LegendConflict,
    accepted: Option<LegendKeepCertificate>,
}
/// Original authenticated choice evidence, retained through canonical cohort reindexing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LegendKeepCertificate {
    conflict: LegendConflict,
    keep: ObjectId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateMovements {
    dead: BTreeMap<ObjectId, StateBasedMoveReasons>,
    moves: CheckMoves,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateActions {
    dead: BTreeMap<ObjectId, StateBasedMoveReasons>,
    cancellations: Vec<(ObjectId, PowerToughnessCounters)>,
    departed_copies: BTreeMap<StackEntryId, crate::stack_window::SpellCopyObject>,
    departed_tokens: BTreeSet<ObjectId>,
    losses: BTreeSet<SeatId>,
    legends: Vec<StagedLegend>,
    movements: Option<StateMovements>,
}
impl StateActions {
    fn is_empty(&self) -> bool {
        self.dead.is_empty()
            && self.legends.is_empty()
            && self.cancellations.is_empty()
            && self.departed_copies.is_empty()
            && self.departed_tokens.is_empty()
            && self.losses.is_empty()
    }
}
impl EffectWindow {
    /// Start supported CR 117.5 checks without granting priority while choices remain.
    pub fn begin_checks(&self, expected: u64) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_check_boundary(CheckBoundary::Priority, None)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let mut next = self.clone();
        next.revision = revision;
        next.pending_work = Some(PendingWork::Checks(CheckProcess {
            boundary: CheckBoundary::Priority,
            event: CheckEventId {
                started_revision: revision,
                pass: 0,
            },
            performed: false,
            phase: CheckPhase::Observe,
        }));
        next.progress_checks(CheckJournal::default(), RulesJournal::default(), None)
    }
    pub(crate) fn begin_cleanup(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let boundary = CheckBoundary::Cleanup(turn.position());
        self.require_check_boundary(boundary, Some(turn))?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let mut next = self.clone();
        next.revision = revision;
        let mut append = RulesJournal::default();
        let phase = if let Some(discard) = self.cleanup_discard_phase()? {
            discard
        } else {
            next.cleanup_characteristics(turn, &mut append)?;
            CheckPhase::Observe
        };
        let discard_skipped = matches!(phase, CheckPhase::Observe);
        next.pending_work = Some(PendingWork::Checks(CheckProcess {
            boundary,
            // Slot zero belongs only to cleanup discard, even when none occurs.
            event: CheckEventId {
                started_revision: revision,
                pass: u64::from(discard_skipped),
            },
            performed: false,
            phase,
        }));
        next.progress_checks(CheckJournal::default(), append, Some(turn))
    }
    fn require_check_boundary(
        &self,
        boundary: CheckBoundary,
        turn: Option<&TurnScheduler>,
    ) -> Result<(), EffectError> {
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        match boundary {
            CheckBoundary::Priority
                if matches!(
                    self.window.phase(),
                    WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
                ) =>
            {
                Ok(())
            }
            CheckBoundary::Cleanup(position)
                if self.window.phase() == WindowPhase::TurnWork
                    && self.window.entries().is_empty()
                    && turn.is_some_and(|t| {
                        t.position() == position && t.stage() == TurnStage::Work(TurnWork::Cleanup)
                    }) =>
            {
                Ok(())
            }
            _ => Err(EffectError::WrongPhase),
        }
    }
    fn cleanup_hand(&self) -> Vec<ObjectId> {
        let Some(active) = self.window.active_player() else {
            return Vec::new();
        };
        self.window
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Hand(active) && c.card_id().is_some())
            .map(GameObject::id)
            .collect()
    }
    fn cleanup_discard_phase(&self) -> Result<Option<CheckPhase>, EffectError> {
        let domain = self.cleanup_hand();
        let required = domain.len().saturating_sub(7);
        if required == 0 {
            return Ok(None);
        }
        Ok(Some(if self.has_cleanup_replacement_source()? {
            CheckPhase::DiscardSelection { domain, required }
        } else {
            CheckPhase::Discard { domain, required }
        }))
    }
    fn check_process(&self) -> Result<&CheckProcess, EffectError> {
        match &self.pending_work {
            Some(PendingWork::Checks(process)) => Ok(process),
            _ => Err(EffectError::WrongPhase),
        }
    }
    fn check_process_mut(&mut self) -> Result<&mut CheckProcess, EffectError> {
        match &mut self.pending_work {
            Some(PendingWork::Checks(process)) => Ok(process),
            _ => Err(EffectError::WrongPhase),
        }
    }
    fn current_check_decision(&self) -> Result<CheckDecision, EffectError> {
        let process = self.check_process()?;
        match &process.phase {
            CheckPhase::Discard { domain, required }
            | CheckPhase::DiscardSelection { domain, required } => {
                if self.cleanup_hand() != *domain || domain.len().saturating_sub(7) != *required {
                    return Err(CheckError::WrongFrame.into());
                }
                let frame = CleanupDiscardChoice {
                    player: self.window.turn_owner(),
                    revision: self.revision,
                    boundary: process.boundary,
                    event: process.event,
                    domain: domain.clone(),
                    required: *required,
                };
                Ok(
                    if matches!(process.phase, CheckPhase::DiscardSelection { .. }) {
                        CheckDecision::CleanupSelect(frame)
                    } else {
                        CheckDecision::CleanupDiscard(frame)
                    },
                )
            }
            CheckPhase::Actions(actions) => {
                self.validate_state_actions(actions, process.event)?;
                if let Some(controller) = actions
                    .legends
                    .iter()
                    .find(|s| s.accepted.is_none())
                    .map(|s| s.conflict.id.controller)
                {
                    return Ok(CheckDecision::Legend(LegendChoice {
                        player: controller,
                        revision: self.revision,
                        boundary: process.boundary,
                        event: process.event,
                        conflicts: actions
                            .legends
                            .iter()
                            .filter(|s| {
                                s.accepted.is_none() && s.conflict.id.controller == controller
                            })
                            .map(|s| s.conflict.clone())
                            .collect(),
                    }));
                }
                let movements = actions.movements.as_ref().ok_or(EffectError::WrongPhase)?;
                self.authorize_state_movements(actions, movements, process)?;
                let group = movements
                    .moves
                    .order_group()
                    .ok_or(EffectError::WrongPhase)?;
                Ok(CheckDecision::GraveyardOrder(GraveyardOrderChoice {
                    player: group.owner,
                    revision: self.revision,
                    boundary: process.boundary,
                    event: process.event,
                    domain: group.entrants.clone(),
                }))
            }
            CheckPhase::CleanupMoves {
                domain,
                required,
                selected,
                moves,
            } => {
                self.authorize_cleanup_moves(domain, *required, selected, moves, process)?;
                let group = moves.order_group().ok_or(EffectError::WrongPhase)?;
                Ok(CheckDecision::GraveyardOrder(GraveyardOrderChoice {
                    player: group.owner,
                    revision: self.revision,
                    boundary: process.boundary,
                    event: process.event,
                    domain: group.entrants.clone(),
                }))
            }
            CheckPhase::Observe | CheckPhase::Placement(_) => Err(EffectError::WrongPhase),
        }
    }
    pub(super) fn resume_checks(
        &self,
        expected: u64,
        frame: &CheckDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_check_boundary(frame.boundary(), turn)?;
        if frame.revision() != self.revision || &self.current_check_decision()? != frame {
            return Err(CheckError::WrongFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let choice = match (frame, reply) {
            (CheckDecision::CleanupSelect(f), DecisionReply::Discard(rows)) => {
                if rows.len() != f.required() || !valid_subset(&rows, f.domain()) {
                    return Err(CheckError::InvalidDiscard.into());
                }
                CheckChoice::CleanupSelected { selected: rows }
            }
            (CheckDecision::CleanupDiscard(f), DecisionReply::Discard(rows)) => {
                if rows.len() != f.required() || !valid_subset(&rows, f.domain()) {
                    return Err(CheckError::InvalidDiscard.into());
                }
                CheckChoice::CleanupDiscard { ordered: rows }
            }
            (CheckDecision::GraveyardOrder(f), DecisionReply::OrderGraveyard(rows)) => {
                if rows.len() != f.domain().len() || !valid_subset(&rows, f.domain()) {
                    return Err(CheckError::InvalidGraveyardOrder.into());
                }
                CheckChoice::GraveyardOrder { ordered: rows }
            }
            (CheckDecision::Legend(f), DecisionReply::KeepLegend { conflict, keep }) => {
                let selected = f
                    .conflicts()
                    .iter()
                    .find(|c| c.id() == conflict)
                    .ok_or(CheckError::InvalidLegendConflict)?;
                if !selected.domain().contains(&keep) {
                    return Err(CheckError::InvalidLegendKeep.into());
                }
                CheckChoice::LegendKeep {
                    conflict,
                    domain: selected.domain().to_vec(),
                    keep,
                }
            }
            _ => return Err(EffectError::WrongDecisionKind),
        };
        let mut next = self.clone();
        next.revision = revision;
        let mut journal = CheckJournal::default();
        let mut append = RulesJournal::default();
        journal.choices.push(CheckChoiceRecord {
            event: frame.event(),
            boundary: frame.boundary(),
            player: actor,
            choice: choice.clone(),
        });
        match choice {
            CheckChoice::CleanupSelected { selected } => {
                let CheckPhase::DiscardSelection { domain, required } =
                    &next.check_process()?.phase
                else {
                    return Err(CheckError::WrongFrame.into());
                };
                let (domain, required) = (domain.clone(), *required);
                let moves = next.new_check_moves(
                    selected
                        .iter()
                        .map(|before| ZoneMoveIntent {
                            before: *before,
                            reason: ZoneReason::Discard { player: actor },
                        })
                        .collect(),
                    EventCause::CleanupDiscard {
                        event: frame.event(),
                        boundary: frame.boundary(),
                    },
                    crate::program::ZoneActionKind::CleanupDiscard,
                )?;
                next.check_process_mut()?.phase = CheckPhase::CleanupMoves {
                    domain,
                    required,
                    selected,
                    moves,
                };
            }
            CheckChoice::CleanupDiscard { ordered } => {
                let before = next.rules_snapshot(turn)?;
                let (window, moves) =
                    next.window
                        .record_cleanup_discards(next.window.revision(), actor, &ordered)?;
                next.window = window;
                next.emit_from(
                    EventCause::CleanupDiscard {
                        event: frame.event(),
                        boundary: frame.boundary(),
                    },
                    &before,
                    turn,
                    &mut append,
                    |previous, current, controllers| {
                        moves
                            .iter()
                            .map(|change| {
                                zone_occurrence_at(
                                    previous,
                                    current,
                                    change,
                                    ZoneReason::Discard { player: actor },
                                    controllers,
                                )
                            })
                            .collect()
                    },
                )?;
                for change in moves {
                    journal.moves.push(CheckMoveRecord {
                        event: frame.event(),
                        boundary: frame.boundary(),
                        cause: CheckMoveCause::CleanupDiscard,
                        before: change.before,
                        after: change.after,
                    });
                }
                next.finish_cleanup_discard(turn.ok_or(EffectError::WrongPhase)?, &mut append)?;
            }
            CheckChoice::GraveyardOrder { ordered } => {
                let moves = match &mut next.check_process_mut()?.phase {
                    CheckPhase::Actions(actions) => {
                        &mut actions
                            .movements
                            .as_mut()
                            .ok_or(EffectError::WrongPhase)?
                            .moves
                    }
                    CheckPhase::CleanupMoves { moves, .. } => moves,
                    _ => return Err(EffectError::WrongPhase),
                };
                moves
                    .order_group_mut()
                    .ok_or(EffectError::WrongPhase)?
                    .order = Some(ordered);
            }
            CheckChoice::LegendKeep { conflict, keep, .. } => {
                let CheckPhase::Actions(actions) = &mut next.check_process_mut()?.phase else {
                    return Err(EffectError::WrongPhase);
                };
                let selected = actions
                    .legends
                    .iter_mut()
                    .find(|s| s.conflict.id == conflict && s.accepted.is_none())
                    .ok_or(CheckError::InvalidLegendConflict)?;
                selected.accepted = Some(LegendKeepCertificate {
                    conflict: selected.conflict.clone(),
                    keep,
                });
            }
        }
        next.progress_checks(journal, append, turn)
    }
    fn finish_cleanup_discard(
        &mut self,
        turn: &TurnScheduler,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.cleanup_characteristics(turn, append)?;
        let process = self.check_process_mut()?;
        process.event.pass = process
            .event
            .pass
            .checked_add(1)
            .ok_or(CheckError::IterationExhausted)?;
        process.phase = CheckPhase::Observe;
        Ok(())
    }
    fn cleanup_characteristics(
        &mut self,
        turn: &TurnScheduler,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let marks_removed = self
            .damage
            .iter()
            .filter(|(object, amount)| {
                **amount != 0.into()
                    && self
                        .window
                        .cards()
                        .get(**object)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            })
            .map(|(object, previous)| MarkedDamageRemoval {
                object: *object,
                previous: previous.clone(),
            })
            .collect::<Vec<_>>();
        let expiring = self
            .characteristics
            .effects()
            .any(|effect| effect.duration == ResolvedDuration::UntilEndOfTurn);
        if !expiring && marks_removed.is_empty() {
            self.damage.clear();
            return Ok(());
        }
        let before = self.rules_snapshot(Some(turn))?;
        let expired_effects = self.characteristics.expire_until_end_of_turn();
        self.damage.clear();
        self.emit_from(
            EventCause::CleanupCharacteristics {
                position: turn.position(),
            },
            &before,
            Some(turn),
            append,
            |previous, current, _| {
                let appearances = characteristic_appearance_changes(
                    previous,
                    current,
                    current
                        .cards
                        .iter()
                        .filter(|card| card.zone() == ObjectZone::Battlefield)
                        .map(GameObject::id),
                )?
                .into_iter()
                .filter(|change| change.before != change.after)
                .collect();
                Ok(vec![RulesOccurrence::CleanupCharacteristics {
                    expired_effects,
                    marks_removed,
                    appearances,
                }])
            },
        )?;
        Ok(())
    }
    fn start_placement(&mut self) -> Result<(), EffectError> {
        let groups = self
            .window
            .apnap_players()?
            .map(|player| {
                (
                    player,
                    self.triggers
                        .waiting()
                        .iter()
                        .filter(|(_, waiting)| waiting.controller == player)
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        if groups.iter().map(|(_, ids)| ids.len()).sum::<usize>() != self.triggers.waiting().len() {
            return Err(EffectError::InvalidSetup);
        }
        let (triggers, wave) = std::mem::take(&mut self.triggers).start_wave()?;
        self.triggers = triggers;
        self.check_process_mut()?.phase = CheckPhase::Placement(PlacementCursor {
            wave,
            groups,
            player: 0,
            stage: PlacementStage::Ordering,
        });
        Ok(())
    }
    fn current_trigger_order(&self) -> Result<TriggerOrder, EffectError> {
        let process = self.check_process()?;
        let CheckPhase::Placement(cursor) = &process.phase else {
            return Err(EffectError::WrongPhase);
        };
        if cursor.stage != PlacementStage::Ordering {
            return Err(TriggerError::WrongOrderFrame.into());
        }
        let (chooser, instances) = cursor
            .groups
            .get(cursor.player)
            .ok_or(EffectError::WrongPhase)?;
        if instances.len() < 2
            || instances.windows(2).any(|ids| ids[0] >= ids[1])
            || instances.iter().any(|id| {
                self.triggers
                    .waiting()
                    .get(id)
                    .is_none_or(|waiting| waiting.controller != *chooser)
            })
        {
            return Err(TriggerError::WrongOrderFrame.into());
        }
        Ok(TriggerOrder {
            revision: self.revision,
            boundary: process.boundary,
            event: process.event,
            wave: cursor.wave,
            chooser: *chooser,
            instances: instances.clone(),
        })
    }
    pub(super) fn resume_trigger_order(
        &self,
        expected: u64,
        frame: &TriggerOrder,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_check_boundary(frame.boundary(), turn)?;
        if frame.revision() != self.revision || &self.current_trigger_order()? != frame {
            return Err(TriggerError::WrongOrderFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::OrderTriggers(ordered) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let unique = ordered.iter().copied().collect::<BTreeSet<_>>();
        if ordered.len() != frame.instances().len()
            || unique.len() != ordered.len()
            || unique.iter().any(|id| !frame.instances().contains(id))
        {
            return Err(TriggerError::InvalidTriggerOrder.into());
        }
        let mut next = self.clone();
        next.revision = revision;
        next.stage_trigger_order(ordered)?;
        next.progress_checks(CheckJournal::default(), RulesJournal::default(), turn)
    }
    fn stage_trigger_order(&mut self, order: Vec<TriggerInstanceId>) -> Result<(), EffectError> {
        let CheckPhase::Placement(cursor) = &mut self.check_process_mut()?.phase else {
            return Err(EffectError::WrongPhase);
        };
        let (_, domain) = cursor
            .groups
            .get(cursor.player)
            .ok_or(EffectError::WrongPhase)?;
        if cursor.stage != PlacementStage::Ordering
            || order.is_empty()
            || order.len() != domain.len()
            || order.iter().copied().collect::<BTreeSet<_>>() != domain.iter().copied().collect()
        {
            return Err(TriggerError::InvalidTriggerOrder.into());
        }
        cursor.stage = PlacementStage::Placing { order, position: 0 };
        Ok(())
    }
    fn placement_metadata(
        &self,
        order: &[TriggerInstanceId],
        position: usize,
        entry: StackEntryId,
    ) -> Result<TriggerPlacement, EffectError> {
        let CheckPhase::Placement(cursor) = &self.check_process()?.phase else {
            return Err(EffectError::WrongPhase);
        };
        let (controller, cohort) = cursor
            .groups
            .get(cursor.player)
            .ok_or(EffectError::InconsistentProgram)?;
        let instance = *order
            .get(position)
            .ok_or(EffectError::InconsistentProgram)?;
        let waiting = self
            .triggers
            .waiting()
            .get(&instance)
            .ok_or(EffectError::InconsistentProgram)?;
        let actual = self
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?;
        if !cohort.contains(&instance)
            || waiting.controller != *controller
            || actual.id() != entry
            || actual.controller() != *controller
            || !matches!(actual.kind(), StackKind::TriggeredAbility { source } if source == &waiting.source.object)
        {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(TriggerPlacement {
            instance,
            wave: cursor.wave,
            controller: *controller,
            position,
            entry,
        })
    }
    /// Privileged metadata for one provisional entry whose payload remains waiting.
    /// Returns None outside a suspended placement-target stage; it does not expose
    /// a completed ability or confer authority to a player.
    ///
    /// # Errors
    /// Returns InconsistentProgram if the current stage, waiting capture or real
    /// unbound top entry disagree, or WrongPhase for a malformed checks process.
    pub fn pending_trigger_placement(&self) -> Result<Option<TriggerPlacement>, EffectError> {
        let Some(PendingWork::Checks(process)) = &self.pending_work else {
            return Ok(None);
        };
        let CheckPhase::Placement(cursor) = &process.phase else {
            return Ok(None);
        };
        let PlacementStage::Targets {
            order,
            position,
            entry,
        } = &cursor.stage
        else {
            return Ok(None);
        };
        let placement = self.placement_metadata(order, *position, *entry)?;
        let actual = self
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?;
        if self
            .triggers
            .waiting()
            .get(&placement.instance)
            .is_none_or(|waiting| waiting.body.target == BodyTarget::None)
            || process.boundary != CheckBoundary::Priority
            || !matches!(
                self.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            )
            || !actual.targets().targets().is_empty()
            || self.payloads.contains_key(entry)
        {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(Some(placement))
    }
    fn current_trigger_target(&self) -> Result<TriggerTargetChoice, EffectError> {
        let placement = self
            .pending_trigger_placement()?
            .ok_or(TriggerError::WrongTargetFrame)?;
        let process = self.check_process()?;
        let waiting = self
            .triggers
            .waiting()
            .get(&placement.instance)
            .ok_or(EffectError::InconsistentProgram)?;
        let domain = self.trigger_target_domain(&waiting.body)?;
        if domain.is_empty() || waiting.body.target == BodyTarget::None {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(TriggerTargetChoice {
            revision: self.revision,
            boundary: process.boundary,
            event: process.event,
            wave: placement.wave,
            chooser: placement.controller,
            instance: placement.instance,
            entry: placement.entry,
            domain,
        })
    }
    pub(super) fn resume_trigger_target(
        &self,
        expected: u64,
        frame: &TriggerTargetChoice,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_check_boundary(frame.boundary(), turn)?;
        if frame.revision() != self.revision || &self.current_trigger_target()? != frame {
            return Err(TriggerError::WrongTargetFrame.into());
        }
        self.validate_check_placement()?;
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::TriggerTarget(target) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if !frame.domain().contains(&target) {
            return Err(EffectError::IllegalTarget);
        }
        let waiting = self
            .triggers
            .waiting()
            .get(&frame.instance())
            .ok_or(EffectError::InconsistentProgram)?;
        if !self.legal_body_target(&waiting.body, target)? {
            return Err(EffectError::IllegalTarget);
        }
        let placement = self
            .pending_trigger_placement()?
            .ok_or(EffectError::InconsistentProgram)?;
        let mut next = self.clone();
        next.revision = revision;
        next.window = next.window.bind_trigger_target(
            next.window.revision(),
            frame.entry(),
            vec![TargetClause {
                id: TargetClauseId(0),
                minimum: 1,
                maximum: 1,
            }],
            vec![TargetChoice {
                id: TargetId(0),
                clause: TargetClauseId(0),
                target,
            }],
        )?;
        let selected = next
            .window
            .entries()
            .last()
            .and_then(|entry| entry.targets().targets().first())
            .ok_or(EffectError::InconsistentProgram)?
            .choice();
        if selected.target != target
            || selected.id != TargetId(0)
            || selected.clause != TargetClauseId(0)
        {
            return Err(EffectError::InconsistentProgram);
        }
        let mut append = RulesJournal::default();
        append.records.push(RulesRecord::TriggerTargetSelected {
            placement: placement.clone(),
            target: selected,
        });
        next.finish_trigger_placement(placement, None, &mut append)?;
        next.progress_checks(CheckJournal::default(), append, turn)
    }
    fn finish_trigger_placement(
        &mut self,
        placement: TriggerPlacement,
        removed: Option<TriggerRemovalReason>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (triggers, mut waiting) =
            std::mem::take(&mut self.triggers).take_waiting(&[placement.instance])?;
        self.triggers = triggers;
        if waiting.len() != 1 {
            return Err(EffectError::InconsistentProgram);
        }
        let waiting = waiting.pop().ok_or(EffectError::InconsistentProgram)?;
        if let Some(reason) = removed {
            append
                .records
                .push(RulesRecord::TriggerRemoved { placement, reason });
        } else {
            self.install_waiting_payload(placement.entry, waiting)?;
            append.records.push(RulesRecord::TriggerPlaced(placement));
        }
        let CheckPhase::Placement(cursor) = &mut self.check_process_mut()?.phase else {
            return Err(EffectError::WrongPhase);
        };
        let stage = std::mem::replace(&mut cursor.stage, PlacementStage::Ordering);
        let (order, position) = match stage {
            PlacementStage::Placing { order, position }
            | PlacementStage::Targets {
                order, position, ..
            } => (order, position),
            PlacementStage::Ordering => return Err(EffectError::InconsistentProgram),
        };
        cursor.stage = PlacementStage::Placing {
            order,
            position: position
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?,
        };
        Ok(())
    }
    fn place_trigger_instance(
        &mut self,
        order: &[TriggerInstanceId],
        position: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let instance = *order
            .get(position)
            .ok_or(EffectError::InconsistentProgram)?;
        let waiting = self
            .triggers
            .waiting()
            .get(&instance)
            .ok_or(TriggerError::InvalidTriggerOrder)?;
        let source = OrderedTrigger {
            controller: waiting.controller,
            source: waiting.source.object.clone(),
        };
        let receipt = self
            .window
            .record_ordered_triggers_with_receipt(self.window.revision(), vec![source])?;
        let (window, entries) = receipt.into_parts();
        let [entry] = entries.as_slice() else {
            return Err(EffectError::InconsistentProgram);
        };
        let entry = *entry;
        self.window = window;
        let placement = self.placement_metadata(order, position, entry)?;
        append
            .records
            .push(RulesRecord::TriggerPlacementStarted(placement.clone()));
        let waiting = self
            .triggers
            .waiting()
            .get(&instance)
            .ok_or(EffectError::InconsistentProgram)?;
        if waiting.body.target == BodyTarget::None {
            return self.finish_trigger_placement(placement, None, append);
        }
        if self.trigger_target_domain(&waiting.body)?.is_empty() {
            self.window = self
                .window
                .remove_unbound_trigger(self.window.revision(), entry)?;
            return self.finish_trigger_placement(
                placement,
                Some(TriggerRemovalReason::NoLegalTargets),
                append,
            );
        }
        let CheckPhase::Placement(cursor) = &mut self.check_process_mut()?.phase else {
            return Err(EffectError::WrongPhase);
        };
        let PlacementStage::Placing { order, position } =
            std::mem::replace(&mut cursor.stage, PlacementStage::Ordering)
        else {
            return Err(EffectError::InconsistentProgram);
        };
        cursor.stage = PlacementStage::Targets {
            order,
            position,
            entry,
        };
        Ok(())
    }
    fn collect_state_actions(&self, event: CheckEventId) -> Result<StateActions, EffectError> {
        let current = self.evaluated_characteristics()?;
        let mut dead: BTreeMap<_, _> = current
            .objects
            .iter()
            .filter_map(|(object, characteristics)| {
                if characteristics.zone != ObjectZone::Battlefield || !characteristics.is_creature()
                {
                    return None;
                }
                let derived = &characteristics.power_toughness.as_ref()?.derived;
                let damage = self.damage.get(object).cloned().unwrap_or_else(|| 0.into());
                let reason = if derived.toughness <= 0.into() {
                    StateBasedMoveReason::ZeroToughness
                } else if damage >= derived.toughness {
                    StateBasedMoveReason::LethalDestruction
                } else {
                    return None;
                };
                Some((*object, StateBasedMoveReasons::new(reason)))
            })
            .collect();
        for object in self.characteristics.world_eliminations(&current)? {
            dead.entry(object)
                .and_modify(|causes| causes.insert(StateBasedMoveReason::World))
                .or_insert_with(|| StateBasedMoveReasons::new(StateBasedMoveReason::World));
        }
        let mut losses = self.failed_draws.clone();
        losses.extend(
            self.life
                .nonpositive_players()
                .filter(|player| self.window.participants().contains(player)),
        );
        Ok(StateActions {
            dead,
            cancellations: self
                .characteristics
                .counter_cancellations()
                .into_iter()
                .filter(|(object, _)| {
                    self.window
                        .cards()
                        .get(*object)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                })
                .collect(),
            departed_copies: self
                .window
                .spell_copies()
                .iter()
                .filter(|(_, c)| c.zone != ObjectZone::Stack)
                .map(|(id, c)| (*id, c.clone()))
                .collect(),
            departed_tokens: self
                .window
                .cards()
                .iter()
                .filter(|object| object.is_token() && object.zone() != ObjectZone::Battlefield)
                .map(GameObject::id)
                .collect(),
            losses,
            legends: self.collect_legend_conflicts(&current, event)?,
            movements: None,
        })
    }
    fn progress_checks(
        mut self,
        mut journal: CheckJournal,
        mut append: RulesJournal,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        loop {
            let process = self.check_process()?.clone();
            match process.phase {
                CheckPhase::Discard { .. } | CheckPhase::DiscardSelection { .. } => {
                    return self.pending_checks(journal, append);
                }
                CheckPhase::CleanupMoves {
                    domain,
                    required,
                    selected,
                    mut moves,
                } => {
                    self.authorize_cleanup_moves(
                        &domain,
                        required,
                        &selected,
                        &moves,
                        self.check_process()?,
                    )?;
                    let pending = self.advance_check_moves(&mut moves)?;
                    self.check_process_mut()?.phase = CheckPhase::CleanupMoves {
                        domain,
                        required,
                        selected,
                        moves,
                    };
                    if pending {
                        return self.pending_checks(journal, append);
                    }
                    self.commit_cleanup_moves(&mut journal, &mut append, turn)?;
                }
                CheckPhase::Observe => {
                    let actions = self.collect_state_actions(process.event)?;
                    #[cfg(test)]
                    super::characteristic_observations::checks_observed(
                        &self,
                        process.boundary,
                        process.event,
                        actions.is_empty(),
                    );
                    if actions.is_empty() {
                        if process.boundary == CheckBoundary::Priority
                            && !self.triggers.waiting().is_empty()
                        {
                            self.start_placement()?;
                            continue;
                        }
                        return self.complete_checks(
                            process.boundary,
                            process.performed,
                            journal,
                            append,
                        );
                    }
                    self.check_process_mut()?.phase = CheckPhase::Actions(actions);
                }
                CheckPhase::Actions(actions) => {
                    self.validate_state_actions(&actions, process.event)?;
                    if actions.legends.iter().any(|s| s.accepted.is_none()) {
                        return self.pending_checks(journal, append);
                    }
                    let Some(movements) = &actions.movements else {
                        let movements = self.determine_state_movements(&actions)?;
                        let CheckPhase::Actions(current) = &mut self.check_process_mut()?.phase
                        else {
                            return Err(EffectError::WrongPhase);
                        };
                        current.movements = Some(movements);
                        continue;
                    };
                    self.authorize_state_movements(&actions, movements, self.check_process()?)?;
                    let mut moves = movements.moves.clone();
                    let pending = self.advance_check_moves(&mut moves)?;
                    let CheckPhase::Actions(current) = &mut self.check_process_mut()?.phase else {
                        return Err(EffectError::WrongPhase);
                    };
                    current
                        .movements
                        .as_mut()
                        .ok_or(EffectError::WrongPhase)?
                        .moves = moves.clone();
                    if pending {
                        return self.pending_checks(journal, append);
                    }
                    self.verify_check_moves(&moves)?;
                    let plan = self.zone_move_plan(
                        &moves.objects(),
                        &moves.orders()?,
                        moves.replacements(),
                    )?;
                    // CR704.8: retain the before-all-components world through publication.
                    let before = self.rules_snapshot(turn)?;
                    let mut occurrences = Vec::new();
                    let mut logical_changes = Vec::new();
                    let logical = plan.logical;
                    let application = plan
                        .application
                        .into_iter()
                        .map(|(id, destination)| (id, destination.physical()))
                        .collect::<Vec<_>>();
                    let expected: BTreeSet<_> = movements.dead.keys().copied().collect();
                    if logical.len() != expected.len()
                        || application.len() != expected.len()
                        || logical.iter().copied().collect::<BTreeSet<_>>() != expected
                        || application
                            .iter()
                            .map(|(id, _)| *id)
                            .collect::<BTreeSet<_>>()
                            != expected
                    {
                        return Err(EffectError::InconsistentProgram);
                    }
                    if !application.is_empty() {
                        let (window, changes) = self
                            .window
                            .record_check_moves(self.window.revision(), application)?;
                        self.window = window;
                        if changes.len() != logical.len() {
                            return Err(EffectError::InconsistentProgram);
                        }
                        let mut by_before: BTreeMap<_, _> =
                            changes.into_iter().map(|m| (m.before.id(), m)).collect();
                        for id in logical {
                            let change = by_before
                                .remove(&id)
                                .ok_or(EffectError::InconsistentProgram)?;
                            journal.moves.push(CheckMoveRecord {
                                event: process.event,
                                boundary: process.boundary,
                                cause: CheckMoveCause::StateBasedAction,
                                before: change.before.clone(),
                                after: change.after.clone(),
                            });
                            logical_changes.push(change);
                        }
                        if !by_before.is_empty() {
                            return Err(EffectError::InconsistentProgram);
                        }
                    }
                    for (object, after) in &actions.cancellations {
                        let previous = before.view().characteristics.counters(*object);
                        occurrences.push(RulesOccurrence::CountersCancelled {
                            object: *object,
                            removed: PowerToughnessCounters {
                                plus_one: previous.plus_one.minus(&after.plus_one),
                                minus_one: previous.minus_one.minus(&after.minus_one),
                            },
                            before: previous,
                            after: after.clone(),
                        });
                    }
                    self.characteristics
                        .apply_counter_cancellations(actions.cancellations);
                    if !actions.departed_copies.is_empty() {
                        self.window = self
                            .window
                            .cease_check_copies(self.window.revision(), &actions.departed_copies)?;
                        occurrences.extend(
                            actions
                                .departed_copies
                                .values()
                                .cloned()
                                .map(|copy| RulesOccurrence::CopyCeased { copy }),
                        );
                    }
                    if !actions.departed_tokens.is_empty() {
                        let (window, ceased) = self
                            .window
                            .cease_tokens(self.window.revision(), &actions.departed_tokens)?;
                        self.window = window;
                        occurrences.extend(
                            ceased
                                .into_iter()
                                .map(|object| RulesOccurrence::TokenCeased { object }),
                        );
                    }
                    for player in self.window.apnap_players()? {
                        if actions.losses.contains(&player) {
                            let mut reasons = BTreeSet::new();
                            if self.failed_draws.contains(&player) {
                                reasons.insert(PlayerLossReason::FailedDraw);
                            }
                            if self.life.get(player).is_some_and(|life| *life <= 0.into()) {
                                reasons.insert(PlayerLossReason::NonpositiveLife);
                            }
                            occurrences.push(RulesOccurrence::PlayerLost { player, reasons });
                        }
                    }
                    self.state_based_losses.extend(actions.losses);
                    self.failed_draws.clear();
                    self.emit_from(
                        EventCause::StateBasedActions {
                            event: process.event,
                            boundary: process.boundary,
                        },
                        &before,
                        turn,
                        &mut append,
                        |previous, current, controllers| {
                            let mut moves = logical_changes
                                .iter()
                                .map(|change| {
                                    zone_occurrence_at(
                                        previous,
                                        current,
                                        change,
                                        ZoneReason::StateBased(
                                            *movements
                                                .dead
                                                .get(&change.before.id())
                                                .ok_or(EffectError::InconsistentProgram)?,
                                        ),
                                        controllers,
                                    )
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            moves.append(&mut occurrences);
                            Ok(moves)
                        },
                    )?;
                    if !self.state_based_losses.is_empty() {
                        let leaving = self.state_based_losses.clone();
                        if self.window.participants().len() - leaving.len() <= 1 {
                            return self.complete_checks(process.boundary, true, journal, append);
                        }
                        let cursor = self.pending_work.take();
                        let (mut survivors, (), departures) =
                            self.depart_players(&leaving, turn, None)?.into_parts();
                        append.append(departures);
                        survivors.pending_work = cursor;
                        self = survivors;
                    }
                    let current = self.check_process_mut()?;
                    current.performed = true;
                    current.event.pass = current
                        .event
                        .pass
                        .checked_add(1)
                        .ok_or(CheckError::IterationExhausted)?;
                    current.phase = CheckPhase::Observe;
                }
                CheckPhase::Placement(cursor) => {
                    let Some((_, instances)) = cursor.groups.get(cursor.player) else {
                        self.check_process_mut()?.phase = CheckPhase::Observe;
                        continue;
                    };
                    match cursor.stage {
                        PlacementStage::Ordering => {
                            if instances.is_empty() {
                                self.advance_placement_player()?;
                            } else if instances.len() == 1 {
                                self.stage_trigger_order(instances.clone())?;
                            } else {
                                return self.pending_checks(journal, append);
                            }
                        }
                        PlacementStage::Placing { order, position } => {
                            if position == order.len() {
                                self.advance_placement_player()?;
                            } else {
                                self.place_trigger_instance(&order, position, &mut append)?;
                            }
                        }
                        PlacementStage::Targets { .. } => {
                            return self.pending_checks(journal, append);
                        }
                    }
                }
            }
        }
    }
    fn advance_placement_player(&mut self) -> Result<(), EffectError> {
        let CheckPhase::Placement(cursor) = &mut self.check_process_mut()?.phase else {
            return Err(EffectError::WrongPhase);
        };
        cursor.player = cursor
            .player
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        cursor.stage = PlacementStage::Ordering;
        Ok(())
    }
    fn pending_checks(
        self,
        checks: CheckJournal,
        append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let frame = if let Some(frame) = self.check_replacement_frame()? {
            EffectDecision::ZoneReplacement(frame)
        } else if let CheckPhase::Placement(cursor) = &self.check_process()?.phase {
            match cursor.stage {
                PlacementStage::Ordering => EffectDecision::Triggers(self.current_trigger_order()?),
                PlacementStage::Targets { .. } => {
                    EffectDecision::TriggerTarget(self.current_trigger_target()?)
                }
                PlacementStage::Placing { .. } => return Err(EffectError::InconsistentProgram),
            }
        } else {
            EffectDecision::Checks(self.current_check_decision()?)
        };
        Ok(RuleProgress::new(
            self,
            RuleResult::Pending {
                frame,
                checks,
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
    fn complete_checks(
        mut self,
        boundary: CheckBoundary,
        performed: bool,
        checks: CheckJournal,
        append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let waiting_triggers = !self.triggers.waiting().is_empty();
        self.prune_payloads();
        self.pending_work = None;
        if boundary == CheckBoundary::Priority && self.state_based_losses.is_empty() {
            self.window = self.window.finish_checks(self.window.revision())?;
        }
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks,
                completion: RuleCompletion::Checks(CheckCompletion {
                    boundary,
                    performed,
                    waiting_triggers,
                }),
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
fn valid_subset(rows: &[ObjectId], domain: &[ObjectId]) -> bool {
    let selected: BTreeSet<_> = rows.iter().copied().collect();
    selected.len() == rows.len() && selected.iter().all(|id| domain.contains(id))
}
