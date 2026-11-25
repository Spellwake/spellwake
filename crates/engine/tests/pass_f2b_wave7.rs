#![forbid(unsafe_code)]
//! Runtime integration tests for Pass F2-B Wave 7 primitives:
//! - `ForEachPlayer` + `ResolvingPlayer::LoopPlayer` (APNAP order)
//! - `OfferAnyPlayer` (accepting player runs `cost_body` + `paid_body`, or `declined_body` if all decline)
//! - `DealDamageToAnnouncedTargets` (multi-target equal damage) and `PermanentTargetAction::Damage`
//! - `UntapOrTapSelect` (non-targeted selection, exact count and `up_to: true`)
//! - `GrantChosenProtection` (`UntilControllerNextTurn`) and `ResolvedPartSpec::{ChosenColor,
//!   ChosenBasicLandType, ProtectionFromEachOpponent, CannotBeTargetedByColors}`
//! - `RemoveCountersFromSubject` (specific kind, any kind, remove all) and
//!   `PermanentTargetAction::RemoveCounters`
//! - `MoveGroupToLibrary` (`LibraryPlacement::BottomRandom`) and `MoveGraveyardGroupOwned`
//!   (`under_owner_control`)
//! - `ExileLibraryTopLinked` + `CastPermissionRule::ExileMatchingExt` (Nightveil Specter flow)
//! - `LookTopChooseExt` (`keep_dest: LibraryTop` and `rest_dest: Hand`) and `RevealUntilMatchExt`
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
        ObjectColorFilter, ObjectFilter, ObjectStatKind, PermanentTargetAction, PlayerRelation,
        PlayerStatExpr, PowerFilter, Program, ProgramKind, ProtectionRecipients,
        PublicZoneDestination, ResolutionChoiceDomain, ResolutionCondition, ResolvingPlayer,
        SourceStateExpr, SourceZoneFilter, SpellFilter, SubjectDestination, Supertype,
        TemporaryRuleKind, TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId, ValueExpr, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementExtra, ZoneReplacementSubject,
    },
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
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
// 1. ForEachPlayer + ResolvingPlayer::LoopPlayer
// ============================================================================

#[test]
fn for_each_player_runs_body_for_each_player_in_apnap_order() {
    let vocab = vocabulary();

    let each_draw_and_lose = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ForEachPlayer {
                    players: AffectedPlayers::EachPlayer,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::DrawPlayer {
                                players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                                amount: ValueExpr::literal(1),
                            },
                            Instruction::AdjustPlayersLife {
                                players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                                gain: false,
                                amount: ValueExpr::literal(1),
                            },
                        ],
                    ),
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, each_draw_and_lose, false),
            library_top(10, P0, validated(basic_forest(), &vocab)),
            library_top(11, P1, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );

    let resolved = cast_untargeted(&game, 1);
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&19.into())
    );
    assert_eq!(
        resolved.state().execution().life_total(P1),
        Some(&19.into())
    );
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(object(resolved.state(), 10))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(object(resolved.state(), 11))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P1)
    );
    let draw_seats: Vec<SeatId> = resolved
        .draw_journal()
        .events
        .iter()
        .map(|ev| match ev {
            spellwake_engine::draw::DrawEvent::Drawn { player, .. }
            | spellwake_engine::draw::DrawEvent::EmptyLibrary { player, .. } => *player,
        })
        .collect();
    assert_eq!(
        draw_seats,
        vec![P0, P1],
        "ForEachPlayer must execute in APNAP order (active player P0 first, then P1)"
    );
}

// ============================================================================
// 2. OfferAnyPlayer
// ============================================================================

#[test]
fn offer_any_player_accepts_first_payer_or_runs_declined_body() {
    let vocab = vocabulary();

    let offer_spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::OfferAnyPlayer {
                    players: AffectedPlayers::EachPlayer,
                    cost_body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::PayLife {
                            player: ResolvingPlayer::LoopPlayer,
                            amount: ValueExpr::literal(3),
                        }],
                    ),
                    paid_body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawPlayer {
                            players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                            amount: ValueExpr::literal(2),
                        }],
                    ),
                    declined_body: Some(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::GainControllerLife {
                            amount: ValueExpr::literal(5),
                        }],
                    )),
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, offer_spell.clone(), false),
            card(2, offer_spell, false),
            library_top(10, P1, validated(basic_forest(), &vocab)),
            library_top(11, P1, validated(basic_forest(), &vocab)),
            library_top(12, P1, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );

    // Case A: P0 declines, P1 accepts -> P1 pays 3 life and draws 2; declined_body does not run.
    let pending_p0 = cast_untargeted(&game, 1);
    let pending_p1 = answer_yes_no_offer(pending_p0.state(), P0, false);
    let after_p1_accept = answer_yes_no_offer(pending_p1.state(), P1, true);
    assert!(after_p1_accept.state().pending_decision().is_none());
    assert_eq!(
        after_p1_accept.state().execution().life_total(P0),
        Some(&20.into()),
        "declining P0 must not pay cost or run declined_body when P1 accepted"
    );
    assert_eq!(
        after_p1_accept.state().execution().life_total(P1),
        Some(&17.into()),
        "accepting P1 must pay 3 life from cost_body"
    );
    let p1_hand_after_a = after_p1_accept
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(P1))
        .count();
    assert_eq!(
        p1_hand_after_a, 2,
        "accepting P1 must draw 2 cards from paid_body"
    );

    // Case B: Both P0 and P1 decline -> declined_body runs (controller P0 gains 5 life).
    let pending_b_p0 = cast_untargeted(after_p1_accept.state(), 2);
    let pending_b_p1 = answer_yes_no_offer(pending_b_p0.state(), P0, false);
    let after_all_decline = answer_yes_no_offer(pending_b_p1.state(), P1, false);
    assert!(after_all_decline.state().pending_decision().is_none());
    assert_eq!(
        after_all_decline.state().execution().life_total(P0),
        Some(&25.into()),
        "declined_body must run when all players decline"
    );
    assert_eq!(
        after_all_decline.state().execution().life_total(P1),
        Some(&17.into())
    );
}

// ============================================================================
// 3. DealDamageToAnnouncedTargets & PermanentTargetAction::Damage
// ============================================================================

#[test]
fn deal_damage_to_announced_targets_and_permanent_target_action_damage() {
    let vocab = vocabulary();

    let twin_bolt = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleAnyTarget,
                vec![Instruction::DealDamageToAnnouncedTargets {
                    amount: ValueExpr::literal(2),
                    min_targets: 1,
                    max_targets: 2,
                }],
            ),
        ),
        &vocab,
    );
    let dual_searing = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::AffectPermanentTargets {
                    minimum: 1,
                    maximum: 2,
                    action: PermanentTargetAction::Damage(ValueExpr::literal(3)),
                }],
            ),
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

    // Part B: PermanentTargetAction::Damage deals 3 to both opp_c1 (lethal: 2+3=5) and opp_c2 (3).
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

