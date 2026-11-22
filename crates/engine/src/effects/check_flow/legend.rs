//! Frozen common-name cohorts and the final unique simultaneous movement union.
use super::*;
use crate::program::Supertype;
use spellwake_contracts::names::EnglishCardName;

impl EffectWindow {
    pub(super) fn collect_legend_conflicts(
        &self,
        current: &EvaluatedCharacteristics,
        event: CheckEventId,
    ) -> Result<Vec<StagedLegend>, EffectError> {
        let mut conflicts = Vec::new();
        for controller in self.window.apnap_players()? {
            let mut by_name: BTreeMap<EnglishCardName, BTreeSet<ObjectId>> = BTreeMap::new();
            let exempt_subtypes=current.objects.values().filter(|o|o.zone==ObjectZone::Battlefield && o.controller==Some(controller)).flat_map(|source|
                source.printed_text.continuous.iter().chain(&source.printed_text.additional_battlefield_abilities).filter_map(move |rule| {
                    let crate::characteristics::StaticContinuousAbility::Modifier { id, rule: crate::characteristics::StaticModifierRule::IgnoreLegendForSubtype(subtype) }=rule else {return None;};
                    (source.abilities.continuous==Some(*id) || source.abilities.additional_battlefield_abilities.contains(id)).then_some(*subtype)
                })).collect::<BTreeSet<_>>();
            for (id, traits) in &current.objects {
                if traits.zone != ObjectZone::Battlefield
                    || traits.controller != Some(controller)
                    || !exempt_subtypes.is_disjoint(&traits.current_creature_subtypes)
                    || !traits.current_supertypes.contains(&Supertype::Legendary)
                {
                    continue;
                }
                for name in self.name_rules.comparison_forms(&traits.current_names) {
                    by_name.entry(name.clone()).or_default().insert(*id);
                }
            }
            // A name witnesses a common-name object cohort. Identical witnesses
            // are one action; strict subsets are covered by the maximal cohort.
            let mut cohorts: BTreeMap<BTreeSet<ObjectId>, BTreeSet<EnglishCardName>> =
                BTreeMap::new();
            for (name, members) in by_name {
                if members.len() >= 2 {
                    cohorts.entry(members).or_default().insert(name);
                }
            }
            let maximal: Vec<_> = cohorts
                .iter()
                .filter(|(members, _)| {
                    !cohorts
                        .keys()
                        .any(|other| members.len() < other.len() && members.is_subset(other))
                })
                .collect();
            for (ordinal, (members, names)) in maximal.into_iter().enumerate() {
                let ordinal =
                    u64::try_from(ordinal).map_err(|_| CheckError::ConflictCountExhausted)?;
                conflicts.push(StagedLegend {
                    conflict: LegendConflict {
                        id: LegendConflictId {
                            event,
                            controller,
                            ordinal,
                        },
                        comparison_names: names.clone(),
                        domain: members.iter().copied().collect(),
                    },
                    keep: None,
                });
            }
        }
        Ok(conflicts)
    }

    pub(super) fn state_dead_union(
        &self,
        actions: &StateActions,
    ) -> Result<BTreeMap<ObjectId, StateBasedMoveReasons>, EffectError> {
        let mut dead = actions.dead.clone();
        for staged in &actions.legends {
            let keep = staged.keep.ok_or(EffectError::WrongPhase)?;
            if !staged.conflict.domain.contains(&keep) {
                return Err(CheckError::WrongFrame.into());
            }
            for object in staged
                .conflict
                .domain
                .iter()
                .copied()
                .filter(|o| *o != keep)
            {
                dead.entry(object)
                    .and_modify(|causes| causes.insert(StateBasedMoveReason::Legend))
                    .or_insert_with(|| StateBasedMoveReasons::new(StateBasedMoveReason::Legend));
            }
        }
        for id in dead.keys() {
            if !self
                .window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
            {
                return Err(EffectError::InconsistentProgram);
            }
        }
        Ok(dead)
    }
    pub(super) fn determine_state_movements(
        &self,
        actions: &StateActions,
    ) -> Result<StateMovements, EffectError> {
        let dead = self.state_dead_union(actions)?;
        let process = self.check_process()?;
        let intents = dead
            .iter()
            .map(|(before, reasons)| ZoneMoveIntent {
                before: *before,
                reason: ZoneReason::StateBased(*reasons),
            })
            .collect();
        let moves = self.new_check_moves(
            intents,
            EventCause::StateBasedActions {
                event: process.event,
                boundary: process.boundary,
            },
            crate::program::ZoneActionKind::StateBased,
        )?;
        Ok(StateMovements { dead, moves })
    }
}
