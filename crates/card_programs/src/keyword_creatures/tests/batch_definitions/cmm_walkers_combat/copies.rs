//! Hybrid casting, permanent copies and loyalty-copy triggers.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_saheeli_pays_either_hybrid_symbol_triggers_on_noncreatures_and_copies_until_cleanup() {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Saheeli, Sublime Artificer", 1, A, ObjectDestination::Hand),
            named("Goliath Sphinx", 2, A, battlefield(A)),
            named("Sol Ring", 3, A, battlefield(A)),
            named("Goliath Sphinx", 4, B, battlefield(B)),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    Default::default(),
                ),
            ),
        ],
        &[(Colourless, 1), (Blue, 2), (Red, 2)],
        Step::PrecombatMain,
    );
    let cast = |choices: Vec<HybridSide>, cost: &[(ManaType, i64)]| {
        round(
            command(
                &initial,
                A,
                GameCommand::CastWithResources {
                    spell: physical(&initial, 1).id(),
                    target: None,
                    payment: CastingPayment {
                        mana: Some(spend(cost)),
                        hybrid_choices: choices,
                        ..Default::default()
                    },
                },
            )
            .into_state(),
        )
        .into_state()
    };
    for (side, color) in [(HybridSide::First, Blue), (HybridSide::Second, Red)] {
        let mono = cast(vec![side, side], &[(Colourless, 1), (color, 2)]);
        assert_eq!(loyalty(&mono, 1), 5);
        assert_eq!(mono.execution().mana(A, color), 0.into());
    }
    let saheeli = cast(
        vec![HybridSide::Second, HybridSide::First],
        &[(Colourless, 1), (Blue, 1), (Red, 1)],
    );
    let creature_spell = cast_untargeted(&saheeli, 6, &[]).into_state();
    assert_eq!(creature_spell.execution().window().entries().len(), 1);
    let ready = round(creature_spell).into_state();
    let stacked = cast_untargeted(&ready, 5, &[]).into_state();
    assert_eq!(stacked.execution().window().entries().len(), 2);
    let made = round(stacked);
    let id = tokens(&made, A, 1)[0];
    let desc = made
        .state()
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap();
    assert_eq!(
        desc.current_types,
        [CardType::Creature, CardType::Artifact].into()
    );
    assert_eq!(
        desc.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Servo").unwrap()].into()
    );
    assert!(desc.current_colors.is_empty());
    assert_eq!(
        desc.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let ready = round(made.into_state()).into_state();
    let copy = |receiver: u64, donor: u64| GameCommand::ActivatePermanentTargets {
        source: physical(&ready, 1).id(),
        ability: ActivatedAbilityId(1),
        targets: vec![
            TargetRef::Object(physical(&ready, receiver).id()),
            TargetRef::Object(physical(&ready, donor).id()),
        ],
        payment: None,
    };
    for (receiver, donor) in [(3, 3), (2, 3), (3, 4)] {
        illegal(&ready, A, copy(receiver, donor));
    }
    for (donor, creature) in [(2, true), (5, false)] {
        let copied = round(command(&ready, A, copy(3, donor)).into_state()).into_state();
        assert_eq!(loyalty(&copied, 1), 3);
        let desc = copied
            .execution()
            .current_characteristics(physical(&copied, 3).id())
            .unwrap()
            .unwrap();
        assert!(desc.has_type(CardType::Artifact));
        assert_eq!(desc.is_creature(), creature);
        if creature {
            assert!(desc.has_keyword(KeywordAbility::Flying));
            assert_eq!(
                desc.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 8.into(),
                    toughness: 7.into()
                }
            );
        } else {
            assert!(desc.current_names.is_empty());
        }
        let done = exile_hand::until(copied, |s| s.turn().unwrap().position().number == 2);
        let original = done
            .execution()
            .current_characteristics(physical(&done, 3).id())
            .unwrap()
            .unwrap();
        assert!(
            original
                .current_names
                .iter()
                .any(|name| name.as_str() == "Sol Ring")
        );
        assert!(!original.is_creature());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_repeated_reverberation_copies_the_next_instant_sorcery_or_loyalty_twice_with_new_targets() {
    use ManaType::*;
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
    let start = game(
        vec![A, B, C],
        vec![
            named("Repeated Reverberation", 1, A, ObjectDestination::Hand),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::instant(ManaCost::free(), bolt.clone()),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::sorcery(ManaCost::free(), bolt),
            ),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ],
        &[(Colourless, 3), (Blue, 2), (Red, 2)],
        Step::PrecombatMain,
    );
    let jace =
        round(cast_untargeted(&start, 2, &[(Colourless, 1), (Blue, 2)]).into_state()).into_state();
    let delayed =
        round(cast_untargeted(&jace, 1, &[(Colourless, 2), (Red, 2)]).into_state()).into_state();
    let artifact = cast_untargeted(&delayed, 5, &[]).into_state();
    assert_eq!(artifact.execution().window().entries().len(), 1);
    let ready = round(artifact).into_state();
    let duplicated = |state: GameState, changes: [CopyTargetChoice; 2]| {
        assert_eq!(state.execution().window().entries().len(), 2);
        let mut choose = round(state).into_state();
        for choice in changes {
            assert!(matches!(
                choose.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::CopyTargets(_))
            ));
            choose = reply(&choose, DecisionReply::CopyTargets(choice)).into_state();
        }
        assert_eq!(choose.execution().window().entries().len(), 3);
        for _ in 0..3 {
            choose = round(choose).into_state();
        }
        choose
    };
    for (id, remaining) in [(3, 4), (4, 3)] {
        let cast = targeted(&ready, A, id, TargetRef::Player(B)).into_state();
        let done = duplicated(
            cast,
            [
                CopyTargetChoice::Retain,
                CopyTargetChoice::Change(TargetRef::Player(C)),
            ],
        );
        assert_eq!(done.execution().life_total(B), Some(&18.into()));
        assert_eq!(done.execution().life_total(C), Some(&19.into()));
        let second = targeted(&done, A, remaining, TargetRef::Player(A)).into_state();
        assert_eq!(second.execution().window().entries().len(), 1);
    }
    let loyalty_stack = command(
        &ready,
        A,
        GameCommand::ActivateAbility {
            source: physical(&ready, 2).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(B),
            payment: None,
        },
    )
    .into_state();
    let done = duplicated(
        loyalty_stack,
        [
            CopyTargetChoice::Change(TargetRef::Player(A)),
            CopyTargetChoice::Change(TargetRef::Player(C)),
        ],
    );
    assert_eq!(loyalty(&done, 2), 2);
    for (player, n) in [(A, 100), (B, 101), (C, 102)] {
        assert_eq!(physical(&done, n).zone(), ObjectZone::Hand(player));
    }
    let expired = exile_hand::until(ready, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::Upkeep
    });
    let priority = pass(pass(&expired).state()).into_state();
    let cast = targeted(&priority, A, 3, TargetRef::Player(B)).into_state();
    assert_eq!(cast.execution().window().entries().len(), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jayas_phoenix_returns_on_a_planeswalker_cast_and_copies_loyalty_after_combat_damage() {
    use ManaType::*;
    use spellwake_engine::effects::{CopyTargetChoice, OptionalEffectChoice};
    let start = game(
        vec![A, B],
        vec![
            named("Jaya's Phoenix", 1, A, ObjectDestination::Graveyard),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            named("Jace Beleren", 3, B, ObjectDestination::Hand),
            named("Island", 4, B, battlefield(B)),
            named("Island", 5, B, battlefield(B)),
            named("Sol Ring", 6, B, battlefield(B)),
            creature(7, B, 3, 3, &[]),
            creature(10, B, 1, 3, &[KeywordAbility::Reach]),
            seed(
                8,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                9,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let first = cast_untargeted(&start, 2, &[(Colourless, 1), (Blue, 2)]).into_state();
    assert_eq!(first.execution().window().entries().len(), 2);
    let offer = round(first).into_state();
    let decline = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&decline, 1).zone(), ObjectZone::Graveyard(A));
    let take = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(physical(&take, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&take, 2).zone(), ObjectZone::Stack);
    let ready = round(take).into_state();
    let a_combat = combat_frame(&ready, true);
    let attack = |state: &GameState, walker| {
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(state, 1).id(),
            defender: B,
            defending_planeswalker: walker,
        }])
    };
    let attacked = reply(&a_combat, attack(&a_combat, None)).into_state();
    let blocks = combat_frame(&attacked, false);
    illegal(
        &blocks,
        B,
        GameCommand::Reply {
            decision: blocks.pending_decision().unwrap().id(),
            choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                attacker: physical(&blocks, 1).id(),
                blocker: physical(&blocks, 7).id(),
            }]),
        },
    );
    let process = |state: GameState| {
        let post = exile_hand::until(state, |s| {
            s.turn().unwrap().position().step == Step::PostcombatMain
        });
        let cast = command(
            &post,
            A,
            GameCommand::ActivateAbility {
                source: physical(&post, 2).id(),
                ability: ActivatedAbilityId(1),
                target: TargetRef::Player(B),
                payment: None,
            },
        )
        .into_state();
        assert_eq!(cast.execution().window().entries().len(), 2);
        let choose = round(cast).into_state();
        let ready = reply(
            &choose,
            DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(A))),
        )
        .into_state();
        assert_eq!(ready.execution().window().entries().len(), 2);
        let library_count =
            |s: &GameState, player| s.execution().window().cards().library(player).len();
        let done = round(round(ready).into_state()).into_state();
        assert_eq!(library_count(&done, A) + 1, library_count(&post, A));
        assert_eq!(library_count(&done, B) + 1, library_count(&post, B));
        assert_eq!(loyalty(&done, 2), 2);
        done
    };
    let hit_player = process(reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state());
    assert_eq!(hit_player.execution().life_total(B), Some(&17.into()));
    let quiet = reply(&a_combat, DecisionReply::DeclareAttackers(vec![])).into_state();
    let mut main = exile_hand::until(quiet, |s| {
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
    let b_walker = round(
        command(
            &main,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&main, 3).id(),
                payment: Some(spend(&[(Colourless, 1), (Blue, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let combat = exile_hand::until(b_walker, |s| {
        s.turn().unwrap().position().number == 3
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let assault = reply(&combat, attack(&combat, Some(physical(&combat, 3).id()))).into_state();
    let killed = process(assault);
    assert_eq!(physical(&killed, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(killed.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sparkshaper_visionary_animates_chosen_walkers_retains_loyalty_and_scries_on_combat_damage() {
    use ManaType::*;
    use spellwake_engine::effects::LibraryTopReply;
    let mut cards = vec![
        named("Sparkshaper Visionary", 1, A, ObjectDestination::Hand),
        named("Jace Beleren", 2, A, ObjectDestination::Hand),
        named("The Wanderer", 3, A, ObjectDestination::Hand),
        creature(4, B, 1, 1, &[]),
    ];
    for n in 10..14 {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let mut state = game(
        vec![A, B],
        cards,
        &[(Colourless, 6), (Blue, 3), (White, 1)],
        Step::PrecombatMain,
    );
    for (id, cost) in [
        (1, vec![(Colourless, 2), (Blue, 1)]),
        (2, vec![(Colourless, 1), (Blue, 2)]),
        (3, vec![(Colourless, 3), (White, 1)]),
    ] {
        state = round(cast_untargeted(&state, id, &cost).into_state()).into_state();
    }
    let targets = |s: &GameState| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    };
    let first = exile_hand::until(state, targets);
    let empty = round(reply(&first, DecisionReply::TriggerPermanentTargets(vec![])).into_state())
        .into_state();
    assert!(
        !empty
            .execution()
            .current_characteristics(physical(&empty, 2).id())
            .unwrap()
            .unwrap()
            .is_creature()
    );
    let next = exile_hand::until(empty, |s| {
        s.turn().unwrap().position().number == 3 && targets(s)
    });
    let select = |nums: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            nums.iter()
                .map(|&n| TargetRef::Object(physical(&next, n).id()))
                .collect(),
        )
    };
    illegal(
        &next,
        A,
        GameCommand::Reply {
            decision: next.pending_decision().unwrap().id(),
            choice: select(&[1]),
        },
    );
    let animated = round(reply(&next, select(&[2, 3])).into_state()).into_state();
    for n in [2, 3] {
        let view = animated
            .execution()
            .current_characteristics(physical(&animated, n).id())
            .unwrap()
            .unwrap();
        assert_eq!(view.current_types, [CardType::Creature].into());
        assert_eq!(view.current_colors, [Color::Blue].into());
        assert_eq!(
            view.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Bird").unwrap()].into()
        );
        assert!(
            view.has_keyword(KeywordAbility::Flying) && view.has_keyword(KeywordAbility::Hexproof)
        );
        assert_eq!(
            view.power_toughness.unwrap().derived,
            PowerToughness {
                power: 3.into(),
                toughness: 3.into()
            }
        );
    }
    let declare = combat_frame(&animated, true);
    let combat = reply(
        &declare,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&declare, 2).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let scry = exile_hand::until(combat, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::LibraryTop(_))
        )
    });
    assert_eq!(scry.execution().life_total(B), Some(&17.into()));
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    let top = frame.domain_top_first().to_vec();
    assert_eq!(top.len(), 1);
    let settled = reply(
        &scry,
        DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: vec![],
            bottom_top_first: top.clone(),
        }),
    )
    .into_state();
    assert_eq!(
        settled.execution().window().cards().library(A).last(),
        top.first()
    );
    let main = exile_hand::until(settled, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    let jace = round(
        command(
            &main,
            A,
            GameCommand::ActivateAbility {
                source: physical(&main, 2).id(),
                ability: ActivatedAbilityId(1),
                target: TargetRef::Player(A),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(loyalty(&jace, 2), 2);
    let end = exile_hand::until(jace, |s| s.turn().unwrap().position().number == 4);
    for n in [2, 3] {
        let view = end
            .execution()
            .current_characteristics(physical(&end, n).id())
            .unwrap()
            .unwrap();
        assert!(view.has_type(CardType::Planeswalker));
        assert!(!view.is_creature());
        assert!(!view.has_keyword(KeywordAbility::Hexproof));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_leori_copies_matching_planeswalker_abilities_for_the_turn_and_can_change_the_target() {
    use ManaType::*;
    use spellwake_engine::{effects::CopyTargetChoice, program::PlaneswalkerSubtype};
    let initial = game(
        vec![A, B],
        vec![
            named("Leori, Sparktouched Hunter", 1, A, ObjectDestination::Hand),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            named("The Wanderer", 3, A, ObjectDestination::Hand),
            creature(4, B, 1, 1, &[]),
            creature(5, B, 1, 1, &[KeywordAbility::Reach]),
            seed(
                8,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                9,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                10,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                11,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 4), (Blue, 3), (White, 2), (Red, 1)],
        Step::PrecombatMain,
    );
    let mut ready = initial;
    for (n, cost) in [
        (1, vec![(Blue, 1), (White, 1), (Red, 1)]),
        (2, vec![(Colourless, 1), (Blue, 2)]),
        (3, vec![(Colourless, 3), (White, 1)]),
    ] {
        ready = round(cast_untargeted(&ready, n, &cost).into_state()).into_state();
    }
    let combat = exile_hand::until(ready, |s| {
        s.turn().unwrap().position().number == 3
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let attacked = reply(
        &combat,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&combat, 1).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    assert_eq!(physical(&attacked, 1).tapped(), Some(false));
    let blocks = combat_frame(&attacked, false);
    let block = |id| {
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            attacker: physical(&blocks, 1).id(),
            blocker: physical(&blocks, id).id(),
        }])
    };
    illegal(
        &blocks,
        B,
        GameCommand::Reply {
            decision: blocks.pending_decision().unwrap().id(),
            choice: block(4),
        },
    );
    let hit = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
    let choice = exile_hand::until(hit, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::PlaneswalkerType(_))
        )
    });
    let copied = |kind| {
        let chosen = reply(&choice, DecisionReply::PlaneswalkerType(kind)).into_state();
        let main = exile_hand::until(chosen, |s| {
            s.turn().unwrap().position().step == Step::PostcombatMain
        });
        command(
            &main,
            A,
            GameCommand::ActivateAbility {
                source: physical(&main, 2).id(),
                ability: ActivatedAbilityId(1),
                target: TargetRef::Player(B),
                payment: None,
            },
        )
        .into_state()
    };
    let unmatched = copied(PlaneswalkerSubtype::Chandra);
    assert_eq!(unmatched.execution().window().entries().len(), 1);
    let matched = copied(PlaneswalkerSubtype::Jace);
    assert_eq!(matched.execution().window().entries().len(), 2);
    let new_target = round(matched).into_state();
    let two = reply(
        &new_target,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(A))),
    )
    .into_state();
    let result = round(round(two).into_state()).into_state();
    assert_eq!(loyalty(&result, 2), 2);
    assert_eq!(physical(&result, 10).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&result, 11).zone(), ObjectZone::Hand(B));
    let expired = exile_hand::until(result, |s| {
        s.turn().unwrap().position().number == 5
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let once = command(
        &expired,
        A,
        GameCommand::ActivateAbility {
            source: physical(&expired, 2).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(B),
            payment: None,
        },
    )
    .into_state();
    assert_eq!(once.execution().window().entries().len(), 1);
}
