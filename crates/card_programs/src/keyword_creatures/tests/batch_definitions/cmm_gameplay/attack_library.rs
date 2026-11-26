//! Attack and combat-damage triggers that grant access to library cards.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_narset_exiles_four_restricts_free_casts_and_survives_by_first_strike_and_hexproof() {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 1), (Red, 1), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Narset, Enlightened Master"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::instant(
                    ManaCost::generic(7.into()),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::GainControllerLife {
                            amount: spellwake_engine::program::ValueExpr::Literal(3.into()),
                        }],
                    ),
                ),
            ),
            named("Sol Ring", 4, A, ObjectDestination::LibraryBottom),
            named("Elvish Mystic", 5, A, ObjectDestination::LibraryBottom),
            named("Forest", 6, A, ObjectDestination::LibraryBottom),
            creature(7, B, 4, 3, &[]),
            seed(
                8,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
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
    let priority = pass(&entered).into_state();
    illegal(
        &priority,
        B,
        GameCommand::Cast {
            spell: physical(&priority, 8).id(),
            target: TargetRef::Object(physical(&priority, 1).id()),
        },
    );
    let attacked = attack(&entered, 1, B);
    let exiled = round(attacked).into_state();
    for n in 3..=6 {
        assert_eq!(physical(&exiled, n).zone(), ObjectZone::Exile);
    }
    assert_eq!(physical(&exiled, 100).zone(), ObjectZone::Library(A));
    for n in [5, 6] {
        illegal(
            &exiled,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&exiled, n).id(),
                payment: None,
            },
        );
    }
    illegal(
        &exiled,
        A,
        GameCommand::PlayLand {
            land: physical(&exiled, 6).id(),
        },
    );
    let spell = round(cast_untargeted(&exiled, 3, &[]).into_state()).into_state();
    assert_eq!(spell.execution().life_total(A), Some(&23.into()));
    let defend = blockers(&spell);
    let block = reply(&defend, block_choice(&defend, 7, 1)).into_state();
    let main = exile_hand::until(block, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(physical(&main, 7).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&main, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(
        main.execution()
            .battlefield_marked_damage(physical(&main, 1).id()),
        Some(0.into())
    );
    let cast = round(cast_untargeted(&main, 4, &[]).into_state());
    assert_eq!(physical(cast.state(), 4).zone(), ObjectZone::Battlefield);
    let expired = next_main(main, 3);
    illegal(
        &expired,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&expired, 4).id(),
            payment: None,
        },
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_ugins_mastery_manifests_colorless_creature_casts_and_may_turn_a_card_up_at_six_attacking_power()
 {
    use ManaType::*;
    use spellwake_engine::face_down::FaceDownKind;
    let cost = [(Colourless, 4)];
    for (card, can_turn) in [("Forest", true), ("Cultivate", false)] {
        let initial = game(
            vec![A, B],
            vec![
                hand("Ugin's Mastery"),
                seed(
                    2,
                    A,
                    ObjectDestination::Hand,
                    Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new()),
                ),
                named("Elvish Mystic", 3, A, ObjectDestination::Hand),
                named("Sol Ring", 4, A, ObjectDestination::Hand),
                named(card, 5, A, ObjectDestination::LibraryBottom),
                creature(6, A, 4, 3, &[]),
                creature(7, A, 2, 3, &[]),
                named(
                    "Fervor",
                    8,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
            ],
            &[(Colourless, 5), (Green, 1)],
            Step::PrecombatMain,
        );
        let ench = cast_named(&initial, &cost);
        let artifact =
            round(cast_untargeted(&ench, 4, &[(Colourless, 1)]).into_state()).into_state();
        let green = round(cast_untargeted(&artifact, 3, &[(Green, 1)]).into_state()).into_state();
        assert_eq!(physical(&green, 5).zone(), ObjectZone::Library(A));
        let colorless = cast_untargeted(&green, 2, &[]).into_state();
        let manifested = round(colorless).into_state();
        assert_eq!(physical(&manifested, 2).zone(), ObjectZone::Stack);
        assert_eq!(
            physical(&manifested, 5).face_down(),
            Some(FaceDownKind::Manifest)
        );
        let entry = round(manifested).into_state();
        let declare = combat_frame(&entry, true);
        let attacking = |ns: &[u64]| {
            DecisionReply::DeclareAttackers(
                ns.iter()
                    .map(|&n| AttackDeclaration {
                        attacker: physical(&declare, n).id(),
                        defender: B,
                        defending_planeswalker: None,
                    })
                    .collect(),
            )
        };
        let below = reply(&declare, attacking(&[6])).into_state();
        assert!(below.execution().window().entries().is_empty());
        let above = reply(&declare, attacking(&[6, 7])).into_state();
        let choose = round(above).into_state();
        let skip = reply(&choose, DecisionReply::SelectResolution(vec![]));
        assert_eq!(
            physical(skip.state(), 5).face_down(),
            Some(FaceDownKind::Manifest)
        );
        let turned = reply(
            &choose,
            DecisionReply::SelectResolution(vec![physical(&choose, 5).id()]),
        );
        assert_eq!(
            physical(turned.state(), 5).face_down(),
            if can_turn {
                None
            } else {
                Some(FaceDownKind::Manifest)
            }
        );
        assert!(
            events(&turned)
                .flat_map(|e| &e.occurrences)
                .any(|o| if can_turn {
                    matches!(o.value, RulesOccurrence::TurnedFaceUpByEffect { .. })
                } else {
                    matches!(o.value, RulesOccurrence::FaceDownRevealed { .. })
                })
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_yuriko_ninjutsus_from_hand_or_command_and_all_opponents_lose_the_revealed_mana_value()
{
    use ManaType::*;
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    let attacker = || {
        let mut p = Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
        p.printed_keywords.insert(Haste);
        p
    };
    let mana = || {
        Program::land(
            Default::default(),
            vec![ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed([(Blue, 1.into()), (Black, 1.into())].into()),
            }],
        )
    };
    for from_command in [false, true] {
        let source = if from_command { 100 } else { 1 };
        let state = if from_command {
            let mut cards = vec![
                named(
                    "Yuriko, the Tiger's Shadow",
                    100,
                    A,
                    ObjectDestination::Command,
                ),
                named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
            ];
            for n in 1..=7 {
                cards.push(seed(
                    n,
                    A,
                    ObjectDestination::LibraryBottom,
                    match n {
                        2 => mana(),
                        3 => attacker(),
                        _ => Program::artifact(None, vec![]),
                    },
                ));
            }
            // The starting player also draws on the first turn of multiplayer Commander.
            cards.push(named("Scaretiller", 8, A, ObjectDestination::LibraryBottom));
            cards.push(named("Scaretiller", 9, A, ObjectDestination::LibraryBottom));
            super::super::cmm_permanent_costs::commander_opening(cards, None)
        } else {
            game(
                vec![A, B, C],
                vec![
                    hand("Yuriko, the Tiger's Shadow"),
                    seed(2, A, ObjectDestination::Hand, mana()),
                    seed(3, A, ObjectDestination::Hand, attacker()),
                    named("Scaretiller", 8, A, ObjectDestination::LibraryBottom),
                ],
                &[],
                Step::PrecombatMain,
            )
        };
        let state = command(
            &state,
            A,
            GameCommand::PlayLand {
                land: physical(&state, 2).id(),
            },
        )
        .into_state();
        let ready = round(cast_untargeted(&state, 3, &[]).into_state()).into_state();
        let attack = attack(&ready, 3, B);
        let ninja_ability = ready
            .execution()
            .copiable_program(physical(&ready, source).id())
            .unwrap()
            .activated
            .iter()
            .find(|ability| {
                ability.source_cost
                    == spellwake_engine::activated::SourceCost::Ninjutsu { from_command: true }
            })
            .expect("commander ninjutsu")
            .id;
        let ninja = |s: &GameState| GameCommand::Ninjutsu {
            source: physical(s, source).id(),
            ability: ninja_ability,
            attacker: physical(s, 3).id(),
            payment: Some(spend(&[(Blue, 1), (Black, 1)])),
        };
        illegal(&attack, A, ninja(&attack));
        let defend = exile_hand::until(attack, |s| {
            s.turn().unwrap().position().step == Step::DeclareBlockers
        });
        let mut unblocked = if matches!(
            defend.pending_decision().map(|d| d.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        ) {
            reply(&defend, DecisionReply::DeclareBlockers(vec![])).into_state()
        } else {
            defend
        };
        unblocked = command(
            &unblocked,
            A,
            GameCommand::ActivateMana {
                source: physical(&unblocked, 2).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let paid = command(&unblocked, A, ninja(&unblocked)).into_state();
        assert_eq!(physical(&paid, 3).zone(), ObjectZone::Hand(A));
        let entered = round(paid).into_state();
        assert_eq!(
            (
                physical(&entered, source).zone(),
                physical(&entered, source).tapped()
            ),
            (ObjectZone::Battlefield, Some(true))
        );
        assert!(
            entered
                .execution()
                .combat()
                .unwrap()
                .attacker(physical(&entered, source).id())
                .is_some()
        );
        let top = *entered
            .execution()
            .window()
            .cards()
            .library(A)
            .first()
            .unwrap();
        let after = exile_hand::until(entered, |s| {
            s.turn().unwrap().position().step == Step::CombatDamage
        });
        let drawn = round(after);
        let appearance = events(&drawn)
            .flat_map(|e| &e.occurrences)
            .find_map(|row| match &row.value {
                RulesOccurrence::LibraryCardRevealed { object } => Some(object),
                _ => None,
            })
            .unwrap();
        assert_eq!(appearance.id(), top);
        assert_eq!(
            physical(drawn.state(), appearance.card_id().unwrap().0).zone(),
            ObjectZone::Hand(A)
        );
        let start = if from_command { 40_i64 } else { 20 };
        assert_eq!(
            drawn.state().execution().life_total(B),
            Some(&(start - 5).into())
        );
        if !from_command {
            assert_eq!(
                drawn.state().execution().life_total(C),
                Some(&(start - 4).into())
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_grenzo_chooses_a_creature_of_the_damaged_player_or_casts_their_top_spell_with_any_color()
 {
    use ManaType::*;
    use spellwake_engine::{land::BasicLandType, mana::CastMode};
    let price = [(Red, 2)];
    let steal = Program::instant(
        ManaCost {
            generic: 0.into(),
            required: [(Blue, 1.into())].into(),
        },
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
            }],
        ),
    );
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Grenzo, Havoc Raiser"),
            creature(2, A, 2, 3, &[]),
            creature(3, B, 2, 4, &[]),
            creature(4, C, 2, 4, &[]),
            seed(5, B, ObjectDestination::LibraryBottom, steal),
            named(
                "Forest",
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Fervor",
                7,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Island", 8, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let defend = blockers(&attack(&entered, 2, B));
    let unblocked = reply(&defend, DecisionReply::DeclareBlockers(vec![])).into_state();
    let choice = exile_hand::until(unblocked, |s| {
        matches!(
            s.pending_decision().map(|d| d.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    let mode = |n, target| {
        DecisionReply::TriggerModes(vec![CastMode {
            mode: n,
            target,
            targets: vec![],
        }])
    };
    illegal(
        &choice,
        A,
        combat_declaration(
            &choice,
            mode(0, Some(TargetRef::Object(physical(&choice, 4).id()))),
        ),
    );
    let target = physical(&choice, 3).id();
    let goad = round(reply(&choice, mode(0, Some(TargetRef::Object(target)))).into_state());
    assert!(events(&goad).flat_map(|e|&e.occurrences).any(|r|matches!(&r.value,RulesOccurrence::CreaturesGoaded{player,objects} if *player==A&&objects.as_slice()==[target])));
    let main = next_main(goad.into_state(), 2);
    let attack = combat_frame(&main, true);
    let declare = |seat| {
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 3).id(),
            defender: seat,
            defending_planeswalker: None,
        }])
    };
    illegal(&attack, B, combat_declaration(&attack, declare(A)));
    reply(&attack, declare(C));
    let exiled = round(reply(&choice, mode(1, None)).into_state()).into_state();
    assert_eq!(physical(&exiled, 5).zone(), ObjectZone::Exile);
    assert_eq!(physical(&exiled, 100).zone(), ObjectZone::Library(A));
    let funded = command(
        &exiled,
        A,
        GameCommand::ActivateMana {
            source: physical(&exiled, 6).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    )
    .into_state();
    let spell = round(cast_untargeted(&funded, 5, &[(Green, 1)]).into_state()).into_state();
    assert_eq!(physical(&spell, 5).zone(), ObjectZone::Graveyard(B));
    assert_eq!(spell.execution().life_total(A), Some(&22.into()));
    let expired = next_main(exiled, 2);
    assert_eq!(physical(&expired, 5).zone(), ObjectZone::Exile);
    assert!(
        !expired
            .execution()
            .may_cast_from_granted_exile(physical(&expired, 5).id(), A)
    );
}
