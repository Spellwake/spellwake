//! Bounded private worker delivery for an exact multiplayer response.
use super::*;

/// Result of one scheduled check at a specific native basis.
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum NativeResponseJobOutcome {
    /// The game moved, became unavailable, or has no supported wait at this basis.
    Inactive,
    /// SQL's current deadline is still in the future.
    Waiting {
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// The canonical host command accepted this checkpoint.
    Accepted {
        #[serde(rename = "checkpointId")]
        checkpoint_id: Uuid,
        revision: Amount,
    },
}

/// Process one game and expected basis under the explicitly configured host owner.
/// A delayed or repeated delivery cannot apply to a later seat or revision.
///
/// # Errors
/// Returns a safe error for invalid identifiers, missing configuration, retention
/// failures, invalid native history, or a racing authoritative commit.
pub fn run_owned_response_job(
    basis: &PlayerBasis,
    request_id: Uuid,
) -> Result<NativeResponseJobOutcome, NativeSourceError> {
    if basis.game_id.is_nil() || basis.context_id.is_nil() || request_id.is_nil() {
        return Err(invalid());
    }
    let repository = NativeSourceRepository::from_env()?;
    let (owner, runtime) = owned_runtime()?;
    run_clock(&repository, &runtime, &owner, basis, request_id)
}

pub(super) fn owned_runtime() -> Result<(NativeHostOwner, NativeGameRuntime), NativeSourceError> {
    let required = |name| std::env::var(name).map_err(|_| invalid());
    let owner = NativeHostOwner::new(&required("SPELLWAKE_NATIVE_HOST_OWNER")?)?;
    let runtime = NativeGameRuntime::from_env(
        &required("SPELLWAKE_SUPABASE_URL")?,
        &required("SPELLWAKE_SUPABASE_SECRET_KEY")?,
    )
    .map_err(|error| {
        NativeSourceError(Failure::Backend(BackendError::caused(
            "response worker",
            error,
        )))
    })?
    .ok_or_else(invalid)?;
    Ok((owner, runtime))
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
enum ClockObservation {
    Accepted {
        #[serde(rename = "checkpointId", deserialize_with = "crate::identifiers::uuid")]
        checkpoint_id: Uuid,
        revision: Amount,
    },
    Inactive,
    Waiting {
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    Bank {
        #[serde(rename = "seatId", deserialize_with = "crate::identifiers::uuid")]
        seat_id: Uuid,
    },
    Response,
}

pub(super) fn run_clock(
    repository: &NativeSourceRepository,
    runtime: &NativeGameRuntime,
    owner: &NativeHostOwner,
    basis: &PlayerBasis,
    request_id: Uuid,
) -> Result<NativeResponseJobOutcome, NativeSourceError> {
    let observation: ClockObservation = repository.native_record("observe_native_clock_job",
        &json!({"p_game":basis.game_id,"p_basis":basis,"p_owner":owner.as_str(),"p_request":request_id}),None)?;
    let bank = match observation {
        ClockObservation::Accepted {
            checkpoint_id,
            revision,
        } => {
            return Ok(NativeResponseJobOutcome::Accepted {
                checkpoint_id,
                revision,
            });
        }
        ClockObservation::Inactive => return Ok(NativeResponseJobOutcome::Inactive),
        ClockObservation::Waiting { expires_at } => {
            parse_utc_microsecond(&expires_at).ok_or_else(invalid)?;
            return Ok(NativeResponseJobOutcome::Waiting { expires_at });
        }
        ClockObservation::Bank { seat_id } => Some(seat_id),
        ClockObservation::Response => None,
    };
    let Some(source) = repository.recover_owned_history(
        basis.game_id,
        &runtime.store,
        &runtime.programs,
        runtime.recovery.clone(),
        owner,
    )?
    else {
        return Ok(NativeResponseJobOutcome::Inactive);
    };
    let checkpoint = source.checkpoint();
    if checkpoint.basis() != basis {
        return Ok(NativeResponseJobOutcome::Inactive);
    }
    let (seat, proposal, kind) = if let Some(seat) = bank {
        let binding = checkpoint
            .binding(seat)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
        let proposal = NativeScope::new(checkpoint.state(), binding)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e.into())))?
            .prepare_bank_expiry(seat)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e.into())))?;
        (seat, proposal, NativeCheckpointInput::BankExpiry)
    } else {
        let Some(seat) = checkpoint.response_seat()? else {
            return Ok(NativeResponseJobOutcome::Inactive);
        };
        let wait = match repository.opening_response_wait(basis.game_id)? {
            Some(current) => Some(current),
            None => repository.adopt_legacy_response(&source, owner)?,
        };
        let Some(wait) = wait else {
            return Ok(NativeResponseJobOutcome::Inactive);
        };
        let deadline = wait.deadline_for(checkpoint, seat)?;
        if !wait.due() {
            return Ok(NativeResponseJobOutcome::Waiting {
                expires_at: deadline.to_owned(),
            });
        }
        (
            seat,
            wait.prepare(checkpoint)?,
            NativeCheckpointInput::ResponseTimeout,
        )
    };
    let input = runtime
        .store
        .retain(
            basis.game_id,
            basis.context_id,
            NativeArtifactRole::Input,
            proposal.record(),
        )
        .map_err(|error| NativeSourceError(Failure::Artifact(error)))?;
    let next = checkpoint
        .append(&runtime.store, input, kind, seat)
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    if next.candidate() != proposal.candidate() {
        return Err(invalid());
    }
    next.retain(&runtime.store)
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    next.prepare_event()
        .and_then(|proof| proof.retain(&runtime.store))
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    next.prepare_results()
        .and_then(|proof| proof.retain(&runtime.store))
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    let checkpoint = if bank.is_some() {
        let candidate = source.prepare_bank_candidate(&next)?;
        let original = NativeSystemOriginal::prepare_bank_expiry(request_id, &source, &next)?;
        repository.commit_bank_expiry(
            &runtime.store,
            &source,
            request_id,
            &next,
            &candidate,
            &original,
        )?
    } else {
        let original = NativeSystemOriginal::prepare_response_timeout(request_id, &source, &next)?;
        repository.commit_opening_response_timeout(
            &runtime.store,
            &source,
            request_id,
            &next,
            &original,
        )?
    };
    Ok(NativeResponseJobOutcome::Accepted {
        checkpoint_id: checkpoint.resource_id,
        revision: checkpoint.revision,
    })
}
