//! Original closed JSON inputs for trusted admission/host producers.
use super::*;
use crate::source_protocol::{
    original_source_octets as original_octets, original_source_terms as original,
    parse_utc_microsecond,
};
use serde_json::value::RawValue;

// Fields prefixed with an underscore still undergo their typed wire decoding;
// SQL consumes them from the retained original JSON rather than rebuilt DTOs.
fn proof_bound(proof: &SourceProof, bound: usize) -> bool {
    proof.as_str().trim_matches(' ').chars().count() <= bound
}
fn unique(values: impl Iterator<Item = Uuid>) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    values.into_iter().all(|id| !id.is_nil() && seen.insert(id))
}
fn index(value: &Amount) -> Option<usize> {
    value.to_string().parse().ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HumanSelection {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    queue_entry_id: Uuid,
    seat_index: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AiSelection {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    sponsor_entry_id: Uuid,
    seat_index: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    profile_id: Uuid,
    configuration_key: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    host_entry_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    human_seats: Vec<HumanSelection>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    ai_seats: Vec<AiSelection>,
}
/// Original concrete matcher selection, never human-provided roster authority.
pub struct QueueSelection {
    value: Selection,
    raw: Box<RawValue>,
}
impl QueueSelection {
    /// Parse the canonical selection envelope and its original262144-byte JSONB bound.
    /// # Errors
    /// Rejects malformed records, IDs, indices, duplicated seats or foreign sponsors.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original::<crate::provider_json::Object<Selection>>(bytes, 262_144)?;
        let total = value.human_seats.len() + value.ai_seats.len();
        let indices: Option<std::collections::BTreeSet<_>> = value
            .human_seats
            .iter()
            .map(|x| index(&x.seat_index))
            .chain(value.ai_seats.iter().map(|x| index(&x.seat_index)))
            .collect();
        let valid = (1..=6).contains(&value.human_seats.len())
            && value.ai_seats.len() <= 5
            && total <= 6
            && unique(value.human_seats.iter().map(|x| x.queue_entry_id))
            && value
                .human_seats
                .iter()
                .any(|x| x.queue_entry_id == value.host_entry_id)
            && value.ai_seats.iter().all(|x| {
                value
                    .human_seats
                    .iter()
                    .any(|h| h.queue_entry_id == x.sponsor_entry_id)
            })
            && indices.is_some_and(|s| s.len() == total && s.into_iter().eq(0..total));
        checked(valid, Self { value, raw })
    }
    /// Exact original JSON, for a future trusted database adapter, never diagnostics.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original JSONB text size, using the same closed-source grammar as intake.
    pub fn original_terms_octets(&self) -> Option<usize> {
        original_octets(&self.raw)
    }
    /// Qualified profile target selected by the matcher, still checked by SQL.
    pub fn profile_id(&self) -> Uuid {
        self.value.profile_id
    }
    /// Original supported configuration key, not an arbitrary configuration body.
    pub fn configuration_key(&self) -> &str {
        self.value.configuration_key.as_str()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SeatInput {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    revision_id: Uuid,
    snapshot_ref: SourceProof,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SlotInput {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    slot_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    revision_id: Uuid,
    snapshot_ref: SourceProof,
}
/// Original seat-to-deck snapshot array for113; SQL checks actual release/ownership.
pub struct AdmissionInputs {
    raw: Box<RawValue>,
}
impl AdmissionInputs {
    /// Parse a complete bounded roster envelope, not proof of native legality.
    /// # Errors
    /// Rejects malformed/duplicate records, references and oversized original terms.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (values, raw) =
            original::<Vec<crate::provider_json::Object<SeatInput>>>(bytes, 1_048_576)?;
        let valid = (1..=6).contains(&values.len())
            && unique(values.iter().map(|x| x.0.seat_id))
            && values
                .iter()
                .all(|x| !x.0.revision_id.is_nil() && proof_bound(&x.0.snapshot_ref, 2048));
        checked(valid, Self { raw })
    }
    /// Original JSON array retained without DTO reserialization.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original JSONB text size, using the same closed-source grammar as intake.
    pub fn original_terms_octets(&self) -> Option<usize> {
        original_octets(&self.raw)
    }
}
/// Original room-slot-to-deck array for185, before SQL creates new match seats.
pub struct RoomStartInputs {
    raw: Box<RawValue>,
}
impl RoomStartInputs {
    /// Parse original slot inputs while preserving their distinct source identity.
    /// # Errors
    /// Rejects malformed/duplicate records, references and oversized original terms.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (values, raw) =
            original::<Vec<crate::provider_json::Object<SlotInput>>>(bytes, 1_048_576)?;
        let valid = (1..=6).contains(&values.len())
            && unique(values.iter().map(|x| x.0.slot_id))
            && values
                .iter()
                .all(|x| !x.0.revision_id.is_nil() && proof_bound(&x.0.snapshot_ref, 2048));
        checked(valid, Self { raw })
    }
    /// Original JSON array, never the transformed child admission fingerprint.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original JSONB text size, using the same closed-source grammar as intake.
    pub fn original_terms_octets(&self) -> Option<usize> {
        original_octets(&self.raw)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Opening {
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_event_schema"
    )]
    event_schema_revision: Option<NativeEventSchema>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    snapshot_ref: SourceProof,
    context_revision: SourceProof,
    state_ref: SourceProof,
    facts_ref: SourceProof,
    event_ref: SourceProof,
    qualification_ref: SourceProof,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_initial_clock"
    )]
    clock_observation: Option<crate::admission_ports::OpeningClockObservation>,
}
/// Original proposed context and six native artifact/proof refs; no runtime codec.
pub struct OpeningArtifacts {
    raw: Box<RawValue>,
    context_id: Uuid,
    event_schema_revision: Option<NativeEventSchema>,
}
impl OpeningArtifacts {
    /// Parse237's closed opening, preserving absent historical observation and16384-byte bound.
    /// # Errors
    /// Rejects missing/extra/duplicate fields, invalid references and oversized terms.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original::<crate::provider_json::Object<Opening>>(bytes, 16_384)?;
        let valid = [
            &value.snapshot_ref,
            &value.context_revision,
            &value.state_ref,
            &value.facts_ref,
            &value.event_ref,
            &value.qualification_ref,
        ]
        .iter()
        .all(|proof| proof_bound(proof, 2048))
            && value
                .clock_observation
                .as_ref()
                .is_none_or(|clock| clock.valid_for(value.state_ref.as_str()));
        checked(
            valid,
            Self {
                raw,
                context_id: value.context_id,
                event_schema_revision: value.event_schema_revision,
            },
        )
    }
    /// Historical absence is not a semantic codec claim.
    pub fn event_schema_revision(&self) -> Option<NativeEventSchema> {
        self.event_schema_revision
    }
    /// Original proposed context identity. SQL acceptance remains independent.
    pub fn context_id(&self) -> Uuid {
        self.context_id
    }
    /// Original source JSON, never regenerated refs or proof of an opening.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original JSONB text size, using the same closed-source grammar as intake.
    pub fn original_terms_octets(&self) -> Option<usize> {
        original_octets(&self.raw)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Eligibility {
    reason: String,
    accepted_revision: Amount,
    #[serde(rename = "proofRef")]
    _proof_ref: SourceProof,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Clock {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(rename = "clockRevision")]
    _clock_revision: Amount,
    #[serde(rename = "remainingMs")]
    _remaining_ms: Amount,
    paused: bool,
    #[serde(default, deserialize_with = "present_object")]
    eligibility: Option<Eligibility>,
}
fn present_object<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    crate::provider_json::object(input).map(Some)
}
fn present_objects<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<T>>, D::Error> {
    crate::provider_json::objects(input).map(Some)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fact {
    event_index: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    #[serde(rename = "accountId")]
    _account_id: Uuid,
    kind: SourceProof,
    revision: SourceProof,
    #[serde(rename = "value")]
    _value: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Participation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(rename = "ownTurns")]
    own_turns: Amount,
    reason: SourceProof,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Terminal {
    outcome: crate::match_history::Outcome,
    #[serde(rename = "winnersRef")]
    winners_ref: SourceProof,
    reason: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    winner_seat_ids: Vec<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BankTimeout {
    kind: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    #[serde(rename = "seatId")]
    _seat_id: Uuid,
    #[serde(rename = "clockRevision")]
    _clock_revision: Amount,
    #[serde(rename = "proofRef")]
    _proof_ref: SourceProof,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Elimination {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    reason: String,
    source_revision: Amount,
    #[serde(rename = "decisionRef")]
    _decision_ref: SourceProof,
    #[serde(rename = "expiryRef")]
    _expiry_ref: SourceProof,
    expired_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_event_schema"
    )]
    event_schema_revision: Option<NativeEventSchema>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    #[serde(rename = "contextId")]
    context_id: Uuid,
    #[serde(rename = "stateRef")]
    state_ref: SourceProof,
    #[serde(rename = "factsRef")]
    facts_ref: SourceProof,
    events: Vec<SourceProof>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    facts: Vec<Fact>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    clocks: Vec<Clock>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    participation: Vec<Participation>,
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    terminal: Option<Terminal>,
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    bank_timeout: Option<BankTimeout>,
    #[serde(default, deserialize_with = "present_objects")]
    native_eliminations: Option<Vec<Elimination>>,
    #[serde(default, deserialize_with = "present_seat")]
    opening_response_seat_id: Option<Uuid>,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_turn_response"
    )]
    turn_response: Option<TurnResponse>,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_turn_protocol"
    )]
    turn_response_protocol: Option<TurnResponseProtocol>,
}
fn present_seat<'de, D: Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    crate::identifiers::uuid(input).map(Some)
}
/// Original normalized native host output, not a public player command or view.
pub struct HostCandidate {
    value: Candidate,
    raw: Box<RawValue>,
}
impl HostCandidate {
    pub(crate) fn opening_response_seat(&self) -> Option<Uuid> {
        self.value.opening_response_seat_id
    }
    pub(crate) fn turn_response_protocol(&self) -> Option<TurnResponseProtocol> {
        self.value.turn_response_protocol
    }
    pub(crate) fn turn_response(&self) -> Option<&TurnResponse> {
        self.value.turn_response.as_ref()
    }
    /// Explicit private codec claim, still requiring actual native comparison.
    pub fn event_schema_revision(&self) -> Option<NativeEventSchema> {
        self.value.event_schema_revision
    }
    /// Parse190's exact nested object, preserving original fields and byte budget.
    /// # Errors
    /// Rejects malformed, noncanonical, duplicate or over-budget normalized frames.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original::<crate::provider_json::Object<Candidate>>(bytes, 1_048_576)?;
        let valid = value.turn_response.as_ref().is_none_or(|wait| {
            value.turn_response_protocol.is_some()
                && value.opening_response_seat_id.is_none()
                && value.terminal.is_none()
                && (2..=6).contains(&wait.remaining_seat_ids.len())
                && unique(wait.remaining_seat_ids.iter().copied())
                && wait.remaining_seat_ids.contains(&wait.seat_id)
        }) && (1..=1024).contains(&value.events.len())
            && value.clocks.iter().all(|clock| {
                clock.eligibility.as_ref().is_none_or(|e| {
                    matches!(
                        e.reason.as_str(),
                        "native_priority" | "native_decision" | "native_wait"
                    )
                })
            })
            && value.facts.len() <= 4096
            && (1..=6).contains(&value.clocks.len())
            && value.participation.len() <= 6
            && unique(value.clocks.iter().map(|x| x.seat_id))
            && unique(value.participation.iter().map(|x| x.seat_id))
            && (value.terminal.is_some() || value.participation.is_empty())
            && value.facts.iter().all(|x| {
                index(&x.event_index).is_some_and(|i| i < value.events.len())
                    && proof_bound(&x.kind, 128)
                    && proof_bound(&x.revision, 128)
            })
            && value
                .participation
                .iter()
                .all(|x| proof_bound(&x.reason, 256))
            && value.terminal.as_ref().is_none_or(|x| {
                proof_bound(&x.reason, 256)
                    && x.winner_seat_ids.len() <= 6
                    && unique(x.winner_seat_ids.iter().copied())
                    && (if x.outcome == crate::match_history::Outcome::Win {
                        !x.winner_seat_ids.is_empty()
                    } else {
                        x.winner_seat_ids.is_empty()
                    })
            })
            && value
                .bank_timeout
                .as_ref()
                .is_none_or(|x| x.kind == "overall_bank")
            && value.native_eliminations.as_ref().is_none_or(|xs| {
                xs.len() <= 6
                    && unique(xs.iter().map(|x| x.seat_id))
                    && xs.iter().all(|x| {
                        x.reason == "native_response_timeout"
                            && parse_utc_microsecond(&x.expired_at).is_some()
                    })
            });
        checked(valid, Self { value, raw })
    }
    /// Compare every supplied clock with actual native ownership and stored bank.
    /// Native expiry/elimination evidence is unsupported and cannot be asserted.
    pub(crate) fn matches_native_clocks(
        &self,
        intent: &crate::native_game::NativeClockIntent,
    ) -> bool {
        self.value.bank_timeout.is_none()
            && self.value.native_eliminations.is_none()
            && self.matches_native_clock_rows(intent)
    }
    pub(crate) fn matches_native_bank_clocks(
        &self,
        intent: &crate::native_game::NativeBankExpiryIntent,
    ) -> bool {
        self.value.native_eliminations.is_none()
            && self.value.bank_timeout.as_ref().is_some_and(|value| {
                value.kind == intent.timeout.kind
                    && value._seat_id == intent.timeout.seat_id
                    && value._clock_revision == intent.timeout.clock_revision
                    && value._proof_ref.as_str() == intent.timeout.proof_ref.as_str()
            })
            && self.matches_native_clock_rows(&intent.clocks)
    }
    fn matches_native_clock_rows(&self, intent: &crate::native_game::NativeClockIntent) -> bool {
        self.value.clocks.len() == intent.rows().len()
            && self.value.clocks.iter().all(|supplied| {
                intent
                    .rows()
                    .iter()
                    .find(|expected| expected.seat_id == supplied.seat_id)
                    .is_some_and(|expected| {
                        supplied._clock_revision == expected.clock_revision
                            && supplied._remaining_ms == expected.remaining_ms
                            && supplied.paused == expected.paused
                            && match (&supplied.eligibility, &expected.eligibility) {
                                (None, None) => true,
                                (Some(left), Some(right)) => {
                                    left.reason == right.reason
                                        && left.accepted_revision == right.accepted_revision
                                        && left._proof_ref.as_str() == right.proof_ref.as_str()
                                }
                                _ => false,
                            }
                    })
            })
    }
    /// Compare actual native result evidence before the native acceptance RPC.
    /// Result-only v1 cannot authorize independently supplied quest/activity facts.
    pub(crate) fn matches_native_results(
        &self,
        terminal: Option<&crate::native_game::NativeTerminal>,
        participation: &[(Uuid, u64)],
    ) -> bool {
        self.value.facts.is_empty() && self.matches_native_result_fields(terminal, participation)
    }
    pub(crate) fn matches_native_activity_results(
        &self,
        terminal: Option<&crate::native_game::NativeTerminal>,
        participation: &[(Uuid, u64)],
        expected: &[crate::native_game::NativeCastFact],
    ) -> bool {
        self.value
            .facts
            .iter()
            .all(|row| row.event_index == Amount::from(0))
            && crate::native_game::NativeCastFact::matches_set(
                expected,
                self.value.facts.iter().map(|row| {
                    (
                        row._account_id,
                        row.kind.as_str(),
                        row.revision.as_str(),
                        &row._value,
                    )
                }),
            )
            && self.matches_native_result_fields(terminal, participation)
    }
    fn matches_native_result_fields(
        &self,
        terminal: Option<&crate::native_game::NativeTerminal>,
        participation: &[(Uuid, u64)],
    ) -> bool {
        match (terminal, &self.value.terminal) {
            (None, None) => participation.is_empty() && self.value.participation.is_empty(),
            (Some(native), Some(supplied)) => {
                supplied.outcome == native.outcome()
                    && supplied.winner_seat_ids == native.winner_seat_ids()
                    && supplied.reason.as_str() == native.reason()
                    && supplied.winners_ref.as_str() == self.value.facts_ref.as_str()
                    && self.value.participation.len() == participation.len()
                    && participation.iter().all(|(seat, count)| {
                        self.value.participation.iter().any(|row| {
                            row.seat_id == *seat
                                && row.own_turns == Amount::from(*count)
                                && row.reason.as_str() == "native_turn_starts_v1"
                        })
                    })
            }
            _ => false,
        }
    }
    /// Check supplied native frame basis. SQL separately owns roster, selected
    /// clock policy, actual elapsed debit, winners and authoritative expiry time.
    pub(crate) fn valid_for(&self, basis: &Amount) -> bool {
        self.value.clocks.iter().all(|x| {
            x.eligibility.as_ref().is_none_or(|e| {
                e.accepted_revision == *basis
                    && if x.paused {
                        e.reason == "native_wait"
                    } else {
                        matches!(e.reason.as_str(), "native_priority" | "native_decision")
                    }
            })
        }) && self
            .value
            .native_eliminations
            .as_ref()
            .is_none_or(|xs| xs.iter().all(|x| x.source_revision == *basis))
    }
    pub(crate) fn artifact_sources(
        &self,
    ) -> (Uuid, &str, &str, impl ExactSizeIterator<Item = &str>) {
        (
            self.value.context_id,
            self.value.state_ref.as_str(),
            self.value.facts_ref.as_str(),
            self.value.events.iter().map(SourceProof::as_str),
        )
    }
    /// Original JSON sent only by a trusted future host adapter.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original JSONB text size, using the same closed-source grammar as intake.
    pub fn original_terms_octets(&self) -> Option<usize> {
        original_octets(&self.raw)
    }
}
