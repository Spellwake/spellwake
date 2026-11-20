//! Shared immutable byte and exact raw-row authentication for closed native source families.
use super::catalogue::DefinitionInput;
use crate::packet;
use flate2::read::MultiGzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    LIBRARY_TOP_PROFILE, LIBRARY_TOP_REGISTRY_VERSION, OracleId, RESOLUTION_SELECTION_PROFILE,
    RESOLUTION_SELECTION_REGISTRY_VERSION, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader},
};
const METADATA_SHA256: &str = "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad";
const SOURCE_RECORDS: u64 = 39_196;
/// Closed source family; descriptors cannot choose their own admission capacity.
#[derive(Clone, Copy)]
pub(super) enum Family {
    Selection,
    LibraryTop,
}
impl Family {
    fn label(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::LibraryTop => "library-top",
        }
    }
    fn header_error(self) -> &'static str {
        match self {
            Self::Selection => "selection descriptor closed source9 header mismatch",
            Self::LibraryTop => "library-top descriptor closed source4 header mismatch",
        }
    }
    fn count(self) -> usize {
        match self {
            Self::Selection => 9,
            Self::LibraryTop => 4,
        }
    }
    fn profile(self) -> &'static str {
        match self {
            Self::Selection => RESOLUTION_SELECTION_PROFILE,
            Self::LibraryTop => LIBRARY_TOP_PROFILE,
        }
    }
    fn revision(self) -> u32 {
        match self {
            Self::Selection => RESOLUTION_SELECTION_REGISTRY_VERSION,
            Self::LibraryTop => LIBRARY_TOP_REGISTRY_VERSION,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceDefinitions {
    pub(super) schema_version: u32,
    pub(super) profile: String,
    pub(super) registry_version: u32,
    pub(super) metadata_bytes: u64,
    pub(super) metadata_sha256: String,
    pub(super) snapshot_bytes: u64,
    pub(super) snapshot_sha256: String,
    pub(super) source_records: u64,
    pub(super) creature_vocabulary_id: String,
    pub(super) definitions: Vec<DefinitionInput>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn descriptor(bytes: &[u8], family: Family) -> Result<SourceDefinitions, String> {
    let value = packet::unique_json(bytes)
        .map_err(|e| format!("{} descriptor JSON: {e}", family.label()))?;
    let value: SourceDefinitions = serde_json::from_value(value)
        .map_err(|e| format!("{} descriptor schema: {e}", family.label()))?;
    if value.schema_version != 1
        || value.profile != family.profile()
        || value.registry_version != family.revision()
        || value.metadata_bytes != 565
        || value.metadata_sha256 != METADATA_SHA256
        || value.snapshot_bytes != 25_229_019
        || value.snapshot_sha256 != TEACHING_SOURCE_SHA256
        || value.source_records != SOURCE_RECORDS
        || value.creature_vocabulary_id != TEACHING_VOCABULARY_ID
        || value.definitions.len() != family.count()
    {
        return Err(family.header_error().into());
    }
    let mut oracle = BTreeSet::new();
    let mut printing = BTreeSet::new();
    let mut lines = BTreeSet::new();
    for row in &value.definitions {
        OracleId::try_from(row.oracle_id.clone())
            .map_err(|e| format!("{} descriptor Oracle ID: {e}", family.label()))?;
        if !oracle.insert(&row.oracle_id)
            || !printing.insert(&row.scryfall_id)
            || !lines.insert(row.source_line_one_based)
            || row.source_line_one_based == 0
            || row.source_line_one_based > SOURCE_RECORDS
        {
            return Err(format!(
                "{} descriptor duplicate or invalid source coordinate",
                family.label()
            ));
        }
    }
    Ok(value)
}

pub(super) fn authenticate_bytes(
    metadata: &[u8],
    snapshot: &[u8],
    definitions: &SourceDefinitions,
    vocabulary: &CreatureTypeVocabulary,
    family: Family,
) -> Result<(), String> {
    if metadata.len() as u64 != definitions.metadata_bytes
        || digest(metadata) != definitions.metadata_sha256
    {
        return Err(format!("{} metadata bytes mismatch", family.label()));
    }
    if snapshot.len() as u64 != definitions.snapshot_bytes
        || digest(snapshot) != definitions.snapshot_sha256
    {
        return Err(format!("{} snapshot bytes mismatch", family.label()));
    }
    // Arc supplies the native immutable vocabulary, not authority from a DTO ID.
    // Its file-byte provenance belongs to the vocabulary's upstream loader.
    if vocabulary.id().to_string() != definitions.creature_vocabulary_id {
        return Err(format!("{} native vocabulary mismatch", family.label()));
    }
    Ok(())
}

pub(super) fn verify_raw_line(
    row: &DefinitionInput,
    number: u64,
    bytes: &[u8],
    family: Family,
) -> Result<(), String> {
    if number != row.source_line_one_based
        || !bytes.ends_with(b"\n")
        || digest(bytes) != row.raw_line_sha256_including_lf
    {
        return Err(format!(
            "{} exact raw line/LF hash mismatch",
            family.label()
        ));
    }
    let value =
        packet::unique_json(bytes).map_err(|e| format!("{} raw JSON: {e}", family.label()))?;
    for (key, expected) in [
        ("name", &row.name),
        ("oracle_id", &row.oracle_id),
        ("id", &row.scryfall_id),
        ("set", &row.set),
        ("collector_number", &row.collector),
        ("lang", &row.language),
    ] {
        if value.get(key).and_then(serde_json::Value::as_str) != Some(expected.as_str()) {
            return Err(format!(
                "{} raw source field mismatch: {key}",
                family.label()
            ));
        }
    }
    Ok(())
}

pub(super) fn verify_raw_rows(
    snapshot: &[u8],
    definitions: &[DefinitionInput],
    family: Family,
) -> Result<(), String> {
    let pending: BTreeMap<_, _> = definitions
        .iter()
        .map(|row| (row.source_line_one_based, row))
        .collect();
    let mut reader = BufReader::new(MultiGzDecoder::new(snapshot));
    let mut bytes = Vec::new();
    let mut number = 0_u64;
    let mut matched = 0;
    loop {
        bytes.clear();
        if reader
            .read_until(b'\n', &mut bytes)
            .map_err(|e| format!("{} raw source read: {e}", family.label()))?
            == 0
        {
            break;
        }
        number += 1;
        if number > SOURCE_RECORDS {
            return Err(format!(
                "{} raw source record count mismatch",
                family.label()
            ));
        }
        if let Some(row) = pending.get(&number) {
            verify_raw_line(row, number, &bytes, family)?;
            matched += 1;
        }
    }
    if number != SOURCE_RECORDS || matched != family.count() {
        return Err(format!(
            "{} raw source record/selected row count mismatch",
            family.label()
        ));
    }
    Ok(())
}
