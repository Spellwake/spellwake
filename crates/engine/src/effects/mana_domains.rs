//! Finite least-fixed-point evaluation of the mana types permanents could produce.
use super::*;

impl EffectWindow {
    pub(super) fn opponents_land_colors(
        &self,
        actor: SeatId,
    ) -> Result<BTreeSet<ManaType>, EffectError> {
        let world = self.evaluated_characteristics()?;
        let lands = world
            .objects
            .values()
            .filter(|object| {
                object.zone == ObjectZone::Battlefield && object.has_type(CardType::Land)
            })
            .collect::<Vec<_>>();
        let mut domains: BTreeMap<ObjectId, BTreeSet<ManaType>> = lands
            .iter()
            .map(|land| (land.object, BTreeSet::new()))
            .collect();
        loop {
            let previous = domains.clone();
            for land in &lands {
                let Some(controller) = land.controller else {
                    continue;
                };
                let mut colors: BTreeSet<_> = land
                    .intrinsic_mana
                    .iter()
                    .map(|kind| kind.mana_type())
                    .collect();
                if !land.granted_any_color_mana.is_empty() {
                    colors.extend(colored_mana());
                }
                colors.extend(land.granted_fixed_mana.values().copied());
                for ability in &land.printed_text.mana {
                    if land.abilities.mana.contains(&ability.id) {
                        colors.extend(self.possible_output(
                            &ability.output,
                            land.object,
                            controller,
                            &world,
                            &previous,
                        )?);
                    }
                }
                for output in land.inherited_mana.values() {
                    colors.extend(self.possible_output(
                        output,
                        land.object,
                        controller,
                        &world,
                        &previous,
                    )?);
                }
                domains.entry(land.object).or_default().extend(colors);
            }
            if domains == previous {
                break;
            }
        }
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), actor)?;
        Ok(lands
            .into_iter()
            .filter(|land| {
                land.controller
                    .is_some_and(|seat| opponents.contains(&seat))
            })
            .flat_map(|land| domains.get(&land.object).into_iter().flatten().copied())
            .filter(|color| *color != ManaType::Colourless)
            .collect())
    }

    fn possible_output(
        &self,
        output: &ManaOutput,
        source: ObjectId,
        controller: SeatId,
        world: &EvaluatedCharacteristics,
        domains: &BTreeMap<ObjectId, BTreeSet<ManaType>>,
    ) -> Result<BTreeSet<ManaType>, EffectError> {
        use crate::mana::{
            CounterManaYield, DynamicManaYield, SpecialColorYield, TriggeredManaRule,
        };
        let colors = match output {
            ManaOutput::Fixed(produced) => produced.keys().copied().collect(),
            ManaOutput::ChooseColor { colors, .. } => colors.clone(),
            // Activation restrictions, costs, restrictions, and side effects do not limit what an ability could produce.
            ManaOutput::Modified { output, .. } => {
                self.possible_output(output, source, controller, world, domains)?
            }
            ManaOutput::Triggered(rule) => match rule {
                TriggeredManaRule::PermanentTappedForColorless => {
                    BTreeSet::from([ManaType::Colourless])
                }
                TriggeredManaRule::ImprintedLandTappedSameType
                | TriggeredManaRule::LandTappedSameType
                | TriggeredManaRule::LandTappedWhileMonarch => colored_mana(),
            },
            ManaOutput::TwoColors { .. } => colored_mana(),
            ManaOutput::CounterBased(kind) => match kind {
                CounterManaYield::RemoveChargeForColor => colored_mana(),
                CounterManaYield::RemoveStorageCounters => BTreeSet::from([ManaType::Colourless]),
                CounterManaYield::SourceChargeCounters => self
                    .characteristics
                    .charge
                    .get(&source)
                    .filter(|n| n.is_positive())
                    .map(|_| ManaType::Colourless)
                    .into_iter()
                    .collect(),
            },
            ManaOutput::Counted {
                mana_type,
                permanents,
            } => {
                if crate::characteristics::count_permanents(
                    self.window.cards(),
                    world,
                    source,
                    controller,
                    permanents,
                    &self.name_rules,
                )
                .is_positive()
                {
                    match mana_type {
                        Some(kind) => BTreeSet::from([*kind]),
                        None => colored_mana(),
                    }
                } else {
                    BTreeSet::new()
                }
            }
            ManaOutput::SpecialColor(kind) => match kind {
                SpecialColorYield::ChosenPlayerColor => colored_mana(),
                SpecialColorYield::HybridFilter { colors } => colors.iter().copied().collect(),
                SpecialColorYield::CommanderColorIdentity => {
                    self.commander_mana_colors(controller)?
                }
                SpecialColorYield::EntryChosenColor { fixed } => std::iter::once(*fixed)
                    .chain(self.entry_color_choices.get(&source).copied())
                    .collect(),
                SpecialColorYield::OpponentsLandColors => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    world
                        .objects
                        .values()
                        .filter(|land| {
                            land.zone == ObjectZone::Battlefield
                                && land.has_type(CardType::Land)
                                && land
                                    .controller
                                    .is_some_and(|seat| opponents.contains(&seat))
                        })
                        .flat_map(|land| domains.get(&land.object).into_iter().flatten().copied())
                        .filter(|color| *color != ManaType::Colourless)
                        .collect()
                }
            },
            ManaOutput::Dynamic(kind) => match kind {
                DynamicManaYield::GreatestCreaturePower => {
                    if self
                        .battlefield_cost_value(
                            crate::program::CastCostValue::GreatestCreaturePower,
                            controller,
                            world,
                        )?
                        .is_positive()
                    {
                        colored_mana()
                    } else {
                        BTreeSet::new()
                    }
                }
                DynamicManaYield::ControlledCreaturePowerColor { .. } => colored_mana(),
                DynamicManaYield::ConditionalControlledLandTypes {
                    requirements,
                    normal,
                    fulfilled,
                } => {
                    let qualified = requirements.iter().all(|required| {
                        world.objects.values().any(|object| {
                            object.zone == ObjectZone::Battlefield
                                && object.controller == Some(controller)
                                && object.has_type(CardType::Land)
                                && required.is_subset(&object.current_land_subtypes)
                        })
                    });
                    if qualified { fulfilled } else { normal }
                        .keys()
                        .copied()
                        .collect()
                }
            },
        };
        Ok(colors)
    }
}

pub(super) fn colored_mana() -> BTreeSet<ManaType> {
    ManaType::ALL
        .into_iter()
        .filter(|color| *color != ManaType::Colourless)
        .collect()
}
