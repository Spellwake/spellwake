#![forbid(unsafe_code)]
//! Runtime and validation integration tests for Pass F2-B Wave 8B engine additions:
//! - `Instruction::MoveCounters` (direct 1-target move, interactive `any_number` `CounterRemoval`,
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
//! - `DamagePreventFollowup::DealPreventedToSourceController`, `Instruction::DrawGame`, and
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
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(amount),
            }],
        ),
    )
}

fn damage_any_instant(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleAnyTarget,
            vec![Instruction::Damage {
                amount: amount.into(),
            }],
        ),
    )
}

fn destroy_permanent_instant() -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::DestroyTarget],
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
// 1. MoveCounters: direct move, interactive any_number CounterRemoval, CR 122.5
// ---------------------------------------------------------------------------

#[test]
fn move_counters_direct_and_interactive_counter_removal() {
    let vocab = vocabulary();

    let mut fluxmage = vanilla_creature(0, 1);
    fluxmage.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::MoveCounters {
                from: MoveSubjectRef::AbilitySource,
                to: MoveSubjectRef::TargetObject,
                kind: Some(CounterKind::PlusOne),
                all: false,
                any_number: false,
                amount: Some(ValueExpr::literal(1)),
            }],
        ),
    });
    let fluxmage = validated(fluxmage, &vocab);

    let bioshift = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::RememberPermanentTarget],
                    ))),
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::RememberPermanentTarget],
                    ))),
                    Instruction::MoveCounters {
                        from: MoveSubjectRef::RememberedFirst,
                        to: MoveSubjectRef::RememberedSecond,
                        kind: Some(CounterKind::PlusOne),
                        all: false,
                        any_number: true,
                        amount: None,
                    },
                ],
            ),
        ),
        &vocab,
    );

    let put_counters = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::PutCountersOnSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: CounterKind::PlusOne,
                    amount: ValueExpr::literal(3),
                }],
            ),
        ),
        &vocab,
    );

    let bear = validated(vanilla_creature(2, 2), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, fluxmage, true),
            card(2, bear.clone(), true),
            card(3, bear, true),
            card(4, put_counters.clone(), false),
            card(5, put_counters, false),
            card(6, bioshift.clone(), false),
            card(7, bioshift, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let mage_id = object(&game, 1);
    let bear_a = object(&game, 2);
    let bear_b = object(&game, 3);

    // Seed 3 +1/+1 counters on fluxmage and 3 +1/+1 counters on bear_a.
    let s1 = cast_targeted(&game, P0, 4, TargetRef::Object(mage_id));
    let s2 = cast_targeted(s1.state(), P0, 5, TargetRef::Object(bear_a));
    assert_eq!(plus_one_counters(s2.state(), mage_id), 3);
    assert_eq!(plus_one_counters(s2.state(), bear_a), 3);
    assert_eq!(plus_one_counters(s2.state(), bear_b), 0);

    // Part A: Direct move (Simic Fluxmage style) moves 1 +1/+1 counter without prompting.
    let act = submit(
        s2.state(),
        P0,
        GameCommand::ActivateAbility {
            source: mage_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(bear_b),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_direct = pass_pair(act.state());
    assert!(after_direct.state().pending_decision().is_none());
    assert_eq!(plus_one_counters(after_direct.state(), mage_id), 2);
    assert_eq!(plus_one_counters(after_direct.state(), bear_b), 1);
    assert_eq!(
        current(after_direct.state(), bear_b)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );

    // Part B: Interactive Bioshift (any_number: true) suspends on CounterRemoval and moves chosen 2.
    let suspended = cast_fixed_targets(
        after_direct.state(),
        P0,
        6,
        vec![TargetRef::Object(bear_a), TargetRef::Object(bear_b)],
    );
    let pending = suspended
        .state()
        .pending_decision()
        .expect("any_number MoveCounters must suspend on CounterRemoval");
    assert_eq!(pending.chooser(), P0);
    let RuleFrame::CounterRemoval(frame) = pending.frame() else {
        panic!("expected CounterRemoval frame, got {:?}", pending.frame());
    };
    assert_eq!(frame.object(), bear_a);
    assert_eq!(frame.minimum(), 0);
    assert_eq!(frame.maximum(), 3);
    assert_eq!(frame.domain().get(&CounterKind::PlusOne), Some(&3));

    let after_bioshift = reply(
        suspended.state(),
        P0,
        DecisionReply::RemoveCounters(BTreeMap::from([(CounterKind::PlusOne, 2)])),
    );
    assert!(after_bioshift.state().pending_decision().is_none());
    assert_eq!(plus_one_counters(after_bioshift.state(), bear_a), 1);
    assert_eq!(plus_one_counters(after_bioshift.state(), bear_b), 3);

    // Part C: CR 122.5 no-op when from == to (no CounterRemoval prompt, counters unchanged).
    let after_noop = cast_fixed_targets(
        after_bioshift.state(),
        P0,
        7,
        vec![TargetRef::Object(bear_b), TargetRef::Object(bear_b)],
    );
    assert!(
        after_noop.state().pending_decision().is_none(),
        "CR 122.5: moving counters from a permanent to itself must be a no-op"
    );
    assert_eq!(plus_one_counters(after_noop.state(), bear_b), 3);
}

// ---------------------------------------------------------------------------
// 2. Uncastable mana cost spell (CR 118.6): hand cast rejected, Suspend & Cascade succeed
// ---------------------------------------------------------------------------

#[test]
fn uncastable_mana_cost_spell_cr_118_6() {
    let vocab = vocabulary();

    let mut living_spell = Program::new(
        BTreeSet::from([CardType::Sorcery]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Spell(SpellDefinition {
            cost: ManaCost::free(),
            uncastable_mana_cost: true,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(4),
                }],
            ),
            modes: None,
        }),
    );
    living_spell.exile_action = Some(ExileAction::Suspend {
        price: ManaCost::free(),
        counters: 1,
    });
    let living_spell = validated(living_spell, &vocab);

    let mut cascader = Program::sorcery(
        ManaCost::generic(1.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    );
    cascader
        .triggered
        .push(TriggeredAbility::cascade(TriggeredAbilityId(1)));
    let cascader = validated(cascader, &vocab);

    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
    });
    let rock = validated(rock, &vocab);

    let filler = validated(basic_forest(), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, living_spell.clone(), false),
            card(2, cascader, false),
            card(3, rock, true),
            library_top(10, P0, filler.clone()),
            library_top(11, P0, living_spell),
            library_top(12, P0, filler.clone()),
            library_top(13, P0, filler.clone()),
            library_top(20, P1, filler.clone()),
            library_top(21, P1, filler),
        ],
        vec![CardId(3)],
    );

    // Normal cast from hand must be rejected per CR 118.6.
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::NotCastableSpell))
    );

    // Suspend from hand exiles card 1 with 1 time counter.
    let suspended = submit(
        &game,
        P0,
        GameCommand::Suspend {
            source: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(zone_of(suspended.state(), 1), ObjectZone::Exile);

    // Advance through Turn 1 End/Cleanup -> Turn 2 (P1) -> Turn 3 (P0 Upkeep).
    let turn1_end = reach_end(suspended.state(), [P0, P1]);
    let turn2_upkeep = pass_pair(&turn1_end).into_state();
    assert_eq!(turn2_upkeep.turn().unwrap().position().number, 2);
    let turn2_draw = pass_in_order(&turn2_upkeep, [P1, P0]).into_state();
    let turn2_main = pass_in_order(&turn2_draw, [P1, P0]).into_state();
    let turn2_end = reach_end(&turn2_main, [P1, P0]);
    let turn3_upkeep = pass_in_order(&turn2_end, [P1, P0]).into_state();
    assert_eq!(turn3_upkeep.turn().unwrap().position().number, 3);
    assert_eq!(turn3_upkeep.turn().unwrap().position().step, Step::Upkeep);

    // Suspend upkeep trigger removes last time counter -> SuspendLastCounter triggers.
    let after_upkeep_trig = pass_pair(&turn3_upkeep);
    assert_eq!(
        after_upkeep_trig
            .state()
            .execution()
            .window()
            .entries()
            .len(),
        1,
        "removing last time counter must stack SuspendLastCounter trigger"
    );
    let offer_suspend = pass_pair(after_upkeep_trig.state());
    assert!(matches!(
        offer_suspend.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(_))
    ));
    let cast_from_suspend = reply(
        offer_suspend.state(),
        P0,
        DecisionReply::SuspendCast {
            target: None,
            payment: CastingPayment::default(),
        },
    );
    let resolved_suspend = pass_pair(cast_from_suspend.state());
    assert_eq!(
        resolved_suspend.state().execution().life_total(P0),
        Some(&24.into()),
        "uncastable spell cast via Suspend must resolve and gain 4 life"
    );

    // Advance to Turn 3 PrecombatMain and cast cascader (MV 1) -> Cascade finds uncastable MV 0 spell (card 11).
    let turn3_draw = pass_pair(resolved_suspend.state()).into_state();
    let turn3_main = pass_pair(&turn3_draw).into_state();
    assert_eq!(
        turn3_main.turn().unwrap().position().step,
        Step::PrecombatMain
    );

    let tapped_rock = submit(
        &turn3_main,
        P0,
        GameCommand::ActivateMana {
            source: object(&turn3_main, 3),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_cascade = submit(
        tapped_rock.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(tapped_rock.state(), 2),
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 1.into(),
                }],
            }),
        },
    );
    // Resolve Cascade trigger -> offers card 11 (uncastable_mana_cost: true, MV 0 < 1).
    let cascade_offer = pass_pair(cast_cascade.state());
    let RuleFrame::OptionalEffect(opt) = cascade_offer
        .state()
        .pending_decision()
        .expect("Cascade must offer uncastable MV 0 spell")
        .frame()
    else {
        panic!("expected OptionalEffect frame for Cascade");
    };
    assert!(opt.exile_cast().is_some());

    let cast_from_cascade = reply(
        cascade_offer.state(),
        P0,
        DecisionReply::CascadeCast {
            target: None,
            payment: CastingPayment::default(),
        },
    );
    let resolved_free = pass_pair(cast_from_cascade.state());
    let resolved_cascader = pass_pair(resolved_free.state());
    assert_eq!(
        resolved_cascader.state().execution().life_total(P0),
        Some(&29.into()),
        "uncastable spell cast free via Cascade (+4) and cascader (+1) must resolve to 29 life"
    );
}

