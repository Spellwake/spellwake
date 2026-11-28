//! Live `GameState` execution tests for Stage 4.0 `v2_lowering` waves.
//!
//! Each test compiles card rules strictly through `v2 -> v2_lowering -> Program`,
//! seeds a live `spellwake_engine::GameState`, executes casts/activations/triggers/decisions,
//! and asserts actual board, zone, life, mana, or stat outcomes.

use std::collections::BTreeMap;
use std::sync::Arc;

use spellwake_card_programs::v2_lowering::compile_v2_with_reason;
use spellwake_content::{CatalogFace, InventoryCard};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_contracts::names::NameRules;
use spellwake_engine::{
    combat::CombatRules,
    damage::DamageSourceId,
    effects::{
        CopyTargetChoice, DecisionReply, LibraryTopReply, OptionalEffectChoice,
        ResolutionPaymentReply, SeedCard,
    },
    game::{GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate, RuleFrame},
    mana::{CastResource, CastingPayment, FloatingMana, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectZone},
    priority::{PriorityPhase, SeatId},
    program::Program,
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    toolbox::PromptChoiceReply,
    turn::{Step, TurnStart},
};

const ALICE: SeatId = SeatId(0);
const BOB: SeatId = SeatId(1);

fn test_vocab() -> Arc<CreatureTypeVocabulary> {
    let mut members = vec![
        "Assassin".to_string(),
        "Bear".to_string(),
        "Beast".to_string(),
        "Bird".to_string(),
        "Druid".to_string(),
        "Elf".to_string(),
        "Goblin".to_string(),
        "Human".to_string(),
        "Knight".to_string(),
        "Mercenary".to_string(),
        "Pirate".to_string(),
        "Rogue".to_string(),
        "Shapeshifter".to_string(),
        "Soldier".to_string(),
        "Spirit".to_string(),
        "Warlock".to_string(),
        "Warrior".to_string(),
        "Weird".to_string(),
        "Werewolf".to_string(),
        "Wizard".to_string(),
        "Zombie".to_string(),
    ];
    members.sort();
    members.dedup();
    Arc::new(
        CreatureTypeVocabulary::new("v2-lowering-gameplay-vocab".to_string(), [1u8; 32], members)
            .expect("test vocab"),
    )
}

fn compile_card(
    vocab: &CreatureTypeVocabulary,
    name: &str,
    type_line: &str,
    mana_cost: Option<&str>,
    pt: Option<(&str, &str)>,
    oracle_text: &str,
) -> Program {
    let card = InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".to_string(),
        oracle_id: Some("00000000-0000-0000-0000-000000000001".to_string()),
        name: name.to_string(),
        layout: "normal".to_string(),
        faces: vec![CatalogFace {
            name: name.to_string(),
            type_line: Some(type_line.to_string()),
            oracle_text: Some(oracle_text.to_string()),
            mana_cost: mana_cost.map(str::to_string),
            power: pt.map(|(p, _)| p.to_string()),
            toughness: pt.map(|(_, t)| t.to_string()),
        }],
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: true,
        program_evidence: "v2-gameplay-test",
    };
    let program = compile_v2_with_reason(&card, vocab)
        .unwrap_or_else(|e| panic!("failed to compile {name}: {e:?}"));
    program
        .validate(vocab)
        .unwrap_or_else(|e| panic!("failed to validate {name}: {e:?}"));
    program
}

fn start_game(
    vocab: Arc<CreatureTypeVocabulary>,
    cards: Vec<SeedCard>,
    mana: &[(SeatId, ManaType, i64)],
) -> GameState {
    start_game_with_padding(vocab, cards, mana, 8)
}

fn start_game_with_padding(
    vocab: Arc<CreatureTypeVocabulary>,
    mut cards: Vec<SeedCard>,
    mana: &[(SeatId, ManaType, i64)],
    library_padding: u64,
) -> GameState {
    for i in 0..library_padding {
        cards.push(SeedCard {
            card: CardId(1000 + i),
            owner: ALICE,
            destination: ObjectDestination::LibraryBottom,
            program: Program::artifact(None, vec![]),
        });
        cards.push(SeedCard {
            card: CardId(2000 + i),
            owner: BOB,
            destination: ObjectDestination::LibraryBottom,
            program: Program::artifact(None, vec![]),
        });
    }
    let battlefield_order = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let names = Arc::new(NameRules::literal_names());
    let context = GameContext::new(
        "v2-lowering-gameplay".into(),
        vocab.rules_revision().into(),
        "0000000000000000000000000000000000000000000000000000000000000000".into(),
        "v2-lowering-gameplay-v1".into(),
        "v2-lowering-gameplay".into(),
        vocab.id(),
        names.id(),
    )
    .expect("context");
    GameState::initialize_scheduled(
        GameId(1),
        context,
        GameSeed::with_uniform_life(
            vec![ALICE, BOB],
            ALICE,
            cards,
            battlefield_order,
            mana.iter()
                .map(|&(player, mana_type, amount)| FloatingMana {
                    player,
                    mana_type,
                    amount: amount.into(),
                })
                .collect(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        ),
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .expect("init game")
    .into_state()
}

fn object(game: &GameState, card: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(card)).expect("card id"))
        .expect("object")
}

fn advance(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command: command.clone(),
    })
    .unwrap_or_else(|e| panic!("command {command:?} failed: {e:?}"))
}

fn pass(game: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!(
            "expected priority, got {:?}, pending={:?}",
            game.execution().window().phase(),
            game.pending_decision()
        );
    };
    advance(game, holder, GameCommand::Pass).into_state()
}

fn resolve_top(game: &GameState) -> GameState {
    let g = pass(game);
    pass(&g)
}

fn reply(game: &GameState, choice: DecisionReply) -> GameState {
    let pending = game.pending_decision().expect("expected pending decision");
    advance(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
    .into_state()
}

fn pay_mana(items: &[(ManaType, i64)]) -> Option<ManaPayment> {
    Some(ManaPayment {
        from_lots: Default::default(),
        spend: items
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    })
}

fn life(game: &GameState, seat: SeatId) -> spellwake_contracts::RulesInteger {
    game.execution()
        .life_total(seat)
        .expect("life total")
        .clone()
}

fn first_activated_id(
    game: &GameState,
    card: u64,
) -> spellwake_engine::activated::ActivatedAbilityId {
    game.execution()
        .programs()
        .find(|&(c, _)| c == CardId(card))
        .expect("program")
        .1
        .activated
        .first()
        .expect("activated ability")
        .id
}

fn hand_count(game: &GameState, seat: SeatId) -> usize {
    game.execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Hand(seat))
        .count()
}

#[test]
fn wave1_self_antecedent_activated_sacrifice_deals_damage() {
    let vocab = test_vocab();
    let aeolipile = compile_card(
        &vocab,
        "Aeolipile",
        "Artifact",
        Some("{2}"),
        None,
        "{1}, {T}, Sacrifice this artifact: It deals 2 damage to any target.",
    );
    let game = start_game(
        Arc::clone(&vocab),
        vec![SeedCard {
            card: CardId(1),
            owner: ALICE,
            destination: ObjectDestination::Battlefield { controller: ALICE },
            program: aeolipile,
        }],
        &[(ALICE, ManaType::Colourless, 1)],
    );

    let act = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&game, 1).id(),
            ability: first_activated_id(&game, 1),
            target: TargetRef::Player(BOB),
            payment: pay_mana(&[(ManaType::Colourless, 1)]),
        },
    )
    .into_state();
    assert_eq!(object(&act, 1).zone(), ObjectZone::Graveyard(ALICE));

    let after = resolve_top(&act);
    assert_eq!(life(&after, BOB), 18.into());
}

#[test]
fn wave1_forward_target_branch_and_mixed_compound_subject() {
    let vocab = test_vocab();
    let branch_pump = compile_card(
        &vocab,
        "Goblin Boost",
        "Instant",
        Some("{R}"),
        None,
        "If you control a Goblin, target creature gets +3/+3 until end of turn. Otherwise, target creature gets +1/+1 until end of turn.",
    );
    let goblin = compile_card(
        &vocab,
        "Raging Goblin",
        "Creature \u{2014} Goblin",
        Some("{R}"),
        Some(("1", "1")),
        "Haste",
    );
    let compound_exile = compile_card(
        &vocab,
        "Mutual Banish",
        "Artifact",
        Some("{1}"),
        None,
        "{T}: Exile this artifact and target creature.",
    );
    let bear = compile_card(
        &vocab,
        "Grizzly Bears",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );

    let game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: branch_pump,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: goblin,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: compound_exile,
            },
            SeedCard {
                card: CardId(4),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
        ],
        &[(ALICE, ManaType::Red, 1)],
    );

    let cast = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: Some(TargetRef::Object(object(&game, 4).id())),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    let after_pump = resolve_top(&cast);
    let bear_pt = after_pump
        .execution()
        .current_characteristics(object(&after_pump, 4).id())
        .expect("chars")
        .expect("present")
        .power_toughness
        .expect("pt");
    assert_eq!(bear_pt.derived.power, 5.into());
    assert_eq!(bear_pt.derived.toughness, 5.into());

    let act = advance(
        &after_pump,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&after_pump, 3).id(),
            ability: first_activated_id(&after_pump, 3),
            target: TargetRef::Object(object(&after_pump, 4).id()),
            payment: None,
        },
    )
    .into_state();
    let after_exile = resolve_top(&act);
    assert_eq!(object(&after_exile, 3).zone(), ObjectZone::Exile);
    assert_eq!(object(&after_exile, 4).zone(), ObjectZone::Exile);
}

