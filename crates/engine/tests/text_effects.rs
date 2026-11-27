#![allow(dead_code, unused_imports)]
//! Creature-type word substitution through the existing layer and execution owners.
//! CR 612.1-612.3, 613.1c, 613.6, 613.7/613.8 and 113.7a.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        ControlRecipient, ControllerSelector, CreatureTypeTextChange, CreatureTypeTextChangeSpec,
        CreatureTypeWord, InstructionSubtypeOperand, NumericExpression, NumericPair,
        PermanentSelector, ResolvedDuration, ResolvedPart, ResolvedPartSpec, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts, StaticSubtypeOperand, StaticTypeChange,
        SubtypeOperation, TypeOperation,
    },
    effects::{CopyTargetChoice, SeedCard},
    game::{DecisionReply, GameCommand, GameState, RuleFrame},
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectId, ObjectZone},
    occurrence::RulesOccurrence,
    program::{
        BodyTarget, CardType, CreatureTypeSlot, ExecutableBody, Instruction, Program,
        TriggeredAbility, TriggeredAbilityId,
    },
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn kind(v: &CreatureTypeVocabulary, name: &str) -> CreatureTypeId {
    v.resolve_label(name).unwrap()
}
fn text(v: &CreatureTypeVocabulary, from: &str, to: &str) -> ResolvedPartSpec {
    ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
        from: CreatureTypeWord::Literal(kind(v, from)),
        to: CreatureTypeWord::Literal(kind(v, to)),
    })
}
fn text_spell(
    v: &CreatureTypeVocabulary,
    id: u64,
    from: &str,
    to: &str,
    duration: ResolvedDuration,
) -> SeedCard {
    spell(
        id,
        BodyTarget::SinglePermanent,
        vec![effect(vec![text(v, from, to)], duration)],
    )
}
fn selector(v: &CreatureTypeVocabulary, name: &str) -> PermanentSelector {
    PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Creature]),
        creature_subtype: Some(kind(v, name)),
        controller: ControllerSelector::SourceController,
        exclude_source: true,
        ..PermanentSelector::default()
    }
}
fn lord(v: &CreatureTypeVocabulary) -> Program {
    let mut p = Program::creature(
        Some(ManaCost::free()),
        3.into(),
        3.into(),
        subtypes(v, &["Elf"]),
    );
    p.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: selector(v, "Elf"),
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    });
    p
}
fn peers(v: &CreatureTypeVocabulary) -> Vec<SeedCard> {
    let mut cards = vec![card(1, lord(v), true)];
    for (id, name) in [(2, "Elf"), (3, "Vampire"), (4, "Goblin")] {
        cards.push(card(
            id,
            Program::creature(None, 3.into(), 3.into(), subtypes(v, &[name])),
            true,
        ));
    }
    cards
}
fn powers(game: &GameState) -> Vec<spellwake_contracts::RulesInteger> {
    [2, 3, 4]
        .into_iter()
        .map(|id| {
            current(game, object(game, id))
                .power_toughness
                .unwrap()
                .derived
                .power
        })
        .collect()
}

#[test]
fn type_line_and_printed_selector_change_together_then_restore_on_expiry() {
    let v = vocabulary();
    let mut cards = peers(&v);
    cards.push(text_spell(
        &v,
        5,
        "Elf",
        "Vampire",
        ResolvedDuration::UntilEndOfTurn,
    ));
    let game = world(v.clone(), cards, (1..=4).map(CardId).collect());
    assert_eq!(powers(&game), vec![4.into(), 3.into(), 3.into()]);
    let source = object(&game, 1);
    let original = game
        .execution()
        .programs()
        .map(|(id, p)| (id, p.clone()))
        .collect::<Vec<_>>();
    let (changed, _) = resolve(&game, 5, source);
    assert_eq!(powers(changed.state()), vec![3.into(), 4.into(), 3.into()]);
    let current = current(changed.state(), source);
    assert_eq!(
        current.current_creature_subtypes,
        subtypes(&v, &["Vampire"])
    );
    assert_eq!(current.defined.creature_subtypes, subtypes(&v, &["Elf"]));
    let Some(StaticContinuousAbility::Battlefield { selector, .. }) =
        current.printed_text.continuous
    else {
        panic!("ordinary static fixture");
    };
    assert_eq!(selector.creature_subtype, Some(kind(&v, "Vampire")));
    assert_eq!(
        changed
            .state()
            .execution()
            .programs()
            .map(|(id, p)| (id, p.clone()))
            .collect::<Vec<_>>(),
        original
    );
    let saved = changed.state().clone();
    for _ in 0..3 {
        assert_eq!(powers(changed.state()), vec![3.into(), 4.into(), 3.into()]);
    }
    assert_eq!(changed.state(), &saved);
    let ended = cleanup(changed.state());
    assert_eq!(powers(ended.state()), vec![4.into(), 3.into(), 3.into()]);
    assert_eq!(
        current_subtypes(ended.state(), source),
        subtypes(&v, &["Elf"])
    );
}
fn current_subtypes(game: &GameState, object: ObjectId) -> BTreeSet<CreatureTypeId> {
    current(game, object).current_creature_subtypes
}

