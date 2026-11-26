//! Honest, incremental card-support evaluation over any validated Scryfall Oracle snapshot.
//!
//! Uses the existing `spellwake-oracle-compiler` (Stage 1 strict AST parse) and
//! `spellwake-card-programs` current compiler (Stage 2 validated engine `Program` lowering),
//! keeps preview formats (`future` / Future Standard) strictly isolated from ordinary
//! base-format legality, and never fabricates Stage 3 gameplay qualification counts.

use crate::keyword_creatures::compile_current_card_for_refresh;
use serde::{Deserialize, Serialize};
use spellwake_content::{
    BulkMetadata, CatalogError, FORMATS, Inventory, InventoryCard, PREVIEW_FORMATS,
    refresh::{
        CatalogRefreshState, IncrementalCatalogDiff, build_refresh_state, card_semantic_sha256,
        diff_refresh_states,
    },
};
use spellwake_contracts::creature_types::{CreatureTypeVocabulary, CreatureVocabularyId};
use spellwake_oracle_compiler::{
    CardInput, CompileOptions, FaceInput, Vocabulary as OracleVocabulary, compile_card_with_options,
};
use std::collections::BTreeMap;

/// Schema version for persisted incremental card-support compilation caches.
pub const SUPPORT_CACHE_SCHEMA_VERSION: u32 = 1;

/// Rolling compiler revision tag embedded in cached entries; bump whenever lowering semantics change.
pub const SUPPORT_COMPILER_REVISION: &str = "keyword-gameplay-current-v29-oracle-compiler-v1";

/// Cached Stage 1 + Stage 2 result for a single card keyed by its semantic fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedCardSupport {
    pub semantic_sha256: String,
    pub stage1_ast_parsed: bool,
    pub stage2_engine_bound: bool,
    pub blocker_reason: Option<String>,
}

/// Persisted per-card compilation cache enabling incremental recompilation across scheduled refreshes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogSupportCache {
    pub schema_version: u32,
    pub compiler_revision: String,
    pub creature_vocabulary_id: CreatureVocabularyId,
    pub cards: BTreeMap<String, CachedCardSupport>,
}

/// Per-format honest support counts (Stage 1 AST parsed vs Stage 2 engine bound).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatSupportCounts {
    pub eligible: usize,
    pub stage1_ast_parsed: usize,
    pub stage2_engine_bound: usize,
}

/// Top blocker reason count for unbound cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockerSummary {
    pub reason: String,
    pub count: usize,
}

/// Ordinary base-format (`commander`, `standard`, `pioneer`, `modern`, `legacy`, `vintage`) support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinarySupportSummary {
    pub formats: Vec<String>,
    pub source_records: usize,
    pub required_records: usize,
    pub stage1_ast_parsed: usize,
    pub stage1_ast_failed: usize,
    pub stage2_engine_bound: usize,
    pub stage2_engine_blocked: usize,
    /// Compiler evaluation alone always reports 0 game-qualified cards; gameplay qualification
    /// requires joining with verified card-specific game receipts.
    pub stage3_game_qualified: usize,
    pub stage3_qualification_state: String,
    pub by_format: BTreeMap<String, FormatSupportCounts>,
    pub top_blockers: Vec<BlockerSummary>,
}

/// Isolated preview-format (`future` / Future Standard) support summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSupportSummary {
    pub formats: Vec<String>,
    pub isolated_from_ordinary_legality: bool,
    pub eligible_by_format: BTreeMap<String, usize>,
    pub preview_only_records: usize,
    pub preview_not_in_standard: usize,
    pub ordinary_overlap_records: usize,
    pub unknown_preview_legality_records: usize,
    pub by_format: BTreeMap<String, FormatSupportCounts>,
    pub preview_only_support: FormatSupportCounts,
}

/// Incremental execution metrics for a catalog refresh run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalEvaluationSummary {
    pub diff: IncrementalCatalogDiff,
    pub reused_cached_cards: usize,
    pub recompiled_cards: usize,
    pub cache_compatible: bool,
}

