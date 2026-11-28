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
        "Beast".to_string(),
        "Bird".to_string(),
        "Druid".to_string(),
        "Elf".to_string(),
        "Goblin".to_string(),
        "Human".to_string(),
        "Knight".to_string(),
        "Soldier".to_string(),
        "Spirit".to_string(),
        "Warrior".to_string(),
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
    if let Some(pending) = game.pending_decision() {
        if let RuleFrame::TimestampOrder(order) = pending.frame() {
            let members = order.members().iter().map(|m| m.member).collect();
            game = reply(&game, DecisionReply::TimestampOrder(members));
        }
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
