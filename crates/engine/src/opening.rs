//! Ordinary individual-turn seven-card London mulligan protocol (CR 103.5).
//! Canonical cards, entropy and player input authority stay in the root owners.
use crate::{object_identity::ObjectId, priority::SeatId};
use std::collections::{BTreeMap, BTreeSet};
pub const HAND_SIZE: usize = 7;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulliganChoice {
    Keep,
    Mulligan,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpeningDecision {
    Declare {
        player: SeatId,
        taken: u8,
        can_mulligan: bool,
    },
    Bottom {
        player: SeatId,
        count: usize,
        hand: Vec<ObjectId>,
    },
}
impl OpeningDecision {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::Declare { player, .. } | Self::Bottom { player, .. } => *player,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpeningStage {
    Declare {
        cursor: usize,
        requested: Vec<SeatId>,
    },
    Bottom {
        cohort: Vec<SeatId>,
        cursor: usize,
        selected: BTreeMap<SeatId, Vec<ObjectId>>,
    },
    Complete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningError {
    InvalidSetup,
    NotOpening,
    WrongStage,
    WrongReply,
    IllegalMulligan,
    InvalidBottom,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningState {
    pub(crate) stage: OpeningStage,
    pub(crate) kept: BTreeSet<SeatId>,
    pub(crate) taken: BTreeMap<SeatId, u8>,
    pub(crate) free: bool,
}
impl OpeningState {
    pub(crate) fn new(multiplayer: bool) -> Self {
        Self {
            stage: OpeningStage::Declare {
                cursor: 0,
                requested: vec![],
            },
            kept: BTreeSet::new(),
            taken: BTreeMap::new(),
            free: multiplayer,
        }
    }
    pub(crate) fn remove_player(
        &mut self,
        order: &[SeatId],
        player: SeatId,
    ) -> Result<(), OpeningError> {
        self.kept.remove(&player);
        self.taken.remove(&player);
        match &mut self.stage {
            OpeningStage::Declare { cursor, requested } => {
                let index = order
                    .iter()
                    .position(|seat| *seat == player)
                    .ok_or(OpeningError::InvalidSetup)?;
                if index < *cursor {
                    *cursor -= 1;
                }
                requested.retain(|seat| *seat != player);
            }
            OpeningStage::Bottom {
                cursor,
                cohort,
                selected,
            } => {
                if let Some(index) = cohort.iter().position(|seat| *seat == player) {
                    if index < *cursor {
                        *cursor -= 1;
                    }
                    cohort.remove(index);
                }
                selected.remove(&player);
            }
            OpeningStage::Complete => return Err(OpeningError::WrongStage),
        }
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.stage == OpeningStage::Complete
    }
    pub fn kept(&self, player: SeatId) -> bool {
        self.kept.contains(&player)
    }
    pub fn mulligans_taken(&self, player: SeatId) -> u8 {
        *self.taken.get(&player).unwrap_or(&0)
    }
    pub fn charged(&self, player: SeatId) -> usize {
        usize::from(
            self.mulligans_taken(player)
                .saturating_sub(u8::from(self.free)),
        )
    }
    pub fn can_mulligan(&self, player: SeatId) -> bool {
        !self.kept(player) && self.charged(player) < HAND_SIZE
    }
}
