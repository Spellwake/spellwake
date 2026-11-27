#![allow(dead_code, unused_imports)]
//! Public native acceptance scenarios for supported cleanup and shared checks.
//! Prepared zero-toughness/life fixtures test the root boundary without claiming
//! a legal preceding turn, complete cards, public projections or replay.

use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    characteristics::{PowerToughness, PowerToughnessCounters, ResolvedPartSpec},
    checks::{CheckBoundary, CheckChoiceKind, CheckDecision, CheckError, CheckMoveCause},
    combat::CombatRules,
    draw::GameOutcome,
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::{GameSeed, StartingLife},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStage, TurnStart, TurnWork},
};
use std::collections::BTreeSet;

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(91);

fn context() -> GameContext {
    GameContext::new(
        "cleanup-native-acceptance".into(),
        "CR20260925".into(),
        "authored-fixtures".into(),
        "literal-programs".into(),
        "individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn seed(seats: Vec<SeatId>, active: SeatId, cards: Vec<SeedCard>) -> GameSeed {
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
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    }
}

fn initialize(input: GameSeed) -> NativeCandidate {
    GameState::initialize(GameId(812), context(), input).unwrap()
}

fn scheduled(input: GameSeed, step: Step) -> NativeCandidate {
    GameState::initialize_scheduled(GameId(813), context(), input, TurnStart { number: 5, step })
        .unwrap()
}

fn hand(card: u64, owner: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![spellwake_engine::program::Instruction::DealDamage(
                    Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::Literal(3.into()),
                        ),
                        exile_if_dies: false,
                    }),
                )],
            ),
        ),
    }
}

fn old_grave(card: u64, owner: SeatId) -> SeedCard {
    SeedCard {
        destination: ObjectDestination::Graveyard,
        ..hand(card, owner)
    }
}