#[test]
fn untap_or_tap_select_supports_exact_count_and_up_to() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let tap_exact_two = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::UntapOrTapSelect {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(ObjectFilter::creature().controlled()),
                    count: ValueExpr::literal(2),
                    up_to: false,
                    tap: true,
                }],
            ),
        ),
        &vocab,
    );
    let untap_up_to_two = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::UntapOrTapSelect {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(ObjectFilter::creature().controlled()),
                    count: ValueExpr::literal(2),
                    up_to: true,
                    tap: false,
                }],
            ),
        ),
        &vocab,
    );

    let b1 = bear();
    let b2 = bear();
    let b3 = bear();
    let game = world(
        vocab,
        vec![
            card(1, b1, true),
            card(2, b2, true),
            card(3, b3, true),
            card(4, tap_exact_two, false),
            card(5, untap_up_to_two, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let c1 = object(&game, 1);
    let c2 = object(&game, 2);
    let c3 = object(&game, 3);

    // Exact count = 2: selecting only 1 creature is rejected, selecting 2 succeeds.
    let pending_tap = cast_untargeted(&game, 4);
    let tap_decision = pending_tap
        .state()
        .pending_decision()
        .expect("UntapOrTapSelect must prompt ResolutionSelect");
    assert!(matches!(
        tap_decision.frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    assert_eq!(
        attempt(
            pending_tap.state(),
            P0,
            GameCommand::Reply {
                decision: tap_decision.id(),
                choice: DecisionReply::SelectResolution(vec![c1]),
            },
        ),
        Err(GameError::Effect(EffectError::InvalidSelectionCount)),
        "exact count UntapOrTapSelect must reject under-selection"
    );
    let after_tap = reply(
        pending_tap.state(),
        P0,
        DecisionReply::SelectResolution(vec![c1, c2]),
    );
    assert_eq!(
        after_tap
            .state()
            .execution()
            .window()
            .cards()
            .get(c1)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        after_tap
            .state()
            .execution()
            .window()
            .cards()
            .get(c2)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        after_tap
            .state()
            .execution()
            .window()
            .cards()
            .get(c3)
            .unwrap()
            .tapped(),
        Some(false)
    );

    // up_to = true: selecting 1 of 2 tapped creatures is accepted.
    let pending_untap = cast_untargeted(after_tap.state(), 5);
    let untap_decision = pending_untap
        .state()
        .pending_decision()
        .expect("up_to UntapOrTapSelect must prompt ResolutionSelect");
    assert!(matches!(
        untap_decision.frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    let after_untap = reply(
        pending_untap.state(),
        P0,
        DecisionReply::SelectResolution(vec![c1]),
    );
    assert_eq!(
        after_untap
            .state()
            .execution()
            .window()
            .cards()
            .get(c1)
            .unwrap()
            .tapped(),
        Some(false),
        "selected tapped creature must untap"
    );
    assert_eq!(
        after_untap
            .state()
            .execution()
            .window()
            .cards()
            .get(c2)
            .unwrap()
            .tapped(),
        Some(true),
        "unselected tapped creature must remain tapped when up_to picks fewer than count"
    );
}

// ============================================================================
// 5. GrantChosenProtection & Wave 7 ResolvedPartSpecs
// ============================================================================

#[test]
fn grant_chosen_protection_duration_and_wave7_resolved_part_specs() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let grant_prot = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::GrantChosenProtection {
                    recipients: ProtectionRecipients::TargetPermanent,
                    duration: ResolvedDuration::UntilControllerNextTurn,
                }],
            ),
        ),
        &vocab,
    );
    let mut red_ping = damage_any_instant(1);
    red_ping.color_indicator = Some(BTreeSet::from([Color::Red]));
    let red_ping = validated(red_ping, &vocab);

    let mut blue_ping = damage_any_instant(1);
    blue_ping.color_indicator = Some(BTreeSet::from([Color::Blue]));
    let blue_ping = validated(blue_ping, &vocab);

    let colorless_ping = validated(damage_any_instant(1), &vocab);

    let paint_and_ward = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::ChooseValue {
                        chooser: ResolvingPlayer::Controller,
                        domain: ResolutionChoiceDomain::Color,
                    },
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![
                            ResolvedPartSpec::ChosenColor { add: false },
                            ResolvedPartSpec::ProtectionFromEachOpponent,
                            ResolvedPartSpec::CannotBeTargetedByColors(ColorMask::BLUE),
                        ]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                ],
            ),
        ),
        &vocab,
    );
    let add_land_type = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::ChooseValue {
                        chooser: ResolvingPlayer::Controller,
                        domain: ResolutionChoiceDomain::BasicLandType,
                    },
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ChosenBasicLandType {
                            add: true,
                        }]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                ],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, bear(), true),
            card(2, bear(), true),
            card(3, validated(basic_forest(), &vocab), true),
            card(4, grant_prot, false),
            card(5, red_ping, false),
            card(6, paint_and_ward, false),
            card(7, blue_ping, false),
            card(8, add_land_type, false),
            opp_hand(9, colorless_ping),
            library_top(20, P0, validated(basic_forest(), &vocab)),
            library_top(21, P1, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let bear1 = object(&game, 1);
    let bear2 = object(&game, 2);
    let forest = object(&game, 3);

    // Part A: GrantChosenProtection (Red) lasts UntilControllerNextTurn.
    let pending_prot = cast_targeted(&game, P0, 4, TargetRef::Object(bear1));
    let after_prot = answer_color_choice(pending_prot.state(), P0, Color::Red);
    assert!(current(after_prot.state(), bear1).has_keyword(KeywordAbility::ProtectionFromRed));
    assert_eq!(
        attempt(
            after_prot.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(after_prot.state(), 5),
                target: TargetRef::Object(bear1),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "red spell must not target creature with chosen protection from red"
    );

    // Part B: ChosenColor (Blue) + ProtectionFromEachOpponent + CannotBeTargetedByColors(BLUE).
    let pending_color = cast_targeted(after_prot.state(), P0, 6, TargetRef::Object(bear2));
    let after_paint = answer_color_choice(pending_color.state(), P0, Color::Blue);
    let bear2_view = current(after_paint.state(), bear2);
    assert_eq!(bear2_view.current_colors, BTreeSet::from([Color::Blue]));
    assert!(bear2_view.has_keyword(KeywordAbility::ProtectionFrom(
        ProtectionFromQuality::Player(P1)
    )));
    assert_eq!(
        attempt(
            after_paint.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(after_paint.state(), 7),
                target: TargetRef::Object(bear2),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "CannotBeTargetedByColors(BLUE) must block even controller's own blue spell"
    );
    // Pass to P1 and verify ProtectionFromEachOpponent blocks P1's colorless spell.
    let p1_prio = submit(after_paint.state(), P0, GameCommand::Pass);
    assert_eq!(
        attempt(
            p1_prio.state(),
            P1,
            GameCommand::CastPaid {
                spell: object(p1_prio.state(), 9),
                target: TargetRef::Object(bear2),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget)),
        "ProtectionFromEachOpponent must block opponent's colorless spell"
    );

    // Part C: ChosenBasicLandType { add: true } adds Island to Forest.
    let p0_beg_combat = submit(p1_prio.state(), P1, GameCommand::Pass);
    let pending_land = cast_targeted(p0_beg_combat.state(), P0, 8, TargetRef::Object(forest));
    let after_land = reply(pending_land.state(), P0, DecisionReply::OptionIndex(1));
    assert_eq!(
        current(after_land.state(), forest).current_land_subtypes,
        BTreeSet::from([LandType::Forest, LandType::Island])
    );

    // Verify UntilControllerNextTurn survives Turn 1 cleanup into Turn 2 (P1), then expires on Turn 3 (P0).
    let mut dec_att = pass_pair(after_land.state()).into_state();
    if dec_att.pending_decision().is_some() {
        dec_att = reply(&dec_att, P0, DecisionReply::DeclareAttackers(vec![])).into_state();
    }
    let end_combat = pass_pair(&dec_att).into_state();
    let postcombat = pass_pair(&end_combat).into_state();
    let end_t1 = pass_pair(&postcombat).into_state();
    let t2_upkeep = pass_pair(&end_t1).into_state();
    assert_eq!(t2_upkeep.turn().unwrap().position().number, 2);
    assert!(
        current(&t2_upkeep, bear1).has_keyword(KeywordAbility::ProtectionFromRed),
        "UntilControllerNextTurn protection must persist through opponent's turn"
    );

    let t2_draw = pass_in_order(&t2_upkeep, [P1, P0]).into_state();
    let t2_main = pass_in_order(&t2_draw, [P1, P0]).into_state();
    let t2_end = reach_end(&t2_main, [P1, P0]);
    let t3_upkeep = pass_in_order(&t2_end, [P1, P0]).into_state();
    assert_eq!(t3_upkeep.turn().unwrap().position().number, 3);
    assert!(
        !current(&t3_upkeep, bear1).has_keyword(KeywordAbility::ProtectionFromRed),
        "UntilControllerNextTurn protection must expire when controller's next turn starts"
    );
}

// ============================================================================
// 6. RemoveCountersFromSubject & PermanentTargetAction::RemoveCounters
// ============================================================================

#[test]
fn remove_counters_from_subject_and_permanent_target_action_remove_counters() {
    let vocab = vocabulary();

    let bear = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );
    let relic = || validated(Program::artifact(None, vec![]), &vocab);
    let seed_counters = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::Matching(ObjectFilter::creature())),
                        kind: CounterKind::PlusOne,
                        amount: ValueExpr::literal(3),
                    },
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::Matching(ObjectFilter::any())),
                        kind: CounterKind::Charge,
                        amount: ValueExpr::literal(2),
                    },
                ],
            ),
        ),
        &vocab,
    );
    let remove_two_plus_one = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::RemoveCountersFromSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: Some(CounterKind::PlusOne),
                    amount: Some(ValueExpr::literal(2)),
                }],
            ),
        ),
        &vocab,
    );
    let remove_one_any = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::RemoveCountersFromSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: None,
                    amount: Some(ValueExpr::literal(1)),
                }],
            ),
        ),
        &vocab,
    );
    let remove_all_any = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::RemoveCountersFromSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: None,
                    amount: None,
                }],
            ),
        ),
        &vocab,
    );
    let drain_two_artifacts = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::AffectPermanentTargets {
                    minimum: 1,
                    maximum: 2,
                    action: PermanentTargetAction::RemoveCounters {
                        kind: Some(CounterKind::Charge),
                        amount: None,
                    },
                }],
            ),
        ),
        &vocab,
    );

    let r1 = relic();
    let r2 = relic();
    let game = world(
        vocab,
        vec![
            card(1, bear, true),
            card(2, r1, true),
            card(3, r2, true),
            card(4, seed_counters, false),
            card(5, remove_two_plus_one, false),
            card(6, remove_one_any, false),
            card(7, remove_all_any, false),
            card(8, drain_two_artifacts, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let bear_id = object(&game, 1);
    let relic1 = object(&game, 2);
    let relic2 = object(&game, 3);

    let loaded = cast_untargeted(&game, 4);
    assert_eq!(
        loaded
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::PlusOne),
        3.into()
    );
    assert_eq!(
        loaded
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::Charge),
        2.into()
    );

    // Case A: specific kind + fixed amount (removes 2 +1/+1, leaves 1 +1/+1 and 2 Charge).
    let after_specific = cast_targeted(loaded.state(), P0, 5, TargetRef::Object(bear_id));
    assert_eq!(
        after_specific
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::PlusOne),
        1.into()
    );
    assert_eq!(
        after_specific
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::Charge),
        2.into()
    );

    // Case B: any kind + fixed amount 1 (handles CounterRemoval prompt if issued, or deterministic).
    let mut after_any_one =
        cast_targeted(after_specific.state(), P0, 6, TargetRef::Object(bear_id));
    if let Some(decision) = after_any_one.state().pending_decision() {
        assert!(matches!(decision.frame(), RuleFrame::CounterRemoval(_)));
        after_any_one = reply(
            after_any_one.state(),
            P0,
            DecisionReply::RemoveCounters(BTreeMap::from([(CounterKind::Charge, 1)])),
        );
    }
    let remaining_total = after_any_one
        .state()
        .execution()
        .counters_of_kind(bear_id, CounterKind::PlusOne)
        .plus(
            &after_any_one
                .state()
                .execution()
                .counters_of_kind(bear_id, CounterKind::Charge),
        );
    assert_eq!(
        remaining_total,
        2.into(),
        "removing 1 counter of any kind must reduce total counters from 3 to 2"
    );

    // Case C: any kind + amount: None removes ALL counters from target.
    let after_all = cast_targeted(after_any_one.state(), P0, 7, TargetRef::Object(bear_id));
    assert_eq!(
        after_all
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::PlusOne),
        0.into()
    );
    assert_eq!(
        after_all
            .state()
            .execution()
            .counters_of_kind(bear_id, CounterKind::Charge),
        0.into()
    );

    // Case D: PermanentTargetAction::RemoveCounters removes all Charge counters from 2 artifacts.
    let cast_drain = submit(
        after_all.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_all.state(), 8),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(relic1), TargetRef::Object(relic2)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_drain = pass_pair(cast_drain.state());
    assert_eq!(
        after_drain
            .state()
            .execution()
            .counters_of_kind(relic1, CounterKind::Charge),
        0.into()
    );
    assert_eq!(
        after_drain
            .state()
            .execution()
            .counters_of_kind(relic2, CounterKind::Charge),
        0.into()
    );
}

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
                vec![Instruction::MoveGroupToLibrary {
                    group: ObjectFilter::creature(),
                    placement: LibraryPlacement::BottomRandom,
                }],
            ),
        ),
        &vocab,
    );
    let mass_reanimate = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MoveGraveyardGroupOwned {
                    players: AffectedPlayers::EachPlayer,
                    filter: Box::new(ObjectFilter::creature()),
                    destination: SubjectDestination::Battlefield {
                        tapped: true,
                        attacking: false,
                        transformed: false,
                        under_owner_control: true,
                        with_counter: None,
                        attach_to_self: false,
                    },
                }],
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
            BodyTarget::SinglePlayer,
            vec![Instruction::ExileLibraryTopLinked {
                player: ResolvingPlayer::TargetPlayer,
                amount: ValueExpr::literal(1),
                group: LinkedExileGroup::SourceLinked,
                face_down: false,
            }],
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
// 9. LookTopChooseExt (LibraryTop & rest_dest: Hand) and RevealUntilMatchExt
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
                vec![Instruction::LookTopChooseExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    count: ValueExpr::literal(2),
                    filter: None,
                    max_picks: 1,
                    take_all_matching: false,
                    keep_dest: LookTopKeepDest::LibraryTop,
                    rest_dest: LookTopRestDest::Graveyard,
                    reveal_kept: false,
                }],
            ),
        ),
        &vocab,
    );
    let filter_to_gy_rest_hand = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::LookTopChooseExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    count: ValueExpr::literal(2),
                    filter: Some(Box::new(ObjectFilter::creature())),
                    max_picks: 1,
                    take_all_matching: true,
                    keep_dest: LookTopKeepDest::Graveyard,
                    rest_dest: LookTopRestDest::Hand,
                    reveal_kept: false,
                }],
            ),
        ),
        &vocab,
    );
    let reveal_until_creature = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::RevealUntilMatchExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(ObjectFilter::creature()),
                    count: ValueExpr::literal(1),
                    match_dest: LookTopKeepDest::Battlefield { tapped: true },
                    rest_dest: LookTopRestDest::BottomRandom,
                }],
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
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTokenCopyExt {
                    source: MoveSubjectRef::TargetObject,
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
                }],
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

