//! Exact life totals and ordinary life adjustments. Damage uses the same owner;
//! life payment, replacement effects and shared team totals remain separate work.
use crate::{priority::SeatId, setup::StartingLife};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeMap;

/// Authored life instruction. Gain/loss amounts must be nonnegative; a set total
/// may be any rules integer (CR 119.3, 119.5). Zero changes create no occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeAdjustment {
    /// Increase the total by a nonnegative amount.
    Gain(RulesInteger),
    /// Decrease the total by a nonnegative amount, without treating it as damage.
    Lose(RulesInteger),
    /// Set a signed total; the resulting difference is a gain or loss.
    Set(RulesInteger),
}
impl LifeAdjustment {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Gain(amount) | Self::Lose(amount) => *amount >= 0.into(),
            Self::Set(_) => true,
        }
    }
}

/// Whether an actual change increased or decreased the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeChangeKind {
    /// The total increased.
    Gain,
    /// The total decreased.
    Loss,
}

/// One actual positive gain or loss, including changes caused by setting a total.
/// Resolution entry/instruction or damage origin belongs to the enclosing event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeChange {
    /// Participant whose total changed.
    pub player: SeatId,
    /// Direction of the actual change, independent of the authored operation.
    pub kind: LifeChangeKind,
    /// Strictly positive difference between the totals.
    pub amount: RulesInteger,
    /// Total immediately before this occurrence.
    pub before: RulesInteger,
    /// Total immediately after this occurrence.
    pub after: RulesInteger,
}

