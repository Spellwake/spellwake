//! Ordered verified cuts of one perspective and disclosure, whether they come
//! from a fresh original native walk or a published portable replay archive.
//! Scan verification, history cursors and page composition consume this one
//! abstraction, so the archive path cannot drift from the native path.
use super::*;
use crate::player_wire::{PlayerProjectionFacts, PlayerSnapshot, PlayerTerminal};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Accepted SQL lineage of one cut, joined against raw archive scan items.
pub(super) struct CutLineage<'a> {
    pub(super) event_id: Uuid,
    pub(super) event_ref: &'a NativeArtifactRef,
    pub(super) state_ref: &'a NativeArtifactRef,
    pub(super) checkpoint_id: Uuid,
    pub(super) creator_receipt_id: Uuid,
}
/// One verified historical cut. Implementors are constructed only after their
/// own source verification; this trait grants no authority by itself.
pub(super) trait HistoryCut {
    fn snapshot(&self) -> &PlayerSnapshot;
    fn cut_facts<'a>(
        &'a self,
        vocabulary: &'a CreatureTypeVocabulary,
    ) -> Result<PlayerProjectionFacts<'a>, NativeSourceError>;
    fn cut_terminal(&self) -> Option<&PlayerTerminal>;
    /// SQL-winning View receipt of this cut, used as a scan/cursor anchor.
    fn view_receipt(&self) -> Uuid;
    fn lineage(&self) -> CutLineage<'_>;
}
/// Complete ordered cuts from raw event zero for one perspective/disclosure.
/// Maintained original/archive constructors enforce dense sequence0..frontier,
/// including silent cuts. Accepted revision is a separate coordinate domain.
/// The original transition-v1 projector admits one event per checkpoint group;
/// sparse/pruned sources or a multi-event codec require an explicit new contract.
/// Page adjacency arithmetic relies on this closed constructor boundary.
pub(super) struct CutSource<'a, C> {
    pub(super) cuts: &'a [C],
    pub(super) disclosure: NativeDisclosure,
    pub(super) opening_receipt_id: Uuid,
    pub(super) vocabulary: &'a CreatureTypeVocabulary,
}
impl<C: HistoryCut> CutSource<'_, C> {
    pub(super) fn index_of(&self, sequence: &Amount) -> Option<usize> {
        self.cuts
            .iter()
            .position(|cut| &cut.snapshot().through_event_sequence == sequence)
    }
    /// Disclosure label used by SQL scans and cursors.
    pub(super) fn disclosure_label(&self) -> &'static str {
        match self.disclosure {
            NativeDisclosure::LiveParticipant => "live",
            NativeDisclosure::CompletedParticipant => "completed",
        }
    }
}
pub(super) fn projection_error(error: NativeProjectionError) -> NativeSourceError {
    NativeSourceError(Failure::View(NativeViewError::Projection(error)))
}
impl HistoryCut for NativeHistoricalObservation {
    fn snapshot(&self) -> &PlayerSnapshot {
        self.projection.snapshot()
    }
    fn cut_facts<'a>(
        &'a self,
        vocabulary: &'a CreatureTypeVocabulary,
    ) -> Result<PlayerProjectionFacts<'a>, NativeSourceError> {
        self.projection.facts(vocabulary).map_err(projection_error)
    }
    fn cut_terminal(&self) -> Option<&PlayerTerminal> {
        self.terminal.as_ref()
    }
    fn view_receipt(&self) -> Uuid {
        self.receipt_id
    }
    fn lineage(&self) -> CutLineage<'_> {
        CutLineage {
            event_id: self.event_id,
            event_ref: &self.event_ref,
            state_ref: &self.state_ref,
            checkpoint_id: self.checkpoint_id,
            creator_receipt_id: self.creator_receipt_id,
        }
    }
}
impl NativeHumanHistory {
    /// The verified walk as a cut source using its original vocabulary.
    pub(super) fn cut_source(&self) -> CutSource<'_, NativeHistoricalObservation> {
        CutSource {
            cuts: self.observations(),
            disclosure: self.disclosure(),
            opening_receipt_id: self.recovered().opening().receipt_ids().0,
            vocabulary: self
                .recovered()
                .checkpoint()
                .state()
                .execution()
                .creature_vocabulary(),
        }
    }
}
