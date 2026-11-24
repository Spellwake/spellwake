//! One suspendable supported state-check process and ordinary cleanup sequence.
mod departure;
mod legend;
mod replacements;
use replacements::CheckMoves;
#[cfg(test)]
mod tests;
use super::*;
use crate::{
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
    Actions(Box<StateActions>),
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
    keep: Option<ObjectId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateMovements {
    dead: BTreeMap<ObjectId, StateBasedMoveReasons>,
    moves: CheckMoves,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommanderGroup {
    owner: SeatId,
    domain: Vec<ObjectId>,
    selected: Option<Vec<ObjectId>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtectorChange {
    battle: ObjectId,
    controller: SeatId,
    domain: Vec<SeatId>,
    selected: Option<SeatId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateActions {
    protectors: Vec<ProtectorChange>,
    start_speed: BTreeSet<SeatId>,
    commanders: Vec<CommanderGroup>,
    detach: BTreeSet<ObjectId>,
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
        self.commanders.is_empty() && self.other_actions_empty()
    }
    fn other_actions_empty(&self) -> bool {
        self.start_speed.is_empty()
            && self.protectors.is_empty()
            && self.detach.is_empty()
            && self.dead.is_empty()
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
        let discard = self.cleanup_discard_phase()?;
        let mut next = self.clone();
        next.revision = revision;
        let mut append = RulesJournal::default();
        let phase = match discard {
            Some(phase) => phase,
            None => {
                next.cleanup_characteristics(turn, &mut append)?;
                CheckPhase::Observe
            }
        };
        next.pending_work = Some(PendingWork::Checks(CheckProcess {
            boundary,
            // Slot zero belongs only to cleanup discard, even when none occurs.
            event: CheckEventId {
                started_revision: revision,
                pass: u64::from(phase == CheckPhase::Observe),
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
    fn cleanup_discard_count(&self, cards_in_hand: usize) -> Result<usize, EffectError> {
        let excess = cards_in_hand.saturating_sub(7);
        if excess == 0 {
            return Ok(0);
        }
        let active = self.window.active_player();
        let emblem = self.characteristics.emblems.values().any(|emblem| {
            emblem.controller == active
                && emblem.definition
                    == crate::emblem::EmblemDefinition::UnlimitedHandWithGraveyardReturn
        });
        let temp_no_limit = self.temporary_rules.iter().any(|rule_entry| {
            matches!(
                &rule_entry.rule,
                crate::program::TemporaryRuleKind::TemporaryStatic(ability)
                    if match ability.as_ref() {
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::NoMaximumHandSize,
                            ..
                        } => rule_entry.controller == active,
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::AllPlayersNoMaximumHandSize,
                            ..
                        } => true,
                        _ => false,
                    }
            )
        });
        let no_limit = emblem
            || temp_no_limit
            || self
                .characteristics
                .no_max_hand_size_players
                .contains(&active)
            || self
                .evaluated_characteristics()?
                .objects
                .values()
                .any(|object| {
                    object.zone == ObjectZone::Battlefield
                    && object.has_battlefield_rule(|rule| match rule {
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::NoMaximumHandSize,
                            ..
                        } => object.controller == Some(active),
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule:
                                crate::characteristics::StaticDrawRule::AllPlayersNoMaximumHandSize,
                            ..
                        } => true,
                        _ => false,
                    })
                });
        Ok(if no_limit { 0 } else { excess })
    }
    fn cleanup_hand(&self) -> Vec<ObjectId> {
        let active = self.window.active_player();
        self.window
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Hand(active) && c.card_id().is_some())
            .map(GameObject::id)
            .collect()
    }
    fn cleanup_discard_phase(&self) -> Result<Option<CheckPhase>, EffectError> {
        let domain = self.cleanup_hand();
        let required = self.cleanup_discard_count(domain.len())?;
        if required == 0 {
            return Ok(None);
        }
        let phase = if self.has_cleanup_replacement_source()? {
            CheckPhase::DiscardSelection { domain, required }
        } else {
            CheckPhase::Discard { domain, required }
        };
        Ok(Some(phase))
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
                if self.cleanup_hand() != *domain
                    || self.cleanup_discard_count(domain.len())? != *required
                {
                    return Err(CheckError::WrongFrame.into());
                }
                let frame = CleanupDiscardChoice {
                    player: self.window.active_player(),
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
                for controller in self.window.apnap_players()? {
                    if let Some(change) = actions
                        .protectors
                        .iter()
                        .find(|p| p.controller == controller && p.selected.is_none())
                    {
                        return Ok(CheckDecision::BattleProtector(
                            crate::checks::BattleProtectorChoice {
                                player: controller,
                                revision: self.revision,
                                boundary: process.boundary,
                                event: process.event,
                                battle: change.battle,
                                domain: change.domain.clone(),
                            },
                        ));
                    }
                    if let Some(group) = actions
                        .commanders
                        .iter()
                        .find(|group| group.owner == controller && group.selected.is_none())
                    {
                        return Ok(CheckDecision::ReturnCommanders(
                            crate::checks::CheckObjectDomain {
                                player: group.owner,
                                revision: self.revision,
                                boundary: process.boundary,
                                event: process.event,
                                domain: group.domain.clone(),
                            },
                        ));
                    }
                    if actions
                        .legends
                        .iter()
                        .any(|row| row.conflict.id.controller == controller && row.keep.is_none())
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
                                    s.keep.is_none() && s.conflict.id.controller == controller
                                })
                                .map(|s| s.conflict.clone())
                                .collect(),
                        }));
                    }
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
            (CheckDecision::BattleProtector(frame), DecisionReply::OpponentChoice(protector)) => {
                if !frame.domain().contains(&protector) {
                    return Err(CheckError::InvalidBattleProtector.into());
                }
                CheckChoice::BattleProtector {
                    battle: frame.battle(),
                    protector,
                }
            }
            (CheckDecision::ReturnCommanders(frame), DecisionReply::ReturnCommanders(selected)) => {
                if !valid_subset(&selected, frame.domain()) {
                    return Err(CheckError::InvalidCommanderSelection.into());
                }
                CheckChoice::ReturnCommanders { selected }
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
                next.cleanup_characteristics(turn.ok_or(EffectError::WrongPhase)?, &mut append)?;
                let process = next.check_process_mut()?;
                process.event.pass = process
                    .event
                    .pass
                    .checked_add(1)
                    .ok_or(CheckError::IterationExhausted)?;
                process.phase = CheckPhase::Observe;
            }
            CheckChoice::BattleProtector { battle, protector } => {
                let CheckPhase::Actions(actions) = &mut next.check_process_mut()?.phase else {
                    return Err(EffectError::WrongPhase);
                };
                let change = actions
                    .protectors
                    .iter_mut()
                    .find(|p| p.battle == battle && p.controller == actor && p.selected.is_none())
                    .ok_or(CheckError::WrongFrame)?;
                change.selected = Some(protector);
            }
            CheckChoice::ReturnCommanders { selected } => {
                let CheckPhase::Actions(actions) = &mut next.check_process_mut()?.phase else {
                    return Err(EffectError::WrongPhase);
                };
                let group = actions
                    .commanders
                    .iter_mut()
                    .find(|g| g.owner == actor && g.selected.is_none())
                    .ok_or(CheckError::WrongFrame)?;
                group.selected = Some(selected);
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
                    .find(|s| s.conflict.id == conflict && s.keep.is_none())
                    .ok_or(CheckError::InvalidLegendConflict)?;
                selected.keep = Some(keep);
            }
        }
        next.progress_checks(journal, append, turn)
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
        let active = self.window.active_player();
        let number = turn.position().number;
        let expiring = self
            .characteristics
            .effects()
            .any(|effect| effect.duration.expires_at_cleanup(active, number));
        self.floating_mana.end_turn_effects();
        self.damage_state.prevention_disallowed_this_turn = false;
        self.damage_state.turn_preventions.clear();
        self.damage_state.expire_reflecting_shields();
        self.flash_through_cleanup.clear();
        self.granted_flashback.clear();
        self.creatures_required_to_attack.clear();
        self.damage_exile_until_cleanup.clear();
        self.temporary_rules
            .retain(|rule| !rule.duration.expires_at_cleanup(active, number));
        self.triggers.expire_end_of_turn_watches();
        self.triggers
            .expire_delayed_event_triggers_at_cleanup(active, number);
        self.regeneration_shields.clear();
        if !expiring && marks_removed.is_empty() && self.characteristics.saddled.is_empty() {
            self.damage.clear();
            return Ok(());
        }
        let before = self.rules_snapshot(Some(turn))?;
        self.characteristics.saddled.clear();
        let expired_effects = self.characteristics.expire_at_cleanup(active, number);
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
        let groups =
            self.window
                .apnap_players()?
                .map(|player| {
                    (
                        player,
                        self.triggers
                            .waiting()
                            .iter()
                            .filter(|(_, waiting)| waiting.controller == player)
                            .map(|(id, _)| *id)
                            .chain(
                                self.triggers
                                    .speed_waiting()
                                    .iter()
                                    .filter_map(|(&id, &seat)| (seat == player).then_some(id)),
                            )
                            .chain(
                                self.triggers
                                    .monarch_waiting()
                                    .iter()
                                    .filter_map(|(&id, &(seat, _))| (seat == player).then_some(id)),
                            )
                            .chain(self.triggers.emblem_waiting().iter().filter_map(
                                |(&id, waiting)| (waiting.controller == player).then_some(id),
                            ))
                            .chain(
                                self.triggers.delayed_copy_waiting().iter().filter_map(
                                    |(&id, (seat, _, _))| (*seat == player).then_some(id),
                                ),
                            )
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
        if groups.iter().map(|(_, ids)| ids.len()).sum::<usize>() != self.triggers.waiting_count() {
            return Err(EffectError::InvalidSetup);
        }
        let (triggers, wave) = std::mem::take(&mut self.triggers).start_wave()?;
        *self.triggers = triggers;
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
            || instances
                .iter()
                .any(|id| self.triggers.waiting_controller(*id) != Some(*chooser))
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
        let actual = self
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?;
        let valid_source = if let Some(waiting) = self.triggers.waiting().get(&instance) {
            waiting.controller == *controller
                && matches!(actual.kind(), StackKind::TriggeredAbility { source } if source == &waiting.source.object)
        } else if let Some(waiting) = self.triggers.emblem_waiting().get(&instance) {
            waiting.controller == *controller
                && matches!(actual.kind(), StackKind::Emblem { emblem } if *emblem == waiting.emblem)
        } else if let Some((seat, original, _)) =
            self.triggers.delayed_copy_waiting().get(&instance)
        {
            *seat == *controller
                && matches!(actual.kind(), StackKind::DelayedCopy { original: actual } if actual == original)
        } else {
            false
        };
        if !cohort.contains(&instance)
            || !valid_source
            || actual.id() != entry
            || actual.controller() != *controller
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
    fn waiting_target_body(
        &self,
        id: TriggerInstanceId,
    ) -> Option<(
        &ExecutableBody,
        Option<&crate::occurrence::SupportedAppearance>,
    )> {
        if let Some(waiting) = self.triggers.waiting().get(&id) {
            Some((&waiting.body, Some(&waiting.source)))
        } else {
            self.triggers
                .emblem_waiting()
                .get(&id)
                .map(|waiting| (&waiting.body, None))
        }
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
            .waiting_target_body(placement.instance)
            .is_none_or(|(body, _)| {
                body.target == BodyTarget::None && body.triggered_modes().is_none()
            })
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
    fn trigger_modes_domain(
        &self,
        spec: &crate::program::TriggeredModes,
        controller: SeatId,
        source: Option<&crate::occurrence::SupportedAppearance>,
        ability_id: Option<crate::program::TriggeredAbilityId>,
    ) -> Result<crate::occurrence::TriggerModesChoice, EffectError> {
        let excluded_modes: Option<&BTreeSet<u8>> = match (spec.uniqueness, source, ability_id) {
            (crate::program::ModalUniqueness::OncePerTurn, Some(src), Some(id)) => {
                self.chosen_modal_modes_turn.get(&(src.object.id(), id))
            }
            (crate::program::ModalUniqueness::OncePerGame, Some(src), Some(id)) => {
                self.chosen_modal_modes_ever.get(&(src.object.id(), id))
            }
            _ => None,
        };
        let mut modes = Vec::new();
        for (index, body) in spec.options.iter().enumerate() {
            let mode_idx = u8::try_from(index).map_err(|_| EffectError::InconsistentProgram)?;
            if excluded_modes.is_some_and(|used| used.contains(&mode_idx)) {
                continue;
            }
            let target_required = body.target != BodyTarget::None;
            let bounds = body.permanent_target_bounds();
            let targets = if target_required {
                self.trigger_target_domain(body, controller, source)?
            } else {
                Vec::new()
            };
            if !target_required || targets.len() >= bounds.map_or(1, |(minimum, _)| minimum) {
                modes.push(crate::occurrence::TriggerModeOption {
                    index: mode_idx,
                    multiple: bounds
                        .map(|(minimum, maximum)| (minimum, maximum.min(targets.len()))),
                    target_required,
                    targets,
                });
            }
        }
        let maximum = if spec.allow_repeat {
            if modes.is_empty() { 0 } else { spec.maximum }
        } else {
            spec.maximum
                .min(u8::try_from(modes.len()).map_err(|_| EffectError::InconsistentProgram)?)
        };
        Ok(crate::occurrence::TriggerModesChoice {
            minimum: spec.minimum,
            maximum,
            distinct_targets: spec.distinct_targets,
            allow_repeat: spec.allow_repeat,
            modes,
        })
    }

    fn current_trigger_target(&self) -> Result<TriggerTargetChoice, EffectError> {
        let placement = self
            .pending_trigger_placement()?
            .ok_or(TriggerError::WrongTargetFrame)?;
        self.available_trigger_target(&placement)?
            .ok_or(EffectError::InconsistentProgram)
    }

    fn available_trigger_target(
        &self,
        placement: &TriggerPlacement,
    ) -> Result<Option<TriggerTargetChoice>, EffectError> {
        let process = self.check_process()?;
        let (body, source) = self
            .waiting_target_body(placement.instance)
            .ok_or(EffectError::InconsistentProgram)?;
        let ability_id = self
            .triggers
            .waiting()
            .get(&placement.instance)
            .map(|waiting| waiting.definition);
        let modal = body
            .triggered_modes()
            .map(|spec| self.trigger_modes_domain(spec, placement.controller, source, ability_id))
            .transpose()?;
        let domain = if modal.is_some() {
            Vec::new()
        } else {
            self.trigger_target_domain(body, placement.controller, source)?
        };
        let bounds = body
            .permanent_target_bounds()
            .or_else(|| body.ordered_any_target_bounds());
        if modal
            .as_ref()
            .is_some_and(|modal| modal.maximum < modal.minimum)
            || (modal.is_none()
                && (domain.len() < bounds.map_or(1, |(min, _)| min)
                    || body.target == BodyTarget::None))
        {
            return Ok(None);
        }
        let secondary_targets = if let Some(relation) = body.dependent_targets() {
            let current = self.evaluated_characteristics()?;
            let objects = self.trigger_target_domain(
                &ExecutableBody::new(BodyTarget::SingleCreature, Vec::new()),
                placement.controller,
                source,
            )?;
            Some(
                domain
                    .iter()
                    .map(|&target| {
                        let eligible = objects
                            .iter()
                            .copied()
                            .filter(|&object| {
                                self.legal_dependent_target(relation, target, object, &current)
                            })
                            .collect();
                        (target, eligible)
                    })
                    .collect(),
            )
        } else {
            None
        };
        Ok(Some(TriggerTargetChoice {
            modal,
            secondary_targets,
            bounds,
            ordered_any: body.ordered_any_target_bounds().is_some(),
            divides_three_damage: body
                .instructions
                .contains(&Instruction::DivideThreeDamageAmongTargets),
            revision: self.revision,
            boundary: process.boundary,
            event: process.event,
            wave: placement.wave,
            chooser: placement.controller,
            instance: placement.instance,
            entry: placement.entry,
            domain,
        }))
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
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if let Some(modal) = frame.modal() {
            let DecisionReply::TriggerModes(modes) = reply else {
                return Err(EffectError::WrongDecisionKind);
            };
            return self.place_selected_trigger_modes(frame, revision, modal, &modes, turn);
        }
        let bounds = frame
            .permanent_target_bounds()
            .or_else(|| frame.ordered_any_target_bounds());
        let (minimum, maximum) = bounds.unwrap_or((1, 1));
        let targets = match (reply, bounds) {
            (DecisionReply::TriggerPermanentTargets(targets), Some(_)) => targets,
            (DecisionReply::TriggerTarget(target), None) => vec![target],
            _ => return Err(EffectError::WrongDecisionKind),
        };
        if !(minimum..=maximum).contains(&targets.len()) {
            return Err(EffectError::IllegalTarget);
        }
        let (body, _) = self
            .waiting_target_body(frame.instance())
            .ok_or(EffectError::InconsistentProgram)?;
        let dependent = frame.secondary_targets();
        if let Some(dependent) = dependent {
            let Some((&first, rest)) = targets.split_first() else {
                return Err(EffectError::IllegalTarget);
            };
            if dependent
                .get(&first)
                .is_none_or(|legal| rest.iter().any(|target| !legal.contains(target)))
            {
                return Err(EffectError::IllegalTarget);
            }
        }
        let mut choices = Vec::new();
        for (ordinal, target) in targets.into_iter().enumerate() {
            if dependent.is_none()
                && (!frame.domain().contains(&target)
                    || !self.legal_body_target(body, target, frame.chooser())?)
            {
                return Err(EffectError::IllegalTarget);
            }
            let id = u32::try_from(ordinal).map_err(|_| EffectError::IllegalTarget)?;
            choices.push(TargetChoice {
                id: TargetId(id),
                clause: TargetClauseId(u32::from(dependent.is_some() && ordinal > 0)),
                target,
            });
        }
        let placement = self
            .pending_trigger_placement()?
            .ok_or(EffectError::InconsistentProgram)?;
        let mut next = self.clone();
        next.revision = revision;
        let clause = TargetClause {
            id: TargetClauseId(0),
            minimum,
            maximum,
        };
        next.window = if dependent.is_some() {
            let clauses = vec![
                TargetClause {
                    id: TargetClauseId(0),
                    minimum: 1,
                    maximum: 1,
                },
                TargetClause {
                    id: TargetClauseId(1),
                    minimum: 0,
                    maximum: body
                        .dependent_targets()
                        .ok_or(EffectError::InconsistentProgram)?
                        .maximum(),
                },
            ];
            next.window.bind_trigger_mode_targets(
                next.window.revision(),
                frame.entry(),
                clauses,
                choices.clone(),
            )?
        } else if bounds.is_some() {
            next.window.bind_trigger_target_set(
                next.window.revision(),
                frame.entry(),
                clause,
                choices.clone(),
            )?
        } else {
            next.window.bind_trigger_target(
                next.window.revision(),
                frame.entry(),
                vec![clause],
                choices.clone(),
            )?
        };
        let mut append = RulesJournal::default();
        for target in choices {
            append.records.push(RulesRecord::TriggerTargetSelected {
                placement: placement.clone(),
                target,
            });
        }
        let target_entry = placement.entry;
        next.finish_trigger_placement(placement, None, &mut append)?;
        let before = next.rules_snapshot(turn)?;
        next.emit_from(
            crate::occurrence::EventCause::TriggerTargetsBound {
                entry: target_entry,
            },
            &before,
            turn,
            &mut append,
            |_, _, _| Ok(Vec::new()),
        )?;
        next.progress_checks(CheckJournal::default(), append, turn)
    }
    fn place_selected_trigger_modes(
        &self,
        frame: &TriggerTargetChoice,
        revision: u64,
        modal: &crate::occurrence::TriggerModesChoice,
        modes: &[crate::mana::CastMode],
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let (body, _) = self
            .waiting_target_body(frame.instance())
            .ok_or(EffectError::InconsistentProgram)?;
        let spec = body
            .triggered_modes()
            .ok_or(EffectError::InconsistentProgram)?;
        let bodies = spec.options.iter().collect::<Vec<_>>();
        if modes
            .iter()
            .any(|mode| !modal.modes.iter().any(|offered| offered.index == mode.mode))
        {
            return Err(EffectError::IllegalTarget);
        }
        let (body, selected, choices) = self.compose_modes_with_wrap(
            &bodies,
            modes,
            (usize::from(modal.minimum), usize::from(modal.maximum)),
            spec.allow_repeat,
            spec.prefix.as_deref(),
            spec.suffix.as_deref(),
            |index, _, target| {
                modal
                    .modes
                    .iter()
                    .find(|offered| offered.index == index)
                    .is_some_and(|offered| offered.targets.contains(&target))
            },
        )?;
        if modal.distinct_targets
            && choices
                .iter()
                .map(|choice| choice.target)
                .collect::<BTreeSet<_>>()
                .len()
                != choices.len()
        {
            return Err(EffectError::IllegalTarget);
        }
        let placement = self
            .pending_trigger_placement()?
            .ok_or(EffectError::InconsistentProgram)?;
        let mut next = self.clone();
        next.revision = revision;
        if spec.uniqueness != crate::program::ModalUniqueness::None
            && let Some(waiting) = self.triggers.waiting().get(&placement.instance)
        {
            let key = (waiting.source.object.id(), waiting.definition);
            let map = match spec.uniqueness {
                crate::program::ModalUniqueness::OncePerTurn => &mut next.chosen_modal_modes_turn,
                crate::program::ModalUniqueness::OncePerGame => &mut next.chosen_modal_modes_ever,
                crate::program::ModalUniqueness::None => unreachable!(),
            };
            let used = map.entry(key).or_default();
            for choice in modes {
                used.insert(choice.mode);
            }
        }
        let has_targets = !choices.is_empty();
        if has_targets {
            let clauses = selected
                .iter()
                .filter_map(|mode| mode.clause.clone())
                .collect();
            next.window = next.window.bind_trigger_mode_targets(
                next.window.revision(),
                frame.entry(),
                clauses,
                choices.clone(),
            )?;
        }
        let mut append = RulesJournal::default();
        for target in choices {
            append.records.push(RulesRecord::TriggerTargetSelected {
                placement: placement.clone(),
                target,
            });
        }
        let entry = placement.entry;
        next.finish_trigger_with_modes(placement, None, Some((body, selected)), &mut append)?;
        if has_targets {
            let before = next.rules_snapshot(turn)?;
            next.emit_from(
                EventCause::TriggerTargetsBound { entry },
                &before,
                turn,
                &mut append,
                |_, _, _| Ok(Vec::new()),
            )?;
        }
        next.progress_checks(CheckJournal::default(), append, turn)
    }

    fn finish_trigger_placement(
        &mut self,
        placement: TriggerPlacement,
        removed: Option<TriggerRemovalReason>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.finish_trigger_with_modes(placement, removed, None, append)
    }

    fn finish_trigger_with_modes(
        &mut self,
        placement: TriggerPlacement,
        removed: Option<TriggerRemovalReason>,
        selected: Option<(ExecutableBody, Vec<super::modes::SelectedMode>)>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if let Some((_, _, action)) = self.triggers.take_delayed_copy(placement.instance) {
            if let Some(reason) = removed {
                append
                    .records
                    .push(RulesRecord::TriggerRemoved { placement, reason });
            } else {
                use crate::occurrence::DelayedCopyAction;
                let instructions = match &action {
                    DelayedCopyAction::ReturnExiled(objects) => {
                        vec![Instruction::ReturnDelayedExile(objects.clone())]
                    }
                    DelayedCopyAction::ReturnExiledWithCounters(objects) => {
                        vec![Instruction::ReturnDelayedExileWithCounters(objects.clone())]
                    }
                    DelayedCopyAction::Draw(amount, _) => vec![Instruction::DrawController {
                        amount: ValueExpr::Literal(amount.clone()),
                    }],
                    DelayedCopyAction::Sacrifice(objects) => {
                        vec![Instruction::SacrificeDelayedObjects(objects.clone())]
                    }
                    DelayedCopyAction::Exile(objects) => {
                        vec![Instruction::ExileDelayedObjects(objects.clone())]
                    }
                    DelayedCopyAction::SpellTwice(..) => vec![Instruction::CopyOnCast(
                        crate::program::CastCopyKeyword::CopyObservedSpell,
                    )],
                    DelayedCopyAction::AbilityTwice(..) => vec![
                        Instruction::CopyCapturedAbility,
                        Instruction::CopyCapturedAbility,
                    ],
                    DelayedCopyAction::GrantCastCardAbilities { recipient, donor } => {
                        vec![Instruction::GainCastCardAbilities {
                            recipient: *recipient,
                            donor: donor.clone(),
                        }]
                    }
                };
                let mut payload = StackPayload::from_body(
                    ExecutableBody::new(BodyTarget::None, instructions),
                    None,
                );
                if let StackPayload::Instructions(p) = &mut payload
                    && matches!(
                        action,
                        DelayedCopyAction::Draw(..)
                            | DelayedCopyAction::SpellTwice(..)
                            | DelayedCopyAction::AbilityTwice(..)
                    )
                {
                    p.delayed_copy_action = Some(action);
                }
                self.payloads.insert(placement.entry, payload);
                append.records.push(RulesRecord::TriggerPlaced(placement));
            }
        } else if let Some(waiting) = self.triggers.take_emblem(placement.instance) {
            if let Some(reason) = removed {
                append
                    .records
                    .push(RulesRecord::TriggerRemoved { placement, reason });
            } else {
                let (body, chosen_modes) = if let Some((b, modes)) = selected {
                    (b, modes)
                } else {
                    (waiting.body, Vec::new())
                };
                let mut payload = StackPayload::from_body(body, None);
                if let StackPayload::Instructions(p) = &mut payload {
                    p.modes = chosen_modes;
                    p.bindings.trigger = waiting.trigger;
                }
                self.payloads.insert(placement.entry, payload);
                append.records.push(RulesRecord::TriggerPlaced(placement));
            }
        } else if let Some((_, beneficiary)) = self.triggers.take_monarch(placement.instance) {
            let instruction = match beneficiary {
                Some(player) => Instruction::BecomeMonarch(Some(player)),
                None => Instruction::DrawController {
                    amount: crate::program::ValueExpr::Literal(1.into()),
                },
            };
            self.payloads.insert(
                placement.entry,
                StackPayload::from_body(
                    ExecutableBody::new(BodyTarget::None, vec![instruction]),
                    None,
                ),
            );
            append.records.push(RulesRecord::TriggerPlaced(placement));
        } else if self.triggers.take_speed(placement.instance).is_some() {
            self.payloads.insert(
                placement.entry,
                StackPayload::from_body(
                    ExecutableBody::new(BodyTarget::None, vec![Instruction::IncreaseSpeed]),
                    None,
                ),
            );
            append.records.push(RulesRecord::TriggerPlaced(placement));
        } else {
            let (triggers, mut waiting) =
                std::mem::take(&mut self.triggers).take_waiting(&[placement.instance])?;
            *self.triggers = triggers;
            if waiting.len() != 1 {
                return Err(EffectError::InconsistentProgram);
            }
            let waiting = waiting.pop().ok_or(EffectError::InconsistentProgram)?;
            if let Some(reason) = removed {
                append
                    .records
                    .push(RulesRecord::TriggerRemoved { placement, reason });
            } else {
                self.install_waiting_payload(placement.entry, waiting, selected)?;
                append.records.push(RulesRecord::TriggerPlaced(placement));
            }
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
        if let Some(waiting) = self.triggers.emblem_waiting().get(&instance).cloned() {
            let (window, entry) = self
                .window
                .record_emblem_trigger(waiting.controller, waiting.emblem)?;
            self.window = window;
            let placement = self.placement_metadata(order, position, entry)?;
            append
                .records
                .push(RulesRecord::TriggerPlacementStarted(placement.clone()));
            if waiting.body.triggered_modes().is_none() && waiting.body.target == BodyTarget::None {
                return self.finish_trigger_placement(placement, None, append);
            }
            if waiting.body.triggered_modes().is_none()
                && self
                    .trigger_target_domain(&waiting.body, waiting.controller, None)?
                    .len()
                    < waiting
                        .body
                        .permanent_target_bounds()
                        .or_else(|| waiting.body.ordered_any_target_bounds())
                        .map_or(1, |(min, _)| min)
            {
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
            return Ok(());
        }
        if let Some((controller, original, _)) = self.triggers.delayed_copy_waiting().get(&instance)
        {
            let (window, entry) = self
                .window
                .record_delayed_copy_trigger(*controller, *original)?;
            self.window = window;
            let placement = self.placement_metadata(order, position, entry)?;
            append
                .records
                .push(RulesRecord::TriggerPlacementStarted(placement.clone()));
            return self.finish_trigger_placement(placement, None, append);
        }
        if let Some(&(controller, _)) = self.triggers.monarch_waiting().get(&instance) {
            let (window, entry) = self.window.record_monarch_trigger(controller)?;
            self.window = window;
            let CheckPhase::Placement(cursor) = &self.check_process()?.phase else {
                return Err(EffectError::WrongPhase);
            };
            let placement = TriggerPlacement {
                instance,
                wave: cursor.wave,
                controller,
                position,
                entry,
            };
            append
                .records
                .push(RulesRecord::TriggerPlacementStarted(placement.clone()));
            return self.finish_trigger_placement(placement, None, append);
        }
        if let Some(&controller) = self.triggers.speed_waiting().get(&instance) {
            let (window, entry) = self.window.record_speed_trigger(controller)?;
            self.window = window;
            let CheckPhase::Placement(cursor) = &self.check_process()?.phase else {
                return Err(EffectError::WrongPhase);
            };
            let placement = TriggerPlacement {
                instance,
                wave: cursor.wave,
                controller,
                position,
                entry,
            };
            append
                .records
                .push(RulesRecord::TriggerPlacementStarted(placement.clone()));
            return self.finish_trigger_placement(placement, None, append);
        }
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
        let has_legal_choice = if let Some(spec) = waiting.body.triggered_modes() {
            let domain = self.trigger_modes_domain(
                spec,
                placement.controller,
                Some(&waiting.source),
                Some(waiting.definition),
            )?;
            domain.maximum >= domain.minimum
        } else if waiting.body.target == BodyTarget::None {
            return self.finish_trigger_placement(placement, None, append);
        } else {
            self.trigger_target_domain(&waiting.body, placement.controller, Some(&waiting.source))?
                .len()
                >= waiting
                    .body
                    .permanent_target_bounds()
                    .or_else(|| waiting.body.ordered_any_target_bounds())
                    .map_or(1, |(min, _)| min)
        };
        if !has_legal_choice {
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
    fn chapter_ability_pending(&self, object: ObjectId) -> bool {
        let chapter = |causes: &crate::occurrence::TriggerCauseSet| {
            causes.members().iter().any(|member| {
                matches!(
                    member.related,
                    crate::occurrence::TriggerRelated::SagaChapter { .. }
                )
            })
        };
        self.waiting_triggers().any(|waiting| waiting.source.object.id() == object && chapter(&waiting.causes))
            || self.window.entries().iter().any(|entry|
                matches!(entry.kind(), StackKind::TriggeredAbility { source } if source.id() == object)
                && matches!(self.payloads.get(&entry.id()), Some(StackPayload::Instructions(payload))
                    if payload.bindings.trigger.as_ref().is_some_and(|binding| chapter(&binding.causes))))
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
                if derived.toughness <= 0.into() {
                    return Some((
                        *object,
                        StateBasedMoveReasons::new(StateBasedMoveReason::ZeroToughness),
                    ));
                }
                if characteristics.has_keyword(KeywordAbility::Indestructible) {
                    return None;
                }
                let mut causes = if damage.is_positive()
                    && damage >= current.lethal_damage_threshold(characteristics)?
                {
                    StateBasedMoveReasons::new(StateBasedMoveReason::LethalDestruction)
                } else if self.deathtouch_damage.contains(object) {
                    return Some((
                        *object,
                        StateBasedMoveReasons::new(StateBasedMoveReason::DeathtouchDestruction),
                    ));
                } else {
                    return None;
                };
                if self.deathtouch_damage.contains(object) {
                    causes.insert(StateBasedMoveReason::DeathtouchDestruction);
                }
                Some((*object, causes))
            })
            .collect();
        let mut protectors = Vec::new();
        for (&object, view) in &current.objects {
            if view.zone == ObjectZone::Battlefield
                && view.has_type(CardType::Battle)
                && !self.combat.as_ref().is_some_and(|combat| {
                    combat
                        .attackers()
                        .any(|a| a.defending_planeswalker() == Some(object))
                })
            {
                let controller = view.controller.ok_or(EffectError::InconsistentProgram)?;
                let domain = if self
                    .copiable_program(object)
                    .is_some_and(|p| p.siege_back.is_some())
                {
                    self.opponents_of(controller)?
                } else {
                    vec![controller]
                };
                if !self
                    .battle_protector(object)
                    .is_some_and(|seat| domain.contains(&seat))
                {
                    if domain.is_empty() {
                        dead.entry(object)
                            .and_modify(|reasons| {
                                reasons.insert(StateBasedMoveReason::NoBattleProtector)
                            })
                            .or_insert_with(|| {
                                StateBasedMoveReasons::new(StateBasedMoveReason::NoBattleProtector)
                            });
                    } else {
                        let selected = match domain.as_slice() {
                            [only] => Some(*only),
                            _ => None,
                        };
                        protectors.push(ProtectorChange {
                            battle: object,
                            controller,
                            selected,
                            domain,
                        });
                    }
                }
            }
            if view.zone == ObjectZone::Battlefield
                && let Some(chapter) = view.final_saga_chapter()
                && self
                    .characteristics
                    .lore
                    .get(&object)
                    .is_some_and(|count| *count >= i64::from(chapter).into())
                && !self.chapter_ability_pending(object)
            {
                dead.entry(object)
                    .and_modify(|causes| causes.insert(StateBasedMoveReason::CompletedSaga))
                    .or_insert_with(|| {
                        StateBasedMoveReasons::new(StateBasedMoveReason::CompletedSaga)
                    });
            }
            if view.zone == ObjectZone::Battlefield && view.has_type(CardType::Battle)
                && !self.counters_of_kind(object, CounterKind::Defense).is_positive()
                && (!self.copiable_program(object).is_some_and(|p| p.siege_back.is_some())
                    || !(self.waiting_triggers().any(|waiting| waiting.source.object.id() == object)
                        || self.window.entries().iter().any(|entry| matches!(entry.kind(), StackKind::TriggeredAbility { source } if source.id() == object))))
            {
                dead.entry(object).and_modify(|causes| causes.insert(StateBasedMoveReason::ZeroDefense))
                    .or_insert_with(|| StateBasedMoveReasons::new(StateBasedMoveReason::ZeroDefense));
            }
            if view.zone == ObjectZone::Battlefield
                && view.has_type(CardType::Planeswalker)
                && !self
                    .characteristics
                    .loyalty
                    .get(&object)
                    .is_some_and(RulesInteger::is_positive)
            {
                dead.entry(object)
                    .and_modify(|causes| causes.insert(StateBasedMoveReason::ZeroLoyalty))
                    .or_insert_with(|| {
                        StateBasedMoveReasons::new(StateBasedMoveReason::ZeroLoyalty)
                    });
            }
        }
        for object in self.characteristics.world_eliminations(&current)? {
            dead.entry(object)
                .and_modify(|causes| causes.insert(StateBasedMoveReason::World))
                .or_insert_with(|| StateBasedMoveReasons::new(StateBasedMoveReason::World));
        }
        let mut detach = BTreeSet::new();
        for (&id, object) in &current.objects {
            if object.zone != ObjectZone::Battlefield {
                continue;
            }
            let host = self.attached_to(id);
            if host.is_none() && !object.has_type(CardType::Enchantment) {
                continue;
            }
            if self.attachment_permitted(object, host, &current) {
                continue;
            }
            if self.characteristics.bestowed_auras.contains_key(&id) {
                detach.insert(id);
            } else if object.has_type(CardType::Enchantment)
                && matches!(
                    self.attachment_kind(id),
                    Some(crate::attachment::AttachmentKind::Aura(_))
                )
            {
                dead.entry(id)
                    .and_modify(|causes| causes.insert(StateBasedMoveReason::IllegalAura))
                    .or_insert_with(|| {
                        StateBasedMoveReasons::new(StateBasedMoveReason::IllegalAura)
                    });
            } else if host.is_some() {
                detach.insert(id);
            }
        }
        // CR 704.5y: If a permanent has more than one Role controlled by the same player
        // attached to it, each of those Roles except the one with the most recent timestamp
        // is put into its owner's graveyard.
        let mut roles_by_controller_host: BTreeMap<(SeatId, TargetRef), Vec<ObjectId>> =
            BTreeMap::new();
        for (&id, object) in &current.objects {
            if object.zone == ObjectZone::Battlefield
                && object.has_type(CardType::Enchantment)
                && object
                    .current_enchantment_subtypes
                    .contains(&crate::program::EnchantmentSubtype::Role)
                && let (Some(ctrl), Some(host)) = (object.controller, self.attached_to(id))
            {
                roles_by_controller_host
                    .entry((ctrl, host))
                    .or_default()
                    .push(id);
            }
        }
        for roles in roles_by_controller_host.values() {
            if roles.len() > 1
                && let Some(&newest) = roles.iter().max()
            {
                for &role_id in roles {
                    if role_id != newest {
                        dead.entry(role_id)
                            .and_modify(|causes| causes.insert(StateBasedMoveReason::IllegalAura))
                            .or_insert_with(|| {
                                StateBasedMoveReasons::new(StateBasedMoveReason::IllegalAura)
                            });
                    }
                }
            }
        }
        let mut losses = self.failed_draws.clone();
        losses.extend(self.life.nonpositive_players());
        losses.extend(self.commander_damage_losses().map(|(player, _)| player));
        losses.extend(
            self.window
                .participants()
                .iter()
                .copied()
                .filter(|&player| {
                    self.poison
                        .get(&player)
                        .is_some_and(|count| count >= &10.into())
                }),
        );
        losses.retain(|player| self.window.participants().contains(player));
        let mut by_owner: BTreeMap<SeatId, Vec<ObjectId>> = BTreeMap::new();
        for record in self.commanders.values() {
            if let Some(object) = record
                .return_eligible
                .and_then(|id| self.window.cards().get(id))
            {
                by_owner
                    .entry(object.owner())
                    .or_default()
                    .push(object.id());
            }
        }
        let commanders = self
            .window
            .apnap_players()?
            .filter_map(|owner| {
                by_owner.remove(&owner).map(|domain| CommanderGroup {
                    owner,
                    domain,
                    selected: None,
                })
            })
            .collect();
        Ok(StateActions {
            protectors,
            start_speed: current
                .objects
                .values()
                .filter(|c| {
                    c.zone == ObjectZone::Battlefield
                        && c.has_keyword(KeywordAbility::StartYourEngines)
                })
                .filter_map(|c| c.controller)
                .filter(|p| self.speed(*p) == 0)
                .collect(),
            commanders,
            detach,
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
                .filter(|object| {
                    (object.is_token() && object.zone() != ObjectZone::Battlefield)
                        || (object.is_spell_copy() && object.zone() != ObjectZone::Stack)
                })
                .map(GameObject::id)
                .collect(),
            losses,
            legends: self.collect_legend_conflicts(&current, event)?,
            movements: None,
        })
    }
    pub(super) fn progress_checks(
        mut self,
        mut journal: CheckJournal,
        mut append: RulesJournal,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        loop {
            if let Some(frame) = self.linked_return_decision()? {
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: journal,
                        frame,
                        journal: DamageJournal::default(),
                        declarations: vec![],
                        assignments: vec![],
                        draws: DrawJournal::default(),
                    },
                    append,
                ));
            }
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
                    if self.window.participants().len() != self.window.original_participants().len()
                    {
                        self.triggers.retain_waiting_for(self.window.participants());
                    }
                    let actions = self.collect_state_actions(process.event)?;
                    #[cfg(test)]
                    super::characteristic_observations::checks_observed(
                        &self,
                        process.boundary,
                        process.event,
                        actions.is_empty(),
                    );
                    if actions.is_empty() {
                        self.deathtouch_damage.clear();
                        if process.boundary == CheckBoundary::Priority
                            && !self.ending_turn
                            && self.triggers.waiting_count() != 0
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
                    self.check_process_mut()?.phase = CheckPhase::Actions(Box::new(actions));
                }
                CheckPhase::Actions(actions) => {
                    self.validate_state_actions(&actions, process.event)?;
                    if actions.protectors.iter().any(|p| p.selected.is_none())
                        || actions.commanders.iter().any(|g| g.selected.is_none())
                        || actions.legends.iter().any(|s| s.keep.is_none())
                    {
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
                    let selected: Vec<_> = actions
                        .commanders
                        .iter()
                        .filter_map(|g| g.selected.as_ref())
                        .flatten()
                        .copied()
                        .collect();
                    for record in self.commanders.values_mut() {
                        record.return_eligible = None;
                    }
                    if selected.is_empty() && actions.other_actions_empty() {
                        self.check_process_mut()?.phase = CheckPhase::Observe;
                        continue;
                    }
                    // CR704.8: retain the before-all-components world through publication.
                    let before = self.rules_snapshot(turn)?;
                    let mut occurrences = self.apply_regenerations(moves.replacements())?;
                    for change in &actions.protectors {
                        self.characteristics.battle_protectors.insert(
                            change.battle,
                            change.selected.ok_or(CheckError::WrongFrame)?,
                        );
                    }
                    for &player in &actions.start_speed {
                        self.characteristics.speed.insert(player, 1);
                        occurrences.push(RulesOccurrence::SpeedChanged {
                            player,
                            before: 0,
                            after: 1,
                        });
                    }
                    let mut logical_changes = Vec::new();
                    let logical = plan.logical;
                    let application = plan
                        .application
                        .into_iter()
                        .map(|(id, destination)| {
                            Ok((
                                id,
                                destination
                                    .physical(None)
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, EffectError>>()?;
                    let expected: BTreeSet<_> = movements
                        .dead
                        .keys()
                        .copied()
                        .filter(|id| {
                            !moves
                                .replacements()
                                .is_some_and(|b| b.saved_from_destruction(*id))
                        })
                        .collect();
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
                    let mut commander_changes = Vec::new();
                    if !selected.is_empty() {
                        let (window, changes) = self.window.return_commanders(&selected)?;
                        self.window = window;
                        for change in &changes {
                            journal.moves.push(CheckMoveRecord {
                                event: process.event,
                                boundary: process.boundary,
                                cause: CheckMoveCause::StateBasedAction,
                                before: change.before.clone(),
                                after: change.after.clone(),
                            });
                        }
                        commander_changes = changes;
                    }
                    for &source in &actions.detach {
                        let before = self.characteristics.detach(source);
                        occurrences.push(RulesOccurrence::AttachmentChanged {
                            source,
                            before,
                            after: None,
                        });
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
                            if self
                                .poison
                                .get(&player)
                                .is_some_and(|count| count >= &10.into())
                            {
                                reasons.insert(PlayerLossReason::TenPoisonCounters);
                            }
                            reasons.extend(
                                self.commander_damage_losses()
                                    .filter(|(damaged, _)| *damaged == player)
                                    .map(|(_, card)| PlayerLossReason::CommanderCombatDamage(card)),
                            );
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
                    self.deathtouch_damage.clear();
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
                            for change in &commander_changes {
                                moves.push(zone_occurrence_at(
                                    previous,
                                    current,
                                    change,
                                    ZoneReason::CommanderReturn,
                                    controllers,
                                )?);
                            }
                            moves.append(&mut occurrences);
                            Ok(moves)
                        },
                    )?;
                    if !self.state_based_losses.is_empty() {
                        if self.window.participants().len() - self.state_based_losses.len() <= 1 {
                            return self.complete_checks(process.boundary, true, journal, append);
                        }
                        let departed = std::mem::take(&mut self.state_based_losses);
                        self.apply_player_departures(&departed, turn, &mut append)?;
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
        let waiting_triggers = self.triggers.waiting_count() != 0;
        self.prune_payloads();
        self.pending_work = None;
        if boundary == CheckBoundary::Priority && self.state_based_losses.is_empty() {
            self.window = if self.ending_turn {
                self.window.finish_end_turn_checks()?
            } else {
                self.window.finish_checks(self.window.revision())?
            };
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
