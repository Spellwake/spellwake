#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Runtime and validation integration tests for Pass F2-B Wave 9 engine additions:
//! - `MoveSubjectRef::RevealedCard` (`Chaos Warp` reading opponent's `revealed_top_card`)
//! - `Instruction::DiscardUpToExt` (filtered up-to/any-number discard, 0 and nonzero counts,
//!   `last_action_succeeded` and `last_action_count`)
//! - `Instruction::Earthbend` and `Instruction::ReturnEarthbentCard` (animation, haste,
//!   +1/+1 counters, `PlayerTriggerAction::Earthbent`, delayed return tapped on death/exile,
//!   and watch dropped on bounce to hand)
//! - `Instruction::SkipNextTurns` (CR 614.10 / CR 614.10a extra-turn consumption, skipping
//!   normal turns without expiring "until your next turn" effects)
//! - `Instruction::SelectExtremeLifePlayer` (clear winner, `ControllerChooses` tie choice,
//!   `Skip`, and `DrawGame`)
//! - `SearchManaValue::ExactManaCostOneOf` (`Urza's Saga` Ch. III exact `{0}`/`{1}` printed cost)
//! - `StaticPart::ModifyPTPerOwnColor` (per-recipient Layer 7c scaling and Layer 5 interaction)
//! - `ActivationRestriction::Boast`, `StaticModifierRule::ActivatedAbilityCost` (including
//!   CR 118.7a `minimum_total_mana: 1` floor), and `Program::activation_target_cost_reductions`
//! - `SpellCostModifier::KeywordCastCost` (`Dash` and `Flashback` discounts without changing
//!   normal cast costs)
//! - `ObjectFilterFlags::{DIED_THIS_TURN, PUT_INTO_GY_THIS_TURN, PUT_INTO_GY_FROM_LIBRARY_THIS_TURN}`
//!   in `Instruction::MoveGraveyardGroup`
//! - `SourceZoneFilter::LastMoved` on `TemporaryRuleKind::ZoneCastPermission` snapshotting
//!   `ExileTopPlay` exiled objects and revoking after zone change
//! - `Instruction::RegenerateSubject`, `PermanentTargetAction::Regenerate`,
//!   `EffectSubject::MatchingOtherThanTarget` (`Dark Dabbling`), and
//!   `EffectSubject::TargetAndSharingColorMatching`
//! - `Instruction::AddManaForPlayer` (fixed and interactive choice for non-controller) and
//!   CR 605.1b `TriggeredManaRule::EnchantedLandTapped`
//! - `StatusTriggerKind::Fought` and `RulesOccurrence::PermanentFought` (firing with 0 power)
//! - `Instruction::AttachChoosingPermanent`, `Instruction::AttachFixedTargets`, and
//!   `Instruction::AttachPermanentToPlayer` (`exclude_current_host`, `attachment_permitted`,
//!   mandatory 1, optional up-to-1, and attach-all modes)
//! - `EntryCopySpecExt` (`EntryCopyZone::OpponentGraveyard`, `enters_tapped`, `extra_mana`,
//!   `SourceStateExpr::DonorControlledByController`, and `TriggerCondition::SourceNotOwnedByController`).

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
    activated::{ActivatedAbility, ActivatedAbilityId, ActivationRestriction, SourceCost},
    attachment::EnchantRestriction,
    characteristics::{
        ActivatedCostScope, CastKeywordCostKind, Color, ColorOperation, ControllerSelector,
        KeywordAbility, PermanentSelector, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        SpellCostModifier, StaticAbilityId, StaticContinuousAbility, StaticModifierRule,
        StaticPart, StaticParts,
    },
    combat::{AttackDeclaration, CombatDecision, CombatRules},
    draw::GameOutcome,
    effects::{
        CounterKind, EffectError, LibraryTopReply, SeedCard, ZoneReplacementReply,
        ZoneReplacementRequest,
    },
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastingPayment, EnchantedLandManaBonus, ManaAbility, ManaAbilityId, ManaAbilityRef,
        ManaCost, ManaOutput, ManaPayment, ManaSpend, ManaType, TriggeredManaRule,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
    priority::SeatId,
    program::{
        AffectedPlayers, ArtifactSubtype, AttachSlot, BodyTarget, CardType, ColorMask, Comparison,
        EffectManaYield, EffectSubject, EndStepCast, EntryCopyRule, EntryCopySpecExt,
        EntryCopyZone, EventValueKind, ExecutableBody, ExtremeLifeTieMode, ImpulsePlayDuration,
        Instruction, LibrarySearch, MoveSubjectRef, ObjectClause, ObjectColorFilter, ObjectFilter,
        ObjectFilterFlags, PermanentDefinition, PermanentTargetAction, PlayerRelation,
        PlayerTriggerAction, Program, ProgramKind, ResolutionCondition, ResolvingPlayer,
        SearchCardMatcher, SearchManaValue, SourceStateExpr, SourceZoneFilter, StatusTriggerKind,
        SubtypeFilter, Supertype, TemporaryRuleKind, TriggerCondition, TriggerEvent,
        TriggerGrouping, TriggerPlayerScope, TriggerSpec, TriggerZone, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    setup::GameSeed,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 9 test program must validate");
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

fn controlled(id: u64, owner: SeatId, controller: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program,
    }
}

fn graveyard_of(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Graveyard,
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

fn with_colors(mut program: Program, colors: &[Color]) -> Program {
    program.color_indicator = Some(colors.iter().copied().collect());
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

fn mana_rock(yields: &[(ManaType, i64)]) -> Program {
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(
            yields
                .iter()
                .map(|(kind, amount)| (*kind, (*amount).into()))
                .collect(),
        ),
    });
    rock
}

fn mana_payment(spends: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: BTreeMap::new(),
        spend: spends
            .iter()
            .map(|(mana_type, amount)| ManaSpend {
                mana_type: *mana_type,
                amount: (*amount).into(),
            })
            .collect(),
    }
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

fn is_tapped(game: &GameState, id: u64) -> Option<bool> {
    let obj = object(game, id);
    game.execution()
        .window()
        .cards()
        .get(obj)
        .expect("seeded card must exist")
        .tapped()
}

fn plus_one_counters(game: &GameState, obj: ObjectId) -> i64 {
    current(game, obj)
        .plus_one_counters
        .to_usize()
        .map_or(0, |v| i64::try_from(v).unwrap())
}

fn controlled_creatures_selector() -> PermanentSelector {
    PermanentSelector {
        controller: ControllerSelector::SourceController,
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    }
}

fn all_occurrences(candidate: &NativeCandidate) -> Vec<RulesOccurrence> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .map(|row| row.value.clone())
        .collect()
}

fn attack_player(attacker: ObjectId, defender: SeatId) -> AttackDeclaration {
    AttackDeclaration {
        attacker,
        defender,
        defending_planeswalker: None,
    }
}

fn advance_to_next_turn_main(mut game: GameState) -> GameState {
    let start_turn = game.turn().unwrap().position().number;
    for _ in 0..80 {
        let pos = game.turn().unwrap().position();
        if pos.number > start_turn && pos.step == Step::PrecombatMain {
            return game;
        }
        if let Some(pending) = game.pending_decision() {
            let chooser = pending.chooser();
            let choice = match pending.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected decision while advancing turn: {other:?}"),
            };
            game = reply(&game, chooser, choice).into_state();
        } else {
            let active = game.execution().window().active_player();
            let order = if active == P0 { [P0, P1] } else { [P1, P0] };
            game = pass_in_order(&game, order).into_state();
        }
    }
    panic!("did not reach next turn PrecombatMain");
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
        GameId(20261126),
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

// ---------------------------------------------------------------------------
// 1. MoveSubjectRef::RevealedCard (Chaos Warp reads opponent's revealed card)
// ---------------------------------------------------------------------------