#[test]
fn activation_cost_reductions_restrictions_and_wave7_extra_costs() {
    let vocab = vocabulary();

    // Artifact 1: {3} ability reduced by controlled artifacts (2 artifacts -> costs {1}).
    let mut reducer = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::generic(3.into()),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(2),
                }],
            ),
        }],
    );
    reducer.activation_cost_reductions.insert(
        ActivatedAbilityId(1),
        ValueExpr::controlled_type(CardType::Artifact),
    );
    let reducer = validated(reducer, &vocab);

    // Artifact 2: {X} ability with XNonZero + OnlySpendManaType(Black).
    let mut rats = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::GenericX {
                symbols: 1,
                tap_source: false,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::chosen_x(),
                }],
            ),
        }],
    );
    rats.activation_restrictions.insert(
        ActivatedAbilityId(1),
        ActivationRestriction::All(vec![
            ActivationRestriction::XNonZero,
            ActivationRestriction::OnlySpendManaType(ManaType::Black),
        ]),
    );
    let rats = validated(rats, &vocab);

    // Artifact 3: PaySnowMana(1) + ExileTopLibrary(1).
    let mut frost_engine = Program::artifact(
        None,
        vec![
            ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(3),
                    }],
                ),
            },
            ActivatedAbility {
                id: ActivatedAbilityId(2),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            },
            ActivatedAbility {
                id: ActivatedAbilityId(3),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::chosen_x(),
                    }],
                ),
            },
        ],
    );
    frost_engine.extra_activation_costs.insert(
        ActivatedAbilityId(1),
        vec![ExtraActivationCost::PaySnowMana(NonZeroU8::new(1).unwrap())],
    );
    frost_engine.extra_activation_costs.insert(
        ActivatedAbilityId(2),
        vec![ExtraActivationCost::ExileTopLibrary {
            count: Some(NonZeroU8::new(1).unwrap()),
            face_down: false,
        }],
    );
    frost_engine.extra_activation_costs.insert(
        ActivatedAbilityId(3),
        vec![ExtraActivationCost::SacrificeAnyNumber(
            ObjectFilter::creature(),
        )],
    );
    let frost_engine = validated(frost_engine, &vocab);

    // Hand card with RevealSource extra activation cost.
    let mut forecast_card = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(4),
                }],
            ),
        }],
    );
    forecast_card.extra_activation_costs.insert(
        ActivatedAbilityId(1),
        vec![ExtraActivationCost::RevealSource],
    );
    let forecast_card = validated(forecast_card, &vocab);

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };

    let game = world(
        vocab.clone(),
        vec![
            card(1, reducer, true),
            card(2, rats, true),
            card(3, frost_engine, true),
            card(4, validated(colorless_rock(1), &vocab), true),
            card(5, validated(colored_rock(ManaType::Black, 1), &vocab), true),
            card(
                6,
                validated(snow_rock(ManaType::Colourless, 1), &vocab),
                true,
            ),
            card(7, forecast_card, false),
            card(8, bear(), true),
            card(9, bear(), true),
            library_top(20, P0, validated(basic_forest(), &vocab)),
        ],
        vec![
            CardId(1),
            CardId(2),
            CardId(3),
            CardId(4),
            CardId(5),
            CardId(6),
            CardId(8),
            CardId(9),
        ],
    );

    // 1. activation_cost_reductions: 6 controlled artifacts reduce {3} to {0}.
    let act_red = submit(
        &game,
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_red = pass_pair(act_red.state());
    assert_eq!(
        after_red.state().execution().life_total(P0),
        Some(&22.into())
    );

    // 2. XNonZero rejects X = 0; OnlySpendManaType(Black) rejects Colourless and accepts Black.
    assert!(
        attempt(
            after_red.state(),
            P0,
            GameCommand::ActivateWithRestrictedMana {
                source: object(after_red.state(), 2),
                ability: ActivatedAbilityId(1),
                granting_effect: None,
                origin: ActivationOrigin::Untargeted,
                chosen_x: Some(0.into()),
                payment: Some(ManaPayment::default()),
                restricted_mana: vec![],
            },
        )
        .is_err(),
        "XNonZero must reject X = 0"
    );
    let with_colorless = submit(
        after_red.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_red.state(), 4),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(
        attempt(
            with_colorless.state(),
            P0,
            GameCommand::ActivateWithRestrictedMana {
                source: object(with_colorless.state(), 2),
                ability: ActivatedAbilityId(1),
                granting_effect: None,
                origin: ActivationOrigin::Untargeted,
                chosen_x: Some(1.into()),
                payment: Some(colorless_payment(1)),
                restricted_mana: vec![],
            },
        )
        .is_err(),
        "OnlySpendManaType(Black) must reject colorless mana"
    );
    // Non-snow colorless mana must also be rejected by PaySnowMana(1).
    assert!(
        attempt(
            with_colorless.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: object(with_colorless.state(), 3),
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "PaySnowMana(1) must reject non-snow mana"
    );

    // Activate rats with Black mana (X = 1) -> +1 life (22 -> 23).
    let with_black = submit(
        after_red.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_red.state(), 5),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let act_black = submit(
        with_black.state(),
        P0,
        GameCommand::ActivateWithRestrictedMana {
            source: object(with_black.state(), 2),
            ability: ActivatedAbilityId(1),
            granting_effect: None,
            origin: ActivationOrigin::Untargeted,
            chosen_x: Some(1.into()),
            payment: Some(mana_payment(ManaType::Black, 1)),
            restricted_mana: vec![],
        },
    );
    let after_black = pass_pair(act_black.state());
    assert_eq!(
        after_black.state().execution().life_total(P0),
        Some(&23.into())
    );

    // 3. PaySnowMana(1) succeeds with mana produced by snow rock -> +3 life (23 -> 26).
    let with_snow = submit(
        after_black.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_black.state(), 6),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let act_snow = submit(
        with_snow.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(with_snow.state(), 3),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_snow = pass_pair(act_snow.state());
    assert_eq!(
        after_snow.state().execution().life_total(P0),
        Some(&26.into())
    );

    // 4. ExileTopLibrary(1) exiles card 20 and resolves (+1 life: 26 -> 27).
    let act_ex_top = submit(
        after_snow.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(after_snow.state(), 3),
            ability: ActivatedAbilityId(2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_ex_top = pass_pair(act_ex_top.state());
    assert_eq!(
        after_ex_top
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_ex_top.state(), 20))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        after_ex_top.state().execution().life_total(P0),
        Some(&27.into())
    );

    // 5. RevealSource activates from hand, keeps card 7 in hand, resolves (+4 life: 27 -> 31).
    let act_reveal = submit(
        after_ex_top.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(after_ex_top.state(), 7),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_reveal = pass_pair(act_reveal.state());
    assert_eq!(
        after_reveal
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_reveal.state(), 7))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    assert_eq!(
        after_reveal.state().execution().life_total(P0),
        Some(&31.into())
    );

    // 6. SacrificeAnyNumber sacrifices 2 creatures, binds X = 2, resolves (+2 life: 31 -> 33).
    let b1 = object(after_reveal.state(), 8);
    let b2 = object(after_reveal.state(), 9);
    let act_sac_any = submit(
        after_reveal.state(),
        P0,
        GameCommand::ActivateWithCostObjects {
            source: object(after_reveal.state(), 3),
            ability: ActivatedAbilityId(3),
            granting_effect: None,
            target: None,
            objects: vec![b1, b2],
            payment: Some(ManaPayment::default()),
        },
    );
    let after_sac_any = pass_pair(act_sac_any.state());
    assert_eq!(
        after_sac_any.state().execution().life_total(P0),
        Some(&33.into())
    );
    assert_eq!(
        after_sac_any
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_sac_any.state(), 8))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        after_sac_any
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_sac_any.state(), 9))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
}

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

