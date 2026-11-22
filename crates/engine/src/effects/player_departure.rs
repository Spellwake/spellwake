//! CR 800.4a cleanup at the cut where a multiplayer loss takes effect.
use super::*;
use crate::occurrence::{EventCause, PlayerLossReason, RulesOccurrence, ZoneReason};

impl EffectWindow {
    pub(crate) fn concede_with_continuation(
        &mut self,
        before_departure: &Self,
        player: SeatId,
        issued: Option<&EffectDecision>,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.has_pending_cost_or_linked()
            || self.has_deferred_payment()
            || self.draw_replacement.is_some()
            || !self.state_based_losses.is_empty()
        {
            return Err(EffectError::WrongPhase);
        }
        let admitted = match (&self.pending_work, self.window.phase()) {
            (
                None,
                WindowPhase::Opening
                | WindowPhase::Priority(PriorityPhase::Ready { .. })
                | WindowPhase::StepEnded,
            ) => true,
            (Some(PendingWork::Checks(_)), _) => self.check_departure_admitted(issued, turn)?,
            (Some(PendingWork::Combat(_)), WindowPhase::TurnWork) => {
                self.combat_declaration_departure_admitted(issued)
            }
            (Some(PendingWork::Damage(_)), _) => self.damage_departure_admitted(issued)?,
            (Some(PendingWork::LibraryTop(_)), WindowPhase::Resolving(_)) => {
                self.library_departure_admitted(issued)?
            }
            (Some(PendingWork::ResolutionSelection(_)), WindowPhase::Resolving(_)) => {
                self.hand_selection_departure_admitted(issued)
            }
            (Some(PendingWork::ManaColor(work)), _) => {
                matches!(&work.parent, ManaParent::Priority | ManaParent::Payment(_))
            }
            (
                Some(
                    PendingWork::Resolution(_)
                    | PendingWork::OptionalEffect(_)
                    | PendingWork::ResolutionPayment(_),
                ),
                WindowPhase::Resolving(_),
            ) => issued.is_some(),
            _ => false,
        };
        if !admitted {
            return Err(EffectError::WrongPhase);
        }
        self.revision = self.next_revision(self.revision)?;
        let before = self.rules_snapshot(turn)?;
        self.emit_from(
            EventCause::PlayerDeparture,
            &before,
            turn,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::PlayerLost {
                    player,
                    reasons: [PlayerLossReason::Conceded].into(),
                }])
            },
        )?;
        self.apply_player_departures(&[player].into(), turn, append)?;
        if matches!(self.pending_work, Some(PendingWork::Checks(_))) {
            self.refresh_checks_after_departure(before_departure, turn, append)?;
        } else if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) {
            self.refresh_library_top_after_departure()?;
        } else if matches!(self.pending_work, Some(PendingWork::ResolutionSelection(_))) {
            self.refresh_hand_selection_after_departure()?;
        } else if matches!(self.pending_work, Some(PendingWork::Damage(_))) {
            self.refresh_damage_after_departure()?;
        }
        self.refresh_resolution_choices_after_departure(append)?;
        Ok(())
    }

    /// Resume the preserved executor owner before the root looks for a fresh priority action.
    pub(crate) fn progress_departure_continuation(
        &self,
        turn: Option<&TurnScheduler>,
    ) -> Result<Option<RuleProgress>, EffectError> {
        let checks = CheckJournal::default();
        let append = RulesJournal::default();
        match &self.pending_work {
            Some(PendingWork::Checks(_)) => {
                self.clone().progress_checks(checks, append, turn).map(Some)
            }
            Some(PendingWork::Combat(_)) => self
                .clone()
                .progress_combat_declaration_after_departure(turn.ok_or(EffectError::WrongPhase)?)
                .map(Some),
            Some(
                PendingWork::Resolution(_)
                | PendingWork::ManaColor(_)
                | PendingWork::LibraryTop(_)
                | PendingWork::ResolutionSelection(_)
                | PendingWork::Damage(_),
            ) => self
                .clone()
                .progress_rules(
                    DamageJournal::default(),
                    Vec::new(),
                    Vec::new(),
                    DrawJournal::default(),
                    append,
                )
                .map(Some),
            _ => Ok(None),
        }
    }

    /// Apply one simultaneous departure with at least two surviving participants.
    /// The enclosing transaction has already emitted the losses while the table was intact.
    pub(super) fn apply_player_departures(
        &mut self,
        players: &BTreeSet<SeatId>,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(turn)?;
        let departed_permanents = self
            .window
            .cards()
            .iter()
            .filter(|object| {
                object.zone() == ObjectZone::Battlefield && players.contains(&object.owner())
            })
            .map(|object| object.id())
            .collect::<Vec<_>>();
        let reveals = self
            .window
            .cards()
            .physical_objects()
            .filter(|object| {
                players.contains(&object.owner())
                    && ((object.face_down().is_some()
                        && matches!(object.zone(), ObjectZone::Battlefield | ObjectZone::Stack))
                        || self.foretold_on(object.id()).is_some())
            })
            .cloned()
            .collect::<Vec<_>>();
        self.window.remove_departed_owners(players)?;
        let spell_control = self.window.end_departed_spell_control(players)?;
        self.characteristics
            .emblems
            .retain(|_, emblem| !players.contains(&emblem.controller));
        self.characteristics.expire_departed_player_control(players);

        let survivors = self
            .window
            .participants()
            .iter()
            .copied()
            .filter(|seat| !players.contains(seat))
            .collect::<Vec<_>>();
        let mut ended_control_effects = Vec::new();
        let evaluated = loop {
            let evaluated = self.evaluate_for_participants(&survivors)?;
            let expired = self
                .characteristics
                .expire_source_control(self.window.cards(), &evaluated);
            if expired.is_empty() {
                break evaluated;
            }
            ended_control_effects.extend(expired);
        };
        let exile = evaluated
            .objects
            .values()
            .filter(|object| {
                let can_exile = object.zone == ObjectZone::Battlefield
                    || (object.zone == ObjectZone::Stack
                        && self
                            .window
                            .cards()
                            .get(object.object)
                            .is_some_and(|card| card.card_id().is_some()));
                can_exile
                    && object
                        .controller
                        .is_some_and(|seat| players.contains(&seat))
            })
            .map(|object| (object.object, ObjectDestination::Exile))
            .collect::<Vec<_>>();
        let exile = self.window.remove_departed_players(players, &exile)?;
        self.prune_payloads();

        let old_monarch = self.characteristics.monarch;
        if old_monarch.is_some_and(|player| players.contains(&player)) {
            let active = self.window.active_player();
            self.characteristics.monarch = Some(match self.window.active_participant() {
                Some(player) => player,
                None => self.window.next_participant_after(active)?,
            });
        }
        let monarch = self.characteristics.monarch;
        self.emit_from(
            EventCause::PlayerDeparture,
            &before,
            turn,
            append,
            |previous, current, controllers| {
                let mut occurrences = reveals
                    .into_iter()
                    .map(|object| RulesOccurrence::FaceDownRevealed { object })
                    .collect::<Vec<_>>();
                for object in departed_permanents {
                    let before = previous
                        .appearance(object)
                        .ok_or(EffectError::InconsistentProgram)?;
                    occurrences.push(RulesOccurrence::PermanentLeftGame {
                        before: Box::new(before),
                    });
                }
                if !ended_control_effects.is_empty() {
                    occurrences.push(RulesOccurrence::SourceControlEffectsEnded {
                        effects: ended_control_effects,
                    });
                }
                occurrences.extend(spell_control.into_iter().map(|(entry, before, after)| {
                    RulesOccurrence::SpellControllerChanged {
                        entry,
                        before,
                        after,
                    }
                }));
                for change in &exile {
                    occurrences.push(zone_occurrence_at(
                        previous,
                        current,
                        change,
                        ZoneReason::PlayerDeparture,
                        controllers,
                    )?);
                }
                if old_monarch != monarch {
                    occurrences.push(RulesOccurrence::MonarchChanged {
                        before: old_monarch,
                        after: monarch,
                    });
                }
                Ok(occurrences)
            },
        )?;
        self.triggers.retain_waiting_for(self.window.participants());
        Ok(())
    }
}
