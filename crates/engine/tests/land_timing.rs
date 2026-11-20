//! Ordinary authored land and instant/sorcery acceptance through GameState.
//! Prepared phases and labelled scheduler controls do not qualify native combat.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody, SpellTargetFilter};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    actions::TimingError,
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate,
    },
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaSpend,
        ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
    turn::{PriorityCompleted, Step, TurnError, TurnStage, TurnStart, TurnWork, WorkCompleted},
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);

fn context() -> GameContext {
    GameContext::new(
        "release".into(),
        "rules".into(),
        "catalogue".into(),
        "programs".into(),
        "land-timing".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn land(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
            }],
        ),
    }
}

fn spell(
    id: u64,
    owner: SeatId,
    sorcery: bool,
    cost: ManaCost,
    instructions: Vec<Instruction>,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: if sorcery {
            Program::sorcery(cost, ExecutableBody::new(body_target, instructions))
        } else {
            Program::instant(cost, ExecutableBody::new(body_target, instructions))
        },
    }
}

fn creature(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(
            None,
            2.into(),
            100.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn seeds() -> Vec<SeedCard> {
    vec![
        land(1, A, ObjectDestination::Hand),
        land(2, A, ObjectDestination::Hand),
        land(3, B, ObjectDestination::Hand),
        creature(90, B, ObjectDestination::Battlefield { controller: B }),
        creature(91, A, ObjectDestination::LibraryBottom),
        creature(92, B, ObjectDestination::LibraryBottom),
    ]
}

fn pools() -> Vec<FloatingMana> {
    vec![
        FloatingMana {
            player: A,
            mana_type: ManaType::Green,
            amount: 3.into(),
        },
        FloatingMana {
            player: B,
            mana_type: ManaType::Blue,
            amount: 2.into(),
        },
    ]
}

fn setup(step: Option<Step>, cards: Vec<SeedCard>) -> GameState {
    let init = if let Some(step) = step {
        GameState::initialize_scheduled(
            GameId(701),
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
                    vec![A, B],
                    A,
                    cards,
                    initial_battlefield_order,
                    pools(),
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart { number: 3, step },
        )
    } else {
        GameState::initialize(GameId(701), context(), {
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
                vec![A, B],
                A,
                cards,
                initial_battlefield_order,
                pools(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        })
    }
    .unwrap();
    assert!(init.land_play().is_none());
    init.into_state()
}

fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn zone(game: &GameState, card: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
        .zone()
}

fn holder(game: &GameState) -> SeatId {
    match game.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
        phase => panic!("expected ready native fixture, got {phase:?}"),
    }
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
    assert_eq!(
        game, &saved,
        "candidate construction mutated accepted state"
    );
    assert_eq!(
        candidate.state().basis().revision,
        game.basis().revision + 1
    );
    candidate
}

fn reject(game: &GameState, actor: SeatId, command: GameCommand, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(input(game, actor, command)), Err(expected));
    assert_eq!(game, &saved, "rejection mutated accepted state");
}

fn timing(error: TimingError) -> GameError {
    GameError::Effect(EffectError::Timing(error))
}

fn play(game: &GameState, actor: SeatId, card: u64) -> NativeCandidate {
    accept(
        game,
        actor,
        GameCommand::PlayLand {
            land: object(game, card),
        },
    )
}

fn pass(game: &GameState) -> GameState {
    let next = accept(game, holder(game), GameCommand::Pass);
    assert!(next.land_play().is_none());
    next.into_state()
}

fn round(game: &GameState) -> GameState {
    pass(&pass(game))
}

fn cast(game: &GameState, actor: SeatId, card: u64, target: TargetRef) -> NativeCandidate {
    let next = accept(
        game,
        actor,
        GameCommand::Cast {
            spell: object(game, card),
            target,
        },
    );
    assert!(next.land_play().is_none());
    next
}

fn damage() -> Instruction {
    Instruction::Damage { amount: 2.into() }
}

fn count(game: &GameState) -> &RulesInteger {
    game.turn().unwrap().lands_played()
}

#[test]
fn land_in_each_prepared_main_moves_once_without_stack_or_payment_and_keeps_priority() {
    for step in [Step::PrecombatMain, Step::PostcombatMain] {
        let game = setup(Some(step), seeds());
        let before = object(&game, 1);
        assert_eq!(count(&game), &0.into());
        let next = play(&game, A, 1);
        let record = next.land_play().unwrap();
        assert_eq!(record.player, A);
        assert_eq!(record.before.id(), before);
        assert_eq!(record.before.zone(), ObjectZone::Hand(A));
        assert_eq!(
            record.after.card_id().expect("physical fixture card"),
            record.before.card_id().expect("physical fixture card")
        );
        assert_eq!(record.after.owner(), A);
        assert_eq!(record.after.controller(), Some(A));
        assert_eq!(record.after.zone(), ObjectZone::Battlefield);
        assert_eq!(record.after.tapped(), Some(false));
        assert_ne!(record.after.id(), before);
        assert!(
            next.state()
                .execution()
                .window()
                .cards()
                .get(before)
                .is_none()
        );
        assert_eq!(object(next.state(), 1), record.after.id());
        assert_eq!(count(next.state()), &1.into());
        assert_eq!(holder(next.state()), A);
        assert_eq!(next.state().turn().unwrap().position().step, step);
        assert!(next.state().execution().window().entries().is_empty());
        assert!(next.mana_payment().is_none());
        assert!(next.mana_production().is_none());
        assert!(next.ability_activation().is_none());
        for pool in pools() {
            assert_eq!(
                next.state().execution().mana(pool.player, pool.mana_type),
                pool.amount
            );
        }
        assert!(next.turn_transitions().is_empty());
        reject(
            next.state(),
            A,
            GameCommand::PlayLand {
                land: object(next.state(), 2),
            },
            GameError::Turn(TurnError::LandLimitReached),
        );
    }
}

#[test]
fn played_land_can_immediately_pay_its_mana_tap_with_the_new_incarnation() {
    let game = setup(Some(Step::PrecombatMain), seeds());
    let land = play(&game, A, 1).into_state();
    let fresh = object(&land, 1);
    let next = accept(
        &land,
        A,
        GameCommand::ActivateMana {
            source: fresh,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(next.land_play().is_none());
    assert_eq!(next.mana_production().unwrap().source, fresh);
    assert_eq!(next.state().execution().mana(A, ManaType::Green), 4.into());
    assert_eq!(
        next.state()
            .execution()
            .window()
            .cards()
            .get(fresh)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(count(next.state()), &1.into());
    assert_eq!(holder(next.state()), A);
    assert!(next.state().execution().window().entries().is_empty());
}

#[test]
fn land_clears_real_prior_passes_after_opponents_mana_action() {
    let mut cards = seeds();
    cards.push(land(4, B, ObjectDestination::Battlefield { controller: B }));
    let game = setup(Some(Step::PrecombatMain), cards);
    let b_holds = pass(&game);
    let mana = accept(
        &b_holds,
        B,
        GameCommand::ActivateMana {
            source: object(&b_holds, 4),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
    .into_state();
    let a_holds_after_one_pass = pass(&mana);
    assert_eq!(holder(&a_holds_after_one_pass), A);
    let played = play(&a_holds_after_one_pass, A, 1).into_state();
    assert_eq!(holder(&played), A);
    let first = pass(&played);
    assert_eq!(first.turn().unwrap().position().step, Step::PrecombatMain);
    assert_eq!(holder(&first), B);
    let second = pass(&first);
    assert_eq!(
        second.turn().unwrap().position().step,
        Step::BeginningCombat
    );
    assert_eq!(count(&second), &1.into());
}

#[test]
fn actual_postcombat_play_resets_only_when_root_enters_the_next_turn() {
    let game = setup(Some(Step::PostcombatMain), seeds());
    let old_handle = object(&game, 1);
    let played = play(&game, A, 1).into_state();
    let end = round(&played);
    assert_eq!(end.turn().unwrap().position().step, Step::End);
    assert_eq!(count(&end), &1.into());
    let upkeep = round(&end);
    assert_eq!(upkeep.execution().window().turn_owner(), B);
    assert_eq!(upkeep.turn().unwrap().position().number, 4);
    assert_eq!(upkeep.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(count(&upkeep), &0.into());
    assert_eq!(zone(&upkeep, 1), ObjectZone::Battlefield);
    let draw = round(&upkeep);
    assert_eq!(draw.turn().unwrap().position().step, Step::Draw);
    assert_eq!(zone(&draw, 92), ObjectZone::Hand(B));
    let main = round(&draw);
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    reject(
        &main,
        B,
        GameCommand::PlayLand { land: old_handle },
        GameError::Effect(EffectError::UnknownObject),
    );
    assert_eq!(count(&main), &0.into());
    let new_play = play(&main, B, 3);
    assert_eq!(new_play.land_play().unwrap().player, B);
    assert_eq!(count(new_play.state()), &1.into());
}

#[test]
fn labelled_scheduler_controls_preserve_accepted_count_across_mains_and_repeated_cleanup() {
    let game = setup(Some(Step::PrecombatMain), seeds());
    let played = play(&game, A, 1).into_state();
    // Trusted scheduler controls test counting separately from the real root no-attack path.
    let mut schedule = played.turn().unwrap().clone();
    for _ in 0..2 {
        schedule = schedule
            .complete_priority(PriorityCompleted::Ordinary)
            .unwrap();
    }
    assert_eq!(
        schedule.stage(),
        TurnStage::Work(TurnWork::DeclareAttackers)
    );
    schedule = schedule
        .complete_work(WorkCompleted::DeclareAttackers)
        .unwrap()
        .complete_priority(PriorityCompleted::Attackers {
            declared_or_put_attacking: false,
        })
        .unwrap()
        .complete_priority(PriorityCompleted::Ordinary)
        .unwrap();
    assert_eq!(schedule.position().step, Step::PostcombatMain);
    assert_eq!(schedule.lands_played(), &1.into());
    for _ in 0..2 {
        schedule = schedule
            .complete_priority(PriorityCompleted::Ordinary)
            .unwrap();
    }
    for iteration in 1..=2 {
        schedule = schedule
            .complete_work(WorkCompleted::Cleanup {
                priority_required: true,
            })
            .unwrap()
            .complete_priority(PriorityCompleted::Ordinary)
            .unwrap();
        assert_eq!(schedule.position().cleanup_iteration, iteration);
        assert_eq!(schedule.lands_played(), &1.into());
    }
    schedule = schedule
        .complete_work(WorkCompleted::Cleanup {
            priority_required: false,
        })
        .unwrap();
    assert_eq!(schedule.position().number, 4);
    assert_eq!(schedule.lands_played(), &0.into());
    assert_eq!(count(&played), &1.into());
    let declarations = round(&round(&played));
    assert_eq!(declarations.pending_turn_work(), None);
    assert_eq!(
        declarations.turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    assert!(!declarations.execution().combat().unwrap().had_attackers());
    let root = round(&round(&declarations));
    assert_eq!(root.turn().unwrap().position().step, Step::PostcombatMain);
    assert!(root.execution().combat().is_none());
    assert_eq!(count(&root), &1.into());
    reject(
        &root,
        A,
        GameCommand::PlayLand {
            land: object(&root, 2),
        },
        GameError::Turn(TurnError::LandLimitReached),
    );
}

#[test]
fn foreign_nonland_and_other_zone_land_handles_reject_without_using_allowance() {
    let mut cards = seeds();
    cards.push(creature(5, A, ObjectDestination::Hand));
    let game = setup(Some(Step::PrecombatMain), cards);
    reject(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 3),
        },
        GameError::Effect(EffectError::NotOwnHand),
    );
    reject(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 5),
        },
        GameError::Effect(EffectError::NotLand),
    );
    for destination in [
        ObjectDestination::LibraryTop,
        ObjectDestination::Graveyard,
        ObjectDestination::Exile,
        ObjectDestination::Command,
        ObjectDestination::Battlefield { controller: A },
    ] {
        let mut cards = seeds();
        cards[0].destination = destination;
        let game = setup(Some(Step::PrecombatMain), cards);
        assert_eq!(
            count(&game),
            &0.into(),
            "prepared battlefield entry is not a land play"
        );
        reject(
            &game,
            A,
            GameCommand::PlayLand {
                land: object(&game, 1),
            },
            GameError::Effect(EffectError::NotOwnHand),
        );
        assert_eq!(count(&play(&game, A, 2).into_state()), &1.into());
    }
}

#[test]
fn wrong_holder_nonactive_player_and_occupied_stack_reject_both_main_actions() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        11,
        B,
        true,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        12,
        A,
        false,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = TargetRef::Object(object(&game, 90));
    let b_holds = pass(&game);
    for command in [
        GameCommand::PlayLand {
            land: object(&b_holds, 1),
        },
        GameCommand::Cast {
            spell: object(&b_holds, 10),
            target,
        },
    ] {
        reject(&b_holds, A, command, timing(TimingError::NotPriorityHolder));
    }
    for command in [
        GameCommand::PlayLand {
            land: object(&b_holds, 3),
        },
        GameCommand::Cast {
            spell: object(&b_holds, 11),
            target,
        },
    ] {
        reject(&b_holds, B, command, timing(TimingError::NotActivePlayer));
    }
    let stacked = cast(&game, A, 12, target).into_state();
    for command in [
        GameCommand::PlayLand {
            land: object(&stacked, 1),
        },
        GameCommand::Cast {
            spell: object(&stacked, 10),
            target,
        },
    ] {
        reject(&stacked, A, command, timing(TimingError::StackNotEmpty));
    }
}

#[test]
fn nonmain_ready_steps_reject_land_and_sorcery_but_allow_instant() {
    for step in [
        Step::Upkeep,
        Step::Draw,
        Step::BeginningCombat,
        Step::EndCombat,
        Step::End,
    ] {
        let mut cards = seeds();
        cards.push(spell(
            10,
            A,
            true,
            ManaCost::free(),
            vec![damage()],
            BodyTarget::SingleCreature,
        ));
        cards.push(spell(
            11,
            A,
            false,
            ManaCost::free(),
            vec![damage()],
            BodyTarget::SingleCreature,
        ));
        let game = setup(Some(step), cards);
        let target = TargetRef::Object(object(&game, 90));
        reject(
            &game,
            A,
            GameCommand::PlayLand {
                land: object(&game, 1),
            },
            timing(TimingError::MainPhaseRequired),
        );
        reject(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, 10),
                target,
            },
            timing(TimingError::MainPhaseRequired),
        );
        let cast = cast(&game, A, 11, target);
        assert_eq!(cast.state().execution().window().entries().len(), 1);
        assert_eq!(count(cast.state()), &0.into());
    }
}

#[test]
fn calendar_free_root_and_direct_executor_cannot_admit_sorcery_or_land() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        11,
        A,
        false,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let game = setup(None, cards);
    let target = TargetRef::Object(object(&game, 90));
    reject(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 1),
        },
        timing(TimingError::TurnRequired),
    );
    reject(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 10),
            target,
        },
        timing(TimingError::TurnRequired),
    );
    let execution = game.execution();
    assert_eq!(
        execution.cast(execution.revision(), A, object(&game, 10), target),
        Err(EffectError::Timing(TimingError::TurnRequired))
    );
    assert_eq!(
        execution.cast_with_payment(
            execution.revision(),
            A,
            object(&game, 10),
            target,
            Some(ManaPayment::default())
        ),
        Err(EffectError::Timing(TimingError::TurnRequired))
    );
    assert_eq!(
        cast(&game, A, 11, target)
            .state()
            .execution()
            .window()
            .entries()
            .len(),
        1
    );
}