#[test]
fn chaos_warp_reveals_and_puts_opponent_permanent_under_owners_control() {
    let vocab = vocabulary();

    let chaos_warp = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Library {
                            position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
                        },
                    ))),
                    Instruction::LookTopChoose {
                        count: ValueExpr::literal(1),
                        library_owner: ResolvingPlayer::TargetOwner,
                        filter: None,
                        reveal: true,
                        reveal_only: true,
                        to_battlefield: false,
                        enters_tapped: false,
                        take_all_matching: false,
                        max_picks: 0,
                        rest_to_graveyard: false,
                        rest_to_exile: false,
                        rest_to_top: false,
                        random_rest_order: false,
                        secondary: None,
                    },
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Revealed,
                        spellwake_engine::toolbox::ZoneDest::Battlefield {
                            controller: if true {
                                spellwake_engine::toolbox::ZoneController::Owner
                            } else {
                                spellwake_engine::toolbox::ZoneController::Player(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    ),
                                ))
                            },
                            tapped: false,
                            attacking: false,
                            transformed: false,
                            face_down: false,
                            entering_counters: (None)
                                .map(|(k, c): (spellwake_engine::effects::CounterKind, u8)| {
                                    (
                                        k,
                                        Box::new(spellwake_engine::program::ValueExpr::literal(
                                            i64::from(c),
                                        )),
                                    )
                                })
                                .into_iter()
                                .collect(),
                            attach_to: if false {
                                Some(Box::new(spellwake_engine::toolbox::ObjectQuery::Source))
                            } else {
                                None
                            },
                        },
                    ))),
                ],
            ),
        ),
        &vocab,
    );

    let opp_relic = validated(named(Program::artifact(None, vec![]), "Opp Relic"), &vocab);
    let opp_dragon = validated(named(vanilla_creature(5, 5), "Opp Dragon"), &vocab);
    let caster_top_bear = validated(named(vanilla_creature(2, 2), "Caster Top Bear"), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, chaos_warp, false),
            opp_battlefield(2, opp_relic),
            library_top(10, P0, caster_top_bear),
            library_top(20, P1, opp_dragon),
        ],
        vec![CardId(2)],
    );

    let opp_relic_id = object(&game, 2);
    let cast = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Object(opp_relic_id),
            payment: ManaPayment::default(),
        },
    );
    let ready = cast
        .state()
        .supply_entropy(cast.state().basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let resolved = pass_pair(&ready);

    // Caster's own top library card (card 10) must NOT be touched or put onto the battlefield.
    assert_eq!(
        zone_of(resolved.state(), 10),
        ObjectZone::Library(P0),
        "Chaos Warp must reveal from target owner's library, never caster's library"
    );

    let revealed_cards: Vec<CardId> = all_occurrences(&resolved)
        .into_iter()
        .filter_map(|occ| match occ {
            RulesOccurrence::LibraryCardRevealed { object } => object.card_id(),
            _ => None,
        })
        .collect();
    assert_eq!(
        revealed_cards.len(),
        1,
        "Chaos Warp must reveal exactly one top card from target owner's library"
    );
    let revealed_card = revealed_cards[0];
    assert!(
        revealed_card == CardId(2) || revealed_card == CardId(20),
        "revealed card must belong to opponent P1 (got {revealed_card:?})"
    );

    let revealed_obj = object(resolved.state(), revealed_card.0);
    let revealed_physical = resolved
        .state()
        .execution()
        .window()
        .cards()
        .get(revealed_obj)
        .unwrap();
    assert_eq!(
        revealed_physical.zone(),
        ObjectZone::Battlefield,
        "revealed permanent from opponent's library must enter the battlefield"
    );
    assert_eq!(
        revealed_physical.controller(),
        Some(P1),
        "revealed permanent must enter under its owner's (P1's) control"
    );
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .library(P1)
            .len(),
        1,
        "opponent's other card must remain in opponent's library"
    );
}

// ---------------------------------------------------------------------------
// 2. Instruction::DiscardUpToExt (filtered, 0 and nonzero counts)
// ---------------------------------------------------------------------------

#[test]
fn discard_up_to_ext_filtered_zero_and_nonzero_counts() {
    let vocab = vocabulary();

    // Discard any number of creature cards. If you discarded at least one, draw that many cards.
    // Then gain 1 life as a completion sentinel.
    let rummage = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ForEachPlayerQuery { selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::One(ResolvingPlayer::Controller))), bind_player: spellwake_engine::toolbox::BindingId(904), body: Box::new(spellwake_engine::program::ExecutableBody::new(spellwake_engine::program::BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(904)))), ObjectFilter::creature()), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(904)), min: Box::new(spellwake_engine::program::ValueExpr::literal(0)), max: Box::new(ValueExpr::QueryCount(Box::new(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(904)))), ObjectFilter::creature())))), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: true }, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard })))])) },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: 3,
                    },
                    Instruction::DrawController {
                        amount: ValueExpr::event(EventValueKind::LastActionCount),
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let bear = validated(vanilla_creature(2, 2), &vocab);
    let forest = validated(basic_forest(), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, rummage.clone(), false),
            card(2, rummage, false),
            card(3, bear.clone(), false),
            card(4, bear.clone(), false),
            card(5, forest.clone(), false),
            library_top(10, P0, forest.clone()),
            library_top(11, P0, forest),
        ],
        vec![],
    );

    let bear3 = object(&game, 3);
    let bear4 = object(&game, 4);
    let land5 = object(&game, 5);

    // Case A: Choose 0 cards -> last_action_succeeded == false -> draws 0 cards, sentinel still runs.
    let pending_zero = cast_untargeted(&game, 1);
    let decision_zero = pending_zero
        .state()
        .pending_decision()
        .expect("DiscardUpToExt must prompt PromptChoice");
    let RuleFrame::PromptChoice(frame) = decision_zero.frame() else {
        panic!("expected PromptChoice frame");
    };
    let spellwake_engine::toolbox::ResolvedPromptDomain::Objects {
        candidates, min, ..
    } = &frame.domain
    else {
        panic!("expected Objects domain");
    };
    assert_eq!(*min, 0);
    let domain: BTreeSet<ObjectId> = candidates.iter().copied().collect();
    assert_eq!(
        domain,
        BTreeSet::from([bear3, bear4]),
        "filtered DiscardUpToExt must include only creature cards and exclude noncreature land"
    );
    assert!(!domain.contains(&land5));

    let after_zero = reply(
        pending_zero.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Objects(
            vec![],
        )),
    );
    assert!(after_zero.state().pending_decision().is_none());
    assert_eq!(zone_of(after_zero.state(), 3), ObjectZone::Hand(P0));
    assert_eq!(zone_of(after_zero.state(), 4), ObjectZone::Hand(P0));
    assert_eq!(
        after_zero
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        2,
        "0 discards must set last_action_succeeded = false and skip the follow-up draw"
    );
    assert_eq!(
        after_zero.state().execution().life_total(P0),
        Some(&21.into())
    );

    // Case B: Discard both creature cards -> last_action_succeeded == true, last_action_count == 2 -> draw 2.
    let pending_two = cast_untargeted(after_zero.state(), 2);
    let mut after_two = reply(
        pending_two.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Objects(vec![
            object(pending_two.state(), 3),
            object(pending_two.state(), 4),
        ])),
    );
    if let Some(decision) = after_two.state().pending_decision() {
        let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
            panic!(
                "expected ResolutionGraveyardOrder frame for 2-card discard, got {:?}",
                decision.frame()
            );
        };
        let order = frame.domain().to_vec();
        after_two = reply(
            after_two.state(),
            P0,
            DecisionReply::OrderResolutionGraveyard(order),
        );
    }
    assert!(after_two.state().pending_decision().is_none());
    assert_eq!(zone_of(after_two.state(), 3), ObjectZone::Graveyard(P0));
    assert_eq!(zone_of(after_two.state(), 4), ObjectZone::Graveyard(P0));
    assert_eq!(zone_of(after_two.state(), 5), ObjectZone::Hand(P0));
    assert_eq!(
        after_two
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        0,
        "discarding 2 creatures must record last_action_count = 2 and draw 2 cards"
    );
    assert_eq!(zone_of(after_two.state(), 10), ObjectZone::Hand(P0));
    assert_eq!(zone_of(after_two.state(), 11), ObjectZone::Hand(P0));
    assert_eq!(
        after_two.state().execution().life_total(P0),
        Some(&22.into())
    );
}

