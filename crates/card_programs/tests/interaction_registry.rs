//! Blue/Green finite registry admission and complete authored-program regressions.
//! The small embedded source is synthetic and cannot qualify full-source intake.
use serde_json::Value;
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    BindingStatus, COUNTERSPELL, INTERACTION_PROFILE, INTERACTION_REGISTRY_VERSION, OracleId,
    ProgramKey, Registry, RegistryError, Scope, TEACHING_REGISTRY_VERSION, TEACHING_SOURCE_SHA256,
    TEACHING_VOCABULARY_ID, UNSUMMON,
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
    mana::{ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, ProgramKind, Supertype, ValueExpr,
    },
};
use std::{collections::BTreeSet, path::Path};

fn vocabulary() -> CreatureTypeVocabulary {
    let path = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("interaction tests require the explicit retained source-qualified vocabulary");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        expected()["creatureVocabularyInputSha256"]
            .as_str()
            .unwrap(),
        "actual retained vocabulary input bytes must match the pinned source"
    );
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    let hash =
        CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned()).unwrap();
    let members = raw["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member.as_str().unwrap().to_owned())
        .collect();
    let value = CreatureTypeVocabulary::new(
        raw["rules_revision"].as_str().unwrap().to_owned(),
        *hash.as_bytes(),
        members,
    )
    .unwrap();
    assert_eq!(value.id().to_string(), raw["id"].as_str().unwrap());
    assert_eq!(value.id().to_string(), TEACHING_VOCABULARY_ID);
    value
}
fn subset() -> Inventory {
    snapshot::decode(
        include_bytes!("fixtures/interaction-cards.meta.json"),
        include_bytes!("fixtures/interaction-cards.jsonl.gz").to_vec(),
    )
    .unwrap()
}
/// Only guard unit tests model post-intake identity. These bytes have a different
/// digest and are never evidence for the actual pinned full-source boundary.
fn guard_inventory() -> Inventory {
    let mut source = subset();
    source.compressed_sha256 = TEACHING_SOURCE_SHA256.into();
    source
}
fn expected() -> Value {
    serde_json::from_slice(include_bytes!("fixtures/interaction-cards.expected.json")).unwrap()
}
fn id(value: &str) -> OracleId {
    value.to_owned().try_into().unwrap()
}
fn teaching_subset() -> Inventory {
    snapshot::decode(
        include_bytes!("../../../tests/catalogue/teaching-cards.meta.json"),
        include_bytes!("../../../tests/catalogue/teaching-cards.jsonl.gz").to_vec(),
    )
    .unwrap()
}
fn assert_selected_coverage(registry: &Registry, source_records: usize) {
    let coverage = registry.coverage();
    assert_eq!(coverage.registry_version, INTERACTION_REGISTRY_VERSION);
    assert_eq!(coverage.source_sha256, TEACHING_SOURCE_SHA256);
    assert_eq!(
        coverage.creature_vocabulary_id.to_string(),
        TEACHING_VOCABULARY_ID
    );
    assert_eq!(coverage.source_records, source_records);
    assert_eq!(coverage.required_records, 20);
    assert_eq!(coverage.required_partial_bindings, 20);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    let expected = expected();
    let selected = expected["cards"].as_array().unwrap();
    assert_eq!(coverage.cards.len(), selected.len());
    assert_eq!(
        coverage
            .cards
            .iter()
            .map(|card| card.oracle_id.as_deref().unwrap())
            .collect::<BTreeSet<_>>(),
        selected
            .iter()
            .map(|card| card["oracleId"].as_str().unwrap())
            .collect()
    );
    for expected in selected {
        let card = coverage
            .cards
            .iter()
            .find(|card| card.oracle_id.as_deref() == expected["oracleId"].as_str())
            .unwrap();
        assert_eq!(card.name, expected["name"].as_str().unwrap());
        assert!(card.required);
        assert_eq!(card.behavior_evidence, "not-evaluated-by-registry");
        let BindingStatus::Partial {
            program,
            scope,
            semantic_sha256,
        } = &card.binding
        else {
            panic!("every selected binding must retain conservative partial status");
        };
        assert_eq!(*scope, Scope::InteractionOrdinaryCardV1);
        assert_eq!(
            semantic_sha256,
            expected["registrySignatureV1Sha256"].as_str().unwrap()
        );
        let key = match card.name.as_str() {
            "Island" | "Forest" => ProgramKey::InteractionBasicLandV1,
            "Unsummon" => ProgramKey::InteractionBounceCreatureV1,
            "Counterspell" | "Cancel" => ProgramKey::InteractionCounterSpellV1,
            "Divination" => ProgramKey::InteractionDrawTwoV1,
            _ => ProgramKey::InteractionVanillaCreatureV1,
        };
        assert_eq!(*program, key);
        let binding = registry
            .resolve(&id(card.oracle_id.as_deref().unwrap()))
            .unwrap();
        assert_eq!(binding.key(), key);
        assert_eq!(binding.scope(), *scope);
    }
}
fn printed_cost(value: &str) -> ManaCost {
    let mut cost = ManaCost::free();
    for symbol in value.trim_matches(['{', '}']).split("}{") {
        match symbol {
            "G" | "U" => {
                let mana = if symbol == "G" {
                    ManaType::Green
                } else {
                    ManaType::Blue
                };
                let count = cost.required.entry(mana).or_insert_with(|| 0.into());
                *count = count.plus(&1.into());
            }
            generic => cost.generic = RulesInteger::try_from(generic.to_owned()).unwrap(),
        }
    }
    cost
}
fn assert_complete_programs(registry: &Registry, vocabulary: &CreatureTypeVocabulary) {
    for (ordinal, card) in expected()["cards"].as_array().unwrap().iter().enumerate() {
        let card_id = CardId(u64::try_from(ordinal).unwrap());
        let seed = registry
            .seed(
                &id(card["oracleId"].as_str().unwrap()),
                card_id,
                SeatId(1),
                ObjectDestination::LibraryBottom,
            )
            .unwrap();
        assert_eq!(seed.card, card_id);
        assert_eq!(seed.owner, SeatId(1));
        assert_eq!(seed.destination, ObjectDestination::LibraryBottom);
        let program = seed.program;
        program.validate(vocabulary).unwrap();
        assert_eq!(
            program.names,
            ObjectNames::single(
                EnglishCardName::new(card["name"].as_str().unwrap().into()).unwrap()
            )
        );
        assert!(program.mana.is_empty(), "basics derive intrinsic mana once");
        assert!(program.activated.is_empty());
        assert!(program.triggered.is_empty());
        assert!(program.continuous.is_none());
        assert!(program.color_indicator.is_none());
        assert!(program.effect_defined_colors.is_none());
        let face = &card["faces"][0];
        let expected_subtypes = card["creatureSubtypes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| vocabulary.resolve_label(label.as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(program.creature_subtypes, expected_subtypes);
        match card["implementation"]["family"].as_str().unwrap() {
            "basic-land" => {
                assert_eq!(program.card_types, BTreeSet::from([CardType::Land]));
                assert_eq!(program.supertypes, BTreeSet::from([Supertype::Basic]));
                assert_eq!(
                    program.land_subtypes,
                    BTreeSet::from([if card["name"] == "Island" {
                        LandType::Island
                    } else {
                        LandType::Forest
                    }])
                );
                assert!(program.defined_colors().is_empty());
                let ProgramKind::Permanent(permanent) = &program.kind else {
                    panic!("basic land must be permanent")
                };
                assert!(permanent.casting_cost.is_none());
                assert!(permanent.printed_pt.is_none());
            }
            family => {
                let cost = printed_cost(face["mana_cost"].as_str().unwrap());
                assert_eq!(
                    program.defined_colors(),
                    BTreeSet::from([if cost.required.contains_key(&ManaType::Blue) {
                        Color::Blue
                    } else {
                        Color::Green
                    }])
                );
                assert!(program.supertypes.is_empty());
                assert!(program.land_subtypes.is_empty());
                if family == "vanilla-creature" {
                    assert_eq!(program.card_types, BTreeSet::from([CardType::Creature]));
                    let ProgramKind::Permanent(permanent) = &program.kind else {
                        panic!("creature must be permanent")
                    };
                    assert_eq!(permanent.casting_cost, Some(cost));
                    assert_eq!(
                        permanent.printed_pt,
                        Some(
                            PowerToughness {
                                power: RulesInteger::try_from(
                                    face["power"].as_str().unwrap().to_owned()
                                )
                                .unwrap(),
                                toughness: RulesInteger::try_from(
                                    face["toughness"].as_str().unwrap().to_owned()
                                )
                                .unwrap(),
                            }
                            .into()
                        )
                    );
                } else {
                    assert_eq!(
                        program.card_types,
                        BTreeSet::from([if family == "instant" {
                            CardType::Instant
                        } else {
                            assert_eq!(family, "sorcery");
                            CardType::Sorcery
                        }])
                    );
                    let ProgramKind::Spell(spell) = &program.kind else {
                        panic!("instruction card must have ordinary spell body")
                    };
                    assert_eq!(spell.cost, cost);
                    let body = match card["name"].as_str().unwrap() {
                        "Unsummon" => ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![spellwake_engine::program::Instruction::ZoneOp(Box::new(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ZoneDest::Hand,
                                ),
                            ))],
                        ),
                        "Cancel" | "Counterspell" => ExecutableBody::new(
                            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                            vec![Instruction::StackOp(Box::new(
                                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                                ),
                            ))],
                        ),
                        "Divination" => ExecutableBody::new(
                            BodyTarget::None,
                            vec![spellwake_engine::program::Instruction::DrawCards(Box::new(
                                spellwake_engine::toolbox::DrawCardsSpec::simple(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    ),
                                    ValueExpr::Literal(2.into()),
                                ),
                            ))],
                        ),
                        _ => panic!("unlisted selected spell"),
                    };
                    assert_eq!(spell.body, body);
                }
            }
        }
    }
}

