//! Exact pending own-decision source. Current collection ownership is not reread.
use super::*;
use crate::match_history::Availability;

/// Decision and clock/deadline coordinates captured by222.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBetweenDecision {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub after_game_id: Uuid,
    pub basis_revision: Amount,
    pub clock_revision: Amount,
    pub deadline_revision: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_at: Option<String>,
    pub state: String,
}
/// Immutable admission identity plus the current match/availability revision.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBetweenMatch {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub revision: Amount,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub legality_revision_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub release_id: Uuid,
    pub availability_revision: Amount,
    pub availability: Availability,
    pub structure: MatchStructure,
    pub started_at: String,
}
/// Original pool and currently selected sealed deck remain distinct identities.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBetweenTerms {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub decision: NativeBetweenDecision,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub r#match: NativeBetweenMatch,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub profile: PreparationProfile,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub release: PreparationRelease,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub original_revision_id: Uuid,
    pub original_snapshot_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub selected_revision_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub entries: Vec<PreparationEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    source_hash: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    terms: NativeBetweenTerms,
}
impl Record {
    fn valid(&self, decision: Uuid, basis: &Amount, selected: Uuid) -> bool {
        let t = &self.terms;
        let d = &t.decision;
        let m = &t.r#match;
        valid_digest(&self.source_hash)
            && t.version == "native-between-source-v1"
            && d.id == decision
            && m.revision == *basis
            && t.selected_revision_id == selected
            && d.match_id == m.id
            && d.basis_revision <= m.revision
            && d.state == "pending"
            && m.profile_id == t.profile.id
            && m.release_id == t.release.id
            && t.profile.valid()
            && m.availability == Availability::Available
            && m.structure == MatchStructure::Bo3
            && parse_utc_microsecond(&m.started_at).is_some()
            && match &d.deadline_at {
                Some(time) => {
                    !d.deadline_revision.is_zero() && parse_utc_microsecond(time).is_some()
                }
                None => d.deadline_revision.is_zero(),
            }
            && valid_entries(&t.entries)
    }
}
/// Sealed observation, not permission to answer and not a retained mutation lease.
pub struct NativeBetweenSource {
    record: Record,
}
impl NativeBetweenSource {
    /// Exact PostgreSQL hash to revalidate with the original selection at commit.
    pub fn source_hash(&self) -> &str {
        &self.record.source_hash
    }
    /// Borrow selected metadata and the separately retained original pool identity.
    pub fn terms(&self) -> &NativeBetweenTerms {
        &self.record.terms
    }
}
impl NativePreparationRepository {
    /// Read the exact pending decision/current match basis and selected revision.
    /// # Errors
    /// Rejects nil IDs before IO, malformed envelopes, unknown failures and
    /// inconsistent source/clock/identity relationships. Does not grant Auth.
    pub fn between(
        &self,
        decision: Uuid,
        basis: &Amount,
        selected: Uuid,
    ) -> Result<Option<NativeBetweenSource>, NativePreparationError> {
        if decision.is_nil() || selected.is_nil() {
            return Err(invalid());
        }
        let record: Option<Record> = self.record(
            "read_native_between_source",
            json!({"p_decision":decision,"p_basis":basis,"p_selected_revision":selected}),
            "source",
        )?;
        record
            .map(|record| {
                if record.valid(decision, basis, selected) {
                    Ok(NativeBetweenSource { record })
                } else {
                    Err(invalid())
                }
            })
            .transpose()
    }
}
