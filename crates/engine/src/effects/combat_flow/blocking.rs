//! Creature evasion is checked against the world at the declare-blockers turn action.
use super::*;
use crate::{
    characteristics::{BlockingRestriction, Color},
    land::LandType,
    program::{CardType, Supertype},
};

impl EffectWindow {
    pub(super) fn blocking_domain(&self, actor: SeatId) -> Result<BlockersDecision, EffectError> {
        let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
        let evaluated = self.evaluated_characteristics()?;
        let single_blocker = combat.one_blocker_for.contains(&actor)
            || evaluated.objects.values().any(|view| {
                view.zone == ObjectZone::Battlefield && view.abilities.combat_limits.one_blocker
            });
        let defenders_lands = landwalk_abilities(&evaluated, actor);
        let blockers: Vec<_> = self
            .window
            .cards()
            .iter()
            .filter(|card| {
                card.zone() == ObjectZone::Battlefield
                    && card.controller() == Some(actor)
                    && card.tapped() == Some(false)
            })
            .filter_map(|card| evaluated.get(card.id()))
            .filter(|current| {
                current.is_creature()
                    && !current.has_type(CardType::Battle)
                    && !current.has_keyword(KeywordAbility::Decayed)
                    && self.blessing_allows_combat(current, actor)
                    && self.soulbond_allows_combat(current, &evaluated)
            })
            .collect();
        let barred_blocks: BTreeSet<_> = evaluated.objects.values().filter(|p|p.zone == ObjectZone::Battlefield &&
            p.abilities.attack_requirement == Some(crate::program::AttackRequirement::EnchantedCannotAttackOrBlockController))
            .filter_map(|p|match p.attached_to {Some(TargetRef::Object(host))=>Some((host,p.controller?)),_=>None}).collect();
        let mut choices: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut attackers = Vec::new();
        let defending_creatures = evaluated
            .objects
            .values()
            .filter(|c| {
                c.zone == ObjectZone::Battlefield && c.controller == Some(actor) && c.is_creature()
            })
            .count();
        let mut minimums = BTreeMap::new();
        let mut require_at_least_one_block = BTreeSet::new();
        let mut at_most_one_blocker = BTreeSet::new();
        for attack in combat
            .attackers()
            .filter(|attack| attack.defender() == actor)
        {
            let id = attack.combatant().object();
            let attacker = evaluated.get(id).ok_or(CombatError::InvalidContext)?;
            attackers.push(id);
            use crate::characteristics::BlockingRestriction;
            let mut minimum = if attacker.has_keyword(KeywordAbility::Menace) {
                2
            } else {
                1
            };
            if attacker
                .blocking_restrictions
                .contains(&BlockingRestriction::AtLeastThreeBlockers)
            {
                minimum = 3;
            }
            if attacker
                .blocking_restrictions
                .contains(&BlockingRestriction::AllDefendingCreaturesMustBlock)
            {
                minimum = minimum.max(defending_creatures);
            }
            let max_one = attacker
                .blocking_restrictions
                .contains(&BlockingRestriction::AtMostOneBlocker);
            if max_one {
                at_most_one_blocker.insert(id);
            }
            if !attacker.keywords.is_disjoint(&defenders_lands) {
                continue;
            }
            let mut available = Vec::new();
            for blocker in &blockers {
                if !attacker
                    .controller
                    .is_some_and(|seat| barred_blocks.contains(&(blocker.object, seat)))
                    && can_block(&evaluated, attacker, blocker)?
                {
                    available.push(blocker.object);
                }
            }
            if minimum > 1 && (single_blocker || max_one || available.len() < minimum) {
                continue;
            }
            if minimum > 1 {
                minimums.insert(id, minimum);
            }
            if !available.is_empty()
                && attacker
                    .blocking_restrictions
                    .contains(&BlockingRestriction::MustBeBlocked)
            {
                require_at_least_one_block.insert(id);
            }
            for blocker in available {
                choices.entry(blocker).or_default().push(id);
            }
        }
        let additional = blockers.iter().filter_map(|blocker| {
            if blocker.blocking_restrictions.contains(&BlockingRestriction::CanBlockAnyNumber) {
                return (!attackers.is_empty()).then_some((blocker.object, attackers.len()));
            }
            let granted = blocker.blocking_restrictions.iter()
                .filter(|rule| **rule == BlockingRestriction::AdditionalBlock).count();
            let monarch = blocker.controller.is_some() && blocker.controller == self.characteristics.monarch
                && blocker.printed_text.continuous.iter().chain(&blocker.printed_text.additional_battlefield_abilities)
                    .any(|ability| matches!(ability, crate::characteristics::StaticContinuousAbility::Modifier { id, rule: crate::characteristics::StaticModifierRule::AdditionalBlockWhileMonarch }
                        if blocker.abilities.continuous == Some(*id) || blocker.abilities.additional_battlefield_abilities.contains(id)));
            let count = granted + usize::from(monarch);
            (count > 0).then_some((blocker.object, count))
        }).collect();
        Ok(
            BlockersDecision::new(self.revision, actor, attackers, choices, minimums)
                .chosen_by(combat.blocking_chooser.unwrap_or(actor))
                .with_single_blocker(single_blocker)
                .with_at_most_one_blocker(at_most_one_blocker)
                .with_additional_blockers(additional)
                .with_block_requirements(
                    &self.must_block_this_turn,
                    &self.must_be_blocked_this_turn,
                    require_at_least_one_block,
                ),
        )
    }
}

