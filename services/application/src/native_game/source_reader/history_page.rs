//! Bounded public participant history pages over verified raw archive scans.
use super::cuts::{CutSource, HistoryCut};
use super::*;
use crate::player_wire::{
    MAX_PLAYER_WIRE_BYTES, PlayerEventFacts, PlayerHistoryFacts, PlayerHistoryPage,
    PlayerHistoryQuery, PlayerVisibleEvent,
};

/// Raw SQL scans one page may consume. An explicit per-request work bound: a
/// page that stops here still reports truthful progress and hasMore.
const MAX_PAGE_SCANS: usize = 16;
/// Reserved for the page envelope, query and cursor around the visible events.
const PAGE_ENVELOPE_BYTES: usize = 8_192;

/// Consecutive verified cuts `index - 1` and `index` as delta facts. A native
/// conclusion ends the game, so a cut after a terminal observation is malformed.
pub(super) fn cut_event_facts<'a, C: HistoryCut>(
    source: &CutSource<'a, C>,
    index: usize,
) -> Result<PlayerEventFacts<'a>, NativeSourceError> {
    let before = index
        .checked_sub(1)
        .and_then(|i| source.cuts.get(i))
        .ok_or_else(invalid)?;
    let after = source.cuts.get(index).ok_or_else(invalid)?;
    if before.cut_terminal().is_some() {
        return Err(invalid());
    }
    Ok(PlayerEventFacts {
        before: before.snapshot(),
        before_projection: before.cut_facts(source.vocabulary)?,
        after: after.snapshot(),
        after_projection: after.cut_facts(source.vocabulary)?,
        terminal: after.cut_terminal(),
    })
}
/// Where a page ends and what else shares its bounds.
pub(super) struct PageScope<'a> {
    /// Last cut a page may scan (inclusive).
    pub(super) at_index: usize,
    /// Live participant access, when cursors must also match it.
    pub(super) access: Option<&'a NativeHumanAccess>,
    /// Other bytes in the same response, e.g. a replay frame's snapshot.
    pub(super) reserved: usize,
}
fn encoded_len(event: &PlayerVisibleEvent) -> Result<usize, NativeSourceError> {
    serde_json::to_vec(event)
        .map(|bytes| bytes.len())
        .map_err(|error| NativeSourceError(Failure::Backend(BackendError::caused("json", error))))
}
impl NativeSourceRepository {
    /// Compose one ordinary live-participant history page from a verified walk.
    ///
    /// Raw event zero is the opening baseline, never a visible event. The query
    /// limit counts visible events; each SQL scan requests at most the remaining
    /// visible count, so every examined raw record is consumed exactly and no
    /// visible record is skipped. Silent/private records advance scannedThrough.
    /// The issued cursor's basis is the basis of the cut it points at.
    /// If the visible events of a scan would exceed the aggregate wire bound, the
    /// same deterministic scan is repeated over only the records that fit. The
    /// history cursor is issued from the exact final SQL scan reaching that
    /// position. The original actor/session/source must still be renewed by the
    /// caller after composition and before HTTP publication.
    ///
    /// # Errors
    /// Rejects foreign/completed-disclosure walks, a snapshot or foreign cursor,
    /// scan/cut drift, delta/validator failure, an indivisible oversized event,
    /// budget exhaustion and issuer faults. No partial page is returned.
    pub fn compose_human_history_page(
        &self,
        access: &NativeHumanAccess,
        history: &NativeHumanHistory,
        query: &PlayerHistoryQuery,
        previous: Option<&IssuedNativeCursor>,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<PlayerHistoryPage, NativeSourceError> {
        let source = history.cut_source();
        let tip = source.cuts.len().checked_sub(1).ok_or_else(invalid)?;
        let viewer = source.cuts[tip].snapshot().viewer_seat_id;
        if history.disclosure() != NativeDisclosure::LiveParticipant
            || viewer != access.viewer_seat_id()
        {
            return Err(invalid());
        }
        self.compose_cut_page(
            &source,
            PageScope {
                at_index: tip,
                access: Some(access),
                reserved: 0,
            },
            query,
            previous,
            budget,
        )
        .map(|(page, _)| page)
    }
    /// Shared page composer over any verified cut source, never scanning past
    /// `at_index`. With live access, issued cursors are also checked against it.
    /// Returns the page and the cut indexes of its events, for frame validation.
    /// `reserved` counts other bytes sharing the response bound, such as a
    /// replay frame's snapshot.
    /// # Errors
    /// As compose_human_history_page.
    pub(super) fn compose_cut_page<C: HistoryCut>(
        &self,
        source: &CutSource<'_, C>,
        scope: PageScope<'_>,
        query: &PlayerHistoryQuery,
        previous: Option<&IssuedNativeCursor>,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<(PlayerHistoryPage, Vec<usize>), NativeSourceError> {
        let PageScope {
            at_index,
            access,
            reserved,
        } = scope;
        let tip = source.cuts.get(at_index).ok_or_else(invalid)?.snapshot();
        let viewer = tip.viewer_seat_id;
        if tip.basis.game_id != query.game_id || tip.basis.context_id != query.context_id {
            return Err(invalid());
        }
        if let Some(previous) = previous {
            previous.validate_cut_history(tip, source.disclosure)?;
            if let Some(access) = access {
                previous.validate_human_history(access)?;
            }
        }
        let previous_public = previous.map(IssuedNativeCursor::cursor);
        query
            .validate(viewer, previous_public.as_ref())
            .map_err(|_| invalid())?;
        let at = tip.through_event_sequence.clone();
        let limit = usize::try_from(query.limit.get()).map_err(|_| invalid())?;
        let mut after = previous_public
            .as_ref()
            .map(|cursor| cursor.through_event_sequence.clone());
        let mut next_index = match &after {
            None => 0,
            Some(position) => source.index_of(position).ok_or_else(invalid)? + 1,
        };
        let mut indexes = Vec::new();
        let mut events = Vec::new();
        let mut bytes = PAGE_ENVELOPE_BYTES.saturating_add(reserved);
        let mut last: Option<NativeArchiveScan> = None;
        // An already-current cursor needs no new scan; its issued position stands.
        let caught_up = after.as_ref() == Some(&at);
        for _ in 0..MAX_PAGE_SCANS {
            let remaining = limit - events.len();
            if caught_up || remaining == 0 || last.as_ref().is_some_and(|scan| !scan.has_more()) {
                break;
            }
            let raw = u32::try_from(remaining).map_err(|_| invalid())?;
            let scan = self.read_cut_archive(source, &at, after.as_ref(), raw, budget)?;
            let mut staged = Vec::new();
            let mut staged_bytes = bytes;
            let mut fit = 0;
            let mut overflow = false;
            for offset in 0..scan.item_count() {
                let index = next_index + offset;
                if index > at_index {
                    return Err(invalid());
                }
                if index > 0 {
                    let facts = cut_event_facts(source, index)?;
                    if let Some(event) =
                        PlayerVisibleEvent::from_projections(&facts).map_err(|_| invalid())?
                    {
                        let size = encoded_len(&event)?;
                        if staged_bytes.saturating_add(size) > MAX_PLAYER_WIRE_BYTES {
                            overflow = true;
                            break;
                        }
                        staged_bytes += size;
                        staged.push((index, event));
                    }
                }
                fit = offset + 1;
            }
            let scan = if !overflow {
                scan
            } else if fit == 0 {
                break;
            } else {
                // Repeat the same immutable prefix over exactly the records that
                // fit, so the issued position equals the last consumed record.
                let partial = self.read_cut_archive(
                    source,
                    &at,
                    after.as_ref(),
                    u32::try_from(fit).map_err(|_| invalid())?,
                    budget,
                )?;
                if partial.item_count() != fit {
                    return Err(invalid());
                }
                partial
            };
            let through = (next_index + scan.item_count())
                .checked_sub(1)
                .ok_or_else(invalid)?;
            if source
                .cuts
                .get(through)
                .map(|cut| &cut.snapshot().through_event_sequence)
                != Some(scan.scanned_through())
            {
                return Err(invalid());
            }
            for (index, event) in staged {
                indexes.push(index);
                events.push(event);
            }
            bytes = staged_bytes;
            next_index = through + 1;
            after = Some(scan.scanned_through().clone());
            last = Some(scan);
            if overflow {
                break;
            }
        }
        let (scanned, has_more) = match (&last, previous) {
            (Some(scan), _) => {
                // Issue the position from the same raw interval anchored at the
                // View of the cut it ends on, so the cursor basis is that cut's
                // basis rather than the current tip. Its identity is then stable
                // across page sizes and later accepted cuts. hasMore still comes
                // from the tip-anchored scan.
                let item_count = scan.item_count();
                let anchored = self.read_cut_archive(
                    source,
                    scan.scanned_through(),
                    scan.after_sequence(),
                    u32::try_from(item_count).map_err(|_| invalid())?,
                    budget,
                )?;
                if anchored.item_count() != item_count
                    || anchored.scanned_through() != scan.scanned_through()
                    || anchored.has_more()
                {
                    return Err(invalid());
                }
                let issued = self.issue_human_history_cursor(&anchored, budget)?;
                issued.validate_cut_history(tip, source.disclosure)?;
                if let Some(access) = access {
                    issued.validate_human_history(access)?;
                }
                (issued.cursor(), scan.has_more())
            }
            (None, Some(previous)) if caught_up => (previous.cursor(), false),
            // The first record alone exceeded the aggregate bound.
            (None, _) => return Err(invalid()),
        };
        let page = PlayerHistoryPage {
            query: query.clone(),
            viewer_seat_id: viewer,
            events,
            scanned_through: scanned.clone(),
            has_more,
        };
        let facts = indexes
            .iter()
            .map(|&index| cut_event_facts(source, index))
            .collect::<Result<Vec<_>, _>>()?;
        page.validate(
            query,
            &PlayerHistoryFacts {
                viewer,
                requested_cursor: previous_public.as_ref(),
                scanned_cursor: &scanned,
                has_more,
                events: &facts,
            },
        )
        .map_err(|_| invalid())?;
        Ok((page, indexes))
    }
}

#[cfg(test)]
mod tests;
