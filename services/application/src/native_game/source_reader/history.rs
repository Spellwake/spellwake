//! Bounded immutable SQL transition-source discovery, not native semantic replay.
use super::*;
use crate::admission_ports::NativeEventSchema;
use serde::Deserialize;
const MAX_PAGE_BYTES: u64 = 8 * 1024 * 1024;
const PAGE_LIMIT: usize = 32;
mod bank_expiry;
mod response_expiry;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Anchor {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    anchor_checkpoint_id: Uuid,
    anchor_state_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    anchor_creator_receipt_id: Uuid,
    through_accepted_revision: NativeRevision,
    through_event_sequence: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Fact {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub(super) account_id: Uuid,
    pub(super) kind: SourceProof,
    pub(super) fact_revision: SourceProof,
    pub(super) value: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Event {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub(super) event_id: Uuid,
    pub(super) sequence: Amount,
    accepted_revision: NativeRevision,
    pub(super) private_payload_ref: NativeArtifactRef,
    accepted_at: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    creator_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub(super) facts: Vec<Fact>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Group {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub(super) checkpoint_id: Uuid,
    pub(super) accepted_revision: NativeRevision,
    pub(super) state_ref: NativeArtifactRef,
    pub(super) facts_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub(super) creator_receipt_id: Uuid,
    // SQL projects only genuine raw absence to null. Missing projection is invalid.
    #[serde(deserialize_with = "Option::deserialize")]
    pub(super) event_schema_revision: Option<NativeEventSchema>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub(super) opening_response_seat_id: Option<Uuid>,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_turn_response"
    )]
    pub(super) turn_response: Option<crate::admission_ports::TurnResponse>,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_turn_protocol"
    )]
    pub(super) turn_response_protocol: Option<crate::admission_ports::TurnResponseProtocol>,
    // Historical ordinary groups keep their exact absence; new bank inputs
    // require authenticated SQL evidence during native replay.
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    pub(super) bank_expiry: Option<bank_expiry::BankExpiryEvidence>,
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    pub(super) opening_response_expiry: Option<response_expiry::ResponseExpiryEvidence>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub(super) events: Vec<Event>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page {
    #[serde(deserialize_with = "crate::provider_json::object")]
    anchor: Anchor,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    groups: Vec<Group>,
    #[serde(deserialize_with = "Option::deserialize")]
    next_after_revision: Option<NativeRevision>,
}
// ApiResponse's internally tagged enum buffers success data. That buffer cannot
// carry the bank evidence's RawValue original. Select the status first, then
// decode the complete strict success owner directly from the original JSON.
#[derive(Deserialize)]
struct PageStatus {
    status: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageSuccess {
    #[serde(rename = "status")]
    _status: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    data: Page,
}
fn decode_page_response(bytes: &[u8]) -> Result<ApiResponse<Page>, serde_json::Error> {
    let status: PageStatus = crate::decode_wire_object(bytes)?;
    if status.status == "success" {
        let success: PageSuccess = crate::decode_wire_object(bytes)?;
        Ok(ApiResponse::Success { data: success.data })
    } else {
        // Failure fields retain their shared unique-key decoder; no RawValue
        // success data is decoded on either failure/error branch.
        let response: ObjectResponse<Page> = crate::decode_wire_object(bytes)?;
        Ok(response.0)
    }
}
/// Independently selected, complete bounded SQL source prefix.
/// Not deserializable and has no player serialization. Native execution,
/// artifact verification and participant disclosure remain separate requirements.
pub struct NativeTransitionSources {
    anchor: Anchor,
    pub(super) groups: Vec<Group>,
    pub(super) cast_accounts: crate::native_game::cast_activity::NativeCastAccounts,
}
/// Borrowed SQL fact metadata for exact comparison, not a reward or native proof.
/// No public player serialization or private artifact locator is supplied.
pub struct NativeActivityFactSource<'a> {
    pub account_id: Uuid,
    pub kind: &'a str,
    pub revision: &'a str,
    pub value: &'a Amount,
    pub accepted_revision: NativeRevision,
    pub event_sequence: &'a Amount,
}
impl NativeTransitionSources {
    /// Inspect retained facts without treating their values as native truth.
    pub fn facts(&self) -> impl Iterator<Item = NativeActivityFactSource<'_>> {
        self.groups.iter().flat_map(|group| {
            group.events.iter().flat_map(move |event| {
                event
                    .facts
                    .iter()
                    .map(move |fact| NativeActivityFactSource {
                        account_id: fact.account_id,
                        kind: fact.kind.as_str(),
                        revision: fact.fact_revision.as_str(),
                        value: &fact.value,
                        accepted_revision: group.accepted_revision,
                        event_sequence: &event.sequence,
                    })
            })
        })
    }
    /// Complete accepted checkpoint groups, including original0.
    pub fn checkpoint_count(&self) -> usize {
        self.groups.len()
    }
    /// Selected immutable native revision, not an operational host fence.
    pub fn through_revision(&self) -> NativeRevision {
        self.anchor.through_accepted_revision
    }
    /// Selected database frontier; old events may share a native revision.
    pub fn through_event_sequence(&self) -> &Amount {
        &self.anchor.through_event_sequence
    }
    /// Claims only, not native Event/metric qualification.
    pub fn all_claim_transition_codec(&self) -> bool {
        self.groups
            .iter()
            .all(|group| group.event_schema_revision.is_some())
    }
}