// ---------------------------------------------------------------------------
// 3. Instruction::Earthbend and Instruction::ReturnEarthbentCard
// ---------------------------------------------------------------------------

#[test]
fn earthbend_animates_returns_tapped_on_death_and_ignores_bounce() {
    let vocab = vocabulary();

    let earthbend_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::land().controlled()),
                vec![Instruction::Earthbend {
                    amount: ValueExpr::literal(3),
                }],
            ),
        ),
        &vocab,
    );

    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::PlayerAction {
            player: PlayerRelation::Controller,
            action: PlayerTriggerAction::Earthbent,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(5),
            }],
        ),
    });
    let watcher = validated(watcher, &vocab);

    let destroy = validated(destroy_permanent_instant(), &vocab);
    let exile_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: matches!(None, Some(spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves)), delayed_return: None, link_group: None })))],
            ),
        ),
        &vocab,
    );
    let bounce_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Hand,
                    ),
                ))],
            ),
        ),
        &vocab,
    );
    let forest = validated(basic_forest(), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, forest.clone(), true),
            card(2, forest.clone(), true),
            card(3, forest, true),
            card(4, watcher, true),
            card(10, earthbend_spell.clone(), false),
            card(11, earthbend_spell.clone(), false),
            card(12, earthbend_spell, false),
            card(20, destroy, false),
            card(21, exile_spell, false),
            card(22, bounce_spell, false),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );

    // Part A: Earthbend(3) on forest 1 -> 0/0 haste creature + 3 +1/+1 counters (3/3) + Earthbent trigger!
    let f1 = object(&game, 1);
    let after_eb1 = cast_targeted(&game, P0, 10, TargetRef::Object(f1));
    // Earthbent trigger is now on the stack.
    assert_eq!(
        after_eb1.state().execution().window().entries().len(),
        1,
        "Earthbend must emit PlayerTriggerAction::Earthbent and stack watcher trigger"
    );
    let after_trig1 = pass_pair(after_eb1.state());
    assert_eq!(
        after_trig1.state().execution().life_total(P0),
        Some(&25.into()),
        "Earthbent watcher trigger must gain 5 life"
    );

    let f1_view = current(after_trig1.state(), f1);
    assert!(f1_view.is_creature());
    assert!(f1_view.has_type(CardType::Land));
    assert!(f1_view.has_keyword(KeywordAbility::Haste));
    assert_eq!(plus_one_counters(after_trig1.state(), f1), 3);
    assert_eq!(f1_view.power_toughness.unwrap().derived, pt(3, 3));

    // Destroy earthbent forest 1 -> delayed ReturnEarthbentCard trigger stacks and returns it tapped.
    let after_destroy = cast_targeted(after_trig1.state(), P0, 20, TargetRef::Object(f1));
    assert_eq!(
        zone_of(after_destroy.state(), 1),
        ObjectZone::Graveyard(P0),
        "destroyed earthbent land must briefly sit in graveyard while delayed return trigger is stacked"
    );
    assert_eq!(
        after_destroy.state().execution().window().entries().len(),
        1,
        "dying earthbent land must stack delayed ReturnEarthbentCard trigger"
    );
    let after_return1 = pass_pair(after_destroy.state());
    assert_eq!(zone_of(after_return1.state(), 1), ObjectZone::Battlefield);
    assert_eq!(
        is_tapped(after_return1.state(), 1),
        Some(true),
        "returned earthbent land must enter tapped"
    );
    let returned_f1 = object(after_return1.state(), 1);
    assert_eq!(plus_one_counters(after_return1.state(), returned_f1), 0);
    assert!(!current(after_return1.state(), returned_f1).is_creature());

    // Part B: Earthbend(3) on forest 2, then exile it -> delayed return trigger returns it tapped!
    let f2 = object(after_return1.state(), 2);
    let after_eb2 = cast_targeted(after_return1.state(), P0, 11, TargetRef::Object(f2));
    let after_trig2 = pass_pair(after_eb2.state());
    let after_exile = cast_targeted(after_trig2.state(), P0, 21, TargetRef::Object(f2));
    assert_eq!(zone_of(after_exile.state(), 2), ObjectZone::Exile);
    assert_eq!(after_exile.state().execution().window().entries().len(), 1);
    let after_return2 = pass_pair(after_exile.state());
    assert_eq!(zone_of(after_return2.state(), 2), ObjectZone::Battlefield);
    assert_eq!(is_tapped(after_return2.state(), 2), Some(true));

    // Part C: Earthbend(3) on forest 3, then bounce to hand -> watch dropped, NO return trigger!
    let f3 = object(after_return2.state(), 3);
    let after_eb3 = cast_targeted(after_return2.state(), P0, 12, TargetRef::Object(f3));
    let after_trig3 = pass_pair(after_eb3.state());
    let after_bounce = cast_targeted(after_trig3.state(), P0, 22, TargetRef::Object(f3));
    assert_eq!(zone_of(after_bounce.state(), 3), ObjectZone::Hand(P0));
    assert!(
        after_bounce
            .state()
            .execution()
            .window()
            .entries()
            .is_empty(),
        "bouncing an earthbent land to hand must drop the watch without triggering return"
    );
}

// ---------------------------------------------------------------------------
// 4. Instruction::SkipNextTurns (CR 614.10 & CR 614.10a)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. Instruction::SelectExtremeLifePlayer (winner, ControllerChooses, Skip, DrawGame)
// ---------------------------------------------------------------------------

#[test]
fn select_extreme_life_player_single_tie_choice_and_draw_game() {
    let vocab = vocabulary();

    let highest_choose_lose3 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::SelectExtremeLifePlayer {
                        highest: true,
                        tie_mode: ExtremeLifeTieMode::ControllerChooses,
                    },
                    Instruction::AdjustPlayersLife {
                        players: AffectedPlayers::One(ResolvingPlayer::ChosenPlayer),
                        gain: false,
                        amount: ValueExpr::literal(3),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let lowest_skip_lose5 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::SelectExtremeLifePlayer {
                        highest: false,
                        tie_mode: ExtremeLifeTieMode::Skip,
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: 3,
                    },
                    Instruction::AdjustPlayersLife {
                        players: AffectedPlayers::One(ResolvingPlayer::ChosenPlayer),
                        gain: false,
                        amount: ValueExpr::literal(5),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let highest_draw_game = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SelectExtremeLifePlayer {
                    highest: true,
                    tie_mode: ExtremeLifeTieMode::DrawGame,
                }],
            ),
        ),
        &vocab,
    );

    let gain5 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(5),
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, highest_choose_lose3.clone(), false),
            card(2, lowest_skip_lose5.clone(), false),
            card(3, highest_draw_game, false),
            card(4, gain5, false),
            card(5, highest_choose_lose3, false),
            card(6, lowest_skip_lose5, false),
        ],
        vec![],
    );

    // Case A: Tied at 20-20 with Skip -> no prompt, last_action_succeeded == false, no life lost.
    let after_skip_tie = cast_untargeted(&game, 2);
    assert!(after_skip_tie.state().pending_decision().is_none());
    assert_eq!(
        after_skip_tie.state().execution().life_total(P0),
        Some(&20.into())
    );
    assert_eq!(
        after_skip_tie.state().execution().life_total(P1),
        Some(&20.into())
    );

    // Case B: Tied at 20-20 with DrawGame -> ends the game immediately in a draw!
    let drawn = cast_untargeted(after_skip_tie.state(), 3);
    assert_eq!(
        drawn
            .state()
            .conclusion()
            .expect("DrawGame tie mode must conclude game on tie")
            .outcome(),
        GameOutcome::Draw
    );

    // Case C: Tied at 20-20 with ControllerChooses -> prompts OptionIndex; pick index 1 (P1) -> P1 loses 3.
    let pending_tie = cast_untargeted(after_skip_tie.state(), 1);
    let decision = pending_tie
        .state()
        .pending_decision()
        .expect("ControllerChooses must suspend on OptionIndex when tied");
    assert_eq!(decision.chooser(), P0);
    let RuleFrame::OptionIndex(opt) = decision.frame() else {
        panic!("expected OptionIndex frame on tie");
    };
    assert_eq!(opt.count(), 2);
    let after_tie_pick = reply(pending_tie.state(), P0, DecisionReply::OptionIndex(1));
    assert_eq!(
        after_tie_pick.state().execution().life_total(P0),
        Some(&20.into())
    );
    assert_eq!(
        after_tie_pick.state().execution().life_total(P1),
        Some(&17.into())
    );

    // Case D: Clear single winner (P0 at 25 vs P1 at 17) -> no prompt, selects P0 automatically (25 -> 22),
    // then lowest (Skip mode) selects P1 automatically (17 -> 12).
    let p0_ahead = cast_untargeted(after_tie_pick.state(), 4);
    assert_eq!(
        p0_ahead.state().execution().life_total(P0),
        Some(&25.into())
    );

    let after_clear_high = cast_untargeted(p0_ahead.state(), 5);
    assert!(
        after_clear_high.state().pending_decision().is_none(),
        "clear extreme-life winner must not prompt"
    );
    assert_eq!(
        after_clear_high.state().execution().life_total(P0),
        Some(&22.into())
    );
    assert_eq!(
        after_clear_high.state().execution().life_total(P1),
        Some(&17.into())
    );

    let after_clear_low = cast_untargeted(after_clear_high.state(), 6);
    assert!(after_clear_low.state().pending_decision().is_none());
    assert_eq!(
        after_clear_low.state().execution().life_total(P0),
        Some(&22.into())
    );
    assert_eq!(
        after_clear_low.state().execution().life_total(P1),
        Some(&12.into())
    );
}

