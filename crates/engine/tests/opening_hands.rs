//! Independent public-boundary checks for ordinary seven-card London openings.
//! Entropy words are trusted deterministic fixtures, not a randomness claim.
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use std::collections::BTreeSet;

use spellwake_engine::{
    activated::ActivatedAbilityId,
    draw::DrawEvent,
    effects::{EffectError, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, OpeningSetup, RuleFrame,
    },
    mana::{ManaAbilityId, ManaPayment, ManaType},
    object_identity::{
        CardId, GameObjects, ObjectDestination, ObjectId, ObjectIdentityError, ObjectZone,
    },
    opening::{MulliganChoice, OpeningDecision, OpeningError},
    priority::{PriorityPhase, SeatId},
    shuffle::{EntropyError, EntropyTape, SHUFFLE_ALGORITHM},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStage},
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);

fn context() -> GameContext {
    GameContext::new(
        "opening-test".into(),
        "cr-103.5".into(),
        "literal-cards".into(),
        "vanilla".into(),
        "individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn card(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(None, 1.into(), 1.into(), std::collections::BTreeSet::new()),
    }
}

fn setup(seats: Vec<SeatId>, starting: SeatId, size: usize, words: Vec<u64>) -> OpeningSetup {
    let cards = seats
        .iter()
        .flat_map(|&owner| {
            (1..=size).map(move |ordinal| {
                card(
                    u64::from(owner.0) * 100 + ordinal as u64,
                    owner,
                    ObjectDestination::LibraryBottom,
                )
            })
        })
        .collect();
    OpeningSetup {
        seed: GameSeed::with_uniform_life(
            seats,
            starting,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
        entropy: EntropyTape::new(words),
    }
}

fn initialize(setup: OpeningSetup) -> Result<NativeCandidate, GameError> {
    GameState::initialize_opening(GameId(1035), context(), setup)
}

fn game(seats: Vec<SeatId>, starting: SeatId) -> GameState {
    initialize(setup(seats, starting, 12, vec![0; 10_000]))
        .unwrap()
        .into_state()
}

fn cards(game: &GameState) -> &GameObjects {
    game.execution().window().cards()
}

fn hand(game: &GameState, player: SeatId) -> Vec<ObjectId> {
    cards(game)
        .iter()
        .filter(|object| object.zone() == ObjectZone::Hand(player))
        .map(|object| object.id())
        .collect()
}

fn names(registry: &GameObjects, ids: &[ObjectId]) -> Vec<CardId> {
    ids.iter()
        .map(|id| {
            registry
                .get(*id)
                .unwrap()
                .card_id()
                .expect("physical fixture card")
        })
        .collect()
}

fn opening_frame(game: &GameState) -> &OpeningDecision {
    let pending = game.pending_decision().expect("opening must await a reply");
    assert_eq!(pending.basis(), game.basis());
    assert_eq!(pending.chooser(), pending.frame().chooser());
    let RuleFrame::Opening(frame) = pending.frame() else {
        panic!("expected opening frame, got {:?}", pending.frame());
    };
    frame
}

fn assert_declare(game: &GameState, player: SeatId, taken: u8) {
    assert_eq!(
        opening_frame(game),
        &OpeningDecision::Declare {
            player,
            taken,
            can_mulligan: true,
        }
    );
    assert_eq!(game.execution().window().phase(), WindowPhase::Opening);
    assert!(game.turn().is_none());
    assert!(game.outcome().is_none());
    assert!(game.execution().state_based_losses().is_empty());
}

fn assert_bottom(game: &GameState, player: SeatId, count: usize) -> Vec<ObjectId> {
    let actual = hand(game, player);
    assert_eq!(
        actual.len(),
        7,
        "every redraw draws a full seven before bottoming"
    );
    assert_eq!(
        opening_frame(game),
        &OpeningDecision::Bottom {
            player,
            count,
            hand: actual.clone(),
        }
    );
    assert_eq!(game.execution().window().phase(), WindowPhase::Opening);
    assert!(game.turn().is_none());
    actual
}

fn request(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}

fn reply_request(game: &GameState, choice: DecisionReply) -> GameInput {
    let pending = game.pending_decision().unwrap();
    request(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}

fn accept(game: &GameState, input: GameInput) -> NativeCandidate {
    let saved = game.clone();
    let candidate = game.advance(input).unwrap();
    assert_eq!(game, &saved);
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    let next_basis = candidate.state().basis();
    assert_eq!(next_basis.game, game.basis().game);
    assert_eq!(next_basis.context, game.basis().context);
    assert_eq!(next_basis.epoch, game.basis().epoch);
    assert_eq!(next_basis.revision, game.basis().revision + 1);
    candidate
}

fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    accept(game, reply_request(game, choice))
}

fn declare(game: &GameState, choice: MulliganChoice) -> NativeCandidate {
    assert!(matches!(
        opening_frame(game),
        OpeningDecision::Declare { .. }
    ));
    reply(game, DecisionReply::Mulligan(choice))
}

fn bottom(game: &GameState, selected: Vec<ObjectId>) -> NativeCandidate {
    assert!(matches!(
        opening_frame(game),
        OpeningDecision::Bottom { .. }
    ));
    reply(game, DecisionReply::Bottom(selected))
}

fn reject(game: &GameState, input: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(input), Err(expected));
    assert_eq!(game, &saved);
}

