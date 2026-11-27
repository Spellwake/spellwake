//! Genuine saved-source Registry/Program admission, not capture or complete cards.
use super::*;
use crate::{BindingStatus, Registry};
use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};
use spellwake_contracts::{
    creature_types::CreatureVocabularyId,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    land::LandType,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{DefinitionError, ProgramKind, Supertype},
};
use std::{
    collections::BTreeSet,
    io::BufRead,
    sync::{Arc, OnceLock},
};
const CONSIDER: &str = "4c9bcba6-87b5-4fb3-97ee-6fe5b739337d";
const OPT: &str = "713332c1-5bd8-400f-bfff-c1ca0697a043";
const SCOUR: &str = "83101ba8-a569-4827-8c53-9ca0dfcd59a7";
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
struct Genuine {
    metadata: Vec<u8>,
    compressed: Vec<u8>,
    inventory: Inventory,
    vocabulary: Arc<CreatureTypeVocabulary>,
    registry: Registry,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn input(key: &str) -> Vec<u8> {
    std::fs::read(
        std::env::var_os(key).expect("explicit genuine saved input required, no fallback"),
    )
    .unwrap()
}
fn genuine() -> &'static Genuine {
    static INPUT: OnceLock<Genuine> = OnceLock::new();
    INPUT.get_or_init(|| {
        let metadata = input("SPELLWAKE_CATALOGUE_METADATA");
        let compressed = input("SPELLWAKE_CATALOGUE_INPUT");
        let bytes = input("SPELLWAKE_CREATURE_VOCABULARY_INPUT");
        assert_eq!(
            hash(&metadata),
            "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad"
        );
        assert_eq!(hash(&compressed), crate::TEACHING_SOURCE_SHA256);
        assert_eq!(
            hash(&bytes),
            "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c"
        );
        let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let rules =
            CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned())
                .unwrap();
        let vocabulary = Arc::new(
            CreatureTypeVocabulary::new(
                raw["rules_revision"].as_str().unwrap().to_owned(),
                *rules.as_bytes(),
                raw["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect(),
            )
            .unwrap(),
        );
        assert_eq!(vocabulary.id().to_string(), crate::TEACHING_VOCABULARY_ID);
        let inventory = spellwake_content::snapshot::decode(&metadata, compressed.clone()).unwrap();
        let registry = Registry::from_library_top_inventory(&inventory, &vocabulary).unwrap();
        Genuine {
            metadata,
            compressed,
            inventory,
            vocabulary,
            registry,
        }
    })
}
fn id(raw: &str) -> OracleId {
    raw.to_owned().try_into().unwrap()
}
fn program(g: &Genuine, oracle: &str) -> Program {
    g.registry
        .seed(
            &id(oracle),
            CardId(1),
            SeatId(0),
            ObjectDestination::LibraryBottom,
        )
        .unwrap()
        .program
}
#[test]
#[ignore = "requires explicit genuine complete metadata/gzip and source-qualified vocabulary"]
fn exact_four_raw_rows_native_source_signatures_and_complete_programs() {
    let g = genuine();
    let expected = [
        (
            11672,
            CONSIDER,
            "500dc323d0212b13cbde39c76f3e24463d617f8dbae380a0ddc55ed4404de847",
        ),
        (
            17254,
            OPT,
            "af33207fec1a5976505ee423c1c2a28de665bbe1ebc6ab5baf63ca0fac691b82",
        ),
        (
            19959,
            SCOUR,
            "fe7f5432af54b62be16c2e85d26c51a5e27c523b95a23014c091318bf5eca33e",
        ),
        (
            27256,
            ISLAND,
            "a5365e49cba1ca36f32f871fefeeea86ee366480b71c34485f306b744cfcf938",
        ),
    ];
    let mut matched = 0;
    let mut reader = std::io::BufReader::new(MultiGzDecoder::new(g.compressed.as_slice()));
    let mut raw = vec![];
    for number in 1..=39196 {
        raw.clear();
        assert!(reader.read_until(b'\n', &mut raw).unwrap() > 0);
        if let Some((_, oracle, digest)) = expected.iter().find(|r| r.0 == number) {
            assert_eq!(hash(&raw), *digest);
            assert!(raw.ends_with(b"\n"));
            let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(value["oracle_id"], *oracle);
            let selected = CARDS.iter().find(|c| c.oracle_id == *oracle).unwrap();
            assert_eq!(value["id"], selected.printing_id);
            matched += 1;
        }
    }
    assert_eq!(reader.read_until(b'\n', &mut raw).unwrap(), 0);
    assert_eq!(matched, 4);
    let coverage = g.registry.coverage();
    assert_eq!(
        (
            coverage.registry_version,
            coverage.cards.len(),
            coverage.required_records,
            coverage.required_partial_bindings,
            coverage.required_without_binding,
            coverage.fully_qualified_cards
        ),
        (11, 4, 4, 4, 0, 0)
    );
    for (oracle, name, action, target, key) in [
        (
            OPT,
            "Opt",
            Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(
                ResolvingPlayer::Controller,
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ))),
            BodyTarget::None,
            ProgramKey::LibraryTopScryDrawSpellV1,
        ),
        (
            CONSIDER,
            "Consider",
            Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            )),
            BodyTarget::None,
            ProgramKey::LibraryTopSurveilDrawSpellV1,
        ),
        (
            SCOUR,
            "Thought Scour",
            Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::mill(
                ResolvingPlayer::TargetPlayer,
                spellwake_engine::program::ValueExpr::Literal(2.into()),
            ))),
            BodyTarget::SINGLE_PLAYER,
            ProgramKey::LibraryTopMillDrawSpellV1,
        ),
    ] {
        let mut expected = Program::instant(
            ManaCost::specific(ManaType::Blue, 1.into()),
            ExecutableBody::new(
                target,
                vec![
                    action,
                    Instruction::DrawCards(Box::new(
                        spellwake_engine::toolbox::DrawCardsSpec::simple(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                    )),
                ],
            ),
        );
        expected.names = ObjectNames::single(EnglishCardName::new(name.into()).unwrap());
        let actual = program(g, oracle);
        assert_eq!(actual, expected);
        actual.validate(&g.vocabulary).unwrap();
        assert_eq!(g.registry.resolve(&id(oracle)).unwrap().key(), key);
        assert_eq!(
            g.registry.resolve(&id(oracle)).unwrap().scope(),
            Scope::LibraryTopOrdinaryCardV1
        );
    }
    let mut island = Program::land(BTreeSet::from([LandType::Island]), vec![]);
    island.names = ObjectNames::single(EnglishCardName::new("Island".into()).unwrap());
    island.supertypes.insert(Supertype::Basic);
    assert_eq!(program(g, ISLAND), island);
    for row in &coverage.cards {
        let oracle = row.oracle_id.as_ref().unwrap();
        let card = g
            .inventory
            .cards
            .iter()
            .find(|c| c.oracle_id.as_ref() == Some(oracle))
            .unwrap();
        let BindingStatus::Partial {
            semantic_sha256,
            scope,
            ..
        } = &row.binding
        else {
            panic!("partial")
        };
        assert_eq!(
            *semantic_sha256,
            crate::signature(card, &id(oracle)).unwrap()
        );
        assert_eq!(*scope, Scope::LibraryTopOrdinaryCardV1);
    }
}
#[test]
#[ignore = "requires genuine full source; malformed materialized inputs are negative controls"]
fn source_envelope_selected_identity_printing_and_complete_face_corruptions_refuse_atomically() {
    let g = genuine();
    let mut inventory =
        spellwake_content::snapshot::decode(&g.metadata, g.compressed.clone()).unwrap();
    let assert_error = |inventory: &Inventory, expected: LibraryTopRegistryError| {
        let before = serde_json::to_vec(inventory).unwrap();
        let error = Registry::from_library_top_inventory(inventory, &g.vocabulary)
            .err()
            .unwrap();
        assert!(
            matches!(&error,RegistryError::LibraryTop(actual) if actual==&expected),
            "{error:?}"
        );
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<LibraryTopRegistryError>(),
            Some(&expected)
        );
        assert_eq!(serde_json::to_vec(inventory).unwrap(), before);
    };
    let saved = std::mem::replace(&mut inventory.compressed_sha256, "foreign".into());
    assert_error(&inventory, LibraryTopRegistryError::SourceMismatch);
    inventory.compressed_sha256 = saved;
    let saved = std::mem::replace(&mut inventory.source.updated_at, "foreign".into());
    assert_error(&inventory, LibraryTopRegistryError::SourceMismatch);
    inventory.source.updated_at = saved;
    inventory.base_format_inventory_resolved = false;
    assert_error(&inventory, LibraryTopRegistryError::SourceMismatch);
    inventory.base_format_inventory_resolved = true;
    let index = inventory
        .cards
        .iter()
        .position(|c| c.oracle_id.as_deref() == Some(CONSIDER))
        .unwrap();
    let other = inventory
        .cards
        .iter()
        .position(|c| {
            !CARDS
                .iter()
                .any(|s| c.oracle_id.as_deref() == Some(s.oracle_id))
        })
        .unwrap();
    let saved = inventory.cards[index].oracle_id.take();
    assert_error(
        &inventory,
        LibraryTopRegistryError::MissingIdentity {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[index].oracle_id = saved;
    let saved = inventory.cards[other].oracle_id.replace(CONSIDER.into());
    assert_error(
        &inventory,
        LibraryTopRegistryError::DuplicateIdentity {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[other].oracle_id = saved;
    let saved = std::mem::replace(&mut inventory.cards[index].scryfall_id, "foreign".into());
    assert_error(
        &inventory,
        LibraryTopRegistryError::PrintingMismatch {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[index].scryfall_id = saved;
    let selected_printing = inventory.cards[index].scryfall_id.clone();
    let saved = std::mem::replace(&mut inventory.cards[other].scryfall_id, selected_printing);
    assert_error(
        &inventory,
        LibraryTopRegistryError::PrintingMismatch {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[other].scryfall_id = saved;
    let saved = std::mem::replace(&mut inventory.cards[index].layout, "transform".into());
    assert_error(
        &inventory,
        LibraryTopRegistryError::SemanticMismatch {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[index].layout = saved;
    for field in 0..5 {
        let face = &mut inventory.cards[index].faces[0];
        let slot = match field {
            0 => &mut face.oracle_text,
            1 => &mut face.mana_cost,
            2 => &mut face.type_line,
            3 => &mut face.power,
            _ => &mut face.toughness,
        };
        let saved = slot.replace("changed".into());
        assert_error(
            &inventory,
            LibraryTopRegistryError::SemanticMismatch {
                oracle_id: CONSIDER,
            },
        );
        let face = &mut inventory.cards[index].faces[0];
        *match field {
            0 => &mut face.oracle_text,
            1 => &mut face.mana_cost,
            2 => &mut face.type_line,
            3 => &mut face.power,
            _ => &mut face.toughness,
        } = saved;
    }
    inventory.cards[index].required_by_base_formats = false;
    assert_error(
        &inventory,
        LibraryTopRegistryError::SavedEligibilityMismatch {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[index].required_by_base_formats = true;
    let saved = inventory.cards[index]
        .legalities
        .insert("legacy".into(), Legality::Banned);
    assert_error(
        &inventory,
        LibraryTopRegistryError::SavedEligibilityMismatch {
            oracle_id: CONSIDER,
        },
    );
    inventory.cards[index]
        .legalities
        .insert("legacy".into(), saved.unwrap());
    Registry::from_library_top_inventory(&inventory, &g.vocabulary).unwrap();
    let foreign =
        CreatureTypeVocabulary::new("foreign".into(), [1; 32], vec!["Bear".into()]).unwrap();
    assert!(matches!(
        Registry::from_library_top_inventory(&inventory, &foreign),
        Err(RegistryError::LibraryTop(
            LibraryTopRegistryError::VocabularyMismatch
        ))
    ));
}
#[test]
#[ignore = "requires genuine complete source and native vocabulary; exact old profile ownership"]
fn old_profiles_stay_closed_and_island_retains_the_single_maintained_definition() {
    let g = genuine();
    for (registry, count, scope) in [
        (
            Registry::from_resolution_selection_inventory(&g.inventory, &g.vocabulary).unwrap(),
            9,
            Scope::ResolutionSelectionOrdinaryCardV1,
        ),
        (
            Registry::from_resolution_selection_natural_mana_inventory(&g.inventory, &g.vocabulary)
                .unwrap(),
            12,
            Scope::ResolutionSelectionOrdinaryCardV1,
        ),
        (
            Registry::from_interaction_inventory(&g.inventory, &g.vocabulary).unwrap(),
            20,
            Scope::InteractionOrdinaryCardV1,
        ),
        (
            Registry::from_mandatory_trigger_inventory(&g.inventory, &g.vocabulary).unwrap(),
            19,
            Scope::MandatoryTriggerOrdinaryCardV1,
        ),
        (
            Registry::from_optional_payment_inventory(&g.inventory, &g.vocabulary).unwrap(),
            19,
            Scope::OptionalPaymentOrdinaryCardV1,
        ),
    ] {
        assert_eq!(registry.coverage().cards.len(), count);
        for oracle in [OPT, CONSIDER, SCOUR] {
            assert!(registry.resolve(&id(oracle)).is_none());
        }
        assert_eq!(registry.resolve(&id(ISLAND)).unwrap().scope(), scope);
        assert_eq!(
            registry
                .seed(
                    &id(ISLAND),
                    CardId(1),
                    SeatId(0),
                    ObjectDestination::LibraryBottom
                )
                .unwrap()
                .program,
            program(g, ISLAND)
        );
    }
    assert!(
        g.registry
            .resolve(&id("058917c1-21ab-488a-9f9c-591c55f3c596"))
            .is_none()
    );
}
#[test]
#[ignore = "requires genuine bound Programs; malformed definitions are native negative controls"]
fn native_admission_rejects_invalid_amount_and_player_role_but_source_digest_is_not_program_authority()
 {
    let g = genuine();
    let mut actual = program(g, SCOUR);
    let ProgramKind::Spell(spell) = &mut actual.kind else {
        panic!("spell")
    };
    spell.body.target = BodyTarget::None;
    assert_eq!(
        actual.validate(&g.vocabulary),
        Err(DefinitionError::ResolvingPlayerTarget)
    );
    let mut actual = program(g, OPT);
    let ProgramKind::Spell(spell) = &mut actual.kind else {
        panic!("spell")
    };
    spell.body.instructions[0] =
        Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(
            ResolvingPlayer::Controller,
            spellwake_engine::program::ValueExpr::Literal((-1).into()),
        )));
    assert_eq!(
        actual.validate(&g.vocabulary),
        Err(DefinitionError::InvalidInstruction)
    );
    let coverage = serde_json::to_vec(g.registry.coverage()).unwrap();
    let mut actual = program(g, OPT);
    let ProgramKind::Spell(spell) = &mut actual.kind else {
        panic!("spell")
    };
    spell.body.instructions.reverse();
    actual.validate(&g.vocabulary).unwrap();
    assert_ne!(actual, program(g, OPT));
    assert_eq!(serde_json::to_vec(g.registry.coverage()).unwrap(), coverage);
}

mod paid;