#[test]
fn grant_spell_keyword_flash_and_first_spell_rule_discount() {
    let vocab = vocabulary();

    let mut flash_granter = Program::enchantment(None);
    flash_granter.continuous = Some(StaticContinuousAbility::CastPermission {
        id: StaticAbilityId(1),
        permission: CastPermissionRule::GrantSpellKeyword {
            matching: SpellFilter::creature(),
            keyword: KeywordAbility::Flash,
        },
    });
    let flash_granter = validated(flash_granter, &vocab);

    let mut first_discount = Program::enchantment(None);
    first_discount.continuous = Some(StaticContinuousAbility::CostModifier {
        id: StaticAbilityId(1),
        modifier: SpellCostModifier::FirstSpellRule {
            filter: SpellFilter::any(),
            own_turn_only: true,
            free: false,
            pay_life_mv: false,
            discount: 2,
        },
    });
    let first_discount = validated(first_discount, &vocab);

    let bear_spell = validated(
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );
    let two_mana_life = validated(
        Program::instant(
            ManaCost::generic(2.into()),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(1),
                }],
            ),
        ),
        &vocab,
    );

    // Part A: FirstSpellRule discounts first spell on own turn from {2} to {0}, but NOT second.
    let game_a = world(
        vocab.clone(),
        vec![
            card(1, first_discount, true),
            card(2, validated(colorless_rock(2), &vocab), true),
            card(3, two_mana_life.clone(), false),
            card(4, two_mana_life, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let after_first = cast_untargeted(&game_a, 3);
    assert_eq!(
        after_first.state().execution().life_total(P0),
        Some(&21.into())
    );

    assert!(
        attempt(
            after_first.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(after_first.state(), 4),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "second spell this turn must not receive FirstSpellRule discount"
    );
    let funded = submit(
        after_first.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_first.state(), 2),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_second = submit(
        funded.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(funded.state(), 4),
            payment: Some(colorless_payment(2)),
        },
    );
    let after_second = pass_pair(cast_second.state());
    assert_eq!(
        after_second.state().execution().life_total(P0),
        Some(&22.into())
    );

    // Part B: GrantSpellKeyword(Flash) allows casting a creature spell on opponent's turn.
    let game_b = world(
        vocab.clone(),
        vec![
            card(1, flash_granter, true),
            card(2, bear_spell, false),
            library_top(10, P1, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1)],
    );
    let p0_on_p1_turn = advance_to_p1_upkeep_p0_priority(&game_b);
    let flash_cast = submit(
        p0_on_p1_turn.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(p0_on_p1_turn.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let entered = pass_in_order(flash_cast.state(), [P0, P1]);
    assert_eq!(
        entered
            .state()
            .execution()
            .window()
            .cards()
            .get(object(entered.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield,
        "creature spell granted Flash via GrantSpellKeyword must cast on opponent's turn"
    );
}

// ============================================================================
// 14. KickerExtra, ConditionalCastCost::Alternative, GraveyardOrExile, CastTiming
// ============================================================================

#[test]
fn kicker_extra_conditional_alternative_graveyard_or_exile_and_cast_timing() {
    let vocab = vocabulary();

    // 1. KickerExtra (pay 3 life as kicker extra cost -> gains 5 life when kicked).
    let mut kicked_life = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::event(EventValueKind::KickerCount(0)).scaled(5),
            }],
        ),
    );
    kicked_life.additional_cast_costs = vec![AdditionalCastCost::KickerExtra {
        mana: ManaCost::free(),
        extra: vec![ExtraActivationCost::PayLife(LifeCostAmount::Fixed(3))],
    }];
    let kicked_life = validated(kicked_life, &vocab);

    // 2. ConditionalCastCost::Alternative ({4} spell cast for {0} + pay 4 life).
    let mut alt_spell = Program::instant(
        ManaCost::generic(4.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    );
    alt_spell.conditional_cost = Some(ConditionalCastCost::Alternative {
        condition: None,
        mana: ManaCost::free(),
        extra: vec![ExtraActivationCost::PayLife(LifeCostAmount::Fixed(4))],
    });
    let alt_spell = validated(alt_spell, &vocab);

    // 3. GraveyardSelfCast::GraveyardOrExile (castable from both graveyard and exile).
    let mut squee = Program::creature(Some(ManaCost::free()), 2.into(), 1.into(), BTreeSet::new());
    squee.graveyard_self_cast = Some(GraveyardSelfCast::GraveyardOrExile);
    let squee = validated(squee, &vocab);

    // 4. CastTimingRestriction::XNonZero and CastTimingRestriction::AfterCombat.
    let mut x_nonzero_spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::chosen_x(),
            }],
        ),
    );
    x_nonzero_spell.x_mana_symbols = 1;
    x_nonzero_spell.cast_timing = Some(CastTimingRestriction::XNonZero);
    let x_nonzero_spell = validated(x_nonzero_spell, &vocab);

    let mut after_combat_spell = gain_life_instant(2);
    after_combat_spell.cast_timing = Some(CastTimingRestriction::AfterCombat);
    let after_combat_spell = validated(after_combat_spell, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, kicked_life.clone(), false),
            card(2, kicked_life, false),
            card(3, alt_spell, false),
            graveyard_of(4, P0, squee.clone()),
            exile_of(5, P0, squee),
            card(6, x_nonzero_spell, false),
            card(7, after_combat_spell, false),
            card(8, validated(colorless_rock(1), &vocab), true),
        ],
        vec![CardId(8)],
    );

    // KickerExtra: unkicked leaves life at 20; kicked pays 3 life then gains 5 -> 22.
    let after_unkicked = cast_untargeted(&game, 1);
    assert_eq!(
        after_unkicked.state().execution().life_total(P0),
        Some(&20.into())
    );
    let cast_kicked = submit(
        after_unkicked.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_unkicked.state(), 2),
            target: None,
            payment: CastingPayment {
                additional: BTreeMap::from([(0, 1.into())]),
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_kicked = pass_pair(cast_kicked.state());
    assert_eq!(
        after_kicked.state().execution().life_total(P0),
        Some(&22.into()),
        "KickerExtra must pay 3 life and resolve with KickerCount(0) == 1 (+5 life)"
    );

    // ConditionalCastCost::Alternative: cast {4} spell for {0} + 4 life -> 22 - 4 + 1 = 19.
    let cast_alt = submit(
        after_kicked.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_kicked.state(), 3),
            target: None,
            payment: CastingPayment {
                use_alternative: true,
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_alt = pass_pair(cast_alt.state());
    assert_eq!(
        after_alt.state().execution().life_total(P0),
        Some(&19.into())
    );

    // GraveyardOrExile: cast from graveyard (4) AND from exile (5).
    let after_gy = cast_untargeted(after_alt.state(), 4);
    assert_eq!(
        after_gy
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_gy.state(), 4))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    let after_ex = cast_untargeted(after_gy.state(), 5);
    assert_eq!(
        after_ex
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_ex.state(), 5))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );

    // CastTimingRestriction::XNonZero rejects X = 0 and accepts X = 1.
    assert!(
        attempt(
            after_ex.state(),
            P0,
            GameCommand::CastWithResources {
                spell: object(after_ex.state(), 6),
                target: None,
                payment: CastingPayment {
                    chosen_x: Some(0.into()),
                    mana: Some(ManaPayment::default()),
                    ..CastingPayment::default()
                },
            },
        )
        .is_err(),
        "CastTimingRestriction::XNonZero must reject X = 0"
    );
    let funded = submit(
        after_ex.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_ex.state(), 8),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_x1 = submit(
        funded.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(funded.state(), 6),
            target: None,
            payment: CastingPayment {
                chosen_x: Some(1.into()),
                mana: Some(colorless_payment(1)),
                ..CastingPayment::default()
            },
        },
    );
    let after_x1 = pass_pair(cast_x1.state());
    assert_eq!(
        after_x1.state().execution().life_total(P0),
        Some(&20.into())
    );

    // CastTimingRestriction::AfterCombat rejects PrecombatMain and accepts PostcombatMain.
    assert!(
        attempt(
            after_x1.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(after_x1.state(), 7),
                payment: Some(ManaPayment::default()),
            },
        )
        .is_err(),
        "CastTimingRestriction::AfterCombat must reject PrecombatMain"
    );
    let postcombat = advance_to_postcombat_main(after_x1.state(), [P0, P1]);
    let after_post = cast_untargeted(&postcombat, 7);
    assert_eq!(
        after_post.state().execution().life_total(P0),
        Some(&22.into())
    );
}