#[test]
fn direct_executor_from_scheduled_root_still_cannot_bypass_sorcery_calendar() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::one(ManaType::Green),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let execution = game.execution();
    let before = execution.clone();
    let target = TargetRef::Object(object(&game, 90));
    assert_eq!(
        execution.cast(execution.revision(), A, object(&game, 10), target),
        Err(EffectError::Timing(TimingError::TurnRequired))
    );
    assert_eq!(execution, &before);
    assert_eq!(
        cast(&game, A, 10, target)
            .state()
            .execution()
            .mana(A, ManaType::Green),
        2.into()
    );
}

#[test]
fn executor_checks_must_finish_before_even_an_instant_can_be_cast() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        false,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let execution = EffectWindow::new({
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
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            pools(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    let before = execution.clone();
    let registry = execution.window().cards();
    let source = registry.current_identity(CardId(10)).unwrap();
    let target = TargetRef::Object(registry.current_identity(CardId(90)).unwrap());
    assert_eq!(
        execution.cast(execution.revision(), A, source, target),
        Err(EffectError::Window(StackWindowError::Priority(
            PriorityError::ChecksRequired
        )))
    );
    assert_eq!(execution, before);
}

#[test]
fn nonactive_instant_can_start_an_empty_stack_and_returns_active_priority_after_resolution() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        B,
        false,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = object(&game, 90);
    let b_holds = pass(&game);
    assert!(b_holds.execution().window().entries().is_empty());
    let instant = cast(&b_holds, B, 10, TargetRef::Object(target)).into_state();
    assert_eq!(holder(&instant), B);
    let done = round(&instant);
    assert_eq!(
        done.execution().creature(target).unwrap().unwrap().damage,
        2.into()
    );
    assert_eq!(holder(&done), A);
    assert_eq!(done.turn().unwrap().position().step, Step::PrecombatMain);
    assert_eq!(count(&done), &0.into());
}

#[test]
fn mandatory_combat_and_discard_work_reject_player_land_and_sorcery() {
    for (step, work) in [
        (Step::DeclareBlockers, TurnWork::DeclareBlockers),
        (
            Step::CombatDamage,
            TurnWork::CombatDamage { additional: false },
        ),
        (Step::Cleanup, TurnWork::Cleanup),
    ] {
        let mut cards = seeds();
        cards.push(spell(
            10,
            A,
            true,
            ManaCost::free(),
            vec![damage()],
            BodyTarget::SingleCreature,
        ));
        if step == Step::Cleanup {
            cards.extend((20..=25).map(|id| land(id, A, ObjectDestination::Hand)));
        }
        let game = setup(Some(step), cards);
        let target = TargetRef::Object(object(&game, 90));
        assert_eq!(game.pending_turn_work(), Some(work));
        for command in [
            GameCommand::PlayLand {
                land: object(&game, 1),
            },
            GameCommand::Cast {
                spell: object(&game, 10),
                target,
            },
        ] {
            reject(&game, A, command, GameError::TurnWorkRequired(work));
        }
    }
}

#[test]
fn ended_game_rejects_land_before_timing_or_physical_work() {
    let cards = seeds()
        .into_iter()
        .filter(|seed| seed.card != CardId(91))
        .collect();
    let game = setup(Some(Step::Draw), cards);
    assert!(game.outcome().is_some());
    reject(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 1),
        },
        GameError::GameEnded,
    );
}

