//! Trusted catalogue producer. Complete evidence precedes source intent; batches
//! stay hidden until the fully consumed stage and canonical release both succeed.
use crate::{
    backend::{BackendClient, BackendConfig},
    catalogue_source::{
        CatalogueStage, MAX_STAGE_BYTES, MetadataRecord, PROJECTION, StageRecord, stage_catalogue,
    },
    price_source::hash_text,
    private_storage::{RetainedFile, private_bucket, retain_file},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fmt,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;

const MAX_BODY: usize = 384 * 1024;
const MAX_LINE: u64 = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueFailure {
    Configuration,
    Source,
    Stage,
    Storage,
    Backend,
    Acknowledgment,
}
/// Safe boundary message with the underlying diagnostic available through source().
#[derive(Debug)]
pub struct CataloguePublishError {
    code: CatalogueFailure,
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl CataloguePublishError {
    fn new(code: CatalogueFailure) -> Self {
        Self { code, source: None }
    }
    fn caused(code: CatalogueFailure, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            code,
            source: Some(Box::new(source)),
        }
    }
    pub fn code(&self) -> CatalogueFailure {
        self.code
    }
}
impl fmt::Display for CataloguePublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Catalogue publication is unresolved.")
    }
}
impl Error for CataloguePublishError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error.as_ref() as &(dyn Error + 'static))
    }
}

/// Runtime-only credentials. Never serialized or included in activity arguments.
pub struct CataloguePublisher {
    backend: BackendClient,
    bucket: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CataloguePublication {
    pub publication_id: Uuid,
    pub source_hash: String,
    pub stage_hash: String,
    pub supported_records: u64,
    pub unresolved_records: u64,
    pub printing_count: u64,
    pub batch_count: usize,
    pub release_elapsed_ms: u128,
}

struct PreparedCatalogue {
    stage: CatalogueStage,
    terms: Value,
    hashes: Vec<String>,
}
type Common = (String, String, Vec<String>);

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.chars().count() <= maximum && !value.contains('\0')
}
fn valid_record(metadata: &MetadataRecord) -> bool {
    !metadata.external_printing_ref.is_nil()
        && !metadata.oracle_ref.is_nil()
        && !metadata.set.external_ref.is_nil()
        && valid_text(&metadata.name, 1024)
        && valid_text(&metadata.set.name, 1024)
        && valid_text(&metadata.collector_number, 128)
        && valid_text(&metadata.layout, 64)
        && (1..=16).contains(&metadata.face_names.len())
        && metadata
            .face_names
            .iter()
            .all(|name| valid_text(name, 1024))
        && (1..=16).contains(&metadata.set.code.len())
        && metadata
            .set
            .code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        && (1..=16).contains(&metadata.language.len())
        && metadata
            .language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        && (1..=64).contains(&metadata.rarity.len())
        && metadata.rarity.bytes().all(|byte| byte.is_ascii_graphic())
        && (1..=3).contains(&metadata.finishes.len())
        && metadata
            .finishes
            .iter()
            .all(|finish| matches!(finish.as_str(), "foil" | "nonfoil" | "etched"))
        && metadata.finishes.iter().collect::<HashSet<_>>().len() == metadata.finishes.len()
}