fn creature(card: u64, owner: SeatId, controller: SeatId, toughness: i64) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::creature(
            None,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn snapshot(game: &GameState, id: ObjectId) -> GameObject {
    game.execution().window().cards().get(id).unwrap().clone()
}

fn grave_cards(game: &GameState, owner: SeatId) -> Vec<CardId> {
    let cards = game.execution().window().cards();
    cards
        .graveyard(owner)
        .iter()
        .map(|id| {
            cards
                .get(*id)
                .unwrap()
                .card_id()
                .expect("physical fixture card")
        })
        .collect()
}

fn domain(game: &GameState) -> &CheckDecision {
    match game.pending_decision().unwrap().frame() {
        RuleFrame::Checks(frame) => frame,
        other => panic!("expected a check decision, got {other:?}"),
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
    assert_eq!(game, &saved);
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    assert_eq!(
        candidate.state().basis().revision,
        game.basis().revision + 1
    );
    candidate
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

fn reject(game: &GameState, attempted: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(attempted), Err(expected));
    assert_eq!(game, &saved);
}

fn reject_reply(game: &GameState, choice: DecisionReply, expected: GameError) {
    let pending = game.pending_decision().unwrap();
    reject(
        game,
        input(
            game,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        ),
        expected,
    );
}

fn holder(game: &GameState) -> SeatId {
    match game.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
        phase => panic!("expected ready priority, got {phase:?}"),
    }
}

fn pass_round(game: &GameState) -> NativeCandidate {
    let mut state = game.clone();
    let participants = game.execution().window().participants().len();
    for _ in 1..participants {
        state = accept(&state, holder(&state), GameCommand::Pass).into_state();
    }
    accept(&state, holder(&state), GameCommand::Pass)
}

fn assert_unmoved(game: &GameState, ids: &[ObjectId]) {
    for id in ids {
        assert_eq!(snapshot(game, *id).zone(), ObjectZone::Battlefield);
        assert!(game.execution().creature(*id).unwrap().is_some());
    }
}

#[test]
fn ordered_cleanup_subset_preserves_old_pile_and_kept_incarnations() {
    let mut cards: Vec<_> = (1..=10).map(|id| hand(id, A)).collect();
    cards.extend([old_grave(90, A), old_grave(91, A), hand(20, B)]);
    let initial = scheduled(seed(vec![A, B], A, cards), Step::Cleanup);
    let game = initial.state();
    let CheckDecision::CleanupDiscard(frame) = domain(game) else {
        panic!("cleanup must request discard first");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.required(), 3);
    assert_eq!(frame.event().pass(), 0);
    assert_eq!(
        frame.boundary(),
        CheckBoundary::Cleanup(game.turn().unwrap().position())
    );
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        (1..=10).map(|id| object(game, id)).collect()
    );
    assert_eq!(game.pending_turn_work(), Some(TurnWork::Cleanup));
    assert_eq!(game.execution().window().phase(), WindowPhase::TurnWork);
    assert!(initial.check_journal().choices.is_empty());
    assert!(initial.check_journal().moves.is_empty());

    let event = frame.event();
    let boundary = frame.boundary();
    let selected = vec![object(game, 8), object(game, 2), object(game, 5)];
    let before: Vec<_> = selected.iter().map(|id| snapshot(game, *id)).collect();
    let kept: Vec<_> = [1, 3, 4, 6, 7, 9, 10, 20, 90, 91]
        .into_iter()
        .map(|id| snapshot(game, object(game, id)))
        .collect();
    let completed = reply(game, DecisionReply::Discard(selected.clone()));
    let next = completed.state();
    assert!(next.pending_decision().is_none());
    assert_eq!(next.execution().window().active_player(), B);
    assert_eq!(next.turn().unwrap().position().number, 6);
    assert_eq!(next.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(holder(next), B);
    assert_eq!(
        grave_cards(next, A),
        vec![CardId(8), CardId(2), CardId(5), CardId(91), CardId(90)]
    );
    for card in kept {
        assert_eq!(snapshot(next, card.id()), card);
    }
    let journal = completed.check_journal().clone();
    assert_eq!(journal.choices.len(), 1);
    let choice = &journal.choices[0];
    assert_eq!(choice.event, event);
    assert_eq!(choice.boundary, boundary);
    assert_eq!(choice.player, A);
    assert_eq!(choice.choice.kind(), CheckChoiceKind::CleanupDiscard);
    assert_eq!(
        choice.choice,
        spellwake_engine::checks::CheckChoice::CleanupDiscard { ordered: selected }
    );
    assert_eq!(journal.moves.len(), 3);
    for (movement, old) in journal.moves.iter().zip(before) {
        assert_eq!(movement.event, event);
        assert_eq!(movement.boundary, boundary);
        assert_eq!(movement.cause, CheckMoveCause::CleanupDiscard);
        assert_eq!(movement.before, old);
        assert_eq!(movement.before.zone(), ObjectZone::Hand(A));
        assert_eq!(movement.after.zone(), ObjectZone::Graveyard(A));
        assert_eq!(
            movement.after.card_id().expect("physical fixture card"),
            movement.before.card_id().expect("physical fixture card")
        );
        assert_eq!(movement.after.owner(), A);
        assert_ne!(movement.before.id(), movement.after.id());
        assert!(
            next.execution()
                .window()
                .cards()
                .get(movement.before.id())
                .is_none()
        );
        assert_eq!(snapshot(next, movement.after.id()), movement.after);
    }
    let later = accept(next, B, GameCommand::Pass);
    assert!(later.check_journal().choices.is_empty());
    assert!(later.check_journal().moves.is_empty());
    assert_eq!(completed.check_journal(), &journal);
}

#[test]
fn ordinary_limit_is_seven_and_only_the_active_hand_matters() {
    for count in [0, 1, 6, 7] {
        let mut cards: Vec<_> = (1..=count).map(|id| hand(id, A)).collect();
        cards.extend((20..=29).map(|id| hand(id, B)));
        let completed = scheduled(seed(vec![A, B], A, cards), Step::Cleanup);
        let next = completed.state();
        assert!(next.pending_decision().is_none());
        assert!(completed.check_journal().choices.is_empty());
        assert!(completed.check_journal().moves.is_empty());
        assert_eq!(next.execution().window().active_player(), B);
        assert_eq!(next.turn().unwrap().position().step, Step::Upkeep);
        assert_eq!(
            next.execution()
                .window()
                .cards()
                .iter()
                .filter(|c| c.zone() == ObjectZone::Hand(A))
                .count(),
            count as usize
        );
        assert_eq!(
            next.execution()
                .window()
                .cards()
                .iter()
                .filter(|c| c.zone() == ObjectZone::Hand(B))
                .count(),
            10
        );
    }
    let mut cards: Vec<_> = (1..=8).map(|id| hand(id, A)).collect();
    cards.extend((20..=29).map(|id| hand(id, B)));
    let initial = scheduled(seed(vec![A, B], A, cards), Step::Cleanup);
    let CheckDecision::CleanupDiscard(frame) = domain(initial.state()) else {
        panic!("expected discard")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.required(), 1);
    assert_eq!(frame.domain().len(), 8);
}

#[test]
fn every_three_card_sba_permutation_is_a_real_owner_choice_above_the_old_pile() {
    for order in [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
    ] {
        let mut cards: Vec<_> = (1..=3).map(|id| creature(id, B, A, 0)).collect();
        cards.extend([old_grave(90, B), old_grave(91, B)]);
        let initial = initialize(seed(vec![A, B], A, cards));
        let game = initial.state();
        let CheckDecision::GraveyardOrder(frame) = domain(game) else {
            panic!("expected graveyard arrangement")
        };
        assert_eq!(frame.chooser(), B);
        assert_eq!(frame.domain().len(), 3);
        assert_eq!(frame.boundary(), CheckBoundary::Priority);
        assert_eq!(frame.event().pass(), 0);
        let event = frame.event();
        let selected: Vec<_> = order.into_iter().map(|id| object(game, id)).collect();
        assert_unmoved(game, &selected);
        assert_eq!(grave_cards(game, B), vec![CardId(91), CardId(90)]);
        assert!(initial.check_journal().choices.is_empty());
        assert!(initial.check_journal().moves.is_empty());
        let completed = reply(game, DecisionReply::OrderGraveyard(selected.clone()));
        let mut expected: Vec<_> = order.into_iter().map(CardId).collect();
        expected.extend([CardId(91), CardId(90)]);
        assert_eq!(grave_cards(completed.state(), B), expected);
        assert!(completed.state().pending_decision().is_none());
        assert_eq!(holder(completed.state()), A);
        assert_eq!(completed.check_journal().choices.len(), 1);
        assert_eq!(
            completed.check_journal().choices[0].choice,
            spellwake_engine::checks::CheckChoice::GraveyardOrder {
                ordered: selected.clone()
            }
        );
        assert_eq!(
            completed.check_journal().choices[0].choice.kind(),
            CheckChoiceKind::GraveyardOrder
        );
        assert_eq!(
            completed
                .check_journal()
                .moves
                .iter()
                .map(|m| m.before.id())
                .collect::<Vec<_>>(),
            selected
        );
        for movement in &completed.check_journal().moves {
            assert_eq!(movement.event, event);
            assert_eq!(movement.cause, CheckMoveCause::StateBasedAction);
            assert_eq!(movement.before.controller(), Some(A));
            assert_eq!(movement.before.owner(), B);
            assert_eq!(movement.after.zone(), ObjectZone::Graveyard(B));
            assert_ne!(movement.before.id(), movement.after.id());
        }
    }
}

#[test]
fn apnap_uses_participant_order_and_owner_while_holding_the_whole_event() {
    let cards = vec![
        creature(31, C, A, 0),
        creature(11, A, B, 0),
        creature(21, B, C, 0),
        creature(32, C, B, 0),
        creature(12, A, C, 0),
        creature(22, B, A, 0),
        old_grave(19, A),
        old_grave(29, B),
        old_grave(39, C),
    ];
    let initial = initialize(seed(vec![C, A, B], A, cards));
    let game = initial.state();
    let all: Vec<_> = [11, 12, 21, 22, 31, 32]
        .into_iter()
        .map(|id| object(game, id))
        .collect();
    assert_eq!(domain(game).chooser(), A);
    assert_eq!(
        match domain(game) {
            CheckDecision::GraveyardOrder(f) => f.domain(),
            _ => panic!("expected graveyard arrangement"),
        }
        .iter()
        .copied()
        .collect::<BTreeSet<_>>(),
        [object(game, 11), object(game, 12)].into_iter().collect()
    );
    let event = domain(game).event();
    let a_order = vec![object(game, 12), object(game, 11)];
    let first = reply(game, DecisionReply::OrderGraveyard(a_order.clone()));
    let after_a = first.state();
    assert_eq!(domain(after_a).chooser(), B);
    assert_eq!(domain(after_a).event(), event);
    assert!(domain(after_a).revision() > domain(game).revision());
    assert_ne!(
        after_a.pending_decision().unwrap().id(),
        game.pending_decision().unwrap().id()
    );
    assert_unmoved(after_a, &all);
    assert_eq!(first.check_journal().choices.len(), 1);
    assert_eq!(
        first.check_journal().choices[0].choice,
        spellwake_engine::checks::CheckChoice::GraveyardOrder {
            ordered: a_order.clone()
        }
    );
    assert!(first.check_journal().moves.is_empty());

    let replay = GameCommand::Reply {
        decision: game.pending_decision().unwrap().id(),
        choice: DecisionReply::OrderGraveyard(a_order.clone()),
    };
    reject(
        after_a,
        input(after_a, A, replay.clone()),
        GameError::WrongDecision,
    );
    reject(after_a, input(game, A, replay), GameError::StaleRevision);

    // A later owner's invalid input cannot roll back the earlier accepted order
    // or commit any member of this simultaneous event.
    reject_reply(
        after_a,
        DecisionReply::OrderGraveyard(vec![object(after_a, 21), object(after_a, 11)]),
        GameError::Effect(EffectError::Check(CheckError::InvalidGraveyardOrder)),
    );
    assert_unmoved(after_a, &all);
    for (owner, old) in [(A, 19), (B, 29), (C, 39)] {
        assert_eq!(grave_cards(after_a, owner), vec![CardId(old)]);
    }
    let second = reply(
        after_a,
        DecisionReply::OrderGraveyard(vec![object(after_a, 21), object(after_a, 22)]),
    );
    let after_b = second.state();
    assert_eq!(domain(after_b).chooser(), C);
    assert_eq!(domain(after_b).event(), event);
    assert_unmoved(after_b, &all);
    assert_eq!(second.check_journal().choices.len(), 1);
    assert_eq!(second.check_journal().choices[0].player, B);
    assert!(second.check_journal().moves.is_empty());
    let completed = reply(
        after_b,
        DecisionReply::OrderGraveyard(vec![object(after_b, 32), object(after_b, 31)]),
    );
    assert_eq!(
        grave_cards(completed.state(), A),
        vec![CardId(12), CardId(11), CardId(19)]
    );
    assert_eq!(
        grave_cards(completed.state(), B),
        vec![CardId(21), CardId(22), CardId(29)]
    );
    assert_eq!(
        grave_cards(completed.state(), C),
        vec![CardId(32), CardId(31), CardId(39)]
    );
    assert_eq!(holder(completed.state()), A);
    assert_eq!(completed.check_journal().choices.len(), 1);
    assert_eq!(completed.check_journal().choices[0].player, C);
    assert_eq!(completed.check_journal().moves.len(), 6);
    assert!(
        completed
            .check_journal()
            .moves
            .iter()
            .all(|m| m.event == event)
    );
    assert_eq!(
        initial.check_journal().choices.len()
            + first.check_journal().choices.len()
            + second.check_journal().choices.len()
            + completed.check_journal().choices.len(),
        3
    );
    assert_eq!(
        first.check_journal().choices[0].choice,
        spellwake_engine::checks::CheckChoice::GraveyardOrder {
            ordered: a_order.clone()
        }
    );
}

#[test]
fn singleton_arrivals_from_one_or_different_owners_do_not_fabricate_choices() {
    for owners in [vec![A], vec![A, B, C]] {
        let mut cards = vec![old_grave(90, A), old_grave(91, B), old_grave(92, C)];
        cards.extend(
            owners
                .iter()
                .enumerate()
                .map(|(i, owner)| creature(i as u64 + 1, *owner, B, 0)),
        );
        let completed = initialize(seed(vec![C, A, B], A, cards));
        assert!(completed.state().pending_decision().is_none());
        assert_eq!(holder(completed.state()), A);
        assert!(completed.check_journal().choices.is_empty());
        assert_eq!(completed.check_journal().moves.len(), owners.len());
        for (i, owner) in owners.iter().enumerate() {
            let old = match *owner {
                A => 90,
                B => 91,
                C => 92,
                _ => unreachable!(),
            };
            assert_eq!(
                grave_cards(completed.state(), *owner),
                vec![CardId(i as u64 + 1), CardId(old)]
            );
        }
        for movement in &completed.check_journal().moves {
            assert_eq!(movement.event.pass(), 0);
            assert_eq!(movement.boundary, CheckBoundary::Priority);
            assert_eq!(movement.cause, CheckMoveCause::StateBasedAction);
        }
    }
}

#[test]
fn loss_waits_for_all_same_event_owner_choices_before_becoming_terminal() {
    let mut prepared = seed(
        vec![A, B],
        A,
        vec![
            creature(1, A, B, 0),
            creature(2, A, A, 0),
            creature(3, B, A, 0),
            creature(4, B, B, 0),
        ],
    );
    prepared.life = vec![
        StartingLife {
            player: A,
            total: 0.into(),
        },
        StartingLife {
            player: B,
            total: 20.into(),
        },
    ];
    let initial = initialize(prepared);
    let game = initial.state();
    let all: Vec<_> = (1..=4).map(|id| object(game, id)).collect();
    assert_eq!(domain(game).chooser(), A);
    assert_eq!(game.outcome(), None);
    assert!(game.execution().state_based_losses().is_empty());
    let first = reply(
        game,
        DecisionReply::OrderGraveyard(vec![object(game, 2), object(game, 1)]),
    );
    let pending = first.state();
    assert_eq!(domain(pending).chooser(), B);
    assert_eq!(domain(pending).event(), domain(game).event());
    assert_eq!(pending.outcome(), None);
    assert!(pending.pending_eliminations().is_empty());
    assert!(pending.execution().state_based_losses().is_empty());
    assert_unmoved(pending, &all);
    assert!(first.check_journal().moves.is_empty());
    let completed = reply(
        pending,
        DecisionReply::OrderGraveyard(vec![object(pending, 3), object(pending, 4)]),
    );
    let ended = completed.state();
    assert_eq!(ended.outcome(), Some(GameOutcome::Winner(B)));
    assert!(ended.pending_decision().is_none());
    assert!(!matches!(
        ended.execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { .. })
    ));
    assert_eq!(completed.check_journal().moves.len(), 4);
    assert_eq!(grave_cards(ended, A), vec![CardId(2), CardId(1)]);
    assert_eq!(grave_cards(ended, B), vec![CardId(3), CardId(4)]);
    reject(
        ended,
        input(ended, B, GameCommand::Pass),
        GameError::GameEnded,
    );
}

