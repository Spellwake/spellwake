#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Runtime integration tests for Pass F2-B Wave 7 primitives:
//! - `ForEachPlayerQuery` + `ResolvingPlayer::LoopPlayer` (APNAP order)
//! - `OfferAnyPlayer` (accepting player runs `cost_body` + `paid_body`, or `declined_body` if all decline)
//! - `DealDamageToAnnouncedTargets` (multi-target equal damage) and bounded multi-target damage
//! - `UntapOrTapSelect` (non-targeted selection, exact count and `up_to: true`)
//! - `GrantChosenProtection` (`UntilControllerNextTurn`) and `ResolvedPartSpec::{ChosenColor,
//!   ChosenBasicLandType, ProtectionFromEachOpponent, CannotBeTargetedByColors}`
//! - `RemoveCountersFromSubject` (specific kind, any kind, remove all) and
//!   bounded multi-target counter removal
//! - `MoveGroupToLibrary` (`LibraryPlacement::BottomRandom`) and `MoveGraveyardGroupOwned`
//!   (`under_owner_control`)
//! - `ExileLibraryTopLinked` + `CastPermissionRule::ExileMatchingExt` (Nightveil Specter flow)
//! - `ZoneOpSpec::look_top_choose_instructions` (`keep_dest: LibraryTop` and `rest_dest: Hand`) and `ZoneOpSpec::reveal_until_match_instructions`
//! - `CreateTokenCopyExt` (nonlegendary, override P/T, extra keywords, extra colors)
//! - `Program.activation_cost_reductions`, `ActivationRestriction::{XNonZero, OnlySpendManaType}`,
//!   and `ExtraActivationCost::{PaySnowMana, ExileTopLibrary, RevealSource, SacrificeAnyNumber}`
//! - `StaticModifierRule::{CrewAndSaddlePowerBonus, ModifyMaxHandSize, ConvertUnspentMana}` and
//!   `StaticRestriction::SkipStep`
//! - `CastPermissionRule::GrantSpellKeyword` (Flash on creature spells) and
//!   `SpellCostModifier::FirstSpellRule`
//! - `AdditionalCastCost::KickerExtra`, `ConditionalCastCost::Alternative`,
//!   `GraveyardSelfCast::GraveyardOrExile`, and `CastTimingRestriction::{XNonZero, AfterCombat}`
//! - `StaticDamageOp::{PreventUpToExpr, PreventAndFollowup}`,
//!   `StaticDamageRule::ConditionalModifyDamage`, and `EventValueKind::DamagePreventedThisWay`
//! - `ZoneReplacementSubject::{SourceSelfDiscardByOpponent, MatchingNotCast}` and
//!   `Program.zone_replacement_extras` (`exile_counters`)
//! - `StaticModifierRule::{ModifyTokensFiltered, StealCreatedTokens}` (including `TemporaryStatic`)
//! - `EntryCopyRule::MatchingExt` (`keep_name`, `extra_keywords`, `conditional_counters`)
//! - Turn/provenance trackers: `SpellsCastMatchingThisTurn`, `ActivatedGraveyardAbilityThisTurn`,
//!   `SourceStateExpr::{WasCast, EnteredFromZone}`, `MatchObjectRef::LibraryTop`, and
//!   `ControlsGreatestStat`
//! - Wave 7 triggers (`DealsAtLeastDamage`, `EntersWithoutBeingPlayed`, `BlockedByAtLeast`) and
//!   validation fixes (`ObjectFilter::uses_target_permanent`, CFG `FlowFacts` promotion across
//!   `OptionalEffect` + `BranchIfFalse(LastActionSucceeded >= 1)`, `OptionalEffect` wrapping
//!   `ExileResolvingSpell`).
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
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationOrigin, ActivationRestriction,
        ExtraActivationCost, LifeCostAmount, PowerAbility, SourceCost,
    },
    characteristics::{
        CastPermissionRule, Color, ControllerSelector, DamagePreventFollowup, DamagePreventMode,
        KeywordAbility, PermanentSelector, PowerToughness, ProtectionFromQuality, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec, SpellCostModifier, StaticAbilityId,
        StaticContinuousAbility, StaticDamageKind, StaticDamageOp, StaticDamageRecipient,
        StaticDamageRule, StaticModifierRule, StaticRestriction, StaticSourceCondition,
    },
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    effects::{
        CounterKind, EffectError, LibraryTopReply, OptionalEffectChoice, PileChoice, SeedCard,
        ZoneReplacementReply, ZoneReplacementRequest,
    },
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::SeatId,
    program::{
        AdditionalCastCost, AffectedPlayers, BlockTriggerKind, BodyTarget, CardType,
        CastTimingRestriction, ColorMask, Comparison, ConditionalCastCost, DamageTriggerRecipient,
        DynamicStatAnchor, EffectSubject, EntryCopyRule, EntryCopySpecExt, EntryCopyZone,
        EventValueKind, ExecutableBody, GraveyardSelfCast, Instruction, LibraryPlacement,
        LinkedExileGroup, LookTopKeepDest, LookTopRestDest, MatchObjectRef, MoveSubjectRef,
        ObjectColorFilter, ObjectFilter, ObjectStatKind, PlayerRelation, PlayerStatExpr,
        PowerFilter, Program, ProgramKind, ProtectionRecipients, PublicZoneDestination,
        ResolutionCondition, ResolvingPlayer, SourceStateExpr, SourceZoneFilter, SpellFilter,
        Supertype, TemporaryRuleKind, TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId, ValueExpr, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementExtra, ZoneReplacementSubject,
    },
    targets::TargetRef,
    token::{
        AttackingDefenderScope, TokenCreation, TokenDefinition, TokenName, TokenRecipients,
        TokenSubtype,
    },
    turn::Step,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU8,
};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 7 test program must validate");
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

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
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
            BodyTarget::SINGLE_ANY_TARGET,
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

