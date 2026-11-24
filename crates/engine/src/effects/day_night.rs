//! CR 502.2, 702.145, and 731. Transform all eligible permanents in one cut.
use super::*;
use crate::{
    day_night::DayNight,
    occurrence::{RulesOccurrence, RulesView},
};

impl EffectWindow {
    fn transform_prohibited(
        evaluated: &EvaluatedCharacteristics,
        target: &ObjectCharacteristics,
    ) -> bool {
        target.controller.is_some_and(|controller| {
            evaluated.controlled_rules(controller, |rule| {
                matches!(rule, crate::characteristics::StaticContinuousAbility::Restriction { rule: crate::characteristics::StaticRestriction::PreventControlledTransform { subtype, except }, .. }
                    if target.current_creature_subtypes.contains(subtype)
                        && !target.current_creature_subtypes.contains(except))
            }) != 0
        })
    }

    pub(super) fn available_transform_source(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<Option<ObjectId>, EffectError> {
        let source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                source
            }
            _ => return Err(EffectError::InconsistentProgram),
        };
        let id = source.id();
        let Some(current) = self.window.cards().get(id) else {
            return Ok(None);
        };
        let Some(front) = current.card_id().and_then(|card| self.programs.get(&card)) else {
            return Ok(None);
        };
        let can_flip = front.flip_back.is_some() && !current.transformed();
        if current.zone() != ObjectZone::Battlefield
            || current.face_down().is_some()
            || current.last_transformed_at() != source.last_transformed_at()
            || (front.reverse_face().is_none() && front.disturb.is_none() && !can_flip)
        {
            return Ok(None);
        }
        let evaluated = self.evaluated_characteristics()?;
        let Some(view) = evaluated.get(id) else {
            return Ok(None);
        };
        if !can_flip
            && (view.has_keyword(KeywordAbility::Daybound)
                || view.has_keyword(KeywordAbility::Nightbound)
                || Self::transform_prohibited(&evaluated, view))
        {
            return Ok(None);
        }
        Ok(Some(id))
    }

    /// Transform an existing incarnation (CR 701.28). The captured source
    /// version prevents a second self-transform from the same announcement.
    pub(super) fn transform_ability_source(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        append: &mut crate::occurrence::RulesJournal,
    ) -> Result<(), EffectError> {
        let Some(id) = self.available_transform_source(entry)? else {
            return Ok(());
        };
        let before = self.rules_snapshot(None)?;
        self.characteristics.restamp_transformations(&[id])?;
        self.window.transform(id)?;
        let clock = self.day_night();
        self.emit_from(
            crate::occurrence::EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |old, new, _| Self::day_change_occurrences(clock, clock, &[id], old, new),
        )?;
        Ok(())
    }

    fn subtype_transform_groups(
        &self,
        kind: spellwake_contracts::creature_types::CreatureTypeId,
    ) -> Result<BTreeMap<SeatId, Vec<ObjectId>>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let mut groups = BTreeMap::<SeatId, Vec<ObjectId>>::new();
        for object in self.window.cards().iter().filter(|object| {
            object.zone() == ObjectZone::Battlefield
                && object.face_down().is_none()
                && object
                    .card_id()
                    .and_then(|card| self.programs.get(&card))
                    .is_some_and(|front| front.reverse_face().is_some() || front.disturb.is_some())
        }) {
            let Some(view) = evaluated.get(object.id()) else {
                continue;
            };
            if let Some(controller) = view.controller
                && view.current_creature_subtypes.contains(&kind)
                && !view.has_keyword(KeywordAbility::Daybound)
                && !view.has_keyword(KeywordAbility::Nightbound)
                && !Self::transform_prohibited(&evaluated, view)
            {
                groups.entry(controller).or_default().push(object.id());
            }
        }
        Ok(groups)
    }

    pub(super) fn resolution_transform_order(
        &self,
        kind: spellwake_contracts::creature_types::CreatureTypeId,
    ) -> Result<Option<crate::day_night::TransformOrder>, EffectError> {
        let mut groups = self.subtype_transform_groups(kind)?;
        for controller in self.window.apnap_players()? {
            if let Some(objects) = groups.remove(&controller)
                && objects.len() > 1
                && !self.transform_orders.contains_key(&controller)
            {
                return Ok(Some(crate::day_night::TransformOrder {
                    controller,
                    objects,
                    clock: self.day_night(),
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn choose_resolution_transform_order(
        &mut self,
        frame: &crate::day_night::TransformOrder,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let Instruction::TransformSubtype(kind) = self.current_instruction()? else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if self.resolution_transform_order(*kind)?.as_ref() != Some(frame)
            || objects.len() != frame.objects.len()
            || objects.iter().collect::<BTreeSet<_>>() != frame.objects.iter().collect()
        {
            return Err(EffectError::IllegalTimestampOrder);
        }
        self.transform_orders.insert(frame.controller, objects);
        Ok(())
    }

    pub(super) fn transform_subtype(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        kind: spellwake_contracts::creature_types::CreatureTypeId,
        append: &mut crate::occurrence::RulesJournal,
    ) -> Result<(), EffectError> {
        let mut groups = self.subtype_transform_groups(kind)?;
        let mut orders = std::mem::take(&mut self.transform_orders);
        let mut subjects = Vec::new();
        for player in self.window.apnap_players()? {
            let Some(group) = groups.remove(&player) else {
                continue;
            };
            if group.len() > 1 {
                subjects.extend(
                    orders
                        .remove(&player)
                        .ok_or(EffectError::TransformTimestampOrderRequired)?,
                );
            } else {
                subjects.extend(group);
            }
        }
        if subjects.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        self.characteristics.restamp_transformations(&subjects)?;
        for object in subjects.iter().copied() {
            self.window.transform(object)?;
        }
        let clock = self.day_night();
        self.emit_from(
            crate::occurrence::EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |old, new, _| Self::day_change_occurrences(clock, clock, &subjects, old, new),
        )
    }

    pub fn day_night(&self) -> DayNight {
        self.window.cards().day_night
    }
    pub(crate) fn next_transform_order(
        &self,
    ) -> Result<Option<crate::day_night::TransformOrder>, EffectError> {
        let Some(clock) = self
            .previous_active_spells
            .and_then(|count| self.day_night().untap(count))
        else {
            return Ok(None);
        };
        let evaluated = self.evaluated_characteristics()?;
        for controller in self.window.apnap_players()? {
            if self.transform_orders.contains_key(&controller) {
                continue;
            }
            let objects = self
                .window
                .cards()
                .iter()
                .filter(|o| {
                    o.zone() == ObjectZone::Battlefield
                        && o.face_down().is_none()
                        && o.card_id()
                            .is_some_and(|id| self.window.cards().daybound_cards.contains(&id))
                        && evaluated.get(o.id()).is_some_and(|v| {
                            v.controller == Some(controller)
                                && !Self::transform_prohibited(&evaluated, v)
                                && match clock {
                                    DayNight::Night => {
                                        !o.transformed() && v.has_keyword(KeywordAbility::Daybound)
                                    }
                                    DayNight::Day => {
                                        o.transformed() && v.has_keyword(KeywordAbility::Nightbound)
                                    }
                                    DayNight::Neither => false,
                                }
                        })
                })
                .map(GameObject::id)
                .collect::<Vec<_>>();
            if objects.len() > 1 {
                return Ok(Some(crate::day_night::TransformOrder {
                    controller,
                    objects,
                    clock,
                }));
            }
        }
        Ok(None)
    }
    pub(crate) fn choose_transform_order(
        &mut self,
        frame: &crate::day_night::TransformOrder,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        if self.next_transform_order()?.as_ref() != Some(frame)
            || objects.len() != frame.objects.len()
            || objects.iter().collect::<BTreeSet<_>>() != frame.objects.iter().collect()
        {
            return Err(EffectError::IllegalTimestampOrder);
        }
        self.transform_orders.insert(frame.controller, objects);
        Ok(())
    }
    pub(super) fn reconcile_day_night(
        &mut self,
        requested: Option<DayNight>,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<(DayNight, Vec<ObjectId>), EffectError> {
        let previous = self.day_night();
        // Any copiable instance of either keyword can establish the designation.

        let mut faces = vec![];
        let mut day = false;
        let mut night = false;
        for object in self
            .window
            .cards()
            .iter()
            .filter(|o| o.zone() == ObjectZone::Battlefield)
        {
            let current = evaluated
                .get(object.id())
                .ok_or(EffectError::InconsistentProgram)?;
            let has_day = current.has_keyword(KeywordAbility::Daybound);
            let has_night = current.has_keyword(KeywordAbility::Nightbound);
            let becomes_day = current.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::Modifier {
                        rule: crate::characteristics::StaticModifierRule::BecomesDayIfNeither,
                        ..
                    }
                )
            });
            day |= has_day || becomes_day;
            night |= has_night;
            if object.face_down().is_none()
                && !Self::transform_prohibited(evaluated, current)
                && object
                    .card_id()
                    .is_some_and(|id| self.window.cards().daybound_cards.contains(&id))
            {
                faces.push((object.id(), object.transformed(), has_day, has_night));
            }
        }
        let clock = requested.unwrap_or(match previous {
            DayNight::Neither if day => DayNight::Day,
            DayNight::Neither if night => DayNight::Night,
            _ => previous,
        });
        let wanted = faces
            .into_iter()
            .filter_map(|(id, back, day, night)| {
                ((clock == DayNight::Day && back && night)
                    || (clock == DayNight::Night && !back && day))
                    .then_some(id)
            })
            .collect::<Vec<_>>();
        if wanted.is_empty() {
            if previous != clock {
                self.window.set_day_faces(clock, &[])?;
            }
            return Ok((clock, vec![]));
        }
        let mut to_flip = Vec::new();
        for controller in self.window.apnap_players()? {
            let group = wanted
                .iter()
                .copied()
                .filter(|id| {
                    evaluated
                        .get(*id)
                        .is_some_and(|p| p.controller == Some(controller))
                })
                .collect::<Vec<_>>();
            if group.len() > 1 {
                let selected = self
                    .transform_orders
                    .get(&controller)
                    .ok_or(EffectError::TransformTimestampOrderRequired)?;
                if selected.len() != group.len()
                    || selected.iter().collect::<BTreeSet<_>>() != group.iter().collect()
                {
                    return Err(EffectError::IllegalTimestampOrder);
                }
                to_flip.extend_from_slice(selected);
            } else {
                to_flip.extend(group);
            }
        }
        if !to_flip.is_empty() {
            self.characteristics.restamp_transformations(&to_flip)?;
        }
        if previous != clock || !to_flip.is_empty() {
            self.window.set_day_faces(clock, &to_flip)?;
        }
        Ok((clock, to_flip))
    }
    pub(super) fn day_change_occurrences(
        before: DayNight,
        after: DayNight,
        faces: &[ObjectId],
        old: &RulesView<'_>,
        new: &RulesView<'_>,
    ) -> Result<Vec<RulesOccurrence>, EffectError> {
        let mut values = vec![];
        if before != after {
            values.push(RulesOccurrence::DayNightChanged { before, after });
        }
        for &id in faces {
            if let (Some(before), Some(after)) = (old.appearance(id), new.appearance(id)) {
                values.push(RulesOccurrence::Transformed {
                    before: Box::new(before),
                    after: Box::new(after),
                });
            }
        }
        Ok(values)
    }
}
