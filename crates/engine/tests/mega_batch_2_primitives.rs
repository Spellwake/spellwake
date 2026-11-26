#![forbid(unsafe_code)]
//! Behavioral integration tests for Mega-Batch 2 engine primitives:
//! - `Instruction::EmpowerJace` (creates blue Jace planeswalker token + adds loyalty; reuses existing Jace token)
//! - `Instruction::TurnFaceDownSubject` (turns nontoken single-faced permanent face down; skips DFCs per CR 712.16)
//! - `Instruction::AddOneOfEachCounterKindOnSubject` (adds 1 of each counter kind present; no-op when empty)
//! - `DamageRecipientsRef::EachSourceToItself` and `EachSourceController`
//! - `Instruction::DivideDamageExprAmongTargets` (dynamic X shares and `max_targets` cap; atomic invalid-share rejection)
//! - `ExecutableBody::with_up_to_x_target_count` (0..=X targets accepted; >X rejected atomically)
//! - `SpellDiscount::Dynamic` with `ValueZone::OpponentsHands`
//! - `PermanentSelector::toughness_greater_than_power`
//! - `StaticRestriction::ControllerCantLoseGame` (prevents controller loss at negative life)
//! - `TemporaryRuleKind::CantCastFromZone` (blocks graveyard casts while permitting hand casts)
//! - `EntryTap::UnlessRevealHandSubtype` (`or_control: true` enters untapped when controlling subtype)
//! - `PlayerTriggerAction::DrewCard` opponent ordinal and `DrewCardExceptFirstInDrawStep`

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::{
        ControllerSelector, NumericPair, PermanentSelector, PowerToughness, ReplaceableEvent,
        ResolvedDuration, SpellCostModifier, SpellDiscount, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts, StaticReplacementAction,
        StaticRestriction,
    },
    effects::{CounterKind, SeedCard},
    game::{GameCommand, GameError, GameInput, GameState, NativeCandidate},
    land::EntryTap,
    life::LifeAdjustment,
    mana::{
        CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, DamageAmountSpec, DamageRecipientsRef,
        DamageSourceRef, EffectSubject, ExecutableBody, Instruction, LookTopKeepDest,
        LookTopRestDest, MoveSubjectRef, ObjectFilter, PermanentDefinition, PermanentTargetAction,
        PlaneswalkerSubtype, PlayerRelation, PlayerTriggerAction, Program, ProgramKind,
        ResolvingPlayer, SourceZoneFilter, SpellFilter, SubjectDestination, TemporaryRuleKind,
        TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr, ValueZone,
    },
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU16;

fn validated(program: Program, v: &CreatureTypeVocabulary) -> Program {
    program.validate(v).expect("program must validate");
    program
}

fn vanilla(p: i64, t: i64) -> Program {
    Program::creature(Some(ManaCost::free()), p.into(), t.into(), BTreeSet::new())
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

fn cast_untargeted(game: &GameState, card_id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, card_id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

fn cast_targeted(game: &GameState, card_id: u64, target: TargetRef) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, card_id),
            target,
            payment: ManaPayment::default(),
        },
    );
    pass_pair(cast.state())
}

