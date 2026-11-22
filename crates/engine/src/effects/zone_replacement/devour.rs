//! Devour choices belong to the prospective entry. Sacrifices retain their own
//! replaceable destinations, owner ordering, and pre-entry occurrence cut.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::effects) struct Devour {
    pub(super) rules: BTreeMap<EntryCopySubject, (SeatId, u8)>,
    fodder: BTreeMap<ObjectId, GameObject>,
    done: BTreeSet<EntryCopySubject>,
    selected: BTreeMap<ObjectId, EntryCopySubject>,
    ordered: BTreeMap<SeatId, Vec<ObjectId>>,
    pub(super) replies: Vec<(usize, ZoneReplacementReply)>,
}

pub(in crate::effects) fn multiplier(program: &Program) -> Option<u8> {
    program
        .entry_counter_abilities
        .iter()
        .find_map(|rule| match rule {
            crate::program::EntryCounterAbility::Devour(n) => Some(*n),
            _ => None,
        })
}

impl ZoneReplacementBatch {
    pub(in crate::effects) fn enable_devour(
        &mut self,
        window: &EffectWindow,
        rules: BTreeMap<EntryCopySubject, (SeatId, u8)>,
    ) {
        self.devour.rules = rules;
        self.devour.fodder = window
            .window
            .cards()
            .iter()
            .filter(|card| {
                self.evaluated
                    .get(card.id())
                    .is_some_and(|obj| obj.zone == ObjectZone::Battlefield && obj.is_creature())
                    && !self.evaluated.sacrifice_prohibited(card.id())
            })
            .map(|card| (card.id(), card.clone()))
            .collect();
    }

    pub(super) fn devour_choice(&self) -> Option<(SeatId, ZoneReplacementRequest)> {
        for &player in &self.apnap {
            for (&entrant, &(controller, _)) in &self.devour.rules {
                let controller = match entrant {
                    EntryCopySubject::Card(id) => {
                        if self.destination(id) != Ok(PublicZoneDestination::Battlefield) {
                            continue;
                        }
                        self.entry_controller(id).unwrap_or(controller)
                    }
                    EntryCopySubject::Token(_) => controller,
                };
                if controller != player || self.devour.done.contains(&entrant) {
                    continue;
                }
                let choices: Vec<_> = self
                    .devour
                    .fodder
                    .keys()
                    .copied()
                    .filter(|id| {
                        !self.devour.selected.contains_key(id)
                            && self
                                .evaluated
                                .get(*id)
                                .is_some_and(|obj| obj.controller == Some(player))
                    })
                    .collect();
                if !choices.is_empty() {
                    return Some((player, ZoneReplacementRequest::Devour { entrant, choices }));
                }
            }
        }
        for &owner in &self.apnap {
            let ordered = self.devour.ordered.get(&owner);
            let choices: Vec<_> = self
                .devour
                .selected
                .keys()
                .copied()
                .filter(|id| {
                    self.devour
                        .fodder
                        .get(id)
                        .is_some_and(|card| card.owner() == owner && card.card_id().is_some())
                        && self.destination(*id) == Ok(PublicZoneDestination::Graveyard)
                        && !ordered.is_some_and(|order| order.contains(id))
                })
                .collect();
            if choices.len() > 1 {
                return Some((
                    owner,
                    ZoneReplacementRequest::DevourGraveyardOrder { choices },
                ));
            }
        }
        None
    }

