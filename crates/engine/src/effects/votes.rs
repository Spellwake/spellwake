//! Sequential public card ballots, ordered from the resolving controller around the table.
use super::*;

impl EffectWindow {
    /// Earlier public votes in the current card ballot, in actual voting order.
    pub fn public_graveyard_votes(&self) -> &[(SeatId, ObjectId)] {
        self.resolution_cursor()
            .map_or(&[], |cursor| cursor.impulse.votes.as_slice())
    }
}