/// Complete honest card-support report emitted by `spellwake-card-programs support-report`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSupportReport {
    pub schema_version: u32,
    pub compiler_revision: String,
    pub source: BulkMetadata,
    pub compressed_sha256: String,
    pub creature_vocabulary_id: CreatureVocabularyId,
    pub incremental: IncrementalEvaluationSummary,
    pub ordinary_base_formats: OrdinarySupportSummary,
    pub preview_formats: PreviewSupportSummary,
}

#[inline(never)]
fn eval_stage1_ast(card: &InventoryCard, vocab: &OracleVocabulary) -> bool {
    let faces: Vec<FaceInput<'_>> = card
        .faces
        .iter()
        .map(|face| FaceInput {
            name: &face.name,
            type_line: face.type_line.as_deref().unwrap_or(""),
            oracle_text: face.oracle_text.as_deref().unwrap_or(""),
        })
        .collect();
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces,
    };
    compile_card_with_options(
        &input,
        vocab,
        CompileOptions {
            allow_tabletop_honor_system: false,
        },
    )
    .strictly_compiled
}

/// Evaluate honest Stage 1 (AST parse) and Stage 2 (validated engine `Program` lowering) support
/// across a validated Scryfall Oracle snapshot, reusing unchanged entries from `previous_cache`
/// when compiler revision, creature vocabulary, and per-card semantic SHA-256 all match.
///
/// Preview formats (`future`) are evaluated and reported in a dedicated isolated structure and
/// never leak into ordinary base-format legality or required-card totals.
///
/// # Errors
/// Returns [`CatalogError`] if the inventory contains duplicate Oracle IDs or semantic hashing fails.
pub fn evaluate_catalog_support(
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
    previous_state: Option<&CatalogRefreshState>,
    previous_cache: Option<&CatalogSupportCache>,
) -> Result<
    (
        CatalogSupportReport,
        CatalogRefreshState,
        CatalogSupportCache,
    ),
    CatalogError,
