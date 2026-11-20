//! Source-backed mandatory-trigger trajectories over the shared native runner.
pub(super) mod data;
mod driver;
mod probes;
use super::{
    catalogue::{self, DefinitionInput, PhysicalRow, Shape},
    fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization},
    record::{self, RunReport},
};
use data::*;
pub(super) use driver::{FiniteIntent, FinitePolicy};
pub(super) use driver::{FiniteIntent as MandatoryIntent, FinitePolicy as MandatoryPolicy};
pub(super) use probes::reached_probes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_card_programs::Registry;
use spellwake_content::Inventory;
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use spellwake_engine::{
    combat::CombatRules,
    game::{GameContext, OpeningSetup},
    object_identity::CardId,
    priority::SeatId,
    setup::{GameSeed, StartingLife},
    shuffle::EntropyTape,
    turn::{Step, TurnStart},
};
use std::{collections::BTreeSet, path::Path, sync::Arc};
/// Closed reviewed case inventory. T10 names probes, not an extra trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum MandatoryCase {
    #[serde(rename = "MT01")]
    Mt01,
    #[serde(rename = "MT02")]
    Mt02,
    #[serde(rename = "T01.own-then-opponent")]
    T01OwnThenOpponent,
    #[serde(rename = "T01.opponent-active")]
    T01OpponentActive,
    #[serde(rename = "T02.own-main")]
    T02OwnMain,
    #[serde(rename = "T02.opponent-main")]
    T02OpponentMain,
    #[serde(rename = "T03.countered")]
    T03Countered,
    #[serde(rename = "T03.departed-recast")]
    T03DepartedRecast,
    #[serde(rename = "T04.one-card")]
    T04OneCard,
    #[serde(rename = "T04.empty")]
    T04Empty,
    #[serde(rename = "T04.one-life")]
    T04OneLife,
    #[serde(rename = "T04.empty-one-life")]
    T04EmptyOneLife,
    #[serde(rename = "T05.combat-dies")]
    T05CombatDies,
    #[serde(rename = "T05.bounce-not-dies")]
    T05BounceNotDies,
    #[serde(rename = "T06.dissenter-top")]
    T06DissenterTop,
    #[serde(rename = "T06.artist-top")]
    T06ArtistTop,
    #[serde(rename = "T06.split-targets")]
    T06SplitTargets,
    #[serde(rename = "T07.death-birth-entry")]
    T07DeathBirthEntry,
    #[serde(rename = "T08.self")]
    T08Self,
    #[serde(rename = "T08.opponent")]
    T08Opponent,
    #[serde(rename = "T09.fresh-next-turn")]
    T09FreshNextTurn,
    #[serde(rename = "T09.bounce-recast-stale")]
    T09BounceRecastStale,
    #[serde(rename = "T11.token-death-cessation")]
    T11TokenDeathCessation,
}
impl MandatoryCase {
    pub const ALL: [Self; 23] = [
        Self::Mt01,
        Self::Mt02,
        Self::T01OwnThenOpponent,
        Self::T01OpponentActive,
        Self::T02OwnMain,
        Self::T02OpponentMain,
        Self::T03Countered,
        Self::T03DepartedRecast,
        Self::T04OneCard,
        Self::T04Empty,
        Self::T04OneLife,
        Self::T04EmptyOneLife,
        Self::T05CombatDies,
        Self::T05BounceNotDies,
        Self::T06DissenterTop,
        Self::T06ArtistTop,
        Self::T06SplitTargets,
        Self::T07DeathBirthEntry,
        Self::T08Self,
        Self::T08Opponent,
        Self::T09FreshNextTurn,
        Self::T09BounceRecastStale,
        Self::T11TokenDeathCessation,
    ];
    pub const FULL_GAMES: [Self; 2] = [Self::Mt01, Self::Mt02];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mt01 => "MT01",
            Self::Mt02 => "MT02",
            Self::T01OwnThenOpponent => "T01.own-then-opponent",
            Self::T01OpponentActive => "T01.opponent-active",
            Self::T02OwnMain => "T02.own-main",
            Self::T02OpponentMain => "T02.opponent-main",
            Self::T03Countered => "T03.countered",
            Self::T03DepartedRecast => "T03.departed-recast",
            Self::T04OneCard => "T04.one-card",
            Self::T04Empty => "T04.empty",
            Self::T04OneLife => "T04.one-life",
            Self::T04EmptyOneLife => "T04.empty-one-life",
            Self::T05CombatDies => "T05.combat-dies",
            Self::T05BounceNotDies => "T05.bounce-not-dies",
            Self::T06DissenterTop => "T06.dissenter-top",
            Self::T06ArtistTop => "T06.artist-top",
            Self::T06SplitTargets => "T06.split-targets",
            Self::T07DeathBirthEntry => "T07.death-birth-entry",
            Self::T08Self => "T08.self",
            Self::T08Opponent => "T08.opponent",
            Self::T09FreshNextTurn => "T09.fresh-next-turn",
            Self::T09BounceRecastStale => "T09.bounce-recast-stale",
            Self::T11TokenDeathCessation => "T11.token-death-cessation",
        }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| format!("unknown mandatory-trigger case {value}"))
    }
    pub(super) const fn game_id(self) -> u128 {
        match self {
            Self::Mt01 => 5501,
            Self::Mt02 => 5502,
            Self::T01OwnThenOpponent => 5503,
            Self::T01OpponentActive => 5504,
            Self::T02OwnMain => 5505,
            Self::T02OpponentMain => 5506,
            Self::T03Countered => 5507,
            Self::T03DepartedRecast => 5508,
            Self::T04OneCard => 5509,
            Self::T04Empty => 5510,
            Self::T04OneLife => 5511,
            Self::T04EmptyOneLife => 5512,
            Self::T05CombatDies => 5513,
            Self::T05BounceNotDies => 5514,
            Self::T06DissenterTop => 5515,
            Self::T06ArtistTop => 5516,
            Self::T06SplitTargets => 5517,
            Self::T07DeathBirthEntry => 5518,
            Self::T08Self => 5519,
            Self::T08Opponent => 5520,
            Self::T09FreshNextTurn => 5521,
            Self::T09BounceRecastStale => 5522,
            Self::T11TokenDeathCessation => 5523,
        }
    }
}

