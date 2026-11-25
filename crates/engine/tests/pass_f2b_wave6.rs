#![forbid(unsafe_code)]
//! Runtime integration tests for Pass F2-B Wave 6 primitives:
//! - Partial `RedirectNextDamage` (split + consume without looping, CR 614.5/614.9) and
//!   dynamic `RedirectNextDamageExpr` (creature power and `ChosenX`)
//! - Targeted damage prevention and redirection: `PreventNextTimeDamage { source_is_target: true }`,
//!   `StaticDamageRecipient::{TargetObject, TargetAny}`, and
//!   `DamageRedirectTarget::{TargetAny, SecondTargetObject}`
//! - Generalized `CreateTokenCopy`: `grant_keywords`, `add_subtypes`, `add_card_types`,
//!   and `TokenRecipients::EachOpponent`
//! - `SpellModes.conditional_maximum`
//! - `Instruction::BindResolutionX` feeding downstream `ValueExpr::chosen_x()` and
//!   `Instruction::EmitReflexiveTrigger`
//! - `Program.conditional_flash` (`SelfFlashPermission::Condition` and `PayExtraMana`)
//! - `Program.cannot_be_copied` blocking `Instruction::CopyStackObject`
//! - `TemporaryRuleKind::TemporaryZoneReplacement` with cleanup expiration
//! - `AbilityChange.suppressed_activations` (`NonMana` vs `All`) and `AbilityChange.doesnt_untap`
//! - `TemporaryRuleKind::{CantActivateAbilities, CantPlayLands, PlayerGrantKeyword}`
//! - `next_only: true` on `CantBeCountered`, `FilteredFlash`, and `NextSpellFree`
//! - `StaticRestriction::{ControllerLifeTotalCantChange, PlayersCantSearchLibraries}`
//! - `Instruction::SetDayNight`, `CombatActionKind::BecomeBlocked`,
//!   `BlockingRestriction::MustBlockAllAttackers`, `Instruction::PutResolvingSpellInLibrary`,
//!   `Instruction::PutSubjectIntoLibraryFromTop`, `SearchCount::UpToExpr`,
//!   `ValueObjectRef::LastAmassedArmy`, `EventValueKind::AdditionalCostPaid`,
//!   `ExtraActivationCost::PayEnergyExpr(ChosenX)`, and
//!   `HandDiscardKeyword::Channel` with `PermanentTargets`.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    actions::TimingError,
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationOrigin, ExtraActivationCost,
        HandDiscardKeyword, LifeCostAmount, SourceCost,
    },
    characteristics::{
        AbilityChange, ActivationSuppression, BlockingRestriction, Color, ControllerSelector,
        DamageRedirectTarget, KeywordAbility, PermanentSelector, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility,
        StaticDamageRecipient, StaticPart, StaticParts, StaticRestriction,
    },
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus, CombatDecision, CombatError},
    day_night::DayNight,
    effects::{CounterKind, EffectError, LibraryTopReply, SeedCard},
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    land::LandType,
    mana::{
        CastMode, CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput,
        ManaPayment, ManaSpend, ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        AdditionalCastCost, AffectedPlayers, BodyTarget, CardType, CombatActionKind, Comparison,
        DamageAmountSpec, DamageRecipientsRef, DamageSourceRef, EffectSubject, EventValueKind,
        ExecutableBody, Instruction, LibraryPlacement, LibrarySearch, ModalUniqueness,
        MoveSubjectRef, ObjectFilter, ObjectStatKind, PermanentTargetAction, PlayerRelation,
        Program, ProgramKind, PublicZoneDestination, ResolutionCondition, ResolvingPlayer,
        SearchCardMatcher, SearchCount, SearchDestination, SearchSource, SelfFlashPermission,
        SpellFilter, SpellModes, StackCopyTarget, TemporaryRuleKind, ValueExpr, ValueObjectRef,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
    targets::TargetRef,
    token::TokenRecipients,
    turn::{Step, TurnError},
};
use std::collections::{BTreeMap, BTreeSet};

fn validated(program: Program, vocab: &CreatureTypeVocabulary) -> Program {
    program
        .validate(vocab)
        .expect("constructed Wave 6 test program must validate");
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

fn library_top(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::LibraryTop,
        program,
    }
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

fn colorless_rock(amount: i64) -> Program {
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, amount.into())])),
    });
    rock
}

fn colorless_payment(amount: i64) -> ManaPayment {
    ManaPayment {
        from_lots: BTreeMap::new(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Colourless,
            amount: amount.into(),
        }],
    }
}

fn basic_forest() -> Program {
    Program::land(BTreeSet::from([LandType::Forest]), vec![])
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

#[test]
fn partial_redirect_next_damage_splits_consumes_and_does_not_loop() {
    let vocab = vocabulary();

    let sponge = validated(
        Program::creature(None, 2.into(), 6.into(), BTreeSet::new()),
        &vocab,
    );
    let redirect_two = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                        source: None,
                        source_is_target: false,
                        from: StaticDamageRecipient::Controller,
                        to: DamageRedirectTarget::TargetObject,
                        combat_only: false,
                        amount: Some(2.into()),
                        one_shot: false,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let bolt_five = validated(damage_any_instant(5), &vocab);
    let bolt_three = validated(damage_any_instant(3), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, sponge, true),
            card(2, redirect_two, false),
            card(3, bolt_five, false),
            card(4, bolt_three, false),
        ],
        vec![CardId(1)],
    );
    let sponge_id = object(&game, 1);

    // Install partial shield: redirect the next 2 damage from P0 to sponge.
    let shielded = cast_targeted(&game, P0, 2, TargetRef::Object(sponge_id));

    // Deal 5 damage to P0: 2 redirected to sponge, 3 remain on P0, shield fully consumed without looping.
    let after_five = cast_targeted(shielded.state(), P0, 3, TargetRef::Player(P0));
    assert_eq!(
        after_five.state().execution().life_total(P0),
        Some(&17.into()),
        "controller should take the unredirected 3 of 5 damage"
    );
    assert_eq!(
        after_five
            .state()
            .execution()
            .creature(sponge_id)
            .unwrap()
            .unwrap()
            .damage,
        2.into(),
        "target creature should receive the redirected 2 damage"
    );

    // Subsequent 3 damage to P0 must not be redirected because the 2-point shield was consumed.
    let after_three = cast_targeted(after_five.state(), P0, 4, TargetRef::Player(P0));
    assert_eq!(
        after_three.state().execution().life_total(P0),
        Some(&14.into()),
        "consumed partial redirect shield must not apply to later damage"
    );
    assert_eq!(
        after_three
            .state()
            .execution()
            .creature(sponge_id)
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
}

