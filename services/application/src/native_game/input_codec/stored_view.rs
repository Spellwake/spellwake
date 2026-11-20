//! Private View bytes carry the public snapshot and exact typed handle registry.
use super::{Location, NativeCodecError, Reference, Source};
use crate::native_game::projection::HistoricalProjectionPermit;
use crate::{
    amount::Amount,
    native_game::*,
    player_wire::{MAX_PLAYER_WIRE_BYTES, PlayerBasis, PlayerSnapshot},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, error::Error, fmt};
use uuid::Uuid;
const VERSION: &str = "native-view-artifact-v1";
// Every issued handle appears in the bounded public snapshot, at least39bytes.
const MAX_REFERENCES: usize = MAX_PLAYER_WIRE_BYTES / 39;

/// Safe private View failure. Diagnostic causes never become public messages.
pub enum NativeViewError {
    Invalid,
    Size,
    Json(serde_json::Error),
    Codec(NativeCodecError),
    Projection(NativeProjectionError),
    Artifact(NativeArtifactError),
}
impl fmt::Display for NativeViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("native View cannot be prepared or restored")
    }
}
impl fmt::Debug for NativeViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeViewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Codec(e) => Some(e),
            Self::Projection(e) => Some(e),
            Self::Artifact(e) => Some(e),
            Self::Invalid | Self::Size => None,
        }
    }
}
impl From<NativeCodecError> for NativeViewError {
    fn from(e: NativeCodecError) -> Self {
        Self::Codec(e)
    }
}
impl From<NativeBridgeError> for NativeViewError {
    fn from(e: NativeBridgeError) -> Self {
        Self::Codec(NativeCodecError::Bridge(e))
    }
}
impl From<NativeProjectionError> for NativeViewError {
    fn from(e: NativeProjectionError) -> Self {
        Self::Projection(e)
    }
}
impl From<NativeArtifactError> for NativeViewError {
    fn from(e: NativeArtifactError) -> Self {
        Self::Artifact(e)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Disclosure {
    Live,
    Completed,
}
impl From<NativeDisclosure> for Disclosure {
    fn from(value: NativeDisclosure) -> Self {
        match value {
            NativeDisclosure::LiveParticipant => Self::Live,
            NativeDisclosure::CompletedParticipant => Self::Completed,
        }
    }
}
impl From<Disclosure> for NativeDisclosure {
    fn from(value: Disclosure) -> Self {
        match value {
            Disclosure::Live => Self::LiveParticipant,
            Disclosure::Completed => Self::CompletedParticipant,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    source: Source,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    viewer_seat_id: Uuid,
    disclosure: Disclosure,
    #[serde(deserialize_with = "Option::deserialize")]
    deadline_at: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    references: Vec<Reference>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    snapshot: PlayerSnapshot,
}
/// Independently issued View ref and semantic cut. Construction checks consistency;
/// the caller must obtain the ref/cut from accepted SQL issuance.
pub struct NativeViewExpected {
    reference: NativeArtifactRef,
    basis: PlayerBasis,
    viewer: Uuid,
    disclosure: Disclosure,
    through: Amount,
}
impl NativeViewExpected {
    /// Bind an accepted artifact to its viewer/disclosure/native cut.
    /// # Errors
    /// Rejects nil IDs, wrong role/scope and an impossible event frontier.
    pub fn new(
        reference: NativeArtifactRef,
        basis: PlayerBasis,
        viewer: Uuid,
        disclosure: NativeDisclosure,
        through: Amount,
    ) -> Result<Self, NativeViewError> {
        if viewer.is_nil()
            || basis.game_id.is_nil()
            || basis.context_id.is_nil()
            || reference.role() != NativeArtifactRole::View
            || reference.game_id() != basis.game_id
            || reference.context_id() != basis.context_id
            || through < Amount::from(basis.revision.get())
        {
            return Err(NativeViewError::Invalid);
        }
        Ok(Self {
            reference,
            basis,
            viewer,
            disclosure: disclosure.into(),
            through,
        })
    }
    /// Exact private ref, never a public or signed URL.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
}
/// Independently reconstructed original context and ordered native seat mapping.
/// These fields must come from the accepted source and original program descriptor,
/// never from the retained View or replay snapshot being compared.
pub(in crate::native_game) struct RetainedViewSource<'a> {
    pub release: Uuid,
    pub profile: Uuid,
    pub catalogue: &'a str,
    pub programs: &'a str,
    pub rules: &'a str,
    pub vocabulary_id: spellwake_contracts::creature_types::CreatureVocabularyId,
    pub name_rules: &'a str,
    pub roster: &'a [(Uuid, u64)],
}

/// Compare an exact SQL-winning View with independently accepted original inputs.
/// This authenticates a retained envelope/snapshot; it does not execute current
/// rules, qualify a fresh native projection or expose the private handle registry.
/// # Errors
/// Rejects a different artifact, malformed record, original source/cut mismatch,
/// changed snapshot or the existing reference/snapshot engineering bounds.
pub(in crate::native_game) fn verify_retained_view(
    artifact: &NativeArtifact,
    expected: &NativeViewExpected,
    source: &RetainedViewSource<'_>,
    snapshot: &PlayerSnapshot,
) -> Result<(), NativeViewError> {
    if artifact.reference() != expected.reference() {
        return Err(NativeViewError::Invalid);
    }
    let record: Record =
        crate::decode_wire_object(artifact.bytes()).map_err(NativeViewError::Json)?;
    verify_retained_record(&record, expected, source, snapshot)
}

fn verify_retained_record(
    record: &Record,
    expected: &NativeViewExpected,
    source: &RetainedViewSource<'_>,
    snapshot: &PlayerSnapshot,
) -> Result<(), NativeViewError> {
    let context = &record.source.context;
    if record.version != VERSION
        || record.source.basis != expected.basis
        || context.release != source.release.to_string()
        || context.profile != source.profile.to_string()
        || context.catalogue != source.catalogue
        || context.programs != source.programs
        || context.rules != source.rules
        || context.creature_vocabulary != source.vocabulary_id.to_string()
        || context.name_rules != source.name_rules
        || record
            .source
            .roster
            .iter()
            .map(|seat| (seat.seat_id, seat.native_seat.get()))
            .ne(source.roster.iter().copied())
        || record.viewer_seat_id != expected.viewer
        || record.disclosure != expected.disclosure
        || record.snapshot.basis != expected.basis
        || record.snapshot.viewer_seat_id != expected.viewer
        || record.snapshot.through_event_sequence != expected.through
        || record.snapshot != *snapshot
    {
        return Err(NativeViewError::Invalid);
    }
    if record.references.len() > MAX_REFERENCES
        || serde_json::to_vec(&record.snapshot)
            .map_err(NativeViewError::Json)?
            .len()
            > MAX_PLAYER_WIRE_BYTES
    {
        return Err(NativeViewError::Size);
    }
    Ok(())
}

/// Unissued proposal. Persist then publish through SQL and use its first winner;
/// a local projection does not establish which registry was issued to a player.
pub struct PreparedNativeView {
    bytes: Vec<u8>,
    reference: NativeArtifactRef,
    snapshot: PlayerSnapshot,
}
impl PreparedNativeView {
    /// Private bytes include native locators and must never be a client payload.
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content reference of this proposal, not acceptance or visibility authority.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
    /// Seat-safe snapshot; do not deliver before SQL selects the issued artifact.
    pub fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }
    /// Retain using authenticated exact-byte readback and no upsert.
    /// # Errors
    /// Returns private Store errors; this never publishes a SQL View receipt.
    pub fn retain(
        &self,
        store: &NativeArtifactStore,
    ) -> Result<NativeArtifactRef, NativeViewError> {
        Ok(store.retain(
            self.reference.game_id(),
            self.reference.context_id(),
            NativeArtifactRole::View,
            &self.bytes,
        )?)
    }
}
/// Restored registry and public snapshot from an independently issued artifact.
pub struct RestoredNativeView<'a> {
    scope: NativeScope<'a>,
    projection: NativeProjectedSnapshot,
    reference: NativeArtifactRef,
}
impl<'a> RestoredNativeView<'a> {
    pub(crate) fn into_snapshot(self) -> PlayerSnapshot {
        self.projection.into_snapshot()
    }
    /// Discard native borrowing and private command registry, retaining only the
    /// actual verified projection and its independent validation metadata.
    pub fn into_projection(self) -> NativeProjectedSnapshot {
        self.projection
    }
    /// Native scope with original issued handles, ready for proposals.
    pub fn scope(&self) -> &NativeScope<'a> {
        &self.scope
    }
    /// Deliver only this snapshot, never the private registry envelope.
    pub fn snapshot(&self) -> &PlayerSnapshot {
        self.projection.snapshot()
    }
    /// Actual native reprojection facts, not metadata copied from stored bytes.
    /// # Errors
    /// Rejects an inconsistent original vocabulary identity.
    pub fn projection_facts(
        &self,
    ) -> Result<crate::player_wire::PlayerProjectionFacts<'_>, NativeProjectionError> {
        self.projection
            .facts(self.scope.source.execution().creature_vocabulary())
    }
    /// Original artifact selected by SQL, not a regenerated projection ref.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
}
/// Project into a fresh registry, preserving the public/private relationship.
/// # Errors
/// Rejects invalid binding/display/projection and public/private engineering bounds.
/// Failures do not change native state or an existing scope.
pub fn prepare_view(
    source: &GameState,
    binding: NativeBinding,
    metadata: &NativeProjectionMetadata,
    display: &dyn NativeDisplaySource,
) -> Result<PreparedNativeView, NativeViewError> {
    prepare_view_checked(source, binding, metadata, display, None)
}
pub(in crate::native_game) fn prepare_historical_view(
    source: &GameState,
    binding: NativeBinding,
    metadata: &NativeProjectionMetadata,
    display: &dyn NativeDisplaySource,
    permit: &HistoricalProjectionPermit<'_>,
) -> Result<PreparedNativeView, NativeViewError> {
    prepare_view_checked(source, binding, metadata, display, Some(permit))
}
fn prepare_view_checked(
    source: &GameState,
    binding: NativeBinding,
    metadata: &NativeProjectionMetadata,
    display: &dyn NativeDisplaySource,
    permit: Option<&HistoricalProjectionPermit<'_>>,
) -> Result<PreparedNativeView, NativeViewError> {
    let mut scope = NativeScope::new(source, binding)?;
    if metadata.through_event_sequence < Amount::from(scope.basis().revision.get()) {
        return Err(NativeViewError::Invalid);
    }
    let snapshot = match permit {
        Some(permit) => scope
            .project_historical(metadata, display, permit)?
            .into_snapshot(),
        None => scope.project(metadata, display)?,
    };
    if scope.references.len() > MAX_REFERENCES
        || serde_json::to_vec(&snapshot)
            .map_err(NativeViewError::Json)?
            .len()
            > MAX_PLAYER_WIRE_BYTES
    {
        return Err(NativeViewError::Size);
    }
    let references = scope
        .references
        .iter()
        .map(|(handle, native)| {
            Ok(Reference {
                handle: handle.clone(),
                location: Location::encode(&scope, *native)?,
            })
        })
        .collect::<Result<_, NativeCodecError>>()?;
    let record = Record {
        version: VERSION.into(),
        source: Source::from_scope(&scope),
        viewer_seat_id: scope.viewer_seat_id(),
        disclosure: metadata.disclosure.into(),
        deadline_at: metadata.deadline_at.clone(),
        references,
        snapshot,
    };
    let bytes = serde_json::to_vec(&record).map_err(NativeViewError::Json)?;
    if u64::try_from(bytes.len()).map_err(|_| NativeViewError::Size)? > MAX_NATIVE_ARTIFACT_BYTES {
        return Err(NativeViewError::Size);
    }
    let reference = NativeArtifactRef::for_bytes(
        scope.basis().game_id,
        scope.basis().context_id,
        NativeArtifactRole::View,
        &bytes,
    )?;
    Ok(PreparedNativeView {
        bytes,
        reference,
        snapshot: record.snapshot,
    })
}
fn issued_handle(handle: &crate::player_wire::PlayerHandle) -> bool {
    let Some(text) = handle.as_str().strip_prefix("n1.") else {
        return false;
    };
    Uuid::parse_str(text).is_ok_and(|id| {
        id.to_string() == text
            && id.get_version_num() == 4
            && id.get_variant() == uuid::Variant::RFC4122
    })
}
/// Restore verified bytes against an accepted View and actual native source.
/// The trusted immutable display source must reproduce original descriptions.
/// # Errors
/// Rejects foreign source/viewer/cut, malformed records, aliases, missing/extra
/// bindings and public projection differences. Tracking every issued handle
/// prevents unused hidden-object registry entries surviving reprojection.
pub fn restore_view<'a>(
    artifact: NativeArtifact,
    expected: &NativeViewExpected,
    source: &'a GameState,
    binding: NativeBinding,
    display: &dyn NativeDisplaySource,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    restore_view_checked(artifact, expected, source, binding, display, None)
}
fn restore_view_checked<'a>(
    artifact: NativeArtifact,
    expected: &NativeViewExpected,
    source: &'a GameState,
    binding: NativeBinding,
    display: &dyn NativeDisplaySource,
    permit: Option<&HistoricalProjectionPermit<'_>>,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    if artifact.reference() != &expected.reference {
        return Err(NativeViewError::Invalid);
    }
    let reference = artifact.reference().clone();
    let record: Record =
        crate::decode_wire_object(artifact.bytes()).map_err(NativeViewError::Json)?;
    let mut scope = NativeScope::new(source, binding)?;
    if record.version != VERSION
        || record.source != Source::from_scope(&scope)
        || scope.basis() != &expected.basis
        || scope.viewer_seat_id() != expected.viewer
        || record.viewer_seat_id != expected.viewer
        || record.disclosure != expected.disclosure
        || record.snapshot.through_event_sequence != expected.through
        || record.references.len() > MAX_REFERENCES
        || serde_json::to_vec(&record.snapshot)
            .map_err(NativeViewError::Json)?
            .len()
            > MAX_PLAYER_WIRE_BYTES
    {
        return Err(NativeViewError::Invalid);
    }
    let mut handles = BTreeSet::new();
    for entry in record.references {
        if !issued_handle(&entry.handle) || !handles.insert(entry.handle.clone()) {
            return Err(NativeViewError::Invalid);
        }
        let native = entry.location.resolve(&scope)?;
        scope.validate_reference(native)?;
        if scope.references.iter().any(|(_, value)| *value == native) {
            return Err(NativeViewError::Invalid);
        }
        scope.references.push((entry.handle, native));
    }
    scope.used_handles = Some(BTreeSet::new());
    let metadata = NativeProjectionMetadata {
        through_event_sequence: expected.through.clone(),
        deadline_at: record.deadline_at,
        disclosure: record.disclosure.into(),
    };
    let projected = match permit {
        Some(permit) => scope.project_historical(&metadata, display, permit)?,
        None => scope.project_with_facts(&metadata, display)?,
    };
    if projected.snapshot() != &record.snapshot || scope.used_handles.as_ref() != Some(&handles) {
        return Err(NativeViewError::Invalid);
    }
    scope.used_handles = None;
    Ok(RestoredNativeView {
        scope,
        projection: projected,
        reference,
    })
}
/// Read exact private bytes, then verify snapshot/registry correspondence.
/// # Errors
/// Returns Store errors or restore_view source/codec failures.
pub fn load_view<'a>(
    store: &NativeArtifactStore,
    expected: &NativeViewExpected,
    source: &'a GameState,
    binding: NativeBinding,
    display: &dyn NativeDisplaySource,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    load_view_checked(store, expected, source, binding, display, None)
}
pub(in crate::native_game) fn load_historical_view<'a>(
    store: &NativeArtifactStore,
    expected: &NativeViewExpected,
    source: &'a GameState,
    binding: NativeBinding,
    display: &dyn NativeDisplaySource,
    permit: &HistoricalProjectionPermit<'_>,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    load_view_checked(store, expected, source, binding, display, Some(permit))
}
fn load_view_checked<'a>(
    store: &NativeArtifactStore,
    expected: &NativeViewExpected,
    source: &'a GameState,
    binding: NativeBinding,
    display: &dyn NativeDisplaySource,
    permit: Option<&HistoricalProjectionPermit<'_>>,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    let artifact = store.read(
        expected.reference(),
        expected.basis.game_id,
        expected.basis.context_id,
        NativeArtifactRole::View,
    )?;
    restore_view_checked(artifact, expected, source, binding, display, permit)
}

