//! Maximize attack requirements when several defenders permit only one attacker.
use super::*;
use crate::targets::TargetRef;

impl AttackersDecision {
    pub(super) fn validate_capped_selection(
        &self,
        attacks: &BTreeMap<ObjectId, TargetRef>,
    ) -> Result<(), CombatError> {
        let mut occupied = BTreeSet::new();
        for &defender in attacks.values() {
            if let TargetRef::Player(player) = defender
                && self.capped_defenders.contains(&player)
                && !occupied.insert(player)
            {
                return Err(CombatError::IllegalDefender);
            }
        }
        let maximizable = |id: ObjectId, target: TargetRef| {
            !self.defender_costs.contains_key(&target) || attacks.get(&id) == Some(&target)
        };
        let score = self
            .scores_with_caps
            .iter()
            .map(|(id, options)| {
                options
                    .iter()
                    .find(|(target, _)| attacks.get(id) == Some(target))
                    .map_or(0, |(_, score)| *score)
            })
            .sum::<usize>();
        if self.single_attacker {
            let best = self
                .scores_with_caps
                .iter()
                .flat_map(|(&id, options)| {
                    options
                        .iter()
                        .filter(move |(target, _)| maximizable(id, *target))
                        .map(|(_, score)| *score)
                })
                .max()
                .unwrap_or(0);
            return if score >= best {
                Ok(())
            } else {
                Err(CombatError::IllegalAttacker)
            };
        }
        let defenders = self.capped_defenders.iter().copied().collect::<Vec<_>>();
        let mut benefits = vec![Vec::new(); defenders.len()];
        let mut base = 0;
        for (&id, options) in &self.scores_with_caps {
            // Each attacker can always stay back. Uncapped destinations are independent.
            let available = options
                .iter()
                .copied()
                .filter(|(target, _)| maximizable(id, *target));
            let free = available.clone().filter(|(target,_)|
                !matches!(target,TargetRef::Player(seat) if self.capped_defenders.contains(seat)))
                .map(|(_,weight)|weight).max().unwrap_or(0);
            base += free;
            for (&seat, gains) in defenders.iter().zip(&mut benefits) {
                let best = available
                    .clone()
                    .find(|(target, _)| *target == TargetRef::Player(seat))
                    .map_or(0, |(_, score)| score);
                gains.push(best.saturating_sub(free));
            }
        }
        if score >= base + maximum_matching_benefit(&benefits) {
            Ok(())
        } else {
            Err(CombatError::IllegalAttacker)
        }
    }

    pub(crate) fn validate_defender_caps(
        &self,
        selected: &[AttackDeclaration],
    ) -> Result<(), CombatError> {
        if self.capped_defenders.is_empty() {
            return Ok(());
        }
        let attacks = selected
            .iter()
            .map(|attack| {
                (
                    attack.attacker,
                    attack
                        .defending_planeswalker
                        .map_or(TargetRef::Player(attack.defender), TargetRef::Object),
                )
            })
            .collect();
        self.validate_capped_selection(&attacks)
    }
}

/// Maximum-weight bipartite assignment. Rows are defenders with a one-attacker limit,
/// columns are creatures, and zero-weight dummy columns allow a defender to take none.
fn maximum_matching_benefit(weights: &[Vec<usize>]) -> usize {
    let rows = weights.len();
    if rows == 0 {
        return 0;
    }
    let creatures = weights[0].len();
    let columns = rows.max(creatures);
    let mut row_price = vec![0i128; rows + 1];
    let mut column_price = vec![0i128; columns + 1];
    let mut row_at_column = vec![0; columns + 1];
    for row in 1..=rows {
        row_at_column[0] = row;
        let mut visited = vec![false; columns + 1];
        let mut slack = vec![i128::MAX; columns + 1];
        let mut previous = vec![0; columns + 1];
        let mut column = 0;
        loop {
            visited[column] = true;
            let active = row_at_column[column];
            let mut next_column = 0;
            let mut delta = i128::MAX;
            for candidate in 1..=columns {
                if visited[candidate] {
                    continue;
                }
                let benefit = weights[active - 1].get(candidate - 1).copied().unwrap_or(0) as i128;
                let reduced = -benefit - row_price[active] - column_price[candidate];
                if reduced < slack[candidate] {
                    slack[candidate] = reduced;
                    previous[candidate] = column;
                }
                if slack[candidate] < delta {
                    delta = slack[candidate];
                    next_column = candidate;
                }
            }
            for index in 0..=columns {
                if visited[index] {
                    row_price[row_at_column[index]] += delta;
                    column_price[index] -= delta;
                } else {
                    slack[index] -= delta;
                }
            }
            column = next_column;
            if row_at_column[column] == 0 {
                break;
            }
        }
        while column != 0 {
            let from = previous[column];
            row_at_column[column] = row_at_column[from];
            column = from;
        }
    }
    row_at_column
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(column, &row)| {
            row.checked_sub(1)
                .and_then(|row| weights[row].get(column - 1))
                .copied()
        })
        .sum()
}
