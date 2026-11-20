//! Private durable publication contracts and repository, without a dispatcher.
//! Service credentials stay server-side. Destination is configured adapter scope,
//! not a separate database principal or browser-supplied delivery authority.
use crate::{
    ApiResponse,
    amount::Amount,
    backend::{BackendClient, BackendConfig, BackendError},
    commands::CommandDisposition,
    provider_json::{Object, ObjectResponse},
    source_protocol::{SourceProof, valid_digest},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, error::Error, fmt, time::Duration};
use uuid::Uuid;

/// Canonical destination queues, configured only by a trusted server adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum Destination {
    GameHost,
    MatchLifecycle,
    MatchSettlement,
}
impl TryFrom<String> for Destination {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "game_host" => Ok(Self::GameHost),
            "match_lifecycle" => Ok(Self::MatchLifecycle),
            "match_settlement" => Ok(Self::MatchSettlement),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Canonical publication kind; it determines the accepted producer source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum HandoffKind {
    ReadyMatch,
    ReservedGame,
    GameOutcome,
    MatchOutcome,
    BetweenDeadline,
}
impl TryFrom<String> for HandoffKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "ready_match" => Ok(Self::ReadyMatch),
            "reserved_game" => Ok(Self::ReservedGame),
            "game_outcome" => Ok(Self::GameOutcome),
            "match_outcome" => Ok(Self::MatchOutcome),
            "between_deadline" => Ok(Self::BetweenDeadline),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Publication observation, separate from whether a command receipt is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PublicationOutcome {
    Unknown,
    Published,
}
impl TryFrom<String> for PublicationOutcome {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "unknown" => Ok(Self::Unknown),
            "published" => Ok(Self::Published),
            _ => Err("invalid string enum value"),
        }
    }
}

