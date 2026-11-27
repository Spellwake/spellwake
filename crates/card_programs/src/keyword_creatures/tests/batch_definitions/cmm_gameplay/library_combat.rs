//! Combat and legend-rule branches for printed library-manipulation cards.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_library_flyers_reject_ground_blockers_and_apply_menace_and_trample() {
    use spellwake_engine::combat::CombatDamageAssignment;
    for name in [
        "Pilgrim's Eye",
        "Cadaver Imp",
        "Custodi Squire",
        "Twilight Prophet",
        "Windrider Wizard",
        "Mindwrack Harpy",
        "Murder of Crows",
        "Teshar, Ancestor's Apostle",
        "Rune-Scarred Demon",
        "Hoarding Dragon",
        "Sandstone Oracle",
        "Demonlord Belzenlok",
        "Cartographer's Hawk",
        "Yennett, Cryptic Sovereign",
    ] {
        let state = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Battlefield { controller: A }),
                creature(2, B, 1, 1, &[]),
                creature(3, B, 1, 1, &[Reach]),
                creature(4, B, 2, 2, &[Flying]),
                named(
                    "Fervor",
                    5,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
            ],
            &[],
            Step::PrecombatMain,
        );
        let attack = attack(&state, 1, B);
        let before_blocks = if name == "Yennett, Cryptic Sovereign" {
            assert_eq!(physical(&attack, 1).tapped(), Some(false));
            round(attack).into_state()
        } else {
            attack
        };
        let defend = blockers(&before_blocks);
        illegal(
            &defend,
            B,
            combat_declaration(&defend, block_choice(&defend, 2, 1)),
        );
        let reach = block_choice(&defend, 3, 1);
        if name == "Yennett, Cryptic Sovereign" {
            illegal(&defend, B, combat_declaration(&defend, reach));
            let double = DecisionReply::DeclareBlockers(
                [3, 4]
                    .into_iter()
                    .map(|n| BlockDeclaration {
                        blocker: physical(&defend, n).id(),
                        attacker: physical(&defend, 1).id(),
                    })
                    .collect(),
            );
            reply(&defend, double);
            continue;
        }
        let blocked = reply(&defend, reach).into_state();
        if name == "Demonlord Belzenlok" {
            let distribution = round(blocked).into_state();
            let source = physical(&distribution, 1).id();
            let blocker = physical(&distribution, 3).id();
            let assigned = reply(
                &distribution,
                DecisionReply::AssignCombatDamage(vec![
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Creature(blocker),
                        amount: 1.into(),
                    },
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Player(B),
                        amount: 5.into(),
                    },
                ]),
            );
            assert_eq!(
                physical(assigned.state(), 3).zone(),
                ObjectZone::Graveyard(B)
            );
            assert_eq!(assigned.state().execution().life_total(B), Some(&15.into()));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_gonti_gorex_and_isareth_kill_surviving_attackers_with_deathtouch() {
    for name in [
        "Gonti, Lord of Luxury",
        "Gorex, the Tombshell",
        "Isareth the Awakener",
    ] {
        let state = game(
            vec![A, B],
            vec![
                named(name, 1, B, ObjectDestination::Battlefield { controller: B }),
                creature(2, A, 1, 12, &[]),
                creature(3, B, 2, 2, &[]),
                named(
                    "Fervor",
                    4,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
            ],
            &[],
            Step::PrecombatMain,
        );
        let defend = blockers(&attack(&state, 2, B));
        for (blocker, zone) in [(1, ObjectZone::Graveyard(A)), (3, ObjectZone::Battlefield)] {
            let blocked = reply(&defend, block_choice(&defend, blocker, 2)).into_state();
            let result = exile_hand::until(blocked, |s| {
                s.turn().unwrap().position().step == Step::EndCombat
            });
            assert_eq!(physical(&result, 2).zone(), zone, "{name}");
            assert_eq!(physical(&result, blocker).zone(), ObjectZone::Battlefield);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_legendary_library_creatures_keep_either_copy_on_their_controllers_battlefield() {
    use ManaType::*;
    use spellwake_engine::checks::CheckDecision;
    for (name, price) in [
        (
            "Meren of Clan Nel Toth",
            vec![(Colourless, 2), (Black, 1), (Green, 1)],
        ),
        ("Erebos, Bleak-Hearted", vec![(Colourless, 3), (Black, 1)]),
        (
            "Losheel, Clockwork Scholar",
            vec![(Colourless, 2), (White, 1)],
        ),
        ("Braids, Conjurer Adept", vec![(Colourless, 2), (Blue, 2)]),
        ("Gonti, Lord of Luxury", vec![(Colourless, 2), (Black, 2)]),
        ("Isareth the Awakener", vec![(Colourless, 1), (Black, 2)]),
        ("Gorex, the Tombshell", vec![(Colourless, 6), (Black, 2)]),
        ("Demonlord Belzenlok", vec![(Colourless, 4), (Black, 2)]),
        (
            "Yennett, Cryptic Sovereign",
            vec![(Colourless, 2), (White, 1), (Blue, 1), (Black, 1)],
        ),
        (
            "Melek, Izzet Paragon",
            vec![(Colourless, 4), (Blue, 1), (Red, 1)],
        ),
        ("Kaho, Minamo Historian", vec![(Colourless, 2), (Blue, 2)]),
        ("Stitcher Geralf", vec![(Colourless, 3), (Blue, 2)]),
        (
            "Padeem, Consul of Innovation",
            vec![(Colourless, 3), (Blue, 1)],
        ),
        ("Mangara, the Diplomat", vec![(Colourless, 3), (White, 1)]),
        (
            "Ghost of Ramirez DePietro",
            vec![(Colourless, 2), (Blue, 1)],
        ),
        (
            "Jolrael, Mwonvuli Recluse",
            vec![(Colourless, 1), (Green, 1)],
        ),
        ("Inga Rune-Eyes", vec![(Colourless, 3), (Blue, 1)]),
        ("Oath of Jace", vec![(Colourless, 2), (Blue, 1)]),
        (
            "Narset, Enlightened Master",
            vec![(Colourless, 3), (Blue, 1), (Red, 1), (White, 1)],
        ),
        (
            "Yuriko, the Tiger's Shadow",
            vec![(Colourless, 1), (Blue, 1), (Black, 1)],
        ),
        ("Grenzo, Havoc Raiser", vec![(Red, 2)]),
        (
            "Nekusar, the Mindrazer",
            vec![(Colourless, 2), (Blue, 1), (Black, 1), (Red, 1)],
        ),
        (
            "Teshar, Ancestor's Apostle",
            vec![(Colourless, 3), (White, 1)],
        ),
        (
            "Sidisi, Brood Tyrant",
            vec![(Colourless, 1), (Blue, 1), (Green, 1), (Black, 1)],
        ),
        (
            "Taigam, Sidisi's Hand",
            vec![(Colourless, 3), (Blue, 1), (Black, 1)],
        ),
        ("Yedora, Grave Gardener", vec![(Colourless, 4), (Green, 1)]),
        (
            "Tatyova, Benthic Druid",
            vec![(Colourless, 3), (Green, 1), (Blue, 1)],
        ),
    ] {
        let state = game(
            vec![A, B],
            vec![
                hand(name),
                named(name, 2, A, ObjectDestination::Battlefield { controller: A }),
                named(name, 3, B, ObjectDestination::Battlefield { controller: B }),
            ],
            &price,
            Step::PrecombatMain,
        );
        let choose = cast_named(&state, &price);
        let RuleFrame::Checks(CheckDecision::Legend(frame)) =
            choose.pending_decision().unwrap().frame()
        else {
            panic!("expected a legend conflict for {name}");
        };
        let [group] = frame.conflicts() else {
            panic!("expected one conflict for {name}")
        };
        let candidates = group.domain().iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            candidates,
            [physical(&choose, 1).id(), physical(&choose, 2).id()].into()
        );
        for (keep, lose) in [(1, 2), (2, 1)] {
            let outcome = reply(
                &choose,
                DecisionReply::KeepLegend {
                    conflict: group.id(),
                    keep: physical(&choose, keep).id(),
                },
            );
            assert_eq!(
                physical(outcome.state(), keep).zone(),
                ObjectZone::Battlefield
            );
            assert_eq!(
                physical(outcome.state(), lose).zone(),
                ObjectZone::Graveyard(A)
            );
            assert_eq!(physical(outcome.state(), 3).zone(), ObjectZone::Battlefield);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_isareth_can_decline_its_attack_payment_and_braids_can_put_a_card_in_on_its_own_upkeep()
{
    use spellwake_engine::effects::ResolutionPaymentReply;
    let isareth = game(
        vec![A, B],
        vec![
            named(
                "Isareth the Awakener",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Elvish Mystic", 3, A, ObjectDestination::Graveyard),
        ],
        &[],
        Step::PrecombatMain,
    );
    let offer = round(attack(&isareth, 1, B)).into_state();
    let declined = reply(
        &offer,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    assert!(declined.execution().window().entries().is_empty());
    assert_eq!(physical(&declined, 3).zone(), ObjectZone::Graveyard(A));

    let braids = game(
        vec![A, B],
        vec![
            named(
                "Braids, Conjurer Adept",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Sol Ring", 2, A, ObjectDestination::Hand),
        ],
        &[],
        Step::PrecombatMain,
    );
    let own = exile_hand::until(braids, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::Upkeep
    });
    let choice = round(own).into_state();
    assert_eq!(choice.pending_decision().unwrap().chooser(), A);
    let entered = reply(
        &choice,
        DecisionReply::SelectResolution(vec![physical(&choice, 2).id()]),
    );
    assert_eq!(physical(entered.state(), 2).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_gorex_uses_entropy_to_return_one_of_the_cards_exiled_when_cast() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Gorex, the Tombshell"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Elvish Mystic", 3, A, ObjectDestination::Graveyard),
            named(
                "Ornithopter of Paradise",
                4,
                A,
                ObjectDestination::Graveyard,
            ),
            named("Elvish Mystic", 5, A, ObjectDestination::Exile),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                objects: [3, 4]
                    .into_iter()
                    .map(|n| CastResource::ExileCreatureForReduction(physical(&initial, n).id()))
                    .collect(),
                ..Default::default()
            },
        },
    );
    let entered = round(cast.into_state()).into_state();
    let attacking = attack(&entered, 1, B);
    let mut returned = BTreeSet::new();
    for sample in [0, 1] {
        let ready = attacking
            .supply_entropy(attacking.basis(), vec![sample])
            .unwrap()
            .into_state();
        let outcome = round(ready).into_state();
        let got = (3..=4)
            .filter(|&n| physical(&outcome, n).zone() == ObjectZone::Hand(A))
            .collect::<Vec<_>>();
        assert_eq!(got.len(), 1);
        returned.insert(got[0]);
        assert_eq!(physical(&outcome, 5).zone(), ObjectZone::Exile);
    }
    assert_eq!(returned, [3, 4].into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cunning_rhetoric_counts_an_attack_on_its_controller_and_planeswalker_only_once() {
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Vronos, Masked Inquisitor"),
            named(
                "Cunning Rhetoric",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, B, 1, 1, &[]),
            creature(4, B, 1, 1, &[]),
            named("Forest", 5, B, ObjectDestination::LibraryBottom),
            named("Sol Ring", 6, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let b_turn = next_main(cast_named(&initial, &price), 2);
    let declare = combat_frame(&b_turn, true);
    for (to_walker, second_defender, count) in
        [(true, None, 1), (true, Some(A), 1), (false, Some(C), 0)]
    {
        let mut attacks = vec![AttackDeclaration {
            attacker: physical(&declare, 3).id(),
            defender: if to_walker { A } else { C },
            defending_planeswalker: to_walker.then(|| physical(&declare, 1).id()),
        }];
        if let Some(defender) = second_defender {
            attacks.push(AttackDeclaration {
                attacker: physical(&declare, 4).id(),
                defender,
                defending_planeswalker: None,
            });
        }
        let attacked = reply(&declare, DecisionReply::DeclareAttackers(attacks)).into_state();
        assert_eq!(attacked.execution().window().entries().len(), count);
        if count == 1 {
            let exiled = round(attacked).into_state();
            assert_eq!(physical(&exiled, 6).zone(), ObjectZone::Exile);
            assert_eq!(physical(&exiled, 101).zone(), ObjectZone::Library(B));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_yedora_ignores_opposing_creatures_tokens_and_its_own_death() {
    let price = [(ManaType::Colourless, 2), (ManaType::Green, 1)];
    let mut cards = vec![
        named(
            "Yedora, Grave Gardener",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Jade Mage",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(3, B, 2, 2, &[]),
    ];
    for n in [4, 5, 6] {
        cards.push(seed(
            n,
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
        ));
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let make = command(
        &initial,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&initial, 2).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&price)),
        },
    );
    let mut state = round(make.into_state()).into_state();
    let token = state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .unwrap()
        .id();
    for (spell, target) in [
        (4, physical(&state, 3).id()),
        (5, token),
        (6, physical(&state, 1).id()),
    ] {
        state =
            round(targeted(&state, A, spell, TargetRef::Object(target)).into_state()).into_state();
        assert!(state.execution().window().entries().is_empty());
    }
    assert_eq!(physical(&state, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&state, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_promise_protects_its_controllers_walker_only_while_the_original_vow_continues() {
    use ManaType::*;
    use spellwake_engine::{effects::CounterKind, land::BasicLandType, mana::CastMode};
    let vow_cost = [(Colourless, 4), (White, 1)];
    let mut cards = vec![
        hand("Promise of Loyalty"),
        named("Vronos, Masked Inquisitor", 2, A, ObjectDestination::Hand),
        creature(3, B, 2, 2, &[]),
        named("Heartless Act", 4, A, ObjectDestination::Hand),
        named("Promise of Loyalty", 5, C, ObjectDestination::Hand),
        named(
            "Sol Ring",
            6,
            C,
            ObjectDestination::Battlefield { controller: C },
        ),
        named("Island", 10, B, ObjectDestination::LibraryBottom),
    ];
    for n in [7, 8, 9] {
        cards.push(named(
            "Plains",
            n,
            C,
            ObjectDestination::Battlefield { controller: C },
        ));
    }
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 8), (Blue, 2), (White, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 2, &[(Colourless, 3), (Blue, 2)]).into_state())
        .into_state();
    let select_vow = |s: GameState| {
        if s.pending_decision().is_some() {
            reply(
                &s,
                DecisionReply::SelectResolution(vec![physical(&s, 3).id()]),
            )
            .into_state()
        } else {
            s
        }
    };
    let protected = select_vow(cast_named(&walker, &vow_cost));
    let attack_restrictions = |state: GameState, turn| {
        let main = next_main(state, turn);
        let declare = combat_frame(&main, true);
        let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
            declare.pending_decision().unwrap().frame()
        else {
            panic!("expected attackers");
        };
        frame
            .forbidden_defenders()
            .get(&physical(&declare, 3).id())
            .cloned()
            .unwrap_or_default()
    };
    let protected_things = attack_restrictions(protected.clone(), 2);
    let walker = TargetRef::Object(physical(&protected, 2).id());
    assert!(protected_things.contains(&TargetRef::Player(A)) && protected_things.contains(&walker));
    let target = TargetRef::Object(physical(&protected, 3).id());
    let heartless = command(
        &protected,
        A,
        GameCommand::CastWithResources {
            spell: physical(&protected, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, 1), (Black, 1)])),
                modes: vec![CastMode {
                    mode: 1,
                    target: Some(target),
                    targets: vec![],
                }],
                ..Default::default()
            },
        },
    );
    let remove = round(heartless.into_state()).into_state();
    let released = reply(
        &remove,
        DecisionReply::RemoveCounters([(CounterKind::Vow, 1)].into()),
    )
    .into_state();
    assert_eq!(
        released
            .execution()
            .counters_of_kind(physical(&released, 3).id(), CounterKind::Vow),
        0.into()
    );
    let once_gone = attack_restrictions(released.clone(), 2);
    assert!(!once_gone.contains(&TargetRef::Player(A)) && !once_gone.contains(&walker));
    let mut third = next_main(released, 3);
    for (id, ability) in [
        (6, ManaAbilityRef::Printed(ManaAbilityId(0))),
        (7, ManaAbilityRef::BasicLand(BasicLandType::Plains)),
        (8, ManaAbilityRef::BasicLand(BasicLandType::Plains)),
        (9, ManaAbilityRef::BasicLand(BasicLandType::Plains)),
    ] {
        third = command(
            &third,
            C,
            GameCommand::ActivateMana {
                source: physical(&third, id).id(),
                ability,
            },
        )
        .into_state();
    }
    let again = command(
        &third,
        C,
        GameCommand::CastUntargeted {
            spell: physical(&third, 5).id(),
            payment: Some(spend(&[(Colourless, 2), (White, 3)])),
        },
    )
    .into_state();
    let renewed = select_vow(round(again).into_state());
    assert_eq!(
        renewed
            .execution()
            .counters_of_kind(physical(&renewed, 3).id(), CounterKind::Vow),
        1.into()
    );
    let renewed = attack_restrictions(renewed, 5);
    assert!(renewed.contains(&TargetRef::Player(C)));
    assert!(!renewed.contains(&TargetRef::Player(A)) && !renewed.contains(&walker));
}