#[test]
fn synthetic_selected_report_has_exact_signatures_keys_and_conservative_coverage() {
    let vocabulary = vocabulary();
    let registry = Registry::from_interaction_inventory(&guard_inventory(), &vocabulary).unwrap();
    assert_selected_coverage(&registry, 20);
    assert_eq!(INTERACTION_REGISTRY_VERSION, 6);
    assert_eq!(INTERACTION_PROFILE, "blue-interaction-v1");
    for (key, wire) in [
        (
            ProgramKey::InteractionBasicLandV1,
            "interaction-basic-land-v1",
        ),
        (
            ProgramKey::InteractionVanillaCreatureV1,
            "interaction-vanilla-creature-v1",
        ),
        (
            ProgramKey::InteractionBounceCreatureV1,
            "interaction-bounce-creature-v1",
        ),
        (
            ProgramKey::InteractionCounterSpellV1,
            "interaction-counter-spell-v1",
        ),
        (ProgramKey::InteractionDrawTwoV1, "interaction-draw-two-v1"),
    ] {
        assert_eq!(serde_json::to_value(key).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_value(Scope::InteractionOrdinaryCardV1).unwrap(),
        "interaction-ordinary-card-v1"
    );
}

#[test]
fn all_twenty_library_seeds_are_complete_ordinary_programs() {
    let vocabulary = vocabulary();
    let registry = Registry::from_interaction_inventory(&guard_inventory(), &vocabulary).unwrap();
    assert_complete_programs(&registry, &vocabulary);
}

#[test]
fn synthetic_subset_and_foreign_source_or_vocabulary_cannot_qualify_full_source_intake() {
    let vocabulary = vocabulary();
    let mut source = subset();
    assert_eq!(
        source.compressed_sha256,
        expected()["subsetSha256"].as_str().unwrap()
    );
    assert!(matches!(
        Registry::from_interaction_inventory(&source, &vocabulary),
        Err(RegistryError::InteractionSourceMismatch)
    ));
    source.compressed_sha256 = "0".repeat(64);
    assert!(matches!(
        Registry::from_interaction_inventory(&source, &vocabulary),
        Err(RegistryError::InteractionSourceMismatch)
    ));
    let foreign = CreatureTypeVocabulary::new(
        "synthetic-foreign-rules".into(),
        *vocabulary.rules_sha256(),
        vocabulary.members().to_vec(),
    )
    .unwrap();
    assert!(matches!(
        Registry::from_interaction_inventory(&guard_inventory(), &foreign),
        Err(RegistryError::InteractionVocabularyMismatch)
    ));
}

#[test]
fn every_selected_missing_duplicate_and_foreign_identity_rejects_before_seeding() {
    let vocabulary = vocabulary();
    for index in 0..20 {
        let mut missing = guard_inventory();
        let affected = missing.cards.remove(index).oracle_id.unwrap();
        assert!(
            matches!(Registry::from_interaction_inventory(&missing, &vocabulary), Err(RegistryError::InteractionMissingIdentity { oracle_id }) if oracle_id == affected)
        );
        let mut duplicate = guard_inventory();
        duplicate.cards.push(subset().cards.remove(index));
        assert!(
            matches!(Registry::from_interaction_inventory(&duplicate, &vocabulary), Err(RegistryError::InteractionDuplicateIdentity { oracle_id }) if oracle_id == affected)
        );
        for changed in [
            None,
            Some("00000000-0000-0000-0000-000000000001"),
            Some("invalid-identity"),
        ] {
            let mut foreign = guard_inventory();
            foreign.cards[index].oracle_id = changed.map(str::to_owned);
            assert!(
                matches!(Registry::from_interaction_inventory(&foreign, &vocabulary), Err(RegistryError::InteractionMissingIdentity { oracle_id }) if oracle_id == affected)
            );
        }
    }
}

#[test]
fn every_selected_materialized_face_field_and_shape_is_guarded() {
    let vocabulary = vocabulary();
    for index in 0..20 {
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
                matches!(Registry::from_interaction_inventory(&source, &vocabulary), Err(RegistryError::InteractionSemanticMismatch { oracle_id }) if oracle_id == affected),
                "index={index} field={field}"
            );
        }
    }
}