// ---------------------------------------------------------------------------
// 6. SearchManaValue::ExactManaCostOneOf (Urza's Saga Ch. III)
// ---------------------------------------------------------------------------

#[test]
fn exact_mana_cost_one_of_matches_only_zero_or_one_generic() {
    let vocab = vocabulary();

    let mut saga_search = LibrarySearch::to_battlefield(
        SearchCardMatcher::CardTypes(BTreeSet::from([CardType::Artifact])),
        false,
    );
    saga_search.mana_value = Some(SearchManaValue::ExactManaCostOneOf(vec![
        ManaCost::free(),
        ManaCost::generic(1.into()),
    ]));

    let tutor = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(saga_search),
                }],
            ),
        ),
        &vocab,
    );

    // 10: {0} artifact -> ELIGIBLE.
    let zero_artifact = validated(
        named(
            Program::artifact(Some(ManaCost::free()), vec![]),
            "Mox Zero",
        ),
        &vocab,
    );
    // 11: {1} artifact -> ELIGIBLE.
    let one_artifact = validated(
        named(
            Program::artifact(Some(ManaCost::generic(1.into())), vec![]),
            "Sol Ring One",
        ),
        &vocab,
    );
    // 12: {W} artifact (MV 1, NOT exact {1}) -> REJECTED.
    let white_artifact = validated(
        named(
            Program::artifact(
                Some(ManaCost {
                    generic: 0.into(),
                    required: BTreeMap::from([(ManaType::White, 1.into())]),
                }),
                vec![],
            ),
            "White Bauble",
        ),
        &vocab,
    );
    // 13: {X} artifact (MV 0, has x_mana_symbols == 1, NOT exact {0}) -> REJECTED.
    let mut x_artifact = named(
        Program::artifact(Some(ManaCost::free()), vec![]),
        "Chalice X",
    );
    x_artifact.x_mana_symbols = 1;
    let x_artifact = validated(x_artifact, &vocab);
    // 14: Artifact Land with NO mana cost (casting_cost: None) -> REJECTED.
    let artifact_land = validated(
        named(
            Program::new(
                BTreeSet::from([CardType::Artifact, CardType::Land]),
                BTreeSet::new(),
                BTreeSet::new(),
                ProgramKind::Permanent(PermanentDefinition {
                    entry_tap: None,
                    attachment: None,
                    printed_pt: None,
                    casting_cost: None,
                }),
            ),
            "Ancient Den",
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, tutor, false),
            library_top(10, P0, zero_artifact),
            library_top(11, P0, one_artifact),
            library_top(12, P0, white_artifact),
            library_top(13, P0, x_artifact),
            library_top(14, P0, artifact_land),
        ],
        vec![],
    );

    let id10 = object(&game, 10);
    let id11 = object(&game, 11);
    let id12 = object(&game, 12);
    let id13 = object(&game, 13);
    let id14 = object(&game, 14);

    let pending = cast_untargeted(&game, 1);
    let RuleFrame::LibraryTop(frame) = pending
        .state()
        .pending_decision()
        .expect("ExactManaCostOneOf search must prompt LibraryTop")
        .frame()
    else {
        panic!("expected LibraryTop frame");
    };
    let domain: BTreeSet<ObjectId> = frame.eligible_results().iter().copied().collect();
    assert_eq!(
        domain,
        BTreeSet::from([id10, id11]),
        "ExactManaCostOneOf([{{0}}, {{1}}]) must match only printed {{0}} and {{1}} artifacts"
    );
    assert!(!domain.contains(&id12), "{{W}} artifact must be excluded");
    assert!(!domain.contains(&id13), "{{X}} artifact must be excluded");
    assert!(
        !domain.contains(&id14),
        "no-mana-cost artifact land must be excluded"
    );

    let ready = pending
        .state()
        .supply_entropy(pending.state().basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let after = reply(
        &ready,
        P0,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(id11),
        }),
    );
    assert_eq!(zone_of(after.state(), 11), ObjectZone::Battlefield);
}

// ---------------------------------------------------------------------------
// 7. StaticPart::ModifyPTPerOwnColor (per-recipient Layer 7c scaling)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 8. Boast + ActivatedAbilityCost (CR 118.7a floor) + target cost reduction
// ---------------------------------------------------------------------------

