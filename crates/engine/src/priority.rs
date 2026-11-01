//! Priority-round bookkeeping for individual-turn games (CR 117.3c–117.5).
//! The enclosing kernel owns legality, state-based actions, triggers and stack resolution.
use std::collections::BTreeSet;

/// Stable player identity within a game, distinct from a controller/session identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeatId(pub u32);

/// The next rule-processing boundary. A checks-required seat does not yet have priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityPhase {
    RequiresChecks { candidate: SeatId },
    Ready { holder: SeatId },
    AllPassed,
}

/// Immutable bookkeeping for one stable participant order and priority round.
/// Shared-team turns and changes to the participant order require their own integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityRound {
    order: Vec<SeatId>,
    revision: u64,
    consecutive_passes: usize,
    phase: PriorityPhase,
}

/// Typed rejection or operational revision exhaustion; no rule outcome is invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityError {
    InvalidOrder,
    UnknownFirstPlayer,
    StaleRevision,
    ChecksRequired,
    ChecksNotPending,
    WrongPlayer,
    RoundClosed,
    RevisionExhausted,
}

impl PriorityRound {
    /// Open a round at a kernel-selected candidate (for example, the active player).
    /// This does not grant priority until the enclosing kernel completes CR 117.5 checks.
    pub fn new(order: Vec<SeatId>, first: SeatId, revision: u64) -> Result<Self, PriorityError> {
        let unique: BTreeSet<_> = order.iter().copied().collect();
        if !(2..=6).contains(&order.len()) || unique.len() != order.len() {
            return Err(PriorityError::InvalidOrder);
        }
        if !unique.contains(&first) {
            return Err(PriorityError::UnknownFirstPlayer);
        }
        Ok(Self {
            order,
            revision,
            consecutive_passes: 0,
            phase: PriorityPhase::RequiresChecks { candidate: first },
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    pub fn phase(&self) -> PriorityPhase {
        self.phase
    }
    pub fn consecutive_passes(&self) -> usize {
        self.consecutive_passes
    }

    fn next_revision(&self, expected: u64) -> Result<u64, PriorityError> {
        if expected != self.revision {
            return Err(PriorityError::StaleRevision);
        }
        self.revision
            .checked_add(1)
            .ok_or(PriorityError::RevisionExhausted)
    }

    /// Trusted kernel continuation after state-based actions and waiting triggers settle.
    /// Not a player command. If those checks change the participant order or invalidate
    /// this round, the enclosing kernel must establish the appropriate new round instead.
    pub fn finish_checks(&self, expected: u64) -> Result<Self, PriorityError> {
        let revision = self.next_revision(expected)?;
        let PriorityPhase::RequiresChecks { candidate } = self.phase else {
            return Err(PriorityError::ChecksNotPending);
        };
        Ok(Self {
            revision,
            phase: PriorityPhase::Ready { holder: candidate },
            ..self.clone()
        })
    }

    fn validate_holder(&self, player: SeatId) -> Result<(), PriorityError> {
        match self.phase {
            PriorityPhase::RequiresChecks { .. } => Err(PriorityError::ChecksRequired),
            PriorityPhase::AllPassed => Err(PriorityError::RoundClosed),
            PriorityPhase::Ready { holder } if holder != player => Err(PriorityError::WrongPlayer),
            PriorityPhase::Ready { .. } => Ok(()),
        }
    }

    /// Record a pass by the current player. The last consecutive pass closes this
    /// round: the enclosing kernel resolves the stack top or ends the step (CR 117.4).
    /// The round itself never resolves an effect or advances a turn.
    pub fn pass(&self, expected: u64, player: SeatId) -> Result<Self, PriorityError> {
        let revision = self.next_revision(expected)?;
        self.validate_holder(player)?;
        let consecutive_passes = self.consecutive_passes + 1;
        let phase = if consecutive_passes == self.order.len() {
            PriorityPhase::AllPassed
        } else {
            let index = self
                .order
                .iter()
                .position(|candidate| *candidate == player)
                .ok_or(PriorityError::InvalidOrder)?;
            PriorityPhase::RequiresChecks {
                candidate: self.order[(index + 1) % self.order.len()],
            }
        };
        Ok(Self {
            revision,
            consecutive_passes,
            phase,
            ..self.clone()
        })
    }

    /// Trusted kernel notification after an otherwise legal priority action completes.
    /// Casting, activating or a special action clears previous passes and retains the
    /// acting player's priority, after CR 117.5 checks. This does not authorize the action.
    pub fn record_action(&self, expected: u64, player: SeatId) -> Result<Self, PriorityError> {
        let revision = self.next_revision(expected)?;
        self.validate_holder(player)?;
        Ok(Self {
            revision,
            consecutive_passes: 0,
            phase: PriorityPhase::RequiresChecks { candidate: player },
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(count: u32, first: u32) -> PriorityRound {
        PriorityRound::new((0..count).map(SeatId).collect(), SeatId(first), 0).unwrap()
    }
    fn checked(round: &PriorityRound) -> PriorityRound {
        round.finish_checks(round.revision()).unwrap()
    }

    #[test]
    fn every_player_must_pass_in_turn_order_for_two_through_six_players() {
        for count in 2..=6 {
            for first in 0..count {
                let mut round = open(count, first);
                for offset in 0..count {
                    let expected_holder = SeatId((first + offset) % count);
                    assert_eq!(
                        round.phase(),
                        PriorityPhase::RequiresChecks {
                            candidate: expected_holder
                        }
                    );
                    round = checked(&round);
                    assert_eq!(
                        round.phase(),
                        PriorityPhase::Ready {
                            holder: expected_holder
                        }
                    );
                    round = round.pass(round.revision(), expected_holder).unwrap();
                    assert_eq!(round.consecutive_passes(), (offset + 1) as usize);
                }
                assert_eq!(round.phase(), PriorityPhase::AllPassed);
                assert_eq!(
                    round.pass(round.revision(), SeatId(first)),
                    Err(PriorityError::RoundClosed)
                );
            }
        }
    }

    #[test]
    fn action_resets_passes_and_keeps_actor_pending_checks() {
        let round = checked(&open(3, 0));
        let round = checked(&round.pass(round.revision(), SeatId(0)).unwrap());
        let round = round.record_action(round.revision(), SeatId(1)).unwrap();
        assert_eq!(round.consecutive_passes(), 0);
        assert_eq!(
            round.phase(),
            PriorityPhase::RequiresChecks {
                candidate: SeatId(1)
            }
        );
        assert_eq!(
            round.pass(round.revision(), SeatId(1)),
            Err(PriorityError::ChecksRequired)
        );
        let mut round = checked(&round);
        for player in [1, 2, 0] {
            round = round.pass(round.revision(), SeatId(player)).unwrap();
            if round.phase() != PriorityPhase::AllPassed {
                round = checked(&round);
            }
        }
        assert_eq!(round.phase(), PriorityPhase::AllPassed);
    }

    #[test]
    fn invalid_stale_and_foreign_actions_preserve_original_state() {
        let initial = open(2, 0);
        assert_eq!(
            initial.pass(0, SeatId(0)),
            Err(PriorityError::ChecksRequired)
        );
        let ready = checked(&initial);
        let saved = ready.clone();
        assert_eq!(ready.pass(0, SeatId(0)), Err(PriorityError::StaleRevision));
        assert_eq!(
            ready.pass(ready.revision(), SeatId(1)),
            Err(PriorityError::WrongPlayer)
        );
        assert_eq!(
            ready.record_action(ready.revision(), SeatId(99)),
            Err(PriorityError::WrongPlayer)
        );
        assert_eq!(
            ready.finish_checks(ready.revision()),
            Err(PriorityError::ChecksNotPending)
        );
        assert_eq!(ready, saved);
        assert_eq!(
            initial.phase(),
            PriorityPhase::RequiresChecks {
                candidate: SeatId(0)
            }
        );
    }

    #[test]
    fn validates_setup_and_never_wraps_revision() {
        assert_eq!(
            PriorityRound::new(vec![SeatId(0), SeatId(0)], SeatId(0), 0),
            Err(PriorityError::InvalidOrder)
        );
        assert_eq!(
            PriorityRound::new(vec![SeatId(0)], SeatId(0), 0),
            Err(PriorityError::InvalidOrder)
        );
        assert_eq!(
            PriorityRound::new((0..7).map(SeatId).collect(), SeatId(0), 0),
            Err(PriorityError::InvalidOrder)
        );
        assert_eq!(
            PriorityRound::new(vec![SeatId(0), SeatId(1)], SeatId(2), 0),
            Err(PriorityError::UnknownFirstPlayer)
        );
        let exhausted =
            PriorityRound::new(vec![SeatId(0), SeatId(1)], SeatId(0), u64::MAX).unwrap();
        assert_eq!(
            exhausted.finish_checks(u64::MAX),
            Err(PriorityError::RevisionExhausted)
        );
    }
}
