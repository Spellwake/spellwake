//! Ordinary attacking bands and their combat relationships (CR 702.22).
use super::*;
use crate::characteristics::{KeywordAbility, ObjectCharacteristics};
/// The two qualities printed on cards in the dated tournament catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BandQuality {
    LegendaryCreatures,
    CreaturesNamedWolvesOfTheHunt,
}
impl BandQuality {
    pub const ALL: [Self; 2] = [
        Self::LegendaryCreatures,
        Self::CreaturesNamedWolvesOfTheHunt,
    ];
    pub(crate) fn matches(
        self,
        object: &crate::characteristics::ObjectCharacteristics,
        names: &spellwake_contracts::names::NameRules,
    ) -> bool {
        if !object.is_creature() {
            return false;
        }
        match self {
            Self::LegendaryCreatures => object
                .current_supertypes
                .contains(&crate::program::Supertype::Legendary),
            Self::CreaturesNamedWolvesOfTheHunt => {
                static NAME: std::sync::OnceLock<spellwake_contracts::names::EnglishCardName> =
                    std::sync::OnceLock::new();
                let name = NAME.get_or_init(|| {
                    spellwake_contracts::names::EnglishCardName::new(
                        "Wolves of the Hunt".to_owned(),
                    )
                    .expect("printed English name")
                });
                names.has_name(&object.current_names, name)
            }
        }
    }
}

/// An attacking or blocking group changes damage assignment only while a qualifying
/// member currently has the keyword. No attacking band need have been declared.
pub(crate) fn controls_opposing_assignment<'a>(
    others: impl Iterator<Item = &'a ObjectCharacteristics>,
    names: &spellwake_contracts::names::NameRules,
) -> bool {
    let mut counts = [(0_usize, false); 2];
    for card in others {
        if card.has_keyword(KeywordAbility::Banding) {
            return true;
        }
        for (i, quality) in BandQuality::ALL.into_iter().enumerate() {
            if quality.matches(card, names) {
                counts[i].0 += 1;
                counts[i].1 |= card.has_keyword(KeywordAbility::BandsWithOther(quality));
                if counts[i].0 > 1 && counts[i].1 {
                    return true;
                }
            }
        }
    }
    false
}

impl AttackersDecision {
    /// Printed band qualities this attacker currently satisfies.
    pub fn band_qualities(&self, attacker: ObjectId) -> impl Iterator<Item = BandQuality> + '_ {
        self.band_qualities
            .iter()
            .filter(move |(_, (members, _))| members.contains(&attacker))
            .map(|(&quality, _)| quality)
    }
    /// Qualified "bands with other" abilities of this attacker.
    pub fn bands_with_other(&self, attacker: ObjectId) -> impl Iterator<Item = BandQuality> + '_ {
        self.band_qualities
            .iter()
            .filter(move |(_, (_, leaders))| leaders.contains(&attacker))
            .map(|(&quality, _)| quality)
    }
    pub(crate) fn observe_banding(
        &mut self,
        current: &crate::characteristics::EvaluatedCharacteristics,
        names: &spellwake_contracts::names::NameRules,
    ) {
        for &id in &self.attackers {
            let Some(view) = current.get(id) else {
                continue;
            };
            if view.has_keyword(KeywordAbility::Banding) {
                self.banding_attackers.insert(id);
            }
            for quality in BandQuality::ALL {
                if quality.matches(view, names) {
                    let (members, leaders) = self.band_qualities.entry(quality).or_default();
                    members.insert(id);
                    if view.has_keyword(KeywordAbility::BandsWithOther(quality)) {
                        leaders.insert(id);
                    }
                }
            }
        }
        self.band_qualities
            .retain(|_, (_, leaders)| !leaders.is_empty());
    }

    /// Attackers that currently have ordinary banding when this declaration is offered.
    pub fn banding_attackers(&self) -> &BTreeSet<ObjectId> {
        &self.banding_attackers
    }

    pub(crate) fn validate_bands(
        &self,
        attacks: &[AttackDeclaration],
        bands: &[Vec<ObjectId>],
    ) -> Result<(), CombatError> {
        let declared: BTreeMap<_, _> = attacks
            .iter()
            .map(|a| (a.attacker, (a.defender, a.defending_planeswalker)))
            .collect();
        let mut used = BTreeSet::new();
        for band in bands {
            let Some(defender) = band.first().and_then(|id| declared.get(id)) else {
                return Err(CombatError::IllegalAttacker);
            };
            let mut with_banding = 0;
            for member in band {
                if !used.insert(member) || declared.get(member) != Some(defender) {
                    return Err(CombatError::IllegalAttacker);
                }
                with_banding += usize::from(self.banding_attackers.contains(member));
            }
            if (with_banding == 0 || band.len() - with_banding > 1)
                && !self.band_qualities.values().any(|(members, leaders)| {
                    band.iter().all(|id| members.contains(id))
                        && band.iter().any(|id| leaders.contains(id))
                })
            {
                return Err(CombatError::IllegalAttacker);
            }
        }
        Ok(())
    }
}

impl CombatState {
    /// Exact surviving members of the original declared bands, in declaration order.
    pub fn attacking_bands(&self) -> &[BTreeSet<ObjectId>] {
        &self.attacking_bands
    }

    pub(crate) fn set_attacking_bands(&mut self, bands: Vec<Vec<ObjectId>>) {
        self.attacking_bands = bands
            .into_iter()
            .map(|band| {
                band.into_iter()
                    .filter(|member| self.attackers.contains_key(member))
                    .collect::<BTreeSet<_>>()
            })
            .filter(|band| !band.is_empty())
            .collect();
    }

    /// Restrictions such as flying apply to the creature chosen to block. Other members
    /// of its band become blocked by the same creature as a consequence (CR 702.22h).
    pub(crate) fn expand_band_blocks(
        &self,
        chosen: Vec<(Combatant, ObjectId)>,
    ) -> Vec<(Combatant, ObjectId)> {
        if self.attacking_bands.is_empty() {
            return chosen;
        }
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for (blocker, attacker) in chosen {
            let mut push = |member| {
                if seen.insert((blocker.object(), member)) {
                    result.push((blocker, member));
                }
            };
            push(attacker);
            if let Some(band) = self
                .attacking_bands
                .iter()
                .find(|band| band.contains(&attacker))
            {
                for member in band {
                    push(*member);
                }
            }
        }
        result
    }
}
