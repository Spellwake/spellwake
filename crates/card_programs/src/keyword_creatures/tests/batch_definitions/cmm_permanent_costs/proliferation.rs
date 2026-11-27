//! Set cards that proliferate from a spell, activation, or combat-damage trigger.
use super::*;
use spellwake_engine::effects::CounterKind;

fn prepare(state: GameState) -> GameState {
    let recipient = physical(&state, 2).id();
    let count =
        round(targeted(&state, A, 6, TargetRef::Object(recipient)).into_state()).into_state();
    let shield =
        round(targeted(&count, A, 7, TargetRef::Object(recipient)).into_state()).into_state();
    round(cast_untargeted(&shield, 8, &[]).into_state()).into_state()
}
fn board(name: &str, price: &[(ManaType, i64)], mut extra: Vec<SeedCard>) -> GameState {
    extra.extend([
        named(name, 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 2, &[]),
        creature(3, A, 1, 1, &[]),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            super::counter_and_sacrifice::plus_counters_spell(1),
        ),
        seed(
            7,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::Indestructible,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ),
        ),
        seed(
            8,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    CounterKind::Experience,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ),
        ),
    ]);
    prepare(game(vec![A, B], extra, price, Step::PrecombatMain))
}
fn check_choices(state: &GameState) {
    let unchanged = physical(state, 2).id();
    let RuleFrame::Proliferate(frame) = state.pending_decision().unwrap().frame() else {
        panic!("choose counters to proliferate");
    };
    assert_eq!(
        frame.domain().get(&TargetRef::Object(unchanged)),
        Some(&[CounterKind::PlusOne, CounterKind::Indestructible].into())
    );
    assert_eq!(
        frame.domain().get(&TargetRef::Player(A)),
        Some(&[CounterKind::Experience].into())
    );
    for targets in [
        vec![],
        vec![TargetRef::Object(unchanged)],
        vec![TargetRef::Object(unchanged), TargetRef::Player(A)],
    ] {
        let on_card = i64::from(targets.contains(&TargetRef::Object(unchanged)));
        let on_player = i64::from(targets.contains(&TargetRef::Player(A)));
        let done = reply(state, DecisionReply::Proliferate(targets)).into_state();
        for kind in [CounterKind::PlusOne, CounterKind::Indestructible] {
            assert_eq!(
                done.execution().counters_of_kind(unchanged, kind),
                (1 + on_card).into()
            );
        }
        assert_eq!(
            done.execution().experience_counters(A),
            (1 + on_player).into()
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_courage_in_crisis_and_wanderers_strike_complete_their_targeted_effect_before_proliferating()
{
    for (name, exiles, color, generic) in [
        ("Courage in Crisis", false, Green, 2),
        ("Wanderer's Strike", true, White, 4),
    ] {
        let price = [(Colourless, generic), (color, 1)];
        let ready = board(name, &price, vec![remove_spell(9, A)]);
        let chosen = physical(&ready, 3).id();
        let cast = command(
            &ready,
            A,
            GameCommand::CastPaid {
                spell: physical(&ready, 1).id(),
                target: TargetRef::Object(chosen),
                payment: spend(&price),
            },
        )
        .into_state();
        let gone =
            round(targeted(&cast, A, 9, TargetRef::Object(chosen)).into_state()).into_state();
        let missed = round(gone).into_state();
        assert!(missed.pending_decision().is_none());
        assert_eq!(missed.execution().experience_counters(A), 1.into());
        let ask = round(cast).into_state();
        if exiles {
            assert_eq!(physical(&ask, 3).zone(), ObjectZone::Exile);
        } else {
            assert_eq!(sheet(&ask, chosen).plus_one_counters, 1.into());
            let twice = reply(
                &ask,
                DecisionReply::Proliferate(vec![TargetRef::Object(chosen)]),
            )
            .into_state();
            assert_eq!(sheet(&twice, chosen).plus_one_counters, 2.into());
        }
        check_choices(&ask);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_karns_bastion_adds_colorless_or_pays_four_and_taps_to_proliferate_selected_counters() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let start = board("Karn's Bastion", &[(Colourless, 4)], vec![]);
    let field = command(
        &start,
        A,
        GameCommand::PlayLand {
            land: physical(&start, 1).id(),
        },
    )
    .into_state();
    let single = command(
        &field,
        A,
        GameCommand::ActivateMana {
            source: physical(&field, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(single.execution().mana(A, Colourless), 5.into());
    let paid = command(&field, A, activation(&field, 1, 0, &[(Colourless, 4)])).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    check_choices(&round(paid).into_state());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_flux_channeler_proliferates_for_each_controlled_noncreature_cast_before_the_spell_resolves()
{
    let price = [(Colourless, 2), (Blue, 1)];
    let ready = board(
        "Flux Channeler",
        &price,
        vec![
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    Default::default(),
                ),
            ),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ],
    );
    let channeler = round(cast_untargeted(&ready, 1, &price).into_state()).into_state();
    let ordinary = cast_untargeted(&channeler, 4, &[]).into_state();
    assert_eq!(ordinary.execution().window().entries().len(), 1);
    let waiting = cast_untargeted(&channeler, 5, &[]).into_state();
    assert_eq!(waiting.execution().window().entries().len(), 2);
    let ask = round(waiting).into_state();
    assert_eq!(physical(&ask, 5).zone(), ObjectZone::Stack);
    check_choices(&ask);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_thrummingbird_and_grateful_apparition_proliferate_after_combat_damage_to_their_printed_recipients()
 {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    for (name, color, walker_triggers) in [
        ("Thrummingbird", Blue, false),
        ("Grateful Apparition", White, true),
    ] {
        let price = [(Colourless, 1), (color, 1)];
        let mut rock = Program::artifact(None, vec![]);
        rock.mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed([(Blue, 2.into()), (Colourless, 1.into())].into()),
        });
        let start = board(
            name,
            &price,
            vec![
                named("Jace Beleren", 9, B, ObjectDestination::Hand),
                seed(
                    10,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    rock,
                ),
            ],
        );
        let bird = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
        assert!(sheet(&bird, physical(&bird, 1).id()).has_keyword(KeywordAbility::Flying));
        let theirs = exile_hand::until(bird, |s| {
            s.turn().unwrap().position().number == 2
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let funded = command(
            &theirs,
            B,
            GameCommand::ActivateMana {
                source: physical(&theirs, 10).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let walker = round(
            command(
                &funded,
                B,
                GameCommand::CastUntargeted {
                    spell: physical(&funded, 9).id(),
                    payment: Some(spend(&[(Blue, 2), (Colourless, 1)])),
                },
            )
            .into_state(),
        )
        .into_state();
        let attacks = exile_hand::until(walker, |s| {
            s.turn().unwrap().position().number == 3
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let source = physical(&attacks, 1).id();
        let defender = physical(&attacks, 9).id();
        for aimed in [false, true] {
            let combat = reply(
                &attacks,
                DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker: source,
                    defender: B,
                    defending_planeswalker: aimed.then_some(defender),
                }]),
            )
            .into_state();
            let reached = exile_hand::until(combat, |s| {
                matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Proliferate(_))
                ) || s.turn().unwrap().position().step == Step::EndCombat
            });
            assert_eq!(
                reached.execution().life_total(B),
                Some(&(20 - i64::from(!aimed)).into())
            );
            assert_eq!(
                reached
                    .execution()
                    .counters_of_kind(defender, CounterKind::Loyalty),
                (3 - i64::from(aimed)).into()
            );
            if !aimed || walker_triggers {
                check_choices(&reached);
            } else {
                assert!(reached.execution().window().entries().is_empty());
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heartless_act_destroys_only_counterless_creatures_or_removes_up_to_three_counters_in_the_chosen_combination()
 {
    use spellwake_engine::mana::CastMode;
    let cost = [(Colourless, 1), (Black, 1)];
    let field = board(
        "Heartless Act",
        &cost,
        vec![seed(
            9,
            A,
            ObjectDestination::Hand,
            super::counter_and_sacrifice::plus_counters_spell(3),
        )],
    );
    let countered = physical(&field, 2).id();
    let ordinary = physical(&field, 3).id();
    let more =
        round(targeted(&field, A, 9, TargetRef::Object(countered)).into_state()).into_state();
    let activate = |n, mode| GameCommand::CastWithResources {
        spell: physical(&more, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&cost)),
            modes: vec![CastMode {
                mode,
                target: Some(TargetRef::Object(n)),
                targets: vec![],
            }],
            ..Default::default()
        },
    };
    illegal(&more, A, activate(countered, 0));
    let killed = round(command(&more, A, activate(ordinary, 0)).into_state()).into_state();
    assert_eq!(physical(&killed, 3).zone(), ObjectZone::Graveyard(A));
    let remove = round(command(&more, A, activate(countered, 1)).into_state()).into_state();
    let RuleFrame::CounterRemoval(frame) = remove.pending_decision().unwrap().frame() else {
        panic!("counter allocation")
    };
    assert_eq!(frame.object(), countered);
    assert_eq!(frame.maximum(), 3);
    let answer = |plus, shield| {
        DecisionReply::RemoveCounters(
            [
                (CounterKind::PlusOne, plus),
                (CounterKind::Indestructible, shield),
            ]
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .collect(),
        )
    };
    illegal(
        &remove,
        A,
        GameCommand::Reply {
            decision: remove.pending_decision().unwrap().id(),
            choice: answer(3, 1),
        },
    );
    for (plus, shield) in [(0, 0), (2, 1), (3, 0)] {
        let done = reply(&remove, answer(plus, shield)).into_state();
        assert_eq!(
            done.execution()
                .counters_of_kind(countered, CounterKind::PlusOne),
            i64::from(4 - plus).into()
        );
        assert_eq!(
            done.execution()
                .counters_of_kind(countered, CounterKind::Indestructible),
            i64::from(1 - shield).into()
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_deepglow_skate_doubles_each_counter_kind_on_only_the_chosen_permanents_including_opposing_permanents()
 {
    let cost = [(Colourless, 4), (Blue, 1)];
    let field = board(
        "Deepglow Skate",
        &cost,
        vec![seed(
            9,
            A,
            ObjectDestination::Hand,
            super::counter_and_sacrifice::plus_counters_spell(2),
        )],
    );
    let own = physical(&field, 3).id();
    let opposing = physical(&field, 2).id();
    let marked = round(targeted(&field, A, 9, TargetRef::Object(own)).into_state()).into_state();
    let ask = round(cast_untargeted(&marked, 1, &cost).into_state()).into_state();
    for chosen in [vec![], vec![opposing], vec![own, opposing]] {
        let targets = chosen.iter().copied().map(TargetRef::Object).collect();
        let stack = reply(&ask, DecisionReply::TriggerPermanentTargets(targets)).into_state();
        let done = round(stack).into_state();
        let factor = |id| if chosen.contains(&id) { 2 } else { 1 };
        for (id, kind, n) in [
            (own, CounterKind::PlusOne, 2),
            (opposing, CounterKind::PlusOne, 1),
            (opposing, CounterKind::Indestructible, 1),
        ] {
            assert_eq!(
                done.execution().counters_of_kind(id, kind),
                (n * factor(id)).into()
            );
        }
        assert_eq!(done.execution().experience_counters(A), 1.into());
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_unbounded_potential_chooses_zero_to_two_creatures_or_proliferates_and_entwine_resolves_counter_placement_first()
 {
    use spellwake_engine::mana::CastMode;
    let base = [(Colourless, 1), (White, 1)];
    let extra = [(Colourless, 4), (White, 2)];
    let game = board("Unbounded Potential", &extra, vec![]);
    let own = physical(&game, 3).id();
    let opponent = physical(&game, 2).id();
    let cast = |modes: &[u8], targets: &[ObjectId], entwine| GameCommand::CastWithResources {
        spell: physical(&game, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(if entwine { &extra } else { &base })),
            additional: if entwine {
                [(0, 1.into())].into()
            } else {
                Default::default()
            },
            modes: modes
                .iter()
                .map(|&mode| CastMode {
                    mode,
                    target: None,
                    targets: if mode == 0 {
                        targets.iter().copied().map(TargetRef::Object).collect()
                    } else {
                        vec![]
                    },
                })
                .collect(),
            ..Default::default()
        },
    };
    illegal(&game, A, cast(&[0, 1], &[own, opponent], false));
    for ids in [vec![], vec![own], vec![opponent, own]] {
        let done = round(command(&game, A, cast(&[0], &ids, false)).into_state()).into_state();
        for (n, original) in [(own, 0), (opponent, 1)] {
            assert_eq!(
                done.execution().counters_of_kind(n, CounterKind::PlusOne),
                (original + i64::from(ids.contains(&n))).into()
            );
        }
    }
    let ask = round(command(&game, A, cast(&[1], &[], false)).into_state()).into_state();
    check_choices(&ask);
    let both =
        round(command(&game, A, cast(&[0, 1], &[own, opponent], true)).into_state()).into_state();
    assert_eq!(
        both.execution().counters_of_kind(own, CounterKind::PlusOne),
        1.into()
    );
    assert_eq!(
        both.execution()
            .counters_of_kind(opponent, CounterKind::PlusOne),
        2.into()
    );
    let done = reply(
        &both,
        DecisionReply::Proliferate(vec![TargetRef::Object(own), TargetRef::Object(opponent)]),
    )
    .into_state();
    for (n, k, c) in [
        (own, CounterKind::PlusOne, 2),
        (opponent, CounterKind::PlusOne, 3),
        (opponent, CounterKind::Indestructible, 2),
    ] {
        assert_eq!(done.execution().counters_of_kind(n, k), c.into());
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_knighted_myr_adapts_only_without_plus_one_counters_and_gains_double_strike_once_per_batch_through_cleanup()
 {
    use spellwake_engine::characteristics::KeywordAbility::DoubleStrike;
    let mut cards = draw_pile();
    cards.extend([
        named("Knighted Myr", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            super::counter_and_sacrifice::plus_counters_spell(3),
        ),
    ]);
    let cost = [(Colourless, 2), (White, 1)];
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 6), (White, 3)],
        Step::PrecombatMain,
    );
    let field = round(cast_untargeted(&start, 1, &cost).into_state()).into_state();
    let id = physical(&field, 1).id();
    let paid = round(command(&field, A, activation(&field, 1, 0, &cost)).into_state()).into_state();
    assert_eq!(
        paid.execution().counters_of_kind(id, CounterKind::PlusOne),
        1.into()
    );
    assert_eq!(paid.execution().window().entries().len(), 1);
    let granted = round(paid).into_state();
    assert!(sheet(&granted, id).has_keyword(DoubleStrike));
    let again =
        round(command(&granted, A, activation(&granted, 1, 0, &cost)).into_state()).into_state();
    assert_eq!(
        again.execution().counters_of_kind(id, CounterKind::PlusOne),
        1.into()
    );
    assert!(again.execution().window().entries().is_empty());
    let third = round(targeted(&again, A, 2, TargetRef::Object(id)).into_state()).into_state();
    assert_eq!(
        third.execution().counters_of_kind(id, CounterKind::PlusOne),
        4.into()
    );
    assert_eq!(third.execution().window().entries().len(), 1);
    let finish = round(third).into_state();
    let expired = exile_hand::until(finish, |s| s.turn().unwrap().position().number == 2);
    assert!(!sheet(&expired, id).has_keyword(DoubleStrike));
    assert_size(&expired, id, 6, 6);
}