#[test]
fn every_selected_saved_modern_legality_and_required_flag_is_guarded() {
    let vocabulary = vocabulary();
    for index in 0..20 {
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
                matches!(Registry::from_interaction_inventory(&source, &vocabulary), Err(RegistryError::InteractionLegalityMismatch { oracle_id }) if oracle_id == affected)
            );
        }
        let mut source = guard_inventory();
        source.cards[index].required_by_base_formats = false;
        assert!(matches!(
            Registry::from_interaction_inventory(&source, &vocabulary),
            Err(RegistryError::InteractionLegalityMismatch { .. })
        ));
    }
}

fn assert_shared_program_equality(source: &Inventory, vocabulary: &CreatureTypeVocabulary) {
    let vanilla = Registry::from_teaching_inventory(source, vocabulary).unwrap();
    let interaction = Registry::from_interaction_inventory(source, vocabulary).unwrap();
    assert_eq!(
        vanilla.coverage().registry_version,
        TEACHING_REGISTRY_VERSION
    );
    let mut shared = 0;
    for selected in &interaction.coverage().cards {
        let oracle = id(selected.oracle_id.as_deref().unwrap());
        if vanilla.resolve(&oracle).is_some() {
            shared += 1;
            let v5 = vanilla
                .seed(
                    &oracle,
                    CardId(91),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )
                .unwrap();
            let v6 = interaction
                .seed(
                    &oracle,
                    CardId(91),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )
                .unwrap();
            assert_eq!(
                v5.program, v6.program,
                "shared program changed for {}",
                selected.name
            );
            assert_eq!(
                vanilla.resolve(&oracle).unwrap().scope(),
                Scope::TeachingOrdinaryCardV1
            );
            assert_eq!(
                interaction.resolve(&oracle).unwrap().scope(),
                Scope::InteractionOrdinaryCardV1
            );
        }
    }
    assert_eq!(shared, 10);
}