#[test]
fn wave1_delayed_end_step_tagged_anaphora_destroys_target() {
    let vocab = test_vocab();
    let doomed_pump = compile_card(
        &vocab,
        "Reckless Surge",
        "Instant",
        Some("{R}"),
        None,
        "Target creature gets +2/+0 until end of turn. Destroy it at the beginning of the next end step.",
    );
    let bear = compile_card(
        &vocab,
        "Grizzly Bears",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );
    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: doomed_pump,
            },
            SeedCard {
                card: CardId(2),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
        ],
        &[(ALICE, ManaType::Red, 1)],
    );

    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: Some(TargetRef::Object(object(&game, 2).id())),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(object(&game, 2).zone(), ObjectZone::Battlefield);
    let pt = game
        .execution()
        .current_characteristics(object(&game, 2).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    assert_eq!(pt.derived.power, 4.into());

    for _ in 0..30 {
        if object(&game, 2).zone() == ObjectZone::Graveyard(BOB) {
            break;
        }
        if let Some(pending) = game.pending_decision() {
            match pending.frame() {
                RuleFrame::Combat(_) => {
                    game = reply(&game, DecisionReply::DeclareAttackers(vec![]));
                }
                other => panic!("unexpected decision while advancing to end step: {other:?}"),
            }
        } else {
            game = pass(&game);
        }
    }
    assert_eq!(
        object(&game, 2).zone(),
        ObjectZone::Graveyard(BOB),
        "delayed end-step trigger should destroy the tagged creature"
    );
}

#[test]
fn wave2_unless_effect_and_choose_branch_target_hoisting_and_sacrifice_cast_cost() {
    let vocab = test_vocab();
    let tax_burn = compile_card(
        &vocab,
        "Taxing Bolt",
        "Sorcery",
        Some("{B}"),
        None,
        "Target opponent loses 4 life unless they pay {2}.",
    );
    let twiddle_tap = compile_card(
        &vocab,
        "Frost Twiddler",
        "Creature \u{2014} Human Wizard",
        Some("{U}"),
        Some(("1", "1")),
        "{U}: Tap or untap target creature.",
    );
    let sac_draw = compile_card(
        &vocab,
        "Blood Harvest",
        "Sorcery",
        Some("{1}{B}"),
        None,
        "As an additional cost to cast this spell, sacrifice a creature.\nDraw cards equal to the sacrificed creature's power.",
    );
    let fodder = compile_card(
        &vocab,
        "Big Beast",
        "Creature \u{2014} Beast",
        Some("{2}{G}"),
        Some(("3", "3")),
        "",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: tax_burn,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: twiddle_tap,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: sac_draw,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: fodder,
            },
        ],
        &[
            (ALICE, ManaType::Black, 2),
            (ALICE, ManaType::Blue, 1),
            (ALICE, ManaType::Colourless, 1),
        ],
    );

    // 1. UnlessEffect: Bob declines payment -> loses 4 life (20 -> 16).
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: Some(TargetRef::Player(BOB)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision() {
        assert_eq!(pending.chooser(), BOB);
        game = reply(
            &game,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
    }
    assert_eq!(life(&game, BOB), 16.into());

    // 2. ChooseBranch with hoisted target: tap Big Beast via option 0.
    assert_eq!(object(&game, 4).tapped(), Some(false));
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&game, 2).id(),
            ability: first_activated_id(&game, 2),
            target: TargetRef::Object(object(&game, 4).id()),
            payment: pay_mana(&[(ManaType::Blue, 1)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision() {
        match pending.frame() {
            RuleFrame::PromptChoice(_) => {
                game = reply(
                    &game,
                    DecisionReply::PromptChoice(PromptChoiceReply::Options(vec![0])),
                );
            }
            RuleFrame::OptionIndex(_) => {
                game = reply(&game, DecisionReply::OptionIndex(0));
            }
            other => panic!("unexpected twiddle decision: {other:?}"),
        }
    }
    assert_eq!(
        object(&game, 4).tapped(),
        Some(true),
        "branch 0 should tap Big Beast via hoisted shared target"
    );

    // 3. Spell with additional sacrifice cost: sacrifice 3/3 Big Beast -> draw 3 cards.
    let hand_before = hand_count(&game, ALICE);
    let fodder_id = object(&game, 4).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1), (ManaType::Colourless, 1)]),
                objects: vec![CastResource::SacrificeForSpell(fodder_id)],
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(object(&game, 4).zone(), ObjectZone::Graveyard(ALICE));
    game = resolve_top(&game);
    let hand_after = hand_count(&game, ALICE);
    assert_eq!(hand_after, hand_before + 2);
}

#[test]
fn wave2_redirect_next_damage_and_spell_cast_copy_retarget() {
    let vocab = test_vocab();
    let en_kor = compile_card(
        &vocab,
        "Shield Bearer",
        "Creature \u{2014} Human Knight",
        Some("{W}{W}"),
        Some(("2", "2")),
        "{0}: The next 1 damage that would be dealt to this creature this turn is dealt to target creature you control instead.",
    );
    let ally = compile_card(
        &vocab,
        "Tough Ally",
        "Creature \u{2014} Human Soldier",
        Some("{1}{W}"),
        Some(("1", "4")),
        "",
    );
    let ping = compile_card(
        &vocab,
        "Spark Ping",
        "Instant",
        Some("{R}"),
        None,
        "Spark Ping deals 2 damage to any target.",
    );
    let copier = compile_card(
        &vocab,
        "Spell Echo",
        "Creature \u{2014} Human Wizard",
        Some("{2}{U}"),
        Some(("2", "2")),
        "Whenever you cast an instant or sorcery spell, you may copy it. You may choose new targets for the copy.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: en_kor,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: ally,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: ping,
            },
        ],
        &[(ALICE, ManaType::Red, 1)],
    );

    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&game, 1).id(),
            ability: first_activated_id(&game, 1),
            target: TargetRef::Object(object(&game, 2).id()),
            payment: None,
        },
    )
    .into_state();
    game = resolve_top(&game);

    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: Some(TargetRef::Object(object(&game, 1).id())),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(object(&game, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(
        game.execution()
            .battlefield_marked_damage(object(&game, 1).id()),
        Some(1.into())
    );
    assert_eq!(
        game.execution()
            .battlefield_marked_damage(object(&game, 2).id()),
        Some(1.into())
    );

    // SpellCast copy + retarget fusion: casting a 2-damage spell at Bob deals 4 total.
    let ping2 = compile_card(
        &vocab,
        "Spark Ping",
        "Instant",
        Some("{R}"),
        None,
        "Spark Ping deals 2 damage to any target.",
    );
    let mut game2 = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: copier,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: ping2,
            },
        ],
        &[(ALICE, ManaType::Red, 1)],
    );
    game2 = advance(
        &game2,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game2, 2).id(),
            target: Some(TargetRef::Player(BOB)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    for _ in 0..20 {
        if let Some(pending) = game2.pending_decision() {
            match pending.frame() {
                RuleFrame::OptionalEffect(_) => {
                    game2 = reply(
                        &game2,
                        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                    );
                }
                RuleFrame::CopyTargets(_) => {
                    game2 = reply(&game2, DecisionReply::CopyTargets(CopyTargetChoice::Retain));
                }
                other => panic!("unexpected copy decision: {other:?}"),
            }
        } else if game2.execution().window().entries().is_empty() {
            break;
        } else {
            game2 = pass(&game2);
        }
    }
    assert_eq!(
        life(&game2, BOB),
        16.into(),
        "original 2 damage + copied 2 damage should leave Bob at 16 life"
    );
}

#[test]
fn wave3_fabled_passage_search_last_moved_untaps_land() {
    let vocab = test_vocab();
    let fabled_passage = compile_card(
        &vocab,
        "Fabled Passage",
        "Land",
        None,
        None,
        "{T}, Sacrifice this land: Search your library for a basic land card, put it onto the battlefield tapped, then shuffle. Then if you control four or more lands, untap that land.",
    );
    let plains = compile_card(
        &vocab,
        "Plains",
        "Basic Land \u{2014} Plains",
        None,
        None,
        "",
    );
    let mountain = compile_card(
        &vocab,
        "Mountain",
        "Basic Land \u{2014} Mountain",
        None,
        None,
        "",
    );

    let mut game = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: fabled_passage,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: plains.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: plains.clone(),
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: plains,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: mountain,
            },
        ],
        &[],
        1,
    );

    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 1).id(),
            ability: first_activated_id(&game, 1),
            payment: None,
        },
    )
    .into_state();
    assert_eq!(object(&game, 1).zone(), ObjectZone::Graveyard(ALICE));

    game = resolve_top(&game);
    let mountain_lib_id = object(&game, 5).id();
    let pending = game.pending_decision().expect("expected library search");
    assert!(matches!(pending.frame(), RuleFrame::LibraryTop(_)));
    game = reply(
        &game,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(mountain_lib_id),
        }),
    );

    assert_eq!(
        object(&game, 5).zone(),
        ObjectZone::Battlefield,
        "searched basic land should enter battlefield"
    );
    assert_eq!(
        object(&game, 5).tapped(),
        Some(false),
        "with 4 lands on battlefield, ObjectQuery::LastMoved should untap the fetched land"
    );
}