// ============================================================================
// 15. StaticDamageOp::{PreventUpToExpr, PreventAndFollowup}, ConditionalModifyDamage,
//     EventValueKind::DamagePreventedThisWay
// ============================================================================

#[test]
fn static_damage_prevent_up_to_expr_prevent_and_followup_and_conditional_modify() {
    let vocab = vocabulary();

    // Case A: PreventUpToExpr prevents up to controlled artifacts (2 of 5 -> P0 takes 3).
    let mut ward_expr = Program::enchantment(None);
    ward_expr.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(1),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::Controller,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventUpToExpr(Box::new(ValueExpr::controlled_type(
                CardType::Artifact,
            ))),
        },
    });
    let ward_expr = validated(ward_expr, &vocab);

    let relic = || validated(Program::artifact(Some(ManaCost::free()), vec![]), &vocab);
    let bolt5 = validated(damage_any_instant(5), &vocab);
    let game_a = world(
        vocab.clone(),
        vec![
            card(1, ward_expr, true),
            card(2, relic(), true),
            card(3, relic(), true),
            card(4, bolt5, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let after_a = cast_targeted(&game_a, P0, 4, TargetRef::Player(P0));
    assert_eq!(
        after_a.state().execution().life_total(P0),
        Some(&17.into()),
        "PreventUpToExpr(2 artifacts) must prevent 2 of 5 damage"
    );

    // Case B: PreventAndFollowup (PutCountersOnRecipient on Vigor creature + InlineBody with
    // DamagePreventedThisWay on Reverse Damage enchantment).
    let mut vigor = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    vigor.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(1),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::SelfObject,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventAndFollowup {
                mode: DamagePreventMode::All,
                followup: DamagePreventFollowup::PutCountersOnRecipient {
                    kind: CounterKind::PlusOne,
                    per_prevented: true,
                },
            },
        },
    });
    let vigor = validated(vigor, &vocab);

    let mut reverse_ward = Program::enchantment(None);
    reverse_ward.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(1),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::Controller,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventAndFollowup {
                mode: DamagePreventMode::All,
                followup: DamagePreventFollowup::InlineBody(Box::new(ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::event(EventValueKind::DamagePreventedThisWay),
                    }],
                ))),
            },
        },
    });
    let reverse_ward = validated(reverse_ward, &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, vigor, true),
            card(2, reverse_ward, true),
            card(3, validated(damage_any_instant(3), &vocab), false),
            card(4, validated(damage_any_instant(4), &vocab), false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let vigor_id = object(&game_b, 1);
    let after_vigor = cast_targeted(&game_b, P0, 3, TargetRef::Object(vigor_id));
    assert_eq!(
        after_vigor
            .state()
            .execution()
            .creature(vigor_id)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    assert_eq!(
        after_vigor
            .state()
            .execution()
            .counters_of_kind(vigor_id, CounterKind::PlusOne),
        3.into(),
        "PreventAndFollowup(PutCountersOnRecipient) must place 3 +1/+1 counters"
    );

    let after_reverse = cast_targeted(after_vigor.state(), P0, 4, TargetRef::Player(P0));
    assert_eq!(
        after_reverse.state().execution().life_total(P0),
        Some(&24.into()),
        "PreventAndFollowup(InlineBody) must gain life equal to DamagePreventedThisWay (4)"
    );

    // Case C: ConditionalModifyDamage (+2 damage to opponents only while controlling an artifact).
    let mut cond_amp = Program::enchantment(None);
    cond_amp.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(1),
        rule: StaticDamageRule::ConditionalModifyDamage {
            condition: Box::new(StaticSourceCondition::ControlsCardType {
                kind: CardType::Artifact,
                minimum: 1,
            }),
            source: None,
            source_controller: PlayerRelation::Controller,
            recipient: StaticDamageRecipient::Opponents,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::Add(2),
        },
    });
    let cond_amp = validated(cond_amp, &vocab);

    let game_c = world(
        vocab.clone(),
        vec![
            card(1, cond_amp, true),
            card(2, validated(damage_any_instant(2), &vocab), false),
            card(3, relic(), false),
            card(4, validated(damage_any_instant(2), &vocab), false),
        ],
        vec![CardId(1)],
    );
    let before_art = cast_targeted(&game_c, P0, 2, TargetRef::Player(P1));
    assert_eq!(
        before_art.state().execution().life_total(P1),
        Some(&18.into()),
        "ConditionalModifyDamage must not apply before condition holds"
    );
    let with_art = cast_untargeted(before_art.state(), 3);
    let after_art = cast_targeted(with_art.state(), P0, 4, TargetRef::Player(P1));
    assert_eq!(
        after_art.state().execution().life_total(P1),
        Some(&14.into()),
        "ConditionalModifyDamage must add +2 damage (2 + 2 = 4) once controller controls an artifact"
    );
}

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
                vec![Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: ValueExpr::literal(1),
                }],
            ),
        ),
        &vocab,
    );
    let opp_discard = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOpponent,
                vec![Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    amount: ValueExpr::literal(1),
                }],
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
                BodyTarget::SingleOwnGraveyardCard,
                vec![Instruction::MoveSubject {
                    subject: MoveSubjectRef::TargetObject,
                    destination: SubjectDestination::Battlefield {
                        tapped: false,
                        attacking: false,
                        transformed: false,
                        under_owner_control: false,
                        with_counter: None,
                        attach_to_self: false,
                    },
                }],
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

