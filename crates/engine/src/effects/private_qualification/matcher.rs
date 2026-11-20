//! Reviewed trusted matcher mutations, never ordinary player commands or fake frames.
use super::*;
use crate::{object_identity::ObjectStatusChange, occurrence::StatusReason};

impl EffectWindow {
    /// The fixture explicitly orders a discard. The actual Hand(player) check
    /// distinguishes that rule action from an arbitrary primitive hand move.
    pub(crate) fn qualification_discard(
        &self,
        expected: u64,
        player: SeatId,
        object: ObjectId,
    ) -> Result<RulesTransition<Self, Vec<ObjectMovement>>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        if self.window.cards().get(object).map(|card| card.zone()) != Some(ObjectZone::Hand(player))
        {
            return Err(EffectError::IllegalTarget);
        }
        let before = self.rules_snapshot(None)?;
        let (window, movements) = self.window.qualification_zone_moves(
            self.window.revision(),
            &[(object, ObjectDestination::Graveyard)],
        )?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::MechanismUnit,
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
                            ZoneReason::Discard { player },
                            controllers,
                        )
                    })
                    .collect()
            },
        )?;
        Ok(RulesTransition::new(next, movements, append))
    }

    pub(crate) fn qualification_set_tapped(
        &self,
        expected: u64,
        objects: &[ObjectId],
        tapped: bool,
    ) -> Result<RulesTransition<Self, Vec<ObjectStatusChange>>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        let before = self.rules_snapshot(None)?;
        let (window, changes) =
            self.window
                .qualification_set_tapped(self.window.revision(), objects, tapped)?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        let mut append = RulesJournal::default();
        if !changes.is_empty() {
            next.emit_from(
                EventCause::MechanismUnit,
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(crate::effects::occurrence_flow::status_occurrences(
                        changes.clone(),
                        StatusReason::MechanismUnit,
                    ))
                },
            )?;
        }
        Ok(RulesTransition::new(next, changes, append))
    }
}
