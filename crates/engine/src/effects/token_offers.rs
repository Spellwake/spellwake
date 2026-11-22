//! In a tempting offer, opponents decide in table order before any of their tokens enter.
use super::*;
use crate::token::{TemptingTokenPhase, TokenCreation};
use resolution_choices::{OptionalEffectChoice, OptionalEffectWork};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TemptingTokenOffer {
    pub(super) amount: RulesInteger,
    pub(super) replies: Vec<(SeatId, bool)>,
    opponents: Vec<SeatId>,
}

impl EffectWindow {
    pub(super) fn progress_token_offer(
        &mut self,
        creation: &TokenCreation,
        phase: TemptingTokenPhase,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let controller = entry.controller();
        let entry = entry.id();
        match phase {
            TemptingTokenPhase::Initial => {
                let index = cursor.instruction;
                let count = self.resolve_value_expr(controller, &creation.amount)?;
                let mut creation = creation.clone();
                creation.amount = crate::program::ValueExpr::Literal(count.clone());
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .tempting_token_offer = Some(TemptingTokenOffer {
                    amount: count,
                    replies: Vec::new(),
                    opponents,
                });
                let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                payload.expand_instruction(
                    index,
                    false,
                    [
                        Instruction::CreateTokens {
                            creations: vec![creation.clone()],
                        },
                        Instruction::TemptWithTokens {
                            creation: creation.clone(),
                            phase: TemptingTokenPhase::Opponents,
                        },
                        Instruction::TemptWithTokens {
                            creation,
                            phase: TemptingTokenPhase::Reward,
                        },
                    ],
                );
            }
            TemptingTokenPhase::Opponents => {
                let offer = cursor
                    .tempting_token_offer
                    .as_ref()
                    .ok_or(EffectError::InconsistentProgram)?;
                if let Some(&chooser) = offer.opponents.get(offer.replies.len()) {
                    self.pending_work =
                        Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                            rebound_or_buyback: false,
                            cast: None,
                            cursor: cursor.clone(),
                            chooser,
                            options: BTreeSet::from([
                                OptionalEffectChoice::Take,
                                OptionalEffectChoice::Decline,
                            ]),
                        })));
                } else {
                    let definition = creation
                        .definition
                        .materialize(&self.creature_vocabulary)
                        .map_err(|_| EffectError::InconsistentProgram)?;
                    let count = offer.amount.to_usize().ok_or(EffectError::TokenCapacity)?;
                    let groups = offer
                        .replies
                        .iter()
                        .filter(|(_, accepted)| *accepted)
                        .map(|(player, _)| {
                            (*player, count, definition.clone(), creation.enters_tapped)
                        })
                        .collect();
                    self.begin_materialized_token_entries(groups, None, false)?;
                }
            }
            TemptingTokenPhase::Reward => {
                let index = cursor.instruction;
                let offer = self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .tempting_token_offer
                    .take()
                    .ok_or(EffectError::InconsistentProgram)?;
                let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let accepted = offer
                    .replies
                    .iter()
                    .filter(|(_, accepted)| *accepted)
                    .count();
                payload.expand_instruction(
                    index,
                    false,
                    std::iter::repeat_n(
                        Instruction::CreateTokens {
                            creations: vec![creation.clone()],
                        },
                        accepted,
                    ),
                );
            }
        }
        Ok(())
    }
}
