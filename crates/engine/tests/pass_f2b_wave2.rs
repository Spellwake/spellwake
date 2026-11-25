//! Runtime integration tests for Pass F2-B Wave 2 primitives:
//! - CR 701.60 `CounterKind::Suspect` (Menace + CannotBlock)
//! - `StatAggregate::Min`
//! - `Instruction::CounterSpellToLibrary { top }`
//! - `StaticRestriction` (`ControllerHasShroud`, `ControllerCantLoseOpponentsCantWin`,
//!   `ControllerCantLoseForZeroLife`, `ControllerCantWinOpponentsCantLose`,
//!   `CantPlayLands`, `PreventCounters` on permanents and players)
//! - `StaticDamageRule::PreventIncomingRemovePlusOneIfCounter` and
//!   `StaticDamageOp::{PreventAtMost, PreventHalf}`
//! - `StaticModifierRule::{BonusTokenOnCreate, OthersEnterWithCountersExpr}`
//! - `StaticSourceCondition::EntryOptionChosen`, `TriggerCondition::SourceEntryOption`,
//!   and `MatchObjectRef::LastMovedToGraveyard`.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    characteristics::{
        BlockingRestriction, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticDamageKind,
        StaticDamageOp, StaticDamageRecipient, StaticDamageRule, StaticModifierRule, StaticPart,
        StaticParts, StaticRestriction, StaticSourceCondition,
    },
    effects::{CounterKind, SeedCard},
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    land::{EntryChoice, EntryTap},
    life::LifeAdjustment,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{
        AffectedPlayers, BodyTarget, CardType, Comparison, DamageAmountSpec, DamageRecipientsRef,
        DamageSourceRef, EffectSubject, EntryCounterAbility, ExecutableBody, Instruction,
        MatchObjectRef, ObjectFilter, ObjectStatKind, PermanentDefinition, PlayerRelation, Program,
        ProgramKind, ResolutionCondition, ResolvingPlayer, SpellFilter, StatAggregate,
        TriggerCondition, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr, ValueZone,
    },
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
};
use std::collections::BTreeSet;

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

fn cast_targeted(
    game: &GameState,
    actor: spellwake_engine::priority::SeatId,
    id: u64,
    target: TargetRef,
) -> NativeCandidate {
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

#[test]
fn suspect_counter_grants_menace_and_cannot_block_until_removed() {
    let vocab = vocabulary();
    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let game = world(
        vocab,
        vec![
            card(1, bear, true),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![Instruction::PutCountersOnSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: CounterKind::Suspect,
                    amount: ValueExpr::literal(1),
                }],
            ),
            spell(
                3,
                BodyTarget::SingleCreature,
                vec![Instruction::RemoveCountersFromPermanent {
                    target_self: false,
                    kind: Some(CounterKind::Suspect),
                    amount: ValueExpr::literal(1),
                }],
            ),
        ],
        vec![CardId(1)],
    );

    let bear_id = object(&game, 1);
    let before = current(&game, bear_id);
    assert!(!before.keywords.contains(&KeywordAbility::Menace));
    assert!(
        !before
            .blocking_restrictions
            .contains(&BlockingRestriction::CannotBlock)
    );

    let suspected = cast_targeted(&game, P0, 2, TargetRef::Object(bear_id));
    let during = current(suspected.state(), bear_id);
    assert!(during.keywords.contains(&KeywordAbility::Menace));
    assert!(
        during
            .blocking_restrictions
            .contains(&BlockingRestriction::CannotBlock)
    );

    let cleared = cast_targeted(suspected.state(), P0, 3, TargetRef::Object(bear_id));
    let after = current(cleared.state(), bear_id);
    assert!(!after.keywords.contains(&KeywordAbility::Menace));
    assert!(
        !after
            .blocking_restrictions
            .contains(&BlockingRestriction::CannotBlock)
    );
}