#[test]
fn modify_tokens_filtered_and_steal_created_tokens_including_temporary_static() {
    let vocab = vocabulary();

    let soldier = soldier_token(&vocab);
    let relic = relic_artifact_token(&vocab);

    // Part A: ModifyTokensFiltered doubles ONLY creature tokens, not noncreature artifact tokens.
    let mut creature_doubler = Program::enchantment(None);
    creature_doubler.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(1),
        rule: StaticModifierRule::ModifyTokensFiltered {
            filter: Box::new(ObjectFilter::creature()),
            controller: PlayerRelation::Controller,
            add: 0,
            multiply: 2,
        },
    });
    let creature_doubler = validated(creature_doubler, &vocab);

    let make_soldier = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(soldier.clone())],
                }],
            ),
        ),
        &vocab,
    );
    let make_relic = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(relic)],
                }],
            ),
        ),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, creature_doubler, true),
            card(2, make_soldier.clone(), false),
            card(3, make_relic, false),
        ],
        vec![CardId(1)],
    );
    let drain_timestamps = |mut cand: NativeCandidate| {
        while let Some(decision) = cand.state().pending_decision() {
            let RuleFrame::TimestampOrder(frame) = decision.frame() else {
                break;
            };
            let chooser = decision.chooser();
            let order = frame.members().iter().map(|m| m.member).collect();
            cand = reply(cand.state(), chooser, DecisionReply::TimestampOrder(order));
        }
        cand
    };
    let after_soldiers = drain_timestamps(cast_untargeted(&game_a, 2));
    let creature_token_count = after_soldiers
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| {
            c.zone() == ObjectZone::Battlefield
                && c.card_id().is_none()
                && current(after_soldiers.state(), c.id()).is_creature()
        })
        .count();
    assert_eq!(
        creature_token_count, 2,
        "creature token must be doubled to 2"
    );

    let after_relic = drain_timestamps(cast_untargeted(after_soldiers.state(), 3));
    let noncreature_token_count = after_relic
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| {
            c.zone() == ObjectZone::Battlefield
                && c.card_id().is_none()
                && !current(after_relic.state(), c.id()).is_creature()
        })
        .count();
    assert_eq!(
        noncreature_token_count, 1,
        "noncreature artifact token must not be doubled by creature-filtered ModifyTokensFiltered"
    );

    // Part B: Crafty Cutpurse (TemporaryStatic wrapping StealCreatedTokens).
    let cutpurse = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                        StaticContinuousAbility::Modifier {
                            id: StaticAbilityId(1),
                            rule: StaticModifierRule::StealCreatedTokens {
                                affected: PlayerRelation::Opponent,
                                token_filter: None,
                            },
                        },
                    ))),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let opp_two_soldiers = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        amount: ValueExpr::literal(2),
                        ..TokenCreation::one(soldier)
                    }],
                }],
            ),
        ),
        &vocab,
    );

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, cutpurse, false),
            opp_hand(2, opp_two_soldiers.clone()),
            opp_hand(3, opp_two_soldiers),
            library_top(10, P1, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );
    let after_cutpurse = cast_untargeted(&game_b, 1);
    let p1_prio = submit(after_cutpurse.state(), P0, GameCommand::Pass);
    let p1_cast = submit(
        p1_prio.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_stolen = drain_timestamps(pass_in_order(p1_cast.state(), [P1, P0]));
    let p0_tokens = after_stolen
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| {
            c.zone() == ObjectZone::Battlefield
                && c.card_id().is_none()
                && current(after_stolen.state(), c.id()).controller == Some(P0)
        })
        .count();
    assert_eq!(
        p0_tokens, 2,
        "Crafty Cutpurse TemporaryStatic must steal both of P1's tokens under P0's control"
    );

    // Advance to Turn 2 after cleanup expires TemporaryStatic: P1's new tokens enter under P1.
    let t2_upkeep = {
        let end = reach_end(after_stolen.state(), [P0, P1]);
        pass_pair(&end).into_state()
    };
    let p1_cast2 = submit(
        &t2_upkeep,
        P1,
        GameCommand::CastUntargeted {
            spell: object(&t2_upkeep, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_t2_tokens = drain_timestamps(pass_in_order(p1_cast2.state(), [P1, P0]));
    let p1_tokens = after_t2_tokens
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| {
            c.zone() == ObjectZone::Battlefield
                && c.card_id().is_none()
                && current(after_t2_tokens.state(), c.id()).controller == Some(P1)
        })
        .count();
    assert_eq!(
        p1_tokens, 2,
        "expired StealCreatedTokens must allow P1 to keep tokens created on Turn 2"
    );
}

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
        extra_activated: vec![],
        extra_triggered: vec![],
        extra_static: vec![],
        override_pt: None,
        extra_colors: BTreeSet::new(),
        keep_name: true,
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

#[test]
fn turn_and_provenance_trackers_evaluate_accurately() {
    let vocab = vocabulary();

    // 1. SpellsCastMatchingThisTurn (Veil of Summer check: opponent cast a blue or black spell).
    let veil_check = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players: AffectedPlayers::EachOpponent,
                        filter: Box::new(ObjectFilter::any().with_color(
                            ObjectColorFilter::any_of(ColorMask::BLUE.union(ColorMask::BLACK)),
                        )),
                        from_zone: None,
                    }),
                }],
            ),
        ),
        &vocab,
    );
    let mut opp_blue_spell = gain_life_instant(0);
    opp_blue_spell.color_indicator = Some(BTreeSet::from([Color::Blue]));
    let opp_blue_spell = validated(opp_blue_spell, &vocab);

    // 2. ActivatedGraveyardAbilityThisTurn.
    let mut gy_card = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Exile {
                tap_source: false,
                from_graveyard: true,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(1),
                }],
            ),
        }],
    );
    gy_card.card_types.insert(CardType::Artifact);
    let gy_card = validated(gy_card, &vocab);

    let gy_ability_check = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::PlayerStat(
                        PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(
                            ResolvingPlayer::Controller,
                        ),
                    )
                    .scaled(3),
                }],
            ),
        ),
        &vocab,
    );

    // 3. SourceStateExpr::{WasCast, EnteredFromZone} on a creature's ETB triggers.
    let mut provenance_creature =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    provenance_creature.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(SourceStateExpr::WasCast {
                        by_controller: true,
                        from_hand: true,
                    })
                    .scaled(2),
                },
                Instruction::GainControllerLife {
                    amount: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                        SourceZoneFilter::Graveyard,
                    ))
                    .scaled(7),
                },
            ],
        ),
    });
    let provenance_creature = validated(provenance_creature, &vocab);

    let reanimate = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOwnGraveyardCard,
                vec![Instruction::MoveSubject {
                    subject: MoveSubjectRef::TargetObject,
                    destination: SubjectDestination::Battlefield {
                        tapped: false,
                        attacking: false,
                        transformed: false,
                        under_owner_control: false,
                        with_counter: None,
                        attach_to_self: false,
                    },
                }],
            ),
        ),
        &vocab,
    );

    // 4. MatchObjectRef::LibraryTop + ControlsGreatestStat.
    let top_and_greatest_check = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::GainControllerLife {
                        amount: ValueExpr::object_matches(
                            MatchObjectRef::LibraryTop(ResolvingPlayer::Controller),
                            ObjectFilter::creature(),
                        )
                        .scaled(10),
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestStat {
                            player: ResolvingPlayer::Controller,
                            filter: Box::new(ObjectFilter::creature()),
                            stat: ObjectStatKind::Power,
                        })
                        .scaled(100),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let big_six = validated(
        Program::creature(Some(ManaCost::free()), 6.into(), 6.into(), BTreeSet::new()),
        &vocab,
    );
    let opp_five = validated(
        Program::creature(None, 5.into(), 5.into(), BTreeSet::new()),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, veil_check.clone(), false),
            card(2, veil_check, false),
            opp_hand(3, opp_blue_spell),
            graveyard_of(4, P0, gy_card),
            card(5, gy_ability_check.clone(), false),
            card(6, gy_ability_check, false),
            card(7, provenance_creature.clone(), false),
            graveyard_of(8, P0, provenance_creature),
            card(9, reanimate, false),
            card(10, top_and_greatest_check.clone(), false),
            card(11, top_and_greatest_check, false),
            card(12, big_six.clone(), false),
            opp_battlefield(13, opp_five),
            library_top(20, P0, big_six),
        ],
        vec![CardId(13)],
    );

    // Veil check before P1 casts blue spell -> +0 life (20).
    let s1 = cast_untargeted(&game, 1);
    assert_eq!(s1.state().execution().life_total(P0), Some(&20.into()));
    // P1 casts blue spell.
    let p1_prio = submit(s1.state(), P0, GameCommand::Pass);
    let p1_cast = submit(
        p1_prio.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let s2 = pass_in_order(p1_cast.state(), [P1, P0]);
    // Veil check after P1 casts blue spell -> +1 life (20 -> 21).
    let s3 = cast_untargeted(s2.state(), 2);
    assert_eq!(s3.state().execution().life_total(P0), Some(&21.into()));

    // ActivatedGraveyardAbilityThisTurn: 0 before (+0 -> 21), activate gy ability (+1 -> 22),
    // then 1 after (+3 -> 25).
    let s4 = cast_untargeted(s3.state(), 5);
    assert_eq!(s4.state().execution().life_total(P0), Some(&21.into()));
    let act_gy = submit(
        s4.state(),
        P0,
        GameCommand::ActivateUntargetedAbility {
            source: object(s4.state(), 4),
            ability: ActivatedAbilityId(1),
            payment: Some(ManaPayment::default()),
        },
    );
    let s5 = pass_pair(act_gy.state());
    assert_eq!(s5.state().execution().life_total(P0), Some(&22.into()));
    let s6 = cast_untargeted(s5.state(), 6);
    assert_eq!(s6.state().execution().life_total(P0), Some(&25.into()));

    // WasCast vs EnteredFromZone(Graveyard):
    // Card 7 cast from hand -> WasCast == 1 (+2), EnteredFromZone(Graveyard) == 0 (+0) -> 27.
    let s7_entered = cast_untargeted(s6.state(), 7);
    let s7 = pass_pair(s7_entered.state());
    assert_eq!(s7.state().execution().life_total(P0), Some(&27.into()));

    // Card 8 reanimated from graveyard -> WasCast == 0 (+0), EnteredFromZone(Graveyard) == 1 (+7) -> 34.
    let gy_8 = object(s7.state(), 8);
    let s8_reanim = cast_targeted(s7.state(), P0, 9, TargetRef::Object(gy_8));
    let s8 = pass_pair(s8_reanim.state());
    assert_eq!(s8.state().execution().life_total(P0), Some(&34.into()));

    // LibraryTop is creature (+10), ControlsGreatestStat is 0 (P0 max 2 < P1 5) -> 34 + 10 = 44.
    let s9 = cast_untargeted(s8.state(), 10);
    assert_eq!(s9.state().execution().life_total(P0), Some(&44.into()));

    // Cast 6/6 -> now P0 controls greatest power (6 > 5): +10 (LibraryTop) + 100 (Greatest) = +110 -> 154.
    let s10 = cast_untargeted(s9.state(), 12);
    let s11 = cast_untargeted(s10.state(), 11);
    assert_eq!(s11.state().execution().life_total(P0), Some(&154.into()));
}

