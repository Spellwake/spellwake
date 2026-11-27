#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Runtime and validation integration tests for Pass F2-B Wave 8B engine additions:
//! - `ObjectMutation::MoveCounters` (direct 1-target move, interactive `any_number` `CounterRemoval`,
//!   and CR 122.5 no-op when `from == to`)
//! - `SpellDefinition.uncastable_mana_cost` (CR 118.6: normal hand cast rejected, Suspend and
//!   Cascade free casts succeed)
//! - `SubtypeFilter::AnyLandMask`, `ObjectFilterFlags::{COMMANDER_ONLY, NOT_COMMANDER}` via
//!   `SupportedAppearance.commander_owner`, and `ObjectClause.owner` vs `controller`
//! - `CastPermissionRule::GrantSpellCascade` and `GrantSpellKeyword { any_player: true, .. }`
//! - `StaticRestriction::ProhibitCasting` with `MATCHES_SOURCE_ENTRY_COLOR` and chosen card name,
//!   plus `BlockingRestriction::CannotBlockMatching` with `PowerFilter::CompareAnchor(SourcePower)`
//! - Conditional `BlockingRestriction::MustAttackEachCombatIfAble` and multi-entry
//!   `AbilityChange::granted_triggers`
//! - `TriggerEvent::PlayerTargeted`, crime tracking (CR 700.13), and
//!   `TriggeredManaRule::LandTappedChosenColor` (CR 605.1b)
//! - World-eval step/phase/player stats (`IsOwnMainPhase`, `IsInStep`, `IsInPhase`,
//!   `PlayerHasMostLife`, `PlayerControlsMost`, `AttackedMatchingThisTurn` surviving attacker death)
//! - Exile-zone trigger discovery (`TriggerZone::Any` in `ObjectZone::Exile`) and
//!   `Instruction::ExileSubjectUntilSourceLeaves` (CR 610.3)
//! - `Instruction::CreateTokenCopyExt { enters_attacking: true, .. }`,
//!   `AttackingDefenderScope::Player`, and `TokenSize::AtCreationPair`
//! - `TemporaryRuleKind::CantBeCounteredByColors` and
//!   `ResolvedPartSpec::CannotBeTargetedBySpellsOfColors`
//! - `Instruction::OfferAnyPlayer` resetting `last_action_succeeded` on all-decline and guarding
//!   `paid_body` when `cost_body` cannot be paid
//! - `DamagePreventFollowup::DealPreventedToSourceController`, `spellwake_engine::program::Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::draw_game()))`, and
//!   `Instruction::TurnFaceUpSubject`
//! - `ControllerFilter::FirstTargetController` and validation fixes for `BranchIfFalse` after
//!   `OptionalEffect + Sacrifice` (`LastSacrificeSucceeded`) and `EmitReflexiveTrigger` inheriting
//!   `LastSacrificedCreature`.

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, BlockingRestriction, CastPermissionRule, Color, ControllerSelector,
        DamagePreventFollowup, DamagePreventMode, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId,
        StaticContinuousAbility, StaticDamageKind, StaticDamageOp, StaticDamageRecipient,
        StaticDamageRule, StaticPart, StaticParts, StaticRestriction, StaticSourceCondition,
    },
    combat::{AttackDeclaration, BlockDeclaration, CombatError, CombatRules},
    draw::GameOutcome,
    effects::{CounterKind, EffectError, LibraryTopReply, OptionalEffectChoice, SeedCard},
    face_down::{FaceDownKind, FaceUpAbility},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, OpeningSetup, RuleFrame,
    },
    land::{CardNameIndex, EntryChoice, EntryTap, LandType, NonlandCardNames},
    mana::{
        CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType, TriggeredManaRule,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::RulesOccurrence,
    opening::MulliganChoice,
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, ColorMask, Comparison, ControllerFilter,
        DynamicStatAnchor, EffectSubject, EventValueKind, ExecutableBody, ExileAction, Instruction,
        LibraryPlacement, LibrarySearch, MoveSubjectRef, ObjectClause, ObjectFilter,
        ObjectFilterFlags, ObjectStatKind, PermanentDefinition, PermanentTypeFilter, PhaseCheck,
        PlaneswalkerSubtype, PlayerRelation, PlayerStatExpr, PowerFilter, Program, ProgramKind,
        ResolutionCondition, ResolvingPlayer, SacrificeSelection, SearchCardMatcher,
        SpellDefinition, SpellFilter, SubtypeFilter, Supertype, TemporaryRuleKind, TriggerEvent,
        TriggerGrouping, TriggerSpec, TriggerZone, TriggeredAbility, TriggeredAbilityId, ValueExpr,
        ValueObjectRef, ValueZone,
    },
    setup::GameSeed,
    shuffle::EntropyTape,
    targets::TargetRef,
    token::{
        AttackingDefenderScope, TokenCreation, TokenDefinition, TokenName, TokenRecipients,
        TokenSize, TokenSubtype,
    },
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU8,
    sync::Arc,
};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 8B test program must validate");
    program
}

