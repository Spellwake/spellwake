//! Paid special actions that move a hand card to a fresh exile incarnation.
use super::*;
use crate::program::ExileAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExileHandAction {
    Plot,
    Foretell,
    Suspend,
}

impl EffectWindow {
    /// Turn on which this exact face-up card became plotted.
    pub fn plotted_on(&self, object: ObjectId) -> Option<u64> {
        self.plotted.get(&object).copied()
    }

    /// Return the turn on which this exact currently exiled card was foretold.
    /// The owner may inspect the card while it remains face down in exile.
    pub fn foretold_on(&self, object: ObjectId) -> Option<u64> {
        self.window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Exile)?;
        self.characteristics.foretold.get(&object).copied()
    }

    pub(crate) fn exile_from_hand(
        &self,
        action: ExileHandAction,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        payment: Option<&ManaPayment>,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let current = self.evaluated_characteristics()?;
        let card = current.get(source).ok_or(EffectError::UnknownObject)?;
        if card.zone != ObjectZone::Hand(actor) {
            return Err(EffectError::NotOwnHand);
        }
        let mut time = 0;
        let foretell_cost = ManaCost::generic(2.into());
        let price = match action {
            ExileHandAction::Plot => {
                validate_main_phase(&self.window, Some(turn), actor)?;
                card.printed_text
                    .plot
                    .as_ref()
                    .ok_or(EffectError::InvalidCastResource)?
            }
            ExileHandAction::Foretell => {
                crate::actions::validate_land_timing(&self.window, Some(turn), actor, true)?;
                if !matches!(
                    card.printed_text.exile_action,
                    Some(ExileAction::Foretell(_))
                ) {
                    return Err(EffectError::InvalidCastResource);
                }
                &foretell_cost
            }
            ExileHandAction::Suspend => {
                self.check_split_second(&current)?;
                self.validate_legendary_spell_cast(source, actor, &current)?;
                if !card.has_type(CardType::Instant) && !card.has_keyword(KeywordAbility::Flash) {
                    validate_main_phase(&self.window, Some(turn), actor)?;
                }
                let Some(ExileAction::Suspend { counters, price }) =
                    &card.printed_text.exile_action
                else {
                    return Err(EffectError::InvalidCastResource);
                };
                time = *counters;
                price
            }
        };
        let spend = self.prepare_mana_payment(price, payment, actor)?;
        let previous = self.rules_snapshot(Some(turn))?;
        let (window, moved) = self.window.exile_hand_special_action(actor, source)?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        let payment = ManaPayment {
            from_lots: payment.map_or_else(Default::default, |p| p.from_lots.clone()),
            spend,
        };
        next.spend_mana_lots(actor, &payment.spend, &[], Some(&payment.from_lots))?;
        next.debit_mana(actor, &payment.spend);
        use crate::occurrence::{EventCause, ZoneReason};
        let (cause, reason) = match action {
            ExileHandAction::Plot => {
                next.plotted
                    .insert(moved.after.id(), turn.position().number);
                (
                    EventCause::Plot {
                        actor,
                        source,
                        payment,
                    },
                    ZoneReason::Plot,
                )
            }
            ExileHandAction::Foretell => {
                next.characteristics
                    .foretold
                    .insert(moved.after.id(), turn.position().number);
                (
                    EventCause::Foretell {
                        actor,
                        source,
                        payment,
                    },
                    ZoneReason::Foretell,
                )
            }
            ExileHandAction::Suspend => {
                next.set_time_counters(moved.after.id(), i64::from(time).into());
                (
                    EventCause::Suspend {
                        actor,
                        source,
                        payment,
                    },
                    ZoneReason::Suspend,
                )
            }
        };
        let mut append = RulesJournal::default();
        next.emit_from(
            cause,
            &previous,
            Some(turn),
            &mut append,
            |old, new, controllers| {
                Ok(vec![zone_occurrence_at(
                    old,
                    new,
                    &moved,
                    reason,
                    controllers,
                )?])
            },
        )?;
        Ok(RulesTransition::new(next, (), append))
    }
}
