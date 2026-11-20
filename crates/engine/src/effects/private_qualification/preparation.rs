//! Disclosed precheck field preparation, not instruction execution or publication.
use super::*;

impl EffectWindow {
    pub(crate) fn qualification_prepare_context_fields(
        &self,
        life: &[(SeatId, RulesInteger)],
        combat: Option<(ObjectId, SeatId)>,
    ) -> Result<Self, EffectError> {
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let mut next = self.clone();
        for (player, total) in life {
            let old = next.life.get(*player).ok_or(EffectError::InvalidSetup)?;
            let amount = old.minus(total);
            if amount < 0.into() {
                return Err(EffectError::InvalidSetup);
            }
            if amount.is_positive() {
                next.life
                    .apply_losses(&BTreeMap::from([(*player, amount)]))
                    .map_err(|_| EffectError::InvalidSetup)?;
            }
        }
        if let Some((object, defender)) = combat {
            if next.creature(object)?.is_none() || !next.window.participants().contains(&defender) {
                return Err(EffectError::IllegalTarget);
            }
            let combatant = crate::combat::Combatant::from_object(
                next.window
                    .cards()
                    .get(object)
                    .ok_or(EffectError::IllegalTarget)?,
            )
            .ok_or(EffectError::IllegalTarget)?;
            let mut prepared = CombatState::new(vec![(combatant, defender)])?;
            prepared.declare_blocks(defender, vec![])?;
            next.combat = Some(prepared);
        }
        Ok(next)
    }
    pub(crate) fn qualification_controller_successor(
        &self,
        object: ObjectId,
        controller: SeatId,
    ) -> Result<(Self, crate::object_identity::ObjectChange), EffectError> {
        let (window, receipt) = self
            .window
            .qualification_controller_successor(object, controller)?;
        Ok((
            Self {
                window,
                ..self.clone()
            },
            receipt,
        ))
    }

    pub(crate) fn qualification_prune_non_battlefield_state(&self) -> Self {
        let mut next = self.clone();
        next.prune_non_battlefield_state();
        next
    }
    pub(crate) fn qualification_prepare_creature_fields(
        &self,
        counters: &[(ObjectId, PowerToughnessCounters)],
        marked: &[(ObjectId, RulesInteger)],
    ) -> Result<Self, EffectError> {
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let mut next = self.clone();
        for (object, value) in counters {
            next.qualification_prepare_counters(*object, value)?;
        }
        for (object, amount) in marked {
            if !next
                .window
                .cards()
                .get(*object)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                || *amount < 0.into()
            {
                return Err(EffectError::IllegalTarget);
            }
            next.damage.insert(*object, amount.clone());
        }
        Ok(next)
    }
}
