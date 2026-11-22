//! Catalogue-backed split spells, including the aftermath permission and departure replacement.
use super::cmm_gameplay::{creature, illegal, named};
use super::*;
use spellwake_engine::program::SplitHalf;

fn cast_half(
    state: &GameState,
    half: SplitHalf,
    target: TargetRef,
    price: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, 1).id(),
        target: Some(target),
        payment: CastingPayment {
            split_half: Some(half),
            mana: Some(spend(price)),
            ..Default::default()
        },
    }
}

fn check_half(state: &GameState, names: &[&str], colors: &[Color], mana_value: i64) {
    let object = physical(state, 1).id();
    let card = state.execution().copiable_program(object).unwrap();
    let view = state
        .execution()
        .current_characteristics(object)
        .unwrap()
        .unwrap();
    assert_eq!(
        view.current_names
            .iter()
            .map(|name| name.as_str())
            .collect::<BTreeSet<_>>(),
        names.iter().copied().collect()
    );
    assert_eq!(view.current_colors, colors.iter().copied().collect());
    assert_eq!(card.mana_value(), mana_value.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_split_claim_fame_uses_combined_characteristics_elsewhere_and_exiles_the_aftermath_on_any_stack_exit()
 {
    use ManaType::*;
    use spellwake_engine::characteristics::KeywordAbility::Haste;
    let claim = [(Black, 1)];
    let fame = [(Colourless, 1), (Red, 1)];
    let game = game(
        vec![A, B],
        vec![
            named("Claim // Fame", 1, A, ObjectDestination::Hand),
            named("Fencing Ace", 2, A, ObjectDestination::Graveyard),
            named("Phyrexian Gargantua", 3, A, ObjectDestination::Graveyard),
            named("Fencing Ace", 4, B, ObjectDestination::Graveyard),
            seed(
                5,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    Instruction::CounterSpell,
                ),
            ),
            named(
                "Elvish Mystic",
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Black, 1), (Colourless, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let obj = |state: &GameState, n| TargetRef::Object(physical(state, n).id());
    check_half(&game, &["Claim", "Fame"], &[Color::Black, Color::Red], 3);
    illegal(
        &game,
        A,
        cast_half(&game, SplitHalf::Right, obj(&game, 6), &fame),
    );
    for n in [3, 4] {
        illegal(
            &game,
            A,
            cast_half(&game, SplitHalf::Left, obj(&game, n), &claim),
        );
    }
    let open = command(
        &game,
        A,
        cast_half(&game, SplitHalf::Left, obj(&game, 2), &claim),
    )
    .into_state();
    check_half(&open, &["Claim"], &[Color::Black], 1);
    let returned = round(open).into_state();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Graveyard(A));
    check_half(
        &returned,
        &["Claim", "Fame"],
        &[Color::Black, Color::Red],
        3,
    );
    let after = command(
        &returned,
        A,
        cast_half(&returned, SplitHalf::Right, obj(&returned, 2), &fame),
    )
    .into_state();
    check_half(&after, &["Fame"], &[Color::Red], 2);
    assert_eq!(after.execution().mana(A, Red), 0.into());
    let cast = pass(&after).into_state();
    let entry = cast.execution().window().entries().last().unwrap().id();
    let countered = round(targeted(&cast, B, 5, TargetRef::Stack(entry)).into_state()).into_state();
    assert_eq!(physical(&countered, 1).zone(), ObjectZone::Exile);
    let done = round(after).into_state();
    assert_eq!(physical(&done, 1).zone(), ObjectZone::Exile);
    check_half(&done, &["Claim", "Fame"], &[Color::Black, Color::Red], 3);
    let ace = done
        .execution()
        .current_characteristics(physical(&done, 2).id())
        .unwrap()
        .unwrap();
    assert!(ace.has_keyword(Haste));
    assert_eq!(
        ace.power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 1.into()
        }
    );
    illegal(
        &done,
        A,
        cast_half(&done, SplitHalf::Right, obj(&done, 2), &fame),
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_split_wax_wane_casts_either_half_from_hand_as_an_instant_and_neither_from_the_graveyard()
{
    use ManaType::*;
    let price = [(Green, 1), (White, 1)];
    let state = game(
        vec![A, B],
        vec![
            named("Wax // Wane", 1, A, ObjectDestination::Hand),
            creature(2, A, 3, 4, &[]),
            seed(
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
        ],
        &price,
        Step::BeginningCombat,
    );
    check_half(&state, &["Wax", "Wane"], &[Color::Green, Color::White], 2);
    for (half, n, cost, name, color) in [
        (SplitHalf::Left, 2, Green, "Wax", Color::Green),
        (SplitHalf::Right, 3, White, "Wane", Color::White),
    ] {
        let target = TargetRef::Object(physical(&state, n).id());
        let invalid = TargetRef::Object(physical(&state, 5 - n).id());
        illegal(&state, A, cast_half(&state, half, invalid, &[(cost, 1)]));
        let cast = command(&state, A, cast_half(&state, half, target, &[(cost, 1)])).into_state();
        check_half(&cast, &[name], &[color], 1);
        let done = round(cast).into_state();
        check_half(&done, &["Wax", "Wane"], &[Color::Green, Color::White], 2);
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
        let size = done
            .execution()
            .power_toughness(physical(&done, 2).id())
            .unwrap()
            .unwrap()
            .derived;
        let (power, toughness, zone) = if half == SplitHalf::Left {
            (5, 6, ObjectZone::Battlefield)
        } else {
            (3, 4, ObjectZone::Graveyard(B))
        };
        assert_eq!(
            size,
            PowerToughness {
                power: power.into(),
                toughness: toughness.into()
            }
        );
        assert_eq!(physical(&done, 3).zone(), zone);
        for choice in [SplitHalf::Left, SplitHalf::Right] {
            illegal(
                &done,
                A,
                cast_half(
                    &done,
                    choice,
                    TargetRef::Object(physical(&done, 2).id()),
                    &[(cost, 1)],
                ),
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_split_spring_mind_combines_types_off_stack_and_casts_the_instant_aftermath_in_response() {
    use ManaType::*;
    use spellwake_engine::effects::LibraryTopReply;
    let state = game(
        vec![A, B],
        vec![
            named("Spring // Mind", 1, A, ObjectDestination::Hand),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Island", 3, A, ObjectDestination::LibraryBottom),
            named("Mountain", 4, A, ObjectDestination::LibraryBottom),
            creature(5, B, 1, 1, &[]),
            seed(
                6,
                B,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ],
        &[(Colourless, 6), (Green, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let types = |s: &GameState| {
        s.execution()
            .current_characteristics(physical(s, 1).id())
            .unwrap()
            .unwrap()
            .current_types
    };
    assert_eq!(types(&state), [CardType::Sorcery, CardType::Instant].into());
    check_half(&state, &["Spring", "Mind"], &[Color::Blue, Color::Green], 9);
    let spring = cast_untargeted(&state, 1, &[(Colourless, 2), (Green, 1)]).into_state();
    assert_eq!(types(&spring), [CardType::Sorcery].into());
    check_half(&spring, &["Spring"], &[Color::Green], 3);
    let choice = round(spring).into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 16])
        .unwrap()
        .into_state();
    let found = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    );
    assert_eq!(found.shuffle_journal().len(), 1);
    let grave = found.into_state();
    assert_eq!(
        (physical(&grave, 2).zone(), physical(&grave, 2).tapped()),
        (ObjectZone::Battlefield, Some(true))
    );
    assert_eq!(types(&grave), [CardType::Sorcery, CardType::Instant].into());
    let other = pass(&grave).into_state();
    let destroy = targeted(&other, B, 6, TargetRef::Object(physical(&other, 5).id())).into_state();
    let priority = pass(&destroy).into_state();
    let cast = |half| GameCommand::CastWithResources {
        spell: physical(&priority, 1).id(),
        target: None,
        payment: CastingPayment {
            split_half: Some(half),
            mana: Some(spend(&[(Colourless, 4), (Blue, 2)])),
            ..Default::default()
        },
    };
    illegal(&priority, A, cast(SplitHalf::Left));
    let mind = command(&priority, A, cast(SplitHalf::Right)).into_state();
    assert_eq!(types(&mind), [CardType::Instant].into());
    check_half(&mind, &["Mind"], &[Color::Blue], 6);
    let before = mind.execution().window().cards().library(A).to_vec();
    let drawn = round(mind).into_state();
    assert_eq!(
        drawn.execution().window().cards().library(A).len() + 2,
        before.len()
    );
    assert_eq!(physical(&drawn, 1).zone(), ObjectZone::Exile);
    assert_eq!(types(&drawn), [CardType::Sorcery, CardType::Instant].into());
    assert_eq!(physical(&drawn, 5).zone(), ObjectZone::Battlefield);
    assert_eq!(
        physical(round(drawn).state(), 5).zone(),
        ObjectZone::Graveyard(B)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_split_cascade_offers_each_legal_half_with_its_own_targets_and_excludes_aftermath_in_exile()
 {
    use ManaType::*;
    for aftermath in [false, true] {
        let split = if aftermath {
            "Claim // Fame"
        } else {
            "Wax // Wane"
        };
        let price = [(Colourless, 2), (Red, 1), (Green, 1)];
        let state = game(
            vec![A, B],
            vec![
                named(split, 1, A, ObjectDestination::LibraryTop),
                named("Bloodbraid Elf", 2, A, ObjectDestination::Hand),
                creature(3, B, 1, 1, &[]),
                named("Elvish Mystic", 4, A, ObjectDestination::Graveyard),
                seed(
                    5,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    Program::enchantment(None),
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        let cascade = round(cast_untargeted(&state, 2, &price).into_state()).into_state();
        let RuleFrame::OptionalEffect(frame) = cascade.pending_decision().unwrap().frame() else {
            panic!("expected cascade's offered split spell")
        };
        let offer = frame.exile_cast().unwrap();
        assert_eq!(offer.spell, physical(&cascade, 1).id());
        assert!(offer.normal_available);
        let obj = |n| TargetRef::Object(physical(&cascade, n).id());
        let cast = |half, target| DecisionReply::CascadeCast {
            target: Some(target),
            payment: CastingPayment {
                split_half: Some(half),
                ..Default::default()
            },
        };
        let (chosen, target) = if aftermath {
            assert!(offer.split_right.is_none());
            assert_eq!(offer.targets.as_ref().unwrap(), &[obj(4)]);
            let decision = cascade.pending_decision().unwrap();
            illegal(
                &cascade,
                A,
                GameCommand::Reply {
                    decision: decision.id(),
                    choice: cast(SplitHalf::Right, obj(3)),
                },
            );
            (SplitHalf::Left, obj(4))
        } else {
            assert_eq!(offer.targets.as_ref().unwrap(), &[obj(3)]);
            assert_eq!(
                offer
                    .split_right
                    .as_ref()
                    .unwrap()
                    .targets
                    .as_ref()
                    .unwrap(),
                &[obj(5)]
            );
            (SplitHalf::Right, obj(5))
        };
        let stacked = reply(&cascade, cast(chosen, target)).into_state();
        assert_eq!(physical(&stacked, 1).zone(), ObjectZone::Stack);
        let done = round(stacked).into_state();
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
        if aftermath {
            assert_eq!(physical(&done, 4).zone(), ObjectZone::Battlefield)
        } else {
            assert_eq!(physical(&done, 5).zone(), ObjectZone::Graveyard(B))
        }
    }
}
