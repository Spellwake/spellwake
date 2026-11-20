//! Complete public observations from independently projected consecutive cuts.
use super::*;
use std::collections::BTreeMap;

impl PlayerVisibleEvent {
    /// Produce every changed public value, or no event for a silent accepted cut.
    ///
    /// The caller must supply independently qualified historical projections and
    /// the actual terminal result at this cut. This pure operation establishes
    /// neither native execution nor participant/archive authority. Metadata-only
    /// coordinate advancement is deliberately not a public change. The enclosing
    /// page/HTTP serializer still enforces its aggregate byte and work limits.
    ///
    /// # Errors
    /// Rejects invalid projections, foreign viewers/epochs/rosters, nonadvancing
    /// coordinates, inconsistent disclosure or an invalid/oversized change list.
    pub fn from_projections(f: &PlayerEventFacts<'_>) -> Result<Option<Self>, InvalidPlayerWire> {
        f.before.validate(&f.before_projection)?;
        f.after.validate(&f.after_projection)?;
        if f.before.viewer_seat_id != f.after.viewer_seat_id
            || f.before.basis.game_id != f.after.basis.game_id
            || f.before.basis.context_id != f.after.basis.context_id
            || f.before.basis.epoch != f.after.basis.epoch
            || f.before.basis.revision > f.after.basis.revision
            || f.before.through_event_sequence >= f.after.through_event_sequence
            || f.before_projection.roster != f.after_projection.roster
            || f.before_projection.authorized_full_disclosure
                != f.after_projection.authorized_full_disclosure
        {
            return Err(InvalidPlayerWire);
        }
        let mut changes = Vec::new();
        for (before, after) in f.before.players.iter().zip(&f.after.players) {
            if before == after {
                continue;
            }
            // Compare complete records so future public fields cannot silently
            // disappear into a life/counter-only observation.
            let mut life_only = before.clone();
            life_only.life = after.life.clone();
            let mut counters_only = before.clone();
            counters_only.counters.clone_from(&after.counters);
            changes.push(if &life_only == after {
                PlayerPublicChange::Life {
                    seat_id: after.seat_id,
                    before: before.life.clone(),
                    after: after.life.clone(),
                }
            } else if &counters_only == after {
                PlayerPublicChange::Counters {
                    subject: VisibleSubject::try_from(PlayerTarget::Seat {
                        seat_id: after.seat_id,
                    })?,
                    counters: after.counters.clone(),
                }
            } else {
                PlayerPublicChange::ReplaceSeat {
                    seat: after.clone(),
                }
            });
        }
        let before_objects: BTreeMap<_, _> = f
            .before
            .visible_objects
            .iter()
            .map(|object| (&object.handle, object))
            .collect();
        let after_objects: BTreeMap<_, _> = f
            .after
            .visible_objects
            .iter()
            .map(|object| (&object.handle, object))
            .collect();
        for handle in before_objects.keys() {
            if !after_objects.contains_key(handle) {
                changes.push(PlayerPublicChange::RemoveObject {
                    handle: (*handle).clone(),
                });
            }
        }
        for object in &f.after.visible_objects {
            if before_objects.get(&object.handle).copied() == Some(object) {
                continue;
            }
            let counters_only = before_objects.get(&object.handle).is_some_and(|before| {
                let mut changed = (*before).clone();
                changed.counters.clone_from(&object.counters);
                &changed == object
            });
            changes.push(if counters_only {
                PlayerPublicChange::Counters {
                    subject: VisibleSubject::try_from(PlayerTarget::Object {
                        handle: object.handle.clone(),
                    })?,
                    counters: object.counters.clone(),
                }
            } else {
                PlayerPublicChange::UpsertObject {
                    object: Box::new(object.clone()),
                }
            });
        }
        for zone in &f.after.zones {
            // A rotated handle replaces the same semantic zone, rather than
            // asserting an additional library/hand/battlefield exists.
            if !f.before.zones.iter().any(|before| {
                before.zone_type() == zone.zone_type()
                    && before.owner_seat_id() == zone.owner_seat_id()
                    && before == zone
            }) {
                changes.push(PlayerPublicChange::ReplaceZone { zone: zone.clone() });
            }
        }
        if f.before.turn != f.after.turn {
            changes.push(PlayerPublicChange::Turn {
                turn: f.after.turn.clone(),
            });
        }
        if f.before.stack != f.after.stack {
            changes.push(PlayerPublicChange::Stack {
                entries: f.after.stack.clone(),
            });
        }
        if f.before.decisions != f.after.decisions {
            changes.push(PlayerPublicChange::Decisions {
                decisions: f.after.decisions.clone(),
            });
        }
        if let Some(result) = f.terminal {
            changes.push(PlayerPublicChange::Terminal {
                result: result.clone(),
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }
        let event = Self {
            basis: f.after.basis.clone(),
            viewer_seat_id: f.after.viewer_seat_id,
            event_sequence: f.after.through_event_sequence.clone(),
            changes,
        };
        event.validate(f)?;
        Ok(Some(event))
    }
}
