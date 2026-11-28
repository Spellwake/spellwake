//! Named cases using the current-profile Oracle registry and normal paid game commands.
use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility::*,
    effects::LibraryTopReply,
    mana::{ManaAbilityId, ManaAbilityRef},
};
pub(super) fn named(
    name: &str,
    number: u64,
    owner: SeatId,
    destination: ObjectDestination,
) -> SeedCard {
    let (inventory, _, registry) = current_catalogue();
    let row = inventory
        .cards
        .iter()
        .find(|r| r.name == name && r.required_by_base_formats)
        .unwrap();
    let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    registry
        .seed(&oracle, CardId(number), owner, destination)
        .unwrap()
}
fn hand(name: &str) -> SeedCard {
    named(name, 1, A, ObjectDestination::Hand)
}
pub(super) fn creature(
    n: u64,
    owner: SeatId,
    power: i64,
    toughness: i64,
    keywords: &[KeywordAbility],
) -> SeedCard {
    let mut program = Program::creature(None, power.into(), toughness.into(), BTreeSet::new());
    program.printed_keywords.extend(keywords);
    seed(
        n,
        owner,
        ObjectDestination::Battlefield { controller: owner },
        program,
    )
}
fn cast_named(state: &GameState, price: &[(ManaType, i64)]) -> GameState {
    let cast = cast_untargeted(state, 1, price);
    for &(kind, amount) in price {
        assert_eq!(
            cast.state().execution().mana(A, kind),
            state.execution().mana(A, kind).minus(&amount.into())
        );
    }
    round(cast.into_state()).into_state()
}
pub(super) fn illegal(state: &GameState, actor: SeatId, command: GameCommand) {
    assert!(
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command
            })
            .is_err()
    );
}
fn next_main(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |state| {
        let p = state.turn().unwrap().position();
        p.number == turn && p.step == Step::PrecombatMain
    })
}
fn combat_declaration(state: &GameState, choice: DecisionReply) -> GameCommand {
    GameCommand::Reply {
        decision: state.pending_decision().unwrap().id(),
        choice,
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_permanent_games_cast_then_tap_for_exact_printed_mana() {
    use ManaType::*;
    for (name, price, amount, fixed, is_creature) in [
        ("Elvish Mystic", (Green, 1), 1, Some(Green), true),
        ("Palladium Myr", (Colourless, 3), 2, Some(Colourless), true),
        ("Thran Dynamo", (Colourless, 4), 3, Some(Colourless), false),
        ("Gilded Lotus", (Colourless, 5), 3, None, false),
        ("Ornithopter of Paradise", (Colourless, 2), 1, None, true),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                creature(2, B, 2, 2, &[]),
                creature(3, B, 1, 2, &[Reach]),
            ],
            &[price],
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &[price]);
        let tap = GameCommand::ActivateMana {
            source: physical(&entered, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        };
        let state = if is_creature {
            illegal(&entered, A, tap.clone());
            next_main(entered, 3)
        } else {
            entered
        };
        if name == "Ornithopter of Paradise" {
            check_flying_blocks(&state, 2, 3);
        }
        let activated = command(&state, A, tap.clone()).into_state();
        let colors = if let Some(one) = fixed {
            vec![one]
        } else {
            vec![White, Blue, Black, Red, Green]
        };
        if fixed.is_none() {
            let RuleFrame::ManaColor(frame) = activated.pending_decision().unwrap().frame() else {
                panic!("mana color")
            };
            assert_eq!(frame.colors(), &colors.iter().copied().collect());
            illegal(
                &activated,
                A,
                combat_declaration(&activated, DecisionReply::ManaColor(Colourless)),
            );
        }
        for color in colors {
            let paid = if fixed.is_some() {
                activated.clone()
            } else {
                reply(&activated, DecisionReply::ManaColor(color)).into_state()
            };
            for k in ManaType::ALL {
                assert_eq!(
                    paid.execution().mana(A, k),
                    state
                        .execution()
                        .mana(A, k)
                        .plus(&(if k == color { amount } else { 0_i64 }).into())
                );
            }
            assert!(physical(&paid, 1).tapped().unwrap());
            illegal(&paid, A, tap.clone());
        }
    }
}

fn attack(state: &GameState, attacker: u64, defender: SeatId) -> GameState {
    let choice = combat_frame(state, true);
    reply(
        &choice,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&choice, attacker).id(),
            defender,
            defending_planeswalker: None,
        }]),
    )
    .into_state()
}
fn blockers(attacked: &GameState) -> GameState {
    combat_frame(attacked, false)
}
fn block_choice(state: &GameState, blocker: u64, attacker: u64) -> DecisionReply {
    DecisionReply::DeclareBlockers(vec![BlockDeclaration {
        blocker: physical(state, blocker).id(),
        attacker: physical(state, attacker).id(),
    }])
}
fn check_flying_blocks(state: &GameState, ground: u64, reach: u64) {
    let blocks = blockers(&attack(state, 1, B));
    illegal(
        &blocks,
        B,
        combat_declaration(&blocks, block_choice(&blocks, ground, 1)),
    );
    reply(&blocks, block_choice(&blocks, reach, 1));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spell_games_spirited_companion_and_reverse_engineer() {
    use ManaType::*;
    let deck = |name: &str| {
        vec![
            hand(name),
            seed(
                2,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ]
    };
    let price = [(Colourless, 1), (White, 1)];
    let before = game(
        vec![A, B],
        deck("Spirited Companion"),
        &price,
        Step::PrecombatMain,
    );
    let top = before.execution().window().cards().library(A)[0];
    let card = before
        .execution()
        .window()
        .cards()
        .get(top)
        .unwrap()
        .card_id()
        .unwrap();
    let entered = cast_named(&before, &price);
    assert_eq!(entered.execution().window().entries().len(), 1);
    let creature = entered
        .execution()
        .current_characteristics(physical(&entered, 1).id())
        .unwrap()
        .unwrap();
    assert_eq!(
        creature.current_types,
        [CardType::Creature, CardType::Enchantment].into()
    );
    assert_eq!(
        creature.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    assert_eq!(entered.execution().window().cards().library(A).len(), 3);
    let draw = round(entered).into_state();
    assert_eq!(physical(&draw, card.0).zone(), ObjectZone::Hand(A));
    assert_eq!(draw.execution().window().cards().library(A).len(), 2);

    let mut cards = deck("Reverse Engineer");
    cards.extend((5..8).map(|id| {
        seed(
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        )
    }));
    let before = game(vec![A, B], cards, &[(Blue, 2)], Step::PrecombatMain);
    let resources = (5..8)
        .map(|id| CastResource::Improvise(physical(&before, id).id()))
        .collect();
    let cast = command(
        &before,
        A,
        GameCommand::CastWithResources {
            spell: physical(&before, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Blue, 2)])),
                objects: resources,
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(cast.execution().mana(A, Blue), 0.into());
    for id in 5..8 {
        assert!(physical(&cast, id).tapped().unwrap());
    }
    let result = round(cast).into_state();
    for id in [2, 3, 100] {
        assert_eq!(physical(&result, id).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(&result, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spell_games_read_the_bones_scry_order_draw_and_life() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 1)];
    let mut cards = vec![hand("Read the Bones")];
    cards.extend((2..5).map(|id| {
        seed(
            id,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let scry = cast_named(&start, &price);
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        panic!("expected scry")
    };
    let order = scry.execution().window().cards().library(A);
    assert_eq!(frame.domain_top_first(), &order[..2]);
    for (top, bottom, drawn) in [
        (vec![order[1], order[0]], vec![], vec![order[1], order[0]]),
        (vec![order[1]], vec![order[0]], vec![order[1], order[2]]),
    ] {
        let result = reply(
            &scry,
            DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: top,
                bottom_top_first: bottom,
            }),
        )
        .into_state();
        for object in drawn {
            let card = scry
                .execution()
                .window()
                .cards()
                .get(object)
                .unwrap()
                .card_id()
                .unwrap();
            assert_eq!(physical(&result, card.0).zone(), ObjectZone::Hand(A));
        }
        assert_eq!(result.execution().life_total(A), Some(&18.into()));
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Graveyard(A));
    }
}

pub(super) fn remove_spell(n: u64, owner: SeatId) -> SeedCard {
    seed(
        n,
        owner,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::SINGLE_CREATURE,
            crate::keyword_creatures::toolbox_emit::zone_op(
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
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spell_games_counterspell_revalidates_the_targeted_stack_entry() {
    let subject = seed(
        2,
        A,
        ObjectDestination::Hand,
        Program::artifact(Some(ManaCost::free()), vec![]),
    );
    let second_counter = seed(
        3,
        B,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
    );
    let before = game(
        vec![A, B],
        vec![hand("Counterspell"), subject, second_counter],
        &[(ManaType::Blue, 2)],
        Step::PrecombatMain,
    );
    let target = cast_untargeted(&before, 2, &[]).into_state();
    let stack = target.execution().window().entries()[0].id();
    let cast = command(
        &target,
        A,
        GameCommand::CastPaid {
            spell: physical(&target, 1).id(),
            target: TargetRef::Stack(stack),
            payment: spend(&[(ManaType::Blue, 2)]),
        },
    )
    .into_state();
    assert_eq!(cast.execution().mana(A, ManaType::Blue), 0.into());
    for intervene in [false, true] {
        let ready = if intervene {
            let priority = pass(&cast).into_state();
            let response = targeted(&priority, B, 3, TargetRef::Stack(stack));
            let removed = round(response.into_state()).into_state();
            assert_eq!(physical(&removed, 2).zone(), ObjectZone::Graveyard(A));
            assert_eq!(removed.execution().window().entries().len(), 1);
            removed
        } else {
            cast.clone()
        };
        let done = round(ready).into_state();
        assert!(done.execution().window().entries().is_empty());
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_permanent_games_ministrant_dies_and_both_afterlife_spirits_fly() {
    let price = [(ManaType::Colourless, 2), (ManaType::White, 1)];
    let start = game(
        vec![A, B],
        vec![
            hand("Ministrant of Obligation"),
            remove_spell(2, A),
            creature(3, B, 2, 2, &[]),
            creature(4, B, 1, 1, &[Reach]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&start, &price);
    let dead = round(
        targeted(
            &entered,
            A,
            2,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
    let ordering = round(dead).into_state();
    let Some(RuleFrame::TimestampOrder(frame)) = ordering.pending_decision().map(|d| d.frame())
    else {
        panic!("expected simultaneous Spirit entry");
    };
    let created = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().rev().map(|m| m.member).collect()),
    );
    let spirits = tokens(&created, A, 2);
    let vocabulary = current_catalogue().1;
    for &id in &spirits {
        let s = created
            .state()
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert_eq!(s.current_colors, [Color::White, Color::Black].into());
        assert_eq!(
            s.current_creature_subtypes,
            [vocabulary.resolve_label("Spirit").unwrap()].into()
        );
        assert_eq!(
            s.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
        assert!(s.keywords.contains(&Flying));
    }
    let start = next_main(created.into_state(), 3);
    let attacking = combat_frame(&start, true);
    for spirit in spirits {
        let attacked = reply(
            &attacking,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: spirit,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        let blocks = blockers(&attacked);
        let declare = |n| {
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: physical(&blocks, n).id(),
                attacker: spirit,
            }])
        };
        illegal(&blocks, B, combat_declaration(&blocks, declare(3)));
        reply(&blocks, declare(4));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_creature_games_flying_haste_vigilance_strike_and_lifelink() {
    use ManaType::*;
    for (name, generic, color, pips, power, haste, vigilance, steps, lifelink) in [
        ("Goliath Sphinx", 5, Blue, 2, 8, false, false, 1, false),
        ("Rorix Bladewing", 3, Red, 3, 6, true, false, 1, false),
        ("Sunblade Angel", 5, White, 1, 3, false, true, 1, true),
        ("Fencing Ace", 1, White, 1, 1, false, false, 2, false),
        (
            "Zetalpa, Primal Dawn",
            6,
            White,
            2,
            4,
            false,
            true,
            2,
            false,
        ),
    ] {
        let price = [(Colourless, generic), (color, pips)];
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                creature(2, B, 1, 1, &[]),
                creature(3, B, 3, 1, &[Reach]),
                remove_spell(4, A),
            ],
            &price,
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &price);
        let state = if haste {
            entered
        } else {
            next_main(entered, 3)
        };
        if name != "Fencing Ace" {
            check_flying_blocks(&state, 2, 3);
        }
        let attacked = attack(&state, 1, B);
        assert_eq!(physical(&attacked, 1).tapped(), Some(!vigilance));
        let blocks = blockers(&attacked);
        let no_blocks = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
        let mut s = no_blocks;
        let mut hits = 0;
        while s.turn().unwrap().position().step != Step::EndCombat {
            let step = pass(&s);
            hits += step
                .damage_journal()
                .applied
                .iter()
                .filter(|d| {
                    d.source.id() == DamageSourceId::Object(physical(&state, 1).id())
                        && d.target == DamageRecipient::Player(B)
                })
                .count();
            s = step.into_state();
        }
        assert_eq!(hits, steps, "{name}");
        assert_eq!(
            s.execution().life_total(B),
            Some(&(20_i64 - power * steps as i64).into())
        );
        assert_eq!(
            s.execution().life_total(A),
            Some(&(20_i64 + if lifelink { power } else { 0 }).into())
        );
        if name == "Sunblade Angel" {
            let blocked = reply(&blocks, block_choice(&blocks, 3, 1)).into_state();
            let first = round(blocked).into_state();
            assert_eq!(physical(&first, 3).zone(), ObjectZone::Graveyard(B));
            assert_eq!(
                first
                    .execution()
                    .battlefield_marked_damage(physical(&first, 1).id()),
                Some(0.into())
            );
        }
        if name == "Zetalpa, Primal Dawn" {
            let destroyed = round(
                targeted(&state, A, 4, TargetRef::Object(physical(&state, 1).id())).into_state(),
            )
            .into_state();
            assert_eq!(physical(&destroyed, 1).zone(), ObjectZone::Battlefield);
            let blocked = reply(&blocks, block_choice(&blocks, 3, 1)).into_state();
            let assigning = round(blocked).into_state();
            let source = physical(&assigning, 1).id();
            let blocker = physical(&assigning, 3).id();
            use spellwake_engine::combat::CombatDamageAssignment;
            let assigned = reply(
                &assigning,
                DecisionReply::AssignCombatDamage(vec![
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Creature(blocker),
                        amount: 1.into(),
                    },
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Player(B),
                        amount: 3.into(),
                    },
                ]),
            )
            .into_state();
            assert_eq!(assigned.execution().life_total(B), Some(&17.into()));
            assert_eq!(physical(&assigned, 3).zone(), ObjectZone::Graveyard(B));
            let end = exile_hand::until(assigned, |s| {
                s.turn().unwrap().position().step == Step::EndCombat
            });
            assert_eq!(end.execution().life_total(B), Some(&13.into()));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_creature_games_reach_deathtouch_vigilance_defender_and_shroud() {
    use ManaType::*;
    for (name, price) in [
        ("Deadly Recluse", vec![(Colourless, 1), (Green, 1)]),
        ("Skysnare Spider", vec![(Colourless, 4), (Green, 2)]),
        (
            "Wall of Denial",
            vec![(Colourless, 1), (White, 1), (Blue, 1)],
        ),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                creature(2, B, 1, 8, &[Flying]),
                remove_spell(3, A),
                remove_spell(4, B),
            ],
            &price,
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &price);
        if name == "Wall of Denial" {
            let target = TargetRef::Object(physical(&entered, 1).id());
            illegal(
                &entered,
                A,
                GameCommand::Cast {
                    spell: physical(&entered, 3).id(),
                    target,
                },
            );
            let other = pass(&entered).into_state();
            illegal(
                &other,
                B,
                GameCommand::Cast {
                    spell: physical(&other, 4).id(),
                    target,
                },
            );
            let turn = next_main(entered.clone(), 3);
            // A legal attacker ensures an actual decision is offered if this wall is excluded.
            let attacks = exile_hand::until(turn, |s| {
                matches!(
                    s.pending_decision().map(|d| d.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                ) || s.turn().unwrap().position().step == Step::EndCombat
            });
            if let Some(d) = attacks.pending_decision() {
                let RuleFrame::Combat(CombatDecision::Attackers(frame)) = d.frame() else {
                    panic!("attack frame")
                };
                assert!(
                    !frame
                        .eligible_attackers()
                        .contains(&physical(&attacks, 1).id())
                );
            } else {
                assert_eq!(attacks.turn().unwrap().position().step, Step::EndCombat);
            }
        } else if name == "Skysnare Spider" {
            let turn = next_main(entered.clone(), 3);
            assert!(!physical(&attack(&turn, 1, B), 1).tapped().unwrap());
        }
        let turn = next_main(entered, 2);
        let attacks = attack(&turn, 2, A);
        let blocks = blockers(&attacks);
        let blocked = reply(&blocks, block_choice(&blocks, 1, 2)).into_state();
        let damage = round(blocked).into_state();
        assert_eq!(damage.execution().life_total(A), Some(&20.into()));
        assert_eq!(
            physical(&damage, 2).zone(),
            if name == "Deadly Recluse" {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

fn trample_into_one_toughness(
    attacked: &GameState,
    attacker: u64,
    blocker: u64,
    power: i64,
) -> GameState {
    use spellwake_engine::combat::CombatDamageAssignment;
    let blocks = blockers(attacked);
    let blocked = reply(&blocks, block_choice(&blocks, blocker, attacker)).into_state();
    let choosing = round(blocked).into_state();
    let source = physical(&choosing, attacker).id();
    let recipient = physical(&choosing, blocker).id();
    let before = choosing.execution().life_total(B).unwrap().clone();
    let damage = reply(
        &choosing,
        DecisionReply::AssignCombatDamage(vec![
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Creature(recipient),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Player(B),
                amount: (power - 1).into(),
            },
        ]),
    )
    .into_state();
    assert_eq!(
        damage.execution().life_total(B),
        Some(&before.minus(&(power - 1).into()))
    );
    assert_eq!(physical(&damage, blocker).zone(), ObjectZone::Graveyard(B));
    damage
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_permanent_games_equipment_costs_targets_transfers_and_combat() {
    use spellwake_engine::activated::ActivatedAbilityId;
    for (name, price, equip, keys) in [
        ("Fireshrieker", 3, 2, vec![DoubleStrike]),
        ("Haunted Cloak", 3, 1, vec![Vigilance, Trample, Haste]),
        ("Lightning Greaves", 2, 0, vec![Haste, Shroud]),
        ("Vulshok Battlegear", 3, 3, vec![]),
    ] {
        let mut host = creature(2, A, 3, 5, &[]);
        if keys.contains(&Haste) {
            host = seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::creature(Some(ManaCost::free()), 3.into(), 5.into(), BTreeSet::new()),
            );
        }
        let start = game(
            vec![A, B],
            vec![
                hand(name),
                host,
                creature(3, A, 2, 3, &[]),
                creature(4, B, 1, 1, &[]),
                remove_spell(5, A),
                remove_spell(6, B),
            ],
            &[(ManaType::Colourless, price + 2 * equip)],
            Step::PrecombatMain,
        );
        let entered = cast_named(&start, &[(ManaType::Colourless, price)]);
        let state = if keys.contains(&Haste) {
            round(cast_untargeted(&entered, 2, &[]).into_state()).into_state()
        } else {
            entered
        };
        let ability = |s: &GameState, host| GameCommand::ActivateAbility {
            source: physical(s, 1).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(physical(s, host).id()),
            payment: (equip > 0).then(|| spend(&[(ManaType::Colourless, equip)])),
        };
        illegal(&state, A, ability(&state, 4));
        let activated = command(&state, A, ability(&state, 2)).into_state();
        assert_eq!(
            activated.execution().mana(A, ManaType::Colourless),
            equip.into()
        );
        assert!(
            activated
                .execution()
                .attached_to(physical(&activated, 1).id())
                .is_none()
        );
        if name == "Vulshok Battlegear" {
            let gone = round(
                targeted(
                    &activated,
                    A,
                    5,
                    TargetRef::Object(physical(&activated, 2).id()),
                )
                .into_state(),
            )
            .into_state();
            let done = round(gone).into_state();
            assert_eq!(done.execution().attached_to(physical(&done, 1).id()), None);
        }
        let attached = round(activated).into_state();
        let props = |s: &GameState, n| {
            s.execution()
                .current_characteristics(physical(s, n).id())
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            attached
                .execution()
                .attached_to(physical(&attached, 1).id()),
            Some(TargetRef::Object(physical(&attached, 2).id()))
        );
        assert_eq!(props(&attached, 2).keywords, keys.iter().copied().collect());
        let bonus = if name == "Vulshok Battlegear" { 3 } else { 0 };
        assert_eq!(
            props(&attached, 2).power_toughness.unwrap().derived,
            PowerToughness {
                power: (3_i64 + bonus).into(),
                toughness: (5_i64 + bonus).into()
            }
        );
        if keys.contains(&Shroud) {
            let target = TargetRef::Object(physical(&attached, 2).id());
            illegal(
                &attached,
                A,
                GameCommand::Cast {
                    spell: physical(&attached, 5).id(),
                    target,
                },
            );
            let other = pass(&attached).into_state();
            illegal(
                &other,
                B,
                GameCommand::Cast {
                    spell: physical(&other, 6).id(),
                    target,
                },
            );
        }
        let moved = round(command(&attached, A, ability(&attached, 3)).into_state()).into_state();
        assert!(props(&moved, 2).keywords.is_empty());
        assert_eq!(props(&moved, 3).keywords, keys.iter().copied().collect());
        assert_eq!(
            props(&moved, 2).power_toughness.unwrap().derived.power,
            3.into()
        );
        assert_eq!(
            props(&moved, 3).power_toughness.unwrap().derived.power,
            (2_i64 + bonus).into()
        );
        let combat = if keys.contains(&Haste) {
            attached
        } else {
            next_main(attached, 3)
        };
        let attacked = attack(&combat, 2, B);
        assert_eq!(
            physical(&attacked, 2).tapped(),
            Some(!keys.contains(&Vigilance))
        );
        if keys.contains(&Trample) {
            trample_into_one_toughness(&attacked, 2, 4, 3);
        }
        if keys.contains(&DoubleStrike) {
            let blocks = blockers(&attacked);
            let open = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
            let first = round(open).into_state();
            assert_eq!(first.execution().life_total(B), Some(&17.into()));
            let second = round(first).into_state();
            assert_eq!(second.execution().life_total(B), Some(&14.into()));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_permanent_games_spitebellows_normal_evoke_and_leaves_damage() {
    use ManaType::*;
    for evoke in [false, true] {
        let price = if evoke {
            [(Colourless, 1), (Red, 2)]
        } else {
            [(Colourless, 5), (Red, 1)]
        };
        let start = game(
            vec![A, B],
            vec![
                hand("Spitebellows"),
                creature(2, B, 1, 7, &[]),
                remove_spell(3, A),
                remove_spell(4, A),
            ],
            &price,
            Step::PrecombatMain,
        );
        let cast = command(
            &start,
            A,
            GameCommand::CastWithResources {
                spell: physical(&start, 1).id(),
                target: None,
                payment: CastingPayment {
                    use_alternative: evoke,
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let entered = round(cast).into_state();
        assert_eq!(physical(&entered, 1).zone(), ObjectZone::Battlefield);
        let dead = if evoke {
            assert_eq!(entered.execution().window().entries().len(), 1);
            round(entered).into_state()
        } else {
            assert!(entered.execution().window().entries().is_empty());
            round(
                targeted(
                    &entered,
                    A,
                    3,
                    TargetRef::Object(physical(&entered, 1).id()),
                )
                .into_state(),
            )
            .into_state()
        };
        assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
        let aimed = reply(
            &dead,
            DecisionReply::TriggerTarget(TargetRef::Object(physical(&dead, 2).id())),
        )
        .into_state();
        let result = round(aimed.clone()).into_state();
        assert_eq!(
            result
                .execution()
                .battlefield_marked_damage(physical(&result, 2).id()),
            Some(6.into())
        );
        let removed =
            round(targeted(&aimed, A, 4, TargetRef::Object(physical(&aimed, 2).id())).into_state())
                .into_state();
        let missed = round(removed);
        assert!(missed.damage_journal().applied.is_empty());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_permanent_games_gargadon_suspends_for_four_upkeeps_then_has_haste_and_trample() {
    use ManaType::*;
    let mut cards = vec![hand("Gargadon"), creature(2, B, 1, 1, &[])];
    // Four upkeeps for the owner require four intervening draw steps for both players.
    for p in [A, B] {
        for n in 0..5 {
            cards.push(seed(
                10 + u64::from(p.0) * 10 + n,
                p,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let suspended = command(
        &initial,
        A,
        GameCommand::Suspend {
            source: physical(&initial, 1).id(),
            payment: Some(spend(&[(Colourless, 1), (Red, 1)])),
        },
    )
    .into_state();
    let exiled = physical(&suspended, 1).id();
    assert_eq!(physical(&suspended, 1).zone(), ObjectZone::Exile);
    assert_eq!(
        suspended.execution().time_counters().get(&exiled),
        Some(&4.into())
    );
    let mut s = suspended;
    for (number, count) in [(3, 3_i64), (5, 2), (7, 1)] {
        s = next_main(s, number);
        assert_eq!(
            s.execution().time_counters().get(&exiled),
            Some(&count.into())
        );
    }
    let offer = exile_hand::until(
        s,
        |s| matches!(s.pending_decision().map(|d|d.frame()),Some(RuleFrame::OptionalEffect(frame)) if frame.suspended_cast().is_some()),
    );
    assert_eq!(offer.turn().unwrap().position().number, 9);
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(spellwake_engine::effects::OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&declined, 1).zone(), ObjectZone::Exile);
    let casting = reply(
        &offer,
        DecisionReply::SuspendCast {
            target: None,
            payment: CastingPayment::default(),
        },
    )
    .into_state();
    let entered = round(casting).into_state();
    assert_eq!(physical(&entered, 1).zone(), ObjectZone::Battlefield);
    let attacked = attack(&entered, 1, B);
    trample_into_one_toughness(&attacked, 1, 2, 7);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_discount_games_medallions_reduce_generic_cost_and_track_active_source() {
    use ManaType::*;
    use spellwake_engine::characteristics::{AbilityChange, AbilityRemoval};
    for (name, color) in [
        ("Pearl Medallion", White),
        ("Sapphire Medallion", Blue),
        ("Jet Medallion", Black),
        ("Ruby Medallion", Red),
        ("Emerald Medallion", Green),
    ] {
        let other = if color == Red { Blue } else { Red };
        let test_spell = |color| {
            Program::instant(
                ManaCost {
                    generic: 2.into(),
                    required: [(color, 1.into())].into(),
                },
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![crate::keyword_creatures::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                    )],
                ),
            )
        };
        let strip = free_instant(
            BodyTarget::SINGLE_PERMANENT,
            crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                    AbilityRemoval::All,
                ))]),
                ResolvedDuration::UntilEndOfTurn,
            ),
        );
        let take = free_instant(
            BodyTarget::SINGLE_PERMANENT,
            crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )]),
                ResolvedDuration::NoStatedDuration,
            ),
        );
        let cards = vec![
            hand(name),
            seed(2, A, ObjectDestination::Hand, test_spell(color)),
            seed(3, A, ObjectDestination::Hand, test_spell(other)),
            named(name, 4, A, ObjectDestination::Hand),
            seed(5, A, ObjectDestination::Hand, strip),
            seed(6, B, ObjectDestination::Hand, take),
        ];
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, 6), (color, 1), (other, 1)],
            Step::PrecombatMain,
        );
        let one = cast_named(&initial, &[(Colourless, 2)]);
        let cast = |state: &GameState, n, kind, generic| GameCommand::CastUntargeted {
            spell: physical(state, n).id(),
            payment: Some(spend(&if generic > 0 {
                vec![(kind, 1), (Colourless, generic)]
            } else {
                vec![(kind, 1)]
            })),
        };
        illegal(&initial, A, cast(&initial, 2, color, 1));
        illegal(&one, A, cast(&one, 3, other, 1));
        let discounted = command(&one, A, cast(&one, 2, color, 1)).into_state();
        assert_eq!(discounted.execution().mana(A, color), 0.into());
        assert_eq!(discounted.execution().mana(A, Colourless), 3.into());
        assert_eq!(
            physical(&round(discounted).into_state(), 2).zone(),
            ObjectZone::Graveyard(A)
        );
        let two = round(cast_untargeted(&one, 4, &[(Colourless, 2)]).into_state()).into_state();
        illegal(
            &two,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&two, 2).id(),
                payment: Some(spend(&[(Colourless, 1)])),
            },
        );
        command(&two, A, cast(&two, 2, color, 0));
        let stripped =
            round(targeted(&one, A, 5, TargetRef::Object(physical(&one, 1).id())).into_state())
                .into_state();
        illegal(&stripped, A, cast(&stripped, 2, color, 1));
        command(&stripped, A, cast(&stripped, 2, color, 2));
        let other_priority = pass(&one).into_state();
        let taken = round(
            targeted(
                &other_priority,
                B,
                6,
                TargetRef::Object(physical(&one, 1).id()),
            )
            .into_state(),
        )
        .into_state();
        illegal(&taken, A, cast(&taken, 2, color, 1));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_discount_games_foundry_inspector_and_jukai_naturalist_filter_spell_types() {
    use ManaType::*;
    for (name, source_cost, reduce_artifacts) in [
        ("Foundry Inspector", vec![(Colourless, 3)], true),
        ("Jukai Naturalist", vec![(Green, 1), (White, 1)], false),
    ] {
        let budget = if reduce_artifacts {
            vec![(Colourless, 6)]
        } else {
            vec![(Colourless, 3), (Green, 1), (White, 1)]
        };
        let cards = vec![
            hand(name),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::generic(2.into())), vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::enchantment(Some(ManaCost::generic(2.into()))),
            ),
        ];
        let initial = game(vec![A, B], cards, &budget, Step::PrecombatMain);
        let state = cast_named(&initial, &source_cost);
        for id in [2, 3] {
            let discounted = (id == 2) == reduce_artifacts;
            let cast = GameCommand::CastUntargeted {
                spell: physical(&state, id).id(),
                payment: Some(spend(&[(Colourless, 1)])),
            };
            if discounted {
                let done = round(command(&state, A, cast).into_state()).into_state();
                assert_eq!(physical(&done, id).zone(), ObjectZone::Battlefield);
            } else {
                illegal(&state, A, cast);
            }
        }
        let s = next_main(state, 3);
        let attacked = attack(&s, 1, B);
        let end = exile_hand::until(attacked, |s| {
            s.turn().unwrap().position().step == Step::EndCombat
        });
        let power = if reduce_artifacts { 3_i64 } else { 2 };
        assert_eq!(
            end.execution().life_total(A),
            Some(&(if reduce_artifacts { 20_i64 } else { 22 }).into())
        );
        assert_eq!(end.execution().life_total(B), Some(&(20 - power).into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_draw_rock_games_pay_tap_sacrifice_and_verify_printed_mana_modes() {
    use ManaType::*;
    for (name, cast, activation, draw, produced, choose) in [
        ("Dreamstone Hedron", 6, 3, 3, 3, false),
        ("Hedron Archive", 4, 2, 2, 2, false),
        ("Letter of Acceptance", 3, 2, 1, 1, true),
        ("Mind Stone", 2, 1, 1, 1, false),
    ] {
        let draw_rock = hand(name);
        let [draw_ability] = draw_rock.program.activated.as_slice() else {
            panic!("{name}: expected one nonmana ability");
        };
        assert_eq!(
            draw_ability.source_cost,
            spellwake_engine::activated::SourceCost::TapSacrifice,
            "{name}",
        );
        let draw_id = draw_ability.id;
        let mut cards = vec![draw_rock];
        cards.extend((2..5).map(|id| {
            seed(
                id,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        }));
        let price = [(Colourless, cast)];
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, cast + activation)],
            Step::PrecombatMain,
        );
        let state = cast_named(&initial, &price);
        let source = physical(&state, 1).id();
        let tap = GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        };
        let draw_command = |amount| GameCommand::ActivateUntargetedAbility {
            source,
            ability: draw_id,
            payment: Some(spend(&[(Colourless, amount)])),
        };
        illegal(&state, A, draw_command(activation - 1));
        let tapped = command(&state, A, tap.clone()).into_state();
        let colors = if choose {
            vec![White, Blue, Black, Red, Green]
        } else {
            vec![Colourless]
        };
        for k in colors {
            let floated = if choose {
                let d = tapped.pending_decision().unwrap();
                let RuleFrame::ManaColor(f) = d.frame() else {
                    panic!("printed any-color")
                };
                assert_eq!(f.colors(), &[White, Blue, Black, Red, Green].into());
                reply(&tapped, DecisionReply::ManaColor(k)).into_state()
            } else {
                tapped.clone()
            };
            assert!(physical(&floated, 1).tapped().unwrap());
            assert_eq!(
                floated.execution().mana(A, k),
                state
                    .execution()
                    .mana(A, k)
                    .plus(&i64::from(produced).into())
            );
            illegal(&floated, A, draw_command(activation));
        }
        let top = state.execution().window().cards().library(A)[..draw]
            .iter()
            .map(|id| {
                state
                    .execution()
                    .window()
                    .cards()
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let paid = command(&state, A, draw_command(activation)).into_state();
        assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(paid.execution().mana(A, Colourless), 0.into());
        assert!(
            top.iter()
                .all(|id| physical(&paid, id.0).zone() == ObjectZone::Library(A))
        );
        let resolved = round(paid).into_state();
        assert!(
            top.iter()
                .all(|id| physical(&resolved, id.0).zone() == ObjectZone::Hand(A))
        );
        assert_eq!(
            resolved.execution().window().cards().library(A).len(),
            4 - draw
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_worn_powerstone_enters_tapped_then_adds_two_without_using_the_stack() {
    let price = [(ManaType::Colourless, 3)];
    let state = cast_named(
        &game(
            vec![A, B],
            vec![hand("Worn Powerstone")],
            &price,
            Step::PrecombatMain,
        ),
        &price,
    );
    let tap = GameCommand::ActivateMana {
        source: physical(&state, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
    };
    assert_eq!(physical(&state, 1).tapped(), Some(true));
    illegal(&state, A, tap.clone());
    let main = next_main(state, 3);
    let done = command(&main, A, tap).into_state();
    assert_eq!(done.execution().mana(A, ManaType::Colourless), 2.into());
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(physical(&done, 1).tapped(), Some(true));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_darksteel_ingot_survives_destroy_but_can_be_sacrificed_and_adds_each_color() {
    use ManaType::*;
    let price = [(Colourless, 3)];
    let cards = vec![
        hand("Darksteel Ingot"),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_PERMANENT,
                crate::keyword_creatures::toolbox_emit::zone_op(
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
            3,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                    AffectedPlayers::One(spellwake_engine::program::ResolvingPlayer::Controller),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                    PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: [CardType::Artifact].into(),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                ))),
            ),
        ),
    ];
    let state = cast_named(
        &game(vec![A, B], cards, &price, Step::PrecombatMain),
        &price,
    );
    let source = physical(&state, 1).id();
    let tap = GameCommand::ActivateMana {
        source,
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
    };
    let choosing = command(&state, A, tap.clone()).into_state();
    let RuleFrame::ManaColor(frame) = choosing.pending_decision().unwrap().frame() else {
        panic!("color choice")
    };
    let colors = [White, Blue, Black, Red, Green];
    assert_eq!(frame.colors(), &colors.into());
    for color in colors {
        let done = reply(&choosing, DecisionReply::ManaColor(color)).into_state();
        assert_eq!(done.execution().mana(A, color), 1.into());
        assert_eq!(physical(&done, 1).tapped(), Some(true));
        illegal(&done, A, tap.clone());
    }
    let stack = targeted(&state, A, 2, TargetRef::Object(source)).into_state();
    let survived = round(stack).into_state();
    assert_eq!(physical(&survived, 1).id(), source);
    assert_eq!(physical(&survived, 1).zone(), ObjectZone::Battlefield);
    let stack = cast_untargeted(&survived, 3, &[]).into_state();
    let selecting = round(stack).into_state();
    let done = reply(&selecting, DecisionReply::SelectResolution(vec![source])).into_state();
    assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jade_mage_pays_for_each_saproling_without_tapping() {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1)];
    let game = game(
        vec![A, B],
        vec![hand("Jade Mage")],
        &[(Colourless, 5), (Green, 3)],
        Step::PrecombatMain,
    );
    let mut state = cast_named(&game, &price);
    let source = physical(&state, 1).id();
    let activate = |cost| GameCommand::ActivateUntargetedAbility {
        source,
        ability: ActivatedAbilityId(0),
        payment: Some(spend(cost)),
    };
    illegal(&state, A, activate(&[(Colourless, 3)]));
    for remaining in [1, 0] {
        let stack = command(&state, A, activate(&[(Colourless, 2), (Green, 1)])).into_state();
        assert_eq!(stack.execution().mana(A, Green), remaining.into());
        assert_eq!(physical(&stack, 1).tapped(), Some(false));
        let done = round(stack);
        let created: Vec<_> = events(&done)
            .flat_map(|event| &event.occurrences)
            .filter_map(|row| match &row.value {
                RulesOccurrence::TokenCreated { creator, after } if *creator == A => {
                    Some(after.object.id())
                }
                _ => None,
            })
            .collect();
        let [token] = created.as_slice() else {
            panic!("one new Saproling")
        };
        let token = *token;
        let sheet = done
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        assert_eq!(sheet.controller, Some(A));
        assert_eq!(
            sheet
                .current_names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>(),
            ["Saproling Token"]
        );
        assert_eq!(
            sheet.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Saproling").unwrap()].into()
        );
        assert_eq!(sheet.current_types, [CardType::Creature].into());
        assert_eq!(sheet.current_colors, [Color::Green].into());
        assert_eq!(
            sheet.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
        state = done.into_state();
    }
    illegal(&state, A, activate(&[(Colourless, 2), (Green, 1)]));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_azusa_rechecks_additional_land_plays_after_source_departure_and_next_turn() {
    use ManaType::*;
    let price = [(Colourless, 2), (Green, 1)];
    let mut cards = vec![
        hand("Azusa, Lost but Seeking"),
        seed(
            10,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::zone_op(
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
    ];
    cards.extend((2..=6).map(|n| {
        seed(
            n,
            A,
            ObjectDestination::Hand,
            Program::land(BTreeSet::new(), vec![]),
        )
    }));
    let game = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let play = |s: &GameState, n| GameCommand::PlayLand {
        land: physical(s, n).id(),
    };
    let first = command(&game, A, play(&game, 2)).into_state();
    illegal(&first, A, play(&first, 3));
    let entered = cast_named(&first, &price);
    let second = command(&entered, A, play(&entered, 3)).into_state();
    let removal =
        targeted(&second, A, 10, TargetRef::Object(physical(&second, 1).id())).into_state();
    let removed = round(removal).into_state();
    assert_eq!(removed.turn().unwrap().lands_played(), &2.into());
    illegal(&removed, A, play(&removed, 4));
    let third = command(&second, A, play(&second, 4)).into_state();
    assert_eq!(third.turn().unwrap().lands_played(), &3.into());
    illegal(&third, A, play(&third, 5));
    let next = next_main(removed, 3);
    assert_eq!(next.turn().unwrap().lands_played(), &0.into());
    let played = command(&next, A, play(&next, 5)).into_state();
    illegal(&played, A, play(&played, 6));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_danitha_discounts_auras_and_equipment_and_deals_first_strike_lifelink_damage() {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 1)];
    let cost = Some(ManaCost::generic(2.into()));
    let cards = vec![
        hand("Danitha Capashen, Paragon"),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            Program::aura(
                cost.clone(),
                spellwake_engine::attachment::EnchantRestriction::Creature,
            ),
        ),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            Program::equipment(cost.clone(), ManaCost::generic(1.into())),
        ),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            Program::fortification(cost, ManaCost::generic(1.into())),
        ),
        remove_spell(5, A),
        creature(6, B, 2, 2, &[]),
    ];
    let before = game(
        vec![A, B],
        cards,
        &[(Colourless, 6), (White, 1)],
        Step::PrecombatMain,
    );
    let state = cast_named(&before, &price);
    let source = physical(&state, 1).id();
    let ability = |s: &GameState, n, amount| GameCommand::CastUntargeted {
        spell: physical(s, n).id(),
        payment: Some(spend(&[(Colourless, amount)])),
    };
    illegal(&state, A, ability(&state, 4, 1));
    let aura = command(
        &state,
        A,
        GameCommand::CastPaid {
            spell: physical(&state, 2).id(),
            target: TargetRef::Object(source),
            payment: spend(&[(Colourless, 1)]),
        },
    )
    .into_state();
    let aura = round(aura).into_state();
    assert_eq!(
        aura.execution()
            .current_characteristics(physical(&aura, 2).id())
            .unwrap()
            .unwrap()
            .attached_to,
        Some(TargetRef::Object(source))
    );
    let equipment = command(&state, A, ability(&state, 3, 1)).into_state();
    assert_eq!(
        physical(&round(equipment).into_state(), 3).zone(),
        ObjectZone::Battlefield
    );
    let removed =
        round(targeted(&state, A, 5, TargetRef::Object(source)).into_state()).into_state();
    illegal(&removed, A, ability(&removed, 3, 1));
    command(&removed, A, ability(&removed, 3, 2));
    let attacking = attack(&next_main(state, 3), 1, B);
    assert_eq!(physical(&attacking, 1).tapped(), Some(false));
    let blockers = blockers(&attacking);
    let blocked = reply(&blockers, block_choice(&blockers, 6, 1)).into_state();
    let end = exile_hand::until(blocked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(physical(&end, 6).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&end, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(end.execution().life_total(A), Some(&22.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_dragon_fodder_makes_two_goblins_together_and_rakka_makes_an_attacking_elemental() {
    use ManaType::*;
    let price = [(Colourless, 1), (Red, 1)];
    let game_fodder = game(
        vec![A, B],
        vec![hand("Dragon Fodder")],
        &price,
        Step::PrecombatMain,
    );
    let ordering = round(cast_untargeted(&game_fodder, 1, &price).into_state()).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("Goblin timestamp order")
    };
    let goblins = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    assert_eq!(
        physical(goblins.state(), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    for token in tokens(&goblins, A, 2) {
        let sheet = goblins
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        assert_eq!(
            sheet.current_names.iter().next().unwrap().as_str(),
            "Goblin Token"
        );
        assert_eq!(
            sheet.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Goblin").unwrap()].into()
        );
        assert_eq!(sheet.current_colors, [Color::Red].into());
        assert_eq!(
            sheet.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
    let initial = game(
        vec![A, B],
        vec![hand("Rakka Mar")],
        &[(Colourless, 2), (Red, 3)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &[(Colourless, 2), (Red, 2)]);
    let activation = GameCommand::ActivateUntargetedAbility {
        source: physical(&entered, 1).id(),
        ability: ActivatedAbilityId(0),
        payment: Some(spend(&[(Red, 1)])),
    };
    let paid = command(&entered, A, activation.clone()).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    let made = round(paid);
    let token = tokens(&made, A, 1)[0];
    let sheet = made
        .state()
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(
        sheet.current_names.iter().next().unwrap().as_str(),
        "Elemental Token"
    );
    assert_eq!(sheet.current_colors, [Color::Red].into());
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 1.into()
        }
    );
    assert!(sheet.keywords.contains(&Haste));
    illegal(made.state(), A, activation);
    let frame = combat_frame(made.state(), true);
    let attacking = reply(
        &frame,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: token,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let done = exile_hand::until(attacking, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(done.execution().life_total(B), Some(&17.into()));
    assert_eq!(physical(&done, 1).tapped(), Some(true));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_vial_and_obelisk_pay_and_sacrifice_before_the_targeted_effect() {
    use ManaType::Colourless;
    for (name, cast, cost) in [("Vial of Dragonfire", 2, 2), ("Unstable Obelisk", 3, 7)] {
        let price = [(Colourless, cast)];
        let initial = game(
            vec![A, B],
            vec![hand(name), creature(2, B, 1, 2, &[]), remove_spell(3, A)],
            &[(Colourless, cast + cost)],
            Step::PrecombatMain,
        );
        let state = cast_named(&initial, &price);
        let source = physical(&state, 1).id();
        let target = physical(&state, 2).id();
        let activate = |amount| GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(spend(&[(Colourless, amount)])),
        };
        illegal(&state, A, activate(cost - 1));
        if name == "Unstable Obelisk" {
            let tapped = command(
                &state,
                A,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                },
            )
            .into_state();
            assert_eq!(tapped.execution().mana(A, Colourless), (cost + 1).into());
            illegal(&tapped, A, activate(cost));
        }
        let stacked = command(&state, A, activate(cost)).into_state();
        assert_eq!(physical(&stacked, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&stacked, 2).zone(), ObjectZone::Battlefield);
        assert_eq!(stacked.execution().mana(A, Colourless), 0.into());
        let done = round(stacked.clone());
        assert_eq!(physical(done.state(), 2).zone(), ObjectZone::Graveyard(B));
        if name == "Vial of Dragonfire" {
            assert!(
                done.damage_journal().applied.iter().any(|d| d.source.id()
                    == DamageSourceId::Object(source)
                    && d.amount == 2.into())
            );
        }
        let removed =
            round(targeted(&stacked, A, 3, TargetRef::Object(target)).into_state()).into_state();
        let fizzled = round(removed);
        assert!(fizzled.damage_journal().applied.is_empty());
        assert!(fizzled.state().execution().window().entries().is_empty());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_exclude_counters_only_a_creature_spell_and_draws_only_on_resolution() {
    use ManaType::*;
    let price = [(Colourless, 2), (Blue, 1)];
    let before = game(
        vec![A, B],
        vec![
            hand("Exclude"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new()),
            ),
            seed(
                3,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
                    Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Graveyard,
                        ),
                    )),
                ),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let play = |s: &GameState, target| GameCommand::CastPaid {
        spell: physical(s, 1).id(),
        target: TargetRef::Stack(target),
        payment: spend(&price),
    };
    let artifact = cast_untargeted(&before, 4, &[]).into_state();
    illegal(
        &artifact,
        A,
        play(&artifact, artifact.execution().window().entries()[0].id()),
    );
    let creature = cast_untargeted(&before, 2, &[]).into_state();
    let entry = creature.execution().window().entries()[0].id();
    let stack = command(&creature, A, play(&creature, entry)).into_state();
    let top = physical(&stack, 100).card_id().unwrap();
    assert_eq!(physical(&stack, top.0).zone(), ObjectZone::Library(A));
    let resolved = round(stack.clone()).into_state();
    assert_eq!(physical(&resolved, top.0).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Graveyard(A));
    let priority = pass(&stack).into_state();
    let reply = targeted(&priority, B, 3, TargetRef::Stack(entry)).into_state();
    let removed = round(reply).into_state();
    let fizzled = round(removed).into_state();
    assert_eq!(physical(&fizzled, top.0).zone(), ObjectZone::Library(A));
    assert_eq!(physical(&fizzled, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tower_and_vessel_prevent_only_their_controllers_cleanup_discard() {
    use ManaType::Colourless;
    for (name, hand_size, is_land) in [("Thought Vessel", 9, false), ("Reliquary Tower", 9, true)] {
        let mut cards = vec![
            hand(name),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PERMANENT,
                    crate::keyword_creatures::toolbox_emit::zone_op(
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
        ];
        cards.extend((0..hand_size - 1).map(|i| {
            seed(
                10 + i,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        }));
        let initial = game(vec![A, B], cards, &[(Colourless, 2)], Step::PrecombatMain);
        let state = if is_land {
            command(
                &initial,
                A,
                GameCommand::PlayLand {
                    land: physical(&initial, 1).id(),
                },
            )
            .into_state()
        } else {
            cast_named(&initial, &[(Colourless, 2)])
        };
        let source = physical(&state, 1).id();
        let tapped = command(
            &state,
            A,
            GameCommand::ActivateMana {
                source,
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        assert_eq!(
            tapped.execution().mana(A, Colourless),
            state.execution().mana(A, Colourless).plus(&1.into())
        );
        let count = |s: &GameState| {
            s.execution()
                .window()
                .cards()
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(A))
                .count()
        };
        assert_eq!(count(&tapped), hand_size as usize);
        let after = next_main(tapped.clone(), 2);
        assert_eq!(count(&after), hand_size as usize);
        let destroyed =
            round(targeted(&tapped, A, 2, TargetRef::Object(source)).into_state()).into_state();
        assert_eq!(count(&next_main(destroyed, 2)), 7);
    }
}

mod chosen_type_games;
mod cycling_games;
mod draw_engines;
mod draw_permanents;
mod graveyard_draw;
mod graveyard_values;
mod library_casts;
mod library_combat;
mod reanimation_games;
mod zone_choices;

mod set_mechanics;

mod set_casting;

mod set_spells;

mod set_slivers;

mod set_modal;

mod set_triggers;

mod set_attackers;

mod set_group_spells;

mod set_tokens;

mod attack_library;
mod library_sagas;
mod triggered_library;

mod creature_draws;
mod set_devotion;

mod set_encore;

mod set_treasures;

mod set_enchantments;
