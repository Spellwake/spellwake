//! Public-API parser regression suite (Waves E6 through E12, layouts, and corpus audit).

use spellwake_oracle_compiler::ast::*;
use spellwake_oracle_compiler::*;

fn test_vocab() -> Vocabulary {
    Vocabulary::from_creature_members(&[
        "Human".to_string(),
        "Soldier".to_string(),
        "Wizard".to_string(),
        "Elemental".to_string(),
        "Demon".to_string(),
        "Beast".to_string(),
        "Goblin".to_string(),
        "Elf".to_string(),
        "Ally".to_string(),
        "Spirit".to_string(),
    ])
}

#[test]
fn compiles_gatecrash_bloodrush_and_shockland_and_modal() {
    let vocab = test_vocab();
    let rubblehulk = CardInput {
        oracle_id: "rubblehulk",
        name: "Rubblehulk",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Rubblehulk",
            type_line: "Creature \u{2014} Elemental",
            oracle_text: "Rubblehulk's power and toughness are each equal to the number of lands you control.\nBloodrush \u{2014} {1}{R}{G}, Discard Rubblehulk: Target attacking creature gets +X/+X until end of turn, where X is the number of lands you control.",
        }],
    };
    let r_ast = compile_card(&rubblehulk, &vocab);
    assert!(
        r_ast.strictly_compiled,
        "failures: {:?}",
        r_ast.faces[0].failures
    );
    assert_eq!(r_ast.faces[0].statics.len(), 1);
    assert_eq!(r_ast.faces[0].abilities.len(), 1);

    let breeding_pool = CardInput {
        oracle_id: "breeding-pool",
        name: "Breeding Pool",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Breeding Pool",
            type_line: "Land \u{2014} Forest Island",
            oracle_text: "({T}: Add {G} or {U}.)\nAs Breeding Pool enters, you may pay 2 life. If you don't, it enters tapped.",
        }],
    };
    let bp_ast = compile_card(&breeding_pool, &vocab);
    assert!(
        bp_ast.strictly_compiled,
        "failures: {:?}",
        bp_ast.faces[0].failures
    );
    assert_eq!(bp_ast.faces[0].replacements.len(), 1);

    let boros_charm = CardInput {
        oracle_id: "boros-charm",
        name: "Boros Charm",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Boros Charm",
            type_line: "Instant",
            oracle_text: "Choose one \u{2014}\n\u{2022} Boros Charm deals 4 damage to target player or planeswalker.\n\u{2022} Permanents you control gain indestructible until end of turn.\n\u{2022} Target creature gains double strike until end of turn.",
        }],
    };
    let bc_ast = compile_card(&boros_charm, &vocab);
    assert!(
        bc_ast.strictly_compiled,
        "failures: {:?}",
        bc_ast.faces[0].failures
    );
    assert!(matches!(
        bc_ast.faces[0].spell_effects.first(),
        Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
    ));
}

#[test]
fn stage_a_and_b_lossless_parser_regressions() {
    use spellwake_oracle_compiler::ast::{
        DelayedStepScope, ManaSpecAst, StaticAbilityAst, SubjectRefAst, TriggerEventAst,
        TurnStepAst, ValueExprAst, ZoneFilter,
    };
    let vocab = test_vocab();

    // 1. Lossless ZoneFilter variants (never collapse multi-zone/command/outside/their-hand)
    let f_gy_ex = spellwake_oracle_compiler::filter::parse_object_filter(
        "a creature card from your graveyard or exile",
        &vocab,
    )
    .expect("gy or exile");
    assert_eq!(f_gy_ex.zone, Some(ZoneFilter::YourGraveyardOrExile));

    let f_hand_gy = spellwake_oracle_compiler::filter::parse_object_filter(
        "a creature card from your hand or graveyard",
        &vocab,
    )
    .expect("hand or gy");
    assert_eq!(f_hand_gy.zone, Some(ZoneFilter::YourHandOrGraveyard));

    let f_cmd = spellwake_oracle_compiler::filter::parse_object_filter(
        "a card from the command zone",
        &vocab,
    )
    .expect("cmd");
    assert_eq!(f_cmd.zone, Some(ZoneFilter::CommandZone));

    let f_out = spellwake_oracle_compiler::filter::parse_object_filter(
        "a card you own from outside the game",
        &vocab,
    )
    .expect("outside");
    assert_eq!(f_out.zone, Some(ZoneFilter::OutsideTheGame));

    let f_their =
        spellwake_oracle_compiler::filter::parse_object_filter("a card from their hand", &vocab)
            .expect("their");
    assert_eq!(f_their.zone, Some(ZoneFilter::ThatPlayerHand));

    // 2. Exotic Orchard & Gond Gate AnyLandCouldProduce ordering
    let exotic = compile_card(
        &CardInput {
            oracle_id: "exotic-orchard",
            name: "Exotic Orchard",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Exotic Orchard",
                type_line: "Land",
                oracle_text: "{T}: Add one mana of any color that a land an opponent controls could produce.",
            }],
        },
        &vocab,
    );
    assert!(exotic.strictly_compiled, "{:?}", exotic.faces[0].failures);
    let AbilityAst::Activated { body, .. } = &exotic.faces[0].abilities[0] else {
        panic!("expected activated ability");
    };
    assert!(matches!(
        &body[0],
        InstructionAst::AddMana {
            mana: ManaSpecAst::AnyLandCouldProduce {
                controller: spellwake_oracle_compiler::ast::ControllerRelation::Opponent,
            },
            ..
        }
    ));

    // Gond Gate and Mox Amber compile into exact lossless ManaSpecAst variants
    let gond = compile_card(
        &CardInput {
            oracle_id: "gond-gate",
            name: "Gond Gate",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Gond Gate",
                type_line: "Land \u{2014} Gate",
                oracle_text: "{T}: Add one mana of any color that a Gate you control could produce.",
            }],
        },
        &vocab,
    );
    assert!(gond.strictly_compiled, "{:?}", gond.faces[0].failures);
    let AbilityAst::Activated {
        body: gond_body, ..
    } = &gond.faces[0].abilities[0]
    else {
        panic!("expected activated ability on Gond Gate");
    };
    assert!(matches!(
        &gond_body[0],
        InstructionAst::AddMana {
            mana: ManaSpecAst::AnyObjectCouldProduce { filter, any_type: false },
            ..
        } if filter.subtypes.iter().any(|s| s.eq_ignore_ascii_case("gate"))
    ));
    let mox_amber = compile_card(
        &CardInput {
            oracle_id: "mox-amber",
            name: "Mox Amber",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Mox Amber",
                type_line: "Legendary Artifact",
                oracle_text: "{T}: Add one mana of any color among legendary creatures and planeswalkers you control.",
            }],
        },
        &vocab,
    );
    assert!(
        mox_amber.strictly_compiled,
        "{:?}",
        mox_amber.faces[0].failures
    );
    let AbilityAst::Activated { body: mox_body, .. } = &mox_amber.faces[0].abilities[0] else {
        panic!("expected activated ability on Mox Amber");
    };
    assert!(matches!(
        &mox_body[0],
        InstructionAst::AddMana {
            mana: ManaSpecAst::AnyColorAmong { .. },
            ..
        }
    ));

    // 3. DieRollBand vs StationBand
    let portent = compile_card(
        &CardInput {
            oracle_id: "die-roll",
            name: "Die Roll Card",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Die Roll Card",
                type_line: "Instant",
                oracle_text: "10+ | Draw two cards.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        portent.faces[0].statics.first(),
        Some(StaticAbilityAst::CompiledDieRollBand { body, .. }) if matches!(body.as_slice(), [InstructionAst::Draw { count, .. }] if **count == ValueExprAst::Const(2))
    ));

    // 4. "when ~ leaves the battlefield" emits DelayedEventTrigger, NOT DelayedTrigger(EndStep)
    let ltb = compile_card(
        &CardInput {
            oracle_id: "ltb-test",
            name: "Test Relic",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Test Relic",
                type_line: "Artifact",
                oracle_text: "When Test Relic enters, sacrifice it when Test Relic leaves the battlefield.",
            }],
        },
        &vocab,
    );
    let AbilityAst::Triggered { body: ltb_body, .. } = &ltb.faces[0].abilities[0] else {
        panic!("expected trigger");
    };
    assert!(matches!(
        &ltb_body[0],
        InstructionAst::DelayedEventTrigger { trigger, .. }
            if matches!(
                trigger.events.first(),
                Some(TriggerEventAst::ZoneChange {
                    from: Some(f),
                    to: None,
                    subject,
                    ..
                }) if f == "battlefield" && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            )
    ));

    // 5. DelayedStepScope distinguishes YourNext vs Next vs ThatPlayersNext
    let your_end = compile_card(
        &CardInput {
            oracle_id: "your-end",
            name: "Your End",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Your End",
                type_line: "Instant",
                oracle_text: "At the beginning of your next end step, draw a card.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        your_end.faces[0].spell_effects.first(),
        Some(InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndStep,
            scope: DelayedStepScope::YourNext,
            ..
        })
    ));
    let next_end = compile_card(
        &CardInput {
            oracle_id: "next-end",
            name: "Next End",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Next End",
                type_line: "Instant",
                oracle_text: "At the beginning of the next end step, draw a card.",
            }],
        },
        &vocab,
    );
    assert!(matches!(
        next_end.faces[0].spell_effects.first(),
        Some(InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndStep,
            scope: DelayedStepScope::Next,
            ..
        })
    ));

    // 6. Sacrifice preserves explicit count ("Sacrifice two creatures")
    let sac_two = compile_card(
        &CardInput {
            oracle_id: "sac-two",
            name: "Sac Two",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Sac Two",
                type_line: "Sorcery",
                oracle_text: "Target player sacrifices two creatures.",
            }],
        },
        &vocab,
    );
    assert!(sac_two.strictly_compiled, "{:?}", sac_two.faces[0].failures);
    assert!(matches!(
        sac_two.faces[0].spell_effects.first(),
        Some(InstructionAst::Sacrifice {
            count: Some(c),
            ..
        }) if matches!(c.as_ref(), ValueExprAst::Const(2))
    ));

    // 7. Previously-lossy forms now compile into exact lossless typed AST nodes
    let sac_all = compile_card(
        &CardInput {
            oracle_id: "sac-all",
            name: "Lossy Card",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Lossy Card",
                type_line: "Artifact",
                oracle_text: "Sacrifice all creatures: Draw a card.",
            }],
        },
        &vocab,
    );
    assert!(sac_all.strictly_compiled, "{:?}", sac_all.faces[0].failures);
    assert!(matches!(
        sac_all.faces[0].abilities.first(),
        Some(AbilityAst::Activated { cost, .. })
            if matches!(cost.as_slice(), [ast::CostAtomAst::SacrificeAll(_)])
    ));

    let sac_compound = compile_card(
        &CardInput {
            oracle_id: "sac-compound",
            name: "Lossy Card",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Lossy Card",
                type_line: "Artifact",
                oracle_text: "Sacrifice a creature and Lossy Card: Draw a card.",
            }],
        },
        &vocab,
    );
    assert!(
        sac_compound.strictly_compiled,
        "{:?}",
        sac_compound.faces[0].failures
    );
    assert!(matches!(
        sac_compound.faces[0].abilities.first(),
        Some(AbilityAst::Activated { cost, .. })
            if matches!(
                cost.as_slice(),
                [ast::CostAtomAst::Sacrifice { .. }, ast::CostAtomAst::SacrificeSelf]
            )
    ));

    let destroy_except = compile_card(
        &CardInput {
            oracle_id: "destroy-except",
            name: "Lossy Card",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Lossy Card",
                type_line: "Sorcery",
                oracle_text: "Destroy all creatures except for Lossy Card.",
            }],
        },
        &vocab,
    );
    assert!(
        destroy_except.strictly_compiled,
        "{:?}",
        destroy_except.faces[0].failures
    );
    assert!(matches!(
        destroy_except.faces[0].spell_effects.first(),
        Some(InstructionAst::Destroy {
            except: Some(ex),
            random: false,
            ..
        }) if matches!(ex.as_ref(), SubjectRefAst::SelfObject)
    ));

    let draw_upto = compile_card(
        &CardInput {
            oracle_id: "draw-upto",
            name: "Lossy Card",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Lossy Card",
                type_line: "Sorcery",
                oracle_text: "Draw up to three cards.",
            }],
        },
        &vocab,
    );
    assert!(
        draw_upto.strictly_compiled,
        "{:?}",
        draw_upto.faces[0].failures
    );
    assert!(matches!(
        draw_upto.faces[0].spell_effects.first(),
        Some(InstructionAst::Draw {
            up_to: true,
            count,
            ..
        }) if matches!(count.as_ref(), ValueExprAst::Const(3))
    ));

    // 8. Lossless SearchLibrary with NameFilterAst preserves exact card name
    let named_search = compile_card(
        &CardInput {
            oracle_id: "named-search",
            name: "Named Tutor",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Named Tutor",
                type_line: "Sorcery",
                oracle_text: "Search your library for a card named Sol Ring, reveal it, put it into your hand, then shuffle.",
            }],
        },
        &vocab,
    );
    assert!(
        named_search.strictly_compiled,
        "{:?}",
        named_search.faces[0].failures
    );
    assert!(matches!(
        named_search.faces[0].spell_effects.first(),
        Some(InstructionAst::SearchLibrary { filter, .. })
            if filter.named == Some(spellwake_oracle_compiler::ast::NameFilterAst::Exact("sol ring".to_string()))
    ));
}