#[test]
fn stat_aggregate_min_evaluates_least_power_among_creatures() {
    let vocab = vocabulary();
    let game = world(
        vocab,
        vec![
            card(
                1,
                Program::creature(None, 5.into(), 5.into(), BTreeSet::new()),
                true,
            ),
            card(
                2,
                Program::creature(None, 2.into(), 4.into(), BTreeSet::new()),
                true,
            ),
            card(
                3,
                Program::creature(None, 7.into(), 1.into(), BTreeSet::new()),
                true,
            ),
            spell(
                4,
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::Aggregate {
                        filter: Box::new(ObjectFilter::creature().controlled()),
                        stat: ObjectStatKind::Power,
                        op: StatAggregate::Min,
                    },
                }],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let after = cast_untargeted(&game, 4);
    assert_eq!(after.state().execution().life_total(P0), Some(&22.into()));
}

#[test]
fn counter_spell_to_library_puts_countered_spell_on_top_or_bottom() {
    let vocab = vocabulary();
    let lib_a = SeedCard {
        card: CardId(10),
        owner: P0,
        destination: ObjectDestination::LibraryBottom,
        program: Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
    };
    let counter_top = SeedCard {
        card: CardId(2),
        owner: P1,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(SpellFilter::any()),
                vec![Instruction::CounterSpellToLibrary { top: true }],
            ),
        ),
    };
    let counter_bottom = SeedCard {
        card: CardId(4),
        owner: P1,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(SpellFilter::any()),
                vec![Instruction::CounterSpellToLibrary { top: false }],
            ),
        ),
    };
    let game = world(
        vocab,
        vec![
            spell(
                1,
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(4),
                }],
            ),
            counter_top,
            spell(
                3,
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(4),
                }],
            ),
            counter_bottom,
            lib_a,
        ],
        vec![],
    );

    // P0 casts spell 1, passes to P1, P1 counters it to top of P0's library.
    let cast1 = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let spell1_entry = cast1
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let pass_to_p1 = submit(cast1.state(), P0, GameCommand::Pass);
    let after_top = cast_targeted(pass_to_p1.state(), P1, 2, TargetRef::Stack(spell1_entry));
    let lib = after_top.state().execution().window().cards().library(P0);
    assert_eq!(lib.len(), 2);
    assert_eq!(lib[0], object(after_top.state(), 1));
    assert_eq!(
        after_top.state().execution().life_total(P0),
        Some(&20.into())
    );

    // P0 casts spell 3, P1 counters it to bottom of P0's library.
    let cast3 = submit(
        after_top.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_top.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let spell3_entry = cast3
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let pass_to_p1 = submit(cast3.state(), P0, GameCommand::Pass);
    let after_bot = cast_targeted(pass_to_p1.state(), P1, 4, TargetRef::Stack(spell3_entry));
    let lib = after_bot.state().execution().window().cards().library(P0);
    assert_eq!(lib.len(), 3);
    assert_eq!(*lib.last().unwrap(), object(after_bot.state(), 3));
}

#[test]
fn shroud_cant_lose_and_cant_play_lands_restrictions_are_enforced() {
    let vocab = vocabulary();

    // Ivory Mask (ControllerHasShroud) blocks even P0 from targeting P0.
    let mut mask = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    mask.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ControllerHasShroud,
    });
    let mut angel = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
    angel.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(2),
        rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
    });
    let mut land_lock = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    land_lock.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(3),
        rule: StaticRestriction::CantPlayLands(PlayerRelation::Controller),
    });
    let land = Program::new(
        BTreeSet::from([CardType::Land]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: None,
        }),
    );

    let game = world(
        vocab,
        vec![
            card(1, mask, true),
            card(2, angel, true),
            card(3, land_lock, true),
            card(4, land, false),
            spell(
                5,
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(1))),
                }],
            ),
            spell(
                6,
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Lose(25.into()),
                }],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    // Shroud rejects targeting P0.
    let shroud_attempt = observe_advance(
        &game,
        GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::CastPaid {
                spell: object(&game, 5),
                target: TargetRef::Player(P0),
                payment: ManaPayment::default(),
            },
        },
    );
    assert!(shroud_attempt.is_err());

    // CantPlayLands rejects P0's land play.
    let land_attempt = observe_advance(
        &game,
        GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::PlayLand {
                land: object(&game, 4),
            },
        },
    );
    assert!(land_attempt.is_err());

    // ControllerCantLoseOpponentsCantWin keeps P0 alive at -5 life!
    let survive = cast_untargeted(&game, 6);
    assert_eq!(
        survive.state().execution().life_total(P0),
        Some(&(-5).into())
    );
    assert!(survive.state().execution().state_based_losses().is_empty());
}

