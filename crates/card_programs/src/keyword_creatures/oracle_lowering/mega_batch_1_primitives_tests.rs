//! Behavioral engine trajectory tests for Mega-Batch 1 primitives and lowering unlocks.
use super::engine_trajectories as trajectories;
use super::*;
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration},
    effects::{DecisionReply, OptionalEffectChoice, SeedCard},
    game::{GameCommand, GameInput, GameState},
    mana::{CastingPayment, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    targets::TargetRef,
    turn::Step,
};
use trajectories::{
    P0, P1, card, controlled, current, land_source, library, object, pass_in_order, pass_pair,
    permanent, reply, submit, vocabulary, world,
};

fn make_card(
    name: &str,
    type_line: &str,
    mana_cost: &str,
    power: Option<&str>,
    toughness: Option<&str>,
    text: &str,
) -> InventoryCard {
    InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
        name: name.into(),
        layout: "normal".into(),
        faces: vec![spellwake_content::CatalogFace {
            name: name.into(),
            type_line: Some(type_line.into()),
            oracle_text: Some(text.into()),
            mana_cost: Some(mana_cost.into()),
            power: power.map(str::to_owned),
            toughness: toughness.map(str::to_owned),
        }],
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: true,
        program_evidence: "mega-batch-1-behavioral-test",
    }
}

fn hand_card(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program,
    }
}

fn free_instant(text: &str, vocab: &CreatureTypeVocabulary) -> Program {
    compile_with_reason(
        &make_card("Test Instant", "Instant", "{0}", None, None, text),
        vocab,
    )
    .expect("instant compiles")
}

fn free_sorcery(text: &str, vocab: &CreatureTypeVocabulary) -> Program {
    compile_with_reason(
        &make_card("Test Sorcery", "Sorcery", "{0}", None, None, text),
        vocab,
    )
    .expect("sorcery compiles")
}

fn try_cmd(game: &GameState, actor: SeatId, command: GameCommand) -> bool {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .is_ok()
}

fn life(game: &GameState, seat: SeatId) -> spellwake_contracts::RulesInteger {
    game.execution()
        .life_total(seat)
        .expect("seat life total")
        .clone()
}

fn hand_count(game: &GameState, seat: SeatId) -> usize {
    game.execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(seat))
        .count()
}

fn drain_graveyard_orders(mut g: GameState) -> GameState {
    while let Some(decision) = g.pending_decision() {
        let chooser = decision.chooser();
        if let spellwake_engine::game::RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame()
        {
            let order = frame.domain().to_vec();
            g = reply(&g, chooser, DecisionReply::OrderResolutionGraveyard(order)).into_state();
        } else {
            break;
        }
    }
    g
}

fn step_turn_forward(g: GameState) -> GameState {
    if let Some(decision) = g.pending_decision() {
        let chooser = decision.chooser();
        let choice = match decision.frame() {
            spellwake_engine::game::RuleFrame::Combat(
                spellwake_engine::combat::CombatDecision::Attackers(_),
            ) => DecisionReply::DeclareAttackers(vec![]),
            spellwake_engine::game::RuleFrame::Combat(
                spellwake_engine::combat::CombatDecision::Blockers(_),
            ) => DecisionReply::DeclareBlockers(vec![]),
            other => panic!("unexpected pending frame while advancing turn: {other:?}"),
        };
        return reply(&g, chooser, choice).into_state();
    }
    let actor = match g.execution().window().phase() {
        spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) => holder,
        other => panic!("unexpected window phase: {other:?}"),
    };
    submit(&g, actor, GameCommand::Pass).into_state()
}