#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
fn budget_error(error: NativeCheckpointError) -> NativeSourceError {
    NativeSourceError(Failure::Checkpoint(error))
}
fn scoped(reference: &NativeArtifactRef, anchor: &Anchor, role: NativeArtifactRole) -> bool {
    reference.game_id() == anchor.game_id
        && reference.context_id() == anchor.context_id
        && reference.role() == role
}
fn group_work(group: &Group) -> Result<usize, NativeSourceError> {
    group.events.iter().try_fold(1usize, |total, event| {
        total
            .checked_add(1)
            .and_then(|n| n.checked_add(event.facts.len()))
            .ok_or_else(|| budget_error(NativeCheckpointError::Budget))
    })
}
impl NativeSourceRepository {
    /// Collect one complete immutable accepted prefix before any native walk.
    /// Every page/group/Event/fact and exact response byte charges the shared
    /// caller budget; native/artifact reads consume its remaining budget later.
    /// # Errors
    /// Rejects malformed/foreign/gapped/repeated/reordered sources, headers,
    /// cursors, roles, duplicate keys/IDs/facts, limits or exhausted work/transport.
    /// Only exact game-unavailable becomes None; partial prefixes are never returned.
    pub fn read_transition_sources(
        &self,
        source: &NativeGameSource,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<Option<NativeTransitionSources>, NativeSourceError> {
        let anchor = Anchor {
            game_id: source.game_id(),
            context_id: source.context_ids().0,
            anchor_checkpoint_id: source.checkpoint_ids().0,
            anchor_state_ref: source.record.checkpoint.state_ref.clone(),
            anchor_creator_receipt_id: source.checkpoint_ids().1,
            through_accepted_revision: NativeRevision::try_from(
                source.accepted_revision().to_string(),
            )
            .map_err(|_| invalid())?,
            through_event_sequence: source.last_event_sequence().clone(),
        };
        let humans: BTreeSet<_> = source
            .controllers()
            .filter_map(|(_, account, kind)| {
                if kind == ControllerKind::Human {
                    account
                } else {
                    None
                }
            })
            .collect();
        let mut groups = Vec::new();
        let mut after: Option<NativeRevision> = None;
        let mut expected_revision = 0u64;
        let mut expected_sequence = Amount::from(0);
        let mut checkpoint_ids = BTreeSet::new();
        let mut creator_ids = BTreeSet::new();
        let mut event_ids = BTreeSet::new();
        loop {
            budget.charge_work(1, 0).map_err(budget_error)?;
            let maximum = MAX_PAGE_BYTES.min(budget.remaining_bytes());
            if maximum == 0 {
                return Err(budget_error(NativeCheckpointError::Budget));
            }
            let bytes=self.backend.rpc_bytes_bounded("read_native_transition_sources",&json!({"p_game":anchor.game_id,"p_context":anchor.context_id,"p_anchor_checkpoint":anchor.anchor_checkpoint_id,"p_after_revision":after,"p_limit":PAGE_LIMIT}),maximum)
                .map_err(|error|NativeSourceError(Failure::Backend(error)))?;
            budget
                .charge_work(
                    0,
                    u64::try_from(bytes.len())
                        .map_err(|_| budget_error(NativeCheckpointError::Budget))?,
                )
                .map_err(budget_error)?;
            let response = decode_page_response(&bytes).map_err(|error| {
                NativeSourceError(Failure::Backend(BackendError::caused(
                    "native history",
                    error,
                )))
            })?;
            let page = match response {
                ApiResponse::Success { data } => data,
                ApiResponse::Fail { data }
                    if data.len() == 1 && data.get("game").is_some_and(|v| v == "unavailable") =>
                {
                    return Ok(None);
                }
                _ => return Err(invalid()),
            };
            if page.anchor != anchor || !(1..=PAGE_LIMIT).contains(&page.groups.len()) {
                return Err(invalid());
            }
            let work = page.groups.iter().try_fold(0usize, |count, group| {
                count
                    .checked_add(group_work(group)?)
                    .ok_or_else(|| budget_error(NativeCheckpointError::Budget))
            })?;
            budget.charge_work(work, 0).map_err(budget_error)?;
            for group in &page.groups {
                if group.accepted_revision.get() != expected_revision
                    || group.accepted_revision > anchor.through_accepted_revision
                    || !checkpoint_ids.insert(group.checkpoint_id)
                    || !creator_ids.insert(group.creator_receipt_id)
                    || !scoped(&group.state_ref, &anchor, NativeArtifactRole::State)
                    || group.state_ref.size() > 32_768
                    || !scoped(&group.facts_ref, &anchor, NativeArtifactRole::Facts)
                    || !(1..=1024).contains(&group.events.len())
                    || group.event_schema_revision.is_some() && group.events.len() != 1
                {
                    return Err(invalid());
                }
                let mut facts = 0usize;
                if group.accepted_revision.get() == 0
                    && (group.events.len() != 1 || !group.events[0].facts.is_empty())
                {
                    return Err(invalid());
                }
                for event in &group.events {
                    if !event_ids.insert(event.event_id)
                        || event.sequence != expected_sequence
                        || event.accepted_revision != group.accepted_revision
                        || event.creator_receipt_id != group.creator_receipt_id
                        || !scoped(
                            &event.private_payload_ref,
                            &anchor,
                            NativeArtifactRole::Event,
                        )
                        || parse_utc_microsecond(&event.accepted_at).is_none()
                    {
                        return Err(invalid());
                    }
                    expected_sequence = expected_sequence
                        .checked_add(&Amount::from(1))
                        .map_err(|_| invalid())?;
                    facts = facts.checked_add(event.facts.len()).ok_or_else(invalid)?;
                    if facts > 4096 {
                        return Err(invalid());
                    }
                    let mut keys = BTreeSet::new();
                    for fact in &event.facts {
                        if !humans.contains(&fact.account_id)
                            || fact.kind.as_str().trim_matches(' ').chars().count() > 128
                            || fact
                                .fact_revision
                                .as_str()
                                .trim_matches(' ')
                                .chars()
                                .count()
                                > 128
                            || !keys.insert((
                                fact.account_id,
                                fact.kind.as_str(),
                                fact.fact_revision.as_str(),
                            ))
                        {
                            return Err(invalid());
                        }
                    }
                }
                expected_revision = expected_revision.checked_add(1).ok_or_else(invalid)?;
            }
            let last = page.groups.last().ok_or_else(invalid)?;
            let complete = last.accepted_revision == anchor.through_accepted_revision;
            if complete {
                if page.next_after_revision.is_some()
                    || last.checkpoint_id != anchor.anchor_checkpoint_id
                    || last.creator_receipt_id != anchor.anchor_creator_receipt_id
                    || last.state_ref != anchor.anchor_state_ref
                    || expected_sequence
                        != anchor
                            .through_event_sequence
                            .checked_add(&Amount::from(1))
                            .map_err(|_| invalid())?
                {
                    return Err(invalid());
                }
            } else if page.next_after_revision != Some(last.accepted_revision)
                || after.is_some_and(|prior| prior >= last.accepted_revision)
            {
                return Err(invalid());
            }
            after = page.next_after_revision;
            groups.extend(page.groups);
            if complete {
                return Ok(Some(NativeTransitionSources {
                    anchor,
                    groups,
                    cast_accounts: source.cast_accounts().map_err(budget_error)?,
                }));
            }
        }
    }
}