#[test]
fn compiles_planechase_archenemy_conspiracy_and_tabletop_uncard() {
    let vocab = test_vocab();

    let plane = CardInput {
        oracle_id: "plane-1",
        name: "Gavony",
        layout: "planar",
        faces: vec![FaceInput {
            name: "Gavony",
            type_line: "Plane \u{2014} Innistrad",
            oracle_text: "All creatures have vigilance.\nWhenever chaos ensues, you gain 4 life.",
        }],
    };
    let p_ast = compile_card(&plane, &vocab);
    assert!(
        p_ast.strictly_compiled,
        "failures: {:?}",
        p_ast.faces[0].failures
    );

    let scheme = CardInput {
        oracle_id: "scheme-1",
        name: "What's Yours Is Now Mine",
        layout: "scheme",
        faces: vec![FaceInput {
            name: "What's Yours Is Now Mine",
            type_line: "Scheme",
            oracle_text: "When you set this scheme in motion, gain control of target creature an opponent controls and untap it.",
        }],
    };
    let s_ast = compile_card(&scheme, &vocab);
    assert!(
        s_ast.strictly_compiled,
        "failures: {:?}",
        s_ast.faces[0].failures
    );

    let conspiracy = CardInput {
        oracle_id: "consp-1",
        name: "Advantageous Proclamation",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Advantageous Proclamation",
            type_line: "Conspiracy",
            oracle_text: "Hidden agenda (Start the game with this conspiracy face down in the command zone and secretly name a card.)",
        }],
    };
    let c_ast = compile_card(&conspiracy, &vocab);
    assert!(
        c_ast.strictly_compiled,
        "failures: {:?}",
        c_ast.faces[0].failures
    );
    assert!(matches!(
        c_ast.faces[0].statics.first(),
        Some(StaticAbilityAst::ConspiracyAgenda {
            hidden: true,
            named_slots: 1
        })
    ));

    let stop_that = CardInput {
        oracle_id: "stop-that",
        name: "Stop That",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Stop That",
            type_line: "Sorcery",
            oracle_text: "Target player discards a card.\nGotcha \u{2014} If an opponent audibly flicks the cards in their hand, you may say \"Gotcha!\" If you do, return Stop That from your graveyard to your hand.",
        }],
    };
    let st_ast = compile_card_with_options(
        &stop_that,
        &vocab,
        CompileOptions {
            allow_tabletop_honor_system: true,
        },
    );
    assert!(st_ast.uses_tabletop_honor_system);
}

#[test]
fn pass_e_api_safe_lib_and_preprocess_unlocks() {
    let vocab = test_vocab();

    // 1. Bulleted die-roll rows emit StaticAbilityAst::DieRollBand
    let die_card = compile_card(
        &CardInput {
            oracle_id: "die-bullets",
            name: "Roller Bot",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Roller Bot",
                type_line: "Artifact Creature \u{2014} Robot",
                oracle_text: "\u{2022} 2 \u{2014} menace\n\u{2022} 1-9 \u{2014} Draw a card.\n\u{2022} 10+ | Draw two cards.",
            }],
        },
        &vocab,
    );
    assert!(
        die_card.strictly_compiled,
        "{:?}",
        die_card.faces[0].failures
    );
    assert_eq!(die_card.faces[0].statics.len(), 3);
    assert!(die_card.faces[0].statics.iter().all(|s| matches!(
        s,
        StaticAbilityAst::DieRollBand { .. } | StaticAbilityAst::CompiledDieRollBand { .. }
    )));

    // 2. Attraction Visit modal bullets attach onto Visit triggered ability
    let attr = compile_card(
        &CardInput {
            oracle_id: "attr-modal",
            name: "Modal Midway",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Modal Midway",
                type_line: "Artifact \u{2014} Attraction",
                oracle_text: "Visit \u{2014} Choose one \u{2014}\n\u{2022} Draw a card.\n\u{2022} You gain 2 life.",
            }],
        },
        &vocab,
    );
    assert!(attr.strictly_compiled, "{:?}", attr.faces[0].failures);
    assert!(matches!(
        attr.faces[0].abilities.first(),
        Some(AbilityAst::Triggered { body, .. })
            if matches!(body.first(), Some(InstructionAst::Modal { modes, .. }) if modes.len() == 2)
    ));

    // 3. As ~ enters, choose one — promotes to AsEntersAction modal
    let enters_modal = compile_card(
        &CardInput {
            oracle_id: "enters-modal",
            name: "Modal Idol",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Modal Idol",
                type_line: "Artifact",
                oracle_text: "As Modal Idol enters, choose one \u{2014}\n\u{2022} Draw a card.\n\u{2022} You gain 2 life.",
            }],
        },
        &vocab,
    );
    assert!(
        enters_modal.strictly_compiled,
        "{:?}",
        enters_modal.faces[0].failures
    );
    assert!(matches!(
        enters_modal.faces[0].replacements.first(),
        Some(ast::ReplacementEffectAst::AsEntersAction(body))
            if matches!(body.first(), Some(InstructionAst::Modal { modes, .. }) if modes.len() == 2)
    ));

    // 4. Deep Oxford-comma trigger & trigger whose effect body contains ", if "
    let deep_trig = compile_card(
        &CardInput {
            oracle_id: "deep-trig",
            name: "Oxford Oracle",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Oxford Oracle",
                type_line: "Enchantment",
                oracle_text: "Whenever you cast an artifact, enchantment, instant, or sorcery spell, draw a card. Then, if you control a creature, you gain 2 life.",
            }],
        },
        &vocab,
    );
    assert!(
        deep_trig.strictly_compiled,
        "{:?}",
        deep_trig.faces[0].failures
    );

    // 5. Eternal-legal sticker cards do not trip physical tabletop guard
    assert!(!preprocess::is_physical_tabletop_clause(
        "add {r} for each vowel on that sticker."
    ));
    assert!(!preprocess::is_physical_tabletop_clause(
        "put a hat sticker on target creature."
    ));
    assert!(!preprocess::is_physical_tabletop_clause(
        "create a hatchling token."
    ));
    assert!(preprocess::is_physical_tabletop_clause(
        "creatures with a hat in their art"
    ));
    assert!(preprocess::is_physical_tabletop_clause("wearing a hat"));

    // 6. Bare "Level 1" on Class opens level 1 with empty cost; bare "Level 2" stays fail-closed
    let cls = compile_card(
        &CardInput {
            oracle_id: "class-l1",
            name: "Test Class",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Test Class",
                type_line: "Enchantment \u{2014} Class",
                oracle_text: "Level 1\nWhen Test Class enters, draw a card.",
            }],
        },
        &vocab,
    );
    assert!(cls.strictly_compiled, "{:?}", cls.faces[0].failures);
    assert_eq!(cls.faces[0].class_levels.len(), 1);
    assert_eq!(cls.faces[0].class_levels[0].level, 1);
    assert!(cls.faces[0].class_levels[0].cost.is_empty());

    let bad_cls = compile_card(
        &CardInput {
            oracle_id: "class-l2",
            name: "Bad Class",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Bad Class",
                type_line: "Enchantment \u{2014} Class",
                oracle_text: "Level 2\nWhen Bad Class enters, draw a card.",
            }],
        },
        &vocab,
    );
    assert!(!bad_cls.strictly_compiled);
}