/// CR 702.14. Use the defending player's current land types and supertypes.
fn landwalk_abilities(
    world: &EvaluatedCharacteristics,
    defender: SeatId,
) -> BTreeSet<KeywordAbility> {
    use KeywordAbility::*;
    let mut evasion = BTreeSet::new();
    for land in world.objects.values().filter(|object| {
        object.zone == ObjectZone::Battlefield
            && object.controller == Some(defender)
            && object.has_type(CardType::Land)
    }) {
        if !land.current_supertypes.contains(&Supertype::Basic) {
            evasion.insert(NonbasicLandwalk);
        }
        if land.current_supertypes.contains(&Supertype::Legendary) {
            evasion.insert(LegendaryLandwalk);
        }
        if land.has_type(CardType::Artifact) {
            evasion.insert(ArtifactLandwalk);
        }
        if land.current_supertypes.contains(&Supertype::Snow) {
            evasion.insert(SnowLandwalk);
            if land.current_land_subtypes.contains(&LandType::Forest) {
                evasion.insert(SnowForestwalk);
            }
            if land.current_land_subtypes.contains(&LandType::Swamp) {
                evasion.insert(SnowSwampwalk);
            }
        }
        for (subtype, ability) in [
            (LandType::Plains, Plainswalk),
            (LandType::Island, Islandwalk),
            (LandType::Swamp, Swampwalk),
            (LandType::Mountain, Mountainwalk),
            (LandType::Forest, Forestwalk),
            (LandType::Desert, Desertwalk),
        ] {
            if land.current_land_subtypes.contains(&subtype) {
                evasion.insert(ability);
            }
        }
    }
    evasion
}

