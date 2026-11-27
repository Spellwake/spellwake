#![allow(dead_code, unused_imports)]
//! Defined color and layer-5 behavior through the canonical evaluator/actions.
//! CR 105.1-105.4, 202.2, 204, 613.1e, 613.6 and 613.8.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    characteristics::{
        AbilityChange, Color, ColorFilter, ColorOperation, ControllerSelector, KeywordAbility,
        NumericExpression, NumericPair, PermanentSelector, ResolvedDuration, ResolvedPartSpec,
        StaticAbilityId, StaticColorOperand, StaticContinuousAbility, StaticPart, StaticParts,
    },
    effects::SeedCard,
    game::{GameCommand, GameInput, GameState},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    occurrence::RulesOccurrence,
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program},
};
use std::collections::{BTreeMap, BTreeSet};

fn set(colors: &[Color]) -> BTreeSet<Color> {
    colors.iter().copied().collect()
}
fn creature(id: u64, colors: &[Color]) -> SeedCard {
    let mut p = Program::creature(Some(ManaCost::free()), 3.into(), 3.into(), BTreeSet::new());
    if !colors.is_empty() {
        p.color_indicator = Some(set(colors));
    }
    card(id, p, true)
}
fn colors(game: &GameState, target: ObjectId) -> BTreeSet<Color> {
    current(game, target).current_colors
}
fn color_spell(
    id: u64,
    change: ColorOperation<BTreeSet<Color>>,
    duration: ResolvedDuration,
) -> SeedCard {
    spell(
        id,
        BodyTarget::SINGLE_PERMANENT,
        vec![effect(vec![ResolvedPartSpec::Color(change)], duration)],
    )
}
fn static_source(id: u64, selector: PermanentSelector, parts: Vec<StaticPart>) -> SeedCard {
    let mut p = Program::artifact(None, vec![]);
    p.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector,
        parts: StaticParts(parts),
    });
    card(id, p, true)
}
fn creatures(color: ColorFilter) -> PermanentSelector {
    PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: BTreeSet::from([CardType::Creature]),
        color,
        ..PermanentSelector::default()
    }
}
fn modifier(value: NumericExpression) -> StaticPart {
    StaticPart::ModifyPT(NumericPair {
        power: value.clone(),
        toughness: value,
    })
}

