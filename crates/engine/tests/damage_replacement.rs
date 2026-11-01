//! Independently derived cases frozen in damage-replacement-scenarios.md before
//! test authoring or new implementation output. This is a bounded native slice.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    damage::{
        DamageEffectId, DamageError, DamageJournal, DamageMultiplier, DamageOutcome,
        PreventionShield,
    },
    effects::{CopyTargetChoice, EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const FIRST: u64 = 100;
const SECOND: u64 = 101;
const HUGE: &str = "900719925474099312345678901234567890";
const PRODUCT: &str = "2702159776422297937037036703703703670";
const PRODUCT_MINUS_SEVEN: &str = "2702159776422297937037036703703703663";
const PRODUCT_PLUS_ONE: &str = "2702159776422297937037036703703703671";

fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}

fn multiplier(factor: i64) -> Instruction {
    Instruction::CreateDamageMultiplier {
        factor: factor.into(),
    }
}

fn damage(amount: i64) -> Instruction {
    Instruction::Damage {
        amount: amount.into(),
    }
}

fn prevention(amount: i64) -> Instruction {
    Instruction::CreatePreventionShield {
        amount: amount.into(),
    }
}

fn context(labels: [&str; 5]) -> GameContext {
    let [release, rules, catalogue, programs, profile] = labels.map(String::from);
    GameContext::new(release, rules, catalogue, programs, profile).unwrap()
}

fn initialize(
    toughness: RulesInteger,
    instructions: Vec<(u64, SeatId, Instruction)>,
    mana: i64,
) -> Result<NativeCandidate, GameError> {
    let mut cards = Vec::new();
    for (card, controller) in [(FIRST, B), (SECOND, A)] {
        cards.push(SeedCard {
            card: CardId(card),
            owner: A,
            destination: CardDestination::Battlefield { controller },
            program: Program::VanillaCreature {
                power: 1.into(),
                toughness: toughness.clone(),
            },
        });
    }
    cards.extend(
        instructions
            .into_iter()
            .map(|(card, owner, instruction)| SeedCard {
                card: CardId(card),
                owner,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::one(ManaType::Blue),
                    instructions: vec![instruction],
                },
            }),
    );
    GameState::initialize(
        GameId(81),
        context([
            "multiplier-tests",
            "rules-20260925",
            "synthetic",
            "literal-multipliers",
            "individual-turn",
        ]),
        vec![A, B],
        A,
        cards,
        [A, B]
            .into_iter()
            .map(|player| FloatingMana {
                player,
                mana_type: ManaType::Blue,
                amount: mana.into(),
            })
            .collect(),
    )
}

fn world(toughness: i64, instructions: Vec<(u64, SeatId, Instruction)>) -> GameState {
    let candidate = initialize(toughness.into(), instructions, 100).unwrap();
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    candidate.into_state()
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
    let mut next_basis = game.basis().clone();
    next_basis.revision += 1;
    assert_eq!(candidate.state().basis(), &next_basis);
    candidate
}

fn reject(game: &GameState, request: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(expected));
    assert_eq!(game, &saved);
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
    let game = if holder(game) != actor {
        let candidate = accept(game, holder(game), GameCommand::Pass);
        assert_eq!(candidate.damage_journal(), &DamageJournal::default());
        candidate.into_state()
    } else {
        game.clone()
    };
    let candidate = accept(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    let entry = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    (candidate.into_state(), entry)
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

fn create_multiplier(
    game: &GameState,
    card: u64,
    target: CardObjectId,
) -> (GameState, DamageMultiplier) {
    let (candidate, source) = execute(game, card, target);
    let record = candidate
        .state()
        .execution()
        .damage_multipliers()
        .find(|record| record.source == source)
        .unwrap()
        .clone();
    assert_eq!(record.target, target);
    assert_eq!(zone(candidate.state(), card), CardZone::Graveyard(A));
    assert_eq!(
        candidate.damage_journal(),
        &DamageJournal {
            created: vec![spellwake_engine::damage::DamageEffectCreation::Multiplier {
                multiplier: record.clone(),
                instruction: 0
            }],
            ..Default::default()
        }
    );
    (candidate.into_state(), record)
}

fn create_shield(
    game: &GameState,
    card: u64,
    target: CardObjectId,
) -> (GameState, PreventionShield) {
    let (candidate, source) = execute(game, card, target);
    let record = candidate
        .state()
        .execution()
        .prevention_shields()
        .find(|record| record.source == source)
        .unwrap()
        .clone();
    assert_eq!(
        candidate.damage_journal(),
        &DamageJournal {
            created: vec![spellwake_engine::damage::DamageEffectCreation::Prevention {
                shield: record.clone(),
                instruction: 0
            }],
            ..Default::default()
        }
    );
    (candidate.into_state(), record)
}

fn choose(game: &GameState, effect: impl Into<DamageEffectId>) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    accept(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::DamageOrder(effect.into()),
        },
    )
}