fn colored_rock(mana_type: ManaType, amount: i64) -> Program {
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(mana_type, amount.into())])),
    });
    rock
}

fn colorless_rock(amount: i64) -> Program {
    colored_rock(ManaType::Colourless, amount)
}

fn snow_rock(mana_type: ManaType, amount: i64) -> Program {
    let mut rock = colored_rock(mana_type, amount);
    rock.supertypes.insert(Supertype::Snow);
    rock
}

fn mana_payment(mana_type: ManaType, amount: i64) -> ManaPayment {
    ManaPayment {
        from_lots: BTreeMap::new(),
        spend: vec![ManaSpend {
            mana_type,
            amount: amount.into(),
        }],
    }
}

fn colorless_payment(amount: i64) -> ManaPayment {
    mana_payment(ManaType::Colourless, amount)
}

fn basic_forest() -> Program {
    Program::land(BTreeSet::from([LandType::Forest]), vec![])
}

fn soldier_token(vocab: &CreatureTypeVocabulary) -> TokenDefinition {
    let soldier = vocab.resolve_label("Soldier").unwrap();
    let body = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([soldier]));
    TokenDefinition::new(
        body,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(soldier)],
        },
        vocab,
    )
    .unwrap()
}

fn relic_artifact_token(vocab: &CreatureTypeVocabulary) -> TokenDefinition {
    let body = Program::artifact(None, vec![]);
    TokenDefinition::new(
        body,
        TokenName::Specified(EnglishCardName::new("Relic".into()).unwrap()),
        vocab,
    )
    .unwrap()
}

fn answer_yes_no_offer(
    game: &GameState,
    expected_chooser: SeatId,
    accept: bool,
) -> NativeCandidate {
    let decision = game
        .pending_decision()
        .expect("expected offer accept/decline decision");
    assert_eq!(decision.chooser(), expected_chooser);
    let choice = match decision.frame() {
        RuleFrame::OptionalEffect(_) => DecisionReply::OptionalEffect(if accept {
            OptionalEffectChoice::Take
        } else {
            OptionalEffectChoice::Decline
        }),
        RuleFrame::OptionIndex(frame) => {
            let labels = frame.labels();
            let decline_idx = labels
                .iter()
                .position(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower.contains("decline") || lower == "no" || lower.contains("pass")
                })
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(1);
            let accept_idx = labels
                .iter()
                .position(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower.contains("accept")
                        || lower.contains("pay")
                        || lower == "yes"
                        || lower.contains("take")
                })
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(0);
            DecisionReply::OptionIndex(if accept { accept_idx } else { decline_idx })
        }
        RuleFrame::PromptChoice(frame) => {
            let spellwake_engine::toolbox::ResolvedPromptDomain::Options { labels, .. } =
                &frame.domain
            else {
                panic!("unexpected PromptChoice offer domain: {:?}", frame.domain);
            };
            let decline_idx = labels
                .iter()
                .position(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower.contains("decline") || lower == "no" || lower.contains("pass")
                })
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(1);
            let accept_idx = labels
                .iter()
                .position(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower.contains("accept")
                        || lower.contains("pay")
                        || lower == "yes"
                        || lower.contains("take")
                })
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(0);
            DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Options(
                vec![if accept { accept_idx } else { decline_idx }],
            ))
        }
        other => panic!("unexpected offer decision frame: {other:?}"),
    };
    reply(game, expected_chooser, choice)
}

fn answer_color_choice(
    game: &GameState,
    expected_chooser: SeatId,
    color: Color,
) -> NativeCandidate {
    let decision = game
        .pending_decision()
        .expect("expected color choice decision");
    assert_eq!(decision.chooser(), expected_chooser);
    let (mana_type, option_idx) = match color {
        Color::White => (ManaType::White, 0_u8),
        Color::Blue => (ManaType::Blue, 1_u8),
        Color::Black => (ManaType::Black, 2_u8),
        Color::Red => (ManaType::Red, 3_u8),
        Color::Green => (ManaType::Green, 4_u8),
    };
    let choice = match decision.frame() {
        RuleFrame::ResolutionManaColor(_) => DecisionReply::ManaColor(mana_type),
        RuleFrame::OptionIndex(_) => DecisionReply::OptionIndex(option_idx),
        RuleFrame::PromptChoice(frame) => match &frame.domain {
            spellwake_engine::toolbox::ResolvedPromptDomain::Color { .. } => {
                DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Color(
                    mana_type,
                ))
            }
            spellwake_engine::toolbox::ResolvedPromptDomain::Options { .. } => {
                DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Options(
                    vec![option_idx],
                ))
            }
            other => panic!("unexpected PromptChoice color choice domain: {other:?}"),
        },
        other => panic!("unexpected color choice frame: {other:?}"),
    };
    reply(game, expected_chooser, choice)
}

