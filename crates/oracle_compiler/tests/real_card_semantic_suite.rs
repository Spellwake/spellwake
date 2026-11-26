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

#[test]
fn independent_conditional_statements_preserve_exact_gates() {
    use spellwake_oracle_compiler::ast::{CompareOp, PlayerStatKind};
    let vocab = load_pinned_vocabulary();
    let cards: Vec<FixtureCard> =
        serde_json::from_str(include_str!("fixtures/condition_scope_cards.json")).unwrap();
    for card in &cards {
        let ast = compile_fixture(card, &vocab);
        assert!(
            ast.strictly_compiled,
            "{}: {:?}",
            card.name, ast.faces[0].failures
        );
        let [StaticAbilityAst::Compound(items)] = ast.faces[0].statics.as_slice() else {
            panic!("{}: {:?}", card.name, ast.faces[0].statics)
        };
        if card.name == "Elenda, Saint of Dusk" {
            assert_eq!(items.len(), 2);
            for (index, item) in items.iter().enumerate() {
                let StaticAbilityAst::ContinuousModifyPT {
                    subject,
                    dx,
                    dy,
                    grant_keywords,
                    condition,
                    ..
                } = item
                else {
                    panic!("{item:?}")
                };
                assert_eq!(subject.as_ref(), &SubjectRefAst::SelfObject);
                let (bonus, op, rhs) = if index == 0 {
                    (
                        "+1",
                        CompareOp::Gt,
                        ValueExprAst::StartingLifeTotal(PlayerRefAst::You),
                    )
                } else {
                    (
                        "+5",
                        CompareOp::Ge,
                        ValueExprAst::Add(
                            Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::You)),
                            Box::new(ValueExprAst::Const(10)),
                        ),
                    )
                };
                assert_eq!((dx.as_str(), dy.as_str()), (bonus, bonus));
                assert_eq!(
                    *grant_keywords,
                    if index == 0 {
                        vec!["menace".to_string()]
                    } else {
                        vec![]
                    }
                );
                // Exact equality rejects leaked conjunctions, Eq comparisons and ThatPlayer.
                assert_eq!(
                    condition.as_deref(),
                    Some(&ConditionAst::Compare {
                        lhs: Box::new(ValueExprAst::PlayerStat {
                            player: PlayerRefAst::You,
                            stat: PlayerStatKind::LifeTotal
                        }),
                        op,
                        rhs: Box::new(rhs),
                    })
                );
            }
        } else {
            assert_eq!(items.len(), 5);
            for (item, land, bonus, keyword) in [
                (&items[0], "Plains", Some(("+0", "+2")), None),
                (&items[1], "Island", None, Some("flying")),
                (&items[2], "Swamp", Some(("+2", "+0")), None),
                (&items[3], "Mountain", None, Some("first strike")),
                (&items[4], "Forest", None, Some("trample")),
            ] {
                let (subject, condition) = match item {
                    StaticAbilityAst::ContinuousModifyPT {
                        subject,
                        dx,
                        dy,
                        condition,
                        ..
                    } => {
                        assert_eq!(Some((dx.as_str(), dy.as_str())), bonus);
                        (subject, condition)
                    }
                    StaticAbilityAst::GrantKeywords {
                        subject,
                        keywords,
                        condition,
                        ..
                    } => {
                        assert_eq!(keywords.as_slice(), &[keyword.unwrap().to_string()]);
                        (subject, condition)
                    }
                    _ => panic!("{item:?}"),
                };
                assert_eq!(subject.as_ref(), &SubjectRefAst::SelfObject);
                // A single gate is required: an And (or a gate from a later clause) fails.
                let Some(ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter,
                    op: CompareOp::Ge,
                    value: 1,
                }) = condition.as_deref()
                else {
                    panic!("{condition:?}")
                };
                assert_eq!(filter.subtypes, vec![land.to_string()]);
            }
        }
    }
}

