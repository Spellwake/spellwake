//! Closed per-game original setup. Reference authority comes from the caller.
use super::selected::SelectedRoot;
use super::*;
use crate::native_game::preparation_source::NativeReservedGameSource;
use spellwake_card_programs::commander::prepare_deck;
use spellwake_engine::characteristics::Color;
use spellwake_engine::{
    combat::CombatRules,
    effects::SeedCard,
    game::{GameId, OpeningSetup},
    object_identity::{CardId, ObjectDestination},
    setup::{GameSeed, StartingLife},
    shuffle::EntropyTape,
};
use std::collections::BTreeMap;

const COMMANDER_ROOT: &str = "native-initial-commander-v3";
const MAX_ROOT_BYTES: usize = 1_048_576;
const MAX_ENTROPY_WORDS: usize = 65_536;
/// One original prepared zone, never an arbitrary saved continuation.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum NativeInitialZone {
    /// Declared top-to-bottom pre-shuffle library, inserted forward at LibraryBottom.
    Library,
    /// Prepared command-zone card; no Commander/companion qualification implied.
    Command,
}
impl TryFrom<String> for NativeInitialZone {
    type Error = NativeInitialError;
    fn try_from(v: String) -> Result<Self, Self::Error> {
        match v.as_str() {
            "library" => Ok(Self::Library),
            "command" => Ok(Self::Command),
            _ => Err(NativeInitialError::Invalid),
        }
    }
}
impl From<NativeInitialZone> for String {
    fn from(v: NativeInitialZone) -> Self {
        match v {
            NativeInitialZone::Library => "library",
            NativeInitialZone::Command => "command",
        }
        .into()
    }
}
/// One exact source-card occurrence. Repetition does not share a native CardId.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeInitialCard {
    pub oracle_id: OracleId,
    pub zone: NativeInitialZone,
}
/// Exact accepted seat input. This shape does not prove ownership or deck legality.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeInitialParticipant {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub life: RulesInteger,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub cards: Vec<NativeInitialCard>,
}
/// Original prepared setup selected by a trusted admission producer.
/// Never accept this as a player-owned Program/roster/entropy authority.
pub struct NativeInitialSpec {
    pub first_player_seat_id: Uuid,
    pub participants: Vec<NativeInitialParticipant>,
    pub entropy: Vec<NativeRevision>,
}
/// Original application identity selected by accepted database source.
#[derive(Clone, Copy)]
pub struct NativeInitialIds {
    pub game_id: Uuid,
    pub context_id: Uuid,
    pub release_id: Uuid,
    pub profile_id: Uuid,
}
fn valid_ids(ids: NativeInitialIds) -> bool {
    [ids.game_id, ids.context_id, ids.release_id, ids.profile_id]
        .iter()
        .all(|id| !id.is_nil())
}
fn valid_roster(roster: &[Uuid]) -> bool {
    (2..=6).contains(&roster.len())
        && roster.iter().all(|id| !id.is_nil())
        && roster.iter().copied().collect::<BTreeSet<_>>().len() == roster.len()
}
/// Independently authorized exact initial artifact and accepted original tuple.
/// Construction checks consistency, never creates database/source authority.
pub struct NativeInitialExpected {
    reference: NativeArtifactRef,
    ids: NativeInitialIds,
    roster: Vec<Uuid>,
}
impl NativeInitialExpected {
    /// Bind the exact ref selected by accepted database/source, never player input.
    /// # Errors
    /// Rejects wrong artifact role/size, nil identities and invalid ordered roster.
    pub fn new(
        reference: NativeArtifactRef,
        release_id: Uuid,
        profile_id: Uuid,
        roster: Vec<Uuid>,
    ) -> Result<Self, NativeInitialError> {
        let ids = NativeInitialIds {
            game_id: reference.game_id(),
            context_id: reference.context_id(),
            release_id,
            profile_id,
        };
        if reference.role() != NativeArtifactRole::Initial
            || reference.size()
                > u64::try_from(MAX_ROOT_BYTES).map_err(|_| NativeInitialError::Size)?
            || !valid_ids(ids)
            || !valid_roster(&roster)
        {
            return Err(NativeInitialError::Invalid);
        }
        Ok(Self {
            reference,
            ids,
            roster,
        })
    }
    /// Exact independently selected original ref, still not a public/signed URL.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Root {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    profile_id: Uuid,
    program_source_id: EvidenceHash,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    first_player_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    participants: Vec<NativeInitialParticipant>,
    entropy: Vec<NativeRevision>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "selected_object"
    )]
    selected: Option<SelectedRoot>,
}
fn selected_object<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<SelectedRoot>, D::Error> {
    // Present null is invalid; only actual absence is the historical v1 default.
    crate::provider_json::object(d).map(Some)
}
struct Initialized {
    candidate: NativeCandidate,
    roster: Vec<NativeSeatBinding>,
    provenance: Option<Arc<NativeInitialProvenance>>,
}
impl Root {
    fn ids(&self) -> NativeInitialIds {
        NativeInitialIds {
            game_id: self.game_id,
            context_id: self.context_id,
            release_id: self.release_id,
            profile_id: self.profile_id,
        }
    }
    fn roster(&self) -> Vec<Uuid> {
        self.participants.iter().map(|p| p.seat_id).collect()
    }
    fn validate(&self, source: &NativeProgramSource) -> Result<(), NativeInitialError> {
        let roster = self.roster();
        if !matches!(
            (self.version.as_str(), self.selected.is_some()),
            ("native-initial-v1", false) | ("native-initial-v2" | COMMANDER_ROOT, true)
        ) || !valid_ids(self.ids())
            || !valid_roster(&roster)
            || !roster.contains(&self.first_player_seat_id)
            || self.entropy.is_empty()
            || self.entropy.len() > MAX_ENTROPY_WORDS
            || self.participants.iter().any(|p| {
                !p.life.is_positive() || p.cards.len() > crate::player_wire::MAX_PLAYER_ITEMS
            })
        {
            return Err(NativeInitialError::Invalid);
        }
        if self.version == COMMANDER_ROOT && !(2..=3).contains(&roster.len()) {
            return Err(NativeInitialError::Invalid);
        }
        if &self.program_source_id != source.identity() {
            return Err(NativeInitialError::ForeignSource);
        }
        self.selected_indices()?;
        Ok(())
    }
    fn selected_indices(&self) -> Result<Option<Vec<Vec<usize>>>, NativeInitialError> {
        self.selected
            .as_ref()
            .map(|selected| {
                if self.version == COMMANDER_ROOT {
                    selected.validate_commander(self.ids(), &self.participants)
                } else {
                    selected.validate(self.ids(), &self.participants)
                }
            })
            .transpose()
    }

