//! The city's blessing is a permanent player designation under CR 702.131.
use super::*;
use crate::characteristics::{EvaluatedCharacteristics, ObjectCharacteristics};

impl EffectWindow {
    /// Whether this player has earned the city's blessing in this game.
    pub fn has_city_blessing(&self, player: SeatId) -> bool {
        self.characteristics.city_blessing.contains(&player)
    }

    pub(super) fn update_city_blessing(
        &mut self,
        current: &EvaluatedCharacteristics,
    ) -> Vec<SeatId> {
        let mut qualified = Vec::new();
        for &player in self.window.participants() {
            if self.has_city_blessing(player) {
                continue;
            }
            let controlled = || {
                current.objects.values().filter(|permanent| {
                    permanent.zone == ObjectZone::Battlefield
                        && permanent.controller == Some(player)
                })
            };
            if controlled().any(|permanent| {
                permanent.has_battlefield_rule(|ability| {
                    matches!(
                        ability,
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule: crate::characteristics::StaticModifierRule::Ascend,
                            ..
                        }
                    )
                })
            }) && controlled().take(10).count() == 10
            {
                self.characteristics.city_blessing.insert(player);
                qualified.push(player);
            }
        }
        qualified
    }

    pub(super) fn blessing_allows_combat(
        &self,
        current: &ObjectCharacteristics,
        controller: SeatId,
    ) -> bool {
        self.has_city_blessing(controller)
            || !current.has_battlefield_rule(|ability| {
                matches!(
                    ability,
                    crate::characteristics::StaticContinuousAbility::Restriction {
                        rule: crate::characteristics::StaticRestriction::CombatRequiresCityBlessing,
                        ..
                    }
                )
            })
    }
}