#[test]
fn prevent_counters_blocks_permanent_and_player_poison_counters() {
    let vocab = vocabulary();
    let mut solemnity_perm = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    solemnity_perm.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::PreventCounters {
            selector: Some(Box::new(PermanentSelector {
                controller: ControllerSelector::Any,
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            })),
            players: None,
            counter_kind: None,
        },
    });
    let mut melira_poison = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    melira_poison.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(2),
        rule: StaticRestriction::PreventCounters {
            selector: None,
            players: Some(PlayerRelation::Controller),
            counter_kind: Some(CounterKind::Poison),
        },
    });
    let bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());

    let game = world(
        vocab,
        vec![
            card(1, solemnity_perm, true),
            card(2, melira_poison, true),
            card(3, bear, true),
            spell(
                4,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::TargetObject),
                        kind: CounterKind::PlusOne,
                        amount: ValueExpr::literal(3),
                    },
                    Instruction::AddPlayerCounter {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        kind: CounterKind::Poison,
                        amount: ValueExpr::literal(2),
                    },
                ],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    let bear_id = object(&game, 3);
    let after = cast_targeted(&game, P0, 4, TargetRef::Object(bear_id));
    let pt_after = current(after.state(), bear_id).power_toughness.unwrap();
    assert_eq!(pt_after.derived.power, 2.into());
    assert_eq!(after.state().execution().poison_counters(P0), 0.into());
}

#[test]
fn damage_prevention_if_counter_at_most_and_half_rules_work() {
    let vocab = vocabulary();

    // Oathsworn Knight style: prevents damage only while it has a +1/+1 counter, removing one.
    let mut knight = Program::creature(Some(ManaCost::free()), 0.into(), 4.into(), BTreeSet::new());
    knight.entry_counter_abilities = vec![EntryCounterAbility::DynamicCounter {
        kind: CounterKind::PlusOne,
        amount: ValueExpr::literal(1),
    }];
    knight.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(1),
        rule: StaticDamageRule::PreventIncomingRemovePlusOneIfCounter,
    });

    // PreventAtMost(2): prevents 2 damage completely, lets 3 damage through untouched.
    let mut ward = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    ward.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(2),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::Controller,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventAtMost(2),
        },
    });
    // PreventHalf { round_up: true }: 5 damage to P1 -> prevent ceil(5/2)=3 -> deals 2!
    let mut half_shield = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    half_shield.continuous = Some(StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(3),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::Opponents,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventHalf { round_up: true },
        },
    });

    let game = world(
        vocab,
        vec![
            card(1, knight, false),
            card(2, ward, true),
            card(3, half_shield, true),
            spell(
                4,
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(2))),
                }],
            ),
            spell(
                5,
                BodyTarget::SingleCreature,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(2))),
                }],
            ),
            spell(
                6,
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(2))),
                }],
            ),
            spell(
                7,
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(3))),
                }],
            ),
            spell(
                8,
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(5))),
                }],
            ),
        ],
        vec![CardId(2), CardId(3)],
    );

    // Cast Knight so it enters with 1 +1/+1 counter.
    let with_knight = cast_untargeted(&game, 1);
    let knight_id = object(with_knight.state(), 1);
    assert_eq!(
        current(with_knight.state(), knight_id)
            .power_toughness
            .unwrap()
            .derived
            .power,
        1.into()
    );

    // First 2-damage hit is prevented and removes the +1/+1 counter (0 damage marked).
    let hit1 = cast_targeted(with_knight.state(), P0, 4, TargetRef::Object(knight_id));
    assert_eq!(
        current(hit1.state(), knight_id)
            .power_toughness
            .unwrap()
            .derived
            .power,
        0.into()
    );
    assert_eq!(
        hit1.state()
            .execution()
            .battlefield_marked_damage(knight_id),
        Some(0.into())
    );

    // Second 2-damage hit at 0 counters is NOT prevented (2 damage marked!).
    let hit2 = cast_targeted(hit1.state(), P0, 5, TargetRef::Object(knight_id));
    assert_eq!(
        hit2.state()
            .execution()
            .battlefield_marked_damage(knight_id),
        Some(2.into())
    );

    // PreventAtMost(2) on P0: 2 damage prevented (life stays 20), 3 damage goes through (life becomes 17).
    let p0_two = cast_targeted(hit2.state(), P0, 6, TargetRef::Player(P0));
    assert_eq!(p0_two.state().execution().life_total(P0), Some(&20.into()));
    let p0_three = cast_targeted(p0_two.state(), P0, 7, TargetRef::Player(P0));
    assert_eq!(
        p0_three.state().execution().life_total(P0),
        Some(&17.into())
    );

    // PreventHalf { round_up: true } on P1: 5 damage -> 3 prevented, 2 dealt (life 20 -> 18).
    let p1_five = cast_targeted(p0_three.state(), P0, 8, TargetRef::Player(P1));
    assert_eq!(p1_five.state().execution().life_total(P1), Some(&18.into()));
}

