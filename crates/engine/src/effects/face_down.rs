//! CR 702.37/702.168 face-down casting and the turn-face-up special action.
use super::*;
use crate::face_down::{FaceDownKind, FaceUpAbility};
impl EffectWindow {
    /// Privileged view of the prospective face-up characteristics, including layers and copied text.
    /// Public callers must authorize looking at the underlying card separately.
    ///
    /// # Errors
    /// Returns an error if the prospective characteristics cannot be evaluated.
    pub fn face_up_preview(
        &self,
        object: ObjectId,
    ) -> Result<Option<ObjectCharacteristics>, EffectError> {
        let Some(card) = self.window.cards().get(object) else {
            return Ok(None);
        };
        if card.face_down().is_none() {
            return Ok(None);
        }
        let mut next = self.clone();
        next.window.set_orientation(object, None)?;
        Ok(next.evaluated_characteristics()?.get(object).cloned())
    }
    /// Mana cost of the actual creature card represented by this manifested or cloaked permanent.
    pub fn manifest_face_up_cost(&self, object: ObjectId) -> Option<&ManaCost> {
        let card = self.window.cards().get(object)?;
        if !matches!(
            card.face_down(),
            Some(FaceDownKind::Manifest | FaceDownKind::Cloak)
        ) || card.zone() != ObjectZone::Battlefield
        {
            return None;
        }
        let program = card.card_id().and_then(|id| self.programs.get(&id))?;
        if !program.has_type(CardType::Creature) {
            return None;
        }
        program.printed_mana_cost()
    }
    pub(crate) fn cast_face_down(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        kind: FaceDownKind,
        payment: Option<ManaPayment>,
        turn: Option<&TurnScheduler>,
    ) -> Result<CastTransition, EffectError> {
        self.next_revision(expected)?;
        let current = self
            .current_characteristics(spell)?
            .ok_or(EffectError::UnknownObject)?;
        if current.zone != ObjectZone::Hand(actor)
            && !self.window.cards().get(spell).is_some_and(|object| {
                self.commander_in_command(object, actor).is_some()
                    || (object.owner() == actor && self.warped_on(spell).is_some())
            })
        {
            return Err(EffectError::NotOwnHand);
        }
        if !current.abilities.face_up.iter().any(|&index| {
            current
                .printed_text
                .face_up_abilities
                .get(index)
                .is_some_and(|rule| rule.kind() == kind)
        }) {
            return Err(EffectError::NotCastableSpell);
        }
        let mut announced = self.clone();
        announced.window.set_orientation(spell, Some(kind))?;
        announced.cast_on_turn(expected, actor, spell, None, payment, turn)
    }
    pub(crate) fn turn_face_up(
        &self,
        expected: u64,
        actor: SeatId,
        object: ObjectId,
        ability: usize,
        payment: Option<&ManaPayment>,
        turn: Option<&TurnScheduler>,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        self.window.validate_action_actor(actor)?;
        let current = self
            .current_characteristics(object)?
            .ok_or(EffectError::UnknownObject)?;
        if current.zone != ObjectZone::Battlefield || current.controller != Some(actor) {
            return Err(EffectError::NotAbilitySource);
        }
        let prospective = self
            .face_up_preview(object)?
            .ok_or(EffectError::NotAbilitySource)?;
        let rule = prospective
            .printed_text
            .face_up_abilities
            .get(ability)
            .filter(|_| prospective.abilities.face_up.contains(&ability));
        let price = if ability == prospective.printed_text.face_up_abilities.len() {
            self.manifest_face_up_cost(object)
        } else {
            rule.map(FaceUpAbility::cost)
        }
        .ok_or(EffectError::UnknownActivatedAbility)?;
        let face_up_action = if prospective.is_creature() {
            crate::mana::ManaSpecialAction::TurnCreatureFaceUp
        } else {
            crate::mana::ManaSpecialAction::TurnFaceUp
        };
        let (spend, restricted_spend) = self
            .prepare_special_action_mana_payment(price, payment, actor, face_up_action)
            .or_else(|_| {
                self.prepare_special_action_mana_payment(
                    price,
                    payment,
                    actor,
                    crate::mana::ManaSpecialAction::TurnFaceUp,
                )
            })?;
        let before = self.rules_snapshot(turn)?;
        let mut next = self.clone();
        next.window = next.window.record_turn_face_up(actor, object)?;
        next.spend_mana_lots(
            actor,
            &spend,
            &restricted_spend,
            payment.map(|allocation| &allocation.from_lots),
        )?;
        next.debit_restricted_mana(actor, &restricted_spend)?;
        next.debit_mana(actor, &spend);
        let mega = matches!(rule, Some(FaceUpAbility::Megamorph(_)));
        let counters = PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into(),
        };
        let counters = if mega {
            let current = next.evaluated_characteristics()?;
            next.adjusted_pt_counters(&current, object, counters, None)?
        } else {
            counters
        };
        if mega && counters.valid_placement() {
            next.characteristics.place_counters(object, &counters)?;
        }
        next.revision = revision;
        use crate::occurrence::{EventCause, RulesOccurrence};
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::TurnFaceUp {
                actor,
                source: object,
                payment: ManaPayment {
                    from_lots: payment.map_or_else(Default::default, |p| p.from_lots.clone()),
                    spend,
                },
            },
            &before,
            turn,
            &mut append,
            |old, new, _| {
                let prior = old
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?;
                let after = new
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?;
                let mut records = Vec::new();
                if mega {
                    records.push(RulesOccurrence::CountersPlaced {
                        object,
                        before: old.characteristics.counters(object),
                        after: new.characteristics.counters(object),
                        added: counters,
                        before_appearance: Box::new(prior.clone()),
                        after_appearance: Box::new(after.clone()),
                    });
                }
                records.push(RulesOccurrence::TurnedFaceUp {
                    before: Box::new(prior),
                    after: Box::new(after),
                    ability,
                });
                Ok(records)
            },
        )?;
        Ok(RulesTransition::new(next, (), append))
    }
}