#[test]
fn boast_and_activated_ability_cost_with_cr118_7a_floor_and_target_discount() {
    let vocab = vocabulary();

    // Boast creature with Haste: Boast {2} -> gain 2 life.
    let mut boaster = vanilla_creature(2, 2);
    boaster.printed_keywords.insert(KeywordAbility::Haste);
    boaster.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::generic(2.into()),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(2),
            }],
        ),
    });
    boaster
        .activation_restrictions
        .insert(ActivatedAbilityId(1), ActivationRestriction::Boast);
    let boaster = validated(boaster, &vocab);

    // Creature with two abilities to verify CR 118.7a minimum_total_mana: 1 floor:
    // Ability 1: {1} -> gain 1 life (must stay {1}, NEVER reduced to {0}!).
    // Ability 2: {1}{U} -> gain 3 life (reduces to {U}, total mana == 1!).
    let mut mage = vanilla_creature(1, 3);
    mage.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::generic(1.into()),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    });
    mage.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: ManaCost {
            generic: 1.into(),
            required: BTreeMap::from([(ManaType::Blue, 1.into())]),
        },
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    let mage = validated(mage, &vocab);

    // Training Grounds style static reducer: activated abilities of creatures you control cost {2} less,
    // minimum_total_mana: 1 (CR 118.7a).
    let mut training_grounds = Program::enchantment(None);
    training_grounds
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Modifier {
            id: StaticAbilityId(1),
            rule: StaticModifierRule::ActivatedAbilityCost {
                scope: ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::creature())),
                controller: PlayerRelation::Controller,
                delta: 2,
                less: true,
                scale: None,
                minimum_total_mana: 1,
            },
        });
    let training_grounds = validated(training_grounds, &vocab);

    // Ghostfire Blade style Equipment: Equip {3}, costs {2} less targeting a colorless creature.
    let mut ghostfire_blade = Program::equipment(None, ManaCost::generic(3.into()));
    let colorless_creature = ObjectFilter::Clause(ObjectClause {
        color: ObjectColorFilter::none_of(ColorMask::ALL),
        ..ObjectClause::creature()
    });
    ghostfire_blade
        .activation_target_cost_reductions
        .insert(ActivatedAbilityId(1), (colorless_creature, 2));
    let ghostfire_blade = validated(ghostfire_blade, &vocab);

    let colorless_target = validated(vanilla_creature(1, 1), &vocab);
    let colored_target = validated(with_colors(vanilla_creature(2, 2), &[Color::Red]), &vocab);
    let bank = validated(
        mana_rock(&[(ManaType::Colourless, 8), (ManaType::Blue, 2)]),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, boaster, true),
            card(2, mage, true),
            card(3, training_grounds, true),
            card(4, ghostfire_blade, true),
            card(5, colorless_target, true),
            card(6, colored_target, true),
            card(7, bank, true),
        ],
        vec![
            CardId(1),
            CardId(2),
            CardId(3),
            CardId(4),
            CardId(5),
            CardId(6),
            CardId(7),
        ],
    );

    let boaster_id = object(&game, 1);
    let mage_id = object(&game, 2);
    let blade_id = object(&game, 4);
    let colorless_id = object(&game, 5);
    let colored_id = object(&game, 6);

    let with_mana = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 7),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
    .into_state();

    // Part A1: Boast BEFORE attacking must fail with ActivationConditionNotMet.
    assert_eq!(
        attempt(
            &with_mana,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: boaster_id,
                ability: ActivatedAbilityId(1),
                payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet)),
        "CR 702.142: Boast cannot be activated before the creature has attacked this turn"
    );

    // Part B1: CR 118.7a floor keeps {1} ability at {1} (free activation must fail!).
    assert!(
        attempt(
            &with_mana,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: mage_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "CR 118.7a: minimum_total_mana: 1 must not reduce {{1}} to {{0}}"
    );
    let act_one = submit(
        &with_mana,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: mage_id,
            ability: ActivatedAbilityId(1),
            payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
        },
    );
    let after_one = pass_pair(act_one.state());
    assert_eq!(
        after_one.state().execution().life_total(P0),
        Some(&21.into())
    );

    // Part B2: {1}{U} ability reduces to {U} (colored total = 1 satisfies minimum_total_mana: 1!).
    let act_u = submit(
        after_one.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: mage_id,
            ability: ActivatedAbilityId(2),
            payment: Some(mana_payment(&[(ManaType::Blue, 1)])),
        },
    );
    let after_u = pass_pair(act_u.state());
    assert_eq!(after_u.state().execution().life_total(P0), Some(&24.into()));

    // Part C: Ghostfire Blade target cost reduction.
    // Equip targeting colored creature costs full {3} (paying {1} fails!).
    assert!(
        attempt(
            after_u.state(),
            P0,
            GameCommand::ActivateAbility {
                source: blade_id,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(colored_id),
                payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
            },
        )
        .is_err(),
        "colored target must not receive colorless-creature equip discount"
    );
    // Equip targeting colorless creature costs only {1} ({3} - {2} = {1})!
    let equip_colorless = submit(
        after_u.state(),
        P0,
        GameCommand::ActivateAbility {
            source: blade_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(colorless_id),
            payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
        },
    );
    let after_equip = pass_pair(equip_colorless.state());
    assert_eq!(
        current(after_equip.state(), blade_id).attached_to,
        Some(TargetRef::Object(colorless_id)),
        "Ghostfire Blade must attach to colorless creature for discounted {{1}}"
    );

    // Part A2: Move to DeclareAttackers and attack with boaster (full post-attack Boast activation
    // and once-per-turn cap are verified in the dedicated test immediately below).
    let beg = pass_pair(after_equip.state()).into_state();
    let decl = pass_pair(&beg).into_state();
    let after_attack = reply(
        &decl,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(boaster_id, P1)]),
    );
    assert_eq!(
        after_attack.state().turn().unwrap().position().step,
        Step::DeclareAttackers
    );
}

#[test]
fn boast_activates_once_after_attacking_and_rejects_second_activation() {
    let vocab = vocabulary();

    let mut boaster = vanilla_creature(2, 2);
    boaster.printed_keywords.insert(KeywordAbility::Haste);
    boaster.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::generic(2.into()),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(2),
            }],
        ),
    });
    boaster
        .activation_restrictions
        .insert(ActivatedAbilityId(1), ActivationRestriction::Boast);
    let boaster = validated(boaster, &vocab);

    // Boast-scoped cost reducer (Birgi / Arni style): boast abilities cost {1} less.
    let mut boast_reducer = Program::enchantment(None);
    boast_reducer
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Modifier {
            id: StaticAbilityId(1),
            rule: StaticModifierRule::ActivatedAbilityCost {
                scope: ActivatedCostScope::Boast,
                controller: PlayerRelation::Controller,
                delta: 1,
                less: true,
                scale: None,
                minimum_total_mana: 0,
            },
        });
    let boast_reducer = validated(boast_reducer, &vocab);

    let bank = validated(mana_rock(&[(ManaType::Colourless, 4)]), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, boaster, true),
            card(2, boast_reducer, true),
            card(3, bank, true),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let boaster_id = object(&game, 1);

    let beg = pass_pair(&game).into_state();
    let decl = pass_pair(&beg).into_state();
    let attacked = reply(
        &decl,
        P0,
        DecisionReply::DeclareAttackers(vec![attack_player(boaster_id, P1)]),
    );
    let with_mana = submit(
        attacked.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(attacked.state(), 3),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );

    // First Boast activation after attacking succeeds for discounted {1} ({2} - {1}).
    let boast1 = submit(
        with_mana.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: boaster_id,
            ability: ActivatedAbilityId(1),
            payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
        },
    );
    let resolved1 = pass_pair(boast1.state());
    assert_eq!(
        resolved1.state().execution().life_total(P0),
        Some(&22.into())
    );

    // Second Boast activation on the same turn must be rejected per CR 702.142!
    assert_eq!(
        attempt(
            resolved1.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: boaster_id,
                ability: ActivatedAbilityId(1),
                payment: Some(mana_payment(&[(ManaType::Colourless, 1)])),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet)),
        "CR 702.142: Boast ability can only be activated once each turn"
    );
}

// ---------------------------------------------------------------------------
// 9. SpellCostModifier::KeywordCastCost (Dash & Flashback)
// ---------------------------------------------------------------------------