impl Role {
    pub(in crate::ordinary_game) fn seat(self) -> SeatId {
        match self {
            Self::P0 => A,
            Self::P1 => B,
        }
    }
}
impl RecipeStep {
    pub(in crate::ordinary_game) fn native(self) -> Step {
        match self {
            Self::Untap => Step::Untap,
            Self::Upkeep => Step::Upkeep,
            Self::Draw => Step::Draw,
            Self::PrecombatMain => Step::PrecombatMain,
            Self::BeginningCombat => Step::BeginningCombat,
            Self::DeclareAttackers => Step::DeclareAttackers,
            Self::DeclareBlockers => Step::DeclareBlockers,
            Self::CombatDamage => Step::CombatDamage,
            Self::EndCombat => Step::EndCombat,
            Self::PostcombatMain => Step::PostcombatMain,
            Self::End => Step::End,
            Self::Cleanup => Step::Cleanup,
        }
    }
}
fn inputs() -> Result<Inputs, String> {
    let bytes = include_bytes!("mandatory-inputs-v1.json");
    if format!("{:x}", Sha256::digest(bytes))
        != "767b6f92ba29a49ab5f5de1694e2a6cfbfec1625eb0a41cfed068ef4a8aa100b"
    {
        return Err("compiled mandatory input authority mismatch".into());
    }
    let value: Inputs = serde_json::from_slice(bytes)
        .map_err(|e| format!("invalid compiled mandatory input: {e}"))?;
    if value.schema_version != 1
        || value.profile != "mandatory-trigger-v1"
        || value.cases.iter().map(|c| c.id).collect::<Vec<_>>() != MandatoryCase::ALL
        || value.definitions.len() != 19
        || value.seats.len() != 2
    {
        return Err("mandatory finite input domain mismatch".into());
    }
    for case in &value.cases {
        let names = case
            .instructions
            .iter()
            .map(Instruction::id)
            .collect::<BTreeSet<_>>();
        if names.len() != case.instructions.len()
            || case.budget.maximum_accepted_commands == 0
            || case.budget.maximum_callbacks == 0
            || !case.decision_plans.is_empty()
            || case.reached_probe_limit.is_some()
            || matches!(case.stop, Stop::BoundedAtStack { .. })
        {
            return Err("mandatory instruction identity/budget mismatch".into());
        }
        for group in &case.trigger_groups {
            if group
                .triggers
                .iter()
                .any(|trigger| trigger.caster.is_some() || trigger.event == TriggerEvent::Cast)
            {
                return Err("mandatory source excludes cast-trigger extensions".into());
            }
            let members = group
                .triggers
                .iter()
                .map(|t| t.name.as_str())
                .collect::<BTreeSet<_>>();
            if members.len() != group.triggers.len()
                || members
                    != group
                        .order_bottom_first
                        .iter()
                        .map(String::as_str)
                        .collect()
                || members.len() != group.order_bottom_first.len()
            {
                return Err("mandatory trigger order is not a complete permutation".into());
            }
        }
    }
    Ok(value)
}
impl<ScriptMana> Instruction<ScriptMana> {
    pub(in crate::ordinary_game) fn id(&self) -> &str {
        match self {
            Self::PlayLand { id, .. }
            | Self::ActivateMana { id, .. }
            | Self::CastUntargeted { id, .. }
            | Self::CastTargeted { id, .. }
            | Self::PassTo { id, .. }
            | Self::AdvanceReady { id, .. }
            | Self::Combat { id, .. }
            | Self::SaveObject { id, .. }
            | Self::ProbeMana { id, .. } => id,
        }
    }
}
/// Source-qualified selected definitions and their sole guarded Registry.
pub struct MandatorySource {
    inventory: Inventory,
    registry: Registry,
    inputs: Inputs,
    definitions: Vec<DefinitionInput>,
}
impl MandatorySource {
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// Load actual catalogue bytes and validate all selected source definitions.
    /// # Errors
    /// Rejects input, source, Registry or vocabulary mismatch without fallback.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, String> {
        let metadata_bytes =
            std::fs::read(metadata).map_err(|e| format!("mandatory metadata: {e}"))?;
        if format!("{:x}", Sha256::digest(&metadata_bytes))
            != "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad"
        {
            return Err("mandatory metadata authority mismatch".into());
        }
        let inventory = spellwake_content::snapshot::load(metadata, snapshot)
            .map_err(|e| format!("mandatory source: {e:?}"))?;
        let registry = Registry::from_mandatory_trigger_inventory(&inventory, vocabulary)
            .map_err(|e| format!("mandatory registry: {e}"))?;
        let inputs = inputs()?;
        let definitions: Vec<DefinitionInput> =
            serde_json::from_str(include_str!("mandatory-definitions-v1.json"))
                .map_err(|e| format!("mandatory definitions: {e}"))?;
        for (source, definition) in definitions.iter().zip(&inputs.definitions) {
            if source.name != definition.name
                || source.oracle_id != definition.oracle_id
                || source.scryfall_id != definition.printing_id
                || source.source_line_one_based != definition.source_line
                || source.raw_line_sha256_including_lf != definition.raw_line_sha256
                || source.registry_signature_v1_sha256 != definition.semantic_sha256
            {
                return Err("mandatory complete definition authority mismatch".into());
            }
        }
        if definitions.len() != 19
            || inventory.compressed_sha256
                != "c1c9dc28fa9a7a4bae61c02330ba5a1190205aab1a93361e590ff5d9a146af08"
            || registry.coverage().registry_version != 7
            || registry.coverage().creature_vocabulary_id != vocabulary.id()
        {
            return Err("mandatory source/registry/vocabulary mismatch".into());
        }
        Ok(Self {
            inventory,
            registry,
            inputs,
            definitions,
        })
    }
    /// Build exact physical input, then the declared supported native initialization.
    /// # Errors
    /// Rejects any alias, partition, printing, context or shape mismatch.
    pub fn fixture(
        &self,
        case: MandatoryCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        let recipe = self
            .inputs
            .cases
            .iter()
            .find(|c| c.id == case)
            .ok_or("mandatory case absent")?;
        let rows = recipe
            .inventory
            .iter()
            .enumerate()
            .map(|(i, r)| {
                Ok(PhysicalRow {
                    alias: r.alias.clone(),
                    card: CardId(u64::try_from(i).map_err(|_| "physical ordinal overflow")? + 1),
                    owner: r.owner.seat(),
                    oracle_id: r.oracle_id.clone(),
                    copy: u32::try_from(r.copy).map_err(|_| "copy ordinal overflow")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for row in &rows {
            let def = self
                .definitions
                .iter()
                .find(|d| d.oracle_id == row.oracle_id)
                .ok_or("foreign mandatory source")?;
            let role = match (&recipe.initialization, row.owner) {
                (Initialization::Opening { .. }, A) => "GW",
                (Initialization::Opening { .. }, _) => "BG",
                (_, A) => "P0",
                (_, _) => "P1",
            };
            if row.alias != format!("{role}.{}.{}", catalogue::slug(&def.name), row.copy) {
                return Err(format!(
                    "mandatory alias/copy/source mismatch: {}",
                    row.alias
                ));
            }
        }
        let shape = match &recipe.initialization {
            Initialization::Opening { decks, .. } => {
                if decks.len() != 2 || decks[0].role != Role::P0 || decks[1].role != Role::P1 {
                    return Err("mandatory ordinary deck roles mismatch".into());
                }
                for deck in decks {
                    if deck.card_count != 60
                        || deck.entries.len() != 11
                        || deck.entries.iter().map(|e| e.count).sum::<u64>() != 60
                        || deck
                            .entries
                            .iter()
                            .map(|e| &e.oracle_id)
                            .collect::<BTreeSet<_>>()
                            .len()
                            != 11
                    {
                        return Err("mandatory ordinary 11-entry deck shape mismatch".into());
                    }
                    for entry in &deck.entries {
                        if rows
                            .iter()
                            .filter(|r| {
                                r.owner == deck.role.seat() && r.oracle_id == entry.oracle_id
                            })
                            .count()
                            != usize::try_from(entry.count).map_err(|_| "deck count overflow")?
                        {
                            return Err("mandatory deck/physical count mismatch".into());
                        }
                    }
                }
                Shape::MandatoryDeckPair
            }
            Initialization::Scheduled { .. } => Shape::MandatoryPrepared,
        };
        let materialized = catalogue::materialize_rows(
            &self.inventory,
            &self.registry,
            &self.definitions,
            &rows,
            shape,
        )?;
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("mandatory-context-v1-{}", case.as_str()),
            release: "mandatory-trigger-v1".into(),
            rules: vocabulary.rules_revision().into(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: names.id(),
            catalogue: format!("oracle-sha256:{}", self.inventory.compressed_sha256),
            programs: "mandatory-trigger-v1:registry7".into(),
            profile: format!("mandatory-trigger-v1:{}", case.as_str()),
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
        .map_err(|e| format!("mandatory context: {e:?}"))?;
        let (active, life) = match &recipe.initialization {
            Initialization::Opening { starter, life, .. } => (
                *starter,
                vec![
                    StartingLife {
                        player: A,
                        total: integer(&life.p0)?,
                    },
                    StartingLife {
                        player: B,
                        total: integer(&life.p1)?,
                    },
                ],
            ),
            Initialization::Scheduled { active, life, .. } => (
                *active,
                vec![
                    StartingLife {
                        player: A,
                        total: integer(&life.p0)?,
                    },
                    StartingLife {
                        player: B,
                        total: integer(&life.p1)?,
                    },
                ],
            ),
        };
        let mut seed = GameSeed::with_uniform_life(
            vec![A, B],
            active.seat(),
            materialized.cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        );
        seed.life = life;
        let mut fixture = Fixture {
            case: CaseId::Mandatory(case),
            initialization: FixtureInitialization::Opening,
            game: spellwake_engine::game::GameId(case.game_id()),
            context,
            context_inputs,
            setup: OpeningSetup {
                seed,
                entropy: EntropyTape::new(vec![]),
            },
            aliases: materialized.aliases,
            definitions: materialized.definitions,
            shuffles: vec![],
        };
        match &recipe.initialization {
            Initialization::Opening {
                starter,
                shuffle_orders,
                keep_choices,
                ..
            } => {
                if shuffle_orders.len() != 2
                    || keep_choices.len() != 2
                    || keep_choices[0].role != *starter
                    || keep_choices[1].role == *starter
                    || keep_choices
                        .iter()
                        .any(|c| c.taken != 0 || c.choice != "keep_seven")
                {
                    return Err("mandatory opening declarations mismatch".into());
                }
                for role in [
                    *starter,
                    if *starter == Role::P0 {
                        Role::P1
                    } else {
                        Role::P0
                    },
                ] {
                    let orders = shuffle_orders
                        .iter()
                        .filter(|o| o.role == role)
                        .collect::<Vec<_>>();
                    let [order] = orders.as_slice() else {
                        return Err("mandatory shuffle role mismatch".into());
                    };
                    let before = fixture
                        .setup
                        .seed
                        .cards
                        .iter()
                        .filter(|c| c.owner == role.seat())
                        .map(|c| c.card)
                        .collect();
                    fixture.add_shuffle(
                        format!("{}.{role:?}.initial", case.as_str()),
                        role.seat(),
                        before,
                        &order.top_first,
                    )?;
                }
                fixture.setup.entropy = EntropyTape::new(
                    fixture
                        .shuffles
                        .iter()
                        .flat_map(|s| s.words.iter().copied())
                        .collect(),
                );
            }
            Initialization::Scheduled {
                turn,
                step,
                zones,
                battlefield_oldest_first,
                ..
            } => {
                let start = TurnStart {
                    number: *turn,
                    step: match step {
                        StartStep::Untap => Step::Untap,
                        StartStep::PrecombatMain => Step::PrecombatMain,
                    },
                };
                let prepared_zones = zones
                    .iter()
                    .map(|zone| catalogue::PreparedZone {
                        owner: zone.role.seat(),
                        hand: &zone.hand,
                        library_top_first: &zone.library_top_first,
                        graveyard_top_first: &zone.graveyard_top_first,
                        exile: &zone.exile,
                    })
                    .collect::<Vec<_>>();
                let prepared_battlefield = battlefield_oldest_first
                    .iter()
                    .map(|row| catalogue::PreparedBattlefield {
                        alias: &row.alias,
                        controller: row.controller.seat(),
                        tapped: row.tapped,
                    })
                    .collect::<Vec<_>>();
                let prepared = catalogue::prepare_legacy_scheduled(
                    catalogue::Materialized {
                        aliases: std::mem::take(&mut fixture.aliases),
                        cards: std::mem::take(&mut fixture.setup.seed.cards),
                        definitions: std::mem::take(&mut fixture.definitions),
                    },
                    catalogue::PreparedSetup {
                        start,
                        active: fixture.setup.seed.active,
                        life: std::mem::take(&mut fixture.setup.seed.life),
                        zones: &prepared_zones,
                        battlefield_oldest_first: &prepared_battlefield,
                        mana: std::mem::take(&mut fixture.setup.seed.mana),
                    },
                )
                .map_err(|error| match error {
                    catalogue::PreparedError::ParticipantPartition => {
                        "mandatory scheduled role partition mismatch".into()
                    }
                    catalogue::PreparedError::UnknownZoneAlias(alias)
                    | catalogue::PreparedError::UnknownBattlefieldAlias(alias) => {
                        format!("unknown fixture alias {alias}")
                    }
                    catalogue::PreparedError::DuplicateOrForeignZone => {
                        "mandatory duplicate/foreign zone member".into()
                    }
                    catalogue::PreparedError::UnsupportedBattlefield => {
                        "mandatory battlefield premise mismatch".into()
                    }
                    catalogue::PreparedError::IncompletePartition => {
                        "mandatory incomplete physical partition".into()
                    }
                    catalogue::PreparedError::MissingSeed => "mandatory seed absent".into(),
                    // Roles and materialize_rows already establish these resource/identity
                    // invariants. Keep any internal inconsistency explicit rather than bypass it.
                    error @ (catalogue::PreparedError::InvalidMana
                    | catalogue::PreparedError::InconsistentInventory) => error.strict_message(),
                })?;
                fixture.aliases = prepared.materialized.aliases;
                fixture.definitions = prepared.materialized.definitions;
                fixture.setup.seed.cards = prepared.materialized.cards;
                fixture.setup.seed.active = prepared.active;
                fixture.setup.seed.life = prepared.life;
                fixture.setup.seed.mana = prepared.mana;
                fixture.setup.seed.initial_battlefield_order = prepared.initial_battlefield_order;
                fixture.initialization = FixtureInitialization::Scheduled(prepared.start);
            }
        }
        Ok(fixture)
    }
    /// Execute each finite recipe through the shared recorder and saved-input rerun.
    /// # Errors
    /// Rejects empty selections or source/fixture errors; actual failed runs remain reports.
    pub fn run(
        &self,
        cases: &[MandatoryCase],
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<MandatoryReport, String> {
        if cases.is_empty() || cases.iter().copied().collect::<BTreeSet<_>>().len() != cases.len() {
            return Err("mandatory suite requires a nonempty unique case selection".into());
        }
        let mut runs = vec![];
        for (index, case) in cases.iter().enumerate() {
            runs.push(record::run_fixture_at(
                self.fixture(*case, vocabulary.clone())?,
                index,
            ));
        }
        Ok(MandatoryReport {
            schema_version: 1,
            profile: "mandatory-trigger-v1",
            registry_version: 7,
            catalogue_sha256: self.inventory.compressed_sha256.clone(),
            runs,
        })
    }
}
fn integer(value: &str) -> Result<RulesInteger, String> {
    RulesInteger::try_from(value.to_owned()).map_err(|e| format!("invalid mandatory integer: {e}"))
}
#[derive(Debug, Serialize)]
pub struct MandatoryReport {
    schema_version: u32,
    profile: &'static str,
    registry_version: u32,
    catalogue_sha256: String,
    pub runs: Vec<RunReport>,
}
impl MandatoryReport {
    pub fn completed(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(RunReport::completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ordinary_game::policy::Policy;
    use spellwake_engine::game::{GameInput, GameState};
    #[test]
    fn frozen_compiled_inputs_have_exact_closed_case_and_source_domains() {
        let inputs = inputs().unwrap();
        assert_eq!(inputs.cases.len(), 23);
        assert_eq!(inputs.definitions.len(), 19);
        assert_eq!(
            inputs
                .cases
                .iter()
                .filter(|c| matches!(c.initialization, Initialization::Opening { .. }))
                .count(),
            2
        );
        assert_eq!(
            inputs
                .cases
                .iter()
                .filter(|c| matches!(c.stop, Stop::BoundedReady { .. }))
                .count(),
            17
        );
    }
    #[test]
    fn strict_input_decoder_rejects_unknown_and_duplicate_fields() {
        let raw = include_str!("mandatory-inputs-v1.json");
        let unknown = raw.replacen(
            "\"schema_version\": 1,",
            "\"schema_version\": 1,\"foreign\":true,",
            1,
        );
        assert!(serde_json::from_str::<Inputs>(&unknown).is_err());
        let duplicate = raw.replacen(
            "\"schema_version\": 1,",
            "\"schema_version\": 1,\"schema_version\":1,",
            1,
        );
        assert!(serde_json::from_str::<Inputs>(&duplicate).is_err());
        let union = raw.replacen(
            "\"kind\": \"activate_mana\"",
            "\"kind\": \"activate_mana\",\"target\":{}",
            1,
        );
        assert!(serde_json::from_str::<Inputs>(&union).is_err());
    }
    #[test]
    #[ignore = "requires actual pinned catalogue and source-qualified vocabulary; direct native driver diagnostic, not ordinary report/capture qualification"]
    fn actual_native_driver_consumes_all_frozen_cases() {
        let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
        let dir = std::path::PathBuf::from(
            std::env::var_os("SPELLWAKE_MANDATORY_CATALOGUE_DIR").expect("explicit catalogue"),
        );
        let source = MandatorySource::load(
            &dir.join("oracle-metadata.json"),
            &dir.join("oracle-cards-20261029210158.jsonl.gz"),
            &vocabulary,
        )
        .unwrap();
        for case in MandatoryCase::ALL {
            let fixture = source.fixture(case, vocabulary.clone()).unwrap();
            let initial = match fixture.initialization {
                FixtureInitialization::Opening => GameState::initialize_opening(
                    fixture.game,
                    fixture.context.clone(),
                    fixture.setup.clone(),
                ),
                FixtureInitialization::Scheduled(start) => GameState::initialize_scheduled(
                    fixture.game,
                    fixture.context.clone(),
                    fixture.setup.seed.clone(),
                    start,
                ),
            }
            .unwrap();
            let mut state = initial.into_state();
            let mut policy = Policy::new(&fixture).unwrap();
            let mut saved: Vec<GameInput> = vec![];
            let mut probes = vec![];
            loop {
                let prepared = policy.prepare(&state).unwrap_or_else(|e| {
                    panic!("{} revision{}: {e}", case.as_str(), state.basis().revision)
                });
                for probe in
                    super::reached_probes(&fixture, &policy, &state, prepared.as_ref()).unwrap()
                {
                    let before = state.clone();
                    assert_eq!(
                        state.advance(probe.input.clone()),
                        Err(probe.expected.clone()),
                        "{} probe{}",
                        case.as_str(),
                        probe.id
                    );
                    assert_eq!(state, before);
                    probes.push((state.basis().revision, probe));
                }
                if let Some(prepared) = prepared {
                    let name = prepared.policy_entry_ref.clone();
                    let input = policy.issue(&state, prepared).unwrap();
                    let candidate = state.advance(input.clone()).unwrap_or_else(|e| {
                        panic!(
                            "{} revision{} {name}: {e:?}",
                            case.as_str(),
                            state.basis().revision
                        )
                    });
                    policy.accept(&input, &candidate).unwrap_or_else(|e| {
                        panic!(
                            "{} revision{} {name}: {e}",
                            case.as_str(),
                            state.basis().revision
                        )
                    });
                    state = candidate.into_state();
                    saved.push(input);
                } else {
                    break;
                }
            }
            policy
                .finish(&state)
                .unwrap_or_else(|e| panic!("{} finish: {e}", case.as_str()));
            for id in policy.required_recipe_probe_ids() {
                assert!(
                    probes.iter().any(|(_, p)| p.id == id),
                    "unexecuted recipe probe{id}"
                );
            }
            let mut replay = match fixture.initialization {
                FixtureInitialization::Opening => GameState::initialize_opening(
                    fixture.game,
                    fixture.context.clone(),
                    fixture.setup.clone(),
                ),
                FixtureInitialization::Scheduled(start) => GameState::initialize_scheduled(
                    fixture.game,
                    fixture.context.clone(),
                    fixture.setup.seed.clone(),
                    start,
                ),
            }
            .unwrap()
            .into_state();
            let mut replayed_probes = 0;
            for input in &saved {
                for (_, probe) in probes
                    .iter()
                    .filter(|(revision, _)| *revision == replay.basis().revision)
                {
                    let before = replay.clone();
                    assert_eq!(
                        replay.advance(probe.input.clone()),
                        Err(probe.expected.clone())
                    );
                    assert_eq!(replay, before);
                    replayed_probes += 1;
                }
                replay = replay.advance(input.clone()).unwrap().into_state();
            }
            for (_, probe) in probes
                .iter()
                .filter(|(revision, _)| *revision == replay.basis().revision)
            {
                let before = replay.clone();
                assert_eq!(
                    replay.advance(probe.input.clone()),
                    Err(probe.expected.clone())
                );
                assert_eq!(replay, before);
                replayed_probes += 1;
            }
            assert_eq!(probes.len(), replayed_probes);
            assert_eq!(state, replay);
            eprintln!(
                "{} direct native commands={} actual-probes={}",
                case.as_str(),
                saved.len(),
                probes.len()
            );
        }
    }
}

#[cfg(test)]
mod foreign_instance_tests {
    use super::*;
    use crate::ordinary_game::{policy::Policy, probes::RejectionProbe};
    use spellwake_engine::{
        effects::EffectError,
        game::{DecisionReply, GameCommand, GameError, GameInput, GameState, RuleFrame},
        occurrence::{TriggerError, TriggerInstanceId},
    };
    fn initialize(fixture: &Fixture) -> GameState {
        match fixture.initialization {
            FixtureInitialization::Opening => GameState::initialize_opening(
                fixture.game,
                fixture.context.clone(),
                fixture.setup.clone(),
            ),
            FixtureInitialization::Scheduled(start) => GameState::initialize_scheduled(
                fixture.game,
                fixture.context.clone(),
                fixture.setup.seed.clone(),
                start,
            ),
        }
        .unwrap()
        .into_state()
    }
    #[test]
    #[ignore = "actual separately source-bound foreign-instance mechanism, not an ordinary report or witness probe"]
    fn mandatory_foreign_trigger_instance_rejects_and_replays_with_real_foreign_handle() {
        let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
        let directory = std::path::PathBuf::from(
            std::env::var_os("SPELLWAKE_MANDATORY_CATALOGUE_DIR")
                .expect("explicit actual catalogue"),
        );
        let source = MandatorySource::load(
            &directory.join("oracle-metadata.json"),
            &directory.join("oracle-cards-20261029210158.jsonl.gz"),
            &vocabulary,
        )
        .unwrap();
        let foreign_fixture = source
            .fixture(MandatoryCase::T01OwnThenOpponent, vocabulary.clone())
            .unwrap();
        let mut foreign_state = initialize(&foreign_fixture);
        let mut foreign_policy = Policy::new(&foreign_fixture).unwrap();
        while let Some(prepared) = foreign_policy.prepare(&foreign_state).unwrap() {
            let input = foreign_policy.issue(&foreign_state, prepared).unwrap();
            let candidate = foreign_state.advance(input.clone()).unwrap();
            foreign_policy.accept(&input, &candidate).unwrap();
            foreign_state = candidate.into_state();
        }
        foreign_policy.finish(&foreign_state).unwrap();
        let observed = foreign_policy.mandatory().unwrap().observed_instances();
        assert_eq!(observed.len(), 5);
        let fixture = source
            .fixture(MandatoryCase::T02OwnMain, vocabulary)
            .unwrap();
        let mut state = initialize(&fixture);
        let mut policy = Policy::new(&fixture).unwrap();
        let mut inputs = vec![];
        let mut saved_probe: Option<(
            u64,
            RejectionProbe,
            TriggerInstanceId,
            Vec<TriggerInstanceId>,
        )> = None;
        while let Some(prepared) = policy.prepare(&state).unwrap() {
            if let Some(decision) = state.pending_decision()
                && let RuleFrame::Triggers(order) = decision.frame()
            {
                assert!(saved_probe.is_none());
                let nonmembers = observed
                    .iter()
                    .copied()
                    .filter(|i| !order.instances().contains(i))
                    .collect::<Vec<_>>();
                let [foreign] = nonmembers.as_slice() else {
                    panic!("foreign actual instance must be unique unequal nonmember")
                };
                let mut selected = order.instances().to_vec();
                selected[0] = *foreign;
                let probe = RejectionProbe {
                    id: "T10.foreign-trigger-instance".into(),
                    expectation_ref: "T10.foreign-trigger-instance".into(),
                    attachment: "separate-actual-test-only-foreign-case-instance".into(),
                    input: GameInput {
                        basis: state.basis().clone(),
                        actor: decision.chooser(),
                        command: GameCommand::Reply {
                            decision: decision.id(),
                            choice: DecisionReply::OrderTriggers(selected),
                        },
                    },
                    expected: GameError::Effect(EffectError::Trigger(
                        TriggerError::InvalidTriggerOrder,
                    )),
                };
                let before = state.clone();
                assert_eq!(
                    state.advance(probe.input.clone()),
                    Err(probe.expected.clone())
                );
                assert_eq!(state, before);
                saved_probe = Some((
                    state.basis().revision,
                    probe,
                    *foreign,
                    order.instances().to_vec(),
                ));
            }
            let input = policy.issue(&state, prepared).unwrap();
            let candidate = state.advance(input.clone()).unwrap();
            policy.accept(&input, &candidate).unwrap();
            state = candidate.into_state();
            inputs.push(input);
        }
        policy.finish(&state).unwrap();
        let (revision, probe, foreign, domain) = saved_probe.unwrap();
        let mut replay = initialize(&fixture);
        let mut rerun_probe = false;
        for input in &inputs {
            if replay.basis().revision == revision {
                let before = replay.clone();
                assert_eq!(
                    replay.advance(probe.input.clone()),
                    Err(probe.expected.clone())
                );
                assert_eq!(replay, before);
                rerun_probe = true;
            }
            replay = replay.advance(input.clone()).unwrap().into_state();
        }
        assert!(rerun_probe);
        assert_eq!(state, replay);
        if let Some(path) = std::env::var_os("SPELLWAKE_MANDATORY_FOREIGN_PROBE_RECEIPT") {
            let payload = serde_json::json!({"kind":"actual-test-only-mandatory-foreign-instance-root-probe","probe_id":probe.id,"ordinary_report_credit":false,"full_witness_credit":false,"forge_credit":false,"observed_foreign_case":foreign_fixture.case.as_str(),"target_case":fixture.case.as_str(),"foreign_instance":foreign.sequence(),"actual_target_domain":domain.iter().map(|i|i.sequence()).collect::<Vec<_>>(),"bound_revision":revision,"no_candidate":true,"whole_source_retained":true,"valid_continuation":true,"saved_probe_rerun":true,"final_state_equal":true});
            std::fs::write(path, serde_json::to_vec_pretty(&payload).unwrap()).unwrap();
        }
    }
}

#[cfg(test)]
mod scheduled_tests;
