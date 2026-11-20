//! Test-local source intake and explicit prepared fixtures. No replacement interpreter.
use serde_json::Value;
use sha2::{Digest, Sha256};
use spellwake_card_programs::{OracleId, Registry, TEACHING_SOURCE_SHA256};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary, names::NameRules};
use spellwake_engine::{
    combat::CombatRules,
    effects::SeedCard,
    game::{GameContext, GameId, GameState, NativeCandidate},
    mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    priority::SeatId,
    program::{Program, ProgramKind},
    setup::GameSeed,
    turn::{Step, TurnStart},
};
use spellwake_reference_tools::{creature_vocabulary, packet};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

pub const P1: SeatId = SeatId(41);
pub const P2: SeatId = SeatId(7);
pub const P3: SeatId = SeatId(19);
pub const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
pub const RAISE: &str = "5b2364d7-a811-4595-a1b4-224c70555ffa";
pub const FLESHBAG: &str = "4b1bf05e-753e-4350-a913-894cf3cecc0c";
pub const EDICT: &str = "058917c1-21ab-488a-9f9c-591c55f3c596";
const ORIGINAL_SHA: &str = "c2a23c4d5a65cd39dd9c95c73e3c8e52881c9f461833a78373a9d8abca3778dd";
const DERIVATIVE_SHA: &str = "a2a01290799de7d0fcec2f2914883ed547b26863c34c8579794be04a921a35f8";
const MANIFEST_SHA: &str = "bc2d6e2da1415f00a72602cf5cd4bf7d24180c8b78c10748a48119f694e88c61";
const VOCABULARY_SHA: &str = "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c";
const METADATA_SHA: &str = "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad";
const RULES_SHA: &str = "8d860e451f20f38865b725b42d82feb714c725373dd8f3b32b8652b3eeb070ca";

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn required_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("explicit {name} required"))
}
fn pinned(name: &str, expected: &str) -> Result<Vec<u8>, String> {
    let path = required_path(name)?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if digest(&bytes) != expected {
        return Err(format!("{name} differs from exact retained source pin"));
    }
    Ok(bytes)
}
pub fn seat(label: &str) -> SeatId {
    match label {
        "P1" => P1,
        "P2" => P2,
        "P3" => P3,
        _ => panic!("unknown source seat {label}"),
    }
}
pub fn text(value: &Value) -> &str {
    value.as_str().expect("pinned source string")
}
pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("pinned source list")
        .iter()
        .map(|v| text(v).to_owned())
        .collect()
}

