//! Privileged authenticated source4 to real native opening bridge.
//! This has no historical report/capture, public host or complete-card contract.
pub mod prepared;
use super::{
    authenticated_source::{self, Family},
    catalogue::{self, DefinitionInput, PhysicalRow, Shape},
    fixture::{A, Alias, B, Definition},
};
use spellwake_card_programs::{
    BindingStatus, LIBRARY_TOP_PROFILE, LIBRARY_TOP_REGISTRY_VERSION, OracleId, Registry,
    RegistryError, Scope, TEACHING_SOURCE_SHA256,
};
use spellwake_content::Inventory;
use spellwake_contracts::{creature_types::CreatureTypeVocabulary, names::NameRules};
use spellwake_engine::{
    combat::CombatRules,
    game::{GameContext, GameError, GameId, GameState, NativeCandidate, OpeningSetup},
    object_identity::CardId,
    setup::GameSeed,
    shuffle::EntropyTape,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
const DEFINITIONS: &[u8] = include_bytes!("library_top/source-definitions-v1.json");
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
const OPT: &str = "713332c1-5bd8-400f-bfff-c1ca0697a043";
const CONSIDER: &str = "4c9bcba6-87b5-4fb3-97ee-6fe5b739337d";
const SCOUR: &str = "83101ba8-a569-4827-8c53-9ca0dfcd59a7";

/// Boundary failures retain the native Registry error and its underlying source.
#[derive(Debug)]
pub enum LibraryTopSourceError {
    /// Closed descriptor, authentication or physical inventory rejection.
    Source(String),
    /// File read failure with the original operating-system error.
    Io {
        stage: &'static str,
        error: std::io::Error,
    },
    /// The maintained snapshot decoder's original failure.
    Snapshot(spellwake_content::snapshot::SnapshotError),
    /// The native Registry's original typed source failure.
    Registry(RegistryError),
    /// Native context construction failure, retained without string erasure.
    Native(GameError),
}
impl std::fmt::Display for LibraryTopSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(message) => f.write_str(message),
            Self::Registry(error) => error.fmt(f),
            Self::Io { stage, error } => write!(f, "library-top {stage}: {error}"),
            Self::Snapshot(error) => write!(f, "library-top snapshot decode: {error:?}"),
            Self::Native(error) => write!(f, "library-top native context: {error:?}"),
        }
    }
}
impl std::error::Error for LibraryTopSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            Self::Io { error, .. } => Some(error),
            Self::Snapshot(spellwake_content::snapshot::SnapshotError::Operational(error)) => {
                Some(error)
            }
            Self::Source(_) | Self::Snapshot(_) | Self::Native(_) => None,
        }
    }
}
impl From<String> for LibraryTopSourceError {
    fn from(value: String) -> Self {
        Self::Source(value)
    }
}
impl From<RegistryError> for LibraryTopSourceError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

