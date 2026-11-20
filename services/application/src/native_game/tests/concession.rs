//! Concession uses the actual native owner, including retained-input replay.
//! Literal mechanism seeds with genuine vocabulary do not qualify whole cards.
use super::*;
use spellwake_engine::{
    combat::CombatRules, draw::GameOutcome, game::OpeningSetup, setup::GameSeed,
    shuffle::EntropyTape,
};

fn check(source: &GameState, viewer: Uuid, actor: SeatId, winner: SeatId) {
    let before = source.clone();
    let view = scope(source, viewer);
    let original = command(&view, PlayerAction::Concede {});
    let prepared = view.propose(&original).unwrap();
    let direct = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor,
            command: GameCommand::Concede,
        })
        .unwrap();
    assert_eq!(prepared.native_input().command, GameCommand::Concede);
    assert_eq!(prepared.candidate(), &direct);
    assert_eq!(
        prepared.candidate().state().outcome(),
        Some(GameOutcome::Winner(winner))
    );
    input_codec::verify(&view, &prepared);
    let bytes = view.encode_input(&prepared).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record["transition"]["references"], serde_json::json!([]));
    assert_eq!(*source, before);
    let mut stale = original.clone();
    stale.basis.revision = source.basis().revision.checked_add(1).unwrap().into();
    assert!(matches!(
        view.propose(&stale),
        Err(NativeBridgeError::StaleBasis)
    ));
    let terminal = direct.into_state();
    let frozen = terminal.clone();
    let terminal_view = scope(&terminal, viewer);
    assert!(
        matches!(terminal_view.propose(&command(&terminal_view,PlayerAction::Concede {})),Err(NativeBridgeError::Native(error)) if matches!(*error,GameError::GameEnded))
    );
    assert!(restore_input(&terminal, binding(&terminal, viewer), &bytes).is_err());
    assert_eq!(terminal, frozen);
}

#[test]
#[ignore = "requires explicit genuine vocabulary; adapter composition only"]
fn concession_replays_for_either_seat_without_priority_or_opening_chooser_authority() {
    let ordinary = state(vec![]);
    check(&ordinary, viewer_a(), A, B);
    check(&ordinary, viewer_b(), B, A);
    let scheduled = main_state(vec![]);
    check(&scheduled, viewer_b(), B, A);
    let cards = [A, B]
        .into_iter()
        .enumerate()
        .flat_map(|(index, owner)| {
            (0..20u64).map(move |offset| {
                card(
                    u64::try_from(index).unwrap() * 20 + offset + 1,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::land(BTreeSet::new(), vec![]),
                )
            })
        })
        .collect();
    let seed = seed(cards);
    let opening = GameState::initialize_opening(
        GameId(90),
        context(&seed.creature_vocabulary),
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 2048]),
        },
    )
    .unwrap()
    .into_state();
    assert!(opening.pending_decision().is_some());
    check(&opening, viewer_a(), A, B);
    check(&opening, viewer_b(), B, A);
}

#[test]
#[ignore = "requires explicit genuine vocabulary; finite multiplayer adapter/replay composition"]
fn multiplayer_concession_replays_with_original_roster_and_survivor_authority() {
    let third = SeatId(99);
    let seed = GameSeed::with_uniform_life(
        vec![A, B, third],
        A,
        vec![],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary(),
        std::sync::Arc::new(NameRules::literal_names()),
    );
    let source = GameState::initialize(GameId(90), context(&seed.creature_vocabulary), seed)
        .unwrap()
        .into_state();
    let before = source.clone();
    let bound = NativeBinding::new(
        context_id(),
        source.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: viewer_a(),
                native_seat: A,
            },
            NativeSeatBinding {
                seat_id: viewer_b(),
                native_seat: B,
            },
            NativeSeatBinding {
                seat_id: Uuid::from_u128(203),
                native_seat: third,
            },
        ],
        viewer_a(),
    )
    .unwrap();
    let view = NativeScope::new(&source, bound).unwrap();
    let prepared = view
        .propose(&command(&view, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&view, &prepared);
    let next = prepared.candidate().state().clone();
    assert_eq!(next.execution().window().participants(), &[B, third]);
    assert_eq!(next.original_participants(), &[A, B, third]);
    assert_eq!(next.execution().window().active_player(), None);
    assert_eq!(next.outcome(), None);
    let departed_binding = NativeBinding::new(
        context_id(),
        next.basis().clone(),
        view.binding.seats.clone(),
        viewer_a(),
    )
    .unwrap();
    let departed = NativeScope::new(&next, departed_binding).unwrap();
    assert!(
        matches!(departed.propose(&command(&departed, PlayerAction::Concede {})),
        Err(NativeBridgeError::Native(error)) if matches!(*error, GameError::UnknownActor))
    );
    let survivor_binding = NativeBinding::new(
        context_id(),
        next.basis().clone(),
        view.binding.seats.clone(),
        viewer_b(),
    )
    .unwrap();
    let survivor = NativeScope::new(&next, survivor_binding).unwrap();
    let prepared = survivor
        .propose(&command(&survivor, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&survivor, &prepared);
    assert_eq!(
        prepared.candidate().state().outcome(),
        Some(GameOutcome::Winner(third))
    );
    assert_eq!(
        prepared
            .candidate()
            .state()
            .conclusion()
            .unwrap()
            .losing_players()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([A, B])
    );
    assert_eq!(source, before);
}
