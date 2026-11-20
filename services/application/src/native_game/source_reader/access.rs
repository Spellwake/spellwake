//! Fresh human participant disclosure, not a durable host/stream authorization.
use super::*;
use crate::player_actor::PlayerActor as Actor;
use crate::{actor::RequestActor, source_protocol::valid_digest};

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    native_epoch: NativeRevision,
    native_revision: NativeRevision,
    runtime_id: String,
    program_source_id: String,
    initial_ref: NativeArtifactRef,
    state_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    checkpoint_id: Uuid,
    last_event_sequence: Amount,
    game_state: GamePhase,
    match_state: MatchPhase,
    availability: Availability,
}
impl Disclosure {
    fn completed_disclosure_absent(&self) -> bool {
        !self.completed
    }
}
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disclosure {
    live: bool,
    completed: bool,
    operational_prefix: bool,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    replay_manifest_id: Option<Uuid>,
}
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    #[serde(deserialize_with = "crate::provider_json::object")]
    actor: Actor,
    #[serde(deserialize_with = "crate::provider_json::object")]
    source: Source,
    #[serde(deserialize_with = "crate::provider_json::object")]
    disclosure: Disclosure,
}
impl Record {
    pub(super) fn into_access(
        self,
        account: Uuid,
        game: Uuid,
        context: Uuid,
    ) -> Result<NativeHumanAccess, NativeSourceError> {
        if !self.valid(account, game, context) {
            return Err(invalid());
        }
        Ok(NativeHumanAccess {
            record: Box::new(self),
        })
    }
    fn valid(&self, account: Uuid, game: Uuid, context: Uuid) -> bool {
        let a = &self.actor;
        a.kind == ControllerKind::Human
            && a.account_id == Some(account)
            && a.grant_id.is_none()
            && a.binding_id.is_none()
            && a.binding_epoch.is_none()
            && self.valid_source(game, context)
    }
    fn valid_source(&self, game: Uuid, context: Uuid) -> bool {
        let s = &self.source;
        let d = &self.disclosure;
        let scoped = |reference: &NativeArtifactRef, role| {
            reference.game_id() == game
                && reference.context_id() == context
                && reference.role() == role
        };
        s.game_id == game
            && s.context_id == context
            && valid_digest(&s.runtime_id)
            && valid_digest(&s.program_source_id)
            && scoped(&s.initial_ref, NativeArtifactRole::Initial)
            && s.initial_ref.size() <= 1_048_576
            && scoped(&s.state_ref, NativeArtifactRole::State)
            && s.state_ref.size() <= 32_768
            && s.last_event_sequence >= Amount::from(s.native_revision.get())
            && matches!(
                s.game_state,
                GamePhase::Active | GamePhase::Completed | GamePhase::NoContest
            )
            && s.match_state != MatchPhase::Admitting
            && d.live
            && !(d.completed && d.operational_prefix)
            && (!d.completed || matches!(s.game_state, GamePhase::Completed | GamePhase::NoContest))
            && (!d.operational_prefix
                || s.game_state == GamePhase::NoContest && d.replay_manifest_id.is_some())
            && (d.replay_manifest_id.is_none() || d.completed || d.operational_prefix)
    }
}
impl Record {
    fn same_except_publication(&self, published: &Self) -> bool {
        let (a, b) = (self, published);
        a.actor == b.actor
            && a.source == b.source
            && a.disclosure.live == b.disclosure.live
            && a.disclosure.completed == b.disclosure.completed
            && (a.disclosure.operational_prefix == b.disclosure.operational_prefix
                || !a.disclosure.operational_prefix
                    && b.disclosure.operational_prefix
                    && b.source.game_state == GamePhase::NoContest)
            && a.disclosure.replay_manifest_id.is_none()
            && b.disclosure.replay_manifest_id.is_some()
    }
}
impl Record {
    fn basis(&self) -> PlayerBasis {
        let s = &self.source;
        PlayerBasis {
            game_id: s.game_id,
            context_id: s.context_id,
            epoch: s.native_epoch,
            revision: s.native_revision,
        }
    }
    pub(super) fn matches_source_metadata(
        &self,
        observed: &NativeGameSource,
        opening: &NativeOpeningSource,
    ) -> bool {
        let s = &self.source;
        s.game_id == observed.game_id()
            && s.context_id == observed.context_ids().0
            && s.match_id == observed.match_id()
            && s.native_epoch == opening.native_epoch()
            && Amount::from(s.native_revision.get()) == *observed.accepted_revision()
            && s.initial_ref == observed.record.initial_ref
            && s.state_ref == observed.record.checkpoint.state_ref
            && s.runtime_id == opening.runtime_id()
            && s.program_source_id == opening.program_source_id()
            && s.checkpoint_id == observed.checkpoint_ids().0
            && s.last_event_sequence == *observed.last_event_sequence()
            && (s.game_state, s.match_state) == observed.phases()
            && s.availability == observed.match_availability()
    }
    pub(super) fn matches_source(&self, recovered: &RecoveredNativeGame) -> bool {
        let s = &self.source;
        let checkpoint = recovered.checkpoint();
        self.matches_source_metadata(recovered.source(), recovered.opening())
            && self.basis() == *checkpoint.basis()
            && &s.initial_ref == checkpoint.initial_reference()
            && &s.state_ref == checkpoint.reference()
    }
}
/// Controller access is a fresh observation, never a credential or disclosure lease.
/// The current SQL actor chooses the seat; callers cannot select it.
pub struct NativeControllerAccess {
    record: Box<Record>,
}
/// Declared canonical controller access outcomes; no human session is synthesized.
pub enum NativeControllerAccessOutcome {
    Granted(NativeControllerAccess),
    Unavailable,
}
impl NativeControllerAccess {
    pub(super) fn matches_controller(
        &self,
        proof: &crate::controller_possession::PossessedController,
    ) -> bool {
        self.record.actor.grant_id == Some(proof.grant())
            && self.record.actor.binding_id == Some(proof.binding())
            && self.record.actor.binding_epoch.as_ref() == Some(proof.epoch())
    }
    pub(super) fn active(&self) -> bool {
        self.record.source.game_state == GamePhase::Active
            && self.record.source.match_state == MatchPhase::Active
    }
    pub(super) fn matches_source(&self, recovered: &RecoveredNativeGame) -> bool {
        self.record.matches_source(recovered)
    }
    pub(super) fn same_delivery_source(&self, other: &Self) -> bool {
        self.record == other.record
    }
    pub(super) fn same_except_publication(&self, published: &Self) -> bool {
        self.record.same_except_publication(&published.record)
    }
    /// Bind only the exact independently recovered accepted native cut and AI seat.
    /// # Errors
    /// Rejects foreign/stale source or a viewer absent from the accepted roster.
    pub fn bind(
        &self,
        recovered: &RecoveredNativeGame,
    ) -> Result<NativeBinding, NativeSourceError> {
        if !self.matches_source(recovered) {
            return Err(invalid());
        }
        recovered
            .checkpoint()
            .binding(self.viewer_seat_id())
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))
    }
    /// AI seat selected by the current canonical grant/binding/match join.
    pub fn viewer_seat_id(&self) -> Uuid {
        self.record.actor.seat_id
    }
    /// Exact observed native cut, with no permission to deliver it later unchanged.
    pub fn basis(&self) -> PlayerBasis {
        let s = &self.record.source;
        PlayerBasis {
            game_id: s.game_id,
            context_id: s.context_id,
            epoch: s.native_epoch,
            revision: s.native_revision,
        }
    }
}
impl Record {
    pub(super) fn into_controller_access(
        self,
        proof: &crate::controller_possession::PossessedController,
        game: Uuid,
        context: Uuid,
    ) -> Result<NativeControllerAccess, NativeSourceError> {
        let a = &self.actor;
        if !a.matches_controller(proof)
            || !self.valid_source(game, context)
            || !matches!(
                self.source.match_state,
                MatchPhase::Ready | MatchPhase::Active | MatchPhase::BetweenGames
            )
        {
            return Err(invalid());
        }
        Ok(NativeControllerAccess {
            record: Box::new(self),
        })
    }
}
/// Declared business outcomes, distinct from malformed upstream or transport errors.
pub enum NativeHumanAccessOutcome {
    Granted(NativeHumanAccess),
    Unauthorized,
    Unavailable,
}
/// Sealed participant disclosure observation, refreshed before each delivery.
#[derive(Clone)]
pub struct NativeHumanAccess {
    record: Box<Record>,
}
impl NativeHumanAccess {
    /// Accepted operational no-contest of this game, before any native
    /// conclusion. Classification only: SQL publication proves the incident.
    pub(crate) fn operational_no_contest(&self) -> bool {
        self.record.source.game_state == GamePhase::NoContest
            && self.record.disclosure.completed_disclosure_absent()
    }
    pub(crate) fn active(&self) -> bool {
        self.record.source.game_state == GamePhase::Active
            && self.record.source.match_state == MatchPhase::Active
    }