/// Complete immutable native source4. The vocabulary's byte provenance stays upstream.
pub struct LibraryTopSource {
    inventory: Inventory,
    registry: Registry,
    definitions: Vec<DefinitionInput>,
    vocabulary: Arc<CreatureTypeVocabulary>,
}
impl LibraryTopSource {
    /// Authenticate full saved bytes, exact LF rows and native source4 semantics.
    ///
    /// # Errors
    /// Rejects missing/foreign bytes, raw source tuples, native vocabulary or any
    /// complete source Registry guard. There is no synthetic fallback.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Self, LibraryTopSourceError> {
        let descriptor = authenticated_source::descriptor(DEFINITIONS, Family::LibraryTop)?;
        let metadata = std::fs::read(metadata).map_err(|error| LibraryTopSourceError::Io {
            stage: "metadata",
            error,
        })?;
        let snapshot = std::fs::read(snapshot).map_err(|error| LibraryTopSourceError::Io {
            stage: "snapshot",
            error,
        })?;
        authenticated_source::authenticate_bytes(
            &metadata,
            &snapshot,
            &descriptor,
            &vocabulary,
            Family::LibraryTop,
        )?;
        let inventory = spellwake_content::snapshot::decode(&metadata, snapshot.clone())
            .map_err(LibraryTopSourceError::Snapshot)?;
        let registry = Registry::from_library_top_inventory(&inventory, &vocabulary)?;
        authenticated_source::verify_raw_rows(
            &snapshot,
            &descriptor.definitions,
            Family::LibraryTop,
        )?;
        let source = Self {
            inventory,
            registry,
            definitions: descriptor.definitions,
            vocabulary,
        };
        source.validate()?;
        Ok(source)
    }
    /// Borrow the complete native Registry. All four bindings remain Partial.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    fn validate(&self) -> Result<(), LibraryTopSourceError> {
        // Reuse the native whole-inventory guard before any physical subset.
        Registry::from_library_top_inventory(&self.inventory, &self.vocabulary)?;
        let coverage = self.registry.coverage();
        if coverage.registry_version != LIBRARY_TOP_REGISTRY_VERSION
            || coverage.source_sha256 != TEACHING_SOURCE_SHA256
            || coverage.source_records != 39_196
            || coverage.creature_vocabulary_id != self.vocabulary.id()
            || coverage.required_records != 4
            || coverage.required_partial_bindings != 4
            || coverage.required_without_binding != 0
            || coverage.fully_qualified_cards != 0
            || coverage.behavior_evidence != "not-evaluated-by-registry"
            || coverage.cards.len() != 4
            || self.definitions.len() != 4
        {
            return Err("library-top complete native source4 coverage mismatch"
                .to_string()
                .into());
        }
        let mut seen = BTreeSet::new();
        for row in &self.definitions {
            let id = OracleId::try_from(row.oracle_id.clone()).map_err(|e| e.to_string())?;
            let bound = self
                .registry
                .resolve(&id)
                .ok_or_else(|| "library-top native binding missing".to_string())?;
            let card = coverage
                .cards
                .iter()
                .find(|c| c.oracle_id.as_deref() == Some(row.oracle_id.as_str()))
                .ok_or_else(|| "library-top coverage missing".to_string())?;
            if !seen.insert(&row.oracle_id)
                || !card.required
                || card.name != row.name
                || card.behavior_evidence != "not-evaluated-by-registry"
                || !matches!(&card.binding, BindingStatus::Partial { program, scope, semantic_sha256 }
                    if *scope == Scope::LibraryTopOrdinaryCardV1 && bound.scope() == *scope && bound.key() == *program && semantic_sha256 == &row.registry_signature_v1_sha256)
            {
                return Err(
                    "library-top exact native source identity/signature mismatch"
                        .to_string()
                        .into(),
                );
            }
        }
        Ok(())
    }
    /// Materialize two sixty-card teaching decks, then retain actual native opening inputs.
    /// Each spell count is zero through four; all remaining physical cards are Island.
    /// This is a teaching inventory contract, not format legality.
    ///
    /// # Errors
    /// Rejects any complete source guard, spell count above four, physical catalogue
    /// mismatch or native context shape error. Source validation precedes subset seeding.
    pub fn opening(
        &self,
        decks: [TeachingDeck; 2],
        entropy: EntropyTape,
    ) -> Result<LibraryTopOpening, LibraryTopSourceError> {
        self.validate()?;
        let mut rows = vec![];
        for (deck, owner, offset, prefix) in
            [(decks[0], A, 0_u64, "A"), (decks[1], B, 100_u64, "B")]
        {
            if [deck.opt, deck.consider, deck.thought_scour]
                .iter()
                .any(|count| *count > 4)
            {
                return Err("library-top teaching spell count exceeds four"
                    .to_string()
                    .into());
            }
            let islands =
                60 - u32::from(deck.opt) - u32::from(deck.consider) - u32::from(deck.thought_scour);
            let mut ordinal = 0;
            for source in &self.definitions {
                let count = match source.oracle_id.as_str() {
                    OPT => u32::from(deck.opt),
                    CONSIDER => u32::from(deck.consider),
                    SCOUR => u32::from(deck.thought_scour),
                    ISLAND => islands,
                    _ => {
                        return Err("library-top foreign compiled source identity"
                            .to_string()
                            .into());
                    }
                };
                for copy in 1..=count {
                    ordinal += 1;
                    rows.push(PhysicalRow {
                        alias: format!("{prefix}.{}.{copy}", catalogue::slug(&source.name)),
                        card: CardId(offset + ordinal),
                        owner,
                        oracle_id: source.oracle_id.clone(),
                        copy,
                    });
                }
            }
        }
        let materialized = catalogue::materialize_rows(
            &self.inventory,
            &self.registry,
            &self.definitions,
            &rows,
            Shape::LibraryTopDeckPair,
        )?;
        let names = Arc::new(NameRules::literal_names());
        let context = GameContext::new(
            LIBRARY_TOP_PROFILE.into(),
            self.vocabulary.rules_revision().into(),
            format!("oracle-sha256:{TEACHING_SOURCE_SHA256}"),
            "library-top-registry11".into(),
            "library-top-native-opening-v1".into(),
            self.vocabulary.id(),
            names.id(),
        )
        .map_err(LibraryTopSourceError::Native)?;
        let setup = OpeningSetup {
            seed: GameSeed::with_uniform_life(
                vec![A, B],
                A,
                materialized.cards,
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                Arc::clone(&self.vocabulary),
                names,
            ),
            entropy,
        };
        Ok(LibraryTopOpening {
            context,
            setup,
            aliases: materialized.aliases,
            definitions: materialized.definitions,
        })
    }
}
/// Finite teaching deck counts. The constructor fills the rest of sixty with Island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TeachingDeck {
    pub opt: u8,
    pub consider: u8,
    pub thought_scour: u8,
}
/// Source-qualified initial native input, retained for exact same-input replay.
/// No caller may replace its library-only seed through this immutable handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTopOpening {
    context: GameContext,
    setup: OpeningSetup,
    aliases: BTreeMap<CardId, Alias>,
    definitions: Vec<Definition>,
}
impl LibraryTopOpening {
    /// Initialize through the real native shuffle, draw and mulligan lifecycle.
    ///
    /// # Errors
    /// Returns native setup or entropy exhaustion failures without prepared fallback.
    pub fn initialize(&self, game: GameId) -> Result<NativeCandidate, GameError> {
        GameState::initialize_opening(game, self.context.clone(), self.setup.clone())
    }
    /// Borrow the exact source-qualified native context.
    pub fn context(&self) -> &GameContext {
        &self.context
    }
    /// Borrow the complete initial seed and actual initial entropy tape.
    pub fn setup(&self) -> &OpeningSetup {
        &self.setup
    }
    /// Borrow deterministic physical CardId and source-definition aliases.
    pub fn aliases(&self) -> &BTreeMap<CardId, Alias> {
        &self.aliases
    }
    /// Borrow complete native Programs for definitions physically used by the decks.
    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }
}
#[cfg(test)]
mod tests;
