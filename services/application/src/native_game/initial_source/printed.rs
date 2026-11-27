//! Saved printed descriptions matched to the exact guarded copiable definition.
use super::*;
use crate::player_wire::PlayerCharacteristics;
use spellwake_content::InventoryCard;
use spellwake_engine::program::Program;
use std::collections::BTreeMap;

/// Exact-equality prefilter index over the immutable retained catalogue.
///
/// Maps each bound program face's leading name (`program.names.iter().next()`)
/// to candidate `OracleId`s. Because `Program` equality requires identical
/// `names`, any matching definition is guaranteed to appear in the bucket, while
/// `printed_card` still enforces exact `Program == Program` equality.
pub(super) struct PrintedCardIndex {
    missing_binding: bool,
    has_unsupported_placement: bool,
    by_name: BTreeMap<Option<String>, Vec<OracleId>>,
}

impl PrintedCardIndex {
    pub(super) fn build(
        catalogue: &BTreeMap<OracleId, InventoryCard>,
        registry: &Registry,
        _meld_results: &BTreeMap<OracleId, (OracleId, spellwake_content::CatalogFace)>,
    ) -> Self {
        let mut missing_binding = false;
        let mut has_unsupported_placement = false;
        let mut by_name: BTreeMap<Option<String>, Vec<OracleId>> = BTreeMap::new();
        let mut record = |program: &Program, oracle: &OracleId| {
            let key = program
                .names
                .iter()
                .next()
                .map(|name| name.as_str().to_owned());
            let bucket = by_name.entry(key).or_default();
            if bucket.last() != Some(oracle) {
                bucket.push(oracle.clone());
            }
        };
        for oracle in catalogue.keys() {
            let Some(bound) = registry.resolve(oracle) else {
                missing_binding = true;
                continue;
            };
            if bound.scope() == spellwake_card_programs::Scope::SeededVanillaBattlefieldOnly {
                has_unsupported_placement = true;
            }
            let root = bound.program();
            record(root, oracle);
            if let Some(proto) = root.prototype.as_deref() {
                record(proto, oracle);
            }
            if let Some(split) = root.split.as_ref() {
                record(&split.left, oracle);
                record(&split.right, oracle);
            }
            if let Some(back) = root.back_face() {
                record(back, oracle);
            }
            if let Some(adventure) = root.adventure.as_deref() {
                record(adventure, oracle);
            }
            if let Some(prepare) = root.prepare_spell.as_deref() {
                record(prepare, oracle);
            }
        }
        Self {
            missing_binding,
            has_unsupported_placement,
            by_name,
        }
    }
}

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

pub(super) fn split_characteristics_pub(
    card: &InventoryCard,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    split_characteristics(card)
}

pub(super) fn face_characteristics_pub(
    face: &spellwake_content::CatalogFace,
    loyalty: Option<&spellwake_contracts::RulesInteger>,
    defense: Option<&spellwake_contracts::RulesInteger>,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    face_characteristics(face, loyalty, defense)
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
        if self.printed_index.missing_binding {
            return Err(RegistryError::MissingBinding.into());
        }
        if self.printed_index.has_unsupported_placement {
            return Err(RegistryError::UnsupportedPlacement.into());
        }
        let key = program
            .names
            .iter()
            .next()
            .map(|name| name.as_str().to_owned());
        let Some(candidates) = self.printed_index.by_name.get(&key) else {
            return Ok(None);
        };
        let mut primary: Option<NativePrintedCard<'_>> = None;
        let mut secondary: Option<NativePrintedCard<'_>> = None;
        for oracle in candidates {
            let Some((oracle, card)) = self.catalogue.get_key_value(oracle) else {
                continue;
            };
            let bound = self
                .registry
                .resolve(oracle)
                .ok_or(RegistryError::MissingBinding)?;
            if bound.scope() == spellwake_card_programs::Scope::SeededVanillaBattlefieldOnly {
                return Err(RegistryError::UnsupportedPlacement.into());
            }
            let _ = self.meld_results.get(oracle);
            let (is_primary, face) = if bound.program() == program
                || bound.program().prototype.as_deref() == Some(program)
            {
                let face = if program.split.is_some() {
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
                } else if program.back_face().is_some()
                    || program.adventure.is_some()
                    || program.prepare_spell.is_some()
                {
                    let [front, _] = card.faces.as_slice() else {
                        return Err(NativeInitialError::Invalid);
                    };
                    face_characteristics(
                        front,
                        program.printed_loyalty.as_ref(),
                        program.printed_defense.as_ref(),
                    )?
                } else if program.has_type(spellwake_engine::program::CardType::Planeswalker)
                    || program.printed_defense.is_some()
                {
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
                };
                (true, face)
            } else if let Some(split) = bound.program().split.as_ref()
                && (split.left == *program || split.right == *program)
            {
                let [left, right] = card.faces.as_slice() else {
                    return Err(NativeInitialError::Invalid);
                };
                (
                    false,
                    face_characteristics(
                        if split.left == *program { left } else { right },
                        None,
                        None,
                    )?,
                )
            } else if bound.program().back_face() == Some(program)
                || bound.program().adventure.as_deref() == Some(program)
                || bound.program().prepare_spell.as_deref() == Some(program)
            {
                let [_, back] = card.faces.as_slice() else {
                    return Err(NativeInitialError::Invalid);
                };
                (
                    false,
                    face_characteristics(
                        back,
                        program.printed_loyalty.as_ref(),
                        program.printed_defense.as_ref(),
                    )?,
                )
            } else {
                continue;
            };
            if card.oracle_id.as_deref() != Some(oracle.as_str()) {
                return Err(NativeInitialError::Invalid);
            }
            if is_primary {
                if primary.is_some() {
                    return Err(NativeInitialError::Invalid);
                }
                primary = Some(NativePrintedCard {
                    oracle,
                    characteristics: face,
                });
            } else if let Some(existing) = &secondary {
                // Distinct multi-face cards may share an identical sub-face (such as a
                // prepared spell) only when their printed characteristics match exactly.
                if existing.characteristics != face {
                    return Err(NativeInitialError::Invalid);
                }
            } else {
                secondary = Some(NativePrintedCard {
                    oracle,
                    characteristics: face,
                });
            }
        }
        Ok(primary.or(secondary))
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
