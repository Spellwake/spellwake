//! Replacement-aware exile of top-library cards during resolution.
use super::*;
use crate::occurrence::{EventCause, ZoneReason};
use crate::program::{PublicZoneDestination, ZoneActionKind};
use crate::stack_window::StackEntry;

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
    pub(super) fn progress_nonland_search(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
    ) -> Result<Option<usize>, EffectError> {
        let state = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .impulse
            .nonland_search
            .get_or_insert_with(|| {
                Box::new(NonlandSearch {
                    phase: NonlandSearchPhase::Exile,
                    entered_hand: 0,
                })
            });
        match state.phase {
            NonlandSearchPhase::Done => Ok(Some(state.entered_hand)),
            NonlandSearchPhase::ToHand { object, .. } => {
                if !self.begin_exile_cohort_to_hand(vec![object])? {
                    return Err(EffectError::InconsistentProgram);
                }
                Ok(None)
            }
            NonlandSearchPhase::Exile => {
                let count = state.entered_hand;
                if self.window.cards().library(entry.controller()).is_empty() {
                    return Ok(Some(count));
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .impulse
                    .started = false;
                self.begin_impulse(entry, index, false)?;
                Ok(None)
            }
        }
    }
    pub(super) fn begin_impulse(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        face_down: bool,
    ) -> Result<bool, EffectError> {
        self.begin_player_impulse(entry, instruction, entry.controller(), face_down)
    }
    pub(super) fn begin_player_impulse(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        player: SeatId,
        face_down: bool,
    ) -> Result<bool, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        if cursor.impulse.started || cursor.entry != entry.id() || cursor.instruction != instruction
        {
            return Err(EffectError::InconsistentProgram);
        }
        let Some(object) = self.window.cards().library(player).first().copied() else {
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse
                .started = true;
            return Ok(false);
        };
        let objects = vec![object];
        let batch = self.impulse_batch(&cursor, &objects)?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ImpulseReplacement(Box::new(
            ImpulseReplacementWork {
                cursor,
                objects,
                play_permission: None,
                face_down_viewer: face_down.then_some(entry.controller()),
                bottom_from_exile: false,
                return_to_hand: false,
                graveyard_orders: BTreeMap::new(),
                batch,
            },
        )));
        Ok(true)
    }
    pub(super) fn progress_permanent_exile_loop(
        &mut self,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let work = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if let Some(card) = work.impulse.repeated_permanent {
            let entered = self
                .window
                .cards()
                .current_identity(card)
                .and_then(|id| self.window.cards().get(id))
                .is_some_and(|object| object.zone() == ObjectZone::Battlefield);
            let impulse = &mut self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse;
            impulse.repeated_permanent = None;
            impulse.exiled = None;
            impulse.decided = false;
            impulse.started = false;
            if !entered {
                self.advance_instruction()?;
            }
            return Ok(());
        }
        if !work.impulse.started {
            if !self.begin_impulse(entry, index, false)? {
                self.advance_instruction()?;
            }
            return Ok(());
        }
        let Some(exiled) = work.impulse.exiled.filter(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|object| object.zone() == ObjectZone::Exile)
                && self
                    .copiable_program(*id)
                    .is_some_and(|program| program.permanent().is_some())
        }) else {
            return self.advance_instruction();
        };
        if work.impulse.decided {
            let card = self
                .window
                .cards()
                .get(exiled)
                .and_then(GameObject::card_id)
                .ok_or(EffectError::InconsistentProgram)?;
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse
                .repeated_permanent = Some(card);
            return self.begin_linked_cohort_return(
                LinkedReturnOrigin::ChosenExile {
                    entry: entry.id(),
                    instruction: index,
                    controller: Some(entry.controller()),
                },
                vec![exiled],
                append,
            );
        }
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
            cursor: work.clone(),
            chooser: entry.controller(),
            options: [OptionalEffectChoice::Take, OptionalEffectChoice::Decline].into(),
            rebound_or_buyback: false,
            cast: None,
        })));
        Ok(())
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
            Instruction::ExileRelatedTopForPlay | Instruction::ExileRelatedTopForCastThisTurn => (
                vec![self.resolving_player(crate::program::ResolvingPlayer::UniqueRelatedPlayer)?],
                None,
                Some(entry.controller()),
            ),
            Instruction::ExileTargetUntilNonlandForFreeCast => (
                vec![self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?],
                None,
                Some(entry.controller()),
            ),
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
                bottom_from_exile: false,
                return_to_hand: false,
                graveyard_orders: BTreeMap::new(),
                batch,
            },
        )));
        Ok(true)
    }
    pub(super) fn begin_linked_exile_to_hand(
        &mut self,
        object: ObjectId,
    ) -> Result<bool, EffectError> {
        self.begin_exile_cohort_to_hand(vec![object])
    }
    pub(super) fn exiled_search_results(&self) -> Result<Vec<ObjectId>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .id();
        let source = self
            .execution_bindings(entry)
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?
            .source
            .object
            .id();
        Ok(self
            .linked_search_results
            .get(&source)
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .is_some_and(|card| card.zone() == ObjectZone::Exile)
            })
            .collect())
    }
    pub(super) fn begin_exile_cohort_to_hand(
        &mut self,
        mut objects: Vec<ObjectId>,
    ) -> Result<bool, EffectError> {
        objects.retain(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|card| card.zone() == ObjectZone::Exile)
        });
        self.begin_public_cards_to_hand(objects)
    }
    pub(super) fn begin_public_cards_to_hand(
        &mut self,
        objects: Vec<ObjectId>,
    ) -> Result<bool, EffectError> {
        if objects.is_empty() {
            return Ok(false);
        }
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        let batch = self.linked_exile_hand_batch(&cursor, &objects)?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ImpulseReplacement(Box::new(
            ImpulseReplacementWork {
                cursor,
                objects,
                play_permission: None,
                face_down_viewer: None,
                bottom_from_exile: false,
                return_to_hand: true,
                graveyard_orders: BTreeMap::new(),
                batch,
            },
        )));
        Ok(true)
    }
    fn linked_exile_hand_batch(
        &self,
        cursor: &ResolutionCursor,
        objects: &[ObjectId],
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action: ZoneActionKind::EffectReturnSelected,
            intents: objects
                .iter()
                .map(|object| zone_replacement::ZoneMoveIntent {
                    before: *object,
                    reason: ZoneReason::ReturnToOwnersHand,
                })
                .collect(),
        })
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
        let fresh = if work.bottom_from_exile {
            self.rewrite_bottom_batch(&work.cursor, &work.objects)?
        } else if work.return_to_hand {
            self.linked_exile_hand_batch(&work.cursor, &work.objects)?
        } else {
            self.impulse_batch(&work.cursor, &work.objects)?
        };
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
            if destination == PublicZoneDestination::Library
                && !work.return_to_hand
                && !work.bottom_from_exile
            {
                continue;
            }
            if (work.return_to_hand || work.bottom_from_exile)
                && matches!(
                    (card.zone(), destination),
                    (ObjectZone::Exile, PublicZoneDestination::Exile)
                        | (ObjectZone::Graveyard(_), PublicZoneDestination::Graveyard)
                )
            {
                continue;
            }
            if destination == PublicZoneDestination::Battlefield {
                return Err(EffectError::InconsistentProgram);
            }
            let physical = destination
                .physical(Some(card.owner()))
                .ok_or(EffectError::InconsistentProgram)?;
            let physical =
                if work.bottom_from_exile && destination == PublicZoneDestination::Library {
                    ObjectDestination::LibraryBottom
                } else {
                    physical
                };
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
            let (window, movements) =
                if work.return_to_hand || work.bottom_from_exile || multi_owner {
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
            let per_type = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                .is_some_and(|op| matches!(op, Instruction::ExileTopForEachSpellType(_)));
            if per_type {
                self.typed_exile_cohorts.insert(
                    entry.id(),
                    TypedExileCohort {
                        player,
                        cards: movements
                            .iter()
                            .filter(|m| m.after.zone() == ObjectZone::Exile)
                            .map(|m| m.after.id())
                            .collect(),
                        used: BTreeSet::new(),
                    },
                );
            }
            let free_play = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                == Some(&Instruction::ExileTopForFreePlay);
            let free_noncreature = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                .is_some_and(|op| matches!(op, Instruction::ExileTopFreeNoncreatureSpells(_)));
            let temporary_cast = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                == Some(&Instruction::ExileRelatedTopForCastThisTurn);
            let any_color = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                == Some(&Instruction::ExileRelatedTopForPlay);
            let linked_source = if matches!(
                self.payloads
                    .get(&entry.id())
                    .and_then(StackPayload::instructions)
                    .and_then(|ops| ops.get(work.cursor.instruction)),
                Some(Instruction::ExileTopForPlay {
                    until_next_from_source: true
                })
            ) {
                let StackKind::TriggeredAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                Some(source.id())
            } else {
                None
            };
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
            let until_nonland = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(work.cursor.instruction))
                == Some(&Instruction::ExileTargetUntilNonlandForFreeCast);
            for movement in &movements {
                if movement.after.zone() == ObjectZone::Exile {
                    let permission = (movement.after.id(), player);
                    if per_type {
                        continue;
                    }
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
                        let mapped_duration = match duration {
                            crate::program::ImpulsePlayDuration::ThisTurn => {
                                CardPlayDuration::ThroughTurn
                            }
                            crate::program::ImpulsePlayDuration::UntilEndOfNextTurn => {
                                if self.window.active_player() == player {
                                    CardPlayDuration::ThroughNextOwnTurnSkipCurrent(player)
                                } else {
                                    CardPlayDuration::ThroughNextOwnTurn(player)
                                }
                            }
                            crate::program::ImpulsePlayDuration::WhileExiled
                            | crate::program::ImpulsePlayDuration::UntilNextFromSource => {
                                CardPlayDuration::WhileInZone
                            }
                        };
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
                    } else if until_nonland {
                        if self
                            .copiable_program(movement.after.id())
                            .is_some_and(|program| !program.has_type(CardType::Land))
                        {
                            self.card_play_permissions.insert(CardPlayPermission {
                                zone: CardPlayZone::Exile,
                                object: permission.0,
                                player,
                                duration: CardPlayDuration::ThroughTurn,
                                any_color: false,
                                free: true,
                                cast_only: true,
                            });
                            self.free_exiled_cards.insert(permission);
                        }
                    } else if free_play {
                        self.card_play_permissions.insert(CardPlayPermission {
                            zone: CardPlayZone::Exile,
                            object: permission.0,
                            player,
                            duration: CardPlayDuration::ThroughTurn,
                            any_color: false,
                            free: true,
                            cast_only: false,
                        });
                        self.free_exiled_cards.insert(permission);
                    } else if free_noncreature {
                        self.free_exiled_noncreature_spells.insert(permission);
                    } else if temporary_cast {
                        self.temporary_any_color_exile_cast.insert(permission);
                    } else if any_color {
                        self.card_play_permissions.insert(CardPlayPermission {
                            zone: CardPlayZone::Exile,
                            object: permission.0,
                            player,
                            duration: CardPlayDuration::WhileInZone,
                            any_color: true,
                            free: false,
                            cast_only: false,
                        });
                    } else if let Some(source) = linked_source {
                        self.exile_play_until_next.insert(source, permission);
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
        } else if !work.return_to_hand && !work.bottom_from_exile {
            work.cursor.impulse.started = true;
            work.cursor.impulse.exiled = movements
                .iter()
                .find(|movement| movement.after.zone() == ObjectZone::Exile)
                .map(|movement| movement.after.id());
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
                                if work.bottom_from_exile {
                                    ZoneReason::ExiledCardsToLibraryBottom
                                } else if work.return_to_hand {
                                    ZoneReason::ReturnToOwnersHand
                                } else {
                                    ZoneReason::ExiledByEffect
                                },
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
        if operation == Some(&Instruction::ExileNonlandForFormerControllers) {
            let rewrite = work
                .cursor
                .impulse
                .rewrite
                .as_mut()
                .ok_or(EffectError::InconsistentProgram)?;
            if work.bottom_from_exile {
                rewrite.finish_player()?;
            } else {
                let player = *rewrite
                    .controllers
                    .front()
                    .ok_or(EffectError::InconsistentProgram)?;
                if let Some(movement) = movements
                    .iter()
                    .find(|movement| movement.after.zone() == ObjectZone::Exile)
                {
                    let id = movement.after.id();
                    if self
                        .copiable_program(id)
                        .ok_or(EffectError::InconsistentProgram)?
                        .has_type(CardType::Land)
                    {
                        rewrite.lands.push(id);
                    } else {
                        rewrite.spells.push((player, id));
                        rewrite.finished_exiling = true;
                    }
                } else if movements.is_empty() {
                    return Err(EffectError::InconsistentProgram);
                }
                work.cursor.impulse.started = false;
            }
        } else if let Some(Instruction::ExileNonlandsToHandAndDamage { repeat_at }) = operation {
            let state = work
                .cursor
                .impulse
                .nonland_search
                .as_mut()
                .ok_or(EffectError::InconsistentProgram)?;
            if work.return_to_hand {
                let NonlandSearchPhase::ToHand { object, repeat } = state.phase else {
                    return Err(EffectError::InconsistentProgram);
                };
                if movements.iter().any(|m| {
                    m.before.id() == object && matches!(m.after.zone(), ObjectZone::Hand(_))
                }) {
                    state.entered_hand = state
                        .entered_hand
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                }
                state.phase = if repeat {
                    NonlandSearchPhase::Exile
                } else {
                    NonlandSearchPhase::Done
                };
            } else if let Some(movement) = movements
                .iter()
                .find(|movement| movement.after.zone() == ObjectZone::Exile)
            {
                let program = self
                    .copiable_program(movement.after.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                if !program.has_type(CardType::Land) {
                    state.phase = NonlandSearchPhase::ToHand {
                        object: movement.after.id(),
                        repeat: program.mana_value() >= i64::from(*repeat_at).into(),
                    };
                }
            } else if movements.is_empty() {
                return Err(EffectError::InconsistentProgram);
            }
        } else if operation == Some(&Instruction::ExileTargetUntilNonlandForFreeCast) {
            if movements.is_empty() {
                return Err(EffectError::InconsistentProgram);
            }
            if movements.iter().any(|movement| {
                movement.after.zone() == ObjectZone::Exile
                    && self
                        .copiable_program(movement.after.id())
                        .is_some_and(|program| !program.has_type(CardType::Land))
            }) {
                work.cursor.instruction = work
                    .cursor
                    .instruction
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentProgram)?;
            }
        } else if work.play_permission.is_some()
            || work.return_to_hand
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
