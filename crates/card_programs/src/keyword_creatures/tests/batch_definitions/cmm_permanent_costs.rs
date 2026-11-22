//! Source-bound Commander Masters cases for replacement costs, delayed draws,
//! variable kicker, global subtype bonuses and inherited activated abilities.
use super::cmm_gameplay::{creature, illegal, named, remove_spell};
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::ObjectCharacteristics,
    effects::{ZoneReplacementReply, ZoneReplacementRequest},
    mana::ManaType::*,
};

fn sheet(state: &GameState, id: ObjectId) -> ObjectCharacteristics {
    state
        .execution()
        .current_characteristics(id)
        .unwrap()
        .unwrap()
}

fn assert_size(state: &GameState, id: ObjectId, power: i64, toughness: i64) {
    assert_eq!(
        sheet(state, id).power_toughness.unwrap().derived,
        PowerToughness {
            power: power.into(),
            toughness: toughness.into()
        }
    );
}

fn hand_size(state: &GameState, player: SeatId) -> usize {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(player))
        .count()
}

fn draw_pile() -> Vec<SeedCard> {
    (13..19)
        .map(|n| {
            seed(
                n,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        })
        .collect()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bloodspore_devours_at_entry_then_adds_its_counters_to_other_creatures() {
    let price = [(Green, 2), (Colourless, 2)];
    let mut entrant = seed(
        4,
        A,
        ObjectDestination::Hand,
        Program::creature(
            Some(ManaCost::free()),
            1.into(),
            1.into(),
            Default::default(),
        ),
    );
    entrant.destination = ObjectDestination::Hand;
    let state = game(
        vec![A, B],
        vec![
            named("Bloodspore Thrinax", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 1, &[]),
            creature(3, A, 1, 1, &[]),
            entrant,
            creature(5, B, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let pending = round(cast_untargeted(&state, 1, &price).into_state()).into_state();
    let RuleFrame::ZoneReplacement(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("devour choice")
    };
    let ZoneReplacementRequest::Devour { choices, .. } = frame.request() else {
        panic!("choose controlled creatures")
    };
    assert_eq!(
        choices.iter().copied().collect::<BTreeSet<_>>(),
        [physical(&pending, 2).id(), physical(&pending, 3).id()].into()
    );
    let decide =
        |s: &GameState, choice| reply(s, DecisionReply::ZoneReplacement(choice)).into_state();
    let declined = decide(&pending, ZoneReplacementReply::Devour(None));
    assert_size(&declined, physical(&declined, 1).id(), 2, 2);
    let first = decide(
        &pending,
        ZoneReplacementReply::Devour(Some(physical(&pending, 2).id())),
    );
    assert_eq!(physical(&first, 2).zone(), ObjectZone::Battlefield);
    let ordered = decide(
        &first,
        ZoneReplacementReply::Devour(Some(physical(&first, 3).id())),
    );
    let RuleFrame::ZoneReplacement(frame) = ordered.pending_decision().unwrap().frame() else {
        panic!("owner's graveyard order")
    };
    assert!(
        matches!(frame.request(),ZoneReplacementRequest::DevourGraveyardOrder{choices} if choices.len()==2)
    );
    let entered = decide(
        &ordered,
        ZoneReplacementReply::DevourGraveyardOrder(physical(&ordered, 3).id()),
    );
    for n in [2, 3] {
        assert_eq!(physical(&entered, n).zone(), ObjectZone::Graveyard(A));
    }
    assert_eq!(
        sheet(&entered, physical(&entered, 1).id()).plus_one_counters,
        2.into()
    );
    assert_size(&entered, physical(&entered, 1).id(), 4, 4);
    let second = round(cast_untargeted(&entered, 4, &[]).into_state()).into_state();
    assert_size(&second, physical(&second, 4).id(), 3, 3);
    assert_eq!(
        sheet(&second, physical(&second, 4).id()).plus_one_counters,
        2.into()
    );
    assert_eq!(
        sheet(&second, physical(&second, 5).id()).plus_one_counters,
        0.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_verdeloth_pays_variable_kicker_and_buffs_each_matching_creature_once() {
    let (_, vocab, _) = current_catalogue();
    let sap = vocab.resolve_label("Saproling").unwrap();
    let tree = vocab.resolve_label("Treefolk").unwrap();
    let dual = seed(
        2,
        B,
        ObjectDestination::Battlefield { controller: B },
        Program::creature(None, 1.into(), 1.into(), [tree, sap].into()),
    );
    let price = [(Green, 2), (Colourless, 6)];
    let state = game(
        vec![A, B],
        vec![
            named("Verdeloth the Ancient", 1, A, ObjectDestination::Hand),
            dual,
        ],
        &price,
        Step::PrecombatMain,
    );
    let normal =
        round(cast_untargeted(&state, 1, &[(Green, 2), (Colourless, 4)]).into_state()).into_state();
    assert!(normal.execution().window().entries().is_empty());
    assert_size(&normal, physical(&normal, 1).id(), 4, 7);
    assert_size(&normal, physical(&normal, 2).id(), 2, 2);
    let cast = command(
        &state,
        A,
        GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: None,
            payment: CastingPayment {
                chosen_x: Some(2.into()),
                additional: [(0, 1.into())].into(),
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(cast.execution().mana(A, Colourless), 0.into());
    assert_eq!(
        sheet(&cast, physical(&cast, 1).id()).defined.mana_value,
        6.into()
    );
    let entered = round(cast).into_state();
    assert_eq!(entered.execution().window().entries().len(), 1);
    let creating = round(entered).into_state();
    let RuleFrame::TimestampOrder(order) = creating.pending_decision().unwrap().frame() else {
        panic!("two simultaneous Saprolings")
    };
    let made = reply(
        &creating,
        DecisionReply::TimestampOrder(order.members().iter().map(|member| member.member).collect()),
    );
    let objects = tokens(&made, A, 2);
    for id in objects {
        let stats = sheet(made.state(), id);
        assert!(stats.current_creature_subtypes.contains(&sap));
        assert_size(made.state(), id, 2, 2);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_subira_discards_its_hand_and_draws_for_each_small_creature_that_hits() {
    let mut start = draw_pile();
    start.extend([
        named("Subira, Tulzidi Caravanner", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 2, &[KeywordAbility::Haste]),
        creature(3, A, 3, 2, &[KeywordAbility::Haste]),
        creature(4, B, 1, 1, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
    ]);
    let state = game(
        vec![A, B],
        start,
        &[(Colourless, 4), (Red, 2)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&state, 1, &[(Colourless, 2), (Red, 1)]).into_state()).into_state();
    let source = physical(&entered, 1).id();
    let bypass = |id| GameCommand::ActivateAbility {
        source,
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(id),
        payment: Some(spend(&[(Colourless, 1)])),
    };
    illegal(&entered, A, bypass(source));
    illegal(&entered, A, bypass(physical(&entered, 3).id()));
    let small = physical(&entered, 2).id();
    let opened = round(command(&entered, A, bypass(small)).into_state()).into_state();
    let activated = command(
        &opened,
        A,
        GameCommand::ActivateUntargetedAbility {
            source,
            ability: ActivatedAbilityId(1),
            payment: Some(spend(&[(Colourless, 1), (Red, 1)])),
        },
    )
    .into_state();
    assert_eq!(physical(&activated, 1).tapped(), Some(true));
    assert_eq!(physical(&activated, 5).zone(), ObjectZone::Graveyard(A));
    let ready = round(activated).into_state();
    let attack = combat_frame(&ready, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(
            [2, 3]
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
    let blocks = combat_frame(&attacked, false);
    let block = DecisionReply::DeclareBlockers(vec![BlockDeclaration {
        blocker: physical(&blocks, 4).id(),
        attacker: small,
    }]);
    illegal(
        &blocks,
        B,
        GameCommand::Reply {
            decision: blocks.pending_decision().unwrap().id(),
            choice: block,
        },
    );
    let unblocked = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
    let end = exile_hand::until(unblocked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(end.execution().life_total(B), Some(&15.into()));
    assert_eq!(hand_size(&end, A), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hunters_insight_watches_only_its_target_and_draws_the_damage_dealt() {
    let mut start = draw_pile();
    start.extend([
        named("Hunter's Insight", 1, A, ObjectDestination::Hand),
        creature(2, A, 3, 2, &[KeywordAbility::Haste]),
        creature(3, A, 2, 2, &[KeywordAbility::Haste]),
        creature(4, B, 2, 2, &[]),
    ]);
    let price = [(Green, 1), (Colourless, 2)];
    let state = game(vec![A, B], start, &price, Step::PrecombatMain);
    let cast = |n| GameCommand::CastPaid {
        spell: physical(&state, 1).id(),
        target: TargetRef::Object(physical(&state, n).id()),
        payment: spend(&price),
    };
    illegal(&state, A, cast(4));
    let ready = round(command(&state, A, cast(2)).into_state()).into_state();
    assert_eq!(physical(&ready, 1).zone(), ObjectZone::Graveyard(A));
    let attack = combat_frame(&ready, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(
            [2, 3]
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
    let end = exile_hand::until(attacked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(end.execution().life_total(B), Some(&15.into()));
    assert_eq!(hand_size(&end, A), 3);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_experiment_kraj_gains_and_loses_other_creatures_activated_abilities() {
    let mut donor = creature(2, B, 2, 2, &[]);
    donor.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(3),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            }],
        ),
    });
    donor
        .program
        .mana
        .push(spellwake_engine::mana::ManaAbility {
            id: spellwake_engine::mana::ManaAbilityId(0),
            output: spellwake_engine::mana::ManaOutput::Fixed([(Blue, 1.into())].into()),
        });
    let mut start = draw_pile();
    start.push(seed(
        20,
        B,
        ObjectDestination::LibraryBottom,
        Program::artifact(None, vec![]),
    ));
    start.extend([
        named("Experiment Kraj", 1, A, ObjectDestination::Hand),
        donor,
        remove_spell(3, A),
    ]);
    let price = [(Colourless, 2), (Green, 2), (Blue, 2)];
    let initial = game(vec![A, B], start, &price, Step::PrecombatMain);
    let entered = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let source = physical(&entered, 1).id();
    let target = physical(&entered, 2).id();
    let add_counter = GameCommand::ActivateAbility {
        source,
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(target),
        payment: None,
    };
    illegal(&entered, A, add_counter.clone());
    let ready = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert!(sheet(&ready, source).granted_activated.is_empty());
    let counters = round(command(&ready, A, add_counter).into_state()).into_state();
    assert_eq!(sheet(&counters, target).plus_one_counters, 1.into());
    let grants = sheet(&counters, source).granted_activated;
    assert_eq!(grants.len(), 1);
    let effect = *grants.keys().next().unwrap();
    let gain = GameCommand::ActivateGranted {
        source,
        effect,
        target: None,
        payment: None,
    };
    let gained = round(command(&counters, A, gain.clone()).into_state()).into_state();
    assert_eq!(gained.execution().life_total(A), Some(&21.into()));
    let mana = sheet(&gained, source)
        .mana_abilities()
        .into_iter()
        .find(|reference| {
            matches!(
                reference,
                spellwake_engine::mana::ManaAbilityRef::Inherited(_)
            )
        })
        .expect("inherited mana ability");
    let tap = GameCommand::ActivateMana {
        source,
        ability: mana,
    };
    illegal(&gained, A, tap.clone());
    let ready = exile_hand::until(gained.clone(), |s| {
        s.turn().unwrap().position().number == 5
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let produced = command(&ready, A, tap).into_state();
    assert_eq!(produced.execution().mana(A, Blue), 1.into());
    assert_eq!(physical(&produced, 1).tapped(), Some(true));
    assert_eq!(physical(&produced, 2).tapped(), Some(false));
    let destroy = GameCommand::Cast {
        spell: physical(&gained, 3).id(),
        target: TargetRef::Object(target),
    };
    let gone = round(command(&gained, A, destroy).into_state()).into_state();
    assert!(sheet(&gone, source).granted_activated.is_empty());
    illegal(&gone, A, gain);
}

fn selected_cost(
    state: &GameState,
    number: u64,
    ability: u64,
    objects: Vec<ObjectId>,
    target: Option<ObjectId>,
    mana: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::ActivateWithCostObjects {
        source: physical(state, number).id(),
        ability: ActivatedAbilityId(ability),
        granting_effect: None,
        target: target.map(TargetRef::Object),
        objects,
        payment: (!mana.is_empty()).then(|| spend(mana)),
    }
}

fn ordered_tokens(state: GameState, expected: usize) -> (GameState, Vec<ObjectId>) {
    let resolved = round(state);
    let result = if expected > 1 {
        let state = resolved.state();
        let RuleFrame::TimestampOrder(frame) = state.pending_decision().unwrap().frame() else {
            panic!("simultaneous tokens")
        };
        reply(
            state,
            DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
        )
    } else {
        resolved
    };
    let ids = tokens(&result, A, expected);
    (result.into_state(), ids)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ghoulcaller_gisa_uses_the_sacrificed_creatures_last_power() {
    let start = game(
        vec![A, B],
        vec![
            named(
                "Fervor",
                91,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Ghoulcaller Gisa",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 3, 1, &[]),
            creature(3, B, 2, 2, &[]),
        ],
        &[(Black, 1)],
        Step::PrecombatMain,
    );
    for illegal_victim in [1, 3] {
        illegal(
            &start,
            A,
            selected_cost(
                &start,
                1,
                0,
                vec![physical(&start, illegal_victim).id()],
                None,
                &[(Black, 1)],
            ),
        );
    }
    let paid = command(
        &start,
        A,
        selected_cost(
            &start,
            1,
            0,
            vec![physical(&start, 2).id()],
            None,
            &[(Black, 1)],
        ),
    )
    .into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    let (done, zombies) = ordered_tokens(paid, 3);
    let kind = current_catalogue().1.resolve_label("Zombie").unwrap();
    for zombie in zombies {
        let row = sheet(&done, zombie);
        assert!(row.current_creature_subtypes.contains(&kind));
        assert_eq!(row.current_colors, [Color::Black].into());
        assert_size(&done, zombie, 2, 2);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_cabal_patriarch_sacrifices_then_exiles_the_same_card_for_separate_effects() {
    let start = game(
        vec![A, B],
        vec![
            named(
                "Cabal Patriarch",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 1, 1, &[]),
            creature(3, B, 5, 5, &[]),
        ],
        &[(Colourless, 4), (Black, 2)],
        Step::PrecombatMain,
    );
    let target = physical(&start, 3).id();
    let activate = |s: &GameState, index| {
        selected_cost(
            s,
            1,
            index,
            vec![physical(s, 2).id()],
            Some(target),
            &[(Colourless, 2), (Black, 1)],
        )
    };
    illegal(&start, A, activate(&start, 1));
    let paid = command(&start, A, activate(&start, 0)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let first = round(paid).into_state();
    assert_size(&first, target, 3, 3);
    let paid = command(&first, A, activate(&first, 1)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Exile);
    let second = round(paid).into_state();
    assert_size(&second, target, 1, 1);
    let expired = exile_hand::until(second, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_size(&expired, target, 5, 5);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fungal_plots_exiles_a_graveyard_creature_then_trades_two_saprolings_for_life_and_a_card() {
    let sap = current_catalogue().1.resolve_label("Saproling").unwrap();
    let land = ObjectDestination::Battlefield { controller: A };
    let other = seed(
        3,
        A,
        land,
        Program::creature(None, 1.into(), 1.into(), [sap].into()),
    );
    let mut grave = creature(2, A, 1, 1, &[]);
    grave.destination = ObjectDestination::Graveyard;
    let mut start = draw_pile();
    start.extend([named("Fungal Plots", 1, A, land), grave, other]);
    let start = game(
        vec![A, B],
        start,
        &[(Colourless, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    let paid = command(
        &start,
        A,
        selected_cost(
            &start,
            1,
            0,
            vec![physical(&start, 2).id()],
            None,
            &[(Colourless, 1), (Green, 1)],
        ),
    )
    .into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Exile);
    let (made, ids) = ordered_tokens(paid, 1);
    assert!(
        sheet(&made, ids[0])
            .current_creature_subtypes
            .contains(&sap)
    );
    assert_size(&made, ids[0], 1, 1);
    let both = selected_cost(
        &made,
        1,
        1,
        vec![ids[0], physical(&made, 3).id()],
        None,
        &[],
    );
    let paid = command(&made, A, both).into_state();
    assert_eq!(physical(&paid, 3).zone(), ObjectZone::Graveyard(A));
    let done = round(paid).into_state();
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
    assert_eq!(hand_size(&done, A), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_whirler_rogue_can_tap_its_new_thopters_and_make_an_opponents_creature_unblockable() {
    let mana = [(Colourless, 2), (Blue, 2)];
    let start = game(
        vec![A, B],
        vec![
            named("Whirler Rogue", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[KeywordAbility::Haste]),
            creature(3, B, 2, 2, &[]),
            creature(4, A, 1, 1, &[KeywordAbility::Haste]),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &mana).into_state()).into_state();
    let (made, thopters) = ordered_tokens(entered, 2);
    for &id in &thopters {
        let row = sheet(&made, id);
        assert!(row.has_type(CardType::Artifact));
        assert!(row.has_keyword(KeywordAbility::Flying));
        assert!(row.current_colors.is_empty());
        assert_size(&made, id, 1, 1);
    }
    let them = selected_cost(
        &made,
        1,
        0,
        thopters.clone(),
        Some(physical(&made, 3).id()),
        &[],
    );
    let their_target = round(command(&made, A, them).into_state()).into_state();
    for &id in &thopters {
        assert_eq!(
            their_target
                .execution()
                .window()
                .cards()
                .get(id)
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    let us = selected_cost(&made, 1, 0, thopters, Some(physical(&made, 2).id()), &[]);
    let ready = round(command(&made, A, us).into_state()).into_state();
    let attack = combat_frame(&ready, true);
    let attacker = physical(&attack, 2).id();
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(
            [attacker, physical(&attack, 4).id()]
                .into_iter()
                .map(|attacker| AttackDeclaration {
                    attacker,
                    defender: B,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    )
    .into_state();
    let blocks = combat_frame(&attacked, false);
    illegal(
        &blocks,
        B,
        GameCommand::Reply {
            decision: blocks.pending_decision().unwrap().id(),
            choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                attacker,
                blocker: physical(&blocks, 3).id(),
            }]),
        },
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_razaketh_pays_life_and_another_creature_to_find_any_card() {
    let landed = ObjectDestination::Battlefield { controller: A };
    let start = game(
        vec![A, B],
        vec![
            named("Razaketh, the Foulblooded", 1, A, landed),
            creature(2, A, 2, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let traits = sheet(&start, physical(&start, 1).id());
    assert!(
        traits.has_keyword(KeywordAbility::Flying) && traits.has_keyword(KeywordAbility::Trample)
    );
    illegal(
        &start,
        A,
        selected_cost(&start, 1, 0, vec![physical(&start, 1).id()], None, &[]),
    );
    let paid = command(
        &start,
        A,
        selected_cost(&start, 1, 0, vec![physical(&start, 2).id()], None, &[]),
    )
    .into_state();
    assert_eq!(paid.execution().life_total(A), Some(&18.into()));
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let search = round(paid).into_state();
    let wanted = physical(&search, 3).id();
    let RuleFrame::LibraryTop(domain) = search.pending_decision().unwrap().frame() else {
        panic!("search library")
    };
    assert!(domain.domain_top_first().contains(&wanted));
    let found = reply(
        &search,
        DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
            selected: Some(wanted),
        }),
    );
    assert_eq!(physical(found.state(), 3).zone(), ObjectZone::Hand(A));
    assert_eq!(found.shuffle_journal().len(), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kykar_creates_a_flying_spirit_on_cast_and_sacrifices_it_for_red_mana() {
    let landed = ObjectDestination::Battlefield { controller: A };
    let instant = seed(
        2,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::None,
            Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            },
        ),
    );
    let state = game(
        vec![A, B],
        vec![
            named("Kykar, Wind's Fury", 1, A, landed),
            instant,
            creature(3, A, 2, 2, &[]),
        ],
        &[],
        Step::PrecombatMain,
    );
    assert!(sheet(&state, physical(&state, 1).id()).has_keyword(KeywordAbility::Flying));
    let cast = cast_untargeted(&state, 2, &[]).into_state();
    assert_eq!(cast.execution().window().entries().len(), 2);
    let (made, spirits) = ordered_tokens(cast, 1);
    let spirit = sheet(&made, spirits[0]);
    assert!(spirit.has_keyword(KeywordAbility::Flying));
    assert_eq!(spirit.current_colors, [Color::White].into());
    assert_eq!(
        spirit.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Spirit").unwrap()].into()
    );
    let cash = |victim| GameCommand::ActivateManaWithCostObjects {
        source: physical(&made, 1).id(),
        ability: spellwake_engine::mana::ManaAbilityRef::Printed(
            spellwake_engine::mana::ManaAbilityId(0),
        ),
        objects: vec![victim],
    };
    illegal(&made, A, cash(physical(&made, 3).id()));
    let paid = command(&made, A, cash(spirits[0])).into_state();
    assert_eq!(paid.execution().window().entries().len(), 1);
    assert_eq!(paid.execution().mana(A, Red), 1.into());
    let done = round(paid).into_state();
    assert_eq!(done.execution().life_total(A), Some(&21.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nemata_creates_saprolings_and_its_sacrifice_pumps_both_players_saprolings() {
    let sap = current_catalogue().1.resolve_label("Saproling").unwrap();
    let opposite = seed(
        2,
        B,
        ObjectDestination::Battlefield { controller: B },
        Program::creature(None, 1.into(), 1.into(), [sap].into()),
    );
    let mana = [(Colourless, 2), (Green, 1)];
    let state = game(
        vec![A, B],
        vec![
            named(
                "Nemata, Grove Guardian",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            opposite,
            creature(3, A, 2, 2, &[]),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let activate = GameCommand::ActivateUntargetedAbility {
        source: physical(&state, 1).id(),
        ability: ActivatedAbilityId(0),
        payment: Some(spend(&mana)),
    };
    let (made, ids) = ordered_tokens(command(&state, A, activate).into_state(), 1);
    assert_eq!(sheet(&made, ids[0]).current_creature_subtypes, [sap].into());
    assert_size(&made, ids[0], 1, 1);
    let cash = |id| selected_cost(&made, 1, 1, vec![id], None, &[]);
    illegal(&made, A, cash(physical(&made, 2).id()));
    illegal(&made, A, cash(physical(&made, 3).id()));
    let pumped = round(command(&made, A, cash(ids[0])).into_state()).into_state();
    assert_size(&pumped, physical(&pumped, 2).id(), 2, 2);
    assert_size(&pumped, physical(&pumped, 3).id(), 2, 2);
    let expired = exile_hand::until(pumped, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_size(&expired, physical(&expired, 2).id(), 1, 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ur_dragon_reduces_other_dragons_in_both_zones_and_counts_a_single_attack_declaration() {
    let dragon = current_catalogue().1.resolve_label("Dragon").unwrap();
    for active in [false, true] {
        let destination = if active {
            ObjectDestination::Battlefield { controller: A }
        } else {
            ObjectDestination::Command
        };
        let mut cards = draw_pile();
        cards.push(named("The Ur-Dragon", 1, A, destination));
        for n in [2, 3, 4] {
            let destination = if n == 4 {
                ObjectDestination::Hand
            } else {
                ObjectDestination::Battlefield { controller: A }
            };
            let mut dragon_card = seed(
                n,
                A,
                destination,
                Program::creature(fixed_cost("{3}"), 1.into(), 1.into(), [dragon].into()),
            );
            if n != 4 {
                dragon_card
                    .program
                    .printed_keywords
                    .insert(KeywordAbility::Haste);
            }
            cards.push(dragon_card);
        }
        let initial = game(vec![A, B], cards, &[(Colourless, 2)], Step::PrecombatMain);
        if active {
            assert!(
                sheet(&initial, physical(&initial, 1).id()).has_keyword(KeywordAbility::Flying)
            );
        }
        let cast = cast_untargeted(&initial, 4, &[(Colourless, 2)]);
        assert_eq!(cast.mana_payment().unwrap().cost.generic, 2.into());
        let ready = round(cast.into_state()).into_state();
        let attack = combat_frame(&ready, true);
        let attacked = reply(
            &attack,
            DecisionReply::DeclareAttackers(
                [2, 3]
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
        if !active {
            assert!(attacked.execution().window().entries().is_empty());
            continue;
        }
        assert_eq!(attacked.execution().window().entries().len(), 1);
        let drawn = round(attacked).into_state();
        assert_eq!(hand_size(&drawn, A), 2);
        let RuleFrame::ResolutionSelect(frame) = drawn.pending_decision().unwrap().frame() else {
            panic!("choose permanent from hand")
        };
        let chosen = frame.domain()[0];
        let physical_id = drawn
            .execution()
            .window()
            .cards()
            .get(chosen)
            .unwrap()
            .card_id()
            .unwrap();
        let declined = reply(&drawn, DecisionReply::SelectResolution(vec![])).into_state();
        assert_eq!(hand_size(&declined, A), 2);
        let chosen = reply(&drawn, DecisionReply::SelectResolution(vec![chosen])).into_state();
        assert_eq!(hand_size(&chosen, A), 1);
        assert_eq!(
            physical(&chosen, physical_id.0).zone(),
            ObjectZone::Battlefield
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zhulodok_grants_two_separately_resolving_cascades_only_at_seven_mana_value() {
    for (price, cascades) in [(6, 0), (7, 2)] {
        let body = Program::artifact(fixed_cost(&format!("{{{price}}}")), vec![]);
        let mut cards = vec![
            named(
                "Zhulodok, Void Gorger",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(2, A, ObjectDestination::Hand, body),
        ];
        cards.extend((3..5).map(|n| {
            seed(
                n,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(Some(ManaCost::free()), vec![]),
            )
        }));
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, price)],
            Step::PrecombatMain,
        );
        let mut state = cast_untargeted(&initial, 2, &[(Colourless, price)]).into_state();
        let mut offers = 0;
        for _ in 0..40 {
            if physical(&state, 2).zone() != ObjectZone::Stack {
                break;
            }
            state = if let Some(pending) = state.pending_decision() {
                let choice = match pending.frame() {
                    RuleFrame::Triggers(frame) => {
                        DecisionReply::OrderTriggers(frame.instances().to_vec())
                    }
                    RuleFrame::OptionalEffect(frame)
                        if frame.exile_cast().is_some_and(|offer| {
                            offer.ability == spellwake_engine::effects::ExileCastAbility::Cascade
                        }) =>
                    {
                        offers += 1;
                        DecisionReply::CascadeCast {
                            target: None,
                            payment: Default::default(),
                        }
                    }
                    frame => panic!("unexpected cascade decision {frame:?}"),
                };
                reply(&state, choice).into_state()
            } else {
                pass(&state).into_state()
            };
        }
        assert_eq!(physical(&state, 2).zone(), ObjectZone::Battlefield);
        assert_eq!(offers, cascades);
        assert_eq!(
            [3, 4]
                .into_iter()
                .filter(|&n| physical(&state, n).zone() == ObjectZone::Battlefield)
                .count(),
            cascades
        );
    }
}

pub(super) fn commander_opening(
    mut cards: Vec<SeedCard>,
    identities: Option<BTreeMap<SeatId, BTreeSet<Color>>>,
) -> GameState {
    use spellwake_engine::{game::OpeningSetup, opening::MulliganChoice, shuffle::EntropyTape};
    let occupied = cards.iter().map(|card| card.card).collect::<BTreeSet<_>>();
    for n in 1..=200 {
        if !occupied.contains(&CardId(n)) {
            cards.push(seed(
                n,
                if n <= 100 { A } else { B },
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let words = [A, B]
        .into_iter()
        .flat_map(|player| {
            let library = cards
                .iter()
                .filter(|card| {
                    card.owner == player && card.destination == ObjectDestination::LibraryBottom
                })
                .count();
            (1..library as u64).rev()
        })
        .collect();
    let (_, vocab, _) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "commander-entry".into(),
        "combat-damage".into(),
        vocab.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![],
        vec![],
        40.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocab,
        names,
    );

    // Selecting the last position on each Fisher-Yates draw preserves the prepared library order.
    let setup = OpeningSetup {
        seed,
        entropy: EntropyTape::new(words),
    };
    let mut state = match identities {
        Some(ids) => GameState::initialize_commander_opening_with_identities(
            GameId(729),
            context,
            setup,
            ids,
        ),
        None => GameState::initialize_commander_opening(GameId(729), context, setup),
    }
    .unwrap()
    .into_state();
    while matches!(
        state.pending_decision().map(|p| p.frame()),
        Some(RuleFrame::Opening(_))
    ) {
        state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep)).into_state();
    }
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().step == Step::PrecombatMain
    })
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_geode_golem_casts_the_chosen_commander_during_the_combat_damage_trigger() {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let cards = vec![
        named("Geode Golem", 1, A, ObjectDestination::LibraryBottom),
        seed(
            2,
            A,
            ObjectDestination::LibraryBottom,
            Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed([(Colourless, 5.into())].into()),
                }],
            ),
        ),
        seed(
            3,
            A,
            ObjectDestination::LibraryBottom,
            free_instant(BodyTarget::SingleCreature, Instruction::ReturnToOwnersHand),
        ),
        named("Experiment Kraj", 100, A, ObjectDestination::Command),
        named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
    ];
    let state = commander_opening(cards, None);
    assert_eq!(physical(&state, 1).zone(), ObjectZone::Hand(A));
    let land = command(
        &state,
        A,
        GameCommand::PlayLand {
            land: physical(&state, 2).id(),
        },
    )
    .into_state();
    let mana = command(
        &land,
        A,
        GameCommand::ActivateMana {
            source: physical(&land, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let entered = round(cast_untargeted(&mana, 1, &[(Colourless, 5)]).into_state()).into_state();
    assert!(sheet(&entered, physical(&entered, 1).id()).has_keyword(KeywordAbility::Trample));
    let ready = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let attack = combat_frame(&ready, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 1).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let choose = exile_hand::until(
        attacked,
        |s| matches!(s.pending_decision().map(|p|p.frame()),Some(RuleFrame::ResolutionSelect(frame)) if frame.operation()==ResolutionSelectionKind::FoundSpellToCast),
    );
    assert_eq!(choose.execution().life_total(B), Some(&35.into()));
    let commander = physical(&choose, 100).id();
    let RuleFrame::ResolutionSelect(frame) = choose.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    assert_eq!(frame.domain(), &[commander]);
    let declined = reply(&choose, DecisionReply::SelectResolution(vec![])).into_state();
    assert_eq!(physical(&declined, 100).zone(), ObjectZone::Command);
    let offer = reply(&choose, DecisionReply::SelectResolution(vec![commander])).into_state();
    let cast = reply(
        &offer,
        DecisionReply::ImpulseCast {
            target: None,
            payment: Default::default(),
            mana_abilities: vec![],
        },
    )
    .into_state();
    assert_eq!(physical(&cast, 100).zone(), ObjectZone::Stack);
    assert_eq!(
        cast.execution()
            .commander_record(CardId(100))
            .unwrap()
            .command_zone_casts(),
        &1.into()
    );
    let done = round(cast).into_state();
    assert_eq!(physical(&done, 100).zone(), ObjectZone::Battlefield);
    // Return the commander, then verify that the next free cast still owes the tax.
    let bounce = command(
        &done,
        A,
        GameCommand::Cast {
            spell: physical(&done, 3).id(),
            target: TargetRef::Object(physical(&done, 100).id()),
        },
    )
    .into_state();
    let replacement = round(bounce).into_state();
    let returned = reply(
        &replacement,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(
            spellwake_engine::effects::ZoneOptionalAnswer::Apply,
        )),
    )
    .into_state();
    assert_eq!(physical(&returned, 100).zone(), ObjectZone::Command);
    let next = exile_hand::until(returned, |s| {
        s.turn().unwrap().position().number == 5
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let attack = combat_frame(&next, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 1).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let hit = exile_hand::until(attacked, |s| {
        s.turn().unwrap().position().step == Step::CombatDamage
            && !s.execution().window().entries().is_empty()
    });
    let funded = command(
        &hit,
        A,
        GameCommand::ActivateMana {
            source: physical(&hit, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let choose = round(funded).into_state();
    let offer = reply(
        &choose,
        DecisionReply::SelectResolution(vec![physical(&choose, 100).id()]),
    )
    .into_state();
    let decision = offer.pending_decision().unwrap().id();
    illegal(
        &offer,
        A,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::ImpulseCast {
                target: None,
                payment: CastingPayment {
                    mana: Some(spend(&[])),
                    ..Default::default()
                },
                mana_abilities: vec![],
            },
        },
    );
    let recast = reply(
        &offer,
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, 2)])),
                ..Default::default()
            },
            mana_abilities: vec![],
        },
    )
    .into_state();
    assert_eq!(recast.execution().mana(A, Colourless), 3.into());
    assert_eq!(
        recast
            .execution()
            .commander_record(CardId(100))
            .unwrap()
            .command_zone_casts(),
        &2.into()
    );
}

fn activation(
    state: &GameState,
    source: u64,
    index: u64,
    amount: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::ActivateUntargetedAbility {
        source: physical(state, source).id(),
        ability: ActivatedAbilityId(index),
        payment: (!amount.is_empty()).then(|| spend(amount)),
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_everflowing_chalice_produces_colorless_mana_for_each_paid_kicker() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    for n in [0, 3] {
        let required = n * 2;
        let initial = game(
            vec![A, B],
            vec![named("Everflowing Chalice", 1, A, ObjectDestination::Hand)],
            &[(Colourless, 6)],
            Step::PrecombatMain,
        );
        let cast = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    additional: if n == 0 {
                        Default::default()
                    } else {
                        [(0, n.into())].into()
                    },
                    mana: (n > 0).then(|| spend(&[(Colourless, required)])),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let landed = round(cast).into_state();
        let obj = physical(&landed, 1).id();
        assert_eq!(sheet(&landed, obj).charge_counters, n.into());
        let used = command(
            &landed,
            A,
            GameCommand::ActivateMana {
                source: obj,
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        assert_eq!(
            used.execution().mana(A, Colourless),
            (6 - required + n).into()
        );
        assert_eq!(physical(&used, 1).tapped(), Some(true));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ashling_counts_resolutions_and_explodes_on_exactly_the_third() {
    let state = game(
        vec![A, B],
        vec![
            named(
                "Ashling the Pilgrim",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, B, 2, 2, &[]),
            creature(3, B, 4, 4, &[]),
        ],
        &[(Colourless, 3), (Red, 3)],
        Step::PrecombatMain,
    );
    let mut current = state;
    for n in 1..=3 {
        let cast = command(
            &current,
            A,
            activation(&current, 1, 0, &[(Colourless, 1), (Red, 1)]),
        )
        .into_state();
        current = round(cast).into_state();
        if n < 3 {
            assert_eq!(
                sheet(&current, physical(&current, 1).id()).plus_one_counters,
                n.into()
            );
            assert_eq!(current.execution().life_total(B), Some(&20.into()));
        }
    }
    assert_eq!(current.execution().life_total(A), Some(&17.into()));
    assert_eq!(current.execution().life_total(B), Some(&17.into()));
    assert_eq!(physical(&current, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&current, 2).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&current, 3).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_yisan_counts_its_paid_verse_counter_even_if_removed_before_search() {
    let body = Program::creature(fixed_cost("{1}"), 2.into(), 2.into(), Default::default());
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Fervor",
                91,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Yisan, the Wanderer Bard",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(2, A, ObjectDestination::LibraryBottom, body),
            remove_spell(3, A),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(fixed_cost("{1}"), vec![]),
            ),
        ],
        &[(Colourless, 2), (Green, 1)],
        Step::PrecombatMain,
    );
    let paid = command(
        &initial,
        A,
        activation(&initial, 1, 0, &[(Colourless, 2), (Green, 1)]),
    )
    .into_state();
    let source = physical(&paid, 1).id();
    assert_eq!(
        paid.execution()
            .counters_of_kind(source, spellwake_engine::effects::CounterKind::Verse),
        1.into()
    );
    let removal = command(
        &paid,
        A,
        GameCommand::Cast {
            spell: physical(&paid, 3).id(),
            target: TargetRef::Object(source),
        },
    )
    .into_state();
    let removed = round(removal).into_state();
    assert_eq!(physical(&removed, 1).zone(), ObjectZone::Graveyard(A));
    let search = round(removed).into_state();
    let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
        panic!("search by verse count")
    };
    let wanted = physical(&search, 2).id();
    assert_eq!(frame.eligible_results(), &[wanted]);
    let search = search
        .supply_entropy(search.basis(), vec![0; 4])
        .unwrap()
        .into_state();
    let result = reply(
        &search,
        DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
            selected: Some(wanted),
        }),
    );
    assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Battlefield);
    assert_eq!(result.shuffle_journal().len(), 1);
}

fn activate_x(
    state: &GameState,
    card: u64,
    ability: u64,
    x: i64,
    price: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::ActivateWithRestrictedMana {
        source: physical(state, card).id(),
        ability: ActivatedAbilityId(ability),
        granting_effect: None,
        origin: spellwake_engine::activated::ActivationOrigin::Untargeted,
        chosen_x: Some(x.into()),
        payment: (!price.is_empty()).then(|| spend(price)),
        restricted_mana: vec![],
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_blast_zone_adds_mana_or_charges_and_uses_its_last_counter_count_after_sacrifice() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let artifact = |n, owner, price| {
        seed(
            n,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            Program::artifact(fixed_cost(price), vec![]),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            named("Blast Zone", 1, A, ObjectDestination::Hand),
            artifact(2, A, "{1}"),
            artifact(3, B, "{1}"),
            artifact(4, B, "{2}"),
            named(
                "Forest",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &[(Colourless, 4)],
        Step::PrecombatMain,
    );
    let played = command(
        &initial,
        A,
        GameCommand::PlayLand {
            land: physical(&initial, 1).id(),
        },
    )
    .into_state();
    let source = physical(&played, 1).id();
    assert_eq!(sheet(&played, source).charge_counters, 1.into());
    let tapped = command(
        &played,
        A,
        GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(tapped.execution().mana(A, Colourless), 5.into());
    illegal(&played, A, activate_x(&played, 1, 0, 2, &[(Colourless, 2)]));
    let charged =
        round(command(&played, A, activate_x(&played, 1, 0, 2, &[(Colourless, 4)])).into_state())
            .into_state();
    assert_eq!(sheet(&charged, source).charge_counters, 3.into());
    let paid = command(&played, A, activation(&played, 1, 1, &[(Colourless, 3)])).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    let destroyed = round(paid).into_state();
    assert_eq!(physical(&destroyed, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&destroyed, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&destroyed, 5).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_steel_hellkite_destroys_only_the_damaged_players_matching_nonlands_once() {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let land = Program::land(
        Default::default(),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed([(Colourless, 1.into())].into()),
        }],
    );
    let mut cards = vec![
        named(
            "Steel Hellkite",
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
        seed(3, A, ObjectDestination::Battlefield { controller: A }, land),
    ];
    for (n, player, cost) in [(4, B, "{1}"), (5, A, "{1}"), (6, C, "{1}"), (7, B, "{2}")] {
        cards.push(seed(
            n,
            player,
            ObjectDestination::Battlefield { controller: player },
            Program::artifact(fixed_cost(cost), vec![]),
        ));
    }
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 2)],
        Step::PrecombatMain,
    );
    assert!(sheet(&initial, physical(&initial, 1).id()).has_keyword(KeywordAbility::Flying));
    let pump =
        round(command(&initial, A, activation(&initial, 1, 0, &[(Colourless, 2)])).into_state())
            .into_state();
    assert_size(&pump, physical(&pump, 1).id(), 6, 5);
    let attack = combat_frame(&pump, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 1).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let ready = exile_hand::until(attacked, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(ready.execution().life_total(B), Some(&14.into()));
    let funded = command(
        &ready,
        A,
        GameCommand::ActivateMana {
            source: physical(&ready, 3).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let resolved =
        round(command(&funded, A, activate_x(&funded, 1, 1, 1, &[(Colourless, 1)])).into_state())
            .into_state();
    assert_eq!(physical(&resolved, 4).zone(), ObjectZone::Graveyard(B));
    for n in [3, 5, 6, 7] {
        assert_eq!(physical(&resolved, n).zone(), ObjectZone::Battlefield);
    }
    illegal(&resolved, A, activate_x(&resolved, 1, 1, 0, &[]));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sakiko_retains_combat_damage_mana_through_the_turn_then_empties_it() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Sakiko, Mother of Summer",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 3, 3, &[KeywordAbility::Haste]),
            creature(3, A, 2, 2, &[KeywordAbility::Haste]),
        ],
        &[(Green, 1)],
        Step::PrecombatMain,
    );
    let attack = combat_frame(&initial, true);
    assert_eq!(attack.execution().mana(A, Green), 0.into());
    let struck = reply(
        &attack,
        DecisionReply::DeclareAttackers(
            [2, 3]
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
    let mut current = struck;
    for _ in 0..60 {
        if current.turn().unwrap().position().step == Step::PostcombatMain {
            break;
        }
        current = if let Some(p) = current.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Triggers(frame) => {
                    DecisionReply::OrderTriggers(frame.instances().to_vec())
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                frame => panic!("unexpected {frame:?}"),
            };
            reply(&current, choice).into_state()
        } else {
            pass(&current).into_state()
        };
    }
    assert_eq!(
        current.turn().unwrap().position().step,
        Step::PostcombatMain
    );
    assert_eq!(current.execution().life_total(B), Some(&15.into()));
    assert_eq!(current.execution().mana(A, Green), 5.into());
    let end = exile_hand::until(current, |s| s.turn().unwrap().position().step == Step::End);
    assert_eq!(end.execution().mana(A, Green), 5.into());
    let next = exile_hand::until(end, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(next.execution().mana(A, Green), 0.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_witchs_cauldron_pays_mana_taps_and_sacrifices_before_drawing_and_gaining_life() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Witch's Cauldron",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 1, 1, &[]),
        ],
        &[(Colourless, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let cast = |id| {
        selected_cost(
            &initial,
            1,
            0,
            vec![id],
            None,
            &[(Colourless, 1), (Black, 1)],
        )
    };
    illegal(&initial, A, cast(physical(&initial, 3).id()));
    let paid = command(&initial, A, cast(physical(&initial, 2).id())).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(hand_size(&paid, A), 0);
    let done = round(paid).into_state();
    assert_eq!(done.execution().life_total(A), Some(&21.into()));
    assert_eq!(hand_size(&done, A), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fellwar_stone_and_exotic_orchard_query_opponents_land_mana_colors() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    for name in ["Fellwar Stone", "Exotic Orchard"] {
        let is_land = name == "Exotic Orchard";
        let mut cards = vec![named(name, 1, A, ObjectDestination::Hand)];
        for (id, owner, land) in [
            (2, A, "Forest"),
            (3, B, "Plains"),
            (4, B, "Mountain"),
            (5, B, "Wastes"),
        ] {
            cards.push(named(
                land,
                id,
                owner,
                ObjectDestination::Battlefield { controller: owner },
            ));
        }
        let initial = game(vec![A, B], cards, &[(Colourless, 2)], Step::PrecombatMain);
        let ready = if is_land {
            command(
                &initial,
                A,
                GameCommand::PlayLand {
                    land: physical(&initial, 1).id(),
                },
            )
            .into_state()
        } else {
            round(cast_untargeted(&initial, 1, &[(Colourless, 2)]).into_state()).into_state()
        };
        let choose = command(
            &ready,
            A,
            GameCommand::ActivateMana {
                source: physical(&ready, 1).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let RuleFrame::ManaColor(frame) = choose.pending_decision().unwrap().frame() else {
            panic!("choose among opponents' possible colors")
        };
        assert_eq!(frame.colors(), &[White, Red].into());
        for color in [White, Red] {
            let used = reply(&choose, DecisionReply::ManaColor(color)).into_state();
            assert_eq!(used.execution().mana(A, color), 1.into());
            assert_eq!(physical(&used, 1).tapped(), Some(true));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_extraplanar_lens_can_decline_imprint_and_rewards_either_players_same_named_land() {
    use spellwake_engine::{
        effects::OptionalEffectChoice, land::BasicLandType, mana::ManaAbilityRef,
    };
    let mut cards = vec![named("Extraplanar Lens", 1, A, ObjectDestination::Hand)];
    for (n, owner, name) in [
        (2, A, "Forest"),
        (3, A, "Forest"),
        (4, B, "Forest"),
        (5, B, "Mountain"),
    ] {
        cards.push(named(
            name,
            n,
            owner,
            ObjectDestination::Battlefield { controller: owner },
        ));
    }
    let initial = game(vec![A, B], cards, &[(Colourless, 3)], Step::PrecombatMain);
    let entered = round(cast_untargeted(&initial, 1, &[(Colourless, 3)]).into_state()).into_state();
    let target = physical(&entered, 2).id();
    let their_land = physical(&entered, 4).id();
    illegal(
        &entered,
        A,
        GameCommand::Reply {
            decision: entered.pending_decision().unwrap().id(),
            choice: DecisionReply::TriggerTarget(TargetRef::Object(their_land)),
        },
    );
    let targeted = reply(
        &entered,
        DecisionReply::TriggerTarget(TargetRef::Object(target)),
    )
    .into_state();
    let offer = round(targeted).into_state();
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&declined, 2).zone(), ObjectZone::Battlefield);
    let imprinted = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(physical(&imprinted, 2).zone(), ObjectZone::Exile);
    for (state, actor, id, land_type, color, amount) in [
        (declined.clone(), A, 3, BasicLandType::Forest, Green, 1),
        (imprinted.clone(), A, 3, BasicLandType::Forest, Green, 2),
        (
            pass(&imprinted).into_state(),
            B,
            4,
            BasicLandType::Forest,
            Green,
            2,
        ),
        (
            pass(&imprinted).into_state(),
            B,
            5,
            BasicLandType::Mountain,
            Red,
            1,
        ),
    ] {
        let mut tapped = command(
            &state,
            actor,
            GameCommand::ActivateMana {
                source: physical(&state, id).id(),
                ability: ManaAbilityRef::BasicLand(land_type),
            },
        )
        .into_state();
        if matches!(
            tapped.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::ManaColor(_))
        ) {
            tapped = reply(&tapped, DecisionReply::ManaColor(color)).into_state();
        }
        assert_eq!(tapped.execution().mana(actor, color), amount.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mazemind_tome_scry_draw_and_fourth_page_exile_with_life_gain() {
    use spellwake_engine::effects::{CounterKind, LibraryTopReply};
    let mut cards = draw_pile();
    cards.extend((31..36).map(|n| {
        seed(
            n,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    cards.push(named(
        "Mazemind Tome",
        1,
        A,
        ObjectDestination::Battlefield { controller: A },
    ));
    let state = game(vec![A, B], cards, &[(Colourless, 2)], Step::PrecombatMain);
    let source = physical(&state, 1).id();
    let paid = command(&state, A, activation(&state, 1, 1, &[(Colourless, 2)])).into_state();
    assert_eq!(
        paid.execution().counters_of_kind(source, CounterKind::Page),
        1.into()
    );
    let mut state = round(paid).into_state();
    assert_eq!(hand_size(&state, A), 1);
    for (turn, page) in [(3, 2), (5, 3), (7, 4)] {
        let main = exile_hand::until(state, |s| {
            s.turn().unwrap().position().number == turn
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let activated = command(&main, A, activation(&main, 1, 0, &[])).into_state();
        assert_eq!(
            activated
                .execution()
                .counters_of_kind(source, CounterKind::Page),
            page.into()
        );
        let pending = if page == 4 {
            let exiled = round(activated).into_state();
            assert_eq!(physical(&exiled, 1).zone(), ObjectZone::Exile);
            assert_eq!(exiled.execution().life_total(A), Some(&24.into()));
            round(exiled).into_state()
        } else {
            round(activated).into_state()
        };
        let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
            panic!("scry one")
        };
        assert_eq!(frame.domain_top_first().len(), 1);
        let top = frame.domain_top_first()[0];
        state = reply(
            &pending,
            DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: vec![],
                bottom_top_first: vec![top],
            }),
        )
        .into_state();
        assert_eq!(
            state.execution().window().cards().library(A).last(),
            Some(&top)
        );
    }
}

fn partner_mana_game(land: &str, mut spells: Vec<SeedCard>) -> GameState {
    let mut piper = named("The Prismatic Piper", 100, A, ObjectDestination::Command);
    *piper.program.commander_color_choice_mut().unwrap() = Some(Color::Blue);
    let mut cards = vec![
        named(land, 1, A, ObjectDestination::LibraryBottom),
        piper,
        named(
            "Keleth, Sunmane Familiar",
            99,
            A,
            ObjectDestination::Command,
        ),
        named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
    ];
    cards.append(&mut spells);
    commander_opening(
        cards,
        Some(
            [
                (A, [Color::Blue, Color::White].into()),
                (B, [Color::Green].into()),
            ]
            .into(),
        ),
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_opal_palace_and_prismatic_piper_use_the_chosen_color_partner_identity_and_commander_cast_count()
 {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let mut rock = Program::artifact(Some(ManaCost::free()), vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 5.into())].into()),
    });
    let game = partner_mana_game(
        "Opal Palace",
        vec![seed(2, A, ObjectDestination::LibraryBottom, rock)],
    );
    assert_eq!(
        game.execution().commander_chosen_color(CardId(100)),
        Some(Color::Blue)
    );
    assert_eq!(
        sheet(&game, physical(&game, 100).id()).current_colors,
        [Color::Blue].into()
    );
    let rock = round(cast_untargeted(&game, 2, &[]).into_state()).into_state();
    let played = command(
        &rock,
        A,
        GameCommand::PlayLand {
            land: physical(&rock, 1).id(),
        },
    )
    .into_state();
    let tap = |state: &GameState, n, index| GameCommand::ActivateMana {
        source: physical(state, n).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(index)),
    };
    let plain = command(&played, A, tap(&played, 1, 0)).into_state();
    assert_eq!(plain.execution().mana(A, Colourless), 1.into());
    let funded = command(&played, A, tap(&played, 2, 0)).into_state();
    let offer = command(
        &funded,
        A,
        GameCommand::ActivateManaWithPayment {
            source: physical(&funded, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            payment: spend(&[(Colourless, 1)]),
        },
    )
    .into_state();
    let RuleFrame::ManaColor(frame) = offer.pending_decision().unwrap().frame() else {
        panic!("commander identity colors")
    };
    assert_eq!(frame.colors(), &[White, Blue].into());
    let mana = reply(&offer, DecisionReply::ManaColor(Blue)).into_state();
    let cast = cast_untargeted(&mana, 100, &[(Colourless, 4), (Blue, 1)]).into_state();
    assert_eq!(
        sheet(&cast, physical(&cast, 100).id()).current_colors,
        [Color::Blue].into()
    );
    let done = round(cast).into_state();
    let sheet = sheet(&done, physical(&done, 100).id());
    assert_eq!(sheet.current_colors, [Color::Blue].into());
    assert_eq!(sheet.plus_one_counters, 1.into());
    assert_eq!(
        done.execution()
            .commander_record(CardId(100))
            .unwrap()
            .command_zone_casts(),
        &1.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_path_of_ancestry_scries_only_when_its_mana_casts_a_creature_sharing_a_commanders_type() {
    use spellwake_engine::{
        effects::LibraryTopReply,
        mana::{ManaAbilityId, ManaAbilityRef},
    };
    let (_, vocab, _) = current_catalogue();
    let spells = ["Shapeshifter", "Goblin"]
        .into_iter()
        .enumerate()
        .map(|(i, ty)| {
            seed(
                i as u64 + 2,
                A,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    fixed_cost("{1}"),
                    1.into(),
                    1.into(),
                    [vocab.resolve_label(ty).unwrap()].into(),
                ),
            )
        })
        .collect();
    let initial = partner_mana_game("Path of Ancestry", spells);
    let played = command(
        &initial,
        A,
        GameCommand::PlayLand {
            land: physical(&initial, 1).id(),
        },
    )
    .into_state();
    assert_eq!(physical(&played, 1).tapped(), Some(true));
    let ready = exile_hand::until(played, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let offer = command(
        &ready,
        A,
        GameCommand::ActivateMana {
            source: physical(&ready, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let RuleFrame::ManaColor(frame) = offer.pending_decision().unwrap().frame() else {
        panic!("partner identity")
    };
    assert_eq!(frame.colors(), &[White, Blue].into());
    let mana = reply(&offer, DecisionReply::ManaColor(White)).into_state();
    let distinct = cast_untargeted(&mana, 3, &[(White, 1)]).into_state();
    assert_eq!(distinct.execution().window().entries().len(), 1);
    let shared = cast_untargeted(&mana, 2, &[(White, 1)]).into_state();
    assert_eq!(shared.execution().window().entries().len(), 2);
    let scry = round(shared).into_state();
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        panic!("scry one")
    };
    assert_eq!(frame.domain_top_first().len(), 1);
    let done = reply(
        &scry,
        DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: frame.domain_top_first().to_vec(),
            bottom_top_first: vec![],
        }),
    )
    .into_state();
    let resolved = round(done).into_state();
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_titans_presence_and_calamity_reveal_only_colorless_creatures_and_compare_the_correct_characteristic()
 {
    for (name, mana, targeted) in [
        ("Titan's Presence", 3, true),
        ("Calamity of the Titans", 6, false),
    ] {
        let creature_at = |n, price, power: i64| {
            seed(
                n,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::creature(
                    fixed_cost(price),
                    power.into(),
                    7.into(),
                    Default::default(),
                ),
            )
        };
        let mut colored =
            Program::creature(fixed_cost("{R}"), 6.into(), 6.into(), Default::default());
        colored.effect_defined_colors = Some([Color::Red].into());
        let mut cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{6}"), 3.into(), 6.into(), Default::default()),
            ),
            seed(3, A, ObjectDestination::Hand, colored),
            creature_at(4, "{6}", 3),
            creature_at(5, "{2}", 4),
            seed(
                6,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(fixed_cost("{1}"), vec![]),
            ),
        ];
        if !targeted {
            cards.push(named("Commodore Guff", 7, A, ObjectDestination::Hand));
        }
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, mana + 1), (Blue, 1), (Red, 1), (White, 1)],
            Step::PrecombatMain,
        );
        let initial = if targeted {
            initial
        } else {
            round(
                cast_untargeted(
                    &initial,
                    7,
                    &[(Colourless, 1), (Blue, 1), (Red, 1), (White, 1)],
                )
                .into_state(),
            )
            .into_state()
        };
        let cast = |reveal: Option<u64>, target: u64| GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: targeted.then(|| TargetRef::Object(physical(&initial, target).id())),
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, mana)])),
                objects: reveal
                    .map(|n| CastResource::RevealColorlessCreature(physical(&initial, n).id()))
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
        };
        illegal(&initial, A, cast(None, 4));
        illegal(&initial, A, cast(Some(3), 4));
        let paid = command(&initial, A, cast(Some(2), 4)).into_state();
        assert_eq!(physical(&paid, 2).zone(), ObjectZone::Hand(A));
        let done = round(paid).into_state();
        if targeted {
            assert_eq!(physical(&done, 4).zone(), ObjectZone::Exile);
            let larger = round(command(&initial, A, cast(Some(2), 5)).into_state()).into_state();
            assert_eq!(physical(&larger, 5).zone(), ObjectZone::Battlefield);
        } else {
            assert_eq!(physical(&done, 4).zone(), ObjectZone::Battlefield);
            assert_eq!(physical(&done, 5).zone(), ObjectZone::Exile);
            assert_eq!(physical(&done, 7).zone(), ObjectZone::Exile);
        }
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Hand(A));
        assert_eq!(physical(&done, 6).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_xantcha_enters_under_an_opponent_any_player_draws_and_it_must_attack_away_from_its_owner() {
    let initial = game(
        vec![A, B, C],
        vec![named(
            "Xantcha, Sleeper Agent",
            1,
            A,
            ObjectDestination::Hand,
        )],
        &[(Colourless, 4), (Red, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let entry =
        round(cast_untargeted(&initial, 1, &[(Colourless, 1), (Red, 1), (Black, 1)]).into_state())
            .into_state();
    let RuleFrame::ZoneReplacement(frame) = entry.pending_decision().unwrap().frame() else {
        panic!("choose Xantcha's controller")
    };
    assert!(
        matches!(frame.request(),ZoneReplacementRequest::EntryController{opponents,..} if opponents.iter().copied().collect::<BTreeSet<_>>()==[B,C].into())
    );
    let given = reply(
        &entry,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryController(B)),
    )
    .into_state();
    assert_eq!(physical(&given, 1).controller(), Some(B));
    let drawing = command(&given, A, activation(&given, 1, 0, &[(Colourless, 3)])).into_state();
    let done = round(drawing).into_state();
    assert_eq!(done.execution().life_total(B), Some(&18.into()));
    assert_eq!(done.execution().life_total(A), Some(&20.into()));
    assert_eq!(hand_size(&done, A), 1);
    let main = exile_hand::until(done, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let attacks = combat_frame(&main, true);
    let who = physical(&attacks, 1).id();
    let attack = |defender| AttackDeclaration {
        attacker: who,
        defender,
        defending_planeswalker: None,
    };
    for refused in [vec![], vec![attack(A)]] {
        illegal(
            &attacks,
            B,
            GameCommand::Reply {
                decision: attacks.pending_decision().unwrap().id(),
                choice: DecisionReply::DeclareAttackers(refused),
            },
        );
    }
    let valid = reply(&attacks, DecisionReply::DeclareAttackers(vec![attack(C)])).into_state();
    let damage = exile_hand::until(valid, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(damage.execution().life_total(C), Some(&15.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hunters_insight_draws_from_damage_to_a_planeswalker() {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let mana = [(Colourless, 1), (Blue, 1), (Red, 1), (White, 1)];
    let mut cards = vec![
        named("Hunter's Insight", 1, B, ObjectDestination::Hand),
        creature(2, B, 3, 3, &[]),
        named("Commodore Guff", 3, A, ObjectDestination::Hand),
        seed(
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed([(Green, 3.into())].into()),
                }],
            ),
        ),
    ];
    cards.extend((13..19).map(|n| {
        seed(
            n,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    let initial = game(vec![A, B], cards, &mana, Step::PrecombatMain);
    let entered = round(cast_untargeted(&initial, 3, &mana).into_state()).into_state();
    let main = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let start = command(
        &main,
        B,
        GameCommand::ActivateMana {
            source: physical(&main, 4).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let targeted = command(
        &start,
        B,
        GameCommand::CastPaid {
            spell: physical(&start, 1).id(),
            target: TargetRef::Object(physical(&start, 2).id()),
            payment: spend(&[(Green, 3)]),
        },
    )
    .into_state();
    let ready = round(targeted).into_state();
    let hand_before = hand_size(&ready, B);
    let attack = combat_frame(&ready, true);
    let attacked = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attack, 2).id(),
            defender: A,
            defending_planeswalker: Some(physical(&attack, 3).id()),
        }]),
    )
    .into_state();
    let end = exile_hand::until(attacked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(hand_size(&end, B), hand_before + 3);
    assert_eq!(end.execution().life_total(A), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_flying_and_trample_restrict_blocks_and_assign_real_excess_damage() {
    use spellwake_engine::combat::CombatDamageAssignment;
    for (name, flies, tramples, power) in [
        ("Kykar, Wind's Fury", true, false, 3),
        ("The Ur-Dragon", true, false, 10),
        ("Steel Hellkite", true, false, 5),
        ("Razaketh, the Foulblooded", true, true, 8),
        ("Geode Golem", false, true, 5),
        ("Demon of Fate's Design", true, true, 6),
        ("Nyxborn Behemoth", false, true, 10),
    ] {
        let mut cards = draw_pile();
        cards.extend([
            named(name, 1, A, ObjectDestination::Battlefield { controller: A }),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, B, 1, 1, &[]),
            creature(4, B, 1, 1, &[KeywordAbility::Reach]),
        ]);
        let initial = game(vec![A, B], cards, &[], Step::PrecombatMain);
        let attack = combat_frame(&initial, true);
        let source = physical(&attack, 1).id();
        let attacked = reply(
            &attack,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: source,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        let blocks = exile_hand::until(attacked, |s| {
            matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
            )
        });
        let block = |id| {
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                attacker: source,
                blocker: physical(&blocks, id).id(),
            }])
        };
        if flies {
            illegal(
                &blocks,
                B,
                GameCommand::Reply {
                    decision: blocks.pending_decision().unwrap().id(),
                    choice: block(3),
                },
            );
        }
        let blocked = reply(&blocks, block(4)).into_state();
        let after = if tramples {
            let assigning = round(blocked).into_state();
            reply(
                &assigning,
                DecisionReply::AssignCombatDamage(vec![
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Creature(physical(&assigning, 4).id()),
                        amount: 1.into(),
                    },
                    CombatDamageAssignment {
                        source,
                        target: DamageRecipient::Player(B),
                        amount: (power - 1_i64).into(),
                    },
                ]),
            )
            .into_state()
        } else {
            blocked
        };
        let end = exile_hand::until(after, |s| {
            s.turn().unwrap().position().step == Step::EndCombat
        });
        assert_eq!(physical(&end, 4).zone(), ObjectZone::Graveyard(B));
        assert_eq!(
            end.execution().life_total(B),
            Some(&(if tramples { 21 - power } else { 20 }).into()),
            "{name}"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_urza_construct_counts_artifacts_taps_one_for_mana_and_plays_the_shuffled_top_for_free() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let mut cards = vec![
        named("Urza, Lord High Artificer", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        ),
        seed(
            3,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(fixed_cost("{9}"), vec![]),
        ),
    ];
    cards.push(creature(4, B, 1, 1, &[]));
    let state = game(
        vec![A, B],
        cards,
        &[(Colourless, 7), (Blue, 2)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&state, 1, &[(Colourless, 2), (Blue, 2)]).into_state()).into_state();
    let (made, tokens) = ordered_tokens(entered, 1);
    let token = tokens[0];
    let props = sheet(&made, token);
    assert!(props.has_type(CardType::Artifact));
    assert!(props.current_colors.is_empty());
    assert_eq!(
        props.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Construct").unwrap()].into()
    );
    assert_size(&made, token, 2, 2);
    let source = physical(&made, 1).id();
    let use_mana = |object| GameCommand::ActivateManaWithCostObjects {
        source,
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        objects: vec![object],
    };
    illegal(&made, A, use_mana(physical(&made, 4).id()));
    let mana = command(&made, A, use_mana(token)).into_state();
    assert_eq!(mana.execution().mana(A, Blue), 1.into());
    assert_eq!(
        mana.execution()
            .window()
            .cards()
            .get(token)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(physical(&mana, 1).tapped(), Some(false));
    let active = command(&mana, A, activation(&mana, 1, 0, &[(Colourless, 5)])).into_state();
    let ready = active
        .supply_entropy(active.basis(), vec![1, 0, 0])
        .unwrap()
        .into_state();
    let revealed = round(ready);
    assert_eq!(revealed.shuffle_journal().len(), 1);
    let exile = revealed.into_state();
    assert_eq!(physical(&exile, 3).zone(), ObjectZone::Exile);
    let expired = exile_hand::until(exile.clone(), |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    illegal(
        &expired,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&expired, 3).id(),
            payment: None,
        },
    );
    let free = round(cast_untargeted(&exile, 3, &[]).into_state()).into_state();
    assert_eq!(physical(&free, 3).zone(), ObjectZone::Battlefield);
    assert_size(&free, token, 3, 3);
}

mod counter_and_sacrifice;

mod alternative_costs;
mod mana_sources;

mod monarch;
mod proliferation;

mod casting_permissions;

mod mass_removal;

mod graveyard_triggers;