#[cfg(test)]
mod retained_tests {
    use super::*;
    use serde_json::{Value, json};

    // A consistent envelope fixture only, with no native rules qualification.
    fn fixture() -> (Value, NativeViewExpected, PlayerSnapshot) {
        let game = Uuid::from_u128(1);
        let context = Uuid::from_u128(2);
        let viewer = Uuid::from_u128(3);
        let basis = PlayerBasis {
            game_id: game,
            context_id: context,
            epoch: 4.into(),
            revision: 5.into(),
        };
        let snapshot = PlayerSnapshot {
            basis: basis.clone(),
            viewer_seat_id: viewer,
            players: vec![],
            turn: None,
            visible_objects: vec![],
            zones: vec![],
            stack: vec![],
            decisions: vec![],
            through_event_sequence: 9.into(),
        };
        let value = json!({
            "version": VERSION,
            "source": {
                "basis": basis,
                "context": {
                    "release": Uuid::from_u128(6),
                    "profile": Uuid::from_u128(7),
                    "catalogue": "catalogue-original",
                    "programs": "programs-original",
                    "rules": "rules-original",
                    "creatureVocabulary": "a".repeat(64),
                    "nameRules": "names-original"
                },
                "roster": [
                    {"seatId": viewer, "nativeSeat": "0"},
                    {"seatId": Uuid::from_u128(8), "nativeSeat": "1"}
                ]
            },
            "viewerSeatId": viewer,
            "disclosure": "live",
            "deadlineAt": null,
            "references": [],
            "snapshot": snapshot
        });
        let reference = NativeArtifactRef::for_bytes(
            game,
            context,
            NativeArtifactRole::View,
            &serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let expected = NativeViewExpected::new(
            reference,
            basis,
            viewer,
            NativeDisclosure::LiveParticipant,
            9.into(),
        )
        .unwrap();
        (value, expected, snapshot)
    }

    #[test]
    fn retained_view_checks_original_context_roster_basis_and_exact_snapshot() {
        let (good, expected, snapshot) = fixture();
        let roster = [(Uuid::from_u128(3), 0), (Uuid::from_u128(8), 1)];
        let source = RetainedViewSource {
            release: Uuid::from_u128(6),
            profile: Uuid::from_u128(7),
            catalogue: "catalogue-original",
            programs: "programs-original",
            rules: "rules-original",
            vocabulary_id: "a".repeat(64).try_into().unwrap(),
            name_rules: "names-original",
            roster: &roster,
        };
        let verify = |value: &Value| {
            let record: Record =
                crate::decode_wire_object(&serde_json::to_vec(value).unwrap()).unwrap();
            verify_retained_record(&record, &expected, &source, &snapshot)
        };
        assert!(verify(&good).is_ok());
        for (pointer, changed) in [
            ("/version", json!("foreign")),
            ("/source/context/release", json!(Uuid::from_u128(99))),
            ("/source/context/profile", json!(Uuid::from_u128(99))),
            ("/source/context/catalogue", json!("foreign")),
            ("/source/context/programs", json!("foreign")),
            ("/source/context/rules", json!("foreign")),
            ("/source/context/creatureVocabulary", json!("b".repeat(64))),
            ("/source/context/nameRules", json!("foreign")),
            ("/source/basis/gameId", json!(Uuid::from_u128(99))),
            ("/source/basis/contextId", json!(Uuid::from_u128(99))),
            ("/source/basis/epoch", json!("99")),
            ("/source/basis/revision", json!("6")),
            ("/source/roster/0/seatId", json!(Uuid::from_u128(99))),
            ("/source/roster/0/nativeSeat", json!("99")),
            ("/viewerSeatId", json!(Uuid::from_u128(8))),
            ("/disclosure", json!("completed")),
            ("/snapshot/basis/revision", json!("6")),
            ("/snapshot/viewerSeatId", json!(Uuid::from_u128(8))),
            ("/snapshot/throughEventSequence", json!("10")),
            (
                "/snapshot/turn",
                json!({
                    "number": "1", "activeSeatIds": [Uuid::from_u128(3)],
                    "prioritySeatIds": [], "phase": "beginning", "step": "untap"
                }),
            ),
        ] {
            let mut changed_record = good.clone();
            *changed_record.pointer_mut(pointer).unwrap() = changed;
            assert!(
                matches!(verify(&changed_record), Err(NativeViewError::Invalid)),
                "{pointer}"
            );
            assert!(verify(&good).is_ok(), "restored {pointer}");
        }
        let mut reordered = good.clone();
        reordered["source"]["roster"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(matches!(verify(&reordered), Err(NativeViewError::Invalid)));
        let mut missing = good.clone();
        missing["source"]["roster"].as_array_mut().unwrap().pop();
        assert!(matches!(verify(&missing), Err(NativeViewError::Invalid)));
        let mut extra = good.clone();
        extra["source"]["roster"]
            .as_array_mut()
            .unwrap()
            .push(json!({"seatId": Uuid::from_u128(9), "nativeSeat": "2"}));
        assert!(matches!(verify(&extra), Err(NativeViewError::Invalid)));
        assert!(verify(&good).is_ok());
        let mut too_many = good.clone();
        too_many["references"] = json!(vec![
            json!({"handle": "retained-test-handle", "location": {"kind": "object", "index": "0"}});
            MAX_REFERENCES + 1
        ]);
        assert!(matches!(verify(&too_many), Err(NativeViewError::Size)));
        let mut oversized = good.clone();
        oversized["snapshot"]["players"] = json!(vec![
            json!({
                "seatId": Uuid::from_u128(3), "life": "20", "poison": "0",
                "counters": [], "manaPool": [], "handCount": "0",
                "libraryCount": "0", "eliminated": false
            });
            MAX_PLAYER_WIRE_BYTES / 100
        ]);
        let record: Record =
            crate::decode_wire_object(&serde_json::to_vec(&oversized).unwrap()).unwrap();
        assert!(serde_json::to_vec(&record.snapshot).unwrap().len() > MAX_PLAYER_WIRE_BYTES);
        // Equal expected bytes still cannot bypass the snapshot engineering limit.
        assert!(matches!(
            verify_retained_record(&record, &expected, &source, &record.snapshot),
            Err(NativeViewError::Size)
        ));
        assert!(verify(&good).is_ok());
    }
}
