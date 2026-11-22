//! The returned attacker identifies the defending player for ninjutsu and sneak.
use super::*;
use crate::{
    combat::{BlockStatus, Combatant},
    object_identity::GameObject,
    occurrence::{EventCause, ZoneReason},
    stack_window::StackEntry,
};
impl EffectWindow {
    /// A ninjutsu source in hand stays revealed while an ability it activated is on the stack.
    pub fn revealed_by_ninjutsu(&self, id: ObjectId) -> bool {
        self.window
            .cards()
            .get(id)
            .is_some_and(|c| matches!(c.zone(), ObjectZone::Hand(_)))
            && self.window.entries().iter().any(|entry| {
                entry.attacking_defender().is_some()
                    && matches!(entry.kind(),StackKind::ActivatedAbility{source} if source.id()==id)
            })
    }
    pub(super) fn unblocked_defender(
        &self,
        attacker: ObjectId,
        actor: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<SeatId, EffectError> {
        let current = evaluated
            .get(attacker)
            .ok_or(EffectError::InvalidCastResource)?;
        let attacking = self
            .combat
            .as_ref()
            .and_then(|combat| combat.attacker(attacker))
            .ok_or(EffectError::InvalidCastResource)?;
        if current.zone != ObjectZone::Battlefield
            || !current.is_creature()
            || current.controller != Some(actor)
            || attacking.block_status() != BlockStatus::Unblocked
        {
            return Err(EffectError::InvalidCastResource);
        }
        Ok(attacking.defender())
    }
    pub(super) fn put_attacking(
        &mut self,
        id: ObjectId,
        defender: SeatId,
    ) -> Result<GameObject, EffectError> {
        let object = self.window.enter_tapped(id)?;
        if self
            .evaluated_characteristics()?
            .get(id)
            .is_some_and(|p| p.is_creature() && !p.has_type(CardType::Battle))
            && let Some(combat) = &mut self.combat
        {
            let combatant =
                Combatant::from_object(&object).ok_or(EffectError::InconsistentProgram)?;
            combat.enter_attacking(combatant, defender);
        }
        Ok(object)
    }

    pub(super) fn put_attacking_at(
        &mut self,
        object: ObjectId,
        defender: SeatId,
        target: TargetRef,
    ) -> Result<(), EffectError> {
        self.put_attacking(object, defender)?;
        if let TargetRef::Object(permanent) = target
            && let Some(combat) = &mut self.combat
            && combat.attacker(object).is_some()
        {
            combat.reselect_defender(object, defender, Some(permanent))?;
        }
        Ok(())
    }
    pub(super) fn resolve_ninjutsu(
        &mut self,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let StackKind::ActivatedAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let defender = entry
            .attacking_defender()
            .ok_or(EffectError::InconsistentProgram)?;
        if self
            .window
            .cards()
            .get(source.id())
            .is_none_or(|card| card.zone() != source.zone())
        {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let (window, exit) = self.window.resolution_move_with_exit(
            self.window.revision(),
            source.id(),
            ObjectDestination::Battlefield {
                controller: entry.controller(),
            },
        )?;
        self.window = window;
        let crate::stack_window::StackExit::Object(mut movement) = exit else {
            return Err(EffectError::InconsistentProgram);
        };
        self.stage_resolution_entry_choice(movement.before.id(), movement.after.id())?;
        self.characteristics
            .register_battlefield_entry(movement.after.id())?;
        self.apply_entry_counters(
            &before,
            movement.after.id(),
            PowerToughnessCounters::default(),
            0,
        )?;
        movement.after = self.put_attacking(movement.after.id(), defender)?;
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
                    ZoneReason::NinjutsuEntry,
                    controllers,
                )?])
            },
        )
    }
}
