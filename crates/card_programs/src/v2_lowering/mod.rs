//! Strictly-typed, fail-closed `v2::ir -> spellwake_engine::program::Program` lowering layer.
//!
//! Translates compiled `spellwake_oracle_compiler::v2` ASTs directly into native Spellwake
//! Universal Rules Toolbox programs without string parsing, regex, or raw `oracle_text`
//! inspection outside `card_meta.rs` printed frame fields.

pub(crate) mod atoms;
pub(crate) mod card;
pub(crate) mod card_meta;
pub(crate) mod condition;
pub(crate) mod cost;
pub(crate) mod ctx;
pub(crate) mod effect;
pub(crate) mod error;
pub(crate) mod filter;
pub(crate) mod keywords;
pub(crate) mod mutation;
pub(crate) mod replacement;
pub(crate) mod static_ability;
pub(crate) mod token;
pub(crate) mod trigger;
pub(crate) mod value;
pub(crate) mod zone_op;

pub use card::{
    compile_v2, compile_v2_modal_dfc, compile_v2_planeswalker, compile_v2_transforming,
    compile_v2_with_inventory, compile_v2_with_reason,
};
pub use error::{LowerResult, LoweringError};

#[cfg(test)]
mod tests {
    use std::cmp::Reverse;
    use std::collections::BTreeMap;

    use spellwake_content::{CatalogFace, InventoryCard};
    use spellwake_contracts::creature_types::CreatureTypeVocabulary;
    use spellwake_engine::{
        attachment::AttachmentKind,
        characteristics::KeywordAbility,
        program::{Program, ProgramKind},
    };
    use spellwake_oracle_compiler::{
        CardInput, FaceInput,
        v2::{compile_card_v2, ir as v2},
    };

    use super::*;
    use crate::v2_lowering::{
        ctx::{LowerCtx, cached_oracle_vocab},
        static_ability::lower_static_into_program,
    };

    fn test_vocab() -> CreatureTypeVocabulary {
        let mut members = vec![
            "Beast".to_string(),
            "Druid".to_string(),
            "Elf".to_string(),
            "Goblin".to_string(),
            "Human".to_string(),
            "Phyrexian".to_string(),
            "Germ".to_string(),
            "Soldier".to_string(),
            "Spirit".to_string(),
            "Warrior".to_string(),
            "Zombie".to_string(),
        ];
        members.sort();
        CreatureTypeVocabulary::new("v2-lowering-test-vocab".to_string(), [0u8; 32], members)
            .expect("test vocab")
    }

    fn test_card(
        name: &str,
        type_line: &str,
        mana_cost: Option<&str>,
        pt: Option<(&str, &str)>,
        oracle_text: &str,
    ) -> InventoryCard {
        InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".to_string(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".to_string()),
            name: name.to_string(),
            layout: "normal".to_string(),
            faces: vec![CatalogFace {
                name: name.to_string(),
                type_line: Some(type_line.to_string()),
                oracle_text: Some(oracle_text.to_string()),
                mana_cost: mana_cost.map(str::to_string),
                power: pt.map(|(p, _)| p.to_string()),
                toughness: pt.map(|(_, t)| t.to_string()),
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "v2-unit-test",
        }
    }

