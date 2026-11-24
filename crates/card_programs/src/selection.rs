//! Exact saved-source materialization for the finite sacrifice/discard pool.
//! Oracle IDs choose authored definitions here, never behavior at runtime. Source
//! hashes do not include Program operands, and no binding is a qualified card.
use super::{Definition, OracleId, ProgramKey, RegistryError, Scope, TeachingPool};
use spellwake_content::{Inventory, Legality};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    life::LifeAdjustment,
    mana::{ManaCost, ManaType},
    program::{
        AffectedPlayers, BodyTarget, CardType, ExecutableBody, Instruction, PermanentTypeFilter,
        Program, ResolvingPlayer, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    },
};
use std::collections::BTreeSet;

/// Saved-source binding revision for the finite selected-nine resolution pool.
pub const RESOLUTION_SELECTION_REGISTRY_VERSION: u32 = 9;
/// Internal source pool, separate from complete-card and game qualification.
pub const RESOLUTION_SELECTION_PROFILE: &str = "resolution-sacrifice-discard-v1";

#[derive(Clone, Copy)]
enum Kind {
    Edict,
    Verdict,
    Blood,
    MindRot,
    Skeins,
    Fleshbag,
    Helper(ProgramKey),
}
struct SelectedCard {
    oracle_id: &'static str,
    printing_id: &'static str,
    source_signature_v1_sha256: &'static str,
    kind: Kind,
}
const CARDS: [SelectedCard; 9] = [
    SelectedCard {
        oracle_id: "058917c1-21ab-488a-9f9c-591c55f3c596",
        printing_id: "4eabbed2-1399-4cf1-9eba-b53c56caced4",
        source_signature_v1_sha256: "7ca8c8e940949dbfa844439711302de45ab8ed2f46675ca0f86b1601931422cd",
        kind: Kind::Edict,
    },
    SelectedCard {
        oracle_id: "0f575f18-4606-4b85-9a90-07f8d8d46d06",
        printing_id: "7a20b5a2-8613-49ed-b5cc-7cae9d0e0850",
        source_signature_v1_sha256: "e127be3ef7411807dd03ecc12ea108c2347c3af67abcf28e250f93a2baf3b9eb",
        kind: Kind::Verdict,
    },
    SelectedCard {
        oracle_id: "14c8f55d-d177-4c25-a931-ebeb9e6062a0",
        printing_id: "409f9b88-f03e-40b6-9883-68c14c37c0de",
        source_signature_v1_sha256: "f0a0e899618134ff97f4c9fa6fbadc2968959374cff030528788dcd43144887b",
        kind: Kind::Helper(ProgramKey::ResolutionSelectionVanillaCreatureV1),
    },
    SelectedCard {
        oracle_id: "4b1bf05e-753e-4350-a913-894cf3cecc0c",
        printing_id: "fce2baa4-2976-4bbd-b6c5-a5a3c6a901be",
        source_signature_v1_sha256: "a332d01376315034cf0c9b6a51aabc9a585fbdb657a80ba917753186d0a1594f",
        kind: Kind::Fleshbag,
    },
    SelectedCard {
        oracle_id: "5b2364d7-a811-4595-a1b4-224c70555ffa",
        printing_id: "6c7c8527-55f6-494d-b4f7-c427a5735053",
        source_signature_v1_sha256: "e1130e72e141cee5b5bfc368b04325703c344a4d6f0860eef07fc018835d0a25",
        kind: Kind::Helper(ProgramKey::ResolutionSelectionTokenCreationSpellV1),
    },
    SelectedCard {
        oracle_id: "6791ec3c-c397-4087-8c8c-84d3797df415",
        printing_id: "29184c64-03f3-4a50-ac18-e34b6c89635e",
        source_signature_v1_sha256: "1f989956e473281c39318da00e53884b4670dc4294f5a0ab7658ab898be1cecf",
        kind: Kind::Blood,
    },
    SelectedCard {
        oracle_id: "7397036f-8114-47cc-b52f-c532a6845d16",
        printing_id: "64b0d9e7-4a0f-4f07-99ae-31c3c9f0037a",
        source_signature_v1_sha256: "ea64e0fe0779eefc69f8c269ab94b7cb75ebcaf200ca3a16911b6f32fbd29fce",
        kind: Kind::Skeins,
    },
    SelectedCard {
        oracle_id: "ad44cf74-b717-48fb-9fa2-77512024d76a",
        printing_id: "833a8604-92d5-443b-9bc0-bd91c973ef07",
        source_signature_v1_sha256: "54c8155e10628092c85972f944622a0fbcdfc692016101ffb9dfd0617b58cbb7",
        kind: Kind::MindRot,
    },
    SelectedCard {
        oracle_id: "b2c6aa39-2d2a-459c-a555-fb48ba993373",
        printing_id: "1335d293-3dd2-4f13-bd2c-22181f4528ee",
        source_signature_v1_sha256: "f7794b130f48b16234927c7fb97a10e5ea44b2ec064d8d8856aeedc7904ac678",
        kind: Kind::Helper(ProgramKey::ResolutionSelectionBasicLandV1),
    },
];