/// One current total per already-validated participant. Loss is adjudicated by
/// the shared state-based checks, not by setup or an observation of this map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifeState(BTreeMap<SeatId, RulesInteger>);
impl LifeState {
    pub(crate) fn new(players: &[SeatId], supplied: Vec<StartingLife>) -> Option<Self> {
        if supplied.len() != players.len() {
            return None;
        }
        let mut totals = BTreeMap::new();
        for input in supplied {
            if !players.contains(&input.player)
                || totals.insert(input.player, input.total).is_some()
            {
                return None;
            }
        }
        Some(Self(totals))
    }
    pub(crate) fn get(&self, player: SeatId) -> Option<&RulesInteger> {
        self.0.get(&player)
    }
    pub(crate) fn nonpositive_players(&self) -> impl Iterator<Item = SeatId> + '_ {
        self.0
            .iter()
            .filter(|(_, total)| **total <= 0.into())
            .map(|(&player, _)| player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_losses_validate_every_recipient_and_quantity_before_any_write() {
        let a = SeatId(4);
        let b = SeatId(90);
        let mut state = LifeState::new(
            &[a, b],
            vec![
                StartingLife {
                    player: a,
                    total: 19.into(),
                },
                StartingLife {
                    player: b,
                    total: 7.into(),
                },
            ],
        )
        .unwrap();
        let before = state.clone();
        for (losses, error) in [
            (
                BTreeMap::from([(a, 2.into()), (SeatId(100), 3.into())]),
                LifeError::UnknownPlayer,
            ),
            (
                BTreeMap::from([(a, 2.into()), (b, 0.into())]),
                LifeError::InvalidLoss,
            ),
            (
                BTreeMap::from([(a, 2.into()), (b, (-1).into())]),
                LifeError::InvalidLoss,
            ),
        ] {
            assert_eq!(state.apply_losses(&losses), Err(error));
            assert_eq!(state, before);
        }
        let huge: RulesInteger = "900719925474099312345678901234567890"
            .to_owned()
            .try_into()
            .unwrap();
        state
            .apply_losses(&BTreeMap::from([(a, huge.clone()), (b, 8.into())]))
            .unwrap();
        assert_eq!(state.get(a), Some(&RulesInteger::from(19).minus(&huge)));
        assert_eq!(state.get(b), Some(&(-1).into()));
        assert_eq!(state.nonpositive_players().collect::<Vec<_>>(), vec![a, b]);
    }

    #[test]
    fn adjustment_batches_reject_unknown_or_invalid_members_atomically() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = LifeState::new(
            &[a, b],
            vec![
                StartingLife {
                    player: a,
                    total: 20.into(),
                },
                StartingLife {
                    player: b,
                    total: 20.into(),
                },
            ],
        )
        .unwrap();
        let saved = state.clone();
        for (adjustments, expected) in [
            (
                BTreeMap::from([
                    (a, LifeAdjustment::Gain(5.into())),
                    (SeatId(2), LifeAdjustment::Lose(1.into())),
                ]),
                LifeError::UnknownPlayer,
            ),
            (
                BTreeMap::from([
                    (a, LifeAdjustment::Gain(5.into())),
                    (b, LifeAdjustment::Gain((-1).into())),
                ]),
                LifeError::InvalidAdjustment,
            ),
            (
                BTreeMap::from([
                    (a, LifeAdjustment::Gain(5.into())),
                    (b, LifeAdjustment::Lose((-1).into())),
                ]),
                LifeError::InvalidAdjustment,
            ),
        ] {
            assert_eq!(state.apply_adjustments(&adjustments), Err(expected));
            assert_eq!(state, saved);
        }
        assert!(
            state
                .apply_adjustments(&BTreeMap::from([
                    (a, LifeAdjustment::Gain(0.into())),
                    (b, LifeAdjustment::Set(20.into()))
                ]))
                .unwrap()
                .is_empty()
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn setting_signed_totals_records_the_exact_gain_or_loss_without_clamping() {
        let player = SeatId(0);
        let mut state = LifeState::new(
            &[player],
            vec![StartingLife {
                player,
                total: 20.into(),
            }],
        )
        .unwrap();
        let huge: RulesInteger = "900719925474099312345678901234567890"
            .to_owned()
            .try_into()
            .unwrap();
        let negative = RulesInteger::from(0).minus(&huge);
        let changes = state
            .apply_adjustments(&BTreeMap::from([(
                player,
                LifeAdjustment::Set(negative.clone()),
            )]))
            .unwrap();
        assert_eq!(
            changes,
            vec![LifeChange {
                player,
                kind: LifeChangeKind::Loss,
                amount: huge.plus(&20.into()),
                before: 20.into(),
                after: negative.clone()
            }]
        );
        let gain = LifeAdjustment::Gain(huge.plus(&7.into()));
        let changes = state
            .apply_adjustments(&BTreeMap::from([(player, gain)]))
            .unwrap();
        assert_eq!(state.get(player), Some(&7.into()));
        assert_eq!(changes[0].kind, LifeChangeKind::Gain);
        assert_eq!(changes[0].before, negative);
        assert_eq!(changes[0].amount, huge.plus(&7.into()));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifeError {
    UnknownPlayer,
    InvalidLoss,
    InvalidAdjustment,
}
impl LifeState {
    /// The enclosing executor commits this exact set with all creature results.
    pub(crate) fn apply_losses(
        &mut self,
        losses: &BTreeMap<SeatId, RulesInteger>,
    ) -> Result<Vec<LifeChange>, LifeError> {
        for (&player, amount) in losses {
            if !amount.is_positive() {
                return Err(LifeError::InvalidLoss);
            }
            if self.get(player).is_none() {
                return Err(LifeError::UnknownPlayer);
            }
        }
        let adjustments = losses
            .iter()
            .map(|(&player, amount)| (player, LifeAdjustment::Lose(amount.clone())))
            .collect();
        self.apply_adjustments(&adjustments)
    }

    /// Prepare the complete batch before writing any total. An operational or
    /// validation error cannot expose a partially applied batch.
    pub(crate) fn apply_adjustments(
        &mut self,
        adjustments: &BTreeMap<SeatId, LifeAdjustment>,
    ) -> Result<Vec<LifeChange>, LifeError> {
        let mut changes = Vec::with_capacity(adjustments.len());
        for (&player, adjustment) in adjustments {
            if !adjustment.valid() {
                return Err(LifeError::InvalidAdjustment);
            }
            let before = self.get(player).ok_or(LifeError::UnknownPlayer)?;
            let after = match adjustment {
                LifeAdjustment::Gain(amount) => before.plus(amount),
                LifeAdjustment::Lose(amount) => before.minus(amount),
                LifeAdjustment::Set(total) => total.clone(),
            };
            if after == *before {
                continue;
            }
            let (kind, amount) = if after > *before {
                (LifeChangeKind::Gain, after.minus(before))
            } else {
                (LifeChangeKind::Loss, before.minus(&after))
            };
            changes.push(LifeChange {
                player,
                kind,
                amount,
                before: before.clone(),
                after,
            });
        }
        for change in &changes {
            self.0.insert(change.player, change.after.clone());
        }
        Ok(changes)
    }
}
