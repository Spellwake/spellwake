//! Atomic departure at stable priority and admitted owned resolution/mana cuts.
//! Suspended-owner, combat and replacement continuations remain separately bounded.
use super::*;
use crate::occurrence::{EventCause, RulesRecord, ZoneReason};

impl EffectWindow {
    pub(crate) fn depart_players(
        &self,
        players: &BTreeSet<SeatId>,
        turn: Option<&TurnScheduler>,
        issued_continuation: Option<&EffectDecision>,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(self.revision)?;
        let resumable_combat = match (&self.pending_work, issued_continuation, turn) {
            (
                Some(PendingWork::Combat(cursor)),
                Some(EffectDecision::Combat(issued)),
                Some(turn),
            ) if cursor.frame == *issued
                && issued.revision() == self.revision
                && self.window.phase() == WindowPhase::TurnWork =>
            {
                match issued {
                    CombatDecision::Attackers(frame) => {
                        cursor.assignments.is_empty()
                            && self.combat.is_none()
                            && frame.chooser() == self.window.turn_owner()
                            && cursor.remaining_actors.is_empty()
                            && turn.stage()
                                == crate::turn::TurnStage::Work(TurnWork::DeclareAttackers)
                    }
                    CombatDecision::Blockers(_) => {
                        cursor.assignments.is_empty()
                            && self.combat.is_some()
                            && turn.stage()
                                == crate::turn::TurnStage::Work(TurnWork::DeclareBlockers)
                    }
                    CombatDecision::DamageAssignments(frame) => {
                        self.combat.is_some()
                            && frame.position() == turn.position()
                            && !frame.position().additional_damage
                            && turn.stage()
                                == crate::turn::TurnStage::Work(TurnWork::CombatDamage {
                                    additional: false,
                                })
                    }
                }
            }
            _ => false,
        };
        // The check owner publishes the entire simultaneous batch before entering
        // departure, and retains the cursor for the following state-action pass.
        let checked = !players.is_empty()
            && players == &self.state_based_losses
            && self.pending_work.is_none()
            && issued_continuation.is_none()
            && matches!(
                self.window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. }) | WindowPhase::TurnWork
            );
        if players.is_empty()
            || players
                .iter()
                .any(|seat| !self.window.participants().contains(seat))
            || self
                .window
                .participants()
                .len()
                .saturating_sub(players.len())
                < 2
            || (!self.state_based_losses.is_empty() && !checked)
            || self.combat.as_ref().is_some_and(|combat| {
                combat
                    .attackers()
                    .any(|a| a.combatant().controller() != self.window.turn_owner())
            })
        {
            return Err(EffectError::UnsupportedDepartureState);
        }
        let admitted = match (&self.pending_work, self.window.phase()) {
            (None, WindowPhase::Priority(PriorityPhase::Ready { .. }) | WindowPhase::Opening) => {
                true
            }
            (
                Some(PendingWork::OptionalEffect(_) | PendingWork::ResolutionPayment(_)),
                WindowPhase::Resolving(_),
            ) => true,
            (Some(PendingWork::Resolution(_)), WindowPhase::Resolving(_)) => {
                match issued_continuation {
                    Some(EffectDecision::CreatureType(frame)) => {
                        self.creature_type_frame()? == *frame
                    }
                    Some(EffectDecision::CopyTargets(frame)) => {
                        self.copy_resolution_frame()? == *frame
                    }
                    _ => false,
                }
            }
            (
                Some(PendingWork::Checks(_)),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. }) | WindowPhase::TurnWork,
            ) => self.check_departure_admitted(turn, issued_continuation)?,
            (Some(PendingWork::ResolutionSelection(_)), WindowPhase::Resolving(_)) => {
                issued_continuation.is_some()
                    && self.selection_frame().as_ref() == issued_continuation
            }
            (Some(PendingWork::LibraryTop(_)), WindowPhase::Resolving(_)) => {
                issued_continuation.is_some()
                    && self.library_top_frame().as_ref() == issued_continuation
            }
            (Some(PendingWork::TokenEntry(_)), WindowPhase::Resolving(_)) => {
                match issued_continuation {
                    Some(EffectDecision::TimestampOrder(frame)) => {
                        self.token_timestamp_frame()? == *frame
                    }
                    _ => false,
                }
            }
            (Some(PendingWork::ManaColor(work)), phase) => matches!(
                (&work.parent, phase),
                (
                    ManaParent::Priority,
                    WindowPhase::Priority(
                        PriorityPhase::Ready { .. } | PriorityPhase::RequiresChecks { .. }
                    )
                ) | (ManaParent::Payment(_), WindowPhase::Resolving(_))
            ),
            _ => false,
        };
        if !admitted && !resumable_combat && !checked {
            return Err(EffectError::UnsupportedDepartureState);
        }
        let before = self.rules_snapshot(turn)?;
        let mut next = self.clone();
        if checked {
            next.state_based_losses.clear();
        }
        if resumable_combat
            && matches!(&self.pending_work, Some(PendingWork::Combat(cursor)) if matches!(cursor.frame, CombatDecision::Attackers(_)))
        {
            // Root drive restarts uncommitted attack selection against the live roster,
            // or continues without an active player if the turn owner departed.
            next.pending_work = None;
        }
        let owned = next.window.remove_departing_owned(players)?;
        next.window = owned.window;
        let owned_objects = owned.objects;
        let mut ceased_entries = owned.entries;
        let mut ceased_copies = owned.copies;
        let ended_control = next.characteristics.end_departed_control(players);
        next.prune_non_battlefield_state();
        let current = next.evaluated_characteristics()?;
        let desired = occurrence_flow::desired_controllers(next.window.cards(), &current)?;
        let survivors = desired
            .into_iter()
            .filter(|(_, player)| {
                self.window.participants().contains(player) && !players.contains(player)
            })
            .collect();
        let controller_changes = if !BTreeMap::is_empty(&survivors) {
            let (window, changes) = next
                .window
                .reconcile_controllers(next.window.revision(), &survivors)?;
            next.window = window;
            changes
        } else {
            vec![]
        };
        let exiles: Vec<_> = next
            .window
            .cards()
            .iter()
            .filter(|object| {
                let controller = if object.zone() == ObjectZone::Battlefield {
                    current
                        .get(object.id())
                        .and_then(|appearance| appearance.controller)
                } else {
                    object.controller()
                };
                controller.is_some_and(|seat| players.contains(&seat))
            })
            .map(GameObject::id)
            .collect();
        // The admitted replacement vocabulary matches specific attempted actions.
        // CR 800.4a exile is none of discard, mill, sacrifice, surveil or an SBA.
        let controlled = next.window.finish_player_departure(players, &exiles)?;
        next.window = controlled.window;
        let movements = controlled.movements;
        ceased_entries.extend(controlled.entries);
        ceased_copies.extend(controlled.copies);
        let discarded_triggers = next.triggers.remove_departed(players);
        next.mana.retain(|(player, _), _| !players.contains(player));
        next.failed_draws.retain(|player| !players.contains(player));
        next.prune_payloads();
        next.revision = revision;
        let mut append = RulesJournal::default();
        let owned_battlefield = owned_objects
            .iter()
            .filter(|object| object.zone() == ObjectZone::Battlefield)
            .map(GameObject::id)
            .collect::<Vec<_>>();
        append.records.push(RulesRecord::PlayersDeparted {
            players: players.clone(),
            owned_objects,
            ended_control,
            ceased_entries,
            ceased_copies,
            discarded_triggers,
        });
        next.emit_from(
            EventCause::PlayerDeparture {
                players: players.clone(),
            },
            &before,
            turn,
            &mut append,
            |before, after, controllers| {
                let mut values = movements
                    .iter()
                    .map(|movement| {
                        occurrence_flow::zone_occurrence_at(
                            before,
                            after,
                            movement,
                            ZoneReason::PlayerDeparture,
                            controllers,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for object in owned_battlefield {
                    values.push(crate::occurrence::RulesOccurrence::ObjectLeftGame {
                        before: Box::new(
                            before
                                .appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                values.extend(
                    controller_changes
                        .into_iter()
                        .map(crate::occurrence::RulesOccurrence::ControllerChanged),
                );
                Ok(values)
            },
        )?;
        // emit_from reconciles combat by object identity, controller, control
        // epoch and creature status, including surviving players' control changes.
        next.refresh_resolution_choice_after_departure(self, turn, &mut append)?;
        Ok(RulesTransition::new(next, (), append))
    }
}
