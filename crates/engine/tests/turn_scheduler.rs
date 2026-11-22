//! Native turn control tests over authored kernel fixtures. Missing prepared
//! combat history suspends; ordinary cleanup has exact discard decisions.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate,
    },
    mana::{FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{
        Phase, PriorityCompleted, Step, TurnError, TurnPosition, TurnScheduler, TurnStage,
        TurnStart, TurnWork, WorkCompleted,
    },
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(91);

fn start(number: u64, step: Step) -> TurnStart {
    TurnStart { number, step }
}
fn schedule(number: u64, step: Step, skip_first_draw: bool) -> TurnScheduler {
    TurnScheduler::new(start(number, step), skip_first_draw).unwrap()
}
fn ordinary(state: &TurnScheduler) -> TurnScheduler {
    state
        .complete_priority(PriorityCompleted::Ordinary)
        .unwrap()
}
fn assert_position(state: &TurnScheduler, number: u64, step: Step, stage: TurnStage) {
    assert_eq!(state.position().number, number);
    assert_eq!(state.position().step, step);
    assert_eq!(state.stage(), stage);
}

#[test]
fn every_position_derives_its_phase() {
    for (phase, steps) in [
        (
            Phase::Beginning,
            vec![Step::Untap, Step::Upkeep, Step::Draw],
        ),
        (Phase::PrecombatMain, vec![Step::PrecombatMain]),
        (
            Phase::Combat,
            vec![
                Step::BeginningCombat,
                Step::DeclareAttackers,
                Step::DeclareBlockers,
                Step::CombatDamage,
                Step::EndCombat,
            ],
        ),
        (Phase::PostcombatMain, vec![Step::PostcombatMain]),
        (Phase::Ending, vec![Step::End, Step::Cleanup]),
    ] {
        for step in steps {
            assert_eq!(step.phase(), phase);
        }
    }
}

#[test]
fn mandatory_work_and_priority_walk_one_complete_normal_turn() {
    let mut state = TurnScheduler::new(TurnStart::default(), false).unwrap();
    assert_position(&state, 1, Step::Untap, TurnStage::Work(TurnWork::Untap));
    state = state.complete_work(WorkCompleted::Untap).unwrap();
    assert_position(&state, 1, Step::Upkeep, TurnStage::Priority);
    state = ordinary(&state);
    assert_position(&state, 1, Step::Draw, TurnStage::Work(TurnWork::Draw));
    state = state.complete_work(WorkCompleted::Draw).unwrap();
    assert_position(&state, 1, Step::Draw, TurnStage::Priority);
    for step in [Step::PrecombatMain, Step::BeginningCombat] {
        state = ordinary(&state);
        assert_position(&state, 1, step, TurnStage::Priority);
    }
    state = ordinary(&state);
    assert_position(
        &state,
        1,
        Step::DeclareAttackers,
        TurnStage::Work(TurnWork::DeclareAttackers),
    );
    state = state
        .complete_work(WorkCompleted::DeclareAttackers)
        .unwrap();
    assert_position(&state, 1, Step::DeclareAttackers, TurnStage::Priority);
    state = state
        .complete_priority(PriorityCompleted::Attackers {
            declared_or_put_attacking: true,
        })
        .unwrap();
    assert_position(
        &state,
        1,
        Step::DeclareBlockers,
        TurnStage::Work(TurnWork::DeclareBlockers),
    );
    state = state.complete_work(WorkCompleted::DeclareBlockers).unwrap();
    assert_position(&state, 1, Step::DeclareBlockers, TurnStage::Priority);
    state = ordinary(&state);
    assert_position(
        &state,
        1,
        Step::CombatDamage,
        TurnStage::Work(TurnWork::CombatDamage { additional: false }),
    );
    state = state
        .complete_work(WorkCompleted::CombatDamage {
            additional_step_required: false,
        })
        .unwrap();
    for step in [Step::EndCombat, Step::PostcombatMain, Step::End] {
        state = ordinary(&state);
        assert_position(&state, 1, step, TurnStage::Priority);
    }
    state = ordinary(&state);
    assert_position(&state, 1, Step::Cleanup, TurnStage::Work(TurnWork::Cleanup));
    state = state
        .complete_work(WorkCompleted::Cleanup {
            priority_required: false,
        })
        .unwrap();
    assert_position(&state, 2, Step::Untap, TurnStage::Work(TurnWork::Untap));
    assert_eq!(state.position().cleanup_iteration, 0);
    assert!(!state.position().additional_damage);
}

#[test]
fn two_player_first_draw_omission_only_applies_to_turn_one() {
    let initial = schedule(1, Step::Upkeep, true);
    assert_eq!(ordinary(&initial).position().step, Step::PrecombatMain);
    for (number, skip) in [(1, false), (2, true), (2, false)] {
        assert_position(
            &ordinary(&schedule(number, Step::Upkeep, skip)),
            number,
            Step::Draw,
            TurnStage::Work(TurnWork::Draw),
        );
    }
    assert_eq!(
        TurnScheduler::new(start(1, Step::Draw), true),
        Err(TurnError::InvalidStart)
    );
    assert_eq!(
        TurnScheduler::new(start(0, Step::Untap), false),
        Err(TurnError::InvalidStart)
    );
}

#[test]
fn no_attacker_skip_is_decided_after_attackers_priority_from_attack_history() {
    let declarations = schedule(8, Step::DeclareAttackers, false);
    let priority = declarations
        .complete_work(WorkCompleted::DeclareAttackers)
        .unwrap();
    assert_position(&priority, 8, Step::DeclareAttackers, TurnStage::Priority);
    assert_eq!(
        priority.complete_priority(PriorityCompleted::Ordinary),
        Err(TurnError::WrongPriorityOutcome)
    );
    let no_attackers = priority
        .complete_priority(PriorityCompleted::Attackers {
            declared_or_put_attacking: false,
        })
        .unwrap();
    assert_position(&no_attackers, 8, Step::EndCombat, TurnStage::Priority);
    // A trusted true outcome includes a creature put onto the battlefield attacking
    // during the priority round, even if it has subsequently left combat.
    let history = priority
        .complete_priority(PriorityCompleted::Attackers {
            declared_or_put_attacking: true,
        })
        .unwrap();
    assert_position(
        &history,
        8,
        Step::DeclareBlockers,
        TurnStage::Work(TurnWork::DeclareBlockers),
    );
}

#[test]
fn additional_damage_is_determined_by_initial_damage_work_after_blockers_priority() {
    let blockers = schedule(3, Step::DeclareBlockers, false)
        .complete_work(WorkCompleted::DeclareBlockers)
        .unwrap();
    assert_position(&blockers, 3, Step::DeclareBlockers, TurnStage::Priority);
    assert_eq!(
        blockers.complete_work(WorkCompleted::CombatDamage {
            additional_step_required: true,
        }),
        Err(TurnError::PriorityRequired)
    );
    let initial = ordinary(&blockers);
    assert_position(
        &initial,
        3,
        Step::CombatDamage,
        TurnStage::Work(TurnWork::CombatDamage { additional: false }),
    );
    for additional_step_required in [false, true] {
        let first_priority = initial
            .complete_work(WorkCompleted::CombatDamage {
                additional_step_required,
            })
            .unwrap();
        assert_position(&first_priority, 3, Step::CombatDamage, TurnStage::Priority);
        assert!(!first_priority.position().additional_damage);
        let next = ordinary(&first_priority);
        if additional_step_required {
            assert_position(
                &next,
                3,
                Step::CombatDamage,
                TurnStage::Work(TurnWork::CombatDamage { additional: true }),
            );
            assert!(next.position().additional_damage);
            let saved = next.clone();
            assert_eq!(
                next.complete_work(WorkCompleted::CombatDamage {
                    additional_step_required: true,
                }),
                Err(TurnError::WrongWork)
            );
            assert_eq!(next, saved);
            let second_priority = next
                .complete_work(WorkCompleted::CombatDamage {
                    additional_step_required: false,
                })
                .unwrap();
            assert_eq!(second_priority.stage(), TurnStage::Priority);
            let end = ordinary(&second_priority);
            assert_position(&end, 3, Step::EndCombat, TurnStage::Priority);
            assert!(!end.position().additional_damage);
        } else {
            assert_position(&next, 3, Step::EndCombat, TurnStage::Priority);
        }
    }
}

#[test]
fn cleanup_exception_priority_repeats_cleanup_until_no_more_work_requires_priority() {
    let mut cleanup = schedule(12, Step::Cleanup, false);
    for iteration in 0..3 {
        assert_eq!(cleanup.position().cleanup_iteration, iteration);
        assert_eq!(cleanup.stage(), TurnStage::Work(TurnWork::Cleanup));
        let priority = cleanup
            .complete_work(WorkCompleted::Cleanup {
                priority_required: true,
            })
            .unwrap();
        assert_eq!(priority.position(), cleanup.position());
        assert_eq!(priority.stage(), TurnStage::Priority);
        cleanup = ordinary(&priority);
        assert_position(
            &cleanup,
            12,
            Step::Cleanup,
            TurnStage::Work(TurnWork::Cleanup),
        );
    }
    let next = cleanup
        .complete_work(WorkCompleted::Cleanup {
            priority_required: false,
        })
        .unwrap();
    assert_position(&next, 13, Step::Untap, TurnStage::Work(TurnWork::Untap));
    assert_eq!(next.position().cleanup_iteration, 0);
}

#[test]
fn wrong_work_and_priority_outcomes_leave_the_input_scheduler_unchanged() {
    let results = [
        WorkCompleted::Untap,
        WorkCompleted::Draw,
        WorkCompleted::DeclareAttackers,
        WorkCompleted::DeclareBlockers,
        WorkCompleted::CombatDamage {
            additional_step_required: false,
        },
        WorkCompleted::Cleanup {
            priority_required: false,
        },
    ];
    for (index, step) in [
        Step::Untap,
        Step::Draw,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
        Step::CombatDamage,
        Step::Cleanup,
    ]
    .into_iter()
    .enumerate()
    {
        let state = schedule(2, step, false);
        let saved = state.clone();
        for (result_index, result) in results.into_iter().enumerate() {
            if result_index != index {
                assert_eq!(state.complete_work(result), Err(TurnError::WrongWork));
                assert_eq!(state, saved);
            }
        }
        for outcome in [
            PriorityCompleted::Ordinary,
            PriorityCompleted::Attackers {
                declared_or_put_attacking: true,
            },
        ] {
            assert_eq!(
                state.complete_priority(outcome),
                Err(TurnError::WorkRequired)
            );
            assert_eq!(state, saved);
        }
    }
    let priority = schedule(2, Step::Upkeep, false);
    let saved = priority.clone();
    for result in results {
        assert_eq!(
            priority.complete_work(result),
            Err(TurnError::PriorityRequired)
        );
        assert_eq!(priority, saved);
    }
    assert_eq!(
        priority.complete_priority(PriorityCompleted::Attackers {
            declared_or_put_attacking: false,
        }),
        Err(TurnError::WrongPriorityOutcome)
    );
    assert_eq!(priority, saved);
}

#[test]
fn exhausting_turn_number_does_not_partially_complete_cleanup() {
    let cleanup = schedule(u64::MAX, Step::Cleanup, false);
    let saved = cleanup.clone();
    assert_eq!(
        cleanup.complete_work(WorkCompleted::Cleanup {
            priority_required: false,
        }),
        Err(TurnError::TurnExhausted)
    );
    assert_eq!(cleanup, saved);
    assert_eq!(
        cleanup
            .complete_work(WorkCompleted::Cleanup {
                priority_required: true,
            })
            .unwrap()
            .position()
            .number,
        u64::MAX
    );
}

fn context() -> GameContext {
    GameContext::new(
        "native-turn-test".into(),
        "rules".into(),
        "cards".into(),
        "programs".into(),
        "restricted-turn-scheduler".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn setup(
    seats: Vec<SeatId>,
    active: SeatId,
    cards: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
    start: TurnStart,
) -> NativeCandidate {
    GameState::initialize_scheduled(
        GameId(142),
        context(),
        {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                seats,
                active,
                cards,
                initial_battlefield_order,
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        start,
    )
    .unwrap()
}
fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}
fn accept(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = game.clone();
    let candidate = game.advance(input(game, actor, command)).unwrap();
    assert_eq!(game, &saved);
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    assert_eq!(
        candidate.state().basis().revision,
        game.basis().revision + 1
    );
    assert_eq!(candidate.state().basis().game, game.basis().game);
    assert_eq!(candidate.state().basis().context, game.basis().context);
    assert_eq!(candidate.state().basis().epoch, game.basis().epoch);
    candidate
}
fn holder(game: &GameState) -> SeatId {
    match game.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
        phase => panic!("expected ready priority, got {phase:?}"),
    }
}
fn pass(game: &GameState) -> NativeCandidate {
    accept(game, holder(game), GameCommand::Pass)
}
fn pass_round(game: &GameState) -> NativeCandidate {
    let count = game.execution().window().participants().len();
    let mut state = game.clone();
    for _ in 1..count {
        let next = pass(&state);
        assert!(next.turn_transitions().is_empty());
        state = next.into_state();
    }
    pass(&state)
}
fn position(game: &GameState) -> TurnPosition {
    game.turn().unwrap().position()
}
fn creature(card: u64, toughness: i64) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(
            None,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn instant(card: u64, owner: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 3.into() }],
            ),
        ),
    }
}
fn land(card: u64, owner: SeatId, controller: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(7),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 2.into())])),
            }],
        ),
    }
}
fn pools(seats: &[SeatId]) -> Vec<FloatingMana> {
    seats
        .iter()
        .enumerate()
        .flat_map(|(seat_index, &player)| {
            ManaType::ALL
                .into_iter()
                .enumerate()
                .map(move |(type_index, mana_type)| FloatingMana {
                    player,
                    mana_type,
                    amount: (seat_index as i64 * 10 + type_index as i64 + 1).into(),
                })
        })
        .collect()
}
fn balances(game: &GameState) -> Vec<RulesInteger> {
    game.execution()
        .window()
        .participants()
        .iter()
        .flat_map(|&player| ManaType::ALL.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}
fn assert_empty_pools(game: &GameState) {
    assert!(balances(game).iter().all(|amount| *amount == 0.into()));
}

#[test]
fn native_first_turn_skips_two_player_draw_and_executes_multiplayer_draw() {
    for seats in [vec![A, B], vec![A, B, C]] {
        let mut draw = instant(500, A);
        draw.destination = ObjectDestination::LibraryBottom;
        let initial = setup(seats.clone(), A, vec![draw], vec![], TurnStart::default());
        assert_eq!(initial.turn_transitions().len(), 2);
        assert_eq!(initial.turn_transitions()[0].from, None);
        assert_eq!(initial.turn_transitions()[0].to.step, Step::Untap);
        assert_eq!(initial.turn_transitions()[1].to.step, Step::Upkeep);
        let state = initial.into_state();
        assert_eq!(position(&state).step, Step::Upkeep);
        assert_eq!(holder(&state), A);
        assert_eq!(state.pending_turn_work(), None);
        let candidate = pass_round(&state);
        assert_eq!(candidate.turn_transitions().len(), 1);
        let next = candidate.into_state();
        if seats.len() == 2 {
            assert_eq!(position(&next).step, Step::PrecombatMain);
            assert_eq!(next.pending_turn_work(), None);
            assert_eq!(holder(&next), A);
        } else {
            assert_eq!(position(&next).step, Step::Draw);
            assert_eq!(next.pending_turn_work(), None);
            assert_eq!(holder(&next), A);
            assert_eq!(
                next.execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|c| c.zone() == ObjectZone::Hand(A))
                    .count(),
                1
            );
        }
        assert!(next.pending_decision().is_none());
        assert_eq!(next.execution().window().active_player(), A);
    }
    let mut draw = instant(501, B);
    draw.destination = ObjectDestination::LibraryBottom;
    let next_turn = setup(vec![A, B], B, vec![draw], vec![], start(2, Step::Upkeep)).into_state();
    let drawn = pass_round(&next_turn);
    assert_eq!(drawn.draw_journal().events.len(), 1);
    assert_eq!(holder(drawn.state()), B);
    assert_eq!(drawn.state().pending_turn_work(), None);
}

