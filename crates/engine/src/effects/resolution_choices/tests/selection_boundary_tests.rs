//! Shared role and empty-selection native root regressions.
use super::*;
use crate::program::{AffectedPlayers, PermanentTypeFilter};

#[test]
fn target_player_role_uses_the_bound_player_for_optional_and_payment_choices() {
    let mut state = initialize_for_target(
        BodyTarget::SinglePlayer,
        vec![
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::TargetPlayer,
                instructions: 1,
            },
            gain(1),
            Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::TargetPlayer,
                    ManaCost::free(),
                    crate::program::ValueExpr::Literal(0.into()),
                    3,
                    4,
                ),
            )),
            gain(2),
        ],
    )
    .into_state();
    state = candidate(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Player(B),
        },
    )
    .into_state();
    state = candidate(&state, A, GameCommand::Pass).into_state();
    state = candidate(&state, B, GameCommand::Pass).into_state();
    let decision = state.pending_decision().unwrap();
    assert_eq!(decision.chooser(), B);
    assert!(matches!(decision.frame(), RuleFrame::OptionalEffect(_)));
    let saved = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            },
        }),
        Err(GameError::WrongChooser)
    );
    assert_eq!(state, saved);
    state = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(state.execution().life_total(A), Some(&21.into()));
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    state = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment::default())),
    )
    .into_state();
    assert_eq!(state.execution().life_total(A), Some(&23.into()));
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert!(state.pending_decision().is_none());
}

#[test]
fn empty_selection_continues_the_same_body_without_prompts_or_selected_moves() {
    for operation in [
        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
            AffectedPlayers::EachPlayer,
            crate::program::ValueExpr::Literal(1.into()),
            PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        ))),
        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
            AffectedPlayers::One(ResolvingPlayer::Controller),
            crate::program::ValueExpr::Literal(0.into()),
        ))),
    ] {
        let mut state = initialize(vec![gain(1), operation, gain(2)]).into_state();
        state = candidate(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: object(&state, 1),
                payment: None,
            },
        )
        .into_state();
        state = candidate(&state, A, GameCommand::Pass).into_state();
        state = candidate(&state, B, GameCommand::Pass).into_state();
        assert_eq!(state.execution().life_total(A), Some(&23.into()));
        assert!(state.pending_decision().is_none());
        assert!(state.execution().resolution_selection_state().is_none());
    }
}
