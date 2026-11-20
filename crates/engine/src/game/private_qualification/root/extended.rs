//! Version-two finite root controls, not new player commands or engine state.
use super::*;
mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::game::private_qualification) struct FrameAttempt {
    pub source: GameState,
    pub frame: EffectDecision,
    pub actor: SeatId,
    pub reply: DecisionReply,
    pub result: Result<RuleProgress, EffectError>,
}

pub(super) fn calls(actions: &[Value]) -> Result<Vec<(usize, &Value)>, String> {
    let mut calls = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        if action["op"] == "repeat_bounded" {
            let count = usize::try_from(
                action["count"]
                    .as_u64()
                    .ok_or("missing bounded repetition count")?,
            )
            .map_err(|error| error.to_string())?;
            let sequence = rows(action, "sequence")?;
            if count == 0
                || count > 512
                || sequence.is_empty()
                || sequence.iter().any(|value| value["op"] == "repeat_bounded")
            {
                return Err("unsupported finite repetition shape".into());
            }
            for _ in 0..count {
                for item in sequence {
                    calls.push((index + 1, item));
                }
            }
        } else {
            calls.push((index + 1, action));
        }
        if calls.len() > 4096 {
            return Err("finite private root call budget exceeded".into());
        }
    }
    Ok(calls)
}

pub(super) fn opening_guard(state: &GameState, action: &Value) -> Result<(), String> {
    let Some(guard) = action.get("opening_guard") else {
        return Ok(());
    };
    let actual = state
        .pending_decision()
        .ok_or("opening guard without actual Pending")?;
    let (stage, player, count) = match actual.frame() {
        RuleFrame::Opening(OpeningDecision::Declare { player, .. }) => ("Declare", *player, None),
        RuleFrame::Opening(OpeningDecision::Bottom { player, count, .. }) => {
            ("Bottom", *player, Some(*count))
        }
        _ => return Err("opening guard found a different actual frame".into()),
    };
    if guard["stage"] != stage
        || packet::seat(string(guard, "player")?)? != player
        || guard
            .get("charged_count")
            .is_some_and(|value| value.as_u64() != count.map(|value| value as u64))
    {
        return Err("literal opening guard differs from actual current frame".into());
    }
    Ok(())
}

pub(super) fn wrong_wave(
    source: &GameState,
    action: &Value,
    fixture: &Value,
    joins: &Joins,
    aliases: &packet::AliasContext,
) -> Result<(Value, FrameAttempt), String> {
    if action["mutation"]["wave"] != "actual_plus_one_checked" {
        return Err("unsupported private frame mutation".into());
    }
    let pending = source
        .pending_decision()
        .ok_or("private frame probe lacks actual root Pending")?;
    let RuleFrame::Triggers(frame) = pending.frame() else {
        return Err("private frame probe is not a real trigger order".into());
    };
    let offered = frame
        .qualification_with_next_wave()
        .map_err(|error| format!("private frame arithmetic failed: {error:?}"))?;
    let input = reply(
        source,
        &json!({"actor":string(action,"actor")?,"kind":"OrderTriggers","items":action["items"]}),
        fixture,
        joins,
    )?;
    let GameCommand::Reply { choice, .. } = input.command else {
        return Err("private frame choice was not a reply".into());
    };
    let frame = EffectDecision::Triggers(offered);
    let before = source.clone();
    let expected = source.execution.revision();
    let result = capture::progress(
        &source.execution,
        aliases,
        projection::EffectCall::ResumeRule {
            expected,
            frame: &frame,
            actor: input.actor,
            reply: &choice,
            turn: source.turn.as_ref(),
        },
        |execution| {
            execution.resume_rule(
                expected,
                &frame,
                input.actor,
                choice.clone(),
                source.turn.as_ref(),
            )
        },
        false,
    )?;
    let actual = match &result {
        Ok(value) => json!({"tag":"Ok","progress":projection::rule_progress(value,aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    let view = json!({"scope":"private-exact-frame-mutation-not-GameInput","basis":projection::basis(source.basis()),"original_frame":projection::frame(pending.frame(),source)?,"offered_frame":projection::effect_frame(&frame,&source.execution)?,"reply":projection::reply(&choice),"result":actual,"retained_source_full_state_eq":source==&before});
    Ok((
        view,
        FrameAttempt {
            source: source.clone(),
            frame,
            actor: input.actor,
            reply: choice,
            result,
        },
    ))
}
