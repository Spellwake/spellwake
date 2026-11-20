//! Strict current definition codec. Raw JSON callers first reject duplicate keys
//! with the shared packet parser; owner admission remains a separate boundary.
//! Empty tagged variants use struct syntax so unknown fields are rejected.
use super::{creature_types, projection};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Value, json};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, Color, ColorFilter, ColorOperation, ControlRecipient,
        ControllerSelector, CreatureTypeTextChange, CreatureTypeTextChangeSpec, CreatureTypeWord,
        DefiningCreatureTypes, DefiningPart, DefiningParts, InstructionSubtypeOperand,
        KeywordAbility, NameFilter, NameOperation, NumericExpression, NumericPair,
        PermanentSelector, PowerToughness, PowerToughnessCounters, PrintedPowerToughness,
        PrintedStat, PrintedText, ResolvedDuration, ResolvedPart, ResolvedPartSpec, ResolvedParts,
        ResolvedPartsSpec, StaticAbilityId, StaticColorOperand, StaticContinuousAbility,
        StaticLandSubtypeOperand, StaticNameOperand, StaticPart, StaticParts, StaticSubtypeOperand,
        StaticTypeChange, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    land::LandType,
    life::{LifeAdjustment, LifeChangeKind},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    program::{
        AffectedPlayers, BodyTarget, CardType, Comparison, ConditionValue, ExecutableBody,
        Instruction, InstructionAmount, ObjectRelation, OrdinaryCondition, PermanentDefinition,
        PermanentTypeFilter, PlayerRelation, Program, ProgramKind, ResolutionCondition,
        ResolvingPlayer, SpellCastFilter, SpellDefinition, SpellTargetFilter, Supertype,
        TriggeredAbility, TriggeredAbilityId,
    },
    program::{
        PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
    turn::Step,
};
use std::collections::{BTreeMap, BTreeSet};

fn object_value<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    if !value.is_object() {
        return Err("expected a JSON object".into());
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    object_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

fn objects<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Vec::<Value>::deserialize(deserializer)?
        .into_iter()
        .map(object_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serde::de::Error::custom)
}