#[test]
fn bonus_token_on_create_and_others_enter_with_counters_expr() {
    let vocab = vocabulary();
    let squirrel = vocab.resolve_label("Squirrel").unwrap();
    let saproling = vocab.resolve_label("Saproling").unwrap();

    let squirrel_tok = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::from([squirrel])),
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(squirrel)],
        },
        &vocab,
    )
    .unwrap();
    let saproling_tok = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::from([saproling])),
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(saproling)],
        },
        &vocab,
    )
    .unwrap();

    // Chatterfang style (per_created: true) + Peregrin style (per_created: false).
    let mut chatterfang = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    chatterfang.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(1),
        rule: StaticModifierRule::BonusTokenOnCreate {
            controller: PlayerRelation::Controller,
            token: Box::new(squirrel_tok.clone()),
            count: 1,
            per_created: true,
        },
    });
    let mut peregrin = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    peregrin.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(2),
        rule: StaticModifierRule::BonusTokenOnCreate {
            controller: PlayerRelation::Controller,
            token: Box::new(squirrel_tok),
            count: 1,
            per_created: false,
        },
    });
    // OthersEnterWithCountersExpr: other creatures enter with +1/+1 counters equal to creatures P0 controls.
    let mut mentor = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    mentor.continuous = Some(StaticContinuousAbility::Modifier {
        id: StaticAbilityId(3),
        rule: StaticModifierRule::OthersEnterWithCountersExpr {
            selector: Box::new(PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                exclude_source: true,
                ..PermanentSelector::default()
            }),
            kind: CounterKind::PlusOne,
            amount: Box::new(ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(ObjectFilter::creature().controlled()),
                controller_scope: None,
            }),
        },
    });

    let game = world(
        vocab,
        vec![
            card(1, chatterfang, true),
            card(2, peregrin, true),
            spell(
                3,
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        enters_tapped: false,
                        definition: saproling_tok,
                        amount: ValueExpr::literal(2),
                        base_size: None,
                        recipients: TokenRecipients::Controller,
                    }],
                }],
            ),
            card(4, mentor, true),
            card(
                5,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
                false,
            ),
        ],
        vec![CardId(1), CardId(2), CardId(4)],
    );

    // Creating 2 Saprolings with Chatterfang (+2 Squirrels) and Peregrin (+1 Squirrel) => 5 tokens!
    let mut after_tokens = cast_untargeted(&game, 3);
    if let Some(RuleFrame::TimestampOrder(order)) =
        after_tokens.state().pending_decision().map(|d| d.frame())
    {
        let members = order.members().iter().map(|m| m.member).collect();
        after_tokens = reply(
            after_tokens.state(),
            P0,
            DecisionReply::TimestampOrder(members),
        );
    }
    let token_count = after_tokens
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.zone() == ObjectZone::Battlefield && o.card_id().is_none())
        .count();
    assert_eq!(token_count, 5);

    // Cast bear (card 5): P0 currently controls 3 nontoken + 5 token = 8 creatures, so mentor gives 8 +1/+1 counters -> 10/10!
    let after_bear = cast_untargeted(after_tokens.state(), 5);
    let bear_pt = current(after_bear.state(), object(after_bear.state(), 5))
        .power_toughness
        .unwrap();
    assert_eq!(bear_pt.derived.power, 10.into());
    assert_eq!(bear_pt.derived.toughness, 10.into());
}

