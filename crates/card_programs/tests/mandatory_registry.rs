//! Finite Registry7 content contract and source guards.
//! Embedded exact provider rows are a unit-only subset. Only the ignored explicit
//! full-source selectors load and hash the actual complete qualifying snapshot.
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    BindingStatus, MANDATORY_TRIGGER_PROFILE, MANDATORY_TRIGGER_REGISTRY_VERSION, OracleId,
    ProgramKey, Registry, RegistryError, Scope, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};
use spellwake_content::{Inventory, Legality, snapshot};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyId},
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    characteristics::{Color, PowerToughness},
    land::LandType,
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, ObjectFilter, PermanentDefinition,
        Program, ProgramKind, SpellDefinition, Supertype, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
};

fn expected() -> Value {
    serde_json::from_slice(include_bytes!("fixtures/mandatory-cards.expected.json")).unwrap()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn vocabulary() -> CreatureTypeVocabulary {
    let bytes = std::fs::read(
        std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
            .expect("explicit retained source-qualified vocabulary required"),
    )
    .unwrap();
    assert_eq!(digest(&bytes), expected()["creatureVocabularyInputSha256"]);
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    let hash =
        CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned()).unwrap();
    let vocabulary = CreatureTypeVocabulary::new(
        raw["rules_revision"].as_str().unwrap().to_owned(),
        *hash.as_bytes(),
        raw["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect(),
    )
    .unwrap();
    assert_eq!(vocabulary.id().to_string(), raw["id"]);
    assert_eq!(vocabulary.id().to_string(), TEACHING_VOCABULARY_ID);
    vocabulary
}
fn subset() -> Inventory {
    snapshot::decode(
        include_bytes!("fixtures/mandatory-cards.meta.json"),
        include_bytes!("fixtures/mandatory-cards.jsonl.gz").to_vec(),
    )
    .unwrap()
}
/// Synthetic post-intake unit premise only; never full-source admission evidence.
fn guard_inventory() -> Inventory {
    let mut source = subset();
    source.compressed_sha256 = TEACHING_SOURCE_SHA256.into();
    source
}
fn id(value: &str) -> OracleId {
    value.to_owned().try_into().unwrap()
}
fn seed(registry: &Registry, oracle: &str) -> Program {
    registry
        .seed(
            &id(oracle),
            CardId(17),
            SeatId(1),
            ObjectDestination::LibraryBottom,
        )
        .unwrap()
        .program
}
fn integer(value: &Value) -> RulesInteger {
    value.as_str().unwrap().to_owned().try_into().unwrap()
}
fn mana_type(label: &str) -> ManaType {
    match label {
        "White" => ManaType::White,
        "Blue" => ManaType::Blue,
        "Black" => ManaType::Black,
        "Green" => ManaType::Green,
        _ => panic!("unlisted frozen mana type"),
    }
}
fn color(label: &str) -> Color {
    match label {
        "White" => Color::White,
        "Blue" => Color::Blue,
        "Black" => Color::Black,
        "Green" => Color::Green,
        _ => panic!("unlisted frozen color"),
    }
}
fn land_type(label: &str) -> LandType {
    match label {
        "Plains" => LandType::Plains,
        "Island" => LandType::Island,
        "Swamp" => LandType::Swamp,
        "Forest" => LandType::Forest,
        _ => panic!("unlisted frozen land type"),
    }
}
fn cost(raw: &Value) -> ManaCost {
    ManaCost {
        generic: integer(&raw["generic"]),
        required: raw["required"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (mana_type(k), integer(v)))
            .collect(),
    }
}
fn names(raw: &Value) -> ObjectNames {
    let values = raw["values"].as_array().unwrap();
    match values.as_slice() {
        [] => ObjectNames::nameless(),
        [value] => {
            ObjectNames::single(EnglishCardName::new(value.as_str().unwrap().to_owned()).unwrap())
        }
        _ => panic!("unlisted frozen name shape"),
    }
}
fn optional_colors(raw: &Value) -> Option<BTreeSet<Color>> {
    raw.as_array()
        .map(|values| values.iter().map(|v| color(v.as_str().unwrap())).collect())
}
fn relation(raw: &Value) -> ObjectFilter {
    match raw.as_str().unwrap() {
        "SelfObject" => spellwake_engine::program::ObjectFilter::self_object(),
        "AnyCreature" => spellwake_engine::program::ObjectFilter::creature(),
        "AnotherCreature" => {
            spellwake_engine::program::ObjectFilter::creature().other_than_ability_source()
        }
        "AnotherControlledCreature" => spellwake_engine::program::ObjectFilter::creature()
            .controlled()
            .other_than_ability_source(),
        _ => panic!("unlisted frozen object relation"),
    }
}
fn adjustment(raw: &Value) -> LifeAdjustment {
    match raw["variant"].as_str().unwrap() {
        "Gain" => LifeAdjustment::Gain(integer(&raw["value"])),
        "Lose" => LifeAdjustment::Lose(integer(&raw["value"])),
        _ => panic!("unlisted frozen life adjustment"),
    }
}
fn amount(raw: &Value) -> ValueExpr {
    assert_eq!(raw["variant"], "Literal");
    ValueExpr::Literal(integer(&raw["value"]))
}
/// Interpret the independently frozen declarative contract only in this test.
/// Exhaustive Program struct construction checks every field/default, without
/// reusing the production authored descriptors or matching on card names.
fn contract_program(raw: &Value, vocabulary: &CreatureTypeVocabulary) -> Program {
    assert_eq!(raw["activated"], json!([]));
    assert!(raw["continuous"].is_null());
    Program {
        cast_timing: None,
        activation_modes: Default::default(),
        adventure: None,
        prepare_spell: None,
        enters_prepared: false,
        entry_tap_unless: None,
        opening_hand_battlefield: false,
        entry_copy: None,
        splice: None,
        nightbound_back: None,
        transform_back: None,
        modal_back: None,
        flip_back: None,
        prototype: None,
        split: None,
        cast_reductions: vec![],
        entry_counter_abilities: vec![],
        conditional_cost: None,
        plot: None,
        exile_action: None,
        madness: None,
        miracle: None,
        cast_from_own_exile: false,
        flashback: None,
        flashback_x_mana_symbols: 0,
        sacrifice_creature_cast_cost: false,
        sacrifice_cast_cost_filter: None,
        reveal_colorless_creature_cast_cost: false,
        discard_cast_cost: false,
        discard_cast_cost_filter: None,
        graveyard_creature_exile_cost: 0,
        graveyard_exile_x_cost: false,
        graveyard_self_cast: None,
        tap_creatures_cast_cost: false,
        flashback_sacrifice_creatures: 0,
        flashback_tap_white_creatures: 0,
        flashback_life: 0,
        additional_cast_costs: vec![],
        per_extra_target_cost: None,
        activation_restrictions: Default::default(),
        activation_cost_reductions: Default::default(),
        activation_target_cost_reductions: Default::default(),
        activation_hybrid_symbols: Default::default(),
        extra_activation_costs: Default::default(),
        extra_cast_costs: Vec::new(),
        loyalty_costs: Default::default(),
        variable_loyalty_costs: Default::default(),
        face_up_abilities: Vec::new(),
        printed_keywords: Default::default(),
        printed_toxic: None,
        x_mana_symbols: 0,
        hybrid_symbols: Vec::new(),
        cannot_be_countered: false,
        attack_requirement: None,
        additional_attack_requirements: Vec::new(),
        combat_limits: Default::default(),
        cast_during_own_combat: false,
        additional_life_cost: None,
        siege_back: None,
        disturb: None,
        meld_partner: None,
        printed_defense: None,
        printed_loyalty: None,
        zone_replacements: vec![],
        zone_replacement_extras: Default::default(),
        card_types: raw["card_types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| match v.as_str().unwrap() {
                "Land" => CardType::Land,
                "Creature" => CardType::Creature,
                "Instant" => CardType::Instant,
                _ => panic!("unlisted frozen card type"),
            })
            .collect(),
        creature_subtypes: raw["creature_subtypes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| vocabulary.resolve_label(v.as_str().unwrap()).unwrap())
            .collect(),
        land_subtypes: raw["land_subtypes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| land_type(v.as_str().unwrap()))
            .collect(),
        spell_subtypes: Default::default(),
        artifact_subtypes: Default::default(),
        enchantment_subtypes: Default::default(),
        planeswalker_subtypes: Default::default(),
        names: names(&raw["names"]),
        supertypes: raw["supertypes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                assert_eq!(v, "Basic");
                Supertype::Basic
            })
            .collect(),
        kind: match raw["kind"]["variant"].as_str().unwrap() {
            "Permanent" => ProgramKind::Permanent(PermanentDefinition {
                entry_tap: None,
                attachment: None,
                casting_cost: (!raw["kind"]["casting_cost"].is_null())
                    .then(|| cost(&raw["kind"]["casting_cost"])),
                printed_pt: (!raw["kind"]["printed_pt"].is_null()).then(|| {
                    PowerToughness {
                        power: integer(&raw["kind"]["printed_pt"]["power"]),
                        toughness: integer(&raw["kind"]["printed_pt"]["toughness"]),
                    }
                    .into()
                }),
            }),
            "Spell" => ProgramKind::Spell(SpellDefinition {
                modes: None,
                cost: cost(&raw["kind"]["cost"]),
                uncastable_mana_cost: false,
                body: contract_body(&raw["kind"]["body"], vocabulary),
            }),
            _ => panic!("unlisted frozen program kind"),
        },
        color_indicator: optional_colors(&raw["color_indicator"]),
        effect_defined_colors: optional_colors(&raw["effect_defined_colors"]),
        mana: raw["mana"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                assert_eq!(v["id"]["type"], "ManaAbilityId");
                assert_eq!(v["output"]["variant"], "Fixed");
                ManaAbility {
                    id: ManaAbilityId(v["id"]["value"].as_u64().unwrap()),
                    output: ManaOutput::Fixed(
                        v["output"]["produced"]
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| (mana_type(k), integer(v)))
                            .collect(),
                    ),
                }
            })
            .collect(),
        activated: Vec::new(),
        triggered: raw["triggered"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                assert_eq!(v["id"]["type"], "TriggeredAbilityId");
                assert_eq!(v["grouping"], "EachOccurrence");
                TriggeredAbility {
                    id: TriggeredAbilityId(v["id"]["value"].as_u64().unwrap()),
                    condition: match v["condition"]["variant"].as_str().unwrap() {
                        "Enters" => spellwake_engine::program::TriggerSpec::enters(relation(
                            &v["condition"]["object_relation"],
                        )),
                        "Dies" => spellwake_engine::program::TriggerSpec::dies(relation(
                            &v["condition"]["object_relation"],
                        )),
                        _ => panic!("unlisted frozen trigger condition"),
                    },
                    grouping: TriggerGrouping::EachOccurrence,
                    body: contract_body(&v["body"], vocabulary),
                }
            })
            .collect(),
        continuous: None,
        additional_battlefield_abilities: Vec::new(),
        static_conditions: Default::default(),
        gift: None,
        casualty: None,
        conspire: false,
        dredge: None,
        assist: false,
        cipher: false,
    }
}
fn contract_body(raw: &Value, vocabulary: &CreatureTypeVocabulary) -> ExecutableBody {
    let target = match raw["target"].as_str() {
        Some("None") => BodyTarget::None,
        Some("SinglePlayer") => BodyTarget::SinglePlayer,
        Some("SingleCreature") => BodyTarget::SingleCreature,
        None => {
            assert_eq!(
                raw["target"],
                json!({"variant":"SingleSpell","filter":"AnySpell"})
            );
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any())
        }
        _ => panic!("unlisted frozen target"),
    };
    ExecutableBody::new(
        target,
        raw["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| match v["variant"].as_str().unwrap() {
                "ReturnToOwnersHand" => spellwake_engine::program::Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Hand,
                    ),
                )),
                "CounterSpell" => Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                )),
                "DrawController" => spellwake_engine::program::Instruction::DrawCards(Box::new(
                    spellwake_engine::toolbox::DrawCardsSpec::simple(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        amount(&v["amount"]),
                    ),
                )),
                "AdjustControllerLife" => match adjustment(&v["adjustment"]) {
                    spellwake_engine::life::LifeAdjustment::Gain(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::gain(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::Controller,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                    spellwake_engine::life::LifeAdjustment::Lose(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::lose(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::Controller,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                    spellwake_engine::life::LifeAdjustment::Set(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::set(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::Controller,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                },
                "AdjustTargetLife" => match adjustment(&v["adjustment"]) {
                    spellwake_engine::life::LifeAdjustment::Gain(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::gain(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                    spellwake_engine::life::LifeAdjustment::Lose(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::lose(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                    spellwake_engine::life::LifeAdjustment::Set(n) => {
                        spellwake_engine::program::Instruction::AdjustLife(Box::new(
                            spellwake_engine::toolbox::AdjustLifeSpec::set(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(n),
                            ),
                        ))
                    }
                },
                "CreateTokens" => Instruction::CreateTokens {
                    creations: v["creations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| {
                            assert_eq!(c["recipients"], "Controller");
                            assert_eq!(c["definition"]["name"]["variant"], "Default");
                            let definition = TokenDefinition::new(
                                contract_program(&c["definition"]["program"], vocabulary),
                                TokenName::Default {
                                    ordered_subtypes: c["definition"]["name"]["ordered_subtypes"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .map(|s| {
                                            assert_eq!(s["variant"], "Creature");
                                            TokenSubtype::Creature(
                                                vocabulary
                                                    .resolve_label(s["label"].as_str().unwrap())
                                                    .unwrap(),
                                            )
                                        })
                                        .collect(),
                                },
                                vocabulary,
                            )
                            .unwrap();
                            definition.validate(vocabulary).unwrap();
                            let materialized = definition.materialize(vocabulary).unwrap();
                            materialized.validate(vocabulary).unwrap();
                            assert_eq!(
                                materialized.names,
                                ObjectNames::single(
                                    EnglishCardName::new(
                                        c["definition"]["materialized_names"][0]
                                            .as_str()
                                            .unwrap()
                                            .to_owned()
                                    )
                                    .unwrap()
                                )
                            );
                            assert_eq!(
                                materialized.defined_colors(),
                                optional_colors(&c["definition"]["defined_colors"]).unwrap()
                            );
                            TokenCreation {
                                definition,
                                amount: amount(&c["amount"]),
                                base_size: None,
                                enters_tapped: false,
                                recipients: TokenRecipients::Controller,
                            }
                        })
                        .collect(),
                },
                _ => panic!("unlisted frozen instruction"),
            })
            .collect(),
    )
}
fn key(card: &Value) -> ProgramKey {
    match card["name"].as_str().unwrap() {
        "Plains" | "Island" | "Swamp" | "Forest" => ProgramKey::MandatoryBasicLandV1,
        "Raise the Alarm" => ProgramKey::MandatoryTokenCreationSpellV1,
        "Unsummon" => ProgramKey::MandatoryBounceCreatureV1,
        "Counterspell" => ProgramKey::MandatoryCounterSpellV1,
        _ if card["programContract"]["triggered"]
            .as_array()
            .unwrap()
            .is_empty() =>
        {
            ProgramKey::MandatoryVanillaCreatureV1
        }
        _ => ProgramKey::MandatoryTriggeredCreatureV1,
    }
}
fn assert_selected(
    registry: &Registry,
    source_records: usize,
    vocabulary: &CreatureTypeVocabulary,
) {
    let coverage = registry.coverage();
    assert_eq!(coverage.registry_version, 7);
    assert_eq!(coverage.source_records, source_records);
    assert_eq!(coverage.source_sha256, TEACHING_SOURCE_SHA256);
    assert_eq!(coverage.creature_vocabulary_id, vocabulary.id());
    assert_eq!(coverage.required_records, 19);
    assert_eq!(coverage.required_partial_bindings, 19);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    let expected = expected();
    let selected = expected["cards"].as_array().unwrap();
    assert_eq!(coverage.cards.len(), 19);
    assert_eq!(
        coverage
            .cards
            .iter()
            .map(|c| c.oracle_id.as_deref().unwrap())
            .collect::<BTreeSet<_>>(),
        selected
            .iter()
            .map(|c| c["oracleId"].as_str().unwrap())
            .collect()
    );
    for card in selected {
        let oracle = card["oracleId"].as_str().unwrap();
        let row = coverage
            .cards
            .iter()
            .find(|c| c.oracle_id.as_deref() == Some(oracle))
            .unwrap();
        assert_eq!(row.name, card["name"]);
        assert!(row.required);
        assert_eq!(row.behavior_evidence, "not-evaluated-by-registry");
        assert_eq!(
            row.binding,
            BindingStatus::Partial {
                program: key(card),
                scope: Scope::MandatoryTriggerOrdinaryCardV1,
                semantic_sha256: card["registrySignatureV1Sha256"].as_str().unwrap().into()
            }
        );
        let actual = seed(registry, oracle);
        actual.validate(vocabulary).unwrap();
        assert_eq!(
            actual,
            contract_program(&card["programContract"], vocabulary),
            "complete Program differs for {}",
            row.name
        );
        assert_eq!(
            actual.defined_colors(),
            optional_colors(&card["definedColors"]).unwrap()
        );
        // Validate/materialize the actual nested templates, independently of the
        // expected-template constructor used for complete equality above.
        let bodies = actual
            .triggered
            .iter()
            .map(|t| &t.body)
            .chain(match &actual.kind {
                ProgramKind::Spell(s) => Some(&s.body),
                _ => None,
            });
        for body in bodies {
            for instruction in &body.instructions {
                if let Instruction::CreateTokens { creations } = instruction {
                    for creation in creations {
                        creation.definition.validate(vocabulary).unwrap();
                        let born = creation.definition.materialize(vocabulary).unwrap();
                        born.validate(vocabulary).unwrap();
                        assert!(
                            born.mana.is_empty()
                                && born.activated.is_empty()
                                && born.triggered.is_empty()
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn unit_only_subset_matches_complete_frozen_programs_and_exact_registry7_contract() {
    let vocabulary = vocabulary();
    let registry =
        Registry::from_mandatory_trigger_inventory(&guard_inventory(), &vocabulary).unwrap();
    assert_selected(&registry, 19, &vocabulary);
    assert_eq!(MANDATORY_TRIGGER_REGISTRY_VERSION, 7);
    assert_eq!(MANDATORY_TRIGGER_PROFILE, "mandatory-trigger-v1");
    for (key, wire) in [
        (ProgramKey::MandatoryBasicLandV1, "mandatory-basic-land-v1"),
        (
            ProgramKey::MandatoryVanillaCreatureV1,
            "mandatory-vanilla-creature-v1",
        ),
        (
            ProgramKey::MandatoryTriggeredCreatureV1,
            "mandatory-triggered-creature-v1",
        ),
        (
            ProgramKey::MandatoryTokenCreationSpellV1,
            "mandatory-token-creation-spell-v1",
        ),
        (
            ProgramKey::MandatoryBounceCreatureV1,
            "mandatory-bounce-creature-v1",
        ),
        (
            ProgramKey::MandatoryCounterSpellV1,
            "mandatory-counter-spell-v1",
        ),
    ] {
        assert_eq!(serde_json::to_value(key).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_value(Scope::MandatoryTriggerOrdinaryCardV1).unwrap(),
        "mandatory-trigger-ordinary-card-v1"
    );
}
#[test]
fn subset_foreign_source_and_foreign_vocabulary_are_not_qualifying_inputs() {
    let vocabulary = vocabulary();
    let mut source = subset();
    assert_eq!(source.compressed_sha256, expected()["subsetSha256"]);
    assert!(matches!(
        Registry::from_mandatory_trigger_inventory(&source, &vocabulary),
        Err(RegistryError::MandatorySourceMismatch)
    ));
    source.compressed_sha256 = "0".repeat(64);
    assert!(matches!(
        Registry::from_mandatory_trigger_inventory(&source, &vocabulary),
        Err(RegistryError::MandatorySourceMismatch)
    ));
    let foreign = CreatureTypeVocabulary::new(
        "unit-only-foreign".into(),
        *vocabulary.rules_sha256(),
        vocabulary.members().to_vec(),
    )
    .unwrap();
    assert!(matches!(
        Registry::from_mandatory_trigger_inventory(&guard_inventory(), &foreign),
        Err(RegistryError::MandatoryVocabularyMismatch)
    ));
}
#[test]
fn every_selected_missing_duplicate_and_foreign_record_rejects_before_seeding() {
    let vocabulary = vocabulary();
    for index in 0..19 {
        let mut source = guard_inventory();
        let affected = source.cards.remove(index).oracle_id.unwrap();
        assert!(
            matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatoryMissingIdentity{oracle_id}) if oracle_id==affected)
        );
        let mut source = guard_inventory();
        source.cards.push(subset().cards.remove(index));
        assert!(
            matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatoryDuplicateIdentity{oracle_id}) if oracle_id==affected)
        );
        for foreign in [
            None,
            Some("00000000-0000-0000-0000-000000000001"),
            Some("invalid"),
            Some("86e3c86c-d50e-4aa9-b080-f68b6afee87c"),
        ] {
            let mut source = guard_inventory();
            source.cards[index].oracle_id = foreign.map(str::to_owned);
            assert!(
                matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatoryMissingIdentity{oracle_id}) if oracle_id==affected)
            );
        }
    }
}
#[test]
fn every_selected_materialized_face_value_presence_and_shape_is_guarded() {
    let vocabulary = vocabulary();
    for index in 0..19 {
        for field in [
            "name",
            "layout",
            "face-name",
            "type",
            "missing-type",
            "cost",
            "missing-cost",
            "text",
            "missing-text",
            "power",
            "missing-power",
            "toughness",
            "missing-toughness",
            "missing-face",
            "extra-face",
        ] {
            let mut source = guard_inventory();
            let extra = subset().cards.remove(index).faces.remove(0);
            let card = &mut source.cards[index];
            let affected = card.oracle_id.clone().unwrap();
            match field {
                "name" => card.name.push('!'),
                "layout" => card.layout = "transform".into(),
                "face-name" => card.faces[0].name.push('!'),
                "type" => card.faces[0].type_line = Some("Artifact".into()),
                "missing-type" => card.faces[0].type_line = None,
                "cost" => card.faces[0].mana_cost = Some("{0}".into()),
                "missing-cost" => card.faces[0].mana_cost = None,
                "text" => card.faces[0].oracle_text = Some("Draw a card.".into()),
                "missing-text" => card.faces[0].oracle_text = None,
                "power" => card.faces[0].power = Some("123".into()),
                "missing-power" if card.faces[0].power.is_none() => continue,
                "missing-power" => card.faces[0].power = None,
                "toughness" => card.faces[0].toughness = Some("456".into()),
                "missing-toughness" if card.faces[0].toughness.is_none() => continue,
                "missing-toughness" => card.faces[0].toughness = None,
                "missing-face" => card.faces.clear(),
                "extra-face" => card.faces.push(extra),
                _ => unreachable!(),
            }
            assert!(
                matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatorySemanticMismatch{oracle_id}) if oracle_id==affected),
                "index={index} field={field}"
            );
        }
    }
}
#[test]
fn every_selected_legality_and_required_status_is_guarded() {
    let vocabulary = vocabulary();
    for index in 0..19 {
        for legality in [
            Some(Legality::Banned),
            Some(Legality::Restricted),
            Some(Legality::NotLegal),
            Some(Legality::Missing),
            Some(Legality::Unknown("future".into())),
            None,
        ] {
            let mut source = guard_inventory();
            let affected = source.cards[index].oracle_id.clone().unwrap();
            if let Some(legality) = legality {
                source.cards[index]
                    .legalities
                    .insert("modern".into(), legality);
            } else {
                source.cards[index].legalities.remove("modern");
            }
            assert!(
                matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatoryLegalityMismatch{oracle_id}) if oracle_id==affected)
            );
        }
        let mut source = guard_inventory();
        source.cards[index].required_by_base_formats = false;
        assert!(matches!(
            Registry::from_mandatory_trigger_inventory(&source, &vocabulary),
            Err(RegistryError::MandatoryLegalityMismatch { .. })
        ));
    }
}
#[test]
fn whole_paragraph_guards_reject_dropped_reordered_or_changed_trigger_mana_and_token_clauses() {
    let vocabulary = vocabulary();
    for (name, text) in [
        (
            "Llanowar Visionary",
            "When this creature enters, draw a card.",
        ),
        (
            "Phyrexian Rager",
            "When this creature enters, you lose 1 life and draw a card.",
        ),
        (
            "Novice Occultist",
            "When this creature dies, you lose 1 life and draw a card.",
        ),
        (
            "Blood Artist",
            "Whenever this creature or another creature dies, you gain 1 life and target player loses 1 life.",
        ),
        (
            "Blood Artist",
            "Whenever another creature dies, target player loses 1 life and you gain 1 life.",
        ),
        (
            "Blood Artist",
            "Whenever this creature or another creature dies, each opponent loses 1 life and you gain 1 life.",
        ),
        (
            "Doomed Dissenter",
            "When this creature leaves the battlefield, create a 2/2 black Zombie creature token.",
        ),
        (
            "Doomed Dissenter",
            "When this creature dies, create two 2/2 black Zombie creature tokens.",
        ),
        (
            "Soul Warden",
            "Whenever a creature enters, you gain 1 life.",
        ),
        (
            "Soul Warden",
            "Whenever another creature you control enters, you gain 1 life.",
        ),
        (
            "Impassioned Orator",
            "Whenever another creature enters, you gain 1 life.",
        ),
        (
            "Elvish Visionary",
            "Whenever another creature enters, draw a card.",
        ),
        (
            "Raise the Alarm",
            "Create one 1/1 white Soldier creature token.",
        ),
        (
            "Raise the Alarm",
            "Create two 1/1 black Soldier creature tokens.",
        ),
        (
            "Raise the Alarm",
            "Create two 2/2 white Soldier creature tokens.",
        ),
        (
            "Raise the Alarm",
            "Create two 1/1 white Zombie creature tokens.",
        ),
        (
            "Raise the Alarm",
            "You may create two 1/1 white Soldier creature tokens.",
        ),
        (
            "Raise the Alarm",
            "Create two 1/1 white Soldier creature tokens with vigilance.",
        ),
    ] {
        let mut source = guard_inventory();
        let card = source.cards.iter_mut().find(|c| c.name == name).unwrap();
        assert_ne!(
            card.faces[0].oracle_text.as_deref(),
            Some(text),
            "negative must change the selected source"
        );
        let affected = card.oracle_id.clone().unwrap();
        card.faces[0].oracle_text = Some(text.into());
        assert!(
            matches!(Registry::from_mandatory_trigger_inventory(&source,&vocabulary),Err(RegistryError::MandatorySemanticMismatch{oracle_id}) if oracle_id==affected),
            "{name}: {text}"
        );
    }
    for index in 0..19 {
        for suffix in ["\n\nYou may draw a card.", "\n\nFlying"] {
            let mut source = guard_inventory();
            source.cards[index].faces[0]
                .oracle_text
                .as_mut()
                .unwrap()
                .push_str(suffix);
            assert!(matches!(
                Registry::from_mandatory_trigger_inventory(&source, &vocabulary),
                Err(RegistryError::MandatorySemanticMismatch { .. })
            ));
        }
    }
}
fn combined_unit_inventory() -> Inventory {
    let mut source = guard_inventory();
    let mut seen = source
        .cards
        .iter()
        .map(|c| c.oracle_id.clone())
        .collect::<BTreeSet<_>>();
    for other in [
        snapshot::decode(
            include_bytes!("../../../tests/catalogue/teaching-cards.meta.json"),
            include_bytes!("../../../tests/catalogue/teaching-cards.jsonl.gz").to_vec(),
        )
        .unwrap(),
        snapshot::decode(
            include_bytes!("fixtures/interaction-cards.meta.json"),
            include_bytes!("fixtures/interaction-cards.jsonl.gz").to_vec(),
        )
        .unwrap(),
    ] {
        for card in other.cards {
            if seen.insert(card.oracle_id.clone()) {
                source.cards.push(card);
            }
        }
    }
    source
}
fn assert_predecessors(source: &Inventory, vocabulary: &CreatureTypeVocabulary) {
    let teaching = Registry::from_teaching_inventory(source, vocabulary).unwrap();
    let interaction = Registry::from_interaction_inventory(source, vocabulary).unwrap();
    let mandatory = Registry::from_mandatory_trigger_inventory(source, vocabulary).unwrap();
    assert_eq!(teaching.coverage().cards.len(), 20);
    assert_eq!(interaction.coverage().cards.len(), 20);
    let mut inherited = 0;
    let mut both = 0;
    for card in expected()["cards"].as_array().unwrap() {
        if card["predecessorProgramEquality"].is_null() {
            continue;
        }
        inherited += 1;
        let oracle = card["oracleId"].as_str().unwrap();
        let expected = seed(&mandatory, oracle);
        if teaching.resolve(&id(oracle)).is_some() {
            assert_eq!(seed(&teaching, oracle), expected);
        }
        if interaction.resolve(&id(oracle)).is_some() {
            assert_eq!(seed(&interaction, oracle), expected);
        }
        if teaching.resolve(&id(oracle)).is_some() && interaction.resolve(&id(oracle)).is_some() {
            both += 1;
        }
    }
    assert_eq!(inherited, 9);
    assert_eq!(both, 5);
    for registry in [&teaching, &interaction] {
        for card in &registry.coverage().cards {
            let oracle = card.oracle_id.as_deref().unwrap();
            if !expected()["cards"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["oracleId"] == oracle)
            {
                assert!(mandatory.resolve(&id(oracle)).is_none());
            }
        }
    }
    let reference = Registry::from_inventory(source, vocabulary).unwrap();
    assert_eq!(reference.coverage().registry_version, 4);
    assert_eq!(
        reference
            .resolve(&id(spellwake_card_programs::UNSUMMON))
            .unwrap()
            .key(),
        ProgramKey::UnsummonCreatureV1
    );
    for card in expected()["cards"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["membership"] == "new-main")
    {
        assert!(
            reference
                .resolve(&id(card["oracleId"].as_str().unwrap()))
                .is_none()
        );
    }
}
#[test]
fn forty_identity_unit_union_preserves_exact_old_pools_nine_inherited_programs_and_reference_scope()
{
    let source = combined_unit_inventory();
    assert_eq!(source.cards.len(), 40);
    let vocabulary = vocabulary();
    assert_predecessors(&source, &vocabulary);
    assert_selected(
        &Registry::from_mandatory_trigger_inventory(&source, &vocabulary).unwrap(),
        40,
        &vocabulary,
    );
    let mut source = source;
    let mut extra = subset().cards.remove(0);
    extra.oracle_id = Some("00000000-0000-0000-0000-000000000001".into());
    source.cards.push(extra);
    let registry = Registry::from_mandatory_trigger_inventory(&source, &vocabulary).unwrap();
    assert_selected(&registry, 41, &vocabulary);
    assert!(matches!(
        registry.seed(
            &id("00000000-0000-0000-0000-000000000001"),
            CardId(99),
            SeatId(0),
            ObjectDestination::LibraryBottom
        ),
        Err(RegistryError::MissingBinding)
    ));
}
fn full_inputs() -> (Vec<u8>, Vec<u8>) {
    let metadata = std::fs::read(
        std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
            .expect("explicit retained full-source metadata required"),
    )
    .unwrap();
    let source = std::fs::read(
        std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
            .expect("explicit retained full-source gzip required"),
    )
    .unwrap();
    assert_eq!(digest(&metadata), expected()["originalMetadataSha256"]);
    assert_eq!(digest(&source), TEACHING_SOURCE_SHA256);
    (metadata, source)
}
#[test]
#[ignore = "requires the actual full catalogue, metadata and explicit source-qualified vocabulary"]
fn actual_full_catalogue_mandatory_inventory_is_source_qualified() {
    let (_metadata, _bytes) = full_inputs();
    let source = snapshot::load(
        &PathBuf::from(std::env::var_os("SPELLWAKE_CATALOGUE_METADATA").unwrap()),
        &PathBuf::from(std::env::var_os("SPELLWAKE_CATALOGUE_INPUT").unwrap()),
    )
    .unwrap();
    assert_eq!(source.cards.len(), 39196);
    assert_eq!(source.source_records, 39196);
    let vocabulary = vocabulary();
    let registry = Registry::from_mandatory_trigger_inventory(&source, &vocabulary).unwrap();
    assert_selected(&registry, 39196, &vocabulary);
    assert_predecessors(&source, &vocabulary);
    for (key, registry) in [
        (
            "registry4",
            Registry::from_inventory(&source, &vocabulary).unwrap(),
        ),
        (
            "registry5",
            Registry::from_teaching_inventory(&source, &vocabulary).unwrap(),
        ),
        (
            "registry6",
            Registry::from_interaction_inventory(&source, &vocabulary).unwrap(),
        ),
    ] {
        assert_eq!(
            digest(&serde_json::to_vec(registry.coverage()).unwrap()),
            expected()["predecessorCoverageSha256"][key],
            "predecessor coverage bytes changed for {key}"
        );
    }
}
#[test]
#[ignore = "recompresses actual full catalogue raw-only mutations; explicit full inputs and output directory required"]
fn actual_full_catalogue_raw_only_traits_fail_at_source_intake() {
    let (metadata, bytes) = full_inputs();
    let vocabulary = vocabulary();
    let baseline = snapshot::decode(&metadata, bytes.clone()).unwrap();
    Registry::from_mandatory_trigger_inventory(&baseline, &vocabulary).unwrap();
    let output = PathBuf::from(
        std::env::var_os("SPELLWAKE_MANDATORY_NEGATIVE_OUTPUT")
            .expect("explicit lane-local raw negative output directory required"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut raw = Vec::new();
    MultiGzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let selected = expected()["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Llanowar Visionary")
        .unwrap()
        .clone();
    let mut lines = raw
        .split_inclusive(|b| *b == b'\n')
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let index = usize::try_from(selected["sourceLineOneBased"].as_u64().unwrap()).unwrap() - 1;
    assert_eq!(digest(&lines[index]), selected["rawLineSha256IncludingLf"]);
    let original: Value = serde_json::from_slice(&lines[index]).unwrap();
    let mut receipts = Vec::new();
    for (label, field, value) in [
        ("colors", "colors", json!(["U"])),
        ("color-identity", "color_identity", json!(["U"])),
        ("keywords", "keywords", json!(["Flying"])),
        ("color-indicator", "color_indicator", json!(["U"])),
        ("produced-mana", "produced_mana", json!(["U"])),
        (
            "multipart",
            "card_faces",
            json!([{"name":"Llanowar Visionary","mana_cost":"{2}{G}","type_line":"Creature — Elf Druid","oracle_text":original["oracle_text"],"power":"2","toughness":"2"}]),
        ),
    ] {
        let mut changed = original.clone();
        assert_ne!(changed.get(field), Some(&value));
        changed[field] = value.clone();
        let mut line = serde_json::to_vec(&changed).unwrap();
        line.push(b'\n');
        lines[index] = line;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        for line in &lines {
            encoder.write_all(line).unwrap();
        }
        let changed_bytes = encoder.finish().unwrap();
        let mut changed_metadata: Value = serde_json::from_slice(&metadata).unwrap();
        changed_metadata["compressed_size"] = json!(changed_bytes.len());
        let changed_metadata = serde_json::to_vec(&changed_metadata).unwrap();
        let inventory = snapshot::decode(&changed_metadata, changed_bytes.clone()).unwrap();
        assert_ne!(inventory.compressed_sha256, TEACHING_SOURCE_SHA256);
        // CatalogFace lacks these raw fields. The actual byte boundary computes
        // the foreign digest, and source-first Registry admission rejects it.
        assert!(matches!(
            Registry::from_mandatory_trigger_inventory(&inventory, &vocabulary),
            Err(RegistryError::MandatorySourceMismatch)
        ));
        std::fs::write(output.join(format!("{label}.jsonl.gz")), &changed_bytes).unwrap();
        std::fs::write(output.join(format!("{label}.meta.json")), &changed_metadata).unwrap();
        receipts.push(json!({"mutation":label,"field":field,"beforePresent":original.get(field).is_some(),"before":original.get(field),"after":value,"oracleId":selected["oracleId"],"originalRawLineSha256":selected["rawLineSha256IncludingLf"],"changedRawLineSha256":digest(&lines[index]),"changedCompressedSha256":digest(&changed_bytes),"changedCompressedBytes":changed_bytes.len(),"metadataSha256":digest(&changed_metadata),"result":"MandatorySourceMismatch"}));
    }
    let mut bad_metadata: Value = serde_json::from_slice(&metadata).unwrap();
    bad_metadata["compressed_size"] = json!(bytes.len() + 1);
    assert!(matches!(
        snapshot::decode(&serde_json::to_vec(&bad_metadata).unwrap(), bytes.clone()),
        Err(snapshot::SnapshotError::Invalid {
            field: "snapshot",
            ..
        })
    ));
    bad_metadata = serde_json::from_slice(&metadata).unwrap();
    bad_metadata["type"] = json!("default_cards");
    assert!(snapshot::decode(&serde_json::to_vec(&bad_metadata).unwrap(), bytes).is_err());
    std::fs::write(output.join("mutation-receipts.json"),serde_json::to_vec_pretty(&json!({"kind":"actual-full-source-raw-only-negative-admission-evidence","originalSourceSha256":TEACHING_SOURCE_SHA256,"records":receipts,"metadataLengthAndSelectionNegatives":true,"noSeparateCatalogFaceRawTraitValidationClaim":true})).unwrap()).unwrap();
}