    fn commander_identities(
        &self,
        source: &NativeProgramSource,
        roster: &[NativeSeatBinding],
    ) -> Result<BTreeMap<SeatId, BTreeSet<Color>>, NativeInitialError> {
        let inventory = source
            .commander_inventory
            .as_ref()
            .ok_or(NativeInitialError::ForeignSource)?;
        self.participants
            .iter()
            .zip(roster)
            .map(|(participant, seat)| {
                let mut commanders = Vec::new();
                let mut library = Vec::new();
                for card in &participant.cards {
                    match card.zone {
                        NativeInitialZone::Library => library.push(card.oracle_id.clone()),
                        NativeInitialZone::Command => commanders.push(card.oracle_id.clone()),
                    }
                }
                let deck = prepare_deck(
                    inventory,
                    &source.names,
                    &commanders,
                    &library,
                    &BTreeMap::new(),
                )
                .map_err(|_| NativeInitialError::ForeignSource)?;
                Ok((seat.native_seat, deck.color_identity().clone()))
            })
            .collect()
    }

    fn initialize(self, source: &NativeProgramSource) -> Result<Initialized, NativeInitialError> {
        self.validate(source)?;
        let indices = self.selected_indices()?;
        let mut physical = BTreeMap::new();
        let mut cards: Vec<SeedCard> = Vec::new();
        let mut roster = Vec::new();
        let mut life = Vec::new();
        let mut next_card = 1_u64;
        let mut active = None;
        for (index, participant) in self.participants.iter().enumerate() {
            let native_seat =
                SeatId(u32::try_from(index).map_err(|_| NativeInitialError::Invalid)?);
            if participant.seat_id == self.first_player_seat_id {
                active = Some(native_seat);
            }
            roster.push(NativeSeatBinding {
                seat_id: participant.seat_id,
                native_seat,
            });
            life.push(StartingLife {
                player: native_seat,
                total: participant.life.clone(),
            });
            for (card_index, card) in participant.cards.iter().enumerate() {
                let destination = match card.zone {
                    NativeInitialZone::Library => ObjectDestination::LibraryBottom,
                    NativeInitialZone::Command => ObjectDestination::Command,
                };
                cards.push(source.registry.seed(
                    &card.oracle_id,
                    CardId(next_card),
                    native_seat,
                    destination,
                )?);
                if let Some(indices) = &indices {
                    physical.insert(CardId(next_card), (index, indices[index][card_index]));
                }
                next_card = next_card
                    .checked_add(1)
                    .ok_or(NativeInitialError::Invalid)?;
            }
        }
        let setup = OpeningSetup {
            seed: GameSeed {
                seats: roster.iter().map(|s| s.native_seat).collect(),
                active: active.ok_or(NativeInitialError::Invalid)?,
                combat_rules: CombatRules::UnrestrictedFreeForAll,
                cards,
                initial_battlefield_order: vec![],
                initial_world_cohorts: vec![],
                mana: vec![],
                life,
                creature_vocabulary: Arc::clone(&source.vocabulary),
                name_rules: Arc::clone(&source.names),
            },
            entropy: EntropyTape::new(self.entropy.iter().map(|word| word.get()).collect()),
        };
        let game = GameId(self.game_id.as_u128());
        let context = source.context(self.release_id, self.profile_id)?;
        let candidate = if self.version == COMMANDER_ROOT {
            let identities = self.commander_identities(source, &roster)?;
            GameState::initialize_commander_opening_with_identities(
                game, context, setup, identities,
            )
        } else {
            GameState::initialize_opening(game, context, setup)
        }
        .map_err(|error| NativeInitialError::Native(Box::new(error)))?;
        let ids = self.ids();
        Ok(Initialized {
            candidate,
            roster,
            provenance: self
                .selected
                .map(|selected| Arc::new(selected.bind(ids, physical))),
        })
    }
}
/// Exact original root plus actual uninstalled opening candidate and seat mapping.
/// No Debug, database acceptance, public admission or full-card qualification.
pub struct PreparedNativeOpening {
    bytes: Vec<u8>,
    reference: NativeArtifactRef,
    candidate: NativeCandidate,
    roster: Vec<NativeSeatBinding>,
    provenance: Option<Arc<NativeInitialProvenance>>,
}
impl PreparedNativeOpening {
    /// Exact frozen selected origin mapping, absent for historical v1 roots.
    pub fn provenance(&self) -> Option<&NativeInitialProvenance> {
        self.provenance.as_deref()
    }
    pub(in crate::native_game) fn shared_provenance(&self) -> Option<Arc<NativeInitialProvenance>> {
        self.provenance.clone()
    }
    /// Original root bytes for private retention; entropy must stay confidential.
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content-derived original ref; this getter does not prove retention/acceptance.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
    /// Actual native opening candidate; caller must durably accept before install.
    pub fn candidate(&self) -> &NativeCandidate {
        &self.candidate
    }
    /// Bind a caller-established participant to this exact uninstalled candidate.
    /// # Errors
    /// Rejects foreign viewer or inconsistent native/application basis/roster.
    pub fn binding(&self, viewer: Uuid) -> Result<NativeBinding, NativeInitialError> {
        Ok(NativeBinding::new(
            self.reference.context_id(),
            self.candidate.state().basis().clone(),
            self.roster.clone(),
            viewer,
        )?)
    }
    /// Transfer exact original bytes/ref/candidate/roster to durable acceptance owner.
    pub fn into_parts(
        self,
    ) -> (
        Vec<u8>,
        NativeArtifactRef,
        NativeCandidate,
        Vec<NativeSeatBinding>,
    ) {
        (self.bytes, self.reference, self.candidate, self.roster)
    }
}
/// Produce an original root and actual opening proposal from trusted prepared inputs.
/// No release/deck/collection source authority is established by this function.
/// # Errors
/// Rejects invalid source/setup/bounds and actual guarded/native initialization errors.
pub fn prepare_initial(
    ids: NativeInitialIds,
    source: &NativeProgramSource,
    spec: NativeInitialSpec,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    let root = Root {
        version: "native-initial-v1".into(),
        game_id: ids.game_id,
        context_id: ids.context_id,
        release_id: ids.release_id,
        profile_id: ids.profile_id,
        program_source_id: source.identity().clone(),
        first_player_seat_id: spec.first_player_seat_id,
        participants: spec.participants,
        entropy: spec.entropy,
        selected: None,
    };
    prepare_root(root, source)
}
/// Preserve a sealed reservation's selected entries and explicit physical assignments.
/// The admission policy still owns the complete setup, section/zone rules, life,
/// entropy, format and native qualification. This function does not grant admission.
/// # Errors
/// Rejects foreign seat/entry/Oracle, overconsumption, invalid scope/setup and original
/// byte bounds, in addition to actual guarded source/native initialization errors.
pub fn prepare_selected_initial(
    context_id: Uuid,
    source: &NativeProgramSource,
    reservation: &NativeReservedGameSource,
    spec: NativeInitialSpec,
    selection: Vec<NativeInitialSelection>,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    let r = reservation.metadata();
    let root = Root {
        version: "native-initial-v2".into(),
        game_id: r.game_id,
        context_id,
        release_id: r.release.id,
        profile_id: r.profile.id,
        program_source_id: source.identity().clone(),
        first_player_seat_id: spec.first_player_seat_id,
        participants: spec.participants,
        entropy: spec.entropy,
        selected: Some(SelectedRoot::from_reserved(reservation, selection)?),
    };
    prepare_root(root, source)
}
/// Construct an exact Commander opening from an independently admitted reservation.
/// The worker supplies the server-selected starter and entropy. The root keeps
/// the original selected entry IDs and revalidates the same deck during replay.
///
/// # Errors
/// Rejects mismatched format, release, roster, sections, deck rules, or
/// unavailable executable definitions. This root covers two or three seats,
/// with color identities determined by the selected cards.
pub fn prepare_reserved_commander_opening(
    context_id: Uuid,
    source: &NativeProgramSource,
    reservation: &NativeReservedGameSource,
    format: &crate::account::FormatSummary,
    first_player_seat_id: Uuid,
    entropy: Vec<NativeRevision>,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    use num_traits::ToPrimitive;
    let r = reservation.metadata();
    if format.key != "commander"
        || format.id != r.format_id
        || !r.game_index.is_zero()
        || !(2..=3).contains(&r.seats.len())
        || r.mode != crate::decks::PlayMode::Open
        || r.competitive
        || r.structure != crate::rooms::configuration::MatchStructure::Bo1
        || r.profile.planar_pool_ref.is_some()
        || !r
            .admission_qualification_ref
            .as_str()
            .strip_prefix("native-constructed-ffa-candidate-v1/commander/sha256:")
            .is_some_and(crate::source_protocol::valid_digest)
        || r.release.program_manifest_ref.as_str() != source.identity().as_str()
        || r.release.kernel_ref.as_str() != format!("sha256:{}", native_runtime_identity())
        || r.release.rules_ref.as_str() != hash_text(source.vocabulary.rules_sha256()).as_str()
    {
        return Err(NativeInitialError::ForeignSource);
    }
    let mut participants = Vec::new();
    let mut selection = Vec::new();
    for seat in &r.seats {
        if seat.controller_kind != crate::match_history::ControllerKind::Human {
            return Err(NativeInitialError::ForeignSource);
        }
        let mut cards = Vec::new();
        let mut entry_ids = Vec::new();
        for entry in &seat.entries {
            let zone = match entry.section.as_str() {
                "commander" => NativeInitialZone::Command,
                "main" => NativeInitialZone::Library,
                _ => return Err(NativeInitialError::ForeignSource),
            };
            let oracle = OracleId::try_from(entry.oracle_ref.as_str().to_owned())
                .map_err(|_| NativeInitialError::ForeignSource)?;
            let quantity = entry
                .quantity
                .integer()
                .to_usize()
                .ok_or(NativeInitialError::Invalid)?;
            if cards
                .len()
                .checked_add(quantity)
                .is_none_or(|count| count > 100)
            {
                return Err(NativeInitialError::ForeignSource);
            }
            cards.extend((0..quantity).map(|_| NativeInitialCard {
                oracle_id: oracle.clone(),
                zone,
            }));
            entry_ids.extend(std::iter::repeat_n(entry.entry_id, quantity));
        }
        participants.push(NativeInitialParticipant {
            seat_id: seat.seat_id,
            life: 40.into(),
            cards,
        });
        selection.push(NativeInitialSelection {
            seat_id: seat.seat_id,
            entry_ids,
        });
    }
    prepare_root(
        Root {
            version: COMMANDER_ROOT.into(),
            game_id: r.game_id,
            context_id,
            release_id: r.release.id,
            profile_id: r.profile.id,
            program_source_id: source.identity().clone(),
            first_player_seat_id,
            participants,
            entropy,
            selected: Some(SelectedRoot::from_reserved(reservation, selection)?),
        },
        source,
    )
}

