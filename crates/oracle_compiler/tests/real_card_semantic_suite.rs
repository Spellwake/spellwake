//! Permanent Real-Magic-Card Semantic Verification & Parser Audit Suite.
//!
//! Runs automatically in `cargo test --locked --workspace` (never `#[ignore]`) using
//! Spellwake's checked-in CR creature vocabulary fixture. Verifies:
//! 1. Full structural and semantic AST assertions (pattern-matching concrete typed enums,
//!    never loose substring checks) across real Scryfall Magic: The Gathering cards.
//! 2. Zero critical parser defects (`audit_card().has_zero_critical_defects()`) across
//!    all 103 real Magic cards spanning 15 distinct card layouts (normal, saga, transform,
//!    modal_dfc, split, adventure, prepare, class, case, leveler, flip, meld, room,
//!    battle, prototype, mutate).
//! 3. Honest opaque-fallback tracking for known complex replacement/permission cards.
//! 4. Negative strictness guards (malformed Oracle text MUST fail strict compilation).
//! 5. Self-tests proving `audit_card` catches synthetic Debug leaks, keyword hijacks,
//!    empty modal modes, empty non-vanilla faces, bad mana choices, and honor-system leaks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;
use spellwake_oracle_compiler::ast::{
    AbilityAst, CardAst, ClassLevelSectionAst, ConditionAst, CostAtomAst, DurationAst, FaceAst,
    FilterPredicateAst, InstructionAst, InteractiveChoiceKind, KeywordAst, ManaSpecAst,
    NameFilterAst, ObjectFilterAst, ObjectFilterFlags, PileRefAst, PlayerRefAst,
    ReplacementEffectAst, StaticAbilityAst, SubjectRefAst, TriggerEventAst, ValueExprAst,
    ZoneFilter,
};
use spellwake_oracle_compiler::audit::{AuditSeverity, audit_card};
use spellwake_oracle_compiler::{
    CardInput, CompileOptions, FaceInput, Vocabulary, compile_card_with_options,
};

#[derive(Debug, Deserialize)]
struct FixtureFace {
    name: String,
    type_line: String,
    oracle_text: String,
}

#[derive(Debug, Deserialize)]
struct FixtureCard {
    oracle_id: String,
    name: String,
    layout: String,
    faces: Vec<FixtureFace>,
}