#[test]
fn mega_batch_1_opponents_sorcery_only_blocks_off_turn_and_nonempty_stack_casts() {
    let vocab = vocabulary();
    let lock = compile_with_reason(
        &make_card(
            "Time Raveler Lock Test",
            "Enchantment",
            "{2}{W}",
            None,
            None,
            "Each opponent can cast spells only any time they could cast a sorcery.",
        ),
        &vocab,
    )
    .expect("OpponentsSorceryOnly compiles");
    let opp_instant = free_instant("You gain 1 life.", &vocab);
    let own_instant = free_instant("You gain 1 life.", &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, lock, true),
            hand_card(2, P1, opp_instant),
            hand_card(3, P0, own_instant),
            library(10, P0, &vocab),
            library(11, P1, &vocab),
        ],
        vec![CardId(1)],
    );

    // P0 passes priority on P0's main phase so P1 holds priority off-turn.
    let p1_prio = submit(&game, P0, GameCommand::Pass).into_state();
    let p1_obj = object(&p1_prio, 2);
    // Negative: opponent cannot cast an instant on P0's turn.
    assert!(
        !try_cmd(
            &p1_prio,
            P1,
            GameCommand::CastUntargeted {
                spell: p1_obj,
                payment: Some(ManaPayment::default()),
            },
        ),
        "opponent must be blocked from casting an instant during P0's turn"
    );

    // Advance into P1's first main phase.
    let mut g = p1_prio;
    let mut safety = 0;
    while !(g.execution().window().active_player() == P1
        && g.turn().unwrap().position().step == Step::PrecombatMain)
    {
        assert!(safety < 40, "failed to reach P1 main phase");
        safety += 1;
        g = step_turn_forward(g);
    }

    // Positive (controller unaffected): P0 casts an instant on P1's turn after P1 passes.
    let p0_on_p1_turn = submit(&g, P1, GameCommand::Pass).into_state();
    let p0_spell = object(&p0_on_p1_turn, 3);
    let stack_nonempty = submit(
        &p0_on_p1_turn,
        P0,
        GameCommand::CastUntargeted {
            spell: p0_spell,
            payment: Some(ManaPayment::default()),
        },
    )
    .into_state();
    let p1_responding = submit(&stack_nonempty, P0, GameCommand::Pass).into_state();
    let p1_obj_resp = object(&p1_responding, 2);
    // Negative: opponent cannot respond with an instant while stack is non-empty even on P1's main phase.
    assert!(
        !try_cmd(
            &p1_responding,
            P1,
            GameCommand::CastUntargeted {
                spell: p1_obj_resp,
                payment: Some(ManaPayment::default()),
            },
        ),
        "opponent must be blocked from casting while stack is non-empty"
    );

    // Let P0's spell resolve so stack is empty during P1's main phase.
    let empty_stack_p1_main = submit(&p1_responding, P1, GameCommand::Pass).into_state();
    let p1_obj_ok = object(&empty_stack_p1_main, 2);
    // Positive: opponent CAN cast during their own main phase with an empty stack.
    let cast_ok = submit(
        &empty_stack_p1_main,
        P1,
        GameCommand::CastUntargeted {
            spell: p1_obj_ok,
            payment: Some(ManaPayment::default()),
        },
    )
    .into_state();
    let resolved = pass_in_order(&cast_ok, [P1, P0]).into_state();
    assert_eq!(life(&resolved, P1), 21.into());
}

