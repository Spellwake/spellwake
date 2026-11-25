//! Match-scoped custom card program overlay compiled from rules-only frozen snapshots.
//!
//! Never mutates the shared official `NativeProgramSource::registry` or catalogue, and never
//! stores art bytes or PII inside native `Initial` artifacts.

use super::*;
use crate::player_wire::PlayerCharacteristics;
use spellwake_card_programs::{
    catalog_support::SUPPORT_COMPILER_REVISION,
    custom_card::{
        CustomCardCompileInput, CustomCardFaceRules, compile_custom_card, normalize_engine_layout,
    },
};
use spellwake_content::{CatalogFace, InventoryCard};
use spellwake_engine::{
    characteristics::{Color, PrintedStat},
    effects::SeedCard,
    object_identity::{CardId, ObjectDestination},
    program::{CardType, Instruction, Program, ProgramKind},
};
use std::collections::{BTreeMap, BTreeSet};

/// Rules-only custom card snapshot frozen inside `native-initial-v4` / `native-initial-commander-v4`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenCustomCardRules {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub custom_card_revision_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub custom_card_id: Uuid,
    pub rules_sha256: String,
    pub name: String,
    pub layout: String,
    pub rarity: String,
    pub faces_rules: Vec<CustomCardFaceRules>,
    pub compiler_revision: String,
}

impl FrozenCustomCardRules {
    /// Synthetic `OracleId` for this immutable custom card revision inside one match overlay.
    ///
    /// # Errors
    /// Rejects nil revision IDs.
    pub fn synthetic_oracle_id(&self) -> Result<OracleId, NativeInitialError> {
        if self.custom_card_revision_id.is_nil() {
            return Err(NativeInitialError::Invalid);
        }
        OracleId::try_from(self.custom_card_revision_id.to_string())
            .map_err(|_| NativeInitialError::Invalid)
    }
}

/// Compiled match-scoped custom card programs and printed face records.
pub struct NativeProgramOverlay {
    programs: BTreeMap<OracleId, Arc<Program>>,
    cards: BTreeMap<Uuid, (OracleId, Arc<Program>, InventoryCard)>,
    color_identities: BTreeMap<OracleId, BTreeSet<Color>>,
}