fn answer_look_pick(
    game: &GameState,
    expected_chooser: SeatId,
    chosen: ObjectId,
) -> NativeCandidate {
    let decision = game
        .pending_decision()
        .expect("expected look-top selection decision");
    assert_eq!(decision.chooser(), expected_chooser);
    let choice = match decision.frame() {
        RuleFrame::PromptChoice(frame)
            if matches!(
                frame.domain,
                spellwake_engine::toolbox::ResolvedPromptDomain::Objects { .. }
            ) =>
        {
            DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::Objects(
                vec![chosen],
            ))
        }
        RuleFrame::RevealedPiles(_) | RuleFrame::ResolutionSelect(_) => {
            DecisionReply::SelectResolution(vec![chosen])
        }
        RuleFrame::LibraryTop(_) => DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(chosen),
        }),
        other => panic!("unexpected look-top selection frame: {other:?}"),
    };
    let mut current_cand = reply(game, expected_chooser, choice);
    while let Some(next_decision) = current_cand.state().pending_decision() {
        let actor = next_decision.chooser();
        let next_choice = match next_decision.frame() {
            RuleFrame::RevealedPiles(frame) => match frame.choice() {
                PileChoice::BottomOrder(domain) | PileChoice::GraveyardOrder(domain) => {
                    DecisionReply::OrderResolutionGraveyard(domain.clone())
                }
                other => panic!("unexpected follow-up pile choice: {other:?}"),
            },
            RuleFrame::ResolutionGraveyardOrder(frame) => {
                DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec())
            }
            other => panic!("unexpected follow-up decision after look-top pick: {other:?}"),
        };
        current_cand = reply(current_cand.state(), actor, next_choice);
    }
    current_cand
}

fn advance_to_postcombat_main(game: &GameState, actors: [SeatId; 2]) -> GameState {
    assert_eq!(game.turn().unwrap().position().step, Step::PrecombatMain);
    let beg_combat = pass_in_order(game, actors).into_state();
    assert_eq!(
        beg_combat.turn().unwrap().position().step,
        Step::BeginningCombat
    );
    let mut dec_att = pass_in_order(&beg_combat, actors).into_state();
    assert_eq!(
        dec_att.turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    if let Some(decision) = dec_att.pending_decision() {
        assert!(matches!(
            decision.frame(),
            RuleFrame::Combat(CombatDecision::Attackers(_))
        ));
        dec_att = reply(&dec_att, actors[0], DecisionReply::DeclareAttackers(vec![])).into_state();
    }
    let end_combat = pass_in_order(&dec_att, actors).into_state();
    assert_eq!(end_combat.turn().unwrap().position().step, Step::EndCombat);
    let postcombat = pass_in_order(&end_combat, actors).into_state();
    assert_eq!(
        postcombat.turn().unwrap().position().step,
        Step::PostcombatMain
    );
    postcombat
}

fn advance_to_p1_upkeep_p0_priority(game: &GameState) -> NativeCandidate {
    let end = reach_end(game, [P0, P1]);
    let t2_upkeep = pass_pair(&end);
    assert_eq!(t2_upkeep.state().turn().unwrap().position().number, 2);
    assert_eq!(
        t2_upkeep.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    submit(t2_upkeep.state(), P1, GameCommand::Pass)
}

// ============================================================================
// 1. ForEachPlayerQuery + ResolvingPlayer::LoopPlayer
// ============================================================================

// ============================================================================
// 2. OfferAnyPlayer
// ============================================================================

// ============================================================================
// 3. DealDamageToAnnouncedTargets & bounded multi-target DealDamage
// ============================================================================

#[test]
fn deal_damage_to_announced_targets_and_permanent_target_action_damage() {
    let vocab = vocabulary();

    let twin_bolt = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_ANY_TARGET,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                        ValueExpr::literal(2),
                        1,
                        2,
                    ),
                ))],
            ),
        ),
        &vocab,
    );
    let dual_searing = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            ValueExpr::literal(3),
                        ),
                        exile_if_dies: false,
                    },
                ))],
            )
            .with_permanent_target_bounds(1, 2),
        ),
        &vocab,
    );
    let giant = || {
        validated(
            Program::creature(None, 2.into(), 5.into(), BTreeSet::new()),
            &vocab,
        )
    };

    let g1 = giant();
    let g2 = giant();
    let game = world(
        vocab,
        vec![
            card(1, twin_bolt, false),
            card(2, dual_searing, false),
            opp_battlefield(3, g1),
            opp_battlefield(4, g2),
        ],
        vec![CardId(3), CardId(4)],
    );
    let opp_c1 = object(&game, 3);
    let opp_c2 = object(&game, 4);

    // Part A: DealDamageToAnnouncedTargets deals 2 to opp_c1 and 2 to P1 simultaneously.
    let cast_a = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 1),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(opp_c1), TargetRef::Player(P1)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_a = pass_pair(cast_a.state());
    assert_eq!(after_a.state().execution().life_total(P1), Some(&18.into()));
    assert_eq!(
        after_a
            .state()
            .execution()
            .creature(opp_c1)
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );

    // Part B: Bounded multi-target DealDamage deals 3 to both opp_c1 (lethal: 2+3=5) and opp_c2 (3).
    let cast_b = submit(
        after_a.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_a.state(), 2),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(opp_c1), TargetRef::Object(opp_c2)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_b = pass_pair(cast_b.state());
    assert_eq!(
        after_b
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_b.state(), 3))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1),
        "opp_c1 should die after taking 2 + 3 lethal damage"
    );
    assert_eq!(
        after_b
            .state()
            .execution()
            .creature(opp_c2)
            .unwrap()
            .unwrap()
            .damage,
        3.into(),
        "opp_c2 should survive with 3 damage marked"
    );
}

// ============================================================================
// 4. UntapOrTapSelect (exact count and up_to: true)
// ============================================================================

// ============================================================================
// 5. GrantChosenProtection & Wave 7 ResolvedPartSpecs
// ============================================================================

// ============================================================================
// 6. RemoveCountersFromSubject & bounded multi-target counter removal
// ============================================================================

// ============================================================================
// 7. MoveGroupToLibrary (BottomRandom) & MoveGraveyardGroupOwned
// ============================================================================

