//! Actual calendar entry, ended-boundary resources and combat occurrence capture.
use super::*;
use crate::{
    effects::occurrence_flow::status_occurrences,
    occurrence::{EventCause, RulesOccurrence, StatusReason},
    turn::{Step, TurnStage},
};

impl EffectWindow {
    /// Adopt one actual scheduler result. Publication does not add owner revisions.
    /// Prepared construction deliberately uses raw enter_turn_step instead.
    pub(crate) fn enter_calendar(
        &self,
        expected: u64,
        previous: Option<&TurnScheduler>,
        next_turn: &TurnScheduler,
        active: SeatId,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        if previous.is_none() && self.window.phase() != WindowPhase::Opening {
            return Err(EffectError::WrongPhase);
        }
        let position = next_turn.position();
        let entered = previous.is_none_or(|turn| turn.position() != position);
        let starts_turn = previous.is_none_or(|turn| turn.position().number != position.number);
        let mut next = self.clone();
        let mut append = RulesJournal::default();
        if let Some(ended) = previous.filter(|_| entered) {
            let players = self.window.apnap_players()?.collect::<Vec<_>>();
            let values = players
                .into_iter()
                .filter_map(|player| {
                    let previous: BTreeMap<_, _> = ManaType::ALL
                        .into_iter()
                        .filter_map(|kind| {
                            let amount = self.mana(player, kind);
                            amount.is_positive().then_some((kind, amount))
                        })
                        .collect();
                    (!previous.is_empty())
                        .then_some(RulesOccurrence::ManaPoolEmptied { player, previous })
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                next.mana.clear();
            } else {
                let before = self.rules_snapshot(Some(ended))?;
                next.mana.clear();
                next.emit_from(
                    EventCause::ManaPoolsEmptied {
                        ended: ended.position(),
                    },
                    &before,
                    Some(ended),
                    &mut append,
                    |_, _, _| Ok(values),
                )?;
            }
        }
        let (state, (), records) = next
            .enter_turn_step(
                next.revision(),
                active,
                next_turn.stage() == TurnStage::Priority,
                false,
            )?
            .into_parts();
        next = state;
        append.append(records);
        if starts_turn {
            next = next.record_turn_start(next.revision())?;
        }
        if entered {
            // The new active/window and actual control-history stamp already exist.
            let before = next.rules_snapshot(Some(next_turn))?;
            next.emit_from(
                EventCause::StepBegan { position },
                &before,
                Some(next_turn),
                &mut append,
                |_, current, _| {
                    Ok(vec![RulesOccurrence::StepBegan {
                        position,
                        active: current.window.active_player(),
                    }])
                },
            )?;
        }
        Ok(RulesTransition::new(next, (), append))
    }

    /// End the actual old combat phase before pool loss or the next step entry.
    pub(crate) fn finish_combat_on_turn(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        if turn.position().step != Step::EndCombat
            || self.window.phase() != WindowPhase::StepEnded
            || self.pending_work.is_some()
        {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let before = self.rules_snapshot(Some(turn))?;
        let previous = self.combat.clone();
        let mut append = RulesJournal::default();
        let mut next = if previous.is_some() {
            let (state, (), records) = self.finish_combat(expected)?.into_parts();
            append.append(records);
            state
        } else {
            // The old root skipped the physical clear when no context existed.
            self.clone()
        };
        next.emit_from(
            EventCause::CombatEnded {
                position: turn.position(),
            },
            &before,
            Some(turn),
            &mut append,
            |_, _, _| Ok(vec![RulesOccurrence::CombatEnded { previous }]),
        )?;
        Ok(RulesTransition::new(next, (), append))
    }

    /// Direct physical-owner unit convenience, without a root calendar context.
    #[cfg(test)]
    pub(crate) fn untap_turn(
        &self,
        expected: u64,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        self.untap_with_context(expected, None)
    }
    pub(crate) fn untap_on_turn(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if turn.position().step != Step::Untap || turn.stage() != TurnStage::Work(TurnWork::Untap) {
            return Err(EffectError::WrongPhase);
        }
        self.untap_with_context(expected, Some(turn))
    }
    fn untap_with_context(
        &self,
        expected: u64,
        turn: Option<&TurnScheduler>,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let before = self.rules_snapshot(turn)?;
        let (window, changes) = self
            .window
            .untap_active_with_changes(self.window.revision())?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        let mut append = RulesJournal::default();
        if !changes.is_empty() {
            next.emit_from(
                EventCause::UntapTurn {
                    active: self.window.turn_owner(),
                },
                &before,
                turn,
                &mut append,
                |_, _, _| Ok(status_occurrences(changes, StatusReason::UntapTurn)),
            )?;
        }
        Ok(RulesTransition::new(next, (), append))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mana::FloatingMana,
        occurrence::{RulesRecord, TriggerError},
        turn::TurnStart,
    };

    fn completed_cleanup(positive: bool) -> (EffectWindow, TurnScheduler, TurnScheduler) {
        let a = SeatId(0);
        let b = SeatId(1);
        let c = SeatId(2);
        let old = TurnScheduler::new(
            TurnStart {
                number: 1,
                step: Step::Cleanup,
            },
            false,
        )
        .unwrap();
        let amount = if positive { 2 } else { 0 };
        let state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![b, c, a],
            c,
            vec![],
            vec![],
            vec![
                FloatingMana {
                    player: b,
                    mana_type: ManaType::White,
                    amount: amount.into(),
                },
                FloatingMana {
                    player: c,
                    mana_type: ManaType::Green,
                    amount: amount.into(),
                },
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Colourless,
                    amount: 0.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        // Disclosed prepared calendar resource unit, then actual cleanup/check completion.
        let state = state
            .enter_turn_step(state.revision(), c, false, false)
            .unwrap()
            .into_state();
        let (
            state,
            RuleResult::Complete {
                completion: RuleCompletion::Checks(completed),
                ..
            },
            _,
        ) = state
            .begin_cleanup(state.revision(), &old)
            .unwrap()
            .into_parts()
        else {
            panic!()
        };
        let next = old
            .complete_work(WorkCompleted::Cleanup {
                priority_required: completed.performed() || completed.waiting_triggers(),
            })
            .unwrap();
        (state, old, next)
    }

    #[test]
    fn zero_only_pool_clear_is_bookkeeping_and_allocates_only_the_real_step_event() {
        let (state, old, next) = completed_cleanup(false);
        let clocks = (
            state.revision(),
            state.window.revision(),
            state.window.cards().revision(),
        );
        let (state, (), append) = state
            .enter_calendar(state.revision(), Some(&old), &next, SeatId(0))
            .unwrap()
            .into_parts();
        assert!(state.mana.is_empty());
        assert_eq!(state.revision(), clocks.0 + 2);
        assert_eq!(state.window.revision(), clocks.1 + 2);
        assert_eq!(state.window.cards().revision(), clocks.2 + 1);
        let [RulesRecord::Event(event)] = append.records.as_slice() else {
            panic!()
        };
        assert!(
            matches!(event.cause, EventCause::StepBegan { position } if position.step == Step::Untap)
        );
        assert_eq!(event.occurrences.len(), 1);
    }

    #[test]
    fn late_step_identity_failure_discards_the_prior_tentative_whole_pool_clear() {
        let (mut state, old, next) = completed_cleanup(true);
        state.triggers.set_test_next_event(u64::MAX - 1);
        let saved = state.clone();
        assert_eq!(
            state.enter_calendar(state.revision(), Some(&old), &next, SeatId(0)),
            Err(EffectError::Trigger(TriggerError::EventIdentityExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(state.window.turn_owner(), SeatId(2));
        assert_eq!(state.mana(SeatId(1), ManaType::White), 2.into());
        assert_eq!(state.mana(SeatId(2), ManaType::Green), 2.into());
    }
}