#[test]
fn empower_jace_creates_then_reuses_jace_planeswalker_token() {
    let v = vocabulary();
    let empower_3 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::EmpowerJace {
                    count: ValueExpr::literal(3),
                }],
            ),
        ),
        &v,
    );
    let empower_2 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::EmpowerJace {
                    count: ValueExpr::literal(2),
                }],
            ),
        ),
        &v,
    );
    let game = world(
        v,
        vec![card(1, empower_3, false), card(2, empower_2, false)],
        vec![],
    );

    let after_first = cast_untargeted(&game, 1);
    let jace_tokens: Vec<_> = after_first
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap()
        .objects
        .values()
        .filter(|o| {
            o.zone == ObjectZone::Battlefield
                && o.controller == Some(P0)
                && o.current_planeswalker_subtypes
                    .contains(&PlaneswalkerSubtype::Jace)
        })
        .map(|o| o.object)
        .collect();
    assert_eq!(
        jace_tokens.len(),
        1,
        "first EmpowerJace must create one Jace token"
    );
    let jace_id = jace_tokens[0];
    assert_eq!(
        after_first
            .state()
            .execution()
            .counters_of_kind(jace_id, CounterKind::Loyalty),
        3.into(),
        "new Jace token must receive 3 loyalty counters"
    );

    let after_second = cast_untargeted(after_first.state(), 2);
    let jace_tokens_after: Vec<_> = after_second
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap()
        .objects
        .values()
        .filter(|o| {
            o.zone == ObjectZone::Battlefield
                && o.controller == Some(P0)
                && o.current_planeswalker_subtypes
                    .contains(&PlaneswalkerSubtype::Jace)
        })
        .map(|o| o.object)
        .collect();
    assert_eq!(
        jace_tokens_after.len(),
        1,
        "second EmpowerJace must reuse existing Jace token rather than creating a second"
    );
    assert_eq!(
        after_second
            .state()
            .execution()
            .counters_of_kind(jace_id, CounterKind::Loyalty),
        5.into(),
        "existing Jace token must grow from 3 to 5 loyalty counters"
    );
}

#[test]
fn turn_face_down_turns_nontoken_permanent_and_skips_double_faced() {
    let v = vocabulary();
    let single_faced = validated(vanilla(5, 5), &v);
    let mut dfc = vanilla(4, 4);
    dfc.modal_back = Some(Box::new(vanilla(6, 6)));
    let dfc = validated(dfc, &v);

    let turn_down_all = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::TurnFaceDownSubject {
                    subject: Box::new(EffectSubject::Matching(ObjectFilter::creature())),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, single_faced, true),
            card(2, dfc, true),
            card(3, turn_down_all, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let after = cast_untargeted(&game, 3);
    let eval = after
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    let c1 = eval.get(object(after.state(), 1)).unwrap();
    let c2 = eval.get(object(after.state(), 2)).unwrap();
    assert_eq!(
        c1.power_toughness.as_ref().map(|pt| pt.derived.clone()),
        Some(PowerToughness {
            power: (2).into(),
            toughness: (2).into()
        }),
        "single-faced nontoken creature must become a 2/2 face-down creature"
    );
    assert_eq!(
        c2.power_toughness.as_ref().map(|pt| pt.derived.clone()),
        Some(PowerToughness {
            power: (4).into(),
            toughness: (4).into()
        }),
        "double-faced permanent must NOT be turned face down (CR 712.16)"
    );
}

#[test]
fn add_one_of_each_counter_kind_adds_every_present_kind_and_skips_empty() {
    let v = vocabulary();
    let bear1 = validated(vanilla(2, 2), &v);
    let bear2 = validated(vanilla(2, 2), &v);
    let setup = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::TargetObject),
                        kind: CounterKind::PlusOne,
                        amount: ValueExpr::literal(1),
                    },
                    Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::TargetObject),
                        kind: CounterKind::Charge,
                        amount: ValueExpr::literal(2),
                    },
                ],
            ),
        ),
        &v,
    );
    let add_each = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::AddOneOfEachCounterKindOnSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, bear1, true),
            card(2, bear2, true),
            card(3, setup, false),
            card(4, add_each.clone(), false),
            card(5, add_each, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let s1 = cast_targeted(&game, 3, TargetRef::Object(object(&game, 1)));
    let s2 = cast_targeted(s1.state(), 4, TargetRef::Object(object(s1.state(), 1)));
    let id1 = object(s2.state(), 1);
    assert_eq!(
        s2.state()
            .execution()
            .counters_of_kind(id1, CounterKind::PlusOne),
        2.into(),
        "must add one +1/+1 counter"
    );
    assert_eq!(
        s2.state()
            .execution()
            .counters_of_kind(id1, CounterKind::Charge),
        3.into(),
        "must add one charge counter"
    );

    let s3 = cast_targeted(s2.state(), 5, TargetRef::Object(object(s2.state(), 2)));
    let id2 = object(s3.state(), 2);
    assert_eq!(
        s3.state()
            .execution()
            .counters_of_kind(id2, CounterKind::PlusOne),
        0.into(),
        "permanent with no counters must stay at 0 counters"
    );
}