#[test]
fn wave3_converge_player_hexproof_adapt_and_support() {
    let vocab = test_vocab();
    let converge_draw = compile_card(
        &vocab,
        "Painful Truths",
        "Sorcery",
        Some("{2}{B}"),
        None,
        "Converge \u{2014} You draw X cards and you lose X life, where X is the number of colors of mana spent to cast this spell.",
    );
    let hexproof_spell = compile_card(
        &vocab,
        "Aegis Ward",
        "Instant",
        Some("{W}"),
        None,
        "You gain 2 life. You gain hexproof until end of turn.",
    );
    let adapter = compile_card(
        &vocab,
        "Simic Ooze",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "{1}{G}: Adapt 2.",
    );
    let supporter = compile_card(
        &vocab,
        "Rally Support",
        "Sorcery",
        Some("{1}{W}"),
        None,
        "Support 2.",
    );
    let ally = compile_card(
        &vocab,
        "Field Ally",
        "Creature \u{2014} Soldier",
        Some("{W}"),
        Some(("1", "1")),
        "",
    );
    let bolt = compile_card(
        &vocab,
        "Spark Ping",
        "Instant",
        Some("{R}"),
        None,
        "Spark Ping deals 2 damage to any target.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: converge_draw,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: hexproof_spell,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: adapter,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: supporter,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: ally,
            },
            SeedCard {
                card: CardId(6),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: bolt,
            },
        ],
        &[
            (ALICE, ManaType::White, 3),
            (ALICE, ManaType::Blue, 1),
            (ALICE, ManaType::Black, 1),
            (ALICE, ManaType::Green, 2),
            (ALICE, ManaType::Colourless, 3),
            (BOB, ManaType::Red, 1),
        ],
    );

    // 1. Converge with 3 colors ({W}{U}{B}): draw 3 cards, lose 3 life (20 -> 17).
    let hand_before = hand_count(&game, ALICE);
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[
                    (ManaType::White, 1),
                    (ManaType::Blue, 1),
                    (ManaType::Black, 1),
                ]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(life(&game, ALICE), 17.into());
    assert_eq!(hand_count(&game, ALICE), hand_before + 2);

    // 2. PlayerGrantKeyword (hexproof): Alice gains 2 life (17 -> 19) and Bob cannot target Alice.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(life(&game, ALICE), 19.into());
    let bob_prio = pass(&game);
    let illegal_bolt = bob_prio.advance(GameInput {
        basis: bob_prio.basis().clone(),
        actor: BOB,
        command: GameCommand::CastWithResources {
            spell: object(&bob_prio, 6).id(),
            target: Some(TargetRef::Player(ALICE)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    });
    assert!(
        illegal_bolt.is_err(),
        "Bob must not be able to target Alice while Alice has hexproof"
    );

    // 3. Adapt 2: first activation adds two +1/+1 counters (2/2 -> 4/4); second activation adds 0.
    for _ in 0..2 {
        game = advance(
            &game,
            ALICE,
            GameCommand::ActivateUntargetedAbility {
                source: object(&game, 3).id(),
                ability: first_activated_id(&game, 3),
                payment: pay_mana(&[(ManaType::Green, 1), (ManaType::Colourless, 1)]),
            },
        )
        .into_state();
        game = resolve_top(&game);
    }
    let ooze_pt = game
        .execution()
        .current_characteristics(object(&game, 3).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    assert_eq!(ooze_pt.derived.power, 4.into());
    assert_eq!(ooze_pt.derived.toughness, 4.into());

    // 4. Support 2: puts one +1/+1 counter on each of two target creatures (4/4 -> 5/5, 1/1 -> 2/2).
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1), (ManaType::Colourless, 1)]),
                permanent_targets: vec![
                    TargetRef::Object(object(&game, 3).id()),
                    TargetRef::Object(object(&game, 5).id()),
                ],
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let ooze_pt2 = game
        .execution()
        .current_characteristics(object(&game, 3).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    let ally_pt = game
        .execution()
        .current_characteristics(object(&game, 5).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    assert_eq!(ooze_pt2.derived.power, 5.into());
    assert_eq!(ally_pt.derived.power, 2.into());
}

#[test]
fn wave3_conditional_alt_cost_and_chosen_source_damage_prevention() {
    let vocab = test_vocab();
    let snuff_out = compile_card(
        &vocab,
        "Snuff Out",
        "Instant",
        Some("{3}{B}"),
        None,
        "If you control a Swamp, you may pay 4 life rather than pay this spell's mana cost.\nDestroy target nonblack creature. It can't be regenerated.",
    );
    let swamp = compile_card(&vocab, "Swamp", "Basic Land \u{2014} Swamp", None, None, "");
    let bear = compile_card(
        &vocab,
        "Grizzly Bears",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );
    let pentagram = compile_card(
        &vocab,
        "Pentagram of the Ages",
        "Artifact",
        Some("{4}"),
        None,
        "{4}, {T}: Prevent all damage that would be dealt to you this turn by a source of your choice.",
    );
    let pinger = compile_card(
        &vocab,
        "Goblin Pinger",
        "Creature \u{2014} Goblin",
        Some("{R}"),
        Some(("1", "1")),
        "Haste\n{T}: This creature deals 3 damage to any target.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: snuff_out,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: swamp,
            },
            SeedCard {
                card: CardId(3),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: pentagram,
            },
            SeedCard {
                card: CardId(5),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: pinger,
            },
        ],
        &[(ALICE, ManaType::Colourless, 4)],
    );

    // 1. Cast Snuff Out for 0 mana + 4 life via conditional alternative cost.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: Some(TargetRef::Object(object(&game, 3).id())),
            payment: CastingPayment {
                use_alternative: true,
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(life(&game, ALICE), 16.into());
    game = resolve_top(&game);
    assert_eq!(object(&game, 3).zone(), ObjectZone::Graveyard(BOB));

    // 2. Activate Pentagram of the Ages, choosing Bob's Goblin Pinger as the prevented source.
    let pinger_id = object(&game, 5).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 4).id(),
            ability: first_activated_id(&game, 4),
            payment: pay_mana(&[(ManaType::Colourless, 4)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game
        .pending_decision()
        .expect("expected damage source choice");
    assert!(matches!(pending.frame(), RuleFrame::DamageSourceChoice(_)));
    game = reply(
        &game,
        DecisionReply::ChooseDamageSource(DamageSourceId::Object(pinger_id)),
    );

    // Bob activates Goblin Pinger targeting Alice -> all 3 damage prevented (Alice stays at 16).
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::ActivateAbility {
            source: object(&game, 5).id(),
            ability: first_activated_id(&game, 5),
            target: TargetRef::Player(ALICE),
            payment: None,
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        16.into(),
        "chosen-source turn prevention should prevent all 3 damage from Goblin Pinger"
    );
}

#[test]
fn wave4_they_gain_keywords_and_pillar_exile_if_dies_and_scent_reveal() {
    let vocab = test_vocab();
    let haste_tokens = compile_card(
        &vocab,
        "Hasty Reinforcements",
        "Sorcery",
        Some("{1}{R}"),
        None,
        "Create two 1/1 white Soldier creature tokens. They gain haste until end of turn.",
    );
    let pillar = compile_card(
        &vocab,
        "Pillar of Flame",
        "Sorcery",
        Some("{R}"),
        None,
        "Pillar of Flame deals 2 damage to any target. If a creature dealt damage this way would die this turn, exile it instead.",
    );
    let scent = compile_card(
        &vocab,
        "Scent of Jasmine",
        "Instant",
        Some("{W}"),
        None,
        "Reveal any number of white cards in your hand. You gain 2 life for each card revealed this way.",
    );
    let white_card_a = compile_card(
        &vocab,
        "Savannah Lions",
        "Creature \u{2014} Soldier",
        Some("{W}"),
        Some(("2", "1")),
        "",
    );
    let white_card_b = compile_card(
        &vocab,
        "Squire",
        "Creature \u{2014} Human Soldier",
        Some("{1}{W}"),
        Some(("1", "2")),
        "",
    );
    let bear = compile_card(
        &vocab,
        "Grizzly Bears",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: haste_tokens,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: pillar,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: scent,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: white_card_a,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: white_card_b,
            },
            SeedCard {
                card: CardId(6),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
        ],
        &[
            (ALICE, ManaType::Red, 2),
            (ALICE, ManaType::White, 1),
            (ALICE, ManaType::Colourless, 1),
        ],
    );

    // 1. Create two tokens with "They gain haste until end of turn" -> both tokens have Haste.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision()
        && let RuleFrame::TimestampOrder(order) = pending.frame()
    {
        let members = order.members().iter().map(|m| m.member).collect();
        game = reply(&game, DecisionReply::TimestampOrder(members));
    }
    let tokens: Vec<_> = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield && o.is_token())
        .map(|o| o.id())
        .collect();
    assert_eq!(tokens.len(), 2, "should have created two Soldier tokens");
    for tok_id in tokens {
        let chars = game
            .execution()
            .current_characteristics(tok_id)
            .unwrap()
            .unwrap();
        assert!(
            chars
                .keywords
                .contains(&spellwake_engine::characteristics::KeywordAbility::Haste),
            "tokens created before 'They gain haste' should gain Haste"
        );
    }

    // 2. Pillar of Flame deals 2 damage to Bob's 2/2 Bear -> exiled instead of graveyard!
    let bear_id = object(&game, 6).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: Some(TargetRef::Object(bear_id)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 6).zone(),
        ObjectZone::Exile,
        "Pillar of Flame should exile a creature dealt lethal damage this way"
    );

    // 3. Scent of Jasmine reveals 2 white cards from Alice's hand -> Alice gains 4 life (20 -> 24).
    let w1 = object(&game, 4).id();
    let w2 = object(&game, 5).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game
        .pending_decision()
        .expect("expected prompt to choose white cards to reveal");
    assert!(matches!(pending.frame(), RuleFrame::PromptChoice(_)));
    game = reply(
        &game,
        DecisionReply::PromptChoice(PromptChoiceReply::Objects(vec![w1, w2])),
    );
    assert_eq!(
        life(&game, ALICE),
        24.into(),
        "revealing 2 white cards with Scent of Jasmine should gain 4 life"
    );
}