#[test]
fn keyword_cast_cost_modifier_reduces_dash_or_flashback() {
    let vocab = vocabulary();

    // Warbringer-style permanent: Dash costs you pay cost {2} less; Flashback costs cost {2} less.
    let mut warbringer = vanilla_creature(3, 3);
    warbringer
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(1),
            modifier: SpellCostModifier::KeywordCastCost {
                keyword: CastKeywordCostKind::Dash,
                controller: PlayerRelation::Controller,
                delta: 2,
                less: true,
            },
        });
    warbringer
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(2),
            modifier: SpellCostModifier::KeywordCastCost {
                keyword: CastKeywordCostKind::Flashback,
                controller: PlayerRelation::Controller,
                delta: 2,
                less: true,
            },
        });
    let warbringer = validated(warbringer, &vocab);

    let dash_cost = ManaCost {
        generic: 2.into(),
        required: BTreeMap::from([(ManaType::Red, 1.into())]),
    };
    let normal_cost = ManaCost {
        generic: 3.into(),
        required: BTreeMap::from([(ManaType::Red, 1.into())]),
    };
    let dasher = validated(
        Program::creature(Some(normal_cost), 2.into(), 2.into(), BTreeSet::new())
            .with_end_step_cast(EndStepCast::Dash, dash_cost, TriggeredAbilityId(1)),
        &vocab,
    );

    let fb_cost = ManaCost {
        generic: 2.into(),
        required: BTreeMap::from([(ManaType::Blue, 1.into())]),
    };
    let mut fb_spell = Program::sorcery(
        fb_cost.clone(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    );
    fb_spell.flashback = Some(fb_cost);
    let fb_spell = validated(fb_spell, &vocab);

    let bank = validated(
        mana_rock(&[(ManaType::Red, 2), (ManaType::Blue, 2)]),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, warbringer, true),
            card(2, dasher, false),
            graveyard_of(3, P0, fb_spell.clone()),
            card(4, fb_spell, false),
            card(5, bank, true),
        ],
        vec![CardId(1), CardId(5)],
    );

    let with_mana = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 5),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
    .into_state();

    // Part A: Normal cast of dasher with {R} fails (still costs {3}{R}, not reduced by Dash discount).
    assert!(
        attempt(
            &with_mana,
            P0,
            GameCommand::CastUntargeted {
                spell: object(&with_mana, 2),
                payment: Some(mana_payment(&[(ManaType::Red, 1)])),
            },
        )
        .is_err(),
        "KeywordCastCost(Dash) must NOT reduce normal casting cost"
    );

    // Dash cast of dasher succeeds for just {R} ({2}{R} - {2} = {R}) and enters with Haste!
    let cast_dash = submit(
        &with_mana,
        P0,
        GameCommand::CastWithResources {
            spell: object(&with_mana, 2),
            target: None,
            payment: CastingPayment {
                use_alternative: true,
                mana: Some(mana_payment(&[(ManaType::Red, 1)])),
                ..CastingPayment::default()
            },
        },
    );
    let dashed_in = pass_pair(cast_dash.state());
    let dasher_id = object(dashed_in.state(), 2);
    assert_eq!(zone_of(dashed_in.state(), 2), ObjectZone::Battlefield);
    assert!(current(dashed_in.state(), dasher_id).has_keyword(KeywordAbility::Haste));

    // Part B: Normal cast of fb_spell from hand for {U} fails (still costs {2}{U}).
    assert!(
        attempt(
            dashed_in.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(dashed_in.state(), 4),
                payment: Some(mana_payment(&[(ManaType::Blue, 1)])),
            },
        )
        .is_err(),
        "KeywordCastCost(Flashback) must NOT reduce normal hand casting cost"
    );

    // Flashback cast from graveyard succeeds for just {U} ({2}{U} - {2} = {U})!
    let cast_fb = submit(
        dashed_in.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(dashed_in.state(), 3),
            payment: Some(mana_payment(&[(ManaType::Blue, 1)])),
        },
    );
    let fb_resolved = pass_pair(cast_fb.state());
    assert_eq!(
        fb_resolved.state().execution().life_total(P0),
        Some(&23.into()),
        "Flashback cast must succeed at reduced {{U}} cost and gain 3 life"
    );
    assert_eq!(zone_of(fb_resolved.state(), 3), ObjectZone::Exile);
}

// ---------------------------------------------------------------------------
// 10. DIED_THIS_TURN and PUT_INTO_GY_FROM_LIBRARY_THIS_TURN in MoveGraveyardGroup
// ---------------------------------------------------------------------------

#[test]
fn died_this_turn_and_put_into_gy_from_library_graveyard_return() {
    let vocab = vocabulary();

    let died_filter = ObjectFilter::Clause(ObjectClause {
        flags: ObjectFilterFlags::DIED_THIS_TURN,
        ..ObjectClause::creature()
    });
    let from_lib_filter = ObjectFilter::Clause(ObjectClause {
        flags: ObjectFilterFlags::PUT_INTO_GY_FROM_LIBRARY_THIS_TURN,
        ..ObjectClause::creature()
    });

    let return_died = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Group(if false {
                                    spellwake_engine::program::AffectedPlayers::EachPlayer
                                } else {
                                    spellwake_engine::program::AffectedPlayers::One(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    )
                                }),
                            )),
                            died_filter,
                        ),
                        if true {
                            spellwake_engine::toolbox::ZoneDest::Battlefield {
                                controller: spellwake_engine::toolbox::ZoneController::Player(
                                    Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    )),
                                ),
                                tapped: false,
                                attacking: false,
                                transformed: false,
                                face_down: false,
                                entering_counters: Vec::new(),
                                attach_to: None,
                            }
                        } else {
                            spellwake_engine::toolbox::ZoneDest::Hand
                        },
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let return_milled = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Group(if false {
                                    spellwake_engine::program::AffectedPlayers::EachPlayer
                                } else {
                                    spellwake_engine::program::AffectedPlayers::One(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    )
                                }),
                            )),
                            from_lib_filter,
                        ),
                        if false {
                            spellwake_engine::toolbox::ZoneDest::Battlefield {
                                controller: spellwake_engine::toolbox::ZoneController::Player(
                                    Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    )),
                                ),
                                tapped: false,
                                attacking: false,
                                transformed: false,
                                face_down: false,
                                entering_counters: Vec::new(),
                                attach_to: None,
                            }
                        } else {
                            spellwake_engine::toolbox::ZoneDest::Hand
                        },
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let mill_one = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::Mill {
                    player: ResolvingPlayer::Controller,
                    amount: ValueExpr::literal(1),
                }],
            ),
        ),
        &vocab,
    );

    let destroy = validated(destroy_permanent_instant(), &vocab);
    let bear = validated(vanilla_creature(2, 2), &vocab);

    let game = world(
        vocab,
        vec![
            graveyard_of(1, P0, bear.clone()), // Already in GY before Turn 1
            card(2, bear.clone(), true),       // Dies from battlefield this turn
            library_top(3, P0, bear),          // Milled from library this turn
            card(10, destroy, false),
            card(11, mill_one, false),
            card(12, return_died, false),
            card(13, return_milled, false),
        ],
        vec![CardId(2)],
    );

    // Destroy card 2 (battlefield -> graveyard) and mill card 3 (library -> graveyard).
    let bear2 = object(&game, 2);
    let after_destroy = cast_targeted(&game, P0, 10, TargetRef::Object(bear2));
    assert_eq!(zone_of(after_destroy.state(), 2), ObjectZone::Graveyard(P0));

    let after_mill = cast_untargeted(after_destroy.state(), 11);
    assert_eq!(zone_of(after_mill.state(), 3), ObjectZone::Graveyard(P0));

    // MoveGraveyardGroup(DIED_THIS_TURN) returns ONLY card 2 to battlefield; cards 1 and 3 stay in GY!
    let after_return_died = cast_untargeted(after_mill.state(), 12);
    assert_eq!(
        zone_of(after_return_died.state(), 2),
        ObjectZone::Battlefield,
        "creature that died this turn must return to battlefield"
    );
    assert_eq!(
        zone_of(after_return_died.state(), 1),
        ObjectZone::Graveyard(P0),
        "creature already in graveyard from earlier must NOT match DIED_THIS_TURN"
    );
    assert_eq!(
        zone_of(after_return_died.state(), 3),
        ObjectZone::Graveyard(P0),
        "creature milled from library must NOT match DIED_THIS_TURN"
    );

    // MoveGraveyardGroup(PUT_INTO_GY_FROM_LIBRARY_THIS_TURN) returns ONLY card 3 to hand; card 1 stays in GY!
    let after_return_milled = cast_untargeted(after_return_died.state(), 13);
    assert_eq!(
        zone_of(after_return_milled.state(), 3),
        ObjectZone::Hand(P0),
        "creature milled from library this turn must match PUT_INTO_GY_FROM_LIBRARY_THIS_TURN"
    );
    assert_eq!(
        zone_of(after_return_milled.state(), 1),
        ObjectZone::Graveyard(P0),
        "prior graveyard creature must NOT match PUT_INTO_GY_FROM_LIBRARY_THIS_TURN"
    );
}

