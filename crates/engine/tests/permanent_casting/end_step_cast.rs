#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    characteristics::{
        ControlRecipient, KeywordAbility, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    combat::CombatDecision,
    mana::CastingPayment,
    program::{EndStepCast, TriggeredAbilityId},
};

fn to_end_trigger(mut game: GameState) -> GameState {
    for _ in 0..80 {
        if game.turn().unwrap().position().step == Step::End
            && !game.execution().window().entries().is_empty()
        {
            return game;
        }
        if let Some(p) = game.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            game = accept(
                &game,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            game = pass(&game).into_state();
        }
    }
    panic!("end step not reached");
}