// ---------------------------------------------------------------------------
// 3. SubtypeFilter::AnyLandMask, COMMANDER_ONLY / NOT_COMMANDER, ObjectClause.owner
// ---------------------------------------------------------------------------

#[test]
fn any_land_mask_and_commander_spell_filter() {
    let vocab = vocabulary();

    let bant_mask = LandType::Plains.bit() | LandType::Island.bit() | LandType::Forest.bit();
    let mut bant_clause = ObjectClause::land();
    bant_clause.subtypes = SubtypeFilter::AnyLandMask(bant_mask);
    let bant_filter = ObjectFilter::Clause(bant_clause);

    let search_bant = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch::to_hand(SearchCardMatcher::Filtered(
                        Box::new(bant_filter),
                    ))),
                }],
            ),
        ),
        &vocab,
    );

    let tap_bant = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SetSubjectTapped {
                    subject: Box::new(EffectSubject::Matching(bant_filter)),
                    tapped: true,
                }],
            ),
        ),
        &vocab,
    );

    let own_ctrl_opp_owner = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                    ObjectClause::creature()
                        .controlled()
                        .with_owner(ControllerFilter::Opponent),
                )),
                vec![Instruction::DestroyTarget],
            ),
        ),
        &vocab,
    );

    let plains = validated(basic_land(LandType::Plains, ManaType::White), &vocab);
    let island = validated(basic_land(LandType::Island, ManaType::Blue), &vocab);
    let forest = validated(basic_land(LandType::Forest, ManaType::Green), &vocab);
    let mountain = validated(basic_land(LandType::Mountain, ManaType::Red), &vocab);
    let swamp = validated(basic_land(LandType::Swamp, ManaType::Black), &vocab);

    // Part A: AnyLandMask in LibrarySearch + battlefield ObjectFilter + ObjectClause.owner.
    let game_a = world(
        vocab.clone(),
        vec![
            card(1, search_bant, false),
            card(2, tap_bant, false),
            card(3, own_ctrl_opp_owner, false),
            // Library cards: Plains, Island, Forest (match) + Mountain, Swamp (exclude).
            library_top(10, P0, plains.clone()),
            library_top(11, P0, island.clone()),
            library_top(12, P0, forest.clone()),
            library_top(13, P0, mountain.clone()),
            library_top(14, P0, swamp.clone()),
            // Battlefield lands: Plains, Island, Forest + Mountain, Swamp.
            card(20, plains, true),
            card(21, island, true),
            card(22, forest, true),
            card(23, mountain, true),
            card(24, swamp, true),
            // Battlefield creatures for ObjectClause.owner test.
            controlled(30, P0, P0, validated(vanilla_creature(2, 2), &vocab)),
            controlled(31, P1, P0, validated(vanilla_creature(2, 2), &vocab)),
        ],
        vec![
            CardId(20),
            CardId(21),
            CardId(22),
            CardId(23),
            CardId(24),
            CardId(30),
            CardId(31),
        ],
    );

    let lib_plains = object(&game_a, 10);
    let lib_island = object(&game_a, 11);
    let lib_forest = object(&game_a, 12);
    let lib_mountain = object(&game_a, 13);
    let lib_swamp = object(&game_a, 14);

    // Library search domain matches Plains, Island, Forest and excludes Mountain, Swamp.
    let search_pending = cast_untargeted(&game_a, 1);
    let RuleFrame::LibraryTop(frame) = search_pending
        .state()
        .pending_decision()
        .expect("library search must prompt")
        .frame()
    else {
        panic!("expected LibraryTop frame");
    };
    let domain: BTreeSet<ObjectId> = frame.eligible_results().iter().copied().collect();
    assert!(domain.contains(&lib_plains));
    assert!(domain.contains(&lib_island));
    assert!(domain.contains(&lib_forest));
    assert!(!domain.contains(&lib_mountain));
    assert!(!domain.contains(&lib_swamp));

    let ready_search = search_pending
        .state()
        .supply_entropy(search_pending.state().basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let after_search = reply(
        &ready_search,
        P0,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(lib_island),
        }),
    );
    assert_eq!(zone_of(after_search.state(), 11), ObjectZone::Hand(P0));

    // Battlefield ObjectFilter with AnyLandMask taps Plains/Island/Forest and leaves Mountain/Swamp untapped.
    let after_tap = cast_untargeted(after_search.state(), 2);
    for id in [20, 21, 22] {
        assert_eq!(
            after_tap
                .state()
                .execution()
                .window()
                .cards()
                .get(object(after_tap.state(), id))
                .unwrap()
                .tapped(),
            Some(true),
            "AnyLandMask must tap land {id}"
        );
    }
    for id in [23, 24] {
        assert_eq!(
            after_tap
                .state()
                .execution()
                .window()
                .cards()
                .get(object(after_tap.state(), id))
                .unwrap()
                .tapped(),
            Some(false),
            "AnyLandMask must NOT tap non-matching land {id}"
        );
    }

    // ObjectClause.owner distinguishes owner vs controller: P0 controls both 30 (owner P0) and 31 (owner P1).
    let own_c = object(after_tap.state(), 30);
    let stolen_c = object(after_tap.state(), 31);
    assert_eq!(
        attempt(
            after_tap.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(after_tap.state(), 3),
                target: TargetRef::Object(own_c),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "creature owned and controlled by P0 must not match owner=Opponent"
    );
    let after_destroy = cast_targeted(after_tap.state(), P0, 3, TargetRef::Object(stolen_c));
    assert_eq!(
        zone_of(after_destroy.state(), 31),
        ObjectZone::Graveyard(P1),
        "creature controlled by P0 but owned by P1 must match controller=You + owner=Opponent"
    );

    // Part B: COMMANDER_ONLY and NOT_COMMANDER via SupportedAppearance.commander_owner.
    let mut watcher = named(Program::enchantment(Some(ManaCost::free())), "Cmd Watcher");
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            SpellFilter::from_object(ObjectFilter::any().commander()),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            SpellFilter::from_object(
                ObjectFilter::any().with_flags(ObjectFilterFlags::NOT_COMMANDER),
            ),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(5),
            }],
        ),
    });
    let watcher = validated(watcher, &vocab);

    let non_cmd_spell = validated(named(vanilla_creature(2, 2), "Plain Bear"), &vocab);
    let mut p0_cmd = named(vanilla_creature(3, 3), "General Alpha");
    p0_cmd.supertypes.insert(Supertype::Legendary);
    let p0_cmd = validated(p0_cmd, &vocab);
    let mut p1_cmd = named(vanilla_creature(3, 3), "General Beta");
    p1_cmd.supertypes.insert(Supertype::Legendary);
    let p1_cmd = validated(p1_cmd, &vocab);

    // Interleave watcher and non_cmd_spell across all 99 cards so opening 7 always contains both.
    let mut top = Vec::new();
    for i in 2..=100u64 {
        let prog = if i % 2 == 0 {
            watcher.clone()
        } else {
            non_cmd_spell.clone()
        };
        top.push((i, prog));
    }
    let cmd_game = commander_main_world(vocab, p0_cmd, p1_cmd, top);
    let watcher_hand_id = find_hand_card_by_name(&cmd_game, P0, "Cmd Watcher");
    let bear_hand_id = find_hand_card_by_name(&cmd_game, P0, "Plain Bear");
    let cmd_obj_id = object(&cmd_game, 1);

    // Put Cmd Watcher onto battlefield.
    let cast_w = submit(
        &cmd_game,
        P0,
        GameCommand::CastUntargeted {
            spell: watcher_hand_id,
            payment: Some(ManaPayment::default()),
        },
    );
    let after_w = pass_pair(cast_w.state());
    assert_eq!(after_w.state().execution().life_total(P0), Some(&40.into()));

    // Cast non-commander spell: ONLY NOT_COMMANDER trigger fires (+5 life -> 45).
    let cast_non_cmd = submit(
        after_w.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: bear_hand_id,
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        cast_non_cmd.state().execution().window().entries().len(),
        2,
        "non-commander cast must stack spell + exactly 1 NOT_COMMANDER trigger"
    );
    let trig_resolved = pass_pair(cast_non_cmd.state());
    let spell_resolved = pass_pair(trig_resolved.state());
    assert_eq!(
        spell_resolved.state().execution().life_total(P0),
        Some(&45.into()),
        "NOT_COMMANDER trigger (+5) must fire for non-commander spell"
    );

    // Cast commander from Command zone: ONLY COMMANDER_ONLY trigger fires (+3 life -> 48).
    let cast_cmd = submit(
        spell_resolved.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: cmd_obj_id,
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        cast_cmd.state().execution().window().entries().len(),
        2,
        "commander cast must stack spell + exactly 1 COMMANDER_ONLY trigger"
    );
    let cmd_trig_resolved = pass_pair(cast_cmd.state());
    let cmd_resolved = pass_pair(cmd_trig_resolved.state());
    assert_eq!(
        cmd_resolved.state().execution().life_total(P0),
        Some(&48.into()),
        "COMMANDER_ONLY trigger (+3) must fire for commander spell"
    );
}

