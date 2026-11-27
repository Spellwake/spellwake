//! Goad designations and the attack declarations satisfying the most requirements.
use super::*;
use crate::{combat::AttackersDecision, program::AttackRequirement};

impl EffectWindow {
    pub(super) fn defender_attack_cost(
        &self,
        target: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> Result<crate::combat::AttackCost, EffectError> {
        let player = match target {
            TargetRef::Player(player) => player,
            TargetRef::Object(id) => {
                let permanent = current.get(id).ok_or(CombatError::IllegalDefender)?;
                if !permanent.has_type(CardType::Planeswalker) {
                    return Ok(crate::combat::AttackCost::default());
                }
                permanent.controller.ok_or(CombatError::IllegalDefender)?
            }
            TargetRef::Stack(_) => return Err(CombatError::IllegalDefender.into()),
        };
        let mut cost = crate::combat::AttackCost::default();
        for object in current.objects.values().filter(|object| {
            object.zone == ObjectZone::Battlefield && object.controller == Some(player)
        }) {
            for req in object.abilities.all_attack_requirements() {
                if let AttackRequirement::PayPerAttacker {
                    protect_player,
                    symbol,
                } = req
                    && (protect_player || matches!(target, TargetRef::Object(_)))
                {
                    match symbol {
                        crate::program::AttackCostSymbol::OneGeneric => {
                            cost.generic = cost.generic.plus(&1.into())
                        }
                        crate::program::AttackCostSymbol::Generic(n) => {
                            cost.generic = cost.generic.plus(&i64::from(n).into())
                        }
                        crate::program::AttackCostSymbol::WhitePhyrexian => {
                            cost.white_phyrexian = cost.white_phyrexian.plus(&1.into())
                        }
                    }
                }
            }
        }
        Ok(cost)
    }

    /// Each goading player creates one requirement to attack and one to attack a different player.
    /// Attacking a planeswalker fulfills only the first requirement.
    pub(super) fn apply_attack_requirements(
        &self,
        frame: &mut AttackersDecision,
        current: &EvaluatedCharacteristics,
    ) -> Result<(), EffectError> {
        let mut goaders = self.goaded.clone();
        let mut protected_from_flying = BTreeSet::new();
        let mut protected_from_enchanted = BTreeSet::new();
        let mut everyone_attacks = 0usize;
        let mut attached_attack_requirements = self.creatures_required_to_attack.clone();
        let mut single_attacker = false;
        let mut scores = BTreeMap::new();
        for p in current
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            single_attacker |= p.abilities.combat_limits.one_attacker;
            for req in p.abilities.all_attack_requirements() {
                match req {
                    AttackRequirement::LimitAttacksAtTappedController
                        if self
                            .window
                            .cards()
                            .get(p.object)
                            .and_then(GameObject::tapped)
                            == Some(true) =>
                    {
                        if let Some(controller) = p.controller
                            && frame.defenders().contains(&controller)
                        {
                            frame.capped_defenders.insert(controller);
                        }
                    }

                    AttackRequirement::FlyingCannotAttackController => {
                        if let Some(seat) = p.controller {
                            protected_from_flying.insert(seat);
                        }
                    }
                    AttackRequirement::EnchantedCannotAttackOrBlockController
                    | AttackRequirement::EquippedCannotAttackControllerOrBeSacrificed
                    | AttackRequirement::AttachedCannotAttackController => {
                        if let (Some(seat), Some(TargetRef::Object(host))) =
                            (p.controller, p.attached_to)
                        {
                            protected_from_enchanted.insert((host, seat));
                        }
                    }
                    AttackRequirement::SourceCannotAttackController => {
                        if let Some(seat) = p.controller {
                            protected_from_enchanted.insert((p.object, seat));
                        }
                    }
                    AttackRequirement::AllCreaturesAttack => everyone_attacks += 1,
                    AttackRequirement::EnchantedPlayerCreaturesAttack
                        if p.attached_to
                            == Some(TargetRef::Player(self.window.active_player())) =>
                    {
                        everyone_attacks += 1;
                    }

                    AttackRequirement::AttachedCreatureAttacks => {
                        if let Some(TargetRef::Object(host)) = p.attached_to {
                            *attached_attack_requirements.entry(host).or_insert(0usize) += 1;
                        }
                    }
                    AttackRequirement::EnchantedCreatureIsGoaded => {
                        if let (Some(TargetRef::Object(host)), Some(controller)) =
                            (p.attached_to, p.controller)
                        {
                            goaders.entry(host).or_default().insert(controller);
                        }
                    }
                    _ => {}
                }
            }
        }
        let targets: Vec<_> = frame
            .defenders()
            .iter()
            .copied()
            .map(TargetRef::Player)
            .chain(
                frame
                    .planeswalkers()
                    .iter()
                    .chain(frame.battles())
                    .copied()
                    .map(TargetRef::Object),
            )
            .collect();
        for &target in &targets {
            let cost = self.defender_attack_cost(target, current)?;
            if cost != crate::combat::AttackCost::default() {
                frame.defender_costs.insert(target, cost);
            }
        }
        let seats = self.window.participants();
        let mut allowed_directional_defenders: BTreeSet<_> =
            frame.defenders().iter().copied().collect();
        if !self.attack_directions.is_empty() {
            let at = seats
                .iter()
                .position(|player| *player == frame.chooser())
                .ok_or(EffectError::InconsistentProgram)?;
            for (_, direction) in &self.attack_directions {
                let nearest = (1..seats.len())
                    .map(|offset| match direction {
                        AttackDirection::Left => seats[(at + offset) % seats.len()],
                        AttackDirection::Right => seats[(at + seats.len() - offset) % seats.len()],
                    })
                    .find(|player| frame.defenders().contains(player));
                allowed_directional_defenders.retain(|player| Some(*player) == nearest);
            }
        }
        for attacker in frame.eligible_attackers().to_vec() {
            let own_rules: Vec<AttackRequirement> = current
                .get(attacker)
                .ok_or(EffectError::InconsistentProgram)?
                .abilities
                .all_attack_requirements()
                .collect();
            let own_rule = own_rules
                .iter()
                .copied()
                .find(|r| {
                    matches!(
                        r,
                        AttackRequirement::SourceAttacks
                            | AttackRequirement::SourceAttacksExceptOwner
                    )
                })
                .or_else(|| own_rules.first().copied());
            let owner = self
                .window
                .cards()
                .get(attacker)
                .ok_or(EffectError::InconsistentProgram)?
                .owner();
            let goaders = goaders.get(&attacker);
            let attack_score = everyone_attacks
                + attached_attack_requirements
                    .get(&attacker)
                    .copied()
                    .unwrap_or_default()
                + goaders.map_or(0, BTreeSet::len)
                + usize::from(matches!(
                    own_rule,
                    Some(
                        AttackRequirement::SourceAttacks
                            | AttackRequirement::SourceAttacksExceptOwner
                    )
                ))
                + current.get(attacker).map_or(0, |p| {
                    p.blocking_restrictions
                        .iter()
                        .filter(|r| {
                            matches!(
                            r,
                            crate::characteristics::BlockingRestriction::MustAttackEachCombatIfAble
                        )
                        })
                        .count()
                });
            let flying = current
                .get(attacker)
                .is_some_and(|p| p.has_keyword(KeywordAbility::Flying));
            let mut scored = Vec::new();
            for &target in &targets {
                let defending_player = match target {
                    TargetRef::Player(player) => player,
                    TargetRef::Object(object) => current
                        .get(object)
                        .and_then(|p| self.permanent_defender(p))
                        .ok_or(EffectError::InconsistentProgram)?,
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                let vow_protects_defender = self
                    .characteristics
                    .vow_protected_players
                    .get(&attacker)
                    .is_some_and(|seats| seats.contains(&defending_player));
                let missing_required_defender_permanent = current.get(attacker).is_some_and(|att| {
                    let excluded = BTreeSet::new();
                    att.blocking_restrictions.iter().any(|rule| match rule {
                        crate::characteristics::BlockingRestriction::CannotAttackPlayer(
                            protected_seat,
                        ) => *protected_seat == defending_player,
                        crate::characteristics::BlockingRestriction::CannotAttackOwner => {
                            defending_player == owner
                        }
                        crate::characteristics::BlockingRestriction::CannotAttackUnlessDefenderControls(
                            filter,
                        ) => !current.objects.values().any(|perm| {
                            perm.zone == ObjectZone::Battlefield
                                && perm.controller == Some(defending_player)
                                && self.matches_object_filter(
                                    *filter,
                                    perm,
                                    defending_player,
                                    Some(attacker),
                                    &excluded,
                                    current,
                                )
                        }),
                        _ => false,
                    })
                });
                if vow_protects_defender
                    || missing_required_defender_permanent
                    || !allowed_directional_defenders.contains(&defending_player)
                    || (own_rule == Some(AttackRequirement::SourceAttacksExceptOwner)
                        && defending_player == owner)
                    || (flying && protected_from_flying.contains(&defending_player))
                    || protected_from_enchanted.contains(&(attacker, defending_player))
                {
                    frame.forbid_defender(attacker, target);
                    continue;
                }
                let score = match (target, goaders) {
                    (TargetRef::Player(player), Some(controllers)) => {
                        controllers.iter().filter(|seat| **seat != player).count()
                    }
                    _ => 0,
                };
                let walker_score = self
                    .required_planeswalker_attacks
                    .iter()
                    .filter(|id| target == TargetRef::Object(**id))
                    .count();
                let encore_score = usize::from(
                    self.encore_attack_targets
                        .get(&attacker)
                        .is_some_and(|player| target == TargetRef::Player(*player)),
                );
                scored.push((target, attack_score + score + walker_score + encore_score));
            }
            if scored.is_empty() {
                frame.exclude_attacker(attacker);
            } else {
                scores.insert(attacker, scored);
            }
        }

        // A player never has to pay to satisfy a requirement. Voluntary paid declarations
        // can satisfy at least as many requirements as the best unrestricted free one.
        if !frame.capped_defenders.is_empty() {
            if single_attacker {
                frame.limit_to_one_attacker(&BTreeSet::new());
            }
            frame.scores_with_caps = scores;
            return Ok(());
        }
        let free_scores = scores
            .iter()
            .map(|(&id, options)| {
                let best = options
                    .iter()
                    .filter(|(target, _)| !frame.defender_costs.contains_key(target))
                    .map(|(_, score)| *score)
                    .max()
                    .unwrap_or(0);
                (id, best)
            })
            .collect::<BTreeMap<_, _>>();
        let table_best = free_scores.values().copied().max().unwrap_or(0);
        let mut single_candidates = BTreeSet::new();
        for (attacker, destinations) in scores {
            let minimum = if single_attacker {
                table_best
            } else {
                free_scores[&attacker]
            };
            let mut valid = BTreeSet::new();
            for (target, score) in destinations {
                if score >= minimum {
                    valid.insert(target);
                } else {
                    frame.forbid_defender(attacker, target);
                }
            }
            if !valid.is_empty() {
                single_candidates.insert(attacker);
                if minimum > 0 {
                    frame.require_attack(attacker, valid);
                }
            }
        }
        if single_attacker {
            frame.limit_to_one_attacker(&single_candidates);
        }
        Ok(())
    }
}
