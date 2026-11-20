//! Private terminal and progression contracts, including the bounded daily quest worker.
mod quest_worker;
use crate::{
    ApiResponse, amount::Amount, ante::AnteState, commands::CommandAck,
    source_protocol::valid_digest,
};
pub use quest_worker::QuestWorker;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::source_protocol::{SourceFailure, SourceResponseError, classify_sqlstate};
use crate::source_protocol::{accepted_response, one_failure, record_response};

/// Trusted terminal source; allocation and randomness are derived by SQL143.
pub struct AnteSettlement {
    pub commitment_id: Uuid,
    pub result_id: Uuid,
}
impl AnteSettlement {
    /// The envelope is not authority to choose a terminal result.
    pub fn valid(&self) -> bool {
        !self.commitment_id.is_nil() && !self.result_id.is_nil()
    }
}
fn prefixed_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_digest)
}
/// Source143 ack, with no private allocation or seed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub state: AnteState,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub operation_id: Uuid,
    pub allocation_ref: String,
}
impl SettlementAck {
    /// Bind the original commitment and actual terminal kind; other known AnteStates are invalid here.
    pub fn valid_for(&self, original: &AnteSettlement, terminal_kind: AnteState) -> bool {
        original.valid()
            && self.resource_id == original.commitment_id
            && matches!(terminal_kind, AnteState::Settled | AnteState::Refunded)
            && self.state == terminal_kind
            && prefixed_digest(&self.allocation_ref)
    }
}
/// Typed source ID for109's nullable UUID scalar result. NULL means nonqualifying
/// participation; a present UUID identifies the account/month row, not participation.
pub struct MonthlyActivity {
    pub participation_id: Uuid,
}
impl MonthlyActivity {
    /// Canonical source lookup supplies account, own turns and completion month.
    pub fn valid(&self) -> bool {
        !self.participation_id.is_nil()
    }
}
/// A nullable scalar decoder matching109, including canonical UUID spelling.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct MonthlyActivityResult(
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")] pub Option<Uuid>,
);
/// One advancing133 cursor invocation, not a replayable command UUID.
pub struct QuestActivityBatch {
    pub game_id: Uuid,
    pub limit: u16,
}
impl QuestActivityBatch {
    /// Limit is1..1000 accepted event rows; one event can complete many assignments.
    pub fn valid(&self) -> bool {
        !self.game_id.is_nil() && (1..=1000).contains(&self.limit)
    }
}
/// The cursor ID is not the input game ID, and reward count may exceed event count.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestActivityAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub events: Amount,
    pub rewards: Amount,
}
impl QuestActivityAck {
    /// Bound consumed events to the original batch without inventing a reward-count cap.
    pub fn valid_for(&self, original: &QuestActivityBatch) -> bool {
        original.valid() && self.events <= Amount::from(u64::from(original.limit))
    }
    /// Bind to the canonical quests-v1/game cursor when its identity is known.
    pub fn matches_cursor(&self, cursor: Uuid) -> bool {
        !cursor.is_nil() && self.resource_id == cursor
    }
}
/// Trusted125 terminal source. SQL owns format sequence, source season and rewards.
pub struct RankedResult {
    pub result_id: Uuid,
}
/// Source-compatible125 outcome; raw normalized sources have no six-player upper bound.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankedResultAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub season_id: Uuid,
    pub sequence: Amount,
    pub players: Amount,
}
impl RankedResultAck {
    /// Validate original source and SQL's>=2 participant language.
    /// A qualified runtime separately binds season/sequence/count to its admitted roster.
    pub fn valid_for(&self, original: &RankedResult) -> bool {
        !original.result_id.is_nil()
            && self.resource_id == original.result_id
            && !self.sequence.is_zero()
            && self.players >= Amount::from(2)
    }
    /// Bind a known accepted result's source season, sequence and qualified roster.
    pub fn matches_context(&self, season: Uuid, sequence: &Amount, players: &Amount) -> bool {
        !season.is_nil()
            && self.season_id == season
            && &self.sequence == sequence
            && &self.players == players
    }
}
/// Trusted173 season finalization. Drain/frontier scheduling is a separate dependency.
pub struct FinalizeRankSeason {
    pub season_id: Uuid,
}
/// Finalized cutoff reference and number of rank rows, without reward source internals.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankSeasonAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub cutoff_ref: String,
    pub ranks: Amount,
}
impl RankSeasonAck {
    /// Bind the source season; cutoff hashing remains canonical Postgres JSONB.
    pub fn valid_for(&self, original: &FinalizeRankSeason) -> bool {
        !original.season_id.is_nil()
            && self.resource_id == original.season_id
            && prefixed_digest(&self.cutoff_ref)
    }
}
/// Frozen trusted selector result plus original request. No initial distribution
/// policy is selected here, and browser input cannot supply this authority.
pub struct DailyQuestSelection {
    pub actor_id: Uuid,
    pub definition_id: Uuid,
    pub day: String,
    pub request_id: Uuid,
}
impl DailyQuestSelection {
    /// Validate the original envelope, allowing historical day replay.
    pub fn valid(&self) -> bool {
        !self.actor_id.is_nil()
            && !self.definition_id.is_nil()
            && !self.request_id.is_nil()
            && crate::progression::valid_day(&self.day)
    }
    /// SQL195 may return the day's existing assignment instead of the requested definition.
    pub fn valid_generate_response(&self, response: &ApiResponse<CommandAck>) -> bool {
        self.valid()
            && match response {
                ApiResponse::Success { data } => !data.resource_id.is_nil(),
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[
                        ("account", "unavailable"),
                        ("day", "not current UTC day"),
                        ("definition", "unavailable"),
                        ("bank", "full"),
                    ],
                ),
                ApiResponse::Error { .. } => false,
            }
    }
}
/// Original human reroll intent plus the once-selected trusted replacement.
pub struct RerollQuest {
    pub assignment_id: Uuid,
    pub selection: DailyQuestSelection,
}
/// Replacement assignment, not an acknowledgment targeting the original assignment.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerollQuestAck {
    pub resource_id: Uuid,
    pub replaced_id: Uuid,
}
impl<'de> Deserialize<'de> for RerollQuestAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            replaced_id: Uuid,
        }
        let wire: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: wire.resource_id,
            replaced_id: wire.replaced_id,
        })
    }
}
impl RerollQuest {
    /// Source-current day/allowance checks remain SQL-owned after original replay.
    pub fn valid(&self) -> bool {
        !self.assignment_id.is_nil() && self.selection.valid()
    }
    /// Preserve the exact195 accepted/rejected vocabulary and original replaced identity.
    pub fn valid_response(&self, response: &ApiResponse<RerollQuestAck>) -> bool {
        self.valid()
            && match response {
                ApiResponse::Success { data } => {
                    data.replaced_id == self.assignment_id
                        && !data.resource_id.is_nil()
                        && data.resource_id != self.assignment_id
                }
                ApiResponse::Fail { data } => one_failure(data, REROLL_FAILURES),
                ApiResponse::Error { .. } => false,
            }
    }
}
pub(crate) const REROLL_FAILURES: &[(&str, &str)] = &[
    ("account", "unavailable"),
    ("day", "not current UTC day"),
    ("assignment", "unavailable"),
    ("definition", "different available definition required"),
    ("reroll", "already used"),
];
/// Independently loaded accepted-result context, not caller-selected ranking policy.
pub struct RankedContext {
    pub season_id: Uuid,
    pub sequence: Amount,
    pub players: Amount,
}
impl AnteSettlement {
    /// Decode the actual143 response, requiring its original commitment and terminal class.
    /// # Errors
    /// Rejects invalid envelopes, non-success results, malformed objects and wrong source context.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        kind: AnteState,
    ) -> Result<SettlementAck, SourceResponseError> {
        let ack: SettlementAck = accepted_response(bytes)?;
        if ack.valid_for(self, kind) {
            Ok(ack)
        } else {
            Err(SourceResponseError::context())
        }
    }
}
impl MonthlyActivity {
    /// Decode109's scalar/null protocol without inventing an object envelope.
    /// # Errors
    /// Rejects invalid source identity and noncanonical/non-scalar results.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<MonthlyActivityResult, SourceResponseError> {
        if !self.valid() {
            return Err(SourceResponseError::context());
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}
impl QuestActivityBatch {
    /// Decode one advancing batch and optionally bind a known quests-v1/game cursor.
    /// # Errors
    /// Rejects non-success, malformed objects, invalid limits or a mismatched known cursor.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        cursor: Option<Uuid>,
    ) -> Result<QuestActivityAck, SourceResponseError> {
        let ack: QuestActivityAck = accepted_response(bytes)?;
        if ack.valid_for(self) && cursor.is_none_or(|id| ack.matches_cursor(id)) {
            Ok(ack)
        } else {
            Err(SourceResponseError::context())
        }
    }
}
impl RankedResult {
    /// Decode125 and bind accepted season/sequence/roster context when supplied.
    /// # Errors
    /// Rejects non-success, invalid source identity, malformed objects or mismatched context.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        context: Option<&RankedContext>,
    ) -> Result<RankedResultAck, SourceResponseError> {
        let ack: RankedResultAck = accepted_response(bytes)?;
        if ack.valid_for(self)
            && context.is_none_or(|c| ack.matches_context(c.season_id, &c.sequence, &c.players))
        {
            Ok(ack)
        } else {
            Err(SourceResponseError::context())
        }
    }
}
impl FinalizeRankSeason {
    /// Decode173's finalized cutoff, including zero-rank and retired-reward outcomes.
    /// # Errors
    /// Rejects non-success, malformed objects, wrong season or malformed cutoff digest.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<RankSeasonAck, SourceResponseError> {
        let ack: RankSeasonAck = accepted_response(bytes)?;
        if ack.valid_for(self) {
            Ok(ack)
        } else {
            Err(SourceResponseError::context())
        }
    }
}
impl DailyQuestSelection {
    /// Decode195's exact generate result without reselecting definition or source day.
    /// # Errors
    /// Rejects malformed objects, invalid original input or undeclared response/failure shapes.
    pub fn decode_generate_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<CommandAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        if self.valid_generate_response(&response) {
            Ok(response)
        } else {
            Err(SourceResponseError::context())
        }
    }
}
impl RerollQuest {
    /// Decode195's exact replacement or declared rejection under the original selection.
    /// # Errors
    /// Rejects malformed objects, invalid original input, wrong replacement context or unknown failure.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RerollQuestAck>, SourceResponseError> {
        let response = record_response(bytes)?;
        if self.valid_response(&response) {
            Ok(response)
        } else {
            Err(SourceResponseError::context())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_wire_object, provider_json::ObjectResponse};
    use serde_json::json;
    use std::collections::BTreeMap;
    fn selection() -> DailyQuestSelection {
        DailyQuestSelection {
            actor_id: Uuid::new_v4(),
            definition_id: Uuid::new_v4(),
            day: "2020-01-01".into(),
            request_id: Uuid::new_v4(),
        }
    }
    #[test]
    fn settlement_accepts_only_actual_terminal_variants_and_original_commitment() {
        let original = AnteSettlement {
            commitment_id: Uuid::new_v4(),
            result_id: Uuid::new_v4(),
        };
        for state in ["settled", "refunded", "offered", "committed", "canceled"] {
            let value = json!({"resourceId":original.commitment_id,"state":state,"operationId":Uuid::new_v4(),"allocationRef":format!("sha256:{}","a".repeat(64))});
            let ack: SettlementAck = decode_wire_object(value.to_string().as_bytes()).unwrap();
            assert_eq!(
                ack.valid_for(&original, ack.state),
                matches!(state, "settled" | "refunded")
            );
        }
    }
    #[test]
    fn scalar_and_advancing_batch_semantics_preserve_source_language() {
        assert!(
            serde_json::from_str::<MonthlyActivityResult>("null")
                .unwrap()
                .0
                .is_none()
        );
        let id = Uuid::new_v4();
        assert_eq!(
            serde_json::from_value::<MonthlyActivityResult>(json!(id))
                .unwrap()
                .0,
            Some(id)
        );
        assert!(serde_json::from_value::<MonthlyActivityResult>(json!(Uuid::nil())).is_err());
        assert!(serde_json::from_value::<MonthlyActivityResult>(json!({"resourceId":id})).is_err());
        let request = QuestActivityBatch {
            game_id: id,
            limit: 1,
        };
        let ack: QuestActivityAck = decode_wire_object(
            json!({"resourceId":Uuid::new_v4(),"events":"1","rewards":"6"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert!(ack.valid_for(&request));
        assert!(
            !QuestActivityAck {
                events: Amount::from(2),
                ..ack
            }
            .valid_for(&request)
        );
        let ranked = RankedResult { result_id: id };
        let ack: RankedResultAck = decode_wire_object(
            json!({"resourceId":id,"seasonId":Uuid::new_v4(),"sequence":"1","players":"7"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert!(ack.valid_for(&ranked));
    }
    #[test]
    fn selector_envelopes_retain_historical_day_and_exact_failure_vocabulary() {
        let original = selection();
        assert!(original.valid());
        assert!(original.valid_generate_response(&ApiResponse::Success {
            data: CommandAck {
                resource_id: Uuid::new_v4()
            }
        }));
        let reroll = RerollQuest {
            assignment_id: Uuid::new_v4(),
            selection: original,
        };
        let response = ApiResponse::Success {
            data: RerollQuestAck {
                resource_id: Uuid::new_v4(),
                replaced_id: reroll.assignment_id,
            },
        };
        assert!(reroll.valid_response(&response));
        assert!(!reroll.valid_response(&ApiResponse::Success {
            data: RerollQuestAck {
                resource_id: reroll.assignment_id,
                replaced_id: reroll.assignment_id
            }
        }));
        for (key, value) in [
            ("reroll", "already used"),
            ("assignment", "unavailable"),
            ("definition", "different available definition required"),
        ] {
            assert!(reroll.valid_response(&ApiResponse::Fail {
                data: BTreeMap::from([(key.into(), value.into())])
            }));
        }
        assert!(!reroll.valid_response(&ApiResponse::Fail {
            data: BTreeMap::from([("request".into(), "conflict".into())])
        }));
    }
    #[test]
    fn private_envelopes_reject_positional_objects_and_duplicate_failure_fields() {
        let id = Uuid::new_v4();
        for value in [
            json!({"status":"success","data":[id,id]}),
            json!({"status":"success","data":{"resourceId":id,"replacedId":id,"private":"proof"}}),
        ] {
            assert!(
                decode_wire_object::<ObjectResponse<RerollQuestAck>>(value.to_string().as_bytes())
                    .is_err()
            );
        }
        let duplicate = b"{\"status\":\"fail\",\"data\":{\"bank\":\"full\",\"bank\":\"full\"}}";
        assert!(decode_wire_object::<ObjectResponse<CommandAck>>(duplicate).is_err());
    }
    #[test]
    fn maintained_decoders_reject_wrong_context_non_success_and_source_specific_boundaries() {
        let id = Uuid::new_v4();
        let settlement = AnteSettlement {
            commitment_id: id,
            result_id: Uuid::new_v4(),
        };
        let response = json!({"status":"success","data":{"resourceId":id,"state":"settled","operationId":Uuid::new_v4(),"allocationRef":format!("sha256:{}","a".repeat(64))}});
        assert!(
            settlement
                .decode_response(response.to_string().as_bytes(), AnteState::Settled)
                .is_ok()
        );
        assert!(
            settlement
                .decode_response(response.to_string().as_bytes(), AnteState::Refunded)
                .is_err()
        );
        for (key, value) in [
            ("resourceId", json!(Uuid::new_v4())),
            ("allocationRef", json!("sha256:wrong")),
            ("state", json!("invented")),
        ] {
            let mut wrong = response.clone();
            wrong["data"][key] = value;
            assert!(
                settlement
                    .decode_response(wrong.to_string().as_bytes(), AnteState::Settled)
                    .is_err()
            );
        }
        let cursor = Uuid::new_v4();
        let batch = QuestActivityBatch {
            game_id: id,
            limit: 1000,
        };
        let response = json!({"status":"success","data":{"resourceId":cursor,"events":"1000","rewards":"2000"}});
        assert!(
            batch
                .decode_response(response.to_string().as_bytes(), Some(cursor))
                .is_ok()
        );
        assert!(
            batch
                .decode_response(response.to_string().as_bytes(), Some(Uuid::new_v4()))
                .is_err()
        );
        for limit in [0, 1001] {
            assert!(
                QuestActivityBatch { game_id: id, limit }
                    .decode_response(response.to_string().as_bytes(), None)
                    .is_err()
            );
        }
        let source = RankedResult { result_id: id };
        let context = RankedContext {
            season_id: Uuid::new_v4(),
            sequence: Amount::from(3),
            players: Amount::from(7),
        };
        let response = json!({"status":"success","data":{"resourceId":id,"seasonId":context.season_id,"sequence":"3","players":"7"}});
        assert!(
            source
                .decode_response(response.to_string().as_bytes(), Some(&context))
                .is_ok()
        );
        for (key, value) in [
            ("resourceId", json!(Uuid::new_v4())),
            ("seasonId", json!(Uuid::new_v4())),
            ("sequence", json!("4")),
            ("players", json!("8")),
            ("sequence", json!("0")),
            ("players", json!("1")),
        ] {
            let mut wrong = response.clone();
            wrong["data"][key] = value;
            assert!(
                source
                    .decode_response(wrong.to_string().as_bytes(), Some(&context))
                    .is_err()
            );
        }
        let reroll = RerollQuest {
            assignment_id: id,
            selection: selection(),
        };
        let positional = json!({"status":"success","data":[Uuid::new_v4(),id]});
        assert!(
            decode_wire_object::<ApiResponse<RerollQuestAck>>(positional.to_string().as_bytes())
                .is_err()
        );
        assert!(
            reroll
                .decode_response(positional.to_string().as_bytes())
                .is_err()
        );
        let invented = b"{\"status\":\"fail\",\"data\":{\"source\":\"unavailable\"}}";
        assert!(batch.decode_response(invented, None).is_err());
        assert!(source.decode_response(invented, None).is_err());
        assert!(
            settlement
                .decode_response(invented, AnteState::Settled)
                .is_err()
        );
        assert!(
            FinalizeRankSeason { season_id: id }
                .decode_response(invented)
                .is_err()
        );
        assert!(reroll.decode_response(invented).is_err());
        let duplicate=b"{\"status\":\"fail\",\"data\":{\"reroll\":\"already used\",\"reroll\":\"already used\"}}";
        assert!(reroll.decode_response(duplicate).is_err());
        assert!(
            !DailyQuestSelection {
                actor_id: Uuid::nil(),
                ..selection()
            }
            .valid()
        );
        assert_eq!(classify_sqlstate("40001"), SourceFailure::Retry);
        assert_eq!(classify_sqlstate("23503"), SourceFailure::MissingSource);
        assert_eq!(classify_sqlstate("22023"), SourceFailure::InvalidEnvelope);
        assert_eq!(classify_sqlstate("23514"), SourceFailure::InvalidSource);
        assert_eq!(classify_sqlstate("XX000"), SourceFailure::Unavailable);
    }
}
