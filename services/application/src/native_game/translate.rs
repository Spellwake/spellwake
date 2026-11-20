//! Exact player shape to typed kernel input. No alternate rules execution.
use super::*;
use crate::player_wire::{ManaColor, PlayerAction, PlayerChoice, PlayerPayment, PlayerTarget};
use spellwake_engine::{
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision},
    damage::{DamageAllocation, DamageRecipient},
    effects::{
        DamageDecision, LibraryChoiceKind, LibraryTopReply, PendingWorkKind,
        ResolutionPaymentReply, ZoneReplacementReply,
    },
    game::{DecisionReply, GameCommand, RuleFrame},
    mana::{ManaPayment, ManaSpend, ManaType},
    object_identity::ObjectId,
    opening::OpeningDecision,
    targets::TargetRef,
};

fn payment(value: &PlayerPayment) -> ManaPayment {
    ManaPayment {
        spend: value
            .spend
            .iter()
            .map(|entry| ManaSpend {
                mana_type: match entry.color {
                    ManaColor::White => ManaType::White,
                    ManaColor::Blue => ManaType::Blue,
                    ManaColor::Black => ManaType::Black,
                    ManaColor::Red => ManaType::Red,
                    ManaColor::Green => ManaType::Green,
                    ManaColor::Colourless => ManaType::Colourless,
                },
                amount: entry.amount.as_rules_integer().clone(),
            })
            .collect(),
    }
}
impl NativeScope<'_> {
    pub(super) fn translate_action(
        &self,
        action: &PlayerAction,
    ) -> Result<GameCommand, NativeBridgeError> {
        Ok(match action {
            PlayerAction::CastUntargeted {
                source,
                payment: allocation,
            } => GameCommand::CastUntargeted {
                spell: self.object(source)?,
                payment: allocation.as_ref().map(payment),
            },
            PlayerAction::CastTargeted {
                source,
                target,
                payment: allocation,
            } => match allocation {
                None => GameCommand::Cast {
                    spell: self.object(source)?,
                    target: self.target(target)?,
                },
                Some(allocation) => GameCommand::CastPaid {
                    spell: self.object(source)?,
                    target: self.target(target)?,
                    payment: payment(allocation),
                },
            },
            PlayerAction::ActivateAbility {
                source,
                ability,
                target,
                payment: allocation,
            } => {
                let source = self.object(source)?;
                let NativeReference::ActivatedAbility {
                    source: bound_source,
                    ability,
                } = self.reference(ability)?
                else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                if source != bound_source {
                    return Err(NativeBridgeError::InvalidReference);
                }
                GameCommand::ActivateAbility {
                    source,
                    ability,
                    target: self.target(target)?,
                    payment: allocation.as_ref().map(payment),
                }
            }
            PlayerAction::ActivateMana { source, ability } => {
                // ResolutionPayment expressly permits this action, including a
                // nested color decision. Do not add a blanket decision gate.
                let source = self.object(source)?;
                let NativeReference::ManaAbility {
                    source: bound_source,
                    ability,
                } = self.reference(ability)?
                else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                if source != bound_source {
                    return Err(NativeBridgeError::InvalidReference);
                }
                GameCommand::ActivateMana { source, ability }
            }
            PlayerAction::PlayLand { source } => GameCommand::PlayLand {
                land: self.object(source)?,
            },
            PlayerAction::Pass {} => GameCommand::Pass,
            PlayerAction::Choose { decision, response } => {
                let NativeReference::Decision(id) = self.reference(decision)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                let actual = self
                    .source
                    .pending_decision()
                    .ok_or(NativeBridgeError::WrongDecision)?;
                if actual.id() != id {
                    return Err(NativeBridgeError::WrongDecision);
                }
                let choice = self.translate_reply(self.frame()?, response)?;
                GameCommand::Reply {
                    decision: id,
                    choice,
                }
            }
            PlayerAction::Concede {} => GameCommand::Concede,
        })
    }
    fn objects(&self, handles: &[PlayerHandle]) -> Result<Vec<ObjectId>, NativeBridgeError> {
        handles.iter().map(|handle| self.object(handle)).collect()
    }
    fn target(&self, target: &PlayerTarget) -> Result<TargetRef, NativeBridgeError> {
        match target {
            PlayerTarget::Object { handle } => Ok(TargetRef::Object(self.object(handle)?)),
            PlayerTarget::Stack { handle } => Ok(TargetRef::Stack(self.stack(handle)?)),
            PlayerTarget::Seat { seat_id } => Ok(TargetRef::Player(self.seat(*seat_id)?)),
        }
    }
    fn recipient(&self, target: &PlayerTarget) -> Result<DamageRecipient, NativeBridgeError> {
        match target {
            PlayerTarget::Seat { seat_id } => Ok(DamageRecipient::Player(self.seat(*seat_id)?)),
            PlayerTarget::Object { handle } => {
                let id = self.object(handle)?;
                let creature = self
                    .source
                    .execution()
                    .creature(id)
                    .map_err(|cause| NativeBridgeError::Observation(Box::new(cause)))?;
                if creature.is_none() {
                    return Err(NativeBridgeError::Unsupported);
                }
                Ok(DamageRecipient::Creature(id))
            }
            PlayerTarget::Stack { .. } => Err(NativeBridgeError::Unsupported),
        }
    }
    fn library_objects(
        &self,
        handles: &[PlayerHandle],
    ) -> Result<Vec<ObjectId>, NativeBridgeError> {
        handles
            .iter()
            .map(|handle| match self.reference(handle)? {
                NativeReference::LibraryCard(id) => Ok(id),
                _ => Err(NativeBridgeError::WrongReferenceKind),
            })
            .collect()
    }
    fn translate_reply(
        &self,
        frame: &RuleFrame,
        choice: &PlayerChoice,
    ) -> Result<DecisionReply, NativeBridgeError> {
        Ok(match (frame, choice) {
            (RuleFrame::LibraryTop(f), PlayerChoice::LibraryPartition { top, other }) => {
                let top_first = self.library_objects(top)?;
                let other = self.library_objects(other)?;
                DecisionReply::LibraryTop(match f.kind() {
                    LibraryChoiceKind::Scry => LibraryTopReply::Scry {
                        top_first,
                        bottom_top_first: other,
                    },
                    LibraryChoiceKind::Surveil => LibraryTopReply::Surveil {
                        top_first,
                        to_graveyard: other,
                    },
                })
            }
            (RuleFrame::ZoneReplacement(_), PlayerChoice::Option { option })
                if crate::native_game::handles::supports_zone_replacement(self.source) =>
            {
                DecisionReply::ZoneReplacement(match self.reference(option)? {
                    NativeReference::ZoneApplication(value) => ZoneReplacementReply::Select(value),
                    NativeReference::ZoneOptional(value) => ZoneReplacementReply::Optional(value),
                    _ => return Err(NativeBridgeError::WrongReferenceKind),
                })
            }
            (RuleFrame::ResolutionSelect(_), PlayerChoice::Objects { objects }) => {
                DecisionReply::SelectResolution(self.objects(objects)?)
            }
            (RuleFrame::ResolutionGraveyardOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::OrderResolutionGraveyard(
                    if self.source.execution().pending_work_kind()
                        == Some(PendingWorkKind::LibraryTop)
                    {
                        self.library_objects(ordered)?
                    } else {
                        self.objects(ordered)?
                    },
                )
            }
            (RuleFrame::ReplacementChooser(_), PlayerChoice::Option { option }) => {
                let NativeReference::ReplacementChooser(player) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ReplacementChooser(player)
            }
            (RuleFrame::OptionalEffect(_), PlayerChoice::Option { option }) => {
                let NativeReference::OptionalEffect(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::OptionalEffect(value)
            }
            (
                RuleFrame::ResolutionPayment(_),
                PlayerChoice::Pay {
                    payment: allocation,
                },
            ) => DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(payment(allocation))),
            (RuleFrame::ResolutionPayment(_), PlayerChoice::Decline {}) => {
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
            }
            (RuleFrame::TimestampOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::TimestampOrder(
                    ordered
                        .iter()
                        .map(|handle| match self.reference(handle)? {
                            NativeReference::TokenMember(value) => Ok(value),
                            _ => Err(NativeBridgeError::WrongReferenceKind),
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            (
                RuleFrame::Checks(CheckDecision::CleanupDiscard(_)),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::Discard(self.objects(objects)?),
            (
                RuleFrame::Checks(CheckDecision::GraveyardOrder(_)),
                PlayerChoice::Ordering { ordered },
            ) => DecisionReply::OrderGraveyard(self.objects(ordered)?),
            (
                RuleFrame::Checks(CheckDecision::Legend(_)),
                PlayerChoice::Legend { conflict, keep },
            ) => {
                let NativeReference::Legend(conflict) = self.reference(conflict)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::KeepLegend {
                    conflict,
                    keep: self.object(keep)?,
                }
            }
            (RuleFrame::Triggers(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::OrderTriggers(
                    ordered
                        .iter()
                        .map(|handle| match self.reference(handle)? {
                            NativeReference::Trigger(value) => Ok(value),
                            _ => Err(NativeBridgeError::WrongReferenceKind),
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            (RuleFrame::TriggerTarget(_), PlayerChoice::Target { target }) => {
                DecisionReply::TriggerTarget(self.target(target)?)
            }
            (
                RuleFrame::Combat(CombatDecision::Attackers(_)),
                PlayerChoice::Attackers { assignments },
            ) => DecisionReply::DeclareAttackers(
                assignments
                    .iter()
                    .map(|entry| {
                        let PlayerTarget::Seat { seat_id } = entry.defender else {
                            return Err(NativeBridgeError::Unsupported);
                        };
                        Ok(AttackDeclaration {
                            attacker: self.object(&entry.attacker)?,
                            defender: self.seat(seat_id)?,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (
                RuleFrame::Combat(CombatDecision::Blockers(_)),
                PlayerChoice::Blockers { assignments },
            ) => {
                let mut declarations = Vec::new();
                for entry in assignments {
                    let blocker = self.object(&entry.blocker)?;
                    for attacker in &entry.attackers {
                        declarations.push(BlockDeclaration {
                            blocker,
                            attacker: self.object(attacker)?,
                        });
                    }
                }
                DecisionReply::DeclareBlockers(declarations)
            }
            (
                RuleFrame::Combat(CombatDecision::DamageAssignments(_)),
                PlayerChoice::CombatDamage { assignments },
            ) => DecisionReply::AssignCombatDamage(
                assignments
                    .iter()
                    .map(|entry| {
                        Ok(CombatDamageAssignment {
                            source: self.object(&entry.source)?,
                            target: self.recipient(&entry.target)?,
                            amount: entry.amount.as_rules_integer().clone(),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (
                RuleFrame::Opening(OpeningDecision::Declare { .. }),
                PlayerChoice::Option { option },
            ) => {
                let NativeReference::Mulligan(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::Mulligan(value)
            }
            (
                RuleFrame::Opening(OpeningDecision::Bottom { .. }),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::Bottom(self.objects(objects)?),
            (RuleFrame::CopyTargets(_), PlayerChoice::Option { option }) => {
                let NativeReference::CopyTargets(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::CopyTargets(value)
            }
            (RuleFrame::Damage(DamageDecision::Order(_)), PlayerChoice::Option { option }) => {
                let NativeReference::DamageApplication(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::DamageOrder(value)
            }
            (
                RuleFrame::Damage(DamageDecision::Allocation(_)),
                PlayerChoice::Prevention { allocations },
            ) => DecisionReply::AllocatePrevention(
                allocations
                    .iter()
                    .map(|entry| {
                        let NativeReference::DamageAttempt(attempt) =
                            self.reference(&entry.attempt)?
                        else {
                            return Err(NativeBridgeError::WrongReferenceKind);
                        };
                        Ok(DamageAllocation {
                            attempt,
                            amount: entry.amount.as_rules_integer().clone(),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (RuleFrame::ManaColor(_), PlayerChoice::Option { option }) => {
                let NativeReference::ManaColor(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ManaColor(value)
            }
            (RuleFrame::CreatureType(_), PlayerChoice::Option { option }) => {
                let NativeReference::CreatureType(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::CreatureType(value)
            }
            _ => return Err(NativeBridgeError::Unsupported),
        })
    }
}
