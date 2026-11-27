//! Chosen creatures pay crew and saddle together before either ability resolves.
use super::*;
use crate::{
    activated::{PowerAbility, SourceCost},
    characteristics::{
        ResolvedDuration, ResolvedPart, ResolvedParts, StaticContinuousAbility, StaticModifierRule,
        SubtypeOperation, TypeChange, TypeOperation, selector_matches_permanent,
    },
    occurrence::{EventCause, RulesOccurrence},
};
impl EffectWindow {
    fn crew_or_saddle_power_bonus(
        &self,
        kind: PowerAbility,
        contributor: ObjectId,
        current: &EvaluatedCharacteristics,
    ) -> RulesInteger {
        if kind == PowerAbility::Station {
            return 0.into();
        }
        let applies_to_kind = |crews: bool, saddles: bool| match kind {
            PowerAbility::Crew => crews,
            PowerAbility::Saddle => saddles,
            PowerAbility::Station => false,
        };
        let mut total_bonus = RulesInteger::from(0);
        for perm in current.objects.values() {
            if perm.zone != ObjectZone::Battlefield {
                continue;
            }
            let Some(perm_ctrl) = perm.controller else {
                continue;
            };
            for rule in perm.active_battlefield_rules() {
                if let StaticContinuousAbility::Modifier {
                    rule:
                        StaticModifierRule::CrewAndSaddlePowerBonus {
                            selector,
                            crews,
                            saddles,
                            bonus,
                        },
                    ..
                } = rule
                    && applies_to_kind(*crews, *saddles)
                    && selector_matches_permanent(
                        self.window.cards(),
                        current,
                        perm.object,
                        perm_ctrl,
                        selector,
                        contributor,
                        &self.name_rules,
                    )
                {
                    total_bonus = total_bonus.plus(&RulesInteger::from(i64::from(*bonus)));
                }
            }
        }
        for active in &self.temporary_rules {
            if let Some(ab) = self.active_temporary_static(active, current)
                && let StaticContinuousAbility::Modifier {
                    rule:
                        StaticModifierRule::CrewAndSaddlePowerBonus {
                            selector,
                            crews,
                            saddles,
                            bonus,
                        },
                    ..
                } = ab
                && applies_to_kind(*crews, *saddles)
                && selector_matches_permanent(
                    self.window.cards(),
                    current,
                    active.source_object.unwrap_or(ObjectId::SENTINEL),
                    active.controller,
                    selector,
                    contributor,
                    &self.name_rules,
                )
            {
                total_bonus = total_bonus.plus(&RulesInteger::from(i64::from(*bonus)));
            }
        }
        for (owner, rule) in &current.emblem_rules {
            if let StaticContinuousAbility::Modifier {
                rule:
                    StaticModifierRule::CrewAndSaddlePowerBonus {
                        selector,
                        crews,
                        saddles,
                        bonus,
                    },
                ..
            } = rule
                && applies_to_kind(*crews, *saddles)
                && selector_matches_permanent(
                    self.window.cards(),
                    current,
                    ObjectId::SENTINEL,
                    *owner,
                    selector,
                    contributor,
                    &self.name_rules,
                )
            {
                total_bonus = total_bonus.plus(&RulesInteger::from(i64::from(*bonus)));
            }
        }
        total_bonus
    }

    pub(super) fn validate_power_contributors(
        &self,
        cost: SourceCost,
        source: ObjectId,
        actor: SeatId,
        contributors: &[ObjectId],
        current: &EvaluatedCharacteristics,
        turn: Option<&TurnScheduler>,
    ) -> Result<(), EffectError> {
        let (kind, minimum) = cost.power_cost().ok_or(EffectError::NotAbilitySource)?;
        if kind == PowerAbility::Station && contributors.len() != 1 {
            return Err(EffectError::InvalidPowerContributors);
        }
        if matches!(kind, PowerAbility::Saddle | PowerAbility::Station) {
            validate_main_phase(&self.window, turn, actor)?;
        }
        let mut seen = BTreeSet::new();
        let mut power = RulesInteger::from(0);
        for &member in contributors {
            let view = current
                .get(member)
                .ok_or(EffectError::InvalidPowerContributors)?;
            if member == source
                || !seen.insert(member)
                || view.zone != ObjectZone::Battlefield
                || view.controller != Some(actor)
                || !view.is_creature()
                || !self
                    .window
                    .cards()
                    .get(member)
                    .is_some_and(|object| object.tapped() == Some(false))
            {
                return Err(EffectError::InvalidPowerContributors);
            }
            let base_power = &view
                .power_toughness
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?
                .derived
                .power;
            let bonus = self.crew_or_saddle_power_bonus(kind, member, current);
            power = power.plus(&base_power.plus(&bonus));
        }
        let required = RulesInteger::try_from(minimum.to_string())
            .map_err(|_| EffectError::InconsistentProgram)?;
        if kind != PowerAbility::Station && power < required {
            return Err(EffectError::InvalidPowerContributors);
        }
        Ok(())
    }

    pub(super) fn resolve_power_ability(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        kind: PowerAbility,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let StackKind::ActivatedAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let source = source.id();
        if !self
            .window
            .cards()
            .get(source)
            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
        {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let mut charge_placed = None;
        let effect = match kind {
            PowerAbility::Station => {
                let [contributor] = entry.power_contributors() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let power = self
                    .growth_stats(*contributor, &self.evaluated_characteristics()?)
                    .map_or_else(|| 0.into(), |pair| pair.power.max(0.into()));
                let added = power.times(&super::permanent_counters::effect_counter_multiplier(
                    &before.view().evaluated,
                    source,
                ));
                if added.is_positive() {
                    let counters = self
                        .characteristics
                        .charge
                        .entry(source)
                        .or_insert_with(|| 0.into());
                    *counters = counters.plus(&added);
                }
                charge_placed = Some(added);
                None
            }
            PowerAbility::Crew => Some(self.characteristics.add_effect(
                entry.id(),
                source,
                ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                    types: TypeOperation::Add(BTreeSet::from([
                        CardType::Artifact,
                        CardType::Creature,
                    ])),
                    supertypes: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Keep,
                    land_subtypes: SubtypeOperation::Keep,
                })]),
                ResolvedDuration::UntilEndOfTurn,
            )?),
            PowerAbility::Saddle => {
                self.characteristics.saddled.insert(source);
                None
            }
        };
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |previous, current, _| {
                let mut facts = Vec::new();
                if let Some(effect) = effect {
                    let appearances = occurrence_flow::characteristic_appearance_changes(
                        previous,
                        current,
                        [source],
                    )?;
                    facts.push(RulesOccurrence::ContinuousEffectCreated {
                        effect,
                        appearances,
                    });
                }
                facts.push(RulesOccurrence::PowerAbilityResolved {
                    source,
                    kind,
                    contributors: entry.power_contributors().to_vec(),
                    charge_placed,
                });
                Ok(facts)
            },
        )
    }
}