#[test]
fn move_group_to_library_bottom_random_and_move_graveyard_group_owned() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let tuck_creatures = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            ObjectFilter::creature(),
                        ),
                        spellwake_engine::toolbox::ZoneDest::Library {
                            position: spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                LibraryPlacement::BottomRandom,
                            ),
                        },
                    ),
                ))],
            ),
        ),
        &vocab,
    );
    let mass_reanimate = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Group(
                                    AffectedPlayers::EachPlayer,
                                ),
                            )),
                            ObjectFilter::creature(),
                        ),
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
                            tapped: true,
                            attacking: false,
                            transformed: false,
                            face_down: None,
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
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, bear(), true),
            opp_battlefield(2, bear()),
            library_top(10, P0, validated(basic_forest(), &vocab)),
            library_top(11, P1, validated(basic_forest(), &vocab)),
            card(3, tuck_creatures, false),
            card(4, mass_reanimate, false),
            graveyard_of(20, P0, bear()),
            graveyard_of(21, P1, bear()),
            graveyard_of(22, P0, validated(gain_life_instant(1), &vocab)),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Part A: MoveGroupToLibrary { placement: BottomRandom } places each creature on the bottom of
    // its owner's library, behind the pre-existing top card.
    let after_tuck = cast_untargeted(&game, 3);
    let p0_lib = after_tuck.state().execution().window().cards().library(P0);
    let p1_lib = after_tuck.state().execution().window().cards().library(P1);
    assert_eq!(p0_lib.len(), 2);
    assert_eq!(p1_lib.len(), 2);
    assert_eq!(p0_lib[0], object(after_tuck.state(), 10));
    assert_eq!(p0_lib[1], object(after_tuck.state(), 1));
    assert_eq!(p1_lib[0], object(after_tuck.state(), 11));
    assert_eq!(p1_lib[1], object(after_tuck.state(), 2));

    // Part B: MoveGraveyardGroupOwned returns each player's graveyard creature tapped under its
    // owner's control, leaving noncreature cards in the graveyard.
    let after_reanimate = cast_untargeted(after_tuck.state(), 4);
    let reanim_p0 = object(after_reanimate.state(), 20);
    let reanim_p1 = object(after_reanimate.state(), 21);
    let stayed_inst = object(after_reanimate.state(), 22);
    assert_eq!(
        after_reanimate
            .state()
            .execution()
            .window()
            .cards()
            .get(reanim_p0)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        current(after_reanimate.state(), reanim_p0).controller,
        Some(P0)
    );
    assert_eq!(
        after_reanimate
            .state()
            .execution()
            .window()
            .cards()
            .get(reanim_p0)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        after_reanimate
            .state()
            .execution()
            .window()
            .cards()
            .get(reanim_p1)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        current(after_reanimate.state(), reanim_p1).controller,
        Some(P1),
        "under_owner_control: true must return opponent's creature under P1's control"
    );
    assert_eq!(
        after_reanimate
            .state()
            .execution()
            .window()
            .cards()
            .get(reanim_p1)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        after_reanimate
            .state()
            .execution()
            .window()
            .cards()
            .get(stayed_inst)
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
}

// ============================================================================
// 8. ExileLibraryTopLinked + CastPermissionRule::ExileMatchingExt (Nightveil Specter)
// ============================================================================

