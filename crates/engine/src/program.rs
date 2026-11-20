//! Authored characteristics and executable capabilities under one definition owner.
//! Constructors assemble authored input. EffectWindow validates the complete
//! record before accepting it into the sole program map.
use crate::{
    activated::ActivatedAbility,
    characteristics::{
        CharacteristicDefinitionError, Color, PowerToughness, PowerToughnessCounters,
        PrintedPowerToughness, ResolvedDuration, ResolvedPartsSpec, StaticContinuousAbility,
    },
    land::LandType,
    life::{LifeAdjustment, LifeChangeKind},
    mana::{ManaAbility, ManaCost},
    turn::Step,
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::collections::BTreeSet;
mod control;
#[cfg(test)]
mod control_tests;
use control::FlowFacts;
pub use control::{Comparison, ConditionValue, ResolutionCondition};
#[cfg(test)]
mod cast_trigger_tests;
#[cfg(test)]
#[path = "program/characteristics_tests.rs"]
mod characteristics_tests;
#[cfg(test)]
mod color_definition_tests;
#[cfg(test)]
mod defining_types_pt_tests;
#[cfg(test)]
mod optional_payment_tests;
#[cfg(test)]
mod permanent_copy_tests;
#[cfg(test)]
mod resolution_selection_tests;
#[cfg(test)]
mod spell_role_lifetime_tests;

/// A body-local value name; admission checks its producer before each use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatureTypeSlot(u16);
impl CreatureTypeSlot {
    pub fn new(ordinal: u16) -> Self {
        Self(ordinal)
    }
    pub fn ordinal(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionAmount {
    Literal(RulesInteger),
    ControlledPermanentsOfChosenType {
        chosen: CreatureTypeSlot,
    },
    /// Number of actual matched occurrences captured by this triggered entry.
    MatchedOccurrences,
}
#[derive(Clone, Copy)]
enum BodyContext {
    Unbound,
    TriggerBound {
        condition: OrdinaryCondition,
        grouping: TriggerGrouping,
    },
}
impl InstructionAmount {
    fn validate(
        &self,
        defined_slots: &BTreeSet<CreatureTypeSlot>,
        context: BodyContext,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Literal(value) => {
                if *value < 0.into() {
                    Err(DefinitionError::InvalidInstruction)
                } else {
                    Ok(())
                }
            }
            Self::ControlledPermanentsOfChosenType { chosen } => {
                if defined_slots.contains(chosen) {
                    Ok(())
                } else {
                    Err(DefinitionError::UndefinedCreatureTypeSlot)
                }
            }
            Self::MatchedOccurrences => match context {
                BodyContext::Unbound => Err(DefinitionError::UnboundTriggerAmount),
                BodyContext::TriggerBound { .. } => Ok(()),
            },
        }
    }
}

/// A player derived from the actual resolving entry, never supplied by a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvingPlayer {
    /// The resolving entry's captured controller, including a triggered ability.
    Controller,
    /// The single actual participant in the entry's complete retained causes.
    /// Admission requires a participant-bearing trigger and singular grouping.
    UniqueRelatedPlayer,
    /// The exact bound current spell entry's controller, including spell copies.
    /// This consumes the body's required [`BodyTarget::SingleSpell`] target.
    /// Admission rejects reads after possible countering until controller LKI exists.
    TargetSpellController,
    /// The body's exact bound player, consuming [`BodyTarget::SinglePlayer`].
    /// A creature-or-player target does not guarantee this player capability.
    TargetPlayer,
}
impl ResolvingPlayer {
    fn validate(
        self,
        context: BodyContext,
        target: BodyTarget,
        target_spell_departed: bool,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        match self {
            Self::Controller => Ok(None),
            Self::UniqueRelatedPlayer => match context {
                BodyContext::Unbound => Err(DefinitionError::UnboundResolvingPlayer),
                BodyContext::TriggerBound {
                    condition,
                    grouping,
                } => {
                    let singular = match grouping {
                        TriggerGrouping::EachOccurrence | TriggerGrouping::OneOrMoreByPlayer => {
                            true
                        }
                        TriggerGrouping::OneOrMoreTogether => false,
                    };
                    if singular && condition.captures_related_player() {
                        Ok(None)
                    } else {
                        Err(DefinitionError::UnguaranteedRelatedPlayer)
                    }
                }
            },
            Self::TargetSpellController => match target {
                // CR 608.2h requires controller LKI after CR 701.6a removes
                // the spell. The runtime role currently reads a live entry only.
                BodyTarget::SingleSpell(_) if target_spell_departed => {
                    Err(DefinitionError::InvalidInstruction)
                }
                BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                BodyTarget::None
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SinglePlayer
                | BodyTarget::SingleCreatureOrPlayer => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::TargetPlayer => match target {
                BodyTarget::SinglePlayer => Ok(Some(OperandFamily::Player)),
                BodyTarget::None
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SingleSpell(_) => Err(DefinitionError::ResolvingPlayerTarget),
            },
        }
    }
}

/// Players instructed to perform one selected-object effect during resolution.
/// Runtime derives participants from the resolving entry and active-player order;
/// neither a decision reply nor a caller-authored seat list supplies this scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedPlayers {
    /// One player derived through the same native role as other resolving work.
    One(ResolvingPlayer),
    /// Every current participant, with choices made in actual APNAP order.
    EachPlayer,
}
impl AffectedPlayers {
    fn validate(
        self,
        context: BodyContext,
        target: BodyTarget,
        target_spell_departed: bool,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        match self {
            Self::One(player) => player.validate(context, target, target_spell_departed),
            Self::EachPlayer => Ok(None),
        }
    }
}

/// Source-independent current-type restrictions for an otherwise eligible permanent.
/// Empty sets impose no type restriction. Whole-body admission rejects an overlap
/// with [`DefinitionError::InvalidInstruction`]; physical zone/control eligibility
/// remains a separate runtime requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentTypeFilter {
    /// Every listed type must occur in the permanent's evaluated current types.
    pub required_types: BTreeSet<CardType>,
    /// None of these types may occur in its evaluated current types.
    pub forbidden_types: BTreeSet<CardType>,
}
impl PermanentTypeFilter {
    /// Match evaluated types only; callers separately require current physical
    /// Battlefield membership and the affected player's control.
    pub(crate) fn matches_types(&self, current: &BTreeSet<CardType>) -> bool {
        self.required_types.is_subset(current) && self.forbidden_types.is_disjoint(current)
    }
    fn validate(&self) -> Result<(), DefinitionError> {
        if self.required_types.is_disjoint(&self.forbidden_types) {
            Ok(())
        } else {
            Err(DefinitionError::InvalidInstruction)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// Look at and reorder a single recipient's top cards; no public view is granted.
    Scry {
        player: ResolvingPlayer,
        amount: InstructionAmount,
    },
    /// Choose top-card graveyard arrivals, then use physical-owner ordering.
    Surveil {
        player: ResolvingPlayer,
        amount: InstructionAmount,
    },
    /// Effect milling only, clamped to the actual library before conversion.
    Mill {
        player: ResolvingPlayer,
        amount: InstructionAmount,
    },
    /// Continue at the next instruction when true, otherwise at a strictly
    /// forward absolute destination. The body length denotes completion.
    BranchIfFalse {
        condition: ResolutionCondition,
        destination: usize,
    },
    /// Continue at a strictly forward absolute destination, including body end.
    Jump {
        destination: usize,
    },
    /// Offer Take at the next primitive or Decline at the instruction after it.
    /// Admission rejects a missing next instruction or another control wrapper.
    OptionalEffect {
        /// Player who chooses whether the following primitive is taken.
        chooser: ResolvingPlayer,
    },
    /// Offer an exact mana cost to the derived payer during resolution.
    /// Paid and Declined destinations are strictly forward and may equal body end.
    /// A zero cost still requires an explicit Pay acknowledgment at runtime.
    OfferManaPayment {
        /// Player permitted to pay the quoted cost or decline it.
        payer: ResolvingPlayer,
        /// Exact authored cost, validated by the shared mana-cost model.
        cost: ManaCost,
        /// Absolute instruction index selected only by a successful payment.
        paid_destination: usize,
        /// Absolute instruction index selected by an explicit decline.
        declined_destination: usize,
    },
    /// One simultaneous event containing every admitted counted literal group.
    /// Entry replacements and permanent-spell-copy conversion remain separate.
    CreateTokens {
        creations: Vec<crate::token::TokenCreation>,
    },
    /// Ordinary direct gain/loss/set for the resolving entry's controller. This
    /// is not damage or payment and does not consume the body's target.
    AdjustControllerLife {
        adjustment: LifeAdjustment,
    },
    /// Direct ordinary gain/loss/set for the body's selected player.
    AdjustTargetLife {
        adjustment: LifeAdjustment,
    },
    ChooseCreatureType {
        into: CreatureTypeSlot,
        excluding: BTreeSet<CreatureTypeId>,
    },
    Damage {
        amount: RulesInteger,
    },
    CreatePreventionShield {
        amount: RulesInteger,
    },
    CreateDamageMultiplier {
        factor: RulesInteger,
    },
    ReturnToOwnersHand,
    CounterSpell,
    CopySpell,
    /// Capture the donor's represented copiable definition for this activated
    /// ability's exact battlefield source. Entry/token/static copies and other
    /// copy exceptions are outside this instruction's admitted language.
    CopyTargetToSource {
        duration: ResolvedDuration,
        power_toughness: Option<PowerToughness>,
    },
    CreateContinuousEffect {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    PutPowerToughnessCounters {
        counters: PowerToughnessCounters,
    },
    /// Reads the resolving stack entry's controller, without consuming a target.
    DrawController {
        amount: InstructionAmount,
    },
    /// Each affected player chooses as many eligible controlled permanents as
    /// possible, then the complete selection is sacrificed simultaneously.
    /// This is an effect, not an additional cost or an optional-effect receipt.
    Sacrifice {
        /// Native resolving roles or all current participants.
        players: AffectedPlayers,
        /// Evaluated once under the resolving controller, then clamped per domain.
        amount: InstructionAmount,
        /// Evaluated types are distinct from physical Battlefield/control checks.
        filter: PermanentTypeFilter,
    },
    /// Each affected player chooses as many physical cards from their own hand
    /// as possible, then discards the complete selection simultaneously.
    /// Random discard and choosing for another player are separate mechanisms.
    Discard {
        /// Native resolving roles or all current participants.
        players: AffectedPlayers,
        /// Evaluated once under the resolving controller, then clamped per hand.
        amount: InstructionAmount,
    },
}
impl Instruction {
    /// Whether this operation can be the single effect of an optional wrapper.
    /// Admission and runtime availability share this exhaustive language boundary.
    pub(crate) fn is_optional_primitive(&self) -> bool {
        match self {
            Self::Scry { .. }
            | Self::Surveil { .. }
            | Self::Mill { .. }
            | Self::BranchIfFalse { .. }
            | Self::Jump { .. }
            | Self::OptionalEffect { .. }
            | Self::OfferManaPayment { .. }
            | Self::Sacrifice { .. }
            | Self::Discard { .. } => false,
            Self::CreateTokens { .. }
            | Self::AdjustControllerLife { .. }
            | Self::AdjustTargetLife { .. }
            | Self::ChooseCreatureType { .. }
            | Self::Damage { .. }
            | Self::CreatePreventionShield { .. }
            | Self::CreateDamageMultiplier { .. }
            | Self::ReturnToOwnersHand
            | Self::CounterSpell
            | Self::CopySpell
            | Self::CopyTargetToSource { .. }
            | Self::CreateContinuousEffect { .. }
            | Self::PutPowerToughnessCounters { .. }
            | Self::DrawController { .. } => true,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellTargetFilter {
    AnySpell,
    /// Current copy-interpreter capacity, not a separate Magic target type.
    CopyableInstructionSpell,
}
/// The body's admission gate is independent of its instruction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyTarget {
    None,
    SinglePermanent,
    SingleCreature,
    SinglePlayer,
    /// Deliberately excludes planeswalkers and battles until those are modeled.
    SingleCreatureOrPlayer,
    SingleSpell(SpellTargetFilter),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableBody {
    pub target: BodyTarget,
    pub instructions: Vec<Instruction>,
}
#[derive(Clone, Copy)]
enum OperandFamily {
    Permanent,
    Player,
    DamageRecipient,
    Spell,
}
impl ExecutableBody {
    pub fn new(target: BodyTarget, instructions: Vec<Instruction>) -> Self {
        Self {
            target,
            instructions,
        }
    }
    /// Check every scalar and operand against the explicit whole-body contract.
    ///
    /// # Errors
    /// Returns a definition error for invalid instructions, operands, resolving
    /// player contexts, costs, forward destinations or local-slot assignments.
    /// Trigger-dependent amounts and related players require a trigger binding.
    pub fn validate(&self, vocabulary: &CreatureTypeVocabulary) -> Result<(), DefinitionError> {
        self.validate_in(vocabulary, BodyContext::Unbound)
    }

    fn validate_in(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        context: BodyContext,
    ) -> Result<(), DefinitionError> {
        if self.instructions.is_empty() {
            return Err(DefinitionError::EmptyBody);
        }
        let mut consuming = false;
        let length = self.instructions.len();
        let points = length
            .checked_add(1)
            .ok_or(DefinitionError::InvalidControlFlow)?;
        let mut incoming = vec![None; points];
        incoming[0] = Some(FlowFacts::default());
        // Unreachable instructions still validate scalar/context/operand syntax.
        // Their local reads do not contribute facts or make a target reachable.
        let declared_slots: BTreeSet<_> = self
            .instructions
            .iter()
            .filter_map(|op| {
                if let Instruction::ChooseCreatureType { into, .. } = op {
                    Some(*into)
                } else {
                    None
                }
            })
            .collect();
        for (index, instruction) in self.instructions.iter().enumerate() {
            let reachable = incoming[index].is_some();
            let mut facts = incoming[index].take().unwrap_or_else(|| FlowFacts {
                definitely: declared_slots.clone(),
                ..FlowFacts::default()
            });
            let mut defined_slots = facts.definitely.clone();
            let mut donor_moved = facts.donor_moved;
            let mut target_spell_departed = facts.target_spell_departed;
            if let Instruction::ChooseCreatureType { into, .. } = instruction {
                if reachable && facts.possibly.contains(into) {
                    return Err(DefinitionError::DuplicateCreatureTypeSlot);
                }
                // Unreachable code has no incoming assignment history.
                if !reachable {
                    defined_slots.remove(into);
                }
            }
            let family = match instruction {
                Instruction::BranchIfFalse {
                    condition,
                    destination,
                } => {
                    condition.validate(context)?;
                    Self::validate_destination(index, *destination, length)?;
                    None
                }
                Instruction::Jump { destination } => {
                    Self::validate_destination(index, *destination, length)?;
                    None
                }
                Instruction::OptionalEffect { chooser } => {
                    let next = index
                        .checked_add(1)
                        .ok_or(DefinitionError::InvalidControlFlow)?;
                    let effect = self
                        .instructions
                        .get(next)
                        .ok_or(DefinitionError::MissingOptionalEffect)?;
                    if !effect.is_optional_primitive() {
                        return Err(DefinitionError::UnsupportedOptionalEffect);
                    }
                    let declined = index
                        .checked_add(2)
                        .ok_or(DefinitionError::InvalidControlFlow)?;
                    Self::validate_destination(index, declined, length)?;
                    chooser.validate(context, self.target, target_spell_departed)?
                }
                Instruction::OfferManaPayment {
                    payer,
                    cost,
                    paid_destination,
                    declined_destination,
                } => {
                    let family = payer.validate(context, self.target, target_spell_departed)?;
                    if !cost.valid() {
                        return Err(DefinitionError::InvalidResolutionPaymentCost);
                    }
                    Self::validate_destination(index, *paid_destination, length)?;
                    Self::validate_destination(index, *declined_destination, length)?;
                    family
                }
                Instruction::CreateTokens { creations } => {
                    if creations.is_empty() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    for creation in creations {
                        creation
                            .definition
                            .validate(vocabulary)
                            .map_err(|_| DefinitionError::TokenDefinition)?;
                        creation.amount.validate(&defined_slots, context)?;
                    }
                    None
                }
                Instruction::AdjustControllerLife { adjustment } => {
                    if !adjustment.valid() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AdjustTargetLife { adjustment } => {
                    if !adjustment.valid() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::ChooseCreatureType { into, excluding } => {
                    if excluding
                        .iter()
                        .any(|value| vocabulary.label(*value).is_none())
                    {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    if excluding.len() == vocabulary.len() {
                        return Err(DefinitionError::EmptyCreatureTypeDomain);
                    }
                    if !defined_slots.insert(*into) {
                        return Err(DefinitionError::DuplicateCreatureTypeSlot);
                    }
                    None
                }
                Instruction::Scry { player, amount }
                | Instruction::Surveil { player, amount }
                | Instruction::Mill { player, amount } => {
                    let family = player.validate(context, self.target, target_spell_departed)?;
                    amount.validate(&defined_slots, context)?;
                    family
                }
                Instruction::DrawController { amount } => {
                    amount.validate(&defined_slots, context)?;
                    None
                }
                Instruction::Sacrifice {
                    players,
                    amount,
                    filter,
                } => {
                    let family = players.validate(context, self.target, target_spell_departed)?;
                    amount.validate(&defined_slots, context)?;
                    filter.validate()?;
                    // A generic selection can move the later copy donor. Until
                    // copiable LKI exists, preserve the existing conservative
                    // CFG boundary. Exact zero adds no possible departure but
                    // must never clear an incoming true fact.
                    donor_moved |=
                        !matches!(amount, InstructionAmount::Literal(value) if *value == 0.into());
                    family
                }
                Instruction::Discard { players, amount } => {
                    let family = players.validate(context, self.target, target_spell_departed)?;
                    amount.validate(&defined_slots, context)?;
                    family
                }
                Instruction::CounterSpell => {
                    target_spell_departed = true;
                    Some(OperandFamily::Spell)
                }
                Instruction::CopySpell => Some(OperandFamily::Spell),
                Instruction::CopyTargetToSource { .. } => {
                    // CR 608.2h would require the departed donor's copiable LKI.
                    // Do not admit that composition until such capture exists.
                    if donor_moved {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::Damage { amount } => {
                    if !amount.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::CreatePreventionShield { amount } => {
                    if !amount.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::CreateDamageMultiplier { factor } => {
                    if !factor.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::PutPowerToughnessCounters { counters } => {
                    if !counters.valid_placement() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::CreateContinuousEffect { parts, .. } => {
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|error| match error {
                            CharacteristicDefinitionError::UndefinedCreatureTypeSlot => {
                                DefinitionError::UndefinedCreatureTypeSlot
                            }
                            _ => DefinitionError::ContinuousEffect,
                        })?;
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReturnToOwnersHand => {
                    donor_moved = true;
                    Some(OperandFamily::Permanent)
                }
            };
            if let Some(family) = family {
                consuming |= reachable;
                if !matches!(
                    (self.target, family),
                    (
                        BodyTarget::SingleCreature | BodyTarget::SinglePermanent,
                        OperandFamily::Permanent
                    ) | (BodyTarget::SingleSpell(_), OperandFamily::Spell)
                        | (BodyTarget::SinglePlayer, OperandFamily::Player)
                        | (
                            BodyTarget::SingleCreature
                                | BodyTarget::SinglePermanent
                                | BodyTarget::SinglePlayer
                                | BodyTarget::SingleCreatureOrPlayer,
                            OperandFamily::DamageRecipient
                        )
                ) {
                    return Err(DefinitionError::OperandFamily);
                }
            }
            if reachable {
                facts.definitely = defined_slots;
                facts.donor_moved = donor_moved;
                facts.target_spell_departed = target_spell_departed;
                if let Instruction::ChooseCreatureType { into, .. } = instruction {
                    facts.possibly.insert(*into);
                }
                let next = index
                    .checked_add(1)
                    .ok_or(DefinitionError::InvalidControlFlow)?;
                match instruction {
                    Instruction::Jump { destination } => {
                        facts.merge_into(&mut incoming[*destination])
                    }
                    Instruction::BranchIfFalse { destination, .. } => {
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[*destination]);
                    }
                    Instruction::OptionalEffect { .. } => {
                        let declined = index
                            .checked_add(2)
                            .ok_or(DefinitionError::InvalidControlFlow)?;
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[declined]);
                    }
                    Instruction::OfferManaPayment {
                        paid_destination,
                        declined_destination,
                        ..
                    } => {
                        facts.merge_into(&mut incoming[*paid_destination]);
                        facts.merge_into(&mut incoming[*declined_destination]);
                    }
                    Instruction::Scry { .. }
                    | Instruction::Surveil { .. }
                    | Instruction::Mill { .. }
                    | Instruction::CreateTokens { .. }
                    | Instruction::AdjustControllerLife { .. }
                    | Instruction::AdjustTargetLife { .. }
                    | Instruction::ChooseCreatureType { .. }
                    | Instruction::Damage { .. }
                    | Instruction::CreatePreventionShield { .. }
                    | Instruction::CreateDamageMultiplier { .. }
                    | Instruction::ReturnToOwnersHand
                    | Instruction::CounterSpell
                    | Instruction::CopySpell
                    | Instruction::CopyTargetToSource { .. }
                    | Instruction::CreateContinuousEffect { .. }
                    | Instruction::PutPowerToughnessCounters { .. }
                    | Instruction::DrawController { .. }
                    | Instruction::Sacrifice { .. }
                    | Instruction::Discard { .. } => facts.merge_into(&mut incoming[next]),
                }
            }
        }
        if self.target != BodyTarget::None && !consuming {
            return Err(DefinitionError::PhantomTarget);
        }
        Ok(())
    }
    fn validate_destination(
        from: usize,
        destination: usize,
        length: usize,
    ) -> Result<(), DefinitionError> {
        if destination <= from || destination > length {
            return Err(DefinitionError::InvalidControlFlow);
        }
        Ok(())
    }
}
/// Admitted card-type labels. Other real types and their mechanics remain open.
/// Executable shape and creature-family membership are independent properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CardType {
    Land,
    Artifact,
    Creature,
    Enchantment,
    Kindred,
    Instant,
    Sorcery,
}
impl CardType {
    /// Whether this admitted type is itself a permanent type (CR 110.4).
    pub const fn is_permanent(self) -> bool {
        matches!(
            self,
            Self::Land | Self::Artifact | Self::Creature | Self::Enchantment
        )
    }
    /// Creature and Kindred share one source-qualified subtype family.
    pub const fn carries_creature_subtypes(self) -> bool {
        matches!(self, Self::Creature | Self::Kindred)
    }
}
/// CR 205.4a's independent supertype axis. Carrying a value does not qualify
/// its associated rules: World/legend checks, Snow costs and schemes have their
/// own admission and execution requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Supertype {
    Basic,
    Legendary,
    Ongoing,
    Snow,
    World,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentDefinition {
    pub printed_pt: Option<PrintedPowerToughness>,
    pub casting_cost: Option<ManaCost>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellDefinition {
    pub cost: ManaCost,
    pub body: ExecutableBody,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramKind {
    Permanent(PermanentDefinition),
    Spell(SpellDefinition),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TriggeredAbilityId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRelation {
    SelfObject,
    AnyCreature,
    AnotherCreature,
    ControlledCreature,
    AnotherControlledCreature,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRelation {
    Controller,
    Opponent,
    AnyParticipant,
}
/// Represented predicates over a completed spell's evaluated card types.
/// These do not grant target legality or imply support for other cast filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellCastFilter {
    AnySpell,
    Noncreature,
    /// Membership in either family, including a Kindred instant or sorcery.
    InstantOrSorcery,
    HasType(CardType),
}
impl SpellCastFilter {
    pub(crate) fn matches(self, types: &BTreeSet<CardType>) -> bool {
        match self {
            Self::AnySpell => true,
            Self::Noncreature => !types.contains(&CardType::Creature),
            Self::InstantOrSorcery => {
                types.contains(&CardType::Instant) || types.contains(&CardType::Sorcery)
            }
            Self::HasType(kind) => types.contains(&kind),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdinaryCondition {
    /// This exact physical object becomes cast while on the stack (CR 113.6k).
    /// Casting a copy of a card remains a separate unimplemented mechanism.
    SelfCast,
    /// One completed positive scry/surveil action, even when no cards were available.
    Scried(PlayerRelation),
    Surveilled(PlayerRelation),
    /// A battlefield watcher observes an actual completed cast (CR 601.2i).
    SpellCast {
        player: PlayerRelation,
        filter: SpellCastFilter,
    },
    /// One occurrence for each token created by the related player.
    TokenCreated(PlayerRelation),
    LifeChanged {
        kind: LifeChangeKind,
        player: PlayerRelation,
    },
    Enters(ObjectRelation),
    LeavesBattlefield(ObjectRelation),
    Dies(ObjectRelation),
    BecomesTapped(ObjectRelation),
    Discarded(PlayerRelation),
    StepBegan {
        step: Step,
        player: PlayerRelation,
    },
}
impl OrdinaryCondition {
    fn captures_related_player(self) -> bool {
        // Keep this exhaustive: a future roleless predicate cannot inherit the
        // participant guarantee of today's occurrence publisher/matchers.
        match self {
            Self::SelfCast | Self::SpellCast { .. } | Self::Scried(_) | Self::Surveilled(_) => true,
            Self::TokenCreated(_) | Self::LifeChanged { .. } => true,
            Self::Enters(_)
            | Self::LeavesBattlefield(_)
            | Self::Dies(_)
            | Self::BecomesTapped(_) => true,
            Self::Discarded(_) | Self::StepBegan { .. } => true,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub id: TriggeredAbilityId,
    pub condition: OrdinaryCondition,
    /// Partition actual matches within one publication, source and definition.
    pub grouping: TriggerGrouping,
    pub body: ExecutableBody,
}
/// Logical trigger-event boundaries within one actual publication (CR 700.1).
/// History restrictions and additional-trigger modifiers are separate mechanisms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerGrouping {
    /// One instance for each matching occurrence.
    EachOccurrence,
    /// One instance owning every match in this publication.
    OneOrMoreTogether,
    /// One instance for each actual participant captured by the condition.
    OneOrMoreByPlayer,
}
/// Authored partial definition. Only validated definitions enter EffectWindow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Copiable battlefield replacement abilities; admitted action scope is explicit.
    pub zone_replacements: Vec<ZoneReplacementAbility>,
    /// Authored copiable types, independent of the executable body shape.
    pub card_types: BTreeSet<CardType>,
    /// The shared Creature/Kindred family; IDs belong to the game vocabulary.
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    /// Authored land family, independent of the Basic supertype and names.
    pub land_subtypes: BTreeSet<LandType>,
    pub names: spellwake_contracts::names::ObjectNames,
    /// Printed copiable supertypes, independent of kind, subtype and name.
    pub supertypes: BTreeSet<Supertype>,
    pub kind: ProgramKind,
    /// Printed color indicator, when present. Engine admission requires at least
    /// one color. Without an indicator ordinary colors come from the mana cost.
    pub color_indicator: Option<BTreeSet<Color>>,
    /// Colors defined by the creating effect (CR 111.3), independently of an
    /// indicator or mana cost. An empty set explicitly defines colorless.
    pub effect_defined_colors: Option<BTreeSet<Color>>,
    pub mana: Vec<ManaAbility>,
    pub activated: Vec<ActivatedAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub continuous: Option<StaticContinuousAbility>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionError {
    /// A replacement predicate requires a movement adapter outside this slice.
    UnsupportedZoneReplacementScope,
    /// A replacement has an empty action domain or no destination transformation.
    InvalidZoneReplacementDefinition,
    TokenDefinition,
    ColorIndicator,
    CardTypes,
    CastingCost,
    ManaSource,
    ActivatedSource,
    TriggeredSource,
    DuplicateAbility,
    EmptyBody,
    InvalidInstruction,
    /// A control successor is not a strictly forward point within its body.
    InvalidControlFlow,
    /// A condition requires a captured trigger but its body has no such binding.
    UnboundTriggerCondition,
    /// UniqueRelatedPlayer is used outside a captured triggered-ability body.
    UnboundResolvingPlayer,
    /// The trigger condition/grouping does not guarantee one related participant.
    UnguaranteedRelatedPlayer,
    /// A targeted resolving role lacks its exact whole-body target capability:
    /// TargetSpellController requires SingleSpell; TargetPlayer requires SinglePlayer.
    ResolvingPlayerTarget,
    /// An optional wrapper has no following primitive instruction.
    MissingOptionalEffect,
    /// An optional wrapper attempts an unsupported control or selected-object effect.
    UnsupportedOptionalEffect,
    /// A resolution payment has an invalid authored mana cost.
    InvalidResolutionPaymentCost,
    OperandFamily,
    PhantomTarget,
    TargetlessActivation,
    InvalidCreatureSubtypes,
    InvalidLandSubtypes,
    ForeignCreatureType,
    DuplicateCreatureTypeSlot,
    UndefinedCreatureTypeSlot,
    /// A body without a trigger binding uses a trigger-dependent amount.
    UnboundTriggerAmount,
    EmptyCreatureTypeDomain,
    ContinuousEffect,
}
mod zone_replacement;
pub use zone_replacement::*;

impl Program {
    /// Construct authored data with explicit traits; the owning engine must call
    /// validate before admission. Invalid definitions remain representable for
    /// typed rejection tests and syntax-only qualification boundaries.
    pub fn new(
        card_types: BTreeSet<CardType>,
        creature_subtypes: BTreeSet<CreatureTypeId>,
        land_subtypes: BTreeSet<LandType>,
        kind: ProgramKind,
    ) -> Self {
        Self {
            card_types,
            creature_subtypes,
            land_subtypes,
            names: spellwake_contracts::names::ObjectNames::nameless(),
            supertypes: BTreeSet::new(),
            kind,
            color_indicator: None,
            effect_defined_colors: None,
            mana: vec![],
            zone_replacements: vec![],
            activated: vec![],
            triggered: vec![],
            continuous: None,
        }
    }
    pub fn creature(
        casting_cost: Option<ManaCost>,
        power: RulesInteger,
        toughness: RulesInteger,
        creature_subtypes: BTreeSet<CreatureTypeId>,
    ) -> Self {
        Self::new(
            BTreeSet::from([CardType::Creature]),
            creature_subtypes,
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                printed_pt: Some(PowerToughness { power, toughness }.into()),
                casting_cost,
            }),
        )
    }
    pub fn artifact(casting_cost: Option<ManaCost>, activated: Vec<ActivatedAbility>) -> Self {
        Self {
            activated,
            ..Self::new(
                BTreeSet::from([CardType::Artifact]),
                BTreeSet::new(),
                BTreeSet::new(),
                ProgramKind::Permanent(PermanentDefinition {
                    printed_pt: None,
                    casting_cost,
                }),
            )
        }
    }
    /// Construct a non-Aura enchantment using the ordinary permanent body.
    pub fn enchantment(casting_cost: Option<ManaCost>) -> Self {
        Self::new(
            BTreeSet::from([CardType::Enchantment]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                printed_pt: None,
                casting_cost,
            }),
        )
    }
    /// Construct a land with explicit subtypes and any authored mana abilities.
    /// Intrinsic basic-land abilities are derived by the characteristic evaluator.
    pub fn land(land_subtypes: BTreeSet<LandType>, mana: Vec<ManaAbility>) -> Self {
        Self {
            mana,
            ..Self::new(
                BTreeSet::from([CardType::Land]),
                BTreeSet::new(),
                land_subtypes,
                ProgramKind::Permanent(PermanentDefinition {
                    printed_pt: None,
                    casting_cost: None,
                }),
            )
        }
    }
    pub fn instant(cost: ManaCost, body: ExecutableBody) -> Self {
        Self::new(
            BTreeSet::from([CardType::Instant]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition { cost, body }),
        )
    }
    pub fn sorcery(cost: ManaCost, body: ExecutableBody) -> Self {
        Self::new(
            BTreeSet::from([CardType::Sorcery]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition { cost, body }),
        )
    }
    pub fn permanent(&self) -> Option<&PermanentDefinition> {
        match &self.kind {
            ProgramKind::Permanent(value) => Some(value),
            _ => None,
        }
    }
    /// Defined single-face colors before continuous effects. Additional costs,
    /// activation costs and produced mana do not contribute colors (CR 202.2).
    /// Color-defining abilities are applied separately in layer 5. Hybrid/
    /// Phyrexian costs and multiple faces remain unimplemented.
    pub fn defined_colors(&self) -> BTreeSet<Color> {
        if let Some(colors) = &self.effect_defined_colors {
            return colors.clone();
        }
        if let Some(indicator) = &self.color_indicator {
            return indicator.clone();
        }
        let cost = match &self.kind {
            ProgramKind::Permanent(value) => value.casting_cost.as_ref(),
            ProgramKind::Spell(value) => Some(&value.cost),
        };
        cost.into_iter()
            .flat_map(|cost| cost.required.iter())
            .filter_map(|(kind, amount)| {
                amount
                    .is_positive()
                    .then(|| Color::from_mana(*kind))
                    .flatten()
            })
            .collect()
    }
    pub fn has_type(&self, kind: CardType) -> bool {
        self.card_types.contains(&kind)
    }
    /// Admit represented permanent types independently of their printed capabilities.
    pub fn validate(&self, vocabulary: &CreatureTypeVocabulary) -> Result<(), DefinitionError> {
        zone_replacement::validate(self)?;
        let permanent = self.card_types.iter().copied().any(CardType::is_permanent);
        let instruction = self.card_types.contains(&CardType::Instant)
            ^ self.card_types.contains(&CardType::Sorcery);
        if self.card_types.is_empty()
            || self.card_types == BTreeSet::from([CardType::Kindred])
            || match &self.kind {
                ProgramKind::Permanent(_) => {
                    !permanent
                        || self
                            .card_types
                            .iter()
                            .any(|ty| matches!(ty, CardType::Instant | CardType::Sorcery))
                }
                ProgramKind::Spell(_) => !instruction || permanent,
            }
        {
            return Err(DefinitionError::CardTypes);
        }
        if !self.has_type(CardType::Land) && !self.land_subtypes.is_empty() {
            return Err(DefinitionError::InvalidLandSubtypes);
        }
        if !self
            .card_types
            .iter()
            .copied()
            .any(CardType::carries_creature_subtypes)
            && !self.creature_subtypes.is_empty()
        {
            return Err(DefinitionError::InvalidCreatureSubtypes);
        }
        if self
            .creature_subtypes
            .iter()
            .any(|id| vocabulary.label(*id).is_none())
        {
            return Err(DefinitionError::ForeignCreatureType);
        }
        if self
            .color_indicator
            .as_ref()
            .is_some_and(BTreeSet::is_empty)
        {
            return Err(DefinitionError::ColorIndicator);
        }
        if let Some(ability) = &self.continuous {
            if matches!(ability, StaticContinuousAbility::Battlefield { .. })
                && !matches!(self.kind, ProgramKind::Permanent(_))
            {
                return Err(DefinitionError::ContinuousEffect);
            }
            ability
                .validate(vocabulary)
                .map_err(|_| DefinitionError::ContinuousEffect)?;
        }
        match &self.kind {
            ProgramKind::Permanent(value) => {
                if value
                    .casting_cost
                    .as_ref()
                    .is_some_and(|cost| !cost.valid())
                    || self.has_type(CardType::Land) && value.casting_cost.is_some()
                {
                    return Err(DefinitionError::CastingCost);
                }
                if self.mana.iter().any(|ability| !ability.valid(vocabulary)) {
                    return Err(DefinitionError::ManaSource);
                }
            }
            ProgramKind::Spell(spell) => {
                if !spell.cost.valid() {
                    return Err(DefinitionError::CastingCost);
                }
                spell.body.validate(vocabulary)?;
                if spell.body.instructions.iter().any(|instruction| {
                    matches!(instruction, Instruction::CopyTargetToSource { .. })
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if !self.mana.is_empty() {
                    return Err(DefinitionError::ManaSource);
                }
                if !self.activated.is_empty() {
                    return Err(DefinitionError::ActivatedSource);
                }
                if self
                    .triggered
                    .iter()
                    .any(|ability| ability.condition != OrdinaryCondition::SelfCast)
                {
                    return Err(DefinitionError::TriggeredSource);
                }
            }
        }
        if self
            .mana
            .iter()
            .map(|a| a.id)
            .collect::<BTreeSet<_>>()
            .len()
            != self.mana.len()
            || self
                .activated
                .iter()
                .map(|a| a.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.activated.len()
            || self
                .triggered
                .iter()
                .map(|a| a.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.triggered.len()
        {
            return Err(DefinitionError::DuplicateAbility);
        }
        for ability in &self.activated {
            if !ability.mana_cost.valid() {
                return Err(DefinitionError::CastingCost);
            }
            ability.body.validate(vocabulary)?;
            if ability.body.target == BodyTarget::None {
                return Err(DefinitionError::TargetlessActivation);
            }
        }
        for ability in &self.triggered {
            ability.body.validate_in(
                vocabulary,
                BodyContext::TriggerBound {
                    condition: ability.condition,
                    grouping: ability.grouping,
                },
            )?;
        }
        Ok(())
    }
}
