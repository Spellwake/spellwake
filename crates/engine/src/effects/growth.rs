//! Capture-based comparisons for mentor and evolve and the printed suffix of backup.
use super::{resolution_choices::CounterPlacement, *};
use crate::{
    characteristics::{
        AbilityChange, PowerToughness, ResolvedDuration, ResolvedPart, ResolvedParts,
    },
    occurrence::{
        EventCause, RulesOccurrence, SupportedAppearance, TriggerCauseSet, TriggerRelated,
    },
    program::GrowthKeyword,
    stack_window::StackEntry,
};

fn entrant(causes: &TriggerCauseSet) -> Option<ObjectId> {
    let [cause] = causes.members() else {
        return None;
    };
    match &cause.related {
        TriggerRelated::TokenCreated { after, .. } | TriggerRelated::ZoneChange { after, .. } => {
            Some(after.object.id())
        }
        _ => None,
    }
}
fn referenced(
    body: &ExecutableBody,
    source: ObjectId,
    causes: &TriggerCauseSet,
) -> Option<ObjectId> {
    if body.instructions.iter().any(|instruction| matches!(instruction,
        Instruction::TapEnchantedCreature | Instruction::LookTopCreatureSharingAttachedSubtype { .. }
    ) || matches!(instruction,
        Instruction::SearchLibrary { filter }
            if filter.curse_host() == Some(crate::program::AuraSearchHost::EnchantedPlayerWithDifferentCurseNames)
    )) {
        return Some(source);
    }
    match body.instructions.as_slice() {
        [Instruction::ResolveGrowth(GrowthKeyword::Mentor | GrowthKeyword::Backup { .. })] => {
            Some(source)
        }
        [Instruction::ResolveGrowth(GrowthKeyword::Evolve)] => entrant(causes),
        _ => None,
    }
}
impl EffectWindow {
    /// Departed objects still referenced by pending mentor, evolve, backup or station abilities.
    pub fn growth_last_known(&self) -> &BTreeMap<ObjectId, SupportedAppearance> {
        &self.growth_last_known
    }
    pub(super) fn update_growth_last_known(&mut self, changes: &[RulesOccurrence]) {
        let departures = changes
            .iter()
            .filter_map(RulesOccurrence::departing_permanent);
        if self.growth_last_known.is_empty() && departures.clone().next().is_none() {
            return;
        }
        let waiting = self
            .triggers
            .waiting()
            .values()
            .filter_map(|w| referenced(&w.body, w.source.object.id(), &w.causes));
        let stacked = self.payloads.values().filter_map(|p| match p {
            StackPayload::Instructions(p) => {
                let b = p.bindings.trigger.as_ref()?;
                referenced(&p.body, b.source.object.id(), &b.causes)
            }
            _ => None,
        });
        let station = self
            .window
            .entries()
            .iter()
            .filter(|entry| {
                self.payloads
                    .get(&entry.id())
                    .and_then(StackPayload::body)
                    .is_some_and(|body| {
                        matches!(
                            body.instructions.as_slice(),
                            [Instruction::ResolvePowerAbility(
                                crate::activated::PowerAbility::Station
                            )]
                        )
                    })
            })
            .flat_map(|entry| entry.power_contributors().iter().copied());
        let source_names = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .chain(self.window.checking_targets())
            .filter(|entry| {
                self.stack_body(entry.id())
                    .is_some_and(|body| body.target == same_name_ability_target())
            })
            .filter_map(|entry| match entry.kind() {
                StackKind::ActivatedAbility { source } => Some(source.id()),
                _ => None,
            });
        let first_targets = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .chain(self.window.checking_targets())
            .filter(|entry| {
                self.stack_body(entry.id()).is_some_and(|body| {
                    body.target == BodyTarget::OpponentOrPlaneswalkerThenCreature
                })
            })
            .filter_map(
                |entry| match entry.targets().targets().first()?.choice().target {
                    TargetRef::Object(id) => Some(id),
                    _ => None,
                },
            );
        let mut needed: BTreeSet<_> = waiting
            .chain(stacked)
            .chain(station)
            .chain(source_names)
            .chain(first_targets)
            .collect();
        let old_hosts = needed
            .iter()
            .filter_map(|source| {
                let host = departures
                    .clone()
                    .find(|appearance| appearance.object.id() == *source)
                    .and_then(|appearance| appearance.attached_to)
                    .or_else(|| self.attachment_host_for_ability(*source));
                match host {
                    Some(TargetRef::Object(id)) => Some(id),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        needed.extend(old_hosts);
        self.growth_last_known.retain(|id, _| needed.contains(id));
        for before in departures.filter(|a| needed.contains(&a.object.id())) {
            self.growth_last_known
                .insert(before.object.id(), before.clone());
        }
    }
    pub(super) fn growth_stats(
        &self,
        id: ObjectId,
        world: &EvaluatedCharacteristics,
    ) -> Option<PowerToughness> {
        if let Some(current) = world.get(id) {
            return (current.zone == ObjectZone::Battlefield && current.is_creature())
                .then(|| current.power_toughness.as_ref().map(|s| s.derived.clone()))
                .flatten();
        }
        self.growth_last_known
            .get(&id)
            .filter(|a| a.is_creature())
            .and_then(|a| a.current_power_toughness.clone())
    }
    pub(super) fn mentor_target(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
        source: Option<&SupportedAppearance>,
        world: &EvaluatedCharacteristics,
    ) -> bool {
        if matches!(
            body.instructions.as_slice(),
            [Instruction::AffectPermanentTargets {
                action: crate::program::PermanentTargetAction::PutPlusOneCounterOnOthers,
                ..
            }]
        ) {
            return source.is_some_and(|source| target != TargetRef::Object(source.object.id()));
        }
        if !matches!(
            body.instructions.as_slice(),
            [Instruction::ResolveGrowth(GrowthKeyword::Mentor)]
        ) {
            return true;
        }
        let (Some(source), TargetRef::Object(target)) = (source, target) else {
            return false;
        };
        if self
            .combat
            .as_ref()
            .and_then(|combat| combat.attacker(target))
            .is_none()
        {
            return false;
        }
        self.growth_stats(source.object.id(), world)
            .zip(self.growth_stats(target, world))
            .is_some_and(|(s, t)| t.power < s.power)
    }
    /// CR 702.165d fixes the granted abilities at stack placement.
    pub(super) fn capture_backup(&self, waiting: &mut WaitingInstance) -> Result<(), EffectError> {
        let [Instruction::ResolveGrowth(GrowthKeyword::Backup { printed_below, .. })] =
            waiting.body.instructions.as_mut_slice()
        else {
            return Ok(());
        };
        let id = waiting.source.object.id();
        let current = self.current_characteristics(id)?;
        let (text, keywords, roster) = match &current {
            Some(actual) => (&actual.printed_text, &actual.keywords, &actual.abilities),
            None => {
                let departure = self.growth_last_known.get(&id).unwrap_or(&waiting.source);
                (
                    &departure.printed_text,
                    &departure.keywords,
                    &departure.abilities,
                )
            }
        };
        // The supported layer-six removals erase all printed nonmana abilities at once.
        // A later grant of the same static keyword is not a surviving printed ability.
        if roster.triggered.contains(&waiting.definition)
            && text.triggered.iter().any(|a| {
                a.id == waiting.definition
                    && matches!(
                        a.body.instructions.as_slice(),
                        [Instruction::ResolveGrowth(GrowthKeyword::Backup { .. })]
                    )
            })
        {
            printed_below.retain(|ability| {
                text.printed_keywords.contains(ability) && keywords.contains(ability)
            });
        } else {
            printed_below.clear();
        }
        Ok(())
    }
    pub(super) fn resolve_growth(
        &mut self,
        keyword: &GrowthKeyword,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let source = binding.source.object.id();
        if matches!(keyword, GrowthKeyword::Evolve | GrowthKeyword::Increment) {
            let world = self.evaluated_characteristics()?;
            let Some(current) = world
                .get(source)
                .filter(|s| s.zone == ObjectZone::Battlefield && s.is_creature())
                .and_then(|s| s.power_toughness.as_ref())
            else {
                return Ok(());
            };
            let placement = if matches!(keyword, GrowthKeyword::Increment) {
                let [cause] = binding.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let TriggerRelated::SpellCast(cast) = &cause.related else {
                    return Err(EffectError::InconsistentProgram);
                };
                let paid = cast
                    .spent_for_increment()
                    .ok_or(EffectError::InconsistentProgram)?;
                if paid <= &current.derived.power && paid <= &current.derived.toughness {
                    return Ok(());
                }
                Some(CounterPlacement::Ordinary)
            } else {
                let entering = entrant(&binding.causes).ok_or(EffectError::InconsistentProgram)?;
                self.growth_stats(entering, &world)
                    .filter(|s| {
                        s.power > current.derived.power || s.toughness > current.derived.toughness
                    })
                    .map(|_| CounterPlacement::Evolve)
            };
            if let Some(placement) = placement {
                self.add_source_counters(source, entry.id(), index, 1.into(), placement, append)?;
            }
            return Ok(());
        }
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TargetRef::Object(target) = bound.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        match keyword {
            GrowthKeyword::Mentor => self.add_source_counters(
                target,
                entry.id(),
                index,
                1.into(),
                CounterPlacement::Mentor(source),
                append,
            ),
            GrowthKeyword::Backup {
                amount,
                printed_below,
            } => {
                if amount.is_positive() {
                    self.add_source_counters(
                        target,
                        entry.id(),
                        index,
                        amount.clone(),
                        CounterPlacement::Ordinary,
                        append,
                    )?;
                }
                if target == source || printed_below.is_empty() {
                    return Ok(());
                }
                let snapshot = self.rules_snapshot(None)?;
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    target,
                    ResolvedParts(vec![ResolvedPart::Abilities(
                        AbilityChange::grant_keywords(printed_below.iter().copied()),
                    )]),
                    ResolvedDuration::UntilEndOfTurn,
                )?;
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &snapshot,
                    None,
                    append,
                    |old, new, _| {
                        Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                            appearances: characteristic_appearance_changes(old, new, [target])?,
                            effect,
                        }])
                    },
                )
            }
            _ => Err(EffectError::InconsistentProgram),
        }
    }
}