#[test]
fn mega_batch_1_remove_subject_from_combat_prevents_combat_damage() {
    let vocab = vocabulary();
    let gustcloak = compile_with_reason(
        &make_card(
            "Gustcloak Runner Test",
            "Creature \u{2014} Human Soldier",
            "{W}",
            Some("1"),
            Some("1"),
            "Whenever this creature becomes blocked, you may untap it and remove it from combat.",
        ),
        &vocab,
    )
    .expect("Gustcloak Runner compiles");
    let blocker = permanent(&vocab, &[CardType::Creature], Some((3, 3)), &["Bear"]);

    let game = world(
        vocab.clone(),
        vec![
            controlled(1, P0, P0, gustcloak),
            controlled(2, P1, P1, blocker),
            library(10, P0, &vocab),
            library(11, P1, &vocab),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Advance from PrecombatMain to DeclareAttackers.
    let g = pass_pair(&game).into_state(); // BeginningOfCombat
    let g = pass_pair(&g).into_state(); // DeclareAttackers frame
    let atk_id = object(&g, 1);
    let g = reply(
        &g,
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: atk_id,
            defender: P1,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let g = pass_pair(&g).into_state(); // DeclareBlockers frame
    let blk_id = object(&g, 2);
    let g = reply(
        &g,
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: blk_id,
            attacker: atk_id,
        }]),
    )
    .into_state();

    // Gustcloak trigger is on the stack; pass priority to resolve and accept the optional untap + remove.
    let g = pass_pair(&g).into_state();
    let g = reply(
        &g,
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert!(
        !current(&g, atk_id).attacking,
        "attacker must be removed from combat"
    );
    assert!(
        g.execution().window().cards().get(atk_id).unwrap().tapped() == Some(false),
        "attacker must be untapped by Gustcloak trigger"
    );

    // Advance through CombatDamage: 1/1 Gustcloak survives blocking by 3/3 Bear because it was removed!
    let g = pass_pair(&g).into_state(); // CombatDamage
    let g = pass_pair(&g).into_state(); // EndOfCombat
    assert_eq!(
        g.execution().window().cards().get(atk_id).unwrap().zone(),
        ObjectZone::Battlefield,
        "removed attacker must survive combat against the 3/3 blocker"
    );
}

#[test]
fn mega_batch_1_has_x_in_cost_matches_only_x_spells() {
    let vocab = vocabulary();
    let counter_x = compile_with_reason(
        &make_card(
            "X Counter Test",
            "Instant",
            "{0}",
            None,
            None,
            "Counter target spell with {X} in its mana cost.",
        ),
        &vocab,
    )
    .expect("HAS_X_IN_COST spell compiles");
    let x_spell = compile_with_reason(
        &make_card(
            "X Gain Test",
            "Sorcery",
            "{X}",
            None,
            None,
            "You gain X life.",
        ),
        &vocab,
    )
    .expect("X spell compiles");
    let non_x_spell = free_sorcery("You gain 2 life.", &vocab);

    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, x_spell),
            hand_card(2, P0, non_x_spell),
            hand_card(3, P1, counter_x.clone()),
            hand_card(4, P1, counter_x),
        ],
        vec![],
    );

    // Negative: counter_x must reject a spell without {X} in its mana cost.
    let non_x_obj = object(&game, 2);
    let g_non_x = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: non_x_obj,
            payment: Some(ManaPayment::default()),
        },
    )
    .into_state();
    let g_non_x_p1 = submit(&g_non_x, P0, GameCommand::Pass).into_state();
    let non_x_entry = g_non_x_p1.execution().window().entries()[0].id();
    assert!(
        !try_cmd(
            &g_non_x_p1,
            P1,
            GameCommand::CastPaid {
                spell: object(&g_non_x_p1, 3),
                target: TargetRef::Stack(non_x_entry),
                payment: ManaPayment::default(),
            },
        ),
        "spell without {{X}} in mana cost must not be a legal target"
    );

    // Positive: counter_x accepts and counters the {X} spell.
    let x_obj = object(&game, 1);
    let pay_x = CastingPayment {
        chosen_x: Some(0.into()),
        ..CastingPayment::default()
    };
    let g_x = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: x_obj,
            target: None,
            payment: pay_x,
        },
    )
    .into_state();
    let g_x_p1 = submit(&g_x, P0, GameCommand::Pass).into_state();
    let x_entry = g_x_p1.execution().window().entries()[0].id();
    let g_countered = submit(
        &g_x_p1,
        P1,
        GameCommand::CastPaid {
            spell: object(&g_x_p1, 4),
            target: TargetRef::Stack(x_entry),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_done = pass_in_order(&g_countered, [P1, P0]).into_state();
    assert!(g_done.execution().window().entries().is_empty());
    assert_eq!(life(&g_done, P0), 20.into());
}

#[test]
fn mega_batch_1_last_action_matching_count_counts_only_filtered_destroyed_objects() {
    let vocab = vocabulary();
    let wrath = free_sorcery(
        "Destroy all creatures. You gain life equal to the number of creatures you controlled that were destroyed this way.",
        &vocab,
    );
    let bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);

    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, wrath),
            controlled(2, P0, P0, bear.clone()),
            controlled(3, P0, P0, bear.clone()),
            controlled(4, P1, P1, bear.clone()),
            controlled(5, P1, P1, bear),
        ],
        vec![CardId(2), CardId(3), CardId(4), CardId(5)],
    );
    let cast_wrath = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    )
    .into_state();
    let resolved = drain_graveyard_orders(pass_pair(&cast_wrath).into_state());
    for id in [2u64, 3, 4, 5] {
        assert_ne!(
            resolved
                .execution()
                .window()
                .cards()
                .get(object(&resolved, id))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
    }
    assert_eq!(
        life(&resolved, P0),
        22.into(),
        "must count only the 2 creatures P0 controlled (not opponent's 2)"
    );
}

