#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
    mana::{CastResource, CastingPayment},
    program::ConditionalCastCost,
};
fn reply(game: &GameState, choice: DecisionReply) -> GameState {
    let pending = game.pending_decision().unwrap();
    accept(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
    .into_state()
}
