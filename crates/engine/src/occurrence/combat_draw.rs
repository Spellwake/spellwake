//! Delayed draw triggers retain their creator and controller until cleanup.
//! The creature that deals damage can change controller or leave and return.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CombatDrawSubject {
    Exact(ObjectId),
    ControlledPowerAtMost(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CombatDrawSource {
    Physical(Box<SupportedAppearance>),
    SpellCopy(StackEntryId, BTreeSet<crate::characteristics::Color>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CombatDrawWatch {
    controller: SeatId,
    source: CombatDrawSource,
    subject: CombatDrawSubject,
}

impl TriggerState {
    pub(crate) fn watch_combat_draw(
        &mut self,
        controller: SeatId,
        source: CombatDrawSource,
        subject: CombatDrawSubject,
    ) {
        self.combat_draws.push(CombatDrawWatch {
            controller,
            source,
            subject,
        });
    }

    pub(super) fn match_combat_draws(
        &mut self,
        event: &RulesEvent,
        after: &RulesView<'_>,
        discoveries: &mut Vec<RulesRecord>,
    ) -> Result<(), TriggerError> {
        if self.combat_draws.is_empty() {
            return Ok(());
        }
        for occurrence in &event.occurrences {
            let RulesOccurrence::DamageDealt {
                damage,
                kind: DamageKind::Combat,
                ..
            } = &occurrence.value
            else {
                continue;
            };
            let crate::damage::DamageSourceId::Object(dealer) = damage.source.id() else {
                continue;
            };
            let (related_player, related) = match damage.target {
                crate::damage::DamageRecipient::Player(player) => (
                    player,
                    TriggerRelated::CombatDamageToPlayer {
                        source: dealer,
                        amount: damage.amount.clone(),
                        player,
                        attempt: damage.attempt,
                    },
                ),
                crate::damage::DamageRecipient::Creature(id)
                | crate::damage::DamageRecipient::Planeswalker(id)
                | crate::damage::DamageRecipient::Battle(id) => {
                    let Some(controller) = after
                        .evaluated
                        .get(id)
                        .filter(|v| v.has_type(CardType::Planeswalker))
                        .and_then(|v| v.controller)
                    else {
                        continue;
                    };
                    (
                        controller,
                        TriggerRelated::CombatDamageToPlaneswalker {
                            planeswalker: id,
                            attempt: damage.attempt,
                            amount: damage.amount.clone(),
                        },
                    )
                }
            };
            for watch in &self.combat_draws {
                let count = match watch.subject {
                    CombatDrawSubject::Exact(id) if id == dealer => damage.amount.clone(),
                    CombatDrawSubject::ControlledPowerAtMost(limit)
                        if matches!(damage.target, crate::damage::DamageRecipient::Player(_))
                            && after.evaluated.get(dealer).is_some_and(|p| {
                                p.controller == Some(watch.controller)
                                    && p.is_creature()
                                    && p.power_toughness.as_ref().is_some_and(|pt| {
                                        pt.derived.power <= i64::from(limit).into()
                                    })
                            }) =>
                    {
                        1.into()
                    }
                    _ => continue,
                };
                let instance = TriggerInstanceId(self.next_instance);
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                match &watch.source {
                    CombatDrawSource::Physical(source) => {
                        let causes = TriggerCauseSet::new(
                            event.id,
                            TriggerCauseKey::Occurrence(occurrence.id),
                            vec![TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(related_player),
                                related: related.clone(),
                            }],
                        )?;
                        let waiting = WaitingInstance {
                            id: instance,
                            definition: TriggeredAbilityId(0),
                            granting_effect: None,
                            causes,
                            source: source.as_ref().clone(),
                            controller: watch.controller,
                            body: ExecutableBody::new(
                                crate::program::BodyTarget::None,
                                vec![crate::program::Instruction::DrawController {
                                    amount: crate::program::ValueExpr::Literal(count),
                                }],
                            ),
                        };
                        self.waiting.insert(instance, waiting.clone());
                        discoveries.push(RulesRecord::TriggerDiscovered(Box::new(waiting)));
                    }
                    CombatDrawSource::SpellCopy(original, colors) => {
                        self.delayed_copy_waiting.insert(
                            instance,
                            (
                                watch.controller,
                                *original,
                                DelayedCopyAction::Draw(count, colors.clone()),
                            ),
                        );
                        discoveries.push(RulesRecord::DelayedCopyTriggerDiscovered {
                            instance,
                            controller: watch.controller,
                            original: *original,
                            cause: occurrence.id,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}
