//! Shared privileged recorder of actual public-root calls for selected producers.
use super::*;
use crate::test_support::{packet::AliasContext, projection};
use serde_json::{Value, json};
pub(super) struct Trace {
    pub(super) aliases: AliasContext,
    pub(super) rows: Vec<Value>,
}
impl Trace {
    pub(super) fn step(
        &mut self,
        state: &GameState,
        actor: SeatId,
        command: GameCommand,
    ) -> NativeCandidate {
        let input = GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        };
        let saved = state.clone();
        let source = projection::state(state, &self.aliases).unwrap();
        let result = state.advance(input.clone()).unwrap();
        let repeated = state.advance(input.clone()).unwrap();
        assert_eq!(result, repeated);
        assert_eq!(state, &saved);
        self.rows.push(json!({"input":projection::input(&input),"source":source,"candidate":projection::candidate(&result,Some(state),&self.aliases).unwrap(),"source_eq":state == &saved,"full_result_eq":result==repeated}));
        result
    }
}
