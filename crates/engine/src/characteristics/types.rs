use super::{
    CastPermissionRule, CharacteristicState, Color, ColorFilter, ColorOperation, SpellCostModifier,
    StaticColorOperand, StaticContinuousAbility, StaticDamageRule, StaticRestriction,
};
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
    UntilEndOfCombat,
    UntilEndOfTurn,
    /// Authored duration bound to the resolving controller's next turn start.
    UntilControllerNextTurn,
    /// Materialized duration expiring at the start of `player`'s first turn after `created_in_turn`.
    UntilPlayerNextTurn {
        player: SeatId,
        created_in_turn: u64,
    },
    /// Authored duration bound to the resolving controller's next turn cleanup.
    ThroughControllerNextTurn,
    /// Authored duration bound to the actual source and its current control history.
    WhileControllingAbilitySource,
    /// Retained duration for a specific incarnation that must stay under continuous control.
    WhileSourceControlled {
        source: ObjectId,
        controller: SeatId,
        epoch: crate::object_identity::ControlEpoch,
    },
    /// Authored duration lasting while the source remains on the battlefield (CR 611.2b).
    WhileAbilitySourceOnBattlefield,
    /// Materialized duration lasting while the specific source incarnation stays on the battlefield.
    WhileSourceOnBattlefield {
        source: ObjectId,
    },
    /// Authored duration lasting while the source remains tapped on the battlefield (CR 611.2b).
    WhileAbilitySourceTapped,
    /// Materialized duration lasting while the specific source incarnation stays tapped on the battlefield.
    WhileSourceTapped {
        source: ObjectId,
    },
    /// Authored duration lasting while you control the source and it remains tapped (CR 611.2b).
    WhileControllingTappedAbilitySource,
    /// Materialized duration lasting while the source stays under continuous control and tapped.
    WhileSourceControlledAndTapped {
        source: ObjectId,
        controller: SeatId,
        epoch: crate::object_identity::ControlEpoch,
    },
    /// Expire during the given player's first cleanup after the creation turn.
    ThroughNextTurn {
        player: SeatId,
        created_in_turn: u64,
    },
}
impl ResolvedDuration {
    /// Whether this authored duration requires an ability source permanent to bind against.
    #[must_use]
    pub const fn requires_ability_source(self) -> bool {
        matches!(
            self,
            Self::WhileControllingAbilitySource
                | Self::WhileAbilitySourceOnBattlefield
                | Self::WhileAbilitySourceTapped
                | Self::WhileControllingTappedAbilitySource
        )
    }

    /// Whether this is a runtime-materialized duration that must not appear in authored programs.
    #[must_use]
    pub const fn is_materialized_source_bound(self) -> bool {
        matches!(
            self,
            Self::WhileSourceControlled { .. }
                | Self::WhileSourceOnBattlefield { .. }
                | Self::WhileSourceTapped { .. }
                | Self::WhileSourceControlledAndTapped { .. }
        )
    }

    pub(crate) fn expires_at_cleanup(self, active: SeatId, turn: u64) -> bool {
        match self {
            Self::NoStatedDuration
            | Self::UntilControllerNextTurn
            | Self::UntilPlayerNextTurn { .. }
            | Self::ThroughControllerNextTurn
            | Self::WhileControllingAbilitySource
            | Self::WhileSourceControlled { .. }
            | Self::WhileAbilitySourceOnBattlefield
            | Self::WhileSourceOnBattlefield { .. }
            | Self::WhileAbilitySourceTapped
            | Self::WhileSourceTapped { .. }
            | Self::WhileControllingTappedAbilitySource
            | Self::WhileSourceControlledAndTapped { .. } => false,
            Self::UntilEndOfTurn | Self::UntilEndOfCombat => true,
            Self::ThroughNextTurn {
                player,
                created_in_turn,
            } => player == active && turn > created_in_turn,
        }
    }

    pub(crate) fn expires_at_end_of_combat(self) -> bool {
        matches!(self, Self::UntilEndOfCombat)
    }

    pub(crate) fn expires_at_turn_start(self, active: SeatId, turn: u64) -> bool {
        matches!(
            self,
            Self::UntilPlayerNextTurn {
                player,
                created_in_turn,
            } if player == active && turn > created_in_turn
        )
    }
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
    /// A one-shot layer-six loss; later grants can restore these keywords.
    Banding,
    BandsWithOther,
    Flying,
    All,
    NonMana,
}
/// A protection quality involving a creature subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubtypeProtection {
    pub subtype: CreatureTypeId,
    /// Some abilities say "creatures of the chosen type", excluding noncreature kindred sources.
    pub creatures_only: bool,
    /// Match sources without this subtype when protection excludes a subtype.
    pub exclude_subtype: bool,
}
impl SubtypeProtection {
    pub fn matches(self, subtypes: &BTreeSet<CreatureTypeId>, is_creature: bool) -> bool {
        (!self.creatures_only || is_creature)
            && subtypes.contains(&self.subtype) != self.exclude_subtype
    }
}

/// A quality from which a permanent has conditional hexproof (CR 702.11e).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HexproofQuality {
    Color(Color),
    Monocolored,
    Multicolored,
    CardType(CardType),
    Subtype(CreatureTypeId),
    ActivatedOrTriggeredAbility,
}

/// Additional orthogonal protection qualities beyond the 5 colors, multicolored, and subtypes (CR 702.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtectionFromQuality {
    Colorless,
    Monocolored,
    CardType(CardType),
    Everything,
    Player(crate::priority::SeatId),
}

/// Implemented static keywords. Unsupported keywords cannot enter this enum as
/// arbitrary text or silently acquire partial behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeywordAbility {
    /// CR 702.124h, the unqualified partner ability.
    Partner,
    /// CR 702.124h, commander pairing with a Background enchantment.
    ChooseABackground,
    /// CR 702.124m, commander pairing with a Time Lord Doctor.
    DoctorsCompanion,
    Bargain,
    /// CR 702.51 allows creatures to help pay a spell's cost.
    Convoke,
    /// CR 702.126 allows artifacts to pay generic mana in the total cost.
    Improvise,
    ProtectionFromWhite,
    ProtectionFromBlue,
    ProtectionFromBlack,
    ProtectionFromRed,
    ProtectionFromGreen,
    ProtectionFromMulticolored,
    ProtectionFromSubtype(SubtypeProtection),
    ProtectionFrom(ProtectionFromQuality),
    /// This source's damage cannot be prevented.
    DamageCannotBePrevented,
    Retrace,
    JumpStart,
    Rebound,
    /// CR 702.66 allows cards in the graveyard to pay generic mana.
    Delve,
    /// Bypass the continuous-control restriction for attacking and tap symbols.
    Haste,
    /// Authorizes attacking bands and changes who assigns opposing combat damage.
    Banding,
    BandsWithOther(crate::combat::BandQuality),
    /// Declaring an attack does not tap this creature.
    Vigilance,
    /// Only flying or reach can block this attacker.
    Flying,
    /// Allows blocking a flying attacker.
    Reach,
    /// Prevents the creature from attacking.
    Daybound,
    Nightbound,
    /// Phases out during its controller's untap step (CR 502.1, 702.26a).
    Phasing,
    Defender,
    /// This permanent cannot block (CR 702.147).
    Decayed,
    /// Requires at least two blockers when blocked.
    Menace,
    /// Restricts combat to other creatures with shadow.
    Shadow,
    /// Only a creature with horsemanship can block this attacker.
    Horsemanship,
    /// Only black or artifact creatures can block this attacker.
    Fear,
    /// Only artifacts or creatures sharing a color can block this attacker.
    Intimidate,
    /// A creature with greater power cannot block this attacker.
    Skulk,
    /// Landwalk for the Plains subtype.
    Plainswalk,
    /// Landwalk for the Island subtype.
    Islandwalk,
    /// Landwalk for the Swamp subtype.
    Swampwalk,
    /// Landwalk for the Mountain subtype.
    Mountainwalk,
    /// Landwalk for the Forest subtype.
    Forestwalk,
    /// Landwalk for the Desert subtype.
    Desertwalk,
    /// Landwalk for nonbasic lands.
    NonbasicLandwalk,
    /// Landwalk for legendary lands.
    LegendaryLandwalk,
    /// Landwalk for a snow land.
    SnowLandwalk,
    /// Landwalk for a snow Forest.
    SnowForestwalk,
    /// Landwalk for a snow Swamp.
    SnowSwampwalk,
    /// Landwalk for an artifact land.
    ArtifactLandwalk,
    /// Allows excess combat damage to be assigned to the defender.
    Trample,
    /// A positive amount of damage from this source counts as lethal to a creature.
    Deathtouch,
    /// Damage dealt by this source causes its controller or owner to gain that much life.
    Lifelink,
    /// Prevents destruction of this permanent.
    Indestructible,
    /// Deals damage to creatures in the form of -1/-1 counters.
    Wither,
    /// Deals damage as poison to players and as -1/-1 counters to creatures.
    Infect,
    /// Participates in the first combat damage step.
    FirstStrike,
    /// Participates in both combat damage steps.
    DoubleStrike,
    /// Prevents any player from targeting the permanent.
    Shroud,
    /// Prevents opponents from targeting the permanent.
    Hexproof,
    /// CR 702.11e: prevents opponents' spells/abilities with the matching quality from targeting this permanent.
    HexproofFrom(HexproofQuality),
    /// CR 702.89: if enchanted permanent would be destroyed, instead remove all damage from it and destroy this Aura.
    UmbraArmor,
    /// Allows this spell to be cast whenever its controller could cast an instant.
    Flash,
    /// Restricts casting and nonmana activations while this spell is on the stack.
    SplitSecond,
    /// Defines every creature type in all zones.
    Changeling,
    /// Defines the object as colorless in all zones.
    Devoid,
    /// CR 702.179. A state-based action starts the controller at speed one.
    StartYourEngines,
}
/// Scope of activated abilities suppressed on a permanent by a layer-6 ability effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSuppression {
    /// Permanent's activated abilities cannot be activated (including mana abilities).
    All,
    /// Permanent's activated abilities cannot be activated unless they're mana abilities.
    NonMana,
}
/// One layer-6 component: remove first, then grant the specified keywords. Both
/// operations share this effect's timestamp, duration and affected set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbilityChange {
    /// Optional removal of the admitted ability families, including keywords.
    pub removal: Option<AbilityRemoval>,
    /// Redundant instances of these keywords do not stack (CR 702.10d/702.20c).
    pub granted_keywords: BTreeSet<KeywordAbility>,
    /// Lose these keywords and suppress later grants while this effect lasts.
    pub suppressed_keywords: BTreeSet<KeywordAbility>,
    /// Suppress activating abilities of this permanent while this effect lasts.
    pub suppressed_activations: Option<ActivationSuppression>,
    /// This permanent doesn't untap during its controller's untap step while this effect lasts.
    pub doesnt_untap: bool,
    /// Separate instances of the triggered ability, each with its own stack entry.
    pub granted_exalted: u16,
    /// Grants the static ability that this creature cannot be blocked.
    pub grants_unblockable: bool,
    /// Triggered ability granted by the same layer-six effect as the keywords.
    pub granted_trigger: Option<Box<crate::program::TriggeredAbility>>,
    /// Total value of toxic instances granted by this effect, if any.
    pub granted_toxic: Option<RulesInteger>,
    /// Ordered, independently triggering keyword instances granted by this effect.
    pub granted_combat_keywords: Vec<crate::program::CombatTriggerKeyword>,
}
impl AbilityChange {
    pub(super) fn validate_grants(&self) -> Result<(), CharacteristicDefinitionError> {
        if self
            .granted_toxic
            .as_ref()
            .is_none_or(|amount| amount >= &0.into())
            && self
                .granted_combat_keywords
                .iter()
                .all(crate::program::CombatTriggerKeyword::valid)
        {
            Ok(())
        } else {
            Err(CharacteristicDefinitionError::InvalidKeywordGrant)
        }
    }

