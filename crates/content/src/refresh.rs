//! Incremental Scryfall catalog refresh state and diffing.
//!
//! Tracks snapshot identity and per-card semantic + legality fingerprints so scheduled
//! catalog refreshes can skip unchanged snapshots, detect semantic drift (invalidating
//! cached compilations and gameplay qualifications), and track preview-format (`future`)
//! transitions completely separately from ordinary base-format legalities.

use crate::{BulkMetadata, CatalogError, Inventory, Legality};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Schema version for persisted incremental catalog refresh state.
pub const REFRESH_STATE_SCHEMA_VERSION: u32 = 1;

/// Outcome of comparing a fetched or loaded snapshot against prior refresh state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefreshStatus {
    /// No previous refresh state was supplied.
    Initial,
    /// Snapshot metadata/hash matched the previous state with zero card changes.
    Unchanged,
    /// Snapshot changed relative to the previous state.
    Updated,
}

/// Deterministic per-card fingerprint separating card semantics from ordinary and preview legalities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardRefreshFingerprint {
    pub oracle_id: String,
    pub scryfall_id: String,
    pub name: String,
    pub layout: String,
    pub semantic_sha256: String,
    pub ordinary_legalities: BTreeMap<String, Legality>,
    pub preview_legalities: BTreeMap<String, Legality>,
}

/// Persisted snapshot summary enabling incremental catalog refreshes across scheduled runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRefreshState {
    pub schema_version: u32,
    pub updated_at: String,
    pub jsonl_download_uri: String,
    pub compressed_size: u64,
    pub compressed_sha256: String,
    pub source_records: usize,
    pub cards: BTreeMap<String, CardRefreshFingerprint>,
}

/// Structured delta between a previous refresh state and the current catalog snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalCatalogDiff {
    pub refresh_status: RefreshStatus,
    pub previous_updated_at: Option<String>,
    pub current_updated_at: String,
    pub previous_sha256: Option<String>,
    pub current_sha256: String,
    pub added_oracle_ids: Vec<String>,
    pub removed_oracle_ids: Vec<String>,
    pub semantically_changed_oracle_ids: Vec<String>,
    pub ordinary_legality_changed_oracle_ids: Vec<String>,
    pub preview_legality_changed_oracle_ids: Vec<String>,
    pub unchanged_oracle_ids: usize,
}

impl IncrementalCatalogDiff {
    /// Oracle identities that must be freshly parsed and lowered because they are new or
    /// their printed semantics changed. Legality-only transitions do not alter card semantics.
    #[must_use]
    pub fn recompilation_oracle_ids(&self) -> BTreeSet<&str> {
        self.added_oracle_ids
            .iter()
            .chain(self.semantically_changed_oracle_ids.iter())
            .map(String::as_str)
            .collect()
    }
}

/// Decide whether a remote Scryfall bulk-data descriptor requires downloading a new snapshot.
///
/// Returns `false` only when the previous persisted state matches `updated_at`,
/// `jsonl_download_uri`, and `compressed_size` exactly.
#[must_use]
pub fn should_download_snapshot(
    previous: Option<&CatalogRefreshState>,
    remote: &BulkMetadata,
) -> bool {
    let Some(prev) = previous else {
        return true;
    };
    prev.updated_at != remote.updated_at
        || prev.jsonl_download_uri != remote.jsonl_download_uri
        || prev.compressed_size != remote.compressed_size
}

#[derive(Serialize)]
struct SemanticInput<'a> {
    version: u32,
    oracle_id: &'a str,
    name: &'a str,
    layout: &'a str,
    faces: &'a [crate::CatalogFace],
    back_color_indicator: Option<&'a [String]>,
    color_identity: Option<&'a [String]>,
    loyalties: Vec<Option<&'a str>>,
    defenses: Vec<Option<&'a str>>,
    meld_parts: Option<[&'a str; 2]>,
    meld_result: Option<&'a str>,
}

