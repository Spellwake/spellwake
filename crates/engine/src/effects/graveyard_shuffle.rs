//! Replacement-aware movement into a library followed by a private shuffle.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::{ResolvingPlayer, ZoneActionKind},
    targets::TargetLegality,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraveyardShuffleWork {
    pub(super) cursor: ResolutionCursor,
    pub(super) batch: ZoneReplacementBatch,
    player: SeatId,
    remaining_players: std::collections::VecDeque<SeatId>,
    collective: bool,
    ability_source: Option<ObjectId>,
    objects: Vec<ObjectId>,
    ready: bool,
    continue_reveals: bool,
}
impl EffectWindow {
    pub(super) fn begin_graveyard_shuffle(&mut self) -> Result<(), EffectError> {
        let player = self.resolving_player(ResolvingPlayer::Controller)?;
        self.begin_shuffle(player, false)
    }

    pub(super) fn begin_ability_source_shuffle(
        &mut self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<(), EffectError> {
        let source = match entry.kind() {
            StackKind::ActivatedAbility { source } => source.id(),
            StackKind::TriggeredAbility { source } => source.id(),
            _ => return Err(EffectError::InconsistentProgram),
        };
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .results
            .shuffled_source_owner = None;
        let Some(object) = self.window.cards().get(source).filter(|object| {
            object.zone() == ObjectZone::Battlefield
                && self.window.cards().phased_out_under(source).is_none()
        }) else {
            return self.advance_instruction();
        };
        self.begin_shuffle_with_cards(object.owner(), false, Some(vec![source]), Some(source))
    }

    pub(super) fn begin_targeted_graveyard_shuffle(&mut self) -> Result<(), EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(primary) = assessment
            .get(TargetId(0))
            .filter(|target| target.legality == TargetLegality::Legal)
        else {
            return self.advance_instruction();
        };
        let TargetRef::Player(player) = primary.choice.target else {
            return Err(EffectError::InconsistentProgram);
        };
        let cards = assessment
            .results()
            .iter()
            .filter(|row| {
                row.choice.clause == TargetClauseId(1) && row.legality == TargetLegality::Legal
            })
            .map(|row| match row.choice.target {
                TargetRef::Object(id) => Ok(id),
                _ => Err(EffectError::InconsistentProgram),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.begin_shuffle_with_cards(player, false, Some(cards), None)
    }

    pub(super) fn begin_shuffle(
        &mut self,
        player: SeatId,
        continue_reveals: bool,
    ) -> Result<(), EffectError> {
        self.begin_shuffle_with_cards(player, continue_reveals, None, None)
    }

    fn begin_shuffle_with_cards(
        &mut self,
        player: SeatId,
        continue_reveals: bool,
        targets: Option<Vec<ObjectId>>,
        ability_source: Option<ObjectId>,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        let collective = self.current_instruction()? == &Instruction::ShuffleEachHandAndGraveyard;
        let mut remaining_players = std::collections::VecDeque::new();
        let player = if collective {
            remaining_players = self.window.apnap_players()?.collect();
            remaining_players
                .pop_front()
                .ok_or(EffectError::WrongResolutionFrame)?
        } else {
            player
        };
        let objects = if let Some(targets) = targets {
            targets
        } else if collective {
            self.window
                .cards()
                .iter()
                .filter(|card| {
                    matches!(card.zone(), ObjectZone::Hand(_) | ObjectZone::Graveyard(_))
                        && card.card_id().is_some()
                })
                .map(GameObject::id)
                .collect()
        } else if continue_reveals || self.current_instruction()? == &Instruction::ShuffleOwnLibrary
        {
            vec![]
        } else {
            self.window.cards().graveyard(player).to_vec()
        };
        let batch = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action: shuffle_action(ability_source),
            intents: objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason: shuffle_reason(collective, ability_source),
                })
                .collect(),
        })?;
        if !matches!(self.pending_work, Some(PendingWork::Resolution(_))) {
            return Err(EffectError::WrongResolutionFrame);
        }
        self.pending_work = Some(PendingWork::GraveyardShuffle(Box::new(
            GraveyardShuffleWork {
                cursor,
                batch,
                player,
                remaining_players,
                collective,
                ability_source,
                objects,
                ready: false,
                continue_reveals,
            },
        )));
        Ok(())
    }
    pub(super) fn graveyard_shuffle_frame(
        &self,
        work: &GraveyardShuffleWork,
    ) -> Option<EffectDecision> {
        if !work.ready {
            return work
                .batch
                .frame(self.revision)
                .map(EffectDecision::ZoneReplacement);
        }
        Some(EffectDecision::LibraryTop(LibraryTopFrame::search(
            self.revision,
            work.cursor.entry,
            work.cursor.instruction,
            work.player,
            vec![],
            vec![],
            library_top::SearchFrameMode {
                exile_batch: false,
                exile_owner: None,
                split: None,
                hand_graveyard: false,
                battlefield_batch: false,
                hand_limit: None,
                shared_land_type: false,
                forest_and_plains: false,
                required: false,
                entry_colors: BTreeMap::new(),
                entry_reveals: BTreeMap::new(),
            },
        )))
    }
    pub(super) fn progress_graveyard_shuffle(
        &mut self,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::GraveyardShuffle(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::GraveyardShuffle(work));
            return Ok(Some(frame));
        }
        work.ready = true;
        let frame = self.graveyard_shuffle_frame(&work);
        self.pending_work = Some(PendingWork::GraveyardShuffle(work));
        Ok(frame)
    }
    pub(crate) fn graveyard_shuffle_pending(&self) -> bool {
        matches!(self.pending_work, Some(PendingWork::GraveyardShuffle(_)))
    }
    pub(crate) fn graveyard_shuffle_ready(&self) -> bool {
        matches!(&self.pending_work, Some(PendingWork::GraveyardShuffle(work)) if work.ready)
    }
    pub(crate) fn preflight_graveyard_shuffle(
        &self,
        tape: &EntropyTape,
    ) -> Result<(), crate::shuffle::EntropyError> {
        let Some(PendingWork::GraveyardShuffle(work)) = &self.pending_work else {
            return Ok(());
        };
        let mut candidates = self.window.cards().library(work.player).to_vec();
        tape.shuffled(candidates.clone())?;
        for id in work.objects.iter().filter(|id| {
            self.window
                .cards()
                .get(**id)
                .is_some_and(|card| card.owner() == work.player && card.card_id().is_some())
        }) {
            candidates.push(*id);
            tape.shuffled(candidates.clone())?;
        }
        Ok(())
    }
    pub(crate) fn finish_graveyard_shuffle(
        mut self,
        tape: &EntropyTape,
    ) -> Result<(RuleProgress, EntropyTape, ShuffleRecord), crate::game::GameError> {
        let Some(PendingWork::GraveyardShuffle(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame.into());
        };
        if !work.ready {
            return Err(EffectError::WrongResolutionFrame.into());
        }
        let cause = EventCause::Instruction {
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
        };
        let collective = work.collective;
        let ability_source = work.ability_source;
        let reason = shuffle_reason(collective, ability_source);
        let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: cause.clone(),
            action: shuffle_action(ability_source),
            intents: work
                .objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        })?;
        work.batch.verify_initial(fresh)?;
        let moves = work
            .objects
            .iter()
            .map(|id| {
                Ok((
                    *id,
                    work.batch
                        .destination(*id)?
                        .physical(None)
                        .ok_or(EffectError::InconsistentProgram)?,
                ))
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let before = self.rules_snapshot(None)?;
        let movements = if moves.is_empty() {
            Vec::new()
        } else {
            let (window, movements) = self
                .window
                .record_library_shuffle_moves(
                    work.cursor.entry,
                    work.player,
                    collective,
                    ability_source,
                    &moves,
                )
                .map_err(EffectError::from)?;
            self.window = window;
            movements
        };

        let mut append = RulesJournal::default();
        if !movements.is_empty() {
            self.emit_from(
                cause,
                &before,
                None,
                &mut append,
                |old, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(old, current, movement, reason, controllers)
                        })
                        .collect()
                },
            )?;
        }
        let (window, entropy, shuffle) = self
            .window
            .search_shuffle(work.player, tape)
            .map_err(EffectError::from)?;
        self.window = window;
        if let Some(player) = work.remaining_players.pop_front() {
            work.player = player;
            work.objects.clear();
            work.batch = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
                cause: EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                action: ZoneActionKind::EffectShuffleGraveyard,
                intents: vec![],
            })?;
            self.pending_work = Some(PendingWork::GraveyardShuffle(work));
            let progress = self.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                append,
            )?;
            return Ok((progress, entropy, shuffle));
        }
        if ability_source.is_some() {
            // Payment starts when the exact source is present. A destination replacement
            // can modify the payment without invalidating the following instruction.
            work.cursor.results.shuffled_source_owner = Some(work.player);
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        if work.continue_reveals {
            let state = &mut self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse;
            if let Some(rewrite) = state.rewrite.as_mut() {
                if rewrite.owners.pop_front() != Some(work.player) {
                    return Err(EffectError::InconsistentProgram.into());
                }
            } else {
                state.transformed_controllers.pop_front();
                state.shuffle_after_reveal = false;
            }
        } else {
            self.advance_instruction()?;
        }
        Ok((
            self.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                append,
            )?,
            entropy,
            shuffle,
        ))
    }
}

fn shuffle_reason(collective: bool, source: Option<ObjectId>) -> ZoneReason {
    if source.is_some() {
        ZoneReason::ShufflePermanentIntoLibrary
    } else if collective {
        ZoneReason::ShuffleHandAndGraveyardIntoLibrary
    } else {
        ZoneReason::ShuffleGraveyardIntoLibrary
    }
}

fn shuffle_action(source: Option<ObjectId>) -> ZoneActionKind {
    if source.is_some() {
        ZoneActionKind::EffectPutIntoLibrary
    } else {
        ZoneActionKind::EffectShuffleGraveyard
    }
}
