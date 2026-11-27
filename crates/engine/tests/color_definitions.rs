#![allow(dead_code, unused_imports)]
//! Printed color CDAs through ordinary actions and exact before/after views.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, Color, ColorFilter, ColorOperation, ControllerSelector,
        DefiningPart, DefiningParts, NumericExpression, NumericPair, PermanentSelector,
        ResolvedDuration, ResolvedPartSpec, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts,
    },
    game::GameCommand,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program},
};
use std::collections::BTreeSet;
use trajectories::*;

fn devoid(mut program: Program) -> Program {
    program.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::Colors(BTreeSet::new())]),
    });
    program
}
fn creature() -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    p.color_indicator = Some(BTreeSet::from([Color::Blue, Color::Red]));
    devoid(p)
}

#[test]
fn actual_ability_removal_retains_color_definition_in_after_appearance() {
    let game = world(
        vocabulary(),
        vec![
            card(1, creature(), true),
            spell(
                2,
                BodyTarget::SINGLE_PERMANENT,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (removed, _) = resolve(&game, 2, target);
    let c = current(removed.state(), target);
    assert!(c.current_colors.is_empty());
    assert_eq!(c.abilities.continuous, None);
    assert_eq!(c.defined.colors, BTreeSet::from([Color::Blue, Color::Red]));
    let changes: Vec<_> = removed
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(&e.occurrences)
            } else {
                None
            }
        })
        .flatten()
        .filter_map(|o| {
            if let RulesOccurrence::ContinuousEffectCreated { appearances, .. } = &o.value {
                Some(appearances)
            } else {
                None
            }
        })
        .flatten()
        .filter(|c| c.object == target)
        .collect();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].before.colors.is_empty());
    assert!(changes[0].after.colors.is_empty());
    assert_eq!(
        changes[0].before.abilities.continuous,
        Some(StaticAbilityId(1))
    );
    assert_eq!(changes[0].after.abilities.continuous, None);
}

#[test]
fn other_sources_color_selectors_and_mana_counts_read_definition_result() {
    let selector = PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Creature]),
        color: ColorFilter::Colorless,
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    let mut lord = Program::artifact(None, vec![]);
    lord.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: selector.clone(),
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::count(selector.clone()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    });
    let mut mana = Program::artifact(None, vec![]);
    mana.mana = vec![ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: selector,
        },
    }];
    let mut blue = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    blue.color_indicator = Some(BTreeSet::from([Color::Blue]));
    let game = world(
        vocabulary(),
        vec![
            card(1, creature(), true),
            card(2, lord, true),
            card(3, mana, true),
            card(4, blue, true),
        ],
        (1..=4).map(CardId).collect(),
    );
    assert_eq!(
        current(&game, object(&game, 1))
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    assert_eq!(
        current(&game, object(&game, 4))
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
    let tapped = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 3),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        tapped.state().execution().mana(P0, ManaType::Green),
        1.into()
    );
}