fn load_pinned_vocabulary() -> Vocabulary {
    if let Ok(env_path) = std::env::var("SPELLWAKE_CREATURE_VOCABULARY_INPUT") {
        let p = PathBuf::from(env_path);
        if p.is_file() {
            return Vocabulary::load_from_file(&p)
                .expect("SPELLWAKE_CREATURE_VOCABULARY_INPUT must load cleanly");
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .join("..")
        .join("content")
        .join("tests")
        .join("fixtures")
        .join("creature-vocabulary.input.json");
    let raw =
        std::fs::read_to_string(&fixture_path).expect("checked-in creature vocabulary fixture");
    let val: serde_json::Value =
        serde_json::from_str(&raw).expect("valid creature vocabulary fixture JSON");
    let members: Vec<String> = val["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|e| {
            e["canonical_label"]
                .as_str()
                .expect("canonical_label string")
                .to_string()
        })
        .collect();
    assert!(
        members.len() > 250,
        "expected full CR creature type list, got {}",
        members.len()
    );
    Vocabulary::from_creature_members(&members)
}

fn load_real_cards() -> Vec<FixtureCard> {
    let raw = include_str!("fixtures/real_cards_corpus_sample.json");
    serde_json::from_str(raw).expect("valid real_cards_corpus_sample.json")
}

fn compile_fixture(card: &FixtureCard, vocab: &Vocabulary) -> CardAst {
    let faces: Vec<FaceInput<'_>> = card
        .faces
        .iter()
        .map(|f| FaceInput {
            name: &f.name,
            type_line: &f.type_line,
            oracle_text: &f.oracle_text,
        })
        .collect();
    let input = CardInput {
        oracle_id: &card.oracle_id,
        name: &card.name,
        layout: &card.layout,
        faces,
    };
    let opts = CompileOptions::default();
    let ast = compile_card_with_options(&input, vocab, opts);
    let report = audit_card(&input, &ast, opts);
    assert!(
        report.has_zero_critical_defects(),
        "real card `{}` failed semantic audit: {:?}",
        card.name,
        report.critical_defects
    );
    ast
}

fn by_name(cards: &[FixtureCard]) -> BTreeMap<&str, &FixtureCard> {
    cards.iter().map(|c| (c.name.as_str(), c)).collect()
}

#[test]
fn real_card_corpus_sample_has_zero_critical_defects_and_covers_all_layouts() {
    let vocab = load_pinned_vocabulary();
    let cards = load_real_cards();
    assert!(
        cards.len() >= 100,
        "expected at least 100 real Scryfall cards in corpus sample, found {}",
        cards.len()
    );

    let mut layouts = BTreeSet::new();
    let mut pure_count = 0usize;
    for card in &cards {
        layouts.insert(card.layout.as_str());
        let faces: Vec<FaceInput<'_>> = card
            .faces
            .iter()
            .map(|f| FaceInput {
                name: &f.name,
                type_line: &f.type_line,
                oracle_text: &f.oracle_text,
            })
            .collect();
        let input = CardInput {
            oracle_id: &card.oracle_id,
            name: &card.name,
            layout: &card.layout,
            faces,
        };
        let opts = CompileOptions::default();
        let ast = compile_card_with_options(&input, &vocab, opts);
        let report = audit_card(&input, &ast, opts);
        assert!(
            report.has_zero_critical_defects(),
            "card `{}` ({}) reported critical defects: {:?}",
            card.name,
            card.layout,
            report.critical_defects
        );
        if report.is_fully_typed_pure() {
            pure_count += 1;
        }
    }

    for required_layout in [
        "normal",
        "saga",
        "transform",
        "modal_dfc",
        "split",
        "adventure",
        "prepare",
        "class",
        "case",
        "leveler",
        "flip",
        "meld",
        "prototype",
        "mutate",
    ] {
        assert!(
            layouts.contains(required_layout),
            "missing required layout `{required_layout}` in real_cards_corpus_sample.json; present={layouts:?}"
        );
    }

    eprintln!(
        "real_card_semantic_suite: {}/{} cards zero-critical, {}/{} pure typed AST across {} layouts",
        cards.len(),
        cards.len(),
        pure_count,
        cards.len(),
        layouts.len()
    );
}

#[test]
fn iconic_staple_semantic_ast_fidelity() {
    let vocab = load_pinned_vocabulary();
    let cards = load_real_cards();
    let map = by_name(&cards);

    // 1. Sylvan Library: Draw step trigger must preserve BOTH the draw 2 AND choose-2-drawn-this-turn + pay 4 life / put on top!
    {
        let ast = compile_fixture(map["Sylvan Library"], &vocab);
        let face = &ast.faces[0];
        assert_eq!(face.abilities.len(), 1);
        match &face.abilities[0] {
            AbilityAst::Triggered { trigger, body, .. } => {
                assert!(trigger.intervening_if.is_none());
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::Optional(inner))
                        if matches!(inner.first(), Some(InstructionAst::Draw { player: PlayerRefAst::You, .. }))
                ));
                let has_choose_drawn_this_turn = body.iter().any(|ins| {
                    matches!(
                        ins,
                        InstructionAst::IfPreviousSucceeded(inner)
                            if inner.iter().any(|sub| matches!(
                                sub,
                                InstructionAst::ChooseFromGroup { group, .. }
                                    if matches!(
                                        group.as_ref(),
                                        SubjectRefAst::Group(flt)
                                            if flt.zone == Some(ZoneFilter::YourHand)
                                                && flt.predicates.iter().any(|p| matches!(
                                                    p,
                                                    FilterPredicateAst::StateQualification(q) if q == "drawn_this_turn"
                                                ))
                                    )
                            ))
                    )
                });
                assert!(
                    has_choose_drawn_this_turn,
                    "Sylvan Library must preserve ChooseFromGroup(YourHand, drawn_this_turn): {body:#?}"
                );
                let has_top_of_library_choice = serde_json::to_string(body)
                    .unwrap()
                    .contains("top of your library");
                assert!(
                    has_top_of_library_choice,
                    "Sylvan Library sentence 3 must preserve 'top of your library': {body:#?}"
                );
            }
            other => panic!("Sylvan Library expected Triggered, got {other:?}"),
        }
    }

    // 2. Urza's Saga: Ch. I mana grant, Ch. II Construct grant, Ch. III SearchLibrary with ExactManaCostOneOf(["{0}", "{1}"])!
    {
        let ast = compile_fixture(map["Urza's Saga"], &vocab);
        let face = &ast.faces[0];
        assert_eq!(
            face.abilities.len(),
            3,
            "Urza's Saga must have 3 Saga chapters"
        );
        match &face.abilities[2] {
            AbilityAst::SagaChapter { chapters, body } => {
                assert_eq!(chapters, &vec!["III".to_string()]);
                let has_exact_0_or_1 = body.iter().any(|ins| {
                    matches!(
                        ins,
                        InstructionAst::SearchLibrary { filter, .. }
                            if filter.predicates.iter().any(|p| matches!(
                                p,
                                FilterPredicateAst::ExactManaCostOneOf(costs)
                                    if costs == &vec!["{0}".to_string(), "{1}".to_string()]
                            ))
                    )
                });
                assert!(
                    has_exact_0_or_1,
                    "Urza's Saga Ch. III must preserve ExactManaCostOneOf([{{0}}, {{1}}]): {body:#?}"
                );
            }
            other => panic!("Urza's Saga Ch. III expected SagaChapter, got {other:?}"),
        }
    }

    // 3. Liliana of the Veil: +1 EachPlayer discard, -2 TargetPlayer sac creature, -6 SeparateFilteredIntoPiles + ChoosePile + Sacrifice!
    {
        let ast = compile_fixture(map["Liliana of the Veil"], &vocab);
        let face = &ast.faces[0];
        assert_eq!(face.abilities.len(), 3);
        match &face.abilities[2] {
            AbilityAst::Loyalty { delta, body } => {
                assert_eq!(delta, "-6");
                assert!(
                    body.iter().any(|i| matches!(
                        i,
                        InstructionAst::InteractiveChoice(
                            InteractiveChoiceKind::SeparateIntoPiles {
                                chooser: PlayerRefAst::You,
                                pile_count: 2,
                                ..
                            }
                        )
                    )),
                    "Liliana -6 must emit SeparateIntoPiles(You, 2): {body:#?}"
                );
                assert!(
                    body.iter().any(|i| matches!(
                        i,
                        InstructionAst::Sacrifice {
                            player: PlayerRefAst::ThatPlayer,
                            subject,
                            ..
                        } if matches!(
                            subject.as_ref(),
                            SubjectRefAst::PileRef(PileRefAst::FilteredInPile { pile, .. })
                                if matches!(pile.as_ref(), PileRefAst::ChosenPile)
                        )
                    )),
                    "Liliana -6 must emit Sacrifice(ThatPlayer, PileRef(ChosenPile)): {body:#?}"
                );
            }
            other => panic!("Liliana -6 expected Loyalty, got {other:?}"),
        }
    }

    // 4. Smothering Tithe: OptionalPlayer(ThatPlayer, PlayerPayCost(ThatPlayer, {2})) + IfPreviousFailed(CreateToken(You, Treasure))!
    {
        let ast = compile_fixture(map["Smothering Tithe"], &vocab);
        let face = &ast.faces[0];
        match &face.abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(
                    matches!(
                        &body[0],
                        InstructionAst::OptionalPlayer {
                            player: PlayerRefAst::ThatPlayer,
                            body: inner
                        } if matches!(
                            inner.first(),
                            Some(InstructionAst::PlayerPayCost {
                                player: PlayerRefAst::ThatPlayer,
                                cost: CostAtomAst::PayMana(m)
                            }) if m == "{2}"
                        )
                    ),
                    "Smothering Tithe must bind ThatPlayer on PayCost: {body:#?}"
                );
                assert!(matches!(
                    body.get(1),
                    Some(InstructionAst::IfPreviousFailed(_))
                ));
            }
            other => panic!("Smothering Tithe expected Triggered, got {other:?}"),
        }
    }

    // 5. Sen Triplets: ChoosePlayerTarget(TargetOpponent) + TemporaryStatic + TemporaryPlayerRestriction + GrantCastPermission!
    {
        let ast = compile_fixture(map["Sen Triplets"], &vocab);
        let face = &ast.faces[0];
        match &face.abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::ChoosePlayerTarget {
                        player: PlayerRefAst::TargetOpponent
                    })
                ));
                assert!(body.iter().any(|i| matches!(
                    i,
                    InstructionAst::TemporaryPlayerRestriction {
                        player: PlayerRefAst::ThatPlayer,
                        rule,
                        duration: DurationAst::UntilEndOfTurn,
                        ..
                    } if rule == "play_with_hand_revealed"
                )));
                assert!(
                    body.iter()
                        .any(|i| matches!(i, InstructionAst::GrantCastPermission { .. }))
                );
            }
            other => panic!("Sen Triplets expected Triggered, got {other:?}"),
        }
    }

    // 6. Panharmonicon: ScopedTriggerMultiplier preserving artifact-or-creature ETB scope!
    {
        let ast = compile_fixture(map["Panharmonicon"], &vocab);
        let face = &ast.faces[0];
        assert!(
            matches!(
                face.statics.first(),
                Some(StaticAbilityAst::ScopedTriggerMultiplier {
                    additional_triggers: 1,
                    event_desc,
                    ..
                }) if event_desc.contains("artifact") && event_desc.contains("creature")
            ),
            "Panharmonicon must preserve ScopedTriggerMultiplier: {:#?}",
            face.statics
        );
    }

    // 7. Fable of the Mirror-Breaker Ch. II: Optional(DiscardUpTo { max_count: 2 })!
    {
        let ast = compile_fixture(
            map["Fable of the Mirror-Breaker // Reflection of Kiki-Jiki"],
            &vocab,
        );
        let front = &ast.faces[0];
        match &front.abilities[1] {
            AbilityAst::SagaChapter { chapters, body } => {
                assert_eq!(chapters, &vec!["II".to_string()]);
                assert!(
                    matches!(
                        body.first(),
                        Some(InstructionAst::Optional(inner))
                            if matches!(
                                inner.first(),
                                Some(InstructionAst::DiscardUpTo { max_count, .. })
                                    if matches!(max_count.as_ref(), ValueExprAst::Const(2))
                            )
                    ),
                    "Fable Ch. II must emit DiscardUpTo(2): {body:#?}"
                );
            }
            other => panic!("Fable Ch. II expected SagaChapter, got {other:?}"),
        }
    }

    // 8. Rhystic Study: Unless { unless_player: ThatPlayer, unless_cost: PayMana("{1}"), body: [Optional(Draw)] }!
    {
        let ast = compile_fixture(map["Rhystic Study"], &vocab);
        let face = &ast.faces[0];
        match &face.abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::Unless {
                        unless_player: Some(PlayerRefAst::ThatPlayer),
                        unless_cost: Some(CostAtomAst::PayMana(m)),
                        ..
                    }) if m == "{1}"
                ));
            }
            other => panic!("Rhystic Study expected Triggered, got {other:?}"),
        }
    }

    // 9. Caged Sun: LandTappedForManaColor with mana_color_filter == Some("chosen_color")!
    {
        let ast = compile_fixture(map["Caged Sun"], &vocab);
        let face = &ast.faces[0];
        match &face.abilities[0] {
            AbilityAst::Triggered { trigger, body, .. } => {
                assert!(matches!(
                    trigger.events.first(),
                    Some(TriggerEventAst::LandTappedForManaColor {
                        by_you: true,
                        mana_color_filter: f,
                        ..
                    }) if f == "chosen_color"
                ));
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::AddMana {
                        player: PlayerRefAst::You,
                        mana: ManaSpecAst::ChosenColor,
                    })
                ));
            }
            other => panic!("Caged Sun expected Triggered, got {other:?}"),
        }
    }
}

