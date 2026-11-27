//! Shared batch continuation and one atomic physical result path.
mod static_prevention;
use super::*;
use crate::{
    life::LifeError,
    occurrence::{EventCause, RulesOccurrence},
};
mod protection;
#[cfg(test)]
mod tests;
mod turn_prevention;

// Kept in the test module: this trusted primitive fixture deliberately exercises
// player recipients without broadening authored spell targeting or player inputs.
#[cfg(test)]
pub(crate) use tests::n04_player_batch_fixture;

pub(super) enum DamageProgressStep {
    Pending(DamagePending),
    NextBatch,
    ResolutionContinued,
    CombatComplete { additional_step_required: bool },
    ManaComplete,
    ManaChoice(ManaColorChoice),
}

pub(super) enum DamagePending {
    Damage(DamageDecision),
    Zone(ZoneReplacementFrame),
    GraveyardOrder(PileChoiceFrame),
}

impl DamagePending {
    pub(super) fn into_frame(self) -> EffectDecision {
        match self {
            Self::Damage(frame) => EffectDecision::Damage(frame),
            Self::Zone(frame) => EffectDecision::ZoneReplacement(frame),
            Self::GraveyardOrder(frame) => EffectDecision::RevealedPiles(frame),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DamageFollowup {
    RemovePlusOneCounter {
        object: ObjectId,
        count: RulesInteger,
    },
    RemoveShieldCounter(ObjectId),
    PutCounters {
        target: DamageRecipient,
        kind: CounterKind,
        count: RulesInteger,
    },
    RemoveCounters {
        object: ObjectId,
        kind: CounterKind,
        count: RulesInteger,
    },
    InlineBody {
        replacement: Box<super::event_replacement::EventBodyReplacement>,
        prevented: RulesInteger,
    },
    LibraryTop {
        player: SeatId,
        amount: RulesInteger,
        mill: bool,
    },
    DealDamage {
        batch: crate::damage::DamageBatchId,
        creator: StackEntryId,
        amount: RulesInteger,
        reflection: Box<crate::damage::DamageReflection>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DamageZoneFollowup {
    batch: ZoneReplacementBatch,
    graveyard_order: Option<Vec<ObjectId>>,
}

impl DamageZoneFollowup {
    fn graveyard_frame(&self, revision: u64) -> Result<Option<PileChoiceFrame>, EffectError> {
        use crate::program::PublicZoneDestination;
        let Some(crate::occurrence::ZoneReason::Mill { player }) =
            self.batch.intents().first().map(|intent| intent.reason)
        else {
            return Ok(None);
        };
        if self.graveyard_order.is_some() {
            return Ok(None);
        }
        let mut graveyard = Vec::new();
        for intent in self.batch.intents() {
            if self.batch.destination(intent.before)? == PublicZoneDestination::Graveyard {
                graveyard.push(intent.before);
            }
        }
        Ok((graveyard.len() > 1)
            .then(|| PileChoiceFrame::graveyard_order(revision, player, graveyard)))
    }
}

impl EffectWindow {
    pub(super) fn install_mana_damage_batch(
        &mut self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        controller: SeatId,
        amount: RulesInteger,
        parent: ManaParent,
        journal: &mut DamageJournal,
    ) -> Result<(), EffectError> {
        let object = self
            .window
            .cards()
            .get(source)
            .filter(|object| {
                object.zone() == ObjectZone::Battlefield && object.controller() == Some(controller)
            })
            .ok_or(EffectError::UnknownObject)?
            .clone();
        let origin = DamageOrigin::ManaAbility { source, ability };
        let input = DamageInput {
            source: DamageSource::Object { object, controller },
            target: DamageRecipient::Player(controller),
            amount,
        };
        let (batch, append) =
            self.damage_state
                .begin_batch(origin, DamageKind::Noncombat, vec![input])?;
        self.pending_work = Some(PendingWork::Damage(DamageWork {
            batch,
            parent: DamageParent::Mana(parent),
            original_origin: origin,
            reflection: None,
            followup_tails: Vec::new(),
            followups: VecDeque::new(),
            results_committed: false,
            followup_zone: None,
        }));
        journal.append(append);
        Ok(())
    }
    pub(super) fn group_damage_recipients(
        &self,
        group: crate::program::DamageGroup,
        controller: SeatId,
    ) -> Result<Vec<DamageRecipient>, EffectError> {
        use crate::program::DamageGroup;
        let participants = self.window.participants();
        if group == DamageGroup::CapturedAttackedPlayerOrPlaneswalker {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|binding| binding.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [cause] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::AttackGroup { attacked, .. } = cause.related
            else {
                return Err(EffectError::InconsistentProgram);
            };
            return Ok(match attacked {
                TargetRef::Player(player) if participants.contains(&player) => {
                    vec![DamageRecipient::Player(player)]
                }
                TargetRef::Object(id)
                    if self.current_characteristics(id)?.is_some_and(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.has_type(CardType::Planeswalker)
                    }) =>
                {
                    vec![DamageRecipient::Planeswalker(id)]
                }
                _ => vec![],
            });
        }
        let players = match group {
            DamageGroup::CapturedAttackedPlayerOrPlaneswalker => Vec::new(),
            DamageGroup::DestroyedTargetController => self
                .resolution_cursor()
                .and_then(|cursor| cursor.results.destroyed_target_controller)
                .filter(|player| participants.contains(player))
                .into_iter()
                .collect(),
            DamageGroup::Controller | DamageGroup::ControllerAndTargetCreature => vec![controller],
            DamageGroup::VolunteeringOpponent => {
                let player = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_opponent)
                    .ok_or(EffectError::InconsistentProgram)?;
                participants
                    .contains(&player)
                    .then_some(player)
                    .into_iter()
                    .collect()
            }
            DamageGroup::RelatedPlayer | DamageGroup::RelatedPlayerOrTheirPlaneswalker => {
                let player = self.captured_related_player()?;
                participants
                    .contains(&player)
                    .then_some(player)
                    .into_iter()
                    .collect()
            }
            DamageGroup::Opponents => self.combat_rules.opponents(participants, controller)?,
            DamageGroup::Players | DamageGroup::CreaturesAndPlayers => participants.to_vec(),
            DamageGroup::FilteredAndPlayers { players, .. }
            | DamageGroup::AffectedPlayers(players) => {
                let cursor = self.resolution_cursor();
                self.selection_players(players, controller, cursor)?
            }
            DamageGroup::CreaturesAndPlaneswalkers
            | DamageGroup::Creatures
            | DamageGroup::CreaturesExcept(_)
            | DamageGroup::CreaturesOfSubtype(_)
            | DamageGroup::CreaturesWithoutFlying
            | DamageGroup::AbilitySource
            | DamageGroup::Filtered(_) => Vec::new(),
        };
        let mut targets: Vec<_> = players.into_iter().map(DamageRecipient::Player).collect();
        if group == DamageGroup::AbilitySource {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if let StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } =
                entry.kind()
                && let Some(o) = self.evaluated_characteristics()?.get(source.id())
                && o.zone == ObjectZone::Battlefield
            {
                if o.is_creature() {
                    targets.push(DamageRecipient::Creature(o.object));
                } else if o.has_type(CardType::Planeswalker) {
                    targets.push(DamageRecipient::Planeswalker(o.object));
                } else if o.has_type(CardType::Battle) {
                    targets.push(DamageRecipient::Battle(o.object));
                }
            }
        }
        if let DamageGroup::Filtered(filter) | DamageGroup::FilteredAndPlayers { filter, .. } =
            group
        {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let ability_source = match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                _ => None,
            };
            let evaluated = self.evaluated_characteristics()?;
            let excluded = BTreeSet::new();
            targets.extend(
                evaluated
                    .objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && self.matches_object_filter(
                                filter,
                                o,
                                controller,
                                ability_source,
                                &excluded,
                                &evaluated,
                            )
                    })
                    .filter_map(|o| {
                        if o.is_creature() {
                            Some(DamageRecipient::Creature(o.object))
                        } else if o.has_type(CardType::Planeswalker) {
                            Some(DamageRecipient::Planeswalker(o.object))
                        } else if o.has_type(CardType::Battle) {
                            Some(DamageRecipient::Battle(o.object))
                        } else {
                            None
                        }
                    }),
            );
        }
        if group == DamageGroup::RelatedPlayerOrTheirPlaneswalker
            && let [DamageRecipient::Player(player)] = targets.as_slice()
        {
            let player = *player;
            targets.extend(
                self.evaluated_characteristics()?
                    .objects
                    .values()
                    .filter(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.controller == Some(player)
                            && object.has_type(CardType::Planeswalker)
                    })
                    .map(|object| DamageRecipient::Planeswalker(object.object)),
            );
        }
        if group == DamageGroup::ControllerAndTargetCreature {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            targets.push(DamageRecipient::Creature(target));
        }
        if group == DamageGroup::CreaturesAndPlaneswalkers {
            targets.extend(
                self.evaluated_characteristics()?
                    .objects
                    .values()
                    .filter(|o| o.zone == ObjectZone::Battlefield)
                    .filter_map(|o| {
                        if o.is_creature() {
                            Some(DamageRecipient::Creature(o.object))
                        } else if o.has_type(CardType::Planeswalker) {
                            Some(DamageRecipient::Planeswalker(o.object))
                        } else {
                            None
                        }
                    }),
            );
        }
        if matches!(
            group,
            DamageGroup::Creatures
                | DamageGroup::CreaturesExcept(_)
                | DamageGroup::CreaturesOfSubtype(_)
                | DamageGroup::CreaturesWithoutFlying
                | DamageGroup::CreaturesAndPlayers
        ) {
            targets.extend(
                self.evaluated_characteristics()?
                    .objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.is_creature()
                            && match group {
                                DamageGroup::CreaturesExcept(kind) => {
                                    !o.current_creature_subtypes.contains(&kind)
                                }
                                DamageGroup::CreaturesOfSubtype(kind) => {
                                    o.current_creature_subtypes.contains(&kind)
                                }
                                DamageGroup::CreaturesWithoutFlying => {
                                    !o.has_keyword(KeywordAbility::Flying)
                                }
                                _ => true,
                            }
                    })
                    .map(|o| DamageRecipient::Creature(o.object)),
            );
        }
        Ok(targets)
    }

    pub(super) fn target_controller_damage_recipients(
        &self,
        target: TargetRef,
        include_target: bool,
    ) -> Result<Vec<DamageRecipient>, EffectError> {
        let current = self.evaluated_characteristics()?;
        let (player, target_recipient, target_object) = match target {
            TargetRef::Player(player) => (player, DamageRecipient::Player(player), None),
            TargetRef::Object(object) => {
                let player = current
                    .get(object)
                    .filter(|permanent| {
                        permanent.zone == ObjectZone::Battlefield
                            && permanent.has_type(CardType::Planeswalker)
                    })
                    .and_then(|permanent| permanent.controller)
                    .ok_or(EffectError::IllegalTarget)?;
                (player, DamageRecipient::Planeswalker(object), Some(object))
            }
            TargetRef::Stack(_) => return Err(EffectError::IllegalTarget),
        };
        let mut recipients = Vec::new();
        if include_target {
            recipients.push(target_recipient);
        }
        recipients.extend(
            current
                .objects
                .values()
                .filter(|permanent| {
                    permanent.zone == ObjectZone::Battlefield
                        && permanent.is_creature()
                        && permanent.controller == Some(player)
                        && (!include_target || Some(permanent.object) != target_object)
                })
                .map(|permanent| DamageRecipient::Creature(permanent.object)),
        );
        Ok(recipients)
    }

    fn multiply_damage_at_cut(
        &mut self,
        application: DamageApplication,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let batch = &mut work.batch;
        let multiplication = self.damage_state.apply_multiplier(batch, application)?;
        self.emit_from(
            EventCause::DamageAdjusted {
                batch: multiplication.batch,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::DamageMultiplied(
                    multiplication.clone(),
                )])
            },
        )?;
        journal.multiplications.push(multiplication);
        Ok(())
    }
    pub(super) fn record_resolution_damage_prevented(&mut self, amount: &RulesInteger) {
        if !amount.is_positive() {
            return;
        }
        if let Some(PendingWork::Damage(work)) = &mut self.pending_work
            && let DamageParent::Resolution(cursor) = &mut work.parent
        {
            cursor.results.damage_prevented_this_resolution =
                cursor.results.damage_prevented_this_resolution.plus(amount);
        }
    }

    fn pending_damage_total(&self) -> RulesInteger {
        self.pending_damage().map_or_else(
            || 0.into(),
            |batch| {
                batch
                    .components()
                    .fold(RulesInteger::from(0), |sum, component| {
                        sum.plus(component.remaining())
                    })
            },
        )
    }

    fn prevent_damage_at_cut(
        &mut self,
        application: DamageApplication,
        allocations: Vec<DamageAllocation>,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let DamageEffectId::Prevention(shield) = application.effect else {
            return Err(EffectError::WrongDamageFrame);
        };
        let reflection = self.damage_state.reflection(shield).cloned();
        let shield_before = self
            .damage_state
            .shield(shield)
            .ok_or(EffectError::WrongDamageFrame)?
            .remaining
            .clone();
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let batch = &mut work.batch;
        let damage_before = batch
            .components()
            .map(|component| (component.attempt().id, component.remaining().clone()))
            .collect();
        let prevention = self
            .damage_state
            .apply_prevention(batch, application, allocations)?;
        if let Some(reflection) = reflection {
            work.followups.push_back(DamageFollowup::DealDamage {
                batch: prevention.batch,
                creator: prevention.creator,
                amount: prevention.amount.clone(),
                reflection: Box::new(reflection),
            });
        }
        self.record_resolution_damage_prevented(&prevention.amount);
        self.emit_from(
            EventCause::DamageAdjusted {
                batch: prevention.batch,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::DamagePrevented {
                    application: prevention.clone(),
                    damage_before,
                    shield_before,
                }])
            },
        )?;
        journal.preventions.push(prevention);
        Ok(())
    }
    pub(super) fn damage_dealer_power(
        &self,
        entry: &crate::stack_window::StackEntry,
        source: crate::program::PowerDamageSource,
    ) -> Result<Option<(DamageSource, RulesInteger)>, EffectError> {
        use crate::program::PowerDamageSource;
        let id = match source {
            PowerDamageSource::AbilitySource => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    source.id()
                }
                _ => return Ok(None),
            },
            PowerDamageSource::TargetCreature
            | PowerDamageSource::TargetCreatureToAbilitySource => {
                let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                target
            }
            PowerDamageSource::GraveyardEntrant => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                let [member] = binding.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let crate::occurrence::TriggerRelated::ZoneChange { after, .. } = &member.related
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                after.object.id()
            }
            PowerDamageSource::CreatureDealtDamage => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                match binding
                    .causes
                    .members()
                    .first()
                    .map(|member| &member.related)
                {
                    Some(crate::occurrence::TriggerRelated::AttachedCreatureDamaged {
                        creature,
                        ..
                    }) => *creature,
                    _ => return Err(EffectError::InconsistentProgram),
                }
            }
        };
        self.damage_source_power(id)
    }

    pub(super) fn ability_source_damage_recipient(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<Option<DamageRecipient>, EffectError> {
        let id = match entry.kind() {
            StackKind::ActivatedAbility { source } => source.id(),
            StackKind::TriggeredAbility { source } => source.id(),
            _ => return Err(EffectError::InconsistentProgram),
        };
        Ok(self
            .current_characteristics(id)?
            .filter(|_| self.window.cards().phased_out_under(id).is_none())
            .and_then(|source| source.damage_recipient()))
    }

    fn damage_source_power(
        &self,
        id: ObjectId,
    ) -> Result<Option<(DamageSource, RulesInteger)>, EffectError> {
        let current = self
            .window
            .cards()
            .get(id)
            .filter(|object| object.zone() == ObjectZone::Battlefield);
        let (object, power) = if let Some(object) = current {
            let power = self
                .current_characteristics(id)?
                .and_then(|view| view.power_toughness.map(|pt| pt.derived.power));
            (object, power)
        } else if let Some(last) = self.departed_damage_sources.get(&id) {
            (&last.object, last.power.clone())
        } else {
            return Ok(None);
        };
        let Some(power) = power.filter(RulesInteger::is_positive) else {
            return Ok(None);
        };
        let controller = object
            .controller()
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(Some((
            DamageSource::Object {
                object: object.clone(),
                controller,
            },
            power,
        )))
    }
    pub(super) fn cohort_power_damage(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        returning: bool,
    ) -> Result<Vec<DamageInput>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        if returning {
            let (target, sources) = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results
                .power_damage_cohort
                .take()
                .ok_or(EffectError::InconsistentProgram)?;
            let Some(view) = evaluated
                .get(target)
                .filter(|view| view.zone == ObjectZone::Battlefield)
            else {
                return Ok(Vec::new());
            };
            let recipient = if view.is_creature() {
                DamageRecipient::Creature(target)
            } else if view.has_type(CardType::Planeswalker) {
                DamageRecipient::Planeswalker(target)
            } else if view.has_type(CardType::Battle) {
                DamageRecipient::Battle(target)
            } else {
                return Ok(Vec::new());
            };
            let mut inputs = Vec::new();
            for id in sources {
                if let Some((source, amount)) = self.damage_source_power(id)? {
                    inputs.push(DamageInput {
                        source,
                        target: recipient,
                        amount,
                    });
                }
            }
            return Ok(inputs);
        }

        let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        let controller = evaluated
            .get(target)
            .and_then(|card| card.controller)
            .ok_or(EffectError::InconsistentProgram)?;
        let others = evaluated
            .objects
            .values()
            .filter(|card| {
                card.object != target
                    && card.zone == ObjectZone::Battlefield
                    && card.is_creature()
                    && card.controller == Some(controller)
            })
            .map(|card| card.object)
            .collect::<Vec<_>>();
        let damage = self.damage_source_power(target)?;
        let inputs = damage.map_or_else(Vec::new, |(source, amount)| {
            others
                .iter()
                .copied()
                .map(|object| DamageInput {
                    source: source.clone(),
                    target: DamageRecipient::Creature(object),
                    amount: amount.clone(),
                })
                .collect()
        });
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .results
            .power_damage_cohort = Some((target, others));
        Ok(inputs)
    }

    pub(super) fn reflected_attached_damage(
        &self,
        entry: StackEntryId,
    ) -> Result<Option<DamageInput>, EffectError> {
        use crate::occurrence::TriggerRelated;
        let binding = self
            .execution_bindings(entry)
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let causes = binding.causes.members();
        let Some(TriggerRelated::AttachedCreatureDamaged { creature, .. }) =
            causes.first().map(|member| &member.related)
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let amount =
            causes
                .iter()
                .try_fold(RulesInteger::from(0), |total, member| {
                    match &member.related {
                        TriggerRelated::AttachedCreatureDamaged {
                            creature: same,
                            amount,
                            ..
                        } if same == creature => Ok(total.plus(amount)),
                        _ => Err(EffectError::InconsistentProgram),
                    }
                })?;
        let current = self
            .window
            .cards()
            .get(*creature)
            .filter(|card| card.zone() == ObjectZone::Battlefield);
        let Some(object) = current.or_else(|| {
            self.departed_damage_sources
                .get(creature)
                .map(|source| &source.object)
        }) else {
            return Ok(None);
        };
        let Some(controller) = object
            .controller()
            .filter(|player| self.window.participants().contains(player))
        else {
            return Ok(None);
        };
        Ok(Some(DamageInput {
            source: DamageSource::Object {
                object: object.clone(),
                controller,
            },
            target: DamageRecipient::Player(controller),
            amount,
        }))
    }

    pub(super) fn resolution_damage_source(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<DamageSource, EffectError> {
        if let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id())
            && let Some(source) = payload.granting_damage_source.as_deref()
        {
            return Ok(source.clone());
        }
        match entry.kind() {
            StackKind::CardSpell(object) => {
                let object = self
                    .window
                    .cards()
                    .get(*object)
                    .ok_or(EffectError::UnknownObject)?;
                Ok(DamageSource::Object {
                    object: object.clone(),
                    controller: entry.controller(),
                })
            }
            StackKind::Speed | StackKind::Monarch | StackKind::DelayedCopy { .. } => {
                Err(EffectError::InconsistentProgram)
            }
            StackKind::Emblem { emblem } => {
                let source = self
                    .characteristics
                    .emblems
                    .get(emblem)
                    .ok_or(EffectError::InconsistentProgram)?;
                Ok(DamageSource::Emblem {
                    emblem: *emblem,
                    controller: source.controller,
                })
            }
            StackKind::SpellCopy { .. } => Ok(DamageSource::SpellCopy {
                entry: entry.id(),
                controller: entry.controller(),
            }),
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let current = self
                    .window
                    .cards()
                    .get(source.id())
                    .filter(|card| card.zone() == ObjectZone::Battlefield);
                let object = current
                    .or_else(|| {
                        self.departed_damage_sources
                            .get(&source.id())
                            .map(|departed| &departed.object)
                    })
                    .unwrap_or(source)
                    .clone();
                // A card outside the battlefield or stack has no controller. When its
                // own ability deals damage, controller references use its owner (CR 108.4a).
                let controller = object.controller().unwrap_or_else(|| object.owner());
                Ok(DamageSource::Object { object, controller })
            }
        }
    }
    #[cfg(test)]
    pub(super) fn combat_damage_source(
        &self,
        object: ObjectId,
    ) -> Result<DamageSource, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        self.combat_damage_source_at(&evaluated, object)
    }
    /// Reuse one evaluated cut while collecting a whole combat damage batch.
    pub(super) fn combat_damage_source_at(
        &self,
        evaluated: &EvaluatedCharacteristics,
        object: ObjectId,
    ) -> Result<DamageSource, EffectError> {
        if !evaluated
            .get(object)
            .is_some_and(ObjectCharacteristics::is_creature)
        {
            return Err(CombatError::InvalidContext.into());
        }
        let object = self
            .window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Battlefield)
            .ok_or(EffectError::UnknownObject)?
            .clone();
        let controller = object.controller().ok_or(CombatError::InvalidContext)?;
        Ok(DamageSource::Object { object, controller })
    }
    pub(crate) fn original_damage_origin(&self) -> Option<DamageOrigin> {
        match &self.pending_work {
            Some(PendingWork::Damage(work)) => Some(work.original_origin),
            _ => None,
        }
    }

    pub(super) fn current_damage_reflection(
        &self,
        source: &DamageSource,
    ) -> Option<&crate::damage::DamageReflection> {
        let Some(PendingWork::Damage(work)) = &self.pending_work else {
            return None;
        };
        work.reflection
            .as_deref()
            .filter(|reflection| reflection.source.id() == source.id())
    }

    fn validate_damage_origin_context(&self, origin: DamageOrigin) -> Result<(), EffectError> {
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        if let Some(PendingWork::Damage(work)) = &self.pending_work {
            let parent_agrees = match (&work.parent, origin) {
                (
                    DamageParent::Resolution(cursor),
                    DamageOrigin::Resolution { entry, instruction },
                ) => cursor.entry == entry && cursor.instruction == instruction,
                (DamageParent::Combat, DamageOrigin::Combat { .. }) => true,
                (DamageParent::Mana(_), DamageOrigin::ManaAbility { .. }) => true,
                (_, DamageOrigin::PreventionFollowup { .. }) => work.reflection.is_some(),
                _ => false,
            };
            if !parent_agrees || work.batch.origin() != origin {
                return Err(EffectError::WrongDamageFrame);
            }
        }
        match origin {
            DamageOrigin::PreventionFollowup { .. } => {
                if self.original_damage_origin().is_none() {
                    return Err(EffectError::WrongDamageFrame);
                }
            }
            DamageOrigin::ManaAbility { source, ability } => {
                if !matches!(&self.pending_work, Some(PendingWork::Damage(work)) if matches!(work.batch.origin(), DamageOrigin::ManaAbility { source: s, ability: a } if s == source && a == ability))
                {
                    return Err(EffectError::WrongDamageFrame);
                }
            }
            DamageOrigin::Resolution { entry, instruction } => {
                if self.window.resolving().map(|e| e.id()) != Some(entry)
                    || !matches!(
                        self.payloads
                            .get(&entry)
                            .and_then(StackPayload::instructions)
                            .and_then(|p| p.get(instruction)),
                        Some(
                            Instruction::Fight { .. }
                                | Instruction::FightSourceWithTarget
                                | Instruction::FightFixedTargets
                                | Instruction::FightPairedCreatures(_)
                                | Instruction::ImpulseCastOrDamage(_)
                                | Instruction::ExileNonlandsToHandAndDamage { .. }
                                | Instruction::ControlledSubtypeDamagesAttacker(_)
                                | Instruction::DivideDamageAmongTargets { .. }
                                | Instruction::DivideDamageExprAmongTargets { .. }
                                | Instruction::DivideThreeDamageAmongTargets
                                | Instruction::DamageOrderedTargets { .. }
                                | Instruction::DamageFromPower(_)
                                | Instruction::CreatureDealsPowerDamageToOther { .. }
                                | Instruction::DealDamage(_)
                                | Instruction::DamageCreatureAtSpellMastery { .. }
                                | Instruction::DamagePlayersHalfLife
                                | Instruction::TargetDamagesItsControllersOtherCreatures
                                | Instruction::OtherCreaturesReturnPowerDamage
                                | Instruction::ReflectAttachedCreatureDamage
                                | Instruction::AddCounterAndDischargeOnResolution(_)
                                | Instruction::DamageTargetControllersCreatures { .. }
                        )
                    )
                {
                    return Err(EffectError::WrongDamageFrame);
                }
            }
            DamageOrigin::Combat { position } => {
                if self.window.phase() != WindowPhase::TurnWork
                    || !self.window.entries().is_empty()
                    || self.combat.is_none()
                    || position.step != crate::turn::Step::CombatDamage
                    || (position.additional_damage
                        && self
                            .combat
                            .as_ref()
                            .is_none_or(|combat| combat.first_strike_participants().is_empty()))
                {
                    return Err(EffectError::WrongPhase);
                }
            }
        }
        Ok(())
    }
    pub(super) fn emit_fight_occurrences(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        pairs: &[(ObjectId, ObjectId)],
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let current = &before.view().evaluated;
        let mut occurrences = Vec::new();
        for &(first, second) in pairs {
            let both_creatures = [first, second].into_iter().all(|id| {
                current
                    .get(id)
                    .is_some_and(|view| view.zone == ObjectZone::Battlefield && view.is_creature())
            });
            if both_creatures {
                occurrences.push(RulesOccurrence::PermanentFought {
                    object: first,
                    opponent: second,
                });
                if first != second {
                    occurrences.push(RulesOccurrence::PermanentFought {
                        object: second,
                        opponent: first,
                    });
                }
            }
        }
        if !occurrences.is_empty() {
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &before,
                None,
                append,
                |_, _, _| Ok(occurrences),
            )?;
        }
        Ok(())
    }

    pub(super) fn fight_damage_inputs(
        &self,
        pairs: &[(ObjectId, ObjectId)],
    ) -> Result<Vec<DamageInput>, EffectError> {
        let current = self.evaluated_characteristics()?;
        let mut inputs = Vec::new();
        for &(first, second) in pairs {
            let creatures = [first, second].map(|id| {
                current
                    .get(id)
                    .filter(|view| view.zone == ObjectZone::Battlefield && view.is_creature())
            });
            let [Some(a), Some(b)] = creatures else {
                continue;
            };
            for (dealer, target) in [(a, second), (b, first)] {
                let power = &dealer
                    .power_toughness
                    .as_ref()
                    .ok_or(EffectError::InconsistentProgram)?
                    .derived
                    .power;
                if power.is_positive() {
                    let source = DamageSource::Object {
                        object: self
                            .window
                            .cards()
                            .get(dealer.object)
                            .ok_or(EffectError::InconsistentProgram)?
                            .clone(),
                        controller: dealer.controller.ok_or(EffectError::InconsistentProgram)?,
                    };
                    inputs.push(DamageInput {
                        source,
                        target: DamageRecipient::Creature(target),
                        amount: power.clone(),
                    });
                }
            }
        }
        Ok(inputs)
    }

    pub(super) fn linked_creature_power_damage(
        &self,
        trample_overflow: bool,
    ) -> Result<Vec<DamageInput>, EffectError> {
        let Some((dealer, target_ref)) = self.legal_linked_target_refs()? else {
            return Ok(Vec::new());
        };
        let evaluated = self.evaluated_characteristics()?;
        let dealer_view = evaluated
            .get(dealer)
            .ok_or(EffectError::InconsistentProgram)?;
        let power = &dealer_view
            .power_toughness
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?
            .derived
            .power;
        if !power.is_positive() {
            return Ok(Vec::new());
        }
        let source = DamageSource::Object {
            object: self
                .window
                .cards()
                .get(dealer)
                .ok_or(EffectError::InconsistentProgram)?
                .clone(),
            controller: dealer_view
                .controller
                .ok_or(EffectError::InconsistentProgram)?,
        };
        match target_ref {
            TargetRef::Player(seat) => Ok(vec![DamageInput {
                source,
                target: DamageRecipient::Player(seat),
                amount: power.clone(),
            }]),
            TargetRef::Object(target) => {
                let target_view = evaluated
                    .get(target)
                    .ok_or(EffectError::InconsistentProgram)?;
                let Some(recipient) = target_view.damage_recipient() else {
                    return Ok(Vec::new());
                };
                let mut to_target = power.clone();
                let mut inputs = Vec::new();
                if trample_overflow
                    && target_view.is_creature()
                    && dealer_view.has_keyword(KeywordAbility::Trample)
                {
                    let marked = self
                        .damage
                        .get(&target)
                        .cloned()
                        .unwrap_or_else(|| 0.into());
                    let mut lethal = evaluated
                        .lethal_damage_threshold(target_view)
                        .ok_or(EffectError::InconsistentProgram)?
                        .minus(&marked)
                        .max(0.into());
                    if dealer_view.has_keyword(KeywordAbility::Deathtouch) {
                        lethal = lethal.min(1.into());
                    }
                    to_target = to_target.min(lethal);
                    let excess = power.minus(&to_target);
                    if excess.is_positive() {
                        let player = target_view
                            .controller
                            .ok_or(EffectError::InconsistentProgram)?;
                        inputs.push(DamageInput {
                            source: source.clone(),
                            target: DamageRecipient::Player(player),
                            amount: excess,
                        });
                    }
                }
                if to_target.is_positive() {
                    inputs.insert(
                        0,
                        DamageInput {
                            source,
                            target: recipient,
                            amount: to_target,
                        },
                    );
                }
                Ok(inputs)
            }
            TargetRef::Stack(_) => Ok(Vec::new()),
        }
    }

    pub(super) fn install_damage_batch(
        &mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
        journal: &mut DamageJournal,
    ) -> Result<(), EffectError> {
        self.validate_damage_origin_context(origin)?;
        match origin {
            DamageOrigin::ManaAbility { .. } | DamageOrigin::PreventionFollowup { .. } => {
                return Err(EffectError::WrongDamageFrame);
            }
            DamageOrigin::Resolution { entry, instruction } => {
                if kind != DamageKind::Noncombat {
                    return Err(EffectError::WrongPhase);
                }
                match &self.pending_work {
                    Some(PendingWork::Resolution(cursor))
                        if cursor.entry == entry && cursor.instruction == instruction => {}
                    Some(PendingWork::Resolution(_)) => return Err(EffectError::WrongDamageFrame),
                    _ => return Err(EffectError::WrongPhase),
                }
            }
            DamageOrigin::Combat { .. } => {
                if kind != DamageKind::Combat || self.pending_work.is_some() {
                    return Err(EffectError::WrongPhase);
                }
            }
        }
        let evaluated = self.evaluated_characteristics()?;
        let mut controllers = BTreeMap::new();
        for input in &inputs {
            controllers.insert(
                input.target,
                self.damage_recipient_controller_at(&evaluated, input.target)?,
            );
            if !self
                .window
                .participants()
                .contains(&input.source.controller())
            {
                return Err(EffectError::InconsistentProgram);
            }
        }
        let unpreventable = inputs
            .iter()
            .filter(|input| {
                self.damage_source_keywords(&evaluated, &input.source)
                    .is_some_and(|keywords| {
                        keywords.contains(&KeywordAbility::DamageCannotBePrevented)
                    })
            })
            .map(|input| input.source.id())
            .collect();
        let (mut batch, append) = self.damage_state.begin_batch(origin, kind, inputs)?;
        batch.disallow_prevention_from(&unpreventable);
        let static_no_prevent = evaluated.objects.values().any(|p| {
            p.zone == ObjectZone::Battlefield
                && p.printed_text
                    .continuous
                    .iter()
                    .chain(&p.printed_text.additional_battlefield_abilities)
                    .any(|ab| {
                        (p.abilities.continuous == Some(ab.id())
                            || p.abilities
                                .additional_battlefield_abilities
                                .contains(&ab.id()))
                            && matches!(
                                ab,
                                crate::characteristics::StaticContinuousAbility::DamageRule {
                                    rule: crate::characteristics::StaticDamageRule::DamageCannotBePrevented,
                                    ..
                                }
                            )
                    })
        }) || self.temporary_rules.iter().any(|active| {
            self.active_temporary_static(active, &evaluated)
                .is_some_and(|ab| {
                    matches!(
                        ab,
                        crate::characteristics::StaticContinuousAbility::DamageRule {
                            rule: crate::characteristics::StaticDamageRule::DamageCannotBePrevented,
                            ..
                        }
                    )
                })
        });
        if self.damage_state.prevention_disallowed_this_turn || static_no_prevent {
            batch.disallow_all_prevention();
        }
        batch.set_recipient_controllers(&controllers);
        let parent = match self.pending_work.take() {
            Some(PendingWork::Resolution(cursor)) => DamageParent::Resolution(cursor),
            None => DamageParent::Combat,
            _ => return Err(EffectError::WrongDamageFrame),
        };
        self.pending_work = Some(PendingWork::Damage(DamageWork {
            batch,
            parent,
            original_origin: origin,
            reflection: None,
            followup_tails: Vec::new(),
            followups: VecDeque::new(),
            results_committed: false,
            followup_zone: None,
        }));
        journal.append(append);
        Ok(())
    }
    /// Test-only direct mechanism fixture; production uses the shared driver.
    #[cfg(test)]
    pub(crate) fn begin_damage_batch(
        mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
        journals: (DamageJournal, DrawJournal),
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
    ) -> Result<RuleProgress, EffectError> {
        let (mut journal, draws) = journals;
        self.install_damage_batch(origin, kind, inputs, &mut journal)?;
        self.progress_rules(
            journal,
            declarations,
            assignments,
            draws,
            RulesJournal::default(),
        )
    }
    fn damage_recipient_controller_at(
        &self,
        evaluated: &EvaluatedCharacteristics,
        target: DamageRecipient,
    ) -> Result<SeatId, EffectError> {
        match target {
            DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id) => {
                if !evaluated.get(id).is_some_and(|view| match target {
                    DamageRecipient::Creature(_) => view.is_creature(),
                    DamageRecipient::Planeswalker(_) => view.has_type(CardType::Planeswalker),
                    DamageRecipient::Battle(_) => view.has_type(CardType::Battle),
                    DamageRecipient::Player(_) => false,
                }) {
                    return Err(EffectError::IllegalTarget);
                }
                self.window
                    .cards()
                    .get(id)
                    .filter(|card| card.zone() == ObjectZone::Battlefield)
                    .and_then(|c| c.controller())
                    .ok_or(EffectError::IllegalTarget)
            }
            DamageRecipient::Player(player) => {
                if !self.window.participants().contains(&player) || self.life.get(player).is_none()
                {
                    return Err(EffectError::IllegalTarget);
                }
                Ok(player)
            }
        }
    }
    fn application_recipient(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageRecipient, EffectError> {
        match application.scope {
            DamageScope::Recipient(recipient) => Ok(recipient),
            DamageScope::ControlledRecipients(player) => Ok(DamageRecipient::Player(player)),
            DamageScope::Component(id) => batch
                .component(id)
                .map(|c| c.attempt().target)
                .ok_or(EffectError::WrongDamageFrame),
        }
    }
    /// Fixed-recipient effects do not share capacity across objects. Process the
    /// next affected controller in APNAP order, then one independent recipient.
    fn next_damage_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> Result<Option<(SeatId, Vec<DamageApplication>)>, EffectError> {
        let mut applications = self.damage_state.applicable(batch);
        applications.extend(
            self.turn_prevention_applications(evaluated, batch)
                .into_keys(),
        );
        applications.extend(self.protection_applications(evaluated, batch).into_keys());
        applications.extend(
            self.static_prevention_applications(evaluated, batch)
                .into_keys(),
        );
        applications.extend(
            self.static_damage_modifications(evaluated, batch)?
                .into_keys(),
        );
        applications.extend(self.emblem_cap_applications(evaluated, batch).into_keys());
        if applications.is_empty() {
            return Ok(None);
        }
        for actor in self.window.apnap_players()? {
            let mut recipients = BTreeMap::<DamageRecipient, Vec<DamageApplication>>::new();
            let mut shared = Vec::new();
            for application in &applications {
                let recipient = self.application_recipient(batch, *application)?;
                if self.damage_recipient_controller_at(evaluated, recipient)? == actor {
                    if matches!(application.scope, DamageScope::ControlledRecipients(_)) {
                        shared.push(*application);
                    }
                    recipients.entry(recipient).or_default().push(*application);
                }
            }
            if !shared.is_empty() {
                let mut choices: BTreeSet<_> = shared.into_iter().collect();
                choices.extend(recipients.into_values().flatten());
                return Ok(Some((actor, choices.into_iter().collect())));
            }
            if let Some((_, applications)) = recipients.into_iter().next() {
                return Ok(Some((actor, applications)));
            }
        }
        Err(EffectError::WrongDamageFrame)
    }
    fn allocation_decision(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageDecision, EffectError> {
        let (shield, eligible, required) =
            self.damage_state.prevention_domain(batch, application)?;
        let recipient = self.application_recipient(batch, application)?;
        let chooser = self.damage_recipient_controller_at(evaluated, recipient)?;
        let eligible = eligible
            .into_iter()
            .map(|(attempt, amount)| {
                let component = batch
                    .component(attempt)
                    .ok_or(EffectError::WrongDamageFrame)?;
                Ok(DamageAllocationDomain {
                    attempt,
                    source: component.attempt().source.clone(),
                    target: component.attempt().target,
                    amount,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        Ok(DamageDecision::Allocation(PreventionAllocationChoice {
            revision: self.revision,
            batch: batch.id(),
            stage: batch.stage(),
            origin: batch.origin(),
            chooser,
            application,
            capacity: shield.remaining.clone(),
            required,
            eligible,
        }))
    }
    fn current_damage_decision(&self) -> Result<DamageDecision, EffectError> {
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        self.validate_damage_origin_context(batch.origin())?;
        let evaluated = self.evaluated_characteristics()?;
        match *batch.phase() {
            DamageBatchPhase::Allocating { application } => {
                self.allocation_decision(&evaluated, batch, application)
            }
            DamageBatchPhase::Selecting => {
                let (chooser, applications) = self
                    .next_damage_applications(&evaluated, batch)?
                    .ok_or(EffectError::WrongDamageFrame)?;
                if applications.len() < 2 {
                    return Err(EffectError::WrongDamageFrame);
                }
                Ok(DamageDecision::Order(DamageOrderChoice {
                    revision: self.revision,
                    batch: batch.id(),
                    stage: batch.stage(),
                    origin: batch.origin(),
                    chooser,
                    applications,
                }))
            }
        }
    }

    /// Admit an issued pre-result prevention choice whose parent can be resumed at
    /// the same instruction or combat step. Nested followups have separate cursors.
    pub(super) fn damage_departure_admitted(
        &self,
        issued: Option<&EffectDecision>,
    ) -> Result<bool, EffectError> {
        let (Some(PendingWork::Damage(work)), Some(EffectDecision::Damage(frame))) =
            (&self.pending_work, issued)
        else {
            return Ok(false);
        };
        Ok(matches!(
            work.parent,
            DamageParent::Resolution(_) | DamageParent::Combat
        ) && !work.results_committed
            && work.reflection.is_none()
            && work.followups.is_empty()
            && work.followup_tails.is_empty()
            && work.followup_zone.is_none()
            && self.current_damage_decision()? == *frame)
    }

    pub(super) fn refresh_damage_after_departure(&mut self) -> Result<(), EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let recipients = batch
            .components()
            .map(|component| component.attempt().target)
            .collect::<BTreeSet<_>>();
        let mut controllers = BTreeMap::new();
        for target in recipients {
            match self.damage_recipient_controller_at(&evaluated, target) {
                Ok(player) => {
                    controllers.insert(target, player);
                }
                Err(EffectError::IllegalTarget) => {}
                Err(error) => return Err(error),
            }
        }
        let Some(PendingWork::Damage(work)) = self.pending_work.as_mut() else {
            return Err(EffectError::WrongDamageFrame);
        };
        work.batch.reconcile_departure(&controllers)?;
        self.damage_state
            .refresh_prevention_after_departure(&mut work.batch)?;
        Ok(())
    }

    pub(super) fn resume_damage(
        &self,
        expected: u64,
        frame: &DamageDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        if matches!(
            self.pending_work,
            Some(PendingWork::ManaColor(_) | PendingWork::Combat(_))
        ) {
            return Err(EffectError::WrongPhase);
        }
        let revision = self.next_revision(expected)?;
        if frame.revision() != self.revision || &self.current_damage_decision()? != frame {
            return Err(EffectError::WrongDamageFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if let Some(DamageOrigin::Combat { position }) = self.original_damage_origin() {
            let turn = turn.ok_or(EffectError::WrongPhase)?;
            self.require_damage_work(turn)?;
            if turn.position() != position {
                return Err(EffectError::WrongDamageFrame);
            }
        }
        let mut next = self.clone();
        next.revision = revision;
        let mut journal = DamageJournal::default();
        let mut append = RulesJournal::default();
        match (frame, reply) {
            (DamageDecision::Order(bound), DecisionReply::DamageOrder(application)) => {
                if !bound.applications.contains(&application) {
                    return Err(DamageError::InapplicableEffect.into());
                }
                match application.effect {
                    DamageEffectId::UntilEndOfTurn { .. } => {
                        let before_rem = next.pending_damage_total();
                        next.apply_turn_prevention(application, &mut append)?;
                        let prevented = before_rem.minus(&next.pending_damage_total());
                        next.record_resolution_damage_prevented(&prevented);
                    }
                    DamageEffectId::StaticPrevention(_)
                    | DamageEffectId::StaticPreventionWithExile(_, _)
                    | DamageEffectId::CombatDamageToMill(_, _)
                    | DamageEffectId::RemovePlusOneToPreventDamage(_, _)
                    | DamageEffectId::ShieldCounter(_) => {
                        next.apply_static_prevention(application, &mut append)?
                    }
                    DamageEffectId::StaticHalfPrevention { .. }
                    | DamageEffectId::StaticDouble(_)
                    | DamageEffectId::StaticModify(_, _)
                    | DamageEffectId::TemporaryStaticModify(_)
                    | DamageEffectId::TemporaryPrevention(_)
                    | DamageEffectId::DoubleDamageFromEquippedCreature(_)
                    | DamageEffectId::DoubleDamageToEquippedCreature(_) => {
                        next.apply_static_damage_modification(application, &mut append)?
                    }
                    DamageEffectId::EmblemCap { .. } => {
                        next.apply_emblem_cap(application, &mut append)?
                    }
                    DamageEffectId::ProtectionMulticolored
                    | DamageEffectId::ProtectionSubtype(_)
                    | DamageEffectId::ProtectionFrom(_)
                    | DamageEffectId::Protection(_) => {
                        let before_rem = next.pending_damage_total();
                        next.prevent_by_protection(application, &mut journal, &mut append)?;
                        let prevented = before_rem.minus(&next.pending_damage_total());
                        next.record_resolution_damage_prevented(&prevented);
                    }
                    DamageEffectId::Multiplier(_) => {
                        next.multiply_damage_at_cut(application, &mut journal, &mut append)?
                    }
                    DamageEffectId::Prevention(_) => {
                        let Some(PendingWork::Damage(work)) = &mut next.pending_work else {
                            return Err(EffectError::WrongDamageFrame);
                        };
                        next.damage_state
                            .select_prevention(&mut work.batch, application)?;
                    }
                }
            }
            (DamageDecision::Allocation(bound), DecisionReply::AllocatePrevention(allocations)) => {
                next.prevent_damage_at_cut(
                    bound.application,
                    allocations,
                    &mut journal,
                    &mut append,
                )?;
            }
            _ => return Err(EffectError::WrongDecisionKind),
        }
        next.progress_rules(journal, vec![], vec![], DrawJournal::default(), append)
    }
    pub(super) fn progress_damage(
        &mut self,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<DamageProgressStep, EffectError> {
        loop {
            if matches!(&self.pending_work, Some(PendingWork::Damage(work)) if work.results_committed)
            {
                return self.progress_damage_followups(journal, append);
            }
            let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
            self.validate_damage_origin_context(batch.origin())?;
            if let DamageBatchPhase::Allocating { application } = *batch.phase() {
                if let Some(allocations) =
                    self.damage_state.forced_allocation(batch, application)?
                {
                    self.prevent_damage_at_cut(application, allocations, journal, append)?;
                    continue;
                }
                let evaluated = self.evaluated_characteristics()?;
                let frame = self.allocation_decision(&evaluated, batch, application)?;
                return Ok(DamageProgressStep::Pending(DamagePending::Damage(frame)));
            }
            let evaluated = self.evaluated_characteristics()?;
            let Some((chooser, applications)) = self.next_damage_applications(&evaluated, batch)?
            else {
                return self.commit_damage_results(journal, append);
            };
            if applications.len() > 1 {
                return Ok(DamageProgressStep::Pending(DamagePending::Damage(
                    DamageDecision::Order(DamageOrderChoice {
                        revision: self.revision,
                        batch: batch.id(),
                        stage: batch.stage(),
                        origin: batch.origin(),
                        chooser,
                        applications,
                    }),
                )));
            }
            let application = *applications.first().ok_or(EffectError::WrongDamageFrame)?;
            match application.effect {
                DamageEffectId::UntilEndOfTurn { .. } => {
                    let before_rem = self.pending_damage_total();
                    self.apply_turn_prevention(application, append)?;
                    let prevented = before_rem.minus(&self.pending_damage_total());
                    self.record_resolution_damage_prevented(&prevented);
                }
                DamageEffectId::StaticPrevention(_)
                | DamageEffectId::StaticPreventionWithExile(_, _)
                | DamageEffectId::CombatDamageToMill(_, _)
                | DamageEffectId::RemovePlusOneToPreventDamage(_, _)
                | DamageEffectId::ShieldCounter(_) => {
                    self.apply_static_prevention(application, append)?
                }
                DamageEffectId::StaticHalfPrevention { .. }
                | DamageEffectId::StaticDouble(_)
                | DamageEffectId::StaticModify(_, _)
                | DamageEffectId::TemporaryStaticModify(_)
                | DamageEffectId::TemporaryPrevention(_)
                | DamageEffectId::DoubleDamageFromEquippedCreature(_)
                | DamageEffectId::DoubleDamageToEquippedCreature(_) => {
                    self.apply_static_damage_modification(application, append)?
                }
                DamageEffectId::EmblemCap { .. } => self.apply_emblem_cap(application, append)?,
                DamageEffectId::ProtectionMulticolored
                | DamageEffectId::ProtectionSubtype(_)
                | DamageEffectId::ProtectionFrom(_)
                | DamageEffectId::Protection(_) => {
                    let before_rem = self.pending_damage_total();
                    self.prevent_by_protection(application, journal, append)?;
                    let prevented = before_rem.minus(&self.pending_damage_total());
                    self.record_resolution_damage_prevented(&prevented);
                }
                DamageEffectId::Multiplier(_) => {
                    self.multiply_damage_at_cut(application, journal, append)?
                }
                DamageEffectId::Prevention(_) => {
                    let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
                        return Err(EffectError::WrongDamageFrame);
                    };
                    self.damage_state
                        .select_prevention(&mut work.batch, application)?;
                }
            }
        }
    }
    fn damage_source_keywords<'a>(
        &'a self,
        evaluated: &'a EvaluatedCharacteristics,
        source: &DamageSource,
    ) -> Option<&'a BTreeSet<KeywordAbility>> {
        if let Some(reflection) = self.current_damage_reflection(source) {
            return Some(&reflection.keywords);
        }
        match source {
            DamageSource::Emblem { .. } => None,
            DamageSource::Object { object, .. } => evaluated
                .get(object.id())
                .map(|view| &view.keywords)
                .or_else(|| {
                    self.departed_damage_sources
                        .get(&object.id())
                        .map(|source| &source.keywords)
                }),
            DamageSource::SpellCopy { entry, .. } => match self.payloads.get(entry)? {
                StackPayload::Instructions(payload) => payload
                    .copiable_traits
                    .as_ref()
                    .map(|traits| &traits.keywords),
                StackPayload::Permanent => None,
            },
        }
    }
    /// Compute all simultaneous contributions before publishing their common post-result total.
    pub(super) fn apply_poison(
        &mut self,
        player: SeatId,
        contributions: Vec<crate::occurrence::PoisonContribution>,
    ) -> RulesOccurrence {
        let before = self.poison_counters(player);
        let after = contributions
            .iter()
            .fold(before.clone(), |total, c| total.plus(&c.amount));
        self.poison.insert(player, after.clone());
        RulesOccurrence::PlayerPoisoned {
            player,
            before,
            after,
            contributions,
        }
    }

    fn commit_damage_results(
        &mut self,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<DamageProgressStep, EffectError> {
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let origin = batch.origin();
        let kind = batch.kind();
        let id = batch.id();
        self.validate_damage_origin_context(origin)?;
        let (applied, completions) = self.damage_state.finish_batch(batch)?;
        let exile_if_dies = match origin {
            DamageOrigin::Resolution { entry, instruction } => {
                match self
                    .payloads
                    .get(&entry)
                    .and_then(StackPayload::instructions)
                    .and_then(|body| body.get(instruction))
                {
                    Some(Instruction::DealDamage(spec)) => spec.exile_if_dies,
                    _ => false,
                }
            }
            _ => false,
        };
        let attacks_if_damaged = match origin {
            DamageOrigin::Resolution { entry, instruction } => matches!(
                self.payloads
                    .get(&entry)
                    .and_then(StackPayload::instructions)
                    .and_then(|body| body.get(instruction)),
                Some(Instruction::DamageTargetControllersCreatures {
                    require_damaged_creatures_to_attack: true,
                    ..
                })
            ),
            _ => false,
        };
        let before = self.rules_snapshot(None)?;
        let previous = before.view();
        if attacks_if_damaged {
            let creatures = applied
                .iter()
                .filter_map(|fact| match fact.target {
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id)
                        if previous
                            .evaluated
                            .get(id)
                            .is_some_and(|permanent| permanent.is_creature()) =>
                    {
                        Some(id)
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            for object in creatures {
                let count = self.creatures_required_to_attack.entry(object).or_default();
                *count = count
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentProgram)?;
            }
        }
        let mut creature_damage_pairs = BTreeSet::new();
        let mut marks = BTreeMap::<ObjectId, RulesInteger>::new();
        let mut losses = BTreeMap::<SeatId, RulesInteger>::new();
        let mut defense_losses = BTreeMap::<ObjectId, RulesInteger>::new();
        let mut loyalty_losses = BTreeMap::<ObjectId, RulesInteger>::new();
        let mut deathtouch = BTreeSet::new();
        let mut gains = BTreeMap::new();
        let mut minus_counters = BTreeMap::<ObjectId, RulesInteger>::new();
        let mut poison = BTreeMap::<SeatId, Vec<crate::occurrence::PoisonContribution>>::new();
        let mut converted = BTreeSet::new();
        let mut toxic_hits = BTreeSet::new();
        // Prepare and validate every result before the first physical write.
        for fact in &applied {
            self.damage_recipient_controller_at(&previous.evaluated, fact.target)?;
            let keywords = self.damage_source_keywords(&previous.evaluated, &fact.source);
            if self
                .window
                .participants()
                .contains(&fact.source.controller())
                && keywords.is_some_and(|keys| keys.contains(&KeywordAbility::Lifelink))
            {
                gains
                    .entry((fact.source.id(), fact.source.controller()))
                    .or_insert_with(Vec::new)
                    .push(fact);
            }
            let infect = keywords.is_some_and(|keys| keys.contains(&KeywordAbility::Infect));
            match fact.target {
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => {
                    let target = previous
                        .evaluated
                        .get(id)
                        .ok_or(EffectError::IllegalTarget)?;
                    if target.has_type(CardType::Battle) {
                        let total = defense_losses.entry(id).or_insert_with(|| 0.into());
                        *total = total.plus(&fact.amount);
                    }
                    if target.has_type(CardType::Planeswalker) {
                        let total = loyalty_losses.entry(id).or_insert_with(|| 0.into());
                        *total = total.plus(&fact.amount);
                    }
                    if !target.is_creature() {
                        continue;
                    }
                    if self.characteristics.creature_damage_pairs.is_some()
                        && let crate::damage::DamageSourceId::Object(source) = fact.source.id()
                    {
                        creature_damage_pairs.insert((source, id));
                    }
                    if keywords.is_some_and(|keys| keys.contains(&KeywordAbility::Deathtouch)) {
                        deathtouch.insert(id);
                    }
                    let amounts = if infect
                        || keywords.is_some_and(|keys| keys.contains(&KeywordAbility::Wither))
                    {
                        converted.insert(fact.attempt);
                        &mut minus_counters
                    } else {
                        &mut marks
                    };
                    let added = if converted.contains(&fact.attempt)
                        && !matches!(origin, DamageOrigin::Combat { .. })
                    {
                        fact.amount
                            .times(&super::permanent_counters::effect_counter_multiplier(
                                &previous.evaluated,
                                id,
                            ))
                    } else {
                        fact.amount.clone()
                    };
                    let total = amounts.entry(id).or_insert_with(|| 0.into());
                    *total = total.plus(&added);
                }
                DamageRecipient::Player(player) => {
                    if self.track_combat_prices
                        && kind == DamageKind::Combat
                        && let DamageSource::Object { object, controller } = &fact.source
                        && let Some(source) = previous.evaluated.get(object.id())
                        && let Some(history) = &mut self.cast_history
                    {
                        history
                            .combat_types
                            .entry(*controller)
                            .or_default()
                            .extend(&source.current_creature_subtypes);
                        let assassin = self
                            .creature_vocabulary
                            .resolve_label("Assassin")
                            .is_some_and(|id| source.current_creature_subtypes.contains(&id));
                        let commander = object
                            .card_id()
                            .is_some_and(|id| self.commanders.contains_key(&id));
                        if source.is_creature() && (assassin || commander) {
                            history.freerunning.insert(*controller);
                        }
                    }
                    let contribution = |origin, amount| crate::occurrence::PoisonContribution {
                        origin,
                        source: fact.source.id(),
                        controller: fact.source.controller(),
                        amount,
                    };
                    if infect {
                        converted.insert(fact.attempt);
                        poison.entry(player).or_default().push(contribution(
                            crate::occurrence::PoisonOrigin::Infect(fact.attempt),
                            fact.amount.clone(),
                        ));
                    } else {
                        let amount = losses.entry(player).or_insert_with(|| 0.into());
                        *amount = amount.plus(&fact.amount);
                    }
                    if kind == DamageKind::Combat
                        && let DamageSource::Object { object, .. } = &fact.source
                        && let Some(value) = previous
                            .evaluated
                            .get(object.id())
                            .and_then(|view| view.toxic_value.as_ref())
                        && value.is_positive()
                        && toxic_hits.insert((fact.source.id(), player))
                    {
                        poison.entry(player).or_default().push(contribution(
                            crate::occurrence::PoisonOrigin::Toxic(fact.attempt),
                            value.clone(),
                        ));
                    }
                }
            }
        }
        if exile_if_dies && let DamageOrigin::Resolution { entry, instruction } = origin {
            for fact in &applied {
                if !fact.amount.is_positive() {
                    continue;
                }
                if let DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) = fact.target
                {
                    self.damage_exile_until_cleanup
                        .entry(id)
                        .or_default()
                        .insert((entry, instruction));
                }
            }
        }
        if kind == DamageKind::Combat && !self.commanders.is_empty() {
            for fact in &applied {
                if let DamageRecipient::Player(player) = fact.target {
                    self.record_commander_damage(&fact.source, player, &fact.amount);
                }
            }
        }
        if !applied.is_empty() {
            let historical = before.view();
            let events = applied
                .iter()
                .map(|damage| {
                    let subtypes =
                        if let Some(reflection) = self.current_damage_reflection(&damage.source) {
                            Some(&reflection.creature_types)
                        } else {
                            match damage.source.id() {
                                crate::damage::DamageSourceId::Object(object) => historical
                                    .evaluated
                                    .get(object)
                                    .map(|source| &source.current_creature_subtypes)
                                    .or_else(|| {
                                        self.departed_damage_sources
                                            .get(&object)
                                            .map(|source| &source.creature_subtypes)
                                    }),
                                crate::damage::DamageSourceId::SpellCopy(entry) => {
                                    self.stack_copiable_creature_subtypes(entry)
                                }
                                crate::damage::DamageSourceId::Emblem(_) => None,
                            }
                        };
                    RulesOccurrence::DamageDealt {
                        damage: damage.clone(),
                        kind,
                        origin,
                        source_subtypes: subtypes.cloned().unwrap_or_default(),
                    }
                })
                .collect();
            if let Some(pairs) = &mut self.characteristics.creature_damage_pairs {
                pairs.extend(creature_damage_pairs);
            }
            self.emit_from(
                EventCause::DamageDealt { batch: id },
                &before,
                None,
                append,
                |_, _, _| Ok(events),
            )?;
        }
        let prohibited = self.life_gain_prohibitions(&before.view().evaluated)?;
        let frozen_life = self.life_total_cant_change_players(&before.view().evaluated);
        gains
            .retain(|(_, player), _| !prohibited.contains(player) && !frozen_life.contains(player));
        // Enforce StaticRestriction::ControllerLifeTotalCantChange and StaticDamageRule::MinimumLifeTotal.
        for (&player, loss) in &mut losses {
            if !loss.is_positive() {
                continue;
            }
            if frozen_life.contains(&player) {
                *loss = 0.into();
                continue;
            }
            let Some(current_life) = self.life.get(player) else {
                continue;
            };
            let mut highest_floor: Option<RulesInteger> = None;
            for active in &self.temporary_rules {
                if active.controller != player {
                    continue;
                }
                if let Some(ability) =
                    self.active_temporary_static(active, &before.view().evaluated)
                    && let crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::MinimumLifeTotal {
                                floor,
                                require_creature,
                            },
                        ..
                    } = ability
                {
                    let creature_ok = !*require_creature
                        || before.view().evaluated.objects.values().any(|c| {
                            c.zone == ObjectZone::Battlefield
                                && c.controller == Some(player)
                                && c.is_creature()
                        });
                    if creature_ok {
                        let f = RulesInteger::from(i64::from(*floor));
                        if highest_floor.as_ref().is_none_or(|cur| f > *cur) {
                            highest_floor = Some(f);
                        }
                    }
                }
            }
            for perm in before
                .view()
                .evaluated
                .objects
                .values()
                .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(player))
            {
                for ability in perm
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&perm.printed_text.additional_battlefield_abilities)
                {
                    let active = perm.abilities.continuous == Some(ability.id())
                        || perm
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id());
                    if !active {
                        continue;
                    }
                    if let crate::characteristics::StaticContinuousAbility::DamageRule {
                        rule:
                            crate::characteristics::StaticDamageRule::MinimumLifeTotal {
                                floor,
                                require_creature,
                            },
                        ..
                    } = ability
                    {
                        let creature_ok = !*require_creature
                            || before.view().evaluated.objects.values().any(|c| {
                                c.zone == ObjectZone::Battlefield
                                    && c.controller == Some(player)
                                    && c.is_creature()
                            });
                        if creature_ok {
                            let f = RulesInteger::from(i64::from(*floor));
                            if highest_floor.as_ref().is_none_or(|cur| f > *cur) {
                                highest_floor = Some(f);
                            }
                        }
                    }
                }
            }
            if let Some(floor) = highest_floor {
                if *current_life <= floor {
                    *loss = 0.into();
                } else {
                    let max_loss = current_life.minus(&floor);
                    if *loss > max_loss {
                        *loss = max_loss;
                    }
                }
            }
        }
        losses.retain(|_, loss| loss.is_positive());
        let life_changes = self
            .life
            .apply_damage_changes(&losses, id, &gains)
            .map_err(|error| match error {
                LifeError::UnknownPlayer => EffectError::IllegalTarget,
                LifeError::InvalidLoss | LifeError::InvalidAdjustment => {
                    EffectError::Damage(DamageError::InvalidAmount)
                }
            })?;
        let mut defense_results = Vec::new();
        for (object, amount) in defense_losses {
            let before = self.counters_of_kind(object, CounterKind::Defense);
            let after = if amount >= before {
                0.into()
            } else {
                before.minus(&amount)
            };
            if before != after {
                self.characteristics.defense.insert(object, after.clone());
                defense_results.push(RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind: CounterKind::Defense,
                    before,
                    after,
                });
            }
        }
        let mut loyalty_results = Vec::new();
        for (&object, amount) in &loyalty_losses {
            let before = self
                .characteristics
                .loyalty
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into());
            let after = if amount >= &before {
                0.into()
            } else {
                before.minus(amount)
            };
            if after.is_positive() {
                self.characteristics.loyalty.insert(object, after.clone());
            } else {
                self.characteristics.loyalty.remove(&object);
            }
            if before != after {
                loyalty_results.push(RulesOccurrence::LoyaltyCountersChanged {
                    object,
                    before,
                    after,
                });
            }
        }
        self.deathtouch_damage.extend(deathtouch);
        for (&object, amount) in &marks {
            let previous = self
                .damage
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into());
            self.damage.insert(object, previous.plus(amount));
        }
        let mut poison_results = Vec::new();
        for (player, contributions) in poison {
            if !self.player_counter_prevented(
                &before.view().evaluated,
                player,
                CounterKind::Poison,
            )? {
                poison_results.push(self.apply_poison(player, contributions));
            }
        }
        let mut applied_minus_counters = false;
        for (&object, amount) in &minus_counters {
            if self.permanent_counter_prevented(
                &before.view().evaluated,
                object,
                CounterKind::MinusOne,
            ) {
                continue;
            }
            self.characteristics.place_counters(
                object,
                &PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: amount.clone(),
                },
            )?;
            applied_minus_counters = true;
        }
        if !marks.is_empty()
            || !losses.is_empty()
            || !defense_results.is_empty()
            || !loyalty_results.is_empty()
            || applied_minus_counters
            || !poison_results.is_empty()
            || !life_changes.is_empty()
        {
            self.emit_from(
                EventCause::DamageResults { batch: id },
                &before,
                None,
                append,
                |previous, current, _| {
                    let mut values: Vec<_> = life_changes
                        .iter()
                        .cloned()
                        .map(RulesOccurrence::LifeChanged)
                        .collect();
                    values.extend(poison_results);
                    values.extend(loyalty_results);
                    values.extend(defense_results);
                    for (object, amount) in minus_counters {
                        let before_appearance = previous
                            .appearance(object)
                            .ok_or(EffectError::InconsistentProgram)?;
                        let after_appearance = current
                            .appearance(object)
                            .ok_or(EffectError::InconsistentProgram)?;
                        values.push(RulesOccurrence::CountersPlaced {
                            object,
                            before: previous.characteristics.counters(object),
                            after: current.characteristics.counters(object),
                            added: PowerToughnessCounters {
                                plus_one: 0.into(),
                                minus_one: amount,
                            },
                            before_appearance: Box::new(before_appearance),
                            after_appearance: Box::new(after_appearance),
                        });
                    }
                    for &object in marks.keys() {
                        let recipient = DamageRecipient::Creature(object);
                        values.push(RulesOccurrence::DamageResult {
                            recipient,
                            before: previous
                                .marks
                                .get(&object)
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                            after: current
                                .marks
                                .get(&object)
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                            components: applied
                                .iter()
                                .filter(|fact| {
                                    matches!(fact.target, DamageRecipient::Creature(id) | DamageRecipient::Planeswalker(id) | DamageRecipient::Battle(id) if id == object) && !converted.contains(&fact.attempt)
                                })
                                .map(|fact| fact.attempt)
                                .collect(),
                        });
                    }
                    for player in current
                        .window
                        .apnap_players()?
                        .filter(|player| losses.contains_key(player))
                    {
                        let recipient = DamageRecipient::Player(player);
                        values.push(RulesOccurrence::DamageResult {
                            recipient,
                            before: previous
                                .life
                                .get(player)
                                .ok_or(EffectError::InconsistentProgram)?
                                .clone(),
                            after: current
                                .life
                                .get(player)
                                .ok_or(EffectError::InconsistentProgram)?
                                .clone(),
                            components: applied
                                .iter()
                                .filter(|fact| {
                                    fact.target == recipient && !converted.contains(&fact.attempt)
                                })
                                .map(|fact| fact.attempt)
                                .collect(),
                        });
                    }
                    Ok(values)
                },
            )?;
        }
        let any_dealt = applied.iter().any(|fact| fact.amount.is_positive());
        journal.applied.extend(applied);
        journal.completions.extend(completions);
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        if any_dealt && let DamageParent::Resolution(cursor) = &mut work.parent {
            cursor.results.dealt_damage_this_resolution = true;
        }
        work.results_committed = true;
        self.progress_damage_followups(journal, append)
    }

    fn progress_damage_followups(
        &mut self,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<DamageProgressStep, EffectError> {
        use super::zone_replacement::{AuthorizedZoneBatch, ZoneMoveIntent};
        use crate::occurrence::ZoneReason;

        loop {
            let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
                return Err(EffectError::WrongDamageFrame);
            };
            let batch = work.batch.id();
            let cause = EventCause::DamageAdjusted { batch };
            if let Some(zone) = &mut work.followup_zone {
                if let Some(decision) = zone.batch.progress(self.revision)? {
                    let EffectDecision::ZoneReplacement(frame) = decision else {
                        return Err(EffectError::WrongDamageFrame);
                    };
                    return Ok(DamageProgressStep::Pending(DamagePending::Zone(frame)));
                }
                if let Some(frame) = zone.graveyard_frame(self.revision)? {
                    return Ok(DamageProgressStep::Pending(DamagePending::GraveyardOrder(
                        frame,
                    )));
                }
                let zone = work
                    .followup_zone
                    .take()
                    .ok_or(EffectError::WrongDamageFrame)?;
                let before = self.rules_snapshot(None)?;
                let reason = zone
                    .batch
                    .intents()
                    .first()
                    .ok_or(EffectError::WrongDamageFrame)?
                    .reason;
                let mut movements = zone
                    .batch
                    .intents()
                    .iter()
                    .map(|intent| {
                        let destination = zone
                            .batch
                            .destination(intent.before)?
                            .physical(None)
                            .ok_or(EffectError::WrongDamageFrame)?;
                        Ok((intent.before, destination))
                    })
                    .collect::<Result<Vec<_>, EffectError>>()?;
                if let Some(order) = zone.graveyard_order {
                    movements
                        .retain(|(_, destination)| *destination != ObjectDestination::Graveyard);
                    movements.extend(
                        order
                            .into_iter()
                            .rev()
                            .map(|id| (id, ObjectDestination::Graveyard)),
                    );
                }
                let (window, results) = self.window.record_linked_return_moves(&movements)?;
                self.window = window;
                self.emit_from(
                    cause,
                    &before,
                    None,
                    append,
                    |before, after, controllers| {
                        results
                            .iter()
                            .map(|movement| {
                                super::occurrence_flow::zone_occurrence_at(
                                    before,
                                    after,
                                    movement,
                                    reason,
                                    controllers,
                                )
                            })
                            .collect()
                    },
                )?;
                continue;
            }
            let Some(followup) = work.followups.pop_front() else {
                if let Some(tail) = work.followup_tails.pop() {
                    work.followups = tail;
                    continue;
                }
                break;
            };
            let (player, amount, mill) = match followup {
                DamageFollowup::RemovePlusOneCounter { object, count } => {
                    let previous = self.characteristics.counters(object);
                    if previous.plus_one.is_positive() && count.is_positive() {
                        let take = if count > previous.plus_one {
                            previous.plus_one.clone()
                        } else {
                            count
                        };
                        let before = self.rules_snapshot(None)?;
                        let after = self.characteristics.take_power_toughness_counters(
                            object,
                            &PowerToughnessCounters {
                                plus_one: take,
                                minus_one: 0.into(),
                            },
                        )?;
                        self.emit_from(cause, &before, None, append, |_, _, _| {
                            Ok(vec![RulesOccurrence::PowerToughnessCountersRemoved {
                                object,
                                before: previous,
                                after,
                            }])
                        })?;
                    }
                    continue;
                }
                DamageFollowup::RemoveShieldCounter(object) => {
                    let previous = self.counters_of_kind(object, CounterKind::Shield);
                    if previous.is_positive() {
                        let before = self.rules_snapshot(None)?;
                        let after = previous.minus(&1.into());
                        if let Some(map) = self
                            .characteristics
                            .named_permanent_counters_mut(CounterKind::Shield)
                        {
                            if after.is_positive() {
                                map.insert(object, after.clone());
                            } else {
                                map.remove(&object);
                            }
                        }
                        self.emit_from(cause, &before, None, append, |_, _, _| {
                            Ok(vec![RulesOccurrence::PermanentCounterChanged {
                                object,
                                kind: CounterKind::Shield,
                                before: previous,
                                after,
                            }])
                        })?;
                    }
                    continue;
                }
                DamageFollowup::PutCounters {
                    target,
                    kind,
                    count,
                } => {
                    if count.is_positive() {
                        match target {
                            DamageRecipient::Creature(object)
                            | DamageRecipient::Planeswalker(object)
                            | DamageRecipient::Battle(object)
                                if kind.is_permanent() =>
                            {
                                if self
                                    .window
                                    .cards()
                                    .get(object)
                                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                                {
                                    self.place_divided_counters(
                                        kind,
                                        vec![(object, count)],
                                        cause,
                                        append,
                                    )?;
                                    if kind.keyword_counter_ability().is_some()
                                        && self.counters_of_kind(object, kind).is_positive()
                                    {
                                        self.characteristics.refresh_keyword_counter_grant(
                                            StackEntryId::entry_replacement_creator(),
                                            object,
                                            kind,
                                        )?;
                                    }
                                }
                            }
                            DamageRecipient::Player(player)
                                if !kind.is_permanent()
                                    && self.window.participants().contains(&player) =>
                            {
                                let before = self.rules_snapshot(None)?;
                                if !self.player_counter_prevented(
                                    &before.view().evaluated,
                                    player,
                                    kind,
                                )? {
                                    let prev = self.player_counters_of_kind(player, kind);
                                    let after = prev.plus(&count);
                                    match kind {
                                        CounterKind::Poison => {
                                            self.poison.insert(player, after.clone());
                                        }
                                        CounterKind::Experience => {
                                            self.characteristics
                                                .experience
                                                .insert(player, after.clone());
                                        }
                                        other => {
                                            self.characteristics
                                                .player_counters
                                                .entry(other)
                                                .or_default()
                                                .insert(player, after.clone());
                                        }
                                    }
                                    self.emit_from(cause, &before, None, append, |_, _, _| {
                                        Ok(vec![RulesOccurrence::PlayerCounterChanged {
                                            player,
                                            kind,
                                            before: prev,
                                            after,
                                        }])
                                    })?;
                                }
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                DamageFollowup::RemoveCounters {
                    object,
                    kind,
                    count,
                } => {
                    if count.is_positive()
                        && self
                            .window
                            .cards()
                            .get(object)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        if matches!(kind, CounterKind::PlusOne | CounterKind::MinusOne) {
                            let previous = self.characteristics.counters(object);
                            let available = if kind == CounterKind::PlusOne {
                                &previous.plus_one
                            } else {
                                &previous.minus_one
                            };
                            if available.is_positive() {
                                let take = count.min(available.clone());
                                let removed = if kind == CounterKind::PlusOne {
                                    PowerToughnessCounters {
                                        plus_one: take,
                                        minus_one: 0.into(),
                                    }
                                } else {
                                    PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: take,
                                    }
                                };
                                let before = self.rules_snapshot(None)?;
                                let after = self
                                    .characteristics
                                    .take_power_toughness_counters(object, &removed)?;
                                self.emit_from(cause, &before, None, append, |_, _, _| {
                                    Ok(vec![RulesOccurrence::PowerToughnessCountersRemoved {
                                        object,
                                        before: previous,
                                        after,
                                    }])
                                })?;
                            }
                        } else {
                            let previous = self.counters_of_kind(object, kind);
                            if previous.is_positive() {
                                let take = count.min(previous.clone());
                                let after = previous.minus(&take);
                                let before = self.rules_snapshot(None)?;
                                let updated = if let Some(counts) =
                                    self.characteristics.named_permanent_counters_mut(kind)
                                {
                                    if after.is_positive() {
                                        counts.insert(object, after.clone());
                                    } else {
                                        counts.remove(&object);
                                        if kind == CounterKind::Vow {
                                            self.characteristics
                                                .vow_protected_players
                                                .remove(&object);
                                        }
                                        if kind.keyword_counter_ability().is_some() {
                                            self.characteristics
                                                .clear_keyword_counter_grant(object, kind);
                                        }
                                    }
                                    true
                                } else {
                                    false
                                };
                                if updated {
                                    let occ = match kind {
                                        CounterKind::Time => RulesOccurrence::TimeCounterRemoved {
                                            object,
                                            before: previous,
                                            after,
                                        },
                                        CounterKind::Fade => RulesOccurrence::FadeCounterRemoved {
                                            object,
                                            before: previous,
                                            after,
                                        },
                                        CounterKind::Loyalty => {
                                            RulesOccurrence::LoyaltyCountersChanged {
                                                object,
                                                before: previous,
                                                after,
                                            }
                                        }
                                        CounterKind::Charge => {
                                            RulesOccurrence::ChargeCountersChanged {
                                                object,
                                                before: previous,
                                                after,
                                            }
                                        }
                                        CounterKind::Storage => {
                                            RulesOccurrence::StorageCountersChanged {
                                                object,
                                                before: previous,
                                                after,
                                            }
                                        }
                                        CounterKind::Suspect => {
                                            RulesOccurrence::SuspectCountersChanged {
                                                object,
                                                before: previous,
                                                after,
                                            }
                                        }
                                        _ => RulesOccurrence::PermanentCounterChanged {
                                            object,
                                            kind,
                                            before: previous,
                                            after,
                                        },
                                    };
                                    self.emit_from(cause, &before, None, append, |_, _, _| {
                                        Ok(vec![occ])
                                    })?;
                                }
                            }
                        }
                    }
                    continue;
                }
                DamageFollowup::InlineBody {
                    replacement,
                    prevented,
                } => {
                    self.execute_inline_replacement_body_with_amount(
                        &replacement,
                        Some(prevented),
                        cause,
                        append,
                    )?;
                    continue;
                }
                DamageFollowup::LibraryTop {
                    player,
                    amount,
                    mill,
                } => (player, amount, mill),
                DamageFollowup::DealDamage {
                    batch,
                    creator,
                    amount,
                    reflection,
                } => {
                    let evaluated = self.evaluated_characteristics()?;
                    let recipient = match reflection.target {
                        TargetRef::Player(player)
                            if self.window.participants().contains(&player) =>
                        {
                            Some(DamageRecipient::Player(player))
                        }
                        TargetRef::Object(object) => evaluated
                            .get(object)
                            .filter(|view| view.zone == ObjectZone::Battlefield)
                            .and_then(|view| view.damage_recipient()),
                        _ => None,
                    };
                    let Some(target) = recipient else {
                        continue;
                    };
                    let controller = self.damage_recipient_controller_at(&evaluated, target)?;
                    let origin = DamageOrigin::PreventionFollowup { batch, creator };
                    let damage = DamageInput {
                        source: reflection.source.clone(),
                        target,
                        amount,
                    };
                    let (mut batch, facts) = self.damage_state.begin_batch(
                        origin,
                        DamageKind::Noncombat,
                        vec![damage],
                    )?;
                    batch.set_recipient_controllers(&BTreeMap::from([(target, controller)]));
                    if reflection
                        .keywords
                        .contains(&KeywordAbility::DamageCannotBePrevented)
                    {
                        batch.disallow_prevention_from(&BTreeSet::from([reflection.source.id()]));
                    }
                    let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
                        return Err(EffectError::WrongDamageFrame);
                    };
                    if !work.followups.is_empty() {
                        work.followup_tails
                            .push(std::mem::take(&mut work.followups));
                    }
                    work.batch = batch;
                    work.reflection = Some(reflection);
                    work.results_committed = false;
                    journal.append(facts);
                    return Ok(DamageProgressStep::NextBatch);
                }
            };
            let library = self.window.cards().library(player);
            let count = amount
                .min(RulesInteger::from_usize(library.len()))
                .to_usize()
                .ok_or(EffectError::WrongDamageFrame)?;
            if count == 0 {
                continue;
            }
            let (action, reason) = if mill {
                (
                    crate::program::ZoneActionKind::EffectMill,
                    ZoneReason::Mill { player },
                )
            } else {
                (
                    crate::program::ZoneActionKind::EffectExile,
                    ZoneReason::ExiledByEffect,
                )
            };
            let intents = library[..count]
                .iter()
                .map(|&before| ZoneMoveIntent { before, reason })
                .collect();
            let zone = self.begin_zone_replacements(AuthorizedZoneBatch {
                cause,
                action,
                intents,
            })?;
            let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
                return Err(EffectError::WrongDamageFrame);
            };
            work.followup_zone = Some(Box::new(DamageZoneFollowup {
                batch: zone,
                graveyard_order: None,
            }));
        }
        self.finish_damage_parent()
    }

    pub(super) fn resume_damage_zone_followup(
        &self,
        expected: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::ZoneReplacement(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let Some(PendingWork::Damage(DamageWork {
            followup_zone: Some(zone),
            results_committed: true,
            ..
        })) = &mut next.pending_work
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        zone.batch.accept(self.revision, frame, actor, reply)?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    pub(super) fn resume_damage_graveyard_order(
        &self,
        expected: u64,
        frame: &PileChoiceFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::OrderResolutionGraveyard(order) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let Some(PendingWork::Damage(DamageWork {
            followup_zone: Some(zone),
            results_committed: true,
            ..
        })) = &mut next.pending_work
        else {
            return Err(EffectError::WrongDamageFrame);
        };
        if zone.graveyard_frame(expected)?.as_ref() != Some(frame) || actor != frame.chooser() {
            return Err(EffectError::WrongResolutionFrame);
        }
        frame.validate_graveyard_order(&order)?;
        zone.graveyard_order = Some(order);
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    fn finish_damage_parent(&mut self) -> Result<DamageProgressStep, EffectError> {
        let Some(PendingWork::Damage(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongDamageFrame);
        };
        match work.parent {
            DamageParent::Resolution(mut cursor) => {
                cursor.instruction = cursor
                    .instruction
                    .checked_add(1)
                    .ok_or(EffectError::WrongDamageFrame)?;
                self.pending_work = Some(PendingWork::Resolution(cursor));
                Ok(DamageProgressStep::ResolutionContinued)
            }
            DamageParent::Combat => {
                self.pending_work = None;
                let DamageOrigin::Combat { position } = work.original_origin else {
                    return Err(EffectError::WrongDamageFrame);
                };
                Ok(DamageProgressStep::CombatComplete {
                    additional_step_required: self.additional_damage_required(position),
                })
            }
            DamageParent::Mana(parent) => {
                if let Some(frame) = self.schedule_triggered_mana(parent.clone()) {
                    return Ok(DamageProgressStep::ManaChoice(frame));
                }
                self.pending_work = match parent {
                    ManaParent::Priority | ManaParent::Casting(_) => None,
                    ManaParent::CombatPayment(cursor) => Some(PendingWork::Combat(*cursor)),
                    ManaParent::Recovery(frame, _) => {
                        Some(PendingWork::CastRecovery(Box::new(frame)))
                    }
                    ManaParent::Payment(work) => {
                        Some(PendingWork::ResolutionPayment(Box::new(work)))
                    }
                };
                Ok(DamageProgressStep::ManaComplete)
            }
        }
    }
}
