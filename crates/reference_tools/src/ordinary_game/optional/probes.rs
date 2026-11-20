//! Actual reached-state authority denials. The shared recorder checks immutable
//! failure, retains the original input and reexecutes it before valid continuation.
use super::OptionalCase;
use crate::ordinary_game::{
    fixture::{CaseId, Fixture, other},
    policy::{Policy, PreparedInput},
    probes::RejectionProbe,
};
use spellwake_engine::{
    effects::{CopyTargetChoice, EffectError, OptionalEffectChoice, ResolutionPaymentReply},
    game::{DecisionReply, GameCommand, GameError, GameInput, GameState, RuleFrame},
    mana::{ManaPayment, ManaPaymentError, ManaSpend, ManaType},
};

pub(in crate::ordinary_game) fn reached_probes(
    fixture: &Fixture,
    policy: &Policy,
    state: &GameState,
    prepared: Option<&PreparedInput>,
) -> Result<Vec<RejectionProbe>, String> {
    let CaseId::Optional(case) = fixture.case else {
        return Err("optional/payment probes require their source case".into());
    };
    let Some(prepared) = prepared else {
        return policy.completion_probes(state);
    };
    let mut probes = prepared.recipe_probes().to_vec();
    let Some(decision) = state.pending_decision() else {
        return Ok(probes);
    };
    let (kind, valid_choice) = match decision.frame() {
        RuleFrame::OptionalEffect(_) => (
            "optional",
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        ),
        RuleFrame::ResolutionPayment(_) => (
            "payment",
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        ),
        RuleFrame::ManaColor(frame) => (
            "color",
            DecisionReply::ManaColor(
                *frame
                    .colors()
                    .iter()
                    .next()
                    .ok_or("empty actual color domain")?,
            ),
        ),
        RuleFrame::CopyTargets(_) => ("copy", DecisionReply::CopyTargets(CopyTargetChoice::Retain)),
        _ => return Ok(probes),
    };
    let valid_reply = GameInput {
        basis: state.basis().clone(),
        actor: decision.chooser(),
        command: GameCommand::Reply {
            decision: decision.id(),
            choice: valid_choice,
        },
    };
    let mut add = |name: &str, input: GameInput, expected: GameError| {
        probes.push(RejectionProbe {
            id: format!("{}.authority.{kind}.{name}", case.as_str()),
            expectation_ref: format!("optional-payment-actual-authority.{name}"),
            attachment: "actual-source-backed-prefix-native-rejection-only".into(),
            input,
            expected,
        });
    };
    let mut wrong_actor = valid_reply.clone();
    wrong_actor.actor = other(decision.chooser())?;
    add("wrong-actor", wrong_actor, GameError::WrongChooser);
    if let Some(previous) = &policy.previous_basis {
        let mut stale = valid_reply.clone();
        stale.basis = previous.clone();
        add("previous-basis", stale, GameError::StaleRevision);
    }
    if let Some(previous) = policy.previous_decision
        && previous != decision.id()
    {
        let mut stale = valid_reply.clone();
        if let GameCommand::Reply { decision, .. } = &mut stale.command {
            *decision = previous;
        }
        add(
            "previous-decision-current-basis",
            stale,
            GameError::WrongDecision,
        );
    }
    let mut wrong_kind = valid_reply.clone();
    if let GameCommand::Reply { choice, .. } = &mut wrong_kind.command {
        *choice = DecisionReply::Discard(vec![]);
    }
    add(
        "wrong-kind",
        wrong_kind,
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let mut pass = valid_reply.clone();
    pass.command = GameCommand::Pass;
    add("pass-during-choice", pass, GameError::DecisionRequired);
    if let RuleFrame::ManaColor(frame) = decision.frame() {
        let mut input = valid_reply.clone();
        input.command = GameCommand::ActivateMana {
            source: frame.source(),
            ability: frame.ability(),
        };
        add("mana-during-color", input, GameError::DecisionRequired);
    }
    if case == OptionalCase::R13 && matches!(decision.frame(), RuleFrame::ResolutionPayment(_)) {
        let mut input = valid_reply;
        if let GameCommand::Reply { choice, .. } = &mut input.command {
            *choice = DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 3.into(),
                }],
            }));
        }
        add(
            "insufficient-pay-stays-pending",
            input,
            GameError::Effect(EffectError::InvalidManaPayment(
                ManaPaymentError::InsufficientMana,
            )),
        );
    }
    Ok(probes)
}
