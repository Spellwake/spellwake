//! Actual World history only. Pure evaluation and dependency probes never call here.
use super::*;
use crate::program::Supertype;

fn current_worlds(current: &EvaluatedCharacteristics) -> BTreeSet<ObjectId> {
    current
        .objects
        .iter()
        .filter_map(|(id, object)| {
            (object.zone == ObjectZone::Battlefield
                && object.current_supertypes.contains(&Supertype::World))
            .then_some(*id)
        })
        .collect()
}

impl CharacteristicState {
    /// Trusted constructor-only initialization, with no fabricated entry events.
    /// Reject incomplete/duplicate/empty/non-World cohorts before changing history.
    pub(crate) fn initialize_world_tenures(
        &mut self,
        cards: &GameObjects,
        current: &EvaluatedCharacteristics,
        cohorts: &[Vec<CardId>],
    ) -> Result<(), CharacteristicsError> {
        if self.next_world_tenure != 1 || !self.world_tenures.is_empty() {
            return Err(CharacteristicsError::InvalidInitialWorldCohorts);
        }
        let expected = current_worlds(current);
        let mut supplied = BTreeSet::new();
        let mut resolved = Vec::new();
        for cohort in cohorts {
            if cohort.is_empty() {
                return Err(CharacteristicsError::InvalidInitialWorldCohorts);
            }
            let mut objects = Vec::new();
            for card in cohort {
                let id = cards
                    .current_identity(*card)
                    .ok_or(CharacteristicsError::InvalidInitialWorldCohorts)?;
                if !expected.contains(&id) || !supplied.insert(id) {
                    return Err(CharacteristicsError::InvalidInitialWorldCohorts);
                }
                objects.push(id);
            }
            resolved.push(objects);
        }
        if supplied != expected {
            return Err(CharacteristicsError::InvalidInitialWorldCohorts);
        }
        let count =
            u64::try_from(cohorts.len()).map_err(|_| CharacteristicsError::WorldTenureExhausted)?;
        let next = self
            .next_world_tenure
            .checked_add(count)
            .ok_or(CharacteristicsError::WorldTenureExhausted)?;
        for (objects, sequence) in resolved.into_iter().zip(self.next_world_tenure..next) {
            for object in objects {
                self.world_tenures.insert(object, WorldTenure(sequence));
            }
        }
        self.next_world_tenure = next;
        Ok(())
    }

    /// Reconcile once on an actual tentative successor, after full evaluation.
    /// All same-cut gainers share one cohort. Overflow leaves this owner unchanged.
    pub(crate) fn reconcile_world_tenures(
        &mut self,
        current: &EvaluatedCharacteristics,
    ) -> Result<Vec<WorldTenureChange>, CharacteristicsError> {
        let worlds = current_worlds(current);
        let gainers: Vec<_> = worlds
            .iter()
            .copied()
            .filter(|id| !self.world_tenures.contains_key(id))
            .collect();
        let next = if gainers.is_empty() {
            self.next_world_tenure
        } else {
            self.next_world_tenure
                .checked_add(1)
                .ok_or(CharacteristicsError::WorldTenureExhausted)?
        };
        let mut changes = Vec::new();
        self.world_tenures.retain(|object, tenure| {
            if worlds.contains(object) {
                true
            } else {
                changes.push(WorldTenureChange::Lost {
                    object: *object,
                    tenure: *tenure,
                });
                false
            }
        });
        let tenure = WorldTenure(self.next_world_tenure);
        for object in gainers {
            self.world_tenures.insert(object, tenure);
            changes.push(WorldTenureChange::Gained { object, tenure });
        }
        self.next_world_tenure = next;
        Ok(changes)
    }

    pub(crate) fn world_tenures(&self) -> impl Iterator<Item = (ObjectId, WorldTenure)> + '_ {
        self.world_tenures
            .iter()
            .map(|(object, tenure)| (*object, *tenure))
    }
    pub(crate) fn next_world_tenure_sequence(&self) -> u64 {
        self.next_world_tenure
    }
    #[cfg(test)]
    pub(crate) fn qualification_set_next_world_tenure(&mut self, value: u64) {
        self.next_world_tenure = value;
    }

    /// CR 704.5k eligibility, without movement or choices. Validate complete live
    /// membership even with zero/one World; missing history never guesses a clock.
    pub(crate) fn world_eliminations(
        &self,
        current: &EvaluatedCharacteristics,
    ) -> Result<BTreeSet<ObjectId>, CharacteristicInvariantError> {
        let worlds = current_worlds(current);
        for object in &worlds {
            if !self.world_tenures.contains_key(object) {
                return Err(CharacteristicInvariantError::MissingWorldTenure(*object));
            }
        }
        for object in self.world_tenures.keys() {
            if !worlds.contains(object) {
                return Err(CharacteristicInvariantError::UnexpectedWorldTenure(*object));
            }
        }
        if worlds.len() <= 1 {
            return Ok(BTreeSet::new());
        }
        let latest = self.world_tenures.values().max().copied();
        let latest_objects: Vec<_> = self
            .world_tenures
            .iter()
            .filter_map(|(object, tenure)| (Some(*tenure) == latest).then_some(*object))
            .collect();
        Ok(if let [survivor] = latest_objects.as_slice() {
            worlds
                .into_iter()
                .filter(|object| object != survivor)
                .collect()
        } else {
            worlds
        })
    }
}