    /// Construct removal with no subsequent keyword grant.
    pub fn remove(removal: AbilityRemoval) -> Self {
        Self {
            removal: Some(removal),
            ..Self::default()
        }
    }
    /// Construct a keyword grant with no removal.
    pub fn grant_keywords(keywords: impl IntoIterator<Item = KeywordAbility>) -> Self {
        Self {
            granted_keywords: keywords.into_iter().collect(),
            ..Self::default()
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
    SourceEntryCreatureType,
    ImprintedCreatureTypes {
        keep: BTreeSet<CreatureTypeId>,
    },
    Literal(BTreeSet<CreatureTypeId>),
    SourceCurrentSubtypes,
    /// CR 205.3m: every creature type in the game vocabulary (e.g., Runed Stalactite, Maskwood Nexus).
    AllCreatureTypes,
}
/// A land operand regenerated from the current scratch world in layer 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticLandSubtypeOperand {
    Literal(BTreeSet<LandType>),
    SourceCurrentLandSubtypes,
    /// Basic land type chosen via [`crate::land::EntryTap::ChooseBasicLandType`] as the source entered.
    SourceEntryBasicLandType,
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
/// A restriction on which creatures may block an attacker; this is not an ability grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingRestriction {
    CannotAttackOrBlock,
    CannotAttack,
    CannotBlock,
    AtMostOneBlocker,
    CanBlockOnlyFlying,
    CannotBlockSubtype(CreatureTypeId),
    CannotBeBlockedByFlying,
    OnlyBlockedByFlyingOrReach,
    CannotBeBlockedBySubtype(CreatureTypeId),
    CannotBeBlockedByPowerAtMost(u16),
    CannotBeBlockedByPowerAtLeast(u16),
    CannotBeBlockedByColor(Color),
    OnlyBlockedByColor(Color),
    OnlyBlockedByColorOrArtifact(Color),
    CannotBeBlockedByTokens,
    OnlyBlockedByArtifact,
    CannotBeBlockedByNonArtifact,
    /// Authored static restriction anchored to the source that grants it.
    BlockerPowerAtLeastSource,
    /// The exact source incarnation bound when a static ability applies.
    BlockerPowerAtLeastPowerOf(ObjectId),
    /// Each active instance adds one to this object's blocking capacity.
    AdditionalBlock,
    RequiresSoulbondPartner,
    CannotBeBlocked,
    OnlyBlockedBySubtype(CreatureTypeId),
    CannotBeBlockedByToughnessAtLeast(u16),
    AtLeastThreeBlockers,
    AllDefendingCreaturesMustBlock,
    /// At least one eligible blocker must block this attacker when possible.
    MustBeBlocked,
    CannotBeBlockedIfPowerOrToughnessAtMostOne,
    /// Blocker cannot block attackers matching this filter (CR 509.1b).
    CannotBlockMatching(crate::program::ObjectFilter),
    /// Attacker cannot be blocked by creatures matching this filter (CR 509.1b).
    CannotBeBlockedByMatching(crate::program::ObjectFilter),
    /// Attacker can be blocked only by creatures matching this filter (CR 509.1b).
    OnlyBlockedByMatching(crate::program::ObjectFilter),
    /// Creature cannot attack unless at least one other creature attacks alongside it (CR 508.1d).
    CannotAttackAlone,
    /// Creature cannot block unless at least one other creature blocks alongside it (CR 509.1b).
    CannotBlockAlone,
    /// Creature assigns combat damage equal to its toughness rather than its power (CR 510.1a).
    AssignDamageByToughness,
    /// Controller may assign this blocked attacker's combat damage as though it weren't blocked.
    AssignDamageAsThoughUnblocked,
    /// Creature can block any number of creatures (CR 509.1b).
    CanBlockAnyNumber,
    /// Creature can attack as though it didn't have defender (CR 702.3b).
    CanAttackAsThoughNoDefender,
    /// Creature must block each combat if able (CR 509.1c).
    MustBlockEachCombatIfAble,
    /// Authored restriction: affected blocker cannot block the ability source.
    CannotBlockSource,
    /// Resolved restriction: affected blocker cannot block this exact attacking object.
    CannotBlockObject(ObjectId),
    /// Attacker cannot attack a defending player unless that player controls a matching permanent.
    CannotAttackUnlessDefenderControls(crate::program::ObjectFilter),
    /// Authored restriction: affected blocker must block the ability source if able (CR 509.1c).
    MustBlockSource,
    /// Resolved restriction: affected blocker must block this exact attacking object if able.
    MustBlockObject(ObjectId),
    /// Authored restriction: affected attacker cannot be blocked by the ability source.
    CannotBeBlockedBySource,
    /// Resolved restriction: affected attacker cannot be blocked by this exact blocking object.
    CannotBeBlockedByObject(ObjectId),
    /// Blocker can block only attackers matching this filter (CR 509.1b).
    CanBlockOnlyMatching(crate::program::ObjectFilter),
    /// Authored restriction: affected creature cannot attack the source controller or their planeswalkers.
    CannotAttackSourceController,
    /// Resolved restriction: affected creature cannot attack this player or their planeswalkers.
    CannotAttackPlayer(SeatId),
    /// Attacker cannot be blocked by creatures with greater power (CR 701.52c Ring-bearer level 1).
    CannotBeBlockedByGreaterPower,
    /// All defending creatures matching `filter` able to block this attacker must do so (CR 509.1c).
    MustBeBlockedByAllMatching(crate::program::ObjectFilter),
    /// Blocker cannot block more than `n` creatures each combat (CR 509.1b).
    CannotBlockMoreThan(u8),
    /// Attacker cannot be blocked by more than `n` creatures (CR 509.1b).
    CannotBeBlockedByMoreThan(u8),
    /// Creature assigns no combat damage (CR 510.1a).
    AssignNoCombatDamage,
    /// Permanent cannot be regenerated (CR 701.19c).
    CannotBeRegenerated,
    /// Blocker must block each attacking creature this turn if able (CR 509.1c, Blaze of Glory).
    MustBlockAllAttackers,
}
// Embeds `Copy` `BlockingRestriction` inline to avoid indirection in small resolved part lists.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPart {
    /// Captured at resolution, never a live donor lookup. Only the admitted
    /// partial Program language is represented; this is not full card metadata.
    Copy(Box<Program>),
    Control(SeatId),
    Text(CreatureTypeTextChange),
    Names(NameOperation<ObjectNames>),
    Type(TypeChange),
    TypeWithSubtypes {
        base: TypeChange,
        artifact_subtypes: SubtypeOperation<
            BTreeSet<crate::program::ArtifactSubtype>,
            crate::program::ArtifactSubtype,
        >,
        enchantment_subtypes: SubtypeOperation<
            BTreeSet<crate::program::EnchantmentSubtype>,
            crate::program::EnchantmentSubtype,
        >,
    },
    Color(ColorOperation<BTreeSet<Color>>),
    /// A layer-six effect granting one independently captured triggered ability.
    GrantTrigger(Box<TriggeredAbility>),
    GrantActivated(Box<crate::activated::ActivatedAbility>),
    GrantActivatedRestricted {
        ability: Box<crate::activated::ActivatedAbility>,
        restriction: Box<crate::activated::ActivationRestriction>,
    },
    InheritedAbilities(Box<super::InheritedAbilities>),
    /// Grant an independent tap ability that adds one mana of any color.
    GrantAnyColorMana,
    GrantFixedColorMana(crate::mana::ManaType),
    /// Grant an independent tap mana ability producing any orthogonal `ManaOutput`.
    GrantMana(Box<crate::mana::ManaOutput>),
    Abilities(AbilityChange),
    BlockingRestriction(BlockingRestriction),
    /// Multiple Layer 6 ability parts bundled by characteristic evaluation for a single effect.
    CompoundAbilities(Vec<ResolvedPart>),
    SetPT(PowerToughness),
    SetToughness(RulesInteger),
    ModifyPT(PowerToughness),
    SwitchPT,
}
impl ResolvedPart {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::Copy(_) => CharacteristicBucket::Copy,
            Self::Control(_) => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) | Self::TypeWithSubtypes { .. } => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::InheritedAbilities(_)
            | Self::GrantActivated(_)
            | Self::GrantActivatedRestricted { .. }
            | Self::GrantTrigger(_)
            | Self::GrantAnyColorMana
            | Self::GrantFixedColorMana(_)
            | Self::GrantMana(_)
            | Self::Abilities(_)
            | Self::BlockingRestriction(_)
            | Self::CompoundAbilities(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) | Self::SetToughness(_) => CharacteristicBucket::SetPT,
            Self::ModifyPT(_) => CharacteristicBucket::ModifyPT,
            Self::SwitchPT => CharacteristicBucket::SwitchPT,
        }
    }
}
// Embeds `Copy` `BlockingRestriction` inline to match `ResolvedPart` without extra boxing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPartSpec {
    Control(ControlRecipient),
    Text(CreatureTypeTextChangeSpec),
    Names(NameOperation<ObjectNames>),
    Type(TypeChangeSpec),
    TypeWithSubtypes {
        base: TypeChangeSpec,
        artifact_subtypes: SubtypeOperation<
            BTreeSet<crate::program::ArtifactSubtype>,
            crate::program::ArtifactSubtype,
        >,
        enchantment_subtypes: SubtypeOperation<
            BTreeSet<crate::program::EnchantmentSubtype>,
            crate::program::EnchantmentSubtype,
        >,
    },
    Color(ColorOperation<BTreeSet<Color>>),
    GrantTrigger(Box<TriggeredAbility>),
    GrantActivated(Box<crate::activated::ActivatedAbility>),
    GrantActivatedRestricted {
        ability: Box<crate::activated::ActivatedAbility>,
        restriction: Box<crate::activated::ActivationRestriction>,
    },
    GrantAnyColorMana,
    GrantFixedColorMana(crate::mana::ManaType),
    GrantMana(Box<crate::mana::ManaOutput>),
    Abilities(AbilityChange),
    BlockingRestriction(BlockingRestriction),
    SetPT(PowerToughness),
    ModifyPT(PowerToughness),
    /// Dynamic +P/+T modification evaluated at resolution time (CR 611.2c).
    ModifyPTExpr {
        power: Box<crate::program::ValueExpr>,
        toughness: Box<crate::program::ValueExpr>,
    },
    /// Dynamic base P/T setter evaluated at resolution time (CR 611.2c).
    SetPTExpr {
        power: Box<crate::program::ValueExpr>,
        toughness: Box<crate::program::ValueExpr>,
    },
    SwitchPT,
}
impl ResolvedPartSpec {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::Control(_) => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) | Self::TypeWithSubtypes { .. } => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::GrantTrigger(_)
            | Self::GrantActivated(_)
            | Self::GrantActivatedRestricted { .. }
            | Self::GrantAnyColorMana
            | Self::GrantFixedColorMana(_)
            | Self::GrantMana(_)
            | Self::Abilities(_)
            | Self::BlockingRestriction(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) | Self::SetPTExpr { .. } => CharacteristicBucket::SetPT,
            Self::ModifyPT(_) | Self::ModifyPTExpr { .. } => CharacteristicBucket::ModifyPT,
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
    /// Only the source permanent while its actual controller has max speed.
    SourceItself,
    SelfAtMaxSpeed,
    /// The source and its current creature partner, when both are present.
    SourcePair,
    Any,
    SourceController,
    Opponents,
    EnchantedPlayer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatSelector {
    Attacking,
    Blocking,
    AttackingOrBlocking,
    OutsideCombat,
    Blocked,
    Unblocked,
    BlockingSource,
    BlockedBySource,
    BlockingOrBlockedBySource,
    NotAttacking,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentSelector {
    /// Require that a recipient be attacking, blocking, in combat, or outside combat.
    pub combat_role: Option<CombatSelector>,
    /// Require this permanent to be untapped.
    pub requires_untapped: bool,
    /// Require this permanent to be tapped.
    pub requires_tapped: bool,
    /// Require this creature to be the only creature its controller controls.
    pub sole_controlled_creature: bool,
    /// Match token or nontoken permanents by their physical identity.
    pub token: Option<bool>,
    pub required_supertypes: BTreeSet<Supertype>,
    pub forbidden_supertypes: BTreeSet<Supertype>,
    pub required_types: BTreeSet<CardType>,
    /// Disjunctive card-type predicate: when nonempty, recipient must have at least one of these types.
    pub any_types: BTreeSet<CardType>,
    pub forbidden_types: BTreeSet<CardType>,
    pub creature_subtype: Option<CreatureTypeId>,
    /// Disjunctive creature-subtype predicate: when nonempty, recipient must have at least one of these subtypes.
    pub any_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub forbidden_creature_subtype: Option<CreatureTypeId>,
    pub land_subtype: Option<LandType>,
    pub color: ColorFilter,
    pub name: NameFilter,
    pub controller: ControllerSelector,
    /// Restrict this selector to the current permanent attached to the source.
    pub attached_to_source: bool,
    /// A station symbol affects its source only once it has the indicated charge counters.
    /// Gate all recipients on the source controller controlling their own commander.
    pub while_controlling_own_commander: bool,
    /// Require a +1/+1 counter on this recipient.
    pub plus_one_counter: bool,
    /// Require a -1/-1 counter on this recipient.
    pub minus_one_counter: bool,
    /// Require at least this many +1/+1 counters on the recipient.
    pub minimum_plus_one_counters: u16,
    /// Require any currently represented counter on this recipient.
    pub any_counter: bool,
    /// Require a live exiled creature card linked to the effect’s source.
    pub source_has_imprinted_creature: bool,
    /// Target the source controller’s creature cards, creature spells and nontoken creatures,
    /// as well as tokens of this subtype that player controls.
    pub controlled_creatures_and_typed_tokens: Option<CreatureTypeId>,
    /// Ignore enchantments that currently have the Aura subtype.
    pub exclude_auras: bool,
    pub artifact_subtype: Option<crate::program::ArtifactSubtype>,
    pub planeswalker_subtype: Option<crate::program::PlaneswalkerSubtype>,
    pub matches_source_entry_type: bool,
    pub controller_type_threshold: Option<(CardType, std::num::NonZeroU16)>,
    /// Additional condition on the source and its evaluated controller.
    pub source_condition: Option<StaticSourceCondition>,
    /// Require flying when evaluating this selector.
    pub requires_flying: bool,
    /// Require a specific keyword ability on this recipient.
    pub required_keyword: Option<KeywordAbility>,
    /// Forbid a specific keyword ability on this recipient.
    pub forbidden_keyword: Option<KeywordAbility>,
    /// Forbid every keyword ability in this set on this recipient (e.g., "without flying or islandwalk").
    pub forbidden_keywords: BTreeSet<KeywordAbility>,
    /// Minimum number of Equipment currently attached to this recipient.
    pub equipped_by_at_least: u16,
    /// Require at least one Aura currently attached to this recipient.
    pub requires_enchanted: bool,
    /// Optional minimum current power required on this recipient.
    pub min_power: Option<i64>,
    /// Optional maximum current power allowed on this recipient.
    pub max_power: Option<i64>,
    /// Optional minimum current toughness required on this recipient.
    pub min_toughness: Option<i64>,
    /// Optional maximum current toughness allowed on this recipient.
    pub max_toughness: Option<i64>,
    /// Optional minimum mana value required on this recipient.
    pub min_mana_value: Option<u32>,
    /// Optional maximum mana value allowed on this recipient.
    pub max_mana_value: Option<u32>,
    /// Restrict recipients while the source controller’s devotion is below the printed threshold.
    pub devotion_below: Option<(Color, u8)>,
    pub station_at: Option<std::num::NonZeroU16>,
    pub exclude_source: bool,
}
impl Default for PermanentSelector {
    fn default() -> Self {
        Self {
            combat_role: None,
            requires_untapped: false,
            requires_tapped: false,
            sole_controlled_creature: false,
            token: None,
            required_supertypes: BTreeSet::new(),
            forbidden_supertypes: BTreeSet::new(),
            required_types: BTreeSet::new(),
            any_types: BTreeSet::new(),
            forbidden_types: BTreeSet::new(),
            creature_subtype: None,
            any_creature_subtypes: BTreeSet::new(),
            forbidden_creature_subtype: None,
            land_subtype: None,
            color: ColorFilter::Any,
            name: NameFilter::Any,
            controller: ControllerSelector::Any,
            attached_to_source: false,
            requires_flying: false,
            required_keyword: None,
            forbidden_keyword: None,
            forbidden_keywords: BTreeSet::new(),
            plus_one_counter: false,
            minus_one_counter: false,
            minimum_plus_one_counters: 0,
            any_counter: false,
            artifact_subtype: None,
            planeswalker_subtype: None,
            matches_source_entry_type: false,
            controller_type_threshold: None,
            equipped_by_at_least: 0,
            requires_enchanted: false,
            min_power: None,
            max_power: None,
            min_toughness: None,
            max_toughness: None,
            min_mana_value: None,
            max_mana_value: None,
            devotion_below: None,
            source_condition: None,
            source_has_imprinted_creature: false,
            controlled_creatures_and_typed_tokens: None,
            exclude_auras: false,
            while_controlling_own_commander: false,
            station_at: None,
            exclude_source: false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSourceCondition {
    ControllerLifeAtMost(u16),
    ControllerLifeAtLeast(u16),
    ControllerControlsSubtype(CreatureTypeId),
    OpponentControlsSubtype(CreatureTypeId),
    OpponentsTurn,
    ControllersTurn,
    HasKeyword(KeywordAbility),
    Delirium,
    Metalcraft,
    Threshold,
    ControlsCardType {
        kind: CardType,
        minimum: u16,
    },
    GraveyardCountAtLeast {
        types: crate::program::CardTypeMask,
        minimum: u16,
    },
    HandCountAtMost(u16),
    HandCountAtLeast(u16),
    SourceCounterAtLeast {
        kind: crate::effects::CounterKind,
        minimum: u16,
    },
    /// Active while this permanent has at least `minimum` total counters of any kind.
    SourceAnyCounterAtLeast(u16),
    /// Active while at least one Equipment is attached to this permanent.
    SourceIsEquipped,
    /// Active while at least one Aura enchantment is attached to this permanent.
    SourceIsEnchanted,
    /// Active while this Aura or Equipment is attached to a permanent matching `selector`.
    AttachedHostMatches(Box<PermanentSelector>),
    /// Active while the source controller controls exactly `count` permanents matching `selector`.
    ControlsExactCount {
        selector: Box<PermanentSelector>,
        count: u16,
    },
    SourceIsTapped,
    SourceIsUntapped,
    /// Active while this Class permanent's current level is at least the given level (CR 716).
    SourceClassLevelAtLeast(u8),
    /// Active while this Case permanent is solved (CR 719).
    SourceIsSolved,
    /// Active while this permanent entered the battlefield during the current turn.
    SourceEnteredThisTurn,
    /// Active while this permanent is currently paired with another creature via soulbond.
    SourcePaired,
    /// Active while this permanent is currently in the prepared state.
    SourceIsPrepared,
    /// Active while this permanent's entry option index equals the given value (e.g. Siege Khans/Dragons).
    EntryOptionChosen(u8),
    /// Active while at least `minimum` nonland permanents entered under controller's control this turn (Celebration).
    NonlandPermanentsEnteredThisTurnAtLeast(u16),
    /// Active while any participant's library has at most `maximum` cards in it.
    AnyLibraryAtMost(u16),
    /// Active while any participant has at most `maximum` cards in hand.
    AnyPlayerHandCountAtMost(u16),
    /// Active while `color` is the most common color (or tied for most common when `or_tied`) among all permanents.
    MostCommonPermanentColor {
        color: Color,
        or_tied: bool,
    },
    ControllerHasCityBlessing,
    ControllerGainedLifeThisTurn,
    ControllerLostLifeThisTurn,
    OpponentLostLifeThisTurn,
    ControllerAttackedThisTurn,
    ControllerCastSpellsThisTurn(u16),
    ControllerDiscardedThisTurn,
    ControllerSacrificedThisTurn,
    PermanentLeftBattlefieldThisTurn,
    CreatureEnteredUnderControlThisTurn,
    DescendedThisTurn,
    CommittedCrimeThisTurn,
    ControllerDrewCardsThisTurn(u16),
    ControlsMatching {
        selector: Box<PermanentSelector>,
        minimum: u16,
    },
    OpponentControlsMatching {
        selector: Box<PermanentSelector>,
        minimum: u16,
    },
    /// General world-evaluable condition expression evaluated against the current scratch world.
    Eval(Box<crate::program::ResolutionCondition>),
    /// Active while the given Room door half is unlocked on the source permanent (CR 709.5).
    RoomDoorUnlocked(crate::program::SplitHalf),
    /// Active while both Room doors are unlocked on the source permanent (CR 709.5).
    RoomFullyUnlocked,
    Not(Box<StaticSourceCondition>),
    And(Box<StaticSourceCondition>, Box<StaticSourceCondition>),
    Or(Box<StaticSourceCondition>, Box<StaticSourceCondition>),
}
/// Zone scanned by [`NumericExpression::ZoneCount`] during static characteristic evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticZone {
    /// Battlefield permanents currently controlled by the source controller.
    ControlledBattlefield,
    /// Cards currently in the source controller's graveyard.
    OwnGraveyard,
    /// Cards currently in the source controller's hand.
    OwnHand,
}

/// Player-level scalar statistic evaluated by [`NumericExpression::PlayerStat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticPlayerStat {
    /// Colored mana symbols in the mana costs of permanents controlled by the source controller.
    Devotion(Color),
    /// Mana of the given type currently in the source controller's mana pool.
    ManaInPool(crate::mana::ManaType),
    /// Half the highest life total among opponents, rounded up.
    HalfHighestOpponentLife,
    /// Highest current power among creature cards in the source controller's graveyard.
    OwnGraveyardGreatestCreaturePower,
}

/// Condition gating a [`NumericExpression::ConditionalBonus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticBonusCondition {
    /// Active while the source's current controller is the active player.
    DuringControllersTurn,
    /// Active while the source's controller controls at least `minimum` lands.
    ControlledLandsAtLeast(u16),
}

// Keep PermanentSelector inline in NumericExpression::Count to match existing pattern-matching ergonomics.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumericExpression {
    /// Fixed integer constant.
    Literal(RulesInteger),
    /// Linear combination over battlefield permanents matching `selector`.
    Count {
        selector: PermanentSelector,
        multiplier: RulesInteger,
        offset: RulesInteger,
    },
    /// Count objects in `zone` matching the required card-type mask (empty mask matches any card).
    ZoneCount {
        zone: StaticZone,
        types: crate::program::CardTypeMask,
    },
    /// Counter count on the ability source (`source: None`) or a linked physical permanent (`source: Some(id)`).
    Counters {
        source: Option<ObjectId>,
        kind: crate::effects::CounterKind,
    },
    /// Power or toughness of the last imprinted creature (`total: false`) or sum of all imprinted
    /// creatures (`total: true`) exiled by the linked ability (CR 607.3).
    ImprintedStat {
        stat: crate::program::ObjectStatKind,
        total: bool,
    },
    /// Player-level scalar statistic (devotion, mana in pool, opponent life, graveyard max power).
    PlayerStat(StaticPlayerStat),
    /// Count actual Auras and Equipment currently attached to the source, scaled by `multiplier`.
    AttachedAurasAndEquipment { multiplier: RulesInteger },
    /// Bonus earned while `condition` holds, otherwise zero.
    ConditionalBonus {
        condition: StaticBonusCondition,
        bonus: RulesInteger,
    },
    /// General world-evaluable numeric expression evaluated against the current scratch world.
    Eval(Box<crate::program::ValueExpr>),
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
            Self::Literal(_)
            | Self::ZoneCount { .. }
            | Self::Counters { .. }
            | Self::ImprintedStat { .. }
            | Self::PlayerStat(_)
            | Self::AttachedAurasAndEquipment { .. }
            | Self::ConditionalBonus { .. } => Ok(()),
            Self::Count { selector, .. } => selector.validate(vocabulary),
            Self::Eval(expr) => {
                if !expr.is_world_evaluable() {
                    return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
                }
                expr.validate_world_expr(vocabulary)
                    .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)
            }
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
    TypeWithSubtypes {
        base: StaticTypeChange,
        artifact_subtypes: SubtypeOperation<
            BTreeSet<crate::program::ArtifactSubtype>,
            crate::program::ArtifactSubtype,
        >,
        enchantment_subtypes: SubtypeOperation<
            BTreeSet<crate::program::EnchantmentSubtype>,
            crate::program::EnchantmentSubtype,
        >,
    },
    Color(ColorOperation<StaticColorOperand>),
    /// A layer-six effect granting one independently captured triggered ability.
    GrantTrigger(Box<TriggeredAbility>),
    GrantActivated(Box<crate::activated::ActivatedAbility>),
    GrantActivatedRestricted {
        ability: Box<crate::activated::ActivatedAbility>,
        restriction: Box<crate::activated::ActivationRestriction>,
    },
    InheritCounteredCreatureAbilities,
    InheritExiledCreatureAbilities,
    /// Grant an independent tap ability that adds one mana of any color.
    GrantAnyColorMana,
    GrantFixedColorMana(crate::mana::ManaType),
    /// Grant an independent tap mana ability producing any orthogonal `ManaOutput`.
    GrantMana(Box<crate::mana::ManaOutput>),
    /// Grant layer-six protection against each subtype selected by this operand.
    GrantSubtypeProtection {
        types: StaticSubtypeOperand,
        creatures_only: bool,
    },
    /// Grant layer-six protection against the color chosen as the source entered the battlefield.
    GrantProtectionChosenColor,
    /// Grant layer-six protection against the player chosen as the source entered the battlefield (e.g. True-Name Nemesis).
    GrantProtectionChosenPlayer,
    Abilities(AbilityChange),
    BlockingRestriction(BlockingRestriction),
    /// Set each recipient’s base power and toughness to its own mana value.
    SetPTToOwnManaValue,
    SetPT(NumericPair),
    ModifyPT(NumericPair),
    SwitchPT,
}
impl StaticPart {
    pub fn bucket(&self) -> CharacteristicBucket {
        match self {
            Self::ControlSourceController => CharacteristicBucket::Control,
            Self::Text(_) | Self::Names(_) => CharacteristicBucket::Text,
            Self::Type(_) | Self::TypeWithSubtypes { .. } => CharacteristicBucket::Type,
            Self::Color(_) => CharacteristicBucket::Color,
            Self::InheritCounteredCreatureAbilities
            | Self::InheritExiledCreatureAbilities
            | Self::GrantActivated(_)
            | Self::GrantActivatedRestricted { .. }
            | Self::GrantTrigger(_)
            | Self::GrantAnyColorMana
            | Self::GrantFixedColorMana(_)
            | Self::GrantMana(_)
            | Self::GrantSubtypeProtection { .. }
            | Self::GrantProtectionChosenColor
            | Self::GrantProtectionChosenPlayer
            | Self::Abilities(_)
            | Self::BlockingRestriction(_) => CharacteristicBucket::Abilities,
            Self::SetPT(_) | Self::SetPTToOwnManaValue => CharacteristicBucket::SetPT,
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
    /// Deck admission supplies the physical commander's pregame choice. Game setup moves
    /// the value out of the printed program so copied abilities never copy the choice.
    CommanderChosenColor(Option<Color>),
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
            Self::CommanderChosenColor(_) | Self::Colors(_) => CharacteristicBucket::Color,
            Self::PowerToughness { .. } => CharacteristicBucket::DefinePT,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Authored defining parts; admission requires nonempty, unique buckets.
pub struct DefiningParts(pub Vec<DefiningPart>);
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
    InvalidKeywordGrant,
}
pub(super) fn validate_buckets(
    buckets: impl Iterator<Item = CharacteristicBucket>,
) -> Result<(), CharacteristicDefinitionError> {
    let mut found = BTreeSet::new();
    for bucket in buckets {
        if !found.insert(bucket) && bucket != CharacteristicBucket::Abilities {
            return Err(CharacteristicDefinitionError::DuplicateBucket);
        }
    }
    if found.is_empty() {
        return Err(CharacteristicDefinitionError::EmptyParts);
    }
    Ok(())
}
pub(super) fn validate_subtypes(
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
        if self.creature_subtype.is_some()
            && self.creature_subtype == self.forbidden_creature_subtype
        {
            return Err(CharacteristicDefinitionError::ContradictorySelector);
        }
        if [
            self.creature_subtype,
            self.forbidden_creature_subtype,
            self.controlled_creatures_and_typed_tokens,
        ]
        .into_iter()
        .flatten()
        .chain(self.any_creature_subtypes.iter().copied())
        .any(|kind| vocabulary.label(kind).is_none())
        {
            return Err(CharacteristicDefinitionError::ForeignCreatureType);
        }
        if let Some(cond) = &self.source_condition {
            validate_static_source_condition(cond, vocabulary)?;
        }
        if self.controlled_creatures_and_typed_tokens.is_some()
            && *self
                != (PermanentSelector {
                    controlled_creatures_and_typed_tokens: self
                        .controlled_creatures_and_typed_tokens,
                    ..Self::default()
                })
        {
            return Err(CharacteristicDefinitionError::ContradictorySelector);
        }
        Ok(())
    }
}

pub(crate) fn validate_static_source_condition(
    cond: &StaticSourceCondition,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), CharacteristicDefinitionError> {
    match cond {
        StaticSourceCondition::ControllerControlsSubtype(kind)
        | StaticSourceCondition::OpponentControlsSubtype(kind) => {
            if vocabulary.label(*kind).is_none() {
                return Err(CharacteristicDefinitionError::ForeignCreatureType);
            }
            Ok(())
        }
        StaticSourceCondition::ControlsMatching { selector, minimum }
        | StaticSourceCondition::OpponentControlsMatching { selector, minimum } => {
            if *minimum == 0 {
                return Err(CharacteristicDefinitionError::ContradictorySelector);
            }
            selector.validate(vocabulary)
        }
        StaticSourceCondition::Eval(cond) => {
            cond.validate_world(vocabulary).map_err(|err| match err {
                crate::program::DefinitionError::ForeignCreatureType => {
                    CharacteristicDefinitionError::ForeignCreatureType
                }
                _ => CharacteristicDefinitionError::ContradictorySelector,
            })
        }
        StaticSourceCondition::Not(inner) => validate_static_source_condition(inner, vocabulary),
        StaticSourceCondition::And(a, b) | StaticSourceCondition::Or(a, b) => {
            validate_static_source_condition(a, vocabulary)?;
            validate_static_source_condition(b, vocabulary)
        }
        _ => Ok(()),
    }
}
/// Finite type-changing effects operate on battlefield objects only.
pub(super) fn validate_battlefield_types(
    operation: &TypeOperation,
) -> Result<(), CharacteristicDefinitionError> {
    let values = match operation {
        TypeOperation::Keep => return Ok(()),
        TypeOperation::Add(values) | TypeOperation::Remove(values) | TypeOperation::Set(values) => {
            values
        }
    };
    if values.iter().any(|value| {
        matches!(
            value,
            CardType::Instant | CardType::Sorcery | CardType::Planeswalker
        )
    }) {
        return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
    }
    Ok(())
}
impl ResolvedParts {
    pub fn validate(&self) -> Result<(), CharacteristicDefinitionError> {
        for part in &self.0 {
            if let ResolvedPart::Abilities(change) = part {
                change.validate_grants()?;
            }
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
        if self
            .0
            .iter()
            .filter(|part| matches!(part, ResolvedPart::Abilities(_)))
            .count()
            > 1
        {
            return Err(CharacteristicDefinitionError::DuplicateBucket);
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
        if self
            .0
            .iter()
            .filter(|part| matches!(part, ResolvedPartSpec::Abilities(_)))
            .count()
            > 1
        {
            return Err(CharacteristicDefinitionError::DuplicateBucket);
        }
        validate_buckets(self.0.iter().map(ResolvedPartSpec::bucket))?;
        for part in &self.0 {
            if matches!(
                part,
                ResolvedPartSpec::BlockingRestriction(
                    BlockingRestriction::BlockerPowerAtLeastSource
                        | BlockingRestriction::BlockerPowerAtLeastPowerOf(_)
                        | BlockingRestriction::CannotBlockObject(_)
                )
            ) {
                return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
            }
            if let ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::OnlyBlockedBySubtype(kind)
                | BlockingRestriction::CannotBlockSubtype(kind)
                | BlockingRestriction::CannotBeBlockedBySubtype(kind),
            ) = part
            {
                validate_subtypes(&BTreeSet::from([*kind]), vocabulary)?;
            }
            if let ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBlockMatching(filter)
                | BlockingRestriction::CannotBeBlockedByMatching(filter)
                | BlockingRestriction::OnlyBlockedByMatching(filter)
                | BlockingRestriction::CanBlockOnlyMatching(filter)
                | BlockingRestriction::CannotAttackUnlessDefenderControls(filter)
                | BlockingRestriction::MustBeBlockedByAllMatching(filter),
            ) = part
            {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
            }
            if let ResolvedPartSpec::Abilities(change) = part {
                if let Some(trigger) = &change.granted_trigger {
                    trigger
                        .validate_body(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                change.validate_grants()?;
            }
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
            if let ResolvedPartSpec::GrantTrigger(trigger) = part {
                trigger
                    .validate_body(vocabulary)
                    .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
            }
            if let ResolvedPartSpec::GrantActivated(ability)
            | ResolvedPartSpec::GrantActivatedRestricted { ability, .. } = part
            {
                if !ability.mana_cost.valid() {
                    return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                }
                let body_context = match ability.source_cost {
                    crate::activated::SourceCost::SacrificeSelected {
                        filter:
                            crate::activated::SelectedSacrificeFilter::Creature
                            | crate::activated::SelectedSacrificeFilter::OtherCreature
                            | crate::activated::SelectedSacrificeFilter::Subtype(_),
                        count: 1,
                        from_graveyard: false,
                        ..
                    }
                    | crate::activated::SourceCost::PayLife {
                        sacrifice:
                            Some(
                                crate::activated::SelectedSacrificeFilter::Creature
                                | crate::activated::SelectedSacrificeFilter::OtherCreature,
                            ),
                        ..
                    } => crate::program::BodyContext::SacrificedCreature,
                    crate::activated::SourceCost::SacrificeSelected {
                        filter: crate::activated::SelectedSacrificeFilter::Matching(filter),
                        count: 1,
                        from_graveyard: false,
                        ..
                    } if filter.guarantees_creature() => {
                        crate::program::BodyContext::SacrificedCreature
                    }
                    crate::activated::SourceCost::SacrificeSelected {
                        count: 1,
                        from_graveyard: false,
                        ..
                    } => crate::program::BodyContext::SacrificedPermanent,
                    _ => crate::program::BodyContext::ACTIVATED,
                };
                ability
                    .body
                    .validate_in(vocabulary, body_context)
                    .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
            }
            if let ResolvedPartSpec::Type(change)
            | ResolvedPartSpec::TypeWithSubtypes { base: change, .. } = part
            {
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
        self.materialize_with_eval(values, resolving_controller, target_owner, |_| {
            Err(CharacteristicInvariantError::NoEligibleComponent)
        })
    }
    /// Materialize cursor locals and dynamic value expressions at resolution time (CR 611.2c).
    ///
    /// # Errors
    /// Returns an invariant error when a required local or dynamic expression cannot be resolved.
    pub fn materialize_with_eval<F>(
        &self,
        values: &BTreeMap<CreatureTypeSlot, CreatureTypeId>,
        resolving_controller: SeatId,
        target_owner: SeatId,
        eval: F,
    ) -> Result<ResolvedParts, CharacteristicInvariantError>
    where
        F: FnMut(&crate::program::ValueExpr) -> Result<RulesInteger, CharacteristicInvariantError>,
    {
        self.materialize_with_source_and_eval(
            values,
            resolving_controller,
            target_owner,
            None,
            eval,
        )
    }
    /// Materialize cursor locals, source-relative restrictions, and dynamic value expressions at resolution time (CR 611.2c).
    ///
    /// # Errors
    /// Returns an invariant error when a required local, source object, or dynamic expression cannot be resolved.
    pub fn materialize_with_source_and_eval<F>(
        &self,
        values: &BTreeMap<CreatureTypeSlot, CreatureTypeId>,
        resolving_controller: SeatId,
        target_owner: SeatId,
        source_object: Option<ObjectId>,
        mut eval: F,
    ) -> Result<ResolvedParts, CharacteristicInvariantError>
    where
        F: FnMut(&crate::program::ValueExpr) -> Result<RulesInteger, CharacteristicInvariantError>,
    {
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
                    ResolvedPartSpec::TypeWithSubtypes {
                        base: change,
                        artifact_subtypes,
                        enchantment_subtypes,
                    } => ResolvedPart::TypeWithSubtypes {
                        base: TypeChange {
                            land_subtypes: change.land_subtypes.clone(),
                            supertypes: change.supertypes.clone(),
                            types: change.types.clone(),
                            subtypes: match &change.subtypes {
                                SubtypeOperation::Keep => SubtypeOperation::Keep,
                                SubtypeOperation::Remove(set) => {
                                    SubtypeOperation::Remove(set.clone())
                                }
                                SubtypeOperation::Add(operand) => {
                                    SubtypeOperation::Add(materialize_subtype(operand, values)?)
                                }
                                SubtypeOperation::Set(operand) => {
                                    SubtypeOperation::Set(materialize_subtype(operand, values)?)
                                }
                            },
                        },
                        artifact_subtypes: artifact_subtypes.clone(),
                        enchantment_subtypes: enchantment_subtypes.clone(),
                    },
                    ResolvedPartSpec::GrantTrigger(trigger) => {
                        ResolvedPart::GrantTrigger(trigger.clone())
                    }
                    ResolvedPartSpec::GrantActivated(ability) => {
                        ResolvedPart::GrantActivated(ability.clone())
                    }
                    ResolvedPartSpec::GrantActivatedRestricted {
                        ability,
                        restriction,
                    } => ResolvedPart::GrantActivatedRestricted {
                        ability: ability.clone(),
                        restriction: restriction.clone(),
                    },
                    ResolvedPartSpec::GrantAnyColorMana => ResolvedPart::GrantAnyColorMana,
                    ResolvedPartSpec::GrantFixedColorMana(color) => {
                        ResolvedPart::GrantFixedColorMana(*color)
                    }
                    ResolvedPartSpec::GrantMana(output) => ResolvedPart::GrantMana(output.clone()),
                    ResolvedPartSpec::SetPT(pair) => ResolvedPart::SetPT(pair.clone()),
                    ResolvedPartSpec::Abilities(change) => ResolvedPart::Abilities(change.clone()),
                    ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotBlockSource,
                    ) => {
                        let source_id = source_object
                            .ok_or(CharacteristicInvariantError::NoEligibleComponent)?;
                        ResolvedPart::BlockingRestriction(BlockingRestriction::CannotBlockObject(
                            source_id,
                        ))
                    }
                    ResolvedPartSpec::BlockingRestriction(BlockingRestriction::MustBlockSource) => {
                        let source_id = source_object
                            .ok_or(CharacteristicInvariantError::NoEligibleComponent)?;
                        ResolvedPart::BlockingRestriction(BlockingRestriction::MustBlockObject(
                            source_id,
                        ))
                    }
                    ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotBeBlockedBySource,
                    ) => {
                        let source_id = source_object
                            .ok_or(CharacteristicInvariantError::NoEligibleComponent)?;
                        ResolvedPart::BlockingRestriction(
                            BlockingRestriction::CannotBeBlockedByObject(source_id),
                        )
                    }
                    ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotAttackSourceController,
                    ) => ResolvedPart::BlockingRestriction(
                        BlockingRestriction::CannotAttackPlayer(resolving_controller),
                    ),
                    ResolvedPartSpec::BlockingRestriction(value) => {
                        ResolvedPart::BlockingRestriction(value.clone())
                    }
                    ResolvedPartSpec::ModifyPT(pair) => ResolvedPart::ModifyPT(pair.clone()),
                    ResolvedPartSpec::ModifyPTExpr { power, toughness } => {
                        ResolvedPart::ModifyPT(PowerToughness {
                            power: eval(power)?,
                            toughness: eval(toughness)?,
                        })
                    }
                    ResolvedPartSpec::SetPTExpr { power, toughness } => {
                        ResolvedPart::SetPT(PowerToughness {
                            power: eval(power)?,
                            toughness: eval(toughness)?,
                        })
                    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContinuousEffect {
    pub id: ContinuousEffectId,
    pub timestamp: CharacteristicTimestamp,
    pub creator: StackEntryId,
    pub affected: BTreeSet<ObjectId>,
    pub parts: ResolvedParts,
    pub duration: ResolvedDuration,
}
/// Duration-scoped battlefield static ability installed by a resolving spell or ability (CR 611.2a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryBattlefieldStatic {
    pub id: u64,
    pub controller: SeatId,
    pub source_object: Option<ObjectId>,
    pub timestamp: CharacteristicTimestamp,
    pub duration: ResolvedDuration,
    pub ability: StaticContinuousAbility,
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
    /// Mana value of the current copiable definition, with X equal to zero.
    pub mana_value: RulesInteger,
    /// Colored symbols in the copiable mana cost, independent of color indicators and color effects.
    pub colored_mana_symbols: BTreeMap<Color, RulesInteger>,
    /// Copiable printed loyalty used by the battlefield entry replacement.
    pub printed_loyalty: Option<RulesInteger>,
    pub printed_defense: Option<RulesInteger>,
    pub names: spellwake_contracts::names::ObjectNames,
    pub supertypes: BTreeSet<Supertype>,
    pub types: BTreeSet<CardType>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub land_subtypes: BTreeSet<LandType>,
    pub spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
    pub artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
    pub enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
    pub planeswalker_subtypes: BTreeSet<crate::program::PlaneswalkerSubtype>,
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
    pub attack_requirement: Option<crate::program::AttackRequirement>,
    pub additional_attack_requirements: Vec<crate::program::AttackRequirement>,
    pub combat_limits: crate::program::CombatDeclarationLimits,
    /// Surviving printed entry-status replacement.
    pub entry_tap: Option<crate::land::EntryTap>,
    /// Whether the printed counter abilities still apply as the permanent enters.
    pub entry_counter_abilities: bool,
    /// Surviving face-up special-action permissions.
    pub face_up: Vec<usize>,
    /// Currently surviving replacement definitions, never inferred from retained text.
    pub zone_replacements: Vec<crate::program::ZoneReplacementAbilityId>,
    pub source: ObjectId,
    pub card: Option<CardId>,
    pub mana: Vec<ManaAbilityId>,
    pub activated: Vec<ActivatedAbilityId>,
    pub triggered: Vec<TriggeredAbilityId>,
    pub continuous: Option<StaticAbilityId>,
    pub additional_battlefield_abilities: Vec<StaticAbilityId>,
}
impl PrintedAbilityRoster {
    pub(crate) fn empty_emblem(source: ObjectId) -> Self {
        Self {
            attack_requirement: None,
            additional_attack_requirements: Vec::new(),
            combat_limits: crate::program::CombatDeclarationLimits::default(),
            entry_tap: None,
            entry_counter_abilities: false,
            face_up: Vec::new(),
            zone_replacements: Vec::new(),
            source,
            card: None,
            mana: Vec::new(),
            activated: Vec::new(),
            triggered: Vec::new(),
            continuous: None,
            additional_battlefield_abilities: Vec::new(),
        }
    }
    pub(super) fn remove(&mut self, removal: AbilityRemoval) {
        match removal {
            AbilityRemoval::Banding | AbilityRemoval::BandsWithOther | AbilityRemoval::Flying => {
                return;
            }
            AbilityRemoval::All => self.mana.clear(),
            AbilityRemoval::NonMana => {}
        }
        self.attack_requirement = None;
        self.additional_attack_requirements.clear();
        self.combat_limits = Default::default();
        self.entry_tap = None;
        self.entry_counter_abilities = false;
        self.face_up.clear();
        self.zone_replacements.clear();
        self.activated.clear();
        self.triggered.clear();
        self.continuous = None;
        self.additional_battlefield_abilities.clear();
    }
    /// Every surviving printed attack requirement on this permanent.
    pub fn all_attack_requirements(
        &self,
    ) -> impl Iterator<Item = crate::program::AttackRequirement> + '_ {
        self.attack_requirement
            .into_iter()
            .chain(self.additional_attack_requirements.iter().copied())
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
                            .chain(&self.additional_battlefield_abilities)
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
    pub face_up_abilities: Vec<crate::face_down::FaceUpAbility>,
    pub activation_restrictions:
        std::collections::BTreeMap<ActivatedAbilityId, crate::activated::ActivationRestriction>,
    pub activation_modes:
        std::collections::BTreeMap<ActivatedAbilityId, crate::program::SpellModes>,
    pub activation_hybrid_symbols:
        std::collections::BTreeMap<ActivatedAbilityId, Vec<crate::mana::HybridSymbol>>,
    pub extra_activation_costs:
        std::collections::BTreeMap<ActivatedAbilityId, Vec<crate::activated::ExtraActivationCost>>,
    pub extra_cast_costs: Vec<crate::activated::ExtraActivationCost>,
    /// Printed loyalty costs for surviving activated abilities.
    pub loyalty_costs: std::collections::BTreeMap<ActivatedAbilityId, i32>,
    /// Activated loyalty costs whose value is announced as X.
    pub variable_loyalty_costs: BTreeSet<ActivatedAbilityId>,
    /// Copiable keyword abilities, including those suppressed in the current world.
    pub printed_keywords: BTreeSet<KeywordAbility>,
    pub printed_toxic: Option<RulesInteger>,
    /// Static discounts retained with the copiable text.
    pub cast_reductions: Vec<crate::program::CastReduction>,
    pub entry_counter_abilities: Vec<crate::program::EntryCounterAbility>,
    /// Copiable mana-only flashback cost.
    pub plot: Option<crate::mana::ManaCost>,
    pub madness: Option<crate::mana::ManaCost>,
    pub miracle: Option<crate::program::MiracleCost>,
    pub exile_action: Option<crate::program::ExileAction>,
    pub flashback: Option<crate::mana::ManaCost>,
    pub flashback_x_mana_symbols: u8,
    pub flashback_sacrifice_creatures: u8,
    /// Count of untapped white creatures required as the printed flashback price.
    pub flashback_tap_white_creatures: u8,
    /// Life paid only when casting using this flashback cost.
    pub flashback_life: u16,
    /// This spell requires discarding one other card as an additional cost.
    pub discard_cast_cost: bool,
    /// Optional filter restricting the card discarded for `discard_cast_cost`.
    pub discard_cast_cost_filter: Option<crate::program::ObjectFilter>,
    pub graveyard_creature_exile_cost: u8,
    pub graveyard_exile_x_cost: bool,
    pub graveyard_self_cast: Option<crate::program::GraveyardSelfCast>,
    /// Casting this spell requires sacrificing one permanent (creature by default) the caster controls.
    pub sacrifice_creature_cast_cost: bool,
    /// Optional filter restricting the permanent sacrificed for `sacrifice_creature_cast_cost`.
    pub sacrifice_cast_cost_filter: Option<crate::program::ObjectFilter>,
    pub tap_creatures_cast_cost: bool,
    /// Reveal a colorless creature card in hand as an additional casting cost.
    pub reveal_colorless_creature_cast_cost: bool,
    pub conditional_cost: Option<crate::program::ConditionalCastCost>,
    pub additional_cast_costs: Vec<crate::program::AdditionalCastCost>,
    /// Semantic replacement definitions; the ability roster separately determines existence.
    pub zone_replacements: Vec<crate::program::ZoneReplacementAbility>,
    pub mana: Vec<ManaAbility>,
    pub activated: Vec<ActivatedAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub continuous: Option<StaticContinuousAbility>,
    pub additional_battlefield_abilities: Vec<StaticContinuousAbility>,
    pub static_conditions: std::collections::BTreeMap<StaticAbilityId, StaticSourceCondition>,
}
impl PrintedText {
    pub(crate) fn from_program(program: &Program) -> Self {
        Self {
            activation_restrictions: program.activation_restrictions.clone(),
            activation_modes: program.activation_modes.clone(),
            activation_hybrid_symbols: program.activation_hybrid_symbols.clone(),
            extra_activation_costs: program.extra_activation_costs.clone(),
            extra_cast_costs: program.extra_cast_costs.clone(),
            loyalty_costs: program.loyalty_costs.clone(),
            variable_loyalty_costs: program.variable_loyalty_costs.clone(),
            face_up_abilities: program.face_up_abilities.clone(),
            printed_keywords: program.printed_keywords.clone(),
            printed_toxic: program.printed_toxic.clone(),
            cast_reductions: program.cast_reductions.clone(),
            entry_counter_abilities: program.entry_counter_abilities.clone(),
            plot: program.plot.clone(),
            madness: program.madness.clone(),
            miracle: program.miracle.clone(),
            exile_action: program.exile_action.clone(),
            flashback: program.flashback.clone(),
            flashback_x_mana_symbols: program.flashback_x_mana_symbols,
            flashback_sacrifice_creatures: program.flashback_sacrifice_creatures,
            flashback_tap_white_creatures: program.flashback_tap_white_creatures,
            flashback_life: program.flashback_life,
            discard_cast_cost: program.discard_cast_cost,
            discard_cast_cost_filter: program.discard_cast_cost_filter,
            graveyard_creature_exile_cost: program.graveyard_creature_exile_cost,
            graveyard_exile_x_cost: program.graveyard_exile_x_cost,
            graveyard_self_cast: program.graveyard_self_cast,
            sacrifice_creature_cast_cost: program.sacrifice_creature_cast_cost,
            sacrifice_cast_cost_filter: program.sacrifice_cast_cost_filter,
            tap_creatures_cast_cost: program.tap_creatures_cast_cost,
            reveal_colorless_creature_cast_cost: program.reveal_colorless_creature_cast_cost,
            conditional_cost: program.conditional_cost.clone(),
            additional_cast_costs: program.additional_cast_costs.clone(),
            zone_replacements: program.zone_replacements.clone(),
            mana: program.mana.clone(),
            activated: program.activated.clone(),
            triggered: program.triggered.clone(),
            continuous: program.continuous.clone(),
            additional_battlefield_abilities: program.additional_battlefield_abilities.clone(),
            static_conditions: program.static_conditions.clone(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectCharacteristics {
    pub(crate) paired_with: Option<ObjectId>,
    pub(crate) tapped: Option<bool>,
    pub(crate) imprinted_cards: Vec<ObjectId>,
    pub(crate) owner: SeatId,
    /// Owner of the physical card if it is designated as a commander, including while copied.
    pub commander_owner: Option<SeatId>,
    /// Copiable attachment subtype, subject to the current artifact or enchantment type.
    pub attachment_kind: Option<crate::attachment::AttachmentKind>,
    /// A token keeps its physical token status through copy and type changes.
    pub is_token: bool,
    /// Host of this exact permanent incarnation, if attached.
    pub attached_to: Option<crate::targets::TargetRef>,
    /// Charge counters currently on this exact incarnation.
    pub charge_counters: RulesInteger,
    /// Blessing counters currently on this exact incarnation.
    pub blessing_counters: RulesInteger,
    /// Positive +1/+1 counter count on this incarnation.
    pub has_plus_one_counter: bool,
    /// Positive -1/-1 counter count on this incarnation.
    pub has_minus_one_counter: bool,
    /// Exact +1/+1 counter count on this incarnation.
    pub plus_one_counters: RulesInteger,
    pub has_any_counter: bool,
    pub attacking: bool,
    pub blocking: bool,
    /// This exact permanent has resolved a saddle ability this turn.
    pub saddled: bool,
    pub object: ObjectId,
    /// Speed of the controller after applying control-changing effects.
    pub controller_speed: u8,
    /// Derived from this exact physical world, never an independent location owner.
    pub zone: ObjectZone,
    pub defined: DefinedCharacteristics,
    pub current_names: spellwake_contracts::names::ObjectNames,
    pub current_supertypes: BTreeSet<Supertype>,
    pub current_types: BTreeSet<CardType>,
    pub current_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub current_land_subtypes: BTreeSet<LandType>,
    /// Derived planeswalker family after type-changing effects.
    pub current_planeswalker_subtypes: BTreeSet<crate::program::PlaneswalkerSubtype>,
    /// Current instant/sorcery subtypes after type-changing effects.
    pub current_spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
    pub current_artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
    pub current_enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
    /// Derived basic-land abilities after current type and ability effects.
    pub intrinsic_mana: BTreeSet<BasicLandType>,
    pub current_colors: BTreeSet<Color>,
    pub controller: Option<SeatId>,
    pub abilities: PrintedAbilityRoster,
    /// Printed semantic definitions after layer 3; availability is in `abilities`.
    pub printed_text: PrintedText,
    /// Surviving printed and granted keyword abilities after the layers.
    pub keywords: BTreeSet<KeywordAbility>,
    pub(crate) suppressed_keywords: BTreeSet<KeywordAbility>,
    pub(crate) entry_creature_type: Option<CreatureTypeId>,
    /// Restrictions applied to this attacker, independent of ability removal.
    pub blocking_restrictions: Vec<BlockingRestriction>,
    /// Currently has the granted static ability that this creature cannot be blocked.
    pub granted_unblockable: bool,
    /// Active static restriction prevents sacrificing this permanent.
    pub cannot_be_sacrificed: bool,
    /// Active static restriction prevents untapping this permanent.
    pub cannot_be_untapped: bool,
    /// Layer-6 ability effect prevents untapping this permanent during its controller's untap step.
    pub doesnt_untap_during_untap_step: bool,
    /// Layer-6 ability effect suppresses activating abilities of this permanent.
    pub suppressed_activations: Option<ActivationSuppression>,
    /// Active static restriction lets controller activate tap/untap abilities as though this creature had haste.
    pub activate_as_though_haste: bool,
    /// Active static restriction prevents Aura spells from targeting this permanent.
    pub cannot_be_targeted_by_auras: bool,
    /// Active static restriction prevents spells or abilities of any color in this mask from targeting this permanent.
    pub cannot_be_targeted_by_colors: crate::program::ColorMask,
    /// Seats that may target this permanent as though it did not have hexproof.
    pub hexproof_ignored_by: BTreeSet<SeatId>,
    /// Sum of surviving printed and granted toxic values (CR 702.164b).
    pub toxic_value: Option<RulesInteger>,
    /// Surviving grants keyed by the exact continuous effect, independent of printed definitions.
    pub granted_exalted: BTreeMap<EffectInstanceKey, u16>,
    /// Each static effect grants a separate ordinary trigger on this permanent.
    pub granted_triggers: BTreeMap<EffectInstanceKey, TriggeredAbility>,
    pub inherited_activation_terms: BTreeMap<EffectInstanceKey, super::ActivationTerms>,
    pub inherited_mana: BTreeMap<EffectInstanceKey, crate::mana::ManaOutput>,
    pub granted_activated: BTreeMap<EffectInstanceKey, crate::activated::ActivatedAbility>,
    /// Surviving independent grants of the tap-for-any-color mana ability.
    pub granted_any_color_mana: BTreeSet<EffectInstanceKey>,
    pub granted_fixed_mana: BTreeMap<EffectInstanceKey, crate::mana::ManaType>,
    /// Exact granting effect and declaration order for every surviving trigger.
    pub granted_combat_keywords:
        BTreeMap<EffectInstanceKey, Vec<crate::program::CombatTriggerKeyword>>,
    /// Current numeric values in this exact zone. Battlefield noncreatures
    /// have none; nonbattlefield objects may retain their printed PT box.
    pub power_toughness: Option<PowerToughnessEvaluation>,
}
impl ObjectCharacteristics {
    /// Highest chapter number among this Saga's current chapter abilities.
    pub(crate) fn final_saga_chapter(&self) -> Option<u8> {
        if !self
            .current_enchantment_subtypes
            .contains(&crate::program::EnchantmentSubtype::Saga)
        {
            return None;
        }
        self.printed_text
            .triggered
            .iter()
            .filter(|ability| self.abilities.triggered.contains(&ability.id))
            .chain(self.granted_triggers.values())
            .filter_map(|ability| match ability.condition.event {
                crate::program::TriggerEvent::CountersChanged {
                    kind: crate::program::CounterTriggerKind::SagaChapter(number),
                    ..
                } => Some(number),
                _ => None,
            })
            .max()
    }

    /// Current printed combat keyword trigger instances, respecting copy effects and ability removal.
    pub fn combat_trigger_keywords(
        &self,
    ) -> impl Iterator<Item = &crate::program::CombatTriggerKeyword> {
        self.printed_text
            .triggered
            .iter()
            .filter(|ability| self.abilities.triggered.contains(&ability.id))
            .filter_map(TriggeredAbility::as_combat_keyword)
            .chain(self.granted_combat_keywords.values().flatten())
    }

    /// Count independently triggering printed and granted instances of exalted.
    pub fn exalted_instances(&self) -> usize {
        let printed = self
            .printed_text
            .triggered
            .iter()
            .filter(|ability| {
                ability.is_exalted() && self.abilities.triggered.contains(&ability.id)
            })
            .count();
        self.granted_exalted.values().fold(printed, |sum, count| {
            sum.saturating_add(usize::from(*count))
        })
    }

    /// All currently available mana references, in deterministic origin/type order.
    /// This query creates no authored definitions or persistent ability roster.
    pub fn mana_abilities(&self) -> BTreeSet<ManaAbilityRef> {
        self.abilities
            .mana
            .iter()
            .copied()
            .filter(|id| {
                self.printed_text
                    .mana
                    .iter()
                    .any(|ability| ability.id == *id && !ability.output.is_triggered())
            })
            .map(ManaAbilityRef::Printed)
            .chain(
                self.inherited_mana
                    .keys()
                    .copied()
                    .map(ManaAbilityRef::Inherited),
            )
            .chain(
                self.granted_any_color_mana
                    .iter()
                    .copied()
                    .map(ManaAbilityRef::GrantedAnyColor),
            )
            .chain(
                self.granted_fixed_mana
                    .iter()
                    .map(|(effect, color)| ManaAbilityRef::GrantedFixedColor(*effect, *color)),
            )
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
    pub(crate) fn active_battlefield_rules(
        &self,
    ) -> impl Iterator<Item = &StaticContinuousAbility> {
        let on_field = self.zone == ObjectZone::Battlefield;
        self.printed_text
            .continuous
            .iter()
            .chain(&self.printed_text.additional_battlefield_abilities)
            .filter(move |ability| {
                on_field
                    && (self.abilities.continuous == Some(ability.id())
                        || self
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id()))
            })
    }
    pub(crate) fn has_battlefield_rule(
        &self,
        matches_rule: impl Fn(&StaticContinuousAbility) -> bool,
    ) -> bool {
        self.zone == ObjectZone::Battlefield
            && self
                .printed_text
                .continuous
                .iter()
                .chain(&self.printed_text.additional_battlefield_abilities)
                .any(|ability| {
                    matches_rule(ability)
                        && (self.abilities.continuous == Some(ability.id())
                            || self
                                .abilities
                                .additional_battlefield_abilities
                                .contains(&ability.id()))
                })
    }

    pub fn has_keyword(&self, keyword: KeywordAbility) -> bool {
        self.keywords.contains(&keyword)
    }
    /// Whether any of the source's colors matches this permanent's current protection abilities.
    pub fn protected_from_colors(&self, colors: &BTreeSet<Color>) -> bool {
        (colors.len() >= 2 && self.has_keyword(KeywordAbility::ProtectionFromMulticolored))
            || colors.iter().any(|color| {
                self.has_keyword(match color {
                    Color::White => KeywordAbility::ProtectionFromWhite,
                    Color::Blue => KeywordAbility::ProtectionFromBlue,
                    Color::Black => KeywordAbility::ProtectionFromBlack,
                    Color::Red => KeywordAbility::ProtectionFromRed,
                    Color::Green => KeywordAbility::ProtectionFromGreen,
                })
            })
    }
    /// Protection from subtype qualities that apply to this actual source.
    pub(crate) fn protected_from_subtypes(
        &self,
        subtypes: &BTreeSet<CreatureTypeId>,
        is_creature: bool,
    ) -> bool {
        self.keywords.iter().any(|keyword| matches!(keyword,
            KeywordAbility::ProtectionFromSubtype(quality) if quality.matches(subtypes, is_creature)))
    }
    /// Unified CR 702.16 protection check covering colors, multicolored, subtypes, card types,
    /// colorless, monocolored, everything, and player qualities.
    pub(crate) fn protected_from_source(
        &self,
        colors: &BTreeSet<Color>,
        card_types: &BTreeSet<CardType>,
        subtypes: &BTreeSet<CreatureTypeId>,
        is_creature: bool,
        controller: Option<SeatId>,
    ) -> bool {
        if self.protected_from_colors(colors) || self.protected_from_subtypes(subtypes, is_creature)
        {
            return true;
        }
        self.keywords.iter().any(|keyword| match keyword {
            KeywordAbility::ProtectionFrom(ProtectionFromQuality::Colorless) => colors.is_empty(),
            KeywordAbility::ProtectionFrom(ProtectionFromQuality::Monocolored) => colors.len() == 1,
            KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(ct)) => {
                card_types.contains(ct)
            }
            KeywordAbility::ProtectionFrom(ProtectionFromQuality::Everything) => true,
            KeywordAbility::ProtectionFrom(ProtectionFromQuality::Player(seat)) => {
                controller == Some(*seat)
            }
            _ => false,
        })
    }
    /// CR 702.11e: whether any `HexproofFrom` ability on this permanent blocks a source controlled by an opponent.
    pub(crate) fn hexproof_from_source(
        &self,
        colors: &BTreeSet<Color>,
        card_types: &BTreeSet<CardType>,
        subtypes: &BTreeSet<CreatureTypeId>,
        is_ability: bool,
    ) -> bool {
        self.keywords.iter().any(|keyword| match keyword {
            KeywordAbility::HexproofFrom(HexproofQuality::Color(c)) => colors.contains(c),
            KeywordAbility::HexproofFrom(HexproofQuality::Monocolored) => colors.len() == 1,
            KeywordAbility::HexproofFrom(HexproofQuality::Multicolored) => colors.len() >= 2,
            KeywordAbility::HexproofFrom(HexproofQuality::CardType(ct)) => card_types.contains(ct),
            KeywordAbility::HexproofFrom(HexproofQuality::Subtype(st)) => subtypes.contains(st),
            KeywordAbility::HexproofFrom(HexproofQuality::ActivatedOrTriggeredAbility) => {
                is_ability
            }
            _ => false,
        })
    }

    /// Whether the current type line can retain this shared subtype family.
    pub fn carries_creature_subtypes(&self) -> bool {
        self.current_types
            .iter()
            .copied()
            .any(CardType::carries_creature_subtypes)
    }
    /// The single recipient identity for damage dealt to this permanent (CR 120.1).
    /// Damage consequences apply independently for every applicable permanent type.
    pub fn damage_recipient(&self) -> Option<crate::damage::DamageRecipient> {
        use crate::damage::DamageRecipient;
        if self.zone != crate::object_identity::ObjectZone::Battlefield {
            return None;
        }
        if self.is_creature() {
            Some(DamageRecipient::Creature(self.object))
        } else if self.has_type(CardType::Planeswalker) {
            Some(DamageRecipient::Planeswalker(self.object))
        } else if self.has_type(CardType::Battle) {
            Some(DamageRecipient::Battle(self.object))
        } else {
            None
        }
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
    pub active_player: SeatId,
    pub participants: &'a [SeatId],
    pub life: Option<&'a crate::life::LifeState>,
    pub entry_creature_types: &'a BTreeMap<ObjectId, CreatureTypeId>,
    pub mana: &'a BTreeMap<(SeatId, crate::mana::ManaType), RulesInteger>,
    pub combat: Option<&'a crate::combat::CombatState>,
    pub cards: &'a GameObjects,
    pub programs: &'a BTreeMap<CardId, Program>,
    pub state: &'a CharacteristicState,
    pub vocabulary: &'a CreatureTypeVocabulary,
    pub name_rules: &'a spellwake_contracts::names::NameRules,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectInstanceKey {
    /// A donor ability at a deterministic position in one exact evaluated action cut.
    Inherited {
        source: ObjectId,
        ability: StaticAbilityId,
        donor: ObjectId,
        ordinal: usize,
    },
    /// Persistent static effect supplied by a command-zone emblem.
    Emblem(crate::emblem::EmblemId),
    /// One authored battlefield static ability supplied by a command-zone emblem.
    EmblemStatic {
        emblem: crate::emblem::EmblemId,
        ability: StaticAbilityId,
    },
    /// Characteristic-defining effect of a printed or copied keyword.
    PrintedKeyword {
        source: ObjectId,
        keyword: KeywordAbility,
    },
    Resolved(ContinuousEffectId),
    Static {
        source: ObjectId,
        ability: StaticAbilityId,
    },
    /// Duration-scoped battlefield static ability installed by a resolving spell or ability.
    TemporaryStatic(u64),
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
    pub(crate) player_color_protection: BTreeMap<SeatId, BTreeSet<Color>>,
    pub(crate) player_granted_keywords: BTreeMap<SeatId, BTreeSet<KeywordAbility>>,
    pub(crate) emblem_rules: Vec<(SeatId, StaticContinuousAbility)>,
}
impl EvaluatedCharacteristics {
    /// Active protection colors of this player at the evaluated cut.
    pub fn player_protection_colors(&self, player: SeatId) -> impl Iterator<Item = Color> + '_ {
        self.player_color_protection
            .get(&player)
            .into_iter()
            .flatten()
            .copied()
    }

    /// Whether a battlefield ability forbids life and creature costs.
    pub fn life_and_creature_costs_prohibited(&self) -> bool {
        self.objects.values().any(|permanent| {
            permanent.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    StaticContinuousAbility::Restriction {
                        rule: StaticRestriction::ProhibitLifeAndCreatureCosts,
                        ..
                    }
                )
            })
        })
    }

    /// Whether a battlefield ability protects a player from opposing sacrifice effects.
    pub(crate) fn ignores_opposing_sacrifices(&self, player: SeatId) -> bool {
        self.objects.values().any(|permanent| {
            permanent.controller == Some(player)
                && permanent.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        StaticContinuousAbility::Restriction {
                            rule: StaticRestriction::ProhibitOpponentsCausingSacrifice,
                            ..
                        }
                    )
                })
        })
    }

