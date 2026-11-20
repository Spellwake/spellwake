//! One source-qualified catalogue fixture materializer for the finite pools.
use super::fixture::{A, Alias, B, Definition, SourceIdentity};
use serde::Deserialize;
use spellwake_card_programs::{OracleId, Registry};
use spellwake_content::Inventory;
use spellwake_engine::{
    effects::SeedCard,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InventoryEntry {
    pub(super) role: String,
    pub(super) oracle_id: String,
    pub(super) name: String,
    pub(super) copy: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Deck {
    pub(super) name: String,
    pub(super) entries: Vec<DeckEntry>,
    pub(super) card_count: usize,
    pub(super) sideboard: Vec<serde_json::Value>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeckEntry {
    pub(super) name: String,
    pub(super) count: u32,
    pub(super) oracle_id: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DefinitionInput {
    pub(super) name: String,
    pub(super) oracle_id: String,
    pub(super) scryfall_id: String,
    pub(super) source_line_one_based: u64,
    pub(super) raw_line_sha256_including_lf: String,
    pub(super) registry_signature_v1_sha256: String,
    pub(super) set: String,
    pub(super) collector: String,
    pub(super) language: String,
}

/// Materialized source-backed physical inventory, before profile-specific zones.
pub(super) struct Materialized {
    pub(super) aliases: BTreeMap<CardId, Alias>,
    pub(super) cards: Vec<SeedCard>,
    pub(super) definitions: Vec<Definition>,
}
/// Single owner for each curated two-deck physical input shape.
pub(super) fn validate_shape(
    source_definitions: &[DefinitionInput],
    decks: &[Deck],
    declared_inventory: &BTreeMap<String, InventoryEntry>,
) -> Result<(), String> {
    if source_definitions.len() != 20 || decks.len() != 2 || declared_inventory.len() != 120 {
        return Err("incomplete compiled teaching inventory".into());
    }
    for deck in decks {
        if deck.card_count != 60
            || deck.entries.len() != 10
            || !deck.sideboard.is_empty()
            || deck.entries.iter().map(|e| e.count as usize).sum::<usize>() != 60
            || deck.name.is_empty()
        {
            return Err("invalid teaching deck shape".into());
        }
    }
    Ok(())
}
/// One declared physical member, shared by old decks and mandatory prepared cases.
#[derive(Debug, Clone)]
pub(super) struct PhysicalRow {
    pub(super) alias: String,
    pub(super) card: CardId,
    pub(super) owner: SeatId,
    pub(super) oracle_id: String,
    pub(super) copy: u32,
}
/// Closed source and physical shapes. No caller-specified capacity bypass.
#[derive(Debug, Clone, Copy)]
pub(super) enum Shape {
    HistoricalDeckPair,
    MandatoryDeckPair,
    MandatoryPrepared,
    OptionalPaymentDeckPair,
    OptionalPaymentPrepared,
    ResolutionSelectionPrepared,
    LibraryTopDeckPair,
    LibraryTopPrepared,
}

/// Shared physical identity/definition validation. Registry remains the sole Program owner.
pub(super) fn materialize(
    inventory: &Inventory,
    registry: &Registry,
    source_definitions: &[DefinitionInput],
    decks: &[Deck],
    declared_inventory: &BTreeMap<String, InventoryEntry>,
    roles: [&str; 2],
) -> Result<Materialized, String> {
    validate_shape(source_definitions, decks, declared_inventory)?;
    let mut rows = vec![];
    for (deck, owner, offset, role) in [
        (&decks[0], A, 0u64, roles[0]),
        (&decks[1], B, 100u64, roles[1]),
    ] {
        let mut ordinal = 0u64;
        let mut seen = BTreeSet::new();
        for entry in &deck.entries {
            if entry.count == 0 || !seen.insert(&entry.oracle_id) {
                return Err("duplicate or empty deck identity".into());
            }
            for copy in 1..=entry.count {
                ordinal = ordinal.checked_add(1).ok_or("physical ordinal overflow")?;
                let alias = format!("{role}.{}.{copy}", slug(&entry.name));
                let declared = declared_inventory
                    .get(&alias)
                    .ok_or("teaching alias missing")?;
                if declared.role != role
                    || declared.oracle_id != entry.oracle_id
                    || declared.name != entry.name
                    || declared.copy != copy
                {
                    return Err("teaching alias identity mismatch".into());
                }
                rows.push(PhysicalRow {
                    alias,
                    card: CardId(offset + ordinal),
                    owner,
                    oracle_id: entry.oracle_id.clone(),
                    copy,
                });
            }
        }
    }
    materialize_rows(
        inventory,
        registry,
        source_definitions,
        &rows,
        Shape::HistoricalDeckPair,
    )
}

/// Validate every selected source definition before constructing any physical instance.
/// All callers then use this single ordered Registry::seed path.
pub(super) fn materialize_rows(
    inventory: &Inventory,
    registry: &Registry,
    sources: &[DefinitionInput],
    rows: &[PhysicalRow],
    shape: Shape,
) -> Result<Materialized, String> {
    let required = match shape {
        Shape::HistoricalDeckPair => 20,
        Shape::ResolutionSelectionPrepared => 9,
        Shape::LibraryTopDeckPair | Shape::LibraryTopPrepared => 4,
        Shape::MandatoryDeckPair
        | Shape::MandatoryPrepared
        | Shape::OptionalPaymentDeckPair
        | Shape::OptionalPaymentPrepared => 19,
    };
    let selected = sources
        .iter()
        .map(|s| &s.oracle_id)
        .collect::<BTreeSet<_>>();
    if sources.len() != required
        || selected.len() != required
        || rows.is_empty()
        || rows.len() > 120
    {
        return Err("catalogue complete source/physical shape mismatch".into());
    }
    let actual_bound = registry
        .coverage()
        .cards
        .iter()
        .filter(|c| {
            matches!(
                c.binding,
                spellwake_card_programs::BindingStatus::Partial { .. }
            )
        })
        .filter_map(|c| c.oracle_id.as_ref())
        .collect::<BTreeSet<_>>();
    if selected != actual_bound {
        return Err("catalogue selected definitions differ from guarded registry".into());
    }
    for source in sources {
        let matching = inventory
            .cards
            .iter()
            .filter(|v| v.oracle_id.as_deref() == Some(source.oracle_id.as_str()))
            .collect::<Vec<_>>();
        let [actual] = matching.as_slice() else {
            return Err("catalogue source identity missing or duplicated".into());
        };
        let coverage = registry
            .coverage()
            .cards
            .iter()
            .find(|c| c.oracle_id.as_deref() == Some(source.oracle_id.as_str()))
            .ok_or("catalogue coverage missing")?;
        if actual.scryfall_id != source.scryfall_id
            || actual.name != source.name
            || source.source_line_one_based == 0
            || !is_digest(&source.raw_line_sha256_including_lf)
            || !is_digest(&source.registry_signature_v1_sha256)
            || !matches!(&coverage.binding, spellwake_card_programs::BindingStatus::Partial { semantic_sha256, .. } if semantic_sha256 == &source.registry_signature_v1_sha256)
        {
            return Err("catalogue source/printing/signature mismatch".into());
        }
    }
    let used = rows.iter().map(|r| &r.oracle_id).collect::<BTreeSet<_>>();
    match shape {
        Shape::HistoricalDeckPair if rows.len() != 120 || used.len() != 20 => {
            return Err("historical physical domain mismatch".into());
        }
        Shape::MandatoryDeckPair if rows.len() != 120 || used.len() != 16 => {
            return Err("mandatory ordinary physical domain mismatch".into());
        }
        Shape::OptionalPaymentDeckPair
            if rows.len() != 120
                || used.len() != 18
                || used
                    != selected
                        .iter()
                        .copied()
                        .filter(|id| id.as_str() != spellwake_card_programs::TWINCAST)
                        .collect::<BTreeSet<_>>() =>
        {
            return Err("optional/payment ordinary physical domain mismatch".into());
        }
        _ => {}
    }
    // Complete physical validation precedes every Program seed construction.
    let mut seen_aliases = BTreeSet::new();
    let mut seen_cards = BTreeSet::new();
    let mut copies = BTreeMap::<(SeatId, &str), BTreeSet<u32>>::new();
    for row in rows {
        if !sources
            .iter()
            .any(|source| source.oracle_id == row.oracle_id)
            || ![A, B].contains(&row.owner)
            || row.copy == 0
            || row.alias.is_empty()
            || !seen_aliases.insert(row.alias.as_str())
            || !seen_cards.insert(row.card)
            || !copies
                .entry((row.owner, row.oracle_id.as_str()))
                .or_default()
                .insert(row.copy)
        {
            return Err("duplicate or foreign physical identity".into());
        }
    }
    for ordinals in copies.values() {
        if ordinals
            .iter()
            .copied()
            .ne(1..=u32::try_from(ordinals.len()).map_err(|_| "copy inventory overflow")?)
        {
            return Err("noncontiguous physical copy labels".into());
        }
    }
    if matches!(shape, Shape::LibraryTopDeckPair) {
        for owner in [A, B] {
            if rows.iter().filter(|row| row.owner == owner).count() != 60 {
                return Err("library-top physical deck must contain sixty cards per seat".into());
            }
        }
        for ((_, oracle), ordinals) in &copies {
            if *oracle != "b2c6aa39-2d2a-459c-a555-fb48ba993373" && ordinals.len() > 4 {
                return Err("library-top physical spell copy limit exceeded".into());
            }
        }
    }
    let mut aliases = BTreeMap::new();
    let mut cards = vec![];
    let mut definitions = vec![];
    let mut defined = BTreeSet::new();
    for row in rows {
        let source = sources
            .iter()
            .find(|s| s.oracle_id == row.oracle_id)
            .ok_or("validated physical definition absent")?;
        let id = OracleId::try_from(row.oracle_id.clone()).map_err(|e| e.to_string())?;
        let seed = registry
            .seed(&id, row.card, row.owner, ObjectDestination::LibraryBottom)
            .map_err(|e| e.to_string())?;
        if defined.insert(&row.oracle_id) {
            definitions.push(Definition {
                reference: row.oracle_id.clone(),
                name: source.name.clone(),
                oracle_id: row.oracle_id.clone(),
                printing_id: source.scryfall_id.clone(),
                set: source.set.clone(),
                collector: source.collector.clone(),
                language: source.language.clone(),
                sources: vec![SourceIdentity {
                    path: "work/catalogue/oracle-cards-20261029210158.jsonl.gz".into(),
                    sha256: inventory.compressed_sha256.clone(),
                    uri: inventory.source.jsonl_download_uri.clone(),
                }],
                program: seed.program.clone(),
            });
        }
        aliases.insert(
            row.card,
            Alias {
                alias: row.alias.clone(),
                card: row.card,
                owner: row.owner,
                definition_ref: row.oracle_id.clone(),
            },
        );
        cards.push(seed);
    }
    Ok(Materialized {
        aliases,
        cards,
        definitions,
    })
}
fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn slug(name: &str) -> String {
    name.to_ascii_lowercase().replace(' ', "-")
}

/// Native-facing prepared zone lists; no role codec or new source interpreter.
pub(super) struct PreparedZone<'a> {
    pub(super) owner: SeatId,
    pub(super) hand: &'a [String],
    pub(super) library_top_first: &'a [String],
    pub(super) graveyard_top_first: &'a [String],
    pub(super) exile: &'a [String],
}
/// One truthful seeded battlefield premise, in supplied timestamp order.
pub(super) struct PreparedBattlefield<'a> {
    pub(super) alias: &'a str,
    pub(super) controller: SeatId,
    pub(super) tapped: bool,
}
/// Shared prepared partition/resource input. Callers keep their own strict wire grammar.
pub(super) struct PreparedSetup<'a> {
    pub(super) start: spellwake_engine::turn::TurnStart,
    pub(super) active: SeatId,
    pub(super) life: Vec<spellwake_engine::setup::StartingLife>,
    pub(super) zones: &'a [PreparedZone<'a>],
    pub(super) battlefield_oldest_first: &'a [PreparedBattlefield<'a>],
    pub(super) mana: Vec<spellwake_engine::mana::FloatingMana>,
}
/// Source materialization with prepared destinations and explicit native resources.
pub(super) struct PreparedMaterialized {
    pub(super) materialized: Materialized,
    pub(super) start: spellwake_engine::turn::TurnStart,
    pub(super) active: SeatId,
    pub(super) life: Vec<spellwake_engine::setup::StartingLife>,
    pub(super) initial_battlefield_order: Vec<CardId>,
    pub(super) mana: Vec<spellwake_engine::mana::FloatingMana>,
}
/// Lower one complete two-seat prepared partition using the existing seeded Programs.
///
/// This does not initialize a game, issue ETBs, imply past control, tap a permanent,
/// generate mana or create aliases. Library lists are top first; graveyard lists
/// are reversed only for the native sequential insertion contract.
///
/// # Errors
/// Rejects incomplete/duplicate/foreign partitions, unsupported tapped premises,
/// invalid participants/resources or inconsistent materialized identities.
pub(super) fn prepare_materialized(
    materialized: Materialized,
    setup: PreparedSetup<'_>,
) -> Result<PreparedMaterialized, String> {
    prepare_with_policy(materialized, setup, PreparedPolicy::StrictSelection)
        .map_err(|error| error.strict_message())
}

