//! Independent scenarios frozen in damage-prevention-scenarios.md.
//! These native observations do not qualify full replacements, replay or cards.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    characteristics::{PowerToughness, PowerToughnessCounters, PowerToughnessOperation},
    damage::{DamageError, DamageJournal, DamageOutcome, PreventionShieldId},
    effects::{CopyTargetChoice, EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const FIRST: u64 = 100;
const SECOND: u64 = 101;
const HUGE: &str = "900719925474099312345678901234567890";
const HUGE_PLUS_ONE: &str = "900719925474099312345678901234567891";
const HUGE_PLUS_SEVEN: &str = "900719925474099312345678901234567897";
const HUGE_PLUS_HUNDRED: &str = "900719925474099312345678901234567990";
const HUGE_MINUS_THREE: &str = "900719925474099312345678901234567887";

fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}

fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
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

fn context_with(labels: [&str; 5]) -> GameContext {
    let [release, rules, catalogue, programs, profile] = labels.map(String::from);
    GameContext::new(release, rules, catalogue, programs, profile).unwrap()
}

fn context() -> GameContext {
    context_with([
        "damage-tests",
        "rules-20260925",
        "synthetic",
        "finite-shields",
        "individual-turn",
    ])
}

fn instant(id: u64, owner: SeatId, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: CardDestination::Hand,
        program: Program::Instant {
            cost: ManaCost::one(ManaType::Blue),
            instructions: vec![instruction],
        },
    }
}

fn seeds(values: PowerToughness, instructions: Vec<(u64, SeatId, Instruction)>) -> Vec<SeedCard> {
    let mut cards = vec![];
    for (id, controller) in [(FIRST, B), (SECOND, A)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner: A,
            destination: CardDestination::Battlefield { controller },
            program: Program::VanillaCreature {
                power: values.power.clone(),
                toughness: values.toughness.clone(),
            },
        });
    }
    cards.extend(
        instructions
            .into_iter()
            .map(|(id, owner, instruction)| instant(id, owner, instruction)),
    );
    cards
}

fn initialize(cards: Vec<SeedCard>, mana: Vec<FloatingMana>) -> Result<NativeCandidate, GameError> {
    GameState::initialize(GameId(67), context(), vec![A, B], A, cards, mana)
}

fn setup(values: PowerToughness, instructions: Vec<(u64, SeatId, Instruction)>) -> GameState {
    let candidate = initialize(
        seeds(values, instructions),
        [A, B]
            .into_iter()
            .map(|player| FloatingMana {
                player,
                mana_type: ManaType::Blue,
                amount: 100.into(),
            })
            .collect(),
    )
    .unwrap();
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

fn transition(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let before = game.clone();
    let candidate = game.advance(input(game, actor, command)).unwrap();
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
    assert_eq!(game, &before);
    candidate
}

fn reject(game: &GameState, request: GameInput, error: GameError) {
    let before = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &before);
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected settled priority")
    };
    holder
}

fn holding(game: GameState, actor: SeatId) -> GameState {
    if holder(&game) == actor {
        return game;
    }
    let candidate = transition(&game, holder(&game), GameCommand::Pass);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert_eq!(holder(candidate.state()), actor);
    candidate.into_state()
}