    #[test]
    fn lowers_lightning_bolt() {
        let vocab = test_vocab();
        let card = test_card(
            "Lightning Bolt",
            "Instant",
            Some("{R}"),
            None,
            "Lightning Bolt deals 3 damage to any target.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Lightning Bolt");
        assert!(matches!(program.kind, ProgramKind::Spell(_)));
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_elvish_visionary() {
        let vocab = test_vocab();
        let card = test_card(
            "Elvish Visionary",
            "Creature \u{2014} Elf",
            Some("{1}{G}"),
            Some(("1", "1")),
            "When Elvish Visionary enters, draw a card.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Elvish Visionary");
        assert!(matches!(program.kind, ProgramKind::Permanent(_)));
        assert_eq!(program.triggered.len(), 1);
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_llanowar_elves_as_pure_mana_ability() {
        let vocab = test_vocab();
        let card = test_card(
            "Llanowar Elves",
            "Creature \u{2014} Elf Druid",
            Some("{G}"),
            Some(("1", "1")),
            "{T}: Add {G}.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Llanowar Elves");
        assert_eq!(program.mana.len(), 1);
        assert!(program.activated.is_empty());
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_counterspell() {
        let vocab = test_vocab();
        let card = test_card(
            "Counterspell",
            "Instant",
            Some("{U}{U}"),
            None,
            "Counter target spell.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Counterspell");
        assert!(matches!(program.kind, ProgramKind::Spell(_)));
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_wrath_of_god() {
        let vocab = test_vocab();
        let card = test_card(
            "Wrath of God",
            "Sorcery",
            Some("{2}{W}{W}"),
            None,
            "Destroy all creatures. They can't be regenerated.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Wrath of God");
        assert!(matches!(program.kind, ProgramKind::Spell(_)));
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_swords_to_plowshares() {
        let vocab = test_vocab();
        let card = test_card(
            "Swords to Plowshares",
            "Instant",
            Some("{W}"),
            None,
            "Exile target creature. Its controller gains life equal to its power.",
        );
        let program = compile_v2_with_reason(&card, &vocab).expect("Swords to Plowshares");
        assert!(matches!(program.kind, ProgramKind::Spell(_)));
        assert!(program.validate(&vocab).is_ok());
    }

    #[test]
    fn lowers_custom_cards_keywords_aura_equipment_and_mana_rock() {
        let vocab = test_vocab();

        // Keyword creature
        let angel = test_card(
            "Serra Vanguard",
            "Creature \u{2014} Human Soldier",
            Some("{2}{W}{W}"),
            Some(("3", "3")),
            "Flying, vigilance, lifelink",
        );
        let p = compile_v2_with_reason(&angel, &vocab).expect("keyword creature");
        assert!(p.printed_keywords.contains(&KeywordAbility::Flying));
        assert!(p.printed_keywords.contains(&KeywordAbility::Vigilance));
        assert!(p.printed_keywords.contains(&KeywordAbility::Lifelink));

        // Mana rock
        let rock = test_card(
            "Emerald Relic",
            "Artifact",
            Some("{2}"),
            None,
            "{T}: Add {G}.",
        );
        let p = compile_v2_with_reason(&rock, &vocab).expect("mana rock");
        assert_eq!(p.mana.len(), 1);

        // Aura with Enchant creature
        let aura = test_card(
            "Radiant Mantle",
            "Enchantment \u{2014} Aura",
            Some("{1}{W}"),
            None,
            "Enchant creature\nEnchanted creature gets +2/+2 and has flying.",
        );
        let p = compile_v2_with_reason(&aura, &vocab).expect("aura");
        assert!(matches!(
            p.permanent().and_then(|d| d.attachment),
            Some(AttachmentKind::Aura(_))
        ));

        // Equipment with Equip {2}
        let sword = test_card(
            "Soldier's Blade",
            "Artifact \u{2014} Equipment",
            Some("{1}"),
            None,
            "Equipped creature gets +2/+0.\nEquip {2}",
        );
        let p = compile_v2_with_reason(&sword, &vocab).expect("equipment");
        assert_eq!(p.activated.len(), 1);
        assert!(matches!(
            p.permanent().and_then(|d| d.attachment),
            Some(AttachmentKind::Equipment)
        ));
    }

    #[test]
    fn fails_closed_on_cost_reduction_rule_and_unparsed_prose() {
        let vocab = test_vocab();

        // Direct IR check: CostReductionRule must fail closed instead of silently no-oping.
        let mut prog = Program::artifact(None, Vec::new());
        let mut ctx = LowerCtx::new(&vocab, "Test Reducer", "Test Reducer");
        let err = lower_static_into_program(
            &v2::StaticAbility::GlobalFlag(v2::StaticFlagKind::CostReductionRule),
            &mut prog,
            &mut ctx,
        )
        .expect_err("CostReductionRule must fail closed");
        assert!(matches!(err, LoweringError::Unsupported(_)));

        // Unparseable oracle text must fail closed with ParseIncomplete.
        let bad = test_card(
            "Nonsense Card",
            "Creature \u{2014} Human",
            Some("{1}{U}"),
            Some(("2", "2")),
            "Flying\nFlibbertigibbet the multiverse twice.",
        );
        assert_eq!(
            compile_v2_with_reason(&bad, &vocab),
            Err(LoweringError::ParseIncomplete)
        );
    }

    #[test]
    fn c3_unbound_anaphora_fails_closed() {
        let vocab = test_vocab();
        let destroy_that = test_card(
            "Dangling Anaphora Creature",
            "Sorcery",
            Some("{1}{B}"),
            None,
            "Destroy that creature.",
        );
        let err = compile_v2_with_reason(&destroy_that, &vocab)
            .expect_err("unbound 'that creature' must fail closed");
        assert_eq!(err.reason_code(), "v2:subject:unbound-anaphora");

        let exile_that = test_card(
            "Dangling Anaphora Permanent",
            "Sorcery",
            Some("{1}{W}"),
            None,
            "Exile that permanent.",
        );
        let err2 = compile_v2_with_reason(&exile_that, &vocab)
            .expect_err("unbound 'that permanent' must fail closed");
        assert_eq!(err2.reason_code(), "v2:subject:unbound-anaphora");
    }

    #[test]
    fn c3_unbound_object_controller_and_owner_fail_closed() {
        let vocab = test_vocab();
        let unbound_ctrl = test_card(
            "Dangling Controller",
            "Sorcery",
            Some("{B}"),
            None,
            "Its controller loses 2 life.",
        );
        let err = compile_v2_with_reason(&unbound_ctrl, &vocab)
            .expect_err("unbound 'its controller' must fail closed");
        assert_eq!(err.reason_code(), "v2:player:unbound-object-controller");

        // Even when a non-targeted ZoneOp populates `last_moved_binding`, `lower_resolving_player`
        // cannot express an arbitrary binding id and must fail closed instead of falling back to
        // `ResolvingPlayer::Controller` or `ResolvingPlayer::SourceOwner`.
        let mut ctx = LowerCtx::new(&vocab, "Unbound ResolvingPlayer", "Unbound ResolvingPlayer");
        ctx.last_moved_binding = Some(ctx.alloc_binding().expect("alloc binding"));
        let mut acc = crate::v2_lowering::ctx::BodyAcc::new();
        let err_rp_ctrl = crate::v2_lowering::atoms::lower_resolving_player(
            v2::PlayerRef::ItsController,
            &mut acc,
            &ctx,
        )
        .expect_err(
            "lower_resolving_player(ItsController) with only last_moved_binding must fail closed",
        );
        assert_eq!(
            err_rp_ctrl.reason_code(),
            "v2:player:unbound-object-controller"
        );

        let err_rp_owner = crate::v2_lowering::atoms::lower_resolving_player(
            v2::PlayerRef::ItsOwner,
            &mut acc,
            &ctx,
        )
        .expect_err(
            "lower_resolving_player(ItsOwner) with only last_moved_binding must fail closed",
        );
        assert_eq!(err_rp_owner.reason_code(), "v2:player:unbound-object-owner");

        let unbound_owner = test_card(
            "Dangling Owner",
            "Sorcery",
            Some("{W}"),
            None,
            "Its owner gains 3 life.",
        );
        let err_owner = compile_v2_with_reason(&unbound_owner, &vocab)
            .expect_err("unbound 'its owner' must fail closed");
        assert_eq!(err_owner.reason_code(), "v2:player:unbound-object-owner");
    }

    #[test]
    fn c3_zone_op_permissive_subject_wildcards_fail_closed() {
        let vocab = test_vocab();
        let mut ctx = LowerCtx::new(&vocab, "ZoneOp Guard Test", "ZoneOp Guard Test");
        let mut acc = crate::v2_lowering::ctx::BodyAcc::new();

        let creature_filter = v2::ObjectFilter {
            all_types: spellwake_oracle_compiler::ast::CardTypeMask::CREATURE,
            ..v2::ObjectFilter::default()
        };
        let counted_creatures = v2::SubjectRef::Counted {
            filter: Box::new(creature_filter),
            count: v2::CountSpec {
                count: v2::ValueExpr::Const(2),
                up_to: false,
                any_number: false,
            },
        };
        let one = v2::CountSpec {
            count: v2::ValueExpr::Const(1),
            up_to: false,
            any_number: false,
        };
        let mods = v2::EntryModifiers::default();

        // Discard: unexpected subject fails closed.
        let err_discard = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::Discard {
                hand: false,
                random: false,
            },
            &v2::SubjectRef::AnyTarget,
            &[],
            None,
            &mods,
            None,
            Some(&one),
            &mut acc,
            &mut ctx,
        )
        .expect_err("discard with unexpected AnyTarget subject must fail closed");
        assert_eq!(err_discard.reason_code(), "v2:discard-unsupported-subject");

        // Draw: filtered Counted subject and unexpected SelfObject fail closed.
        let err_draw_filtered = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::Draw,
            &counted_creatures,
            &[],
            None,
            &mods,
            None,
            None,
            &mut acc,
            &mut ctx,
        )
        .expect_err("draw with filtered Counted subject must fail closed");
        assert_eq!(
            err_draw_filtered.reason_code(),
            "v2:draw-unsupported-subject"
        );

        let err_draw_self = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::Draw,
            &v2::SubjectRef::SelfObject,
            &[],
            None,
            &mods,
            None,
            None,
            &mut acc,
            &mut ctx,
        )
        .expect_err("draw with SelfObject subject must fail closed");
        assert_eq!(err_draw_self.reason_code(), "v2:draw-unsupported-subject");

        // Mill: unexpected SelfObject fails closed.
        let err_mill = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::Mill,
            &v2::SubjectRef::SelfObject,
            &[],
            None,
            &mods,
            None,
            None,
            &mut acc,
            &mut ctx,
        )
        .expect_err("mill with SelfObject subject must fail closed");
        assert_eq!(err_mill.reason_code(), "v2:mill-unsupported-subject");

        // LookTop: filtered Counted subject fails closed.
        let err_look = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::LookTop { reveal: false },
            &counted_creatures,
            &[],
            None,
            &mods,
            None,
            None,
            &mut acc,
            &mut ctx,
        )
        .expect_err("look_top with filtered Counted subject must fail closed");
        assert_eq!(err_look.reason_code(), "v2:look-top-unsupported-subject");

        // ExileUntilReveal: unexpected SelfObject fails closed.
        let err_exile_until = crate::v2_lowering::zone_op::lower_zone_op(
            v2::PlayerRef::You,
            &v2::ZoneOpKind::ExileUntilReveal {
                filter: Box::new(v2::ObjectFilter {
                    all_types: spellwake_oracle_compiler::ast::CardTypeMask::LAND,
                    ..v2::ObjectFilter::default()
                }),
            },
            &v2::SubjectRef::SelfObject,
            &[],
            None,
            &mods,
            None,
            None,
            &mut acc,
            &mut ctx,
        )
        .expect_err("exile_until_reveal with SelfObject subject must fail closed");
        assert_eq!(
            err_exile_until.reason_code(),
            "v2:exile-until-reveal-unsupported-subject"
        );
    }

