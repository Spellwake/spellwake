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
        AbilityChange, AbilityRemoval, BlockingRestriction, CastPermissionRule, Color, ColorFilter,
        ColorOperation, ControlRecipient, ControllerSelector, CreatureTypeTextChange,
        CreatureTypeTextChangeSpec, CreatureTypeWord, DefiningCreatureTypes, DefiningPart,
        DefiningParts, InstructionSubtypeOperand, KeywordAbility, LibraryTopPermission, NameFilter,
        NameOperation, NumericExpression, NumericPair, PermanentSelector, PowerToughness,
        PowerToughnessCounters, PrintedPowerToughness, PrintedStat, PrintedText, ResolvedDuration,
        ResolvedPart, ResolvedPartSpec, ResolvedParts, ResolvedPartsSpec, SpellCostModifier,
        SpellDiscount, StaticAbilityId, StaticColorOperand, StaticContinuousAbility,
        StaticDamageRule, StaticDrawRule, StaticLandSubtypeOperand, StaticModifierRule,
        StaticNameOperand, StaticPart, StaticParts, StaticRestriction, StaticSubtypeOperand,
        StaticTypeChange, StaticUntapRule, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    land::LandType,
    life::{LifeAdjustment, LifeChangeKind},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    program::{
        AffectedPlayers, AnyTargetKind, ArtifactSubtype, AttachedCountKind, AttackTriggerMode,
        BlockTriggerKind, BodyTarget, CardType, CardTypeMask, CastEntryKind, CombatTriggerKeyword,
        Comparison, ControllerFilter, CounterSubjectRef, CounterTriggerKind,
        DamageTriggerRecipient, DiesTriggerMode, EventValueKind, ExecutableBody, Instruction,
        MatchObjectRef, ObjectColorFilter, ObjectFilter, ObjectFilterFlags, ObjectStatKind,
        PermanentDefinition, PermanentTypeFilter, PlayerRelation, PlayerStatExpr,
        PlayerTriggerAction, PowerFilter, Program, ProgramKind, ResolutionCondition,
        ResolvingPlayer, SourceRelation, SpellDefinition, SpellFilter, StackTargetKind,
        StatAggregate, StateThresholdKind, StatusTriggerKind, SubtypeFilter, Supertype, TargetZone,
        TriggerCondition, TriggerEvent, TriggerFromZone, TriggerKeywordEvent, TriggerPlayerScope,
        TriggerSpec, TriggerZone, TriggeredAbility, TriggeredAbilityId, ValueExpr, ValueObjectRef,
        ValueZone,
    },
    program::{
        PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSize, TokenSubtype},
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
fn spell_subtypes(
    values: Vec<String>,
) -> Result<BTreeSet<spellwake_engine::program::SpellSubtype>, String> {
    let mut result = BTreeSet::new();
    for label in values {
        let value = spellwake_engine::program::SpellSubtype::from_wire_label(&label)
            .ok_or("unknown spell subtype")?;
        if !result.insert(value) {
            return Err("duplicate spell subtype".into());
        }
    }
    Ok(result)
}
fn artifact_subtypes(
    values: Vec<String>,
) -> Result<BTreeSet<spellwake_engine::program::ArtifactSubtype>, String> {
    let mut result = BTreeSet::new();
    for label in values {
        let value = spellwake_engine::program::ArtifactSubtype::from_wire_label(&label)
            .ok_or("unknown artifact subtype")?;
        if !result.insert(value) {
            return Err("duplicate artifact subtype".into());
        }
    }
    Ok(result)
}
fn enchantment_subtypes(
    values: Vec<String>,
) -> Result<BTreeSet<spellwake_engine::program::EnchantmentSubtype>, String> {
    let mut result = BTreeSet::new();
    for label in values {
        let value = spellwake_engine::program::EnchantmentSubtype::from_wire_label(&label)
            .ok_or("unknown enchantment subtype")?;
        if !result.insert(value) {
            return Err("duplicate enchantment subtype".into());
        }
    }
    Ok(result)
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
        "Battle" => Ok(CardType::Battle),
        "Planeswalker" => Ok(CardType::Planeswalker),
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
    Lacks { color: String },
    Colorless {},
    Monocolored {},
    Multicolored {},
}
impl ColorFilterWire {
    fn native(self) -> Result<ColorFilter, String> {
        Ok(match self {
            Self::Any {} => ColorFilter::Any,
            Self::Has { color: name } => ColorFilter::Has(color(&name)?),
            Self::Lacks { color: name } => ColorFilter::Lacks(color(&name)?),
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
pub fn mana_values(values: &BTreeMap<ManaType, RulesInteger>) -> Value {
    json!(
        values
            .iter()
            .map(|(kind, amount)| (format!("{kind:?}"), amount))
            .collect::<BTreeMap<_, _>>()
    )
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
// Wire-schema fixture enum preserving exact JSON variant names.
#[allow(clippy::enum_variant_names)]
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Target {
    SingleOwnGraveyardCard {},
    FilteredOwnGraveyardCard { filter: String },
    OwnGraveyardCardWithSubtype { subtype: Value },
    AnyGraveyardCreatureOrPlaneswalker {},
    RecentlyDiscardedOrFromLibrary {},
    AnyGraveyardCard {},
    AnyGraveyardCreature {},
    FilteredPermanent { filter: String },
    CreatureOfSubtype { subtype: Value },
    PermanentOfCreatureSubtype { subtype: Value },
    None {},
    SinglePermanent {},
    SingleCreature {},
    SingleOpponent {},
    SinglePlayer {},
    SingleCreatureOrPlayer {},
    SinglePlayerOrPlaneswalker {},
    SinglePlaneswalker {},
    SingleStackEntry {},
    ControlledAbility { colorless_source: bool },
    StackEntryTargetingControlledPermanent {},
    SingleAnyTarget {},
    SingleSpell { support_gate: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    target: Target,
    instructions: Vec<Op>,
    #[serde(default)]
    triggered_modes: Option<TriggerModesWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerModesWire {
    minimum: u8,
    maximum: u8,
    distinct_targets: bool,
    options: Vec<Body>,
}
#[derive(Deserialize)]
#[serde(tag = "family", deny_unknown_fields)]
enum TokenSubtypeWire {
    Artifact { r#type: String },
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
                            TokenSubtypeWire::Artifact { r#type } => TokenSubtype::Artifact(
                                spellwake_engine::program::ArtifactSubtype::from_wire_label(
                                    &r#type,
                                )
                                .ok_or("unknown token artifact subtype")?,
                            ),
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
    #[serde(default)]
    base_size: Option<TokenSizeWire>,
    #[serde(default)]
    enters_tapped: bool,
    recipients: TokenRecipientsWire,
}
#[derive(Deserialize)]
enum TokenRecipientsWire {
    CombatDamageSourceController,
    RememberedTargetController,
    ExiledTargetController,
    Controller,
    EachPlayer,
    EachOpponent,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum TokenSizeWire {
    AtCreation(Amount),
    Live(LiveTokenSizeWire),
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum LiveTokenSizeWire {
    LiveAbilitySourceCounters { kind: String },
}
impl TokenSizeWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TokenSize, String> {
        match self {
            Self::AtCreation(amount) => Ok(TokenSize::AtCreation(amount.native(vocabulary)?)),
            Self::Live(LiveTokenSizeWire::LiveAbilitySourceCounters { kind }) => {
                Ok(TokenSize::AbilitySourceCounters(
                    spellwake_engine::effects::CounterKind::PERMANENT
                        .into_iter()
                        .find(|candidate| format!("{candidate:?}") == kind)
                        .ok_or("unknown permanent counter")?,
                ))
            }
        }
    }
}
impl TokenCreationWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TokenCreation, String> {
        Ok(TokenCreation {
            definition: self.definition.native(vocabulary)?,
            amount: self.amount.native(vocabulary)?,
            base_size: self
                .base_size
                .map(|value| value.native(vocabulary))
                .transpose()?,
            enters_tapped: self.enters_tapped,
            recipients: match self.recipients {
                TokenRecipientsWire::CombatDamageSourceController => {
                    TokenRecipients::CombatDamageSourceController
                }
                TokenRecipientsWire::RememberedTargetController => {
                    TokenRecipients::RememberedTargetController
                }
                TokenRecipientsWire::ExiledTargetController => {
                    TokenRecipients::ExiledTargetController
                }
                TokenRecipientsWire::Controller => TokenRecipients::Controller,
                TokenRecipientsWire::EachPlayer => TokenRecipients::EachPlayer,
                TokenRecipientsWire::EachOpponent => TokenRecipients::EachOpponent,
            },
            attacking_defender: None,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum CounterKeywordWire {
    Fading {
        amount: u32,
    },
    Vanishing {
        #[serde(deserialize_with = "required_nullable")]
        amount: Option<u32>,
    },
    VanishingLastCounter {},
    SuspendUpkeep {},
    SuspendLastCounter {},
    Persist {},
    Undying {},
}
impl CounterKeywordWire {
    fn native(self) -> spellwake_engine::program::CounterKeyword {
        use spellwake_engine::program::CounterKeyword as K;
        match self {
            Self::Fading { amount } => K::Fading(amount),
            Self::Vanishing { amount } => K::Vanishing(amount),
            Self::VanishingLastCounter {} => K::VanishingLastCounter,
            Self::SuspendUpkeep {} => K::SuspendUpkeep,
            Self::SuspendLastCounter {} => K::SuspendLastCounter,
            Self::Persist {} => K::Persist,
            Self::Undying {} => K::Undying,
        }
    }
}
fn counter_keyword(keyword: spellwake_engine::program::CounterKeyword) -> Value {
    use spellwake_engine::program::CounterKeyword as K;
    match keyword {
        K::Fading(n) => json!({"kind":"Fading","amount":n}),
        K::Vanishing(n) => json!({"kind":"Vanishing","amount":n}),
        other => json!({"kind":format!("{other:?}")}),
    }
}
#[derive(Deserialize)]
enum LinkWire {
    FiendHunter,
    Helvault,
    SourceLinked,
}
impl From<LinkWire> for spellwake_engine::program::LinkedExileGroup {
    fn from(value: LinkWire) -> Self {
        match value {
            LinkWire::FiendHunter => Self::FiendHunter,
            LinkWire::Helvault => Self::Helvault,
            LinkWire::SourceLinked => Self::SourceLinked,
        }
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
    OfferSourceCounters {
        amount: RulesInteger,
        placed_destination: usize,
        declined_destination: usize,
    },
    OptionalSequence {
        chooser: ResolvingPlayerWire,
        instructions: usize,
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
    CreateTokenAndAttach {
        definition: TokenDefinitionWire,
    },
    CreateTokens {
        #[serde(deserialize_with = "objects")]
        creations: Vec<TokenCreationWire>,
    },
    GainControllerLife {
        amount: Amount,
    },
    DrainTargetAndGain {
        amount: Amount,
    },
    AdjustControllerLife {
        adjustment: LifeAdjustmentWire,
    },
    AdjustTargetLife {
        adjustment: LifeAdjustmentWire,
    },
    OpponentsLoseAndControllerGains {
        amount: Amount,
    },
    AdjustEachOpponentLife {
        adjustment: LifeAdjustmentWire,
        #[allow(dead_code)]
        controller_adjustment: Option<LifeAdjustmentWire>,
    },
    RememberPermanentTarget {},
    DamageEachOpponent {
        amount: RulesInteger,
    },
    DamageGroup {
        group: String,
        amount: RulesInteger,
    },
    TakeExtraTurn {},
    DisallowDamagePreventionThisTurn {},
    GrantNoMaxHandSizeRestOfGame {},
    ExtraCombatControlled {},
    ExtraCombatHighestLife {},
    ExtraCombatSubtype {
        subtype: Value,
    },
    GainControlOfSpell {},
    ChooseNewTargets {},
    ChangeOnlyTarget {},
    DestroyGroup {
        group: GroupWire,
    },
    DestroyGroupWithoutRegeneration {
        group: GroupWire,
    },
    MoveGroup {
        group: GroupWire,
        destination: String,
    },
    DrawTargetPlayer {
        amount: Amount,
    },
    DrawEachPlayer {
        amount: Amount,
    },
    ExileTopFaceDown {},
    ReturnExiledTargetCohort {
        next_end_step: bool,
        #[serde(default)]
        owner_controls: bool,
    },
    ExileTopForPlay {
        until_next_from_source: bool,
    },
    AddFixedMana {
        produced: BTreeMap<String, RulesInteger>,
    },
    CreateReturnFallenArtifactsEmblem {},
    CreateUnlimitedHandWithGraveyardReturnEmblem {},
    CreateOpponentUnlimitedHandWithGraveyardReturnEmblems {},
    CreateOpponentReturnFallenArtifactsEmblems {},
    CreateSacrificeCreatureGainLifeDrawEmblem {},
    CreateOpponentSacrificeCreatureGainLifeDrawEmblems {},
    CreateEmblem {
        power_toughness: Pt,
        keywords: Vec<String>,
    },
    CreateSpellcastDamageEmblem {
        amount: RulesInteger,
        noncreature_only: bool,
    },
    UntapControlledLands {},
    GiveControlOfAbilitySource {},
    ChangeAbilitySource {
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    DoubleCountersOnTargets {},
    Proliferate {},
    ManifestTop {
        player: ResolvingPlayerWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
    },
    PutHandPermanentsOntoBattlefield {
        player: ResolvingPlayerWire,
        types: Vec<String>,
        maximum: u16,
        #[serde(default)]
        tapped: bool,
    },
    DamageCreaturesExcept {
        subtype: Value,
        amount: RulesInteger,
    },
    CreatePreventAllButOneEmblem {},
    CreateOpponentPreventAllButOneEmblems {},
    CreateLoyaltyAnyTurnEmblem {},
    CreateOpponentLoyaltyAnyTurnEmblems {},
    CreateUpkeepDamageEmblem {
        amount: RulesInteger,
    },
    CreateOpponentUpkeepDamageEmblems {
        amount: RulesInteger,
    },
    CreateOpponentSpellcastDamageEmblems {
        amount: RulesInteger,
        noncreature_only: bool,
    },
    CreateOpponentCreatureBonusEmblems {
        power_toughness: Pt,
        keywords: Vec<String>,
    },
    DrawController {
        amount: Amount,
    },
    AffectPermanentTargets {
        minimum: u16,
        maximum: u16,
        action: String,
    },
    ModifyAnyNumberOfPermanents {
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    ModifyPermanentTargets {
        minimum: u16,
        maximum: u16,
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    ModifyPermanentTargetAndPartner {
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    MillEachPlayer {
        amount: u16,
    },
    ExileOnePerPlayerUntilSourceLeaves {},
    Populate {},
    Bolster {
        amount: u16,
    },
    Amass {
        army: Value,
        subtype: Value,
        amount: u16,
    },
    ExileChosenPermanents {
        #[serde(deserialize_with = "object")]
        players: AffectedPlayersWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
        #[serde(deserialize_with = "object")]
        filter: PermanentTypeFilterWire,
    },
    ReturnControlledPermanents {
        #[serde(deserialize_with = "object")]
        players: AffectedPlayersWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
        #[serde(deserialize_with = "object")]
        filter: PermanentTypeFilterWire,
    },
    Sacrifice {
        #[serde(deserialize_with = "object")]
        players: AffectedPlayersWire,
        #[serde(deserialize_with = "object")]
        amount: Amount,
        #[serde(deserialize_with = "object")]
        filter: PermanentTypeFilterWire,
    },
    DiscardUpTo {
        maximum: u16,
    },
    DiscardOneAtRandom {
        player: Option<ResolvingPlayerWire>,
    },
    SacrificeRandomUntilLand {},
    SacrificeMatching {
        subtype: Option<Value>,
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
    Afflict {
        amount: RulesInteger,
    },
    Dethrone {},
    Melee {},
    Renown {
        amount: RulesInteger,
    },
    Training {},
    Prowess {},
    BattleCry {},
    Bushido {
        amount: RulesInteger,
    },
    Decayed {},
    Mobilize {
        amount: RulesInteger,
    },
    Ingest {},
    Poisonous {
        amount: RulesInteger,
    },
    Frenzy {
        amount: RulesInteger,
    },
    Rampage {
        amount: RulesInteger,
    },
    Flanking {},
    SearchLibrary {
        filter: SearchWire,
    },
    ResolveCounterKeyword {
        keyword: CounterKeywordWire,
    },
    Graft {
        amount: u32,
    },
    Modular {
        amount: u32,
    },
    Soulshift {
        maximum: u32,
    },
    RegenerateTarget {},
    RelatedPlayerLosesHalfLife {},
    PutCountersOnControlledCreatures {},
    PutCountersOnControlledSubtype {
        subtype: Value,
    },
    PayControllerLife {
        amount: u16,
    },
    Encore {},
    ScheduleCreatedTokenSacrifice {},
    ScheduleCreatedTokenExile {
        at: String,
    },
    PutCountersOnOtherCreatures {},
    PutChargeCounterOnSource {},
    PutBlessingCounterOnSource {},
    PutNamedCounterOnSource {
        kind: String,
        amount: Option<u16>,
    },
    PutQuestCounterOnSource {},
    PutStorageCounterOnSource {},
    UntapAbilitySource {},
    TapAbilitySource {},
    TransformSource {},
    GrantFlashback {
        targeted: bool,
    },
    ExileTargetWithLink {
        group: LinkWire,
    },
    ReturnCardsExiledWithLink {
        group: LinkWire,
    },
    ReturnOwnGraveyardCardsOfTypeToHand {
        kind: String,
    },
    TransformSubtype {
        subtype: Value,
    },
    RegenerateSource {},
    ExileDefeatedSiege {},
    CastDefeatedSiege {},
    ReturnSourceToHand {},
    GoadTarget {},
    RequireTargetAttack {},
    RemoveTargetFromCombat {},
    RegenerateControlledSubtype {
        subtype: Value,
    },
    GoadOtherPlayersCreatures {},
    ExtortDrain {},
    DrainOpponents {
        amount: Amount,
    },
    DamageAmount {
        amount: Amount,
    },
    DamageAmountThenExileIfDies {
        amount: Amount,
    },
    DamageFromPower {
        source: String,
    },
    CreatureDealsPowerDamageToOther {
        trample_overflow: bool,
    },
    TapEnchantedCreature {},
    RequireMonarch {
        opponent: bool,
    },
    RequireOwnCommander {},
    RequireOpponentMoreLands {},
    RequireNoLoyaltyActivation {},
    ReselectAttackDefender {},
    AdditionalLoyaltyActivation {},
    SetBlockDeclarations {
        rule: String,
    },
    AbilitySourceControllerLosesLife {
        amount: u8,
    },
    RelatedPlayerLosesLife {
        amount: u8,
    },
    RequireOpponentLifeLoss {
        amount: u8,
    },
    RequireSourceQuestCounters {
        amount: u8,
    },
    RequireSourceLoyalty {},
    RequireGreatestArtifact {},
    RequireControlledCreatureCount {
        count: u16,
    },
    RequireNoOtherControlledCreatures {},
    Cascade {},
    FlipCoin {
        lose_destination: usize,
    },
    CascadeBottom {},
    CopyOnCast {
        keyword: String,
        #[serde(default)]
        replicate: Option<usize>,
    },
    ExileLibraryTop {
        player: ResolvingPlayerWire,
        amount: u16,
    },
    BindOrderedPermanents {
        #[allow(dead_code)]
        minimum: u16,
        #[allow(dead_code)]
        maximum: u16,
    },
    FixedTargetClause {
        body: Box<Body>,
    },
    ExileResolvingSpell {},
    FightFixedTargets {},
    AddCombatDamageManaUntilCleanup {
        kind: String,
    },
    AddCountedMana {
        kind: String,
        amount: Amount,
    },
    AddSplitMana {
        first: String,
        second: String,
        amount: Amount,
    },
    AddRestrictedManaChoice {
        colors: Vec<String>,
    },
    PhaseOut {
        subject: String,
    },
    PairWithCreature {
        entrant_only: bool,
    },
    DamageOpponentsAmount {
        amount: Amount,
    },
    DamageRelatedPlayerAmount {
        amount: Amount,
    },
    ReturnChosenXGraveyardCreatures {},
    ReturnGraveyardTargetsTapped {
        #[allow(dead_code)]
        count: u8,
    },
    DivideThreeDamageAmongTargets {},
    DamageTargetControllersCreatures {
        amount: Amount,
        include_target: bool,
        require_damaged_creatures_to_attack: bool,
    },
    DamageOrderedTargets {
        first: RulesInteger,
        others: RulesInteger,
    },
    Mentor {},
    Increment {},
    Evolve {},
    Backup {
        amount: RulesInteger,
        printed_below: Vec<String>,
    },
    SacrificeEvoked {},
    SacrificeStateTriggerSource {},
    CounterWardSource {},
    Ninjutsu {},
    Unearth {},
    ModifyGroupByAmount {
        group: GroupWire,
        scale: Pt,
        amount: Amount,
        keywords: Vec<String>,
    },
    AddExperienceCounter {},
    GrantControllerFlashUntilCleanup {},
    RequireOpponentsCreaturesBlockThisTurn {},
    RequireBlocksAgainstTargetThisTurn {},
    SuppressGroupKeywords {
        group: GroupWire,
        keywords: Vec<String>,
    },
    ModifyGroupUntilEndOfTurn {
        group: GroupWire,
        #[serde(deserialize_with = "required_nullable")]
        power_toughness: Option<Pt>,
        keywords: Vec<String>,
    },
    BoostSource {
        #[serde(deserialize_with = "required_nullable")]
        power_toughness: Option<Pt>,
        keywords: Vec<String>,
    },
    BoostLoneAttacker {},
    Equip {},
    Fortify {},
    Reconfigure {},
    Unattach {},
    PutCountersOnSource {
        amount: RulesInteger,
    },
    ResolvePowerAbility {
        kind: String,
    },
    ExileTarget {},
    ExileAbilitySource {},
    ExileAllGraveyards {},
    ShuffleOwnLibrary {},
    ShuffleEachHandAndGraveyard {},
    EndControllerTurn {},
    ShuffleOwnGraveyardIntoLibrary {},
    ReturnTargetGraveyardCreatureToHand {},
    ReturnRandomGraveyardCards {
        count: std::num::NonZeroU8,
        #[serde(default)]
        to_battlefield: bool,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        subtype: Option<Value>,
    },
    ChooseRevealedHandCard {
        minimum_mana_value: u16,
        #[serde(default)]
        count: Option<std::num::NonZeroU8>,
        #[serde(default)]
        nonland_only: bool,
        exile: bool,
    },
    ExileGraveyardTarget {},
    ExileCardsFromGraveyard {
        player: ResolvingPlayerWire,
        count: std::num::NonZeroU8,
    },
    PutTargetInLibrary {
        placement: String,
    },
    PutOwnHandCardOnLibraryBottom {},
    PutGroupOnLibraryBottom {
        group: GroupWire,
    },
    ReturnSourceGraveyardToBattlefield {
        tapped: bool,
    },
    ReturnSourceGraveyardToHand {},
    ExileTargetUntilSourceLeaves {},
    ExileUntilNextEndStep {},
    ReturnToOwnersHand {},
    ReturnOwnGraveyardCreatureToBattlefield {},
    ReturnOwnGraveyardPermanentToBattlefield {
        tapped: bool,
    },
    ReturnAnyGraveyardCreatureOrPlaneswalker {},
    CounterSpell {},
    CopyAbility {},
    WatchNextLoyaltyCopy {},
    Myriad {},
    RequireCastFromGraveyard {},
    WatchNextSpellOrLoyaltyTwice {},
    CopySpell {},
    CopyTargetToSource {
        duration: Duration,
        #[serde(deserialize_with = "required_nullable")]
        power_toughness: Option<Pt>,
    },
    MayRevealLibraryTop {},
    BoostTargetPower {
        amount: Amount,
        keywords: Vec<String>,
        #[serde(default)]
        also_toughness: bool,
    },
    CreateContinuousEffect {
        parts: Vec<PartWire<InstructionOperand, RulesInteger>>,
        duration: Duration,
    },
    PutIndestructibleCounter {},
    PutLoyaltyCounter {},
    GrantProtectionChosenColor {
        #[serde(default)]
        for_controller_and_permanents: bool,
    },
    PutPowerToughnessCounters {
        #[serde(deserialize_with = "object")]
        counters: Counters,
    },
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum ResolvingPlayerWire {
    HighestEntrantController,
    CombatDamageSourceController,
    TriggeringAttackerController,
    Controller,
    UniqueRelatedPlayer,
    TargetSpellController,
    TargetPlayer,
    TargetController,
    TargetOwner,
    ExiledTargetController,
    DestructionTargetController,
}
impl TryFrom<String> for ResolvingPlayerWire {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "TriggeringAttackerController" => Ok(Self::TriggeringAttackerController),
            "Controller" => Ok(Self::Controller),
            "HighestEntrantController" => Ok(Self::HighestEntrantController),
            "CombatDamageSourceController" => Ok(Self::CombatDamageSourceController),
            "UniqueRelatedPlayer" => Ok(Self::UniqueRelatedPlayer),
            "TargetSpellController" => Ok(Self::TargetSpellController),
            "TargetPlayer" => Ok(Self::TargetPlayer),
            "TargetController" => Ok(Self::TargetController),
            "TargetOwner" => Ok(Self::TargetOwner),
            "DestructionTargetController" => Ok(Self::DestructionTargetController),
            "ExiledTargetController" => Ok(Self::ExiledTargetController),
            _ => Err(format!("unknown resolving player {value}")),
        }
    }
}
impl From<ResolvingPlayerWire> for ResolvingPlayer {
    fn from(value: ResolvingPlayerWire) -> Self {
        match value {
            ResolvingPlayerWire::TriggeringAttackerController => Self::TriggeringAttackerController,
            ResolvingPlayerWire::Controller => Self::Controller,
            ResolvingPlayerWire::HighestEntrantController => Self::HighestEntrantController,
            ResolvingPlayerWire::CombatDamageSourceController => Self::CombatDamageSourceController,
            ResolvingPlayerWire::UniqueRelatedPlayer => Self::UniqueRelatedPlayer,
            ResolvingPlayerWire::TargetSpellController => Self::TargetSpellController,
            ResolvingPlayerWire::TargetPlayer => Self::TargetPlayer,
            ResolvingPlayerWire::TargetController => Self::TargetController,
            ResolvingPlayerWire::TargetOwner => Self::TargetOwner,
            ResolvingPlayerWire::DestructionTargetController => Self::DestructionTargetController,
            ResolvingPlayerWire::ExiledTargetController => Self::ExiledTargetController,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum AffectedPlayersWire {
    EachOtherPlayer {},
    EachOpponent {},
    One { player: ResolvingPlayerWire },
    EachPlayer {},
}
impl From<AffectedPlayersWire> for AffectedPlayers {
    fn from(value: AffectedPlayersWire) -> Self {
        match value {
            AffectedPlayersWire::One { player } => Self::One(player.into()),
            AffectedPlayersWire::EachOtherPlayer {} => Self::EachOtherPlayer,
            AffectedPlayersWire::EachOpponent {} => Self::EachOpponent,
            AffectedPlayersWire::EachPlayer {} => Self::EachPlayer,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermanentTypeFilterWire {
    #[serde(default)]
    any_types: Vec<String>,
    required_types: Vec<String>,
    forbidden_types: Vec<String>,
}
impl PermanentTypeFilterWire {
    fn native(self) -> Result<PermanentTypeFilter, String> {
        Ok(PermanentTypeFilter {
            any_types: card_types(self.any_types)?,
            required_types: card_types(self.required_types)?,
            forbidden_types: card_types(self.forbidden_types)?,
            object_filter: None,
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
enum ObjectStatWire {
    Power,
    Toughness,
    ManaValue,
    Loyalty,
}

impl From<ObjectStatWire> for ObjectStatKind {
    fn from(value: ObjectStatWire) -> Self {
        match value {
            ObjectStatWire::Power => Self::Power,
            ObjectStatWire::Toughness => Self::Toughness,
            ObjectStatWire::ManaValue => Self::ManaValue,
            ObjectStatWire::Loyalty => Self::Loyalty,
        }
    }
}

// This codec's group vocabulary is deliberately bounded. Do not use the broader
// legacy projection when a subtype match would discard additional filter fields.
fn distinct_stat_filter_wire(filter: ObjectFilter) -> Value {
    let mut exact = [
        ObjectFilter::creature(),
        ObjectFilter::creature().controlled(),
        ObjectFilter::creature().opposing(),
        ObjectFilter::creature()
            .controlled()
            .other_than_ability_source(),
        ObjectFilter::creature().controlled().attacking(),
        ObjectFilter::creature()
            .controlled()
            .with_power(PowerFilter::AtLeast(4)),
    ]
    .contains(&filter);
    if let ObjectFilter::Clause(clause) = filter
        && let SubtypeFilter::Has(kind) = clause.subtypes
    {
        exact |= [
            ObjectFilter::creature().with_subtype(kind),
            ObjectFilter::creature().controlled().with_subtype(kind),
            ObjectFilter::creature()
                .controlled()
                .other_than_ability_source()
                .with_subtype(kind),
        ]
        .contains(&filter);
    }
    if exact {
        effect_group_wire(spellwake_engine::program::EffectSubject::Matching(filter))
    } else {
        json!(format!("unsupported distinct-stat filter: {filter:?}"))
    }
}

fn distinct_stat_value(
    group: &GroupWire,
    stat: ObjectStatWire,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, String> {
    let spellwake_engine::program::EffectSubject::Matching(filter) =
        effect_group(group, vocabulary)?
    else {
        return Err("distinct statistics require a matching object filter".into());
    };
    Ok(ValueExpr::aggregate(
        filter,
        stat.into(),
        StatAggregate::CountDistinct,
    ))
}

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ConditionValueWire {
    DistinctStatValues {
        group: GroupWire,
        stat: ObjectStatWire,
    },
    PaidAlternative,
    ChosenX,
    CastDuringOwnMain,
    CastFromGraveyard,
    DeclaredAttackersAtController,
    Bargained,
    KickerCount {
        index: usize,
    },
    Literal {
        value: RulesInteger,
    },
    ControllerLife {},
    PlayerIsActivePlayer {
        player: ResolvingPlayerWire,
    },
    ControllerLifeAboveStarting {},
    RevealedTopHasType {
        first: String,
        second: String,
    },
    ControllerExperienceCounters {},
    TargetGraveyardCardManaValue {},
    CreaturesDiedThisTurn {},
    LastSacrificeSucceeded {},
    DiscardedCardOfType {
        kind: String,
    },
    DestructionTargetHadSubtype {
        subtype: Value,
    },
    SourceAttackedThisTurn {},
    SelectedGraveyardExileCreatures {},
    ControllerGraveyardSize {},
    DyingCreatureHasSubtype {
        subtype: Value,
    },
    SacrificedCreatureHadSubtype {
        subtype: Value,
    },
    TargetCreatureHasSubtype {
        subtype: Value,
    },
    TriggerSourceOnBattlefield {},
    GreatestControlledCreaturePower {},
    ControlsGreatestCreature {},
    ControlledPermanentsOfSubtype {
        subtype: Value,
    },
    ControlledPermanentsOfType {
        kind: String,
    },
    TotalControlledCreaturePower {},
    ControllerIsMonarch {},
    MatchedOccurrences {},
}
impl ConditionValueWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ValueExpr, String> {
        Ok(match self {
            Self::DistinctStatValues { group, stat } => {
                distinct_stat_value(&group, stat, vocabulary)?
            }
            Self::Literal { value } => ValueExpr::Literal(value),
            Self::PaidAlternative => ValueExpr::event(EventValueKind::PaidAlternative),
            Self::ChosenX => ValueExpr::chosen_x(),
            Self::CastDuringOwnMain => ValueExpr::event(EventValueKind::CastDuringOwnMain),
            Self::CastFromGraveyard => ValueExpr::event(EventValueKind::CastFromGraveyard),
            Self::DeclaredAttackersAtController => {
                ValueExpr::event(EventValueKind::DeclaredAttackersAtController)
            }
            Self::Bargained => ValueExpr::event(EventValueKind::Bargained),
            Self::KickerCount { index } => ValueExpr::event(EventValueKind::KickerCount(index)),
            Self::PlayerIsActivePlayer { player } => {
                ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(player.into()))
            }
            Self::ControllerLife {} => ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
            Self::ControllerLifeAboveStarting {} => {
                ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeAboveStarting)
            }
            Self::RevealedTopHasType { first, second } => ValueExpr::object_matches(
                MatchObjectRef::RevealedTop,
                ObjectFilter::any_types(
                    CardTypeMask::from_card_type(card_type(&first)?)
                        .union(CardTypeMask::from_card_type(card_type(&second)?)),
                ),
            ),
            Self::ControllerExperienceCounters {} => {
                ValueExpr::counters(CounterSubjectRef::ControllerExperience, None)
            }
            Self::TargetGraveyardCardManaValue {} => {
                ValueExpr::object_stat(ValueObjectRef::TargetObject, ObjectStatKind::ManaValue)
            }
            Self::CreaturesDiedThisTurn {} => {
                ValueExpr::event(EventValueKind::CreaturesDiedThisTurn)
            }
            Self::DestructionTargetHadSubtype { subtype } => ValueExpr::object_matches(
                MatchObjectRef::DestructionTarget,
                ObjectFilter::any().with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::DiscardedCardOfType { kind } => ValueExpr::object_matches(
                MatchObjectRef::DiscardedCard,
                ObjectFilter::any_types(CardTypeMask::from_card_type(card_type(&kind)?)),
            ),
            Self::SourceAttackedThisTurn {} => {
                ValueExpr::event(EventValueKind::SourceAttackedThisTurn)
            }
            Self::LastSacrificeSucceeded {} => {
                ValueExpr::event(EventValueKind::LastSacrificeSucceeded)
            }
            Self::SelectedGraveyardExileCreatures {} => {
                ValueExpr::event(EventValueKind::SelectedGraveyardExileCreatures)
            }
            Self::ControllerGraveyardSize {} => {
                ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::any())
            }
            Self::TriggerSourceOnBattlefield {} => {
                ValueExpr::event(EventValueKind::TriggerSourceOnBattlefield)
            }
            Self::ControlsGreatestCreature {} => {
                ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestCreature)
            }
            Self::ControlledPermanentsOfSubtype { subtype } => {
                ValueExpr::controlled_subtype(creature_types::id(subtype, vocabulary)?)
            }
            Self::GreatestControlledCreaturePower {} => ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::Max,
            ),
            Self::ControlledPermanentsOfType { kind } => {
                ValueExpr::controlled_type(card_type(&kind)?)
            }
            Self::TotalControlledCreaturePower {} => ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::Sum,
            ),
            Self::ControllerIsMonarch {} => {
                ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch)
            }
            Self::TargetCreatureHasSubtype { subtype } => ValueExpr::object_matches(
                MatchObjectRef::TargetObject,
                ObjectFilter::creature().with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::SacrificedCreatureHadSubtype { subtype } => ValueExpr::object_matches(
                MatchObjectRef::SacrificedCreature,
                ObjectFilter::creature().with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::DyingCreatureHasSubtype { subtype } => ValueExpr::object_matches(
                MatchObjectRef::DyingCreature,
                ObjectFilter::creature().with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::MatchedOccurrences {} => ValueExpr::event(EventValueKind::MatchedOccurrences),
        })
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
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ResolutionCondition, String> {
        Ok(ResolutionCondition {
            left: self.left.native(vocabulary)?,
            comparison: match self.comparison {
                ComparisonWire::Equal => Comparison::Equal,
                ComparisonWire::NotEqual => Comparison::NotEqual,
                ComparisonWire::Less => Comparison::Less,
                ComparisonWire::LessOrEqual => Comparison::LessOrEqual,
                ComparisonWire::Greater => Comparison::Greater,
                ComparisonWire::GreaterOrEqual => Comparison::GreaterOrEqual,
            },
            right: self.right.native(vocabulary)?,
        })
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
    DistinctStatValues {
        group: GroupWire,
        stat: ObjectStatWire,
    },
    PlayerIsActivePlayer {
        player: ResolvingPlayerWire,
    },
    AbilitySourceCounters {
        kind: String,
    },
    AbilitySourcePower {},
    GreatestControlledCreaturePower {},
    TargetCreaturePower {},
    TargetCreatureToughness {},
    TriggeringLifeChange {},
    OtherControlledCreaturesTimes {
        multiplier: u16,
    },
    OpponentsLifeLostThisTurn {},
    CardsShortOfHandSize {
        cards: u8,
    },
    LargestHandSize {},
    TargetPlayerHandSize {},
    PaidCreatureTapsTimes {
        factor: u16,
    },
    PaidNamedCounters {},
    TargetPlayerTappedCreatures {},
    TriggeringSpellManaValue {},
    FinalSagaManaValue {},
    CardsDrawnThisTurn {},
    SacrificedCreaturePower {},
    SacrificedCreatureToughness {},
    LastSacrificedCreatureToughness {},
    LastSacrificedCreaturePower {},
    SacrificedPermanentManaValue {},
    TriggerSourcePower {},
    TriggerSourcePlusOneCounters {},
    DyingSourcePower {},
    DyingCreatureToughness {},
    DestructionTargetToughness {},
    ExiledCostCreaturePower {},
    GreatestOtherControlledCreaturePower {},
    OtherControlledCreaturesSharingSourceType {},
    OneOrTwoByDyingPower {
        minimum: u16,
    },
    LargestDiscard {},
    DestroyedPermanents {},
    ExiledMilledCreaturePower {},
    MilledCardsManaValue {},
    HighestMilledManaValue {},
    EnteredKickerX {},
    DeclaredSubtypeAttackers {},
    AurasAndEquipmentAttachedToSource {},
    EquipmentAttachedToSource {},
    AllCountersOnDyingSource {},
    PlusOneCountersOnDyingSource {},
    CombatDamageToPlayer {},
    DamageReceived {},
    CursesAttachedToRelatedPlayer {},
    AttackingCreatures {},
    CreaturesAttackingController {},
    ChosenX {},
    ChosenXTimes {
        multiplier: std::num::NonZeroU16,
    },
    ControlledPermanentsOfSubtype {
        kind: Value,
    },
    HalfControlledPermanentsOfSubtype {
        kind: Value,
    },
    ControlledPermanentsOfType {
        card_type: String,
    },
    ControlledColorlessCreatures {},
    OwnGraveyardCreatures {},
    OwnGraveyardInstantsOrSorceries {},
    ControlledCreaturesOfColor {
        color: String,
    },
    ControlledCreaturesWithPlusOneCounters {},
    Literal {
        value: RulesInteger,
    },
    ControlledPermanentsOfChosenType {
        chosen: Value,
    },
    MatchedOccurrences {},
}

impl Amount {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ValueExpr, String> {
        Ok(match self {
            Self::DistinctStatValues { group, stat } => {
                distinct_stat_value(&group, stat, vocabulary)?
            }
            Self::PlayerIsActivePlayer { player } => {
                ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(player.into()))
            }
            Self::AbilitySourceCounters { kind } => ValueExpr::counters(
                CounterSubjectRef::AbilitySource,
                Some(
                    spellwake_engine::effects::CounterKind::PERMANENT
                        .into_iter()
                        .find(|value| format!("{value:?}") == kind)
                        .ok_or("unknown permanent counter")?,
                ),
            ),
            Self::AbilitySourcePower {} => {
                ValueExpr::object_stat(ValueObjectRef::AbilitySource, ObjectStatKind::Power)
            }
            Self::GreatestControlledCreaturePower {} => ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::Max,
            ),
            Self::TargetCreaturePower {} => {
                ValueExpr::object_stat(ValueObjectRef::TargetObject, ObjectStatKind::Power)
            }
            Self::TargetCreatureToughness {} => {
                ValueExpr::object_stat(ValueObjectRef::TargetObject, ObjectStatKind::Toughness)
            }
            Self::TriggeringLifeChange {} => ValueExpr::event(EventValueKind::TriggeringLifeChange),
            Self::OtherControlledCreaturesTimes { multiplier } => ValueExpr::count_battlefield(
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
            )
            .scaled(i64::from(multiplier)),
            Self::HalfControlledPermanentsOfSubtype { kind } => {
                ValueExpr::controlled_subtype(creature_types::id(kind, vocabulary)?).half_down()
            }
            Self::ControlledPermanentsOfSubtype { kind } => {
                ValueExpr::controlled_subtype(creature_types::id(kind, vocabulary)?)
            }
            Self::ControlledPermanentsOfType { card_type: kind } => {
                ValueExpr::controlled_type(card_type(&kind)?)
            }
            Self::ControlledColorlessCreatures {} => ValueExpr::count_battlefield(
                ObjectFilter::creature()
                    .controlled()
                    .with_color(ObjectColorFilter::Colorless),
            ),
            Self::OwnGraveyardCreatures {} => {
                ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::creature())
            }
            Self::OwnGraveyardInstantsOrSorceries {} => ValueExpr::count_graveyard(
                ResolvingPlayer::Controller,
                ObjectFilter::instant_or_sorcery(),
            ),
            Self::ControlledCreaturesOfColor { color: shade } => ValueExpr::count_battlefield(
                ObjectFilter::creature()
                    .controlled()
                    .with_color(ObjectColorFilter::has(color(&shade)?)),
            ),
            Self::ControlledCreaturesWithPlusOneCounters {} => ValueExpr::count_battlefield(
                ObjectFilter::creature()
                    .controlled()
                    .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER),
            ),
            Self::Literal { value } => ValueExpr::Literal(value),
            Self::CreaturesAttackingController {} => ValueExpr::CountAttackingController,
            Self::AttackingCreatures {} => {
                ValueExpr::count_battlefield(ObjectFilter::creature().attacking())
            }
            Self::OpponentsLifeLostThisTurn {} => {
                ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn)
            }
            Self::CardsShortOfHandSize { cards } => {
                ValueExpr::PlayerStat(PlayerStatExpr::CardsShortOfHandSize(cards))
            }
            Self::TargetPlayerTappedCreatures {} => ValueExpr::count_player_battlefield(
                ResolvingPlayer::TargetPlayer,
                ObjectFilter::creature().tapped(),
            ),
            Self::PaidNamedCounters {} => ValueExpr::event(EventValueKind::PaidNamedCounters),
            Self::PaidCreatureTapsTimes { factor } => {
                ValueExpr::event(EventValueKind::PaidCreatureTaps).scaled(i64::from(factor))
            }
            Self::TargetPlayerHandSize {} => ValueExpr::count_hand(ResolvingPlayer::TargetPlayer),
            Self::LargestHandSize {} => ValueExpr::PlayerStat(PlayerStatExpr::LargestHandSize),
            Self::FinalSagaManaValue {} => {
                ValueExpr::object_stat(ValueObjectRef::FinalSaga, ObjectStatKind::ManaValue)
            }
            Self::TriggeringSpellManaValue {} => {
                ValueExpr::object_stat(ValueObjectRef::TriggeringSpell, ObjectStatKind::ManaValue)
            }
            Self::CardsDrawnThisTurn {} => {
                ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn)
            }
            Self::SacrificedPermanentManaValue {} => ValueExpr::object_stat(
                ValueObjectRef::SacrificedPermanent,
                ObjectStatKind::ManaValue,
            ),
            Self::SacrificedCreaturePower {} => {
                ValueExpr::object_stat(ValueObjectRef::SacrificedCreature, ObjectStatKind::Power)
            }
            Self::LastSacrificedCreatureToughness {} => ValueExpr::object_stat(
                ValueObjectRef::LastSacrificedCreature,
                ObjectStatKind::Toughness,
            ),
            Self::LastSacrificedCreaturePower {} => ValueExpr::object_stat(
                ValueObjectRef::LastSacrificedCreature,
                ObjectStatKind::Power,
            ),
            Self::SacrificedCreatureToughness {} => ValueExpr::object_stat(
                ValueObjectRef::SacrificedCreature,
                ObjectStatKind::Toughness,
            ),
            Self::TriggerSourcePlusOneCounters {} => ValueExpr::counters(
                CounterSubjectRef::TriggerSource,
                Some(spellwake_engine::effects::CounterKind::PlusOne),
            ),
            Self::TriggerSourcePower {} => {
                ValueExpr::object_stat(ValueObjectRef::TriggerSource, ObjectStatKind::Power)
            }
            Self::DyingSourcePower {} => {
                ValueExpr::object_stat(ValueObjectRef::DyingSource, ObjectStatKind::Power)
            }
            Self::ExiledCostCreaturePower {} => {
                ValueExpr::object_stat(ValueObjectRef::ExiledCostCreature, ObjectStatKind::Power)
            }
            Self::DestructionTargetToughness {} => {
                ValueExpr::object_stat(ValueObjectRef::DestructionTarget, ObjectStatKind::Toughness)
            }
            Self::DyingCreatureToughness {} => {
                ValueExpr::object_stat(ValueObjectRef::DyingCreature, ObjectStatKind::Toughness)
            }
            Self::OtherControlledCreaturesSharingSourceType {} => {
                ValueExpr::CountOtherSharingSourceSubtype
            }
            Self::GreatestOtherControlledCreaturePower {} => ValueExpr::aggregate(
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
                ObjectStatKind::Power,
                StatAggregate::Max,
            ),
            Self::OneOrTwoByDyingPower { minimum } => ValueExpr::conditional(
                ValueExpr::object_stat(ValueObjectRef::DyingCreature, ObjectStatKind::Power),
                Comparison::GreaterOrEqual,
                ValueExpr::literal(i64::from(minimum)),
                ValueExpr::literal(2),
                ValueExpr::literal(1),
            ),
            Self::LargestDiscard {} => ValueExpr::event(EventValueKind::LargestDiscard),
            Self::DeclaredSubtypeAttackers {} => {
                ValueExpr::event(EventValueKind::DeclaredSubtypeAttackers)
            }
            Self::EnteredKickerX {} => ValueExpr::event(EventValueKind::EnteredKickerX),
            Self::MilledCardsManaValue {} => {
                ValueExpr::event(EventValueKind::MilledCardsManaValueSum)
            }
            Self::HighestMilledManaValue {} => {
                ValueExpr::event(EventValueKind::MilledCardsManaValueMax)
            }
            Self::ExiledMilledCreaturePower {} => {
                ValueExpr::event(EventValueKind::ExiledMilledCreaturePower)
            }
            Self::DestroyedPermanents {} => ValueExpr::event(EventValueKind::DestroyedPermanents),
            Self::AurasAndEquipmentAttachedToSource {} => {
                ValueExpr::AttachedCount(AttachedCountKind::AurasAndEquipmentOnSource)
            }
            Self::EquipmentAttachedToSource {} => {
                ValueExpr::AttachedCount(AttachedCountKind::EquipmentOnSource)
            }
            Self::AllCountersOnDyingSource {} => {
                ValueExpr::counters(CounterSubjectRef::DyingSource, None)
            }
            Self::PlusOneCountersOnDyingSource {} => ValueExpr::counters(
                CounterSubjectRef::DyingSource,
                Some(spellwake_engine::effects::CounterKind::PlusOne),
            ),
            Self::CombatDamageToPlayer {} => ValueExpr::event(EventValueKind::CombatDamageToPlayer),
            Self::DamageReceived {} => ValueExpr::event(EventValueKind::DamageReceived),
            Self::CursesAttachedToRelatedPlayer {} => {
                ValueExpr::AttachedCount(AttachedCountKind::CursesOnRelatedPlayer)
            }
            Self::ChosenX {} => ValueExpr::chosen_x(),
            Self::ChosenXTimes { multiplier } => {
                ValueExpr::chosen_x().scaled(i64::from(multiplier.get()))
            }
            Self::MatchedOccurrences {} => ValueExpr::event(EventValueKind::MatchedOccurrences),
            Self::ControlledPermanentsOfChosenType { chosen } => ValueExpr::count_battlefield(
                ObjectFilter::any()
                    .controlled()
                    .of_chosen_slot(creature_types::slot(chosen)?),
            ),
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
    SourceEntryBasicLandType {},
}
impl LandOperand {
    fn literal(self) -> Result<BTreeSet<LandType>, String> {
        match self {
            Self::Literal { types } => land_types(types),
            Self::SourceCurrentLandSubtypes {} | Self::SourceEntryBasicLandType {} => {
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
            Self::SourceEntryBasicLandType {} => StaticLandSubtypeOperand::SourceEntryBasicLandType,
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
    SourceEntryCreatureType {},
    ImprintedCreatureTypes { keep: Vec<Value> },
}
impl StaticOperand {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<StaticSubtypeOperand, String> {
        Ok(match self {
            Self::Literal { types } => {
                StaticSubtypeOperand::Literal(subtype_set(types, vocabulary, "creature subtype")?)
            }
            Self::SourceEntryCreatureType {} => StaticSubtypeOperand::SourceEntryCreatureType,
            Self::SourceCurrentSubtypes {} => StaticSubtypeOperand::SourceCurrentSubtypes,
            Self::ImprintedCreatureTypes { keep } => StaticSubtypeOperand::ImprintedCreatureTypes {
                keep: subtype_set(keep, vocabulary, "creature subtype")?,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum AbilityRemovalWire {
    Flying,
    Banding,
    BandsWithOther,
    All,
    NonMana,
}
impl TryFrom<String> for AbilityRemovalWire {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "All" => Ok(Self::All),
            "NonMana" => Ok(Self::NonMana),
            "Flying" => Ok(Self::Flying),
            "Banding" => Ok(Self::Banding),
            "BandsWithOther" => Ok(Self::BandsWithOther),
            _ => Err(format!("unknown ability removal {value}")),
        }
    }
}
impl AbilityRemovalWire {
    fn native(self) -> AbilityRemoval {
        match self {
            Self::All => AbilityRemoval::All,
            Self::NonMana => AbilityRemoval::NonMana,
            Self::Flying => AbilityRemoval::Flying,
            Self::Banding => AbilityRemoval::Banding,
            Self::BandsWithOther => AbilityRemoval::BandsWithOther,
        }
    }
}
fn keyword_set(labels: Vec<String>) -> Result<BTreeSet<KeywordAbility>, String> {
    let mut keywords = BTreeSet::new();
    for label in labels {
        let keyword = match label.as_str() {
            "Haste" => KeywordAbility::Haste,
            "Bargain" => KeywordAbility::Bargain,
            "Partner" => KeywordAbility::Partner,
            "BandsWithOther(LegendaryCreatures)" => KeywordAbility::BandsWithOther(
                spellwake_engine::combat::BandQuality::LegendaryCreatures,
            ),
            "BandsWithOther(CreaturesNamedWolvesOfTheHunt)" => KeywordAbility::BandsWithOther(
                spellwake_engine::combat::BandQuality::CreaturesNamedWolvesOfTheHunt,
            ),
            "Banding" => KeywordAbility::Banding,
            "Vigilance" => KeywordAbility::Vigilance,
            "Flying" => KeywordAbility::Flying,
            "Reach" => KeywordAbility::Reach,
            "Decayed" => KeywordAbility::Decayed,
            "Daybound" => KeywordAbility::Daybound,
            "Nightbound" => KeywordAbility::Nightbound,
            "Phasing" => KeywordAbility::Phasing,
            "Defender" => KeywordAbility::Defender,
            "Menace" => KeywordAbility::Menace,
            "Shadow" => KeywordAbility::Shadow,
            "Horsemanship" => KeywordAbility::Horsemanship,
            "Fear" => KeywordAbility::Fear,
            "Intimidate" => KeywordAbility::Intimidate,
            "Skulk" => KeywordAbility::Skulk,
            "Plainswalk" => KeywordAbility::Plainswalk,
            "Islandwalk" => KeywordAbility::Islandwalk,
            "Swampwalk" => KeywordAbility::Swampwalk,
            "Mountainwalk" => KeywordAbility::Mountainwalk,
            "Forestwalk" => KeywordAbility::Forestwalk,
            "Desertwalk" => KeywordAbility::Desertwalk,
            "NonbasicLandwalk" => KeywordAbility::NonbasicLandwalk,
            "LegendaryLandwalk" => KeywordAbility::LegendaryLandwalk,
            "SnowLandwalk" => KeywordAbility::SnowLandwalk,
            "SnowForestwalk" => KeywordAbility::SnowForestwalk,
            "SnowSwampwalk" => KeywordAbility::SnowSwampwalk,
            "ArtifactLandwalk" => KeywordAbility::ArtifactLandwalk,
            "Trample" => KeywordAbility::Trample,
            "Deathtouch" => KeywordAbility::Deathtouch,
            "Convoke" => KeywordAbility::Convoke,
            "Delve" => KeywordAbility::Delve,
            "Improvise" => KeywordAbility::Improvise,
            "ProtectionFromWhite" => KeywordAbility::ProtectionFromWhite,
            "ProtectionFromBlue" => KeywordAbility::ProtectionFromBlue,
            "ProtectionFromBlack" => KeywordAbility::ProtectionFromBlack,
            "ProtectionFromRed" => KeywordAbility::ProtectionFromRed,
            "DamageCannotBePrevented" => KeywordAbility::DamageCannotBePrevented,
            "ProtectionFromMulticolored" => KeywordAbility::ProtectionFromMulticolored,
            "ProtectionFromGreen" => KeywordAbility::ProtectionFromGreen,
            "Retrace" => KeywordAbility::Retrace,
            "JumpStart" => KeywordAbility::JumpStart,
            "Rebound" => KeywordAbility::Rebound,
            "Lifelink" => KeywordAbility::Lifelink,
            "Indestructible" => KeywordAbility::Indestructible,
            "Infect" => KeywordAbility::Infect,
            "Wither" => KeywordAbility::Wither,
            "FirstStrike" => KeywordAbility::FirstStrike,
            "DoubleStrike" => KeywordAbility::DoubleStrike,
            "Shroud" => KeywordAbility::Shroud,
            "Hexproof" => KeywordAbility::Hexproof,
            "Flash" => KeywordAbility::Flash,
            "SplitSecond" => KeywordAbility::SplitSecond,
            "Changeling" => KeywordAbility::Changeling,
            "StartYourEngines" => KeywordAbility::StartYourEngines,
            "Devoid" => KeywordAbility::Devoid,
            _ => return Err(format!("unsupported keyword {label}")),
        };
        if !keywords.insert(keyword) {
            return Err(format!("duplicate keyword {label}"));
        }
    }
    Ok(keywords)
}

/// Encode the ordered, finite native keyword set.
pub fn keywords(value: &BTreeSet<KeywordAbility>) -> Value {
    json!(
        value
            .iter()
            .map(|keyword| format!("{keyword:?}"))
            .collect::<Vec<_>>()
    )
}

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum CombatKeywordWire {
    Afflict { amount: RulesInteger },
    Dethrone {},
    Melee {},
    Renown { amount: RulesInteger },
    Training {},
    Prowess {},
    BattleCry {},
    Flanking {},
    Bushido { amount: RulesInteger },
    Decayed {},
    Mobilize { amount: RulesInteger },
    Ingest {},
    Poisonous { amount: RulesInteger },
    Frenzy { amount: RulesInteger },
    Rampage { amount: RulesInteger },
}
impl CombatKeywordWire {
    fn native(self) -> CombatTriggerKeyword {
        match self {
            Self::Afflict { amount } => CombatTriggerKeyword::Afflict(amount),
            Self::Dethrone {} => CombatTriggerKeyword::Dethrone,
            Self::Melee {} => CombatTriggerKeyword::Melee,
            Self::Renown { amount } => CombatTriggerKeyword::Renown(amount),
            Self::Training {} => CombatTriggerKeyword::Training,
            Self::Prowess {} => CombatTriggerKeyword::Prowess,
            Self::BattleCry {} => CombatTriggerKeyword::BattleCry,
            Self::Flanking {} => CombatTriggerKeyword::Flanking,
            Self::Bushido { amount } => CombatTriggerKeyword::Bushido(amount),
            Self::Decayed {} => CombatTriggerKeyword::Decayed,
            Self::Mobilize { amount } => CombatTriggerKeyword::Mobilize(amount),
            Self::Ingest {} => CombatTriggerKeyword::Ingest,
            Self::Poisonous { amount } => CombatTriggerKeyword::Poisonous(amount),
            Self::Frenzy { amount } => CombatTriggerKeyword::Frenzy(amount),
            Self::Rampage { amount } => CombatTriggerKeyword::Rampage(amount),
        }
    }
}
pub fn combat_keyword(keyword: &CombatTriggerKeyword) -> Value {
    match keyword {
        CombatTriggerKeyword::Afflict(amount) => json!({"tag":"Afflict","amount":amount}),
        CombatTriggerKeyword::Dethrone => json!({"tag":"Dethrone"}),
        CombatTriggerKeyword::Melee => json!({"tag":"Melee"}),
        CombatTriggerKeyword::Renown(amount) => json!({"tag":"Renown","amount":amount}),
        CombatTriggerKeyword::Training => json!({"tag":"Training"}),
        CombatTriggerKeyword::Prowess => json!({"tag":"Prowess"}),
        CombatTriggerKeyword::BattleCry => json!({"tag":"BattleCry"}),
        CombatTriggerKeyword::Flanking => json!({"tag":"Flanking"}),
        CombatTriggerKeyword::Bushido(amount) => json!({"tag":"Bushido","amount":amount}),
        CombatTriggerKeyword::Decayed => json!({"tag":"Decayed"}),
        CombatTriggerKeyword::Mobilize(n) => json!({"tag":"Mobilize","amount":n}),
        CombatTriggerKeyword::Ingest => json!({"tag":"Ingest"}),
        CombatTriggerKeyword::Poisonous(amount) => json!({"tag":"Poisonous","amount":amount}),
        CombatTriggerKeyword::Frenzy(amount) => json!({"tag":"Frenzy","amount":amount}),
        CombatTriggerKeyword::Rampage(amount) => json!({"tag":"Rampage","amount":amount}),
    }
}
fn ability_change(
    granted_toxic: Option<RulesInteger>,
    granted_combat_keywords: Vec<CombatKeywordWire>,
    granted_abilities: (bool, Vec<TriggeredAbility>),
    granted_exalted: u16,
    removal: Option<AbilityRemovalWire>,
    keywords: Vec<String>,
    suppressed: Vec<String>,
) -> Result<AbilityChange, String> {
    Ok(AbilityChange {
        removal: removal.map(AbilityRemovalWire::native),
        granted_keywords: keyword_set(keywords)?,
        suppressed_keywords: keyword_set(suppressed)?,
        grants_unblockable: granted_abilities.0,
        granted_triggers: granted_abilities.1,
        granted_exalted,
        granted_toxic,
        granted_combat_keywords: granted_combat_keywords
            .into_iter()
            .map(CombatKeywordWire::native)
            .collect(),
        suppressed_activations: None,
        doesnt_untap: false,
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
enum BlockingRestrictionWire {
    CannotBlockSubtype { kind: Value },
    CanBlockOnlyFlying {},
    CannotBeBlockedByFlying {},
    OnlyBlockedByFlyingOrReach {},
    CannotBeBlockedBySubtype { subtype: Value },
    BlockerPowerAtLeastSource {},
    AdditionalBlock {},
    RequiresSoulbondPartner {},

    CannotBeBlocked {},
    OnlyBlockedBySubtype { kind: Value },
    CannotBeBlockedByToughnessAtLeast { minimum: u16 },
    AtLeastThreeBlockers {},
    AllDefendingCreaturesMustBlock {},
    MustBeBlocked {},
    CannotAttackOrBlock {},
    CannotBlock {},
    CannotBeBlockedIfPowerOrToughnessAtMostOne {},
    MustAttackEachCombatIfAble {},
    CannotAttackOwner {},
    MustBeBlockedByTwoOrMore {},
    CannotCrewVehicles {},
}
impl BlockingRestrictionWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<BlockingRestriction, String> {
        Ok(match self {
            Self::CannotBlockSubtype { kind } => {
                BlockingRestriction::CannotBlockSubtype(creature_types::id(kind, vocabulary)?)
            }
            Self::CanBlockOnlyFlying {} => BlockingRestriction::CanBlockOnlyFlying,
            Self::CannotBeBlockedByFlying {} => BlockingRestriction::CannotBeBlockedByFlying,
            Self::OnlyBlockedByFlyingOrReach {} => BlockingRestriction::OnlyBlockedByFlyingOrReach,
            Self::CannotBeBlockedBySubtype { subtype } => {
                BlockingRestriction::CannotBeBlockedBySubtype(creature_types::id(
                    subtype, vocabulary,
                )?)
            }
            Self::BlockerPowerAtLeastSource {} => BlockingRestriction::BlockerPowerAtLeastSource,
            Self::AdditionalBlock {} => BlockingRestriction::AdditionalBlock,
            Self::RequiresSoulbondPartner {} => BlockingRestriction::RequiresSoulbondPartner,
            Self::CannotBeBlocked {} => BlockingRestriction::CannotBeBlocked,
            Self::CannotBeBlockedByToughnessAtLeast { minimum } => {
                BlockingRestriction::CannotBeBlockedByToughnessAtLeast(minimum)
            }
            Self::OnlyBlockedBySubtype { kind } => {
                BlockingRestriction::OnlyBlockedBySubtype(creature_types::id(kind, vocabulary)?)
            }
            Self::AtLeastThreeBlockers {} => BlockingRestriction::AtLeastThreeBlockers,
            Self::AllDefendingCreaturesMustBlock {} => {
                BlockingRestriction::AllDefendingCreaturesMustBlock
            }
            Self::MustBeBlocked {} => BlockingRestriction::MustBeBlocked,
            Self::CannotAttackOrBlock {} => BlockingRestriction::CannotAttackOrBlock,
            Self::CannotBlock {} => BlockingRestriction::CannotBlock,
            Self::CannotBeBlockedIfPowerOrToughnessAtMostOne {} => {
                BlockingRestriction::CannotBeBlockedIfPowerOrToughnessAtMostOne
            }
            Self::MustAttackEachCombatIfAble {} => BlockingRestriction::MustAttackEachCombatIfAble,
            Self::CannotAttackOwner {} => BlockingRestriction::CannotAttackOwner,
            Self::MustBeBlockedByTwoOrMore {} => BlockingRestriction::MustBeBlockedByTwoOrMore,
            Self::CannotCrewVehicles {} => BlockingRestriction::CannotCrewVehicles,
        })
    }
}
fn blocking_restriction(value: &BlockingRestriction) -> Value {
    let restriction = match value {
        BlockingRestriction::CannotBlockSubtype(kind) => {
            json!({"tag":"CannotBlockSubtype","kind":projection::creature_type_id(*kind)})
        }
        BlockingRestriction::BlockerPowerAtLeastPowerOf(source) => {
            json!({"tag":"BlockerPowerAtLeastPowerOf","source":projection::opaque(source)})
        }
        BlockingRestriction::CanBlockOnlyFlying => json!({"tag":"CanBlockOnlyFlying"}),
        BlockingRestriction::CannotBeBlockedByFlying => json!({"tag":"CannotBeBlockedByFlying"}),
        BlockingRestriction::CannotBeBlockedByGreaterPower => {
            json!({"tag":"CannotBeBlockedByGreaterPower"})
        }
        BlockingRestriction::OnlyBlockedByFlyingOrReach => {
            json!({"tag":"OnlyBlockedByFlyingOrReach"})
        }
        BlockingRestriction::CannotBeBlockedBySubtype(kind) => {
            json!({"tag":"CannotBeBlockedBySubtype","subtype":projection::creature_type_id(*kind)})
        }
        BlockingRestriction::BlockerPowerAtLeastSource => {
            json!({"tag":"BlockerPowerAtLeastSource"})
        }
        BlockingRestriction::MustBlockAllAttackers => {
            json!({"tag":"MustBlockAllAttackers"})
        }
        BlockingRestriction::AdditionalBlock => json!({"tag":"AdditionalBlock"}),
        BlockingRestriction::RequiresSoulbondPartner => json!({"tag":"RequiresSoulbondPartner"}),
        BlockingRestriction::CannotBeBlocked => json!({"tag":"CannotBeBlocked"}),
        BlockingRestriction::CannotBeBlockedByToughnessAtLeast(minimum) => {
            json!({"tag":"CannotBeBlockedByToughnessAtLeast","minimum":minimum})
        }
        BlockingRestriction::OnlyBlockedBySubtype(kind) => {
            json!({"tag":"OnlyBlockedBySubtype","kind":projection::creature_type_id(*kind)})
        }
        BlockingRestriction::AtLeastThreeBlockers => json!({"tag":"AtLeastThreeBlockers"}),
        BlockingRestriction::MustBeBlocked => json!({"tag":"MustBeBlocked"}),
        BlockingRestriction::AllDefendingCreaturesMustBlock => {
            json!({"tag":"AllDefendingCreaturesMustBlock"})
        }
        BlockingRestriction::CannotAttackOrBlock => json!({"tag":"CannotAttackOrBlock"}),
        BlockingRestriction::CannotAttack => json!({"tag":"CannotAttack"}),
        BlockingRestriction::CannotBlock => json!({"tag":"CannotBlock"}),
        BlockingRestriction::AtMostOneBlocker => json!({"tag":"AtMostOneBlocker"}),
        BlockingRestriction::CannotBeBlockedByPowerAtMost(maximum) => {
            json!({"tag":"CannotBeBlockedByPowerAtMost","maximum":maximum})
        }
        BlockingRestriction::CannotBeBlockedByPowerAtLeast(minimum) => {
            json!({"tag":"CannotBeBlockedByPowerAtLeast","minimum":minimum})
        }
        BlockingRestriction::CannotBeBlockedByColor(color) => {
            json!({"tag":"CannotBeBlockedByColor","color":format!("{color:?}")})
        }
        BlockingRestriction::OnlyBlockedByColor(color) => {
            json!({"tag":"OnlyBlockedByColor","color":format!("{color:?}")})
        }
        BlockingRestriction::OnlyBlockedByColorOrArtifact(color) => {
            json!({"tag":"OnlyBlockedByColorOrArtifact","color":format!("{color:?}")})
        }
        BlockingRestriction::CannotBeBlockedByTokens => json!({"tag":"CannotBeBlockedByTokens"}),
        BlockingRestriction::OnlyBlockedByArtifact => json!({"tag":"OnlyBlockedByArtifact"}),
        BlockingRestriction::CannotBeBlockedByNonArtifact => {
            json!({"tag":"CannotBeBlockedByNonArtifact"})
        }
        BlockingRestriction::CannotBeBlockedIfPowerOrToughnessAtMostOne => {
            json!({"tag":"CannotBeBlockedIfPowerOrToughnessAtMostOne"})
        }
        BlockingRestriction::CannotBlockMatching(filter) => {
            json!({"tag":"CannotBlockMatching","filter":format!("{filter:?}")})
        }
        BlockingRestriction::CannotBeBlockedByMatching(filter) => {
            json!({"tag":"CannotBeBlockedByMatching","filter":format!("{filter:?}")})
        }
        BlockingRestriction::OnlyBlockedByMatching(filter) => {
            json!({"tag":"OnlyBlockedByMatching","filter":format!("{filter:?}")})
        }
        BlockingRestriction::CannotAttackAlone => json!({"tag":"CannotAttackAlone"}),
        BlockingRestriction::CannotBlockAlone => json!({"tag":"CannotBlockAlone"}),
        BlockingRestriction::AssignDamageByToughness => {
            json!({"tag":"AssignDamageByToughness"})
        }
        BlockingRestriction::AssignDamageAsThoughUnblocked => {
            json!({"tag":"AssignDamageAsThoughUnblocked"})
        }
        BlockingRestriction::CanBlockAnyNumber => json!({"tag":"CanBlockAnyNumber"}),
        BlockingRestriction::CanAttackAsThoughNoDefender => {
            json!({"tag":"CanAttackAsThoughNoDefender"})
        }
        BlockingRestriction::MustBlockEachCombatIfAble => {
            json!({"tag":"MustBlockEachCombatIfAble"})
        }
        BlockingRestriction::MustAttackEachCombatIfAble => {
            json!({"tag":"MustAttackEachCombatIfAble"})
        }
        BlockingRestriction::CannotBlockSource => json!({"tag":"CannotBlockSource"}),
        BlockingRestriction::CannotBlockObject(id) => {
            json!({"tag":"CannotBlockObject","object":format!("{id:?}")})
        }
        BlockingRestriction::CannotAttackUnlessDefenderControls(filter) => {
            json!({"tag":"CannotAttackUnlessDefenderControls","filter":format!("{filter:?}")})
        }
        BlockingRestriction::MustBlockSource => json!({"tag":"MustBlockSource"}),
        BlockingRestriction::MustBlockObject(id) => {
            json!({"tag":"MustBlockObject","object":format!("{id:?}")})
        }
        BlockingRestriction::CannotBeBlockedBySource => json!({"tag":"CannotBeBlockedBySource"}),
        BlockingRestriction::CannotBeBlockedByObject(id) => {
            json!({"tag":"CannotBeBlockedByObject","object":format!("{id:?}")})
        }
        BlockingRestriction::CanBlockOnlyMatching(filter) => {
            json!({"tag":"CanBlockOnlyMatching","filter":format!("{filter:?}")})
        }
        BlockingRestriction::CannotAttackSourceController => {
            json!({"tag":"CannotAttackSourceController"})
        }
        BlockingRestriction::CannotAttackPlayer(seat) => {
            json!({"tag":"CannotAttackPlayer","player":format!("{seat:?}")})
        }
        BlockingRestriction::MustBeBlockedByAllMatching(filter) => {
            json!({"tag":"MustBeBlockedByAllMatching","filter":format!("{filter:?}")})
        }
        BlockingRestriction::CannotBlockMoreThan(n) => {
            json!({"tag":"CannotBlockMoreThan","max":n})
        }
        BlockingRestriction::CannotBeBlockedByMoreThan(n) => {
            json!({"tag":"CannotBeBlockedByMoreThan","max":n})
        }
        BlockingRestriction::AssignNoCombatDamage => json!({"tag":"AssignNoCombatDamage"}),
        BlockingRestriction::CannotBeRegenerated => json!({"tag":"CannotBeRegenerated"}),
        BlockingRestriction::CannotAttackOwner => json!({"tag":"CannotAttackOwner"}),
        BlockingRestriction::MustBeBlockedByTwoOrMore => {
            json!({"tag":"MustBeBlockedByTwoOrMore"})
        }
        BlockingRestriction::CannotCrewVehicles => json!({"tag":"CannotCrewVehicles"}),
    };
    json!({"tag":"BlockingRestriction", "restriction":restriction})
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum PartWire<T, N> {
    GrantTrigger {
        ability: Box<Trigger>,
    },
    GrantActivated {
        ability: Box<Activated>,
    },
    InheritCounteredCreatureAbilities,
    InheritExiledCreatureAbilities,
    GrantAnyColorMana {},
    GrantFixedColorMana {
        color: String,
    },
    GrantSubtypeProtection {
        types: StaticOperand,
        creatures_only: bool,
    },
    BlockingRestriction {
        restriction: BlockingRestrictionWire,
    },
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
    SetPTToOwnManaValue {},
    SwitchPT {},
    Abilities {
        #[serde(deserialize_with = "Option::deserialize")]
        removal: Option<AbilityRemovalWire>,
        granted_keywords: Vec<String>,
        #[serde(default)]
        suppressed_keywords: Vec<String>,
        #[serde(default)]
        granted_toxic: Option<RulesInteger>,
        #[serde(default)]
        granted_combat_keywords: Vec<CombatKeywordWire>,
        #[serde(default)]
        granted_exalted: u16,
        #[serde(default)]
        grants_unblockable: bool,
        #[serde(default)]
        granted_trigger: Option<Box<Trigger>>,
        #[serde(default)]
        granted_triggers: Vec<Trigger>,
    },
}
impl PartWire<InstructionOperand, RulesInteger> {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ResolvedPartSpec, String> {
        Ok(match self {
            Self::InheritCounteredCreatureAbilities
            | Self::InheritExiledCreatureAbilities
            | Self::GrantActivated { .. }
            | Self::GrantTrigger { .. } => {
                return Err("static trigger grant in a resolution effect".into());
            }
            Self::GrantAnyColorMana {}
            | Self::GrantFixedColorMana { .. }
            | Self::GrantSubtypeProtection { .. } => {
                return Err("static grant in a resolution effect".into());
            }
            Self::BlockingRestriction { restriction } => {
                ResolvedPartSpec::BlockingRestriction(restriction.native(vocabulary)?)
            }
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
            Self::SetPTToOwnManaValue {} => {
                return Err("recipient mana value requires a static effect".into());
            }
            Self::SwitchPT {} => ResolvedPartSpec::SwitchPT,
            Self::Abilities {
                removal,
                granted_keywords,
                suppressed_keywords,
                granted_trigger,
                granted_triggers,
                grants_unblockable,
                granted_exalted,
                granted_toxic,
                granted_combat_keywords,
            } => {
                let mut triggers = granted_trigger
                    .into_iter()
                    .map(|value| value.native(vocabulary))
                    .collect::<Result<Vec<_>, _>>()?;
                for trigger in granted_triggers {
                    triggers.push(trigger.native(vocabulary)?);
                }
                ResolvedPartSpec::Abilities(ability_change(
                    granted_toxic,
                    granted_combat_keywords,
                    (grants_unblockable, triggers),
                    granted_exalted,
                    removal,
                    granted_keywords,
                    suppressed_keywords,
                )?)
            }
        })
    }
}
impl PartWire<StaticOperand, Expression> {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<StaticPart, String> {
        Ok(match self {
            Self::InheritExiledCreatureAbilities => StaticPart::InheritExiledCreatureAbilities,
            Self::InheritCounteredCreatureAbilities => {
                StaticPart::InheritCounteredCreatureAbilities
            }
            Self::GrantActivated { ability } => {
                StaticPart::GrantActivated(Box::new(ability.native(vocabulary)?))
            }
            Self::GrantTrigger { ability } => {
                StaticPart::GrantTrigger(Box::new(ability.native(vocabulary)?))
            }
            Self::GrantAnyColorMana {} => StaticPart::GrantAnyColorMana,
            Self::GrantSubtypeProtection {
                types,
                creatures_only,
            } => StaticPart::GrantSubtypeProtection {
                types: types.native(vocabulary)?,
                creatures_only,
            },
            Self::GrantFixedColorMana { color } => StaticPart::GrantFixedColorMana(mana(&color)?),
            Self::BlockingRestriction { restriction } => {
                StaticPart::BlockingRestriction(restriction.native(vocabulary)?)
            }
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
            Self::SetPTToOwnManaValue {} => StaticPart::SetPTToOwnManaValue,
            Self::SwitchPT {} => StaticPart::SwitchPT,
            Self::Abilities {
                removal,
                granted_keywords,
                suppressed_keywords,
                granted_trigger,
                granted_triggers,
                grants_unblockable,
                granted_exalted,
                granted_toxic,
                granted_combat_keywords,
            } => {
                let mut triggers = granted_trigger
                    .into_iter()
                    .map(|value| value.native(vocabulary))
                    .collect::<Result<Vec<_>, _>>()?;
                for trigger in granted_triggers {
                    triggers.push(trigger.native(vocabulary)?);
                }
                StaticPart::Abilities(ability_change(
                    granted_toxic,
                    granted_combat_keywords,
                    (grants_unblockable, triggers),
                    granted_exalted,
                    removal,
                    granted_keywords,
                    suppressed_keywords,
                )?)
            }
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum SearchMatcherWire {
    Any {},
    Nonland {},
    BasicLand {},
    LandSubtypes {
        kinds: Vec<String>,
        require_land_type: bool,
    },
    CardTypes {
        kinds: Vec<String>,
    },
    CreatureSubtype {
        kind: Value,
    },
    Equipment {},
    CurseForHost {
        host: String,
    },
    AnyOfNames {
        names: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum SearchManaValueWire {
    Exact { value: RulesInteger },
    AtLeast { minimum: u32 },
    AtMost { maximum: u32 },
    AtMostAnnouncedX {},
    EqualToSourceCounters { kind: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchWire {
    #[serde(default)]
    searcher: Option<String>,
    #[serde(default)]
    source: Option<String>,
    matcher: SearchMatcherWire,
    #[serde(default)]
    mana_value: Option<SearchManaValueWire>,
    #[serde(default)]
    count: Option<Value>,
    #[serde(default)]
    pair_constraint: Option<Value>,
    destination: Value,
    reveal: bool,
}

impl SearchWire {
    fn native(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<spellwake_engine::program::LibrarySearch, String> {
        use spellwake_engine::program::{
            AuraSearchHost, LibrarySearch, ResolvingPlayer, SearchBattlefieldPostEntry,
            SearchCardMatcher, SearchCount, SearchDestination, SearchExileLink, SearchManaValue,
            SearchPairConstraint, SearchSource,
        };
        let searcher = match self.searcher.as_deref() {
            None | Some("Controller") => ResolvingPlayer::Controller,
            Some("ExiledTargetController") => ResolvingPlayer::ExiledTargetController,
            Some("DestructionTargetController") => ResolvingPlayer::DestructionTargetController,
            Some(other) => return Err(format!("unknown search actor {other}")),
        };
        let source = match self.source.as_deref() {
            None | Some("OwnLibrary") => SearchSource::OwnLibrary,
            Some("OwnLibraryOrGraveyard") => SearchSource::OwnLibraryOrGraveyard,
            Some("TargetOpponentLibrary") => SearchSource::TargetOpponentLibrary,
            Some(other) => return Err(format!("unknown search source {other}")),
        };
        let parse_host = |host: &str| match host {
            "TargetPlayer" => Ok(AuraSearchHost::TargetPlayer),
            "EnchantedPlayerWithDifferentCurseNames" => {
                Ok(AuraSearchHost::EnchantedPlayerWithDifferentCurseNames)
            }
            other => Err(format!("unknown curse host {other}")),
        };
        let matcher = match self.matcher {
            SearchMatcherWire::Any {} => SearchCardMatcher::Any,
            SearchMatcherWire::Nonland {} => SearchCardMatcher::Nonland,
            SearchMatcherWire::BasicLand {} => SearchCardMatcher::BasicLand,
            SearchMatcherWire::LandSubtypes {
                kinds,
                require_land_type,
            } => SearchCardMatcher::LandSubtypes {
                subtypes: kinds
                    .iter()
                    .map(|kind| LandType::from_wire_label(kind).ok_or("invalid land subtype"))
                    .collect::<Result<_, _>>()?,
                require_land_type,
            },
            SearchMatcherWire::CardTypes { kinds } => SearchCardMatcher::CardTypes(
                kinds
                    .iter()
                    .map(|kind| card_type(kind))
                    .collect::<Result<_, _>>()?,
            ),
            SearchMatcherWire::CreatureSubtype { kind } => {
                SearchCardMatcher::CreatureSubtype(creature_types::id(kind, vocabulary)?)
            }
            SearchMatcherWire::Equipment {} => SearchCardMatcher::Equipment,
            SearchMatcherWire::CurseForHost { host } => {
                SearchCardMatcher::CurseForHost(parse_host(&host)?)
            }
            SearchMatcherWire::AnyOfNames { names } => SearchCardMatcher::AnyOfNames(
                names
                    .into_iter()
                    .map(spellwake_contracts::names::EnglishCardName::new)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "invalid card name")?,
            ),
        };
        let mana_value = match self.mana_value {
            None => None,
            Some(SearchManaValueWire::Exact { value }) => Some(SearchManaValue::Exact(value)),
            Some(SearchManaValueWire::AtLeast { minimum }) => {
                Some(SearchManaValue::AtLeast(minimum))
            }
            Some(SearchManaValueWire::AtMost { maximum }) => Some(SearchManaValue::AtMost(maximum)),
            Some(SearchManaValueWire::AtMostAnnouncedX {}) => {
                Some(SearchManaValue::AtMostAnnouncedX)
            }
            Some(SearchManaValueWire::EqualToSourceCounters { kind }) => {
                let counter = spellwake_engine::effects::CounterKind::PERMANENT
                    .into_iter()
                    .find(|entry| format!("{entry:?}") == kind)
                    .ok_or("unknown permanent counter")?;
                Some(SearchManaValue::EqualToSourceCounters(counter))
            }
        };
        let count = match self.count {
            None => SearchCount::UpTo(1),
            Some(v) => match v.get("tag").and_then(Value::as_str) {
                Some("UpTo") => SearchCount::UpTo(
                    u8::try_from(
                        v.get("count")
                            .and_then(Value::as_u64)
                            .ok_or("missing count")?,
                    )
                    .map_err(|_| "invalid search count")?,
                ),
                Some("Exact") => SearchCount::Exact(
                    std::num::NonZeroU8::new(
                        u8::try_from(
                            v.get("count")
                                .and_then(Value::as_u64)
                                .ok_or("missing count")?,
                        )
                        .map_err(|_| "invalid search count")?,
                    )
                    .ok_or("exact search count must be nonzero")?,
                ),
                Some("UpToPlayersWithMoreLands") => SearchCount::UpToPlayersWithMoreLands,
                _ => return Err("invalid search count".into()),
            },
        };
        let pair_constraint = match self.pair_constraint {
            None => None,
            Some(v) => match v.get("tag").and_then(Value::as_str) {
                Some("ShareLandType") => Some(SearchPairConstraint::ShareLandType),
                Some("DistinctLandTypes") => {
                    let first = LandType::from_wire_label(
                        v.get("first")
                            .and_then(Value::as_str)
                            .ok_or("missing first")?,
                    )
                    .ok_or("invalid land subtype")?;
                    let second = LandType::from_wire_label(
                        v.get("second")
                            .and_then(Value::as_str)
                            .ok_or("missing second")?,
                    )
                    .ok_or("invalid land subtype")?;
                    Some(SearchPairConstraint::DistinctLandTypes(first, second))
                }
                _ => return Err("invalid pair constraint".into()),
            },
        };
        let destination = match self.destination.get("tag").and_then(Value::as_str) {
            Some("Hand") => SearchDestination::Hand,
            Some("LibraryTop") => SearchDestination::LibraryTop,
            Some("SplitBattlefieldTappedAndHand") => {
                SearchDestination::SplitBattlefieldTappedAndHand
            }
            Some("HandOrBattlefieldIfCreatureDied") => {
                SearchDestination::HandOrBattlefieldIfCreatureDied
            }
            Some("SplitHandAndGraveyard") => SearchDestination::SplitHandAndGraveyard,
            Some("Battlefield") => {
                let tapped = self
                    .destination
                    .get("tapped")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let post_entry = match self.destination.get("post_entry").and_then(Value::as_str) {
                    None | Some("None") => SearchBattlefieldPostEntry::None,
                    Some("HasteAndReturnAtEndStep") => {
                        SearchBattlefieldPostEntry::HasteAndReturnAtEndStep
                    }
                    Some("Transfigure") => SearchBattlefieldPostEntry::Transfigure,
                    Some("AttachToCurseHost") => {
                        SearchBattlefieldPostEntry::AttachToCurseHost(parse_host(
                            self.destination
                                .get("host")
                                .and_then(Value::as_str)
                                .ok_or("missing host")?,
                        )?)
                    }
                    _ => return Err("invalid post_entry".into()),
                };
                SearchDestination::Battlefield { tapped, post_entry }
            }
            Some("Exile") => {
                let link = match self.destination.get("link").and_then(Value::as_str) {
                    None | Some("None") => SearchExileLink::None,
                    Some("LinkedToTriggerSource") => SearchExileLink::LinkedToTriggerSource,
                    Some("FreeCastCohort") => SearchExileLink::FreeCastCohort,
                    _ => return Err("invalid exile link".into()),
                };
                SearchDestination::Exile { link }
            }
            _ => return Err("invalid search destination".into()),
        };
        Ok(LibrarySearch {
            searcher,
            source,
            matcher,
            mana_value,
            count,
            pair_constraint,
            destination,
            reveal: self.reveal,
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
// Wire-schema fixture enum preserving exact JSON variant names.
#[allow(clippy::enum_variant_names)]
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum Duration {
    NoStatedDuration,
    UntilEndOfTurn,
    WhileControllingAbilitySource,
}
impl TryFrom<String> for Duration {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "NoStatedDuration" => Ok(Self::NoStatedDuration),
            "UntilEndOfTurn" => Ok(Self::UntilEndOfTurn),
            "WhileControllingAbilitySource" => Ok(Self::WhileControllingAbilitySource),
            _ => Err(format!("unknown continuous effect duration {value}")),
        }
    }
}
impl From<Duration> for ResolvedDuration {
    fn from(value: Duration) -> Self {
        match value {
            Duration::NoStatedDuration => Self::NoStatedDuration,
            Duration::UntilEndOfTurn => Self::UntilEndOfTurn,
            Duration::WhileControllingAbilitySource => Self::WhileControllingAbilitySource,
        }
    }
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum Controller {
    SourceItself,
    SelfAtMaxSpeed,
    SourcePair,
    Any,
    Source,
    OwnerMatchesSource,
    OwnerIsOpponent,
}
impl TryFrom<String> for Controller {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "Any" => Ok(Self::Any),
            "SourceItself" => Ok(Self::SourceItself),
            "SelfAtMaxSpeed" => Ok(Self::SelfAtMaxSpeed),
            "SourcePair" => Ok(Self::SourcePair),
            "SourceController" => Ok(Self::Source),
            "OwnerIsSourceController" => Ok(Self::OwnerMatchesSource),
            "OwnerIsOpponent" => Ok(Self::OwnerIsOpponent),
            _ => Err(format!("unknown selector controller {value}")),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selector {
    #[serde(default)]
    token: Option<bool>,
    required_supertypes: Vec<String>,
    forbidden_supertypes: Vec<String>,
    required_types: Vec<String>,
    forbidden_types: Vec<String>,
    #[serde(deserialize_with = "required_nullable")]
    creature_subtype: Option<Value>,
    forbidden_creature_subtype: Option<Value>,
    #[serde(deserialize_with = "Option::deserialize")]
    land_subtype: Option<String>,
    color: ColorFilterWire,
    name: NameFilterWire,
    controller: Controller,
    #[serde(default)]
    attached_to_source: bool,
    #[serde(default)]
    combat_role: Option<String>,
    #[serde(default)]
    plus_one_counter: bool,
    #[serde(default)]
    minimum_plus_one_counters: u16,
    #[serde(default)]
    any_counter: bool,
    #[serde(default)]
    exclude_auras: bool,
    #[serde(default)]
    source_has_imprinted_creature: bool,
    controlled_creatures_and_typed_tokens: Option<Value>,
    #[serde(default)]
    requires_flying: bool,
    #[serde(default)]
    requires_untapped: bool,
    #[serde(default)]
    sole_controlled_creature: bool,
    #[serde(default)]
    equipped_by_at_least: u16,
    #[serde(default)]
    artifact_subtype: Option<String>,
    #[serde(default)]
    matches_source_entry_type: bool,
    #[serde(default)]
    controller_type_threshold: Option<(String, std::num::NonZeroU16)>,
    #[serde(default)]
    devotion_below: Option<(String, u8)>,
    #[serde(default)]
    while_controlling_own_commander: bool,
    #[serde(default)]
    station_at: Option<std::num::NonZeroU16>,
    exclude_source: bool,
}
impl Selector {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<PermanentSelector, String> {
        Ok(PermanentSelector {
            token: self.token,
            min_power: None,
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
            forbidden_creature_subtype: self
                .forbidden_creature_subtype
                .map(|value| creature_types::id(value, vocabulary))
                .transpose()?,
            controller: match self.controller {
                Controller::Any => ControllerSelector::Any,
                Controller::SourceItself => ControllerSelector::SourceItself,
                Controller::SelfAtMaxSpeed => ControllerSelector::SelfAtMaxSpeed,
                Controller::SourcePair => ControllerSelector::SourcePair,
                Controller::Source => ControllerSelector::SourceController,
                Controller::OwnerMatchesSource => ControllerSelector::OwnerIsSourceController,
                Controller::OwnerIsOpponent => ControllerSelector::OwnerIsOpponent,
            },
            combat_role: match self.combat_role.as_deref() {
                None => None,
                Some("Attacking") => {
                    Some(spellwake_engine::characteristics::CombatSelector::Attacking)
                }
                Some("OutsideCombat") => {
                    Some(spellwake_engine::characteristics::CombatSelector::OutsideCombat)
                }
                Some(_) => return Err("invalid combat selector".into()),
            },
            matches_source_entry_type: self.matches_source_entry_type,
            artifact_subtype: self
                .artifact_subtype
                .map(|kind| {
                    spellwake_engine::program::ArtifactSubtype::from_wire_label(&kind)
                        .ok_or_else(|| format!("unknown artifact subtype {kind}"))
                })
                .transpose()?,
            requires_untapped: self.requires_untapped,
            sole_controlled_creature: self.sole_controlled_creature,
            source_condition: None,
            controller_type_threshold: self
                .controller_type_threshold
                .map(|(kind, count)| card_type(&kind).map(|kind| (kind, count)))
                .transpose()?,
            plus_one_counter: self.plus_one_counter,
            minimum_plus_one_counters: self.minimum_plus_one_counters,
            any_counter: self.any_counter,
            requires_flying: self.requires_flying,
            equipped_by_at_least: self.equipped_by_at_least,
            devotion_below: self
                .devotion_below
                .map(|(s, n)| color(&s).map(|c| (c, n)))
                .transpose()?,
            while_controlling_own_commander: self.while_controlling_own_commander,
            station_at: self.station_at,
            attached_to_source: self.attached_to_source,
            exclude_source: self.exclude_source,
            exclude_auras: self.exclude_auras,
            source_has_imprinted_creature: self.source_has_imprinted_creature,
            controlled_creatures_and_typed_tokens: self
                .controlled_creatures_and_typed_tokens
                .map(|value| creature_types::id(value, vocabulary))
                .transpose()?,
            ..PermanentSelector::default()
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Expression {
    OwnGraveyardGreatestCreaturePower {},
    OwnGraveyardCreatureCount {},
    OwnGraveyardInstantOrSorceryCount {},
    OwnHandSize {},
    DuringControllersTurn {
        bonus: RulesInteger,
    },
    HalfHighestOpponentLife {},
    ManaInPool {
        kind: String,
    },
    SourceChargeCounters {},
    SourceBlessingCounters {},
    ImprintedCreaturePower {},
    ImprintedCreatureToughness {},
    TotalImprintedCreaturePower {},
    TotalImprintedCreatureToughness {},
    ControlledArtifactsOrEnchantments {},
    Devotion {
        color: String,
    },
    ControlledLandsThreshold {
        minimum: u16,
        bonus: RulesInteger,
    },
    AttachedAurasAndEquipment {
        multiplier: RulesInteger,
    },
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
        use spellwake_engine::characteristics::{
            StaticBonusCondition, StaticPlayerStat, StaticZone,
        };
        use spellwake_engine::program::{CardTypeMask, ObjectStatKind};
        Ok(match self {
            Self::OwnGraveyardCreatureCount {} => NumericExpression::ZoneCount {
                zone: StaticZone::OwnGraveyard,
                types: CardTypeMask::CREATURE,
            },
            Self::OwnGraveyardInstantOrSorceryCount {} => NumericExpression::ZoneCount {
                zone: StaticZone::OwnGraveyard,
                types: CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
            },
            Self::OwnHandSize {} => NumericExpression::ZoneCount {
                zone: StaticZone::OwnHand,
                types: CardTypeMask::EMPTY,
            },
            Self::OwnGraveyardGreatestCreaturePower {} => {
                NumericExpression::PlayerStat(StaticPlayerStat::OwnGraveyardGreatestCreaturePower)
            }
            Self::HalfHighestOpponentLife {} => {
                NumericExpression::PlayerStat(StaticPlayerStat::HalfHighestOpponentLife)
            }
            Self::DuringControllersTurn { bonus } => NumericExpression::ConditionalBonus {
                condition: StaticBonusCondition::DuringControllersTurn,
                bonus,
            },
            Self::ManaInPool { kind } => {
                NumericExpression::PlayerStat(StaticPlayerStat::ManaInPool(mana(&kind)?))
            }
            Self::SourceChargeCounters {} => NumericExpression::Counters {
                source: None,
                kind: spellwake_engine::effects::CounterKind::Charge,
            },
            Self::SourceBlessingCounters {} => NumericExpression::Counters {
                source: None,
                kind: spellwake_engine::effects::CounterKind::Blessing,
            },
            Self::ImprintedCreaturePower {} => NumericExpression::ImprintedStat {
                stat: ObjectStatKind::Power,
                total: false,
            },
            Self::ImprintedCreatureToughness {} => NumericExpression::ImprintedStat {
                stat: ObjectStatKind::Toughness,
                total: false,
            },
            Self::TotalImprintedCreaturePower {} => NumericExpression::ImprintedStat {
                stat: ObjectStatKind::Power,
                total: true,
            },
            Self::TotalImprintedCreatureToughness {} => NumericExpression::ImprintedStat {
                stat: ObjectStatKind::Toughness,
                total: true,
            },
            Self::ControlledArtifactsOrEnchantments {} => NumericExpression::ZoneCount {
                zone: StaticZone::ControlledBattlefield,
                types: CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT),
            },
            Self::Devotion { color: label } => {
                NumericExpression::PlayerStat(StaticPlayerStat::Devotion(color(&label)?))
            }
            Self::ControlledLandsThreshold { minimum, bonus } => {
                NumericExpression::ConditionalBonus {
                    condition: StaticBonusCondition::ControlledLandsAtLeast(minimum),
                    bonus,
                }
            }
            Self::AttachedAurasAndEquipment { multiplier } => {
                NumericExpression::AttachedAurasAndEquipment { multiplier }
            }
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
enum FlashWire {
    Any {},
    Colorless {},
    Historic {},
    CardType { kind: String },
    CreatureSubtype { kind: Value },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum DiscountWire {
    Any {},
    Colorless {},
    CreaturePowerAtLeastFour {},
    CreatureOfSourceEntryType {},
    EitherSubtype { kinds: [Value; 2] },
    AuraOrEquipment {},
    Color { color: String },
    CardType { kind: String },
    InstantOrSorcery {},
}
impl DiscountWire {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<SpellFilter, String> {
        Ok(match self {
            Self::Any {} => SpellFilter::any(),
            Self::Colorless {} => SpellFilter::colorless(),
            Self::CreaturePowerAtLeastFour {} => {
                SpellFilter::creature().with_power(PowerFilter::AtLeast(4))
            }
            Self::Color { color: label } => SpellFilter::of_color(color(&label)?),
            Self::CardType { kind } => SpellFilter::of_type(card_type(&kind)?),
            Self::CreatureOfSourceEntryType {} => SpellFilter::creature().of_chosen_entry_type(),
            Self::EitherSubtype {
                kinds: [first, second],
            } => SpellFilter::either_subtype(
                creature_types::id(first, vocabulary)?,
                creature_types::id(second, vocabulary)?,
            ),
            Self::AuraOrEquipment {} => SpellFilter::aura_or_equipment(),
            Self::InstantOrSorcery {} => SpellFilter::instant_or_sorcery(),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum DiscountAmountWire {
    Fixed { generic: std::num::NonZeroU8 },
    PerExperienceCounter { value: std::num::NonZeroU8 },
    ControlledCreaturesWithPlusOneCounters {},
    SourcePlusOneCounters {},
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum StaticAbility {
    PayLifeForEnchantmentOwnTurnOnce {
        id: u64,
    },
    FreeColorlessHandSpellOnce {
        id: u64,
    },
    SeeLibraryTop {
        id: u64,
        public: bool,
    },
    PlayLibraryTop {
        id: u64,
        matching: String,
    },
    SubtypeSpellsHaveReplicate {
        id: u64,
        subtype: Value,
    },
    OtherSubtypeSpellsCostLessFromCommandOrBattlefield {
        id: u64,
        subtype: Value,
        amount: std::num::NonZeroU8,
    },
    ColorlessHandSpellsHaveCascade {
        id: u64,
        minimum_mana_value: u16,
        count: std::num::NonZeroU8,
    },
    GraveyardSubtypeEncore {
        id: u64,
        subtype: Value,
    },
    IgnoreLegendForSubtype {
        id: u64,
        subtype: Value,
    },
    ChosenTypeSpellsCostOneLessOfEachColor {
        id: u64,
    },
    GraveyardCreatureCast {
        id: u64,
    },
    AdditionalBlockWhileMonarch {
        id: u64,
    },
    EnchantedCreatureSkipsUntap {
        id: u64,
    },
    EnchantedPlayerCastLimit {
        id: u64,
    },
    RestrictGraveyardAndLibrary {
        id: u64,
    },
    DoubleDamageToEnchantedPlayer {
        id: u64,
    },
    SourceSkipsUntap {
        id: u64,
    },
    ControllerHasHexproof {
        id: u64,
    },
    DoubleEquippedCombatDamage {
        id: u64,
    },
    FreeSubtypeCreatureSpells {
        id: u64,
        subtype: Value,
    },
    PreventEnchantedCreatureDamage {
        id: u64,
    },
    EnterControlledCreaturesAsEnchantedCopy {
        id: u64,
    },
    PreventIncomingCombatDamageAndExile {
        id: u64,
    },
    EnchantedCreatureUntapsOnlyForMonarch {
        id: u64,
    },
    CastAsThoughFlash {
        id: u64,
        any_player: bool,
        matching: FlashWire,
    },
    RetainMana {
        id: u64,
        kind: String,
    },
    OpponentsCannotGainLife {
        id: u64,
    },
    DoubleCreatedTokens {
        id: u64,
    },
    DoublePlacedCounters {
        id: u64,
    },
    Ascend {
        id: u64,
    },
    CombatRequiresCityBlessing {
        id: u64,
    },
    UntapDuringOtherPlayersUntap {
        id: u64,
    },
    AdditionalSelfPlusOneCounter {
        id: u64,
    },
    NonartifactSpellsHaveImprovise {
        id: u64,
    },
    RestrictOpponentsDuringOwnTurn {
        id: u64,
    },
    AdditionalCreatureDeathTrigger {
        id: u64,
    },
    OtherCreaturesEnterWithSourceCounters {
        id: u64,
    },
    ExtraEntryLoyalty {
        id: u64,
    },
    TwiceLoyaltyPerTurn {
        id: u64,
    },
    LethalUsesPower {
        id: u64,
    },
    PreventSourceCombatDamage {
        id: u64,
    },
    PreventCombatDamageToAttackingArtifacts {
        id: u64,
    },
    DoubleDamageToOpponents {
        id: u64,
    },
    PreventHalfIncomingDamage {
        id: u64,
    },
    PreventNoncombatDamage {
        id: u64,
    },
    SplitOpponentsMultipleDraw {
        id: u64,
    },
    ReplaceDrawWithLandKind {
        id: u64,
    },
    SkipControllerDrawStep {
        id: u64,
    },
    OpponentsDrawAtMostOne {
        id: u64,
    },
    ControllerSpellsCannotBeCountered {
        id: u64,
        types: Vec<String>,
    },
    HighManaValueSpellsProtected {
        id: u64,
        minimum: std::num::NonZeroU8,
        reduction: std::num::NonZeroU8,
    },
    PreventLoyaltyActivations {
        id: u64,
    },
    PreventArtifactActivations {
        id: u64,
    },
    PreventControlledTransform {
        id: u64,
        subtype: Value,
        except: Value,
    },
    IncreaseSpellCostExceptType {
        id: u64,
        exempt: String,
        amount: std::num::NonZeroU8,
    },
    ProhibitLifeAndCreatureCosts {
        id: u64,
    },
    ProhibitOpponentsCausingSacrifice {
        id: u64,
    },
    NoMaximumHandSize {
        id: u64,
    },
    AdditionalLandPlays {
        id: u64,
        count: std::num::NonZeroU8,
    },
    ControllerSpellsCostOneLess {
        id: u64,
        matching: DiscountWire,
    },
    ControllerSpellsCostLess {
        id: u64,
        matching: DiscountWire,
        amount: DiscountAmountWire,
    },
    AllSpellsCostLess {
        id: u64,
        matching: DiscountWire,
        amount: std::num::NonZeroU8,
    },
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
            Self::CastAsThoughFlash {
                id,
                any_player,
                matching,
            } => {
                let matching = match matching {
                    FlashWire::Any {} => SpellFilter::any(),
                    FlashWire::Colorless {} => SpellFilter::colorless(),
                    FlashWire::Historic {} => SpellFilter::historic(),
                    FlashWire::CardType { kind } => SpellFilter::of_type(card_type(&kind)?),
                    FlashWire::CreatureSubtype { kind } => {
                        SpellFilter::of_subtype(creature_types::id(kind, vocabulary)?)
                    }
                };
                StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(id),
                    permission: CastPermissionRule::Flash {
                        any_player,
                        matching,
                    },
                }
            }
            Self::PayLifeForEnchantmentOwnTurnOnce { id } => {
                StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(id),
                    modifier: SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce,
                }
            }
            Self::FreeColorlessHandSpellOnce { id } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::FreeColorlessHandSpellOnce,
            },
            Self::SeeLibraryTop { id, public } => StaticContinuousAbility::CastPermission {
                id: StaticAbilityId(id),
                permission: CastPermissionRule::SeeLibraryTop { public },
            },
            Self::PlayLibraryTop { id, matching } => {
                let matching = match matching.as_str() {
                    "Land" => LibraryTopPermission::Land,
                    "InstantOrSorcery" => LibraryTopPermission::InstantOrSorcery,
                    "ArtifactOrColorlessSpell" => LibraryTopPermission::ArtifactOrColorlessSpell,
                    "CreatureOfSourceEntryType" => LibraryTopPermission::CreatureOfSourceEntryType,
                    _ => return Err("invalid library top permission".into()),
                };
                StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(id),
                    permission: CastPermissionRule::PlayLibraryTop(matching),
                }
            }
            Self::OtherSubtypeSpellsCostLessFromCommandOrBattlefield {
                id,
                subtype,
                amount,
            } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield {
                    subtype: creature_types::id(subtype, vocabulary)?,
                    amount,
                },
            },
            Self::ColorlessHandSpellsHaveCascade {
                id,
                minimum_mana_value,
                count,
            } => StaticContinuousAbility::CastPermission {
                id: StaticAbilityId(id),
                permission: CastPermissionRule::ColorlessHandSpellsHaveCascade {
                    minimum_mana_value,
                    count,
                },
            },
            Self::SubtypeSpellsHaveReplicate { id, subtype } => {
                StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(id),
                    permission: CastPermissionRule::SubtypeSpellsHaveReplicate(creature_types::id(
                        subtype, vocabulary,
                    )?),
                }
            }
            Self::GraveyardSubtypeEncore { id, subtype } => {
                StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(id),
                    permission: CastPermissionRule::GraveyardSubtypeEncore(creature_types::id(
                        subtype, vocabulary,
                    )?),
                }
            }
            Self::IgnoreLegendForSubtype { id, subtype } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::IgnoreLegendForSubtype(creature_types::id(
                    subtype, vocabulary,
                )?),
            },
            Self::ChosenTypeSpellsCostOneLessOfEachColor { id } => {
                StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(id),
                    modifier: SpellCostModifier::ChosenTypeOneLessOfEachColor,
                }
            }
            Self::GraveyardCreatureCast { id } => StaticContinuousAbility::CastPermission {
                id: StaticAbilityId(id),
                permission: CastPermissionRule::GraveyardCreatureCast,
            },
            Self::AdditionalBlockWhileMonarch { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::AdditionalBlockWhileMonarch,
            },
            Self::EnchantedCreatureSkipsUntap { id } => StaticContinuousAbility::UntapRule {
                id: StaticAbilityId(id),
                rule: StaticUntapRule::EnchantedCreatureSkipsUntap,
            },
            Self::EnchantedPlayerCastLimit { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::EnchantedPlayerCastLimit,
            },
            Self::RestrictGraveyardAndLibrary { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::RestrictGraveyardAndLibrary,
            },
            Self::DoubleDamageToEnchantedPlayer { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::DoubleToEnchantedPlayer,
            },
            Self::FreeSubtypeCreatureSpells { id, subtype } => {
                StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(id),
                    modifier: SpellCostModifier::FreeSubtypeCreatureSpells(creature_types::id(
                        subtype, vocabulary,
                    )?),
                }
            }
            Self::DoubleEquippedCombatDamage { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::DoubleEquippedCombat,
            },
            Self::ControllerHasHexproof { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::ControllerHasHexproof,
            },
            Self::SourceSkipsUntap { id } => StaticContinuousAbility::UntapRule {
                id: StaticAbilityId(id),
                rule: StaticUntapRule::SourceSkipsUntap,
            },
            Self::PreventIncomingCombatDamageAndExile { id } => {
                StaticContinuousAbility::DamageRule {
                    id: StaticAbilityId(id),
                    rule: StaticDamageRule::PreventIncomingCombatAndExile,
                }
            }
            Self::EnterControlledCreaturesAsEnchantedCopy { id } => {
                StaticContinuousAbility::Modifier {
                    id: StaticAbilityId(id),
                    rule: StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy,
                }
            }
            Self::PreventEnchantedCreatureDamage { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::PreventEnchantedCreature { combat_only: false },
            },
            Self::EnchantedCreatureUntapsOnlyForMonarch { id } => {
                StaticContinuousAbility::UntapRule {
                    id: StaticAbilityId(id),
                    rule: StaticUntapRule::EnchantedCreatureUntapsOnlyForMonarch,
                }
            }
            Self::ProhibitOpponentsCausingSacrifice { id } => {
                StaticContinuousAbility::Restriction {
                    id: StaticAbilityId(id),
                    rule: StaticRestriction::ProhibitOpponentsCausingSacrifice,
                }
            }
            Self::ProhibitLifeAndCreatureCosts { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::ProhibitLifeAndCreatureCosts,
            },
            Self::PreventControlledTransform {
                id,
                subtype,
                except,
            } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::PreventControlledTransform {
                    subtype: creature_types::id(subtype, vocabulary)?,
                    except: creature_types::id(except, vocabulary)?,
                },
            },
            Self::PreventArtifactActivations { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::PreventArtifactActivations,
            },
            Self::IncreaseSpellCostExceptType { id, exempt, amount } => {
                StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(id),
                    modifier: SpellCostModifier::IncreaseExceptType {
                        exempt: card_type(&exempt)?,
                        amount,
                    },
                }
            }
            Self::PreventLoyaltyActivations { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::PreventLoyaltyActivations,
            },
            Self::HighManaValueSpellsProtected {
                id,
                minimum,
                reduction,
            } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::HighManaValueProtected { minimum, reduction },
            },
            Self::ControllerSpellsCannotBeCountered { id, types } => {
                StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(id),
                    permission: CastPermissionRule::ControllerSpellsCannotBeCountered(
                        types
                            .into_iter()
                            .map(|value| card_type(&value))
                            .collect::<Result<_, _>>()?,
                    ),
                }
            }
            Self::SplitOpponentsMultipleDraw { id } => StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(id),
                rule: StaticDrawRule::SplitOpponentsMultipleDraw,
            },
            Self::SkipControllerDrawStep { id } => StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(id),
                rule: StaticDrawRule::SkipControllerDrawStep,
            },
            Self::ReplaceDrawWithLandKind { id } => StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(id),
                rule: StaticDrawRule::ReplaceWithLandKind,
            },
            Self::OpponentsDrawAtMostOne { id } => StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(id),
                rule: StaticDrawRule::OpponentsAtMostOne,
            },
            Self::RetainMana { id, kind } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::RetainMana(mana(&kind)?),
            },
            Self::OpponentsCannotGainLife { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::OpponentsCannotGainLife,
            },
            Self::DoubleCreatedTokens { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::DoubleCreatedTokens,
            },
            Self::DoublePlacedCounters { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::DoublePlacedCounters,
            },
            Self::Ascend { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::Ascend,
            },
            Self::CombatRequiresCityBlessing { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::CombatRequiresCityBlessing,
            },
            Self::UntapDuringOtherPlayersUntap { id } => StaticContinuousAbility::UntapRule {
                id: StaticAbilityId(id),
                rule: StaticUntapRule::UntapDuringOtherPlayersUntap,
            },
            Self::AdditionalSelfPlusOneCounter { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::AdditionalSelfPlusOneCounter,
            },
            Self::NonartifactSpellsHaveImprovise { id } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::NonartifactSpellsHaveImprovise,
            },
            Self::RestrictOpponentsDuringOwnTurn { id } => StaticContinuousAbility::Restriction {
                id: StaticAbilityId(id),
                rule: StaticRestriction::RestrictOpponentsDuringOwnTurn,
            },
            Self::AdditionalCreatureDeathTrigger { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::AdditionalCreatureDeathTrigger,
            },
            Self::OtherCreaturesEnterWithSourceCounters { id } => {
                StaticContinuousAbility::Modifier {
                    id: StaticAbilityId(id),
                    rule: StaticModifierRule::OtherCreaturesEnterWithSourceCounters,
                }
            }
            Self::ExtraEntryLoyalty { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::ExtraEntryLoyalty,
            },
            Self::TwiceLoyaltyPerTurn { id } => StaticContinuousAbility::Modifier {
                id: StaticAbilityId(id),
                rule: StaticModifierRule::TwiceLoyaltyPerTurn,
            },
            Self::LethalUsesPower { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::LethalUsesPower,
            },
            Self::DoubleDamageToOpponents { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::DoubleToOpponents,
            },
            Self::PreventHalfIncomingDamage { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::PreventHalfIncoming,
            },
            Self::PreventCombatDamageToAttackingArtifacts { id } => {
                StaticContinuousAbility::DamageRule {
                    id: StaticAbilityId(id),
                    rule: StaticDamageRule::PreventCombatToAttackingArtifacts,
                }
            }
            Self::PreventSourceCombatDamage { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::PreventSourceCombat,
            },
            Self::PreventNoncombatDamage { id } => StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(id),
                rule: StaticDamageRule::PreventNoncombat,
            },
            Self::NoMaximumHandSize { id } => StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(id),
                rule: StaticDrawRule::NoMaximumHandSize,
            },
            Self::AdditionalLandPlays { id, count } => StaticContinuousAbility::CastPermission {
                id: StaticAbilityId(id),
                permission: CastPermissionRule::AdditionalLandPlays(count),
            },
            Self::ControllerSpellsCostOneLess { id, matching } => {
                StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(id),
                    modifier: SpellCostModifier::Discount {
                        any_player: false,
                        matching: matching.native(vocabulary)?,
                        amount: SpellDiscount::Fixed(std::num::NonZeroU8::MIN),
                    },
                }
            }
            Self::AllSpellsCostLess {
                id,
                matching,
                amount,
            } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::Discount {
                    any_player: true,
                    matching: matching.native(vocabulary)?,
                    amount: SpellDiscount::Fixed(amount),
                },
            },
            Self::ControllerSpellsCostLess {
                id,
                matching,
                amount,
            } => StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(id),
                modifier: SpellCostModifier::Discount {
                    any_player: false,
                    matching: matching.native(vocabulary)?,
                    amount: match amount {
                        DiscountAmountWire::SourcePlusOneCounters {} => {
                            SpellDiscount::SourcePlusOneCounters
                        }
                        DiscountAmountWire::Fixed { generic } => SpellDiscount::Fixed(generic),
                        DiscountAmountWire::PerExperienceCounter { value } => {
                            SpellDiscount::PerExperienceCounter(value)
                        }
                        DiscountAmountWire::ControlledCreaturesWithPlusOneCounters {} => {
                            SpellDiscount::ControlledCreaturesWithPlusOneCounters
                        }
                    },
                },
            },
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
    CommanderChosenColor {
        chosen: Option<String>,
    },
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
            Self::CommanderChosenColor { chosen } => {
                DefiningPart::CommanderChosenColor(chosen.as_deref().map(color).transpose()?)
            }
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
        if let Some(modes) = self.triggered_modes {
            if !matches!(self.target, Target::None {}) || !self.instructions.is_empty() {
                return Err("modal trigger body has separate targets or instructions".into());
            }
            return Ok(ExecutableBody::with_triggered_modes(
                spellwake_engine::program::TriggeredModes {
                    minimum: modes.minimum,
                    maximum: modes.maximum,
                    distinct_targets: modes.distinct_targets,
                    allow_repeat: false,
                    uniqueness: spellwake_engine::program::ModalUniqueness::None,
                    prefix: None,
                    suffix: None,
                    options: modes
                        .options
                        .into_iter()
                        .map(|option| option.native(vocabulary))
                        .collect::<Result<_, _>>()?,
                },
            ));
        }
        let target = match self.target {
            Target::CreatureOfSubtype { subtype } => BodyTarget::filtered_permanent(
                spellwake_engine::program::ObjectFilter::creature()
                    .with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Target::PermanentOfCreatureSubtype { subtype } => BodyTarget::filtered_permanent(
                spellwake_engine::program::ObjectFilter::any()
                    .with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Target::FilteredPermanent { filter } => {
                BodyTarget::filtered_permanent(battlefield_filter_from_label(&filter)?)
            }
            Target::OwnGraveyardCardWithSubtype { subtype } => BodyTarget::filtered_own_graveyard(
                spellwake_engine::program::ObjectFilter::any()
                    .with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Target::FilteredOwnGraveyardCard { filter } => {
                BodyTarget::filtered_own_graveyard(graveyard_filter_from_label(&filter)?)
            }
            Target::AnyGraveyardCreatureOrPlaneswalker {} => {
                BodyTarget::ANY_GRAVEYARD_CREATURE_OR_PLANESWALKER
            }
            Target::RecentlyDiscardedOrFromLibrary {} => {
                BodyTarget::RECENTLY_DISCARDED_OR_FROM_LIBRARY
            }
            Target::AnyGraveyardCard {} => BodyTarget::ANY_GRAVEYARD_CARD,
            Target::AnyGraveyardCreature {} => BodyTarget::ANY_GRAVEYARD_CREATURE,
            Target::SingleOwnGraveyardCard {} => BodyTarget::SINGLE_OWN_GRAVEYARD_CARD,
            Target::None {} => BodyTarget::None,
            Target::SinglePermanent {} => BodyTarget::SINGLE_PERMANENT,
            Target::SingleCreature {} => BodyTarget::SINGLE_CREATURE,
            Target::SingleOpponent {} => BodyTarget::SINGLE_OPPONENT,
            Target::SinglePlayer {} => BodyTarget::SINGLE_PLAYER,
            Target::SingleCreatureOrPlayer {} => BodyTarget::SINGLE_CREATURE_OR_PLAYER,
            Target::SinglePlayerOrPlaneswalker {} => BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER,
            Target::SinglePlaneswalker {} => BodyTarget::SINGLE_PLANESWALKER,
            Target::ControlledAbility { colorless_source } => {
                BodyTarget::controlled_ability(colorless_source)
            }
            Target::StackEntryTargetingControlledPermanent {} => {
                BodyTarget::STACK_ENTRY_TARGETING_CONTROLLED_PERMANENT
            }
            Target::SingleStackEntry {} => BodyTarget::SINGLE_STACK_ENTRY,
            Target::SingleAnyTarget {} => BodyTarget::SINGLE_ANY_TARGET,
            Target::SingleSpell { support_gate } => {
                BodyTarget::single_spell(match support_gate.as_str() {
                    "NotCastFromOwnersHand" => SpellFilter::any().not_from_owners_hand(),
                    "AnySpell" => SpellFilter::any(),
                    "SpellWithSingleTarget" => SpellFilter::any().single_target(),
                    "SpellTargetingPlayer" => SpellFilter::any().targeting_player(),
                    label if label.starts_with("CastNumberThisTurn(") => SpellFilter::any()
                        .ordinal_this_turn(
                            label
                                .strip_prefix("CastNumberThisTurn(")
                                .and_then(|text| text.strip_suffix(')'))
                                .and_then(|number| number.parse().ok())
                                .ok_or("invalid cast position")?,
                        ),
                    "SorcerySpell" => SpellFilter::of_type(CardType::Sorcery),
                    "NoncreatureSpell" => SpellFilter::noncreature(),
                    "CreatureSpell" => SpellFilter::creature(),
                    "ControlledInstantOrSorcery" => SpellFilter::instant_or_sorcery().controlled(),
                    "CopyableInstructionSpell" => SpellFilter::copyable_instruction(),
                    _ => return Err(format!("unknown spell support gate {support_gate}")),
                })
            }
        };
        let mut permanent_target_bounds: Option<(u16, u16)> = None;
        let mut instructions = Vec::new();
        for op in self.instructions {
            match op {
                Op::ExileTopForPlay { until_next_from_source } => {
                    instructions.push(Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::exile_top_library(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            ValueExpr::Literal(1.into()),
                            false,
                        ),
                    )));
                    instructions.push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::impulse_play_permission(
                            spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            if until_next_from_source {
                                spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource
                            } else {
                                spellwake_engine::program::ImpulsePlayDuration::ThisTurn
                            },
                            false,
                            false,
                            false,
                        ),
                    )));
                    continue;
                }
                Op::ModifyPermanentTargetAndPartner { parts, duration } => {
                    permanent_target_bounds = Some((1, 1));
                    instructions.push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                            spellwake_engine::toolbox::ObjectQuery::TargetsWithPairedPartner,
                            ResolvedPartsSpec(
                                parts
                                    .into_iter()
                                    .map(|part| part.native(vocabulary))
                                    .collect::<Result<Vec<_>, _>>()?,
                            ),
                            duration.into(),
                        ),
                    )));
                    continue;
                }
                Op::ModifyPermanentTargets { minimum, maximum, parts, duration } => {
                    permanent_target_bounds = Some((minimum, maximum));
                    instructions.push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(
                                parts
                                    .into_iter()
                                    .map(|part| part.native(vocabulary))
                                    .collect::<Result<Vec<_>, _>>()?,
                            ),
                            duration.into(),
                        ),
                    )));
                    continue;
                }
                Op::AffectPermanentTargets { minimum, maximum, action } => {
                    permanent_target_bounds = Some((minimum, maximum));
                    match action.as_str() {
                        "Destroy" => instructions.push(Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                        allow_regenerate: true,
                                    },
                                },
                            ),
                        ))),
                        "Exile" => instructions.push(Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ))),
                        "ReturnToHand" => instructions.push(Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Hand,
                            ),
                        ))),
                        "Tap" => instructions.push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(true)),
                            ),
                        ))),
                        "ToggleTap" => instructions.push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Toggle),
                            ),
                        ))),
                        "TapAndSkipNextUntap" => {
                            instructions.push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(true)),
                                ),
                            )));
                            instructions.push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::SkipNextUntap),
                                ),
                            )));
                        }
                        "Untap" => instructions.push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(false)),
                            ),
                        ))),
                        "PutPlusOneCounter" | "PutPlusOneCounterOnOthers" => {
                            instructions.push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ObjectMutation::Counters(
                                        spellwake_engine::toolbox::CounterOp::Add {
                                            kind: spellwake_engine::effects::CounterKind::PlusOne,
                                            amount: ValueExpr::literal(1),
                                        },
                                    ),
                                ),
                            )));
                        }
                        "PutCountersByCurrentType" => {
                            instructions.push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Filtered {
                                        source: Box::new(
                                            spellwake_engine::toolbox::ObjectQuery::Targets,
                                        ),
                                        filter: Box::new(
                                            spellwake_engine::program::ObjectFilter::creature(),
                                        ),
                                    },
                                    spellwake_engine::toolbox::ObjectMutation::Counters(
                                        spellwake_engine::toolbox::CounterOp::Add {
                                            kind: spellwake_engine::effects::CounterKind::PlusOne,
                                            amount: ValueExpr::literal(1),
                                        },
                                    ),
                                ),
                            )));
                            instructions.push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Filtered {
                                        source: Box::new(
                                            spellwake_engine::toolbox::ObjectQuery::Targets,
                                        ),
                                        filter: Box::new(
                                            spellwake_engine::program::ObjectFilter::from_clause(
                                                spellwake_engine::program::ObjectClause::planeswalker(),
                                            ),
                                        ),
                                    },
                                    spellwake_engine::toolbox::ObjectMutation::Counters(
                                        spellwake_engine::toolbox::CounterOp::Add {
                                            kind: spellwake_engine::effects::CounterKind::Loyalty,
                                            amount: ValueExpr::literal(1),
                                        },
                                    ),
                                ),
                            )));
                        }
                        "Regenerate" => instructions.push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ObjectMutation::Regenerate,
                            ),
                        ))),
                        _ => return Err("unknown simultaneous target action".into()),
                    }
                    continue;
                }
                op => instructions.push(match op {
                    Op::OfferSourceCounters {
                        amount,
                        placed_destination,
                        declined_destination,
                    } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::offer_source_counters(
                            amount,
                            placed_destination,
                            declined_destination,
                        ),
                    )),
                    Op::OptionalSequence { chooser, instructions } => Instruction::OptionalSequence { chooser: chooser.into(), instructions },
                    Op::OptionalEffect { chooser } => Instruction::OptionalSequence {
                        chooser: chooser.into(),
                        instructions: 1,
                    },
                    Op::OfferManaPayment {
                        payer,
                        cost,
                        paid_destination,
                        declined_destination,
                    } => Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            payer.into(),
                            cost.native()?,
                            ValueExpr::Literal(0.into()),
                            instruction_destination(paid_destination)?,
                            instruction_destination(declined_destination)?,
                        ),
                    )),
                    Op::BranchIfFalse {
                        condition,
                        destination,
                    } => Instruction::BranchCondition {
                        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                            Box::new(condition.native(vocabulary)?),
                        )),
                        destination: instruction_destination(destination)?,
                    },
                    Op::Jump { destination } => Instruction::Jump {
                        destination: instruction_destination(destination)?,
                    },
                    Op::CreateTokenAndAttach { definition } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::create_token_and_attach(
                            Box::new(definition.native(vocabulary)?),
                        ),
                    )),
                    Op::CreateTokens { creations } => Instruction::CreateTokens {
                        creations: creations
                            .into_iter()
                            .map(|creation| creation.native(vocabulary))
                            .collect::<Result<_, _>>()?,
                    },
                    Op::DrainTargetAndGain { amount } => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::drain(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::TargetPlayer), amount.native(vocabulary)?, spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), false))),
                    Op::GainControllerLife { amount } => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), amount.native(vocabulary)?))),
                    Op::AdjustControllerLife { adjustment } => match adjustment.native() { spellwake_engine::life::LifeAdjustment::Gain(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Lose(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Set(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::set(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::program::ValueExpr::Literal(n)))), },
                    Op::AdjustTargetLife { adjustment } => match adjustment.native() { spellwake_engine::life::LifeAdjustment::Gain(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::TargetPlayer), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Lose(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::TargetPlayer), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Set(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::set(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::TargetPlayer), spellwake_engine::program::ValueExpr::Literal(n)))), },
                    Op::OpponentsLoseAndControllerGains { amount } => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::drain(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), amount.native(vocabulary)?, spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), false))),
                    Op::AdjustEachOpponentLife { adjustment, controller_adjustment: _ } => match adjustment.native() { spellwake_engine::life::LifeAdjustment::Gain(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Lose(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::program::ValueExpr::Literal(n)))), spellwake_engine::life::LifeAdjustment::Set(n) => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::set(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::program::ValueExpr::Literal(n)))), },
                    Op::RememberPermanentTarget {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                    Op::DamageEachOpponent { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Players(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent)), amount: spellwake_engine::program::DamageAmountSpec::Expr(spellwake_engine::program::ValueExpr::Literal(amount)), exile_if_dies: false })),
                    Op::DamageGroup { group, amount } => {
                        use spellwake_engine::program::DamageGroup;
                        let group = match group.as_str() {
                            "Controller" => DamageGroup::Controller,
                            "ControllerAndTargetCreature" => DamageGroup::ControllerAndTargetCreature,
                            "DestroyedTargetController" => DamageGroup::DestroyedTargetController,
                            "CapturedAttackedPlayerOrPlaneswalker" => DamageGroup::CapturedAttackedPlayerOrPlaneswalker,
                            "VolunteeringOpponent" => DamageGroup::VolunteeringOpponent,
                            "RelatedPlayer" => DamageGroup::RelatedPlayer,
                            "RelatedPlayerOrTheirPlaneswalker" => DamageGroup::RelatedPlayerOrTheirPlaneswalker,
                            "CreaturesAndPlaneswalkers" => DamageGroup::CreaturesAndPlaneswalkers,
                            "Creatures" => DamageGroup::Creatures,
                            "Players" => DamageGroup::Players,
                            "CreaturesAndPlayers" => DamageGroup::CreaturesAndPlayers,
                            _ => return Err(format!("unknown damage group {group}")),
                        };
                        Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(spellwake_engine::program::DamageRecipientsRef::Group(group))), amount: spellwake_engine::program::DamageAmountSpec::Expr(spellwake_engine::program::ValueExpr::Literal(amount)), exile_if_dies: false }))
                    }
                    Op::Scry { player, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::scry(
                            player.into(),
                            amount.native(vocabulary)?,
                        ),
                    )),
                    Op::Surveil { player, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                            player.into(),
                            amount.native(vocabulary)?,
                        ),
                    )),
                    Op::Mill { player, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::mill(
                            player.into(),
                            amount.native(vocabulary)?,
                        ),
                    )),
                    Op::ExtraCombatControlled {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(spellwake_engine::program::CombatUntapGroup::ControlledCreatures))),
                    Op::ExtraCombatHighestLife {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(spellwake_engine::program::CombatUntapGroup::AttackersIfSourceAttacksHighestLife))),
                    Op::ExtraCombatSubtype {subtype} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(spellwake_engine::program::CombatUntapGroup::SourceAndControlledSubtype(creature_types::id(subtype,vocabulary)?)))),
                    Op::TakeExtraTurn {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::take_extra_turn(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller)))),
                    Op::DisallowDamagePreventionThisTurn {} => {
                        Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec::temporary_rule(
                                spellwake_engine::program::TemporaryRuleKind::DamageCantBePrevented,
                                spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                            ),
                        ))
                    }
                    Op::GrantNoMaxHandSizeRestOfGame {} => {
                        Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: None, payload: spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(Box::new(spellwake_engine::program::TemporaryRuleKind::TemporaryStatic { ability: Box::new(spellwake_engine::characteristics::StaticContinuousAbility::DrawRule { id: spellwake_engine::characteristics::StaticAbilityId(1), rule: spellwake_engine::characteristics::StaticDrawRule::NoMaximumHandSize }), condition: None, one_shot: false })), duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration, condition: None, one_shot: false, as_emblem: None, emblem_label: None }))
                    }
                    Op::ChangeOnlyTarget {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::retarget(true))),
                    Op::GainControlOfSpell {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::gain_control())),
                    Op::ChooseNewTargets {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::retarget(false))),
                    Op::MoveGroup { group, destination } => {
                        let dest = match public_destination(&destination)? {
                            spellwake_engine::program::PublicZoneDestination::Hand | spellwake_engine::program::PublicZoneDestination::Command => spellwake_engine::toolbox::ZoneDest::Hand,
                            spellwake_engine::program::PublicZoneDestination::Graveyard => spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Move },
                            spellwake_engine::program::PublicZoneDestination::Exile => spellwake_engine::toolbox::ZoneDest::exile(),
                            spellwake_engine::program::PublicZoneDestination::Library => spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::Top },
                            spellwake_engine::program::PublicZoneDestination::Battlefield => spellwake_engine::toolbox::ZoneDest::battlefield(spellwake_engine::toolbox::ZoneController::Owner),
                        };
                        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, permanent_group(&group, vocabulary)?), dest)))
                    }
                    Op::DestroyGroupWithoutRegeneration { group } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, permanent_group(&group, vocabulary)?), spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: false } }))),
                    Op::DestroyGroup { group } => {
                        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, permanent_group(&group, vocabulary)?), spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } })))
                    }
                    Op::DrawTargetPlayer { amount } => Instruction::DrawCards(Box::new(spellwake_engine::toolbox::DrawCardsSpec { players: spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::One(spellwake_engine::program::ResolvingPlayer::TargetPlayer)), amount: amount.native(vocabulary)?, bind_drawn: None })),
                    Op::ReturnExiledTargetCohort {
                        next_end_step,
                        owner_controls,
                    } => Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::return_exiled_target_cohort(
                            next_end_step,
                            owner_controls,
                        ),
                    )),
                    Op::ExileTopForPlay { .. }
                    | Op::ModifyPermanentTargetAndPartner { .. }
                    | Op::ModifyPermanentTargets { .. }
                    | Op::AffectPermanentTargets { .. } => unreachable!("handled above"),
                    Op::ExileTopFaceDown {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::ZoneScan { zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), filter: Box::new(spellwake_engine::program::ObjectFilter::any()), slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(spellwake_engine::program::ValueExpr::literal(1))) }, spellwake_engine::toolbox::ZoneDest::Exile { face_down: true, viewer: Some(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller)), link_to_source: true, delayed_return: None, link_group: None }))),
                    Op::AddFixedMana { produced } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::EffectManaYield::Fixed(mana_map(produced)?),
                        ),
                    )),
                    Op::DrawEachPlayer { amount } => Instruction::DrawCards(Box::new(spellwake_engine::toolbox::DrawCardsSpec { players: spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachPlayer), amount: amount.native(vocabulary)?, bind_drawn: None })),
                    Op::CreateOpponentUpkeepDamageEmblems { amount } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::UpkeepDamage { amount }))),
                    Op::CreateOpponentSpellcastDamageEmblems { amount, noncreature_only } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::SpellcastDamage { amount, noncreature_only }))),
                    Op::CreateOpponentCreatureBonusEmblems { power_toughness, keywords } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::ControlledCreatureBonus { power_toughness: power_toughness.into(), keywords: keyword_set(keywords)? }))),
                    Op::UntapControlledLands {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::land().controlled()))), spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(false))))),
                    Op::GiveControlOfAbilitySource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Control(spellwake_engine::toolbox::ControlOp::Gain { controller: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::TargetPlayer), duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration })))),
                    Op::ChangeAbilitySource {parts, duration} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(spellwake_engine::toolbox::ObjectQuery::Source, ResolvedPartsSpec(parts.into_iter().map(|p|p.native(vocabulary)).collect::<Result<Vec<_>,_>>()?), duration.into()))),
                    Op::DoubleCountersOnTargets {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Double { kind: None })))),
                    Op::Proliferate {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::proliferate())),
                    Op::ManifestTop {player,amount}=>Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::manifest_top(player.into(),amount.native(vocabulary)?))),
                    Op::PutHandPermanentsOntoBattlefield {
                        player,
                        types,
                        maximum,
                        tapped,
                    } => Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::put_hand_permanents_onto_battlefield(
                            player.into(),
                            card_types(types)?,
                            maximum,
                            tapped,
                        ),
                    )),
                    Op::DamageCreaturesExcept { subtype, amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(spellwake_engine::program::DamageRecipientsRef::Group(spellwake_engine::program::DamageGroup::CreaturesExcept(creature_types::id(subtype, vocabulary)?)))), amount: spellwake_engine::program::DamageAmountSpec::Expr(spellwake_engine::program::ValueExpr::Literal(amount)), exile_if_dies: false })),
                    Op::CreateOpponentLoyaltyAnyTurnEmblems {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::LoyaltyAnyTurn))),
                    Op::CreatePreventAllButOneEmblem {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::PreventAllButOne))),
                    Op::CreateOpponentPreventAllButOneEmblems {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::PreventAllButOne))),
                    Op::CreateLoyaltyAnyTurnEmblem {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::LoyaltyAnyTurn))),
                    Op::CreateUpkeepDamageEmblem { amount } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::UpkeepDamage { amount }))),
                    Op::CreateSpellcastDamageEmblem { amount, noncreature_only } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::SpellcastDamage { amount, noncreature_only }))),
                    Op::CreateUnlimitedHandWithGraveyardReturnEmblem {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::UnlimitedHandWithGraveyardReturn))),
                    Op::CreateReturnFallenArtifactsEmblem {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::ReturnFallenArtifacts))),
                    Op::CreateOpponentUnlimitedHandWithGraveyardReturnEmblems {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::UnlimitedHandWithGraveyardReturn))),
                    Op::CreateOpponentReturnFallenArtifactsEmblems {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::ReturnFallenArtifacts))),
                    Op::CreateSacrificeCreatureGainLifeDrawEmblem {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::SacrificeCreatureGainLifeDraw))),
                    Op::CreateOpponentSacrificeCreatureGainLifeDrawEmblems {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), spellwake_engine::emblem::EmblemDefinition::SacrificeCreatureGainLifeDraw))),
                    Op::CreateEmblem { power_toughness, keywords } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::emblem(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::emblem::EmblemDefinition::ControlledCreatureBonus { power_toughness: power_toughness.into(), keywords: keyword_set(keywords)? }))),
                    Op::DrawController { amount } => Instruction::DrawCards(Box::new(spellwake_engine::toolbox::DrawCardsSpec::simple(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), amount.native(vocabulary)?))),
                    Op::ModifyAnyNumberOfPermanents {parts,duration} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(ResolvedPartsSpec(parts.into_iter().map(|part|part.native(vocabulary)).collect::<Result<_,_>>()?),duration.into()))),
                    Op::MillEachPlayer {amount} => Instruction::ForEachPlayerQuery { selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachPlayer)), bind_player: spellwake_engine::toolbox::BindingId(905), body: Box::new(spellwake_engine::program::ExecutableBody::new(spellwake_engine::program::BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::ZoneScan { zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(905)))), filter: Box::new(spellwake_engine::program::ObjectFilter::any()), slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(spellwake_engine::program::ValueExpr::literal(i64::from(amount)))) }, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Mill })))])) },
                    Op::ExileOnePerPlayerUntilSourceLeaves {} => Instruction::MutatePlayer(
                        Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::exile_one_per_player_until_source_leaves(),
                        ),
                    ),
                    Op::Populate {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::populate())),
                    Op::Bolster {amount} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::bolster(amount))),
                    Op::Amass { army, subtype, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::amass(
                            creature_types::id(army, vocabulary)?,
                            creature_types::id(subtype, vocabulary)?,
                            amount,
                        ),
                    )),
                    Op::ExileChosenPermanents {
                        players,
                        amount,
                        filter,
                    } => {
                        let amt = amount.native(vocabulary)?;
                        let ptf = filter.native()?;
                        let mut clause = spellwake_engine::program::ObjectClause::any();
                        if !ptf.any_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.any_types { m = m.with(*ct); } clause = clause.any_types(m); }
                        if !ptf.required_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.required_types { m = m.with(*ct); } clause = clause.required_types(m); }
                        if !ptf.forbidden_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.forbidden_types { m = m.with(*ct); } clause = clause.forbid_types(m); }
                        let q = spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(902)))), spellwake_engine::program::ObjectFilter::from_clause(clause));
                        Instruction::ForEachPlayerQuery { selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players.into())), bind_player: spellwake_engine::toolbox::BindingId(902), body: Box::new(spellwake_engine::program::ExecutableBody::new(spellwake_engine::program::BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(q, spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(902)), min: Box::new(amt.clone()), max: Box::new(amt), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: false }, spellwake_engine::toolbox::ZoneDest::exile())))])) }
                    },
                    Op::ReturnControlledPermanents {
                        players,
                        amount,
                        filter,
                    } => {
                        let amt = amount.native(vocabulary)?;
                        let ptf = filter.native()?;
                        let mut clause = spellwake_engine::program::ObjectClause::any();
                        if !ptf.any_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.any_types { m = m.with(*ct); } clause = clause.any_types(m); }
                        if !ptf.required_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.required_types { m = m.with(*ct); } clause = clause.required_types(m); }
                        if !ptf.forbidden_types.is_empty() { let mut m = spellwake_engine::program::CardTypeMask::EMPTY; for ct in &ptf.forbidden_types { m = m.with(*ct); } clause = clause.forbid_types(m); }
                        let q = spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(902)))), spellwake_engine::program::ObjectFilter::from_clause(clause));
                        Instruction::ForEachPlayerQuery { selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players.into())), bind_player: spellwake_engine::toolbox::BindingId(902), body: Box::new(spellwake_engine::program::ExecutableBody::new(spellwake_engine::program::BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(q, spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(902)), min: Box::new(amt.clone()), max: Box::new(amt), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: false }, spellwake_engine::toolbox::ZoneDest::Hand)))])) }
                    },
                    Op::Sacrifice {
                        players,
                        amount,
                        filter,
                    } => Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                            players.into(),
                            amount.native(vocabulary)?,
                            filter.native()?,
                        ),
                    )),
                    Op::DiscardOneAtRandom { player } => { let p: ResolvingPlayer = player.map_or(ResolvingPlayer::Controller, Into::into); Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(p))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneSelection::Random { count: Box::new(spellwake_engine::program::ValueExpr::literal(1)), chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(p) }, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard }))) }
                    Op::SacrificeRandomUntilLand {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::sacrifice_random_until_land(),
                    )),
                    Op::DiscardUpTo {maximum} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), min: Box::new(spellwake_engine::program::ValueExpr::literal(0)), max: Box::new(spellwake_engine::program::ValueExpr::literal(i64::from(maximum))), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: true }, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard }))),
                    Op::SacrificeMatching { subtype } => { let f = match subtype { Some(s) => spellwake_engine::program::ObjectFilter::creature().with_subtype(creature_types::id(s, vocabulary)?).controlled(), None => spellwake_engine::program::ObjectFilter::creature().nontoken().controlled() }; Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, f), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), min: Box::new(spellwake_engine::program::ValueExpr::literal(1)), max: Box::new(spellwake_engine::program::ValueExpr::literal(1)), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: false }, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice }))) },
                    Op::Discard { players, amount } => Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::discard(
                            players.into(),
                            amount.native(vocabulary)?,
                        ),
                    )),
                    Op::ChooseCreatureType { into, excluding } => Instruction::PromptChoice(
                        Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                                creature_types::slot(into)?,
                                subtype_set(excluding, vocabulary, "excluded creature type")?,
                            ),
                        ),
                    ),
                    Op::Damage { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets, amount: spellwake_engine::program::DamageAmountSpec::Expr(spellwake_engine::program::ValueExpr::Literal(amount)), exile_if_dies: false })),
                    Op::CreatePreventionShield { amount } => Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec {
                            selector: None,
                            payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield { amount },
                            duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                            condition: None,
                            one_shot: true,
                            as_emblem: None,
                            emblem_label: None,
                        },
                    )),
                    Op::CreateDamageMultiplier { factor } => {
                        Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec::damage_multiplier(factor),
                        ))
                    }
                    Op::Afflict { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Afflict(amount),
                        ),
                    )),
                    Op::Dethrone {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Dethrone,
                        ),
                    )),
                    Op::Melee {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Melee,
                        ),
                    )),
                    Op::Renown { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Renown(amount),
                        ),
                    )),
                    Op::Training {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Training,
                        ),
                    )),
                    Op::Prowess {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Prowess,
                        ),
                    )),
                    Op::BattleCry {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::BattleCry,
                        ),
                    )),
                    Op::Bushido { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Bushido(amount),
                        ),
                    )),
                    Op::Decayed {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Decayed,
                        ),
                    )),
                    Op::Mobilize { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Mobilize(amount),
                        ),
                    )),
                    Op::Ingest {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Ingest,
                        ),
                    )),
                    Op::Poisonous { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Poisonous(amount),
                        ),
                    )),
                    Op::Frenzy { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Frenzy(amount),
                        ),
                    )),
                    Op::Rampage { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Rampage(amount),
                        ),
                    )),
                    Op::Flanking {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_keyword_trigger(
                            CombatTriggerKeyword::Flanking,
                        ),
                    )),
                    Op::SearchLibrary { filter } => Instruction::SearchLibrary { filter: Box::new(filter.native(vocabulary)?) },
                    Op::Ninjutsu {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::ninjutsu(),
                    )),
                    Op::Unearth {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::unearth(),
                    )),
                    Op::ResolveCounterKeyword { keyword } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_counter_keyword(
                            keyword.native(),
                        ),
                    )),
                    Op::Graft { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_transfer(
                            spellwake_engine::program::TransferKeyword::Graft(amount),
                        ),
                    )),
                    Op::Modular { amount } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_transfer(
                            spellwake_engine::program::TransferKeyword::Modular(amount),
                        ),
                    )),
                    Op::Soulshift { maximum } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_transfer(
                            spellwake_engine::program::TransferKeyword::Soulshift(maximum),
                        ),
                    )),
                    Op::CreatureDealsPowerDamageToOther { trample_overflow } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(trample_overflow))),
                    Op::DamageFromPower { source } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::from_power(match source.as_str() {
                        "AbilitySource" => spellwake_engine::program::PowerDamageSource::AbilitySource,
                        "GraveyardEntrant" => spellwake_engine::program::PowerDamageSource::GraveyardEntrant,
                        "CreatureDealtDamage" => spellwake_engine::program::PowerDamageSource::CreatureDealtDamage,
                        "TargetCreature" => spellwake_engine::program::PowerDamageSource::TargetCreature,
                        "TargetCreatureToAbilitySource" => spellwake_engine::program::PowerDamageSource::TargetCreatureToAbilitySource,
                        _ => return Err(format!("unknown power damage source {source}")),
                    }))),
                    Op::DamageAmountThenExileIfDies { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets, amount: spellwake_engine::program::DamageAmountSpec::Expr(amount.native(vocabulary)?), exile_if_dies: true })),
                    Op::DamageAmount { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets, amount: spellwake_engine::program::DamageAmountSpec::Expr(amount.native(vocabulary)?), exile_if_dies: false })),
                    Op::DrainOpponents { amount } => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::drain(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent), amount.native(vocabulary)?, spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), true))),
                    Op::RegenerateTarget {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Regenerate))),
                    Op::ExileDefeatedSiege {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::exile_defeated_siege(),
                    )),
                    Op::CastDefeatedSiege {} => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::cast_defeated_siege(),
                    )),
                    Op::ReturnSourceToHand {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ZoneDest::Hand))),
                    Op::RelatedPlayerLosesHalfLife {} => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose_half(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::UniqueRelatedPlayer), true))),
                    Op::PutCountersOnControlledCreatures {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().controlled()))), spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::PlusOne, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutCountersOnControlledSubtype { subtype } => {
                        Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().controlled().with_subtype(creature_types::id(subtype, vocabulary)?)))), spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::PlusOne, amount: ValueExpr::Literal(1.into()) }))))
                    }
                    Op::PayControllerLife {amount} => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::pay(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::program::ValueExpr::literal(i64::from(amount))))),
                    Op::Encore {} => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::encore(),
                    )),
                    Op::ScheduleCreatedTokenSacrifice {} => Instruction::ScheduleDelayedStepTrigger {
                        step: spellwake_engine::turn::Step::End,
                        own_step_only: false,
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                                },
                            )))],
                        )),
                    },
                    Op::ScheduleCreatedTokenExile { at } => Instruction::ScheduleDelayedStepTrigger {
                        step: step(&at)?,
                        own_step_only: false,
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            )))],
                        )),
                    },
                    Op::PutCountersOnOtherCreatures {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().controlled().other_than_ability_source()))), spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::PlusOne, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutNamedCounterOnSource { kind, amount } => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        spellwake_engine::toolbox::ObjectMutation::Counters(
                            spellwake_engine::toolbox::CounterOp::Add {
                                kind: spellwake_engine::effects::CounterKind::PERMANENT.into_iter()
                                    .find(|value| format!("{value:?}") == kind).ok_or("unknown permanent counter")?,
                                amount: ValueExpr::Literal(i64::from(amount.unwrap_or(1)).into()),
                            },
                        ),
                    ))),
                    Op::PutBlessingCounterOnSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Blessing, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutChargeCounterOnSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Charge, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutQuestCounterOnSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Quest, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutStorageCounterOnSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Storage, amount: ValueExpr::Literal(1.into()) })))),
                    Op::UntapAbilitySource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(false))))),
                    Op::TapAbilitySource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(true))))),
                    Op::TransformSubtype { subtype } => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, spellwake_engine::program::ObjectFilter::creature().with_subtype(creature_types::id(subtype, vocabulary)?)), spellwake_engine::toolbox::ObjectMutation::Orientation(spellwake_engine::toolbox::OrientationOp::Transform)))),
                    Op::ReturnOwnGraveyardCardsOfTypeToHand { kind } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), spellwake_engine::program::ObjectFilter::from_clause(spellwake_engine::program::ObjectClause::any().any_types(spellwake_engine::program::CardTypeMask::from_card_type(card_type(&kind)?)))), spellwake_engine::toolbox::ZoneDest::Hand))),
                    Op::ExileTargetWithLink { group } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: false, delayed_return: None, link_group: Some(group.into()) }))),
                    Op::ReturnCardsExiledWithLink { group } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: Some(group.into()) }, spellwake_engine::toolbox::ZoneDest::battlefield(spellwake_engine::toolbox::ZoneController::Owner)))),
                    Op::GrantFlashback { targeted } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::all(
                            if targeted {
                                spellwake_engine::toolbox::ObjectQuery::Targets
                            } else {
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                                            spellwake_engine::program::ResolvingPlayer::Controller,
                                        ),
                                    )),
                                    spellwake_engine::program::ObjectFilter::instant_or_sorcery(),
                                )
                            },
                            spellwake_engine::toolbox::ObjectMutation::StateMarker(spellwake_engine::toolbox::ObjectStateMarkerOp::GrantFlashback),
                        ),
                    )),
                    Op::TransformSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Orientation(spellwake_engine::toolbox::OrientationOp::Transform)))),
                    Op::RegenerateSource {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Regenerate))),
                    Op::RemoveTargetFromCombat {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Combat(spellwake_engine::toolbox::ObjectCombatOp::RemoveFromCombat)))),
                    Op::RegenerateControlledSubtype { subtype } => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().controlled().with_subtype(creature_types::id(subtype, vocabulary)?)))),
                        spellwake_engine::toolbox::ObjectMutation::Regenerate,
                    ))),
                    Op::RequireTargetAttack {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets), payload: spellwake_engine::toolbox::ContinuousPayload::CombatAction(spellwake_engine::program::CombatActionKind::MustAttack), duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })),
                    Op::GoadTarget {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Goad { by: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), permanent: false }))),
                    Op::GoadOtherPlayersCreatures {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().not_controlled()))), spellwake_engine::toolbox::ObjectMutation::Goad { by: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), permanent: false }))),
                    Op::ExtortDrain {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::extort_drain(),
                    )),
                    Op::TapEnchantedCreature {}=>Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::AttachedHost, spellwake_engine::toolbox::ObjectMutation::TapState(spellwake_engine::toolbox::TapStateOp::Set(true))))),
                    Op::RequireMonarch { opponent } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::Monarch { opponent },
                        ),
                    )),
                    Op::RequireOwnCommander {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::OwnCommander,
                        ),
                    )),
                    Op::RequireOpponentMoreLands {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::OpponentMoreLands,
                        ),
                    )),
                    Op::RequireNoLoyaltyActivation {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::NoLoyaltyActivation,
                        ),
                    )),
                    Op::ReselectAttackDefender {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::reselect_attack_defender())),
                    Op::SetBlockDeclarations {rule} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::block_declarations(match rule.as_str() {
                        "EachOpponentOneBlocker" => spellwake_engine::program::BlockDeclarationRule::EachOpponentOneBlocker,
                        "ControllerChooses" => spellwake_engine::program::BlockDeclarationRule::ControllerChooses,
                        _ => return Err("unknown blocker-declaration rule".into()),
                    }))),
                    Op::AdditionalLoyaltyActivation {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::additional_loyalty_activation())),
                    Op::AbilitySourceControllerLosesLife {amount} => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(spellwake_engine::toolbox::PlayerSelector::ControllersOf(Box::new(spellwake_engine::toolbox::ObjectQuery::Source)), spellwake_engine::program::ValueExpr::literal(i64::from(amount))))),
                    Op::RelatedPlayerLosesLife {amount} => Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::UniqueRelatedPlayer), spellwake_engine::program::ValueExpr::literal(i64::from(amount))))),
                    Op::RequireOpponentLifeLoss { amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::OpponentLifeLoss(
                                amount,
                            ),
                        ),
                    )),
                    Op::RequireSourceLoyalty {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::SourceLoyalty,
                        ),
                    )),
                    Op::RequireSourceQuestCounters { amount } => Instruction::MutatePlayer(
                        Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                spellwake_engine::program::InterveningCondition::SourceQuestCounters(
                                    amount,
                                ),
                            ),
                        ),
                    ),
                    Op::RequireGreatestArtifact {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::GreatestArtifact,
                        ),
                    )),
                    Op::RequireNoOtherControlledCreatures {} => Instruction::MutatePlayer(
                        Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                spellwake_engine::program::InterveningCondition::NoOtherControlledCreatures,
                            ),
                        ),
                    ),
                    Op::RequireControlledCreatureCount { count } => Instruction::MutatePlayer(
                        Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                spellwake_engine::program::InterveningCondition::ControlledCreatureCount(
                                    count,
                                ),
                            ),
                        ),
                    ),
                    Op::Cascade {} => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::cascade(),
                    )),
                    Op::FlipCoin { lose_destination } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::flip_coins(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::CoinFlipMode::Single,
                            None,
                            None,
                            Some(lose_destination),
                        ),
                    )),
                    Op::CascadeBottom {} => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::cascade_bottom(),
                    )),
                    Op::BindOrderedPermanents { .. } => return Err("BindOrderedPermanents retired".to_owned()),
                    Op::ExileLibraryTop {player,amount} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::ZoneScan { zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(player.into()))), filter: Box::new(spellwake_engine::program::ObjectFilter::default()), slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(ValueExpr::Literal(i64::from(amount).into()))) }, spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::FixedTargetClause { body } => Instruction::FixedTargetClause(Box::new(body.native(vocabulary)?)),
                    Op::ExileResolvingSpell {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::FightFixedTargets {} => Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::fight(
                            spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                spellwake_engine::program::MoveSubjectRef::RememberedFirst,
                            ),
                            spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                spellwake_engine::program::MoveSubjectRef::RememberedSecond,
                            ),
                        ),
                    )),
                    Op::PairWithCreature { entrant_only } => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::soulbond_pair(entrant_only))),
                    Op::PhaseOut { subject } => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(match subject.as_str() {
                        "Target" => spellwake_engine::toolbox::ObjectQuery::Targets,
                        "Source" => spellwake_engine::toolbox::ObjectQuery::Source,
                        "ControlledLands" => spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::land().controlled()))),
                        _ => return Err(format!("unknown phasing subject {subject}")),
                    }, spellwake_engine::toolbox::ObjectMutation::Orientation(spellwake_engine::toolbox::OrientationOp::PhaseOut)))),
                    Op::AddRestrictedManaChoice { colors } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::EffectManaYield::RestrictedChoice { colors: colors.iter().map(|value| mana(value)).collect::<Result<_, _>>()?, amount: spellwake_engine::program::ValueExpr::literal(1), restriction: spellwake_engine::mana::ManaRestriction::NoncreatureSpell },
                        ),
                    )),
                    Op::AddCombatDamageManaUntilCleanup { kind } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::EffectManaYield::CombatDamageUntilCleanup(mana(&kind)?),
                        ),
                    )),
                    Op::AddCountedMana { kind, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::EffectManaYield::Counted { kind: mana(&kind)?, amount: amount.native(vocabulary)? },
                        ),
                    )),
                    Op::AddSplitMana { first, second, amount } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::EffectManaYield::Split {
                                first: mana(&first)?, second: mana(&second)?, amount: amount.native(vocabulary)?,
                            },
                        ),
                    )),
                    Op::DamageRelatedPlayerAmount { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Players(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::UniqueRelatedPlayer)), amount: spellwake_engine::program::DamageAmountSpec::Expr(amount.native(vocabulary)?), exile_if_dies: false })),
                    Op::DamageOpponentsAmount { amount } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec { source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource, recipients: spellwake_engine::toolbox::DamageTargetQuery::Players(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachOpponent)), amount: spellwake_engine::program::DamageAmountSpec::Expr(amount.native(vocabulary)?), exile_if_dies: false })),
                    Op::ReturnGraveyardTargetsTapped { count: _ } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Battlefield { controller: spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), tapped: true, attacking: false, transformed: false, face_down: None, entering_counters: Vec::new(), attach_to: None }))),
                    Op::ReturnChosenXGraveyardCreatures {} => Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::return_chosen_x_graveyard_creatures(),
                    )),
                    Op::DamageTargetControllersCreatures { amount, include_target, require_damaged_creatures_to_attack } =>
                        Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::target_controllers_creatures(amount.native(vocabulary)?, include_target, require_damaged_creatures_to_attack))),
                    Op::DivideThreeDamageAmongTargets {} => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::divide_three())),
                    Op::DamageOrderedTargets { first, others } => Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::ordered_targets(first, others))),
                    Op::CopyOnCast { keyword, replicate } => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_on_cast(
                            match (keyword.as_str(), replicate) {
                                ("CopyThisPermanentSpell", None) => {
                                    spellwake_engine::program::CastCopyKeyword::CopyThisPermanentSpell
                                }
                                ("CopyObservedSpell", None) => {
                                    spellwake_engine::program::CastCopyKeyword::CopyObservedSpell
                                }
                                ("OfferCopyToOtherPlayers", None) => {
                                    spellwake_engine::program::CastCopyKeyword::OfferCopyToOtherPlayers
                                }
                                ("Storm", None) => {
                                    spellwake_engine::program::CastCopyKeyword::Storm
                                }
                                ("Gravestorm", None) => {
                                    spellwake_engine::program::CastCopyKeyword::Gravestorm
                                }
                                ("Replicate", Some(index)) => {
                                    spellwake_engine::program::CastCopyKeyword::Replicate(index)
                                }
                                _ => return Err("invalid cast-copy keyword".into()),
                            },
                        ),
                    )),
                    Op::Mentor {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_growth(
                            spellwake_engine::program::GrowthKeyword::Mentor,
                        ),
                    )),
                    Op::Increment {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_growth(
                            spellwake_engine::program::GrowthKeyword::Increment,
                        ),
                    )),
                    Op::Evolve {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_growth(
                            spellwake_engine::program::GrowthKeyword::Evolve,
                        ),
                    )),
                    Op::Backup {
                        amount,
                        printed_below,
                    } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_growth(
                            spellwake_engine::program::GrowthKeyword::Backup {
                                amount,
                                printed_below: keyword_set(printed_below)?,
                            },
                        ),
                    )),
                    Op::SacrificeEvoked {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::sacrifice_evoked(),
                    )),
                    Op::SacrificeStateTriggerSource {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::sacrifice_state_trigger_source(),
                    )),
                    Op::CounterWardSource {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_ward_source())),
                    Op::ModifyGroupByAmount { group, scale, amount, keywords } => { let s: spellwake_engine::characteristics::PowerToughness = scale.into(); let amt = amount.native(vocabulary)?; Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(effect_group(&group, vocabulary)?))), payload: spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { power: amt.clone().scaled(s.power), toughness: amt.scaled(s.toughness), keywords: keyword_set(keywords)?, suppress_keywords: BTreeSet::new() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })) },
                    Op::AddExperienceCounter {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::add_counter(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), spellwake_engine::effects::CounterKind::Experience, spellwake_engine::program::ValueExpr::literal(1)))),
                    Op::GrantControllerFlashUntilCleanup {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: None, payload: spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(Box::new(spellwake_engine::program::TemporaryRuleKind::FilteredFlash { player: spellwake_engine::program::ResolvingPlayer::Controller, filter: spellwake_engine::program::ObjectFilter::default(), next_only: false })), duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })),
                    Op::RequireBlocksAgainstTargetThisTurn {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets), payload: spellwake_engine::toolbox::ContinuousPayload::CombatAction(spellwake_engine::program::CombatActionKind::MustBeBlocked), duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })),
                    Op::RequireOpponentsCreaturesBlockThisTurn {} => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(spellwake_engine::program::EffectSubject::Matching(spellwake_engine::program::ObjectFilter::creature().not_controlled())))), payload: spellwake_engine::toolbox::ContinuousPayload::CombatAction(spellwake_engine::program::CombatActionKind::MustBlock), duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })),
                    Op::SuppressGroupKeywords { group, keywords } => Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(effect_group(&group, vocabulary)?))), payload: spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { power: ValueExpr::Literal(0.into()), toughness: ValueExpr::Literal(0.into()), keywords: BTreeSet::new(), suppress_keywords: keyword_set(keywords)? }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })),
                    Op::ModifyGroupUntilEndOfTurn { group, power_toughness, keywords } => {
                        let group = Box::new(effect_group(&group, vocabulary)?);
                        { let pt: Option<spellwake_engine::characteristics::PowerToughness> = power_toughness.map(Into::into); let (pw, tw) = pt.map_or((0.into(), 0.into()), |v| (v.power, v.toughness)); Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(group)), payload: spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { power: ValueExpr::Literal(pw), toughness: ValueExpr::Literal(tw), keywords: keyword_set(keywords)?, suppress_keywords: BTreeSet::new() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })) }
                    }
                    Op::BoostSource{power_toughness,keywords}=>{ let pt: Option<spellwake_engine::characteristics::PowerToughness> = power_toughness.map(Into::into); let (pw, tw) = pt.map_or((0.into(), 0.into()), |v| (v.power, v.toughness)); Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::Source), payload: spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { power: ValueExpr::Literal(pw), toughness: ValueExpr::Literal(tw), keywords: keyword_set(keywords)?, suppress_keywords: BTreeSet::new() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })) },
                    Op::BoostLoneAttacker {} => return Err("BoostLoneAttacker retired".to_owned()),
                    Op::Equip {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            spellwake_engine::attachment::AttachAbility::Equip,
                        ),
                    )),
                    Op::Reconfigure {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            spellwake_engine::attachment::AttachAbility::Reconfigure,
                        ),
                    )),
                    Op::ResolvePowerAbility { kind } => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::resolve_power_ability(
                            match kind.as_str() {
                                "Station" => spellwake_engine::activated::PowerAbility::Station,
                                "Crew" => spellwake_engine::activated::PowerAbility::Crew,
                                "Saddle" => spellwake_engine::activated::PowerAbility::Saddle,
                                _ => return Err("unknown power ability".into()),
                            },
                        ),
                    )),
                    Op::PutCountersOnSource { amount } => {
                        Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::PlusOne, amount: ValueExpr::Literal(amount) }))))
                    }
                    Op::Unattach {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ObjectMutation::Attach(spellwake_engine::toolbox::AttachOp::Unattach)))),
                    Op::Fortify {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            spellwake_engine::attachment::AttachAbility::Fortify,
                        ),
                    )),
                    Op::ExileTarget {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::ExileAbilitySource {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::ExileAllGraveyards {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachPlayer))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::ShuffleOwnLibrary {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::shuffle(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller)))),
                    Op::ShuffleEachHandAndGraveyard {} => Instruction::ForEachPlayerQuery { selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(spellwake_engine::program::AffectedPlayers::EachPlayer)), bind_player: spellwake_engine::toolbox::BindingId(901), body: Box::new(spellwake_engine::program::ExecutableBody::new(spellwake_engine::program::BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Union(vec![spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(901)))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(901)))), spellwake_engine::program::ObjectFilter::any())]), spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto })))])) },
                    Op::EndControllerTurn {} => Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::end_turn())),
                    Op::ShuffleOwnGraveyardIntoLibrary {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::ZoneScan { zone: spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), filter: Box::new(spellwake_engine::program::ObjectFilter::default()), slice: spellwake_engine::toolbox::QuerySlice::All }, spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto }))),
                    Op::ReturnTargetGraveyardCreatureToHand {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Hand))),
                    Op::ReturnRandomGraveyardCards { count, kind, subtype, to_battlefield } => {
                        use spellwake_engine::program::ObjectFilter as F;
                        let filter = match (kind.as_deref(), subtype) {
                            (None, None) => None,
                            (Some("Creature"), None) => Some(Box::new(F::creature())),
                            (Some("InstantOrSorcery"), None) => Some(Box::new(F::instant_or_sorcery())),
                            (None, Some(subtype)) => Some(Box::new(F::any().with_subtype(creature_types::id(subtype, vocabulary)?))),
                            _ => return Err("unsupported random graveyard card filter".into()),
                        };
                        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), filter.map_or_else(spellwake_engine::program::ObjectFilter::any, |b| *b)), spellwake_engine::toolbox::ZoneSelection::Random { count: Box::new(spellwake_engine::program::ValueExpr::literal(i64::from(count.get()))), chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller) }, if to_battlefield { spellwake_engine::toolbox::ZoneDest::battlefield(spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller)))) } else { spellwake_engine::toolbox::ZoneDest::Hand })))
                    }
                    Op::ChooseRevealedHandCard {
                        minimum_mana_value,
                        nonland_only,
                        count,
                        exile,
                    } => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                            minimum_mana_value,
                            nonland_only,
                            None,
                            count.unwrap_or(std::num::NonZeroU8::MIN),
                            if exile {
                                spellwake_engine::program::ChosenHandAction::Exile
                            } else {
                                spellwake_engine::program::ChosenHandAction::Discard
                            },
                        ),
                    )),
                    Op::ExileGraveyardTarget {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::exile()))),
                    Op::ExileCardsFromGraveyard { player, count } => { let p: spellwake_engine::program::ResolvingPlayer = player.into(); let n = spellwake_engine::program::ValueExpr::literal(i64::from(count.get())); Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(p))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(p), min: Box::new(n.clone()), max: Box::new(n), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: false }, spellwake_engine::toolbox::ZoneDest::exile()))) }
                    Op::PutOwnHandCardOnLibraryBottom {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Hand(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneSelection::Prompt { chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller), min: Box::new(spellwake_engine::program::ValueExpr::literal(1)), max: Box::new(spellwake_engine::program::ValueExpr::literal(1)), constraint: None, category: spellwake_engine::toolbox::ChoiceCategory::Generic, hidden: true }, spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::Bottom }))),
                    Op::PutTargetInLibrary { placement } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Library { position: match placement.as_str() { "Top" => spellwake_engine::toolbox::LibraryOrderPos::Top, "Bottom" => spellwake_engine::toolbox::LibraryOrderPos::Bottom, _ => return Err("invalid library placement".into()) } }))),
                    Op::PutGroupOnLibraryBottom { group } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, permanent_group(&group, vocabulary)?), spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::Bottom }))),
                    Op::ReturnSourceGraveyardToBattlefield { tapped } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ZoneDest::Battlefield { controller: spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), tapped, attacking: false, transformed: false, face_down: None, entering_counters: Vec::new(), attach_to: None }))),
                    Op::ReturnSourceGraveyardToHand {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Source, spellwake_engine::toolbox::ZoneDest::Hand))),
                    Op::ExileTargetUntilSourceLeaves {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: true, delayed_return: None, link_group: None }))),
                    Op::ExileUntilNextEndStep {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: false, delayed_return: Some(spellwake_engine::program::DelayedReturnTiming::NextEndStep), link_group: None }))),
                    Op::ReturnToOwnersHand {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Hand))),
                    Op::ReturnOwnGraveyardCreatureToBattlefield {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::battlefield(spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))))))),
                    Op::ReturnAnyGraveyardCreatureOrPlaneswalker {} => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::battlefield(spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))))))),
                    Op::ReturnOwnGraveyardPermanentToBattlefield { tapped } => Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Battlefield { controller: spellwake_engine::toolbox::ZoneController::Player(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), tapped, attacking: false, transformed: false, face_down: None, entering_counters: Vec::new(), attach_to: None }))),
                    Op::CounterSpell {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Graveyard))),
                    Op::WatchNextSpellOrLoyaltyTwice {}=>Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::watch_next_spell_or_loyalty(false))),
                    Op::Myriad {} => Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::myriad(),
                    )),
                    Op::RequireCastFromGraveyard {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::CastFromGraveyard,
                        ),
                    )),
                    Op::WatchNextLoyaltyCopy {}=>Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::watch_next_spell_or_loyalty(true))),
                    Op::CopyAbility {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::copy_targeted_ability())),
                    Op::CopySpell {} => Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell())),
                    Op::CopyTargetToSource {
                        duration,
                        power_toughness,
                    } => Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::become_copy(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                spellwake_engine::program::EffectSubject::AbilitySource,
                            )),
                            spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                spellwake_engine::program::MoveSubjectRef::TargetObject,
                            ),
                            duration.into(),
                            spellwake_engine::program::CopyExceptions {
                                override_pt: power_toughness.map(Into::into),
                                ..Default::default()
                            },
                        ),
                    )),
                    Op::PutIndestructibleCounter {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Indestructible, amount: ValueExpr::Literal(1.into()) })))),
                    Op::PutLoyaltyCounter {} => Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::Counters(spellwake_engine::toolbox::CounterOp::Add { kind: spellwake_engine::effects::CounterKind::Loyalty, amount: ValueExpr::Literal(1.into()) })))),
                    Op::GrantProtectionChosenColor { for_controller_and_permanents } => Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::chosen_color_protection(
                            if for_controller_and_permanents { spellwake_engine::program::ProtectionRecipients::ControllerAndPermanents }
                            else { spellwake_engine::program::ProtectionRecipients::TargetPermanent },
                            spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                        ),
                    )),
                    Op::PutPowerToughnessCounters { counters } => {
                        let c: spellwake_engine::characteristics::PowerToughnessCounters = counters.into();
                        let (kind, amount) = if c.minus_one.is_positive() {
                            (spellwake_engine::effects::CounterKind::MinusOne, c.minus_one)
                        } else {
                            (spellwake_engine::effects::CounterKind::PlusOne, c.plus_one)
                        };
                        Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ObjectMutation::Counters(
                                spellwake_engine::toolbox::CounterOp::Add {
                                    kind,
                                    amount: ValueExpr::Literal(amount),
                                },
                            ),
                        )))
                    }
                    Op::MayRevealLibraryTop {} => Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::may_reveal_library_top(),
                    )),
                    Op::BoostTargetPower { amount, keywords, also_toughness } => { let amt = amount.native(vocabulary)?; let tw = if also_toughness { amt.clone() } else { ValueExpr::Literal(0.into()) }; Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets), payload: spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { power: amt, toughness: tw, keywords: keyword_set(keywords)?, suppress_keywords: BTreeSet::new() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: false, as_emblem: None, emblem_label: None })) },
                    Op::CreateContinuousEffect { parts, duration } => {
                        Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec {
                            selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets),
                            payload: spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(Box::new(ResolvedPartsSpec(
                                parts
                                    .into_iter()
                                    .map(|part| part.native(vocabulary))
                                    .collect::<Result<Vec<_>, _>>()?,
                            ))),
                            duration: duration.into(),
                            condition: None, one_shot: false, as_emblem: None, emblem_label: None,
                        }))
                    }
                }),
            }
        }
        let mut body = ExecutableBody::new(target, instructions);
        if let Some((min, max)) = permanent_target_bounds {
            body = body.with_permanent_target_bounds(min, max);
        }
        Ok(body)
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum EntryTapWire {
    ReadAhead { final_chapter: u8 },
    Always {},
    UnlessTwoOtherLands {},
    UnlessTwoOrFewerOtherLands {},
    UnlessTwoBasicLands {},
    AlwaysChooseColorExcept { excluded: String },
    ChooseCreatureType {},
    ChosenOpponentControls {},
    ChooseTwoPlayers {},
    SiegeProtector {},
    UnlessTwoOpponents {},
    UnlessOpponentLandsEight {},
    UnlessPlayerLifeThirteenOrLess {},
    UnlessLandType { first: String, second: String },
    UnlessRevealHandLandType { first: String, second: String },
}
impl EntryTapWire {
    fn native(self) -> Result<spellwake_engine::land::EntryTap, String> {
        use spellwake_engine::land::{EntryTap, LandType};
        let basic = |label: &str| {
            LandType::from_wire_label(label)
                .and_then(LandType::basic)
                .ok_or_else(|| "expected a basic land type".to_owned())
        };
        Ok(match self {
            Self::ReadAhead { final_chapter } => EntryTap::ReadAhead { final_chapter },
            Self::SiegeProtector {} => EntryTap::SiegeProtector,
            Self::ChooseTwoPlayers {} => EntryTap::ChooseTwoPlayers,
            Self::Always {} => EntryTap::Always,
            Self::AlwaysChooseColorExcept { excluded } => {
                EntryTap::AlwaysChooseColorExcept(mana(&excluded)?)
            }
            Self::ChosenOpponentControls {} => EntryTap::ChosenOpponentControls,
            Self::ChooseCreatureType {} => EntryTap::ChooseCreatureType,
            Self::UnlessTwoOtherLands {} => EntryTap::UnlessTwoOtherLands,
            Self::UnlessTwoOrFewerOtherLands {} => EntryTap::UnlessTwoOrFewerOtherLands,
            Self::UnlessTwoBasicLands {} => EntryTap::UnlessTwoBasicLands,
            Self::UnlessTwoOpponents {} => EntryTap::UnlessTwoOpponents,
            Self::UnlessOpponentLandsEight {} => EntryTap::UnlessOpponentLandsEight,
            Self::UnlessPlayerLifeThirteenOrLess {} => EntryTap::UnlessPlayerLifeThirteenOrLess,
            Self::UnlessLandType { first, second } => {
                EntryTap::UnlessLandType(basic(&first)?, basic(&second)?)
            }
            Self::UnlessRevealHandLandType { first, second } => {
                EntryTap::UnlessRevealHandLandType(basic(&first)?, basic(&second)?)
            }
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeWire {
    minimum: u8,
    maximum: u8,
    allow_repeat: bool,
    additional: Vec<Body>,
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Kind {
    Permanent {
        #[serde(default)]
        attachment: Option<String>,
        #[serde(default)]
        entry_tap: Option<EntryTapWire>,
        #[serde(deserialize_with = "required_nullable")]
        printed_pt: Option<PrintedPt>,
        #[serde(deserialize_with = "required_nullable")]
        casting_cost: Option<Cost>,
    },
    Spell {
        #[serde(default)]
        modes: Option<ModeWire>,
        #[serde(deserialize_with = "object")]
        casting_cost: Cost,
        #[serde(deserialize_with = "object")]
        body: Body,
    },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Output {
    SourceChargeCounters,
    RemoveStorageCounters,
    RemoveChargeForColor,
    CommanderColorIdentity,
    CommanderIdentityWithSpendEffect {
        cost: Cost,
        effect: String,
    },
    EntryChosenColor {
        fixed: String,
    },
    TwoDistinctColors,
    PaidGreatestCreaturePower {
        cost: Cost,
    },
    TapControlledArtifact {
        produced: BTreeMap<String, RulesInteger>,
    },
    LandTappedWhileMonarch,
    ImprintedLandTappedSameType,
    LandTappedSameType,
    PermanentTappedForColorless,
    ChosenPlayerColor,
    FixedWithLifeGain {
        produced: BTreeMap<String, RulesInteger>,
        life: RulesInteger,
    },
    OpponentsLandColors,
    ChooseColorForCreatureType,
    ChooseColorForCreatureTypeUncounterable,
    ChooseColorForCreatureTypeOrAbility,
    SacrificeSource {
        tap: bool,
        #[serde(deserialize_with = "object")]
        output: Box<Output>,
    },
    SacrificeSubtype {
        subtype: Value,
        #[serde(deserialize_with = "object")]
        output: Box<Output>,
    },
    SacrificeCreature {
        #[serde(deserialize_with = "object")]
        output: Box<Output>,
    },
    RestrictedFixed {
        produced: BTreeMap<String, RulesInteger>,
        restriction: String,
    },
    RestrictedChooseColor {
        amount: RulesInteger,
        colors: Vec<String>,
        restriction: String,
    },
    PaidTwoDistinctRestricted {
        #[serde(deserialize_with = "object")]
        cost: Cost,
        restriction: String,
    },
    ControlledCreaturePowerColor {
        minimum_power: RulesInteger,
        otherwise: RulesInteger,
        matching: RulesInteger,
    },
    CountedColor {
        #[serde(deserialize_with = "object")]
        permanents: Selector,
    },
    HybridFilter {
        colors: [String; 2],
    },
    Conditional {
        #[serde(deserialize_with = "object")]
        selector: Selector,
        minimum: RulesInteger,
        #[serde(deserialize_with = "object")]
        output: Box<Output>,
    },
    ConditionalControlledLandTypes {
        requirements: Vec<Vec<String>>,
        normal: BTreeMap<String, RulesInteger>,
        fulfilled: BTreeMap<String, RulesInteger>,
    },
    Counted {
        mana_type: String,
        #[serde(deserialize_with = "object")]
        permanents: Selector,
    },
    Fixed {
        produced: BTreeMap<String, RulesInteger>,
    },
    PaidFixed {
        #[serde(deserialize_with = "object")]
        cost: Cost,
        produced: BTreeMap<String, RulesInteger>,
    },
    PaidChooseColor {
        #[serde(deserialize_with = "object")]
        cost: Cost,
        amount: RulesInteger,
        colors: Vec<String>,
    },
    ChooseColor {
        amount: RulesInteger,
        colors: Vec<String>,
    },
    ChooseColorWithControllerDamage {
        colors: Vec<String>,
        damage: RulesInteger,
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
    #[serde(default)]
    cost_subtype: Option<Value>,
    #[serde(default)]
    cost_filter: Option<String>,
    #[serde(default)]
    cost_count: Option<u8>,
    #[serde(default)]
    cost_tap_source: Option<bool>,
    #[serde(deserialize_with = "object")]
    body: Body,
}
impl Activated {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<ActivatedAbility, String> {
        use spellwake_engine::activated::{
            CounterCostDelta, DiscardCostMode, GraveyardExileCost, HandDiscardKeyword,
            LifeCostAmount, PowerAbility, SelectedSacrificeFilter, TapCostCount, TapCostFilter,
        };
        use spellwake_engine::effects::CounterKind;
        let nz = |n: Option<u8>| n.and_then(std::num::NonZeroU8::new);
        Ok(ActivatedAbility {
            id: ActivatedAbilityId(self.id),
            mana_cost: self.mana_cost.native()?,
            source_cost: match self.source_cost.as_str() {
                "GraveyardSacrificeArtifacts"
                    if self.cost_subtype.is_none()
                        && self.cost_filter.is_none()
                        && self.cost_tap_source.is_none() =>
                {
                    SourceCost::SacrificeSelected {
                        filter: SelectedSacrificeFilter::Artifact,
                        count: self
                            .cost_count
                            .filter(|count| *count > 0)
                            .ok_or("invalid cost count")?,
                        tap_source: false,
                        from_graveyard: true,
                    }
                }
                "TapSourceAndCreatures" => SourceCost::TapPermanents {
                    filter: TapCostFilter::Creature,
                    count: TapCostCount::Fixed(
                        nz(self.cost_count).ok_or("missing contributor count")?,
                    ),
                    tap_source: true,
                    exclude_source: true,
                },
                "TapSourceAndSubtype"
                    if self.cost_filter.is_none() && self.cost_tap_source.is_none() =>
                {
                    SourceCost::TapPermanents {
                        filter: TapCostFilter::Subtype(creature_types::id(
                            self.cost_subtype.ok_or("missing cost subtype")?,
                            vocabulary,
                        )?),
                        count: TapCostCount::Fixed(
                            nz(self.cost_count).ok_or("invalid cost count")?,
                        ),
                        tap_source: true,
                        exclude_source: true,
                    }
                }
                "TapSelectedArtifacts"
                    if self.cost_subtype.is_none()
                        && self.cost_filter.is_none()
                        && self.cost_tap_source.is_none() =>
                {
                    SourceCost::TapPermanents {
                        filter: TapCostFilter::Artifact,
                        count: TapCostCount::Fixed(
                            nz(self.cost_count).ok_or("invalid cost count")?,
                        ),
                        tap_source: false,
                        exclude_source: false,
                    }
                }
                "SacrificeSelected" if self.cost_filter.as_deref() == Some("Subtype") => {
                    SourceCost::SacrificeSelected {
                        filter: SelectedSacrificeFilter::Subtype(creature_types::id(
                            self.cost_subtype.ok_or("missing cost subtype")?,
                            vocabulary,
                        )?),
                        count: self
                            .cost_count
                            .filter(|count| *count > 0)
                            .ok_or("invalid cost count")?,
                        tap_source: self.cost_tap_source.ok_or("missing cost tap")?,
                        from_graveyard: false,
                    }
                }
                "SacrificeSelected" if self.cost_subtype.is_none() => {
                    SourceCost::SacrificeSelected {
                        filter: match self.cost_filter.as_deref().ok_or("missing cost filter")? {
                            "Artifact" => SelectedSacrificeFilter::Artifact,
                            "OtherEnchantment" => SelectedSacrificeFilter::OtherEnchantment,
                            "OtherCreature" => SelectedSacrificeFilter::OtherCreature,
                            "Creature" => SelectedSacrificeFilter::Creature,
                            "CreatureOrEnchantment" => {
                                SelectedSacrificeFilter::CreatureOrEnchantment
                            }
                            "ArtifactOrCreature" => SelectedSacrificeFilter::ArtifactOrCreature,
                            "Desert" => SelectedSacrificeFilter::Desert,
                            "GrantingPermanent" => SelectedSacrificeFilter::GrantingPermanent,
                            _ => return Err("invalid cost filter".into()),
                        },
                        count: self
                            .cost_count
                            .filter(|count| *count > 0)
                            .ok_or("invalid cost count")?,
                        tap_source: self.cost_tap_source.ok_or("missing cost tap")?,
                        from_graveyard: false,
                    }
                }
                label if label.starts_with("TapWithGenericX(") => SourceCost::GenericX {
                    symbols: label
                        .strip_prefix("TapWithGenericX(")
                        .and_then(|s| s.strip_suffix(')'))
                        .and_then(|s| s.parse().ok())
                        .ok_or("invalid X symbol count")?,
                    tap_source: true,
                },
                "PayGenericX" => SourceCost::GenericX {
                    symbols: 1,
                    tap_source: false,
                },
                "TapPutPage" => SourceCost::Counter {
                    kind: CounterKind::Page,
                    delta: CounterCostDelta::Add(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                "TapPutVerse" => SourceCost::Counter {
                    kind: CounterKind::Verse,
                    delta: CounterCostDelta::Add(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                "RemoveThreeQuestSacrifice" => SourceCost::Counter {
                    kind: CounterKind::Quest,
                    delta: CounterCostDelta::Remove(std::num::NonZeroU8::new(3).ok_or("nonzero")?),
                    tap_source: false,
                    sacrifice_source: true,
                },
                "TapMillOne" => SourceCost::Mill {
                    count: std::num::NonZeroU8::MIN,
                    tap_source: true,
                },
                "TapRemoveSuspect" => SourceCost::Counter {
                    kind: CounterKind::Suspect,
                    delta: CounterCostDelta::Remove(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                "TapRemoveCharge" => SourceCost::Counter {
                    kind: CounterKind::Charge,
                    delta: CounterCostDelta::Remove(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                "TapRemovePlusOne" => SourceCost::Counter {
                    kind: CounterKind::PlusOne,
                    delta: CounterCostDelta::Remove(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                "DiscardMatchingSpellManaValue" => SourceCost::Discard {
                    mode: DiscardCostMode::MatchingTargetSpellManaValue,
                    tap_source: false,
                    sacrifice_source: false,
                },
                "DiscardSelected"
                    if self.cost_subtype.is_none()
                        && self.cost_filter.is_none()
                        && self.cost_count.is_none() =>
                {
                    SourceCost::Discard {
                        mode: DiscardCostMode::SelectedOne,
                        tap_source: self.cost_tap_source.ok_or("missing cost tap")?,
                        sacrifice_source: false,
                    }
                }
                _ if self.cost_filter.is_some()
                    || self.cost_count.is_some()
                    || self.cost_tap_source.is_some() =>
                {
                    return Err("unexpected selected cost fields".into());
                }
                "TapSourceAndSubtypeX" => SourceCost::TapPermanents {
                    filter: TapCostFilter::Subtype(creature_types::id(
                        self.cost_subtype.ok_or("missing cost subtype")?,
                        vocabulary,
                    )?),
                    count: TapCostCount::ChosenX,
                    tap_source: true,
                    exclude_source: true,
                },
                "TapPermanentOfSubtype" => SourceCost::TapPermanents {
                    filter: TapCostFilter::Subtype(creature_types::id(
                        self.cost_subtype.ok_or("missing cost subtype")?,
                        vocabulary,
                    )?),
                    count: TapCostCount::Fixed(std::num::NonZeroU8::MIN),
                    tap_source: false,
                    exclude_source: false,
                },
                _ if self.cost_subtype.is_some() => return Err("unexpected cost subtype".into()),
                "TapDiscardHand" => SourceCost::Discard {
                    mode: DiscardCostMode::WholeHand,
                    tap_source: true,
                    sacrifice_source: false,
                },
                "TapExileGraveyardX" => SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::AnyCountX,
                    tap_source: true,
                },
                "ExileSelectedGraveyardCreature" => SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Creature {
                        pay_mana_cost: false,
                    },
                    tap_source: false,
                },
                "ExileSelectedGraveyardCreatureAndPayItsManaCost" => SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Creature {
                        pay_mana_cost: true,
                    },
                    tap_source: false,
                },
                "TapExileSelectedGraveyardCreature" => SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Creature {
                        pay_mana_cost: false,
                    },
                    tap_source: true,
                },
                "TapLegendaryPermanent" => SourceCost::TapPermanents {
                    filter: TapCostFilter::Legendary,
                    count: TapCostCount::Fixed(std::num::NonZeroU8::MIN),
                    tap_source: false,
                    exclude_source: false,
                },
                "None" => SourceCost::None,
                "Tap" => SourceCost::Tap,
                "Sacrifice" => SourceCost::Sacrifice,
                "TapSacrifice" => SourceCost::TapSacrifice,
                "Exile" => SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: false,
                },
                "TapExile" => SourceCost::Exile {
                    tap_source: true,
                    from_graveyard: false,
                },
                "CycleDiscard" => SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                "Ninjutsu" => SourceCost::Ninjutsu {
                    from_command: false,
                },
                "CommanderNinjutsu" => SourceCost::Ninjutsu { from_command: true },
                "Unearth" => SourceCost::GraveyardSelf { unearth: true },
                "GraveyardSelf" => SourceCost::GraveyardSelf { unearth: false },
                "GraveyardExile" => SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                "ReinforceDiscard" => SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce),
                "ChannelDiscard" => SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel),
                label if label.starts_with("PayLifeSacrificeOtherCreature(") => {
                    SourceCost::PayLife {
                        amount: LifeCostAmount::Fixed(
                            label
                                .strip_prefix("PayLifeSacrificeOtherCreature(")
                                .and_then(|s| s.strip_suffix(')'))
                                .and_then(|s| s.parse().ok())
                                .ok_or("invalid life cost")?,
                        ),
                        tap_source: false,
                        sacrifice: Some(SelectedSacrificeFilter::OtherCreature),
                    }
                }
                "TapPayCommanderColors" => SourceCost::PayLife {
                    amount: LifeCostAmount::CommanderColors,
                    tap_source: true,
                    sacrifice: None,
                },
                label if label.starts_with("TapPayLife(") => SourceCost::PayLife {
                    amount: LifeCostAmount::Fixed(
                        label
                            .strip_prefix("TapPayLife(")
                            .and_then(|s| s.strip_suffix(')'))
                            .and_then(|s| s.parse().ok())
                            .ok_or("invalid tap and life cost")?,
                    ),
                    tap_source: true,
                    sacrifice: None,
                },
                label if label.starts_with("PayLife(") => SourceCost::PayLife {
                    amount: LifeCostAmount::Fixed(
                        label
                            .strip_prefix("PayLife(")
                            .and_then(|s| s.strip_suffix(')'))
                            .and_then(|s| s.parse().ok())
                            .ok_or("invalid life cost")?,
                    ),
                    tap_source: false,
                    sacrifice: None,
                },
                "TransmuteDiscard" => SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute),
                "Station" => SourceCost::Power {
                    kind: PowerAbility::Station,
                    minimum: 0,
                },
                label if label.starts_with("Crew(") => SourceCost::Power {
                    kind: PowerAbility::Crew,
                    minimum: label
                        .strip_prefix("Crew(")
                        .and_then(|n| n.strip_suffix(')'))
                        .and_then(|n| n.parse().ok())
                        .ok_or("invalid crew value")?,
                },
                label if label.starts_with("Saddle(") => SourceCost::Power {
                    kind: PowerAbility::Saddle,
                    minimum: label
                        .strip_prefix("Saddle(")
                        .and_then(|n| n.strip_suffix(')'))
                        .and_then(|n| n.parse().ok())
                        .ok_or("invalid saddle value")?,
                },
                _ => return Err("unknown source cost".into()),
            },
            body: self.body.native(vocabulary)?,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum CastFilter {
    ColorlessCreature {},
    InstantOrSorceryFromOwnLibrary {},
    InstantSorceryOrSubtype { subtype: Value },
    InstantOrSorceryAboveExperience {},
    OrdinalThisTurn { ordinal: std::num::NonZeroU16 },
    CreatureOfChosenEntryType {},
    Colorless {},
    AnySpell {},
    Noncreature {},
    InstantOrSorcery {},
    Historic {},
    HasType { card_type: String },
}
impl CastFilter {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<SpellFilter, String> {
        Ok(match self {
            Self::AnySpell {} => SpellFilter::any(),
            Self::Noncreature {} => SpellFilter::noncreature(),
            Self::InstantSorceryOrSubtype { subtype } => {
                SpellFilter::instant_sorcery_or_subtype(creature_types::id(subtype, vocabulary)?)
            }
            Self::ColorlessCreature {} => SpellFilter::creature().with_colorless(),
            Self::Colorless {} => SpellFilter::colorless(),
            Self::InstantOrSorceryAboveExperience {} => {
                SpellFilter::instant_or_sorcery().above_experience()
            }
            Self::OrdinalThisTurn { ordinal } => {
                SpellFilter::any().ordinal_this_turn(std::num::NonZeroU32::from(ordinal))
            }
            Self::CreatureOfChosenEntryType {} => SpellFilter::creature().of_chosen_entry_type(),
            Self::InstantOrSorceryFromOwnLibrary {} => {
                SpellFilter::instant_or_sorcery().from_own_library()
            }
            Self::InstantOrSorcery {} => SpellFilter::instant_or_sorcery(),
            Self::Historic {} => SpellFilter::historic(),
            Self::HasType { card_type: kind } => SpellFilter::of_type(card_type(&kind)?),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Condition {
    MonarchUpkeep { opponent: bool },
    UpkeepWithCityBlessing {},
    CombatWhileControllingOwnCommander {},
    UpkeepOpponentMoreLands {},
    OwnEndPowerAtLeast { minimum: u16 },
    OwnBeginningCombatTotalPowerAtLeast { minimum: u16 },
    EntersOrControllerCastsAtLeast { minimum: u16 },
    ControllerCastsOrCopiesInstantOrSorcery {},
    OpponentAttacksController {},
    EndWithoutLoyaltyActivation {},
    OwnEndWithoutAttacking {},
    EndOpponentLostLife { amount: u8 },
    OpponentGraveyardWithQuestCounters { amount: u8 },
    UpkeepGreatestArtifact {},
    ControllerCastsSpellTargetingOnlySource {},
    GraveyardOwnerCastsPlaneswalker {},
    GraveyardUpkeep {},
    SelfBecomesBlocked {},
    SelfAttacksHighestLife {},
    ControllerAttacksWithTotalPower { minimum: u16 },
    ControlledSubtypeAttacks { subtype: Value },
    ControllerPlacesCounterOnAnotherColorlessCreature {},
    SelfAttacksWithStrongerCreature {},
    UnrenownedSelfDealsCombatDamageToPlayer {},
    SelfBecomesBlockedCaptureDefender {},
    ControlledSubtypeDies { subtype: Value },
    SubtypeDamagedController { subtype: Value },
    ControlledNontokenSubtypeDies { subtype: Value },
    ControlledSubtypeEnters { subtype: Value, other: bool },
    ControlledTokenAttacks { subtype: Value, minimum_power: u16 },
    CountersOnOtherControlledCreatureExceptSubtype { subtype: Value },
    ControlledPermanentTypeDies { kind: String },
    ControlledSubtypeDealsCombatDamageToPlayer { subtype: Value },
    SubtypeDealsCombatDamageToPlayer { subtype: Value },
    AttachedCreatureDealsCombatDamageToPlayer {},
    SelfDealsDamageToOpponent {},
    SelfDealtDamage {},
    DamageToLinkedPlayer {},
    SagaChapter { number: u8 },
    SelfDealsCombatDamageToPlayerOrPlaneswalker {},
    SelfDealsCombatDamageToCreature {},
    ControlledCreatureDealsCombatDamageToPlayer {},
    SelfDealsCombatDamageToPlayerWithMoreLands {},
    SelfDealsCombatDamageToPlayerWithoutOtherCreatures {},
    SelfDealsCombatDamageToPlayer {},
    SelfAttacksUnblocked {},
    SelfAttacks {},
    SelfAttacksOrBlocks {},
    SelfEntersOrAttacks {},
    CreatureCardsLibraryToGraveyard {},
    OwnCreatureCardsPutIntoGraveyardOnOwnTurn {},
    SelfAttacksWhileSaddled {},
    SelfAttacksCaptureDefender {},
    SelfFirstAttack { highest_life: bool },
    SelfAttacksWithAtLeastOthers { minimum: u8 },
    SelfEntersDuringDeclareAttackers {},
    SelfEntersFromGraveyard {},
    CreatureEntersFromControllerGraveyard {},
    SelfEntersAttachedToCreature {},
    AttachedCreatureDealtDamage {},
    AttachedCreatureAttacks {},
    BlitzDies {},
    EnteredIfCast { from_hand: bool },
    EnteredEvoked {},
    EnteredKicked {},
    SelfTurnsFaceUp {},
    SelfMutated,
    SelfPhased { out: bool },
    OwnMeldPairAtStep { at: String },
    Soulbond { entrant_only: bool },
    SelfBlocksOrBecomesBlocked {},
    BlockedByCreatureWithoutFlanking {},
    BlockedByCreature {},
    BlocksOrBlockedByCreature {},
    AttachedCreatureBlocksOrBlockedBySubtype { subtype: Value },
    ControlledCreatureWithPlusOneCounterAttacks {},
    ControlledCreatureAttacks {},
    ControlledCreatureAttacksCaptureDefender {},
    AnotherControlledCreatureAttacks {},
    AttacksYouOrYourPlaneswalker {},
    AttacksAnOpponentOrTheirPlaneswalker {},
    AttackAtLeastTwoAtController {},
    ControlledCommanderAttacks {},
    ControllerAttacksPlayerWithEquippedCreatures {},
    AttacksAlone {},
    Scried { player: String },
    Surveilled { player: String },
    CounterKeyword { keyword: CounterKeywordWire },
    Graft {},
    Increment {},
    Evolve {},
    EchoUpkeep {},
    CumulativeUpkeep {},
    OtherCreaturesUpkeep {},
    TargetedBySpell {},
    AttachedCreatureTargetedBySpell {},
    ControlledCreatureTargetedByInstantOrSorcery {},
    WardTargeted {},
    ControlledSubtypeTargeted { subtype: Value },
    PageCountersAtLeast { amount: u8 },
    ControlledSubtypeThreshold { subtype: Value, minimum: u16 },
    SelfCastBelowHandSize { cards: u8 },
    SelfCast {},
    SelfCycled {},
    Cycled { player: String },
    DrewCard { player: String },
    DrewOwnNthCard { ordinal: std::num::NonZeroU16 },
    OpponentArtifactTappedForMana {},
    SpellCast { player: String, filter: CastFilter },
    TokenCreated { player: String },
    LifeChanged { kind: String, player: String },
    Enters { object: String },
    EntersWhileSourceHasLoyalty { kind: String },
    LeavesBattlefield { object: String },
    SourcePutIntoGraveyardFromBattlefield {},
    CreatureDamagedBySourceDies {},
    LastDefenseRemoved {},
    PlusOneCountersOnSource {},
    Dies { object: String },
    CreaturePowerAtMostToOwnGraveyard { power: i16 },
    BecomesTapped { object: String },
    Sacrificed { player: String },
    SacrificedType { player: String, kind: String },
    ControlledSagaFinalChapterResolved {},
    SacrificedNontoken { player: String },
    Discarded { player: String },
    StepBegan { step: String, player: String },
    ControllerStepWithCreatureCount { step: String, count: u16 },
}
fn battlefield_filter_from_label(
    filter: &str,
) -> Result<spellwake_engine::program::ObjectFilter, String> {
    use spellwake_engine::characteristics::KeywordAbility;
    use spellwake_engine::program::{
        CardTypeMask, ManaValueFilter, ObjectClause, ObjectColorFilter, ObjectFilter, PowerFilter,
    };
    Ok(match filter {
        "Artifact" => ObjectFilter::artifact(),
        "Enchantment" => ObjectFilter::enchantment(),
        "Land" => ObjectFilter::land(),
        "ControlledLand" => ObjectFilter::land().controlled(),
        "OtherControlledPermanent" => ObjectFilter::any().controlled().other_than_trigger_source(),
        "ControlledPlaneswalker" => ObjectFilter::planeswalker().controlled(),
        "OtherControlledPlaneswalker" => ObjectFilter::planeswalker()
            .controlled()
            .other_than_trigger_source(),
        "CreatureOfRelatedPlayer" => ObjectFilter::creature().of_related_player(),
        "OtherControlledCreature" => ObjectFilter::creature()
            .controlled()
            .other_than_ability_source(),
        "FlyingCreature" => ObjectFilter::creature().with_keyword(KeywordAbility::Flying),
        "ArtifactOrLand" => ObjectFilter::artifact_or_land(),
        "ControlledCreature" => ObjectFilter::creature().controlled(),
        "OpposingCreature" => ObjectFilter::creature().opposing(),
        "CreatureNotControlled" => ObjectFilter::creature().not_controlled(),
        "LegendaryCreature" => ObjectFilter::creature().legendary(),
        "NontokenCreature" => ObjectFilter::creature().nontoken(),
        "AttackingCreature" => ObjectFilter::creature().attacking(),
        "AttackingOrBlockingCreature" => ObjectFilter::creature().attacking_or_blocking(),
        "CreatureOrPlaneswalker" => ObjectFilter::creature_or_planeswalker(),
        "LargeCreature" => ObjectFilter::creature().with_power(PowerFilter::AtLeast(4)),
        label if label.starts_with("OtherCreaturePowerAtMost(") => {
            let max: u16 = label
                .strip_prefix("OtherCreaturePowerAtMost(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.parse().ok())
                .ok_or("invalid power maximum")?;
            ObjectFilter::creature()
                .with_power(PowerFilter::AtMost(max))
                .other_than_ability_source()
        }
        "CreaturePowerAtMostControlledCreatureCount" => {
            ObjectFilter::creature().with_power(PowerFilter::AtMostControlledCreatureCount)
        }
        "OtherCreature" => ObjectFilter::creature().other_than_trigger_source(),
        "CreaturePowerAtMostX" => ObjectFilter::creature().with_power(PowerFilter::AtMostX),
        "SmallPowerOrToughnessCreature" => {
            ObjectFilter::creature().with_power(PowerFilter::PowerOrToughnessAtMost(1))
        }
        "CreatureWithoutCounters" => ObjectFilter::creature().without_counters(),
        "ControlledArtifact" => ObjectFilter::artifact().controlled(),
        "ControlledArtifactCreatureOrLand" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        )
        .controlled(),
        "ControlledCreatureOrPlaneswalker" => ObjectFilter::creature_or_planeswalker().controlled(),
        "ControlledArtifactOrCreature" => ObjectFilter::artifact_or_creature().controlled(),
        "ControlledEnchantment" => ObjectFilter::enchantment().controlled(),
        "ControlledPermanent" => ObjectFilter::any().controlled(),
        "OwnedCreature" => ObjectFilter::creature().owned(),
        "Noncreature" => ObjectFilter::noncreature(),
        "Colored" => ObjectFilter::any().with_color(ObjectColorFilter::Colored),
        "ArtifactOrEnchantmentOrLand" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::ENCHANTMENT)
                .union(CardTypeMask::LAND),
        ),
        "OpposingCreatureOrEnchantment" => ObjectFilter::creature_or_enchantment().opposing(),
        "EnchantmentOrTappedArtifactOrCreature" => ObjectFilter::either(
            ObjectClause::enchantment(),
            ObjectClause::any()
                .any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE))
                .tapped(),
        ),
        "TappedCreature" => ObjectFilter::creature().tapped(),
        "Nonland" => ObjectFilter::nonland(),
        "OtherControllersArtifact" => ObjectFilter::artifact().not_controlled(),
        "OtherControllersNonland" => ObjectFilter::nonland().not_controlled(),
        "ControlledCreatureOrEnchantment" => ObjectFilter::creature_or_enchantment().controlled(),
        "PermanentOtherThanAbilitySource" => ObjectFilter::any().other_than_ability_source(),
        "ColorlessCreatureEnteredThisTurn" => ObjectFilter::creature()
            .with_color(ObjectColorFilter::Colorless)
            .entered_this_turn(),
        "CommanderEnteredThisTurn" => ObjectFilter::any().commander().entered_this_turn(),
        "CreatureOtherThanAbilitySource" => ObjectFilter::creature().other_than_ability_source(),
        "ArtifactCreatureEnchantmentOrLand" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT)
                .union(CardTypeMask::LAND),
        ),
        "NonenchantmentNonland" => ObjectFilter::nonland().nonenchantment(),
        "NonSagaNonland" => ObjectFilter::nonland().non_saga(),
        "OpposingEvenManaValue" => ObjectFilter::any()
            .opposing()
            .with_mana_value(ManaValueFilter::Even),
        "OpposingNonland" => ObjectFilter::nonland().opposing(),
        "ArtifactOrEnchantment" => ObjectFilter::artifact_or_enchantment(),
        "ArtifactOrCreature" => ObjectFilter::artifact_or_creature(),
        "ArtifactOrEnchantmentOrFlyingCreature" => ObjectFilter::either(
            ObjectClause::any().any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT)),
            ObjectClause::creature().with_keyword(KeywordAbility::Flying),
        ),
        "ArtifactCreatureOrEnchantment" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT),
        ),
        _ => return Err(format!("unknown BattlefieldTargetFilter {filter}")),
    })
}

fn graveyard_filter_from_label(
    filter: &str,
) -> Result<spellwake_engine::program::ObjectFilter, String> {
    use spellwake_engine::program::{ManaValueFilter, ObjectFilter};
    Ok(match filter {
        "Creature" => ObjectFilter::creature(),
        "Artifact" => ObjectFilter::artifact(),
        "Planeswalker" => ObjectFilter::planeswalker(),
        "LegendaryCreature" => ObjectFilter::creature().legendary(),
        "CreatureOrLand" => ObjectFilter::creature_or_land(),
        label if label.starts_with("CreatureManaValueAtMost(") => {
            let max: u32 = label
                .strip_prefix("CreatureManaValueAtMost(")
                .and_then(|x| x.strip_suffix(')'))
                .and_then(|x| x.parse().ok())
                .ok_or("invalid creature mana-value filter")?;
            ObjectFilter::creature().with_mana_value(ManaValueFilter::AtMost(max))
        }
        label if label.starts_with("PermanentManaValueAtMost(") => {
            let max: u32 = label
                .strip_prefix("PermanentManaValueAtMost(")
                .and_then(|x| x.strip_suffix(')'))
                .and_then(|x| x.parse().ok())
                .ok_or("invalid permanent mana-value filter")?;
            ObjectFilter::permanent().with_mana_value(ManaValueFilter::AtMost(max))
        }
        "ArtifactOrEnchantment" => ObjectFilter::artifact_or_enchantment(),
        "NonAuraEnchantment" => ObjectFilter::enchantment().non_aura(),
        "Instant" => ObjectFilter::instant(),
        "InstantOrSorcery" => ObjectFilter::instant_or_sorcery(),
        "Enchantment" => ObjectFilter::enchantment(),
        "Land" => ObjectFilter::land(),
        "Permanent" => ObjectFilter::permanent(),
        _ => return Err(format!("unknown GraveyardTargetFilter {filter}")),
    })
}

fn battlefield_filter_label(filter: spellwake_engine::program::ObjectFilter) -> String {
    use spellwake_engine::characteristics::KeywordAbility;
    use spellwake_engine::program::{
        CardTypeMask, ManaValueFilter, ObjectClause, ObjectColorFilter, ObjectFilter, PowerFilter,
    };
    if filter == ObjectFilter::artifact() {
        return "Artifact".into();
    }
    if filter == ObjectFilter::enchantment() {
        return "Enchantment".into();
    }
    if filter == ObjectFilter::land() {
        return "Land".into();
    }
    if filter == ObjectFilter::land().controlled() {
        return "ControlledLand".into();
    }
    if filter == ObjectFilter::any().controlled().other_than_trigger_source() {
        return "OtherControlledPermanent".into();
    }
    if filter == ObjectFilter::planeswalker().controlled() {
        return "ControlledPlaneswalker".into();
    }
    if filter
        == ObjectFilter::planeswalker()
            .controlled()
            .other_than_trigger_source()
    {
        return "OtherControlledPlaneswalker".into();
    }
    if filter == ObjectFilter::creature().of_related_player() {
        return "CreatureOfRelatedPlayer".into();
    }
    if filter
        == ObjectFilter::creature()
            .controlled()
            .other_than_ability_source()
    {
        return "OtherControlledCreature".into();
    }
    if filter == ObjectFilter::creature().with_keyword(KeywordAbility::Flying) {
        return "FlyingCreature".into();
    }
    if filter == ObjectFilter::artifact_or_land() {
        return "ArtifactOrLand".into();
    }
    if filter == ObjectFilter::creature().controlled() {
        return "ControlledCreature".into();
    }
    if filter == ObjectFilter::creature().opposing() {
        return "OpposingCreature".into();
    }
    if filter == ObjectFilter::creature().not_controlled() {
        return "CreatureNotControlled".into();
    }
    if filter == ObjectFilter::creature().legendary() {
        return "LegendaryCreature".into();
    }
    if filter == ObjectFilter::creature().nontoken() {
        return "NontokenCreature".into();
    }
    if filter == ObjectFilter::creature().attacking() {
        return "AttackingCreature".into();
    }
    if filter == ObjectFilter::creature().attacking_or_blocking() {
        return "AttackingOrBlockingCreature".into();
    }
    if filter == ObjectFilter::creature_or_planeswalker() {
        return "CreatureOrPlaneswalker".into();
    }
    if filter == ObjectFilter::creature().with_power(PowerFilter::AtLeast(4)) {
        return "LargeCreature".into();
    }
    if let ObjectFilter::Clause(c) = filter
        && c.any_types == CardTypeMask::CREATURE
        && c.source_relation == spellwake_engine::program::SourceRelation::OtherThanAbilitySource
        && let PowerFilter::AtMost(max) = c.power
    {
        return format!("OtherCreaturePowerAtMost({max})");
    }
    if filter == ObjectFilter::creature().with_power(PowerFilter::AtMostControlledCreatureCount) {
        return "CreaturePowerAtMostControlledCreatureCount".into();
    }
    if filter == ObjectFilter::creature().other_than_trigger_source() {
        return "OtherCreature".into();
    }
    if filter == ObjectFilter::creature().with_power(PowerFilter::AtMostX) {
        return "CreaturePowerAtMostX".into();
    }
    if filter == ObjectFilter::creature().with_power(PowerFilter::PowerOrToughnessAtMost(1)) {
        return "SmallPowerOrToughnessCreature".into();
    }
    if filter == ObjectFilter::creature().without_counters() {
        return "CreatureWithoutCounters".into();
    }
    if filter == ObjectFilter::artifact().controlled() {
        return "ControlledArtifact".into();
    }
    if filter
        == ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        )
        .controlled()
    {
        return "ControlledArtifactCreatureOrLand".into();
    }
    if filter == ObjectFilter::creature_or_planeswalker().controlled() {
        return "ControlledCreatureOrPlaneswalker".into();
    }
    if filter == ObjectFilter::artifact_or_creature().controlled() {
        return "ControlledArtifactOrCreature".into();
    }
    if filter == ObjectFilter::enchantment().controlled() {
        return "ControlledEnchantment".into();
    }
    if filter == ObjectFilter::any().controlled() {
        return "ControlledPermanent".into();
    }
    if filter == ObjectFilter::creature().owned() {
        return "OwnedCreature".into();
    }
    if filter == ObjectFilter::noncreature() {
        return "Noncreature".into();
    }
    if filter == ObjectFilter::any().with_color(ObjectColorFilter::Colored) {
        return "Colored".into();
    }
    if filter
        == ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::ENCHANTMENT)
                .union(CardTypeMask::LAND),
        )
    {
        return "ArtifactOrEnchantmentOrLand".into();
    }
    if filter == ObjectFilter::creature_or_enchantment().opposing() {
        return "OpposingCreatureOrEnchantment".into();
    }
    if filter
        == ObjectFilter::either(
            ObjectClause::enchantment(),
            ObjectClause::any()
                .any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE))
                .tapped(),
        )
    {
        return "EnchantmentOrTappedArtifactOrCreature".into();
    }
    if filter == ObjectFilter::creature().tapped() {
        return "TappedCreature".into();
    }
    if filter == ObjectFilter::nonland() {
        return "Nonland".into();
    }
    if filter == ObjectFilter::artifact().not_controlled() {
        return "OtherControllersArtifact".into();
    }
    if filter == ObjectFilter::nonland().not_controlled() {
        return "OtherControllersNonland".into();
    }
    if filter == ObjectFilter::creature_or_enchantment().controlled() {
        return "ControlledCreatureOrEnchantment".into();
    }
    if filter == ObjectFilter::any().other_than_ability_source() {
        return "PermanentOtherThanAbilitySource".into();
    }
    if filter
        == ObjectFilter::creature()
            .with_color(ObjectColorFilter::Colorless)
            .entered_this_turn()
    {
        return "ColorlessCreatureEnteredThisTurn".into();
    }
    if filter == ObjectFilter::any().commander().entered_this_turn() {
        return "CommanderEnteredThisTurn".into();
    }
    if filter == ObjectFilter::creature().other_than_ability_source() {
        return "CreatureOtherThanAbilitySource".into();
    }
    if filter
        == ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT)
                .union(CardTypeMask::LAND),
        )
    {
        return "ArtifactCreatureEnchantmentOrLand".into();
    }
    if filter == ObjectFilter::nonland().nonenchantment() {
        return "NonenchantmentNonland".into();
    }
    if filter == ObjectFilter::nonland().non_saga() {
        return "NonSagaNonland".into();
    }
    if filter
        == ObjectFilter::any()
            .opposing()
            .with_mana_value(ManaValueFilter::Even)
    {
        return "OpposingEvenManaValue".into();
    }
    if filter == ObjectFilter::nonland().opposing() {
        return "OpposingNonland".into();
    }
    if filter == ObjectFilter::artifact_or_enchantment() {
        return "ArtifactOrEnchantment".into();
    }
    if filter == ObjectFilter::artifact_or_creature() {
        return "ArtifactOrCreature".into();
    }
    if filter
        == ObjectFilter::either(
            ObjectClause::any().any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT)),
            ObjectClause::creature().with_keyword(KeywordAbility::Flying),
        )
    {
        return "ArtifactOrEnchantmentOrFlyingCreature".into();
    }
    if filter
        == ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT),
        )
    {
        return "ArtifactCreatureOrEnchantment".into();
    }
    format!("{filter:?}")
}

fn graveyard_filter_label(filter: spellwake_engine::program::ObjectFilter) -> String {
    use spellwake_engine::program::{CardTypeMask, ManaValueFilter, ObjectFilter};
    if filter == ObjectFilter::creature() {
        return "Creature".into();
    }
    if filter == ObjectFilter::artifact() {
        return "Artifact".into();
    }
    if filter == ObjectFilter::planeswalker() {
        return "Planeswalker".into();
    }
    if filter == ObjectFilter::creature().legendary() {
        return "LegendaryCreature".into();
    }
    if filter == ObjectFilter::creature_or_land() {
        return "CreatureOrLand".into();
    }
    if let ObjectFilter::Clause(c) = filter
        && let ManaValueFilter::AtMost(max) = c.mana_value
    {
        if c.any_types == CardTypeMask::CREATURE {
            return format!("CreatureManaValueAtMost({max})");
        }
        if c.any_types == CardTypeMask::PERMANENT {
            return format!("PermanentManaValueAtMost({max})");
        }
    }
    if filter == ObjectFilter::artifact_or_enchantment() {
        return "ArtifactOrEnchantment".into();
    }
    if filter == ObjectFilter::enchantment().non_aura() {
        return "NonAuraEnchantment".into();
    }
    if filter == ObjectFilter::instant() {
        return "Instant".into();
    }
    if filter == ObjectFilter::instant_or_sorcery() {
        return "InstantOrSorcery".into();
    }
    if filter == ObjectFilter::enchantment() {
        return "Enchantment".into();
    }
    if filter == ObjectFilter::land() {
        return "Land".into();
    }
    if filter == ObjectFilter::permanent() {
        return "Permanent".into();
    }
    format!("{filter:?}")
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum GroupWire {
    Named(String),
    CreatureSubtype {
        creature_subtype: Value,
    },
    WithoutSubtype {
        creatures_without_subtype: Value,
    },
    ExceptChosen {
        except_chosen_type: Value,
    },
    ControlledSubtype {
        controlled_permanent_subtype: Value,
    },
    ControlledCreatureSubtype {
        controlled_creature_subtype: Value,
    },
    OtherControlledCreatureSubtype {
        other_controlled_creature_subtype: Value,
    },
    WithSubtype {
        permanent_subtype: Value,
    },
    NonlandsAtMost {
        nonlands_at_most_mana_value: u16,
    },
}
fn permanent_group_wire(group: spellwake_engine::program::ObjectFilter) -> Value {
    use spellwake_engine::program::{
        CardTypeMask, ManaValueFilter, ObjectFilter, PowerFilter, SubtypeFilter,
    };
    if let ObjectFilter::Clause(c) = group {
        if let SubtypeFilter::Lacks(kind) = c.subtypes
            && c.any_types == CardTypeMask::CREATURE
        {
            return json!({"creatures_without_subtype": projection::creature_type_id(kind)});
        }
        if let SubtypeFilter::Has(kind) = c.subtypes
            && c.any_types.is_empty()
        {
            return json!({"permanent_subtype": projection::creature_type_id(kind)});
        }
        if let SubtypeFilter::ExceptChosenSlot(slot) = c.subtypes {
            return json!({"except_chosen_type": slot.ordinal().to_string()});
        }
        if matches!(c.subtypes, SubtypeFilter::OutsideNamedSlots) {
            return json!("CreaturesOutsideNamedTypes");
        }
        if let ManaValueFilter::AtMost(limit) = c.mana_value
            && c.forbidden_types == CardTypeMask::LAND
        {
            return json!({"nonlands_at_most_mana_value": limit});
        }
    }
    if group == ObjectFilter::creature() {
        return json!("Creatures");
    }
    if group == ObjectFilter::artifact().not_controlled() {
        return json!("OtherControllersArtifacts");
    }
    if group == ObjectFilter::nonland().not_controlled() {
        return json!("OtherControllersNonlands");
    }
    if group == ObjectFilter::enchantment() {
        return json!("Enchantments");
    }
    if group == ObjectFilter::artifact_or_enchantment() {
        return json!("ArtifactsAndEnchantments");
    }
    if group
        == ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT),
        )
    {
        return json!("ArtifactsCreaturesAndEnchantments");
    }
    if group == ObjectFilter::nonland().nonlegendary() {
        return json!("NonlegendaryNonlandPermanents");
    }
    if group == ObjectFilter::nonland() {
        return json!("NonlandPermanents");
    }
    if group == ObjectFilter::creature().nonenchantment() {
        return json!("NonenchantmentCreatures");
    }
    if group == ObjectFilter::creature().with_power(PowerFilter::AtLeast(4)) {
        return json!("CreaturesWithPowerFourOrGreater");
    }
    json!(format!("{group:?}"))
}
fn effect_group_wire(group: spellwake_engine::program::EffectSubject) -> Value {
    use spellwake_engine::program::{
        CardTypeMask, ControllerFilter, EffectSubject, ObjectFilter, PowerFilter, SourceRelation,
        SubtypeFilter,
    };
    match group {
        EffectSubject::TriggeringAttacker => json!("TriggeringAttacker"),
        EffectSubject::TriggeringTarget => json!("TriggeringTarget"),
        EffectSubject::TriggerSubject => json!("TriggerSubject"),
        EffectSubject::TriggeringAttackGroup => json!("TriggeringAttackGroup"),
        EffectSubject::AbilitySource => json!("AbilitySource"),
        EffectSubject::TargetObject => json!("TargetObject"),
        EffectSubject::Matching(filter) => {
            if let ObjectFilter::Clause(c) = filter {
                if let SubtypeFilter::ExceptChosenSlot(slot) = c.subtypes {
                    return json!({"except_chosen_type": slot.ordinal().to_string()});
                }
                if let SubtypeFilter::Has(kind) = c.subtypes {
                    if c.any_types == CardTypeMask::CREATURE {
                        return match (c.controller, c.source_relation) {
                            (ControllerFilter::You, SourceRelation::OtherThanAbilitySource) => {
                                json!({"other_controlled_creature_subtype": projection::creature_type_id(kind)})
                            }
                            (ControllerFilter::You, _) => {
                                json!({"controlled_creature_subtype": projection::creature_type_id(kind)})
                            }
                            _ => json!({"creature_subtype": projection::creature_type_id(kind)}),
                        };
                    }
                    if c.controller == ControllerFilter::You {
                        return json!({"controlled_permanent_subtype": projection::creature_type_id(kind)});
                    }
                }
            }
            if filter
                == ObjectFilter::creature()
                    .controlled()
                    .with_power(PowerFilter::AtLeast(4))
            {
                return json!("ControlledCreaturesWithPowerFourOrGreater");
            }
            if filter == ObjectFilter::creature() {
                return json!("AllCreatures");
            }
            if filter
                == ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source()
            {
                return json!("OtherControlledCreatures");
            }
            if filter == ObjectFilter::creature().controlled() {
                return json!("ControlledCreatures");
            }
            if filter == ObjectFilter::creature().opposing() {
                return json!("OpposingCreatures");
            }
            if filter == ObjectFilter::creature().controlled().attacking() {
                return json!("ControlledAttackingCreatures");
            }
            if filter == ObjectFilter::creature().attacking() {
                return json!("AttackingCreatures");
            }
            if filter == ObjectFilter::any().controlled() {
                return json!("ControlledPermanents");
            }
            json!(format!("{filter:?}"))
        }
        EffectSubject::CreatedTokens => json!("CreatedTokens"),
        EffectSubject::LinkedFirst => json!({"kind":"LinkedFirst"}),
        EffectSubject::LastMoved => json!("LastMoved"),
        EffectSubject::TargetAndSameNameMatching(filter) => {
            json!({"target_and_same_name_matching": permanent_group_wire(filter)})
        }
        EffectSubject::MatchingOtherThanTarget(filter) => {
            json!({"matching_other_than_target": permanent_group_wire(filter)})
        }
        EffectSubject::TargetAndSharingColorMatching(filter) => {
            json!({"target_and_sharing_color_matching": permanent_group_wire(filter)})
        }
    }
}
fn permanent_group(
    group: &GroupWire,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<spellwake_engine::program::ObjectFilter, String> {
    use spellwake_engine::program::{CardTypeMask, ManaValueFilter, ObjectFilter, PowerFilter};
    if let GroupWire::NonlandsAtMost {
        nonlands_at_most_mana_value,
    } = group
    {
        return Ok(
            ObjectFilter::nonland().with_mana_value(ManaValueFilter::AtMost(u32::from(
                *nonlands_at_most_mana_value,
            ))),
        );
    }
    if let GroupWire::WithoutSubtype {
        creatures_without_subtype,
    } = group
    {
        return Ok(ObjectFilter::creature().without_subtype(creature_types::id(
            creatures_without_subtype.clone(),
            vocabulary,
        )?));
    }
    if let GroupWire::WithSubtype { permanent_subtype } = group {
        return Ok(ObjectFilter::any()
            .with_subtype(creature_types::id(permanent_subtype.clone(), vocabulary)?));
    }
    let GroupWire::Named(group) = group else {
        let GroupWire::ExceptChosen { except_chosen_type } = group else {
            return Err("invalid permanent group".into());
        };
        return Ok(ObjectFilter::creature()
            .except_chosen_slot(creature_types::slot(except_chosen_type.clone())?));
    };
    Ok(match group.as_str() {
        "Creatures" => ObjectFilter::creature(),
        "OtherControllersArtifacts" => ObjectFilter::artifact().not_controlled(),
        "OtherControllersNonlands" => ObjectFilter::nonland().not_controlled(),
        "Enchantments" => ObjectFilter::enchantment(),
        "ArtifactsAndEnchantments" => ObjectFilter::artifact_or_enchantment(),
        "ArtifactsCreaturesAndEnchantments" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::ENCHANTMENT),
        ),
        "NonlegendaryNonlandPermanents" => ObjectFilter::nonland().nonlegendary(),
        "NonlandPermanents" => ObjectFilter::nonland(),
        "NonenchantmentCreatures" => ObjectFilter::creature().nonenchantment(),
        "CreaturesWithPowerFourOrGreater" => {
            ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))
        }
        _ => return Err(format!("unknown destruction group {group}")),
    })
}
fn object_relation(value: &str) -> Result<ObjectFilter, String> {
    match value {
        "AnotherNontokenCreature" => Ok(ObjectFilter::creature()
            .nontoken()
            .other_than_ability_source()),
        "ControlledNontokenCreatureWithPlusOneCounter" => Ok(ObjectFilter::creature()
            .controlled()
            .nontoken()
            .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)),
        "ControlledLegendaryCreature" => Ok(ObjectFilter::creature().controlled().legendary()),
        "SelfOrControlledEquipment" => Ok(ObjectFilter::any()
            .controlled()
            .with_artifact_subtype(ArtifactSubtype::Equipment)
            .or_self()),
        "ControlledEquipment" => Ok(ObjectFilter::any()
            .controlled()
            .with_artifact_subtype(ArtifactSubtype::Equipment)),
        "ControlledToken" => Ok(ObjectFilter::any().controlled().token()),
        "OpposingNontokenCreature" => Ok(ObjectFilter::creature().opposing().nontoken()),
        "OpposingCreature" => Ok(ObjectFilter::creature().opposing()),
        "AnotherControlledNontokenCreature" => Ok(ObjectFilter::creature()
            .controlled()
            .nontoken()
            .other_than_ability_source()),
        "SelfOrControlledNontokenCreature" => {
            Ok(ObjectFilter::creature().controlled().nontoken().or_self())
        }
        "ControlledNontokenCreature" => Ok(ObjectFilter::creature().controlled().nontoken()),
        v if v.starts_with("SelfOrControlledType(") => Ok(ObjectFilter::any_types(
            CardTypeMask::from_card_type(card_type(
                v.strip_prefix("SelfOrControlledType(")
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or("invalid controlled type")?,
            )?),
        )
        .controlled()
        .or_self()),
        v if v.starts_with("ControlledCreatureOfType(") => Ok(ObjectFilter::creature()
            .with_required_type(card_type(
                v.strip_prefix("ControlledCreatureOfType(")
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or("invalid controlled creature type")?,
            )?)
            .controlled()),
        v if v.starts_with("ControlledType(") => Ok(ObjectFilter::any_types(
            CardTypeMask::from_card_type(card_type(
                v.strip_prefix("ControlledType(")
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or("invalid controlled type")?,
            )?),
        )
        .controlled()),
        "AttachedPermanent" => Ok(ObjectFilter::any().attached_to_self()),
        "SelfObject" => Ok(ObjectFilter::self_object()),
        "AnyCreature" => Ok(ObjectFilter::creature()),
        "AnotherCreature" => Ok(ObjectFilter::creature().other_than_ability_source()),
        "ControlledCreature" => Ok(ObjectFilter::creature().controlled()),
        "AnotherControlledCreature" => Ok(ObjectFilter::creature()
            .controlled()
            .other_than_ability_source()),
        value if value.starts_with("AnotherControlledCreatureAtMostPower:") => value
            ["AnotherControlledCreatureAtMostPower:".len()..]
            .parse()
            .map(|max| {
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source()
                    .with_power(PowerFilter::AtMost(max))
            })
            .map_err(|_| "invalid entry power bound".to_string()),
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
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TriggerSpec, String> {
        Ok(match self {
            Self::SelfBecomesBlocked {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BecomesBlocked {
                    capture_defender: false,
                },
            ),
            Self::SelfAttacksHighestLife {} => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::HighestLifeDefender,
            ),
            Self::ControlledSubtypeAttacks { subtype } => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::GroupSubtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::ControllerAttacksWithTotalPower { minimum } => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::GroupTotalPowerAtLeast(minimum),
            ),
            Self::ControllerPlacesCounterOnAnotherColorlessCreature {} => {
                TriggerSpec::counters_changed(
                    ObjectFilter::creature()
                        .with_color(ObjectColorFilter::Colorless)
                        .other_than_ability_source(),
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: true,
                    },
                )
            }
            Self::SelfAttacksWithStrongerCreature {} => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::WithStrongerCreature,
            ),
            Self::UnrenownedSelfDealsCombatDamageToPlayer {} => {
                TriggerSpec::self_deals_combat_damage_to_player()
                    .only_if(TriggerCondition::UnrenownedSource)
            }
            Self::SelfBecomesBlockedCaptureDefender {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BecomesBlocked {
                    capture_defender: true,
                },
            ),
            Self::ControlledSubtypeEnters { subtype, other } => {
                let id = creature_types::id(subtype, vocabulary)?;
                let filter = ObjectFilter::any().controlled().with_subtype(id);
                TriggerSpec::enters(if other {
                    filter.other_than_ability_source()
                } else {
                    filter
                })
            }
            Self::ControlledNontokenSubtypeDies { subtype } => TriggerSpec::dies(
                ObjectFilter::creature()
                    .controlled()
                    .nontoken()
                    .with_subtype(creature_types::id(subtype, vocabulary)?),
            ),
            Self::ControlledTokenAttacks {
                subtype,
                minimum_power,
            } => TriggerSpec::attacks(
                ObjectFilter::creature()
                    .controlled()
                    .token()
                    .with_subtype(creature_types::id(subtype, vocabulary)?)
                    .with_power(PowerFilter::AtLeast(minimum_power as i16)),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::SubtypeDamagedController { subtype } => TriggerSpec::deals_damage(
                ObjectFilter::any().with_subtype(creature_types::id(subtype, vocabulary)?),
                DamageTriggerRecipient::Controller,
                false,
            ),
            Self::ControlledSubtypeDies { subtype } => TriggerSpec::new(TriggerEvent::Dies {
                subject: ObjectFilter::any()
                    .controlled()
                    .with_subtype(creature_types::id(subtype, vocabulary)?),
                mode: DiesTriggerMode::Permanent,
            }),
            Self::CountersOnOtherControlledCreatureExceptSubtype { subtype } => {
                TriggerSpec::counters_changed(
                    ObjectFilter::creature()
                        .controlled()
                        .other_than_ability_source()
                        .without_subtype(creature_types::id(subtype, vocabulary)?),
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: false,
                    },
                )
            }
            Self::ControlledPermanentTypeDies { kind } => TriggerSpec::new(TriggerEvent::Dies {
                subject: ObjectFilter::any_types(CardTypeMask::from_card_type(card_type(&kind)?))
                    .controlled(),
                mode: DiesTriggerMode::Permanent,
            }),
            Self::ControlledSubtypeDealsCombatDamageToPlayer { subtype } => {
                TriggerSpec::deals_damage(
                    ObjectFilter::any()
                        .controlled()
                        .with_subtype(creature_types::id(subtype, vocabulary)?),
                    DamageTriggerRecipient::AnyPlayer,
                    true,
                )
            }
            Self::SubtypeDealsCombatDamageToPlayer { subtype } => TriggerSpec::deals_damage(
                ObjectFilter::any().with_subtype(creature_types::id(subtype, vocabulary)?),
                DamageTriggerRecipient::AnyPlayer,
                true,
            ),
            Self::AttachedCreatureDealsCombatDamageToPlayer {} => TriggerSpec::deals_damage(
                ObjectFilter::any().attached_to_self(),
                DamageTriggerRecipient::AnyPlayer,
                true,
            ),
            Self::SelfDealsDamageToOpponent {} => TriggerSpec::deals_damage(
                ObjectFilter::self_object(),
                DamageTriggerRecipient::Opponent,
                false,
            ),
            Self::DamageToLinkedPlayer {} => TriggerSpec::new(TriggerEvent::DealtDamage {
                subject: ObjectFilter::any(),
                linked_player: true,
            }),
            Self::SelfDealtDamage {} => TriggerSpec::new(TriggerEvent::DealtDamage {
                subject: ObjectFilter::self_object(),
                linked_player: false,
            }),
            Self::SagaChapter { number } => TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::SagaChapter(number),
            ),
            Self::SelfDealsCombatDamageToCreature {} => TriggerSpec::deals_damage(
                ObjectFilter::self_object(),
                DamageTriggerRecipient::Creature,
                true,
            ),
            Self::SelfDealsCombatDamageToPlayerOrPlaneswalker {} => TriggerSpec::deals_damage(
                ObjectFilter::self_object(),
                DamageTriggerRecipient::PlayerOrPlaneswalker,
                true,
            ),
            Self::ControlledCreatureDealsCombatDamageToPlayer {} => TriggerSpec::deals_damage(
                ObjectFilter::creature().controlled(),
                DamageTriggerRecipient::AnyPlayer,
                true,
            ),
            Self::SelfDealsCombatDamageToPlayerWithoutOtherCreatures {} => {
                TriggerSpec::self_deals_combat_damage_to_player()
                    .only_if(TriggerCondition::NoOtherControlledCreatures)
            }
            Self::SelfDealsCombatDamageToPlayerWithMoreLands {} => {
                TriggerSpec::self_deals_combat_damage_to_player()
                    .only_if(TriggerCondition::DefendingPlayerHasMoreLands)
            }
            Self::SelfDealsCombatDamageToPlayer {} => {
                TriggerSpec::self_deals_combat_damage_to_player()
            }
            Self::SelfAttacksUnblocked {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::Unblocked,
            ),
            Self::BlitzDies {} => TriggerSpec::new(TriggerEvent::Dies {
                subject: ObjectFilter::self_object(),
                mode: DiesTriggerMode::Blitz,
            }),
            Self::EnteredIfCast { from_hand } => TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Any,
                cast_origin: Some(CastEntryKind::Cast { from_hand }),
                attached_to_creature: false,
            }),
            Self::EnteredKicked {} => TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Any,
                cast_origin: Some(CastEntryKind::Kicked),
                attached_to_creature: false,
            }),
            Self::EnteredEvoked {} => TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Any,
                cast_origin: Some(CastEntryKind::Evoked),
                attached_to_creature: false,
            }),
            Self::SelfTurnsFaceUp {} => TriggerSpec::status_changed(
                ObjectFilter::self_object(),
                StatusTriggerKind::TurnsFaceUp,
            ),
            Self::OwnMeldPairAtStep { at } => {
                TriggerSpec::step_began(step(&at)?, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControlsMeldPair)
            }
            Self::Soulbond { entrant_only } => {
                TriggerSpec::keyword(TriggerKeywordEvent::Soulbond { entrant_only })
            }
            Self::SelfMutated => {
                TriggerSpec::status_changed(ObjectFilter::self_object(), StatusTriggerKind::Mutated)
            }
            Self::SelfPhased { out } => TriggerSpec::status_changed(
                ObjectFilter::self_object(),
                StatusTriggerKind::Phased { out },
            ),
            Self::AttachedCreatureDealtDamage {} => TriggerSpec::new(TriggerEvent::DealtDamage {
                subject: ObjectFilter::any().attached_to_self(),
                linked_player: false,
            }),
            Self::AttachedCreatureAttacks {} => TriggerSpec::attacks(
                ObjectFilter::any().attached_to_self(),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::SelfEntersAttachedToCreature {} => TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Any,
                cast_origin: None,
                attached_to_creature: true,
            }),
            Self::CreatureEntersFromControllerGraveyard {} => {
                TriggerSpec::new(TriggerEvent::Enters {
                    subject: ObjectFilter::creature(),
                    from_zone: TriggerFromZone::ControllerGraveyard,
                    cast_origin: None,
                    attached_to_creature: false,
                })
            }
            Self::SelfEntersFromGraveyard {} => TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Graveyard,
                cast_origin: None,
                attached_to_creature: false,
            }),
            Self::SelfEntersDuringDeclareAttackers {} => TriggerSpec::self_enters()
                .only_if(TriggerCondition::DuringStep(Step::DeclareAttackers)),
            Self::SelfFirstAttack { highest_life } => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::FirstThisTurn { highest_life },
            ),
            Self::SelfAttacksWithAtLeastOthers { minimum } => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::WithAtLeastOthers(minimum),
            ),
            Self::SelfAttacksCaptureDefender {} => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::Individual {
                    capture_defender: true,
                },
            ),
            Self::SelfAttacksWhileSaddled {} => {
                TriggerSpec::self_attacks().only_if(TriggerCondition::SourceSaddled)
            }
            Self::SelfAttacksOrBlocks {} => TriggerSpec::any_of(vec![
                TriggerEvent::Attacks {
                    subject: ObjectFilter::self_object(),
                    mode: AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                },
                TriggerEvent::BlocksOrBlocked {
                    subject: ObjectFilter::self_object(),
                    kind: BlockTriggerKind::Blocks,
                },
            ]),
            Self::SelfAttacks {} => TriggerSpec::self_attacks(),
            Self::SelfEntersOrAttacks {} => TriggerSpec::any_of(vec![
                TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: None,
                    attached_to_creature: false,
                },
                TriggerEvent::Attacks {
                    subject: ObjectFilter::self_object(),
                    mode: AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                },
            ]),
            Self::CreatureCardsLibraryToGraveyard {} => {
                TriggerSpec::new(TriggerEvent::PutIntoGraveyard {
                    subject: ObjectFilter::creature(),
                    player: PlayerRelation::Controller,
                    from_library: true,
                    own_turn_only: false,
                })
            }
            Self::OwnCreatureCardsPutIntoGraveyardOnOwnTurn {} => {
                TriggerSpec::new(TriggerEvent::PutIntoGraveyard {
                    subject: ObjectFilter::creature(),
                    player: PlayerRelation::Controller,
                    from_library: false,
                    own_turn_only: true,
                })
            }
            Self::SelfBlocksOrBecomesBlocked {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlocksOrBecomesBlocked,
            ),
            Self::BlockedByCreature {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlockedByEach {
                    without_flanking_only: false,
                },
            ),
            Self::AttachedCreatureBlocksOrBlockedBySubtype { subtype } => {
                TriggerSpec::blocks_or_blocked(
                    ObjectFilter::any().attached_to_self(),
                    BlockTriggerKind::BlocksOrBlockedByEach {
                        required_subtype: Some(creature_types::id(subtype, vocabulary)?),
                    },
                )
            }
            Self::BlocksOrBlockedByCreature {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlocksOrBlockedByEach {
                    required_subtype: None,
                },
            ),
            Self::BlockedByCreatureWithoutFlanking {} => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlockedByEach {
                    without_flanking_only: true,
                },
            ),
            Self::ControlledCreatureAttacksCaptureDefender {} => TriggerSpec::attacks(
                ObjectFilter::creature().controlled(),
                AttackTriggerMode::Individual {
                    capture_defender: true,
                },
            ),
            Self::ControlledCreatureAttacks {} => TriggerSpec::attacks(
                ObjectFilter::creature().controlled(),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::ControlledCreatureWithPlusOneCounterAttacks {} => TriggerSpec::attacks(
                ObjectFilter::creature()
                    .controlled()
                    .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::AnotherControlledCreatureAttacks {} => TriggerSpec::attacks(
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::AttacksYouOrYourPlaneswalker {} => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::AtControllerOrPlaneswalker,
            ),
            Self::AttacksAnOpponentOrTheirPlaneswalker {} => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::AtOpponentOrPlaneswalker,
            ),
            Self::AttackAtLeastTwoAtController {} => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::OpponentsAtController { minimum: 2 },
            ),
            Self::ControllerAttacksPlayerWithEquippedCreatures {} => {
                TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::EquippedAtPlayer)
            }
            Self::ControlledCommanderAttacks {} => TriggerSpec::attacks(
                ObjectFilter::any().controlled().commander(),
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            Self::AttacksAlone {} => {
                TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::Alone)
            }
            Self::SelfCycled {} => TriggerSpec::player_action(
                PlayerRelation::Controller,
                PlayerTriggerAction::Cycled { self_only: true },
            )
            .in_zone(TriggerZone::Any),
            Self::Cycled { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Cycled { self_only: false },
            ),
            Self::DrewOwnNthCard { ordinal } => TriggerSpec::player_action(
                PlayerRelation::Controller,
                PlayerTriggerAction::DrewCard {
                    nth_each_turn: Some(ordinal),
                },
            ),
            Self::DrewCard { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::DrewCard {
                    nth_each_turn: None,
                },
            ),
            Self::OpponentArtifactTappedForMana {} => TriggerSpec::player_action(
                PlayerRelation::Opponent,
                PlayerTriggerAction::ArtifactTappedForMana,
            ),
            Self::CounterKeyword { keyword } => TriggerSpec::counter_keyword(keyword.native()),
            Self::Graft {} => TriggerSpec::keyword(TriggerKeywordEvent::Graft),
            Self::Increment {} => TriggerSpec::keyword(TriggerKeywordEvent::Increment),
            Self::Evolve {} => TriggerSpec::keyword(TriggerKeywordEvent::Evolve),
            Self::EchoUpkeep {} => {
                TriggerSpec::upkeep_keyword(spellwake_engine::program::UpkeepKeyword::Echo)
            }
            Self::OtherCreaturesUpkeep {} => TriggerSpec::upkeep_keyword(
                spellwake_engine::program::UpkeepKeyword::OtherCreaturesUpkeep,
            ),
            Self::CumulativeUpkeep {} => TriggerSpec::upkeep_keyword(
                spellwake_engine::program::UpkeepKeyword::CumulativeUpkeep,
            ),
            Self::ControlledCreatureTargetedByInstantOrSorcery {} => {
                TriggerSpec::new(TriggerEvent::Targeted {
                    subject: ObjectFilter::creature().controlled(),
                    by_opponent: false,
                    spell_only: true,
                    instant_or_sorcery_only: true,
                })
            }
            Self::AttachedCreatureTargetedBySpell {} => TriggerSpec::new(TriggerEvent::Targeted {
                subject: ObjectFilter::any().attached_to_self(),
                by_opponent: false,
                spell_only: true,
                instant_or_sorcery_only: false,
            }),
            Self::TargetedBySpell {} => TriggerSpec::new(TriggerEvent::Targeted {
                subject: ObjectFilter::self_object(),
                by_opponent: false,
                spell_only: true,
                instant_or_sorcery_only: false,
            }),
            Self::WardTargeted {} => TriggerSpec::new(TriggerEvent::Targeted {
                subject: ObjectFilter::self_object(),
                by_opponent: true,
                spell_only: false,
                instant_or_sorcery_only: false,
            }),
            Self::ControlledSubtypeTargeted { subtype } => {
                TriggerSpec::new(TriggerEvent::Targeted {
                    subject: ObjectFilter::creature()
                        .controlled()
                        .with_subtype(creature_types::id(subtype, vocabulary)?),
                    by_opponent: true,
                    spell_only: false,
                    instant_or_sorcery_only: false,
                })
            }
            Self::ControlledSubtypeThreshold { subtype, minimum } => {
                TriggerSpec::state_threshold(StateThresholdKind::ControlledSubtype {
                    subtype: creature_types::id(subtype, vocabulary)?,
                    minimum,
                })
            }
            Self::PageCountersAtLeast { amount } => {
                TriggerSpec::state_threshold(StateThresholdKind::SourceCounters {
                    kind: spellwake_engine::effects::CounterKind::Page,
                    comparison: Comparison::GreaterOrEqual,
                    amount,
                })
            }
            Self::SelfCastBelowHandSize { cards } => {
                TriggerSpec::self_cast().only_if(TriggerCondition::HandSizeBelow(cards))
            }
            Self::SelfCast {} => TriggerSpec::self_cast(),
            Self::SpellCast { player, filter } => {
                TriggerSpec::spell_cast(player_relation(&player)?, filter.native(vocabulary)?)
            }
            Self::TokenCreated { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::TokenCreated,
            ),
            Self::LifeChanged { kind, player } => TriggerSpec::life_changed(
                match kind.as_str() {
                    "Gain" => LifeChangeKind::Gain,
                    "Loss" => LifeChangeKind::Loss,
                    _ => return Err(format!("unknown life change kind {kind}")),
                },
                player_relation(&player)?,
            ),
            Self::EntersWhileSourceHasLoyalty { kind } => TriggerSpec::enters(
                ObjectFilter::any_types(CardTypeMask::from_card_type(card_type(&kind)?))
                    .controlled(),
            )
            .only_if(TriggerCondition::SourceHasLoyalty),
            Self::Enters { object } => TriggerSpec::enters(object_relation(&object)?),
            Self::LeavesBattlefield { object } => {
                TriggerSpec::leaves_battlefield(object_relation(&object)?)
            }
            Self::LastDefenseRemoved {} => TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::LastDefenseRemoved,
            ),
            Self::CreatureDamagedBySourceDies {} => TriggerSpec::new(TriggerEvent::Dies {
                subject: ObjectFilter::creature(),
                mode: DiesTriggerMode::DamagedBySource,
            }),
            Self::SourcePutIntoGraveyardFromBattlefield {} => {
                TriggerSpec::new(TriggerEvent::Dies {
                    subject: ObjectFilter::self_object(),
                    mode: DiesTriggerMode::Permanent,
                })
            }
            Self::CreaturePowerAtMostToOwnGraveyard { power } => {
                TriggerSpec::new(TriggerEvent::Dies {
                    subject: ObjectFilter::creature().with_power(PowerFilter::AtMost(power as u16)),
                    mode: DiesTriggerMode::ToControllerGraveyard,
                })
            }
            Self::PlusOneCountersOnSource {} => TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::PlusOnePlaced {
                    by_controller_only: false,
                },
            ),
            Self::Dies { object } => TriggerSpec::dies(object_relation(&object)?),
            Self::BecomesTapped { object } => {
                TriggerSpec::becomes_tapped(object_relation(&object)?)
            }
            Self::Scried { player } => {
                TriggerSpec::player_action(player_relation(&player)?, PlayerTriggerAction::Scried)
            }
            Self::Surveilled { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Surveilled,
            ),
            Self::SacrificedNontoken { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Sacrificed(ObjectFilter::any().nontoken()),
            ),
            Self::SacrificedType { player, kind } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Sacrificed(ObjectFilter::any_types(
                    CardTypeMask::from_card_type(card_type(&kind)?),
                )),
            ),
            Self::ControlledSagaFinalChapterResolved {} => TriggerSpec::player_action(
                PlayerRelation::Controller,
                PlayerTriggerAction::SagaFinalChapterResolved,
            ),
            Self::Sacrificed { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Sacrificed(ObjectFilter::any()),
            ),
            Self::Discarded { player } => TriggerSpec::player_action(
                player_relation(&player)?,
                PlayerTriggerAction::Discarded,
            ),
            Self::UpkeepWithCityBlessing {} => {
                TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControllerHasCityBlessing)
            }
            Self::MonarchUpkeep { opponent } => {
                TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                    .only_if(TriggerCondition::Monarch { opponent })
            }
            Self::CombatWhileControllingOwnCommander {} => {
                TriggerSpec::step_began(Step::BeginningCombat, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControlsOwnCommander)
            }
            Self::OpponentAttacksController {} => TriggerSpec::attacks(
                ObjectFilter::any(),
                AttackTriggerMode::OpponentsAtController { minimum: 1 },
            ),
            Self::OwnBeginningCombatTotalPowerAtLeast { minimum } => {
                TriggerSpec::step_began(Step::BeginningCombat, PlayerRelation::Controller)
                    .only_if(TriggerCondition::TotalControlledPowerAtLeast(minimum))
            }
            Self::ControllerCastsOrCopiesInstantOrSorcery {} => {
                TriggerSpec::new(TriggerEvent::SpellCast {
                    caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                    filter: SpellFilter::instant_or_sorcery(),
                    self_cast: false,
                    include_copy: true,
                    min_mana_value: None,
                })
            }
            Self::EntersOrControllerCastsAtLeast { minimum } => TriggerSpec::any_of(vec![
                TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: None,
                    attached_to_creature: false,
                },
                TriggerEvent::SpellCast {
                    caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                    filter: SpellFilter::any(),
                    self_cast: false,
                    include_copy: false,
                    min_mana_value: Some(minimum),
                },
            ]),
            Self::OwnEndPowerAtLeast { minimum } => {
                TriggerSpec::step_began(Step::End, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControlledCreaturePowerAtLeast(minimum))
            }
            Self::UpkeepOpponentMoreLands {} => {
                TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                    .only_if(TriggerCondition::OpponentHasMoreLands)
            }
            Self::OwnEndWithoutAttacking {} => {
                TriggerSpec::step_began(Step::End, PlayerRelation::Controller)
                    .only_if(TriggerCondition::SourceDidNotAttackThisTurn)
            }
            Self::EndWithoutLoyaltyActivation {} => {
                TriggerSpec::step_began(Step::End, PlayerRelation::Controller)
                    .only_if(TriggerCondition::NoLoyaltyActivationThisTurn)
            }
            Self::EndOpponentLostLife { amount } => {
                TriggerSpec::step_began(Step::End, PlayerRelation::Controller)
                    .only_if(TriggerCondition::OpponentLostLifeAtLeast(amount))
            }
            Self::OpponentGraveyardWithQuestCounters { amount } => {
                TriggerSpec::new(TriggerEvent::PutIntoGraveyard {
                    subject: ObjectFilter::any(),
                    player: PlayerRelation::Opponent,
                    from_library: false,
                    own_turn_only: false,
                })
                .only_if(TriggerCondition::SourceCounterAtLeast {
                    kind: spellwake_engine::effects::CounterKind::Quest,
                    minimum: amount,
                })
            }
            Self::UpkeepGreatestArtifact {} => {
                TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControlsGreatestArtifact)
            }
            Self::ControllerCastsSpellTargetingOnlySource {} => TriggerSpec::spell_cast(
                PlayerRelation::Controller,
                SpellFilter::instant_or_sorcery().targeting_only_source(),
            ),
            Self::GraveyardOwnerCastsPlaneswalker {} => TriggerSpec::spell_cast(
                PlayerRelation::Controller,
                SpellFilter::of_type(CardType::Planeswalker),
            )
            .in_zone(TriggerZone::Graveyard),
            Self::GraveyardUpkeep {} => {
                TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                    .in_zone(TriggerZone::Graveyard)
            }
            Self::ControllerStepWithCreatureCount { step: name, count } => {
                TriggerSpec::step_began(step(&name)?, PlayerRelation::Controller)
                    .only_if(TriggerCondition::ControlledCreatureCount(count))
            }
            Self::StepBegan { step: name, player } => {
                TriggerSpec::step_began(step(&name)?, player_relation(&player)?)
            }
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
impl Trigger {
    fn native(self, vocabulary: &CreatureTypeVocabulary) -> Result<TriggeredAbility, String> {
        Ok(TriggeredAbility {
            grouping: trigger_grouping(&self.grouping)?,
            id: TriggeredAbilityId(self.id),
            condition: self.condition.native(vocabulary)?,
            body: self.body.native(vocabulary)?,
        })
    }
}
fn effect_group(
    group: &GroupWire,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<spellwake_engine::program::EffectSubject, String> {
    use spellwake_engine::program::{EffectSubject, ObjectFilter, PowerFilter};
    let GroupWire::Named(group) = group else {
        return match group {
            GroupWire::ExceptChosen { except_chosen_type } => Ok(EffectSubject::Matching(
                ObjectFilter::creature()
                    .except_chosen_slot(creature_types::slot(except_chosen_type.clone())?),
            )),
            GroupWire::CreatureSubtype { creature_subtype } => Ok(EffectSubject::Matching(
                ObjectFilter::creature()
                    .with_subtype(creature_types::id(creature_subtype.clone(), vocabulary)?),
            )),
            GroupWire::OtherControlledCreatureSubtype {
                other_controlled_creature_subtype,
            } => Ok(EffectSubject::Matching(
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source()
                    .with_subtype(creature_types::id(
                        other_controlled_creature_subtype.clone(),
                        vocabulary,
                    )?),
            )),
            GroupWire::ControlledCreatureSubtype {
                controlled_creature_subtype,
            } => Ok(EffectSubject::Matching(
                ObjectFilter::creature()
                    .controlled()
                    .with_subtype(creature_types::id(
                        controlled_creature_subtype.clone(),
                        vocabulary,
                    )?),
            )),
            GroupWire::ControlledSubtype {
                controlled_permanent_subtype,
            } => Ok(EffectSubject::Matching(
                ObjectFilter::any()
                    .controlled()
                    .with_subtype(creature_types::id(
                        controlled_permanent_subtype.clone(),
                        vocabulary,
                    )?),
            )),
            GroupWire::Named(_)
            | GroupWire::WithSubtype { .. }
            | GroupWire::WithoutSubtype { .. }
            | GroupWire::NonlandsAtMost { .. } => Err("invalid effect group".into()),
        };
    };
    Ok(match group.as_str() {
        "ControlledCreaturesWithPowerFourOrGreater" => EffectSubject::Matching(
            ObjectFilter::creature()
                .controlled()
                .with_power(PowerFilter::AtLeast(4)),
        ),
        "AllCreatures" => EffectSubject::Matching(ObjectFilter::creature()),
        "TriggeringAttacker" => EffectSubject::TriggeringAttacker,
        "TriggeringTarget" => EffectSubject::TriggeringTarget,
        "TriggeringAttackGroup" => EffectSubject::TriggeringAttackGroup,
        "AbilitySource" => EffectSubject::AbilitySource,
        "OtherControlledCreatures" => EffectSubject::Matching(
            ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
        ),
        "ControlledCreatures" => EffectSubject::Matching(ObjectFilter::creature().controlled()),
        "OpposingCreatures" => EffectSubject::Matching(ObjectFilter::creature().opposing()),
        "ControlledAttackingCreatures" => {
            EffectSubject::Matching(ObjectFilter::creature().controlled().attacking())
        }
        "AttackingCreatures" => EffectSubject::Matching(ObjectFilter::creature().attacking()),
        "ControlledPermanents" => EffectSubject::Matching(ObjectFilter::any().controlled()),
        _ => return Err(format!("unknown effect group {group}")),
    })
}
fn trigger_grouping(value: &str) -> Result<spellwake_engine::program::TriggerGrouping, String> {
    use spellwake_engine::program::TriggerGrouping;
    match value {
        "EachOccurrence" => Ok(TriggerGrouping::EachOccurrence),
        "OneOrMoreOnceEachTurn" => Ok(TriggerGrouping::OneOrMoreOnceEachTurn),
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
        "EffectExile" => ZoneActionKind::EffectExile,
        "EffectDestroy" => ZoneActionKind::EffectDestroy,
        "EffectSacrifice" => ZoneActionKind::EffectSacrifice,
        "EffectReturnSelected" => ZoneActionKind::EffectReturnSelected,
        "EffectLookToHand" => ZoneActionKind::EffectLookToHand,
        "EffectDiscard" => ZoneActionKind::EffectDiscard,
        "EffectMill" => ZoneActionKind::EffectMill,
        "MillCost" => ZoneActionKind::MillCost,
        "EffectSurveil" => ZoneActionKind::EffectSurveil,
        "EffectEnterBattlefield" => ZoneActionKind::EffectEnterBattlefield,
        "SacrificeCost" => ZoneActionKind::SacrificeCost,
        "ExileCost" => ZoneActionKind::ExileCost,
        "EffectRevealedToGraveyard" => ZoneActionKind::EffectRevealedToGraveyard,
        "EffectSearchToGraveyard" => ZoneActionKind::EffectSearchToGraveyard,
        "EffectPutIntoLibrary" => ZoneActionKind::EffectPutIntoLibrary,
        "EffectShuffleGraveyard" => ZoneActionKind::EffectShuffleGraveyard,
        "DiscardCost" => ZoneActionKind::DiscardCost,
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
        "Hand" => Ok(PublicZoneDestination::Hand),
        "Battlefield" => Ok(PublicZoneDestination::Battlefield),
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
enum CastCostCountWire {
    BattlefieldCreatures,
    AttackingCreatures,
    ControlledCreaturesWithPlusOneCounters,
    OwnGraveyardCreatures,
    OwnGraveyardInstantsOrSorceries,
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum CastReductionWire {
    ExileGraveyardCreatures { per_card: u8 },
    StackTargetThreatensCreature { minimum_power: u8, discount: u8 },
    PerObject { count: CastCostCountWire },
    BattlefieldValue { value: String },
    Affinity { selector: Box<Selector> },
    Undaunted,
}

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ExileActionWire {
    Foretell { cost: Cost },
    Suspend { cost: Cost, counters: u16 },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum ConditionalCostWire {
    ControlsCommander {
        cost: Cost,
    },
    Prowl {
        cost: Cost,
    },
    Freerunning {
        cost: Cost,
    },
    Mayhem {
        cost: Cost,
    },
    ExileHandCards {
        cost: Cost,
        count: u8,
        color: String,
    },
    TapPermanents {
        cost: Cost,
        count: u8,
        quality: String,
    },
    WebSlinging {
        cost: Cost,
    },

    Overload {
        cost: Cost,
        each: Body,
    },
    Cleave {
        cost: Cost,
        unbracketed: Body,
    },
    Emerge {
        cost: Cost,
        from: String,
    },
    Dash {
        cost: Cost,
    },
    Blitz {
        cost: Cost,
    },
    Warp {
        cost: Cost,
    },
    Sneak {
        cost: Cost,
    },
    Evoke {
        cost: Cost,
    },
    Mutate {
        cost: Cost,
    },
    Bestow {
        cost: Cost,
    },
    Surge {
        cost: Cost,
    },
    Spectacle {
        cost: Cost,
    },
    Escape {
        cost: Cost,
        exile: u16,
        entry_counters: u16,
    },
}
#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum AdditionalCostWire {
    Entwine { cost: Cost },
    Kicker { cost: Cost },
    KickerX { cost: Cost },
    Multikicker { cost: Cost },
    Replicate { cost: Cost },
    Buyback { cost: Cost },
}
fn additional_costs(values: &[spellwake_engine::program::AdditionalCastCost]) -> Value {
    use spellwake_engine::program::AdditionalCastCost;
    json!(
        values
            .iter()
            .map(|v| match v {
                AdditionalCastCost::Entwine(c) => json!({"tag":"Entwine","cost":cost(c)}),
                AdditionalCastCost::KickerX(c) => json!({"tag":"KickerX","cost":cost(c)}),
                AdditionalCastCost::Kicker(c) => json!({"tag":"Kicker","cost":cost(c)}),
                AdditionalCastCost::KickerExtra { mana, extra } => {
                    json!({"tag":"KickerExtra","cost":cost(mana),"extra":format!("{extra:?}")})
                }
                AdditionalCastCost::Replicate(c) => json!({"tag":"Replicate","cost":cost(c)}),
                AdditionalCastCost::Multikicker(c) => json!({"tag":"Multikicker","cost":cost(c)}),
                AdditionalCastCost::Buyback(c) => json!({"tag":"Buyback","cost":cost(c)}),
            })
            .collect::<Vec<_>>()
    )
}
fn cast_reductions(values: &[spellwake_engine::program::CastReduction]) -> Value {
    use spellwake_engine::program::CastReduction;
    json!(
        values
            .iter()
            .map(|value| match value {
                CastReduction::BattlefieldValue(value) => json!({"tag":"BattlefieldValue","value":format!("{value:?}")}),
                CastReduction::ExileGraveyardCreatures {per_card} => json!({"tag":"ExileGraveyardCreatures","per_card":per_card}),
                CastReduction::PerObject(count) => json!({"tag":"PerObject","count":format!("{count:?}")}),
                CastReduction::Affinity(quality) =>
                    json!({"tag":"Affinity","selector":selector(quality)}),
                CastReduction::StackTargetThreatensCreature {minimum_power,discount} => json!({"tag":"StackTargetThreatensCreature","minimum_power":minimum_power,"discount":discount}),
                CastReduction::Undaunted => json!({"tag":"Undaunted"}),
                CastReduction::CountZone { zone, filter, per_card } => json!({"tag":"CountZone","zone":format!("{zone:?}"),"filter":format!("{filter:?}"),"per_card":per_card}),
                CastReduction::Conditional { condition, discount } => json!({"tag":"Conditional","condition":format!("{condition:?}"),"discount":discount}),
                CastReduction::IfTargets { filter, discount } => json!({"tag":"IfTargets","filter":format!("{filter:?}"),"discount":discount}),
                CastReduction::Dynamic(expr) => json!({"tag":"Dynamic","expr":format!("{expr:?}")}),
            })
            .collect::<Vec<_>>()
    )
}

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum EntryCounterWire {
    ChosenXPlusOne,
    SuspectForGreatestCreatureCount,
    KickedOpposingNonbasicLands { index: usize },
    ChargePerKicker { index: usize },
    FixedCharge { amount: RulesInteger },
    FixedLoyalty { amount: RulesInteger },
    FixedPlusOne { amount: RulesInteger },
    MorbidPlusOne { amount: RulesInteger },
    Bloodthirst { amount: RulesInteger },
    Devour { multiplier: u8 },
    BloodthirstX,
    Sunburst,
}
fn entry_counters(values: &[spellwake_engine::program::EntryCounterAbility]) -> Value {
    use spellwake_engine::program::EntryCounterAbility::*;
    json!(
        values
            .iter()
            .map(|v| match v {
                ChosenXPlusOne => json!({"tag":"ChosenXPlusOne"}),
                SuspectForGreatestCreatureCount => json!({"tag":"SuspectForGreatestCreatureCount"}),
                KickedOpposingNonbasicLands(index) =>
                    json!({"tag":"KickedOpposingNonbasicLands","index":index}),
                ChargePerKicker(index) => json!({"tag":"ChargePerKicker","index":index}),
                FixedLoyalty(n) => json!({"tag":"FixedLoyalty","amount":n}),
                FixedCharge(n) => json!({"tag":"FixedCharge","amount":n}),
                FixedNamed { kind, amount } => {
                    json!({"tag":"FixedNamed","kind":format!("{kind:?}"),"amount":amount})
                }
                FixedPlusOne(n) => json!({"tag":"FixedPlusOne","amount":n}),
                FixedMinusOne(n) => json!({"tag":"FixedMinusOne","amount":n}),
                DynamicCounter { kind, amount } => {
                    json!({"tag":"DynamicCounter","kind":format!("{kind:?}"),"amount":instruction_amount(amount)})
                }
                ConditionalCounter { kind, amount, .. } => {
                    json!({"tag":"ConditionalCounter","kind":format!("{kind:?}"),"amount":instruction_amount(amount)})
                }
                MorbidPlusOne(n) => json!({"tag":"MorbidPlusOne","amount":n}),
                ControlledAndGraveyardSubtypePlusOne(subtype) => json!({
                    "tag":"ControlledAndGraveyardSubtypePlusOne","subtype":subtype
                }),
                GraveyardControlledSubtypePlusOne(subtype) => json!({
                    "tag":"GraveyardControlledSubtypePlusOne","subtype":subtype
                }),
                Sunburst => json!({"tag":"Sunburst"}),
                Devour(n) => json!({"tag":"Devour","multiplier":n}),
                BloodthirstX => json!({"tag":"BloodthirstX"}),
                Bloodthirst(n) => json!({"tag":"Bloodthirst","amount":n}),
                Amplify(n) => json!({"tag":"Amplify","amount":n}),
            })
            .collect::<Vec<_>>()
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum FaceUpWire {
    Morph(Cost),
    Megamorph(Cost),
    Disguise(Cost),
}
impl FaceUpWire {
    fn native(self) -> Result<spellwake_engine::face_down::FaceUpAbility, String> {
        use spellwake_engine::face_down::FaceUpAbility;
        Ok(match self {
            Self::Morph(price) => FaceUpAbility::Morph(price.native()?),
            Self::Megamorph(price) => FaceUpAbility::Megamorph(price.native()?),
            Self::Disguise(price) => FaceUpAbility::Disguise(price.native()?),
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum SpliceDefinition {
    Arcane(Cost),
    InstantOrSorcery(Cost),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisturbDefinition {
    cost: Cost,
    back: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitDefinition {
    left: Value,
    right: Value,
    aftermath: bool,
    #[serde(default)]
    fuse: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MiracleDefinition {
    mana: Cost,
    x_symbols: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    #[serde(default)]
    split: Option<SplitDefinition>,
    #[serde(default)]
    meld_partner: Option<spellwake_contracts::names::EnglishCardName>,
    #[serde(default)]
    disturb: Option<DisturbDefinition>,
    #[serde(default)]
    splice: Option<SpliceDefinition>,
    #[serde(default)]
    adventure: Option<Value>,
    #[serde(default)]
    exile_action: Option<ExileActionWire>,
    #[serde(default, deserialize_with = "objects")]
    entry_counter_abilities: Vec<EntryCounterWire>,
    #[serde(default, deserialize_with = "objects")]
    additional_cast_costs: Vec<AdditionalCostWire>,
    #[serde(default)]
    plot: Option<Cost>,
    #[serde(default)]
    madness: Option<Cost>,
    #[serde(default)]
    miracle: Option<MiracleDefinition>,
    #[serde(default)]
    cast_from_own_exile: bool,
    #[serde(default)]
    flashback: Option<Cost>,
    #[serde(default)]
    flashback_x_mana_symbols: u8,
    #[serde(default)]
    flashback_sacrifice_creatures: u8,
    #[serde(default)]
    flashback_tap_white_creatures: u8,
    #[serde(default)]
    flashback_life: u16,
    #[serde(default)]
    discard_cast_cost: bool,
    #[serde(default)]
    sacrifice_creature_cast_cost: bool,
    #[serde(default)]
    tap_creatures_cast_cost: bool,
    #[serde(default)]
    graveyard_creature_exile_cost: u8,
    #[serde(default)]
    graveyard_exile_x_cost: bool,
    #[serde(default)]
    reveal_colorless_creature_cast_cost: bool,
    #[serde(default)]
    conditional_cost: Option<ConditionalCostWire>,
    #[serde(default, deserialize_with = "objects")]
    cast_reductions: Vec<CastReductionWire>,
    #[serde(default)]
    printed_keywords: Vec<String>,
    #[serde(default, deserialize_with = "objects")]
    face_up_abilities: Vec<FaceUpWire>,
    #[serde(default)]
    activation_restrictions: BTreeMap<u64, String>,
    #[serde(default)]
    loyalty_costs: BTreeMap<u64, i32>,
    #[serde(default)]
    variable_loyalty_costs: BTreeSet<u64>,
    #[serde(default)]
    printed_toxic: Option<RulesInteger>,
    #[serde(default)]
    printed_loyalty: Option<RulesInteger>,
    #[serde(default)]
    printed_defense: Option<RulesInteger>,
    #[serde(default)]
    siege_back: Option<Value>,
    #[serde(default)]
    x_mana_symbols: u8,
    #[serde(default)]
    hybrid_symbols: Vec<[String; 2]>,
    #[serde(default)]
    cannot_be_countered: bool,
    #[serde(default)]
    attack_requirement: Option<String>,
    #[serde(default)]
    entry_copy: Option<String>,
    #[serde(default)]
    one_attacker: bool,
    #[serde(default)]
    cast_during_own_combat: bool,
    #[serde(default)]
    one_blocker: bool,
    #[serde(default)]
    additional_life_cost: Option<Amount>,
    #[serde(default, deserialize_with = "objects")]
    zone_replacements: Vec<ZoneReplacementWire>,
    card_types: Vec<String>,
    land_subtypes: Vec<String>,
    #[serde(default)]
    spell_subtypes: Vec<String>,
    #[serde(default)]
    artifact_subtypes: Vec<String>,
    #[serde(default)]
    enchantment_subtypes: Vec<String>,
    #[serde(default)]
    planeswalker_subtypes: Vec<String>,
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
    #[serde(default)]
    additional_battlefield_abilities: Vec<StaticAbility>,
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
            entry_tap,
            attachment,
            printed_pt,
            casting_cost,
        } => ProgramKind::Permanent(PermanentDefinition {
            entry_tap: entry_tap.map(EntryTapWire::native).transpose()?,
            attachment: attachment
                .map(|label| {
                    attachment_labels()
                        .into_iter()
                        .find(|(name, _)| *name == label)
                        .map(|(_, value)| value)
                        .ok_or_else(|| "unknown attachment".to_owned())
                })
                .transpose()?,
            printed_pt: printed_pt.map(Into::into),
            casting_cost: casting_cost.map(Cost::native).transpose()?,
        }),
        Kind::Spell {
            casting_cost,
            body,
            modes,
        } => ProgramKind::Spell(SpellDefinition {
            modes: modes
                .map(|spec| {
                    Ok::<_, String>(spellwake_engine::program::SpellModes {
                        minimum: spec.minimum,
                        maximum: spec.maximum,
                        conditional_maximum: None,
                        allow_repeat: spec.allow_repeat,
                        distinct_targets: false,
                        uniqueness: spellwake_engine::program::ModalUniqueness::None,
                        mode_costs: Vec::new(),
                        escalate_mana: None,
                        prefix: None,
                        suffix: None,
                        additional: spec
                            .additional
                            .into_iter()
                            .map(|body| body.native(vocabulary))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .transpose()?,
            cost: casting_cost.native()?,
            uncastable_mana_cost: false,
            body: body.native(vocabulary)?,
        }),
    };
    let mana = raw
        .mana
        .into_iter()
        .map(|ability| {
            use spellwake_engine::mana::{
                ChosenTypeManaScope, CounterManaYield, DynamicManaYield, ManaSacrificeCost,
                ManaSideEffect, SpecialColorYield, TriggeredManaRule,
            };
            let output = match ability.output {
                Output::CommanderIdentityWithSpendEffect { cost, effect } => {
                    let effect = match effect.as_str() {
                        "CommanderEntryCounters" => {
                            spellwake_engine::mana::ManaSpendEffect::CommanderEntryCounters
                        }
                        "SharedCommanderTypeScry" => {
                            spellwake_engine::mana::ManaSpendEffect::SharedCommanderTypeScry
                        }
                        _ => return Err("unknown mana spend effect".into()),
                    };
                    ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
                        .spend_effect(effect)
                        .paid(cost.native()?)
                }
                Output::CommanderColorIdentity => {
                    ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
                }
                Output::SourceChargeCounters => {
                    ManaOutput::CounterBased(CounterManaYield::SourceChargeCounters)
                }
                Output::RemoveChargeForColor => {
                    ManaOutput::CounterBased(CounterManaYield::RemoveChargeForColor)
                }
                Output::RemoveStorageCounters => {
                    ManaOutput::CounterBased(CounterManaYield::RemoveStorageCounters)
                }
                Output::TapControlledArtifact { produced } => {
                    ManaOutput::Fixed(mana_map(produced)?).tap_controlled_artifact()
                }
                Output::PaidGreatestCreaturePower { cost } => {
                    ManaOutput::Dynamic(DynamicManaYield::GreatestCreaturePower)
                        .paid(cost.native()?)
                }
                Output::TwoDistinctColors => ManaOutput::TwoColors { distinct: true },
                Output::LandTappedWhileMonarch => {
                    ManaOutput::Triggered(TriggeredManaRule::LandTappedWhileMonarch)
                }
                Output::ImprintedLandTappedSameType => {
                    ManaOutput::Triggered(TriggeredManaRule::ImprintedLandTappedSameType)
                }
                Output::LandTappedSameType => {
                    ManaOutput::Triggered(TriggeredManaRule::LandTappedSameType)
                }
                Output::PermanentTappedForColorless => {
                    ManaOutput::Triggered(TriggeredManaRule::PermanentTappedForColorless)
                }
                Output::ChosenPlayerColor => {
                    ManaOutput::SpecialColor(SpecialColorYield::ChosenPlayerColor)
                }
                Output::FixedWithLifeGain { produced, life } => {
                    ManaOutput::Fixed(mana_map(produced)?)
                        .side_effect(ManaSideEffect::GainLife(life))
                }
                Output::OpponentsLandColors => {
                    ManaOutput::SpecialColor(SpecialColorYield::OpponentsLandColors)
                }
                Output::ChooseColorForCreatureType => {
                    ManaOutput::choose_color_for_creature_type(ChosenTypeManaScope::CreatureSpell)
                }
                Output::ChooseColorForCreatureTypeUncounterable => {
                    ManaOutput::choose_color_for_creature_type(
                        ChosenTypeManaScope::CreatureSpellUncounterable,
                    )
                }
                Output::ChooseColorForCreatureTypeOrAbility => {
                    ManaOutput::choose_color_for_creature_type(
                        ChosenTypeManaScope::CreatureSpellOrAbility,
                    )
                }
                Output::SacrificeSource { tap, output } => {
                    nested_mana_output(*output)?.sacrifice(ManaSacrificeCost::Source { tap })
                }
                Output::SacrificeSubtype { subtype, output } => nested_mana_output(*output)?
                    .sacrifice(ManaSacrificeCost::ControlledSubtype(creature_types::id(
                        subtype, vocabulary,
                    )?)),
                Output::SacrificeCreature { output } => {
                    nested_mana_output(*output)?.sacrifice(ManaSacrificeCost::ControlledCreature)
                }
                Output::RestrictedFixed {
                    produced,
                    restriction,
                } => {
                    ManaOutput::Fixed(mana_map(produced)?).restricted(match restriction.as_str() {
                        "PlaneswalkerSpell" => {
                            spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell
                        }
                        "ColorlessSpell" => spellwake_engine::mana::ManaRestriction::ColorlessSpell,
                        "CommanderSpell" => spellwake_engine::mana::ManaRestriction::CommanderSpell,
                        "ColorlessEldraziSpellOrAbility" => {
                            spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility
                        }
                        "NoncreatureSpell" => {
                            spellwake_engine::mana::ManaRestriction::NoncreatureSpell
                        }
                        "ArtifactSpellOrAbility" => {
                            spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility
                        }
                        _ => return Err("unknown mana restriction".to_owned()),
                    })
                }
                output @ Output::RestrictedChooseColor { .. } => nested_mana_output(output)?,
                Output::PaidTwoDistinctRestricted { cost, restriction } => ManaOutput::TwoColors {
                    distinct: true,
                }
                .restricted(match restriction.as_str() {
                    "PlaneswalkerSpell" => {
                        spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell
                    }
                    "ColorlessSpell" => spellwake_engine::mana::ManaRestriction::ColorlessSpell,
                    "CommanderSpell" => spellwake_engine::mana::ManaRestriction::CommanderSpell,
                    "ColorlessEldraziSpellOrAbility" => {
                        spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility
                    }
                    "NoncreatureSpell" => spellwake_engine::mana::ManaRestriction::NoncreatureSpell,
                    "ArtifactSpellOrAbility" => {
                        spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility
                    }
                    _ => return Err("unknown mana restriction".to_owned()),
                })
                .paid(cost.native()?),
                Output::ControlledCreaturePowerColor {
                    minimum_power,
                    otherwise,
                    matching,
                } => ManaOutput::Dynamic(DynamicManaYield::ControlledCreaturePowerColor {
                    minimum_power,
                    otherwise,
                    matching,
                }),
                Output::CountedColor { permanents } => ManaOutput::Counted {
                    mana_type: None,
                    permanents: permanents.native(vocabulary)?,
                },
                Output::EntryChosenColor { fixed } => {
                    ManaOutput::SpecialColor(SpecialColorYield::EntryChosenColor {
                        fixed: mana(&fixed)?,
                    })
                }
                Output::Conditional {
                    selector,
                    minimum,
                    output,
                } => {
                    let nested = nested_mana_output(*output)?;
                    nested.require_permanents(selector.native(vocabulary)?, minimum)
                }
                Output::HybridFilter { colors } => {
                    ManaOutput::SpecialColor(SpecialColorYield::HybridFilter {
                        colors: [mana(&colors[0])?, mana(&colors[1])?],
                    })
                }
                Output::ConditionalControlledLandTypes {
                    requirements,
                    normal,
                    fulfilled,
                } => ManaOutput::Dynamic(DynamicManaYield::ConditionalControlledLandTypes {
                    requirements: requirements
                        .into_iter()
                        .map(land_types)
                        .collect::<Result<Vec<_>, _>>()?,
                    normal: mana_map(normal)?,
                    fulfilled: mana_map(fulfilled)?,
                }),
                Output::Counted {
                    mana_type,
                    permanents,
                } => ManaOutput::Counted {
                    mana_type: Some(mana(&mana_type)?),
                    permanents: permanents.native(vocabulary)?,
                },
                Output::Fixed { produced } => ManaOutput::Fixed(mana_map(produced)?),
                Output::PaidFixed { cost, produced } => {
                    ManaOutput::Fixed(mana_map(produced)?).paid(cost.native()?)
                }
                Output::ChooseColorWithControllerDamage { colors, damage } => {
                    let count = colors.len();
                    let colors = colors
                        .iter()
                        .map(|name| mana(name))
                        .collect::<Result<BTreeSet<_>, _>>()?;
                    if colors.len() != count {
                        return Err("duplicate mana color".into());
                    }
                    ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors,
                    }
                    .side_effect(ManaSideEffect::DamageController(damage))
                }
                Output::PaidChooseColor {
                    cost,
                    amount,
                    colors,
                } => {
                    let count = colors.len();
                    let colors = colors
                        .iter()
                        .map(|name| mana(name))
                        .collect::<Result<BTreeSet<_>, _>>()?;
                    if colors.len() != count {
                        return Err("duplicate mana color".into());
                    }
                    ManaOutput::ChooseColor { amount, colors }.paid(cost.native()?)
                }
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
        .map(|ability| ability.native(vocabulary))
        .collect::<Result<Vec<_>, String>>()?;
    let triggered = raw
        .triggered
        .into_iter()
        .map(|ability| ability.native(vocabulary))
        .collect::<Result<Vec<_>, String>>()?;
    let program = Program {
        cannot_be_copied: false,
        conditional_flash: None,
        activation_cost_reductions: BTreeMap::new(),
        activation_target_cost_reductions: BTreeMap::new(),
        zone_replacement_extras: BTreeMap::new(),
        cast_timing: None,
        activation_modes: Default::default(),
        entry_copy: raw.entry_copy.map(|rule| match rule.as_str() {
            "GraveyardCreature" => Ok(spellwake_engine::program::EntryCopyRule::GraveyardCreature),
            "ControlledCreatureOrPlaneswalker" => Ok(spellwake_engine::program::EntryCopyRule::ControlledCreatureOrPlaneswalker),
            _ => Err(format!("unknown entry copy rule {rule}")),
        }).transpose()?,
        entry_tap_unless: None,
        gift: None,
        casualty: None,
        conspire: false,
        dredge: None,
        assist: false,
        cipher: false,
        opening_hand_battlefield: false,
        entry_counter_abilities: raw
            .entry_counter_abilities
            .into_iter()
            .map(|v| {
                use spellwake_engine::program::EntryCounterAbility;
                match v {
                    EntryCounterWire::Bloodthirst { amount } => {
                        EntryCounterAbility::Bloodthirst(amount)
                    }
                    EntryCounterWire::ChosenXPlusOne => EntryCounterAbility::ChosenXPlusOne,
                    EntryCounterWire::SuspectForGreatestCreatureCount=>EntryCounterAbility::SuspectForGreatestCreatureCount,
                    EntryCounterWire::KickedOpposingNonbasicLands{index}=>EntryCounterAbility::KickedOpposingNonbasicLands(index),
                    EntryCounterWire::ChargePerKicker{index}=>EntryCounterAbility::ChargePerKicker(index),
                    EntryCounterWire::FixedLoyalty { amount } => EntryCounterAbility::FixedLoyalty(amount),
                    EntryCounterWire::FixedCharge{amount}=>EntryCounterAbility::FixedCharge(amount),
                    EntryCounterWire::FixedPlusOne{amount}=>EntryCounterAbility::FixedPlusOne(amount),
                    EntryCounterWire::Devour { multiplier } => EntryCounterAbility::Devour(multiplier),
                    EntryCounterWire::MorbidPlusOne { amount } => EntryCounterAbility::MorbidPlusOne(amount),
                    EntryCounterWire::BloodthirstX => EntryCounterAbility::BloodthirstX,
                    EntryCounterWire::Sunburst => EntryCounterAbility::Sunburst,
                }
            })
            .collect(),
        cast_reductions: raw
            .cast_reductions
            .into_iter()
            .map(|value| {
                Ok(match value {
                    CastReductionWire::BattlefieldValue { value } => {
                        use spellwake_engine::program::{CastCostValue, CastReduction};
                        CastReduction::BattlefieldValue(match value.as_str() {
                            "TotalCreaturePower" => CastCostValue::TotalCreaturePower,
                            "GreatestCreaturePower" => CastCostValue::GreatestCreaturePower,
                            "NoncreatureArtifactManaValue" => CastCostValue::NoncreatureArtifactManaValue,
                            "NoncreatureEnchantmentManaValue" => CastCostValue::NoncreatureEnchantmentManaValue,
                            _ => return Err(format!("unknown cost formula {value}")),
                        })
                    }
                    CastReductionWire::ExileGraveyardCreatures {per_card} => spellwake_engine::program::CastReduction::ExileGraveyardCreatures {per_card},
                    CastReductionWire::PerObject { count } => {
                        use spellwake_engine::program::{CastCostCount, CastReduction};
                        CastReduction::PerObject(match count {
                            CastCostCountWire::BattlefieldCreatures => CastCostCount::BattlefieldCreatures,
                            CastCostCountWire::AttackingCreatures => CastCostCount::AttackingCreatures,
                            CastCostCountWire::ControlledCreaturesWithPlusOneCounters => CastCostCount::ControlledCreaturesWithPlusOneCounters,
                            CastCostCountWire::OwnGraveyardCreatures => CastCostCount::OwnGraveyardCreatures,
                            CastCostCountWire::OwnGraveyardInstantsOrSorceries => CastCostCount::OwnGraveyardInstantsOrSorceries,
                        })
                    }
                    CastReductionWire::Affinity { selector } => {
                        spellwake_engine::program::CastReduction::Affinity(Box::new(
                            selector.native(vocabulary)?,
                        ))
                    }
                    CastReductionWire::StackTargetThreatensCreature {minimum_power,discount} => spellwake_engine::program::CastReduction::StackTargetThreatensCreature {minimum_power,discount},
                    CastReductionWire::Undaunted => {
                        spellwake_engine::program::CastReduction::Undaunted
                    }
                })
            })
            .collect::<Result<_, String>>()?,
        per_extra_target_cost: None,
        additional_cast_costs: raw
            .additional_cast_costs
            .into_iter()
            .map(|v| {
                use spellwake_engine::program::AdditionalCastCost;
                Ok(match v {
                    AdditionalCostWire::Entwine {cost}=>AdditionalCastCost::Entwine(cost.native()?),
                    AdditionalCostWire::KickerX {cost} => AdditionalCastCost::KickerX(cost.native()?),
                    AdditionalCostWire::Kicker { cost } => {
                        AdditionalCastCost::Kicker(cost.native()?)
                    }
                    AdditionalCostWire::Replicate { cost } => {
                        AdditionalCastCost::Replicate(cost.native()?)
                    }
                    AdditionalCostWire::Multikicker { cost } => {
                        AdditionalCastCost::Multikicker(cost.native()?)
                    }
                    AdditionalCostWire::Buyback { cost } => {
                        AdditionalCastCost::Buyback(cost.native()?)
                    }
                })
            })
            .collect::<Result<_, String>>()?,
        conditional_cost: raw
            .conditional_cost
            .map(|choice| {
                use spellwake_engine::program::ConditionalCastCost;
                Ok::<_, String>(match choice {
                    ConditionalCostWire::Dash{cost}=>ConditionalCastCost::EndStep{kind:spellwake_engine::program::EndStepCast::Dash,mana:cost.native()?},
                    ConditionalCostWire::Blitz{cost}=>ConditionalCastCost::EndStep{kind:spellwake_engine::program::EndStepCast::Blitz,mana:cost.native()?},
                    ConditionalCostWire::Warp{cost}=>ConditionalCastCost::EndStep{kind:spellwake_engine::program::EndStepCast::Warp,mana:cost.native()?},
                    ConditionalCostWire::Overload{cost,each}=>ConditionalCastCost::Overload {mana:cost.native()?,each:Box::new(each.native(vocabulary)?)},
                    ConditionalCostWire::Cleave{cost,unbracketed}=>ConditionalCastCost::Cleave{mana:cost.native()?,unbracketed:Box::new(unbracketed.native(vocabulary)?)},
                    ConditionalCostWire::Prowl{cost}=>ConditionalCastCost::Prowl(cost.native()?),
                    ConditionalCostWire::Freerunning{cost}=>ConditionalCastCost::Freerunning(cost.native()?),
                    ConditionalCostWire::Mayhem{cost}=>ConditionalCastCost::Mayhem(cost.native()?),
                    ConditionalCostWire::ExileHandCards {cost,count,color:label} => ConditionalCastCost::ExileHandCards {mana:cost.native()?,count,color:color(&label)?},
                    ConditionalCostWire::TapPermanents {cost,count,quality}=>ConditionalCastCost::TapPermanents {mana:cost.native()?,count,quality:match quality.as_str() {"Artifact"=>spellwake_engine::program::CastTapQuality::Artifact,"FlyingCreature"=>spellwake_engine::program::CastTapQuality::FlyingCreature,_=>return Err("unknown tap quality".into())}},
                    ConditionalCostWire::WebSlinging{cost}=>ConditionalCastCost::WebSlinging(cost.native()?),
                    ConditionalCostWire::Emerge{cost,from}=>ConditionalCastCost::Emerge{mana:cost.native()?,from:card_type(&from)?},
                    ConditionalCostWire::Sneak{cost}=>ConditionalCastCost::Sneak(cost.native()?),
                    ConditionalCostWire::Evoke{cost}=>ConditionalCastCost::Evoke(cost.native()?),
                    ConditionalCostWire::Mutate {cost}=>ConditionalCastCost::Mutate(cost.native()?),
                    ConditionalCostWire::Bestow { cost } => {
                        ConditionalCastCost::Bestow(cost.native()?)
                    }
                    ConditionalCostWire::ControlsCommander { cost } => ConditionalCastCost::ControlsCommander(cost.native()?),
                    ConditionalCostWire::Surge { cost } => {
                        ConditionalCastCost::Surge(cost.native()?)
                    }
                    ConditionalCostWire::Escape {
                        cost,
                        exile,
                        entry_counters,
                    } => ConditionalCastCost::Escape {
                        mana: cost.native()?,
                        exile,
                        entry_counters,
                    },
                    ConditionalCostWire::Spectacle { cost } => {
                        ConditionalCastCost::Spectacle(cost.native()?)
                    }
                })
            })
            .transpose()?,
        exile_action: raw
            .exile_action
            .map(|kind| {
                Ok::<_, String>(match kind {
                    ExileActionWire::Foretell { cost } => {
                        spellwake_engine::program::ExileAction::Foretell(cost.native()?)
                    }
                    ExileActionWire::Suspend { cost, counters } => {
                        spellwake_engine::program::ExileAction::Suspend {
                            price: cost.native()?,
                            counters,
                        }
                    }
                })
            })
            .transpose()?,
        plot: raw.plot.map(Cost::native).transpose()?,
        splice: raw.splice.map(|splice| {
            use spellwake_engine::program::{SpliceAbility, SpliceQuality};
            let (quality, cost) = match splice {
                SpliceDefinition::Arcane(cost) => (SpliceQuality::Arcane, cost),
                SpliceDefinition::InstantOrSorcery(cost) => (SpliceQuality::InstantOrSorcery, cost),
            };
            cost.native().map(|cost|SpliceAbility {quality,cost})
        }).transpose()?,
        split: raw.split.map(|split| -> Result<_, String> {Ok(Box::new(spellwake_engine::program::SplitCard {
            left:decode_for_owner(split.left,vocabulary)?, right:decode_for_owner(split.right,vocabulary)?, aftermath:split.aftermath, fuse:split.fuse,
        }))}).transpose()?,
        adventure:raw.adventure.map(|value|decode_for_owner(value,vocabulary).map(Box::new)).transpose()?,
        prepare_spell: None,
        enters_prepared: false,
        nightbound_back: None,
        transform_back: None,
        modal_back: None,
        flip_back: None,
        prototype: None,
        madness: raw.madness.map(Cost::native).transpose()?,
        miracle: raw.miracle.map(|value| -> Result<_, String> {
            Ok(spellwake_engine::program::MiracleCost { mana: value.mana.native()?, x_symbols: value.x_symbols })
        }).transpose()?,
        cast_from_own_exile: raw.cast_from_own_exile,
        flashback: raw.flashback.map(Cost::native).transpose()?,
        flashback_x_mana_symbols: raw.flashback_x_mana_symbols,
        flashback_sacrifice_creatures: raw.flashback_sacrifice_creatures,
        flashback_tap_white_creatures: raw.flashback_tap_white_creatures,
        flashback_life:raw.flashback_life,
        discard_cast_cost:raw.discard_cast_cost,
        discard_cast_cost_filter: None,
        sacrifice_creature_cast_cost:raw.sacrifice_creature_cast_cost,
        sacrifice_cast_cost_filter: None,
        tap_creatures_cast_cost:raw.tap_creatures_cast_cost,
        graveyard_creature_exile_cost: raw.graveyard_creature_exile_cost,
        graveyard_exile_x_cost: raw.graveyard_exile_x_cost,
        graveyard_self_cast: None,
        reveal_colorless_creature_cast_cost:raw.reveal_colorless_creature_cast_cost,
        activation_hybrid_symbols: BTreeMap::new(),
        extra_activation_costs: BTreeMap::new(),
        extra_cast_costs: Vec::new(),
        activation_restrictions: raw
            .activation_restrictions
            .into_iter()
            .map(|(id, rule)| {
                Ok((
                    ActivatedAbilityId(id),
                    match rule.as_str() {
                        "Sorcery" => spellwake_engine::activated::ActivationRestriction::Sorcery,
                        "HasCityBlessing" => spellwake_engine::activated::ActivationRestriction::HasCityBlessing,
                        "EmptyHand" => spellwake_engine::activated::ActivationRestriction::EmptyHand,
                        "ControlsCreaturePowerFourOrGreater" => spellwake_engine::activated::ActivationRestriction::ControlsCreaturePowerFourOrGreater,
                        "CreatedTokenThisTurn" => spellwake_engine::activated::ActivationRestriction::CreatedTokenThisTurn,
                        "CreatureDiedThisTurn" => spellwake_engine::activated::ActivationRestriction::CreatureDiedThisTurn,
                        "ControlsThreeLandsWithSameName" => spellwake_engine::activated::ActivationRestriction::ControlsThreeLandsWithSameName,
                        "DuringUpkeep" => spellwake_engine::activated::ActivationRestriction::DuringUpkeep,
                        "OwnTurnBeforeAttackers" => spellwake_engine::activated::ActivationRestriction::OwnTurnBeforeAttackers,
                        "AnyPlayer" => spellwake_engine::activated::ActivationRestriction::AnyPlayer,
                        "DiscountLegendaryCreaturesAndPlaneswalkers" => spellwake_engine::activated::ActivationRestriction::DiscountLegendaryCreaturesAndPlaneswalkers,
                        "PowerUp" => spellwake_engine::activated::ActivationRestriction::PowerUp,
                        "OnceEachTurn" => spellwake_engine::activated::ActivationRestriction::OnceEachTurn,
                        "Exhaust" => spellwake_engine::activated::ActivationRestriction::Exhaust,
                        "Boast" => spellwake_engine::activated::ActivationRestriction::Boast,
                        _ => return Err("invalid activation restriction".to_owned()),
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        face_up_abilities: raw
            .face_up_abilities
            .into_iter()
            .map(FaceUpWire::native)
            .collect::<Result<_, _>>()?,
        loyalty_costs: raw.loyalty_costs.into_iter().map(|(id, value)| (ActivatedAbilityId(id), value)).collect(),
        variable_loyalty_costs: raw.variable_loyalty_costs.into_iter().map(ActivatedAbilityId).collect(),
        printed_keywords: keyword_set(raw.printed_keywords)?,
        printed_toxic: raw.printed_toxic,
        x_mana_symbols: raw.x_mana_symbols,
        hybrid_symbols: raw.hybrid_symbols.into_iter().map(|[first, other]| {
            use spellwake_engine::mana::{HybridSymbol, HybridAlternative};
            Ok::<_,String>(HybridSymbol {first: self::mana(&first)?, alternative: if other == "TwoLife" {
                HybridAlternative::TwoLife
            } else if other == "TwoGeneric" {
                HybridAlternative::TwoGeneric
            } else { HybridAlternative::Mana(self::mana(&other)?) }})
        }).collect::<Result<_,_>>()?,
        cannot_be_countered: raw.cannot_be_countered,
        cast_during_own_combat:raw.cast_during_own_combat,
        combat_limits:spellwake_engine::program::CombatDeclarationLimits {one_attacker:raw.one_attacker,one_blocker:raw.one_blocker},
        attack_requirement: raw.attack_requirement.map(|name| {
            use spellwake_engine::program::AttackRequirement::*;
            use spellwake_engine::program::AttackCostSymbol::*;
            match name.as_str() {
                "LimitAttacksAtTappedController" => Ok(LimitAttacksAtTappedController),
                "TaxPlayerAndPlaneswalkersGeneric" => Ok(PayPerAttacker { protect_player:true, symbol:OneGeneric }),
                "TaxPlaneswalkersGeneric" => Ok(PayPerAttacker { protect_player:false, symbol:OneGeneric }),
                "TaxPlayerAndPlaneswalkersWhitePhyrexian" => Ok(PayPerAttacker { protect_player:true, symbol:WhitePhyrexian }),
                "TaxPlaneswalkersWhitePhyrexian" => Ok(PayPerAttacker { protect_player:false, symbol:WhitePhyrexian }),
                "FlyingCannotAttackController"=>Ok(FlyingCannotAttackController), "EnchantedCannotAttackOrBlockController"=>Ok(EnchantedCannotAttackOrBlockController), "EquippedCannotAttackControllerOrBeSacrificed"=>Ok(EquippedCannotAttackControllerOrBeSacrificed),
                "SourceAttacks" => Ok(SourceAttacks), "SourceAttacksExceptOwner" => Ok(SourceAttacksExceptOwner),
                "AllCreaturesAttack" => Ok(AllCreaturesAttack), "EnchantedCreatureIsGoaded" => Ok(EnchantedCreatureIsGoaded), "AttachedCreatureAttacks" => Ok(AttachedCreatureAttacks),
                _ => Err(format!("unknown attack requirement {name}")),
            }
        }).transpose()?,
        additional_attack_requirements: Vec::new(),
        additional_life_cost: raw.additional_life_cost.map(|amount|amount.native(vocabulary)).transpose()?,
        printed_loyalty: raw.printed_loyalty,
        printed_defense: raw.printed_defense,
        meld_partner:raw.meld_partner,
        disturb: raw.disturb.map(|value| -> Result<_,String> {Ok(Box::new(spellwake_engine::program::Disturb {cost:value.cost.native()?,back:decode_for_owner(value.back,vocabulary)?}))}).transpose()?,
        siege_back: raw.siege_back.map(|value|decode_for_owner(value,vocabulary).map(Box::new)).transpose()?,
        zone_replacements: raw
            .zone_replacements
            .into_iter()
            .map(ZoneReplacementWire::native)
            .collect::<Result<Vec<_>, _>>()?,
        card_types: card_types(raw.card_types)?,
        land_subtypes: land_types(raw.land_subtypes)?,
        spell_subtypes: spell_subtypes(raw.spell_subtypes)?,
        artifact_subtypes: artifact_subtypes(raw.artifact_subtypes)?,
        enchantment_subtypes: enchantment_subtypes(raw.enchantment_subtypes)?,
        planeswalker_subtypes: raw.planeswalker_subtypes.into_iter().map(|value| spellwake_engine::program::PlaneswalkerSubtype::from_label(&value).ok_or_else(|| format!("unknown planeswalker subtype {value}"))).collect::<Result<_, _>>()?,
        creature_subtypes: subtype_set(raw.creature_subtypes, vocabulary, "creature subtype")?,
        names: raw.names,
        supertypes: supertypes(raw.supertypes)?,
        kind,
        color_indicator: raw.color_indicator.map(color_set).transpose()?,
        effect_defined_colors: raw.effect_defined_colors.map(color_set).transpose()?,
        mana,
        activated,
        triggered,
        additional_battlefield_abilities: raw.additional_battlefield_abilities.into_iter().map(|ability| ability.native(vocabulary)).collect::<Result<_, _>>()?,
        static_conditions: BTreeMap::new(),
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
        ColorFilter::Lacks(color) => json!({"tag":"Lacks","color":format!("{color:?}")}),
        ColorFilter::LacksAll(mask) => json!({"tag":"LacksAll","mask":mask}),
        ColorFilter::Has(color) => json!({"tag":"Has","color":format!("{color:?}")}),
        ColorFilter::AnyOf(mask) => json!({"tag":"AnyOf","mask":mask}),
        ColorFilter::Colorless => json!({"tag":"Colorless"}),
        ColorFilter::Colored => json!({"tag":"Colored"}),
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
        StaticLandSubtypeOperand::SourceEntryBasicLandType => {
            json!({"tag":"SourceEntryBasicLandType"})
        }
    }
}
fn static_operand(value: &StaticSubtypeOperand) -> Value {
    match value {
        StaticSubtypeOperand::Literal(types) => literal_subtypes(types),
        StaticSubtypeOperand::SourceEntryCreatureType => json!({"tag":"SourceEntryCreatureType"}),
        StaticSubtypeOperand::SourceCurrentSubtypes => json!({"tag":"SourceCurrentSubtypes"}),
        StaticSubtypeOperand::ImprintedCreatureTypes { keep } => {
            json!({"tag":"ImprintedCreatureTypes","keep":keep.iter().copied().map(projection::creature_type_id).collect::<Vec<_>>()})
        }
        StaticSubtypeOperand::AllCreatureTypes => json!({"tag":"AllCreatureTypes"}),
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
        AbilityRemoval::Flying => "Flying",
        AbilityRemoval::Banding => "Banding",
        AbilityRemoval::BandsWithOther => "BandsWithOther",
    });
    let mut projected = json!({"tag":"Abilities","removal":removal,"granted_keywords":keywords(&value.granted_keywords)});
    if !value.granted_triggers.is_empty() {
        projected["granted_triggers"] = triggered_abilities(&value.granted_triggers);
    }
    if value.grants_unblockable {
        projected["grants_unblockable"] = json!(true);
    }
    if !value.suppressed_keywords.is_empty() {
        projected["suppressed_keywords"] = keywords(&value.suppressed_keywords);
    }
    if let Some(amount) = &value.granted_toxic {
        projected["granted_toxic"] = json!(amount);
    }
    if !value.granted_combat_keywords.is_empty() {
        projected["granted_combat_keywords"] = json!(
            value
                .granted_combat_keywords
                .iter()
                .map(combat_keyword)
                .collect::<Vec<_>>()
        );
    }
    if value.granted_exalted > 0 {
        projected["granted_exalted"] = json!(value.granted_exalted);
    }
    projected
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
                ResolvedPartSpec::TypeWithSubtypes {
                    base: change,
                    artifact_subtypes,
                    enchantment_subtypes,
                } => json!({
                    "tag": "TypeWithSubtypes",
                    "base": type_part(
                        &change.supertypes,
                        &change.types,
                        subtype_operation(&change.subtypes, instruction_operand, subtype_values),
                        subtype_operation(&change.land_subtypes, land_literal, land_type_values),
                    ),
                    "artifact_subtypes": format!("{artifact_subtypes:?}"),
                    "enchantment_subtypes": format!("{enchantment_subtypes:?}"),
                }),
                ResolvedPartSpec::GrantActivated(ability) => json!({"tag":"GrantActivated", "ability":activated(std::slice::from_ref(ability))[0]}),
                ResolvedPartSpec::GrantActivatedRestricted { ability, restriction } => json!({"tag":"GrantActivatedRestricted", "ability":activated(std::slice::from_ref(ability))[0], "restriction":format!("{restriction:?}")}),
                ResolvedPartSpec::GrantTrigger(ability) => json!({"tag":"GrantTrigger", "ability":triggered_abilities(std::slice::from_ref(ability))[0]}),
                ResolvedPartSpec::GrantAnyColorMana => json!({"tag":"GrantAnyColorMana"}),
                ResolvedPartSpec::GrantFixedColorMana(color) => json!({"tag":"GrantFixedColorMana","color":format!("{color:?}")}),
                ResolvedPartSpec::SetPT(value) => {
                    json!({"tag":"SetPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPartSpec::ModifyPT(value) => {
                    json!({"tag":"ModifyPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPartSpec::ModifyPTExpr { power, toughness } => {
                    json!({"tag":"ModifyPTExpr","power":instruction_amount(power),"toughness":instruction_amount(toughness)})
                }
                ResolvedPartSpec::SetPTExpr { power, toughness } => {
                    json!({"tag":"SetPTExpr","power":instruction_amount(power),"toughness":instruction_amount(toughness)})
                }
                ResolvedPartSpec::SetPower(power) => {
                    json!({"tag":"SetPower","power":instruction_amount(power)})
                }
                ResolvedPartSpec::SetToughness(toughness) => {
                    json!({"tag":"SetToughness","toughness":instruction_amount(toughness)})
                }
                ResolvedPartSpec::SwitchPT => json!({"tag":"SwitchPT"}),
                ResolvedPartSpec::Abilities(value) => ability_part(value),
                ResolvedPartSpec::BlockingRestriction(value) => blocking_restriction(value),
                ResolvedPartSpec::GrantMana(output) => {
                    json!({"tag":"GrantMana","output":format!("{output:?}")})
                }
                ResolvedPartSpec::ChosenColor { add } => {
                    json!({"tag":"ChosenColor","add":add})
                }
                ResolvedPartSpec::ChosenBasicLandType { add } => {
                    json!({"tag":"ChosenBasicLandType","add":add})
                }
                ResolvedPartSpec::ProtectionFromEachOpponent => {
                    json!({"tag":"ProtectionFromEachOpponent"})
                }
                ResolvedPartSpec::CannotBeTargetedByColors(colors) => {
                    json!({"tag":"CannotBeTargetedByColors","colors":format!("{colors:?}")})
                }
                ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(colors) => {
                    json!({"tag":"CannotBeTargetedBySpellsOfColors","colors":format!("{colors:?}")})
                }
                ResolvedPartSpec::MustBlockSubject(subject) => {
                    json!({"tag":"MustBlockSubject","subject":format!("{subject:?}")})
                }
                ResolvedPartSpec::GrantLoyalty { ability, loyalty, variable_loyalty } => {
                    json!({"tag":"GrantLoyalty","ability":format!("{ability:?}"),"loyalty":loyalty,"variable_loyalty":variable_loyalty})
                }
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
                ResolvedPart::TypeWithSubtypes {
                    base: change,
                    artifact_subtypes,
                    enchantment_subtypes,
                } => json!({
                    "tag": "TypeWithSubtypes",
                    "base": type_part(
                        &change.supertypes,
                        &change.types,
                        subtype_operation(&change.subtypes, literal_subtypes, subtype_values),
                        subtype_operation(&change.land_subtypes, land_literal, land_type_values),
                    ),
                    "artifact_subtypes": format!("{artifact_subtypes:?}"),
                    "enchantment_subtypes": format!("{enchantment_subtypes:?}"),
                }),
                ResolvedPart::SetPT(value) => {
                    json!({"tag":"SetPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPart::SetPower(value) => {
                    json!({"tag":"SetPower","power":value})
                }
                ResolvedPart::SetToughness(value) => {
                    json!({"tag":"SetToughness","toughness":value})
                }
                ResolvedPart::ModifyPT(value) => {
                    json!({"tag":"ModifyPT","power":value.power,"toughness":value.toughness})
                }
                ResolvedPart::SwitchPT => json!({"tag":"SwitchPT"}),
                ResolvedPart::Abilities(value) => ability_part(value),
                ResolvedPart::InheritedAbilities(abilities) => json!({"tag":"InheritedAbilities", "activated":abilities.activated.iter().map(|(key,(a,_))|(format!("{key:?}"),activated(std::slice::from_ref(a))[0].clone())).collect::<std::collections::BTreeMap<_,_>>(),"mana":abilities.mana.iter().map(|(k,v)|(format!("{k:?}"),format!("{v:?}"))).collect::<std::collections::BTreeMap<_,_>>()}),
                ResolvedPart::GrantActivated(ability) => json!({"tag":"GrantActivated", "ability":activated(std::slice::from_ref(ability))[0]}),
                ResolvedPart::GrantActivatedRestricted { ability, restriction } => json!({"tag":"GrantActivatedRestricted", "ability":activated(std::slice::from_ref(ability))[0], "restriction":format!("{restriction:?}")}),
                ResolvedPart::GrantTrigger(ability) => json!({"tag":"GrantTrigger", "ability":triggered_abilities(std::slice::from_ref(ability))[0]}),
                ResolvedPart::GrantAnyColorMana => json!({"tag":"GrantAnyColorMana"}),
                ResolvedPart::GrantFixedColorMana(color) => json!({"tag":"GrantFixedColorMana","color":format!("{color:?}")}),
                ResolvedPart::BlockingRestriction(value) => blocking_restriction(value),
                ResolvedPart::CompoundAbilities(parts) => json!({"tag":"CompoundAbilities","count":parts.len()}),
                ResolvedPart::GrantMana(output) => {
                    json!({"tag":"GrantMana","output":format!("{output:?}")})
                }
                ResolvedPart::CannotBeTargetedByColors(colors) => {
                    json!({"tag":"CannotBeTargetedByColors","colors":format!("{colors:?}")})
                }
                ResolvedPart::CannotBeTargetedBySpellsOfColors(colors) => {
                    json!({"tag":"CannotBeTargetedBySpellsOfColors","colors":format!("{colors:?}")})
                }
                ResolvedPart::GrantRestriction(restriction) => {
                    json!({"tag":"GrantRestriction","restriction":format!("{restriction:?}")})
                }
                ResolvedPart::GrantLoyalty { ability, loyalty, variable_loyalty } => {
                    json!({"tag":"GrantLoyalty","ability":format!("{ability:?}"),"loyalty":loyalty,"variable_loyalty":variable_loyalty})
                }
            })
            .collect::<Vec<_>>()
    )
}
/// The explicit current duration string; no omitted-duration default exists.
pub fn duration(value: ResolvedDuration) -> Value {
    match value {
        ResolvedDuration::NoStatedDuration => json!("NoStatedDuration"),
        ResolvedDuration::UntilEndOfCombat => json!("UntilEndOfCombat"),
        ResolvedDuration::UntilEndOfTurn => json!("UntilEndOfTurn"),
        ResolvedDuration::UntilControllerNextTurn => json!("UntilControllerNextTurn"),
        ResolvedDuration::UntilPlayerNextTurn {
            player,
            created_in_turn,
        } => {
            json!({"tag":"UntilPlayerNextTurn","player":player.0,"created_in_turn":created_in_turn})
        }
        ResolvedDuration::ThroughControllerNextTurn => json!("ThroughControllerNextTurn"),
        ResolvedDuration::WhileControllingAbilitySource => json!("WhileControllingAbilitySource"),
        ResolvedDuration::WhileSourceControlled {
            source,
            controller,
            epoch,
        } => {
            json!({"tag":"WhileSourceControlled","source":format!("{source:?}"),"controller":controller.0,"epoch":format!("{epoch:?}")})
        }
        ResolvedDuration::WhileAbilitySourceOnBattlefield => {
            json!("WhileAbilitySourceOnBattlefield")
        }
        ResolvedDuration::WhileSourceOnBattlefield { source } => {
            json!({"tag":"WhileSourceOnBattlefield","source":format!("{source:?}")})
        }
        ResolvedDuration::WhileAbilitySourceTapped => json!("WhileAbilitySourceTapped"),
        ResolvedDuration::WhileSourceTapped { source } => {
            json!({"tag":"WhileSourceTapped","source":format!("{source:?}")})
        }
        ResolvedDuration::WhileControllingTappedAbilitySource => {
            json!("WhileControllingTappedAbilitySource")
        }
        ResolvedDuration::WhileSourceControlledAndTapped {
            source,
            controller,
            epoch,
        } => {
            json!({"tag":"WhileSourceControlledAndTapped","source":format!("{source:?}"),"controller":controller.0,"epoch":format!("{epoch:?}")})
        }
        ResolvedDuration::ThroughNextTurn {
            player,
            created_in_turn,
        } => json!({"tag":"ThroughNextTurn","player":player.0,"created_in_turn":created_in_turn}),
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
    let mut result = json!({
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
            ControllerSelector::SourceItself=>"SourceItself",
            ControllerSelector::SelfAtMaxSpeed=>"SelfAtMaxSpeed",
            ControllerSelector::SourcePair => "SourcePair",
            ControllerSelector::SourceController => "SourceController",
            ControllerSelector::Opponents => "Opponents",
            ControllerSelector::EnchantedPlayer => "EnchantedPlayer",
            ControllerSelector::OwnerIsSourceController => "OwnerIsSourceController",
            ControllerSelector::OwnerIsOpponent => "OwnerIsOpponent",
        },
        "exclude_source":value.exclude_source,
    });
    if let Some(token) = value.token {
        result["token"] = json!(token);
    }
    if let Some(subtype) = value.forbidden_creature_subtype {
        result["forbidden_creature_subtype"] = projection::creature_type_id(subtype);
    }
    if let Some((color, limit)) = value.devotion_below {
        result["devotion_below"] = json!([format!("{color:?}"), limit]);
    }
    if let Some(role) = value.combat_role {
        result["combat_role"] = json!(format!("{role:?}"));
    }
    if value.minimum_plus_one_counters != 0 {
        result["minimum_plus_one_counters"] = json!(value.minimum_plus_one_counters);
    }
    if value.plus_one_counter {
        result["plus_one_counter"] = json!(true);
    }
    if value.matches_source_entry_type {
        result["matches_source_entry_type"] = json!(true);
    }
    if let Some(kind) = value.artifact_subtype {
        result["artifact_subtype"] = json!(kind.wire_label());
    }
    if value.requires_untapped {
        result["requires_untapped"] = json!(true);
    }
    if value.sole_controlled_creature {
        result["sole_controlled_creature"] = json!(true);
    }
    if let Some((kind, count)) = value.controller_type_threshold {
        result["controller_type_threshold"] = json!([format!("{kind:?}"), count.get()]);
    }
    if value.requires_flying {
        result["requires_flying"] = json!(true);
    }
    if value.any_counter {
        result["any_counter"] = json!(true);
    }
    if value.exclude_auras {
        result["exclude_auras"] = json!(true);
    }
    if value.source_has_imprinted_creature {
        result["source_has_imprinted_creature"] = json!(true);
    }
    if let Some(kind) = value.controlled_creatures_and_typed_tokens {
        result["controlled_creatures_and_typed_tokens"] = projection::creature_type_id(kind);
    }
    if value.equipped_by_at_least > 0 {
        result["equipped_by_at_least"] = json!(value.equipped_by_at_least);
    }
    if value.while_controlling_own_commander {
        result["while_controlling_own_commander"] = json!(true);
    }
    if let Some(minimum) = value.station_at {
        result["station_at"] = json!(minimum);
    }
    if value.attached_to_source {
        result["attached_to_source"] = json!(true);
    }
    result
}
fn discounted_spell_wire(matching: &SpellFilter) -> Value {
    let m = *matching;
    if m == SpellFilter::any() {
        json!({"tag":"Any"})
    } else if m == SpellFilter::colorless() {
        json!({"tag":"Colorless"})
    } else if m == SpellFilter::creature().with_power(PowerFilter::AtLeast(4)) {
        json!({"tag":"CreaturePowerAtLeastFour"})
    } else if m == SpellFilter::creature().of_chosen_entry_type() {
        json!({"tag":"CreatureOfSourceEntryType"})
    } else if m == SpellFilter::aura_or_equipment() {
        json!({"tag":"AuraOrEquipment"})
    } else if m == SpellFilter::instant_or_sorcery() {
        json!({"tag":"InstantOrSorcery"})
    } else if let ObjectFilter::Either(a, b) = m.object
        && let (SubtypeFilter::Has(first), SubtypeFilter::Has(second)) = (a.subtypes, b.subtypes)
    {
        json!({"tag":"EitherSubtype","kinds":[projection::creature_type_id(first), projection::creature_type_id(second)]})
    } else if let ObjectFilter::Clause(c) = m.object {
        if let ObjectColorFilter::Mask { any_of, .. } = c.color
            && let Some(color) = [
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]
            .into_iter()
            .find(|&col| any_of == spellwake_engine::program::ColorMask::from_color(col))
        {
            json!({"tag":"Color","color":format!("{color:?}")})
        } else {
            let types = mask_card_types(c.any_types);
            json!({"tag":"CardType","kind":format!("{:?}", types[0])})
        }
    } else {
        json!({"tag":"SpellFilter","debug":format!("{m:?}")})
    }
}

fn expression(value: &NumericExpression) -> Value {
    use spellwake_engine::characteristics::{StaticBonusCondition, StaticPlayerStat, StaticZone};
    use spellwake_engine::effects::CounterKind;
    use spellwake_engine::program::{CardTypeMask, ObjectStatKind};
    match value {
        NumericExpression::Eval(expr) => json!({"tag":"Eval","expr":format!("{expr:?}")}),
        NumericExpression::ZoneCount {
            zone: StaticZone::OwnGraveyard,
            types,
        } if *types == CardTypeMask::CREATURE => json!({"tag":"OwnGraveyardCreatureCount"}),
        NumericExpression::ZoneCount {
            zone: StaticZone::OwnGraveyard,
            types,
        } if *types == CardTypeMask::INSTANT.union(CardTypeMask::SORCERY) => {
            json!({"tag":"OwnGraveyardInstantOrSorceryCount"})
        }
        NumericExpression::ZoneCount {
            zone: StaticZone::OwnHand,
            types,
        } if types.is_empty() => json!({"tag":"OwnHandSize"}),
        NumericExpression::ZoneCount {
            zone: StaticZone::ControlledBattlefield,
            types,
        } if *types == CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT) => {
            json!({"tag":"ControlledArtifactsOrEnchantments"})
        }
        NumericExpression::ZoneCount { zone, types } => {
            json!({"tag":"ZoneCount","zone":format!("{zone:?}"),"types":types.bits()})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::OwnGraveyardGreatestCreaturePower) => {
            json!({"tag":"OwnGraveyardGreatestCreaturePower"})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::HalfHighestOpponentLife) => {
            json!({"tag":"HalfHighestOpponentLife"})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::ManaInPool(kind)) => {
            json!({"tag":"ManaInPool","kind":format!("{kind:?}")})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::Devotion(color)) => {
            json!({"tag":"Devotion","color":format!("{color:?}")})
        }
        NumericExpression::ImprintedStat {
            stat: ObjectStatKind::Power,
            total: false,
        } => json!({"tag":"ImprintedCreaturePower"}),
        NumericExpression::ImprintedStat {
            stat: ObjectStatKind::Toughness,
            total: false,
        } => json!({"tag":"ImprintedCreatureToughness"}),
        NumericExpression::ImprintedStat {
            stat: ObjectStatKind::Power,
            total: true,
        } => json!({"tag":"TotalImprintedCreaturePower"}),
        NumericExpression::ImprintedStat {
            stat: ObjectStatKind::Toughness,
            total: true,
        } => json!({"tag":"TotalImprintedCreatureToughness"}),
        NumericExpression::ImprintedStat { stat, total } => {
            json!({"tag":"ImprintedStat","stat":format!("{stat:?}"),"total":total})
        }
        NumericExpression::Counters {
            source: None,
            kind: CounterKind::Blessing,
        } => json!({"tag":"SourceBlessingCounters"}),
        NumericExpression::Counters {
            source: None,
            kind: CounterKind::Charge,
        } => json!({"tag":"SourceChargeCounters"}),
        NumericExpression::Counters { source: None, kind } => {
            json!({"tag":"SourceCounters","kind":format!("{kind:?}")})
        }
        NumericExpression::Counters {
            source: Some(source),
            kind,
        } => {
            json!({"tag":"LinkedPermanentCounters","source":projection::target(spellwake_engine::targets::TargetRef::Object(*source)),"kind":format!("{kind:?}")})
        }
        NumericExpression::ConditionalBonus {
            condition: StaticBonusCondition::DuringControllersTurn,
            bonus,
        } => json!({"tag":"DuringControllersTurn","bonus":bonus}),
        NumericExpression::ConditionalBonus {
            condition: StaticBonusCondition::ControlledLandsAtLeast(minimum),
            bonus,
        } => json!({"tag":"ControlledLandsThreshold","minimum":minimum,"bonus":bonus}),
        NumericExpression::AttachedAurasAndEquipment { multiplier } => {
            json!({"tag":"AttachedAurasAndEquipment","multiplier":multiplier})
        }
        NumericExpression::Literal(value) => json!({"tag":"Literal","value":value}),
        NumericExpression::Count {
            selector: value,
            multiplier,
            offset,
        } => {
            json!({"tag":"Count","selector":selector(value),"multiplier":multiplier,"offset":offset})
        }
        NumericExpression::Add(lhs, rhs) => {
            json!({"tag":"Add","lhs":expression(lhs),"rhs":expression(rhs)})
        }
        NumericExpression::Scale {
            expr,
            multiplier,
            offset,
        } => {
            json!({"tag":"Scale","expr":expression(expr),"multiplier":multiplier,"offset":offset})
        }
        NumericExpression::Cap { expr, max } => {
            json!({"tag":"Cap","expr":expression(expr),"max":max})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::TotalUnspentMana) => {
            json!({"tag":"PlayerStat","stat":"TotalUnspentMana"})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::OtherControlledCreatureCounters(kind)) => {
            json!({"tag":"PlayerStat","stat":"OtherControlledCreatureCounters","kind":format!("{kind:?}")})
        }
        NumericExpression::PlayerStat(StaticPlayerStat::OwnGraveyardWithCycling) => {
            json!({"tag":"PlayerStat","stat":"OwnGraveyardWithCycling"})
        }
    }
}
/// The current singleton static definition, with exact variant-specific fields.
pub fn static_ability(value: &StaticContinuousAbility) -> Value {
    let (id, target, parts) = match value {
        StaticContinuousAbility::CastPermission { id, permission } => {
            return match permission {
                CastPermissionRule::Flash {
                    any_player,
                    matching,
                } => {
                    let m = *matching;
                    let matching_json = if m == SpellFilter::any() {
                        json!({"tag":"Any"})
                    } else if m == SpellFilter::colorless() {
                        json!({"tag":"Colorless"})
                    } else if m == SpellFilter::historic() {
                        json!({"tag":"Historic"})
                    } else if let ObjectFilter::Clause(c) = m.object {
                        if let SubtypeFilter::Has(kind) = c.subtypes {
                            json!({"tag":"CreatureSubtype","kind":projection::creature_type_id(kind)})
                        } else {
                            let types = mask_card_types(c.any_types);
                            json!({"tag":"CardType","kind":format!("{:?}", types[0])})
                        }
                    } else {
                        json!({"tag":"SpellFilter","debug":format!("{m:?}")})
                    };
                    json!({"tag":"CastAsThoughFlash","id":id.0,"any_player":any_player,"matching":matching_json})
                }
                CastPermissionRule::SeeLibraryTop { public } => {
                    json!({"tag":"SeeLibraryTop","id":id.0,"public":public})
                }
                CastPermissionRule::AllPlayersRevealLibraryTop => {
                    json!({"tag":"AllPlayersRevealLibraryTop","id":id.0})
                }
                CastPermissionRule::RevealHands { opponents_only } => {
                    json!({"tag":"RevealHands","id":id.0,"opponents_only":opponents_only})
                }
                CastPermissionRule::PlayLibraryTop(matching) => {
                    json!({"tag":"PlayLibraryTop","id":id.0,"matching":format!("{matching:?}")})
                }
                CastPermissionRule::GraveyardCreatureCast => {
                    json!({"tag":"GraveyardCreatureCast","id":id.0})
                }
                CastPermissionRule::GraveyardSubtypeEncore(subtype) => {
                    json!({"tag":"GraveyardSubtypeEncore","id":id.0,"subtype":projection::creature_type_id(*subtype)})
                }
                CastPermissionRule::SubtypeSpellsHaveReplicate(subtype) => {
                    json!({"tag":"SubtypeSpellsHaveReplicate","id":id.0,"subtype":projection::creature_type_id(*subtype)})
                }
                CastPermissionRule::ColorlessHandSpellsHaveCascade {
                    minimum_mana_value,
                    count,
                } => {
                    json!({"tag":"ColorlessHandSpellsHaveCascade","id":id.0,"minimum_mana_value":minimum_mana_value,"count":count})
                }
                CastPermissionRule::AdditionalLandPlays(count) => {
                    json!({"tag":"AdditionalLandPlays","id":id.0,"count":count})
                }
                CastPermissionRule::AllPlayersAdditionalLandPlays(count) => {
                    json!({"tag":"AllPlayersAdditionalLandPlays","id":id.0,"count":count})
                }
                CastPermissionRule::ControllerSpellsCannotBeCountered(types) => {
                    json!({"tag":"ControllerSpellsCannotBeCountered","id":id.0,"types":types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
                }
                CastPermissionRule::PlayLibraryTopMatching {
                    lands_allowed,
                    filter,
                } => {
                    json!({"tag":"PlayLibraryTopMatching","id":id.0,"lands_allowed":lands_allowed,"filter":format!("{filter:?}")})
                }
                CastPermissionRule::GraveyardLandPlay => {
                    json!({"tag":"GraveyardLandPlay","id":id.0})
                }
                CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(filter) => {
                    json!({"tag":"ControllerSpellsCannotBeCounteredFiltered","id":id.0,"filter":format!("{filter:?}")})
                }
                CastPermissionRule::AllSpellsCannotBeCounteredFiltered(filter) => {
                    json!({"tag":"AllSpellsCannotBeCounteredFiltered","id":id.0,"filter":format!("{filter:?}")})
                }
                CastPermissionRule::LookAtSourceExiledCards => {
                    json!({"tag":"LookAtSourceExiledCards","id":id.0})
                }
                CastPermissionRule::GraveyardMatching {
                    lands_allowed,
                    filter,
                    once_each_turn,
                } => {
                    json!({"tag":"GraveyardMatching","id":id.0,"lands_allowed":lands_allowed,"filter":format!("{filter:?}"),"once_each_turn":once_each_turn})
                }
                CastPermissionRule::GraveyardSelfConditional { condition } => {
                    json!({"tag":"GraveyardSelfConditional","id":id.0,"condition":format!("{condition:?}")})
                }
                CastPermissionRule::ExileMatching {
                    filter,
                    linked_to_source,
                    free,
                    any_color,
                } => {
                    json!({"tag":"ExileMatching","id":id.0,"filter":format!("{filter:?}"),"linked_to_source":linked_to_source,"free":free,"any_color":any_color})
                }
                CastPermissionRule::GrantSpellKeyword {
                    any_player,
                    matching,
                    keyword,
                } => {
                    json!({"tag":"GrantSpellKeyword","id":id.0,"any_player":any_player,"matching":format!("{matching:?}"),"keyword":format!("{keyword:?}")})
                }
                CastPermissionRule::GrantSpellCascade {
                    any_player,
                    matching,
                    count,
                } => {
                    json!({"tag":"GrantSpellCascade","id":id.0,"any_player":any_player,"matching":format!("{matching:?}"),"count":count})
                }
                CastPermissionRule::PlayLibraryTopFiltered {
                    lands_allowed,
                    filter,
                    once_each_turn,
                    own_turn_only,
                } => {
                    json!({"tag":"PlayLibraryTopFiltered","id":id.0,"lands_allowed":lands_allowed,"filter":format!("{filter:?}"),"once_each_turn":once_each_turn,"own_turn_only":own_turn_only})
                }
                CastPermissionRule::GraveyardMatchingOwnTurn {
                    lands_allowed,
                    filter,
                    once_each_turn,
                } => {
                    json!({"tag":"GraveyardMatchingOwnTurn","id":id.0,"lands_allowed":lands_allowed,"filter":format!("{filter:?}"),"once_each_turn":once_each_turn})
                }
                CastPermissionRule::ExileMatchingExt {
                    filter,
                    linked_to_source,
                    free,
                    any_color,
                    lands_allowed,
                    once_each_turn,
                    own_turn_only,
                } => {
                    json!({"tag":"ExileMatchingExt","id":id.0,"filter":format!("{filter:?}"),"linked_to_source":linked_to_source,"free":free,"any_color":any_color,"lands_allowed":lands_allowed,"once_each_turn":once_each_turn,"own_turn_only":own_turn_only})
                }
            };
        }
        StaticContinuousAbility::CostModifier { id, modifier } => {
            return match modifier {
                SpellCostModifier::Discount {
                    any_player: true,
                    matching,
                    amount: SpellDiscount::Fixed(amount),
                } => {
                    json!({"tag":"AllSpellsCostLess","id":id.0,"matching":discounted_spell_wire(matching),"amount":amount})
                }
                SpellCostModifier::Discount {
                    any_player: false,
                    matching,
                    amount,
                } => {
                    let matching = discounted_spell_wire(matching);
                    let amount = match amount {
                        SpellDiscount::SourcePlusOneCounters => {
                            json!({"tag":"SourcePlusOneCounters"})
                        }
                        SpellDiscount::Fixed(generic) if generic.get() == 1 => {
                            return json!({"tag":"ControllerSpellsCostOneLess","id":id.0,"matching":matching});
                        }
                        SpellDiscount::Fixed(generic) => json!({"tag":"Fixed","generic":generic}),
                        SpellDiscount::PerExperienceCounter(value) => {
                            json!({"tag":"PerExperienceCounter","value":value.get()})
                        }
                        SpellDiscount::ControlledCreaturesWithPlusOneCounters => {
                            json!({"tag":"ControlledCreaturesWithPlusOneCounters"})
                        }
                        SpellDiscount::Dynamic(expr) => {
                            json!({"tag":"Dynamic","expr":instruction_amount(expr)})
                        }
                    };
                    json!({"tag":"ControllerSpellsCostLess","id":id.0,"matching":matching,"amount":amount})
                }
                SpellCostModifier::Discount {
                    any_player: true,
                    matching,
                    amount,
                } => {
                    json!({"tag":"AllSpellsCostLess","id":id.0,"matching":discounted_spell_wire(matching),"amount":format!("{amount:?}")})
                }
                SpellCostModifier::IncreaseExceptType { exempt, amount } => {
                    json!({"tag":"IncreaseSpellCostExceptType","id":id.0,"exempt":format!("{exempt:?}"),"amount":amount})
                }
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, amount } => {
                    json!({"tag":"OtherSubtypeSpellsCostLessFromCommandOrBattlefield","id":id.0,"subtype":projection::creature_type_id(*subtype),"amount":amount})
                }
                SpellCostModifier::ChosenTypeOneLessOfEachColor => {
                    json!({"tag":"ChosenTypeSpellsCostOneLessOfEachColor","id":id.0})
                }
                SpellCostModifier::FreeSubtypeCreatureSpells(subtype) => {
                    json!({"tag":"FreeSubtypeCreatureSpells","id":id.0,"subtype":projection::creature_type_id(*subtype)})
                }
                SpellCostModifier::FreeColorlessHandSpellOnce => {
                    json!({"tag":"FreeColorlessHandSpellOnce","id":id.0})
                }
                SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce => {
                    json!({"tag":"PayLifeForEnchantmentOwnTurnOnce","id":id.0})
                }
                SpellCostModifier::NonartifactSpellsHaveImprovise => {
                    json!({"tag":"NonartifactSpellsHaveImprovise","id":id.0})
                }
                SpellCostModifier::HighManaValueProtected { minimum, reduction } => {
                    json!({"tag":"HighManaValueSpellsProtected","id":id.0,"minimum":minimum,"reduction":reduction})
                }
                SpellCostModifier::Increase {
                    any_player,
                    opponents_only,
                    matching,
                    amount,
                } => {
                    json!({"tag":"IncreaseSpellCost","id":id.0,"any_player":any_player,"opponents_only":opponents_only,"matching":format!("{matching:?}"),"amount":amount})
                }
                SpellCostModifier::FirstSpellRule {
                    filter,
                    own_turn_only,
                    each_player,
                    free,
                    pay_life_mv,
                    discount,
                } => {
                    json!({"tag":"FirstSpellRule","id":id.0,"filter":format!("{filter:?}"),"own_turn_only":own_turn_only,"each_player":each_player,"free":free,"pay_life_mv":pay_life_mv,"discount":discount})
                }
                SpellCostModifier::AlternativeForMatching {
                    filter,
                    mana,
                    once_each_turn,
                    own_turn_only,
                    pay_life_mv,
                    each_player,
                } => {
                    json!({"tag":"AlternativeForMatching","id":id.0,"filter":format!("{filter:?}"),"mana":cost(mana),"once_each_turn":once_each_turn,"own_turn_only":own_turn_only,"pay_life_mv":pay_life_mv,"each_player":each_player})
                }
                SpellCostModifier::KeywordCastCost {
                    keyword,
                    controller,
                    delta,
                    less,
                } => {
                    json!({"tag":"KeywordCastCost","id":id.0,"keyword":format!("{keyword:?}"),"controller":format!("{controller:?}"),"delta":delta,"less":less})
                }
            };
        }
        StaticContinuousAbility::DamageRule { id, rule } => {
            return match rule {
                StaticDamageRule::PreventNoncombat => {
                    json!({"tag":"PreventNoncombatDamage","id":id.0})
                }
                StaticDamageRule::PreventSourceCombat => {
                    json!({"tag":"PreventSourceCombatDamage","id":id.0})
                }
                StaticDamageRule::PreventCombatToAttackingArtifacts => {
                    json!({"tag":"PreventCombatDamageToAttackingArtifacts","id":id.0})
                }
                StaticDamageRule::PreventEnchantedCreature { combat_only: false } => {
                    json!({"tag":"PreventEnchantedCreatureDamage","id":id.0})
                }
                StaticDamageRule::PreventEnchantedCreature { combat_only: true } => {
                    json!({"tag":"PreventEnchantedCreatureCombatDamage","id":id.0})
                }
                StaticDamageRule::PreventIncomingCombatAndExile => {
                    json!({"tag":"PreventIncomingCombatDamageAndExile","id":id.0})
                }
                StaticDamageRule::PreventIncomingRemovePlusOne => {
                    json!({"tag":"PreventIncomingDamageRemovePlusOne","id":id.0})
                }
                StaticDamageRule::PreventHalfIncoming => {
                    json!({"tag":"PreventHalfIncomingDamage","id":id.0})
                }
                StaticDamageRule::DoubleToOpponents => {
                    json!({"tag":"DoubleDamageToOpponents","id":id.0})
                }
                StaticDamageRule::DoubleEquippedCombat => {
                    json!({"tag":"DoubleEquippedCombatDamage","id":id.0})
                }
                StaticDamageRule::DoubleToEnchantedPlayer => {
                    json!({"tag":"DoubleDamageToEnchantedPlayer","id":id.0})
                }
                StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype) => {
                    json!({"tag":"ReplaceSubtypeCombatDamageWithMill","id":id.0,"subtype":format!("{subtype:?}")})
                }
                StaticDamageRule::LethalUsesPower => json!({"tag":"LethalUsesPower","id":id.0}),
                StaticDamageRule::ModifyDamage {
                    source,
                    source_controller,
                    recipient,
                    kind,
                    op,
                } => {
                    json!({"tag":"ModifyDamage","id":id.0,"source":format!("{source:?}"),"source_controller":format!("{source_controller:?}"),"recipient":format!("{recipient:?}"),"kind":format!("{kind:?}"),"op":format!("{op:?}")})
                }
                StaticDamageRule::PreventIncomingCombatToSource => {
                    json!({"tag":"PreventIncomingCombatToSource","id":id.0})
                }
                StaticDamageRule::PreventIncomingRemoveNPlusOne => {
                    json!({"tag":"PreventIncomingRemoveNPlusOne","id":id.0})
                }
                StaticDamageRule::RedirectPlayerDamageToSource {
                    include_other_permanents,
                } => {
                    json!({"tag":"RedirectPlayerDamageToSource","id":id.0,"include_other_permanents":include_other_permanents})
                }
                StaticDamageRule::RedirectPlayerDamageToEnchanted => {
                    json!({"tag":"RedirectPlayerDamageToEnchanted","id":id.0})
                }
                StaticDamageRule::MinimumLifeTotal {
                    floor,
                    require_creature,
                } => {
                    json!({"tag":"MinimumLifeTotal","id":id.0,"floor":floor,"require_creature":require_creature})
                }
                StaticDamageRule::DamageCannotBePrevented => {
                    json!({"tag":"DamageCannotBePrevented","id":id.0})
                }
                StaticDamageRule::PreventIncomingRemovePlusOneIfCounter => {
                    json!({"tag":"PreventIncomingRemovePlusOneIfCounter","id":id.0})
                }
                StaticDamageRule::RedirectDamage {
                    source,
                    from,
                    to,
                    combat_only,
                } => {
                    json!({"tag":"RedirectDamage","id":id.0,"source":format!("{source:?}"),"from":format!("{from:?}"),"to":format!("{to:?}"),"combat_only":combat_only})
                }
                StaticDamageRule::ConditionalModifyDamage {
                    condition,
                    source,
                    source_controller,
                    recipient,
                    kind,
                    op,
                } => {
                    json!({"tag":"ConditionalModifyDamage","id":id.0,"condition":format!("{condition:?}"),"source":format!("{source:?}"),"source_controller":format!("{source_controller:?}"),"recipient":format!("{recipient:?}"),"kind":format!("{kind:?}"),"op":format!("{op:?}")})
                }
            };
        }
        StaticContinuousAbility::DrawRule { id, rule } => {
            return match rule {
                StaticDrawRule::OpponentsAtMostOne => {
                    json!({"tag":"OpponentsDrawAtMostOne","id":id.0})
                }
                StaticDrawRule::EmptyDrawWins => json!({"tag":"EmptyDrawWins","id":id.0}),
                StaticDrawRule::EmptyDrawPutCounters { kind, amount } => {
                    json!({"tag":"EmptyDrawPutCounters","id":id.0,"kind":format!("{kind:?}"),"amount":amount})
                }
                StaticDrawRule::ModifyDraw {
                    add,
                    multiply,
                    skip_first_draw_step,
                    require_empty_hand,
                } => {
                    json!({"tag":"ModifyDraw","id":id.0,"add":add,"multiply":multiply,"skip_first_draw_step":skip_first_draw_step,"require_empty_hand":require_empty_hand})
                }
                StaticDrawRule::SplitOpponentsMultipleDraw => {
                    json!({"tag":"SplitOpponentsMultipleDraw","id":id.0})
                }
                StaticDrawRule::ReplaceWithLandKind => {
                    json!({"tag":"ReplaceDrawWithLandKind","id":id.0})
                }
                StaticDrawRule::SkipControllerDrawStep => {
                    json!({"tag":"SkipControllerDrawStep","id":id.0})
                }
                StaticDrawRule::PlayersCannotDraw => {
                    json!({"tag":"PlayersCannotDraw","id":id.0})
                }
                StaticDrawRule::OpponentsCannotDraw => {
                    json!({"tag":"OpponentsCannotDraw","id":id.0})
                }
                StaticDrawRule::NoMaximumHandSize => json!({"tag":"NoMaximumHandSize","id":id.0}),
                StaticDrawRule::AllPlayersNoMaximumHandSize => {
                    json!({"tag":"AllPlayersNoMaximumHandSize","id":id.0})
                }
                StaticDrawRule::MaxHandSize(limit) => {
                    json!({"tag":"MaxHandSize","id":id.0,"limit":limit})
                }
            };
        }
        StaticContinuousAbility::UntapRule { id, rule } => {
            return match rule {
                StaticUntapRule::UntapDuringOtherPlayersUntap => {
                    json!({"tag":"UntapDuringOtherPlayersUntap","id":id.0})
                }
                StaticUntapRule::EnchantedCreatureUntapsOnlyForMonarch => {
                    json!({"tag":"EnchantedCreatureUntapsOnlyForMonarch","id":id.0})
                }
                StaticUntapRule::EnchantedCreatureSkipsUntap => {
                    json!({"tag":"EnchantedCreatureSkipsUntap","id":id.0})
                }
                StaticUntapRule::SourceSkipsUntap => json!({"tag":"SourceSkipsUntap","id":id.0}),
                StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(selector) => {
                    json!({"tag":"UntapMatchingDuringOtherPlayersUntap","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticUntapRule::MayChooseNotToUntapSource => {
                    json!({"tag":"MayChooseNotToUntapSource","id":id.0})
                }
            };
        }
        StaticContinuousAbility::Restriction { id, rule } => {
            return match rule {
                StaticRestriction::PreventLoyaltyActivations => {
                    json!({"tag":"PreventLoyaltyActivations","id":id.0})
                }
                StaticRestriction::PreventArtifactActivations => {
                    json!({"tag":"PreventArtifactActivations","id":id.0})
                }
                StaticRestriction::PreventControlledTransform { subtype, except } => {
                    json!({"tag":"PreventControlledTransform","id":id.0,"subtype":projection::creature_type_id(*subtype),"except":projection::creature_type_id(*except)})
                }
                StaticRestriction::ProhibitLifeAndCreatureCosts => {
                    json!({"tag":"ProhibitLifeAndCreatureCosts","id":id.0})
                }
                StaticRestriction::ProhibitOpponentsCausingSacrifice => {
                    json!({"tag":"ProhibitOpponentsCausingSacrifice","id":id.0})
                }
                StaticRestriction::OpponentsCannotGainLife => {
                    json!({"tag":"OpponentsCannotGainLife","id":id.0})
                }
                StaticRestriction::ControllerLifeTotalCantChange => {
                    json!({"tag":"ControllerLifeTotalCantChange","id":id.0})
                }
                StaticRestriction::PlayersCantSearchLibraries(rel) => {
                    json!({"tag":"PlayersCantSearchLibraries","id":id.0,"players":format!("{rel:?}")})
                }
                StaticRestriction::RestrictOpponentsDuringOwnTurn => {
                    json!({"tag":"RestrictOpponentsDuringOwnTurn","id":id.0})
                }
                StaticRestriction::EnchantedPlayerCastLimit => {
                    json!({"tag":"EnchantedPlayerCastLimit","id":id.0})
                }
                StaticRestriction::ProhibitCastingChosenName(names) => {
                    json!({"tag":"ProhibitCastingChosenName","id":id.0,"nameCount":names.options().len()})
                }
                StaticRestriction::RestrictGraveyardAndLibrary => {
                    json!({"tag":"RestrictGraveyardAndLibrary","id":id.0})
                }
                StaticRestriction::CombatRequiresCityBlessing => {
                    json!({"tag":"CombatRequiresCityBlessing","id":id.0})
                }
                StaticRestriction::ControllerHasHexproof => {
                    json!({"tag":"ControllerHasHexproof","id":id.0})
                }
                StaticRestriction::PlayersCannotGainLife => {
                    json!({"tag":"PlayersCannotGainLife","id":id.0})
                }
                StaticRestriction::OthersEnterTapped(selector) => {
                    json!({"tag":"OthersEnterTapped","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::PreventMatchingActivations(selector) => {
                    json!({"tag":"PreventMatchingActivations","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::OneSpellPerTurn => {
                    json!({"tag":"OneSpellPerTurn","id":id.0})
                }
                StaticRestriction::SuppressEntersTriggers => {
                    json!({"tag":"SuppressEntersTriggers","id":id.0})
                }
                StaticRestriction::ProhibitCasting { player, filter } => {
                    json!({"tag":"ProhibitCasting","id":id.0,"player":format!("{player:?}"),"filter":format!("{filter:?}")})
                }
                StaticRestriction::EnterUntapped(selector) => {
                    json!({"tag":"EnterUntapped","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::ProhibitEntering(selector) => {
                    json!({"tag":"ProhibitEntering","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::FlashSacrificeAtCleanup => {
                    json!({"tag":"FlashSacrificeAtCleanup","id":id.0})
                }
                StaticRestriction::ControllerHasShroud => {
                    json!({"tag":"ControllerHasShroud","id":id.0})
                }
                StaticRestriction::ControllerCantLoseOpponentsCantWin => {
                    json!({"tag":"ControllerCantLoseOpponentsCantWin","id":id.0})
                }
                StaticRestriction::ControllerCantLoseGame => {
                    json!({"tag":"ControllerCantLoseGame","id":id.0})
                }
                StaticRestriction::ControllerCantLoseForZeroLife => {
                    json!({"tag":"ControllerCantLoseForZeroLife","id":id.0})
                }
                StaticRestriction::ControllerCantWinOpponentsCantLose => {
                    json!({"tag":"ControllerCantWinOpponentsCantLose","id":id.0})
                }
                StaticRestriction::CantPlayLands(rel) => {
                    json!({"tag":"CantPlayLands","id":id.0,"players":format!("{rel:?}")})
                }
                StaticRestriction::PreventCounters {
                    selector,
                    players,
                    counter_kind,
                } => {
                    json!({"tag":"PreventCounters","id":id.0,"selector":format!("{selector:?}"),"players":format!("{players:?}"),"counter_kind":format!("{counter_kind:?}")})
                }
                StaticRestriction::IgnoreOpponentsHexproof(selector) => {
                    json!({"tag":"IgnoreOpponentsHexproof","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::ActivateAbilitiesAsThoughHaste(selector) => {
                    json!({"tag":"ActivateAbilitiesAsThoughHaste","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::PreventMatchingNonManaActivations(selector) => {
                    json!({"tag":"PreventMatchingNonManaActivations","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::PreventGraveyardTargeting => {
                    json!({"tag":"PreventGraveyardTargeting","id":id.0})
                }
                StaticRestriction::CannotBeSacrificed(selector) => {
                    json!({"tag":"CannotBeSacrificed","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::CannotBeUntapped(selector) => {
                    json!({"tag":"CannotBeUntapped","id":id.0,"selector":format!("{selector:?}")})
                }
                StaticRestriction::CantBeTargetedBy {
                    selector,
                    auras_only,
                    colors,
                } => {
                    json!({"tag":"CantBeTargetedBy","id":id.0,"selector":format!("{selector:?}"),"auras_only":auras_only,"colors":format!("{colors:?}")})
                }
                StaticRestriction::ControllerOneSpellPerTurn => {
                    json!({"tag":"ControllerOneSpellPerTurn","id":id.0})
                }
                StaticRestriction::OpponentsCantCastDuringCombat => {
                    json!({"tag":"OpponentsCantCastDuringCombat","id":id.0})
                }
                StaticRestriction::OpponentsSorceryOnly => {
                    json!({"tag":"OpponentsSorceryOnly","id":id.0})
                }
                StaticRestriction::SkipStep { player, step } => {
                    json!({"tag":"SkipStep","id":id.0,"player":format!("{player:?}"),"step":format!("{step:?}")})
                }
            };
        }
        StaticContinuousAbility::Modifier { id, rule } => {
            return match rule {
                StaticModifierRule::AdditionalTrigger(mult) => {
                    json!({"tag":"AdditionalTrigger","id":id.0,"rule":format!("{mult:?}")})
                }
                StaticModifierRule::DoubleCreatedTokens => {
                    json!({"tag":"DoubleCreatedTokens","id":id.0})
                }
                StaticModifierRule::DoublePlacedCounters => {
                    json!({"tag":"DoublePlacedCounters","id":id.0})
                }
                StaticModifierRule::AdditionalSelfPlusOneCounter => {
                    json!({"tag":"AdditionalSelfPlusOneCounter","id":id.0})
                }
                StaticModifierRule::OtherCreaturesEnterWithSourceCounters => {
                    json!({"tag":"OtherCreaturesEnterWithSourceCounters","id":id.0})
                }
                StaticModifierRule::OthersEnterWithCounters {
                    selector,
                    kind,
                    count,
                } => {
                    json!({"tag":"OthersEnterWithCounters","id":id.0,"selector":format!("{selector:?}"),"kind":format!("{kind:?}"),"count":count})
                }
                StaticModifierRule::ModifyCounters {
                    selector,
                    counter_kind,
                    add,
                    multiply,
                } => {
                    json!({"tag":"ModifyCounters","id":id.0,"selector":format!("{selector:?}"),"counter_kind":format!("{counter_kind:?}"),"add":add,"multiply":multiply})
                }
                StaticModifierRule::ModifyTokens {
                    controller,
                    add,
                    multiply,
                } => {
                    json!({"tag":"ModifyTokens","id":id.0,"controller":format!("{controller:?}"),"add":add,"multiply":multiply})
                }
                StaticModifierRule::ModifyTokenSubtype {
                    subtype,
                    controller,
                    add,
                    multiply,
                } => {
                    json!({"tag":"ModifyTokenSubtype","id":id.0,"subtype":format!("{subtype:?}"),"controller":format!("{controller:?}"),"add":add,"multiply":multiply})
                }
                StaticModifierRule::HalveOpponentCounters => {
                    json!({"tag":"HalveOpponentCounters","id":id.0})
                }
                StaticModifierRule::ConditionalModifyLifeGain {
                    condition,
                    player,
                    add,
                    multiply,
                } => {
                    json!({"tag":"ConditionalModifyLifeGain","id":id.0,"condition":format!("{condition:?}"),"player":format!("{player:?}"),"add":add,"multiply":multiply})
                }
                StaticModifierRule::BecomesDayIfNeither => {
                    json!({"tag":"BecomesDayIfNeither","id":id.0})
                }
                StaticModifierRule::ModifyLifeGain {
                    player,
                    add,
                    multiply,
                    lose_instead,
                } => {
                    json!({"tag":"ModifyLifeGain","id":id.0,"player":format!("{player:?}"),"add":add,"multiply":multiply,"lose_instead":lose_instead})
                }
                StaticModifierRule::ExtraEntryLoyalty => {
                    json!({"tag":"ExtraEntryLoyalty","id":id.0})
                }
                StaticModifierRule::TwiceLoyaltyPerTurn => {
                    json!({"tag":"TwiceLoyaltyPerTurn","id":id.0})
                }
                StaticModifierRule::AdditionalCreatureDeathTrigger => {
                    json!({"tag":"AdditionalCreatureDeathTrigger","id":id.0})
                }
                StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy => {
                    json!({"tag":"EnterControlledCreaturesAsEnchantedCopy","id":id.0})
                }
                StaticModifierRule::EnterControlledCreaturesAsSourceCopy => {
                    json!({"tag":"EnterControlledCreaturesAsSourceCopy","id":id.0})
                }
                StaticModifierRule::IgnoreLegendForSubtype(subtype) => {
                    json!({"tag":"IgnoreLegendForSubtype","id":id.0,"subtype":projection::creature_type_id(*subtype)})
                }
                StaticModifierRule::IgnoreLegendRule => {
                    json!({"tag":"IgnoreLegendRule","id":id.0})
                }
                StaticModifierRule::IgnoreLegendRuleForController => {
                    json!({"tag":"IgnoreLegendRuleForController","id":id.0})
                }
                StaticModifierRule::RetainMana(kind) => {
                    json!({"tag":"RetainMana","id":id.0,"kind":format!("{kind:?}")})
                }
                StaticModifierRule::Ascend => json!({"tag":"Ascend","id":id.0}),
                StaticModifierRule::AdditionalBlockWhileMonarch => {
                    json!({"tag":"AdditionalBlockWhileMonarch","id":id.0})
                }
                StaticModifierRule::OthersEnterWithCountersExpr {
                    selector,
                    kind,
                    amount,
                } => {
                    json!({"tag":"OthersEnterWithCountersExpr","id":id.0,"selector":format!("{selector:?}"),"kind":format!("{kind:?}"),"amount":format!("{amount:?}")})
                }
                StaticModifierRule::BonusTokenOnCreate {
                    controller,
                    token,
                    count,
                    per_created,
                } => {
                    json!({"tag":"BonusTokenOnCreate","id":id.0,"controller":format!("{controller:?}"),"token":format!("{token:?}"),"count":count,"per_created":per_created})
                }
                StaticModifierRule::ConvertUnspentMana(kind) => {
                    json!({"tag":"ConvertUnspentMana","id":id.0,"mana":format!("{kind:?}")})
                }
                StaticModifierRule::ModifyTokensFiltered {
                    filter,
                    controller,
                    add,
                    multiply,
                } => {
                    json!({"tag":"ModifyTokensFiltered","id":id.0,"filter":format!("{filter:?}"),"controller":format!("{controller:?}"),"add":add,"multiply":multiply})
                }
                StaticModifierRule::BonusTokenFiltered {
                    filter,
                    controller,
                    token,
                    count,
                    per_created,
                } => {
                    json!({"tag":"BonusTokenFiltered","id":id.0,"filter":format!("{filter:?}"),"controller":format!("{controller:?}"),"token":format!("{token:?}"),"count":count,"per_created":per_created})
                }
                StaticModifierRule::StealCreatedTokens {
                    affected,
                    token_filter,
                } => {
                    json!({"tag":"StealCreatedTokens","id":id.0,"affected":format!("{affected:?}"),"token_filter":format!("{token_filter:?}")})
                }
                StaticModifierRule::CrewAndSaddlePowerBonus {
                    selector,
                    crews,
                    saddles,
                    bonus,
                } => {
                    json!({"tag":"CrewAndSaddlePowerBonus","id":id.0,"selector":format!("{selector:?}"),"crews":crews,"saddles":saddles,"bonus":bonus})
                }
                StaticModifierRule::ModifyMaxHandSize { player, delta } => {
                    json!({"tag":"ModifyMaxHandSize","id":id.0,"player":format!("{player:?}"),"delta":delta})
                }
                StaticModifierRule::ActivatedAbilityCost {
                    scope,
                    controller,
                    delta,
                    less,
                    scale,
                    minimum_total_mana,
                } => {
                    json!({"tag":"ActivatedAbilityCost","id":id.0,"scope":format!("{scope:?}"),"controller":format!("{controller:?}"),"delta":delta,"less":less,"scale":scale.as_deref().map(instruction_amount),"minimum_total_mana":minimum_total_mana})
                }
            };
        }
        StaticContinuousAbility::CommandZoneActive { id, agenda } => {
            return json!({"tag":"CommandZoneActive","id":id.0,"hidden":agenda.hidden,"named_slots":agenda.named_slots});
        }
        StaticContinuousAbility::Battlefield {
            id,
            selector,
            parts,
        } => (id, selector, parts),
        StaticContinuousAbility::CharacteristicDefining { id, parts } => {
            let parts: Vec<_> = parts.0.iter().map(|part| match part {
                DefiningPart::CommanderChosenColor(value) => json!({"tag":"CommanderChosenColor","chosen":value.map(|c|format!("{c:?}"))}),
                DefiningPart::Colors(value) => json!({"tag":"Colors","colors":colors(value)}),
                DefiningPart::CreatureTypes(value) => json!({"tag":"CreatureTypes","types":match value {
                    DefiningCreatureTypes::All => json!({"tag":"All"}),
                    DefiningCreatureTypes::Literal(types) => literal_subtypes(types),
                }}),
                DefiningPart::PowerToughness { power, toughness } => json!({"tag":"PowerToughness","power":power.as_ref().map(|value|expression(value)),"toughness":toughness.as_ref().map(|value|expression(value))}),
            }).collect();
            return json!({"tag":"CharacteristicDefining","id":id.0,"parts":parts});
        }
        StaticContinuousAbility::EventReplacement {
            id,
            event,
            condition,
            action,
        } => {
            return json!({"tag":"EventReplacement","id":id.0,"event":format!("{event:?}"),"condition":format!("{condition:?}"),"action":format!("{action:?}")});
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
        StaticPart::TypeWithSubtypes {
            base: change,
            artifact_subtypes,
            enchantment_subtypes,
        } => json!({
            "tag": "TypeWithSubtypes",
            "base": type_part(
                &change.supertypes,
                &change.types,
                subtype_operation(&change.subtypes, static_operand, subtype_values),
                subtype_operation(&change.land_subtypes, static_land_operand, land_type_values),
            ),
            "artifact_subtypes": format!("{artifact_subtypes:?}"),
            "enchantment_subtypes": format!("{enchantment_subtypes:?}"),
        }),
        StaticPart::SetPT(value) => {
            json!({"tag":"SetPT","power":expression(&value.power),"toughness":expression(&value.toughness)})
        }
        StaticPart::SetPower(power) => {
            json!({"tag":"SetPower","power":expression(power)})
        }
        StaticPart::SetToughness(toughness) => {
            json!({"tag":"SetToughness","toughness":expression(toughness)})
        }
        StaticPart::ModifyPT(value) => {
            json!({"tag":"ModifyPT","power":expression(&value.power),"toughness":expression(&value.toughness)})
        }
        StaticPart::ModifyPTPerOwnColor { power, toughness } => {
            json!({"tag":"ModifyPTPerOwnColor","power":power,"toughness":toughness})
        }
        StaticPart::SetPTToOwnManaValue => json!({"tag":"SetPTToOwnManaValue"}),
        StaticPart::SwitchPT => json!({"tag":"SwitchPT"}),
        StaticPart::Abilities(value) => ability_part(value),
                StaticPart::InheritExiledCreatureAbilities => json!({"tag":"InheritExiledCreatureAbilities"}),
                StaticPart::InheritCounteredCreatureAbilities => json!({"tag":"InheritCounteredCreatureAbilities"}),
                StaticPart::GrantActivated(ability) => json!({"tag":"GrantActivated", "ability":activated(std::slice::from_ref(ability))[0]}),
                StaticPart::GrantActivatedRestricted { ability, restriction } => json!({"tag":"GrantActivatedRestricted", "ability":activated(std::slice::from_ref(ability))[0], "restriction":format!("{restriction:?}")}),
                StaticPart::GrantTrigger(ability) => json!({"tag":"GrantTrigger", "ability":triggered_abilities(std::slice::from_ref(ability))[0]}),
                StaticPart::GrantAnyColorMana => json!({"tag":"GrantAnyColorMana"}),
                StaticPart::GrantSubtypeProtection { types, creatures_only } =>
                    json!({"tag":"GrantSubtypeProtection","types":static_operand(types),"creatures_only":creatures_only}),
                StaticPart::GrantFixedColorMana(color) => json!({"tag":"GrantFixedColorMana","color":format!("{color:?}")}),
                StaticPart::GrantProtectionChosenColor => json!({"tag":"GrantProtectionChosenColor"}),
                StaticPart::GrantProtectionChosenPlayer => json!({"tag":"GrantProtectionChosenPlayer"}),
                StaticPart::BlockingRestriction(value) => blocking_restriction(value),
                StaticPart::GrantMana(output) => {
                    json!({"tag":"GrantMana","output":format!("{output:?}")})
                }
                StaticPart::GrantRestriction(restriction) => {
                    json!({"tag":"GrantRestriction","restriction":format!("{restriction:?}")})
                }
                StaticPart::GrantLoyalty { ability, loyalty, variable_loyalty } => {
                    json!({"tag":"GrantLoyalty","ability":format!("{ability:?}"),"loyalty":loyalty,"variable_loyalty":variable_loyalty})
                }
                StaticPart::InheritZoneAbilities { zone, filter, loyalty_only } => {
                    json!({"tag":"InheritZoneAbilities","zone":format!("{zone:?}"),"filter":format!("{filter:?}"),"loyalty_only":loyalty_only})
                }
    }).collect::<Vec<_>>();
    json!({"tag":"Battlefield","id":id.0,"selector":selector(target),"parts":parts})
}
fn mask_card_types(mask: CardTypeMask) -> Vec<CardType> {
    [
        CardType::Battle,
        CardType::Planeswalker,
        CardType::Land,
        CardType::Artifact,
        CardType::Creature,
        CardType::Enchantment,
        CardType::Kindred,
        CardType::Instant,
        CardType::Sorcery,
    ]
    .into_iter()
    .filter(|kind| mask.contains(*kind))
    .collect()
}

fn single_color_in_filter(filter: ObjectColorFilter) -> Option<Color> {
    let ObjectColorFilter::Mask { any_of, none_of } = filter else {
        return None;
    };
    if !none_of.is_empty() {
        return None;
    }
    let colors: Vec<Color> = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ]
    .into_iter()
    .filter(|c| any_of.contains(*c))
    .collect();
    (colors.len() == 1).then(|| colors[0])
}

pub fn instruction_amount(value: &ValueExpr) -> Value {
    match value {
        ValueExpr::Aggregate {
            filter,
            stat,
            op: StatAggregate::CountDistinct,
        } => {
            json!({"tag":"DistinctStatValues","group":distinct_stat_filter_wire(**filter),"stat":format!("{stat:?}")})
        }
        ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(player)) => {
            json!({"tag":"PlayerIsActivePlayer","player":format!("{player:?}")})
        }
        ValueExpr::CounterCount {
            subject: CounterSubjectRef::AbilitySource,
            kind: Some(kind),
        } => json!({"tag":"AbilitySourceCounters","kind":format!("{kind:?}")}),
        ValueExpr::CounterCount {
            subject: CounterSubjectRef::TriggerSource,
            kind: Some(spellwake_engine::effects::CounterKind::PlusOne),
        } => json!({"tag":"TriggerSourcePlusOneCounters"}),
        ValueExpr::CounterCount {
            subject: CounterSubjectRef::DyingSource,
            kind: None,
        } => json!({"tag":"AllCountersOnDyingSource"}),
        ValueExpr::CounterCount {
            subject: CounterSubjectRef::DyingSource,
            kind: Some(spellwake_engine::effects::CounterKind::PlusOne),
        } => json!({"tag":"PlusOneCountersOnDyingSource"}),
        ValueExpr::Aggregate {
            filter,
            stat: ObjectStatKind::Power,
            op: StatAggregate::Max,
        } if **filter == ObjectFilter::creature().controlled() => {
            json!({"tag":"GreatestControlledCreaturePower"})
        }
        ValueExpr::Aggregate {
            filter,
            stat: ObjectStatKind::Power,
            op: StatAggregate::Max,
        } if **filter
            == ObjectFilter::creature()
                .controlled()
                .other_than_ability_source() =>
        {
            json!({"tag":"GreatestOtherControlledCreaturePower"})
        }
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::AbilitySource,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"AbilitySourcePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::TargetObject,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"TargetCreaturePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::TargetObject,
            stat: ObjectStatKind::Toughness,
        } => json!({"tag":"TargetCreatureToughness"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::FinalSaga,
            stat: ObjectStatKind::ManaValue,
        } => json!({"tag":"FinalSagaManaValue"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::TriggeringSpell,
            stat: ObjectStatKind::ManaValue,
        } => json!({"tag":"TriggeringSpellManaValue"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::SacrificedPermanent,
            stat: ObjectStatKind::ManaValue,
        } => json!({"tag":"SacrificedPermanentManaValue"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::LastSacrificedCreature,
            stat: ObjectStatKind::Toughness,
        } => json!({"tag":"LastSacrificedCreatureToughness"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::LastSacrificedCreature,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"LastSacrificedCreaturePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::SacrificedCreature,
            stat: ObjectStatKind::Toughness,
        } => json!({"tag":"SacrificedCreatureToughness"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::SacrificedCreature,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"SacrificedCreaturePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::TriggerSource,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"TriggerSourcePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::DyingSource,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"DyingSourcePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::ExiledCostCreature,
            stat: ObjectStatKind::Power,
        } => json!({"tag":"ExiledCostCreaturePower"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::DestructionTarget,
            stat: ObjectStatKind::Toughness,
        } => json!({"tag":"DestructionTargetToughness"}),
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::DyingCreature,
            stat: ObjectStatKind::Toughness,
        } => json!({"tag":"DyingCreatureToughness"}),
        ValueExpr::EventValue(EventValueKind::TriggeringLifeChange) => {
            json!({"tag":"TriggeringLifeChange"})
        }
        ValueExpr::EventValue(EventValueKind::PaidNamedCounters) => {
            json!({"tag":"PaidNamedCounters"})
        }
        ValueExpr::EventValue(EventValueKind::LargestDiscard) => json!({"tag":"LargestDiscard"}),
        ValueExpr::EventValue(EventValueKind::DeclaredSubtypeAttackers) => {
            json!({"tag":"DeclaredSubtypeAttackers"})
        }
        ValueExpr::EventValue(EventValueKind::EnteredKickerX) => json!({"tag":"EnteredKickerX"}),
        ValueExpr::EventValue(EventValueKind::EnteredKickerCount(idx)) => {
            json!({"tag":"EnteredKickerCount","index":idx})
        }
        ValueExpr::EventValue(EventValueKind::MilledCardsManaValueSum) => {
            json!({"tag":"MilledCardsManaValue"})
        }
        ValueExpr::EventValue(EventValueKind::MilledCardsManaValueMax) => {
            json!({"tag":"HighestMilledManaValue"})
        }
        ValueExpr::EventValue(EventValueKind::ExiledMilledCreaturePower) => {
            json!({"tag":"ExiledMilledCreaturePower"})
        }
        ValueExpr::EventValue(EventValueKind::DestroyedPermanents) => {
            json!({"tag":"DestroyedPermanents"})
        }
        ValueExpr::EventValue(EventValueKind::CombatDamageToPlayer) => {
            json!({"tag":"CombatDamageToPlayer"})
        }
        ValueExpr::EventValue(EventValueKind::DamageReceived) => json!({"tag":"DamageReceived"}),
        ValueExpr::EventValue(EventValueKind::ChosenX) => json!({"tag":"ChosenX"}),
        ValueExpr::EventValue(EventValueKind::MatchedOccurrences) => {
            json!({"tag":"MatchedOccurrences"})
        }
        ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn) => {
            json!({"tag":"OpponentsLifeLostThisTurn"})
        }
        ValueExpr::PlayerStat(PlayerStatExpr::CardsShortOfHandSize(cards)) => {
            json!({"tag":"CardsShortOfHandSize","cards":cards})
        }
        ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn) => {
            json!({"tag":"CardsDrawnThisTurn"})
        }
        ValueExpr::PlayerStat(PlayerStatExpr::LargestHandSize) => json!({"tag":"LargestHandSize"}),
        ValueExpr::AttachedCount(AttachedCountKind::AurasAndEquipmentOnSource) => {
            json!({"tag":"AurasAndEquipmentAttachedToSource"})
        }
        ValueExpr::AttachedCount(AttachedCountKind::EquipmentOnSource) => {
            json!({"tag":"EquipmentAttachedToSource"})
        }
        ValueExpr::AttachedCount(AttachedCountKind::CursesOnRelatedPlayer) => {
            json!({"tag":"CursesAttachedToRelatedPlayer"})
        }
        ValueExpr::Literal(value) => json!({"tag":"Literal","value":value}),
        ValueExpr::CountAttackingController => json!({"tag":"CreaturesAttackingController"}),
        ValueExpr::CountOtherSharingSourceSubtype => {
            json!({"tag":"OtherControlledCreaturesSharingSourceType"})
        }
        ValueExpr::Scale { expr, factor } => match expr.as_ref() {
            ValueExpr::EventValue(EventValueKind::ChosenX) => {
                json!({"tag":"ChosenXTimes","multiplier":factor.to_usize().unwrap_or(0)})
            }
            ValueExpr::EventValue(EventValueKind::PaidCreatureTaps) => {
                json!({"tag":"PaidCreatureTapsTimes","factor":factor.to_usize().unwrap_or(0)})
            }
            ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter,
                controller_scope: None,
            } if **filter
                == ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source() =>
            {
                json!({"tag":"OtherControlledCreaturesTimes","multiplier":factor.to_usize().unwrap_or(0)})
            }
            ValueExpr::Count {
                zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                filter,
                controller_scope: None,
            } => {
                let creatures_only = **filter == ObjectFilter::creature();
                json!({"tag":"GraveyardCount","creatures_only":creatures_only,"multiplier":factor.to_usize().unwrap_or(0)})
            }
            _ => json!({"tag":"Scale","expr":instruction_amount(expr),"factor":factor}),
        },
        ValueExpr::DivConst {
            expr,
            divisor,
            round_up: false,
        } if divisor.get() == 2 => match expr.as_ref() {
            ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter,
                controller_scope: None,
            } => {
                if let ObjectFilter::Clause(c) = filter.as_ref()
                    && let SubtypeFilter::Has(kind) = c.subtypes
                    && **filter == ObjectFilter::any().controlled().with_subtype(kind)
                {
                    json!({"tag":"HalfControlledPermanentsOfSubtype","kind":projection::creature_type_id(kind)})
                } else {
                    json!({"tag":"DivConst","expr":instruction_amount(expr),"divisor":divisor.get()})
                }
            }
            _ => json!({"tag":"DivConst","expr":instruction_amount(expr),"divisor":divisor.get()}),
        },
        ValueExpr::Add(a, b) => {
            json!({"tag":"Add","left":instruction_amount(a),"right":instruction_amount(b)})
        }
        ValueExpr::Sub(a, b) => {
            json!({"tag":"Sub","left":instruction_amount(a),"right":instruction_amount(b)})
        }
        ValueExpr::DomainCount => json!({"tag":"DomainCount"}),
        ValueExpr::OpponentsCount => json!({"tag":"OpponentsCount"}),
        ValueExpr::Devotion(colors) => json!({"tag":"Devotion","colors":format!("{colors:?}")}),
        ValueExpr::Conditional {
            left,
            comparison: Comparison::GreaterOrEqual,
            right,
            then_value,
            else_value,
        } if matches!(
            left.as_ref(),
            ValueExpr::ObjectStat {
                subject: ValueObjectRef::DyingCreature,
                stat: ObjectStatKind::Power,
            }
        ) && then_value.as_literal() == Some(&RulesInteger::from(2))
            && else_value.as_literal() == Some(&RulesInteger::from(1)) =>
        {
            let minimum = right
                .as_literal()
                .and_then(RulesInteger::to_usize)
                .unwrap_or(0);
            json!({"tag":"OneOrTwoByDyingPower","minimum":minimum})
        }
        ValueExpr::Count {
            zone: ValueZone::Hand(ResolvingPlayer::TargetPlayer),
            ..
        } => json!({"tag":"TargetPlayerHandSize"}),
        ValueExpr::Count {
            zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
            filter,
            controller_scope: None,
        } => {
            if **filter == ObjectFilter::creature() {
                json!({"tag":"OwnGraveyardCreatures"})
            } else if **filter == ObjectFilter::instant_or_sorcery() {
                json!({"tag":"OwnGraveyardInstantsOrSorceries"})
            } else {
                json!({"tag":"GraveyardCount","creatures_only":false,"multiplier":1})
            }
        }
        ValueExpr::Count {
            zone: ValueZone::Battlefield,
            filter,
            controller_scope: Some(ResolvingPlayer::TargetPlayer),
        } if **filter == ObjectFilter::creature().tapped() => {
            json!({"tag":"TargetPlayerTappedCreatures"})
        }
        ValueExpr::Count {
            zone: ValueZone::Battlefield,
            filter,
            controller_scope: None,
        } => {
            if **filter == ObjectFilter::creature().attacking() {
                json!({"tag":"AttackingCreatures"})
            } else if **filter
                == ObjectFilter::creature()
                    .controlled()
                    .with_color(ObjectColorFilter::Colorless)
            {
                json!({"tag":"ControlledColorlessCreatures"})
            } else if **filter
                == ObjectFilter::creature()
                    .controlled()
                    .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            {
                json!({"tag":"ControlledCreaturesWithPlusOneCounters"})
            } else if let ObjectFilter::Clause(c) = filter.as_ref() {
                if let SubtypeFilter::Has(kind) = c.subtypes
                    && **filter == ObjectFilter::any().controlled().with_subtype(kind)
                {
                    json!({"tag":"ControlledPermanentsOfSubtype","kind":projection::creature_type_id(kind)})
                } else if let SubtypeFilter::MatchesChosenSlot(chosen) = c.subtypes
                    && **filter == ObjectFilter::any().controlled().of_chosen_slot(chosen)
                {
                    json!({"tag":"ControlledPermanentsOfChosenType","chosen":chosen.ordinal().to_string()})
                } else if let Some(shade) = single_color_in_filter(c.color)
                    && **filter
                        == ObjectFilter::creature()
                            .controlled()
                            .with_color(ObjectColorFilter::has(shade))
                {
                    json!({"tag":"ControlledCreaturesOfColor","color":format!("{shade:?}")})
                } else {
                    let types = mask_card_types(c.any_types);
                    if types.len() == 1
                        && **filter
                            == ObjectFilter::any_types(CardTypeMask::from_card_type(types[0]))
                                .controlled()
                    {
                        json!({"tag":"ControlledPermanentsOfType","card_type":format!("{:?}", types[0])})
                    } else {
                        json!({"tag":"Count","filter":format!("{filter:?}")})
                    }
                }
            } else {
                json!({"tag":"Count","filter":format!("{filter:?}")})
            }
        }
        other => json!({"tag":"ValueExpr","debug":format!("{other:?}")}),
    }
}
fn affected_players(value: AffectedPlayers) -> Value {
    match value {
        AffectedPlayers::One(player) => json!({"tag":"One","player":format!("{player:?}")}),
        AffectedPlayers::EachPlayer => json!({"tag":"EachPlayer"}),
        AffectedPlayers::EachOtherPlayer => json!({"tag":"EachOtherPlayer"}),
        AffectedPlayers::EachOpponent => json!({"tag":"EachOpponent"}),
    }
}
fn permanent_type_filter(value: &PermanentTypeFilter) -> Value {
    let mut row = json!({
        "required_types":value.required_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),
        "forbidden_types":value.forbidden_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()
    });
    if !value.any_types.is_empty() {
        row["any_types"] = json!(
            value
                .any_types
                .iter()
                .map(|kind| format!("{kind:?}"))
                .collect::<Vec<_>>()
        );
    }
    row
}
#[allow(dead_code)]
fn life_adjustment(value: &LifeAdjustment) -> Value {
    match value {
        LifeAdjustment::Gain(amount) => json!({"tag":"Gain","amount":amount}),
        LifeAdjustment::Lose(amount) => json!({"tag":"Lose","amount":amount}),
        LifeAdjustment::Set(total) => json!({"tag":"Set","total":total}),
    }
}
fn token_creation(creation: &TokenCreation) -> Value {
    let recipients = match creation.recipients {
        TokenRecipients::CombatDamageSourceController => "CombatDamageSourceController",
        TokenRecipients::RememberedTargetController => "RememberedTargetController",
        TokenRecipients::ExiledTargetController => "ExiledTargetController",
        TokenRecipients::Controller => "Controller",
        TokenRecipients::EachPlayer => "EachPlayer",
        TokenRecipients::EachOpponent => "EachOpponent",
        TokenRecipients::Player(_) => "Player",
        TokenRecipients::DestroyedPermanentControllers => "DestroyedPermanentControllers",
    };
    let mut row = json!({
        "definition": token_definition(&creation.definition),
        "amount": instruction_amount(&creation.amount),
        "recipients": recipients,
    });
    if let Some(size) = &creation.base_size {
        row["base_size"] = match size {
            TokenSize::AtCreation(amount) => instruction_amount(amount),
            TokenSize::AtCreationPair { power, toughness } => {
                json!({"tag":"AtCreationPair","power":instruction_amount(power),"toughness":instruction_amount(toughness)})
            }
            TokenSize::AbilitySourceCounters(kind) => {
                json!({"tag":"LiveAbilitySourceCounters","kind":format!("{kind:?}")})
            }
        };
    }
    if creation.enters_tapped {
        row["enters_tapped"] = json!(true);
    }
    row
}

pub fn token_definition(value: &TokenDefinition) -> Value {
    let name = match value.name() {
        TokenName::Specified(name) => json!({"tag":"Specified","name":name}),
        TokenName::Default { ordered_subtypes } => {
            json!({"tag":"Default","ordered_subtypes":ordered_subtypes.iter().map(|word|match word {
            TokenSubtype::Creature(value) => json!({"family":"Creature","type":projection::creature_type_id(*value)}),
            TokenSubtype::Artifact(value) => json!({"family":"Artifact","type":value.wire_label()}),
            TokenSubtype::Enchantment(value) => json!({"family":"Enchantment","type":value.wire_label()}),
            TokenSubtype::Land(value) => json!({"family":"Land","type":value.wire_label()}),
        }).collect::<Vec<_>>()})
        }
    };
    json!({"program":view(value.program()),"name":name})
}
/// Shared exact resolution-condition encoding for bodies and real control records.
pub fn resolution_condition(value: &ResolutionCondition) -> Value {
    fn operand(value: &ValueExpr) -> Value {
        match value {
            ValueExpr::Aggregate {
                filter,
                stat,
                op: StatAggregate::CountDistinct,
            } => {
                json!({"tag":"DistinctStatValues","group":distinct_stat_filter_wire(**filter),"stat":format!("{stat:?}")})
            }
            ValueExpr::Literal(value) => json!({"tag":"Literal","value":value}),
            ValueExpr::EventValue(EventValueKind::PaidAlternative) => {
                json!({"tag":"PaidAlternative"})
            }
            ValueExpr::EventValue(EventValueKind::ChosenX) => json!({"tag":"ChosenX"}),
            ValueExpr::EventValue(EventValueKind::CastFromGraveyard) => {
                json!({"tag":"CastFromGraveyard"})
            }
            ValueExpr::EventValue(EventValueKind::CastDuringOwnMain) => {
                json!({"tag":"CastDuringOwnMain"})
            }
            ValueExpr::EventValue(EventValueKind::DeclaredAttackersAtController) => {
                json!({"tag":"DeclaredAttackersAtController"})
            }
            ValueExpr::EventValue(EventValueKind::Bargained) => json!({"tag":"Bargained"}),
            ValueExpr::EventValue(EventValueKind::KickerCount(index)) => {
                json!({"tag":"KickerCount","index":index})
            }
            ValueExpr::EventValue(EventValueKind::SelectedGraveyardExileCreatures) => {
                json!({"tag":"SelectedGraveyardExileCreatures"})
            }
            ValueExpr::EventValue(EventValueKind::SourceAttackedThisTurn) => {
                json!({"tag":"SourceAttackedThisTurn"})
            }
            ValueExpr::EventValue(EventValueKind::LastSacrificeSucceeded) => {
                json!({"tag":"LastSacrificeSucceeded"})
            }
            ValueExpr::EventValue(EventValueKind::CreaturesDiedThisTurn) => {
                json!({"tag":"CreaturesDiedThisTurn"})
            }
            ValueExpr::EventValue(EventValueKind::TriggerSourceOnBattlefield) => {
                json!({"tag":"TriggerSourceOnBattlefield"})
            }
            ValueExpr::EventValue(EventValueKind::MatchedOccurrences) => {
                json!({"tag":"MatchedOccurrences"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeAboveStarting) => {
                json!({"tag":"ControllerLifeAboveStarting"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife) => {
                json!({"tag":"ControllerLife"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestCreature) => {
                json!({"tag":"ControlsGreatestCreature"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch) => {
                json!({"tag":"ControllerIsMonarch"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(player)) => {
                json!({"tag":"PlayerIsActivePlayer","player":format!("{player:?}")})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer) => {
                json!({"tag":"ControllerIsActivePlayer"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes) => {
                json!({"tag":"OwnGraveyardDistinctCardTypes"})
            }
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn) => {
                json!({"tag":"ControllerLifeLostThisTurn"})
            }
            ValueExpr::PlayerStat(other) => {
                json!({"tag":"PlayerStat","kind":format!("{other:?}")})
            }
            ValueExpr::CounterCount {
                subject: CounterSubjectRef::AbilitySource,
                kind: Some(kind),
            } => json!({"tag":"AbilitySourceCounters","kind":format!("{kind:?}")}),
            ValueExpr::CounterCount {
                subject: CounterSubjectRef::ControllerExperience,
                kind: None,
            } => json!({"tag":"ControllerExperienceCounters"}),
            ValueExpr::ObjectStat {
                subject: ValueObjectRef::TargetObject,
                stat: ObjectStatKind::ManaValue,
            } => json!({"tag":"TargetGraveyardCardManaValue"}),
            ValueExpr::ObjectMatches { subject, filter } => match (subject, filter.as_ref()) {
                (MatchObjectRef::RevealedTop, ObjectFilter::Clause(c)) => {
                    let types = mask_card_types(c.any_types);
                    let first = types.first().copied().unwrap_or(CardType::Creature);
                    let second = types.get(1).copied().unwrap_or(first);
                    json!({"tag":"RevealedTopHasType","first":format!("{first:?}"),"second":format!("{second:?}")})
                }
                (MatchObjectRef::TargetObject, ObjectFilter::Clause(c))
                    if let SubtypeFilter::Has(subtype) = c.subtypes =>
                {
                    json!({"tag":"TargetCreatureHasSubtype","subtype":projection::creature_type_id(subtype)})
                }
                (MatchObjectRef::SacrificedCreature, ObjectFilter::Clause(c))
                    if let SubtypeFilter::Has(subtype) = c.subtypes =>
                {
                    json!({"tag":"SacrificedCreatureHadSubtype","subtype":projection::creature_type_id(subtype)})
                }
                (MatchObjectRef::DyingCreature, ObjectFilter::Clause(c))
                    if let SubtypeFilter::Has(subtype) = c.subtypes =>
                {
                    json!({"tag":"DyingCreatureHasSubtype","subtype":projection::creature_type_id(subtype)})
                }
                (MatchObjectRef::DestructionTarget, ObjectFilter::Clause(c))
                    if let SubtypeFilter::Has(kind) = c.subtypes =>
                {
                    json!({"tag":"DestructionTargetHadSubtype","subtype":projection::creature_type_id(kind)})
                }
                (MatchObjectRef::MovedLibraryCard, ObjectFilter::Clause(c)) => {
                    let kind = mask_card_types(c.any_types)
                        .first()
                        .copied()
                        .unwrap_or(CardType::Creature);
                    json!({"tag":"MovedLibraryCardOfType","kind":format!("{kind:?}")})
                }
                (MatchObjectRef::DiscardedCard, ObjectFilter::Clause(c)) => {
                    let kind = mask_card_types(c.any_types)
                        .first()
                        .copied()
                        .unwrap_or(CardType::Creature);
                    json!({"tag":"DiscardedCardOfType","kind":format!("{kind:?}")})
                }
                _ => instruction_amount(value),
            },
            ValueExpr::Count {
                zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                filter,
                controller_scope: None,
            } if **filter == ObjectFilter::any() => json!({"tag":"ControllerGraveyardSize"}),
            ValueExpr::Aggregate {
                filter,
                stat: ObjectStatKind::Power,
                op: StatAggregate::Sum,
            } if **filter == ObjectFilter::creature().controlled() => {
                json!({"tag":"TotalControlledCreaturePower"})
            }
            ValueExpr::Aggregate {
                filter,
                stat: ObjectStatKind::Power,
                op: StatAggregate::Max,
            } if **filter == ObjectFilter::creature().controlled() => {
                json!({"tag":"GreatestControlledCreaturePower"})
            }
            ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter,
                controller_scope: None,
            } => {
                if let ObjectFilter::Clause(c) = filter.as_ref() {
                    if let SubtypeFilter::Has(kind) = c.subtypes
                        && **filter == ObjectFilter::any().controlled().with_subtype(kind)
                    {
                        json!({"tag":"ControlledPermanentsOfSubtype","subtype":projection::creature_type_id(kind)})
                    } else {
                        let types = mask_card_types(c.any_types);
                        if types.len() == 1
                            && **filter
                                == ObjectFilter::any_types(CardTypeMask::from_card_type(types[0]))
                                    .controlled()
                        {
                            json!({"tag":"ControlledPermanentsOfType","kind":format!("{:?}", types[0])})
                        } else {
                            instruction_amount(value)
                        }
                    }
                } else {
                    instruction_amount(value)
                }
            }
            _ => instruction_amount(value),
        }
    }
    json!({"left":operand(&value.left),"comparison":format!("{:?}",value.comparison),"right":operand(&value.right)})
}
pub fn body(value: &ExecutableBody) -> Value {
    let target = match value.target {
        BodyTarget::None => json!({"tag":"None"}),
        BodyTarget::Object { zone, filter } => match zone {
            TargetZone::Battlefield => {
                if filter == ObjectFilter::any() {
                    json!({"tag":"SinglePermanent"})
                } else if filter == ObjectFilter::creature() {
                    json!({"tag":"SingleCreature"})
                } else if filter == ObjectFilter::planeswalker() {
                    json!({"tag":"SinglePlaneswalker"})
                } else if let ObjectFilter::Clause(c) = filter
                    && let SubtypeFilter::Has(kind) = c.subtypes
                    && filter == ObjectFilter::creature().with_subtype(kind)
                {
                    json!({"tag":"CreatureOfSubtype","subtype":projection::creature_type_id(kind)})
                } else if let ObjectFilter::Clause(c) = filter
                    && let SubtypeFilter::Has(kind) = c.subtypes
                    && c.any_types == CardTypeMask::EMPTY
                    && filter == ObjectFilter::any().with_subtype(kind)
                {
                    json!({"tag":"PermanentOfCreatureSubtype","subtype":projection::creature_type_id(kind)})
                } else {
                    json!({"tag":"FilteredPermanent","filter":battlefield_filter_label(filter)})
                }
            }
            TargetZone::OwnGraveyard => {
                if filter == ObjectFilter::any() {
                    json!({"tag":"SingleOwnGraveyardCard"})
                } else if let ObjectFilter::Clause(c) = filter
                    && let SubtypeFilter::Has(kind) = c.subtypes
                    && filter == ObjectFilter::any().with_subtype(kind)
                {
                    json!({"tag":"OwnGraveyardCardWithSubtype","subtype":projection::creature_type_id(kind)})
                } else {
                    json!({"tag":"FilteredOwnGraveyardCard","filter":graveyard_filter_label(filter)})
                }
            }
            TargetZone::AnyGraveyard { opponents_only } => {
                if !opponents_only && filter == ObjectFilter::any() {
                    json!({"tag":"AnyGraveyardCard"})
                } else if !opponents_only && filter == ObjectFilter::creature() {
                    json!({"tag":"AnyGraveyardCreature"})
                } else if !opponents_only && filter == ObjectFilter::creature_or_planeswalker() {
                    json!({"tag":"AnyGraveyardCreatureOrPlaneswalker"})
                } else {
                    json!({"tag":"FilteredAnyGraveyardCard","filter":permanent_group_wire(filter),"opponents_only":opponents_only})
                }
            }
            TargetZone::Exile {
                owned_flashback_only,
            } => {
                if owned_flashback_only && filter == ObjectFilter::any() {
                    json!({"tag":"OwnedExiledFlashbackCard"})
                } else {
                    json!({"tag":"FilteredExiledCard","filter":permanent_group_wire(filter)})
                }
            }
            TargetZone::RecentlyDiscardedOrFromLibrary => {
                json!({"tag":"RecentlyDiscardedOrFromLibrary"})
            }
        },
        BodyTarget::Player { opponents_only } => {
            if opponents_only {
                json!({"tag":"SingleOpponent"})
            } else {
                json!({"tag":"SinglePlayer"})
            }
        }
        BodyTarget::AnyTarget(kind) => match kind {
            AnyTargetKind::Any => json!({"tag":"SingleAnyTarget"}),
            AnyTargetKind::CreatureOrPlayer => json!({"tag":"SingleCreatureOrPlayer"}),
            AnyTargetKind::PlayerOrPlaneswalker => json!({"tag":"SinglePlayerOrPlaneswalker"}),
            AnyTargetKind::OpponentOrPlaneswalker => json!({"tag":"SingleOpponentOrPlaneswalker"}),
            AnyTargetKind::OpponentOrPlaneswalkerThenCreature => {
                json!({"tag":"OpponentOrPlaneswalkerThenCreature"})
            }
        },
        BodyTarget::Spell {
            filter: gate,
            include_abilities,
        } => {
            if include_abilities {
                json!({"tag":"SingleStackObject","filter":format!("{gate:?}")})
            } else {
                let support_gate = if gate == SpellFilter::any() {
                    "AnySpell".to_owned()
                } else if gate == SpellFilter::any().not_from_owners_hand() {
                    "NotCastFromOwnersHand".to_owned()
                } else if gate == SpellFilter::any().single_target() {
                    "SpellWithSingleTarget".to_owned()
                } else if gate == SpellFilter::any().targeting_player() {
                    "SpellTargetingPlayer".to_owned()
                } else if let Some(ordinal) = gate.ordinal_this_turn
                    && gate == SpellFilter::any().ordinal_this_turn(ordinal)
                {
                    format!("CastNumberThisTurn({})", ordinal.get())
                } else if gate == SpellFilter::of_type(CardType::Sorcery) {
                    "SorcerySpell".to_owned()
                } else if gate == SpellFilter::noncreature() {
                    "NoncreatureSpell".to_owned()
                } else if gate == SpellFilter::creature() {
                    "CreatureSpell".to_owned()
                } else if gate == SpellFilter::instant_or_sorcery().controlled() {
                    "ControlledInstantOrSorcery".to_owned()
                } else if gate == SpellFilter::copyable_instruction() {
                    "CopyableInstructionSpell".to_owned()
                } else {
                    format!("{gate:?}")
                };
                json!({"tag":"SingleSpell","support_gate":support_gate})
            }
        }
        BodyTarget::StackEntry(kind) => match kind {
            StackTargetKind::AnyEntry => json!({"tag":"SingleStackEntry"}),
            StackTargetKind::TargetingControlledPermanent => {
                json!({"tag":"StackEntryTargetingControlledPermanent"})
            }
            StackTargetKind::ControlledAbility { colorless_source } => {
                json!({"tag":"ControlledAbility","colorless_source":colorless_source})
            }
        },
    };
    let instructions = value
        .instructions
        .iter()
        .map(|instruction| match instruction {
            Instruction::OptionalSequence { chooser, instructions } => {
                if *instructions == 1 {
                    json!({"op":"OptionalEffect","chooser":format!("{chooser:?}")})
                } else {
                    json!({"op":"OptionalSequence","chooser":format!("{chooser:?}"),"instructions":instructions})
                }
            }
            Instruction::Jump { destination } => json!({"op":"Jump","destination":destination.to_string()}),
            Instruction::ZoneOp(spec) => {
                if let Some((players, amount, filter, object_filter)) = spec.as_group_sacrifice()
                    && let spellwake_engine::program::ObjectFilter::Clause(clause) = filter
                {
                    const ALL_TYPES: [CardType; 9] = [
                        CardType::Battle,
                        CardType::Planeswalker,
                        CardType::Land,
                        CardType::Artifact,
                        CardType::Creature,
                        CardType::Enchantment,
                        CardType::Kindred,
                        CardType::Instant,
                        CardType::Sorcery,
                    ];
                    let mask_to_set =
                        |mask: spellwake_engine::program::CardTypeMask| -> BTreeSet<CardType> {
                            ALL_TYPES
                                .into_iter()
                                .filter(|&kind| mask.contains(kind))
                                .collect()
                        };
                    let ptf = PermanentTypeFilter {
                        any_types: mask_to_set(clause.any_types),
                        required_types: mask_to_set(clause.required_types),
                        forbidden_types: mask_to_set(clause.forbidden_types),
                        object_filter,
                    };
                    json!({"op":"Sacrifice","players":affected_players(players),"amount":instruction_amount(amount),"filter":permanent_type_filter(&ptf)})
                } else if let Some((players, amount)) = spec.as_group_discard() {
                    json!({"op":"Discard","players":affected_players(players),"amount":instruction_amount(amount)})
                } else if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                    && matches!(spec.selection, spellwake_engine::toolbox::ZoneSelection::All)
                    && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Hand)
                {
                    json!({"op":"ReturnToOwnersHand"})
                } else if let spellwake_engine::toolbox::ZoneDest::Delayed(action) =
                    &spec.destination
                {
                    use spellwake_engine::toolbox::DelayedZoneAction;
                    match action {
                        DelayedZoneAction::ReturnExiledTargetCohort {
                            next_end_step,
                            owner_controls,
                        } => {
                            let mut value = json!({"op":"ReturnExiledTargetCohort","next_end_step":next_end_step});
                            if *owner_controls {
                                value["owner_controls"] = json!(true);
                            }
                            value
                        }
                        DelayedZoneAction::ReturnChosenXGraveyardCreatures => {
                            json!({"op":"ReturnChosenXGraveyardCreatures"})
                        }
                        DelayedZoneAction::PutHandPermanentsOntoBattlefield {
                            player,
                            types,
                            maximum,
                            tapped,
                        } => {
                            let mut value = json!({"op":"PutHandPermanentsOntoBattlefield","player":format!("{player:?}"),"types":types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"maximum":maximum});
                            if *tapped {
                                value["tapped"] = json!(true);
                            }
                            value
                        }
                        _ => json!({"op":"ZoneOp","spec":format!("{spec:?}")}),
                    }
                } else {
                    json!({"op":"ZoneOp","spec":format!("{spec:?}")})
                }
            }
            Instruction::DrawCards(spec) => {
                if matches!(
                    &spec.players,
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller
                    )
                ) && spec.bind_drawn.is_none()
                {
                    json!({"op":"DrawController","amount":instruction_amount(&spec.amount)})
                } else {
                    json!({"op":"DrawCards","spec":format!("{spec:?}")})
                }
            }
            Instruction::MutateObject(spec) => {
                use spellwake_engine::toolbox::{
                    AttachOp, KeywordZoneOp, ObjectMutation, ObjectStateMarkerOp, TokenAttachOp,
                };
                match &spec.op {
                    ObjectMutation::Attach(AttachOp::ToObject {
                        ability: Some(kind),
                        ..
                    }) => json!({"op":format!("{kind:?}")}),
                    ObjectMutation::TokenAttach(TokenAttachOp::CreateAndAttach { definition }) => {
                        json!({"op":"CreateTokenAndAttach","definition":token_definition(definition)})
                    }
                    ObjectMutation::StateMarker(ObjectStateMarkerOp::RememberPermanentTarget) => {
                        json!({"op":"RememberPermanentTarget"})
                    }
                    ObjectMutation::ResolveKeywordTrigger(keyword) => match keyword {
                        CombatTriggerKeyword::Afflict(amount) => {
                            json!({"op":"Afflict","amount":amount})
                        }
                        CombatTriggerKeyword::Dethrone => json!({"op":"Dethrone"}),
                        CombatTriggerKeyword::Melee => json!({"op":"Melee"}),
                        CombatTriggerKeyword::Renown(amount) => {
                            json!({"op":"Renown","amount":amount})
                        }
                        CombatTriggerKeyword::Training => json!({"op":"Training"}),
                        CombatTriggerKeyword::Prowess => json!({"op":"Prowess"}),
                        CombatTriggerKeyword::BattleCry => json!({"op":"BattleCry"}),
                        CombatTriggerKeyword::Bushido(amount) => {
                            json!({"op":"Bushido","amount":amount})
                        }
                        CombatTriggerKeyword::Rampage(amount) => {
                            json!({"op":"Rampage","amount":amount})
                        }
                        CombatTriggerKeyword::Decayed => json!({"op":"Decayed"}),
                        CombatTriggerKeyword::Mobilize(n) => json!({"op":"Mobilize","amount":n}),
                        CombatTriggerKeyword::Ingest => json!({"op":"Ingest"}),
                        CombatTriggerKeyword::Poisonous(amount) => {
                            json!({"op":"Poisonous","amount":amount})
                        }
                        CombatTriggerKeyword::Frenzy(amount) => {
                            json!({"op":"Frenzy","amount":amount})
                        }
                        CombatTriggerKeyword::Flanking => json!({"op":"Flanking"}),
                    },
                    ObjectMutation::Ninjutsu => json!({"op":"Ninjutsu"}),
                    ObjectMutation::Unearth => json!({"op":"Unearth"}),
                    ObjectMutation::ResolveCounterKeyword(k) => {
                        json!({"op":"ResolveCounterKeyword","keyword":counter_keyword(*k)})
                    }
                    ObjectMutation::ResolveTransfer(k) => match k {
                        spellwake_engine::program::TransferKeyword::Graft(amount) => {
                            json!({"op":"Graft","amount":amount})
                        }
                        spellwake_engine::program::TransferKeyword::Modular(amount) => {
                            json!({"op":"Modular","amount":amount})
                        }
                        spellwake_engine::program::TransferKeyword::Soulshift(maximum) => {
                            json!({"op":"Soulshift","maximum":maximum})
                        }
                    },
                    ObjectMutation::KeywordZoneAction(KeywordZoneOp::ExileDefeatedSiege) => json!({"op":"ExileDefeatedSiege"}),
                    ObjectMutation::ResolveGrowth(k) => match k {
                        spellwake_engine::program::GrowthKeyword::Increment => {
                            json!({"op":"Increment"})
                        }
                        spellwake_engine::program::GrowthKeyword::Mentor => json!({"op":"Mentor"}),
                        spellwake_engine::program::GrowthKeyword::Evolve => json!({"op":"Evolve"}),
                        spellwake_engine::program::GrowthKeyword::Backup {
                            amount,
                            printed_below,
                        } => {
                            json!({"op":"Backup","amount":amount,"printed_below":keywords(printed_below)})
                        }
                    },
                    ObjectMutation::KeywordZoneAction(KeywordZoneOp::SacrificeEvoked) => {
                        json!({"op":"SacrificeEvoked"})
                    }
                    ObjectMutation::KeywordZoneAction(
                        KeywordZoneOp::SacrificeStateTriggerSource,
                    ) => {
                        json!({"op":"SacrificeStateTriggerSource"})
                    }
                    ObjectMutation::ResolvePowerAbility(kind) => {
                        json!({"op":"ResolvePowerAbility","kind":format!("{kind:?}")})
                    }
                    ObjectMutation::Myriad => json!({"op":"Myriad"}),
                    _ => json!({"op":"MutateObject","spec":format!("{spec:?}")}),
                }
            }
            Instruction::AdjustLife(spec) => {
                use spellwake_engine::{
                    life::LifeAdjustment as LA,
                    program::{ResolvingPlayer as RP, ValueExpr as VE},
                    toolbox::{LifeOp, PlayerSelector as PS},
                };
                if spec.bind_amount.is_none() {
                    match (&spec.players, &spec.op) {
                        (PS::Resolving(RP::Controller), LifeOp::Gain(VE::Literal(n))) => {
                            json!({"op":"AdjustControllerLife","adjustment":life_adjustment(&LA::Gain(n.clone()))})
                        }
                        (PS::Resolving(RP::Controller), LifeOp::Lose(VE::Literal(n))) => {
                            json!({"op":"AdjustControllerLife","adjustment":life_adjustment(&LA::Lose(n.clone()))})
                        }
                        (PS::Resolving(RP::Controller), LifeOp::Set(VE::Literal(n))) => {
                            json!({"op":"AdjustControllerLife","adjustment":life_adjustment(&LA::Set(n.clone()))})
                        }
                        (PS::Resolving(RP::TargetPlayer), LifeOp::Gain(VE::Literal(n))) => {
                            json!({"op":"AdjustTargetLife","adjustment":life_adjustment(&LA::Gain(n.clone()))})
                        }
                        (PS::Resolving(RP::TargetPlayer), LifeOp::Lose(VE::Literal(n))) => {
                            json!({"op":"AdjustTargetLife","adjustment":life_adjustment(&LA::Lose(n.clone()))})
                        }
                        (PS::Resolving(RP::TargetPlayer), LifeOp::Set(VE::Literal(n))) => {
                            json!({"op":"AdjustTargetLife","adjustment":life_adjustment(&LA::Set(n.clone()))})
                        }
                        (PS::Resolving(RP::Controller), LifeOp::Gain(expr)) => {
                            json!({"op":"GainControllerLife","amount":instruction_amount(expr)})
                        }
                        _ => json!({"op":"AdjustLife","spec":format!("{spec:?}")}),
                    }
                } else {
                    json!({"op":"AdjustLife","spec":format!("{spec:?}")})
                }
            }
            Instruction::DealDamage(spec) => {
                if matches!(spec.source, spellwake_engine::toolbox::DamageSourceQuery::EntrySource)
                    && matches!(spec.recipients, spellwake_engine::toolbox::DamageTargetQuery::Targets)
                    && !spec.exile_if_dies
                    && let spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(n),
                    ) = &spec.amount
                {
                    json!({"op":"Damage","amount":n})
                } else {
                    json!({"op":"DealDamage","spec":format!("{spec:?}")})
                }
            }
            Instruction::MutatePlayer(spec) => {
                use spellwake_engine::toolbox::{
                    LibraryTopAction, PlayerKeywordAction, PlayerMutation,
                };
                match (&spec.op, spec.resolving_player()) {
                    (
                        PlayerMutation::LibraryTop(LibraryTopAction::Scry(amount)),
                        Some(player),
                    ) => {
                        json!({"op":"Scry","player":format!("{player:?}"),"amount":instruction_amount(amount)})
                    }
                    (
                        PlayerMutation::LibraryTop(LibraryTopAction::Surveil(amount)),
                        Some(player),
                    ) => {
                        json!({"op":"Surveil","player":format!("{player:?}"),"amount":instruction_amount(amount)})
                    }
                    (
                        PlayerMutation::LibraryTop(LibraryTopAction::Mill(amount)),
                        Some(player),
                    ) => {
                        json!({"op":"Mill","player":format!("{player:?}"),"amount":instruction_amount(amount)})
                    }
                    (
                        PlayerMutation::OfferSourceCounters {
                            amount,
                            placed_destination,
                            declined_destination,
                        },
                        _,
                    ) => {
                        json!({"op":"OfferSourceCounters","amount":amount,"placed_destination":placed_destination,"declined_destination":declined_destination})
                    }
                    (PlayerMutation::RequireCondition(cond), _) => match cond {
                        spellwake_engine::program::InterveningCondition::ReturnSourceOnBattlefield => {
                            json!({"op":"RequireReturnSourceOnBattlefield"})
                        }
                        spellwake_engine::program::InterveningCondition::Monarch { opponent } => {
                            json!({"op":"RequireMonarch","opponent":opponent})
                        }
                        spellwake_engine::program::InterveningCondition::OwnCommander => {
                            json!({"op":"RequireOwnCommander"})
                        }
                        spellwake_engine::program::InterveningCondition::OpponentMoreLands => {
                            json!({"op":"RequireOpponentMoreLands"})
                        }
                        spellwake_engine::program::InterveningCondition::NoLoyaltyActivation => {
                            json!({"op":"RequireNoLoyaltyActivation"})
                        }
                        spellwake_engine::program::InterveningCondition::OpponentLifeLoss(amount) => {
                            json!({"op":"RequireOpponentLifeLoss","amount":amount})
                        }
                        spellwake_engine::program::InterveningCondition::SourceLoyalty => {
                            json!({"op":"RequireSourceLoyalty"})
                        }
                        spellwake_engine::program::InterveningCondition::SourceQuestCounters(amount) => {
                            json!({"op":"RequireSourceQuestCounters","amount":amount})
                        }
                        spellwake_engine::program::InterveningCondition::GreatestArtifact => {
                            json!({"op":"RequireGreatestArtifact"})
                        }
                        spellwake_engine::program::InterveningCondition::NoOtherControlledCreatures => {
                            json!({"op":"RequireNoOtherControlledCreatures"})
                        }
                        spellwake_engine::program::InterveningCondition::ControlledCreatureCount(count) => {
                            json!({"op":"RequireControlledCreatureCount","count":count})
                        }
                        spellwake_engine::program::InterveningCondition::CastFromGraveyard => {
                            json!({"op":"RequireCastFromGraveyard"})
                        }
                    },
                    (PlayerMutation::ExileOnePerPlayerUntilSourceLeaves, _) => {
                        json!({"op":"ExileOnePerPlayerUntilSourceLeaves"})
                    }
                    (PlayerMutation::SacrificeRandomUntilLand, _) => {
                        json!({"op":"SacrificeRandomUntilLand"})
                    }
                    (PlayerMutation::KeywordAction(PlayerKeywordAction::ExtortDrain), _) => json!({"op":"ExtortDrain"}),
                    (
                        PlayerMutation::ChooseRevealedHandCard {
                            minimum_mana_value,
                            nonland_only,
                            count,
                            action,
                            filter: _,
                        },
                        _,
                    ) => {
                        let mut wire = json!({"op":"ChooseRevealedHandCard","minimum_mana_value":minimum_mana_value,"exile":*action==spellwake_engine::program::ChosenHandAction::Exile});
                        if *nonland_only {
                            wire["nonland_only"] = json!(true);
                        }
                        if count.get() != 1 {
                            wire["count"] = json!(count.get());
                        }
                        wire
                    }
                    (PlayerMutation::LibraryTop(LibraryTopAction::MayRevealTop), _) => {
                        json!({"op":"MayRevealLibraryTop"})
                    }
                    _ => json!({"op":"MutatePlayer","spec":format!("{spec:?}")}),
                }
            }
            Instruction::ApplyContinuous(spec) => {
                if matches!(spec.selector, Some(spellwake_engine::toolbox::ObjectQuery::Targets))
                    && spec.condition.is_none()
                    && !spec.one_shot
                    && spec.as_emblem.is_none()
                    && let spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts) = &spec.payload
                {
                    json!({"op":"CreateContinuousEffect","parts":parts_spec(parts),"duration":duration(spec.duration)})
                } else if spec.selector.is_none()
                    && spec.condition.is_none()
                    && spec.as_emblem.is_none()
                    && let spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield { amount } = &spec.payload
                {
                    json!({"op":"CreatePreventionShield","amount":amount})
                } else if spec.condition.is_none()
                    && !spec.one_shot
                    && spec.as_emblem.is_none()
                    && let spellwake_engine::toolbox::ContinuousPayload::BecomeCopy { donor, exceptions } = &spec.payload
                {
                    json!({"op":"BecomeCopy","receiver":format!("{:?}", spec.selector),"donor":format!("{donor:?}"),"duration":format!("{:?}", spec.duration),"exceptions":format!("{exceptions:?}")})
                } else {
                    json!({"op":"ApplyContinuous","spec":format!("{spec:?}")})
                }
            }
            Instruction::PromptChoice(spec) => {
                if let spellwake_engine::toolbox::PromptDomain::Subtype {
                    kind: spellwake_engine::toolbox::SubtypeChoiceKind::Creature,
                    excluding,
                    legacy_slot: Some(into),
                } = &spec.domain
                {
                    json!({"op":"ChooseCreatureType","into":into.ordinal().to_string(),"excluding":excluding.iter().map(|value|projection::creature_type_id(*value)).collect::<Vec<_>>()})
                } else if let Some((
                    payer,
                    fixed,
                    extra_generic,
                    paid_destination,
                    declined_destination,
                )) = spec.as_mana_payment()
                {
                    if *extra_generic == ValueExpr::Literal(0.into()) {
                        json!({"op":"OfferManaPayment","payer":format!("{payer:?}"),"cost":cost(fixed),"paid_destination":paid_destination.to_string(),"declined_destination":declined_destination.to_string()})
                    } else {
                        json!({"op":"OfferDynamicManaPayment","payer":format!("{payer:?}"),"fixed":format!("{fixed:?}"),"extra_generic":instruction_amount(extra_generic),"paid":paid_destination,"declined":declined_destination})
                    }
                } else {
                    json!({"op":"PromptChoice","spec":format!("{spec:?}")})
                }
            }
            Instruction::BranchCondition { condition, destination } => {
                if let spellwake_engine::toolbox::ConditionExpr::Resolution(cond) = condition.as_ref() {
                    json!({"op":"BranchIfFalse","condition":resolution_condition(cond),"destination":destination.to_string()})
                } else {
                    json!({"op":"BranchCondition","condition":format!("{condition:?}"),"destination":destination.to_string()})
                }
            }
            Instruction::ForEach { query, bind_item, .. } => json!({"op":"ForEach","query":format!("{query:?}"),"bind_item":bind_item.0}),
            Instruction::ForEachPlayerQuery { selector, bind_player, .. } => json!({"op":"ForEachPlayerQuery","selector":format!("{selector:?}"),"bind_player":bind_player.0}),
            Instruction::BindEnv { binding, value } => json!({"op":"BindEnv","binding":binding.0,"value":format!("{value:?}")}),
            Instruction::SetObjectTag { query, key, value } => json!({"op":"SetObjectTag","query":format!("{query:?}"),"key":format!("{key:?}"),"value":format!("{value:?}")}),
            Instruction::UpdateStateTrack { key, op } => json!({"op":"UpdateStateTrack","key":format!("{key:?}"),"op":format!("{op:?}")}),
            Instruction::SideDeckOp(spec) => json!({"op":"SideDeckOp","spec":format!("{spec:?}")}),
            Instruction::StackOp(spec) => {
                use spellwake_engine::toolbox::{CounterDestination, StackOpKind, StackOpSpec};
                if **spec == StackOpSpec::counter_targeted(CounterDestination::Graveyard) {
                    json!({"op":"CounterSpell"})
                } else if **spec == StackOpSpec::copy_targeted_spell() {
                    json!({"op":"CopySpell"})
                } else if **spec == StackOpSpec::copy_targeted_ability() {
                    json!({"op":"CopyAbility"})
                } else if **spec == StackOpSpec::retarget(true) {
                    json!({"op":"ChangeOnlyTarget"})
                } else if **spec == StackOpSpec::retarget(false) {
                    json!({"op":"ChooseNewTargets"})
                } else if **spec == StackOpSpec::gain_control() {
                    json!({"op":"GainControlOfSpell"})
                } else if **spec == StackOpSpec::counter_ward_source() {
                    json!({"op":"CounterWardSource"})
                } else {
                    match &spec.op {
                        StackOpKind::Cascade => json!({"op":"Cascade"}),
                        StackOpKind::CascadeBottom => json!({"op":"CascadeBottom"}),
                        StackOpKind::Encore => json!({"op":"Encore"}),
                        StackOpKind::CastDefeatedSiege => json!({"op":"CastDefeatedSiege"}),
                        StackOpKind::CopyOnCast(keyword) => match keyword {
                            spellwake_engine::program::CastCopyKeyword::CopyThisPermanentSpell => {
                                json!({"op":"CopyOnCast","keyword":"CopyThisPermanentSpell"})
                            }
                            spellwake_engine::program::CastCopyKeyword::CopyObservedSpell => {
                                json!({"op":"CopyOnCast","keyword":"CopyObservedSpell"})
                            }
                            spellwake_engine::program::CastCopyKeyword::OfferCopyToOtherPlayers => {
                                json!({"op":"CopyOnCast","keyword":"OfferCopyToOtherPlayers"})
                            }
                            spellwake_engine::program::CastCopyKeyword::Storm => {
                                json!({"op":"CopyOnCast","keyword":"Storm"})
                            }
                            spellwake_engine::program::CastCopyKeyword::Gravestorm => {
                                json!({"op":"CopyOnCast","keyword":"Gravestorm"})
                            }
                            spellwake_engine::program::CastCopyKeyword::Replicate(i) => {
                                json!({"op":"CopyOnCast","keyword":"Replicate","replicate":i})
                            }
                        },
                        _ => json!({"op":"StackOp","spec":format!("{spec:?}")}),
                    }
                }
            }
            Instruction::CreateTokens { creations } =>
                json!({"op":"CreateTokens","creations":creations.iter().map(token_creation).collect::<Vec<_>>()}),
            Instruction::SearchLibrary { filter } => {
                use spellwake_engine::program::{
                    SearchBattlefieldPostEntry, SearchCardMatcher, SearchCount, SearchDestination,
                    SearchManaValue, SearchPairConstraint, SearchSource,
                };
                let matcher = match &filter.matcher {
                    SearchCardMatcher::Any => json!({"tag":"Any"}),
                    SearchCardMatcher::Nonland => json!({"tag":"Nonland"}),
                    SearchCardMatcher::BasicLand => json!({"tag":"BasicLand"}),
                    SearchCardMatcher::LandSubtypes {
                        subtypes,
                        require_land_type,
                    } => json!({
                        "tag":"LandSubtypes",
                        "kinds": subtypes.iter().map(|k| k.wire_label()).collect::<Vec<_>>(),
                        "require_land_type": require_land_type
                    }),
                    SearchCardMatcher::CardTypes(kinds) => json!({
                        "tag":"CardTypes",
                        "kinds": kinds.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>()
                    }),
                    SearchCardMatcher::CreatureSubtype(kind) => json!({
                        "tag":"CreatureSubtype",
                        "kind": projection::creature_type_id(*kind)
                    }),
                    SearchCardMatcher::Equipment => json!({"tag":"Equipment"}),
                    SearchCardMatcher::CurseForHost(host) => json!({
                        "tag":"CurseForHost",
                        "host": format!("{host:?}")
                    }),
                    SearchCardMatcher::AnyOfNames(names) => json!({
                        "tag":"AnyOfNames",
                        "names": names.iter().map(|n| n.as_str()).collect::<Vec<_>>()
                    }),
                    SearchCardMatcher::SameNameAs { reference, extra } => json!({
                        "tag":"SameNameAs",
                        "reference": format!("{reference:?}"),
                        "extra": extra.as_deref().map(|f| permanent_group_wire(*f))
                    }),
                    SearchCardMatcher::Filtered(obj_filter) => json!({
                        "tag":"Filtered",
                        "filter": permanent_group_wire(**obj_filter)
                    }),
                };
                let mana_value = filter.mana_value.as_ref().map(|mv| match mv {
                    SearchManaValue::Exact(value) => json!({"tag":"Exact","value":value}),
                    SearchManaValue::AtLeast(minimum) => json!({"tag":"AtLeast","minimum":minimum}),
                    SearchManaValue::AtMost(maximum) => json!({"tag":"AtMost","maximum":maximum}),
                    SearchManaValue::AtMostAnnouncedX => json!({"tag":"AtMostAnnouncedX"}),
                    SearchManaValue::EqualToSourceCounters(kind) => {
                        json!({"tag":"EqualToSourceCounters","kind":format!("{kind:?}")})
                    }
                    SearchManaValue::ExactManaCostOneOf(costs) => {
                        json!({"tag":"ExactManaCostOneOf","costs":costs.iter().map(cost).collect::<Vec<_>>()})
                    }
                });
                let count = match &filter.count {
                    SearchCount::UpTo(n) => json!({"tag":"UpTo","count":n}),
                    SearchCount::UpToExpr(expr) => json!({"tag":"UpToExpr","expr":instruction_amount(expr)}),
                    SearchCount::Exact(n) => json!({"tag":"Exact","count":n.get()}),
                    SearchCount::UpToPlayersWithMoreLands => {
                        json!({"tag":"UpToPlayersWithMoreLands"})
                    }
                };
                let pair_constraint = filter.pair_constraint.map(|c| match c {
                    SearchPairConstraint::ShareLandType => json!({"tag":"ShareLandType"}),
                    SearchPairConstraint::DistinctLandTypes(first, second) => json!({
                        "tag":"DistinctLandTypes",
                        "first": first.wire_label(),
                        "second": second.wire_label()
                    }),
                });
                let destination = match filter.destination {
                    SearchDestination::Hand => json!({"tag":"Hand"}),
                    SearchDestination::LibraryTop => json!({"tag":"LibraryTop"}),
                    SearchDestination::SplitBattlefieldTappedAndHand => {
                        json!({"tag":"SplitBattlefieldTappedAndHand"})
                    }
                    SearchDestination::HandOrBattlefieldIfCreatureDied => {
                        json!({"tag":"HandOrBattlefieldIfCreatureDied"})
                    }
                    SearchDestination::SplitHandAndGraveyard => {
                        json!({"tag":"SplitHandAndGraveyard"})
                    }
                    SearchDestination::Battlefield { tapped, post_entry } => match post_entry {
                        SearchBattlefieldPostEntry::AttachToCurseHost(host) => json!({
                            "tag":"Battlefield",
                            "tapped": tapped,
                            "post_entry": "AttachToCurseHost",
                            "host": format!("{host:?}")
                        }),
                        other => json!({
                            "tag":"Battlefield",
                            "tapped": tapped,
                            "post_entry": format!("{other:?}")
                        }),
                    },
                    SearchDestination::Exile { link } => json!({
                        "tag":"Exile",
                        "link": format!("{link:?}")
                    }),
                    SearchDestination::Graveyard => json!({"tag":"Graveyard"}),
                };
                let source = match filter.source {
                    SearchSource::OwnLibrary => "OwnLibrary".to_owned(),
                    SearchSource::OwnLibraryOrGraveyard => "OwnLibraryOrGraveyard".to_owned(),
                    SearchSource::TargetOpponentLibrary => "TargetOpponentLibrary".to_owned(),
                    SearchSource::TargetPlayerLibrary => "TargetPlayerLibrary".to_owned(),
                    SearchSource::PlayerLibrary(seat) => format!("PlayerLibrary({})", seat.0),
                };
                json!({
                    "op":"SearchLibrary",
                    "filter":{
                        "searcher": format!("{:?}", filter.searcher),
                        "source": source,
                        "matcher": matcher,
                        "mana_value": mana_value,
                        "count": count,
                        "pair_constraint": pair_constraint,
                        "destination": destination,
                        "reveal": filter.reveal
                    }
                })
            }
            Instruction::FixedTargetClause(clause)=>json!({"op":"FixedTargetClause","body":body(clause)}),
            Instruction::RepeatLoop { count, body } => {
                json!({"op":"RepeatLoop","count":instruction_amount(count),"body":format!("{body:?}")})
            }
            Instruction::ScheduleDelayedStepTrigger {
                step,
                own_step_only,
                body: delayed,
            } => {
                json!({"op":"ScheduleDelayedStepTrigger","step":format!("{step:?}"),"own_step_only":own_step_only,"body":body(delayed)})
            }
            Instruction::EmitReflexiveTrigger { body: reflexive } => {
                json!({"op":"EmitReflexiveTrigger","body":body(reflexive)})
            }
            Instruction::ScheduleDelayedEventTrigger { trigger, body: inner, expires, once } => {
                json!({"op":"ScheduleDelayedEventTrigger","trigger":format!("{trigger:?}"),"body":body(inner),"expires":format!("{expires:?}"),"once":once})
            }
        })
        .collect::<Vec<_>>();
    let mut result = json!({"target":target,"instructions":instructions});
    if let Some(modes) = value.triggered_modes() {
        result["triggered_modes"] = json!({
            "minimum": modes.minimum, "maximum": modes.maximum, "distinct_targets": modes.distinct_targets,
            "options": modes.options.iter().map(body).collect::<Vec<_>>(),
        });
    }
    result
}
pub fn condition(value: &TriggerSpec) -> Value {
    match (&value.event, value.zone, value.intervening_if.as_ref()) {
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::Scried,
            },
            _,
            _,
        ) => {
            json!({"tag":"Scried","player":format!("{player:?}")})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::Surveilled,
            },
            _,
            _,
        ) => {
            json!({"tag":"Surveilled","player":format!("{player:?}")})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BecomesBlocked {
                        capture_defender: false,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfBecomesBlocked"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::HighestLifeDefender,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfAttacksHighestLife"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::GroupSubtype(subtype),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControlledSubtypeAttacks","subtype":projection::creature_type_id(*subtype)})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::GroupTotalPowerAtLeast(minimum),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControllerAttacksWithTotalPower","minimum":minimum})
        }
        (
            TriggerEvent::CountersChanged {
                kind:
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: true,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControllerPlacesCounterOnAnotherColorlessCreature"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::WithStrongerCreature,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfAttacksWithStrongerCreature"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            Some(TriggerCondition::UnrenownedSource),
        ) if *source == ObjectFilter::self_object() => {
            json!({"tag":"UnrenownedSelfDealsCombatDamageToPlayer"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BecomesBlocked {
                        capture_defender: true,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfBecomesBlockedCaptureDefender"})
        }
        (
            TriggerEvent::DealsDamage {
                source: ObjectFilter::Clause(c),
                recipient: DamageTriggerRecipient::Controller,
                combat_only: false,
            },
            _,
            _,
        ) if let SubtypeFilter::Has(subtype) = c.subtypes => {
            json!({"tag":"SubtypeDamagedController","subtype":projection::creature_type_id(subtype)})
        }
        (
            TriggerEvent::Dies {
                subject: ObjectFilter::Clause(c),
                mode: DiesTriggerMode::Permanent,
            },
            _,
            _,
        ) if let SubtypeFilter::Has(subtype) = c.subtypes => {
            json!({"tag":"ControlledSubtypeDies","subtype":projection::creature_type_id(subtype)})
        }
        (
            TriggerEvent::CountersChanged {
                subject: ObjectFilter::Clause(c),
                kind:
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: false,
                    },
            },
            _,
            _,
        ) if let SubtypeFilter::Lacks(subtype) = c.subtypes => {
            json!({"tag":"CountersOnOtherControlledCreatureExceptSubtype","subtype":projection::creature_type_id(subtype)})
        }
        (
            TriggerEvent::Dies {
                subject: ObjectFilter::Clause(c),
                mode: DiesTriggerMode::Permanent,
            },
            _,
            _,
        ) if c.controller == ControllerFilter::You && !c.any_types.is_empty() => {
            let types = mask_card_types(c.any_types);
            json!({"tag":"ControlledPermanentTypeDies","kind":format!("{:?}", types[0])})
        }
        (
            TriggerEvent::DealsDamage {
                source: ObjectFilter::Clause(c),
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            _,
        ) if let SubtypeFilter::Has(subtype) = c.subtypes => {
            if c.controller == ControllerFilter::You {
                json!({"tag":"ControlledSubtypeDealsCombatDamageToPlayer","subtype":projection::creature_type_id(subtype)})
            } else {
                json!({"tag":"SubtypeDealsCombatDamageToPlayer","subtype":projection::creature_type_id(subtype)})
            }
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            None,
        ) if *source == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureDealsCombatDamageToPlayer"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::Opponent,
                combat_only: false,
            },
            _,
            _,
        ) if *source == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureDealsDamageToOpponent"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                subject,
                kind: BlockTriggerKind::Blocks,
            },
            _,
            _,
        ) if *subject == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureBlocks"})
        }
        (
            TriggerEvent::DealsDamage {
                recipient: DamageTriggerRecipient::EnchantedPlayer,
                combat_only: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"CombatDamageToEnchantedPlayer"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::Opponent,
                combat_only: false,
            },
            _,
            _,
        ) if *source == ObjectFilter::self_object() => {
            json!({"tag":"SelfDealsDamageToOpponent"})
        }
        (
            TriggerEvent::DealtDamage {
                linked_player: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"DamageToLinkedPlayer"})
        }
        (
            TriggerEvent::DealtDamage {
                subject,
                linked_player: false,
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"SelfDealtDamage"})
        }
        (
            TriggerEvent::CountersChanged {
                kind: CounterTriggerKind::SagaChapter(number),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SagaChapter","number":number})
        }
        (
            TriggerEvent::DealsDamage {
                recipient: DamageTriggerRecipient::Creature,
                combat_only: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfDealsCombatDamageToCreature"})
        }
        (
            TriggerEvent::DealsDamage {
                recipient: DamageTriggerRecipient::PlayerOrPlaneswalker,
                combat_only: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfDealsCombatDamageToPlayerOrPlaneswalker"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            None,
        ) if *source == ObjectFilter::creature().controlled() => {
            json!({"tag":"ControlledCreatureDealsCombatDamageToPlayer"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            Some(TriggerCondition::NoOtherControlledCreatures),
        ) if *source == ObjectFilter::self_object() => {
            json!({"tag":"SelfDealsCombatDamageToPlayerWithoutOtherCreatures"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            Some(TriggerCondition::DefendingPlayerHasMoreLands),
        ) if *source == ObjectFilter::self_object() => {
            json!({"tag":"SelfDealsCombatDamageToPlayerWithMoreLands"})
        }
        (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::AnyPlayer,
                combat_only: true,
            },
            _,
            None,
        ) if *source == ObjectFilter::self_object() => {
            json!({"tag":"SelfDealsCombatDamageToPlayer"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind: BlockTriggerKind::Unblocked,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfAttacksUnblocked"})
        }
        (
            TriggerEvent::Dies {
                mode: DiesTriggerMode::Blitz,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"BlitzDies"})
        }
        (
            TriggerEvent::Enters {
                cast_origin: Some(CastEntryKind::Cast { from_hand }),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"EnteredIfCast","from_hand":from_hand})
        }
        (
            TriggerEvent::Enters {
                cast_origin: Some(CastEntryKind::Kicked),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"EnteredKicked"})
        }
        (
            TriggerEvent::Enters {
                cast_origin: Some(CastEntryKind::Evoked),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"EnteredEvoked"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep,
                player: TriggerPlayerScope::EnchantedPlayer,
            },
            _,
            _,
        ) => {
            json!({"tag":"EnchantedPlayerUpkeep"})
        }
        (
            TriggerEvent::StatusChanged {
                kind: StatusTriggerKind::Transforms,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfTransforms"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            _,
            Some(TriggerCondition::PreviousTurnCasts(cond)),
        ) => {
            json!({"tag":"PreviousTurnUpkeep","condition":format!("{cond:?}")})
        }
        (TriggerEvent::Enters { .. }, _, Some(TriggerCondition::Morbid)) => {
            json!({"tag":"MorbidEnters"})
        }
        (TriggerEvent::StepBegan { step, .. }, _, Some(TriggerCondition::Morbid)) => {
            json!({"tag":"MorbidStep","step":format!("{step:?}")})
        }
        (
            TriggerEvent::StatusChanged {
                kind: StatusTriggerKind::TurnsFaceUp,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfTurnsFaceUp"})
        }
        (TriggerEvent::StepBegan { step: at, .. }, _, Some(TriggerCondition::ControlsMeldPair)) => {
            json!({"tag":"OwnMeldPairAtStep","at":format!("{at:?}")})
        }
        (TriggerEvent::Keyword(TriggerKeywordEvent::Soulbond { entrant_only }), _, _) => {
            json!({"tag":"Soulbond","entrant_only":entrant_only})
        }
        (
            TriggerEvent::StatusChanged {
                kind: StatusTriggerKind::Mutated,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfMutated"})
        }
        (
            TriggerEvent::StatusChanged {
                kind: StatusTriggerKind::Phased { out },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfPhased","out":out})
        }
        (
            TriggerEvent::DealtDamage {
                subject,
                linked_player: false,
            },
            _,
            _,
        ) if *subject == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureDealtDamage"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureAttacks"})
        }
        (
            TriggerEvent::Enters {
                attached_to_creature: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfEntersAttachedToCreature"})
        }
        (
            TriggerEvent::Enters {
                from_zone: TriggerFromZone::ControllerGraveyard,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"CreatureEntersFromControllerGraveyard"})
        }
        (
            TriggerEvent::Enters {
                from_zone: TriggerFromZone::Graveyard,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfEntersFromGraveyard"})
        }
        (
            TriggerEvent::Enters { .. },
            _,
            Some(TriggerCondition::DuringStep(Step::DeclareAttackers)),
        ) => {
            json!({"tag":"SelfEntersDuringDeclareAttackers"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::FirstThisTurn { highest_life },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfFirstAttack","highest_life":highest_life})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::WithAtLeastOthers(minimum),
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfAttacksWithAtLeastOthers","minimum":minimum})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"SelfAttacksCaptureDefender"})
        }
        (TriggerEvent::Attacks { subject, .. }, _, Some(TriggerCondition::SourceSaddled))
            if *subject == ObjectFilter::self_object() =>
        {
            json!({"tag":"SelfAttacksWhileSaddled"})
        }
        (TriggerEvent::AnyOf(events), _, _) => match events.as_slice() {
            [
                TriggerEvent::Attacks { .. },
                TriggerEvent::BlocksOrBlocked {
                    kind: BlockTriggerKind::Blocks,
                    ..
                },
            ] => {
                json!({"tag":"SelfAttacksOrBlocks"})
            }
            [
                TriggerEvent::Enters { .. },
                TriggerEvent::StatusChanged {
                    kind: StatusTriggerKind::Transforms,
                    ..
                },
            ] => {
                json!({"tag":"SelfEntersOrTransforms"})
            }
            [TriggerEvent::Enters { .. }, TriggerEvent::Attacks { .. }] => {
                json!({"tag":"SelfEntersOrAttacks"})
            }
            [
                TriggerEvent::Enters { .. },
                TriggerEvent::SpellCast {
                    min_mana_value: Some(minimum),
                    ..
                },
            ] => {
                json!({"tag":"EntersOrControllerCastsAtLeast","minimum":minimum})
            }
            _ => json!({"tag":"AnyOf","debug":format!("{value:?}")}),
        },
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"SelfAttacks"})
        }
        (
            TriggerEvent::PutIntoGraveyard {
                player: PlayerRelation::Controller,
                from_library: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"CreatureCardsLibraryToGraveyard"})
        }
        (
            TriggerEvent::PutIntoGraveyard {
                player,
                from_library: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"CreatureCardsLibraryToGraveyard","player":format!("{player:?}")})
        }
        (
            TriggerEvent::PutIntoGraveyard {
                own_turn_only: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"OwnCreatureCardsPutIntoGraveyardOnOwnTurn"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind: BlockTriggerKind::BlocksOrBecomesBlocked,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfBlocksOrBecomesBlocked"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BlockedByEach {
                        without_flanking_only: false,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"BlockedByCreature"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BlocksOrBlockedByEach {
                        required_subtype: Some(subtype),
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"AttachedCreatureBlocksOrBlockedBySubtype","subtype":projection::creature_type_id(*subtype)})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BlocksOrBlockedByEach {
                        required_subtype: None,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"BlocksOrBlockedByCreature"})
        }
        (
            TriggerEvent::BlocksOrBlocked {
                kind:
                    BlockTriggerKind::BlockedByEach {
                        without_flanking_only: true,
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"BlockedByCreatureWithoutFlanking"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
            },
            _,
            _,
        ) if *subject == ObjectFilter::creature().controlled() => {
            json!({"tag":"ControlledCreatureAttacksCaptureDefender"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject == ObjectFilter::creature().controlled() => {
            json!({"tag":"ControlledCreatureAttacks"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject
            == ObjectFilter::creature()
                .controlled()
                .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER) =>
        {
            json!({"tag":"ControlledCreatureWithPlusOneCounterAttacks"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject
            == ObjectFilter::creature()
                .controlled()
                .other_than_ability_source() =>
        {
            json!({"tag":"AnotherControlledCreatureAttacks"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::AtControllerOrPlaneswalker,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"AttacksYouOrYourPlaneswalker"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::AtOpponentOrPlaneswalker,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"AttacksAnOpponentOrTheirPlaneswalker"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::OpponentsAtController { minimum: 2 },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"AttackAtLeastTwoAtController"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::OpponentsAtController { minimum: 1 },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"OpponentAttacksController"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::EquippedAtPlayer,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControllerAttacksPlayerWithEquippedCreatures"})
        }
        (
            TriggerEvent::Attacks {
                subject,
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            None,
        ) if *subject == ObjectFilter::any().controlled().commander() => {
            json!({"tag":"ControlledCommanderAttacks"})
        }
        (
            TriggerEvent::Attacks {
                mode: AttackTriggerMode::Alone,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"AttacksAlone"})
        }
        (
            TriggerEvent::Attacks {
                subject: ObjectFilter::Clause(c),
                mode:
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
            },
            _,
            _,
        ) if c.token == Some(true)
            && let SubtypeFilter::Has(subtype) = c.subtypes
            && let PowerFilter::AtLeast(minimum_power) = c.power =>
        {
            json!({"tag":"ControlledTokenAttacks","subtype":projection::creature_type_id(subtype),"minimum_power":minimum_power})
        }
        (
            TriggerEvent::PlayerAction {
                action: PlayerTriggerAction::Cycled { self_only: true },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"SelfCycled"})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::Cycled { self_only: false },
            },
            _,
            _,
        ) => {
            json!({"tag":"Cycled","player":format!("{player:?}")})
        }
        (
            TriggerEvent::PlayerAction {
                action:
                    PlayerTriggerAction::DrewCard {
                        nth_each_turn: Some(ordinal),
                    },
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"DrewOwnNthCard","ordinal":ordinal.get()})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action:
                    PlayerTriggerAction::DrewCard {
                        nth_each_turn: None,
                    },
            },
            _,
            _,
        ) => {
            json!({"tag":"DrewCard","player":format!("{player:?}")})
        }
        (
            TriggerEvent::PlayerAction {
                action: PlayerTriggerAction::ArtifactTappedForMana,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"OpponentArtifactTappedForMana"})
        }
        (TriggerEvent::Keyword(TriggerKeywordEvent::Counter(k)), _, _) => {
            json!({"tag":"CounterKeyword","keyword":counter_keyword(*k)})
        }
        (TriggerEvent::Keyword(TriggerKeywordEvent::Graft), _, _) => json!({"tag":"Graft"}),
        (TriggerEvent::Keyword(TriggerKeywordEvent::Increment), _, _) => json!({"tag":"Increment"}),
        (TriggerEvent::Keyword(TriggerKeywordEvent::Evolve), _, _) => json!({"tag":"Evolve"}),
        (TriggerEvent::Keyword(TriggerKeywordEvent::Upkeep(kind)), _, _) => {
            json!({"tag":match kind {
                spellwake_engine::program::UpkeepKeyword::Echo => "EchoUpkeep",
                spellwake_engine::program::UpkeepKeyword::CumulativeUpkeep => "CumulativeUpkeep",
                spellwake_engine::program::UpkeepKeyword::OtherCreaturesUpkeep => "OtherCreaturesUpkeep",
            }})
        }
        (
            TriggerEvent::Targeted {
                instant_or_sorcery_only: true,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControlledCreatureTargetedByInstantOrSorcery"})
        }
        (
            TriggerEvent::Targeted {
                subject,
                by_opponent: false,
                spell_only: true,
                instant_or_sorcery_only: false,
            },
            _,
            _,
        ) if *subject == ObjectFilter::any().attached_to_self() => {
            json!({"tag":"AttachedCreatureTargetedBySpell"})
        }
        (
            TriggerEvent::Targeted {
                subject,
                by_opponent: false,
                spell_only: true,
                instant_or_sorcery_only: false,
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"TargetedBySpell"})
        }
        (
            TriggerEvent::Targeted {
                subject,
                by_opponent: true,
                spell_only: false,
                ..
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"WardTargeted"})
        }
        (
            TriggerEvent::Targeted {
                subject: ObjectFilter::Clause(c),
                by_opponent: true,
                spell_only: false,
                ..
            },
            _,
            _,
        ) if let SubtypeFilter::Has(subtype) = c.subtypes => {
            json!({"tag":"ControlledSubtypeTargeted","subtype":projection::creature_type_id(subtype)})
        }
        (
            TriggerEvent::StateThreshold(StateThresholdKind::ControlledSubtype {
                subtype,
                minimum,
            }),
            _,
            _,
        ) => {
            json!({"tag":"ControlledSubtypeThreshold","subtype":projection::creature_type_id(*subtype),"minimum":minimum})
        }
        (
            TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                kind: spellwake_engine::effects::CounterKind::Page,
                comparison: Comparison::GreaterOrEqual,
                amount,
            }),
            _,
            _,
        ) => json!({"tag":"PageCountersAtLeast","amount":amount}),
        (
            TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                kind,
                comparison,
                amount,
            }),
            _,
            _,
        ) => {
            json!({"tag":"SourceCounterThreshold","kind":format!("{kind:?}"),"comparison":format!("{comparison:?}"),"amount":amount})
        }
        (
            TriggerEvent::SpellCast {
                self_cast: true, ..
            },
            _,
            Some(TriggerCondition::HandSizeBelow(cards)),
        ) => {
            json!({"tag":"SelfCastBelowHandSize","cards":cards})
        }
        (
            TriggerEvent::SpellCast {
                self_cast: true, ..
            },
            _,
            None,
        ) => json!({"tag":"SelfCast"}),
        (TriggerEvent::PlayerLosesGame(relation), _, _) => {
            json!({"tag":"PlayerLosesGame","player":format!("{relation:?}")})
        }
        (
            TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::EnchantedPlayer,
                filter,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"EnchantedPlayerCasts","filter":format!("{filter:?}")})
        }
        (
            TriggerEvent::SpellCast {
                include_copy: true, ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControllerCastsOrCopiesInstantOrSorcery"})
        }
        (
            TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                filter,
                self_cast: false,
                ..
            },
            TriggerZone::Graveyard,
            _,
        ) if *filter == SpellFilter::of_type(CardType::Planeswalker) => {
            json!({"tag":"GraveyardOwnerCastsPlaneswalker"})
        }
        (
            TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                filter,
                self_cast: false,
                ..
            },
            TriggerZone::Battlefield,
            _,
        ) if *filter == SpellFilter::instant_or_sorcery().targeting_only_source() => {
            json!({"tag":"ControllerCastsSpellTargetingOnlySource"})
        }
        (
            TriggerEvent::SpellCast {
                caster: TriggerPlayerScope::Relation(player),
                filter,
                self_cast: false,
                ..
            },
            _,
            _,
        ) => {
            let filter = *filter;
            let filter_json = if filter == SpellFilter::any().from_graveyard() {
                json!({"tag":"FromGraveyard"})
            } else if filter == SpellFilter::any() {
                json!({"tag":"AnySpell"})
            } else if filter == SpellFilter::noncreature() {
                json!({"tag":"Noncreature"})
            } else if filter == SpellFilter::instant_or_sorcery().from_own_library() {
                json!({"tag":"InstantOrSorceryFromOwnLibrary"})
            } else if let ObjectFilter::Either(_, b) = filter.object
                && let SubtypeFilter::Has(kind) = b.subtypes
                && filter == SpellFilter::instant_sorcery_or_subtype(kind)
            {
                json!({"tag":"InstantSorceryOrSubtype","subtype":projection::creature_type_id(kind)})
            } else if filter == SpellFilter::creature().with_colorless() {
                json!({"tag":"ColorlessCreature"})
            } else if filter == SpellFilter::colorless() {
                json!({"tag":"Colorless"})
            } else if filter == SpellFilter::instant_or_sorcery().above_experience() {
                json!({"tag":"InstantOrSorceryAboveExperience"})
            } else if let Some(ordinal) = filter.ordinal_this_turn
                && filter == SpellFilter::any().ordinal_this_turn(ordinal)
            {
                json!({"tag":"OrdinalThisTurn","ordinal":ordinal.get()})
            } else if filter == SpellFilter::creature().of_chosen_entry_type() {
                json!({"tag":"CreatureOfChosenEntryType"})
            } else if filter == SpellFilter::instant_or_sorcery() {
                json!({"tag":"InstantOrSorcery"})
            } else if filter == SpellFilter::historic() {
                json!({"tag":"Historic"})
            } else if let ObjectFilter::Clause(c) = filter.object {
                let types = mask_card_types(c.any_types);
                if types.len() == 1 && filter == SpellFilter::of_type(types[0]) {
                    json!({"tag":"HasType","card_type":format!("{:?}", types[0])})
                } else {
                    json!({"tag":"SpellFilter","debug":format!("{filter:?}")})
                }
            } else {
                json!({"tag":"SpellFilter","debug":format!("{filter:?}")})
            };
            json!({"tag":"SpellCast","player":format!("{player:?}"),"filter":filter_json})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::TokenCreated,
            },
            _,
            _,
        ) => {
            json!({"tag":"TokenCreated","player":format!("{player:?}")})
        }
        (TriggerEvent::LifeChanged { kind, player }, _, _) => {
            json!({"tag":"LifeChanged","kind":format!("{kind:?}"),"player":format!("{player:?}")})
        }
        (
            TriggerEvent::Enters {
                subject: ObjectFilter::Clause(c),
                ..
            },
            _,
            Some(TriggerCondition::SourceHasLoyalty),
        ) => {
            let types = mask_card_types(c.any_types);
            json!({"tag":"EntersWhileSourceHasLoyalty","kind":format!("{:?}", types[0])})
        }
        (
            TriggerEvent::Enters {
                subject,
                from_zone: TriggerFromZone::Any,
                cast_origin: None,
                attached_to_creature: false,
            },
            _,
            None,
        ) => {
            let object = *subject;
            if let ObjectFilter::Clause(c) = object
                && let SubtypeFilter::Has(subtype) = c.subtypes
                && c.controller == ControllerFilter::You
                && c.any_types == CardTypeMask::EMPTY
            {
                let other = c.source_relation == SourceRelation::OtherThanAbilitySource;
                json!({"tag":"ControlledSubtypeEnters","subtype":projection::creature_type_id(subtype),"other":other})
            } else {
                json!({"tag":"Enters","object":object_relation_label(object)})
            }
        }
        (TriggerEvent::LeavesBattlefield { subject }, _, _) => {
            json!({"tag":"LeavesBattlefield","object":object_relation_label(*subject)})
        }
        (TriggerEvent::Exiled { subject, from_zone }, _, _) => {
            json!({"tag":"Exiled","object":object_relation_label(*subject),"from_zone":format!("{from_zone:?}")})
        }
        (TriggerEvent::ReturnedToHand { subject }, _, _) => {
            json!({"tag":"ReturnedToHand","object":object_relation_label(*subject)})
        }
        (TriggerEvent::LeavesGraveyard { subject, player }, _, _) => {
            json!({"tag":"LeavesGraveyard","object":object_relation_label(*subject),"player":format!("{player:?}")})
        }
        (
            TriggerEvent::CountersChanged {
                kind: CounterTriggerKind::LastDefenseRemoved,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"LastDefenseRemoved"})
        }
        (
            TriggerEvent::Dies {
                mode: DiesTriggerMode::DamagedBySource,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"CreatureDamagedBySourceDies"})
        }
        (
            TriggerEvent::Dies {
                subject,
                mode: DiesTriggerMode::Permanent,
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"SourcePutIntoGraveyardFromBattlefield"})
        }
        (
            TriggerEvent::Dies {
                subject: ObjectFilter::Clause(c),
                mode: DiesTriggerMode::ToControllerGraveyard,
            },
            _,
            _,
        ) if let PowerFilter::AtMost(power) = c.power => {
            json!({"tag":"CreaturePowerAtMostToOwnGraveyard","power":power as i16})
        }
        (
            TriggerEvent::Dies {
                subject,
                mode: DiesTriggerMode::Creature,
            },
            _,
            _,
        ) => {
            let object = *subject;
            if let ObjectFilter::Clause(c) = object
                && let SubtypeFilter::Has(subtype) = c.subtypes
                && object
                    == ObjectFilter::creature()
                        .controlled()
                        .nontoken()
                        .with_subtype(subtype)
            {
                json!({"tag":"ControlledNontokenSubtypeDies","subtype":projection::creature_type_id(subtype)})
            } else {
                json!({"tag":"Dies","object":object_relation_label(object)})
            }
        }
        (
            TriggerEvent::CountersChanged {
                subject,
                kind:
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: false,
                    },
            },
            _,
            _,
        ) if *subject == ObjectFilter::self_object() => {
            json!({"tag":"PlusOneCountersOnSource"})
        }
        (
            TriggerEvent::StatusChanged {
                subject,
                kind: StatusTriggerKind::BecomesTapped,
            },
            _,
            _,
        ) => {
            json!({"tag":"BecomesTapped","object":object_relation_label(*subject)})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::Sacrificed(filter),
            },
            _,
            _,
        ) => {
            let filter = *filter;
            if filter == ObjectFilter::any() {
                json!({"tag":"Sacrificed","player":format!("{player:?}")})
            } else if filter == ObjectFilter::any().nontoken() {
                json!({"tag":"SacrificedNontoken","player":format!("{player:?}")})
            } else if let ObjectFilter::Clause(c) = filter {
                let types = mask_card_types(c.any_types);
                json!({"tag":"SacrificedType","player":format!("{player:?}"),"kind":format!("{:?}", types[0])})
            } else {
                json!({"tag":"Sacrificed","player":format!("{player:?}")})
            }
        }
        (
            TriggerEvent::PlayerAction {
                action: PlayerTriggerAction::SagaFinalChapterResolved,
                ..
            },
            _,
            _,
        ) => {
            json!({"tag":"ControlledSagaFinalChapterResolved"})
        }
        (
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerAction::Discarded,
            },
            _,
            _,
        ) => {
            json!({"tag":"Discarded","player":format!("{player:?}")})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::ControllerHasCityBlessing),
        ) => {
            json!({"tag":"UpkeepWithCityBlessing"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::Monarch { opponent }),
        ) => {
            json!({"tag":"MonarchUpkeep","opponent":opponent})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::BeginningCombat,
                ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::ControlsOwnCommander),
        ) => {
            json!({"tag":"CombatWhileControllingOwnCommander"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::BeginningCombat,
                ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::TotalControlledPowerAtLeast(minimum)),
        ) => {
            json!({"tag":"OwnBeginningCombatTotalPowerAtLeast","minimum":minimum})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::End, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::ControlledCreaturePowerAtLeast(minimum)),
        ) => {
            json!({"tag":"OwnEndPowerAtLeast","minimum":minimum})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::OpponentHasMoreLands),
        ) => {
            json!({"tag":"UpkeepOpponentMoreLands"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::End, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::SourceDidNotAttackThisTurn),
        ) => {
            json!({"tag":"OwnEndWithoutAttacking"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::End, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::NoLoyaltyActivationThisTurn),
        ) => {
            json!({"tag":"EndWithoutLoyaltyActivation"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::End, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::OpponentLostLifeAtLeast(amount)),
        ) => {
            json!({"tag":"EndOpponentLostLife","amount":amount})
        }
        (
            TriggerEvent::PutIntoGraveyard {
                player: PlayerRelation::Opponent,
                ..
            },
            _,
            Some(TriggerCondition::SourceCounterAtLeast {
                kind: spellwake_engine::effects::CounterKind::Quest,
                minimum: amount,
            }),
        ) => {
            json!({"tag":"OpponentGraveyardWithQuestCounters","amount":amount})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            TriggerZone::Battlefield,
            Some(TriggerCondition::ControlsGreatestArtifact),
        ) => {
            json!({"tag":"UpkeepGreatestArtifact"})
        }
        (
            TriggerEvent::StepBegan {
                step: Step::Upkeep, ..
            },
            TriggerZone::Graveyard,
            _,
        ) => {
            json!({"tag":"GraveyardUpkeep"})
        }
        (
            TriggerEvent::StepBegan { step, .. },
            _,
            Some(TriggerCondition::ControlledCreatureCount(count)),
        ) => {
            json!({"tag":"ControllerStepWithCreatureCount","step":format!("{step:?}"),"count":count})
        }
        (
            TriggerEvent::StepBegan {
                step,
                player: TriggerPlayerScope::Relation(player),
            },
            _,
            None,
        ) => {
            json!({"tag":"StepBegan","step":format!("{step:?}"),"player":format!("{player:?}")})
        }
        _ => json!({"tag":"TriggerSpec","debug":format!("{value:?}")}),
    }
}
fn object_relation_label(filter: ObjectFilter) -> String {
    if filter == ObjectFilter::self_object() {
        "SelfObject".to_owned()
    } else if filter == ObjectFilter::any().attached_to_self() {
        "AttachedPermanent".to_owned()
    } else if filter == ObjectFilter::creature() {
        "AnyCreature".to_owned()
    } else if filter == ObjectFilter::creature().other_than_ability_source() {
        "AnotherCreature".to_owned()
    } else if filter == ObjectFilter::creature().controlled() {
        "ControlledCreature".to_owned()
    } else if filter
        == ObjectFilter::creature()
            .controlled()
            .other_than_ability_source()
    {
        "AnotherControlledCreature".to_owned()
    } else if filter
        == ObjectFilter::creature()
            .nontoken()
            .other_than_ability_source()
    {
        "AnotherNontokenCreature".to_owned()
    } else if filter
        == ObjectFilter::creature()
            .controlled()
            .nontoken()
            .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
    {
        "ControlledNontokenCreatureWithPlusOneCounter".to_owned()
    } else if filter == ObjectFilter::creature().controlled().legendary() {
        "ControlledLegendaryCreature".to_owned()
    } else if filter
        == ObjectFilter::any()
            .controlled()
            .with_artifact_subtype(ArtifactSubtype::Equipment)
            .or_self()
    {
        "SelfOrControlledEquipment".to_owned()
    } else if filter
        == ObjectFilter::any()
            .controlled()
            .with_artifact_subtype(ArtifactSubtype::Equipment)
    {
        "ControlledEquipment".to_owned()
    } else if filter == ObjectFilter::any().controlled().token() {
        "ControlledToken".to_owned()
    } else if filter == ObjectFilter::creature().opposing().nontoken() {
        "OpposingNontokenCreature".to_owned()
    } else if filter == ObjectFilter::creature().opposing() {
        "OpposingCreature".to_owned()
    } else if filter
        == ObjectFilter::creature()
            .controlled()
            .nontoken()
            .other_than_ability_source()
    {
        "AnotherControlledNontokenCreature".to_owned()
    } else if filter == ObjectFilter::creature().controlled().nontoken().or_self() {
        "SelfOrControlledNontokenCreature".to_owned()
    } else if filter == ObjectFilter::creature().controlled().nontoken() {
        "ControlledNontokenCreature".to_owned()
    } else if let ObjectFilter::Clause(c) = filter {
        if let PowerFilter::AtMost(max) = c.power
            && filter
                == ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source()
                    .with_power(PowerFilter::AtMost(max))
        {
            return format!("AnotherControlledCreatureAtMostPower:{max}");
        }
        let req = mask_card_types(c.required_types);
        if req.len() == 1
            && filter
                == ObjectFilter::creature()
                    .with_required_type(req[0])
                    .controlled()
        {
            return format!("ControlledCreatureOfType({:?})", req[0]);
        }
        let any = mask_card_types(c.any_types);
        if any.len() == 1
            && filter == ObjectFilter::any_types(CardTypeMask::from_card_type(any[0])).controlled()
        {
            return format!("ControlledType({:?})", any[0]);
        }
        format!("{filter:?}")
    } else if let ObjectFilter::Either(a, b) = filter
        && a.source_relation == SourceRelation::SelfObject
    {
        let any = mask_card_types(b.any_types);
        if any.len() == 1
            && filter
                == ObjectFilter::any_types(CardTypeMask::from_card_type(any[0]))
                    .controlled()
                    .or_self()
        {
            return format!("SelfOrControlledType({:?})", any[0]);
        }
        format!("{filter:?}")
    } else {
        format!("{filter:?}")
    }
}

fn activated(abilities: &[ActivatedAbility]) -> Value {
    use spellwake_engine::activated::{
        CounterCostDelta, DiscardCostMode, GraveyardExileCost, HandDiscardKeyword, LifeCostAmount,
        PowerAbility, SelectedSacrificeFilter, TapCostCount, TapCostFilter,
    };
    use spellwake_engine::effects::CounterKind;
    json!(abilities.iter().map(|ability| {
        let mut row = json!({"id": ability.id.0, "mana_cost": cost(&ability.mana_cost), "source_cost": "", "body": body(&ability.body)});
        match ability.source_cost {
            SourceCost::None => row["source_cost"] = json!("None"),
            SourceCost::Tap => row["source_cost"] = json!("Tap"),
            SourceCost::Sacrifice => row["source_cost"] = json!("Sacrifice"),
            SourceCost::TapSacrifice => row["source_cost"] = json!("TapSacrifice"),
            SourceCost::Exile { from_graveyard: true, .. } => row["source_cost"] = json!("GraveyardExile"),
            SourceCost::Exile { tap_source: true, .. } => row["source_cost"] = json!("TapExile"),
            SourceCost::Exile { .. } => row["source_cost"] = json!("Exile"),
            SourceCost::PayLife { amount: LifeCostAmount::CommanderColors, .. } => row["source_cost"] = json!("TapPayCommanderColors"),
            SourceCost::PayLife { amount: LifeCostAmount::Fixed(n), sacrifice: Some(_), .. } => row["source_cost"] = json!(format!("PayLifeSacrificeOtherCreature({n})")),
            SourceCost::PayLife { amount: LifeCostAmount::Fixed(n), tap_source: true, .. } => row["source_cost"] = json!(format!("TapPayLife({n})")),
            SourceCost::PayLife { amount: LifeCostAmount::Fixed(n), .. } => row["source_cost"] = json!(format!("PayLife({n})")),
            SourceCost::PayLife { amount: LifeCostAmount::ChosenX, .. } => row["source_cost"] = json!("PayLife(X)"),
            SourceCost::GenericX { symbols, tap_source: true } => row["source_cost"] = json!(format!("TapWithGenericX({symbols})")),
            SourceCost::GenericX { .. } => row["source_cost"] = json!("PayGenericX"),
            SourceCost::Counter { kind: CounterKind::Page, delta: CounterCostDelta::Add(_), .. } => row["source_cost"] = json!("TapPutPage"),
            SourceCost::Counter { kind: CounterKind::Verse, delta: CounterCostDelta::Add(_), .. } => row["source_cost"] = json!("TapPutVerse"),
            SourceCost::Counter { kind: CounterKind::Quest, sacrifice_source: true, .. } => row["source_cost"] = json!("RemoveThreeQuestSacrifice"),
            SourceCost::Counter { kind: CounterKind::Suspect, .. } => row["source_cost"] = json!("TapRemoveSuspect"),
            SourceCost::Counter { kind: CounterKind::Charge, .. } => row["source_cost"] = json!("TapRemoveCharge"),
            SourceCost::Counter { kind: CounterKind::PlusOne, .. } => row["source_cost"] = json!("TapRemovePlusOne"),
            SourceCost::Counter { .. } => row["source_cost"] = json!(format!("{:?}", ability.source_cost)),
            SourceCost::TapPermanents { filter: TapCostFilter::Creature, count: TapCostCount::Fixed(count), tap_source: true, .. } => {
                row["source_cost"] = json!("TapSourceAndCreatures");
                row["cost_count"] = json!(count.get());
            }
            SourceCost::TapPermanents { filter: TapCostFilter::Subtype(kind), count: TapCostCount::Fixed(count), tap_source: true, .. } => {
                row["source_cost"] = json!("TapSourceAndSubtype");
                row["cost_subtype"] = projection::creature_type_id(kind);
                row["cost_count"] = json!(count.get());
            }
            SourceCost::TapPermanents { filter: TapCostFilter::Subtype(kind), count: TapCostCount::ChosenX, .. } => {
                row["source_cost"] = json!("TapSourceAndSubtypeX");
                row["cost_subtype"] = projection::creature_type_id(kind);
            }
            SourceCost::TapPermanents { filter: TapCostFilter::Artifact, count: TapCostCount::Fixed(count), .. } => {
                row["source_cost"] = json!("TapSelectedArtifacts");
                row["cost_count"] = json!(count.get());
            }
            SourceCost::TapPermanents { filter: TapCostFilter::Subtype(kind), tap_source: false, .. } => {
                row["source_cost"] = json!("TapPermanentOfSubtype");
                row["cost_subtype"] = projection::creature_type_id(kind);
            }
            SourceCost::TapPermanents { filter: TapCostFilter::Legendary, .. } => row["source_cost"] = json!("TapLegendaryPermanent"),
            SourceCost::TapPermanents { .. } => row["source_cost"] = json!(format!("{:?}", ability.source_cost)),
            SourceCost::SacrificeSelected { from_graveyard: true, count, .. } => {
                row["source_cost"] = json!("GraveyardSacrificeArtifacts");
                row["cost_count"] = json!(count);
            }
            SourceCost::SacrificeSelected { filter, count, tap_source, from_graveyard: false } => {
                row["source_cost"] = json!("SacrificeSelected");
                row["cost_filter"] = if let SelectedSacrificeFilter::Subtype(kind) = filter {
                    row["cost_subtype"] = projection::creature_type_id(kind);
                    json!("Subtype")
                } else {
                    json!(format!("{filter:?}"))
                };
                row["cost_count"] = json!(count);
                row["cost_tap_source"] = json!(tap_source);
            }
            SourceCost::Discard { mode: DiscardCostMode::WholeHand, .. } => row["source_cost"] = json!("TapDiscardHand"),
            SourceCost::Discard { mode: DiscardCostMode::MatchingTargetSpellManaValue, .. } => row["source_cost"] = json!("DiscardMatchingSpellManaValue"),
            SourceCost::Discard { mode: DiscardCostMode::SelectedOne, tap_source, .. } => {
                row["source_cost"] = json!("DiscardSelected");
                row["cost_tap_source"] = json!(tap_source);
            }
            SourceCost::Discard { mode: DiscardCostMode::Filtered { filter, count }, tap_source, .. } => {
                row["source_cost"] = json!(format!("DiscardFiltered({count})"));
                row["cost_filter"] = json!(format!("{filter:?}"));
                row["cost_tap_source"] = json!(tap_source);
            }
            SourceCost::ExileGraveyard { filter: GraveyardExileCost::AnyCountX, .. } => row["source_cost"] = json!("TapExileGraveyardX"),
            SourceCost::ExileGraveyard { filter: GraveyardExileCost::Creature { pay_mana_cost: true }, .. } => row["source_cost"] = json!("ExileSelectedGraveyardCreatureAndPayItsManaCost"),
            SourceCost::ExileGraveyard { filter: GraveyardExileCost::Creature { pay_mana_cost: false }, tap_source: true } => row["source_cost"] = json!("TapExileSelectedGraveyardCreature"),
            SourceCost::ExileGraveyard { filter: GraveyardExileCost::Creature { pay_mana_cost: false }, tap_source: false } => row["source_cost"] = json!("ExileSelectedGraveyardCreature"),
            SourceCost::ExileGraveyard { filter: GraveyardExileCost::Filtered { filter, count }, tap_source } => {
                row["source_cost"] = json!(format!("ExileGraveyardFiltered({count})"));
                row["cost_filter"] = json!(format!("{filter:?}"));
                row["cost_tap_source"] = json!(tap_source);
            }
            SourceCost::Mill { .. } => row["source_cost"] = json!("TapMillOne"),
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling) => row["source_cost"] = json!("CycleDiscard"),
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute) => row["source_cost"] = json!("TransmuteDiscard"),
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce) => row["source_cost"] = json!("ReinforceDiscard"),
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel) => row["source_cost"] = json!("ChannelDiscard"),
            SourceCost::GraveyardSelf { unearth: true } => row["source_cost"] = json!("Unearth"),
            SourceCost::GraveyardSelf { unearth: false } => row["source_cost"] = json!("GraveyardSelf"),
            SourceCost::GraveyardDiscard { mode } => row["source_cost"] = json!(format!("GraveyardDiscard({mode:?})")),
            SourceCost::GraveyardExileOther { filter } => row["source_cost"] = json!(format!("GraveyardExileOther({filter:?})")),
            SourceCost::GraveyardPayLife { amount } => row["source_cost"] = json!(format!("GraveyardPayLife({amount:?})")),
            SourceCost::Ninjutsu { from_command: false } => row["source_cost"] = json!("Ninjutsu"),
            SourceCost::Ninjutsu { from_command: true } => row["source_cost"] = json!("CommanderNinjutsu"),
            SourceCost::Power { kind: PowerAbility::Station, .. } => row["source_cost"] = json!("Station"),
            SourceCost::Power { kind: PowerAbility::Crew, minimum } => row["source_cost"] = json!(format!("Crew({minimum})")),
            SourceCost::Power { kind: PowerAbility::Saddle, minimum } => row["source_cost"] = json!(format!("Saddle({minimum})")),
            SourceCost::PayEnergy { amount, tap_source } => {
                row["source_cost"] = json!(format!("PayEnergy({amount})"));
                row["cost_tap_source"] = json!(tap_source);
            }
            SourceCost::Untap => row["source_cost"] = json!("Untap"),
            SourceCost::ReturnSelfToHand { tap_source } => {
                row["source_cost"] = json!("ReturnSelfToHand");
                row["cost_tap_source"] = json!(tap_source);
            }
        }
        row
    }).collect::<Vec<_>>())
}
/// Encode the admitted trigger collection for definitions, current text and copies.
pub fn triggered_abilities(abilities: &[TriggeredAbility]) -> Value {
    use spellwake_engine::program::TriggerGrouping;
    json!(abilities.iter().map(|ability| json!({"id":ability.id.0,"condition":condition(&ability.condition),"grouping":match ability.grouping {TriggerGrouping::EachOccurrence=>"EachOccurrence",TriggerGrouping::OneOrMoreTogether=>"OneOrMoreTogether",TriggerGrouping::OneOrMoreOnceEachTurn=>"OneOrMoreOnceEachTurn",TriggerGrouping::OneOrMoreByPlayer=>"OneOrMoreByPlayer"},"body":body(&ability.body)})).collect::<Vec<_>>())
}
fn nested_mana_output(output: Output) -> Result<ManaOutput, String> {
    Ok(match output {
        Output::Fixed { produced } => ManaOutput::Fixed(mana_map(produced)?),
        Output::RestrictedFixed {
            produced,
            restriction,
        } => ManaOutput::Fixed(mana_map(produced)?).restricted(match restriction.as_str() {
            "PlaneswalkerSpell" => spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell,
            "ColorlessSpell" => spellwake_engine::mana::ManaRestriction::ColorlessSpell,
            "CommanderSpell" => spellwake_engine::mana::ManaRestriction::CommanderSpell,
            "ColorlessEldraziSpellOrAbility" => {
                spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility
            }
            "NoncreatureSpell" => spellwake_engine::mana::ManaRestriction::NoncreatureSpell,
            "ArtifactSpellOrAbility" => {
                spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility
            }
            _ => return Err("unknown mana restriction".into()),
        }),
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
        Output::RestrictedChooseColor {
            amount,
            colors,
            restriction,
        } => {
            let count = colors.len();
            let colors = colors
                .iter()
                .map(|name| mana(name))
                .collect::<Result<BTreeSet<_>, _>>()?;
            if colors.len() != count {
                return Err("duplicate mana color".into());
            }
            ManaOutput::ChooseColor { amount, colors }.restricted(match restriction.as_str() {
                "PlaneswalkerSpell" => spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell,
                "ColorlessSpell" => spellwake_engine::mana::ManaRestriction::ColorlessSpell,
                "CommanderSpell" => spellwake_engine::mana::ManaRestriction::CommanderSpell,
                "ColorlessEldraziSpellOrAbility" => {
                    spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility
                }
                "NoncreatureSpell" => spellwake_engine::mana::ManaRestriction::NoncreatureSpell,
                "ArtifactSpellOrAbility" => {
                    spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility
                }
                _ => return Err("unknown mana restriction".into()),
            })
        }
        _ => return Err("invalid nested mana output".into()),
    })
}
fn mana_output_view(output: &ManaOutput) -> Value {
    use spellwake_engine::mana::{
        ChosenTypeManaScope, CounterManaYield, DynamicManaYield, ManaOutputModifier,
        ManaOutputRestriction, ManaSacrificeCost, ManaSideEffect, SpecialColorYield,
        TriggeredManaRule,
    };
    match output {
        ManaOutput::Fixed(values) => {
            json!({"tag":"Fixed","produced":values.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
        }
        ManaOutput::ChooseColor { amount, colors } => {
            json!({"tag":"ChooseColor","amount":amount,"colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
        }
        ManaOutput::Triggered(rule) => match rule {
            TriggeredManaRule::LandTappedWhileMonarch => json!({"tag":"LandTappedWhileMonarch"}),
            TriggeredManaRule::ImprintedLandTappedSameType => {
                json!({"tag":"ImprintedLandTappedSameType"})
            }
            TriggeredManaRule::LandTappedSameType => json!({"tag":"LandTappedSameType"}),
            TriggeredManaRule::PermanentTappedForColorless => {
                json!({"tag":"PermanentTappedForColorless"})
            }
            TriggeredManaRule::LandTappedChosenColor { controller_only } => {
                json!({"tag":"LandTappedChosenColor","controller_only":controller_only})
            }
            TriggeredManaRule::EnchantedLandTapped { extra } => {
                json!({"tag":"EnchantedLandTapped","extra":format!("{extra:?}")})
            }
        },
        ManaOutput::CounterBased(kind) => match kind {
            CounterManaYield::SourceChargeCounters => json!({"tag":"SourceChargeCounters"}),
            CounterManaYield::RemoveChargeForColor => json!({"tag":"RemoveChargeForColor"}),
            CounterManaYield::RemoveStorageCounters => json!({"tag":"RemoveStorageCounters"}),
        },
        ManaOutput::TwoColors { distinct: true } => json!({"tag":"TwoDistinctColors"}),
        ManaOutput::TwoColors { distinct: false } => json!({"tag":"TwoColors","distinct":false}),
        ManaOutput::Counted {
            mana_type: Some(mana_type),
            permanents,
        } => {
            json!({"tag":"Counted","mana_type":format!("{mana_type:?}"),"permanents":selector(permanents)})
        }
        ManaOutput::Counted {
            mana_type: None,
            permanents,
        } => json!({"tag":"CountedColor","permanents":selector(permanents)}),
        ManaOutput::SpecialColor(kind) => match kind {
            SpecialColorYield::CommanderColorIdentity => json!({"tag":"CommanderColorIdentity"}),
            SpecialColorYield::ChosenPlayerColor => json!({"tag":"ChosenPlayerColor"}),
            SpecialColorYield::OpponentsLandColors => json!({"tag":"OpponentsLandColors"}),
            SpecialColorYield::EntryChosenColor { fixed } => {
                json!({"tag":"EntryChosenColor","fixed":format!("{fixed:?}")})
            }
            SpecialColorYield::HybridFilter { colors } => {
                json!({"tag":"HybridFilter","colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
            }
        },
        ManaOutput::Dynamic(kind) => match kind {
            DynamicManaYield::GreatestCreaturePower => json!({"tag":"GreatestCreaturePower"}),
            DynamicManaYield::ControlledCreaturePowerColor {
                minimum_power,
                otherwise,
                matching,
            } => {
                json!({"tag":"ControlledCreaturePowerColor","minimum_power":minimum_power,"otherwise":otherwise,"matching":matching})
            }
            DynamicManaYield::ConditionalControlledLandTypes {
                requirements,
                normal,
                fulfilled,
            } => {
                json!({"tag":"ConditionalControlledLandTypes", "requirements":requirements.iter().map(land_type_values).collect::<Vec<_>>(), "normal":normal.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>(), "fulfilled":fulfilled.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
            }
        },
        ManaOutput::Modified {
            modifier,
            output: inner,
        } => match (modifier, inner.as_ref()) {
            (ManaOutputModifier::Sacrifice(ManaSacrificeCost::Source { tap }), inner) => {
                json!({"tag":"SacrificeSource","tap":tap,"output":mana_output_view(inner)})
            }
            (
                ManaOutputModifier::Sacrifice(ManaSacrificeCost::ControlledSubtype(subtype)),
                inner,
            ) => {
                json!({"tag":"SacrificeSubtype","subtype":projection::creature_type_id(*subtype),"output":mana_output_view(inner)})
            }
            (ManaOutputModifier::Sacrifice(ManaSacrificeCost::ControlledCreature), inner) => {
                json!({"tag":"SacrificeCreature","output":mana_output_view(inner)})
            }
            (
                ManaOutputModifier::RequirePermanents {
                    selector: filter,
                    minimum,
                },
                inner,
            ) => {
                json!({"tag":"Conditional","selector":selector(filter),"minimum":minimum,"output":mana_output_view(inner)})
            }
            (ManaOutputModifier::TapControlledArtifact, ManaOutput::Fixed(produced)) => {
                json!({"tag":"TapControlledArtifact","produced":mana_values(produced)})
            }
            (
                ManaOutputModifier::SideEffect(ManaSideEffect::GainLife(life)),
                ManaOutput::Fixed(produced),
            ) => {
                json!({"tag":"FixedWithLifeGain","produced":produced.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>(),"life":life})
            }
            (
                ManaOutputModifier::SideEffect(ManaSideEffect::DamageController(damage)),
                ManaOutput::ChooseColor { colors, .. },
            ) => {
                json!({"tag":"ChooseColorWithControllerDamage","damage":damage,"colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
            }
            (
                ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(restriction)),
                ManaOutput::Fixed(produced),
            ) => {
                json!({"tag":"RestrictedFixed","restriction":format!("{restriction:?}"),"produced":produced.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
            }
            (
                ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(restriction)),
                ManaOutput::ChooseColor { amount, colors },
            ) => {
                json!({"tag":"RestrictedChooseColor","amount":amount,"restriction":format!("{restriction:?}"),"colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
            }
            (
                ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(restriction)),
                ManaOutput::TwoColors { distinct: false },
            ) => json!({"tag":"TwoColoredManaRestricted","restriction":format!("{restriction:?}")}),
            (ManaOutputModifier::Restrict(ManaOutputRestriction::ChosenCreatureType(scope)), _) => {
                match scope {
                    ChosenTypeManaScope::CreatureSpell => {
                        json!({"tag":"ChooseColorForCreatureType"})
                    }
                    ChosenTypeManaScope::CreatureSpellUncounterable => {
                        json!({"tag":"ChooseColorForCreatureTypeUncounterable"})
                    }
                    ChosenTypeManaScope::CreatureSpellOrAbility => {
                        json!({"tag":"ChooseColorForCreatureTypeOrAbility"})
                    }
                }
            }
            (ManaOutputModifier::PayMana(price), ManaOutput::Fixed(produced)) => {
                json!({"tag":"PaidFixed","cost":cost(price),"produced":produced.iter().map(|(kind,amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
            }
            (ManaOutputModifier::PayMana(price), ManaOutput::ChooseColor { amount, colors }) => {
                json!({"tag":"PaidChooseColor","cost":cost(price),"amount":amount,"colors":colors.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>()})
            }
            (
                ManaOutputModifier::PayMana(price),
                ManaOutput::Dynamic(DynamicManaYield::GreatestCreaturePower),
            ) => json!({"tag":"PaidGreatestCreaturePower","cost":cost(price)}),
            (
                ManaOutputModifier::PayMana(price),
                ManaOutput::Modified {
                    modifier:
                        ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(restriction)),
                    output: base,
                },
            ) if matches!(base.as_ref(), ManaOutput::TwoColors { distinct: true }) => {
                json!({"tag":"PaidTwoDistinctRestricted","cost":cost(price),"restriction":format!("{restriction:?}")})
            }
            (
                ManaOutputModifier::PayMana(price),
                ManaOutput::Modified {
                    modifier: ManaOutputModifier::SpendEffect(effect),
                    output: base,
                },
            ) if matches!(
                base.as_ref(),
                ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
            ) =>
            {
                json!({"tag":"CommanderIdentityWithSpendEffect","cost":cost(price),"effect":format!("{effect:?}")})
            }
            (modifier, inner) => {
                json!({"tag":"Modified","modifier":format!("{modifier:?}"),"output":mana_output_view(inner)})
            }
        },
    }
}
fn mana_abilities(abilities: &[ManaAbility]) -> Value {
    json!(
        abilities
            .iter()
            .map(|ability| json!({"id":ability.id.0,"output":mana_output_view(&ability.output)}))
            .collect::<Vec<_>>()
    )
}
fn face_up_abilities(values: &[spellwake_engine::face_down::FaceUpAbility]) -> Value {
    use spellwake_engine::face_down::FaceUpAbility;
    json!(
        values
            .iter()
            .map(|ability| match ability {
                FaceUpAbility::Morph(p) => json!({"Morph":cost(p)}),
                FaceUpAbility::Megamorph(p) => json!({"Megamorph":cost(p)}),
                FaceUpAbility::Disguise(p) => json!({"Disguise":cost(p)}),
            })
            .collect::<Vec<_>>()
    )
}
pub fn printed_text(value: &PrintedText) -> Value {
    let mut result = json!({"mana":mana_abilities(&value.mana),"activated":activated(&value.activated),"triggered":triggered_abilities(&value.triggered),"continuous":value.continuous.as_ref().map(static_ability)});
    if !value.additional_battlefield_abilities.is_empty() {
        result["additional_battlefield_abilities"] = json!(
            value
                .additional_battlefield_abilities
                .iter()
                .map(static_ability)
                .collect::<Vec<_>>()
        );
    }
    if !value.entry_counter_abilities.is_empty() {
        result["entry_counter_abilities"] = entry_counters(&value.entry_counter_abilities);
    }
    if !value.cast_reductions.is_empty() {
        result["cast_reductions"] = cast_reductions(&value.cast_reductions);
    }
    if !value.additional_cast_costs.is_empty() {
        result["additional_cast_costs"] = additional_costs(&value.additional_cast_costs);
    }
    if let Some(alternative) = &value.conditional_cost {
        use spellwake_engine::program::ConditionalCastCost;
        result["conditional_cost"] = match alternative {
            ConditionalCastCost::EndStep { kind, mana } => {
                json!({"tag":format!("{kind:?}"),"cost":cost(mana)})
            }
            ConditionalCastCost::Overload { mana, each } => {
                json!({"tag":"Overload","cost":cost(mana),"each":body(each)})
            }
            ConditionalCastCost::Cleave { mana, unbracketed } => {
                json!({"tag":"Cleave","cost":cost(mana),"unbracketed":body(unbracketed)})
            }
            ConditionalCastCost::Prowl(price) => json!({"tag":"Prowl","cost":cost(price)}),
            ConditionalCastCost::Freerunning(price) => {
                json!({"tag":"Freerunning","cost":cost(price)})
            }
            ConditionalCastCost::Mayhem(price) => json!({"tag":"Mayhem","cost":cost(price)}),
            ConditionalCastCost::ExileHandCards { mana, count, color } => {
                json!({"tag":"ExileHandCards","cost":cost(mana),"count":count,"color":format!("{color:?}")})
            }
            ConditionalCastCost::TapPermanents {
                mana,
                count,
                quality,
            } => {
                json!({"tag":"TapPermanents","cost":cost(mana),"count":count,"quality":format!("{quality:?}")})
            }
            ConditionalCastCost::WebSlinging(price) => {
                json!({"tag":"WebSlinging","cost":cost(price)})
            }
            ConditionalCastCost::Emerge { mana, from } => {
                json!({"tag":"Emerge","cost":cost(mana),"from":format!("{from:?}")})
            }
            ConditionalCastCost::Sneak(price) => json!({"tag":"Sneak","cost":cost(price)}),
            ConditionalCastCost::Evoke(price) => json!({"tag":"Evoke","cost":cost(price)}),
            ConditionalCastCost::Mutate(price) => json!({"tag":"Mutate","cost":cost(price)}),
            ConditionalCastCost::Bestow(price) => json!({"tag":"Bestow","cost":cost(price)}),
            ConditionalCastCost::ControlsCommander(price) => {
                json!({"tag":"ControlsCommander","cost":cost(price)})
            }
            ConditionalCastCost::Surge(price) => json!({"tag":"Surge","cost":cost(price)}),
            ConditionalCastCost::Escape {
                mana,
                exile,
                entry_counters,
            } => {
                json!({"tag":"Escape","cost":cost(mana),"exile":exile,"entry_counters":entry_counters})
            }
            ConditionalCastCost::Spectacle(price) => json!({"tag":"Spectacle","cost":cost(price)}),
            ConditionalCastCost::Condition { condition, mana } => {
                json!({"tag":"Condition","condition":format!("{condition:?}"),"cost":cost(mana)})
            }
            ConditionalCastCost::Awaken { mana, counters } => {
                json!({"tag":"Awaken","cost":cost(mana),"counters":counters})
            }
            ConditionalCastCost::Impending { mana, counters } => {
                json!({"tag":"Impending","cost":cost(mana),"counters":counters})
            }
            ConditionalCastCost::Harmonize(price) => json!({"tag":"Harmonize","cost":cost(price)}),
            ConditionalCastCost::Offering { mana, subtype } => {
                json!({"tag":"Offering","cost":cost(mana),"subtype":format!("{subtype:?}")})
            }
            ConditionalCastCost::Alternative {
                condition,
                mana,
                extra,
            } => {
                json!({"tag":"Alternative","condition":format!("{condition:?}"),"cost":cost(mana),"extra":format!("{extra:?}")})
            }
        };
    }
    if let Some(action) = &value.exile_action {
        result["exile_action"] = match action {
            spellwake_engine::program::ExileAction::Foretell(price) => {
                json!({"tag":"Foretell","cost":cost(price)})
            }
            spellwake_engine::program::ExileAction::Suspend { price, counters } => {
                json!({"tag":"Suspend","cost":cost(price),"counters":counters})
            }
        };
    }
    if let Some(price) = &value.madness {
        result["madness"] = cost(price);
    }
    if let Some(price) = &value.miracle {
        result["miracle"] = json!({"mana":cost(&price.mana),"x_symbols":price.x_symbols});
    }
    if let Some(price) = &value.plot {
        result["plot"] = cost(price);
    }
    if let Some(price) = &value.flashback {
        result["flashback"] = cost(price);
    }
    if value.sacrifice_creature_cast_cost {
        result["sacrifice_creature_cast_cost"] = json!(true);
    }
    if value.tap_creatures_cast_cost {
        result["tap_creatures_cast_cost"] = json!(true);
    }
    if value.reveal_colorless_creature_cast_cost {
        result["reveal_colorless_creature_cast_cost"] = json!(true);
    }
    if value.discard_cast_cost {
        result["discard_cast_cost"] = json!(true);
    }
    if value.flashback_x_mana_symbols != 0 {
        result["flashback_x_mana_symbols"] = json!(value.flashback_x_mana_symbols);
    }
    if value.flashback_life != 0 {
        result["flashback_life"] = json!(value.flashback_life);
    }
    if value.flashback_sacrifice_creatures != 0 {
        result["flashback_sacrifice_creatures"] = json!(value.flashback_sacrifice_creatures);
    }
    if value.flashback_tap_white_creatures != 0 {
        result["flashback_tap_white_creatures"] = json!(value.flashback_tap_white_creatures);
    }
    if let Some(amount) = &value.printed_toxic {
        result["printed_toxic"] = json!(amount);
    }
    if !value.loyalty_costs.is_empty() {
        result["loyalty_costs"] = json!(
            value
                .loyalty_costs
                .iter()
                .map(|(id, cost)| (id.0, cost))
                .collect::<BTreeMap<_, _>>()
        );
    }
    if !value.activation_restrictions.is_empty() {
        result["activation_restrictions"] = json!(
            value
                .activation_restrictions
                .iter()
                .map(|(id, rule)| (id.0, format!("{rule:?}")))
                .collect::<BTreeMap<_, _>>()
        );
    }
    if !value.face_up_abilities.is_empty() {
        result["face_up_abilities"] = face_up_abilities(&value.face_up_abilities);
    }
    if !value.printed_keywords.is_empty() {
        result["printed_keywords"] = keywords(&value.printed_keywords);
    }
    if value.graveyard_exile_x_cost {
        result["graveyard_exile_x_cost"] = json!(true);
    }
    if value.graveyard_creature_exile_cost != 0 {
        result["graveyard_creature_exile_cost"] = json!(value.graveyard_creature_exile_cost);
    }
    if let Some(permission) = &value.graveyard_self_cast {
        result["graveyard_self_cast"] = json!(format!("{permission:?}"));
    }
    if !value.zone_replacements.is_empty() {
        result["zone_replacements"] = zone_replacements(&value.zone_replacements);
    }
    result
}
fn attachment_labels() -> [(&'static str, spellwake_engine::attachment::AttachmentKind); 11] {
    use spellwake_engine::attachment::{
        AttachmentKind::{Aura, Equipment},
        EnchantRestriction::*,
    };
    [
        ("Equipment", Equipment),
        (
            "Fortification",
            spellwake_engine::attachment::AttachmentKind::Fortification,
        ),
        ("AuraCreature", Aura(Creature)),
        ("AuraNonblackCreature", Aura(NonblackCreature)),
        ("AuraCreatureYouControl", Aura(CreatureYouControl)),
        (
            "AuraCreatureAnOpponentControls",
            Aura(CreatureAnOpponentControls),
        ),
        ("AuraArtifact", Aura(Artifact)),
        ("AuraEnchantment", Aura(Enchantment)),
        ("AuraLand", Aura(Land)),
        ("AuraPermanent", Aura(Permanent)),
        ("AuraPlayer", Aura(Player)),
    ]
}
pub fn view(value: &Program) -> Value {
    let kind = match &value.kind {
        ProgramKind::Permanent(value) => {
            let mut wire = json!({"tag":"Permanent","printed_pt":value.printed_pt.as_ref().map(printed_pt),"casting_cost":value.casting_cost.as_ref().map(cost)});
            if let Some(rule) = value.entry_tap {
                wire["entry_tap"] = match rule {
                    spellwake_engine::land::EntryTap::SiegeProtector => {
                        json!({"tag":"SiegeProtector"})
                    }
                    spellwake_engine::land::EntryTap::ChooseTwoPlayers => {
                        json!({"tag":"ChooseTwoPlayers"})
                    }
                    spellwake_engine::land::EntryTap::ReadAhead { final_chapter } => {
                        json!({"tag":"ReadAhead","final_chapter":final_chapter})
                    }
                    spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(excluded) => {
                        json!({"tag":"AlwaysChooseColorExcept","excluded":format!("{excluded:?}")})
                    }
                    spellwake_engine::land::EntryTap::UnlessLandType(a, b) => {
                        json!({"tag":"UnlessLandType","first":a.land_type().wire_label(),"second":b.land_type().wire_label()})
                    }
                    spellwake_engine::land::EntryTap::UnlessRevealHandLandType(a, b) => {
                        json!({"tag":"UnlessRevealHandLandType","first":a.land_type().wire_label(),"second":b.land_type().wire_label()})
                    }
                    spellwake_engine::land::EntryTap::UnlessRevealHandSubtype {
                        subtype,
                        or_control,
                    } => {
                        json!({"tag":"UnlessRevealHandSubtype","subtype":projection::creature_type_id(subtype),"orControl":or_control})
                    }
                    _ => json!({"tag":format!("{rule:?}")}),
                };
            }
            if let Some((label, _)) = attachment_labels()
                .into_iter()
                .find(|(_, kind)| Some(*kind) == value.attachment)
            {
                wire["attachment"] = json!(label);
            }
            wire
        }
        ProgramKind::Spell(value) => {
            let mut wire =
                json!({"tag":"Spell","casting_cost":cost(&value.cost),"body":body(&value.body)});
            if let Some(spec) = &value.modes {
                wire["modes"] = json!({"minimum":spec.minimum,"maximum":spec.maximum,"allow_repeat":spec.allow_repeat,
                    "additional":spec.additional.iter().map(body).collect::<Vec<_>>()});
            }
            wire
        }
    };
    let mut result = json!({"land_subtypes":land_type_values(&value.land_subtypes),"card_types":value.card_types.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"creature_subtypes":subtype_values(&value.creature_subtypes),"names":value.names,"supertypes":value.supertypes.iter().map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"kind":kind,"color_indicator":value.color_indicator.as_ref().map(colors),"effect_defined_colors":value.effect_defined_colors.as_ref().map(colors),"mana":mana_abilities(&value.mana),
        "activated":activated(&value.activated),
        "triggered":triggered_abilities(&value.triggered),
        "continuous":value.continuous.as_ref().map(static_ability)});
    if !value.additional_battlefield_abilities.is_empty() {
        result["additional_battlefield_abilities"] = json!(
            value
                .additional_battlefield_abilities
                .iter()
                .map(static_ability)
                .collect::<Vec<_>>()
        );
    }
    if let Some(price) = &value.additional_life_cost {
        result["additional_life_cost"] = instruction_amount(price);
    }
    if value.cast_during_own_combat {
        result["cast_during_own_combat"] = json!(true);
    }
    if value.combat_limits.one_attacker {
        result["one_attacker"] = json!(true);
    }
    if value.combat_limits.one_blocker {
        result["one_blocker"] = json!(true);
    }
    if let Some(rule) = &value.entry_copy {
        result["entry_copy"] = json!(format!("{rule:?}"));
    }
    if let Some(requirement) = value.attack_requirement {
        use spellwake_engine::program::{AttackCostSymbol, AttackRequirement};
        let name = match requirement {
            AttackRequirement::PayPerAttacker {
                protect_player,
                symbol,
            } => format!(
                "Tax{}{}",
                if protect_player {
                    "PlayerAndPlaneswalkers"
                } else {
                    "Planeswalkers"
                },
                match symbol {
                    AttackCostSymbol::OneGeneric | AttackCostSymbol::Generic(_) => "Generic",
                    AttackCostSymbol::WhitePhyrexian => "WhitePhyrexian",
                }
            ),
            rule => format!("{rule:?}"),
        };
        result["attack_requirement"] = json!(name);
    }
    if !value.hybrid_symbols.is_empty() {
        result["hybrid_symbols"] = json!(
            value
                .hybrid_symbols
                .iter()
                .map(|symbol| [
                    format!("{:?}", symbol.first),
                    match symbol.alternative {
                        spellwake_engine::mana::HybridAlternative::Mana(color) =>
                            format!("{color:?}"),
                        spellwake_engine::mana::HybridAlternative::TwoLife => "TwoLife".to_owned(),
                        spellwake_engine::mana::HybridAlternative::TwoGeneric => {
                            "TwoGeneric".to_owned()
                        }
                    }
                ])
                .collect::<Vec<_>>()
        );
    }
    if value.x_mana_symbols != 0 {
        result["x_mana_symbols"] = json!(value.x_mana_symbols);
    }
    if value.cannot_be_countered {
        result["cannot_be_countered"] = json!(true);
    }
    if !value.variable_loyalty_costs.is_empty() {
        result["variable_loyalty_costs"] = json!(
            value
                .variable_loyalty_costs
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>()
        );
    }
    if let Some(loyalty) = &value.printed_loyalty {
        result["printed_loyalty"] = json!(loyalty);
    }
    if !value.planeswalker_subtypes.is_empty() {
        result["planeswalker_subtypes"] = json!(
            value
                .planeswalker_subtypes
                .iter()
                .map(|kind| format!("{kind:?}"))
                .collect::<Vec<_>>()
        );
    }
    if !value.spell_subtypes.is_empty() {
        result["spell_subtypes"] = json!(
            value
                .spell_subtypes
                .iter()
                .map(|subtype| subtype.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.artifact_subtypes.is_empty() {
        result["artifact_subtypes"] = json!(
            value
                .artifact_subtypes
                .iter()
                .map(|subtype| subtype.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.enchantment_subtypes.is_empty() {
        result["enchantment_subtypes"] = json!(
            value
                .enchantment_subtypes
                .iter()
                .map(|subtype| subtype.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.entry_counter_abilities.is_empty() {
        result["entry_counter_abilities"] = entry_counters(&value.entry_counter_abilities);
    }
    if !value.cast_reductions.is_empty() {
        result["cast_reductions"] = cast_reductions(&value.cast_reductions);
    }
    if !value.additional_cast_costs.is_empty() {
        result["additional_cast_costs"] = additional_costs(&value.additional_cast_costs);
    }
    if let Some(alternative) = &value.conditional_cost {
        use spellwake_engine::program::ConditionalCastCost;
        result["conditional_cost"] = match alternative {
            ConditionalCastCost::EndStep { kind, mana } => {
                json!({"tag":format!("{kind:?}"),"cost":cost(mana)})
            }
            ConditionalCastCost::Overload { mana, each } => {
                json!({"tag":"Overload","cost":cost(mana),"each":body(each)})
            }
            ConditionalCastCost::Cleave { mana, unbracketed } => {
                json!({"tag":"Cleave","cost":cost(mana),"unbracketed":body(unbracketed)})
            }
            ConditionalCastCost::Prowl(price) => json!({"tag":"Prowl","cost":cost(price)}),
            ConditionalCastCost::Freerunning(price) => {
                json!({"tag":"Freerunning","cost":cost(price)})
            }
            ConditionalCastCost::Mayhem(price) => json!({"tag":"Mayhem","cost":cost(price)}),
            ConditionalCastCost::ExileHandCards { mana, count, color } => {
                json!({"tag":"ExileHandCards","cost":cost(mana),"count":count,"color":format!("{color:?}")})
            }
            ConditionalCastCost::TapPermanents {
                mana,
                count,
                quality,
            } => {
                json!({"tag":"TapPermanents","cost":cost(mana),"count":count,"quality":format!("{quality:?}")})
            }
            ConditionalCastCost::WebSlinging(price) => {
                json!({"tag":"WebSlinging","cost":cost(price)})
            }
            ConditionalCastCost::Emerge { mana, from } => {
                json!({"tag":"Emerge","cost":cost(mana),"from":format!("{from:?}")})
            }
            ConditionalCastCost::Sneak(price) => json!({"tag":"Sneak","cost":cost(price)}),
            ConditionalCastCost::Evoke(price) => json!({"tag":"Evoke","cost":cost(price)}),
            ConditionalCastCost::Mutate(price) => json!({"tag":"Mutate","cost":cost(price)}),
            ConditionalCastCost::Bestow(price) => json!({"tag":"Bestow","cost":cost(price)}),
            ConditionalCastCost::ControlsCommander(price) => {
                json!({"tag":"ControlsCommander","cost":cost(price)})
            }
            ConditionalCastCost::Surge(price) => json!({"tag":"Surge","cost":cost(price)}),
            ConditionalCastCost::Escape {
                mana,
                exile,
                entry_counters,
            } => {
                json!({"tag":"Escape","cost":cost(mana),"exile":exile,"entry_counters":entry_counters})
            }
            ConditionalCastCost::Spectacle(price) => json!({"tag":"Spectacle","cost":cost(price)}),
            ConditionalCastCost::Condition { condition, mana } => {
                json!({"tag":"Condition","condition":format!("{condition:?}"),"cost":cost(mana)})
            }
            ConditionalCastCost::Awaken { mana, counters } => {
                json!({"tag":"Awaken","cost":cost(mana),"counters":counters})
            }
            ConditionalCastCost::Impending { mana, counters } => {
                json!({"tag":"Impending","cost":cost(mana),"counters":counters})
            }
            ConditionalCastCost::Harmonize(price) => json!({"tag":"Harmonize","cost":cost(price)}),
            ConditionalCastCost::Offering { mana, subtype } => {
                json!({"tag":"Offering","cost":cost(mana),"subtype":format!("{subtype:?}")})
            }
            ConditionalCastCost::Alternative {
                condition,
                mana,
                extra,
            } => {
                json!({"tag":"Alternative","condition":format!("{condition:?}"),"cost":cost(mana),"extra":format!("{extra:?}")})
            }
        };
    }
    if let Some(action) = &value.exile_action {
        result["exile_action"] = match action {
            spellwake_engine::program::ExileAction::Foretell(price) => {
                json!({"tag":"Foretell","cost":cost(price)})
            }
            spellwake_engine::program::ExileAction::Suspend { price, counters } => {
                json!({"tag":"Suspend","cost":cost(price),"counters":counters})
            }
        };
    }
    if let Some(price) = &value.madness {
        result["madness"] = cost(price);
    }
    if value.cast_from_own_exile {
        result["cast_from_own_exile"] = json!(true);
    }
    if let Some(price) = &value.miracle {
        result["miracle"] = json!({"mana":cost(&price.mana),"x_symbols":price.x_symbols});
    }
    if let Some(price) = &value.plot {
        result["plot"] = cost(price);
    }
    if let Some(price) = &value.flashback {
        result["flashback"] = cost(price);
    }
    if value.sacrifice_creature_cast_cost {
        result["sacrifice_creature_cast_cost"] = json!(true);
    }
    if value.tap_creatures_cast_cost {
        result["tap_creatures_cast_cost"] = json!(true);
    }
    if value.reveal_colorless_creature_cast_cost {
        result["reveal_colorless_creature_cast_cost"] = json!(true);
    }
    if value.discard_cast_cost {
        result["discard_cast_cost"] = json!(true);
    }
    if value.flashback_x_mana_symbols != 0 {
        result["flashback_x_mana_symbols"] = json!(value.flashback_x_mana_symbols);
    }
    if value.flashback_life != 0 {
        result["flashback_life"] = json!(value.flashback_life);
    }
    if value.flashback_sacrifice_creatures != 0 {
        result["flashback_sacrifice_creatures"] = json!(value.flashback_sacrifice_creatures);
    }
    if value.flashback_tap_white_creatures != 0 {
        result["flashback_tap_white_creatures"] = json!(value.flashback_tap_white_creatures);
    }
    if let Some(amount) = &value.printed_toxic {
        result["printed_toxic"] = json!(amount);
    }
    if !value.loyalty_costs.is_empty() {
        result["loyalty_costs"] = json!(
            value
                .loyalty_costs
                .iter()
                .map(|(id, cost)| (id.0, cost))
                .collect::<BTreeMap<_, _>>()
        );
    }
    if !value.activation_restrictions.is_empty() {
        result["activation_restrictions"] = json!(
            value
                .activation_restrictions
                .iter()
                .map(|(id, rule)| (id.0, format!("{rule:?}")))
                .collect::<BTreeMap<_, _>>()
        );
    }
    if !value.face_up_abilities.is_empty() {
        result["face_up_abilities"] = face_up_abilities(&value.face_up_abilities);
    }
    if !value.printed_keywords.is_empty() {
        result["printed_keywords"] = keywords(&value.printed_keywords);
    }
    if value.graveyard_exile_x_cost {
        result["graveyard_exile_x_cost"] = json!(true);
    }
    if value.graveyard_creature_exile_cost != 0 {
        result["graveyard_creature_exile_cost"] = json!(value.graveyard_creature_exile_cost);
    }
    if let Some(permission) = &value.graveyard_self_cast {
        result["graveyard_self_cast"] = json!(format!("{permission:?}"));
    }
    if !value.zone_replacements.is_empty() {
        result["zone_replacements"] = zone_replacements(&value.zone_replacements);
    }
    if let Some(splice) = &value.splice {
        result["splice"] = json!({format!("{:?}", splice.quality):cost(&splice.cost)});
    }
    if let Some(split) = &value.split {
        result["split"] = if split.fuse {
            serde_json::json!({"left":view(&split.left), "right":view(&split.right), "aftermath":split.aftermath, "fuse":true})
        } else {
            serde_json::json!({"left":view(&split.left), "right":view(&split.right), "aftermath":split.aftermath})
        };
    }
    if let Some(adventure) = &value.adventure {
        result["adventure"] = view(adventure);
    }
    if let Some(printed) = &value.printed_defense {
        result["printed_defense"] = json!(printed);
    }
    if let Some(partner) = value.meld_partner.as_ref() {
        result["meld_partner"] = json!(partner);
    }
    if let Some(disturb) = value.disturb.as_ref() {
        result["disturb"] = json!({"cost":cost(&disturb.cost),"back":view(&disturb.back)});
    }
    if let Some(back) = value.siege_back.as_deref() {
        result["siege_back"] = view(back);
    }
    if let Some(back) = value.nightbound_back.as_deref() {
        result["nightbound_back"] = view(back);
    }
    if let Some(back) = value.transform_back.as_deref() {
        result["transform_back"] = view(back);
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

    #[test]
    fn wave10_player_turn_condition_and_amount_codec_preserve_roles() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for role in [
            ResolvingPlayer::Controller,
            ResolvingPlayer::UniqueRelatedPlayer,
            ResolvingPlayer::TargetPlayer,
        ] {
            let expr = ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(role));
            let encoded = instruction_amount(&expr);
            let decoded: Amount = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(decoded.native(&vocabulary).unwrap(), expr);
            let condition = ResolutionCondition {
                left: expr,
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            };
            let decoded: ResolutionConditionWire =
                serde_json::from_value(resolution_condition(&condition)).unwrap();
            assert_eq!(decoded.native(&vocabulary).unwrap(), condition);
            let mut invalid = encoded;
            invalid["unexpected"] = json!(true);
            assert!(serde_json::from_value::<Amount>(invalid.clone()).is_err());
            assert!(serde_json::from_value::<ConditionValueWire>(invalid).is_err());
        }
    }

    #[test]
    fn wave11_distinct_stat_codec_preserves_statistic_and_filter() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        for filter in [
            ObjectFilter::creature(),
            ObjectFilter::creature().controlled(),
            ObjectFilter::creature().opposing(),
        ] {
            for stat in [
                ObjectStatKind::Power,
                ObjectStatKind::Toughness,
                ObjectStatKind::ManaValue,
                ObjectStatKind::Loyalty,
            ] {
                let expr = ValueExpr::aggregate(filter, stat, StatAggregate::CountDistinct);
                let encoded = instruction_amount(&expr);
                let decoded: Amount = serde_json::from_value(encoded.clone()).unwrap();
                assert_eq!(decoded.native(&vocabulary).unwrap(), expr);
                let condition = ResolutionCondition {
                    left: expr,
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(3),
                };
                let decoded: ResolutionConditionWire =
                    serde_json::from_value(resolution_condition(&condition)).unwrap();
                assert_eq!(decoded.native(&vocabulary).unwrap(), condition);
                let mut invalid = encoded;
                invalid["unexpected"] = json!(true);
                assert!(serde_json::from_value::<Amount>(invalid.clone()).is_err());
                assert!(serde_json::from_value::<ConditionValueWire>(invalid).is_err());
            }
        }
    }

    #[test]
    fn wave11_distinct_stat_codec_refuses_lossy_subtype_projection() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let elf = vocabulary.resolve_label("Elf").unwrap();
        let exact = ObjectFilter::creature().controlled().with_subtype(elf);
        let expr = ValueExpr::aggregate(exact, ObjectStatKind::Power, StatAggregate::CountDistinct);
        let decoded: Amount = serde_json::from_value(instruction_amount(&expr)).unwrap();
        assert_eq!(decoded.native(&vocabulary).unwrap(), expr);
        let narrowed = ValueExpr::aggregate(
            exact.tapped(),
            ObjectStatKind::Power,
            StatAggregate::CountDistinct,
        );
        let decoded: Amount = serde_json::from_value(instruction_amount(&narrowed)).unwrap();
        assert!(
            decoded.native(&vocabulary).is_err(),
            "unsupported tapped subtype must never broaden to all matching creatures"
        );
    }

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
                    BodyTarget::single_spell(SpellFilter::any())
                }
                ResolvingPlayer::TargetPlayer => BodyTarget::SINGLE_PLAYER,
                ResolvingPlayer::HighestEntrantController
                | ResolvingPlayer::TriggeringAttackerController
                | ResolvingPlayer::CombatDamageSourceController
                | ResolvingPlayer::Controller
                | ResolvingPlayer::UniqueRelatedPlayer => BodyTarget::None,
                ResolvingPlayer::ExiledTargetController
                | ResolvingPlayer::DestructionTargetController
                | ResolvingPlayer::TargetController
                | ResolvingPlayer::TargetOwner
                | ResolvingPlayer::ActivePlayer
                | ResolvingPlayer::EnchantedPlayer
                | ResolvingPlayer::AttachedHostController
                | ResolvingPlayer::TriggerSubjectController
                | ResolvingPlayer::TriggerSubjectOwner
                | ResolvingPlayer::SourceChosenPlayer
                | ResolvingPlayer::DefendingPlayer
                | ResolvingPlayer::ChosenPlayer
                | ResolvingPlayer::LoopPlayer
                | ResolvingPlayer::RandomOpponent
                | ResolvingPlayer::SourceOwner => {
                    unreachable!("role is not in this fixed fixture")
                }
            };
            let primitive = match role {
                ResolvingPlayer::TargetSpellController => Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                )),
                ResolvingPlayer::TargetPlayer => Instruction::AdjustLife(Box::new(
                    spellwake_engine::toolbox::AdjustLifeSpec::gain(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                        ),
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                )),
                ResolvingPlayer::HighestEntrantController
                | ResolvingPlayer::TriggeringAttackerController
                | ResolvingPlayer::CombatDamageSourceController
                | ResolvingPlayer::Controller
                | ResolvingPlayer::UniqueRelatedPlayer => Instruction::AdjustLife(Box::new(
                    spellwake_engine::toolbox::AdjustLifeSpec::gain(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                )),
                ResolvingPlayer::ExiledTargetController
                | ResolvingPlayer::DestructionTargetController
                | ResolvingPlayer::TargetController
                | ResolvingPlayer::TargetOwner
                | ResolvingPlayer::ActivePlayer
                | ResolvingPlayer::EnchantedPlayer
                | ResolvingPlayer::AttachedHostController
                | ResolvingPlayer::TriggerSubjectController
                | ResolvingPlayer::TriggerSubjectOwner
                | ResolvingPlayer::SourceChosenPlayer
                | ResolvingPlayer::DefendingPlayer
                | ResolvingPlayer::ChosenPlayer
                | ResolvingPlayer::LoopPlayer
                | ResolvingPlayer::RandomOpponent
                | ResolvingPlayer::SourceOwner => {
                    unreachable!("role is not in this fixed fixture")
                }
            };
            for instructions in [
                vec![
                    Instruction::OptionalSequence {
                        chooser: role,
                        instructions: 1,
                    },
                    primitive,
                ],
                vec![Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        role,
                        quoted.clone(),
                        ValueExpr::Literal(0.into()),
                        1,
                        1,
                    ),
                ))],
            ] {
                let body = ExecutableBody::new(target, instructions);
                let (program, body_pointer) = if role == ResolvingPlayer::UniqueRelatedPlayer {
                    let mut program = Program::artifact(None, vec![]);
                    program.triggered.push(TriggeredAbility {
                        id: TriggeredAbilityId(1),
                        condition: TriggerSpec::spell_cast(
                            PlayerRelation::AnyParticipant,
                            SpellFilter::any(),
                        ),
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
            ValueExpr::Literal(0.into()),
            ValueExpr::Literal(huge.clone()),
            ValueExpr::count_battlefield(
                ObjectFilter::any()
                    .controlled()
                    .of_chosen_slot(CreatureTypeSlot::new(0)),
            ),
            ValueExpr::event(EventValueKind::MatchedOccurrences),
        ];
        for players in scopes {
            for amount in &amounts {
                let target = match players {
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer) => {
                        BodyTarget::SINGLE_PLAYER
                    }
                    AffectedPlayers::One(ResolvingPlayer::TargetSpellController) => {
                        BodyTarget::single_spell(SpellFilter::any())
                    }
                    AffectedPlayers::One(
                        ResolvingPlayer::HighestEntrantController
                        | ResolvingPlayer::TriggeringAttackerController
                        | ResolvingPlayer::CombatDamageSourceController
                        | ResolvingPlayer::Controller
                        | ResolvingPlayer::UniqueRelatedPlayer,
                    )
                    | AffectedPlayers::EachOtherPlayer
                    | AffectedPlayers::EachPlayer
                    | AffectedPlayers::EachOpponent => BodyTarget::None,
                    AffectedPlayers::One(
                        ResolvingPlayer::ExiledTargetController
                        | ResolvingPlayer::DestructionTargetController
                        | ResolvingPlayer::TargetController
                        | ResolvingPlayer::TargetOwner
                        | ResolvingPlayer::ActivePlayer
                        | ResolvingPlayer::EnchantedPlayer
                        | ResolvingPlayer::AttachedHostController
                        | ResolvingPlayer::TriggerSubjectController
                        | ResolvingPlayer::TriggerSubjectOwner
                        | ResolvingPlayer::SourceChosenPlayer
                        | ResolvingPlayer::DefendingPlayer
                        | ResolvingPlayer::ChosenPlayer
                        | ResolvingPlayer::LoopPlayer
                        | ResolvingPlayer::RandomOpponent
                        | ResolvingPlayer::SourceOwner,
                    ) => unreachable!("role is not in this fixed fixture"),
                };
                let mut instructions = vec![];
                if matches!(
                    amount,
                    ValueExpr::Count {
                        zone: ValueZone::Battlefield,
                        ..
                    }
                ) {
                    instructions.push(Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                            CreatureTypeSlot::new(0),
                            BTreeSet::new(),
                        ),
                    )));
                }
                instructions.extend([
                    Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                            players,
                            amount.clone(),
                            PermanentTypeFilter {
                                any_types: Default::default(),
                                required_types: BTreeSet::from([
                                    CardType::Artifact,
                                    CardType::Creature,
                                ]),
                                forbidden_types: BTreeSet::from([
                                    CardType::Land,
                                    CardType::Enchantment,
                                ]),
                                object_filter: None,
                            },
                        ),
                    )),
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                        players,
                        amount.clone(),
                    ))),
                ]);
                let mut program = Program::artifact(None, vec![]);
                program.triggered.push(TriggeredAbility {
                    id: TriggeredAbilityId(1),
                    condition: TriggerSpec::spell_cast(
                        PlayerRelation::AnyParticipant,
                        SpellFilter::any(),
                    ),
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
                if let ValueExpr::Literal(value) = amount {
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
                BodyTarget::SINGLE_PLAYER,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        ValueExpr::Literal(2.into()),
                    ),
                ))],
            ),
        });
        leaf.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(7),
            condition: TriggerSpec::self_enters(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                        AffectedPlayers::EachPlayer,
                        ValueExpr::event(EventValueKind::MatchedOccurrences),
                        PermanentTypeFilter {
                            any_types: Default::default(),
                            required_types: BTreeSet::from([CardType::Creature]),
                            forbidden_types: BTreeSet::new(),
                            object_filter: None,
                        },
                    ),
                ))],
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
            condition: TriggerSpec::self_enters(),
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
            (
                "/kind/body/instructions/0/players",
                json!({"tag":"EachOpponent"}),
            ),
            ("/kind/body/instructions/0/amount/value", json!("2")),
            (
                "/kind/body/instructions/0/filter/required_types",
                json!(["Artifact"]),
            ),
            (
                "/kind/body/instructions/0/filter/required_types",
                json!(["Planeswalker"]),
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
                json!({"tag":"EachOpponent","player":"Controller"}),
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
            json!({"required_types":["Invalid"],"forbidden_types":[]}),
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
        let chooser = json!({"op":"OptionalEffect","chooser":"Controller"});
        let discard_one = selection_instruction("Discard");
        let allowed = optional_payment_definition(json!([chooser.clone(), discard_one.clone()]));
        assert_eq!(
            view(&decode(allowed.clone(), &vocabulary).unwrap()),
            allowed
        );

        // Multi-player selection effects remain rejected inside OptionalEffect.
        let mut each_discard = discard_one;
        each_discard["players"] = json!({"tag":"EachPlayer"});
        let mut each_sacrifice = selection_instruction("Sacrifice");
        each_sacrifice["players"] = json!({"tag":"EachPlayer"});
        for instruction in [each_sacrifice, each_discard] {
            let raw = optional_payment_definition(json!([chooser.clone(), instruction]));
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
                BodyTarget::SINGLE_PERMANENT,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec {
                        selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets),
                        payload: spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(
                            Box::new(ResolvedPartsSpec(vec![ResolvedPartSpec::Names(
                                NameOperation::Set(ObjectNames::nameless()),
                            )])),
                        ),
                        duration: ResolvedDuration::NoStatedDuration,
                        condition: None,
                        one_shot: false,
                        as_emblem: None,
                        emblem_label: None,
                    },
                ))],
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
            TriggerSpec::player_action(PlayerRelation::Opponent, PlayerTriggerAction::Scried),
            TriggerSpec::player_action(PlayerRelation::Controller, PlayerTriggerAction::Surveilled),
        ] {
            let mut native = Program::artifact(None, vec![]);
            native.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(0),
                condition,
                grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        spellwake_engine::toolbox::AdjustLifeSpec::gain(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                    ))],
                ),
            });
            assert_eq!(decode(view(&native), &vocabulary).unwrap(), native);
        }
    }
}
