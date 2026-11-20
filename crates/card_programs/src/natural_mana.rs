//! Separate twelve-source pool, sharing the original nine and authored basic lands.
//! Source signatures retain their version1 source-record meaning, not Program operands.
use super::{Definition, OracleId, ProgramKey, RegistryError, Scope, TeachingPool, selection};
use spellwake_content::{Inventory, Legality};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Independent report revision for the selected pool with natural-mana helpers.
pub const RESOLUTION_SELECTION_NATURAL_MANA_REGISTRY_VERSION: u32 = 10;
/// Internal dated source pool; no natural-game or complete-card qualification.
pub const RESOLUTION_SELECTION_NATURAL_MANA_PROFILE: &str = "resolution-selection-natural-mana-v1";

struct Helper {
    oracle_id: &'static str,
    printing_id: &'static str,
    source_signature_v1_sha256: &'static str,
}
const HELPERS: [Helper; 3] = [
    Helper {
        oracle_id: "56719f6a-1a6c-4c0a-8d21-18f7d7350b68",
        printing_id: "b233b117-9369-45c1-80c5-cc857386b6be",
        source_signature_v1_sha256: "99c053ff97b10a5083056f046942b7716a7b6e882e3639e8608dee57d3f6aad4",
    },
    Helper {
        oracle_id: "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6",
        printing_id: "2e781bfd-f8cb-4d20-b977-4359f3a21311",
        source_signature_v1_sha256: "add8a0fda94261088dc4c292b7201ebcfadb7ea46be6b3b40ce67dd7049a8fe6",
    },
    Helper {
        oracle_id: "bc71ebf6-2056-41f7-be35-b2e5c34afa99",
        printing_id: "09ac862f-0a51-44dd-b86f-9412b8218b13",
        source_signature_v1_sha256: "c4511685fb2da8e920d2da7ae6217aa981fb64c3670788582dee723013e0c7c6",
    },
];

pub(super) fn selected(oracle_id: &str) -> bool {
    selection::selected(oracle_id) || HELPERS.iter().any(|helper| helper.oracle_id == oracle_id)
}

pub(super) fn definition(
    id: &OracleId,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<Definition>, RegistryError> {
    let Some(helper) = HELPERS
        .iter()
        .find(|helper| helper.oracle_id == id.as_str())
    else {
        return selection::definition(id, vocabulary);
    };
    let Some(mut definition) =
        super::teaching::definition(id, vocabulary, TeachingPool::Mandatory)?
    else {
        return Err(RegistryError::ResolutionSelectionMissingIdentity {
            oracle_id: helper.oracle_id,
        });
    };
    // Swamp, Forest and Plains already share the same authored BasicLand owner.
    // Preserve its complete Program; attribute only the selected mechanism scope.
    definition.key = ProgramKey::ResolutionSelectionBasicLandV1;
    definition.scope = Scope::ResolutionSelectionOrdinaryCardV1;
    Ok(Some(definition))
}

pub(super) fn validate_inventory(
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), RegistryError> {
    // One existing owner checks the full source/metadata/vocabulary envelope and
    // the complete original nine. Never weaken or widen that constructor.
    selection::validate_inventory(inventory, vocabulary)?;
    for expected in &HELPERS {
        let mut matching = inventory
            .cards
            .iter()
            .filter(|card| card.oracle_id.as_deref() == Some(expected.oracle_id));
        let card = matching
            .next()
            .ok_or(RegistryError::ResolutionSelectionMissingIdentity {
                oracle_id: expected.oracle_id,
            })?;
        if matching.next().is_some() {
            return Err(RegistryError::ResolutionSelectionDuplicateIdentity {
                oracle_id: expected.oracle_id,
            });
        }
        if card.scryfall_id != expected.printing_id
            || inventory
                .cards
                .iter()
                .filter(|row| row.scryfall_id == expected.printing_id)
                .count()
                != 1
        {
            return Err(RegistryError::ResolutionSelectionPrintingMismatch {
                oracle_id: expected.oracle_id,
            });
        }
        if !card.required_by_base_formats || card.legalities.get("legacy") != Some(&Legality::Legal)
        {
            return Err(RegistryError::ResolutionSelectionLegalityMismatch {
                oracle_id: expected.oracle_id,
            });
        }
        let id = OracleId(expected.oracle_id.to_owned());
        let definition = definition(&id, vocabulary)?.ok_or(
            RegistryError::ResolutionSelectionMissingIdentity {
                oracle_id: expected.oracle_id,
            },
        )?;
        if !super::matches(card, &definition)
            || super::signature(card, &id)? != expected.source_signature_v1_sha256
        {
            return Err(RegistryError::ResolutionSelectionSemanticMismatch {
                oracle_id: expected.oracle_id,
            });
        }
    }
    Ok(())
}
