//! Join independently accepted SQL cuts to one actual retained-input replay.
use super::*;

/// Scope of evidence checked during this recovery, never a player authority.
/// This cannot be deserialized or supplied by an external caller. Even complete
/// transition evidence does not qualify feature metrics, cards or public replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHistoryEvidence {
    /// Only the native State/input chain was recovered.
    StateOnly,
    /// Every accepted cut has exact native-transition-v1 Event/result evidence.
    TransitionV1,
    /// Every cut has checked transition evidence; some have physical-cast metrics.
    /// Older V1 cuts retain result-only scope and are never backfilled.
    ActivityTransitionsV1,
    /// At least one accepted play-land action was independently recomputed.
    LandPlayTransitionsV1,
    /// The State chain was checked against all accepted cuts, but at least one
    /// historical cut has no transition codec. Its opaque evidence is unqualified.
    HistoricalUnqualified,
}

impl NativeHistoryEvidence {
    pub(super) fn transitions_verified(self) -> bool {
        match self {
            Self::TransitionV1 | Self::ActivityTransitionsV1 | Self::LandPlayTransitionsV1 => true,
            Self::StateOnly | Self::HistoricalUnqualified => false,
        }
    }
}
pub(super) struct HistoryReplay<'a> {
    sources: &'a NativeTransitionSources,
    next: usize,
    historical: bool,
    activity: bool,
    land_play: bool,
    previous_wait: Option<super::response_wait::LogicalWait>,
}
impl<'a> HistoryReplay<'a> {
    pub(super) fn new(sources: &'a NativeTransitionSources) -> Self {
        Self {
            sources,
            next: 0,
            historical: false,
            activity: false,
            land_play: false,
            previous_wait: None,
        }
    }
    pub(super) fn observe(
        &mut self,
        store: &NativeArtifactStore,
        checkpoint: &NativeCheckpoint,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<(), NativeCheckpointError> {
        let group = self
            .sources
            .groups
            .get(self.next)
            .ok_or(NativeCheckpointError::Invalid)?;
        if group.accepted_revision != checkpoint.basis().revision
            || group.state_ref != *checkpoint.reference()
        {
            return Err(NativeCheckpointError::Invalid);
        }
        if let Some(seat) = group.opening_response_seat_id
            && checkpoint
                .opening_response_seat()
                .map_err(|_| NativeCheckpointError::Invalid)?
                != Some(seat)
        {
            return Err(NativeCheckpointError::Invalid);
        }
        match group.turn_response_protocol {
            Some(protocol)
                if checkpoint.turn_response_protocol().is_some()
                    && checkpoint
                        .turn_response_for(self.previous_wait.as_ref(), protocol)
                        .map_err(|_| NativeCheckpointError::Invalid)?
                        == group.turn_response => {}
            None if group.turn_response.is_none() => {}
            _ => return Err(NativeCheckpointError::Invalid),
        }
        self.previous_wait = if group.turn_response.is_some()
            || self.next + 1 < self.sources.groups.len()
                && self.sources.groups[self.next + 1].turn_response.is_some()
        {
            checkpoint.logical_wait()
        } else {
            None
        };
        let response_timeout = matches!(
            checkpoint.retained_input(),
            Some((NativeCheckpointInput::ResponseTimeout, _, _))
        );
        match (response_timeout, &group.opening_response_expiry) {
            (true, Some(evidence)) if group.event_schema_revision.is_some() => {
                evidence.verify(checkpoint)?
            }
            (false, None) => {}
            _ => return Err(NativeCheckpointError::Invalid),
        }
        let bank_input = checkpoint
            .retained_input()
            .is_some_and(|(kind, _, _)| matches!(kind, NativeCheckpointInput::BankExpiry));
        match (bank_input, &group.bank_expiry) {
            (true, Some(evidence)) if group.event_schema_revision.is_some() => {
                evidence.verify(checkpoint)?
            }
            (false, None) => {}
            _ => return Err(NativeCheckpointError::Invalid),
        }
        if let Some(schema) = group.event_schema_revision {
            let [event] = group.events.as_slice() else {
                return Err(NativeCheckpointError::Invalid);
            };
            match schema {
                crate::admission_ports::NativeEventSchema::TransitionV1 => {
                    if !event.facts.is_empty() {
                        return Err(NativeCheckpointError::Invalid);
                    }
                }
                crate::admission_ports::NativeEventSchema::ActivityV1
                | crate::admission_ports::NativeEventSchema::LandPlayV1 => {
                    let expected = self
                        .sources
                        .cast_accounts
                        .facts_for_schema(checkpoint, schema)?;
                    self.land_play |=
                        schema == crate::admission_ports::NativeEventSchema::LandPlayV1;
                    if !crate::native_game::NativeCastFact::matches_set(
                        &expected,
                        event.facts.iter().map(|row| {
                            (
                                row.account_id,
                                row.kind.as_str(),
                                row.fact_revision.as_str(),
                                &row.value,
                            )
                        }),
                    ) {
                        return Err(NativeCheckpointError::Invalid);
                    }
                    self.activity = true;
                }
            }
            let prepared_event = checkpoint.prepare_event_schema(schema)?;
            let prepared_results = checkpoint.prepare_results_schema(schema)?;
            if prepared_event.reference() != &event.private_payload_ref
                || prepared_results.reference() != &group.facts_ref
            {
                return Err(NativeCheckpointError::Invalid);
            }
            let bytes = event
                .private_payload_ref
                .size()
                .checked_add(group.facts_ref.size())
                .ok_or(NativeCheckpointError::Budget)?;
            budget.charge_work(2, bytes)?;
            prepared_event.verify_retained(store, event.private_payload_ref.as_str())?;
            prepared_results.verify_retained(store, group.facts_ref.as_str())?;
        } else {
            // Absence is historical, not permission to rewrite or invent bytes.
            self.historical = true;
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or(NativeCheckpointError::Budget)?;
        Ok(())
    }
    pub(super) fn last_verified_group(
        &self,
    ) -> Result<&'a super::history::Group, NativeCheckpointError> {
        let index = self
            .next
            .checked_sub(1)
            .ok_or(NativeCheckpointError::Invalid)?;
        self.sources
            .groups
            .get(index)
            .ok_or(NativeCheckpointError::Invalid)
    }
    pub(super) fn finish(self) -> Result<NativeHistoryEvidence, NativeCheckpointError> {
        if self.next != self.sources.groups.len() {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(if self.historical {
            NativeHistoryEvidence::HistoricalUnqualified
        } else if self.land_play {
            NativeHistoryEvidence::LandPlayTransitionsV1
        } else if self.activity {
            NativeHistoryEvidence::ActivityTransitionsV1
        } else {
            NativeHistoryEvidence::TransitionV1
        })
    }
}
