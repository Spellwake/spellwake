//! Typed visible observations. These are not a native journal or executable patch.
use super::*;
use crate::{amount::Amount, match_history::Outcome};
mod delta;
/// Normalized terminal result supplied by the authoritative result adapter.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerTerminal {
    pub outcome: Outcome,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub winner_seat_ids: Vec<Uuid>,
}
/// Finite public observation vocabulary. No private native payload/Debug escape.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerPublicChange {
    Life {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        seat_id: Uuid,
        before: RulesInteger,
        after: RulesInteger,
    },
    Counters {
        #[serde(deserialize_with = "crate::provider_json::object")]
        subject: VisibleSubject,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        counters: Vec<PlayerCounter>,
    },
    /// Complete public seat values for mana, poison, elimination and count changes.
    ReplaceSeat {
        #[serde(deserialize_with = "crate::provider_json::object")]
        seat: PlayerSeatView,
    },
    UpsertObject {
        #[serde(deserialize_with = "crate::provider_json::object")]
        object: Box<PlayerObject>,
    },
    RemoveObject {
        handle: PlayerHandle,
    },
    ReplaceZone {
        #[serde(deserialize_with = "crate::provider_json::object")]
        zone: PlayerZone,
    },
    Turn {
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        turn: Option<PlayerTurn>,
    },
    Stack {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        entries: Vec<PlayerStackEntry>,
    },
    Decisions {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        decisions: Vec<PlayerDecision>,
    },
    Terminal {
        #[serde(deserialize_with = "crate::provider_json::object")]
        result: PlayerTerminal,
    },
}
/// A visible historical record at an accepted coordinate, not the latest board.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerVisibleEvent {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_seat_id: Uuid,
    pub event_sequence: Amount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub changes: Vec<PlayerPublicChange>,
}
/// Historical projections established by the real archive/projector. A synthetic
/// pair proves only validation behavior, not complete native replay or disclosure.
pub struct PlayerEventFacts<'a> {
    pub before: &'a PlayerSnapshot,
    pub before_projection: PlayerProjectionFacts<'a>,
    pub after: &'a PlayerSnapshot,
    pub after_projection: PlayerProjectionFacts<'a>,
    pub terminal: Option<&'a PlayerTerminal>,
}
impl PlayerVisibleEvent {
    /// # Errors
    /// Rejects foreign coordinates/viewers, nonhistorical projections, private
    /// object/zone/decision substitutions and mismatched public before/after values.
    /// Does not assert a complete change list or reexecute a native transition.
    pub fn validate(&self, f: &PlayerEventFacts<'_>) -> Result<(), InvalidPlayerWire> {
        f.before.validate(&f.before_projection)?;
        f.after.validate(&f.after_projection)?;
        if self.basis != f.after.basis
            || self.viewer_seat_id != f.after.viewer_seat_id
            || self.viewer_seat_id != f.before.viewer_seat_id
            || self.event_sequence != f.after.through_event_sequence
            || f.before.through_event_sequence >= self.event_sequence
            || f.before.basis.game_id != self.basis.game_id
            || f.before.basis.context_id != self.basis.context_id
            || f.before_projection.roster != f.after_projection.roster
            || f.before.basis.compare_revision_in_epoch(&self.basis)
                == Some(std::cmp::Ordering::Greater)
            || self.changes.is_empty()
            || self.changes.len() > MAX_PLAYER_ITEMS
        {
            return Err(InvalidPlayerWire);
        }
        for change in &self.changes {
            let valid = match change {
                PlayerPublicChange::Life {
                    seat_id,
                    before,
                    after,
                } => {
                    f.before
                        .players
                        .iter()
                        .any(|p| p.seat_id == *seat_id && &p.life == before)
                        && f.after
                            .players
                            .iter()
                            .any(|p| p.seat_id == *seat_id && &p.life == after)
                }
                PlayerPublicChange::Counters { subject, counters } => match subject.target() {
                    PlayerTarget::Object { handle } => f
                        .after
                        .visible_objects
                        .iter()
                        .any(|o| &o.handle == handle && &o.counters == counters),
                    PlayerTarget::Seat { seat_id } => f
                        .after
                        .players
                        .iter()
                        .any(|p| p.seat_id == *seat_id && &p.counters == counters),
                    PlayerTarget::Stack { .. } => false,
                },
                PlayerPublicChange::ReplaceSeat { seat } => {
                    f.after.players.contains(seat)
                        && f.before
                            .players
                            .iter()
                            .any(|previous| previous.seat_id == seat.seat_id && previous != seat)
                }
                PlayerPublicChange::UpsertObject { object } => {
                    f.after.visible_objects.iter().any(|o| o == object.as_ref())
                }
                PlayerPublicChange::RemoveObject { handle } => {
                    f.before.visible_objects.iter().any(|o| &o.handle == handle)
                        && !f.after.visible_objects.iter().any(|o| &o.handle == handle)
                }
                PlayerPublicChange::ReplaceZone { zone } => f.after.zones.contains(zone),
                PlayerPublicChange::Turn { turn } => &f.after.turn == turn,
                PlayerPublicChange::Stack { entries } => &f.after.stack == entries,
                PlayerPublicChange::Decisions { decisions } => &f.after.decisions == decisions,
                PlayerPublicChange::Terminal { result } => {
                    f.terminal == Some(result)
                        && super::actions::unique(result.winner_seat_ids.iter())
                        && result
                            .winner_seat_ids
                            .iter()
                            .all(|id| f.after_projection.roster.contains(id))
                        && match result.outcome {
                            Outcome::Win => !result.winner_seat_ids.is_empty(),
                            Outcome::Draw | Outcome::NoContest => result.winner_seat_ids.is_empty(),
                        }
                }
            };
            if !valid {
                return Err(InvalidPlayerWire);
            }
        }
        Ok(())
    }
}
