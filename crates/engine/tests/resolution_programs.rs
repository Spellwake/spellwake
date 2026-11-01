//! Independently derived ordered-program cases frozen in resolution-program-scenarios.md.
//! These test native mechanisms, not complete cards or a production game protocol.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    characteristics::{PowerToughness, PowerToughnessCounters, PowerToughnessOperation},
    damage::{DamageEffectCreation, DamageEffectId, DamageJournal, DamageOutcome},
    effects::{
        CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, ResolutionProgress,
        SeedCard,
    },
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};
use std::collections::BTreeSet;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const FIRST: u64 = 100;
const SECOND: u64 = 101;
const HUGE: &str = "900719925474099312345678901234567890";
const FOUR_HUGE: &str = "3602879701896397249382715604938271560";

fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}

fn damage(amount: i64) -> Instruction {
    Instruction::Damage {
        amount: amount.into(),
    }
}

fn shield(amount: i64) -> Instruction {
    Instruction::CreatePreventionShield {
        amount: amount.into(),
    }
}

fn multiplier(factor: i64) -> Instruction {
    Instruction::CreateDamageMultiplier {
        factor: factor.into(),
    }
}

fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn continuous(operation: PowerToughnessOperation) -> Instruction {
    Instruction::ContinuousPowerToughness { operation }
}

fn counters(plus: i64, minus: i64) -> Instruction {
    Instruction::PutPowerToughnessCounters {
        counters: PowerToughnessCounters {
            plus_one: plus.into(),
            minus_one: minus.into(),
        },
    }
}

fn initialize(
    toughness: RulesInteger,
    programs: Vec<(u64, SeatId, Vec<Instruction>)>,
) -> Result<NativeCandidate, GameError> {
    let mut cards: Vec<_> = [(FIRST, B), (SECOND, A)]
        .into_iter()
        .map(|(card, controller)| SeedCard {
            card: CardId(card),
            owner: A,
            destination: CardDestination::Battlefield { controller },
            program: Program::VanillaCreature {
                power: 1.into(),
                toughness: toughness.clone(),
            },
        })
        .collect();
    cards.extend(
        programs
            .into_iter()
            .map(|(card, owner, instructions)| SeedCard {
                card: CardId(card),
                owner,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::one(ManaType::Blue),
                    instructions,
                },
            }),
    );
    GameState::initialize(
        GameId(93),
        GameContext::new(
            "ordered-tests".into(),
            "rules-20260925".into(),
            "synthetic".into(),
            "ordered-primitives".into(),
            "individual-turn".into(),
        )
        .unwrap(),
        vec![A, B],
        A,
        cards,
        [A, B]
            .into_iter()
            .map(|player| FloatingMana {
                player,
                mana_type: ManaType::Blue,
                amount: 100.into(),
            })
            .collect(),
    )
}

fn world(toughness: i64, programs: Vec<(u64, SeatId, Vec<Instruction>)>) -> GameState {
    initialize(toughness.into(), programs).unwrap().into_state()
}

fn object(game: &GameState, card: u64) -> CardObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn zone(game: &GameState, card: u64) -> CardZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
        .zone()
}

fn marked(game: &GameState, target: CardObjectId) -> RulesInteger {
    game.execution().creature(target).unwrap().damage
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
    let mut expected = game.basis().clone();
    expected.revision += 1;
    assert_eq!(candidate.state().basis(), &expected);
    candidate
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected settled priority")
    };
    holder
}