    /// Whether an active static restriction or Equipment prohibits sacrificing this permanent.
    pub fn sacrifice_prohibited(&self, object: ObjectId) -> bool {
        if self.get(object).is_some_and(|p| p.cannot_be_sacrificed) {
            return true;
        }
        self.get(object).is_some_and(ObjectCharacteristics::is_creature)
            && self.objects.values().any(|equipment| {
                equipment.zone == ObjectZone::Battlefield
                    && equipment.abilities.all_attack_requirements().any(|req| {
                        req == crate::program::AttackRequirement::EquippedCannotAttackControllerOrBeSacrificed
                    })
                    && equipment.attached_to == Some(crate::targets::TargetRef::Object(object))
            })
    }

    /// Distinct sources of a replicate grant in stable source/ability order.
    pub(crate) fn replicate_grants(
        &self,
        actor: SeatId,
        spell: &ObjectCharacteristics,
    ) -> Vec<EffectInstanceKey> {
        self.objects
            .values()
            .filter(|o| o.zone == ObjectZone::Battlefield && o.controller == Some(actor))
            .flat_map(|source| {
                source
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&source.printed_text.additional_battlefield_abilities)
                    .filter_map(move |rule| {
                        let StaticContinuousAbility::CastPermission {
                            id,
                            permission: CastPermissionRule::SubtypeSpellsHaveReplicate(subtype),
                        } = rule
                        else {
                            return None;
                        };
                        (spell.current_creature_subtypes.contains(subtype)
                            && (source.abilities.continuous == Some(*id)
                                || source
                                    .abilities
                                    .additional_battlefield_abilities
                                    .contains(id)))
                        .then_some(EffectInstanceKey::Static {
                            source: source.object,
                            ability: *id,
                        })
                    })
            })
            .collect()
    }
    pub(crate) fn grants_improvise(&self, actor: SeatId) -> bool {
        self.controlled_rules(actor, |a| {
            matches!(
                a,
                StaticContinuousAbility::CostModifier {
                    modifier: SpellCostModifier::NonartifactSpellsHaveImprovise,
                    ..
                }
            )
        }) > 0
    }

