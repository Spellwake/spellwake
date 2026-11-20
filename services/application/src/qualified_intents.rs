//! Original human intents before qualification/selection. No handler, qualifier,
//! selector or durable preparation repository is installed by these contracts.
pub use crate::room_controls::RevisionRequest as RoomAdmissionIntent;
pub use crate::terminal_consumers::RerollQuestAck as QuestRerollAck;
use crate::{
    ApiResponse,
    admission_ports::{COUNTDOWN_FAILURES, QualifiedCountdown, QualifiedReady, READY_FAILURES},
    amount::Amount,
    commands::{CommandDisposition, RevisionAck},
    deadline_ports::{BetweenGameReply, SELECTION_FAILURES},
    source_protocol::{SourceResponseError, checked, ids, one_failure},
    terminal_consumers::REROLL_FAILURES,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;
/// Existing small human-command bound, independent of private proof-text grammar.
pub const MAX_QUALIFIED_INTENT_BYTES: usize = 32768;
#[cfg(test)]
mod tests;
/// Decode original public bytes without normalizing duplicates or positional records.
/// # Errors
/// Rejects over-budget, malformed or non-object original input. Context validation follows.
pub fn decode_intent<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, SourceResponseError> {
    if bytes.len() > MAX_QUALIFIED_INTENT_BYTES {
        return Err(SourceResponseError::context());
    }
    Ok(crate::decode_wire_object(bytes)?)
}
/// Original own decision selection, without native reply/proof or a caller principal.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BetweenGameIntent {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub decision_id: Uuid,
    pub expected_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub selected_revision_id: Uuid,
}
/// Original assignment reroll; replacement definition and UTC day are server-selected.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestRerollIntent {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub assignment_id: Uuid,
}
/// Public projection excludes qualification_ref; private empty UUID becomes explicit null.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BetweenGameAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub reserved_game_id: Option<Uuid>,
}
// SQL227 projects its canonical child UUID as the original string, including
// the empty string when this reply leaves another participant's decision open.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct StoredBetweenAck(pub(crate) BetweenGameAck);
impl<'de> Deserialize<'de> for StoredBetweenAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Stored {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            revision: crate::amount::Amount,
            #[serde(deserialize_with = "crate::identifiers::empty_uuid")]
            reserved_game_id: Option<Uuid>,
        }
        let value: Stored = crate::provider_json::object(input)?;
        Ok(Self(BetweenGameAck {
            resource_id: value.resource_id,
            revision: value.revision,
            reserved_game_id: value.reserved_game_id,
        }))
    }
}