// ---------------------------------------------------------------------------
// 4. GrantSpellCascade and GrantSpellKeyword { any_player: true, .. }
// ---------------------------------------------------------------------------

#[test]
fn grant_spell_cascade_and_any_player_keyword() {
    let vocab = vocabulary();

    // Part A: GrantSpellCascade on commander spells triggers cascade for commander, not non-commander.
    let mut cascade_granter = named(
        Program::enchantment(Some(ManaCost::free())),
        "Cascade Conduit",
    );
    cascade_granter.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::GrantSpellCascade {
            any_player: false,
            matching: SpellFilter::from_object(ObjectFilter::any().commander()),
            count: NonZeroU8::new(1).unwrap(),
        },
    });
    let cascade_granter = validated(cascade_granter, &vocab);

    let mut p0_cmd = named(
        Program::creature(
            Some(ManaCost::generic(2.into())),
            3.into(),
            3.into(),
            BTreeSet::new(),
        ),
        "Cascade Commander",
    );
    p0_cmd.supertypes.insert(Supertype::Legendary);
    let p0_cmd = validated(p0_cmd, &vocab);

    let mut p1_cmd = named(vanilla_creature(2, 2), "Rival Commander");
    p1_cmd.supertypes.insert(Supertype::Legendary);
    let p1_cmd = validated(p1_cmd, &vocab);

    let non_cmd_2mv = validated(
        named(
            Program::creature(
                Some(ManaCost::generic(2.into())),
                2.into(),
                2.into(),
                BTreeSet::new(),
            ),
            "Ordinary Knight",
        ),
        &vocab,
    );
    let cascade_hit = validated(named(gain_life_instant(6), "Cascade Boon"), &vocab);

    let mut mana_rock = named(Program::artifact(Some(ManaCost::free()), vec![]), "Sol Gem");
    mana_rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 4.into())])),
    });
    let mana_rock = validated(mana_rock, &vocab);

    // Interleave all 4 card kinds across the 99-card deck so the 7-card opening hand always
    // contains conduit, rock, and knight, while the library retains Cascade Boon (MV 0 < 2).
    let mut top = Vec::new();
    for i in 2..=100u64 {
        let prog = match i % 4 {
            0 => cascade_granter.clone(),
            1 => mana_rock.clone(),
            2 => non_cmd_2mv.clone(),
            _ => cascade_hit.clone(),
        };
        top.push((i, prog));
    }
    let cmd_game = commander_main_world(vocab.clone(), p0_cmd, p1_cmd, top);
    let conduit_id = find_hand_card_by_name(&cmd_game, P0, "Cascade Conduit");
    let rock_id = find_hand_card_by_name(&cmd_game, P0, "Sol Gem");
    let knight_id = find_hand_card_by_name(&cmd_game, P0, "Ordinary Knight");
    let cmd_id = object(&cmd_game, 1);

    let s1 = pass_pair(
        submit(
            &cmd_game,
            P0,
            GameCommand::CastUntargeted {
                spell: conduit_id,
                payment: Some(ManaPayment::default()),
            },
        )
        .state(),
    );
    let s2 = pass_pair(
        submit(
            s1.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: rock_id,
                payment: Some(ManaPayment::default()),
            },
        )
        .state(),
    );
    let rock_bf = s2
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| {
            c.zone() == ObjectZone::Battlefield
                && current(s2.state(), c.id())
                    .current_names
                    .iter()
                    .any(|n| n.as_str() == "Sol Gem")
        })
        .unwrap()
        .id();
    let s3 = submit(
        s2.state(),
        P0,
        GameCommand::ActivateMana {
            source: rock_bf,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );

    // Non-commander MV 2 spell does NOT trigger GrantSpellCascade (commander-only filter).
    let cast_knight = submit(
        s3.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: knight_id,
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 2.into(),
                }],
            }),
        },
    );
    assert_eq!(
        cast_knight.state().execution().window().entries().len(),
        1,
        "non-commander spell must not trigger commander-filtered GrantSpellCascade"
    );
    let after_knight = pass_pair(cast_knight.state());

    // Commander MV 2 spell DOES trigger GrantSpellCascade!
    let cast_cmd = submit(
        after_knight.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: cmd_id,
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 2.into(),
                }],
            }),
        },
    );
    assert_eq!(
        cast_cmd.state().execution().window().entries().len(),
        2,
        "commander spell must trigger GrantSpellCascade"
    );
    let offer = pass_pair(cast_cmd.state());
    assert!(matches!(
        offer.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(_))
    ));
    let cascaded = reply(
        offer.state(),
        P0,
        DecisionReply::CascadeCast {
            target: None,
            payment: CastingPayment::default(),
        },
    );
    assert_eq!(
        cascaded.state().execution().window().entries().len(),
        2,
        "CascadeCast must place the free cascaded spell onto the stack above the commander"
    );
    // Resolve cascaded spell + commander spell.
    let r1 = pass_pair(cascaded.state());
    let r2 = pass_pair(r1.state());
    assert_eq!(zone_of(r2.state(), 1), ObjectZone::Battlefield);

    // Part B: GrantSpellKeyword { any_player: true, matching: Creature, keyword: Flash }.
    let mut global_flash = Program::enchantment(Some(ManaCost::free()));
    global_flash.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::GrantSpellKeyword {
            any_player: true,
            matching: SpellFilter::creature(),
            keyword: KeywordAbility::Flash,
        },
    });
    let global_flash = validated(global_flash, &vocab);

    let opp_creature = validated(vanilla_creature(2, 2), &vocab);
    let opp_sorcery = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(2),
                }],
            ),
        ),
        &vocab,
    );

    let game_b = world(
        vocab,
        vec![
            card(1, global_flash, true),
            opp_hand(2, opp_creature),
            opp_hand(3, opp_sorcery),
        ],
        vec![CardId(1)],
    );

    // P0 passes priority on P0's own PrecombatMain so P1 gets priority on P0's turn.
    let p1_prio = submit(&game_b, P0, GameCommand::Pass);
    assert_priority(p1_prio.state(), P1);

    // P1 still cannot cast a Sorcery during P0's turn (filter is creature-only).
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
        "sorcery must remain uncastable on opponent's turn when only creatures have Flash"
    );

    // P1 CAN cast a creature spell during P0's turn thanks to any_player: true Flash!
    let opp_flash_cast = submit(
        p1_prio.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let opp_entered = pass_in_order(opp_flash_cast.state(), [P1, P0]);
    assert_eq!(zone_of(opp_entered.state(), 2), ObjectZone::Battlefield);
}

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