#[test]
fn mega_batch_1_skip_next_step_skips_only_one_draw_step_then_resumes() {
    let vocab = vocabulary();
    let skip_draw = free_sorcery("Target player skips their next draw step.", &vocab);
    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, skip_draw),
            library(10, P0, &vocab),
            library(11, P0, &vocab),
            library(12, P0, &vocab),
            library(20, P1, &vocab),
            library(21, P1, &vocab),
            library(22, P1, &vocab),
        ],
        vec![],
    );
    assert_eq!(hand_count(&game, P1), 0);
    let cast_skip = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let mut g = pass_pair(&cast_skip).into_state();

    // Advance into P1's first PrecombatMain: P1's draw step was skipped so hand stays 0.
    let mut safety = 0;
    while !(g.execution().window().active_player() == P1
        && g.turn().unwrap().position().step == Step::PrecombatMain)
    {
        assert!(safety < 40, "failed to reach P1 turn 1 main");
        safety += 1;
        g = step_turn_forward(g);
    }
    assert_eq!(hand_count(&g, P1), 0, "P1's next draw step must be skipped");

    // Advance through P0's turn and into P1's SECOND PrecombatMain: P1 draws normally now!
    let turn_p1_first = g.turn().unwrap().position().number;
    while !(g.execution().window().active_player() == P1
        && g.turn().unwrap().position().step == Step::PrecombatMain
        && g.turn().unwrap().position().number > turn_p1_first)
    {
        assert!(safety < 120, "failed to reach P1 turn 2 main");
        safety += 1;
        g = step_turn_forward(g);
    }
    assert_eq!(
        hand_count(&g, P1),
        1,
        "skip must be consumed after one skipped draw step; second turn draws normally"
    );
}