fn attempt(
    game: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<NativeCandidate, GameError> {
    observe_advance(
        game,
        GameInput {
            basis: game.basis().clone(),
            actor,
            command,
        },
    )
}

fn cast_untargeted_as(game: &GameState, actor: SeatId, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    cast_untargeted_as(game, P0, id)
}

fn cast_targeted(game: &GameState, actor: SeatId, id: u64, target: TargetRef) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastPaid {
            spell: object(game, id),
            target,
            payment: ManaPayment::default(),
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

fn cast_fixed_targets(
    game: &GameState,
    actor: SeatId,
    id: u64,
    fixed_targets: Vec<TargetRef>,
) -> NativeCandidate {
    let cast = submit(
        game,
        actor,
        GameCommand::CastWithResources {
            spell: object(game, id),
            target: None,
            payment: CastingPayment {
                fixed_targets,
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let order = if actor == P0 { [P0, P1] } else { [P1, P0] };
    pass_in_order(cast.state(), order)
}

fn opp_battlefield(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P1,
        destination: ObjectDestination::Battlefield { controller: P1 },
        program,
    }
}

fn opp_hand(id: u64, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P1,
        destination: ObjectDestination::Hand,
        program,
    }
}

fn exile_of(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Exile,
        program,
    }
}

fn library_top(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::LibraryTop,
        program,
    }
}

fn named(mut program: Program, name: &str) -> Program {
    program.names = ObjectNames::single(EnglishCardName::new(name.into()).unwrap());
    program
}

fn with_color(mut program: Program, color: Color) -> Program {
    program.color_indicator = Some(BTreeSet::from([color]));
    program
}

fn vanilla_creature(power: i64, toughness: i64) -> Program {
    Program::creature(
        Some(ManaCost::free()),
        power.into(),
        toughness.into(),
        BTreeSet::new(),
    )
}

fn basic_land(land_type: LandType, mana_type: ManaType) -> Program {
    let mut land = Program::land(
        BTreeSet::from([land_type]),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(mana_type, 1.into())])),
        }],
    );
    land.supertypes.insert(Supertype::Basic);
    land
}

fn basic_forest() -> Program {
    basic_land(LandType::Forest, ManaType::Green)
}

fn gain_life_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::AdjustLife(
                Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    ValueExpr::literal(amount),
                )),
            )],
        ),
    )
}

fn damage_any_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleAnyTarget,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(amount.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    )
}

fn destroy_permanent_instant() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                            allow_regenerate: true,
                        },
                    },
                ),
            ))],
        ),
    )
}

fn zone_of(game: &GameState, id: u64) -> ObjectZone {
    let obj = object(game, id);
    game.execution()
        .window()
        .cards()
        .get(obj)
        .expect("seeded card must exist")
        .zone()
}

fn plus_one_counters(game: &GameState, obj: ObjectId) -> i64 {
    current(game, obj)
        .plus_one_counters
        .to_usize()
        .map_or(0, |v| i64::try_from(v).unwrap())
}

fn num_pair(power: i64, toughness: i64) -> NumericPair {
    NumericPair {
        power: NumericExpression::Literal(power.into()),
        toughness: NumericExpression::Literal(toughness.into()),
    }
}

fn self_selector() -> PermanentSelector {
    PermanentSelector {
        controller: ControllerSelector::SourceItself,
        ..PermanentSelector::default()
    }
}

fn controlled_creatures_selector() -> PermanentSelector {
    PermanentSelector {
        controller: ControllerSelector::SourceController,
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    }
}

fn attack_player(attacker: ObjectId, defender: SeatId) -> AttackDeclaration {
    AttackDeclaration {
        attacker,
        defender,
        defending_planeswalker: None,
    }
}