fn assert_dealt(
    journal: &DamageJournal,
    source: StackEntryId,
    target: CardObjectId,
    amount: RulesInteger,
) {
    assert_eq!(journal.applied.len(), 1);
    let applied = &journal.applied[0];
    assert_eq!(
        (applied.source, applied.target, &applied.amount),
        (source, target, &amount)
    );
    assert_eq!(journal.completions.len(), 1);
    assert_eq!(journal.completions[0].attempt, applied.attempt);
    assert_eq!(journal.completions[0].outcome, DamageOutcome::Dealt(amount));
}

#[test]
fn first_shield_and_multiplier_have_distinct_typed_options_and_order_changes_damage() {
    let game = world(
        100,
        vec![
            (1, A, prevention(4)),
            (2, A, multiplier(2)),
            (3, A, damage(3)),
        ],
    );
    let target = object(&game, FIRST);
    let (game, shield) = create_shield(&game, 1, target);
    let (game, multiplier) = create_multiplier(&game, 2, target);
    let shield_id = DamageEffectId::from(shield.id);
    let multiplier_id = DamageEffectId::from(multiplier.id);
    assert_ne!(shield_id, multiplier_id);
    let (start, source) = execute(&game, 3, target);
    let pending = start.state();
    let decision = pending.pending_decision().unwrap();
    let RuleFrame::DamageOrder(frame) = decision.frame() else {
        panic!("expected damage choice")
    };
    assert_eq!(decision.chooser(), B);
    assert_eq!(frame.chooser(), B);
    assert_eq!(frame.source(), source);
    assert_eq!(frame.options().len(), 2);
    assert!(frame.options().contains(&shield_id));
    assert!(frame.options().contains(&multiplier_id));
    assert_eq!(start.damage_journal().attempts.len(), 1);
    assert_eq!(start.damage_journal().attempts[0].amount, 3.into());

    let doubled = choose(pending, multiplier.id);
    let journal = doubled.damage_journal();
    assert!(journal.attempts.is_empty());
    assert_eq!(journal.multiplications.len(), 1);
    let fact = &journal.multiplications[0];
    assert_eq!(
        (
            fact.multiplier,
            fact.multiplier_source,
            fact.target,
            fact.stage
        ),
        (multiplier.id, multiplier.source, target, 1)
    );
    assert_eq!(
        (&fact.before, &fact.factor, &fact.after),
        (&3.into(), &2.into(), &6.into())
    );
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(
        (
            journal.preventions[0].stage,
            &journal.preventions[0].amount,
            &journal.preventions[0].damage_remaining
        ),
        (2, &4.into(), &2.into())
    );
    assert_dealt(journal, source, target, 2.into());
    assert_eq!(marked(doubled.state(), target), 2.into());
    assert!(
        doubled
            .state()
            .execution()
            .prevention_shield(shield.id)
            .is_none()
    );
    assert_eq!(
        doubled.state().execution().damage_multiplier(multiplier.id),
        Some(&multiplier)
    );

    let prevented = choose(pending, shield.id);
    let journal = prevented.damage_journal();
    assert!(journal.multiplications.is_empty());
    assert!(journal.applied.is_empty());
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(journal.preventions[0].stage, 1);
    assert_eq!(journal.preventions[0].damage_remaining, 0.into());
    assert_eq!(journal.completions.len(), 1);
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
    assert_eq!(
        prevented
            .state()
            .execution()
            .prevention_shield(shield.id)
            .unwrap()
            .remaining,
        1.into()
    );
    assert_eq!(
        prevented
            .state()
            .execution()
            .damage_multiplier(multiplier.id),
        Some(&multiplier)
    );
    assert_eq!(marked(prevented.state(), target), 0.into());
    assert_eq!(choose(pending, multiplier.id), doubled);
}

