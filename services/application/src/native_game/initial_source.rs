//! Original native initialization from an exact authorized root and guarded source.
//! Materialization is not release/deck/format admission or durable acceptance.
use super::*;
use crate::{
    player_wire::NativeRevision,
    price_source::{EvidenceHash, hash_text},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{OracleId, Registry, RegistryError};
use spellwake_content::snapshot::SnapshotError;
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyError, CreatureVocabularyId},
    names::NameRules,
};
mod admission_manifest;
pub(crate) use admission_manifest::AdmissionManifest;
mod catalogue;
pub(super) mod commander_admission;
mod constructed_policy;
mod release_candidate;
pub use constructed_policy::inspect_constructed_room;
pub use release_candidate::{inspect_limited_deck, inspect_release_candidate};
mod printed;
pub use printed::NativePrintedCard;
mod overlay;
pub use overlay::{FrozenCustomCardRules, NativeProgramOverlay};
mod display;
pub use display::NativeImmutableDisplay;
mod selected;
mod token_display;
pub use selected::{NativeInitialProvenance, NativeInitialSelection, NativePhysicalCardSource};
mod root;
pub use root::{
    NativeInitialCard, NativeInitialExpected, NativeInitialIds, NativeInitialParticipant,
    NativeInitialSpec, NativeInitialZone, PreparedNativeOpening, load_initial, prepare_initial,
    prepare_reserved_commander_opening, prepare_selected_initial, restore_initial,
};

