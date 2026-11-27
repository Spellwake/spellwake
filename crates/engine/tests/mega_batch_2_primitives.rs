#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Behavioral integration tests for Mega-Batch 2 engine primitives:
//! - `Instruction::EmpowerJace` (creates blue Jace planeswalker token + adds loyalty; reuses existing Jace token)
//! - `Instruction::TurnFaceDownSubject` (turns nontoken single-faced permanent face down; skips DFCs per CR 712.16)
//! - `Instruction::AddOneOfEachCounterKindOnSubject` (adds 1 of each counter kind present; no-op when empty)
//! - `DamageRecipientsRef::EachSourceToItself` and `EachSourceController`
//! - `DamageTargetQuery::Divided` (dynamic X shares and `max_targets` cap; atomic invalid-share rejection)
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
        LookTopRestDest, ObjectFilter, PermanentDefinition, PermanentTargetAction,
        PlaneswalkerSubtype, PlayerRelation, PlayerTriggerAction, Program, ProgramKind,
        ResolvingPlayer, SourceZoneFilter, SpellFilter, TemporaryRuleKind, TriggerEvent,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId, ValueExpr, ValueZone,
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
fn divide_damage_expr_and_up_to_x_target_count_enforce_bounds() {
    let v = vocabulary();
    let target_a = validated(vanilla(1, 5), &v);
    let target_b = validated(vanilla(1, 5), &v);
    let target_c = validated(vanilla(1, 5), &v);

    let mut divide_x = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec::divided(ValueExpr::chosen_x(), Some(2)),
            ))],
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
            vec![Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_triggering(),
            ))],
        ),
    });
    stifle_perm
        .validate(&v)
        .expect("StackOp::counter_triggering must validate on AbilityActivated triggers");

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
                    reveal_all: false,
                    reveal_only: false,
                    secondary: None,
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
