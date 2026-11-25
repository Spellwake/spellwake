//! Bounded atomic preparation of explicit own mana activations followed by one cast.
//! No candidate is installed or persisted here; the acceptance owner retains all records.
use super::*;
use crate::player_wire::PlayerAction;
use spellwake_engine::{priority::PriorityPhase, stack_window::WindowPhase};

/// A rejected batch leaves the original source and its issued registry unchanged.
#[derive(Debug)]
pub enum NativeCastBatchError {
    /// Wrong length, action ordering, duplicate request or source basis.
    InvalidShape,
    /// An intermediate step needs another actor/choice or has ended the game.
    Boundary { step: usize },
    /// Native reference or transition rejected one exact step.
    Bridge {
        step: usize,
        cause: NativeBridgeError,
    },
    /// An original private input could not be encoded or exactly reconstructed.
    Codec {
        step: usize,
        cause: NativeCodecError,
    },
    /// Reconstruction did not produce the prepared state.
    ReplayMismatch { step: usize },
}
impl fmt::Display for NativeCastBatchError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShape => out.write_str("invalid mana-and-cast batch"),
            Self::Boundary { step } => {
                write!(out, "batch step {step} crosses a required player boundary")
            }
            Self::Bridge { step, .. } => write!(out, "batch step {step} was rejected"),
            Self::Codec { step, .. } => write!(out, "batch step {step} could not be recorded"),
            Self::ReplayMismatch { step } => write!(out, "batch step {step} failed reconstruction"),
        }
    }
}
impl Error for NativeCastBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bridge { cause, .. } => Some(cause),
            Self::Codec { cause, .. } => Some(cause),
            _ => None,
        }
    }
}
/// Exact step records and final privately staged state; not a durable acceptance receipt.
pub struct PreparedNativeCastBatch {
    state: GameState,
    records: Vec<Vec<u8>>,
    candidates: Vec<NativeCandidate>,
}
impl PreparedNativeCastBatch {
    /// Read the final uninstalled state to qualify the proposed sequence.
    pub fn state(&self) -> &GameState {
        &self.state
    }
    /// Ordered accepted native candidates for each step in this batch.
    pub fn candidates(&self) -> &[NativeCandidate] {
        &self.candidates
    }
    /// Consume this prepared batch into its ordered accepted native candidates.
    pub fn into_candidates(self) -> Vec<NativeCandidate> {
        self.candidates
    }
    /// Transfer final state and ordered original-input records to the acceptance owner.
    pub fn into_parts(self) -> (GameState, Vec<Vec<u8>>) {
        (self.state, self.records)
    }
}
impl NativeScope<'_> {
    /// Stage zero to eight explicitly selected mana activations, then exactly one cast.
    /// All inputs must carry this original public basis and distinct request IDs. Only
    /// references already issued in this participant scope can be carried forward.
    /// Each step validates those references on its actual intermediate source and uses
    /// a fresh basis; no opponent priority, required choice or stack resolution is skipped.
    /// The final cast may suspend at a required decision, which the caller must deliver.
    /// # Errors
    /// Rejects malformed/stale batches, unsupported action ordering, a changed actor or
    /// pending decision, missing/stale references, actual engine rejections and original
    /// input encoding/reconstruction failures. Every rejection preserves the source.
    pub fn prepare_cast_batch(
        &self,
        originals: &[PlayerCommand],
    ) -> Result<PreparedNativeCastBatch, NativeCastBatchError> {
        if originals.is_empty() || originals.len() > 9 {
            return Err(NativeCastBatchError::InvalidShape);
        }
        let mut requests = BTreeSet::new();
        for (step, original) in originals.iter().enumerate() {
            if original.validate().is_err()
                || original.basis != *self.basis()
                || !requests.insert(original.request_id)
            {
                return Err(NativeCastBatchError::InvalidShape);
            }
            let valid = if step + 1 == originals.len() {
                matches!(
                    original.action,
                    PlayerAction::CastUntargeted { .. }
                        | PlayerAction::CastTargeted { .. }
                        | PlayerAction::ActivateAbility { .. }
                        | PlayerAction::ActivateUntargetedAbility { .. }
                )
            } else {
                matches!(original.action, PlayerAction::ActivateMana { .. })
            };
            if !valid {
                return Err(NativeCastBatchError::InvalidShape);
            }
        }
        let mut state = self.source.clone();
        let mut records = Vec::with_capacity(originals.len());
        let mut candidates = Vec::with_capacity(originals.len());
        for (step, original) in originals.iter().enumerate() {
            if state.pending_decision().is_some()
                || state.outcome().is_some()
                || !matches!(state.execution().window().phase(),WindowPhase::Priority(PriorityPhase::Ready{holder}) if holder==self.binding.actor)
            {
                return Err(NativeCastBatchError::Boundary { step });
            }
            let mut binding = self.binding.clone();
            binding.basis = state.basis().clone();
            binding.public_basis.revision = state.basis().revision.into();
            let mut scope = NativeScope::new(&state, binding)
                .map_err(|cause| NativeCastBatchError::Bridge { step, cause })?;
            // Only the finite grammar's actual handles are carried. Preserve their
            // original wire token after exact typed/current-reference validation.
            let mut handles = Vec::new();
            match &original.action {
                PlayerAction::ActivateMana { source, ability } => handles.extend([source, ability]),
                PlayerAction::CastUntargeted { source, .. } => handles.push(source),
                PlayerAction::CastTargeted { source, target, .. } => {
                    handles.push(source);
                    match target {
                        crate::player_wire::PlayerTarget::Object { handle }
                        | crate::player_wire::PlayerTarget::Stack { handle } => {
                            handles.push(handle)
                        }
                        crate::player_wire::PlayerTarget::Seat { .. } => {}
                    }
                }
                PlayerAction::ActivateUntargetedAbility {
                    source, ability, ..
                } => {
                    handles.extend([source, ability]);
                }
                PlayerAction::ActivateAbility {
                    source,
                    ability,
                    target,
                    ..
                } => {
                    handles.extend([source, ability]);
                    match target {
                        crate::player_wire::PlayerTarget::Object { handle }
                        | crate::player_wire::PlayerTarget::Stack { handle } => {
                            handles.push(handle)
                        }
                        crate::player_wire::PlayerTarget::Seat { .. } => {}
                    }
                }
                _ => return Err(NativeCastBatchError::InvalidShape),
            }
            for handle in handles {
                let reference = self
                    .reference(handle)
                    .map_err(|cause| NativeCastBatchError::Bridge { step, cause })?;
                scope
                    .validate_reference(reference)
                    .map_err(|cause| NativeCastBatchError::Bridge { step, cause })?;
                if !scope
                    .references
                    .iter()
                    .any(|(existing, _)| existing == handle)
                {
                    scope.references.push((handle.clone(), reference));
                }
            }
            let current = PlayerCommand {
                request_id: original.request_id,
                basis: scope.basis().clone(),
                action: original.action.clone(),
            };
            let prepared = scope
                .propose(&current)
                .map_err(|cause| NativeCastBatchError::Bridge { step, cause })?;
            let record = scope
                .encode_input(&prepared)
                .map_err(|cause| NativeCastBatchError::Codec { step, cause })?;
            let restored = restore_input(&state, scope.binding.clone(), &record)
                .map_err(|cause| NativeCastBatchError::Codec { step, cause })?;
            if restored.candidate() != prepared.candidate() {
                return Err(NativeCastBatchError::ReplayMismatch { step });
            }
            records.push(record);
            let candidate = prepared.into_parts().3;
            state = candidate.state().clone();
            candidates.push(candidate);
        }
        Ok(PreparedNativeCastBatch {
            state,
            records,
            candidates,
        })
    }

    /// Lightweight read-only probe: apply `taps` once on a cloned state (without
    /// journal encoding or replay verification) and return the indices of every
    /// action in `candidates` accepted by the native rules engine.
    pub fn probe_cast_candidates(
        &self,
        taps: &[PlayerAction],
        candidates: &[PlayerAction],
    ) -> Vec<usize> {
        if candidates.is_empty() || taps.len() > 8 {
            return Vec::new();
        }
        let mut state = self.source.clone();
        for tap in taps {
            let mut binding = self.binding.clone();
            binding.basis = state.basis().clone();
            binding.public_basis.revision = state.basis().revision.into();
            let Ok(mut scope) = NativeScope::new(&state, binding) else {
                return Vec::new();
            };
            let PlayerAction::ActivateMana { source, ability } = tap else {
                return Vec::new();
            };
            for handle in [source, ability] {
                let Ok(reference) = self.reference(handle) else {
                    return Vec::new();
                };
                if scope.validate_reference(reference).is_err() {
                    return Vec::new();
                }
                if !scope
                    .references
                    .iter()
                    .any(|(existing, _)| existing == handle)
                {
                    scope.references.push((handle.clone(), reference));
                }
            }
            let cmd = PlayerCommand {
                request_id: uuid::Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: tap.clone(),
            };
            let Ok(prepared) = scope.propose(&cmd) else {
                return Vec::new();
            };
            state = prepared.into_parts().3.into_state();
        }

        let mut binding = self.binding.clone();
        binding.basis = state.basis().clone();
        binding.public_basis.revision = state.basis().revision.into();
        let Ok(mut scope) = NativeScope::new(&state, binding) else {
            return Vec::new();
        };

        let mut accepted = Vec::new();
        for (idx, candidate) in candidates.iter().enumerate() {
            let mut handles = Vec::new();
            match candidate {
                PlayerAction::CastUntargeted { source, .. } => handles.push(source),
                PlayerAction::CastTargeted { source, target, .. } => {
                    handles.push(source);
                    match target {
                        crate::player_wire::PlayerTarget::Object { handle }
                        | crate::player_wire::PlayerTarget::Stack { handle } => {
                            handles.push(handle);
                        }
                        crate::player_wire::PlayerTarget::Seat { .. } => {}
                    }
                }
                PlayerAction::ActivateUntargetedAbility {
                    source, ability, ..
                } => {
                    handles.extend([source, ability]);
                }
                PlayerAction::ActivateAbility {
                    source,
                    ability,
                    target,
                    ..
                } => {
                    handles.extend([source, ability]);
                    match target {
                        crate::player_wire::PlayerTarget::Object { handle }
                        | crate::player_wire::PlayerTarget::Stack { handle } => {
                            handles.push(handle);
                        }
                        crate::player_wire::PlayerTarget::Seat { .. } => {}
                    }
                }
                _ => continue,
            }
            let mut refs_ok = true;
            for handle in handles {
                let Ok(reference) = self.reference(handle) else {
                    refs_ok = false;
                    break;
                };
                if scope.validate_reference(reference).is_err() {
                    refs_ok = false;
                    break;
                }
                if !scope
                    .references
                    .iter()
                    .any(|(existing, _)| existing == handle)
                {
                    scope.references.push((handle.clone(), reference));
                }
            }
            if !refs_ok {
                continue;
            }
            let cmd = PlayerCommand {
                request_id: uuid::Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: candidate.clone(),
            };
            if scope.propose(&cmd).is_ok() {
                accepted.push(idx);
            }
        }
        accepted
    }
}

#[cfg(test)]
mod tests;