#[test]
fn cleanup_terminal_event_does_not_complete_the_calendar_work() {
    let mut input_seed = seed(
        vec![A, B],
        A,
        vec![creature(1, B, A, 0), creature(2, B, B, 0)],
    );
    input_seed.life[0].total = 0.into();
    let initial = scheduled(input_seed, Step::Cleanup);
    let game = initial.state();
    let position = game.turn().unwrap().position();
    assert_eq!(domain(game).boundary(), CheckBoundary::Cleanup(position));
    assert_eq!(domain(game).event().pass(), 1);
    assert_eq!(game.outcome(), None);
    assert!(initial.check_journal().choices.is_empty());
    let completed = reply(
        game,
        DecisionReply::OrderGraveyard(vec![object(game, 2), object(game, 1)]),
    );
    assert_eq!(completed.state().outcome(), Some(GameOutcome::Winner(B)));
    assert_eq!(completed.state().turn().unwrap().position(), position);
    assert_eq!(
        completed.state().turn().unwrap().stage(),
        TurnStage::Work(TurnWork::Cleanup)
    );
    assert_eq!(
        completed.state().execution().window().phase(),
        WindowPhase::TurnWork
    );
    assert!(completed.turn_transitions().is_empty());
    assert_eq!(completed.check_journal().moves.len(), 2);
    assert!(
        completed
            .check_journal()
            .moves
            .iter()
            .all(|m| m.event.pass() == 1)
    );
}