#[test]
fn redirect_next_damage_expr_evaluates_creature_power_and_chosen_x() {
    let vocab = vocabulary();

    // Case A: dynamic amount from target creature's power (3/6 -> redirects 3 of 5).
    let guardian = validated(
        Program::creature(None, 3.into(), 6.into(), BTreeSet::new()),
        &vocab,
    );
    let redirect_by_power = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::RedirectNextDamageExpr {
                        source: None,
                        source_is_target: false,
                        from: StaticDamageRecipient::Controller,
                        to: DamageRedirectTarget::TargetObject,
                        combat_only: false,
                        amount: Box::new(ValueExpr::object_stat(
                            ValueObjectRef::TargetObject,
                            ObjectStatKind::Power,
                        )),
                        one_shot: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let bolt_five = validated(damage_any_instant(5), &vocab);

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, guardian, true),
            card(2, redirect_by_power, false),
            card(3, bolt_five.clone(), false),
        ],
        vec![CardId(1)],
    );
    let guardian_id = object(&game_a, 1);

    let shielded_a = cast_targeted(&game_a, P0, 2, TargetRef::Object(guardian_id));
    let after_a = cast_targeted(shielded_a.state(), P0, 3, TargetRef::Player(P0));
    assert_eq!(after_a.state().execution().life_total(P0), Some(&18.into()));
    assert_eq!(
        after_a
            .state()
            .execution()
            .creature(guardian_id)
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );

    // Case B: dynamic amount from announced ChosenX (X = 4 -> redirects 4 of 5).
    let wall = validated(
        Program::creature(None, 0.into(), 6.into(), BTreeSet::new()),
        &vocab,
    );
    let mut redirect_by_x = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::RedirectNextDamageExpr {
                    source: None,
                    source_is_target: false,
                    from: StaticDamageRecipient::Controller,
                    to: DamageRedirectTarget::TargetObject,
                    combat_only: false,
                    amount: Box::new(ValueExpr::chosen_x()),
                    one_shot: true,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            }],
        ),
    );
    redirect_by_x.x_mana_symbols = 1;
    let redirect_by_x = validated(redirect_by_x, &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, wall, true),
            card(2, validated(colorless_rock(4), &vocab), true),
            card(3, redirect_by_x, false),
            card(4, bolt_five, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let wall_id = object(&game_b, 1);

    let funded = submit(
        &game_b,
        P0,
        GameCommand::ActivateMana {
            source: object(&game_b, 2),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_x = submit(
        funded.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(funded.state(), 3),
            target: Some(TargetRef::Object(wall_id)),
            payment: CastingPayment {
                chosen_x: Some(4.into()),
                mana: Some(colorless_payment(4)),
                ..CastingPayment::default()
            },
        },
    );
    let shielded_b = pass_pair(cast_x.state());
    let after_b = cast_targeted(shielded_b.state(), P0, 4, TargetRef::Player(P0));
    assert_eq!(after_b.state().execution().life_total(P0), Some(&19.into()));
    assert_eq!(
        after_b
            .state()
            .execution()
            .creature(wall_id)
            .unwrap()
            .unwrap()
            .damage,
        4.into()
    );
}

#[test]
fn prevent_next_time_damage_source_is_target_filters_by_targeted_source() {
    let vocab = vocabulary();

    let pinger = || {
        let mut creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
        creature.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::DealDamageFrom {
                    source: Box::new(DamageSourceRef::AbilitySource),
                    recipients: Box::new(DamageRecipientsRef::Target),
                    amount: Box::new(DamageAmountSpec::Expr(ValueExpr::literal(3))),
                }],
            ),
        });
        validated(creature, &vocab)
    };

    let prevent_from_target = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                        source: None,
                        source_chosen_color: false,
                        source_is_target: true,
                        recipient: StaticDamageRecipient::Controller,
                        combat_only: false,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            opp_battlefield(1, pinger()),
            opp_battlefield(2, pinger()),
            card(3, prevent_from_target, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let pinger_a = object(&game, 1);
    let pinger_b = object(&game, 2);

    // Prevent the next time pinger_a would deal damage to P0.
    let shielded = cast_targeted(&game, P0, 3, TargetRef::Object(pinger_a));

    // Pass priority to P1; pinger_b is NOT the targeted source so its 3 damage gets through.
    let p1_prio = submit(shielded.state(), P0, GameCommand::Pass);
    let act_b = submit(
        p1_prio.state(),
        P1,
        GameCommand::ActivateAbility {
            source: pinger_b,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P0),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_b = pass_in_order(act_b.state(), [P1, P0]);
    assert_eq!(after_b.state().execution().life_total(P0), Some(&17.into()));

    // First ping from pinger_a is prevented and consumes the one-shot shield.
    let p1_prio2 = submit(after_b.state(), P0, GameCommand::Pass);
    let act_a1 = submit(
        p1_prio2.state(),
        P1,
        GameCommand::ActivateAbility {
            source: pinger_a,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P0),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_a1 = pass_in_order(act_a1.state(), [P1, P0]);
    assert_eq!(
        after_a1.state().execution().life_total(P0),
        Some(&17.into()),
        "next damage from targeted source must be prevented"
    );

    // Second ping from pinger_a deals full damage because the shield was consumed.
    let p1_prio3 = submit(after_a1.state(), P0, GameCommand::Pass);
    let act_a2 = submit(
        p1_prio3.state(),
        P1,
        GameCommand::ActivateAbility {
            source: pinger_a,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P0),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_a2 = pass_in_order(act_a2.state(), [P1, P0]);
    assert_eq!(
        after_a2.state().execution().life_total(P0),
        Some(&14.into()),
        "second damage instance from targeted source must not be prevented"
    );
}

#[test]
fn targeted_damage_prevention_and_redirection_recipients_and_second_target() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 5.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let prevent_obj = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PreventNextDamage {
                        amount: 2.into(),
                        source: None,
                        recipient: StaticDamageRecipient::TargetObject,
                        combat_only: false,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let prevent_any = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleAnyTarget,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                        source: None,
                        source_chosen_color: false,
                        source_is_target: false,
                        recipient: StaticDamageRecipient::TargetAny,
                        combat_only: false,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let redirect_to_any = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleAnyTarget,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                        source: None,
                        source_is_target: false,
                        from: StaticDamageRecipient::Controller,
                        to: DamageRedirectTarget::TargetAny,
                        combat_only: false,
                        amount: None,
                        one_shot: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let redirect_second = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                                source: None,
                                source_is_target: false,
                                from: StaticDamageRecipient::TargetObject,
                                to: DamageRedirectTarget::SecondTargetObject,
                                combat_only: false,
                                amount: None,
                                one_shot: true,
                            }),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        }],
                    ))),
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt(0, 0))]),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        }],
                    ))),
                ],
            ),
        ),
        &vocab,
    );
    let bolt_four = validated(damage_any_instant(4), &vocab);
    let bolt_three = validated(damage_any_instant(3), &vocab);

    // Part A: StaticDamageRecipient::TargetObject and TargetAny.
    let game_a = world(
        vocab.clone(),
        vec![
            card(1, bear(), true),
            card(2, bear(), true),
            card(3, prevent_obj, false),
            card(4, bolt_four.clone(), false),
            card(5, bolt_four, false),
            card(6, prevent_any.clone(), false),
            card(7, bolt_three.clone(), false),
            card(8, prevent_any, false),
            card(9, bolt_three.clone(), false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let bear_a = object(&game_a, 1);
    let bear_b = object(&game_a, 2);

    let s1 = cast_targeted(&game_a, P0, 3, TargetRef::Object(bear_a));
    let s2 = cast_targeted(s1.state(), P0, 4, TargetRef::Object(bear_a));
    assert_eq!(
        s2.state()
            .execution()
            .creature(bear_a)
            .unwrap()
            .unwrap()
            .damage,
        2.into(),
        "TargetObject shield must prevent 2 of 4 damage to bear_a"
    );
    let s3 = cast_targeted(s2.state(), P0, 5, TargetRef::Object(bear_b));
    assert_eq!(
        s3.state()
            .execution()
            .creature(bear_b)
            .unwrap()
            .unwrap()
            .damage,
        4.into(),
        "TargetObject shield on bear_a must not protect bear_b"
    );

    // TargetAny protecting a player, then TargetAny protecting a creature.
    let s4 = cast_targeted(s3.state(), P0, 6, TargetRef::Player(P0));
    let s5 = cast_targeted(s4.state(), P0, 7, TargetRef::Player(P0));
    assert_eq!(
        s5.state().execution().life_total(P0),
        Some(&20.into()),
        "TargetAny shield on P0 must prevent damage to P0"
    );
    let s6 = cast_targeted(s5.state(), P0, 8, TargetRef::Object(bear_a));
    let s7 = cast_targeted(s6.state(), P0, 9, TargetRef::Object(bear_a));
    assert_eq!(
        s7.state()
            .execution()
            .creature(bear_a)
            .unwrap()
            .unwrap()
            .damage,
        2.into(),
        "TargetAny shield on bear_a must prevent additional damage to bear_a"
    );

    // Part B: DamageRedirectTarget::TargetAny (to opponent) and SecondTargetObject (creature -> creature).
    let game_b = world(
        vocab.clone(),
        vec![
            card(1, bear(), true),
            card(2, bear(), true),
            card(3, redirect_to_any, false),
            card(4, bolt_three.clone(), false),
            card(5, redirect_second, false),
            card(6, bolt_three, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let c1 = object(&game_b, 1);
    let c2 = object(&game_b, 2);

    let r1 = cast_targeted(&game_b, P0, 3, TargetRef::Player(P1));
    let r2 = cast_targeted(r1.state(), P0, 4, TargetRef::Player(P0));
    assert_eq!(r2.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        r2.state().execution().life_total(P1),
        Some(&17.into()),
        "DamageRedirectTarget::TargetAny must redirect damage from P0 to targeted player P1"
    );

    let cast_two = submit(
        r2.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(r2.state(), 5),
            target: None,
            payment: CastingPayment {
                fixed_targets: vec![TargetRef::Object(c1), TargetRef::Object(c2)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let r3 = pass_pair(cast_two.state());
    let r4 = cast_targeted(r3.state(), P0, 6, TargetRef::Object(c1));
    assert_eq!(
        r4.state().execution().creature(c1).unwrap().unwrap().damage,
        0.into(),
        "damage to first target creature must be redirected away"
    );
    assert_eq!(
        r4.state().execution().creature(c2).unwrap().unwrap().damage,
        3.into(),
        "DamageRedirectTarget::SecondTargetObject must deliver redirected damage to second target"
    );
}

#[test]
fn create_token_copy_grants_keywords_subtypes_card_types_and_each_opponent() {
    let vocab = vocabulary();
    let elf = vocab.resolve_label("Elf").unwrap();
    let zombie = vocab.resolve_label("Zombie").unwrap();
    let wizard = vocab.resolve_label("Wizard").unwrap();

    let original = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::from([elf])),
        &vocab,
    );
    let clone_for_opponents = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::CreateTokenCopy {
                    source: MoveSubjectRef::TargetObject,
                    count: ValueExpr::literal(1),
                    tapped: false,
                    attacking: false,
                    sacrifice_at_end_step: false,
                    exile_at_end_step: false,
                    grant_keywords: BTreeSet::from([KeywordAbility::Flying, KeywordAbility::Haste]),
                    nonlegendary: true,
                    override_pt: None,
                    override_colors: None,
                    add_subtypes: BTreeSet::from([zombie, wizard]),
                    add_card_types: BTreeSet::from([CardType::Artifact]),
                    strip_mana_cost: false,
                    recipients: TokenRecipients::EachOpponent,
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![card(1, original, true), card(2, clone_for_opponents, false)],
        vec![CardId(1)],
    );
    let orig_id = object(&game, 1);

    let resolved = cast_targeted(&game, P0, 2, TargetRef::Object(orig_id));
    let tokens: Vec<_> = resolved
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|obj| obj.card_id().is_none() && obj.zone() == ObjectZone::Battlefield)
        .collect();
    assert_eq!(
        tokens.len(),
        1,
        "EachOpponent in 2-player game must create exactly 1 token copy"
    );
    let token_id = tokens[0].id();
    assert_eq!(
        tokens[0].controller(),
        Some(P1),
        "TokenRecipients::EachOpponent must create the token copy under P1's control"
    );

    let view = current(resolved.state(), token_id);
    assert_eq!(
        view.current_types,
        BTreeSet::from([CardType::Creature, CardType::Artifact])
    );
    assert_eq!(
        view.current_creature_subtypes,
        BTreeSet::from([elf, zombie, wizard])
    );
    assert!(view.keywords.contains(&KeywordAbility::Flying));
    assert!(view.keywords.contains(&KeywordAbility::Haste));
    assert_eq!(view.power_toughness.unwrap().derived, pt(2, 2));
}

#[test]
fn spell_modes_conditional_maximum_enforces_base_and_elevated_bounds() {
    let vocab = vocabulary();

    let mode_0 = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(2),
        }],
    );
    let mode_1 = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustPlayersLife {
            players: AffectedPlayers::EachOpponent,
            gain: false,
            amount: ValueExpr::literal(3),
        }],
    );
    let mut modal_spell = Program::instant(ManaCost::free(), mode_0);
    if let ProgramKind::Spell(def) = &mut modal_spell.kind {
        def.modes = Some(SpellModes {
            minimum: 1,
            maximum: 1,
            conditional_maximum: Some((
                Box::new(ResolutionCondition {
                    left: ValueExpr::controlled_type(CardType::Artifact),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                2,
            )),
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: ModalUniqueness::None,
            mode_costs: vec![],
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: vec![mode_1],
        });
    }
    let modal_spell = validated(modal_spell, &vocab);
    let relic = validated(Program::artifact(Some(ManaCost::free()), vec![]), &vocab);

    let game = world(
        vocab,
        vec![
            card(1, modal_spell.clone(), false),
            card(2, modal_spell, false),
            card(3, relic, false),
        ],
        vec![],
    );

    let two_modes = vec![
        CastMode {
            mode: 0,
            target: None,
            targets: vec![],
        },
        CastMode {
            mode: 1,
            target: None,
            targets: vec![],
        },
    ];

    // Condition false (0 artifacts): choosing 2 modes is rejected.
    assert_eq!(
        attempt(
            &game,
            P0,
            GameCommand::CastWithResources {
                spell: object(&game, 1),
                target: None,
                payment: CastingPayment {
                    modes: two_modes.clone(),
                    mana: Some(ManaPayment::default()),
                    ..CastingPayment::default()
                },
            },
        ),
        Err(GameError::Effect(EffectError::InvalidCastResource))
    );

    // Condition false: choosing 1 mode succeeds and resolves mode 0 only.
    let cast_one = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 1),
            target: None,
            payment: CastingPayment {
                modes: vec![CastMode {
                    mode: 0,
                    target: None,
                    targets: vec![],
                }],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_one = pass_pair(cast_one.state());
    assert_eq!(
        after_one.state().execution().life_total(P0),
        Some(&22.into())
    );
    assert_eq!(
        after_one.state().execution().life_total(P1),
        Some(&20.into())
    );

    // Cast an artifact so the condition becomes true; choosing both modes now succeeds and resolves both.
    let with_artifact = cast_untargeted(after_one.state(), 3);
    let cast_both = submit(
        with_artifact.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(with_artifact.state(), 2),
            target: None,
            payment: CastingPayment {
                modes: two_modes,
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_both = pass_pair(cast_both.state());
    assert_eq!(
        after_both.state().execution().life_total(P0),
        Some(&24.into()),
        "mode 0 must resolve when conditional_maximum permits both modes"
    );
    assert_eq!(
        after_both.state().execution().life_total(P1),
        Some(&17.into()),
        "mode 1 must resolve when conditional_maximum permits both modes"
    );
}

#[test]
fn bind_resolution_x_feeds_downstream_chosen_x_and_reflexive_trigger() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let bind_and_reflect = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::BindResolutionX(ValueExpr::controlled_type(CardType::Creature)),
                    Instruction::GainControllerLife {
                        amount: ValueExpr::chosen_x(),
                    },
                    Instruction::EmitReflexiveTrigger {
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::AdjustPlayersLife {
                                players: AffectedPlayers::EachOpponent,
                                gain: false,
                                amount: ValueExpr::chosen_x(),
                            }],
                        )),
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
            card(3, bear(), true),
            card(4, bind_and_reflect, false),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );

    // Resolving the spell binds X = 3, gains 3 life, and queues the reflexive trigger with chosen_x = 3.
    let after_spell = cast_untargeted(&game, 4);
    assert_eq!(
        after_spell.state().execution().life_total(P0),
        Some(&23.into()),
        "downstream instruction must read chosen_x bound by BindResolutionX"
    );
    assert_eq!(
        after_spell.state().execution().life_total(P1),
        Some(&20.into())
    );
    assert_eq!(
        after_spell.state().execution().window().entries().len(),
        1,
        "EmitReflexiveTrigger must put a reflexive trigger on the stack"
    );

    // Resolving the reflexive trigger reads the captured chosen_x = 3 and makes P1 lose 3 life.
    let after_reflexive = pass_pair(after_spell.state());
    assert!(
        after_reflexive
            .state()
            .execution()
            .window()
            .entries()
            .is_empty()
    );
    assert_eq!(
        after_reflexive.state().execution().life_total(P1),
        Some(&17.into()),
        "reflexive trigger must preserve and read chosen_x bound by BindResolutionX"
    );
}

