//! One root result owner; terminal publication never resumes child rules work.
use super::*;

/// Provenance of the restricted root result, distinct from prospective SBA work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameConclusionCause {
    /// Actual committed checked losses, preserving their original occurrences.
    CheckedLosses { players: BTreeSet<SeatId> },
    /// A checked loss among the final two survivors, after earlier departures.
    CheckedLossesAfterDepartures {
        players: BTreeSet<SeatId>,
        earlier_departures: BTreeSet<SeatId>,
    },
    /// The acting player conceded an originally two-player game.
    Conceded { player: SeatId },
    /// The final opponent conceded after earlier multiplayer departures.
    ConcededLastSurvivor {
        player: SeatId,
        earlier_departures: BTreeSet<SeatId>,
    },
    /// A multiplayer response deadline removed the final opponent.
    ResponseExpiredLastSurvivor {
        player: SeatId,
        earlier_departures: BTreeSet<SeatId>,
    },
    /// The trusted clock owner expired this player's whole-match bank.
    BankExpired { player: SeatId },
}

/// Deterministic input from a trusted, independently authenticated clock owner.
///
/// This value is not evidence that wall time elapsed or that a bank is empty.
/// The application must authenticate the exact expiry and policy atomically before
/// accepting or installing this tentative candidate. It is intentionally absent from player commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankExpiryInput {
    /// Exact accepted native root to which the authenticated expiry applies.
    pub basis: GameBasis,
    /// Native participant whose whole-match bank expired.
    pub player: SeatId,
}

/// Privately constructed terminal result owned by one accepted root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConclusion {
    outcome: GameOutcome,
    cause: GameConclusionCause,
}
impl GameConclusion {
    /// Existing restricted winner/draw vocabulary, derived by the root owner.
    pub fn outcome(&self) -> GameOutcome {
        self.outcome
    }
    /// Actual terminal cause; checked losses are never inferred from raw life.
    pub fn cause(&self) -> &GameConclusionCause {
        &self.cause
    }
    /// Deterministic losing seats without another stored loss set.
    pub fn losing_players(&self) -> impl Iterator<Item = SeatId> + '_ {
        let (earlier, checked, final_loser) = match &self.cause {
            GameConclusionCause::CheckedLosses { players } => (None, Some(players), None),
            GameConclusionCause::CheckedLossesAfterDepartures {
                players,
                earlier_departures,
            } => (Some(earlier_departures), Some(players), None),
            GameConclusionCause::ConcededLastSurvivor {
                player,
                earlier_departures,
            }
            | GameConclusionCause::ResponseExpiredLastSurvivor {
                player,
                earlier_departures,
            } => (Some(earlier_departures), None, Some(*player)),
            GameConclusionCause::Conceded { player }
            | GameConclusionCause::BankExpired { player } => (None, None, Some(*player)),
        };
        earlier
            .into_iter()
            .chain(checked)
            .flat_map(|players| players.iter().copied())
            .chain(final_loser)
    }
}

#[derive(Clone, Copy)]
enum DepartureReason {
    Concession,
    ResponseTimeout,
}

impl GameState {
    /// Settle a simultaneous checked loss leaving one winner or an empty roster.
    pub(super) fn finalize_checked_losses(&mut self) {
        if self.conclusion.is_some() {
            return;
        }
        let order = self.execution.window().participants();
        let losses = self.execution.state_based_losses();
        if !losses.is_empty()
            && order.iter().filter(|seat| !losses.contains(seat)).count() <= 1
            && self
                .original_participants
                .iter()
                .all(|seat| self.execution.window().known_player(*seat))
        {
            let outcome = order
                .iter()
                .find(|seat| !losses.contains(seat))
                .map_or(GameOutcome::Draw, |winner| GameOutcome::Winner(*winner));
            self.conclusion = Some(GameConclusion {
                outcome,
                cause: if self.original_participants.len() == 2 {
                    GameConclusionCause::CheckedLosses {
                        players: losses.clone(),
                    }
                } else {
                    GameConclusionCause::CheckedLossesAfterDepartures {
                        players: losses.clone(),
                        earlier_departures: self
                            .original_participants
                            .iter()
                            .copied()
                            .filter(|seat| !order.contains(seat))
                            .collect(),
                    }
                },
            });
        }
    }