#[test]
fn damage_recipients_each_source_to_itself_and_controller() {
    let v = vocabulary();
    let big = validated(vanilla(3, 5), &v);
    let opp_small = validated(vanilla(2, 2), &v);
    let hit_self = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::Matching(ObjectFilter::creature())),
                    recipients: Box::new(DamageRecipientsRef::EachSourceToItself),
                    amount: Box::new(DamageAmountSpec::SourcePower),
                }],
            ),
        ),
        &v,
    );
    let hit_ctrl = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::Matching(ObjectFilter::creature())),
                    recipients: Box::new(DamageRecipientsRef::EachSourceController),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(1))),
                }],
            ),
        ),
        &v,
    );
    let game = world(
        v,
        vec![
            card(1, big, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: opp_small,
                destination: ObjectDestination::Battlefield { controller: P1 },
            },
            card(3, hit_self, false),
            card(4, hit_ctrl, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let after_ctrl = cast_untargeted(&game, 4);
    assert_eq!(
        after_ctrl.state().execution().life_total(P0),
        Some(&19.into())
    );
    assert_eq!(
        after_ctrl.state().execution().life_total(P1),
        Some(&19.into())
    );

    let after_self = cast_untargeted(after_ctrl.state(), 3);
    assert_eq!(
        after_self
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_self.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        after_self
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_self.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1)
    );
}

#[test]
fn divide_damage_expr_and_up_to_x_target_count_enforce_bounds() {
    let v = vocabulary();
    let target_a = validated(vanilla(1, 5), &v);
    let target_b = validated(vanilla(1, 5), &v);
    let target_c = validated(vanilla(1, 5), &v);

    let mut divide_x = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::DivideDamageExprAmongTargets {
                total: ValueExpr::chosen_x(),
                max_targets: Some(2),
            }],
        ),
    );
    divide_x.x_mana_symbols = 1;
    let divide_x = validated(divide_x, &v);

    let mut up_to_x = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::AffectPermanentTargets {
                action: PermanentTargetAction::PutPlusOneCounter,
                minimum: 0,
                maximum: u16::MAX,
            }],
        )
        .with_up_to_x_target_count(),
    );
    up_to_x.x_mana_symbols = 1;
    let up_to_x = validated(up_to_x, &v);

    let mut mana_rock = Program::artifact(None, vec![]);
    mana_rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 4.into())])),
    });
    let mana_rock = validated(mana_rock, &v);

    let game = world(
        v,
        vec![
            card(1, target_a, true),
            card(2, target_b, true),
            card(3, target_c, true),
            card(4, divide_x, false),
            card(5, up_to_x, false),
            card(6, mana_rock, true),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(6)],
    );

    let tapped = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 6),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let id1 = object(tapped.state(), 1);
    let id2 = object(tapped.state(), 2);
    let id3 = object(tapped.state(), 3);

    // Negative: divide_x with 3 targets violates max_targets = Some(2)
    let bad_cap = attempt(
        tapped.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(tapped.state(), 4),
            target: None,
            payment: CastingPayment {
                mana: Some(ManaPayment {
                    from_lots: BTreeMap::new(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Colourless,
                        amount: 4.into(),
                    }],
                }),
                chosen_x: Some(4.into()),
                permanent_targets: vec![
                    TargetRef::Object(id1),
                    TargetRef::Object(id2),
                    TargetRef::Object(id3),
                ],
                target_division: vec![2, 1, 1],
                ..CastingPayment::default()
            },
        },
    );
    assert!(bad_cap.is_err(), "exceeding max_targets must be rejected");

    // Positive: divide_x X=4 divided [3, 1] across 2 targets succeeds
    let ok_div = submit(
        tapped.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(tapped.state(), 4),
            target: None,
            payment: CastingPayment {
                mana: Some(ManaPayment {
                    from_lots: BTreeMap::new(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Colourless,
                        amount: 4.into(),
                    }],
                }),
                chosen_x: Some(4.into()),
                permanent_targets: vec![TargetRef::Object(id1), TargetRef::Object(id2)],
                target_division: vec![3, 1],
                ..CastingPayment::default()
            },
        },
    );
    let after_div = pass_pair(ok_div.state());
    assert_eq!(
        after_div.state().execution().battlefield_marked_damage(id1),
        Some(3.into())
    );
    assert_eq!(
        after_div.state().execution().battlefield_marked_damage(id2),
        Some(1.into())
    );

    // Negative: up_to_x with X=0 and 1 target rejected
    let bad_upto = attempt(
        after_div.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_div.state(), 5),
            target: None,
            payment: CastingPayment {
                mana: Some(ManaPayment::default()),
                chosen_x: Some(0.into()),
                permanent_targets: vec![TargetRef::Object(id1)],
                ..CastingPayment::default()
            },
        },
    );
    assert!(bad_upto.is_err(), "more targets than X must be rejected");

    // Positive: up_to_x with X=0 and 0 targets succeeds
    let ok_upto = submit(
        after_div.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(after_div.state(), 5),
            target: None,
            payment: CastingPayment {
                mana: Some(ManaPayment::default()),
                chosen_x: Some(0.into()),
                permanent_targets: vec![],
                ..CastingPayment::default()
            },
        },
    );
    let _ = pass_pair(ok_upto.state());
}

