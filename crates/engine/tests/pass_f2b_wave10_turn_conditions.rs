#![forbid(unsafe_code)]
//! Trigger-time and resolution-time evaluation of a related player's turn.

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    effects::SeedCard,
    game::GameCommand,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination},
    program::{
        BodyTarget, Comparison, ExecutableBody, Instruction, PlayerRelation, PlayerStatExpr,
        Program, ResolutionCondition, ResolvingPlayer, SpellFilter, TriggerCondition, TriggerEvent,
        TriggerGrouping, TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr,
    },
};
use trajectories::*;

fn related_player_is_active() -> ValueExpr {
    ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(
        ResolvingPlayer::UniqueRelatedPlayer,
    ))
}

#[test]
fn related_players_turn_is_checked_at_capture_and_resolution() {
    for actor in [P0, P1] {
        for expected in [0, 1] {
            let vocab = vocabulary();
            let mut watcher = Program::enchantment(None);
            let mut condition = TriggerSpec::new(TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
                filter: SpellFilter::any(),
                self_cast: false,
                include_copy: false,
                min_mana_value: None,
            });
            condition.intervening_if =
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: related_player_is_active(),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(expected),
                })));
            watcher.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition,
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::Add(
                            Box::new(ValueExpr::literal(3)),
                            Box::new(related_player_is_active()),
                        ),
                    }],
                ),
            });
            watcher
                .validate(&vocab)
                .expect("related player supplied by cast event");
            let castable = Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(0),
                    }],
                ),
            );
            let game = world(
                vocab,
                vec![
                    card(1, watcher, true),
                    SeedCard {
                        card: CardId(2),
                        owner: actor,
                        destination: ObjectDestination::Hand,
                        program: castable,
                    },
                ],
                vec![CardId(1)],
            );
            let ready = if actor == P1 {
                submit(&game, P0, GameCommand::Pass).into_state()
            } else {
                game
            };
            let cast = submit(
                &ready,
                actor,
                GameCommand::CastUntargeted {
                    spell: object(&ready, 2),
                    payment: Some(ManaPayment::default()),
                },
            );
            let matches = i64::from(actor == P0) == expected;
            assert_eq!(
                cast.state().execution().window().entries().len(),
                if matches { 2 } else { 1 }
            );
            if matches {
                let resolved =
                    pass_in_order(cast.state(), [actor, if actor == P0 { P1 } else { P0 }]);
                assert_eq!(
                    resolved.state().execution().life_total(P0),
                    Some(&(23 + expected).into())
                );
            }
        }
    }
}

#[test]
fn player_turn_expression_rejects_missing_related_or_target_context() {
    let vocab = vocabulary();
    for role in [
        ResolvingPlayer::UniqueRelatedPlayer,
        ResolvingPlayer::TargetPlayer,
    ] {
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::GainControllerLife {
                    amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(role)),
                }],
            ),
        );
        assert!(
            program.validate(&vocab).is_err(),
            "missing role {role:?} must fail validation"
        );
    }
}
