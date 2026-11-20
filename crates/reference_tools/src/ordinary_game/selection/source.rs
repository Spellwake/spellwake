//! Immutable source-nine intake, delegating exact byte/row authentication to the
//! shared closed-family owner. Authored semantics/signatures stay with Registry9.
use super::super::catalogue::DefinitionInput;
#[cfg(test)]
use crate::packet;
#[cfg(test)]
use flate2::read::MultiGzDecoder;
use spellwake_card_programs::{
    BindingStatus, OracleId, RESOLUTION_SELECTION_REGISTRY_VERSION, Registry, Scope,
    TEACHING_SOURCE_SHA256,
};
use spellwake_content::Inventory;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[cfg(test)]
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
};
const SOURCE_RECORDS: u64 = 39_196;
const DEFINITIONS: &[u8] = include_bytes!("source-definitions-v1.json");
use super::super::authenticated_source::{self, Family, SourceDefinitions};
fn descriptor(bytes: &[u8]) -> Result<SourceDefinitions, String> {
    authenticated_source::descriptor(bytes, Family::Selection)
}
fn authenticate_bytes(
    metadata: &[u8],
    snapshot: &[u8],
    definitions: &SourceDefinitions,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), String> {
    authenticated_source::authenticate_bytes(
        metadata,
        snapshot,
        definitions,
        vocabulary,
        Family::Selection,
    )
}
fn verify_raw_rows(snapshot: &[u8], definitions: &[DefinitionInput]) -> Result<(), String> {
    authenticated_source::verify_raw_rows(snapshot, definitions, Family::Selection)
}
#[cfg(test)]
fn verify_raw_line(row: &DefinitionInput, number: u64, bytes: &[u8]) -> Result<(), String> {
    authenticated_source::verify_raw_line(row, number, bytes, Family::Selection)
}
fn validate_bindings(
    registry: &Registry,
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
    definitions: &[DefinitionInput],
) -> Result<(), String> {
    registry
        .validate_resolution_selection_source(inventory, vocabulary)
        .map_err(|e| format!("selection native source binding: {e}"))?;
    let coverage = registry.coverage();
    if coverage.registry_version != RESOLUTION_SELECTION_REGISTRY_VERSION
        || coverage.source_sha256 != TEACHING_SOURCE_SHA256
        || coverage.source_records != SOURCE_RECORDS as usize
        || coverage.creature_vocabulary_id != vocabulary.id()
        || coverage.required_records != 9
        || coverage.required_partial_bindings != 9
        || coverage.required_without_binding != 0
        || coverage.fully_qualified_cards != 0
        || coverage.behavior_evidence != "not-evaluated-by-registry"
        || coverage.cards.len() != 9
        || definitions.len() != 9
    {
        return Err("selection native source9 coverage mismatch".into());
    }
    let mut seen = BTreeSet::new();
    for row in definitions {
        if !seen.insert(&row.oracle_id) {
            return Err("selection duplicate native source identity".into());
        }
        let id = OracleId::try_from(row.oracle_id.clone())
            .map_err(|e| format!("selection source Oracle ID: {e}"))?;
        let bound = registry
            .resolve(&id)
            .ok_or("selection native binding missing")?;
        let card = coverage
            .cards
            .iter()
            .find(|card| card.oracle_id.as_deref() == Some(row.oracle_id.as_str()))
            .ok_or("selection native source coverage identity missing")?;
        let original = inventory
            .cards
            .iter()
            .find(|card| card.oracle_id.as_deref() == Some(row.oracle_id.as_str()))
            .ok_or("selection inventory source identity missing")?;
        let BindingStatus::Partial {
            program,
            scope,
            semantic_sha256,
        } = &card.binding
        else {
            return Err("selection native binding is not Partial".into());
        };
        if !card.required
            || card.name != row.name
            || original.name != row.name
            || original.scryfall_id != row.scryfall_id
            || *scope != Scope::ResolutionSelectionOrdinaryCardV1
            || bound.scope() != *scope
            || bound.key() != *program
            || semantic_sha256 != &row.registry_signature_v1_sha256
            || card.behavior_evidence != "not-evaluated-by-registry"
        {
            return Err("selection exact native source identity/signature mismatch".into());
        }
    }
    Ok(())
}

/// Authenticated saved source9, immutable native Registry9 and native vocabulary.
/// Contains input provenance only; no game, caller-delivery or parity evidence.
pub struct SelectionSource {
    inventory: Inventory,
    registry: Registry,
    definitions: Vec<DefinitionInput>,
    vocabulary: Arc<CreatureTypeVocabulary>,
}

impl SelectionSource {
    /// Authenticate the exact full metadata/gzip and each selected raw source row.
    /// Vocabulary content identity is native; its file-byte provenance stays upstream.
    ///
    /// # Errors
    /// Rejects unavailable files, foreign bytes or vocabulary, malformed compiled
    /// descriptor, any raw line/printing tuple mismatch or failed Registry9 guard.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Self, String> {
        let definitions = descriptor(DEFINITIONS)?;
        let metadata = std::fs::read(metadata).map_err(|e| format!("selection metadata: {e}"))?;
        let snapshot = std::fs::read(snapshot).map_err(|e| format!("selection snapshot: {e}"))?;
        authenticate_bytes(&metadata, &snapshot, &definitions, &vocabulary)?;
        // Decode the same authenticated compressed bytes through the maintained
        // inventory owner; retain one clone only for exact raw-line verification.
        let inventory = spellwake_content::snapshot::decode(&metadata, snapshot.clone())
            .map_err(|e| format!("selection snapshot decode: {e:?}"))?;
        let registry = Registry::from_resolution_selection_inventory(&inventory, &vocabulary)
            .map_err(|e| format!("selection registry: {e}"))?;
        verify_raw_rows(&snapshot, &definitions.definitions)?;
        let source = Self {
            inventory,
            registry,
            definitions: definitions.definitions,
            vocabulary,
        };
        source.validate_witness(&source.vocabulary.id().to_string())?;
        Ok(source)
    }

    /// Borrow the guarded, immutable native Registry9. Its bindings remain Partial.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Recheck native bindings against the immutable source authenticated at load.
    /// This does not reread files or claim that a supplied ID authenticates bytes.
    ///
    /// # Errors
    /// Rejects a foreign requested vocabulary ID or any source9/native binding gap.
    pub(crate) fn validate_witness(&self, vocabulary_id: &str) -> Result<(), String> {
        if vocabulary_id != self.vocabulary().id().to_string() {
            return Err("selection witness vocabulary mismatch".into());
        }
        validate_bindings(
            self.registry(),
            self.inventory(),
            self.vocabulary(),
            self.definitions(),
        )
    }

    pub(in crate::ordinary_game) fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub(in crate::ordinary_game) fn definitions(&self) -> &[DefinitionInput] {
        &self.definitions
    }
    pub(in crate::ordinary_game) fn vocabulary(&self) -> &Arc<CreatureTypeVocabulary> {
        &self.vocabulary
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
use spellwake_card_programs::TEACHING_VOCABULARY_ID;