    #[test]
    fn c3_custom_card_report_surfaces_v2_reason_and_cost_reuses_counter_lookup() {
        let vocab = test_vocab();
        let input = crate::custom_card::CustomCardCompileInput {
            layout: "normal".to_string(),
            rarity: "uncommon".to_string(),
            faces: vec![crate::custom_card::CustomCardFaceRules {
                name: "Unbound Diagnostic Spell".to_string(),
                mana_cost: Some("{1}{B}".to_string()),
                type_line: "Sorcery".to_string(),
                oracle_text: "Destroy that creature.".to_string(),
                power: None,
                toughness: None,
                loyalty: None,
                defense: None,
                color_indicator: None,
            }],
        };
        let compiled_custom = crate::custom_card::compile_custom_card(&input, &vocab);
        assert!(
            compiled_custom.program.is_none(),
            "unbound anaphora spell must not compile"
        );
        let report = &compiled_custom.report;
        assert!(!report.stage2_playable);
        let stage2_diag = report
            .diagnostics
            .iter()
            .find(|d| d.stage == crate::custom_card::DiagnosticStage::Stage2)
            .expect("expected a Stage2 diagnostic");
        assert_ne!(stage2_diag.code, "outside-ordinary-creature-type");
        assert!(
            stage2_diag.code.starts_with("v2:"),
            "expected v2:* blocker reason, got {:?}",
            stage2_diag.code
        );
        assert_eq!(stage2_diag.code, "v2:subject:unbound-anaphora");

        // Cost lowering now delegates to atoms::lower_counter_kind (supports shield/stun/etc.).
        let shield_cost_card = test_card(
            "Shield Conduit",
            "Artifact",
            Some("{2}"),
            None,
            "{T}, Remove a shield counter from Shield Conduit: Draw a card.",
        );
        let compiled = compile_v2_with_reason(&shield_cost_card, &vocab)
            .expect("shield counter removal cost should lower via atoms::lower_counter_kind");
        assert!(compiled.validate(&vocab).is_ok());
    }

