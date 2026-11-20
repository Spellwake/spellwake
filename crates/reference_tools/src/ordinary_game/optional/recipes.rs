//! Finite source-input lowering over the shared catalogue, policy and recorder.
//! The compiled recipe pin binds the independently reviewed source-input epoch.
use super::{OptionalCase, OptionalSource, validate_native_templates};
use crate::ordinary_game::{
    catalogue::{self, PhysicalRow, Shape},
    fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization},
    mandatory::data::{Case, Initialization, Inputs, Role, StartStep},
    record::{self, RunReport},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary, names::NameRules};
use spellwake_engine::{
    combat::CombatRules,
    game::{GameContext, GameId, OpeningSetup},
    object_identity::CardId,
    setup::{GameSeed, StartingLife},
    shuffle::EntropyTape,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

// Frozen v2 changes only the two reviewed R04 causal-token subject references.
const RECIPES_SHA256: &str = "ce084e897cd3e6b3af4412ab6d431e600642a7d6b1c0bf89496470f698fab46e";

fn recipes() -> Result<Inputs<OptionalCase>, String> {
    let bytes = include_bytes!("../optional-recipes-v2.json");
    if format!("{:x}", Sha256::digest(bytes)) != RECIPES_SHA256 {
        return Err("optional finite recipe authority mismatch".into());
    }
    let recipes: Inputs<OptionalCase> = serde_json::from_slice(bytes)
        .map_err(|error| format!("optional finite recipe syntax: {error}"))?;
    let epoch = &recipes.epochs;
    if recipes.schema_version != 1
        || recipes.profile != "optional-payment-v1"
        || (
            epoch.registry,
            epoch.report,
            epoch.aggregate,
            epoch.current,
            epoch.witness,
            epoch.record_codec,
            epoch.vocabulary,
        ) != (8, 4, 1, 10, 8, 2, 1)
        || recipes.cases.iter().map(|case| case.id).collect::<Vec<_>>() != OptionalCase::ALL
        || recipes.seats.len() != 2
        || recipes.authorities.len() != 5
        || recipes.seats[0].role != Role::P0
        || recipes.seats[0].native_seat != u64::from(A.0)
        || recipes.seats[1].role != Role::P1
        || recipes.seats[1].native_seat != u64::from(B.0)
        || recipes.seats[0].ordinary_role != "P0"
        || recipes.seats[1].ordinary_role != "P1"
    {
        return Err("optional finite recipe profile/case/seat/epoch mismatch".into());
    }
    let source = super::inputs()?;
    if recipes.definitions.len() != source.definitions.len() {
        return Err("optional finite recipe source domain mismatch".into());
    }
    for (declared, actual) in recipes.definitions.iter().zip(&source.definitions) {
        if declared.name != actual.name
            || declared.oracle_id != actual.oracle_id
            || declared.printing_id != actual.scryfall_id
            || declared.source_line != actual.source_line_one_based
            || declared.raw_line_sha256 != actual.raw_line_sha256_including_lf
            || declared.semantic_sha256 != actual.registry_signature_v1_sha256
        {
            return Err("optional finite recipe differs from native-bound source19".into());
        }
    }
    let source_receipts = recipes
        .authorities
        .iter()
        .map(|authority| &authority.name)
        .collect::<BTreeSet<_>>();
    if source_receipts.len() != recipes.authorities.len()
        || recipes
            .authorities
            .iter()
            .any(|authority| authority.path.is_empty() || !super::is_digest(&authority.sha256))
    {
        return Err("optional finite recipe source receipts malformed".into());
    }
    for case in &recipes.cases {
        if case.inventory.is_empty()
            || case.inventory.len() > 120
            || case.instructions.is_empty()
            || case
                .instructions
                .iter()
                .map(|instruction| instruction.id())
                .collect::<BTreeSet<_>>()
                .len()
                != case.instructions.len()
            || case.reached_probe_limit != Some(if case.id.is_full_game() { 256 } else { 128 })
            || case.id.is_full_game()
                != matches!(case.initialization, Initialization::Opening { .. })
        {
            return Err("optional finite recipe physical/instruction/budget shape mismatch".into());
        }
    }
    Ok(recipes)
}

pub(in crate::ordinary_game) fn recipe(case: OptionalCase) -> Result<Case<CaseId>, String> {
    recipes()?
        .cases
        .into_iter()
        .find(|candidate| candidate.id == case)
        .map(|candidate| candidate.map_id(CaseId::Optional))
        .ok_or_else(|| "optional finite recipe missing".into())
}

fn integer(value: &str) -> Result<RulesInteger, String> {
    RulesInteger::try_from(value.to_owned())
        .map_err(|error| format!("optional initial life: {error}"))
}

impl OptionalSource {
    /// Construct the exact declared opening or reduced physical input.
    /// No game state, stack entry, choice, mana or result is fabricated here.
    ///
    /// # Errors
    /// Rejects a foreign vocabulary, malformed recipe/source/partition, duplicate
    /// physical member, or mismatch with the independently retained opening data.
    pub fn fixture(
        &self,
        case: OptionalCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        if vocabulary.id() != self.registry.coverage().creature_vocabulary_id {
            return Err("optional prepared vocabulary mismatch".into());
        }
        let recipe = recipe(case)?;
        self.fixture_from_recipe(case, vocabulary, &recipe)
    }

    fn fixture_from_recipe(
        &self,
        case: OptionalCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
        recipe: &Case<CaseId>,
    ) -> Result<Fixture, String> {
        if case.is_full_game() {
            let fixture = self.full_fixture(case, vocabulary)?;
            let Initialization::Opening {
                starter,
                life,
                decks,
                shuffle_orders,
                keep_choices,
            } = &recipe.initialization
            else {
                return Err("optional full recipe requires an opening".into());
            };
            if *starter != Role::P0
                || life.p0 != "20"
                || life.p1 != "20"
                || decks.len() != 2
                || shuffle_orders.len() != 2
                || keep_choices.len() != 2
                || keep_choices[0].role != Role::P0
                || keep_choices[1].role != Role::P1
                || keep_choices
                    .iter()
                    .any(|choice| choice.taken != 0 || choice.choice != "keep_seven")
                || recipe.inventory.len() != 120
                || recipe
                    .inventory
                    .iter()
                    .map(|row| row.alias.as_str())
                    .collect::<BTreeSet<_>>()
                    != fixture
                        .aliases
                        .values()
                        .map(|alias| alias.alias.as_str())
                        .collect::<BTreeSet<_>>()
            {
                return Err("optional lowered opening declaration mismatch".into());
            }
            for row in &recipe.inventory {
                let card = fixture.card(&row.alias)?;
                let alias = fixture.alias(card)?;
                let physical = self
                    .inputs
                    .physical_inventory
                    .iter()
                    .find(|physical| physical.alias == row.alias)
                    .ok_or("optional lowered opening physical member absent")?;
                if row.owner.seat() != alias.owner
                    || row.oracle_id != physical.oracle_id
                    || row.copy != u64::from(physical.copy)
                    || card.0.to_string() != physical.card_id
                {
                    return Err("optional lowered opening physical identity mismatch".into());
                }
            }
            for (index, role) in [Role::P0, Role::P1].into_iter().enumerate() {
                let declared = &decks[index];
                let actual = &self.inputs.decks[index];
                let order = &shuffle_orders[index];
                let shuffle = &fixture.shuffles[index];
                if declared.role != role
                    || declared.card_count != 60
                    || declared.entries.len() != 11
                    || declared
                        .entries
                        .iter()
                        .map(|entry| (&entry.oracle_id, entry.count))
                        .collect::<BTreeMap<_, _>>()
                        != actual
                            .entries
                            .iter()
                            .map(|entry| (&entry.oracle_id, u64::from(entry.count)))
                            .collect::<BTreeMap<_, _>>()
                    || order.role != role
                    || order
                        .top_first
                        .iter()
                        .map(|alias| fixture.card(alias))
                        .collect::<Result<Vec<_>, _>>()?
                        != shuffle.target
                {
                    return Err("optional lowered deck/shuffle differs from source inputs".into());
                }
            }
            return Ok(fixture);
        }
        let Initialization::Scheduled {
            turn,
            step,
            active,
            life,
            zones,
            battlefield_oldest_first,
        } = &recipe.initialization
        else {
            return Err("optional reduced input requires its scheduled partition".into());
        };
        let rows = recipe
            .inventory
            .iter()
            .enumerate()
            .map(|(index, row)| {
                Ok(PhysicalRow {
                    alias: row.alias.clone(),
                    card: CardId(
                        30_001u64
                            .checked_add(
                                u64::try_from(index).map_err(|_| "optional ordinal overflow")?,
                            )
                            .ok_or("optional CardId overflow")?,
                    ),
                    owner: row.owner.seat(),
                    oracle_id: row.oracle_id.clone(),
                    copy: u32::try_from(row.copy).map_err(|_| "optional copy ordinal overflow")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let materialized = catalogue::materialize_rows(
            &self.inventory,
            &self.registry,
            &self.inputs.definitions,
            &rows,
            Shape::OptionalPaymentPrepared,
        )?;
        validate_native_templates(&materialized, &self.inputs, &vocabulary)?;
        let names = Arc::new(NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("optional-context-v1-{}", case.as_str()),
            release: "optional-payment-v1".into(),
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
        .map_err(|error| format!("optional prepared context: {error:?}"))?;
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
        seed.life = vec![
            StartingLife {
                player: A,
                total: integer(&life.p0)?,
            },
            StartingLife {
                player: B,
                total: integer(&life.p1)?,
            },
        ];
        let start = TurnStart {
            number: *turn,
            step: match step {
                StartStep::Untap => Step::Untap,
                StartStep::PrecombatMain => Step::PrecombatMain,
            },
        };
        let mut fixture = Fixture {
            case: CaseId::Optional(case),
            initialization: FixtureInitialization::Scheduled(start),
            game: GameId(case.game_id()),
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
                "optional prepared role partition mismatch".into()
            }
            catalogue::PreparedError::UnknownZoneAlias(alias)
            | catalogue::PreparedError::UnknownBattlefieldAlias(alias) => {
                format!("unknown fixture alias {alias}")
            }
            catalogue::PreparedError::DuplicateOrForeignZone => {
                "optional duplicate or foreign zone member".into()
            }
            catalogue::PreparedError::UnsupportedBattlefield => {
                "optional unsupported or duplicate initial battlefield member".into()
            }
            catalogue::PreparedError::IncompletePartition => {
                "optional incomplete physical partition".into()
            }
            catalogue::PreparedError::MissingSeed => "optional partition source missing".into(),
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
        Ok(fixture)
    }

    /// Execute the selected source recipes through the shared recorder/replay.
    /// # Errors
    /// Rejects empty/duplicate selections or malformed source input construction.
    /// Native policy failures remain explicit failed runs in the returned report.
    pub fn run(
        &self,
        cases: &[OptionalCase],
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<OptionalReport, String> {
        if cases.is_empty() || cases.iter().copied().collect::<BTreeSet<_>>().len() != cases.len() {
            return Err("optional suite requires a nonempty unique selection".into());
        }
        let runs = cases
            .iter()
            .enumerate()
            .map(|(index, case)| {
                Ok(record::run_fixture_at(
                    self.fixture(*case, vocabulary.clone())?,
                    index,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(OptionalReport {
            schema_version: 1,
            profile: "optional-payment-v1",
            registry_version: 8,
            catalogue_sha256: self.inventory.compressed_sha256.clone(),
            runs,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct OptionalReport {
    schema_version: u32,
    profile: &'static str,
    registry_version: u32,
    catalogue_sha256: String,
    pub runs: Vec<RunReport>,
}
impl OptionalReport {
    pub fn completed(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(RunReport::completed)
    }
}

#[cfg(test)]
mod scheduled_tests;