#[test]
fn conditional_flash_condition_and_pay_extra_mana() {
    let vocab = vocabulary();

    // Part A: SelfFlashPermission::Condition.
    let mut cond_sorcery = Program::sorcery(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(3),
            }],
        ),
    );
    cond_sorcery.conditional_flash = Some(SelfFlashPermission::Condition(Box::new(
        ResolutionCondition {
            left: ValueExpr::controlled_type(CardType::Artifact),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        },
    )));
    let cond_sorcery = validated(cond_sorcery, &vocab);

    let mut flash_relic = Program::artifact(Some(ManaCost::free()), vec![]);
    flash_relic.printed_keywords.insert(KeywordAbility::Flash);
    let flash_relic = validated(flash_relic, &vocab);

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, cond_sorcery, false),
            card(2, flash_relic, false),
            library(3, P0, &vocab),
            library(4, P1, &vocab),
        ],
        vec![],
    );
    let opp_turn_p0 = advance_to_p1_upkeep_p0_priority(&game_a);

    // Condition false (0 artifacts): sorcery cannot be cast on opponent's turn.
    assert_eq!(
        attempt(
            opp_turn_p0.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(opp_turn_p0.state(), 1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::Timing(
            TimingError::NotActivePlayer
        )))
    );

    // Cast flash artifact on opponent's turn; condition becomes true so sorcery now casts at instant speed.
    let cast_art = submit(
        opp_turn_p0.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(opp_turn_p0.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let with_art = pass_in_order(cast_art.state(), [P0, P1]);
    let p0_prio_again = submit(with_art.state(), P1, GameCommand::Pass);
    let cast_sorc = submit(
        p0_prio_again.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(p0_prio_again.state(), 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let resolved_a = pass_in_order(cast_sorc.state(), [P0, P1]);
    assert_eq!(
        resolved_a.state().execution().life_total(P0),
        Some(&23.into()),
        "sorcery with satisfied SelfFlashPermission::Condition must resolve on opponent's turn"
    );

    // Part B: SelfFlashPermission::PayExtraMana.
    let mut paid_flash_sorcery = Program::sorcery(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(4),
            }],
        ),
    );
    paid_flash_sorcery.conditional_flash = Some(SelfFlashPermission::PayExtraMana(
        ManaCost::generic(2.into()),
    ));
    let paid_flash_sorcery = validated(paid_flash_sorcery, &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, validated(colorless_rock(2), &vocab), true),
            card(2, paid_flash_sorcery.clone(), false),
            card(3, paid_flash_sorcery, false),
            library(4, P0, &vocab),
            library(5, P1, &vocab),
        ],
        vec![CardId(1)],
    );

    // During controller's own main phase, extra flash mana is NOT charged.
    let main_resolved = cast_untargeted(&game_b, 2);
    assert_eq!(
        main_resolved.state().execution().life_total(P0),
        Some(&24.into())
    );

    // On opponent's turn, casting without paying the extra {2} is rejected.
    let opp_turn_b = advance_to_p1_upkeep_p0_priority(main_resolved.state());
    let unpaid_err = attempt(
        opp_turn_b.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(opp_turn_b.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        matches!(
            unpaid_err,
            Err(GameError::Effect(
                EffectError::InsufficientMana | EffectError::InvalidManaPayment(_)
            ))
        ),
        "SelfFlashPermission::PayExtraMana must reject unpaid instant-speed cast, got {unpaid_err:?}"
    );

    // Paying the extra {2} allows casting and resolving on opponent's turn.
    let funded_b = submit(
        opp_turn_b.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(opp_turn_b.state(), 1),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_paid_flash = submit(
        funded_b.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(funded_b.state(), 3),
            payment: Some(colorless_payment(2)),
        },
    );
    let resolved_b = pass_in_order(cast_paid_flash.state(), [P0, P1]);
    assert_eq!(
        resolved_b.state().execution().life_total(P0),
        Some(&28.into())
    );
}