fn cast(
    game: &GameState,
    actor: SeatId,
    card: u64,
    target: TargetRef,
) -> (GameState, StackEntryId) {
    let game = if holder(game) == actor {
        game.clone()
    } else {
        accept(game, holder(game), GameCommand::Pass).into_state()
    };
    assert_eq!(holder(&game), actor);
    let next = accept(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    assert_eq!(next.damage_journal(), &DamageJournal::default());
    let source = next
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    (next.into_state(), source)
}

fn resolve(game: &GameState) -> NativeCandidate {
    let first = accept(game, holder(game), GameCommand::Pass);
    assert_eq!(first.damage_journal(), &DamageJournal::default());
    accept(first.state(), holder(first.state()), GameCommand::Pass)
}

fn execute(game: &GameState, card: u64, target: CardObjectId) -> (NativeCandidate, StackEntryId) {
    let (game, source) = cast(game, A, card, TargetRef::Card(target));
    (resolve(&game), source)
}

fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    accept(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}

fn choose(game: &GameState, effect: impl Into<DamageEffectId>) -> NativeCandidate {
    reply(game, DecisionReply::DamageOrder(effect.into()))
}

fn checking(game: &GameState) -> EffectWindow {
    let first = accept(game, holder(game), GameCommand::Pass).into_state();
    let execution = first
        .execution()
        .pass(first.execution().revision(), holder(&first))
        .unwrap();
    assert!(matches!(
        execution.window().phase(),
        WindowPhase::CheckingTargets(_)
    ));
    execution
}

fn assert_finished(candidate: &NativeCandidate, source: StackEntryId, card: u64) {
    let game = candidate.state();
    assert!(game.pending_decision().is_none());
    assert_eq!(game.execution().instruction_index(), None);
    assert!(
        game.execution()
            .window()
            .entries()
            .iter()
            .all(|entry| entry.id() != source)
    );
    assert_eq!(zone(game, card), CardZone::Graveyard(A));
    assert_eq!(holder(game), A);
}

fn assert_rejected(game: &GameState, request: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(expected));
    assert_eq!(game, &saved);
}

