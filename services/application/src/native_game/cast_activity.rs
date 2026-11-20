//! Finite physical completed-cast metrics, derived from each native candidate delta.
//! These do not qualify a complete objective roster, copied-spell casting, or cards.
use super::*;
use crate::{amount::Amount, player_wire::NativeRevision};
use serde::Serialize;
use spellwake_engine::{
    characteristics::Color,
    occurrence::{RulesOccurrence, RulesRecord},
};
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeCastActivity {
    seat_id: Uuid,
    red_or_green: NativeRevision,
    black_or_green: NativeRevision,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeCastFact {
    pub(crate) event_index: Amount,
    pub(crate) account_id: Uuid,
    pub(crate) kind: &'static str,
    pub(crate) revision: &'static str,
    pub(crate) value: Amount,
}
impl NativeCastFact {
    pub(crate) fn matches(
        &self,
        account: Uuid,
        kind: &str,
        revision: &str,
        value: &Amount,
    ) -> bool {
        self.account_id == account
            && self.kind == kind
            && self.revision == revision
            && &self.value == value
    }
    /// Exact complete set for the singleton native Event. Expected coordinates
    /// are uniquely derived above; no supplied value becomes native truth.
    pub(crate) fn matches_set<'a>(
        expected: &[Self],
        actual: impl ExactSizeIterator<Item = (Uuid, &'a str, &'a str, &'a Amount)> + Clone,
    ) -> bool {
        expected.len() == actual.len()
            && expected.iter().all(|fact| {
                actual.clone().any(|(account, kind, revision, value)| {
                    fact.matches(account, kind, revision, value)
                })
            })
    }
}
/// Immutable source-selected seat/account mapping retained for history recomputation.
/// No external caller can construct or deserialize this semantic projection.
pub(super) struct NativeCastAccounts {
    game: Uuid,
    context: Uuid,
    seats: BTreeMap<Uuid, Option<Uuid>>,
}
impl NativeCastAccounts {
    pub(super) fn new(
        game: Uuid,
        context: Uuid,
        rows: impl IntoIterator<Item = (Uuid, Option<Uuid>)>,
    ) -> Result<Self, NativeCheckpointError> {
        let mut seats = BTreeMap::new();
        let mut humans = BTreeSet::new();
        for (seat, account) in rows {
            if seat.is_nil()
                || seats.insert(seat, account).is_some()
                || account.is_some_and(|id| id.is_nil() || !humans.insert(id))
            {
                return Err(NativeCheckpointError::Invalid);
            }
        }
        if game.is_nil() || context.is_nil() || seats.is_empty() || seats.len() > 6 {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(Self {
            game,
            context,
            seats,
        })
    }
    pub(super) fn facts(
        &self,
        checkpoint: &NativeCheckpoint,
    ) -> Result<Vec<NativeCastFact>, NativeCheckpointError> {
        if checkpoint.basis().game_id != self.game
            || checkpoint.basis().context_id != self.context
            || checkpoint.roster.len() != self.seats.len()
            || checkpoint
                .roster
                .iter()
                .any(|seat| !self.seats.contains_key(&seat.seat_id))
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let mut facts = Vec::new();
        for row in checkpoint.cast_activity()? {
            let account = self
                .seats
                .get(&row.seat_id)
                .ok_or(NativeCheckpointError::Invalid)?;
            let Some(account_id) = account else { continue };
            for (kind, count) in [
                ("native:cast-red-or-green", row.red_or_green.get()),
                ("native:cast-black-or-green", row.black_or_green.get()),
            ] {
                if count > 0 {
                    facts.push(NativeCastFact {
                        event_index: 0.into(),
                        account_id: *account_id,
                        kind,
                        revision: "native-cast-colors-v1",
                        value: count.into(),
                    });
                }
            }
        }
        facts.sort_by_key(|fact| (fact.account_id, fact.kind));
        Ok(facts)
    }
}
impl NativeCastAccounts {
    pub(super) fn facts_for_schema(
        &self,
        checkpoint: &NativeCheckpoint,
        schema: crate::admission_ports::NativeEventSchema,
    ) -> Result<Vec<NativeCastFact>, NativeCheckpointError> {
        let mut facts = self.facts(checkpoint)?;
        if schema == crate::admission_ports::NativeEventSchema::LandPlayV1 {
            let seat = checkpoint.land_player()?;
            if let Some(account) = self
                .seats
                .get(&seat)
                .ok_or(NativeCheckpointError::Invalid)?
            {
                facts.push(NativeCastFact {
                    event_index: 0.into(),
                    account_id: *account,
                    kind: "native:land-play",
                    revision: "native-land-play-v1",
                    value: 1.into(),
                });
                facts.sort_by_key(|fact| (fact.account_id, fact.kind));
            }
        }
        Ok(facts)
    }
}
impl NativeCheckpoint {
    pub(super) fn activity_schema(&self) -> crate::admission_ports::NativeEventSchema {
        if self.candidate().land_play().is_some() {
            crate::admission_ports::NativeEventSchema::LandPlayV1
        } else {
            crate::admission_ports::NativeEventSchema::ActivityV1
        }
    }
    pub(super) fn land_player(&self) -> Result<Uuid, NativeCheckpointError> {
        let play = self
            .candidate()
            .land_play()
            .ok_or(NativeCheckpointError::Invalid)?;
        self.roster
            .iter()
            .find(|seat| seat.native_seat == play.player)
            .map(|seat| seat.seat_id)
            .ok_or(NativeCheckpointError::Invalid)
    }
    pub(super) fn cast_activity(&self) -> Result<Vec<NativeCastActivity>, NativeCheckpointError> {
        let mut counts = vec![(0u64, 0u64); self.roster.len()];
        // Project precisely Event occurrences; unrelated rule records/occurrences
        // intentionally carry no completed physical casting credit.
        for record in &self.candidate().rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::CastCompleted(cast) = &occurrence.value {
                        let index = self
                            .roster
                            .iter()
                            .position(|seat| seat.native_seat == cast.controller())
                            .ok_or(NativeCheckpointError::Invalid)?;
                        let colors = &cast.spell().colors;
                        let (red_or_green, black_or_green) = cast_color_families(colors);
                        if red_or_green {
                            counts[index].0 = counts[index]
                                .0
                                .checked_add(1)
                                .ok_or(NativeCheckpointError::Invalid)?;
                        }
                        if black_or_green {
                            counts[index].1 = counts[index]
                                .1
                                .checked_add(1)
                                .ok_or(NativeCheckpointError::Invalid)?;
                        }
                    }
                }
            }
        }
        Ok(self
            .roster
            .iter()
            .zip(counts)
            .filter_map(|(seat, (red, black))| {
                ((red, black) != (0, 0)).then_some(NativeCastActivity {
                    seat_id: seat.seat_id,
                    red_or_green: red.into(),
                    black_or_green: black.into(),
                })
            })
            .collect())
    }
}

fn cast_color_families(colors: &BTreeSet<Color>) -> (bool, bool) {
    (
        colors.contains(&Color::Red) || colors.contains(&Color::Green),
        colors.contains(&Color::Black) || colors.contains(&Color::Green),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_color_families_use_or_once_including_multicolor() {
        use Color::{Black, Blue, Green, Red, White};
        let cases: [(&[Color], (bool, bool)); 8] = [
            (&[], (false, false)),
            (&[White, Blue], (false, false)),
            (&[Red], (true, false)),
            (&[Black], (false, true)),
            (&[Green], (true, true)),
            (&[Red, Green], (true, true)),
            (&[Red, Black], (true, true)),
            (&[White, Blue, Black, Red, Green], (true, true)),
        ];
        for (colors, expected) in cases {
            assert_eq!(
                cast_color_families(&colors.iter().copied().collect()),
                expected
            );
        }
    }
}