pub(super) fn selected(oracle_id: &str) -> bool {
    CARDS.iter().any(|card| card.oracle_id == oracle_id)
}

fn sacrifice(players: AffectedPlayers) -> Instruction {
    Instruction::Sacrifice {
        players,
        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
        filter: PermanentTypeFilter {
            any_types: Default::default(),
            required_types: BTreeSet::from([CardType::Creature]),
            forbidden_types: BTreeSet::new(),
            object_filter: None,
        },
    }
}
fn cost(generic: i64, black: i64) -> ManaCost {
    let mut cost = ManaCost::specific(ManaType::Black, black.into());
    cost.generic = generic.into();
    cost
}

pub(super) fn definition(
    id: &OracleId,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<Definition>, RegistryError> {
    let Some(card) = CARDS.iter().find(|card| card.oracle_id == id.as_str()) else {
        return Ok(None);
    };
    let targeted = AffectedPlayers::One(ResolvingPlayer::TargetPlayer);
    let scope = Scope::ResolutionSelectionOrdinaryCardV1;
    let (key, name, mana, type_line, text, power, toughness, program) = match card.kind {
        Kind::Helper(key) => {
            // Preserve one maintained owner for Bears, Island and Raise's full
            // nested Soldier token definition. Only profile attribution changes.
            let Some(mut def) =
                super::teaching::definition(id, vocabulary, TeachingPool::Mandatory)?
            else {
                return Err(RegistryError::ResolutionSelectionMissingIdentity {
                    oracle_id: card.oracle_id,
                });
            };
            def.key = key;
            def.scope = scope;
            return Ok(Some(def));
        }
        Kind::Edict => (
            ProgramKey::ResolutionSelectionTargetedSacrificeSpellV1,
            "Diabolic Edict",
            "{1}{B}",
            "Instant",
            "Target player sacrifices a creature of their choice.",
            None,
            None,
            Program::instant(
                cost(1, 1),
                ExecutableBody::new(BodyTarget::SinglePlayer, vec![sacrifice(targeted)]),
            ),
        ),
        Kind::Verdict => (
            ProgramKey::ResolutionSelectionSacrificeLifeLossSpellV1,
            "Geth's Verdict",
            "{B}{B}",
            "Instant",
            "Target player sacrifices a creature of their choice and loses 1 life.",
            None,
            None,
            Program::instant(
                cost(0, 2),
                ExecutableBody::new(
                    BodyTarget::SinglePlayer,
                    vec![
                        sacrifice(targeted),
                        Instruction::AdjustTargetLife {
                            adjustment: LifeAdjustment::Lose(1.into()),
                        },
                    ],
                ),
            ),
        ),
        Kind::Blood => (
            ProgramKey::ResolutionSelectionEachPlayerSacrificeSpellV1,
            "Innocent Blood",
            "{B}",
            "Sorcery",
            "Each player sacrifices a creature of their choice.",
            None,
            None,
            Program::sorcery(
                cost(0, 1),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![sacrifice(AffectedPlayers::EachPlayer)],
                ),
            ),
        ),
        Kind::MindRot => (
            ProgramKey::ResolutionSelectionTargetedDiscardSpellV1,
            "Mind Rot",
            "{2}{B}",
            "Sorcery",
            "Target player discards two cards.",
            None,
            None,
            Program::sorcery(
                cost(2, 1),
                ExecutableBody::new(
                    BodyTarget::SinglePlayer,
                    vec![Instruction::Discard {
                        players: targeted,
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                    }],
                ),
            ),
        ),
        Kind::Skeins => (
            ProgramKey::ResolutionSelectionEachPlayerDiscardSpellV1,
            "Delirium Skeins",
            "{2}{B}",
            "Sorcery",
            "Each player discards three cards.",
            None,
            None,
            Program::sorcery(
                cost(2, 1),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::EachPlayer,
                        amount: spellwake_engine::program::ValueExpr::Literal(3.into()),
                    }],
                ),
            ),
        ),
        Kind::Fleshbag => {
            let subtypes = ["Zombie", "Warrior"]
                .into_iter()
                .map(|name| {
                    vocabulary
                        .resolve_label(name)
                        .ok_or(RegistryError::MissingCreatureType)
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            let mut program = Program::creature(Some(cost(2, 1)), 3.into(), 1.into(), subtypes);
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                grouping: TriggerGrouping::EachOccurrence,
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![sacrifice(AffectedPlayers::EachPlayer)],
                ),
            });
            (
                ProgramKey::ResolutionSelectionTriggeredCreatureV1,
                "Fleshbag Marauder",
                "{2}{B}",
                "Creature — Zombie Warrior",
                "When this creature enters, each player sacrifices a creature of their choice.",
                Some("3"),
                Some("1"),
                program,
            )
        }
    };
    Ok(Some(Definition {
        key,
        scope,
        name,
        mana,
        type_line,
        text,
        power,
        toughness,
        program,
    }))
}

