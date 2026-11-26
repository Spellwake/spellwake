#![forbid(unsafe_code)]
//! Distinct current-power counting and intervening-if rechecks for coven.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    characteristics::{ResolvedDuration, ResolvedPartSpec},
    effects::SeedCard,
    game::{GameCommand, GameState, NativeCandidate},
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination},
    program::{
        BodyTarget, Comparison, ExecutableBody, Instruction, ObjectFilter, ObjectStatKind,
        PlayerRelation, Program, ResolutionCondition, SpellFilter, StatAggregate, TriggerCondition,
        TriggerEvent, TriggerGrouping, TriggerPlayerScope, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

fn distinct_powers(filter: ObjectFilter) -> ValueExpr {
    ValueExpr::aggregate(filter, ObjectStatKind::Power, StatAggregate::CountDistinct)
}

fn count_spell(id: u64, filter: ObjectFilter) -> SeedCard {
    spell(
        id,
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: distinct_powers(filter),
        }],
    )
}

fn creature(power: RulesInteger) -> Program {
    Program::creature(None, power, 5.into(), BTreeSet::new())
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

fn cast_pump(game: &GameState, id: u64, target: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, id),
            target: TargetRef::Object(object(game, target)),
            payment: ManaPayment::default(),
        },
    );
    pass_pair(cast.state())
}

#[test]
fn distinct_power_counts_exact_signed_values_and_current_layered_power() {
    let huge = RulesInteger::from(i64::MAX).times(&RulesInteger::from(i64::MAX));
    let filter = ObjectFilter::creature().controlled();
    let game = world(
        vocabulary(),
        vec![
            card(1, creature((-2).into()), true),
            card(2, creature((-2).into()), true),
            card(3, creature(0.into()), true),
            card(4, creature(2.into()), true),
            card(5, creature(huge), true),
            controlled(6, P1, P1, creature(9.into())),
            card(7, Program::enchantment(None), true),
            count_spell(10, filter),
            count_spell(11, filter),
            spell(
                12,
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(2, 0))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        (1..=7).map(CardId).collect(),
    );
    let initial = cast_untargeted(&game, 10);
    assert_eq!(initial.state().execution().life_total(P0), Some(&24.into()));
    let changed = cast_pump(initial.state(), 12, 3);
    assert_eq!(
        current(changed.state(), object(changed.state(), 3))
            .power_toughness
            .unwrap()
            .derived
            .power,
        2.into()
    );
    let recounted = cast_untargeted(changed.state(), 11);
    assert_eq!(
        recounted.state().execution().life_total(P0),
        Some(&27.into())
    );
}

#[test]
fn distinct_power_omits_objects_without_power_and_counts_empty_as_zero() {
    let game = world(
        vocabulary(),
        vec![
            card(1, Program::enchantment(None), true),
            count_spell(10, ObjectFilter::any()),
        ],
        vec![CardId(1)],
    );
    let counted = cast_untargeted(&game, 10);
    assert_eq!(counted.state().execution().life_total(P0), Some(&20.into()));
}

#[test]
fn coven_checks_current_power_on_capture_and_again_when_trigger_resolves() {
    for collapse_before_capture in [false, true] {
        for collapse_before_resolution in [false, true] {
            let vocab = vocabulary();
            let mut watcher = Program::enchantment(None);
            let mut condition = TriggerSpec::new(TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                filter: SpellFilter::any(),
                self_cast: false,
                include_copy: false,
                min_mana_value: None,
            });
            condition.intervening_if =
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: distinct_powers(ObjectFilter::creature().controlled()),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(3),
                })));
            watcher.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition,
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(7),
                    }],
                ),
            });
            watcher.validate(&vocab).unwrap();
            let game = world(
                vocab,
                vec![
                    card(1, watcher, true),
                    card(2, creature(1.into()), true),
                    card(3, creature(2.into()), true),
                    card(4, creature(3.into()), true),
                    spell(
                        10,
                        BodyTarget::SingleCreature,
                        vec![effect(
                            vec![ResolvedPartSpec::ModifyPT(pt(1, 0))],
                            ResolvedDuration::UntilEndOfTurn,
                        )],
                    ),
                    SeedCard {
                        card: CardId(11),
                        owner: P1,
                        destination: ObjectDestination::Hand,
                        program: Program::instant(
                            ManaCost::free(),
                            ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::GainControllerLife {
                                    amount: ValueExpr::literal(0),
                                }],
                            ),
                        ),
                    },
                ],
                vec![CardId(1), CardId(2), CardId(3), CardId(4)],
            );
            let ready = if collapse_before_capture {
                cast_pump(&game, 10, 2).into_state()
            } else {
                game
            };
            let priority = submit(&ready, P0, GameCommand::Pass);
            let cast = submit(
                priority.state(),
                P1,
                GameCommand::CastUntargeted {
                    spell: object(priority.state(), 11),
                    payment: Some(ManaPayment::default()),
                },
            );
            assert_eq!(
                cast.state().execution().window().entries().len(),
                if collapse_before_capture { 1 } else { 2 }
            );
            if !collapse_before_capture {
                let before_resolution = if collapse_before_resolution {
                    let priority = submit(cast.state(), P1, GameCommand::Pass);
                    cast_pump(priority.state(), 10, 2).into_state()
                } else {
                    submit(cast.state(), P1, GameCommand::Pass).into_state()
                };
                // After the opponent passes without a response, P0 holds priority; the
                // first pass completes that round. A response starts a fresh round.
                let resolved = if collapse_before_resolution {
                    pass_pair(&before_resolution)
                } else {
                    submit(&before_resolution, P0, GameCommand::Pass)
                };
                assert_eq!(
                    resolved.state().execution().life_total(P0),
                    Some(&(if collapse_before_resolution { 20 } else { 27 }).into())
                );
            }
        }
    }
}