#[test]
fn thirty_authored_identity_union_shares_green_programs_and_keeps_pool_and_reference_scopes_separate()
 {
    let vocabulary = vocabulary();
    let mut source = teaching_subset();
    let existing = source
        .cards
        .iter()
        .map(|card| card.oracle_id.clone().unwrap())
        .collect::<BTreeSet<_>>();
    source.cards.extend(
        subset()
            .cards
            .into_iter()
            .filter(|card| !existing.contains(card.oracle_id.as_ref().unwrap())),
    );
    source.compressed_sha256 = TEACHING_SOURCE_SHA256.into();
    assert_eq!(source.cards.len(), 30);
    assert_shared_program_equality(&source, &vocabulary);
    let interaction = Registry::from_interaction_inventory(&source, &vocabulary).unwrap();
    assert_selected_coverage(&interaction, 30);
    let vanilla = Registry::from_teaching_inventory(&source, &vocabulary).unwrap();
    for card in teaching_subset().cards.iter().filter(|card| {
        card.faces[0].mana_cost.as_deref().unwrap().contains('W') || card.name == "Plains"
    }) {
        let oracle = id(card.oracle_id.as_deref().unwrap());
        assert!(interaction.resolve(&oracle).is_none());
        assert!(matches!(
            interaction.seed(
                &oracle,
                CardId(1),
                SeatId(0),
                ObjectDestination::LibraryBottom
            ),
            Err(RegistryError::MissingBinding)
        ));
    }
    for card in subset().cards.iter().filter(|card| {
        card.faces[0].mana_cost.as_deref().unwrap().contains('U') || card.name == "Island"
    }) {
        assert!(
            vanilla
                .resolve(&id(card.oracle_id.as_deref().unwrap()))
                .is_none()
        );
    }
    let reference = Registry::from_inventory(&source, &vocabulary).unwrap();
    assert_eq!(reference.coverage().registry_version, 4);
    assert_eq!(
        reference.resolve(&id(UNSUMMON)).unwrap().key(),
        ProgramKey::UnsummonCreatureV1
    );
    assert_eq!(
        reference.resolve(&id(UNSUMMON)).unwrap().scope(),
        Scope::CreatureTargetOnly
    );
    assert_eq!(
        reference.resolve(&id(COUNTERSPELL)).unwrap().key(),
        ProgramKey::CounterspellSupportedInstantV2
    );
    assert_eq!(
        reference.resolve(&id(COUNTERSPELL)).unwrap().scope(),
        Scope::SupportedInstantSpellTargetOnly
    );
}

