//! Test-build access to this owner's operational allocators, never a game action.
use super::{CharacteristicState, CharacteristicTimestamp};
use crate::object_identity::ObjectId;

impl CharacteristicState {
    pub(crate) fn qualification_set_next_effect(&mut self, next: u64) {
        self.next_effect = next;
    }
    pub(crate) fn qualification_set_next_timestamp(&mut self, next: u64) {
        self.next_timestamp = next;
    }
    pub(crate) fn qualification_remove_object_timestamp(&mut self, object: ObjectId) {
        self.object_timestamps.remove(&object);
    }
}

pub(crate) use super::evaluation::qualification_static_probe;

/// Executes only the selected ordering contract for the two frozen O units.
/// Supplied waits are not inferred dependencies or card-equivalence evidence.
pub(crate) fn qualification_ordering_selection(
    bucket: super::CharacteristicBucket,
    timestamps: &std::collections::BTreeMap<super::EffectInstanceKey, CharacteristicTimestamp>,
    original_waits: &std::collections::BTreeSet<super::ComponentWait>,
) -> Result<super::ComponentSelection, super::CharacteristicInvariantError> {
    let selection = super::ordering::select(timestamps, original_waits)?;
    Ok(super::ComponentSelection {
        bucket,
        generated: vec![],
        original_waits: original_waits.clone(),
        strongly_connected: selection.strongly_connected,
        derived_timestamp_waits: selection.derived_timestamp_waits,
        retained_outside_waits: selection.retained_outside_waits,
        eligible: selection.eligible,
        chosen: selection.chosen,
    })
}
