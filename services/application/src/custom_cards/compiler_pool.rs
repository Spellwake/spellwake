//! Dedicated 16 MiB-stack worker pool and shared creature-vocabulary loader for
//! custom card compilation and phrasing standardization.
//!
//! Runs independently of `SPELLWAKE_NATIVE_VIEWS_POOL` so the Custom Card Studio's
//! compile and standardize endpoints remain available even on lightweight API nodes.

use serde::Deserialize;
use spellwake_card_programs::custom_card::{
    CustomCardCompileInput, CustomCardCompiled, compile_custom_card,
};
use spellwake_contracts::creature_types::{CreatureTypeVocabulary, CreatureVocabularyId};
use spellwake_oracle_compiler::standardize::{
    StandardizeFaceInput, StandardizeOutput, standardize_faces,
};
use std::{
    error::Error,
    fmt,
    fs::File,
    io::Read,
    sync::{Arc, Mutex, mpsc},
    thread,
};

/// Explicit 16 MiB OS stack per compiler worker so deep oracle AST recursion
/// never overflows Tokio's 2 MiB default blocking thread stack.
const COMPILER_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// Bounded request queue depth so abusive traffic fails closed with 503.
const MAX_QUEUED_JOBS: usize = 64;

/// Embedded CR-20260925 creature vocabulary used only as a studio fallback when
/// `SPELLWAKE_CREATURE_VOCABULARY_INPUT` is not configured in local dev or tests.
const EMBEDDED_STUDIO_CREATURE_VOCABULARY: &[u8] =
    include_bytes!("../../../../crates/content/tests/fixtures/creature-vocabulary.input.json");

/// Safe typed failures for the custom card compiler worker pool and vocabulary loader.
#[derive(Debug)]
pub enum CompilerPoolError {
    Configuration(&'static str),
    Overloaded,
    WorkerStopped,
}

impl fmt::Display for CompilerPoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(msg) => {
                write!(f, "custom card compiler configuration error: {msg}")
            }
            Self::Overloaded => f.write_str("custom card compiler queue is full"),
            Self::WorkerStopped => f.write_str("custom card compiler worker exited unexpectedly"),
        }
    }
}

impl Error for CompilerPoolError {}

enum CompilerJob {
    Compile {
        input: CustomCardCompileInput,
        reply: mpsc::SyncSender<CustomCardCompiled>,
    },
    Standardize {
        faces: Vec<StandardizeFaceInput>,
        reply: mpsc::SyncSender<StandardizeOutput>,
    },
}

/// Fixed-size pool of 16 MiB-stack worker threads compiling and standardizing custom cards.
pub struct CustomCardCompilerPool {
    vocabulary: Arc<CreatureTypeVocabulary>,
    tx: mpsc::SyncSender<CompilerJob>,
}

impl CustomCardCompilerPool {
    /// Construct a compiler pool over an already-validated shared `CreatureTypeVocabulary`.
    ///
    /// # Errors
    /// Returns `CompilerPoolError::Configuration` if worker threads cannot be spawned.
    pub fn with_vocabulary(
        vocabulary: Arc<CreatureTypeVocabulary>,
        worker_count: usize,
    ) -> Result<Self, CompilerPoolError> {
        let count = worker_count.clamp(1, 4);
        let (tx, rx) = mpsc::sync_channel::<CompilerJob>(MAX_QUEUED_JOBS);
        let shared_rx = Arc::new(Mutex::new(rx));

        for idx in 0..count {
            let rx_clone = Arc::clone(&shared_rx);
            let vocab_clone = Arc::clone(&vocabulary);
            thread::Builder::new()
                .name(format!("spellwake-custom-compiler-{idx}"))
                .stack_size(COMPILER_WORKER_STACK_BYTES)
                .spawn(move || worker_loop(rx_clone, vocab_clone))
                .map_err(|_| {
                    CompilerPoolError::Configuration("failed to spawn custom compiler worker")
                })?;
        }

        Ok(Self { vocabulary, tx })
    }

