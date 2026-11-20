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
/// earlier chains determine eligibility. IDs never resolve timestamp ties.
pub(super) fn select(
    timestamps: &BTreeMap<EffectInstanceKey, CharacteristicTimestamp>,
    original_waits: &BTreeSet<ComponentWait>,
) -> Result<Selection, CharacteristicInvariantError> {
    let mut by_timestamp: Vec<_> = timestamps.keys().copied().collect();
    by_timestamp.sort_by_key(|key| timestamps[key]);
    for adjacent in by_timestamp.windows(2) {
        if timestamps[&adjacent[0]] == timestamps[&adjacent[1]] {
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
            ordered.sort_by_key(|key| timestamps[key]);
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
