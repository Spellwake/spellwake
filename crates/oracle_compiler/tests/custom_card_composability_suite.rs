//! Custom-Card Compositional Stress & Normalization Suite (`v2`).
//!
//! Validates that Spellwake's strictly-typed `v2` parser compiles brand-new, never-printed
//! synthetic Magic cards deterministically via orthogonal compositional grammar rules (not
//! printed-card memorization) and fails closed honestly on unparseable prose.

use spellwake_oracle_compiler::ast::{CardTypeMask, ColorMask, CompareOp};
use spellwake_oracle_compiler::v2::{
    compile_card_v2,
    ir::{
        Ability, ChoiceCount, ChoiceKind, CompiledCardV2, CounterKind, Duration, Effect,
        KeywordKind, LifeOp, ObjectMutation, PlayerMutation, PlayerRef, ScaledPlayerAction,
        StaticAbility, SubjectRef, TriggerEvent, ValueExpr, ZoneKind, ZoneOpKind,
    },
};
use spellwake_oracle_compiler::{CardInput, FaceInput, Vocabulary};

fn custom_vocab() -> Vocabulary {
    let members: Vec<String> = [
        "Angel",
        "Assassin",
        "Beast",
        "Bird",
        "Cleric",
        "Construct",
        "Demon",
        "Dinosaur",
        "Dragon",
        "Druid",
        "Elemental",
        "Elf",
        "Faerie",
        "Fractal",
        "Fungus",
        "Giant",
        "Goblin",
        "Golem",
        "Human",
        "Hydra",
        "Illusion",
        "Insect",
        "Knight",
        "Merfolk",
        "Monk",
        "Ninja",
        "Ooze",
        "Orc",
        "Otter",
        "Pirate",
        "Rogue",
        "Scout",
        "Shaman",
        "Soldier",
        "Spirit",
        "Vampire",
        "Warrior",
        "Wizard",
        "Zombie",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    Vocabulary::from_creature_members(&members)
}

fn compile_custom(name: &str, type_line: &str, oracle_text: &str) -> CompiledCardV2 {
    let vocab = custom_vocab();
    let input = CardInput {
        oracle_id: "00000000-0000-4000-8000-000000000099",
        name,
        layout: "normal",
        faces: vec![FaceInput {
            name,
            type_line,
            oracle_text,
        }],
    };
    compile_card_v2(&input, &vocab)
}

#[test]
fn custom_triggered_creature_composes_orthogonal_filters_and_actions() {
    let card = compile_custom(
        "Astral Chronomancer",
        "Legendary Creature - Elf Wizard",
        "Flying, ward {2}\nWhenever another Elf or Wizard you control enters, scry 1, then draw a card and you gain 2 life.",
    );
    assert!(card.strictly_compiled, "errors: {:?}", card.faces[0].errors);
    let face = &card.faces[0];
    assert_eq!(face.keywords.len(), 2);
    assert_eq!(face.keywords[0].kind, KeywordKind::Flying);
    assert_eq!(face.keywords[1].kind, KeywordKind::Ward);
    assert_eq!(face.abilities.len(), 1);
    match &face.abilities[0] {
        Ability::Triggered { trigger, body } => {
            assert!(matches!(
                trigger.events.as_slice(),
                [TriggerEvent::ZoneChange {
                    to: Some(ZoneKind::Battlefield),
                    ..
                }]
            ));
            assert!(body.iter().any(|e| matches!(
                e,
                Effect::MutatePlayer {
                    mutation: PlayerMutation::ScaledAction {
                        kind: ScaledPlayerAction::Scry,
                        ..
                    },
                    ..
                }
            )));
            assert!(body.iter().any(|e| matches!(
                e,
                Effect::ZoneOp {
                    op: ZoneOpKind::Draw,
                    ..
                }
            )));
            assert!(body.iter().any(|e| matches!(
                e,
                Effect::AdjustLife {
                    player: PlayerRef::You,
                    op: LifeOp::Gain,
                    ..
                }
            )));
        }
        other => panic!("expected triggered ability, got {other:?}"),
    }
}

#[test]
fn custom_modal_instant_composes_three_distinct_modes() {
    let card = compile_custom(
        "Prismweaver's Decree",
        "Instant",
        "Choose two \u{2014}\n\u{2022} Target creature gets +3/+1 and gains first strike until end of turn.\n\u{2022} Exile target artifact or enchantment.\n\u{2022} Target opponent loses 3 life and you gain 3 life.",
    );
    assert!(card.strictly_compiled, "errors: {:?}", card.faces[0].errors);
    let face = &card.faces[0];
    assert_eq!(face.spell_effects.len(), 1);
    match &face.spell_effects[0] {
        Effect::PromptChoice {
            chooser: PlayerRef::You,
            kind: ChoiceKind::Modal { count, modes },
        } => {
            assert_eq!(*count, ChoiceCount::Exact(2));
            assert_eq!(modes.len(), 3);
        }
        other => panic!("expected modal PromptChoice, got {other:?}"),
    }
}

#[test]
fn custom_conjunctive_vs_disjunctive_type_filters_stay_distinct() {
    let conj = compile_custom(
        "Scrapyard Bolt",
        "Instant",
        "Destroy target artifact creature.",
    );
    assert!(conj.strictly_compiled, "errors: {:?}", conj.faces[0].errors);
    match &conj.faces[0].spell_effects[0] {
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            ..
        } => {
            assert_eq!(
                filter.all_types,
                CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
            );
            assert!(filter.any_types.is_empty());
        }
        other => panic!("expected conjunctive ZoneOp, got {other:?}"),
    }

    let disj = compile_custom(
        "Shatter or Slay",
        "Instant",
        "Destroy target artifact or creature.",
    );
    assert!(disj.strictly_compiled, "errors: {:?}", disj.faces[0].errors);
    match &disj.faces[0].spell_effects[0] {
        Effect::ZoneOp {
            subject: SubjectRef::TargetObject { filter, .. },
            ..
        } => {
            assert!(filter.all_types.is_empty());
            assert_eq!(
                filter.any_types,
                CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
            );
        }
        other => panic!("expected disjunctive ZoneOp, got {other:?}"),
    }
}