    /// Construct a compiler pool using the shared vocabulary loader from environment variables.
    ///
    /// # Errors
    /// Fails closed if `SPELLWAKE_CREATURE_VOCABULARY_INPUT` is explicitly set to a missing or
    /// malformed file, or if worker threads cannot be spawned.
    pub fn from_env() -> Result<Self, CompilerPoolError> {
        let vocabulary = load_custom_card_vocabulary_from_env()?;
        let workers = std::env::var("SPELLWAKE_CUSTOM_COMPILER_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(2);
        Self::with_vocabulary(vocabulary, workers)
    }

    /// Borrow the active creature type vocabulary used by this compiler pool.
    #[must_use]
    pub fn vocabulary(&self) -> &Arc<CreatureTypeVocabulary> {
        &self.vocabulary
    }

    /// Compile one custom card on a 16 MiB-stack worker thread.
    ///
    /// # Errors
    /// Returns `Overloaded` if the bounded work queue is full, or `WorkerStopped` if a worker died.
    pub fn compile(
        &self,
        input: CustomCardCompileInput,
    ) -> Result<CustomCardCompiled, CompilerPoolError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        match self.tx.try_send(CompilerJob::Compile {
            input,
            reply: reply_tx,
        }) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => return Err(CompilerPoolError::Overloaded),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(CompilerPoolError::WorkerStopped);
            }
        }
        reply_rx
            .recv()
            .map_err(|_| CompilerPoolError::WorkerStopped)
    }

    /// Standardize custom card oracle phrasing on a 16 MiB-stack worker thread.
    ///
    /// # Errors
    /// Returns `Overloaded` if the bounded work queue is full, or `WorkerStopped` if a worker died.
    pub fn standardize(
        &self,
        faces: Vec<StandardizeFaceInput>,
    ) -> Result<StandardizeOutput, CompilerPoolError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        match self.tx.try_send(CompilerJob::Standardize {
            faces,
            reply: reply_tx,
        }) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => return Err(CompilerPoolError::Overloaded),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(CompilerPoolError::WorkerStopped);
            }
        }
        reply_rx
            .recv()
            .map_err(|_| CompilerPoolError::WorkerStopped)
    }
}