pub struct Source {
    pub registry: Registry,
    pub vocabulary: Arc<CreatureTypeVocabulary>,
    pub names: Arc<NameRules>,
    pub cases: BTreeMap<String, Value>,
}
impl Source {
    fn load() -> Result<Self, String> {
        let metadata = pinned("SPELLWAKE_CATALOGUE_METADATA", METADATA_SHA)?;
        let snapshot = pinned("SPELLWAKE_CATALOGUE_INPUT", TEACHING_SOURCE_SHA256)?;
        let vocabulary_bytes = pinned("SPELLWAKE_CREATURE_VOCABULARY_INPUT", VOCABULARY_SHA)?;
        let vocabulary = Arc::new(creature_vocabulary::vocabulary(packet::unique_json(
            &vocabulary_bytes,
        )?)?);
        let manifest = packet::unique_json(&pinned(
            "SPELLWAKE_SELECTION_SOURCE_MANIFEST_INPUT",
            MANIFEST_SHA,
        )?)?;
        let original = packet::unique_json(&pinned(
            "SPELLWAKE_SELECTION_ORIGINAL_MATRIX_INPUT",
            ORIGINAL_SHA,
        )?)?;
        let derivative = packet::unique_json(&pinned(
            "SPELLWAKE_SELECTION_RAISE_MATRIX_INPUT",
            DERIVATIVE_SHA,
        )?)?;
        let rules = pinned("SPELLWAKE_SELECTION_RULES_INPUT", RULES_SHA)?;
        let rules = std::str::from_utf8(&rules).map_err(|error| error.to_string())?;
        for rule in [
            "101.4.", "111.6.", "111.7.", "400.3.", "404.3.", "608.2n", "609.3.", "701.9a",
            "701.21a", "704.5d",
        ] {
            assert!(
                rules.lines().any(|line| line.starts_with(rule)),
                "actual saved CR {rule}"
            );
        }
        let inventory = spellwake_content::snapshot::decode(&metadata, snapshot)
            .map_err(|error| format!("full source intake: {error:?}"))?;
        let registry = Registry::from_resolution_selection_inventory(&inventory, &vocabulary)
            .map_err(|error| error.to_string())?;
        let coverage = registry.coverage();
        assert_eq!(coverage.registry_version, 9);
        assert_eq!(coverage.cards.len(), 9);
        assert_eq!(coverage.required_partial_bindings, 9);
        assert_eq!(coverage.fully_qualified_cards, 0);
        let selected = manifest["cards"]
            .as_array()
            .ok_or("original nine-source selection missing")?;
        assert_eq!(selected.len(), 9);
        for row in selected {
            let oracle = text(&row["oracle_id"]);
            let matching: Vec<_> = inventory
                .cards
                .iter()
                .filter(|r| r.oracle_id.as_deref() == Some(oracle))
                .collect();
            let [actual] = matching.as_slice() else {
                return Err("selected source identity is not unique in full inventory".into());
            };
            assert_eq!(actual.scryfall_id, row["printing_id"]);
            assert_eq!(actual.name, row["name"]);
            let [face] = actual.faces.as_slice() else {
                return Err("source face cardinality changed".into());
            };
            assert_eq!(face.oracle_text.as_deref(), row["saved_oracle"].as_str());
            assert!(
                coverage
                    .cards
                    .iter()
                    .any(|c| c.oracle_id.as_deref() == Some(oracle))
            );
        }
        let original_cases = original["scenarios"]
            .as_array()
            .ok_or("original20 missing")?;
        let derivative_cases = derivative["scenarios"]
            .as_array()
            .ok_or("reviewed derivative missing")?;
        assert_eq!(original_cases.len(), 20);
        assert_eq!(derivative_cases.len(), 20);
        assert_eq!(
            original_cases
                .iter()
                .map(|s| s["invalid_variants"].as_array().map_or(0, Vec::len))
                .sum::<usize>(),
            24
        );
        let mut cases = BTreeMap::new();
        for (old, reviewed) in original_cases.iter().zip(derivative_cases) {
            assert_eq!(old["id"], reviewed["id"]);
            let id = text(&old["id"]);
            let selected = if matches!(id, "SD09" | "SD10") {
                reviewed
            } else {
                assert_eq!(old, reviewed);
                old
            };
            if cases.insert(id.to_owned(), selected.clone()).is_some() {
                return Err("duplicate source case".into());
            }
        }
        Ok(Self {
            registry,
            vocabulary,
            names: Arc::new(NameRules::literal_names()),
            cases,
        })
    }
    pub fn card(
        &self,
        oracle: &str,
        card: CardId,
        owner: SeatId,
        destination: ObjectDestination,
    ) -> SeedCard {
        let oracle = OracleId::try_from(oracle.to_owned()).expect("pinned source Oracle ID");
        self.registry
            .seed(&oracle, card, owner, destination)
            .expect("real source9 Program")
    }
    pub fn program(&self, oracle: &str) -> Program {
        self.card(oracle, CardId(1), P1, ObjectDestination::Hand)
            .program
    }
}
static SOURCE: OnceLock<Result<Source, String>> = OnceLock::new();
pub fn source() -> Result<&'static Source, String> {
    SOURCE
        .get_or_init(Source::load)
        .as_ref()
        .map_err(Clone::clone)
}

pub fn cost(program: &Program) -> ManaCost {
    match &program.kind {
        ProgramKind::Spell(spell) => spell.cost.clone(),
        ProgramKind::Permanent(permanent) => permanent
            .casting_cost
            .clone()
            .expect("actual source permanent has a casting cost"),
    }
}
pub fn payment(cost: &ManaCost) -> ManaPayment {
    let mut by_type = cost.required.clone();
    if cost.generic > 0.into() {
        let old = by_type.entry(ManaType::Colourless).or_insert(0.into());
        *old = old.plus(&cost.generic);
    }
    ManaPayment {
        spend: by_type
            .into_iter()
            .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
            .collect(),
    }
}
#[derive(Clone)]
pub struct CastSpec {
    pub alias: String,
    pub oracle: String,
    pub caster: SeatId,
    pub target: Option<SeatId>,
}
/// Retained arguments/results of one successful Registry::seed fixture call.
pub struct SeedIdentity {
    pub alias: String,
    pub oracle: String,
    pub card: CardId,
    pub owner: SeatId,
    pub destination: ObjectDestination,
}

