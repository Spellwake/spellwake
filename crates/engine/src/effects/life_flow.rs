use super::*;
use crate::{
    life::{LifeAdjustment, LifeError},
    occurrence::{EventCause, RulesOccurrence},
};

impl EffectWindow {
    pub(super) fn adjust_life_at_cut(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        adjustment: &LifeAdjustment,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let changes = self
            .life
            .apply_adjustments(&BTreeMap::from([(player, adjustment.clone())]))
            .map_err(|error| match error {
                LifeError::UnknownPlayer
                | LifeError::InvalidAdjustment
                | LifeError::InvalidLoss => EffectError::InconsistentProgram,
            })?;
        if changes.is_empty() {
            return Ok(());
        }
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(changes
                    .into_iter()
                    .map(RulesOccurrence::LifeChanged)
                    .collect())
            },
        )?;
        Ok(())
    }
}