fn cast(game: GameState, actor: SeatId, card: u64, target: TargetRef) -> (GameState, StackEntryId) {
    let game = holding(game, actor);
    let candidate = transition(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    let source = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    (candidate.into_state(), source)
}

fn resolve(game: GameState) -> NativeCandidate {
    let first = transition(&game, holder(&game), GameCommand::Pass);
    assert_eq!(first.damage_journal(), &DamageJournal::default());
    transition(first.state(), holder(first.state()), GameCommand::Pass)
}

fn execute(game: GameState, card: u64, target: CardObjectId) -> (NativeCandidate, StackEntryId) {
    let (game, source) = cast(game, A, card, TargetRef::Card(target));
    (resolve(game), source)
}

fn create_shield(
    game: GameState,
    card: u64,
    target: CardObjectId,
) -> (GameState, PreventionShieldId, StackEntryId) {
    let (candidate, source) = execute(game, card, target);
    let id = candidate
        .state()
        .execution()
        .prevention_shields()
        .find(|shield| shield.source == source)
        .unwrap()
        .id;
    assert_eq!(
        candidate.damage_journal(),
        &DamageJournal {
            created: vec![spellwake_engine::damage::DamageEffectCreation::Prevention {
                shield: candidate
                    .state()
                    .execution()
                    .prevention_shield(id)
                    .unwrap()
                    .clone(),
                instruction: 0,
            }],
            ..Default::default()
        }
    );
    (candidate.into_state(), id, source)
}

fn remaining(game: &GameState, id: PreventionShieldId) -> Option<RulesInteger> {
    game.execution()
        .prevention_shield(id)
        .map(|shield| shield.remaining.clone())
}

fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let decision = game.pending_decision().unwrap();
    transition(
        game,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
}

fn assert_applied(
    journal: &DamageJournal,
    source: StackEntryId,
    target: CardObjectId,
    amount: RulesInteger,
) {
    assert_eq!(journal.applied.len(), 1);
    let fact = &journal.applied[0];
    assert_eq!(
        (fact.source, fact.target, &fact.amount),
        (source, target, &amount)
    );
    assert_eq!(journal.completions.len(), 1);
    assert_eq!(journal.completions[0].attempt, fact.attempt);
    assert_eq!(journal.completions[0].outcome, DamageOutcome::Dealt(amount));
}

fn assert_prevented(journal: &DamageJournal) {
    assert!(journal.applied.is_empty());
    assert_eq!(journal.completions.len(), 1);
    assert_eq!(
        journal.completions[0].outcome,
        DamageOutcome::FullyPrevented
    );
}

#[test]
fn unshielded_damage_is_one_positive_occurrence_in_only_its_resolution_append() {
    let game = setup(pt(2, 100), vec![(1, A, damage(5))]);
    let target = object(&game, FIRST);
    let (game, source) = cast(game, A, 1, TargetRef::Card(target));
    assert_eq!(marked(&game, target), 0.into());
    let candidate = resolve(game);
    let saved = candidate.clone();
    let journal = candidate.damage_journal();
    assert_eq!(journal.attempts.len(), 1);
    assert_eq!(
        (
            journal.attempts[0].source,
            journal.attempts[0].target,
            &journal.attempts[0].amount
        ),
        (source, target, &5.into())
    );
    assert!(journal.preventions.is_empty());
    assert_applied(journal, source, target, 5.into());
    assert_eq!(journal.applied[0].attempt, journal.attempts[0].id);
    assert_eq!(marked(candidate.state(), target), 5.into());
    assert_eq!(zone(candidate.state(), 1), CardZone::Graveyard(A));
    let later = transition(
        candidate.state(),
        holder(candidate.state()),
        GameCommand::Pass,
    );
    assert_eq!(later.damage_journal(), &DamageJournal::default());
    assert_eq!(candidate, saved);
}

#[test]
fn one_shield_prevents_exactly_its_capacity_without_a_choice_or_zero_damage_fact() {
    for (capacity, amount, dealt, left) in [(2, 5, 3, None), (5, 5, 0, None), (5, 3, 0, Some(2))] {
        let game = setup(
            pt(2, 100),
            vec![(1, A, shield(capacity)), (2, A, damage(amount))],
        );
        let target = object(&game, FIRST);
        let initial_pt = game.execution().power_toughness(target).unwrap();
        let (game, id, shield_source) = create_shield(game, 1, target);
        assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
        let (candidate, source) = execute(game, 2, target);
        let journal = candidate.damage_journal();
        assert_eq!(journal.attempts.len(), 1);
        assert_eq!(journal.preventions.len(), 1);
        let prevention = &journal.preventions[0];
        assert_eq!(
            (
                prevention.shield,
                prevention.shield_source,
                prevention.target
            ),
            (id, shield_source, target)
        );
        assert_eq!(prevention.attempt, journal.attempts[0].id);
        assert_eq!(prevention.amount, std::cmp::min(capacity, amount).into());
        assert_eq!(prevention.damage_remaining, dealt.into());
        assert_eq!(prevention.shield_remaining, left.unwrap_or(0).into());
        if dealt == 0 {
            assert_prevented(journal);
        } else {
            assert_applied(journal, source, target, dealt.into());
        }
        let game = candidate.into_state();
        assert!(game.pending_decision().is_none());
        assert!(game.execution().pending_damage().is_none());
        assert_eq!(remaining(&game, id), left.map(Into::into));
        assert_eq!(marked(&game, target), dealt.into());
        assert_eq!(
            game.execution().power_toughness(target).unwrap(),
            initial_pt
        );
        assert_eq!(
            game.execution().power_toughness_counters(target),
            Some(PowerToughnessCounters::default())
        );
        let saved = game.clone();
        for _ in 0..3 {
            let _ = remaining(&game, id);
            let _ = game.execution().creature(target);
        }
        assert_eq!(game, saved);
    }
}

#[test]
fn one_finite_shield_keeps_its_remainder_across_different_damage_events() {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(5)),
            (2, A, damage(3)),
            (3, A, damage(1)),
            (4, A, damage(2)),
        ],
    );
    let target = object(&game, FIRST);
    let (mut game, id, _) = create_shield(game, 1, target);
    let mut previous_attempt = None;
    for (card, left, dealt) in [(2, Some(2), 0), (3, Some(1), 0), (4, None, 1)] {
        let (candidate, source) = execute(game, card, target);
        let journal = candidate.damage_journal();
        assert_eq!(journal.attempts.len(), 1);
        assert_ne!(Some(journal.attempts[0].id), previous_attempt);
        previous_attempt = Some(journal.attempts[0].id);
        assert_eq!(journal.preventions.len(), 1);
        if dealt == 0 {
            assert_prevented(journal);
        } else {
            assert_applied(journal, source, target, 1.into());
        }
        game = candidate.into_state();
        assert_eq!(remaining(&game, id), left.map(Into::into));
        assert_eq!(marked(&game, target), dealt.into());
    }
}