/// Read exactly the staged bytes while emitting bounded deterministic bodies. On
/// replay, emitted batches are checked against the previously verified hash plan
/// before dispatch. EOF/hash/coverage failure never reaches final release.
fn visit_batches(
    stage: &CatalogueStage,
    mut emit: impl FnMut(u32, &str) -> Result<(), CataloguePublishError>,
) -> Result<Vec<String>, CataloguePublishError> {
    if stage.projection != PROJECTION
        || stage.records_bytes == 0
        || stage.records_bytes > MAX_STAGE_BYTES
        || stage.supported_records == 0
    {
        return Err(CataloguePublishError::new(CatalogueFailure::Stage));
    }
    let mut reader = BufReader::new(
        File::open(&stage.records_path)
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Stage, error))?,
    );
    let mut line = Vec::new();
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut identities = HashSet::new();
    let mut definitions: HashMap<Uuid, Common> = HashMap::new();
    let mut sets: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut codes = HashMap::new();
    let mut supported = 0u64;
    let mut printings = 0u64;
    let mut unresolved = BTreeMap::new();
    let mut hashes = Vec::new();
    let mut body = String::from("[");
    let mut body_records = 0usize;
    let mut flush =
        |body: &mut String, body_records: &mut usize| -> Result<(), CataloguePublishError> {
            if *body_records == 0 {
                return Ok(());
            }
            body.push(']');
            let sequence = u32::try_from(hashes.len() + 1)
                .map_err(|_| CataloguePublishError::new(CatalogueFailure::Stage))?;
            if sequence > 20000 {
                return Err(CataloguePublishError::new(CatalogueFailure::Stage));
            }
            emit(sequence, body)?;
            hashes.push(
                hash_text(&Sha256::digest(body.as_bytes()))
                    .as_str()
                    .to_owned(),
            );
            body.clear();
            body.push('[');
            *body_records = 0;
            Ok(())
        };
    loop {
        line.clear();
        let read = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Stage, error))?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .filter(|bytes| *bytes <= MAX_STAGE_BYTES)
            .ok_or_else(|| CataloguePublishError::new(CatalogueFailure::Stage))?;
        if read as u64 > MAX_LINE || identities.len() >= 5000000 {
            return Err(CataloguePublishError::new(CatalogueFailure::Stage));
        }
        digest.update(&line);
        let record: StageRecord = serde_json::from_slice(&line)
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Stage, error))?;
        match record {
            StageRecord::Supported { metadata } => {
                if !valid_record(&metadata) || !identities.insert(metadata.external_printing_ref) {
                    return Err(CataloguePublishError::new(CatalogueFailure::Stage));
                }
                let common = (
                    metadata.name.clone(),
                    metadata.layout.clone(),
                    metadata.face_names.clone(),
                );
                if definitions
                    .get(&metadata.oracle_ref)
                    .is_some_and(|prior| prior != &common)
                {
                    return Err(CataloguePublishError::new(CatalogueFailure::Stage));
                }
                definitions.entry(metadata.oracle_ref).or_insert(common);
                let set = (metadata.set.name.clone(), metadata.set.code.clone());
                if sets
                    .get(&metadata.set.external_ref)
                    .is_some_and(|prior| prior != &set)
                    || codes
                        .get(&metadata.set.code.to_ascii_lowercase())
                        .is_some_and(|id| id != &metadata.set.external_ref)
                {
                    return Err(CataloguePublishError::new(CatalogueFailure::Stage));
                }
                sets.entry(metadata.set.external_ref).or_insert(set);
                codes.insert(
                    metadata.set.code.to_ascii_lowercase(),
                    metadata.set.external_ref,
                );
                supported += 1;
                printings += metadata.finishes.len() as u64;
                let encoded = serde_json::to_string(&metadata).map_err(|error| {
                    CataloguePublishError::caused(CatalogueFailure::Stage, error)
                })?;
                if encoded.len() + 2 > MAX_BODY {
                    return Err(CataloguePublishError::new(CatalogueFailure::Stage));
                }
                if body_records == 500 || body.len() + encoded.len() + 2 > MAX_BODY {
                    flush(&mut body, &mut body_records)?;
                }
                if body_records > 0 {
                    body.push(',');
                }
                body.push_str(&encoded);
                body_records += 1;
            }
            StageRecord::Unresolved {
                external_printing_ref,
                reason,
            } => {
                if external_printing_ref.is_nil()
                    || !identities.insert(external_printing_ref)
                    || !matches!(
                        reason.as_str(),
                        "missing_root_oracle"
                            | "invalid_metadata"
                            | "invalid_finishes"
                            | "unsupported_finish"
                            | "duplicate_finish"
                    )
                {
                    return Err(CataloguePublishError::new(CatalogueFailure::Stage));
                }
                *unresolved.entry(reason).or_insert(0u64) += 1;
            }
        }
    }
    flush(&mut body, &mut body_records)?;
    if bytes != stage.records_bytes
        || hash_text(&digest.finalize()).as_str() != stage.records_hash.as_str()
        || identities.len() as u64 != stage.source.record_count
        || supported != stage.supported_records
        || printings != stage.printing_count
        || definitions.len() as u64 != stage.definition_count
        || sets.len() as u64 != stage.set_count
        || unresolved != stage.unresolved_reasons
        || unresolved.values().sum::<u64>() != stage.unresolved_records
    {
        return Err(CataloguePublishError::new(CatalogueFailure::Stage));
    }
    Ok(hashes)
}