fn can_block(
    evaluated: &EvaluatedCharacteristics,
    attacker: &ObjectCharacteristics,
    blocker: &ObjectCharacteristics,
) -> Result<bool, CombatError> {
    use KeywordAbility::*;
    if blocker
        .blocking_restrictions
        .iter()
        .any(|restriction| match restriction {
            BlockingRestriction::CannotBlock | BlockingRestriction::CannotAttackOrBlock => true,
            BlockingRestriction::CanBlockOnlyFlying => !attacker.has_keyword(Flying),
            BlockingRestriction::CannotBlockSubtype(kind) => {
                attacker.current_creature_subtypes.contains(kind)
            }
            BlockingRestriction::CannotBlockMatching(filter) => {
                let excluded = BTreeSet::new();
                filter.matches_static_characteristics(attacker, &excluded, || {
                    Some(attacker.defined.mana_value.clone())
                })
            }
            _ => false,
        })
    {
        return Ok(false);
    }
    if attacker.granted_unblockable {
        return Ok(false);
    }
    for restriction in &attacker.blocking_restrictions {
        match restriction {
            BlockingRestriction::CannotBeBlockedByToughnessAtLeast(minimum)
                if blocker
                    .power_toughness
                    .as_ref()
                    .ok_or(CombatError::InvalidContext)?
                    .derived
                    .toughness
                    >= i64::from(*minimum).into() =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByPowerAtMost(maximum)
                if blocker
                    .power_toughness
                    .as_ref()
                    .ok_or(CombatError::InvalidContext)?
                    .derived
                    .power
                    <= i64::from(*maximum).into() =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByPowerAtLeast(minimum)
                if blocker
                    .power_toughness
                    .as_ref()
                    .ok_or(CombatError::InvalidContext)?
                    .derived
                    .power
                    >= i64::from(*minimum).into() =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByColor(color)
                if blocker.current_colors.contains(color) =>
            {
                return Ok(false);
            }
            BlockingRestriction::OnlyBlockedByColor(color)
                if !blocker.current_colors.contains(color) =>
            {
                return Ok(false);
            }
            BlockingRestriction::OnlyBlockedByColorOrArtifact(color)
                if !blocker.current_colors.contains(color)
                    && !blocker.has_type(CardType::Artifact) =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByTokens if blocker.is_token => {
                return Ok(false);
            }
            BlockingRestriction::OnlyBlockedByArtifact if !blocker.has_type(CardType::Artifact) => {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByNonArtifact
                if !blocker.has_type(CardType::Artifact) =>
            {
                return Ok(false);
            }
            BlockingRestriction::OnlyBlockedByFlyingOrReach
                if !blocker.has_keyword(Flying) && !blocker.has_keyword(Reach) =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedBySubtype(kind)
                if blocker.current_creature_subtypes.contains(kind) =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedByFlying if blocker.has_keyword(Flying) => {
                return Ok(false);
            }
            BlockingRestriction::BlockerPowerAtLeastPowerOf(source) => {
                if let Some(source_pt) = evaluated
                    .get(*source)
                    .and_then(|p| p.power_toughness.as_ref())
                    && blocker
                        .power_toughness
                        .as_ref()
                        .ok_or(CombatError::InvalidContext)?
                        .derived
                        .power
                        < source_pt.derived.power
                {
                    return Ok(false);
                }
            }
            BlockingRestriction::CannotBeBlocked => return Ok(false),
            BlockingRestriction::OnlyBlockedBySubtype(kind)
                if !blocker.current_creature_subtypes.contains(kind) =>
            {
                return Ok(false);
            }
            BlockingRestriction::CannotBeBlockedIfPowerOrToughnessAtMostOne => {
                let pt = attacker
                    .power_toughness
                    .as_ref()
                    .ok_or(CombatError::InvalidContext)?;
                if pt.derived.power <= 1.into() || pt.derived.toughness <= 1.into() {
                    return Ok(false);
                }
            }
            BlockingRestriction::CannotBeBlockedByMatching(filter) => {
                let excluded = BTreeSet::new();
                if filter.matches_static_characteristics(blocker, &excluded, || {
                    Some(blocker.defined.mana_value.clone())
                }) {
                    return Ok(false);
                }
            }
            BlockingRestriction::OnlyBlockedByMatching(filter) => {
                let excluded = BTreeSet::new();
                if !filter.matches_static_characteristics(blocker, &excluded, || {
                    Some(blocker.defined.mana_value.clone())
                }) {
                    return Ok(false);
                }
            }
            _ => {}
        }
    }
    if attacker.protected_from_colors(&blocker.current_colors)
        || attacker
            .protected_from_subtypes(&blocker.current_creature_subtypes, blocker.is_creature())
    {
        return Ok(false);
    }
    if attacker.has_keyword(Flying) && !(blocker.has_keyword(Flying) || blocker.has_keyword(Reach))
        || attacker.has_keyword(Shadow) != blocker.has_keyword(Shadow)
        || attacker.has_keyword(Horsemanship) && !blocker.has_keyword(Horsemanship)
    {
        return Ok(false);
    }
    if !blocker.has_type(CardType::Artifact)
        && (attacker.has_keyword(Fear) && !blocker.current_colors.contains(&Color::Black)
            || attacker.has_keyword(Intimidate)
                && attacker.current_colors.is_disjoint(&blocker.current_colors))
    {
        return Ok(false);
    }
    if attacker.has_keyword(Skulk) {
        let blocker_pt = blocker
            .power_toughness
            .as_ref()
            .ok_or(CombatError::InvalidContext)?;
        let attacker_pt = attacker
            .power_toughness
            .as_ref()
            .ok_or(CombatError::InvalidContext)?;
        return Ok(blocker_pt.derived.power <= attacker_pt.derived.power);
    }
    Ok(true)
}
