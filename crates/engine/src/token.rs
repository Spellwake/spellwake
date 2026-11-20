//! Literal creation templates with name provenance (CR 111.3, 111.4 and 612.2a).
//! Program remains the sole characteristic/capability definition. Materialized
//! objects own that Program; the template's name source is instruction semantics.
use crate::{
    characteristics::CreatureTypeTextChange,
    land::LandType,
    program::{DefinitionError, Program},
};
use spellwake_contracts::{
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::{EnglishCardName, NameDefinitionError, ObjectNames},
};
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

/// Players who create each literal group in a simultaneous instruction.
/// The current participant model is free-for-all; teams and targeted recipients
/// require their own represented relationship/operand contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRecipients {
    Controller,
    EachPlayer,
    EachOpponent,
}

/// One authored group; every group in a CreateTokens instruction enters together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCreation {
    pub definition: TokenDefinition,
    pub amount: crate::program::InstructionAmount,
    pub recipients: TokenRecipients,
}
impl TokenCreation {
    /// Create one token for the resolving controller through the batch mechanism.
    pub fn one(definition: TokenDefinition) -> Self {
        Self {
            definition,
            amount: crate::program::InstructionAmount::Literal(1.into()),
            recipients: TokenRecipients::Controller,
        }
    }
}

/// Ephemeral coordinate in one bound creation instruction, never an ObjectId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TokenBatchMember(usize);
impl TokenBatchMember {
    /// Construct a requested coordinate; the issued frame validates membership.
    pub fn new(ordinal: usize) -> Self {
        Self(ordinal)
    }
    pub fn ordinal(self) -> usize {
        self.0
    }
}

/// Owned creation preview. No actual incarnation has been allocated yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEntryPreview {
    pub member: TokenBatchMember,
    pub creator: crate::priority::SeatId,
    pub definition: Program,
}

/// Executor-issued earliest-to-latest order for one APNAP player's newcomers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenTimestampOrder {
    pub(crate) revision: u64,
    pub(crate) entry: crate::stack_window::StackEntryId,
    pub(crate) instruction: usize,
    pub(crate) chooser: crate::priority::SeatId,
    pub(crate) members: Vec<TokenEntryPreview>,
}
impl TokenTimestampOrder {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn entry(&self) -> crate::stack_window::StackEntryId {
        self.entry
    }
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    pub fn chooser(&self) -> crate::priority::SeatId {
        self.chooser
    }
    pub fn members(&self) -> &[TokenEntryPreview] {
        &self.members
    }
}

/// Authored subtype words in name order, independent of set iteration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TokenSubtype {
    Creature(CreatureTypeId),
    Land(LandType),
}
/// A specified proper name is never inferred from its spelling or suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenName {
    Specified(EnglishCardName),
    Default { ordered_subtypes: Vec<TokenSubtype> },
}
/// Immutable validated authored template. Its Program is nameless until birth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenDefinition {
    program: Program,
    name: TokenName,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Template admission or materialization failure, before any token is inserted.
pub enum TokenDefinitionError {
    Definition(DefinitionError),
    NotPermanent,
    TemplateHasNames,
    DuplicateSubtype,
    SubtypeCoverage,
    ForeignCreatureType,
    Name(NameDefinitionError),
}
impl TokenDefinition {
    /// Construct a literal template with one exact name source.
    ///
    /// # Errors
    /// Rejects invalid/nonpermanent Programs, pre-materialized names, duplicate
    /// or foreign subtype words and default lists not covering the Program sets.
    pub fn new(
        program: Program,
        name: TokenName,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, TokenDefinitionError> {
        let value = Self { program, name };
        value.validate(vocabulary)?;
        Ok(value)
    }
    /// The immutable nameless template, before the creation-time name is applied.
    pub fn program(&self) -> &Program {
        &self.program
    }
    /// The authored name source, retained through future text-changing effects.
    pub fn name(&self) -> &TokenName {
        &self.name
    }
    /// Validate against the enclosing game's actual creature vocabulary.
    ///
    /// # Errors
    /// Returns the same definition/name/subtype errors as `new`.
    pub fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), TokenDefinitionError> {
        self.program
            .validate(vocabulary)
            .map_err(TokenDefinitionError::Definition)?;
        if self.program.permanent().is_none() {
            return Err(TokenDefinitionError::NotPermanent);
        }
        if !self.program.names.is_empty() {
            return Err(TokenDefinitionError::TemplateHasNames);
        }
        if let TokenName::Default { ordered_subtypes } = &self.name {
            let mut supplied = BTreeSet::new();
            for word in ordered_subtypes {
                if let TokenSubtype::Creature(value) = word
                    && vocabulary.label(*value).is_none()
                {
                    return Err(TokenDefinitionError::ForeignCreatureType);
                }
                if !supplied.insert(*word) {
                    return Err(TokenDefinitionError::DuplicateSubtype);
                }
            }
            let expected = self
                .program
                .creature_subtypes
                .iter()
                .copied()
                .map(TokenSubtype::Creature)
                .chain(
                    self.program
                        .land_subtypes
                        .iter()
                        .copied()
                        .map(TokenSubtype::Land),
                )
                .collect();
            if supplied != expected {
                return Err(TokenDefinitionError::SubtypeCoverage);
            }
        }
        Ok(())
    }
    /// Produce one independently owned baseline with its final singular name.
    ///
    /// # Errors
    /// Rejects a foreign vocabulary or invalid template; invalid generated name
    /// representation is returned as `Name` rather than silently substituted.
    pub fn materialize(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Program, TokenDefinitionError> {
        self.validate(vocabulary)?;
        let name = match &self.name {
            TokenName::Specified(name) => name.clone(),
            TokenName::Default { ordered_subtypes } => {
                let mut words = ordered_subtypes
                    .iter()
                    .map(|word| match word {
                        TokenSubtype::Creature(value) => vocabulary
                            .label(*value)
                            .ok_or(TokenDefinitionError::ForeignCreatureType),
                        TokenSubtype::Land(value) => Ok(value.rules_label()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                words.push("Token");
                EnglishCardName::new(words.join(" ")).map_err(TokenDefinitionError::Name)?
            }
        };
        let mut program = self.program.clone();
        program.names = ObjectNames::single(name);
        Ok(program)
    }
    /// Rewrite literal creature-type words in the creating ability, retaining
    /// proper names and first-surviving default-name word order (CR 612.2a).
    /// This transforms the template, never a token that has already been born.
    pub(crate) fn rewrite_creature_types(&mut self, change: CreatureTypeTextChange) {
        crate::characteristics::rewrite_program_creature_types(change, &mut self.program);
        if let TokenName::Default { ordered_subtypes } = &mut self.name {
            for word in ordered_subtypes.iter_mut() {
                if *word == TokenSubtype::Creature(change.from) {
                    *word = TokenSubtype::Creature(change.to);
                }
            }
            let mut seen = BTreeSet::new();
            ordered_subtypes.retain(|word| seen.insert(*word));
        }
    }
}