fn prepare_root(
    root: Root,
    source: &NativeProgramSource,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    root.validate(source)?;
    let ids = root.ids();
    let bytes = serde_json::to_vec(&root).map_err(NativeInitialError::Json)?;
    if bytes.len() > MAX_ROOT_BYTES {
        return Err(NativeInitialError::Size);
    }
    let reference = NativeArtifactRef::for_bytes(
        ids.game_id,
        ids.context_id,
        NativeArtifactRole::Initial,
        &bytes,
    )?;
    let Initialized {
        candidate,
        roster,
        provenance,
    } = root.initialize(source)?;
    Ok(PreparedNativeOpening {
        bytes,
        reference,
        candidate,
        roster,
        provenance,
    })
}
/// Reconstruct from verified original bytes plus independently selected exact source.
/// # Errors
/// Rejects any original ref/source/header/roster substitution and actual native errors.
pub fn restore_initial(
    artifact: NativeArtifact,
    expected: &NativeInitialExpected,
    source: &NativeProgramSource,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    if artifact.reference() != expected.reference() {
        return Err(NativeInitialError::ForeignSource);
    }
    let reference = artifact.reference().clone();
    let bytes = artifact.into_bytes();
    if bytes.len() > MAX_ROOT_BYTES {
        return Err(NativeInitialError::Size);
    }
    let root: Root = crate::decode_wire_object(&bytes).map_err(NativeInitialError::Json)?;
    root.validate(source)?;
    let ids = root.ids();
    if ids.game_id != expected.ids.game_id
        || ids.context_id != expected.ids.context_id
        || ids.release_id != expected.ids.release_id
        || ids.profile_id != expected.ids.profile_id
        || root.roster() != expected.roster
    {
        return Err(NativeInitialError::ForeignSource);
    }
    let Initialized {
        candidate,
        roster,
        provenance,
    } = root.initialize(source)?;
    Ok(PreparedNativeOpening {
        bytes,
        reference,
        candidate,
        roster,
        provenance,
    })
}
/// Read the exact accepted Initial ref through private Store, then reconstruct it.
/// Expected tuple and source package must come from independent original authority.
/// # Errors
/// Rejects wrong/missing/corrupt bytes and all root/source/native construction errors.
pub fn load_initial(
    store: &NativeArtifactStore,
    expected: &NativeInitialExpected,
    source: &NativeProgramSource,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    let artifact = store.read(
        expected.reference(),
        expected.ids.game_id,
        expected.ids.context_id,
        NativeArtifactRole::Initial,
    )?;
    restore_initial(artifact, expected, source)
}

#[cfg(test)]
mod wire_baseline {
    use super::*;
    #[test]
    fn retained_predecessor_v1_wire_bytes_encode_identically() {
        // Generated by the exact pre-change Root encoder declaration, whose source
        // matches the retained predecessor SHA. This is a fixed wire vector only;
        // it does not establish source/runtime identity or initialize a game.
        let bytes = include_bytes!("../../../tests/fixtures/native-initial-v1-wire-golden.json");
        let root: Root = crate::decode_wire_object(bytes).unwrap();
        assert_eq!(root.version, "native-initial-v1");
        assert!(root.selected.is_none());
        assert_eq!(serde_json::to_vec(&root).unwrap(), bytes);
    }
}