    #[cfg(test)]
    pub(super) fn fixture_for_outcome_test(revision: u64) -> Self {
        let mut value = tests::fixture();
        value["data"]["source"]["nativeRevision"] = json!(revision.to_string());
        value["data"]["source"]["lastEventSequence"] = json!(revision.to_string());
        Self {
            record: Box::new(serde_json::from_value(value["data"].clone()).unwrap()),
        }
    }
    #[cfg(test)]
    pub(super) fn fixture_for_publication_test(revision: u64, published: bool) -> Self {
        let mut value = tests::fixture();
        value["data"]["source"]["nativeRevision"] = json!(revision.to_string());
        value["data"]["source"]["lastEventSequence"] = json!(revision.to_string());
        value["data"]["source"]["gameState"] = json!("completed");
        value["data"]["disclosure"]["completed"] = json!(true);
        if published {
            value["data"]["disclosure"]["replayManifestId"] =
                json!(uuid::Uuid::from_u128(9).to_string());
        }
        Self {
            record: Box::new(serde_json::from_value(value["data"].clone()).unwrap()),
        }
    }
    pub(crate) fn same_delivery_source(&self, other: &Self) -> bool {
        self.record == other.record
    }
    /// Same actor, source and disclosure flags, except that a replay manifest
    /// was published between the two observations (none before, one after).
    pub(crate) fn same_except_publication(&self, published: &Self) -> bool {
        self.record.same_except_publication(&published.record)
    }
    pub(super) fn frontier(&self) -> &Amount {
        &self.record.source.last_event_sequence
    }
    pub(super) fn matches_actor(&self, actor: &RequestActor) -> bool {
        actor.account().is_some() && self.record.actor.account_id == actor.account()
    }
    /// Seat selected by current session/account authority, never caller selection.
    pub fn viewer_seat_id(&self) -> Uuid {
        self.record.actor.seat_id
    }
    /// Exact native cut selected by the access observation, not host fencing.
    pub fn basis(&self) -> PlayerBasis {
        let s = &self.record.source;
        PlayerBasis {
            game_id: s.game_id,
            context_id: s.context_id,
            epoch: s.native_epoch,
            revision: s.native_revision,
        }
    }
    /// Full participant disclosure is allowed only by the accepted terminal source.
    pub fn completed_disclosure(&self) -> Option<NativeDisclosure> {
        self.record
            .disclosure
            .completed
            .then_some(NativeDisclosure::CompletedParticipant)
    }
    /// Accepted replay identity and operational-prefix permission, not a public URL.
    pub fn replay(&self) -> (Option<Uuid>, bool) {
        (
            self.record.disclosure.replay_manifest_id,
            self.record.disclosure.operational_prefix,
        )
    }
    pub(super) fn matches_source_metadata(
        &self,
        observed: &NativeGameSource,
        opening: &NativeOpeningSource,
    ) -> bool {
        self.record.matches_source_metadata(observed, opening)
    }
    pub(super) fn matches_source(&self, recovered: &RecoveredNativeGame) -> bool {
        self.record.matches_source(recovered)
    }
    /// Bind this viewer to an independently reconstructed exact accepted cut.
    /// # Errors
    /// Rejects foreign/stale source, original runtime/program/artifact identity,
    /// checkpoint/frontier/phase mismatch or a viewer absent from the native roster.
    /// Equality is not a lease: command commits/delivery must refresh authority.
    pub fn bind(
        &self,
        recovered: &RecoveredNativeGame,
    ) -> Result<NativeBinding, NativeSourceError> {
        if !self.matches_source(recovered) {
            return Err(invalid());
        }
        let checkpoint = recovered.checkpoint();
        checkpoint
            .binding(self.viewer_seat_id())
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))
    }
}
impl NativeSourceRepository {
    /// Resolve fresh participant disclosure using a verified original bearer actor.
    /// SQL rechecks current Auth/session and admitted human ownership. A sponsor
    /// account cannot use this boundary as its AI seat. No controller secret
    /// locator is accepted as credential possession.
    /// # Errors
    /// Rejects nil scope, malformed/duplicate/positional records, foreign account/
    /// game/context, inconsistent disclosure and unknown failure vocabulary.
    pub fn human_access(
        &self,
        actor: &RequestActor,
        game: Uuid,
        context: Uuid,
    ) -> Result<NativeHumanAccessOutcome, NativeSourceError> {
        if game.is_nil() || context.is_nil() {
            return Err(invalid());
        }
        let Some(account) = actor.account() else {
            return Ok(NativeHumanAccessOutcome::Unavailable);
        };
        self.human_identity(actor.subject(), actor.session(), account, game, context)
    }
    fn human_identity(
        &self,
        subject: Uuid,
        session: Uuid,
        account: Uuid,
        game: Uuid,
        context: Uuid,
    ) -> Result<NativeHumanAccessOutcome, NativeSourceError> {
        if [subject, session, account, game, context]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(invalid());
        }
        let response: ObjectResponse<Record> = self
            .backend
            .rpc(
                "read_native_human_access",
                &json!({"p_subject":subject,"p_session":session,"p_game":game,"p_context":context}),
            )
            .map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        match response.0 {
            ApiResponse::Success { data } => data
                .into_access(account, game, context)
                .map(NativeHumanAccessOutcome::Granted),
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data
                        .get("session")
                        .is_some_and(|value| value == "unauthorized") =>
            {
                Ok(NativeHumanAccessOutcome::Unauthorized)
            }
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data.get("game").is_some_and(|value| value == "unavailable") =>
            {
                Ok(NativeHumanAccessOutcome::Unavailable)
            }
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