#[test]
fn two_multipliers_each_apply_once_and_keep_the_original_attempt_amount() {
    let game = world(
        100,
        vec![
            (1, A, multiplier(2)),
            (2, A, multiplier(3)),
            (3, A, damage(5)),
        ],
    );
    let target = object(&game, FIRST);
    let (game, first) = create_multiplier(&game, 1, target);
    let (game, second) = create_multiplier(&game, 2, target);
    let (start, source) = execute(&game, 3, target);
    let attempt = start.damage_journal().attempts[0].clone();
    let done = choose(start.state(), first.id);
    let journal = done.damage_journal();
    assert!(journal.attempts.is_empty());
    assert!(journal.preventions.is_empty());
    assert_eq!(
        journal
            .multiplications
            .iter()
            .map(|fact| (
                fact.multiplier,
                fact.stage,
                fact.before.clone(),
                fact.after.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (first.id, 1, 5.into(), 10.into()),
            (second.id, 2, 10.into(), 30.into())
        ]
    );
    assert!(
        journal
            .multiplications
            .iter()
            .all(|fact| fact.attempt == attempt.id)
    );
    assert_eq!(attempt.amount, 5.into());
    assert_dealt(journal, source, target, 30.into());
    assert!(done.state().pending_decision().is_none());
    assert!(done.state().execution().pending_damage().is_none());
    assert_eq!(
        done.state().execution().damage_multiplier(first.id),
        Some(&first)
    );
    assert_eq!(
        done.state().execution().damage_multiplier(second.id),
        Some(&second)
    );
}

#[test]
fn factor_one_is_an_application_even_without_numeric_change_and_terminates() {
    for count in [1, 2] {
        let game = world(
            100,
            vec![
                (1, A, multiplier(1)),
                (2, A, multiplier(1)),
                (3, A, damage(7)),
            ],
        );
        let target = object(&game, FIRST);
        let (game, first) = create_multiplier(&game, 1, target);
        let game = if count == 2 {
            create_multiplier(&game, 2, target).0
        } else {
            game
        };
        let (start, source) = execute(&game, 3, target);
        let done = if count == 2 {
            choose(start.state(), first.id)
        } else {
            start
        };
        let facts = &done.damage_journal().multiplications;
        assert_eq!(facts.len(), count);
        for (index, fact) in facts.iter().enumerate() {
            assert_eq!(fact.stage, index as u64 + 1);
            assert_eq!(
                (&fact.before, &fact.factor, &fact.after),
                (&7.into(), &1.into(), &7.into())
            );
        }
        assert_dealt(done.damage_journal(), source, target, 7.into());
        assert_eq!(marked(done.state(), target), 7.into());
        assert!(done.state().execution().pending_damage().is_none());
    }
}

#[test]
fn fully_prevented_event_does_not_consume_multiplier_for_a_new_attempt() {
    let game = world(
        100,
        vec![
            (1, A, multiplier(2)),
            (2, A, prevention(4)),
            (3, A, damage(3)),
            (4, A, damage(3)),
        ],
    );
    let target = object(&game, FIRST);
    let (game, multiplier) = create_multiplier(&game, 1, target);
    let (game, shield) = create_shield(&game, 2, target);
    let (start, _) = execute(&game, 3, target);
    let first_attempt = start.damage_journal().attempts[0].clone();
    let prevented = choose(start.state(), shield.id);
    assert!(prevented.damage_journal().multiplications.is_empty());
    assert!(prevented.damage_journal().applied.is_empty());
    assert_eq!(
        prevented.damage_journal().completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
    let (next, source) = execute(prevented.state(), 4, target);
    let next_attempt = next.damage_journal().attempts[0].clone();
    assert_ne!(next_attempt.id, first_attempt.id);
    assert_eq!(next_attempt.amount, 3.into());
    let active = next.state().execution().pending_damage().unwrap();
    assert!(active.used_effects().is_empty());
    assert_eq!(active.stage(), 0);
    let dealt = choose(next.state(), multiplier.id);
    assert_eq!(dealt.damage_journal().multiplications[0].stage, 1);
    assert_eq!(
        dealt.damage_journal().multiplications[0].attempt,
        next_attempt.id
    );
    assert_eq!(dealt.damage_journal().preventions[0].stage, 2);
    assert_eq!(dealt.damage_journal().preventions[0].amount, 1.into());
    assert_dealt(dealt.damage_journal(), source, target, 5.into());
    let later = accept(dealt.state(), holder(dealt.state()), GameCommand::Pass);
    assert_eq!(later.damage_journal(), &DamageJournal::default());
    assert_eq!(start.damage_journal().attempts, vec![first_attempt]);
    assert!(prevented.damage_journal().multiplications.is_empty());
    assert_eq!(next.damage_journal().attempts, vec![next_attempt]);
}

#[test]
fn persistent_multiplier_applies_again_to_a_later_positive_attempt() {
    let game = world(
        100,
        vec![(1, A, multiplier(2)), (2, A, damage(3)), (3, A, damage(3))],
    );
    let target = object(&game, FIRST);
    let (game, multiplier) = create_multiplier(&game, 1, target);
    let (first, first_source) = execute(&game, 2, target);
    let first_attempt = first.damage_journal().attempts[0].clone();
    assert_dealt(first.damage_journal(), first_source, target, 6.into());
    assert_eq!(marked(first.state(), target), 6.into());
    let (second, second_source) = execute(first.state(), 3, target);
    let second_attempt = &second.damage_journal().attempts[0];
    assert_ne!(second_attempt.id, first_attempt.id);
    assert_eq!(second_attempt.amount, 3.into());
    assert_dealt(second.damage_journal(), second_source, target, 6.into());
    assert_eq!(marked(second.state(), target), 12.into());
    for candidate in [&first, &second] {
        let journal = candidate.damage_journal();
        assert_eq!(journal.attempts.len(), 1);
        assert_eq!(journal.multiplications.len(), 1);
        let fact = &journal.multiplications[0];
        assert_eq!(fact.attempt, journal.attempts[0].id);
        assert_eq!((fact.multiplier, fact.stage), (multiplier.id, 1));
        assert_eq!(
            candidate
                .state()
                .execution()
                .damage_multiplier(multiplier.id),
            Some(&multiplier)
        );
    }
    assert_eq!(first.damage_journal().attempts, vec![first_attempt]);
}

struct Mixed {
    start: NativeCandidate,
    first: DamageMultiplier,
    second: DamageMultiplier,
    shield: PreventionShield,
    unrelated_multiplier: DamageMultiplier,
    unrelated_shield: PreventionShield,
    source: StackEntryId,
    target: CardObjectId,
}

fn mixed(toughness: i64) -> Mixed {
    let game = world(
        toughness,
        vec![
            (1, A, multiplier(2)),
            (2, A, multiplier(2)),
            (3, A, prevention(4)),
            (4, A, damage(5)),
            (5, A, multiplier(7)),
            (6, A, prevention(9)),
            (7, A, damage(1)),
        ],
    );
    let target = object(&game, FIRST);
    let other = object(&game, SECOND);
    let (game, first) = create_multiplier(&game, 1, target);
    let (game, second) = create_multiplier(&game, 2, target);
    let (game, shield) = create_shield(&game, 3, target);
    let (game, unrelated_multiplier) = create_multiplier(&game, 5, other);
    let (game, unrelated_shield) = create_shield(&game, 6, other);
    let (start, source) = execute(&game, 4, target);
    Mixed {
        start,
        first,
        second,
        shield,
        unrelated_multiplier,
        unrelated_shield,
        source,
        target,
    }
}

#[test]
fn mixed_partial_replies_preserve_one_attempt_and_order_selected_then_automatic_work() {
    let case = mixed(100);
    let initial = case.start.state();
    let attempt = case.start.damage_journal().attempts[0].clone();
    let old_decision = initial.pending_decision().unwrap();
    let window_revision = initial.execution().window().revision();
    let accepted = choose(initial, case.first.id);
    let journal = accepted.damage_journal();
    assert!(journal.attempts.is_empty());
    assert!(journal.preventions.is_empty());
    assert!(journal.applied.is_empty());
    assert!(journal.completions.is_empty());
    assert_eq!(journal.multiplications.len(), 1);
    assert_eq!(
        (
            journal.multiplications[0].stage,
            &journal.multiplications[0].before,
            &journal.multiplications[0].after
        ),
        (1, &5.into(), &10.into())
    );
    let pending = accepted.state();
    let active = pending.execution().pending_damage().unwrap();
    assert_eq!(active.attempt(), &attempt);
    assert_eq!(active.remaining(), &10.into());
    assert_eq!(active.stage(), 1);
    assert_eq!(
        active.used_effects().iter().copied().collect::<Vec<_>>(),
        vec![case.first.id.into()]
    );
    assert_eq!(pending.execution().window().revision(), window_revision);
    assert_eq!(
        pending.execution().revision(),
        initial.execution().revision() + 1
    );
    let decision = pending.pending_decision().unwrap();
    assert_ne!(decision.id(), old_decision.id());
    assert_eq!(decision.basis(), pending.basis());
    let RuleFrame::DamageOrder(frame) = decision.frame() else {
        panic!("expected damage choice")
    };
    assert_eq!(frame.attempt(), attempt.id);
    assert_eq!(frame.source(), case.source);
    assert_eq!(frame.options().len(), 2);
    assert!(frame.options().contains(&case.second.id.into()));
    assert!(frame.options().contains(&case.shield.id.into()));
    assert_eq!(marked(pending, case.target), 0.into());
    assert_eq!(zone(pending, 4), CardZone::Stack);

    let done = choose(pending, case.shield.id);
    let journal = done.damage_journal();
    assert!(journal.attempts.is_empty());
    assert_eq!(journal.preventions.len(), 1);
    assert_eq!(
        (
            journal.preventions[0].attempt,
            journal.preventions[0].stage,
            &journal.preventions[0].amount,
            &journal.preventions[0].damage_remaining
        ),
        (attempt.id, 2, &4.into(), &6.into())
    );
    assert_eq!(journal.multiplications.len(), 1);
    let fact = &journal.multiplications[0];
    assert_eq!(
        (
            fact.attempt,
            fact.multiplier,
            fact.multiplier_source,
            fact.target,
            fact.stage
        ),
        (
            attempt.id,
            case.second.id,
            case.second.source,
            case.target,
            3
        )
    );
    assert_eq!(
        (&fact.before, &fact.factor, &fact.after),
        (&6.into(), &2.into(), &12.into())
    );
    assert_dealt(journal, case.source, case.target, 12.into());
    assert_eq!(marked(done.state(), case.target), 12.into());
    assert_eq!(
        done.state()
            .execution()
            .damage_multiplier(case.unrelated_multiplier.id),
        Some(&case.unrelated_multiplier)
    );
    assert_eq!(
        done.state()
            .execution()
            .prevention_shield(case.unrelated_shield.id),
        Some(&case.unrelated_shield)
    );
    assert_eq!(accepted.damage_journal().multiplications.len(), 1);
    assert_eq!(case.start.damage_journal().attempts, vec![attempt]);
}

#[test]
fn mixed_pending_rejects_stale_foreign_and_wrong_kind_inputs_without_losing_prior_work() {
    let case = mixed(100);
    let first = case.start.state();
    let old_decision = first.pending_decision().unwrap().clone();
    let accepted = choose(first, case.first.id);
    let game = accepted.state();
    let decision = game.pending_decision().unwrap();
    let command = GameCommand::Reply {
        decision: decision.id(),
        choice: DecisionReply::DamageOrder(case.shield.id.into()),
    };
    for effect in [
        case.first.id.into(),
        case.unrelated_multiplier.id.into(),
        case.unrelated_shield.id.into(),
    ] {
        let request = input(
            game,
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DamageOrder(effect),
            },
        );
        reject(
            game,
            request,
            GameError::Effect(EffectError::Damage(DamageError::InapplicableEffect)),
        );
    }
    reject(
        game,
        input(game, A, command.clone()),
        GameError::WrongChooser,
    );
    reject(
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
    reject(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: old_decision.id(),
                choice: DecisionReply::DamageOrder(case.shield.id.into()),
            },
        ),
        GameError::WrongDecision,
    );
    reject(
        game,
        input(first, B, command.clone()),
        GameError::StaleRevision,
    );
    let mut request = input(game, B, command.clone());
    request.basis.game = GameId(82);
    reject(game, request, GameError::WrongGame);
    let mut request = input(game, B, command.clone());
    request.basis.epoch += 1;
    reject(game, request, GameError::WrongEpoch);
    for index in 0..5 {
        let mut labels = [
            "multiplier-tests",
            "rules-20260925",
            "synthetic",
            "literal-multipliers",
            "individual-turn",
        ];
        labels[index] = "different";
        let mut request = input(game, B, command.clone());
        request.basis.context = context(labels);
        reject(game, request, GameError::WrongContext);
    }
    reject(
        game,
        input(game, B, GameCommand::Pass),
        GameError::DecisionRequired,
    );
    reject(
        game,
        input(
            game,
            A,
            GameCommand::Cast {
                spell: object(game, 7),
                target: TargetRef::Card(case.target),
            },
        ),
        GameError::DecisionRequired,
    );
    let execution = game.execution();
    let saved = execution.clone();
    assert_eq!(
        execution.resume_resolution(
            execution.revision(),
            old_decision.frame(),
            B,
            DecisionReply::DamageOrder(case.shield.id.into())
        ),
        Err(EffectError::WrongResolutionFrame)
    );
    assert_eq!(
        execution.resume_resolution(
            first.execution().revision(),
            decision.frame(),
            B,
            DecisionReply::DamageOrder(case.shield.id.into())
        ),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(
        execution.begin_resolution(execution.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        execution.settle_checks(execution.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(execution, &saved);
    assert_eq!(execution.pending_damage().unwrap().stage(), 1);
    assert_eq!(execution.pending_damage().unwrap().remaining(), &10.into());
    assert_eq!(accepted.damage_journal().multiplications.len(), 1);
    assert_dealt(
        choose(game, case.shield.id).damage_journal(),
        case.source,
        case.target,
        12.into(),
    );
}

#[test]
fn huge_products_and_following_prevention_never_narrow_or_round() {
    let game = world(
        100,
        vec![
            (1, A, multiplier(3)),
            (
                2,
                A,
                Instruction::CreatePreventionShield {
                    amount: exact(PRODUCT_MINUS_SEVEN),
                },
            ),
            (
                3,
                A,
                Instruction::Damage {
                    amount: exact(HUGE),
                },
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, multiplier) = create_multiplier(&game, 1, target);
    let (game, shield) = create_shield(&game, 2, target);
    let (start, source) = execute(&game, 3, target);
    assert_eq!(start.damage_journal().attempts[0].amount, exact(HUGE));
    let done = choose(start.state(), multiplier.id);
    let journal = done.damage_journal();
    let fact = &journal.multiplications[0];
    assert_eq!(
        (&fact.before, &fact.factor, &fact.after),
        (&exact(HUGE), &3.into(), &exact(PRODUCT))
    );
    assert_eq!(fact.stage, 1);
    assert_eq!(journal.preventions[0].amount, exact(PRODUCT_MINUS_SEVEN));
    assert_eq!(journal.preventions[0].damage_remaining, 7.into());
    assert_eq!(journal.preventions[0].stage, 2);
    assert!(
        done.state()
            .execution()
            .prevention_shield(shield.id)
            .is_none()
    );
    assert_dealt(journal, source, target, 7.into());
    assert_eq!(marked(done.state(), target), 7.into());
}

#[test]
fn huge_factor_is_persistent_exact_data_rather_than_a_machine_integer() {
    let game = initialize(
        exact(PRODUCT_PLUS_ONE),
        vec![
            (
                1,
                A,
                Instruction::CreateDamageMultiplier {
                    factor: exact(HUGE),
                },
            ),
            (2, A, damage(3)),
        ],
        100,
    )
    .unwrap()
    .into_state();
    let target = object(&game, FIRST);
    let (game, multiplier) = create_multiplier(&game, 1, target);
    assert_eq!(multiplier.factor, exact(HUGE));
    let (done, source) = execute(&game, 2, target);
    assert!(done.state().pending_decision().is_none());
    assert_eq!(done.damage_journal().attempts[0].amount, 3.into());
    let fact = &done.damage_journal().multiplications[0];
    assert_eq!(
        (&fact.before, &fact.factor, &fact.after),
        (&3.into(), &exact(HUGE), &exact(PRODUCT))
    );
    assert_dealt(done.damage_journal(), source, target, exact(PRODUCT));
    assert_eq!(marked(done.state(), target), exact(PRODUCT));
    assert_eq!(
        done.state().execution().damage_multiplier(multiplier.id),
        Some(&multiplier)
    );
}

#[test]
fn fixed_target_multipliers_do_not_modify_another_live_creature() {
    let game = world(
        100,
        vec![
            (1, A, multiplier(9)),
            (2, A, prevention(2)),
            (3, A, damage(5)),
            (4, A, damage(1)),
        ],
    );
    let first = object(&game, FIRST);
    let other = object(&game, SECOND);
    let (game, multiplier) = create_multiplier(&game, 1, first);
    let (game, shield) = create_shield(&game, 2, first);
    let (done, source) = execute(&game, 3, other);
    assert!(done.state().pending_decision().is_none());
    assert!(done.damage_journal().multiplications.is_empty());
    assert!(done.damage_journal().preventions.is_empty());
    assert_dealt(done.damage_journal(), source, other, 5.into());
    assert_eq!(marked(done.state(), first), 0.into());
    assert_eq!(
        done.state().execution().damage_multiplier(multiplier.id),
        Some(&multiplier)
    );
    assert_eq!(
        done.state().execution().prevention_shield(shield.id),
        Some(&shield)
    );
    let (next, source) = execute(done.state(), 4, first);
    let next = choose(next.state(), multiplier.id);
    assert_dealt(next.damage_journal(), source, first, 7.into());
}

#[test]
fn target_departure_prunes_only_its_multipliers_and_step_end_is_not_expiry() {
    let game = world(
        100,
        vec![
            (1, A, multiplier(2)),
            (2, A, multiplier(3)),
            (3, A, Instruction::ReturnToOwnersHand),
        ],
    );
    let first = object(&game, FIRST);
    let other = object(&game, SECOND);
    let (game, first_multiplier) = create_multiplier(&game, 1, first);
    let (game, other_multiplier) = create_multiplier(&game, 2, other);
    let (done, _) = execute(&game, 3, first);
    assert_eq!(done.damage_journal(), &DamageJournal::default());
    assert_eq!(zone(done.state(), FIRST), CardZone::Hand(A));
    assert_ne!(object(done.state(), FIRST), first);
    assert!(
        done.state()
            .execution()
            .damage_multiplier(first_multiplier.id)
            .is_none()
    );
    assert_eq!(
        done.state()
            .execution()
            .damage_multiplier(other_multiplier.id),
        Some(&other_multiplier)
    );
    let ended = resolve(done.state());
    assert_eq!(
        ended.state().execution().window().phase(),
        WindowPhase::StepEnded
    );
    assert_eq!(ended.damage_journal(), &DamageJournal::default());
    assert_eq!(
        ended
            .state()
            .execution()
            .damage_multiplier(other_multiplier.id),
        Some(&other_multiplier)
    );
}

#[test]
fn copied_multiplier_records_survive_copy_cessation_and_original_countering() {
    for counter_original in [false, true] {
        let game = world(
            100,
            vec![
                (1, A, multiplier(3)),
                (2, B, Instruction::CopySpell),
                (3, A, Instruction::CounterSpell),
                (4, A, damage(2)),
            ],
        );
        let target = object(&game, FIRST);
        let other = object(&game, SECOND);
        let (game, original) = cast(&game, A, 1, TargetRef::Card(target));
        let (game, _) = cast(&game, B, 2, TargetRef::Stack(original));
        let pending = resolve(&game);
        let choice = if counter_original {
            CopyTargetChoice::Change(TargetRef::Card(other))
        } else {
            CopyTargetChoice::Retain
        };
        let decision = pending.state().pending_decision().unwrap();
        let copied = accept(
            pending.state(),
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::CopyTargets(choice),
            },
        );
        let copy = *copied
            .state()
            .execution()
            .window()
            .spell_copies()
            .keys()
            .next()
            .unwrap();
        assert_ne!(copy, original);
        let game = if counter_original {
            let (game, _) = cast(copied.state(), A, 3, TargetRef::Stack(original));
            let countered = resolve(&game);
            assert_eq!(countered.damage_journal(), &DamageJournal::default());
            countered.into_state()
        } else {
            copied.into_state()
        };
        let resolved_copy = resolve(&game);
        let multiplier = resolved_copy
            .state()
            .execution()
            .damage_multipliers()
            .find(|effect| effect.source == copy)
            .unwrap()
            .clone();
        assert_eq!(
            resolved_copy.damage_journal(),
            &DamageJournal {
                created: vec![spellwake_engine::damage::DamageEffectCreation::Multiplier {
                    multiplier,
                    instruction: 0
                }],
                ..Default::default()
            }
        );
        assert!(
            resolved_copy
                .state()
                .execution()
                .window()
                .spell_copies()
                .is_empty()
        );
        let copy_multiplier = resolved_copy
            .state()
            .execution()
            .damage_multipliers()
            .find(|record| record.source == copy)
            .unwrap()
            .clone();
        assert_eq!(copy_multiplier.factor, 3.into());
        assert_eq!(
            copy_multiplier.target,
            if counter_original { other } else { target }
        );
        let game = if counter_original {
            resolved_copy.into_state()
        } else {
            resolve(resolved_copy.state()).into_state()
        };
        assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
        let original_multiplier = game
            .execution()
            .damage_multipliers()
            .find(|record| record.source == original)
            .cloned();
        assert_eq!(original_multiplier.is_some(), !counter_original);
        if let Some(record) = &original_multiplier {
            assert_ne!(record.id, copy_multiplier.id);
            assert_eq!(record.factor, 3.into());
        }
        let affected = if counter_original { other } else { target };
        let (start, source) = execute(&game, 4, affected);
        let done = if counter_original {
            start
        } else {
            choose(start.state(), copy_multiplier.id)
        };
        assert_dealt(
            done.damage_journal(),
            source,
            affected,
            if counter_original {
                6.into()
            } else {
                18.into()
            },
        );
        assert_eq!(
            done.damage_journal().multiplications.len(),
            if counter_original { 1 } else { 2 }
        );
        assert_eq!(
            done.damage_journal().multiplications[0].multiplier_source,
            copy
        );
        assert_eq!(
            done.state()
                .execution()
                .damage_multiplier(copy_multiplier.id),
            Some(&copy_multiplier)
        );
    }
}

#[test]
fn countered_and_all_illegal_multiplier_creations_allocate_no_effect_or_attempt() {
    let game = world(
        100,
        vec![(1, A, multiplier(2)), (2, A, Instruction::CounterSpell)],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(&game, A, 1, TargetRef::Card(target));
    let (game, _) = cast(&game, A, 2, TargetRef::Stack(original));
    let done = resolve(&game);
    assert_eq!(done.damage_journal(), &DamageJournal::default());
    assert_eq!(done.state().execution().damage_multipliers().count(), 0);
    assert_eq!(zone(done.state(), 1), CardZone::Graveyard(A));
    for instruction in [multiplier(2), damage(5)] {
        let game = world(
            100,
            vec![(1, A, instruction), (2, A, Instruction::ReturnToOwnersHand)],
        );
        let target = object(&game, FIRST);
        let (game, _) = cast(&game, A, 1, TargetRef::Card(target));
        let (game, _) = cast(&game, A, 2, TargetRef::Card(target));
        let bounced = resolve(&game);
        assert_ne!(object(bounced.state(), FIRST), target);
        let done = resolve(bounced.state());
        assert_eq!(done.damage_journal(), &DamageJournal::default());
        assert_eq!(done.state().execution().damage_multipliers().count(), 0);
        assert!(done.state().execution().pending_damage().is_none());
        assert!(done.state().pending_decision().is_none());
        assert_eq!(zone(done.state(), 1), CardZone::Graveyard(A));
        assert_eq!(done.state().execution().mana(A, ManaType::Blue), 98.into());
    }
}

#[test]
fn invalid_factors_and_invalid_casts_preserve_the_full_native_state() {
    for factor in [0, -1] {
        assert_eq!(
            initialize(100.into(), vec![(1, A, multiplier(factor))], 100),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
    let game = world(
        100,
        vec![
            (1, A, multiplier(2)),
            (2, B, multiplier(2)),
            (3, A, damage(1)),
        ],
    );
    let target = object(&game, FIRST);
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Cast {
                spell: object(&game, 1),
                target: TargetRef::Card(target),
            },
        ),
        GameError::Effect(EffectError::NotOwnHand),
    );
    reject(
        &game,
        input(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, 1),
                target: TargetRef::Card(object(&game, 3)),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let (stacked, entry) = cast(&game, A, 3, TargetRef::Card(target));
    reject(
        &stacked,
        input(
            &stacked,
            A,
            GameCommand::Cast {
                spell: object(&stacked, 1),
                target: TargetRef::Stack(entry),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let empty = initialize(100.into(), vec![(1, A, multiplier(2))], 0)
        .unwrap()
        .into_state();
    reject(
        &empty,
        input(
            &empty,
            A,
            GameCommand::Cast {
                spell: object(&empty, 1),
                target: TargetRef::Card(object(&empty, FIRST)),
            },
        ),
        GameError::Effect(EffectError::InsufficientMana),
    );
}

#[test]
fn lethal_completion_retains_old_incarnation_facts_after_multiplier_pruning() {
    let case = mixed(12);
    let accepted = choose(case.start.state(), case.first.id);
    assert_eq!(marked(accepted.state(), case.target), 0.into());
    assert_eq!(zone(accepted.state(), FIRST), CardZone::Battlefield);
    let old_decision = accepted.state().pending_decision().unwrap().clone();
    let done = choose(accepted.state(), case.shield.id);
    assert_dealt(done.damage_journal(), case.source, case.target, 12.into());
    assert_eq!(done.damage_journal().multiplications[0].after, 12.into());
    assert_eq!(done.damage_journal().multiplications[0].target, case.target);
    assert_eq!(zone(done.state(), FIRST), CardZone::Graveyard(A));
    assert_ne!(object(done.state(), FIRST), case.target);
    assert!(done.state().execution().creature(case.target).is_none());
    assert!(
        done.state()
            .execution()
            .damage_multiplier(case.first.id)
            .is_none()
    );
    assert!(
        done.state()
            .execution()
            .damage_multiplier(case.second.id)
            .is_none()
    );
    assert_eq!(
        done.state()
            .execution()
            .damage_multiplier(case.unrelated_multiplier.id),
        Some(&case.unrelated_multiplier)
    );
    reject(
        done.state(),
        input(
            done.state(),
            B,
            GameCommand::Reply {
                decision: old_decision.id(),
                choice: DecisionReply::DamageOrder(case.shield.id.into()),
            },
        ),
        GameError::NoDecision,
    );
    let later = accept(done.state(), holder(done.state()), GameCommand::Pass);
    assert_eq!(later.damage_journal(), &DamageJournal::default());
    assert_eq!(done.damage_journal().applied[0].target, case.target);
}