#[test]
fn exile_library_top_linked_and_exile_matching_ext_nightveil_specter_flow() {
    let vocab = vocabulary();

    let mut specter = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    specter.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_PLAYER,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                        )),
                        filter: Box::new(spellwake_engine::program::ObjectFilter::any()),
                        slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                            ValueExpr::literal(1),
                        )),
                    },
                    spellwake_engine::toolbox::ZoneDest::Exile {
                        face_down: false,
                        viewer: (false).then_some(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                        ),
                        link_to_source: false,
                        delayed_return: None,
                        link_group: Some(LinkedExileGroup::SourceLinked),
                    },
                ),
            ))],
        ),
    });
    specter.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::ExileMatchingExt {
            filter: Box::new(ObjectFilter::any()),
            linked_to_source: true,
            free: false,
            any_color: false,
            lands_allowed: true,
            once_each_turn: false,
            own_turn_only: false,
        },
    });
    let specter = validated(specter, &vocab);

    let opp_creature = validated(
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );

    // Push creature first, then forest on top so library top-to-bottom is [10: forest, 11: creature].
    let game = world(
        vocab.clone(),
        vec![
            card(1, specter, true),
            library_top(11, P1, opp_creature.clone()),
            library_top(10, P1, validated(basic_forest(), &vocab)),
            exile_of(12, P1, opp_creature),
        ],
        vec![CardId(1)],
    );
    let specter_id = object(&game, 1);

    // Exile top 2 cards of P1's library linked to Specter.
    let act1 = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: specter_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_ex1 = pass_pair(act1.state());
    let act2 = submit(
        after_ex1.state(),
        P0,
        GameCommand::ActivateAbility {
            source: specter_id,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_ex2 = pass_pair(act2.state());
    assert_eq!(
        after_ex2
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_ex2.state(), 10))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        after_ex2
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_ex2.state(), 11))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );

    // P0 plays P1's linked exiled land (`lands_allowed: true`).
    let after_land = submit(
        after_ex2.state(),
        P0,
        GameCommand::PlayLand {
            land: object(after_ex2.state(), 10),
        },
    );
    let land_id = object(after_land.state(), 10);
    assert_eq!(
        after_land
            .state()
            .execution()
            .window()
            .cards()
            .get(land_id)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(current(after_land.state(), land_id).controller, Some(P0));

    // P0 casts P1's linked exiled creature.
    let after_creature = cast_untargeted(after_land.state(), 11);
    let creature_id = object(after_creature.state(), 11);
    assert_eq!(
        after_creature
            .state()
            .execution()
            .window()
            .cards()
            .get(creature_id)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        current(after_creature.state(), creature_id).controller,
        Some(P0)
    );

    // Unlinked exiled card 12 must NOT be castable because `linked_to_source: true`.
    assert!(
        attempt(
            after_creature.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(after_creature.state(), 12),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "unlinked exiled card must be rejected by ExileMatchingExt with linked_to_source: true"
    );
}

// ============================================================================
// 9. ZoneOpSpec look-top choose (LibraryTop & rest_dest: Hand) and reveal-until-match
// ============================================================================

#[test]
fn look_top_choose_ext_library_top_and_rest_to_hand_and_reveal_until_match_ext() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let dimir_charm_look = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                    ResolvingPlayer::Controller,
                    ResolvingPlayer::Controller,
                    ValueExpr::literal(2),
                    None,
                    1,
                    false,
                    LookTopKeepDest::LibraryTop,
                    LookTopRestDest::Graveyard,
                    false,
                    false,
                    false,
                    None,
                ),
            ),
        ),
        &vocab,
    );
    let filter_to_gy_rest_hand = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                    ResolvingPlayer::Controller,
                    ResolvingPlayer::Controller,
                    ValueExpr::literal(2),
                    Some(ObjectFilter::creature()),
                    1,
                    true,
                    LookTopKeepDest::Graveyard,
                    LookTopRestDest::Hand,
                    false,
                    false,
                    false,
                    None,
                ),
            ),
        ),
        &vocab,
    );
    let reveal_until_creature = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                spellwake_engine::toolbox::ZoneOpSpec::reveal_until_match_instructions(
                    ResolvingPlayer::Controller,
                    ResolvingPlayer::Controller,
                    ObjectFilter::creature(),
                    ValueExpr::literal(1),
                    LookTopKeepDest::Battlefield {
                        tapped: true,
                        attacking: false,
                    },
                    LookTopRestDest::BottomRandom,
                    false,
                    false,
                    false,
                ),
            ),
        ),
        &vocab,
    );

    // Part A: keep_dest = LibraryTop, rest_dest = Graveyard (library top-to-bottom: [10, 11]).
    let game_a = world(
        vocab.clone(),
        vec![
            card(1, dimir_charm_look, false),
            library_top(11, P0, bear()),
            library_top(10, P0, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );
    let card_11 = object(&game_a, 11);
    let pending_a = cast_untargeted(&game_a, 1);
    assert!(matches!(
        pending_a.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::PromptChoice(frame))
            if matches!(
                frame.domain,
                spellwake_engine::toolbox::ResolvedPromptDomain::Objects { .. }
            )
    ));
    let after_a = answer_look_pick(pending_a.state(), P0, card_11);
    let lib_a = after_a.state().execution().window().cards().library(P0);
    assert_eq!(lib_a, vec![object(after_a.state(), 11)]);
    assert_eq!(
        after_a
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_a.state(), 10))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );

    // Part B: rest_dest = Hand (library top-to-bottom: [20: creature, 21: forest]).
    let game_b = world(
        vocab.clone(),
        vec![
            card(1, filter_to_gy_rest_hand, false),
            library_top(21, P0, validated(basic_forest(), &vocab)),
            library_top(20, P0, bear()),
        ],
        vec![],
    );
    let mut after_b = cast_untargeted(&game_b, 1);
    if after_b.state().pending_decision().is_some() {
        let pick_20 = object(after_b.state(), 20);
        after_b = answer_look_pick(after_b.state(), P0, pick_20);
    }
    assert_eq!(
        after_b
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_b.state(), 20))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        after_b
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_b.state(), 21))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0),
        "unkept card must move to hand when rest_dest is LookTopRestDest::Hand"
    );

    // Part C: RevealUntilMatchExt (library top-to-bottom: [30: forest, 31: creature]).
    let game_c = world(
        vocab.clone(),
        vec![
            card(1, reveal_until_creature, false),
            library_top(31, P0, bear()),
            library_top(30, P0, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );
    let after_c = cast_untargeted(&game_c, 1);
    let matched_31 = object(after_c.state(), 31);
    assert_eq!(
        after_c
            .state()
            .execution()
            .window()
            .cards()
            .get(matched_31)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        after_c
            .state()
            .execution()
            .window()
            .cards()
            .get(matched_31)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        after_c.state().execution().window().cards().library(P0),
        vec![object(after_c.state(), 30)]
    );
}

// ============================================================================
// 10. CreateTokenCopyExt
// ============================================================================

#[test]
fn create_token_copy_ext_applies_nonlegendary_pt_keywords_and_colors() {
    let vocab = vocabulary();
    let elf = vocab.resolve_label("Elf").unwrap();

    let mut legend = named(
        Program::creature(None, 5.into(), 5.into(), BTreeSet::from([elf])),
        "Elder Champion",
    );
    legend.supertypes.insert(Supertype::Legendary);
    legend.color_indicator = Some(BTreeSet::from([Color::Green]));
    let legend = validated(legend, &vocab);

    let mirror_copy = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::create_token_copy(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::TargetObject,
                        ),
                        Box::new(spellwake_engine::toolbox::TokenCopySpec {
                            count: ValueExpr::literal(1),
                            recipients: TokenRecipients::Controller,
                            enters_tapped: false,
                            enters_attacking: false,
                            haste: true,
                            sacrifice_at_end_step: false,
                            exile_at_end_step: false,
                            nonlegendary: true,
                            extra_types: BTreeSet::from([CardType::Artifact]),
                            extra_subtypes: BTreeSet::new(),
                            extra_keywords: BTreeSet::from([KeywordAbility::Flying]),
                            override_pt: Some((1.into(), 1.into())),
                            extra_colors: BTreeSet::from([Color::Blue]),
                            defender_scope: AttackingDefenderScope::AnyLegal,
                            override_colors: None,
                            strip_mana_cost: false,
                            extra_activated: Vec::new(),
                            extra_triggered: Vec::new(),
                            extra_static: Vec::new(),
                            extra_mana: Vec::new(),
                            extra_counters: Vec::new(),
                            keep_name: false,
                        }),
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![card(1, legend, true), card(2, mirror_copy, false)],
        vec![CardId(1)],
    );
    let legend_id = object(&game, 1);

    let after = cast_targeted(&game, P0, 2, TargetRef::Object(legend_id));
    let tokens: Vec<ObjectId> = after
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Battlefield && c.card_id().is_none())
        .map(|c| c.id())
        .collect();
    assert_eq!(tokens.len(), 1, "exactly one token copy must enter");
    assert_eq!(
        after
            .state()
            .execution()
            .window()
            .cards()
            .get(legend_id)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield,
        "original legendary creature must survive because token copy is nonlegendary"
    );

    let token_view = current(after.state(), tokens[0]);
    assert!(
        !token_view
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    assert_eq!(
        token_view.power_toughness.as_ref().unwrap().derived,
        pt(1, 1),
        "override_pt must set token copy to 1/1"
    );
    assert!(token_view.has_keyword(KeywordAbility::Flying));
    assert!(token_view.has_keyword(KeywordAbility::Haste));
    assert_eq!(
        token_view.current_colors,
        BTreeSet::from([Color::Green, Color::Blue])
    );
    assert!(token_view.current_types.contains(&CardType::Artifact));
    assert!(token_view.current_types.contains(&CardType::Creature));
    assert!(token_view.current_creature_subtypes.contains(&elf));
}

