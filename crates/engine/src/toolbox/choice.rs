//! Universal `PromptChoice` engine, choice history, and `ChoiceMade` event metadata.
//!
//! Replaces bespoke `*ChoiceFrame` structs with one generic interactive choice
//! packet supporting objects, players, options, numbers, colors, card types,
//! subtypes, card names, and N-pile partitions. Every resolved choice records
//! history and emits a generic `ChoiceMade` occurrence.

use crate::{
    effects::EffectError,
    land::LandType,
    mana::ManaType,
    object_identity::ObjectId,
    priority::SeatId,
    program::{CardType, ColorMask, DefinitionError, PlaneswalkerSubtype, ValueExpr},
    stack_window::StackEntryId,
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
        tags::TagKey,
        zone_op::SelectionConstraint,
    },
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::collections::BTreeSet;

/// Semantic category of a [`PromptChoiceSpec`], carried into choice history and
/// `RulesOccurrence::ChoiceMade` so triggers like Doctor Who villainous choices,
/// Star Trek dilemmas, votes, and modal watchers reuse one event pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceCategory {
    Generic,
    Modal,
    VillainousChoice,
    Dilemma,
    Vote,
    PileSplit,
    Search,
    Tribute,
    Clash,
    Custom(u16),
}

/// Which subtype family is offered by [`PromptDomain::Subtype`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtypeChoiceKind {
    Creature,
    BasicLand,
    Planeswalker,
}

/// Program-time domain specification for `Instruction::PromptChoice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptDomain {
    /// Choose `min..=max` objects from `query`.
    Objects {
        query: ObjectQuery,
        min: Box<ValueExpr>,
        max: Box<ValueExpr>,
        constraint: Option<SelectionConstraint>,
        hidden: bool,
    },
    /// Choose `min..=max` players from `selector`.
    Players {
        selector: PlayerSelector,
        min: u8,
        max: u8,
    },
    /// Choose `min..=max` labeled options.
    Options {
        labels: Vec<String>,
        min: u8,
        max: u8,
        allow_repeat: bool,
    },
    /// Choose an integer in `min..=max`.
    Number {
        min: Box<ValueExpr>,
        max: Box<ValueExpr>,
    },
    /// Choose one color from `allowed` (or all five WUBRG colors when empty).
    Color { allowed: ColorMask },
    /// Choose one card type from `allowed` (or the 7 standard types when empty).
    CardType { allowed: Vec<CardType> },
    /// Choose a subtype from a typed family.
    Subtype { kind: SubtypeChoiceKind },
    /// Choose a card name (optionally nonland only).
    CardName { nonland_only: bool },
    /// Partition all objects from `query` into `pile_count` disjoint piles (e.g. Fact or Fiction,
    /// Raging River, Space Beleren).
    Partition {
        query: ObjectQuery,
        pile_count: u8,
        min_per_pile: u8,
    },
}

/// Complete program specification for `Instruction::PromptChoice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoiceSpec {
    pub chooser: PlayerSelector,
    pub domain: PromptDomain,
    pub category: ChoiceCategory,
    /// Primary binding slot receiving the chosen value in [`crate::toolbox::ExecEnv`].
    pub bind_to: BindingId,
    /// For `Objects` domains, optional slot receiving unchosen candidates.
    pub bind_unchosen: Option<BindingId>,
    /// For `Partition` domains, optional tag key tagging every object with its assigned pile index.
    pub tag_partition: Option<TagKey>,
    /// When true, skip prompting if there is at most one legal answer (e.g. 0 candidates with min 0).
    pub auto_resolve_trivial: bool,
}

impl PromptChoiceSpec {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        if self.chooser.uses_target() {
            return true;
        }
        match &self.domain {
            PromptDomain::Objects {
                query, min, max, ..
            } => query.uses_target() || min.uses_target() || max.uses_target(),
            PromptDomain::Players { selector, .. } => selector.uses_target(),
            PromptDomain::Number { min, max } => min.uses_target() || max.uses_target(),
            PromptDomain::Partition { query, .. } => query.uses_target(),
            PromptDomain::Options { .. }
            | PromptDomain::Color { .. }
            | PromptDomain::CardType { .. }
            | PromptDomain::Subtype { .. }
            | PromptDomain::CardName { .. } => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.chooser.validate_vocabulary(vocabulary)?;
        match &self.domain {
            PromptDomain::Objects {
                query, min, max, ..
            } => {
                query.validate_vocabulary(vocabulary)?;
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
            PromptDomain::Players { selector, min, max } => {
                if min > max {
                    return Err(DefinitionError::InvalidInstruction);
                }
                selector.validate_vocabulary(vocabulary)?;
            }
            PromptDomain::Options {
                labels, min, max, ..
            } => {
                if labels.is_empty() || min > max {
                    return Err(DefinitionError::InvalidInstruction);
                }
            }
            PromptDomain::Number { min, max } => {
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
            PromptDomain::Partition {
                query, pile_count, ..
            } => {
                if *pile_count < 2 {
                    return Err(DefinitionError::InvalidInstruction);
                }
                query.validate_vocabulary(vocabulary)?;
            }
            PromptDomain::Color { .. }
            | PromptDomain::CardType { .. }
            | PromptDomain::Subtype { .. }
            | PromptDomain::CardName { .. } => {}
        }
        Ok(())
    }
}

/// Executor-frozen legal domain issued to the wire for a single [`PromptChoiceFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPromptDomain {
    Objects {
        candidates: Vec<ObjectId>,
        min: usize,
        max: usize,
        hidden: bool,
    },
    Players {
        candidates: Vec<SeatId>,
        min: u8,
        max: u8,
    },
    Options {
        labels: Vec<String>,
        min: u8,
        max: u8,
        allow_repeat: bool,
    },
    Number {
        min: RulesInteger,
        max: RulesInteger,
    },
    Color {
        allowed: BTreeSet<ManaType>,
    },
    CardType {
        allowed: Vec<CardType>,
    },
    CreatureType {
        allowed: Vec<CreatureTypeId>,
    },
    BasicLandType {
        allowed: Vec<LandType>,
    },
    PlaneswalkerType {
        allowed: Vec<PlaneswalkerSubtype>,
    },
    CardName {
        nonland_only: bool,
    },
    Partition {
        candidates: Vec<ObjectId>,
        pile_count: u8,
        min_per_pile: u8,
    },
}

/// Single generic immutable choice frame emitted by the universal toolbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoiceFrame {
    pub revision: u64,
    pub entry: StackEntryId,
    pub instruction: usize,
    pub chooser: SeatId,
    pub category: ChoiceCategory,
    pub domain: ResolvedPromptDomain,
}