/// An explicit null or object is valid; an omitted field is not current syntax.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Option::<Value>::deserialize(deserializer)?
        .map(object_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn land_types(values: Vec<String>) -> Result<BTreeSet<LandType>, String> {
    let mut result = BTreeSet::new();
    for label in values {
        let value = LandType::from_wire_label(&label).ok_or("unknown land subtype")?;
        if !result.insert(value) {
            return Err("duplicate land subtype".into());
        }
    }
    Ok(result)
}
fn land_type_values(values: &BTreeSet<LandType>) -> Value {
    json!(
        values
            .iter()
            .map(|value| value.wire_label())
            .collect::<Vec<_>>()
    )
}
fn card_types(values: Vec<String>) -> Result<BTreeSet<CardType>, String> {
    let count = values.len();
    let values = values
        .into_iter()
        .map(|value| card_type(&value))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if values.len() != count {
        return Err("duplicate card type".into());
    }
    Ok(values)
}
fn card_type(value: &str) -> Result<CardType, String> {
    match value {
        "Land" => Ok(CardType::Land),
        "Artifact" => Ok(CardType::Artifact),
        "Creature" => Ok(CardType::Creature),
        "Enchantment" => Ok(CardType::Enchantment),
        "Kindred" => Ok(CardType::Kindred),
        "Instant" => Ok(CardType::Instant),
        "Sorcery" => Ok(CardType::Sorcery),
        _ => Err(format!("unknown card type {value}")),
    }
}

fn supertypes(values: Vec<String>) -> Result<BTreeSet<Supertype>, String> {
    let count = values.len();
    let values = values
        .into_iter()
        .map(|value| match value.as_str() {
            "Basic" => Ok(Supertype::Basic),
            "Legendary" => Ok(Supertype::Legendary),
            "Ongoing" => Ok(Supertype::Ongoing),
            "Snow" => Ok(Supertype::Snow),
            "World" => Ok(Supertype::World),
            _ => Err(format!("unknown supertype {value}")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if values.len() != count {
        return Err("duplicate supertype".into());
    }
    Ok(values)
}

fn subtype_set(
    values: Vec<Value>,
    vocabulary: &CreatureTypeVocabulary,
    name: &str,
) -> Result<BTreeSet<CreatureTypeId>, String> {
    let count = values.len();
    let values = values
        .into_iter()
        .map(|value| {
            if !value.is_object() {
                return Err("creature type ID must be a JSON object".into());
            }
            creature_types::id(value, vocabulary)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if values.len() != count {
        return Err(format!("duplicate {name}"));
    }
    Ok(values)
}

fn color(name: &str) -> Result<Color, String> {
    match name {
        "White" => Ok(Color::White),
        "Blue" => Ok(Color::Blue),
        "Black" => Ok(Color::Black),
        "Red" => Ok(Color::Red),
        "Green" => Ok(Color::Green),
        _ => Err(format!("unknown Magic color {name}")),
    }
}
fn color_set(names: Vec<String>) -> Result<BTreeSet<Color>, String> {
    let count = names.len();
    let values = names
        .iter()
        .map(|name| color(name))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if values.len() != count {
        return Err("duplicate Magic color".into());
    }
    Ok(values)
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ColorOperand {
    Literal { colors: Vec<String> },
    SourceCurrentColors {},
}
impl ColorOperand {
    fn literal(self) -> Result<BTreeSet<Color>, String> {
        match self {
            Self::Literal { colors } => color_set(colors),
            Self::SourceCurrentColors {} => Err("live color operand in a resolved effect".into()),
        }
    }
    fn live(self) -> Result<StaticColorOperand, String> {
        Ok(match self {
            Self::Literal { colors } => StaticColorOperand::Literal(color_set(colors)?),
            Self::SourceCurrentColors {} => StaticColorOperand::SourceCurrentColors,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ColorOperationWire {
    Set { value: ColorOperand },
    Add { value: ColorOperand },
    Remove { value: ColorOperand },
}
impl ColorOperationWire {
    fn native<T>(
        self,
        operand: impl FnOnce(ColorOperand) -> Result<T, String>,
    ) -> Result<ColorOperation<T>, String> {
        Ok(match self {
            Self::Set { value } => ColorOperation::Set(operand(value)?),
            Self::Add { value } => ColorOperation::Add(operand(value)?),
            Self::Remove { value } => ColorOperation::Remove(operand(value)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ColorFilterWire {
    Any {},
    Has { color: String },
    Colorless {},
    Monocolored {},
    Multicolored {},
}
impl ColorFilterWire {
    fn native(self) -> Result<ColorFilter, String> {
        Ok(match self {
            Self::Any {} => ColorFilter::Any,
            Self::Has { color: name } => ColorFilter::Has(color(&name)?),
            Self::Colorless {} => ColorFilter::Colorless,
            Self::Monocolored {} => ColorFilter::Monocolored,
            Self::Multicolored {} => ColorFilter::Multicolored,
        })
    }
}

pub fn mana(name: &str) -> Result<ManaType, String> {
    match name {
        "White" => Ok(ManaType::White),
        "Blue" => Ok(ManaType::Blue),
        "Black" => Ok(ManaType::Black),
        "Red" => Ok(ManaType::Red),
        "Green" => Ok(ManaType::Green),
        "Colourless" => Ok(ManaType::Colourless),
        _ => Err(format!("unknown mana type {name}")),
    }
}
pub fn step(name: &str) -> Result<Step, String> {
    match name {
        "Untap" => Ok(Step::Untap),
        "Upkeep" => Ok(Step::Upkeep),
        "Draw" => Ok(Step::Draw),
        "PrecombatMain" => Ok(Step::PrecombatMain),
        "BeginningCombat" => Ok(Step::BeginningCombat),
        "DeclareAttackers" => Ok(Step::DeclareAttackers),
        "DeclareBlockers" => Ok(Step::DeclareBlockers),
        "CombatDamage" => Ok(Step::CombatDamage),
        "EndCombat" => Ok(Step::EndCombat),
        "PostcombatMain" => Ok(Step::PostcombatMain),
        "End" => Ok(Step::End),
        "Cleanup" => Ok(Step::Cleanup),
        _ => Err(format!("unknown step {name}")),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cost {
    generic: RulesInteger,
    required: BTreeMap<String, RulesInteger>,
}
impl Cost {
    fn native(self) -> Result<ManaCost, String> {
        Ok(ManaCost {
            generic: self.generic,
            required: mana_map(self.required)?,
        })
    }
}
fn mana_map(
    values: BTreeMap<String, RulesInteger>,
) -> Result<BTreeMap<ManaType, RulesInteger>, String> {
    values
        .into_iter()
        .map(|(name, amount)| Ok((mana(&name)?, amount)))
        .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pt {
    power: RulesInteger,
    toughness: RulesInteger,
}
impl From<Pt> for PowerToughness {
    fn from(value: Pt) -> Self {
        Self {
            power: value.power,
            toughness: value.toughness,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Counters {
    plus_one: RulesInteger,
    minus_one: RulesInteger,
}
impl From<Counters> for PowerToughnessCounters {
    fn from(value: Counters) -> Self {
        Self {
            plus_one: value.plus_one,
            minus_one: value.minus_one,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Target {
    None {},
    SinglePermanent {},
    SingleCreature {},
    SinglePlayer {},
    SingleCreatureOrPlayer {},
    SingleSpell { support_gate: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    target: Target,
    instructions: Vec<Op>,
}
#[derive(Deserialize)]
#[serde(tag = "family", deny_unknown_fields)]
enum TokenSubtypeWire {
    Creature { r#type: Value },
    Land { r#type: String },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum TokenNameWire {
    Specified {
        name: EnglishCardName,
    },
    Default {
        #[serde(deserialize_with = "objects")]
        ordered_subtypes: Vec<TokenSubtypeWire>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenDefinitionWire {
    program: Value,
    #[serde(deserialize_with = "object")]
    name: TokenNameWire,
}
impl TokenDefinitionWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TokenDefinition, String> {
        let name = match self.name {
            TokenNameWire::Specified { name } => TokenName::Specified(name),
            TokenNameWire::Default { ordered_subtypes } => TokenName::Default {
                ordered_subtypes: ordered_subtypes
                    .into_iter()
                    .map(|word| {
                        Ok(match word {
                            TokenSubtypeWire::Creature { r#type } => {
                                TokenSubtype::Creature(creature_types::id(r#type, vocabulary)?)
                            }
                            TokenSubtypeWire::Land { r#type } => TokenSubtype::Land(
                                LandType::from_wire_label(&r#type)
                                    .ok_or("unknown token land subtype")?,
                            ),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            },
        };
        TokenDefinition::new(
            decode_for_owner(self.program, vocabulary)?,
            name,
            vocabulary,
        )
        .map_err(|error| format!("invalid token definition: {error:?}"))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenCreationWire {
    #[serde(deserialize_with = "object")]
    definition: TokenDefinitionWire,
    #[serde(deserialize_with = "object")]
    amount: Amount,
    recipients: TokenRecipientsWire,
}
#[derive(Deserialize)]
enum TokenRecipientsWire {
    Controller,
    EachPlayer,
    EachOpponent,
}
impl TokenCreationWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TokenCreation, String> {
        Ok(TokenCreation {
            definition: self.definition.native(vocabulary)?,
            amount: self.amount.native()?,
            recipients: match self.recipients {
                TokenRecipientsWire::Controller => TokenRecipients::Controller,
                TokenRecipientsWire::EachPlayer => TokenRecipients::EachPlayer,
                TokenRecipientsWire::EachOpponent => TokenRecipients::EachOpponent,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Op {
    Scry {
        player: ResolvingPlayerWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
    },
    Surveil {
        player: ResolvingPlayerWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
    },
    Mill {
        player: ResolvingPlayerWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
    },
    OptionalEffect {
        chooser: ResolvingPlayerWire,
    },
    OfferManaPayment {
        payer: ResolvingPlayerWire,
        #[serde(deserialize_with = "object")]
        cost: Cost,
        paid_destination: String,
        declined_destination: String,
    },
    BranchIfFalse {
        #[serde(deserialize_with = "object")]
        condition: ResolutionConditionWire,
        destination: String,
    },
    Jump {
        destination: String,
    },
    CreateTokens {
        #[serde(deserialize_with = "objects")]
        creations: Vec<TokenCreationWire>,
    },
    AdjustControllerLife {
        adjustment: LifeAdjustmentWire,
    },
    AdjustTargetLife {
        adjustment: LifeAdjustmentWire,
    },
    DrawController {
        amount: Amount,
    },
    Sacrifice {
        #[serde(deserialize_with = "object")]
        players: AffectedPlayersWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
        #[serde(deserialize_with = "object")]
        filter: PermanentTypeFilterWire,
    },
    Discard {
        #[serde(deserialize_with = "object")]
        players: AffectedPlayersWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
    },
    ChooseCreatureType {
        into: Value,
        excluding: Vec<Value>,
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
    ReturnToOwnersHand {},
    CounterSpell {},
    CopySpell {},
    CopyTargetToSource {
        duration: Duration,
        #[serde(deserialize_with = "required_nullable")]
        power_toughness: Option<Pt>,
    },
    CreateContinuousEffect {
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    PutPowerToughnessCounters {
        #[serde(deserialize_with = "object")]
        counters: Counters,
    },
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum ResolvingPlayerWire {
    Controller,
    UniqueRelatedPlayer,
    TargetSpellController,
    TargetPlayer,
}
impl TryFrom<String> for ResolvingPlayerWire {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "Controller" => Ok(Self::Controller),
            "UniqueRelatedPlayer" => Ok(Self::UniqueRelatedPlayer),
            "TargetSpellController" => Ok(Self::TargetSpellController),
            "TargetPlayer" => Ok(Self::TargetPlayer),
            _ => Err(format!("unknown resolving player {value}")),
        }
    }
}
impl From<ResolvingPlayerWire> for ResolvingPlayer {
    fn from(value: ResolvingPlayerWire) -> Self {
        match value {
            ResolvingPlayerWire::Controller => Self::Controller,
            ResolvingPlayerWire::UniqueRelatedPlayer => Self::UniqueRelatedPlayer,
            ResolvingPlayerWire::TargetSpellController => Self::TargetSpellController,
            ResolvingPlayerWire::TargetPlayer => Self::TargetPlayer,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum AffectedPlayersWire {
    One { player: ResolvingPlayerWire },
    EachPlayer {},
}
impl From<AffectedPlayersWire> for AffectedPlayers {
    fn from(value: AffectedPlayersWire) -> Self {
        match value {
            AffectedPlayersWire::One { player } => Self::One(player.into()),
            AffectedPlayersWire::EachPlayer {} => Self::EachPlayer,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermanentTypeFilterWire {
    required_types: Vec<String>,
    forbidden_types: Vec<String>,
}
impl PermanentTypeFilterWire {
    fn native(self) -> Result<PermanentTypeFilter, String> {
        Ok(PermanentTypeFilter {
            required_types: card_types(self.required_types)?,
            forbidden_types: card_types(self.forbidden_types)?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolutionConditionWire {
    left: ConditionValueWire,
    comparison: ComparisonWire,
    right: ConditionValueWire,
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ConditionValueWire {
    Literal { value: RulesInteger },
    ControllerLife {},
    MatchedOccurrences {},
}
impl ConditionValueWire {
    fn native(self) -> ConditionValue {
        match self {
            Self::Literal { value } => ConditionValue::Literal(value),
            Self::ControllerLife {} => ConditionValue::ControllerLife,
            Self::MatchedOccurrences {} => ConditionValue::MatchedOccurrences,
        }
    }
}
#[derive(Deserialize)]
enum ComparisonWire {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}
impl ResolutionConditionWire {
    fn native(self) -> ResolutionCondition {
        ResolutionCondition {
            left: self.left.native(),
            comparison: match self.comparison {
                ComparisonWire::Equal => Comparison::Equal,
                ComparisonWire::NotEqual => Comparison::NotEqual,
                ComparisonWire::Less => Comparison::Less,
                ComparisonWire::LessOrEqual => Comparison::LessOrEqual,
                ComparisonWire::Greater => Comparison::Greater,
                ComparisonWire::GreaterOrEqual => Comparison::GreaterOrEqual,
            },
            right: self.right.native(),
        }
    }
}
fn instruction_destination(value: String) -> Result<usize, String> {
    let parsed = value.parse::<usize>().map_err(|_| {
        "instruction destination must be a canonical unsigned usize decimal string".to_owned()
    })?;
    if parsed.to_string() != value {
        return Err(
            "instruction destination must be a canonical unsigned usize decimal string".into(),
        );
    }
    Ok(parsed)
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum LifeAdjustmentWire {
    Gain { amount: RulesInteger },
    Lose { amount: RulesInteger },
    Set { total: RulesInteger },
}
impl LifeAdjustmentWire {
    fn native(self) -> LifeAdjustment {
        match self {
            Self::Gain { amount } => LifeAdjustment::Gain(amount),
            Self::Lose { amount } => LifeAdjustment::Lose(amount),
            Self::Set { total } => LifeAdjustment::Set(total),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Amount {
    Literal { value: RulesInteger },
    ControlledPermanentsOfChosenType { chosen: Value },
    MatchedOccurrences {},
}
impl Amount {
    fn native(self) -> Result<InstructionAmount, String> {
        Ok(match self {
            Self::Literal { value } => InstructionAmount::Literal(value),
            Self::MatchedOccurrences {} => InstructionAmount::MatchedOccurrences,
            Self::ControlledPermanentsOfChosenType { chosen } => {
                InstructionAmount::ControlledPermanentsOfChosenType {
                    chosen: creature_types::slot(chosen)?,
                }
            }
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum TypeOperationWire {
    Keep {},
    Add { types: Vec<String> },
    Remove { types: Vec<String> },
    Set { types: Vec<String> },
}
impl TypeOperationWire {
    fn native<T>(
        self,
        decode: impl FnOnce(Vec<String>) -> Result<BTreeSet<T>, String>,
    ) -> Result<TypeOperation<T>, String> {
        Ok(match self {
            Self::Keep {} => TypeOperation::Keep,
            Self::Add { types } => TypeOperation::Add(decode(types)?),
            Self::Remove { types } => TypeOperation::Remove(decode(types)?),
            Self::Set { types } => TypeOperation::Set(decode(types)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum SubtypeOperationWire<T, Member = Value> {
    Keep {},
    Add { value: T },
    Remove { types: Vec<Member> },
    Set { value: T },
}
impl<T, RawMember> SubtypeOperationWire<T, RawMember> {
    fn native<U, Member>(
        self,
        operand: impl FnOnce(T) -> Result<U, String>,
        members: impl FnOnce(Vec<RawMember>) -> Result<BTreeSet<Member>, String>,
    ) -> Result<SubtypeOperation<U, Member>, String> {
        Ok(match self {
            Self::Keep {} => SubtypeOperation::Keep,
            Self::Add { value } => SubtypeOperation::Add(operand(value)?),
            Self::Remove { types } => SubtypeOperation::Remove(members(types)?),
            Self::Set { value } => SubtypeOperation::Set(operand(value)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum LandOperand {
    Literal { types: Vec<String> },
    SourceCurrentLandSubtypes {},
}
impl LandOperand {
    fn literal(self) -> Result<BTreeSet<LandType>, String> {
        match self {
            Self::Literal { types } => land_types(types),
            Self::SourceCurrentLandSubtypes {} => {
                Err("live land operand in a resolved effect".into())
            }
        }
    }
    fn live(self) -> Result<StaticLandSubtypeOperand, String> {
        Ok(match self {
            Self::Literal { types } => StaticLandSubtypeOperand::Literal(land_types(types)?),
            Self::SourceCurrentLandSubtypes {} => {
                StaticLandSubtypeOperand::SourceCurrentLandSubtypes
            }
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum InstructionOperand {
    Literal { types: Vec<Value> },
    Chosen { slot: Value },
}
impl InstructionOperand {
    fn native(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<InstructionSubtypeOperand, String> {
        Ok(match self {
            Self::Literal { types } => InstructionSubtypeOperand::Literal(subtype_set(
                types,
                vocabulary,
                "creature subtype",
            )?),
            Self::Chosen { slot } => InstructionSubtypeOperand::Chosen(creature_types::slot(slot)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum StaticOperand {
    Literal { types: Vec<Value> },
    SourceCurrentSubtypes {},
}
impl StaticOperand {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<StaticSubtypeOperand, String> {
        Ok(match self {
            Self::Literal { types } => {
                StaticSubtypeOperand::Literal(subtype_set(types, vocabulary, "creature subtype")?)
            }
            Self::SourceCurrentSubtypes {} => StaticSubtypeOperand::SourceCurrentSubtypes,
        })
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum AbilityRemovalWire {
    All,
    NonMana,
}
impl TryFrom<String> for AbilityRemovalWire {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "All" => Ok(Self::All),
            "NonMana" => Ok(Self::NonMana),
            _ => Err(format!("unknown ability removal {value}")),
        }
    }
}
impl AbilityRemovalWire {
    fn native(self) -> AbilityRemoval {
        match self {
            Self::All => AbilityRemoval::All,
            Self::NonMana => AbilityRemoval::NonMana,
        }
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum KeywordWire {
    Haste,
    Vigilance,
}
impl TryFrom<String> for KeywordWire {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "Haste" => Ok(Self::Haste),
            "Vigilance" => Ok(Self::Vigilance),
            _ => Err(format!("unsupported keyword {value}")),
        }
    }
}
fn ability_change(
    removal: Option<AbilityRemovalWire>,
    keywords: Vec<KeywordWire>,
) -> Result<AbilityChange, String> {
    let count = keywords.len();
    let granted_keywords: BTreeSet<_> = keywords
        .into_iter()
        .map(|keyword| match keyword {
            KeywordWire::Haste => KeywordAbility::Haste,
            KeywordWire::Vigilance => KeywordAbility::Vigilance,
        })
        .collect();
    if granted_keywords.len() != count {
        return Err("duplicate granted keyword".into());
    }
    Ok(AbilityChange {
        removal: removal.map(AbilityRemovalWire::native),
        granted_keywords,
    })
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum ControlRecipientWire {
    ResolvingController,
    TargetOwner,
}
impl TryFrom<String> for ControlRecipientWire {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "ResolvingController" => Ok(Self::ResolvingController),
            "TargetOwner" => Ok(Self::TargetOwner),
            _ => Err(format!("unknown control recipient {value}")),
        }
    }
}
impl ControlRecipientWire {
    fn native(self) -> ControlRecipient {
        match self {
            Self::ResolvingController => ControlRecipient::ResolvingController,
            Self::TargetOwner => ControlRecipient::TargetOwner,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum NameOperandWire {
    Literal { names: ObjectNames },
    SourceCurrentNames {},
}
impl NameOperandWire {
    fn literal(self) -> Result<ObjectNames, String> {
        match self {
            Self::Literal { names } => Ok(names),
            Self::SourceCurrentNames {} => Err("live name operand in resolved effect".into()),
        }
    }
    fn live(self) -> Result<StaticNameOperand, String> {
        Ok(match self {
            Self::Literal { names } => StaticNameOperand::Literal(names),
            Self::SourceCurrentNames {} => StaticNameOperand::SourceCurrentNames,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum NameOperationWire {
    Set { operand: NameOperandWire },
    Add { operand: NameOperandWire },
}
impl NameOperationWire {
    fn native<T>(
        self,
        resolve: impl FnOnce(NameOperandWire) -> Result<T, String>,
    ) -> Result<NameOperation<T>, String> {
        Ok(match self {
            Self::Set { operand } => NameOperation::Set(resolve(operand)?),
            Self::Add { operand } => NameOperation::Add(resolve(operand)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum NameFilterWire {
    Any {},
    Named { name: EnglishCardName },
    SharesWithSource {},
    DifferentFromSource {},
}
impl NameFilterWire {
    fn native(self) -> NameFilter {
        match self {
            Self::Any {} => NameFilter::Any,
            Self::Named { name } => NameFilter::Named(name),
            Self::SharesWithSource {} => NameFilter::SharesWithSource,
            Self::DifferentFromSource {} => NameFilter::DifferentFromSource,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum PartWire<T, N> {
    Names {
        change: NameOperationWire,
    },
    Color {
        change: ColorOperationWire,
    },
    Text {
        from: WordWire,
        to: WordWire,
    },
    Control {
        recipient: ControlRecipientWire,
    },
    ControlSourceController {},
    Type {
        supertypes: TypeOperationWire,
        types: TypeOperationWire,
        subtypes: SubtypeOperationWire<T>,
        land_subtypes: SubtypeOperationWire<LandOperand, String>,
    },
    SetPT {
        power: N,
        toughness: N,
    },
    ModifyPT {
        power: N,
        toughness: N,
    },
    SwitchPT {},
    Abilities {
        #[serde(deserialize_with = "Option::deserialize")]
        removal: Option<AbilityRemovalWire>,
        granted_keywords: Vec<KeywordWire>,
    },
}
impl PartWire<InstructionOperand, RulesInteger> {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ResolvedPartSpec, String> {
        Ok(match self {
            Self::Names { change } => {
                ResolvedPartSpec::Names(change.native(NameOperandWire::literal)?)
            }
            Self::Color { change } => {
                ResolvedPartSpec::Color(change.native(ColorOperand::literal)?)
            }
            Self::Text { from, to } => ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                from: from.native(vocabulary)?,
                to: to.native(vocabulary)?,
            }),
            Self::Control { recipient } => ResolvedPartSpec::Control(recipient.native()),
            Self::ControlSourceController {} => {
                return Err("live control operand in a resolved effect".into());
            }
            Self::Type {
                supertypes: supers,
                types,
                subtypes,
                land_subtypes,
            } => ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: land_subtypes.native(LandOperand::literal, land_types)?,
                supertypes: supers.native(supertypes)?,
                types: types.native(card_types)?,
                subtypes: subtypes.native(
                    |value| InstructionOperand::native(value, vocabulary),
                    |values| subtype_set(values, vocabulary, "creature subtype"),
                )?,
            }),
            Self::SetPT { power, toughness } => {
                ResolvedPartSpec::SetPT(PowerToughness { power, toughness })
            }
            Self::ModifyPT { power, toughness } => {
                ResolvedPartSpec::ModifyPT(PowerToughness { power, toughness })
            }
            Self::SwitchPT {} => ResolvedPartSpec::SwitchPT,
            Self::Abilities {
                removal,
                granted_keywords,
            } => ResolvedPartSpec::Abilities(ability_change(removal, granted_keywords)?),
        })
    }
}
impl PartWire<StaticOperand, Expression> {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<StaticPart, String> {
        Ok(match self {
            Self::Names { change } => StaticPart::Names(change.native(NameOperandWire::live)?),
            Self::Color { change } => StaticPart::Color(change.native(ColorOperand::live)?),
            Self::Text { from, to } => StaticPart::Text(CreatureTypeTextChange {
                from: from.literal(vocabulary)?,
                to: to.literal(vocabulary)?,
            }),
            Self::ControlSourceController {} => StaticPart::ControlSourceController,
            Self::Control { .. } => {
                return Err("resolution control operand in a live static".into());
            }
            Self::Type {
                supertypes: supers,
                types,
                subtypes,
                land_subtypes,
            } => StaticPart::Type(StaticTypeChange {
                land_subtypes: land_subtypes.native(LandOperand::live, land_types)?,
                supertypes: supers.native(supertypes)?,
                types: types.native(card_types)?,
                subtypes: subtypes.native(
                    |value| StaticOperand::native(value, vocabulary),
                    |values| subtype_set(values, vocabulary, "creature subtype"),
                )?,
            }),
            Self::SetPT { power, toughness } => StaticPart::SetPT(NumericPair {
                power: power.native(vocabulary)?,
                toughness: toughness.native(vocabulary)?,
            }),
            Self::ModifyPT { power, toughness } => StaticPart::ModifyPT(NumericPair {
                power: power.native(vocabulary)?,
                toughness: toughness.native(vocabulary)?,
            }),
            Self::SwitchPT {} => StaticPart::SwitchPT,
            Self::Abilities {
                removal,
                granted_keywords,
            } => StaticPart::Abilities(ability_change(removal, granted_keywords)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum WordWire {
    Literal { value: Value },
    Chosen { slot: Value },
}
impl WordWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<CreatureTypeWord, String> {
        Ok(match self {
            Self::Literal { value } => {
                if !value.is_object() {
                    return Err("creature-type word ID must be a JSON object".into());
                }
                CreatureTypeWord::Literal(creature_types::id(value, vocabulary)?)
            }
            Self::Chosen { slot } => CreatureTypeWord::Chosen(creature_types::slot(slot)?),
        })
    }
    fn literal(self, vocabulary: &CreatureTypeVocabulary) -> Result<CreatureTypeId, String> {
        match self.native(vocabulary)? {
            CreatureTypeWord::Literal(value) => Ok(value),
            CreatureTypeWord::Chosen(_) => {
                Err("resolution-local text word in a live static".into())
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum Duration {
    NoStatedDuration,
    UntilEndOfTurn,
}
impl TryFrom<String> for Duration {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "NoStatedDuration" => Ok(Self::NoStatedDuration),
            "UntilEndOfTurn" => Ok(Self::UntilEndOfTurn),
            _ => Err(format!("unknown continuous effect duration {value}")),
        }
    }
}
impl From<Duration> for ResolvedDuration {
    fn from(value: Duration) -> Self {
        match value {
            Duration::NoStatedDuration => Self::NoStatedDuration,
            Duration::UntilEndOfTurn => Self::UntilEndOfTurn,
        }
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum Controller {
    Any,
    SourceController,
}
impl TryFrom<String> for Controller {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "Any" => Ok(Self::Any),
            "SourceController" => Ok(Self::SourceController),
            _ => Err(format!("unknown selector controller {value}")),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selector {
    required_supertypes: Vec<String>,
    forbidden_supertypes: Vec<String>,
    required_types: Vec<String>,
    forbidden_types: Vec<String>,
    #[serde(deserialize_with = "required_nullable")]
    creature_subtype: Option<Value>,
    #[serde(deserialize_with = "Option::deserialize")]
    land_subtype: Option<String>,
    color: ColorFilterWire,
    name: NameFilterWire,
    controller: Controller,
    exclude_source: bool,
}
impl Selector {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<PermanentSelector, String> {
        Ok(PermanentSelector {
            land_subtype: self
                .land_subtype
                .map(|label| {
                    LandType::from_wire_label(&label)
                        .ok_or_else(|| "unknown land subtype".to_owned())
                })
                .transpose()?,
            required_supertypes: supertypes(self.required_supertypes)?,
            forbidden_supertypes: supertypes(self.forbidden_supertypes)?,
            color: self.color.native()?,
            name: self.name.native(),
            required_types: card_types(self.required_types)?,
            forbidden_types: card_types(self.forbidden_types)?,
            creature_subtype: self
                .creature_subtype
                .map(|value| creature_types::id(value, vocabulary))
                .transpose()?,
            controller: match self.controller {
                Controller::Any => ControllerSelector::Any,
                Controller::SourceController => ControllerSelector::SourceController,
            },
            exclude_source: self.exclude_source,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Expression {
    Literal {
        value: RulesInteger,
    },
    Count {
        #[serde(deserialize_with = "object")]
        selector: Box<Selector>,
        multiplier: RulesInteger,
        offset: RulesInteger,
    },
}
impl Expression {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<NumericExpression, String> {
        Ok(match self {
            Self::Literal { value } => NumericExpression::Literal(value),
            Self::Count {
                selector,
                multiplier,
                offset,
            } => NumericExpression::Count {
                selector: (*selector).native(vocabulary)?,
                multiplier,
                offset,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum StaticAbility {
    Battlefield {
        id: u64,
        #[serde(deserialize_with = "object")]
        selector: Box<Selector>,
        parts: Vec<PartWire<StaticOperand, Expression>>,
    },
    CharacteristicDefining {
        id: u64,
        parts: Vec<DefiningPartWire>,
    },
}
impl StaticAbility {
    fn native(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<StaticContinuousAbility, String> {
        Ok(match self {
            Self::CharacteristicDefining { id, parts } => {
                StaticContinuousAbility::CharacteristicDefining {
                    id: StaticAbilityId(id),
                    parts: DefiningParts(
                        parts
                            .into_iter()
                            .map(|part| part.native(vocabulary))
                            .collect::<Result<_, _>>()?,
                    ),
                }
            }
            Self::Battlefield {
                id,
                selector,
                parts,
            } => StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(id),
                selector: (*selector).native(vocabulary)?,
                parts: StaticParts(
                    parts
                        .into_iter()
                        .map(|part| part.native(vocabulary))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum DefiningTypesWire {
    Literal { types: Vec<Value> },
    All {},
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum DefiningPartWire {
    Colors {
        colors: Vec<String>,
    },
    CreatureTypes {
        types: DefiningTypesWire,
    },
    PowerToughness {
        #[serde(deserialize_with = "required_nullable")]
        power: Option<Box<Expression>>,
        #[serde(deserialize_with = "required_nullable")]
        toughness: Option<Box<Expression>>,
    },
}
impl DefiningPartWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<DefiningPart, String> {
        Ok(match self {
            Self::Colors { colors } => DefiningPart::Colors(color_set(colors)?),
            Self::CreatureTypes { types } => DefiningPart::CreatureTypes(match types {
                DefiningTypesWire::All {} => DefiningCreatureTypes::All,
                DefiningTypesWire::Literal { types } => DefiningCreatureTypes::Literal(
                    subtype_set(types, vocabulary, "defining creature subtype")?,
                ),
            }),
            Self::PowerToughness { power, toughness } => DefiningPart::PowerToughness {
                power: power
                    .map(|p| (*p).native(vocabulary).map(Box::new))
                    .transpose()?,
                toughness: toughness
                    .map(|t| (*t).native(vocabulary).map(Box::new))
                    .transpose()?,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum PrintedStatWire {
    Number {
        value: RulesInteger,
    },
    Star {
        multiplier: RulesInteger,
        offset: RulesInteger,
    },
}
impl From<PrintedStatWire> for PrintedStat {
    fn from(value: PrintedStatWire) -> Self {
        match value {
            PrintedStatWire::Number { value } => Self::Number(value),
            PrintedStatWire::Star { multiplier, offset } => Self::Star { multiplier, offset },
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintedPt {
    power: PrintedStatWire,
    toughness: PrintedStatWire,
}
impl From<PrintedPt> for PrintedPowerToughness {
    fn from(value: PrintedPt) -> Self {
        Self {
            power: value.power.into(),
            toughness: value.toughness.into(),
        }
    }
}
impl Body {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ExecutableBody, String> {
        let target = match self.target {
            Target::None {} => BodyTarget::None,
            Target::SinglePermanent {} => BodyTarget::SinglePermanent,
            Target::SingleCreature {} => BodyTarget::SingleCreature,
            Target::SinglePlayer {} => BodyTarget::SinglePlayer,
            Target::SingleCreatureOrPlayer {} => BodyTarget::SingleCreatureOrPlayer,
            Target::SingleSpell { support_gate } => {
                BodyTarget::SingleSpell(match support_gate.as_str() {
                    "AnySpell" => SpellTargetFilter::AnySpell,
                    "CopyableInstructionSpell" => SpellTargetFilter::CopyableInstructionSpell,
                    _ => return Err(format!("unknown spell support gate {support_gate}")),
                })
            }
        };
        let instructions = self
            .instructions
            .into_iter()
            .map(|op| {
                Ok(match op {
                    Op::OptionalEffect { chooser } => Instruction::OptionalEffect {
                        chooser: chooser.into(),
                    },
                    Op::OfferManaPayment {
                        payer,
                        cost,
                        paid_destination,
                        declined_destination,
                    } => Instruction::OfferManaPayment {
                        payer: payer.into(),
                        cost: cost.native()?,
                        paid_destination: instruction_destination(paid_destination)?,
                        declined_destination: instruction_destination(declined_destination)?,
                    },
                    Op::BranchIfFalse {
                        condition,
                        destination,
                    } => Instruction::BranchIfFalse {
                        condition: condition.native(),
                        destination: instruction_destination(destination)?,
                    },
                    Op::Jump { destination } => Instruction::Jump {
                        destination: instruction_destination(destination)?,
                    },
                    Op::CreateTokens { creations } => Instruction::CreateTokens {
                        creations: creations
                            .into_iter()
                            .map(|creation| creation.native(vocabulary))
                            .collect::<Result<_, _>>()?,
                    },
                    Op::AdjustControllerLife { adjustment } => Instruction::AdjustControllerLife {
                        adjustment: adjustment.native(),
                    },
                    Op::AdjustTargetLife { adjustment } => Instruction::AdjustTargetLife {
                        adjustment: adjustment.native(),
                    },
                    Op::Scry { player, amount } => Instruction::Scry {
                        player: player.into(),
                        amount: amount.native()?,
                    },
                    Op::Surveil { player, amount } => Instruction::Surveil {
                        player: player.into(),
                        amount: amount.native()?,
                    },
                    Op::Mill { player, amount } => Instruction::Mill {
                        player: player.into(),
                        amount: amount.native()?,
                    },
                    Op::DrawController { amount } => Instruction::DrawController {
                        amount: amount.native()?,
                    },
                    Op::Sacrifice {
                        players,
                        amount,
                        filter,
                    } => Instruction::Sacrifice {
                        players: players.into(),
                        amount: amount.native()?,
                        filter: filter.native()?,
                    },
                    Op::Discard { players, amount } => Instruction::Discard {
                        players: players.into(),
                        amount: amount.native()?,
                    },
                    Op::ChooseCreatureType { into, excluding } => Instruction::ChooseCreatureType {
                        into: creature_types::slot(into)?,
                        excluding: subtype_set(excluding, vocabulary, "excluded creature type")?,
                    },
                    Op::Damage { amount } => Instruction::Damage { amount },
                    Op::CreatePreventionShield { amount } => {
                        Instruction::CreatePreventionShield { amount }
                    }
                    Op::CreateDamageMultiplier { factor } => {
                        Instruction::CreateDamageMultiplier { factor }
                    }
                    Op::ReturnToOwnersHand {} => Instruction::ReturnToOwnersHand,
                    Op::CounterSpell {} => Instruction::CounterSpell,
                    Op::CopySpell {} => Instruction::CopySpell,
                    Op::CopyTargetToSource {
                        duration,
                        power_toughness,
                    } => Instruction::CopyTargetToSource {
                        duration: duration.into(),
                        power_toughness: power_toughness.map(Into::into),
                    },
                    Op::PutPowerToughnessCounters { counters } => {
                        Instruction::PutPowerToughnessCounters {
                            counters: counters.into(),
                        }
                    }
                    Op::CreateContinuousEffect { parts, duration } => {
                        Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(
                                parts
                                    .into_iter()
                                    .map(|part| part.native(vocabulary))
                                    .collect::<Result<Vec<_>, _>>()?,
                            ),
                            duration: duration.into(),
                        }
                    }
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ExecutableBody::new(target, instructions))
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Kind {
    Permanent {
        #[serde(deserialize_with = "required_nullable")]
        printed_pt: Option<PrintedPt>,
        #[serde(deserialize_with = "required_nullable")]
        casting_cost: Option<Cost>,
    },
    Spell {
        #[serde(deserialize_with = "object")]
        casting_cost: Cost,
        #[serde(deserialize_with = "object")]
        body: Body,
    },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Output {
    Counted {
        mana_type: String,
        #[serde(deserialize_with = "object")]
        permanents: Selector,
    },
    Fixed {
        produced: BTreeMap<String, RulesInteger>,
    },
    ChooseColor {
        amount: RulesInteger,
        colors: Vec<String>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mana {
    id: u64,
    output: Output,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activated {
    id: u64,
    #[serde(deserialize_with = "object")]
    mana_cost: Cost,
    source_cost: String,
    #[serde(deserialize_with = "object")]
    body: Body,
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum CastFilter {
    AnySpell {},
    Noncreature {},
    InstantOrSorcery {},
    HasType { card_type: String },
}
impl CastFilter {
    fn native(self) -> Result<SpellCastFilter, String> {
        Ok(match self {
            Self::AnySpell {} => SpellCastFilter::AnySpell,
            Self::Noncreature {} => SpellCastFilter::Noncreature,
            Self::InstantOrSorcery {} => SpellCastFilter::InstantOrSorcery,
            Self::HasType { card_type: kind } => SpellCastFilter::HasType(card_type(&kind)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Condition {
    Scried { player: String },
    Surveilled { player: String },
    SelfCast {},
    SpellCast { player: String, filter: CastFilter },
    TokenCreated { player: String },
    LifeChanged { kind: String, player: String },
    Enters { object: String },
    LeavesBattlefield { object: String },
    Dies { object: String },
    BecomesTapped { object: String },
    Discarded { player: String },
    StepBegan { step: String, player: String },
}
fn object_relation(value: &str) -> Result<ObjectRelation, String> {
    match value {
        "SelfObject" => Ok(ObjectRelation::SelfObject),
        "AnyCreature" => Ok(ObjectRelation::AnyCreature),
        "AnotherCreature" => Ok(ObjectRelation::AnotherCreature),
        "ControlledCreature" => Ok(ObjectRelation::ControlledCreature),
        "AnotherControlledCreature" => Ok(ObjectRelation::AnotherControlledCreature),
        _ => Err(format!("unknown object relation {value}")),
    }
}
fn player_relation(value: &str) -> Result<PlayerRelation, String> {
    match value {
        "Controller" => Ok(PlayerRelation::Controller),
        "Opponent" => Ok(PlayerRelation::Opponent),
        "AnyParticipant" => Ok(PlayerRelation::AnyParticipant),
        _ => Err(format!("unknown player relation {value}")),
    }
}
impl Condition {
    fn native(self) -> Result<OrdinaryCondition, String> {
        Ok(match self {
            Self::SelfCast {} => OrdinaryCondition::SelfCast,
            Self::SpellCast { player, filter } => OrdinaryCondition::SpellCast {
                player: player_relation(&player)?,
                filter: filter.native()?,
            },
            Self::TokenCreated { player } => {
                OrdinaryCondition::TokenCreated(player_relation(&player)?)
            }
            Self::LifeChanged { kind, player } => OrdinaryCondition::LifeChanged {
                kind: match kind.as_str() {
                    "Gain" => LifeChangeKind::Gain,
                    "Loss" => LifeChangeKind::Loss,
                    _ => return Err(format!("unknown life change kind {kind}")),
                },
                player: player_relation(&player)?,
            },
            Self::Enters { object } => OrdinaryCondition::Enters(object_relation(&object)?),
            Self::LeavesBattlefield { object } => {
                OrdinaryCondition::LeavesBattlefield(object_relation(&object)?)
            }
            Self::Dies { object } => OrdinaryCondition::Dies(object_relation(&object)?),
            Self::BecomesTapped { object } => {
                OrdinaryCondition::BecomesTapped(object_relation(&object)?)
            }
            Self::Scried { player } => OrdinaryCondition::Scried(player_relation(&player)?),
            Self::Surveilled { player } => OrdinaryCondition::Surveilled(player_relation(&player)?),
            Self::Discarded { player } => OrdinaryCondition::Discarded(player_relation(&player)?),
            Self::StepBegan { step: name, player } => OrdinaryCondition::StepBegan {
                step: step(&name)?,
                player: player_relation(&player)?,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trigger {
    id: u64,
    condition: Condition,
    grouping: String,
    #[serde(deserialize_with = "object")]
    body: Body,
}
fn trigger_grouping(value: &str) -> Result<spellwake_engine::program::TriggerGrouping, String> {
    use spellwake_engine::program::TriggerGrouping;
    match value {
        "EachOccurrence" => Ok(TriggerGrouping::EachOccurrence),
        "OneOrMoreTogether" => Ok(TriggerGrouping::OneOrMoreTogether),
        "OneOrMoreByPlayer" => Ok(TriggerGrouping::OneOrMoreByPlayer),
        _ => Err(format!("unknown trigger grouping {value}")),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, tag = "tag")]
enum ZoneActionsWire {
    Any {},
    OneOf { actions: Vec<String> },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoneReplacementWire {
    id: u64,
    actions: ZoneActionsWire,
    objects: String,
    subject: String,
    destination: String,
    replace_with: String,
    choice: String,
}
fn zone_action(value: &str) -> Result<ZoneActionKind, String> {
    Ok(match value {
        "EffectSacrifice" => ZoneActionKind::EffectSacrifice,
        "EffectDiscard" => ZoneActionKind::EffectDiscard,
        "EffectMill" => ZoneActionKind::EffectMill,
        "EffectSurveil" => ZoneActionKind::EffectSurveil,
        "SacrificeCost" => ZoneActionKind::SacrificeCost,
        "CleanupDiscard" => ZoneActionKind::CleanupDiscard,
        "StateBased" => ZoneActionKind::StateBased,
        "Surveil" => ZoneActionKind::Surveil,
        "Mill" => ZoneActionKind::Mill,
        "Draw" => ZoneActionKind::Draw,
        "Countered" => ZoneActionKind::Countered,
        "ResolutionExit" => ZoneActionKind::ResolutionExit,
        "ReturnToHand" => ZoneActionKind::ReturnToHand,
        "LandPlay" => ZoneActionKind::LandPlay,
        "CastAnnouncement" => ZoneActionKind::CastAnnouncement,
        _ => return Err("unknown zone action".into()),
    })
}
fn public_destination(value: &str) -> Result<PublicZoneDestination, String> {
    match value {
        "Graveyard" => Ok(PublicZoneDestination::Graveyard),
        "Exile" => Ok(PublicZoneDestination::Exile),
        _ => Err("unknown public destination".into()),
    }
}
impl ZoneReplacementWire {
    fn native(self) -> Result<ZoneReplacementAbility, String> {
        let actions = match self.actions {
            ZoneActionsWire::Any {} => ZoneActionPredicate::Any,
            ZoneActionsWire::OneOf { actions } => {
                let count = actions.len();
                let values = actions
                    .iter()
                    .map(|action| zone_action(action))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                if values.len() != count {
                    return Err("duplicate zone action".into());
                }
                ZoneActionPredicate::OneOf(values)
            }
        };
        Ok(ZoneReplacementAbility {
            id: ZoneReplacementAbilityId(self.id),
            actions,
            objects: match self.objects.as_str() {
                "Card" => ZoneObjectKind::Card,
                "Token" => ZoneObjectKind::Token,
                "Either" => ZoneObjectKind::Either,
                _ => return Err("unknown zone object kind".into()),
            },
            subject: match self.subject.as_str() {
                "AnyObject" => ZoneReplacementSubject::AnyObject,
                "SourceSelf" => ZoneReplacementSubject::SourceSelf,
                _ => return Err("unknown zone subject".into()),
            },
            destination: public_destination(&self.destination)?,
            replace_with: public_destination(&self.replace_with)?,
            choice: match self.choice.as_str() {
                "Mandatory" => ZoneReplacementChoice::Mandatory,
                "OptionalAffectedPlayer" => ZoneReplacementChoice::OptionalAffectedPlayer,
                _ => return Err("unknown zone choice".into()),
            },
        })
    }
}
/// Complete generic native definition data; this grants no profile admission.
pub fn zone_replacements(values: &[ZoneReplacementAbility]) -> Value {
    json!(values.iter().map(|value|json!({"id":value.id.0,"actions":match &value.actions {ZoneActionPredicate::Any=>json!({"tag":"Any"}),ZoneActionPredicate::OneOf(actions)=>json!({"tag":"OneOf","actions":actions.iter().map(|action|format!("{action:?}")).collect::<Vec<_>>()})},"objects":format!("{:?}",value.objects),"subject":format!("{:?}",value.subject),"destination":format!("{:?}",value.destination),"replace_with":format!("{:?}",value.replace_with),"choice":format!("{:?}",value.choice)})).collect::<Vec<_>>())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    #[serde(default, deserialize_with = "objects")]
    zone_replacements: Vec<ZoneReplacementWire>,
    card_types: Vec<String>,
    land_subtypes: Vec<String>,
    creature_subtypes: Vec<Value>,
    names: spellwake_contracts::names::ObjectNames,
    supertypes: Vec<String>,
    kind: Kind,
    #[serde(deserialize_with = "Option::deserialize")]
    color_indicator: Option<Vec<String>>,
    #[serde(deserialize_with = "Option::deserialize")]
    effect_defined_colors: Option<Vec<String>>,
    #[serde(deserialize_with = "objects")]
    mana: Vec<Mana>,
    #[serde(deserialize_with = "objects")]
    activated: Vec<Activated>,
    #[serde(deserialize_with = "objects")]
    triggered: Vec<Trigger>,
    #[serde(deserialize_with = "required_nullable")]
    continuous: Option<StaticAbility>,
}
/// Decode strict authored syntax without admitting it into an engine owner.
/// Private constructor-rejection callers pass the result to the actual owner validator.
pub fn decode_for_owner(
    value: Value,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, String> {
    let raw: Definition = object_value(value)?;
    let kind = match raw.kind {
        Kind::Permanent {
            printed_pt,
            casting_cost,
        } => ProgramKind::Permanent(PermanentDefinition {
            printed_pt: printed_pt.map(Into::into),
            casting_cost: casting_cost.map(Cost::native).transpose()?,
        }),
        Kind::Spell { casting_cost, body } => ProgramKind::Spell(SpellDefinition {
            cost: casting_cost.native()?,
            body: body.native(vocabulary)?,
        }),
    };
    let mana = raw
        .mana
        .into_iter()
        .map(|ability| {
            let output = match ability.output {
                Output::Counted {
                    mana_type,
                    permanents,
                } => ManaOutput::Counted {
                    mana_type: mana(&mana_type)?,
                    permanents: permanents.native(vocabulary)?,
                },
                Output::Fixed { produced } => ManaOutput::Fixed(mana_map(produced)?),
                Output::ChooseColor { amount, colors } => {
                    let count = colors.len();
                    let colors = colors
                        .iter()
                        .map(|name| mana(name))
                        .collect::<Result<BTreeSet<_>, _>>()?;
                    if colors.len() != count {
                        return Err("duplicate mana color".into());
                    }
                    ManaOutput::ChooseColor { amount, colors }
                }
            };
            Ok(ManaAbility {
                id: ManaAbilityId(ability.id),
                output,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let activated = raw
        .activated
        .into_iter()
        .map(|ability| {
            Ok(ActivatedAbility {
                id: ActivatedAbilityId(ability.id),
                mana_cost: ability.mana_cost.native()?,
                source_cost: match ability.source_cost.as_str() {
                    "None" => SourceCost::None,
                    "Tap" => SourceCost::Tap,
                    "Sacrifice" => SourceCost::Sacrifice,
                    _ => return Err("unknown source cost".into()),
                },
                body: ability.body.native(vocabulary)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let triggered = raw
        .triggered
        .into_iter()
        .map(|ability| {
            Ok(TriggeredAbility {
                grouping: trigger_grouping(&ability.grouping)?,
                id: TriggeredAbilityId(ability.id),
                condition: ability.condition.native()?,
                body: ability.body.native(vocabulary)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let program = Program {
        zone_replacements: raw
            .zone_replacements
            .into_iter()
            .map(ZoneReplacementWire::native)
            .collect::<Result<Vec<_>, _>>()?,
        card_types: card_types(raw.card_types)?,
        land_subtypes: land_types(raw.land_subtypes)?,
        creature_subtypes: subtype_set(raw.creature_subtypes, vocabulary, "creature subtype")?,
        names: raw.names,
        supertypes: supertypes(raw.supertypes)?,
        kind,
        color_indicator: raw.color_indicator.map(color_set).transpose()?,
        effect_defined_colors: raw.effect_defined_colors.map(color_set).transpose()?,
        mana,
        activated,
        triggered,
        continuous: raw
            .continuous
            .map(|ability| ability.native(vocabulary))
            .transpose()?,
    };
    Ok(program)
}

/// Strict syntax plus selected-definition semantic admission for public callers.
pub fn decode(value: Value, vocabulary: &CreatureTypeVocabulary) -> Result<Program, String> {
    let program = decode_for_owner(value, vocabulary)?;
    program
        .validate(vocabulary)
        .map_err(|error| format!("native definition rejected: {error:?}"))?;
    Ok(program)
}

pub fn pt(value: &PowerToughness) -> Value {
    json!({"power":value.power,"toughness":value.toughness})
}
pub fn printed_pt(value: &PrintedPowerToughness) -> Value {
    fn stat(value: &PrintedStat) -> Value {
        match value {
            PrintedStat::Number(value) => json!({"tag":"Number","value":value}),
            PrintedStat::Star { multiplier, offset } => {
                json!({"tag":"Star","multiplier":multiplier,"offset":offset})
            }
        }
    }
    json!({"power":stat(&value.power),"toughness":stat(&value.toughness)})
}
pub fn counters(value: &PowerToughnessCounters) -> Value {
    json!({"plus_one":value.plus_one,"minus_one":value.minus_one})
}
pub fn cost(value: &ManaCost) -> Value {
    json!({"generic":value.generic,"required":value.required.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
}
fn type_operation<T: std::fmt::Debug>(value: &TypeOperation<T>) -> Value {
    match value {
        TypeOperation::Keep => json!({"tag":"Keep"}),
        TypeOperation::Add(types) => {
            json!({"tag":"Add","types":types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
        }
        TypeOperation::Remove(types) => {
            json!({"tag":"Remove","types":types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
        }
        TypeOperation::Set(types) => {
            json!({"tag":"Set","types":types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
        }
    }
}
fn subtype_values(values: &BTreeSet<CreatureTypeId>) -> Value {
    json!(
        values
            .iter()
            .map(|value| projection::creature_type_id(*value))
            .collect::<Vec<_>>()
    )
}
fn literal_subtypes(values: &BTreeSet<CreatureTypeId>) -> Value {
    json!({"tag":"Literal","types":subtype_values(values)})
}
fn subtype_operation<T, Member>(
    value: &SubtypeOperation<T, Member>,
    operand: impl Fn(&T) -> Value,
    members: impl Fn(&BTreeSet<Member>) -> Value,
) -> Value {
    match value {
        SubtypeOperation::Keep => json!({"tag":"Keep"}),
        SubtypeOperation::Add(value) => json!({"tag":"Add","value":operand(value)}),
        SubtypeOperation::Remove(types) => json!({"tag":"Remove","types":members(types)}),
        SubtypeOperation::Set(value) => json!({"tag":"Set","value":operand(value)}),
    }
}
fn instruction_operand(value: &InstructionSubtypeOperand) -> Value {
    match value {
        InstructionSubtypeOperand::Literal(types) => literal_subtypes(types),
        InstructionSubtypeOperand::Chosen(slot) => {
            json!({"tag":"Chosen","slot":slot.ordinal().to_string()})
        }
    }
}
fn text_word(value: CreatureTypeWord) -> Value {
    match value {
        CreatureTypeWord::Literal(value) => {
            json!({"tag":"Literal","value":projection::creature_type_id(value)})
        }
        CreatureTypeWord::Chosen(slot) => json!({"tag":"Chosen","slot":slot.ordinal().to_string()}),
    }
}
fn text_change(value: CreatureTypeTextChange) -> Value {
    json!({"tag":"Text","from":text_word(CreatureTypeWord::Literal(value.from)),"to":text_word(CreatureTypeWord::Literal(value.to))})
}
fn colors(value: &BTreeSet<Color>) -> Value {
    json!(
        value
            .iter()
            .map(|color| format!("{color:?}"))
            .collect::<Vec<_>>()
    )
}
fn color_literal(value: &BTreeSet<Color>) -> Value {
    json!({"tag":"Literal","colors":colors(value)})
}
fn color_static(value: &StaticColorOperand) -> Value {
    match value {
        StaticColorOperand::Literal(value) => color_literal(value),
        StaticColorOperand::SourceCurrentColors => json!({"tag":"SourceCurrentColors"}),
    }
}
fn color_part<T>(value: &ColorOperation<T>, operand: impl Fn(&T) -> Value) -> Value {
    let change = match value {
        ColorOperation::Set(value) => json!({"tag":"Set","value":operand(value)}),
        ColorOperation::Add(value) => json!({"tag":"Add","value":operand(value)}),
        ColorOperation::Remove(value) => json!({"tag":"Remove","value":operand(value)}),
    };
    json!({"tag":"Color","change":change})
}
fn color_filter(value: ColorFilter) -> Value {
    match value {
        ColorFilter::Any => json!({"tag":"Any"}),
        ColorFilter::Has(color) => json!({"tag":"Has","color":format!("{color:?}")}),
        ColorFilter::Colorless => json!({"tag":"Colorless"}),
        ColorFilter::Monocolored => json!({"tag":"Monocolored"}),
        ColorFilter::Multicolored => json!({"tag":"Multicolored"}),
    }
}
fn land_literal(values: &BTreeSet<LandType>) -> Value {
    json!({"tag":"Literal","types":land_type_values(values)})
}
fn static_land_operand(value: &StaticLandSubtypeOperand) -> Value {
    match value {
        StaticLandSubtypeOperand::Literal(values) => land_literal(values),
        StaticLandSubtypeOperand::SourceCurrentLandSubtypes => {
            json!({"tag":"SourceCurrentLandSubtypes"})
        }
    }
}
fn static_operand(value: &StaticSubtypeOperand) -> Value {
    match value {
        StaticSubtypeOperand::Literal(types) => literal_subtypes(types),
        StaticSubtypeOperand::SourceCurrentSubtypes => json!({"tag":"SourceCurrentSubtypes"}),
    }
}
fn type_part(
    supertypes: &TypeOperation<Supertype>,
    types: &TypeOperation,
    subtypes: Value,
    land_subtypes: Value,
) -> Value {
    json!({"tag":"Type","supertypes":type_operation(supertypes),"types":type_operation(types),"subtypes":subtypes,"land_subtypes":land_subtypes})
}
fn ability_part(value: &AbilityChange) -> Value {
    let removal = value.removal.map(|removal| match removal {
        AbilityRemoval::All => "All",
        AbilityRemoval::NonMana => "NonMana",
    });
    json!({"tag":"Abilities","removal":removal,"granted_keywords":value.granted_keywords.iter().map(|keyword|format!("{keyword:?}")).collect::<Vec<_>>()})
}
/// Current authored parts syntax, retaining body-local chosen operands.
pub fn parts_spec(value: &ResolvedPartsSpec) -> Value {
    json!(
        value
            .0
            .iter()
            .map(|part| match part {
                ResolvedPartSpec::Names(change) => name_part(change, name_literal),
                ResolvedPartSpec::Color(change) => color_part(change, color_literal),
                ResolvedPartSpec::Text(change) =>
                    json!({"tag":"Text","from":text_word(change.from),"to":text_word(change.to)}),
                ResolvedPartSpec::Control(recipient) =>
                    json!({"tag":"Control","recipient":format!("{recipient:?}")}),
                ResolvedPartSpec::Type(change) => type_part(
                    &change.supertypes,
                    &change.types,
                    subtype_operation(&change.subtypes, instruction_operand, subtype_values),
                    subtype_operation(&change.land_subtypes, land_literal, land_type_values),
                ),
                ResolvedPartSpec::SetPT(value) => {
                    json!({"tag":"SetPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPartSpec::ModifyPT(value) => {
                    json!({"tag":"ModifyPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPartSpec::SwitchPT => json!({"tag":"SwitchPT"}),
                ResolvedPartSpec::Abilities(value) => ability_part(value),
            })
            .collect::<Vec<_>>()
    )
}
/// Materialized parts use the same current syntax with only literal operands.
pub fn resolved_parts(value: &ResolvedParts) -> Value {
    json!(
        value
            .0
            .iter()
            .map(|part| match part {
                ResolvedPart::Copy(program) => json!({"tag":"Copy","program":view(program)}),
                ResolvedPart::Names(change) => name_part(change, name_literal),
                ResolvedPart::Color(change) => color_part(change, color_literal),
                ResolvedPart::Text(change) => text_change(*change),
                ResolvedPart::Control(controller) =>
                    json!({"tag":"Control","controller":controller.0}),
                ResolvedPart::Type(change) => type_part(
                    &change.supertypes,
                    &change.types,
                    subtype_operation(&change.subtypes, literal_subtypes, subtype_values),
                    subtype_operation(&change.land_subtypes, land_literal, land_type_values),
                ),
                ResolvedPart::SetPT(value) => {
                    json!({"tag":"SetPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPart::ModifyPT(value) => {
                    json!({"tag":"ModifyPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPart::SwitchPT => json!({"tag":"SwitchPT"}),
                ResolvedPart::Abilities(value) => ability_part(value),
            })
            .collect::<Vec<_>>()
    )
}
/// The explicit current duration string; no omitted-duration default exists.
pub fn duration(value: ResolvedDuration) -> &'static str {
    match value {
        ResolvedDuration::NoStatedDuration => "NoStatedDuration",
        ResolvedDuration::UntilEndOfTurn => "UntilEndOfTurn",
    }
}
fn name_part<T>(value: &NameOperation<T>, operand: impl Fn(&T) -> Value) -> Value {
    let change = match value {
        NameOperation::Set(names) => json!({"tag":"Set","operand":operand(names)}),
        NameOperation::Add(names) => json!({"tag":"Add","operand":operand(names)}),
    };
    json!({"tag":"Names","change":change})
}
fn name_literal(value: &ObjectNames) -> Value {
    json!({"tag":"Literal","names":value})
}
fn name_static(value: &StaticNameOperand) -> Value {
    match value {
        StaticNameOperand::Literal(names) => name_literal(names),
        StaticNameOperand::SourceCurrentNames => json!({"tag":"SourceCurrentNames"}),
    }
}
fn selector(value: &PermanentSelector) -> Value {
    json!({
        "required_supertypes":value.required_supertypes.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "forbidden_supertypes":value.forbidden_supertypes.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "required_types":value.required_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "forbidden_types":value.forbidden_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "creature_subtype":value.creature_subtype.map(projection::creature_type_id),
        "land_subtype":value.land_subtype.map(LandType::wire_label),
        "color":color_filter(value.color),
        "name":match &value.name {
            NameFilter::Any => json!({"tag":"Any"}),
            NameFilter::Named(name) => json!({"tag":"Named","name":name}),
            NameFilter::SharesWithSource => json!({"tag":"SharesWithSource"}),
            NameFilter::DifferentFromSource => json!({"tag":"DifferentFromSource"}),
        },
        "controller":match value.controller {
            ControllerSelector::Any => "Any",
            ControllerSelector::SourceController => "SourceController",
        },
        "exclude_source":value.exclude_source,
    })
}
fn expression(value: &NumericExpression) -> Value {
    match value {
        NumericExpression::Literal(value) => json!({"tag":"Literal","value":value}),
        NumericExpression::Count {
            selector: value,
            multiplier,
            offset,
        } => {
            json!({"tag":"Count","selector":selector(value),"multiplier":multiplier,"offset":offset})
        }
    }
}
/// The current singleton static definition, with exact variant-specific fields.
pub fn static_ability(value: &StaticContinuousAbility) -> Value {
    let (id, target, parts) = match value {
        StaticContinuousAbility::Battlefield {
            id,
            selector,
            parts,
        } => (id, selector, parts),
        StaticContinuousAbility::CharacteristicDefining { id, parts } => {
            let parts: Vec<_> = parts.0.iter().map(|part| match part {
                DefiningPart::Colors(value) => json!({"tag":"Colors","colors":colors(value)}),
                DefiningPart::CreatureTypes(value) => json!({"tag":"CreatureTypes","types":match value {
                    DefiningCreatureTypes::All => json!({"tag":"All"}),
                    DefiningCreatureTypes::Literal(types) => literal_subtypes(types),
                }}),
                DefiningPart::PowerToughness { power, toughness } => json!({"tag":"PowerToughness","power":power.as_ref().map(|value|expression(value)),"toughness":toughness.as_ref().map(|value|expression(value))}),
            }).collect();
            return json!({"tag":"CharacteristicDefining","id":id.0,"parts":parts});
        }
    };
    let parts = parts.0.iter().map(|part| match part {
        StaticPart::ControlSourceController => json!({"tag":"ControlSourceController"}),
        StaticPart::Names(change) => name_part(change, name_static),
        StaticPart::Color(change) => color_part(change, color_static),
        StaticPart::Text(change) => text_change(*change),
        StaticPart::Type(change) => type_part(
                    &change.supertypes,
            &change.types,
            subtype_operation(&change.subtypes, static_operand, subtype_values),
            subtype_operation(&change.land_subtypes, static_land_operand, land_type_values),
        ),
        StaticPart::SetPT(value) => {
            json!({"tag":"SetPT","power":expression(&value.power),"toughness":expression(&value.toughness)})
        }
        StaticPart::ModifyPT(value) => {
            json!({"tag":"ModifyPT","power":expression(&value.power),"toughness":expression(&value.toughness)})
        }
        StaticPart::SwitchPT => json!({"tag":"SwitchPT"}),
        StaticPart::Abilities(value) => ability_part(value),
    }).collect::<Vec<_>>();
    json!({"tag":"Battlefield","id":id.0,"selector":selector(target),"parts":parts})
}
pub fn instruction_amount(value: &InstructionAmount) -> Value {
    match value {
        InstructionAmount::Literal(value) => json!({"tag":"Literal","value":value}),
        InstructionAmount::MatchedOccurrences => json!({"tag":"MatchedOccurrences"}),
        InstructionAmount::ControlledPermanentsOfChosenType { chosen } => {
            json!({"tag":"ControlledPermanentsOfChosenType","chosen":chosen.ordinal().to_string()})
        }
    }
}
fn affected_players(value: AffectedPlayers) -> Value {
    match value {
        AffectedPlayers::One(player) => json!({"tag":"One","player":format!("{player:?}")}),
        AffectedPlayers::EachPlayer => json!({"tag":"EachPlayer"}),
    }
}
fn permanent_type_filter(value: &PermanentTypeFilter) -> Value {
    json!({
        "required_types":value.required_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "forbidden_types":value.forbidden_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()
    })
}
fn life_adjustment(value: &LifeAdjustment) -> Value {
    match value {
        LifeAdjustment::Gain(amount) => json!({"tag":"Gain","amount":amount}),
        LifeAdjustment::Lose(amount) => json!({"tag":"Lose","amount":amount}),
        LifeAdjustment::Set(total) => json!({"tag":"Set","total":total}),
    }
}
pub fn token_definition(value: &TokenDefinition) -> Value {
    let name = match value.name() {
        TokenName::Specified(name) => json!({"tag":"Specified","name":name}),
        TokenName::Default { ordered_subtypes } => {
            json!({"tag":"Default","ordered_subtypes":ordered_subtypes.iter().map(|word|match word {
            TokenSubtype::Creature(value) => json!({"family":"Creature","type":projection::creature_type_id(*value)}),
            TokenSubtype::Land(value) => json!({"family":"Land","type":value.wire_label()}),
        }).collect::<Vec<_>>()})
        }
    };
    json!({"program":view(value.program()),"name":name})
}
/// Shared exact resolution-condition encoding for bodies and real control records.
pub fn resolution_condition(value: &ResolutionCondition) -> Value {
    fn operand(value: &ConditionValue) -> Value {
        match value {
            ConditionValue::Literal(value) => json!({"tag":"Literal","value":value}),
            ConditionValue::ControllerLife => json!({"tag":"ControllerLife"}),
            ConditionValue::MatchedOccurrences => json!({"tag":"MatchedOccurrences"}),
        }
    }
    json!({"left":operand(&value.left),"comparison":format!("{:?}",value.comparison),"right":operand(&value.right)})
}
pub fn body(value: &ExecutableBody) -> Value {
    let target = match value.target {
        BodyTarget::None => json!({"tag":"None"}),
        BodyTarget::SinglePermanent => json!({"tag":"SinglePermanent"}),
        BodyTarget::SingleCreature => json!({"tag":"SingleCreature"}),
        BodyTarget::SinglePlayer => json!({"tag":"SinglePlayer"}),
        BodyTarget::SingleCreatureOrPlayer => json!({"tag":"SingleCreatureOrPlayer"}),
        BodyTarget::SingleSpell(gate) => {
            json!({"tag":"SingleSpell","support_gate":format!("{gate:?}")})
        }
    };
    let instructions = value
        .instructions
        .iter()
        .map(|instruction| match instruction {
            Instruction::Scry { player, amount } => json!({"op":"Scry","player":format!("{player:?}"),"amount":instruction_amount(amount)}),
            Instruction::Surveil { player, amount } => json!({"op":"Surveil","player":format!("{player:?}"),"amount":instruction_amount(amount)}),
            Instruction::Mill { player, amount } => json!({"op":"Mill","player":format!("{player:?}"),"amount":instruction_amount(amount)}),
            Instruction::OptionalEffect { chooser } => json!({"op":"OptionalEffect","chooser":format!("{chooser:?}")}),
            Instruction::OfferManaPayment { payer, cost: quoted, paid_destination, declined_destination } => json!({"op":"OfferManaPayment","payer":format!("{payer:?}"),"cost":cost(quoted),"paid_destination":paid_destination.to_string(),"declined_destination":declined_destination.to_string()}),
            Instruction::BranchIfFalse { condition, destination } => json!({"op":"BranchIfFalse","condition":resolution_condition(condition),"destination":destination.to_string()}),
            Instruction::Jump { destination } => json!({"op":"Jump","destination":destination.to_string()}),
            Instruction::CreateTokens { creations } => json!({"op":"CreateTokens","creations":creations.iter().map(|creation|json!({"definition":token_definition(&creation.definition),"amount":instruction_amount(&creation.amount),"recipients":match creation.recipients {TokenRecipients::Controller=>"Controller",TokenRecipients::EachPlayer=>"EachPlayer",TokenRecipients::EachOpponent=>"EachOpponent"}})).collect::<Vec<_>>()}),
            Instruction::AdjustControllerLife { adjustment } => {
                json!({"op":"AdjustControllerLife","adjustment":life_adjustment(adjustment)})
            }
            Instruction::AdjustTargetLife { adjustment } => json!({"op":"AdjustTargetLife","adjustment":life_adjustment(adjustment)}),
            Instruction::DrawController { amount } => {
                json!({"op":"DrawController","amount":instruction_amount(amount)})
            }
            Instruction::Sacrifice { players, amount, filter } => json!({"op":"Sacrifice","players":affected_players(*players),"amount":instruction_amount(amount),"filter":permanent_type_filter(filter)}),
            Instruction::Discard { players, amount } => json!({"op":"Discard","players":affected_players(*players),"amount":instruction_amount(amount)}),
            Instruction::ChooseCreatureType { into, excluding } => {
                json!({"op":"ChooseCreatureType","into":into.ordinal().to_string(),"excluding":excluding.iter().map(|value|projection::creature_type_id(*value)).collect::<Vec<_>>()})
            }
            Instruction::Damage { amount } => json!({"op":"Damage","amount":amount}),
            Instruction::CreatePreventionShield { amount } => {
                json!({"op":"CreatePreventionShield","amount":amount})
            }
            Instruction::CreateDamageMultiplier { factor } => {
                json!({"op":"CreateDamageMultiplier","factor":factor})
            }
            Instruction::ReturnToOwnersHand => json!({"op":"ReturnToOwnersHand"}),
            Instruction::CounterSpell => json!({"op":"CounterSpell"}),
            Instruction::CopySpell => json!({"op":"CopySpell"}),
            Instruction::CopyTargetToSource { duration: value, power_toughness } => {
                json!({"op":"CopyTargetToSource","duration":duration(*value),"power_toughness":power_toughness.as_ref().map(pt)})
            }
            Instruction::CreateContinuousEffect { parts, duration: value } => {
                json!({"op":"CreateContinuousEffect","parts":parts_spec(parts),"duration":duration(*value)})
            }
            Instruction::PutPowerToughnessCounters { counters: value } => {
                json!({"op":"PutPowerToughnessCounters","counters":counters(value)})
            }
        })
        .collect::<Vec<_>>();
    json!({"target":target,"instructions":instructions})
}
pub fn condition(value: OrdinaryCondition) -> Value {
    match value {
        OrdinaryCondition::Scried(player) => json!({"tag":"Scried","player":format!("{player:?}")}),
        OrdinaryCondition::Surveilled(player) => {
            json!({"tag":"Surveilled","player":format!("{player:?}")})
        }
        OrdinaryCondition::SelfCast => json!({"tag":"SelfCast"}),
        OrdinaryCondition::SpellCast { player, filter } => {
            let filter = match filter {
                SpellCastFilter::AnySpell => json!({"tag":"AnySpell"}),
                SpellCastFilter::Noncreature => json!({"tag":"Noncreature"}),
                SpellCastFilter::InstantOrSorcery => json!({"tag":"InstantOrSorcery"}),
                SpellCastFilter::HasType(kind) => {
                    json!({"tag":"HasType","card_type":format!("{kind:?}")})
                }
            };
            json!({"tag":"SpellCast","player":format!("{player:?}"),"filter":filter})
        }
        OrdinaryCondition::TokenCreated(player) => {
            json!({"tag":"TokenCreated","player":format!("{player:?}")})
        }
        OrdinaryCondition::LifeChanged { kind, player } => {
            json!({"tag":"LifeChanged","kind":format!("{kind:?}"),"player":format!("{player:?}")})
        }
        OrdinaryCondition::Enters(object) => json!({"tag":"Enters","object":format!("{object:?}")}),
        OrdinaryCondition::LeavesBattlefield(object) => {
            json!({"tag":"LeavesBattlefield","object":format!("{object:?}")})
        }
        OrdinaryCondition::Dies(object) => json!({"tag":"Dies","object":format!("{object:?}")}),
        OrdinaryCondition::BecomesTapped(object) => {
            json!({"tag":"BecomesTapped","object":format!("{object:?}")})
        }
        OrdinaryCondition::Discarded(player) => {
            json!({"tag":"Discarded","player":format!("{player:?}")})
        }
        OrdinaryCondition::StepBegan { step, player } => {
            json!({"tag":"StepBegan","step":format!("{step:?}"),"player":format!("{player:?}")})
        }
    }
}
fn activated(abilities: &[ActivatedAbility]) -> Value {
    json!(abilities.iter().map(|ability| json!({"id":ability.id.0,"mana_cost":cost(&ability.mana_cost),"source_cost":format!("{:?}",ability.source_cost),"body":body(&ability.body)})).collect::<Vec<_>>())
}
/// Encode the admitted trigger collection for definitions, current text and copies.
pub fn triggered_abilities(abilities: &[TriggeredAbility]) -> Value {
    use spellwake_engine::program::TriggerGrouping;
    json!(abilities.iter().map(|ability| json!({"id":ability.id.0,"condition":condition(ability.condition),"grouping":match ability.grouping {TriggerGrouping::EachOccurrence=>"EachOccurrence",TriggerGrouping::OneOrMoreTogether=>"OneOrMoreTogether",TriggerGrouping::OneOrMoreByPlayer=>"OneOrMoreByPlayer"},"body":body(&ability.body)})).collect::<Vec<_>>())
}
fn mana_abilities(abilities: &[ManaAbility]) -> Value {
    json!(abilities.iter().map(|ability| json!({"id":ability.id.0,"output":match &ability.output {
        ManaOutput::Counted { mana_type, permanents } => json!({"tag":"Counted","mana_type":format!("{mana_type:?}"),"permanents":selector(permanents)}),
        ManaOutput::Fixed(values) => json!({"tag":"Fixed","produced":values.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()}),
        ManaOutput::ChooseColor { amount, colors } => json!({"tag":"ChooseColor","amount":amount,"colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
    }})).collect::<Vec<_>>())
}
pub fn printed_text(value: &PrintedText) -> Value {
    let mut result = json!({"mana":mana_abilities(&value.mana),"activated":activated(&value.activated),"triggered":triggered_abilities(&value.triggered),"continuous":value.continuous.as_ref().map(static_ability)});
    if !value.zone_replacements.is_empty() {
        result["zone_replacements"] = zone_replacements(&value.zone_replacements);
    }
    result
}
pub fn view(value: &Program) -> Value {
    let kind = match &value.kind {
        ProgramKind::Permanent(value) => {
            json!({"tag":"Permanent","printed_pt":value.printed_pt.as_ref().map(printed_pt),"casting_cost":value.casting_cost.as_ref().map(cost)})
        }
        ProgramKind::Spell(value) => {
            json!({"tag":"Spell","casting_cost":cost(&value.cost),"body":body(&value.body)})
        }
    };
    let mut result = json!({"land_subtypes":land_type_values(&value.land_subtypes),"card_types":value.card_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"creature_subtypes":subtype_values(&value.creature_subtypes),"names":value.names,"supertypes":value.supertypes.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"kind":kind,"color_indicator":value.color_indicator.as_ref().map(colors),"effect_defined_colors":value.effect_defined_colors.as_ref().map(colors),"mana":mana_abilities(&value.mana),
        "activated":activated(&value.activated),
        "triggered":triggered_abilities(&value.triggered),
        "continuous":value.continuous.as_ref().map(static_ability)});
    if !value.zone_replacements.is_empty() {
        result["zone_replacements"] = zone_replacements(&value.zone_replacements);
    }
    result
}

/// Decode one offered timestamp member coordinate; legal membership/permutation
/// belongs to the current native frame, not this JSON scalar parser.
pub fn token_member(value: &Value) -> Result<spellwake_engine::token::TokenBatchMember, String> {
    let value = value
        .as_u64()
        .ok_or("timestamp member must be an unsigned native numeric ordinal")?;
    usize::try_from(value)
        .map(spellwake_engine::token::TokenBatchMember::new)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> Value {
        json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Instant"],"creature_subtypes":[],"kind":{"tag":"Spell","casting_cost":{"generic":"0","required":{}},"body":{"target":{"tag":"None"},"instructions":[{"op":"ChooseCreatureType","into":"0","excluding":[]},{"op":"DrawController","amount":{"tag":"ControlledPermanentsOfChosenType","chosen":"0"}}]}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null})
    }
    #[test]
    fn current_program_round_trip_preserves_choice_producer_and_tagged_amount() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let value = definition();
        let decoded = decode(value.clone(), &vocabulary).unwrap();
        assert_eq!(view(&decoded), value);
        let mut invalid = value.clone();
        invalid["kind"]["body"]["instructions"][1]["amount"] = json!("1");
        assert!(decode_for_owner(invalid, &vocabulary).is_err());
        let mut invalid = value;
        invalid["kind"]["body"]["instructions"][0]["implicit_chooser"] = json!("A");
        assert!(decode_for_owner(invalid, &vocabulary).is_err());
    }
    fn optional_payment_definition(instructions: Value) -> Value {
        let mut value = definition();
        value["kind"]["body"]["instructions"] = instructions;
        value
    }
    fn payment_instruction() -> Value {
        json!({"op":"OfferManaPayment","payer":"Controller","cost":{"generic":"0","required":{}},"paid_destination":"1","declined_destination":"1"})
    }
    #[test]
    fn optional_payment_round_trip_preserves_each_role_and_the_existing_exact_cost_codec() {
        use spellwake_engine::program::TriggerGrouping;
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let exact =
            RulesInteger::try_from("900719925474099312345678901234567890".to_owned()).unwrap();
        let quoted = ManaCost {
            generic: exact.clone(),
            required: ManaType::ALL
                .into_iter()
                .map(|kind| (kind, exact.clone()))
                .collect(),
        };
        for role in [
            ResolvingPlayer::Controller,
            ResolvingPlayer::UniqueRelatedPlayer,
            ResolvingPlayer::TargetSpellController,
            ResolvingPlayer::TargetPlayer,
        ] {
            let target = match role {
                ResolvingPlayer::TargetSpellController => {
                    BodyTarget::SingleSpell(SpellTargetFilter::AnySpell)
                }
                ResolvingPlayer::TargetPlayer => BodyTarget::SinglePlayer,
                ResolvingPlayer::Controller | ResolvingPlayer::UniqueRelatedPlayer => {
                    BodyTarget::None
                }
            };
            let primitive = match role {
                ResolvingPlayer::TargetSpellController => Instruction::CounterSpell,
                ResolvingPlayer::TargetPlayer => Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
                ResolvingPlayer::Controller | ResolvingPlayer::UniqueRelatedPlayer => {
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    }
                }
            };
            for instructions in [
                vec![Instruction::OptionalEffect { chooser: role }, primitive],
                vec![Instruction::OfferManaPayment {
                    payer: role,
                    cost: quoted.clone(),
                    paid_destination: 1,
                    declined_destination: 1,
                }],
            ] {
                let body = ExecutableBody::new(target, instructions);
                let (program, body_pointer) = if role == ResolvingPlayer::UniqueRelatedPlayer {
                    let mut program = Program::artifact(None, vec![]);
                    program.triggered.push(TriggeredAbility {
                        id: TriggeredAbilityId(1),
                        condition: OrdinaryCondition::SpellCast {
                            player: PlayerRelation::AnyParticipant,
                            filter: SpellCastFilter::AnySpell,
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                        body,
                    });
                    (program, "/triggered/0/body")
                } else {
                    (Program::instant(ManaCost::free(), body), "/kind/body")
                };
                let raw = view(&program);
                let first = &raw.pointer(body_pointer).unwrap()["instructions"][0];
                let role_field = if first["op"] == "OptionalEffect" {
                    "chooser"
                } else {
                    assert_eq!(first["cost"], cost(&quoted));
                    assert_eq!(first["cost"]["generic"], String::from(exact.clone()));
                    assert_eq!(first["paid_destination"], "1");
                    assert_eq!(first["declined_destination"], "1");
                    "payer"
                };
                assert_eq!(first[role_field], format!("{role:?}"));
                assert_eq!(decode(raw.clone(), &vocabulary).unwrap(), program);
                assert_eq!(view(&decode(raw.clone(), &vocabulary).unwrap()), raw);
            }
        }
    }
    #[test]
    fn optional_payment_roles_are_exact_strings_with_required_instruction_fields() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for (instruction, field) in [
            (
                json!({"op":"OptionalEffect","chooser":"Controller"}),
                "chooser",
            ),
            (payment_instruction(), "payer"),
        ] {
            for role in [
                json!(null),
                json!(0),
                json!("controller"),
                json!("RelatedPlayer"),
                json!({"Controller":null}),
                json!({"tag":"Controller"}),
                json!(["Controller"]),
            ] {
                let mut wrong = instruction.clone();
                wrong[field] = role;
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err()
                );
            }
            for missing in instruction.as_object().unwrap().keys() {
                let mut wrong = instruction.clone();
                wrong.as_object_mut().unwrap().remove(missing);
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err(),
                    "{missing}"
                );
            }
            let mut wrong = instruction;
            wrong["implicit_player"] = json!("A");
            assert!(
                decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary).is_err()
            );
        }
    }
    #[test]
    fn payment_cost_and_destinations_require_canonical_current_syntax() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for field in ["paid_destination", "declined_destination"] {
            for destination in [
                json!(1),
                json!(-1),
                json!("+1"),
                json!("01"),
                json!("-0"),
                json!(" 1"),
                json!("1.0"),
                json!("1e0"),
                json!(null),
                json!(format!("{}0", usize::MAX)),
            ] {
                let mut wrong = payment_instruction();
                wrong[field] = destination;
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err(),
                    "{field}"
                );
            }
        }
        for quoted in [
            json!(null),
            json!([]),
            json!(["0", {}]),
            json!({}),
            json!({"generic":"0"}),
            json!({"required":{}}),
            json!({"generic":0,"required":{}}),
            json!({"generic":"01","required":{}}),
            json!({"generic":"0","required":{"Blue":1}}),
            json!({"generic":"0","required":{"Blue":"+1"}}),
            json!({"generic":"0","required":{"Colorless":"1"}}),
            json!({"generic":"0","required":{"Blue":"1"},"optional":true}),
        ] {
            let mut wrong = payment_instruction();
            wrong["cost"] = quoted;
            assert!(
                decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary).is_err()
            );
        }
    }
    #[test]
    fn optional_payment_syntax_defers_wrapper_role_cost_and_flow_domains_to_native_admission() {
        use spellwake_engine::program::DefinitionError;
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let optional = json!({"op":"OptionalEffect","chooser":"Controller"});
        for (instructions, expected) in [
            (json!([optional]), DefinitionError::MissingOptionalEffect),
            (
                json!([optional, {"op":"Jump","destination":"2"}]),
                DefinitionError::UnsupportedOptionalEffect,
            ),
            (
                json!([optional, payment_instruction()]),
                DefinitionError::UnsupportedOptionalEffect,
            ),
            (
                json!([{"op":"OptionalEffect","chooser":"UniqueRelatedPlayer"},{"op":"AdjustControllerLife","adjustment":{"tag":"Gain","amount":"1"}}]),
                DefinitionError::UnboundResolvingPlayer,
            ),
            (
                json!([{"op":"OptionalEffect","chooser":"TargetSpellController"},{"op":"AdjustControllerLife","adjustment":{"tag":"Gain","amount":"1"}}]),
                DefinitionError::ResolvingPlayerTarget,
            ),
        ] {
            let raw = optional_payment_definition(instructions);
            let program = decode_for_owner(raw.clone(), &vocabulary).unwrap();
            assert_eq!(program.validate(&vocabulary), Err(expected));
            assert!(decode(raw, &vocabulary).is_err());
        }
        for (field, value, expected) in [
            (
                "payer",
                json!("UniqueRelatedPlayer"),
                DefinitionError::UnboundResolvingPlayer,
            ),
            (
                "payer",
                json!("TargetSpellController"),
                DefinitionError::ResolvingPlayerTarget,
            ),
            (
                "cost",
                json!({"generic":"-1","required":{}}),
                DefinitionError::InvalidResolutionPaymentCost,
            ),
            (
                "cost",
                json!({"generic":"0","required":{"Blue":"-1"}}),
                DefinitionError::InvalidResolutionPaymentCost,
            ),
            (
                "paid_destination",
                json!("0"),
                DefinitionError::InvalidControlFlow,
            ),
            (
                "declined_destination",
                json!("2"),
                DefinitionError::InvalidControlFlow,
            ),
        ] {
            let mut instruction = payment_instruction();
            instruction[field] = value;
            let raw = optional_payment_definition(json!([instruction]));
            let program = decode_for_owner(raw.clone(), &vocabulary).unwrap();
            assert_eq!(program.validate(&vocabulary), Err(expected));
            assert!(decode(raw, &vocabulary).is_err());
        }
        assert!(
            decode(
                optional_payment_definition(json!([payment_instruction()])),
                &vocabulary
            )
            .is_ok()
        );
    }
    #[test]
    fn optional_payment_raw_duplicate_fields_reject_before_definition_interpretation() {
        for instruction in [
            r#"{"op":"OptionalEffect","chooser":"Controller","chooser":"Controller"}"#,
            r#"{"op":"OfferManaPayment","payer":"Controller","payer":"Controller"}"#,
            r#"{"op":"OfferManaPayment","cost":{"generic":"0","generic":"0","required":{}}}"#,
            r#"{"op":"OfferManaPayment","cost":{"generic":"0","required":{"Blue":"1","Blue":"1"}}}"#,
            r#"{"op":"OfferManaPayment","paid_destination":"1","paid_destination":"1"}"#,
            r#"{"op":"OfferManaPayment","declined_destination":"1","declined_destination":"1"}"#,
        ] {
            let raw = format!(r#"{{"definition":{{"body":{{"instructions":[{instruction}]}}}}}}"#);
            assert!(
                super::super::packet::unique_json(raw.as_bytes())
                    .unwrap_err()
                    .contains("duplicate object field")
            );
        }
    }
    fn selection_instruction(op: &str) -> Value {
        let mut value = json!({
            "op":op,
            "players":{"tag":"One","player":"Controller"},
            "amount":{"tag":"Literal","value":"1"}
        });
        if op == "Sacrifice" {
            value["filter"] = json!({"required_types":["Creature"],"forbidden_types":[]});
        }
        value
    }

    #[test]
    fn selection_round_trip_preserves_every_role_scope_and_amount_without_narrowing() {
        use spellwake_engine::program::{CreatureTypeSlot, TriggerGrouping};
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let huge = RulesInteger::try_from("184467440737095516161234567890".to_owned()).unwrap();
        let scopes = [
            AffectedPlayers::One(ResolvingPlayer::Controller),
            AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
            AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
            AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
            AffectedPlayers::EachPlayer,
        ];
        let amounts = [
            InstructionAmount::Literal(0.into()),
            InstructionAmount::Literal(huge.clone()),
            InstructionAmount::ControlledPermanentsOfChosenType {
                chosen: CreatureTypeSlot::new(0),
            },
            InstructionAmount::MatchedOccurrences,
        ];
        for players in scopes {
            for amount in &amounts {
                let target = match players {
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer) => BodyTarget::SinglePlayer,
                    AffectedPlayers::One(ResolvingPlayer::TargetSpellController) => {
                        BodyTarget::SingleSpell(SpellTargetFilter::AnySpell)
                    }
                    AffectedPlayers::One(
                        ResolvingPlayer::Controller | ResolvingPlayer::UniqueRelatedPlayer,
                    )
                    | AffectedPlayers::EachPlayer => BodyTarget::None,
                };
                let mut instructions = vec![];
                if matches!(
                    amount,
                    InstructionAmount::ControlledPermanentsOfChosenType { .. }
                ) {
                    instructions.push(Instruction::ChooseCreatureType {
                        into: CreatureTypeSlot::new(0),
                        excluding: BTreeSet::new(),
                    });
                }
                instructions.extend([
                    Instruction::Sacrifice {
                        players,
                        amount: amount.clone(),
                        filter: PermanentTypeFilter {
                            required_types: BTreeSet::from([
                                CardType::Artifact,
                                CardType::Creature,
                            ]),
                            forbidden_types: BTreeSet::from([
                                CardType::Land,
                                CardType::Enchantment,
                            ]),
                        },
                    },
                    Instruction::Discard {
                        players,
                        amount: amount.clone(),
                    },
                ]);
                let mut program = Program::artifact(None, vec![]);
                program.triggered.push(TriggeredAbility {
                    id: TriggeredAbilityId(1),
                    condition: OrdinaryCondition::SpellCast {
                        player: PlayerRelation::AnyParticipant,
                        filter: SpellCastFilter::AnySpell,
                    },
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(target, instructions),
                });
                let raw = view(&program);
                assert_eq!(decode(raw.clone(), &vocabulary).unwrap(), program);
                assert_eq!(view(&decode(raw.clone(), &vocabulary).unwrap()), raw);
                let encoded = &raw["triggered"][0]["body"]["instructions"];
                let sacrifice = &encoded[encoded.as_array().unwrap().len() - 2];
                assert_eq!(sacrifice["players"], affected_players(players));
                assert_eq!(sacrifice["amount"], instruction_amount(amount));
                assert_eq!(
                    sacrifice["filter"]["required_types"],
                    json!(["Artifact", "Creature"])
                );
                assert_eq!(
                    sacrifice["filter"]["forbidden_types"],
                    json!(["Land", "Enchantment"])
                );
                if let InstructionAmount::Literal(value) = amount {
                    assert_eq!(sacrifice["amount"]["value"], String::from(value.clone()));
                }
            }
        }
    }

    #[test]
    fn selection_nested_token_trigger_and_activated_bodies_round_trip_completely() {
        use spellwake_engine::program::TriggerGrouping;
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut leaf = Program::artifact(None, vec![]);
        leaf.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(3),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    amount: InstructionAmount::Literal(2.into()),
                }],
            ),
        });
        leaf.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(7),
            condition: OrdinaryCondition::Enters(ObjectRelation::SelfObject),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::Sacrifice {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::MatchedOccurrences,
                    filter: PermanentTypeFilter {
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                    },
                }],
            ),
        });
        let token = TokenDefinition::new(
            leaf,
            TokenName::Default {
                ordered_subtypes: vec![],
            },
            &vocabulary,
        )
        .unwrap();
        let mut middle = Program::artifact(None, vec![]);
        middle.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::Enters(ObjectRelation::SelfObject),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(token)],
                }],
            ),
        });
        let token = TokenDefinition::new(
            middle,
            TokenName::Default {
                ordered_subtypes: vec![],
            },
            &vocabulary,
        )
        .unwrap();
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(token)],
                }],
            ),
        );
        let raw = view(&program);
        assert_eq!(decode(raw.clone(), &vocabulary).unwrap(), program);
        assert_eq!(view(&decode(raw.clone(), &vocabulary).unwrap()), raw);
        let leaf_pointer = "/kind/body/instructions/0/creations/0/definition/program/triggered/0/body/instructions/0/creations/0/definition/program";
        let mut changed = raw.clone();
        changed.pointer_mut(leaf_pointer).unwrap()["activated"][0]["body"]["instructions"][0]["amount"]
            ["value"] = json!("3");
        let changed_program = decode(changed.clone(), &vocabulary).unwrap();
        assert_ne!(changed_program, program);
        assert_ne!(changed, raw);
        assert_eq!(view(&changed_program), changed);
        let mut changed = raw;
        changed.pointer_mut(leaf_pointer).unwrap()["triggered"][0]["body"]["instructions"][0]["players"] =
            json!({"tag":"One","player":"Controller"});
        let changed_program = decode(changed.clone(), &vocabulary).unwrap();
        assert_ne!(changed_program, program);
        assert_eq!(view(&changed_program), changed);
    }

    #[test]
    fn selection_operand_changes_are_distinct_complete_program_values_and_encodings() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let baseline = optional_payment_definition(json!([selection_instruction("Sacrifice")]));
        let program = decode(baseline.clone(), &vocabulary).unwrap();
        let modifications = [
            (
                "/kind/body/instructions/0/players",
                json!({"tag":"EachPlayer"}),
            ),
            ("/kind/body/instructions/0/amount/value", json!("2")),
            (
                "/kind/body/instructions/0/filter/required_types",
                json!(["Artifact"]),
            ),
            (
                "/kind/body/instructions/0/filter/forbidden_types",
                json!(["Land"]),
            ),
        ];
        for (pointer, operand) in modifications {
            let mut changed = baseline.clone();
            *changed.pointer_mut(pointer).unwrap() = operand;
            let decoded = decode(changed.clone(), &vocabulary).unwrap();
            assert_ne!(decoded, program, "{pointer}");
            assert_ne!(view(&decoded), view(&program), "{pointer}");
            assert_eq!(view(&decoded), changed, "{pointer}");
        }
        let mut targeted = baseline.clone();
        targeted["kind"]["body"]["target"] = json!({"tag":"SinglePlayer"});
        targeted["kind"]["body"]["instructions"][0]["players"] =
            json!({"tag":"One","player":"TargetPlayer"});
        let decoded = decode(targeted.clone(), &vocabulary).unwrap();
        assert_ne!(decoded, program);
        assert_eq!(view(&decoded), targeted);

        let mut unordered = baseline;
        unordered["kind"]["body"]["instructions"][0]["filter"] = json!({"required_types":["Creature","Artifact"],"forbidden_types":["Enchantment","Land"]});
        let canonical = view(&decode(unordered, &vocabulary).unwrap());
        assert_eq!(
            canonical["kind"]["body"]["instructions"][0]["filter"],
            json!({"required_types":["Artifact","Creature"],"forbidden_types":["Land","Enchantment"]})
        );
    }

    #[test]
    fn selection_syntax_rejects_unknown_duplicate_missing_and_malformed_operands() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for op in ["Sacrifice", "Discard"] {
            let instruction = selection_instruction(op);
            for key in instruction.as_object().unwrap().keys() {
                let mut wrong = instruction.clone();
                wrong.as_object_mut().unwrap().remove(key);
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err(),
                    "{op}/{key}"
                );
            }
            for players in [
                json!("EachPlayer"),
                json!(null),
                json!([]),
                json!({"tag":"EachPlayer","player":"Controller"}),
                json!({"tag":"One"}),
                json!({"tag":"One","player":"controller"}),
                json!({"tag":"One","player":{"tag":"Controller"}}),
                json!({"tag":"One","player":"Controller","extra":true}),
                json!({"tag":"EachOpponent"}),
            ] {
                let mut wrong = instruction.clone();
                wrong["players"] = players;
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err()
                );
            }
            for amount in [
                json!(1),
                json!("1"),
                json!(null),
                json!({"tag":"Literal","value":1}),
                json!({"tag":"Literal","value":"01"}),
                json!({"tag":"Literal","value":"+1"}),
                json!({"tag":"Literal","value":"-0"}),
                json!({"tag":"MatchedOccurrences","extra":true}),
                json!({"tag":"Unknown"}),
            ] {
                let mut wrong = instruction.clone();
                wrong["amount"] = amount;
                assert!(
                    decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary)
                        .is_err()
                );
            }
            let mut wrong = instruction;
            wrong["destination"] = json!("Graveyard");
            assert!(
                decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary).is_err()
            );
        }
        let sacrifice = selection_instruction("Sacrifice");
        for filter in [
            json!({"required_types":[],"forbidden_types":[],"extra":true}),
            json!({"required_types":[]}),
            json!({"forbidden_types":[]}),
            json!({"required_types":["Creature","Creature"],"forbidden_types":[]}),
            json!({"required_types":[],"forbidden_types":["Land","Land"]}),
            json!({"required_types":["Planeswalker"],"forbidden_types":[]}),
            json!({"required_types":[],"forbidden_types":["creature"]}),
            json!({"required_types":null,"forbidden_types":[]}),
            json!([]),
            json!(null),
        ] {
            let mut wrong = sacrifice.clone();
            wrong["filter"] = filter;
            assert!(
                decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary).is_err()
            );
        }
        let mut wrong = selection_instruction("Discard");
        wrong["filter"] = json!({"required_types":[],"forbidden_types":[]});
        assert!(
            decode_for_owner(optional_payment_definition(json!([wrong])), &vocabulary).is_err()
        );
    }

    #[test]
    fn selection_syntax_preserves_semantic_rejections_for_the_actual_native_owner() {
        use spellwake_engine::program::DefinitionError;
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut negative = selection_instruction("Sacrifice");
        negative["amount"]["value"] = json!("-1");
        let mut overlap = selection_instruction("Sacrifice");
        overlap["filter"]["forbidden_types"] = json!(["Creature"]);
        let mut target = selection_instruction("Discard");
        target["players"]["player"] = json!("TargetPlayer");
        let mut related = selection_instruction("Discard");
        related["players"]["player"] = json!("UniqueRelatedPlayer");
        let mut matched = selection_instruction("Discard");
        matched["amount"] = json!({"tag":"MatchedOccurrences"});
        let mut chosen = selection_instruction("Sacrifice");
        chosen["amount"] = json!({"tag":"ControlledPermanentsOfChosenType","chosen":"0"});
        for (instruction, expected) in [
            (negative, DefinitionError::InvalidInstruction),
            (overlap, DefinitionError::InvalidInstruction),
            (target, DefinitionError::ResolvingPlayerTarget),
            (related, DefinitionError::UnboundResolvingPlayer),
            (matched, DefinitionError::UnboundTriggerAmount),
            (chosen, DefinitionError::UndefinedCreatureTypeSlot),
        ] {
            let raw = optional_payment_definition(json!([instruction]));
            let decoded = decode_for_owner(raw.clone(), &vocabulary).unwrap();
            assert_eq!(decoded.validate(&vocabulary), Err(expected));
            assert!(decode(raw, &vocabulary).is_err());
        }
        for op in ["Sacrifice", "Discard"] {
            let raw = optional_payment_definition(
                json!([{"op":"OptionalEffect","chooser":"Controller"},selection_instruction(op)]),
            );
            let decoded = decode_for_owner(raw.clone(), &vocabulary).unwrap();
            assert_eq!(
                decoded.validate(&vocabulary),
                Err(DefinitionError::UnsupportedOptionalEffect)
            );
            assert!(decode(raw, &vocabulary).is_err());
        }
    }

    #[test]
    fn selection_raw_duplicate_fields_reject_before_interpretation_including_nested_values() {
        for instruction in [
            r#"{"op":"Sacrifice","op":"Sacrifice"}"#,
            r#"{"op":"Discard","players":{"tag":"One","tag":"One","player":"Controller"}}"#,
            r#"{"op":"Discard","players":{"tag":"One","player":"Controller","player":"TargetPlayer"}}"#,
            r#"{"op":"Discard","amount":{"tag":"Literal","value":"1","value":"2"}}"#,
            r#"{"op":"Sacrifice","filter":{"required_types":[],"required_types":[],"forbidden_types":[]}}"#,
            r#"{"op":"Sacrifice","filter":{"required_types":[],"forbidden_types":[],"forbidden_types":[]}}"#,
        ] {
            let raw = format!(r#"{{"definition":{{"body":{{"instructions":[{instruction}]}}}}}}"#);
            assert!(
                super::super::packet::unique_json(raw.as_bytes())
                    .unwrap_err()
                    .contains("duplicate object field")
            );
        }
    }
    #[test]
    fn land_labels_are_required_strict_and_independent_of_other_root_traits() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut raw = definition();
        raw["card_types"] = json!(["Land"]);
        raw["kind"] = json!({"tag":"Permanent","printed_pt":null,"casting_cost":null});
        raw["land_subtypes"] = json!(LandType::ALL.map(LandType::wire_label));
        assert_eq!(view(&decode(raw.clone(), &vocabulary).unwrap()), raw);
        for invalid in [
            json!(["Plains", "Plains"]),
            json!(["Urza’s"]),
            json!(["Urza's"]),
            json!(["Power-Plant"]),
            json!(["Clue"]),
            json!(null),
            json!([1]),
        ] {
            let mut malformed = raw.clone();
            malformed["land_subtypes"] = invalid;
            assert!(decode_for_owner(malformed, &vocabulary).is_err());
        }
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove("land_subtypes");
        assert!(decode_for_owner(missing, &vocabulary).is_err());
        raw["card_types"] = json!(["Artifact"]);
        assert!(decode_for_owner(raw.clone(), &vocabulary).is_ok());
        assert!(decode(raw, &vocabulary).is_err());
    }
    #[test]
    fn root_card_types_and_shared_family_are_required_strict_and_round_trip() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let elf = projection::creature_type_id(vocabulary.resolve_label("Elf").unwrap());
        let mut raw = definition();
        raw["card_types"] = json!(["Kindred", "Instant"]);
        raw["creature_subtypes"] = json!([elf]);
        assert_eq!(view(&decode(raw.clone(), &vocabulary).unwrap()), raw);
        for field in ["card_types", "creature_subtypes", "land_subtypes"] {
            let mut missing = raw.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(decode_for_owner(missing, &vocabulary).is_err(), "{field}");
        }
        for obsolete in ["types", "creature_subtypes"] {
            let mut old = raw.clone();
            old["kind"][obsolete] = json!([]);
            assert!(decode_for_owner(old, &vocabulary).is_err(), "{obsolete}");
        }
        for invalid in [
            json!(["Kindred", "Instant", "Instant"]),
            json!(["Plane"]),
            json!(null),
        ] {
            let mut malformed = raw.clone();
            malformed["card_types"] = invalid;
            assert!(decode_for_owner(malformed, &vocabulary).is_err());
        }
        for obsolete in ["Instant", "Sorcery"] {
            let mut old = raw.clone();
            old["kind"]["tag"] = json!(obsolete);
            assert!(decode_for_owner(old, &vocabulary).is_err());
        }
        let mut owner_rejects = raw;
        owner_rejects["card_types"] = json!(["Kindred"]);
        assert!(decode_for_owner(owner_rejects.clone(), &vocabulary).is_ok());
        assert!(decode(owner_rejects, &vocabulary).is_err());
    }
    #[test]
    fn syntax_and_owner_admission_stay_distinct_without_a_permissive_old_codec() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut value = definition();
        value["kind"]["body"]["instructions"] = json!([{"op":"DrawController","amount":{"tag":"ControlledPermanentsOfChosenType","chosen":"0"}}]);
        assert!(decode_for_owner(value.clone(), &vocabulary).is_ok());
        assert!(decode(value, &vocabulary).is_err());
        let value = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Artifact"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":null,"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null});
        assert!(decode(value.clone(), &vocabulary).is_ok());
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("creature_subtypes");
        assert!(decode_for_owner(missing, &vocabulary).is_err());
    }
    #[test]
    fn subtype_and_exclusion_inputs_share_the_same_membership_and_duplicate_checks() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let elf = projection::creature_type_id(vocabulary.resolve_label("Elf").unwrap());
        let mut value = definition();
        value["kind"]["body"]["instructions"][0]["excluding"] = json!([elf.clone(), elf]);
        assert!(decode_for_owner(value, &vocabulary).is_err());
    }
    #[test]
    fn names_are_required_exact_sets_and_live_operands_cannot_enter_resolved_effects() {
        let v = super::super::creature_types::synthetic_vocabulary();
        let mut p = Program::artifact(None, vec![]);
        p.names =
            ObjectNames::single(EnglishCardName::new("SP//dr, Piloted by Peni".into()).unwrap());
        let value = view(&p);
        assert_eq!(decode(value.clone(), &v).unwrap(), p);
        for invalid in [
            json!(null),
            json!("Elf"),
            json!(["Elf", "Elf"]),
            json!([" Elf"]),
        ] {
            let mut wrong = value.clone();
            wrong["names"] = invalid;
            assert!(decode_for_owner(wrong, &v).is_err());
        }
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("names");
        assert!(decode_for_owner(missing, &v).is_err());
        p.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector::default(),
            parts: StaticParts(vec![StaticPart::Names(NameOperation::Add(
                StaticNameOperand::SourceCurrentNames,
            ))]),
        });
        let value = view(&p);
        assert_eq!(decode(value.clone(), &v).unwrap(), p);
        let mut missing = value.clone();
        missing["continuous"]["selector"]
            .as_object_mut()
            .unwrap()
            .remove("name");
        assert!(decode_for_owner(missing, &v).is_err());
        let mut wrong = value;
        wrong["continuous"]["selector"]["name"] = json!({"tag":"Any","name":"Elf"});
        assert!(decode_for_owner(wrong, &v).is_err());
        let p = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Names(NameOperation::Set(
                        ObjectNames::nameless(),
                    ))]),
                    duration: ResolvedDuration::NoStatedDuration,
                }],
            ),
        );
        let mut wrong = view(&p);
        assert_eq!(decode(wrong.clone(), &v).unwrap(), p);
        wrong["kind"]["body"]["instructions"][0]["parts"][0]["change"]["operand"] =
            json!({"tag":"SourceCurrentNames"});
        assert!(decode_for_owner(wrong, &v).is_err());
    }

    #[test]
    fn supertypes_require_known_unique_sets_and_are_not_type_or_name_inference() {
        let v = super::super::creature_types::synthetic_vocabulary();
        let mut p = Program::artifact(None, vec![]);
        p.supertypes = BTreeSet::from([
            Supertype::Basic,
            Supertype::Legendary,
            Supertype::Ongoing,
            Supertype::Snow,
            Supertype::World,
        ]);
        let value = view(&p);
        assert_eq!(decode(value.clone(), &v).unwrap(), p);
        for invalid in [
            json!(null),
            json!("Snow"),
            json!(["Snow", "Snow"]),
            json!(["Creature"]),
            json!(["snow"]),
        ] {
            let mut wrong = value.clone();
            wrong["supertypes"] = invalid;
            assert!(decode_for_owner(wrong, &v).is_err());
        }
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("supertypes");
        assert!(decode_for_owner(missing, &v).is_err());
    }

    #[test]
    fn supertype_operations_and_filters_roundtrip_strictly_and_reject_contradictions() {
        let v = super::super::creature_types::synthetic_vocabulary();
        let mut p = Program::artifact(None, vec![]);
        p.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                required_supertypes: BTreeSet::from([Supertype::Snow]),
                forbidden_supertypes: BTreeSet::from([Supertype::Legendary]),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Add(BTreeSet::from([Supertype::Basic])),
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Keep,
            })]),
        });
        let value = view(&p);
        assert_eq!(decode(value.clone(), &v).unwrap(), p);
        for key in ["required_supertypes", "forbidden_supertypes"] {
            let mut wrong = value.clone();
            wrong["continuous"]["selector"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(decode_for_owner(wrong, &v).is_err());
            let mut wrong = value.clone();
            wrong["continuous"]["selector"][key] = json!(["Snow", "Snow"]);
            assert!(decode_for_owner(wrong, &v).is_err());
        }
        let mut contradiction = value.clone();
        contradiction["continuous"]["selector"]["forbidden_supertypes"] = json!(["Snow"]);
        assert!(decode_for_owner(contradiction.clone(), &v).is_ok());
        assert!(decode(contradiction, &v).is_err());
        let mut missing = value.clone();
        missing["continuous"]["parts"][0]
            .as_object_mut()
            .unwrap()
            .remove("supertypes");
        assert!(decode_for_owner(missing, &v).is_err());
        for operand in [
            json!({"tag":"Set","types":["Snow","Snow"]}),
            json!({"tag":"Add","types":["Creature"]}),
            json!({"tag":"Keep","types":[]}),
        ] {
            let mut wrong = value.clone();
            wrong["continuous"]["parts"][0]["supertypes"] = operand;
            assert!(decode_for_owner(wrong, &v).is_err());
        }
    }

    #[test]
    fn land_operations_and_filters_roundtrip_strictly_without_creature_or_live_operand_coercion() {
        let v = super::super::creature_types::synthetic_vocabulary();
        for operation in [
            SubtypeOperation::Keep,
            SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(BTreeSet::from([
                LandType::Forest,
                LandType::Urzas,
            ]))),
            SubtypeOperation::Set(StaticLandSubtypeOperand::SourceCurrentLandSubtypes),
            SubtypeOperation::Remove(BTreeSet::from([LandType::Island])),
        ] {
            let mut p = Program::artifact(None, vec![]);
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: PermanentSelector {
                    land_subtype: Some(LandType::Plains),
                    ..PermanentSelector::default()
                },
                parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                    land_subtypes: operation,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Keep,
                })]),
            });
            let raw = view(&p);
            assert_eq!(decode(raw.clone(), &v).unwrap(), p);
            let mut missing = raw.clone();
            missing["continuous"]["parts"][0]
                .as_object_mut()
                .unwrap()
                .remove("land_subtypes");
            assert!(decode_for_owner(missing, &v).is_err());
            let mut missing = raw.clone();
            missing["continuous"]["selector"]
                .as_object_mut()
                .unwrap()
                .remove("land_subtype");
            assert!(decode_for_owner(missing, &v).is_err());
            for invalid in [
                json!({"tag":"Add","value":{"tag":"Literal","types":["Plains","Plains"]}}),
                json!({"tag":"Remove","types":["Elf"]}),
                json!({"tag":"Set","value":{"tag":"Chosen","slot":"0"}}),
                json!({"tag":"Add","value":{"tag":"SourceCurrentSubtypes"}}),
                json!({"tag":"Keep","value":[]}),
            ] {
                let mut bad = raw.clone();
                bad["continuous"]["parts"][0]["land_subtypes"] = invalid;
                assert!(decode_for_owner(bad, &v).is_err());
            }
            for invalid in [
                json!("Elf"),
                json!("Urza’s"),
                json!(1),
                json!({"type":"Plains"}),
            ] {
                let mut bad = raw.clone();
                bad["continuous"]["selector"]["land_subtype"] = invalid;
                assert!(decode_for_owner(bad, &v).is_err());
            }
        }
        for operation in [
            SubtypeOperation::Keep,
            SubtypeOperation::Set(BTreeSet::from([LandType::Mountain])),
            SubtypeOperation::Add(BTreeSet::from([LandType::Swamp])),
            SubtypeOperation::Remove(BTreeSet::from([LandType::Cave])),
        ] {
            let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: operation,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Keep,
            })]);
            let raw = parts_spec(&parts);
            assert_eq!(
                object_value::<PartWire<InstructionOperand, RulesInteger>>(raw[0].clone())
                    .and_then(|part| part.native(&v))
                    .unwrap(),
                parts.0[0]
            );
            let mut bad = raw;
            bad[0]["land_subtypes"] =
                json!({"tag":"Add","value":{"tag":"SourceCurrentLandSubtypes"}});
            assert!(
                object_value::<PartWire<InstructionOperand, RulesInteger>>(bad[0].clone())
                    .and_then(|part| part.native(&v))
                    .is_err()
            );
            let materialized = parts
                .materialize(
                    &BTreeMap::new(),
                    spellwake_engine::priority::SeatId(0),
                    spellwake_engine::priority::SeatId(1),
                )
                .unwrap();
            let ResolvedPart::Type(actual) = &materialized.0[0] else {
                panic!()
            };
            let ResolvedPartSpec::Type(authored) = &parts.0[0] else {
                panic!()
            };
            assert_eq!(actual.land_subtypes, authored.land_subtypes);
        }
    }
    #[test]
    fn native_library_program_codec_round_trips_meaningful_variants_and_rejects_malformed_fields() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for op in ["Scry", "Surveil", "Mill"] {
            let raw = optional_payment_definition(
                json!([{"op":op,"player":"Controller","amount":{"tag":"Literal","value":"9999999999999999999999999999999"}}]),
            );
            let program = decode(raw.clone(), &vocabulary).unwrap();
            assert_eq!(view(&program), raw);
            for malformed in [
                json!(null),
                json!("1"),
                json!({"tag":"Literal","value":"1","extra":true}),
            ] {
                let mut bad = raw.clone();
                bad["kind"]["body"]["instructions"][0]["amount"] = malformed;
                assert!(decode(bad, &vocabulary).is_err());
            }
            let mut bad = raw.clone();
            bad["kind"]["body"]["instructions"][0]["extra"] = json!(true);
            assert!(decode(bad, &vocabulary).is_err());
            let mut bad = raw;
            bad["kind"]["body"]["instructions"][0]["player"] = json!("UnknownRole");
            assert!(decode(bad, &vocabulary).is_err());
        }
        for condition in [
            OrdinaryCondition::Scried(PlayerRelation::Opponent),
            OrdinaryCondition::Surveilled(PlayerRelation::Controller),
        ] {
            let mut native = Program::artifact(None, vec![]);
            native.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(0),
                condition,
                grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    }],
                ),
            });
            assert_eq!(decode(view(&native), &vocabulary).unwrap(), native);
        }
    }
}
