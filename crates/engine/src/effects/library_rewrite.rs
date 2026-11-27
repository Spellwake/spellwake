//! Actor resolution for resolving cast instructions.
use super::*;

impl EffectWindow {
    pub(super) fn resolving_cast_actor(
        &self,
        _instruction: &Instruction,
    ) -> Result<SeatId, EffectError> {
        self.resolving_player(crate::program::ResolvingPlayer::Controller)
    }
}
