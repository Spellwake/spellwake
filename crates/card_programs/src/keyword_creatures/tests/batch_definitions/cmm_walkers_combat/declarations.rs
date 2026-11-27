//! Combat declaration limits, choices and control through multiplayer games.
use super::*;

pub(super) fn combat_on(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && matches!(
                s.pending_decision().map(|d| d.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    })
}
pub(super) fn attacks(
    state: &GameState,
    declarations: &[(u64, SeatId, Option<u64>)],
) -> DecisionReply {
    DecisionReply::DeclareAttackers(
        declarations
            .iter()
            .map(|&(attacker, defender, walker)| AttackDeclaration {
                attacker: physical(state, attacker).id(),
                defender,
                defending_planeswalker: walker.map(|id| physical(state, id).id()),
            })
            .collect(),
    )
}
pub(super) fn blocks(state: &GameState, pairs: &[(u64, u64)]) -> DecisionReply {
    DecisionReply::DeclareBlockers(
        pairs
            .iter()
            .map(|&(blocker, attacker)| BlockDeclaration {
                blocker: physical(state, blocker).id(),
                attacker: physical(state, attacker).id(),
            })
            .collect(),
    )
}
pub(super) fn invalid(state: &GameState, actor: SeatId, choice: DecisionReply) {
    illegal(
        state,
        actor,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice,
        },
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mirri_limits_each_opponents_blocks_and_only_attacks_directed_at_its_tapped_controller() {
    use ManaType::*;
    let cards = vec![
        named("Mirri, Weatherlight Duelist", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 3, &[]),
        creature(3, B, 3, 2, &[]),
        creature(4, B, 2, 3, &[]),
        creature(5, B, 2, 3, &[]),
        named("Jace Beleren", 6, A, ObjectDestination::Hand),
        creature(7, C, 2, 3, &[]),
        seed(
            8,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ];
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 2), (Green, 1), (White, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let mirri = round(
        cast_untargeted(&initial, 1, &[(Colourless, 1), (Green, 1), (White, 1)]).into_state(),
    )
    .into_state();
    let jace =
        round(cast_untargeted(&mirri, 6, &[(Colourless, 1), (Blue, 2)]).into_state()).into_state();
    let attack = combat_on(jace, 4);
    let declared = reply(&attack, attacks(&attack, &[(1, B, None), (2, B, None)])).into_state();
    assert_eq!(physical(&declared, 1).tapped(), Some(true));
    let defense = combat_frame(&declared, false);
    invalid(&defense, B, blocks(&defense, &[(3, 1), (4, 2)]));
    let duel = reply(&defense, blocks(&defense, &[(3, 1)])).into_state();
    let next = combat_on(duel, 5);
    assert_eq!(physical(&next, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&next, 1).zone(), ObjectZone::Battlefield);
    invalid(&next, B, attacks(&next, &[(4, A, None), (5, A, None)]));
    for declarations in [
        [(4, A, None), (5, C, None)],
        [(4, A, Some(6)), (5, A, Some(6))],
    ] {
        let legal = reply(&next, attacks(&next, &declarations)).into_state();
        assert_eq!(physical(&legal, 4).tapped(), Some(true));
        assert_eq!(physical(&legal, 5).tapped(), Some(true));
    }
    let quiet = reply(&attack, DecisionReply::DeclareAttackers(vec![])).into_state();
    let free = combat_on(quiet, 5);
    assert_eq!(physical(&free, 1).tapped(), Some(false));
    let all = reply(&free, attacks(&free, &[(3, A, None), (4, A, None)])).into_state();
    assert_eq!(physical(&all, 3).tapped(), Some(true));
    assert_eq!(physical(&all, 4).tapped(), Some(true));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_silent_arbiter_allows_at_most_one_declared_attacker_and_one_declared_blocker() {
    let cost = [(ManaType::Colourless, 4)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Silent Arbiter", 1, A, ObjectDestination::Hand),
            creature(2, A, 3, 3, &[KeywordAbility::Haste]),
            creature(3, A, 3, 3, &[KeywordAbility::Haste]),
            creature(4, B, 2, 3, &[]),
            creature(5, B, 2, 3, &[]),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    let combat = combat_frame(&cast, true);
    invalid(&combat, A, attacks(&combat, &[(2, B, None), (3, C, None)]));
    let attacked = reply(&combat, attacks(&combat, &[(2, B, None)])).into_state();
    let defending = combat_frame(&attacked, false);
    invalid(&defending, B, blocks(&defending, &[(4, 2), (5, 2)]));
    let block = reply(&defending, blocks(&defending, &[(4, 2)])).into_state();
    let end = exile_hand::until(block, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(physical(&end, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(end.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_avatar_of_slaughter_requires_each_eligible_creature_to_attack_and_grants_universal_double_strike()
 {
    use ManaType::*;
    let cost = [(Colourless, 6), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Avatar of Slaughter", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 3, &[KeywordAbility::Haste]),
            creature(3, A, 2, 3, &[KeywordAbility::Haste]),
            creature(4, B, 2, 3, &[]),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    for id in 1..=4 {
        assert!(
            cast.execution()
                .current_characteristics(physical(&cast, id).id())
                .unwrap()
                .unwrap()
                .has_keyword(KeywordAbility::DoubleStrike)
        );
    }
    let attack = combat_frame(&cast, true);
    invalid(&attack, A, attacks(&attack, &[]));
    invalid(&attack, A, attacks(&attack, &[(2, B, None)]));
    let both = reply(&attack, attacks(&attack, &[(2, B, None), (3, B, None)])).into_state();
    let defense = combat_frame(&both, false);
    let open = reply(&defense, DecisionReply::DeclareBlockers(vec![])).into_state();
    let next = combat_on(open, 2);
    assert_eq!(next.execution().life_total(B), Some(&14.into()));
    invalid(&next, B, attacks(&next, &[]));
    let strikes = reply(&next, attacks(&next, &[(4, A, None)])).into_state();
    let end = exile_hand::until(strikes, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(end.execution().life_total(A), Some(&16.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_odric_chooses_the_defenders_blocks_only_after_attacking_with_three_other_creatures() {
    let cost = [(ManaType::Colourless, 2), (ManaType::White, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Odric, Master Tactician", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[]),
            creature(3, A, 2, 3, &[]),
            creature(4, A, 2, 3, &[]),
            creature(5, B, 4, 2, &[]),
            creature(6, B, 1, 3, &[]),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    let combat = combat_on(cast, 3);
    for (number, chooser) in [(3, B), (4, A)] {
        let group: Vec<_> = (1..=number).map(|id| (id, B, None)).collect();
        let declared = reply(&combat, attacks(&combat, &group)).into_state();
        let defense = combat_frame(&declared, false);
        assert_eq!(defense.pending_decision().unwrap().chooser(), chooser);
        invalid(
            &defense,
            if chooser == A { B } else { A },
            blocks(&defense, &[(5, 1)]),
        );
        let choice = reply(&defense, blocks(&defense, &[(5, 1), (6, 2)])).into_state();
        let end = exile_hand::until(choice, |s| {
            s.turn().unwrap().position().step == Step::EndCombat
        });
        assert_eq!(physical(&end, 5).zone(), ObjectZone::Graveyard(B));
        assert_eq!(physical(&end, 1).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_portal_mage_redirects_an_attacker_to_a_legal_player_or_planeswalker_before_blocks() {
    use ManaType::*;
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    let source = |n, seat, mana| {
        seed(
            n,
            seat,
            battlefield(seat),
            Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(mana),
                }],
            ),
        )
    };
    let initial = game(
        vec![A, B, C],
        vec![
            named("Portal Mage", 1, A, ObjectDestination::Hand),
            creature(2, A, 4, 4, &[]),
            named("Jace Beleren", 3, A, ObjectDestination::Hand),
            named("Jace Beleren", 4, B, ObjectDestination::Hand),
            source(5, A, [(Colourless, 2.into()), (Blue, 1.into())].into()),
            source(6, B, [(Colourless, 1.into()), (Blue, 2.into())].into()),
            named("Norn's Annex", 7, B, battlefield(B)),
        ],
        &[(Colourless, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let own = round(cast_untargeted(&initial, 3, &[(Colourless, 1), (Blue, 2)]).into_state())
        .into_state();
    let tap = |state: &GameState, id, actor| {
        command(
            state,
            actor,
            GameCommand::ActivateMana {
                source: physical(state, id).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state()
    };
    let our_mana = tap(&own, 5, A);
    let early = round(cast_untargeted(&our_mana, 1, &[(Colourless, 2), (Blue, 1)]).into_state())
        .into_state();
    assert!(early.execution().window().entries().is_empty());
    let b_main = exile_hand::until(own, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let b_mana = tap(&b_main, 6, B);
    let jace = round(
        command(
            &b_mana,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&b_mana, 4).id(),
                payment: Some(spend(&[(Colourless, 1), (Blue, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let declare = combat_on(jace, 4);
    let attacking = reply(&declare, attacks(&declare, &[(2, C, None)])).into_state();
    let paid = tap(&attacking, 5, A);
    let entry =
        round(cast_untargeted(&paid, 1, &[(Colourless, 2), (Blue, 1)]).into_state()).into_state();
    let target = DecisionReply::TriggerTarget(TargetRef::Object(physical(&entry, 2).id()));
    let stack = reply(&entry, target).into_state();
    let choose = round(stack).into_state();
    let target = |n| TargetRef::Object(physical(&choose, n).id());
    for illegal_target in [TargetRef::Player(A), target(3), target(1)] {
        invalid(
            &choose,
            A,
            DecisionReply::ReselectAttackDefender(Some(illegal_target)),
        );
    }
    for (to, defender, walk) in [
        (None, C, None),
        (Some(TargetRef::Player(B)), B, None),
        (Some(target(4)), B, Some(4)),
    ] {
        let moved = reply(&choose, DecisionReply::ReselectAttackDefender(to)).into_state();
        let member = moved
            .execution()
            .combat()
            .unwrap()
            .attacker(physical(&moved, 2).id())
            .unwrap();
        assert_eq!(member.defender(), defender);
        assert_eq!(
            member.defending_planeswalker(),
            walk.map(|n| physical(&moved, n).id())
        );
        assert_eq!(moved.execution().life_total(A), Some(&20.into()));
        let end = exile_hand::until(moved, |s| {
            s.turn().unwrap().position().step == Step::EndCombat
        });
        if walk.is_some() {
            assert_eq!(physical(&end, 4).zone(), ObjectZone::Graveyard(B));
        } else {
            assert_eq!(end.execution().life_total(defender), Some(&16.into()));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_herald_of_the_host_myriad_copies_enter_tapped_attacking_and_leave_at_end_of_combat() {
    use ManaType::*;
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    let cost = [(Colourless, 3), (White, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Herald of the Host", 1, A, ObjectDestination::Hand),
            named("Doubling Season", 2, A, battlefield(A)),
            named("Jace Beleren", 3, B, ObjectDestination::Hand),
            seed(
                4,
                B,
                battlefield(B),
                Program::land(
                    Default::default(),
                    vec![ManaAbility {
                        id: ManaAbilityId(0),
                        output: ManaOutput::Fixed(
                            [(Colourless, 1.into()), (Blue, 2.into())].into(),
                        ),
                    }],
                ),
            ),
            creature(5, C, 3, 3, &[]),
            creature(6, C, 1, 3, &[KeywordAbility::Reach]),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let herald = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    let main = exile_hand::until(herald, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let mana = command(
        &main,
        B,
        GameCommand::ActivateMana {
            source: physical(&main, 4).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let defended = round(
        command(
            &mana,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&mana, 3).id(),
                payment: Some(spend(&[(Colourless, 1), (Blue, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let attack = combat_on(defended, 4);
    let stack = reply(&attack, attacks(&attack, &[(1, C, None)])).into_state();
    assert_eq!(physical(&stack, 1).tapped(), Some(false));
    let offer = round(stack).into_state();
    let RuleFrame::MyriadDefender(frame) = offer.pending_decision().unwrap().frame() else {
        panic!("myriad");
    };
    let walker = TargetRef::Object(physical(&offer, 3).id());
    assert_eq!(frame.opponent(), B);
    assert_eq!(
        frame.defenders().collect::<BTreeSet<_>>(),
        [walker, TargetRef::Player(B)].into()
    );
    assert!(frame.may_decline());
    let skipped = reply(&offer, DecisionReply::MyriadDefender(None)).into_state();
    assert_eq!(skipped.execution().combat().unwrap().attackers().count(), 1);
    invalid(
        &offer,
        A,
        DecisionReply::MyriadDefender(Some(TargetRef::Player(C))),
    );
    let twice = reply(&offer, DecisionReply::MyriadDefender(Some(walker))).into_state();
    invalid(&twice, A, DecisionReply::MyriadDefender(None));
    let order = reply(
        &twice,
        DecisionReply::MyriadDefender(Some(TargetRef::Player(B))),
    )
    .into_state();
    let RuleFrame::TimestampOrder(frame) = order.pending_decision().unwrap().frame() else {
        panic!("copy order");
    };
    let created = reply(
        &order,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    let copies = tokens(&created, A, 2);
    let state = created.into_state();
    let mut defenders = BTreeSet::new();
    for &id in &copies {
        let copy = state.execution().window().cards().get(id).unwrap();
        assert_eq!(copy.tapped(), Some(true));
        let view = state
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert!(
            view.current_names
                .iter()
                .any(|name| name.as_str() == "Herald of the Host")
        );
        assert!(
            view.has_keyword(KeywordAbility::Flying) && view.has_keyword(KeywordAbility::Vigilance)
        );
        let member = state.execution().combat().unwrap().attacker(id).unwrap();
        assert_eq!(member.defender(), B);
        defenders.insert(member.defending_planeswalker());
    }
    assert_eq!(defenders, [None, Some(physical(&state, 3).id())].into());
    let blocks = combat_frame(&state, false);
    invalid(&blocks, C, self::blocks(&blocks, &[(5, 1)]));
    let open = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
    let done = exile_hand::until(open, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(done.execution().life_total(B), Some(&16.into()));
    assert_eq!(done.execution().life_total(C), Some(&16.into()));
    assert_eq!(physical(&done, 3).zone(), ObjectZone::Graveyard(B));
    for id in copies {
        assert!(done.execution().window().cards().get(id).is_none());
    }
    assert_eq!(physical(&done, 1).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spectral_grasp_and_sandwurm_convergence_restrict_only_the_relevant_attacks_and_blocks() {
    use ManaType::*;
    let initial = game(
        vec![A, B, C],
        vec![
            named("Spectral Grasp", 1, A, ObjectDestination::Hand),
            named("Sandwurm Convergence", 2, A, ObjectDestination::Hand),
            named("Jace Beleren", 3, A, ObjectDestination::Hand),
            creature(4, B, 1, 3, &[]),
            creature(5, B, 1, 3, &[KeywordAbility::Flying]),
            creature(6, B, 1, 3, &[]),
            creature(7, A, 2, 3, &[KeywordAbility::Haste]),
            creature(8, C, 2, 3, &[]),
        ],
        &[(Colourless, 8), (White, 1), (Blue, 2), (Green, 2)],
        Step::PrecombatMain,
    );
    let jace = round(cast_untargeted(&initial, 3, &[(Colourless, 1), (Blue, 2)]).into_state())
        .into_state();
    let enchantment =
        round(cast_untargeted(&jace, 2, &[(Colourless, 6), (Green, 2)]).into_state()).into_state();
    let aura = round(
        command(
            &enchantment,
            A,
            GameCommand::CastWithResources {
                spell: physical(&enchantment, 1).id(),
                target: Some(TargetRef::Object(physical(&enchantment, 4).id())),
                payment: CastingPayment {
                    mana: Some(spend(&[(Colourless, 1), (White, 1)])),
                    ..Default::default()
                },
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(
        aura.execution().attached_to(physical(&aura, 1).id()),
        Some(TargetRef::Object(physical(&aura, 4).id()))
    );
    let a_attack = combat_frame(&aura, true);
    let attacked = reply(&a_attack, attacks(&a_attack, &[(7, B, None)])).into_state();
    let defense = combat_frame(&attacked, false);
    invalid(&defense, B, blocks(&defense, &[(4, 7)]));
    let defended = reply(&defense, blocks(&defense, &[(6, 7)])).into_state();
    let b_attack = combat_on(defended, 2);
    let tokens: Vec<_> = b_attack
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .collect();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].controller(), Some(A));
    let wurm = b_attack
        .execution()
        .current_characteristics(tokens[0].id())
        .unwrap()
        .unwrap();
    assert_eq!(
        wurm.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Wurm").unwrap()].into()
    );
    assert_eq!(wurm.current_colors, [Color::Green].into());
    assert_eq!(
        wurm.power_toughness.unwrap().derived,
        PowerToughness {
            power: 5.into(),
            toughness: 5.into()
        }
    );
    for attacker in [4, 5] {
        for walker in [None, Some(3)] {
            invalid(&b_attack, B, attacks(&b_attack, &[(attacker, A, walker)]));
        }
        let free = reply(&b_attack, attacks(&b_attack, &[(attacker, C, None)])).into_state();
        assert_eq!(physical(&free, attacker).tapped(), Some(true));
    }
    let ground = reply(&b_attack, attacks(&b_attack, &[(6, A, Some(3))])).into_state();
    assert_eq!(physical(&ground, 6).tapped(), Some(true));
    let quiet = reply(&b_attack, DecisionReply::DeclareAttackers(vec![])).into_state();
    let c_attack = combat_on(quiet, 3);
    let swing = reply(&c_attack, attacks(&c_attack, &[(8, B, None)])).into_state();
    let allowed = combat_frame(&swing, false);
    let blocked = reply(&allowed, blocks(&allowed, &[(4, 8)])).into_state();
    assert!(
        blocked
            .execution()
            .combat()
            .unwrap()
            .blockers()
            .any(|block| block.combatant().object() == physical(&blocked, 4).id())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sun_quan_grants_horsemanship_only_to_own_creatures_and_shifting_sliver_restricts_sliver_blocks()
 {
    use ManaType::*;
    let sliver = |id, seat| {
        let mut program = Program::creature(
            None,
            2.into(),
            3.into(),
            [current_catalogue().1.resolve_label("Sliver").unwrap()].into(),
        );
        program.printed_keywords.insert(KeywordAbility::Haste);
        seed(id, seat, battlefield(seat), program)
    };
    for (name, cost, grants_horsemanship) in [
        (
            "Sun Quan, Lord of Wu",
            vec![(Colourless, 4), (Blue, 2)],
            true,
        ),
        ("Shifting Sliver", vec![(Colourless, 3), (Blue, 1)], false),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                sliver(2, A),
                creature(3, A, 2, 3, &[KeywordAbility::Haste]),
                sliver(4, B),
                creature(5, B, 2, 3, &[KeywordAbility::Horsemanship]),
                creature(6, B, 1, 3, &[KeywordAbility::Flying]),
            ],
            &cost,
            Step::PrecombatMain,
        );
        let ready = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
        let view = |n| {
            ready
                .execution()
                .current_characteristics(physical(&ready, n).id())
                .unwrap()
                .unwrap()
        };
        if grants_horsemanship {
            for n in [1, 2, 3] {
                assert!(view(n).has_keyword(KeywordAbility::Horsemanship));
            }
            for n in [4, 6] {
                assert!(!view(n).has_keyword(KeywordAbility::Horsemanship));
            }
        }
        let attack_frame = combat_frame(&ready, true);
        let swinging = reply(
            &attack_frame,
            attacks(&attack_frame, &[(2, B, None), (3, B, None)]),
        )
        .into_state();
        let defense = combat_frame(&swinging, false);
        let (legal, illegal_block) = if grants_horsemanship { (5, 4) } else { (4, 5) };
        invalid(&defense, B, blocks(&defense, &[(illegal_block, 2)]));
        invalid(&defense, B, blocks(&defense, &[(6, 2)]));
        let result = reply(&defense, blocks(&defense, &[(legal, 2)])).into_state();
        assert_eq!(result.execution().combat().unwrap().blockers().count(), 1);
        if !grants_horsemanship {
            let ordinary = reply(&defense, blocks(&defense, &[(6, 3)])).into_state();
            assert_eq!(ordinary.execution().combat().unwrap().blockers().count(), 1);
            let b_turn = combat_on(
                reply(&attack_frame, DecisionReply::DeclareAttackers(vec![])).into_state(),
                2,
            );
            let opponent = reply(&b_turn, attacks(&b_turn, &[(4, A, None)])).into_state();
            let defend = combat_frame(&opponent, false);
            invalid(&defend, A, blocks(&defend, &[(3, 4)]));
            let sliver_block = reply(&defend, blocks(&defend, &[(2, 4)])).into_state();
            assert_eq!(
                sliver_block
                    .execution()
                    .combat()
                    .unwrap()
                    .blockers()
                    .count(),
                1
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hellkite_charger_can_pay_while_its_trigger_resolves_to_untap_attackers_and_attack_again() {
    use ManaType::*;
    use spellwake_engine::{
        effects::ResolutionPaymentReply,
        mana::{ManaAbility, ManaOutput},
    };
    let price = [(Colourless, 4), (Red, 2)];
    let extra = [(Colourless, 5), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Hellkite Charger", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[KeywordAbility::Haste]),
            creature(3, B, 1, 3, &[]),
            creature(4, B, 1, 3, &[KeywordAbility::Reach]),
            seed(
                5,
                A,
                battlefield(A),
                Program::land(
                    Default::default(),
                    vec![ManaAbility {
                        id: ManaAbilityId(0),
                        output: ManaOutput::Fixed(extra.map(|(kind, n)| (kind, n.into())).into()),
                    }],
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let dragon = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let attack = combat_frame(&dragon, true);
    let attack_both =
        |s: &GameState| reply(s, attacks(s, &[(1, B, None), (2, B, None)])).into_state();
    let triggered = attack_both(&attack);
    assert_eq!(physical(&triggered, 1).tapped(), Some(true));
    let offer = round(triggered).into_state();
    let decline = reply(
        &offer,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    assert_eq!(physical(&decline, 2).tapped(), Some(true));
    let declined_end = exile_hand::until(decline, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
            || matches!(
                s.pending_decision().map(|d| d.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    assert_eq!(
        declined_end.turn().unwrap().position().step,
        Step::PostcombatMain
    );
    let funded = command(
        &offer,
        A,
        GameCommand::ActivateMana {
            source: physical(&offer, 5).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let paid = reply(
        &funded,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&extra))),
    )
    .into_state();
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    assert_eq!(paid.execution().mana(A, Red), 0.into());
    for n in [1, 2] {
        assert_eq!(physical(&paid, n).tapped(), Some(false));
    }
    let defense = combat_frame(&paid, false);
    invalid(&defense, B, blocks(&defense, &[(3, 1)]));
    let open = reply(&defense, blocks(&defense, &[])).into_state();
    let second = combat_on(open, 1);
    assert_eq!(second.execution().life_total(B), Some(&13.into()));
    let repeated = attack_both(&second);
    let second_offer = round(repeated).into_state();
    let declined = reply(
        &second_offer,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    let done = exile_hand::until(declined, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(done.execution().life_total(B), Some(&6.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_scourge_of_the_throne_dethrones_at_a_tie_and_adds_combat_only_on_its_first_attack() {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Scourge of the Throne", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 3, &[]),
            creature(4, B, 2, 2, &[]),
            creature(5, B, 2, 3, &[KeywordAbility::Reach]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PLAYER,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(1),
                        false,
                    ),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let not_highest = round(targeted(&cast, A, 3, TargetRef::Player(B)).into_state()).into_state();
    let low = combat_on(not_highest, 4);
    let normal = reply(&low, attacks(&low, &[(1, B, None)])).into_state();
    assert!(normal.execution().window().entries().is_empty());
    assert_eq!(physical(&normal, 1).tapped(), Some(true));
    let defense = combat_frame(&normal, false);
    invalid(&defense, B, blocks(&defense, &[(4, 1)]));
    let reach = reply(&defense, blocks(&defense, &[(5, 1)])).into_state();
    assert_eq!(reach.execution().combat().unwrap().blockers().count(), 1);
    let first = combat_on(cast, 4);
    let order = reply(&first, attacks(&first, &[(1, B, None), (2, B, None)])).into_state();
    let RuleFrame::Triggers(frame) = order.pending_decision().unwrap().frame() else {
        panic!("dethrone and first attack");
    };
    let stack = reply(
        &order,
        DecisionReply::OrderTriggers(frame.instances().to_vec()),
    )
    .into_state();
    let reverse = round(round(stack).into_state()).into_state();
    for n in [1, 2] {
        assert_eq!(physical(&reverse, n).tapped(), Some(false));
    }
    let counters = |s: &GameState| {
        s.execution()
            .power_toughness_counters(physical(s, 1).id())
            .unwrap()
            .plus_one
            .clone()
    };
    assert_eq!(counters(&reverse), 1.into());
    let second = combat_on(reverse, 4);
    assert_eq!(second.execution().life_total(B), Some(&13.into()));
    let stack = reply(&second, attacks(&second, &[(1, C, None), (2, C, None)])).into_state();
    assert_eq!(stack.execution().window().entries().len(), 1);
    let dethroned = round(stack).into_state();
    assert_eq!(counters(&dethroned), 2.into());
    assert_eq!(physical(&dethroned, 1).tapped(), Some(true));
    let ended = exile_hand::until(dethroned, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
            || matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    assert_eq!(ended.turn().unwrap().position().step, Step::PostcombatMain);
    assert_eq!(ended.execution().life_total(C), Some(&12.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_godo_optionally_finds_equipment_and_untaps_only_itself_and_own_samurai_for_the_extra_combat()
{
    use ManaType::*;
    use spellwake_engine::effects::{LibraryTopReply, OptionalEffectChoice};
    let price = [(Colourless, 5), (Red, 1)];
    let samurai = Program::creature(
        None,
        1.into(),
        3.into(),
        [current_catalogue().1.resolve_label("Samurai").unwrap()].into(),
    );
    let start = game(
        vec![A, B],
        vec![
            named("Godo, Bandit Warlord", 1, A, ObjectDestination::Hand),
            seed(2, A, battlefield(A), samurai),
            creature(3, A, 1, 3, &[]),
            named("Fireshrieker", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let offer = round(cast).into_state();
    let skip = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&skip, 4).zone(), ObjectZone::Library(A));
    let choose = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let found = reply(
        &choose,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&choose, 4).id()),
        }),
    )
    .into_state();
    assert_eq!(physical(&found, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(
        found.execution().attached_to(physical(&found, 4).id()),
        None
    );
    let first = combat_on(found, 3);
    let stacked = reply(
        &first,
        attacks(&first, &[(1, B, None), (2, B, None), (3, B, None)]),
    )
    .into_state();
    for n in 1..4 {
        assert_eq!(physical(&stacked, n).tapped(), Some(true));
    }
    let untapped = round(stacked).into_state();
    for n in 1..4 {
        assert_eq!(physical(&untapped, n).tapped(), Some(n == 3));
    }
    let second = combat_on(untapped, 3);
    let final_swing = reply(&second, attacks(&second, &[(1, B, None), (2, B, None)])).into_state();
    assert!(final_swing.execution().window().entries().is_empty());
    let done = exile_hand::until(final_swing, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(done.execution().life_total(B), Some(&11.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_savage_beating_checks_own_combat_timing_and_resolves_each_mode_or_both_when_entwined() {
    use ManaType::*;
    use spellwake_engine::mana::{CastMode, ManaAbility, ManaOutput};
    let supply = [(Colourless, 4), (Red, 3)];
    let start = game(
        vec![A, B],
        vec![
            named("Savage Beating", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[KeywordAbility::Haste]),
            seed(
                3,
                A,
                battlefield(A),
                Program::land(
                    Default::default(),
                    vec![ManaAbility {
                        id: ManaAbilityId(0),
                        output: ManaOutput::Fixed(supply.map(|(c, n)| (c, n.into())).into()),
                    }],
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let fund = |s: &GameState| {
        command(
            s,
            A,
            GameCommand::ActivateMana {
                source: physical(s, 3).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state()
    };
    let cast = |s: &GameState, modes: &[u8], entwine| GameCommand::CastWithResources {
        spell: physical(s, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(if entwine {
                &supply
            } else {
                &[(Colourless, 3), (Red, 2)]
            })),
            modes: modes
                .iter()
                .map(|&mode| CastMode {
                    mode,
                    target: None,
                    targets: vec![],
                })
                .collect(),
            additional: if entwine {
                [(0, 1.into())].into()
            } else {
                Default::default()
            },
            ..Default::default()
        },
    };
    let too_early = fund(&start);
    illegal(&too_early, A, cast(&too_early, &[0], false));
    let declare = combat_frame(&start, true);
    let attack = reply(&declare, attacks(&declare, &[(2, B, None)])).into_state();
    let mana = fund(&attack);
    illegal(&mana, A, cast(&mana, &[0, 1], false));
    for (choices, extra, strike) in [
        (vec![0], false, true),
        (vec![1], true, false),
        (vec![0, 1], true, true),
    ] {
        let entwine = choices.len() == 2;
        let spell =
            round(command(&mana, A, cast(&mana, &choices, entwine)).into_state()).into_state();
        let view = spell
            .execution()
            .current_characteristics(physical(&spell, 2).id())
            .unwrap()
            .unwrap();
        assert_eq!(view.has_keyword(KeywordAbility::DoubleStrike), strike);
        assert_eq!(physical(&spell, 2).tapped(), Some(!extra));
        let end = if extra {
            let second = combat_on(spell, 1);
            reply(&second, attacks(&second, &[(2, B, None)])).into_state()
        } else {
            spell
        };
        let done = exile_hand::until(end, |s| {
            s.turn().unwrap().position().step == Step::PostcombatMain
        });
        assert_eq!(
            done.execution().life_total(B),
            Some(&(if extra && strike { 12 } else { 16 }).into())
        );
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
    }
}
