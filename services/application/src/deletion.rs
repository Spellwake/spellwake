//! Deletion intent and bounded cleanup work. Settlement and final erasure are
//! distinct obligations; an empty cancelable workset never proves finality.
use crate::amount::Amount;
#[cfg(test)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum DeletionState {
    DeletionPending,
}
impl TryFrom<String> for DeletionState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "deletion_pending" => Ok(Self::DeletionPending),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Counts can overlap: a matched prediction is also a held commitment. There
/// is deliberately no summed total or ready-to-finalize decision here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeletionObligations {
    pub uncommitted_work: Amount,
    pub unsettled_prediction_markets: Amount,
    pub purchases_in_flight: Amount,
    pub verified_patreon_events: Amount,
    /// Associated Stripe notifications whose reconciliation is not committed.
    pub pending_stripe_events: Amount,
    pub matches_without_result: Amount,
    pub held_commitments: Amount,
    pub pending_commands: Amount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeletionStatus {
    pub state: DeletionState,
    pub requested_at: DateTime<Utc>,
    pub obligations: DeletionObligations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Account deletion intent; caller identity comes exclusively from the live session.
pub struct RequestDeletion {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    /// Lowercase SHA256 of the decoded observer key, never the raw capability.
    pub status_key_hash: String,
}
impl RequestDeletion {
    /// Validate the request identity and exact digest encoding before dispatch.
    ///
    /// # Errors
    /// Rejects nil identities and digests outside the lowercase 64-hex form.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self.status_key_hash.len() != 64
            || !self
                .status_key_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("deletion request identity required");
        }
        Ok(())
    }
}

/// A private status capability, never an account credential or a logged DTO.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObserveDeletion {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub status_key: String,
}
impl ObserveDeletion {
    /// Validate canonical 32-byte base64url input and hash the decoded bytes.
    ///
    /// # Errors
    /// Returns a static error for missing identities or noncanonical key encoding.
    pub fn key_hash(&self) -> Result<String, &'static str> {
        if self.account_id.is_nil() || self.request_id.is_nil() || self.status_key.len() != 43 {
            return Err("deletion observer unavailable");
        }
        crate::capability::key_hash(&self.status_key).map_err(|_| "deletion observer unavailable")
    }
}