#[test]
fn wave4_kodamas_reach_split_search_and_scaled_energy_cost_and_restricted_mana() {
    let vocab = test_vocab();
    let kodama = compile_card(
        &vocab,
        "Kodama's Reach",
        "Sorcery \u{2014} Arcane",
        Some("{2}{G}"),
        None,
        "Search your library for up to two basic land cards, reveal those cards, put one onto the battlefield tapped and the other into your hand, then shuffle.",
    );
    let forest = compile_card(
        &vocab,
        "Forest",
        "Basic Land \u{2014} Forest",
        None,
        None,
        "",
    );
    let plains = compile_card(
        &vocab,
        "Plains",
        "Basic Land \u{2014} Plains",
        None,
        None,
        "",
    );
    let energy_spell = compile_card(
        &vocab,
        "Aether Surge",
        "Sorcery",
        Some("{U}"),
        None,
        "You get {E}{E}{E}{E}{E}{E}.",
    );
    let energy_engine = compile_card(
        &vocab,
        "Aether Reservoir",
        "Artifact",
        Some("{3}"),
        None,
        "{T}, Pay six {E}: Draw a card.",
    );
    let untaidake = compile_card(
        &vocab,
        "Untaidake, the Cloud Keeper",
        "Legendary Land",
        None,
        None,
        "Untaidake enters tapped.\n{T}, Pay 2 life: Add {C}{C}. Spend this mana only to cast legendary spells.",
    );

    let mut game = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: kodama,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: forest,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: plains,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: energy_spell,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: energy_engine,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: untaidake,
            },
        ],
        &[
            (ALICE, ManaType::Green, 1),
            (ALICE, ManaType::Blue, 1),
            (ALICE, ManaType::Colourless, 2),
        ],
        1,
    );

    // 1. Kodama's Reach searches 2 basic lands: Forest to battlefield tapped, Plains to hand.
    let forest_id = object(&game, 2).id();
    let plains_id = object(&game, 3).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1), (ManaType::Colourless, 2)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game.pending_decision().expect("expected library search");
    assert!(matches!(pending.frame(), RuleFrame::LibraryTop(_)));
    game = reply(
        &game,
        DecisionReply::LibraryTop(LibraryTopReply::SearchSplit {
            battlefield: Some(forest_id),
            hand: Some(plains_id),
        }),
    );
    assert_eq!(
        object(&game, 2).zone(),
        ObjectZone::Battlefield,
        "Forest should enter the battlefield"
    );
    assert_eq!(
        object(&game, 2).tapped(),
        Some(true),
        "Forest should enter tapped from Kodama's Reach"
    );
    assert_eq!(
        object(&game, 3).zone(),
        ObjectZone::Hand(ALICE),
        "Plains should go to hand from Kodama's Reach"
    );

    // 2. Gain 6 energy then activate `{T}, Pay six {E}: Draw two cards`.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let hand_before_draw = hand_count(&game, ALICE);
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 5).id(),
            ability: first_activated_id(&game, 5),
            payment: None,
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert!(
        hand_count(&game, ALICE) > hand_before_draw,
        "Pay six {{E}} activated ability should draw cards"
    );

    // 3. Activate Untaidake (`{T}, Pay 2 life: Add {C}{C}` restricted to legendary spells).
    let life_before = life(&game, ALICE);
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 6).id(),
            ability: first_activated_id(&game, 6),
            payment: None,
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        life_before.minus(&2.into()),
        "Untaidake should pay 2 life on activation"
    );
    assert!(
        !game.execution().restricted_mana().is_empty(),
        "Untaidake should add restricted colorless mana for legendary spells"
    );
}