#[test]
fn set_add_remove_and_colorless_expire_to_the_surviving_older_colors() {
    let game = world(
        vocabulary(),
        vec![
            creature(1, &[Color::Green]),
            color_spell(
                2,
                ColorOperation::Set(set(&[Color::Blue])),
                ResolvedDuration::NoStatedDuration,
            ),
            color_spell(
                3,
                ColorOperation::Add(set(&[Color::Red])),
                ResolvedDuration::NoStatedDuration,
            ),
            color_spell(
                4,
                ColorOperation::Remove(set(&[Color::Blue])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            color_spell(
                5,
                ColorOperation::Set(set(&[])),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (blue, _) = resolve(&game, 2, target);
    assert_eq!(colors(blue.state(), target), set(&[Color::Blue]));
    let (both, _) = resolve(blue.state(), 3, target);
    assert_eq!(
        colors(both.state(), target),
        set(&[Color::Blue, Color::Red])
    );
    let (red, _) = resolve(both.state(), 4, target);
    assert_eq!(colors(red.state(), target), set(&[Color::Red]));
    let (colorless, _) = resolve(red.state(), 5, target);
    assert!(colors(colorless.state(), target).is_empty());
    assert_eq!(
        current(colorless.state(), target).defined.colors,
        set(&[Color::Green])
    );
    let actual = all_occurrences(&colorless)
        .into_iter()
        .find_map(|row| match row {
            RulesOccurrence::ContinuousEffectCreated { appearances, .. } => {
                appearances.iter().find(|change| change.object == target)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(actual.before.colors, set(&[Color::Red]));
    assert!(actual.after.colors.is_empty());
    let saved = colorless.state().clone();
    for _ in 0..3 {
        assert!(colors(colorless.state(), target).is_empty());
    }
    assert_eq!(colorless.state(), &saved);
    let ended = cleanup(colorless.state());
    assert_eq!(
        colors(ended.state(), target),
        set(&[Color::Blue, Color::Red])
    );
    assert_eq!(ended.state().execution().continuous_effects().count(), 2);
}

#[test]
fn all_color_filters_feed_live_selectors_and_exact_count_operands() {
    for (filter, expected) in [
        (ColorFilter::Any, [true, true, true]),
        (ColorFilter::Has(Color::Blue), [false, true, false]),
        (ColorFilter::Colorless, [false, false, true]),
        (ColorFilter::Monocolored, [true, false, false]),
        (ColorFilter::Multicolored, [false, true, false]),
    ] {
        let v = vocabulary();
        let mut count_selector = creatures(filter);
        count_selector.exclude_source = true;
        let mut counted = permanent(
            &v,
            &[CardType::Artifact, CardType::Creature],
            Some((3, 3)),
            &[],
        );
        counted.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                required_supertypes: std::collections::BTreeSet::new(),
                forbidden_supertypes: std::collections::BTreeSet::new(),
                required_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::SetPT(NumericPair {
                power: NumericExpression::count(count_selector),
                toughness: NumericExpression::Literal(1.into()),
            })]),
        });
        let mut buff_selector = creatures(filter);
        buff_selector.controller = ControllerSelector::SourceController;
        let game = world(
            v,
            vec![
                creature(1, &[Color::White]),
                creature(2, &[Color::White, Color::Blue]),
                creature(3, &[]),
                static_source(
                    4,
                    buff_selector,
                    vec![modifier(NumericExpression::Literal(1.into()))],
                ),
                controlled(5, P1, P1, counted),
            ],
            (1..=5).map(CardId).collect(),
        );
        for (id, affected) in (1..=3).zip(expected) {
            let pt = current(&game, object(&game, id))
                .power_toughness
                .unwrap()
                .derived;
            assert_eq!(pt.power, if affected { 4.into() } else { 3.into() });
        }
        let count = expected.into_iter().filter(|value| *value).count();
        assert_eq!(
            current(&game, object(&game, 5))
                .power_toughness
                .unwrap()
                .derived,
            pt(i64::try_from(count).unwrap(), 1)
        );
    }
}

#[test]
fn static_source_colors_recompute_before_an_older_dependent_component() {
    let mut source = static_source(
        1,
        creatures(ColorFilter::Any),
        vec![StaticPart::Color(ColorOperation::Set(
            StaticColorOperand::SourceCurrentColors,
        ))],
    );
    source.program.color_indicator = Some(set(&[Color::Green]));
    let game = world(
        vocabulary(),
        vec![
            source,
            creature(2, &[Color::White]),
            creature(3, &[Color::Black]),
            color_spell(
                4,
                ColorOperation::Set(set(&[Color::Blue])),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    assert_eq!(colors(&game, object(&game, 2)), set(&[Color::Green]));
    let (changed, _) = resolve(&game, 4, object(&game, 1));
    for id in 1..=3 {
        assert_eq!(
            colors(changed.state(), object(changed.state(), id)),
            set(&[Color::Blue])
        );
    }
    let ended = cleanup(changed.state());
    for id in 1..=3 {
        assert_eq!(
            colors(ended.state(), object(ended.state(), id)),
            set(&[Color::Green])
        );
    }
    assert_eq!(
        current(ended.state(), object(ended.state(), 2))
            .defined
            .colors,
        set(&[Color::White])
    );
}

#[test]
fn a_color_dependency_and_later_component_keep_the_started_affected_set() {
    let game = world(
        vocabulary(),
        vec![
            static_source(
                1,
                creatures(ColorFilter::Has(Color::Blue)),
                vec![
                    StaticPart::Color(ColorOperation::Set(StaticColorOperand::Literal(set(&[
                        Color::Green,
                    ])))),
                    modifier(NumericExpression::Literal(1.into())),
                ],
            ),
            static_source(
                2,
                creatures(ColorFilter::Any),
                vec![StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(set(&[Color::Blue])),
                ))],
            ),
            creature(3, &[Color::Red]),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let actual = current(&game, object(&game, 3));
    assert_eq!(actual.current_colors, set(&[Color::Green]));
    assert_eq!(actual.power_toughness.unwrap().derived, pt(4, 4));
}

#[test]
fn a_live_color_filter_grants_haste_to_a_new_creature_and_real_tap_costs_use_it() {
    let mut entering = creature(1, &[Color::Green]);
    entering.destination = ObjectDestination::Hand;
    entering.program.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    let game = world(
        vocabulary(),
        vec![
            entering,
            static_source(
                2,
                creatures(ColorFilter::Has(Color::Blue)),
                vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                    KeywordAbility::Haste,
                ]))],
            ),
            color_spell(
                3,
                ColorOperation::Set(set(&[Color::Blue])),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(2)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let entered = pass_pair(cast.state());
    let target = object(entered.state(), 1);
    let input = GameInput {
        basis: entered.state().basis().clone(),
        actor: P0,
        command: GameCommand::ActivateMana {
            source: target,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    };
    let saved = entered.state().clone();
    assert!(entered.state().advance(input).is_err());
    assert_eq!(entered.state(), &saved);
    let (blue, _) = resolve(entered.state(), 3, target);
    assert!(current(blue.state(), target).has_keyword(KeywordAbility::Haste));
    assert_eq!(
        blue.state()
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    let tapped = submit(
        blue.state(),
        P0,
        GameCommand::ActivateMana {
            source: target,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        tapped
            .state()
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        tapped.state().execution().mana(P0, ManaType::Green),
        1.into()
    );
    assert_eq!(colors(tapped.state(), target), set(&[Color::Blue]));
    let ended = cleanup(tapped.state());
    assert_eq!(colors(ended.state(), target), set(&[Color::Green]));
    assert!(!current(ended.state(), target).has_keyword(KeywordAbility::Haste));
}

fn green_cost() -> SeedCard {
    let mut target = creature(1, &[]);
    let spellwake_engine::program::ProgramKind::Permanent(p) = &mut target.program.kind else {
        unreachable!()
    };
    p.casting_cost = Some(ManaCost::one(ManaType::Green));
    target
}

#[test]
fn additive_color_expires_over_an_older_set_and_drives_a_blue_static() {
    let game = world(
        vocabulary(),
        vec![
            green_cost(),
            static_source(
                2,
                creatures(ColorFilter::Has(Color::Blue)),
                vec![modifier(NumericExpression::Literal(1.into()))],
            ),
            color_spell(
                3,
                ColorOperation::Set(set(&[Color::Blue])),
                ResolvedDuration::NoStatedDuration,
            ),
            color_spell(
                4,
                ColorOperation::Add(set(&[Color::Red])),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let target = object(&game, 1);
    assert_eq!(colors(&game, target), set(&[Color::Green]));
    assert_eq!(
        current(&game, target).power_toughness.unwrap().derived,
        pt(3, 3)
    );
    let (blue, _) = resolve(&game, 3, target);
    assert_eq!(colors(blue.state(), target), set(&[Color::Blue]));
    assert_eq!(
        current(blue.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    let (both, _) = resolve(blue.state(), 4, target);
    assert_eq!(
        colors(both.state(), target),
        set(&[Color::Blue, Color::Red])
    );
    assert_eq!(
        current(both.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    let ended = cleanup(both.state());
    assert_eq!(colors(ended.state(), target), set(&[Color::Blue]));
    assert_eq!(
        current(ended.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
}

#[test]
fn colorless_expiration_restores_the_mana_cost_defined_color() {
    let game = world(
        vocabulary(),
        vec![
            green_cost(),
            color_spell(
                2,
                ColorOperation::Set(set(&[])),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    assert_eq!(colors(&game, target), set(&[Color::Green]));
    let (clear, _) = resolve(&game, 2, target);
    assert!(colors(clear.state(), target).is_empty());
    assert_eq!(
        current(clear.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
    let ended = cleanup(clear.state());
    assert_eq!(colors(ended.state(), target), set(&[Color::Green]));
    assert_eq!(
        current(ended.state(), target)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
}