#[test]
fn cleanup_discards_before_zero_toughness_checks_and_real_sba_repeats_cleanup() {
    let mut cards: Vec<_> = (1..=9).map(|id| hand(id, A)).collect();
    cards.extend([
        creature(70, B, A, 0),
        creature(71, B, B, 0),
        old_grave(79, B),
    ]);
    let initial = scheduled(seed(vec![A, B], A, cards), Step::Cleanup);
    let game = initial.state();
    let cleanup = game.turn().unwrap().position();
    let dead = [object(game, 70), object(game, 71)];
    assert!(matches!(domain(game), CheckDecision::CleanupDiscard(_)));
    assert_unmoved(game, &dead);
    let discard_event = domain(game).event();
    let selected = vec![object(game, 8), object(game, 3)];
    let discarded = reply(game, DecisionReply::Discard(selected.clone()));
    let after_discard = discarded.state();
    assert!(matches!(
        domain(after_discard),
        CheckDecision::GraveyardOrder(_)
    ));
    assert_eq!(domain(after_discard).chooser(), B);
    assert_eq!(
        domain(after_discard).boundary(),
        CheckBoundary::Cleanup(cleanup)
    );
    let sba_event = domain(after_discard).event();
    assert_eq!(discard_event.pass(), 0);
    assert_eq!(sba_event.pass(), 1);
    assert_eq!(
        sba_event.started_revision(),
        discard_event.started_revision()
    );
    assert_ne!(sba_event, discard_event);
    assert_unmoved(after_discard, &dead);
    assert_eq!(grave_cards(after_discard, A), vec![CardId(8), CardId(3)]);
    assert_eq!(grave_cards(after_discard, B), vec![CardId(79)]);
    assert_eq!(discarded.check_journal().choices.len(), 1);
    assert_eq!(discarded.check_journal().moves.len(), 2);
    assert!(
        discarded
            .check_journal()
            .moves
            .iter()
            .all(|m| m.cause == CheckMoveCause::CleanupDiscard && m.event == discard_event)
    );

    // These exact source incarnations are gone after discard, even though the
    // stable cards still exist as fresh graveyard objects.
    assert!(
        after_discard
            .execution()
            .window()
            .cards()
            .get(selected[0])
            .is_none()
    );
    reject_reply(
        after_discard,
        DecisionReply::OrderGraveyard(vec![dead[0], selected[0]]),
        GameError::Effect(EffectError::Check(CheckError::InvalidGraveyardOrder)),
    );
    let settled = reply(
        after_discard,
        DecisionReply::OrderGraveyard(vec![dead[1], dead[0]]),
    );
    let exception = settled.state();
    assert_eq!(exception.turn().unwrap().position(), cleanup);
    assert_eq!(exception.turn().unwrap().stage(), TurnStage::Priority);
    assert_eq!(holder(exception), A);
    assert_eq!(
        grave_cards(exception, B),
        vec![CardId(71), CardId(70), CardId(79)]
    );
    assert_eq!(settled.check_journal().choices.len(), 1);
    assert_eq!(
        settled.check_journal().choices[0].choice.kind(),
        CheckChoiceKind::GraveyardOrder
    );
    assert!(
        settled
            .check_journal()
            .moves
            .iter()
            .all(|m| m.event == sba_event)
    );
    let repeated = pass_round(exception);
    let transitions = repeated.turn_transitions();
    assert_eq!(transitions.len(), 3);
    assert_eq!(transitions[0].from, Some(cleanup));
    assert_eq!(transitions[0].to.step, Step::Cleanup);
    assert_eq!(transitions[0].to.cleanup_iteration, 1);
    assert_eq!(transitions[1].to.step, Step::Untap);
    assert_eq!(transitions[1].to.number, 6);
    assert_eq!(transitions[2].to.step, Step::Upkeep);
    assert_eq!(repeated.state().execution().window().active_player(), B);
    assert!(repeated.state().pending_decision().is_none());
    assert!(repeated.check_journal().choices.is_empty());
    assert!(repeated.check_journal().moves.is_empty());
}

