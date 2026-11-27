//! One replacement-aware, simultaneous exile of all physical graveyard cards.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::ZoneActionKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraveyardExileWork {
    pub(super) cursor: ResolutionCursor,
    pub(super) batch: ZoneReplacementBatch,
    objects: Vec<ObjectId>,
}
impl EffectWindow {
    pub(super) fn progress_graveyard_exile(
        &mut self,
        journal: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::GraveyardExile(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::GraveyardExile(work));
            return Ok(Some(frame));
        }
        let cause = EventCause::Instruction {
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
        };
        let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: cause.clone(),
            action: ZoneActionKind::EffectExile,
            intents: work
                .objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason: ZoneReason::ExiledByEffect,
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
        let (window, movements) = self
            .window
            .record_graveyard_exile_moves(work.cursor.entry, &moves)?;
        self.window = window;
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        if !movements.is_empty() {
            self.emit_from(
                cause,
                &before,
                None,
                journal,
                |old, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(
                                old,
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
        self.advance_instruction()?;
        Ok(None)
    }
}