/// Structured partition failures retain the actual missing alias for legacy callers.
/// The strict selection adapter keeps its existing String messages unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreparedError {
    ParticipantPartition,
    InvalidMana,
    InconsistentInventory,
    UnknownZoneAlias(String),
    DuplicateOrForeignZone,
    UnknownBattlefieldAlias(String),
    UnsupportedBattlefield,
    IncompletePartition,
    MissingSeed,
}
impl PreparedError {
    pub(super) fn strict_message(&self) -> String {
        match self {
            Self::ParticipantPartition => "prepared participant/resource partition mismatch",
            Self::InvalidMana => "prepared invalid floating mana",
            Self::InconsistentInventory => "prepared inconsistent materialized physical inventory",
            Self::UnknownZoneAlias(_) => "prepared unknown zone alias",
            Self::DuplicateOrForeignZone => "prepared duplicate/foreign zone member",
            Self::UnknownBattlefieldAlias(_) => "prepared unknown battlefield alias",
            Self::UnsupportedBattlefield => "prepared unsupported battlefield premise",
            Self::IncompletePartition => "prepared incomplete physical partition",
            Self::MissingSeed => "prepared seed absent",
        }
        .into()
    }
}

/// Closed construction contracts, independent of every wire/recipe grammar.
#[derive(Clone, Copy)]
enum PreparedPolicy {
    StrictSelection,
    // Historical Scheduled fixture construction leaves turn-zero rejection to
    // native initialization and supplies no seeded floating mana.
    LegacyScheduled,
}