#[test]
fn dynamic_spell_discount_opponents_hands_and_toughness_gt_power_selector() {
    let v = vocabulary();
    let mut discounter = Program::artifact(None, vec![]);
    discounter
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(1),
            modifier: SpellCostModifier::Discount {
                any_player: false,
                matching: SpellFilter::any(),
                amount: SpellDiscount::Dynamic(Box::new(ValueExpr::Count {
                    zone: ValueZone::OpponentsHands,
                    filter: Box::new(ObjectFilter::any()),
                    controller_scope: None,
                })),
            },
        });
    let mut butt_lord = Program::artifact(None, vec![]);
    let mut sel = PermanentSelector::default();
    sel.required_types.insert(CardType::Creature);
    sel.controller = ControllerSelector::SourceController;
    sel.toughness_greater_than_power = true;
    butt_lord
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(2),
            selector: sel,
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: spellwake_engine::characteristics::NumericExpression::Literal(2.into()),
                toughness: spellwake_engine::characteristics::NumericExpression::Literal(2.into()),
            })]),
        });

    let cost_2_spell = validated(
        Program::instant(
            ManaCost::generic(2.into()),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        &v,
    );
    let wall_1_4 = validated(vanilla(1, 4), &v);
    let bear_2_2 = validated(vanilla(2, 2), &v);
    let filler = validated(vanilla(1, 1), &v);
    let discounter = validated(discounter, &v);
    let butt_lord = validated(butt_lord, &v);

    let game = world(
        v,
        vec![
            card(1, discounter, true),
            card(2, butt_lord, true),
            card(3, wall_1_4, true),
            card(4, bear_2_2, true),
            card(5, cost_2_spell, false),
            SeedCard {
                card: CardId(6),
                owner: P1,
                program: filler.clone(),
                destination: ObjectDestination::Hand,
            },
            SeedCard {
                card: CardId(7),
                owner: P1,
                program: filler,
                destination: ObjectDestination::Hand,
            },
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );

    let eval = game.execution().evaluated_characteristics().unwrap();
    assert_eq!(
        eval.get(object(&game, 3))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        PowerToughness {
            power: (3).into(),
            toughness: (6).into()
        }
    );
    assert_eq!(
        eval.get(object(&game, 4))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        PowerToughness {
            power: (2).into(),
            toughness: (2).into()
        }
    );

    let after = cast_untargeted(&game, 5);
    assert_eq!(after.state().execution().life_total(P0), Some(&21.into()));
}

#[test]
fn controller_cant_lose_game_and_cant_cast_from_zone_work() {
    let v = vocabulary();
    let mut angel = vanilla(4, 4);
    angel
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Restriction {
            id: StaticAbilityId(1),
            rule: StaticRestriction::ControllerCantLoseGame,
        });
    let angel = validated(angel, &v);

    let lose_25 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Lose(25.into()),
                }],
            ),
        ),
        &v,
    );
    let lock_gy = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantCastFromZone {
                        players: AffectedPlayers::EachPlayer,
                        zone: SourceZoneFilter::AnyGraveyard,
                        filter: None,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &v,
    );
    let mut gy_flashback = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(2.into()),
            }],
        ),
    );
    gy_flashback.flashback = Some(ManaCost::free());
    let gy_flashback = validated(gy_flashback, &v);

    let hand_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, angel, true),
            card(2, lose_25, false),
            card(3, lock_gy, false),
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: gy_flashback,
                destination: ObjectDestination::Graveyard,
            },
            card(5, hand_spell, false),
        ],
        vec![CardId(1)],
    );

    let after_lose = cast_untargeted(&game, 2);
    assert!(
        after_lose.state().conclusion().is_none(),
        "ControllerCantLoseGame must prevent loss at -5 life"
    );
    assert_eq!(
        after_lose.state().execution().life_total(P0),
        Some(&(-5).into())
    );

    let after_lock = cast_untargeted(after_lose.state(), 3);
    let gy_attempt = attempt(
        after_lock.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_lock.state(), 4),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        gy_attempt.is_err(),
        "CantCastFromZone(AnyGraveyard) must prohibit graveyard flashback cast"
    );

    let after_hand = cast_untargeted(after_lock.state(), 5);
    assert_eq!(
        after_hand.state().execution().life_total(P0),
        Some(&(-4).into()),
        "CantCastFromZone(AnyGraveyard) must still allow casting from hand"
    );
}

