//! Speed is a player designation, independent of the permanents that started it.
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence, TriggerInstanceId},
    stack_window::StackEntry,
};
impl EffectWindow {
    pub(crate) fn enter_speed_turn(&mut self, number: u64, active: SeatId) {
        self.triggers.enter_speed_turn(number, active);
    }
    /// Current speed, including zero before the player's engines have started.
    pub fn speed(&self, player: SeatId) -> u8 {
        self.characteristics
            .speed
            .get(&player)
            .copied()
            .unwrap_or(0)
    }
    /// Owner of a pending source-free speed trigger in the APNAP order-choice domain.
    pub fn waiting_speed_trigger(&self, instance: TriggerInstanceId) -> Option<SeatId> {
        self.triggers.speed_waiting().get(&instance).copied()
    }
    pub(super) fn resolve_speed(
        &mut self,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if entry.kind() != &StackKind::Speed {
            return Err(EffectError::InconsistentProgram);
        }
        let player = entry.controller();
        let initial = self.speed(player);
        if !(1..4).contains(&initial) {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        self.characteristics.speed.insert(player, initial + 1);
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::SpeedChanged {
                    player,
                    before: initial,
                    after: initial + 1,
                }])
            },
        )
    }
}
