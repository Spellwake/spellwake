//! Runtime and validation tests for Pass F2-B Wave 1:
//! `StaticReplacementAction::ReplaceWithBody` across draw, life, mill, and scry events
//! (including optional prompts and CR 614.5 non-recursion) plus direct validation fixes.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, PowerAbility, SourceCost},
    characteristics::{
        PermanentSelector, ReplaceableEvent, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts, StaticReplacementAction,
    },
    effects::{CounterKind, OptionalEffectChoice, SeedCard},
    game::{DecisionReply, GameCommand, GameState, NativeCandidate, RuleFrame},
    life::LifeAdjustment,
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::{
        AffectedPlayers, BodyTarget, EntryCounterAbility, ExecutableBody, Instruction,
        ObjectFilter, Program, ResolvingPlayer, ValueExpr, ValueZone,
    },
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

fn library_card(id: u64) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P0,
        destination: ObjectDestination::LibraryBottom,
        program: Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
    }
}

fn replacement_permanent(
    event: ReplaceableEvent,
    optional: bool,
    instructions: Vec<Instruction>,
) -> Program {
    let mut program = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    program.continuous = Some(StaticContinuousAbility::EventReplacement {
        id: StaticAbilityId(1),
        event,
        condition: None,
        action: StaticReplacementAction::ReplaceWithBody {
            optional,
            body: Box::new(ExecutableBody::new(BodyTarget::None, instructions)),
        },
    });
    program
}

#[test]
fn mandatory_draw_body_replacement_executes_inline_without_drawing() {
    let replacer = replacement_permanent(
        ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(3),
        }],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, replacer, true),
            spell(
                2,
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: ValueExpr::literal(1),
                }],
            ),
            library_card(3),
        ],
        vec![CardId(1)],
    );
    let after = cast_untargeted(&game, 2);
    assert!(after.state().pending_decision().is_none());
    assert_eq!(after.state().execution().life_total(P0), Some(&23.into()));
    assert_eq!(
        after.state().execution().window().cards().library(P0).len(),
        1
    );
    assert!(after.draw_journal().events.is_empty());
}

#[test]
fn optional_draw_body_replacement_handles_per_card_take_and_decline() {
    let replacer = replacement_permanent(
        ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        true,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(3),
        }],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, replacer, true),
            spell(
                2,
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: ValueExpr::literal(2),
                }],
            ),
            library_card(3),
            library_card(4),
        ],
        vec![CardId(1)],
    );
    let first_prompt = cast_untargeted(&game, 2);
    assert!(matches!(
        first_prompt.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(frame)) if frame.chooser() == P0
    ));
    // Take on draw 1 -> gain 3 life instead of drawing; draw 2 prompts again (CR 121.2).
    let second_prompt = reply(
        first_prompt.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(
        second_prompt.state().execution().life_total(P0),
        Some(&23.into())
    );
    assert_eq!(
        second_prompt
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        2
    );
    assert!(matches!(
        second_prompt.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(frame)) if frame.chooser() == P0
    ));
    // Decline on draw 2 -> draw 1 card normally.
    let finished = reply(
        second_prompt.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert!(finished.state().pending_decision().is_none());
    assert_eq!(
        finished.state().execution().life_total(P0),
        Some(&23.into())
    );
    assert_eq!(
        finished
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        1
    );
    assert_eq!(finished.draw_journal().events.len(), 1);
}

#[test]
fn draw_body_replacement_enforces_cr_614_5_no_self_reapplication() {
    // Replacement body both gains 2 life and draws 1 card; CR 614.5 prevents self-recursion.
    let replacer = replacement_permanent(
        ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![
            Instruction::GainControllerLife {
                amount: ValueExpr::literal(2),
            },
            Instruction::DrawController {
                amount: ValueExpr::literal(1),
            },
        ],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, replacer, true),
            spell(
                2,
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: ValueExpr::literal(1),
                }],
            ),
            library_card(3),
            library_card(4),
        ],
        vec![CardId(1)],
    );
    let after = cast_untargeted(&game, 2);
    assert!(after.state().pending_decision().is_none());
    assert_eq!(after.state().execution().life_total(P0), Some(&22.into()));
    assert_eq!(
        after.state().execution().window().cards().library(P0).len(),
        1
    );
}

#[test]
fn mandatory_life_gain_body_replacement_draws_instead_of_gaining_life() {
    let replacer = replacement_permanent(
        ReplaceableEvent::LifeGain {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![Instruction::DrawController {
            amount: ValueExpr::literal(1),
        }],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, replacer, true),
            spell(
                2,
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(4.into()),
                }],
            ),
            library_card(3),
        ],
        vec![CardId(1)],
    );
    let after = cast_untargeted(&game, 2);
    assert!(after.state().pending_decision().is_none());
    assert_eq!(after.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        after.state().execution().window().cards().library(P0).len(),
        0
    );
}

