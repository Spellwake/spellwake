//! Identity- and semantic-guarded bindings into the restricted native executor.
//! Authored programs are partial implementations, not inferred from names, general
//! Oracle parsing, behavioral evidence or whole-card certification.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_content::{CatalogFace, Inventory, InventoryCard};
use spellwake_contracts::creature_types::{CreatureTypeVocabulary, CreatureVocabularyId};
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    effects::{Instruction, Program, SeedCard},
    mana::{ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
};
use std::collections::{BTreeMap, BTreeSet};

pub mod catalog_support;
pub mod commander;
pub mod custom_card;
mod keyword_creatures;
pub use keyword_creatures::{
    CURRENT_KEYWORD_PROFILE, KEYWORD_CREATURE_PROFILE, KEYWORD_CREATURE_REGISTRY_VERSION,
    KEYWORD_GAMEPLAY_PROFILE, KEYWORD_GAMEPLAY_REGISTRY_VERSION, KEYWORD_GAMEPLAY_V3_PROFILE,
    KEYWORD_GAMEPLAY_V4_PROFILE, KEYWORD_GAMEPLAY_V5_PROFILE, KEYWORD_GAMEPLAY_V6_PROFILE,
};
mod library_top;
pub use library_top::{LIBRARY_TOP_PROFILE, LIBRARY_TOP_REGISTRY_VERSION, LibraryTopRegistryError};
mod natural_mana;
mod selection;
mod teaching;
pub use natural_mana::{
    RESOLUTION_SELECTION_NATURAL_MANA_PROFILE, RESOLUTION_SELECTION_NATURAL_MANA_REGISTRY_VERSION,
};
pub use selection::{RESOLUTION_SELECTION_PROFILE, RESOLUTION_SELECTION_REGISTRY_VERSION};
use teaching::TeachingPool;
pub use teaching::{
    OPTIONAL_PAYMENT_PROFILE, OPTIONAL_PAYMENT_REGISTRY_VERSION, TEACHING_PROFILE,
    TEACHING_REGISTRY_VERSION, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};

/// Coverage revision for the finite Blue interaction ordinary-card profile.
pub const INTERACTION_REGISTRY_VERSION: u32 = 6;
/// Internal selected Blue/Green ordinary-card pool; not full-card qualification.
pub const INTERACTION_PROFILE: &str = "blue-interaction-v1";

/// Coverage revision for the finite mandatory-trigger ordinary-card profile.
pub const MANDATORY_TRIGGER_REGISTRY_VERSION: u32 = 7;
/// Selected entry/death-trigger pool; not full-card qualification.
pub const MANDATORY_TRIGGER_PROFILE: &str = "mandatory-trigger-v1";

pub const BOLT: &str = "4457ed35-7c10-48c8-9776-456485fdf070";
pub const UNSUMMON: &str = "837182db-1bf3-4a2c-bd01-1af9d9873561";
pub const COUNTERSPELL: &str = "cc187110-1148-4090-bbb8-e205694a39f5";
pub const TWINCAST: &str = "8f878efc-850f-43d2-a6fe-5ea8d1dd5afb";
pub const LION: &str = "e3c0b11e-7340-4e0a-98b7-91b38439d4f9";