#[test]
fn wave5_spell_targeting_object_repartee_and_intervene() {
    let vocab = test_vocab();
    let scornmage = compile_card(
        &vocab,
        "Lecturing Scornmage",
        "Creature \u{2014} Human Wizard",
        Some("{1}{B}"),
        Some(("2", "2")),
        "Whenever you cast an instant or sorcery spell that targets a creature, put a +1/+1 counter on this creature.",
    );
    let bolt = compile_card(
        &vocab,
        "Lightning Bolt",
        "Instant",
        Some("{R}"),
        None,
        "Lightning Bolt deals 3 damage to any target.",
    );
    let intervene = compile_card(
        &vocab,
        "Intervene",
        "Instant",
        Some("{U}"),
        None,
        "Counter target spell that targets a creature.",
    );
    let bear = compile_card(
        &vocab,
        "Sturdy Beast",
        "Creature \u{2014} Beast",
        Some("{2}{G}"),
        Some(("3", "3")),
        "",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: scornmage,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: bolt.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: bolt,
            },
            SeedCard {
                card: CardId(4),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: intervene,
            },
            SeedCard {
                card: CardId(5),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
        ],
        &[(ALICE, ManaType::Red, 2), (BOB, ManaType::Blue, 1)],
    );

    // 1. Cast Lightning Bolt targeting Bob (a player):
    //    - Repartee must NOT trigger (Scornmage stays 2/2).
    //    - Intervene must NOT be able to target Bolt on the stack.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: Some(TargetRef::Player(BOB)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(
        game.execution().window().entries().len(),
        1,
        "spell targeting a player must not trigger Repartee"
    );
    let bolt1_entry = game.execution().window().entries()[0].id();
    let bob_prio = pass(&game);
    let illegal_intervene = bob_prio.advance(GameInput {
        basis: bob_prio.basis().clone(),
        actor: BOB,
        command: GameCommand::CastWithResources {
            spell: object(&bob_prio, 4).id(),
            target: Some(TargetRef::Stack(bolt1_entry)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    });
    assert!(
        illegal_intervene.is_err(),
        "Intervene must reject a spell that only targets a player"
    );
    game = resolve_top(&game);
    assert_eq!(life(&game, BOB), 17.into());
    let scornmage_pt = game
        .execution()
        .current_characteristics(object(&game, 1).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    assert_eq!(scornmage_pt.derived.power, 2.into());

    // 2. Cast Lightning Bolt targeting Bob's 3/3 Beast:
    //    - Repartee DOES trigger on top of Bolt (stack len == 2).
    //    - Bob casts Intervene targeting Bolt -> Bolt is countered and Beast survives.
    //    - Repartee resolves -> Scornmage grows from 2/2 to 3/3.
    let bear_id = object(&game, 5).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: Some(TargetRef::Object(bear_id)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(
        game.execution().window().entries().len(),
        2,
        "spell targeting a creature must trigger Repartee"
    );
    let bolt2_entry = game.execution().window().entries()[0].id();
    // Pass Alice's priority so Bob can respond with Intervene targeting Bolt.
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: Some(TargetRef::Stack(bolt2_entry)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    // Resolve Intervene (counters Bolt), then Repartee (+1/+1 counter on Scornmage).
    game = resolve_top(&game);
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 3).zone(),
        ObjectZone::Graveyard(ALICE),
        "Intervene should counter Lightning Bolt targeting a creature"
    );
    assert_eq!(
        object(&game, 5).zone(),
        ObjectZone::Battlefield,
        "Bob's Beast should survive after Bolt is countered"
    );
    let scornmage_pt2 = game
        .execution()
        .current_characteristics(object(&game, 1).id())
        .unwrap()
        .unwrap()
        .power_toughness
        .unwrap();
    assert_eq!(
        scornmage_pt2.derived.power,
        3.into(),
        "Repartee should put a +1/+1 counter on Lecturing Scornmage"
    );
}

#[test]
fn wave5_shares_creature_type_trigger_attached_to_source_and_both_directions_prevention() {
    let vocab = test_vocab();
    let mana_echoes = compile_card(
        &vocab,
        "Mana Echoes",
        "Enchantment",
        Some("{2}{R}{R}"),
        None,
        "Whenever a creature enters, you may add an amount of {C} equal to the number of creatures you control that share a creature type with it.",
    );
    let goblin1 = compile_card(
        &vocab,
        "Goblin Scout",
        "Creature \u{2014} Goblin",
        Some("{R}"),
        Some(("1", "1")),
        "",
    );
    let goblin2 = compile_card(
        &vocab,
        "Goblin Recruit",
        "Creature \u{2014} Goblin",
        Some("{R}"),
        Some(("1", "1")),
        "",
    );
    let elf = compile_card(
        &vocab,
        "Forest Elf",
        "Creature \u{2014} Elf",
        Some("{G}"),
        Some(("1", "1")),
        "",
    );
    let arms_master = compile_card(
        &vocab,
        "Arms Master",
        "Creature \u{2014} Human Soldier",
        Some("{2}{W}"),
        Some(("2", "2")),
        "Haste\n{T}: Create a 1/1 white Soldier creature token for each Equipment attached to this creature.",
    );
    let short_sword = compile_card(
        &vocab,
        "Test Blade",
        "Artifact \u{2014} Equipment",
        Some("{1}"),
        None,
        "Equipped creature gets +1/+1.\nEquip {1}",
    );
    let chariot = compile_card(
        &vocab,
        "Soratami Cloud Chariot",
        "Artifact",
        Some("{5}"),
        None,
        "{2}: Target creature you control gains flying until end of turn.\n{2}: Prevent all combat damage that would be dealt to and dealt by target creature you control this turn.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: mana_echoes,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: goblin1,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: elf,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: goblin2,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: arms_master,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: short_sword,
            },
            SeedCard {
                card: CardId(7),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: chariot,
            },
        ],
        &[(ALICE, ManaType::Red, 1), (ALICE, ManaType::Colourless, 3)],
    );

    // 1. Cast second Goblin: Mana Echoes triggers; accepting adds 2 {C} (two Goblins, not the Elf).
    let colorless_before = game.execution().mana(ALICE, ManaType::Colourless);
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    // Resolve Goblin Recruit (triggers Mana Echoes).
    game = resolve_top(&game);
    // Resolve Mana Echoes trigger and accept optional mana addition.
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision() {
        assert!(matches!(pending.frame(), RuleFrame::OptionalEffect(_)));
        game = reply(
            &game,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
    }
    let colorless_after = game.execution().mana(ALICE, ManaType::Colourless);
    assert_eq!(
        colorless_after,
        colorless_before.plus(&2.into()),
        "Mana Echoes should add exactly 2 {{C}} for the 2 Goblins (excluding the Elf)"
    );

    // 2. Equip Test Blade to Arms Master, then activate `{T}: Create a 1/1 Soldier for each Equipment attached to this creature`.
    let bf_before_equip = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield)
        .count();
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&game, 6).id(),
            ability: first_activated_id(&game, 6),
            target: TargetRef::Object(object(&game, 5).id()),
            payment: pay_mana(&[(ManaType::Colourless, 1)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 5).id(),
            ability: first_activated_id(&game, 5),
            payment: None,
        },
    )
    .into_state();
    game = resolve_top(&game);
    let bf_after_equip = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield)
        .count();
    assert_eq!(
        bf_after_equip,
        bf_before_equip + 1,
        "ATTACHED_TO_SOURCE should count the 1 attached Equipment and create 1 Soldier token"
    );
    // Soldier token entering also triggers Mana Echoes; resolve and decline it to clear the stack.
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision() {
        assert!(matches!(pending.frame(), RuleFrame::OptionalEffect(_)));
        game = reply(
            &game,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
    }

    // 3. Activate Soratami Cloud Chariot's second ability (both_directions combat damage prevention).
    let chariot_prev_ability = game
        .execution()
        .programs()
        .find_map(|(cid, prog)| (cid == CardId(7)).then(|| prog.activated[1].id))
        .expect("chariot program");
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: object(&game, 7).id(),
            ability: chariot_prev_ability,
            target: TargetRef::Object(object(&game, 2).id()),
            payment: pay_mana(&[(ManaType::Colourless, 2)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert!(
        game.execution().window().entries().is_empty(),
        "Soratami Cloud Chariot both_directions damage prevention should resolve cleanly"
    );
}

#[test]
fn wave6_sleight_of_hand_wave_goodbye_and_sun_droplet() {
    let vocab = test_vocab();
    let sleight = compile_card(
        &vocab,
        "Sleight of Hand",
        "Sorcery",
        Some("{U}"),
        None,
        "Look at the top two cards of your library. Put one of them into your hand and the other on the bottom of your library.",
    );
    let wave_goodbye = compile_card(
        &vocab,
        "Wave Goodbye",
        "Sorcery",
        Some("{2}{U}{U}"),
        None,
        "Return each creature without a +1/+1 counter on it to its owner's hand.",
    );
    let sun_droplet = compile_card(
        &vocab,
        "Sun Droplet",
        "Artifact",
        Some("{2}"),
        None,
        "Whenever you're dealt damage, put that many charge counters on this artifact.",
    );
    let bear_plain = compile_card(
        &vocab,
        "Plain Beast",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );
    let bear_counter = compile_card(
        &vocab,
        "Countered Beast",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "This creature enters with a +1/+1 counter on it.",
    );
    let bolt = compile_card(
        &vocab,
        "Lightning Bolt",
        "Instant",
        Some("{R}"),
        None,
        "Lightning Bolt deals 3 damage to any target.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: sleight,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: bear_plain.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: bear_plain.clone(),
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: wave_goodbye,
            },
            SeedCard {
                card: CardId(5),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear_plain,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: bear_counter,
            },
            SeedCard {
                card: CardId(7),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: sun_droplet,
            },
            SeedCard {
                card: CardId(8),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: bolt,
            },
        ],
        &[
            (ALICE, ManaType::Blue, 3),
            (ALICE, ManaType::Green, 1),
            (ALICE, ManaType::Colourless, 3),
            (BOB, ManaType::Red, 1),
        ],
    );

    // 1. Sleight of Hand: look at top 2 (CardId(3), CardId(2)), choose CardId(3) for hand, CardId(2) goes to bottom.
    let card3_id = object(&game, 3).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game
        .pending_decision()
        .expect("expected Sleight of Hand object choice");
    assert!(matches!(pending.frame(), RuleFrame::PromptChoice(_)));
    game = reply(
        &game,
        DecisionReply::PromptChoice(PromptChoiceReply::Objects(vec![card3_id])),
    );
    assert_eq!(
        object(&game, 3).zone(),
        ObjectZone::Hand(ALICE),
        "chosen card from Sleight of Hand must enter hand"
    );
    assert_eq!(
        object(&game, 2).zone(),
        ObjectZone::Library(ALICE),
        "OtherPile card from Sleight of Hand must remain in library (bottom)"
    );

    // 2. Cast Countered Beast (enters with a +1/+1 counter), then Wave Goodbye:
    //    Plain Beast (CardId(5), no counters) bounces to Bob's hand; Countered Beast (CardId(6)) stays on battlefield!
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 6).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(object(&game, 6).zone(), ObjectZone::Battlefield);

    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 2), (ManaType::Colourless, 2)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 5).zone(),
        ObjectZone::Hand(BOB),
        "Wave Goodbye must return creature without a +1/+1 counter to owner's hand"
    );
    assert_eq!(
        object(&game, 6).zone(),
        ObjectZone::Battlefield,
        "Wave Goodbye must leave creature with a +1/+1 counter on the battlefield"
    );

    // 3. Bob bolts Alice for 3 -> Sun Droplet triggers on DealtDamage(Player(You)) and gets 3 charge counters.
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::CastWithResources {
            spell: object(&game, 8).id(),
            target: Some(TargetRef::Player(ALICE)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    // Resolve Lightning Bolt (triggers Sun Droplet), then resolve Sun Droplet trigger.
    game = resolve_top(&game);
    assert_eq!(life(&game, ALICE), 17.into());
    assert_eq!(
        game.execution().window().entries().len(),
        1,
        "Sun Droplet should trigger when its controller is dealt damage"
    );
    game = resolve_top(&game);
    let charge_count = game
        .execution()
        .current_characteristics(object(&game, 7).id())
        .unwrap()
        .unwrap()
        .charge_counters
        .clone();
    assert_eq!(
        charge_count,
        3.into(),
        "Sun Droplet should gain 3 charge counters when controller is dealt 3 damage"
    );
}

#[test]
fn wave6_untap_lock_duration_and_colossus_library_replacement_and_cant_attack_unless() {
    let vocab = test_vocab();
    let frost_keeper = compile_card(
        &vocab,
        "Frost Keeper",
        "Creature \u{2014} Wizard",
        Some("{1}{U}"),
        Some(("2", "2")),
        "When this creature enters, tap target creature an opponent controls. That creature doesn't untap during its controller's untap step for as long as you control this creature.",
    );
    let shuffler = compile_card(
        &vocab,
        "Iron Colossus",
        "Artifact Creature \u{2014} Warrior",
        Some("{6}"),
        Some(("6", "6")),
        "If Iron Colossus would be put into a graveyard from anywhere, reveal Iron Colossus and shuffle it into its owner's library instead.",
    );
    let murder = compile_card(
        &vocab,
        "Doom Blade Test",
        "Instant",
        Some("{1}{B}"),
        None,
        "Destroy target creature.",
    );
    let castaway = compile_card(
        &vocab,
        "Desperate Castaways",
        "Creature \u{2014} Human Soldier",
        Some("{1}{B}"),
        Some(("2", "3")),
        "This creature can't attack unless you control an artifact.",
    );
    let bear = compile_card(
        &vocab,
        "Target Bear",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );

    let mut game = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: frost_keeper,
            },
            SeedCard {
                card: CardId(2),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: shuffler,
            },
            SeedCard {
                card: CardId(4),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: murder,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: castaway,
            },
        ],
        &[
            (ALICE, ManaType::Blue, 1),
            (ALICE, ManaType::Colourless, 1),
            (BOB, ManaType::Black, 1),
            (BOB, ManaType::Colourless, 1),
        ],
        0,
    );

    // 1. While Alice controls Iron Colossus (an artifact), Desperate Castaways does NOT have CannotAttack.
    let castaway_chars_with_art = game
        .execution()
        .current_characteristics(object(&game, 5).id())
        .unwrap()
        .unwrap();
    assert!(
        !castaway_chars_with_art
            .blocking_restrictions
            .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotAttack),
        "Desperate Castaways should be able to attack while controller controls an artifact"
    );

    // 2. Cast Frost Keeper -> ETB targets Bob's Bear -> taps it AND sets doesnt_untap_during_untap_step!
    let bear_id = object(&game, 2).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    if let Some(pending) = game.pending_decision() {
        assert!(matches!(pending.frame(), RuleFrame::TriggerTarget(_)));
        game = reply(
            &game,
            DecisionReply::TriggerTarget(TargetRef::Object(bear_id)),
        );
    }
    game = resolve_top(&game);
    assert_eq!(object(&game, 2).tapped(), Some(true));
    let bear_chars = game
        .execution()
        .current_characteristics(object(&game, 2).id())
        .unwrap()
        .unwrap();
    assert!(
        bear_chars.doesnt_untap_during_untap_step,
        "Frost Keeper must apply doesnt_untap_during_untap_step while controller controls it"
    );

    // 3. Bob destroys Iron Colossus: instead of graveyard, Iron Colossus is shuffled into Alice's library!
    //    AND now Alice controls no artifact -> Desperate Castaways gains CannotAttack!
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: Some(TargetRef::Object(object(&game, 3).id())),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 3).zone(),
        ObjectZone::Library(ALICE),
        "Iron Colossus replacement must shuffle it into owner's library instead of graveyard"
    );
    let castaway_chars_no_art = game
        .execution()
        .current_characteristics(object(&game, 5).id())
        .unwrap()
        .unwrap();
    assert!(
        castaway_chars_no_art
            .blocking_restrictions
            .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotAttack),
        "Desperate Castaways must gain CannotAttack once Alice no longer controls an artifact"
    );
}