fn answer_yes_no_offer(game: &GameState, actor: SeatId, accept: bool) -> NativeCandidate {
    let pending = game
        .pending_decision()
        .expect("expected OfferAnyPlayer OptionIndex frame");
    assert_eq!(pending.chooser(), actor);
    assert!(matches!(pending.frame(), RuleFrame::OptionIndex(_)));
    reply(game, actor, DecisionReply::OptionIndex(u8::from(!accept)))
}

fn world_at_upkeep(
    vocabulary: Arc<CreatureTypeVocabulary>,
    cards: Vec<SeedCard>,
    order: Vec<CardId>,
) -> GameState {
    let context = GameContext::new(
        "characteristics-frozen-I-v1".into(),
        vocabulary.rules_revision().into(),
        "literal-frozen-definitions".into(),
        "characteristics-layers-v2".into(),
        "ordinary-first-turn".into(),
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let game = GameState::initialize_scheduled(
        GameId(20261125),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    game
}

fn advance_to_declare_attackers(game: &GameState) -> GameState {
    assert_eq!(game.turn().unwrap().position().step, Step::PrecombatMain);
    let beg = pass_pair(game).into_state();
    assert_eq!(beg.turn().unwrap().position().step, Step::BeginningCombat);
    let decl = pass_pair(&beg).into_state();
    assert_eq!(decl.turn().unwrap().position().step, Step::DeclareAttackers);
    decl
}

fn commander_main_world(
    vocab: Arc<CreatureTypeVocabulary>,
    p0_commander: Program,
    p1_commander: Program,
    p0_top_cards: Vec<(u64, Program)>,
) -> GameState {
    let context = GameContext::new(
        "wave8b-commander-v1".into(),
        vocab.rules_revision().into(),
        "literal-frozen-definitions".into(),
        "characteristics-layers-v2".into(),
        "commander-opening".into(),
        vocab.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    let mut cards = Vec::with_capacity(200);
    cards.push(SeedCard {
        card: CardId(1),
        owner: P0,
        destination: ObjectDestination::Command,
        program: p0_commander,
    });
    let filler = validated(basic_forest(), &vocab);
    for (id, prog) in &p0_top_cards {
        cards.push(SeedCard {
            card: CardId(*id),
            owner: P0,
            destination: ObjectDestination::LibraryTop,
            program: prog.clone(),
        });
    }
    let p0_remaining = 99usize.checked_sub(p0_top_cards.len()).unwrap();
    for i in 0..p0_remaining {
        cards.push(SeedCard {
            card: CardId(200 + u64::try_from(i).unwrap()),
            owner: P0,
            destination: ObjectDestination::LibraryBottom,
            program: filler.clone(),
        });
    }
    cards.push(SeedCard {
        card: CardId(101),
        owner: P1,
        destination: ObjectDestination::Command,
        program: p1_commander,
    });
    for i in 0..99usize {
        cards.push(SeedCard {
            card: CardId(400 + u64::try_from(i).unwrap()),
            owner: P1,
            destination: ObjectDestination::LibraryBottom,
            program: filler.clone(),
        });
    }

    let seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        vec![],
        vec![],
        40.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocab,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let setup = OpeningSetup {
        seed,
        entropy: EntropyTape::new(vec![0; 20_000]),
    };
    let mut state = GameState::initialize_commander_opening(GameId(90301), context, setup)
        .unwrap()
        .into_state();
    // Both players keep opening seven-card hands.
    while let Some(decision) = state.pending_decision() {
        let chooser = decision.chooser();
        state = reply(
            &state,
            chooser,
            DecisionReply::Mulligan(MulliganChoice::Keep),
        )
        .into_state();
    }
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    let main = pass_pair(&state).into_state();
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    main
}

fn find_hand_card_by_name(game: &GameState, owner: SeatId, expected: &str) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .iter()
        .find(|obj| {
            obj.zone() == ObjectZone::Hand(owner)
                && current(game, obj.id())
                    .current_names
                    .iter()
                    .any(|n| n.as_str() == expected)
        })
        .expect("expected named card in hand")
        .id()
}

// ---------------------------------------------------------------------------
// 1. ObjectMutation::MoveCounters: direct move, interactive any_number CounterRemoval, CR 122.5
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 2. Uncastable mana cost spell (CR 118.6): hand cast rejected, Suspend & Cascade succeed
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 3. SubtypeFilter::AnyLandMask, COMMANDER_ONLY / NOT_COMMANDER, ObjectClause.owner
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 4. GrantSpellCascade and GrantSpellKeyword { any_player: true, .. }
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. ProhibitCasting (entry color + chosen name) & CannotBlockMatching CompareAnchor(SourcePower)
// ---------------------------------------------------------------------------

#[test]
fn prohibit_casting_chosen_color_and_name_and_blocking_compare_anchor() {
    let vocab = vocabulary();

    // Part A: Iona-style ProhibitCasting with MATCHES_SOURCE_ENTRY_COLOR.
    let mut iona = Program::new(
        BTreeSet::from([CardType::Creature]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: Some(EntryTap::ChooseColor),
            attachment: None,
            printed_pt: Some(pt(7, 7).into()),
            casting_cost: Some(ManaCost::free()),
        }),
    );
    iona.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ProhibitCasting {
            player: PlayerRelation::Opponent,
            filter: Box::new(SpellFilter::from_object(
                ObjectFilter::any().with_flags(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR),
            )),
        },
    });
    let iona = validated(iona, &vocab);

    // Meddling-Mage-style chosen-name ProhibitCasting using NonlandCardNames + MATCHES_CHOSEN_NAME.
    let bolt_name = EnglishCardName::new("Lightning Bolt".into()).unwrap();
    let giant_name = EnglishCardName::new("Giant Growth".into()).unwrap();
    let table = Arc::new(NonlandCardNames::new(BTreeSet::from([
        bolt_name, giant_name,
    ])));
    let mut mage = Program::new(
        BTreeSet::from([CardType::Creature]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: Some(EntryTap::ChooseNonlandCardName),
            attachment: None,
            printed_pt: Some(pt(2, 2).into()),
            casting_cost: Some(ManaCost::free()),
        }),
    );
    mage.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ProhibitCastingChosenName(table),
    });
    let mage = validated(mage, &vocab);

    let blue_spell = validated(with_color(gain_life_instant(1), Color::Blue), &vocab);
    let red_bolt = validated(
        named(
            with_color(gain_life_instant(2), Color::Red),
            "Lightning Bolt",
        ),
        &vocab,
    );
    let red_other = validated(
        named(with_color(gain_life_instant(3), Color::Red), "Giant Growth"),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, iona, false),
            card(2, mage, false),
            opp_hand(3, blue_spell),
            opp_hand(4, red_bolt),
            opp_hand(5, red_other),
        ],
        vec![],
    );

    // Cast Iona and choose Blue.
    let iona_prompt = cast_untargeted(&game_a, 1);
    assert!(matches!(
        iona_prompt.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(_))
    ));
    let after_iona = reply(
        iona_prompt.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::Color(ManaType::Blue)),
    );

    // Cast Meddling Mage and choose "Lightning Bolt" from the sorted NonlandCardNames domain.
    let mage_prompt = cast_untargeted(after_iona.state(), 2);
    let RuleFrame::EntryChoice(mage_frame) = mage_prompt
        .state()
        .pending_decision()
        .expect("Meddling Mage must prompt for card name")
        .frame()
    else {
        panic!("expected EntryChoice frame for Meddling Mage");
    };
    let bolt_idx = mage_frame
        .card_names()
        .iter()
        .position(|n| n.as_str() == "Lightning Bolt")
        .expect("Lightning Bolt must be in NonlandCardNames");
    let after_mage = reply(
        mage_prompt.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::CardName(CardNameIndex(bolt_idx))),
    );

    // Pass priority to P1.
    let p1_prio = submit(after_mage.state(), P0, GameCommand::Pass);
    assert_priority(p1_prio.state(), P1);

    // Blue spell is blocked by Iona's chosen entry color.
    assert!(
        attempt(
            p1_prio.state(),
            P1,
            GameCommand::CastUntargeted {
                spell: object(p1_prio.state(), 3),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "Blue spell must be prohibited by Iona choosing Blue"
    );

    // Red "Lightning Bolt" is blocked by chosen card name.
    assert!(
        attempt(
            p1_prio.state(),
            P1,
            GameCommand::CastUntargeted {
                spell: object(p1_prio.state(), 4),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "named card 'Lightning Bolt' must be prohibited"
    );

    // Red "Giant Growth" is permitted (neither Blue nor named "Lightning Bolt").
    let red_cast = submit(
        p1_prio.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio.state(), 5),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_red = pass_in_order(red_cast.state(), [P1, P0]);
    assert_eq!(
        after_red.state().execution().life_total(P1),
        Some(&23.into())
    );

    // Part B: CannotBlockMatching with CompareAnchor(SourcePower) (Spitfire Handler style).
    let mut handler = vanilla_creature(2, 2);
    handler.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: self_selector(),
        parts: StaticParts(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotBlockMatching(ObjectFilter::creature().with_power(
                PowerFilter::CompareAnchor {
                    op: Comparison::Greater,
                    anchor: DynamicStatAnchor::SourcePower,
                },
            )),
        )]),
    });
    let handler = validated(handler, &vocab);

    let pump = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(2, 2))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ),
        &vocab,
    );

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, validated(vanilla_creature(3, 3), &vocab), true),
            card(2, validated(vanilla_creature(2, 2), &vocab), true),
            opp_battlefield(3, handler),
            opp_hand(4, pump),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let brute = object(&game_b, 1);
    let bear = object(&game_b, 2);
    let handler_id = object(&game_b, 3);

    // Declare both 3/3 brute and 2/2 bear as attackers.
    let decl_atk = advance_to_declare_attackers(&game_b);
    let after_atk = reply(
        &decl_atk,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(brute, P1), attack_player(bear, P1)]),
    );

    // Move to DeclareBlockers (without pumping handler): handler is 2/2.
    let decl_blk_unpumped = pass_pair(after_atk.state());
    assert_eq!(
        decl_blk_unpumped.state().turn().unwrap().position().step,
        Step::DeclareBlockers
    );
    let blk_decision = decl_blk_unpumped.state().pending_decision().unwrap();
    assert_eq!(blk_decision.chooser(), P1);

    // Blocking 3/3 brute (3 > 2) must be rejected.
    assert!(
        attempt(
            decl_blk_unpumped.state(),
            P1,
            GameCommand::Reply {
                decision: blk_decision.id(),
                choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: handler_id,
                    attacker: brute,
                }]),
            },
        )
        .is_err(),
        "2/2 Spitfire Handler must not be able to block 3/3 attacker"
    );

    // Blocking 2/2 bear (2 <= 2) must succeed.
    let blocked_bear = reply(
        decl_blk_unpumped.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: handler_id,
            attacker: bear,
        }]),
    );
    assert!(blocked_bear.state().pending_decision().is_none());

    // Now branch from after_atk: P1 pumps handler (+2/+2 -> 4/4) during DeclareAttackers priority,
    // then in DeclareBlockers handler (power 4) CAN block 3/3 brute!
    let p1_atk_prio = submit(after_atk.state(), P0, GameCommand::Pass);
    let pumped = cast_targeted(p1_atk_prio.state(), P1, 4, TargetRef::Object(handler_id));
    assert_eq!(
        current(pumped.state(), handler_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    let decl_blk_pumped = pass_pair(pumped.state());
    let blocked_brute = reply(
        decl_blk_pumped.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: handler_id,
            attacker: brute,
        }]),
    );
    assert!(
        blocked_brute.state().pending_decision().is_none(),
        "4/4 pumped Spitfire Handler must now be able to block 3/3 attacker"
    );
}

