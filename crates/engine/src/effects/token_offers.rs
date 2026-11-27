//! In a tempting offer, opponents decide in table order before any of their tokens enter.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TemptingTokenOffer {
    pub(super) amount: RulesInteger,
    pub(super) replies: Vec<(SeatId, bool)>,
}
