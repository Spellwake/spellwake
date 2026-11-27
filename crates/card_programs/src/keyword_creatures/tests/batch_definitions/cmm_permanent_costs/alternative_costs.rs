//! Whole-card scenarios for printed alternative prices and cost reductions.
use super::*;
use spellwake_engine::mana::{
    CastResource, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput,
};

fn alternative(
    state: &GameState,
    id: u64,
    target: Option<TargetRef>,
    mana: &[(ManaType, i64)],
    objects: Vec<CastResource>,
) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, id).id(),
        target,
        payment: CastingPayment {
            use_alternative: true,
            mana: (!mana.is_empty()).then(|| spend(mana)),
            objects,
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zahid_and_sephara_offer_their_printed_price_or_require_the_right_number_of_untapped_controlled_permanents()
 {
    use spellwake_engine::characteristics::KeywordAbility::{Flying, Indestructible, Lifelink};
    for (name, angel) in [
        ("Zahid, Djinn of the Lamp", false),
        ("Sephara, Sky's Blade", true),
    ] {
        let mut cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[Flying]),
            creature(3, A, 1, 1, &[Flying]),
            creature(4, A, 1, 1, &[Flying]),
            creature(5, A, 1, 1, &[Flying]),
            creature(6, A, 1, 1, &[]),
            creature(7, B, 1, 1, &[Flying]),
            seed(
                8,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
            seed(
                9,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(None, vec![]),
            ),
            remove_spell(10, A),
        ];
        cards.extend(draw_pile());
        let price = if angel {
            vec![(Colourless, 4), (White, 3)]
        } else {
            vec![(Colourless, 4), (Blue, 2)]
        };
        let reduced = if angel {
            vec![(White, 1)]
        } else {
            vec![(Colourless, 3), (Blue, 1)]
        };
        let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let ordinary = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
        for id in [2, 3, 4, 5, 8] {
            assert_eq!(physical(&ordinary, id).tapped(), Some(false));
        }
        let cost = |ids: &[u64]| {
            alternative(
                &start,
                1,
                None,
                &reduced,
                ids.iter()
                    .map(|&n| CastResource::TapForAlternative(physical(&start, n).id()))
                    .collect(),
            )
        };
        illegal(&start, A, cost(&[]));
        let legal = if angel { vec![2, 3, 4, 5] } else { vec![8] };
        for bad in if angel {
            vec![vec![2, 3, 4, 7], vec![2, 3, 4, 6], vec![2, 3, 4, 4]]
        } else {
            vec![vec![9], vec![2], vec![8, 9]]
        } {
            illegal(&start, A, cost(&bad));
        }
        let tapped = command(&start, A, cost(&legal)).into_state();
        for &id in &legal {
            assert_eq!(physical(&tapped, id).tapped(), Some(true));
        }
        let entered = round(tapped).into_state();
        let source = physical(&entered, 1).id();
        assert!(sheet(&entered, source).has_keyword(Flying));
        if angel {
            assert!(sheet(&entered, source).has_keyword(Lifelink));
            assert!(!sheet(&entered, source).has_keyword(Indestructible));
            assert!(!sheet(&entered, physical(&entered, 6).id()).has_keyword(Indestructible));
            assert!(!sheet(&entered, physical(&entered, 7).id()).has_keyword(Indestructible));
            for id in [2, 3, 4, 5] {
                assert!(sheet(&entered, physical(&entered, id).id()).has_keyword(Indestructible));
            }
            let spared = round(
                targeted(
                    &entered,
                    A,
                    10,
                    TargetRef::Object(physical(&entered, 2).id()),
                )
                .into_state(),
            )
            .into_state();
            assert_eq!(physical(&spared, 2).zone(), ObjectZone::Battlefield);
            let killed = round(targeted(&entered, A, 10, TargetRef::Object(source)).into_state())
                .into_state();
            assert_eq!(physical(&killed, 1).zone(), ObjectZone::Graveyard(A));
            assert!(!sheet(&killed, physical(&killed, 2).id()).has_keyword(Indestructible));
        }
    }
}