fn worker_loop(
    rx: Arc<Mutex<mpsc::Receiver<CompilerJob>>>,
    vocabulary: Arc<CreatureTypeVocabulary>,
) {
    loop {
        let job = {
            let Ok(guard) = rx.lock() else {
                break;
            };
            match guard.recv() {
                Ok(job) => job,
                Err(_) => break,
            }
        };
        match job {
            CompilerJob::Compile { input, reply } => {
                let compiled = compile_custom_card(&input, &vocabulary);
                let _ = reply.send(compiled);
            }
            CompilerJob::Standardize { faces, reply } => {
                let output = standardize_faces(&faces);
                let _ = reply.send(output);
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductionVocabularyInput {
    rules_revision: String,
    rules_sha256: String,
    members: Vec<String>,
    id: CreatureVocabularyId,
}

#[derive(Deserialize)]
struct FixtureVocabularySource {
    sha256: String,
    rules_revision: String,
}

#[derive(Deserialize)]
struct FixtureVocabularyEntry {
    canonical_label: String,
}

#[derive(Deserialize)]
struct FixtureVocabularyInput {
    source: FixtureVocabularySource,
    entries: Vec<FixtureVocabularyEntry>,
}

/// Load the shared `CreatureTypeVocabulary` for custom card compilation.
///
/// Prefers `SPELLWAKE_CREATURE_VOCABULARY_INPUT` (failing closed if explicitly configured
/// with an invalid path or file), then `SPELLWAKE_CUSTOM_CARD_VOCABULARY_INPUT`, and falls back
/// to the embedded CR-20260925 creature vocabulary fixture when neither env var is set.
///
/// # Errors
/// Returns `CompilerPoolError::Configuration` if an explicitly configured file cannot be read or
/// fails vocabulary validation.
pub fn load_custom_card_vocabulary_from_env()
-> Result<Arc<CreatureTypeVocabulary>, CompilerPoolError> {
    for var_name in [
        "SPELLWAKE_CREATURE_VOCABULARY_INPUT",
        "SPELLWAKE_CUSTOM_CARD_VOCABULARY_INPUT",
    ] {
        if let Some(raw_path) = std::env::var_os(var_name) {
            let path = raw_path
                .into_string()
                .map_err(|_| CompilerPoolError::Configuration("non-UTF8 vocabulary path"))?;
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut bytes = Vec::new();
            File::open(trimmed)
                .map_err(|_| CompilerPoolError::Configuration("cannot open creature vocabulary"))?
                .take(1_048_577)
                .read_to_end(&mut bytes)
                .map_err(|_| CompilerPoolError::Configuration("cannot read creature vocabulary"))?;
            if bytes.is_empty() || bytes.len() > 1_048_576 {
                return Err(CompilerPoolError::Configuration(
                    "creature vocabulary exceeds 1 MiB bound",
                ));
            }
            return load_vocabulary_from_bytes(&bytes).map(Arc::new);
        }
    }
    load_vocabulary_from_bytes(EMBEDDED_STUDIO_CREATURE_VOCABULARY).map(Arc::new)
}

/// Decode either the production `{ rules_revision, rules_sha256, members, id }` vocabulary format
/// or the repository CR-20260925 fixture `{ source, entries }` format.
///
/// # Errors
/// Rejects oversized, malformed, or digest-mismatched inputs.
pub fn load_vocabulary_from_bytes(
    bytes: &[u8],
) -> Result<CreatureTypeVocabulary, CompilerPoolError> {
    if bytes.is_empty() || bytes.len() > 1_048_576 {
        return Err(CompilerPoolError::Configuration(
            "creature vocabulary size out of bounds",
        ));
    }
    if let Ok(prod) = crate::decode_wire_object::<ProductionVocabularyInput>(bytes) {
        let rules_hash = crate::source_protocol::digest_bytes(&prod.rules_sha256).ok_or(
            CompilerPoolError::Configuration("invalid vocabulary rules_sha256"),
        )?;
        let vocab = CreatureTypeVocabulary::new(prod.rules_revision, rules_hash, prod.members)
            .map_err(|_| CompilerPoolError::Configuration("invalid creature vocabulary members"))?;
        if vocab.id() != prod.id {
            return Err(CompilerPoolError::Configuration(
                "creature vocabulary id mismatch",
            ));
        }
        return Ok(vocab);
    }

    let fixture: FixtureVocabularyInput = serde_json::from_slice(bytes)
        .map_err(|_| CompilerPoolError::Configuration("malformed creature vocabulary JSON"))?;
    let rules_hash = crate::source_protocol::digest_bytes(&fixture.source.sha256).ok_or(
        CompilerPoolError::Configuration("invalid fixture vocabulary sha256"),
    )?;
    let members = fixture
        .entries
        .into_iter()
        .map(|entry| entry.canonical_label)
        .collect();
    CreatureTypeVocabulary::new(fixture.source.rules_revision, rules_hash, members)
        .map_err(|_| CompilerPoolError::Configuration("invalid fixture creature vocabulary"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_card_programs::custom_card::CustomCardFaceRules;

    #[test]
    fn compiler_pool_compiles_and_standardizes_on_16mib_worker() {
        let vocab = load_vocabulary_from_bytes(EMBEDDED_STUDIO_CREATURE_VOCABULARY)
            .expect("embedded CR-20260925 creature vocabulary decodes cleanly");
        assert!(vocab.resolve_label("Human").is_some());
        assert!(vocab.resolve_label("Artificer").is_some());

        let pool = CustomCardCompilerPool::with_vocabulary(Arc::new(vocab), 1)
            .expect("compiler pool starts");

        let std_res = pool
            .standardize(vec![StandardizeFaceInput {
                name: "Aetherblade Vanguard".to_owned(),
                mana_cost: Some("{2}{u}{r}".to_owned()),
                type_line: "Creature - Human Artificer".to_owned(),
                oracle_text: "When Aetherblade Vanguard enters the battlefield, draw a card."
                    .to_owned(),
                power: Some("3".to_owned()),
                toughness: Some("2".to_owned()),
                loyalty: None,
                defense: None,
                color_indicator: None,
            }])
            .expect("standardize succeeds");
        assert!(std_res.changed);
        assert!(std_res.faces[0].type_line.contains('\u{2014}'));
        assert!(std_res.faces[0].oracle_text.contains("enters"));

        let compiled = pool
            .compile(CustomCardCompileInput {
                layout: "normal".to_owned(),
                rarity: "rare".to_owned(),
                faces: vec![CustomCardFaceRules {
                    name: "Aetherblade Vanguard".to_owned(),
                    mana_cost: Some("{2}{U}{R}".to_owned()),
                    type_line: "Creature \u{2014} Human Artificer".to_owned(),
                    oracle_text: "Flying, haste".to_owned(),
                    power: Some("3".to_owned()),
                    toughness: Some("2".to_owned()),
                    loyalty: None,
                    defense: None,
                    color_indicator: None,
                }],
            })
            .expect("compile succeeds");
        assert!(compiled.report.stage1_parsed);
        assert!(compiled.report.stage2_playable);
        assert!(compiled.program.is_some());
    }
}