#[test]
fn ordinary_commands_cannot_bypass_mandatory_turn_work() {
    let artifact = SeedCard {
        card: CardId(3),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(7),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            }],
        ),
    };
    let cards = vec![land(1, A, A), instant(2, A), artifact, creature(99, 100)];
    for (step, work) in [
        (Step::DeclareBlockers, TurnWork::DeclareBlockers),
        (
            Step::CombatDamage,
            TurnWork::CombatDamage { additional: false },
        ),
        (Step::Cleanup, TurnWork::Cleanup),
    ] {
        let mut prepared = cards.clone();
        if step == Step::Cleanup {
            prepared.extend((4..=10).map(|card| instant(card, A)));
        }
        let game = setup(vec![A, B], A, prepared, pools(&[A, B]), start(2, step)).into_state();
        let target = TargetRef::Object(object(&game, 99));
        let commands = [
            GameCommand::Pass,
            GameCommand::ActivateMana {
                source: object(&game, 1),
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            },
            GameCommand::ActivateAbility {
                source: object(&game, 3),
                ability: ActivatedAbilityId(7),
                target,
                payment: None,
            },
            GameCommand::Cast {
                spell: object(&game, 2),
                target,
            },
            GameCommand::CastPaid {
                spell: object(&game, 2),
                target,
                payment: ManaPayment::default(),
            },
            GameCommand::Reply {
                decision: DecisionId(1),
                choice: DecisionReply::ManaColor(ManaType::Red),
            },
        ];
        let saved = game.clone();
        assert_eq!(game.pending_turn_work(), Some(work));
        assert_eq!(game.execution().window().phase(), WindowPhase::TurnWork);
        for actor in [A, B] {
            for command in commands.clone() {
                let error =
                    if work == TurnWork::Cleanup && matches!(command, GameCommand::Reply { .. }) {
                        if actor == A {
                            GameError::Effect(EffectError::WrongDecisionKind)
                        } else {
                            GameError::WrongChooser
                        }
                    } else {
                        GameError::TurnWorkRequired(work)
                    };
                assert_eq!(game.advance(input(&game, actor, command)), Err(error));
                assert_eq!(game, saved);
            }
        }
    }
}

