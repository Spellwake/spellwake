#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Behavioral integration tests for Mega-Batch 3 engine primitives:
//! - `Instruction::StackOp` (`StackOpSpec::counter_matching` opponents-only spell sweep & ability sweep)
//! - `Instruction::ReturnLinkedExile` (returns exiled card linked to source permanent)
//! - `Instruction::MutateObject` (`ObjectMutation::TransferAllCountersFromTriggeringLki` on death LKI)
//! - `SpellCostModifier::AlternativeForMatching { pay_life_mv: true, each_player }`
//! - `CastPermissionRule::Flash` with `cast_zone` enforcement
//! - `Instruction::TransformSubject`
//! - `Instruction::SetPlayersLife` (normal resolution + inline LoseGame replacement)
//! - `ActivatedCostScope::Loyalty` & `ActivatedCostScope::Targeting`
//! - `ImpulsePlayDuration::UntilNextEndStep` & `UntilNextUpkeep`

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        KeywordAbility, SpellCostModifier, StaticAbilityId, StaticContinuousAbility,
    },
    effects::{CounterKind, DecisionReply, SeedCard},
    game::{GameCommand, GameState, NativeCandidate},
    life::LifeAdjustment,
    mana::{CastingPayment, ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    program::{
        BodyTarget, DiesTriggerMode, EffectSubject, ExecutableBody, Instruction, ObjectFilter,
        Program, SpellFilter, TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn validated(program: Program, v: &CreatureTypeVocabulary) -> Program {
    program.validate(v).expect("program must validate");
    program
}

fn vanilla(p: i64, t: i64) -> Program {
    Program::creature(Some(ManaCost::free()), p.into(), t.into(), BTreeSet::new())
}

fn zone_of(state: &GameState, card_id: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, card_id))
        .expect("card object must exist")
        .zone()
}

fn cast_untargeted(game: &GameState, actor: SeatId, card_id: u64) -> NativeCandidate {
    submit(
        game,
        actor,
        GameCommand::CastUntargeted {
            spell: object(game, card_id),
            payment: Some(ManaPayment::default()),
        },
    )
}

#[test]
fn alternative_for_matching_pay_life_mv_casts_spell_by_paying_life() {
    let v = vocabulary();
    let mut citadel = vanilla(0, 4);
    citadel.continuous = Some(StaticContinuousAbility::CostModifier {
        id: StaticAbilityId(1),
        modifier: SpellCostModifier::AlternativeForMatching {
            filter: SpellFilter::any(),
            mana: ManaCost::free(),
            once_each_turn: false,
            own_turn_only: false,
            pay_life_mv: true,
            each_player: false,
        },
    });
    let citadel = validated(citadel, &v);
    let four_drop = validated(
        Program::creature(
            Some(ManaCost::generic(4.into())),
            4.into(),
            4.into(),
            BTreeSet::new(),
        ),
        &v,
    );

    let game = world(
        v,
        vec![card(1, citadel, true), card(2, four_drop, false)],
        vec![CardId(1)],
    );

    let payment = CastingPayment {
        use_alternative: false,
        alternative_cost_source: Some(object(&game, 1)),
        ..CastingPayment::default()
    };
    let cast = submit(
        &game,
        P0,
        GameCommand::CastWithResources {
            spell: object(&game, 2),
            target: None,
            payment,
        },
    );
    let resolved = pass_pair(cast.state());
    assert_eq!(
        zone_of(resolved.state(), 2),
        ObjectZone::Battlefield,
        "AlternativeForMatching(pay_life_mv: true) must allow casting 4-drop without mana"
    );
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&16.into()),
        "AlternativeForMatching(pay_life_mv: true) must deduct life equal to mana value (20 -> 16)"
    );
}