impl NativeProgramOverlay {
    /// Recompile and verify every frozen custom card snapshot against `vocabulary`.
    ///
    /// Fails closed if any card fails Stage 2 playability, mismatches its recorded `rules_sha256`,
    /// targets a different compiler revision, or has duplicate revision IDs.
    ///
    /// # Errors
    /// Returns `NativeInitialError::ForeignSource` or `NativeInitialError::Invalid` on verification failure.
    pub fn compile(
        snapshots: &[FrozenCustomCardRules],
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, NativeInitialError> {
        let mut programs = BTreeMap::new();
        let mut cards = BTreeMap::new();
        let mut color_identities = BTreeMap::new();

        for snap in snapshots {
            if snap.custom_card_revision_id.is_nil()
                || snap.custom_card_id.is_nil()
                || snap.compiler_revision != SUPPORT_COMPILER_REVISION
            {
                return Err(NativeInitialError::ForeignSource);
            }
            let oracle = snap.synthetic_oracle_id()?;
            if programs.contains_key(&oracle) {
                return Err(NativeInitialError::Invalid);
            }
            let compile_input = CustomCardCompileInput {
                layout: snap.layout.clone(),
                rarity: snap.rarity.clone(),
                faces: snap.faces_rules.clone(),
            };
            let compiled = compile_custom_card(&compile_input, vocabulary);
            if compiled.report.rules_sha256 != snap.rules_sha256 || !compiled.report.stage2_playable
            {
                return Err(NativeInitialError::ForeignSource);
            }
            let Some(program) = compiled.program else {
                return Err(NativeInitialError::ForeignSource);
            };
            let arc_prog = Arc::new(*program);

            let faces = snap
                .faces_rules
                .iter()
                .map(|f| CatalogFace {
                    name: f.name.clone(),
                    type_line: Some(f.type_line.clone()),
                    oracle_text: (!f.oracle_text.is_empty()).then(|| f.oracle_text.clone()),
                    mana_cost: f.mana_cost.clone(),
                    power: f.power.clone(),
                    toughness: f.toughness.clone(),
                })
                .collect();
            let inv_card = InventoryCard {
                scryfall_id: snap.custom_card_revision_id.to_string(),
                oracle_id: Some(oracle.as_str().to_owned()),
                name: snap.name.clone(),
                layout: normalize_engine_layout(&snap.layout).to_owned(),
                faces,
                legalities: BTreeMap::new(),
                eligible_formats: vec![],
                required_by_base_formats: false,
                program_evidence: "custom-card-overlay-v1",
            };

            let mut ci = BTreeSet::new();
            for sym in &compiled.report.color_identity {
                match sym.as_str() {
                    "W" => {
                        ci.insert(Color::White);
                    }
                    "U" => {
                        ci.insert(Color::Blue);
                    }
                    "B" => {
                        ci.insert(Color::Black);
                    }
                    "R" => {
                        ci.insert(Color::Red);
                    }
                    "G" => {
                        ci.insert(Color::Green);
                    }
                    _ => {}
                }
            }

            programs.insert(oracle.clone(), Arc::clone(&arc_prog));
            color_identities.insert(oracle.clone(), ci);
            cards.insert(snap.custom_card_revision_id, (oracle, arc_prog, inv_card));
        }

        Ok(Self {
            programs,
            cards,
            color_identities,
        })
    }

    /// Seed one custom card occurrence into the opening engine setup.
    #[must_use]
    pub fn seed(
        &self,
        oracle: &OracleId,
        card_id: CardId,
        owner: SeatId,
        destination: ObjectDestination,
    ) -> Option<SeedCard> {
        let program = self.programs.get(oracle)?;
        Some(SeedCard {
            card: card_id,
            owner,
            destination,
            program: (**program).clone(),
        })
    }

    /// Look up the color identity of a custom card by its synthetic `OracleId`.
    #[must_use]
    pub fn color_identity(&self, oracle: &OracleId) -> Option<&BTreeSet<Color>> {
        self.color_identities.get(oracle)
    }

    /// Resolve printed characteristics for a specific physical custom card revision,
    /// disambiguating functional reprints with identical compiled `Program`s.
    ///
    /// # Errors
    /// Rejects malformed face metadata or public text bounds.
    pub fn printed_card_for_revision(
        &self,
        rev_id: Uuid,
        definition: &Program,
    ) -> Result<Option<PlayerCharacteristics>, NativeInitialError> {
        let Some((_, bound, card)) = self.cards.get(&rev_id) else {
            return Ok(None);
        };
        match_card_face(bound, card, definition)
    }

    /// Resolve printed characteristics across all custom cards in this match overlay
    /// (used for copied spells/permanents where no physical custom card origin is present).
    ///
    /// # Errors
    /// Rejects malformed face metadata or public text bounds.
    pub fn printed_card(
        &self,
        definition: &Program,
    ) -> Result<Option<PlayerCharacteristics>, NativeInitialError> {
        for (_, bound, card) in self.cards.values() {
            if let Some(ch) = match_card_face(bound, card, definition)? {
                return Ok(Some(ch));
            }
        }
        Ok(None)
    }

    /// Project token characteristics for any `CreateTokens` or `CreateAttackingTokens`
    /// template emitted by custom cards in this match overlay (including non-vanilla
    /// and non-creature tokens such as Treasure, Clue, Food, or keyword tokens).
    ///
    /// # Errors
    /// Rejects malformed token definitions or invalid public text bounds.
    pub fn token_description(
        &self,
        definition: &Program,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Option<PlayerCharacteristics>, NativeInitialError> {
        for (_, bound, _) in self.cards.values() {
            let mut bodies = Vec::new();
            if let ProgramKind::Spell(spell) = &bound.kind {
                bodies.push(&spell.body);
            }
            bodies.extend(bound.activated.iter().map(|a| &a.body));
            bodies.extend(bound.triggered.iter().map(|a| &a.body));
            if let Some(back) = bound.back_face() {
                if let ProgramKind::Spell(spell) = &back.kind {
                    bodies.push(&spell.body);
                }
                bodies.extend(back.activated.iter().map(|a| &a.body));
                bodies.extend(back.triggered.iter().map(|a| &a.body));
            }
            for instruction in bodies.iter().flat_map(|body| &body.instructions) {
                let creations = match instruction {
                    Instruction::CreateTokens { creations } => creations.as_slice(),
                    Instruction::CreateAttackingTokens { creations, .. } => creations.as_slice(),
                    _ => continue,
                };
                for creation in creations {
                    let materialized = creation
                        .definition
                        .materialize(vocabulary)
                        .map_err(RegistryError::TokenDefinition)?;
                    if &materialized == definition {
                        return general_token_description(definition, vocabulary).map(Some);
                    }
                }
            }
        }
        Ok(None)
    }
}

fn match_card_face(
    bound: &Program,
    card: &InventoryCard,
    program: &Program,
) -> Result<Option<PlayerCharacteristics>, NativeInitialError> {
    if bound == program || bound.prototype.as_deref() == Some(program) {
        if program.split.is_some() {
            return super::printed::split_characteristics_pub(card).map(Some);
        }
        let Some(front) = card.faces.first() else {
            return Err(NativeInitialError::Invalid);
        };
        return super::printed::face_characteristics_pub(
            front,
            program.printed_loyalty.as_ref(),
            program.printed_defense.as_ref(),
        )
        .map(Some);
    }
    if let Some(split) = bound.split.as_ref()
        && (split.left == *program || split.right == *program)
    {
        let [left, right] = card.faces.as_slice() else {
            return Err(NativeInitialError::Invalid);
        };
        return super::printed::face_characteristics_pub(
            if split.left == *program { left } else { right },
            None,
            None,
        )
        .map(Some);
    }
    if bound.back_face() == Some(program)
        || bound.adventure.as_deref() == Some(program)
        || bound.prepare_spell.as_deref() == Some(program)
    {
        let [_, back] = card.faces.as_slice() else {
            return Err(NativeInitialError::Invalid);
        };
        return super::printed::face_characteristics_pub(
            back,
            program.printed_loyalty.as_ref(),
            program.printed_defense.as_ref(),
        )
        .map(Some);
    }
    Ok(None)
}

fn general_token_description(
    program: &Program,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PlayerCharacteristics, NativeInitialError> {
    let ProgramKind::Permanent(permanent) = &program.kind else {
        return Err(NativeInitialError::Invalid);
    };
    let name = program
        .names
        .iter()
        .next()
        .map(|n| n.as_str().to_owned())
        .unwrap_or_else(|| "Token".to_owned());

    let mut type_words = vec!["Token".to_owned()];
    for ct in &program.card_types {
        type_words.push(match ct {
            CardType::Artifact => "Artifact".to_owned(),
            CardType::Battle => "Battle".to_owned(),
            CardType::Creature => "Creature".to_owned(),
            CardType::Enchantment => "Enchantment".to_owned(),
            CardType::Instant => "Instant".to_owned(),
            CardType::Kindred => "Kindred".to_owned(),
            CardType::Land => "Land".to_owned(),
            CardType::Planeswalker => "Planeswalker".to_owned(),
            CardType::Sorcery => "Sorcery".to_owned(),
        });
    }

    let mut subtypes = Vec::new();
    for id in &program.creature_subtypes {
        if let Some(label) = vocabulary.label(*id) {
            subtypes.push(label.to_owned());
        }
    }
    for art_sub in &program.artifact_subtypes {
        subtypes.push(format!("{art_sub:?}"));
    }
    for ench_sub in &program.enchantment_subtypes {
        subtypes.push(format!("{ench_sub:?}"));
    }
    let type_line = if subtypes.is_empty() {
        type_words.join(" ")
    } else {
        format!("{} \u{2014} {}", type_words.join(" "), subtypes.join(" "))
    };

    let (printed_power, printed_toughness) = match permanent.printed_pt.as_ref() {
        Some(stats) => {
            let p = match &stats.power {
                PrintedStat::Number(n) => Some(String::from(n.clone())),
                _ => Some("*".to_owned()),
            };
            let t = match &stats.toughness {
                PrintedStat::Number(n) => Some(String::from(n.clone())),
                _ => Some("*".to_owned()),
            };
            (p, t)
        }
        None => (None, None),
    };

    // Effective token keywords are projected separately via `publicProperties.keywords`.
    let rules_text = String::new();

    let result = PlayerCharacteristics {
        name,
        type_line,
        rules_text,
        mana_cost: None,
        printed_power,
        printed_toughness,
        printed_loyalty: program.printed_loyalty.clone().map(String::from),
        printed_defense: program.printed_defense.clone().map(String::from),
    };
    if !result.valid() {
        return Err(NativeInitialError::Invalid);
    }
    Ok(result)
}