#[test]
fn conditional_must_attack_each_combat_and_multi_granted_triggers() {
    let vocab = vocabulary();

    // Part A: MustAttackEachCombatIfAble gated by StaticSourceCondition::ControllerLifeAtLeast(20).
    let mut berserker = vanilla_creature(2, 2);
    let mut cond_sel = self_selector();
    cond_sel.source_condition = Some(StaticSourceCondition::ControllerLifeAtLeast(20));
    berserker.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: cond_sel,
        parts: StaticParts(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::MustAttackEachCombatIfAble,
        )]),
    });
    let berserker = validated(berserker, &vocab);

    let lose_life = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PayLife {
                    player: ResolvingPlayer::Controller,
                    amount: ValueExpr::literal(2),
                }],
            ),
        ),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![card(1, berserker, true), card(2, lose_life, false)],
        vec![CardId(1)],
    );
    let berserker_id = object(&game_a, 1);

    // At 20 life (condition TRUE), declining to attack must be rejected.
    let decl_forced = advance_to_declare_attackers(&game_a);
    let atk_dec = decl_forced.pending_decision().unwrap();
    assert!(matches!(
        attempt(
            &decl_forced,
            P0,
            GameCommand::Reply {
                decision: atk_dec.id(),
                choice: DecisionReply::DeclareAttackers(vec![]),
            },
        ),
        Err(GameError::Effect(EffectError::Combat(
            CombatError::IllegalAttacker
        )))
    ));
    // Declaring berserker as attacker succeeds.
    let attacked = reply(
        &decl_forced,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(berserker_id, P1)]),
    );
    assert!(attacked.state().pending_decision().is_none());

    // Branch from game_a: P0 loses 2 life (now 18 < 20, condition FALSE) -> empty attack succeeds!
    let low_life = cast_untargeted(&game_a, 2).into_state();
    assert_eq!(low_life.execution().life_total(P0), Some(&18.into()));
    let decl_free = advance_to_declare_attackers(&low_life);
    let not_forced = reply(&decl_free, P0, DecisionReply::DeclareAttackers(vec![]));
    assert!(not_forced.state().pending_decision().is_none());

    // Part B: AbilityChange::granted_triggers with 2 triggers fires both independently.
    let trig_a = TriggeredAbility {
        id: TriggeredAbilityId(10),
        condition: TriggerSpec::self_attacks(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(2),
            }],
        ),
    };
    let trig_b = TriggeredAbility {
        id: TriggeredAbilityId(11),
        condition: TriggerSpec::self_attacks(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustPlayersLife {
                players: AffectedPlayers::EachOpponent,
                gain: false,
                amount: ValueExpr::literal(3),
            }],
        ),
    };
    let mut granter = Program::enchantment(Some(ManaCost::free()));
    granter.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: controlled_creatures_selector(),
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            granted_triggers: vec![trig_a, trig_b],
            ..AbilityChange::default()
        })]),
    });
    let granter = validated(granter, &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, validated(vanilla_creature(2, 2), &vocab), true),
            card(2, granter, true),
        ],
        vec![CardId(1), CardId(2)],
    );
    let attacker_id = object(&game_b, 1);
    let decl_b = advance_to_declare_attackers(&game_b);
    let after_decl = reply(
        &decl_b,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(attacker_id, P1)]),
    );

    // Two simultaneous triggers -> order triggers frame with 2 distinct instances!
    let pending_trigs = after_decl
        .state()
        .pending_decision()
        .expect("two granted attack triggers must prompt for trigger order");
    let RuleFrame::Triggers(order_frame) = pending_trigs.frame() else {
        panic!("expected Triggers frame, got {:?}", pending_trigs.frame());
    };
    let instance_ids = order_frame.instances().to_vec();
    assert_eq!(
        instance_ids.len(),
        2,
        "both granted triggers must fire independently without key collision"
    );

    let ordered = reply(
        after_decl.state(),
        P0,
        DecisionReply::OrderTriggers(instance_ids),
    );
    let r1 = pass_pair(ordered.state());
    let r2 = pass_pair(r1.state());
    assert_eq!(r2.state().execution().life_total(P0), Some(&22.into()));
    assert_eq!(r2.state().execution().life_total(P1), Some(&17.into()));
}

// ---------------------------------------------------------------------------
// 7. PlayerTargeted trigger, crime tracking (CR 700.13), LandTappedChosenColor (CR 605.1b)
// ---------------------------------------------------------------------------