#[test]
fn entry_option_chosen_and_last_moved_to_graveyard_condition() {
    let vocab = vocabulary();

    // Siege permanent: ChooseKhansOrDragons. Option 0 -> +2/+0 to controlled creatures;
    // Option 1 -> ETB trigger for creatures you control that gains 3 life.
    let make_siege = || {
        let mut siege = Program::new(
            BTreeSet::from([CardType::Enchantment]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: Some(EntryTap::ChooseKhansOrDragons),
                attachment: None,
                printed_pt: None,
                casting_cost: Some(ManaCost::free()),
            }),
        );
        siege.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: BTreeSet::from([CardType::Creature]),
                source_condition: Some(StaticSourceCondition::EntryOptionChosen(0)),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(2.into()),
                toughness: NumericExpression::Literal(0.into()),
            })]),
        });
        let mut trig_spec = TriggerSpec::enters(ObjectFilter::creature().controlled());
        trig_spec.intervening_if = Some(TriggerCondition::SourceEntryOption(1));
        siege.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: trig_spec,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(3),
                }],
            ),
        });
        siege
    };

    // Destroy target creature; if a creature card moved to graveyard this way, gain 4 life!
    let destroy_and_check = spell(
        3,
        BodyTarget::SingleCreature,
        vec![
            Instruction::DestroyTarget,
            Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::LastMovedToGraveyard,
                        filter: Box::new(ObjectFilter::creature()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                destination: 3,
            },
            Instruction::GainControllerLife {
                amount: ValueExpr::literal(4),
            },
        ],
    );

    let game = world(
        vocab,
        vec![
            card(1, make_siege(), false),
            card(
                2,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
                true,
            ),
            destroy_and_check,
            card(4, make_siege(), false),
            card(
                5,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
                false,
            ),
        ],
        vec![CardId(2)],
    );

    // Cast siege 1 -> choose option 0 (Khans) -> bear becomes 4/2!
    let cast1 = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let prompted1 = pass_pair(cast1.state());
    assert!(matches!(
        prompted1.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(_))
    ));
    let chosen0 = reply(
        prompted1.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::OptionIndex(0)),
    );
    assert_eq!(
        current(chosen0.state(), object(chosen0.state(), 2))
            .power_toughness
            .unwrap()
            .derived
            .power,
        4.into()
    );

    // Cast destroy_and_check on bear -> bear dies to graveyard -> LastMovedToGraveyard matches creature -> P0 gains 4 life (20 -> 24)!
    let bear_id = object(chosen0.state(), 2);
    let after_destroy = cast_targeted(chosen0.state(), P0, 3, TargetRef::Object(bear_id));
    assert_eq!(
        after_destroy.state().execution().life_total(P0),
        Some(&24.into())
    );

    // Cast siege 2 -> choose option 1 (Dragons) -> casting creature 5 triggers SourceEntryOption(1) -> P0 gains 3 life (24 -> 27)!
    let cast2 = submit(
        after_destroy.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_destroy.state(), 4),
            payment: Some(ManaPayment::default()),
        },
    );
    let prompted2 = pass_pair(cast2.state());
    let chosen1 = reply(
        prompted2.state(),
        P0,
        DecisionReply::EntryChoice(EntryChoice::OptionIndex(1)),
    );
    let after_creature = cast_untargeted(chosen1.state(), 5);
    // Resolve the triggered ability stacked by siege 2.
    let after_trigger = pass_pair(after_creature.state());
    assert_eq!(
        after_trigger.state().execution().life_total(P0),
        Some(&27.into())
    );
}