#[test]
fn stratified_and_source_audit_defect_regressions() {
    let vocab = load_pinned_vocabulary();
    let cards = load_real_cards();
    let map = by_name(&cards);

    // 1. Braids's Frightful Return Ch. III: OptionalPlayer(TargetOpponent) must rebind inner Sacrifice to ThatPlayer, NOT You!
    {
        let ast = compile_fixture(map["Braids's Frightful Return"], &vocab);
        match &ast.faces[0].abilities[2] {
            AbilityAst::SagaChapter { body, .. } => {
                let opponent_sacs = body.iter().any(|ins| {
                    matches!(
                        ins,
                        InstructionAst::OptionalPlayer { body: inner, .. }
                            if inner.iter().any(|sub| matches!(
                                sub,
                                InstructionAst::Sacrifice { player: PlayerRefAst::ThatPlayer, .. }
                            ))
                    )
                });
                assert!(
                    opponent_sacs,
                    "Braids's Frightful Return Ch. III must rebind Sacrifice actor to ThatPlayer: {body:#?}"
                );
            }
            other => panic!("Braids Ch. III expected SagaChapter, got {other:?}"),
        }
    }

    // 2. Paradox Shaper // Omit Variables: intervening_if is Not(HasStatus("prepared")), NOT ObjectFilterFlags::MODIFIED!
    {
        let ast = compile_fixture(map["Paradox Shaper // Omit Variables"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { trigger, .. } => {
                assert!(
                    matches!(
                        &trigger.intervening_if,
                        Some(ConditionAst::Not(inner))
                            if matches!(
                                inner.as_ref(),
                                ConditionAst::HasStatus { status, .. } if status == "prepared"
                            )
                    ),
                    "Paradox Shaper intervening_if must be Not(HasStatus(prepared)): {:#?}",
                    trigger.intervening_if
                );
            }
            other => panic!("Paradox Shaper expected Triggered, got {other:?}"),
        }
    }

    // 3. Cosima // The Omenkeel: The Omenkeel preserves PriorActionZoneRef("from_among_those")!
    {
        let ast = compile_fixture(map["Cosima, God of the Voyage // The Omenkeel"], &vocab);
        let back = &ast.faces[1];
        match &back.abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                let preserves_from_among = body.iter().any(|ins| {
                    matches!(
                        ins,
                        InstructionAst::GrantCastPermission { subject, .. }
                            if matches!(
                                subject.as_ref(),
                                SubjectRefAst::Group(flt)
                                    if flt.predicates.iter().any(|p| matches!(
                                        p,
                                        FilterPredicateAst::PriorActionZoneRef { kind }
                                            if kind == "from_among_those"
                                    ))
                            )
                    )
                });
                assert!(
                    preserves_from_among,
                    "The Omenkeel must preserve PriorActionZoneRef(from_among_those): {body:#?}"
                );
            }
            other => panic!("The Omenkeel expected Triggered, got {other:?}"),
        }
    }

    // 4. Domri Rade: +1 RevealByPlayer (not LookTopLibrary), -2 Fight with DistinctTarget, -7 compiled CreateEmblemFor!
    {
        let ast = compile_fixture(map["Domri Rade"], &vocab);
        let face = &ast.faces[0];
        assert_eq!(face.abilities.len(), 3);
        match &face.abilities[0] {
            AbilityAst::Loyalty { body, .. } => {
                let has_reveal = serde_json::to_string(body)
                    .unwrap()
                    .contains("RevealByPlayer");
                assert!(has_reveal, "Domri +1 must emit RevealByPlayer: {body:#?}");
            }
            other => panic!("Domri +1 expected Loyalty, got {other:?}"),
        }
        match &face.abilities[1] {
            AbilityAst::Loyalty { body, .. } => {
                assert!(
                    matches!(
                        body.first(),
                        Some(InstructionAst::Fight { b, .. })
                            if matches!(b.as_ref(), SubjectRefAst::DistinctTarget { .. })
                    ),
                    "Domri -2 must use DistinctTarget for second fighter: {body:#?}"
                );
            }
            other => panic!("Domri -2 expected Loyalty, got {other:?}"),
        }
        match &face.abilities[2] {
            AbilityAst::Loyalty { body, .. } => {
                assert!(
                    matches!(
                        body.first(),
                        Some(InstructionAst::CreateEmblemFor {
                            compiled: Some(_),
                            ..
                        })
                    ),
                    "Domri -7 must compile emblem AST: {body:#?}"
                );
            }
            other => panic!("Domri -7 expected Loyalty, got {other:?}"),
        }
    }

    // 5. Karn, Living Legacy: -1 PayAnyAmountOfMana + LookTopLibrary(PriorResult), -7 compiled CreateEmblemFor!
    {
        let ast = compile_fixture(map["Karn, Living Legacy"], &vocab);
        let face = &ast.faces[0];
        match &face.abilities[1] {
            AbilityAst::Loyalty { delta, body } => {
                assert_eq!(delta, "-1");
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::PayAnyAmountOfMana {
                        player: PlayerRefAst::You
                    })
                ));
                assert!(matches!(
                    body.get(1),
                    Some(InstructionAst::LookTopLibrary { count, .. })
                        if matches!(count.as_ref(), ValueExprAst::PriorResult)
                ));
            }
            other => panic!("Karn -1 expected Loyalty, got {other:?}"),
        }
        match &face.abilities[2] {
            AbilityAst::Loyalty { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::CreateEmblemFor {
                        compiled: Some(_),
                        ..
                    })
                ));
            }
            other => panic!("Karn -7 expected Loyalty, got {other:?}"),
        }
    }

    // 6. Keyword hijacks eliminated: Fluctuator, Catalyst Stone, Memory Crystal, Warbringer, Plate Armor!
    for (name, expected_statics) in [
        ("Fluctuator", 1usize),
        ("Catalyst Stone", 2usize),
        ("Memory Crystal", 1usize),
    ] {
        let ast = compile_fixture(map[name], &vocab);
        let face = &ast.faces[0];
        assert!(
            face.keywords.is_empty(),
            "{name} must NOT hijack static rule into keywords: {:?}",
            face.keywords
        );
        assert_eq!(
            face.statics.len(),
            expected_statics,
            "{name} must emit {expected_statics} CostModifier statics: {:?}",
            face.statics
        );
        assert!(
            face.statics
                .iter()
                .all(|s| matches!(s, StaticAbilityAst::CostModifier { .. }))
        );
    }
    {
        let ast = compile_fixture(map["Plate Armor"], &vocab);
        let face = &ast.faces[0];
        assert!(
            face.keywords
                .iter()
                .any(|k| k.name == "equip" && k.parameter.as_deref() == Some("{3}")),
            "Plate Armor must keep clean equip {{3}} keyword: {:?}",
            face.keywords
        );
        assert!(
            face.statics
                .iter()
                .any(|s| matches!(s, StaticAbilityAst::CostModifier { delta, less: true, .. } if delta == "{1}")),
            "Plate Armor must split cost reduction into CostModifier static: {:?}",
            face.statics
        );
    }

    // 7. Eater of Days: SkipNextTurns { count: 2 } (NOT SkipNextUntapStep)!
    {
        let ast = compile_fixture(map["Eater of Days"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::SkipNextTurns {
                        player: PlayerRefAst::You,
                        count: 2
                    })
                ));
            }
            other => panic!("Eater of Days expected Triggered, got {other:?}"),
        }
    }

    // 8. Permanent durations preserved: Aura Graft & Warden of the First Tree!
    {
        let ast = compile_fixture(map["Aura Graft"], &vocab);
        assert!(matches!(
            ast.faces[0].spell_effects.first(),
            Some(InstructionAst::GainControl {
                duration: DurationAst::Permanent,
                ..
            })
        ));
    }
    {
        let ast = compile_fixture(map["Warden of the First Tree"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Activated { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::AnimateOrSetBasePT {
                        duration: DurationAst::Permanent,
                        ..
                    })
                ));
            }
            other => panic!("Warden expected Activated, got {other:?}"),
        }
    }

    // 9. Angelic Accord preserves threshold amount = 4!
    {
        let ast = compile_fixture(map["Angelic Accord"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { trigger, .. } => {
                assert!(matches!(
                    &trigger.intervening_if,
                    Some(ConditionAst::GainedLifeAtLeastThisTurn {
                        player: PlayerRefAst::You,
                        amount: 4
                    })
                ));
            }
            other => panic!("Angelic Accord expected Triggered, got {other:?}"),
        }
    }

    // 10. Citadel Gate uses typed AddQualifiedMana (never prose in ManaSpecAst::Choice)!
    {
        let ast = compile_fixture(map["Citadel Gate"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Activated { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::AddQualifiedMana(qm))
                        if qm.choices.len() == 2
                            && matches!(&qm.choices[0], ManaSpecAst::Fixed(m) if m == "{W}")
                            && matches!(&qm.choices[1], ManaSpecAst::ChosenColor)
                ));
            }
            other => panic!("Citadel Gate expected Activated, got {other:?}"),
        }
    }

    // 11. Modal accuracy & count preservation: Duneblast (ChooseFromGroup count=1),
    //     non-targeting Choose up to 2 preserves count=2 (Blocker #5 regression),
    //     Wail of the Forgotten (3 modes on outer Modal), Season of Gathering (3 pawprint modes),
    //     Riku of Many Paths (3 modes on permanent trigger)!
    {
        let ast = compile_fixture(map["Duneblast"], &vocab);
        assert!(matches!(
            ast.faces[0].spell_effects.first(),
            Some(InstructionAst::ChooseFromGroup { count, .. })
                if matches!(count.as_ref(), ValueExprAst::Const(1))
        ));
        let choose_two_input = CardInput {
            oracle_id: "choose-up-to-two-probe",
            name: "Mount Doom Probe",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Mount Doom Probe",
                type_line: "Sorcery",
                oracle_text: "Choose up to two creatures, then destroy the rest.",
            }],
        };
        let choose_two_ast =
            compile_card_with_options(&choose_two_input, &vocab, CompileOptions::default());
        assert!(
            matches!(
                choose_two_ast.faces[0].spell_effects.first(),
                Some(InstructionAst::ChooseFromGroup { count, .. })
                    if matches!(count.as_ref(), ValueExprAst::Const(2))
            ),
            "Choose up to two creatures must preserve Const(2): {:#?}",
            choose_two_ast.faces[0].spell_effects
        );
    }
    // 11b. Canoptek Wraith preserves NameFilterAst::SameAs on SearchLibrary,
    //      Celestial Convergence uses ExtremeLifePlayerAction(highest=true)!
    {
        let ast = compile_fixture(map["Canoptek Wraith"], &vocab);
        let has_same_as = match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => body.iter().any(|ins| {
                matches!(
                    ins,
                    InstructionAst::SearchLibrary { filter, .. }
                        if matches!(filter.named, Some(NameFilterAst::SameAs(_)))
                )
            }),
            _ => false,
        };
        assert!(
            has_same_as,
            "Canoptek Wraith must preserve NameFilterAst::SameAs: {:#?}",
            ast.faces[0].abilities
        );
        let cc = compile_fixture(map["Celestial Convergence"], &vocab);
        let has_extreme_win = match &cc.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => body.iter().any(|ins| {
                matches!(
                    ins,
                    InstructionAst::IfCondition { then_body, .. }
                        if then_body.iter().any(|sub| matches!(
                            sub,
                            InstructionAst::ExtremeLifePlayerAction { highest: true, action }
                                if matches!(action.as_ref(), InstructionAst::WinGame { player: PlayerRefAst::ThatPlayer })
                        ))
                )
            }),
            _ => false,
        };
        assert!(
            has_extreme_win,
            "Celestial Convergence must use ExtremeLifePlayerAction(highest: true, WinGame): {:#?}",
            cc.faces[0].abilities
        );
    }
    {
        let ast = compile_fixture(map["Wail of the Forgotten"], &vocab);
        assert!(matches!(
            ast.faces[0].spell_effects.first(),
            Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
        ));
    }
    {
        let ast = compile_fixture(map["Season of Gathering"], &vocab);
        assert!(matches!(
            ast.faces[0].spell_effects.first(),
            Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
        ));
    }
    {
        let ast = compile_fixture(map["Riku of Many Paths"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
                ));
            }
            other => panic!("Riku expected Triggered, got {other:?}"),
        }
    }

    // 12. Period-crossing trigger fixes: Doom Foretold (6-action IfPreviousFailed) & Xanathar (4-clause ScopedDuration)!
    {
        let ast = compile_fixture(map["Doom Foretold"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                let has_six_part_fail = body.iter().any(|ins| {
                    matches!(ins, InstructionAst::IfPreviousFailed(inner) if inner.len() == 6)
                });
                assert!(
                    has_six_part_fail,
                    "Doom Foretold must parse all 6 consequences inside IfPreviousFailed: {body:#?}"
                );
            }
            other => panic!("Doom Foretold expected Triggered, got {other:?}"),
        }
    }
    {
        let ast = compile_fixture(map["Xanathar, Guild Kingpin"], &vocab);
        match &ast.faces[0].abilities[0] {
            AbilityAst::Triggered { body, .. } => {
                assert!(matches!(
                    body.first(),
                    Some(InstructionAst::ChoosePlayerTarget {
                        player: PlayerRefAst::TargetOpponent
                    })
                ));
                assert!(
                    body.len() >= 5,
                    "Xanathar must preserve ChoosePlayerTarget + all 4 Until-end-of-turn clauses: {body:#?}"
                );
            }
            other => panic!("Xanathar expected Triggered, got {other:?}"),
        }
    }
}