#[test]
fn mana_survives_passing_and_real_resolution_then_clears_with_damage_at_step_end() {
    let game = setup(
        vec![A, B],
        A,
        vec![instant(1, A), instant(2, B), creature(99, 100)],
        pools(&[A, B]),
        start(7, Step::End),
    )
    .into_state();
    let before = balances(&game);
    let target = object(&game, 99);
    let cast = accept(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(target),
        },
    );
    assert!(cast.turn_transitions().is_empty());
    assert_eq!(balances(cast.state()), before);
    let original_entry = cast.state().execution().window().entries()[0].id();
    let first_pass = pass(cast.state());
    assert!(first_pass.turn_transitions().is_empty());
    assert_eq!(balances(first_pass.state()), before);
    let resolved = pass(first_pass.state());
    assert!(resolved.turn_transitions().is_empty());
    assert_eq!(position(resolved.state()).step, Step::End);
    assert_eq!(balances(resolved.state()), before);
    assert!(resolved.state().execution().window().entries().is_empty());
    assert_eq!(
        resolved
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );
    let boundary = pass_round(resolved.state());
    assert_eq!(
        boundary
            .turn_transitions()
            .iter()
            .map(|transition| transition.to.step)
            .collect::<Vec<_>>(),
        vec![Step::Cleanup, Step::Untap, Step::Upkeep]
    );
    assert_empty_pools(boundary.state());
    assert_eq!(
        boundary
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    assert_eq!(object(boundary.state(), 99), target);
    assert_eq!(position(boundary.state()).number, 8);
    assert_eq!(boundary.state().execution().window().active_player(), B);
    let later = accept(
        boundary.state(),
        B,
        GameCommand::Cast {
            spell: object(boundary.state(), 2),
            target: TargetRef::Object(target),
        },
    );
    assert!(later.turn_transitions().is_empty());
    assert!(later.state().execution().window().entries()[0].id() > original_entry);
}