fn prepare(
    qualification: &Path,
    destination: &Path,
) -> Result<PreparedCatalogue, CataloguePublishError> {
    eprintln!("catalogue_publisher phase=verify_source");
    let (_, stage) = stage_catalogue(qualification, destination)
        .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Source, error))?;
    eprintln!("catalogue_publisher phase=plan_batches");
    let hashes = visit_batches(&stage, |_, _| Ok(()))?;
    let terms = json!({"projection":stage.projection,"artifactHash":stage.source.hash,"metadataHash":stage.source.metadata_hash,"stageHash":stage.records_hash,
        "artifactRef":format!("prices/scryfall/{}.jsonl.gz",stage.source.hash.hex()),"metadataRef":format!("prices/scryfall/metadata/{}.json",stage.source.metadata_hash.hex()),"stageRef":format!("catalogue/scryfall/{}.jsonl",stage.records_hash.hex()),
        "sourceUpdatedAt":stage.source.metadata.updated_at,"acquiredAt":stage.source.acquired_at,"compressedBytes":stage.source.compressed_bytes,"decompressedBytes":stage.source.decompressed_bytes,"recordCount":stage.source.record_count,
        "stageBytes":stage.records_bytes,"supportedRecords":stage.supported_records,"unresolvedRecords":stage.unresolved_records,"printingCount":stage.printing_count,"definitionCount":stage.definition_count,"setCount":stage.set_count,
        "batchCount":hashes.len(),"batchHashes":hashes,"unresolvedReasons":stage.unresolved_reasons});
    Ok(PreparedCatalogue {
        stage,
        terms,
        hashes,
    })
}