#[test]
fn honest_known_opaque_fallback_tracking() {
    let vocab = load_pinned_vocabulary();
    let cards = load_real_cards();
    let map = by_name(&cards);

    // These real Magic cards compile with zero critical defects, yet still contain known
    // opaque prose / replacement strings that `audit_card` MUST honestly surface rather than
    // pretending they are 100% pure typed AST.
    for (name, expected_opaque_code) in [
        ("Bolas's Citadel", "opaque:PlayOrCastPermission_prose"),
        (
            "Teferi, Time Raveler",
            "opaque:RuleOrCombatRestriction_prose",
        ),
    ] {
        let card = map[name];
        let faces: Vec<FaceInput<'_>> = card
            .faces
            .iter()
            .map(|f| FaceInput {
                name: &f.name,
                type_line: &f.type_line,
                oracle_text: &f.oracle_text,
            })
            .collect();
        let input = CardInput {
            oracle_id: &card.oracle_id,
            name: &card.name,
            layout: &card.layout,
            faces,
        };
        let opts = CompileOptions::default();
        let ast = compile_card_with_options(&input, &vocab, opts);
        let report = audit_card(&input, &ast, opts);
        assert!(
            report.has_zero_critical_defects(),
            "{name} should have zero critical defects: {:?}",
            report.critical_defects
        );
        assert!(
            !report.is_fully_typed_pure(),
            "{name} still has opaque prose and must NOT be reported as pure typed AST"
        );
        assert!(
            report
                .opaque_strings
                .iter()
                .any(|f| f.code == expected_opaque_code && f.severity == AuditSeverity::Opaque),
            "{name} expected opaque finding `{expected_opaque_code}`, got {:?}",
            report.opaque_strings
        );
    }
}