/// Minimal status only. Completion additionally requires both committed sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeletionObservation {
    Pending {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "accountId")]
        account_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "requestId")]
        request_id: Uuid,
    },
    Finalizing {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "accountId")]
        account_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "requestId")]
        request_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "retirementOperationId")]
        retirement_operation_id: Uuid,
    },
    Deleted {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "accountId")]
        account_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "requestId")]
        request_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "retirementOperationId")]
        retirement_operation_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "authCleanupReceiptId")]
        auth_cleanup_receipt_id: Uuid,
    },
}
impl DeletionObservation {
    /// Reject cross-scope replies and nil source identities at the transport boundary.
    pub fn matches(&self, account: Uuid, request: Uuid) -> bool {
        let (actor, intent, sources) = match self {
            Self::Pending {
                account_id,
                request_id,
            } => (*account_id, *request_id, true),
            Self::Finalizing {
                account_id,
                request_id,
                retirement_operation_id,
            } => (*account_id, *request_id, !retirement_operation_id.is_nil()),
            Self::Deleted {
                account_id,
                request_id,
                retirement_operation_id,
                auth_cleanup_receipt_id,
            } => (
                *account_id,
                *request_id,
                !retirement_operation_id.is_nil() && !auth_cleanup_receipt_id.is_nil(),
            ),
        };
        !actor.is_nil() && !intent.is_nil() && actor == account && intent == request && sources
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum CleanupKind {
    AiSlot,
    AiStop,
    Ante,
    Invitation,
    Listing,
    Prediction,
    Queue,
    Room,
    Trade,
}
impl TryFrom<String> for CleanupKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "ai_slot" => Ok(Self::AiSlot),
            "ai_stop" => Ok(Self::AiStop),
            "ante" => Ok(Self::Ante),
            "invitation" => Ok(Self::Invitation),
            "listing" => Ok(Self::Listing),
            "prediction" => Ok(Self::Prediction),
            "queue" => Ok(Self::Queue),
            "room" => Ok(Self::Room),
            "trade" => Ok(Self::Trade),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupWork {
    pub kind: CleanupKind,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
}

/// Kind-specific canonical cancellation acknowledgment. A stop request is not
/// proof that an external AI runner has stopped; retirement tracks that separately.
pub struct CleanupAck {
    resource_id: Uuid,
    state: Option<String>,
    stop_id: Option<Uuid>,
    disposition: Option<String>,
}
// Missing optional fields remain distinct from explicitly null fields.
fn present<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}
fn present_id<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    crate::identifiers::uuid(input).map(Some)
}
impl<'de> Deserialize<'de> for CleanupAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            #[serde(default, deserialize_with = "present")]
            state: Option<String>,
            #[serde(default, deserialize_with = "present_id")]
            stop_id: Option<Uuid>,
            #[serde(default, deserialize_with = "present")]
            disposition: Option<String>,
        }
        let wire: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: wire.resource_id,
            state: wire.state,
            stop_id: wire.stop_id,
            disposition: wire.disposition,
        })
    }
}
impl CleanupAck {
    /// Bind the exact successful result shape to the original discovered work.
    pub fn matches(&self, original: &CleanupWork) -> bool {
        self.resource_id == original.id
            && !original.id.is_nil()
            && match original.kind {
                CleanupKind::Ante => {
                    self.state.as_deref() == Some("canceled")
                        && self.stop_id.is_none()
                        && self.disposition.is_none()
                }
                CleanupKind::AiStop => {
                    self.state.is_none()
                        && self.stop_id.is_some_and(|id| !id.is_nil())
                        && self.disposition.as_deref() == Some("stop requested")
                }
                _ => self.state.is_none() && self.stop_id.is_none() && self.disposition.is_none(),
            }
    }
}

#[cfg(test)]
mod cleanup_ack_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn original_kind_and_target_control_cancellation_acknowledgment() {
        let target = Uuid::new_v4();
        for (kind, data) in [
            (CleanupKind::Trade, json!({"resourceId":target})),
            (
                CleanupKind::Ante,
                json!({"resourceId":target,"state":"canceled"}),
            ),
            (
                CleanupKind::AiStop,
                json!({"resourceId":target,"stopId":Uuid::new_v4(),"disposition":"stop requested"}),
            ),
        ] {
            let original = CleanupWork { kind, id: target };
            let ack: CleanupAck =
                crate::decode_wire_object(&serde_json::to_vec(&data).unwrap()).unwrap();
            assert!(ack.matches(&original));
            assert!(!ack.matches(&CleanupWork {
                kind,
                id: Uuid::new_v4()
            }));
            for changed in [
                json!({"resourceId":target,"state":null}),
                json!({"resourceId":target,"stopId":null}),
                json!({"resourceId":target,"disposition":null}),
                json!([target]),
            ] {
                assert!(
                    crate::decode_wire_object::<CleanupAck>(&serde_json::to_vec(&changed).unwrap())
                        .is_err()
                );
            }
            let duplicate = data.to_string().replace(
                "\"resourceId\":",
                &format!("\"resourceId\":\"{target}\",\"resourceId\":"),
            );
            assert!(crate::decode_wire_object::<CleanupAck>(duplicate.as_bytes()).is_err());
        }
        let minimal: CleanupAck =
            crate::decode_wire_object(&serde_json::to_vec(&json!({"resourceId":target})).unwrap())
                .unwrap();
        for kind in [CleanupKind::Ante, CleanupKind::AiStop] {
            assert!(!minimal.matches(&CleanupWork { kind, id: target }));
        }
    }
}

