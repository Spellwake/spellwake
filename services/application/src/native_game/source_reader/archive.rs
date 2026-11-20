//! Exact raw SQL archive scans joined to independently verified historical cuts.
use super::*;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    event_id: Uuid,
    sequence: Amount,
    accepted_revision: NativeRevision,
    event_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    creator_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    checkpoint_id: Uuid,
    state_ref: NativeArtifactRef,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Replay {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    manifest_id: Uuid,
    artifact_ref: NativeArtifactRef,
    visibility_index_ref: NativeArtifactRef,
    schema_revision: String,
    through_sequence: Amount,
    through_revision: NativeRevision,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    viewer_seat_id: Uuid,
    disclosure: String,
    #[serde(deserialize_with = "Option::deserialize")]
    after_sequence: Option<Amount>,
    limit: u32,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_view_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    opening_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    items: Vec<Item>,
    #[serde(deserialize_with = "Option::deserialize")]
    scanned_through: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    next_after: Option<Amount>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    replay: Option<Replay>,
    source_hash: String,
}
/// A bounded scan independently joined to actual accepted native observations.
/// Raw source refs and hash stay private. It does not renew actor/session authority
/// or prove that a reused positional cursor certifies this particular page scan.
pub struct NativeArchiveScan {
    basis: PlayerBasis,
    record: Record,
    through: Amount,
    disclosure: NativeDisclosure,
}
impl NativeArchiveScan {
    /// Last raw coordinate examined, including invisible/silent native records.
    pub fn scanned_through(&self) -> &Amount {
        &self.through
    }
    /// More accepted raw records exist within this immutable View anchor.
    pub fn has_more(&self) -> bool {
        self.record.next_after.is_some()
    }
    /// Number of raw records, not number of public visible observations.
    pub fn item_count(&self) -> usize {
        self.record.items.len()
    }
    /// Original raw scan boundary; absence includes the opening baseline.
    pub fn after_sequence(&self) -> Option<&Amount> {
        self.record.after_sequence.as_ref()
    }
    pub(super) fn basis(&self) -> &PlayerBasis {
        &self.basis
    }
    pub(super) fn viewer(&self) -> Uuid {
        self.record.viewer_seat_id
    }
    pub(super) fn source_receipt(&self) -> Uuid {
        self.record.source_view_receipt_id
    }
    pub(super) fn disclosure(&self) -> NativeDisclosure {
        self.disclosure
    }
    pub(super) fn cursor_arguments(&self) -> Result<serde_json::Value, NativeSourceError> {
        let scan = serde_json::to_value(&self.record).map_err(|error| {
            NativeSourceError(Failure::Backend(BackendError::caused("json", error)))
        })?;
        let b = &self.basis;
        Ok(
            json!({"p_game":b.game_id,"p_context":b.context_id,"p_epoch":b.epoch,"p_revision":b.revision,
            "p_viewer":self.viewer(),"p_through_sequence":self.through,"p_purpose":"history",
            "p_source_receipt":self.source_receipt(),"p_scan":scan}),
        )
    }
}
impl NativeSourceRepository {
    /// Scan a chosen actual SQL-winning historical View's immutable raw prefix.
    /// The anchor is derived from the verified history collection by coordinate;
    /// no caller-supplied receipt/ref/disclosure is accepted as source authority.
    /// The existing ordinary-seat path deliberately uses live scans, whose replay
    /// field stays null across later operational-prefix publication.
    /// # Errors
    /// Rejects foreign/future boundaries, limits outside1..100, malformed raw JSON,
    /// missing/null substitutions, source/cut/ref/progress drift and exhausted budget.
    pub fn read_human_archive(
        &self,
        history: &NativeHumanHistory,
        at_sequence: &Amount,
        after_sequence: Option<&Amount>,
        limit: u32,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<NativeArchiveScan, NativeSourceError> {
        self.read_cut_archive(
            &history.cut_source(),
            at_sequence,
            after_sequence,
            limit,
            budget,
        )
    }
    /// Shared scan verification over any verified cut source (native walk or
    /// published archive). Same checks as read_human_archive.
    pub(super) fn read_cut_archive<C: super::cuts::HistoryCut>(
        &self,
        source: &super::cuts::CutSource<'_, C>,
        at_sequence: &Amount,
        after_sequence: Option<&Amount>,
        limit: u32,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<NativeArchiveScan, NativeSourceError> {
        if !(1..=100).contains(&limit) {
            return Err(invalid());
        }
        let anchor = &source.cuts[source.index_of(at_sequence).ok_or_else(invalid)?];
        let snapshot = anchor.snapshot();
        if let Some(after) = after_sequence
            && (after > at_sequence || source.index_of(after).is_none())
        {
            return Err(invalid());
        }
        let b = &snapshot.basis;
        let disclosure = source.disclosure_label();
        let record: Record = self.native_record(
            "read_native_archive_source",
            &json!({"p_game":b.game_id,"p_context":b.context_id,"p_viewer":snapshot.viewer_seat_id,
                "p_after_sequence":after_sequence,"p_limit":limit,"p_disclosure":disclosure,
                "p_source_view_receipt":anchor.view_receipt()}),
            Some(budget),
        )?;
        if record.game_id != b.game_id
            || record.context_id != b.context_id
            || record.viewer_seat_id != snapshot.viewer_seat_id
            || record.disclosure != disclosure
            || record.after_sequence.as_ref() != after_sequence
            || record.limit != limit
            || record.source_view_receipt_id != anchor.view_receipt()
            || record.opening_receipt_id != source.opening_receipt_id
            || record.items.len() > usize::try_from(limit).map_err(|_| invalid())?
            || !crate::source_protocol::valid_digest(&record.source_hash)
            || record.replay.is_some()
        {
            return Err(invalid());
        }
        // Hash grammar is not Rust recomputation of PostgreSQL jsonb hashing.
        // SQL cursor issuance independently reexecutes and compares this exact scan.
        let mut previous = after_sequence.cloned();
        for item in &record.items {
            let expected = previous.as_ref().map_or(Ok(Amount::from(0)), |previous| {
                previous
                    .checked_add(&Amount::from(1))
                    .map_err(|_| invalid())
            })?;
            let cut = &source.cuts[source.index_of(&item.sequence).ok_or_else(invalid)?];
            let cut_snapshot = cut.snapshot();
            let lineage = cut.lineage();
            if item.sequence != expected
                || &item.sequence > at_sequence
                || item.accepted_revision != cut_snapshot.basis.revision
                || item.accepted_revision > b.revision
                || item.event_id != lineage.event_id
                || &item.event_ref != lineage.event_ref
                || &item.state_ref != lineage.state_ref
                || item.checkpoint_id != lineage.checkpoint_id
                || item.creator_receipt_id != lineage.creator_receipt_id
            {
                return Err(invalid());
            }
            previous = Some(item.sequence.clone());
        }
        let through = previous.ok_or_else(invalid)?;
        let expected_next = (&through < at_sequence).then_some(&through);
        if record.scanned_through.as_ref() != Some(&through)
            || record.next_after.as_ref() != expected_next
            || expected_next.is_some() && record.items.is_empty()
        {
            return Err(invalid());
        }
        Ok(NativeArchiveScan {
            basis: b.clone(),
            record,
            through,
            disclosure: source.disclosure,
        })
    }
}
