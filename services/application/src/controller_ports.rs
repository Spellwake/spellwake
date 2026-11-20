//! Private normalized controller source contracts.
//! SQL234 grants selected trusted entry points; internal helpers remain closed.
//! This module installs no provider, secret delivery, runner, model call,
//! RPC adapter or worker. Capability grants do not establish credential possession.
use crate::{
    ApiResponse,
    amount::Amount,
    source_protocol::{
        SecretReference, SourceProof, SourceResponseError, checked, flat_jsonb_octets, ids,
        one_failure, record_response,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
/// SQL172 bounds the original runtime terms before receipt creation.
pub const MAX_RUNTIME_TERMS_BYTES: usize = 262_144;
///172 nullable start work is fenced by current authority epoch and prior process stop.
pub struct ReadStartWork {
    pub binding_id: Uuid,
    pub epoch: Amount,
}
/// Private process startup identity. A present row does not prove running.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartWork {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub binding_id: Uuid,
    pub epoch: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub start_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub start_key: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub seat_id: Option<Uuid>,
}
impl ReadStartWork {
    /// Decode172's nullable object while preserving the original binding/epoch.
    /// # Errors
    /// Rejects invalid input, malformed/positional records and wrong startup context.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<Option<StartWork>, SourceResponseError> {
        if self.binding_id.is_nil() {
            return Err(SourceResponseError::context());
        }
        let work =
            serde_json::from_slice::<Option<crate::provider_json::Object<StartWork>>>(bytes)?
                .map(|v| v.0);
        let valid = work.as_ref().is_none_or(|w| {
            w.binding_id == self.binding_id && w.epoch == self.epoch && w.start_key == w.start_id
        });
        checked(valid, work)
    }
}
///215 bounded private discovery;172 remains the canonical historical row source.
/// Polls restart from null after a discovery pass. UUID ordering is not an atomic
/// snapshot or worker authorization, and confirmed rows may disappear between pages.
pub struct ReadStopWork {
    pub binding_id: Uuid,
    pub after: Option<Uuid>,
    pub limit: u16,
}
/// Original-byte acceptance budget. A SQL-valid page can exceed it and is rejected
/// without truncating source rows; the row-count limit is a separate constraint.
pub const MAX_STOP_WORK_PAGE_BYTES: usize = 1_048_576;
#[cfg(test)]
mod stop_page_tests;
/// Historical process work survives replacement of current binding pointers.
/// Retained untyped sources permit nullable start/process/provider fields.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopWork {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub binding_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub stop_id: Uuid,
    pub epoch: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub start_id: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub process_epoch: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub provider_ref: Option<String>,
}
/// Required-nullable cursor, preserving all source generations and nullable fields.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopWorkPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub binding_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<StopWork>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl ReadStopWork {
    /// Original discovery scope, never an authorization proof.
    pub fn valid(&self) -> bool {
        !self.binding_id.is_nil()
            && self.after.is_none_or(|id| !id.is_nil())
            && (1..=100).contains(&self.limit)
    }
    fn row_valid(&self, work: &StopWork) -> bool {
        self.valid()
            && work.binding_id == self.binding_id
            && !work.stop_id.is_nil()
            && work.start_id.is_none_or(|id| !id.is_nil())
            && self.after.is_none_or(|id| work.stop_id > id)
            && work
                .provider_ref
                .as_ref()
                .is_none_or(|value| !value.contains('\0'))
    }
    /// Decode215's actual bounded object, not a guessed newest-only stop.
    /// # Errors
    /// Rejects original bytes over budget, invalid scope, malformed/null omissions,
    /// foreign/duplicate/unordered rows and a cursor differing from the exact page.
    pub fn decode_page(&self, bytes: &[u8]) -> Result<StopWorkPage, SourceResponseError> {
        if !self.valid() || bytes.len() > MAX_STOP_WORK_PAGE_BYTES {
            return Err(SourceResponseError::context());
        }
        let page: StopWorkPage = crate::decode_wire_object(bytes)?;
        checked(page.valid_for(self), page)
    }
    /// Decode one actual SETOF row, not a fabricated page or newest-only pointer.
    /// # Errors
    /// Rejects malformed records, private text impossible in Postgres, or another binding's row.
    pub fn decode_row(&self, bytes: &[u8]) -> Result<StopWork, SourceResponseError> {
        if bytes.len() > MAX_STOP_WORK_PAGE_BYTES {
            return Err(SourceResponseError::context());
        }
        let work: StopWork = crate::decode_wire_object(bytes)?;
        checked(self.row_valid(&work), work)
    }
    /// Validate172's exclusive ascending stop receipt order without inventing a page size.
    pub fn ordered_after(&self, work: &StopWork, previous: Option<Uuid>) -> bool {
        self.row_valid(work) && previous.is_none_or(|id| !id.is_nil() && work.stop_id > id)
    }
}
impl StopWorkPage {
    /// Validate source scope and exact exclusive continuation without inferring absence.
    pub fn valid_for(&self, original: &ReadStopWork) -> bool {
        if !original.valid()
            || self.binding_id != original.binding_id
            || self.items.len() > usize::from(original.limit)
        {
            return false;
        }
        let mut previous = original.after;
        for work in &self.items {
            if !original.ordered_after(work, previous) {
                return false;
            }
            previous = Some(work.stop_id);
        }
        self.next_cursor
            == if self.items.len() == usize::from(original.limit) {
                self.items.last().map(|work| work.stop_id)
            } else {
                None
            }
    }
}
/// Actual observed provider state, not bootstrap intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcome {
    Running,
    Unavailable,
    Unknown,
}
/// Exact172 acknowledgment vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ProviderDisposition {
    Running,
    Unavailable,
    Unknown,
    #[serde(rename = "already observed")]
    AlreadyObserved,
}
impl TryFrom<String> for ProviderDisposition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "running" => Ok(Self::Running),
            "unavailable" => Ok(Self::Unavailable),
            "unknown" => Ok(Self::Unknown),
            "already observed" => Ok(Self::AlreadyObserved),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Original trusted observation. Provider refs/proof text stay out of Debug/Serialize.
