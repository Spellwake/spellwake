//! Saved printed descriptions matched to the exact guarded copiable definition.
use super::*;
use crate::player_wire::PlayerCharacteristics;
use spellwake_content::InventoryCard;
use spellwake_engine::program::Program;

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
    let [face] = card.faces.as_slice() else {
        return Err(NativeInitialError::Invalid);
    };
    if card.layout != "normal" || card.name != face.name {
        return Err(NativeInitialError::Invalid);
    }
    face_characteristics(face, None, None)
}
fn split_characteristics(
    card: &InventoryCard,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    let [left, right] = card.faces.as_slice() else {
        return Err(NativeInitialError::Invalid);
    };
    let mut result = face_characteristics(left, None, None)?;
    let right = face_characteristics(right, None, None)?;
    result.name = card.name.clone();
    result.type_line = format!("{} // {}", result.type_line, right.type_line);
    result.rules_text = format!(
        "{}
//
{}",
        result.rules_text, right.rules_text
    );
    result.mana_cost = result
        .mana_cost
        .zip(right.mana_cost)
        .map(|(left, right)| format!("{left} // {right}"));
    result
        .valid()
        .then_some(result)
        .ok_or(NativeInitialError::Invalid)
}
fn face_characteristics(
    face: &spellwake_content::CatalogFace,
    loyalty: Option<&spellwake_contracts::RulesInteger>,
    defense: Option<&spellwake_contracts::RulesInteger>,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    let result = PlayerCharacteristics {
        name: face.name.clone(),
        type_line: face.type_line.clone().ok_or(NativeInitialError::Invalid)?,
        // Missing source text means no printed rules text, as in basic lands.
        rules_text: face.oracle_text.clone().unwrap_or_default(),
        mana_cost: face.mana_cost.clone(),
        printed_power: face.power.clone(),
        printed_toughness: face.toughness.clone(),
        printed_loyalty: loyalty.cloned().map(String::from),
        printed_defense: defense.cloned().map(String::from),
    };
    if !result.valid()
        || (result
            .type_line
            .split_whitespace()
            .any(|word| word == "Planeswalker")
            && loyalty.is_none())
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
            let bound = self
                .registry
                .resolve(oracle)
                .ok_or(RegistryError::MissingBinding)?;
            if bound.scope() == spellwake_card_programs::Scope::SeededVanillaBattlefieldOnly {
                return Err(RegistryError::UnsupportedPlacement.into());
            }
            if let Some((joined,face))=self.meld_results.get(oracle)
                && bound.program().triggered.iter().any(|ability|ability.body.instructions.iter().any(|op|matches!(op,spellwake_engine::program::Instruction::MeldExiledPair(back) if back.as_ref()==program))) {
                if found.is_some(){return Err(NativeInitialError::Invalid);}
                found=Some(NativePrintedCard {oracle:joined,characteristics:face_characteristics(face,program.printed_loyalty.as_ref(),program.printed_defense.as_ref())?});
                continue;
            }
            let face = if bound.program() == program {
                if program.split.is_some() {
                    split_characteristics(card)?
                } else if card.layout == "meld" && program.meld_partner.is_some() {
                    let [face] = card.faces.as_slice() else {
                        return Err(NativeInitialError::Invalid);
                    };
                    face_characteristics(
                        face,
                        program.printed_loyalty.as_ref(),
                        program.printed_defense.as_ref(),
                    )?
                } else if program.reverse_face().is_some() || program.adventure.is_some() {
                    let [front, _] = card.faces.as_slice() else {
                        return Err(NativeInitialError::Invalid);
                    };
                    face_characteristics(
                        front,
                        program.printed_loyalty.as_ref(),
                        program.printed_defense.as_ref(),
                    )?
                } else if program.has_type(spellwake_engine::program::CardType::Planeswalker) {
                    let [face] = card.faces.as_slice() else {
                        return Err(NativeInitialError::Invalid);
                    };
                    face_characteristics(
                        face,
                        program.printed_loyalty.as_ref(),
                        program.printed_defense.as_ref(),
                    )?
                } else {
                    characteristics(card)?
                }
            } else if let Some(split) = bound.program().split.as_ref()
                && (split.left == *program || split.right == *program)
            {
                let [left, right] = card.faces.as_slice() else {
                    return Err(NativeInitialError::Invalid);
                };
                face_characteristics(
                    if split.left == *program { left } else { right },
                    None,
                    None,
                )?
            } else if bound.program().reverse_face() == Some(program)
                || bound.program().adventure.as_deref() == Some(program)
            {
                let [_, back] = card.faces.as_slice() else {
                    return Err(NativeInitialError::Invalid);
                };
                face_characteristics(
                    back,
                    program.printed_loyalty.as_ref(),
                    program.printed_defense.as_ref(),
                )?
            } else {
                continue;
            };
            if found.is_some() || card.oracle_id.as_deref() != Some(oracle.as_str()) {
                return Err(NativeInitialError::Invalid);
            }
            found = Some(NativePrintedCard {
                oracle,
                characteristics: face,
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
