//! Combat and draw triggers on named creatures, including turn and ownership restrictions.
use super::*;
use spellwake_engine::effects::OptionalEffectChoice;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_murder_of_crows_loots_for_other_deaths_but_never_for_its_own_death() {
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Murder of Crows"),
            creature(2, A, 1, 1, &[]),
            creature(3, B, 1, 1, &[]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
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
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    for n in [1, 2, 3] {
        let dead = round(
            targeted(
                &entered,
                A,
                4,
                TargetRef::Object(physical(&entered, n).id()),
            )
            .into_state(),
        )
        .into_state();
        if n == 1 {
            assert!(dead.execution().window().entries().is_empty());
            continue;
        }
        let offer = round(dead).into_state();
        let declined = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_eq!(
            physical(declined.state(), 100).zone(),
            ObjectZone::Library(A)
        );
        let choice = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        assert_eq!(physical(&choice, 100).zone(), ObjectZone::Hand(A));
        let end = reply(
            &choice,
            DecisionReply::SelectResolution(vec![physical(&choice, 100).id()]),
        );
        assert_eq!(physical(end.state(), 100).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_looter_il_kor_only_blocks_and_is_blocked_by_shadow_and_loots_on_opponent_damage() {
    let price = [(ManaType::Colourless, 1), (ManaType::Blue, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Looter il-Kor"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, B, 1, 1, &[]),
            creature(4, B, 1, 1, &[Shadow]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let defending = blockers(&attack(&entered, 1, B));
    illegal(
        &defending,
        B,
        combat_declaration(&defending, block_choice(&defending, 3, 1)),
    );
    reply(&defending, block_choice(&defending, 4, 1));
    let unblocked = reply(&defending, DecisionReply::DeclareBlockers(vec![])).into_state();
    let damaging = exile_hand::until(unblocked, |s| {
        s.turn().unwrap().position().step == Step::CombatDamage
    });
    let choose = round(damaging).into_state();
    assert_eq!(choose.execution().life_total(B), Some(&19.into()));
    assert_eq!(physical(&choose, 100).zone(), ObjectZone::Hand(A));
    let done = reply(
        &choose,
        DecisionReply::SelectResolution(vec![physical(&choose, 100).id()]),
    )
    .into_state();
    assert_eq!(physical(&done, 100).zone(), ObjectZone::Graveyard(A));
    let opposing = game(
        vec![A, B],
        vec![
            named(
                "Looter il-Kor",
                1,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(2, A, 1, 1, &[Shadow]),
            creature(3, A, 1, 1, &[]),
            named(
                "Fervor",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(5, B, 1, 1, &[]),
        ],
        &[],
        Step::PrecombatMain,
    );
    let declare = combat_frame(&opposing, true);
    let attacks = reply(
        &declare,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .map(|n| AttackDeclaration {
                    attacker: physical(&declare, n).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .to_vec(),
        ),
    )
    .into_state();
    let defend = blockers(&attacks);
    illegal(
        &defend,
        B,
        combat_declaration(&defend, block_choice(&defend, 1, 3)),
    );
    reply(&defend, block_choice(&defend, 1, 2));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_jolrael_rewards_only_the_second_draw_and_sets_allied_base_size_at_resolution_until_cleanup()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1)];
    let draw = || {
        free_instant(
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        )
    };
    let mut cards = vec![
        hand("Jolrael, Mwonvuli Recluse"),
        creature(2, A, 1, 1, &[]),
        creature(3, B, 1, 1, &[]),
    ];
    for n in 4..7 {
        cards.push(seed(n, A, ObjectDestination::Hand, draw()));
    }
    for n in 7..11 {
        cards.push(named("Forest", n, A, ObjectDestination::LibraryBottom));
    }
    let s = game(
        vec![A, B],
        cards,
        &[(Colourless, 5), (Green, 3)],
        Step::PrecombatMain,
    );
    let mut state = cast_named(&s, &price);
    for n in 4..7 {
        let drew = round(cast_untargeted(&state, n, &[]).into_state()).into_state();
        if n == 5 {
            let cat = round(drew);
            let id = tokens(&cat, A, 1)[0];
            let view = cat
                .state()
                .execution()
                .current_characteristics(id)
                .unwrap()
                .unwrap();
            assert_eq!(view.current_colors, [Color::Green].into());
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Cat").unwrap()].into()
            );
            assert_eq!(
                view.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 2.into(),
                    toughness: 2.into()
                }
            );
            state = cat.into_state();
        } else {
            assert!(drew.execution().window().entries().is_empty());
            state = drew;
        }
    }
    let boosted = round(
        command(
            &state,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: physical(&state, 1).id(),
                ability: ActivatedAbilityId(0),
                payment: Some(spend(&[(Colourless, 4), (Green, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let size = |s: &GameState, n| {
        s.execution()
            .power_toughness(physical(s, n).id())
            .unwrap()
            .unwrap()
            .derived
    };
    for n in [1, 2] {
        assert_eq!(
            size(&boosted, n),
            PowerToughness {
                power: 3.into(),
                toughness: 3.into()
            }
        );
    }
    assert_eq!(
        size(&boosted, 3),
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    assert_eq!(
        size(&next_main(boosted, 2), 2),
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_ohran_frostfang_grants_only_attackers_deathtouch_and_draws_per_creature_hitting_a_player()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Green, 2)];
    let entered = cast_named(
        &game(
            vec![A, B],
            vec![
                hand("Ohran Frostfang"),
                creature(2, A, 1, 2, &[]),
                creature(3, A, 1, 2, &[]),
                creature(4, B, 1, 8, &[]),
                named(
                    "Fervor",
                    5,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                named("Island", 6, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        ),
        &price,
    );
    let choose = combat_frame(&entered, true);
    let attacks = reply(
        &choose,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .map(|n| AttackDeclaration {
                    attacker: physical(&choose, n).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .to_vec(),
        ),
    )
    .into_state();
    for n in [1, 2, 3, 4] {
        assert_eq!(
            attacks
                .execution()
                .current_characteristics(physical(&attacks, n).id())
                .unwrap()
                .unwrap()
                .has_keyword(Deathtouch),
            matches!(n, 2 | 3)
        );
    }
    let defend = blockers(&attacks);
    for blocked in [true, false] {
        let choices = if blocked {
            block_choice(&defend, 4, 2)
        } else {
            DecisionReply::DeclareBlockers(vec![])
        };
        let next = reply(&defend, choices).into_state();
        let mut damage = exile_hand::until(next, |s| {
            s.turn().unwrap().position().step == Step::CombatDamage
        });
        if let Some(RuleFrame::Triggers(frame)) = damage.pending_decision().map(|d| d.frame()) {
            damage = reply(
                &damage,
                DecisionReply::OrderTriggers(frame.instances().to_vec()),
            )
            .into_state();
        }
        let count = if blocked { 1 } else { 2 };
        assert_eq!(damage.execution().window().entries().len(), count);
        for _ in 0..count {
            damage = round(damage).into_state();
        }
        assert_eq!(
            physical(&damage, 4).zone(),
            if blocked {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
        assert_eq!(physical(&damage, 6).zone(), ObjectZone::Hand(A));
        assert_eq!(
            physical(&damage, 100).zone(),
            if blocked {
                ObjectZone::Library(A)
            } else {
                ObjectZone::Hand(A)
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_ghost_of_ramirez_depietro_evades_large_blockers_and_returns_recently_discarded_or_milled_cards_to_their_owners()
 {
    use spellwake_engine::program::ResolvingPlayer;
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Ghost of Ramirez DePietro"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::Controller),
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ))),
                ),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PLAYER,
                    Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::mill(
                            ResolvingPlayer::TargetPlayer,
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                    )),
                ),
            ),
            named("Island", 5, A, ObjectDestination::Hand),
            named("Forest", 6, B, ObjectDestination::LibraryBottom),
            named("Sol Ring", 7, A, ObjectDestination::Graveyard),
            creature(8, B, 1, 2, &[]),
            creature(9, B, 1, 3, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let discard = round(cast_untargeted(&entered, 3, &[]).into_state()).into_state();
    let discarded = reply(
        &discard,
        DecisionReply::SelectResolution(vec![physical(&discard, 5).id()]),
    )
    .into_state();
    let milled = round(targeted(&discarded, A, 4, TargetRef::Player(B)).into_state()).into_state();
    let define = blockers(&attack(&milled, 1, B));
    reply(&define, block_choice(&define, 8, 1));
    illegal(
        &define,
        B,
        combat_declaration(&define, block_choice(&define, 9, 1)),
    );
    let unblocked = reply(&define, DecisionReply::DeclareBlockers(vec![])).into_state();
    let trigger = exile_hand::until(unblocked, |s| {
        matches!(
            s.pending_decision().map(|d| d.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    let choose = |nums: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            nums.iter()
                .map(|&n| TargetRef::Object(physical(&trigger, n).id()))
                .collect(),
        )
    };
    for nums in [vec![3], vec![7], vec![5, 6]] {
        illegal(&trigger, A, combat_declaration(&trigger, choose(&nums)));
    }
    for wanted in [None, Some(5), Some(6)] {
        let chosen = reply(&trigger, choose(wanted.as_slice())).into_state();
        let done = round(chosen).into_state();
        for (n, owner) in [(5, A), (6, B)] {
            assert_eq!(
                physical(&done, n).zone(),
                if wanted == Some(n) {
                    ObjectZone::Hand(owner)
                } else {
                    ObjectZone::Graveyard(owner)
                }
            );
        }
    }
    let inventory = current_catalogue().0;
    let id = |name: &str| {
        OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|r| r.name == name && r.required_by_base_formats)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let commanders = [
        id("Ghost of Ramirez DePietro"),
        id("Keleth, Sunmane Familiar"),
    ];
    let deck = crate::commander::prepare_deck(
        inventory,
        &spellwake_contracts::names::NameRules::literal_names(),
        &commanders,
        &vec![id("Wastes"); 98],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(deck.commanders(), commanders);
    assert_eq!(deck.color_identity(), &[Color::Blue, Color::White].into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mangara_counts_attackers_at_its_controller_and_planeswalker_and_each_opponents_second_cast()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let mut cards = vec![
        hand("Mangara, the Diplomat"),
        named("Vronos, Masked Inquisitor", 2, A, ObjectDestination::Hand),
        creature(3, B, 1, 1, &[]),
        creature(4, B, 1, 1, &[]),
        named(
            "Fervor",
            5,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ];
    for n in 6..=8 {
        cards.push(seed(
            n,
            B,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ));
    }
    for n in 9..=11 {
        cards.push(named("Forest", n, A, ObjectDestination::LibraryBottom));
    }
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 6), (White, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 2, &[(Colourless, 3), (Blue, 2)]).into_state())
        .into_state();
    let live = cast_named(&walker, &price);
    let attacking = attack(&live, 1, C);
    let damage = exile_hand::until(attacking, |s| {
        s.turn().unwrap().position().step == Step::CombatDamage
    });
    assert_eq!(damage.execution().life_total(A), Some(&22.into()));
    let mut their_main = next_main(live, 2);
    for n in 6..=8 {
        let stacked = command(
            &their_main,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&their_main, n).id(),
                payment: None,
            },
        )
        .into_state();
        assert_eq!(
            stacked.execution().window().entries().len(),
            if n == 7 { 2 } else { 1 }
        );
        if n == 7 {
            let drew = round(stacked).into_state();
            assert_eq!(physical(&drew, 9).zone(), ObjectZone::Hand(A));
            their_main = round(drew).into_state();
        } else {
            their_main = round(stacked).into_state();
        }
    }
    let declaration = combat_frame(&their_main, true);
    for (second_defender, to_walker, draw) in [(A, true, true), (A, false, true), (C, false, false)]
    {
        let attackers = vec![
            AttackDeclaration {
                attacker: physical(&declaration, 3).id(),
                defender: A,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: physical(&declaration, 4).id(),
                defender: second_defender,
                defending_planeswalker: to_walker.then(|| physical(&declaration, 2).id()),
            },
        ];
        let declared = reply(&declaration, DecisionReply::DeclareAttackers(attackers)).into_state();
        assert_eq!(
            declared.execution().window().entries().len(),
            usize::from(draw)
        );
        if draw {
            let result = round(declared);
            assert_eq!(physical(result.state(), 10).zone(), ObjectZone::Hand(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_padeem_protects_only_own_artifacts_and_rechecks_its_upkeep_lead_or_tie_at_resolution()
{
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 1)];
    for (opposing, should_draw) in [
        ("Sol Ring", true),
        ("Mind Stone", true),
        ("Worn Powerstone", false),
    ] {
        let mut cards = vec![
            hand("Padeem, Consul of Innovation"),
            named(
                "Mind Stone",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                opposing,
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Island", 4, A, ObjectDestination::LibraryTop),
        ];
        for (n, owner) in [(5, A), (6, B)] {
            cards.push(seed(
                n,
                owner,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PERMANENT,
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
            ));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let hex = |n| {
            entered
                .execution()
                .current_characteristics(physical(&entered, n).id())
                .unwrap()
                .unwrap()
                .has_keyword(Hexproof)
        };
        assert!(!hex(1));
        assert!(hex(2));
        assert!(!hex(3));
        let their = pass(&entered).into_state();
        let aim = |n| GameCommand::Cast {
            spell: physical(&their, 6).id(),
            target: TargetRef::Object(physical(&their, n).id()),
        };
        illegal(&their, B, aim(2));
        assert!(
            their
                .advance(GameInput {
                    basis: their.basis().clone(),
                    actor: B,
                    command: aim(3)
                })
                .is_ok()
        );
        let opposing_upkeep = exile_hand::until(entered, |s| {
            let p = s.turn().unwrap().position();
            p.number == 2 && p.step == Step::Upkeep
        });
        assert!(opposing_upkeep.execution().window().entries().is_empty());
        let ours = exile_hand::until(opposing_upkeep, |s| {
            let p = s.turn().unwrap().position();
            p.number == 3 && p.step == Step::Upkeep
        });
        assert_eq!(
            ours.execution().window().entries().len(),
            usize::from(should_draw)
        );
        if should_draw {
            let draw = round(ours.clone());
            assert_eq!(physical(draw.state(), 4).zone(), ObjectZone::Hand(A));
            let removed = round(
                targeted(&ours, A, 5, TargetRef::Object(physical(&ours, 2).id())).into_state(),
            )
            .into_state();
            assert_eq!(physical(&removed, 2).zone(), ObjectZone::Graveyard(A));
            let no_draw = round(removed);
            assert_eq!(physical(no_draw.state(), 4).zone(), ObjectZone::Library(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_meren_counts_only_other_controlled_deaths_and_returns_own_creatures_by_experience_at_its_end_step()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Meren of Clan Nel Toth"),
            named(
                "Elvish Mystic",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Fencing Ace", 3, A, ObjectDestination::Graveyard),
            named("Sol Ring", 4, A, ObjectDestination::Graveyard),
            named("Fencing Ace", 5, B, ObjectDestination::Graveyard),
            creature(6, B, 2, 2, &[]),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let kill = |n| {
        round(
            targeted(
                &entered,
                A,
                7,
                TargetRef::Object(physical(&entered, n).id()),
            )
            .into_state(),
        )
        .into_state()
    };
    for n in [1, 6] {
        let negative = kill(n);
        assert!(negative.execution().window().entries().is_empty());
        assert_eq!(negative.execution().experience_counters(A), 0.into());
    }
    let experienced = round(kill(2)).into_state();
    assert_eq!(experienced.execution().experience_counters(A), 1.into());
    assert_eq!(experienced.execution().experience_counters(B), 0.into());
    let end = exile_hand::until(experienced, |s| {
        s.turn().unwrap().position().step == Step::End
    });
    let pick = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&end, n).id()));
    for n in [4, 5] {
        illegal(&end, A, combat_declaration(&end, pick(n)));
    }
    for (n, zone) in [(2, ObjectZone::Battlefield), (3, ObjectZone::Hand(A))] {
        let returned = round(reply(&end, pick(n)).into_state()).into_state();
        assert_eq!(physical(&returned, n).zone(), zone);
        let other_end = exile_hand::until(returned, |s| {
            let p = s.turn().unwrap().position();
            p.number == 2 && p.step == Step::End
        });
        assert!(other_end.execution().window().entries().is_empty());
        assert!(other_end.pending_decision().is_none());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_erebos_loses_creature_type_below_devotion_and_sacrifices_other_creatures_for_a_penalty_and_optional_paid_draw()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Black, 1)];
    let source = hand("Erebos, Bleak-Hearted");
    let ability = source.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            source,
            named(
                "Phyrexian Gargantua",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Phyrexian Gargantua",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(4, B, 4, 4, &[]),
            named("Island", 5, A, ObjectDestination::LibraryTop),
            remove_spell(6, A),
        ],
        &[(Colourless, 4), (Black, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let god = |s: &GameState| {
        s.execution()
            .current_characteristics(physical(s, 1).id())
            .unwrap()
            .unwrap()
    };
    assert!(god(&entered).current_types.contains(&CardType::Creature));
    assert!(god(&entered).current_types.contains(&CardType::Enchantment));
    assert!(god(&entered).has_keyword(Indestructible));
    let destroyed = round(
        targeted(
            &entered,
            A,
            6,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state(),
    );
    assert_eq!(
        physical(destroyed.state(), 1).zone(),
        ObjectZone::Battlefield
    );
    let other = round(
        targeted(
            &entered,
            A,
            6,
            TargetRef::Object(physical(&entered, 4).id()),
        )
        .into_state(),
    );
    assert!(other.state().execution().window().entries().is_empty());
    let activate = |id| GameCommand::ActivateWithCostObjects {
        source: physical(&entered, 1).id(),
        ability,
        granting_effect: None,
        target: Some(TargetRef::Object(physical(&entered, 4).id())),
        objects: vec![physical(&entered, id).id()],
        payment: Some(spend(&[(Colourless, 1), (Black, 1)])),
    };
    for wrong in [1, 4] {
        illegal(&entered, A, activate(wrong));
    }
    let paid = command(&entered, A, activate(2)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert!(!god(&paid).current_types.contains(&CardType::Creature));
    assert!(god(&paid).current_types.contains(&CardType::Enchantment));
    assert!(god(&paid).has_keyword(Indestructible));
    let option = round(paid).into_state();
    for take in [false, true] {
        let done = reply(
            &option,
            DecisionReply::OptionalEffect(if take {
                OptionalEffectChoice::Take
            } else {
                OptionalEffectChoice::Decline
            }),
        )
        .into_state();
        assert_eq!(
            done.execution().life_total(A),
            Some(&if take { 18.into() } else { 20.into() })
        );
        assert_eq!(
            physical(&done, 5).zone(),
            if take {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
        let shrunk = round(done).into_state();
        let size = |s: &GameState| {
            s.execution()
                .power_toughness(physical(s, 4).id())
                .unwrap()
                .unwrap()
                .derived
        };
        assert_eq!(
            size(&shrunk),
            PowerToughness {
                power: 2.into(),
                toughness: 3.into()
            }
        );
        assert_eq!(
            size(&next_main(shrunk, 2)),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_losheel_prevents_damage_only_to_controlled_attackers_that_are_artifact_creatures_and_draws_on_the_first_entry_each_turn()
 {
    use ManaType::*;
    let artifact_body = |id, owner, zone| {
        let mut p = Program::creature(Some(ManaCost::free()), 1.into(), 4.into(), BTreeSet::new());
        p.card_types.insert(CardType::Artifact);
        p.printed_keywords.insert(Flash);
        seed(id, owner, zone, p)
    };
    let price = [(Colourless, 2), (White, 1)];
    let mut cards = vec![
        hand("Losheel, Clockwork Scholar"),
        named(
            "Fervor",
            13,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        artifact_body(2, A, ObjectDestination::Battlefield { controller: A }),
        creature(3, A, 1, 4, &[]),
        creature(4, B, 2, 6, &[]),
        creature(5, B, 2, 6, &[]),
        named("Island", 6, A, ObjectDestination::LibraryBottom),
        named("Forest", 7, A, ObjectDestination::LibraryBottom),
        seed(
            12,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ),
        ),
    ];
    for n in 8..11 {
        cards.push(artifact_body(n, A, ObjectDestination::Hand));
    }
    cards.push(artifact_body(11, B, ObjectDestination::Hand));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entered = cast_named(&initial, &price);
    let first = round(cast_untargeted(&entered, 8, &[]).into_state()).into_state();
    assert_eq!(first.execution().window().entries().len(), 1);
    let drawn = round(first).into_state();
    assert_eq!(physical(&drawn, 6).zone(), ObjectZone::Hand(A));
    let second = round(cast_untargeted(&drawn, 9, &[]).into_state()).into_state();
    assert!(second.execution().window().entries().is_empty());
    let opponent = next_main(second, 2);
    let other = round(
        command(
            &opponent,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&opponent, 11).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert!(other.execution().window().entries().is_empty());
    let priority = pass(&other).into_state();
    let third = round(cast_untargeted(&priority, 10, &[]).into_state()).into_state();
    assert_eq!(third.execution().window().entries().len(), 1);
    assert_eq!(
        physical(round(third).state(), 7).zone(),
        ObjectZone::Hand(A)
    );

    let declare = combat_frame(&entered, true);
    let attacked = reply(
        &declare,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .map(|n| AttackDeclaration {
                    attacker: physical(&declare, n).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .to_vec(),
        ),
    )
    .into_state();
    let defend = blockers(&attacked);
    let blocked = reply(
        &defend,
        DecisionReply::DeclareBlockers(
            [(2, 4), (3, 5)]
                .map(|(a, b)| BlockDeclaration {
                    blocker: physical(&defend, b).id(),
                    attacker: physical(&defend, a).id(),
                })
                .to_vec(),
        ),
    )
    .into_state();
    let next = round(
        targeted(
            &blocked,
            A,
            12,
            TargetRef::Object(physical(&blocked, 2).id()),
        )
        .into_state(),
    )
    .into_state();
    let damage = exile_hand::until(next, |s| {
        s.turn().unwrap().position().step == Step::CombatDamage
    });
    for (n, marked) in [(2, 1), (3, 2), (4, 1), (5, 1)] {
        assert_eq!(
            damage
                .execution()
                .battlefield_marked_damage(physical(&damage, n).id()),
            Some(marked.into())
        );
    }
}
