//! Private original-input reconstruction records, never saved-state or Auth proofs.
//!
//! A trusted loader must reconstruct the actual immutable initial source first.
//! Records then resolve exact current native references and reuse the maintained
//! translator/engine. Neither matching metadata nor a successful proposal grants
//! durable acceptance, original-source authenticity or public disclosure.
use super::*;
use crate::player_wire::{NativeRevision, PlayerDecodeError, decode_player_record};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
mod intent;
mod locations;
mod stored_view;
use locations::Location;
pub use stored_view::{
    NativeViewError, NativeViewExpected, PreparedNativeView, RestoredNativeView, load_view,
    prepare_view, restore_view,
};
pub(super) use stored_view::{
    RetainedViewSource, load_historical_view, prepare_historical_view, verify_retained_view,
};

/// Private reconstruction record bound, independent of the public command bound.
pub(super) const MAX_RECORD_BYTES: usize = 1_048_576;
const VERSION: &str = "native-input-v1";
const RESPONSE_VERSION: &str = "native-response-timeout-input-v1";
const BANK_VERSION: &str = "native-bank-input-v1";

/// Safe reconstruction failure. Underlying details remain private typed causes.
pub enum NativeCodecError {
    /// Invalid record shape, version, binding set or reference location.
    InvalidRecord,
    /// Record/proposal does not belong to the caller-established exact source.
    WrongSource,
    /// Original private record or embedded public command exceeds its byte bound.
    Size,
    /// Private record serialization or original-byte decoding failed.
    Json(serde_json::Error),
    /// Embedded original command failed its maintained bounded decoder.
    Player(PlayerDecodeError),
    /// Reference resolution or actual native proposal failed.
    Bridge(NativeBridgeError),
}
impl fmt::Display for NativeCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRecord => "invalid native input record",
            Self::WrongSource => "native input source does not match",
            Self::Size => "native input record exceeds its boundary",
            Self::Json(_) | Self::Player(_) => "native input record cannot be decoded",
            Self::Bridge(_) => "native input record cannot be proposed",
        })
    }
}
impl fmt::Debug for NativeCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeCodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(cause) => Some(cause),
            Self::Player(cause) => Some(cause),
            Self::Bridge(cause) => Some(cause),
            _ => None,
        }
    }
}
impl From<NativeBridgeError> for NativeCodecError {
    fn from(cause: NativeBridgeError) -> Self {
        Self::Bridge(cause)
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Context {
    release: String,
    rules: String,
    catalogue: String,
    programs: String,
    profile: String,
    creature_vocabulary: String,
    name_rules: String,
}
impl Context {
    fn from_native(value: &spellwake_engine::game::GameContext) -> Self {
        Self {
            release: value.release().to_owned(),
            rules: value.rules().to_owned(),
            catalogue: value.catalogue().to_owned(),
            programs: value.programs().to_owned(),
            profile: value.profile().to_owned(),
            creature_vocabulary: value.creature_vocabulary_id().to_string(),
            name_rules: value.name_rules_id().to_string(),
        }
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Seat {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    native_seat: NativeRevision,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    #[serde(deserialize_with = "crate::provider_json::object")]
    context: Context,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    roster: Vec<Seat>,
}
impl Source {
    fn from_scope(scope: &NativeScope<'_>) -> Self {
        Self {
            basis: scope.basis().clone(),
            context: Context::from_native(&scope.source.basis().context),
            roster: scope
                .binding
                .seats
                .iter()
                .map(|seat| Seat {
                    seat_id: seat.seat_id,
                    native_seat: u64::from(seat.native_seat.0).into(),
                })
                .collect(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    handle: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::object")]
    location: Location,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Transition {
    Player {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        viewer_seat_id: Uuid,
        // Preserve the nested original byte budget, not a shorter reserialization.
        command: Box<RawValue>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        references: Vec<Reference>,
    },
    Entropy {
        words: Vec<NativeRevision>,
    },
    BankExpiry {
        seat_id: Uuid,
    },
    ResponseTimeout {
        seat_id: Uuid,
    },
}
// Internally tagged Serde deserialization buffers Content and loses RawValue's
// original byte span. Inspect the discriminator from retained bytes, then decode
// the exact closed variant directly from the same original JSON instead.
impl<'de> Deserialize<'de> for Transition {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Kind {
            kind: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Player {
            kind: String,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            viewer_seat_id: Uuid,
            command: Box<RawValue>,
            #[serde(deserialize_with = "crate::provider_json::objects")]
            references: Vec<Reference>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Entropy {
            kind: String,
            words: Vec<NativeRevision>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ClockExpiry {
            kind: String,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            seat_id: Uuid,
        }
        let raw = Box::<RawValue>::deserialize(input)?;
        let kind: Kind =
            crate::decode_wire_object(raw.get().as_bytes()).map_err(serde::de::Error::custom)?;
        match kind.kind.as_str() {
            "player" => {
                let value: Player = crate::decode_wire_object(raw.get().as_bytes())
                    .map_err(serde::de::Error::custom)?;
                if value.kind != "player" {
                    return Err(serde::de::Error::custom("invalid native transition kind"));
                }
                Ok(Self::Player {
                    viewer_seat_id: value.viewer_seat_id,
                    command: value.command,
                    references: value.references,
                })
            }
            "entropy" => {
                let value: Entropy = crate::decode_wire_object(raw.get().as_bytes())
                    .map_err(serde::de::Error::custom)?;
                if value.kind != "entropy" {
                    return Err(serde::de::Error::custom("invalid native transition kind"));
                }
                Ok(Self::Entropy { words: value.words })
            }
            "bank-expiry" | "response-timeout" => {
                let value: ClockExpiry = crate::decode_wire_object(raw.get().as_bytes())
                    .map_err(serde::de::Error::custom)?;
                if value.kind != kind.kind {
                    return Err(serde::de::Error::custom("invalid native transition kind"));
                }
                Ok(if value.kind == "bank-expiry" {
                    Self::BankExpiry {
                        seat_id: value.seat_id,
                    }
                } else {
                    Self::ResponseTimeout {
                        seat_id: value.seat_id,
                    }
                })
            }
            _ => Err(serde::de::Error::custom("invalid native transition kind")),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    source: Source,
    transition: Transition,
}
fn encode(record: &Record) -> Result<Vec<u8>, NativeCodecError> {
    let bytes = serde_json::to_vec(record).map_err(NativeCodecError::Json)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(NativeCodecError::Size);
    }
    Ok(bytes)
}
fn decode(bytes: &[u8], scope: &NativeScope<'_>) -> Result<Record, NativeCodecError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(NativeCodecError::Size);
    }
    let record: Record = crate::decode_wire_object(bytes).map_err(NativeCodecError::Json)?;
    let expected_version = match record.transition {
        Transition::BankExpiry { .. } => BANK_VERSION,
        Transition::ResponseTimeout { .. } => RESPONSE_VERSION,
        _ => VERSION,
    };
    if record.version != expected_version {
        return Err(NativeCodecError::InvalidRecord);
    }
    if record.source != Source::from_scope(scope) {
        return Err(NativeCodecError::WrongSource);
    }
    Ok(record)
}
impl NativeScope<'_> {
    /// Encode an actual proposal from this exact scope for private durable storage.
    /// This does not certify its future database acceptance or source authenticity.
    /// # Errors
    /// Rejects another scope's proposal, changed input, unresolved references and
    /// oversized records/commands. All failures preserve source and registry.
    pub fn encode_input(
        &self,
        prepared: &PreparedNativeInput,
    ) -> Result<Vec<u8>, NativeCodecError> {
        if !Arc::ptr_eq(&self.identity, &prepared.identity)
            || prepared.viewer != self.binding.viewer
            || prepared.input.basis != *self.source.basis()
            || prepared.original.basis != *self.basis()
            || prepared.input.actor != self.binding.actor
            || prepared.input.command != self.translate_action(&prepared.original.action)?
        {
            return Err(NativeCodecError::WrongSource);
        }
        let raw = serde_json::to_string(&prepared.original).map_err(NativeCodecError::Json)?;
        // Validate the same bounded original bytes later consumed by reconstruction.
        let original: PlayerCommand =
            decode_player_record(raw.as_bytes()).map_err(NativeCodecError::Player)?;
        original
            .validate()
            .map_err(|_| NativeCodecError::InvalidRecord)?;
        let references = intent::handles(&original.action)
            .into_iter()
            .map(|handle| {
                let reference = self.reference(handle)?;
                Ok(Reference {
                    handle: handle.clone(),
                    location: Location::encode(self, reference)?,
                })
            })
            .collect::<Result<_, NativeCodecError>>()?;
        encode(&Record {
            version: VERSION.into(),
            source: Source::from_scope(self),
            transition: Transition::Player {
                viewer_seat_id: prepared.viewer,
                command: RawValue::from_string(raw).map_err(NativeCodecError::Json)?,
                references,
            },
        })
    }
    /// Propose a host-only entropy addition and its exact private record together.
    /// The caller owns entropy quality, authorization and durable acceptance.
    /// # Errors
    /// Rejects over-budget/empty input and all actual native supply rejections.
    pub fn prepare_entropy(
        &self,
        words: Vec<u64>,
    ) -> Result<PreparedNativeEntropy, NativeCodecError> {
        if words.is_empty() {
            return Err(NativeCodecError::InvalidRecord);
        }
        let record = encode(&Record {
            version: VERSION.into(),
            source: Source::from_scope(self),
            transition: Transition::Entropy {
                words: words.iter().copied().map(NativeRevision::from).collect(),
            },
        })?;
        let candidate = self
            .source
            .supply_entropy(self.source.basis(), words)
            .map_err(|cause| NativeBridgeError::Native(Box::new(cause)))?;
        Ok(PreparedNativeEntropy { record, candidate })
    }
}
/// A real host-only native entropy candidate and its private reconstruction bytes.
/// No database acceptance, state install or random-source guarantee is implied.
pub struct PreparedNativeEntropy {
    record: Vec<u8>,
    candidate: NativeCandidate,
}
impl PreparedNativeEntropy {
    /// Inspect private original record bytes; never include them in diagnostics.
    pub fn record(&self) -> &[u8] {
        &self.record
    }
    /// Inspect the actual uninstalled native candidate.
    pub fn candidate(&self) -> &NativeCandidate {
        &self.candidate
    }
    /// Transfer record/candidate to a trusted durable acceptance owner.
    pub fn into_parts(self) -> (Vec<u8>, NativeCandidate) {
        (self.record, self.candidate)
    }
}
/// Reconstruct a player proposal against a caller-established exact immutable source.
/// The record's metadata cannot authenticate that source or authorize acceptance.
/// # Errors
/// Rejects wrong source/viewer/roster/version, malformed bytes, invalid binding
/// sets/locations and actual native rejections. No source or existing scope mutates.
pub fn restore_input(
    source: &GameState,
    binding: NativeBinding,
    bytes: &[u8],
) -> Result<PreparedNativeInput, NativeCodecError> {
    let mut scope = NativeScope::new(source, binding)?;
    let record = decode(bytes, &scope)?;
    let Transition::Player {
        viewer_seat_id,
        command,
        references,
    } = record.transition
    else {
        return Err(NativeCodecError::InvalidRecord);
    };
    if viewer_seat_id != scope.binding.viewer {
        return Err(NativeCodecError::WrongSource);
    }
    let original: PlayerCommand =
        decode_player_record(command.get().as_bytes()).map_err(NativeCodecError::Player)?;
    original
        .validate()
        .map_err(|_| NativeCodecError::InvalidRecord)?;
    if original.basis != *scope.basis() {
        return Err(NativeCodecError::WrongSource);
    }
    let handles = intent::handles(&original.action);
    if references.len() != handles.len() {
        return Err(NativeCodecError::InvalidRecord);
    }
    // Exact sorted handle bijection rejects aliases/extras without changing intent order.
    for (entry, expected) in references.iter().zip(handles) {
        if entry.handle != *expected {
            return Err(NativeCodecError::InvalidRecord);
        }
        let native = entry.location.resolve(&scope)?;
        scope.validate_reference(native)?;
        if scope
            .references
            .iter()
            .any(|(_, existing)| *existing == native)
        {
            return Err(NativeCodecError::InvalidRecord);
        }
        scope.references.push((entry.handle.clone(), native));
    }
    scope.propose(&original).map_err(NativeCodecError::Bridge)
}
/// Reconstruct a host entropy proposal through actual GameState::supply_entropy.
/// No caller identity, entropy quality or durable commitment is inferred.
/// # Errors
/// Rejects invalid source/record/kind, empty words and actual native rejections.
pub fn restore_entropy(
    source: &GameState,
    binding: NativeBinding,
    bytes: &[u8],
) -> Result<NativeCandidate, NativeCodecError> {
    let scope = NativeScope::new(source, binding)?;
    let record = decode(bytes, &scope)?;
    let Transition::Entropy { words } = record.transition else {
        return Err(NativeCodecError::InvalidRecord);
    };
    if words.is_empty() {
        return Err(NativeCodecError::InvalidRecord);
    }
    source
        .supply_entropy(
            source.basis(),
            words.into_iter().map(NativeRevision::get).collect(),
        )
        .map_err(|cause| NativeCodecError::Bridge(NativeBridgeError::Native(Box::new(cause))))
}

/// A deterministic clock-expiry candidate with its private reconstruction bytes.
/// This record carries no elapsed-time, clock-source or durable acceptance proof.
/// The trusted acceptance owner must independently authenticate SQL expiry.
pub struct PreparedNativeClockExpiry {
    record: Vec<u8>,
    candidate: NativeCandidate,
}
/// Provisional two-seat bank expiry, retained for existing callers.
pub type PreparedNativeBankExpiry = PreparedNativeClockExpiry;
impl PreparedNativeClockExpiry {
    /// Private original input bytes, never a public player command.
    pub fn record(&self) -> &[u8] {
        &self.record
    }
    /// Actual uninstalled native successor.
    pub fn candidate(&self) -> &NativeCandidate {
        &self.candidate
    }
    /// Transfer to the trusted acceptance owner without implying authorization.
    pub fn into_parts(self) -> (Vec<u8>, NativeCandidate) {
        (self.record, self.candidate)
    }
}
impl NativeScope<'_> {
    /// Reconstructable mechanics for a caller-authenticated bank expiry.
    /// This grants no clock authority and cannot be submitted as a player action.
    /// # Errors
    /// Rejects foreign seats, oversized records and actual native ineligibility.
    pub fn prepare_bank_expiry(
        &self,
        seat_id: Uuid,
    ) -> Result<PreparedNativeBankExpiry, NativeCodecError> {
        self.prepare_clock_expiry(seat_id, false)
    }
    /// Encode a tentative multiplayer response timeout at this exact source.
    /// The accepting database must independently establish deadline and authority.
    /// # Errors
    /// Rejects an unsupported game frame, foreign seat or non-waiting participant.
    pub fn prepare_response_timeout(
        &self,
        seat_id: Uuid,
    ) -> Result<PreparedNativeClockExpiry, NativeCodecError> {
        self.prepare_clock_expiry(seat_id, true)
    }

    fn prepare_clock_expiry(
        &self,
        seat_id: Uuid,
        response: bool,
    ) -> Result<PreparedNativeClockExpiry, NativeCodecError> {
        let player = self
            .binding
            .seats
            .iter()
            .find(|seat| seat.seat_id == seat_id)
            .ok_or(NativeCodecError::WrongSource)?
            .native_seat;
        let (version, transition, candidate) = if response {
            match clocks::waiting(self.source, &self.binding)? {
                clocks::Waiting::Decision(owner) | clocks::Waiting::Priority(owner)
                    if owner == seat_id => {}
                _ => return Err(NativeCodecError::WrongSource),
            }
            (
                RESPONSE_VERSION,
                Transition::ResponseTimeout { seat_id },
                self.source.expire_response(self.source.basis(), player),
            )
        } else {
            (
                BANK_VERSION,
                Transition::BankExpiry { seat_id },
                self.source
                    .expire_bank(spellwake_engine::game::BankExpiryInput {
                        basis: self.source.basis().clone(),
                        player,
                    }),
            )
        };
        let candidate = candidate.map_err(|cause| {
            NativeCodecError::Bridge(NativeBridgeError::Native(Box::new(cause)))
        })?;
        let record = encode(&Record {
            version: version.into(),
            source: Source::from_scope(self),
            transition,
        })?;
        Ok(PreparedNativeClockExpiry { record, candidate })
    }
}
/// Replay private bank-expiry mechanics against a caller-established exact source.
/// Neither record metadata nor replay success authenticates the original expiry.
/// # Errors
/// Rejects wrong version, kind, source, roster, seat and actual native ineligibility.
pub fn restore_bank_expiry(
    source: &GameState,
    binding: NativeBinding,
    bytes: &[u8],
) -> Result<NativeCandidate, NativeCodecError> {
    let scope = NativeScope::new(source, binding)?;
    let record = decode(bytes, &scope)?;
    let Transition::BankExpiry { seat_id } = record.transition else {
        return Err(NativeCodecError::InvalidRecord);
    };
    scope
        .prepare_bank_expiry(seat_id)
        .map(|prepared| prepared.candidate)
}

/// Reconstruct the same tentative multiplayer transition from its original input.
/// # Errors
/// Rejects foreign source, roster, kind, seat and ineligible or terminal waits.
pub fn restore_response_timeout(
    source: &GameState,
    binding: NativeBinding,
    bytes: &[u8],
) -> Result<NativeCandidate, NativeCodecError> {
    let scope = NativeScope::new(source, binding)?;
    let record = decode(bytes, &scope)?;
    let Transition::ResponseTimeout { seat_id } = record.transition else {
        return Err(NativeCodecError::InvalidRecord);
    };
    Ok(scope.prepare_response_timeout(seat_id)?.candidate)
}
