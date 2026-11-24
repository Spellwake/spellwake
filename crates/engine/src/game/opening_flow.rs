//! Opening orchestration below the single GameState input/candidate owner.
use super::*;
#[cfg(test)]
use crate::combat::CombatRules;

impl GameState {
    pub(super) fn opening_order(&self) -> Result<Vec<SeatId>, GameError> {
        let order = self.execution.window().participants();
        let start = order
            .iter()
            .position(|p| *p == self.execution.window().active_player())
            .ok_or(GameError::UnexpectedKernelPhase)?;
        Ok(order
            .iter()
            .cycle()
            .skip(start)
            .take(order.len())
            .copied()
            .collect())
    }
    fn opening_hand(&self, player: SeatId) -> Vec<ObjectId> {
        self.execution
            .window()
            .cards()
            .iter()
            .filter(|object| object.zone() == ObjectZone::Hand(player))
            .map(|object| object.id())
            .collect()
    }
    pub(super) fn shuffle_opening(
        &mut self,
        player: SeatId,
        phase: OpeningShufflePhase,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let entropy = self.random.as_ref().ok_or(OpeningError::NotOpening)?;
        let transition = self.execution.opening_shuffle_with_phase(
            self.execution.revision(),
            player,
            entropy,
            phase,
        )?;
        let (entropy, record) = self.accept_execution(transition, facts);
        self.random = Some(entropy);
        facts.shuffle_journal.push(record);
        Ok(())
    }
    pub(super) fn draw_opening(
        &mut self,
        player: SeatId,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let transition = self
            .execution
            .draw_cards(self.execution.revision(), player, 7.into())?;
        let draws = self.accept_execution(transition, facts);
        facts.draw_journal.append(draws);
        Ok(())
    }
    fn opening_pregame_eligible(&self, player: SeatId) -> Vec<ObjectId> {
        self.opening_hand(player)
            .into_iter()
            .filter(|&id| {
                self.execution
                    .copiable_program(id)
                    .is_some_and(|p| p.opening_hand_battlefield)
            })
            .collect()
    }
    fn opening_decision(&self) -> Result<OpeningDecision, GameError> {
        let opening = self.opening.as_ref().ok_or(OpeningError::NotOpening)?;
        match &opening.stage {
            OpeningStage::Declare { cursor, .. } => {
                let player = *self
                    .opening_order()?
                    .get(*cursor)
                    .ok_or(OpeningError::WrongStage)?;
                Ok(OpeningDecision::Declare {
                    player,
                    taken: opening.mulligans_taken(player),
                    can_mulligan: opening.can_mulligan(player),
                })
            }
            OpeningStage::Bottom { cohort, cursor, .. } => {
                let player = *cohort.get(*cursor).ok_or(OpeningError::WrongStage)?;
                Ok(OpeningDecision::Bottom {
                    player,
                    count: opening.charged(player),
                    hand: self.opening_hand(player),
                })
            }
            OpeningStage::PregameBattlefield { cursor, .. } => {
                let player = *self
                    .opening_order()?
                    .get(*cursor)
                    .ok_or(OpeningError::WrongStage)?;
                Ok(OpeningDecision::PregameBattlefield {
                    player,
                    eligible: self.opening_pregame_eligible(player),
                })
            }
            _ => Err(OpeningError::WrongStage.into()),
        }
    }
    pub(super) fn reply_opening(
        &mut self,
        frame: &OpeningDecision,
        reply: DecisionReply,
    ) -> Result<(), GameError> {
        if &self.opening_decision()? != frame {
            return Err(OpeningError::WrongStage.into());
        }
        match (frame, reply) {
            (
                OpeningDecision::Declare {
                    player,
                    can_mulligan,
                    ..
                },
                DecisionReply::Mulligan(choice),
            ) => {
                if choice == MulliganChoice::Mulligan && !can_mulligan {
                    return Err(OpeningError::IllegalMulligan.into());
                }
                let opening = self.opening.as_mut().ok_or(OpeningError::NotOpening)?;
                if let OpeningStage::Declare { cursor, requested } = &mut opening.stage {
                    if choice == MulliganChoice::Mulligan {
                        requested.push(*player);
                    } else {
                        opening.kept.insert(*player);
                    }
                    *cursor += 1;
                } else {
                    return Err(OpeningError::WrongStage.into());
                }
            }
            (
                OpeningDecision::Bottom {
                    player,
                    count,
                    hand,
                },
                DecisionReply::Bottom(selected),
            ) => {
                let chosen: BTreeSet<_> = selected.iter().copied().collect();
                if selected.len() != *count
                    || chosen.len() != selected.len()
                    || !chosen.iter().all(|id| hand.contains(id))
                {
                    return Err(OpeningError::InvalidBottom.into());
                }
                let opening = self.opening.as_mut().ok_or(OpeningError::NotOpening)?;
                if let OpeningStage::Bottom {
                    cursor,
                    selected: staged,
                    ..
                } = &mut opening.stage
                {
                    staged.insert(*player, selected);
                    *cursor += 1;
                } else {
                    return Err(OpeningError::WrongStage.into());
                }
            }
            (
                OpeningDecision::PregameBattlefield { player, eligible },
                DecisionReply::OpeningBattlefield(selected),
            ) => {
                let chosen: BTreeSet<_> = selected.iter().copied().collect();
                if chosen.len() != selected.len() || !chosen.iter().all(|id| eligible.contains(id))
                {
                    return Err(OpeningError::InvalidBottom.into());
                }
                let opening = self.opening.as_mut().ok_or(OpeningError::NotOpening)?;
                if let OpeningStage::PregameBattlefield { pending, .. } = &mut opening.stage {
                    *pending = Some((*player, selected));
                } else {
                    return Err(OpeningError::WrongStage.into());
                }
            }
            _ => return Err(OpeningError::WrongReply.into()),
        }
        Ok(())
    }
    pub(super) fn drive_opening(&mut self, facts: &mut TransitionFacts) -> Result<(), GameError> {
        loop {
            let stage = self
                .opening
                .as_ref()
                .ok_or(OpeningError::NotOpening)?
                .stage
                .clone();
            match stage {
                OpeningStage::Declare { cursor, requested } => {
                    let order = self.opening_order()?;
                    if let Some(&player) = order.get(cursor) {
                        let opening = self.opening.as_mut().ok_or(OpeningError::NotOpening)?;
                        if opening.kept(player) || !opening.can_mulligan(player) {
                            opening.kept.insert(player);
                            opening.stage = OpeningStage::Declare {
                                cursor: cursor + 1,
                                requested,
                            };
                            continue;
                        }
                        self.bind_decision(RuleFrame::Opening(self.opening_decision()?))?;
                        return Ok(());
                    }
                    if requested.is_empty() {
                        self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                            OpeningStage::PregameBattlefield {
                                cursor: 0,
                                pending: None,
                            };
                        continue;
                    }
                    // Bracket the complete simultaneous return cohort with actual receipts.
                    let selected = requested
                        .iter()
                        .map(|player| (*player, self.opening_hand(*player)))
                        .collect::<Vec<_>>();
                    let transition = self.execution.opening_move_cohort(
                        self.execution.revision(),
                        OpeningMovePhase::ReturnForMulligan,
                        &requested,
                        &selected,
                    )?;
                    self.accept_execution(transition, facts);
                    for &player in &requested {
                        self.shuffle_opening(player, OpeningShufflePhase::Mulligan, facts)?;
                    }
                    for &player in &requested {
                        self.draw_opening(player, facts)?;
                    }
                    let opening = self.opening.as_mut().ok_or(OpeningError::NotOpening)?;
                    for &player in &requested {
                        let count = opening
                            .mulligans_taken(player)
                            .checked_add(1)
                            .ok_or(OpeningError::IllegalMulligan)?;
                        opening.taken.insert(player, count);
                    }
                    opening.stage = OpeningStage::Bottom {
                        cohort: requested,
                        cursor: 0,
                        selected: BTreeMap::new(),
                    };
                }
                OpeningStage::Bottom {
                    cohort,
                    cursor,
                    selected,
                } => {
                    if let Some(&player) = cohort.get(cursor) {
                        if self
                            .opening
                            .as_ref()
                            .ok_or(OpeningError::NotOpening)?
                            .charged(player)
                            == 0
                        {
                            self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                                OpeningStage::Bottom {
                                    cohort,
                                    cursor: cursor + 1,
                                    selected,
                                };
                            continue;
                        }
                        self.bind_decision(RuleFrame::Opening(self.opening_decision()?))?;
                        return Ok(());
                    }
                    let mut rows = Vec::new();
                    for &player in &cohort {
                        let charge = self
                            .opening
                            .as_ref()
                            .ok_or(OpeningError::NotOpening)?
                            .charged(player);
                        if charge > 0 {
                            rows.push((
                                player,
                                selected
                                    .get(&player)
                                    .ok_or(OpeningError::InvalidBottom)?
                                    .clone(),
                            ));
                        }
                    }
                    if !rows.is_empty() {
                        let transition = self.execution.opening_move_cohort(
                            self.execution.revision(),
                            OpeningMovePhase::ChargedBottom,
                            &cohort,
                            &rows,
                        )?;
                        self.accept_execution(transition, facts);
                    }
                    self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                        OpeningStage::Declare {
                            cursor: 0,
                            requested: vec![],
                        };
                }
                OpeningStage::PregameBattlefield { cursor, pending } => {
                    if let Some((player, selected)) = pending {
                        if !selected.is_empty() {
                            let transition = self.execution.opening_enter_battlefield(
                                self.execution.revision(),
                                player,
                                &selected,
                            )?;
                            self.accept_execution(transition, facts);
                        }
                        self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                            OpeningStage::PregameBattlefield {
                                cursor: cursor + 1,
                                pending: None,
                            };
                        continue;
                    }
                    let order = self.opening_order()?;
                    let Some(&player) = order.get(cursor) else {
                        self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                            OpeningStage::Complete;
                        continue;
                    };
                    if self.opening_pregame_eligible(player).is_empty() {
                        self.opening.as_mut().ok_or(OpeningError::NotOpening)?.stage =
                            OpeningStage::PregameBattlefield {
                                cursor: cursor + 1,
                                pending: None,
                            };
                        continue;
                    }
                    self.bind_decision(RuleFrame::Opening(self.opening_decision()?))?;
                    return Ok(());
                }
                OpeningStage::Complete => {
                    if self.turn.is_some() {
                        return Err(OpeningError::WrongStage.into());
                    }
                    let schedule = TurnScheduler::new(
                        TurnStart::default(),
                        self.execution.window().original_participants().len() == 2,
                    )?;
                    let active = self.execution.window().active_player();
                    let transition = self.execution.enter_calendar(
                        self.execution.revision(),
                        None,
                        &schedule,
                        active,
                    )?;
                    self.accept_execution(transition, facts);
                    facts.turn_transitions.push(TurnTransition {
                        from: None,
                        to: schedule.position(),
                        active_before: active,
                        active_after: active,
                    });
                    self.turn = Some(schedule);
                    return Ok(());
                }
            }
        }
    }
    /// Host-only native entropy append. A future authenticated host must serialize
    /// acceptance; no player command, entropy source guarantee or wire API exists.
    pub fn supply_entropy(
        &self,
        basis: &GameBasis,
        words: Vec<u64>,
    ) -> Result<NativeCandidate, GameError> {
        self.validate_basis(basis)?;
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        if !self.execution.state_based_losses().is_empty() {
            return Err(GameError::PlayerEliminationRequired);
        }
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        let empty = EntropyTape::new(vec![]);
        let searching = matches!(self.pending.as_ref().map(|p| &p.frame), Some(RuleFrame::LibraryTop(frame)) if frame.kind() == crate::effects::LibraryChoiceKind::Search);
        let random = self
            .random
            .as_ref()
            .or((searching || self.execution.has_stacked_random_operation()).then_some(&empty))
            .ok_or(OpeningError::NotOpening)?
            .appended(words)?;
        let mut next = self.clone();
        next.cascade_reveals.clear();
        next.random = Some(random);
        next.basis.revision = revision;
        if let Some(pending) = &self.pending {
            next.bind_decision(pending.frame.clone())?;
        }
        next.finalize_checked_losses();
        Ok(NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            facts: TransitionFacts::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        effects::Program,
        object_identity::{CardId, ObjectDestination},
    };
    fn fixture(words: usize) -> GameState {
        let mut cards = vec![];
        for player in [SeatId(0), SeatId(1)] {
            for index in 0..12 {
                cards.push(SeedCard {
                    card: CardId(u64::from(player.0) * 100 + index + 1),
                    owner: player,
                    destination: ObjectDestination::LibraryBottom,
                    program: Program::creature(
                        None,
                        1.into(),
                        1.into(),
                        std::collections::BTreeSet::new(),
                    ),
                });
            }
        }
        GameState::initialize_opening(
            GameId(1200),
            GameContext::new(
                "r".into(),
                "cr".into(),
                "c".into(),
                "p".into(),
                "opening".into(),
                crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            OpeningSetup {
                seed: GameSeed::with_uniform_life(
                    vec![SeatId(0), SeatId(1)],
                    SeatId(0),
                    cards,
                    vec![],
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                ),
                entropy: EntropyTape::new(vec![0; words]),
            },
        )
        .unwrap()
        .into_state()
    }
    fn input(state: &GameState, choice: DecisionReply) -> GameInput {
        let decision = state.pending_decision().unwrap();
        GameInput {
            basis: state.basis().clone(),
            actor: decision.chooser(),
            command: GameCommand::Reply {
                decision: decision.id(),
                choice,
            },
        }
    }
    fn reply(state: &GameState, choice: DecisionReply) -> GameState {
        state.advance(input(state, choice)).unwrap().into_state()
    }
    #[test]
    fn failed_last_declaration_keeps_prior_vote_and_all_accepted_physical_state() {
        for fault in 0..7 {
            let initial = fixture(if fault == 0 { 22 } else { 1000 });
            let mut state = reply(&initial, DecisionReply::Mulligan(MulliganChoice::Mulligan));
            match fault {
                0 => {}
                1 => state
                    .execution
                    .set_test_object_allocator(u64::MAX - 8, true),
                2 => state
                    .execution
                    .set_test_object_allocator(u64::MAX - 8, false),
                3 => state.execution.set_test_window_revision(u64::MAX - 2),
                4 => state.execution.set_test_revision(u64::MAX - 2),
                5 => state.next_decision = u64::MAX,
                _ => state.basis.revision = u64::MAX,
            }
            let before = state.clone();
            assert!(
                state
                    .advance(input(&state, DecisionReply::Mulligan(MulliganChoice::Keep)))
                    .is_err()
            );
            assert_eq!(state, before);
            assert_eq!(state.opening().unwrap().mulligans_taken(SeatId(0)), 0);
            assert_eq!(state.opening_hand(SeatId(0)).len(), 7);
        }
    }
    #[test]
    fn failed_last_bottom_preserves_earlier_staged_choice_and_both_seven_card_hands() {
        for token_fault in [false, true] {
            let mut state = fixture(1000);
            for _ in 0..2 {
                state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Mulligan));
            }
            let a = state.opening_hand(SeatId(0))[0];
            state = reply(&state, DecisionReply::Bottom(vec![a]));
            let b = state.opening_hand(SeatId(1))[0];
            if token_fault {
                state.next_decision = u64::MAX;
            } else {
                state
                    .execution
                    .set_test_object_allocator(u64::MAX - 1, true);
            }
            let before = state.clone();
            assert!(
                state
                    .advance(input(&state, DecisionReply::Bottom(vec![b])))
                    .is_err()
            );
            assert_eq!(state, before);
            assert_eq!(state.opening_hand(SeatId(0)).len(), 7);
            assert_eq!(state.opening_hand(SeatId(1)).len(), 7);
        }
    }
    #[test]
    fn entropy_supply_failure_does_not_append_or_replace_the_current_decision() {
        let mut state = fixture(22);
        state.next_decision = u64::MAX;
        let before = state.clone();
        assert_eq!(
            state.supply_entropy(state.basis(), vec![42]),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, before);
    }
}