    #[test]
    #[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
    fn v2_lowering_corpus_coverage_audit() {
        let (inventory, vocabulary, _registry) =
            crate::keyword_creatures::tests::current_catalogue();
        let oracle_vocab = cached_oracle_vocab(&vocabulary);

        let mut total_required = 0usize;
        let mut v2_parsed_ok = 0usize;
        let mut v2_lowered_ok = 0usize;
        let dump_samples = std::env::var("V2_DUMP_SAMPLES").is_ok();
        let top_n: usize = std::env::var("V2_TOP_N")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40);
        let inspect_codes: Vec<String> = std::env::var("V2_INSPECT_CODE")
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let inspect_limit: usize = std::env::var("V2_INSPECT_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        let mut blocker_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut blocker_samples: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();

        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            total_required += 1;

            let face_inputs: Vec<FaceInput<'_>> = card
                .faces
                .iter()
                .map(|f| FaceInput {
                    name: &f.name,
                    type_line: f.type_line.as_deref().unwrap_or(""),
                    oracle_text: f.oracle_text.as_deref().unwrap_or(""),
                })
                .collect();
            let input = CardInput {
                oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                name: &card.name,
                layout: &card.layout,
                faces: face_inputs,
            };
            let compiled = compile_card_v2(&input, &oracle_vocab);
            if compiled.strictly_compiled {
                v2_parsed_ok += 1;
            }

            match compile_v2_with_inventory(card, Some(inventory), &vocabulary, true) {
                Ok(_) => {
                    v2_lowered_ok += 1;
                }
                Err(err) => {
                    let code = err.reason_code();
                    *blocker_counts.entry(code).or_insert(0) += 1;
                    let inspected = inspect_codes.iter().any(|c| c == code);
                    if (dump_samples || inspected) && code != "v2:ast-parse-incomplete" {
                        let entry = blocker_samples.entry(code).or_default();
                        let cap = if inspected { inspect_limit } else { 3 };
                        if entry.len() < cap {
                            let oracle = card
                                .faces
                                .iter()
                                .map(|f| f.oracle_text.as_deref().unwrap_or(""))
                                .collect::<Vec<_>>()
                                .join(" // ");
                            let ir = format!("{:?}", compiled.faces);
                            let ir_cap = if inspected { 900 } else { 420 };
                            let ir_short: String = ir.chars().take(ir_cap).collect();
                            entry.push(format!(
                                "{} [{}]: {} || IR={}",
                                card.name,
                                card.layout,
                                oracle.replace('\n', " | "),
                                ir_short
                            ));
                        }
                    }
                }
            }
        }

