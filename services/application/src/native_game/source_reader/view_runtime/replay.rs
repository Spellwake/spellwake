//! Authenticated participant replay frames. Completed native games and
//! operational no-contest prefixes are read from the published portable
//! archive (archived and canonically published first when absent). A game in
//! progress serves only the caller's own historical seat from a fresh original
//! native walk and never publishes a manifest.
use super::*;
use crate::{
    native_game::source_reader::{
        cuts::{CutSource, HistoryCut},
        history_page::{PageScope, cut_event_facts},
        replay_archive::{REPLAY_SCHEMA, ReplayTarget},
    },
    player_wire::{
        PlayerHistoryFacts, PlayerReplayFacts, PlayerReplayFrame, PlayerReplayMode,
        PlayerReplayRequest, PlayerReplaySchema, PlayerReplaySource,
    },
};

/// Replay-specific delivery result inside the common refusal envelope.
pub(crate) enum NativeReplayRead {
    /// Verified frame; the caller still renews the bearer before publication.
    Frame(Box<PlayerReplayFrame>),
    /// The mode or perspective is not permitted for this game's state.
    ModeUnavailable,
    /// The coordinate is not an accepted cut of this game.
    CoordinateUnavailable,
    /// Unknown, foreign, snapshot-purpose or other-disclosure history cursor.
    CursorUnavailable,
}
impl NativeGameRuntime {
    /// Compose one replay frame: the exact snapshot at the requested coordinate
    /// plus a history page that never scans past it. Historical-seat mode is the
    /// caller's own seat; completed-full-information may select any retained
    /// seat only after actual native and SQL completion. An operational
    /// no-contest allows only its own-seat published prefix; a game in progress
    /// only its own-seat live history. Access is renewed after composition; the
    /// HTTP caller renews the bearer afterwards.
    ///
    /// # Errors
    /// Build/publication, manifest/archive verification, release drift, native
    /// walk, scan/cursor/page and frame validation failures. No partial frame.
    pub(crate) fn replay_frame(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        request: &PlayerReplayRequest,
    ) -> Result<NativeReadOutcome<NativeReplayRead>, NativeSourceError> {
        let scope = PlayerViewRequest {
            game_id: request.game_id,
            context_id: request.context_id,
        };
        scope.validate().map_err(|_| invalid())?;
        let account = actor.account().ok_or_else(invalid)?;
        let refused = |reason| Ok(NativeReadOutcome::Refused(reason));
        let mut access = match repository.human_access(actor, scope.game_id, scope.context_id)? {
            NativeHumanAccessOutcome::Granted(access) => access,
            NativeHumanAccessOutcome::Unauthorized => {
                return refused(NativeReadRefusal::Unauthorized);
            }
            NativeHumanAccessOutcome::Unavailable => {
                return refused(NativeReadRefusal::Unavailable);
            }
        };
        let full = request.mode == PlayerReplayMode::CompletedFullInformation;
        let completed = access.completed_disclosure().is_some();
        let own = request.perspective_seat_id == access.viewer_seat_id();
        if full && !completed || !full && !own {
            if let Some(outcome) = Self::refresh(repository, actor, &scope, &access, None)? {
                return refused(outcome);
            }
            return Ok(NativeReadOutcome::Ready(NativeReplayRead::ModeUnavailable));
        }
        if !completed && !access.operational_no_contest() {
            return self.live_replay_frame(repository, actor, &scope, &access, request, account);
        }
        let manifest = match access.replay().0 {
            Some(manifest) => manifest,
            None => {
                let published = match repository.publish_portable_replay(
                    &self.store,
                    &self.programs,
                    &access,
                    &self.recovery,
                ) {
                    Ok(Some(manifest)) => manifest,
                    Ok(None) => return refused(NativeReadRefusal::Unavailable),
                    Err(error) => {
                        if let Some(outcome) =
                            Self::refresh(repository, actor, &scope, &access, None)?
                        {
                            return refused(outcome);
                        }
                        return Err(error);
                    }
                };
                let renewed =
                    match repository.human_access(actor, scope.game_id, scope.context_id)? {
                        NativeHumanAccessOutcome::Granted(renewed) => renewed,
                        NativeHumanAccessOutcome::Unauthorized => {
                            return refused(NativeReadRefusal::Unauthorized);
                        }
                        NativeHumanAccessOutcome::Unavailable => {
                            return refused(NativeReadRefusal::Unavailable);
                        }
                    };
                if !access.same_except_publication(&renewed)
                    || renewed.replay().0 != Some(published)
                {
                    return refused(NativeReadRefusal::Stale);
                }
                access = renewed;
                published
            }
        };
        let disclosure = if full {
            NativeDisclosure::CompletedParticipant
        } else {
            NativeDisclosure::LiveParticipant
        };
        let mut budget = self.recovery.clone();
        let composed = (|| {
            let Some(archive) = repository.read_portable_replay(
                &self.store,
                ReplayTarget {
                    game: scope.game_id,
                    context: scope.context_id,
                    manifest,
                },
                request.perspective_seat_id,
                disclosure,
                Some(&request.at_event_sequence),
                &mut budget,
            )?
            else {
                return Ok(NativeReplayRead::ModeUnavailable);
            };
            let release = current_release(repository, &scope)?;
            if archive.manifest_id() != manifest || archive.release_id() != release {
                return Err(invalid());
            }
            compose_frame(
                repository,
                &archive.cut_source(),
                FrameScope {
                    release,
                    access: None,
                    completed,
                    account,
                    own_seat: own,
                },
                request,
                &mut budget,
            )
        })();
        #[cfg(test)]
        self.observe_read_delivery(TestDeliveryStage::ArchiveReplayCompose, repository, None);
        if let Some(outcome) = Self::refresh(repository, actor, &scope, &access, None)? {
            return refused(outcome);
        }
        composed.map(NativeReadOutcome::Ready)
    }
    /// Own-seat frame of a game still in progress, from the same fresh original
    /// native walk and live cursors as participant history pages.
    fn live_replay_frame(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        scope: &PlayerViewRequest,
        access: &NativeHumanAccess,
        request: &PlayerReplayRequest,
        account: Uuid,
    ) -> Result<NativeReadOutcome<NativeReplayRead>, NativeSourceError> {
        let history = match repository.project_human_history(
            &self.store,
            &self.programs,
            access,
            self.recovery.clone(),
        ) {
            Ok(Some(history)) => history,
            Ok(None) => return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unavailable)),
            Err(error) => {
                #[cfg(test)]
                self.observe_read_delivery(TestDeliveryStage::LiveReplayCompose, repository, None);
                if let Some(outcome) = Self::refresh(repository, actor, scope, access, None)? {
                    return Ok(NativeReadOutcome::Refused(outcome));
                }
                return Err(error);
            }
        };
        let composed = (|| {
            let mut budget = history.remaining_budget();
            compose_frame(
                repository,
                &history.cut_source(),
                FrameScope {
                    release: current_release(repository, scope)?,
                    access: Some(access),
                    completed: false,
                    account,
                    own_seat: true,
                },
                request,
                &mut budget,
            )
        })();
        #[cfg(test)]
        self.observe_read_delivery(
            TestDeliveryStage::LiveReplayCompose,
            repository,
            Some(history.recovered()),
        );
        if let Some(outcome) =
            Self::refresh(repository, actor, scope, access, Some(history.recovered()))?
        {
            return Ok(NativeReadOutcome::Refused(outcome));
        }
        composed.map(NativeReadOutcome::Ready)
    }
}
/// The game's current release, read for its exact context.
fn current_release(
    repository: &NativeSourceRepository,
    scope: &PlayerViewRequest,
) -> Result<Uuid, NativeSourceError> {
    let game = repository.read(scope.game_id)?.ok_or_else(invalid)?;
    if game.context_ids().0 != scope.context_id {
        return Err(invalid());
    }
    Ok(game.context_ids().1)
}
/// Authority facts for one frame over an already authorized cut source.
struct FrameScope<'a> {
    release: Uuid,
    /// Live participant access whose cursors must also bind; None for archives.
    access: Option<&'a NativeHumanAccess>,
    completed: bool,
    account: Uuid,
    /// Whether the requested perspective is the requester's own seat.
    own_seat: bool,
}
fn compose_frame<C: HistoryCut>(
    repository: &NativeSourceRepository,
    source: &CutSource<'_, C>,
    scope: FrameScope<'_>,
    request: &PlayerReplayRequest,
    budget: &mut NativeRecoveryBudget,
) -> Result<NativeReplayRead, NativeSourceError> {
    let Some(at_index) = source.index_of(&request.at_event_sequence) else {
        return Ok(NativeReplayRead::CoordinateUnavailable);
    };
    let previous = match &request.history.cursor {
        None => None,
        Some(requested) => match repository.read_cut_history_cursor(
            source,
            at_index,
            requested,
            Some(&mut *budget),
        )? {
            Some(previous) => Some(previous),
            None => return Ok(NativeReplayRead::CursorUnavailable),
        },
    };
    let previous_public = previous.as_ref().map(IssuedNativeCursor::cursor);
    request
        .validate(previous_public.as_ref())
        .map_err(|_| invalid())?;
    let cut = source.cuts.get(at_index).ok_or_else(invalid)?;
    let reserved = serde_json::to_vec(cut.snapshot())
        .map_err(|error| NativeSourceError(Failure::Backend(BackendError::caused("json", error))))?
        .len();
    let (history, indexes) = repository.compose_cut_page(
        source,
        PageScope {
            at_index,
            access: scope.access,
            reserved,
        },
        &request.history,
        previous.as_ref(),
        budget,
    )?;
    let schema = PlayerReplaySchema::try_from(REPLAY_SCHEMA.to_owned()).map_err(|_| invalid())?;
    let frame = PlayerReplayFrame {
        request: request.clone(),
        release_id: scope.release,
        schema_revision: schema.clone(),
        snapshot: Box::new(cut.snapshot().clone()),
        history,
    };
    let events = indexes
        .iter()
        .map(|&index| cut_event_facts(source, index))
        .collect::<Result<Vec<_>, _>>()?;
    // Only the requester's own participation is asserted; other participants'
    // accounts are not needed to authorize this frame.
    let participants = [scope.account];
    frame
        .validate(
            request,
            &PlayerReplayFacts {
                requester_account_id: scope.account,
                participant_account_ids: &participants,
                historical_seat_id: request.perspective_seat_id,
                historical_seat_owner: scope.own_seat.then_some(scope.account),
                completed: scope.completed,
                full_information_eligible: scope.completed,
                source: PlayerReplaySource {
                    game_id: request.game_id,
                    context_id: request.context_id,
                    release_id: scope.release,
                    schema_revision: &schema,
                },
                projection: cut.cut_facts(source.vocabulary)?,
                history: PlayerHistoryFacts {
                    viewer: request.perspective_seat_id,
                    requested_cursor: previous_public.as_ref(),
                    scanned_cursor: &frame.history.scanned_through,
                    has_more: frame.history.has_more,
                    events: &events,
                },
            },
        )
        .map_err(|_| invalid())?;
    Ok(NativeReplayRead::Frame(Box::new(frame)))
}
