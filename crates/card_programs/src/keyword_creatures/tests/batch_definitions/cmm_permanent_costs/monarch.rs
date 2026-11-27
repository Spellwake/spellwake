//! Source-bound commander and monarch games.
use super::*;
use spellwake_engine::{characteristics::KeywordAbility::*, combat::CombatDecision};

fn monarch_game(name: &str, price: &[(ManaType, i64)]) -> (GameState, GameState) {
    let mut cards = draw_pile();
    cards.extend([
        named(name, 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 2, &[Haste]),
        creature(3, B, 1, 1, &[Haste]),
        named(
            "Forest",
            4,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Sol Ring",
            5,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed(
            6,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::become_monarch(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                ),
            ),
        ),
    ]);
    cards.extend((20..25).map(|n| {
        seed(
            n,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    let start = game(vec![A, B], cards, price, Step::PrecombatMain);
    let entered = round(cast_untargeted(&start, 1, price).into_state()).into_state();
    assert_eq!(entered.execution().monarch(), None);
    let crown = round(entered.clone()).into_state();
    assert_eq!(crown.execution().monarch(), Some(A));
    (entered, crown)
}
fn lose_monarch(crown: GameState) -> GameState {
    let attacks = exile_hand::until(crown, |s| {
        s.turn().unwrap().position().number == 2
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let declared = reply(
        &attacks,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attacks, 2).id(),
            defender: A,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    exile_hand::until(declared, |s| s.execution().monarch() == Some(B))
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_crimson_fleet_commodore_staunch_throneguard_and_thorn_of_the_black_rose_gain_monarch_on_entry_and_can_lose_it_to_combat()
 {
    for (name, price, keyword, p, t) in [
        (
            "Crimson Fleet Commodore",
            vec![(Colourless, 3), (Red, 1)],
            Trample,
            5,
            2,
        ),
        (
            "Staunch Throneguard",
            vec![(Colourless, 5)],
            Vigilance,
            2,
            5,
        ),
        (
            "Thorn of the Black Rose",
            vec![(Colourless, 3), (Black, 1)],
            Deathtouch,
            1,
            3,
        ),
    ] {
        let (_, crown) = monarch_game(name, &price);
        let source = physical(&crown, 1).id();
        assert_size(&crown, source, p, t);
        assert!(sheet(&crown, source).has_keyword(keyword));
        let end = exile_hand::until(crown, |s| {
            s.turn().unwrap().position().step == Step::End
                && !s.execution().window().entries().is_empty()
        });
        let before = hand_size(&end, A);
        let drew = round(end).into_state();
        assert_eq!(hand_size(&drew, A), before + 1);
        let stolen = lose_monarch(drew);
        assert_eq!(stolen.execution().life_total(A), Some(&18.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_skyline_despot_and_queen_marchesa_check_the_monarch_at_upkeep_and_make_the_correct_token() {
    let vocab = current_catalogue().1;
    for (name, price, dragon) in [
        ("Skyline Despot", vec![(Colourless, 5), (Red, 2)], true),
        (
            "Queen Marchesa",
            vec![(Colourless, 1), (Red, 1), (White, 1), (Black, 1)],
            false,
        ),
    ] {
        let (_, crown) = monarch_game(name, &price);
        let source = sheet(&crown, physical(&crown, 1).id());
        if dragon {
            assert!(source.has_keyword(Flying));
        } else {
            assert!(source.has_keyword(Deathtouch) && source.has_keyword(Haste));
        }
        for (state, own) in [(crown.clone(), true), (lose_monarch(crown), false)] {
            let upkeep = exile_hand::until(state, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::Upkeep
            });
            assert_eq!(
                upkeep.execution().window().entries().len(),
                usize::from(own == dragon)
            );
            if own == dragon {
                let made = round(upkeep);
                let id = tokens(&made, A, 1)[0];
                let token = sheet(made.state(), id);
                assert_eq!(
                    token.current_creature_subtypes,
                    [vocab
                        .resolve_label(if dragon { "Dragon" } else { "Assassin" })
                        .unwrap()]
                    .into()
                );
                assert_eq!(
                    token.current_colors,
                    [if dragon { Color::Red } else { Color::Black }].into()
                );
                if dragon {
                    assert!(token.has_keyword(Flying));
                    assert_size(made.state(), id, 5, 5);
                } else {
                    assert!(token.has_keyword(Haste) && token.has_keyword(Deathtouch));
                    assert_size(made.state(), id, 1, 1);
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_regal_behemoth_grants_one_additional_mana_of_any_color_only_for_a_controlled_land_while_monarch()
 {
    use spellwake_engine::{
        land::BasicLandType,
        mana::{ManaAbilityId, ManaAbilityRef},
    };
    let (entered, crown) = monarch_game("Regal Behemoth", &[(Colourless, 4), (Green, 2)]);
    assert!(sheet(&crown, physical(&crown, 1).id()).has_keyword(Trample));
    let forest = |s: &GameState| GameCommand::ActivateMana {
        source: physical(s, 4).id(),
        ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
    };
    let before = command(&entered, A, forest(&entered)).into_state();
    assert_eq!(before.execution().mana(A, Green), 1.into());
    assert!(before.pending_decision().is_none());
    let choice = command(&crown, A, forest(&crown)).into_state();
    let RuleFrame::ManaColor(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("five bonus colors");
    };
    let colors = [White, Blue, Black, Red, Green];
    assert_eq!(frame.colors(), &colors.into());
    for color in colors {
        let resolved = reply(&choice, DecisionReply::ManaColor(color)).into_state();
        assert_eq!(
            resolved.execution().mana(A, color),
            (1 + i64::from(color == Green)).into()
        );
        assert_eq!(
            resolved.execution().mana(A, Green),
            (1 + i64::from(color == Green)).into()
        );
    }
    let rock = command(
        &crown,
        A,
        GameCommand::ActivateMana {
            source: physical(&crown, 5).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert!(rock.pending_decision().is_none());
    assert_eq!(rock.execution().mana(A, Colourless), 2.into());
    let lost = lose_monarch(crown);
    let main = exile_hand::until(lost, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let plain = command(&main, A, forest(&main)).into_state();
    assert_eq!(plain.execution().mana(A, Green), 1.into());
    assert!(plain.pending_decision().is_none());
}

fn retire_commander(state: &GameState, removal: u64) -> GameState {
    let exile = round(
        targeted(
            state,
            A,
            removal,
            TargetRef::Object(physical(state, 100).id()),
        )
        .into_state(),
    )
    .into_state();
    reply(&exile, DecisionReply::ReturnCommanders(vec![])).into_state()
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_loyal_apprentice_drake_guardian_and_subordinate_check_their_own_commander_at_combat_start_and_again_on_resolution()
 {
    let cases = [
        ("Loyal Apprentice", vec![(Colourless, 1), (Red, 1)], Haste),
        ("Loyal Drake", vec![(Colourless, 2), (Blue, 1)], Flying),
        ("Loyal Guardian", vec![(Colourless, 4), (Green, 1)], Trample),
        (
            "Loyal Subordinate",
            vec![(Colourless, 2), (Black, 1)],
            Menace,
        ),
    ];
    for (name, price, keyword) in cases {
        let (absent, present) = super::alternative_costs::conditional_commander_game(
            name,
            vec![
                seed(
                    3,
                    A,
                    ObjectDestination::LibraryBottom,
                    free_instant(
                        BodyTarget::SINGLE_CREATURE,
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
                seed(
                    4,
                    A,
                    ObjectDestination::LibraryBottom,
                    Program::creature(
                        Some(ManaCost::free()),
                        1.into(),
                        1.into(),
                        Default::default(),
                    ),
                ),
            ],
        );
        for (base, commander) in [(absent, false), (present, true)] {
            let ally = round(cast_untargeted(&base, 4, &[]).into_state()).into_state();
            let field = round(cast_untargeted(&ally, 1, &price).into_state()).into_state();
            let source = physical(&field, 1).id();
            assert!(sheet(&field, source).has_keyword(keyword));
            let phase = exile_hand::until(field, |s| {
                s.turn().unwrap().position().step == Step::BeginningCombat
            });
            assert_eq!(
                phase.execution().window().entries().len(),
                usize::from(commander)
            );
            if !commander {
                continue;
            }
            let missing = retire_commander(&phase, 3);
            let size = hand_size(&missing, A);
            let stopped = round(missing).into_state();
            assert_eq!(hand_size(&stopped, A), size);
            assert_eq!(stopped.execution().life_total(B), Some(&40.into()));
            assert_eq!(sheet(&stopped, source).plus_one_counters, 0.into());
            assert_eq!(
                stopped
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|o| o.zone() == ObjectZone::Battlefield && o.card_id().is_none())
                    .count(),
                0
            );
            let before = hand_size(&phase, A);
            let done = round(phase);
            match name {
                "Loyal Apprentice" => {
                    let id = tokens(&done, A, 1)[0];
                    let token = sheet(done.state(), id);
                    assert!(
                        token.has_type(CardType::Artifact)
                            && token.has_keyword(Flying)
                            && token.has_keyword(Haste)
                    );
                    assert!(token.current_colors.is_empty());
                    assert_eq!(
                        token.current_creature_subtypes,
                        [current_catalogue().1.resolve_label("Thopter").unwrap()].into()
                    );
                    assert_size(done.state(), id, 1, 1);
                    let future = exile_hand::until(done.into_state(), |s| {
                        s.turn().unwrap().position().number == 2
                            && s.turn().unwrap().position().step == Step::PrecombatMain
                    });
                    assert!(!sheet(&future, id).has_keyword(Haste));
                }
                "Loyal Drake" => assert_eq!(hand_size(done.state(), A), before + 1),
                "Loyal Guardian" => {
                    for n in [1, 4, 100] {
                        assert_eq!(
                            sheet(done.state(), physical(done.state(), n).id()).plus_one_counters,
                            1.into()
                        );
                    }
                }
                "Loyal Subordinate" => {
                    assert_eq!(done.state().execution().life_total(B), Some(&37.into()));
                    assert_eq!(done.state().execution().life_total(A), Some(&40.into()));
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_angelic_field_marshal_gains_two_power_and_toughness_and_grants_vigilance_only_while_its_commander_is_controlled()
 {
    let (absent, present) = super::alternative_costs::conditional_commander_game(
        "Angelic Field Marshal",
        vec![
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    Default::default(),
                ),
            ),
        ],
    );
    for (base, controlled) in [(absent, false), (present, true)] {
        let ally = round(cast_untargeted(&base, 4, &[]).into_state()).into_state();
        let field = round(cast_untargeted(&ally, 1, &[(Colourless, 2), (White, 2)]).into_state())
            .into_state();
        let source = physical(&field, 1).id();
        assert!(sheet(&field, source).has_keyword(Flying));
        let stat = if controlled { 5 } else { 3 };
        assert_size(&field, source, stat, stat);
        for n in [1, 4] {
            assert_eq!(
                sheet(&field, physical(&field, n).id()).has_keyword(Vigilance),
                controlled
            );
        }
        if controlled {
            assert!(sheet(&field, physical(&field, 100).id()).has_keyword(Vigilance));
            let expired = retire_commander(&field, 3);
            assert_size(&expired, source, 3, 3);
            assert!(!sheet(&expired, source).has_keyword(Vigilance));
            assert!(!sheet(&expired, physical(&expired, 4).id()).has_keyword(Vigilance));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_loyal_unicorn_prevents_damage_to_its_controllers_creatures_and_grants_other_creatures_vigilance_through_cleanup()
 {
    let mut ally = Program::creature(
        Some(ManaCost::free()),
        2.into(),
        2.into(),
        Default::default(),
    );
    ally.printed_keywords.insert(Haste);
    let mut blocker = Program::creature(
        Some(ManaCost::free()),
        3.into(),
        4.into(),
        Default::default(),
    );
    blocker.printed_keywords.insert(Flash);
    let (absent, present) = super::alternative_costs::conditional_commander_game(
        "Loyal Unicorn",
        vec![
            seed(3, A, ObjectDestination::LibraryBottom, ally),
            seed(103, B, ObjectDestination::LibraryBottom, blocker),
        ],
    );
    for (base, commander) in [(absent, false), (present, true)] {
        let ally = round(cast_untargeted(&base, 3, &[]).into_state()).into_state();
        let field = round(cast_untargeted(&ally, 1, &[(Colourless, 3), (White, 1)]).into_state())
            .into_state();
        assert!(sheet(&field, physical(&field, 1).id()).has_keyword(Vigilance));
        let priority = pass(&field).into_state();
        let all = round(
            command(
                &priority,
                B,
                GameCommand::CastUntargeted {
                    spell: physical(&priority, 103).id(),
                    payment: None,
                },
            )
            .into_state(),
        )
        .into_state();
        let phase = exile_hand::until(all, |s| {
            s.turn().unwrap().position().step == Step::BeginningCombat
        });
        assert_eq!(
            phase.execution().window().entries().len(),
            usize::from(commander)
        );
        let done = if commander {
            round(phase).into_state()
        } else {
            phase
        };
        let knight = physical(&done, 3).id();
        let defender = physical(&done, 103).id();
        assert_eq!(sheet(&done, knight).has_keyword(Vigilance), commander);
        assert!(!sheet(&done, defender).has_keyword(Vigilance));
        let attack = combat_frame(&done, true);
        let battle = reply(
            &attack,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: knight,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        assert_eq!(physical(&battle, 3).tapped(), Some(!commander));
        let asking = combat_frame(&battle, false);
        let blocked = reply(
            &asking,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: defender,
                attacker: knight,
            }]),
        )
        .into_state();
        let end = exile_hand::until(blocked, |s| {
            s.turn().unwrap().position().step == Step::EndCombat
        });
        assert_eq!(
            end.execution().battlefield_marked_damage(defender),
            Some(2.into())
        );
        if commander {
            assert_eq!(
                end.execution().battlefield_marked_damage(knight),
                Some(0.into())
            );
            let later = exile_hand::until(end, |s| {
                s.turn().unwrap().position().number == 2
                    && s.turn().unwrap().position().step == Step::PrecombatMain
            });
            assert!(!sheet(&later, knight).has_keyword(Vigilance));
        } else {
            assert_eq!(physical(&end, 3).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_entourage_of_trest_can_block_two_attackers_only_when_its_controller_is_the_monarch() {
    let (_, crown) = monarch_game("Entourage of Trest", &[(Colourless, 4), (Green, 1)]);
    let b = pass(&crown).into_state();
    let stolen = round(
        command(
            &b,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&b, 6).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    for (state, entitled) in [(crown, true), (stolen, false)] {
        let attacks = exile_hand::until(state, |s| {
            s.turn().unwrap().position().number == 2
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let declared = reply(
            &attacks,
            DecisionReply::DeclareAttackers(
                [2, 3]
                    .into_iter()
                    .map(|n| AttackDeclaration {
                        attacker: physical(&attacks, n).id(),
                        defender: A,
                        defending_planeswalker: None,
                    })
                    .collect(),
            ),
        )
        .into_state();
        let blockers = combat_frame(&declared, false);
        let source = physical(&blockers, 1).id();
        let both = DecisionReply::DeclareBlockers(
            [2, 3]
                .into_iter()
                .map(|n| BlockDeclaration {
                    attacker: physical(&blockers, n).id(),
                    blocker: source,
                })
                .collect(),
        );
        if entitled {
            let chosen = reply(&blockers, both).into_state();
            assert_eq!(physical(&chosen, 1).zone(), ObjectZone::Battlefield);
        } else {
            illegal(
                &blockers,
                A,
                GameCommand::Reply {
                    decision: blockers.pending_decision().unwrap().id(),
                    choice: both,
                },
            );
            let one = reply(
                &blockers,
                DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    attacker: physical(&blockers, 2).id(),
                    blocker: source,
                }]),
            )
            .into_state();
            assert_eq!(physical(&one, 1).zone(), ObjectZone::Battlefield);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_stormsurge_kraken_gains_a_blocked_draw_trigger_and_two_power_and_toughness_only_under_lieutenant()
 {
    use spellwake_engine::effects::OptionalEffectChoice;
    let mut defender = Program::creature(
        Some(ManaCost::free()),
        1.into(),
        8.into(),
        Default::default(),
    );
    defender.printed_keywords.insert(Flash);
    let (absent, present) = super::alternative_costs::conditional_commander_game(
        "Stormsurge Kraken",
        vec![seed(103, B, ObjectDestination::LibraryBottom, defender)],
    );
    for (base, controlled) in [(absent, false), (present, true)] {
        let field = round(cast_untargeted(&base, 1, &[(Colourless, 3), (Blue, 2)]).into_state())
            .into_state();
        let kraken = physical(&field, 1).id();
        let stat = if controlled { 7 } else { 5 };
        assert_size(&field, kraken, stat, stat);
        assert!(sheet(&field, kraken).has_keyword(Hexproof));
        let priority = pass(&field).into_state();
        let all = round(
            command(
                &priority,
                B,
                GameCommand::CastUntargeted {
                    spell: physical(&priority, 103).id(),
                    payment: None,
                },
            )
            .into_state(),
        )
        .into_state();
        let attack = exile_hand::until(all, |s| {
            s.turn().unwrap().position().number == 3
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let attacking = reply(
            &attack,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: kraken,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        let asking = combat_frame(&attacking, false);
        let blocked = reply(
            &asking,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: physical(&asking, 103).id(),
                attacker: kraken,
            }]),
        )
        .into_state();
        assert_eq!(
            blocked.execution().window().entries().len(),
            usize::from(controlled)
        );
        if controlled {
            let offer = round(blocked).into_state();
            let before = hand_size(&offer, A);
            for (answer, n) in [
                (OptionalEffectChoice::Decline, 0),
                (OptionalEffectChoice::Take, 2),
            ] {
                let end = reply(&offer, DecisionReply::OptionalEffect(answer)).into_state();
                assert_eq!(hand_size(&end, A), before + n);
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_palace_jailer_exiles_an_opponents_creature_until_an_opponent_becomes_monarch_even_after_the_jailer_leaves()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("Palace Jailer", 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 2, &[Haste]),
        creature(3, B, 2, 3, &[]),
        creature(4, A, 1, 1, &[]),
        remove_spell(5, A),
    ]);
    let price = [(Colourless, 2), (White, 2)];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let mut settled = field;
    for _ in 0..8 {
        if settled.execution().monarch() == Some(A)
            && physical(&settled, 3).zone() == ObjectZone::Exile
        {
            break;
        }
        settled = if let Some(pending) = settled.pending_decision() {
            let answer = match pending.frame() {
                RuleFrame::Triggers(order) => {
                    DecisionReply::OrderTriggers(order.instances().to_vec())
                }
                RuleFrame::TriggerTarget(_) => {
                    illegal(
                        &settled,
                        A,
                        GameCommand::Reply {
                            decision: pending.id(),
                            choice: DecisionReply::TriggerTarget(TargetRef::Object(
                                physical(&settled, 4).id(),
                            )),
                        },
                    );
                    DecisionReply::TriggerTarget(TargetRef::Object(physical(&settled, 3).id()))
                }
                other => panic!("unexpected entry decision {other:?}"),
            };
            reply(&settled, answer).into_state()
        } else {
            round(settled).into_state()
        };
    }
    assert_eq!(physical(&settled, 3).zone(), ObjectZone::Exile);
    assert_eq!(settled.execution().monarch(), Some(A));
    let left = round(
        targeted(
            &settled,
            A,
            5,
            TargetRef::Object(physical(&settled, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(physical(&left, 3).zone(), ObjectZone::Exile);
    let lost = lose_monarch(left);
    assert_eq!(physical(&lost, 3).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&lost, 3).controller(), Some(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_feast_of_succession_applies_minus_four_to_all_creatures_until_cleanup_and_makes_the_caster_monarch()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("Feast of Succession", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 4, &[]),
        creature(3, B, 5, 5, &[]),
        creature(4, A, 7, 8, &[]),
    ]);
    let price = [(Colourless, 4), (Black, 2)];
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let done = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    assert_eq!(done.execution().monarch(), Some(A));
    assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(A));
    assert_size(&done, physical(&done, 3).id(), 1, 1);
    assert_size(&done, physical(&done, 4).id(), 3, 4);
    let later = exile_hand::until(done, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_size(&later, physical(&later, 3).id(), 5, 5);
    assert_size(&later, physical(&later, 4).id(), 7, 8);
    assert_eq!(later.execution().monarch(), Some(A));
}

fn partners_with_campfire() -> GameState {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let mut rock = Program::artifact(Some(ManaCost::free()), vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 10.into()), (White, 2.into())].into()),
    });
    let start = partner_mana_game(
        "Campfire",
        vec![
            seed(2, A, ObjectDestination::LibraryBottom, rock),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    2.into(),
                    Default::default(),
                ),
            ),
        ],
    );
    let ready = round(cast_untargeted(&start, 2, &[]).into_state()).into_state();
    command(
        &ready,
        A,
        GameCommand::ActivateMana {
            source: physical(&ready, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_campfire_gains_life_or_exiles_itself_to_recover_both_owned_partners_then_shuffles_the_rest_of_the_graveyard()
 {
    use spellwake_engine::effects::ZoneOptionalAnswer;
    let funded = partners_with_campfire();
    let field = round(cast_untargeted(&funded, 1, &[(Colourless, 1)]).into_state()).into_state();
    let gain = command(&field, A, activation(&field, 1, 0, &[(Colourless, 1)])).into_state();
    assert_eq!(physical(&gain, 1).tapped(), Some(true));
    let gained = round(gain).into_state();
    assert_eq!(gained.execution().life_total(A), Some(&42.into()));
    let partner = round(cast_untargeted(&field, 99, &[(Colourless, 1), (White, 1)]).into_state())
        .into_state();
    let dead = round(
        targeted(
            &partner,
            A,
            3,
            TargetRef::Object(physical(&partner, 99).id()),
        )
        .into_state(),
    )
    .into_state();
    let yard = reply(&dead, DecisionReply::ReturnCommanders(vec![])).into_state();
    assert_eq!(physical(&yard, 99).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&yard, 100).zone(), ObjectZone::Command);
    let paid = command(&yard, A, activation(&yard, 1, 1, &[(Colourless, 2)])).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Exile);
    let ready = paid
        .supply_entropy(paid.basis(), vec![0; 128])
        .unwrap()
        .into_state();
    let mut done = round(ready).into_state();
    while let Some(RuleFrame::ZoneReplacement(frame)) =
        done.pending_decision().map(|pending| pending.frame())
    {
        let answer = match frame.request() {
            ZoneReplacementRequest::Select { applications } => {
                ZoneReplacementReply::Select(applications[0])
            }
            ZoneReplacementRequest::ConfirmOptional { .. } => {
                ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline)
            }
            other => panic!("unexpected commander replacement {other:?}"),
        };
        done = reply(&done, DecisionReply::ZoneReplacement(answer)).into_state();
    }
    for partner in [99, 100] {
        assert_eq!(physical(&done, partner).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(&done, 200).zone(), ObjectZone::Command);
    assert_eq!(physical(&done, 3).zone(), ObjectZone::Library(A));
    assert!(done.execution().window().cards().graveyard(A).is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_keleth_puts_a_counter_on_each_attacking_commander_in_a_partner_game_and_skips_the_ordinary_attacker()
 {
    let mut field = partners_with_campfire();
    for (n, cost) in [
        (4, vec![]),
        (99, vec![(Colourless, 1), (White, 1)]),
        (100, vec![(Colourless, 5)]),
    ] {
        field = round(cast_untargeted(&field, n, &cost).into_state()).into_state();
    }
    let attack = exile_hand::until(field, |s| {
        s.turn().unwrap().position().number == 3
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let mut declared = reply(
        &attack,
        DecisionReply::DeclareAttackers(
            [4, 99, 100]
                .into_iter()
                .map(|n| AttackDeclaration {
                    attacker: physical(&attack, n).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    )
    .into_state();
    if let Some(RuleFrame::Triggers(order)) = declared.pending_decision().map(|p| p.frame()) {
        declared = reply(
            &declared,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        )
        .into_state();
    }
    assert_eq!(declared.execution().window().entries().len(), 2);
    let done = round(round(declared).into_state()).into_state();
    for (n, counters) in [(4, 0), (99, 1), (100, 1)] {
        assert_eq!(
            sheet(&done, physical(&done, n).id()).plus_one_counters,
            counters.into()
        );
    }
}
