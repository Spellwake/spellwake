//! The exact permanent produced by a dash, blitz or warp cast owns its delayed effect.
use super::*;
use crate::{
    object_identity::ObjectMovement,
    occurrence::{EventCause, ZoneReason},
    program::EndStepCast,
    stack_window::StackEntry,
};
impl EffectWindow {
    /// Turn on which this exact exiled card was exiled by its warp delayed trigger.
    pub fn warped_on(&self, object: ObjectId) -> Option<u64> {
        self.warped.get(&object).copied()
    }
    pub(super) fn resolve_end_step_cast(
        &mut self,
        entry: &StackEntry,
        index: usize,
        kind: EndStepCast,
        permanent: ObjectId,
        turn: u64,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (destination, reason) = match kind {
            EndStepCast::Dash => (ObjectDestination::Hand, ZoneReason::DashReturn),
            EndStepCast::Warp => (ObjectDestination::Exile, ZoneReason::WarpExile),
            EndStepCast::Blitz => return Err(EffectError::InconsistentProgram), // Sacrifices use the selection and replacement pipeline.
        };
        let Some(source) = self
            .window
            .cards()
            .get(permanent)
            .filter(|o| o.zone() == ObjectZone::Battlefield)
            .cloned()
        else {
            return Ok(());
        };
        let card = source.card_id().ok_or(EffectError::InconsistentProgram)?;
        let before = self.rules_snapshot(None)?;
        self.window =
            self.window
                .record_resolution_move(self.window.revision(), permanent, destination)?;
        let current = self
            .window
            .cards()
            .current_identity(card)
            .and_then(|id| self.window.cards().get(id))
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        if kind == EndStepCast::Warp && current.zone() == ObjectZone::Exile {
            self.warped.insert(current.id(), turn);
        }
        let movement = ObjectMovement {
            before: source,
            after: current,
        };
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            append,
            |old, new, controllers| {
                Ok(vec![zone_occurrence_at(
                    old,
                    new,
                    &movement,
                    reason,
                    controllers,
                )?])
            },
        )
    }
}
