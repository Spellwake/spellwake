//! Private native facts for ordinary, unreplaced card draws (CR 121).
//! No public projection, draw replacement/trigger execution or archival replay.
use crate::{
    object_identity::{GameObject, GameObjects, ObjectMovement},
    priority::SeatId,
};
use spellwake_contracts::RulesInteger;

// Keep Drawn inline as the primary high-frequency draw event payload.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawEvent {
    Drawn {
        player: SeatId,
        before: GameObject,
        after: GameObject,
    },
    /// Exact consecutive failed attempts after the library became empty. This
    /// compression is valid only in this universe without draw replacements.
    EmptyLibrary {
        player: SeatId,
        attempts: RulesInteger,
    },
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawJournal {
    pub events: Vec<DrawEvent>,
}
impl DrawJournal {
    pub fn append(&mut self, mut other: Self) {
        self.events.append(&mut other.events);
    }
    pub(crate) fn record_draw(&mut self, player: SeatId, movement: &ObjectMovement) {
        self.events.push(DrawEvent::Drawn {
            player,
            before: movement.before.clone(),
            after: movement.after.clone(),
        });
    }
}
/// Trusted physical primitive result, not a complete candidate or player action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDraw {
    pub next: GameObjects,
    pub journal: DrawJournal,
}
/// Only empty-draw losses are currently represented, without loss prevention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOutcome {
    Winner(SeatId),
    Draw,
}
