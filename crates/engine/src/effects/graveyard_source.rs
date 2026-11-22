//! Exact-source graveyard returns, distinct from unearth and its delayed effects.
use super::*;
use crate::occurrence::{EventCause, ZoneReason};

impl EffectWindow {
    pub(super) fn return_exact_source_graveyard(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        object: ObjectId,
        tapped: bool,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let (window, exit) = self.window.resolution_move_with_exit(
            self.window.revision(),
            object,
            ObjectDestination::Battlefield {
                controller: entry.controller(),
            },
        )?;
        self.window = window;
        let crate::stack_window::StackExit::Object(mut movement) = exit else {
            return Err(EffectError::InconsistentProgram);
        };
        let entering = movement.after.id();
        self.stage_resolution_entry_choice(object, entering)?;
        self.characteristics.register_battlefield_entry(entering)?;
        if tapped {
            self.window.enter_tapped(entering)?;
        }
        let added =
            self.apply_entry_counters(&before, entering, PowerToughnessCounters::default(), 0)?;
        movement.after = self
            .window
            .cards()
            .get(entering)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            journal,
            |old, current, controllers| {
                let mut rows = vec![zone_occurrence_at(
                    old,
                    current,
                    &movement,
                    ZoneReason::ReturnFromGraveyardToBattlefield,
                    controllers,
                )?];
                if added.valid_placement() {
                    rows.push(crate::occurrence::RulesOccurrence::CountersPlaced {
                        object: entering,
                        before: old.characteristics.counters(entering),
                        added,
                        after: current.characteristics.counters(entering),
                        before_appearance: Box::new(
                            old.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(entering)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )
    }
}