/// All external names bind actual seeded CardIds or actual token birth receipts.
/// CardIds are fixture handles; current ObjectIds always come from native objects.
pub struct Fixture {
    pub initialized: NativeCandidate,
    pub cards: BTreeMap<String, CardId>,
    pub seeded_identities: Vec<SeedIdentity>,
    pub spells: Vec<CastSpec>,
    pub case: Value,
}
impl Fixture {
    pub fn build(source: &Source, id: &str) -> Self {
        Self::build_with_history(source, id, false)
    }
    /// Separately named derivative: real prior decision/move, no original-row credit.
    pub fn build_with_history(source: &Source, id: &str, history: bool) -> Self {
        assert!(!history || matches!(id, "SD04" | "SD14"));
        assert_ne!(
            id, "SD20",
            "unchanged SourceGap must never be executed as a passing source case"
        );
        let case = source.cases.get(id).expect("original case").clone();
        let zones = &case["initial_players_and_zones"];
        let mut order: Vec<_> = strings(&zones["turn"]["order"])
            .iter()
            .map(|p| seat(p))
            .collect();
        // Rotation preserves the original cyclic turn order while proving APNAP
        // starts from the actual active player, not Vec index or numeric SeatId.
        order.rotate_right(1);
        let active = seat(text(&zones["turn"]["active"]));
        let mut cards = Vec::new();
        let mut aliases = BTreeMap::new();
        let mut seeded_identities = Vec::new();
        let mut spells = Vec::new();
        let mut next = 1_u64;
        let mut add = |alias: String, oracle: &str, owner, destination| {
            let card = CardId(next);
            next = next.checked_add(1).expect("finite fixture handles");
            assert!(aliases.insert(alias.clone(), card).is_none());
            let seeded = source.card(oracle, card, owner, destination);
            seeded_identities.push(SeedIdentity {
                alias,
                oracle: oracle.to_owned(),
                card: seeded.card,
                owner: seeded.owner,
                destination: seeded.destination,
            });
            cards.push(seeded);
        };
        for label in strings(&zones["turn"]["order"]) {
            let player = seat(&label);
            for zone in ["battlefield", "hand", "graveyard"] {
                for row in zones[&label][zone].as_array().expect("source zone") {
                    if row["token"].as_bool() == Some(true) {
                        continue;
                    }
                    let alias = text(&row["instance_id"]).to_owned();
                    let owner = seat(text(&row["owner"]));
                    let special_raise = matches!(id, "SD09" | "SD10") && alias == "P1-GY-RAISE-1";
                    let special_fleshbag =
                        matches!(id, "SD18" | "SD19") && alias == "P1-BF-FLESHBAG-1";
                    let historical_stale = history && alias.contains("-STALE");
                    let destination = if historical_stale && id == "SD04" {
                        ObjectDestination::Battlefield { controller: P2 }
                    } else if historical_stale
                        || special_raise
                        || special_fleshbag
                        || zone == "hand"
                    {
                        ObjectDestination::Hand
                    } else if zone == "graveyard" {
                        ObjectDestination::Graveyard
                    } else {
                        ObjectDestination::Battlefield {
                            controller: seat(text(&row["controller"])),
                        }
                    };
                    add(alias.clone(), text(&row["oracle_id"]), owner, destination);
                    if special_raise {
                        spells.push(CastSpec {
                            alias,
                            oracle: RAISE.into(),
                            caster: P1,
                            target: None,
                        });
                    } else if special_fleshbag {
                        spells.push(CastSpec {
                            alias,
                            oracle: FLESHBAG.into(),
                            caster: P1,
                            target: None,
                        });
                    }
                }
            }
            let count = zones[&label]["library"]["count"]
                .as_u64()
                .expect("bounded source library count");
            for ordinal in 0..count {
                // The adopted source explicitly leaves these twenty contents
                // unspecified. This visible fixture completion uses genuine
                // source Island definitions; no draw or library effect executes.
                add(
                    format!("{label}-DISCLOSED-LIBRARY-ISLAND-{ordinal}"),
                    ISLAND,
                    player,
                    ObjectDestination::LibraryBottom,
                );
            }
        }
        if !matches!(id, "SD18" | "SD19") {
            let row = &zones["stack"][0];
            let caster = seat(text(&row["controller"]));
            let target = row["targets"]
                .as_array()
                .expect("source spell targets")
                .first()
                .map(|p| seat(text(p)));
            add(
                "CASE-SPELL".into(),
                text(&row["oracle_id"]),
                caster,
                ObjectDestination::Hand,
            );
            spells.push(CastSpec {
                alias: "CASE-SPELL".into(),
                oracle: text(&row["oracle_id"]).into(),
                caster,
                target,
            });
        } else if id == "SD19" {
            add("CASE-RESPONSE".into(), EDICT, P2, ObjectDestination::Hand);
            spells.push(CastSpec {
                alias: "CASE-RESPONSE".into(),
                oracle: EDICT.into(),
                caster: P2,
                target: Some(P1),
            });
        }
        if history {
            let oracle = if id == "SD04" {
                EDICT
            } else {
                "ad44cf74-b717-48fb-9fa2-77512024d76a"
            };
            add(
                "COMPANION-HISTORY-SPELL".into(),
                oracle,
                P1,
                ObjectDestination::Hand,
            );
            if id == "SD14" {
                add(
                    "COMPANION-EXTRA-ISLAND".into(),
                    ISLAND,
                    P2,
                    ObjectDestination::Hand,
                );
            }
            spells.insert(
                0,
                CastSpec {
                    alias: "COMPANION-HISTORY-SPELL".into(),
                    oracle: oracle.into(),
                    caster: P1,
                    target: Some(P2),
                },
            );
        }
        let mut funding: BTreeMap<(SeatId, ManaType), RulesInteger> = BTreeMap::new();
        for spell in &spells {
            for spend in payment(&cost(&source.program(&spell.oracle))).spend {
                let old = funding
                    .entry((spell.caster, spend.mana_type))
                    .or_insert_with(|| 0.into());
                *old = old.plus(&spend.amount);
            }
        }
        let mana = funding
            .into_iter()
            .map(|((player, mana_type), amount)| FloatingMana {
                player,
                mana_type,
                amount,
            })
            .collect();
        let battlefield = cards
            .iter()
            .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
            .map(|c| c.card)
            .collect();
        let mut seed = GameSeed::with_uniform_life(
            order,
            active,
            cards,
            battlefield,
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            Arc::clone(&source.vocabulary),
            Arc::clone(&source.names),
        );
        for row in &mut seed.life {
            let label = match row.player {
                P1 => "P1",
                P2 => "P2",
                P3 => "P3",
                _ => unreachable!(),
            };
            row.total = zones[label]["life"]
                .as_i64()
                .expect("source life total")
                .into();
        }
        let context = GameContext::new(
            "source9-native-cases-v1".into(),
            source.vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "registry9-resolution-selection".into(),
            "resolution-sacrifice-discard-v1".into(),
            source.vocabulary.id(),
            source.names.id(),
        )
        .unwrap();
        let number: u64 = id.trim_start_matches("SD").parse().unwrap();
        let game = GameId(u128::from(910_000 + number));
        let start = TurnStart {
            number: 2,
            step: Step::PrecombatMain,
        };
        let initialized =
            GameState::initialize_scheduled(game, context.clone(), seed.clone(), start).unwrap();
        for card in &seed.cards {
            let cards = initialized.state().execution().window().cards();
            let actual = cards
                .get(cards.current_identity(card.card).unwrap())
                .unwrap();
            assert_eq!(
                actual.owner(),
                card.owner,
                "actual identity preserves source input ownership"
            );
            let (zone, controller) = match card.destination {
                ObjectDestination::Hand => (
                    spellwake_engine::object_identity::ObjectZone::Hand(card.owner),
                    None,
                ),
                ObjectDestination::Graveyard => (
                    spellwake_engine::object_identity::ObjectZone::Graveyard(card.owner),
                    None,
                ),
                ObjectDestination::LibraryBottom => (
                    spellwake_engine::object_identity::ObjectZone::Library(card.owner),
                    None,
                ),
                ObjectDestination::Battlefield { controller } => (
                    spellwake_engine::object_identity::ObjectZone::Battlefield,
                    Some(controller),
                ),
                _ => panic!("only explicitly disclosed source fixture destinations"),
            };
            assert_eq!(actual.zone(), zone);
            assert_eq!(actual.controller(), controller);
            assert_eq!(
                initialized.state().execution().base_program(actual.id()),
                Some(&card.program),
                "complete actual seeded Registry9 Program"
            );
        }
        assert_eq!(
            GameState::initialize_scheduled(game, context, seed, start),
            Ok(initialized.clone()),
            "same actual prepared seed/context initialization Result equality"
        );
        Self {
            initialized,
            cards: aliases,
            seeded_identities,
            spells,
            case,
        }
    }
    pub fn object(&self, state: &GameState, alias: &str) -> ObjectId {
        state
            .execution()
            .window()
            .cards()
            .current_identity(self.cards[alias])
            .expect("actual current seeded physical identity")
    }
}
