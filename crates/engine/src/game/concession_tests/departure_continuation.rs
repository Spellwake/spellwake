use super::*;

fn settle_three(mut game: GameState) -> GameState {
    for _ in 0..3 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            game.execution().window().phase()
        else {
            panic!("all three players must have priority in turn before resolution");
        };
        game = step(&game, holder, GameCommand::Pass);
    }
    game
}

#[test]
fn opening_departure_preserves_the_surviving_mulligan_and_starter() {
    let cards = [A, B, C]
        .into_iter()
        .flat_map(|owner| {
            (0..15).map(move |index| {
                card(
                    100 + u64::from(owner.0) * 100 + index,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                )
            })
        })
        .collect();
    let (context, seed) = seed(cards, vec![A, B, C], &[20, 20, 20]);
    let state = GameState::initialize_opening(
        GameId(1050),
        context,
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 300]),
        },
    )
    .unwrap()
    .into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    let state = step(&state, A, GameCommand::Concede);
    assert_eq!(state.execution().window().participants(), &[B, C]);
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    assert_eq!(state.execution().window().active_player(), B);
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Mulligan));
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    assert_eq!(state.opening().unwrap().mulligans_taken(B), 1);
    assert_eq!(state.opening().unwrap().charged(B), 0);
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    assert!(state.opening().unwrap().complete());
    assert_eq!(state.execution().window().active_player(), B);
    assert_eq!(state.turn().unwrap().position().number, 1);
}

#[test]
fn grouped_discard_keeps_accepted_selections_and_skips_the_departed_hand() {
    let spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    );
    let mut cards = vec![card(1, A, ObjectDestination::Hand, spell)];
    cards.extend(
        [(A, 10), (A, 11), (B, 20), (B, 21), (C, 30), (C, 31)]
            .into_iter()
            .map(|(owner, id)| {
                card(
                    id,
                    owner,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                )
            }),
    );
    let state = root(cards, &[20, 20, 20], None, vec![A, B, C]);
    let state = settle_three(step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    ));
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![object(&state, 10)]),
    );
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    let state = step(&state, B, GameCommand::Concede);
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(20))
            .is_none()
    );
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![object(&state, 30)]),
    );
    let cards = state.execution().window().cards();
    assert_eq!(
        cards.get(object(&state, 10)).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        cards.get(object(&state, 11)).unwrap().zone(),
        ObjectZone::Hand(A)
    );
    assert_eq!(
        cards.get(object(&state, 30)).unwrap().zone(),
        ObjectZone::Graveyard(C)
    );
    assert_eq!(
        cards.get(object(&state, 31)).unwrap().zone(),
        ObjectZone::Hand(C)
    );
}
