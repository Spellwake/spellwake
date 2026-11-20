//! Explicit cfg(test) access to reached choice/parent fixtures, never a restore API.
use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum QualificationResolutionParentFault {
    Entry(StackEntryId),
    Instruction(usize),
}

impl EffectWindow {
    pub(crate) fn creature_type_choice(&self) -> Result<CreatureTypeChoice, EffectError> {
        self.creature_type_frame()
    }

    pub(crate) fn qualification_corrupt_resolution_parent(
        &mut self,
        fault: QualificationResolutionParentFault,
    ) -> Result<(), EffectError> {
        let Some(PendingWork::Damage(DamageWork {
            parent: DamageParent::Resolution(cursor),
            ..
        })) = &mut self.pending_work
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        match fault {
            QualificationResolutionParentFault::Entry(entry) => cursor.entry = entry,
            QualificationResolutionParentFault::Instruction(instruction) => {
                cursor.instruction = instruction
            }
        }
        Ok(())
    }

    pub(crate) fn qualification_resolution_damage_source(
        &self,
    ) -> Result<DamageSource, EffectError> {
        self.resolution_damage_source(self.window.resolving().ok_or(EffectError::WrongPhase)?)
    }

    pub(crate) fn qualification_physical_damage_source(
        &self,
        object: ObjectId,
    ) -> Result<DamageSource, EffectError> {
        self.combat_damage_source(object)
    }

    pub(crate) fn qualification_prepare_empty_combat(&self) -> Result<Self, EffectError> {
        let combat = CombatState::new(vec![])?;
        let mut next = self.clone();
        next.combat = Some(combat);
        Ok(next)
    }
}