// ---------------------------------------------------------------------------
// 11. SourceZoneFilter::LastMoved on ZoneCastPermission
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 12. RegenerateSubject, PermanentTargetAction::Regenerate, MatchingOtherThanTarget,
//     and TargetAndSharingColorMatching
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 13. AddManaForPlayer and CR 605.1b TriggeredManaRule::EnchantedLandTapped
// ---------------------------------------------------------------------------

#[test]
fn add_mana_for_player_and_cr605_1b_enchanted_land_tapped() {
    let vocab = vocabulary();

    // Spell 1: Target opponent adds {R}{R} (fixed).
    let gift_fixed = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOpponent,
                vec![Instruction::AddManaForPlayer {
                    player: ResolvingPlayer::TargetPlayer,
                    yield_kind: EffectManaYield::Fixed(BTreeMap::from([(ManaType::Red, 2.into())])),
                }],
            ),
        ),
        &vocab,
    );

    // Spell 2: Target opponent adds one mana of Blue or Green (interactive choice by target opponent!).
    let gift_choice = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOpponent,
                vec![Instruction::AddManaForPlayer {
                    player: ResolvingPlayer::TargetPlayer,
                    yield_kind: EffectManaYield::Choice {
                        colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
                        amount: ValueExpr::literal(1),
                    },
                }],
            ),
        ),
        &vocab,
    );

    // Wild Growth style Aura: Enchant Land, whenever enchanted land is tapped for mana, add {G}.
    let mut wild_growth = Program::aura(Some(ManaCost::free()), EnchantRestriction::Land);
    wild_growth.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Triggered(TriggeredManaRule::EnchantedLandTapped {
            extra: EnchantedLandManaBonus::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
        }),
    });
    let wild_growth = validated(wild_growth, &vocab);

    // Fertile Ground style Aura: Enchant Land, whenever enchanted land is tapped for mana, add 1 of any color.
    let mut fertile_ground = Program::aura(Some(ManaCost::free()), EnchantRestriction::Land);
    fertile_ground.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Triggered(TriggeredManaRule::EnchantedLandTapped {
            extra: EnchantedLandManaBonus::AnyColor { count: 1 },
        }),
    });
    let fertile_ground = validated(fertile_ground, &vocab);

    let forest = validated(basic_forest(), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, gift_fixed, false),
            card(2, gift_choice, false),
            card(3, wild_growth, false),
            card(4, fertile_ground, false),
            card(5, forest.clone(), true),
            card(6, forest.clone(), true),
            card(7, forest, true),
        ],
        vec![CardId(5), CardId(6), CardId(7)],
    );

    // Part A1: Fixed AddManaForPlayer credits P1 with 2 Red while P0 stays at 0.
    let after_fixed = cast_targeted(&game, P0, 1, TargetRef::Player(P1));
    assert_eq!(
        after_fixed.state().execution().mana(P1, ManaType::Red),
        2.into()
    );
    assert_eq!(
        after_fixed.state().execution().mana(P0, ManaType::Red),
        0.into()
    );

    // Part A2: Interactive AddManaForPlayer prompts P1 (NOT P0!) to choose color.
    let pending_choice = cast_targeted(after_fixed.state(), P0, 2, TargetRef::Player(P1));
    let decision = pending_choice
        .state()
        .pending_decision()
        .expect("interactive AddManaForPlayer must prompt recipient");
    assert_eq!(
        decision.chooser(),
        P1,
        "recipient P1 (not spell controller P0) must choose the mana color"
    );
    assert!(matches!(
        decision.frame(),
        RuleFrame::ResolutionManaColor(_)
    ));
    let after_choice = reply(
        pending_choice.state(),
        P1,
        DecisionReply::ManaColor(ManaType::Blue),
    );
    assert_eq!(
        after_choice.state().execution().mana(P1, ManaType::Blue),
        1.into()
    );
    assert_eq!(
        after_choice.state().execution().mana(P0, ManaType::Blue),
        0.into()
    );

    // Part B1: Cast Wild Growth onto Forest 5, then tap Forest 5 -> immediate +2 Green with empty stack!
    let f5 = object(after_choice.state(), 5);
    let enchanted5 = cast_targeted(after_choice.state(), P0, 3, TargetRef::Object(f5));
    let tapped5 = submit(
        enchanted5.state(),
        P0,
        GameCommand::ActivateMana {
            source: f5,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(tapped5.state().pending_decision().is_none());
    assert!(
        tapped5.state().execution().window().entries().is_empty(),
        "CR 605.1b: EnchantedLandTapped(Fixed) must resolve immediately without using the stack"
    );
    assert_eq!(
        tapped5.state().execution().mana(P0, ManaType::Green),
        2.into()
    );

    // Tap unenchanted Forest 7 -> only +1 Green (total 3).
    let f7 = object(tapped5.state(), 7);
    let tapped7 = submit(
        tapped5.state(),
        P0,
        GameCommand::ActivateMana {
            source: f7,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(tapped7.state().pending_decision().is_none());
    assert_eq!(
        tapped7.state().execution().mana(P0, ManaType::Green),
        3.into()
    );

    // Part B2: Cast Fertile Ground onto Forest 6, tap Forest 6 -> stackless ManaColor prompt, choose Black!
    let f6 = object(tapped7.state(), 6);
    let enchanted6 = cast_targeted(tapped7.state(), P0, 4, TargetRef::Object(f6));
    let tapped6 = submit(
        enchanted6.state(),
        P0,
        GameCommand::ActivateMana {
            source: f6,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(
        tapped6.state().execution().window().entries().is_empty(),
        "CR 605.1b: EnchantedLandTapped(AnyColor) must remain stackless"
    );
    let mana_decision = tapped6
        .state()
        .pending_decision()
        .expect("EnchantedLandTapped(AnyColor) must prompt stackless ManaColor");
    assert!(matches!(mana_decision.frame(), RuleFrame::ManaColor(_)));
    let after_fertile = reply(
        tapped6.state(),
        P0,
        DecisionReply::ManaColor(ManaType::Black),
    );
    assert!(
        after_fertile
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
    assert_eq!(
        after_fertile.state().execution().mana(P0, ManaType::Green),
        4.into()
    );
    assert_eq!(
        after_fertile.state().execution().mana(P0, ManaType::Black),
        1.into()
    );
}

// ---------------------------------------------------------------------------
// 14. StatusTriggerKind::Fought + RulesOccurrence::PermanentFought (0-power fight)
// ---------------------------------------------------------------------------

#[test]
fn permanent_fought_trigger_fires_with_zero_power_creature() {
    let vocab = vocabulary();

    // 0/4 creature with "Whenever this creature fights, you gain 3 life."
    let mut zero_fighter = vanilla_creature(0, 4);
    zero_fighter.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::StatusChanged {
            subject: ObjectFilter::self_object(),
            kind: StatusTriggerKind::Fought,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    let zero_fighter = validated(zero_fighter, &vocab);

    let opp_wall = validated(vanilla_creature(0, 4), &vocab);

    let fight_spell = validated(
        Program::sorcery(
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
                    Instruction::FightFixedTargets,
                ],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, zero_fighter, true),
            opp_battlefield(2, opp_wall),
            card(3, fight_spell, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    let f1 = object(&game, 1);
    let f2 = object(&game, 2);

    // 0/4 fights 0/4: zero damage is dealt, yet CR 701.12a/b says both creatures fought!
    let after_fight = cast_fixed_targets(
        &game,
        P0,
        3,
        vec![TargetRef::Object(f1), TargetRef::Object(f2)],
    );
    assert!(
        all_occurrences(&after_fight).iter().any(|occ| matches!(
            occ,
            RulesOccurrence::PermanentFought { object, opponent }
                if *object == f1 && *opponent == f2
        )),
        "CR 701.12a/b: Fight must emit PermanentFought even when both fighters have 0 power"
    );
    assert_eq!(
        after_fight.state().execution().window().entries().len(),
        1,
        "StatusTriggerKind::Fought trigger must stack after 0-power fight"
    );
    let after_trigger = pass_pair(after_fight.state());
    assert_eq!(
        after_trigger.state().execution().life_total(P0),
        Some(&23.into()),
        "Fought trigger must resolve and gain 3 life"
    );
}

// ---------------------------------------------------------------------------
// 15. AttachChoosingPermanent, AttachFixedTargets, AttachPermanentToPlayer
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 16. EntryCopySpecExt (OpponentGraveyard, enters_tapped, extra_mana,
//     DonorControlledByController, SourceNotOwnedByController)
// ---------------------------------------------------------------------------

#[test]
fn entry_copy_opponent_graveyard_enters_tapped_and_extra_mana() {
    let vocab = vocabulary();

    // Part A: Clone copying from OpponentGraveyard, entering tapped with an extra {B} mana ability.
    let mut opp_gy_spec =
        EntryCopySpecExt::new(EntryCopyZone::OpponentGraveyard, ObjectFilter::creature());
    opp_gy_spec.enters_tapped = true;
    opp_gy_spec.extra_mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Black, 1.into())])),
    });
    let mut gy_clone = vanilla_creature(0, 0);
    gy_clone.entry_copy = Some(EntryCopyRule::MatchingExt(Box::new(opp_gy_spec)));
    let gy_clone = validated(gy_clone, &vocab);

    // Part B: Clone copying from Battlefield with conditional +1/+1 counters when
    // SourceStateExpr::DonorControlledByController == 1.
    let mut bf_spec = EntryCopySpecExt::new(EntryCopyZone::Battlefield, ObjectFilter::creature());
    bf_spec.conditional_counters.push((
        ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::DonorControlledByController),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        },
        CounterKind::PlusOne,
        2,
    ));
    let mut bf_clone = vanilla_creature(0, 0);
    bf_clone.entry_copy = Some(EntryCopyRule::MatchingExt(Box::new(bf_spec)));
    let bf_clone = validated(bf_clone, &vocab);

    let own_gy_bear = validated(named(vanilla_creature(2, 2), "Own GY Bear"), &vocab);
    let opp_gy_giant = validated(named(vanilla_creature(4, 4), "Opp GY Giant"), &vocab);
    let own_bf_bear = validated(named(vanilla_creature(2, 2), "Own BF Bear"), &vocab);
    let opp_bf_bear = validated(named(vanilla_creature(2, 2), "Opp BF Bear"), &vocab);
    let forest = validated(basic_forest(), &vocab);

    let mut seeds = vec![
        graveyard_of(1, P0, own_gy_bear),
        graveyard_of(2, P1, opp_gy_giant),
        card(3, own_bf_bear, true),
        opp_battlefield(4, opp_bf_bear),
        card(10, gy_clone, false),
        card(11, bf_clone.clone(), false),
        card(12, bf_clone, false),
    ];
    for i in 0..6 {
        seeds.push(library_top(100 + i, P0, forest.clone()));
        seeds.push(library_top(200 + i, P1, forest.clone()));
    }

    let game = world(vocab.clone(), seeds, vec![CardId(3), CardId(4)]);
    let own_gy_id = object(&game, 1);
    let opp_gy_id = object(&game, 2);
    let own_bf_id = object(&game, 3);
    let opp_bf_id = object(&game, 4);

    // Cast gy_clone: donor list must contain ONLY opp_gy_id (NOT own_gy_id)!
    let pending_gy = cast_untargeted(&game, 10);
    let RuleFrame::ZoneReplacement(frame) = pending_gy
        .state()
        .pending_decision()
        .expect("OpponentGraveyard EntryCopy must prompt")
        .frame()
    else {
        panic!("expected ZoneReplacement frame");
    };
    let ZoneReplacementRequest::EntryCopy { donors, .. } = frame.request() else {
        panic!("expected EntryCopy request");
    };
    assert_eq!(
        donors,
        &vec![opp_gy_id],
        "EntryCopyZone::OpponentGraveyard must exclude controller's own graveyard"
    );
    assert!(!donors.contains(&own_gy_id));

    let after_gy = reply(
        pending_gy.state(),
        P0,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(Some(opp_gy_id))),
    );
    let gy_clone_id = object(after_gy.state(), 10);
    assert_eq!(zone_of(after_gy.state(), 10), ObjectZone::Battlefield);
    assert_eq!(
        is_tapped(after_gy.state(), 10),
        Some(true),
        "enters_tapped: true on EntryCopySpecExt must cause clone to enter tapped"
    );
    assert_eq!(
        current(after_gy.state(), gy_clone_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
    let granted_mana_id = *current(after_gy.state(), gy_clone_id)
        .abilities
        .mana
        .first()
        .expect("extra_mana must grant a printed mana ability to the copy");

    // Part B1: Copy own battlefield creature -> DonorControlledByController == 1 -> +2 +1/+1 counters (4/4)!
    let pending_own = cast_untargeted(after_gy.state(), 11);
    let after_own = reply(
        pending_own.state(),
        P0,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(Some(own_bf_id))),
    );
    let clone_own_id = object(after_own.state(), 11);
    assert_eq!(
        plus_one_counters(after_own.state(), clone_own_id),
        2,
        "DonorControlledByController must evaluate to 1 when copying a controlled donor"
    );
    assert_eq!(
        current(after_own.state(), clone_own_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );

    // Part B2: Copy opponent's battlefield creature -> DonorControlledByController == 0 -> 0 counters (2/2)!
    let pending_opp = cast_untargeted(after_own.state(), 12);
    let after_opp = reply(
        pending_opp.state(),
        P0,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(Some(opp_bf_id))),
    );
    let clone_opp_id = object(after_opp.state(), 12);
    assert_eq!(
        plus_one_counters(after_opp.state(), clone_opp_id),
        0,
        "DonorControlledByController must evaluate to 0 when copying an opponent's donor"
    );
    assert_eq!(
        current(after_opp.state(), clone_opp_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );

    // Advance to P0's next turn (Turn 3) so gy_clone untaps and can tap for its extra_mana {B}!
    let turn2 = advance_to_next_turn_main(after_opp.into_state());
    let turn3 = advance_to_next_turn_main(turn2);
    assert_eq!(turn3.execution().window().active_player(), P0);
    assert_eq!(is_tapped(&turn3, 10), Some(false));
    let tapped_extra = submit(
        &turn3,
        P0,
        GameCommand::ActivateMana {
            source: gy_clone_id,
            ability: ManaAbilityRef::Printed(granted_mana_id),
        },
    );
    assert_eq!(
        tapped_extra.state().execution().mana(P0, ManaType::Black),
        1.into(),
        "extra_mana granted on EntryCopySpecExt must tap to produce {{B}}"
    );

    // Part C: TriggerCondition::SourceNotOwnedByController fires ONLY for stolen permanent!
    let mut stolen_watcher = vanilla_creature(2, 2);
    stolen_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec {
            event: TriggerEvent::StepBegan {
                step: Step::Upkeep,
                player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
            },
            zone: TriggerZone::Battlefield,
            intervening_if: Some(TriggerCondition::SourceNotOwnedByController),
        },
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    let stolen_watcher = validated(stolen_watcher, &vocab);

    // Seed both an owned copy (owner P0, controller P0) and a stolen copy (owner P1, controller P0)
    // and check Upkeep trigger count!
    let upkeep_game = world_at_upkeep(
        vocab,
        vec![
            controlled(1, P0, P0, stolen_watcher.clone()),
            controlled(2, P1, P0, stolen_watcher),
        ],
        vec![CardId(1), CardId(2)],
    );
    assert_eq!(
        upkeep_game.execution().window().entries().len(),
        1,
        "SourceNotOwnedByController must trigger ONLY for the P1-owned permanent controlled by P0"
    );
    let after_upkeep = pass_pair(&upkeep_game);
    assert_eq!(
        after_upkeep.state().execution().life_total(P0),
        Some(&23.into()),
        "only the stolen permanent's upkeep trigger must resolve (+3 life, not +6)"
    );
}