    pub(super) fn select_devour(&mut self, reply: ZoneReplacementReply) -> Result<(), EffectError> {
        let (actor, request) = self
            .devour_choice()
            .ok_or(ZoneReplacementError::WrongFrame)?;
        match (request, &reply) {
            (
                ZoneReplacementRequest::Devour { entrant, choices },
                ZoneReplacementReply::Devour(Some(id)),
            ) if choices.contains(id) => {
                let before = self
                    .devour
                    .fodder
                    .get(id)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?
                    .clone();
                let key = match self.cause {
                    EventCause::Instruction { entry, instruction } => ZoneEventKey::Instruction {
                        entry,
                        instruction,
                        before: *id,
                    },
                    EventCause::ResolutionCompleted { entry, .. } => {
                        ZoneEventKey::PermanentEntry { entry, before: *id }
                    }
                    EventCause::LandPlay { .. } => ZoneEventKey::LandPlay { before: *id },
                    EventCause::MechanismUnit => ZoneEventKey::LinkedReturn { before: *id },
                    _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
                };
                let grave = PublicZoneDestination::Graveyard;
                let candidate = ZoneCandidate {
                    key,
                    before,
                    action: ZoneActionKind::EffectSacrifice,
                    reason: ZoneReason::Sacrifice { player: actor },
                    original: grave,
                    current: grave,
                    entry_prohibited: false,
                    entering_controller: None,
                };
                if self.candidates.insert(key, candidate).is_some() {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                self.devour.selected.insert(*id, entrant);
            }
            (
                ZoneReplacementRequest::Devour { entrant, .. },
                ZoneReplacementReply::Devour(None),
            ) => {
                self.devour.done.insert(entrant);
            }
            (
                ZoneReplacementRequest::DevourGraveyardOrder { choices },
                ZoneReplacementReply::DevourGraveyardOrder(id),
            ) if choices.contains(id) => {
                self.devour.ordered.entry(actor).or_default().push(*id);
            }
            _ => return Err(EffectError::IllegalTarget),
        }
        self.bump()?;
        self.devour.replies.push((self.history.len(), reply));
        Ok(())
    }

    pub(in crate::effects) fn devour_counters(&self, entrant: EntryCopySubject) -> usize {
        self.devour.rules.get(&entrant).map_or(0, |(_, factor)| {
            usize::from(*factor)
                * self
                    .devour
                    .selected
                    .values()
                    .filter(|&&key| key == entrant)
                    .count()
        })
    }

    pub(in crate::effects) fn commit_devour(
        &self,
        window: &mut EffectWindow,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.devour.selected.is_empty() {
            return Ok(());
        }
        let objects: Vec<_> = self.devour.selected.keys().copied().collect();
        let orders = window
            .zone_arrival_groups(&objects, Some(self))?
            .into_iter()
            .map(|group| {
                let mut top_to_bottom = self
                    .devour
                    .ordered
                    .get(&group.owner)
                    .cloned()
                    .unwrap_or_default();
                let remaining: Vec<_> = group
                    .entrants
                    .into_iter()
                    .filter(|id| !top_to_bottom.contains(id))
                    .collect();
                if remaining.len() > 1 {
                    return Err(EffectError::InconsistentProgram);
                }
                top_to_bottom.extend(remaining);
                Ok(ZoneOwnerOrder {
                    owner: group.owner,
                    top_to_bottom,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let plan = window.zone_move_plan(&objects, &orders, Some(self))?;
        let moves = plan
            .application
            .into_iter()
            .map(|(id, dest)| {
                Ok((
                    id,
                    dest.physical(None)
                        .ok_or(EffectError::InconsistentProgram)?,
                ))
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let before = window.rules_snapshot(None)?;
        let (next, movements) = window.window.record_linked_return_moves(&moves)?;
        window.window = next;
        window.emit_from(
            self.cause.clone(),
            &before,
            None,
            append,
            |old, current, controllers| {
                movements
                    .iter()
                    .map(|movement| {
                        let player = self
                            .evaluated
                            .get(movement.before.id())
                            .and_then(|obj| obj.controller)
                            .ok_or(EffectError::InconsistentProgram)?;
                        zone_occurrence_at(
                            old,
                            current,
                            movement,
                            ZoneReason::Sacrifice { player },
                            controllers,
                        )
                    })
                    .collect()
            },
        )?;
        for &subject in self.devour.rules.keys() {
            if let EntryCopySubject::Card(id) = subject {
                window
                    .prospective_devour_counters
                    .insert(id, self.devour_counters(subject));
            }
        }
        Ok(())
    }
}