#[test]
fn no_excess_cleanup_starts_actual_sba_at_one_without_a_discard_fact() {
    let initial = scheduled(
        seed(
            vec![A, B],
            A,
            vec![creature(1, B, A, 0), creature(2, B, B, 0)],
        ),
        Step::Cleanup,
    );
    let game = initial.state();
    assert!(matches!(domain(game), CheckDecision::GraveyardOrder(_)));
    assert_eq!(domain(game).event().pass(), 1);
    assert!(initial.check_journal().choices.is_empty());
    assert!(initial.check_journal().moves.is_empty());
    let completed = reply(
        game,
        DecisionReply::OrderGraveyard(vec![object(game, 1), object(game, 2)]),
    );
    assert_eq!(completed.check_journal().choices.len(), 1);
    assert_eq!(
        completed.check_journal().choices[0].choice.kind(),
        CheckChoiceKind::GraveyardOrder
    );
    assert_eq!(completed.check_journal().moves.len(), 2);
    assert!(
        completed
            .check_journal()
            .moves
            .iter()
            .all(|m| m.event.pass() == 1 && m.cause == CheckMoveCause::StateBasedAction)
    );
}

#[test]
fn cleanup_exact_reply_guards_reject_without_changing_any_accepted_state() {
    let mut cards: Vec<_> = (1..=9).map(|id| hand(id, A)).collect();
    cards.extend([hand(20, B), old_grave(90, A)]);
    let initial = scheduled(seed(vec![A, B], A, cards), Step::Cleanup);
    let game = initial.state();
    let pending = game.pending_decision().unwrap();
    let valid = vec![object(game, 9), object(game, 1)];
    let command = GameCommand::Reply {
        decision: pending.id(),
        choice: DecisionReply::Discard(valid.clone()),
    };
    reject(
        game,
        input(game, B, command.clone()),
        GameError::WrongChooser,
    );
    reject(
        game,
        input(game, SeatId(999), command.clone()),
        GameError::UnknownActor,
    );
    reject(
        game,
        input(
            game,
            A,
            GameCommand::Reply {
                decision: DecisionId(pending.id().0 + 1),
                choice: DecisionReply::Discard(valid.clone()),
            },
        ),
        GameError::WrongDecision,
    );
    let mut bad = input(game, A, command.clone());
    bad.basis.game = GameId(999);
    reject(game, bad, GameError::WrongGame);
    let mut bad = input(game, A, command.clone());
    bad.basis.context = GameContext::new(
        "different".into(),
        "rules".into(),
        "cards".into(),
        "programs".into(),
        "profile".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    reject(game, bad, GameError::WrongContext);
    let mut bad = input(game, A, command.clone());
    bad.basis.epoch += 1;
    reject(game, bad, GameError::WrongEpoch);
    let mut bad = input(game, A, command);
    bad.basis.revision += 1;
    reject(game, bad, GameError::StaleRevision);
    for rows in [
        vec![],
        vec![valid[0]],
        vec![valid[0], valid[1], object(game, 2)],
        vec![valid[0], valid[0]],
        vec![valid[0], object(game, 20)],
        vec![valid[0], object(game, 90)],
    ] {
        reject_reply(
            game,
            DecisionReply::Discard(rows),
            GameError::Effect(EffectError::Check(CheckError::InvalidDiscard)),
        );
    }
    for wrong in [
        DecisionReply::OrderGraveyard(valid.clone()),
        DecisionReply::ManaColor(ManaType::Blue),
    ] {
        reject_reply(
            game,
            wrong,
            GameError::Effect(EffectError::WrongDecisionKind),
        );
    }
    reject(
        game,
        input(game, A, GameCommand::Pass),
        GameError::TurnWorkRequired(TurnWork::Cleanup),
    );
    let completed = reply(game, DecisionReply::Discard(valid));
    reject(
        completed.state(),
        input(
            completed.state(),
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::Discard(vec![
                    object(completed.state(), 2),
                    object(completed.state(), 3),
                ]),
            },
        ),
        GameError::NoDecision,
    );
}

