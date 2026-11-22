//! Emblems are independent command-zone objects, not their creating permanents.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

impl EffectWindow {
    pub(super) fn schedule_return_at_next_end_step(
        &mut self,
        entry: StackEntryId,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(entry)
            .and_then(|value| value.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        self.triggers
            .delay_exile_return(binding.source.clone(), controller, objects);
        Ok(())
    }

    pub(crate) fn activate_sacrifice_emblem(
        &self,
        actor: SeatId,
        emblem: crate::emblem::EmblemId,
        creature: ObjectId,
        payment: Option<&ManaPayment>,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        self.ensure_resolution_or_idle()?;
        self.window.validate_action_actor(actor)?;
        if !self
            .characteristics
            .emblems
            .get(&emblem)
            .is_some_and(|value| {
                value.controller == actor
                    && value.definition
                        == crate::emblem::EmblemDefinition::SacrificeCreatureGainLifeDraw
            })
        {
            return Err(EffectError::UnknownActivatedAbility);
        }
        if !self
            .evaluated_characteristics()?
            .get(creature)
            .is_some_and(|card| {
                card.zone == ObjectZone::Battlefield
                    && card.controller == Some(actor)
                    && card.is_creature()
            })
        {
            return Err(EffectError::InvalidCastResource);
        }
        let cost = ManaCost {
            generic: 1.into(),
            required: [(ManaType::Black, 1.into())].into(),
        };
        let spend = self.prepare_mana_payment(&cost, payment, actor)?;
        let (window, entry) =
            self.window
                .record_emblem_activation(self.window.revision(), actor, emblem)?;
        let mut next = self.clone();
        next.revision = self.next_revision(self.revision)?;
        next.window = window;
        let power = ValueExpr::object_stat(
            crate::program::ValueObjectRef::SacrificedCreature,
            crate::program::ObjectStatKind::Power,
        );
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::GainControllerLife {
                    amount: power.clone(),
                },
                Instruction::DrawController { amount: power },
            ],
        );
        next.payloads
            .insert(entry, StackPayload::from_body(body, None));
        let action = crate::program::ZoneActionKind::SacrificeCost;
        let batch = next.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::ActivatedAbilityCost {
                entry,
                ability: ActivatedAbilityId(0),
            },
            action,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: creature,
                reason: crate::occurrence::ZoneReason::SacrificeCost,
            }],
        })?;
        next.pending_work = Some(PendingWork::SelectedActivationCost(Box::new(
            selected_activation_cost::SelectedActivationCostWork {
                batch,
                objects: vec![creature],
                graveyard_orders: BTreeMap::new(),
                owner: selected_activation_cost::SelectedActivationOwner::Emblem {
                    entry,
                    controller: actor,
                    emblem,
                },
                action,
                receipt: ManaPaymentRecord {
                    from_lots: payment.map_or_else(Default::default, |p| p.from_lots.clone()),
                    source: entry,
                    payer: actor,
                    cost,
                    spend,
                    restricted_mana: vec![],
                },
            },
        )));
        next.progress_selected_activation_cost(turn)
    }

    /// Source identity of a pending emblem trigger in an APNAP ordering choice.
    pub fn waiting_emblem_trigger(
        &self,
        instance: crate::occurrence::TriggerInstanceId,
    ) -> Option<crate::emblem::EmblemId> {
        self.triggers
            .emblem_waiting()
            .get(&instance)
            .map(|waiting| waiting.emblem)
    }
    /// All emblems in this game, including those whose creating permanent has left.
    pub fn emblems(&self) -> impl Iterator<Item = &crate::emblem::Emblem> {
        self.characteristics.emblems.values()
    }

    pub(super) fn resolve_create_opponent_emblems(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        definition: crate::emblem::EmblemDefinition,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let players = self
            .combat_rules
            .opponents(self.window.participants(), entry.controller())?;
        let before = self.rules_snapshot(None)?;
        let mut emblems = Vec::with_capacity(players.len());
        for player in players {
            emblems.push(
                self.characteristics
                    .create_emblem(player, definition.clone())?,
            );
        }
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(emblems
                    .into_iter()
                    .map(|emblem| RulesOccurrence::EmblemCreated { emblem })
                    .collect())
            },
        )
    }

    pub(super) fn resolve_create_emblem(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        definition: crate::emblem::EmblemDefinition,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let emblem = self
            .characteristics
            .create_emblem(entry.controller(), definition)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| Ok(vec![RulesOccurrence::EmblemCreated { emblem }]),
        )
    }
}