impl PromptChoiceFrame {
    /// Authorized seat that must answer this frame.
    #[must_use]
    pub const fn chooser(&self) -> SeatId {
        self.chooser
    }
}

/// Single generic player reply paired with [`PromptChoiceFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptChoiceReply {
    Objects(Vec<ObjectId>),
    Players(Vec<SeatId>),
    Options(Vec<u8>),
    Number(RulesInteger),
    Color(ManaType),
    CardType(CardType),
    CreatureType(CreatureTypeId),
    BasicLandType(LandType),
    PlaneswalkerType(PlaneswalkerSubtype),
    CardName(String),
    Partition(Vec<Vec<ObjectId>>),
}

/// Immutable history record appended whenever a [`PromptChoiceSpec`] resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRecord {
    pub turn: u64,
    pub chooser: SeatId,
    pub source: Option<ObjectId>,
    pub category: ChoiceCategory,
    pub option_indices: Vec<u8>,
    pub chosen_count: usize,
}

/// Atomically validate `reply` against `frame.domain` before mutating any engine state.
///
/// # Errors
/// Returns [`EffectError::WrongDecisionKind`] when the reply variant does not match the domain,
/// or [`EffectError::InvalidSelectionCount`] / [`EffectError::DuplicateSelection`] / [`EffectError::SelectionOutsideDomain`] when counts, uniqueness, membership, or partition
/// completeness invariants are violated.
pub fn validate_prompt_reply(
    frame: &PromptChoiceFrame,
    reply: &PromptChoiceReply,
) -> Result<(), EffectError> {
    match (&frame.domain, reply) {
        (
            ResolvedPromptDomain::Objects {
                candidates,
                min,
                max,
                ..
            },
            PromptChoiceReply::Objects(chosen),
        ) => {
            if chosen.len() < *min || chosen.len() > *max {
                return Err(EffectError::InvalidSelectionCount);
            }
            let allowed: BTreeSet<ObjectId> = candidates.iter().copied().collect();
            let mut seen = BTreeSet::new();
            for id in chosen {
                if !allowed.contains(id) {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !seen.insert(*id) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Players {
                candidates,
                min,
                max,
            },
            PromptChoiceReply::Players(chosen),
        ) => {
            if chosen.len() < usize::from(*min) || chosen.len() > usize::from(*max) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let allowed: BTreeSet<SeatId> = candidates.iter().copied().collect();
            let mut seen = BTreeSet::new();
            for seat in chosen {
                if !allowed.contains(seat) {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !seen.insert(*seat) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Options {
                labels,
                min,
                max,
                allow_repeat,
            },
            PromptChoiceReply::Options(chosen),
        ) => {
            if chosen.len() < usize::from(*min) || chosen.len() > usize::from(*max) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let mut seen = BTreeSet::new();
            for &idx in chosen {
                if usize::from(idx) >= labels.len() {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !*allow_repeat && !seen.insert(idx) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (ResolvedPromptDomain::Number { min, max }, PromptChoiceReply::Number(val)) => {
            if val < min || val > max {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::Color { allowed }, PromptChoiceReply::Color(color)) => {
            if !allowed.contains(color) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CardType { allowed }, PromptChoiceReply::CardType(ty)) => {
            if !allowed.contains(ty) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CreatureType { allowed }, PromptChoiceReply::CreatureType(sub)) => {
            if !allowed.contains(sub) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::BasicLandType { allowed }, PromptChoiceReply::BasicLandType(lt)) => {
            if !allowed.contains(lt) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::PlaneswalkerType { allowed },
            PromptChoiceReply::PlaneswalkerType(pw),
        ) => {
            if !allowed.contains(pw) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CardName { .. }, PromptChoiceReply::CardName(name)) => {
            if name.trim().is_empty() {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Partition {
                candidates,
                pile_count,
                min_per_pile,
            },
            PromptChoiceReply::Partition(piles),
        ) => {
            if piles.len() != usize::from(*pile_count) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let expected: BTreeSet<ObjectId> = candidates.iter().copied().collect();
            let mut assigned = BTreeSet::new();
            for pile in piles {
                if pile.len() < usize::from(*min_per_pile) {
                    return Err(EffectError::InvalidSelectionCount);
                }
                for id in pile {
                    if !expected.contains(id) {
                        return Err(EffectError::SelectionOutsideDomain);
                    }
                    if !assigned.insert(*id) {
                        return Err(EffectError::DuplicateSelection);
                    }
                }
            }
            if assigned.len() != expected.len() {
                return Err(EffectError::InvalidSelectionCount);
            }
            Ok(())
        }
        _ => Err(EffectError::WrongDecisionKind),
    }
}
