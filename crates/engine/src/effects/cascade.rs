//! Cascade retains the exact cards it exiled, including the spell's captured mana value.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, TriggerRelated, ZoneReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CascadeBatch {
    pub(super) limit: RulesInteger,
    pub(super) candidate: Option<ObjectId>,
    exiled: Vec<ObjectId>,
}

impl CascadeBatch {
    fn qualifies_card(&self, program: &Program) -> bool {
        !program.has_type(CardType::Land) && below_cascade_limit(&program.mana_value(), &self.limit)
    }
    pub(super) fn allows_result(&self, selected: ObjectId, result: &RulesInteger) -> bool {
        self.candidate == Some(selected) && below_cascade_limit(result, &self.limit)
    }
}
impl EffectWindow {
    pub(super) fn stack_spell_mana_value(
        &self,
        object: ObjectId,
    ) -> Result<RulesInteger, EffectError> {
        let card = self
            .window
            .cards()
            .get(object)
            .filter(|c| c.zone() == ObjectZone::Stack)
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(if card.face_down().is_some() {
            0.into()
        } else {
            let program = self
                .copiable_program(object)
                .ok_or(EffectError::InconsistentProgram)?;
            let fixed = if card.transformed() {
                card.card_id()
                    .and_then(|card| self.programs.get(&card))
                    .map(Program::mana_value)
                    .ok_or(EffectError::InconsistentProgram)?
            } else {
                program.mana_value()
            };
            if program.x_mana_symbols == 0 {
                return Ok(fixed);
            }
            let entry = self
                .window
                .entries()
                .iter()
                .chain(self.window.resolving())
                .chain(self.window.checking_targets())
                .find(|entry| self.window.spell_object(entry) == Some(object))
                .ok_or(EffectError::InconsistentProgram)?;
            entry.chosen_x().map_or(fixed.clone(), |x| {
                fixed.plus(&x.times(&i64::from(program.x_mana_symbols).into()))
            })
        })
    }
    pub(crate) fn has_stacked_random_operation(&self) -> bool {
        self.payloads
            .values()
            .filter_map(StackPayload::instructions)
            .any(|body| {
                body.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::Cascade
                            | Instruction::CascadeBottom
                            | Instruction::ReturnRandomLinkedExile
                            | Instruction::RevealCreatureOfSacrificedType
                            | Instruction::ExileNonlandForFormerControllers
                            | Instruction::DiscardOneAtRandom(_)
                            | Instruction::SacrificeRandomUntilLand
                            | Instruction::ReturnRandomGraveyardCards { .. }
                            | Instruction::FlipCoin { .. }
                            | Instruction::RollDie { .. }
                            | Instruction::FlipCoins { .. }
                            | Instruction::Discover { .. }
                            | Instruction::ShuffleOwnLibrary
                            | Instruction::ShufflePlayerLibrary(_)
                            | Instruction::ShuffleTargetIntoLibrary
                            | Instruction::RevealPlayerHand(_)
                            | Instruction::ShuffleAbilitySourceIntoLibrary
                            | Instruction::ShuffleOwnGraveyardIntoLibrary
                            | Instruction::ShuffleTargetGraveyardCards { .. }
                            | Instruction::ShuffleEachHandAndGraveyard
                            | Instruction::RevealCreatureForEachTargetController
                            | Instruction::RevealUntilLandKind
                            | Instruction::LookTopPlaneswalkers
                            | Instruction::LookTopTakeOne { .. }
                            | Instruction::LookTopReveal { .. }
                    )
                })
            })
    }

    pub(super) fn cascade_batch(&self, entry: StackEntryId) -> Option<&CascadeBatch> {
        let StackPayload::Instructions(payload) = self.payloads.get(&entry)? else {
            return None;
        };
        payload.cascade.as_ref()
    }
    fn set_cascade(
        &mut self,
        entry: StackEntryId,
        batch: Option<CascadeBatch>,
    ) -> Result<(), EffectError> {
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) else {
            return Err(EffectError::InconsistentProgram);
        };
        payload.cascade = batch;
        Ok(())
    }
    /// CR 702.85a examines individual exiled objects in top-to-bottom order.
    pub(super) fn exile_for_cascade(
        &mut self,
        cursor: &ResolutionCursor,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(cursor.entry)
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TriggerRelated::SpellCast(cast) = &member.related else {
            return Err(EffectError::InconsistentProgram);
        };
        let source = cast.spell().object.id();
        let limit = if self
            .window
            .cards()
            .get(source)
            .is_some_and(|o| o.zone() == ObjectZone::Stack)
        {
            self.stack_spell_mana_value(source)?
        } else {
            cast.mana_value().clone()
        };
        let mut batch = CascadeBatch {
            limit,
            candidate: None,
            exiled: Vec::new(),
        };
        let actor = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        while let Some(&top) = self.window.cards().library(actor).first() {
            let before = self.rules_snapshot(None)?;
            let (window, exit) = self.window.resolution_move_with_exit(
                self.window.revision(),
                top,
                ObjectDestination::Exile,
            )?;
            let crate::stack_window::StackExit::Object(movement) = exit else {
                return Err(EffectError::InconsistentProgram);
            };
            self.window = window;
            let object = movement.after.id();
            let program = self
                .copiable_program(object)
                .ok_or(EffectError::InconsistentProgram)?;
            let eligible = batch.qualifies_card(program);
            self.emit_from(
                EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                &before,
                None,
                append,
                |prev, now, controllers| {
                    Ok(vec![occurrence_flow::zone_occurrence_at(
                        prev,
                        now,
                        &movement,
                        ZoneReason::CascadeExile,
                        controllers,
                    )?])
                },
            )?;
            batch.exiled.push(object);
            if eligible {
                batch.candidate = Some(object);
                break;
            }
        }
        self.set_cascade(cursor.entry, Some(batch))
    }
    pub(super) fn cascade_remaining(
        &self,
        entry: StackEntryId,
    ) -> Result<Vec<ObjectId>, EffectError> {
        Ok(self
            .cascade_batch(entry)
            .ok_or(EffectError::InconsistentProgram)?
            .exiled
            .iter()
            .copied()
            .filter(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .is_some_and(|obj| obj.zone() == ObjectZone::Exile)
            })
            .collect())
    }
    pub(super) fn return_cascade_cards(
        &mut self,
        cursor: &ResolutionCursor,
        ordered: &[ObjectId],
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !ordered.is_empty() {
            let before = self.rules_snapshot(None)?;
            let mut movements = Vec::with_capacity(ordered.len());
            for &id in ordered {
                let (window, exit) = self.window.resolution_move_with_exit(
                    self.window.revision(),
                    id,
                    ObjectDestination::LibraryBottom,
                )?;
                let crate::stack_window::StackExit::Object(movement) = exit else {
                    return Err(EffectError::InconsistentProgram);
                };
                self.window = window;
                movements.push(movement);
            }
            self.emit_from(
                EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                &before,
                None,
                append,
                |prev, now, controllers| {
                    movements
                        .iter()
                        .map(|m| {
                            occurrence_flow::zone_occurrence_at(
                                prev,
                                now,
                                m,
                                ZoneReason::CascadeBottom,
                                controllers,
                            )
                        })
                        .collect::<Result<Vec<RulesOccurrence>, _>>()
                },
            )?;
        }
        self.set_cascade(cursor.entry, None)?;
        self.advance_instruction()
    }
    pub(super) fn random_move_candidates(
        &self,
        instruction: &Instruction,
    ) -> Result<Vec<ObjectId>, EffectError> {
        use crate::program::ResolvingPlayer;
        match instruction {
            Instruction::DiscardOneAtRandom(role) => {
                let player = self.resolving_player(*role)?;
                Ok(self
                    .window
                    .cards()
                    .iter()
                    .filter(|object| object.zone() == ObjectZone::Hand(player))
                    .map(GameObject::id)
                    .collect())
            }
            Instruction::SacrificeRandomUntilLand => {
                let player = self.resolving_player(ResolvingPlayer::TargetPlayer)?;
                Ok(self
                    .evaluated_characteristics()?
                    .objects
                    .values()
                    .filter(|permanent| {
                        permanent.zone == ObjectZone::Battlefield
                            && permanent.controller == Some(player)
                    })
                    .map(|permanent| permanent.object)
                    .collect())
            }
            _ => Err(EffectError::InconsistentProgram),
        }
    }

    pub(super) fn random_graveyard_candidates(
        &self,
        player: SeatId,
        filter: Option<crate::program::ObjectFilter>,
    ) -> Result<Vec<ObjectId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(self
            .window
            .cards()
            .graveyard(player)
            .iter()
            .copied()
            .filter(|object| {
                evaluated.get(*object).is_some_and(|card| {
                    filter.is_none_or(|filter| self.matches_graveyard_card(filter, card))
                })
            })
            .collect())
    }

    /// A host-owned random-order continuation. It is never published as a player decision.
    pub(crate) fn random_resolution_pending(&self) -> bool {
        let Some(PendingWork::OptionalEffect(work)) = &self.pending_work else {
            return false;
        };
        if work
            .cursor
            .revealed_piles
            .as_ref()
            .is_some_and(|piles| piles.pending_random_bottom.is_some())
            || work
                .cursor
                .results
                .discover_state
                .as_ref()
                .is_some_and(|d| d.awaiting_random_bottom)
        {
            return true;
        }
        matches!(
            self.payloads
                .get(&work.cursor.entry)
                .and_then(StackPayload::instructions)
                .and_then(|body| body.get(work.cursor.instruction)),
            Some(
                Instruction::CascadeBottom
                    | Instruction::ReturnRandomLinkedExile
                    | Instruction::RevealCreatureOfSacrificedType
                    | Instruction::ExileNonlandForFormerControllers
                    | Instruction::DiscardOneAtRandom(_)
                    | Instruction::SacrificeRandomUntilLand
                    | Instruction::ReturnRandomGraveyardCards { .. }
                    | Instruction::FlipCoin { .. }
                    | Instruction::RollDie { .. }
                    | Instruction::FlipCoins { .. }
            )
        )
    }
    pub(crate) fn finish_random_resolution(
        mut self,
        tape: &EntropyTape,
    ) -> Result<(RuleProgress, EntropyTape), crate::game::GameError> {
        if !self.random_resolution_pending() {
            return Err(EffectError::WrongResolutionFrame.into());
        }
        let Some(PendingWork::OptionalEffect(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame.into());
        };
        if let Some(piles) = work.cursor.revealed_piles.as_mut()
            && let Some(remainder) = piles.pending_random_bottom.take()
        {
            let (order, entropy) = tape.shuffled(remainder)?;
            piles.bottom_order = Some(order);
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        if work
            .cursor
            .results
            .discover_state
            .as_ref()
            .is_some_and(|d| d.awaiting_random_bottom)
        {
            let remaining: Vec<ObjectId> = work
                .cursor
                .results
                .discover_state
                .as_ref()
                .map(|d| {
                    d.exiled
                        .iter()
                        .copied()
                        .filter(|id| {
                            self.window
                                .cards()
                                .get(*id)
                                .is_some_and(|o| o.zone() == ObjectZone::Exile)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let (order, entropy) = tape.shuffled(remaining)?;
            if let Some(d) = work.cursor.results.discover_state.as_mut() {
                d.awaiting_random_bottom = false;
                d.bottom_order = Some(order);
            }
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        if let Some(Instruction::RollDie { sides, count, .. }) = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            .cloned()
        {
            let player = work.chooser;
            let cursor = work.cursor;
            let controller = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            self.pending_work = Some(PendingWork::Resolution(cursor.clone()));
            let raw_n = self.resolve_value_expr(controller, &count)?;
            let n = if raw_n.is_positive() {
                raw_n.to_usize().unwrap_or(1).min(64)
            } else {
                0
            };
            let side_count = usize::from(sides.get());
            let mut entropy = tape.clone();
            let mut rolls = Vec::with_capacity(n);
            for _ in 0..n {
                let (idx, next) = entropy.uniform_index(side_count)?;
                entropy = next;
                let value = u16::try_from(idx + 1).unwrap_or(sides.get());
                rolls.push(value);
            }
            let cause = EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            };
            let before = self.rules_snapshot(None)?;
            let mut append = RulesJournal::default();
            let occs: Vec<RulesOccurrence> = rolls
                .iter()
                .map(|&result| RulesOccurrence::DieRolled {
                    player,
                    sides: sides.get(),
                    result,
                })
                .collect();
            if !occs.is_empty() {
                self.emit_from(cause, &before, None, &mut append, |_, _, _| Ok(occs))?;
            }
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.last_die_rolls = rolls;
                cur.results.die_roll_ready = true;
            }
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                entropy,
            ));
        }
        if let Some(Instruction::FlipCoins { mode, .. }) = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            .cloned()
        {
            let player = work.chooser;
            let cursor = work.cursor;
            let controller = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            self.pending_work = Some(PendingWork::Resolution(cursor.clone()));
            let mut entropy = tape.clone();
            let mut wins: usize = 0;
            let mut occs = Vec::new();
            match mode {
                crate::program::CoinFlipMode::Single => {
                    let (won, next) = entropy.flip_coin()?;
                    entropy = next;
                    if won {
                        wins = 1;
                    }
                    occs.push(RulesOccurrence::CoinFlipped { player, won });
                }
                crate::program::CoinFlipMode::Count(expr) => {
                    let raw_n = self.resolve_value_expr(controller, &expr)?;
                    let n = if raw_n.is_positive() {
                        raw_n.to_usize().unwrap_or(1).min(256)
                    } else {
                        0
                    };
                    for _ in 0..n {
                        let (won, next) = entropy.flip_coin()?;
                        entropy = next;
                        if won {
                            wins += 1;
                        }
                        occs.push(RulesOccurrence::CoinFlipped { player, won });
                    }
                }
                crate::program::CoinFlipMode::UntilLose => {
                    for _ in 0..256 {
                        let (won, next) = entropy.flip_coin()?;
                        entropy = next;
                        occs.push(RulesOccurrence::CoinFlipped { player, won });
                        if won {
                            wins += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            let cause = EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            };
            let before = self.rules_snapshot(None)?;
            let mut append = RulesJournal::default();
            if !occs.is_empty() {
                self.emit_from(cause, &before, None, &mut append, |_, _, _| Ok(occs))?;
            }
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.last_coin_flips_won = Some(RulesInteger::from_usize(wins));
                cur.results.last_coin_flip_won = Some(wins > 0);
                cur.results.coin_flip_ready = true;
            }
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                entropy,
            ));
        }
        if self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            == Some(&Instruction::ReturnRandomLinkedExile)
        {
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            let candidates = self.exiled_search_results()?;
            let (order, entropy) = tape.shuffled(candidates)?;
            let chosen = *order.first().ok_or(EffectError::InconsistentProgram)?;
            if !self.begin_linked_exile_to_hand(chosen)? {
                return Err(EffectError::InconsistentProgram.into());
            }
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        if let Some(
            instruction @ (Instruction::DiscardOneAtRandom(_)
            | Instruction::SacrificeRandomUntilLand),
        ) = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            .cloned()
        {
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            let candidates = self.random_move_candidates(&instruction)?;
            let (index, entropy) = tape.uniform_index(candidates.len())?;
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .random_selection = Some(candidates[index]);
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        if let Some(Instruction::ReturnRandomGraveyardCards { filter, count, .. }) = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            .cloned()
        {
            let mut candidates =
                self.random_graveyard_candidates(work.chooser, filter.as_deref().copied())?;
            let mut chosen = BTreeSet::new();
            let mut entropy = tape.clone();
            for _ in 0..count.get() {
                let (index, next) = entropy.uniform_index(candidates.len())?;
                chosen.insert(candidates.swap_remove(index));
                entropy = next;
            }
            let mut cursor = work.cursor;
            cursor.results.sampled_graveyard_cards = Some(chosen);
            self.pending_work = Some(PendingWork::Resolution(cursor));
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        if let Some(Instruction::FlipCoin { lose_destination }) = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
        {
            let lose_destination = *lose_destination;
            let (won, entropy) = tape.flip_coin()?;
            let player = work.chooser;
            let cursor = work.cursor;
            let cause = EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            };
            let before = self.rules_snapshot(None)?;
            self.pending_work = Some(PendingWork::Resolution(cursor));
            if won {
                self.advance_instruction()?;
            } else {
                self.set_instruction(lose_destination)?;
            }
            let mut append = RulesJournal::default();
            self.emit_from(cause, &before, None, &mut append, |_, _, _| {
                Ok(vec![RulesOccurrence::CoinFlipped { player, won }])
            })?;
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                entropy,
            ));
        }
        if self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            == Some(&Instruction::RevealCreatureOfSacrificedType)
        {
            let kin = work
                .cursor
                .impulse
                .kin_reveal
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?;
            let (order, entropy) = tape.shuffled(kin.rest.clone())?;
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            let mut journal = RulesJournal::default();
            self.finish_kin_reveal(order, &mut journal)?;
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    journal,
                )?,
                entropy,
            ));
        }
        if self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction))
            == Some(&Instruction::ExileNonlandForFormerControllers)
        {
            let rewrite = work
                .cursor
                .impulse
                .rewrite
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?;
            let (order, entropy) = tape.shuffled(rewrite.lands.clone())?;
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            self.begin_rewrite_bottom(order)?;
            return Ok((
                self.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )?,
                entropy,
            ));
        }
        let ids = self.cascade_remaining(work.cursor.entry)?;
        let (order, entropy) = tape.shuffled(ids)?;
        let cursor = work.cursor;
        self.pending_work = Some(PendingWork::Resolution(cursor.clone()));
        let mut append = RulesJournal::default();
        self.return_cascade_cards(&cursor, &order, &mut append)?;
        Ok((
            self.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                append,
            )?,
            entropy,
        ))
    }
}

fn below_cascade_limit(value: &RulesInteger, limit: &RulesInteger) -> bool {
    value < limit
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_exiled_front_face_and_chosen_spell_must_each_be_below_the_original_value() {
        // The current engine admits only a single represented face. The second check also
        // rules out a more expensive result when a future alternate-face cast is offered.
        let lower = 2.into();
        let limit = 4.into();
        let higher = 7.into();
        assert!(below_cascade_limit(&lower, &limit));
        assert!(!below_cascade_limit(&higher, &limit));
        assert!(!below_cascade_limit(&limit, &limit));
    }
}