#[test]
fn a_later_shield_never_rewrites_prior_damage_or_prior_candidate_facts() {
    let game = setup(
        pt(2, 100),
        vec![(1, A, damage(2)), (2, A, shield(3)), (3, A, damage(4))],
    );
    let target = object(&game, FIRST);
    let (prior, prior_source) = execute(game, 1, target);
    let saved = prior.clone();
    let (game, id, _) = create_shield(prior.state().clone(), 2, target);
    assert_eq!(marked(&game, target), 2.into());
    let (candidate, source) = execute(game, 3, target);
    assert_applied(candidate.damage_journal(), source, target, 1.into());
    assert_eq!(marked(candidate.state(), target), 3.into());
    assert_eq!(remaining(candidate.state(), id), None);
    assert_applied(prior.damage_journal(), prior_source, target, 2.into());
    assert_eq!(prior, saved);
}

fn pending_two(
    amount: i64,
    first: i64,
    second: i64,
) -> (
    NativeCandidate,
    [PreventionShieldId; 2],
    StackEntryId,
    CardObjectId,
) {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(first)),
            (2, A, shield(second)),
            (3, A, damage(amount)),
        ],
    );
    let target = object(&game, FIRST);
    let (game, one, _) = create_shield(game, 1, target);
    let (game, two, _) = create_shield(game, 2, target);
    let (candidate, source) = execute(game, 3, target);
    (candidate, [one, two], source, target)
}

#[test]
fn affected_controller_orders_distinct_shields_and_the_order_changes_their_remainders() {
    let (pending, [one, two], source, target) = pending_two(3, 2, 4);
    let game = pending.state();
    let decision = game.pending_decision().unwrap();
    assert_eq!(decision.chooser(), B);
    assert_eq!(decision.basis(), game.basis());
    let RuleFrame::DamageOrder(frame) = decision.frame() else {
        panic!("expected prevention order")
    };
    let active = game.execution().pending_damage().unwrap();
    assert_eq!(frame.options(), &[one.into(), two.into()]);
    assert_eq!(frame.source(), source);
    assert_eq!(frame.chooser(), B);
    assert_eq!(frame.attempt(), active.attempt().id);
    assert_eq!(active.attempt(), &pending.damage_journal().attempts[0]);
    assert_eq!(active.remaining(), &3.into());
    assert!(active.used_effects().is_empty());
    assert_eq!(active.stage(), 0);
    assert!(pending.damage_journal().preventions.is_empty());
    assert!(pending.damage_journal().completions.is_empty());
    assert_eq!(marked(game, target), 0.into());

    let first = reply(game, DecisionReply::DamageOrder(one.into()));
    assert!(first.damage_journal().attempts.is_empty());
    assert_eq!(
        first
            .damage_journal()
            .preventions
            .iter()
            .map(|fact| fact.shield)
            .collect::<Vec<_>>(),
        vec![one, two]
    );
    assert_prevented(first.damage_journal());
    assert_eq!(remaining(first.state(), one), None);
    assert_eq!(remaining(first.state(), two), Some(3.into()));

    let second = reply(game, DecisionReply::DamageOrder(two.into()));
    assert_eq!(second.damage_journal().preventions.len(), 1);
    assert_prevented(second.damage_journal());
    assert_eq!(remaining(second.state(), one), Some(2.into()));
    assert_eq!(remaining(second.state(), two), Some(1.into()));
    assert_eq!(marked(second.state(), target), 0.into());
    assert_eq!(reply(game, DecisionReply::DamageOrder(two.into())), second);
    assert_ne!(first.state(), second.state());
    assert!(first.state().pending_decision().is_none());
    assert!(second.state().pending_decision().is_none());
}

#[test]
fn equal_shields_stay_distinct_and_excess_damage_still_requires_an_order() {
    let (pending, [one, two], _, _) = pending_two(3, 4, 4);
    assert_ne!(one, two);
    let candidate = reply(pending.state(), DecisionReply::DamageOrder(two.into()));
    assert_eq!(remaining(candidate.state(), one), Some(4.into()));
    assert_eq!(remaining(candidate.state(), two), Some(1.into()));
    assert_eq!(candidate.damage_journal().preventions.len(), 1);
    assert_prevented(candidate.damage_journal());
    for reverse in [false, true] {
        let (pending, [one, two], source, target) = pending_two(10, 2, 3);
        assert!(pending.state().pending_decision().is_some());
        let selected = if reverse { two } else { one };
        let candidate = reply(pending.state(), DecisionReply::DamageOrder(selected.into()));
        assert_eq!(candidate.damage_journal().preventions[0].shield, selected);
        assert_eq!(candidate.damage_journal().preventions.len(), 2);
        assert_applied(candidate.damage_journal(), source, target, 5.into());
        assert_eq!(remaining(candidate.state(), one), None);
        assert_eq!(remaining(candidate.state(), two), None);
    }
}