#[test]
fn pending_order_requires_exact_actor_token_kind_and_full_permutation() {
    let initial = initialize(seed(
        vec![A, B],
        A,
        vec![
            creature(1, B, A, 0),
            creature(2, B, B, 0),
            creature(3, B, A, 0),
            creature(4, A, A, 5),
            old_grave(90, B),
        ],
    ));
    let game = initial.state();
    let pending = game.pending_decision().unwrap();
    let valid: Vec<_> = [3, 1, 2].into_iter().map(|id| object(game, id)).collect();
    reject(
        game,
        input(
            game,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderGraveyard(valid.clone()),
            },
        ),
        GameError::WrongChooser,
    );
    reject(
        game,
        input(
            game,
            B,
            GameCommand::Reply {
                decision: DecisionId(pending.id().0 + 1),
                choice: DecisionReply::OrderGraveyard(valid.clone()),
            },
        ),
        GameError::WrongDecision,
    );
    let mut wrong_basis = input(
        game,
        B,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderGraveyard(valid.clone()),
        },
    );
    wrong_basis.basis.revision += 1;
    reject(game, wrong_basis, GameError::StaleRevision);
    for rows in [
        vec![],
        vec![valid[0], valid[1]],
        vec![valid[0], valid[1], valid[2], object(game, 90)],
        vec![valid[0], valid[0], valid[2]],
        vec![valid[0], valid[1], object(game, 4)],
        vec![valid[0], valid[1], object(game, 90)],
    ] {
        reject_reply(
            game,
            DecisionReply::OrderGraveyard(rows),
            GameError::Effect(EffectError::Check(CheckError::InvalidGraveyardOrder)),
        );
    }
    reject_reply(
        game,
        DecisionReply::Discard(valid.clone()),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    reject(
        game,
        input(game, A, GameCommand::Pass),
        GameError::DecisionRequired,
    );
    let settled = reply(game, DecisionReply::OrderGraveyard(valid));
    assert_eq!(
        grave_cards(settled.state(), B),
        vec![CardId(3), CardId(1), CardId(2), CardId(90)]
    );
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("CR20260925")
}
