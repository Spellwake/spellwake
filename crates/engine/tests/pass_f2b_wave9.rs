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

// ---------------------------------------------------------------------------
// 3. Instruction::Earthbend and Instruction::ReturnEarthbentCard
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 4. Instruction::SkipNextTurns (CR 614.10 & CR 614.10a)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. Instruction::SelectExtremeLifePlayer (winner, ControllerChooses, Skip, DrawGame)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 9. SpellCostModifier::KeywordCastCost (Dash & Flashback)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 15. AttachChoosingPermanent, AttachFixedTargets, AttachPermanentToPlayer
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 16. EntryCopySpecExt (OpponentGraveyard, enters_tapped, extra_mana,
//     DonorControlledByController, SourceNotOwnedByController)
// ---------------------------------------------------------------------------
