//! Four complete authored sources using shared native library operators.
//! Source signatures describe source records, separately from Program operands.
use super::{Definition, OracleId, ProgramKey, RegistryError, Scope, TeachingPool};
use spellwake_content::{Inventory, Legality};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    mana::{ManaCost, ManaType},
    program::{BodyTarget, ExecutableBody, Instruction, Program, ResolvingPlayer},
};

/// Independent coverage revision for the four-source library-action pool.
pub const LIBRARY_TOP_REGISTRY_VERSION: u32 = 11;
/// Internal dated source family, distinct from complete-card qualification.
pub const LIBRARY_TOP_PROFILE: &str = "library-top-effects-v1";

/// Exact source-family intake failures; no error fabricates a native transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryTopRegistryError {
    /// Full saved source, metadata or complete inventory envelope differs.
    SourceMismatch,
    /// The supplied native vocabulary is foreign to the saved source.
    VocabularyMismatch,
    /// One required identity is absent.
    MissingIdentity { oracle_id: &'static str },
    /// One required Oracle identity occurs more than once.
    DuplicateIdentity { oracle_id: &'static str },
    /// The exact saved printing is changed or duplicated.
    PrintingMismatch { oracle_id: &'static str },
    /// The complete selected materialized face or source signature differs.
    SemanticMismatch { oracle_id: &'static str },
    /// The selected record's saved eligibility premise differs.
    SavedEligibilityMismatch { oracle_id: &'static str },
}
impl std::fmt::Display for LibraryTopRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SourceMismatch => "library-top catalogue source mismatch",
            Self::VocabularyMismatch => "library-top creature vocabulary mismatch",
            Self::MissingIdentity { .. } => "missing library-top card identity",
            Self::DuplicateIdentity { .. } => "duplicate library-top card identity",
            Self::PrintingMismatch { .. } => "library-top card printing mismatch",
            Self::SemanticMismatch { .. } => "library-top complete source semantic mismatch",
            Self::SavedEligibilityMismatch { .. } => "library-top saved eligibility mismatch",
        })
    }
}
impl std::error::Error for LibraryTopRegistryError {}
impl From<LibraryTopRegistryError> for RegistryError {
    fn from(error: LibraryTopRegistryError) -> Self {
        Self::LibraryTop(error)
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Scry,
    Surveil,
    Mill,
    Island,
}
struct SelectedCard {
    oracle_id: &'static str,
    printing_id: &'static str,
    source_signature_v1_sha256: &'static str,
    kind: Kind,
}
const CARDS: [SelectedCard; 4] = [
    SelectedCard {
        oracle_id: "4c9bcba6-87b5-4fb3-97ee-6fe5b739337d",
        printing_id: "4f28e706-c488-4525-ad8a-977a5fa77f80",
        source_signature_v1_sha256: "ec84368c8991b9164f403b6f4403bbdd4ed5a6031c56783513454e743f9feab9",
        kind: Kind::Surveil,
    },
    SelectedCard {
        oracle_id: "713332c1-5bd8-400f-bfff-c1ca0697a043",
        printing_id: "bbc99a51-1501-4525-a3cc-f48249b64bed",
        source_signature_v1_sha256: "ca57d9b78160f45faeb651e680e685f9ddb298e1be69c4f75a3b6ed735c6574d",
        kind: Kind::Scry,
    },
    SelectedCard {
        oracle_id: "83101ba8-a569-4827-8c53-9ca0dfcd59a7",
        printing_id: "4afee6b3-55a4-44e5-b08c-85e0c813cd09",
        source_signature_v1_sha256: "26a45c20c99b6adbccf3bee740037d780763290d282255f97525c87d48c15487",
        kind: Kind::Mill,
    },
    SelectedCard {
        oracle_id: "b2c6aa39-2d2a-459c-a555-fb48ba993373",
        printing_id: "1335d293-3dd2-4f13-bd2c-22181f4528ee",
        source_signature_v1_sha256: "f7794b130f48b16234927c7fb97a10e5ea44b2ec064d8d8856aeedc7904ac678",
        kind: Kind::Island,
    },
];
pub(super) fn selected(id: &str) -> bool {
    CARDS.iter().any(|card| card.oracle_id == id)
}
pub(super) fn definition(
    id: &OracleId,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<Definition>, RegistryError> {
    let Some(card) = CARDS.iter().find(|card| card.oracle_id == id.as_str()) else {
        return Ok(None);
    };
    let one = || spellwake_engine::program::ValueExpr::Literal(1.into());
    let (key, name, text, target, action) = match card.kind {
        Kind::Scry => (
            ProgramKey::LibraryTopScryDrawSpellV1,
            "Opt",
            "Scry 1. (Look at the top card of your library. You may put that card on the bottom.)\nDraw a card.",
            BodyTarget::None,
            Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(
                ResolvingPlayer::Controller,
                one(),
            ))),
        ),
        Kind::Surveil => (
            ProgramKey::LibraryTopSurveilDrawSpellV1,
            "Consider",
            "Surveil 1. (Look at the top card of your library. You may put it into your graveyard.)\nDraw a card.",
            BodyTarget::None,
            Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                    ResolvingPlayer::Controller,
                    one(),
                ),
            )),
        ),
        Kind::Mill => (
            ProgramKey::LibraryTopMillDrawSpellV1,
            "Thought Scour",
            "Target player mills two cards.\nDraw a card.",
            BodyTarget::SinglePlayer,
            Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::mill(
                ResolvingPlayer::TargetPlayer,
                spellwake_engine::program::ValueExpr::Literal(2.into()),
            ))),
        ),
        Kind::Island => {
            let mut definition =
                super::teaching::definition(id, vocabulary, TeachingPool::Mandatory)?.ok_or(
                    LibraryTopRegistryError::MissingIdentity {
                        oracle_id: card.oracle_id,
                    },
                )?;
            // Reuse Basic, land subtype, name and intrinsic mana responsibilities.
            definition.key = ProgramKey::LibraryTopBasicLandV1;
            definition.scope = Scope::LibraryTopOrdinaryCardV1;
            return Ok(Some(definition));
        }
    };
    Ok(Some(Definition {
        key,
        scope: Scope::LibraryTopOrdinaryCardV1,
        name,
        mana: "{U}",
        type_line: "Instant",
        text,
        power: None,
        toughness: None,
        program: Program::instant(
            ManaCost::one(ManaType::Blue),
            ExecutableBody::new(
                target,
                vec![
                    action,
                    Instruction::DrawCards(Box::new(
                        spellwake_engine::toolbox::DrawCardsSpec::simple(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            one(),
                        ),
                    )),
                ],
            ),
        ),
    }))
}
pub(super) fn validate_inventory(
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), RegistryError> {
    if !super::saved_inventory_matches(inventory) {
        return Err(LibraryTopRegistryError::SourceMismatch.into());
    }
    if vocabulary.id().to_string() != super::TEACHING_VOCABULARY_ID {
        return Err(LibraryTopRegistryError::VocabularyMismatch.into());
    }
    for expected in &CARDS {
        let mut rows = inventory
            .cards
            .iter()
            .filter(|card| card.oracle_id.as_deref() == Some(expected.oracle_id));
        let card = rows
            .next()
            .ok_or(LibraryTopRegistryError::MissingIdentity {
                oracle_id: expected.oracle_id,
            })?;
        if rows.next().is_some() {
            return Err(LibraryTopRegistryError::DuplicateIdentity {
                oracle_id: expected.oracle_id,
            }
            .into());
        }
        if card.scryfall_id != expected.printing_id
            || inventory
                .cards
                .iter()
                .filter(|card| card.scryfall_id == expected.printing_id)
                .count()
                != 1
        {
            return Err(LibraryTopRegistryError::PrintingMismatch {
                oracle_id: expected.oracle_id,
            }
            .into());
        }
        if !card.required_by_base_formats || card.legalities.get("legacy") != Some(&Legality::Legal)
        {
            return Err(LibraryTopRegistryError::SavedEligibilityMismatch {
                oracle_id: expected.oracle_id,
            }
            .into());
        }
        let id = OracleId(expected.oracle_id.into());
        let definition =
            definition(&id, vocabulary)?.ok_or(LibraryTopRegistryError::MissingIdentity {
                oracle_id: expected.oracle_id,
            })?;
        if !super::matches(card, &definition)
            || super::signature(card, &id)? != expected.source_signature_v1_sha256
        {
            return Err(LibraryTopRegistryError::SemanticMismatch {
                oracle_id: expected.oracle_id,
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
