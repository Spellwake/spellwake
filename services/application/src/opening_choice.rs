//! Starting-player selection for a reserved native game.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An empty selection observes the current chooser and roll.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpeningChoice {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    pub play_first: Option<bool>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub prior_chooser_seat_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub first_seat_id: Option<Uuid>,
}
impl OpeningChoice {
    pub(crate) fn valid(&self) -> bool {
        !self.game_id.is_nil()
            && self.first_seat_id.is_none_or(|id| !id.is_nil())
            && self.prior_chooser_seat_id.is_none_or(|id| !id.is_nil())
            && [
                self.first_seat_id.is_some(),
                self.prior_chooser_seat_id.is_some(),
                self.play_first.is_some(),
            ]
            .into_iter()
            .filter(|present| *present)
            .count()
                <= 1
    }
}
/// Public chooser, selected starter and ordered table rolls.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub chooser_seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub first_seat_id: Option<Uuid>,
    pub your_choice: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub rolls: Option<Vec<u8>>,
    #[serde(
        default,
        deserialize_with = "crate::identifiers::nullable_uuid_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub seat_ids: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_rounds: Option<Vec<Vec<u8>>>,
}
impl Selection {
    pub(crate) fn matches(&self, game: Uuid) -> bool {
        let face = |v: &u8| (1..=20).contains(v);
        if self.game_id != game {
            return false;
        }
        let Some(seats) = &self.seat_ids else {
            return self.roll_rounds.is_none()
                && self
                    .rolls
                    .as_ref()
                    .is_none_or(|r| r.len() == 2 && r[0] != r[1] && r.iter().all(face));
        };
        let Some(rounds) = &self.roll_rounds else {
            return false;
        };
        if !(3..=6).contains(&seats.len())
            || seats
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != seats.len()
            || !seats.contains(&self.chooser_seat_id)
            || self.first_seat_id.is_some_and(|id| !seats.contains(&id))
            || rounds.is_empty()
            || rounds.first() != self.rolls.as_ref()
        {
            return false;
        }
        let mut contenders = seats.clone();
        for round in rounds {
            if contenders.len() < 2 || round.len() != contenders.len() || !round.iter().all(face) {
                return false;
            }
            let highest = round.iter().max();
            contenders = contenders
                .into_iter()
                .zip(round)
                .filter_map(|(seat, roll)| (Some(roll) == highest).then_some(seat))
                .collect();
        }
        contenders == [self.chooser_seat_id]
    }
}