#[test]
#[ignore]
fn stage1_corpus_audit() {
    use std::collections::{BTreeMap, HashMap};
    use std::io::{BufRead, BufReader};
    use std::path::Path;
    use std::process::{Command, Stdio};

    #[derive(serde::Deserialize)]
    struct RawFace {
        name: String,
        #[serde(default)]
        type_line: Option<String>,
        #[serde(default)]
        oracle_text: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawCard {
        #[serde(default)]
        oracle_id: Option<String>,
        name: String,
        layout: String,
        #[serde(default)]
        type_line: Option<String>,
        #[serde(default)]
        oracle_text: Option<String>,
        #[serde(default)]
        card_faces: Option<Vec<RawFace>>,
        #[serde(default)]
        legalities: BTreeMap<String, String>,
    }

    let vocab_path = std::env::var("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("SPELLWAKE_CREATURE_VOCABULARY_INPUT");
    let cat_path = std::env::var("SPELLWAKE_CATALOGUE_INPUT").expect("SPELLWAKE_CATALOGUE_INPUT");
    let vocab = Vocabulary::load_from_file(Path::new(&vocab_path)).expect("load vocab");

    let mut child = Command::new("gzip")
        .args(["-dc", &cat_path])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn gzip");
    let stdout = child.stdout.take().expect("gzip stdout");
    let reader = BufReader::new(stdout);

    const BASE_FORMATS: [&str; 6] = [
        "commander",
        "standard",
        "pioneer",
        "modern",
        "legacy",
        "vintage",
    ];

    let mut total = 0usize;
    let mut strict = 0usize;
    let mut cards_with_structured_rule = 0usize;
    let mut structured_rule_nodes = 0usize;
    let mut cards_with_game_state_flag = 0usize;
    let mut structured_by_kind: HashMap<String, Vec<String>> = HashMap::new();
    let mut purity_dump: Vec<String> = Vec::new();
    let mut by_bucket: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_stage: HashMap<String, usize> = HashMap::new();
    let mut dump_lines: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let raw: RawCard = serde_json::from_str(&line).expect("parse card json");
        let eligible = BASE_FORMATS.iter().any(|fmt| {
            matches!(
                raw.legalities.get(*fmt).map(String::as_str),
                Some("legal" | "restricted")
            )
        });
        if !eligible {
            continue;
        }
        total += 1;
        let owned_faces: Vec<(String, String, String)> = match raw.card_faces {
            Some(fs) if !fs.is_empty() => fs
                .into_iter()
                .map(|f| {
                    (
                        f.name,
                        f.type_line.unwrap_or_default(),
                        f.oracle_text.unwrap_or_default(),
                    )
                })
                .collect(),
            _ => vec![(
                raw.name.clone(),
                raw.type_line.unwrap_or_default(),
                raw.oracle_text.unwrap_or_default(),
            )],
        };
        let face_inputs: Vec<FaceInput<'_>> = owned_faces
            .iter()
            .map(|(n, tl, ot)| FaceInput {
                name: n.as_str(),
                type_line: tl.as_str(),
                oracle_text: ot.as_str(),
            })
            .collect();
        let ast = compile_card(
            &CardInput {
                oracle_id: raw.oracle_id.as_deref().unwrap_or(""),
                name: &raw.name,
                layout: &raw.layout,
                faces: face_inputs,
            },
            &vocab,
        );
        if ast.strictly_compiled {
            strict += 1;
        }
        if let Ok(json) = serde_json::to_string(&ast) {
            if json.contains("\"GameStateFlag\"") {
                cards_with_game_state_flag += 1;
            }
            if json.contains("\"StructuredRule\"") {
                cards_with_structured_rule += 1;
                let mut search_from = 0usize;
                while let Some(pos) = json[search_from..].find("\"StructuredRule\":{\"kind\":\"") {
                    structured_rule_nodes += 1;
                    let start = search_from + pos + "\"StructuredRule\":{\"kind\":\"".len();
                    let kind = json[start..]
                        .split_once('"')
                        .map_or("unknown", |(k, _)| k)
                        .to_string();
                    let text_marker = ",\"text\":\"";
                    let text = json[start..]
                        .find(text_marker)
                        .and_then(|tp| {
                            let ts = start + tp + text_marker.len();
                            json[ts..].split_once("\"}").map(|(t, _)| t)
                        })
                        .unwrap_or("")
                        .to_string();
                    structured_by_kind
                        .entry(kind.clone())
                        .or_default()
                        .push(raw.name.clone());
                    purity_dump.push(format!("StructuredRule\t{kind}\t{}\t{text}", raw.name));
                    search_from = start;
                }
            }
        }
        if !ast.strictly_compiled {
            for face in &ast.faces {
                for fail in &face.failures {
                    *by_stage.entry(fail.stage.clone()).or_insert(0) += 1;
                    let norm_reason = fail
                        .reason
                        .split_once(':')
                        .map_or(fail.reason.as_str(), |(h, _)| h);
                    let words: Vec<&str> = fail.text.split_whitespace().take(3).collect();
                    let prefix = words.join(" ").to_ascii_lowercase();
                    let key = format!("{}:{norm_reason}:[{prefix}]", fail.stage);
                    let entry = by_bucket.entry(key).or_default();
                    if entry.len() < 4 {
                        entry.push(format!("[{}] {}", raw.name, fail.text));
                    } else {
                        entry.push(String::new());
                    }
                    dump_lines.push(format!(
                        "{}\t{}\t{}\t{}",
                        fail.stage, norm_reason, raw.name, fail.text
                    ));
                }
            }
        }
    }
    let _ = child.wait();

    if let Ok(dump_path) = std::env::var("SPELLWAKE_AST_FAIL_DUMP") {
        let _ = std::fs::write(&dump_path, dump_lines.join("\n"));
    }
    if let Ok(purity_path) = std::env::var("SPELLWAKE_AST_PURITY_DUMP") {
        let _ = std::fs::write(&purity_path, purity_dump.join("\n"));
    }

    let failed = total.saturating_sub(strict);
    eprintln!(
        "=== FAST STAGE 1 AST AUDIT: {strict} / {total} ({:.2}%), {failed} incomplete ===",
        (strict as f64 / total.max(1) as f64) * 100.0
    );
    eprintln!(
        "AST purity: StructuredRule={structured_rule_nodes} nodes across {cards_with_structured_rule} cards ({} distinct kinds), GameStateFlag={cards_with_game_state_flag} cards",
        structured_by_kind.len()
    );
    let mut stage_sorted: Vec<(String, usize)> = by_stage.into_iter().collect();
    stage_sorted.sort_by_key(|e| std::cmp::Reverse(e.1));
    eprintln!("By stage: {:?}", stage_sorted);

    let mut sorted: Vec<(String, Vec<String>)> = by_bucket.into_iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
    for (key, samples) in sorted.iter().take(70) {
        let ex: Vec<&str> = samples
            .iter()
            .filter(|s| !s.is_empty())
            .take(3)
            .map(String::as_str)
            .collect();
        eprintln!("  {:5}  {key}  e.g. {:?}", samples.len(), ex);
    }
}

#[test]
fn pass_e6_bug_fixes_and_lossless_fidelity_traps() {
    let members: Vec<String> = [
        "Dragon", "Troll", "Warrior", "Human", "Hero", "Avatar", "Ally", "Knight", "Rogue",
        "Advisor", "Villain", "Otter", "Wizard", "Frog", "Octopus",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // Bug 1: normalize_self earliest delimiter ("Shang-Chi and the Ten Rings" -> "~")
    let shang = compile(
        "Shang-Chi and the Ten Rings",
        "Legendary Creature — Human Warrior Hero",
        "First strike\nWhenever you draw a card, put a +1/+1 counter on Shang-Chi.",
    );
    assert!(
        shang.strictly_compiled,
        "Shang-Chi: {:?}",
        shang.faces[0].failures
    );

    // Bug 2: Oxford-comma trigger headers (Sarkhan's Unsealing, Avatar Aang, River Song)
    let sarkhan = compile(
        "Sarkhan's Unsealing",
        "Enchantment",
        "Whenever you cast a creature spell with power 4, 5, or 6, Sarkhan's Unsealing deals 4 damage to any target.",
    );
    assert!(
        sarkhan.strictly_compiled,
        "Sarkhan's Unsealing: {:?}",
        sarkhan.faces[0].failures
    );

    let aang = compile(
        "Avatar Aang",
        "Legendary Creature — Human Avatar Ally",
        "Flying\nWhenever you waterbend, earthbend, firebend, or airbend, draw a card.",
    );
    assert!(
        aang.strictly_compiled,
        "Avatar Aang: {:?}",
        aang.faces[0].failures
    );

    // Bug 3: Multi-quote + trailing comma + nested single quotes (Terminal Velocity, Old-Growth Troll, Bronzehide Lion)
    let term_vel = compile(
        "Terminal Velocity",
        "Sorcery",
        "You may put an artifact or creature card from your hand onto the battlefield. That permanent gains haste, \"When this permanent leaves the battlefield, it deals damage equal to its mana value to each creature,\" and \"At the beginning of your end step, sacrifice this permanent.\"",
    );
    assert!(
        term_vel.strictly_compiled,
        "Terminal Velocity: {:?}",
        term_vel.faces[0].failures
    );

    let og_troll = compile(
        "Old-Growth Troll",
        "Creature — Troll Warrior",
        "Trample\nWhen this creature dies, if it was a creature, return it to the battlefield. It's an Aura enchantment with enchant Forest you control and \"Enchanted Forest has '{T}: Add {G}{G}' and '{1}, {T}, Sacrifice this land: Create a tapped 4/4 green Troll Warrior creature token with trample.'\"",
    );
    assert!(
        og_troll.strictly_compiled,
        "Old-Growth Troll: {:?}",
        og_troll.faces[0].failures
    );

    // Bug 4: "not named ~" in SearchLibrary (Tiamat) & bare protection/ward (Shay Cormac)
    let tiamat = compile(
        "Tiamat",
        "Legendary Creature — Dragon God",
        "Flying\nWhen Tiamat enters, if you cast it, search your library for up to five Dragon cards not named Tiamat that each have different names, reveal them, put them into your hand, then shuffle.",
    );
    assert!(
        tiamat.strictly_compiled,
        "Tiamat: {:?}",
        tiamat.faces[0].failures
    );

    let shay = compile(
        "Shay Cormac",
        "Legendary Creature — Human Knight Rogue",
        "{1}: Permanents your opponents control lose hexproof, indestructible, protection, shroud, and ward until end of turn.",
    );
    assert!(
        shay.strictly_compiled,
        "Shay Cormac: {:?}",
        shay.faces[0].failures
    );

    // Trap #2: Exclude(referenced), NOT EXCLUDE_SELF (Knowledge Pool)
    let kp = compile(
        "Knowledge Pool",
        "Artifact",
        "Whenever a player casts a spell from their hand, that player exiles it. If the player does, they may cast a spell from among other cards exiled with Knowledge Pool without paying its mana cost.",
    );
    assert!(
        kp.strictly_compiled,
        "Knowledge Pool: {:?}",
        kp.faces[0].failures
    );
    let kp_json = serde_json::to_string(&kp).unwrap_or_default();
    assert!(
        kp_json.contains("\"Exclude\""),
        "Knowledge Pool must use FilterPredicateAst::Exclude, got: {kp_json}"
    );

    // Trap #3: Evil's Thrall (UntilEndOfNextTurn = controller's next turn) vs Nuka-Nuke Launcher (UntilEndOfPlayersNextTurn(DefendingPlayer))
    let thrall = compile(
        "Evil's Thrall",
        "Sorcery",
        "Gain control of target creature until end of turn. If you control a Villain with greater mana value than that creature, gain control of that creature until the end of your next turn instead.",
    );
    assert!(
        thrall.strictly_compiled,
        "Evil's Thrall: {:?}",
        thrall.faces[0].failures
    );
    let thrall_json = serde_json::to_string(&thrall).unwrap_or_default();
    assert!(
        thrall_json.contains("\"UntilEndOfNextTurn\"")
            || thrall_json.contains("\"UntilEndOfPlayersNextTurn\""),
        "Evil's Thrall must preserve end-of-next-turn duration: {thrall_json}"
    );

    let nuka = compile(
        "Nuka-Nuke Launcher",
        "Artifact — Equipment",
        "Whenever equipped creature attacks, until the end of defending player's next turn, that player gets two rad counters whenever they cast a spell.",
    );
    assert!(
        nuka.strictly_compiled,
        "Nuka-Nuke Launcher: {:?}",
        nuka.faces[0].failures
    );
    let nuka_json = serde_json::to_string(&nuka).unwrap_or_default();
    assert!(
        nuka_json.contains("\"UntilEndOfPlayersNextTurn\":\"DefendingPlayer\""),
        "Nuka-Nuke Launcher must preserve defending-player scoped duration: {nuka_json}"
    );

    // Trap #5: Stinging Study (greatest: false) vs Cloudkill (greatest: true)
    let sting = compile(
        "Stinging Study",
        "Instant",
        "You draw X cards and you lose X life, where X is the mana value of a commander you own on the battlefield or in the command zone.",
    );
    assert!(
        sting.strictly_compiled,
        "Stinging Study: {:?}",
        sting.faces[0].failures
    );
    let sting_json = serde_json::to_string(&sting).unwrap_or_default();
    assert!(
        sting_json.contains("\"greatest\":false"),
        "Stinging Study must allow choosing partner commander (greatest: false)"
    );

    let cloudkill = compile(
        "Cloudkill",
        "Sorcery",
        "All creatures get -X/-X until end of turn, where X is the greatest mana value of a commander you own on the battlefield or in the command zone.",
    );
    assert!(
        cloudkill.strictly_compiled,
        "Cloudkill: {:?}",
        cloudkill.faces[0].failures
    );
    let ck_json = serde_json::to_string(&cloudkill).unwrap_or_default();
    assert!(
        ck_json.contains("\"greatest\":true"),
        "Cloudkill must use greatest: true"
    );

    // Trap #6: SeparateIntoPiles face_down_piles (Atris)
    let atris = compile(
        "Atris, Oracle of Half-Truths",
        "Legendary Creature — Human Advisor",
        "Menace\nWhen Atris enters, target opponent looks at the top three cards of your library and separates them into a face-down pile and a face-up pile.",
    );
    assert!(
        atris.strictly_compiled,
        "Atris: {:?}",
        atris.faces[0].failures
    );
    let atris_json = serde_json::to_string(&atris).unwrap_or_default();
    assert!(
        atris_json.contains("\"face_down_piles\":1"),
        "Atris must preserve face_down_piles: 1"
    );

    // Trap #8: The First Tyrannic War must use OthersEnterWithCounters (replacement), never PutCounters
    let tyrannic = compile(
        "The First Tyrannic War",
        "Enchantment — Saga",
        "I — You may put a creature card from your hand onto the battlefield. If its mana cost contains {X}, it enters with a number of +1/+1 counters on it equal to the number of lands you control.",
    );
    assert!(
        tyrannic.strictly_compiled,
        "The First Tyrannic War: {:?}",
        tyrannic.faces[0].failures
    );
    let tyr_json = serde_json::to_string(&tyrannic).unwrap_or_default();
    assert!(
        tyr_json.contains("\"OthersEnterWithCounters\""),
        "0/0 Hydra trap: must emit OthersEnterWithCounters"
    );
    assert!(
        !tyr_json.contains("\"PutCounters\""),
        "0/0 Hydra trap: must not emit PutCounters"
    );

    // Trap #9: Serpentine Spike DistinctTarget + Ballroom Brawlers GrantKeywordChoice
    let spike = compile(
        "Serpentine Spike",
        "Sorcery",
        "Serpentine Spike deals 2 damage to target creature, 3 damage to another target creature, and 4 damage to a third target creature.",
    );
    assert!(
        spike.strictly_compiled,
        "Serpentine Spike: {:?}",
        spike.faces[0].failures
    );
    let spike_json = serde_json::to_string(&spike).unwrap_or_default();
    assert!(
        spike_json.contains("\"DistinctTarget\""),
        "Serpentine Spike must preserve DistinctTarget slots"
    );

    // Trap #10: Turtles Forever exact: true, Master of the Wild Hunt divider: ItsController, Gatecrash Signal the Clans
    let turtles = compile(
        "Turtles Forever",
        "Sorcery",
        "Search your library and/or outside the game for exactly four legendary creature cards you own with different names, then reveal those cards.",
    );
    assert!(
        turtles.strictly_compiled,
        "Turtles Forever: {:?}",
        turtles.faces[0].failures
    );
    let turtles_json = serde_json::to_string(&turtles).unwrap_or_default();
    assert!(
        turtles_json.contains("\"exact\":true"),
        "Turtles Forever must preserve exact: true"
    );

    let signal = compile(
        "Signal the Clans",
        "Instant",
        "Search your library for three creature cards and reveal them. If you reveal three cards with different names, choose one of them at random and put that card into your hand. Shuffle the rest into your library.",
    );
    assert!(
        signal.strictly_compiled,
        "Signal the Clans (gtc): {:?}",
        signal.faces[0].failures
    );
}

#[test]
fn pass_e7_untap_and_lossless_upgrades() {
    let members: Vec<String> = [
        "Shark",
        "Illusion",
        "Drake",
        "Human",
        "Wizard",
        "Goblin",
        "Artificer",
        "Dragon",
        "Avatar",
        "Zombie",
        "God",
        "Ooze",
        "Tiefling",
        "Warrior",
        "Phyrexian",
        "Angel",
        "Wall",
        "Fish",
        "Beast",
        "Bird",
        "Hippo",
        "Construct",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // 1. Giant Shark trigger routing: uses typed DEALT_DAMAGE_THIS_TURN instead of dead branch
    let shark = compile(
        "Giant Shark",
        "Creature — Shark",
        "This creature can't attack unless defending player controls an Island.\nWhenever this creature blocks or becomes blocked by a creature that has been dealt damage this turn, this creature gets +2/+0 and gains trample until end of turn.\nWhen you control no Islands, sacrifice this creature.",
    );
    assert!(
        shark.strictly_compiled,
        "Giant Shark: {:?}",
        shark.faces[0].failures
    );
    let has_typed_block_trig = shark.faces[0].abilities.iter().any(|ab| {
        matches!(
            ab,
            AbilityAst::Triggered { trigger, .. }
                if trigger.events.iter().any(|ev| matches!(
                    ev,
                    TriggerEventAst::BlocksOrBecomesBlockedBy {
                        other_filter,
                        blocks: true,
                        becomes_blocked: true,
                        min_count: 1,
                        ..
                    } if other_filter.flags.0 & ast::ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN.0 != 0
                ))
        )
    });
    assert!(
        has_typed_block_trig,
        "Giant Shark must emit typed BlocksOrBecomesBlockedBy with DEALT_DAMAGE_THIS_TURN: {:?}",
        shark.faces[0].abilities
    );

    // 2. InstructionAst::UntapPermanent preserves count and up_to for non-targeted untap cards
    let palinchron = compile(
        "Palinchron",
        "Creature — Illusion",
        "Flying\nWhen Palinchron enters, untap up to seven lands.\n{2}{U}{U}: Return Palinchron to its owner's hand.",
    );
    assert!(palinchron.strictly_compiled);
    assert!(palinchron.faces[0].abilities.iter().any(|ab| matches!(
        ab,
        AbilityAst::Triggered { body, .. }
            if body.iter().any(|ins| matches!(
                ins,
                InstructionAst::UntapPermanent {
                    subject,
                    count: Some(cnt),
                    up_to: true,
                } if matches!(subject.as_ref(), ast::SubjectRefAst::Group(f) if f.any_types == ast::CardTypeMask::LAND)
                    && **cnt == ast::ValueExprAst::Const(7)
            ))
    )));

    let frantic = compile(
        "Frantic Search",
        "Instant",
        "Draw two cards, then discard two cards. Untap up to three lands.",
    );
    assert!(frantic.strictly_compiled);
    assert!(frantic.faces[0].spell_effects.iter().any(|ins| matches!(
        ins,
        InstructionAst::UntapPermanent {
            count: Some(cnt),
            up_to: true,
            ..
        } if **cnt == ast::ValueExprAst::Const(3)
    )));

    let azusa = compile(
        "Likeness of the Seeker",
        "Enchantment Creature — Human Monk",
        "When Likeness of the Seeker enters, untap up to three lands you control.",
    );
    assert!(azusa.strictly_compiled);
    assert!(azusa.faces[0].abilities.iter().any(|ab| matches!(
        ab,
        AbilityAst::Triggered { body, .. }
            if body.iter().any(|ins| matches!(
                ins,
                InstructionAst::UntapPermanent {
                    subject,
                    count: Some(cnt),
                    up_to: true,
                } if matches!(subject.as_ref(), ast::SubjectRefAst::Group(f) if f.controller == Some(ast::ControllerRelation::You))
                    && **cnt == ast::ValueExprAst::Const(3)
            ))
    )));

    let shriekwood = compile(
        "Shriekwood Devourer",
        "Creature — Treefolk",
        "Whenever Shriekwood Devourer attacks, untap up to X lands, where X is its power.",
    );
    assert!(shriekwood.strictly_compiled);
    let shriek_json = serde_json::to_string(&shriekwood).unwrap_or_default();
    assert!(
        shriek_json.contains("\"UntapPermanent\"")
            && shriek_json.contains("\"ChosenX\"")
            && shriek_json.contains("\"up_to\":true"),
        "Shriekwood Devourer must preserve ChosenX + up_to: {shriek_json}"
    );

    // Targeted untap preserves TargetObject with count: None, up_to: false
    let twiddle = compile(
        "Garruk Wildspeaker",
        "Planeswalker — Garruk",
        "+1: Untap two target lands.",
    );
    assert!(twiddle.strictly_compiled);
    assert!(twiddle.faces[0].abilities.iter().any(|ab| matches!(
        ab,
        AbilityAst::Loyalty { body, .. }
            if body.iter().any(|ins| matches!(
                ins,
                InstructionAst::UntapPermanent {
                    subject,
                    count: None,
                    up_to: false,
                } if matches!(subject.as_ref(), ast::SubjectRefAst::TargetObject { count: ast::TargetCountAst::Exact(2), .. })
            ))
    )));

    // 3. All 9 high-playability black-border staples compile strictly with zero StructuredRule / GameStateFlag
    let staples = [
        (
            "Chains of Mephistopheles",
            "Enchantment",
            "If a player would draw a card except the first one they draw in each of their draw steps, that player discards a card instead. If the player discards a card this way, they draw a card. If the player doesn't discard a card this way, they mill a card.",
        ),
        (
            "Chancellor of the Annex",
            "Creature — Phyrexian Angel",
            "You may reveal this card from your opening hand. If you do, when each opponent casts their first spell of the game, counter that spell unless that player pays {1}.\nFlying\nWhenever an opponent casts a spell, counter it unless that player pays {1}.",
        ),
        (
            "Goblin Welder",
            "Creature — Goblin Artificer",
            "{T}: Choose target artifact a player controls and target artifact card in that player's graveyard. If both targets are still legal as this ability resolves, that player simultaneously sacrifices the artifact and returns the artifact card to the battlefield.",
        ),
        (
            "Helm of Obedience",
            "Artifact",
            "{X}, {T}: Target opponent mills a card, then repeats this process until a creature card or X cards have been put into their graveyard this way, whichever comes first. If one or more creature cards were put into that graveyard this way, sacrifice this artifact and put one of them onto the battlefield under your control. X can't be 0.",
        ),
        (
            "Niv-Mizzet Reborn",
            "Legendary Creature — Dragon Avatar",
            "Flying\nWhen Niv-Mizzet enters, reveal the top ten cards of your library. For each color pair, choose a card that's exactly those colors from among them. Put the chosen cards into your hand and the rest on the bottom of your library in a random order.",
        ),
        (
            "The Book of Vile Darkness",
            "Legendary Artifact — Book",
            "At the beginning of your end step, if you lost 2 or more life this turn, create a 2/2 black Zombie creature token.\n{T}, Exile The Book of Vile Darkness and artifacts you control named Eye of Vecna and Hand of Vecna: Create Vecna, a legendary 8/8 black Zombie God creature token with indestructible and all triggered abilities of the exiled cards.",
        ),
        (
            "The Chain Veil",
            "Legendary Artifact",
            "At the beginning of your end step, if you didn't activate a loyalty ability of a planeswalker this turn, you lose 2 life.\n{4}, {T}: For each planeswalker you control, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn.",
        ),
        (
            "The Mimeoplasm",
            "Legendary Creature — Ooze",
            "As The Mimeoplasm enters, you may exile two creature cards from graveyards. If you do, it enters as a copy of one of those cards with a number of additional +1/+1 counters on it equal to the power of the other card.",
        ),
        (
            "Zevlor, Elturel Exile",
            "Legendary Creature — Tiefling Warrior",
            "Haste\n{2}, {T}: When you next cast an instant or sorcery spell that targets only a single opponent or a single permanent an opponent controls this turn, for each other opponent, choose that player or a permanent they control, copy that spell, and the copy targets the chosen player or permanent.",
        ),
    ];
    for (name, tl, ot) in staples {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "{name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(
            !json.contains("\"StructuredRule\""),
            "{name} must not emit StructuredRule: {json}"
        );
        assert!(
            !json.contains("\"GameStateFlag\""),
            "{name} must not emit GameStateFlag: {json}"
        );
    }

    // Verify typed AST structure on key Pass E7 orthogonal variants
    let welder = compile(
        "Goblin Welder",
        "Creature — Goblin Artificer",
        "{T}: Choose target artifact a player controls and target artifact card in that player's graveyard. If both targets are still legal as this ability resolves, that player simultaneously sacrifices the artifact and returns the artifact card to the battlefield.",
    );
    let welder_json = serde_json::to_string(&welder).unwrap_or_default();
    assert!(
        welder_json.contains("\"AllTargetsLegal\"") && welder_json.contains("\"Simultaneous\""),
        "Goblin Welder must emit AllTargetsLegal + Simultaneous: {welder_json}"
    );

    let snowblind = compile(
        "Snowblind",
        "Enchantment — Aura",
        "Enchant creature\nEnchanted creature gets -X/-Y. If that creature is attacking, X is the number of snow lands defending player controls. Otherwise, X is the number of snow lands its controller controls. Y is equal to X or to enchanted creature's toughness minus 1, whichever is smaller.",
    );
    assert!(
        snowblind.strictly_compiled,
        "Snowblind: {:?}",
        snowblind.faces[0].failures
    );
    let snow_json = serde_json::to_string(&snowblind).unwrap_or_default();
    assert!(
        snow_json.contains("\"DynamicModifyPT\"") && snow_json.contains("\"Conditional\""),
        "Snowblind must emit DynamicModifyPT + Conditional: {snow_json}"
    );

    let wild = compile(
        "Wild Endeavor",
        "Sorcery",
        "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature tokens equal to that result. Then search your library for a number of basic land cards equal to the other result, put them onto the battlefield tapped, then shuffle.",
    );
    assert!(
        wild.strictly_compiled,
        "Wild Endeavor: {:?}",
        wild.faces[0].failures
    );
}

#[test]
fn pass_e8_compositional_and_zero_structured_rules() {
    let members: Vec<String> = [
        "Goblin",
        "Artificer",
        "Dragon",
        "Avatar",
        "Human",
        "Wizard",
        "Beast",
        "Bird",
        "Cleric",
        "Rogue",
        "Warrior",
        "Soldier",
        "Zombie",
        "Hydra",
        "Angel",
        "Elf",
        "Dinosaur",
        "Phoenix",
        "Dog",
        "Spirit",
        "Eldrazi",
        "Shapeshifter",
        "Construct",
        "Phyrexian",
        "Horror",
        "Warlock",
        "Giant",
        "Fish",
        "Knight",
        "Vampire",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e8",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // 1. Hypothetical unseen future cards proving compositional generalization
    let unseen_cards: &[(&str, &str, &str, &[&str])] = &[
        (
            "Enchantment Welder",
            "Creature \u{2014} Human Artificer",
            "{T}: Choose target enchantment a player controls and target enchantment card in that player's graveyard. If both targets are still legal as this ability resolves, that player simultaneously sacrifices the enchantment and returns the enchantment card to the battlefield.",
            &["\"AllTargetsLegal\"", "\"Simultaneous\""],
        ),
        (
            "Blue Chain Veil",
            "Legendary Artifact",
            "{3}, {T}: For each blue planeswalker you control, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn.",
            &["\"ForEachObject\""],
        ),
        (
            "Guild Artificer",
            "Legendary Creature \u{2014} Human Artificer",
            "When Guild Artificer enters, reveal the top seven cards of your library. For each color pair, choose an artifact card that's exactly those colors from among them. Put the chosen cards into your hand and the rest on the bottom of your library in a random order.",
            &["\"ForEachCategory\"", "\"ColorPair\""],
        ),
        (
            "Sands of Enchantments",
            "Artifact",
            "At the beginning of each player's upkeep, that player simultaneously untaps each tapped enchantment and planeswalker they control and taps each untapped enchantment and planeswalker they control.",
            &["\"Simultaneous\""],
        ),
        (
            "Triple Power Instant",
            "Instant",
            "Triple target creature's power until end of turn.",
            &["\"DoubleStatOrCounters\"", "\"triple_power\""],
        ),
        (
            "Creature Firebombers",
            "Creature \u{2014} Giant Warrior",
            "When Creature Firebombers enters, each player sacrifices all creatures they control except for two.",
            &["\"SacrificeAllExcept\""],
        ),
        (
            "Red Soul Burn",
            "Sorcery",
            "Red Soul Burn deals X damage to any target. You gain life equal to the damage dealt, but not more life than the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness.",
            &["\"DealDamage\"", "\"ChangeLife\"", "\"TargetPreDamageCap\""],
        ),
        (
            "Power Timesifter",
            "Artifact",
            "At the beginning of each upkeep, each player exiles the top card of their library. The player who exiled the card with the greatest power takes an extra turn after this one. If two or more players' cards are tied for greatest, the tied players repeat this process until the tie is broken.",
            &["\"ExtraTurn\"", "\"RepeatProcessExt\""],
        ),
        (
            "Enchantment Reins",
            "Instant",
            "Target opponent and you exchange control of all enchantments you each control until end of turn.",
            &["\"ExchangeAllControlled\""],
        ),
        (
            "Draw-Step Paradox",
            "Enchantment \u{2014} Aura",
            "Enchant player\nAt the beginning of enchanted player's first draw step each turn, that player gets an additional draw step after this step.",
            &["\"AdditionalStep\"", "\"DrawStep\""],
        ),
        (
            "Main Phase Silence",
            "Sorcery",
            "Target opponent skips all main phases of their next turn.",
            &["\"SkipAllPhasesNextTurn\""],
        ),
        (
            "Oath of Artifacts",
            "Enchantment",
            "At the beginning of each player's upkeep, that player chooses target player whose graveyard has fewer artifact cards in it than their graveyard does and is their opponent.",
            &["\"ChooseFromGroup\""],
        ),
        (
            "Artifact Terror",
            "Creature \u{2014} Dragon",
            "Spells your opponents cast that target an artifact you control cost an additional 4 life to cast.",
            &["\"AdditionalActivationCost\"", "\"PayLife\""],
        ),
        (
            "Dice Hydra",
            "Creature \u{2014} Hydra",
            "As Dice Hydra enters, roll X d8. It enters with a number of +1/+1 counters on it equal to the total of those results.",
            &[
                "\"AsEntersAction\"",
                "\"sides\":8",
                "\"EntersWithCounters\"",
            ],
        ),
        (
            "Goblin Scion",
            "Creature \u{2014} Goblin",
            "Each Goblin you control has haste if it's red, deathtouch if it's black, and flying if it's blue.",
            &["\"Compound\"", "\"GrantKeywords\""],
        ),
        (
            "Boast Doubler",
            "Enchantment",
            "Each boast ability of creatures you control can be activated an additional time.",
            &["\"ActivationLimitOverride\"", "\"extra_times\":1"],
        ),
    ];

    for (name, tl, ot, expected_tokens) in unseen_cards {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Unseen card {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(
            !json.contains("\"StructuredRule\""),
            "Unseen card {name} emitted StructuredRule: {json}"
        );
        assert!(
            !json.contains("\"GameStateFlag\""),
            "Unseen card {name} emitted GameStateFlag: {json}"
        );
        for tok in *expected_tokens {
            assert!(
                json.contains(tok),
                "Unseen card {name} missing expected AST token {tok}: {json}"
            );
        }
    }

    // 2. Former E5_STRUCTURED_RULES & E3/E4_STATIC_EXACTS staples compile strictly with zero catch-alls
    let former_catchall_staples: &[(&str, &str, &str)] = &[
        (
            "Karn Liberated",
            "Legendary Planeswalker \u{2014} Karn",
            "\u{2212}14: Restart the game, leaving in exile all non-Aura permanent cards exiled with Karn Liberated. Then put those cards onto the battlefield under your control.",
        ),
        (
            "Jace, the Mind Sculptor",
            "Legendary Planeswalker \u{2014} Jace",
            "\u{2212}12: Exile all cards from target player's library, then that player shuffles their hand into their library.",
        ),
        (
            "Once Upon a Time",
            "Instant",
            "If this spell is the first spell you've cast this game, you may cast it without paying its mana cost.\nLook at the top five cards of your library. You may reveal a creature or land card from among them and put it into your hand. Put the rest on the bottom of your library in a random order.",
        ),
        (
            "Approach of the Second Sun",
            "Sorcery",
            "If this spell was cast from your hand and you've cast another spell named Approach of the Second Sun this game, you win the game. Otherwise, put Approach of the Second Sun into its owner's library seventh from the top and you gain 7 life.",
        ),
        (
            "Long-Term Plans",
            "Instant",
            "Search your library for a card, then shuffle and put that card third from the top.",
        ),
        (
            "Reins of Power",
            "Instant",
            "Untap all creatures you control and all creatures target opponent controls. You and that opponent each gain control of all creatures the other controls until end of turn. Those creatures gain haste until end of turn.",
        ),
        (
            "Sands of Time",
            "Artifact",
            "Each player skips their untap step.\nAt the beginning of each player's upkeep, that player simultaneously untaps each tapped artifact, creature, and land they control and taps each untapped artifact, creature, and land they control.",
        ),
        (
            "Tifa's Limit Break",
            "Instant",
            "Tiered (Choose one additional cost.)\n\u{2022} Somersault \u{2014} {0} \u{2014} Target creature gets +2/+2 until end of turn.\n\u{2022} Meteor Strikes \u{2014} {2} \u{2014} Double target creature's power and toughness until end of turn.\n\u{2022} Final Heaven \u{2014} {6}{G} \u{2014} Triple target creature's power and toughness until end of turn.",
        ),
        (
            "Terror of the Peaks",
            "Creature \u{2014} Dragon",
            "Flying\nSpells your opponents cast that target Terror of the Peaks cost an additional 3 life to cast.",
        ),
        (
            "Jetmir, Nexus of Revels",
            "Legendary Creature \u{2014} Cat Demon",
            "Creatures you control get +1/+0 and have vigilance as long as you control three or more creatures.\nCreatures you control also get +1/+0 and have trample as long as you control six or more creatures.\nCreatures you control also get +1/+0 and have double strike as long as you control nine or more creatures.",
        ),
        (
            "On Serra's Wings",
            "Enchantment \u{2014} Aura",
            "Enchant creature\nEnchanted creature is legendary, gets +1/+1, and has flying, vigilance, and lifelink.",
        ),
        (
            "Scion of Draco",
            "Artifact Creature \u{2014} Dragon",
            "Flying\nEach creature you control has vigilance if it's white, hexproof if it's blue, lifelink if it's black, first strike if it's red, and trample if it's green.",
        ),
        (
            "Arcades, the Strategist",
            "Legendary Creature \u{2014} Elder Dragon",
            "Flying, vigilance\nEach creature you control with defender assigns combat damage equal to its toughness rather than its power and can attack as though it didn't have defender.",
        ),
        (
            "Winter, Misanthropic Guide",
            "Legendary Creature \u{2014} Human Warlock",
            "As long as there are four or more card types among cards in your graveyard, each opponent's maximum hand size is equal to seven minus the number of those card types.",
        ),
        (
            "Trinisphere",
            "Artifact",
            "As long as Trinisphere is untapped, each spell that would cost less than three mana to cast costs three mana to cast.",
        ),
        (
            "K'rrik, Son of Yawgmoth",
            "Legendary Creature \u{2014} Phyrexian Horror Minion",
            "Lifelink\nFor each {B} in a cost, you may pay 2 life rather than pay that mana.",
        ),
        (
            "Opposition Agent",
            "Creature \u{2014} Human Rogue",
            "Flash\nYou control your opponents while they're searching their libraries.\nWhile an opponent is searching their library, they exile each card they find. You may play those cards for as long as they remain exiled, and you may spend mana as though it were mana of any color to cast them.",
        ),
        (
            "Neverwinter Hydra",
            "Creature \u{2014} Hydra",
            "Trample, ward {4}\nAs Neverwinter Hydra enters, roll X d6. It enters with a number of +1/+1 counters on it equal to the total of those results.",
        ),
        (
            "Deadpool, Trading Card",
            "Legendary Creature \u{2014} Mutant Mercenary Hero",
            "As Deadpool enters, you may exchange his text box and another creature's.",
        ),
        (
            "Firestorm Phoenix",
            "Creature \u{2014} Phoenix",
            "Flying\nIf Firestorm Phoenix would die, return it to its owner's hand instead. Until that player's next turn, that player plays with that card revealed in their hand and can't play it.",
        ),
        (
            "Cultist of the Absolute",
            "Legendary Enchantment \u{2014} Background",
            "Commander creatures you own get +3/+3 and have flying, deathtouch, \"Ward\u{2014}Pay 3 life,\" and \"At the beginning of your upkeep, sacrifice a creature.\"",
        ),
        (
            "Oath of Teferi",
            "Legendary Enchantment",
            "You may activate the loyalty abilities of planeswalkers you control twice each turn rather than only once.",
        ),
        (
            "Teferi, Master of Time",
            "Legendary Planeswalker \u{2014} Teferi",
            "You may activate loyalty abilities of Teferi, Master of Time on any player's turn any time you could cast an instant.",
        ),
        (
            "Mycosynth Lattice",
            "Artifact",
            "All permanents are artifacts in addition to their other types.\nAll cards that aren't on the battlefield, spells, and permanents are colorless.\nPlayers may spend mana as though it were mana of any color.",
        ),
        (
            "Drought",
            "Enchantment",
            "Spells cost an additional \"Sacrifice a Swamp\" to cast for each black mana symbol in their mana costs.\nActivated abilities cost an additional \"Sacrifice a Swamp\" to activate for each black mana symbol in their activation costs.",
        ),
        (
            "Shalai, Voice of Plenty",
            "Legendary Creature \u{2014} Angel",
            "Flying\nYou, planeswalkers you control, and other creatures you control have hexproof.",
        ),
    ];

    for (name, tl, ot) in former_catchall_staples {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Former catch-all staple {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(
            !json.contains("\"StructuredRule\""),
            "Staple {name} emitted StructuredRule: {json}"
        );
        assert!(
            !json.contains("\"GameStateFlag\""),
            "Staple {name} emitted GameStateFlag: {json}"
        );
    }
}

#[test]
fn pass_e8_lossless_engine_handoff_fixes() {
    let members: Vec<String> = [
        "Wall",
        "Pilot",
        "Human",
        "Wizard",
        "Knight",
        "Vampire",
        "Zombie",
        "Rebel",
        "Elf",
        "Archer",
        "Goblin",
        "Dragon",
        "Pirate",
        "Ouphe",
        "Horror",
        "Warlock",
        "Construct",
        "Merfolk",
        "Scout",
        "Mount",
        "Avatar",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e8-fixes",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // 1. AsEntersAction Siege modal bullets try compile_granted_ability first (lossless granted: Some(FaceAst))
    let monastery = compile(
        "Monastery Siege",
        "Enchantment",
        "As Monastery Siege enters, choose Khans or Dragons.\n\u{2022} Khans \u{2014} At the beginning of your draw step, draw an additional card, then discard a card.\n\u{2022} Dragons \u{2014} Spells your opponents cast that target you or a permanent you control cost {2} more to cast.",
    );
    assert!(
        monastery.strictly_compiled,
        "Monastery Siege: {:?}",
        monastery.faces[0].failures
    );
    let monastery_json = serde_json::to_string(&monastery).unwrap_or_default();
    assert!(
        monastery_json.contains("\"CostModifier\"")
            && monastery_json.contains("\"{2}\"")
            && monastery_json.matches("\"granted\":{").count() == 2,
        "Monastery Siege must emit lossless granted FaceAst for both Khans and Dragons: {monastery_json}"
    );

    let glacierwood = compile(
        "Glacierwood Siege",
        "Enchantment",
        "As Glacierwood Siege enters, choose Temur or Sultai.\n\u{2022} Temur \u{2014} Whenever you cast an instant or sorcery spell, target player mills four cards.\n\u{2022} Sultai \u{2014} You may play lands from your graveyard.",
    );
    assert!(
        glacierwood.strictly_compiled,
        "Glacierwood Siege: {:?}",
        glacierwood.faces[0].failures
    );
    let glacier_json = serde_json::to_string(&glacierwood).unwrap_or_default();
    assert!(
        glacier_json.contains("\"PlayOrCastPermission\"")
            && glacier_json.contains("\"permission\":\"from your graveyard\"")
            && glacier_json.matches("\"granted\":{").count() == 2,
        "Glacierwood Siege must preserve from your graveyard in granted FaceAst: {glacier_json}"
    );

    let outpost = compile(
        "Outpost Siege",
        "Enchantment",
        "As Outpost Siege enters, choose Khans or Dragons.\n\u{2022} Khans \u{2014} At the beginning of your upkeep, exile the top card of your library. Until end of turn, you may play that card.\n\u{2022} Dragons \u{2014} Whenever a creature you control leaves the battlefield, Outpost Siege deals 1 damage to any target.",
    );
    assert!(
        outpost.strictly_compiled,
        "Outpost Siege: {:?}",
        outpost.faces[0].failures
    );
    let outpost_json = serde_json::to_string(&outpost).unwrap_or_default();
    assert!(
        outpost_json.matches("\"granted\":{").count() == 2,
        "Outpost Siege must emit granted FaceAst for both modes: {outpost_json}"
    );

    // 2. Crew / Saddle power bonus preserves exact numeric bonus N (and generalizes to unseen cards)
    let hotshot = compile(
        "Hotshot Mechanic",
        "Creature \u{2014} Fox Pilot",
        "Hotshot Mechanic crews Vehicles as though its power were 2 greater.",
    );
    assert!(hotshot.strictly_compiled, "{:?}", hotshot.faces[0].failures);
    assert!(hotshot.faces[0].statics.iter().any(|st| matches!(
        st,
        StaticAbilityAst::CrewOrSaddlePowerBonus {
            crews_vehicles: true,
            saddles_mounts: false,
            bonus,
            ..
        } if **bonus == ast::ValueExprAst::Const(2)
    )));

    let deathless = compile(
        "Deathless Pilot",
        "Creature \u{2014} Zombie Pilot",
        "Deathless Pilot saddles Mounts and crews Vehicles as though its power were 2 greater.",
    );
    assert!(
        deathless.strictly_compiled,
        "{:?}",
        deathless.faces[0].failures
    );
    assert!(deathless.faces[0].statics.iter().any(|st| matches!(
        st,
        StaticAbilityAst::CrewOrSaddlePowerBonus {
            crews_vehicles: true,
            saddles_mounts: true,
            bonus,
            ..
        } if **bonus == ast::ValueExprAst::Const(2)
    )));

    let unseen_mount = compile(
        "Master Mountaineer",
        "Creature \u{2014} Human Pilot",
        "Master Mountaineer saddles Mounts as though its power were 3 greater.",
    );
    assert!(
        unseen_mount.strictly_compiled,
        "{:?}",
        unseen_mount.faces[0].failures
    );
    assert!(unseen_mount.faces[0].statics.iter().any(|st| matches!(
        st,
        StaticAbilityAst::CrewOrSaddlePowerBonus {
            crews_vehicles: false,
            saddles_mounts: true,
            bonus,
            ..
        } if **bonus == ast::ValueExprAst::Const(3)
    )));

    // 3. CantBeBlockedBy / CantBeBlockedExceptBy never silently drop unparseable filters to None
    let doorman = compile(
        "Doorman",
        "Creature \u{2014} Human",
        "{T}: Until end of turn, target creature can't be blocked by creatures with power 2 or less and/or Walls.",
    );
    assert!(doorman.strictly_compiled, "{:?}", doorman.faces[0].failures);
    let doorman_json = serde_json::to_string(&doorman).unwrap_or_default();
    assert!(
        doorman_json.contains("\"Compound\"")
            && doorman_json.contains("\"CantBeBlockedBy\"")
            && doorman_json.contains("\"Wall\"")
            && !doorman_json.contains("\"filter\":null"),
        "Doorman must split into lossless Compound CantBeBlockedBy filters without filter:null: {doorman_json}"
    );

    let elven = compile(
        "Elven Riders",
        "Creature \u{2014} Elf",
        "Elven Riders can't be blocked except by Walls and/or creatures with flying.",
    );
    assert!(elven.strictly_compiled, "{:?}", elven.faces[0].failures);
    let elven_json = serde_json::to_string(&elven).unwrap_or_default();
    assert!(
        elven_json.contains("\"CantBeBlockedExceptByAny\"")
            && elven_json.contains("\"Wall\"")
            && elven_json.contains("\"flying\""),
        "Elven Riders must emit lossless CantBeBlockedExceptByAny: {elven_json}"
    );

    let jasmine = compile(
        "Jasmine Boreal of the Seven",
        "Legendary Creature \u{2014} Human Druid",
        "Creatures you control with no abilities can't be blocked by creatures with abilities.",
    );
    assert!(jasmine.strictly_compiled, "{:?}", jasmine.faces[0].failures);
    let jasmine_json = serde_json::to_string(&jasmine).unwrap_or_default();
    assert!(
        jasmine_json.contains("\"has_abilities\":false")
            && jasmine_json.contains("\"has_abilities\":true")
            && !jasmine_json.contains("\"filter\":null"),
        "Jasmine Boreal must preserve HasAbilities predicates on both sides: {jasmine_json}"
    );

    let admiral = compile(
        "Azure Fleet Admiral",
        "Creature \u{2014} Human Pirate",
        "Azure Fleet Admiral can't be blocked by creatures the monarch controls.",
    );
    assert!(admiral.strictly_compiled, "{:?}", admiral.faces[0].failures);
    let admiral_json = serde_json::to_string(&admiral).unwrap_or_default();
    assert!(
        admiral_json.contains("\"ControlledByMonarch\"")
            && !admiral_json.contains("\"filter\":null"),
        "Azure Fleet Admiral must preserve ControlledByMonarch: {admiral_json}"
    );

    // 4. "you control <flt> named <names>" preserves exact names and decomposes multi-name lists
    let scepter = compile(
        "Scepter of Empires",
        "Artifact",
        "{T}: Scepter of Empires deals 1 damage to target player or planeswalker. It deals 3 damage instead if you control artifacts named Crown of Empires and Throne of Empires.",
    );
    assert!(scepter.strictly_compiled, "{:?}", scepter.faces[0].failures);
    let scepter_json = serde_json::to_string(&scepter).unwrap_or_default();
    assert!(
        scepter_json.contains("\"Exact\":\"Crown of Empires\"")
            && scepter_json.contains("\"Exact\":\"Throne of Empires\"")
            && scepter_json.contains("\"And\""),
        "Scepter of Empires must emit And of two named ControlsCount conditions: {scepter_json}"
    );

    let kaldra = compile(
        "Helm of Kaldra",
        "Legendary Artifact \u{2014} Equipment",
        "{1}: If you control Equipment named Helm of Kaldra, Sword of Kaldra, and Shield of Kaldra, create Kaldra, a legendary 4/4 colorless Avatar creature token.",
    );
    assert!(kaldra.strictly_compiled, "{:?}", kaldra.faces[0].failures);
    let kaldra_json = serde_json::to_string(&kaldra).unwrap_or_default();
    assert!(
        kaldra_json.contains("\"SelfName\"")
            && kaldra_json.contains("\"Exact\":\"Sword of Kaldra\"")
            && kaldra_json.contains("\"Exact\":\"Shield of Kaldra\""),
        "Helm of Kaldra must preserve all three named Equipment: {kaldra_json}"
    );

    let newt = compile(
        "Festering Newt",
        "Creature \u{2014} Salamander",
        "When Festering Newt dies, target creature an opponent controls gets -1/-1 until end of turn. That creature gets -4/-4 instead if you control a creature named Bogbrew Witch.",
    );
    assert!(newt.strictly_compiled, "{:?}", newt.faces[0].failures);
    let newt_json = serde_json::to_string(&newt).unwrap_or_default();
    assert!(
        newt_json.contains("\"Exact\":\"Bogbrew Witch\""),
        "Festering Newt must preserve Bogbrew Witch name filter: {newt_json}"
    );

    let liu_bei = compile(
        "Liu Bei, Lord of Shu",
        "Legendary Creature \u{2014} Human Soldier",
        "Horsemanship\nLiu Bei, Lord of Shu gets +2/+2 as long as you control a permanent named Guan Yu, Sainted Warrior or Zhang Fei, Fierce Warrior.",
    );
    assert!(liu_bei.strictly_compiled, "{:?}", liu_bei.faces[0].failures);
    let liu_json = serde_json::to_string(&liu_bei).unwrap_or_default();
    assert!(
        liu_json.contains("\"Or\"")
            && liu_json.contains("\"Exact\":\"Guan Yu, Sainted Warrior\"")
            && liu_json.contains("\"Exact\":\"Zhang Fei, Fierce Warrior\""),
        "Liu Bei must emit Or of two named ControlsCount conditions preserving commas: {liu_json}"
    );

    let galactus = compile(
        "Galactus, Devourer of Worlds",
        "Legendary Creature \u{2014} Alien",
        "Galactus attacks each combat if able unless you control a creature named Silver Surfer, Galactus's Herald.",
    );
    assert!(
        galactus.strictly_compiled,
        "{:?}",
        galactus.faces[0].failures
    );
    let galactus_json = serde_json::to_string(&galactus).unwrap_or_default();
    assert!(
        galactus_json.contains("\"Exact\":\"Silver Surfer, ~'s Herald\""),
        "Galactus must keep comma inside singular legendary card name intact: {galactus_json}"
    );
}

#[test]
fn pass_e9_zero_literal_effect_tables() {
    let members: Vec<String> = [
        "Elemental",
        "Samurai",
        "Wizard",
        "Human",
        "Eldrazi",
        "Spider",
        "Hero",
        "Cat",
        "Warrior",
        "Spirit",
        "Doctor",
        "Goblin",
        "Vampire",
        "Zombie",
        "Horror",
        "Dragon",
        "Angel",
        "Beast",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e9",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // 1. Unseen future cards proving compositional generalization from Wave E9 primitives
    let unseen: &[(&str, &str, &str, &[&str])] = &[
        (
            "Arcane Tripler",
            "Sorcery",
            "Target player takes three extra turns after this one.",
            &["\"ExtraTurn\""],
        ),
        (
            "Scaled Flame Volley",
            "Sorcery",
            "Scaled Flame Volley deals 2 damage to any target, 3 damage to another target, and 4 damage to a third target.",
            &[
                "\"AnyTarget\"",
                "\"DistinctTarget\"",
                "\"ordinal\":2",
                "\"ordinal\":3",
            ],
        ),
        (
            "Vault Unlocker",
            "Instant",
            "Unlock a locked door of up to one target Room you control.",
            &["\"UnlockDoor\""],
        ),
        (
            "Blocker Fortress",
            "Instant",
            "Target creature can block up to three additional creatures this turn.",
            &["\"ChangeBlockerLimit\"", "\"extra_blockers\":{\"Const\":3}"],
        ),
        (
            "Mana Doubler Ring",
            "Artifact",
            "{4}, {T}: Double the amount of each type of unspent mana you have.",
            &["\"DoubleUnspentMana\""],
        ),
        (
            "Advantage Roll",
            "Sorcery",
            "Roll three d12 and ignore the lower roll.",
            &["\"RollDiceAndKeep\"", "\"sides\":12", "\"drop_lowest\":1"],
        ),
        (
            "Counter Scrubber",
            "Sorcery",
            "You may remove up to four shield counters from among all permanents.",
            &["\"RemoveCounters\"", "\"counter_kind\":\"shield\""],
        ),
    ];

    for (name, tl, ot, expected) in unseen {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Unseen E9 card {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(!json.contains("\"StructuredRule\""), "{name}: {json}");
        assert!(!json.contains("\"GameStateFlag\""), "{name}: {json}");
        assert!(
            !json.contains("\"AbilityRestriction\""),
            "{name} should not emit opaque AbilityRestriction: {json}"
        );
        for tok in *expected {
            assert!(json.contains(tok), "{name} missing {tok}: {json}");
        }
    }

    // 2. Former E3_EFFECT_EXACTS & E4_EFFECT_EXACTS staples now emit typed AST nodes
    let staples: &[(&str, &str, &str, &str)] = &[
        (
            "Time Stretch",
            "Sorcery",
            "Target player takes two extra turns after this one.",
            "\"ExtraTurn\"",
        ),
        (
            "Cone of Flame",
            "Sorcery",
            "Cone of Flame deals 1 damage to any target, 2 damage to another target, and 3 damage to a third target.",
            "\"DistinctTarget\"",
        ),
        (
            "Spectral Searchlight",
            "Artifact",
            "{T}: Choose a player. That player adds one mana of any color they choose.",
            "\"AddMana\"",
        ),
        (
            "Stadium Vendors",
            "Creature \u{2014} Goblin",
            "At the beginning of your postcombat main phase, choose a player. That player adds two mana of any one color they choose.",
            "\"single_color\":true",
        ),
        (
            "Doubling Cube",
            "Artifact",
            "{3}, {T}: Double the amount of each type of unspent mana you have.",
            "\"DoubleUnspentMana\"",
        ),
        (
            "Reverse the Sands",
            "Sorcery",
            "Redistribute any number of players' life totals.",
            "\"RedistributeLifeTotals\"",
        ),
        (
            "Exchange of Words",
            "Enchantment",
            "When Exchange of Words enters, exile target creature and another target creature. For as long as Exchange of Words remains on the battlefield, exchange the text boxes of those creatures.",
            "\"ExchangeTextBoxes\"",
        ),
        (
            "Primal Prayers",
            "Enchantment",
            "You may cast creature spells with mana value 3 or less by paying {E} rather than paying their mana costs. If you cast a spell this way, you may cast it as though it had flash.",
            "\"as_though_flash\":true",
        ),
        (
            "Disciple of Caelus Nin",
            "Creature \u{2014} Human Wizard",
            "When Disciple of Caelus Nin enters, each player chooses a permanent they control. All permanents other than Disciple of Caelus Nin that weren't chosen this way phase out.",
            "\"PhaseOut\"",
        ),
        (
            "Sensational Spider-Man",
            "Legendary Creature \u{2014} Spider Human Hero",
            "Whenever Sensational Spider-Man attacks, tap target creature defending player controls and put a stun counter on it. Then you may remove up to three stun counters from among all permanents.",
            "\"RemoveCounters\"",
        ),
        (
            "Mirri, Weatherlight Duelist",
            "Legendary Creature \u{2014} Cat Warrior",
            "Whenever Mirri, Weatherlight Duelist attacks, choose up to one target creature defending player controls. That creature can't block this combat, and each opponent can't block with more than one creature this combat.",
            "\"ChangeBlockerLimit\"",
        ),
        (
            "Search for Survivors",
            "Sorcery",
            "Reorder your graveyard at random.",
            "\"ReorderZone\"",
        ),
    ];

    for (name, tl, ot, expected) in staples {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Former E3/E4 exact staple {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(!json.contains("\"StructuredRule\""), "{name}: {json}");
        assert!(!json.contains("\"GameStateFlag\""), "{name}: {json}");
        assert!(json.contains(expected), "{name} missing {expected}: {json}");
    }
}

#[test]
fn pass_e10_hundred_percent_atomic_coverage() {
    let members: Vec<String> = [
        "Human",
        "Soldier",
        "Centaur",
        "Performer",
        "Pirate",
        "Rogue",
        "Artificer",
        "Time",
        "Lord",
        "Doctor",
        "Goblin",
        "Wizard",
        "Dragon",
        "Elf",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e10",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    // 1. All 18 final corpus cards + fixed sibling cards (Apocalypse Chime, City in a Bottle)
    let final_cards: &[(&str, &str, &str, &[&str])] = &[
        (
            "Space Beleren",
            "Legendary Planeswalker \u{2014} Jace",
            "Space sculptor (Space Beleren divides the battlefield into alpha, beta, and gamma sectors. If a creature isn't assigned to a sector, its controller assigns it to one. Opponents assign first.)\n+1: Creatures in each sector can be blocked this turn only by creatures in the same sector.\n\u{2212}1: Put a +1/+1 counter on each creature in the sector of your choice.\n\u{2212}5: Destroy all creatures in the sector of your choice.",
            &[
                "\"BlockingPartitionConstraint\"",
                "\"InObjectPartition\"",
                "\"Sector\"",
            ],
        ),
        (
            "Raging River",
            "Enchantment",
            "Whenever one or more creatures you control attack, each defending player divides all creatures without flying they control into a \"left\" pile and a \"right\" pile. Then, for each attacking creature you control, choose \"left\" or \"right.\" That creature can't be blocked this combat except by creatures with flying and creatures in a pile with the chosen label.",
            &[
                "\"PartitionObjects\"",
                "\"LabeledPiles\"",
                "\"CantBeBlockedExceptByAny\"",
                "\"InObjectPartition\"",
            ],
        ),
        (
            "Camouflage",
            "Instant",
            "Cast this spell only during your declare attackers step.\nThis turn, instead of declaring blockers, each defending player chooses any number of creatures they control and divides them into a number of piles equal to the number of attacking creatures for whom that player is the defending player. Creatures those players control that can block additional creatures may likewise be put into additional piles. Assign each pile to a different one of those attacking creatures at random. Each creature in a pile that can block the creature that pile is assigned to does so. (Piles can be empty.)",
            &[
                "\"CombatBlockerPiles\"",
                "\"AllowExtraPartitionMembership\"",
                "\"AssignPilesToSubjects\"",
                "\"BlockAssignedPartitionTarget\"",
            ],
        ),
        (
            "Ferris Wheel",
            "Artifact \u{2014} Attraction",
            "Visit \u{2014} Choose target creature that hasn't been phased out with this Attraction. That creature phases out until you roll a 3 or less while rolling to visit your Attractions.",
            &[
                "\"PhasedOutBySource\"",
                "\"PhaseOutUntilDieRoll\"",
                "\"VisitAttractions\"",
            ],
        ),
        (
            "Paliano Vanguard",
            "Creature \u{2014} Human Soldier",
            "Draft this card face up.\nAs you draft a creature card, you may reveal it, note its creature types, then turn this card face down.\nOther creatures you control of a type you noted for cards named Paliano Vanguard get +1/+1.",
            &["\"MatchesDraftLedger\"", "\"NotedCreatureType\""],
        ),
        (
            "Regicide",
            "Instant",
            "Reveal this card as you draft it. The player to your right chooses a color, you choose another color, then the player to your left chooses a third color.\nDestroy target creature that's one or more of the colors chosen as you drafted cards named Regicide.",
            &[
                "\"DraftAction\"",
                "\"MatchesDraftLedger\"",
                "\"NotedColor\"",
            ],
        ),
        (
            "Smuggler Captain",
            "Creature \u{2014} Human Pirate",
            "Draft this card face up.\nAs you draft a card, you may reveal it, note its name, then turn this card face down.\nWhen this creature enters, you may search your library for a card with a name you noted for cards named Smuggler Captain, reveal it, put it into your hand, then shuffle.",
            &["\"MatchesDraftLedger\"", "\"NotedName\""],
        ),
        (
            "The Seventh Doctor",
            "Legendary Creature \u{2014} Time Lord Doctor",
            "Whenever The Seventh Doctor attacks, choose a card in your hand. Defending player guesses whether that card's mana value is greater than the number of artifacts you control. If they guessed wrong, you may cast it without paying its mana cost. If you don't cast a spell this way, investigate.",
            &["\"GuessCondition\"", "\"Compare\"", "\"Gt\""],
        ),
        (
            "The Celestial Toymaker",
            "Legendary Creature \u{2014} Rogue Artificer",
            "Whenever The Celestial Toymaker attacks, look at the top three cards of your library. Exile any number of them in a face-down pile and the rest in a face-up pile. Defending player chooses one of those piles. Put that pile into your hand.\nAt the beginning of each end step, each opponent loses 2 life for each spell or ability that caused one or more players to guess or to group cards or permanents into a pile this turn.",
            &[
                "\"ExileIntoVisibilityPiles\"",
                "\"FaceDown\"",
                "\"FaceUp\"",
                "\"guess_or_pile_spell_or_ability\"",
            ],
        ),
        (
            "General Jarkeld",
            "Legendary Creature \u{2014} Human Soldier",
            "{T}: Choose two target blocked attacking creatures. If each of those creatures could be blocked by all creatures that the other is blocked by, each creature that's blocking exactly one of those attacking creatures stops blocking it and is blocking the other attacking creature. Activate only during the declare blockers step.",
            &[
                "\"CanLegallySwapBlockAssignments\"",
                "\"BlockedAttackers\"",
                "\"SwapBlockerAssignments\"",
            ],
        ),
        (
            "Sorrow's Path",
            "Land",
            "{T}: Choose two target blocking creatures controlled by the same opponent. If each of those creatures could block all creatures that the other is blocking, remove both of them from combat. Each one then blocks all creatures the other was blocking.\nWhenever this land becomes tapped, it deals 2 damage to you and each creature you control.",
            &[
                "\"CanLegallySwapBlockAssignments\"",
                "\"BlockingCreatures\"",
                "\"RemoveFromCombat\"",
                "\"SwapBlockerAssignments\"",
            ],
        ),
        (
            "Goblin Game",
            "Sorcery",
            "Each player hides at least one item, then all players reveal them simultaneously. Each player loses life equal to the number of items they revealed. The player who revealed the fewest items then loses half their life, rounded up. If two or more players are tied for fewest, each loses half their life, rounded up.",
            &[
                "\"SecretNumberReveal\"",
                "\"LoseFractionLife\"",
                "\"PlayersTiedFor\"",
            ],
        ),
        (
            "Golgothian Sylex",
            "Artifact",
            "{1}, {T}: Each nontoken permanent with a name originally printed in the Antiquities expansion is sacrificed by its controller.",
            &[
                "\"FirstPrintedInExpansion\":\"Antiquities\"",
                "\"Sacrifice\"",
            ],
        ),
        (
            "Apocalypse Chime",
            "Artifact",
            "{2}, {T}, Sacrifice this artifact: Destroy all nontoken permanents with a name originally printed in the Homelands expansion. They can't be regenerated.",
            &["\"FirstPrintedInExpansion\":\"Homelands\""],
        ),
        (
            "City in a Bottle",
            "Artifact",
            "Whenever one or more other nontoken permanents with a name originally printed in the Arabian Nights expansion are on the battlefield, their controllers sacrifice them.\nPlayers can't cast spells or play lands with a name originally printed in the Arabian Nights expansion.",
            &["\"FirstPrintedInExpansion\":\"Arabian Nights\""],
        ),
        (
            "Pick-a-Beeble",
            "Artifact \u{2014} Attraction",
            "Visit \u{2014} Roll a six-sided die. Put a number of luck counters on Pick-a-Beeble equal to the result and create a Treasure token. Then if there are six or more luck counters on Pick-a-Beeble, claim the prize!\nPrize \u{2014} Create two Treasure tokens, then sacrifice Pick-a-Beeble and open an Attraction.",
            &[
                "\"ClaimPrize\"",
                "\"ability_word\":\"Prize\"",
                "\"PrizeClaimedSelf\"",
            ],
        ),
        (
            "Pin Collection",
            "Artifact \u{2014} Equipment",
            "When this Equipment enters, you may put an ability sticker with ticket cost X or less on it without paying that sticker's ticket cost.\nEquipped creature gets +1/+1 and has all abilities of ability stickers on this Equipment.\nEquip {2}",
            &[
                "\"PutStickerWithCost\"",
                "\"without_paying_ticket_cost\":true",
                "\"HasStickerKind\":\"ability\"",
            ],
        ),
        (
            "Lineprancers",
            "Creature \u{2014} Centaur Performer",
            "When Lineprancers enters, you get {TK}{TK}, then you may put a power and toughness sticker on a creature you own.\n{3}{G}: Target creature you don't control blocks target creature you control with a power and toughness sticker on it other than Lineprancers this turn if able.",
            &[
                "\"PutSticker\"",
                "\"must_block:target creature you control with a power and toughness sticker on it other than ~\"",
            ],
        ),
        (
            "Word of Command",
            "Instant",
            "Look at target opponent's hand and choose a card from it. You control that player until Word of Command finishes resolving. The player plays that card if able. While doing so, the player can activate mana abilities only if they're from lands that player controls and only if mana they produce is spent to activate other mana abilities of lands the player controls and/or to play that card. If the chosen card is cast as a spell, you control the player while that spell is resolving.",
            &[
                "\"ControlPlayerDuringResolution\"",
                "\"ForcePlayCard\"",
                "\"RestrictManaAbilitiesDuringAction\"",
                "\"ChosenCardCastAsSpell\"",
            ],
        ),
        (
            "Illusionary Mask",
            "Artifact",
            "{X}: You may choose a creature card in your hand whose mana cost could be paid by some amount of, or all of, the mana you spent on {X}. If you do, you may cast that card face down as a 2/2 creature spell without paying its mana cost. If the creature that spell becomes as it resolves has not been turned face up and would assign or deal damage, be dealt damage, or become tapped, instead it's turned face up and assigns or deals damage, is dealt damage, or becomes tapped. Activate only as a sorcery.",
            &[
                "\"ManaCostPayableBySpentMana\"",
                "\"CastFaceDown\"",
                "\"without_paying_mana_cost\":true",
            ],
        ),
    ];

    for (name, tl, ot, expected) in final_cards {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Pass E10 card {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(!json.contains("\"StructuredRule\""), "{name}: {json}");
        assert!(!json.contains("\"GameStateFlag\""), "{name}: {json}");
        for tok in *expected {
            assert!(json.contains(tok), "{name} missing {tok}: {json}");
        }
    }

    // Direct typed filter verification for power and toughness sticker predicate
    let pt_sticker_flt = spellwake_oracle_compiler::filter::parse_object_filter(
        "creature you control with a power and toughness sticker on it other than ~",
        &vocab,
    )
    .expect("power and toughness sticker filter");
    assert!(pt_sticker_flt.predicates.iter().any(|p| matches!(
        p,
        spellwake_oracle_compiler::ast::FilterPredicateAst::HasStickerKind(k) if k == "power_and_toughness"
    )));

    // 2. Unseen hypothetical future cards proving every Pass E10 primitive generalizes
    let unseen: &[(&str, &str, &str, &[&str])] = &[
        (
            "Legends Relic",
            "Artifact",
            "{2}, {T}: Each nontoken permanent with a name originally printed in the Legends expansion is sacrificed by its controller.",
            &["\"FirstPrintedInExpansion\":\"Legends\"", "\"Sacrifice\""],
        ),
        (
            "Riddle of the Library",
            "Sorcery",
            "Choose a card in your hand. Target opponent guesses whether that card's power is greater than the number of lands you control.",
            &["\"GuessCondition\"", "\"Power\""],
        ),
        (
            "Shadow Split",
            "Sorcery",
            "Look at the top four cards of your library. Exile any number of them in a face-up pile and the rest in a face-down pile.",
            &["\"ExileIntoVisibilityPiles\"", "\"FaceUp\"", "\"FaceDown\""],
        ),
        (
            "Prize Booth",
            "Artifact \u{2014} Attraction",
            "Visit \u{2014} Put a charge counter on Prize Booth. Then if there are three or more charge counters on Prize Booth, claim the prize!\nPrize \u{2014} Draw two cards, then sacrifice Prize Booth.",
            &["\"ClaimPrize\"", "\"PrizeClaimedSelf\""],
        ),
        (
            "Ticket Booth Curator",
            "Creature \u{2014} Human Performer",
            "When Ticket Booth Curator enters, you may put a name sticker with ticket cost 2 or less on it without paying that sticker's ticket cost.",
            &["\"PutStickerWithCost\"", "\"kind\":\"name\""],
        ),
        (
            "Dimensional Rollercoaster",
            "Artifact \u{2014} Attraction",
            "Visit \u{2014} Target creature phases out until you roll a 2 or less while rolling to visit your Attractions.",
            &["\"PhaseOutUntilDieRoll\"", "\"threshold\":2"],
        ),
        (
            "North-South Divide",
            "Enchantment",
            "Whenever one or more creatures you control attack, each defending player divides all creatures without flying they control into a \"north\" pile and a \"south\" pile.",
            &["\"PartitionObjects\"", "\"north\"", "\"south\""],
        ),
        (
            "Draft-Marked Verdict",
            "Instant",
            "Reveal this card as you draft it. The player to your right chooses a color, you choose another color, then the player to your left chooses a third color.\nExile target creature that's one or more of the colors chosen as you drafted cards named Draft-Marked Verdict.",
            &[
                "\"DraftAction\"",
                "\"MatchesDraftLedger\"",
                "\"NotedColor\"",
            ],
        ),
        (
            "Goblin Bidding War",
            "Sorcery",
            "Each player hides at least two items, then all players reveal them simultaneously. The player who revealed the most items then loses half their life, rounded down.",
            &[
                "\"SecretNumberReveal\"",
                "\"min_value\":2",
                "\"lowest\":false",
                "\"Down\"",
            ],
        ),
        (
            "Phantom Cast",
            "Instant",
            "You may cast a creature card from your hand face down as a 2/2 creature spell without paying its mana cost.",
            &["\"CastFaceDown\"", "\"power\":2", "\"toughness\":2"],
        ),
    ];

    for (name, tl, ot, expected) in unseen {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Unseen Pass E10 card {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(!json.contains("\"StructuredRule\""), "{name}: {json}");
        assert!(!json.contains("\"GameStateFlag\""), "{name}: {json}");
        for tok in *expected {
            assert!(json.contains(tok), "{name} missing {tok}: {json}");
        }
    }
}

#[test]
fn pass_e11_lossless_ast_hardening_regressions() {
    let members: Vec<String> = [
        "Human",
        "Soldier",
        "Djinn",
        "Ooze",
        "Demon",
        "Orc",
        "Warrior",
        "Rogue",
        "Goblin",
        "Zombie",
        "Dragon",
        "Elf",
        "Beast",
        "Dinosaur",
        "Fungus",
        "Spirit",
        "Angel",
        "Knight",
        "Wizard",
        "Wall",
        "Phyrexian",
        "Giant",
        "Shapeshifter",
        "Avatar",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e11",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };

    type E11Case<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);
    let cases: &[E11Case<'_>] = &[
        // Wave 5: Opening-hand reveal vs Exert-on-attack split
        (
            "Chancellor of the Forge",
            "Creature \u{2014} Phyrexian Giant",
            "You may reveal this card from your opening hand. If you do, at the beginning of the first upkeep, create a 1/1 red Phyrexian Goblin creature token with haste.",
            &["\"OpeningHandRevealAction\""],
            &["\"FollowUp\""],
        ),
        (
            "Nef-Crop Entangler",
            "Creature \u{2014} Zombie Warrior",
            "Trample\nYou may exert Nef-Crop Entangler as it attacks. When you do, it gets +1/+2 until end of turn.",
            &["\"ExertOnAttackAction\""],
            &["\"FollowUp\""],
        ),
        // Wave 5: Most common permanent color (Sulam Djinn)
        (
            "Sulam Djinn",
            "Creature \u{2014} Djinn",
            "Trample\nSulam Djinn gets -2/-2 as long as green is the most common color among all permanents or is tied for most common.",
            &["\"MostCommonPermanentColor\"", "\"or_tied\":true"],
            &["\"ControlsGreatestStat\""],
        ),
        // Wave 5: During your end step & Once during each of your turns
        (
            "End Step Static Test",
            "Enchantment",
            "During your end step, creatures you control get +1/+1.\nOnce during each of your turns, you may cast a creature spell from your graveyard.",
            &["\"DuringStep\"", "\"EndStep\"", "\"OnceDuringYourTurn\""],
            &["\"IsYourTurn\""],
        ),
        // Wave 6: Modal keyword choice (Butcher of the Horde)
        (
            "Butcher of the Horde",
            "Creature \u{2014} Demon",
            "Flying\nSacrifice another creature: Butcher of the Horde gains your choice of vigilance, lifelink, or haste until end of turn.",
            &["\"Choice\"", "\"vigilance\"", "\"lifelink\"", "\"haste\""],
            &[],
        ),
        // Wave 6: Bane Alley Broker face_down: true
        (
            "Bane Alley Broker",
            "Creature \u{2014} Human Rogue",
            "{T}: Draw a card, then exile a card from your hand face down.",
            &["\"Exile\"", "\"face_down\":true"],
            &["\"face_down\":false"],
        ),
        // Wave 6: Release to the Wind subject-preserving RemainsExiled
        (
            "Release to the Wind",
            "Instant",
            "Exile target nonland permanent. For as long as that card remains exiled, its owner may cast it without paying its mana cost.",
            &["\"SubjectRemainsExiled\"", "\"that card\""],
            &["\"RemainsExiled\""],
        ),
        // Wave 6: Hinder preserves instead_of
        (
            "Hinder",
            "Instant",
            "Counter target spell. If that spell is countered this way, put that card on your choice of the top or bottom of its owner's library instead of into that player's graveyard.",
            &[
                "\"ReplaceEffectWith\"",
                "\"instead_of\":\"into that player's graveyard\"",
            ],
            &[],
        ),
        // Wave 6: Volrath's Laboratory chosen color & type token
        (
            "Volrath's Laboratory",
            "Artifact",
            "As Volrath's Laboratory enters, choose a color and a creature type.\n{5}, {T}: Create a 2/2 creature token of the chosen color and type.",
            &[
                "\"CreateQualifiedToken\"",
                "\"chosen_color\":true",
                "\"chosen_type\":true",
            ],
            &[],
        ),
        // Wave 6: Dragon's Approach SelfName (not MATCHES_NAMED_AGENDA)
        (
            "Dragon's Approach",
            "Sorcery",
            "Dragon's Approach deals 3 damage to each opponent. You may exile Dragon's Approach and four cards named Dragon's Approach from your graveyard.",
            &["\"SelfName\""],
            &[],
        ),
        // Wave 7: Essence of the Wild & Infinite Reflection OthersEnterAsCopy
        (
            "Essence of the Wild",
            "Creature \u{2014} Avatar",
            "Creatures you control enter as a copy of Essence of the Wild.",
            &["\"OthersEnterAsCopy\""],
            &["\"EntersAsCopy\""],
        ),
        (
            "Infinite Reflection",
            "Enchantment \u{2014} Aura",
            "Enchant creature\nNontoken creatures you control enter as a copy of enchanted creature.",
            &["\"OthersEnterAsCopy\""],
            &["\"EntersAsCopy\""],
        ),
        // Wave 7: EntersAsCopyModified (tapped / until end of turn)
        (
            "Tapped Clone Test",
            "Creature \u{2014} Shapeshifter",
            "You may have Tapped Clone Test enter tapped as a copy of any creature on the battlefield.",
            &["\"EntersAsCopyModified\"", "\"enters_tapped\":true"],
            &[],
        ),
        // Wave 7: As ~ becomes attached to a creature -> AsBecomesAttachedAction
        (
            "Attach Choice Test",
            "Enchantment \u{2014} Aura",
            "Enchant creature\nAs Attach Choice Test becomes attached to a creature, choose a color.",
            &["\"AsBecomesAttachedAction\""],
            &["\"AsEntersAction\""],
        ),
        // Wave 7: Green Slime AbilityCounteredThisWay (not NotManaAbility)
        (
            "Green Slime",
            "Creature \u{2014} Ooze",
            "Flash\nWhen Green Slime enters, counter target activated or triggered ability from an artifact or enchantment source. If a permanent's ability is countered this way, destroy that permanent.",
            &["\"AbilityCounteredThisWay\"", "\"permanent_only\":true"],
            &["\"NotManaAbility\""],
        ),
        // Wave 7: Excess damage trigger sets excess_only: true
        (
            "Excess Damage Test",
            "Enchantment",
            "Whenever excess damage is dealt to a creature an opponent controls, draw a card.",
            &["\"excess_only\":true"],
            &["\"excess_only\":false"],
        ),
        // Wave 7: Not that player's turn uses IsPlayersTurn(ThatPlayer), not IsYourTurn
        (
            "Not Their Turn Test",
            "Instant",
            "If it's not that player's turn, draw a card.",
            &["\"IsPlayersTurn\":\"ThatPlayer\""],
            &["\"IsYourTurn\""],
        ),
        // Wave 7: It's not a creature uses MatchesFilter, not ControlsCount
        (
            "Not Filter Test",
            "Instant",
            "If it isn't a creature, draw a card.",
            &["\"MatchesFilter\""],
            &["\"ControlsCount\""],
        ),
        // Wave 7: Opponent entered / left battlefield / filtered morbid use ZoneChangeThisTurn
        (
            "Zone Change Cond Test",
            "Instant",
            "If a creature left the battlefield this turn, draw a card.",
            &["\"ZoneChangeThisTurn\""],
            &["\"EnteredUnderYourControlThisTurn\""],
        ),
        // Wave 7: Shuffle library trigger does not emit LibrarySearched
        (
            "Shuffle Trigger Test",
            "Enchantment",
            "Whenever a player shuffles their library, you gain 1 life.",
            &["\"shuffle_library\""],
            &["\"LibrarySearched\""],
        ),
        // Wave 7: You cast your commander sets IS_COMMANDER flag (1 << 24 = 16777216)
        (
            "Commander Cast Trigger Test",
            "Enchantment",
            "Whenever you cast your commander, draw a card.",
            &["\"flags\":16777216"],
            &["\"spell\":null"],
        ),
        // Pass E11: Flash cast permission with additional non-mana cost or behold condition
        (
            "Tegwyll's Scouring",
            "Sorcery",
            "You may cast Tegwyll's Scouring as though it had flash by tapping three untapped creatures you control with flying in addition to paying its other costs.\nDestroy all creatures.",
            &[
                "\"GrantModifiedCastPermission\"",
                "\"as_though_flash\":true",
                "tapping three untapped creatures you control with flying",
            ],
            &[],
        ),
        (
            "Molten Exhale",
            "Sorcery",
            "You may cast Molten Exhale as though it had flash if you behold a Dragon as an additional cost to cast it.\nMolten Exhale deals 4 damage to target creature.",
            &[
                "\"GrantModifiedCastPermission\"",
                "\"as_though_flash\":true",
                "if you behold a dragon as an additional cost to cast it",
            ],
            &[],
        ),
    ];

    for (name, tl, ot, must_have, must_not_have) in cases {
        let compiled = compile(name, tl, ot);
        assert!(
            compiled.strictly_compiled,
            "Pass E11 card {name} failed: {:?}",
            compiled.faces[0].failures
        );
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        assert!(!json.contains("\"StructuredRule\""), "{name}: {json}");
        assert!(!json.contains("\"GameStateFlag\""), "{name}: {json}");
        for tok in *must_have {
            assert!(json.contains(tok), "{name} missing {tok}: {json}");
        }
        for bad in *must_not_have {
            assert!(!json.contains(bad), "{name} must NOT contain {bad}: {json}");
        }
    }
}

#[test]
fn pass_e12_lossless_ast_hardening_regressions() {
    let members: Vec<String> = [
        "Human", "Soldier", "Beast", "Zombie", "Dragon", "Elf", "Goblin", "Wizard", "Ape",
        "Monkey", "Eldrazi",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let vocab = Vocabulary::from_creature_members(&members);
    let compile = |name: &str, type_line: &str, oracle_text: &str| {
        compile_card(
            &CardInput {
                oracle_id: "test-e12",
                name,
                layout: "normal",
                faces: vec![FaceInput {
                    name,
                    type_line,
                    oracle_text,
                }],
            },
            &vocab,
        )
    };
    type E12Case<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);
    let cases: &[E12Case<'_>] = &[
        // 1. Chosen type vs chosen color split (flags bits 26 vs 27)
        (
            "Chosen Type Test",
            "Enchantment",
            "Creatures you control of the chosen type get +1/+1.",
            &["\"flags\":67108864"],
            &["\"flags\":16384"],
        ),
        (
            "Chosen Color Test",
            "Enchantment",
            "Creatures you control of the chosen color get +1/+1.",
            &["\"flags\":134217728"],
            &["\"flags\":16384"],
        ),
        // 2. Zone suffixes: ItsOwnerGraveyard, exile owner, milled/library-this-turn
        (
            "Owner Exile Test",
            "Sorcery",
            "Return target creature card you own in exile to your hand.",
            &["\"zone\":\"Exile\"", "\"owner\":\"You\""],
            &[],
        ),
        (
            "Its Owner GY Test",
            "Sorcery",
            "Exile target creature card in its owner's graveyard.",
            &["\"zone\":\"ItsOwnerGraveyard\""],
            &["\"zone\":\"AnyGraveyard\""],
        ),
        // 3. Didn't attack this turn -> DID_NOT_ATTACK_THIS_TURN (1 << 30 = 1073741824), not CombatState::NotAttacking
        (
            "Didnt Attack Test",
            "Sorcery",
            "Destroy target creature that didn't attack or enter this turn.",
            &["1073741824", "\"DidNotEnterThisTurn\""],
            &["\"NotAttacking\""],
        ),
        // 4. Sacrifice another -> EXCLUDE_SELF (bit 0)
        (
            "Sacrifice Another Cost Test",
            "Creature \u{2014} Human",
            "Sacrifice another creature: Draw a card.",
            &["\"flags\":1"],
            &["\"flags\":0"],
        ),
        // 5. Commander filter -> IS_COMMANDER (1 << 24 = 16777216), not LEGENDARY
        (
            "Commander Filter Test",
            "Instant",
            "Destroy target commander.",
            &["\"flags\":16777216"],
            &["\"required_supertypes\":1"],
        ),
        // 6. Keywords OR vs AND (KEYWORDS_MATCH_ANY = 1 << 31 = 2147483648)
        (
            "Keywords Or Test",
            "Instant",
            "Destroy target creature with flying or reach.",
            &["2147483648"],
            &[],
        ),
        // 7. Subtypes OR -> SubtypesMatchAny
        (
            "Subtypes Or Test",
            "Instant",
            "Destroy target Ape or Monkey.",
            &["\"SubtypesMatchAny\""],
            &[],
        ),
        // 8. Multi-way ManaSpecAst::Choice splits all 3 options cleanly
        (
            "Tri-Color Mana Test",
            "Land",
            "{T}: Add {W}, {U}, or {B}.",
            &["\"Choice\":[\"{W}\",\"{U}\",\"{B}\"]"],
            &[" or "],
        ),
        // 9. Token creation preserves attacking target
        (
            "Attacking Target Token Test",
            "Enchantment",
            "Whenever a creature attacks, create a 1/1 white Soldier creature token that's tapped and attacking defending player.",
            &["\"CreateQualifiedToken\"", "\"DefendingPlayer\""],
            &[],
        ),
        // 10. Conditions: controls 3 or fewer lands -> ThatPlayer; targets/attached/is -> typed variants
        (
            "Controls Three Or Fewer Lands Test",
            "Instant",
            "If that player controls three or fewer lands, draw a card.",
            &[
                "\"ControlsCount\"",
                "\"player\":\"ThatPlayer\"",
                "\"value\":3",
            ],
            &["\"PlayerActionThisTurn\""],
        ),
        (
            "Targets Subject Typed Test",
            "Instant",
            "If that spell targets a creature you control, draw a card.",
            &["\"TargetsSubject\""],
            &["\"targets:"],
        ),
        (
            "Matches Filter Typed Test",
            "Instant",
            "If that permanent is a Zombie, draw a card.",
            &["\"MatchesFilter\""],
            &["\"is:"],
        ),
        // 11. CRITICAL: "A or B" emits Choice, not AND concatenation
        (
            "Or Choice Not And Test",
            "Sorcery",
            "You gain 3 life or draw a card.",
            &["\"Choice\""],
            &[],
        ),
        // 12. DelayedTrigger preserves YourNext scope
        (
            "Delayed Scope Test",
            "Instant",
            "At the beginning of your next end step, draw a card.",
            &["\"DelayedTrigger\"", "\"scope\":\"YourNext\""],
            &["\"scope\":\"Next\""],
        ),
        // 13. Enters tapped and with counters preserves tapped
        (
            "Tapped With Counters Test",
            "Artifact",
            "Tapped With Counters Test enters tapped and with three charge counters on it.",
            &["\"EntersTappedAndWithCounters\""],
            &["\"EntersWithCounters\""],
        ),
        // 14. Optional enters tapped preserves optionality
        (
            "Optional Tapped Test",
            "Land",
            "You may have Optional Tapped Test enter tapped.",
            &["\"OptionalEntersTapped\""],
            &["\"EntersTapped\""],
        ),
        // 15. As ~ transforms -> AsTransformsAction (not AsEntersAction)
        (
            "As Transforms Test",
            "Creature \u{2014} Human",
            "As As Transforms Test transforms into As Transforms Test, choose a creature type.",
            &["\"AsTransformsAction\""],
            &["\"AsEntersAction\""],
        ),
        // 16. Caged Sun -> LandTappedForManaColor (not TapStateChanged SelfObject)
        (
            "Caged Sun Trigger Test",
            "Artifact",
            "Whenever a land's ability causes you to add one or more mana of the chosen color, add an additional one mana of that color.",
            &["\"LandTappedForManaColor\""],
            &["\"TapStateChanged\""],
        ),
        // 17. Becomes blocked by two or more creatures -> BecomesBlockedByCount
        (
            "Blocked By Two Test",
            "Creature \u{2014} Beast",
            "Whenever Blocked By Two Test becomes blocked by two or more creatures, draw a card.",
            &["\"BecomesBlockedByCount\"", "\"min_blockers\":2"],
            &[],
        ),
        // 18. Enters from graveyard preserves from: graveyard
        (
            "Enters From GY Test",
            "Enchantment",
            "Whenever a creature enters from a graveyard, draw a card.",
            &["\"from\":\"graveyard\""],
            &["\"from\":null"],
        ),
        // 19. Wave E9: goaded for as long as they control it uses SubjectControlledBy
        (
            "Goaded Controlled Test",
            "Instant",
            "It's goaded for as long as they control it.",
            &["\"SubjectControlledBy\""],
            &["\"ControlsCount\""],
        ),
        // 20. Wave E9: Spawnsire cast any number preserves AnyNumber
        (
            "Any Number Outside Game Test",
            "Sorcery",
            "Cast any number of Eldrazi cards from among cards you own outside the game without paying their mana costs.",
            &["\"AnyNumber\""],
            &["\"count_limit\":null"],
        ),
        // 21. No Debug formatting in set_type / remove_snow / exchange_control
        (
            "No Debug Format Test",
            "Instant",
            "Target land becomes Forests until end of turn.",
            &["\"SetBasicLandType\""],
            &["set_type:"],
        ),
    ];

    let mut errs = Vec::new();
    for (name, tl, ot, must_have, must_not_have) in cases {
        let compiled = compile(name, tl, ot);
        if !compiled.strictly_compiled {
            errs.push(format!(
                "Pass E12 card {name} failed: {:?}",
                compiled.faces[0].failures
            ));
            continue;
        }
        let json = serde_json::to_string(&compiled).unwrap_or_default();
        if json.contains("\"StructuredRule\"") {
            errs.push(format!("{name} has StructuredRule: {json}"));
        }
        if json.contains("\"GameStateFlag\"") {
            errs.push(format!("{name} has GameStateFlag: {json}"));
        }
        for tok in *must_have {
            if !json.contains(tok) {
                errs.push(format!("{name} missing {tok}: {json}"));
            }
        }
        for bad in *must_not_have {
            if json.contains(bad) {
                errs.push(format!("{name} must NOT contain {bad}: {json}"));
            }
        }
    }
    assert!(errs.is_empty(), "Pass E12 failures:\n{}", errs.join("\n"));

    // 22. compile_granted_ability compiles Planeswalker loyalty & Equipment abilities
    let loyalty = compile_granted_ability("+1: Draw a card.", &vocab);
    assert!(
        loyalty.is_some(),
        "Planeswalker loyalty granted ability failed"
    );
    let equip = compile_granted_ability("Equip {2}", &vocab);
    assert!(equip.is_some(), "Equipment granted ability failed");
}