#[test]
fn timestamp_chains_recompute_from_printed_text_when_the_newer_effect_expires() {
    let v = vocabulary();
    let mut cards = peers(&v);
    cards.push(text_spell(
        &v,
        5,
        "Elf",
        "Vampire",
        ResolvedDuration::NoStatedDuration,
    ));
    cards.push(text_spell(
        &v,
        6,
        "Vampire",
        "Goblin",
        ResolvedDuration::UntilEndOfTurn,
    ));
    let game = world(v.clone(), cards, (1..=4).map(CardId).collect());
    let source = object(&game, 1);
    let (first, _) = resolve(&game, 5, source);
    assert_eq!(powers(first.state()), vec![3.into(), 4.into(), 3.into()]);
    let (second, _) = resolve(first.state(), 6, source);
    assert_eq!(powers(second.state()), vec![3.into(), 3.into(), 4.into()]);
    assert_eq!(
        current_subtypes(second.state(), source),
        subtypes(&v, &["Goblin"])
    );
    let ended = cleanup(second.state());
    assert_eq!(powers(ended.state()), vec![3.into(), 4.into(), 3.into()]);
    assert_eq!(
        current_subtypes(ended.state(), source),
        subtypes(&v, &["Vampire"])
    );
}

#[test]
fn reverse_timestamp_order_does_not_collapse_substitutions_into_a_transitive_map() {
    let v = vocabulary();
    let mut cards = peers(&v);
    cards.push(text_spell(
        &v,
        5,
        "Vampire",
        "Goblin",
        ResolvedDuration::NoStatedDuration,
    ));
    cards.push(text_spell(
        &v,
        6,
        "Elf",
        "Vampire",
        ResolvedDuration::NoStatedDuration,
    ));
    let game = world(v.clone(), cards, (1..=4).map(CardId).collect());
    let source = object(&game, 1);
    let (first, _) = resolve(&game, 5, source);
    let (second, _) = resolve(first.state(), 6, source);
    assert_eq!(powers(second.state()), vec![3.into(), 4.into(), 3.into()]);
    assert_eq!(
        current_subtypes(second.state(), source),
        subtypes(&v, &["Vampire"])
    );
}

#[test]
fn chosen_text_and_control_are_one_owned_effect_with_separate_layer_components() {
    let v = vocabulary();
    let slot = CreatureTypeSlot::new(7);
    let mut target = card(1, lord(&v), true);
    target.destination =
        spellwake_engine::object_identity::ObjectDestination::Battlefield { controller: P1 };
    let game = world(
        v.clone(),
        vec![
            target,
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                            slot,
                            subtypes(&v, &["Vampire"]),
                        ),
                    )),
                    effect(
                        vec![
                            ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                            ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                                from: CreatureTypeWord::Chosen(slot),
                                to: CreatureTypeWord::Literal(kind(&v, "Vampire")),
                            }),
                        ],
                        ResolvedDuration::NoStatedDuration,
                    ),
                ],
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (cast, _) = cast(&game, 2, target);
    let pending = pass_pair(cast.state());
    let done = reply(
        pending.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
            kind(&v, "Elf"),
        )),
    );
    assert_eq!(current(done.state(), target).controller, Some(P0));
    assert_eq!(
        current_subtypes(done.state(), target),
        subtypes(&v, &["Vampire"])
    );
    let effects = done
        .state()
        .execution()
        .continuous_effects()
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 1);
    assert!(
        effects[0]
            .parts
            .0
            .contains(&ResolvedPart::Text(CreatureTypeTextChange {
                from: kind(&v, "Elf"),
                to: kind(&v, "Vampire")
            }))
    );
}