#[test]
fn setup_validates_every_instruction_and_homogeneous_nonempty_programs() {
    let invalid = vec![
        vec![],
        vec![damage(1), Instruction::CopySpell],
        vec![Instruction::CounterSpell, shield(1)],
        vec![damage(1), damage(0)],
        vec![damage(1), shield(-1)],
        vec![shield(1), multiplier(0)],
        vec![damage(1), counters(-1, 2)],
        vec![damage(1), counters(0, 0)],
    ];
    for instructions in invalid {
        assert_eq!(
            initialize(100.into(), vec![(1, A, instructions)]),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
    for instructions in [
        vec![damage(1), shield(1)],
        vec![Instruction::CopySpell, Instruction::CounterSpell],
    ] {
        assert!(initialize(100.into(), vec![(1, A, instructions)]).is_ok());
    }
}

#[test]
fn transient_zero_toughness_recovers_before_any_state_based_action() {
    let game = world(
        3,
        vec![(
            1,
            A,
            vec![
                continuous(PowerToughnessOperation::Set(pt(1, 0))),
                continuous(PowerToughnessOperation::Modify(pt(0, 2))),
            ],
        )],
    );
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    assert_eq!(object(candidate.state(), FIRST), target);
    assert_eq!(zone(candidate.state(), FIRST), CardZone::Battlefield);
    let derived = candidate
        .state()
        .execution()
        .power_toughness(target)
        .unwrap();
    assert_eq!(derived.base, pt(1, 0));
    assert_eq!(derived.derived, pt(1, 2));
    assert_eq!(derived.applied_effects.len(), 2);
    for effect in derived.applied_effects {
        assert_eq!(
            candidate
                .state()
                .execution()
                .power_toughness_effect(effect)
                .unwrap()
                .source,
            source
        );
    }
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
}

#[test]
fn lethal_damage_then_bounce_retains_exact_applied_identity_and_reaches_hand() {
    let game = world(
        3,
        vec![(1, A, vec![damage(3), Instruction::ReturnToOwnersHand])],
    );
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    assert_eq!(zone(candidate.state(), FIRST), CardZone::Hand(A));
    assert_ne!(object(candidate.state(), FIRST), target);
    assert!(candidate.state().execution().creature(target).is_none());
    let journal = candidate.damage_journal();
    assert_eq!(journal.attempts.len(), 1);
    assert_eq!(journal.applied.len(), 1);
    assert_eq!(
        (
            journal.applied[0].source,
            journal.applied[0].target,
            journal.applied[0].amount.clone()
        ),
        (source, target, 3.into())
    );
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::Dealt(3.into())
    );
}

#[test]
fn departed_operand_skips_every_later_creature_primitive_without_allocating_effects() {
    let game = world(
        3,
        vec![(
            1,
            A,
            vec![
                Instruction::ReturnToOwnersHand,
                damage(3),
                continuous(PowerToughnessOperation::Set(pt(9, 9))),
                counters(1, 0),
                shield(4),
                multiplier(2),
                Instruction::ReturnToOwnersHand,
            ],
        )],
    );
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    assert_eq!(zone(candidate.state(), FIRST), CardZone::Hand(A));
    assert_ne!(object(candidate.state(), FIRST), target);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert_eq!(
        candidate.state().execution().prevention_shields().count(),
        0
    );
    assert_eq!(
        candidate.state().execution().damage_multipliers().count(),
        0
    );
    assert_eq!(
        candidate
            .state()
            .execution()
            .copiable_power_toughness(object(candidate.state(), FIRST)),
        Some(pt(1, 3))
    );
    assert_eq!(
        marked(candidate.state(), object(candidate.state(), SECOND)),
        0.into()
    );
}

#[test]
fn a_created_shield_is_observed_even_when_later_instructions_consume_it() {
    let game = world(100, vec![(1, A, vec![shield(4), damage(3), damage(3)])]);
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    let journal = candidate.damage_journal();
    let [
        DamageEffectCreation::Prevention {
            shield,
            instruction,
        },
    ] = journal.created.as_slice()
    else {
        panic!("expected exact shield birth")
    };
    assert_eq!(
        (
            *instruction,
            shield.source,
            shield.target,
            shield.remaining.clone()
        ),
        (0, source, target, 4.into())
    );
    assert_eq!(journal.attempts.len(), 2);
    assert_ne!(journal.attempts[0].id, journal.attempts[1].id);
    assert_eq!(journal.preventions.len(), 2);
    for (index, amount) in [3, 1].into_iter().enumerate() {
        let event = &journal.preventions[index];
        assert_eq!(
            (
                event.shield,
                event.shield_source,
                event.target,
                event.stage,
                event.amount.clone()
            ),
            (shield.id, source, target, 1, amount.into())
        );
        assert_eq!(event.attempt, journal.attempts[index].id);
        assert_eq!(journal.completions[index].attempt, event.attempt);
    }
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
    assert_eq!(
        journal.completions[1].outcome,
        DamageOutcome::Dealt(2.into())
    );
    assert_eq!(journal.applied.len(), 1);
    assert_eq!(journal.applied[0].attempt, journal.attempts[1].id);
    assert_eq!(
        candidate.state().execution().prevention_shield(shield.id),
        None
    );
    assert_eq!(marked(candidate.state(), target), 2.into());
}

#[test]
fn a_multiplier_is_reused_once_for_each_independent_instruction_attempt() {
    let game = world(100, vec![(1, A, vec![multiplier(2), damage(1), damage(1)])]);
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    let journal = candidate.damage_journal();
    let [
        DamageEffectCreation::Multiplier {
            multiplier,
            instruction,
        },
    ] = journal.created.as_slice()
    else {
        panic!("expected multiplier birth")
    };
    assert_eq!(
        (*instruction, multiplier.source, multiplier.target),
        (0, source, target)
    );
    assert_eq!(journal.attempts.len(), 2);
    assert_eq!(journal.multiplications.len(), 2);
    for (index, event) in journal.multiplications.iter().enumerate() {
        assert_eq!(event.attempt, journal.attempts[index].id);
        assert_eq!(
            (
                event.multiplier,
                event.stage,
                event.before.clone(),
                event.after.clone()
            ),
            (multiplier.id, 1, 1.into(), 2.into())
        );
    }
    assert_eq!(marked(candidate.state(), target), 4.into());
    assert_eq!(
        candidate
            .state()
            .execution()
            .damage_multiplier(multiplier.id),
        Some(multiplier)
    );
}

#[test]
fn several_same_source_births_bind_actual_ids_before_a_later_damage_choice() {
    let game = world(
        100,
        vec![(1, A, vec![shield(3), shield(4), damage(5), damage(3)])],
    );
    let target = object(&game, FIRST);
    let (start, source) = execute(&game, 1, target);
    let game = start.state();
    assert_eq!(zone(game, 1), CardZone::Stack);
    assert_eq!(game.execution().instruction_index(), Some(2));
    let RuleFrame::DamageOrder(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected later instruction choice")
    };
    assert_eq!(frame.instruction_index(), 2);
    assert_eq!(frame.source(), source);
    assert_eq!(frame.chooser(), B);
    let births = &start.damage_journal().created;
    assert_eq!(births.len(), 2);
    let mut ids = Vec::new();
    for (index, birth) in births.iter().enumerate() {
        let DamageEffectCreation::Prevention {
            shield,
            instruction,
        } = birth
        else {
            panic!("expected shields")
        };
        assert_eq!(
            (*instruction, shield.source, shield.target),
            (index, source, target)
        );
        assert_eq!(game.execution().prevention_shield(shield.id), Some(shield));
        ids.push(shield.id);
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(frame.options(), &[ids[0].into(), ids[1].into()]);
    assert_eq!(start.damage_journal().attempts.len(), 1);
    assert!(start.damage_journal().completions.is_empty());
    let done = choose(game, ids[0]);
    assert_finished(&done, source, 1);
    assert!(done.damage_journal().created.is_empty());
    assert_eq!(done.damage_journal().attempts.len(), 1);
    assert_eq!(done.damage_journal().attempts[0].amount, 3.into());
    assert_eq!(done.damage_journal().completions.len(), 2);
    assert_eq!(
        done.damage_journal().completions[0].attempt,
        start.damage_journal().attempts[0].id
    );
    assert_eq!(
        done.damage_journal().completions[1].outcome,
        DamageOutcome::Dealt(1.into())
    );
    assert_eq!(marked(done.state(), target), 1.into());
}

#[test]
fn pending_later_instruction_retains_earlier_work_and_blocks_priority_and_checks() {
    let game = world(
        100,
        vec![(1, A, vec![damage(2), shield(4), multiplier(2), damage(3)])],
    );
    let target = object(&game, FIRST);
    let (start, source) = execute(&game, 1, target);
    let game = start.state();
    assert_eq!(marked(game, target), 2.into());
    assert_eq!(game.execution().instruction_index(), Some(3));
    let RuleFrame::DamageOrder(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected damage choice")
    };
    assert_eq!(frame.instruction_index(), 3);
    assert_eq!(start.damage_journal().attempts.len(), 2);
    assert_eq!(start.damage_journal().completions.len(), 1);
    assert_eq!(start.damage_journal().applied[0].amount, 2.into());
    let saved = game.clone();
    assert_rejected(
        game,
        input(game, A, GameCommand::Pass),
        GameError::DecisionRequired,
    );
    assert!(
        game.execution()
            .pass(game.execution().revision(), A)
            .is_err()
    );
    assert_eq!(
        game.execution().settle_checks(game.execution().revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(game, &saved);
    assert_eq!(zone(game, 1), CardZone::Stack);
    let shield = game.execution().prevention_shields().next().unwrap().id;
    let done = choose(game, shield);
    assert_finished(&done, source, 1);
    assert!(done.damage_journal().attempts.is_empty());
    assert!(done.damage_journal().created.is_empty());
    assert!(done.damage_journal().applied.is_empty());
    assert_eq!(done.damage_journal().completions.len(), 1);
    assert_eq!(marked(done.state(), target), 2.into());
}

#[test]
fn twelve_attempts_in_one_candidate_preserve_typed_encounter_order() {
    let game = world(100, vec![(1, A, (1..=12).map(damage).collect())]);
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    let journal = candidate.damage_journal();
    assert_eq!(
        (
            journal.attempts.len(),
            journal.applied.len(),
            journal.completions.len()
        ),
        (12, 12, 12)
    );
    let ids: Vec<_> = journal.attempts.iter().map(|attempt| attempt.id).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(ids.iter().copied().collect::<BTreeSet<_>>().len(), 12);
    for (index, attempt) in journal.attempts.iter().enumerate() {
        let amount: RulesInteger = (index as i64 + 1).into();
        assert_eq!(
            (attempt.source, attempt.target, attempt.amount.clone()),
            (source, target, amount.clone())
        );
        assert_eq!(journal.applied[index].attempt, attempt.id);
        assert_eq!(journal.completions[index].attempt, attempt.id);
        assert_eq!(
            journal.completions[index].outcome,
            DamageOutcome::Dealt(amount)
        );
    }
    assert_eq!(marked(candidate.state(), target), 78.into());
    assert!(journal.created.is_empty());
}

#[test]
fn reply_completes_prior_attempt_then_emits_twelve_later_attempts_without_replaying_facts() {
    let mut instructions = vec![shield(1), shield(1), damage(1)];
    instructions.extend((1..=12).map(|_| damage(1)));
    let game = world(100, vec![(1, A, instructions)]);
    let target = object(&game, FIRST);
    let (start, source) = execute(&game, 1, target);
    let old_attempt = start.damage_journal().attempts[0].id;
    let first_shield = start
        .state()
        .execution()
        .prevention_shields()
        .next()
        .unwrap()
        .id;
    let done = choose(start.state(), first_shield);
    assert_finished(&done, source, 1);
    let journal = done.damage_journal();
    assert!(journal.created.is_empty());
    assert_eq!(
        (
            journal.attempts.len(),
            journal.completions.len(),
            journal.applied.len()
        ),
        (12, 13, 11)
    );
    assert_eq!(journal.completions[0].attempt, old_attempt);
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
    assert_eq!(
        journal.completions[1].outcome,
        DamageOutcome::FullyPrevented
    );
    assert_eq!(journal.preventions.len(), 2);
    assert_eq!(journal.preventions[0].attempt, old_attempt);
    assert_eq!(journal.preventions[1].attempt, journal.attempts[0].id);
    assert!(
        journal
            .attempts
            .iter()
            .all(|attempt| attempt.id > old_attempt)
    );
    assert!(
        journal
            .attempts
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
    );
    for (index, attempt) in journal.attempts.iter().enumerate() {
        assert_eq!(journal.completions[index + 1].attempt, attempt.id);
        assert_eq!(
            (attempt.source, attempt.target, attempt.amount.clone()),
            (source, target, 1.into())
        );
    }
    assert_eq!(marked(done.state(), target), 11.into());
    assert_eq!(
        start
            .state()
            .execution()
            .pending_damage()
            .unwrap()
            .attempt()
            .id,
        old_attempt
    );
}

#[test]
fn two_copy_choices_keep_source_cursor_and_independent_full_programs_above_it() {
    let game = world(
        100,
        vec![
            (1, A, vec![damage(1), damage(2)]),
            (2, B, vec![Instruction::CopySpell, Instruction::CopySpell]),
        ],
    );
    let first = object(&game, FIRST);
    let second = object(&game, SECOND);
    let (game, original) = cast(&game, A, 1, TargetRef::Card(first));
    let (game, source) = cast(&game, B, 2, TargetRef::Stack(original));
    let start = resolve(&game);
    let RuleFrame::CopyTargets(old_frame) = start.state().pending_decision().unwrap().frame()
    else {
        panic!("expected copy")
    };
    assert_eq!(
        (
            old_frame.entry(),
            old_frame.original(),
            old_frame.instruction_index()
        ),
        (source, original, 0)
    );
    let middle = reply(
        start.state(),
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    let game = middle.state();
    let RuleFrame::CopyTargets(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected second copy")
    };
    assert_eq!(
        (frame.entry(), frame.original(), frame.instruction_index()),
        (source, original, 1)
    );
    assert_eq!(game.execution().instruction_index(), Some(1));
    assert_ne!(
        game.pending_decision().unwrap().id(),
        start.state().pending_decision().unwrap().id()
    );
    assert_rejected(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: start.state().pending_decision().unwrap().id(),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            },
        ),
        GameError::WrongDecision,
    );
    assert_eq!(zone(game, 2), CardZone::Stack);
    assert_eq!(game.execution().window().entries().len(), 3);
    assert_ne!(
        game.execution().window().entries().last().unwrap().id(),
        source
    );
    assert_eq!(game.execution().window().resolving().unwrap().id(), source);
    let saved = game.clone();
    assert_eq!(
        game.execution().resume_copy_resolution(
            game.execution().revision(),
            old_frame,
            B,
            CopyTargetChoice::Retain
        ),
        Err(EffectError::WrongResolutionFrame)
    );
    assert_eq!(game, &saved);
    let finished = reply(
        game,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Card(second))),
    );
    let mut game = finished.into_state();
    assert!(game.pending_decision().is_none());
    assert_eq!(zone(&game, 2), CardZone::Graveyard(B));
    assert_eq!(game.execution().instruction_index(), None);
    assert_eq!(game.execution().window().entries().len(), 3);
    let copies: Vec<_> = game
        .execution()
        .window()
        .entries()
        .iter()
        .filter(|entry| matches!(entry.kind(), StackKind::SpellCopy { .. }))
        .map(|entry| entry.id())
        .collect();
    assert_eq!(copies.len(), 2);
    assert_ne!(copies[0], copies[1]);
    for _ in 0..3 {
        let candidate = resolve(&game);
        assert_eq!(candidate.damage_journal().attempts.len(), 2);
        game = candidate.into_state();
    }
    assert_eq!(marked(&game, first), 6.into());
    assert_eq!(marked(&game, second), 3.into());
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn copy_then_counter_preserves_the_independent_copied_sequence() {
    let game = world(
        100,
        vec![
            (1, A, vec![damage(1), damage(2)]),
            (
                2,
                B,
                vec![Instruction::CopySpell, Instruction::CounterSpell],
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(&game, A, 1, TargetRef::Card(target));
    let (game, source) = cast(&game, B, 2, TargetRef::Stack(original));
    let start = resolve(&game);
    let done = reply(
        start.state(),
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    let game = done.into_state();
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    assert_eq!(zone(&game, 2), CardZone::Graveyard(B));
    assert!(
        game.execution()
            .window()
            .entries()
            .iter()
            .all(|entry| entry.id() != original && entry.id() != source)
    );
    assert_eq!(game.execution().window().entries().len(), 1);
    assert_eq!(marked(&game, target), 0.into());
    let copied = resolve(&game);
    assert_eq!(copied.damage_journal().attempts.len(), 2);
    assert_eq!(marked(copied.state(), target), 3.into());
}

#[test]
fn counter_then_copy_and_repeated_counter_skip_departed_stack_operands() {
    for later in [Instruction::CopySpell, Instruction::CounterSpell] {
        let game = world(
            100,
            vec![
                (1, A, vec![damage(1), shield(5)]),
                (2, B, vec![Instruction::CounterSpell, later]),
            ],
        );
        let target = object(&game, FIRST);
        let (game, original) = cast(&game, A, 1, TargetRef::Card(target));
        let (game, _) = cast(&game, B, 2, TargetRef::Stack(original));
        let done = resolve(&game);
        assert!(done.state().pending_decision().is_none());
        assert_eq!(done.damage_journal(), &DamageJournal::default());
        assert_eq!(zone(done.state(), 1), CardZone::Graveyard(A));
        assert_eq!(zone(done.state(), 2), CardZone::Graveyard(B));
        assert_eq!(marked(done.state(), target), 0.into());
        assert!(done.state().execution().window().entries().is_empty());
    }
}

#[test]
fn initial_all_illegal_aborts_the_whole_program_before_any_effect_or_attempt() {
    let game = world(
        100,
        vec![
            (1, A, vec![shield(4), damage(3)]),
            (2, B, vec![Instruction::ReturnToOwnersHand]),
        ],
    );
    let target = object(&game, FIRST);
    let (game, source) = cast(&game, A, 1, TargetRef::Card(target));
    let (game, _) = cast(&game, B, 2, TargetRef::Card(target));
    let bounced = resolve(&game).into_state();
    assert_eq!(zone(&bounced, FIRST), CardZone::Hand(A));
    let done = resolve(&bounced);
    assert_finished(&done, source, 1);
    assert_eq!(done.damage_journal(), &DamageJournal::default());
    assert_eq!(zone(done.state(), FIRST), CardZone::Hand(A));
    assert_eq!(done.state().execution().prevention_shields().count(), 0);
}

#[test]
fn one_choice_convenience_wrappers_reject_a_second_prompt_atomically() {
    let game = world(
        100,
        vec![
            (1, A, vec![damage(1)]),
            (
                2,
                B,
                vec![
                    Instruction::CopySpell,
                    Instruction::CopySpell,
                    Instruction::CopySpell,
                ],
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(&game, A, 1, TargetRef::Card(target));
    let (game, _) = cast(&game, B, 2, TargetRef::Stack(original));
    let execution = checking(&game);
    let saved = execution.clone();
    assert_eq!(
        execution.resolve_top_with_copy_choice(execution.revision(), B, CopyTargetChoice::Retain),
        Err(EffectError::CopyChoiceRequired)
    );
    assert_eq!(execution, saved);
    let ResolutionProgress::Pending {
        state: execution,
        frame: RuleFrame::CopyTargets(frame),
        ..
    } = execution.begin_resolution(execution.revision()).unwrap()
    else {
        panic!("expected first copy prompt")
    };
    let saved = execution.clone();
    assert_eq!(
        execution.resume_copy_resolution(execution.revision(), &frame, B, CopyTargetChoice::Retain),
        Err(EffectError::CopyChoiceRequired)
    );
    assert_eq!(execution, saved);
    let ResolutionProgress::Pending {
        state: execution,
        frame: RuleFrame::CopyTargets(frame),
        ..
    } = execution
        .resume_resolution(
            execution.revision(),
            &RuleFrame::CopyTargets(frame),
            B,
            DecisionReply::CopyTargets(CopyTargetChoice::Retain),
        )
        .unwrap()
    else {
        panic!("expected second copy prompt")
    };
    assert_eq!(execution.instruction_index(), Some(1));
    assert_eq!(execution.window().entries().len(), 3);
    let saved = execution.clone();
    assert_eq!(
        execution.resume_copy_resolution(execution.revision(), &frame, B, CopyTargetChoice::Retain),
        Err(EffectError::CopyChoiceRequired)
    );
    assert_eq!(execution, saved);
}

#[test]
fn invalid_replies_preserve_prior_work_and_the_exact_second_choice() {
    let game = world(
        100,
        vec![(
            1,
            A,
            vec![damage(2), shield(4), multiplier(2), damage(3), damage(2)],
        )],
    );
    let target = object(&game, FIRST);
    let (start, _) = execute(&game, 1, target);
    let game = start.state();
    let decision = game.pending_decision().unwrap();
    let effect = game.execution().prevention_shields().next().unwrap().id;
    assert_rejected(
        game,
        input(
            game,
            A,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DamageOrder(effect.into()),
            },
        ),
        GameError::WrongChooser,
    );
    assert_rejected(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            },
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    assert_rejected(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: DecisionId(decision.id().0 + 1),
                choice: DecisionReply::DamageOrder(effect.into()),
            },
        ),
        GameError::WrongDecision,
    );
    let mut stale = input(
        game,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::DamageOrder(effect.into()),
        },
    );
    stale.basis.revision -= 1;
    assert_rejected(game, stale, GameError::StaleRevision);
    let saved = game.clone();
    let valid = choose(game, effect);
    assert_eq!(valid, choose(&saved, effect));
    assert_eq!(marked(game, target), 2.into());
    assert_eq!(zone(game, 1), CardZone::Stack);
}

#[test]
fn exact_large_multiplication_and_sequential_addition_never_narrow() {
    let amount = exact(HUGE);
    let game = initialize(
        exact(FOUR_HUGE).plus(&1.into()),
        vec![(
            1,
            A,
            vec![
                multiplier(2),
                Instruction::Damage {
                    amount: amount.clone(),
                },
                Instruction::Damage {
                    amount: amount.clone(),
                },
            ],
        )],
    )
    .unwrap()
    .into_state();
    let target = object(&game, FIRST);
    let (candidate, source) = execute(&game, 1, target);
    assert_finished(&candidate, source, 1);
    assert_eq!(marked(candidate.state(), target), exact(FOUR_HUGE));
    let journal = candidate.damage_journal();
    assert_eq!(journal.applied.len(), 2);
    for event in &journal.applied {
        assert_eq!(event.amount, amount.times(&2.into()));
    }
    assert_eq!(journal.multiplications.len(), 2);
    for event in &journal.multiplications {
        assert_eq!(
            (
                event.before.clone(),
                event.factor.clone(),
                event.after.clone()
            ),
            (amount.clone(), 2.into(), amount.times(&2.into()))
        );
    }
}

#[test]
fn zero_toughness_target_survives_a_real_suspension_until_later_recovery() {
    let game = world(
        3,
        vec![(
            1,
            A,
            vec![
                continuous(PowerToughnessOperation::Set(pt(1, 0))),
                shield(3),
                shield(4),
                damage(5),
                continuous(PowerToughnessOperation::Set(pt(1, 2))),
            ],
        )],
    );
    let target = object(&game, FIRST);
    let (start, source) = execute(&game, 1, target);
    let game = start.state();
    assert_eq!(game.execution().instruction_index(), Some(3));
    assert_eq!(
        game.execution().creature(target).unwrap().toughness,
        0.into()
    );
    assert_eq!(object(game, FIRST), target);
    assert_eq!(zone(game, FIRST), CardZone::Battlefield);
    assert_eq!(zone(game, 1), CardZone::Stack);
    let shield = game.execution().prevention_shields().next().unwrap().id;
    let done = choose(game, shield);
    assert_finished(&done, source, 1);
    assert_eq!(zone(done.state(), FIRST), CardZone::Battlefield);
    assert_eq!(object(done.state(), FIRST), target);
    assert_eq!(
        done.state().execution().creature(target).unwrap().toughness,
        2.into()
    );
    assert_eq!(marked(done.state(), target), 0.into());
    assert_eq!(done.damage_journal().completions.len(), 1);
    assert_eq!(
        done.damage_journal().completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
}

#[test]
fn later_damage_choice_has_fresh_identity_stage_and_exact_instruction_index() {
    let game = world(
        100,
        vec![(
            1,
            A,
            vec![multiplier(2), multiplier(3), damage(1), damage(1)],
        )],
    );
    let target = object(&game, FIRST);
    let (start, source) = execute(&game, 1, target);
    let decision = start.state().pending_decision().unwrap();
    let RuleFrame::DamageOrder(old_frame) = decision.frame() else {
        panic!("expected first damage choice")
    };
    assert_eq!(old_frame.instruction_index(), 2);
    let effects: Vec<_> = start
        .state()
        .execution()
        .damage_multipliers()
        .map(|effect| effect.id)
        .collect();
    let middle = choose(start.state(), effects[0]);
    let game = middle.state();
    let later = game.pending_decision().unwrap();
    let RuleFrame::DamageOrder(frame) = later.frame() else {
        panic!("expected later damage choice")
    };
    assert_eq!(frame.instruction_index(), 3);
    assert_eq!(frame.source(), source);
    assert_eq!(frame.chooser(), old_frame.chooser());
    assert_eq!(frame.options(), old_frame.options());
    assert_ne!(frame.attempt(), old_frame.attempt());
    assert_ne!(later.id(), decision.id());
    assert_eq!(game.execution().pending_damage().unwrap().stage(), 0);
    assert!(
        game.execution()
            .pending_damage()
            .unwrap()
            .used_effects()
            .is_empty()
    );
    assert_eq!(marked(game, target), 6.into());
    assert_eq!(middle.damage_journal().attempts.len(), 1);
    assert_eq!(middle.damage_journal().multiplications.len(), 2);
    assert_eq!(middle.damage_journal().completions.len(), 1);
    assert!(middle.damage_journal().created.is_empty());
    assert_eq!(middle.damage_journal().multiplications[0].stage, 1);
    assert_eq!(middle.damage_journal().multiplications[1].stage, 2);
    let saved = game.clone();
    assert_eq!(
        game.execution().resume_resolution(
            game.execution().revision(),
            &RuleFrame::DamageOrder(old_frame.clone()),
            B,
            DecisionReply::DamageOrder(effects[0].into()),
        ),
        Err(EffectError::WrongResolutionFrame)
    );
    assert_eq!(game, &saved);
    assert_rejected(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DamageOrder(effects[0].into()),
            },
        ),
        GameError::WrongDecision,
    );
    let done = choose(game, effects[1]);
    assert_finished(&done, source, 1);
    assert_eq!(marked(done.state(), target), 12.into());
    assert!(done.damage_journal().attempts.is_empty());
    assert!(done.damage_journal().created.is_empty());
    assert_eq!(done.damage_journal().multiplications.len(), 2);
    assert_eq!(done.damage_journal().multiplications[0].stage, 1);
    assert_eq!(done.damage_journal().multiplications[1].stage, 2);
}
