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
    #[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
    fn v2_lowering_corpus_coverage_audit() {
        let (inventory, vocabulary, _registry) =
            crate::keyword_creatures::tests::current_catalogue();
        let oracle_vocab = cached_oracle_vocab(&vocabulary);

        let mut total_required = 0usize;
        let mut v2_parsed_ok = 0usize;
        let mut v2_lowered_ok = 0usize;
        let dump_samples = std::env::var("V2_DUMP_SAMPLES").is_ok();
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
                    if dump_samples && code != "v2:ast-parse-incomplete" {
                        let entry = blocker_samples.entry(code).or_default();
                        if entry.len() < 3 {
                            let oracle = card
                                .faces
                                .iter()
                                .map(|f| f.oracle_text.as_deref().unwrap_or(""))
                                .collect::<Vec<_>>()
                                .join(" // ");
                            let ir = format!("{:?}", compiled.faces);
                            let ir_short: String = ir.chars().take(420).collect();
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
        eprintln!("--- Top 40 blocker reasons ---");

        let mut ranked: Vec<(&'static str, usize)> = blocker_counts.into_iter().collect();
        ranked.sort_by_key(|&(code, count)| (Reverse(count), code));
        for (idx, (code, count)) in ranked.iter().take(40).enumerate() {
            eprintln!("{:>2}. {:>6}  {code}", idx + 1, count);
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
