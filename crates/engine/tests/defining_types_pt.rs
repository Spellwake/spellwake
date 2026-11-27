#![allow(dead_code, unused_imports)]
//! Actual game cuts for finite defining types/PT and all-zone observations.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, ControlRecipient, ControllerSelector,
        CreatureTypeTextChangeSpec, CreatureTypeWord, DefiningCreatureTypes, DefiningPart,
        DefiningParts, NumericExpression, PermanentSelector, PowerToughnessCounters,
        PrintedPowerToughness, PrintedStat, ResolvedDuration, ResolvedPartSpec, StaticAbilityId,
        StaticContinuousAbility, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    effects::SeedCard,
    game::GameCommand,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program, ProgramKind},
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

fn printed() -> PrintedPowerToughness {
    PrintedPowerToughness {
        power: PrintedStat::Star {
            multiplier: 1.into(),
            offset: 0.into(),
        },
        toughness: PrintedStat::Star {
            multiplier: 1.into(),
            offset: 1.into(),
        },
    }
}
fn counted(selector: PermanentSelector, offset: i64) -> NumericExpression {
    NumericExpression::Count {
        selector,
        multiplier: 1.into(),
        offset: offset.into(),
    }
}
fn definition(parts: Vec<DefiningPart>) -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 0.into(), 1.into(), BTreeSet::new());
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(printed());
    p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(parts),
    });
    p
}
fn land_count() -> DefiningPart {
    let selector = PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Land]),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    DefiningPart::PowerToughness {
        power: Some(Box::new(counted(selector.clone(), 0))),
        toughness: Some(Box::new(counted(selector, 1))),
    }
}
fn removals(id: u64) -> SeedCard {
    spell(
        id,
        BodyTarget::SinglePermanent,
        vec![effect(
            vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))],
            ResolvedDuration::UntilEndOfTurn,
        )],
    )
}
fn pt_of(
    game: &spellwake_engine::game::GameState,
    id: u64,
) -> spellwake_engine::characteristics::PowerToughness {
    current(game, object(game, id))
        .power_toughness
        .unwrap()
        .derived
}

#[test]
fn pt_only_removal_preserves_printed_formula_and_cleanup_recalculates_current_value() {
    let game = world(
        vocabulary(),
        vec![
            card(1, definition(vec![land_count()]), true),
            card(
                2,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            card(
                3,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            removals(4),
        ],
        (1..=3).map(CardId).collect(),
    );
    let source = object(&game, 1);
    assert_eq!(pt_of(&game, 1), pt(2, 3));
    let (removed, _) = resolve(&game, 4, source);
    assert_eq!(pt_of(removed.state(), 1), pt(0, 1));
    assert_eq!(
        removed.state().execution().copiable_power_toughness(source),
        Some(printed())
    );
    assert_eq!(current(removed.state(), source).abilities.continuous, None);
    let changes = removed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(e) = record {
                Some(&e.occurrences)
            } else {
                None
            }
        })
        .flatten()
        .find_map(|o| {
            if let RulesOccurrence::ContinuousEffectCreated { appearances, .. } = &o.value {
                appearances.iter().find(|a| a.object == source)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(changes.before.current_power_toughness, Some(pt(2, 3)));
    assert_eq!(changes.after.current_power_toughness, Some(pt(0, 1)));
    assert_eq!(changes.before.copiable_power_toughness, Some(printed()));
    assert_eq!(changes.after.copiable_power_toughness, Some(printed()));
    let ended = cleanup(removed.state());
    assert_eq!(pt_of(ended.state(), 1), pt(2, 3));
}

#[test]
fn a_definition_started_in_type_continues_to_pt_after_actual_ability_removal() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let game = world(
        v,
        vec![
            card(
                1,
                definition(vec![
                    DefiningPart::CreatureTypes(DefiningCreatureTypes::All),
                    land_count(),
                ]),
                true,
            ),
            card(
                2,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            card(
                3,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            removals(4),
        ],
        (1..=3).map(CardId).collect(),
    );
    let source = object(&game, 1);
    let (removed, _) = resolve(&game, 4, source);
    assert_eq!(current(removed.state(), source).abilities.continuous, None);
    assert!(
        current(removed.state(), source)
            .current_creature_subtypes
            .contains(&elf)
    );
    assert_eq!(pt_of(removed.state(), 1), pt(2, 3));
}

fn word(v: &CreatureTypeVocabulary, name: &str) -> CreatureTypeWord {
    CreatureTypeWord::Literal(v.resolve_label(name).unwrap())
}

#[test]
fn text_rewrites_defining_literal_types_and_pt_count_predicates() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let vampire = v.resolve_label("Vampire").unwrap();
    let selector = PermanentSelector {
        creature_subtype: Some(elf),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    let source = definition(vec![DefiningPart::PowerToughness {
        power: Some(Box::new(counted(selector.clone(), 0))),
        toughness: Some(Box::new(counted(selector, 1))),
    }]);
    let literal = definition(vec![DefiningPart::CreatureTypes(
        DefiningCreatureTypes::Literal(BTreeSet::from([elf])),
    )]);
    let change = CreatureTypeTextChangeSpec {
        from: word(&v, "Elf"),
        to: word(&v, "Vampire"),
    };
    let vamp = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([vampire]));
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::from([elf])),
                true,
            ),
            card(3, vamp.clone(), true),
            card(4, vamp, true),
            card(5, literal, true),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Text(change)],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
            spell(
                7,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Text(change)],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        (1..=5).map(CardId).collect(),
    );
    // The literal defining peer is also an Elf, so the two cohorts both count two.
    assert_eq!(pt_of(&game, 1), pt(2, 3));
    let (literal_changed, _) = resolve(&game, 6, object(&game, 5));
    assert_eq!(
        current(literal_changed.state(), object(literal_changed.state(), 5))
            .current_creature_subtypes,
        BTreeSet::from([vampire])
    );
    assert_eq!(pt_of(literal_changed.state(), 1), pt(1, 2));
    let (count_changed, _) = resolve(
        literal_changed.state(),
        7,
        object(literal_changed.state(), 1),
    );
    assert_eq!(pt_of(count_changed.state(), 1), pt(3, 4));
    let ended = cleanup(count_changed.state());
    assert_eq!(pt_of(ended.state(), 1), pt(2, 3));
}