#[test]
fn opponent_second_draw_trigger_and_reveal_hand_subtype_entry_tap_execute() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").expect("Elf in vocab");

    let mut watcher = vanilla(2, 2);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::PlayerAction {
            player: PlayerRelation::Opponent,
            action: PlayerTriggerAction::DrewCard {
                nth_each_turn: NonZeroU16::new(2),
            },
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            }],
        ),
    });
    let watcher = validated(watcher, &v);

    let reveal_land = validated(
        Program::new(
            BTreeSet::from([CardType::Land]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: Some(EntryTap::UnlessRevealHandSubtype {
                    subtype: elf,
                    or_control: true,
                }),
                attachment: None,
                printed_pt: None,
                casting_cost: None,
            }),
        ),
        &v,
    );
    let elf_creature = validated(
        Program::creature(
            Some(ManaCost::free()),
            1.into(),
            1.into(),
            BTreeSet::from([elf]),
        ),
        &v,
    );
    let opp_draws_2 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOpponent,
                vec![Instruction::DrawTargetPlayer {
                    amount: ValueExpr::literal(2),
                }],
            ),
        ),
        &v,
    );
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, watcher, true),
            card(2, elf_creature, true),
            card(3, reveal_land, false),
            SeedCard {
                card: CardId(4),
                owner: P1,
                program: filler.clone(),
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(5),
                owner: P1,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
            card(6, opp_draws_2, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    let played = submit(
        &game,
        P0,
        GameCommand::PlayLand {
            land: object(&game, 3),
        },
    );
    assert_eq!(
        played
            .state()
            .execution()
            .window()
            .cards()
            .get(object(played.state(), 3))
            .unwrap()
            .tapped(),
        Some(false),
        "UnlessRevealHandSubtype(or_control=true) must enter untapped when controlling an Elf"
    );

    // Force P1 (opponent) to draw 2 cards -> opponent second-draw trigger fires and P0 gains 3 life!
    let after_draw = cast_targeted(played.state(), 6, TargetRef::Player(P1));
    let after_trig = pass_pair(after_draw.state());
    assert_eq!(
        after_trig.state().execution().life_total(P0),
        Some(&23.into()),
        "Opponent second-draw trigger must fire when P1 draws their 2nd card"
    );
}

#[test]
fn unattach_subject_detaches_equipment_from_host_creature() {
    use spellwake_engine::{attachment::AttachmentKind, program::MoveSubjectRef};
    let v = vocabulary();
    let bear = validated(vanilla(2, 2), &v);
    let mut sword = Program::new(
        BTreeSet::from([CardType::Artifact]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: Some(AttachmentKind::Equipment),
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    let sel = PermanentSelector {
        attached_to_source: true,
        ..PermanentSelector::default()
    };
    sword
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: sel,
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: spellwake_engine::characteristics::NumericExpression::Literal(2.into()),
                toughness: spellwake_engine::characteristics::NumericExpression::Literal(2.into()),
            })]),
        });
    let sword = validated(sword, &v);

    let mut equip_sword = sword;
    equip_sword
        .activated
        .push(spellwake_engine::activated::ActivatedAbility {
            id: spellwake_engine::activated::ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: spellwake_engine::activated::SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::AttachPermanent {
                    attachment: MoveSubjectRef::AbilitySource,
                    host: MoveSubjectRef::TargetObject,
                }],
            ),
        });
    let equip_sword = validated(equip_sword, &v);

    let unattach_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::UnattachSubject {
                    subject: MoveSubjectRef::TargetObject,
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, bear, true),
            card(2, equip_sword, true),
            card(3, unattach_spell, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let act = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: object(&game, 2),
            ability: spellwake_engine::activated::ActivatedAbilityId(1),
            target: TargetRef::Object(object(&game, 1)),
            payment: Some(ManaPayment::default()),
        },
    );
    let equipped = pass_pair(act.state());
    assert_eq!(
        equipped
            .state()
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .get(object(equipped.state(), 1))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        PowerToughness {
            power: 4.into(),
            toughness: 4.into()
        },
        "equipped bear must be 4/4 before UnattachSubject"
    );

    let detached = cast_targeted(
        equipped.state(),
        3,
        TargetRef::Object(object(equipped.state(), 2)),
    );
    assert_eq!(
        detached
            .state()
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .get(object(detached.state(), 1))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        },
        "UnattachSubject must detach the Equipment so bear returns to 2/2"
    );
}