/// Canonical UUID spelling for catalogue rules identities, distinct from a game
/// CardId or a printing's Scryfall identity. Invalid external spellings never bind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OracleId(String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidOracleId;
impl std::fmt::Display for InvalidOracleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("expected canonical lowercase Oracle UUID")
    }
}
impl std::error::Error for InvalidOracleId {}
impl TryFrom<String> for OracleId {
    type Error = InvalidOracleId;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
        {
            return Err(InvalidOracleId);
        }
        Ok(Self(value))
    }
}
impl From<OracleId> for String {
    fn from(value: OracleId) -> Self {
        value.0
    }
}
impl OracleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgramKey {
    /// Single-face, fixed-cost creature with a complete keyword-only rules box.
    KeywordCreatureV1,
    /// Complete card compiled by the second keyword gameplay profile.
    KeywordGameplayV2,
    /// Additional source-admitted native mechanics in the third profile.
    KeywordGameplayV3,
    /// Source-admitted mechanics in the current gameplay registry.
    KeywordGameplayCurrent,
    /// Source-guarded cards compiled by the fifth gameplay profile.
    KeywordGameplayV5,
    /// Source-guarded cards compiled by the sixth gameplay profile.
    KeywordGameplayV6,
    /// Source-guarded cards compiled by the seventh gameplay profile.
    KeywordGameplayV7,
    /// Complete source-backed scry followed by controller draw.
    LibraryTopScryDrawSpellV1,
    /// Complete source-backed surveil followed by controller draw.
    LibraryTopSurveilDrawSpellV1,
    /// Complete source-backed targeted mill followed by controller draw.
    LibraryTopMillDrawSpellV1,
    /// Maintained basic Island definition attributed to the four-source pool.
    LibraryTopBasicLandV1,
    BoltCreatureV1,
    UnsummonCreatureV1,
    LionBattlefieldV2,
    CounterspellSupportedInstantV2,
    TwincastSingleTargetInstantV1,
    TeachingBasicLandV1,
    TeachingVanillaCreatureV1,
    /// Complete authored basic land in the selected Blue/Green pool.
    InteractionBasicLandV1,
    /// Complete authored vanilla creature in the selected Blue/Green pool.
    InteractionVanillaCreatureV1,
    /// Ordinary instant returning one target creature to its owner's hand.
    InteractionBounceCreatureV1,
    /// Ordinary instant countering any target spell.
    InteractionCounterSpellV1,
    /// Ordinary sorcery drawing two cards for its controller.
    InteractionDrawTwoV1,
    /// Basic land in the finite mandatory-trigger pool.
    MandatoryBasicLandV1,
    /// Vanilla creature in the finite mandatory-trigger pool.
    MandatoryVanillaCreatureV1,
    /// Creature with complete mandatory triggers and any printed mana ability.
    MandatoryTriggeredCreatureV1,
    /// Instant creating one simultaneous group of plain Soldier tokens.
    MandatoryTokenCreationSpellV1,
    /// Ordinary instant returning one target creature to its owner's hand.
    MandatoryBounceCreatureV1,
    /// Ordinary instant countering any target spell.
    MandatoryCounterSpellV1,
    /// Ordinary basic land in the optional/payment pool.
    OptionalPaymentBasicLandV1,
    /// Ordinary vanilla creature in the optional/payment pool.
    OptionalPaymentVanillaCreatureV1,
    /// Complete selected creature trigger and printed mana definitions.
    OptionalPaymentTriggeredCreatureV1,
    /// Ordinary instant returning its target creature to its owner's hand.
    OptionalPaymentBounceCreatureV1,
    /// Ordinary instant countering any target spell.
    OptionalPaymentCounterSpellV1,
    /// Ordinary sorcery drawing two cards for its controller.
    OptionalPaymentDrawTwoV1,
    /// Ordinary instant creating two plain Soldier tokens together.
    OptionalPaymentTokenCreationSpellV1,
    /// Counter unless the exact target spell controller pays three generic mana.
    OptionalPaymentUnlessCounterSpellV1,
    /// Copy an admitted instant or sorcery through the shared copy owner.
    OptionalPaymentCopySpellV1,
    /// Plain artifact with its complete color-choice tap mana ability.
    OptionalPaymentManaArtifactV1,
    /// Plain enchantment with its complete optional/payment cast trigger.
    OptionalPaymentTriggeredEnchantmentV1,
    /// Ordinary targeted creature-sacrifice spell in the selected resolution pool.
    ResolutionSelectionTargetedSacrificeSpellV1,
    /// Targeted creature sacrifice followed by its complete life-loss clause.
    ResolutionSelectionSacrificeLifeLossSpellV1,
    /// Ordinary each-player creature-sacrifice spell.
    ResolutionSelectionEachPlayerSacrificeSpellV1,
    /// Ordinary affected-player-selected discard spell with a player target.
    ResolutionSelectionTargetedDiscardSpellV1,
    /// Ordinary each-player affected-player-selected discard spell.
    ResolutionSelectionEachPlayerDiscardSpellV1,
    /// Real creature spell with its complete mandatory enters-trigger body.
    ResolutionSelectionTriggeredCreatureV1,
    /// Existing maintained basic-land definition used in this finite pool.
    ResolutionSelectionBasicLandV1,
    /// Existing maintained vanilla creature definition used in this finite pool.
    ResolutionSelectionVanillaCreatureV1,
    /// Existing maintained two-Soldier creation spell used in this finite pool.
    ResolutionSelectionTokenCreationSpellV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Ordinary single-face creatures in the pinned keyword-only profile.
    KeywordCreatureOrdinaryCardV1,
    /// Rules and actions admitted by the second keyword gameplay compiler.
    KeywordGameplayV2,
    /// Additional source-admitted native mechanics in the third profile.
    KeywordGameplayV3,
    /// Source-admitted mechanics in the current gameplay registry.
    KeywordGameplayCurrent,
    /// Source-guarded cards compiled by the fifth gameplay profile.
    KeywordGameplayV5,
    /// Source-guarded cards compiled by the sixth gameplay profile.
    KeywordGameplayV6,
    /// Source-guarded cards compiled by the seventh gameplay profile.
    KeywordGameplayV7,
    /// Four dated library-action sources; no complete-card qualification.
    LibraryTopOrdinaryCardV1,
    CreatureTargetOnly,
    SupportedInstantSpellTargetOnly,
    SeededVanillaBattlefieldOnly,
    TeachingOrdinaryCardV1,
    /// Ordinary library, hand, casting and permanent definitions for the Blue/Green pool.
    InteractionOrdinaryCardV1,
    /// Exact finite mandatory entry/death-trigger ordinary-card pool.
    MandatoryTriggerOrdinaryCardV1,
    /// Exact source-backed optional-effect and resolution-payment ordinary pool.
    OptionalPaymentOrdinaryCardV1,
    /// Source-backed sacrifice/discard mechanisms; not complete-card qualification.
    ResolutionSelectionOrdinaryCardV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum BindingStatus {
    Missing,
    Blocked {
        reason: &'static str,
    },
    Partial {
        program: ProgramKey,
        scope: Scope,
        semantic_sha256: String,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardCoverage {
    pub name: String,
    pub oracle_id: Option<String>,
    pub required: bool,
    pub binding: BindingStatus,
    pub behavior_evidence: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub source_sha256: String,
    pub source_records: usize,
    pub required_records: usize,
    pub required_partial_bindings: usize,
    pub required_without_binding: usize,
    pub fully_qualified_cards: usize,
    pub behavior_evidence: &'static str,
    pub registry_version: u32,
    pub creature_vocabulary_id: CreatureVocabularyId,
    pub cards: Vec<CardCoverage>,
}
/// Private compiled instruction and guarded source identity. The caller cannot
/// alter a resolved binding through this registry.
#[derive(Debug, Clone)]
pub struct BoundProgram {
    key: ProgramKey,
    scope: Scope,
    instruction: Program,
}
impl BoundProgram {
    /// Immutable guarded definition materialized by this binding.
    pub fn program(&self) -> &Program {
        &self.instruction
    }
    pub fn key(&self) -> ProgramKey {
        self.key
    }
    pub fn scope(&self) -> Scope {
        self.scope
    }
}
pub struct Registry {
    bindings: BTreeMap<OracleId, BoundProgram>,
    coverage: Coverage,
}
#[derive(Debug)]
pub enum RegistryError {
    /// Foreign or altered materialized input for the keyword-only profile.
    KeywordCreatureSourceMismatch,
    /// Foreign creature subtype vocabulary for the keyword-only profile.
    KeywordCreatureVocabularyMismatch,
    /// An admitted keyword definition failed native validation.
    InvalidKeywordCreatureDefinition {
        oracle_id: OracleId,
        reason: spellwake_engine::program::DefinitionError,
    },
    /// Every rejected candidate when several definitions fail the same registry build.
    InvalidKeywordCreatureDefinitions(Vec<(OracleId, spellwake_engine::program::DefinitionError)>),
    /// The independently closed four-source library-action guard refused intake.
    LibraryTop(LibraryTopRegistryError),
    /// The resolution-selection pool requires its pinned full source/metadata.
    ResolutionSelectionSourceMismatch,
    /// The resolution-selection pool requires its source-qualified vocabulary.
    ResolutionSelectionVocabularyMismatch,
    /// A selected resolution-selection identity is absent.
    ResolutionSelectionMissingIdentity {
        oracle_id: &'static str,
    },
    /// A selected resolution-selection identity occurs more than once.
    ResolutionSelectionDuplicateIdentity {
        oracle_id: &'static str,
    },
    /// A selected resolution-selection face differs from its complete source guard.
    ResolutionSelectionSemanticMismatch {
        oracle_id: &'static str,
    },
    /// A selected saved printing is changed, absent or duplicated.
    ResolutionSelectionPrintingMismatch {
        oracle_id: &'static str,
    },
    /// A selected identity is not required and saved Legacy-legal.
    ResolutionSelectionLegalityMismatch {
        oracle_id: &'static str,
    },
    Encoding(serde_json::Error),
    Name(spellwake_contracts::names::NameDefinitionError),
    /// An authored literal token template failed its existing typed validator.
    TokenDefinition(spellwake_engine::token::TokenDefinitionError),
    MissingBinding,
    UnsupportedPlacement,
    MissingCreatureType,
    /// The finite teaching profile requires the selected complete source snapshot.
    TeachingSourceMismatch,
    /// The optional/payment pool requires the exact full saved source and metadata.
    OptionalPaymentSourceMismatch,
    /// The optional/payment pool requires its source-qualified vocabulary.
    OptionalPaymentVocabularyMismatch,
    /// A selected optional/payment identity is absent.
    OptionalPaymentMissingIdentity {
        oracle_id: &'static str,
    },
    /// A selected optional/payment identity occurs more than once.
    OptionalPaymentDuplicateIdentity {
        oracle_id: &'static str,
    },
    /// A complete selected face differs from its authored source guard.
    OptionalPaymentSemanticMismatch {
        oracle_id: &'static str,
    },
    /// A selected printing is changed, absent or duplicated in the inventory.
    OptionalPaymentPrintingMismatch {
        oracle_id: &'static str,
    },
    /// A selected identity is not required and saved Legacy-legal.
    OptionalPaymentLegalityMismatch {
        oracle_id: &'static str,
    },
    /// The finite teaching profile requires its source-qualified vocabulary.
    TeachingVocabularyMismatch,
    TeachingMissingIdentity {
        oracle_id: &'static str,
    },
    TeachingDuplicateIdentity {
        oracle_id: &'static str,
    },
    TeachingSemanticMismatch {
        oracle_id: &'static str,
    },
    TeachingLegalityMismatch {
        oracle_id: &'static str,
    },
    /// The interaction profile requires the selected complete source snapshot.
    InteractionSourceMismatch,
    /// The interaction profile requires its source-qualified vocabulary.
    InteractionVocabularyMismatch,
    /// A selected interaction identity is absent from the materialized inventory.
    InteractionMissingIdentity {
        oracle_id: &'static str,
    },
    /// A selected interaction identity occurs more than once.
    InteractionDuplicateIdentity {
        oracle_id: &'static str,
    },
    /// A selected interaction card differs from its complete authored face guard.
    InteractionSemanticMismatch {
        oracle_id: &'static str,
    },
    /// A selected interaction card is not saved Modern-legal and required.
    InteractionLegalityMismatch {
        oracle_id: &'static str,
    },
    /// The mandatory-trigger profile requires the pinned full snapshot.
    MandatorySourceMismatch,
    /// The mandatory-trigger profile requires its source-qualified vocabulary.
    MandatoryVocabularyMismatch,
    /// A selected mandatory-trigger identity is absent.
    MandatoryMissingIdentity {
        oracle_id: &'static str,
    },
    /// A selected mandatory-trigger identity occurs more than once.
    MandatoryDuplicateIdentity {
        oracle_id: &'static str,
    },
    /// A selected mandatory-trigger face differs from its complete guard.
    MandatorySemanticMismatch {
        oracle_id: &'static str,
    },
    /// A selected mandatory-trigger identity is not saved Modern-legal/required.
    MandatoryLegalityMismatch {
        oracle_id: &'static str,
    },
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeywordCreatureSourceMismatch => {
                f.write_str("keyword creature catalogue source mismatch")
            }
            Self::KeywordCreatureVocabularyMismatch => {
                f.write_str("keyword creature vocabulary mismatch")
            }
            Self::InvalidKeywordCreatureDefinition { oracle_id, reason } => {
                write!(
                    f,
                    "invalid compiled definition for {}: {reason:?}",
                    oracle_id.as_str()
                )
            }
            Self::InvalidKeywordCreatureDefinitions(errors) => {
                write!(f, "{} invalid compiled definitions", errors.len())?;
                for (id, error) in errors {
                    write!(f, "\n{}: {error:?}", id.as_str())?;
                }
                Ok(())
            }
            Self::LibraryTop(error) => error.fmt(f),
            Self::ResolutionSelectionSourceMismatch => {
                f.write_str("resolution-selection catalogue source mismatch")
            }
            Self::ResolutionSelectionVocabularyMismatch => {
                f.write_str("resolution-selection creature vocabulary mismatch")
            }
            Self::ResolutionSelectionMissingIdentity { .. } => {
                f.write_str("missing resolution-selection card identity")
            }
            Self::ResolutionSelectionDuplicateIdentity { .. } => {
                f.write_str("duplicate resolution-selection card identity")
            }
            Self::ResolutionSelectionSemanticMismatch { .. } => {
                f.write_str("resolution-selection card semantic mismatch")
            }
            Self::ResolutionSelectionPrintingMismatch { .. } => {
                f.write_str("resolution-selection card printing mismatch")
            }
            Self::ResolutionSelectionLegalityMismatch { .. } => {
                f.write_str("resolution-selection card legality mismatch")
            }
            Self::OptionalPaymentSourceMismatch => {
                f.write_str("optional-payment catalogue source mismatch")
            }
            Self::OptionalPaymentVocabularyMismatch => {
                f.write_str("optional-payment creature vocabulary mismatch")
            }
            Self::OptionalPaymentMissingIdentity { .. } => {
                f.write_str("missing optional-payment card identity")
            }
            Self::OptionalPaymentDuplicateIdentity { .. } => {
                f.write_str("duplicate optional-payment card identity")
            }
            Self::OptionalPaymentSemanticMismatch { .. } => {
                f.write_str("optional-payment card semantic mismatch")
            }
            Self::OptionalPaymentPrintingMismatch { .. } => {
                f.write_str("optional-payment card printing mismatch")
            }
            Self::OptionalPaymentLegalityMismatch { .. } => {
                f.write_str("optional-payment card legality mismatch")
            }
            Self::MandatorySourceMismatch => {
                f.write_str("mandatory-trigger catalogue source mismatch")
            }
            Self::MandatoryVocabularyMismatch => {
                f.write_str("mandatory-trigger creature vocabulary mismatch")
            }
            Self::MandatoryMissingIdentity { .. } => {
                f.write_str("missing mandatory-trigger card identity")
            }
            Self::MandatoryDuplicateIdentity { .. } => {
                f.write_str("duplicate mandatory-trigger card identity")
            }
            Self::MandatorySemanticMismatch { .. } => {
                f.write_str("mandatory-trigger card semantic mismatch")
            }
            Self::MandatoryLegalityMismatch { .. } => {
                f.write_str("mandatory-trigger card legality mismatch")
            }
            Self::TokenDefinition(_) => f.write_str("invalid authored token template"),
            Self::TeachingSourceMismatch => f.write_str("teaching catalogue source mismatch"),
            Self::TeachingVocabularyMismatch => {
                f.write_str("teaching creature vocabulary mismatch")
            }
            Self::TeachingMissingIdentity { .. } => f.write_str("missing teaching card identity"),
            Self::TeachingDuplicateIdentity { .. } => {
                f.write_str("duplicate teaching card identity")
            }
            Self::TeachingSemanticMismatch { .. } => f.write_str("teaching card semantic mismatch"),
            Self::TeachingLegalityMismatch { .. } => f.write_str("teaching card legality mismatch"),
            Self::InteractionSourceMismatch => f.write_str("interaction catalogue source mismatch"),
            Self::InteractionVocabularyMismatch => {
                f.write_str("interaction creature vocabulary mismatch")
            }
            Self::InteractionMissingIdentity { .. } => {
                f.write_str("missing interaction card identity")
            }
            Self::InteractionDuplicateIdentity { .. } => {
                f.write_str("duplicate interaction card identity")
            }
            Self::InteractionSemanticMismatch { .. } => {
                f.write_str("interaction card semantic mismatch")
            }
            Self::InteractionLegalityMismatch { .. } => {
                f.write_str("interaction card legality mismatch")
            }
            Self::MissingCreatureType => {
                f.write_str("selected vocabulary has no required Cat creature type")
            }
            Self::Name(_) => f.write_str("invalid guarded English card name"),
            Self::Encoding(_) => f.write_str("program signature encoding failed"),
            Self::UnsupportedPlacement => {
                f.write_str("program scope does not support this initial placement")
            }
            Self::MissingBinding => {
                f.write_str("no guarded program binding for this Oracle identity")
            }
        }
    }
}
impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LibraryTop(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Name(error) => Some(error),
            Self::TokenDefinition(spellwake_engine::token::TokenDefinitionError::Name(error)) => {
                Some(error)
            }
            _ => None,
        }
    }
}

