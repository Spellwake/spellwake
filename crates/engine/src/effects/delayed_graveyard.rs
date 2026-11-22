//! Follow only the graveyard incarnations the originating triggers can identify.
use super::*;
use crate::occurrence::{
    RulesOccurrence, RulesRecord, StateBasedMoveReason, TriggerRelated, ZoneReason,
};
use crate::program::DelayedGraveyardReturn;
use crate::stack_window::StackEntry;

const AURA_RETURN: Instruction = Instruction::ScheduleGraveyardReturnAtEndStep(
    DelayedGraveyardReturn::AuraAfterEnchantedCreatureDies,
);

impl EffectWindow {
    pub(super) fn record_aura_graveyard_returns(&mut self, records: &RulesJournal) {
        for event in records.records.iter().filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        }) {
            for row in &event.occurrences {
                let RulesOccurrence::ZoneChange(movement) = &row.value else {
                    continue;
                };
                let aura = &movement.before.object;
                let graveyard = &movement.after.object;
                if aura.zone() != ObjectZone::Battlefield
                    || graveyard.zone() != ObjectZone::Graveyard(aura.owner())
                    || graveyard.card_id().is_none()
                {
                    continue;
                }
                let unattached = matches!(movement.reason,
                    ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::IllegalAura));
                let waiting = self.triggers.waiting().values().filter(|trigger| {
                    trigger.source.object.id() == aura.id()
                        && trigger.body.instructions.contains(&AURA_RETURN)
                });
                let bound = waiting.into_iter().any(|trigger| {
                    unattached
                        || trigger.causes.members().iter().any(|member| {
                            event
                                .occurrences
                                .iter()
                                .any(|row| row.id == member.occurrence)
                        })
                });
                if bound {
                    self.aura_graveyard_returns
                        .insert(aura.id(), graveyard.id());
                }
            }
        }
    }

    pub(super) fn prune_aura_graveyard_returns(&mut self) {
        if self.aura_graveyard_returns.is_empty() {
            return;
        }
        let waiting = self
            .triggers
            .waiting()
            .values()
            .filter(|trigger| trigger.body.instructions.contains(&AURA_RETURN))
            .map(|trigger| trigger.source.object.id());
        let stacked = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .chain(self.window.checking_targets())
            .filter_map(|entry| {
                let StackKind::TriggeredAbility { source } = entry.kind() else {
                    return None;
                };
                self.stack_body(entry.id())?
                    .instructions
                    .contains(&AURA_RETURN)
                    .then(|| source.id())
            });
        let needed = waiting.chain(stacked).collect::<BTreeSet<_>>();
        self.aura_graveyard_returns
            .retain(|source, _| needed.contains(source));
    }

    pub(super) fn delayed_graveyard_return_player(&self, object: ObjectId) -> Option<SeatId> {
        let card = self.window.cards().get(object)?;
        let ObjectZone::Graveyard(player) = card.zone() else {
            return None;
        };
        (card.card_id().is_some() && self.copiable_program(object)?.permanent().is_some())
            .then_some(player)
    }

    pub(super) fn delayed_transformed_return_player(&self, object: ObjectId) -> Option<SeatId> {
        let player = self.delayed_graveyard_return_player(object)?;
        self.copiable_program(object)?.transform_back.as_ref()?;
        Some(player)
    }

    pub(super) fn schedule_graveyard_return(
        &mut self,
        entry: &StackEntry,
        rule: DelayedGraveyardReturn,
    ) -> Result<(), EffectError> {
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|binding| binding.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = trigger.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TriggerRelated::ZoneChange { after, .. } = &member.related else {
            return Err(EffectError::InconsistentProgram);
        };
        let (object, while_source_remains) = match rule {
            DelayedGraveyardReturn::DyingSourceToHand => {
                self.triggers.delay_graveyard_to_hand(
                    trigger.source.clone(),
                    entry.controller(),
                    after.object.id(),
                );
                return Ok(());
            }
            DelayedGraveyardReturn::DyingSourceTransformed => {
                self.triggers.delay_transformed_return(
                    trigger.source.clone(),
                    entry.controller(),
                    after.object.id(),
                );
                return Ok(());
            }
            DelayedGraveyardReturn::DyingCreatureWhileSourceRemains => (after.object.id(), true),
            DelayedGraveyardReturn::AuraAfterEnchantedCreatureDies => {
                let Some(&graveyard) = self.aura_graveyard_returns.get(&trigger.source.object.id())
                else {
                    return Ok(());
                };
                (graveyard, false)
            }
        };
        if self.delayed_graveyard_return_player(object).is_some() {
            self.triggers.delay_graveyard_return(
                trigger.source.clone(),
                entry.controller(),
                object,
                while_source_remains,
            );
        }
        Ok(())
    }
}