#[test]
fn land_played_filtered_and_drew_card_except_first_in_draw_step_execute() {
    use spellwake_engine::program::{ObjectClause, Supertype, SupertypeMask};
    let v = vocabulary();

    // Watcher 1: Whenever you play a legendary land, gain 4 life
    let mut leg_land_clause = ObjectClause::land();
    leg_land_clause.required_supertypes = SupertypeMask::LEGENDARY;
    let mut land_watcher = vanilla(2, 2);
    land_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::PlayerAction {
            player: PlayerRelation::Controller,
            action: PlayerTriggerAction::LandPlayedFiltered(ObjectFilter::Clause(leg_land_clause)),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(4.into()),
            }],
        ),
    });
    // Watcher 2: Whenever an opponent draws a card except first in draw step, gain 2 life
    land_watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(2),
        condition: TriggerSpec::new(TriggerEvent::PlayerAction {
            player: PlayerRelation::Opponent,
            action: PlayerTriggerAction::DrewCardExceptFirstInDrawStep,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(2.into()),
            }],
        ),
    });
    let land_watcher = validated(land_watcher, &v);

    let mut leg_land = Program::new(
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
    leg_land.supertypes.insert(Supertype::Legendary);
    let leg_land = validated(leg_land, &v);

    let opp_draw_1 = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleOpponent,
                vec![Instruction::DrawTargetPlayer {
                    amount: ValueExpr::literal(1),
                }],
            ),
        ),
        &v,
    );
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, land_watcher, true),
            card(2, leg_land, false),
            card(3, opp_draw_1, false),
            SeedCard {
                card: CardId(4),
                owner: P1,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![CardId(1)],
    );

    // Play legendary land -> LandPlayedFiltered triggers and P0 gains 4 life (20 -> 24)
    let played = submit(
        &game,
        P0,
        GameCommand::PlayLand {
            land: object(&game, 2),
        },
    );
    let after_land_trig = pass_pair(played.state());
    assert_eq!(
        after_land_trig.state().execution().life_total(P0),
        Some(&24.into()),
        "LandPlayedFiltered must trigger for legendary land"
    );

    // Force P1 to draw during P0's main phase (NOT P1's draw step) -> DrewCardExceptFirstInDrawStep triggers (+2 life -> 26)
    let after_draw = cast_targeted(after_land_trig.state(), 3, TargetRef::Player(P1));
    let after_draw_trig = pass_pair(after_draw.state());
    assert_eq!(
        after_draw_trig.state().execution().life_total(P0),
        Some(&26.into()),
        "DrewCardExceptFirstInDrawStep must trigger on opponent draw outside their draw step"
    );
}