#[test]
fn cannot_be_copied_prevents_copy_stack_object() {
    let vocab = vocabulary();

    let mut uncopyable = gain_life_instant(4);
    uncopyable.cannot_be_copied = true;
    let uncopyable = validated(uncopyable, &vocab);

    let copyable = validated(gain_life_instant(4), &vocab);
    let copier = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(SpellFilter::any()),
                vec![Instruction::CopyStackObject {
                    target: StackCopyTarget::TargetSpell,
                    count: ValueExpr::literal(1),
                    may_choose_new_targets: false,
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, uncopyable, false),
            card(2, copier.clone(), false),
            card(3, copyable, false),
            card(4, copier, false),
        ],
        vec![],
    );

    // Casting CopyStackObject targeting a cannot_be_copied spell creates NO copy on the stack.
    let cast_a = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry_a = cast_a.state().execution().window().entries()[0].id();
    let cast_copy_a = submit(
        cast_a.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(cast_a.state(), 2),
            target: TargetRef::Stack(entry_a),
            payment: ManaPayment::default(),
        },
    );
    let after_copy_a = pass_pair(cast_copy_a.state());
    assert_eq!(
        after_copy_a.state().execution().window().entries().len(),
        1,
        "cannot_be_copied spell must not generate a stack copy"
    );
    let after_a = pass_pair(after_copy_a.state());
    assert_eq!(after_a.state().execution().life_total(P0), Some(&24.into()));

    // Control: CopyStackObject on a normal spell creates a copy and resolves twice (+8 life).
    let cast_b = submit(
        after_a.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_a.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry_b = cast_b.state().execution().window().entries()[0].id();
    let cast_copy_b = submit(
        cast_b.state(),
        P0,
        GameCommand::CastPaid {
            spell: object(cast_b.state(), 4),
            target: TargetRef::Stack(entry_b),
            payment: ManaPayment::default(),
        },
    );
    let after_copy_b = pass_pair(cast_copy_b.state());
    assert_eq!(
        after_copy_b.state().execution().window().entries().len(),
        2,
        "normal spell must gain a stack copy"
    );
    let after_copy_res = pass_pair(after_copy_b.state());
    let after_orig_res = pass_pair(after_copy_res.state());
    assert_eq!(
        after_orig_res.state().execution().life_total(P0),
        Some(&32.into())
    );
}

#[test]
fn temporary_zone_replacement_exiles_dying_creatures_until_cleanup() {
    let vocab = vocabulary();

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let exile_dying_this_turn = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::TemporaryZoneReplacement(Box::new(
                        ZoneReplacementAbility {
                            id: ZoneReplacementAbilityId(1),
                            actions: ZoneActionPredicate::Any,
                            objects: ZoneObjectKind::Either,
                            subject: ZoneReplacementSubject::Matching {
                                filter: Box::new(ObjectFilter::creature()),
                                owner_relation: None,
                            },
                            destination: PublicZoneDestination::Graveyard,
                            replace_with: PublicZoneDestination::Exile,
                            choice: ZoneReplacementChoice::Mandatory,
                        },
                    ))),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let destroy = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::DestroyTarget]),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, bear(), true),
            card(2, bear(), true),
            card(3, exile_dying_this_turn, false),
            card(4, destroy.clone(), false),
            card(5, destroy, false),
            library(6, P0, &vocab),
            library(7, P1, &vocab),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Install temporary zone replacement on Turn 1 and destroy creature 1 -> exiled instead of graveyard.
    let active = cast_untargeted(&game, 3);
    let after_first_kill = cast_targeted(
        active.state(),
        P0,
        4,
        TargetRef::Object(object(active.state(), 1)),
    );
    assert_eq!(
        after_first_kill
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_first_kill.state(), 1))
            .map(|c| c.zone()),
        Some(ObjectZone::Exile),
        "creature dying while TemporaryZoneReplacement is active must be exiled"
    );

    // Pass through Turn 1 cleanup into Turn 2: temporary replacement must have expired.
    let t1_end = reach_end(after_first_kill.state(), [P0, P1]);
    let t2_upkeep = pass_pair(&t1_end);
    assert_eq!(t2_upkeep.state().turn().unwrap().position().number, 2);
    let p0_prio = submit(t2_upkeep.state(), P1, GameCommand::Pass);
    let after_second_kill = cast_targeted(
        p0_prio.state(),
        P0,
        5,
        TargetRef::Object(object(p0_prio.state(), 2)),
    );
    assert_eq!(
        after_second_kill
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_second_kill.state(), 2))
            .map(|c| c.zone()),
        Some(ObjectZone::Graveyard(P0)),
        "after cleanup expiration, dying creature must go to its owner's graveyard"
    );
}

#[test]
fn ability_change_suppressed_activations_nonmana_vs_all_and_doesnt_untap() {
    let vocab = vocabulary();

    let mut dork = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    dork.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    });
    dork.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    let dork = validated(dork, &vocab);

    let creature_selector = PermanentSelector {
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    };

    // Part A1: ActivationSuppression::NonMana blocks non-mana ability but allows mana ability.
    let mut suppress_nonmana = Program::enchantment(None);
    suppress_nonmana.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: creature_selector.clone(),
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            suppressed_activations: Some(ActivationSuppression::NonMana),
            ..AbilityChange::default()
        })]),
    });
    let suppress_nonmana = validated(suppress_nonmana, &vocab);

    let game_nm = world(
        vocab.clone(),
        vec![card(1, dork.clone(), true), card(2, suppress_nonmana, true)],
        vec![CardId(1), CardId(2)],
    );
    let dork_nm = object(&game_nm, 1);
    assert_eq!(
        attempt(
            &game_nm,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: dork_nm,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    let mana_ok = submit(
        &game_nm,
        P0,
        GameCommand::ActivateMana {
            source: dork_nm,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        mana_ok.state().execution().mana(P0, ManaType::Green),
        1.into()
    );

    // Part A2: ActivationSuppression::All blocks BOTH non-mana and mana abilities.
    let mut suppress_all = Program::enchantment(None);
    suppress_all.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: creature_selector,
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            suppressed_activations: Some(ActivationSuppression::All),
            ..AbilityChange::default()
        })]),
    });
    let suppress_all = validated(suppress_all, &vocab);

    let game_all = world(
        vocab.clone(),
        vec![card(1, dork, true), card(2, suppress_all, true)],
        vec![CardId(1), CardId(2)],
    );
    let dork_all = object(&game_all, 1);
    assert_eq!(
        attempt(
            &game_all,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: dork_all,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    assert_eq!(
        attempt(
            &game_all,
            P0,
            GameCommand::ActivateMana {
                source: dork_all,
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        ),
        Err(GameError::Effect(
            EffectError::ManaActivationConditionNotMet
        ))
    );

    // Part B: AbilityChange.doesnt_untap skips controller's untap step while still allowing spell untap.
    let tap_ability = || ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    };
    let mut locked_art_creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    locked_art_creature.card_types.insert(CardType::Artifact);
    locked_art_creature.activated.push(tap_ability());
    let locked_art_creature = validated(locked_art_creature, &vocab);

    let mut normal_bear = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    normal_bear.activated.push(tap_ability());
    let normal_bear = validated(normal_bear, &vocab);

    let mut stasis_charm = Program::enchantment(None);
    stasis_charm.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
            doesnt_untap: true,
            ..AbilityChange::default()
        })]),
    });
    let stasis_charm = validated(stasis_charm, &vocab);

    let untap_spell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::AffectPermanentTargets {
                    minimum: 1,
                    maximum: 1,
                    action: PermanentTargetAction::Untap,
                }],
            ),
        ),
        &vocab,
    );

    let game_u = world(
        vocab.clone(),
        vec![
            card(1, locked_art_creature, true),
            card(2, normal_bear, true),
            card(3, stasis_charm, true),
            card(4, untap_spell, false),
            library(5, P0, &vocab),
            library(6, P1, &vocab),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let locked_id = object(&game_u, 1);
    let bear_id = object(&game_u, 2);

    // Tap both creatures on Turn 1.
    let t1 = pass_pair(
        submit(
            &game_u,
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: locked_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        )
        .state(),
    );
    let both_tapped = pass_pair(
        submit(
            t1.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: bear_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        )
        .state(),
    );

    // Advance to Turn 3 Upkeep (after P0's untap step).
    let t1_end = reach_end(both_tapped.state(), [P0, P1]);
    let t2_upkeep = pass_pair(&t1_end);
    let t2_draw = pass_in_order(t2_upkeep.state(), [P1, P0]);
    let t2_main = pass_in_order(t2_draw.state(), [P1, P0]);
    let t2_end = reach_end(t2_main.state(), [P1, P0]);
    let t3_upkeep = pass_in_order(&t2_end, [P1, P0]);
    assert_eq!(t3_upkeep.state().turn().unwrap().position().number, 3);
    assert_eq!(
        t3_upkeep.state().turn().unwrap().position().step,
        Step::Upkeep
    );

    // Normal bear untaps; doesnt_untap creature stays tapped.
    assert_eq!(
        t3_upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(bear_id)
            .and_then(|c| c.tapped()),
        Some(false)
    );
    assert_eq!(
        t3_upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(locked_id)
            .and_then(|c| c.tapped()),
        Some(true),
        "AbilityChange.doesnt_untap must keep the permanent tapped through controller's untap step"
    );

    // Unlike CannotBeUntapped, an untap spell STILL untaps a permanent with doesnt_untap.
    let untap_cast = submit(
        t3_upkeep.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(t3_upkeep.state(), 4),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![TargetRef::Object(locked_id)],
                mana: Some(ManaPayment::default()),
                ..CastingPayment::default()
            },
        },
    );
    let after_untap = pass_pair(untap_cast.state());
    assert_eq!(
        after_untap
            .state()
            .execution()
            .window()
            .cards()
            .get(locked_id)
            .and_then(|c| c.tapped()),
        Some(false),
        "AbilityChange.doesnt_untap must still allow untapping via spells or abilities"
    );
}