    pub(crate) fn controlled_rule_sources<'a>(
        &'a self,
        controller: SeatId,
        predicate: impl Fn(&StaticContinuousAbility) -> bool + Copy + 'a,
    ) -> impl Iterator<Item = ObjectId> + 'a {
        self.objects
            .values()
            .filter(move |permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && permanent.controller == Some(controller)
            })
            .flat_map(move |permanent| {
                permanent
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&permanent.printed_text.additional_battlefield_abilities)
                    .filter(move |ability| {
                        predicate(ability)
                            && (permanent.abilities.continuous == Some(ability.id())
                                || permanent
                                    .abilities
                                    .additional_battlefield_abilities
                                    .contains(&ability.id()))
                    })
                    .map(|_| permanent.object)
            })
    }

    pub(crate) fn controlled_rules(
        &self,
        controller: SeatId,
        predicate: impl Fn(&StaticContinuousAbility) -> bool + Copy,
    ) -> usize {
        self.controlled_rule_sources(controller, predicate).count()
            + self
                .emblem_rules
                .iter()
                .filter(|(seat, ability)| *seat == controller && predicate(ability))
                .count()
    }

    /// Threshold for lethal marked damage, including trample assignment, at this exact cut.
    pub(crate) fn lethal_damage_threshold(
        &self,
        creature: &ObjectCharacteristics,
    ) -> Option<RulesInteger> {
        let pt = &creature.power_toughness.as_ref()?.derived;
        let power = creature.controller.is_some_and(|ctrl| {
            self.controlled_rules(ctrl, |ability| {
                matches!(
                    ability,
                    StaticContinuousAbility::DamageRule {
                        rule: StaticDamageRule::LethalUsesPower,
                        ..
                    }
                )
            }) > 0
        });
        Some(if power {
            pt.power.clone().max(1.into())
        } else {
            pt.toughness.clone()
        })
    }

    pub(crate) fn has_other_controlled_creatures(
        &self,
        controller: SeatId,
        source: ObjectId,
    ) -> bool {
        self.objects.values().any(|permanent| {
            permanent.object != source
                && permanent.zone == ObjectZone::Battlefield
                && permanent.controller == Some(controller)
                && permanent.is_creature()
        })
    }

    pub(crate) fn controlled_creature_count(&self, controller: SeatId) -> usize {
        self.objects
            .values()
            .filter(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && permanent.controller == Some(controller)
                    && permanent.is_creature()
            })
            .count()
    }

    pub(crate) fn total_creature_power(&self, controller: SeatId) -> RulesInteger {
        self.objects
            .values()
            .filter(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && permanent.controller == Some(controller)
                    && permanent.is_creature()
            })
            .filter_map(|permanent| permanent.power_toughness.as_ref())
            .fold(0.into(), |sum, stats| sum.plus(&stats.derived.power))
    }

    pub(crate) fn auras_and_equipment_attached_to(&self, host: ObjectId) -> RulesInteger {
        RulesInteger::from_usize(
            self.objects
                .values()
                .filter(|p| {
                    p.zone == ObjectZone::Battlefield
                        && p.attached_to == Some(crate::targets::TargetRef::Object(host))
                        && match p.attachment_kind {
                            Some(crate::attachment::AttachmentKind::Equipment) => {
                                p.has_type(CardType::Artifact)
                            }
                            Some(crate::attachment::AttachmentKind::Aura(_)) => {
                                p.has_type(CardType::Enchantment)
                            }
                            _ => false,
                        }
                })
                .count(),
        )
    }
    pub(crate) fn equipment_attached_to(&self, host: ObjectId) -> RulesInteger {
        RulesInteger::from_usize(
            self.objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.attached_to == Some(crate::targets::TargetRef::Object(host))
                        && object.attachment_kind
                            == Some(crate::attachment::AttachmentKind::Equipment)
                        && object.has_type(CardType::Artifact)
                })
                .count(),
        )
    }

    pub fn get(&self, object: ObjectId) -> Option<&ObjectCharacteristics> {
        self.objects.get(&object)
    }
}
