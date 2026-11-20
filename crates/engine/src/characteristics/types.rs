use super::{CharacteristicState, Color, ColorFilter, ColorOperation, StaticColorOperand};
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId},
    land::{BasicLandType, LandType},
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef},
    object_identity::{CardId, GameObjects, ObjectId, ObjectZone},
    priority::SeatId,
    program::{
        CardType, CreatureTypeSlot, Program, Supertype, TriggeredAbility, TriggeredAbilityId,
    },
    stack_window::StackEntryId,
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::{EnglishCardName, ObjectNames},
};
use std::collections::{BTreeMap, BTreeSet};

/// Exact numeric pair; negative values are never clamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughness {
    pub power: RulesInteger,
    pub toughness: RulesInteger,
}
impl PowerToughness {
    pub fn zero() -> Self {
        Self {
            power: 0.into(),
            toughness: 0.into(),
        }
    }
}
/// Printed, copiable stat syntax, distinct from an evaluated rules integer.
/// Affine stars represent *, 1+*, *+1, 2+* and 7-* without raw expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintedStat {
    Number(RulesInteger),
    Star {
        multiplier: RulesInteger,
        offset: RulesInteger,
    },
}
impl PrintedStat {
    pub(crate) fn baseline(&self) -> RulesInteger {
        match self {
            Self::Number(value) => value.clone(),
            Self::Star { offset, .. } => offset.clone(),
        }
    }
}
impl From<RulesInteger> for PrintedStat {
    fn from(value: RulesInteger) -> Self {
        Self::Number(value)
    }
}
/// One printed box. Star formulas remain copiable when their defining ability
/// is removed or when the calculated value changes (CR 208.2a, 707.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintedPowerToughness {
    pub power: PrintedStat,
    pub toughness: PrintedStat,
}
impl PrintedPowerToughness {
    pub(crate) fn baseline(&self) -> PowerToughness {
        PowerToughness {
            power: self.power.baseline(),
            toughness: self.toughness.baseline(),
        }
    }
}
impl From<PowerToughness> for PrintedPowerToughness {
    fn from(value: PowerToughness) -> Self {
        Self {
            power: PrintedStat::Number(value.power),
            toughness: PrintedStat::Number(value.toughness),
        }
    }
}
/// Physical same-incarnation counts, independent of current Creature membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughnessCounters {
    pub plus_one: RulesInteger,
    pub minus_one: RulesInteger,
}
impl Default for PowerToughnessCounters {
    fn default() -> Self {
        Self {
            plus_one: 0.into(),
            minus_one: 0.into(),
        }
    }
}
impl PowerToughnessCounters {
    pub(crate) fn valid_placement(&self) -> bool {
        self.plus_one >= 0.into()
            && self.minus_one >= 0.into()
            && (self.plus_one.is_positive() || self.minus_one.is_positive())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContinuousEffectId(pub(crate) u64);
impl ContinuousEffectId {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
/// Ordering identity shared by BF objects and resolved effects, not an effect ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CharacteristicTimestamp(pub(crate) u64);
impl CharacteristicTimestamp {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StaticAbilityId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDuration {
    NoStatedDuration,
    UntilEndOfTurn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CharacteristicBucket {
    Copy,
    Control,
    Text,
    Type,
    Color,
    Abilities,
    DefinePT,
    SetPT,
    ModifyPT,
    SwitchPT,
}
impl CharacteristicBucket {
    pub const ALL: [Self; 10] = [
        Self::Copy,
        Self::Control,
        Self::Text,
        Self::Type,
        Self::Color,
        Self::Abilities,
        Self::DefinePT,
        Self::SetPT,
        Self::ModifyPT,
        Self::SwitchPT,
    ];
}
/// A resolution-bound player operand, materialized once rather than reread from
/// a source whose controller may subsequently change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRecipient {
    ResolvingController,
    TargetOwner,
}
/// One semantic creature-type word in printed text, not a displayed string or a
/// set of subtypes. A chosen value is read once when the effect is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatureTypeWord {
    Literal(CreatureTypeId),
    Chosen(CreatureTypeSlot),
}
/// Layer-3 substitution of a creature-type word wherever it is used as that
/// type in the admitted printed type line and ability definitions (CR 612.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureTypeTextChange {
    pub from: CreatureTypeId,
    pub to: CreatureTypeId,
}
/// Resolution-authored text operands, validated against the body's local scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureTypeTextChangeSpec {
    pub from: CreatureTypeWord,
    pub to: CreatureTypeWord,
}
/// Layer 6 removal of the admitted printed ability families (CR 613.1f).
/// This removes abilities, not independent spell/ability entries on the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityRemoval {
    All,
    NonMana,
}
/// Implemented static keywords. Unsupported keywords cannot enter this enum as
/// arbitrary text or silently acquire partial behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeywordAbility {
    /// Bypass the continuous-control restriction for attacking and tap symbols.
    Haste,
    /// Declaring an attack does not tap this creature.
    Vigilance,
}
/// One layer-6 component: remove first, then grant the specified keywords. Both
/// operations share this effect's timestamp, duration and affected set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbilityChange {
    /// Optional removal of the admitted ability families, including keywords.
    pub removal: Option<AbilityRemoval>,
    /// Redundant instances of these keywords do not stack (CR 702.10d/702.20c).
    pub granted_keywords: BTreeSet<KeywordAbility>,
}
impl AbilityChange {
    /// Construct removal with no subsequent keyword grant.
    pub fn remove(removal: AbilityRemoval) -> Self {
        Self {
            removal: Some(removal),
            granted_keywords: BTreeSet::new(),
        }
    }
    /// Construct a keyword grant with no removal.
    pub fn grant_keywords(keywords: impl IntoIterator<Item = KeywordAbility>) -> Self {
        Self {
            removal: None,
            granted_keywords: keywords.into_iter().collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeOperation<T = CardType> {
    Keep,
    Add(BTreeSet<T>),
    Remove(BTreeSet<T>),
    Set(BTreeSet<T>),
}
/// Add/Set accept the authored operand language of their owning part. Removal
/// always names literal members of the owning subtype family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtypeOperation<T, Member = CreatureTypeId> {
    Keep,
    Add(T),
    Remove(BTreeSet<Member>),
    Set(T),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeChange {
    pub land_subtypes: SubtypeOperation<BTreeSet<LandType>, LandType>,
    pub supertypes: TypeOperation<Supertype>,
    pub types: TypeOperation,
    pub subtypes: SubtypeOperation<BTreeSet<CreatureTypeId>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionSubtypeOperand {
    Literal(BTreeSet<CreatureTypeId>),
    Chosen(CreatureTypeSlot),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeChangeSpec {
    pub land_subtypes: SubtypeOperation<BTreeSet<LandType>, LandType>,
    pub supertypes: TypeOperation<Supertype>,
    pub types: TypeOperation,
    pub subtypes: SubtypeOperation<InstructionSubtypeOperand>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSubtypeOperand {
    Literal(BTreeSet<CreatureTypeId>),
    SourceCurrentSubtypes,
}
/// A land operand regenerated from the current scratch world in layer 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticLandSubtypeOperand {
    Literal(BTreeSet<LandType>),
    SourceCurrentLandSubtypes,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticTypeChange {
    pub land_subtypes: SubtypeOperation<StaticLandSubtypeOperand, LandType>,
    pub supertypes: TypeOperation<Supertype>,
    pub types: TypeOperation,
    pub subtypes: SubtypeOperation<StaticSubtypeOperand>,
}
/// One layer-3 name operation. Set may make an object nameless; Add retains
/// existing names. Neither operation changes the copiable definition (CR 612.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameOperation<T> {
    Set(T),
    Add(T),
}
/// A live name operand evaluated within the exact dependency probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticNameOperand {
    Literal(ObjectNames),
    SourceCurrentNames,
}
/// Typed name predicates, never creature-type words or source-self references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NameFilter {
    #[default]
    Any,
    Named(EnglishCardName),
    SharesWithSource,
    /// The candidate must be named and disjoint from the source (CR 201.2c).
    DifferentFromSource,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPart {
    /// Captured at resolution, never a live donor lookup. Only the admitted
    /// partial Program language is represented; this is not full card metadata.
    Copy(Box<Program>),
    Control(SeatId),
    Text(CreatureTypeTextChange),
    Names(NameOperation<ObjectNames>),
    Type(TypeChange),
    Color(ColorOperation<BTreeSet<Color>>),
    Abilities(AbilityChange),
    SetPT(PowerToughness),
    ModifyPT(PowerToughness),
    SwitchPT,
}
impl ResolvedPart {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::Copy(_) => CharacteristicBucket::Copy,
            Self::Control(_) => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::Abilities(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) => CharacteristicBucket::SetPT,
            Self::ModifyPT(_) => CharacteristicBucket::ModifyPT,
            Self::SwitchPT => CharacteristicBucket::SwitchPT,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPartSpec {
    Control(ControlRecipient),
    Text(CreatureTypeTextChangeSpec),
    Names(NameOperation<ObjectNames>),
    Type(TypeChangeSpec),
    Color(ColorOperation<BTreeSet<Color>>),
    Abilities(AbilityChange),
    SetPT(PowerToughness),
    ModifyPT(PowerToughness),
    SwitchPT,
}
impl ResolvedPartSpec {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::Control(_) => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::Abilities(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) => CharacteristicBucket::SetPT,
            Self::ModifyPT(_) => CharacteristicBucket::ModifyPT,
            Self::SwitchPT => CharacteristicBucket::SwitchPT,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedParts(pub Vec<ResolvedPart>);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPartsSpec(pub Vec<ResolvedPartSpec>);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerSelector {
    Any,
    SourceController,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentSelector {
    pub required_supertypes: BTreeSet<Supertype>,
    pub forbidden_supertypes: BTreeSet<Supertype>,
    pub required_types: BTreeSet<CardType>,
    pub forbidden_types: BTreeSet<CardType>,
    pub creature_subtype: Option<CreatureTypeId>,
    pub land_subtype: Option<LandType>,
    pub color: ColorFilter,
    pub name: NameFilter,
    pub controller: ControllerSelector,
    pub exclude_source: bool,
}
impl Default for PermanentSelector {
    fn default() -> Self {
        Self {
            required_supertypes: BTreeSet::new(),
            forbidden_supertypes: BTreeSet::new(),
            required_types: BTreeSet::new(),
            forbidden_types: BTreeSet::new(),
            creature_subtype: None,
            land_subtype: None,
            color: ColorFilter::Any,
            name: NameFilter::Any,
            controller: ControllerSelector::Any,
            exclude_source: false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumericExpression {
    Literal(RulesInteger),
    Count {
        selector: PermanentSelector,
        multiplier: RulesInteger,
        offset: RulesInteger,
    },
}
impl NumericExpression {
    /// Author an unadjusted exact permanent count; wire operands remain explicit.
    pub fn count(selector: PermanentSelector) -> Self {
        Self::Count {
            selector,
            multiplier: 1.into(),
            offset: 0.into(),
        }
    }
    pub(crate) fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), CharacteristicDefinitionError> {
        match self {
            Self::Literal(_) => Ok(()),
            Self::Count { selector, .. } => selector.validate(vocabulary),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumericPair {
    pub power: NumericExpression,
    pub toughness: NumericExpression,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticPart {
    /// Set the affected permanents' controller to the source's current controller.
    ControlSourceController,
    Text(CreatureTypeTextChange),
    Names(NameOperation<StaticNameOperand>),
    Type(StaticTypeChange),
    Color(ColorOperation<StaticColorOperand>),
    Abilities(AbilityChange),
    SetPT(NumericPair),
    ModifyPT(NumericPair),
    SwitchPT,
}
impl StaticPart {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::ControlSourceController => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::Abilities(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) => CharacteristicBucket::SetPT,
            Self::ModifyPT(_) => CharacteristicBucket::ModifyPT,
            Self::SwitchPT => CharacteristicBucket::SwitchPT,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticParts(pub Vec<StaticPart>);
/// Creature subtype definitions use the exact game vocabulary for All.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefiningCreatureTypes {
    Literal(BTreeSet<CreatureTypeId>),
    All,
}
/// Unconditional self-only defining operations (CR 604.3a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefiningPart {
    CreatureTypes(DefiningCreatureTypes),
    Colors(BTreeSet<Color>),
    /// Box present expressions to keep non-PT variants compact. This is a
    /// representation requirement from Clippy, not a measured speedup.
    PowerToughness {
        power: Option<Box<NumericExpression>>,
        toughness: Option<Box<NumericExpression>>,
    },
}
impl DefiningPart {
    /// Evaluation bucket of this unconditional self-only component.
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::CreatureTypes(_) => CharacteristicBucket::Type,
            Self::Colors(_) => CharacteristicBucket::Color,
            Self::PowerToughness { .. } => CharacteristicBucket::DefinePT,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Authored defining parts; admission requires nonempty, unique buckets.
pub struct DefiningParts(pub Vec<DefiningPart>);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticContinuousAbility {
    /// Ordinary continuous effects generated by a battlefield source.
    Battlefield {
        id: StaticAbilityId,
        selector: PermanentSelector,
        parts: StaticParts,
    },
    /// Printed self-only definitions in every represented physical zone.
    /// Earlier-layer parts continue after ability removal under CR 613.6.
    CharacteristicDefining {
        id: StaticAbilityId,
        parts: DefiningParts,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacteristicDefinitionError {
    UnsupportedTypeOperation,
    MixedCopyParts,
    EmptyParts,
    DuplicateBucket,
    ForeignCreatureType,
    UndefinedCreatureTypeSlot,
    ContradictorySelector,
    EmptyPowerToughnessDefinition,
}
pub(super) fn validate_buckets(
    buckets: impl Iterator<Item = CharacteristicBucket>,
) -> Result<(), CharacteristicDefinitionError> {
    let mut found = BTreeSet::new();
    for bucket in buckets {
        if !found.insert(bucket) {
            return Err(CharacteristicDefinitionError::DuplicateBucket);
        }
    }
    if found.is_empty() {
        return Err(CharacteristicDefinitionError::EmptyParts);
    }
    Ok(())
}
fn validate_subtypes(
    subtypes: &BTreeSet<CreatureTypeId>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), CharacteristicDefinitionError> {
    if subtypes
        .iter()
        .any(|value| vocabulary.label(*value).is_none())
    {
        return Err(CharacteristicDefinitionError::ForeignCreatureType);
    }
    Ok(())
}
impl PermanentSelector {
    pub fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), CharacteristicDefinitionError> {
        if !self.required_types.is_disjoint(&self.forbidden_types)
            || !self
                .required_supertypes
                .is_disjoint(&self.forbidden_supertypes)
        {
            return Err(CharacteristicDefinitionError::ContradictorySelector);
        }
        if self
            .creature_subtype
            .is_some_and(|value| vocabulary.label(value).is_none())
        {
            return Err(CharacteristicDefinitionError::ForeignCreatureType);
        }
        Ok(())
    }
}
/// Finite type-changing effects operate on battlefield objects only.
fn validate_battlefield_types(
    operation: &TypeOperation,
) -> Result<(), CharacteristicDefinitionError> {
    let values = match operation {
        TypeOperation::Keep => return Ok(()),
        TypeOperation::Add(values) | TypeOperation::Remove(values) | TypeOperation::Set(values) => {
            values
        }
    };
    if values
        .iter()
        .any(|value| matches!(value, CardType::Instant | CardType::Sorcery))
    {
        return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
    }
    Ok(())
}
impl ResolvedParts {
    pub fn validate(&self) -> Result<(), CharacteristicDefinitionError> {
        for part in &self.0 {
            if let ResolvedPart::Type(change) = part {
                validate_battlefield_types(&change.types)?;
            }
        }
        if self.0.len() != 1
            && self
                .0
                .iter()
                .any(|part| matches!(part, ResolvedPart::Copy(_)))
        {
            return Err(CharacteristicDefinitionError::MixedCopyParts);
        }
        validate_buckets(self.0.iter().map(ResolvedPart::bucket))
    }
}
impl ResolvedPartsSpec {
    pub fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        defined_slots: &BTreeSet<CreatureTypeSlot>,
    ) -> Result<(), CharacteristicDefinitionError> {
        validate_buckets(self.0.iter().map(ResolvedPartSpec::bucket))?;
        for part in &self.0 {
            if let ResolvedPartSpec::Text(change) = part {
                for word in [change.from, change.to] {
                    match word {
                        CreatureTypeWord::Literal(value) if vocabulary.label(value).is_none() => {
                            return Err(CharacteristicDefinitionError::ForeignCreatureType);
                        }
                        CreatureTypeWord::Chosen(slot) if !defined_slots.contains(&slot) => {
                            return Err(CharacteristicDefinitionError::UndefinedCreatureTypeSlot);
                        }
                        CreatureTypeWord::Literal(_) | CreatureTypeWord::Chosen(_) => {}
                    }
                }
            }
            if let ResolvedPartSpec::Type(change) = part {
                validate_battlefield_types(&change.types)?;
                match &change.subtypes {
                    SubtypeOperation::Keep => {}
                    SubtypeOperation::Remove(values)
                    | SubtypeOperation::Add(InstructionSubtypeOperand::Literal(values))
                    | SubtypeOperation::Set(InstructionSubtypeOperand::Literal(values)) => {
                        validate_subtypes(values, vocabulary)?
                    }
                    SubtypeOperation::Add(InstructionSubtypeOperand::Chosen(slot))
                    | SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(slot)) => {
                        if !defined_slots.contains(slot) {
                            return Err(CharacteristicDefinitionError::UndefinedCreatureTypeSlot);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    /// Materialize cursor locals into owned literal data before allocating a
    /// resolved record. Bind player operands to the actual resolving controller
    /// and target owner. No cursor reference survives this call.
    ///
    /// # Errors
    /// Returns `MissingChosenCreatureType` when a required local was not bound.
    /// The enclosing executor validates participant membership and definitions.
    pub fn materialize(
        &self,
        values: &BTreeMap<CreatureTypeSlot, CreatureTypeId>,
        resolving_controller: SeatId,
        target_owner: SeatId,
    ) -> Result<ResolvedParts, CharacteristicInvariantError> {
        self.0
            .iter()
            .map(|part| {
                Ok(match part {
                    ResolvedPartSpec::Names(change) => ResolvedPart::Names(change.clone()),
                    ResolvedPartSpec::Color(change) => ResolvedPart::Color(change.clone()),
                    ResolvedPartSpec::Text(change) => ResolvedPart::Text(CreatureTypeTextChange {
                        from: materialize_word(change.from, values)?,
                        to: materialize_word(change.to, values)?,
                    }),
                    ResolvedPartSpec::Control(recipient) => {
                        ResolvedPart::Control(match recipient {
                            ControlRecipient::ResolvingController => resolving_controller,
                            ControlRecipient::TargetOwner => target_owner,
                        })
                    }
                    ResolvedPartSpec::Type(change) => ResolvedPart::Type(TypeChange {
                        land_subtypes: change.land_subtypes.clone(),
                        supertypes: change.supertypes.clone(),
                        types: change.types.clone(),
                        subtypes: match &change.subtypes {
                            SubtypeOperation::Keep => SubtypeOperation::Keep,
                            SubtypeOperation::Remove(set) => SubtypeOperation::Remove(set.clone()),
                            SubtypeOperation::Add(operand) => {
                                SubtypeOperation::Add(materialize_subtype(operand, values)?)
                            }
                            SubtypeOperation::Set(operand) => {
                                SubtypeOperation::Set(materialize_subtype(operand, values)?)
                            }
                        },
                    }),
                    ResolvedPartSpec::SetPT(pair) => ResolvedPart::SetPT(pair.clone()),
                    ResolvedPartSpec::Abilities(change) => ResolvedPart::Abilities(change.clone()),
                    ResolvedPartSpec::ModifyPT(pair) => ResolvedPart::ModifyPT(pair.clone()),
                    ResolvedPartSpec::SwitchPT => ResolvedPart::SwitchPT,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|mut parts| {
                parts.sort_by_key(ResolvedPart::bucket);
                ResolvedParts(parts)
            })
    }
}
fn materialize_word(
    word: CreatureTypeWord,
    values: &BTreeMap<CreatureTypeSlot, CreatureTypeId>,
) -> Result<CreatureTypeId, CharacteristicInvariantError> {
    match word {
        CreatureTypeWord::Literal(value) => Ok(value),
        CreatureTypeWord::Chosen(slot) => values.get(&slot).copied().ok_or(
            CharacteristicInvariantError::MissingChosenCreatureType(slot),
        ),
    }
}
fn materialize_subtype(
    operand: &InstructionSubtypeOperand,
    values: &BTreeMap<CreatureTypeSlot, CreatureTypeId>,
) -> Result<BTreeSet<CreatureTypeId>, CharacteristicInvariantError> {
    match operand {
        InstructionSubtypeOperand::Literal(values) => Ok(values.clone()),
        InstructionSubtypeOperand::Chosen(slot) => values
            .get(slot)
            .map(|value| BTreeSet::from([*value]))
            .ok_or(CharacteristicInvariantError::MissingChosenCreatureType(
                *slot,
            )),
    }
}
impl StaticContinuousAbility {
    /// The printed identity is independent of the effect's operating zone.
    pub fn id(&self) -> StaticAbilityId {
        match self {
            Self::Battlefield { id, .. } | Self::CharacteristicDefining { id, .. } => *id,
        }
    }

    /// Validate ordinary selectors/parts or finite self-only defining parts.
    ///
    /// # Errors
    /// Returns a typed selector, part, vocabulary or empty-definition failure.
    pub fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), CharacteristicDefinitionError> {
        let (selector, parts) = match self {
            Self::Battlefield {
                selector, parts, ..
            } => (selector, parts),
            Self::CharacteristicDefining { parts, .. } => {
                validate_buckets(parts.0.iter().map(DefiningPart::bucket))?;
                for part in &parts.0 {
                    match part {
                        DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(values)) => {
                            validate_subtypes(values, vocabulary)?
                        }
                        DefiningPart::PowerToughness { power, toughness } => {
                            if power.is_none() && toughness.is_none() {
                                return Err(
                                    CharacteristicDefinitionError::EmptyPowerToughnessDefinition,
                                );
                            }
                            for expression in power.iter().chain(toughness) {
                                expression.validate(vocabulary)?;
                            }
                        }
                        DefiningPart::CreatureTypes(DefiningCreatureTypes::All)
                        | DefiningPart::Colors(_) => {}
                    }
                }
                return Ok(());
            }
        };
        selector.validate(vocabulary)?;
        validate_buckets(parts.0.iter().map(StaticPart::bucket))?;
        for part in &parts.0 {
            match part {
                StaticPart::Text(change) => {
                    validate_subtypes(&BTreeSet::from([change.from, change.to]), vocabulary)?;
                }
                StaticPart::Type(change) => {
                    validate_battlefield_types(&change.types)?;
                    match &change.subtypes {
                        SubtypeOperation::Keep
                        | SubtypeOperation::Add(StaticSubtypeOperand::SourceCurrentSubtypes)
                        | SubtypeOperation::Set(StaticSubtypeOperand::SourceCurrentSubtypes) => {}
                        SubtypeOperation::Remove(values)
                        | SubtypeOperation::Add(StaticSubtypeOperand::Literal(values))
                        | SubtypeOperation::Set(StaticSubtypeOperand::Literal(values)) => {
                            validate_subtypes(values, vocabulary)?
                        }
                    }
                }
                StaticPart::SetPT(pair) | StaticPart::ModifyPT(pair) => {
                    for expression in [&pair.power, &pair.toughness] {
                        expression.validate(vocabulary)?;
                    }
                }
                StaticPart::Names(_)
                | StaticPart::ControlSourceController
                | StaticPart::Color(_)
                | StaticPart::Abilities(_)
                | StaticPart::SwitchPT => {}
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContinuousEffect {
    pub id: ContinuousEffectId,
    pub timestamp: CharacteristicTimestamp,
    pub creator: StackEntryId,
    pub affected: BTreeSet<ObjectId>,
    pub parts: ResolvedParts,
    pub duration: ResolvedDuration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacteristicsError {
    EffectIdentityExhausted,
    TimestampExhausted,
    WorldTenureExhausted,
    InvalidInitialWorldCohorts,
    InvalidCounterPlacement,
    InvalidInitialBattlefieldOrder,
    InitialBattlefieldAlreadyRegistered,
    DuplicateBattlefieldTimestamp,
    UnsupportedTimestampProducer,
    InvalidParts(CharacteristicDefinitionError),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacteristicInvariantError {
    MissingDefinition(ObjectId),
    MissingBattlefieldTimestamp(ObjectId),
    MissingBattlefieldController(ObjectId),
    DuplicateTimestamp(CharacteristicTimestamp),
    MissingChosenCreatureType(CreatureTypeSlot),
    InvalidResolvedParts(ContinuousEffectId),
    InvalidStaticParts(ObjectId),
    NoEligibleComponent,
    MissingWorldTenure(ObjectId),
    UnexpectedWorldTenure(ObjectId),
}
/// One cohort of objects that most recently gained World at the same actual cut.
/// Ordering is uninterrupted World tenure, independent of effect/object timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorldTenure(pub(super) u64);
impl WorldTenure {
    /// Monotonic cohort ordinal within this game's existing characteristic owner.
    pub fn sequence(self) -> u64 {
        self.0
    }
}
/// Actual membership changes at one published cut; retained Worlds emit no change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldTenureChange {
    Gained {
        object: ObjectId,
        tenure: WorldTenure,
    },
    Lost {
        object: ObjectId,
        tenure: WorldTenure,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedCharacteristics {
    pub names: spellwake_contracts::names::ObjectNames,
    pub supertypes: BTreeSet<Supertype>,
    pub types: BTreeSet<CardType>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub land_subtypes: BTreeSet<LandType>,
    pub colors: BTreeSet<Color>,
    pub printed_pt: Option<PrintedPowerToughness>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrintedAbilityKind {
    Mana(ManaAbilityId),
    Activated(ActivatedAbilityId),
    Triggered(TriggeredAbilityId),
    Continuous(StaticAbilityId),
    ZoneReplacement(crate::program::ZoneReplacementAbilityId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrintedAbilityIdentity {
    pub source: ObjectId,
    pub card: Option<CardId>,
    pub kind: PrintedAbilityKind,
}
/// Current surviving printed definition identities after layer 6. Exact ability
/// lookup uses this roster and the evaluated printed text; stack entries and
/// waiting trigger captures own their bodies independently of later changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintedAbilityRoster {
    /// Currently surviving replacement definitions, never inferred from retained text.
    pub zone_replacements: Vec<crate::program::ZoneReplacementAbilityId>,
    pub source: ObjectId,
    pub card: Option<CardId>,
    pub mana: Vec<ManaAbilityId>,
    pub activated: Vec<ActivatedAbilityId>,
    pub triggered: Vec<TriggeredAbilityId>,
    pub continuous: Option<StaticAbilityId>,
}
impl PrintedAbilityRoster {
    pub(super) fn remove(&mut self, removal: AbilityRemoval) {
        match removal {
            AbilityRemoval::All => self.mana.clear(),
            AbilityRemoval::NonMana => {}
        }
        self.zone_replacements.clear();
        self.activated.clear();
        self.triggered.clear();
        self.continuous = None;
    }
    pub fn identities(&self) -> Vec<PrintedAbilityIdentity> {
        self.zone_replacements
            .iter()
            .map(|id| PrintedAbilityKind::ZoneReplacement(*id))
            .chain(
                self.mana
                    .iter()
                    .map(|ability| PrintedAbilityKind::Mana(*ability))
                    .chain(
                        self.activated
                            .iter()
                            .map(|ability| PrintedAbilityKind::Activated(*ability)),
                    )
                    .chain(
                        self.triggered
                            .iter()
                            .map(|ability| PrintedAbilityKind::Triggered(*ability)),
                    )
                    .chain(
                        self.continuous
                            .iter()
                            .map(|ability| PrintedAbilityKind::Continuous(*ability)),
                    ),
            )
            .map(|kind| PrintedAbilityIdentity {
                source: self.source,
                card: self.card,
                kind,
            })
            .collect()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughnessEvaluation {
    pub object: ObjectId,
    pub copiable: Option<PrintedPowerToughness>,
    pub base: PowerToughness,
    pub derived: PowerToughness,
    pub applied_effects: Vec<EffectInstanceKey>,
}
/// Scratch copies of the printed ability families containing admitted semantic
/// creature-type words. Counted mana includes its semantic selector. This is
/// not a second definition owner or a roster of granted abilities (CR 612.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintedText {
    /// Semantic replacement definitions; the ability roster separately determines existence.
    pub zone_replacements: Vec<crate::program::ZoneReplacementAbility>,
    pub mana: Vec<ManaAbility>,
    pub activated: Vec<ActivatedAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub continuous: Option<StaticContinuousAbility>,
}
impl PrintedText {
    pub(super) fn from_program(program: &Program) -> Self {
        Self {
            zone_replacements: program.zone_replacements.clone(),
            mana: program.mana.clone(),
            activated: program.activated.clone(),
            triggered: program.triggered.clone(),
            continuous: program.continuous.clone(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectCharacteristics {
    pub object: ObjectId,
    /// Derived from this exact physical world, never an independent location owner.
    pub zone: ObjectZone,
    pub defined: DefinedCharacteristics,
    pub current_names: spellwake_contracts::names::ObjectNames,
    pub current_supertypes: BTreeSet<Supertype>,
    pub current_types: BTreeSet<CardType>,
    pub current_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub current_land_subtypes: BTreeSet<LandType>,
    /// Derived basic-land abilities after current type and ability effects.
    pub intrinsic_mana: BTreeSet<BasicLandType>,
    pub current_colors: BTreeSet<Color>,
    pub controller: Option<SeatId>,
    pub abilities: PrintedAbilityRoster,
    /// Printed semantic definitions after layer 3; availability is in `abilities`.
    pub printed_text: PrintedText,
    /// Keywords generated in the current pass; not physical status or counters.
    pub keywords: BTreeSet<KeywordAbility>,
    /// Current numeric values in this exact zone. Battlefield noncreatures
    /// have none; nonbattlefield objects may retain their printed PT box.
    pub power_toughness: Option<PowerToughnessEvaluation>,
}
impl ObjectCharacteristics {
    /// All currently available mana references, in deterministic origin/type order.
    /// This query creates no authored definitions or persistent ability roster.
    pub fn mana_abilities(&self) -> BTreeSet<ManaAbilityRef> {
        self.abilities
            .mana
            .iter()
            .copied()
            .map(ManaAbilityRef::Printed)
            .chain(
                self.intrinsic_mana
                    .iter()
                    .copied()
                    .map(ManaAbilityRef::BasicLand),
            )
            .collect()
    }
    pub(super) fn derive_intrinsic_mana(&mut self) {
        self.intrinsic_mana = if self.has_type(CardType::Land) {
            self.current_land_subtypes
                .iter()
                .copied()
                .filter_map(LandType::basic)
                .collect()
        } else {
            BTreeSet::new()
        };
    }
    /// Query a keyword in this exact evaluated world.
    pub fn has_keyword(&self, keyword: KeywordAbility) -> bool {
        self.keywords.contains(&keyword)
    }
    /// Whether the current type line can retain this shared subtype family.
    pub fn carries_creature_subtypes(&self) -> bool {
        self.current_types
            .iter()
            .copied()
            .any(CardType::carries_creature_subtypes)
    }
    pub fn is_creature(&self) -> bool {
        self.current_types.contains(&CardType::Creature)
    }
    pub fn has_type(&self, kind: CardType) -> bool {
        self.current_types.contains(&kind)
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct CharacteristicWorld<'a> {
    pub cards: &'a GameObjects,
    pub programs: &'a BTreeMap<CardId, Program>,
    pub state: &'a CharacteristicState,
    pub vocabulary: &'a CreatureTypeVocabulary,
    pub name_rules: &'a spellwake_contracts::names::NameRules,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectInstanceKey {
    Resolved(ContinuousEffectId),
    Static {
        source: ObjectId,
        ability: StaticAbilityId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassBinding {
    NotProcessed,
    Processed(BTreeSet<ObjectId>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedComponent {
    pub key: EffectInstanceKey,
    pub bucket: CharacteristicBucket,
    pub present: bool,
    pub affected: BTreeSet<ObjectId>,
    pub generated: BTreeMap<ObjectId, ResolvedPart>,
}
impl GeneratedComponent {
    pub(crate) fn same_semantics(&self, other: &Self) -> bool {
        self.present == other.present
            && self.affected == other.affected
            && self.generated == other.generated
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentWait {
    pub waiting: EffectInstanceKey,
    pub prerequisite: EffectInstanceKey,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSelection {
    pub bucket: CharacteristicBucket,
    pub generated: Vec<GeneratedComponent>,
    pub original_waits: BTreeSet<ComponentWait>,
    pub strongly_connected: Vec<BTreeSet<EffectInstanceKey>>,
    pub derived_timestamp_waits: BTreeSet<ComponentWait>,
    pub retained_outside_waits: BTreeSet<ComponentWait>,
    pub eligible: Vec<EffectInstanceKey>,
    pub chosen: EffectInstanceKey,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedComponent {
    pub key: EffectInstanceKey,
    pub bucket: CharacteristicBucket,
    pub affected: BTreeSet<ObjectId>,
    pub generated: BTreeMap<ObjectId, ResolvedPart>,
}
/// Derived record at the one physical-counter calculation between represented
/// layer 7c effects and layer 7d. It is not an occurrence or a physical write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterApplication {
    pub object: ObjectId,
    pub after_components: usize,
    pub counters: PowerToughnessCounters,
    pub before: PowerToughness,
    pub after: PowerToughness,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationTrace {
    pub selections: Vec<ComponentSelection>,
    pub applied: Vec<AppliedComponent>,
    pub counter_applications: Vec<CounterApplication>,
    pub bindings: BTreeMap<EffectInstanceKey, PassBinding>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluatedCharacteristics {
    pub objects: BTreeMap<ObjectId, ObjectCharacteristics>,
    pub trace: EvaluationTrace,
}
impl EvaluatedCharacteristics {
    pub fn get(&self, object: ObjectId) -> Option<&ObjectCharacteristics> {
        self.objects.get(&object)
    }
}