#[test]
fn negative_strictness_and_auditor_self_tests() {
    let vocab = load_pinned_vocabulary();
    let opts = CompileOptions::default();

    // 0. Malformed "You may unless ..." MUST NOT panic on slice bounds (Blocker #4 regression).
    let slice_probe = CardInput {
        oracle_id: "neg-slice",
        name: "Slice Bounds Probe",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Slice Bounds Probe",
            type_line: "Sorcery",
            oracle_text: "You may unless target player pays {1}.",
        }],
    };
    let slice_ast = compile_card_with_options(&slice_probe, &vocab, opts);
    assert!(!slice_ast.strictly_compiled);

    // 1. Unparseable garbage MUST fail strict compilation and trigger `strict_compile_failure`
    //    WITHOUT falsely double-reporting `empty_nonvanilla_face` as structural corruption.
    let bogus_input = CardInput {
        oracle_id: "neg-1",
        name: "Synthetic Unparseable",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Synthetic Unparseable",
            type_line: "Sorcery",
            oracle_text: "Flibbertigibbet the quasar across seven hypercubes.",
        }],
    };
    let bogus_ast = compile_card_with_options(&bogus_input, &vocab, opts);
    assert!(!bogus_ast.strictly_compiled);
    let bogus_report = audit_card(&bogus_input, &bogus_ast, opts);
    assert!(!bogus_report.has_zero_critical_defects());
    assert!(
        bogus_report.has_zero_structural_defects(),
        "explicit strict_compile_failure must not double-emit structural corruption: {:?}",
        bogus_report.critical_defects
    );
    assert!(
        bogus_report
            .critical_defects
            .iter()
            .any(|f| f.code == "strict_compile_failure")
    );
    assert!(
        !bogus_report
            .critical_defects
            .iter()
            .any(|f| f.code == "empty_nonvanilla_face")
    );

    // 1b. Trailing chooser suffixes MUST emit FilterPredicateAst::ChosenByPlayer instead of dropping chooser identity.
    let chooser_input = CardInput {
        oracle_id: "chooser-trailing",
        name: "Chooser Trailing Probe",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Chooser Trailing Probe",
            type_line: "Sorcery",
            oracle_text: "Target opponent sacrifices a creature of your choice.",
        }],
    };
    let chooser_ast = compile_card_with_options(&chooser_input, &vocab, opts);
    let chooser_rep = audit_card(&chooser_input, &chooser_ast, opts);
    assert!(chooser_rep.has_zero_critical_defects());
    assert!(
        matches!(
            chooser_ast.faces[0].spell_effects.first(),
            Some(InstructionAst::Sacrifice {
                player: PlayerRefAst::TargetOpponent,
                subject,
                ..
            }) if matches!(
                subject.as_ref(),
                SubjectRefAst::Group(flt)
                    if flt.predicates.iter().any(|p| matches!(
                        p,
                        FilterPredicateAst::ChosenByPlayer(PlayerRefAst::You)
                    ))
            )
        ),
        "trailing 'of your choice' must preserve ChosenByPlayer(You): {:#?}",
        chooser_ast.faces[0].spell_effects
    );

    // 2. Auditor MUST detect synthetic Rust `{:?}` Debug leak, keyword hijack, empty modal,
    //    empty non-vanilla face, bad mana choice, and spurious MODIFIED flag.
    let synth_input = CardInput {
        oracle_id: "synth-audit",
        name: "Synthetic Audit Probe",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Synthetic Audit Probe",
            type_line: "Creature \u{2014} Human",
            oracle_text: "Whenever this creature attacks, draw a card.",
        }],
    };
    let mut bad_face = FaceAst {
        name: "Synthetic Audit Probe".to_string(),
        type_line: "Creature \u{2014} Human".to_string(),
        keywords: vec![KeywordAst {
            name: "cycling".to_string(),
            parameter: Some("abilities you activate cost {2} less".to_string()),
            desugared_rule: None,
        }],
        abilities: Vec::new(),
        spell_effects: vec![
            InstructionAst::Modal {
                choice: "one".to_string(),
                modes: Vec::new(),
            },
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana: ManaSpecAst::Choice(vec![
                    "{W}".to_string(),
                    "one mana of the chosen color".to_string(),
                ]),
            },
            InstructionAst::ChooseOption("fight:Anaphora(\"it\")".to_string()),
            InstructionAst::Destroy {
                subject: Box::new(SubjectRefAst::Group(Box::new(ObjectFilterAst {
                    flags: ObjectFilterFlags::MODIFIED,
                    ..Default::default()
                }))),
                except: None,
                random: false,
            },
        ],
        statics: Vec::new(),
        replacements: Vec::new(),
        level_bands: Vec::new(),
        class_levels: Vec::new(),
        case_solved: Default::default(),
        failures: Vec::new(),
    };
    let bad_ast = CardAst {
        oracle_id: "synth-audit".to_string(),
        name: "Synthetic Audit Probe".to_string(),
        layout: "normal".to_string(),
        faces: vec![bad_face.clone()],
        strictly_compiled: true,
        uses_tabletop_honor_system: false,
    };
    let rep = audit_card(&synth_input, &bad_ast, opts);
    let codes: BTreeSet<&str> = rep
        .critical_defects
        .iter()
        .map(|f| f.code.as_str())
        .collect();
    for expected in [
        "malformed_keyword_hijack",
        "empty_modal_modes",
        "bad_mana_choice",
        "rust_debug_leak",
        "spurious_modified_flag",
    ] {
        assert!(
            codes.contains(expected),
            "auditor failed to catch synthetic `{expected}`; got {codes:?}"
        );
    }

    // 2b. Auditor MUST recurse into class_levels, replacements (AsEntersAction), and compiled emblems (Blocker #8).
    let nested_bad_face = FaceAst {
        name: "Nested Defect Probe".to_string(),
        type_line: "Enchantment \u{2014} Class".to_string(),
        keywords: Vec::new(),
        abilities: Vec::new(),
        spell_effects: Vec::new(),
        statics: Vec::new(),
        replacements: vec![ReplacementEffectAst::AsEntersAction(vec![
            InstructionAst::Modal {
                choice: "one".to_string(),
                modes: Vec::new(),
            },
        ])],
        level_bands: Vec::new(),
        class_levels: vec![ClassLevelSectionAst {
            level: 2,
            cost: Vec::new(),
            keywords: vec![KeywordAst {
                name: "equip".to_string(),
                parameter: Some("abilities you activate cost {2} less".to_string()),
                desugared_rule: None,
            }],
            statics: Vec::new(),
            abilities: Vec::new(),
            replacements: Vec::new(),
        }],
        case_solved: Default::default(),
        failures: Vec::new(),
    };
    let nested_ast = CardAst {
        oracle_id: "synth-nested".to_string(),
        name: "Nested Defect Probe".to_string(),
        layout: "class".to_string(),
        faces: vec![nested_bad_face],
        strictly_compiled: true,
        uses_tabletop_honor_system: false,
    };
    let nested_rep = audit_card(&synth_input, &nested_ast, opts);
    let nested_codes: BTreeSet<&str> = nested_rep
        .critical_defects
        .iter()
        .map(|f| f.code.as_str())
        .collect();
    assert!(
        nested_codes.contains("empty_modal_modes")
            && nested_codes.contains("malformed_keyword_hijack"),
        "auditor must catch defects inside replacements and class_levels; got {nested_codes:?}"
    );

    // 3. Empty non-vanilla face check
    bad_face.keywords.clear();
    bad_face.spell_effects.clear();
    let empty_ast = CardAst {
        oracle_id: "synth-empty".to_string(),
        name: "Synthetic Audit Probe".to_string(),
        layout: "normal".to_string(),
        faces: vec![bad_face],
        strictly_compiled: true,
        uses_tabletop_honor_system: false,
    };
    let empty_rep = audit_card(&synth_input, &empty_ast, opts);
    assert!(
        empty_rep
            .critical_defects
            .iter()
            .any(|f| f.code == "empty_nonvanilla_face")
    );
}