#[test]
fn partial_choices_resume_one_attempt_without_rewinding_or_reemitting_accepted_work() {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(1)),
            (2, A, shield(2)),
            (3, A, shield(10)),
            (4, A, damage(5)),
            (5, A, shield(7)),
            (6, A, shield(8)),
        ],
    );
    let target = object(&game, FIRST);
    let other = object(&game, SECOND);
    let (game, one, _) = create_shield(game, 1, target);
    let (game, two, _) = create_shield(game, 2, target);
    let (game, three, _) = create_shield(game, 3, target);
    let (game, unrelated, _) = create_shield(game, 5, other);
    let (start, source) = execute(game, 4, target);
    let first = start.state();
    let first_decision = first.pending_decision().unwrap().clone();
    let first_execution_revision = first.execution().revision();
    let first_window_revision = first.execution().window().revision();
    let attempt = first
        .execution()
        .pending_damage()
        .unwrap()
        .attempt()
        .clone();
    assert_eq!(start.damage_journal().attempts, vec![attempt.clone()]);

    let accepted = reply(first, DecisionReply::DamageOrder(one.into()));
    assert!(accepted.damage_journal().attempts.is_empty());
    assert!(accepted.damage_journal().applied.is_empty());
    assert!(accepted.damage_journal().completions.is_empty());
    assert_eq!(accepted.damage_journal().preventions.len(), 1);
    assert_eq!(accepted.damage_journal().preventions[0].shield, one);
    assert_eq!(
        accepted.damage_journal().preventions[0].damage_remaining,
        4.into()
    );
    let second = accepted.state();
    let second_decision = second.pending_decision().unwrap();
    assert_ne!(second_decision.id(), first_decision.id());
    assert_eq!(second_decision.basis(), second.basis());
    assert_eq!(
        second.execution().window().revision(),
        first_window_revision
    );
    assert_eq!(second.execution().revision(), first_execution_revision + 1);
    let RuleFrame::DamageOrder(token) = second_decision.frame() else {
        panic!("expected second order")
    };
    assert_eq!(token.options(), &[two.into(), three.into()]);
    assert_eq!(token.attempt(), attempt.id);
    assert_eq!(token.source(), source);
    let active = second.execution().pending_damage().unwrap();
    assert_eq!(active.attempt(), &attempt);
    assert_eq!(active.remaining(), &4.into());
    assert_eq!(
        active.used_effects().iter().copied().collect::<Vec<_>>(),
        vec![one.into()]
    );
    assert_eq!(active.stage(), 1);
    assert_eq!(marked(second, target), 0.into());
    assert_eq!(remaining(second, one), None);

    for id in [one, unrelated] {
        reject(
            second,
            input(
                second,
                B,
                GameCommand::Reply {
                    decision: second_decision.id(),
                    choice: DecisionReply::DamageOrder(id.into()),
                },
            ),
            GameError::Effect(EffectError::Damage(DamageError::InapplicableEffect)),
        );
    }
    reject(
        second,
        input(
            second,
            B,
            GameCommand::Reply {
                decision: second_decision.id(),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            },
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    reject(
        second,
        input(
            second,
            B,
            GameCommand::Reply {
                decision: first_decision.id(),
                choice: DecisionReply::DamageOrder(two.into()),
            },
        ),
        GameError::WrongDecision,
    );
    reject(
        second,
        input(
            first,
            B,
            GameCommand::Reply {
                decision: first_decision.id(),
                choice: DecisionReply::DamageOrder(one.into()),
            },
        ),
        GameError::StaleRevision,
    );
    for (actor, command) in [
        (B, GameCommand::Pass),
        (
            A,
            GameCommand::Cast {
                spell: object(second, 6),
                target: TargetRef::Card(target),
            },
        ),
    ] {
        reject(
            second,
            input(second, actor, command),
            GameError::DecisionRequired,
        );
    }
    let saved = second.execution().clone();
    assert_eq!(
        saved.settle_checks(saved.revision()),
        Err(EffectError::WrongPhase)
    );
    assert!(matches!(saved.window().phase(), WindowPhase::Resolving(_)));
    assert_eq!(
        saved.begin_resolution(saved.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        saved.resume_resolution(
            first_execution_revision,
            second_decision.frame(),
            B,
            DecisionReply::DamageOrder(two.into())
        ),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(
        saved.resume_resolution(
            saved.revision(),
            first_decision.frame(),
            B,
            DecisionReply::DamageOrder(two.into())
        ),
        Err(EffectError::WrongResolutionFrame)
    );
    assert_eq!(second.execution(), &saved);

    let final_candidate = reply(second, DecisionReply::DamageOrder(two.into()));
    let journal = final_candidate.damage_journal();
    assert!(journal.attempts.is_empty());
    assert_eq!(
        journal
            .preventions
            .iter()
            .map(|fact| (fact.shield, fact.amount.clone()))
            .collect::<Vec<_>>(),
        vec![(two, 2.into()), (three, 2.into())]
    );
    assert!(
        journal
            .preventions
            .iter()
            .all(|fact| fact.attempt == attempt.id)
    );
    assert_prevented(journal);
    assert_eq!(journal.completions[0].attempt, attempt.id);
    assert_eq!(remaining(final_candidate.state(), three), Some(8.into()));
    assert_eq!(
        remaining(final_candidate.state(), unrelated),
        Some(7.into())
    );
    assert_eq!(marked(final_candidate.state(), target), 0.into());
    assert!(
        final_candidate
            .state()
            .execution()
            .pending_damage()
            .is_none()
    );
    reject(
        final_candidate.state(),
        input(
            final_candidate.state(),
            B,
            GameCommand::Reply {
                decision: second_decision.id(),
                choice: DecisionReply::DamageOrder(three.into()),
            },
        ),
        GameError::NoDecision,
    );
    assert_eq!(accepted.damage_journal().preventions.len(), 1);
    assert_eq!(start.damage_journal().attempts, vec![attempt]);
}

#[test]
fn prevention_decisions_keep_all_game_authority_and_basis_checks() {
    let (pending, [one, _], _, _) = pending_two(3, 2, 4);
    let game = pending.state();
    let decision = game.pending_decision().unwrap().id();
    let command = GameCommand::Reply {
        decision,
        choice: DecisionReply::DamageOrder(one.into()),
    };
    let mut wrong = input(game, B, command.clone());
    wrong.basis.game = GameId(68);
    reject(game, wrong, GameError::WrongGame);
    let mut wrong = input(game, B, command.clone());
    wrong.basis.epoch += 1;
    reject(game, wrong, GameError::WrongEpoch);
    let mut wrong = input(game, B, command.clone());
    wrong.basis.revision -= 1;
    reject(game, wrong, GameError::StaleRevision);
    for index in 0..5 {
        let mut labels = [
            "damage-tests",
            "rules-20260925",
            "synthetic",
            "finite-shields",
            "individual-turn",
        ];
        labels[index] = "different";
        let mut wrong = input(game, B, command.clone());
        wrong.basis.context = context_with(labels);
        reject(game, wrong, GameError::WrongContext);
    }
    reject(
        game,
        input(game, A, command.clone()),
        GameError::WrongChooser,
    );
    reject(
        game,
        input(game, SeatId(9), command),
        GameError::UnknownActor,
    );
    reject(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: DecisionId(decision.0 + 1),
                choice: DecisionReply::DamageOrder(one.into()),
            },
        ),
        GameError::WrongDecision,
    );
    let saved = game.clone();
    for _ in 0..3 {
        let _ = game.pending_decision().unwrap().frame();
        let _ = game.execution().pending_damage().unwrap().remaining();
        let _ = game.execution().prevention_shields().count();
    }
    assert_eq!(game, &saved);
}

#[test]
fn shields_on_other_incarnations_do_not_enter_the_choice_domain() {
    let game = setup(
        pt(2, 100),
        vec![(1, A, shield(2)), (2, A, shield(4)), (3, A, damage(3))],
    );
    let first = object(&game, FIRST);
    let second = object(&game, SECOND);
    let (game, one, _) = create_shield(game, 1, first);
    let (game, two, _) = create_shield(game, 2, second);
    let (candidate, source) = execute(game, 3, first);
    assert!(candidate.state().pending_decision().is_none());
    assert_applied(candidate.damage_journal(), source, first, 1.into());
    assert_eq!(candidate.damage_journal().preventions[0].shield, one);
    assert_eq!(remaining(candidate.state(), one), None);
    assert_eq!(remaining(candidate.state(), two), Some(4.into()));
    assert_eq!(marked(candidate.state(), second), 0.into());
}

fn pending_copy(
    game: GameState,
    original_card: u64,
    copy_card: u64,
    target: CardObjectId,
) -> (GameState, StackEntryId) {
    let (game, original) = cast(game, A, original_card, TargetRef::Card(target));
    let (game, _) = cast(game, B, copy_card, TargetRef::Stack(original));
    let candidate = resolve(game);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert!(matches!(
        candidate.state().pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    (candidate.into_state(), original)
}

#[test]
fn copied_damage_retains_its_exact_payload_and_own_cause_after_original_exit() {
    let values = PowerToughness {
        power: 2.into(),
        toughness: exact(HUGE_PLUS_HUNDRED),
    };
    let game = setup(
        values,
        vec![
            (
                1,
                A,
                Instruction::Damage {
                    amount: exact(HUGE_PLUS_SEVEN),
                },
            ),
            (2, B, Instruction::CopySpell),
            (3, A, Instruction::CounterSpell),
            (
                4,
                A,
                Instruction::CreatePreventionShield {
                    amount: exact(HUGE),
                },
            ),
        ],
    );
    let first = object(&game, FIRST);
    let second = object(&game, SECOND);
    let (game, shield_id, _) = create_shield(game, 4, second);
    let (game, original) = pending_copy(game, 1, 2, first);
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Reply {
                decision: game.pending_decision().unwrap().id(),
                choice: DecisionReply::DamageOrder(shield_id.into()),
            },
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let candidate = reply(
        &game,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Card(second))),
    );
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    let game = candidate.into_state();
    let copy = *game
        .execution()
        .window()
        .spell_copies()
        .keys()
        .next()
        .unwrap();
    assert_ne!(copy, original);
    assert_eq!(game.execution().mana(B, ManaType::Blue), 99.into());
    let (game, _) = cast(game, A, 3, TargetRef::Stack(original));
    let countered = resolve(game);
    assert_eq!(countered.damage_journal(), &DamageJournal::default());
    assert_eq!(zone(countered.state(), 1), CardZone::Graveyard(A));
    let candidate = resolve(countered.into_state());
    let journal = candidate.damage_journal();
    assert_eq!(journal.attempts[0].amount, exact(HUGE_PLUS_SEVEN));
    assert_eq!(journal.preventions[0].amount, exact(HUGE));
    assert_applied(journal, copy, second, 7.into());
    assert_eq!(marked(candidate.state(), first), 0.into());
    assert_eq!(marked(candidate.state(), second), 7.into());
    assert_eq!(remaining(candidate.state(), shield_id), None);
    assert!(
        candidate
            .state()
            .execution()
            .window()
            .spell_copies()
            .is_empty()
    );
}

#[test]
fn copied_shields_are_independent_effects_and_survive_copy_cessation() {
    for counter_original in [false, true] {
        let game = setup(
            pt(2, 100),
            vec![
                (
                    1,
                    A,
                    Instruction::CreatePreventionShield {
                        amount: exact(HUGE),
                    },
                ),
                (2, B, Instruction::CopySpell),
                (3, A, Instruction::CounterSpell),
                (4, A, damage(3)),
            ],
        );
        let target = object(&game, FIRST);
        let (game, original) = pending_copy(game, 1, 2, target);
        let game = reply(&game, DecisionReply::CopyTargets(CopyTargetChoice::Retain)).into_state();
        let copy = *game
            .execution()
            .window()
            .spell_copies()
            .keys()
            .next()
            .unwrap();
        let game = if counter_original {
            let (game, _) = cast(game, A, 3, TargetRef::Stack(original));
            resolve(game).into_state()
        } else {
            game
        };
        let candidate = resolve(game);
        let created = candidate
            .state()
            .execution()
            .prevention_shields()
            .find(|shield| shield.source == copy)
            .unwrap()
            .clone();
        assert_eq!(
            candidate.damage_journal(),
            &DamageJournal {
                created: vec![spellwake_engine::damage::DamageEffectCreation::Prevention {
                    shield: created,
                    instruction: 0
                }],
                ..Default::default()
            }
        );
        assert!(
            candidate
                .state()
                .execution()
                .window()
                .spell_copies()
                .is_empty()
        );
        let copy_shield = candidate
            .state()
            .execution()
            .prevention_shields()
            .find(|shield| shield.source == copy)
            .unwrap()
            .clone();
        assert_eq!(
            (copy_shield.target, &copy_shield.remaining),
            (target, &exact(HUGE))
        );
        let game = if counter_original {
            candidate.into_state()
        } else {
            resolve(candidate.into_state()).into_state()
        };
        assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
        let original_shield = game
            .execution()
            .prevention_shields()
            .find(|shield| shield.source == original)
            .cloned();
        assert_eq!(original_shield.is_some(), !counter_original);
        if let Some(original_shield) = &original_shield {
            assert_ne!(original_shield.id, copy_shield.id);
        }
        let (candidate, _) = execute(game, 4, target);
        let candidate = if counter_original {
            candidate
        } else {
            reply(
                candidate.state(),
                DecisionReply::DamageOrder(copy_shield.id.into()),
            )
        };
        assert_prevented(candidate.damage_journal());
        assert_eq!(
            remaining(candidate.state(), copy_shield.id),
            Some(exact(HUGE_MINUS_THREE))
        );
        if let Some(original_shield) = original_shield {
            assert_eq!(
                remaining(candidate.state(), original_shield.id),
                Some(exact(HUGE))
            );
        }
    }
}

#[test]
fn target_departure_prunes_shields_with_other_physical_state_and_leaves_other_targets_alone() {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(2)),
            (2, A, shield(4)),
            (3, A, Instruction::ReturnToOwnersHand),
            (4, A, damage(1)),
            (
                5,
                A,
                Instruction::ContinuousPowerToughness {
                    operation: PowerToughnessOperation::Modify(pt(1, 1)),
                },
            ),
            (
                6,
                A,
                Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: 1.into(),
                        minus_one: 0.into(),
                    },
                },
            ),
        ],
    );
    let target = object(&game, FIRST);
    let other = object(&game, SECOND);
    let (candidate, _) = execute(game, 4, target);
    let (candidate, _) = execute(candidate.into_state(), 5, target);
    let (candidate, _) = execute(candidate.into_state(), 6, target);
    let (game, one, _) = create_shield(candidate.into_state(), 1, target);
    let (game, two, _) = create_shield(game, 2, other);
    let (candidate, _) = execute(game, 3, target);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    let game = candidate.into_state();
    assert_eq!(zone(&game, FIRST), CardZone::Hand(A));
    assert_ne!(object(&game, FIRST), target);
    assert!(game.execution().creature(target).is_none());
    assert!(game.execution().power_toughness_counters(target).is_none());
    assert!(game.execution().prevention_shield(one).is_none());
    assert_eq!(remaining(&game, two), Some(4.into()));
    assert_eq!(game.execution().prevention_shields().count(), 1);
    assert_eq!(marked(&game, other), 0.into());
}