/// Reuse the single partition owner without changing historical validation timing.
///
/// # Errors
/// Returns a structured error for a malformed two-seat partition or unsupported
/// seeded resource. Legacy callers retain their own error text at their boundary.
pub(super) fn prepare_legacy_scheduled(
    materialized: Materialized,
    setup: PreparedSetup<'_>,
) -> Result<PreparedMaterialized, PreparedError> {
    prepare_with_policy(materialized, setup, PreparedPolicy::LegacyScheduled)
}

fn prepare_with_policy(
    materialized: Materialized,
    setup: PreparedSetup<'_>,
    policy: PreparedPolicy,
) -> Result<PreparedMaterialized, PreparedError> {
    let participants = [A, B];
    if (matches!(policy, PreparedPolicy::StrictSelection) && setup.start.number == 0)
        || !participants.contains(&setup.active)
        || setup.zones.len() != 2
        || setup.zones[0].owner != A
        || setup.zones[1].owner != B
        || setup.life.len() != 2
        || setup
            .life
            .iter()
            .map(|row| row.player)
            .collect::<BTreeSet<_>>()
            != BTreeSet::from(participants)
    {
        return Err(PreparedError::ParticipantPartition);
    }
    if matches!(policy, PreparedPolicy::LegacyScheduled) && !setup.mana.is_empty() {
        return Err(PreparedError::InvalidMana);
    }
    let mut pools = BTreeSet::new();
    for pool in &setup.mana {
        if !participants.contains(&pool.player)
            || pool.amount < 0.into()
            || !pools.insert((pool.player, pool.mana_type))
        {
            return Err(PreparedError::InvalidMana);
        }
    }
    let Materialized {
        aliases,
        cards,
        definitions,
    } = materialized;
    let by_alias = aliases
        .values()
        .map(|row| (row.alias.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    let card_count = cards.len();
    let mut original = cards
        .into_iter()
        .map(|card| (card.card, card))
        .collect::<BTreeMap<_, _>>();
    if by_alias.len() != aliases.len()
        || original.len() != card_count
        || original.len() != aliases.len()
        || aliases.iter().any(|(card, row)| {
            row.card != *card
                || !participants.contains(&row.owner)
                || original
                    .get(card)
                    .is_none_or(|seed| seed.owner != row.owner)
        })
    {
        return Err(PreparedError::InconsistentInventory);
    }
    let mut assignments = vec![];
    let mut seen = BTreeSet::new();
    for zone in setup.zones {
        for (list, destination, reverse) in [
            (zone.hand, ObjectDestination::Hand, false),
            (
                zone.library_top_first,
                ObjectDestination::LibraryBottom,
                false,
            ),
            (zone.graveyard_top_first, ObjectDestination::Graveyard, true),
            (zone.exile, ObjectDestination::Exile, false),
        ] {
            let mut ordered = list.iter().collect::<Vec<_>>();
            if reverse {
                ordered.reverse();
            }
            for alias in ordered {
                let row = by_alias
                    .get(alias.as_str())
                    .ok_or_else(|| PreparedError::UnknownZoneAlias(alias.clone()))?;
                if row.owner != zone.owner || !seen.insert(row.card) {
                    return Err(PreparedError::DuplicateOrForeignZone);
                }
                assignments.push((row.card, destination));
            }
        }
    }
    let mut initial_battlefield_order = vec![];
    for premise in setup.battlefield_oldest_first {
        let row = by_alias
            .get(premise.alias)
            .ok_or_else(|| PreparedError::UnknownBattlefieldAlias(premise.alias.into()))?;
        if premise.tapped || !participants.contains(&premise.controller) || !seen.insert(row.card) {
            return Err(PreparedError::UnsupportedBattlefield);
        }
        assignments.push((
            row.card,
            ObjectDestination::Battlefield {
                controller: premise.controller,
            },
        ));
        initial_battlefield_order.push(row.card);
    }
    if seen.len() != aliases.len() {
        return Err(PreparedError::IncompletePartition);
    }
    let cards = assignments
        .into_iter()
        .map(|(card, destination)| {
            let mut seed = original.remove(&card).ok_or(PreparedError::MissingSeed)?;
            seed.destination = destination;
            Ok(seed)
        })
        .collect::<Result<Vec<_>, PreparedError>>()?;
    Ok(PreparedMaterialized {
        materialized: Materialized {
            aliases,
            cards,
            definitions,
        },
        start: setup.start,
        active: setup.active,
        life: setup.life,
        initial_battlefield_order,
        mana: setup.mana,
    })
}
