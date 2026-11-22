//! One selection from the current original dependency graph. Derived loop chains
//! exist only for this selection; the evaluator regenerates after applying one part.
use super::{
    CharacteristicInvariantError, CharacteristicTimestamp, ComponentWait, EffectInstanceKey,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Selection {
    pub strongly_connected: Vec<BTreeSet<EffectInstanceKey>>,
    pub derived_timestamp_waits: BTreeSet<ComponentWait>,
    pub retained_outside_waits: BTreeSet<ComponentWait>,
    pub eligible: Vec<EffectInstanceKey>,
    pub chosen: EffectInstanceKey,
}
fn reachable(
    start: EffectInstanceKey,
    waits: &BTreeSet<ComponentWait>,
) -> BTreeSet<EffectInstanceKey> {
    let mut seen = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(current) = pending.pop() {
        for wait in waits.iter().filter(|wait| wait.waiting == current) {
            if seen.insert(wait.prerequisite) {
                pending.push(wait.prerequisite);
            }
        }
    }
    seen
}
/// Original maximal SCCs, original inter-SCC waits and strict later-to-immediate-
/// earlier chains determine eligibility. Static abilities of the same source share its
/// timestamp and retain definition order when independent.
pub(super) fn select(
    timestamps: &BTreeMap<EffectInstanceKey, CharacteristicTimestamp>,
    original_waits: &BTreeSet<ComponentWait>,
) -> Result<Selection, CharacteristicInvariantError> {
    let mut by_timestamp: Vec<_> = timestamps.keys().copied().collect();
    by_timestamp.sort_by_key(|key| (timestamps[key], *key));
    for adjacent in by_timestamp.windows(2) {
        let same_static_source = matches!((adjacent[0],adjacent[1]),(
            EffectInstanceKey::Static{source:first,..}, EffectInstanceKey::Static{source:second,..}
        ) if first==second);
        if timestamps[&adjacent[0]] == timestamps[&adjacent[1]] && !same_static_source {
            return Err(CharacteristicInvariantError::DuplicateTimestamp(
                timestamps[&adjacent[0]],
            ));
        }
    }
    let reach: BTreeMap<_, _> = by_timestamp
        .iter()
        .map(|key| (*key, reachable(*key, original_waits)))
        .collect();
    let mut components = Vec::new();
    let mut component_of = BTreeMap::new();
    for key in &by_timestamp {
        if component_of.contains_key(key) {
            continue;
        }
        let component: BTreeSet<_> = by_timestamp
            .iter()
            .copied()
            .filter(|other| reach[key].contains(other) && reach[other].contains(key))
            .collect();
        let index = components.len();
        for member in &component {
            component_of.insert(*member, index);
        }
        components.push(component);
    }
    let retained_outside_waits: BTreeSet<_> = original_waits
        .iter()
        .copied()
        .filter(|wait| component_of[&wait.waiting] != component_of[&wait.prerequisite])
        .collect();
    let mut derived_timestamp_waits = BTreeSet::new();
    for component in &components {
        if component.len() > 1
            || component.iter().any(|key| {
                original_waits.contains(&ComponentWait {
                    waiting: *key,
                    prerequisite: *key,
                })
            })
        {
            let mut ordered: Vec<_> = component.iter().copied().collect();
            ordered.sort_by_key(|key| (timestamps[key], *key));
            for adjacent in ordered.windows(2) {
                derived_timestamp_waits.insert(ComponentWait {
                    waiting: adjacent[1],
                    prerequisite: adjacent[0],
                });
            }
        }
    }
    let eligible: Vec<_> = by_timestamp
        .into_iter()
        .filter(|key| {
            !retained_outside_waits
                .iter()
                .chain(derived_timestamp_waits.iter())
                .any(|wait| wait.waiting == *key)
        })
        .collect();
    let chosen = eligible
        .first()
        .copied()
        .ok_or(CharacteristicInvariantError::NoEligibleComponent)?;
    Ok(Selection {
        strongly_connected: components,
        derived_timestamp_waits,
        retained_outside_waits,
        eligible,
        chosen,
    })
}

#[cfg(test)]
mod same_source_tests {
    use super::*;
    use crate::{
        characteristics::StaticAbilityId,
        object_identity::{CardId, GameObjects, ObjectDestination},
        priority::SeatId,
    };
    #[test]
    fn static_timestamp_ties_use_source_ability_order_only_after_dependencies() {
        let first = GameObjects::new(0)
            .seed_card(0, CardId(1), SeatId(0), ObjectDestination::Hand)
            .unwrap();
        let second = first
            .next
            .seed_card(
                first.next.revision(),
                CardId(2),
                SeatId(0),
                ObjectDestination::Hand,
            )
            .unwrap();
        let static_key = |source, number| EffectInstanceKey::Static {
            source,
            ability: StaticAbilityId(number),
        };
        let a = static_key(first.after.id(), 1);
        let b = static_key(first.after.id(), 2);
        let unrelated = static_key(second.after.id(), 1);
        let time = CharacteristicTimestamp(3);
        let tied = [(a, time), (b, time)].into();
        assert_eq!(select(&tied, &BTreeSet::new()).unwrap().chosen, a);
        let dependent = ComponentWait {
            waiting: a,
            prerequisite: b,
        };
        assert_eq!(select(&tied, &[dependent].into()).unwrap().chosen, b);
        let cyclic = [
            dependent,
            ComponentWait {
                waiting: b,
                prerequisite: a,
            },
        ]
        .into();
        assert_eq!(select(&tied, &cyclic).unwrap().chosen, a);
        assert!(
            matches!(select(&[(a,time),(unrelated,time)].into(),&BTreeSet::new()),Err(CharacteristicInvariantError::DuplicateTimestamp(t)) if t==time)
        );
    }
}
