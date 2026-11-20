//! Rejected actual root calls. Inputs retain real basis/decision/object handles.
use super::MandatoryCase;
use crate::ordinary_game::{
    fixture::{A, B, CaseId, Fixture},
    policy::{Policy, PreparedInput},
    probes::RejectionProbe,
};
use spellwake_engine::{
    checks::CheckError,
    effects::EffectError,
    game::{DecisionReply, GameCommand, GameError, GameInput, GameState, RuleFrame},
    occurrence::TriggerError,
    targets::TargetRef,
    token::TokenBatchMember,
};
/// Generate controls from the one prescribed input, or explicit trailing annotations.
pub(crate) fn reached_probes(
    fixture: &Fixture,
    policy: &Policy,
    state: &GameState,
    prepared: Option<&PreparedInput>,
) -> Result<Vec<RejectionProbe>, String> {
    let CaseId::Mandatory(case) = fixture.case else {
        return Err("mandatory probes require mandatory case".into());
    };
    let Some(prepared) = prepared else {
        return policy.completion_probes(state);
    };
    let mut probes = prepared.recipe_probes().to_vec();
    let valid = &prepared.input;
    let mut add = |id: &str, input: GameInput, error: GameError| {
        probes.push(RejectionProbe {
            id: id.into(),
            expectation_ref: id.into(),
            attachment: "actual-mandatory-reached-state".into(),
            input,
            expected: error,
        });
    };
    let reply = |choice| -> Result<GameInput, String> {
        let Some(decision) = state.pending_decision() else {
            return Err("probe reply has no real frame".into());
        };
        Ok(GameInput {
            basis: valid.basis.clone(),
            actor: decision.chooser(),
            command: GameCommand::Reply {
                decision: decision.id(),
                choice,
            },
        })
    };
    let wrong_actor = |mut input: GameInput| {
        input.actor = if input.actor == A { B } else { A };
        input
    };
    if let Some(decision) = state.pending_decision() {
        match decision.frame() {
            RuleFrame::Triggers(order) if case == MandatoryCase::T02OwnMain => {
                let mut input = valid.clone();
                input.actor = if input.actor == A { B } else { A };
                add("T10.wrong-actor", input, GameError::WrongChooser);
                add(
                    "T10.wrong-decision-kind",
                    reply(DecisionReply::TriggerTarget(TargetRef::Player(A)))?,
                    GameError::Effect(EffectError::WrongDecisionKind),
                );
                let mut stale = valid.clone();
                stale.basis = policy
                    .previous_basis
                    .clone()
                    .ok_or("stale probe lacks actual earlier basis")?;
                add("T10.stale-outer-basis", stale, GameError::StaleRevision);
                let members = order.instances();
                if members.len() < 2 {
                    return Err("T10 order probes require multi-instance frame".into());
                }
                add(
                    "T10.omitted-order-member",
                    reply(DecisionReply::OrderTriggers(members[1..].to_vec()))?,
                    invalid_trigger(),
                );
                let mut duplicate = members.to_vec();
                duplicate[1] = duplicate[0];
                add(
                    "T10.duplicate-order-member",
                    reply(DecisionReply::OrderTriggers(duplicate))?,
                    invalid_trigger(),
                );
            }
            RuleFrame::Triggers(order)
                if case == MandatoryCase::T01OwnThenOpponent && order.instances().len() > 1 =>
            {
                let old = policy
                    .mandatory()
                    .ok_or("mandatory strategy absent")?
                    .prior_nonmember(order.instances())?;
                let mut stale = order.instances().to_vec();
                stale[0] = old;
                add(
                    "T10.stale-trigger-instance",
                    reply(DecisionReply::OrderTriggers(stale))?,
                    invalid_trigger(),
                );
                let mut foreign = order.instances().to_vec();
                foreign.push(old);
                add(
                    "T10.foreign-order-member",
                    reply(DecisionReply::OrderTriggers(foreign))?,
                    invalid_trigger(),
                );
            }
            RuleFrame::TimestampOrder(order) if case == MandatoryCase::T02OwnMain => {
                let members = order.members().iter().map(|m| m.member).collect::<Vec<_>>();
                if members.len() != 2 || members.contains(&TokenBatchMember::new(2)) {
                    return Err("T10 timestamp requires exact actual two-member domain".into());
                }
                add(
                    "T10.wrong-actor.timestamp",
                    wrong_actor(valid.clone()),
                    GameError::WrongChooser,
                );
                add(
                    "T10.wrong-decision-kind.timestamp",
                    reply(DecisionReply::TriggerTarget(TargetRef::Player(A)))?,
                    GameError::Effect(EffectError::WrongDecisionKind),
                );
                add(
                    "T10.omitted-order-member.timestamp",
                    reply(DecisionReply::TimestampOrder(members[..1].to_vec()))?,
                    GameError::Effect(EffectError::IllegalTimestampOrder),
                );
                add(
                    "T10.duplicate-order-member.timestamp",
                    reply(DecisionReply::TimestampOrder(vec![members[0], members[0]]))?,
                    GameError::Effect(EffectError::IllegalTimestampOrder),
                );
                add(
                    "T10.foreign-order-member.timestamp",
                    reply(DecisionReply::TimestampOrder(vec![
                        members[0],
                        TokenBatchMember::new(2),
                    ]))?,
                    GameError::Effect(EffectError::IllegalTimestampOrder),
                );
            }
            RuleFrame::Checks(spellwake_engine::checks::CheckDecision::GraveyardOrder(order))
                if case == MandatoryCase::T06ArtistTop =>
            {
                let domain = order.domain();
                let foreign = fixture.current_object(state, "P0.grizzly-bears.1")?;
                if domain.len() != 2 || domain.contains(&foreign) {
                    return Err(
                        "T10 graveyard requires real two deaths and surviving foreign Bear".into(),
                    );
                }
                add(
                    "T10.wrong-actor.graveyard",
                    wrong_actor(valid.clone()),
                    GameError::WrongChooser,
                );
                add(
                    "T10.wrong-decision-kind.graveyard",
                    reply(DecisionReply::TriggerTarget(TargetRef::Player(A)))?,
                    GameError::Effect(EffectError::WrongDecisionKind),
                );
                let error =
                    GameError::Effect(EffectError::Check(CheckError::InvalidGraveyardOrder));
                add(
                    "T10.omitted-order-member.graveyard",
                    reply(DecisionReply::OrderGraveyard(domain[..1].to_vec()))?,
                    error.clone(),
                );
                add(
                    "T10.duplicate-order-member.graveyard",
                    reply(DecisionReply::OrderGraveyard(vec![domain[0], domain[0]]))?,
                    error.clone(),
                );
                add(
                    "T10.foreign-order-member.graveyard",
                    reply(DecisionReply::OrderGraveyard(vec![domain[0], foreign]))?,
                    error,
                );
            }
            RuleFrame::TriggerTarget(target) if case == MandatoryCase::T06SplitTargets => {
                if policy
                    .mandatory()
                    .ok_or("mandatory strategy absent")?
                    .player_choices_consumed()
                    == 0
                {
                    add(
                        "T10.creature-target-in-player-frame",
                        reply(DecisionReply::TriggerTarget(TargetRef::Object(
                            fixture.current_object(state, "P0.grizzly-bears.1")?,
                        )))?,
                        GameError::Effect(EffectError::IllegalTarget),
                    );
                    add(
                        "T10.stack-target-in-player-frame",
                        reply(DecisionReply::TriggerTarget(TargetRef::Stack(
                            target.entry(),
                        )))?,
                        GameError::Effect(EffectError::IllegalTarget),
                    );
                } else {
                    let mut input = valid.clone();
                    let GameCommand::Reply { decision, .. } = &mut input.command else {
                        return Err("target probe lacks reply".into());
                    };
                    *decision = policy
                        .previous_decision
                        .ok_or("target probe lacks earlier actual DecisionId")?;
                    if *decision
                        == state
                            .pending_decision()
                            .ok_or("current target absent")?
                            .id()
                    {
                        return Err("target stale handle unexpectedly current".into());
                    }
                    add(
                        "T10.target-from-another-frame",
                        input,
                        GameError::WrongDecision,
                    );
                }
            }
            _ => {}
        }
    } else if case == MandatoryCase::T08Self
        && policy
            .mandatory()
            .ok_or("mandatory strategy absent")?
            .just_accepted_player_target(state.basis().revision)
    {
        let decision = policy
            .previous_decision
            .ok_or("consumed reply probe lacks actual prior decision")?;
        add(
            "T10.repeated-consumed-reply",
            GameInput {
                basis: valid.basis.clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision,
                    choice: DecisionReply::TriggerTarget(TargetRef::Player(A)),
                },
            },
            GameError::NoDecision,
        );
    }
    Ok(probes)
}
fn invalid_trigger() -> GameError {
    GameError::Effect(EffectError::Trigger(TriggerError::InvalidTriggerOrder))
}