#[test]
fn player_targeted_trigger_and_crime_and_land_tapped_chosen_color() {
    let vocab = vocabulary();

    // Part A: PlayerTargeted trigger + CommittedCrimeThisTurn.
    let mut opp_watcher = Program::enchantment(Some(ManaCost::free()));
    opp_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::PlayerTargeted {
            player: PlayerRelation::Controller,
            by_opponent: true,
            spell_only: true,
            instant_or_sorcery_only: false,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(4),
            }],
        ),
    });
    let opp_watcher = validated(opp_watcher, &vocab);

    let mut crime_buff = vanilla_creature(2, 2);
    let mut crime_sel = self_selector();
    crime_sel.source_condition = Some(StaticSourceCondition::CommittedCrimeThisTurn);
    crime_buff.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: crime_sel,
        parts: StaticParts(vec![StaticPart::ModifyPT(num_pair(2, 2))]),
    });
    let crime_buff = validated(crime_buff, &vocab);

    let ping = validated(damage_any_instant(1), &vocab);

    // Part B: LandTappedChosenColor (CR 605.1b triggered mana ability).
    let mut gauntlet = Program::new(
        BTreeSet::from([CardType::Artifact]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: Some(EntryTap::ChooseColor),
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    gauntlet.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Triggered(TriggeredManaRule::LandTappedChosenColor {
            controller_only: true,
        }),
    });
    let gauntlet = validated(gauntlet, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, crime_buff, true),
            opp_battlefield(2, opp_watcher),
            card(3, ping.clone(), false),
            card(4, ping, false),
            card(5, gauntlet, false),
            card(6, validated(basic_forest(), &vocab), true),
            card(
                7,
                validated(basic_land(LandType::Island, ManaType::Blue), &vocab),
                true,
            ),
            opp_battlefield(8, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1), CardId(2), CardId(6), CardId(7), CardId(8)],
    );

    let crime_id = object(&game, 1);
    assert_eq!(
        current(&game, crime_id).power_toughness.unwrap().derived,
        pt(2, 2),
        "before targeting an opponent, no crime has been committed"
    );

    // Targeting self (P0) does NOT commit a crime and does NOT fire P1's PlayerTargeted trigger.
    let ping_self = cast_targeted(&game, P0, 3, TargetRef::Player(P0));
    assert_eq!(
        ping_self.state().execution().life_total(P0),
        Some(&19.into())
    );
    assert_eq!(
        ping_self.state().execution().life_total(P1),
        Some(&20.into())
    );
    assert_eq!(
        current(ping_self.state(), crime_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );

    // Targeting opponent P1 fires P1's PlayerTargeted trigger AND commits a crime for P0!
    let cast_ping_opp = submit(
        ping_self.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(ping_self.state(), 4),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    );
    assert_eq!(
        cast_ping_opp.state().execution().window().entries().len(),
        2,
        "targeting P1 must stack spell + P1's PlayerTargeted trigger"
    );
    assert_eq!(
        current(cast_ping_opp.state(), crime_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4),
        "CR 700.13: targeting an opponent must immediately record a crime for P0"
    );
    let after_trig = pass_pair(cast_ping_opp.state());
    let after_ping = pass_pair(after_trig.state());
    assert_eq!(
        after_ping.state().execution().life_total(P1),
        Some(&23.into()),
        "P1 gains 4 from PlayerTargeted trigger then takes 1 from ping -> 23"
    );

    // Part B: Cast Gauntlet, choose Green, tap Forest -> immediate +2 Green with empty stack!
    let gauntlet_prompt = cast_untargeted(after_ping.state(), 5);
    let after_gauntlet = reply(
        gauntlet_prompt.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::Color(ManaType::Green)),
    );

    let tapped_forest = submit(
        after_gauntlet.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_gauntlet.state(), 6),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(
        tapped_forest
            .state()
            .execution()
            .window()
            .entries()
            .is_empty(),
        "CR 605.1b: LandTappedChosenColor triggered mana ability must not use the stack"
    );
    let after_triggered_mana = if let Some(decision) = tapped_forest.state().pending_decision() {
        let RuleFrame::ManaColor(choice) = decision.frame() else {
            panic!(
                "expected stackless ManaColor frame, got {:?}",
                decision.frame()
            );
        };
        assert_eq!(choice.colors(), &BTreeSet::from([ManaType::Green]));
        reply(
            tapped_forest.state(),
            P0,
            DecisionReply::ManaColor(ManaType::Green),
        )
    } else {
        tapped_forest
    };
    assert!(
        after_triggered_mana
            .state()
            .execution()
            .window()
            .entries()
            .is_empty(),
        "triggered mana resolution must remain stackless"
    );
    assert_eq!(
        after_triggered_mana
            .state()
            .execution()
            .mana(P0, ManaType::Green),
        2.into(),
        "tapping Forest for chosen color Green must yield 2 Green mana immediately"
    );

    // Tapping Island (Blue != chosen Green) yields only 1 Blue and no extra Green.
    let tapped_island = submit(
        after_triggered_mana.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_triggered_mana.state(), 7),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(tapped_island.state().pending_decision().is_none());
    assert_eq!(
        tapped_island.state().execution().mana(P0, ManaType::Blue),
        1.into()
    );
    assert_eq!(
        tapped_island.state().execution().mana(P0, ManaType::Green),
        2.into()
    );

    // Opponent tapping Forest yields only 1 Green for P1 because controller_only: true.
    let p1_prio = submit(tapped_island.state(), P0, GameCommand::Pass);
    let opp_tapped = submit(
        p1_prio.state(),
        P1,
        GameCommand::ActivateMana {
            source: object(p1_prio.state(), 8),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(opp_tapped.state().pending_decision().is_none());
    assert_eq!(
        opp_tapped.state().execution().mana(P1, ManaType::Green),
        1.into()
    );
}

// ---------------------------------------------------------------------------
// 8. World eval step (IsOwnMainPhase, IsInStep, IsInPhase) & Pass E12 stats
// ---------------------------------------------------------------------------

#[test]
fn world_eval_step_is_own_main_phase_and_pass_e12_player_stats() {
    let vocab = vocabulary();
    let elf = vocab.resolve_label("Elf").unwrap();
    let goblin = vocab.resolve_label("Goblin").unwrap();

    let stat_buff_creature = |expr: PlayerStatExpr, bonus: i64| -> Program {
        let mut c = vanilla_creature(1, 1);
        let mut sel = self_selector();
        sel.source_condition = Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
            left: ValueExpr::PlayerStat(expr),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        })));
        c.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: sel,
            parts: StaticParts(vec![StaticPart::ModifyPT(num_pair(bonus, bonus))]),
        });
        validated(c, &vocab)
    };

    let own_main_c = stat_buff_creature(PlayerStatExpr::IsOwnMainPhase, 3);
    let in_upkeep_c = stat_buff_creature(
        PlayerStatExpr::IsInStep {
            step: Step::Upkeep,
            player: Some(ResolvingPlayer::Controller),
        },
        2,
    );
    let in_beginning_c = stat_buff_creature(
        PlayerStatExpr::IsInPhase {
            phase: PhaseCheck::Beginning,
            player: Some(ResolvingPlayer::Controller),
        },
        1,
    );
    let most_life_strict = stat_buff_creature(
        PlayerStatExpr::PlayerHasMostLife {
            player: ResolvingPlayer::Controller,
            allow_tie: false,
        },
        4,
    );
    let most_life_tie = stat_buff_creature(
        PlayerStatExpr::PlayerHasMostLife {
            player: ResolvingPlayer::Controller,
            allow_tie: true,
        },
        4,
    );
    let controls_most_strict = stat_buff_creature(
        PlayerStatExpr::PlayerControlsMost {
            player: ResolvingPlayer::Controller,
            zone: ValueZone::Battlefield,
            filter: Box::new(ObjectFilter::creature()),
            allow_tie: false,
        },
        5,
    );

    let elf_attacker = validated(
        Program::creature(
            Some(ManaCost::free()),
            1.into(),
            1.into(),
            BTreeSet::from([elf]),
        ),
        &vocab,
    );
    let check_attacked_elf = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::creature().with_subtype(elf)),
                    }),
                }],
            ),
        ),
        &vocab,
    );
    let check_attacked_goblin = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::creature().with_subtype(goblin)),
                    }),
                }],
            ),
        ),
        &vocab,
    );

    let upkeep = world_at_upkeep(
        vocab.clone(),
        vec![
            card(1, own_main_c, true),
            card(2, in_upkeep_c, true),
            card(3, in_beginning_c, true),
            card(4, most_life_strict, true),
            card(5, most_life_tie, true),
            card(6, controls_most_strict, true),
            card(7, elf_attacker, true),
            opp_battlefield(8, validated(vanilla_creature(3, 3), &vocab)),
            card(9, validated(gain_life_instant(1), &vocab), false),
            card(10, check_attacked_elf, false),
            card(11, check_attacked_goblin, false),
        ],
        vec![
            CardId(1),
            CardId(2),
            CardId(3),
            CardId(4),
            CardId(5),
            CardId(6),
            CardId(7),
            CardId(8),
        ],
    );

    // During Upkeep: IsOwnMainPhase is FALSE (1/1), IsInStep(Upkeep) is TRUE (3/3), IsInPhase(Beginning) is TRUE (2/2).
    assert_eq!(
        current(&upkeep, object(&upkeep, 1))
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1),
        "IsOwnMainPhase must be FALSE during Upkeep"
    );
    assert_eq!(
        current(&upkeep, object(&upkeep, 2))
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3),
        "IsInStep(Upkeep) must be TRUE during Upkeep"
    );
    assert_eq!(
        current(&upkeep, object(&upkeep, 3))
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2),
        "IsInPhase(Beginning) must be TRUE during Upkeep"
    );

    // Tied 20-20 life: strict most-life is FALSE (1/1), allow_tie is TRUE (5/5).
    assert_eq!(
        current(&upkeep, object(&upkeep, 4))
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );
    assert_eq!(
        current(&upkeep, object(&upkeep, 5))
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5)
    );
    // P0 controls 7 creatures vs P1's 1 creature -> PlayerControlsMost strict is TRUE (6/6).
    assert_eq!(
        current(&upkeep, object(&upkeep, 6))
            .power_toughness
            .unwrap()
            .derived,
        pt(6, 6)
    );

    // Advance to PrecombatMain: IsOwnMainPhase flips TRUE (4/4), Upkeep & Beginning flip FALSE (1/1).
    let main = pass_pair(&upkeep).into_state();
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    assert_eq!(
        current(&main, object(&main, 1))
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4),
        "IsOwnMainPhase must be TRUE during PrecombatMain"
    );
    assert_eq!(
        current(&main, object(&main, 2))
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );
    assert_eq!(
        current(&main, object(&main, 3))
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );

    // P0 gains 1 life (21 vs 20) -> strict PlayerHasMostLife flips TRUE (5/5).
    let gained = cast_untargeted(&main, 9).into_state();
    assert_eq!(
        current(&gained, object(&gained, 4))
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5)
    );

    // Attack with 1/1 Elf (card 7), blocked and killed by P1's 3/3 (card 8).
    let elf_id = object(&gained, 7);
    let blocker_id = object(&gained, 8);
    let decl_atk = advance_to_declare_attackers(&gained);
    let after_atk = reply(
        &decl_atk,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(elf_id, P1)]),
    );
    let decl_blk = pass_pair(after_atk.state());
    let after_blk = reply(
        decl_blk.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: blocker_id,
            attacker: elf_id,
        }]),
    );
    // CombatDamage resolves -> 1/1 Elf dies to 3/3 blocker.
    let combat_dmg = pass_pair(after_blk.state());
    assert_eq!(
        zone_of(combat_dmg.state(), 7),
        ObjectZone::Graveyard(P0),
        "1/1 attacking Elf must die in combat"
    );
    let end_combat = pass_pair(combat_dmg.state());
    let post_main = pass_pair(end_combat.state());
    assert_eq!(
        post_main.state().turn().unwrap().position().step,
        Step::PostcombatMain
    );

    // Even after the Elf died, AttackedMatchingThisTurn(Elf) == 1 and Goblin == 0!
    let after_elf_check = cast_untargeted(post_main.state(), 10);
    assert_eq!(
        after_elf_check.state().execution().life_total(P0),
        Some(&22.into()),
        "AttackedMatchingThisTurn must remain 1 even after the attacking Elf died"
    );
    let after_goblin_check = cast_untargeted(after_elf_check.state(), 11);
    assert_eq!(
        after_goblin_check.state().execution().life_total(P0),
        Some(&22.into()),
        "AttackedMatchingThisTurn must return 0 for non-matching Goblin subtype"
    );
}

