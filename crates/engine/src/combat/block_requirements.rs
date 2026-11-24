//! Compare block declarations against the greatest attainable requirement score (CR 509.1c).
use super::*;

impl BlockersDecision {
    /// Blockers that every maximum-scoring declaration must include when all qualify.
    pub fn mandatory_blockers(&self) -> impl Iterator<Item = ObjectId> + '_ {
        let attainable: usize = self
            .blockers
            .iter()
            .map(|&blocker| self.unconstrained_score(blocker))
            .sum();
        self.blockers.iter().copied().filter(move |&blocker| {
            self.block_requirement_score == attainable && self.unconstrained_score(blocker) > 0
        })
    }

    /// Whether at least one blocking requirement can be satisfied in this declaration.
    pub fn has_satisfiable_block_requirements(&self) -> bool {
        self.block_requirement_score > 0
    }

    pub(super) fn block_requirement(&self, blocker: ObjectId) -> usize {
        self.block_requirements.get(&blocker).copied().unwrap_or(0)
    }

    pub(super) fn blocked_attacker_requirement(&self, attacker: ObjectId) -> usize {
        self.blocked_attacker_requirements
            .get(&attacker)
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn must_block_pair(&self, blocker: ObjectId, attacker: ObjectId) -> usize {
        self.must_block_pairs
            .get(&(blocker, attacker))
            .copied()
            .unwrap_or(0)
    }

    fn unconstrained_score(&self, blocker: ObjectId) -> usize {
        let mut scores = self
            .legal_attackers(blocker)
            .iter()
            .map(|&attacker| {
                self.blocked_attacker_requirement(attacker)
                    + self.must_block_pair(blocker, attacker)
                    + usize::from(self.require_at_least_one_block.contains(&attacker))
            })
            .collect::<Vec<_>>();
        scores.sort_unstable_by(|left, right| right.cmp(left));
        self.block_requirement(blocker)
            + scores
                .into_iter()
                .take(self.maximum_blocks(blocker))
                .sum::<usize>()
    }

    pub(super) fn maximum_block_requirement_score(&self) -> usize {
        if self.block_requirements.is_empty()
            && self.blocked_attacker_requirements.is_empty()
            && self.must_block_pairs.is_empty()
            && self.require_at_least_one_block.is_empty()
        {
            return 0;
        }
        let upper_scores = self
            .blockers
            .iter()
            .map(|&blocker| self.unconstrained_score(blocker));
        if self.single_blocker {
            return upper_scores.max().unwrap_or(0);
        }
        let upper_bound = upper_scores.sum();
        if (self.block_minimums.is_empty() && self.require_at_least_one_block.is_empty())
            || upper_bound == 0
        {
            return upper_bound;
        }
        let mut group_minimums = self.block_minimums.clone();
        for &attacker in &self.require_at_least_one_block {
            group_minimums.entry(attacker).or_insert(1);
        }
        let constrained: BTreeMap<_, _> = group_minimums
            .keys()
            .enumerate()
            .map(|(index, attacker)| (*attacker, index))
            .collect();
        let minimums = group_minimums.values().copied().collect::<Vec<_>>();
        let required = group_minimums
            .keys()
            .map(|id| self.require_at_least_one_block.contains(id))
            .collect::<Vec<_>>();
        let required_score = |counts: &[usize]| {
            counts
                .iter()
                .zip(&required)
                .filter(|(count, required)| **count > 0 && **required)
                .count()
        };
        let mut states = BTreeMap::from([(vec![0; minimums.len()], 0)]);
        for &blocker in &self.blockers {
            let mut assignments = BTreeMap::from([((vec![0; minimums.len()], 0), 0)]);
            for &attacker in self.legal_attackers(blocker) {
                let group = constrained.get(&attacker).copied();
                let score = self.blocked_attacker_requirement(attacker)
                    + self.must_block_pair(blocker, attacker);
                if group.is_none() && score == 0 && self.block_requirement(blocker) == 0 {
                    continue;
                }
                for ((mut counts, assigned), previous) in assignments.clone() {
                    if assigned == self.maximum_blocks(blocker) {
                        continue;
                    }
                    if let Some(group) = group {
                        add_block(&mut counts, &minimums, group);
                    }
                    let generic_requirement = if assigned == 0 {
                        self.block_requirement(blocker)
                    } else {
                        0
                    };
                    store_score(
                        &mut assignments,
                        (counts, assigned + 1),
                        previous + score + generic_requirement,
                    );
                }
            }
            let mut next = BTreeMap::new();
            for (existing, previous) in states {
                for ((counts, _), score) in &assignments {
                    let combined: Vec<_> = existing
                        .iter()
                        .zip(counts)
                        .zip(&minimums)
                        .map(|((&left, &right), &minimum)| (left + right).min(minimum))
                        .collect();
                    store_score(&mut next, combined, previous + score);
                }
            }
            if next.iter().any(|(counts, score)| {
                *score + required_score(counts) == upper_bound && complete_groups(counts, &minimums)
            }) {
                return upper_bound;
            }
            states = next;
        }
        states
            .into_iter()
            .filter_map(|(counts, score)| {
                complete_groups(&counts, &minimums).then(|| score + required_score(&counts))
            })
            .max()
            .unwrap_or(0)
    }
}

fn add_block(counts: &mut [usize], minimums: &[usize], target: usize) {
    if let Some(count) = counts.get_mut(target) {
        *count = (*count + 1).min(minimums[target]);
    }
}

fn store_score<K: Ord>(states: &mut BTreeMap<K, usize>, counts: K, score: usize) {
    states
        .entry(counts)
        .and_modify(|best| *best = (*best).max(score))
        .or_insert(score);
}

fn complete_groups(counts: &[usize], minimums: &[usize]) -> bool {
    counts
        .iter()
        .zip(minimums)
        .all(|(&count, &minimum)| count == 0 || count == minimum)
}