#[test]
fn stale_input_and_foreign_basis_cannot_duplicate_accepted_land_or_spend_resources() {
    let game = setup(Some(Step::PrecombatMain), seeds());
    let stale = input(
        &game,
        A,
        GameCommand::PlayLand {
            land: object(&game, 2),
        },
    );
    let played = play(&game, A, 1).into_state();
    let before = played.clone();
    assert_eq!(played.advance(stale), Err(GameError::StaleRevision));
    for (which, expected) in [
        (0, GameError::WrongGame),
        (1, GameError::WrongContext),
        (2, GameError::WrongEpoch),
    ] {
        let mut invalid = input(
            &played,
            A,
            GameCommand::PlayLand {
                land: object(&played, 2),
            },
        );
        match which {
            0 => invalid.basis.game = GameId(999),
            1 => {
                invalid.basis.context = GameContext::new(
                    "other".into(),
                    "rules".into(),
                    "catalogue".into(),
                    "programs".into(),
                    "land-timing".into(),
                    test_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap()
            }
            _ => invalid.basis.epoch += 1,
        }
        assert_eq!(played.advance(invalid), Err(expected));
    }
    assert_eq!(played, before);
    assert_eq!(count(&played), &1.into());
    assert_eq!(played.execution().mana(A, ManaType::Green), 3.into());
}

#[test]
fn sorcery_in_both_mains_pays_exact_cost_and_resolves_in_the_same_step() {
    for step in [Step::PrecombatMain, Step::PostcombatMain] {
        let mut cards = seeds();
        cards.push(spell(
            10,
            A,
            true,
            ManaCost::one(ManaType::Green),
            vec![damage(), damage()],
            BodyTarget::SingleCreature,
        ));
        let game = setup(Some(step), cards);
        let target = object(&game, 90);
        let cast = cast(&game, A, 10, TargetRef::Object(target));
        let receipt = cast.mana_payment().unwrap();
        assert_eq!(receipt.payer, A);
        assert_eq!(receipt.cost, ManaCost::one(ManaType::Green));
        assert_eq!(cast.state().execution().mana(A, ManaType::Green), 2.into());
        assert_eq!(zone(cast.state(), 10), ObjectZone::Stack);
        let done = round(cast.state());
        assert_eq!(
            done.execution().creature(target).unwrap().unwrap().damage,
            4.into()
        );
        assert_eq!(zone(&done, 10), ObjectZone::Graveyard(A));
        assert_eq!(done.turn().unwrap().position().step, step);
        assert_eq!(holder(&done), A);
        assert_eq!(count(&done), &0.into());
    }
}

#[test]
fn invalid_sorcery_payment_and_targets_preserve_prior_land_and_nonzero_pool() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::generic(1.into()),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let played = play(&game, A, 1).into_state();
    let target = TargetRef::Object(object(&played, 90));
    for payment in [
        ManaPayment { spend: vec![] },
        ManaPayment {
            spend: vec![ManaSpend {
                mana_type: ManaType::Green,
                amount: 4.into(),
            }],
        },
    ] {
        let before = played.clone();
        assert!(matches!(
            played.advance(input(
                &played,
                A,
                GameCommand::CastPaid {
                    spell: object(&played, 10),
                    target,
                    payment
                }
            )),
            Err(GameError::Effect(EffectError::InvalidManaPayment(_)))
        ));
        assert_eq!(played, before);
    }
    reject(
        &played,
        A,
        GameCommand::Cast {
            spell: object(&played, 10),
            target: TargetRef::Object(object(&played, 2)),
        },
        GameError::Effect(EffectError::IllegalTarget),
    );
    let paid = accept(
        &played,
        A,
        GameCommand::CastPaid {
            spell: object(&played, 10),
            target,
            payment: ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 1.into(),
                }],
            },
        },
    );
    assert!(paid.land_play().is_none());
    assert_eq!(count(paid.state()), &1.into());
    assert_eq!(paid.state().execution().mana(A, ManaType::Green), 2.into());
}