/// Compute a deterministic SHA-256 over all gameplay-relevant printed source fields for one card.
///
/// # Errors
/// Returns [`CatalogError::Invalid`] if the card is missing an `oracle_id` or cannot be serialized.
pub fn card_semantic_sha256(
    inventory: &Inventory,
    card_index: usize,
) -> Result<String, CatalogError> {
    let card = inventory
        .cards
        .get(card_index)
        .ok_or_else(|| CatalogError::Invalid("card index out of bounds".into()))?;
    let oracle_id = card
        .oracle_id
        .as_deref()
        .ok_or_else(|| CatalogError::Invalid("card missing oracle_id".into()))?;
    let face_count = card.faces.len();
    let loyalties = (0..face_count)
        .map(|idx| inventory.source_loyalty(oracle_id, idx))
        .collect();
    let defenses = (0..face_count)
        .map(|idx| inventory.source_defense(oracle_id, idx))
        .collect();
    let meld = inventory.meld_set(oracle_id);
    let payload = SemanticInput {
        version: 1,
        oracle_id,
        name: &card.name,
        layout: &card.layout,
        faces: &card.faces,
        back_color_indicator: inventory.back_color_indicator(oracle_id),
        color_identity: inventory.source_color_identity(oracle_id),
        loyalties,
        defenses,
        meld_parts: meld.map(|m| [m.parts[0].as_str(), m.parts[1].as_str()]),
        meld_result: meld.map(|m| m.result.as_str()),
    };
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| CatalogError::Invalid(format!("semantic fingerprint failed: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Build an incremental refresh state snapshot from a validated [`Inventory`].
///
/// Records every card with a unique `oracle_id`; snapshots containing duplicate Oracle IDs are
/// rejected because incremental identity diffing requires canonical 1:1 Oracle identity mapping.
///
/// # Errors
/// Returns [`CatalogError::Invalid`] if the inventory contains duplicate Oracle IDs or serialization fails.
pub fn build_refresh_state(inventory: &Inventory) -> Result<CatalogRefreshState, CatalogError> {
    if !inventory.duplicate_oracle_ids.is_empty() {
        return Err(CatalogError::Invalid(
            "cannot build incremental refresh state with duplicate oracle_id rows".into(),
        ));
    }
    let mut cards = BTreeMap::new();
    for (index, card) in inventory.cards.iter().enumerate() {
        let Some(oracle_id) = card.oracle_id.as_ref() else {
            continue;
        };
        let semantic_sha256 = card_semantic_sha256(inventory, index)?;
        let preview_legalities = inventory
            .preview_legalities_at(index)
            .cloned()
            .unwrap_or_default();
        cards.insert(
            oracle_id.clone(),
            CardRefreshFingerprint {
                oracle_id: oracle_id.clone(),
                scryfall_id: card.scryfall_id.clone(),
                name: card.name.clone(),
                layout: card.layout.clone(),
                semantic_sha256,
                ordinary_legalities: card.legalities.clone(),
                preview_legalities,
            },
        );
    }
    Ok(CatalogRefreshState {
        schema_version: REFRESH_STATE_SCHEMA_VERSION,
        updated_at: inventory.source.updated_at.clone(),
        jsonl_download_uri: inventory.source.jsonl_download_uri.clone(),
        compressed_size: inventory.source.compressed_size,
        compressed_sha256: inventory.compressed_sha256.clone(),
        source_records: inventory.source_records,
        cards,
    })
}

/// Compute the deterministic incremental delta between an optional previous refresh state and the
/// current refresh state.
#[must_use]
pub fn diff_refresh_states(
    previous: Option<&CatalogRefreshState>,
    current: &CatalogRefreshState,
) -> IncrementalCatalogDiff {
    let Some(prev) = previous else {
        return IncrementalCatalogDiff {
            refresh_status: RefreshStatus::Initial,
            previous_updated_at: None,
            current_updated_at: current.updated_at.clone(),
            previous_sha256: None,
            current_sha256: current.compressed_sha256.clone(),
            added_oracle_ids: current.cards.keys().cloned().collect(),
            removed_oracle_ids: Vec::new(),
            semantically_changed_oracle_ids: Vec::new(),
            ordinary_legality_changed_oracle_ids: Vec::new(),
            preview_legality_changed_oracle_ids: Vec::new(),
            unchanged_oracle_ids: 0,
        };
    };

    let mut added_oracle_ids = Vec::new();
    let mut removed_oracle_ids = Vec::new();
    let mut semantically_changed_oracle_ids = Vec::new();
    let mut ordinary_legality_changed_oracle_ids = Vec::new();
    let mut preview_legality_changed_oracle_ids = Vec::new();
    let mut unchanged_oracle_ids = 0usize;

    for (oracle_id, cur) in &current.cards {
        match prev.cards.get(oracle_id) {
            None => added_oracle_ids.push(oracle_id.clone()),
            Some(old) => {
                let semantic_changed = old.semantic_sha256 != cur.semantic_sha256;
                let ordinary_changed = old.ordinary_legalities != cur.ordinary_legalities;
                let preview_changed = old.preview_legalities != cur.preview_legalities;
                if semantic_changed {
                    semantically_changed_oracle_ids.push(oracle_id.clone());
                }
                if ordinary_changed {
                    ordinary_legality_changed_oracle_ids.push(oracle_id.clone());
                }
                if preview_changed {
                    preview_legality_changed_oracle_ids.push(oracle_id.clone());
                }
                if !semantic_changed && !ordinary_changed && !preview_changed {
                    unchanged_oracle_ids += 1;
                }
            }
        }
    }

    for oracle_id in prev.cards.keys() {
        if !current.cards.contains_key(oracle_id) {
            removed_oracle_ids.push(oracle_id.clone());
        }
    }

    let any_card_change = !added_oracle_ids.is_empty()
        || !removed_oracle_ids.is_empty()
        || !semantically_changed_oracle_ids.is_empty()
        || !ordinary_legality_changed_oracle_ids.is_empty()
        || !preview_legality_changed_oracle_ids.is_empty();

    let refresh_status = if !any_card_change && prev.compressed_sha256 == current.compressed_sha256
    {
        RefreshStatus::Unchanged
    } else {
        RefreshStatus::Updated
    };

    IncrementalCatalogDiff {
        refresh_status,
        previous_updated_at: Some(prev.updated_at.clone()),
        current_updated_at: current.updated_at.clone(),
        previous_sha256: Some(prev.compressed_sha256.clone()),
        current_sha256: current.compressed_sha256.clone(),
        added_oracle_ids,
        removed_oracle_ids,
        semantically_changed_oracle_ids,
        ordinary_legality_changed_oracle_ids,
        preview_legality_changed_oracle_ids,
        unchanged_oracle_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory;
    use std::io::Cursor;

    fn metadata(updated_at: &str, uri: &str, size: u64) -> BulkMetadata {
        BulkMetadata {
            object: "bulk_data".into(),
            id: "27bf3214-1271-490b-bdfe-c0be6c23d02e".into(),
            kind: "oracle_cards".into(),
            updated_at: updated_at.into(),
            jsonl_download_uri: uri.into(),
            compressed_size: size,
        }
    }

    fn card_json(
        id: &str,
        oracle_id: &str,
        name: &str,
        text: &str,
        standard: &str,
        future: &str,
    ) -> String {
        serde_json::json!({
            "object": "card",
            "id": id,
            "oracle_id": oracle_id,
            "name": name,
            "layout": "normal",
            "type_line": "Instant",
            "mana_cost": "{R}",
            "oracle_text": text,
            "legalities": {
                "commander": "legal",
                "standard": standard,
                "pioneer": "not_legal",
                "modern": "legal",
                "legacy": "legal",
                "vintage": "legal",
                "future": future
            }
        })
        .to_string()
    }

    #[test]
    fn incremental_diff_classifies_added_removed_semantic_and_legality_changes() {
        let m1 = metadata(
            "2026-10-29T21:00:00Z",
            "https://data.scryfall.io/oracle-cards/oracle-cards-20261029.jsonl.gz",
            100,
        );
        let snap1 = [
            card_json(
                "s1",
                "00000000-0000-0000-0000-000000000001",
                "Unchanged Bolt",
                "Deal 3 damage to any target.",
                "legal",
                "legal",
            ),
            card_json(
                "s2",
                "00000000-0000-0000-0000-000000000002",
                "Errata Card",
                "Draw a card.",
                "legal",
                "legal",
            ),
            card_json(
                "s3",
                "00000000-0000-0000-0000-000000000003",
                "Rotated Card",
                "Gain 2 life.",
                "legal",
                "not_legal",
            ),
            card_json(
                "s4",
                "00000000-0000-0000-0000-000000000004",
                "Preview Transition Card",
                "Scry 1.",
                "not_legal",
                "not_legal",
            ),
            card_json(
                "s5",
                "00000000-0000-0000-0000-000000000005",
                "Removed Card",
                "Gain 1 life.",
                "not_legal",
                "not_legal",
            ),
        ]
        .join("\n");
        let inv1 = inventory(Cursor::new(snap1), m1.clone(), "a".repeat(64)).unwrap();
        let state1 = build_refresh_state(&inv1).unwrap();

        assert!(!should_download_snapshot(Some(&state1), &m1));

        let initial_diff = diff_refresh_states(None, &state1);
        assert_eq!(initial_diff.refresh_status, RefreshStatus::Initial);
        assert_eq!(initial_diff.added_oracle_ids.len(), 5);

        let unchanged_diff = diff_refresh_states(Some(&state1), &state1);
        assert_eq!(unchanged_diff.refresh_status, RefreshStatus::Unchanged);
        assert_eq!(unchanged_diff.unchanged_oracle_ids, 5);
        assert!(unchanged_diff.recompilation_oracle_ids().is_empty());

        let m2 = metadata(
            "2026-11-05T21:00:00Z",
            "https://data.scryfall.io/oracle-cards/oracle-cards-20261105.jsonl.gz",
            120,
        );
        assert!(should_download_snapshot(Some(&state1), &m2));

        let snap2 = [
            card_json(
                "s1",
                "00000000-0000-0000-0000-000000000001",
                "Unchanged Bolt",
                "Deal 3 damage to any target.",
                "legal",
                "legal",
            ),
            card_json(
                "s2",
                "00000000-0000-0000-0000-000000000002",
                "Errata Card",
                "Draw two cards.",
                "legal",
                "legal",
            ),
            card_json(
                "s3",
                "00000000-0000-0000-0000-000000000003",
                "Rotated Card",
                "Gain 2 life.",
                "banned",
                "not_legal",
            ),
            card_json(
                "s4",
                "00000000-0000-0000-0000-000000000004",
                "Preview Transition Card",
                "Scry 1.",
                "not_legal",
                "legal",
            ),
            card_json(
                "s6",
                "00000000-0000-0000-0000-000000000006",
                "Brand New Spoiler",
                "Draw a card.",
                "not_legal",
                "legal",
            ),
        ]
        .join("\n");
        let inv2 = inventory(Cursor::new(snap2), m2, "b".repeat(64)).unwrap();
        let state2 = build_refresh_state(&inv2).unwrap();

        let delta = diff_refresh_states(Some(&state1), &state2);
        assert_eq!(delta.refresh_status, RefreshStatus::Updated);
        assert_eq!(
            delta.added_oracle_ids,
            vec!["00000000-0000-0000-0000-000000000006"]
        );
        assert_eq!(
            delta.removed_oracle_ids,
            vec!["00000000-0000-0000-0000-000000000005"]
        );
        assert_eq!(
            delta.semantically_changed_oracle_ids,
            vec!["00000000-0000-0000-0000-000000000002"]
        );
        assert_eq!(
            delta.ordinary_legality_changed_oracle_ids,
            vec!["00000000-0000-0000-0000-000000000003"]
        );
        assert_eq!(
            delta.preview_legality_changed_oracle_ids,
            vec!["00000000-0000-0000-0000-000000000004"]
        );
        assert_eq!(delta.unchanged_oracle_ids, 1);

        let recompile = delta.recompilation_oracle_ids();
        assert_eq!(recompile.len(), 2);
        assert!(recompile.contains("00000000-0000-0000-0000-000000000002"));
        assert!(recompile.contains("00000000-0000-0000-0000-000000000006"));
        // Legality-only changes do not trigger semantic recompilation.
        assert!(!recompile.contains("00000000-0000-0000-0000-000000000003"));
        assert!(!recompile.contains("00000000-0000-0000-0000-000000000004"));
    }
}