#[test]
fn custom_static_lord_and_activated_counter_engine_compile_cleanly() {
    let card = compile_custom(
        "Cinderforge Overseer",
        "Creature - Goblin Artificer",
        "Other Goblins you control get +1/+1 and have haste.\n{1}{R}, Sacrifice an artifact: Put two +1/+1 counters on Cinderforge Overseer.",
    );
    assert!(card.strictly_compiled, "errors: {:?}", card.faces[0].errors);
    let face = &card.faces[0];
    assert!(
        face.statics
            .iter()
            .any(|s| matches!(s, StaticAbility::ContinuousModifyPT { .. }))
    );
    assert_eq!(face.abilities.len(), 1);
    match &face.abilities[0] {
        Ability::Activated { cost, body, .. } => {
            assert_eq!(cost.len(), 2);
            assert!(matches!(
                body.as_slice(),
                [Effect::MutateObject {
                    subject: SubjectRef::SelfObject,
                    mutation: ObjectMutation::PutCounters {
                        kind: CounterKind::PlusOnePlusOne,
                        count: ValueExpr::Const(2),
                    },
                    ..
                }]
            ));
        }
        other => panic!("expected activated ability, got {other:?}"),
    }
}

#[test]
fn custom_scoped_duration_and_color_filter_compose() {
    let card = compile_custom(
        "Solar Sanctuary",
        "Instant",
        "Until end of turn, white creatures you control gain indestructible and lifelink.",
    );
    assert!(card.strictly_compiled, "errors: {:?}", card.faces[0].errors);
    let face = &card.faces[0];
    assert_eq!(face.spell_effects.len(), 1);
    match &face.spell_effects[0] {
        Effect::ScopedDuration {
            duration: Duration::UntilEndOfTurn,
            body,
        } => match &body[0] {
            Effect::MutateObject {
                subject: SubjectRef::Group(flt),
                mutation: ObjectMutation::GrantKeywords(kws),
                ..
            } => {
                assert_eq!(flt.any_colors, ColorMask::WHITE);
                assert_eq!(kws.len(), 2);
            }
            other => panic!("unexpected inner effect: {other:?}"),
        },
        other => panic!("expected ScopedDuration, got {other:?}"),
    }
}

#[test]
fn custom_unparseable_prose_and_trailing_gibberish_fail_closed() {
    let bad = compile_custom(
        "Chaos Nonsense",
        "Sorcery",
        "Draw a card, then flibbertigibbet the top library clockwise.",
    );
    assert!(
        !bad.strictly_compiled,
        "trailing unknown prose MUST fail closed"
    );
    assert!(!bad.faces[0].errors.is_empty());

    let bad_cond = compile_custom(
        "Broken Condition",
        "Instant",
        "If the moon is full in your kitchen, draw three cards.",
    );
    assert!(
        !bad_cond.strictly_compiled,
        "unrecognized condition MUST fail closed"
    );
}

#[test]
fn custom_conditional_cost_reduction_and_stat_comparison_compose() {
    let card = compile_custom(
        "Grove Sentinel",
        "Creature - Treefolk Warrior",
        "Reach\nAs long as you control three or more Forests, Grove Sentinel gets +2/+2 and has trample.",
    );
    assert!(card.strictly_compiled, "errors: {:?}", card.faces[0].errors);
    assert_eq!(card.faces[0].keywords.len(), 1);
    assert!(card.faces[0].statics.iter().any(|s| matches!(
        s,
        StaticAbility::ContinuousModifyPT {
            condition: Some(_),
            ..
        }
    )));

    let cmp = compile_custom(
        "Tidewatch Scholar",
        "Sorcery",
        "Draw two cards. If you have 10 or less life, you gain 4 life.",
    );
    assert!(cmp.strictly_compiled, "errors: {:?}", cmp.faces[0].errors);
    assert!(cmp.faces[0].spell_effects.iter().any(|e| matches!(
        e,
        Effect::BranchCondition {
            condition: spellwake_oracle_compiler::v2::ir::ConditionExpr::Compare {
                op: CompareOp::Le,
                ..
            },
            ..
        }
    )));
}
