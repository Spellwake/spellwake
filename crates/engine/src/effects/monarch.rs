//! The game-wide monarch designation is independent of its creating permanent.
use super::*;

impl EffectWindow {
    /// Current monarch, or `None` before the designation has been created.
    pub fn monarch(&self) -> Option<SeatId> {
        self.characteristics.monarch
    }
    /// Controller and optional new monarch for an inherent waiting monarch trigger.
    pub fn waiting_monarch_trigger(
        &self,
        instance: crate::occurrence::TriggerInstanceId,
    ) -> Option<(SeatId, Option<SeatId>)> {
        self.triggers.monarch_waiting().get(&instance).copied()
    }
    pub(super) fn become_monarch(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !self.window.participants().contains(&player) {
            return Err(EffectError::InconsistentProgram);
        }
        let previous = self.characteristics.monarch;
        if previous == Some(player) {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        self.characteristics.monarch = Some(player);
        self.emit_from(
            crate::occurrence::EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::MonarchChanged {
                    before: previous,
                    after: Some(player),
                }])
            },
        )?;
        let mut returning = Vec::new();
        for (object, controller) in std::mem::take(&mut self.monarch_linked_exiles) {
            if self
                .combat_rules
                .opponents(self.window.participants(), controller)?
                .contains(&player)
            {
                returning.push(object);
            } else {
                self.monarch_linked_exiles.push((object, controller));
            }
        }
        if !returning.is_empty() {
            if let Some(work) = &mut self.linked_return_work {
                if work.origin != LinkedReturnOrigin::Exile {
                    return Err(EffectError::InconsistentProgram);
                }
                work.objects.extend(returning);
                work.orders.clear();
                work.graveyard_orders.clear();
                work.replacements = None;
            } else {
                self.linked_return_work = Some(LinkedReturnWork {
                    library_bottom: None,
                    origin: LinkedReturnOrigin::Exile,
                    wake_entry: None,
                    objects: returning,
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
            }
            self.finish_linked_return_if_ready(append)?;
        }
        Ok(())
    }
}
