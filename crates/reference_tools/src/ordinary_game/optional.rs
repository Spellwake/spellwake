//! Source-qualified optional/payment inputs over the shared ordinary fixture owner.
//! It admits source inputs and exposes finite trajectories through the shared
//! native policy and recorder. Registry bindings remain Partial.
// The frozen grammar deliberately retains primary policy/expected-value intent
// that later shared policy/record consumers will read. Those fields are required
// for strict source admission even when fixture construction does not inspect them.
#[allow(dead_code)]
mod data;
mod probes;
mod recipes;
mod registry_witness;
pub(super) use probes::reached_probes;
pub use recipes::OptionalReport;
pub(super) use recipes::recipe;

use super::{
    catalogue::{self, Materialized, PhysicalRow, Shape},
    fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization},
};
use data::{FullGameCase, Inputs, Role};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    BindingStatus, OPTIONAL_PAYMENT_PROFILE, OPTIONAL_PAYMENT_REGISTRY_VERSION, OracleId, Registry,
    Scope, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};
use spellwake_content::Inventory;
use spellwake_contracts::{creature_types::CreatureTypeVocabulary, names::NameRules};
use spellwake_engine::{
    combat::CombatRules,
    game::{GameContext, GameId, OpeningSetup},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{Instruction, ProgramKind},
    setup::GameSeed,
    shuffle::{EntropyTape, SHUFFLE_ALGORITHM},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

const INPUTS_SHA256: &str = "d7858e932aff0de4960a7abe848a87061a350c69523a8917543dbb61ff35d1c2";
const METADATA_SHA256: &str = "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad";
const OPENING_INTENT: &str =
    "shuffle both complete libraries in participant order, deal7 each; both Keep0; no bottom";

/// Closed source-backed full/reduced optional-payment case vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum OptionalCase {
    #[serde(rename = "OP-FG01")]
    Fg01,
    #[serde(rename = "OP-FG02")]
    Fg02,
    #[serde(rename = "OP-R01")]
    R01,
    #[serde(rename = "OP-R02")]
    R02,
    #[serde(rename = "OP-R03")]
    R03,
    #[serde(rename = "OP-R04")]
    R04,
    #[serde(rename = "OP-R05")]
    R05,
    #[serde(rename = "OP-R06")]
    R06,
    #[serde(rename = "OP-R07")]
    R07,
    #[serde(rename = "OP-R08")]
    R08,
    #[serde(rename = "OP-R09")]
    R09,
    #[serde(rename = "OP-R10")]
    R10,
    #[serde(rename = "OP-R11")]
    R11,
    #[serde(rename = "OP-R12")]
    R12,
    #[serde(rename = "OP-R13")]
    R13,
    #[serde(rename = "OP-R14")]
    R14,
    #[serde(rename = "OP-R15")]
    R15,
    #[serde(rename = "OP-R16")]
    R16,
    #[serde(rename = "OP-R17")]
    R17,
    #[serde(rename = "OP-R18")]
    R18,
    #[serde(rename = "OP-R19")]
    R19,
    #[serde(rename = "OP-R20")]
    R20,
    #[serde(rename = "OP-R21")]
    R21,
    #[serde(rename = "OP-R22")]
    R22,
    #[serde(rename = "OP-R23")]
    R23,
    #[serde(rename = "OP-R24")]
    R24,
}
impl OptionalCase {
    /// All finite cases; construction of reduced fixtures is a separate API.
    pub const ALL: [Self; 26] = [
        Self::Fg01,
        Self::Fg02,
        Self::R01,
        Self::R02,
        Self::R03,
        Self::R04,
        Self::R05,
        Self::R06,
        Self::R07,
        Self::R08,
        Self::R09,
        Self::R10,
        Self::R11,
        Self::R12,
        Self::R13,
        Self::R14,
        Self::R15,
        Self::R16,
        Self::R17,
        Self::R18,
        Self::R19,
        Self::R20,
        Self::R21,
        Self::R22,
        Self::R23,
        Self::R24,
    ];
    /// The two reviewed natural opening trajectories.
    pub const FULL_GAMES: [Self; 2] = [Self::Fg01, Self::Fg02];
    /// Stable source-case identity.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fg01 => "OP-FG01",
            Self::Fg02 => "OP-FG02",
            Self::R01 => "OP-R01",
            Self::R02 => "OP-R02",
            Self::R03 => "OP-R03",
            Self::R04 => "OP-R04",
            Self::R05 => "OP-R05",
            Self::R06 => "OP-R06",
            Self::R07 => "OP-R07",
            Self::R08 => "OP-R08",
            Self::R09 => "OP-R09",
            Self::R10 => "OP-R10",
            Self::R11 => "OP-R11",
            Self::R12 => "OP-R12",
            Self::R13 => "OP-R13",
            Self::R14 => "OP-R14",
            Self::R15 => "OP-R15",
            Self::R16 => "OP-R16",
            Self::R17 => "OP-R17",
            Self::R18 => "OP-R18",
            Self::R19 => "OP-R19",
            Self::R20 => "OP-R20",
            Self::R21 => "OP-R21",
            Self::R22 => "OP-R22",
            Self::R23 => "OP-R23",
            Self::R24 => "OP-R24",
        }
    }
    /// Whether the case has a reviewed complete opening recipe.
    pub const fn is_full_game(self) -> bool {
        matches!(self, Self::Fg01 | Self::Fg02)
    }
    /// Parse an exact finite case ID.
    ///
    /// # Errors
    /// Rejects unknown IDs rather than treating them as another case.
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|case| case.as_str() == value)
            .ok_or_else(|| format!("unknown optional-payment case {value}"))
    }
    pub(super) const fn game_id(self) -> u128 {
        match self {
            Self::Fg01 => 5601,
            Self::Fg02 => 5602,
            Self::R01 => 5603,
            Self::R02 => 5604,
            Self::R03 => 5605,
            Self::R04 => 5606,
            Self::R05 => 5607,
            Self::R06 => 5608,
            Self::R07 => 5609,
            Self::R08 => 5610,
            Self::R09 => 5611,
            Self::R10 => 5612,
            Self::R11 => 5613,
            Self::R12 => 5614,
            Self::R13 => 5615,
            Self::R14 => 5616,
            Self::R15 => 5617,
            Self::R16 => 5618,
            Self::R17 => 5619,
            Self::R18 => 5620,
            Self::R19 => 5621,
            Self::R20 => 5622,
            Self::R21 => 5623,
            Self::R22 => 5624,
            Self::R23 => 5625,
            Self::R24 => 5626,
        }
    }
}