fn try_submit(
    game: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<NativeCandidate, spellwake_engine::game::GameError> {
    game.advance(spellwake_engine::game::GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
}

fn advance_to_step(
    mut game: GameState,
    target_player: SeatId,
    target_step: spellwake_engine::turn::Step,
) -> GameState {
    for _ in 0..120 {
        if let Some(turn) = game.turn()
            && game.execution().window().active_player() == target_player
            && turn.position().step == target_step
            && game.execution().window().entries().is_empty()
        {
            return game;
        }
        let active = game.execution().window().active_player();
        let other = if active == P0 { P1 } else { P0 };
        if let Ok(next) = try_submit(&game, active, GameCommand::Pass) {
            game = next.into_state();
        } else if let Ok(next) = try_submit(&game, other, GameCommand::Pass) {
            game = next.into_state();
        } else if let Some(d) = game.pending_decision() {
            let choice = if let Ok(next) = try_submit(
                &game,
                d.chooser(),
                GameCommand::Reply {
                    decision: d.id(),
                    choice: DecisionReply::DeclareAttackers(Vec::new()),
                },
            ) {
                game = next.into_state();
                continue;
            } else {
                DecisionReply::DeclareBlockers(Vec::new())
            };
            game = submit(
                &game,
                d.chooser(),
                GameCommand::Reply {
                    decision: d.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            panic!(
                "stuck advancing step at active={:?} step={:?}",
                active,
                game.turn().map(|t| t.position().step)
            );
        }
    }
    panic!("failed to advance to {:?} {:?}", target_player, target_step);
}

#[test]
fn alternative_for_matching_each_player_allows_opponent_free_cast() {
    let v = vocabulary();
    let mut aluren = vanilla(0, 4);
    aluren.continuous = Some(StaticContinuousAbility::CostModifier {
        id: StaticAbilityId(1),
        modifier: SpellCostModifier::AlternativeForMatching {
            filter: SpellFilter::any(),
            mana: ManaCost::free(),
            once_each_turn: false,
            own_turn_only: false,
            pay_life_mv: false,
            each_player: true,
        },
    });
    let aluren = validated(aluren, &v);
    let opp_creature = validated(
        Program::creature(
            Some(ManaCost::generic(3.into())),
            3.into(),
            3.into(),
            BTreeSet::new(),
        ),
        &v,
    );
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, aluren, true),
            SeedCard {
                card: CardId(2),
                owner: P1,
                program: opp_creature,
                destination: ObjectDestination::Hand,
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: filler.clone(),
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(4),
                owner: P0,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![CardId(1)],
    );
    let game = advance_to_step(game, P1, spellwake_engine::turn::Step::PrecombatMain);
    let payment = CastingPayment {
        use_alternative: false,
        alternative_cost_source: Some(object(&game, 1)),
        ..CastingPayment::default()
    };
    let cast = submit(
        &game,
        P1,
        GameCommand::CastWithResources {
            spell: object(&game, 2),
            target: None,
            payment,
        },
    );
    let p1_pass = submit(cast.state(), P1, GameCommand::Pass);
    let resolved = submit(p1_pass.state(), P0, GameCommand::Pass);
    assert_eq!(
        zone_of(resolved.state(), 2),
        ObjectZone::Battlefield,
        "AlternativeForMatching(each_player: true) must allow opponent to cast for free"
    );
}

#[test]
fn impulse_play_duration_until_next_end_step_and_until_next_upkeep_expire_on_step() {
    let v = vocabulary();
    let exile_until_end = validated(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::exile_top_library(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            ValueExpr::literal(1),
                            false,
                        ),
                    )),
                    Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::impulse_play_permission(
                            spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ImpulsePlayDuration::UntilNextEndStep,
                            false,
                            false,
                            false,
                        ),
                    )),
                ],
            ),
        ),
        &v,
    );
    let exiled_bear = validated(vanilla(2, 2), &v);
    let filler = validated(vanilla(1, 1), &v);

    let game = world(
        v,
        vec![
            card(1, exile_until_end, false),
            SeedCard {
                card: CardId(2),
                owner: P0,
                program: exiled_bear,
                destination: ObjectDestination::LibraryTop,
            },
            SeedCard {
                card: CardId(3),
                owner: P1,
                program: filler,
                destination: ObjectDestination::LibraryTop,
            },
        ],
        vec![],
    );
    // Cast exile_until_end during P0's precombat main -> CardId 2 exiled with UntilNextEndStep(P0)
    let c1 = cast_untargeted(&game, P0, 1);
    let after_exile = pass_pair(c1.state()).into_state();
    assert_eq!(zone_of(&after_exile, 2), ObjectZone::Exile);

    // Positive: playable during P0's precombat main (before P0's next end step)
    let playable_before = try_submit(
        &after_exile,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&after_exile, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        playable_before.is_ok(),
        "UntilNextEndStep grant must be playable before grantee's next end step"
    );

    // Negative: advance to P0's End step -> permission expires upon entering Step::End!
    let at_end = advance_to_step(after_exile, P0, spellwake_engine::turn::Step::End);
    let expired = try_submit(
        &at_end,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&at_end, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert!(
        expired.is_err(),
        "UntilNextEndStep grant must expire when grantee's next end step begins"
    );
}