fn assert_no_physical_facts(candidate: &NativeCandidate) {
    assert!(candidate.shuffle_journal().is_empty());
    assert!(candidate.draw_journal().events.is_empty());
    assert_eq!(candidate.damage_journal(), &Default::default());
    assert!(candidate.mana_payment().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.ability_activation().is_none());
}

fn assert_draws(candidate: &NativeCandidate, players: &[SeatId]) {
    assert_eq!(candidate.draw_journal().events.len(), players.len() * 7);
    for (events, &expected_player) in candidate
        .draw_journal()
        .events
        .as_chunks::<7>()
        .0
        .iter()
        .zip(players)
    {
        for event in events {
            let DrawEvent::Drawn {
                player,
                before,
                after,
            } = event
            else {
                panic!("an admitted opening cannot run out while redrawing seven");
            };
            assert_eq!(*player, expected_player);
            assert_eq!(before.zone(), ObjectZone::Library(*player));
            assert_eq!(after.zone(), ObjectZone::Hand(*player));
            assert_eq!(
                before.card_id().expect("physical fixture card"),
                after.card_id().expect("physical fixture card")
            );
            assert_ne!(before.id(), after.id());
            assert!(cards(candidate.state()).get(before.id()).is_none());
            assert_eq!(cards(candidate.state()).get(after.id()), Some(after));
        }
    }
}

fn registry(size: usize) -> GameObjects {
    let mut cards = GameObjects::new(0);
    for ordinal in 0..size {
        cards = cards
            .seed_card(
                cards.revision(),
                CardId(ordinal as u64 + 1),
                A,
                ObjectDestination::LibraryBottom,
            )
            .unwrap()
            .next;
    }
    cards
}

#[test]
fn all_six_three_card_fisher_yates_branches_have_exact_order_and_keep_identity() {
    let original = registry(3);
    let ids = original.library(A).to_vec();
    let mut observed = BTreeSet::new();
    let cases = [
        ([0, 0], [2, 3, 1]),
        ([0, 1], [3, 2, 1]),
        ([1, 0], [3, 1, 2]),
        ([1, 1], [1, 3, 2]),
        ([2, 0], [2, 1, 3]),
        ([2, 1], [1, 2, 3]),
    ];
    for (words, expected) in cases {
        let tape = EntropyTape::new(words.to_vec());
        let result = original
            .shuffle_library(original.revision(), A, &tape)
            .unwrap();
        let actual = names(&result.next, result.next.library(A));
        assert_eq!(actual, expected.map(CardId));
        assert!(observed.insert(actual));
        assert_eq!(result.record.player, A);
        assert_eq!(result.record.algorithm, "fisher-yates-u64-tail-v1");
        assert_eq!(result.record.algorithm, SHUFFLE_ALGORITHM);
        assert_eq!(result.record.before, ids);
        assert_eq!(result.record.after, result.next.library(A));
        assert_eq!(
            (result.record.entropy_start, result.record.entropy_end),
            (0, 2)
        );
        assert_eq!(result.entropy.position(), 2);
        assert_eq!(result.entropy.remaining(), 0);
        assert_eq!(tape.position(), 0);
        assert_eq!(result.next.revision(), original.revision() + 1);
        for id in &ids {
            assert_eq!(result.next.get(*id), original.get(*id));
        }
    }
    assert_eq!(observed.len(), 6);
    assert_eq!(original.library(A), ids);
}

#[test]
fn tail_rejection_consumes_max_word_at_bound_three_but_accepts_it_at_bound_two() {
    let original = registry(3);
    let tape = EntropyTape::new(vec![u64::MAX, 2, u64::MAX]);
    let result = original
        .shuffle_library(original.revision(), A, &tape)
        .unwrap();
    assert_eq!(result.next.library(A), original.library(A));
    assert_eq!(
        (result.record.entropy_start, result.record.entropy_end),
        (0, 3)
    );
    assert_eq!(result.entropy.position(), 3);
    assert_eq!(tape.position(), 0);
}