fn advanced(value: &Amount, basis: &Amount) -> bool {
    basis
        .checked_add(&Amount::from(1))
        .is_ok_and(|n| n == *value)
}
/// A Ready/countdown acknowledgment may retain its original revision on a no-op.
pub fn valid_room_ack(original: &RoomAdmissionIntent, room: Uuid, ack: &RevisionAck) -> bool {
    ids(&[original.request_id, room])
        && ack.resource_id == room
        && (ack.revision == original.expected_revision
            || advanced(&ack.revision, &original.expected_revision))
}
/// Exact persisted Ready rejection, not a gateway/qualification exception.
pub fn ready_failure(errors: &BTreeMap<String, String>) -> bool {
    one_failure(errors, READY_FAILURES)
}
/// Exact persisted host countdown rejection.
pub fn force_start_failure(errors: &BTreeMap<String, String>) -> bool {
    one_failure(errors, COUNTDOWN_FAILURES)
}
/// Exact persisted original-pool/deadline rejection.
pub fn between_failure(errors: &BTreeMap<String, String>) -> bool {
    one_failure(errors, SELECTION_FAILURES)
        || one_failure(errors, &[("deadline", "qualified cutoff has elapsed")])
}
/// Exact persisted assignment/replacement/day rejection.
pub fn reroll_failure(errors: &BTreeMap<String, String>) -> bool {
    one_failure(errors, REROLL_FAILURES)
}
impl BetweenGameIntent {
    /// Validate nonnil original identifiers.
    pub fn valid(&self) -> bool {
        ids(&[self.request_id, self.decision_id, self.selected_revision_id])
    }
    /// Bind the public acknowledgment to the original target and revision.
    pub fn valid_ack(&self, ack: &BetweenGameAck) -> bool {
        self.valid()
            && ack.resource_id == self.decision_id
            && advanced(&ack.revision, &self.expected_revision)
            && ack.reserved_game_id.is_none_or(|id| !id.is_nil())
    }
    /// Validate persisted state and operation-specific failures against the original intent.
    pub fn valid_disposition(&self, d: &CommandDisposition<BetweenGameAck>) -> bool {
        self.valid()
            && match d {
                CommandDisposition::Accepted { ack } => self.valid_ack(ack),
                CommandDisposition::Rejected { errors } => between_failure(errors),
                _ => true,
            }
    }
}
impl QuestRerollIntent {
    /// Validate nonnil original identifiers.
    pub fn valid(&self) -> bool {
        ids(&[self.request_id, self.assignment_id])
    }
    /// Bind the public acknowledgment to the original target and revision.
    pub fn valid_ack(&self, ack: &QuestRerollAck) -> bool {
        self.valid()
            && ack.replaced_id == self.assignment_id
            && !ack.resource_id.is_nil()
            && ack.resource_id != self.assignment_id
    }
    /// Validate persisted state and operation-specific failures against the original intent.
    pub fn valid_disposition(&self, d: &CommandDisposition<QuestRerollAck>) -> bool {
        self.valid()
            && match d {
                CommandDisposition::Accepted { ack } => self.valid_ack(ack),
                CommandDisposition::Rejected { errors } => reroll_failure(errors),
                _ => true,
            }
    }
}
/// Validate against the original intent, never a current room revision.
pub fn valid_room_disposition(
    original: &RoomAdmissionIntent,
    room: Uuid,
    force: bool,
    d: &CommandDisposition<RevisionAck>,
) -> bool {
    ids(&[original.request_id, room])
        && match d {
            CommandDisposition::Accepted { ack } => valid_room_ack(original, room, ack),
            CommandDisposition::Rejected { errors } => {
                if force {
                    force_start_failure(errors)
                } else {
                    ready_failure(errors)
                }
            }
            _ => true,
        }
}
enum RoomSource {
    Ready(QualifiedReady),
    Force(QualifiedCountdown),
}
/// Own the once-prepared complete private source. No mutable access/reselection
/// method exists. A future durable adapter must retain this exact source for lookup.
/// Construction checks correlation only, not bearer authentication or native truth.
pub struct PreparedRoomAdmission {
    source: RoomSource,
}
impl PreparedRoomAdmission {
    /// Correlate a lookup with its retained original operation and verified account.
    pub fn matches_request(
        &self,
        original: &RoomAdmissionIntent,
        room: Uuid,
        verified_account: Uuid,
        force: bool,
    ) -> bool {
        let (stored, target, kind) = self.original();
        let actor = match &self.source {
            RoomSource::Ready(s) => s.actor_id,
            RoomSource::Force(s) => s.actor_id,
        };
        original.request_id == stored.request_id
            && original.expected_revision == stored.expected_revision
            && room == target
            && force == kind
            && verified_account == actor
    }
    /// # Errors
    /// Rejects a foreign actor/room/request/basis, unready or incomplete qualification.
    pub fn ready(
        original: &RoomAdmissionIntent,
        room: Uuid,
        verified_account: Uuid,
        source: QualifiedReady,
    ) -> Result<Self, SourceResponseError> {
        let valid = ids(&[original.request_id, room, verified_account])
            && source.actor_id == verified_account
            && source.room_id == room
            && source.request_id == original.request_id
            && source.basis == original.expected_revision
            && source.ready
            && source.selected_revision_id.is_some_and(|id| !id.is_nil())
            && source.qualification_ref.is_some();
        checked(
            valid,
            Self {
                source: RoomSource::Ready(source),
            },
        )
    }
    /// # Errors
    /// Rejects a foreign original host intent. Host authority still requires the adapter/SQL.
    pub fn force_start(
        original: &RoomAdmissionIntent,
        room: Uuid,
        verified_account: Uuid,
        source: QualifiedCountdown,
    ) -> Result<Self, SourceResponseError> {
        let valid = ids(&[original.request_id, room, verified_account])
            && source.actor_id == verified_account
            && source.room_id == room
            && source.request_id == original.request_id
            && source.basis == original.expected_revision;
        checked(
            valid,
            Self {
                source: RoomSource::Force(source),
            },
        )
    }
    /// Immutable original Ready source, if this is that operation.
    pub fn ready_source(&self) -> Option<&QualifiedReady> {
        match &self.source {
            RoomSource::Ready(s) => Some(s),
            _ => None,
        }
    }
    /// Immutable original force-start source, if this is that operation.
    pub fn force_start_source(&self) -> Option<&QualifiedCountdown> {
        match &self.source {
            RoomSource::Force(s) => Some(s),
            _ => None,
        }
    }
    fn original(&self) -> (RoomAdmissionIntent, Uuid, bool) {
        match &self.source {
            RoomSource::Ready(s) => (
                RoomAdmissionIntent {
                    request_id: s.request_id,
                    expected_revision: s.basis.clone(),
                },
                s.room_id,
                false,
            ),
            RoomSource::Force(s) => (
                RoomAdmissionIntent {
                    request_id: s.request_id,
                    expected_revision: s.basis.clone(),
                },
                s.room_id,
                true,
            ),
        }
    }
    /// # Errors
    /// Rejects unknown/mismatched source output. Private proof fields are never public data.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RevisionAck>, SourceResponseError> {
        let response = match &self.source {
            RoomSource::Ready(s) => s.decode_response(bytes)?,
            RoomSource::Force(s) => s.decode_response(bytes)?,
        };
        match response {
            ApiResponse::Success { data } => Ok(ApiResponse::Success {
                data: RevisionAck {
                    resource_id: data.resource_id,
                    revision: data.revision,
                },
            }),
            ApiResponse::Fail { data } => Ok(ApiResponse::Fail { data }),
            _ => Err(SourceResponseError::context()),
        }
    }
    /// # Errors
    /// Rejects malformed or substituted public dispositions; never requalifies current state.
    pub fn decode_disposition(
        &self,
        original: &RoomAdmissionIntent,
        room: Uuid,
        verified_account: Uuid,
        force: bool,
        bytes: &[u8],
    ) -> Result<CommandDisposition<RevisionAck>, SourceResponseError> {
        if !self.matches_request(original, room, verified_account, force) {
            return Err(SourceResponseError::context());
        }
        let d = decode_intent(bytes)?;
        let (original, room, force) = self.original();
        checked(valid_room_disposition(&original, room, force, &d), d)
    }
}
/// Independently resolved own-decision provenance; syntax is not authentication.
pub struct BetweenDecisionOwner {
    pub decision_id: Uuid,
    pub account_id: Uuid,
}
/// Own the original pool validation/reply proof rather than revalidating on recovery.
pub struct PreparedBetweenGame {
    source: BetweenGameReply,
    account_id: Uuid,
}
impl PreparedBetweenGame {
    /// # Errors
    /// Rejects foreign decision/owner or any source field differing from the original selection.
    pub fn new(
        original: &BetweenGameIntent,
        verified_account: Uuid,
        owner: &BetweenDecisionOwner,
        source: BetweenGameReply,
    ) -> Result<Self, SourceResponseError> {
        let valid = original.valid()
            && ids(&[verified_account])
            && owner.account_id == verified_account
            && owner.decision_id == original.decision_id
            && source.decision_id == original.decision_id
            && source.request_id == original.request_id
            && source.basis == original.expected_revision
            && source.selected_revision_id == original.selected_revision_id;
        checked(
            valid,
            Self {
                source,
                account_id: verified_account,
            },
        )
    }
    /// Immutable original selected revision, reply and qualification proof.
    pub fn source(&self) -> &BetweenGameReply {
        &self.source
    }
    fn original(&self) -> BetweenGameIntent {
        BetweenGameIntent {
            request_id: self.source.request_id,
            decision_id: self.source.decision_id,
            expected_revision: self.source.basis.clone(),
            selected_revision_id: self.source.selected_revision_id,
        }
    }
    /// Check the actual lookup's complete human input and verified original owner.
    pub fn matches_request(&self, original: &BetweenGameIntent, verified_account: Uuid) -> bool {
        verified_account == self.account_id
            && original.request_id == self.source.request_id
            && original.decision_id == self.source.decision_id
            && original.expected_revision == self.source.basis
            && original.selected_revision_id == self.source.selected_revision_id
    }
    /// # Errors
    /// Rejects private source drift, then strips its proof and maps empty reservation to null.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<BetweenGameAck>, SourceResponseError> {
        match self.source.decode_response(bytes)? {
            ApiResponse::Success { data } => Ok(ApiResponse::Success {
                data: BetweenGameAck {
                    resource_id: data.resource_id,
                    revision: data.revision,
                    reserved_game_id: data.reserved_game_id,
                },
            }),
            ApiResponse::Fail { data } => Ok(ApiResponse::Fail { data }),
            _ => Err(SourceResponseError::context()),
        }
    }
    /// # Errors
    /// Rejects public proof leakage, changed original result or unsupported stored failure.
    pub fn decode_disposition(
        &self,
        original: &BetweenGameIntent,
        verified_account: Uuid,
        bytes: &[u8],
    ) -> Result<CommandDisposition<BetweenGameAck>, SourceResponseError> {
        if !self.matches_request(original, verified_account) {
            return Err(SourceResponseError::context());
        }
        let d = decode_intent(bytes)?;
        checked(self.original().valid_disposition(&d), d)
    }
}
