//! Runtime-independent authentication of retained observations against originals.
use super::*;
use crate::{
    match_history::Outcome,
    native_game::{
        initial_source::VerifiedProgramIdentity,
        input_codec::{RetainedViewSource, verify_retained_view},
    },
    price_source::EvidenceHash,
};

/// Independent accepted inputs; none is reconstructed from an archived snapshot.
pub(super) struct Originals<'a> {
    pub(super) source: &'a NativeGameSource,
    pub(super) opening: &'a NativeOpeningSource,
    pub(super) program: &'a VerifiedProgramIdentity,
    pub(super) program_hash: &'a EvidenceHash,
    pub(super) vocabulary: &'a CreatureTypeVocabulary,
    pub(super) roster: &'a [(Uuid, u64)],
    pub(super) perspective: Uuid,
    pub(super) disclosure: NativeDisclosure,
}
impl Originals<'_> {
    pub(super) fn verify_view(
        &self,
        repository: &NativeSourceRepository,
        store: &NativeArtifactStore,
        cut: &EncodedCut,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<(), NativeSourceError> {
        let snapshot = cut.projection.snapshot();
        let (context, release, profile) = self.source.context_ids();
        if snapshot.basis.game_id != self.source.game_id()
            || snapshot.basis.context_id != context
            || snapshot.basis.epoch != self.opening.native_epoch()
            || snapshot.viewer_seat_id != self.perspective
            || snapshot.through_event_sequence != cut.sequence
        {
            return Err(fault("original View scope"));
        }
        let expected = repository
            .archived_view_source(
                super::super::view::ArchivedViewClaim {
                    basis: &snapshot.basis,
                    viewer: self.perspective,
                    disclosure: self.disclosure,
                    opening: self.opening.receipt_ids().0,
                    checkpoint: cut.checkpoint_id,
                    through: &cut.sequence,
                    receipt: cut.view_receipt_id,
                    reference: &cut.view_ref,
                },
                budget,
            )
            .inspect_err(|_| note_fault("sql-winning View"))?;
        charge(budget, expected.reference())?;
        let artifact = store
            .read(
                expected.reference(),
                self.source.game_id(),
                context,
                NativeArtifactRole::View,
            )
            .map_err(artifact_error)?;
        verify_retained_view(
            &artifact,
            &expected,
            &RetainedViewSource {
                release,
                profile,
                catalogue: self.program.catalogue_id(),
                programs: self.program_hash.as_str(),
                rules: self.vocabulary.rules_revision(),
                vocabulary_id: self.vocabulary.id(),
                name_rules: self.program.name_rules_id(),
                roster: self.roster,
            },
            snapshot,
        )
        .map_err(|error| {
            note_fault("retained View");
            NativeSourceError(Failure::View(error))
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_event_id: Uuid,
    outcome: Outcome,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    winner_seat_ids: Vec<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalEvent {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    sequence: Amount,
    accepted_revision: NativeRevision,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_receipt_id: Uuid,
}
pub(super) struct TerminalSource {
    event: Uuid,
    value: PlayerTerminal,
}
impl NativeSourceRepository {
    /// Accepted result truth is independent of archive terminal metadata. The
    /// exact final event lookup does not load or certify later archive pages.
    pub(super) fn portable_terminal_source(
        &self,
        source: &NativeGameSource,
        index: &EncodedIndex,
        native_conclusion: bool,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<Option<TerminalSource>, NativeSourceError> {
        let rows: Vec<ResultRow> = self.native_rows(
            "game_result",
            &[
                (
                    "select",
                    "id,game_id,source_event_id,outcome,winner_seat_ids".into(),
                ),
                ("game_id", format!("eq.{}", source.game_id())),
                ("limit", "2".into()),
            ],
            16_384,
            budget,
        )?;
        if !native_conclusion {
            return if rows.is_empty() {
                Ok(None)
            } else {
                Err(fault("incident result"))
            };
        }
        let [result] = <[ResultRow; 1]>::try_from(rows).map_err(|_| fault("result rows"))?;
        if result.id.is_nil()
            || result.game_id != source.game_id()
            || result.winner_seat_ids.len() > 6
            || result
                .winner_seat_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != result.winner_seat_ids.len()
            || result
                .winner_seat_ids
                .iter()
                .any(|winner| !source.controllers().any(|seat| seat.0 == *winner))
            || !matches!(result.outcome, Outcome::Win | Outcome::Draw)
            || (result.outcome == Outcome::Win) != !result.winner_seat_ids.is_empty()
        {
            return Err(fault("result fields"));
        }
        let events: Vec<TerminalEvent> = self.native_rows("game_event", &[
            ("select", "id,game_id,context_id,sequence::text,accepted_revision::text,source_receipt_id".into()),
            ("id", format!("eq.{}", result.source_event_id)),
            ("game_id", format!("eq.{}", source.game_id())),
            ("context_id", format!("eq.{}", index.context_id)),
            ("limit", "2".into()),
        ], 16_384, budget)?;
        let [event] =
            <[TerminalEvent; 1]>::try_from(events).map_err(|_| fault("terminal event rows"))?;
        if event.id != result.source_event_id
            || event.game_id != source.game_id()
            || event.context_id != index.context_id
            || event.sequence != index.through_event_sequence
            || Amount::from(event.accepted_revision.get()) != index.through_revision
            || event.source_receipt_id != source.checkpoint_ids().1
        {
            return Err(fault("terminal event source"));
        }
        Ok(Some(TerminalSource {
            event: event.id,
            value: PlayerTerminal {
                outcome: result.outcome,
                winner_seat_ids: result.winner_seat_ids,
            },
        }))
    }

    /// Authenticate every loaded cut, including baseline and silent records,
    /// regardless of which visible-history page the caller later requests.
    pub(super) fn verify_portable_cut_sources(
        &self,
        source: &CutSource<'_, ArchivedCut>,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<(), NativeSourceError> {
        let last = source
            .cuts
            .last()
            .ok_or_else(invalid)?
            .snapshot()
            .through_event_sequence
            .clone();
        let mut after = None;
        loop {
            let scan = self
                .read_cut_archive(source, &last, after.as_ref(), 100, budget)
                .inspect_err(|_| note_fault("accepted cut source"))?;
            if after
                .as_ref()
                .is_some_and(|previous| scan.scanned_through() <= previous)
            {
                return Err(fault("source closure progress"));
            }
            if !scan.has_more() {
                if scan.scanned_through() != &last {
                    return Err(fault("source closure frontier"));
                }
                return Ok(());
            }
            after = Some(scan.scanned_through().clone());
        }
    }
}
/// Only a consumed final cut carries a terminal value. Earlier bounded reads
/// authenticate conclusion metadata without pretending to read later pages.
pub(super) fn verify_terminal(
    cuts: &[ArchivedCut],
    index: &EncodedIndex,
    terminal: Option<&TerminalSource>,
) -> Result<(), NativeSourceError> {
    let last = cuts.last().ok_or_else(invalid)?;
    verify_terminal_value(
        &last.snapshot().through_event_sequence,
        last.event_id,
        last.terminal.as_ref(),
        index,
        terminal,
    )
}

// Keep the consumed-cut comparison independent of projection construction.
// This is the production terminal verifier's only value comparison.
fn verify_terminal_value(
    loaded_sequence: &Amount,
    event: Uuid,
    value: Option<&PlayerTerminal>,
    index: &EncodedIndex,
    terminal: Option<&TerminalSource>,
) -> Result<(), NativeSourceError> {
    if *loaded_sequence == index.through_event_sequence {
        match (terminal, value) {
            (Some(source), Some(value)) if source.event == event && source.value == *value => {}
            (None, None) if index.conclusion == OPERATIONAL_INCIDENT => {}
            _ => return Err(fault("terminal value")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod closure_tests;
#[cfg(test)]
mod tests;