#[test]
fn temporary_cant_activate_cant_play_lands_and_player_grant_keyword() {
    let vocab = vocabulary();

    // Part A: CantActivateAbilities (nonmana_only: true vs false).
    let mut rock = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(1),
                }],
            ),
        }],
    );
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    let rock = validated(rock, &vocab);

    let silence_nonmana = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantActivateAbilities {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        nonmana_only: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let silence_all = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantActivateAbilities {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        nonmana_only: false,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );

    let game_act = world(
        vocab.clone(),
        vec![
            card(1, rock, true),
            card(2, silence_nonmana, false),
            card(3, silence_all, false),
        ],
        vec![CardId(1)],
    );
    let rock_id = object(&game_act, 1);

    let nm_active = cast_untargeted(&game_act, 2);
    assert_eq!(
        attempt(
            nm_active.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: rock_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    let nm_mana = submit(
        nm_active.state(),
        P0,
        GameCommand::ActivateMana {
            source: rock_id,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        nm_mana.state().execution().mana(P0, ManaType::Green),
        1.into()
    );

    let all_active = cast_untargeted(nm_mana.state(), 3);
    assert_eq!(
        attempt(
            all_active.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: rock_id,
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::ActivationConditionNotMet))
    );
    assert_eq!(
        attempt(
            all_active.state(),
            P0,
            GameCommand::ActivateMana {
                source: rock_id,
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        ),
        Err(GameError::Effect(
            EffectError::ManaActivationConditionNotMet
        ))
    );

    // Part B: CantPlayLands blocks land play this turn and expires on next turn.
    let block_lands = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantPlayLands {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let game_land = world(
        vocab.clone(),
        vec![
            card(1, block_lands, false),
            card(2, validated(basic_forest(), &vocab), false),
            library(3, P0, &vocab),
            library(4, P1, &vocab),
        ],
        vec![],
    );
    let land_blocked = cast_untargeted(&game_land, 1);
    assert_eq!(
        attempt(
            land_blocked.state(),
            P0,
            GameCommand::PlayLand {
                land: object(land_blocked.state(), 2),
            },
        ),
        Err(GameError::Turn(TurnError::LandLimitReached))
    );

    // Advance to Turn 3 PrecombatMain (P0's next turn): rule has expired and land play succeeds.
    let t1_end = reach_end(land_blocked.state(), [P0, P1]);
    let t2_upkeep = pass_pair(&t1_end);
    let t2_draw = pass_in_order(t2_upkeep.state(), [P1, P0]);
    let t2_main = pass_in_order(t2_draw.state(), [P1, P0]);
    let t2_end = reach_end(t2_main.state(), [P1, P0]);
    let t3_upkeep = pass_in_order(&t2_end, [P1, P0]);
    let t3_draw = pass_pair(t3_upkeep.state());
    let t3_main = pass_pair(t3_draw.state());
    let played = submit(
        t3_main.state(),
        P0,
        GameCommand::PlayLand {
            land: object(t3_main.state(), 2),
        },
    );
    assert_eq!(
        played
            .state()
            .execution()
            .window()
            .cards()
            .get(object(played.state(), 2))
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );

    // Part C: PlayerGrantKeyword (Hexproof and ProtectionFromRed).
    let grant_hexproof = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        keywords: BTreeSet::from([KeywordAbility::Hexproof]),
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let grant_prot_red = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        keywords: BTreeSet::from([KeywordAbility::ProtectionFromRed]),
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let mut red_aoe = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DamageOpponentsAmount(ValueExpr::literal(3))],
        ),
    );
    red_aoe.color_indicator = Some(BTreeSet::from([Color::Red]));
    let red_aoe = validated(red_aoe, &vocab);

    let mut blue_aoe = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DamageOpponentsAmount(ValueExpr::literal(3))],
        ),
    );
    blue_aoe.color_indicator = Some(BTreeSet::from([Color::Blue]));
    let blue_aoe = validated(blue_aoe, &vocab);

    let bolt = validated(damage_any_instant(2), &vocab);

    let game_kw = world(
        vocab,
        vec![
            card(1, grant_hexproof, false),
            card(2, grant_prot_red, false),
            card(3, bolt.clone(), false),
            opp_hand(4, bolt),
            opp_hand(5, red_aoe),
            opp_hand(6, blue_aoe),
        ],
        vec![],
    );

    // Grant Hexproof to P0: P1 cannot target P0, but P0 can still target self.
    let hex_p0 = cast_untargeted(&game_kw, 1);
    let p1_prio = submit(hex_p0.state(), P0, GameCommand::Pass);
    assert_eq!(
        attempt(
            p1_prio.state(),
            P1,
            GameCommand::CastPaid {
                spell: object(p1_prio.state(), 4),
                target: TargetRef::Player(P0),
                payment: ManaPayment::default(),
            },
        ),
        Err(GameError::Effect(EffectError::IllegalTarget))
    );
    let self_bolt = cast_targeted(hex_p0.state(), P0, 3, TargetRef::Player(P0));
    assert_eq!(
        self_bolt.state().execution().life_total(P0),
        Some(&18.into())
    );

    // Grant ProtectionFromRed to P0: red untargeted damage is prevented, blue untargeted damage deals 3.
    let prot_p0 = cast_untargeted(self_bolt.state(), 2);
    let p1_prio_r = submit(prot_p0.state(), P0, GameCommand::Pass);
    let cast_red = submit(
        p1_prio_r.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio_r.state(), 5),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_red = pass_in_order(cast_red.state(), [P1, P0]);
    assert_eq!(
        after_red.state().execution().life_total(P0),
        Some(&18.into()),
        "ProtectionFromRed granted via PlayerGrantKeyword must prevent red damage to controller"
    );

    let p1_prio_u = submit(after_red.state(), P0, GameCommand::Pass);
    let cast_blue = submit(
        p1_prio_u.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(p1_prio_u.state(), 6),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_blue = pass_in_order(cast_blue.state(), [P1, P0]);
    assert_eq!(
        after_blue.state().execution().life_total(P0),
        Some(&15.into()),
        "ProtectionFromRed must not prevent blue damage"
    );
}