#[test]
fn text_changes_do_not_rewrite_the_literal_operands_of_already_created_effects() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Elf"])),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![
                        text(&v, "Elf", "Vampire"),
                        type_part(
                            TypeOperation::Keep,
                            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes(
                                &v,
                                &["Elf"],
                            ))),
                        ),
                    ],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (done, _) = resolve(&game, 2, target);
    assert_eq!(
        current_subtypes(done.state(), target),
        subtypes(&v, &["Elf", "Vampire"])
    );
}

fn type_ability(v: &CreatureTypeVocabulary, source_cost: SourceCost) -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Keep,
                        SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes(
                            v,
                            &["Elf"],
                        ))),
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        }],
    )
}
fn activate(
    game: &GameState,
    source: ObjectId,
    target: ObjectId,
) -> spellwake_engine::game::NativeCandidate {
    submit(
        game,
        P0,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(ManaPayment::default()),
        },
    )
}

#[test]
fn activation_captures_rewritten_printed_text_before_sacrificing_its_source() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, type_ability(&v, SourceCost::Sacrifice), true),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Goblin"])),
                true,
            ),
            text_spell(&v, 3, "Elf", "Vampire", ResolvedDuration::NoStatedDuration),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    let (changed, _) = resolve(&game, 3, source);
    let activated = activate(changed.state(), source, target);
    assert_eq!(
        activated
            .state()
            .execution()
            .window()
            .cards()
            .get(object(activated.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    let done = pass_pair(activated.state());
    assert_eq!(
        current_subtypes(done.state(), target),
        subtypes(&v, &["Goblin", "Vampire"])
    );
}

#[test]
fn a_captured_ability_is_not_rewritten_when_its_source_changes_text_later() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, type_ability(&v, SourceCost::Tap), true),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Goblin"])),
                true,
            ),
            text_spell(&v, 3, "Elf", "Vampire", ResolvedDuration::NoStatedDuration),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    let activated = activate(&game, source, target);
    let (changed, _) = resolve(activated.state(), 3, source);
    let done = pass_pair(changed.state());
    assert_eq!(
        current_subtypes(done.state(), target),
        subtypes(&v, &["Elf", "Goblin"])
    );
}

#[test]
fn rewritten_static_type_and_count_operands_feed_the_existing_later_layers() {
    let v = vocabulary();
    let mut source = permanent(
        &v,
        &[CardType::Artifact, CardType::Creature],
        Some((3, 3)),
        &["Elf"],
    );
    let mut counted = selector(&v, "Elf");
    counted.exclude_source = false;
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Artifact]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![
            StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(subtypes(
                    &v,
                    &["Elf"],
                ))),
            }),
            StaticPart::SetPT(NumericPair {
                power: NumericExpression::count(counted.clone()),
                toughness: NumericExpression::count(counted),
            }),
        ]),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Elf"])),
                true,
            ),
            card(
                3,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Vampire"])),
                true,
            ),
            card(
                4,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Vampire"])),
                true,
            ),
            text_spell(&v, 5, "Elf", "Vampire", ResolvedDuration::NoStatedDuration),
        ],
        (1..=4).map(CardId).collect(),
    );
    let source = object(&game, 1);
    assert_eq!(
        current(&game, source).power_toughness.unwrap().derived,
        pt(2, 2)
    );
    let (changed, _) = resolve(&game, 5, source);
    assert_eq!(
        current(changed.state(), source)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
    assert_eq!(
        current_subtypes(changed.state(), source),
        subtypes(&v, &["Vampire"])
    );
}