pub(super) fn conditional_commander_game(
    name: &str,
    extra: Vec<SeedCard>,
) -> (GameState, GameState) {
    let mut rock = Program::artifact(Some(ManaCost::free()), vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed(
            [
                (Colourless, 9.into()),
                (White, 3.into()),
                (Blue, 3.into()),
                (Black, 3.into()),
                (Red, 3.into()),
                (Green, 3.into()),
            ]
            .into(),
        ),
    });
    let mut cards = vec![
        named(name, 1, A, ObjectDestination::LibraryBottom),
        seed(2, A, ObjectDestination::LibraryBottom, rock),
        named("The Ur-Dragon", 100, A, ObjectDestination::Command),
        named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
    ];
    cards.extend(extra);
    let start = commander_opening(cards, None);
    let rock = round(cast_untargeted(&start, 2, &[]).into_state()).into_state();
    let funded = command(
        &rock,
        A,
        GameCommand::ActivateMana {
            source: physical(&rock, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let commander = round(
        cast_untargeted(
            &funded,
            100,
            &[
                (Colourless, 4),
                (White, 1),
                (Blue, 1),
                (Black, 1),
                (Red, 1),
                (Green, 1),
            ],
        )
        .into_state(),
    )
    .into_state();
    (funded, commander)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_deadly_rollick_and_flawless_maneuver_pay_the_printed_price_or_use_a_controlled_commander() {
    use spellwake_engine::characteristics::KeywordAbility::Indestructible;
    for (name, exile, cost) in [
        ("Deadly Rollick", true, vec![(Colourless, 3), (Black, 1)]),
        (
            "Flawless Maneuver",
            false,
            vec![(Colourless, 2), (White, 1)],
        ),
    ] {
        let (waiting, field) = conditional_commander_game(
            name,
            vec![
                seed(
                    3,
                    A,
                    ObjectDestination::LibraryBottom,
                    Program::creature(
                        Some(ManaCost::free()),
                        1.into(),
                        1.into(),
                        Default::default(),
                    ),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::LibraryBottom,
                    free_instant(
                        BodyTarget::SingleCreature,
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
        );
        for (base, has_commander) in [(waiting, false), (field, true)] {
            let ready = round(cast_untargeted(&base, 3, &[]).into_state()).into_state();
            let target = exile.then(|| TargetRef::Object(physical(&ready, 3).id()));
            let free = alternative(&ready, 1, target, &[], vec![]);
            if !has_commander {
                illegal(&ready, A, free.clone());
            }
            for gratis in [false, true]
                .into_iter()
                .filter(|free| !*free || has_commander)
            {
                let request = if gratis {
                    free.clone()
                } else {
                    GameCommand::CastWithResources {
                        spell: physical(&ready, 1).id(),
                        target,
                        payment: CastingPayment {
                            mana: Some(spend(&cost)),
                            ..Default::default()
                        },
                    }
                };
                let paid = command(&ready, A, request).into_state();
                for &(color, n) in &cost {
                    assert_eq!(
                        paid.execution().mana(A, color),
                        ready.execution().mana(A, color).minus(&if gratis {
                            0.into()
                        } else {
                            n.into()
                        })
                    );
                }
                let done = round(paid).into_state();
                if exile {
                    assert_eq!(physical(&done, 3).zone(), ObjectZone::Exile);
                } else {
                    assert!(sheet(&done, physical(&done, 3).id()).has_keyword(Indestructible));
                    if has_commander {
                        assert!(
                            sheet(&done, physical(&done, 100).id()).has_keyword(Indestructible)
                        );
                    }
                    let spared = round(
                        targeted(&done, A, 4, TargetRef::Object(physical(&done, 3).id()))
                            .into_state(),
                    )
                    .into_state();
                    assert_eq!(physical(&spared, 3).zone(), ObjectZone::Battlefield);
                    let expired = exile_hand::until(spared, |s| {
                        s.turn().unwrap().position().number == 2
                            && s.turn().unwrap().position().step == Step::PrecombatMain
                    });
                    assert!(
                        !sheet(&expired, physical(&expired, 3).id()).has_keyword(Indestructible)
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fierce_guardianship_counters_only_noncreature_spells_at_the_printed_or_commander_price() {
    let (waiting, field) = conditional_commander_game(
        "Fierce Guardianship",
        vec![
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(Some(ManaCost::free()), vec![]),
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
    for (base, has_commander) in [(waiting, false), (field, true)] {
        for n in [3, 4] {
            let target = cast_untargeted(&base, n, &[]).into_state();
            let entry = Some(TargetRef::Stack(
                target.execution().window().entries().last().unwrap().id(),
            ));
            let free = alternative(&target, 1, entry, &[], vec![]);
            let normal = GameCommand::CastWithResources {
                spell: physical(&target, 1).id(),
                target: entry,
                payment: CastingPayment {
                    mana: Some(spend(&[(Blue, 1), (Colourless, 2)])),
                    ..Default::default()
                },
            };
            if n == 4 {
                illegal(&target, A, free);
                illegal(&target, A, normal);
                continue;
            }
            if !has_commander {
                illegal(&target, A, free.clone());
            }
            for (request, gratis) in [(normal, false), (free, true)]
                .into_iter()
                .filter(|(_, free)| !*free || has_commander)
            {
                let paid = command(&target, A, request).into_state();
                assert_eq!(
                    paid.execution().mana(A, Blue),
                    target
                        .execution()
                        .mana(A, Blue)
                        .minus(&i64::from(!gratis).into())
                );
                let done = round(paid).into_state();
                assert_eq!(physical(&done, n).zone(), ObjectZone::Graveyard(A));
                assert!(done.execution().window().entries().is_empty());
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_deflecting_swat_can_retain_or_change_a_spell_or_an_ability_target_and_use_the_commander_price()
 {
    use spellwake_engine::effects::CopyTargetChoice;
    let bolt = ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                spellwake_engine::program::ValueExpr::literal(1),
                false,
            ),
        ],
    );
    let mut pinger = Program::artifact(Some(ManaCost::free()), vec![]);
    pinger.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: bolt.clone(),
    });
    let (waiting, field) = conditional_commander_game(
        "Deflecting Swat",
        vec![
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::instant(ManaCost::free(), bolt),
            ),
            seed(4, A, ObjectDestination::LibraryBottom, pinger),
        ],
    );
    for (base, has_commander) in [(waiting, false), (field, true)] {
        for ability in [false, true] {
            let ready = if ability {
                let pinger = round(cast_untargeted(&base, 4, &[]).into_state()).into_state();
                command(
                    &pinger,
                    A,
                    GameCommand::ActivateAbility {
                        source: physical(&pinger, 4).id(),
                        ability: ActivatedAbilityId(0),
                        target: TargetRef::Player(B),
                        payment: None,
                    },
                )
                .into_state()
            } else {
                targeted(&base, A, 3, TargetRef::Player(B)).into_state()
            };
            let target = Some(TargetRef::Stack(
                ready.execution().window().entries().last().unwrap().id(),
            ));
            let free = alternative(&ready, 1, target, &[], vec![]);
            if !has_commander {
                illegal(&ready, A, free.clone());
            }
            let select = if has_commander {
                free
            } else {
                GameCommand::CastWithResources {
                    spell: physical(&ready, 1).id(),
                    target,
                    payment: CastingPayment {
                        mana: Some(spend(&[(Colourless, 2), (Red, 1)])),
                        ..Default::default()
                    },
                }
            };
            let chose = round(command(&ready, A, select).into_state()).into_state();
            for (choice, victim) in [
                (CopyTargetChoice::Retain, B),
                (CopyTargetChoice::Change(TargetRef::Player(A)), A),
            ] {
                let redirected = reply(&chose, DecisionReply::CopyTargets(choice)).into_state();
                let done = round(redirected).into_state();
                for player in [A, B] {
                    assert_eq!(
                        done.execution().life_total(player),
                        Some(&(40 - i64::from(victim == player)).into())
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_darksteel_monolith_survives_destruction_and_pays_zero_for_only_one_colorless_hand_spell_each_turn()
 {
    let life = || {
        Program::instant(
            ManaCost::generic(3.into()),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ],
            ),
        )
    };
    let mut colored = life();
    colored.color_indicator = Some([Color::Red].into());
    let start = game(
        vec![A, B],
        vec![
            named("Darksteel Monolith", 1, A, ObjectDestination::Hand),
            seed(2, A, ObjectDestination::Hand, life()),
            seed(3, A, ObjectDestination::Hand, life()),
            seed(4, A, ObjectDestination::Hand, colored),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePermanent,
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
            seed(6, A, ObjectDestination::Graveyard, life()),
        ],
        &[(Colourless, 8)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &[(Colourless, 8)]).into_state()).into_state();
    let ready = round(
        targeted(
            &entered,
            A,
            5,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(physical(&ready, 1).zone(), ObjectZone::Battlefield);
    let mono = physical(&ready, 1).id();
    let free = |state: &GameState, n| GameCommand::CastWithResources {
        spell: physical(state, n).id(),
        target: None,
        payment: CastingPayment {
            alternative_cost_source: Some(mono),
            ..Default::default()
        },
    };
    illegal(&ready, A, free(&ready, 4));
    illegal(&ready, A, free(&ready, 6));
    let once = round(command(&ready, A, free(&ready, 2)).into_state()).into_state();
    assert_eq!(once.execution().life_total(A), Some(&21.into()));
    illegal(&once, A, free(&once, 3));
    let next = exile_hand::until(once, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let priority = pass(&next).into_state();
    let again = round(command(&priority, A, free(&priority, 3)).into_state()).into_state();
    assert_eq!(again.execution().life_total(A), Some(&22.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_obscuring_haze_prevents_damage_only_from_opposing_creatures_and_only_during_the_current_turn()
 {
    let bolt = ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                spellwake_engine::program::ValueExpr::literal(2),
                false,
            ),
        ],
    );
    let mut pinger = Program::creature(
        Some(ManaCost::free()),
        1.into(),
        1.into(),
        Default::default(),
    );
    pinger.printed_keywords.insert(KeywordAbility::Flash);
    pinger.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: bolt.clone(),
    });
    let (waiting, field) = conditional_commander_game(
        "Obscuring Haze",
        vec![
            seed(3, A, ObjectDestination::LibraryBottom, pinger.clone()),
            seed(103, B, ObjectDestination::LibraryBottom, pinger),
            seed(
                104,
                B,
                ObjectDestination::LibraryBottom,
                Program::instant(ManaCost::free(), bolt),
            ),
        ],
    );
    for (base, has_commander) in [(waiting, false), (field, true)] {
        let ours = round(cast_untargeted(&base, 3, &[]).into_state()).into_state();
        let priority = pass(&ours).into_state();
        let both = round(
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
        let free = alternative(&both, 1, None, &[], vec![]);
        let request = if has_commander {
            free
        } else {
            illegal(&both, A, free);
            GameCommand::CastUntargeted {
                spell: physical(&both, 1).id(),
                payment: Some(spend(&[(Colourless, 2), (Green, 1)])),
            }
        };
        let fog = round(command(&both, A, request).into_state()).into_state();
        let ping = |s: &GameState, n, victim| GameCommand::ActivateAbility {
            source: physical(s, n).id(),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Player(victim),
            payment: None,
        };
        let friendly = round(command(&fog, A, ping(&fog, 3, B)).into_state()).into_state();
        assert_eq!(friendly.execution().life_total(B), Some(&38.into()));
        let opponent = pass(&fog).into_state();
        for victim in [A, B] {
            let blocked = round(command(&opponent, B, ping(&opponent, 103, victim)).into_state())
                .into_state();
            assert_eq!(blocked.execution().life_total(victim), Some(&40.into()));
        }
        let instant =
            round(targeted(&opponent, B, 104, TargetRef::Player(A)).into_state()).into_state();
        assert_eq!(instant.execution().life_total(A), Some(&38.into()));
        let future = exile_hand::until(fog, |s| {
            s.turn().unwrap().position().number == 2
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let hit = round(command(&future, B, ping(&future, 103, A)).into_state()).into_state();
        assert_eq!(hit.execution().life_total(A), Some(&38.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ghalta_uses_total_controlled_power_to_reduce_only_its_generic_cost() {
    for (power, generic) in [(0, 10), (5, 3), (13, 0)] {
        let mut cards = vec![
            named("Ghalta, Primal Hunger", 1, A, ObjectDestination::Hand),
            creature(4, B, 12, 12, &[]),
        ];
        if power > 0 {
            cards.extend([creature(2, A, power, 2, &[]), creature(3, A, 2, 2, &[])]);
        }
        let start = game(
            vec![A, B],
            cards,
            &[(Colourless, 10), (Green, 2)],
            Step::PrecombatMain,
        );
        let price = if generic > 0 {
            vec![(Colourless, generic), (Green, 2)]
        } else {
            vec![(Green, 2)]
        };
        illegal(
            &start,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&start, 1).id(),
                payment: Some(spend(&[(Green, 1)])),
            },
        );
        let done = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
        assert_eq!(done.execution().mana(A, Colourless), (10 - generic).into());
        assert_eq!(done.execution().mana(A, Green), 0.into());
        assert_size(&done, physical(&done, 1).id(), 12, 12);
        assert!(sheet(&done, physical(&done, 1).id()).has_keyword(KeywordAbility::Trample));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_metalwork_colossus_counts_noncreature_artifact_mana_values_and_can_sacrifice_two_artifacts_from_the_graveyard()
 {
    let start = game(
        vec![A, B],
        vec![
            named("Metalwork Colossus", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(fixed_cost("{2}"), vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(fixed_cost("{3}"), vec![]),
            ),
            named(
                "Suspicious Bookcase",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(fixed_cost("{4}"), vec![]),
            ),
            remove_spell(6, A),
            creature(7, A, 1, 1, &[]),
        ],
        &[(Colourless, 6)],
        Step::PrecombatMain,
    );
    illegal(
        &start,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&start, 1).id(),
            payment: Some(spend(&[(Colourless, 4)])),
        },
    );
    let cast = round(cast_untargeted(&start, 1, &[(Colourless, 6)]).into_state()).into_state();
    assert_size(&cast, physical(&cast, 1).id(), 10, 10);
    let killed =
        round(targeted(&cast, A, 6, TargetRef::Object(physical(&cast, 1).id())).into_state())
            .into_state();
    let cost = |objects: &[u64]| {
        selected_cost(
            &killed,
            1,
            0,
            objects.iter().map(|&n| physical(&killed, n).id()).collect(),
            None,
            &[],
        )
    };
    for bad in [vec![2], vec![2, 2], vec![2, 5], vec![2, 7]] {
        illegal(&killed, A, cost(&bad));
    }
    let choose = command(&killed, A, cost(&[2, 4])).into_state();
    let RuleFrame::LinkedReturnOrder(frame) = choose.pending_decision().unwrap().frame() else {
        panic!("order sacrificed artifacts");
    };
    let paid = reply(
        &choose,
        DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
    )
    .into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    for n in [2, 4] {
        assert_eq!(physical(&paid, n).zone(), ObjectZone::Graveyard(A));
    }
    let returned = round(paid).into_state();
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_karador_reduces_its_cost_for_own_graveyard_creatures_and_allows_one_creature_cast_each_own_turn()
 {
    let mut flash = Program::creature(
        Some(ManaCost::free()),
        1.into(),
        1.into(),
        Default::default(),
    );
    flash.printed_keywords.insert(KeywordAbility::Flash);
    let mut cards = draw_pile();
    cards.extend([
        named("Karador, Ghost Chieftain", 1, A, ObjectDestination::Hand),
        seed(2, A, ObjectDestination::Graveyard, flash.clone()),
        seed(3, A, ObjectDestination::Graveyard, flash.clone()),
        seed(4, B, ObjectDestination::Graveyard, flash),
        seed(
            5,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 3), (White, 1), (Black, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    illegal(
        &start,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&start, 1).id(),
            payment: Some(spend(&[
                (Colourless, 2),
                (White, 1),
                (Black, 1),
                (Green, 1),
            ])),
        },
    );
    let field = round(
        cast_untargeted(
            &start,
            1,
            &[(Colourless, 3), (White, 1), (Black, 1), (Green, 1)],
        )
        .into_state(),
    )
    .into_state();
    let cast = |state: &GameState, n| GameCommand::CastWithResources {
        spell: physical(state, n).id(),
        target: None,
        payment: CastingPayment {
            graveyard_permission: Some(physical(state, 1).id()),
            ..Default::default()
        },
    };
    for id in [4, 5] {
        illegal(&field, A, cast(&field, id));
    }
    let used = round(command(&field, A, cast(&field, 2)).into_state()).into_state();
    assert_eq!(physical(&used, 2).zone(), ObjectZone::Battlefield);
    illegal(&used, A, cast(&used, 3));
    let theirs = exile_hand::until(used, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let priority = pass(&theirs).into_state();
    illegal(&priority, A, cast(&priority, 3));
    let next = exile_hand::until(priority, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let second = round(command(&next, A, cast(&next, 3)).into_state()).into_state();
    assert_eq!(physical(&second, 3).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_starfield_mystic_and_herald_of_the_pantheon_discount_enchantments_and_trigger_only_at_the_printed_event()
 {
    for (name, mystic) in [
        ("Starfield Mystic", true),
        ("Herald of the Pantheon", false),
    ] {
        let paid_color = if mystic { White } else { Green };
        let mut cards = draw_pile();
        cards.extend([
            named(name, 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::enchantment(fixed_cost("{2}{G}")),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{2}{G}"), vec![]),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePermanent,
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
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
        ]);
        let start = game(
            vec![A, B],
            cards,
            &[
                (Colourless, 3),
                (paid_color, if mystic { 1 } else { 2 }),
                (if mystic { Green } else { White }, 1),
            ],
            Step::PrecombatMain,
        );
        let field =
            round(cast_untargeted(&start, 1, &[(Colourless, 1), (paid_color, 1)]).into_state())
                .into_state();
        illegal(
            &field,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&field, 3).id(),
                payment: Some(spend(&[(Colourless, 1), (Green, 1)])),
            },
        );
        let unaffected =
            round(cast_untargeted(&field, 3, &[(Colourless, 2), (Green, 1)]).into_state())
                .into_state();
        assert!(unaffected.execution().window().entries().is_empty());
        let mut enchanted = cast_untargeted(&field, 2, &[(Colourless, 1), (Green, 1)]).into_state();
        if !mystic {
            assert_eq!(enchanted.execution().window().entries().len(), 2);
            enchanted = round(enchanted).into_state();
            assert_eq!(enchanted.execution().life_total(A), Some(&21.into()));
            assert_eq!(physical(&enchanted, 2).zone(), ObjectZone::Stack);
        }
        let resolved = round(enchanted).into_state();
        assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
        for (n, own) in [(2, true), (5, false)] {
            let loss = round(
                targeted(
                    &resolved,
                    A,
                    4,
                    TargetRef::Object(physical(&resolved, n).id()),
                )
                .into_state(),
            )
            .into_state();
            let applies = mystic && own;
            assert_eq!(
                loss.execution().window().entries().len(),
                usize::from(applies)
            );
            let done = if applies {
                round(loss).into_state()
            } else {
                loss
            };
            assert_eq!(
                sheet(&done, physical(&done, 1).id()).plus_one_counters,
                i64::from(applies).into()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sythis_triggers_when_its_second_copy_is_cast_then_lets_its_controller_keep_either_legend() {
    use spellwake_engine::checks::CheckDecision;
    let mut cards = draw_pile();
    cards.extend([
        named("Sythis, Harvest's Hand", 1, A, ObjectDestination::Hand),
        named("Sythis, Harvest's Hand", 2, A, ObjectDestination::Hand),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Green, 2), (White, 2)],
        Step::PrecombatMain,
    );
    let first =
        round(cast_untargeted(&start, 1, &[(Green, 1), (White, 1)]).into_state()).into_state();
    assert_eq!(first.execution().life_total(A), Some(&20.into()));
    let ordinary = round(cast_untargeted(&first, 3, &[]).into_state()).into_state();
    assert!(ordinary.execution().window().entries().is_empty());
    let stack = cast_untargeted(&first, 2, &[(Green, 1), (White, 1)]).into_state();
    assert_eq!(stack.execution().window().entries().len(), 2);
    let hand = hand_size(&stack, A);
    let trigger = round(stack).into_state();
    assert_eq!(trigger.execution().life_total(A), Some(&21.into()));
    assert_eq!(hand_size(&trigger, A), hand + 1);
    let offered = round(trigger).into_state();
    let RuleFrame::Checks(CheckDecision::Legend(frame)) =
        offered.pending_decision().unwrap().frame()
    else {
        panic!("two Sythis permanents");
    };
    let [conflict] = frame.conflicts() else {
        panic!("one legend name");
    };
    for (keep, lose) in [(1, 2), (2, 1)] {
        let done = reply(
            &offered,
            DecisionReply::KeepLegend {
                conflict: conflict.id(),
                keep: physical(&offered, keep).id(),
            },
        )
        .into_state();
        assert_eq!(physical(&done, keep).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&done, lose).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_morophon_reduces_each_colored_symbol_once_and_buffs_only_other_controlled_creatures_of_the_selected_type()
 {
    use spellwake_engine::land::EntryChoice;
    let vocab = current_catalogue().1;
    let elf = vocab.resolve_label("Elf").unwrap();
    let dragon = vocab.resolve_label("Dragon").unwrap();
    let elf_spell = Program::creature(
        fixed_cost("{2}{W}{U}{B}{R}{G}{G}"),
        1.into(),
        2.into(),
        [elf].into(),
    );
    let start = game(
        vec![A, B],
        vec![
            named("Morophon, the Boundless", 1, A, ObjectDestination::Hand),
            seed(2, A, ObjectDestination::Hand, elf_spell.clone()),
            seed(
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
                elf_spell.clone(),
            ),
            seed(
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
                elf_spell,
            ),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{2}{G}"), 1.into(), 1.into(), [dragon].into()),
            ),
        ],
        &[(Colourless, 9), (Green, 1)],
        Step::PrecombatMain,
    );
    let ask = round(cast_untargeted(&start, 1, &[(Colourless, 7)]).into_state()).into_state();
    let field = reply(
        &ask,
        DecisionReply::EntryChoice(EntryChoice::CreatureType(elf)),
    )
    .into_state();
    let source = physical(&field, 1).id();
    let morph = sheet(&field, source);
    assert!(
        morph.current_creature_subtypes.contains(&elf)
            && morph.current_creature_subtypes.contains(&dragon)
    );
    assert_size(&field, source, 6, 6);
    assert_size(&field, physical(&field, 3).id(), 1, 2);
    assert_size(&field, physical(&field, 4).id(), 2, 3);
    for id in [2, 5] {
        illegal(
            &field,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&field, id).id(),
                payment: Some(spend(&[(Colourless, 2)])),
            },
        );
    }
    let elf_cast =
        round(cast_untargeted(&field, 2, &[(Colourless, 2), (Green, 1)]).into_state()).into_state();
    assert_size(&elf_cast, physical(&elf_cast, 2).id(), 2, 3);
    let dragon_cast =
        round(cast_untargeted(&field, 5, &[(Colourless, 2), (Green, 1)]).into_state()).into_state();
    assert_size(&dragon_cast, physical(&dragon_cast, 5).id(), 1, 1);
}

fn top_target(state: &GameState) -> TargetRef {
    TargetRef::Stack(state.execution().window().entries().last().unwrap().id())
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_imps_mischief_requires_a_single_target_changes_it_and_costs_life_equal_to_the_original_spells_mana_value()
 {
    use spellwake_engine::effects::CopyTargetChoice;
    let original = Program::instant(
        ManaCost::generic(3.into()),
        ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(2),
                    false,
                ),
            ],
        ),
    );
    let opening = game(
        vec![A, B],
        vec![
            named("Imp's Mischief", 1, A, ObjectDestination::Hand),
            seed(2, A, ObjectDestination::Hand, original),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ],
        &[(Colourless, 4), (Black, 1)],
        Step::PrecombatMain,
    );
    let blank = cast_untargeted(&opening, 3, &[]).into_state();
    let cast = |s: &GameState, id, aim, price: &[(ManaType, i64)]| GameCommand::CastWithResources {
        spell: physical(s, id).id(),
        target: Some(aim),
        payment: CastingPayment {
            mana: Some(spend(price)),
            ..Default::default()
        },
    };
    illegal(
        &blank,
        A,
        cast(
            &blank,
            1,
            top_target(&blank),
            &[(Colourless, 1), (Black, 1)],
        ),
    );
    let stack = command(
        &opening,
        A,
        cast(&opening, 2, TargetRef::Player(A), &[(Colourless, 3)]),
    )
    .into_state();
    let offer = round(
        command(
            &stack,
            A,
            cast(
                &stack,
                1,
                top_target(&stack),
                &[(Colourless, 1), (Black, 1)],
            ),
        )
        .into_state(),
    )
    .into_state();
    for bad in [
        CopyTargetChoice::Retain,
        CopyTargetChoice::Change(TargetRef::Player(A)),
    ] {
        illegal(
            &offer,
            A,
            GameCommand::Reply {
                decision: offer.pending_decision().unwrap().id(),
                choice: DecisionReply::CopyTargets(bad),
            },
        );
    }
    let changed = reply(
        &offer,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(B))),
    )
    .into_state();
    assert_eq!(changed.execution().life_total(A), Some(&17.into()));
    let done = round(changed).into_state();
    assert_eq!(done.execution().life_total(B), Some(&18.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_not_of_this_world_counters_a_spell_or_ability_targeting_an_own_permanent_and_is_free_at_seven_power()
 {
    let body = ExecutableBody::new(
        BodyTarget::SinglePermanent,
        vec![
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
        ],
    );
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(0),
        source_cost: SourceCost::None,
        mana_cost: ManaCost::free(),
        body: body.clone(),
    };
    let start = game(
        vec![A, B],
        vec![
            named("Not of This World", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::instant(ManaCost::free(), body),
            ),
            seed(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![ability]),
            ),
            creature(4, A, 6, 6, &[]),
            creature(5, A, 7, 2, &[]),
            creature(6, B, 7, 4, &[]),
            named(
                "Island",
                7,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 7)],
        Step::PrecombatMain,
    );
    for is_spell in [true, false] {
        for member in [4, 5, 6, 7] {
            let aim = TargetRef::Object(physical(&start, member).id());
            let stack = if is_spell {
                targeted(&start, A, 2, aim).into_state()
            } else {
                command(
                    &start,
                    A,
                    GameCommand::ActivateAbility {
                        source: physical(&start, 3).id(),
                        ability: ActivatedAbilityId(0),
                        target: aim,
                        payment: None,
                    },
                )
                .into_state()
            };
            let cancel = |price| GameCommand::CastWithResources {
                spell: physical(&stack, 1).id(),
                target: Some(top_target(&stack)),
                payment: CastingPayment {
                    mana: price,
                    ..Default::default()
                },
            };
            if member == 6 {
                illegal(&stack, A, cancel(Some(spend(&[(Colourless, 7)]))));
                continue;
            }
            let free = member == 5;
            if !free {
                illegal(&stack, A, cancel(None));
            }
            let done = round(
                command(
                    &stack,
                    A,
                    cancel((!free).then(|| spend(&[(Colourless, 7)]))),
                )
                .into_state(),
            )
            .into_state();
            assert!(done.execution().window().entries().is_empty());
            assert_eq!(physical(&done, member).zone(), ObjectZone::Battlefield);
            assert_eq!(
                done.execution().mana(A, Colourless),
                if free { 7.into() } else { 0.into() }
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kozilek_the_great_distortion_draws_to_seven_on_cast_and_counters_only_by_discarding_a_card_of_matching_mana_value()
 {
    use spellwake_engine::characteristics::KeywordAbility::Menace;
    for full in [false, true] {
        let mut cards = draw_pile();
        cards.extend([
            named(
                "Kozilek, the Great Distortion",
                1,
                A,
                ObjectDestination::Hand,
            ),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{2}"), vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{2}"), vec![]),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ]);
        if full {
            cards.extend((20..24).map(|n| {
                seed(
                    n,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                )
            }));
        }
        let start = game(vec![A, B], cards, &[(Colourless, 12)], Step::PrecombatMain);
        let stack = cast_untargeted(&start, 1, &[(Colourless, 10)]).into_state();
        assert_eq!(
            stack.execution().window().entries().len(),
            if full { 1 } else { 2 }
        );
        let seven = if full {
            stack
        } else {
            round(stack).into_state()
        };
        assert_eq!(hand_size(&seven, A), 7);
        let field = round(seven).into_state();
        assert!(sheet(&field, physical(&field, 1).id()).has_keyword(Menace));
        let spell = cast_untargeted(&field, 2, &[(Colourless, 2)]).into_state();
        let counter = |card| GameCommand::ActivateWithCostObjects {
            source: physical(&spell, 1).id(),
            ability: ActivatedAbilityId(0),
            granting_effect: None,
            target: Some(top_target(&spell)),
            objects: vec![physical(&spell, card).id()],
            payment: None,
        };
        illegal(&spell, A, counter(4));
        let paid = command(&spell, A, counter(3)).into_state();
        assert_eq!(physical(&paid, 3).zone(), ObjectZone::Graveyard(A));
        let done = round(paid).into_state();
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(A));
        assert!(done.execution().window().entries().is_empty());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_commandeer_pays_the_normal_price_or_exiles_two_blue_hand_cards_takes_control_and_can_retarget_a_noncreature_spell()
 {
    use spellwake_engine::characteristics::KeywordAbility::Flash;
    use spellwake_engine::effects::CopyTargetChoice;
    let mut artifact = Program::artifact(Some(ManaCost::free()), vec![]);
    artifact.printed_keywords.insert(Flash);
    let mut creature = Program::creature(
        Some(ManaCost::free()),
        1.into(),
        1.into(),
        Default::default(),
    );
    creature.printed_keywords.insert(Flash);
    let opening = game(
        vec![A, B],
        vec![
            named("Commandeer", 1, A, ObjectDestination::Hand),
            seed(2, B, ObjectDestination::Hand, artifact),
            seed(
                3,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SinglePlayer,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(2),
                        false,
                    ),
                ),
            ),
            seed(4, B, ObjectDestination::Hand, creature),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{U}"), 1.into(), 1.into(), Default::default()),
            ),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{U}"), 1.into(), 1.into(), Default::default()),
            ),
            seed(
                8,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{R}"), 1.into(), 1.into(), Default::default()),
            ),
        ],
        &[(Colourless, 5), (Blue, 2)],
        Step::PrecombatMain,
    );
    let other = pass(&opening).into_state();
    for n in [2, 3, 4] {
        let cast = if n == 3 {
            targeted(&other, B, 3, TargetRef::Player(A)).into_state()
        } else {
            command(
                &other,
                B,
                GameCommand::CastUntargeted {
                    spell: physical(&other, n).id(),
                    payment: None,
                },
            )
            .into_state()
        };
        let priority = pass(&cast).into_state();
        let target = Some(top_target(&priority));
        let alternate = |cards: &[u64]| {
            alternative(
                &priority,
                1,
                target,
                &[],
                cards
                    .iter()
                    .map(|id| CastResource::ExileHandForAlternative(physical(&priority, *id).id()))
                    .collect(),
            )
        };
        let printed = GameCommand::CastWithResources {
            spell: physical(&priority, 1).id(),
            target,
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, 5), (Blue, 2)])),
                ..Default::default()
            },
        };
        if n == 4 {
            illegal(&priority, A, printed);
            illegal(&priority, A, alternate(&[6, 7]));
            continue;
        }
        illegal(&priority, A, alternate(&[6]));
        illegal(&priority, A, alternate(&[6, 8]));
        for (mode, free) in [(printed, false), (alternate(&[6, 7]), true)] {
            let paid = command(&priority, A, mode).into_state();
            for id in [6, 7] {
                assert_eq!(
                    physical(&paid, id).zone(),
                    if free {
                        ObjectZone::Exile
                    } else {
                        ObjectZone::Hand(A)
                    }
                )
            }
            let taken = round(paid).into_state();
            if n == 2 {
                let done = round(taken).into_state();
                assert_eq!(physical(&done, 2).controller(), Some(A));
                assert_eq!(physical(&done, 2).owner(), B);
            } else {
                assert_eq!(taken.pending_decision().unwrap().chooser(), A);
                for (choice, hurt) in [
                    (CopyTargetChoice::Retain, A),
                    (CopyTargetChoice::Change(TargetRef::Player(B)), B),
                ] {
                    let retargeted = reply(&taken, DecisionReply::CopyTargets(choice)).into_state();
                    assert_eq!(
                        retargeted
                            .execution()
                            .window()
                            .entries()
                            .last()
                            .unwrap()
                            .controller(),
                        A
                    );
                    let done = round(retargeted).into_state();
                    assert_eq!(done.execution().life_total(hurt), Some(&18.into()));
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_abstruse_archaic_copies_controlled_colorless_activated_and_triggered_abilities_and_can_choose_new_targets()
 {
    use spellwake_engine::{characteristics::KeywordAbility::Vigilance, effects::CopyTargetChoice};
    let ping = ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ],
        ),
    };
    let artifact = Program::artifact(None, vec![ping]);
    let mut colored = artifact.clone();
    colored.effect_defined_colors = Some([Color::Blue].into());
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 8.into())].into()),
    });
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Abstruse Archaic",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named("Meteor Golem", 2, A, ObjectDestination::Hand),
        seed(
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
            artifact.clone(),
        ),
        seed(
            4,
            A,
            ObjectDestination::Battlefield { controller: A },
            colored,
        ),
        seed(
            5,
            B,
            ObjectDestination::Battlefield { controller: B },
            artifact.clone(),
        ),
        seed(6, A, ObjectDestination::Battlefield { controller: A }, rock),
        seed(
            7,
            B,
            ObjectDestination::Battlefield { controller: B },
            artifact,
        ),
    ]);
    let opening = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let main = exile_hand::until(opening, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert!(sheet(&main, physical(&main, 1).id()).has_keyword(Vigilance));
    let field = command(
        &main,
        A,
        GameCommand::ActivateMana {
            source: physical(&main, 6).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let copy = |s: &GameState| GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(0),
        target: top_target(s),
        payment: Some(spend(&[(Colourless, 1)])),
    };
    for (n, actor) in [(3, A), (4, A), (5, B)] {
        let base = if actor == A {
            field.clone()
        } else {
            pass(&field).into_state()
        };
        let stack = command(
            &base,
            actor,
            GameCommand::ActivateAbility {
                source: physical(&base, n).id(),
                ability: ActivatedAbilityId(0),
                target: TargetRef::Player(B),
                payment: None,
            },
        )
        .into_state();
        let ready = if actor == A {
            stack
        } else {
            pass(&stack).into_state()
        };
        if n != 3 {
            illegal(&ready, A, copy(&ready));
            continue;
        }
        let chosen = round(command(&ready, A, copy(&ready)).into_state()).into_state();
        for (change, ka, kb) in [
            (CopyTargetChoice::Retain, 20, 18),
            (CopyTargetChoice::Change(TargetRef::Player(A)), 19, 19),
        ] {
            let copied = reply(&chosen, DecisionReply::CopyTargets(change)).into_state();
            let end = round(round(copied).into_state()).into_state();
            assert_eq!(end.execution().life_total(A), Some(&ka.into()));
            assert_eq!(end.execution().life_total(B), Some(&kb.into()));
        }
    }
    let golem = cast_untargeted(&field, 2, &[(Colourless, 7)]).into_state();
    illegal(&golem, A, copy(&golem));
    let enters = round(golem).into_state();
    let trigger = reply(
        &enters,
        DecisionReply::TriggerTarget(TargetRef::Object(physical(&enters, 5).id())),
    )
    .into_state();
    let selected = round(command(&trigger, A, copy(&trigger)).into_state()).into_state();
    let stack = reply(
        &selected,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(
            physical(&selected, 7).id(),
        ))),
    )
    .into_state();
    let end = round(round(stack).into_state()).into_state();
    for id in [5, 7] {
        assert_eq!(physical(&end, id).zone(), ObjectZone::Graveyard(B));
    }
}