#[test]
fn next_only_rules_for_cant_be_countered_filtered_flash_and_next_spell_free() {
    let vocab = vocabulary();

    // Part A: CantBeCountered { next_only: true } protects first spell only.
    let next_uncounterable = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantBeCountered {
                        filter: None,
                        controller_only: true,
                        next_only: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let counterspell = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(SpellFilter::any()),
                vec![Instruction::CounterSpell],
            ),
        ),
        &vocab,
    );
    let heal = validated(gain_life_instant(3), &vocab);

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, next_uncounterable, false),
            card(2, heal.clone(), false),
            card(3, heal, false),
            opp_hand(4, counterspell.clone()),
            opp_hand(5, counterspell),
        ],
        vec![],
    );
    let primed_a = cast_untargeted(&game_a, 1);

    // First spell cannot be countered.
    let cast1 = submit(
        primed_a.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(primed_a.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry1 = cast1.state().execution().window().entries()[0].id();
    let p1_resp1 = submit(cast1.state(), P0, GameCommand::Pass);
    let ctr1 = submit(
        p1_resp1.state(),
        P1,
        GameCommand::CastPaid {
            spell: object(p1_resp1.state(), 4),
            target: TargetRef::Stack(entry1),
            payment: ManaPayment::default(),
        },
    );
    let after_ctr1 = pass_in_order(ctr1.state(), [P1, P0]);
    assert_eq!(
        after_ctr1.state().execution().window().entries().len(),
        1,
        "first spell must survive counterspell under CantBeCountered next_only"
    );
    let after_spell1 = pass_pair(after_ctr1.state());
    assert_eq!(
        after_spell1.state().execution().life_total(P0),
        Some(&23.into())
    );

    // Second spell CAN be countered because the next_only rule was consumed.
    let cast2 = submit(
        after_spell1.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_spell1.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry2 = cast2.state().execution().window().entries()[0].id();
    let p1_resp2 = submit(cast2.state(), P0, GameCommand::Pass);
    let ctr2 = submit(
        p1_resp2.state(),
        P1,
        GameCommand::CastPaid {
            spell: object(p1_resp2.state(), 5),
            target: TargetRef::Stack(entry2),
            payment: ManaPayment::default(),
        },
    );
    let after_ctr2 = pass_in_order(ctr2.state(), [P1, P0]);
    assert!(
        after_ctr2.state().execution().window().entries().is_empty(),
        "second spell must be countered after next_only CantBeCountered was consumed"
    );
    assert_eq!(
        after_ctr2.state().execution().life_total(P0),
        Some(&23.into())
    );

    // Part B: FilteredFlash { next_only: true } grants flash to first matching creature only.
    let next_creature_flash = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::FilteredFlash {
                        player: ResolvingPlayer::Controller,
                        filter: ObjectFilter::creature(),
                        next_only: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let bear = validated(
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, next_creature_flash, false),
            card(2, bear.clone(), false),
            card(3, bear, false),
            library(4, P0, &vocab),
            library(5, P1, &vocab),
        ],
        vec![],
    );
    let opp_turn_b = advance_to_p1_upkeep_p0_priority(&game_b);

    let flash_cast = submit(
        opp_turn_b.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(opp_turn_b.state(), 1),
            payment: Some(ManaPayment::default()),
        },
    );
    let flash_ready = pass_in_order(flash_cast.state(), [P0, P1]);
    let p0_prio_b1 = submit(flash_ready.state(), P1, GameCommand::Pass);

    // First creature casts at instant speed on opponent's turn.
    let cast_c1 = submit(
        p0_prio_b1.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(p0_prio_b1.state(), 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let after_c1 = pass_in_order(cast_c1.state(), [P0, P1]);
    assert_eq!(
        after_c1
            .state()
            .execution()
            .window()
            .cards()
            .get(object(after_c1.state(), 2))
            .map(|c| c.zone()),
        Some(ObjectZone::Battlefield)
    );

    // Second creature is rejected on opponent's turn because FilteredFlash next_only was consumed.
    let p0_prio_b2 = submit(after_c1.state(), P1, GameCommand::Pass);
    assert_eq!(
        attempt(
            p0_prio_b2.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(p0_prio_b2.state(), 3),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::Timing(
            TimingError::NotActivePlayer
        )))
    );

    // Part C: NextSpellFree allows casting first matching spell for free and is consumed on cast.
    let next_free = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::NextSpellFree { filter: None }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        ),
        &vocab,
    );
    let expensive_heal = validated(
        Program::instant(
            ManaCost::generic(5.into()),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::literal(4),
                }],
            ),
        ),
        &vocab,
    );

    let game_c = world(
        vocab,
        vec![
            card(1, next_free, false),
            card(2, expensive_heal.clone(), false),
            card(3, expensive_heal, false),
        ],
        vec![],
    );
    let free_ready = cast_untargeted(&game_c, 1);

    // First 5-mana spell casts for 0 mana.
    let after_free_cast = cast_untargeted(free_ready.state(), 2);
    assert_eq!(
        after_free_cast.state().execution().life_total(P0),
        Some(&24.into())
    );

    // Second 5-mana spell without mana is rejected because NextSpellFree was consumed.
    let second_err = attempt(
        after_free_cast.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(after_free_cast.state(), 3),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        matches!(
            second_err,
            Err(GameError::Effect(
                EffectError::InsufficientMana | EffectError::InvalidManaPayment(_)
            ))
        ),
        "second spell must require its normal mana cost after NextSpellFree is consumed, got {second_err:?}"
    );
}

#[test]
fn static_restrictions_life_total_cant_change_and_players_cant_search_libraries() {
    let vocab = vocabulary();

    // Part A: StaticRestriction::ControllerLifeTotalCantChange.
    let mut emperion = Program::enchantment(None);
    emperion.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::ControllerLifeTotalCantChange,
    });
    let emperion = validated(emperion, &vocab);

    let life_payer = validated(
        Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::PayLife {
                    amount: LifeCostAmount::Fixed(2),
                    tap_source: false,
                    sacrifice: None,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            }],
        ),
        &vocab,
    );
    let gain_five = validated(gain_life_instant(5), &vocab);
    let lose_four = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustPlayersLife {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    gain: false,
                    amount: ValueExpr::literal(4),
                }],
            ),
        ),
        &vocab,
    );
    let bolt_three = validated(damage_any_instant(3), &vocab);

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, emperion, true),
            card(2, life_payer, true),
            card(3, gain_five, false),
            card(4, lose_four, false),
            card(5, bolt_three.clone(), false),
            card(6, bolt_three, false),
        ],
        vec![CardId(1), CardId(2)],
    );

    // Life gain is blocked.
    let s1 = cast_untargeted(&game_a, 3);
    assert_eq!(s1.state().execution().life_total(P0), Some(&20.into()));

    // Life loss is blocked.
    let s2 = cast_untargeted(s1.state(), 4);
    assert_eq!(s2.state().execution().life_total(P0), Some(&20.into()));

    // Damage life change to controller is blocked, while opponent's life still changes normally.
    let s3 = cast_targeted(s2.state(), P0, 5, TargetRef::Player(P0));
    assert_eq!(s3.state().execution().life_total(P0), Some(&20.into()));
    let s4 = cast_targeted(s3.state(), P0, 6, TargetRef::Player(P1));
    assert_eq!(s4.state().execution().life_total(P1), Some(&17.into()));

    // Paying positive life as a cost is rejected.
    assert_eq!(
        attempt(
            s4.state(),
            P0,
            GameCommand::ActivateUntargetedAbility {
                source: object(s4.state(), 2),
                ability: ActivatedAbilityId(1),
                payment: Some(ManaPayment::default()),
            },
        ),
        Err(GameError::Effect(EffectError::InvalidCastResource))
    );

    // Part B: StaticRestriction::PlayersCantSearchLibraries.
    let mut no_search = Program::enchantment(None);
    no_search.continuous = Some(StaticContinuousAbility::Restriction {
        id: StaticAbilityId(1),
        rule: StaticRestriction::PlayersCantSearchLibraries(PlayerRelation::Controller),
    });
    let no_search = validated(no_search, &vocab);

    let search_and_check = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch {
                            searcher: ResolvingPlayer::Controller,
                            source: SearchSource::OwnLibrary,
                            matcher: SearchCardMatcher::Any,
                            mana_value: None,
                            count: SearchCount::UpTo(1),
                            pair_constraint: None,
                            destination: SearchDestination::Hand,
                            reveal: false,
                        }),
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        },
                        destination: 3,
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(5),
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, no_search, true),
            card(2, search_and_check, false),
            library_top(3, P0, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1)],
    );

    // Search does not emit a pending search frame and sets last_action_succeeded = false (skips +5, gains +1).
    let after_blocked_search = cast_untargeted(&game_b, 2);
    assert!(
        after_blocked_search.state().pending_decision().is_none(),
        "PlayersCantSearchLibraries must prevent SearchLibrary from emitting a search frame"
    );
    assert_eq!(
        after_blocked_search.state().execution().life_total(P0),
        Some(&21.into()),
        "blocked SearchLibrary must mark last_action_succeeded = false"
    );
    assert_eq!(
        after_blocked_search
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        1
    );
}