impl Role {
    fn seat(self) -> SeatId {
        match self {
            Self::P0 => A,
            Self::P1 => B,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn decode_inputs(bytes: &[u8]) -> Result<Inputs, String> {
    if digest(bytes) != INPUTS_SHA256 {
        return Err("optional-payment compiled input authority mismatch".into());
    }
    let inputs: Inputs = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid compiled optional-payment input: {error}"))?;
    validate_input_shape(&inputs)?;
    Ok(inputs)
}
fn inputs() -> Result<Inputs, String> {
    decode_inputs(include_bytes!("optional-inputs-v1.json"))
}
fn card_id(value: &str) -> Result<CardId, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| "invalid optional physical CardId")?;
    if parsed == 0 || parsed.to_string() != value {
        return Err("noncanonical optional physical CardId".into());
    }
    Ok(CardId(parsed))
}
fn physical_rows(inputs: &Inputs) -> Result<Vec<PhysicalRow>, String> {
    inputs
        .physical_inventory
        .iter()
        .map(|row| {
            if row.owner != u64::from(row.role.seat().0) {
                return Err("optional physical role/owner mismatch".into());
            }
            let source = inputs
                .definitions
                .iter()
                .find(|source| source.oracle_id == row.oracle_id)
                .ok_or("optional physical source missing")?;
            if row.name != source.name || !row.alias.starts_with(&format!("{}.", row.role.as_str()))
            {
                return Err("optional physical alias/name/source mismatch".into());
            }
            Ok(PhysicalRow {
                alias: row.alias.clone(),
                card: card_id(&row.card_id)?,
                owner: row.role.seat(),
                oracle_id: row.oracle_id.clone(),
                copy: row.copy,
            })
        })
        .collect()
}

fn validate_input_shape(inputs: &Inputs) -> Result<(), String> {
    let e = &inputs.epochs;
    if inputs.schema_version != 1
        || !matches!(inputs.profile, data::Profile::OptionalPayment)
        || (
            e.registry,
            e.report,
            e.aggregate,
            e.current,
            e.witness,
            e.record_codec,
            e.vocabulary,
        ) != (8, 4, 1, 10, 8, 2, 1)
        || inputs.definitions.len() != 19
        || inputs.decks.len() != 2
        || inputs.physical_inventory.len() != 120
        || inputs.token_templates.len() != 2
        || inputs.authorities.len() != 4
    {
        return Err("optional compiled profile/header/shape mismatch".into());
    }
    let sources = inputs
        .definitions
        .iter()
        .map(|d| d.oracle_id.as_str())
        .collect::<BTreeSet<_>>();
    if sources.len() != 19
        || inputs
            .definitions
            .iter()
            .map(|d| d.scryfall_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != 19
        || inputs
            .authorities
            .iter()
            .map(|a| a.path.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != 4
        || inputs
            .authorities
            .iter()
            .any(|a| a.bytes == 0 || !is_digest(&a.sha256) || a.path.is_empty())
    {
        return Err("optional duplicate or invalid source authority".into());
    }
    for source in &inputs.definitions {
        if source.name.is_empty()
            || source.source_line_one_based == 0
            || !is_digest(&source.raw_line_sha256_including_lf)
            || !is_digest(&source.registry_signature_v1_sha256)
            || source.set.is_empty()
            || source.collector.is_empty()
            || source.language != "en"
        {
            return Err("optional incomplete source definition".into());
        }
    }
    let rows = physical_rows(inputs)?;
    let mut aliases = BTreeSet::new();
    let mut cards = BTreeSet::new();
    if rows
        .iter()
        .any(|row| !aliases.insert(row.alias.as_str()) || !cards.insert(row.card))
    {
        return Err("optional duplicate physical alias or CardId".into());
    }
    for (index, role) in [Role::P0, Role::P1].into_iter().enumerate() {
        let deck = &inputs.decks[index];
        let owned = rows
            .iter()
            .filter(|row| row.owner == role.seat())
            .collect::<Vec<_>>();
        if deck.role != role
            || deck.card_count != 60
            || deck.entries.len() != 11
            || !deck.sideboard.is_empty()
            || owned.len() != 60
            || deck.entries.iter().any(|entry| entry.count == 0)
            || deck
                .entries
                .iter()
                .map(|entry| u64::from(entry.count))
                .sum::<u64>()
                != 60
            || deck
                .entries
                .iter()
                .map(|entry| entry.oracle_id.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != 11
        {
            return Err("optional 60-card/eleven-entry deck mismatch".into());
        }
        for (ordinal, row) in owned.iter().enumerate() {
            let base = if role == Role::P0 { 10_001 } else { 20_001 };
            if row.card.0
                != base + u64::try_from(ordinal).map_err(|_| "physical ordinal overflow")?
            {
                return Err("optional exact physical CardId order mismatch".into());
            }
        }
        for entry in &deck.entries {
            let source = inputs
                .definitions
                .iter()
                .find(|source| source.oracle_id == entry.oracle_id)
                .ok_or("optional deck source missing")?;
            if source.name != entry.name
                || owned
                    .iter()
                    .filter(|row| row.oracle_id == entry.oracle_id)
                    .count()
                    != usize::try_from(entry.count).map_err(|_| "optional deck count overflow")?
            {
                return Err("optional deck/physical/source count mismatch".into());
            }
        }
    }
    let used = rows
        .iter()
        .map(|row| row.oracle_id.as_str())
        .collect::<BTreeSet<_>>();
    let expected_main = sources
        .iter()
        .copied()
        .filter(|id| *id != spellwake_card_programs::TWINCAST)
        .collect::<BTreeSet<_>>();
    if used.len() != 18 || used != expected_main {
        return Err("optional main18/reduced-only Twincast domain mismatch".into());
    }
    let full = &inputs.full_game_recipes;
    if full.participants.len() != 2
        || full.participants[0].role != Role::P0
        || full.participants[1].role != Role::P1
        || full.participants[0].seat != u64::from(A.0)
        || full.participants[1].seat != u64::from(B.0)
        || full.first_player != Role::P0
        || full.life_each != "20"
        || full.opening != OPENING_INTENT
        || full.cases.len() != 2
        || full.cases[0].id != OptionalCase::Fg01
        || full.cases[1].id != OptionalCase::Fg02
    {
        return Err("optional reviewed opening/participant/case mismatch".into());
    }
    for recipe in &full.cases {
        validate_full_recipe(inputs, recipe)?;
    }
    Ok(())
}

fn validate_full_recipe(inputs: &Inputs, recipe: &FullGameCase) -> Result<(), String> {
    if !recipe.id.is_full_game()
        || recipe.decks.len() != 2
        || recipe.global_entropy_words.len() != 118
        || recipe.budgets.accepted_inputs_max != 4096
        || recipe.budgets.reached_rejection_probes_max != 256
        || recipe.budgets.global_turns_max != recipe.primary_expected.terminal_global_turn
        || u64::try_from(recipe.turns.len()).map_err(|_| "optional turn count overflow")?
            != recipe.budgets.global_turns_max
    {
        return Err("optional full-game recipe/budget mismatch".into());
    }
    for (index, role) in [Role::P0, Role::P1].into_iter().enumerate() {
        let deck = &recipe.decks[index];
        let physical = inputs
            .physical_inventory
            .iter()
            .filter(|row| row.role == role)
            .collect::<Vec<_>>();
        if deck.role != role
            || deck.physical_cards.len() != 60
            || deck.before_card_ids.len() != 60
            || deck.after_card_ids.len() != 60
            || deck.after_aliases_top_first.len() != 60
            || deck.opening_hand_aliases.len() != 7
            || deck.entropy_words.len() != 59
            || deck.entropy_words_consumed != 59
            || deck.algorithm != SHUFFLE_ALGORITHM
        {
            return Err("optional physical shuffle input shape mismatch".into());
        }
        for (source, row) in physical.iter().zip(&deck.physical_cards) {
            if row.alias != source.alias
                || row.card_id != source.card_id
                || row.owner != source.owner
                || row.name != source.name
                || row.oracle_id != source.oracle_id
            {
                return Err("optional case/shared physical inventory mismatch".into());
            }
        }
        if deck
            .before_card_ids
            .iter()
            .map(String::as_str)
            .ne(physical.iter().map(|row| row.card_id.as_str()))
        {
            return Err("optional initial Library card order mismatch".into());
        }
        let before = deck
            .before_card_ids
            .iter()
            .map(|value| card_id(value))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let after = deck
            .after_card_ids
            .iter()
            .map(|value| card_id(value))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if before.len() != 60
            || after != before
            || deck.opening_hand_aliases.as_slice() != &deck.after_aliases_top_first[..7]
        {
            return Err("optional full Library permutation/opening hand mismatch".into());
        }
        for (card, alias) in deck
            .after_card_ids
            .iter()
            .zip(&deck.after_aliases_top_first)
        {
            if !physical
                .iter()
                .any(|row| row.card_id == *card && row.alias == *alias)
            {
                return Err("optional after CardId/alias order mismatch".into());
            }
        }
    }
    if recipe
        .decks
        .iter()
        .flat_map(|deck| deck.entropy_words.iter())
        .copied()
        .collect::<Vec<_>>()
        != recipe.global_entropy_words
    {
        return Err("optional participant-ordered entropy input mismatch".into());
    }
    Ok(())
}

fn validate_registry_against_inputs(
    registry: &Registry,
    vocabulary_id: &str,
    inputs: &Inputs,
) -> Result<(), String> {
    let coverage = registry.coverage();
    if vocabulary_id != TEACHING_VOCABULARY_ID
        || coverage.creature_vocabulary_id.to_string() != vocabulary_id
        || coverage.registry_version != OPTIONAL_PAYMENT_REGISTRY_VERSION
        || coverage.source_sha256 != TEACHING_SOURCE_SHA256
        || coverage.source_records != 39_196
        || coverage.cards.len() != 19
        || coverage.required_records != 19
        || coverage.required_partial_bindings != 19
        || coverage.required_without_binding != 0
        || coverage.fully_qualified_cards != 0
        || coverage.behavior_evidence != "not-evaluated-by-registry"
    {
        return Err("optional witness source/registry/vocabulary scope mismatch".into());
    }
    let mut actual = BTreeMap::new();
    for card in &coverage.cards {
        let oracle = card
            .oracle_id
            .as_deref()
            .ok_or("optional witness Oracle ID missing")?;
        if actual.insert(oracle, card).is_some() {
            return Err("optional witness duplicate Oracle ID".into());
        }
    }
    for source in &inputs.definitions {
        let card = actual
            .get(source.oracle_id.as_str())
            .ok_or("optional witness source identity missing")?;
        let id = OracleId::try_from(source.oracle_id.clone())
            .map_err(|e| format!("optional source Oracle ID: {e}"))?;
        let bound = registry
            .resolve(&id)
            .ok_or("optional native binding missing")?;
        let BindingStatus::Partial {
            program,
            scope,
            semantic_sha256,
        } = &card.binding
        else {
            return Err("optional witness binding is not Partial".into());
        };
        if card.name != source.name
            || !card.required
            || *scope != Scope::OptionalPaymentOrdinaryCardV1
            || bound.scope() != *scope
            || bound.key() != *program
            || semantic_sha256 != &source.registry_signature_v1_sha256
            || card.behavior_evidence != "not-evaluated-by-registry"
        {
            return Err("optional witness exact native source binding mismatch".into());
        }
    }
    Ok(())
}

/// Validate a typed guarded Registry8 witness against the final compiled source19.
/// Actual source-byte authentication remains with `OptionalSource::load`.
///
/// # Errors
/// Rejects a foreign registry/vocabulary, non-Partial coverage or any changed
/// source/native signature relationship. Pending/foreign compiled inputs reject.
pub(crate) fn validate_registry_witness(
    registry: &Registry,
    vocabulary_id: &str,
) -> Result<(), String> {
    validate_registry_against_inputs(registry, vocabulary_id, &inputs()?)
}

fn validate_native_templates(
    materialized: &Materialized,
    inputs: &Inputs,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), String> {
    for definition in &materialized.definitions {
        definition
            .program
            .validate(vocabulary)
            .map_err(|e| format!("optional native definition admission: {e:?}"))?;
    }
    let mut coordinates = BTreeSet::new();
    for reference in &inputs.token_templates {
        if !coordinates.insert((
            reference.source_oracle_id.as_str(),
            reference.triggered_ability_id,
            reference.instruction,
            reference.creation,
        )) {
            return Err("optional duplicate token template reference".into());
        }
        let program = &materialized
            .definitions
            .iter()
            .find(|d| d.oracle_id == reference.source_oracle_id)
            .ok_or("optional materialized token source missing")?
            .program;
        let body = match reference.triggered_ability_id {
            Some(id) => {
                let matching = program
                    .triggered
                    .iter()
                    .filter(|ability| ability.id.0 == id)
                    .collect::<Vec<_>>();
                let [ability] = matching.as_slice() else {
                    return Err("optional token trigger reference mismatch".into());
                };
                &ability.body
            }
            None => match &program.kind {
                ProgramKind::Spell(spell) => &spell.body,
                ProgramKind::Permanent(_) => {
                    return Err("optional token spell reference is not a spell".into());
                }
            },
        };
        let Some(Instruction::CreateTokens { creations }) =
            body.instructions.get(reference.instruction)
        else {
            return Err("optional token instruction reference mismatch".into());
        };
        let creation = creations
            .get(reference.creation)
            .ok_or("optional token creation reference mismatch")?;
        creation
            .definition
            .validate(vocabulary)
            .map_err(|e| format!("optional token template admission: {e:?}"))?;
    }
    Ok(())
}

/// Actual full saved inventory, immutable guarded Registry8 and materialized inputs.
/// This source owner constructs input fixtures and carries no native game result.
pub struct OptionalSource {
    inventory: Inventory,
    registry: Registry,
    inputs: Inputs,
    materialized: Materialized,
}
impl OptionalSource {
    /// Actual full saved inventory admitted by the loader.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    /// Immutable native Registry8 and its Partial coverage witness.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// Load the exact full metadata/snapshot and final strict native-bound input.
    ///
    /// # Errors
    /// Rejects foreign or unavailable source bytes, metadata, vocabulary or input,
    /// any native source/physical/template mismatch, and malformed compiled data.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, String> {
        let metadata_bytes =
            std::fs::read(metadata).map_err(|e| format!("optional metadata: {e}"))?;
        if digest(&metadata_bytes) != METADATA_SHA256 {
            return Err("optional metadata authority mismatch".into());
        }
        let snapshot_bytes =
            std::fs::read(snapshot).map_err(|e| format!("optional snapshot: {e}"))?;
        let inventory = spellwake_content::snapshot::decode(&metadata_bytes, snapshot_bytes)
            .map_err(|e| format!("optional source: {e:?}"))?;
        if inventory.source_records != 39_196 || inventory.cards.len() != 39_196 {
            return Err("optional full catalogue record count mismatch".into());
        }
        let registry = Registry::from_optional_payment_inventory(&inventory, vocabulary)
            .map_err(|e| format!("optional registry: {e}"))?;
        let inputs = inputs()?;
        validate_registry_against_inputs(&registry, &vocabulary.id().to_string(), &inputs)?;
        let materialized = catalogue::materialize_rows(
            &inventory,
            &registry,
            &inputs.definitions,
            &physical_rows(&inputs)?,
            Shape::OptionalPaymentDeckPair,
        )?;
        validate_native_templates(&materialized, &inputs, vocabulary)?;
        Ok(Self {
            inventory,
            registry,
            inputs,
            materialized,
        })
    }
    /// Construct one reviewed complete opening fixture from its source inputs.
    /// Keep0 remains a later actual policy reply; no native game is executed here.
    ///
    /// # Errors
    /// Rejects every reduced case, a foreign vocabulary, or any physical/order/
    /// entropy mismatch. This API never substitutes an opening for a reduced case.
    pub fn full_fixture(
        &self,
        case: OptionalCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        if !case.is_full_game() {
            return Err("optional reduced case is outside the full opening fixture API".into());
        }
        if vocabulary.id() != self.registry.coverage().creature_vocabulary_id {
            return Err("optional fixture vocabulary mismatch".into());
        }
        let recipe = self
            .inputs
            .full_game_recipes
            .cases
            .iter()
            .find(|recipe| recipe.id == case)
            .ok_or("optional full recipe missing")?;
        let names = Arc::new(NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("optional-context-v1-{}", case.as_str()),
            release: OPTIONAL_PAYMENT_PROFILE.into(),
            rules: vocabulary.rules_revision().into(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: names.id(),
            catalogue: format!("oracle-sha256:{}", self.inventory.compressed_sha256),
            programs: "optional-payment-v1:registry8".into(),
            profile: format!("optional-payment-v1:{}", case.as_str()),
        };
        let context = GameContext::new(
            context_inputs.release.clone(),
            context_inputs.rules.clone(),
            context_inputs.catalogue.clone(),
            context_inputs.programs.clone(),
            context_inputs.profile.clone(),
            vocabulary.id(),
            names.id(),
        )
        .map_err(|e| format!("optional context: {e:?}"))?;
        let mut fixture = Fixture {
            case: CaseId::Optional(case),
            initialization: FixtureInitialization::Opening,
            game: GameId(case.game_id()),
            context,
            context_inputs,
            setup: OpeningSetup {
                seed: GameSeed::with_uniform_life(
                    vec![A, B],
                    A,
                    self.materialized.cards.clone(),
                    vec![],
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    vocabulary,
                    names,
                ),
                entropy: EntropyTape::new(vec![]),
            },
            aliases: self.materialized.aliases.clone(),
            definitions: self.materialized.definitions.clone(),
            shuffles: vec![],
        };
        if fixture
            .setup
            .seed
            .cards
            .iter()
            .any(|card| card.destination != ObjectDestination::LibraryBottom)
        {
            return Err("optional opening contains a non-Library seed".into());
        }
        for deck in &recipe.decks {
            let before = fixture
                .setup
                .seed
                .cards
                .iter()
                .filter(|card| card.owner == deck.role.seat())
                .map(|card| card.card)
                .collect::<Vec<_>>();
            let declared_before = deck
                .before_card_ids
                .iter()
                .map(|value| card_id(value))
                .collect::<Result<Vec<_>, _>>()?;
            if before != declared_before {
                return Err("optional native seed Library order mismatch".into());
            }
            fixture.add_shuffle(
                format!("{}.{}.initial", case.as_str(), deck.role.as_str()),
                deck.role.seat(),
                before,
                &deck.after_aliases_top_first,
            )?;
            let actual = fixture
                .shuffles
                .last()
                .ok_or("optional declared shuffle missing")?;
            let target = deck
                .after_card_ids
                .iter()
                .map(|value| card_id(value))
                .collect::<Result<Vec<_>, _>>()?;
            if actual.target != target || actual.words != deck.entropy_words {
                return Err("optional reviewed shuffle/entropy recipe mismatch".into());
            }
        }
        if fixture
            .shuffles
            .iter()
            .flat_map(|s| s.words.iter())
            .copied()
            .collect::<Vec<_>>()
            != recipe.global_entropy_words
        {
            return Err("optional complete entropy recipe mismatch".into());
        }
        fixture.setup.entropy = EntropyTape::new(recipe.global_entropy_words.clone());
        Ok(fixture)
    }
}