#[test]
fn turn_wrap_rotates_the_canonical_active_seat_in_table_order() {
    let seats = vec![C, A, B];
    for (index, &active) in seats.iter().enumerate() {
        let game = setup(
            seats.clone(),
            active,
            vec![],
            pools(&seats),
            start(20, Step::End),
        )
        .into_state();
        let candidate = pass_round(&game);
        let next_active = seats[(index + 1) % seats.len()];
        let records = candidate.turn_transitions();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].from.unwrap().step, Step::End);
        assert_eq!(records[0].to.step, Step::Cleanup);
        assert_eq!(
            (records[0].active_before, records[0].active_after),
            (active, active)
        );
        assert_eq!(records[1].to.step, Step::Untap);
        assert_eq!(
            (records[1].active_before, records[1].active_after),
            (active, next_active)
        );
        assert_eq!(records[2].to.step, Step::Upkeep);
        assert_eq!(
            (records[2].active_before, records[2].active_after),
            (next_active, next_active)
        );
        let next = candidate.into_state();
        assert_eq!(next.execution().window().participants(), seats);
        assert_eq!(next.execution().window().active_player(), next_active);
        assert_eq!(holder(&next), next_active);
        assert_eq!(position(&next).number, 21);
        assert_empty_pools(&next);
    }
}