    /// Called only after exact basis, actor and terminal authentication.
    pub(super) fn concede(&self, player: SeatId) -> Result<NativeCandidate, GameError> {
        self.depart(player, DepartureReason::Concession)
    }

    /// Propose the departure of the current multiplayer responder. The host owns
    /// wall time and must authenticate expiry before committing the candidate.
    /// # Errors
    /// Rejects stale bases, inactive seats, two-player games, a different waiter,
    /// terminal roots and departures the current rules frame cannot preserve.
    pub fn expire_response(
        &self,
        basis: &GameBasis,
        player: SeatId,
    ) -> Result<NativeCandidate, GameError> {
        self.validate_basis(basis)?;
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        if self.original_participants.len() < 3 {
            return Err(GameError::UnsupportedResponseExpiryScope);
        }
        if !self.execution.window().participants().contains(&player) {
            return Err(GameError::UnknownActor);
        }
        if !self.is_waiting_for(player) {
            return Err(GameError::ResponsePlayerNotWaiting);
        }
        self.depart(player, DepartureReason::ResponseTimeout)
    }

    fn depart(
        &self,
        player: SeatId,
        reason: DepartureReason,
    ) -> Result<NativeCandidate, GameError> {
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        if self.original_participants.len() != 2 {
            if !self.execution.state_based_losses().is_empty() {
                return Err(GameError::UnsupportedConcessionScope);
            }
            if let [first, second] = self.execution.window().participants() {
                let winner = if player == *first { *second } else { *first };
                let earlier_departures = self
                    .original_participants
                    .iter()
                    .copied()
                    .filter(|seat| !self.execution.window().participants().contains(seat))
                    .collect();
                return Ok(self.conclusion_candidate(
                    revision,
                    GameConclusion {
                        outcome: GameOutcome::Winner(winner),
                        cause: match reason {
                            DepartureReason::Concession => {
                                GameConclusionCause::ConcededLastSurvivor {
                                    player,
                                    earlier_departures,
                                }
                            }
                            DepartureReason::ResponseTimeout => {
                                GameConclusionCause::ResponseExpiredLastSurvivor {
                                    player,
                                    earlier_departures,
                                }
                            }
                        },
                    },
                ));
            }
            let issued_continuation = if matches!(
                self.execution.pending_work_kind(),
                Some(
                    crate::effects::PendingWorkKind::Resolution
                        | crate::effects::PendingWorkKind::TokenEntry
                        | crate::effects::PendingWorkKind::LibraryTop
                        | crate::effects::PendingWorkKind::ResolutionSelection
                        | crate::effects::PendingWorkKind::Checks
                        | crate::effects::PendingWorkKind::Combat
                )
            ) {
                let pending = self
                    .pending
                    .as_ref()
                    .ok_or(GameError::UnsupportedConcessionScope)?;
                if pending.basis != self.basis {
                    return Err(GameError::UnsupportedConcessionScope);
                }
                Some(
                    pending
                        .frame
                        .effect_decision()
                        .ok_or(GameError::UnsupportedConcessionScope)?,
                )
            } else {
                None
            };
            let transition = self
                .execution
                .depart_players(
                    &BTreeSet::from([player]),
                    self.turn.as_ref(),
                    issued_continuation.as_ref(),
                )
                .map_err(|error| match error {
                    EffectError::UnsupportedDepartureState => GameError::UnsupportedConcessionScope,
                    error => error.into(),
                })?;
            let mut next = self.clone();
            next.basis.revision = revision;
            // The executor has preserved or ceased its exact continuation. Only
            // the old root response capability is retired, never the cursor.
            next.pending = None;
            if let Some(opening) = next.opening.as_mut().filter(|opening| !opening.complete()) {
                opening.remove_player(&self.opening_order()?, player)?;
            }
            let mut facts = TransitionFacts::default();
            next.accept_execution(transition, &mut facts);
            if let Some(frame) = next.execution.pending_mana_color().cloned() {
                next.bind_decision(RuleFrame::ManaColor(frame))?;
            } else if matches!(
                next.execution.pending_work_kind(),
                Some(
                    crate::effects::PendingWorkKind::Resolution
                        | crate::effects::PendingWorkKind::TokenEntry
                        | crate::effects::PendingWorkKind::LibraryTop
                        | crate::effects::PendingWorkKind::ResolutionSelection
                        | crate::effects::PendingWorkKind::Checks
                        | crate::effects::PendingWorkKind::Combat
                )
            ) {
                let progress = next.execution.progress_departure_work(next.turn.as_ref())?;
                next.accept_rule_progress(progress, &mut facts)?;
            }
            let next = next.drive(&mut facts)?;
            return Ok(NativeCandidate {
                base: CandidateBase::Advance(self.basis.clone()),
                next,
                facts,
            });
        }
        let [first, second] = self.execution.window().participants() else {
            return Err(GameError::UnsupportedConcessionScope);
        };
        let winner = if player == *first { *second } else { *first };
        let conclusion = GameConclusion {
            outcome: GameOutcome::Winner(winner),
            cause: GameConclusionCause::Conceded { player },
        };
        Ok(self.conclusion_candidate(revision, conclusion))
    }

