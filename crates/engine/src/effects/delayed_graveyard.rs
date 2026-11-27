//! Follow only the graveyard incarnations the originating triggers can identify.
use super::*;

impl EffectWindow {
    pub(super) fn prune_aura_graveyard_returns(&mut self) {
        self.aura_graveyard_returns.clear();
    }

    pub(super) fn delayed_graveyard_return_player(&self, object: ObjectId) -> Option<SeatId> {
        let card = self.window.cards().get(object)?;
        let ObjectZone::Graveyard(player) = card.zone() else {
            return None;
        };
        (card.card_id().is_some() && self.copiable_program(object)?.permanent().is_some())
            .then_some(player)
    }
}