#[test]
fn resolution_after_turn_wrap_returns_priority_to_the_new_active_player() {
    let initial = setup(
        vec![A, B, C],
        A,
        vec![instant(1, C), creature(99, 100)],
        vec![],
        start(20, Step::End),
    )
    .into_state();
    let next = pass_round(&initial).into_state();
    assert_eq!(holder(&next), B);
    let next = pass(&next).into_state();
    assert_eq!(holder(&next), C);
    let cast = accept(
        &next,
        C,
        GameCommand::Cast {
            spell: object(&next, 1),
            target: TargetRef::Object(object(&next, 99)),
        },
    );
    let entry = cast.state().execution().window().entries()[0].id();
    let resolved = pass_round(cast.state());
    assert!(resolved.turn_transitions().is_empty());
    assert_eq!(position(resolved.state()).step, Step::Upkeep);
    assert_eq!(resolved.state().execution().window().active_player(), B);
    assert_eq!(holder(resolved.state()), B);
    assert!(
        resolved
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .all(|item| item.id() != entry)
    );
}

#[test]
fn next_untap_changes_actual_tapped_objects_by_controller_and_preserves_identity() {
    let mut game = setup(
        vec![A, B],
        A,
        vec![land(1, B, A), land(2, A, B), land(3, B, B)],
        vec![],
        start(9, Step::End),
    )
    .into_state();
    let identities = [object(&game, 1), object(&game, 2), object(&game, 3)];
    game = accept(
        &game,
        A,
        GameCommand::ActivateMana {
            source: identities[0],
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    )
    .into_state();
    game = pass(&game).into_state();
    for source in identities.into_iter().skip(1) {
        game = accept(
            &game,
            B,
            GameCommand::ActivateMana {
                source,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            },
        )
        .into_state();
    }
    for source in identities {
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    let candidate = pass_round(&game);
    let next = candidate.state();
    assert_eq!(next.execution().window().active_player(), B);
    assert_eq!(
        next.execution()
            .window()
            .cards()
            .get(identities[0])
            .unwrap()
            .tapped(),
        Some(true)
    );
    for source in identities.into_iter().skip(1) {
        assert_eq!(
            next.execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(false)
        );
    }
    for (card, identity) in (1..=3).zip(identities) {
        assert_eq!(object(next, card), identity);
    }
    assert_empty_pools(next);
    let activation = accept(
        next,
        B,
        GameCommand::ActivateMana {
            source: identities[1],
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    assert_eq!(activation.mana_production().unwrap().source, identities[1]);
    assert_eq!(
        activation.state().execution().mana(B, ManaType::Red),
        2.into()
    );
}

#[test]
fn prepared_cleanup_runs_its_own_sba_before_exception_priority_then_repeats() {
    let candidate = setup(
        vec![A, B],
        A,
        vec![creature(99, 0)],
        vec![],
        start(5, Step::Cleanup),
    );
    let game = candidate.state();
    assert_eq!(position(game).step, Step::Cleanup);
    assert_eq!(position(game).cleanup_iteration, 0);
    assert_eq!(game.turn().unwrap().stage(), TurnStage::Priority);
    assert_eq!(game.pending_turn_work(), None);
    assert_eq!(holder(game), A);
    let dead = object(game, 99);
    assert_eq!(
        game.execution().window().cards().get(dead).unwrap().zone(),
        ObjectZone::Graveyard(B)
    );
    // Initial cleanup work has not been consumed by an initial priority SBA pass.
    assert_eq!(candidate.turn_transitions().len(), 1);
    let repeated = pass_round(game);
    let records = repeated.turn_transitions();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].from.unwrap().cleanup_iteration, 0);
    assert_eq!(records[0].to.step, Step::Cleanup);
    assert_eq!(records[0].to.cleanup_iteration, 1);
    assert_eq!(records[1].to.step, Step::Untap);
    assert_eq!(records[1].to.number, 6);
    assert_eq!(records[2].to.step, Step::Upkeep);
    assert_eq!(repeated.state().execution().window().active_player(), B);
    assert_eq!(holder(repeated.state()), B);
    assert_eq!(object(repeated.state(), 99), dead);
}

#[test]
fn prepared_cleanup_needing_discard_does_not_run_initial_priority_sba() {
    let mut cards: Vec<_> = (1..=8).map(|card| instant(card, A)).collect();
    cards.push(creature(99, 0));
    let candidate = setup(vec![A, B], A, cards, vec![], start(5, Step::Cleanup));
    let game = candidate.state();
    assert_eq!(game.pending_turn_work(), Some(TurnWork::Cleanup));
    assert_eq!(game.execution().window().phase(), WindowPhase::TurnWork);
    assert_eq!(candidate.turn_transitions().len(), 1);
    let zero_toughness = object(game, 99);
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(zero_toughness)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        game.execution()
            .creature(zero_toughness)
            .unwrap()
            .unwrap()
            .toughness,
        0.into()
    );
}

#[test]
fn excess_hand_cleanup_suspends_before_clearing_existing_damage() {
    let mut cards: Vec<_> = (1..=9).map(|card| instant(card, A)).collect();
    cards.push(creature(99, 100));
    let game = setup(vec![A, B], A, cards, pools(&[A, B]), start(4, Step::End)).into_state();
    let target = object(&game, 99);
    let cast = accept(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(target),
        },
    );
    let resolved = pass_round(cast.state());
    assert_eq!(
        resolved
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );
    let cleanup = pass_round(resolved.state());
    assert_eq!(cleanup.turn_transitions().len(), 1);
    assert_eq!(cleanup.turn_transitions()[0].to.step, Step::Cleanup);
    let blocked = cleanup.state();
    assert_eq!(blocked.pending_turn_work(), Some(TurnWork::Cleanup));
    assert_eq!(blocked.execution().window().phase(), WindowPhase::TurnWork);
    assert_eq!(
        blocked
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        3.into()
    );
    assert_eq!(blocked.execution().window().active_player(), A);
    assert_empty_pools(blocked);
    assert_eq!(
        blocked
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|card| card.zone() == ObjectZone::Hand(A))
            .count(),
        8
    );
    for command in [
        GameCommand::Pass,
        GameCommand::Cast {
            spell: object(blocked, 2),
            target: TargetRef::Object(target),
        },
        GameCommand::Reply {
            decision: DecisionId(1),
            choice: DecisionReply::ManaColor(ManaType::Blue),
        },
    ] {
        let saved = blocked.clone();
        let error = if matches!(command, GameCommand::Reply { .. }) {
            GameError::Effect(EffectError::WrongDecisionKind)
        } else {
            GameError::TurnWorkRequired(TurnWork::Cleanup)
        };
        assert_eq!(blocked.advance(input(blocked, A, command)), Err(error));
        assert_eq!(blocked, &saved);
    }
    let completed = accept(
        blocked,
        A,
        GameCommand::Reply {
            decision: blocked.pending_decision().unwrap().id(),
            choice: DecisionReply::Discard(vec![object(blocked, 2)]),
        },
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    assert_eq!(completed.check_journal().choices.len(), 1);
    assert_eq!(completed.check_journal().moves.len(), 1);
    assert_eq!(
        completed.check_journal().moves[0]
            .before
            .card_id()
            .expect("physical fixture card"),
        CardId(2)
    );
    assert_eq!(
        completed.state().execution().window().cards().graveyard(A)[0],
        object(completed.state(), 2)
    );
}