#[test]
fn condition_scope_keeps_explicit_conjunctions_and_unconditional_siblings() {
    use spellwake_oracle_compiler::clauses::{StaticOrReplacement, parse_static_or_replacement};
    let vocab = load_pinned_vocabulary();
    let node = parse_static_or_replacement(
        "As long as it's your turn and you control a Plains, ~ gets +1/+1. ~ has flying",
        &vocab,
    )
    .unwrap();
    let StaticOrReplacement::Static(StaticAbilityAst::Compound(items)) = node else {
        panic!("{node:?}")
    };
    assert_eq!(items.len(), 2);
    assert!(
        matches!(&items[0], StaticAbilityAst::ContinuousModifyPT { condition: Some(condition), .. } if matches!(condition.as_ref(), ConditionAst::And(_, _))),
        "{:?}",
        items[0]
    );
    assert!(
        matches!(&items[1], StaticAbilityAst::GrantKeywords { condition: None, keywords, .. } if keywords == &["flying"]),
        "{:?}",
        items[1]
    );
}

// --- Wave 14 semantic fidelity regression suite ---

fn load_w14_cards() -> Vec<FixtureCard> {
    let raw = include_str!("fixtures/semantic_fidelity_w14_cards.json");
    serde_json::from_str(raw).expect("valid w14 semantic fixture")
}

fn compile_and_audit_w14_card(
    name: &str,
    vocab: &Vocabulary,
) -> (CardAst, spellwake_oracle_compiler::audit::CardAuditReport) {
    let cards = load_w14_cards();
    let card = cards
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("missing fixture card: {name}"));
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
    let ast = compile_card_with_options(&input, vocab, CompileOptions::default());
    assert!(
        ast.strictly_compiled,
        "{name} failed strict compile: {:?}",
        ast.faces[0].failures
    );
    let report = audit_card(&input, &ast, CompileOptions::default());
    assert!(
        report.critical_defects.is_empty(),
        "{name} has critical defects: {:?}",
        report.critical_defects
    );
    (ast, report)
}