pub struct ProviderObservation {
    pub binding_id: Uuid,
    pub epoch: Amount,
    pub start_id: Uuid,
    pub outcome: ProviderOutcome,
    pub provider_ref: Option<SourceProof>,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub disposition: ProviderDisposition,
}
impl ProviderObservation {
    /// Exact byte count of172's original flat JSONB terms, excluding request identity.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"bindingId":self.binding_id,"epoch":self.epoch,"startId":self.start_id,"outcome":self.outcome,"providerRef":self.provider_ref.as_ref().map(SourceProof::as_str),"proofRef":self.proof_ref.as_str()}),
        )
    }
    /// A running observation requires a provider locator; other outcomes requireNULL.
    pub fn valid(&self) -> bool {
        ids(&[self.binding_id, self.start_id, self.request_id])
            && self
                .original_terms_octets()
                .is_some_and(|size| size <= MAX_RUNTIME_TERMS_BYTES)
            && self.provider_ref.is_some() == (self.outcome == ProviderOutcome::Running)
    }
    /// Decode canonical observation/replay. An accepted Unknown is not an unknown receipt.
    /// # Errors
    /// Rejects invalid original input, malformed records, wrong target/outcome or undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<ProviderAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = self.valid()
            && match &response {
                ApiResponse::Success {
                    data:
                        ProviderAck {
                            resource_id,
                            disposition,
                        },
                } => {
                    *resource_id == self.binding_id
                        && matches!(
                            (self.outcome, *disposition),
                            (
                                ProviderOutcome::Running,
                                ProviderDisposition::Running | ProviderDisposition::AlreadyObserved
                            ) | (
                                ProviderOutcome::Unavailable,
                                ProviderDisposition::Unavailable
                            ) | (ProviderOutcome::Unknown, ProviderDisposition::Unknown)
                        )
                }
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[("source", "stale"), ("provider", "conflicting observation")],
                ),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///177 credential generation input. A locator is not a delivered credential.