#[test]
fn pending_mana_choice_prevents_step_closure_until_the_real_reply_completes() {
    let mut source = land(1, A, A);
    source.program = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::ChooseColor {
                amount: 2.into(),
                colors: BTreeSet::from([ManaType::Red, ManaType::Blue]),
            },
        }],
    );
    let game = setup(
        vec![A, B],
        A,
        vec![source],
        pools(&[A, B]),
        start(6, Step::End),
    )
    .into_state();
    let source = object(&game, 1);
    let pending = accept(
        &game,
        A,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    assert!(pending.turn_transitions().is_empty());
    let blocked = pending.state();
    assert_eq!(position(blocked), position(&game));
    assert_eq!(balances(blocked), balances(&game));
    assert_eq!(
        blocked
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    for actor in [A, B] {
        let saved = blocked.clone();
        assert_eq!(
            blocked.advance(input(blocked, actor, GameCommand::Pass)),
            Err(GameError::DecisionRequired)
        );
        assert_eq!(blocked, &saved);
    }
    let reply = accept(
        blocked,
        A,
        GameCommand::Reply {
            decision: blocked.pending_decision().unwrap().id(),
            choice: DecisionReply::ManaColor(ManaType::Red),
        },
    );
    assert!(reply.turn_transitions().is_empty());
    assert!(reply.state().pending_decision().is_none());
    assert_eq!(
        reply.state().execution().mana(A, ManaType::Red),
        game.execution().mana(A, ManaType::Red).plus(&2.into())
    );
    let boundary = pass_round(reply.state());
    assert_eq!(position(boundary.state()).number, 7);
    assert_eq!(boundary.state().execution().window().active_player(), B);
    assert_empty_pools(boundary.state());
}

#[test]
fn turn_exhaustion_rejects_the_final_pass_without_accepting_step_or_resource_changes() {
    let game = setup(
        vec![A, B],
        A,
        vec![],
        pools(&[A, B]),
        start(u64::MAX, Step::End),
    )
    .into_state();
    let first = pass(&game).into_state();
    let saved = first.clone();
    assert_eq!(
        first.advance(input(&first, holder(&first), GameCommand::Pass)),
        Err(GameError::Turn(TurnError::TurnExhausted))
    );
    assert_eq!(first, saved);
    assert_eq!(position(&first).step, Step::End);
    assert_eq!(position(&first).number, u64::MAX);
    assert_eq!(first.execution().window().active_player(), A);
    assert_eq!(balances(&first), balances(&game));
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules")
}