// =========================================================================
// Wave 4 Engine Primitives Behavioral Tests
// =========================================================================

fn zone_of(state: &GameState, card_id: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, card_id))
        .expect("card object must exist")
        .zone()
}

#[test]
fn grant_last_moved_play_permission_filters_and_allows_free_cast_from_exile() {
    let v = vocabulary();
    let spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ExileLibraryTop {
                        player: ResolvingPlayer::Controller,
                        amount: 2,
                    },
                    Instruction::GrantLastMovedPlayPermission {
                        player: ResolvingPlayer::Controller,
                        filter: Some(Box::new(ObjectFilter::creature())),
                        duration: ResolvedDuration::UntilEndOfTurn,
                        free: true,
                        cast_only: true,
                        any_color: false,
                    },
                ],
            ),
        ),
        &v,
    );
    let expensive_creature = validated(
        Program::creature(
            Some(ManaCost::generic(5.into())),
            4.into(),
            4.into(),
            BTreeSet::new(),
        ),
        &v,
    );
    let plain_land = validated(
        Program::new(
            [CardType::Land].into(),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: None,
                attachment: None,
                printed_pt: None,
                casting_cost: None,
            }),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, spell, false),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: plain_land,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: expensive_creature,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );

    let after = cast_untargeted(&game, 1);
    assert_eq!(zone_of(after.state(), 2), ObjectZone::Exile);
    assert_eq!(zone_of(after.state(), 3), ObjectZone::Exile);

    // Negative: land must NOT be playable from exile (filtered out and cast_only: true)
    assert!(
        attempt(
            after.state(),
            P0,
            GameCommand::PlayLand {
                land: object(after.state(), 2),
            }
        )
        .is_err(),
        "GrantLastMovedPlayPermission with creature filter and cast_only must not allow playing exiled land"
    );

    // Positive: expensive creature CAN be cast from exile for FREE (zero mana in pool)
    let cast_free = submit(
        after.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(cast_free.state());
    assert_eq!(
        zone_of(resolved.state(), 3),
        ObjectZone::Battlefield,
        "GrantLastMovedPlayPermission(free: true) must allow casting matching exiled creature for free"
    );
}

#[test]
fn move_last_moved_filtered_moves_only_matching_objects() {
    let v = vocabulary();
    let spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ExileLibraryTop {
                        player: ResolvingPlayer::Controller,
                        amount: 2,
                    },
                    Instruction::MoveLastMovedFiltered {
                        filter: Some(Box::new(ObjectFilter::creature())),
                        destination: SubjectDestination::Hand,
                    },
                ],
            ),
        ),
        &v,
    );
    let bear = validated(vanilla(2, 2), &v);
    let bolt = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![
            card(1, spell, false),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: bolt,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P0,
                program: bear,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );

    let after = cast_untargeted(&game, 1);
    assert_eq!(
        zone_of(after.state(), 3),
        ObjectZone::Hand(P0),
        "MoveLastMovedFiltered must move matching creature to hand"
    );
    assert_eq!(
        zone_of(after.state(), 2),
        ObjectZone::Exile,
        "MoveLastMovedFiltered must leave non-matching instant in exile"
    );
}

#[test]
fn one_shot_lose_game_replacement_intercepts_once_then_consumes() {
    let v = vocabulary();
    let reversal = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::OneShotEventReplacement(Box::new(
                        StaticContinuousAbility::EventReplacement {
                            id: StaticAbilityId(1),
                            event: ReplaceableEvent::LoseGame {
                                player: AffectedPlayers::One(ResolvingPlayer::Controller),
                            },
                            condition: None,
                            action: StaticReplacementAction::ReplaceWithBody {
                                optional: false,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![
                                        Instruction::DrawController {
                                            amount: ValueExpr::literal(1),
                                        },
                                        Instruction::AdjustControllerLife {
                                            adjustment: LifeAdjustment::Set(1.into()),
                                        },
                                    ],
                                )),
                            },
                        },
                    ))),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &v,
    );
    let kill_self = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::LoseGame(AffectedPlayers::One(
                    ResolvingPlayer::Controller,
                ))],
            ),
        ),
        &v,
    );
    let draw_card = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, reversal, false),
            card(2, kill_self.clone(), false),
            card(3, kill_self, false),
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: draw_card,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );

    let armed = cast_untargeted(&game, 1);
    let survived = cast_untargeted(armed.state(), 2);
    assert_eq!(
        survived.state().execution().life_total(P0),
        Some(&1.into()),
        "OneShotEventReplacement must replace LoseGame and set life to 1"
    );
    assert_eq!(
        zone_of(survived.state(), 4),
        ObjectZone::Hand(P0),
        "OneShotEventReplacement inline body must draw 1 card"
    );
    assert!(
        survived.state().conclusion().is_none(),
        "First LoseGame must be prevented by OneShotEventReplacement"
    );

    let lost = cast_untargeted(survived.state(), 3);
    assert!(
        lost.state().conclusion().is_some(),
        "Consumed OneShotEventReplacement must not prevent a second LoseGame"
    );
}