#[test]
fn wave7_always_choose_color_exile_with_counters_and_greatest_stat() {
    let vocab = test_vocab();
    let coldsteel = compile_card(
        &vocab,
        "Coldsteel Heart",
        "Snow Artifact",
        Some("{2}"),
        None,
        "This artifact enters tapped.\nAs this artifact enters, choose a color.\n{T}: Add one mana of the chosen color.",
    );
    let exile_time = compile_card(
        &vocab,
        "Chrono Banish",
        "Sorcery",
        Some("{1}{U}"),
        None,
        "Exile target creature with three time counters on it.",
    );
    let greatest_check = compile_card(
        &vocab,
        "Artificer's Insight",
        "Sorcery",
        Some("{U}"),
        None,
        "If you control the artifact with the greatest mana value or tied for the greatest mana value, draw a card.",
    );
    let bear = compile_card(
        &vocab,
        "Target Bear",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: coldsteel,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: exile_time,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: greatest_check,
            },
            SeedCard {
                card: CardId(4),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
        ],
        &[(ALICE, ManaType::Blue, 2), (ALICE, ManaType::Colourless, 3)],
    );

    // 1. Cast Coldsteel Heart -> EntryChoice::Color(Green) -> enters battlefield tapped.
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Colourless, 2)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game
        .pending_decision()
        .expect("expected Coldsteel Heart color entry choice");
    assert!(matches!(pending.frame(), RuleFrame::EntryChoice(_)));
    game = reply(
        &game,
        DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Color(ManaType::Green)),
    );
    assert_eq!(object(&game, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(
        object(&game, 1).tapped(),
        Some(true),
        "Coldsteel Heart (AlwaysChooseColor) must enter tapped"
    );

    // 2. Exile Bob's Bear with 3 time counters -> Bear is in Exile with 3 time counters.
    let bear_id = object(&game, 4).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: Some(TargetRef::Object(bear_id)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 4).zone(),
        ObjectZone::Exile,
        "Chrono Banish must exile target creature"
    );
    let exiled_bear_id = object(&game, 4).id();
    assert_eq!(
        game.execution()
            .counters_of_kind(exiled_bear_id, spellwake_engine::effects::CounterKind::Time),
        3.into(),
        "exiled creature must receive 3 time counters"
    );

    // 3. Alice controls Coldsteel Heart (MV 2, greatest among artifacts) -> Artificer's Insight draws 1.
    let hand_before = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(ALICE))
        .count();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let hand_after = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(ALICE))
        .count();
    assert_eq!(
        hand_after, hand_before,
        "casting Artificer's Insight (-1) and drawing (+1) should keep hand size equal"
    );
}

#[test]
fn wave7_search_any_number_optional_if_previous_and_prevent_damage_compound() {
    let vocab = test_vocab();
    let land_surge = compile_card(
        &vocab,
        "Land Surge",
        "Sorcery",
        Some("{1}{G}"),
        None,
        "Search your library for any number of basic land cards, reveal those cards, put them into your hand, then shuffle.",
    );
    let forest = compile_card(
        &vocab,
        "Forest",
        "Basic Land \u{2014} Forest",
        None,
        None,
        "",
    );
    let optional_draw = compile_card(
        &vocab,
        "Dark Bargain Test",
        "Sorcery",
        Some("{B}"),
        None,
        "You may pay 2 life. If you do, draw a card.",
    );
    let safe_passage = compile_card(
        &vocab,
        "Safe Passage Test",
        "Instant",
        Some("{1}{W}"),
        None,
        "Prevent all damage that would be dealt to you and creatures you control this turn.",
    );
    let bolt = compile_card(
        &vocab,
        "Lightning Bolt",
        "Instant",
        Some("{R}"),
        None,
        "Lightning Bolt deals 3 damage to any target.",
    );

    let mut game = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: land_surge,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: forest.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: forest,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: optional_draw.clone(),
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: optional_draw,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: safe_passage,
            },
            SeedCard {
                card: CardId(7),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: bolt,
            },
        ],
        &[
            (ALICE, ManaType::Green, 1),
            (ALICE, ManaType::Black, 2),
            (ALICE, ManaType::White, 1),
            (ALICE, ManaType::Colourless, 2),
            (BOB, ManaType::Red, 1),
        ],
        1,
    );

    // 1. Land Surge searches any number of basic lands (picks both Forests).
    let f1 = object(&game, 2).id();
    let f2 = object(&game, 3).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let pending = game
        .pending_decision()
        .expect("expected any-number library search");
    assert!(matches!(pending.frame(), RuleFrame::LibraryTop(_)));
    game = reply(
        &game,
        DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
            selected: vec![f1, f2],
            colors: vec![],
        }),
    );
    assert_eq!(object(&game, 2).zone(), ObjectZone::Hand(ALICE));
    assert_eq!(object(&game, 3).zone(), ObjectZone::Hand(ALICE));

    // 2. Dark Bargain Test #1: accept optional pay 2 life -> life 20 -> 18, draws 1 card!
    let hand_before_accept = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(ALICE))
        .count();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert!(matches!(
        game.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    game = reply(
        &game,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(life(&game, ALICE), 18.into());
    let hand_after_accept = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(ALICE))
        .count();
    assert_eq!(
        hand_after_accept, hand_before_accept,
        "accepting optional pay 2 life must trigger IfPrevious draw (+1 after -1 cast)"
    );

    // 3. Dark Bargain Test #2: decline optional -> life stays 18, draws 0 cards!
    let hand_before_decline = hand_after_accept;
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 5).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    game = reply(
        &game,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(life(&game, ALICE), 18.into());
    let hand_after_decline = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(ALICE))
        .count();
    assert_eq!(
        hand_after_decline,
        hand_before_decline - 1,
        "declining optional must NOT draw a card"
    );

    // 4. Cast Safe Passage Test (Compound target: you and creatures you control),
    //    then Bob casts Lightning Bolt at Alice -> damage prevented, life stays 18!
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 6).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::CastWithResources {
            spell: object(&game, 7).id(),
            target: Some(TargetRef::Player(ALICE)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        18.into(),
        "Safe Passage compound prevention shield must prevent Lightning Bolt to Alice"
    );
}