// ============================================================================
// 11. activation_cost_reductions, ActivationRestrictions & ExtraActivationCosts
// ============================================================================

// ============================================================================
// 12. CrewAndSaddlePowerBonus, ModifyMaxHandSize, SkipStep, ConvertUnspentMana
// ============================================================================

#[test]
fn crew_power_bonus_modify_max_hand_size_skip_step_and_convert_unspent_mana() {
    let vocab = vocabulary();

    let mut ship = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility::tap_for_power(
            ActivatedAbilityId(1),
            PowerAbility::Crew,
            4,
        )],
    );
    if let ProgramKind::Permanent(def) = &mut ship.kind {
        def.printed_pt = Some(
            PowerToughness {
                power: 5.into(),
                toughness: 5.into(),
            }
            .into(),
        );
    }
    let ship = validated(ship, &vocab);
    let pilot = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );

    let mut rules_engine = Program::enchantment(None);
    rules_engine.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(1),
        rule: StaticModifierRule::CrewAndSaddlePowerBonus {
            selector: Box::new(PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            }),
            crews: true,
            saddles: false,
            bonus: 2,
        },
    });
    rules_engine
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Modifier {
            id: StaticAbilityId(2),
            rule: StaticModifierRule::ConvertUnspentMana(ManaType::Colourless),
        });
    rules_engine
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Modifier {
            id: StaticAbilityId(3),
            rule: StaticModifierRule::ModifyMaxHandSize {
                player: PlayerRelation::Controller,
                delta: -5,
            },
        });
    rules_engine
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Restriction {
            id: StaticAbilityId(4),
            rule: StaticRestriction::SkipStep {
                player: PlayerRelation::Opponent,
                step: Step::Draw,
            },
        });
    let rules_engine = validated(rules_engine, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, ship, true),
            card(2, pilot, true),
            card(3, rules_engine, true),
            card(4, validated(colored_rock(ManaType::Green, 2), &vocab), true),
            // Four cards in P0's hand (max hand size is 7 - 5 = 2 -> must discard 2 at cleanup).
            card(10, validated(basic_forest(), &vocab), false),
            card(11, validated(basic_forest(), &vocab), false),
            card(12, validated(basic_forest(), &vocab), false),
            card(13, validated(basic_forest(), &vocab), false),
            library_top(20, P1, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    let ship_id = object(&game, 1);
    let pilot_id = object(&game, 2);

    // 1. CrewAndSaddlePowerBonus (+2): 2/2 pilot crews Crew 4 Vehicle alone (2 + 2 >= 4).
    let crew_act = submit(
        &game,
        P0,
        GameCommand::ActivateWithCreatures {
            source: ship_id,
            ability: ActivatedAbilityId(1),
            creatures: vec![pilot_id],
        },
    );
    let crewed = pass_pair(crew_act.state());
    assert!(current(crewed.state(), ship_id).is_creature());

    // 2. ConvertUnspentMana(Colourless): 2 Green mana converts to 2 Colourless across step change.
    let floated = submit(
        crewed.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(crewed.state(), 4),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        floated.state().execution().mana(P0, ManaType::Green),
        2.into()
    );
    let beg_combat = pass_pair(floated.state()).into_state();
    assert_eq!(
        beg_combat.turn().unwrap().position().step,
        Step::BeginningCombat
    );
    assert_eq!(beg_combat.execution().mana(P0, ManaType::Green), 0.into());
    assert_eq!(
        beg_combat.execution().mana(P0, ManaType::Colourless),
        2.into(),
        "ConvertUnspentMana(Colourless) must convert unspent Green mana to Colourless on step end"
    );

    // Advance to Turn 1 End step, then enter Cleanup to test ModifyMaxHandSize (-5 -> limit 2).
    let mut dec_att = pass_pair(&beg_combat).into_state();
    if dec_att.pending_decision().is_some() {
        dec_att = reply(&dec_att, P0, DecisionReply::DeclareAttackers(vec![])).into_state();
    }
    let end_combat = pass_pair(&dec_att).into_state();
    let postcombat = pass_pair(&end_combat).into_state();
    let end_t1 = pass_pair(&postcombat).into_state();
    let cleanup_step = pass_pair(&end_t1);

    let cleanup_decision = cleanup_step
        .state()
        .pending_decision()
        .expect("ModifyMaxHandSize (-5) with 4 cards in hand must prompt cleanup discard");
    let RuleFrame::Checks(
        CheckDecision::CleanupDiscard(choice) | CheckDecision::CleanupSelect(choice),
    ) = cleanup_decision.frame()
    else {
        panic!("expected CleanupDiscard or CleanupSelect frame");
    };
    assert_eq!(choice.chooser(), P0);
    assert_eq!(
        choice.required(),
        2,
        "4 cards minus reduced max hand size 2 = discard 2"
    );
    let to_discard = choice.domain()[..2].to_vec();
    let t2_upkeep =
        reply(cleanup_step.state(), P0, DecisionReply::Discard(to_discard)).into_state();
    assert_eq!(t2_upkeep.turn().unwrap().position().number, 2);
    assert_eq!(t2_upkeep.turn().unwrap().position().step, Step::Upkeep);

    // 4. SkipStep(Opponent, Step::Draw): passing Turn 2 Upkeep skips P1's Draw step directly to
    // PrecombatMain, leaving P1's library card undrawn.
    let after_t2_upkeep = pass_in_order(&t2_upkeep, [P1, P0]).into_state();
    assert_eq!(
        after_t2_upkeep.turn().unwrap().position().step,
        Step::PrecombatMain,
        "SkipStep(Opponent, Draw) must jump directly from Upkeep to PrecombatMain"
    );
    assert_eq!(
        after_t2_upkeep
            .execution()
            .window()
            .cards()
            .library(P1)
            .len(),
        1,
        "skipped Draw step must not draw a card for P1"
    );
}