impl CataloguePublisher {
    /// Construct only inside a trusted worker. Public app bundles receive no secret.
    /// # Errors
    /// Rejects unsafe origins, secret-key shape, bucket names or HTTP configuration.
    pub fn new(base: &str, key: &str, bucket: &str) -> Result<Self, CataloguePublishError> {
        if bucket.is_empty()
            || bucket.len() > 80
            || !bucket
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CataloguePublishError::new(CatalogueFailure::Configuration));
        }
        let config = BackendConfig::new(base, key).map_err(|error| {
            CataloguePublishError::caused(CatalogueFailure::Configuration, error)
        })?;
        let backend = BackendClient::new(
            config,
            "Spellwake-Private-Catalogue-Publisher/0.1",
            Duration::from_secs(20 * 60),
        )
        .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Configuration, error))?;
        Ok(Self {
            backend,
            bucket: bucket.to_owned(),
        })
    }
    /// Read runtime-only configuration; callers must select their intended environment.
    /// # Errors
    /// Returns Configuration if any required setting is absent or invalid.
    pub fn from_env() -> Result<Self, CataloguePublishError> {
        let setting = |name| {
            std::env::var(name)
                .map_err(|_| CataloguePublishError::new(CatalogueFailure::Configuration))
        };
        Self::new(
            &setting("SPELLWAKE_SUPABASE_URL")?,
            &setting("SPELLWAKE_SUPABASE_SECRET_KEY")?,
            &setting("SPELLWAKE_CATALOGUE_BUCKET")?,
        )
    }
    /// Verify source, retain exact evidence and publish through canonical functions.
    /// Retrying unchanged inputs uses the same source and exact batch identities.
    /// # Errors
    /// Source/stage/storage failures prevent release. An RPC failure may be an
    /// unknown acknowledgment; it does not prove that a commit did not happen.
    pub fn publish(
        &self,
        qualification: &Path,
        destination: &Path,
    ) -> Result<CataloguePublication, CataloguePublishError> {
        let prepared = prepare(qualification, destination)?;
        self.publish_prepared(&prepared)
    }
    fn publish_prepared(
        &self,
        prepared: &PreparedCatalogue,
    ) -> Result<CataloguePublication, CataloguePublishError> {
        let stage = &prepared.stage;
        eprintln!("catalogue_publisher phase=retain_evidence");
        let directory = stage
            .records_path
            .parent()
            .ok_or_else(|| CataloguePublishError::new(CatalogueFailure::Stage))?;
        private_bucket(&self.backend, &self.bucket)
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Storage, error))?;
        for (key, path, hash, size, content_type) in [
            (
                format!(
                    "prices/scryfall/metadata/{}.json",
                    stage.source.metadata_hash.hex()
                ),
                stage.source.metadata_path.as_path(),
                &stage.source.metadata_hash,
                std::fs::metadata(&stage.source.metadata_path)
                    .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Stage, error))?
                    .len(),
                "application/json",
            ),
            (
                format!("prices/scryfall/{}.jsonl.gz", stage.source.hash.hex()),
                stage.source.artifact_path.as_path(),
                &stage.source.hash,
                stage.source.compressed_bytes,
                "application/gzip",
            ),
            (
                format!("catalogue/scryfall/{}.jsonl", stage.records_hash.hex()),
                stage.records_path.as_path(),
                &stage.records_hash,
                stage.records_bytes,
                "application/x-ndjson",
            ),
        ] {
            retain_file(
                &self.backend,
                &self.bucket,
                directory,
                &RetainedFile {
                    key: &key,
                    path,
                    hash,
                    size,
                    content_type,
                },
            )
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Storage, error))?;
        }
        let publication: Uuid = self
            .backend
            .rpc(
                "begin_catalogue_publication",
                &json!({"p_terms":prepared.terms}),
            )
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Backend, error))?;
        if publication.is_nil() {
            return Err(CataloguePublishError::new(CatalogueFailure::Acknowledgment));
        }
        eprintln!(
            "catalogue_publisher phase=append_batches total={}",
            prepared.hashes.len()
        );
        let consumed = visit_batches(stage, |sequence, body| {
            if prepared
                .hashes
                .get(sequence as usize - 1)
                .is_none_or(|expected| {
                    expected != hash_text(&Sha256::digest(body.as_bytes())).as_str()
                })
            {
                return Err(CataloguePublishError::new(CatalogueFailure::Stage));
            }
            let batch: Uuid = self
                .backend
                .rpc(
                    "append_catalogue_batch",
                    &json!({"p_publication":publication,"p_sequence":sequence,"p_body":body}),
                )
                .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Backend, error))?;
            if batch.is_nil() {
                return Err(CataloguePublishError::new(CatalogueFailure::Acknowledgment));
            }
            if sequence % 100 == 0 {
                eprintln!("catalogue_publisher phase=append_progress completed={sequence}");
            }
            Ok(())
        })?;
        if consumed != prepared.hashes {
            return Err(CataloguePublishError::new(CatalogueFailure::Stage));
        }
        let started = Instant::now();
        eprintln!("catalogue_publisher phase=release");
        let finished: Uuid = self
            .backend
            .rpc(
                "finish_catalogue_publication",
                &json!({"p_publication":publication}),
            )
            .map_err(|error| CataloguePublishError::caused(CatalogueFailure::Backend, error))?;
        if finished != publication {
            return Err(CataloguePublishError::new(CatalogueFailure::Acknowledgment));
        }
        Ok(CataloguePublication {
            publication_id: publication,
            source_hash: stage.source.hash.as_str().to_owned(),
            stage_hash: stage.records_hash.as_str().to_owned(),
            supported_records: stage.supported_records,
            unresolved_records: stage.unresolved_records,
            printing_count: stage.printing_count,
            batch_count: consumed.len(),
            release_elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

#[cfg(test)]
#[path = "catalogue_publisher/tests.rs"]
mod tests;