// Shared exact saved-corpus envelope, independent of a selected card inventory.
// Raw byte authentication remains the upstream snapshot owner's responsibility.
fn saved_inventory_matches(inventory: &Inventory) -> bool {
    inventory.compressed_sha256 == TEACHING_SOURCE_SHA256
        && teaching::source_metadata_matches(inventory)
        && inventory.source_records == 39_196
        && inventory.cards.len() == 39_196
        && inventory.unique_oracle_ids == 39_196
        && inventory.missing_oracle_ids == 0
        && inventory.duplicate_oracle_ids.is_empty()
        && inventory.base_format_inventory_resolved
}

#[derive(Serialize)]
struct Signature<'a> {
    version: u32,
    oracle_id: &'a str,
    name: &'a str,
    layout: &'a str,
    faces: &'a [CatalogFace],
}
fn signature(card: &InventoryCard, id: &OracleId) -> Result<String, RegistryError> {
    let bytes = serde_json::to_vec(&Signature {
        version: 1,
        oracle_id: id.as_str(),
        name: &card.name,
        layout: &card.layout,
        faces: &card.faces,
    })
    .map_err(RegistryError::Encoding)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
struct Definition {
    key: ProgramKey,
    scope: Scope,
    name: &'static str,
    mana: &'static str,
    type_line: &'static str,
    text: &'static str,
    power: Option<&'static str>,
    toughness: Option<&'static str>,
    program: Program,
}
fn definition(id: &OracleId) -> Option<Definition> {
    let mut def = Definition {
        key: ProgramKey::BoltCreatureV1,
        scope: Scope::CreatureTargetOnly,
        name: "Lightning Bolt",
        mana: "{R}",
        type_line: "Instant",
        text: "Lightning Bolt deals 3 damage to any target.",
        power: None,
        toughness: None,
        program: Program::instant(
            ManaCost::one(ManaType::Red),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 3.into() }],
            ),
        ),
    };
    match id.as_str() {
        BOLT => {}
        UNSUMMON => {
            def.key = ProgramKey::UnsummonCreatureV1;
            def.name = "Unsummon";
            def.mana = "{U}";
            def.text = "Return target creature to its owner's hand.";
            def.program = Program::instant(
                ManaCost::one(ManaType::Blue),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            );
        }
        COUNTERSPELL => {
            def.key = ProgramKey::CounterspellSupportedInstantV2;
            def.scope = Scope::SupportedInstantSpellTargetOnly;
            def.name = "Counterspell";
            def.mana = "{U}{U}";
            def.text = "Counter target spell.";
            def.program = Program::instant(
                ManaCost::specific(ManaType::Blue, 2.into()),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    vec![Instruction::CounterSpell],
                ),
            );
        }
        TWINCAST => {
            def.key = ProgramKey::TwincastSingleTargetInstantV1;
            def.scope = Scope::SupportedInstantSpellTargetOnly;
            def.name = "Twincast";
            def.mana = "{U}{U}";
            def.text =
                "Copy target instant or sorcery spell. You may choose new targets for the copy.";
            def.program = Program::instant(
                ManaCost::specific(ManaType::Blue, 2.into()),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                    vec![Instruction::CopySpell],
                ),
            );
        }
        LION => {
            def.key = ProgramKey::LionBattlefieldV2;
            def.scope = Scope::SeededVanillaBattlefieldOnly;
            def.name = "Silvercoat Lion";
            def.mana = "{1}{W}";
            def.type_line = "Creature — Cat";
            def.text = "";
            def.power = Some("2");
            def.toughness = Some("2");
            def.program = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
        }
        _ => return None,
    }
    Some(def)
}
fn matches(card: &InventoryCard, def: &Definition) -> bool {
    let [face] = card.faces.as_slice() else {
        return false;
    };
    card.name == def.name
        && card.layout == "normal"
        && face.name == def.name
        && face.type_line.as_deref() == Some(def.type_line)
        && face.mana_cost.as_deref() == Some(def.mana)
        && face.oracle_text.as_deref() == Some(def.text)
        && face.power.as_deref() == def.power
        && face.toughness.as_deref() == def.toughness
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegistryProfile {
    LibraryTop,
    Reference,
    Authored(TeachingPool),
    ResolutionSelection,
    ResolutionSelectionNaturalMana,
}

impl Registry {
    /// Bind exactly Opt, Consider, Thought Scour and the saved basic Island.
    ///
    /// Load genuine complete saved bytes with `spellwake_content::snapshot` first.
    /// The trusted Inventory boundary retains hashes and materialized faces, not
    /// raw bytes: this constructor cannot reauthenticate discarded input bytes.
    /// Full source/metadata/vocabulary, selected printing/face/signature and saved
    /// eligibility guards are mandatory. All four bindings remain Partial; this
    /// grants no ordinary capture, public view, format or complete-card support.
    ///
    /// # Errors
    /// Returns `RegistryError::LibraryTop` for a foreign envelope or missing,
    /// duplicate or changed selected source, preserving the typed guard through
    /// `Error::source()`. Existing name/signature errors retain their variants.
    pub fn from_library_top_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        library_top::validate_inventory(inventory, vocabulary)?;
        Self::assemble(inventory, vocabulary, RegistryProfile::LibraryTop)
    }
    /// Bind only exact known identities whose full materialized supported signature
    /// matches. Changed text/stats/layout, missing identities and duplicates fail closed.
    pub fn from_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::assemble(inventory, vocabulary, RegistryProfile::Reference)
    }

    /// Bind exactly the twenty authored cards in the internal teaching profile.
    ///
    /// The caller must obtain `inventory` by loading the actual pinned bytes
    /// through `spellwake_content::snapshot`; this method checks the retained
    /// source identity and every selected materialized semantic record. It does
    /// not re-hash bytes that are no longer present in `Inventory`.
    ///
    /// Coverage contains only this selected pool. Bindings remain `Partial` and
    /// global fully-qualified-card coverage remains zero.
    ///
    /// # Errors
    /// Returns a typed error for a foreign source or vocabulary, any missing,
    /// duplicated, changed or non-Modern-legal selected card, invalid selected
    /// creature types, or failure to encode a guarded signature or object name.
    pub fn from_teaching_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        teaching::validate_inventory(inventory, vocabulary, TeachingPool::Vanilla)?;
        Self::assemble(
            inventory,
            vocabulary,
            RegistryProfile::Authored(TeachingPool::Vanilla),
        )
    }

    /// Bind exactly the twenty authored Blue/Green ordinary interaction cards.
    ///
    /// The caller must load the actual pinned bytes through
    /// `spellwake_content::snapshot` before passing their materialized inventory.
    /// This checks the retained full-source identity, vocabulary and all selected
    /// records; it cannot re-hash bytes no longer owned by `Inventory`.
    /// Coverage remains `Partial`, with global fully-qualified coverage zero.
    ///
    /// # Errors
    /// Returns a typed error for a foreign source or vocabulary, a missing,
    /// duplicate, semantically changed or non-Modern-legal selected record, an
    /// unavailable creature type, or a guarded name/signature encoding failure.
    pub fn from_interaction_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        teaching::validate_inventory(inventory, vocabulary, TeachingPool::Interaction)?;
        Self::assemble(
            inventory,
            vocabulary,
            RegistryProfile::Authored(TeachingPool::Interaction),
        )
    }

    /// Bind exactly nineteen selected mandatory entry/death-trigger pool cards.
    ///
    /// Obtain the inventory through `spellwake_content::snapshot` using the
    /// pinned complete source bytes. This constructor checks the retained source
    /// identity, vocabulary, saved legality and every selected materialized face;
    /// it cannot re-hash bytes no longer owned by `Inventory`. Bindings remain
    /// `Partial`, and global fully qualified coverage stays zero.
    ///
    /// # Errors
    /// Returns a typed error for a foreign source/vocabulary, missing, duplicate,
    /// semantically changed or non-Modern-legal selected record, an unavailable
    /// subtype, invalid token template, or name/signature encoding failure.
    pub fn from_mandatory_trigger_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        teaching::validate_inventory(inventory, vocabulary, TeachingPool::Mandatory)?;
        Self::assemble(
            inventory,
            vocabulary,
            RegistryProfile::Authored(TeachingPool::Mandatory),
        )
    }

    /// Bind the exact nineteen selected optional/payment ordinary-card sources.
    ///
    /// Load the actual full saved bytes with `spellwake_content::snapshot` first.
    /// This constructor checks the resulting source identity and metadata, all
    /// complete selected faces, printing identities and saved Legacy legality.
    /// Raw fields absent from `Inventory` remain guarded by that byte boundary;
    /// this constructor cannot re-hash bytes no longer owned by the inventory.
    /// Every binding remains `Partial`, and complete-card coverage stays zero.
    ///
    /// # Errors
    /// Returns a typed error for foreign source/metadata/vocabulary, a missing,
    /// duplicate or changed selected identity, face or printing, a non-Legacy-legal
    /// source, an unavailable subtype, or token/name/signature encoding failure.
    pub fn from_optional_payment_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        teaching::validate_inventory(inventory, vocabulary, TeachingPool::OptionalPayment)?;
        Self::assemble(
            inventory,
            vocabulary,
            RegistryProfile::Authored(TeachingPool::OptionalPayment),
        )
    }

    /// Bind exactly the nine saved sacrifice/discard and helper identities.
    ///
    /// Load the actual complete pinned bytes through `spellwake_content::snapshot`
    /// first. This constructor checks the resulting full-source identity and
    /// metadata, vocabulary, complete selected materialized faces, printing joins
    /// and saved Legacy legality. It cannot re-hash bytes no longer in Inventory.
    /// Existing Bears, Island and Raise definitions have their maintained owners;
    /// no name-based runtime behavior is introduced. All bindings remain Partial.
    ///
    /// # Errors
    /// Returns a typed error for foreign source/metadata/vocabulary, a missing,
    /// duplicate, changed or non-Legacy-legal selected record, unavailable creature
    /// type, or token/name/source-signature encoding failure. This is source
    /// materialization, not execution, privacy or complete-card qualification.
    pub fn from_resolution_selection_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        selection::validate_inventory(inventory, vocabulary)?;
        Self::assemble(inventory, vocabulary, RegistryProfile::ResolutionSelection)
    }

    /// Revalidate a decoded source against this immutable nine-source registry.
    ///
    /// Requires the exact Registry9 profile, source hash and vocabulary carried
    /// by this registry, then delegates metadata, printing, complete selected
    /// faces and saved legality to the existing resolution-selection guard.
    /// This leaves stored Programs unchanged and grants no execution/card
    /// qualification.
    ///
    /// The loader still authenticates actual metadata/snapshot/vocabulary bytes.
    /// Inventory cannot re-hash bytes it no longer contains; raw-line receipts
    /// and set/collector/language annotations remain upstream input obligations.
    ///
    /// # Errors
    /// Returns existing resolution-selection source or vocabulary errors for a
    /// different registry profile/source/vocabulary, and preserves every typed
    /// identity, printing, face, legality or encoding error from the shared guard.
    pub fn validate_resolution_selection_source(
        &self,
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), RegistryError> {
        if self.coverage.registry_version != RESOLUTION_SELECTION_REGISTRY_VERSION
            || self.coverage.source_sha256 != inventory.compressed_sha256
        {
            return Err(RegistryError::ResolutionSelectionSourceMismatch);
        }
        if self.coverage.creature_vocabulary_id != vocabulary.id() {
            return Err(RegistryError::ResolutionSelectionVocabularyMismatch);
        }
        selection::validate_inventory(inventory, vocabulary)
    }

    /// Bind the original nine selected sources plus three saved natural-mana basics.
    ///
    /// Load the actual pinned complete bytes through `spellwake_content::snapshot`
    /// first. This separately guarded pool preserves the original nine-source
    /// constructor and adds exact Swamp, Forest and Plains source/printing/Legacy
    /// checks. Definitions reuse the existing teaching basic-land owner, including
    /// intrinsic mana derived from land subtypes. Every binding remains Partial;
    /// this does not execute natural games or qualify complete cards or formats.
    ///
    /// # Errors
    /// Returns the existing resolution-selection errors for foreign source,
    /// metadata or vocabulary, missing/duplicate/changed selected identities,
    /// printings, complete faces or saved Legacy legality. Underlying name,
    /// token and source-signature encoding errors retain their existing variants.
    pub fn from_resolution_selection_natural_mana_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        natural_mana::validate_inventory(inventory, vocabulary)?;
        Self::assemble(
            inventory,
            vocabulary,
            RegistryProfile::ResolutionSelectionNaturalMana,
        )
    }

    fn assemble(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
        profile: RegistryProfile,
    ) -> Result<Self, RegistryError> {
        let mut seen = BTreeSet::new();
        let mut duplicate = BTreeSet::new();
        for card in &inventory.cards {
            if let Some(id) = &card.oracle_id
                && !seen.insert(id.clone())
            {
                duplicate.insert(id.clone());
            }
        }
        let mut bindings = BTreeMap::new();
        let mut cards = Vec::new();
        let mut partial = 0;
        let mut required = 0;
        for card in inventory.cards.iter().filter(|card| match profile {
            RegistryProfile::LibraryTop => {
                card.oracle_id.as_deref().is_some_and(library_top::selected)
            }
            RegistryProfile::Reference => true,
            RegistryProfile::Authored(pool) => card
                .oracle_id
                .as_deref()
                .is_some_and(|id| teaching::selected(id, pool)),
            RegistryProfile::ResolutionSelection => {
                card.oracle_id.as_deref().is_some_and(selection::selected)
            }
            RegistryProfile::ResolutionSelectionNaturalMana => card
                .oracle_id
                .as_deref()
                .is_some_and(natural_mana::selected),
        }) {
            required += usize::from(card.required_by_base_formats);
            let binding = match card
                .oracle_id
                .clone()
                .and_then(|id| OracleId::try_from(id).ok())
            {
                None => BindingStatus::Blocked {
                    reason: "missing-or-invalid-oracle-id",
                },
                Some(id) if duplicate.contains(id.as_str()) => BindingStatus::Blocked {
                    reason: "ambiguous-oracle-id",
                },
                Some(id) => match match profile {
                    RegistryProfile::LibraryTop => library_top::definition(&id, vocabulary)?,
                    RegistryProfile::Reference => definition(&id),
                    RegistryProfile::Authored(pool) => teaching::definition(&id, vocabulary, pool)?,
                    RegistryProfile::ResolutionSelection => selection::definition(&id, vocabulary)?,
                    RegistryProfile::ResolutionSelectionNaturalMana => {
                        natural_mana::definition(&id, vocabulary)?
                    }
                } {
                    None => BindingStatus::Missing,
                    Some(def) if !matches(card, &def) => BindingStatus::Blocked {
                        reason: "semantic-guard-mismatch",
                    },
                    Some(mut def) => {
                        if def.key == ProgramKey::LionBattlefieldV2 {
                            let cat = vocabulary
                                .resolve_label("Cat")
                                .ok_or(RegistryError::MissingCreatureType)?;
                            def.program =
                                Program::creature(None, 2.into(), 2.into(), BTreeSet::from([cat]));
                        }
                        def.program.names = spellwake_contracts::names::ObjectNames::single(
                            spellwake_contracts::names::EnglishCardName::new(card.name.clone())
                                .map_err(RegistryError::Name)?,
                        );
                        // The authored ordinary basics retain Basic. The five
                        // reference definitions have no printed supertypes.
                        if profile == RegistryProfile::Reference {
                            def.program.supertypes = BTreeSet::new();
                        }
                        let semantic_sha256 = signature(card, &id)?;
                        if card.required_by_base_formats {
                            partial += 1;
                        }
                        bindings.insert(
                            id,
                            BoundProgram {
                                key: def.key,
                                scope: def.scope,
                                instruction: def.program,
                            },
                        );
                        BindingStatus::Partial {
                            program: def.key,
                            scope: def.scope,
                            semantic_sha256,
                        }
                    }
                },
            };
            cards.push(CardCoverage {
                name: card.name.clone(),
                oracle_id: card.oracle_id.clone(),
                required: card.required_by_base_formats,
                binding,
                behavior_evidence: "not-evaluated-by-registry",
            });
        }
        Ok(Self {
            bindings,
            coverage: Coverage {
                source_sha256: inventory.compressed_sha256.clone(),
                source_records: inventory.cards.len(),
                required_records: required,
                required_partial_bindings: partial,
                required_without_binding: required - partial,
                fully_qualified_cards: 0,
                behavior_evidence: "not-evaluated-by-registry",
                registry_version: match profile {
                    RegistryProfile::LibraryTop => LIBRARY_TOP_REGISTRY_VERSION,
                    RegistryProfile::Reference => 4,
                    RegistryProfile::Authored(pool) => pool.registry_version(),
                    RegistryProfile::ResolutionSelection => RESOLUTION_SELECTION_REGISTRY_VERSION,
                    RegistryProfile::ResolutionSelectionNaturalMana => {
                        RESOLUTION_SELECTION_NATURAL_MANA_REGISTRY_VERSION
                    }
                },
                creature_vocabulary_id: vocabulary.id(),
                cards,
            },
        })
    }
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }
    pub fn resolve(&self, id: &OracleId) -> Option<&BoundProgram> {
        self.bindings.get(id)
    }
    /// Create an ordinary seed only through the guarded catalogue binding. This
    /// grants no collection ownership, public action authority or full card support.
    pub fn seed(
        &self,
        oracle: &OracleId,
        card: CardId,
        owner: SeatId,
        destination: ObjectDestination,
    ) -> Result<SeedCard, RegistryError> {
        let binding = self.resolve(oracle).ok_or(RegistryError::MissingBinding)?;
        if binding.scope == Scope::SeededVanillaBattlefieldOnly
            && !matches!(destination, ObjectDestination::Battlefield { .. })
        {
            return Err(RegistryError::UnsupportedPlacement);
        }
        Ok(SeedCard {
            card,
            owner,
            destination,
            program: binding.instruction.clone(),
        })
    }
}