pub(super) fn validate_inventory(
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), RegistryError> {
    if !super::saved_inventory_matches(inventory) {
        return Err(RegistryError::ResolutionSelectionSourceMismatch);
    }
    if vocabulary.id().to_string() != super::TEACHING_VOCABULARY_ID {
        return Err(RegistryError::ResolutionSelectionVocabularyMismatch);
    }
    for expected in &CARDS {
        let mut matching = inventory
            .cards
            .iter()
            .filter(|card| card.oracle_id.as_deref() == Some(expected.oracle_id));
        let card = matching
            .next()
            .ok_or(RegistryError::ResolutionSelectionMissingIdentity {
                oracle_id: expected.oracle_id,
            })?;
        if matching.next().is_some() {
            return Err(RegistryError::ResolutionSelectionDuplicateIdentity {
                oracle_id: expected.oracle_id,
            });
        }
        if card.scryfall_id != expected.printing_id
            || inventory
                .cards
                .iter()
                .filter(|row| row.scryfall_id == expected.printing_id)
                .count()
                != 1
        {
            return Err(RegistryError::ResolutionSelectionPrintingMismatch {
                oracle_id: expected.oracle_id,
            });
        }
        if !card.required_by_base_formats || card.legalities.get("legacy") != Some(&Legality::Legal)
        {
            return Err(RegistryError::ResolutionSelectionLegalityMismatch {
                oracle_id: expected.oracle_id,
            });
        }
        let id = OracleId(expected.oracle_id.to_owned());
        let def = definition(&id, vocabulary)?.ok_or(
            RegistryError::ResolutionSelectionMissingIdentity {
                oracle_id: expected.oracle_id,
            },
        )?;
        if !super::matches(card, &def)
            || super::signature(card, &id)? != expected.source_signature_v1_sha256
        {
            return Err(RegistryError::ResolutionSelectionSemanticMismatch {
                oracle_id: expected.oracle_id,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BindingStatus, Registry};
    use flate2::read::MultiGzDecoder;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use spellwake_contracts::{
        creature_types::CreatureVocabularyId,
        names::{EnglishCardName, ObjectNames},
    };
    use spellwake_engine::{
        object_identity::{CardId, ObjectDestination},
        priority::SeatId,
        program::ProgramKind,
    };
    use std::io::Read;

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn vocabulary() -> CreatureTypeVocabulary {
        let bytes = std::fs::read(
            std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
                .expect("explicit source-qualified vocabulary required"),
        )
        .unwrap();
        assert_eq!(
            digest(&bytes),
            "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c"
        );
        let raw: Value = serde_json::from_slice(&bytes).unwrap();
        let hash = CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned())
            .unwrap();
        let value = CreatureTypeVocabulary::new(
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
        assert_eq!(value.id().to_string(), crate::TEACHING_VOCABULARY_ID);
        value
    }
    fn full_inputs() -> (Vec<u8>, Vec<u8>) {
        let metadata = std::fs::read(
            std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
                .expect("explicit actual complete metadata required"),
        )
        .unwrap();
        let bytes = std::fs::read(
            std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
                .expect("explicit actual complete source required"),
        )
        .unwrap();
        assert_eq!(digest(&bytes), crate::TEACHING_SOURCE_SHA256);
        (metadata, bytes)
    }
    fn source() -> Inventory {
        let (metadata, bytes) = full_inputs();
        spellwake_content::snapshot::decode(&metadata, bytes).unwrap()
    }
    fn id(raw: &str) -> OracleId {
        raw.to_owned().try_into().unwrap()
    }
    fn index(source: &Inventory, oracle: &str) -> usize {
        source
            .cards
            .iter()
            .position(|card| card.oracle_id.as_deref() == Some(oracle))
            .unwrap()
    }

    #[test]
    fn selection_complete_new_programs_equal_independent_saved_clause_expectations() {
        // Expected complete native values come from the selected saved clauses,
        // not a clone of definition() or its production cost/effect helpers.
        let vocabulary = vocabulary();
        let targeted = AffectedPlayers::One(ResolvingPlayer::TargetPlayer);
        let exact_cost = |generic: i64, black: i64| ManaCost {
            generic: generic.into(),
            required: std::collections::BTreeMap::from([(ManaType::Black, black.into())]),
        };
        let expected_sacrifice = |players| Instruction::Sacrifice {
            players,
            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            filter: PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        };
        let mut fleshbag = Program::creature(
            Some(exact_cost(2, 1)),
            3.into(),
            1.into(),
            ["Zombie", "Warrior"]
                .into_iter()
                .map(|name| vocabulary.resolve_label(name).unwrap())
                .collect(),
        );
        fleshbag.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            grouping: TriggerGrouping::EachOccurrence,
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![expected_sacrifice(AffectedPlayers::EachPlayer)],
            ),
        });
        let expected = [
            (
                "058917c1-21ab-488a-9f9c-591c55f3c596",
                Program::instant(
                    exact_cost(1, 1),
                    ExecutableBody::new(
                        BodyTarget::SinglePlayer,
                        vec![expected_sacrifice(targeted)],
                    ),
                ),
            ),
            (
                "0f575f18-4606-4b85-9a90-07f8d8d46d06",
                Program::instant(
                    exact_cost(0, 2),
                    ExecutableBody::new(
                        BodyTarget::SinglePlayer,
                        vec![
                            expected_sacrifice(targeted),
                            Instruction::AdjustTargetLife {
                                adjustment: LifeAdjustment::Lose(1.into()),
                            },
                        ],
                    ),
                ),
            ),
            (
                "6791ec3c-c397-4087-8c8c-84d3797df415",
                Program::sorcery(
                    exact_cost(0, 1),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![expected_sacrifice(AffectedPlayers::EachPlayer)],
                    ),
                ),
            ),
            (
                "ad44cf74-b717-48fb-9fa2-77512024d76a",
                Program::sorcery(
                    exact_cost(2, 1),
                    ExecutableBody::new(
                        BodyTarget::SinglePlayer,
                        vec![Instruction::Discard {
                            players: targeted,
                            amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                        }],
                    ),
                ),
            ),
            (
                "7397036f-8114-47cc-b52f-c532a6845d16",
                Program::sorcery(
                    exact_cost(2, 1),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::Discard {
                            players: AffectedPlayers::EachPlayer,
                            amount: spellwake_engine::program::ValueExpr::Literal(3.into()),
                        }],
                    ),
                ),
            ),
            ("4b1bf05e-753e-4350-a913-894cf3cecc0c", fleshbag),
        ];
        for (oracle, expected) in expected {
            let actual = definition(&id(oracle), &vocabulary)
                .unwrap()
                .unwrap()
                .program;
            assert_eq!(
                actual, expected,
                "complete authored source Program {oracle}"
            );
            actual.validate(&vocabulary).unwrap();
        }
    }

    #[test]
    fn selection_authored_definitions_admit_complete_clauses_and_reuse_helper_owners() {
        let vocabulary = vocabulary();
        for card in &CARDS {
            let id = id(card.oracle_id);
            let def = definition(&id, &vocabulary).unwrap().unwrap();
            def.program.validate(&vocabulary).unwrap();
            assert_eq!(def.scope, Scope::ResolutionSelectionOrdinaryCardV1);
            assert!(def.program.activated.is_empty());
            assert!(def.program.continuous.is_none());
            match card.kind {
                Kind::Helper(_) => assert_eq!(
                    def.program,
                    crate::teaching::definition(&id, &vocabulary, TeachingPool::Mandatory)
                        .unwrap()
                        .unwrap()
                        .program
                ),
                Kind::Fleshbag => {
                    assert!(matches!(def.program.kind, ProgramKind::Permanent(_)));
                    assert_eq!(
                        def.program.triggered,
                        vec![TriggeredAbility {
                            id: TriggeredAbilityId(1),
                            grouping: TriggerGrouping::EachOccurrence,
                            condition: spellwake_engine::program::TriggerSpec::enters(
                                spellwake_engine::program::ObjectFilter::self_object()
                            ),
                            body: ExecutableBody::new(
                                BodyTarget::None,
                                vec![sacrifice(AffectedPlayers::EachPlayer)]
                            ),
                        }]
                    );
                    assert_eq!(
                        def.program.creature_subtypes,
                        ["Zombie", "Warrior"]
                            .into_iter()
                            .map(|name| vocabulary.resolve_label(name).unwrap())
                            .collect()
                    );
                }
                Kind::Verdict => {
                    let ProgramKind::Spell(spell) = def.program.kind else {
                        panic!("instant expected")
                    };
                    assert_eq!(spell.cost, ManaCost::specific(ManaType::Black, 2.into()));
                    assert_eq!(spell.body.target, BodyTarget::SinglePlayer);
                    assert_eq!(
                        spell.body.instructions,
                        vec![
                            sacrifice(AffectedPlayers::One(ResolvingPlayer::TargetPlayer)),
                            Instruction::AdjustTargetLife {
                                adjustment: LifeAdjustment::Lose(1.into())
                            }
                        ]
                    );
                }
                Kind::Edict | Kind::Blood | Kind::MindRot | Kind::Skeins => {
                    let ProgramKind::Spell(spell) = def.program.kind else {
                        panic!("spell expected")
                    };
                    assert_eq!(spell.body.instructions.len(), 1);
                    assert!(def.program.triggered.is_empty());
                    assert_eq!(
                        spell.body.target == BodyTarget::SinglePlayer,
                        matches!(card.kind, Kind::Edict | Kind::MindRot)
                    );
                    match (&spell.body.instructions[0], card.kind) {
                        (
                            Instruction::Sacrifice {
                                players,
                                amount,
                                filter,
                            },
                            Kind::Edict | Kind::Blood,
                        ) => {
                            assert_eq!(
                                *players,
                                if matches!(card.kind, Kind::Edict) {
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                } else {
                                    AffectedPlayers::EachPlayer
                                }
                            );
                            assert_eq!(
                                *amount,
                                spellwake_engine::program::ValueExpr::Literal(1.into())
                            );
                            assert_eq!(filter.required_types, BTreeSet::from([CardType::Creature]));
                            assert!(filter.forbidden_types.is_empty());
                        }
                        (
                            Instruction::Discard { players, amount },
                            Kind::MindRot | Kind::Skeins,
                        ) => {
                            assert_eq!(
                                *players,
                                if matches!(card.kind, Kind::MindRot) {
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                } else {
                                    AffectedPlayers::EachPlayer
                                }
                            );
                            assert_eq!(
                                *amount,
                                spellwake_engine::program::ValueExpr::Literal(
                                    if matches!(card.kind, Kind::MindRot) {
                                        2
                                    } else {
                                        3
                                    }
                                    .into()
                                )
                            );
                        }
                        _ => panic!("source clause differs from complete authored instruction"),
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires actual complete dated catalogue/metadata and source-qualified vocabulary; no subset fallback"]
    fn actual_selection_nine_raw_source_joins_and_complete_program_materialization() {
        const LINES: [(usize, &str); 9] = [
            (
                888,
                "9b7df481a8a2c666978ae6e768adc835e5e921b52c8e834889c9f4c236ca4594",
            ),
            (
                2333,
                "bc641160ded619a454719b8b1391de59421290fa1e29ecda9b8d38ee9e28b711",
            ),
            (
                3221,
                "3e51dcca3d60899dd6d213ace72ab7fc333b40bbb1c04b20d34de1283b5df04b",
            ),
            (
                11435,
                "cbe5abb0b9cf4493e6cd9bf9d54020e98cc49e948cb2d7022e829ed45af7fd22",
            ),
            (
                13868,
                "495b615dc6de83baec52786c1b8d345726e15db3320dcd11f8175e2a72ce3bad",
            ),
            (
                15761,
                "7c9790e61836e1f7e4ed88fd37f14aa8dd37855b86dc5b099b343a9ee5f637a7",
            ),
            (
                17619,
                "7eee4cbd91c6746d22cd14627895a13bddd03bb1abcc53941de60047cef3bb78",
            ),
            (
                26417,
                "ca8ebb746b2728bec9ad33a1a0e9dc93324b7decec18c895898f212af6c381e8",
            ),
            (
                27256,
                "a5365e49cba1ca36f32f871fefeeea86ee366480b71c34485f306b744cfcf938",
            ),
        ];
        let (metadata, bytes) = full_inputs();
        let source = spellwake_content::snapshot::decode(&metadata, bytes.clone()).unwrap();
        let vocabulary = vocabulary();
        let registry = Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
        let coverage = registry.coverage();
        assert_eq!(coverage.registry_version, 9);
        assert_eq!(coverage.cards.len(), 9);
        assert_eq!(coverage.required_records, 9);
        assert_eq!(coverage.required_partial_bindings, 9);
        assert_eq!(coverage.required_without_binding, 0);
        assert_eq!(coverage.fully_qualified_cards, 0);
        let mut raw = Vec::new();
        MultiGzDecoder::new(bytes.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let lines: Vec<_> = raw.split_inclusive(|byte| *byte == b'\n').collect();
        assert_eq!(lines.len(), 39_196);
        for (card, (line, hash)) in CARDS.iter().zip(LINES) {
            let raw_line = lines[line - 1];
            assert_eq!(digest(raw_line), hash);
            let raw: Value = serde_json::from_slice(raw_line).unwrap();
            assert_eq!(raw["oracle_id"], card.oracle_id);
            assert_eq!(raw["id"], card.printing_id);
            assert_eq!(raw["legalities"]["legacy"], "legal");
            assert_eq!(raw["keywords"], serde_json::json!([]));
            let identity = id(card.oracle_id);
            let materialized = &source.cards[index(&source, card.oracle_id)];
            assert_eq!(
                crate::signature(materialized, &identity).unwrap(),
                card.source_signature_v1_sha256
            );
            let coverage = coverage
                .cards
                .iter()
                .find(|row| row.oracle_id.as_deref() == Some(card.oracle_id))
                .unwrap();
            assert!(
                matches!(&coverage.binding, BindingStatus::Partial { scope: Scope::ResolutionSelectionOrdinaryCardV1, semantic_sha256, .. } if semantic_sha256 == card.source_signature_v1_sha256)
            );
            let mut expected = definition(&identity, &vocabulary).unwrap().unwrap().program;
            expected.names =
                ObjectNames::single(EnglishCardName::new(materialized.name.clone()).unwrap());
            let actual = registry
                .seed(&identity, CardId(17), SeatId(0), ObjectDestination::Hand)
                .unwrap()
                .program;
            assert_eq!(actual, expected);
            actual.validate(&vocabulary).unwrap();
        }
    }

    #[test]
    #[ignore = "requires actual complete dated catalogue/metadata and source-qualified vocabulary; no subset fallback"]
    fn actual_selection_source_and_identity_guards_reject_materialized_corruption() {
        let mut source = source();
        let vocabulary = vocabulary();
        Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
        let old = std::mem::replace(&mut source.compressed_sha256, "0".repeat(64));
        assert!(matches!(
            Registry::from_resolution_selection_inventory(&source, &vocabulary),
            Err(RegistryError::ResolutionSelectionSourceMismatch)
        ));
        source.compressed_sha256 = old;
        let old = std::mem::replace(&mut source.source.updated_at, "foreign".into());
        assert!(matches!(
            Registry::from_resolution_selection_inventory(&source, &vocabulary),
            Err(RegistryError::ResolutionSelectionSourceMismatch)
        ));
        source.source.updated_at = old;
        let old = source.source_records;
        source.source_records -= 1;
        assert!(matches!(
            Registry::from_resolution_selection_inventory(&source, &vocabulary),
            Err(RegistryError::ResolutionSelectionSourceMismatch)
        ));
        source.source_records = old;
        let foreign =
            CreatureTypeVocabulary::new("foreign".into(), [0; 32], vec!["Zombie".into()]).unwrap();
        assert!(matches!(
            Registry::from_resolution_selection_inventory(&source, &foreign),
            Err(RegistryError::ResolutionSelectionVocabularyMismatch)
        ));
        let foreign_index = source
            .cards
            .iter()
            .position(|row| !row.oracle_id.as_deref().is_some_and(selected))
            .unwrap();
        for expected in &CARDS {
            let i = index(&source, expected.oracle_id);
            let old = source.cards[i].oracle_id.take();
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionMissingIdentity { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[i].oracle_id = old;
            let old = source.cards[foreign_index]
                .oracle_id
                .replace(expected.oracle_id.into());
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionDuplicateIdentity { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[foreign_index].oracle_id = old;
            let old = std::mem::replace(
                &mut source.cards[i].scryfall_id,
                "00000000-0000-0000-0000-000000000000".into(),
            );
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionPrintingMismatch { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[i].scryfall_id = old;
            let old = std::mem::replace(
                &mut source.cards[foreign_index].scryfall_id,
                expected.printing_id.into(),
            );
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionPrintingMismatch { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[foreign_index].scryfall_id = old;
            let old = source.cards[i]
                .legalities
                .insert("legacy".into(), Legality::Banned);
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionLegalityMismatch { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[i]
                .legalities
                .insert("legacy".into(), old.unwrap());
            source.cards[i].required_by_base_formats = false;
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionLegalityMismatch { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[i].required_by_base_formats = true;
            let old = source.cards[i].faces[0]
                .oracle_text
                .replace("changed complete source clause".into());
            assert!(
                matches!(Registry::from_resolution_selection_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionSemanticMismatch { oracle_id }) if oracle_id == expected.oracle_id)
            );
            source.cards[i].faces[0].oracle_text = old;
        }
        Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
    }

    #[test]
    #[ignore = "requires actual complete dated catalogue/metadata and source-qualified vocabulary; no subset fallback"]
    fn actual_selection_source_digests_remain_fixed_when_native_operands_change() {
        let source = source();
        let vocabulary = vocabulary();
        let registry = Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
        for oracle in [
            "058917c1-21ab-488a-9f9c-591c55f3c596",
            "ad44cf74-b717-48fb-9fa2-77512024d76a",
        ] {
            let identity = id(oracle);
            let row = &source.cards[index(&source, oracle)];
            let before = crate::signature(row, &identity).unwrap();
            let original = registry
                .seed(
                    &identity,
                    CardId(18),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )
                .unwrap()
                .program;
            for variant in 0..3 {
                let mut changed = original.clone();
                let ProgramKind::Spell(spell) = &mut changed.kind else {
                    panic!("spell expected")
                };
                match (&mut spell.body.instructions[0], variant) {
                    (
                        Instruction::Sacrifice { amount, .. } | Instruction::Discard { amount, .. },
                        0,
                    ) => *amount = spellwake_engine::program::ValueExpr::Literal(7.into()),
                    (
                        Instruction::Sacrifice { players, .. }
                        | Instruction::Discard { players, .. },
                        1,
                    ) => *players = AffectedPlayers::EachPlayer,
                    (Instruction::Sacrifice { filter, .. }, 2) => {
                        filter.forbidden_types.insert(CardType::Land);
                    }
                    (Instruction::Discard { players, .. }, 2) => {
                        *players = AffectedPlayers::One(ResolvingPlayer::Controller)
                    }
                    _ => panic!("distinct represented operand expected"),
                }
                assert_ne!(changed, original);
                assert_eq!(crate::signature(row, &identity).unwrap(), before);
            }
        }
    }
}