// ============================================================================
// 13. CastPermissionRule::GrantSpellKeyword & SpellCostModifier::FirstSpellRule
// ============================================================================

// ============================================================================
// 14. KickerExtra, ConditionalCastCost::Alternative, GraveyardOrExile, CastTiming
// ============================================================================

// ============================================================================
// 15. StaticDamageOp::{PreventUpToExpr, PreventAndFollowup}, ConditionalModifyDamage,
//     EventValueKind::DamagePreventedThisWay
// ============================================================================

// ============================================================================
// 16. ZoneReplacementSubject::{SourceSelfDiscardByOpponent, MatchingNotCast} & exile_counters
// ============================================================================

#[test]
fn zone_replacement_source_self_discard_by_opponent_matching_not_cast_and_exile_counters() {
    let vocab = vocabulary();

    // Part A: Loxodon Smiter (SourceSelfDiscardByOpponent).
    let mut smiter = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
    smiter.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(1),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::SourceSelfDiscardByOpponent,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Battlefield,
        choice: ZoneReplacementChoice::Mandatory,
    });
    let smiter = validated(smiter, &vocab);

    let self_discard = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::Controller),
                        ValueExpr::literal(1),
                    ),
                ))],
            ),
        ),
        &vocab,
    );
    let opp_discard = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_OPPONENT,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        ValueExpr::literal(1),
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, smiter.clone(), false),
            card(2, smiter, false),
            card(3, self_discard, false),
            opp_hand(4, opp_discard),
        ],
        vec![],
    );
    let smiter1 = object(&game_a, 1);
    let smiter2 = object(&game_a, 2);

    // Controller discarding smiter1 puts it in graveyard (NOT opponent's effect).
    let pend_self = cast_untargeted(&game_a, 3);
    let after_self = reply(
        pend_self.state(),
        P0,
        DecisionReply::SelectResolution(vec![smiter1]),
    );
    assert_eq!(
        after_self
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_self.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0),
        "controller's own discard effect must send Smiter to graveyard"
    );

    // Opponent causing P0 to discard smiter2 puts smiter2 onto the battlefield!
    let p1_prio = submit(after_self.state(), P0, GameCommand::Pass);
    let after_opp = cast_targeted(p1_prio.state(), P1, 4, TargetRef::Player(P0));
    let after_opp = if after_opp.state().pending_decision().is_some() {
        reply(
            after_opp.state(),
            P0,
            DecisionReply::SelectResolution(vec![smiter2]),
        )
    } else {
        after_opp
    };
    assert_eq!(
        after_opp
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_opp.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield,
        "opponent-caused discard of Smiter must replace graveyard with battlefield"
    );

    // Part B: Containment Priest (MatchingNotCast) + zone_replacement_extras (exile_counters).
    let mut priest = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    priest.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(1),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([
            ZoneActionKind::EffectEnterBattlefield,
        ])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::MatchingNotCast {
            filter: Box::new(ObjectFilter::creature()),
            owner_relation: None,
        },
        destination: PublicZoneDestination::Battlefield,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::Mandatory,
    });
    priest.zone_replacement_extras.insert(
        ZoneReplacementAbilityId(1),
        ZoneReplacementExtra {
            exile_counters: Some((CounterKind::Void, 1)),
            library_placement: None,
            followup_body: None,
        },
    );
    let priest = validated(priest, &vocab);

    let castable_bear = validated(
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );
    let reanimate_one = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_OWN_GRAVEYARD_CARD,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Battlefield {
                            controller: if false {
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
                            face_down: None,
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
                    ),
                ))],
            ),
        ),
        &vocab,
    );

    let game_b = world(
        vocab,
        vec![
            card(1, priest, true),
            card(2, castable_bear.clone(), false),
            graveyard_of(3, P0, castable_bear),
            card(4, reanimate_one, false),
        ],
        vec![CardId(1)],
    );

    // Cast creature enters battlefield normally.
    let after_cast = cast_untargeted(&game_b, 2);
    assert_eq!(
        after_cast
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_cast.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );

    // Non-cast reanimated creature is exiled instead with 1 Void counter!
    let gy_bear = object(after_cast.state(), 3);
    let after_reanim = cast_targeted(after_cast.state(), P0, 4, TargetRef::Object(gy_bear));
    let exiled_bear = object(after_reanim.state(), 3);
    assert_eq!(
        after_reanim
            .state()
            .execution()
            .window()
            .cards()
            .get(exiled_bear)
            .unwrap()
            .zone(),
        ObjectZone::Exile,
        "MatchingNotCast must exile non-cast creature entering the battlefield"
    );
    assert_eq!(
        after_reanim
            .state()
            .execution()
            .counters_of_kind(exiled_bear, CounterKind::Void),
        1.into(),
        "zone_replacement_extras.exile_counters must place 1 Void counter on the exiled card"
    );
}

