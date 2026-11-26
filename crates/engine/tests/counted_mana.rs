#![allow(dead_code, unused_imports)]
//! Counted mana and semantic printed text through actual mana/tap/pool owners.
//! CR 106.3-106.4, 106.12a, 605.1a, 605.3b, 612.2 and 613.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, ControlRecipient, ControllerSelector,
        CreatureTypeTextChangeSpec, CreatureTypeWord, KeywordAbility, NumericExpression,
        NumericPair, PermanentSelector, ResolvedDuration, ResolvedPartSpec, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate},
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectId},
    occurrence::RulesOccurrence,
    program::{
        BodyTarget, CardType, CreatureTypeSlot, ExecutableBody, Instruction, Program,
        TriggeredAbility, TriggeredAbilityId,
    },
};
use std::collections::BTreeMap;

fn elf_selector(v: &CreatureTypeVocabulary) -> PermanentSelector {
    PermanentSelector {
        creature_subtype: Some(v.resolve_label("Elf").unwrap()),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    }
}
fn counted(selector: PermanentSelector) -> ManaAbility {
    ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: selector,
        },
    }
}
fn archdruid(v: &CreatureTypeVocabulary) -> Program {
    let mut p = Program::creature(
        Some(ManaCost {
            generic: 1.into(),
            required: BTreeMap::from([(ManaType::Green, 2.into())]),
        }),
        2.into(),
        2.into(),
        subtypes(v, &["Elf", "Druid"]),
    );
    p.mana.push(counted(elf_selector(v)));
    let mut boost = elf_selector(v);
    boost.required_types.insert(CardType::Creature);
    boost.exclude_source = true;
    p.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: boost,
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    });
    p
}
fn simple(v: &CreatureTypeVocabulary, name: &str) -> Program {
    Program::creature(None, 2.into(), 2.into(), subtypes(v, &[name]))
}
fn activate(game: &GameState, source: ObjectId) -> NativeCandidate {
    submit(
        game,
        P0,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
}
fn produced(candidate: &NativeCandidate) -> Vec<&spellwake_engine::mana::ManaProductionRecord> {
    all_occurrences(candidate)
        .into_iter()
        .filter_map(|row| match row {
            RulesOccurrence::ManaProduced { production, .. } => Some(production),
            _ => None,
        })
        .collect()
}

#[test]
fn count_includes_the_source_but_not_other_controllers_or_nonbattlefield_cards() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, archdruid(&v), true),
            card(2, simple(&v, "Elf"), true),
            controlled(3, P1, P1, simple(&v, "Elf")),
            card(4, simple(&v, "Goblin"), true),
            card(5, simple(&v, "Elf"), false),
        ],
        (1..=4).map(CardId).collect(),
    );
    let source = object(&game, 1);
    let done = activate(&game, source);
    assert_eq!(done.state().execution().mana(P0, ManaType::Green), 2.into());
    assert_eq!(done.state().execution().mana(P1, ManaType::Green), 0.into());
    assert_eq!(produced(&done).len(), 1);
    assert_eq!(
        produced(&done)[0].produced,
        BTreeMap::from([(ManaType::Green, 2.into())])
    );
    assert_eq!(produced(&done)[0].source, source);
    assert_eq!(produced(&done)[0].controller, P0);
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert!(done.state().execution().window().entries().is_empty());
    assert!(done.state().pending_decision().is_none());
}

#[test]
fn zero_count_keeps_cost_and_activation_without_production_and_still_triggers_a_tap() {
    let v = vocabulary();
    let mut p = Program::artifact(None, vec![]);
    p.mana.push(counted(elf_selector(&v)));
    p.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::becomes_tapped(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    });
    let game = world(v, vec![card(1, p, true)], vec![CardId(1)]);
    let source = object(&game, 1);
    let done = activate(&game, source);
    assert!(produced(&done).is_empty());
    assert_eq!(done.state().execution().mana(P0, ManaType::Green), 0.into());
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert!(all_occurrences(&done).into_iter().any(|row| matches!(row, RulesOccurrence::ManaAbility { source: actual, .. } if actual.id() == source)));
    assert_eq!(done.state().execution().window().entries().len(), 1);
    let resolved = pass_pair(done.state());
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert_eq!(
        resolved.state().execution().mana(P0, ManaType::Green),
        0.into()
    );
}