// ---------------------------------------------------------------------------
// 9. Exile-zone trigger discovery & ExileSubjectUntilSourceLeaves (CR 610.3)
// ---------------------------------------------------------------------------

#[test]
fn exile_zone_trigger_discovery_and_exile_subject_until_leaves_cr_610_3() {
    let vocab = vocabulary();

    // Part A: Card sitting in Exile with TriggerZone::Any triggers on battlefield event.
    let mut exile_watcher = Program::enchantment(Some(ManaCost::free()));
    exile_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::enters(ObjectFilter::creature().controlled())
            .in_zone(TriggerZone::Any),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    let exile_watcher = validated(exile_watcher, &vocab);

    // Part B: ExileSubjectUntilSourceLeaves + CR 610.3.
    let mut banisher = Program::enchantment(Some(ManaCost::free()));
    banisher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::Matching(ObjectFilter::creature().not_controlled()),
                    )),
                    spellwake_engine::toolbox::ZoneDest::Exile {
                        face_down: false,
                        viewer: None,
                        link_to_source: true,
                        delayed_return: Some(
                            spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves,
                        ),
                        link_group: None,
                    },
                ),
            ))],
        ),
    });
    let banisher = validated(banisher, &vocab);

    let destroy = validated(destroy_permanent_instant(), &vocab);
    let bear = validated(vanilla_creature(2, 2), &vocab);

    let game = world(
        vocab,
        vec![
            exile_of(1, P0, exile_watcher),
            card(2, bear.clone(), false),
            card(3, banisher.clone(), false),
            card(4, banisher, false),
            card(5, destroy.clone(), false),
            card(6, destroy, false),
            opp_battlefield(10, bear.clone()),
            opp_battlefield(11, bear),
        ],
        vec![CardId(10), CardId(11)],
    );

    // Part A: Casting bear enters battlefield -> card 1 in Exile triggers!
    let entered_bear = cast_untargeted(&game, 2);
    assert_eq!(
        entered_bear.state().execution().window().entries().len(),
        1,
        "trigger on card in Exile must be discovered and stacked"
    );
    let after_exile_trig = pass_pair(entered_bear.state());
    assert_eq!(
        after_exile_trig.state().execution().life_total(P0),
        Some(&23.into()),
        "Exile-zone trigger must resolve and gain 3 life"
    );

    // Part B1: Normal ExileSubjectUntilSourceLeaves exiles both opponent creatures and returns them on LTB.
    let entered_b1 = cast_untargeted(after_exile_trig.state(), 3);
    let b1_id = object(entered_b1.state(), 3);
    let after_exile = pass_pair(entered_b1.state());
    assert_eq!(zone_of(after_exile.state(), 10), ObjectZone::Exile);
    assert_eq!(zone_of(after_exile.state(), 11), ObjectZone::Exile);

    // Destroy banisher #1 -> both exiled creatures return to battlefield under P1's control.
    let destroyed_b1 = cast_targeted(after_exile.state(), P0, 5, TargetRef::Object(b1_id));
    let after_return = if let Some(decision) = destroyed_b1.state().pending_decision() {
        let RuleFrame::LinkedReturnOrder(frame) = decision.frame() else {
            panic!("unexpected decision on return: {:?}", decision.frame());
        };
        reply(
            destroyed_b1.state(),
            decision.chooser(),
            DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
        )
    } else {
        destroyed_b1
    };
    assert_eq!(zone_of(after_return.state(), 10), ObjectZone::Battlefield);
    assert_eq!(zone_of(after_return.state(), 11), ObjectZone::Battlefield);

    // Part B2 (CR 610.3): Banisher #2 enters, trigger stacks, then Banisher #2 is destroyed before
    // the ETB trigger resolves -> nothing is exiled!
    let entered_b2 = cast_untargeted(after_return.state(), 4);
    let b2_id = object(entered_b2.state(), 4);
    assert_eq!(entered_b2.state().execution().window().entries().len(), 1);
    let killed_b2 = cast_targeted(entered_b2.state(), P0, 6, TargetRef::Object(b2_id));
    assert_eq!(zone_of(killed_b2.state(), 4), ObjectZone::Graveyard(P0));
    let resolved_b2_etb = pass_pair(killed_b2.state());
    assert_eq!(
        zone_of(resolved_b2_etb.state(), 10),
        ObjectZone::Battlefield,
        "CR 610.3: ExileSubjectUntilSourceLeaves must exile nothing when source already left"
    );
    assert_eq!(
        zone_of(resolved_b2_etb.state(), 11),
        ObjectZone::Battlefield,
        "CR 610.3: ExileSubjectUntilSourceLeaves must exile nothing when source already left"
    );
}

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

