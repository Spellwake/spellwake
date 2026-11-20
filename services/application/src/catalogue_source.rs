//! Source-bound local catalogue staging. A complete stage is not a database publication,
//! an executable-card qualification, or evidence about physical booster supply.
use crate::price_source::{
    BulkArtifact, EvidenceHash, SourceError, hash_file, hash_text, parse_index, visit_artifact,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(crate) const PROJECTION: &str = "scryfall-catalogue-metadata-v1";
pub(crate) const MAX_STAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Original acquisition assertions, checked against both retained inputs again.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogueSource {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub metadata: BulkArtifact,
    pub acquired_at: DateTime<Utc>,
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub hash: EvidenceHash,
    pub metadata_hash: EvidenceHash,
    pub qualification_path: PathBuf,
    pub compressed_bytes: u64,
    pub decompressed_bytes: u64,
    pub record_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Complete local projection evidence. Consumers must recheck these hashes and
/// counts before accepting staged bytes; this is not a database publication ID.
pub struct CatalogueStage {
    pub projection: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub source: CatalogueSource,
    pub records_path: PathBuf,
    pub records_hash: EvidenceHash,
    pub records_bytes: u64,
    pub supported_records: u64,
    pub unresolved_records: u64,
    pub printing_count: u64,
    pub definition_count: u64,
    pub set_count: u64,
    pub unresolved_reasons: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceSet {
    pub external_ref: Uuid,
    pub name: String,
    pub code: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MetadataRecord {
    pub external_printing_ref: Uuid,
    pub oracle_ref: Uuid,
    pub name: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub set: SourceSet,
    pub collector_number: String,
    pub language: String,
    pub rarity: String,
    pub finishes: Vec<String>,
    pub layout: String,
    pub face_names: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
#[allow(
    clippy::large_enum_variant,
    reason = "One record is serialized immediately; boxing adds an allocation for every supported source row."
)]
pub(crate) enum StageRecord {
    Supported {
        #[serde(deserialize_with = "crate::provider_json::object")]
        metadata: MetadataRecord,
    },
    Unresolved {
        external_printing_ref: Uuid,
        reason: String,
    },
}

fn label(value: &Value, field: &str, maximum: usize) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.chars().count() <= maximum && !text.contains('\0'))
        .map(str::to_owned)
}
fn canonical_uuid(value: &Value, field: &str) -> Option<Uuid> {
    let text = value.get(field)?.as_str()?;
    crate::identifiers::parse(text).ok()
}
fn project(id: Uuid, value: &Value) -> Result<MetadataRecord, &'static str> {
    // Reversible records with face-level Oracle IDs cannot be represented by the
    // current single-definition printing. Retain and report, never pick a face.
    if value.get("oracle_id").is_none_or(Value::is_null) {
        return Err("missing_root_oracle");
    }
    let oracle_ref = canonical_uuid(value, "oracle_id").ok_or("invalid_metadata")?;
    let set_ref = canonical_uuid(value, "set_id").ok_or("invalid_metadata")?;
    let finishes = value
        .get("finishes")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or("invalid_finishes")?;
    let mut finish_names = Vec::new();
    for finish in finishes {
        let finish = finish.as_str().ok_or("invalid_finishes")?;
        if !matches!(finish, "nonfoil" | "foil" | "etched") {
            return Err("unsupported_finish");
        }
        if finish_names.iter().any(|prior| prior == finish) {
            return Err("duplicate_finish");
        }
        finish_names.push(finish.to_owned());
    }
    finish_names.sort();
    let name = label(value, "name", 1024).ok_or("invalid_metadata")?;
    let face_names = match value.get("card_faces") {
        None => vec![name.clone()],
        Some(Value::Array(faces)) if !faces.is_empty() && faces.len() <= 16 => faces
            .iter()
            .map(|face| label(face, "name", 1024).ok_or("invalid_metadata"))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("invalid_metadata"),
    };
    let set_code = label(value, "set", 16)
        .filter(|text| text.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .ok_or("invalid_metadata")?;
    let language = label(value, "lang", 16)
        .filter(|text| {
            text.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or("invalid_metadata")?;
    let rarity = label(value, "rarity", 64)
        .filter(|text| text.bytes().all(|byte| byte.is_ascii_graphic()))
        .ok_or("invalid_metadata")?;
    Ok(MetadataRecord {
        external_printing_ref: id,
        oracle_ref,
        name,
        set: SourceSet {
            external_ref: set_ref,
            name: label(value, "set_name", 1024).ok_or("invalid_metadata")?,
            code: set_code,
        },
        collector_number: label(value, "collector_number", 128).ok_or("invalid_metadata")?,
        language,
        rarity,
        finishes: finish_names,
        layout: label(value, "layout", 64).ok_or("invalid_metadata")?,
        face_names,
    })
}

struct StageWriter<W> {
    inner: W,
    bytes: u64,
}
impl<W: Write> Write for StageWriter<W> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.bytes.saturating_add(input.len() as u64) > MAX_STAGE_BYTES {
            return Err(io::Error::other("catalogue stage bound exceeded"));
        }
        let count = self.inner.write(input)?;
        self.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
pub(crate) fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, SourceError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(SourceError::BoundsExceeded);
    }
    Ok(bytes)
}

/// Verify retained evidence and write a bounded private projection. Only a final
/// complete.json marker binds the successfully verified source and staged bytes.
/// Partial directories remain for diagnosis and are never accepted as publications.
///
/// # Errors
/// Rejects inconsistent original metadata, source/hash/count/size mismatch, invalid
/// gzip/JSONL/identity evidence, staging limits and filesystem failures.
pub fn stage_catalogue(
    qualification: &Path,
    destination: &Path,
) -> Result<(PathBuf, CatalogueStage), SourceError> {
    let source = serde_json::from_slice::<crate::provider_json::Object<CatalogueSource>>(
        &read_bounded(qualification, 65_536)?,
    )
    .map_err(SourceError::invalid_index)?
    .0;
    source.metadata.validate(source.acquired_at)?;
    if source.acquired_at > Utc::now() || source.record_count == 0 {
        return Err(SourceError::InvalidIndex);
    }
    let index = read_bounded(&source.metadata_path, 1024 * 1024)?;
    let metadata = parse_index(&index)?;
    if hash_text(&Sha256::digest(&index)).as_str() != source.metadata_hash.as_str()
        || serde_json::to_value(&metadata).map_err(SourceError::invalid_index)?
            != serde_json::to_value(&source.metadata).map_err(SourceError::invalid_index)?
    {
        return Err(SourceError::HashMismatch);
    }
    let stage = fs::canonicalize(destination)?.join(format!("catalogue-{}", Uuid::new_v4()));
    fs::create_dir(&stage)?;
    let records_path = stage.join("records.jsonl");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&records_path)?;
    let mut writer = StageWriter {
        inner: BufWriter::new(file),
        bytes: 0,
    };
    let mut definitions = HashSet::new();
    let mut sets = HashSet::new();
    let mut supported = 0u64;
    let mut printings = 0u64;
    let mut reasons = BTreeMap::new();
    let scan = visit_artifact(&source.artifact_path, |id, value| {
        let record = match project(id, &value) {
            Ok(metadata) => {
                definitions.insert(metadata.oracle_ref);
                sets.insert(metadata.set.external_ref);
                supported += 1;
                printings += metadata.finishes.len() as u64;
                StageRecord::Supported { metadata }
            }
            Err(reason) => {
                *reasons.entry(reason.to_owned()).or_insert(0) += 1;
                StageRecord::Unresolved {
                    external_printing_ref: id,
                    reason: reason.to_owned(),
                }
            }
        };
        serde_json::to_writer(&mut writer, &record).map_err(SourceError::staging)?;
        writer.write_all(b"\n")?;
        Ok(())
    })?;
    writer.flush()?;
    writer.inner.get_ref().sync_all()?;
    if scan.hash.as_str() != source.hash.as_str()
        || scan.record_count != source.record_count
        || scan.compressed_bytes != source.compressed_bytes
        || scan.compressed_bytes != metadata.compressed_size
        || scan.decompressed_bytes != source.decompressed_bytes
    {
        return Err(SourceError::HashMismatch);
    }
    let (records_hash, records_bytes) = hash_file(&records_path)?;
    if records_bytes != writer.bytes {
        return Err(SourceError::HashMismatch);
    }
    let result = CatalogueStage {
        projection: PROJECTION.to_owned(),
        unresolved_records: scan.record_count - supported,
        source,
        records_path,
        records_hash,
        records_bytes,
        supported_records: supported,
        printing_count: printings,
        definition_count: definitions.len() as u64,
        set_count: sets.len() as u64,
        unresolved_reasons: reasons,
    };
    let marker = stage.join("complete.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)?;
    serde_json::to_writer(&mut file, &result).map_err(SourceError::staging)?;
    file.sync_all()?;
    Ok((marker, result))
}