#[test]
fn empty_and_single_card_shuffles_are_real_facts_without_entropy_consumption() {
    for size in [0, 1] {
        let original = registry(size);
        let tape = EntropyTape::new(vec![]);
        let result = original
            .shuffle_library(original.revision(), A, &tape)
            .unwrap();
        assert_eq!(result.record.player, A);
        assert_eq!(result.record.algorithm, SHUFFLE_ALGORITHM);
        assert_eq!(result.record.before, original.library(A));
        assert_eq!(result.record.after, original.library(A));
        assert_eq!(
            (result.record.entropy_start, result.record.entropy_end),
            (0, 0)
        );
        assert_eq!(result.entropy, tape);
        assert_eq!(result.next.revision(), original.revision() + 1);
        assert_eq!(
            result.next.iter().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn raw_shuffle_exhaustion_and_stale_revision_preserve_registry_and_tape() {
    let original = registry(3);
    let saved = original.clone();
    for words in [vec![], vec![0], vec![u64::MAX]] {
        let tape = EntropyTape::new(words);
        let saved_tape = tape.clone();
        assert_eq!(
            original.shuffle_library(original.revision(), A, &tape),
            Err(ObjectIdentityError::Entropy(EntropyError::Exhausted))
        );
        assert_eq!(tape, saved_tape);
        assert_eq!(original, saved);
    }
    assert_eq!(
        original.shuffle_library(original.revision() + 1, A, &EntropyTape::new(vec![0; 2])),
        Err(ObjectIdentityError::StaleRevision)
    );
    assert_eq!(original, saved);
}

#[test]
fn entropy_debug_redacts_words_and_append_preserves_consumed_offset() {
    let secret = 12_345_678_901_234_567_890;
    let tape = EntropyTape::new(vec![secret, 0]);
    let result = registry(3).shuffle_library(3, A, &tape).unwrap();
    let appended = result.entropy.appended(vec![77]).unwrap();
    assert_eq!(appended.position(), 2);
    assert_eq!(appended.remaining(), 1);
    let debug = format!("{tape:?}");
    assert!(!debug.contains(&secret.to_string()));
    assert!(debug.contains("position"));
    assert_eq!(tape.appended(vec![]), Err(EntropyError::EmptySupply));
}

#[test]
fn admission_rejects_invalid_participants_start_owner_and_short_libraries() {
    for seats in [vec![], vec![A], vec![A, A], (0..7).map(SeatId).collect()] {
        assert_eq!(
            initialize(setup(seats, A, 7, vec![0; 100])),
            Err(GameError::Opening(OpeningError::InvalidSetup))
        );
    }
    let mut bad_start = setup(vec![A, B], C, 7, vec![0; 100]);
    assert_eq!(
        initialize(bad_start.clone()),
        Err(GameError::Opening(OpeningError::InvalidSetup))
    );
    bad_start.seed.active = A;
    bad_start
        .seed
        .cards
        .push(card(999, C, ObjectDestination::Command));
    assert_eq!(
        initialize(bad_start),
        Err(GameError::Opening(OpeningError::InvalidSetup))
    );
    for player in [A, B] {
        let mut short = setup(vec![A, B], A, 7, vec![0; 100]);
        let index = short
            .seed
            .cards
            .iter()
            .position(|c| c.owner == player)
            .unwrap();
        short.seed.cards.remove(index);
        assert_eq!(
            initialize(short),
            Err(GameError::Opening(OpeningError::InvalidSetup))
        );
    }
}

#[test]
fn admission_allows_prepared_command_cards_and_explicit_library_positions_only() {
    for (destination, initial_battlefield_order) in [
        (ObjectDestination::Hand, vec![]),
        (ObjectDestination::Graveyard, vec![]),
        (
            ObjectDestination::Battlefield { controller: A },
            vec![CardId(999)],
        ),
        (ObjectDestination::Stack { controller: A }, vec![]),
        (ObjectDestination::Exile, vec![]),
    ] {
        let mut invalid = setup(vec![A, B], A, 7, vec![0; 100]);
        invalid.seed.cards.push(card(999, A, destination));
        invalid.seed.initial_battlefield_order = initial_battlefield_order;
        assert_eq!(
            initialize(invalid),
            Err(GameError::Opening(OpeningError::InvalidSetup))
        );
    }
    let mut valid = setup(vec![A, B], A, 7, vec![0; 100]);
    valid.seed.cards[0].destination = ObjectDestination::LibraryTop;
    valid
        .seed
        .cards
        .push(card(999, B, ObjectDestination::Command));
    let state = initialize(valid).unwrap().into_state();
    let command = cards(&state).current_identity(CardId(999)).unwrap();
    assert_eq!(
        cards(&state).get(command).unwrap().zone(),
        ObjectZone::Command
    );
    assert_eq!(hand(&state, A).len(), 7);
    assert_eq!(hand(&state, B).len(), 7);
}

#[test]
fn initial_shuffle_and_draw_are_actual_ordered_facts_rotated_to_starting_player() {
    let initial = initialize(setup(vec![C, A, B], A, 9, vec![0; 100])).unwrap();
    assert_eq!(
        initial.base(),
        &CandidateBase::Initialize {
            game: GameId(1035),
            context: context()
        }
    );
    assert_eq!(initial.state().basis().revision, 0);
    assert_eq!(initial.shuffle_journal().len(), 3);
    assert_draws(&initial, &[A, B, C]);
    for (index, (&player, record)) in [A, B, C].iter().zip(initial.shuffle_journal()).enumerate() {
        assert_eq!(record.player, player);
        assert_eq!(record.algorithm, SHUFFLE_ALGORITHM);
        assert_eq!(
            (record.entropy_start, record.entropy_end),
            (index * 8, (index + 1) * 8)
        );
        assert_eq!(record.before.len(), 9);
        let mut rotated = record.before.clone();
        rotated.rotate_left(1);
        assert_eq!(record.after, rotated);
        let events = &initial.draw_journal().events[index * 7..index * 7 + 7];
        for (event, &library_id) in events.iter().zip(&record.after) {
            let DrawEvent::Drawn { before, .. } = event else {
                unreachable!()
            };
            assert_eq!(before.id(), library_id);
        }
        assert_eq!(cards(initial.state()).library(player), &record.after[7..]);
    }
    assert_eq!(initial.state().entropy().unwrap().position(), 24);
    assert_declare(initial.state(), A, 0);
    assert!(initial.turn_transitions().is_empty());
    assert!(initial.state().pending_turn_work().is_none());
    assert_eq!(
        initial
            .state()
            .execution()
            .settle_checks(initial.state().execution().revision()),
        Err(EffectError::WrongPhase)
    );
}

#[test]
fn every_gameplay_command_is_blocked_for_every_seat_while_opening() {
    let state = game(vec![A, B, C], B);
    let id = hand(&state, B)[0];
    for actor in [A, B, C] {
        for command in [
            GameCommand::Pass,
            GameCommand::Cast {
                spell: id,
                target: TargetRef::Object(id),
            },
            GameCommand::CastPaid {
                spell: id,
                target: TargetRef::Object(id),
                payment: ManaPayment::default(),
            },
            GameCommand::ActivateMana {
                source: id,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
            GameCommand::ActivateAbility {
                source: id,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(id),
                payment: None,
            },
        ] {
            reject(
                &state,
                request(&state, actor, command),
                GameError::DecisionRequired,
            );
        }
    }
}

#[test]
fn declaration_order_rotates_and_all_votes_precede_cohort_hand_changes() {
    let original = game(vec![A, B, C], B);
    let saved_cards = cards(&original).clone();
    assert_declare(&original, B, 0);
    let first = declare(&original, MulliganChoice::Mulligan);
    assert_no_physical_facts(&first);
    assert_eq!(cards(first.state()), &saved_cards);
    assert_declare(first.state(), C, 0);
    let second = declare(first.state(), MulliganChoice::Mulligan);
    assert_no_physical_facts(&second);
    assert_eq!(cards(second.state()), &saved_cards);
    assert_eq!(second.state().opening().unwrap().mulligans_taken(B), 0);
    assert_eq!(second.state().opening().unwrap().mulligans_taken(C), 0);
    assert_declare(second.state(), A, 0);
    let cohort = declare(second.state(), MulliganChoice::Keep);
    assert_draws(&cohort, &[B, C]);
    assert_eq!(
        cohort
            .shuffle_journal()
            .iter()
            .map(|r| r.player)
            .collect::<Vec<_>>(),
        [B, C]
    );
    assert_eq!(hand(cohort.state(), A), hand(&original, A));
    assert_eq!(cohort.state().opening().unwrap().mulligans_taken(B), 1);
    assert_eq!(cohort.state().opening().unwrap().mulligans_taken(C), 1);
    assert!(cohort.state().opening().unwrap().kept(A));
    assert_declare(cohort.state(), B, 1);
}

#[test]
fn cohort_bottoms_are_staged_until_every_selection_then_moved_as_one_batch() {
    let mut state = game(vec![A, B], B);
    state = declare(&state, MulliganChoice::Mulligan).into_state();
    let redraw = declare(&state, MulliganChoice::Mulligan);
    assert_draws(&redraw, &[B, A]);
    let state = redraw.into_state();
    let b_selected = assert_bottom(&state, B, 1)[3];
    let a_selected = hand(&state, A)[5];
    let saved = cards(&state).clone();
    let b_card = saved
        .get(b_selected)
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let a_card = saved
        .get(a_selected)
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let staged = bottom(&state, vec![b_selected]);
    assert_no_physical_facts(&staged);
    assert_eq!(cards(staged.state()), &saved);
    assert_bottom(staged.state(), A, 1);
    let committed = bottom(staged.state(), vec![a_selected]);
    assert_no_physical_facts(&committed);
    for (player, id, stable) in [(B, b_selected, b_card), (A, a_selected, a_card)] {
        let objects = cards(committed.state());
        assert_eq!(hand(committed.state(), player).len(), 6);
        assert!(objects.get(id).is_none());
        let new_id = objects.current_identity(stable).unwrap();
        assert_eq!(objects.library(player).last(), Some(&new_id));
        assert_eq!(
            objects.get(new_id).unwrap().zone(),
            ObjectZone::Library(player)
        );
    }
    // B's physical batch is applied first even though A's selection closed it.
    let objects = cards(committed.state());
    assert!(objects.current_identity(b_card).unwrap() < objects.current_identity(a_card).unwrap());
    assert_declare(committed.state(), B, 1);
}

#[test]
fn kept_players_are_skipped_permanently_and_never_redraw_again() {
    let original = game(vec![A, B], A);
    let kept_hand = hand(&original, A);
    let state = declare(&original, MulliganChoice::Keep).into_state();
    let first = declare(&state, MulliganChoice::Mulligan);
    assert_eq!(
        first
            .shuffle_journal()
            .iter()
            .map(|r| r.player)
            .collect::<Vec<_>>(),
        [B]
    );
    assert_draws(&first, &[B]);
    let selected = assert_bottom(first.state(), B, 1)[0];
    let state = bottom(first.state(), vec![selected]).into_state();
    assert_declare(&state, B, 1);
    let second = declare(&state, MulliganChoice::Mulligan);
    assert_draws(&second, &[B]);
    assert_eq!(second.shuffle_journal().len(), 1);
    let selected = assert_bottom(second.state(), B, 2)[..2].to_vec();
    let state = bottom(second.state(), selected).into_state();
    assert_declare(&state, B, 2);
    assert!(state.opening().unwrap().kept(A));
    assert_eq!(state.opening().unwrap().mulligans_taken(A), 0);
    assert_eq!(hand(&state, A), kept_hand);
    assert_eq!(hand(&state, B).len(), 5);
}

#[test]
fn literal_london_bottoms_after_each_redraw_and_keeps_the_order_of_selected_cards() {
    let mut state = game(vec![A, B], A);
    state = declare(&state, MulliganChoice::Mulligan).into_state();
    let first = declare(&state, MulliganChoice::Keep);
    assert_draws(&first, &[A]);
    let selected = assert_bottom(first.state(), A, 1)[0];
    let state = bottom(first.state(), vec![selected]).into_state();
    assert_eq!(hand(&state, A).len(), 6);
    assert_declare(&state, A, 1);
    let second = declare(&state, MulliganChoice::Mulligan);
    assert_draws(&second, &[A]);
    let current = assert_bottom(second.state(), A, 2);
    let selection = vec![current[5], current[1]];
    let expected = names(cards(second.state()), &selection);
    let state = bottom(second.state(), selection).into_state();
    assert_eq!(hand(&state, A).len(), 5);
    let library = cards(&state).library(A);
    assert_eq!(
        names(cards(&state), &library[library.len() - 2..]),
        expected
    );
    assert_declare(&state, A, 2);
    let kept = declare(&state, MulliganChoice::Keep);
    assert_no_physical_facts(&kept);
    assert_eq!(hand(kept.state(), A).len(), 5);
    assert!(kept.state().opening().unwrap().complete());
}

#[test]
fn first_multiplayer_mulligan_is_free_but_two_player_first_mulligan_is_charged() {
    for seats in [vec![A, B], vec![A, B, C]] {
        let mut state = game(seats.clone(), A);
        state = declare(&state, MulliganChoice::Mulligan).into_state();
        let mut last = None;
        for _ in 1..seats.len() {
            let candidate = declare(&state, MulliganChoice::Keep);
            state = candidate.state().clone();
            last = Some(candidate);
        }
        assert_draws(last.as_ref().unwrap(), &[A]);
        assert_eq!(state.opening().unwrap().mulligans_taken(A), 1);
        if seats.len() == 2 {
            assert_eq!(state.opening().unwrap().charged(A), 1);
            assert_bottom(&state, A, 1);
        } else {
            assert_eq!(state.opening().unwrap().charged(A), 0);
            assert_eq!(hand(&state, A).len(), 7);
            assert_declare(&state, A, 1);
            let paid = declare(&state, MulliganChoice::Mulligan);
            assert_draws(&paid, &[A]);
            assert_eq!(paid.state().opening().unwrap().charged(A), 1);
            assert_bottom(paid.state(), A, 1);
        }
    }
}

#[test]
fn seven_charged_mulligans_bottom_all_seven_then_force_keep_without_another_prompt() {
    for seats in [vec![A, B], vec![A, B, C]] {
        let mut state = game(seats.clone(), A);
        let total = if seats.len() == 2 { 7 } else { 8 };
        for taken in 1..=total {
            assert_declare(&state, A, taken - 1);
            let mut candidate = declare(&state, MulliganChoice::Mulligan);
            if taken == 1 {
                for _ in 1..seats.len() {
                    candidate = declare(candidate.state(), MulliganChoice::Keep);
                }
            }
            assert_draws(&candidate, &[A]);
            let charge = usize::from(taken - u8::from(seats.len() > 2));
            assert_eq!(candidate.state().opening().unwrap().charged(A), charge);
            if charge == 0 {
                state = candidate.into_state();
            } else {
                let selected = assert_bottom(candidate.state(), A, charge)[..charge].to_vec();
                state = bottom(candidate.state(), selected).into_state();
            }
            assert_eq!(hand(&state, A).len(), 7 - charge);
        }
        let opening = state.opening().unwrap();
        assert!(opening.complete());
        assert!(opening.kept(A));
        assert!(!opening.can_mulligan(A));
        assert_eq!(opening.mulligans_taken(A), total);
        assert_eq!(opening.charged(A), 7);
        assert!(state.pending_decision().is_none());
        assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
        assert!(state.outcome().is_none());
        for &other in &seats[1..] {
            assert_eq!(hand(&state, other).len(), 7);
        }
    }
}

#[test]
fn seven_card_library_redraws_same_cards_with_new_incarnations_before_bottoming() {
    let initial = initialize(setup(vec![A, B], A, 7, vec![0; 1_000])).unwrap();
    let original = initial.into_state();
    let old_ids = hand(&original, A);
    let old_names: BTreeSet<_> = names(cards(&original), &old_ids).into_iter().collect();
    assert!(cards(&original).library(A).is_empty());
    let state = declare(&original, MulliganChoice::Mulligan).into_state();
    let redraw = declare(&state, MulliganChoice::Keep);
    assert_draws(&redraw, &[A]);
    let new_ids = assert_bottom(redraw.state(), A, 1);
    let new_names: BTreeSet<_> = names(cards(redraw.state()), &new_ids).into_iter().collect();
    assert_eq!(old_names, new_names);
    for id in &old_ids {
        assert!(cards(redraw.state()).get(*id).is_none());
    }
    assert!(old_ids.iter().all(|id| !new_ids.contains(id)));
    assert!(cards(redraw.state()).library(A).is_empty());
    assert!(redraw.state().execution().state_based_losses().is_empty());
    assert!(redraw.state().outcome().is_none());
    reject(
        redraw.state(),
        reply_request(redraw.state(), DecisionReply::Bottom(vec![old_ids[0]])),
        GameError::Opening(OpeningError::InvalidBottom),
    );
}

#[test]
fn returning_a_hand_appends_in_canonical_incarnation_order_before_shuffle() {
    let original = game(vec![A, B], A);
    let old_hand = hand(&original, A);
    let expected_hand = names(cards(&original), &old_hand);
    let original_library = cards(&original).library(A).to_vec();
    let state = declare(&original, MulliganChoice::Mulligan).into_state();
    let redrawn = declare(&state, MulliganChoice::Keep);
    let record = &redrawn.shuffle_journal()[0];
    assert_eq!(&record.before[..original_library.len()], original_library);
    let returned = &record.before[original_library.len()..];
    assert_eq!(returned.len(), 7);
    // All zero words rotate left by one; snapshots identify the returned cards
    // even when the subsequent draw has already retired their library handles.
    let mut before_names = Vec::new();
    for id in returned {
        let from_draw = redrawn
            .draw_journal()
            .events
            .iter()
            .find_map(|event| match event {
                DrawEvent::Drawn { before, .. } if before.id() == *id => {
                    Some(before.card_id().expect("physical fixture card"))
                }
                _ => None,
            });
        before_names.push(from_draw.unwrap_or_else(|| {
            cards(redrawn.state())
                .get(*id)
                .unwrap()
                .card_id()
                .expect("physical fixture card")
        }));
    }
    assert_eq!(before_names, expected_hand);
    for id in old_hand {
        assert!(cards(redrawn.state()).get(id).is_none());
    }
}

#[test]
fn stale_basis_wrong_game_context_epoch_actor_token_and_chooser_never_mutate_opening() {
    let state = game(vec![A, B], A);
    let good = reply_request(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    let mut cases = Vec::new();
    let mut wrong = good.clone();
    wrong.basis.game = GameId(9);
    cases.push((wrong, GameError::WrongGame));
    let mut wrong = good.clone();
    wrong.basis.context = GameContext::new(
        "other".into(),
        "r".into(),
        "c".into(),
        "p".into(),
        "i".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    cases.push((wrong, GameError::WrongContext));
    let mut wrong = good.clone();
    wrong.basis.epoch += 1;
    cases.push((wrong, GameError::WrongEpoch));
    let mut wrong = good.clone();
    wrong.basis.revision += 1;
    cases.push((wrong, GameError::StaleRevision));
    let mut wrong = good.clone();
    wrong.actor = C;
    cases.push((wrong, GameError::UnknownActor));
    let mut wrong = good.clone();
    wrong.actor = B;
    cases.push((wrong, GameError::WrongChooser));
    let mut wrong = good.clone();
    wrong.command = GameCommand::Reply {
        decision: DecisionId(state.pending_decision().unwrap().id().0 + 1),
        choice: DecisionReply::Mulligan(MulliganChoice::Keep),
    };
    cases.push((wrong, GameError::WrongDecision));
    for (input, error) in cases {
        reject(&state, input, error);
    }
    let next = declare(&state, MulliganChoice::Keep).into_state();
    reject(&next, good, GameError::StaleRevision);
}

#[test]
fn declaration_and_bottom_frames_reject_other_reply_kinds_unchanged() {
    let initial = game(vec![A, B], A);
    for choice in [
        DecisionReply::Bottom(vec![]),
        DecisionReply::ManaColor(ManaType::Blue),
    ] {
        reject(
            &initial,
            reply_request(&initial, choice),
            GameError::Opening(OpeningError::WrongReply),
        );
    }
    let state = declare(&initial, MulliganChoice::Mulligan).into_state();
    let state = declare(&state, MulliganChoice::Keep).into_state();
    for choice in [
        DecisionReply::Mulligan(MulliganChoice::Keep),
        DecisionReply::ManaColor(ManaType::Blue),
    ] {
        reject(
            &state,
            reply_request(&state, choice),
            GameError::Opening(OpeningError::WrongReply),
        );
    }
}

#[test]
fn bottom_rejects_wrong_count_duplicates_other_hand_library_and_retired_incarnations() {
    let original = game(vec![A, B], A);
    let retired = hand(&original, A)[0];
    let state = declare(&original, MulliganChoice::Mulligan).into_state();
    let state = declare(&state, MulliganChoice::Keep).into_state();
    let selected = assert_bottom(&state, A, 1)[0];
    let state = bottom(&state, vec![selected]).into_state();
    let state = declare(&state, MulliganChoice::Mulligan).into_state();
    let current = assert_bottom(&state, A, 2);
    let foreign = hand(&state, B)[0];
    let library = cards(&state).library(A)[0];
    for selection in [
        vec![],
        vec![current[0]],
        vec![current[0], current[1], current[2]],
        vec![current[0], current[0]],
        vec![current[0], foreign],
        vec![current[0], library],
        vec![current[0], retired],
    ] {
        reject(
            &state,
            reply_request(&state, DecisionReply::Bottom(selection)),
            GameError::Opening(OpeningError::InvalidBottom),
        );
    }
    assert_bottom(&state, A, 2);
}

#[test]
fn cohort_entropy_exhaustion_rolls_back_last_vote_and_prior_cohort_work_then_refill_retries() {
    // Initial shuffles consume 16 words. Eight words then permit only the first
    // cohort shuffle, so failure must discard real preceding work in that input.
    let initial = initialize(setup(vec![A, B], A, 9, vec![0; 24]))
        .unwrap()
        .into_state();
    let accepted_vote = declare(&initial, MulliganChoice::Mulligan).into_state();
    let last_vote = reply_request(
        &accepted_vote,
        DecisionReply::Mulligan(MulliganChoice::Mulligan),
    );
    reject(
        &accepted_vote,
        last_vote.clone(),
        GameError::Effect(EffectError::Window(StackWindowError::Objects(
            ObjectIdentityError::Entropy(EntropyError::Exhausted),
        ))),
    );
    assert_eq!(cards(&accepted_vote), cards(&initial));
    assert_eq!(accepted_vote.entropy().unwrap().position(), 16);
    assert_eq!(accepted_vote.opening().unwrap().mulligans_taken(A), 0);
    assert_eq!(accepted_vote.opening().unwrap().mulligans_taken(B), 0);
    assert_declare(&accepted_vote, B, 0);
    let old_decision = accepted_vote.pending_decision().unwrap().clone();
    let saved = accepted_vote.clone();
    let supplied = accepted_vote
        .supply_entropy(accepted_vote.basis(), vec![0; 8])
        .unwrap();
    assert_eq!(accepted_vote, saved);
    assert_eq!(
        supplied.base(),
        &CandidateBase::Advance(accepted_vote.basis().clone())
    );
    assert_eq!(
        supplied.state().basis().revision,
        accepted_vote.basis().revision + 1
    );
    assert_no_physical_facts(&supplied);
    assert!(supplied.turn_transitions().is_empty());
    assert_eq!(supplied.state().execution(), accepted_vote.execution());
    assert_eq!(supplied.state().opening(), accepted_vote.opening());
    assert_eq!(supplied.state().entropy().unwrap().position(), 16);
    assert_eq!(supplied.state().entropy().unwrap().remaining(), 16);
    let rebound = supplied.state().pending_decision().unwrap();
    assert_ne!(rebound.id(), old_decision.id());
    assert_eq!(rebound.frame(), old_decision.frame());
    assert_eq!(rebound.chooser(), old_decision.chooser());
    assert_eq!(rebound.basis(), supplied.state().basis());
    reject(
        supplied.state(),
        last_vote.clone(),
        GameError::StaleRevision,
    );
    let mut old_token = last_vote;
    old_token.basis = supplied.state().basis().clone();
    reject(supplied.state(), old_token, GameError::WrongDecision);
    let retried = declare(supplied.state(), MulliganChoice::Mulligan);
    assert_draws(&retried, &[A, B]);
    assert_eq!(retried.shuffle_journal().len(), 2);
    assert_eq!(
        retried
            .shuffle_journal()
            .iter()
            .map(|r| (r.player, r.entropy_start, r.entropy_end))
            .collect::<Vec<_>>(),
        [(A, 16, 24), (B, 24, 32)]
    );
    assert_bottom(retried.state(), A, 1);
}

#[test]
fn entropy_supply_rejects_empty_words_and_every_inexact_basis_without_rebinding() {
    let state = game(vec![A, B], A);
    let saved = state.clone();
    assert_eq!(
        state.supply_entropy(state.basis(), vec![]),
        Err(GameError::Entropy(EntropyError::EmptySupply))
    );
    let mut bases = Vec::new();
    let mut basis = state.basis().clone();
    basis.game = GameId(99);
    bases.push((basis, GameError::WrongGame));
    let mut basis = state.basis().clone();
    basis.context = GameContext::new(
        "other".into(),
        "r".into(),
        "c".into(),
        "p".into(),
        "i".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    bases.push((basis, GameError::WrongContext));
    let mut basis = state.basis().clone();
    basis.epoch += 1;
    bases.push((basis, GameError::WrongEpoch));
    let mut basis = state.basis().clone();
    basis.revision += 1;
    bases.push((basis, GameError::StaleRevision));
    for (basis, error) in bases {
        assert_eq!(state.supply_entropy(&basis, vec![0]), Err(error));
    }
    assert_eq!(state, saved);
}

#[test]
fn entropy_supply_between_bottoms_preserves_prior_staged_selection() {
    let state = game(vec![A, B], A);
    let state = declare(&state, MulliganChoice::Mulligan).into_state();
    let state = declare(&state, MulliganChoice::Mulligan).into_state();
    let a = assert_bottom(&state, A, 1)[0];
    let state = bottom(&state, vec![a]).into_state();
    let b = assert_bottom(&state, B, 1)[0];
    let a_name = cards(&state)
        .get(a)
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let b_name = cards(&state)
        .get(b)
        .unwrap()
        .card_id()
        .expect("physical fixture card");
    let supplied = state.supply_entropy(state.basis(), vec![0]).unwrap();
    assert_eq!(supplied.state().execution(), state.execution());
    let final_bottom = bottom(supplied.state(), vec![b]);
    for (player, stable, old) in [(A, a_name, a), (B, b_name, b)] {
        let objects = cards(final_bottom.state());
        assert!(objects.get(old).is_none());
        assert_eq!(
            objects.library(player).last(),
            objects.current_identity(stable).as_ref()
        );
        assert_eq!(hand(final_bottom.state(), player).len(), 6);
    }
}

#[test]
fn all_keeps_handoff_once_through_real_turn_one_untap_to_upkeep_then_normal_drawing() {
    for count in [2, 3, 6] {
        let seats = (0..count).map(SeatId).collect::<Vec<_>>();
        let starting = SeatId(count - 1);
        let initial = game(seats.clone(), starting);
        let initial_cards = cards(&initial).clone();
        let entropy_offset = initial.entropy().unwrap().position();
        let mut state = initial;
        let mut last = None;
        for index in 0..count {
            let player = SeatId((count - 1 + index) % count);
            assert_declare(&state, player, 0);
            let candidate = declare(&state, MulliganChoice::Keep);
            assert_no_physical_facts(&candidate);
            if index + 1 < count {
                assert!(candidate.turn_transitions().is_empty());
            }
            state = candidate.state().clone();
            last = Some(candidate);
        }
        let final_keep = last.unwrap();
        // The actual first-turn marker advances registry revision even though
        // this opening has no battlefield permanents to stamp.
        assert_eq!(cards(&state).revision(), initial_cards.revision() + 1);
        assert_eq!(
            cards(&state).iter().collect::<Vec<_>>(),
            initial_cards.iter().collect::<Vec<_>>()
        );
        for &player in &seats {
            assert_eq!(cards(&state).library(player), initial_cards.library(player));
        }
        assert_eq!(state.entropy().unwrap().position(), entropy_offset);
        assert!(state.opening().unwrap().complete());
        assert!(state.pending_decision().is_none());
        assert_eq!(state.turn().unwrap().position().number, 1);
        assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
        assert_eq!(state.turn().unwrap().stage(), TurnStage::Priority);
        assert_eq!(state.execution().window().active_player(), starting);
        assert_eq!(
            state.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { holder: starting })
        );
        let transitions = final_keep.turn_transitions();
        assert_eq!(transitions.len(), 2);
        assert!(transitions[0].from.is_none());
        assert_eq!(transitions[0].to.step, Step::Untap);
        assert_eq!(transitions[1].from, Some(transitions[0].to));
        assert_eq!(transitions[1].to.step, Step::Upkeep);
        for transition in transitions {
            assert_eq!(transition.to.number, 1);
            assert_eq!(transition.active_before, starting);
            assert_eq!(transition.active_after, starting);
        }
        for &player in &seats {
            assert!(state.opening().unwrap().kept(player));
            assert_eq!(hand(&state, player).len(), 7);
        }
        let mut last_pass = None;
        for _ in 0..count {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("expected normal upkeep priority")
            };
            let candidate = accept(&state, request(&state, holder, GameCommand::Pass));
            assert!(candidate.shuffle_journal().is_empty());
            state = candidate.state().clone();
            last_pass = Some(candidate);
        }
        let after_upkeep = last_pass.unwrap();
        assert_eq!(after_upkeep.turn_transitions().len(), 1);
        assert_eq!(
            after_upkeep.turn_transitions()[0].from.unwrap().step,
            Step::Upkeep
        );
        if count == 2 {
            assert_eq!(state.turn().unwrap().position().step, Step::PrecombatMain);
            assert!(after_upkeep.draw_journal().events.is_empty());
            assert_eq!(hand(&state, starting).len(), 7);
        } else {
            assert_eq!(state.turn().unwrap().position().step, Step::Draw);
            assert_eq!(after_upkeep.draw_journal().events.len(), 1);
            assert_eq!(hand(&state, starting).len(), 8);
        }
        assert_eq!(state.entropy().unwrap().position(), entropy_offset);
    }
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-103.5")
}
