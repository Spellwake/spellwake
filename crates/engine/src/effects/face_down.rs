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
    /// Mana cost of the actual creature card represented by this manifested permanent.
    pub fn manifest_face_up_cost(&self, object: ObjectId) -> Option<&ManaCost> {
        let card = self.window.cards().get(object)?;
        if card.face_down() != Some(FaceDownKind::Manifest)
            || card.zone() != ObjectZone::Battlefield
        {
            return None;
        }
        let program = card.card_id().and_then(|id| self.programs.get(&id))?;
        if !program.has_type(CardType::Creature) {
            return None;
        }
        program.printed_mana_cost()
    }
    pub(super) fn turn_face_up_selection(
        &self,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let domain = self
            .evaluated_characteristics()?
            .objects
            .values()
            .filter(|view| {
                view.zone == ObjectZone::Battlefield
                    && view.is_creature()
                    && view.controller == Some(entry.controller())
                    && self
                        .window
                        .cards()
                        .get(view.object)
                        .is_some_and(|card| card.face_down().is_some())
            })
            .map(|view| view.object)
            .collect::<Vec<_>>();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                entry.id(),
                cursor.instruction,
                entry.controller(),
                domain,
                1,
                ResolutionSelectionKind::TurnCreatureFaceUp,
            )
        }))
    }
    pub(super) fn resume_turn_face_up_effect(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.current_instruction()? != &Instruction::MayTurnControlledCreatureFaceUp
            || self.turn_face_up_selection()?.as_ref() != Some(frame)
        {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(selected) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if selected.len() > 1
            || selected
                .first()
                .is_some_and(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let mut append = RulesJournal::default();
        if let Some(&id) = selected.first() {
            use crate::occurrence::{EventCause, RulesOccurrence};
            let before = self.rules_snapshot(None)?;
            let card = self
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?
                .clone();
            let underlying = self
                .printed_face(&card)
                .ok_or(EffectError::InconsistentProgram)?;
            let instant_or_sorcery =
                underlying.has_type(CardType::Instant) || underlying.has_type(CardType::Sorcery);
            if !instant_or_sorcery {
                next.window.set_orientation(id, None)?;
            }
            next.emit_from(
                EventCause::Instruction {
                    entry: frame.entry(),
                    instruction: frame.instruction(),
                },
                &before,
                None,
                &mut append,
                |old, new, _| {
                    Ok(vec![if instant_or_sorcery {
                        RulesOccurrence::FaceDownRevealed { object: card }
                    } else {
                        RulesOccurrence::TurnedFaceUpByEffect {
                            before: Box::new(
                                old.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                            ),
                            after: Box::new(
                                new.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                            ),
                        }
                    }])
                },
            )?;
        }
        next.advance_instruction()?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
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
        let spend = self.prepare_mana_payment(price, payment, actor)?;
        let before = self.rules_snapshot(turn)?;
        let mut next = self.clone();
        next.window = next.window.record_turn_face_up(actor, object)?;
        next.spend_mana_lots(
            actor,
            &spend,
            &[],
            payment.map(|allocation| &allocation.from_lots),
        )?;
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