#[test]
fn optional_life_gain_body_replacement_supports_take_and_decline() {
    let replacer = replacement_permanent(
        ReplaceableEvent::LifeGain {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        true,
        vec![Instruction::DrawController {
            amount: ValueExpr::literal(1),
        }],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, replacer, true),
            spell(
                2,
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(4.into()),
                }],
            ),
            library_card(3),
        ],
        vec![CardId(1)],
    );
    let prompt = cast_untargeted(&game, 2);
    assert!(matches!(
        prompt.state().pending_decision().map(|d| d.frame()),
        Some(RuleFrame::OptionalEffect(frame)) if frame.chooser() == P0
    ));
    let taken = reply(
        prompt.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert!(taken.state().pending_decision().is_none());
    assert_eq!(taken.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        taken.state().execution().window().cards().library(P0).len(),
        0
    );

    let declined = reply(
        prompt.state(),
        P0,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert!(declined.state().pending_decision().is_none());
    assert_eq!(
        declined.state().execution().life_total(P0),
        Some(&24.into())
    );
    assert_eq!(
        declined
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
fn mandatory_mill_and_scry_body_replacements_execute_inline() {
    let mill_rep = replacement_permanent(
        ReplaceableEvent::Mill {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(5),
        }],
    );
    let scry_rep = replacement_permanent(
        ReplaceableEvent::Scry {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![Instruction::GainControllerLife {
            amount: ValueExpr::literal(7),
        }],
    );
    let game = world(
        vocabulary(),
        vec![
            card(1, mill_rep, true),
            card(2, scry_rep, true),
            spell(
                3,
                BodyTarget::None,
                vec![Instruction::Mill {
                    player: ResolvingPlayer::Controller,
                    amount: ValueExpr::literal(2),
                }],
            ),
            spell(
                4,
                BodyTarget::None,
                vec![Instruction::Scry {
                    player: ResolvingPlayer::Controller,
                    amount: ValueExpr::literal(2),
                }],
            ),
            library_card(5),
            library_card(6),
        ],
        vec![CardId(1), CardId(2)],
    );
    let after_mill = cast_untargeted(&game, 3);
    assert!(after_mill.state().pending_decision().is_none());
    assert_eq!(
        after_mill.state().execution().life_total(P0),
        Some(&25.into())
    );
    assert_eq!(
        after_mill
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        2
    );
    let after_scry = cast_untargeted(after_mill.state(), 4);
    assert!(after_scry.state().pending_decision().is_none());
    assert_eq!(
        after_scry.state().execution().life_total(P0),
        Some(&32.into())
    );
    assert_eq!(
        after_scry
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        2
    );
}

#[test]
fn wave1_validation_fixes_and_dynamic_entry_counter_other_than_source() {
    let vocab = vocabulary();
    let army = vocab.resolve_label("Army").unwrap();
    let orc = vocab.resolve_label("Orc").unwrap();

    // 1. Targeted spell with CreateArmyIfAbsent + Amass validates cleanly.
    let targeted_amass = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::AffectPermanentTargets {
                    minimum: 1,
                    maximum: 1,
                    action: spellwake_engine::program::PermanentTargetAction::Tap,
                },
                Instruction::CreateArmyIfAbsent { army, subtype: orc },
                Instruction::Amass {
                    army,
                    subtype: orc,
                    amount: 2,
                },
            ],
        ),
    );
    assert!(targeted_amass.validate(&vocab).is_ok());

    // 2. GrantActivated with SourceCost::Power validates cleanly.
    let mut crew_granter = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    crew_granter.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: [spellwake_engine::program::CardType::Creature].into(),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantActivated(Box::new(
            ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Power {
                    kind: PowerAbility::Crew,
                    minimum: 2,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            },
        ))]),
    });
    assert!(crew_granter.validate(&vocab).is_ok());

    // 3. ReplaceWithBody rejects unsupported inline instructions at validation time.
    let invalid_rep = replacement_permanent(
        ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        false,
        vec![Instruction::Proliferate],
    );
    assert!(invalid_rep.validate(&vocab).is_err());

    // 4. DynamicCounter referencing other_than_ability_source validates AND evaluates at entry.
    let mut entering =
        Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());
    entering
        .entry_counter_abilities
        .push(EntryCounterAbility::DynamicCounter {
            kind: CounterKind::PlusOne,
            amount: ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(
                    ObjectFilter::creature()
                        .controlled()
                        .other_than_ability_source(),
                ),
                controller_scope: None,
            },
        });
    assert!(entering.validate(&vocab).is_ok());

    let game = world(
        vocab,
        vec![
            card(
                1,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
                true,
            ),
            card(
                2,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
                true,
            ),
            card(3, entering, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let entered = cast_untargeted(&game, 3);
    let entered_id = object(entered.state(), 3);
    assert_eq!(
        entered
            .state()
            .execution()
            .window()
            .cards()
            .get(entered_id)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    let counters = entered
        .state()
        .execution()
        .power_toughness_counters(entered_id)
        .unwrap();
    assert_eq!(counters.plus_one, 2.into());
}