#[test]
fn static_sba_lose_game_replacement_exiles_source_and_restores_life() {
    let v = vocabulary();
    let mut angel = Program::creature(Some(ManaCost::free()), 5.into(), 5.into(), BTreeSet::new());
    angel.continuous = Some(StaticContinuousAbility::EventReplacement {
        id: StaticAbilityId(1),
        event: ReplaceableEvent::LoseGame {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        condition: None,
        action: StaticReplacementAction::ReplaceWithBody {
            optional: false,
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::MoveSubject {
                        subject: MoveSubjectRef::AbilitySource,
                        destination: SubjectDestination::Exile {
                            delayed_return: None,
                        },
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Set(20.into()),
                    },
                ],
            )),
        },
    });
    let angel = validated(angel, &v);

    let self_lethal = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Lose(25.into()),
                }],
            ),
        ),
        &v,
    );

    let game = world(
        v,
        vec![card(1, angel, true), card(2, self_lethal, false)],
        vec![CardId(1)],
    );

    let after = cast_untargeted(&game, 2);
    assert!(
        after.state().conclusion().is_none(),
        "Static LoseGame replacement must intercept SBA nonpositive-life loss"
    );
    assert_eq!(
        after.state().execution().life_total(P0),
        Some(&20.into()),
        "Static LoseGame replacement must restore life total to 20"
    );
    assert_eq!(
        zone_of(after.state(), 1),
        ObjectZone::Exile,
        "Static LoseGame replacement must exile its own source permanent"
    );
}

#[test]
fn look_top_exile_free_and_ability_activated_counter_trigger_validate() {
    let v = vocabulary();
    let mut stifle_perm = vanilla(2, 2);
    stifle_perm.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::AbilityActivated {
            player: PlayerRelation::Opponent,
            source_filter: ObjectFilter::any(),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CounterTriggeringStackObject],
        ),
    });
    stifle_perm
        .validate(&v)
        .expect("CounterTriggeringStackObject must validate on AbilityActivated triggers");

    let spell = validated(
        Program::sorcery(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::LookTopChooseExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    count: ValueExpr::literal(1),
                    filter: Some(Box::new(ObjectFilter::creature())),
                    max_picks: 1,
                    take_all_matching: true,
                    keep_dest: LookTopKeepDest::Exile {
                        playable: true,
                        free: true,
                        cast_only: true,
                        any_color: false,
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                    rest_dest: LookTopRestDest::BottomRandom,
                    reveal_kept: true,
                }],
            ),
        ),
        &v,
    );
    let expensive_creature = validated(
        Program::creature(
            Some(ManaCost::generic(6.into())),
            6.into(),
            6.into(),
            BTreeSet::new(),
        ),
        &v,
    );
    let game = world(
        v,
        vec![
            card(1, spell, false),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: expensive_creature,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );
    let after = cast_untargeted(&game, 1);
    assert_eq!(zone_of(after.state(), 2), ObjectZone::Exile);
    let cast_free = submit(
        after.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved = pass_pair(cast_free.state());
    assert_eq!(
        zone_of(resolved.state(), 2),
        ObjectZone::Battlefield,
        "LookTopKeepDest::Exile(free: true) must allow casting exiled creature for free"
    );
}