#[test]
fn wave8_overload_graveyard_trigger_and_token_sorcery_restriction() {
    let vocab = test_vocab();
    let weapon_surge = compile_card(
        &vocab,
        "Weapon Surge",
        "Instant",
        Some("{R}"),
        None,
        "Target creature you control gets +1/+0 and gains first strike until end of turn.\nOverload {1}{R}",
    );
    let punishing_fire = compile_card(
        &vocab,
        "Punishing Fire",
        "Instant",
        Some("{1}{R}"),
        None,
        "Punishing Fire deals 2 damage to any target.\nWhenever an opponent gains life, you may pay {R}. If you do, return this card from your graveyard to your hand.",
    );
    let life_spell = compile_card(
        &vocab,
        "Revitalize Test",
        "Instant",
        Some("{W}"),
        None,
        "You gain 3 life.",
    );
    let merc_maker = compile_card(
        &vocab,
        "Hire Mercenary Test",
        "Sorcery",
        Some("{1}{R}"),
        None,
        "Create a 1/1 red Mercenary creature token with \"{T}: Target creature you control gets +1/+0 until end of turn. Activate only as a sorcery.\"",
    );
    let bear = compile_card(
        &vocab,
        "Target Bear",
        "Creature \u{2014} Beast",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: weapon_surge,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: bear.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: bear,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Graveyard,
                program: punishing_fire,
            },
            SeedCard {
                card: CardId(5),
                owner: BOB,
                destination: ObjectDestination::Hand,
                program: life_spell,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: merc_maker,
            },
        ],
        &[
            (ALICE, ManaType::Red, 3),
            (ALICE, ManaType::Colourless, 2),
            (BOB, ManaType::White, 1),
        ],
    );

    // 1. Cast Weapon Surge with Overload {1}{R} (use_alternative = true, no target) -> BOTH Bears become 3/2 First Strike!
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1), (ManaType::Colourless, 1)]),
                use_alternative: true,
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    for card_num in [2u64, 3u64] {
        let chars = game
            .execution()
            .current_characteristics(object(&game, card_num).id())
            .unwrap()
            .unwrap();
        let pt = chars.power_toughness.as_ref().unwrap();
        assert_eq!(pt.derived.power, 3.into());
        assert_eq!(pt.derived.toughness, 2.into());
        assert!(chars.has_keyword(spellwake_engine::characteristics::KeywordAbility::FirstStrike));
    }

    // 2. Cast Hire Mercenary Test -> creates a 1/1 red Mercenary token with its sorcery-restricted activated ability!
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 6).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    let merc_token = game
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.zone() == ObjectZone::Battlefield && c.is_token())
        .expect("expected Mercenary token on battlefield");
    let merc_chars = game
        .execution()
        .current_characteristics(merc_token.id())
        .unwrap()
        .unwrap();
    assert_eq!(merc_chars.printed_text.activated.len(), 1);
    assert_eq!(
        merc_chars.printed_text.activation_restrictions.len(),
        1,
        "Mercenary token must preserve its sorcery activation restriction"
    );

    // 3. Bob casts Revitalize Test (gains 3 life) -> Punishing Fire triggers from Alice's GRAVEYARD!
    //    Alice pays {R} -> Punishing Fire returns from graveyard to Alice's hand!
    game = pass(&game);
    game = advance(
        &game,
        BOB,
        GameCommand::CastWithResources {
            spell: object(&game, 5).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(life(&game, BOB), 23.into());
    assert_eq!(
        game.execution().window().entries().len(),
        1,
        "Punishing Fire must trigger from graveyard when opponent gains life"
    );
    game = resolve_top(&game);
    // Pay {R} when prompted for optional mana payment.
    let pending = game
        .pending_decision()
        .expect("expected Punishing Fire {R} payment prompt");
    match pending.frame() {
        RuleFrame::ResolutionPayment(_) => {
            game = reply(
                &game,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(
                    pay_mana(&[(ManaType::Red, 1)]).unwrap(),
                )),
            );
        }
        RuleFrame::OptionalEffect(_) => {
            game = reply(
                &game,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            );
            if matches!(
                game.pending_decision().map(|d| d.frame()),
                Some(RuleFrame::ResolutionPayment(_))
            ) {
                game = reply(
                    &game,
                    DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(
                        pay_mana(&[(ManaType::Red, 1)]).unwrap(),
                    )),
                );
            }
        }
        other => panic!("unexpected decision frame for Punishing Fire: {other:?}"),
    }
    assert_eq!(
        object(&game, 4).zone(),
        ObjectZone::Hand(ALICE),
        "Punishing Fire must return from graveyard to Alice's hand"
    );
}

#[test]
fn wave8_werewolf_spells_cast_last_turn_and_didnt_put_in_zone() {
    let vocab = test_vocab();
    let werewolf_watch = compile_card(
        &vocab,
        "Moon Watch",
        "Creature \u{2014} Human Werewolf",
        Some("{1}{G}"),
        Some(("2", "2")),
        "At the beginning of each upkeep, if no spells were cast last turn, you gain 2 life.",
    );
    let courier = compile_card(
        &vocab,
        "Forest Courier Test",
        "Sorcery",
        Some("{G}"),
        None,
        "You may search your library for a Forest card, reveal it, put it into your hand, then shuffle. If you don't put a card into your hand this way, you gain 3 life.",
    );
    let forest = compile_card(
        &vocab,
        "Forest",
        "Basic Land \u{2014} Forest",
        None,
        None,
        "",
    );

    // Part A: Pass Turn 1 with ZERO spells cast -> at Turn 2 Upkeep, Moon Watch triggers (20 -> 22)!
    let mut game = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: werewolf_watch,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: courier.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: courier,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: forest,
            },
        ],
        &[(ALICE, ManaType::Green, 2)],
        1,
    );

    // Advance from Turn 1 PrecombatMain through end of Turn 1 to Turn 2 Upkeep (no spells cast!).
    for _ in 0..40 {
        if game.turn().is_some_and(|t| t.position().number == 2)
            && !game.execution().window().entries().is_empty()
        {
            break;
        }
        if matches!(
            game.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { .. })
        ) {
            game = pass(&game);
        } else {
            break;
        }
    }
    assert_eq!(
        game.turn().unwrap().position().number,
        2,
        "should have reached Turn 2"
    );
    assert_eq!(
        game.execution().window().entries().len(),
        1,
        "Moon Watch must trigger on Turn 2 upkeep when no spells were cast on Turn 1"
    );
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        22.into(),
        "Moon Watch trigger should gain 2 life (20 -> 22)"
    );

    // Part B: Test DidntPutInZone on a fresh Turn 1 PrecombatMain game:
    //   1) Decline optional Forest search -> DidntPutInZone(Hand) is TRUE -> gain 3 life (20 -> 23).
    //   2) Accept optional Forest search and find Forest -> DidntPutInZone(Hand) is FALSE -> stays 23!
    let mut game2 = start_game_with_padding(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: compile_card(
                    &vocab,
                    "Forest Courier Test",
                    "Sorcery",
                    Some("{G}"),
                    None,
                    "You may search your library for a Forest card, reveal it, put it into your hand, then shuffle. If you don't put a card into your hand this way, you gain 3 life.",
                ),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: compile_card(
                    &vocab,
                    "Forest Courier Test 2",
                    "Sorcery",
                    Some("{G}"),
                    None,
                    "You may search your library for a Forest card, reveal it, put it into your hand, then shuffle. If you don't put a card into your hand this way, you gain 3 life.",
                ),
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::LibraryTop,
                program: compile_card(
                    &vocab,
                    "Forest",
                    "Basic Land \u{2014} Forest",
                    None,
                    None,
                    "",
                ),
            },
        ],
        &[(ALICE, ManaType::Green, 2)],
        1,
    );

    // Cast Courier #1 and DECLINE search -> DidntPutInZone(Hand) fires -> 20 -> 23!
    game2 = advance(
        &game2,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game2, 2).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game2 = resolve_top(&game2);
    game2 = reply(
        &game2,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(
        life(&game2, ALICE),
        23.into(),
        "declining Forest search must trigger DidntPutInZone(Hand) -> +3 life"
    );

    // Cast Courier #2, ACCEPT search and pick Forest -> Forest in hand, life stays 23!
    let forest_id = object(&game2, 4).id();
    game2 = advance(
        &game2,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game2, 3).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game2 = resolve_top(&game2);
    game2 = reply(
        &game2,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    game2 = reply(
        &game2,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(forest_id),
        }),
    );
    assert_eq!(object(&game2, 4).zone(), ObjectZone::Hand(ALICE));
    assert_eq!(
        life(&game2, ALICE),
        23.into(),
        "finding Forest must NOT trigger DidntPutInZone(Hand)"
    );
}