// ============================================================================
// 20. Wave 7 Triggers: DealsAtLeastDamage, EntersWithoutBeingPlayed, BlockedByAtLeast
// ============================================================================

#[test]
fn wave7_triggers_deals_at_least_enters_without_played_and_blocked_by_at_least() {
    let vocab = vocabulary();

    // Trigger 1 & 2 watcher enchantment.
    let mut watcher = Program::enchantment(None);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::DealsAtLeastDamage {
            source: ObjectFilter::any(),
            recipient: DamageTriggerRecipient::Opponent,
            min_damage: 3,
            exact: false,
            combat_only: Some(false),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(5),
            }],
        ),
    });
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::new(TriggerEvent::EntersWithoutBeingPlayed {
            subject: ObjectFilter::land(),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    });
    let watcher = validated(watcher, &vocab);

    // Trigger 3 attacker with BlockedByAtLeast(2).
    let mut juggernaut = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
    juggernaut.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::blocks_or_blocked(
            ObjectFilter::self_object(),
            BlockTriggerKind::BlockedByAtLeast {
                min_count: 2,
                blocker_filter: None,
            },
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(4),
            }],
        ),
    });
    let juggernaut = validated(juggernaut, &vocab);

    let reanimate_land = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOwnGraveyardCard,
                vec![Instruction::MoveSubject {
                    subject: MoveSubjectRef::TargetObject,
                    destination: SubjectDestination::Battlefield {
                        tapped: false,
                        attacking: false,
                        transformed: false,
                        under_owner_control: false,
                        with_counter: None,
                        attach_to_self: false,
                    },
                }],
            ),
        ),
        &vocab,
    );
    let blocker = || {
        validated(
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            &vocab,
        )
    };

    let game = world(
        vocab.clone(),
        vec![
            card(1, watcher, true),
            card(2, juggernaut, true),
            opp_battlefield(3, blocker()),
            opp_battlefield(4, blocker()),
            card(5, validated(damage_any_instant(2), &vocab), false),
            card(6, validated(damage_any_instant(3), &vocab), false),
            card(7, validated(basic_forest(), &vocab), false),
            graveyard_of(8, P0, validated(basic_forest(), &vocab)),
            card(9, reanimate_land, false),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );

    // 1. DealsAtLeastDamage(3): 2 damage does NOT trigger; 3 damage DOES trigger (+5 life).
    let after_2 = cast_targeted(&game, P0, 5, TargetRef::Player(P1));
    assert!(after_2.state().execution().window().entries().is_empty());
    assert_eq!(after_2.state().execution().life_total(P0), Some(&20.into()));

    let after_3 = cast_targeted(after_2.state(), P0, 6, TargetRef::Player(P1));
    assert_eq!(after_3.state().execution().window().entries().len(), 1);
    let after_3_trig = pass_pair(after_3.state());
    assert_eq!(
        after_3_trig.state().execution().life_total(P0),
        Some(&25.into())
    );

    // 2. EntersWithoutBeingPlayed: playing land 7 normally does NOT trigger; moving land 8 onto
    // battlefield via effect DOES trigger (+3 life -> 28).
    let after_played_land = submit(
        after_3_trig.state(),
        P0,
        GameCommand::PlayLand {
            land: object(after_3_trig.state(), 7),
        },
    );
    assert!(
        after_played_land
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
    let gy_land = object(after_played_land.state(), 8);
    let after_put_land =
        cast_targeted(after_played_land.state(), P0, 9, TargetRef::Object(gy_land));
    assert_eq!(
        after_put_land.state().execution().window().entries().len(),
        1
    );
    let after_land_trig = pass_pair(after_put_land.state());
    assert_eq!(
        after_land_trig.state().execution().life_total(P0),
        Some(&28.into())
    );

    // 3. BlockedByAtLeast(2): attacking juggernaut blocked by 2 blockers triggers (+4 life -> 32).
    let att_id = object(after_land_trig.state(), 2);
    let blk1 = object(after_land_trig.state(), 3);
    let blk2 = object(after_land_trig.state(), 4);
    let beg_combat = pass_pair(after_land_trig.state()).into_state();
    let dec_att = pass_pair(&beg_combat).into_state();
    let after_att = reply(
        &dec_att,
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: att_id,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let dec_blk = pass_pair(after_att.state()).into_state();
    let after_blk = reply(
        &dec_blk,
        P1,
        DecisionReply::DeclareBlockers(vec![
            BlockDeclaration {
                blocker: blk1,
                attacker: att_id,
            },
            BlockDeclaration {
                blocker: blk2,
                attacker: att_id,
            },
        ]),
    );
    assert_eq!(
        after_blk.state().execution().window().entries().len(),
        1,
        "BlockedByAtLeast(2) must trigger when blocked by 2 creatures"
    );
    let after_blk_trig = pass_pair(after_blk.state());
    assert_eq!(
        after_blk_trig.state().execution().life_total(P0),
        Some(&32.into())
    );
}

// ============================================================================
// 21. Validation & Runtime Fixes: uses_target_permanent, CFG FlowFacts Promotion,
//     OptionalEffect wrapping ExileResolvingSpell
// ============================================================================

#[test]
fn wave7_validation_uses_target_permanent_cfg_flow_facts_and_optional_exile_spell() {
    let vocab = vocabulary();

    // Part A: ObjectFilter::uses_target_permanent() (Mandate of Abaddon).
    let mandate = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::DestroyGroup(Box::new(
                    ObjectFilter::creature().with_power(PowerFilter::CompareAnchor {
                        op: Comparison::Less,
                        anchor: DynamicStatAnchor::TargetPower,
                    }),
                ))],
            ),
        ),
        &vocab,
    );

    // Part B: CFG FlowFacts promotion across OptionalEffect + BranchIfFalse(LastActionSucceeded >= 1).
    let optional_exile_then_draw = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::ExileTarget,
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: 4,
                    },
                    Instruction::DrawPlayer {
                        players: AffectedPlayers::One(ResolvingPlayer::ExiledTargetController),
                        amount: ValueExpr::literal(1),
                    },
                ],
            ),
        ),
        &vocab,
    );

    // Part C: OptionalEffect wrapping ExileResolvingSpell.
    let optional_self_exile = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(2),
                    },
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::ExileResolvingSpell,
                ],
            ),
        ),
        &vocab,
    );

    let four_four = validated(
        Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
        &vocab,
    );
    let two_two = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );
    let five_five = validated(
        Program::creature(None, 5.into(), 5.into(), BTreeSet::new()),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, four_four, true),
            opp_battlefield(2, two_two),
            opp_battlefield(3, five_five),
            card(4, mandate, false),
            card(5, optional_exile_then_draw.clone(), false),
            card(6, optional_exile_then_draw, false),
            card(7, optional_self_exile.clone(), false),
            card(8, optional_self_exile, false),
            library_top(20, P1, validated(basic_forest(), &vocab)),
            library_top(21, P1, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let target_4 = object(&game, 1);
    let opp_5 = object(&game, 3);

    // Part A runtime: Mandate targeting 4/4 destroys 2/2 and spares 4/4 and 5/5.
    let after_mandate = cast_targeted(&game, P0, 4, TargetRef::Object(target_4));
    assert_eq!(
        after_mandate
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_mandate.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1),
        "Mandate of Abaddon must destroy creatures with power less than target's power (2 < 4)"
    );
    assert_eq!(
        after_mandate
            .state()
            .execution()
            .window()
            .cards()
            .get(target_4)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        after_mandate
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_5)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );

    // Part B runtime: decline optional exile -> opp_5 survives and P1 draws 0.
    let pend_dec = cast_targeted(after_mandate.state(), P0, 5, TargetRef::Object(opp_5));
    let after_dec = reply(
        pend_dec.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(
        after_dec
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_5)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        after_dec
            .state()
            .execution()
            .window()
            .cards()
            .library(P1)
            .len(),
        2
    );

    // Accept optional exile -> opp_5 is exiled and ExiledTargetController (P1) draws 1.
    let pend_take = cast_targeted(after_dec.state(), P0, 6, TargetRef::Object(opp_5));
    let after_take = reply(
        pend_take.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(
        after_take
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_take.state(), 3))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(
        after_take
            .state()
            .execution()
            .window()
            .cards()
            .library(P1)
            .len(),
        1
    );

    // Part C runtime: OptionalEffect wrapping ExileResolvingSpell.
    let pend_exile_spell = cast_untargeted(after_take.state(), 7);
    let after_exile_spell = reply(
        pend_exile_spell.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(
        after_exile_spell.state().execution().life_total(P0),
        Some(&22.into())
    );
    assert_eq!(
        after_exile_spell
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_exile_spell.state(), 7))
            .unwrap()
            .zone(),
        ObjectZone::Exile,
        "taking OptionalEffect(ExileResolvingSpell) must exile the resolving spell"
    );

    let pend_keep_gy = cast_untargeted(after_exile_spell.state(), 8);
    let after_keep_gy = reply(
        pend_keep_gy.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(
        after_keep_gy.state().execution().life_total(P0),
        Some(&24.into())
    );
    assert_eq!(
        after_keep_gy
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_keep_gy.state(), 8))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0),
        "declining OptionalEffect(ExileResolvingSpell) must send the resolving spell to graveyard"
    );
}
