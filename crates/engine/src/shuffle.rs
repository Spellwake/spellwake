//! Deterministic private shuffle entropy. The trusted host supplies independent
//! uniform secret words; shape validation cannot establish their randomness.
use crate::{
    object_identity::{GameObjects, ObjectId},
    priority::SeatId,
};
use std::fmt;

pub const SHUFFLE_ALGORITHM: &str = "fisher-yates-u64-tail-v1";
#[derive(Clone, PartialEq, Eq)]
pub struct EntropyTape {
    words: Vec<u64>,
    position: usize,
}
impl fmt::Debug for EntropyTape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntropyTape")
            .field("position", &self.position)
            .field("available", &self.remaining())
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropyError {
    Exhausted,
    InvalidBound,
    Capacity,
    EmptySupply,
}
impl EntropyTape {
    pub fn new(words: Vec<u64>) -> Self {
        Self { words, position: 0 }
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn remaining(&self) -> usize {
        self.words.len() - self.position
    }
    pub fn appended(&self, words: Vec<u64>) -> Result<Self, EntropyError> {
        if words.is_empty() {
            return Err(EntropyError::EmptySupply);
        }
        let mut next = self.clone();
        next.words
            .try_reserve(words.len())
            .map_err(|_| EntropyError::Capacity)?;
        next.words.extend(words);
        Ok(next)
    }
    fn below(&mut self, bound: usize) -> Result<usize, EntropyError> {
        let bound = u64::try_from(bound).map_err(|_| EntropyError::InvalidBound)?;
        if bound == 0 {
            return Err(EntropyError::InvalidBound);
        }
        let domain = 1u128 << 64;
        let limit = (domain / u128::from(bound)) * u128::from(bound);
        loop {
            let word = *self
                .words
                .get(self.position)
                .ok_or(EntropyError::Exhausted)?;
            self.position += 1;
            if u128::from(word) < limit {
                return usize::try_from(word % bound).map_err(|_| EntropyError::InvalidBound);
            }
        }
    }
    pub(crate) fn uniform_index(&self, bound: usize) -> Result<(usize, Self), EntropyError> {
        let mut next = self.clone();
        let index = next.below(bound)?;
        Ok((index, next))
    }

    pub(crate) fn flip_coin(&self) -> Result<(bool, Self), EntropyError> {
        let mut next = self.clone();
        let won = next.below(2)? == 1;
        Ok((won, next))
    }
    pub(crate) fn shuffled(
        &self,
        mut order: Vec<ObjectId>,
    ) -> Result<(Vec<ObjectId>, Self), EntropyError> {
        let mut next = self.clone();
        for index in (1..order.len()).rev() {
            let selected = next.below(index + 1)?;
            order.swap(index, selected);
        }
        Ok((order, next))
    }
}
/// Private current-input fact, including empty/single-card shuffle events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShuffleRecord {
    pub player: SeatId,
    pub algorithm: &'static str,
    pub before: Vec<ObjectId>,
    pub after: Vec<ObjectId>,
    pub entropy_start: usize,
    pub entropy_end: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryShuffle {
    pub next: GameObjects,
    pub entropy: EntropyTape,
    pub record: ShuffleRecord,
}
