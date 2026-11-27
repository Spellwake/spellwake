//! Independent branches exercise every loyalty cost from the same battlefield.
use super::*;
mod additional_walkers;
mod last_walkers;
mod library_walkers;

fn with_loyalty_for_ultimate(
    name: &str,
    cost: &[(ManaType, i64)],
    starting_loyalty: i64,
    ultimate_cost: i64,
    mut cards: Vec<SeedCard>,
) -> GameState {
    cards.push(named(name, 1, A, ObjectDestination::Hand));
    let additional = usize::try_from(ultimate_cost - starting_loyalty).unwrap();
    cards.push(seed(
        90,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_PERMANENT,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        spellwake_engine::effects::CounterKind::Loyalty,
                        spellwake_engine::program::ValueExpr::literal(1)
                    );
                    additional
                ],
            ),
        ),
    ));
    let initial = game(vec![A, B, C], cards, cost, Step::PrecombatMain);
    let cast = round(cast_untargeted(&initial, 1, cost).into_state()).into_state();
    assert_eq!(loyalty(&cast, 1), starting_loyalty);
    let ready =
        round(targeted(&cast, A, 90, TargetRef::Object(physical(&cast, 1).id())).into_state())
            .into_state();
    assert_eq!(loyalty(&ready, 1), ultimate_cost);
    ready
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jace_beleren_draws_for_each_player_or_one_target_and_mills_exactly_twenty() {
    use ManaType::*;
    let cards = (10..32)
        .map(|id| {
            seed(
                id,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        })
        .collect();
    let ready =
        with_loyalty_for_ultimate("Jace Beleren", &[(Colourless, 1), (Blue, 2)], 3, 10, cards);
    let all = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    for (seat, id) in [(A, 100), (B, 10), (C, 102)] {
        assert_eq!(physical(&all, id).zone(), ObjectZone::Hand(seat));
    }
    for who in [A, B, C] {
        let one =
            round(command(&ready, A, loyalty_at(&ready, 1, TargetRef::Player(who))).into_state())
                .into_state();
        for (seat, id) in [(A, 100), (B, 10), (C, 102)] {
            assert_eq!(
                physical(&one, id).zone() == ObjectZone::Hand(seat),
                seat == who
            );
        }
    }
    let stack = command(&ready, A, loyalty_at(&ready, 2, TargetRef::Player(B))).into_state();
    assert_eq!(physical(&stack, 1).zone(), ObjectZone::Graveyard(A));
    let order = round(stack).into_state();
    let RuleFrame::ResolutionGraveyardOrder(frame) = order.pending_decision().unwrap().frame()
    else {
        panic!("milled graveyard order");
    };
    assert_eq!(frame.chooser(), B);
    assert_eq!(frame.domain().len(), 20);
    let done = reply(
        &order,
        DecisionReply::OrderResolutionGraveyard(frame.domain().iter().rev().copied().collect()),
    )
    .into_state();
    for n in 10..30 {
        assert_eq!(physical(&done, n).zone(), ObjectZone::Graveyard(B));
    }
    assert_eq!(done.execution().window().cards().library(B).len(), 3);
    assert_eq!(
        done.execution().window().cards().library(B)[0],
        physical(&done, 30).id()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_elspeth_suns_champion_makes_soldiers_destroys_by_power_and_grants_a_persistent_emblem() {
    use ManaType::*;
    let ready = with_loyalty_for_ultimate(
        "Elspeth, Sun's Champion",
        &[(Colourless, 4), (White, 2)],
        4,
        7,
        vec![
            creature(2, A, 4, 4, &[]),
            creature(3, B, 6, 2, &[]),
            creature(4, C, 3, 7, &[]),
            creature(5, A, 1, 1, &[]),
        ],
    );
    let order = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    let RuleFrame::TimestampOrder(frame) = order.pending_decision().unwrap().frame() else {
        panic!("Soldier order");
    };
    let soldiers = reply(
        &order,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    for id in tokens(&soldiers, A, 3) {
        let card = soldiers
            .state()
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert_eq!(card.current_colors, [Color::White].into());
        assert_eq!(
            card.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Soldier").unwrap()].into()
        );
        assert_eq!(
            card.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
    let wiped = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    for (id, owner) in [(2, A), (3, B)] {
        assert_eq!(physical(&wiped, id).zone(), ObjectZone::Graveyard(owner));
    }
    for id in [4, 5] {
        assert_eq!(physical(&wiped, id).zone(), ObjectZone::Battlefield);
    }
    let emblem = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(physical(&emblem, 1).zone(), ObjectZone::Graveyard(A));
    for (id, power, toughness, owns) in [
        (2, 6, 6, true),
        (3, 6, 2, false),
        (4, 3, 7, false),
        (5, 3, 3, true),
    ] {
        let card = emblem
            .execution()
            .current_characteristics(physical(&emblem, id).id())
            .unwrap()
            .unwrap();
        assert_eq!(card.has_keyword(KeywordAbility::Flying), owns);
        assert_eq!(
            card.power_toughness.unwrap().derived,
            PowerToughness {
                power: power.into(),
                toughness: toughness.into()
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_freyalise_druid_produces_green_destroys_either_type_and_draws_for_own_green_creatures() {
    use ManaType::*;
    verify_commander_deck("Freyalise, Llanowar's Fury", "Forest");
    let mut cards = vec![
        named("Elvish Mystic", 2, A, battlefield(A)),
        named("Elvish Mystic", 3, A, battlefield(A)),
        named("Elvish Mystic", 4, B, battlefield(B)),
        creature(5, A, 1, 1, &[]),
        seed(6, B, battlefield(B), Program::artifact(None, vec![])),
        seed(7, B, battlefield(B), Program::enchantment(None)),
    ];
    for id in 10..14 {
        cards.push(seed(
            id,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let ready = with_loyalty_for_ultimate(
        "Freyalise, Llanowar's Fury",
        &[(Colourless, 3), (Green, 2)],
        3,
        6,
        cards,
    );
    let made = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state());
    let id = tokens(&made, A, 1)[0];
    let card = made
        .state()
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap();
    assert_eq!(card.current_colors, [Color::Green].into());
    assert_eq!(
        card.current_creature_subtypes,
        ["Elf", "Druid"]
            .map(|kind| current_catalogue().1.resolve_label(kind).unwrap())
            .into()
    );
    assert_eq!(
        card.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let tap = || GameCommand::ActivateMana {
        source: id,
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
    };
    illegal(made.state(), A, tap());
    let next = exile_hand::until(made.into_state(), |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let green = command(&next, A, tap()).into_state();
    assert_eq!(green.execution().mana(A, Green), 1.into());
    assert_eq!(
        green.execution().window().cards().get(id).unwrap().tapped(),
        Some(true)
    );
    for n in [6, 7] {
        let gone = round(
            command(
                &ready,
                A,
                loyalty_at(&ready, 1, TargetRef::Object(physical(&ready, n).id())),
            )
            .into_state(),
        )
        .into_state();
        assert_eq!(physical(&gone, n).zone(), ObjectZone::Graveyard(B));
    }
    illegal(
        &ready,
        A,
        loyalty_at(&ready, 1, TargetRef::Object(physical(&ready, 4).id())),
    );
    let draw = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    for n in [10, 11] {
        assert_eq!(physical(&draw, n).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(
        draw.execution().window().cards().library(A)[0],
        physical(&draw, 12).id()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_the_wanderer_prevents_noncombat_damage_to_its_controller_and_other_permanents() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("The Wanderer", 1, A, ObjectDestination::Hand),
            creature(2, A, 3, 3, &[]),
            creature(3, B, 4, 4, &[KeywordAbility::Haste]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_ANY_TARGET,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(2),
                        false,
                    ),
                ),
            ),
            named("Jace Beleren", 5, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 4), (White, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let ready = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let ready =
        round(cast_untargeted(&ready, 5, &[(Colourless, 1), (Blue, 2)]).into_state()).into_state();
    let hit = |target| round(targeted(&ready, A, 4, target).into_state()).into_state();
    let object = |n| TargetRef::Object(physical(&ready, n).id());
    for (seat, life) in [(A, 20), (B, 18)] {
        assert_eq!(
            hit(TargetRef::Player(seat)).execution().life_total(seat),
            Some(&life.into())
        );
    }
    for (n, damage) in [(2, 0), (3, 2)] {
        assert_eq!(
            hit(object(n))
                .execution()
                .battlefield_marked_damage(physical(&ready, n).id()),
            Some(damage.into())
        );
    }
    assert_eq!(loyalty(&hit(object(1)), 1), loyalty(&ready, 1) - 2);
    assert_eq!(loyalty(&hit(object(5)), 5), loyalty(&ready, 5));
    illegal(&ready, A, loyalty_at(&ready, 1, object(2)));
    let exiled =
        round(command(&ready, A, loyalty_at(&ready, 1, object(3))).into_state()).into_state();
    assert_eq!(physical(&exiled, 3).zone(), ObjectZone::Exile);
    let declaration = exile_hand::until(ready, |s| {
        s.turn().unwrap().position().number == 2
            && matches!(
                s.pending_decision().map(|d| d.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let attack = reply(
        &declaration,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&declaration, 3).id(),
            defender: A,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let damage = exile_hand::until(attack, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(damage.execution().life_total(A), Some(&16.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_teyo_creates_its_wall_draws_for_two_and_limits_attacks_in_either_direction() {
    use ManaType::*;
    use spellwake_engine::effects::AttackDirection;
    let cost = [(Colourless, 2), (White, 1)];
    let start = game(
        vec![A, B, C],
        vec![
            named("Teyo, Geometric Tactician", 1, A, ObjectDestination::Hand),
            creature(2, B, 2, 3, &[KeywordAbility::Haste]),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &cost).into_state()).into_state();
    let created = round(entered);
    let id = tokens(&created, A, 1)[0];
    let wall = created
        .state()
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap();
    assert_eq!(
        wall.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Wall").unwrap()].into()
    );
    assert_eq!(wall.current_colors, [Color::White].into());
    assert_eq!(
        wall.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 0.into(),
            toughness: 4.into()
        }
    );
    assert!(wall.has_keyword(KeywordAbility::Defender) && wall.has_keyword(KeywordAbility::Flying));
    let ready = created.into_state();
    illegal(&ready, A, loyalty_at(&ready, 1, TargetRef::Player(A)));
    let draw = round(command(&ready, A, loyalty_at(&ready, 1, TargetRef::Player(C))).into_state())
        .into_state();
    for (seat, card) in [(A, 100), (C, 102)] {
        assert_eq!(physical(&draw, card).zone(), ObjectZone::Hand(seat));
    }
    assert_eq!(draw.execution().window().cards().library(B).len(), 1);
    let offer = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    for (direction, allowed, denied) in [
        (AttackDirection::Left, C, A),
        (AttackDirection::Right, A, C),
    ] {
        let chosen = reply(&offer, DecisionReply::AttackDirection(direction)).into_state();
        let decision = exile_hand::until(chosen, |s| {
            s.turn().unwrap().position().number == 2
                && matches!(
                    s.pending_decision().map(|d| d.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let attack = |defender, pw| {
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: physical(&decision, 2).id(),
                defender,
                defending_planeswalker: pw,
            }])
        };
        illegal(
            &decision,
            B,
            GameCommand::Reply {
                decision: decision.pending_decision().unwrap().id(),
                choice: attack(denied, None),
            },
        );
        let walked = attack(A, Some(physical(&decision, 1).id()));
        if denied == A {
            illegal(
                &decision,
                B,
                GameCommand::Reply {
                    decision: decision.pending_decision().unwrap().id(),
                    choice: walked,
                },
            );
        } else {
            assert_eq!(
                physical(reply(&decision, walked).state(), 2).tapped(),
                Some(true)
            );
        }
        assert_eq!(
            physical(reply(&decision, attack(allowed, None)).state(), 2).tapped(),
            Some(true)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_teyo_independent_directions_can_prevent_attacks_until_their_separate_expirations() {
    use ManaType::*;
    use spellwake_engine::effects::AttackDirection;
    let cost = [(Colourless, 2), (White, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Teyo, Geometric Tactician", 1, A, ObjectDestination::Hand),
            named("Teyo, Geometric Tactician", 2, B, ObjectDestination::Hand),
            named("Sol Ring", 3, B, battlefield(B)),
            named("Plains", 4, B, battlefield(B)),
            creature(5, A, 2, 3, &[]),
            creature(6, B, 2, 3, &[]),
            creature(7, C, 2, 3, &[]),
            seed(
                8,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let first =
        round(round(cast_untargeted(&initial, 1, &cost).into_state()).into_state()).into_state();
    let choose = |game: &GameState, seat, card, dir| {
        let offer =
            round(command(game, seat, activate(game, card, 2, &[])).into_state()).into_state();
        reply(&offer, DecisionReply::AttackDirection(dir)).into_state()
    };
    let left = choose(&first, A, 1, AttackDirection::Left);
    let mut main = exile_hand::until(left, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    for (n, ability) in [
        (3, ManaAbilityRef::Printed(ManaAbilityId(0))),
        (
            4,
            ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Plains),
        ),
    ] {
        main = command(
            &main,
            B,
            GameCommand::ActivateMana {
                source: physical(&main, n).id(),
                ability,
            },
        )
        .into_state();
    }
    let cast = command(
        &main,
        B,
        GameCommand::CastUntargeted {
            spell: physical(&main, 2).id(),
            payment: Some(spend(&cost)),
        },
    )
    .into_state();
    let ready = round(round(cast).into_state()).into_state();
    let both = choose(&ready, B, 2, AttackDirection::Right);
    let declare = |game: GameState, turn| {
        exile_hand::until(game, |s| {
            s.turn().unwrap().position().number == turn
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        })
    };
    let attack = |game: &GameState, card, defender| GameCommand::Reply {
        decision: game.pending_decision().unwrap().id(),
        choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(game, card).id(),
            defender,
            defending_planeswalker: None,
        }]),
    };
    let no_attack = exile_hand::until(both, |s| {
        s.turn().unwrap().position().number == 3
            && (s.turn().unwrap().position().step == Step::PostcombatMain
                || matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                ))
    });
    assert_eq!(
        no_attack.turn().unwrap().position().step,
        Step::PostcombatMain
    );
    assert_eq!(physical(&no_attack, 7).tapped(), Some(false));
    let right = declare(no_attack, 4);
    illegal(&right, A, attack(&right, 5, B));
    assert_eq!(
        physical(command(&right, A, attack(&right, 5, C)).state(), 5).tapped(),
        Some(true)
    );
    let passed = reply(&right, DecisionReply::DeclareAttackers(vec![])).into_state();
    let free = declare(passed, 5);
    for defender in [A, C] {
        assert_eq!(
            physical(command(&free, B, attack(&free, 6, defender)).state(), 6).tapped(),
            Some(true)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_freyalise_and_ob_nixilis_are_commanders_and_pay_the_tax_when_recast() {
    use ManaType::*;
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    for (name, color, basic) in [
        ("Ob Nixilis of the Black Oath", Black, "Swamp"),
        ("Freyalise, Llanowar's Fury", Green, "Forest"),
    ] {
        verify_commander_deck(name, basic);
        let initial = super::super::cmm_permanent_costs::commander_opening(
            vec![
                named(name, 100, A, ObjectDestination::Command),
                named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
                seed(
                    1,
                    A,
                    ObjectDestination::LibraryBottom,
                    Program::land(
                        Default::default(),
                        vec![ManaAbility {
                            id: ManaAbilityId(0),
                            output: ManaOutput::Fixed(
                                [(Colourless, 8.into()), (color, 4.into())].into(),
                            ),
                        }],
                    ),
                ),
                seed(
                    2,
                    A,
                    ObjectDestination::LibraryBottom,
                    free_instant(
                        BodyTarget::SINGLE_PERMANENT,
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                            allow_regenerate: true,
                                        },
                                },
                            ),
                        ),
                    ),
                ),
            ],
            None,
        );
        let played = command(
            &initial,
            A,
            GameCommand::PlayLand {
                land: physical(&initial, 1).id(),
            },
        )
        .into_state();
        let mana = command(
            &played,
            A,
            GameCommand::ActivateMana {
                source: physical(&played, 1).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let cast = |s: &GameState, generic| GameCommand::CastWithResources {
            spell: physical(s, 100).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, generic), (color, 2)])),
                ..Default::default()
            },
        };
        let first = round(command(&mana, A, cast(&mana, 3)).into_state()).into_state();
        assert_eq!(loyalty(&first, 100), 3);
        let return_choice = round(
            targeted(&first, A, 2, TargetRef::Object(physical(&first, 100).id())).into_state(),
        )
        .into_state();
        let returned = reply(
            &return_choice,
            DecisionReply::ReturnCommanders(vec![physical(&return_choice, 100).id()]),
        )
        .into_state();
        assert_eq!(physical(&returned, 100).zone(), ObjectZone::Command);
        illegal(&returned, A, cast(&returned, 3));
        let replayed = round(command(&returned, A, cast(&returned, 5)).into_state()).into_state();
        assert_eq!(loyalty(&replayed, 100), 3);
        assert_eq!(
            replayed
                .execution()
                .commander_record(CardId(100))
                .unwrap()
                .command_zone_casts(),
            &2.into()
        );
        assert_eq!(replayed.execution().mana(A, Colourless), 0.into());
        assert_eq!(replayed.execution().mana(A, color), 0.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_gatewatch_beacon_taps_for_white_and_optionally_moves_a_counter_to_an_entering_planeswalker()
{
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    let initial = game(
        vec![A, B],
        vec![
            named("Gatewatch Beacon", 1, A, ObjectDestination::Hand),
            named("The Wanderer", 2, A, ObjectDestination::Hand),
            named("Jace Beleren", 3, B, ObjectDestination::Hand),
            named("Island", 4, B, battlefield(B)),
            named("Island", 5, B, battlefield(B)),
            named("Sol Ring", 6, B, battlefield(B)),
        ],
        &[(Colourless, 5), (White, 1)],
        Step::PrecombatMain,
    );
    let beacon = round(cast_untargeted(&initial, 1, &[(Colourless, 2), (White, 1)]).into_state())
        .into_state();
    assert_eq!(loyalty(&beacon, 1), 3);
    let white = command(
        &beacon,
        A,
        GameCommand::ActivateMana {
            source: physical(&beacon, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(white.execution().mana(A, White), 1.into());
    assert_eq!(physical(&white, 1).tapped(), Some(true));
    let walker =
        round(cast_untargeted(&white, 2, &[(Colourless, 3), (White, 1)]).into_state()).into_state();
    assert_eq!(loyalty(&walker, 2), 5);
    let offer = round(walker).into_state();
    for (choice, counters) in [
        (OptionalEffectChoice::Take, (2, 6)),
        (OptionalEffectChoice::Decline, (3, 5)),
    ] {
        let done = reply(&offer, DecisionReply::OptionalEffect(choice)).into_state();
        assert_eq!((loyalty(&done, 1), loyalty(&done, 2)), counters);
    }
    let done = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let mut opp = exile_hand::until(done, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    for (n, ability) in [
        (
            4,
            ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Island),
        ),
        (
            5,
            ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Island),
        ),
        (6, ManaAbilityRef::Printed(ManaAbilityId(0))),
    ] {
        opp = command(
            &opp,
            B,
            GameCommand::ActivateMana {
                source: physical(&opp, n).id(),
                ability,
            },
        )
        .into_state();
    }
    let opposing = round(
        command(
            &opp,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&opp, 3).id(),
                payment: Some(spend(&[(Colourless, 1), (Blue, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(loyalty(&opposing, 3), 3);
    assert_eq!(loyalty(&opposing, 1), 2);
    assert!(opposing.execution().window().entries().is_empty());
}

pub(super) fn mana_source(n: u64, produced: &[(ManaType, i64)]) -> SeedCard {
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    seed(
        n,
        A,
        battlefield(A),
        Program::land(
            Default::default(),
            vec![ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(
                    produced
                        .iter()
                        .map(|&(color, n)| (color, n.into()))
                        .collect(),
                ),
            }],
        ),
    )
}
fn tap_source(s: &GameState, id: u64) -> GameState {
    command(
        s,
        A,
        GameCommand::ActivateMana {
            source: physical(s, id).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state()
}
fn selected_from(s: &GameState, ability: u64, objects: &[u64]) -> GameCommand {
    GameCommand::ActivatePermanentTargets {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(ability),
        targets: objects
            .iter()
            .map(|&id| TargetRef::Object(physical(s, id).id()))
            .collect(),
        payment: None,
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ajani_steadfast_grants_combat_abilities_distributes_counters_and_prevents_damage_per_source()
{
    use ManaType::*;
    let blue = [(Colourless, 1), (Blue, 2)];
    let mut cards = vec![
        creature(2, A, 2, 3, &[KeywordAbility::Haste]),
        creature(3, A, 1, 2, &[]),
        creature(4, B, 1, 5, &[]),
        named("Jace Beleren", 5, A, ObjectDestination::Hand),
        mana_source(6, &blue),
    ];
    for n in [7, 8] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_ANY_TARGET,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(3),
                    false,
                ),
            ),
        ));
    }
    let base = with_loyalty_for_ultimate(
        "Ajani Steadfast",
        &[(Colourless, 3), (White, 1)],
        4,
        7,
        cards,
    );
    let mana = tap_source(&base, 6);
    let ready = round(cast_untargeted(&mana, 5, &blue).into_state()).into_state();
    let zero = round(command(&ready, A, selected_from(&ready, 0, &[])).into_state()).into_state();
    assert_eq!(loyalty(&zero, 1), 8);
    let one = round(command(&ready, A, selected_from(&ready, 0, &[2])).into_state()).into_state();
    let sheet = one
        .execution()
        .current_characteristics(physical(&one, 2).id())
        .unwrap()
        .unwrap();
    assert_eq!(
        sheet.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 4.into()
        }
    );
    for kw in [
        KeywordAbility::Lifelink,
        KeywordAbility::Vigilance,
        KeywordAbility::FirstStrike,
    ] {
        assert!(sheet.has_keyword(kw));
    }
    let attack = combat_frame(&one, true);
    let open = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 2).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    assert_eq!(physical(&open, 2).tapped(), Some(false));
    let end = exile_hand::until(open, |s| s.turn().unwrap().position().number == 2);
    assert_eq!(end.execution().life_total(A), Some(&23.into()));
    assert_eq!(end.execution().life_total(B), Some(&17.into()));
    let sheet = end
        .execution()
        .current_characteristics(physical(&end, 2).id())
        .unwrap()
        .unwrap();
    assert!(!sheet.has_keyword(KeywordAbility::FirstStrike));
    assert_eq!(sheet.power_toughness.unwrap().derived.power, 2.into());
    let counters = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    assert_eq!(loyalty(&counters, 1), 5);
    assert_eq!(loyalty(&counters, 5), 4);
    for (n, num) in [(2, 1), (3, 1), (4, 0)] {
        assert_eq!(
            counters
                .execution()
                .power_toughness_counters(physical(&counters, n).id())
                .unwrap()
                .plus_one,
            num.into()
        );
    }
    let emblem = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(physical(&emblem, 1).zone(), ObjectZone::Graveyard(A));
    for (receiver, life) in [(A, 19), (B, 17)] {
        let one =
            round(targeted(&emblem, A, 7, TargetRef::Player(receiver)).into_state()).into_state();
        assert_eq!(one.execution().life_total(receiver), Some(&life.into()));
    }
    let walker =
        round(targeted(&emblem, A, 7, TargetRef::Object(physical(&emblem, 5).id())).into_state())
            .into_state();
    assert_eq!(loyalty(&walker, 5), 2);
    let creature =
        round(targeted(&walker, A, 8, TargetRef::Object(physical(&walker, 2).id())).into_state())
            .into_state();
    assert_eq!(physical(&creature, 2).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_teferi_temporal_archmage_selects_one_card_untaps_up_to_four_and_permits_instant_speed_loyalty()
 {
    use ManaType::*;
    verify_commander_deck("Teferi, Temporal Archmage", "Island");
    let jace_price = [(Colourless, 1), (Blue, 2)];
    let mut cards = vec![
        named("Jace Beleren", 2, A, ObjectDestination::Hand),
        mana_source(3, &jace_price),
    ];
    for n in 4..=7 {
        cards.push(mana_source(n, &[(Colourless, 1)]));
    }
    for n in 10..13 {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let base = with_loyalty_for_ultimate(
        "Teferi, Temporal Archmage",
        &[(Colourless, 4), (Blue, 2)],
        5,
        10,
        cards,
    );
    let ready =
        round(cast_untargeted(&tap_source(&base, 3), 2, &jace_price).into_state()).into_state();
    let look = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    let RuleFrame::LibraryTop(frame) = look.pending_decision().unwrap().frame() else {
        panic!("looked cards");
    };
    assert_eq!(
        frame.domain_top_first(),
        &[physical(&look, 10).id(), physical(&look, 11).id()]
    );
    let chosen = reply(
        &look,
        DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
            selected: Some(physical(&look, 11).id()),
        }),
    )
    .into_state();
    assert_eq!(physical(&chosen, 11).zone(), ObjectZone::Hand(A));
    assert_eq!(
        chosen.execution().window().cards().library(A).last(),
        Some(&physical(&chosen, 10).id())
    );
    let mut tapped = ready.clone();
    for n in 4..=7 {
        tapped = tap_source(&tapped, n);
    }
    illegal(&tapped, A, selected_from(&tapped, 1, &[3, 4, 5, 6, 7]));
    for targets in [&[][..], &[4, 5, 6, 7][..]] {
        let untap = round(command(&tapped, A, selected_from(&tapped, 1, targets)).into_state())
            .into_state();
        for n in 3..=7 {
            assert_eq!(physical(&untap, n).tapped(), Some(!targets.contains(&n)));
        }
    }
    let opp_priority = |s: GameState| {
        let main = exile_hand::until(s, |s| {
            s.turn().unwrap().position().number == 2
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        pass(pass(&main).state()).into_state()
    };
    let negative = opp_priority(ready.clone());
    illegal(&negative, A, activate(&negative, 2, 0, &[]));
    let emblem = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(physical(&emblem, 1).zone(), ObjectZone::Graveyard(A));
    let priority = opp_priority(emblem);
    let ability = command(
        &priority,
        A,
        GameCommand::ActivateAbility {
            source: physical(&priority, 2).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(A),
            payment: None,
        },
    )
    .into_state();
    let drawn = round(ability).into_state();
    assert_eq!(physical(&drawn, 10).zone(), ObjectZone::Hand(A));
    assert_eq!(loyalty(&drawn, 2), 2);
}
