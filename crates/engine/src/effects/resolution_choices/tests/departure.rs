//! Genuine root cuts for the two resolution-time departure continuations.
use super::*;

const C: SeatId = SeatId(2);
const D: SeatId = SeatId(3);

fn targeted_offer(body: Vec<Instruction>) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "departure-choice-unit".into(),
        vocabulary.rules_revision().into(),
        "synthetic-native-only".into(),
        "unit".into(),
        "unit".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seats = [A, B, C, D];
    let cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SINGLE_PLAYER, body),
        ),
    }];
    let seed = GameSeed::with_uniform_life(
        seats.to_vec(),
        A,
        cards,
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let state = GameState::initialize(GameId(7712), context, seed)
        .unwrap()
        .into_state();
    let state = candidate(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Player(B),
        },
    )
    .into_state();
    seats.into_iter().fold(state, |state, seat| {
        candidate(&state, seat, GameCommand::Pass).into_state()
    })
}

#[test]
fn a_replacement_chooser_can_leave_before_answering_the_original_choice() {
    for body in [
        vec![
            gain(2),
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::TargetPlayer,
                instructions: 1,
            },
            gain(3),
        ],
        vec![
            gain(2),
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::TargetPlayer,
                instructions: 2,
            },
            gain(1),
            gain(2),
        ],
    ] {
        let offered = targeted_offer(body);
        assert_eq!(offered.execution().life_total(A), Some(&22.into()));
        assert_eq!(offered.pending_decision().unwrap().chooser(), B);
        let after_departure = candidate(&offered, B, GameCommand::Concede).into_state();
        let Some(RuleFrame::ReplacementChooser(frame)) = after_departure
            .pending_decision()
            .map(|pending| pending.frame())
        else {
            panic!("the original effect remains suspended while a replacement is selected");
        };
        assert_eq!(
            (
                frame.instruction(),
                frame.departed(),
                frame.chooser(),
                frame.candidates()
            ),
            (1, B, A, &[C, D][..])
        );
        let picked = reply(&after_departure, DecisionReply::ReplacementChooser(C));
        assert!(
            picked
                .rules_journal()
                .records
                .iter()
                .any(|row| matches!(row,
            RulesRecord::ChooserReplaced { departed, chooser, selected, .. }
                if (*departed, *chooser, *selected) == (B, A, C)))
        );
        let picked = picked.into_state();
        assert_eq!(picked.pending_decision().unwrap().chooser(), C);
        let after_second_departure = candidate(&picked, C, GameCommand::Concede).into_state();
        let replacement = reply(
            &after_second_departure,
            DecisionReply::ReplacementChooser(D),
        )
        .into_state();
        assert_eq!(replacement.pending_decision().unwrap().chooser(), D);
        let resolved = reply(
            &replacement,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(resolved.state().execution().life_total(A), Some(&25.into()));
        assert!(resolved.state().pending_decision().is_none());
    }
}

#[test]
fn a_departed_payer_uses_the_unpaid_suffix_without_replaying_the_prefix() {
    let state = targeted_offer(vec![
        gain(2),
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::TargetPlayer,
                ManaCost::free(),
                crate::program::ValueExpr::Literal(0.into()),
                2,
                4,
            ),
        )),
        gain(7),
        Instruction::Jump { destination: 5 },
        gain(3),
    ]);
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    let departed = candidate(&state, B, GameCommand::Concede);
    assert_eq!(departed.state().execution().life_total(A), Some(&25.into()));
    assert!(departed.state().pending_decision().is_none());
    assert!(
        departed
            .rules_journal()
            .records
            .iter()
            .any(|row| matches!(row,
        RulesRecord::ResolutionPaymentOutcome { payer, instruction: 1,
            outcome: ResolutionPaymentOutcome::UnpaidDeparted, next: 4, .. } if *payer == B))
    );
}