#[test]
fn mega_batch_1_sacrifice_up_to_allows_zero_partial_and_rejects_nonmatching() {
    let vocab = vocabulary();
    let sac_any = free_sorcery(
        "Sacrifice any number of creatures. Draw a card for each creature sacrificed this way.",
        &vocab,
    );
    let bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);
    let land = land_source();

    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, sac_any),
            controlled(2, P0, P0, bear.clone()),
            controlled(3, P0, P0, bear.clone()),
            controlled(4, P0, P0, bear),
            controlled(5, P0, P0, land),
            library(10, P0, &vocab),
            library(11, P0, &vocab),
            library(12, P0, &vocab),
        ],
        vec![CardId(2), CardId(3), CardId(4), CardId(5)],
    );

    let g_cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    )
    .into_state();
    let g_pending = pass_pair(&g_cast).into_state();
    let b2 = object(&g_pending, 2);
    let b3 = object(&g_pending, 3);
    let land_obj = object(&g_pending, 5);
    let decision_id = g_pending.pending_decision().expect("selection frame").id();

    // Negative: attempting to sacrifice a land (non-creature) must fail.
    assert!(
        !try_cmd(
            &g_pending,
            P0,
            GameCommand::Reply {
                decision: decision_id,
                choice: DecisionReply::SelectResolution(vec![land_obj]),
            },
        ),
        "non-creature land must be rejected by SacrificeUpTo(creature)"
    );

    // Positive 1: sacrificing 0 creatures is legal and draws 0 cards.
    let g_zero = reply(&g_pending, P0, DecisionReply::SelectResolution(vec![])).into_state();
    assert_eq!(hand_count(&g_zero, P0), 0);
    assert_eq!(
        g_zero.execution().window().cards().get(b2).unwrap().zone(),
        ObjectZone::Battlefield
    );

    // Positive 2: sacrificing 2 of 3 creatures puts those 2 in graveyard, keeps the 3rd, draws 2.
    let g_two = drain_graveyard_orders(
        reply(
            &g_pending,
            P0,
            DecisionReply::SelectResolution(vec![b2, b3]),
        )
        .into_state(),
    );
    assert_eq!(hand_count(&g_two, P0), 2);
    assert_eq!(
        g_two
            .execution()
            .window()
            .cards()
            .get(object(&g_two, 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        g_two
            .execution()
            .window()
            .cards()
            .get(object(&g_two, 4))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}

#[test]
fn mega_batch_1_void_tracks_nonland_leaving_and_warp_cast() {
    let vocab = vocabulary();
    let plasma_bolt = compile_with_reason(
        &make_card(
            "Plasma Bolt Test",
            "Sorcery",
            "{0}",
            None,
            None,
            "Plasma Bolt Test deals 2 damage to any target.\nVoid \u{2014} If a nonland permanent left the battlefield this turn or a spell was warped this turn, Plasma Bolt Test deals 3 damage instead.",
        ),
        &vocab,
    )
    .expect("Plasma Bolt Void compiles");
    let destroy_perm = free_sorcery("Destroy target permanent.", &vocab);
    let warped_creature = compile_with_reason(
        &make_card(
            "Warp Scout Test",
            "Creature \u{2014} Scout",
            "{2}",
            Some("2"),
            Some("2"),
            "Warp {0}",
        ),
        &vocab,
    )
    .expect("Warp creature compiles");
    let opp_bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);
    let opp_land = land_source();

    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, plasma_bolt),
            hand_card(2, P0, destroy_perm),
            hand_card(3, P0, warped_creature),
            controlled(4, P1, P1, opp_bear),
            controlled(5, P1, P1, opp_land),
            library(10, P0, &vocab),
            library(11, P1, &vocab),
        ],
        vec![CardId(4), CardId(5)],
    );

    // Negative 1: neither nonland left nor warped -> deals 2 damage.
    let g_base = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_base = pass_pair(&g_base).into_state();
    assert_eq!(life(&g_base, P1), 18.into(), "without Void, deals 2 damage");

    // Negative 2: destroying a LAND does NOT satisfy Void -> still deals 2 damage.
    let g_land = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 2),
            target: TargetRef::Object(object(&game, 5)),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_land = pass_pair(&g_land).into_state();
    let g_land_bolt = submit(
        &g_land,
        P0,
        GameCommand::CastPaid {
            spell: object(&g_land, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_land_bolt = pass_pair(&g_land_bolt).into_state();
    assert_eq!(
        life(&g_land_bolt, P1),
        18.into(),
        "land leaving must NOT activate Void"
    );

    // Positive 1: opponent's NONLAND permanent leaves -> activates Void (deals 3 damage!).
    let g_bear = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 2),
            target: TargetRef::Object(object(&game, 4)),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_bear = pass_pair(&g_bear).into_state();
    let g_bear_bolt = submit(
        &g_bear,
        P0,
        GameCommand::CastPaid {
            spell: object(&g_bear, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_bear_bolt = pass_pair(&g_bear_bolt).into_state();
    assert_eq!(
        life(&g_bear_bolt, P1),
        17.into(),
        "opponent's nonland leaving must activate Void (3 damage)"
    );

    // Positive 2: casting a spell via Warp activates Void (deals 3 damage!).
    let warp_pay = CastingPayment {
        use_alternative: true,
        ..CastingPayment::default()
    };
    let g_warp = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 3),
            target: None,
            payment: warp_pay,
        },
    )
    .into_state();
    let g_warp = pass_pair(&g_warp).into_state();
    let g_warp_bolt = submit(
        &g_warp,
        P0,
        GameCommand::CastPaid {
            spell: object(&g_warp, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g_warp_bolt = pass_pair(&g_warp_bolt).into_state();
    assert_eq!(
        life(&g_warp_bolt, P1),
        17.into(),
        "warping a spell this turn must activate Void (3 damage)"
    );
}

#[test]
fn mega_batch_1_has_toxic_and_no_abilities_filters_execute_accurately() {
    let vocab = vocabulary();
    let hexgold = compile_with_reason(
        &make_card(
            "Hexgold Slash Test",
            "Instant",
            "{0}",
            None,
            None,
            "Hexgold Slash Test deals 2 damage to target creature. If that creature has toxic, it deals 4 damage instead.",
        ),
        &vocab,
    )
    .expect("Hexgold Slash compiles");
    let toxic_creature = compile_with_reason(
        &make_card(
            "Mite Test",
            "Creature \u{2014} Phyrexian Mite",
            "{W}",
            Some("1"),
            Some("4"),
            "Toxic 1",
        ),
        &vocab,
    )
    .expect("Toxic creature compiles");
    let vanilla_creature = permanent(&vocab, &[CardType::Creature], Some((1, 4)), &["Bear"]);
    let flyer = compile_with_reason(
        &make_card(
            "Bird Test",
            "Creature \u{2014} Bird",
            "{W}",
            Some("1"),
            Some("4"),
            "Flying",
        ),
        &vocab,
    )
    .expect("Flying creature compiles");

    let game = world(
        vocab.clone(),
        vec![
            hand_card(1, P0, hexgold.clone()),
            hand_card(2, P0, hexgold),
            controlled(3, P1, P1, toxic_creature),
            controlled(4, P1, P1, vanilla_creature),
            controlled(5, P1, P1, flyer),
        ],
        vec![CardId(3), CardId(4), CardId(5)],
    );

    let toxic_id = object(&game, 3);
    let vanilla_id = object(&game, 4);
    let flyer_id = object(&game, 5);
    assert!(!current(&game, toxic_id).has_no_abilities());
    assert!(current(&game, vanilla_id).has_no_abilities());
    assert!(!current(&game, flyer_id).has_no_abilities());

    // Positive: Hexgold Slash deals 4 lethal damage to the 1/4 Toxic creature.
    let g1 = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Object(toxic_id),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g1 = pass_pair(&g1).into_state();
    assert_eq!(
        g1.execution()
            .window()
            .cards()
            .get(object(&g1, 3))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1),
        "1/4 toxic creature must take 4 damage and die"
    );

    // Negative: Hexgold Slash deals only 2 damage to the 1/4 vanilla creature, so it survives.
    let g2 = submit(
        &g1,
        P0,
        GameCommand::CastPaid {
            spell: object(&g1, 2),
            target: TargetRef::Object(vanilla_id),
            payment: ManaPayment::default(),
        },
    )
    .into_state();
    let g2 = pass_pair(&g2).into_state();
    assert_eq!(
        g2.execution()
            .window()
            .cards()
            .get(vanilla_id)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield,
        "1/4 non-toxic creature must take only 2 damage and survive"
    );
}

#[test]
fn mega_batch_1_pinned_real_cards_compile_cleanly() {
    let (inventory, vocabulary, _) = crate::keyword_creatures::tests::current_catalogue();
    let pinned = [
        "Polymorph",
        "Marina Vendrell's Grimoire",
        "Eldritch Evolution",
        "Fang-Druid Summoner",
        "Balustrade Spy",
        "Decode Transmissions",
        "Porcelain Zealot",
        "Hexgold Slash",
        "Hylderblade",
        "Spinner of Souls",
        "Insatiable Skittermaw",
        "Mind Grind",
        "Consuming Aberration",
        "Plasma Bolt",
        "Compleat Devotion",
        "Undercity Informer",
        "Needle Specter",
        "Shortcut to Mushrooms",
        "Lulu, Loyal Hollyphant",
        "Fork in the Road",
        "Final Parting",
        "Avenging Druid",
        "Old Stickfingers",
        "Mind Funeral",
        "Deadly Alliance",
        "Wandering Wolf",
        "Crashing Tide",
    ];
    for name in pinned {
        let card = inventory
            .cards
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("pinned card missing from catalog: {name}"));
        let result = compile_with_inventory_reason(card, inventory, &vocabulary);
        assert!(
            result.is_ok(),
            "{name} failed to compile: {:?}",
            result.err()
        );
    }
}
