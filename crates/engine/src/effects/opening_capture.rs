//! Opening physical operations and cohort occurrence capture.
use super::*;
use crate::{
    object_identity::ObjectMovement,
    occurrence::{EventCause, OpeningMovePhase, OpeningShufflePhase, RulesOccurrence, ZoneReason},
};

impl EffectWindow {
    /// Direct opening-unit convenience with an explicit Initial origin.
    #[cfg(test)]
    pub(crate) fn opening_shuffle(
        &self,
        expected: u64,
        player: SeatId,
        entropy: &EntropyTape,
    ) -> Result<RulesTransition<Self, (EntropyTape, ShuffleRecord)>, EffectError> {
        self.opening_shuffle_with_phase(expected, player, entropy, OpeningShufflePhase::Initial)
    }
    pub(crate) fn opening_shuffle_with_phase(
        &self,
        expected: u64,
        player: SeatId,
        entropy: &EntropyTape,
        phase: OpeningShufflePhase,
    ) -> Result<RulesTransition<Self, (EntropyTape, ShuffleRecord)>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        let (window, entropy, record) =
            self.window
                .opening_shuffle(self.window.revision(), player, entropy)?;
        let before = self.rules_snapshot(None)?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::OpeningShuffle { player, phase },
            &before,
            None,
            &mut append,
            |_, _, _| Ok(vec![RulesOccurrence::Shuffled(record.clone())]),
        )?;
        Ok(RulesTransition::new(next, (entropy, record), append))
    }
    fn opening_bottom_movements(
        &self,
        expected: u64,
        player: SeatId,
        objects: &[ObjectId],
    ) -> Result<RulesTransition<Self, Vec<ObjectMovement>>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        let (window, movements) =
            self.window
                .opening_bottom_with_movements(self.window.revision(), player, objects)?;
        Ok(RulesTransition::new(
            Self {
                revision,
                window,
                ..self.clone()
            },
            movements,
            RulesJournal::default(),
        ))
    }

    /// All real per-player moves share the cohort's complete before/after world.
    /// Each processed player retains its existing executor/window revision advance.
    pub(crate) fn opening_move_cohort(
        &self,
        expected: u64,
        phase: OpeningMovePhase,
        cohort: &[SeatId],
        selected: &[(SeatId, Vec<ObjectId>)],
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        if self.window.phase() != WindowPhase::Opening
            || self.pending_work.is_some()
            || !self.state_based_losses.is_empty()
        {
            return Err(EffectError::WrongPhase);
        }
        let before = self.rules_snapshot(None)?;
        let mut next = self.clone();
        let mut movements = Vec::new();
        let mut append = RulesJournal::default();
        for (player, objects) in selected {
            let (state, records, child) = next
                .opening_bottom_movements(next.revision(), *player, objects)?
                .into_parts();
            next = state;
            movements.extend(records);
            append.append(child);
        }
        if !movements.is_empty() {
            next.emit_from(
                EventCause::OpeningCardsMoved {
                    phase,
                    cohort: cohort.to_vec(),
                },
                &before,
                None,
                &mut append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(
                                previous,
                                current,
                                movement,
                                ZoneReason::Opening(phase),
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
        }
        Ok(RulesTransition::new(next, (), append))
    }

    /// Move chosen opening-hand permanents (e.g. Leylines, CR 103.6) onto the battlefield
    /// before Turn 1 begins, registering their battlefield timestamps.
    pub(crate) fn opening_enter_battlefield(
        &self,
        expected: u64,
        player: SeatId,
        objects: &[ObjectId],
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.window.phase() != WindowPhase::Opening
            || self.pending_work.is_some()
            || !self.state_based_losses.is_empty()
        {
            return Err(EffectError::WrongPhase);
        }
        let before = self.rules_snapshot(None)?;
        let (window, movements) = self.window.opening_battlefield_with_movements(
            self.window.revision(),
            player,
            objects,
        )?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        for movement in &movements {
            next.characteristics
                .register_battlefield_entry(movement.after.id())?;
        }
        let mut append = RulesJournal::default();
        if !movements.is_empty() {
            next.emit_from(
                EventCause::OpeningCardsMoved {
                    phase: OpeningMovePhase::PregameBattlefield,
                    cohort: vec![player],
                },
                &before,
                None,
                &mut append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(
                                previous,
                                current,
                                movement,
                                ZoneReason::Opening(OpeningMovePhase::PregameBattlefield),
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
        }
        Ok(RulesTransition::new(next, (), append))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::occurrence::RulesRecord;

    #[test]
    fn actual_empty_singleton_and_unchanged_shuffles_each_publish_one_event() {
        for size in [0, 1, 8] {
            let a = SeatId(0);
            let state = EffectWindow::new(GameSeed::with_uniform_life(
                vec![a, SeatId(1)],
                a,
                (0..size)
                    .map(|n| SeedCard {
                        card: CardId(n + 1),
                        owner: a,
                        destination: ObjectDestination::LibraryBottom,
                        program: Program::creature(
                            None,
                            1.into(),
                            1.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    })
                    .collect(),
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ))
            .unwrap();
            let state = state.begin_opening(state.revision()).unwrap();
            let tape = EntropyTape::new(if size == 8 {
                vec![7, 6, 5, 4, 3, 2, 1]
            } else {
                vec![]
            });
            let old_registry = state.window.cards().revision();
            let (next, (_, record), append) = state
                .opening_shuffle(state.revision(), a, &tape)
                .unwrap()
                .into_parts();
            assert_eq!(record.before, record.after);
            assert_eq!(next.revision(), state.revision() + 1);
            assert_eq!(next.window.revision(), state.window.revision() + 1);
            assert_eq!(next.window.cards().revision(), old_registry + 1);
            let [RulesRecord::Event(event)] = append.records.as_slice() else {
                panic!()
            };
            assert!(
                matches!(event.cause, EventCause::OpeningShuffle { player, phase: OpeningShufflePhase::Initial } if player == a)
            );
            assert_eq!(event.occurrences.len(), 1);
            assert_eq!(
                event.occurrences[0].value,
                RulesOccurrence::Shuffled(record)
            );
        }
    }
}