#[test]
fn a_chosen_word_updates_boost_and_mana_count_without_changing_the_mana_symbol() {
    let v = vocabulary();
    let slot = CreatureTypeSlot::new(0);
    let game = world(
        v.clone(),
        vec![
            card(1, archdruid(&v), true),
            card(2, simple(&v, "Elf"), true),
            card(3, simple(&v, "Vampire"), true),
            card(4, simple(&v, "Vampire"), true),
            controlled(5, P1, P1, simple(&v, "Vampire")),
            spell(
                6,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::ChooseCreatureType {
                        into: slot,
                        excluding: subtypes(&v, &["Vampire"]),
                    },
                    effect(
                        vec![
                            ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                            ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                                from: CreatureTypeWord::Chosen(slot),
                                to: CreatureTypeWord::Literal(v.resolve_label("Vampire").unwrap()),
                            }),
                        ],
                        ResolvedDuration::NoStatedDuration,
                    ),
                ],
            ),
        ],
        (1..=5).map(CardId).collect(),
    );
    assert_eq!(
        current(&game, object(&game, 2))
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
    let source = object(&game, 1);
    let (cast, _) = cast(&game, 6, source);
    let pending = pass_pair(cast.state());
    let changed = reply(
        pending.state(),
        P0,
        DecisionReply::CreatureType(v.resolve_label("Elf").unwrap()),
    );
    assert_eq!(
        current(changed.state(), object(changed.state(), 2))
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );
    for id in [3, 4] {
        assert_eq!(
            current(changed.state(), object(changed.state(), id))
                .power_toughness
                .unwrap()
                .derived,
            pt(3, 3)
        );
    }
    assert_eq!(
        current(changed.state(), object(changed.state(), 5))
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );
    let current = current(changed.state(), source);
    assert_eq!(
        current.current_creature_subtypes,
        subtypes(&v, &["Vampire", "Druid"])
    );
    assert!(
        matches!(&current.printed_text.mana[0].output, ManaOutput::Counted { mana_type: Some(ManaType::Green), permanents } if permanents.creature_subtype == Some(v.resolve_label("Vampire").unwrap()))
    );
    let done = activate(changed.state(), source);
    assert_eq!(done.state().execution().mana(P0, ManaType::Green), 3.into());
    assert_eq!(
        produced(&done)[0].produced,
        BTreeMap::from([(ManaType::Green, 3.into())])
    );
}

#[test]
fn the_captured_ability_controller_gets_its_count_and_mana_after_a_control_change() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            controlled(1, P1, P1, archdruid(&v)),
            controlled(2, P1, P1, simple(&v, "Elf")),
            card(3, simple(&v, "Elf"), true),
            card(4, simple(&v, "Elf"), true),
            spell(
                5,
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![
                        ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                        ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                            KeywordAbility::Haste,
                        ])),
                    ],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        (1..=4).map(CardId).collect(),
    );
    let source = object(&game, 1);
    let (stolen, _) = resolve(&game, 5, source);
    let physical = stolen
        .state()
        .execution()
        .window()
        .cards()
        .get(source)
        .unwrap();
    assert_eq!(physical.owner(), P1);
    assert_eq!(physical.controller(), Some(P0));
    assert_eq!(
        physical.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let done = activate(stolen.state(), source);
    assert_eq!(done.state().execution().mana(P0, ManaType::Green), 3.into());
    assert_eq!(done.state().execution().mana(P1, ManaType::Green), 0.into());
    assert_eq!(produced(&done)[0].controller, P0);
}

#[test]
fn removed_counted_mana_rejects_before_cost_without_mutating_the_board() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, archdruid(&v), true),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    let (removed, _) = resolve(&game, 2, source);
    let saved = removed.state().clone();
    let result = removed.state().advance(GameInput {
        basis: removed.state().basis().clone(),
        actor: P0,
        command: GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    });
    assert!(result.is_err());
    assert_eq!(removed.state(), &saved);
    assert_eq!(
        removed
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_eq!(
        removed.state().execution().mana(P0, ManaType::Green),
        0.into()
    );
}
