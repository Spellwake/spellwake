//! Resolution-local choices and once-current controlled-permanent counts.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

impl EffectWindow {
    pub(super) fn creature_type_frame(&self) -> Result<CreatureTypeChoice, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        let Instruction::ChooseCreatureType { into, excluding } = self.current_instruction()?
        else {
            return Err(EffectError::WrongCreatureTypeFrame);
        };
        if cursor.entry != entry.id() || cursor.values.contains_key(into) {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(CreatureTypeChoice {
            revision: self.revision,
            entry: entry.id(),
            instruction: cursor.instruction,
            slot: *into,
            chooser: entry.controller(),
            vocabulary: self.creature_vocabulary.id(),
            excluding: excluding.clone(),
        })
    }

    /// The production one-instruction acceptance step. The caller then runs the
    /// existing iterative driver; no priority or checks occur between these steps.
    pub(crate) fn accept_creature_type(
        &self,
        expected: u64,
        bound: &CreatureTypeChoice,
        actor: SeatId,
        selected: CreatureTypeId,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        self.ensure_resolution_or_idle()?;
        let revision = self.next_revision(expected)?;
        if &self.creature_type_frame()? != bound {
            return Err(EffectError::WrongCreatureTypeFrame);
        }
        if actor != bound.chooser {
            return Err(EffectError::WrongChooser);
        }
        if self.creature_vocabulary.label(selected).is_none() || bound.excluding.contains(&selected)
        {
            return Err(EffectError::IllegalCreatureType);
        }
        let before = self.rules_snapshot(None)?;
        let mut next = self.clone();
        next.revision = revision;
        let cursor = next
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongCreatureTypeFrame)?;
        if cursor.values.insert(bound.slot, selected).is_some() {
            return Err(EffectError::InconsistentProgram);
        }
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::Instruction {
                entry: bound.entry,
                instruction: bound.instruction,
            },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::CreatureTypeChosen {
                    entry: bound.entry,
                    instruction: bound.instruction,
                    slot: bound.slot,
                    chooser: actor,
                    value: selected,
                }])
            },
        )?;
        next.advance_instruction()?;
        Ok(RulesTransition::new(next, (), append))
    }

    pub(super) fn resolve_instruction_amount(
        &self,
        controller: SeatId,
        amount: &InstructionAmount,
    ) -> Result<RulesInteger, EffectError> {
        self.resolve_instruction_amount_at(controller, amount, self.resolution_cursor())
    }
    pub(super) fn resolve_instruction_amount_at(
        &self,
        controller: SeatId,
        amount: &InstructionAmount,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<RulesInteger, EffectError> {
        match amount {
            InstructionAmount::Literal(value) => Ok(value.clone()),
            InstructionAmount::MatchedOccurrences => {
                self.resolution_trigger_count_at(controller, cursor)
            }
            InstructionAmount::ControlledPermanentsOfChosenType { chosen } => {
                let selected = cursor
                    .and_then(|cursor| cursor.values.get(chosen))
                    .copied()
                    .ok_or(EffectError::InconsistentProgram)?;
                if self.creature_vocabulary.label(selected).is_none() {
                    return Err(EffectError::InconsistentProgram);
                }
                let evaluated = self.evaluated_characteristics()?;
                let count = self
                    .window
                    .cards()
                    .iter()
                    .filter(|card| {
                        card.zone() == ObjectZone::Battlefield
                            && card.controller() == Some(controller)
                            && evaluated.get(card.id()).is_some_and(|current| {
                                current.current_creature_subtypes.contains(&selected)
                            })
                    })
                    .count();
                Ok(RulesInteger::from_usize(count))
            }
        }
    }
}