> {
    let current_state = build_refresh_state(inventory)?;
    let diff = diff_refresh_states(previous_state, &current_state);

    let cache_compatible = previous_cache.is_some_and(|cache| {
        cache.schema_version == SUPPORT_CACHE_SCHEMA_VERSION
            && cache.compiler_revision == SUPPORT_COMPILER_REVISION
            && cache.creature_vocabulary_id == vocabulary.id()
    });
    let usable_cache = if cache_compatible {
        previous_cache.map(|cache| &cache.cards)
    } else {
        None
    };

    let oracle_vocab = OracleVocabulary::from_creature_members(vocabulary.members());
    let mut nonland_names = None;

    let mut next_cache_cards = BTreeMap::new();
    let mut reused_cached_cards = 0usize;
    let mut recompiled_cards = 0usize;

    let mut ordinary_required = 0usize;
    let mut ordinary_stage1 = 0usize;
    let mut ordinary_stage2 = 0usize;
    let mut ordinary_by_format: BTreeMap<String, FormatSupportCounts> = FORMATS
        .into_iter()
        .map(|fmt| (fmt.to_owned(), FormatSupportCounts::default()))
        .collect();
    let mut blocker_counts: BTreeMap<String, usize> = BTreeMap::new();

    let mut preview_by_format: BTreeMap<String, FormatSupportCounts> = PREVIEW_FORMATS
        .into_iter()
        .map(|fmt| (fmt.to_owned(), FormatSupportCounts::default()))
        .collect();
    let mut preview_only_support = FormatSupportCounts::default();

    for (index, card) in inventory.cards.iter().enumerate() {
        let preview_eligible = inventory.is_preview_eligible_at(index);
        if !card.required_by_base_formats && !preview_eligible {
            continue;
        }

        let evaluation = if let Some(oracle_id) = card.oracle_id.as_deref() {
            let semantic_sha256 = current_state
                .cards
                .get(oracle_id)
                .map(|entry| entry.semantic_sha256.clone())
                .map_or_else(|| card_semantic_sha256(inventory, index), Ok)?;
            let cached = usable_cache
                .and_then(|cards| cards.get(oracle_id))
                .filter(|entry| entry.semantic_sha256 == semantic_sha256);
            let result = if let Some(hit) = cached {
                reused_cached_cards += 1;
                hit.clone()
            } else {
                recompiled_cards += 1;
                let stage1_ast_parsed = eval_stage1_ast(card, &oracle_vocab);
                let (stage2_engine_bound, blocker_reason) = match compile_current_card_for_refresh(
                    card,
                    inventory,
                    vocabulary,
                    &mut nonland_names,
                ) {
                    Ok(_) => (true, None),
                    Err(reason) => (false, Some(reason)),
                };
                CachedCardSupport {
                    semantic_sha256,
                    stage1_ast_parsed,
                    stage2_engine_bound,
                    blocker_reason,
                }
            };
            next_cache_cards.insert(oracle_id.to_owned(), result.clone());
            result
        } else {
            recompiled_cards += 1;
            let stage1_ast_parsed = eval_stage1_ast(card, &oracle_vocab);
            CachedCardSupport {
                semantic_sha256: String::new(),
                stage1_ast_parsed,
                stage2_engine_bound: false,
                blocker_reason: Some("missing-oracle-id".to_owned()),
            }
        };

        if card.required_by_base_formats {
            ordinary_required += 1;
            if evaluation.stage1_ast_parsed {
                ordinary_stage1 += 1;
            }
            if evaluation.stage2_engine_bound {
                ordinary_stage2 += 1;
            } else if let Some(reason) = &evaluation.blocker_reason {
                *blocker_counts.entry(reason.clone()).or_insert(0) += 1;
            }
            for format in &card.eligible_formats {
                if let Some(counts) = ordinary_by_format.get_mut(format) {
                    counts.eligible += 1;
                    if evaluation.stage1_ast_parsed {
                        counts.stage1_ast_parsed += 1;
                    }
                    if evaluation.stage2_engine_bound {
                        counts.stage2_engine_bound += 1;
                    }
                }
            }
        } else if preview_eligible {
            preview_only_support.eligible += 1;
            if evaluation.stage1_ast_parsed {
                preview_only_support.stage1_ast_parsed += 1;
            }
            if evaluation.stage2_engine_bound {
                preview_only_support.stage2_engine_bound += 1;
            }
        }

        if let Some(preview_map) = inventory.preview_legalities_at(index) {
            for (format, status) in preview_map {
                if status.is_eligible()
                    && let Some(counts) = preview_by_format.get_mut(format)
                {
                    counts.eligible += 1;
                    if evaluation.stage1_ast_parsed {
                        counts.stage1_ast_parsed += 1;
                    }
                    if evaluation.stage2_engine_bound {
                        counts.stage2_engine_bound += 1;
                    }
                }
            }
        }
    }

    let mut top_blockers: Vec<BlockerSummary> = blocker_counts
        .into_iter()
        .map(|(reason, count)| BlockerSummary { reason, count })
        .collect();
    top_blockers.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.reason.cmp(&b.reason)));
    top_blockers.truncate(25);

    let preview_inv = inventory.preview_inventory();
    let report = CatalogSupportReport {
        schema_version: 1,
        compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
        source: inventory.source.clone(),
        compressed_sha256: inventory.compressed_sha256.clone(),
        creature_vocabulary_id: vocabulary.id(),
        incremental: IncrementalEvaluationSummary {
            diff,
            reused_cached_cards,
            recompiled_cards,
            cache_compatible,
        },
        ordinary_base_formats: OrdinarySupportSummary {
            formats: FORMATS.into_iter().map(str::to_owned).collect(),
            source_records: inventory.source_records,
            required_records: ordinary_required,
            stage1_ast_parsed: ordinary_stage1,
            stage1_ast_failed: ordinary_required.saturating_sub(ordinary_stage1),
            stage2_engine_bound: ordinary_stage2,
            stage2_engine_blocked: ordinary_required.saturating_sub(ordinary_stage2),
            stage3_game_qualified: 0,
            stage3_qualification_state: "not-evaluated-by-compiler".to_owned(),
            by_format: ordinary_by_format,
            top_blockers,
        },
        preview_formats: PreviewSupportSummary {
            formats: preview_inv.formats,
            isolated_from_ordinary_legality: preview_inv.isolated_from_ordinary_legality,
            eligible_by_format: preview_inv.eligible_by_format,
            preview_only_records: preview_inv.preview_only_records,
            preview_not_in_standard: preview_inv.preview_not_in_standard,
            ordinary_overlap_records: preview_inv.ordinary_overlap_records,
            unknown_preview_legality_records: preview_inv.unknown_preview_legality_records,
            by_format: preview_by_format,
            preview_only_support,
        },
    };

    let next_cache = CatalogSupportCache {
        schema_version: SUPPORT_CACHE_SCHEMA_VERSION,
        compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
        creature_vocabulary_id: vocabulary.id(),
        cards: next_cache_cards,
    };

    Ok((report, current_state, next_cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_content::{inventory, refresh::RefreshStatus};
    use std::io::Cursor;

    fn test_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test-catalog-support-vocab".into(),
            [7u8; 32],
            vec!["Bear".into(), "Elf".into(), "Human".into()],
        )
        .unwrap()
    }

    fn meta(updated_at: &str) -> BulkMetadata {
        BulkMetadata {
            object: "bulk_data".into(),
            id: "27bf3214-1271-490b-bdfe-c0be6c23d02e".into(),
            kind: "oracle_cards".into(),
            updated_at: updated_at.into(),
            jsonl_download_uri:
                "https://data.scryfall.io/oracle-cards/oracle-cards-fixture.jsonl.gz".into(),
            compressed_size: 128,
        }
    }

    #[test]
    fn incremental_evaluation_matches_full_evaluation_and_isolates_preview_cards() {
        // Unoptimized debug frames in `spellwake-oracle-compiler` + `oracle_lowering` exceed
        // libtest's default 2 MiB worker stack; run with an explicit 16 MiB stack so plain
        // `cargo test --locked --workspace` passes without requiring `RUST_MIN_STACK`.
        let handle = std::thread::Builder::new()
            .name("catalog-support-test".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(run_incremental_evaluation_test)
            .unwrap();
        handle.join().unwrap();
    }

    fn run_incremental_evaluation_test() {
        let vocab = test_vocab();
        let bear = serde_json::json!({
            "object": "card",
            "id": "bear-1",
            "oracle_id": "11111111-1111-1111-1111-111111111111",
            "name": "Grizzly Bears",
            "layout": "normal",
            "type_line": "Creature \u{2014} Bear",
            "mana_cost": "{1}{G}",
            "power": "2",
            "toughness": "2",
            "oracle_text": "",
            "legalities": {
                "commander": "legal",
                "standard": "not_legal",
                "pioneer": "not_legal",
                "modern": "legal",
                "legacy": "legal",
                "vintage": "legal",
                "future": "not_legal"
            }
        });
        let preview_spoiler = serde_json::json!({
            "object": "card",
            "id": "spoiler-1",
            "oracle_id": "22222222-2222-2222-2222-222222222222",
            "name": "Future Visionary",
            "layout": "normal",
            "type_line": "Creature \u{2014} Elf",
            "mana_cost": "{1}{G}",
            "power": "1",
            "toughness": "1",
            "oracle_text": "When Future Visionary enters, draw a card.",
            "legalities": {
                "commander": "not_legal",
                "standard": "not_legal",
                "pioneer": "not_legal",
                "modern": "not_legal",
                "legacy": "not_legal",
                "vintage": "not_legal",
                "future": "legal"
            }
        });

        let inv1 = inventory(
            Cursor::new(format!("{bear}\n{preview_spoiler}")),
            meta("2026-10-29T21:00:00Z"),
            "a".repeat(64),
        )
        .unwrap();

        let (r1, state1, cache1) = evaluate_catalog_support(&inv1, &vocab, None, None).unwrap();
        assert_eq!(r1.incremental.diff.refresh_status, RefreshStatus::Initial);
        assert_eq!(r1.incremental.reused_cached_cards, 0);
        assert_eq!(r1.incremental.recompiled_cards, 2);
        // Only Grizzly Bears is an ordinary base-format card; Future Visionary is preview-only.
        assert_eq!(r1.ordinary_base_formats.required_records, 1);
        assert_eq!(r1.ordinary_base_formats.stage1_ast_parsed, 1);
        assert_eq!(r1.ordinary_base_formats.stage2_engine_bound, 1);
        assert_eq!(r1.ordinary_base_formats.stage3_game_qualified, 0);
        assert!(r1.preview_formats.isolated_from_ordinary_legality);
        assert_eq!(r1.preview_formats.preview_only_records, 1);
        assert_eq!(r1.preview_formats.preview_not_in_standard, 1);
        assert_eq!(r1.preview_formats.preview_only_support.eligible, 1);
        assert_eq!(r1.preview_formats.preview_only_support.stage1_ast_parsed, 1);
        assert_eq!(
            r1.preview_formats.preview_only_support.stage2_engine_bound,
            1
        );

        // Re-running on unchanged snapshot reuses 100% of cached entries with identical counts.
        let (r_same, _, _) =
            evaluate_catalog_support(&inv1, &vocab, Some(&state1), Some(&cache1)).unwrap();
        assert_eq!(
            r_same.incremental.diff.refresh_status,
            RefreshStatus::Unchanged
        );
        assert_eq!(r_same.incremental.reused_cached_cards, 2);
        assert_eq!(r_same.incremental.recompiled_cards, 0);
        assert_eq!(r_same.ordinary_base_formats, r1.ordinary_base_formats);
        assert_eq!(r_same.preview_formats, r1.preview_formats);

        // Second snapshot: Grizzly Bears unchanged, Future Visionary gets errata text, new card added.
        let mut errata_spoiler = preview_spoiler.clone();
        errata_spoiler["oracle_text"] =
            serde_json::json!("When Future Visionary enters, scry 1, then draw a card.");
        let bolt = serde_json::json!({
            "object": "card",
            "id": "bolt-1",
            "oracle_id": "33333333-3333-3333-3333-333333333333",
            "name": "Lightning Bolt",
            "layout": "normal",
            "type_line": "Instant",
            "mana_cost": "{R}",
            "oracle_text": "Lightning Bolt deals 3 damage to any target.",
            "legalities": {
                "commander": "legal",
                "standard": "not_legal",
                "pioneer": "not_legal",
                "modern": "legal",
                "legacy": "legal",
                "vintage": "legal",
                "future": "not_legal"
            }
        });
        let inv2 = inventory(
            Cursor::new(format!("{bear}\n{errata_spoiler}\n{bolt}")),
            meta("2026-11-05T21:00:00Z"),
            "b".repeat(64),
        )
        .unwrap();

        let (r_inc, _, _) =
            evaluate_catalog_support(&inv2, &vocab, Some(&state1), Some(&cache1)).unwrap();
        let (r_full, _, _) = evaluate_catalog_support(&inv2, &vocab, None, None).unwrap();

        assert_eq!(
            r_inc.incremental.diff.refresh_status,
            RefreshStatus::Updated
        );
        assert_eq!(r_inc.incremental.reused_cached_cards, 1);
        assert_eq!(r_inc.incremental.recompiled_cards, 2);
        assert_eq!(r_inc.ordinary_base_formats, r_full.ordinary_base_formats);
        assert_eq!(r_inc.preview_formats, r_full.preview_formats);
        assert_eq!(r_inc.ordinary_base_formats.required_records, 2);
        assert_eq!(r_inc.ordinary_base_formats.stage2_engine_bound, 2);
    }
}