#[test]
fn opponent_instant_response_counters_actual_sorcery() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        11,
        B,
        false,
        ManaCost::free(),
        vec![Instruction::CounterSpell],
        BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = object(&game, 90);
    let stacked = cast(&game, A, 10, TargetRef::Object(target)).into_state();
    let original = stacked.execution().window().entries()[0].id();
    let opponent = pass(&stacked);
    let counter = cast(&opponent, B, 11, TargetRef::Stack(original)).into_state();
    assert_eq!(counter.execution().window().entries().len(), 2);
    let done = round(&counter);
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(zone(&done, 10), ObjectZone::Graveyard(A));
    assert_eq!(zone(&done, 11), ObjectZone::Graveyard(B));
    assert_eq!(
        done.execution().creature(target).unwrap().unwrap().damage,
        0.into()
    );
    assert_eq!(holder(&done), A);
}

#[test]
fn copying_actual_sorcery_uses_owned_reply_and_blocks_land_during_decision() {
    let mut cards = seeds();
    cards.push(spell(
        10,
        A,
        true,
        ManaCost::free(),
        vec![damage()],
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        11,
        B,
        false,
        ManaCost::free(),
        vec![Instruction::CopySpell],
        BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = object(&game, 90);
    let stacked = cast(&game, A, 10, TargetRef::Object(target)).into_state();
    let original = stacked.execution().window().entries()[0].id();
    let b_holds = pass(&stacked);
    let copy_spell = cast(&b_holds, B, 11, TargetRef::Stack(original)).into_state();
    let pending = round(&copy_spell);
    let decision = pending.pending_decision().unwrap();
    reject(
        &pending,
        A,
        GameCommand::PlayLand {
            land: object(&pending, 1),
        },
        GameError::DecisionRequired,
    );
    let replied = accept(
        &pending,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
        },
    );
    assert!(replied.land_play().is_none());
    assert_eq!(replied.state().execution().window().entries().len(), 2);
    let copied_done = round(replied.state());
    assert_eq!(
        copied_done
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    let original_done = round(&copied_done);
    assert_eq!(
        original_done
            .execution()
            .creature(target)
            .unwrap()
            .unwrap()
            .damage,
        4.into()
    );
    assert_eq!(zone(&original_done, 10), ObjectZone::Graveyard(A));
    assert!(original_done.execution().window().spell_copies().is_empty());
}

#[test]
fn played_land_mana_color_reply_preserves_count_without_repeating_record() {
    let mut cards = seeds();
    cards[0].program = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 2.into(),
                colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
            },
        }],
    );
    let game = setup(Some(Step::PrecombatMain), cards);
    let played = play(&game, A, 1).into_state();
    let pending = accept(
        &played,
        A,
        GameCommand::ActivateMana {
            source: object(&played, 1),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert!(pending.land_play().is_none());
    let decision = pending.state().pending_decision().unwrap();
    reject(
        pending.state(),
        A,
        GameCommand::PlayLand {
            land: object(pending.state(), 2),
        },
        GameError::DecisionRequired,
    );
    let reply = accept(
        pending.state(),
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::ManaColor(ManaType::Blue),
        },
    );
    assert!(reply.land_play().is_none());
    assert_eq!(count(reply.state()), &1.into());
    assert_eq!(reply.state().execution().mana(A, ManaType::Blue), 2.into());
    assert_eq!(reply.mana_production().unwrap().source, object(&played, 1));
}

