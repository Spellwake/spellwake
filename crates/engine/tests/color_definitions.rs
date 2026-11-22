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
                BodyTarget::SinglePermanent,
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
fn bounce_and_reentry_reapply_definition_to_fresh_incarnations() {
    let game = world(
        vocabulary(),
        vec![
            card(1, creature(), true),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Color(ColorOperation::Set(
                        BTreeSet::from([Color::Green]),
                    ))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1)],
    );
    let old = object(&game, 1);
    let (green, _) = resolve(&game, 2, old);
    assert_eq!(
        current(green.state(), old).current_colors,
        BTreeSet::from([Color::Green])
    );
    let (hand, _) = resolve(green.state(), 3, old);
    let hand_id = object(hand.state(), 1);
    assert_ne!(hand_id, old);
    assert!(current(hand.state(), hand_id).current_colors.is_empty());
    let cast = submit(
        hand.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: hand_id,
            payment: Some(ManaPayment::default()),
        },
    );
    let returned = pass_pair(cast.state());
    let fresh = object(returned.state(), 1);
    assert_ne!(fresh, hand_id);
    assert!(current(returned.state(), fresh).current_colors.is_empty());
    assert!(returned.rules_journal().records.iter().any(|r| matches!(r,
        RulesRecord::Event(event) if event.occurrences.iter().any(|o| matches!(&o.value,
            RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::PermanentResolution
                && change.after.object.id() == fresh && change.after.colors.is_empty()
        ))
    )));
    let saved = returned.state().clone();
    for _ in 0..3 {
        assert!(current(returned.state(), fresh).current_colors.is_empty());
    }
    assert_eq!(returned.state(), &saved);
}

#[test]
fn color_definition_on_an_instant_survives_hand_stack_and_graveyard() {
    let mut p = devoid(Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::ReturnToOwnersHand],
        ),
    ));
    p.color_indicator = Some(BTreeSet::from([Color::Blue]));
    let game = world(
        vocabulary(),
        vec![
            card(1, p, false),
            card(2, Program::artifact(None, vec![]), true),
        ],
        vec![CardId(2)],
    );
    assert!(current(&game, object(&game, 1)).current_colors.is_empty());
    let (announced, _) = cast(&game, 1, object(&game, 2));
    let resolved = pass_pair(announced.state());
    assert!(
        current(announced.state(), object(announced.state(), 1))
            .current_colors
            .is_empty()
    );
    let id = object(resolved.state(), 1);
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(id)
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert!(current(resolved.state(), id).current_colors.is_empty());
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