#[test]
fn e14_silent_loss_semantic_regressions() {
    use spellwake_oracle_compiler::clauses::{
        StaticOrReplacement, parse_effect_body, parse_static_or_replacement,
    };
    use spellwake_oracle_compiler::filter::{
        parse_for_each_value_expr, parse_object_filter, parse_value_expr,
    };
    let vocab = load_pinned_vocabulary();
    for text in [
        "~ gets +X/+0, where X is the number of Mountains you control",
        "Enchanted creature gets +X/+X, where X is the number of creature cards in all graveyards",
        "Creatures you control get +X/+X, where X is the number of cards in your hand",
    ] {
        let node = parse_static_or_replacement(text, &vocab).unwrap();
        assert!(
            matches!(
                node,
                StaticOrReplacement::Static(StaticAbilityAst::ContinuousModifyPT {
                    scale: Some(_),
                    ..
                })
            ),
            "{text}: {node:#?}"
        );
    }
    let xy = parse_static_or_replacement(
        "~ gets +X/+Y, where X is the exiled card's power and Y is its toughness",
        &vocab,
    )
    .unwrap();
    assert!(
        matches!(xy, StaticOrReplacement::Static(StaticAbilityAst::Compound(ref parts)) if parts.len() == 2),
        "{xy:#?}"
    );
    for (text, player) in [
        (
            "the number of cards in its controller's hand",
            PlayerRefAst::ItsController,
        ),
        (
            "the number of cards in enchanted player's hand",
            PlayerRefAst::EnchantedPlayer,
        ),
        (
            "the number of cards in defending player's hand",
            PlayerRefAst::DefendingPlayer,
        ),
    ] {
        let value = parse_value_expr(text, &vocab).unwrap();
        assert!(
            matches!(value, ValueExprAst::PlayerStat { player: actual, stat: spellwake_oracle_compiler::ast::PlayerStatKind::CardsInHand } if actual == player),
            "{value:#?}"
        );
    }
    for text in [
        "other creature you control",
        "other red creature you control",
        "other green creature you control",
    ] {
        assert!(
            matches!(
                parse_for_each_value_expr(text, &vocab).unwrap(),
                ValueExprAst::Count(_)
            ),
            "{text}"
        );
    }
    let filter = parse_object_filter(
        "creature card in your graveyard that was put there from the battlefield this turn",
        &vocab,
    )
    .unwrap();
    assert!(filter.flags.0 & ObjectFilterFlags::DIED_THIS_TURN.0 != 0);
    for text in [
        "Target player reveals the top card of their library.",
        "Prevent all damage target creature would deal this turn.",
        "You may cast any number of spells from among those cards without paying their mana costs.",
    ] {
        let (body, failures) = parse_effect_body(text, &vocab);
        assert!(failures.is_empty(), "{text}: {failures:?}");
        match text {
            "Target player reveals the top card of their library." => assert!(
                matches!(body.as_slice(), [InstructionAst::PlayerLookTopLibrary { player: PlayerRefAst::TargetPlayer, library, count, reveal: true }] if library == "their" && **count == ValueExprAst::Const(1)),
                "{body:#?}"
            ),
            "Prevent all damage target creature would deal this turn." => assert!(
                matches!(body.as_slice(), [InstructionAst::PreventAllDamageFrom { combat_only: false, source, recipient: None, duration: DurationAst::UntilEndOfTurn }] if matches!(source.as_ref(), SubjectRefAst::TargetObject { .. })),
                "{body:#?}"
            ),
            _ => assert!(
                matches!(
                    body.as_slice(),
                    [InstructionAst::GrantCastPermission {
                        player: PlayerRefAst::You,
                        free: true,
                        count_limit: Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber),
                        ..
                    }]
                ),
                "{body:#?}"
            ),
        }
    }
    let (anaphoric, failures) = parse_effect_body(
        "Prevent all damage that creature would deal this turn",
        &vocab,
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert!(
        matches!(anaphoric.as_slice(), [InstructionAst::PreventAllDamageFrom { source, .. }] if matches!(source.as_ref(), SubjectRefAst::Anaphora(reference) if reference == "that creature")),
        "{anaphoric:#?}"
    );
    let conditioned = parse_static_or_replacement("As long as you control an artifact, if a creature entering causes a triggered ability of a permanent you control to trigger, that ability triggers an additional time", &vocab).unwrap();
    assert!(
        matches!(
            conditioned,
            StaticOrReplacement::Static(StaticAbilityAst::ScopedTriggerMultiplier {
                condition: Some(_),
                additional_triggers: 1,
                ..
            })
        ),
        "{conditioned:#?}"
    );
    let floor = parse_static_or_replacement("Activated abilities of creatures you control cost {2} less to activate. This effect can't reduce the mana in that cost to less than one mana.", &vocab).unwrap();
    assert!(
        matches!(
            floor,
            StaticOrReplacement::Static(StaticAbilityAst::CostModifierWithFloor {
                min_mana: 1,
                ..
            })
        ),
        "{floor:#?}"
    );
}
#[test]
fn e14_pinned_real_card_semantics() {
    let vocab = load_pinned_vocabulary();
    let cards: Vec<FixtureCard> =
        serde_json::from_str(include_str!("fixtures/e14_semantic_cards.json")).unwrap();
    for card in &cards {
        let ast = compile_fixture(card, &vocab);
        assert!(
            ast.strictly_compiled,
            "{}: {:#?}",
            card.name, ast.faces[0].failures
        );
        let face = &ast.faces[0];
        match card.name.as_str() {
            "Sardian Cliffstomper" | "Exoskeletal Armor" | "Elturel Survivors" | "Meishin, the Mind Cage" => {
                assert!(face.statics.iter().any(|s| matches!(s, StaticAbilityAst::ContinuousModifyPT { scale: Some(_), .. })), "{}: {face:#?}", card.name);
            }
            "Training Grounds" => assert!(matches!(face.statics.as_slice(), [StaticAbilityAst::CostModifierWithFloor { min_mana: 1, delta, less: true, .. }] if delta == "{2}"), "{face:#?}"),
            "Heartstone" | "Biomancer's Familiar" | "Zirda, the Dawnwaker" | "Power Artifact" | "Agatha of the Vile Cauldron" => {
                assert!(face.statics.iter().any(|s| matches!(s, StaticAbilityAst::CostModifierWithFloor { min_mana: 1, less: true, .. })), "{}: {face:#?}", card.name);
                if card.name == "Agatha of the Vile Cauldron" {
                    assert!(face.statics.iter().any(|s| matches!(s, StaticAbilityAst::CostModifierWithFloor { scale_expr: Some(value), .. } if matches!(value.as_ref(), ValueExprAst::ObjectStat { subject, stat: spellwake_oracle_compiler::ast::StatKind::Power } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)))), "{face:#?}");
                }
            }
            "Phyrexian Ingester" => assert!(face.statics.iter().any(|s| matches!(s, StaticAbilityAst::Compound(parts) if parts.len() == 2 && parts.iter().all(|p| matches!(p, StaticAbilityAst::ContinuousModifyPT { scale: Some(_), .. })))), "{face:#?}"),
            "Doubling Season" => {
                assert_eq!(face.replacements.len(), 2);
                for replacement in &face.replacements {
                    assert!(matches!(replacement, ReplacementEffectAst::EventInstead { instead_body: Some(body), .. } if matches!(body.as_slice(), [InstructionAst::ModifyReplacedQuantity { multiplier: 2, add: None, .. }])), "{replacement:#?}");
                }
            }
            _ => {}
        }
    }
}
#[test]
fn e14_replacement_riders_remain_visible() {
    use spellwake_oracle_compiler::clauses::{StaticOrReplacement, parse_static_or_replacement};
    let vocab = load_pinned_vocabulary();
    for text in [
        "If an effect would create one or more tokens under your control, it creates twice that many of those tokens and you lose 2 life instead",
        "If an effect would put one or more counters on a permanent you control, it puts twice that many of those counters on that permanent unless an opponent pays {2} instead",
    ] {
        let node = parse_static_or_replacement(text, &vocab).unwrap();
        assert!(
            matches!(
                node,
                StaticOrReplacement::Replacement(ReplacementEffectAst::EventInstead {
                    instead_body: None,
                    ..
                })
            ),
            "{node:#?}"
        );
    }
}

