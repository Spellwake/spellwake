//! Authenticated live-participant history pages. No replay archive, completed
//! full reveal or SSE history mode is delivered through this path.
use super::*;
use crate::player_wire::{PlayerHistoryPage, PlayerHistoryQuery};

/// History-specific delivery result inside the common refusal envelope.
pub(crate) enum NativeHistoryRead {
    /// Verified page; the caller still renews the actor before publication.
    Page(Box<PlayerHistoryPage>),
    /// The supplied cursor is unknown, foreign, snapshot-purpose or another
    /// disclosure. There is deliberately no reset-to-start fallback.
    CursorUnavailable,
}
impl NativeGameRuntime {
    /// Compose one own-seat history page over a fresh original native walk.
    /// Client fields select game/context/cursor/limit only, never viewer or
    /// disclosure. Complete access is renewed after composition, including on
    /// composition errors; the HTTP caller renews the bearer afterwards.
    ///
    /// # Errors
    /// Rejects invalid scope, unavailable/malformed sources, recovery or page
    /// budget exhaustion and every strict scan/cursor/delta validation failure.
    pub(crate) fn history_page(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        query: &PlayerHistoryQuery,
    ) -> Result<NativeReadOutcome<NativeHistoryRead>, NativeSourceError> {
        let request = PlayerViewRequest {
            game_id: query.game_id,
            context_id: query.context_id,
        };
        request.validate().map_err(|_| invalid())?;
        let access = match repository.human_access(actor, request.game_id, request.context_id)? {
            NativeHumanAccessOutcome::Granted(access) => access,
            NativeHumanAccessOutcome::Unauthorized => {
                return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unauthorized));
            }
            NativeHumanAccessOutcome::Unavailable => {
                return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unavailable));
            }
        };
        let history = match repository.project_human_history(
            &self.store,
            &self.programs,
            &access,
            self.recovery.clone(),
        ) {
            Ok(Some(history)) => history,
            Ok(None) => return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unavailable)),
            Err(error) => {
                #[cfg(test)]
                self.observe_read_delivery(TestDeliveryStage::HistoryCompose, repository, None);
                if let Some(outcome) = Self::refresh(repository, actor, &request, &access, None)? {
                    return Ok(NativeReadOutcome::Refused(outcome));
                }
                return Err(error);
            }
        };
        let composed = (|| {
            let mut budget = history.remaining_budget();
            let previous = match &query.cursor {
                None => None,
                Some(requested) => {
                    let source = history.cut_source();
                    let tip = source.cuts.len().checked_sub(1).ok_or_else(invalid)?;
                    match repository.read_cut_history_cursor(
                        &source,
                        tip,
                        requested,
                        Some(&mut budget),
                    )? {
                        Some(previous) => Some(previous),
                        None => return Ok(NativeHistoryRead::CursorUnavailable),
                    }
                }
            };
            repository
                .compose_human_history_page(
                    &access,
                    &history,
                    query,
                    previous.as_ref(),
                    &mut budget,
                )
                .map(|page| NativeHistoryRead::Page(Box::new(page)))
        })();
        #[cfg(test)]
        self.observe_read_delivery(
            TestDeliveryStage::HistoryCompose,
            repository,
            Some(history.recovered()),
        );
        if let Some(outcome) = Self::refresh(
            repository,
            actor,
            &request,
            &access,
            Some(history.recovered()),
        )? {
            return Ok(NativeReadOutcome::Refused(outcome));
        }
        composed.map(NativeReadOutcome::Ready)
    }
}
