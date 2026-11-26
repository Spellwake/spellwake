#![forbid(unsafe_code)]
//! Next-spell flash grants expire on the matching successful cast, independently of timing needs.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    actions::TimingError,
    characteristics::{KeywordAbility, ResolvedDuration},
    effects::EffectError,
    game::{GameCommand, GameError, GameInput, GameState, NativeCandidate},
    mana::{ManaCost, ManaPayment},
    program::{
        BodyTarget, ExecutableBody, Instruction, ObjectFilter, Program, ResolvingPlayer,
        TemporaryRuleKind,
    },
    turn::Step,
};
use std::collections::BTreeSet;

fn next_creature_flash(count: usize) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            (0..count)
                .map(|_| Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::FilteredFlash {
                        player: ResolvingPlayer::Controller,
                        filter: ObjectFilter::creature(),
                        next_only: true,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                })
                .collect(),
        ),
    )
}

fn creature(flash: bool, cost: ManaCost) -> Program {
    let mut program = Program::creature(Some(cost), 2.into(), 2.into(), BTreeSet::new());
    if flash {
        program.printed_keywords.insert(KeywordAbility::Flash);
    }
    program
}

fn cast_attempt(game: &GameState, id: u64) -> Result<NativeCandidate, GameError> {
    observe_advance(
        game,
        GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::CastUntargeted {
                spell: object(game, id),
                payment: Some(ManaPayment::default()),
            },
        },
    )
}

fn ready_on_opponents_turn(count: usize, inherent_flash: bool) -> GameState {
    let vocab = vocabulary();
    let cards = vec![
        card(1, next_creature_flash(count), false),
        card(2, creature(inherent_flash, ManaCost::free()), false),
        card(3, creature(false, ManaCost::free()), false),
        card(4, creature(false, ManaCost::generic(1.into())), false),
        library(10, P0, &vocab),
        library(11, P1, &vocab),
    ];
    for card in &cards {
        card.program
            .validate(&vocab)
            .expect("flash fixture program");
    }
    let game = world(vocab, cards, vec![]);
    let end = reach_end(&game, [P0, P1]);
    let upkeep = pass_pair(&end);
    assert_eq!(upkeep.state().turn().unwrap().position().number, 2);
    assert_eq!(upkeep.state().turn().unwrap().position().step, Step::Upkeep);
    let priority = submit(upkeep.state(), P1, GameCommand::Pass);
    let grant = cast_attempt(priority.state(), 1).expect("grant flash on opponent turn");
    let resolved = pass_in_order(grant.state(), [P0, P1]);
    submit(resolved.state(), P1, GameCommand::Pass).into_state()
}

fn assert_consumed_after_first_creature(game: &GameState) {
    let first = cast_attempt(game, 2).expect("first creature has flash permission");
    let resolved = pass_in_order(first.state(), [P0, P1]);
    let priority = submit(resolved.state(), P1, GameCommand::Pass);
    assert_eq!(
        cast_attempt(priority.state(), 3),
        Err(GameError::Effect(EffectError::Timing(
            TimingError::NotActivePlayer
        )))
    );
}

#[test]
fn inherent_flash_still_consumes_next_matching_spell_permission() {
    assert_consumed_after_first_creature(&ready_on_opponents_turn(1, true));
}

#[test]
fn one_cast_consumes_every_matching_next_flash_effect() {
    assert_consumed_after_first_creature(&ready_on_opponents_turn(2, false));
}

#[test]
fn failed_cast_keeps_all_next_flash_effects_and_the_original_state() {
    let game = ready_on_opponents_turn(2, false);
    let before = game.clone();
    assert!(
        cast_attempt(&game, 4).is_err(),
        "one mana is required and no mana is available"
    );
    assert_eq!(game, before, "rejected cast must be atomic");
    assert_consumed_after_first_creature(&game);
}

#[test]
fn normal_sorcery_timing_still_consumes_next_flash_permission() {
    let vocab = vocabulary();
    let game = world(
        vocab.clone(),
        vec![
            card(1, next_creature_flash(1), false),
            card(2, creature(false, ManaCost::free()), false),
            card(3, creature(false, ManaCost::free()), false),
            library(10, P0, &vocab),
            library(11, P1, &vocab),
        ],
        vec![],
    );
    let mut main = game;
    for _ in 0..3 {
        if main.turn().unwrap().position().step == Step::PrecombatMain {
            break;
        }
        main = pass_pair(&main).into_state();
    }
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    let grant = cast_attempt(&main, 1).expect("cast flash grant");
    let resolved = pass_pair(grant.state());
    let first = cast_attempt(resolved.state(), 2).expect("normal main-phase creature cast");
    assert!(
        matches!(
            cast_attempt(first.state(), 3),
            Err(GameError::Effect(EffectError::Timing(_)))
        ),
        "the second creature cannot use a consumed flash grant to be cast onto a nonempty stack"
    );
}