#[test]
fn set_day_night_transitions_and_reconciles_daybound_nightbound() {
    let vocab = vocabulary();

    let mut back_face = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
    back_face
        .printed_keywords
        .insert(KeywordAbility::Nightbound);

    let mut werewolf =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    werewolf.printed_keywords.insert(KeywordAbility::Daybound);
    werewolf.nightbound_back = Some(Box::new(back_face));
    let werewolf = validated(werewolf, &vocab);

    let make_night = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SetDayNight(DayNight::Night)],
            ),
        ),
        &vocab,
    );
    let make_day = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SetDayNight(DayNight::Day)],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, make_night.clone(), false),
            card(2, werewolf, false),
            card(3, make_day, false),
            card(4, make_night, false),
        ],
        vec![],
    );
    assert_eq!(game.execution().day_night(), DayNight::Neither);

    // Transition Neither -> Night.
    let night1 = cast_untargeted(&game, 1);
    assert_eq!(night1.state().execution().day_night(), DayNight::Night);

    // Cast Daybound/Nightbound werewolf during Night -> enters on its Nightbound 4/4 back face.
    let wolf_entered = cast_untargeted(night1.state(), 2);
    let wolf_id = object(wolf_entered.state(), 2);
    assert_eq!(
        wolf_entered
            .state()
            .execution()
            .window()
            .cards()
            .get(wolf_id)
            .map(|c| c.transformed()),
        Some(true)
    );
    assert_eq!(
        current(wolf_entered.state(), wolf_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );

    // Transition Night -> Day: reconciles werewolf back to its Daybound 2/2 front face.
    let day1 = cast_untargeted(wolf_entered.state(), 3);
    assert_eq!(day1.state().execution().day_night(), DayNight::Day);
    assert_eq!(
        day1.state()
            .execution()
            .window()
            .cards()
            .get(wolf_id)
            .map(|c| c.transformed()),
        Some(false)
    );
    assert_eq!(
        current(day1.state(), wolf_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(2, 2)
    );

    // Transition Day -> Night: reconciles werewolf back to its Nightbound 4/4 back face.
    let night2 = cast_untargeted(day1.state(), 4);
    assert_eq!(night2.state().execution().day_night(), DayNight::Night);
    assert_eq!(
        night2
            .state()
            .execution()
            .window()
            .cards()
            .get(wolf_id)
            .map(|c| c.transformed()),
        Some(true)
    );
    assert_eq!(
        current(night2.state(), wolf_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(4, 4)
    );
}

#[test]
fn combat_become_blocked_and_must_block_all_attackers() {
    let vocab = vocabulary();

    // Part A: CombatActionKind::BecomeBlocked marks an unblocked attacker blocked so it deals 0 combat damage.
    let attacker_prog = validated(
        Program::creature(None, 3.into(), 3.into(), BTreeSet::new()),
        &vocab,
    );
    let curtain_of_light = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::RequireCombatAction {
                    subject: Box::new(EffectSubject::TargetObject),
                    kind: CombatActionKind::BecomeBlocked,
                }],
            ),
        ),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![card(1, attacker_prog, true), opp_hand(2, curtain_of_light)],
        vec![CardId(1)],
    );
    let att_id = object(&game_a, 1);

    let beg_combat = pass_pair(&game_a);
    let dec_att = pass_pair(beg_combat.state());
    let after_att = reply(
        dec_att.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: att_id,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let dec_blk = pass_pair(after_att.state());
    let after_no_blk = if let Some(decision) = dec_blk.state().pending_decision() {
        assert!(matches!(
            decision.frame(),
            RuleFrame::Combat(CombatDecision::Blockers(_))
        ));
        reply(dec_blk.state(), P1, DecisionReply::DeclareBlockers(vec![]))
    } else {
        dec_blk
    };

    assert_eq!(
        after_no_blk
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .find(|a| a.combatant().object() == att_id)
            .unwrap()
            .block_status(),
        BlockStatus::Unblocked
    );

    // P0 passes to P1 in DeclareBlockers; P1 casts BecomeBlocked targeting att_id.
    let p1_blk_prio = submit(after_no_blk.state(), P0, GameCommand::Pass);
    let after_become_blocked = cast_targeted(p1_blk_prio.state(), P1, 2, TargetRef::Object(att_id));
    assert_eq!(
        after_become_blocked
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .find(|a| a.combatant().object() == att_id)
            .unwrap()
            .block_status(),
        BlockStatus::Blocked,
        "CombatActionKind::BecomeBlocked must mark an unblocked attacker as blocked"
    );

    // Advance into CombatDamage: blocked attacker with 0 blockers and no trample deals 0 damage to P1.
    let combat_dmg = pass_pair(after_become_blocked.state());
    assert_eq!(
        combat_dmg.state().execution().life_total(P1),
        Some(&20.into())
    );

    // Part B: BlockingRestriction::MustBlockAllAttackers enforces blocking every eligible attacker.
    let two_two = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let mut mega_blocker = Program::creature(None, 5.into(), 5.into(), BTreeSet::new());
    let self_sel = PermanentSelector {
        controller: ControllerSelector::SourceItself,
        ..PermanentSelector::default()
    };
    mega_blocker.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: self_sel.clone(),
        parts: StaticParts(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CanBlockAnyNumber,
        )]),
    });
    mega_blocker
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(2),
            selector: self_sel,
            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::MustBlockAllAttackers,
            )]),
        });
    let mega_blocker = validated(mega_blocker, &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, two_two(), true),
            card(2, two_two(), true),
            opp_battlefield(3, mega_blocker),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let a1 = object(&game_b, 1);
    let a2 = object(&game_b, 2);
    let blocker = object(&game_b, 3);

    let beg_b = pass_pair(&game_b);
    let dec_att_b = pass_pair(beg_b.state());
    let after_att_b = reply(
        dec_att_b.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![
            AttackDeclaration {
                attacker: a1,
                defender: P1,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: a2,
                defender: P1,
                defending_planeswalker: None,
            },
        ]),
    );
    let dec_blk_b = pass_pair(after_att_b.state());
    let blk_decision = dec_blk_b
        .state()
        .pending_decision()
        .expect("DeclareBlockers decision expected");
    let blk_decision_id = blk_decision.id();

    // Blocking 0 or only 1 of the 2 attackers is rejected as IllegalBlocker.
    for insufficient in [
        vec![],
        vec![BlockDeclaration {
            blocker,
            attacker: a1,
        }],
        vec![BlockDeclaration {
            blocker,
            attacker: a2,
        }],
    ] {
        assert_eq!(
            attempt(
                dec_blk_b.state(),
                P1,
                GameCommand::Reply {
                    decision: blk_decision_id,
                    choice: DecisionReply::DeclareBlockers(insufficient),
                },
            ),
            Err(GameError::Effect(EffectError::Combat(
                CombatError::IllegalBlocker
            )))
        );
    }

    // Blocking BOTH attackers satisfies MustBlockAllAttackers and succeeds.
    let blocked_both = reply(
        dec_blk_b.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![
            BlockDeclaration {
                blocker,
                attacker: a1,
            },
            BlockDeclaration {
                blocker,
                attacker: a2,
            },
        ]),
    );
    let combat = blocked_both.state().execution().combat().unwrap();
    assert_eq!(
        combat
            .attackers()
            .find(|a| a.combatant().object() == a1)
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
    assert_eq!(
        combat
            .attackers()
            .find(|a| a.combatant().object() == a2)
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
}

#[test]
fn put_resolving_spell_in_library_and_put_subject_into_library_from_top() {
    let vocab = vocabulary();

    // Part A: PutResolvingSpellInLibrary (Top and Bottom instead of graveyard).
    let to_top = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(2),
                    },
                    Instruction::PutResolvingSpellInLibrary(LibraryPlacement::Top),
                ],
            ),
        ),
        &vocab,
    );
    let to_bottom = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    },
                    Instruction::PutResolvingSpellInLibrary(LibraryPlacement::Bottom),
                ],
            ),
        ),
        &vocab,
    );

    let game_a = world(
        vocab.clone(),
        vec![
            card(1, to_top, false),
            card(2, to_bottom, false),
            library_top(3, P0, validated(basic_forest(), &vocab)),
        ],
        vec![],
    );

    let after_top = cast_untargeted(&game_a, 1);
    assert_eq!(
        after_top.state().execution().life_total(P0),
        Some(&22.into())
    );
    let lib_after_top = after_top.state().execution().window().cards().library(P0);
    assert_eq!(lib_after_top.len(), 2);
    assert_eq!(
        lib_after_top[0],
        object(after_top.state(), 1),
        "PutResolvingSpellInLibrary(Top) must place the resolved spell at index 0 of library"
    );
    assert!(
        after_top
            .state()
            .execution()
            .window()
            .cards()
            .graveyard(P0)
            .is_empty()
    );

    let after_bottom = cast_untargeted(after_top.state(), 2);
    assert_eq!(
        after_bottom.state().execution().life_total(P0),
        Some(&23.into())
    );
    let lib_after_bottom = after_bottom
        .state()
        .execution()
        .window()
        .cards()
        .library(P0);
    assert_eq!(lib_after_bottom.len(), 3);
    assert_eq!(
        lib_after_bottom.last().copied(),
        Some(object(after_bottom.state(), 2)),
        "PutResolvingSpellInLibrary(Bottom) must place the resolved spell at the bottom of library"
    );
    assert!(
        after_bottom
            .state()
            .execution()
            .window()
            .cards()
            .graveyard(P0)
            .is_empty()
    );

    // Part B: PutSubjectIntoLibraryFromTop with a dynamic offset (controlled artifact count = 2).
    let tuck_dynamic = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::PutSubjectIntoLibraryFromTop {
                    subject: MoveSubjectRef::TargetObject,
                    offset: ValueExpr::controlled_type(CardType::Artifact),
                }],
            ),
        ),
        &vocab,
    );
    let bear = validated(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        &vocab,
    );
    let relic = || validated(Program::artifact(None, vec![]), &vocab);

    let game_b = world(
        vocab.clone(),
        vec![
            card(1, bear, true),
            card(2, relic(), true),
            card(3, relic(), true),
            card(4, tuck_dynamic, false),
            library_top(10, P0, validated(basic_forest(), &vocab)),
            library_top(11, P0, validated(basic_forest(), &vocab)),
            library_top(12, P0, validated(basic_forest(), &vocab)),
            library_top(13, P0, validated(basic_forest(), &vocab)),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let bear_id = object(&game_b, 1);

    let after_tuck = cast_targeted(&game_b, P0, 4, TargetRef::Object(bear_id));
    let lib_b = after_tuck.state().execution().window().cards().library(P0);
    assert_eq!(lib_b.len(), 5);
    assert_eq!(
        lib_b[2],
        object(after_tuck.state(), 1),
        "PutSubjectIntoLibraryFromTop with offset=2 must insert the tucked permanent at index 2 (third from top)"
    );
}

#[test]
fn search_count_up_to_expr_enforces_dynamic_limit() {
    let vocab = vocabulary();

    let relic = || validated(Program::artifact(None, vec![]), &vocab);
    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };
    let dynamic_tutor = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch {
                        searcher: ResolvingPlayer::Controller,
                        source: SearchSource::OwnLibrary,
                        matcher: SearchCardMatcher::CardTypes(BTreeSet::from([CardType::Creature])),
                        mana_value: None,
                        count: SearchCount::UpToExpr(Box::new(ValueExpr::controlled_type(
                            CardType::Artifact,
                        ))),
                        pair_constraint: None,
                        destination: SearchDestination::Hand,
                        reveal: false,
                    }),
                }],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab.clone(),
        vec![
            card(1, relic(), true),
            card(2, relic(), true),
            card(3, dynamic_tutor, false),
            library_top(10, P0, bear()),
            library_top(11, P0, bear()),
            library_top(12, P0, bear()),
        ],
        vec![CardId(1), CardId(2)],
    );

    let pending = cast_untargeted(&game, 3);
    let decision = pending
        .state()
        .pending_decision()
        .expect("dynamic SearchLibrary must emit a LibraryTop decision");
    let RuleFrame::LibraryTop(frame) = decision.frame() else {
        panic!("expected LibraryTop frame");
    };
    assert!(frame.is_bounded_batch_search());
    assert_eq!(
        frame.search_batch_limit(),
        2,
        "UpToExpr must evaluate dynamically to controlled artifact count (2)"
    );
    assert_eq!(frame.eligible_results().len(), 3);

    let eligible = frame.eligible_results().to_vec();

    // Selecting 3 cards exceeds dynamic limit 2 and is rejected.
    assert_eq!(
        attempt(
            pending.state(),
            P0,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                    selected: eligible.clone(),
                    colors: vec![],
                }),
            },
        ),
        Err(GameError::Effect(EffectError::InvalidLibraryPartition))
    );

    // Selecting 2 cards succeeds and moves both to P0's hand.
    let done = reply(
        pending.state(),
        P0,
        DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
            selected: eligible[..2].to_vec(),
            colors: vec![],
        }),
    );
    assert!(done.state().pending_decision().is_none());
    let hand_count = done
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(P0))
        .count();
    assert_eq!(hand_count, 2);
    assert_eq!(
        done.state().execution().window().cards().library(P0).len(),
        1
    );
}