#[test]
fn illegal_targets_and_countered_creation_produce_no_damage_or_new_shield() {
    for instruction in [damage(5), shield(5)] {
        let game = setup(
            pt(2, 100),
            vec![
                (1, A, instruction),
                (2, A, Instruction::ReturnToOwnersHand),
                (3, A, shield(2)),
            ],
        );
        let target = object(&game, FIRST);
        let (game, _, _) = create_shield(game, 3, target);
        let (game, _) = cast(game, A, 1, TargetRef::Card(target));
        let (game, _) = cast(game, A, 2, TargetRef::Card(target));
        let game = resolve(game).into_state();
        assert_eq!(zone(&game, FIRST), CardZone::Hand(A));
        let candidate = resolve(game);
        assert_eq!(candidate.damage_journal(), &DamageJournal::default());
        assert!(
            candidate
                .state()
                .execution()
                .prevention_shields()
                .next()
                .is_none()
        );
        assert!(candidate.state().pending_decision().is_none());
        assert_eq!(zone(candidate.state(), 1), CardZone::Graveyard(A));
        assert_eq!(
            candidate.state().execution().mana(A, ManaType::Blue),
            97.into()
        );
    }
    let game = setup(
        pt(2, 100),
        vec![(1, A, shield(5)), (2, A, Instruction::CounterSpell)],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(game, A, 1, TargetRef::Card(target));
    let (game, _) = cast(game, A, 2, TargetRef::Stack(original));
    let candidate = resolve(game);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert!(
        candidate
            .state()
            .execution()
            .prevention_shields()
            .next()
            .is_none()
    );
    assert_eq!(zone(candidate.state(), 1), CardZone::Graveyard(A));
}

#[test]
fn prevention_uses_current_derived_toughness_and_lethal_facts_survive_sba_pruning() {
    for (defined, change, amount, survives) in [
        (pt(2, 2), pt(0, 2), 5, true),
        (pt(2, 5), pt(0, -3), 4, false),
    ] {
        let game = setup(
            defined,
            vec![
                (
                    1,
                    A,
                    Instruction::ContinuousPowerToughness {
                        operation: PowerToughnessOperation::Modify(change),
                    },
                ),
                (2, A, shield(2)),
                (3, A, damage(amount)),
            ],
        );
        let target = object(&game, FIRST);
        let (candidate, _) = execute(game, 1, target);
        let (game, shield_id, _) = create_shield(candidate.into_state(), 2, target);
        let (candidate, source) = execute(game, 3, target);
        assert_applied(
            candidate.damage_journal(),
            source,
            target,
            (amount - 2).into(),
        );
        assert_eq!(
            candidate.state().execution().creature(target).is_some(),
            survives
        );
        assert!(candidate.state().pending_decision().is_none());
        assert_eq!(remaining(candidate.state(), shield_id), None);
        if survives {
            let creature = candidate.state().execution().creature(target).unwrap();
            assert_eq!((creature.toughness, creature.damage), (4.into(), 3.into()));
        } else {
            assert_eq!(zone(candidate.state(), FIRST), CardZone::Graveyard(A));
            assert_ne!(object(candidate.state(), FIRST), target);
            assert!(
                candidate
                    .state()
                    .execution()
                    .power_toughness(target)
                    .is_none()
            );
        }
        let _ = holder(candidate.state());
    }
    for (capacity, survives) in [(2, true), (1, false)] {
        let game = setup(
            pt(1, 5),
            vec![
                (
                    1,
                    A,
                    Instruction::ContinuousPowerToughness {
                        operation: PowerToughnessOperation::Switch,
                    },
                ),
                (2, A, shield(capacity)),
                (3, A, damage(2)),
            ],
        );
        let target = object(&game, FIRST);
        let (candidate, _) = execute(game, 1, target);
        let (game, _, _) = create_shield(candidate.into_state(), 2, target);
        let (candidate, source) = execute(game, 3, target);
        assert_eq!(
            candidate.state().execution().creature(target).is_some(),
            survives
        );
        if survives {
            assert_prevented(candidate.damage_journal());
        } else {
            assert_applied(candidate.damage_journal(), source, target, 1.into());
        }
    }
}

#[test]
fn huge_prevention_remainders_accumulated_damage_and_lethal_equality_stay_exact() {
    let values = PowerToughness {
        power: 2.into(),
        toughness: exact(HUGE_PLUS_HUNDRED),
    };
    let game = setup(
        values,
        vec![
            (
                1,
                A,
                Instruction::CreatePreventionShield {
                    amount: exact(HUGE_PLUS_SEVEN),
                },
            ),
            (
                2,
                A,
                Instruction::Damage {
                    amount: exact(HUGE),
                },
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, id, _) = create_shield(game, 1, target);
    let (candidate, _) = execute(game, 2, target);
    assert_prevented(candidate.damage_journal());
    assert_eq!(remaining(candidate.state(), id), Some(7.into()));
    assert_eq!(
        candidate.damage_journal().preventions[0].amount,
        exact(HUGE)
    );
    let values = PowerToughness {
        power: 2.into(),
        toughness: exact(HUGE_PLUS_ONE),
    };
    let game = setup(
        values,
        vec![
            (1, A, shield(7)),
            (
                2,
                A,
                Instruction::Damage {
                    amount: exact(HUGE_PLUS_SEVEN),
                },
            ),
            (3, A, damage(1)),
        ],
    );
    let target = object(&game, FIRST);
    let (game, _, _) = create_shield(game, 1, target);
    let (candidate, source) = execute(game, 2, target);
    assert_applied(candidate.damage_journal(), source, target, exact(HUGE));
    assert_eq!(marked(candidate.state(), target), exact(HUGE));
    assert_eq!(
        candidate
            .state()
            .execution()
            .creature(target)
            .unwrap()
            .toughness,
        exact(HUGE_PLUS_ONE)
    );
    let (candidate, source) = execute(candidate.into_state(), 3, target);
    assert_applied(candidate.damage_journal(), source, target, 1.into());
    assert_eq!(zone(candidate.state(), FIRST), CardZone::Graveyard(A));
    assert!(candidate.state().execution().creature(target).is_none());
}

#[test]
fn zero_and_negative_literal_programs_reject_without_restricting_huge_positive_amounts() {
    for amount in [0, -1] {
        for instruction in [damage(amount), shield(amount)] {
            assert_eq!(
                initialize(seeds(pt(2, 100), vec![(1, A, instruction)]), vec![]),
                Err(GameError::Effect(EffectError::InvalidSetup))
            );
        }
    }
    let game = setup(
        pt(2, 100),
        vec![(
            1,
            A,
            Instruction::CreatePreventionShield {
                amount: exact(HUGE),
            },
        )],
    );
    let target = object(&game, FIRST);
    let (game, id, _) = create_shield(game, 1, target);
    assert_eq!(remaining(&game, id), Some(exact(HUGE)));
}

#[test]
fn prevention_neither_stops_zero_toughness_death_nor_expires_at_step_end() {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(7)),
            (
                2,
                A,
                Instruction::ContinuousPowerToughness {
                    operation: PowerToughnessOperation::Set(pt(2, 0)),
                },
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, id, _) = create_shield(game, 1, target);
    let (candidate, _) = execute(game, 2, target);
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert_eq!(remaining(candidate.state(), id), None);
    assert_eq!(zone(candidate.state(), FIRST), CardZone::Graveyard(A));
    let game = setup(pt(2, 100), vec![(1, A, shield(7))]);
    let target = object(&game, FIRST);
    let (game, id, _) = create_shield(game, 1, target);
    let candidate = resolve(game);
    assert_eq!(
        candidate.state().execution().window().phase(),
        WindowPhase::StepEnded
    );
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert_eq!(remaining(candidate.state(), id), Some(7.into()));
}

#[test]
fn invalid_shield_casts_preserve_mana_stack_registry_and_full_game_state() {
    let game = setup(
        pt(2, 100),
        vec![
            (1, A, shield(3)),
            (2, A, damage(1)),
            (3, B, shield(3)),
            (4, A, Instruction::ReturnToOwnersHand),
        ],
    );
    let target = object(&game, FIRST);
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Cast {
                spell: object(&game, 3),
                target: TargetRef::Card(target),
            },
        ),
        GameError::Effect(EffectError::Window(StackWindowError::Priority(
            PriorityError::WrongPlayer,
        ))),
    );
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
                target: TargetRef::Player(A),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let (stacked, source) = cast(game.clone(), A, 2, TargetRef::Card(target));
    reject(
        &stacked,
        input(
            &stacked,
            A,
            GameCommand::Cast {
                spell: object(&stacked, 1),
                target: TargetRef::Stack(source),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let request = input(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Card(target),
        },
    );
    reject(&stacked, request, GameError::StaleRevision);
    let (bounced, _) = execute(game, 4, target);
    reject(
        bounced.state(),
        input(
            bounced.state(),
            A,
            GameCommand::Cast {
                spell: object(bounced.state(), 1),
                target: TargetRef::Card(target),
            },
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let no_mana = initialize(seeds(pt(2, 100), vec![(1, A, shield(3))]), vec![])
        .unwrap()
        .into_state();
    reject(
        &no_mana,
        input(
            &no_mana,
            A,
            GameCommand::Cast {
                spell: object(&no_mana, 1),
                target: TargetRef::Card(object(&no_mana, FIRST)),
            },
        ),
        GameError::Effect(EffectError::InsufficientMana),
    );
}