#[test]
fn noncreature_printed_pt_is_visible_in_hand_and_stack_but_absent_on_battlefield() {
    let mut p = Program::artifact(Some(ManaCost::free()), vec![]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(pt(4, 5).into());
    let game = world(vocabulary(), vec![card(1, p, false)], vec![]);
    assert_eq!(pt_of(&game, 1), pt(4, 5));
    assert!(
        game.execution()
            .creature(object(&game, 1))
            .unwrap()
            .is_none()
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(pt_of(cast.state(), 1), pt(4, 5));
    let entered = pass_pair(cast.state());
    let current = current(entered.state(), object(entered.state(), 1));
    assert!(current.power_toughness.is_none());
    assert!(!current.is_creature());
    assert!(
        entered
            .state()
            .execution()
            .power_toughness_counters(current.object)
            .is_some()
    );
}

#[test]
fn ordinary_loss_of_creature_type_clears_creature_subtypes_and_pt_until_cleanup() {
    let change = spell(
        4,
        BodyTarget::SinglePermanent,
        vec![effect(
            vec![ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Set(BTreeSet::from([CardType::Artifact])),
                subtypes: SubtypeOperation::Keep,
            })],
            ResolvedDuration::UntilEndOfTurn,
        )],
    );
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                definition(vec![
                    DefiningPart::CreatureTypes(DefiningCreatureTypes::All),
                    land_count(),
                ]),
                true,
            ),
            card(
                2,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            card(
                3,
                Program::land(std::collections::BTreeSet::new(), vec![]),
                true,
            ),
            change,
        ],
        (1..=3).map(CardId).collect(),
    );
    let (changed, _) = resolve(&game, 4, object(&game, 1));
    let c = current(changed.state(), object(changed.state(), 1));
    assert!(!c.is_creature());
    assert!(c.current_creature_subtypes.is_empty());
    assert!(c.power_toughness.is_none());
    assert!(
        changed
            .state()
            .execution()
            .power_toughness_counters(c.object)
            .is_some()
    );
    let ended = cleanup(changed.state());
    let c = current(ended.state(), object(ended.state(), 1));
    assert!(c.is_creature());
    assert!(!c.current_creature_subtypes.is_empty());
    assert_eq!(c.power_toughness.unwrap().derived, pt(2, 3));
}