#[test]
fn last_amassed_army_reads_power_of_amassed_token() {
    let vocab = vocabulary();
    let army = vocab.resolve_label("Army").unwrap();
    let zombie = vocab.resolve_label("Zombie").unwrap();

    let amass_and_gain = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::CreateArmyIfAbsent {
                        army,
                        subtype: zombie,
                    },
                    Instruction::Amass {
                        army,
                        subtype: zombie,
                        amount: 3,
                    },
                    Instruction::GainControllerLife {
                        amount: ValueExpr::object_stat(
                            ValueObjectRef::LastAmassedArmy,
                            ObjectStatKind::Power,
                        ),
                    },
                ],
            ),
        ),
        &vocab,
    );

    let game = world(
        vocab,
        vec![
            card(1, amass_and_gain.clone(), false),
            card(2, amass_and_gain, false),
        ],
        vec![],
    );

    // First amass creates a 0/0 Zombie Army and adds three +1/+1 counters -> LastAmassedArmy power = 3.
    let after_first = cast_untargeted(&game, 1);
    assert_eq!(
        after_first.state().execution().life_total(P0),
        Some(&23.into()),
        "LastAmassedArmy must read power 3 of the freshly amassed Army"
    );

    // Second amass grows the existing Army to 6/6 -> LastAmassedArmy power = 6.
    let after_second = cast_untargeted(after_first.state(), 2);
    assert_eq!(
        after_second.state().execution().life_total(P0),
        Some(&29.into()),
        "LastAmassedArmy must read updated power 6 of the grown Army"
    );
}

#[test]
fn additional_cost_paid_distinguishes_unpaid_vs_kicked() {
    let vocab = vocabulary();

    let mut kicked_spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::event(EventValueKind::AdditionalCostPaid),
            }],
        ),
    );
    kicked_spell.additional_cast_costs =
        vec![AdditionalCastCost::Kicker(ManaCost::generic(1.into()))];
    let kicked_spell = validated(kicked_spell, &vocab);

    let game = world(
        vocab.clone(),
        vec![
            card(1, validated(colorless_rock(1), &vocab), true),
            card(2, kicked_spell.clone(), false),
            card(3, kicked_spell, false),
        ],
        vec![CardId(1)],
    );

    // Cast without paying kicker -> AdditionalCostPaid evaluates to 0.
    let after_unkicked = cast_untargeted(&game, 2);
    assert_eq!(
        after_unkicked.state().execution().life_total(P0),
        Some(&20.into()),
        "AdditionalCostPaid must evaluate to 0 when kicker is not paid"
    );

    // Pay kicker -> AdditionalCostPaid evaluates to 1.
    let funded = submit(
        after_unkicked.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(after_unkicked.state(), 1),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let cast_kicked = submit(
        funded.state(),
        P0,
        GameCommand::CastWithResources {
            spell: object(funded.state(), 3),
            target: None,
            payment: CastingPayment {
                additional: BTreeMap::from([(0, 1.into())]),
                mana: Some(colorless_payment(1)),
                ..CastingPayment::default()
            },
        },
    );
    let after_kicked = pass_pair(cast_kicked.state());
    assert_eq!(
        after_kicked.state().execution().life_total(P0),
        Some(&21.into()),
        "AdditionalCostPaid must evaluate to 1 when kicker is paid"
    );
}

#[test]
fn pay_energy_expr_chosen_x_and_channel_with_permanent_targets() {
    let vocab = vocabulary();

    // Part A: ExtraActivationCost::PayEnergyExpr(ValueExpr::chosen_x()).
    let mut dynamo = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::chosen_x(),
                }],
            ),
        }],
    );
    dynamo.extra_activation_costs.insert(
        ActivatedAbilityId(1),
        vec![ExtraActivationCost::PayEnergyExpr(ValueExpr::chosen_x())],
    );
    let dynamo = validated(dynamo, &vocab);

    let charge_energy = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AddPlayerCounter {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    kind: CounterKind::Energy,
                    amount: ValueExpr::literal(4),
                }],
            ),
        ),
        &vocab,
    );

    // Part B: HandDiscardKeyword::Channel with PermanentTargets (1..=2 creatures tapped).
    let mut channel_card =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    channel_card.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel),
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::AffectPermanentTargets {
                minimum: 1,
                maximum: 2,
                action: PermanentTargetAction::Tap,
            }],
        ),
    });
    let channel_card = validated(channel_card, &vocab);

    let bear = || {
        validated(
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            &vocab,
        )
    };

    let game = world(
        vocab.clone(),
        vec![
            card(1, dynamo, true),
            card(2, charge_energy, false),
            card(3, channel_card, false),
            opp_battlefield(4, bear()),
            opp_battlefield(5, bear()),
        ],
        vec![CardId(1), CardId(4), CardId(5)],
    );
    let dynamo_id = object(&game, 1);
    let opp_c1 = object(&game, 4);
    let opp_c2 = object(&game, 5);

    // Give P0 4 energy counters.
    let charged = cast_untargeted(&game, 2);
    assert_eq!(
        charged
            .state()
            .execution()
            .player_counters_of_kind(P0, CounterKind::Energy),
        4.into()
    );

    // Activating PayEnergyExpr(ChosenX) with X = 5 (> 4 available) is rejected.
    assert_eq!(
        attempt(
            charged.state(),
            P0,
            GameCommand::ActivateWithRestrictedMana {
                source: dynamo_id,
                ability: ActivatedAbilityId(1),
                granting_effect: None,
                origin: ActivationOrigin::Untargeted,
                chosen_x: Some(5.into()),
                payment: Some(ManaPayment::default()),
                restricted_mana: vec![],
            },
        ),
        Err(GameError::Effect(EffectError::InvalidCastResource))
    );

    // Activating with X = 3 pays 3 energy (leaving 1) and gains 3 life on resolution.
    let act_energy = submit(
        charged.state(),
        P0,
        GameCommand::ActivateWithRestrictedMana {
            source: dynamo_id,
            ability: ActivatedAbilityId(1),
            granting_effect: None,
            origin: ActivationOrigin::Untargeted,
            chosen_x: Some(3.into()),
            payment: Some(ManaPayment::default()),
            restricted_mana: vec![],
        },
    );
    assert_eq!(
        act_energy
            .state()
            .execution()
            .player_counters_of_kind(P0, CounterKind::Energy),
        1.into(),
        "PayEnergyExpr(ChosenX) must spend X energy counters at activation"
    );
    let after_energy = pass_pair(act_energy.state());
    assert_eq!(
        after_energy.state().execution().life_total(P0),
        Some(&23.into()),
        "ability body must read chosen_x paid to PayEnergyExpr"
    );

    // Activate Channel ability from hand using ActivationOrigin::PermanentTargets targeting both opponent creatures.
    let channel_act = submit(
        after_energy.state(),
        P0,
        GameCommand::ActivatePermanentTargets {
            source: object(after_energy.state(), 3),
            ability: ActivatedAbilityId(1),
            targets: vec![TargetRef::Object(opp_c1), TargetRef::Object(opp_c2)],
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        channel_act
            .state()
            .execution()
            .window()
            .cards()
            .get(object(channel_act.state(), 3))
            .map(|c| c.zone()),
        Some(ObjectZone::Graveyard(P0)),
        "Channel activation cost must discard the card from hand to graveyard"
    );
    let after_channel = pass_pair(channel_act.state());
    assert_eq!(
        after_channel
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_c1)
            .and_then(|c| c.tapped()),
        Some(true)
    );
    assert_eq!(
        after_channel
            .state()
            .execution()
            .window()
            .cards()
            .get(opp_c2)
            .and_then(|c| c.tapped()),
        Some(true)
    );
}
