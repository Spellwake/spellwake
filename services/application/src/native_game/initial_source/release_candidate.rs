//! Inspect the saved ordinary constructed pool before an application release is selected.
use super::*;
use crate::{amount::Amount, catalogue_source::read_bounded};
use num_traits::ToPrimitive;
use spellwake_content::Legality;
use std::{collections::BTreeMap, error::Error, path::Path};
mod limited;
pub use limited::inspect_limited_deck;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deck {
    format: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    oracle_ref: OracleId,
    quantity: Amount,
    section: String,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DeckFailure {
    UnsupportedFormat,
    InvalidQuantity,
    UnsupportedCard,
    IllegalCard,
    UnsupportedSection,
    CopyLimit,
    PreparationCapacity,
    MainTooSmall,
    SideboardTooLarge,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeckCounts {
    main: u32,
    sideboard: u32,
}
fn basic_land(card: &spellwake_content::InventoryCard) -> bool {
    matches!(card.faces.as_slice(), [face] if face.type_line.as_deref()
                .and_then(|line| line.split('—').next())
                .is_some_and(|types| types.split_whitespace().any(|t| t == "Basic")
                    && types.split_whitespace().any(|t| t == "Land")))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CardSourceRefs {
    definition_revision_ref: EvidenceHash,
    rules_text_ref: EvidenceHash,
    program_ref: EvidenceHash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicationWitness {
    schema: String,
    artifact_hash: EvidenceHash,
    oracle_snapshot_hash: EvidenceHash,
    cards: Vec<PublishedCard>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishedCard {
    oracle_ref: OracleId,
    printing: Uuid,
    definition_hash: EvidenceHash,
    rules_hash: EvidenceHash,
}
impl NativeProgramSource {
    fn card_source_refs(&self, oracle: &OracleId) -> Result<CardSourceRefs, NativeInitialError> {
        let card = self
            .source_card(oracle)
            .ok_or(NativeInitialError::ForeignSource)?;
        let program = self
            .registry
            .resolve(oracle)
            .ok_or(NativeInitialError::ForeignSource)?;
        let text: Vec<_> = card
            .faces
            .iter()
            .map(|face| (&face.name, &face.oracle_text))
            .collect();
        let definition =
            serde_json::to_vec(&("native-definition-v1", oracle, &card.name, &card.faces))
                .map_err(NativeInitialError::Json)?;
        let text = serde_json::to_vec(&("native-rules-text-v1", oracle, text))
            .map_err(NativeInitialError::Json)?;
        let executable = serde_json::to_vec(&(
            "native-program-v1",
            self.identity(),
            oracle,
            program.key(),
            program.scope(),
        ))
        .map_err(NativeInitialError::Json)?;
        Ok(CardSourceRefs {
            definition_revision_ref: digest(&definition),
            rules_text_ref: digest(&text),
            program_ref: digest(&executable),
        })
    }

    // A catalogue revision retains publication locators. The native card hashes
    // come from the same canonical English printing in both verified bulk files.
    // SQL additionally ties the revision to the original accepted publisher receipt.
    fn published_program_ref(
        &self,
        release: &preparation_source::PreparationRelease,
        entry: &preparation_source::PreparationEntry,
        oracle: &OracleId,
        refs: &CardSourceRefs,
    ) -> Result<EvidenceHash, NativeInitialError> {
        use std::sync::LazyLock;
        static WITNESS: LazyLock<PublicationWitness> = LazyLock::new(|| {
            serde_json::from_slice(include_bytes!("published-card-witness-v1.json"))
                .expect("compiled publication witness must match its schema")
        });
        let witness = &*WITNESS;
        let publication = entry
            .definition_revision_ref
            .as_str()
            .strip_prefix("catalogue:")
            .and_then(|id| Uuid::parse_str(id).ok())
            .filter(|id| !id.is_nil())
            .ok_or(NativeInitialError::ForeignSource)?;
        let card = self
            .source_card(oracle)
            .ok_or(NativeInitialError::ForeignSource)?;
        if witness.schema != "native-published-card-witness-v1"
            || witness.oracle_snapshot_hash != self.snapshot
            || entry.definition_revision_ref.as_str() != format!("catalogue:{publication}")
            || entry.rules_text_ref.as_str()
                != format!(
                    "prices/scryfall/{}.jsonl.gz#oracle={}",
                    witness.artifact_hash.hex(),
                    entry.oracle_ref.as_str()
                )
            || !witness.cards.iter().any(|w| {
                &w.oracle_ref == oracle
                    && w.printing.to_string() == card.scryfall_id
                    && w.definition_hash == refs.definition_revision_ref
                    && w.rules_hash == refs.rules_text_ref
            })
        {
            return Err(NativeInitialError::ForeignSource);
        }
        let bytes = serde_json::to_vec(&(
            "native-published-program-v1",
            self.identity(),
            oracle,
            release.id,
            entry.definition_id,
            entry.definition_revision_id,
            entry.definition_revision_ref.as_str(),
            entry.rules_text_ref.as_str(),
            &refs.definition_revision_ref,
            &refs.rules_text_ref,
            &refs.program_ref,
        ))
        .map_err(NativeInitialError::Json)?;
        Ok(digest(&bytes))
    }

    /// Check that the stored release and every selected definition, text and program
    /// identify these exact immutable inputs. Admission policy is evaluated separately.
    /// # Errors
    /// Rejects unavailable bindings and source references from another revision.
    pub fn verify_constructed_release_content(
        &self,
        release: &preparation_source::PreparationRelease,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<(), NativeInitialError> {
        if release.kernel_ref.as_str() != format!("sha256:{}", native_runtime_identity())
            || release.rules_ref.as_str() != hash_text(self.vocabulary.rules_sha256()).as_str()
            || release.program_manifest_ref.as_str() != self.identity().as_str()
            || !preparation_source::valid_entries(entries)
        {
            return Err(NativeInitialError::ForeignSource);
        }
        for entry in entries {
            let oracle = OracleId::try_from(entry.oracle_ref.as_str().to_owned())
                .map_err(|_| NativeInitialError::ForeignSource)?;
            let refs = self.card_source_refs(&oracle)?;
            let expected = if entry
                .definition_revision_ref
                .as_str()
                .starts_with("catalogue:")
            {
                self.published_program_ref(release, entry, &oracle, &refs)?
            } else if entry.definition_revision_ref.as_str()
                == refs.definition_revision_ref.as_str()
                && entry.rules_text_ref.as_str() == refs.rules_text_ref.as_str()
            {
                refs.program_ref
            } else {
                return Err(NativeInitialError::ForeignSource);
            };
            if entry.program_ref.as_str() != expected.as_str() {
                return Err(NativeInitialError::ForeignSource);
            }
        }
        Ok(())
    }
    /// Inspect the complete selected SQL deck against this finite Legacy catalogue.
    /// Returns main and sideboard counts. Release and table approval are independent.
    /// # Errors
    /// Rejects unsupported identities, sections, quantities, legality or deck size.
    pub fn inspect_selected_legacy_deck(
        &self,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<(u32, u32), NativeInitialError> {
        self.inspect_selected_constructed_deck("legacy", entries)
    }

    /// Check the complete selected deck against its configured ordinary format.
    /// # Errors
    /// Rejects unknown formats and unsupported or illegal selected cards and quantities.
    pub fn inspect_selected_constructed_deck(
        &self,
        format_key: &str,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<(u32, u32), NativeInitialError> {
        if matches!(format_key, "draft" | "cube") {
            return Err(NativeInitialError::ForeignSource);
        }
        self.inspect_selected_match_deck(format_key, entries)
    }

    pub(crate) fn inspect_selected_match_deck(
        &self,
        format_key: &str,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<(u32, u32), NativeInitialError> {
        let counts = self
            .selected_match_deck_counts(format_key, entries)?
            .map_err(|_| NativeInitialError::ForeignSource)?;
        Ok((counts.main, counts.sideboard))
    }

    // Separate a supported source's deck-rule refusal from failures to bind
    // the release's native card data.
    pub(super) fn selected_match_deck_counts(
        &self,
        format_key: &str,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<Result<DeckCounts, DeckFailure>, NativeInitialError> {
        if !preparation_source::valid_entries(entries) {
            return Err(NativeInitialError::ForeignSource);
        }
        let entries = entries
            .iter()
            .map(|e| {
                let oracle_ref = OracleId::try_from(e.oracle_ref.as_str().to_owned())
                    .map_err(|_| NativeInitialError::ForeignSource)?;
                let card = self
                    .source_card(&oracle_ref)
                    .ok_or(NativeInitialError::ForeignSource)?;
                if basic_land(card) != e.unlimited_basic_land {
                    return Err(NativeInitialError::ForeignSource);
                }
                Ok(Entry {
                    oracle_ref,
                    section: e.section.as_str().to_owned(),
                    quantity: e.quantity.clone(),
                })
            })
            .collect::<Result<Vec<_>, NativeInitialError>>()?;
        if matches!(format_key, "draft" | "cube") {
            // The locked room writer proves membership in the seat's sealed
            // pool. Rust independently checks this release's card programs and
            // the sections and size needed by the native game initializer.
            return Ok(limited::check_match_sections(&entries));
        }
        Ok(self.constructed_deck(&Deck {
            format: format_key.to_owned(),
            entries,
        }))
    }
    /// Materialize an ordinary twenty-life constructed candidate from every reserved seat.
    /// Main-deck occurrences keep their exact SQL entry and printing. Sideboards
    /// remain in the retained selection. The caller owns release/profile admission.
    /// # Errors
    /// Rejects invalid selected decks or an invalid native opening.
    pub fn prepare_selected_constructed_opening(
        &self,
        context_id: Uuid,
        source: &preparation_source::NativeReservedGameSource,
        format: &crate::account::FormatSummary,
        first_player_seat_id: Uuid,
        entropy: Vec<NativeRevision>,
    ) -> Result<PreparedNativeOpening, NativeInitialError> {
        if source.metadata().format_id != format.id {
            return Err(NativeInitialError::ForeignSource);
        }
        self.prepare_reserved_opening(
            context_id,
            source,
            &format.key,
            first_player_seat_id,
            entropy,
        )
    }

    pub(crate) fn prepare_reserved_opening(
        &self,
        context_id: Uuid,
        source: &preparation_source::NativeReservedGameSource,
        format_key: &str,
        first_player_seat_id: Uuid,
        entropy: Vec<NativeRevision>,
    ) -> Result<PreparedNativeOpening, NativeInitialError> {
        super::constructed_policy::verify_reserved_format(source.metadata(), format_key)?;
        let mut participants = Vec::new();
        let mut selections = Vec::new();
        for seat in &source.metadata().seats {
            self.verify_constructed_release_content(&source.metadata().release, &seat.entries)?;
            let (participant, selection) =
                self.selected_constructed_participant(format_key, seat.seat_id, &seat.entries)?;
            participants.push(participant);
            selections.push(selection);
        }
        prepare_selected_initial(
            context_id,
            self,
            source,
            NativeInitialSpec {
                participants,
                first_player_seat_id,
                entropy,
            },
            selections,
        )
    }

    fn selected_constructed_participant(
        &self,
        format_key: &str,
        seat_id: Uuid,
        entries: &[preparation_source::PreparationEntry],
    ) -> Result<(NativeInitialParticipant, NativeInitialSelection), NativeInitialError> {
        self.inspect_selected_match_deck(format_key, entries)?;
        let mut cards = Vec::new();
        let mut entry_ids = Vec::new();
        for entry in entries.iter().filter(|e| e.section.as_str() == "main") {
            let quantity = entry
                .quantity
                .integer()
                .to_usize()
                .ok_or(NativeInitialError::ForeignSource)?;
            let oracle_id = OracleId::try_from(entry.oracle_ref.as_str().to_owned())
                .map_err(|_| NativeInitialError::ForeignSource)?;
            cards.extend((0..quantity).map(|_| NativeInitialCard {
                oracle_id: oracle_id.clone(),
                zone: NativeInitialZone::Library,
            }));
            entry_ids.extend(std::iter::repeat_n(entry.entry_id, quantity));
        }
        Ok((
            NativeInitialParticipant {
                seat_id,
                life: 20.into(),
                cards,
            },
            NativeInitialSelection { seat_id, entry_ids },
        ))
    }
    // The constructor pins this finite pool's faces, names, types and texts.
    // Its cards have literal English names and no deck-construction exceptions.
    fn constructed_deck(&self, deck: &Deck) -> Result<DeckCounts, DeckFailure> {
        if !matches!(
            deck.format.as_str(),
            "standard" | "pioneer" | "modern" | "legacy" | "vintage"
        ) {
            return Err(DeckFailure::UnsupportedFormat);
        }
        let mut counts = DeckCounts {
            main: 0,
            sideboard: 0,
        };
        let mut copies = BTreeMap::<&str, u32>::new();
        for entry in &deck.entries {
            let quantity = entry
                .quantity
                .integer()
                .to_u32()
                .filter(|n| *n > 0)
                .ok_or(DeckFailure::InvalidQuantity)?;
            let card = self
                .source_card(&entry.oracle_ref)
                .ok_or(DeckFailure::UnsupportedCard)?;
            let basic = basic_land(card);
            let limit = match card.legalities.get(&deck.format) {
                Some(Legality::Legal) if basic => u32::MAX,
                Some(Legality::Legal) => 4,
                Some(Legality::Restricted) if deck.format == "vintage" => 1,
                _ => return Err(DeckFailure::IllegalCard),
            };
            let total = copies.entry(&card.name).or_default();
            *total = total
                .checked_add(quantity)
                .ok_or(DeckFailure::InvalidQuantity)?;
            if *total > limit {
                return Err(DeckFailure::CopyLimit);
            }
            let section = match entry.section.as_str() {
                "main" => &mut counts.main,
                "sideboard" => &mut counts.sideboard,
                _ => return Err(DeckFailure::UnsupportedSection),
            };
            *section = section
                .checked_add(quantity)
                .ok_or(DeckFailure::InvalidQuantity)?;
        }
        if u64::from(counts.main) + u64::from(counts.sideboard) > 1000 {
            return Err(DeckFailure::PreparationCapacity);
        }
        if counts.main < 60 {
            return Err(DeckFailure::MainTooSmall);
        }
        if counts.sideboard > 15 {
            return Err(DeckFailure::SideboardTooLarge);
        }
        Ok(counts)
    }
}

/// Build a local release review record from the actual executable registry and saved rules.
/// The candidate includes exact input identities, selected bindings and the deck result.
/// # Errors
/// Rejects unavailable, oversized or foreign source files and malformed deck inputs.
/// A structurally invalid deck yields a named rejection in the report.
pub fn inspect_release_candidate(
    pool: NativeProgramPool,
    metadata: &Path,
    snapshot: &Path,
    vocabulary: &Path,
    rules: &Path,
    deck: &Path,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let deck_bytes = read_bounded(deck, 262_144)?;
    let deck: Deck = crate::decode_wire_object(&deck_bytes)?;
    let source = NativeProgramSource::from_bytes(
        pool,
        &read_bounded(metadata, 1_048_576)?,
        read_bounded(snapshot, MAX_NATIVE_ARTIFACT_BYTES)?,
        &read_bounded(vocabulary, 1_048_576)?,
    )?;
    let rules = read_bounded(rules, 8 * 1024 * 1024)?;
    if &<[u8; 32]>::from(Sha256::digest(&rules)) != source.vocabulary.rules_sha256() {
        return Err(NativeInitialError::ForeignSource.into());
    }
    let mut bindings = Vec::with_capacity(source.catalogue.len());
    for row in &source.registry.coverage().cards {
        let Some(id) = &row.oracle_id else { continue };
        let Ok(oracle) = OracleId::try_from(id.clone()) else {
            continue;
        };
        if let Some(card) = source.source_card(&oracle) {
            bindings.push(
                serde_json::json!({"oracleRef":id,"sourcePrinting":card.scryfall_id,"sourceRefs":source.card_source_refs(&oracle)?,
                "name":card.name,"legacy":card.legalities.get("legacy"),"selectedFormatLegality":card.legalities.get(&deck.format),"binding":row.binding}),
            );
        }
    }
    let descriptor: SourceIdentity = crate::decode_wire_object(source.program_identity_bytes())?;
    Ok(serde_json::json!({
        "schema":"spellwake-native-release-candidate-v1",
        "status":"candidate",
        "programIdentity":source.identity(), "descriptor":descriptor,
        "releaseContent":{"kernelRef":format!("sha256:{}",native_runtime_identity()),"rulesRef":hash_text(source.vocabulary.rules_sha256()),"programManifestRef":source.identity()},
        "rulesRevision":source.vocabulary.rules_revision(),"rulesHash":digest(&rules),
        "fullyQualifiedCards":source.fully_qualified_cards(), "bindings":bindings,
        "format":deck.format,"deckInputHash":digest(&deck_bytes),"deckConstruction":source.constructed_deck(&deck),
        "applicationRelease":null
    }))
}

fn candidate_deck_catalogue(
    metadata: &Path,
    snapshot: &Path,
) -> Result<spellwake_content::Inventory, Box<dyn Error>> {
    let archive = read_bounded(snapshot, MAX_NATIVE_ARTIFACT_BYTES)?;
    if digest(&archive).hex() != spellwake_card_programs::TEACHING_SOURCE_SHA256 {
        return Err(NativeInitialError::ForeignSource.into());
    }
    let inventory =
        spellwake_content::snapshot::decode(&read_bounded(metadata, 1_048_576)?, archive)
            .map_err(|error| NativeInitialError::Snapshot(Box::new(error)))?;
    if !inventory.duplicate_oracle_ids.is_empty() {
        return Err(NativeInitialError::ForeignSource.into());
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
    const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
    fn row(id: &str, section: &str, qty: u64) -> Value {
        json!({"oracleRef":id,"section":section,"quantity":qty.to_string()})
    }
    #[test]
    #[ignore = "requires the genuine guarded catalogue and vocabulary"]
    fn constructed_deck_checks_full_sections_and_combined_name_counts() {
        let source = super::super::tests::source(NativeProgramPool::KeywordGameplay);
        let check = |format: &str, rows: Vec<Value>| {
            let bytes = serde_json::to_vec(&json!({"format":format,"entries":rows})).unwrap();
            source.constructed_deck(&crate::decode_wire_object(&bytes).unwrap())
        };
        assert_eq!(
            check(
                "legacy",
                vec![
                    row(FOREST, "main", 58),
                    row(BEARS, "main", 2),
                    row(BEARS, "sideboard", 2),
                    row(FOREST, "sideboard", 13)
                ]
            ),
            Ok(DeckCounts {
                main: 60,
                sideboard: 15
            })
        );
        for format in ["standard", "pioneer", "modern", "legacy", "vintage"] {
            assert_eq!(
                check(format, vec![row(FOREST, "main", 60)]),
                Ok(DeckCounts {
                    main: 60,
                    sideboard: 0
                })
            );
        }
        let edict = "058917c1-21ab-488a-9f9c-591c55f3c596";
        for format in ["standard", "pioneer"] {
            assert_eq!(
                check(
                    format,
                    vec![row(FOREST, "main", 60), row(edict, "sideboard", 1)]
                ),
                Err(DeckFailure::IllegalCard)
            );
        }
        assert!(
            check(
                "legacy",
                vec![row(FOREST, "main", 60), row(edict, "sideboard", 1)]
            )
            .is_ok()
        );
        for (rows, expected) in [
            (
                vec![row(FOREST, "main", 59), row(FOREST, "sideboard", 1)],
                DeckFailure::MainTooSmall,
            ),
            (
                vec![row(FOREST, "main", 60), row(FOREST, "sideboard", 16)],
                DeckFailure::SideboardTooLarge,
            ),
            (
                vec![
                    row(FOREST, "main", 60),
                    row(BEARS, "main", 3),
                    row(BEARS, "sideboard", 2),
                ],
                DeckFailure::CopyLimit,
            ),
            (
                vec![row(FOREST, "main", 60), row(BEARS, "commander", 1)],
                DeckFailure::UnsupportedSection,
            ),
            (
                vec![row(FOREST, "main", 1000), row(FOREST, "sideboard", 1)],
                DeckFailure::PreparationCapacity,
            ),
            (vec![row(FOREST, "main", 0)], DeckFailure::InvalidQuantity),
            (
                vec![row(FOREST, "main", u64::MAX)],
                DeckFailure::InvalidQuantity,
            ),
            (
                vec![row("00000000-0000-4000-8000-000000000001", "main", 60)],
                DeckFailure::UnsupportedCard,
            ),
        ] {
            assert_eq!(check("legacy", rows), Err(expected));
        }
        assert_eq!(
            check("commander", vec![row(FOREST, "main", 100)]),
            Err(DeckFailure::UnsupportedFormat)
        );
    }
    #[test]
    #[ignore = "requires the genuine guarded catalogue and vocabulary"]
    fn selected_library_preserves_occurrences_and_excludes_sideboard() {
        let source = super::super::tests::source(NativeProgramPool::KeywordGameplay);
        let entry = |id, oracle, section, quantity, basic| {
            let bytes = serde_json::to_vec(&json!({
                "entryId":Uuid::from_u128(id),"definitionId":Uuid::from_u128(id+10),
                "oracleRef":oracle,"definitionRevisionId":Uuid::from_u128(id+20),
                "definitionRevisionRef":"fixture:revision","rulesTextRef":"fixture:rules",
                "programRef":"fixture:program","unlimitedBasicLand":basic,"printing":null,
                "section":section,"quantity":quantity
            }))
            .unwrap();
            crate::decode_wire_object(&bytes).unwrap()
        };
        let mut entries = vec![
            entry(1, FOREST, "main", "56", true),
            entry(2, FOREST, "sideboard", "4", true),
            entry(3, BEARS, "main", "4", false),
        ];
        let seat = Uuid::from_u128(10);
        let (player, selection) = source
            .selected_constructed_participant("legacy", seat, &entries)
            .unwrap();
        assert_eq!(player.cards.len(), 60);
        assert_eq!(selection.entry_ids.len(), 60);
        assert!(
            selection.entry_ids[..56]
                .iter()
                .all(|id| *id == entries[0].entry_id)
        );
        assert!(
            selection.entry_ids[56..]
                .iter()
                .all(|id| *id == entries[2].entry_id)
        );
        entries[2].unlimited_basic_land = true;
        assert!(
            source
                .selected_constructed_participant("legacy", seat, &entries)
                .is_err()
        );
        entries[2].unlimited_basic_land = false;
        entries[0].unlimited_basic_land = false;
        assert!(
            source
                .selected_constructed_participant("legacy", seat, &entries)
                .is_err()
        );
    }
    #[test]
    #[ignore = "requires the genuine guarded catalogue and vocabulary"]
    fn release_content_links_the_kernel_rules_and_each_selected_card_program() {
        let source = super::super::tests::source(NativeProgramPool::KeywordGameplay);
        let oracle = OracleId::try_from(FOREST.to_owned()).unwrap();
        let refs = source.card_source_refs(&oracle).unwrap();
        let release = json!({"id":Uuid::from_u128(17),"releaseRef":"fixture:controlled-release","evidenceRef":"fixture:controlled-review",
            "kernelRef":format!("sha256:{}",native_runtime_identity()),"rulesRef":hash_text(source.vocabulary.rules_sha256()),"programManifestRef":source.identity()});
        let entry = json!({"entryId":Uuid::from_u128(1),"definitionId":Uuid::from_u128(2),"oracleRef":FOREST,
            "definitionRevisionId":Uuid::from_u128(3),"definitionRevisionRef":refs.definition_revision_ref,
            "rulesTextRef":refs.rules_text_ref,"programRef":refs.program_ref,"unlimitedBasicLand":true,
            "printing":null,"section":"main","quantity":"60"});
        let check = |r: &Value, e: &Value| {
            let release = crate::decode_wire_object(&serde_json::to_vec(r).unwrap()).unwrap();
            let entry = crate::decode_wire_object(&serde_json::to_vec(e).unwrap()).unwrap();
            source.verify_constructed_release_content(&release, &[entry])
        };
        assert!(check(&release, &entry).is_ok());
        let foreign = json!(digest(b"foreign"));
        for field in ["kernelRef", "rulesRef", "programManifestRef"] {
            let mut changed = release.clone();
            changed[field] = foreign.clone();
            assert!(check(&changed, &entry).is_err());
        }
        for field in ["definitionRevisionRef", "rulesTextRef", "programRef"] {
            let mut changed = entry.clone();
            changed[field] = foreign.clone();
            assert!(check(&release, &changed).is_err());
        }
        let mut published = entry.clone();
        let publication = Uuid::from_u128(41);
        published["definitionRevisionRef"] = json!(format!("catalogue:{publication}"));
        let witness: PublicationWitness =
            serde_json::from_slice(include_bytes!("published-card-witness-v1.json")).unwrap();
        published["rulesTextRef"] = json!(format!(
            "prices/scryfall/{}.jsonl.gz#oracle={FOREST}",
            witness.artifact_hash.hex()
        ));
        // The ordinary card program alone cannot authorize a published SQL revision.
        assert!(check(&release, &published).is_err());
        let stored_release =
            crate::decode_wire_object(&serde_json::to_vec(&release).unwrap()).unwrap();
        let stored_entry =
            crate::decode_wire_object(&serde_json::to_vec(&published).unwrap()).unwrap();
        published["programRef"] = json!(
            source
                .published_program_ref(&stored_release, &stored_entry, &oracle, &refs)
                .unwrap()
        );
        assert!(check(&release, &published).is_ok());
        for field in ["definitionId", "definitionRevisionId"] {
            let mut foreign = published.clone();
            foreign[field] = json!(Uuid::from_u128(57));
            assert!(check(&release, &foreign).is_err());
        }
        for (field, value) in [
            (
                "definitionRevisionRef",
                format!("catalogue:{}", Uuid::from_u128(57)),
            ),
            (
                "rulesTextRef",
                format!(
                    "prices/scryfall/{}.jsonl.gz#oracle={BEARS}",
                    witness.artifact_hash.hex()
                ),
            ),
            ("oracleRef", BEARS.into()),
        ] {
            let mut foreign = published.clone();
            foreign[field] = json!(value);
            assert!(check(&release, &foreign).is_err());
        }
        let mut foreign_release = release.clone();
        foreign_release["id"] = json!(Uuid::from_u128(57));
        assert!(check(&foreign_release, &published).is_err());
        let mut substituted = entry;
        substituted["oracleRef"] = json!(BEARS);
        assert!(check(&release, &substituted).is_err());
    }
}
