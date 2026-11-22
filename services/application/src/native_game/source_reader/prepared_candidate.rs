//! One trusted result-only frame producer for human and host API orchestration.
use super::*;
use crate::admission_ports::{HostCandidate, NativeEventSchema};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Participation {
    seat_id: Uuid,
    own_turns: NativeRevision,
    reason: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Terminal<'a> {
    outcome: crate::match_history::Outcome,
    winner_seat_ids: &'a [Uuid],
    reason: &'static str,
    winners_ref: &'a NativeArtifactRef,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame<'a> {
    event_schema_revision: NativeEventSchema,
    context_id: Uuid,
    state_ref: &'a NativeArtifactRef,
    facts_ref: &'a NativeArtifactRef,
    events: [&'a NativeArtifactRef; 1],
    facts: Vec<crate::native_game::NativeCastFact>,
    clocks: NativeClockIntent,
    participation: Vec<Participation>,
    terminal: Option<Terminal<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    opening_response_seat_id: Option<Uuid>,
}
impl RecoveredNativeGame {
    /// Prepare the canonical trusted frame for a genuine linked native successor.
    /// Derives every Event/result/clock/participation field from actual operands;
    /// no client proof, clock, winner or feature fact is accepted by this method.
    /// This is preparation only. Retain the successor State, Event and Results
    /// before committing; commit methods recheck exact bytes, original intent,
    /// actor/source and SQL authority. It does not grant a host lease or admission.
    /// # Errors
    /// Rejects unavailable/foreign/unlinked sources, unsupported native clock or
    /// terminal boundaries, unknown human turn counts, encoding or frame limits.
    pub fn prepare_candidate(
        &self,
        next: &NativeCheckpoint,
    ) -> Result<HostCandidate, NativeSourceError> {
        self.prepare_candidate_schema(next, NativeEventSchema::TransitionV1)
    }
    /// Prepare the additive physical-cast activity frame from actual native operands.
    /// Human facts use the selected immutable roster; AI casts never credit sponsors.
    /// # Errors
    /// Rejects invalid linkage, attribution, overflow, evidence or frame bounds.
    pub fn prepare_activity_candidate(
        &self,
        next: &NativeCheckpoint,
    ) -> Result<HostCandidate, NativeSourceError> {
        self.prepare_candidate_schema(next, NativeEventSchema::ActivityV1)
    }
    fn prepare_candidate_schema(
        &self,
        next: &NativeCheckpoint,
        schema: NativeEventSchema,
    ) -> Result<HostCandidate, NativeSourceError> {
        let checkpoint_error = |error| NativeSourceError(Failure::Checkpoint(error));
        // Existing clock producer owns exact source/parent/basis validation.
        let clocks = self.clock_intent(next).map_err(checkpoint_error)?;
        let event = next
            .prepare_event_schema(schema)
            .map_err(checkpoint_error)?;
        let results = next
            .prepare_results_schema(schema)
            .map_err(checkpoint_error)?;
        let terminal = next.terminal().map_err(checkpoint_error)?;
        let participation = if terminal.is_some() {
            self.source()
                .human_seats()
                .map(|seat_id| {
                    Ok(Participation {
                        seat_id,
                        own_turns: next.own_turns(seat_id).ok_or_else(invalid)?.into(),
                        reason: "native_turn_starts_v1",
                    })
                })
                .collect::<Result<Vec<_>, NativeSourceError>>()?
        } else {
            vec![]
        };
        let frame = Frame {
            event_schema_revision: schema,
            context_id: next.basis().context_id,
            state_ref: next.reference(),
            facts_ref: results.reference(),
            events: [event.reference()],
            facts: match schema {
                NativeEventSchema::TransitionV1 => vec![],
                NativeEventSchema::ActivityV1 => self
                    .source()
                    .cast_accounts()
                    .and_then(|accounts| accounts.facts(next))
                    .map_err(checkpoint_error)?,
            },
            clocks,
            participation,
            opening_response_seat_id: next.opening_response_seat()?,
            terminal: terminal.as_ref().map(|result| Terminal {
                outcome: result.outcome(),
                winner_seat_ids: result.winner_seat_ids(),
                reason: result.reason(),
                winners_ref: results.reference(),
            }),
        };
        let bytes = serde_json::to_vec(&frame).map_err(|error| {
            NativeSourceError(Failure::Backend(BackendError::caused(
                "native frame",
                error,
            )))
        })?;
        HostCandidate::from_bytes(&bytes).map_err(|error| {
            NativeSourceError(Failure::Backend(BackendError::caused(
                "native frame",
                error,
            )))
        })
    }
}
