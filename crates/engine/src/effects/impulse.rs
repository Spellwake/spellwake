//! Replacement-aware exile of top-library cards during resolution.
use super::*;
use crate::occurrence::{EventCause, ZoneReason};
use crate::program::{PublicZoneDestination, ZoneActionKind};

impl EffectWindow {
    fn impulse_batch(
        &self,
        cursor: &ResolutionCursor,
        objects: &[ObjectId],
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action: ZoneActionKind::EffectExile,
            intents: objects
                .iter()
                .map(|object| zone_replacement::ZoneMoveIntent {
                    before: *object,
                    reason: ZoneReason::ExiledByEffect,
                })
                .collect(),
        })
    }
    pub(super) fn begin_exile_top_for_play(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        count: usize,
    ) -> Result<bool, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        if cursor.entry != entry.id() || cursor.instruction != instruction {
            return Err(EffectError::InconsistentProgram);
        }
        let (owners, face_down_viewer, play_permission) = match self.current_instruction()? {
            Instruction::ExileTopPlay {
                library_owner,
                face_down,
                grant_play,
                ..
            } => (
                self.selection_players(*library_owner, entry.controller(), Some(&cursor))?,
                (*face_down).then_some(entry.controller()),
                (*grant_play).then_some(entry.controller()),
            ),
            _ => (vec![entry.controller()], None, Some(entry.controller())),
        };
        let mut objects = Vec::new();
        for owner in owners {
            objects.extend(
                self.window
                    .cards()
                    .library(owner)
                    .iter()
                    .take(count)
                    .copied(),
            );
        }
        if objects.is_empty() {
            if matches!(
                self.current_instruction()?,
                Instruction::ExileTopPlay { .. }
            ) && let Some(cursor) = self.resolution_cursor_mut()
            {
                cursor.results.last_moved_objects.clear();
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
            }
            return Ok(false);
        }
        let batch = self.impulse_batch(&cursor, &objects)?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ImpulseReplacement(Box::new(
            ImpulseReplacementWork {
                cursor,
                objects,
                play_permission,
                face_down_viewer,
                graveyard_orders: BTreeMap::new(),
                batch,
            },
        )));
        Ok(true)
    }
    /// Whether this exact private ordering frame belongs to the pending face-up top-card exile.
    pub fn is_pending_exile_graveyard_order(&self, frame: &LinkedReturnOrderFrame) -> bool {
        matches!(&self.pending_work, Some(PendingWork::ImpulseReplacement(work))
            if self.impulse_graveyard_order(work).ok().flatten().as_ref() == Some(frame))
    }
    pub(super) fn impulse_graveyard_order(
        &self,
        work: &ImpulseReplacementWork,
    ) -> Result<Option<LinkedReturnOrderFrame>, EffectError> {
        for chooser in self.window.apnap_players()? {
            if work.graveyard_orders.contains_key(&chooser) {
                continue;
            }
            let objects: Vec<_> = work
                .objects
                .iter()
                .copied()
                .filter(|id| {
                    work.batch.destination(*id).ok() == Some(PublicZoneDestination::Graveyard)
                        && self
                            .window
                            .cards()
                            .get(*id)
                            .is_some_and(|object| object.owner() == chooser)
                })
                .collect();
            if objects.len() > 1 {
                return Ok(Some(LinkedReturnOrderFrame {
                    revision: self.revision,
                    chooser,
                    objects,
                    destination: PublicZoneDestination::Graveyard,
                }));
            }
        }
        Ok(None)
    }
    pub(super) fn progress_impulse_replacement(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::ImpulseReplacement(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::ImpulseReplacement(work));
            return Ok(Some(frame));
        }
        if let Some(frame) = self.impulse_graveyard_order(&work)? {
            self.pending_work = Some(PendingWork::ImpulseReplacement(work));
            return Ok(Some(EffectDecision::LinkedReturnOrder(frame)));
        }
        let fresh = self.impulse_batch(&work.cursor, &work.objects)?;
        work.batch.verify_initial(fresh)?;
        let mut moves = Vec::new();
        let mut ordered = work
            .objects
            .iter()
            .copied()
            .filter(|id| work.batch.destination(*id).ok() != Some(PublicZoneDestination::Graveyard))
            .collect::<Vec<_>>();
        for player in self.window.apnap_players()? {
            if let Some(order) = work.graveyard_orders.get(&player) {
                ordered.extend(order.iter().rev().copied());
            } else {
                ordered.extend(work.objects.iter().copied().filter(|id| {
                    work.batch.destination(*id).ok() == Some(PublicZoneDestination::Graveyard)
                        && self
                            .window
                            .cards()
                            .get(*id)
                            .is_some_and(|card| card.owner() == player)
                }));
            }
        }
        for object in &ordered {
            let destination = work.batch.destination(*object)?;
            let card = self
                .window
                .cards()
                .get(*object)
                .ok_or(EffectError::UnknownObject)?;
            if destination == PublicZoneDestination::Library {
                continue;
            }
            if destination == PublicZoneDestination::Battlefield {
                return Err(EffectError::InconsistentProgram);
            }
            let physical = destination
                .physical(Some(card.owner()))
                .ok_or(EffectError::InconsistentProgram)?;
            moves.push((*object, physical));
        }
        let before = self.rules_snapshot(None)?;
        let movements = if moves.is_empty() {
            Vec::new()
        } else {
            let player = self
                .window
                .resolving()
                .filter(|entry| entry.id() == work.cursor.entry)
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            let multi_owner = work.objects.first().is_some_and(|first| {
                let first_owner = self.window.cards().get(*first).map(GameObject::owner);
                work.objects
                    .iter()
                    .skip(1)
                    .any(|id| self.window.cards().get(*id).map(GameObject::owner) != first_owner)
            });
            let (window, movements) = if multi_owner {
                self.window.record_linked_return_moves(&moves)?
            } else {
                {
                    let owner = self
                        .window
                        .cards()
                        .get(work.objects[0])
                        .ok_or(EffectError::UnknownObject)?
                        .owner();
                    self.window.record_library_search_moves(
                        self.window.revision(),
                        work.cursor.entry,
                        owner,
                        player,
                        &moves,
                    )?
                }
            };
            self.window = window;
            movements
        };
        if let Some(viewer) = work.face_down_viewer {
            for movement in &movements {
                if movement.after.zone() == ObjectZone::Exile {
                    self.face_down_exile.insert(movement.after.id(), viewer);
                }
            }
        }
        if let Some(player) = work.play_permission {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let exile_top_play = match self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
            {
                Some(Instruction::ExileTopPlay {
                    duration,
                    cast_only,
                    free,
                    any_color,
                    ..
                }) => Some((*duration, *cast_only, *free, *any_color)),
                _ => None,
            };
            let exile_source = match entry.kind() {
                StackKind::TriggeredAbility { source } | StackKind::ActivatedAbility { source } => {
                    Some(source.id())
                }
                _ => None,
            };
            for movement in &movements {
                if movement.after.zone() == ObjectZone::Exile {
                    let permission = (movement.after.id(), player);
                    if let Some((duration, cast_only, free, any_color_mana)) = exile_top_play {
                        if cast_only
                            && self
                                .copiable_program(movement.after.id())
                                .is_some_and(|program| program.has_type(CardType::Land))
                        {
                            continue;
                        }
                        if duration == crate::program::ImpulsePlayDuration::UntilNextFromSource
                            && let Some(source) = exile_source
                        {
                            self.exile_play_until_next.insert(source, permission);
                            if free {
                                self.free_exiled_cards.insert(permission);
                            }
                            continue;
                        }
                        let mapped_duration = self.map_impulse_play_duration(duration, player);
                        self.card_play_permissions.insert(CardPlayPermission {
                            zone: CardPlayZone::Exile,
                            object: permission.0,
                            player,
                            duration: mapped_duration,
                            any_color: any_color_mana,
                            free,
                            cast_only,
                        });
                        if free {
                            self.free_exiled_cards.insert(permission);
                        }
                    } else {
                        self.card_play_permissions.insert(CardPlayPermission {
                            zone: CardPlayZone::Exile,
                            object: permission.0,
                            player,
                            duration: CardPlayDuration::ThroughTurn,
                            any_color: false,
                            free: false,
                            cast_only: false,
                        });
                    }
                }
            }
        } else {
            work.cursor.impulse.started = true;
            work.cursor.impulse.exiled = movements
                .iter()
                .find(|movement| movement.after.zone() == ObjectZone::Exile)
                .map(|movement| movement.after.id());
        }
        if matches!(
            self.payloads
                .get(&work.cursor.entry)
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction)),
            Some(Instruction::ExileTopPlay { .. })
        ) {
            work.cursor.results.last_moved_objects = movements
                .iter()
                .map(|movement| movement.after.id())
                .collect();
            work.cursor.results.last_action_succeeded = !movements.is_empty();
            work.cursor.results.last_action_count = Some(RulesInteger::from_usize(movements.len()));
        }
        if !movements.is_empty() {
            self.emit_from(
                EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                &before,
                None,
                append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(
                                previous,
                                current,
                                movement,
                                ZoneReason::ExiledByEffect,
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
        }
        let operation = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(work.cursor.instruction));
        if work.play_permission.is_some()
            || matches!(operation, Some(Instruction::ExileTopPlay { .. }))
        {
            work.cursor.instruction = work
                .cursor
                .instruction
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        Ok(None)
    }
}