    /// Tentatively expire a trusted whole-match bank at the exact current root.
    ///
    /// No wall clock, bank balance, authorization, durable acceptance or timeout
    /// player action is supplied here. The caller must establish those facts at acceptance from
    /// the canonical clock source before installing this tentative candidate. Only the currently waiting participant of an
    /// originally two-player game is supported by this native transition.
    ///
    /// # Errors
    /// Rejects foreign/stale basis, unknown participant, terminal game, revision
    /// exhaustion, unsupported participant scope or a participant not currently
    /// waiting for a native decision or priority. Rejection leaves the root intact.
    pub fn expire_bank(&self, input: BankExpiryInput) -> Result<NativeCandidate, GameError> {
        self.validate_basis(&input.basis)?;
        let participants = self.execution.window().participants();
        if !participants.contains(&input.player) {
            return Err(GameError::UnknownActor);
        }
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        if self.original_participants.len() != 2 {
            return Err(GameError::UnsupportedBankExpiryScope);
        }
        let [first, second] = participants else {
            return Err(GameError::UnsupportedBankExpiryScope);
        };
        if !self.is_waiting_for(input.player) {
            return Err(GameError::BankPlayerNotWaiting);
        }
        let winner = if input.player == *first {
            *second
        } else {
            *first
        };
        Ok(self.conclusion_candidate(
            revision,
            GameConclusion {
                outcome: GameOutcome::Winner(winner),
                cause: GameConclusionCause::BankExpired {
                    player: input.player,
                },
            },
        ))
    }

    fn is_waiting_for(&self, player: SeatId) -> bool {
        if let Some(decision) = &self.pending {
            decision.basis == self.basis && decision.chooser == player
        } else {
            self.pending_turn_work().is_none()
                && self.execution.pending_work_kind().is_none()
                && matches!(
                    self.execution.window().phase(),
                    crate::stack_window::WindowPhase::Priority(
                        crate::priority::PriorityPhase::Ready { holder }
                    ) if holder == player
                )
        }
    }

    /// One root terminal-candidate owner shared by distinct authenticated causes.
    fn conclusion_candidate(&self, revision: u64, conclusion: GameConclusion) -> NativeCandidate {
        let mut next = self.clone();
        next.basis.revision = revision;
        next.conclusion = Some(conclusion.clone());
        let mut facts = TransitionFacts::default();
        facts
            .rules_journal
            .records
            .push(crate::occurrence::RulesRecord::GameConcluded { conclusion });
        NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            facts,
        }
    }
}