// ---------------------------------------------------------------------------
// 6. Conditional MustAttackEachCombatIfAble and multi granted_triggers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 7. PlayerTargeted trigger, crime tracking (CR 700.13), LandTappedChosenColor (CR 605.1b)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 8. World eval step (IsOwnMainPhase, IsInStep, IsInPhase) & Pass E12 stats
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 9. Exile-zone trigger discovery & ExileSubjectUntilSourceLeaves (CR 610.3)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 10. CreateTokenCopyExt enters_attacking + AttackingDefenderScope::Player + TokenSize::AtCreationPair
// ---------------------------------------------------------------------------

#[test]
fn create_token_copy_ext_enters_attacking_and_defender_scope_and_pair_pt() {
    let vocab = vocabulary();
    let soldier = vocab.resolve_label("Soldier").unwrap();

    // Part A: Attacker with attack trigger creating an attacking token copy scoped to Player(DefendingPlayer).
    let mut raider = vanilla_creature(3, 2);
    raider.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_attacks(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokenCopyExt {
                source: MoveSubjectRef::AbilitySource,
                count: ValueExpr::literal(1),
                recipients: TokenRecipients::Controller,
                enters_tapped: true,
                enters_attacking: true,
                haste: false,
                sacrifice_at_end_step: false,
                exile_at_end_step: false,
                nonlegendary: false,
                extra_types: BTreeSet::new(),
                extra_subtypes: BTreeSet::new(),
                extra_keywords: BTreeSet::new(),
                override_pt: None,
                extra_colors: BTreeSet::new(),
                defender_scope: AttackingDefenderScope::Player(ResolvingPlayer::DefendingPlayer),
                override_colors: None,
                strip_mana_cost: false,
            }],
        ),
    });
    let raider = validated(raider, &vocab);

    let mut opp_pw = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    opp_pw.printed_loyalty = Some(3.into());
    opp_pw
        .planeswalker_subtypes
        .insert(PlaneswalkerSubtype::Ajani);
    let opp_pw = validated(opp_pw, &vocab);

    // Part B: TokenSize::AtCreationPair creates asymmetric X/1 creature token.
    let token_template = TokenDefinition::new(
        Program::creature(None, 0.into(), 0.into(), BTreeSet::from([soldier])),
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(soldier)],
        },
        &vocab,
    )
    .unwrap();
    let mut pair_creation = TokenCreation::one(token_template);
    pair_creation.base_size = Some(TokenSize::AtCreationPair {
        power: Box::new(ValueExpr::controlled_type(CardType::Land)),
        toughness: Box::new(ValueExpr::literal(1)),
    });
    let make_pair_token = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![pair_creation],
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, raider, true),
            opp_battlefield(2, opp_pw),
            card(3, make_pair_token, false),
            card(4, validated(basic_forest(), &vocab), true),
            card(5, validated(basic_forest(), &vocab), true),
            card(6, validated(basic_forest(), &vocab), true),
        ],
        vec![CardId(1), CardId(2), CardId(4), CardId(5), CardId(6)],
    );

    // Part B: Cast make_pair_token with 3 controlled Forests -> creates 3/1 Soldier token!
    let after_pair = cast_untargeted(&game, 3);
    let pair_token_id = after_pair
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.zone() == ObjectZone::Battlefield && c.card_id().is_none())
        .expect("pair token must be on battlefield")
        .id();
    assert_eq!(
        current(after_pair.state(), pair_token_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 1),
        "TokenSize::AtCreationPair must create an asymmetric 3/1 token"
    );

    // Part A: Attack P1 with raider -> trigger resolves -> creates tapped ATTACKING token copy!
    let raider_id = object(after_pair.state(), 1);
    let decl_atk = advance_to_declare_attackers(after_pair.state());
    let after_atk = reply(
        &decl_atk,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(raider_id, P1)]),
    );
    let after_copy = pass_pair(after_atk.state());
    assert!(
        after_copy.state().pending_decision().is_none(),
        "AttackingDefenderScope::Player(DefendingPlayer) must restrict to P1 and exclude P1's planeswalker"
    );

    let copy_token_id = after_copy
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| {
            c.zone() == ObjectZone::Battlefield && c.card_id().is_none() && c.id() != pair_token_id
        })
        .expect("token copy must be on battlefield")
        .id();

    let combat = after_copy
        .state()
        .execution()
        .combat()
        .expect("must still be in combat");
    let copy_attacker = combat
        .attackers()
        .find(|a| a.combatant().object() == copy_token_id)
        .expect("CreateTokenCopyExt with enters_attacking: true MUST enter attacking");
    assert_eq!(copy_attacker.defender(), P1);
    assert_eq!(copy_attacker.defending_planeswalker(), None);

    // Unblocked combat damage: raider (3) + attacking token copy (3) = 6 damage to P1 (20 -> 14)!
    // P1 has no blockers, so DeclareBlockers has no blocking decision and passes straight into CombatDamage.
    let decl_blk = pass_pair(after_copy.state());
    let after_blk = if decl_blk.state().pending_decision().is_some() {
        reply(decl_blk.state(), P1, DecisionReply::DeclareBlockers(vec![]))
    } else {
        decl_blk
    };
    let after_dmg = pass_pair(after_blk.state());
    assert_eq!(
        after_dmg.state().execution().life_total(P1),
        Some(&14.into()),
        "raider (3) + attacking token copy (3) must deal 6 total combat damage"
    );
}

// ---------------------------------------------------------------------------
// 11. Autumn's Veil: CantBeCounteredByColors & CannotBeTargetedBySpellsOfColors
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 12. OfferAnyPlayer: all-decline resets last_action_succeeded & unpayable cost guard
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 13. Deflecting Palm followup, DrawGame, and TurnFaceUpSubject
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 14. FirstTargetController and validation fixes (BranchIfFalse + EmitReflexiveTrigger)
// ---------------------------------------------------------------------------