#[test]
fn autumn_veil_cant_be_countered_by_colors_and_cant_be_targeted_by_spells_of_colors() {
    let vocab = vocabulary();

    let ub_mask = ColorMask::BLUE.union(ColorMask::BLACK);
    let autumn_veil = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::CantBeCounteredByColors {
                            colors: ub_mask,
                            filter: None,
                            controller_only: true,
                            next_only: false,
                        }),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                    Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::Matching(
                            ObjectFilter::creature().controlled(),
                        )),
                        parts: ResolvedPartsSpec(vec![
                            ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(ub_mask),
                        ]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                ],
            ),
        ),
        &vocab,
    );

    let counterspell = |color: Color| -> Program {
        validated(
            with_color(
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::SingleSpell(SpellFilter::any()),
                        vec![Instruction::CounterSpell],
                    ),
                ),
                color,
            ),
            &vocab,
        )
    };

    let blue_kill_spell = validated(with_color(destroy_permanent_instant(), Color::Blue), &vocab);
    let mut blue_pinger = with_color(vanilla_creature(1, 1), Color::Blue);
    blue_pinger.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
    });
    let blue_pinger = validated(blue_pinger, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, autumn_veil, false),
            card(2, validated(vanilla_creature(2, 2), &vocab), true),
            card(3, validated(vanilla_creature(3, 3), &vocab), false),
            card(4, validated(vanilla_creature(3, 3), &vocab), false),
            opp_hand(5, counterspell(Color::Blue)),
            opp_hand(6, counterspell(Color::Red)),
            opp_hand(7, blue_kill_spell),
            opp_battlefield(8, blue_pinger),
        ],
        vec![CardId(2), CardId(8)],
    );

    let shielded_bear = object(&game, 2);
    let pinger_id = object(&game, 8);

    // Resolve Autumn's Veil.
    let veiled = cast_untargeted(&game, 1);

    // Part A1: P0 casts creature #3; P1 responds with BLUE Counterspell -> NOT countered!
    let cast_c3 = submit(
        veiled.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(veiled.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let c3_entry = cast_c3
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let p1_resp_blue = submit(cast_c3.state(), P0, GameCommand::Pass);
    let cast_blue_ctr = submit(
        p1_resp_blue.state(),
        P1,
        GameCommand::CastPaid {
            spell: object(p1_resp_blue.state(), 5),
            target: TargetRef::Stack(c3_entry),
            payment: ManaPayment::default(),
        },
    );
    let blue_ctr_resolved = pass_in_order(cast_blue_ctr.state(), [P1, P0]);
    // Creature #3 is still on the stack and resolves onto battlefield!
    let c3_resolved = pass_pair(blue_ctr_resolved.state());
    assert_eq!(
        zone_of(c3_resolved.state(), 3),
        ObjectZone::Battlefield,
        "CantBeCounteredByColors(Blue|Black) must prevent blue counterspell"
    );

    // Part A2: P0 casts creature #4; P1 responds with RED Counterspell -> IS countered!
    let cast_c4 = submit(
        c3_resolved.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(c3_resolved.state(), 4),
            payment: Some(ManaPayment::default()),
        },
    );
    let c4_entry = cast_c4
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let p1_resp_red = submit(cast_c4.state(), P0, GameCommand::Pass);
    let cast_red_ctr = submit(
        p1_resp_red.state(),
        P1,
        GameCommand::CastPaid {
            spell: object(p1_resp_red.state(), 6),
            target: TargetRef::Stack(c4_entry),
            payment: ManaPayment::default(),
        },
    );
    let red_ctr_resolved = pass_in_order(cast_red_ctr.state(), [P1, P0]);
    assert_eq!(
        zone_of(red_ctr_resolved.state(), 4),
        ObjectZone::Graveyard(P0),
        "CantBeCounteredByColors(Blue|Black) must allow red counterspell to counter"
    );

    // Part B: CannotBeTargetedBySpellsOfColors blocks blue SPELL targeting shielded_bear,
    // while allowing a blue permanent's ACTIVATED ABILITY to target shielded_bear!
    let p1_prio = submit(red_ctr_resolved.state(), P0, GameCommand::Pass);
    assert_eq!(
        attempt(
            p1_prio.state(),
            P1,
            GameCommand::CastPaid {
                spell: object(p1_prio.state(), 7),
                target: TargetRef::Object(shielded_bear),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "blue spell must not be able to target creature with CannotBeTargetedBySpellsOfColors(Blue)"
    );

    let act_ping = submit(
        p1_prio.state(),
        P1,
        GameCommand::ActivateAbility {
            source: pinger_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(shielded_bear),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_ping = pass_in_order(act_ping.state(), [P1, P0]);
    assert_eq!(
        after_ping
            .state()
            .execution()
            .creature(shielded_bear)
            .unwrap()
            .unwrap()
            .damage,
        1.into(),
        "blue permanent's activated ability must still legally target creature with CannotBeTargetedBySpellsOfColors"
    );
}

// ---------------------------------------------------------------------------
// 12. OfferAnyPlayer: all-decline resets last_action_succeeded & unpayable cost guard
// ---------------------------------------------------------------------------

#[test]
fn offer_any_player_all_decline_and_unpayable_cost_guard() {
    let vocab = vocabulary();

    // Part A: Prior instruction sets last_action_succeeded = true; OfferAnyPlayer with all players
    // declining must reset last_action_succeeded = false so trailing BranchIfFalse skips bonus!
    let decline_guard_spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PayLife {
                        player: ResolvingPlayer::Controller,
                        amount: ValueExpr::literal(1),
                    },
                    Instruction::OfferAnyPlayer {
                        players: AffectedPlayers::EachOpponent,
                        cost_body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::PayLife {
                                player: ResolvingPlayer::LoopPlayer,
                                amount: ValueExpr::literal(2),
                            }],
                        ),
                        paid_body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::GainControllerLife {
                                amount: ValueExpr::literal(10),
                            }],
                        ),
                        declined_body: None,
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::EventValue(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: 4,
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(50),
                    },
                ],
            ),
        ),
        &vocab,
    );

    // Part B: Shivan Wumpus style ETB OfferAnyPlayer: any opponent may sacrifice a land; if a
    // player does, put Wumpus on top of its owner's library.
    let mut wumpus = vanilla_creature(6, 6);
    wumpus.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::OfferAnyPlayer {
                players: AffectedPlayers::EachOpponent,
                cost_body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Sacrifice {
                        players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                        amount: ValueExpr::literal(1),
                        filter: PermanentTypeFilter {
                            any_types: BTreeSet::new(),
                            required_types: BTreeSet::from([CardType::Land]),
                            forbidden_types: BTreeSet::new(),
                            object_filter: None,
                        },
                    }],
                ),
                paid_body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            spellwake_engine::toolbox::ZoneDest::Library {
                                position:
                                    spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                        LibraryPlacement::Top,
                                    ),
                            },
                        ),
                    ))],
                ),
                declined_body: None,
            }],
        ),
    });
    let wumpus = validated(wumpus, &vocab);

    // World A: P1 has 0 lands.
    let game_no_land = world(
        vocab.clone(),
        vec![
            card(1, decline_guard_spell, false),
            card(2, wumpus.clone(), false),
        ],
        vec![],
    );

    // Part A: P1 declines offer -> last_action_succeeded reset to false -> +50 life skipped (20 - 1 = 19).
    let pending_decline = cast_untargeted(&game_no_land, 1);
    let after_decline = answer_yes_no_offer(pending_decline.state(), P1, false);
    assert!(after_decline.state().pending_decision().is_none());
    assert_eq!(
        after_decline.state().execution().life_total(P0),
        Some(&19.into()),
        "OfferAnyPlayer all-decline must reset last_action_succeeded = false"
    );

    // Part B1: Wumpus enters with P1 controlling 0 lands; P1 clicks Accept -> cost_body fails ->
    // paid_body skipped -> Wumpus stays on battlefield!
    let wumpus_entered = cast_untargeted(after_decline.state(), 2);
    let wumpus_prompt = pass_pair(wumpus_entered.state());
    let after_unpayable = answer_yes_no_offer(wumpus_prompt.state(), P1, true);
    assert!(after_unpayable.state().pending_decision().is_none());
    assert_eq!(
        zone_of(after_unpayable.state(), 2),
        ObjectZone::Battlefield,
        "unpayable cost_body (0 lands) must skip paid_body so Wumpus stays on battlefield"
    );

    // Part B2: Wumpus enters with P1 controlling 1 Forest; P1 accepts and sacrifices Forest ->
    // paid_body runs -> Wumpus moves to top of P0's library!
    let game_with_land = world(
        vocab.clone(),
        vec![
            card(1, wumpus, false),
            opp_battlefield(2, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(2)],
    );
    let w2_entered = cast_untargeted(&game_with_land, 1);
    let w2_prompt = pass_pair(w2_entered.state());
    let w2_accepted = answer_yes_no_offer(w2_prompt.state(), P1, true);
    let after_paid = if let Some(decision) = w2_accepted.state().pending_decision() {
        let RuleFrame::ResolutionSelect(sel) = decision.frame() else {
            panic!(
                "expected ResolutionSelect for land sacrifice, got {:?}",
                decision.frame()
            );
        };
        reply(
            w2_accepted.state(),
            P1,
            DecisionReply::SelectResolution(vec![sel.domain()[0]]),
        )
    } else {
        w2_accepted
    };
    assert!(after_paid.state().pending_decision().is_none());
    assert_eq!(
        zone_of(after_paid.state(), 2),
        ObjectZone::Graveyard(P1),
        "accepting P1 must sacrifice Forest"
    );
    assert_eq!(
        zone_of(after_paid.state(), 1),
        ObjectZone::Library(P0),
        "when cost_body succeeds, paid_body must put Wumpus on top of P0's library"
    );
}

// ---------------------------------------------------------------------------
// 13. Deflecting Palm followup, DrawGame, and TurnFaceUpSubject
// ---------------------------------------------------------------------------

