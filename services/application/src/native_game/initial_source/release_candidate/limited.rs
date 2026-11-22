//! Deck eligibility from a supplied seat-pool snapshot and an explicit basic-land policy.
use super::{Deck, basic_land, candidate_deck_catalogue, read_bounded};
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use spellwake_card_programs::OracleId;
use spellwake_content::InventoryCard;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
};
use uuid::Uuid;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PoolKind {
    Draft,
    Cube,
}
impl PoolKind {
    fn key(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Cube => "cube",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PoolSelection {
    oracle_ref: OracleId,
    quantity: crate::amount::Amount,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SeatPool {
    schema: String,
    kind: PoolKind,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_revision_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    cards: Vec<PoolSelection>,
    free_basic_lands: Vec<OracleId>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DeckFailure {
    InvalidPool,
    WrongFormat,
    CardUnavailable,
    InvalidQuantity,
    ExceedsPool,
    UnsupportedSection,
    MainTooSmall,
}

fn check(
    pool: &SeatPool,
    deck: &Deck,
    cards: &BTreeMap<&str, &InventoryCard>,
) -> Result<super::DeckCounts, DeckFailure> {
    if pool.schema != "spellwake-limited-seat-pool-v1"
        || pool.cards.is_empty()
        || pool.seat_id.is_nil()
        || pool.source_revision_id.is_nil()
    {
        return Err(DeckFailure::InvalidPool);
    }
    if pool.kind.key() != deck.format {
        return Err(DeckFailure::WrongFormat);
    }
    let mut allocation = BTreeMap::new();
    for row in &pool.cards {
        let oracle = row.oracle_ref.as_str();
        let count = row
            .quantity
            .integer()
            .to_u32()
            .filter(|count| *count > 0)
            .ok_or(DeckFailure::InvalidPool)?;
        if !cards.contains_key(oracle) || allocation.insert(oracle, count).is_some() {
            return Err(DeckFailure::InvalidPool);
        }
    }
    let free: BTreeSet<_> = pool.free_basic_lands.iter().map(OracleId::as_str).collect();
    if free.len() != pool.free_basic_lands.len()
        || free
            .iter()
            .any(|oracle| !cards.get(oracle).is_some_and(|card| basic_land(card)))
    {
        return Err(DeckFailure::InvalidPool);
    }
    let mut counts = super::DeckCounts {
        main: 0,
        sideboard: 0,
    };
    for row in &deck.entries {
        let oracle = row.oracle_ref.as_str();
        if !cards.contains_key(oracle) {
            return Err(DeckFailure::CardUnavailable);
        }
        let quantity = row
            .quantity
            .integer()
            .to_u32()
            .filter(|quantity| *quantity > 0)
            .ok_or(DeckFailure::InvalidQuantity)?;
        if !free.contains(oracle) {
            let remaining = allocation.get_mut(oracle).ok_or(DeckFailure::ExceedsPool)?;
            *remaining = remaining
                .checked_sub(quantity)
                .ok_or(DeckFailure::ExceedsPool)?;
        }
        let count = match row.section.as_str() {
            "main" => &mut counts.main,
            "sideboard" => &mut counts.sideboard,
            _ => return Err(DeckFailure::UnsupportedSection),
        };
        *count = count
            .checked_add(quantity)
            .ok_or(DeckFailure::InvalidQuantity)?;
    }
    if counts.main < 40 {
        return Err(DeckFailure::MainTooSmall);
    }
    Ok(counts)
}

pub(super) fn check_match_sections(
    entries: &[super::Entry],
) -> Result<super::DeckCounts, super::DeckFailure> {
    let mut counts = super::DeckCounts {
        main: 0,
        sideboard: 0,
    };
    for entry in entries {
        let quantity = entry
            .quantity
            .integer()
            .to_u32()
            .filter(|quantity| *quantity > 0)
            .ok_or(super::DeckFailure::InvalidQuantity)?;
        let count = match entry.section.as_str() {
            "main" => &mut counts.main,
            "sideboard" => &mut counts.sideboard,
            _ => return Err(super::DeckFailure::UnsupportedSection),
        };
        *count = count
            .checked_add(quantity)
            .ok_or(super::DeckFailure::InvalidQuantity)?;
    }
    if counts.main < 40 {
        return Err(super::DeckFailure::MainTooSmall);
    }
    Ok(counts)
}

/// Inspect Draft or Cube deck eligibility for a supplied pool snapshot.
/// The report pins the input bytes for review. Production still establishes
/// how the pool was allocated, its owner, and the executable release.
/// # Errors
/// Rejects unreadable inputs, malformed requests and an unpinned Oracle archive.
pub fn inspect_limited_deck(
    metadata: &Path,
    snapshot: &Path,
    pool: &Path,
    deck: &Path,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let pool_bytes = read_bounded(pool, 262_144)?;
    let pool: SeatPool = crate::decode_wire_object(&pool_bytes)?;
    let deck_bytes = read_bounded(deck, 262_144)?;
    let deck: Deck = crate::decode_wire_object(&deck_bytes)?;
    let inventory = candidate_deck_catalogue(metadata, snapshot)?;
    let ids: BTreeSet<_> = deck
        .entries
        .iter()
        .map(|row| row.oracle_ref.as_str())
        .chain(pool.cards.iter().map(|row| row.oracle_ref.as_str()))
        .chain(pool.free_basic_lands.iter().map(OracleId::as_str))
        .collect();
    let cards = inventory
        .cards
        .iter()
        .filter_map(|card| {
            card.oracle_id
                .as_deref()
                .filter(|id| ids.contains(id))
                .map(|id| (id, card))
        })
        .collect();
    Ok(serde_json::json!({
        "scope":"deck eligibility against a supplied draft or cube seat pool",
        "catalogueSha256":inventory.compressed_sha256,
        "poolInputHash":super::digest(&pool_bytes),
        "deckInputHash":super::digest(&deck_bytes),
        "sourceKind":pool.kind,
        "sourceRevisionId":pool.source_revision_id,
        "seatId":pool.seat_id,
        "deckConstruction":check(&pool, &deck, &cards),
        "applicationRelease":null
    }))
}