pub struct IssueGrant {
    pub binding_id: Uuid,
    pub epoch: Amount,
    pub secret_ref: SecretReference,
    pub request_id: Uuid,
}
/// Issuance is pending; later observation can be unknown or delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum DeliveryPhase {
    Pending,
    Unknown,
    Delivered,
}
impl TryFrom<String> for DeliveryPhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "pending" => Ok(Self::Pending),
            "unknown" => Ok(Self::Unknown),
            "delivered" => Ok(Self::Delivered),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrantAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub delivery: DeliveryPhase,
}
impl IssueGrant {
    /// Exact byte count of177's original runtime terms; secret locators stay private.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"bindingId":self.binding_id,"epoch":self.epoch,"secretRef":self.secret_ref.as_str()}),
        )
    }
    /// Decode177's grant identity; it is not the requested binding identity.
    /// # Errors
    /// Rejects invalid input, nonpending issuance, malformed objects and undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<GrantAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = ids(&[self.binding_id, self.request_id])
            && self
                .original_terms_octets()
                .is_some_and(|size| size <= MAX_RUNTIME_TERMS_BYTES)
            && match &response {
                ApiResponse::Success {
                    data: GrantAck { delivery, .. },
                } => *delivery == DeliveryPhase::Pending,
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[
                        ("sponsor", "unavailable"),
                        ("binding", "stale or unavailable"),
                    ],
                ),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Trusted155 delivery observation, separate from initial grant allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    Unknown,
    Delivered,
}
pub struct GrantDeliveryObservation {
    pub grant_id: Uuid,
    pub basis: Amount,
    pub outcome: DeliveryOutcome,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
impl GrantDeliveryObservation {
    /// Exact byte count of155's original delivery terms, not the enriched receipt source.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"grantId":self.grant_id,"basis":self.basis,"outcome":self.outcome,"proofRef":self.proof_ref.as_str()}),
        )
    }
    /// Decode155 using the exact original grant/basis/proof/request on every retry.
    /// # Errors
    /// Rejects wrong grants/outcomes, malformed objects or undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<GrantAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = ids(&[self.grant_id, self.request_id])
            && self
                .original_terms_octets()
                .is_some_and(|size| size <= MAX_RUNTIME_TERMS_BYTES)
            && match &response {
                ApiResponse::Success {
                    data:
                        GrantAck {
                            resource_id,
                            delivery,
                        },
                } => {
                    *resource_id == self.grant_id
                        && matches!(
                            (self.outcome, *delivery),
                            (DeliveryOutcome::Unknown, DeliveryPhase::Unknown)
                                | (DeliveryOutcome::Delivered, DeliveryPhase::Delivered)
                        )
                }
                ApiResponse::Fail { data } => {
                    one_failure(data, &[("grant", "stale or unavailable")])
                }
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Trusted authenticated170 Ready source; the sponsor cannot impersonate this controller.
pub struct ReadyConfirmation {
    pub grant_id: Uuid,
    pub epoch: Amount,
    pub revision_id: Uuid,
    pub qualification_ref: SourceProof,
    pub authentication_ref: SourceProof,
    pub configuration_id: Uuid,
    pub request_id: Uuid,
}
/// Exact private Ready ack. Proof text must never be published or logged.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub epoch: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub revision_id: Uuid,
    pub qualification_ref: SourceProof,
    pub authentication_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub configuration_id: Uuid,
}
impl ReadyConfirmation {
    /// Decode against the actual grant's resolved binding, not a sponsor-supplied binding.
    /// Legacy NULL issue sources may passSQL without proving credential delivery.
    /// # Errors
    /// Rejects invalid source input/context, changed accepted proof fields or undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        binding: Uuid,
    ) -> Result<ApiResponse<ReadyAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = ids(&[
            self.grant_id,
            self.revision_id,
            self.configuration_id,
            self.request_id,
            binding,
        ]) && match &response {
            ApiResponse::Success {
                data:
                    ReadyAck {
                        resource_id,
                        epoch,
                        revision_id,
                        qualification_ref,
                        authentication_ref,
                        configuration_id,
                    },
            } => {
                *resource_id == binding
                    && epoch == &self.epoch
                    && *revision_id == self.revision_id
                    && *configuration_id == self.configuration_id
                    && qualification_ref.as_str() == self.qualification_ref.as_str()
                    && authentication_ref.as_str() == self.authentication_ref.as_str()
            }
            ApiResponse::Fail { data } => one_failure(
                data,
                &[
                    ("sponsor", "unavailable"),
                    ("slot", "retired or unavailable"),
                    ("configuration", "stale"),
                    ("controller", "stale or unavailable"),
                    ("deck", "unavailable"),
                    ("deck", "accepted selection is immutable"),
                ],
            ),
            ApiResponse::Error { .. } => false,
        };
        checked(valid, response)
    }
}
///172 recovery advances authority while retaining the old physical process generation.
pub struct RebindRunner {
    pub binding_id: Uuid,
    pub basis: Amount,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RebindAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub epoch: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub start_key: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub previous_stop_id: Uuid,
}
impl RebindRunner {
    /// Exact original input size; later lineage fields are outside this fingerprint envelope.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"bindingId":self.binding_id,"basis":self.basis,"proofRef":self.proof_ref.as_str()}),
        )
    }
    /// Decode172's exact next epoch; startKey and previousStopId are receipt identities.
    /// # Errors
    /// Rejects invalid77-digit basis, wrong epoch/target, malformed objects and undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RebindAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = ids(&[self.binding_id, self.request_id])
            && self.basis.to_string().len() <= 77
            && self
                .original_terms_octets()
                .is_some_and(|size| size <= MAX_RUNTIME_TERMS_BYTES)
            && match &response {
                ApiResponse::Success {
                    data:
                        RebindAck {
                            resource_id, epoch, ..
                        },
                } => {
                    *resource_id == self.binding_id
                        && self
                            .basis
                            .checked_add(&Amount::from(1))
                            .is_ok_and(|next| next == *epoch)
                }
                ApiResponse::Fail { data } => {
                    one_failure(data, &[("binding", "stale or unavailable")])
                }
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Trusted process absence evidence can target a historical stop after rebind.
pub struct StopObservation {
    pub binding_id: Uuid,
    pub epoch: Amount,
    pub stop_id: Uuid,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum StopDisposition {
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "historical stop confirmed")]
    HistoricalStopConfirmed,
}
impl TryFrom<String> for StopDisposition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "stopped" => Ok(Self::Stopped),
            "historical stop confirmed" => Ok(Self::HistoricalStopConfirmed),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub disposition: StopDisposition,
}
impl StopObservation {
    /// Exact original stop-observation input size, preserving historical process identity.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"bindingId":self.binding_id,"epoch":self.epoch,"stopId":self.stop_id,"proofRef":self.proof_ref.as_str()}),
        )
    }
    /// Decode172 current or historical confirmation without asserting a new process stopped.
    /// # Errors
    /// Rejects invalid original identities, malformed objects, wrong binding or undeclared failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<StopAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        let valid = ids(&[self.binding_id, self.stop_id, self.request_id])
            && self
                .original_terms_octets()
                .is_some_and(|size| size <= MAX_RUNTIME_TERMS_BYTES)
            && match &response {
                ApiResponse::Success {
                    data: StopAck { resource_id, .. },
                } => *resource_id == self.binding_id,
                ApiResponse::Fail { data } => one_failure(data, &[("source", "stale")]),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Internal stop composition; cause may be pending inside the enclosing transaction.
/// A stopped binding can reuse its existing receipt before inspecting a new cause.
pub struct RequestStop {
    pub binding_id: Uuid,
    pub cause_id: Uuid,
}
#[derive(Deserialize)]
#[serde(transparent)]
pub struct StopRequestResult(#[serde(deserialize_with = "crate::identifiers::uuid")] pub Uuid);
impl RequestStop {
    /// Decode the canonical stop receipt UUID, not a binding UUID acknowledgment object.
    /// # Errors
    /// Rejects invalid envelope and noncanonical/nil/nonscalar source results.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<StopRequestResult, SourceResponseError> {
        if !ids(&[self.binding_id, self.cause_id]) {
            return Err(SourceResponseError::context());
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}
/// Internal room-start adoption composition. Repeated standalone adoption rejects;
/// the enclosing start command recovers before invoking adoption again.
pub struct AdoptRoomAi {
    pub room_id: Uuid,
    pub match_id: Uuid,
    pub start_id: Uuid,
}
/// The SQL-internal composition has no application result payload.
/// No JSON transport or standalone adoption endpoint is declared.
pub type AdoptionResult = ();
impl AdoptRoomAi {
    /// Validate internal source identities; the enclosing start command owns execution/recovery.
    pub fn valid(&self) -> bool {
        ids(&[self.room_id, self.match_id, self.start_id])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn provider(outcome: ProviderOutcome) -> ProviderObservation {
        ProviderObservation {
            binding_id: Uuid::new_v4(),
            epoch: Amount::from(1),
            start_id: Uuid::new_v4(),
            outcome,
            provider_ref: (outcome == ProviderOutcome::Running)
                .then(|| "fixture:provider".parse().unwrap()),
            proof_ref: "fixture:proof".parse().unwrap(),
            request_id: Uuid::new_v4(),
        }
    }
    #[test]
    fn nullable_work_and_historical_rows_keep_process_and_authority_epochs_separate() {
        let binding = Uuid::new_v4();
        let start = Uuid::new_v4();
        let stop = Uuid::new_v4();
        let read = ReadStartWork {
            binding_id: binding,
            epoch: Amount::from(2),
        };
        assert!(read.decode_response(b"null").unwrap().is_none());
        let work =
            json!({"bindingId":binding,"epoch":"2","startId":start,"startKey":start,"seatId":null});
        assert!(
            read.decode_response(work.to_string().as_bytes())
                .unwrap()
                .is_some()
        );
        let mut wrong = work.clone();
        wrong["startKey"] = json!(Uuid::new_v4());
        assert!(read.decode_response(wrong.to_string().as_bytes()).is_err());
        let mut missing = work;
        missing.as_object_mut().unwrap().remove("seatId");
        assert!(
            read.decode_response(missing.to_string().as_bytes())
                .is_err()
        );
        let read = ReadStopWork {
            binding_id: binding,
            after: None,
            limit: 100,
        };
        for row in [
            json!({"bindingId":binding,"stopId":stop,"epoch":"3","startId":start,"processEpoch":"1","providerRef":"fixture:old-process"}),
            json!({"bindingId":binding,"stopId":stop,"epoch":"0","startId":null,"processEpoch":null,"providerRef":null}),
        ] {
            let decoded = read.decode_row(row.to_string().as_bytes()).unwrap();
            assert!(read.ordered_after(&decoded, None));
            assert!(!read.ordered_after(&decoded, Some(stop)));
        }
    }
    #[test]
    fn provider_states_require_exact_context_and_only_running_can_already_be_observed() {
        for (outcome, disposition) in [
            (ProviderOutcome::Running, "running"),
            (ProviderOutcome::Unavailable, "unavailable"),
            (ProviderOutcome::Unknown, "unknown"),
        ] {
            let original = provider(outcome);
            let body = json!({"status":"success","data":{"resourceId":original.binding_id,"disposition":disposition}});
            assert!(
                original
                    .decode_response(body.to_string().as_bytes())
                    .is_ok()
            );
            let mut wrong = body;
            wrong["data"]["resourceId"] = json!(Uuid::new_v4());
            assert!(
                original
                    .decode_response(wrong.to_string().as_bytes())
                    .is_err()
            );
            let observed = json!({"status":"success","data":{"resourceId":original.binding_id,"disposition":"already observed"}});
            assert_eq!(
                original
                    .decode_response(observed.to_string().as_bytes())
                    .is_ok(),
                outcome == ProviderOutcome::Running
            );
        }
        let mut invalid = provider(ProviderOutcome::Unknown);
        invalid.provider_ref = Some("fixture:unexpected".parse().unwrap());
        assert!(!invalid.valid());
    }
    #[test]
    fn grant_issue_and_delivery_are_distinct_and_secret_reference_uses_utf8_bytes() {
        let issue = IssueGrant {
            binding_id: Uuid::new_v4(),
            epoch: Amount::from(1),
            secret_ref: "fixture:secret-generation".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        let grant = Uuid::new_v4();
        let pending = json!({"status":"success","data":{"resourceId":grant,"delivery":"pending"}});
        assert!(
            issue
                .decode_response(pending.to_string().as_bytes())
                .is_ok()
        );
        let delivery = GrantDeliveryObservation {
            grant_id: grant,
            basis: Amount::from(0),
            outcome: DeliveryOutcome::Unknown,
            proof_ref: "fixture:delivery".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        assert!(
            delivery
                .decode_response(pending.to_string().as_bytes())
                .is_err()
        );
        let body = json!({"status":"success","data":{"resourceId":grant,"delivery":"unknown"}});
        assert!(
            delivery
                .decode_response(body.to_string().as_bytes())
                .is_ok()
        );
        let mut wrong = body;
        wrong["data"]["delivery"] = json!("delivered");
        assert!(
            delivery
                .decode_response(wrong.to_string().as_bytes())
                .is_err()
        );
        assert!("🂡".repeat(512).parse::<SecretReference>().is_ok());
        assert!("🂡".repeat(513).parse::<SecretReference>().is_err());
    }
    #[test]
    fn ready_rebind_and_stop_preserve_original_proofs_next_epoch_and_old_stop_identity() {
        let binding = Uuid::new_v4();
        let ready = ReadyConfirmation {
            grant_id: Uuid::new_v4(),
            epoch: Amount::from(2),
            revision_id: Uuid::new_v4(),
            qualification_ref: "fixture:deck".parse().unwrap(),
            authentication_ref: "fixture:auth".parse().unwrap(),
            configuration_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
        };
        let body = json!({"status":"success","data":{"resourceId":binding,"epoch":"2","revisionId":ready.revision_id,"qualificationRef":"fixture:deck","authenticationRef":"fixture:auth","configurationId":ready.configuration_id}});
        assert!(
            ready
                .decode_response(body.to_string().as_bytes(), binding)
                .is_ok()
        );
        for (key, value) in [
            ("configurationId", json!(Uuid::new_v4())),
            ("qualificationRef", json!("fixture:changed")),
            ("authenticationRef", json!("fixture:changed")),
            ("epoch", json!("1")),
        ] {
            let mut wrong = body.clone();
            wrong["data"][key] = value;
            assert!(
                ready
                    .decode_response(wrong.to_string().as_bytes(), binding)
                    .is_err()
            );
        }
        let rebind = RebindRunner {
            binding_id: binding,
            basis: Amount::from(2),
            proof_ref: "fixture:restart".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        let stop = Uuid::new_v4();
        let body = json!({"status":"success","data":{"resourceId":binding,"epoch":"3","startKey":Uuid::new_v4(),"previousStopId":stop}});
        assert!(rebind.decode_response(body.to_string().as_bytes()).is_ok());
        let mut wrong = body;
        wrong["data"]["epoch"] = json!("4");
        assert!(
            rebind
                .decode_response(wrong.to_string().as_bytes())
                .is_err()
        );
        let observed = StopObservation {
            binding_id: binding,
            epoch: Amount::from(2),
            stop_id: stop,
            proof_ref: "fixture:old-stop-absence".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        for disposition in ["stopped", "historical stop confirmed"] {
            assert!(observed.decode_response(json!({"status":"success","data":{"resourceId":binding,"disposition":disposition}}).to_string().as_bytes()).is_ok());
        }
    }
    #[test]
    fn original_flat_jsonb_terms_preserve_space_escape_unicode_and_exact_runtime_budget() {
        let mut original = provider(ProviderOutcome::Unknown);
        let initial = original.original_terms_octets().unwrap();
        original.proof_ref = format!(
            "{}fixture:proof",
            " ".repeat(MAX_RUNTIME_TERMS_BYTES - initial)
        )
        .parse()
        .unwrap();
        assert_eq!(
            original.original_terms_octets(),
            Some(MAX_RUNTIME_TERMS_BYTES)
        );
        assert!(original.valid());
        original.proof_ref = format!(" {}", original.proof_ref.as_str()).parse().unwrap();
        assert_eq!(
            original.original_terms_octets(),
            Some(MAX_RUNTIME_TERMS_BYTES + 1)
        );
        assert!(!original.valid());
        let forged = json!({"status":"success","data":{"resourceId":original.binding_id,"disposition":"unknown"}});
        assert!(
            original
                .decode_response(forged.to_string().as_bytes())
                .is_err()
        );
        let text = json!({"x":"\"\\\n\t🂡","nullable":null});
        assert_eq!(
            flat_jsonb_octets(&text),
            Some(serde_json::to_vec(&text).unwrap().len() + 3)
        );
        assert_eq!(flat_jsonb_octets(&json!({"nested":{}})), None);
    }
    #[test]
    fn ready_source_does_not_inherit_the_separate_runtime_terms_budget() {
        let binding = Uuid::new_v4();
        let original = ReadyConfirmation {
            grant_id: Uuid::new_v4(),
            epoch: 1.into(),
            revision_id: Uuid::new_v4(),
            qualification_ref: format!("{}fixture:deck", " ".repeat(MAX_RUNTIME_TERMS_BYTES))
                .parse()
                .unwrap(),
            authentication_ref: "fixture:auth".parse().unwrap(),
            configuration_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
        };
        let body = json!({"status":"success","data":{"resourceId":binding,"epoch":"1","revisionId":original.revision_id,"qualificationRef":original.qualification_ref.as_str(),"authenticationRef":original.authentication_ref.as_str(),"configurationId":original.configuration_id}});
        assert!(
            original
                .decode_response(body.to_string().as_bytes(), binding)
                .is_ok()
        );
    }
    #[test]
    fn maintained_paths_reject_positional_records_duplicates_and_wrong_scalar_protocols() {
        let original = provider(ProviderOutcome::Unknown);
        let positional = json!({"status":"success","data":[original.binding_id,"unknown"]});
        assert!(
            original
                .decode_response(positional.to_string().as_bytes())
                .is_err()
        );
        let duplicate = format!(
            "{{\"status\":\"success\",\"data\":{{\"resourceId\":\"{}\",\"disposition\":\"unknown\",\"disposition\":\"unknown\"}}}}",
            original.binding_id
        );
        assert!(original.decode_response(duplicate.as_bytes()).is_err());
        assert!(
            original
                .decode_response(
                    b"{\"status\":\"fail\",\"data\":{\"source\":\"stale\",\"source\":\"stale\"}}"
                )
                .is_err()
        );
        let stop = RequestStop {
            binding_id: original.binding_id,
            cause_id: original.start_id,
        };
        assert!(
            stop.decode_response(json!(Uuid::new_v4()).to_string().as_bytes())
                .is_ok()
        );
        assert!(
            stop.decode_response(json!({"resourceId":Uuid::new_v4()}).to_string().as_bytes())
                .is_err()
        );
        let unknown = Value::Null;
        assert!(
            original
                .decode_response(unknown.to_string().as_bytes())
                .is_err()
        );
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_variants::<ProviderDisposition>(&[
            ("running", "Running"),
            ("unavailable", "Unavailable"),
            ("unknown", "Unknown"),
            ("already observed", "AlreadyObserved"),
        ]);
        crate::test_http::string_enum_variants::<DeliveryPhase>(&[
            ("pending", "Pending"),
            ("unknown", "Unknown"),
            ("delivered", "Delivered"),
        ]);
        crate::test_http::string_enum_variants::<StopDisposition>(&[
            ("stopped", "Stopped"),
            ("historical stop confirmed", "HistoricalStopConfirmed"),
        ]);
    }
    #[test]
    fn outbound_observation_tokens() {
        assert_eq!(
            serde_json::to_value(ProviderOutcome::Running).unwrap(),
            serde_json::json!("running")
        );
        assert_eq!(
            serde_json::to_value(ProviderOutcome::Unavailable).unwrap(),
            serde_json::json!("unavailable")
        );
        assert_eq!(
            serde_json::to_value(ProviderOutcome::Unknown).unwrap(),
            serde_json::json!("unknown")
        );
        assert_eq!(
            serde_json::to_value(DeliveryOutcome::Unknown).unwrap(),
            serde_json::json!("unknown")
        );
        assert_eq!(
            serde_json::to_value(DeliveryOutcome::Delivered).unwrap(),
            serde_json::json!("delivered")
        );
    }
}

#[cfg(test)]
mod nested_enum_wire_tests {
    use super::*;
    #[test]
    fn provider_ack_disposition_must_be_a_string_on_the_actual_decoder() {
        let original = ProviderObservation {
            binding_id: Uuid::new_v4(),
            epoch: 1.into(),
            start_id: Uuid::new_v4(),
            outcome: ProviderOutcome::Running,
            provider_ref: Some("fixture:provider".parse().unwrap()),
            proof_ref: "fixture:source".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        let valid = json!({"status":"success","data":{"resourceId":original.binding_id,"disposition":"running"}});
        assert!(
            original
                .decode_response(valid.to_string().as_bytes())
                .is_ok()
        );
        for bad in [json!({"running":null}), json!(["running"])] {
            let mut malformed = valid.clone();
            malformed["data"]["disposition"] = bad;
            assert!(
                original
                    .decode_response(malformed.to_string().as_bytes())
                    .is_err()
            );
        }
    }
}
