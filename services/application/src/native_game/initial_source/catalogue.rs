//! Preserve the exact decoded source records, without widening Registry membership.
use super::*;
use spellwake_content::InventoryCard;
use std::collections::BTreeMap;

pub(super) fn retain(
    cards: &[InventoryCard],
    registry: &Registry,
) -> Result<BTreeMap<OracleId, InventoryCard>, NativeInitialError> {
    // Coverage supplies candidates, not readiness. Only an actual Registry key
    // can be retained. Invalid/unbound catalogue IDs cannot resolve and remain
    // outside this selected scope; guarded constructors own selected-source truth.
    let expected: BTreeSet<_> = registry
        .coverage()
        .cards
        .iter()
        .filter_map(|row| row.oracle_id.as_ref())
        .filter_map(|raw| OracleId::try_from(raw.clone()).ok())
        .filter(|id| registry.resolve(id).is_some())
        .collect();
    if expected.is_empty() {
        return Err(NativeInitialError::Invalid);
    }
    let keys: BTreeMap<_, _> = expected.iter().map(|id| (id.as_str(), id)).collect();
    let mut retained = BTreeMap::new();
    for card in cards {
        let Some(id) = card.oracle_id.as_deref().and_then(|raw| keys.get(raw)) else {
            continue;
        };
        if retained.insert((*id).clone(), card.clone()).is_some() {
            return Err(NativeInitialError::Invalid);
        }
    }
    if !retained.keys().eq(expected.iter()) {
        return Err(NativeInitialError::Invalid);
    }
    Ok(retained)
}