/// Validate a whole bounded page before delivering any cancellation calls.
pub fn validate_cleanup_work(items: &[CleanupWork], limit: usize) -> Result<(), &'static str> {
    if !(1..=100).contains(&limit) || items.len() > limit {
        return Err("invalid deletion cleanup bounds");
    }
    let mut seen = BTreeSet::new();
    for item in items {
        if item.id.is_nil() || !seen.insert((item.kind, item.id)) {
            return Err("invalid or duplicate deletion cleanup identity");
        }
    }
    if items
        .windows(2)
        .any(|pair| (pair[0].kind, pair[0].id) >= (pair[1].kind, pair[1].id))
    {
        return Err("deletion cleanup page is not strictly ordered");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn observer_hashes_exact_bytes_and_rejects_padding_aliases_and_cross_scope_results() {
        let account = Uuid::new_v4();
        let request = Uuid::new_v4();
        let mut capability = ObserveDeletion {
            account_id: account,
            request_id: request,
            status_key: URL_SAFE_NO_PAD.encode([0_u8; 32]),
        };
        assert_eq!(
            capability.key_hash().unwrap(),
            "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
        );
        for invalid in [
            "A".repeat(42),
            "A".repeat(44),
            format!("{}B", "A".repeat(42)),
            format!("{}=", "A".repeat(43)),
        ] {
            capability.status_key = invalid;
            assert!(capability.key_hash().is_err());
        }
        let pending: DeletionObservation = serde_json::from_value(
            json!({"accountId":account,"requestId":request,"phase":"pending"}),
        )
        .unwrap();
        assert!(pending.matches(account, request));
        assert!(!pending.matches(Uuid::new_v4(), request));
        assert!(serde_json::from_value::<DeletionObservation>(json!({"accountId":account,"requestId":request,"phase":"deleted","retirementOperationId":Uuid::new_v4()})).is_err());
        assert!(serde_json::from_value::<DeletionObservation>(json!({"accountId":account,"requestId":request,"phase":"pending","username":"private"})).is_err());
    }

    #[test]
    fn deletion_status_preserves_exact_obligations_without_finality_claim() {
        let mut value = json!({"state":"deletion_pending","requestedAt":"2026-11-01T12:00:00Z","obligations":{"uncommittedWork":"0","unsettledPredictionMarkets":"1","purchasesInFlight":"1","verifiedPatreonEvents":"0","pendingStripeEvents":"0","matchesWithoutResult":"0","heldCommitments":"1","pendingCommands":"0"}});
        let status: DeletionStatus = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(status.obligations.held_commitments.to_string(), "1");
        value["readyToFinalize"] = json!(true);
        assert!(serde_json::from_value::<DeletionStatus>(value.clone()).is_err());
        value.as_object_mut().unwrap().remove("readyToFinalize");
        value["obligations"]["heldCommitments"] = json!(1);
        assert!(serde_json::from_value::<DeletionStatus>(value).is_err());
    }

    #[test]
    fn deletion_intent_cannot_claim_actor_or_settlement_and_work_is_bounded() {
        let request = json!({"requestId":Uuid::new_v4(),"statusKeyHash":"a".repeat(64)});
        serde_json::from_value::<RequestDeletion>(request.clone())
            .unwrap()
            .validate()
            .unwrap();
        for field in [
            "accountId",
            "state",
            "paymentSettled",
            "retirementOperationId",
        ] {
            let mut bad = request.clone();
            bad[field] = json!("untrusted");
            assert!(serde_json::from_value::<RequestDeletion>(bad).is_err());
        }
        assert!(
            RequestDeletion {
                request_id: Uuid::nil(),
                status_key_hash: "a".repeat(64),
            }
            .validate()
            .is_err()
        );
        let item = CleanupWork {
            kind: CleanupKind::Trade,
            id: Uuid::new_v4(),
        };
        validate_cleanup_work(std::slice::from_ref(&item), 100).unwrap();
        assert!(validate_cleanup_work(&[item.clone(), item], 100).is_err());
        assert!(validate_cleanup_work(&[], 101).is_err());
        assert!(
            serde_json::from_value::<CleanupWork>(
                json!({"kind":"filled_position","id":Uuid::new_v4()})
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<DeletionState>(&["deletion_pending"]);
        crate::test_http::string_enum_roundtrip::<CleanupKind>(&[
            "ai_slot",
            "ai_stop",
            "ante",
            "invitation",
            "listing",
            "prediction",
            "queue",
            "room",
            "trade",
        ]);
    }
}