/// The retained armed timer snapshot, not the mutable current decision timer.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimerSnapshot {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub decision_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub after_game_id: Uuid,
    pub opened_at: DateTime<Utc>,
    pub basis_revision: Amount,
    pub clock_revision: Amount,
    pub policy_ref: SourceProof,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub origin_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub source_receipt_id: Uuid,
    pub availability_revision: Amount,
    pub accounted_at: DateTime<Utc>,
    pub due_at: DateTime<Utc>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub remaining_us: Option<Amount>,
}
impl TimerSnapshot {
    /// Validate the retained armed form shared by publication and due-work discovery.
    pub fn valid_armed(&self) -> bool {
        self.version == "between-deadline-snapshot-v1"
            && !self.revision.is_zero()
            && self.remaining_us.is_none()
            && self.accounted_at >= self.opened_at
            && self.due_at >= self.opened_at
    }
}
/// Exact accepted producer manifest. No artifact contents or player-safe view is implied.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceManifest {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub handoff_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub game_id: Option<Uuid>,
    pub source_ref: String,
    pub destination: Destination,
    pub kind: HandoffKind,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub producer_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub timer: Option<TimerSnapshot>,
}
impl SourceManifest {
    /// Check source shape and cross-field identity; accepted proof remains SQL-owned.
    pub fn valid_for(&self, destination: Destination) -> bool {
        if self.version != "durable-handoff-source-v1" || self.destination != destination {
            return false;
        }
        match (self.kind, destination) {
            (HandoffKind::ReadyMatch, Destination::GameHost) => {
                self.game_id.is_some()
                    && self.timer.is_none()
                    && self.source_ref == self.match_id.to_string()
            }
            (HandoffKind::ReservedGame, Destination::GameHost) => {
                self.game_id
                    .is_some_and(|id| self.source_ref == id.to_string())
                    && self.timer.is_none()
            }
            (HandoffKind::GameOutcome, Destination::MatchLifecycle) => {
                self.game_id.is_some()
                    && self.timer.is_none()
                    && crate::identifiers::parse(&self.source_ref).is_ok()
            }
            (HandoffKind::MatchOutcome, Destination::MatchSettlement) => {
                self.timer.is_none() && crate::identifiers::parse(&self.source_ref).is_ok()
            }
            (HandoffKind::BetweenDeadline, Destination::MatchLifecycle) => {
                let Some(timer) = &self.timer else {
                    return false;
                };
                self.game_id.is_none()
                    && timer.valid_armed()
                    && timer.match_id == self.match_id
                    && timer.source_receipt_id == self.producer_receipt_id
                    && (self.source_ref == self.producer_receipt_id.to_string()
                        || self.source_ref
                            == format!("{}:{}", self.producer_receipt_id, timer.decision_id))
            }
            _ => false,
        }
    }
}
/// Dispatch identity and the different accepted preparation receipt identity.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedHandoff {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub handoff_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub preparation_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub dispatch_key: Uuid,
    pub destination: Destination,
    pub source_hash: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub manifest: SourceManifest,
}
impl PreparedHandoff {
    /// Validate the original target/destination without recomputing Postgres JSONB hashes.
    pub fn valid_for(&self, target: Uuid, destination: Destination) -> bool {
        self.handoff_id == target
            && self.dispatch_key == target
            && self.destination == destination
            && valid_digest(&self.source_hash)
            && self.manifest.handoff_id == target
            && self.manifest.valid_for(destination)
    }
}
/// Trusted destination evidence. Retain this exact original input for recovery.
/// Proof syntax does not establish successful delivery.
pub struct PublicationObservation {
    pub handoff_id: Uuid,
    pub preparation_id: Uuid,
    pub outcome: PublicationOutcome,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
impl PublicationObservation {
    fn valid(&self) -> bool {
        !self.handoff_id.is_nil() && !self.preparation_id.is_nil() && !self.request_id.is_nil()
    }
}
/// Accepted observation; Unknown still leaves the handoff pending.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub preparation_id: Uuid,
    pub outcome: PublicationOutcome,
}
impl PublicationAck {
    /// Bind the response to the original observation, never just its request UUID.
    pub fn valid_for(&self, original: &PublicationObservation) -> bool {
        self.resource_id == original.handoff_id
            && self.preparation_id == original.preparation_id
            && self.outcome == original.outcome
    }
}
/// Internal synthesized continuation: SQL itself returns a bare ordered array.
pub struct HandoffPage {
    pub items: Vec<SourceManifest>,
    pub next_cursor: Option<Uuid>,
}
/// Safe private-port failure while preserving an underlying transport cause.
pub struct HandoffError {
    source: Option<BackendError>,
}
impl HandoffError {
    fn protocol() -> Self {
        Self { source: None }
    }
}
impl From<BackendError> for HandoffError {
    fn from(source: BackendError) -> Self {
        Self {
            source: Some(source),
        }
    }
}
impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("handoff disposition is unavailable")
    }
}
impl fmt::Debug for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for HandoffError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| s as &(dyn Error + 'static))
    }
}
/// Narrow blocking repository for trusted server use. This does not publish work.
pub struct HandoffRepository {
    backend: BackendClient,
    destination: Destination,
}
fn already_acknowledged(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && errors
            .get("handoff")
            .is_some_and(|v| v == "already acknowledged")
}
impl HandoffRepository {
    /// Construct server-only transport from configured Supabase secrets.
    /// # Errors
    /// Rejects missing/invalid configuration and unavailable transport initialization.
    pub fn from_env(destination: Destination) -> Result<Self, HandoffError> {
        Self::new(BackendConfig::from_env()?, destination)
    }
    fn new(config: BackendConfig, destination: Destination) -> Result<Self, HandoffError> {
        Ok(Self {
            backend: BackendClient::new(
                config,
                "Spellwake-Private-Handoff-Repository/0.1",
                Duration::from_secs(60),
            )?,
            destination,
        })
    }
    /// Read up to100 pending manifests, with exclusive UUID continuation.
    /// # Errors
    /// Rejects invalid bounds, unavailable sources, malformed or cross-bound responses.
    pub fn pending(&self, after: Option<Uuid>, limit: u16) -> Result<HandoffPage, HandoffError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
            return Err(HandoffError::protocol());
        }
        let items: Vec<Object<SourceManifest>> = self.backend.rpc(
            "read_handoff_delivery_work",
            &json!({"p_destination":self.destination,"p_after":after,"p_limit":limit}),
        )?;
        let items: Vec<_> = items.into_iter().map(|v| v.0).collect();
        if items.len() > usize::from(limit)
            || items.iter().any(|v| {
                !v.valid_for(self.destination) || after.is_some_and(|id| v.handoff_id <= id)
            })
            || items.windows(2).any(|v| v[0].handoff_id >= v[1].handoff_id)
        {
            return Err(HandoffError::protocol());
        }
        let next_cursor = if items.len() == usize::from(limit) {
            items.last().map(|v| v.handoff_id)
        } else {
            None
        };
        Ok(HandoffPage { items, next_cursor })
    }
    /// Prepare or recover the one immutable dispatch for a handoff.
    /// # Errors
    /// Rejects nil targets, unavailable source proofs and malformed/context-mismatched results.
    pub fn prepare(&self, handoff: Uuid) -> Result<PreparedHandoff, HandoffError> {
        if handoff.is_nil() {
            return Err(HandoffError::protocol());
        }
        let ObjectResponse(response) = self.backend.rpc_object(
            "prepare_handoff_delivery",
            &json!({"p_destination":self.destination,"p_handoff":handoff}),
        )?;
        match response {
            ApiResponse::Success { data: prepared }
                if PreparedHandoff::valid_for(&prepared, handoff, self.destination) =>
            {
                Ok(prepared)
            }
            _ => Err(HandoffError::protocol()),
        }
    }
    fn parameters(
        &self,
        original: &PublicationObservation,
    ) -> Result<serde_json::Value, HandoffError> {
        if !original.valid() {
            return Err(HandoffError::protocol());
        }
        Ok(
            json!({"p_destination":self.destination,"p_handoff":original.handoff_id,"p_preparation":original.preparation_id,"p_outcome":original.outcome,"p_proof":original.proof_ref.as_str(),"p_request":original.request_id}),
        )
    }
    /// Record destination evidence; transport failure remains unresolved under this original request.
    /// # Errors
    /// Rejects invalid identity, unavailable transport/source and malformed or mismatched acknowledgment.
    pub fn acknowledge(
        &self,
        original: &PublicationObservation,
    ) -> Result<ApiResponse<PublicationAck>, HandoffError> {
        let ObjectResponse(response) = self
            .backend
            .rpc_object("acknowledge_handoff_delivery", &self.parameters(original)?)?;
        match &response {
            ApiResponse::Success { data } if PublicationAck::valid_for(data, original) => {
                Ok(response)
            }
            ApiResponse::Fail { data } if already_acknowledged(data) => Ok(response),
            _ => Err(HandoffError::protocol()),
        }
    }
    /// Recover only this exact original observation, including its proof and preparation receipt.
    /// # Errors
    /// Rejects invalid identity, unavailable transport and inconsistent receipt states or context.
    pub fn disposition(
        &self,
        original: &PublicationObservation,
    ) -> Result<ApiResponse<CommandDisposition<PublicationAck>>, HandoffError> {
        let ObjectResponse(response): ObjectResponse<CommandDisposition<PublicationAck>> = self
            .backend
            .rpc_object("read_handoff_delivery_ack", &self.parameters(original)?)?;
        let valid = match &response {
            ApiResponse::Success {
                data: CommandDisposition::Unknown | CommandDisposition::Pending,
            } => true,
            ApiResponse::Success {
                data: CommandDisposition::Accepted { ack },
            } => ack.valid_for(original),
            ApiResponse::Success {
                data: CommandDisposition::Rejected { errors },
            } => already_acknowledged(errors),
            ApiResponse::Fail { data } => {
                data.len() == 1 && data.get("request").is_some_and(|v| v == "conflict")
            }
            _ => false,
        };
        if valid {
            Ok(response)
        } else {
            Err(HandoffError::protocol())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Mock, Step, positional};
    use serde_json::Value;
    fn repository(mock: &Mock, destination: Destination) -> HandoffRepository {
        HandoffRepository::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            destination,
        )
        .unwrap()
    }
    fn manifest(id: Uuid) -> Value {
        let matched = Uuid::new_v4();
        json!({"version":"durable-handoff-source-v1","handoffId":id,"matchId":matched,"gameId":Uuid::new_v4(),"sourceRef":matched,"destination":"game_host","kind":"ready_match","producerReceiptId":Uuid::new_v4(),"timer":null})
    }
    fn prepared(id: Uuid, preparation: Uuid) -> Value {
        json!({"status":"success","data":{"handoffId":id,"preparationId":preparation,"dispatchKey":id,"destination":"game_host","sourceHash":"a".repeat(64),"manifest":manifest(id)}})
    }
    fn observation() -> PublicationObservation {
        PublicationObservation {
            handoff_id: Uuid::new_v4(),
            preparation_id: Uuid::new_v4(),
            outcome: PublicationOutcome::Unknown,
            proof_ref: "fixture:verified-destination-observation".parse().unwrap(),
            request_id: Uuid::new_v4(),
        }
    }
    fn ack(original: &PublicationObservation) -> Value {
        json!({"status":"success","data":{"resourceId":original.handoff_id,"preparationId":original.preparation_id,"outcome":original.outcome}})
    }
    #[test]
    fn private_repository_preserves_preparation_identity_array_pagination_and_original_recovery() {
        let id = Uuid::from_u128(100);
        let preparation = Uuid::new_v4();
        let original = PublicationObservation {
            handoff_id: id,
            preparation_id: preparation,
            ..observation()
        };
        let prepared = prepared(id, preparation);
        let ack = ack(&original);
        let mock = Mock::new(vec![
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_work",
                json!([prepared["data"]["manifest"].clone()]),
            ),
            Step::json("/rest/v1/rpc/prepare_handoff_delivery", prepared),
            Step::json("/rest/v1/rpc/acknowledge_handoff_delivery", ack.clone()),
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"success","data":{"state":"accepted","response":ack}}),
            ),
            Step::json("/rest/v1/rpc/read_handoff_delivery_work", json!([])),
        ]);
        let repository = repository(&mock, Destination::GameHost);
        assert_eq!(repository.pending(None, 1).unwrap().next_cursor, Some(id));
        let prepared = repository.prepare(id).unwrap();
        assert_eq!(prepared.preparation_id, preparation);
        assert_eq!(prepared.dispatch_key, id);
        assert!(
            matches!(repository.acknowledge(&original).unwrap(),ApiResponse::Success { data } if data.outcome==PublicationOutcome::Unknown)
        );
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Accepted { .. }
            }
        ));
        assert!(
            repository
                .pending(Some(id), 1)
                .unwrap()
                .next_cursor
                .is_none()
        );
        let seen = mock.finish();
        assert_eq!(
            serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
            json!({"p_destination":"game_host","p_after":null,"p_limit":1})
        );
        assert_eq!(seen[2].2, seen[3].2);
        assert_eq!(
            serde_json::from_slice::<Value>(&seen[2].2).unwrap()["p_preparation"],
            preparation.to_string()
        );
    }
    #[test]
    fn lost_ack_is_recovered_without_changing_proof_or_request_identity() {
        let original = observation();
        let mut lost = Step::json("/rest/v1/rpc/acknowledge_handoff_delivery", ack(&original));
        lost.drop_ack = true;
        let mock = Mock::new(vec![
            lost,
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"success","data":{"state":"accepted","response":ack(&original)}}),
            ),
        ]);
        let repository = repository(&mock, Destination::GameHost);
        let error = match repository.acknowledge(&original) {
            Err(error) => error,
            Ok(_) => panic!("lost response must remain unresolved"),
        };
        assert!(error.source().is_some());
        assert!(!format!("{error:?} {error}").contains(original.proof_ref.as_str()));
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Accepted { .. }
            }
        ));
        let seen = mock.finish();
        assert_eq!(seen[0].2, seen[1].2);
    }
    #[test]
    fn pending_and_unknown_receipts_are_distinct_from_publication_outcomes_and_terminal_rejection()
    {
        let original = observation();
        let rejection = json!({"status":"fail","data":{"handoff":"already acknowledged"}});
        let mock = Mock::new(vec![
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"success","data":{"state":"unknown","response":null}}),
            ),
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"success","data":{"state":"pending","response":null}}),
            ),
            Step::json(
                "/rest/v1/rpc/acknowledge_handoff_delivery",
                rejection.clone(),
            ),
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"success","data":{"state":"rejected","response":rejection}}),
            ),
            Step::json(
                "/rest/v1/rpc/read_handoff_delivery_ack",
                json!({"status":"fail","data":{"request":"conflict"}}),
            ),
        ]);
        let repository = repository(&mock, Destination::MatchLifecycle);
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Unknown
            }
        ));
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Pending
            }
        ));
        assert!(matches!(
            repository.acknowledge(&original).unwrap(),
            ApiResponse::Fail { .. }
        ));
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Rejected { .. }
            }
        ));
        assert!(matches!(
            repository.disposition(&original).unwrap(),
            ApiResponse::Fail { .. }
        ));
        mock.finish();
    }
    #[test]
    fn malformed_preparations_and_discovery_pages_never_escape_the_private_boundary() {
        let id = Uuid::from_u128(100);
        let valid = prepared(id, Uuid::new_v4());
        let mut malformed = vec![
            Value::Null,
            json!([]),
            json!({"status":"success","data":positional(&valid["data"],&["handoffId","preparationId","dispatchKey","destination","sourceHash","manifest"])}),
            valid.clone(),
            valid.clone(),
            valid.clone(),
            valid.clone(),
        ];
        malformed[3]["data"]["dispatchKey"] = json!(Uuid::new_v4());
        malformed[4]["data"]["manifest"]["timer"] = json!({});
        malformed[5]["data"]["sourceHash"] = json!("A".repeat(64));
        malformed[6]["data"]["manifest"]["destination"] = json!("match_settlement");
        for body in malformed {
            let mock = Mock::new(vec![Step::json(
                "/rest/v1/rpc/prepare_handoff_delivery",
                body,
            )]);
            assert!(
                repository(&mock, Destination::GameHost)
                    .prepare(id)
                    .is_err()
            );
            mock.finish();
        }
        let source = manifest(id);
        for body in [
            json!({"items":[]}),
            json!([source.clone(), source.clone()]),
            json!([manifest(Uuid::from_u128(99))]),
            json!([positional(
                &source,
                &[
                    "version",
                    "handoffId",
                    "matchId",
                    "gameId",
                    "sourceRef",
                    "destination",
                    "kind",
                    "producerReceiptId",
                    "timer"
                ]
            )]),
        ] {
            let mock = Mock::new(vec![Step::json(
                "/rest/v1/rpc/read_handoff_delivery_work",
                body,
            )]);
            assert!(
                repository(&mock, Destination::GameHost)
                    .pending(Some(Uuid::from_u128(99)), 1)
                    .is_err()
            );
            mock.finish();
        }
    }
    #[test]
    fn malformed_observation_responses_and_duplicate_keys_fail_closed() {
        let original = observation();
        let valid = ack(&original);
        let mut invalid = vec![
            valid.clone(),
            valid.clone(),
            valid.clone(),
            json!({"status":"fail","data":{"request":"conflict"}}),
            json!({"status":"fail","data":{"handoff":"already acknowledged","private":"source"}}),
            json!({"status":"success","data":positional(&valid["data"],&["resourceId","preparationId","outcome"])}),
        ];
        invalid[0]["data"]["resourceId"] = json!(Uuid::new_v4());
        invalid[1]["data"]["preparationId"] = json!(Uuid::new_v4());
        invalid[2]["data"]["outcome"] = json!("published");
        for body in invalid {
            for recovered in [false, true] {
                let (path, body) = if recovered {
                    (
                        "/rest/v1/rpc/read_handoff_delivery_ack",
                        json!({"status":"success","data":{"state":"accepted","response":body}}),
                    )
                } else {
                    ("/rest/v1/rpc/acknowledge_handoff_delivery", body.clone())
                };
                let mock = Mock::new(vec![Step::json(path, body)]);
                let repository = repository(&mock, Destination::GameHost);
                assert!(if recovered {
                    repository.disposition(&original).is_err()
                } else {
                    repository.acknowledge(&original).is_err()
                });
                mock.finish();
            }
        }
        let duplicate = format!(
            "{{\"status\":\"success\",\"data\":{{\"resourceId\":\"{}\",\"resourceId\":\"{}\",\"preparationId\":\"{}\",\"outcome\":\"unknown\"}}}}",
            original.handoff_id, original.handoff_id, original.preparation_id
        );
        let mut step = Step::json("/rest/v1/rpc/acknowledge_handoff_delivery", Value::Null);
        step.body = duplicate.into_bytes();
        let mock = Mock::new(vec![step]);
        assert!(
            repository(&mock, Destination::GameHost)
                .acknowledge(&original)
                .is_err()
        );
        mock.finish();
    }
    #[test]
    fn invalid_original_ids_and_bounds_do_no_transport_work() {
        let mock = Mock::new(vec![]);
        let repository = repository(&mock, Destination::GameHost);
        assert!(repository.pending(None, 0).is_err());
        assert!(repository.pending(None, 101).is_err());
        assert!(repository.pending(Some(Uuid::nil()), 1).is_err());
        assert!(repository.prepare(Uuid::nil()).is_err());
        let mut original = observation();
        original.request_id = Uuid::nil();
        assert!(repository.acknowledge(&original).is_err());
        assert!(repository.disposition(&original).is_err());
        assert!(mock.finish().is_empty());
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<Destination>(&[
            "game_host",
            "match_lifecycle",
            "match_settlement",
        ]);
        crate::test_http::string_enum_variants::<HandoffKind>(&[
            ("ready_match", "ReadyMatch"),
            ("reserved_game", "ReservedGame"),
            ("game_outcome", "GameOutcome"),
            ("match_outcome", "MatchOutcome"),
            ("between_deadline", "BetweenDeadline"),
        ]);
        crate::test_http::string_enum_roundtrip::<PublicationOutcome>(&["unknown", "published"]);
    }
}
