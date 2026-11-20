//! Saved printed descriptions matched to the exact guarded copiable definition.
use super::*;
use crate::player_wire::PlayerCharacteristics;
use spellwake_content::InventoryCard;
use spellwake_engine::{
    object_identity::{CardId, ObjectDestination},
    program::Program,
};

/// A single immutable rules description, without a selected application printing.
/// This is neither effective characteristics nor permission to expose a card.
pub struct NativePrintedCard<'a> {
    oracle: &'a OracleId,
    characteristics: PlayerCharacteristics,
}
impl NativePrintedCard<'_> {
    /// Guarded rules identity, never an internal definition or printing row ID.
    pub fn oracle_id(&self) -> &OracleId {
        self.oracle
    }
    /// Exact saved single-face text. Numeric strings are printed descriptions only.
    pub fn characteristics(&self) -> &PlayerCharacteristics {
        &self.characteristics
    }
}

fn characteristics(card: &InventoryCard) -> Result<PlayerCharacteristics, NativeInitialError> {
    // The retained content format has no printed loyalty field. Do not silently
    // represent a planeswalker or a multi-face layout with incomplete metadata.
    let [face] = card.faces.as_slice() else {
        return Err(NativeInitialError::Invalid);
    };
    if card.layout != "normal" || card.name != face.name {
        return Err(NativeInitialError::Invalid);
    }
    let result = PlayerCharacteristics {
        name: face.name.clone(),
        type_line: face.type_line.clone().ok_or(NativeInitialError::Invalid)?,
        // Missing source text means no printed rules text, as in basic lands.
        rules_text: face.oracle_text.clone().unwrap_or_default(),
        mana_cost: face.mana_cost.clone(),
        printed_power: face.power.clone(),
        printed_toughness: face.toughness.clone(),
        printed_loyalty: None,
    };
    if !result.valid()
        || result
            .type_line
            .split_whitespace()
            .any(|word| word == "Planeswalker")
    {
        return Err(NativeInitialError::Invalid);
    }
    Ok(result)
}

impl NativeProgramSource {
    /// Join an actual copiable Program to its exact saved guarded source record.
    /// Use the native copiable definition before later text/type/ability changes;
    /// an arbitrary name match, current catalogue or serialized Debug is not used.
    ///
    /// The result supplies no visibility decision, selected printing, application
    /// definition ID, ability label, token description or complete-card qualification.
    /// No matching guarded definition returns None rather than invented metadata.
    ///
    /// # Errors
    /// Rejects ambiguous definitions, an inconsistent retained Oracle, unsupported
    /// face/loyalty metadata or public text bounds. Preserves registry seed errors.
    pub fn printed_card(
        &self,
        program: &Program,
    ) -> Result<Option<NativePrintedCard<'_>>, NativeInitialError> {
        let mut found = None;
        for (oracle, card) in &self.catalogue {
            // Reuse the sole guarded definition materializer. These coordinates
            // only retrieve its Program; no GameObject or game is initialized.
            let bound = self.registry.seed(
                oracle,
                CardId(1),
                SeatId(0),
                ObjectDestination::LibraryBottom,
            )?;
            if &bound.program != program {
                continue;
            }
            if found.is_some() || card.oracle_id.as_deref() != Some(oracle.as_str()) {
                return Err(NativeInitialError::Invalid);
            }
            found = Some(NativePrintedCard {
                oracle,
                characteristics: characteristics(card)?,
            });
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_content::CatalogFace;
    use std::collections::BTreeMap;

    fn card() -> InventoryCard {
        InventoryCard {
            scryfall_id: Uuid::from_u128(1).to_string(),
            oracle_id: Some(Uuid::from_u128(2).to_string()),
            name: "Explicit text-boundary fixture".into(),
            layout: "normal".into(),
            faces: vec![CatalogFace {
                name: "Explicit text-boundary fixture".into(),
                type_line: Some("Basic Land".into()),
                oracle_text: None,
                mana_cost: None,
                power: None,
                toughness: None,
            }],
            legalities: BTreeMap::new(),
            eligible_formats: vec![],
            required_by_base_formats: false,
            program_evidence: "synthetic text validation only",
        }
    }
    #[test]
    fn printed_descriptions_preserve_source_text_and_enforce_existing_public_bounds() {
        let mut source = card();
        let plain = characteristics(&source).unwrap();
        assert!(plain.rules_text.is_empty());
        assert!(plain.mana_cost.is_none());
        assert!(plain.printed_loyalty.is_none());
        source.faces[0].oracle_text = Some("Exact\nprinted {U} text".into());
        source.faces[0].power = Some("*".into());
        let described = characteristics(&source).unwrap();
        assert_eq!(described.rules_text, "Exact\nprinted {U} text");
        assert_eq!(described.printed_power.as_deref(), Some("*"));
        source.faces[0].oracle_text = Some("x".repeat(8193));
        assert!(characteristics(&source).is_err());
        source.faces[0].oracle_text = Some("private\0invalid".into());
        assert!(characteristics(&source).is_err());
    }
    #[test]
    fn incomplete_faces_names_and_layouts_do_not_produce_guessed_descriptions() {
        let mut source = card();
        source.faces[0].type_line = None;
        assert!(characteristics(&source).is_err());
        source = card();
        source.faces[0].name = "Another face".into();
        assert!(characteristics(&source).is_err());
        source = card();
        source.layout = "transform".into();
        assert!(characteristics(&source).is_err());
        source = card();
        source.faces.clear();
        assert!(characteristics(&source).is_err());
    }
}
