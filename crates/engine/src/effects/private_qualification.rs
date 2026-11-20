//! Private qualification adapters; all rule work remains in the actual owners.
use super::*;
use crate::{
    object_identity::ObjectMovement,
    occurrence::{EventCause, ZoneReason},
};
mod resolution_values;
pub(crate) use resolution_values::QualificationResolutionParentFault;

/// Single-field controls on a genuinely reached Checks placement owner.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum QualificationCheckOrderFault {
    DuplicateFutureMember,
    ForeignFutureMember,
    DuplicateFutureController,
    MissingFutureCapture,
    UnallocatedWave,
    UnsupportedStage,
}

/// Single-field structural controls on a genuinely accepted Legend certificate.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum QualificationLegendFault {
    Event,
    Controller,
    Domain,
    Names,
    Keep,
}

/// Structural controls on an actual provisional target placement.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum QualificationCheckTargetFault {
    DuplicateOrder,
    ExtendedOrder,
    PositionPastEnd,
    WrongEntry,
    MissingCapture,
    TargetlessCapture,
    DifferentSource(ObjectId),
}

/// Closed operational fields selected by the input-only private recipes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum QualificationFault {
    Revision(u64),
    WindowRevision(u64),
    PriorityRevision(u64),
    ObjectRevision(u64),
    ObjectIdentity(u64),
    StackEntry(u64),
    Event(u64),
    Instance(u64),
    Wave(u64),
    ContinuousEffect(u64),
    CharacteristicTimestamp(u64),
    WorldTenure(u64),
    Shield(u64),
    Multiplier(u64),
    DamageBatch(u64),
    DamageAttempt(u64),
}

impl EffectWindow {
    /// The caller first clones a genuinely reached source and records the sole
    /// selected field change. No pending frame or gameplay value is rebuilt.
    pub(crate) fn qualification_set_fault(&mut self, fault: QualificationFault) {
        match fault {
            QualificationFault::Revision(value) => self.set_test_revision(value),
            QualificationFault::WindowRevision(value) => self.set_test_window_revision(value),
            QualificationFault::PriorityRevision(value) => self.set_test_priority_revision(value),
            QualificationFault::ObjectRevision(value) => {
                self.set_test_object_allocator(value, false);
            }
            QualificationFault::ObjectIdentity(value) => {
                self.set_test_object_allocator(value, true);
            }
            QualificationFault::StackEntry(value) => self.set_test_stack_next_entry(value),
            QualificationFault::Event(value) => self.triggers.set_test_next_event(value),
            QualificationFault::Instance(value) => self.triggers.set_test_next_instance(value),
            QualificationFault::Wave(value) => self.triggers.set_test_next_wave(value),
            QualificationFault::ContinuousEffect(value) => {
                self.characteristics.qualification_set_next_effect(value);
            }
            QualificationFault::CharacteristicTimestamp(value) => {
                self.characteristics.qualification_set_next_timestamp(value);
            }
            QualificationFault::WorldTenure(value) => self
                .characteristics
                .qualification_set_next_world_tenure(value),
            QualificationFault::Shield(value) => {
                self.damage_state.qualification_set_next_shield(value);
            }
            QualificationFault::Multiplier(value) => {
                self.damage_state.qualification_set_next_multiplier(value);
            }
            QualificationFault::DamageBatch(value) => {
                self.damage_state.qualification_set_next_batch(value);
            }
            QualificationFault::DamageAttempt(value) => {
                self.damage_state.qualification_set_next_attempt(value);
            }
        }
    }

    /// One shared actual before/after world surrounds the complete real batch.
    /// The unit intentionally stops before checks or trigger placement.
    pub(crate) fn qualification_zone_moves(
        &self,
        expected: u64,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<RulesTransition<Self, Vec<ObjectMovement>>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        if moves
            .iter()
            .filter(|(_, destination)| matches!(destination, ObjectDestination::Battlefield { .. }))
            .count()
            > 1
        {
            return Err(CharacteristicsError::UnsupportedTimestampProducer.into());
        }
        let before = self.rules_snapshot(None)?;
        let (window, movements) = self
            .window
            .qualification_zone_moves(self.window.revision(), moves)?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        for movement in &movements {
            if movement.after.zone() == ObjectZone::Battlefield {
                if !matches!(
                    next.base_program(movement.after.id())
                        .map(|program| &program.kind),
                    Some(ProgramKind::Permanent(_))
                ) {
                    return Err(EffectError::InconsistentProgram);
                }
                next.characteristics
                    .register_battlefield_entry(movement.after.id())?;
            }
        }
        let mut append = RulesJournal::default();
        if !movements.is_empty() {
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
                                ZoneReason::MechanismUnit,
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
        }
        Ok(RulesTransition::new(next, movements, append))
    }

    /// Preparation before the first real check, with both physical counter kinds
    /// present together. This is not a counter instruction or a root input.
    pub(crate) fn qualification_prepare_counters(
        &mut self,
        object: ObjectId,
        placement: &PowerToughnessCounters,
    ) -> Result<(), EffectError> {
        if self.pending_work.is_some()
            || !self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        {
            return Err(EffectError::IllegalTarget);
        }
        self.characteristics.place_counters(object, placement)?;
        Ok(())
    }
}

mod matcher;
mod shared_damage;
pub(crate) use shared_damage::QualificationDamageComponent;
mod preparation;
mod tests;

mod contract_cases;

mod related_player_fault;