#[test]
fn same_layer_dependencies_use_rewritten_static_selectors_and_substitution_operands() {
    let v = vocabulary();
    let mut rewritten = Program::artifact(None, vec![]);
    rewritten.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: selector(&v, "Elf"),
        parts: StaticParts(vec![StaticPart::Text(CreatureTypeTextChange {
            from: kind(&v, "Elf"),
            to: kind(&v, "Goblin"),
        })]),
    });
    let mut rewriter = Program::artifact(None, vec![]);
    rewriter.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Artifact]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Text(CreatureTypeTextChange {
            from: kind(&v, "Elf"),
            to: kind(&v, "Vampire"),
        })]),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, rewritten, true),
            card(2, rewriter, true),
            card(
                3,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Elf"])),
                true,
            ),
            card(
                4,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Vampire"])),
                true,
            ),
        ],
        (1..=4).map(CardId).collect(),
    );
    assert_eq!(
        current_subtypes(&game, object(&game, 3)),
        subtypes(&v, &["Elf"])
    );
    assert_eq!(
        current_subtypes(&game, object(&game, 4)),
        subtypes(&v, &["Goblin"])
    );
    let StaticContinuousAbility::Battlefield {
        selector, parts, ..
    } = current(&game, object(&game, 1))
        .printed_text
        .continuous
        .unwrap()
    else {
        panic!("ordinary static fixture");
    };
    assert_eq!(selector.creature_subtype, Some(kind(&v, "Vampire")));
    assert_eq!(
        parts.0,
        vec![StaticPart::Text(CreatureTypeTextChange {
            from: kind(&v, "Vampire"),
            to: kind(&v, "Goblin")
        })]
    );
}

#[test]
fn text_changes_update_later_operands_without_reselecting_an_earlier_started_effect() {
    let v = vocabulary();
    let mut p = Program::artifact(None, vec![]);
    let mut counted = selector(&v, "Elf");
    counted.exclude_source = false;
    p.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: counted.clone(),
        parts: StaticParts(vec![
            StaticPart::ControlSourceController,
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::count(counted.clone()),
                toughness: NumericExpression::count(counted),
            }),
        ]),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, p, true),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Elf"])),
                true,
            ),
            card(
                3,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Vampire"])),
                true,
            ),
            card(
                4,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Vampire"])),
                true,
            ),
            text_spell(&v, 5, "Elf", "Vampire", ResolvedDuration::NoStatedDuration),
        ],
        (1..=4).map(CardId).collect(),
    );
    assert_eq!(powers(&game), vec![4.into(), 3.into(), 3.into()]);
    let (changed, _) = resolve(&game, 5, object(&game, 1));
    assert_eq!(powers(changed.state()), vec![5.into(), 3.into(), 3.into()]);
}

#[test]
fn copied_text_spells_choose_independent_words_and_retarget_independent_objects() {
    let v = vocabulary();
    let slot = CreatureTypeSlot::new(0);
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Elf"])),
                true,
            ),
            card(
                2,
                Program::creature(None, 3.into(), 3.into(), subtypes(&v, &["Goblin"])),
                true,
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                            slot,
                            subtypes(&v, &["Vampire"]),
                        ),
                    )),
                    effect(
                        vec![ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                            from: CreatureTypeWord::Chosen(slot),
                            to: CreatureTypeWord::Literal(kind(&v, "Vampire")),
                        })],
                        ResolvedDuration::NoStatedDuration,
                    ),
                ],
            ),
            spell(
                4,
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let elf = object(&game, 1);
    let goblin = object(&game, 2);
    let (original, entry) = cast(&game, 3, elf);
    let copying = submit(
        original.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(original.state(), 4),
            target: TargetRef::Stack(entry),
            payment: ManaPayment::default(),
        },
    );
    let target_choice = pass_pair(copying.state());
    let copied = reply(
        target_choice.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(goblin))),
    );
    let copy_choice = pass_pair(copied.state());
    let copy_done = reply(
        copy_choice.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
            kind(&v, "Goblin"),
        )),
    );
    assert_eq!(
        current_subtypes(copy_done.state(), goblin),
        subtypes(&v, &["Vampire"])
    );
    assert_eq!(
        current_subtypes(copy_done.state(), elf),
        subtypes(&v, &["Elf"])
    );
    let original_choice = pass_pair(copy_done.state());
    let done = reply(
        original_choice.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
            kind(&v, "Elf"),
        )),
    );
    assert_eq!(
        current_subtypes(done.state(), elf),
        subtypes(&v, &["Vampire"])
    );
    assert_eq!(done.state().execution().continuous_effects().count(), 2);
}