#[test]
fn deflecting_palm_prevent_followup_and_draw_game_and_turn_face_up() {
    let vocab = vocabulary();

    // Part A: Deflecting Palm prevention followup deals prevented damage to source's controller.
    let palm = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                        StaticContinuousAbility::DamageRule {
                            id: StaticAbilityId(1),
                            rule: StaticDamageRule::ModifyDamage {
                                source: None,
                                source_controller: PlayerRelation::Opponent,
                                recipient: StaticDamageRecipient::Controller,
                                kind: StaticDamageKind::Any,
                                op: StaticDamageOp::PreventAndFollowup {
                                    mode: DamagePreventMode::All,
                                    followup:
                                        DamagePreventFollowup::DealPreventedToSourceController,
                                },
                            },
                        },
                    ))),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );

    // Part B: Instruction::DrawGame ends the game in an immediate draw.
    let draw_game_spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::None, vec![Instruction::DrawGame]),
        ),
        &vocab,
    );

    // Part C: Instruction::TurnFaceUpSubject turns a face-down Morph permanent face up.
    let mut morph_beast = vanilla_creature(5, 5);
    morph_beast.face_up_abilities = vec![FaceUpAbility::Morph(ManaCost::generic(1.into()))];
    let morph_beast = validated(morph_beast, &vocab);

    let turn_up_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::TurnFaceUpSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                }],
            ),
        ),
        &vocab,
    );

    let mut mana_rock = Program::artifact(None, vec![]);
    mana_rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 3.into())])),
    });
    let mana_rock = validated(mana_rock, &vocab);

    let opp_bolt = validated(damage_any_instant(4), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, palm, false),
            opp_hand(2, opp_bolt),
            card(3, morph_beast, false),
            card(4, turn_up_spell, false),
            card(5, mana_rock, true),
            card(6, draw_game_spell, false),
        ],
        vec![CardId(5)],
    );

    // Part A: Resolve Palm, pass priority to P1, P1 bolts P0 for 4 -> P0 takes 0, P1 takes 4!
    let palmed = cast_untargeted(&game, 1);
    let p1_prio = submit(palmed.state(), P0, GameCommand::Pass);
    let after_bolt = cast_targeted(p1_prio.state(), P1, 2, TargetRef::Player(P0));
    assert_eq!(
        after_bolt.state().execution().life_total(P0),
        Some(&20.into()),
        "Deflecting Palm must prevent all 4 damage to P0"
    );
    assert_eq!(
        after_bolt.state().execution().life_total(P1),
        Some(&16.into()),
        "DealPreventedToSourceController must deal 4 prevented damage to P1"
    );

    // Part C: Cast morph_beast face down for {3} -> 2/2 face down; TurnFaceUpSubject turns it face up (5/5)!
    let tapped = submit(
        after_bolt.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_bolt.state(), 5),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_fd = submit(
        tapped.state(),
        P0,
        GameCommand::CastFaceDown {
            spell: object(tapped.state(), 3),
            kind: FaceDownKind::Morph,
            payment: Some(ManaPayment {
                from_lots: BTreeMap::new(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 3.into(),
                }],
            }),
        },
    );
    let fd_entered = pass_pair(cast_fd.state());
    let fd_id = object(fd_entered.state(), 3);
    assert!(
        fd_entered
            .state()
            .execution()
            .window()
            .cards()
            .get(fd_id)
            .unwrap()
            .face_down()
            .is_some(),
        "morph creature must enter face down"
    );
    assert_eq!(
        current(fd_entered.state(), fd_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );

    let turned_up = cast_targeted(fd_entered.state(), P0, 4, TargetRef::Object(fd_id));
    assert!(
        turned_up
            .state()
            .execution()
            .window()
            .cards()
            .get(fd_id)
            .unwrap()
            .face_down()
            .is_none(),
        "TurnFaceUpSubject must turn the permanent face up"
    );
    assert_eq!(
        current(turned_up.state(), fd_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(5, 5),
        "face-up morph creature must regain printed 5/5 P/T"
    );
    assert!(
        all_occurrences(&turned_up)
            .iter()
            .any(|occ| matches!(occ, RulesOccurrence::TurnedFaceUpByEffect { .. })),
        "TurnFaceUpSubject must emit TurnedFaceUpByEffect occurrence"
    );

    // Part B: Instruction::DrawGame ends the game immediately in a draw.
    let drawn = cast_untargeted(turned_up.state(), 6);
    let conclusion = drawn
        .state()
        .conclusion()
        .expect("DrawGame must conclude the game immediately");
    assert_eq!(conclusion.outcome(), GameOutcome::Draw);
}

// ---------------------------------------------------------------------------
// 14. FirstTargetController and validation fixes (BranchIfFalse + EmitReflexiveTrigger)
// ---------------------------------------------------------------------------

#[test]
fn first_target_controller_and_validation_fixes() {
    let vocab = vocabulary();

    // Part A: Mutiny-style FixedTargetClause where clause 1 uses ControllerFilter::FirstTargetController.
    let mut clause_1 = ObjectClause::creature();
    clause_1.controller = ControllerFilter::FirstTargetController;
    let mutiny = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::FilteredPermanent(ObjectFilter::creature().not_controlled()),
                        vec![Instruction::RememberPermanentTarget],
                    ))),
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::FilteredPermanent(ObjectFilter::Clause(clause_1)),
                        vec![Instruction::RememberPermanentTarget],
                    ))),
                    Instruction::FightFixedTargets,
                ],
            ),
        ),
        &vocab,
    );

    // Part B: Adipose Offspring style ETB: OptionalEffect + SacrificeMatching -> BranchIfFalse on
    // LastSacrificeSucceeded -> EmitReflexiveTrigger referencing LastSacrificedCreature power!
    let mut offspring = vanilla_creature(2, 2);
    offspring.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                },
                Instruction::SacrificeMatching(SacrificeSelection::OtherCreature),
                Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::EventValue(EventValueKind::LastSacrificeSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    destination: 4,
                },
                Instruction::EmitReflexiveTrigger {
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::GainControllerLife {
                            amount: ValueExpr::ObjectStat {
                                subject: ValueObjectRef::LastSacrificedCreature,
                                stat: ObjectStatKind::Power,
                            },
                        }],
                    )),
                },
            ],
        ),
    });
    let offspring = validated(offspring, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, mutiny, false),
            card(2, validated(vanilla_creature(4, 3), &vocab), true),
            opp_battlefield(3, validated(vanilla_creature(4, 4), &vocab)),
            opp_battlefield(4, validated(vanilla_creature(2, 2), &vocab)),
            card(5, offspring.clone(), false),
            card(6, offspring, false),
        ],
        vec![CardId(2), CardId(3), CardId(4)],
    );

    let own_fodder = object(&game, 2);
    let opp_big = object(&game, 3);
    let opp_small = object(&game, 4);

    // Part A: Clause 1 targeting P0's creature when clause 0 targets P1's creature is REJECTED.
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastWithResources {
                spell: object(&game, 1),
                target: None,
                payment: CastingPayment {
                    fixed_targets: vec![TargetRef::Object(opp_big), TargetRef::Object(own_fodder),],
                    mana: Some(ManaPayment::default()),
                    ..CastingPayment::default()
                },
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "FirstTargetController must reject clause 1 target controlled by a different player"
    );

    // Targeting two creatures controlled by the same player (P1) succeeds and resolves fight!
    let after_mutiny = cast_fixed_targets(
        &game,
        P0,
        1,
        vec![TargetRef::Object(opp_big), TargetRef::Object(opp_small)],
    );
    assert_eq!(
        zone_of(after_mutiny.state(), 4),
        ObjectZone::Graveyard(P1),
        "2/2 opp_small must die fighting 4/4 opp_big"
    );
    assert_eq!(zone_of(after_mutiny.state(), 3), ObjectZone::Battlefield);

    // Part B1: Offspring #1 enters -> accept optional sacrifice of 4/3 own_fodder -> reflexive trigger
    // emits and gains 4 life from LastSacrificedCreature power!
    let entered_1 = cast_untargeted(after_mutiny.state(), 5);
    let opt_1 = pass_pair(entered_1.state());
    assert!(matches!(
        opt_1.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(_))
    ));
    let accepted = reply(
        opt_1.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let after_sac = if let Some(decision) = accepted.state().pending_decision() {
        let RuleFrame::ResolutionSelect(_) = decision.frame() else {
            panic!(
                "expected ResolutionSelect for sacrifice, got {:?}",
                decision.frame()
            );
        };
        reply(
            accepted.state(),
            P0,
            DecisionReply::SelectResolution(vec![own_fodder]),
        )
    } else {
        accepted
    };
    assert_eq!(zone_of(after_sac.state(), 2), ObjectZone::Graveyard(P0));
    assert_eq!(
        after_sac.state().execution().window().entries().len(),
        1,
        "EmitReflexiveTrigger must stack reflexive trigger after successful sacrifice"
    );
    let after_reflexive = pass_pair(after_sac.state());
    assert_eq!(
        after_reflexive.state().execution().life_total(P0),
        Some(&24.into()),
        "reflexive trigger inheriting LastSacrificedCreature must gain 4 life (fodder power)"
    );

    // Part B2: Offspring #2 enters -> decline optional sacrifice -> LastSacrificeSucceeded == 0 ->
    // BranchIfFalse skips EmitReflexiveTrigger -> no trigger stacked, life stays 24!
    let entered_2 = cast_untargeted(after_reflexive.state(), 6);
    let opt_2 = pass_pair(entered_2.state());
    let declined = reply(
        opt_2.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert!(
        declined.state().execution().window().entries().is_empty(),
        "declining sacrifice must skip EmitReflexiveTrigger via LastSacrificeSucceeded branch"
    );
    assert_eq!(
        declined.state().execution().life_total(P0),
        Some(&24.into())
    );
}