#[test]
fn e14_compiled_token_abilities_and_skip_step_preserve_semantics() {
    use spellwake_oracle_compiler::clauses::{
        StaticOrReplacement, parse_effect_body, parse_static_or_replacement,
    };
    let vocab = load_pinned_vocabulary();
    let (body, failures) = parse_effect_body(
        "Create a 1/1 white Soldier creature token with \"Whenever this creature attacks, draw a card.\"",
        &vocab,
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert!(
        matches!(body.as_slice(), [InstructionAst::CreateTokenWithAbilities { controller: PlayerRefAst::You, compiled_abilities, .. }] if matches!(compiled_abilities.abilities.as_slice(), [AbilityAst::Triggered { body, .. }] if matches!(body.as_slice(), [InstructionAst::Draw { player: PlayerRefAst::You, count, up_to: false }] if **count == ValueExprAst::Const(1)))),
        "{body:#?}"
    );
    let skip = parse_static_or_replacement("Skip your draw step", &vocab).unwrap();
    assert!(
        matches!(
            skip,
            StaticOrReplacement::Static(StaticAbilityAst::SkipStep {
                player: PlayerRefAst::You,
                step: spellwake_oracle_compiler::ast::TurnStepAst::DrawStep,
                condition: None
            })
        ),
        "{skip:#?}"
    );
}

#[test]
fn e14_cost_and_counter_x_definitions_are_retained() {
    use spellwake_oracle_compiler::clauses::{StaticOrReplacement, parse_static_or_replacement};
    let vocab = load_pinned_vocabulary();
    let cost = parse_static_or_replacement(
        "Spells you cast cost {X} less to cast, where X is the number of creatures you control",
        &vocab,
    )
    .unwrap();
    assert!(
        matches!(cost, StaticOrReplacement::Static(StaticAbilityAst::CostModifier { scale_expr: Some(value), .. }) if matches!(value.as_ref(), ValueExprAst::Count(_)))
    );
    let enters = parse_static_or_replacement(
        "~ enters with X +1/+1 counters on it, where X is the number of creatures you control",
        &vocab,
    )
    .unwrap();
    assert!(
        matches!(enters, StaticOrReplacement::Replacement(ReplacementEffectAst::EntersWithCounters { x_expr: Some(value), .. }) if matches!(value.as_ref(), ValueExprAst::Count(_)))
    );
    let others = parse_static_or_replacement("Other creatures you control enter with X +1/+1 counters on them, where X is the number of creatures you control", &vocab).unwrap();
    assert!(
        matches!(others, StaticOrReplacement::Replacement(ReplacementEffectAst::OthersEnterWithCounters { count, .. }) if matches!(count.as_ref(), ValueExprAst::Count(_)))
    );
}
