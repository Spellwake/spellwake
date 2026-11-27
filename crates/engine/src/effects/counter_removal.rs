//! Allocate removals against one current battlefield incarnation, then publish one rules cut.

use super::*;

/// Frozen counter-removal quantities for one resolving instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRemovalChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    object: ObjectId,
    minimum: u8,
    maximum: u8,
    domain: BTreeMap<CounterKind, u8>,
}

impl CounterRemovalChoice {
    /// Player who makes the allocation.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }

    /// Targeted battlefield incarnation from which the counters will be removed.
    pub fn object(&self) -> ObjectId {
        self.object
    }

    /// Minimum number of counters that must be removed in total.
    pub fn minimum(&self) -> u8 {
        self.minimum
    }

    /// Maximum number of counters that may be removed in total.
    pub fn maximum(&self) -> u8 {
        self.maximum
    }

    /// Available quantities by kind, bounded by the instruction's maximum.
    pub fn domain(&self) -> &BTreeMap<CounterKind, u8> {
        &self.domain
    }
}