// ============================================================================
// 17. ModifyTokensFiltered & StealCreatedTokens (including TemporaryStatic)
// ============================================================================

// ============================================================================
// 18. EntryCopyRule::MatchingExt
// ============================================================================

#[test]
fn entry_copy_matching_ext_keeps_name_grants_keywords_and_conditional_counters() {
    let vocab = vocabulary();

    let mut donor = named(
        Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
        "Dragon Boss",
    );
    donor.supertypes.insert(Supertype::Legendary);
    donor.color_indicator = Some(BTreeSet::from([Color::Red]));
    let donor = validated(donor, &vocab);

    let mut clone = named(
        Program::creature(Some(ManaCost::free()), 0.into(), 0.into(), BTreeSet::new()),
        "Sakashima Variant",
    );
    clone.entry_copy = Some(EntryCopyRule::MatchingExt(Box::new(EntryCopySpecExt {
        zone: EntryCopyZone::Battlefield,
        filter: ObjectFilter::creature(),
        nonlegendary: true,
        extra_types: BTreeSet::from([CardType::Artifact]),
        extra_creature_subtypes: BTreeSet::new(),
        extra_keywords: BTreeSet::from([KeywordAbility::Flying]),
        extra_counters: vec![],
        conditional_counters: vec![(
            ResolutionCondition {
                left: ValueExpr::controlled_type(CardType::Artifact),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            CounterKind::PlusOne,
            2,
        )],
        override_colors: None,
        extra_artifact_subtypes: BTreeSet::new(),
        extra_enchantment_subtypes: BTreeSet::new(),
        extra_supertypes: BTreeSet::new(),
        extra_activated: vec![],
        extra_triggered: vec![],
        extra_static: vec![],
        override_pt: None,
        extra_colors: BTreeSet::new(),
        keep_name: true,
        optional: true,
        enters_tapped: false,
        extra_mana: vec![],
    })));
    let clone = validated(clone, &vocab);

    let relic = validated(Program::artifact(None, vec![]), &vocab);
    let game = world(
        vocab,
        vec![
            card(1, donor, true),
            card(2, relic, true),
            card(3, clone, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let donor_id = object(&game, 1);

    let pending_copy = cast_untargeted(&game, 3);
    let decision = pending_copy
        .state()
        .pending_decision()
        .expect("EntryCopyRule::MatchingExt must prompt ZoneReplacement::EntryCopy");
    let RuleFrame::ZoneReplacement(frame) = decision.frame() else {
        panic!("expected ZoneReplacement frame");
    };
    let ZoneReplacementRequest::EntryCopy { donors, .. } = frame.request() else {
        panic!("expected EntryCopy request");
    };
    assert!(donors.contains(&donor_id));

    let entered = reply(
        pending_copy.state(),
        P0,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(Some(donor_id))),
    );
    let clone_id = object(entered.state(), 3);
    let clone_view = current(entered.state(), clone_id);
    assert_eq!(
        clone_view.current_names,
        ObjectNames::single(EnglishCardName::new("Sakashima Variant".into()).unwrap()),
        "keep_name: true must preserve the clone's own name"
    );
    assert!(
        !clone_view
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    assert!(clone_view.has_keyword(KeywordAbility::Flying));
    assert!(clone_view.current_types.contains(&CardType::Artifact));
    assert_eq!(clone_view.current_colors, BTreeSet::from([Color::Red]));
    assert_eq!(
        entered
            .state()
            .execution()
            .counters_of_kind(clone_id, CounterKind::PlusOne),
        2.into(),
        "conditional_counters must place 2 +1/+1 counters when condition holds"
    );
    assert_eq!(
        clone_view.power_toughness.unwrap().derived,
        pt(6, 6),
        "4/4 donor with 2 +1/+1 counters must evaluate to 6/6"
    );
}

// ============================================================================
// 19. Turn & Provenance Trackers
// ============================================================================

// ============================================================================
// 20. Wave 7 Triggers: DealsAtLeastDamage, EntersWithoutBeingPlayed, BlockedByAtLeast
// ============================================================================

// ============================================================================
// 21. Validation & Runtime Fixes: uses_target_permanent, CFG FlowFacts Promotion,
//     OptionalEffect wrapping ExileResolvingSpell
// ============================================================================