/// Strict private initialization/source failure; underlying details stay private.
pub enum NativeInitialError {
    /// Invalid original shape, bounds, IDs or prepared setup.
    Invalid,
    /// Exact authorized root/header/roster/source identity does not match.
    ForeignSource,
    /// Original private root exceeds the declared engineering bound.
    Size,
    /// Original JSON syntax/shape failure.
    Json(serde_json::Error),
    /// Actual private artifact transport/identity failure.
    Artifact(NativeArtifactError),
    /// Source snapshot failure, retained as a typed value.
    Snapshot(Box<SnapshotError>),
    /// Actual guarded registry rejection.
    Registry(Box<RegistryError>),
    /// Actual immutable vocabulary rejection.
    Vocabulary(CreatureVocabularyError),
    /// Actual native initializer rejection, retained without Debug disclosure.
    Native(Box<GameError>),
    /// Exact roster/basis consistency failure.
    Binding(NativeBridgeError),
}
impl fmt::Display for NativeInitialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ForeignSource => "native initial source does not match",
            Self::Size => "native initial source exceeds its boundary",
            Self::Artifact(_) => "native initial artifact is unavailable",
            Self::Invalid | Self::Json(_) => "invalid native initial source",
            Self::Snapshot(_) | Self::Registry(_) | Self::Vocabulary(_) => {
                "native program source is unavailable"
            }
            Self::Native(_) | Self::Binding(_) => "native opening cannot be reconstructed",
        })
    }
}
impl fmt::Debug for NativeInitialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeInitialError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Artifact(e) => Some(e),
            Self::Registry(e) => Some(e.as_ref()),
            Self::Vocabulary(e) => Some(e),
            Self::Binding(e) => Some(e),
            Self::Snapshot(e) => match e.as_ref() {
                SnapshotError::Operational(e) => Some(e),
                SnapshotError::Invalid { .. } => None,
            },
            Self::Invalid | Self::ForeignSource | Self::Size | Self::Native(_) => None,
        }
    }
}
impl NativeInitialError {
    /// Exact private native rejection, never a public error message.
    pub fn native_error(&self) -> Option<&GameError> {
        match self {
            Self::Native(e) => Some(e),
            _ => None,
        }
    }
    /// Exact private source decoding rejection where upstream has no Error impl.
    pub fn snapshot_error(&self) -> Option<&SnapshotError> {
        match self {
            Self::Snapshot(e) => Some(e),
            _ => None,
        }
    }
}
impl From<NativeArtifactError> for NativeInitialError {
    fn from(e: NativeArtifactError) -> Self {
        Self::Artifact(e)
    }
}
impl From<RegistryError> for NativeInitialError {
    fn from(e: RegistryError) -> Self {
        Self::Registry(Box::new(e))
    }
}
impl From<NativeBridgeError> for NativeInitialError {
    fn from(e: NativeBridgeError) -> Self {
        Self::Binding(e)
    }
}
fn digest(bytes: &[u8]) -> EvidenceHash {
    hash_text(&Sha256::digest(bytes))
}
/// Conservative compiled declared-input ID, not deployed binary attestation.
pub fn native_runtime_identity() -> &'static str {
    env!("SPELLWAKE_NATIVE_RUNTIME_ID")
}
/// The exact maintained guarded source pool; neither is a qualified public format.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeProgramPool {
    /// Original nine-source sacrifice/discard pool (registry9).
    ResolutionSelection,
    /// Twelve-source pool with natural-mana helpers (registry10).
    ResolutionSelectionNaturalMana,
    /// Complete-face keyword compiler and the mana sources used in its games.
    KeywordGameplay,
}
impl NativeProgramPool {
    /// Canonical source materialization profile, not a table/release approval.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KeywordGameplay => spellwake_card_programs::CURRENT_KEYWORD_PROFILE,
            Self::ResolutionSelection => spellwake_card_programs::RESOLUTION_SELECTION_PROFILE,
            Self::ResolutionSelectionNaturalMana => {
                spellwake_card_programs::RESOLUTION_SELECTION_NATURAL_MANA_PROFILE
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VocabularyInput {
    rules_revision: String,
    rules_sha256: String,
    members: Vec<String>,
    id: CreatureVocabularyId,
}
/// Decode exact retained vocabulary input bytes into the typed vocabulary.
/// Shared by source construction and the portable replay reader, so archived
/// labels are checked against the original vocabulary, never a newer one.
/// # Errors
/// Rejects malformed input, invalid members and a declared id that differs.
pub(in crate::native_game) fn decode_vocabulary_input(
    vocabulary: &[u8],
) -> Result<CreatureTypeVocabulary, NativeInitialError> {
    if vocabulary.is_empty() || vocabulary.len() > 1_048_576 {
        return Err(NativeInitialError::Size);
    }
    let raw: VocabularyInput =
        crate::decode_wire_object(vocabulary).map_err(NativeInitialError::Json)?;
    let rules_hash = crate::source_protocol::digest_bytes(&raw.rules_sha256)
        .ok_or(NativeInitialError::Invalid)?;
    let native = CreatureTypeVocabulary::new(raw.rules_revision, rules_hash, raw.members)
        .map_err(NativeInitialError::Vocabulary)?;
    if native.id() != raw.id {
        return Err(NativeInitialError::ForeignSource);
    }
    Ok(native)
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceIdentity {
    version: String,
    runtime: String,
    pool: String,
    registry_version: u32,
    #[serde(deserialize_with = "crate::provider_json::object")]
    metadata: BlobIdentity,
    #[serde(deserialize_with = "crate::provider_json::object")]
    snapshot: BlobIdentity,
    #[serde(deserialize_with = "crate::provider_json::object")]
    vocabulary: BlobIdentity,
    name_rules: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BlobIdentity {
    sha256: EvidenceHash,
    bytes: u64,
}
fn blob(bytes: &[u8]) -> Result<BlobIdentity, NativeInitialError> {
    Ok(BlobIdentity {
        sha256: digest(bytes),
        bytes: u64::try_from(bytes.len()).map_err(|_| NativeInitialError::Size)?,
    })
}
const MAX_PROGRAM_IDENTITY_BYTES: usize = 16 * 1024;

/// Preserve the original domain-separated, length-prefixed source identity recipe.
fn program_identity(encoded: &[u8]) -> Result<EvidenceHash, NativeInitialError> {
    let mut hash = Sha256::new();
    hash.update(b"spellwake.native-program-source.v1\0");
    hash.update(
        u64::try_from(encoded.len())
            .map_err(|_| NativeInitialError::Size)?
            .to_be_bytes(),
    );
    hash.update(encoded);
    Ok(hash_text(&hash.finalize()))
}

/// Original input identities checked against an independently accepted opening.
/// This is not authority to execute or qualify absent descriptor inputs.
pub(in crate::native_game) struct VerifiedProgramIdentity(SourceIdentity);
impl VerifiedProgramIdentity {
    /// Exact original catalogue blob identity for stored-View context comparison.
    pub(in crate::native_game) fn catalogue_id(&self) -> &str {
        self.0.snapshot.sha256.as_str()
    }
    /// Original name rules identity for stored-View context comparison.
    pub(in crate::native_game) fn name_rules_id(&self) -> &str {
        &self.0.name_rules
    }
}

/// Authenticate original descriptor/vocabulary bytes without today's runtime.
/// # Errors
/// Rejects out-of-bound descriptors, malformed objects (including nested objects),
/// or disagreement with the accepted original runtime, program ID or vocabulary.
/// Other descriptor blobs remain input identities; their contents are not decoded.
pub(in crate::native_game) fn verify_program_identity(
    bytes: &[u8],
    vocabulary: &[u8],
    expected_runtime: &str,
    expected_program: &EvidenceHash,
) -> Result<VerifiedProgramIdentity, NativeInitialError> {
    if bytes.is_empty() || bytes.len() > MAX_PROGRAM_IDENTITY_BYTES {
        return Err(NativeInitialError::Size);
    }
    let descriptor: SourceIdentity =
        crate::decode_wire_object(bytes).map_err(NativeInitialError::Json)?;
    if descriptor.version != "native-program-source-v1" {
        return Err(NativeInitialError::Invalid);
    }
    // The source producer hashes its canonical field order and string encoding.
    let encoded = serde_json::to_vec(&descriptor).map_err(NativeInitialError::Json)?;
    if encoded != bytes
        || descriptor.runtime != expected_runtime
        || program_identity(&encoded)? != *expected_program
        || descriptor.vocabulary.bytes
            != u64::try_from(vocabulary.len()).map_err(|_| NativeInitialError::Size)?
        || descriptor.vocabulary.sha256 != digest(vocabulary)
    {
        return Err(NativeInitialError::ForeignSource);
    }
    Ok(VerifiedProgramIdentity(descriptor))
}

/// Immutable shared native source materialization. Keep one package per identity;
/// per-game roots reference it instead of copying a full catalogue each time.
/// No Debug/Serialize and no mutable registry/vocabulary access is supplied.
pub struct NativeProgramSource {
    registry: Registry,
    commander_inventory: Option<spellwake_content::Inventory>,
    catalogue: std::collections::BTreeMap<OracleId, spellwake_content::InventoryCard>,
    meld_results: std::collections::BTreeMap<OracleId, (OracleId, spellwake_content::CatalogFace)>,
    printed_index: printed::PrintedCardIndex,
    vocabulary: Arc<CreatureTypeVocabulary>,
    vocabulary_input: Box<[u8]>,
    program_identity_bytes: Box<[u8]>,
    names: Arc<NameRules>,
    identity: EvidenceHash,
    snapshot: EvidenceHash,
    pool: NativeProgramPool,
}
impl NativeProgramSource {
    /// Authenticate exact input bytes/content against the explicit guarded pool.
    /// The caller still establishes provider/release authority and original root.
    /// # Errors
    /// Rejects byte bounds/foreign snapshot before decompression, malformed source
    /// or vocabulary, mismatched identities and actual guarded registry failures.
    pub fn from_bytes(
        pool: NativeProgramPool,
        metadata: &[u8],
        snapshot: Vec<u8>,
        vocabulary: &[u8],
    ) -> Result<Self, NativeInitialError> {
        if metadata.is_empty()
            || metadata.len() > 1_048_576
            || vocabulary.is_empty()
            || vocabulary.len() > 1_048_576
            || snapshot.is_empty()
            || u64::try_from(snapshot.len()).map_err(|_| NativeInitialError::Size)?
                > MAX_NATIVE_ARTIFACT_BYTES
        {
            return Err(NativeInitialError::Size);
        }
        let metadata_identity = blob(metadata)?;
        let snapshot_identity = blob(&snapshot)?;
        let vocabulary_identity = blob(vocabulary)?;
        if snapshot_identity.sha256.hex() != spellwake_card_programs::TEACHING_SOURCE_SHA256 {
            return Err(NativeInitialError::ForeignSource);
        }
        let native = decode_vocabulary_input(vocabulary)?;
        let inventory = spellwake_content::snapshot::decode(metadata, snapshot)
            .map_err(|e| NativeInitialError::Snapshot(Box::new(e)))?;
        let registry = match pool {
            NativeProgramPool::KeywordGameplay => {
                Registry::from_current_keyword_inventory(&inventory, &native)?
            }
            NativeProgramPool::ResolutionSelection => {
                Registry::from_resolution_selection_inventory(&inventory, &native)?
            }
            NativeProgramPool::ResolutionSelectionNaturalMana => {
                Registry::from_resolution_selection_natural_mana_inventory(&inventory, &native)?
            }
        };
        let mut meld_results = std::collections::BTreeMap::new();
        for card in inventory.cards.iter().filter(|row| row.layout == "meld") {
            let Some(raw) = card.oracle_id.as_ref() else {
                continue;
            };
            let Some(set) = inventory.meld_set(raw) else {
                continue;
            };
            let id = OracleId::try_from(raw.clone()).map_err(|_| NativeInitialError::Invalid)?;
            if !registry.resolve(&id).is_some_and(|bound| {
                bound.program().triggered.iter().any(|ability| {
                    ability.body.instructions.iter().any(|op| {
                        matches!(
                            op,
                            spellwake_engine::program::Instruction::MeldExiledPair(_)
                        )
                    })
                })
            }) {
                continue;
            }
            let result = inventory
                .cards
                .iter()
                .find(|row| row.layout == "meld" && row.name == set.result)
                .ok_or(NativeInitialError::Invalid)?;
            let [face] = result.faces.as_slice() else {
                return Err(NativeInitialError::Invalid);
            };
            let result_id = OracleId::try_from(
                result
                    .oracle_id
                    .clone()
                    .ok_or(NativeInitialError::Invalid)?,
            )
            .map_err(|_| NativeInitialError::Invalid)?;
            meld_results.insert(id, (result_id, face.clone()));
        }
        let catalogue = catalogue::retain(&inventory.cards, &registry)?;
        let commander_inventory = (pool == NativeProgramPool::KeywordGameplay).then_some(inventory);
        let names = Arc::new(NameRules::literal_names());
        let identity = SourceIdentity {
            version: "native-program-source-v1".into(),
            runtime: native_runtime_identity().into(),
            pool: pool.as_str().into(),
            registry_version: registry.coverage().registry_version,
            metadata: metadata_identity,
            snapshot: BlobIdentity {
                sha256: snapshot_identity.sha256.clone(),
                bytes: snapshot_identity.bytes,
            },
            vocabulary: vocabulary_identity,
            name_rules: names.id().to_string(),
        };
        let encoded = serde_json::to_vec(&identity).map_err(NativeInitialError::Json)?;
        if encoded.len() > MAX_PROGRAM_IDENTITY_BYTES {
            return Err(NativeInitialError::Size);
        }
        let printed_index = printed::PrintedCardIndex::build(&catalogue, &registry, &meld_results);
        Ok(Self {
            registry,
            commander_inventory,
            catalogue,
            meld_results,
            printed_index,
            vocabulary: Arc::new(native),
            vocabulary_input: vocabulary.into(),
            names,
            identity: program_identity(&encoded)?,
            program_identity_bytes: encoded.into_boxed_slice(),
            snapshot: snapshot_identity.sha256,
            pool,
        })
    }
    /// Exact original package identity, including declared runtime inputs.
    pub fn identity(&self) -> &EvidenceHash {
        &self.identity
    }
    /// Canonical original source descriptor bytes, retained once per package.
    pub(in crate::native_game) fn program_identity_bytes(&self) -> &[u8] {
        &self.program_identity_bytes
    }
    /// Exact bounded original vocabulary input, retained once per source package.
    /// Its byte identity participates in identity(); archive retention must preserve
    /// these bytes rather than regenerate a members-only assertion. Construction
    /// checks content identity, not independent provider/release qualification.
    pub fn vocabulary_input(&self) -> &[u8] {
        &self.vocabulary_input
    }
    /// Borrow the exact saved source record for an actually materialized Oracle.
    /// Unsupported identities have no record here. This is immutable source
    /// metadata, not card/format readiness, visibility or collection ownership.
    /// Its external Scryfall printing reference does not identify the player's
    /// Describe the exact guarded runtime and four cards in the Commander review fixture.
    ///
    /// # Errors
    /// Rejects a source pool that does not bind every selected Oracle identity.
    pub fn commander_pilot_source(&self) -> Result<serde_json::Value, NativeInitialError> {
        let inventory = self
            .commander_inventory
            .as_ref()
            .ok_or(NativeInitialError::ForeignSource)?;
        let cards = [
            "Rograkh, Son of Rohgahh",
            "Ishai, Ojutai Dragonspeaker",
            "Island",
            "Plains",
        ]
        .into_iter()
        .map(|name| {
            let card = inventory
                .cards
                .iter()
                .find(|card| card.name == name)
                .ok_or(NativeInitialError::ForeignSource)?;
            let oracle = OracleId::try_from(
                card.oracle_id
                    .clone()
                    .ok_or(NativeInitialError::ForeignSource)?,
            )
            .map_err(|_| NativeInitialError::ForeignSource)?;
            self.registry
                .resolve(&oracle)
                .ok_or(NativeInitialError::ForeignSource)?;
            let proof = digest(
                format!(
                    "spellwake-commander-release-program-v1/{}/{}",
                    self.identity().as_str(),
                    oracle.as_str()
                )
                .as_bytes(),
            );
            Ok(serde_json::json!({"name":name,"oracleId":oracle.as_str(),"program":proof}))
        })
        .collect::<Result<Vec<_>, NativeInitialError>>()?;
        Ok(serde_json::json!({
            "scope":"commander-review-v1",
            "kernelRef":format!("sha256:{}",native_runtime_identity()),
            "rulesRef":hash_text(self.vocabulary.rules_sha256()),
            "programManifestRef":self.identity(),
            "catalogueSha256":inventory.compressed_sha256,
            "cards":cards
        }))
    }

    /// chosen printing or an internal application definition/printing row.
    pub fn source_card(&self, oracle: &OracleId) -> Option<&spellwake_content::InventoryCard> {
        self.catalogue.get(oracle)
    }
    /// Explicit guarded source pool.
    pub fn pool(&self) -> NativeProgramPool {
        self.pool
    }
    /// Current registry's actual whole-card qualification count, presently zero.
    pub fn fully_qualified_cards(&self) -> usize {
        self.registry.coverage().fully_qualified_cards
    }
    /// Whether this guarded pool materialized the requested rules identity.
    pub fn supports(&self, oracle: &OracleId) -> bool {
        self.registry.resolve(oracle).is_some()
    }
    fn context(
        &self,
        release: Uuid,
        profile: Uuid,
    ) -> Result<spellwake_engine::game::GameContext, NativeInitialError> {
        if release.is_nil() || profile.is_nil() {
            return Err(NativeInitialError::Invalid);
        }
        spellwake_engine::game::GameContext::new(
            release.to_string(),
            self.vocabulary.rules_revision().to_owned(),
            self.snapshot.as_str().into(),
            self.identity.as_str().into(),
            profile.to_string(),
            self.vocabulary.id(),
            self.names.id(),
        )
        .map_err(|e| NativeInitialError::Native(Box::new(e)))
    }
}

#[cfg(test)]
pub(super) mod tests;