#[test]
fn hand_lands_and_permanents_have_no_invented_casting_cost_or_spell_path() {
    let mut cards = seeds();
    cards.push(creature(5, A, ObjectDestination::Hand));
    cards.push(SeedCard {
        card: CardId(6),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(BodyTarget::SingleCreature, vec![damage()]),
            }],
        ),
    });
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = TargetRef::Object(object(&game, 90));
    for card in [1, 5, 6] {
        reject(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, card),
                target,
            },
            GameError::Effect(EffectError::NotCastableSpell),
        );
        reject(
            &game,
            A,
            GameCommand::CastPaid {
                spell: object(&game, card),
                target,
                payment: ManaPayment::default(),
            },
            GameError::Effect(EffectError::NotCastableSpell),
        );
    }
}

#[test]
fn invalid_authored_sorcery_programs_reject_at_setup() {
    for (cost, instructions, destination) in [
        (
            ManaCost::generic((-1).into()),
            vec![damage()],
            ObjectDestination::Hand,
        ),
        (
            ManaCost::specific(ManaType::Green, (-1).into()),
            vec![damage()],
            ObjectDestination::Hand,
        ),
        (ManaCost::free(), vec![], ObjectDestination::Hand),
        (
            ManaCost::free(),
            vec![damage(), Instruction::CounterSpell],
            ObjectDestination::Hand,
        ),
        (
            ManaCost::free(),
            vec![damage()],
            ObjectDestination::Battlefield { controller: A },
        ),
    ] {
        let mut cards = seeds();
        let mut authored = spell(10, A, true, cost, instructions, BodyTarget::SingleCreature);
        authored.destination = destination;
        cards.push(authored);
        assert_eq!(
            GameState::initialize_scheduled(
                GameId(701),
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
                        vec![A, B],
                        A,
                        cards,
                        initial_battlefield_order,
                        pools(),
                        20.into(),
                        CombatRules::UnrestrictedFreeForAll,
                        test_vocabulary(),
                        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                    )
                },
                TurnStart {
                    number: 3,
                    step: Step::PrecombatMain
                }
            ),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules")
}