#[test]
fn wave9_outlaw_batching_multicolor_protection_and_any_combination_mana() {
    let vocab = test_vocab();
    let shoot = compile_card(
        &vocab,
        "Shoot the Sheriff",
        "Instant",
        Some("{1}{B}"),
        None,
        "Destroy target non-outlaw creature.",
    );
    let posse_boss = compile_card(
        &vocab,
        "Outlaw Marshal Test",
        "Creature \u{2014} Human Mercenary",
        Some("{2}{R}"),
        Some(("2", "2")),
        "Other outlaws you control have haste.",
    );
    let pirate = compile_card(
        &vocab,
        "Corsair Test",
        "Creature \u{2014} Human Pirate",
        Some("{1}{R}"),
        Some(("2", "1")),
        "",
    );
    let rogue = compile_card(
        &vocab,
        "Bobs Rogue",
        "Creature \u{2014} Human Rogue",
        Some("{1}{U}"),
        Some(("2", "1")),
        "",
    );
    let bear = compile_card(
        &vocab,
        "Bobs Bear",
        "Creature \u{2014} Bear",
        Some("{1}{G}"),
        Some(("2", "2")),
        "",
    );
    let crusader = compile_card(
        &vocab,
        "Mirran Crusader",
        "Creature \u{2014} Human Knight",
        Some("{1}{W}{W}"),
        Some(("2", "2")),
        "Double strike, protection from black and from green",
    );
    let manamorphose = compile_card(
        &vocab,
        "Mana Mix Test",
        "Instant",
        Some("{1}{R}"),
        None,
        "Add two mana in any combination of colors.",
    );
    let gu_spell = compile_card(
        &vocab,
        "Simic Check Test",
        "Sorcery",
        Some("{G}{U}"),
        None,
        "You gain 3 life.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: shoot,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: posse_boss,
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: pirate,
            },
            SeedCard {
                card: CardId(4),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: rogue,
            },
            SeedCard {
                card: CardId(5),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: bear,
            },
            SeedCard {
                card: CardId(6),
                owner: BOB,
                destination: ObjectDestination::Battlefield { controller: BOB },
                program: crusader,
            },
            SeedCard {
                card: CardId(7),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: manamorphose,
            },
            SeedCard {
                card: CardId(8),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: gu_spell,
            },
        ],
        &[
            (ALICE, ManaType::Black, 1),
            (ALICE, ManaType::Red, 1),
            (ALICE, ManaType::Colourless, 2),
        ],
    );

    // 1. Outlaw batching static: Alice's Pirate gains Haste; Bob's Bear does not.
    let pirate_kw = game
        .execution()
        .current_characteristics(object(&game, 3).id())
        .unwrap()
        .unwrap()
        .keywords;
    assert!(
        pirate_kw.contains(&spellwake_engine::characteristics::KeywordAbility::Haste),
        "Other outlaws you control should grant Haste to Alice's Pirate"
    );
    let bear_kw = game
        .execution()
        .current_characteristics(object(&game, 5).id())
        .unwrap()
        .unwrap()
        .keywords;
    assert!(
        !bear_kw.contains(&spellwake_engine::characteristics::KeywordAbility::Haste),
        "Non-outlaw creature should not gain Haste"
    );

    // 2. Multi-color Protection: Mirran Crusader has both ProtectionFromBlack and ProtectionFromGreen.
    let crusader_kw = game
        .execution()
        .current_characteristics(object(&game, 6).id())
        .unwrap()
        .unwrap()
        .keywords;
    assert!(
        crusader_kw
            .contains(&spellwake_engine::characteristics::KeywordAbility::ProtectionFromBlack)
            && crusader_kw
                .contains(&spellwake_engine::characteristics::KeywordAbility::ProtectionFromGreen),
        "Mirran Crusader must project both ProtectionFromBlack and ProtectionFromGreen"
    );

    // 3. Shoot the Sheriff cannot target Bob's Rogue (an outlaw) or Mirran Crusader (pro-black),
    //    but CAN target and destroy Bob's Bear.
    let rogue_id = object(&game, 4).id();
    let bear_id = object(&game, 5).id();
    let crusader_id = object(&game, 6).id();
    assert!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: ALICE,
            command: GameCommand::CastWithResources {
                spell: object(&game, 1).id(),
                target: Some(TargetRef::Object(rogue_id)),
                payment: CastingPayment {
                    mana: pay_mana(&[(ManaType::Black, 1), (ManaType::Colourless, 1)]),
                    ..Default::default()
                },
            },
        })
        .is_err(),
        "Shoot the Sheriff must reject targeting a Rogue (outlaw)"
    );
    assert!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: ALICE,
            command: GameCommand::CastWithResources {
                spell: object(&game, 1).id(),
                target: Some(TargetRef::Object(crusader_id)),
                payment: CastingPayment {
                    mana: pay_mana(&[(ManaType::Black, 1), (ManaType::Colourless, 1)]),
                    ..Default::default()
                },
            },
        })
        .is_err(),
        "Shoot the Sheriff must reject targeting Mirran Crusader (protection from black)"
    );
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 1).id(),
            target: Some(TargetRef::Object(bear_id)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Black, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        object(&game, 5).zone(),
        ObjectZone::Graveyard(BOB),
        "Shoot the Sheriff must destroy Bob's non-outlaw Bear"
    );

    // 4. Any-combination mana: cast Mana Mix Test -> choose Green then Blue -> cast {G}{U} spell!
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 7).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Red, 1), (ManaType::Colourless, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert!(matches!(
        game.pending_decision()
            .expect("first mana color choice")
            .frame(),
        RuleFrame::ResolutionManaColor(_)
    ));
    game = reply(&game, DecisionReply::ManaColor(ManaType::Green));
    assert!(matches!(
        game.pending_decision()
            .expect("second mana color choice")
            .frame(),
        RuleFrame::ResolutionManaColor(_)
    ));
    game = reply(&game, DecisionReply::ManaColor(ManaType::Blue));
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 8).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Green, 1), (ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        23.into(),
        "spending the {{G}}{{U}} produced by any-combination mana should cast Simic Check Test (20 -> 23)"
    );
}

#[test]
fn wave9_first_spell_discount_target_object_stat_and_copy_keeps_ability() {
    let vocab = test_vocab();
    let melek = compile_card(
        &vocab,
        "First Spell Scholar",
        "Creature \u{2014} Weird Wizard",
        Some("{3}{U}{R}"),
        Some(("2", "2")),
        "The first instant or sorcery spell you cast each turn costs {3} less to cast.",
    );
    let draw_spell = compile_card(
        &vocab,
        "Expensive Insight",
        "Sorcery",
        Some("{3}{U}"),
        None,
        "Draw a card.",
    );
    let reverence = compile_card(
        &vocab,
        "Power Blessing",
        "Sorcery",
        Some("{W}"),
        None,
        "You gain life equal to the power of target creature you control.",
    );
    let beast = compile_card(
        &vocab,
        "Giant Beast",
        "Creature \u{2014} Beast",
        Some("{3}{G}"),
        Some(("4", "4")),
        "",
    );
    let imitator = compile_card(
        &vocab,
        "Shapeshift Mimic",
        "Creature \u{2014} Shapeshifter",
        Some("{1}{U}"),
        Some(("1", "1")),
        "{U}: This creature becomes a copy of target creature, except it has this ability.",
    );

    let mut game = start_game(
        Arc::clone(&vocab),
        vec![
            SeedCard {
                card: CardId(1),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: melek,
            },
            SeedCard {
                card: CardId(2),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: draw_spell.clone(),
            },
            SeedCard {
                card: CardId(3),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: draw_spell,
            },
            SeedCard {
                card: CardId(4),
                owner: ALICE,
                destination: ObjectDestination::Hand,
                program: reverence,
            },
            SeedCard {
                card: CardId(5),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: beast,
            },
            SeedCard {
                card: CardId(6),
                owner: ALICE,
                destination: ObjectDestination::Battlefield { controller: ALICE },
                program: imitator,
            },
        ],
        &[
            (ALICE, ManaType::Blue, 4),
            (ALICE, ManaType::White, 1),
            (ALICE, ManaType::Colourless, 3),
        ],
    );

    // 1. First instant/sorcery spell ({3}{U}) costs only {U} thanks to {3} discount!
    let hand_before = hand_count(&game, ALICE);
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        hand_count(&game, ALICE),
        hand_before,
        "first sorcery should cast for {{U}} (discounted by {{3}}) and draw a card"
    );

    // Second sorcery this turn is NOT discounted: paying only {U} fails, paying {3}{U} succeeds.
    assert!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: ALICE,
            command: GameCommand::CastWithResources {
                spell: object(&game, 3).id(),
                target: None,
                payment: CastingPayment {
                    mana: pay_mana(&[(ManaType::Blue, 1)]),
                    ..Default::default()
                },
            },
        })
        .is_err(),
        "second sorcery this turn must NOT receive the first-spell {{3}} discount"
    );
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 3).id(),
            target: None,
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::Blue, 1), (ManaType::Colourless, 3)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);

    // 2. ObjectStat on TargetObject: Power Blessing targeting 4/4 Beast gains 4 life (20 -> 24).
    let beast_id = object(&game, 5).id();
    game = advance(
        &game,
        ALICE,
        GameCommand::CastWithResources {
            spell: object(&game, 4).id(),
            target: Some(TargetRef::Object(beast_id)),
            payment: CastingPayment {
                mana: pay_mana(&[(ManaType::White, 1)]),
                ..Default::default()
            },
        },
    )
    .into_state();
    game = resolve_top(&game);
    assert_eq!(
        life(&game, ALICE),
        24.into(),
        "Power Blessing targeting 4/4 Beast must gain 4 life (20 -> 24)"
    );

    // 3. CopyException::HasThisAbility: Shapeshift Mimic becomes a 4/4 Beast copy AND keeps its ability!
    let mimic_id = object(&game, 6).id();
    let melek_id = object(&game, 1).id();
    let ability_id = first_activated_id(&game, 6);
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: mimic_id,
            ability: ability_id,
            target: TargetRef::Object(beast_id),
            payment: pay_mana(&[(ManaType::Blue, 1)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    let projected = game
        .execution()
        .current_characteristics(mimic_id)
        .unwrap()
        .unwrap();
    let pt = projected.power_toughness.as_ref().unwrap();
    assert_eq!(
        pt.derived.power,
        4.into(),
        "mimic should copy Beast's power"
    );
    assert_eq!(
        pt.derived.toughness,
        4.into(),
        "mimic should copy Beast's toughness"
    );
    // Activate the retained copy ability a second time targeting Melek (2/2) to prove HasThisAbility!
    // CopyExceptions::keep_abilities renumbers kept abilities sequentially from ActivatedAbilityId(0).
    game = advance(
        &game,
        ALICE,
        GameCommand::ActivateAbility {
            source: mimic_id,
            ability: spellwake_engine::activated::ActivatedAbilityId(0),
            target: TargetRef::Object(melek_id),
            payment: pay_mana(&[(ManaType::Blue, 1)]),
        },
    )
    .into_state();
    game = resolve_top(&game);
    let projected2 = game
        .execution()
        .current_characteristics(mimic_id)
        .unwrap()
        .unwrap();
    let pt2 = projected2.power_toughness.as_ref().unwrap();
    assert_eq!(
        pt2.derived.power,
        2.into(),
        "mimic must retain its copy ability via HasThisAbility and re-copy into a 2/2"
    );
}