        let total_pct = if total_required == 0 {
            0.0
        } else {
            (v2_lowered_ok as f64 / total_required as f64) * 100.0
        };
        let parsed_pct = if total_required == 0 {
            0.0
        } else {
            (v2_parsed_ok as f64 / total_required as f64) * 100.0
        };
        let bind_of_parsed_pct = if v2_parsed_ok == 0 {
            0.0
        } else {
            (v2_lowered_ok as f64 / v2_parsed_ok as f64) * 100.0
        };

        eprintln!("=== V2 LOWERING CORPUS COVERAGE AUDIT ===");
        eprintln!("Total required cards:     {total_required}");
        eprintln!("v2 strictly compiled:     {v2_parsed_ok} ({parsed_pct:.2}%)");
        eprintln!(
            "v2_lowering bound valid:  {v2_lowered_ok} ({total_pct:.2}% of corpus, {bind_of_parsed_pct:.2}% of v2 parsed)"
        );
        eprintln!("--- Top {top_n} blocker reasons ---");

        let mut ranked: Vec<(&'static str, usize)> = blocker_counts.into_iter().collect();
        ranked.sort_by_key(|&(code, count)| (Reverse(count), code));
        for (idx, (code, count)) in ranked.iter().take(top_n).enumerate() {
            eprintln!("{:>2}. {:>6}  {code}", idx + 1, count);
        }
        if !inspect_codes.is_empty() {
            eprintln!("--- Inspected Blocker Samples ---");
            for want in &inspect_codes {
                if let Some(&(code, count)) = ranked.iter().find(|(c, _)| *c == want.as_str()) {
                    eprintln!("=== {code} ({count}) ===");
                    if let Some(samples) = blocker_samples.get(code) {
                        for s in samples {
                            eprintln!("  * {s}");
                        }
                    }
                }
            }
        }
        if dump_samples {
            eprintln!("--- Blocker IR Samples (Top 30 non-parse) ---");
            for (code, count) in ranked
                .into_iter()
                .filter(|(c, _)| *c != "v2:ast-parse-incomplete")
                .take(30)
            {
                eprintln!("=== {code} ({count}) ===");
                if let Some(samples) = blocker_samples.get(code) {
                    for s in samples {
                        eprintln!("  * {s}");
                    }
                }
            }
        }
    }
}
