//! Stackless source-sacrifice mana costs and their zone-replacement continuation.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, StatusReason, ZoneReason};

impl EffectWindow {
    // Internal sacrifice-mana coordinator forwarding activation state from EffectWindow.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin_sacrifice_mana(
        &self,
        revision: u64,
        actor: SeatId,
        source: GameObject,
        sacrificed: GameObject,
        ability: crate::mana::ManaAbilityRef,
        parent: ManaParent,
        tap: bool,
        was_land: bool,
        output: ManaOutput,
    ) -> Result<ManaActivationProgress, EffectError> {
        let before = self.rules_snapshot(None)?;
        let window = match (&parent, tap) {
            (ManaParent::CombatPayment(_), true) => {
                self.window
                    .record_declaration_taps(self.window.revision(), actor, &[source.id()])?
                    .0
            }
            (ManaParent::CombatPayment(_), false) => {
                self.window
                    .record_declaration_taps(self.window.revision(), actor, &[])?
                    .0
            }
            (ManaParent::Priority, true) => {
                self.window
                    .record_tap_action(self.window.revision(), actor, source.id())?
            }
            (ManaParent::Priority, false) => self
                .window
                .record_mana_action(self.window.revision(), actor)?,
            (ManaParent::Casting(entry) | ManaParent::Recovery(_, entry), true) => self
                .window
                .record_resolution_tap(self.window.revision(), *entry, actor, source.id())?,
            (ManaParent::Casting(entry) | ManaParent::Recovery(_, entry), false) => self
                .window
                .record_resolution_mana_action(self.window.revision(), *entry)?,
            (ManaParent::Payment(work), true) => self.window.record_resolution_tap(
                self.window.revision(),
                work.cursor.entry,
                actor,
                source.id(),
            )?,
            (ManaParent::Payment(work), false) => self
                .window
                .record_resolution_mana_action(self.window.revision(), work.cursor.entry)?,
        };
        let mut next = self.clone();
        next.revision = revision;
        next.window = window;
        let mut append = RulesJournal::default();
        if tap {
            let after = next
                .window
                .cards()
                .get(source.id())
                .ok_or(EffectError::UnknownObject)?
                .clone();
            next.emit_from(
                EventCause::ManaAbilityCost {
                    source: source.id(),
                    ability,
                },
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(occurrence_flow::status_occurrences(
                        vec![crate::object_identity::ObjectStatusChange {
                            before: source.clone(),
                            after,
                        }],
                        StatusReason::ManaAbilityCost,
                    ))
                },
            )?;
        }
        let batch = next.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::ManaAbilityCost {
                source: source.id(),
                ability,
            },
            action: crate::program::ZoneActionKind::SacrificeCost,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: sacrificed.id(),
                reason: ZoneReason::SacrificeCost,
            }],
        })?;
        let was_artifact = next
            .current_characteristics(source.id())?
            .is_some_and(|view| view.has_type(CardType::Artifact));
        let names = next
            .current_characteristics(source.id())?
            .ok_or(EffectError::InconsistentProgram)?
            .current_names;
        let source_tap = SacrificedManaSource {
            tapped: tap,
            was_land,
            was_artifact,
            names,
        };
        let source = next
            .window
            .cards()
            .get(source.id())
            .ok_or(EffectError::UnknownObject)?
            .clone();
        next.pending_work = Some(PendingWork::ManaSacrifice(Box::new(ManaSacrificeWork {
            source,
            sacrificed,
            ability,
            actor,
            parent,
            output,
            source_tap,
            batch: Some(batch),
        })));
        let result = next.advance_sacrifice_mana(&mut append)?;
        Ok(ManaActivationProgress::new(next, result, append))
    }

    pub(super) fn advance_sacrifice_mana(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<ManaActivationResult, EffectError> {
        let Some(PendingWork::ManaSacrifice(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        if let Some(batch) = &mut work.batch {
            if let Some(frame) = batch.progress(self.revision)? {
                self.pending_work = Some(PendingWork::ManaSacrifice(work));
                return Ok(ManaActivationResult::PendingRule { frame });
            }
            let cause = EventCause::ManaAbilityCost {
                source: work.source.id(),
                ability: work.ability,
            };
            let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
                cause: cause.clone(),
                action: crate::program::ZoneActionKind::SacrificeCost,
                intents: vec![zone_replacement::ZoneMoveIntent {
                    before: work.sacrificed.id(),
                    reason: ZoneReason::SacrificeCost,
                }],
            })?;
            batch.verify_initial(fresh)?;
            let destination = batch
                .destination(work.sacrificed.id())?
                .physical(None)
                .ok_or(EffectError::InconsistentProgram)?;
            let before = self.rules_snapshot(None)?;
            let current = self
                .window
                .cards()
                .get(work.sacrificed.id())
                .ok_or(EffectError::UnknownObject)?
                .clone();
            let (window, after) =
                self.window
                    .sacrifice_mana_source(work.actor, work.sacrificed.id(), destination)?;
            self.window = window;
            if work.source.id() == current.id() {
                work.source = current.clone();
            }
            let movement = crate::object_identity::ObjectMovement {
                before: current,
                after,
            };
            self.emit_from(cause, &before, None, append, |old, new, controllers| {
                Ok(vec![zone_occurrence_at(
                    old,
                    new,
                    &movement,
                    ZoneReason::SacrificeCost,
                    controllers,
                )?])
            })?;
            work.batch = None;
        }
        if self.linked_return_work.is_some() {
            self.pending_work = Some(PendingWork::ManaSacrifice(work));
            self.finish_linked_return_if_ready(append)?;
            if let Some(frame) = self.linked_return_decision()? {
                return Ok(ManaActivationResult::PendingRule { frame });
            }
            let Some(PendingWork::ManaSacrifice(returned)) = self.pending_work.take() else {
                return Err(EffectError::WrongPhase);
            };
            work = returned;
        }
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::ManaAbilityActivated {
                source: work.source.id(),
                ability: work.ability,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::ManaAbility {
                    source: work.source.clone(),
                    ability: work.ability,
                    controller: work.actor,
                }])
            },
        )?;
        let (inner, restriction) = match work.output {
            ManaOutput::Modified {
                modifier:
                    crate::mana::ManaOutputModifier::Restrict(
                        crate::mana::ManaOutputRestriction::Fixed(rule),
                    ),
                output,
            } => (*output, Some(rule)),
            other => (other, None),
        };
        match inner {
            ManaOutput::ChooseColor { amount, colors } => {
                let frame = ManaColorChoice {
                    revision: self.revision,
                    source: work.source.id(),
                    ability: work.ability,
                    chooser: work.actor,
                    amount,
                    colors,
                };
                self.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                    frame: frame.clone(),
                    parent: work.parent,
                    controller_damage: None,
                    triggered: false,
                    activation_controller: None,
                    restriction,
                    sacrificed_source: Some(work.source_tap),
                })));
                Ok(ManaActivationResult::Pending { frame })
            }
            ManaOutput::Fixed(produced) => {
                let production = self.credit_mana_at_cut(
                    work.source.id(),
                    work.ability,
                    (work.actor, work.actor),
                    produced,
                    restriction,
                    false,
                    Some(work.source_tap),
                    append,
                )?;
                if let Some(frame) = self.schedule_triggered_mana(work.parent.clone()) {
                    return Ok(ManaActivationResult::Pending { frame });
                }
                self.pending_work = match work.parent {
                    ManaParent::Priority | ManaParent::Casting(_) => None,
                    ManaParent::CombatPayment(cursor) => Some(PendingWork::Combat(*cursor)),
                    ManaParent::Recovery(frame, _) => {
                        Some(PendingWork::CastRecovery(Box::new(frame)))
                    }
                    ManaParent::Payment(parent) => {
                        Some(PendingWork::ResolutionPayment(Box::new(parent)))
                    }
                };
                Ok(ManaActivationResult::Complete { production })
            }
            _ => Err(EffectError::InconsistentProgram),
        }
    }
}
