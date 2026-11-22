//! One root result owner; terminal publication never resumes child rules work.
use super::*;

/// Provenance of the restricted root result, distinct from prospective SBA work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameConclusionCause {
    /// Actual committed checked losses, preserving their original occurrences.
    CheckedLosses { players: BTreeSet<SeatId> },
    /// The acting player conceded, leaving one participant.
    Conceded { player: SeatId },
    /// A rules effect ended the game with an immediate winner.
    WonByEffect {
        winner: SeatId,
        source: crate::object_identity::ObjectId,
        other_players: BTreeSet<SeatId>,
    },
}

/// Privately constructed terminal result owned by one accepted root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConclusion {
    face_down_reveals: Vec<crate::object_identity::GameObject>,
    outcome: GameOutcome,
    cause: GameConclusionCause,
    earlier_departures: BTreeSet<SeatId>,
}
impl GameConclusion {
    /// Exact face-down spells and permanents revealed to all players at game end (CR 708.9).
    pub fn face_down_reveals(&self) -> &[crate::object_identity::GameObject] {
        &self.face_down_reveals
    }

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
        let checked = match &self.cause {
            GameConclusionCause::CheckedLosses { players }
            | GameConclusionCause::WonByEffect {
                other_players: players,
                ..
            } => Some(players),
            GameConclusionCause::Conceded { .. } => None,
        };
        let conceded = match self.cause {
            GameConclusionCause::Conceded { player } => Some(player),
            GameConclusionCause::CheckedLosses { .. } | GameConclusionCause::WonByEffect { .. } => {
                None
            }
        };
        self.earlier_departures.iter().copied().chain(
            checked
                .into_iter()
                .flat_map(|players| players.iter().copied())
                .chain(conceded),
        )
    }
}

impl GameState {
    fn terminal_face_down_reveals(&self) -> Vec<crate::object_identity::GameObject> {
        self.execution
            .window()
            .cards()
            .iter()
            .filter(|o| {
                (o.face_down().is_some()
                    && matches!(o.zone(), ObjectZone::Stack | ObjectZone::Battlefield))
                    || self.execution.foretold_on(o.id()).is_some()
            })
            .cloned()
            .collect()
    }

    /// Publish a win at the cut that created it, before further work or state checks.
    pub(super) fn finalize_effect_win(&mut self, facts: &mut TransitionFacts) -> bool {
        let Some((winner, source)) = self.execution.won_by_effect else {
            return false;
        };
        let other_players = self
            .execution
            .window()
            .original_participants()
            .iter()
            .copied()
            .filter(|&player| player != winner)
            .collect();
        let conclusion = GameConclusion {
            face_down_reveals: self.terminal_face_down_reveals(),
            outcome: GameOutcome::Winner(winner),
            cause: GameConclusionCause::WonByEffect {
                winner,
                source,
                other_players,
            },
            earlier_departures: BTreeSet::new(),
        };
        self.pending = None;
        self.conclusion = Some(conclusion.clone());
        facts
            .rules_journal
            .records
            .push(crate::occurrence::RulesRecord::GameConcluded { conclusion });
        true
    }

    /// A checked loss ends the game once at most one participant survives.
    pub(super) fn finalize_checked_losses(&mut self) {
        if self.conclusion.is_some() {
            return;
        }
        let order = self.execution.window().participants();
        let losses = self.execution.state_based_losses();
        if !losses.is_empty() && order.iter().filter(|seat| !losses.contains(seat)).count() <= 1 {
            let winner = order.iter().copied().find(|seat| !losses.contains(seat));
            let outcome = winner.map_or(GameOutcome::Draw, GameOutcome::Winner);
            self.conclusion = Some(GameConclusion {
                face_down_reveals: self.terminal_face_down_reveals(),
                outcome,
                cause: GameConclusionCause::CheckedLosses {
                    players: self
                        .execution
                        .window()
                        .original_participants()
                        .iter()
                        .copied()
                        .filter(|seat| Some(*seat) != winner)
                        .collect(),
                },
                earlier_departures: BTreeSet::new(),
            });
        }
    }

    /// Called only after exact basis, actor and terminal authentication.
    pub(super) fn concede(&self, player: SeatId) -> Result<NativeCandidate, GameError> {
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        let [first, second] = self.execution.window().participants() else {
            if self.counter_replacement.is_some() {
                return Err(GameError::UnsupportedConcessionScope);
            }
            let issued = self
                .pending
                .as_ref()
                .and_then(|pending| pending.frame.effect_decision());
            let mut next = self.clone();
            next.basis.revision = revision;
            next.pending = None;
            next.face_down_reveals.clear();
            next.cascade_reveals.clear();
            if let Some(opening) = next.opening.as_mut().filter(|opening| !opening.complete()) {
                opening.remove_player(&self.opening_order()?, player)?;
            }
            let mut facts = TransitionFacts::default();
            next.execution
                .concede_with_continuation(
                    &self.execution,
                    player,
                    issued.as_ref(),
                    next.turn.as_ref(),
                    &mut facts.rules_journal,
                )
                .map_err(|error| match error {
                    EffectError::WrongPhase => GameError::UnsupportedConcessionScope,
                    error => error.into(),
                })?;
            if let Some(progress) = next
                .execution
                .progress_departure_continuation(next.turn.as_ref())?
            {
                next.accept_rule_progress(progress, &mut facts)?;
            }
            let mut next = next.drive(&mut facts)?;
            if next.pending_decision().is_some_and(|decision| {
                !next
                    .execution
                    .window()
                    .participants()
                    .contains(&decision.chooser())
            }) {
                return Err(GameError::UnsupportedConcessionScope);
            }
            next.capture_face_down_reveals(&facts.rules_journal);
            return Ok(NativeCandidate {
                base: CandidateBase::Advance(self.basis.clone()),
                next,
                facts,
            });
        };
        let winner = if player == *first { *second } else { *first };
        let conclusion = GameConclusion {
            face_down_reveals: self.terminal_face_down_reveals(),
            outcome: GameOutcome::Winner(winner),
            cause: GameConclusionCause::Conceded { player },
            earlier_departures: self
                .execution
                .window()
                .original_participants()
                .iter()
                .copied()
                .filter(|seat| !self.execution.window().participants().contains(seat))
                .collect(),
        };
        let mut next = self.clone();
        next.basis.revision = revision;
        next.face_down_reveals.clear();
        next.cascade_reveals.clear();
        next.conclusion = Some(conclusion.clone());
        let mut facts = TransitionFacts::default();
        facts
            .rules_journal
            .records
            .push(crate::occurrence::RulesRecord::GameConcluded { conclusion });
        Ok(NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            facts,
        })
    }
}