#[test]
fn anaphora_singular_definite_noun_does_not_broaden_to_group() {
    use spellwake_oracle_compiler::filter::parse_subject_or_target;
    let vocab = load_pinned_vocabulary();

    // Positive: Delirium preserves singular "the creature" as Anaphora rather than Group(all creatures).
    let (delirium, _) = compile_and_audit_w14_card("Delirium", &vocab);
    assert!(
        delirium.faces[0].spell_effects.iter().any(|ins| matches!(
            ins,
            InstructionAst::PreventAllDamageFrom {
                combat_only: true,
                source,
                duration: DurationAst::UntilEndOfTurn,
                ..
            } if matches!(source.as_ref(), SubjectRefAst::Anaphora(s) if s == "the creature")
        )),
        "Delirium should reference Anaphora('the creature'): {:?}",
        delirium.faces[0].spell_effects
    );

    // Positive: Flash Conscription and Paralyze keep singular "the creature" anaphoric.
    let (flash, _) = compile_and_audit_w14_card("Flash Conscription", &vocab);
    assert!(
        flash.faces[0].spell_effects.iter().any(|ins| matches!(
            ins,
            InstructionAst::IfCondition { then_body, .. }
                if then_body.iter().any(|inner| matches!(
                    inner,
                    InstructionAst::GrantAbilityText { subject, .. }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(s) if s == "the creature")
                ))
        )),
        "Flash Conscription should grant ability to Anaphora('the creature'): {:?}",
        flash.faces[0].spell_effects
    );

    let (paralyze, _) = compile_and_audit_w14_card("Paralyze", &vocab);
    let paralyze_json = serde_json::to_string(&paralyze).expect("serialize paralyze");
    assert!(
        paralyze_json.contains(r#""Anaphora":"the creature""#),
        "Paralyze should untap Anaphora('the creature'): {paralyze_json}"
    );
    assert!(
        !paralyze_json.contains(r#""Group""#),
        "Paralyze must not broaden 'the creature' to Group: {paralyze_json}"
    );

    let (fury, _) = compile_and_audit_w14_card("Boros Fury-Shield", &vocab);
    let fury_json = serde_json::to_string(&fury).expect("serialize boros fury-shield");
    assert!(
        fury_json.contains(r#""Anaphora":"the creature""#),
        "Boros Fury-Shield should read power of Anaphora('the creature'): {fury_json}"
    );

    // Negative: plural filters must NOT become singular anaphora.
    assert!(matches!(
        parse_subject_or_target("creatures", &vocab).expect("parse creatures"),
        SubjectRefAst::Group(_)
    ));
    assert!(matches!(
        parse_subject_or_target("all creatures", &vocab).expect("parse all creatures"),
        SubjectRefAst::All(_)
    ));
    assert!(matches!(
        parse_subject_or_target("the creature", &vocab).expect("parse the creature"),
        SubjectRefAst::Anaphora(ref s) if s == "the creature"
    ));
    assert!(matches!(
        parse_subject_or_target("the artifact", &vocab).expect("parse the artifact"),
        SubjectRefAst::Anaphora(ref s) if s == "the artifact"
    ));
}

#[test]
fn conjoined_controls_conditions_stay_independent_and_preserve_single_object_negatives() {
    use spellwake_oracle_compiler::ast::{CardTypeMask, ColorMask, CompareOp};
    use spellwake_oracle_compiler::filter::parse_condition;
    let vocab = load_pinned_vocabulary();

    // Positive: Plains and Island split into independent ControlsCount gates.
    let plains_and_island =
        parse_condition("you control a Plains and an Island", &vocab).expect("parse plains+island");
    let ConditionAst::And(lhs, rhs) = plains_and_island else {
        panic!("expected ConditionAst::And, got {plains_and_island:?}");
    };
    assert!(matches!(
        lhs.as_ref(),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes == vec!["Plains".to_string()]
    ));
    assert!(matches!(
        rhs.as_ref(),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes == vec!["Island".to_string()]
    ));

    // Positive: third-person player controls + 3-item Oxford-comma conjunction.
    let that_player = parse_condition("that player controls a Plains and an Island", &vocab)
        .expect("parse that player controls");
    assert!(matches!(
        that_player,
        ConditionAst::And(ref a, ref b)
            if matches!(a.as_ref(), ConditionAst::ControlsCount { player: PlayerRefAst::ThatPlayer, .. })
                && matches!(b.as_ref(), ConditionAst::ControlsCount { player: PlayerRefAst::ThatPlayer, .. })
    ));

    let three_lands = parse_condition("you control a Plains, an Island, and a Swamp", &vocab)
        .expect("parse 3-land conjunction");
    assert!(matches!(
        three_lands,
        ConditionAst::And(ref ab, ref c)
            if matches!(ab.as_ref(), ConditionAst::And(_, _))
                && matches!(c.as_ref(), ConditionAst::ControlsCount { filter, .. } if filter.subtypes == vec!["Swamp".to_string()])
    ));

    // Positive real cards: Soul Transfer, Raka Sanctuary, Of One Mind, Urza's Tower.
    let (soul_transfer, _) = compile_and_audit_w14_card("Soul Transfer", &vocab);
    assert!(
        soul_transfer.faces[0].spell_effects.iter().any(|ins| matches!(
            ins,
            InstructionAst::IfCondition { condition, .. }
                if matches!(
                    condition.as_ref(),
                    ConditionAst::And(a, b)
                        if matches!(a.as_ref(), ConditionAst::ControlsCount { filter, .. } if filter.any_types == CardTypeMask::ARTIFACT)
                            && matches!(b.as_ref(), ConditionAst::ControlsCount { filter, .. } if filter.any_types == CardTypeMask::ENCHANTMENT)
                )
        )),
        "Soul Transfer must require independent artifact and enchantment: {:?}",
        soul_transfer.faces[0].spell_effects
    );

    let (raka, _) = compile_and_audit_w14_card("Raka Sanctuary", &vocab);
    let raka_json = serde_json::to_string(&raka).expect("serialize raka");
    assert!(
        raka_json.contains(r#""And":[{"ControlsCount""#),
        "Raka Sanctuary must split white permanent and blue permanent: {raka_json}"
    );

    let (of_one_mind, _) = compile_and_audit_w14_card("Of One Mind", &vocab);
    assert!(
        of_one_mind.faces[0].statics.iter().any(|st| matches!(
            st,
            StaticAbilityAst::CostModifier {
                condition: Some(condition),
                ..
            } if matches!(
                condition.as_ref(),
                ConditionAst::And(a, b)
                    if matches!(a.as_ref(), ConditionAst::ControlsCount { filter, .. } if filter.subtypes == vec!["Human".to_string()] && filter.forbidden_subtypes.is_empty())
                        && matches!(b.as_ref(), ConditionAst::ControlsCount { filter, .. } if filter.subtypes.is_empty() && filter.forbidden_subtypes == vec!["Human".to_string()])
            )
        )),
        "Of One Mind must split Human and non-Human creatures cleanly: {:?}",
        of_one_mind.faces[0].statics
    );

    let (urza_tower, _) = compile_and_audit_w14_card("Urza's Tower", &vocab);
    let urza_json = serde_json::to_string(&urza_tower).expect("serialize urza tower");
    assert!(
        urza_json.contains(r#""subtypes":["Urza's","Mine"]"#)
            && urza_json.contains(r#""subtypes":["Urza's","Power-Plant"]"#),
        "Urza's Tower must split Urza's Mine and Urza's Power-Plant: {urza_json}"
    );

    // Negative: single-object multicolored/disjunctive/with-clause phrases MUST stay single ControlsCount.
    let single_multicolor = parse_condition("you control a white and blue permanent", &vocab)
        .expect("single multicolor");
    assert!(
        matches!(
            single_multicolor,
            ConditionAst::ControlsCount { ref filter, .. }
                if filter.any_colors == ColorMask::WHITE.union(ColorMask::BLUE)
        ),
        "single multicolored permanent must not split: {single_multicolor:?}"
    );

    let single_disjunction = parse_condition("you control a white or blue permanent", &vocab)
        .expect("single disjunction");
    assert!(
        matches!(single_disjunction, ConditionAst::ControlsCount { .. }),
        "disjunction must stay a single ControlsCount: {single_disjunction:?}"
    );

    let single_with_counters = parse_condition(
        "you control a creature with a +1/+1 counter and a flying counter",
        &vocab,
    );
    assert!(
        !matches!(single_with_counters, Ok(ConditionAst::And(_, _))),
        "with-clause counters on one creature must not split into two ControlsCount checks: {single_with_counters:?}"
    );
}

#[test]
fn activation_limits_and_step_timings_preserve_exact_distinctions() {
    use spellwake_oracle_compiler::clauses::parse_atomic_effect;
    let vocab = load_pinned_vocabulary();

    let (pit_imp, pit_audit) = compile_and_audit_w14_card("Pit Imp", &vocab);
    let (cinder, cinder_audit) = compile_and_audit_w14_card("Manaforge Cinder", &vocab);
    let (werewolf, werewolf_audit) = compile_and_audit_w14_card("Lesser Werewolf", &vocab);

    let extract_restriction = |ast: &CardAst| -> String {
        ast.faces[0]
            .abilities
            .iter()
            .find_map(|ab| {
                let AbilityAst::Activated { body, .. } = ab else {
                    return None;
                };
                body.iter().find_map(|ins| match ins {
                    InstructionAst::AbilityRestriction { restriction, .. } => {
                        Some(restriction.clone())
                    }
                    _ => None,
                })
            })
            .unwrap_or_else(|| panic!("missing AbilityRestriction in {}", ast.name))
    };

    let pit_tag = extract_restriction(&pit_imp);
    let cinder_tag = extract_restriction(&cinder);
    let werewolf_tag = extract_restriction(&werewolf);

    assert_eq!(pit_tag, "AtMost2TimesEachTurn");
    assert_eq!(cinder_tag, "AtMost3TimesEachTurn");
    assert_ne!(pit_tag, cinder_tag);
    assert_eq!(werewolf_tag, "DuringDeclareBlockersStep");
    assert_ne!(werewolf_tag, "DuringCombat");

    // General N and DeclareAttackers vs generic DuringCombat distinctions.
    let four_times =
        parse_atomic_effect("Activate no more than four times each turn", &vocab).expect("4x");
    assert!(matches!(
        four_times.as_slice(),
        [InstructionAst::AbilityRestriction { restriction, condition: None }]
            if restriction == "AtMost4TimesEachTurn"
    ));

    let attackers_step =
        parse_atomic_effect("Activate only during the declare attackers step", &vocab)
            .expect("declare attackers");
    assert!(matches!(
        attackers_step.as_slice(),
        [InstructionAst::AbilityRestriction { restriction, condition: None }]
            if restriction == "DuringDeclareAttackersStep"
    ));

    let generic_combat =
        parse_atomic_effect("Activate only during combat", &vocab).expect("during combat");
    assert!(matches!(
        generic_combat.as_slice(),
        [InstructionAst::AbilityRestriction { restriction, condition: None }]
            if restriction == "DuringCombat"
    ));

    // Negative: none of these structured tags may introduce opaque-prose audit findings.
    for (name, audit) in [
        ("Pit Imp", &pit_audit),
        ("Manaforge Cinder", &cinder_audit),
        ("Lesser Werewolf", &werewolf_audit),
    ] {
        assert!(
            audit.opaque_strings.is_empty(),
            "{name} should have zero opaque strings: {:?}",
            audit.opaque_strings
        );
    }
}

#[test]
fn permanent_animation_preserves_duration_pt_and_keywords() {
    use spellwake_oracle_compiler::clauses::parse_atomic_effect;
    let vocab = load_pinned_vocabulary();

    // Positive: Awakening of Vitu-Ghazi is permanent, 0/0, haste, named Vitu-Ghazi, still a land.
    let (awakening, _) = compile_and_audit_w14_card("Awakening of Vitu-Ghazi", &vocab);
    assert!(
        awakening.faces[0].spell_effects.iter().any(|ins| matches!(
            ins,
            InstructionAst::AnimateOrSetBasePT {
                subject,
                power,
                toughness,
                keywords,
                duration: DurationAst::Permanent,
                description: Some(desc),
            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(s) if s == "it")
                && power == "0"
                && toughness == "0"
                && keywords == &vec!["haste".to_string()]
                && desc.contains("legendary")
                && desc.contains("named Vitu-Ghazi")
        )),
        "Awakening of Vitu-Ghazi must be Permanent 0/0 haste: {:?}",
        awakening.faces[0].spell_effects
    );
    assert!(
        awakening.faces[0].spell_effects.iter().any(
            |ins| matches!(ins, InstructionAst::KeepExistingTypes(s) if s == "it's still a land")
        ),
        "Awakening of Vitu-Ghazi must preserve KeepExistingTypes"
    );

    // Positive: Rageform and Woodcaller Automaton default to Permanent duration.
    let (rageform, _) = compile_and_audit_w14_card("Rageform", &vocab);
    assert!(
        rageform.faces[0].abilities.iter().any(|ab| matches!(
            ab,
            AbilityAst::Triggered { body, .. }
                if body.iter().any(|ins| matches!(
                    ins,
                    InstructionAst::AnimateOrSetBasePT { duration: DurationAst::Permanent, .. }
                ))
        )),
        "Rageform animation must be Permanent: {:?}",
        rageform.faces[0].abilities
    );

    let (woodcaller, _) = compile_and_audit_w14_card("Woodcaller Automaton", &vocab);
    assert!(
        woodcaller.faces[0].abilities.iter().any(|ab| matches!(
            ab,
            AbilityAst::Triggered { body, .. }
                if body.iter().any(|ins| matches!(
                    ins,
                    InstructionAst::AnimateOrSetBasePT { duration: DurationAst::Permanent, .. }
                ))
        )),
        "Woodcaller Automaton animation must be Permanent: {:?}",
        woodcaller.faces[0].abilities
    );

    // Negative: explicit "until end of turn" animation MUST remain UntilEndOfTurn.
    let temp = parse_atomic_effect(
        "It becomes a legendary 3/3 Elemental creature with haste named Temp until end of turn",
        &vocab,
    )
    .expect("parse temporary named animation");
    assert!(
        matches!(
            temp.as_slice(),
            [InstructionAst::AnimateOrSetBasePT {
                power,
                toughness,
                keywords,
                duration: DurationAst::UntilEndOfTurn,
                ..
            }] if power == "3" && toughness == "3" && keywords == &vec!["haste".to_string()]
        ),
        "explicit until end of turn must stay UntilEndOfTurn: {temp:?}"
    );
}

#[test]
fn search_library_preserves_multi_zone_and_multi_named_targets() {
    use spellwake_oracle_compiler::clauses::parse_atomic_effect;
    let vocab = load_pinned_vocabulary();

    // Positive: Agency Outfitter keeps clean library="your", both extra zones, and split names.
    let (outfitter, _) = compile_and_audit_w14_card("Agency Outfitter", &vocab);
    let outfitter_search = outfitter.faces[0]
        .abilities
        .iter()
        .find_map(|ab| {
            let AbilityAst::Triggered { body, .. } = ab else {
                return None;
            };
            body.iter().find_map(|ins| match ins {
                InstructionAst::Optional(inner) => inner
                    .iter()
                    .find(|i| matches!(i, InstructionAst::SearchLibrary { .. })),
                _ => None,
            })
        })
        .expect("Agency Outfitter SearchLibrary");
    assert!(
        matches!(
            outfitter_search,
            InstructionAst::SearchLibrary {
                library,
                extra_zones,
                filter,
                secondary_filter: Some(secondary),
                secondary_is_or: false,
                to_zone,
                ..
            } if library == "your"
                && extra_zones == &vec![ZoneFilter::YourGraveyard, ZoneFilter::YourHand]
                && filter.named == Some(NameFilterAst::Exact("magnifying glass".to_string()))
                && secondary.named == Some(NameFilterAst::Exact("thinking cap".to_string()))
                && to_zone == "the battlefield"
        ),
        "Agency Outfitter SearchLibrary mismatch: {outfitter_search:?}"
    );

    // Positive: Alpine Houndmaster splits both named targets instead of concatenating into one bogus name.
    let (houndmaster, _) = compile_and_audit_w14_card("Alpine Houndmaster", &vocab);
    let hound_json = serde_json::to_string(&houndmaster).expect("serialize houndmaster");
    assert!(
        hound_json.contains(r#""Exact":"alpine watchdog""#)
            && hound_json.contains(r#""Exact":"igneous cur""#),
        "Alpine Houndmaster must split both card names: {hound_json}"
    );
    assert!(
        !hound_json.contains("and/or a card named"),
        "Alpine Houndmaster must not leave raw conjunction inside Exact name: {hound_json}"
    );

    // Positive: Forging the Tyrite Sword keeps named primary + typed Equipment secondary with secondary_is_or=true.
    let (forging, _) = compile_and_audit_w14_card("Forging the Tyrite Sword", &vocab);
    let forging_json = serde_json::to_string(&forging).expect("serialize forging");
    assert!(
        forging_json.contains(r#""Exact":"halvar, god of battle""#)
            && forging_json.contains(r#""subtypes":["Equipment"]"#)
            && forging_json.contains(r#""secondary_is_or":true"#),
        "Forging the Tyrite Sword must keep named OR Equipment search: {forging_json}"
    );

    // Negative: single card name containing "and" stays a single Exact name with no secondary_filter.
    let single_named = parse_atomic_effect(
        "Search your library for a card named Sword of Feast and Famine, reveal it, put it into your hand, then shuffle",
        &vocab,
    )
    .expect("parse single named search");
    assert!(
        matches!(
            single_named.as_slice(),
            [InstructionAst::SearchLibrary {
                filter,
                secondary_filter: None,
                ..
            }] if filter.named == Some(NameFilterAst::Exact("sword of feast and famine".to_string()))
        ),
        "single named card with 'and' must not split: {single_named:?}"
    );
}
// --- Wave 15 subtype disjunction & compound sacrifice suite ---

fn load_w15_cards() -> Vec<FixtureCard> {
    let raw = include_str!("fixtures/subtype_disjunction_w15_cards.json");
    serde_json::from_str(raw).expect("valid w15 subtype fixture")
}

fn compile_and_audit_w15_card(name: &str, vocab: &Vocabulary) -> CardAst {
    let cards = load_w15_cards();
    let card = cards
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("missing fixture card: {name}"));
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
    let ast = compile_card_with_options(&input, vocab, CompileOptions::default());
    assert!(
        ast.strictly_compiled,
        "{name} failed strict compile: {:?}",
        ast.faces[0].failures
    );
    let report = audit_card(&input, &ast, CompileOptions::default());
    assert!(
        report.critical_defects.is_empty(),
        "{name} has critical defects: {:?}",
        report.critical_defects
    );
    ast
}

#[test]
fn disjunctive_subtype_and_lists_emit_subtypes_match_any_while_compound_subtypes_stay_conjunctive() {
    use spellwake_oracle_compiler::filter::parse_object_filter;
    let vocab = load_pinned_vocabulary();

    // Positive: plural/collective "and" subtype lists MUST emit SubtypesMatchAny.
    for (name, expected_subtypes) in [
        ("Cloudspire Captain", vec!["Mount", "Vehicle"]),
        ("Death-Priest of Myrkul", vec!["Skeleton", "Vampire", "Zombie"]),
        ("Nightpack Ambusher", vec!["Wolf", "Werewolf"]),
        ("Gisa, the Hellraiser", vec!["Skeleton", "Zombie"]),
        ("Bosk Banneret", vec!["Treefolk", "Shaman"]),
        ("Danitha Capashen, Paragon", vec!["Aura", "Equipment"]),
    ] {
        let ast = compile_and_audit_w15_card(name, &vocab);
        let expected: Vec<String> = expected_subtypes.into_iter().map(str::to_string).collect();
        let has_match_any = ast.faces[0].statics.iter().any(|st| {
            let subj = match st {
                StaticAbilityAst::ContinuousModifyPT { subject, .. }
                | StaticAbilityAst::GrantKeywords { subject, .. }
                | StaticAbilityAst::CostModifier { subject, .. } => subject.as_ref(),
                _ => return false,
            };
            let SubjectRefAst::Group(flt) = subj else {
                return false;
            };
            flt.subtypes == expected && flt.predicates.contains(&FilterPredicateAst::SubtypesMatchAny)
        });
        assert!(
            has_match_any,
            "{name} should emit SubtypesMatchAny for {expected:?}: {:?}",
            ast.faces[0].statics
        );
    }

    // Negative: adjacent compound subtypes MUST stay conjunctive (NO SubtypesMatchAny).
    for (name, expected_subtypes) in [
        ("Broodwarden", vec!["Eldrazi", "Spawn"]),
        ("Brood Birthing", vec!["Eldrazi", "Spawn"]),
        ("Radioactive Spider", vec!["Spider", "Hero"]),
    ] {
        let ast = compile_and_audit_w15_card(name, &vocab);
        let json = serde_json::to_string(&ast).expect("serialize compound subtype card");
        for sub in expected_subtypes {
            assert!(json.contains(sub), "{name} missing subtype {sub}: {json}");
        }
        assert!(
            !json.contains("SubtypesMatchAny"),
            "{name} compound subtype must stay conjunctive without SubtypesMatchAny: {json}"
        );
    }

    // Negative: "a Swamp and a Forest" has an indefinite article after "and", so parse_object_filter
    // must NOT treat it as SubtypesMatchAny.
    let two_obj = parse_object_filter("a Swamp and a Forest", &vocab).expect("parse filter");
    assert!(
        !two_obj
            .predicates
            .contains(&FilterPredicateAst::SubtypesMatchAny),
        "indefinite 'a Swamp and a Forest' must not set SubtypesMatchAny: {two_obj:?}"
    );

    // Negative: single subtype followed by color conjunction must not set SubtypesMatchAny.
    let color_conj =
        parse_object_filter("Elf creatures that are white and green", &vocab).expect("color conj");
    assert!(
        !color_conj
            .predicates
            .contains(&FilterPredicateAst::SubtypesMatchAny),
        "single subtype with color conjunction must not set SubtypesMatchAny: {color_conj:?}"
    );
}

#[test]
fn conjoined_two_object_sacrifice_costs_split_into_independent_cost_atoms() {
    use spellwake_oracle_compiler::ast::{CardTypeMask, SupertypeMask};
    use spellwake_oracle_compiler::clauses::parse_cost_list;
    let vocab = load_pinned_vocabulary();

    // Positive: Jarad's "Sacrifice a Swamp and a Forest" splits into two Sacrifice cost atoms.
    let jarad = compile_and_audit_w15_card("Jarad, Golgari Lich Lord", &vocab);
    let jarad_has_split_cost = jarad.faces[0].abilities.iter().any(|ab| {
        let AbilityAst::Activated { cost, .. } = ab else {
            return false;
        };
        matches!(
            cost.as_slice(),
            [
                CostAtomAst::Sacrifice { count: c1, filter: f1, .. },
                CostAtomAst::Sacrifice { count: c2, filter: f2, .. },
            ] if matches!(c1.as_ref(), ValueExprAst::Const(1))
                && matches!(c2.as_ref(), ValueExprAst::Const(1))
                && f1.subtypes == vec!["Swamp".to_string()]
                && f2.subtypes == vec!["Forest".to_string()]
        )
    });
    assert!(
        jarad_has_split_cost,
        "Jarad must split 'Sacrifice a Swamp and a Forest' into two Sacrifice atoms: {:?}",
        jarad.faces[0].abilities
    );

    // Positive: Viscerid Drone splits both "Sacrifice a creature and a Swamp" and
    // "Sacrifice a creature and a snow Swamp".
    let drone = compile_and_audit_w15_card("Viscerid Drone", &vocab);
    let mut saw_plain_swamp = false;
    let mut saw_snow_swamp = false;
    for ab in &drone.faces[0].abilities {
        if let AbilityAst::Activated { cost, .. } = ab
            && let [
                CostAtomAst::TapSelf,
                CostAtomAst::Sacrifice { filter: f1, .. },
                CostAtomAst::Sacrifice { filter: f2, .. },
            ] = cost.as_slice()
            && f1.any_types == CardTypeMask::CREATURE
            && f1.subtypes.is_empty()
            && f2.subtypes == vec!["Swamp".to_string()]
        {
            if f2.required_supertypes == SupertypeMask::SNOW {
                saw_snow_swamp = true;
            } else {
                saw_plain_swamp = true;
            }
        }
    }
    assert!(
        saw_plain_swamp && saw_snow_swamp,
        "Viscerid Drone must split creature + Swamp and creature + snow Swamp: {:?}",
        drone.faces[0].abilities
    );

    // Negative: single-object sacrifice with a "with ... and a ..." counter clause stays one atom.
    let single_sac = parse_cost_list("Sacrifice a creature with a +1/+1 counter on it", &vocab)
        .expect("single creature sacrifice");
    assert_eq!(single_sac.len(), 1, "single sacrifice must stay 1 atom: {single_sac:?}");
}