#[test]
#[ignore = "requires the retained pinned full catalogue and explicit source-qualified vocabulary"]
fn actual_pinned_full_source_admits_exact_twenty_complete_programs_and_preserves_shared_green_values()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = snapshot::load(
        &root.join("work/catalogue/oracle-metadata.json"),
        &root.join("work/catalogue/oracle-cards-20261029210158.jsonl.gz"),
    )
    .unwrap();
    assert_eq!(source.source_records, 39196);
    assert_eq!(source.cards.len(), 39196);
    assert_eq!(source.compressed_sha256, TEACHING_SOURCE_SHA256);
    let vocabulary = vocabulary();
    let interaction = Registry::from_interaction_inventory(&source, &vocabulary).unwrap();
    assert_selected_coverage(&interaction, 39196);
    assert_complete_programs(&interaction, &vocabulary);
    assert_shared_program_equality(&source, &vocabulary);
    // These byte hashes were compared with the actual predecessor implementations
    // identified by predecessorSourceCodeSha256 in the retained expected fixture.
    let reference = Registry::from_inventory(&source, &vocabulary).unwrap();
    let vanilla = Registry::from_teaching_inventory(&source, &vocabulary).unwrap();
    for (key, coverage) in [
        ("registry4", reference.coverage()),
        ("registry5", vanilla.coverage()),
    ] {
        let bytes = serde_json::to_vec(coverage).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            expected()["predecessorCoverageSha256"][key]
                .as_str()
                .unwrap(),
            "previous selected report bytes changed for {key}"
        );
    }
}
