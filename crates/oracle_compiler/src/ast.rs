//! Strongly-typed consolidated AST for Spellwake's Phase 4 Oracle Clause Compiler.
//!
//! Designed with zero dependency on `spellwake-engine` so this compiler can be developed,
//! benchmarked, and verified across all 33,138 real-paper Magic cards independently,
//! then lowered into `spellwake_engine::program::*` via a thin adapter.

use serde::{Deserialize, Serialize};

/// Bitmask over Magic card types (mirrors `spellwake_engine::program::CardTypeMask`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardTypeMask(pub u16);

impl CardTypeMask {
    pub const EMPTY: Self = Self(0);
    pub const BATTLE: Self = Self(1 << 0);
    pub const PLANESWALKER: Self = Self(1 << 1);
    pub const LAND: Self = Self(1 << 2);
    pub const ARTIFACT: Self = Self(1 << 3);
    pub const CREATURE: Self = Self(1 << 4);
    pub const ENCHANTMENT: Self = Self(1 << 5);
    pub const KINDRED: Self = Self(1 << 6);
    pub const INSTANT: Self = Self(1 << 7);
    pub const SORCERY: Self = Self(1 << 8);
    pub const PERMANENT: Self = Self(
        Self::BATTLE.0
            | Self::PLANESWALKER.0
            | Self::LAND.0
            | Self::ARTIFACT.0
            | Self::CREATURE.0
            | Self::ENCHANTMENT.0,
    );

    /// Combine two type masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no card type bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Five-bit color mask supporting arbitrary unions and exclusions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorMask(pub u8);

impl ColorMask {
    pub const EMPTY: Self = Self(0);
    pub const WHITE: Self = Self(1 << 0);
    pub const BLUE: Self = Self(1 << 1);
    pub const BLACK: Self = Self(1 << 2);
    pub const RED: Self = Self(1 << 3);
    pub const GREEN: Self = Self(1 << 4);

    /// Combine two color masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no color bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Bitmask over Magic supertypes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupertypeMask(pub u8);

impl SupertypeMask {
    pub const EMPTY: Self = Self(0);
    pub const BASIC: Self = Self(1 << 0);
    pub const LEGENDARY: Self = Self(1 << 1);
    pub const SNOW: Self = Self(1 << 2);
    pub const WORLD: Self = Self(1 << 3);
    pub const HISTORIC: Self = Self(1 << 4);

    /// Combine two supertype masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no supertype bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Compact bitflags for status, counter, and turn-history predicates on `ObjectFilterAst`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectFilterFlags(pub u32);

impl ObjectFilterFlags {
    pub const EMPTY: Self = Self(0);
    pub const EXCLUDE_SELF: Self = Self(1 << 0);
    pub const ENTERED_THIS_TURN: Self = Self(1 << 1);
    pub const DEALT_DAMAGE_THIS_TURN: Self = Self(1 << 2);
    pub const ATTACKED_THIS_TURN: Self = Self(1 << 3);
    pub const DIED_THIS_TURN: Self = Self(1 << 4);
    pub const MODIFIED: Self = Self(1 << 5);
    pub const EQUIPPED: Self = Self(1 << 6);
    pub const ENCHANTED: Self = Self(1 << 7);
    pub const FACE_DOWN: Self = Self(1 << 8);
    pub const WITH_ANY_COUNTER: Self = Self(1 << 9);
    pub const WITHOUT_COUNTERS: Self = Self(1 << 10);
    pub const EXILED_WITH_SELF: Self = Self(1 << 11);
    pub const ATTACHED_TO_SELF: Self = Self(1 << 12);
    pub const DEALT_DAMAGE_BY_SELF_THIS_TURN: Self = Self(1 << 13);
    pub const MATCHES_CHOSEN_TYPE_OR_COLOR: Self = Self(1 << 14);
    pub const MATCHES_NAMED_AGENDA: Self = Self(1 << 15);
    /// Matches a spell or ability on the stack that has exactly one target.
    pub const SINGLE_TARGET: Self = Self(1 << 16);

    /// Combine two flag sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Color-arity predicate (`colorless`, `multicolored`, `monocolored`, `colored`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorArity {
    Colorless,
    Colored,
    Multicolored,
    Monocolored,
}

/// Controller or owner relation relative to the ability/spell controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerRelation {
    You,
    YourTeam,
    NotYou,
    Opponent,
    Opponents,
    TargetOpponent,
    TargetPlayer,
    ThatPlayer,
    DefendingPlayer,
    ItsController,
    OwnerYou,
    OwnerOpponent,
    OwnerThatPlayer,
}

/// Zone predicate on a filtered card/object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneFilter {
    Battlefield,
    Stack,
    Exile,
    YourGraveyard,
    OpponentGraveyard,
    OpponentsGraveyards,
    AnyGraveyard,
    AllGraveyards,
    ThatPlayerGraveyard,
    TargetPlayerGraveyard,
    YourHand,
    ThatPlayerHand,
    YourLibrary,
    CommandZone,
    OutsideTheGame,
    YourGraveyardOrExile,
    AnyGraveyardOrExile,
    YourHandOrGraveyard,
}

/// Combat-state predicate on a battlefield creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatState {
    Attacking,
    Blocking,
    AttackingOrBlocking,
    Blocked,
    Unblocked,
    BlockingSelf,
    BlockedBySelf,
    BlockingOrBlockedBySelf,
    NotAttacking,
}

/// Numeric comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

/// Object stat kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatKind {
    Power,
    Toughness,
    ManaValue,
    Loyalty,
}

/// Stat bound predicate inside `ObjectFilterAst`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatBoundAst {
    pub stat: StatKind,
    pub op: CompareOp,
    pub value: Box<ValueExprAst>,
}

/// Card-name predicate inside `ObjectFilterAst`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NameFilterAst {
    SelfName,
    Exact(String),
    SameAsReferenced,
}

/// Orthogonal 13-dimension object filter AST.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectFilterAst {
    pub any_types: CardTypeMask,
    pub forbidden_types: CardTypeMask,
    pub required_supertypes: SupertypeMask,
    pub forbidden_supertypes: SupertypeMask,
    pub subtypes: Vec<String>,
    pub forbidden_subtypes: Vec<String>,
    pub any_colors: ColorMask,
    pub forbidden_colors: ColorMask,
    pub color_arity: Option<ColorArity>,
    pub controller: Option<ControllerRelation>,
    pub zone: Option<ZoneFilter>,
    pub combat: Option<CombatState>,
    pub tapped: Option<bool>,
    pub token: Option<bool>,
    pub required_keywords: Vec<String>,
    pub forbidden_keywords: Vec<String>,
    pub stat_bounds: Vec<StatBoundAst>,
    pub counter_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<NameFilterAst>,
    pub flags: ObjectFilterFlags,
    pub any_permanent: bool,
    pub any_card: bool,
    pub any_spell: bool,
    pub is_ability: bool,
}

impl ObjectFilterAst {
    /// True when no predicate has been populated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.any_types.is_empty()
            && self.forbidden_types.is_empty()
            && self.required_supertypes.is_empty()
            && self.forbidden_supertypes.is_empty()
            && self.subtypes.is_empty()
            && self.forbidden_subtypes.is_empty()
            && self.any_colors.is_empty()
            && self.forbidden_colors.is_empty()
            && self.color_arity.is_none()
            && self.controller.is_none()
            && self.zone.is_none()
            && self.combat.is_none()
            && self.tapped.is_none()
            && self.token.is_none()
            && self.required_keywords.is_empty()
            && self.forbidden_keywords.is_empty()
            && self.stat_bounds.is_empty()
            && self.counter_kind.is_none()
            && self.named.is_none()
            && self.flags.is_empty()
            && !self.any_permanent
            && !self.any_card
            && !self.any_spell
            && !self.is_ability
    }
}

/// Player reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerRefAst {
    You,
    TargetPlayer,
    TargetOpponent,
    EachPlayer,
    EachOpponent,
    EachOtherOpponent,
    EachOtherPlayer,
    AnyOpponent,
    AnyPlayer,
    ThatPlayer,
    DefendingPlayer,
    ActivePlayer,
    EnchantedPlayer,
    ItsController,
    ItsOwner,
    ThatObjectController,
}

/// Target cardinality specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetCountAst {
    Exact(u16),
    UpTo(u16),
    ChosenX,
    UpToX,
    AnyNumber,
}

/// Subject or target reference inside an effect, trigger, or static clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectRefAst {
    SelfObject,
    Anaphora(String),
    AttachedHost(String),
    AnyTarget,
    TargetPlayerOrPlaneswalker {
        opponent_only: bool,
    },
    TargetPlayer {
        opponent_only: bool,
        count: TargetCountAst,
    },
    TargetObject {
        filter: Box<ObjectFilterAst>,
        count: TargetCountAst,
    },
    Player(PlayerRefAst),
    PlayerMatching {
        player: PlayerRefAst,
        condition: Box<ConditionAst>,
    },
    Each(Box<ObjectFilterAst>),
    All(Box<ObjectFilterAst>),
    Group(Box<ObjectFilterAst>),
    /// Composed multi-subject reference (e.g. `you and permanents you control`,
    /// `each creature and each planeswalker`, `up to one target land and up to one target creature`).
    Compound(Vec<SubjectRefAst>),
    TriggerSubject,
    TriggerCause,
    /// Opt-in tabletop honor-system selection (e.g. bystander pick or dexterity touch count).
    ManualTabletopSelection {
        prompt: String,
        filter: Option<Box<ObjectFilterAst>>,
    },
}

/// Player numeric statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerStatKind {
    LifeTotal,
    CardsInHand,
    CardsInLibrary,
    CardsInGraveyard,
}

/// Aggregation operator over a filtered set of objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregateOp {
    Max,
    Min,
    Sum,
}

/// Rounding direction for integer division.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rounding {
    Down,
    Up,
}

/// Recursive dynamic numeric expression AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueExprAst {
    Const(i32),
    ChosenX,
    PriorResult,
    Count(Box<ObjectFilterAst>),
    ObjectStat {
        subject: Box<SubjectRefAst>,
        stat: StatKind,
    },
    CounterCount {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
    },
    PlayerStat {
        player: PlayerRefAst,
        stat: PlayerStatKind,
    },
    AggregateStat {
        op: AggregateOp,
        stat: StatKind,
        filter: Box<ObjectFilterAst>,
    },
    Add(Box<ValueExprAst>, Box<ValueExprAst>),
    Sub(Box<ValueExprAst>, Box<ValueExprAst>),
    Mul(Box<ValueExprAst>, Box<ValueExprAst>),
    DivConst {
        expr: Box<ValueExprAst>,
        denom: i32,
        rounding: Rounding,
    },
    RollDie {
        sides: u16,
    },
    ColorsOfManaSpent,
    DomainCount,
    CardTypesInGraveyard {
        all_graveyards: bool,
    },
    Devotion {
        colors: String,
    },
    TimesKicked,
    OpponentsCount,
    ColorsAmong(Box<ObjectFilterAst>),
    CardTypesAmong(Box<ObjectFilterAst>),
    SubtypesAmong(Box<ObjectFilterAst>),
    PlayerCounterCount {
        player: PlayerRefAst,
        counter_kind: String,
    },
    StartingLifeTotal(PlayerRefAst),
    SelfColorsCount,
    /// Player-level aggregation (e.g. lowest life total among all players, fewest lands controlled).
    AggregateOverPlayers {
        op: AggregateOp,
        metric: Box<ValueExprAst>,
    },
    /// Amount of life a player gained or lost this turn.
    LifeChangedThisTurn {
        player: PlayerRefAst,
        gained: bool,
    },
    /// Total mana (or specific mana type like `{S}`) spent to cast the spell.
    ManaSpentToCast {
        mana_filter: Option<String>,
    },
    /// Number of creatures in your party (0..=4).
    PartySize,
    /// Total number of cards in all players' hands.
    TotalCardsInAllHands,
    /// Opt-in tabletop honor-system integer input (e.g. bystander number, dexterity count).
    ManualTabletopNumber {
        prompt: String,
    },
}

/// Effect duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurationAst {
    Instant,
    UntilEndOfCombat,
    UntilEndOfTurn,
    UntilYourNextTurn,
    UntilEndOfNextTurn,
    Permanent,
    UntilSourceLeaves,
    ForAsLongAs(Box<ConditionAst>),
}

/// Closed typed Magic game-state, turn-history, and zone markers (replaces untyped `GameStateFlag` strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMarker {
    CitysBlessing,
    Monarch(PlayerRefAst),
    NoMonarch,
    Initiative(PlayerRefAst),
    Day,
    Night,
    NeitherDayNorNight,
    MaxSpeed,
    FullParty,
    DescendedThisTurn,
    CommittedCrimeThisTurn,
    CompletedDungeon,
    Bargained,
    GiftPromised,
    Foretold,
    CastFromGraveyard,
    CastFromExile,
    CastFromOutsideHand,
    CastUsingTeamwork,
    EnteredOrCastFromGraveyard,
    YouAreAttacking,
    YouAreDefending,
    FirstCombatPhase,
    ControlsSelf,
    RemainsTapped,
    RemainsOnBattlefield,
    RemainsExiled,
    AttackingAlone,
    AttackedOrBlockedThisCombat,
    AttackedThisTurn {
        subject: Box<SubjectRefAst>,
    },
    OpponentDealtDamageThisTurn {
        noncombat_only: bool,
    },
    CardLeftGraveyardThisTurn,
    NonlandPermanentLeftThisTurn,
    SpellWarpedThisTurn,
    DamagePreventedThisWay,
    SingleTarget,
    IsSuspended,
    PairedWithAnotherCreature,
    NoCreaturesOnBattlefield,
    NotManaAbility,
    NotDeclaredAttacker,
    RingBearerOtherThanSelf,
    TargetsCommander,
    SharesCardTypeWithExiled,
    SameManaValueAsDiscarded,
    SameNameAsTarget,
    InZone {
        subject: Box<SubjectRefAst>,
        zone: ZoneFilter,
    },
    OwnerHasCardInExile {
        player: PlayerRefAst,
    },
    PlayerIsPoisoned {
        player: PlayerRefAst,
    },
    PlayerHasRadCounter {
        player: PlayerRefAst,
    },
    DidNotPutToZone {
        zone: String,
    },
    HasNotPlayedOrCast,
    ControlledSubject(Box<SubjectRefAst>),
    VoteResult {
        winning: String,
    },
    PayCostCondition(Box<CostAtomAst>),
    AttackedWithMinCreaturesThisTurn {
        player: PlayerRefAst,
        min_count: u16,
    },
    DrewCardsThisTurn {
        player: PlayerRefAst,
        op: CompareOp,
        value: u16,
    },
    CastSpellsThisTurn {
        player: PlayerRefAst,
        op: CompareOp,
        value: u16,
        from_hand_only: bool,
    },
    DifferentManaValuesInGraveyard {
        op: CompareOp,
        value: u16,
    },
    PlayersLostGame {
        op: CompareOp,
        value: u16,
    },
    AbilityActivatedThisTurn {
        op: CompareOp,
        value: u16,
    },
    PlayerGainedOrLostLifeThisTurn {
        player: PlayerRefAst,
    },
    ControlsNeither,
    ControlsAllThree,
    ControlsMoreThanOpponents {
        filter: Box<ObjectFilterAst>,
        than_each_other: bool,
    },
    ControlsFewerThanOpponent {
        filter: Box<ObjectFilterAst>,
    },
    HandMoreThanEachOpponent,
    InCommandZoneOrBattlefield,
    DidNotAttackThisTurn {
        subject: Box<SubjectRefAst>,
        or_entered_this_turn: bool,
    },
    DiedThisWay,
    AttackedOrBlockedThisTurn {
        subject: Box<SubjectRefAst>,
    },
    CardPutIntoGraveyardThisTurn {
        filter: Box<ObjectFilterAst>,
    },
    SacrificedThisTurn {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    ControlsNamed {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        name: String,
    },
    GreaterPowerOrToughnessThan {
        subject: Box<SubjectRefAst>,
        other: Box<SubjectRefAst>,
    },
    AnyEnteredOrCastFromGraveyard,
    HasUnattackedOpponent,
    LifePaidToActivate,
    AbilityResolvedNthTimeThisTurn(u8),
    CastDuringMainPhase,
    Escaped,
    ZoneActionHappenedThisWay {
        filter: Box<ObjectFilterAst>,
        action: String,
    },
    SameManaValueAsRevealed,
    LostLifeLastTurn {
        player: PlayerRefAst,
    },
    EvidenceCollected,
    PlayerHasMoreHandThan {
        player: PlayerRefAst,
        than: PlayerRefAst,
    },
    AttackedThisCombat {
        subject: Box<SubjectRefAst>,
    },
    DidNotCastThisTurn {
        player: PlayerRefAst,
        filter: Option<Box<ObjectFilterAst>>,
    },
    DidNotAttackWithCreatureThisTurn {
        player: PlayerRefAst,
    },
    ManaSpentLessThanManaValue,
    SubjectEnteredThisTurn(Box<SubjectRefAst>),
    SubjectWasCast {
        subject: Box<SubjectRefAst>,
        was_cast: bool,
    },
    CreatedTokenThisTurn {
        player: PlayerRefAst,
    },
    LandOrSpellOutsideHandThisTurn {
        player: PlayerRefAst,
    },
    DamageDealtToSubjectThisTurn {
        subject: Box<SubjectRefAst>,
        min_amount: u16,
    },
    ControlsGreatestStat {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        stat: StatKind,
    },
    OnlyObjectInZone {
        subject: Box<SubjectRefAst>,
        filter: Box<ObjectFilterAst>,
        zone: ZoneFilter,
    },
    KeywordCostPaid(String),
    PutCountersOnSubjectThisTurn {
        player: PlayerRefAst,
        subject: Box<SubjectRefAst>,
        counter_kind: String,
    },
    AllMatchingAre {
        filter: Box<ObjectFilterAst>,
        predicate: Box<ObjectFilterAst>,
    },
}

/// Condition AST used by intervening-if triggers, resolution branches, and conditional statics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionAst {
    /// Orthogonal numeric comparison across any two `ValueExprAst` expressions.
    Compare {
        lhs: Box<ValueExprAst>,
        op: CompareOp,
        rhs: Box<ValueExprAst>,
    },
    /// Typed closed game-state or turn-history marker.
    Marker(GameMarker),
    WasKicked,
    /// CR 702.33c multi-slot / "and/or" kicker check (`If ~ was kicked with its {B} kicker`).
    WasKickedSlot(String),
    WasCast {
        by_you: bool,
        from_hand: bool,
    },
    Morbid {
        your_control_only: bool,
    },
    Revolt,
    Threshold,
    Delirium,
    Metalcraft,
    IsYourTurn,
    GainedLifeThisTurn,
    OpponentLostLifeThisTurn,
    AttackedThisTurn,
    EnteredThisTurn,
    ControlsCommander,
    TributeNotPaid,
    IsNotToken,
    HasKeyword {
        subject: Box<SubjectRefAst>,
        keyword: String,
    },
    CastSpellThisTurn {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    OpponentMoreThanYou {
        metric: String,
        filter: Option<Box<ObjectFilterAst>>,
    },
    SharesTypeWithSelf,
    PlayerCounterCompare {
        player: PlayerRefAst,
        counter_kind: String,
        op: CompareOp,
        value: u16,
    },
    OpponentCountCompare {
        op: CompareOp,
        value: u16,
    },
    TotalStatCompare {
        filter: Box<ObjectFilterAst>,
        stat: StatKind,
        op: CompareOp,
        value: i32,
    },
    ManaSpentToCast(String),
    TargetsFilter(Box<ObjectFilterAst>),
    PlayerActionThisTurn {
        player: PlayerRefAst,
        action: String,
    },
    WonContest,
    GameStateFlag(String),
    SearchedLibraryThisWay,
    SpellsCastLastTurn {
        any_player_single: bool,
        op: CompareOp,
        value: u16,
    },
    HasStatus {
        subject: Box<SubjectRefAst>,
        status: String,
    },
    ControlsCount {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        op: CompareOp,
        value: u16,
    },
    ZoneObjectCount {
        filter: Box<ObjectFilterAst>,
        op: CompareOp,
        value: u16,
    },
    LifeCompare {
        player: PlayerRefAst,
        op: CompareOp,
        value: i32,
    },
    HandCount {
        player: PlayerRefAst,
        op: CompareOp,
        value: u16,
    },
    StatCompare {
        subject: Box<SubjectRefAst>,
        stat: StatKind,
        op: CompareOp,
        value: i32,
    },
    CounterCompare {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        op: CompareOp,
        value: u16,
    },
    MatchesFilter {
        subject: Box<SubjectRefAst>,
        filter: Box<ObjectFilterAst>,
    },
    EnteredUnderYourControlThisTurn {
        filter: Box<ObjectFilterAst>,
        min_count: u16,
    },
    GraveyardCount {
        player: PlayerRefAst,
        op: CompareOp,
        value: u16,
    },
    DevotionCompare {
        colors: String,
        op: CompareOp,
        value: u16,
    },
    LostLifeThisTurn {
        player: PlayerRefAst,
        min_amount: i32,
    },
    Not(Box<ConditionAst>),
    And(Box<ConditionAst>, Box<ConditionAst>),
    Or(Box<ConditionAst>, Box<ConditionAst>),
    /// Opt-in tabletop honor-system yes/no check (e.g. attire, physical feat, social prompt).
    ManualTabletopCheck {
        prompt: String,
    },
}

/// Mana production specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSpecAst {
    Fixed(String),
    Choice(Vec<String>),
    AnyColor {
        amount: Box<ValueExprAst>,
        single_color: bool,
    },
    DifferentColors {
        amount: Box<ValueExprAst>,
    },
    ScaledSymbol {
        symbol: String,
        amount: Box<ValueExprAst>,
    },
    CombinationOf {
        amount: Box<ValueExprAst>,
        symbols: Vec<String>,
    },
    AnyLandCouldProduce {
        controller: ControllerRelation,
    },
    ChosenColor,
    CommanderColorIdentity,
}

/// Token creation specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenSpecAst {
    Predefined {
        name: String,
        count: Box<ValueExprAst>,
        tapped: bool,
    },
    Copy {
        count: Box<ValueExprAst>,
        source: Box<SubjectRefAst>,
        tapped: bool,
        attacking: bool,
        exception: Option<String>,
    },
    Creature {
        count: Box<ValueExprAst>,
        power: String,
        toughness: String,
        colors: ColorMask,
        supertypes: SupertypeMask,
        card_types: CardTypeMask,
        subtypes: Vec<String>,
        keywords: Vec<String>,
        tapped: bool,
        attacking: bool,
        named: Option<String>,
    },
    Role {
        role_name: String,
        attached_to: Box<SubjectRefAst>,
    },
}

/// Orthogonal activation or additional cost atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CostAtomAst {
    PayMana(String),
    PayManaChoice(Vec<String>),
    PayScaledMana {
        base: String,
        per: String,
    },
    Behold(Box<ObjectFilterAst>),
    TapSelf,
    UntapSelf,
    PayLife(Box<ValueExprAst>),
    PayHalfLife,
    SacrificeSelf,
    Sacrifice {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
    },
    DiscardSelf,
    DiscardHand,
    Discard {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
        random: bool,
    },
    ExileSelf {
        from_graveyard: bool,
        from_hand: bool,
    },
    Exile {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
    },
    ExileTopLibrary {
        count: Box<ValueExprAst>,
        face_down: bool,
    },
    SacrificeAnyNumber(Box<ObjectFilterAst>),
    RemoveCounters {
        all: bool,
        count: Option<Box<ValueExprAst>>,
        counter_kind: String,
        subject: Box<SubjectRefAst>,
    },
    PutCounters {
        count: Box<ValueExprAst>,
        counter_kind: String,
        subject: Box<SubjectRefAst>,
    },
    TapPermanent {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
    },
    UntapPermanent {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
    },
    BounceToHand(Box<SubjectRefAst>),
    Reveal(Box<SubjectRefAst>),
    Mill(Box<ValueExprAst>),
    ExertSelf,
    PayEnergy(u16),
    PayEnergyExpr(Box<ValueExprAst>),
    PayTickets(u16),
    CollectEvidence(Box<ValueExprAst>),
    Forage,
    Blight(Box<ValueExprAst>),
    Waterbend(String),
    ChaosCost,
    /// Eldrazi Processor cost (`Put a card an opponent owns from exile into that player's graveyard`).
    ProcessExiledCard,
    Unattach {
        subject: Box<SubjectRefAst>,
        from: Option<Box<SubjectRefAst>>,
    },
    RollDie(u8),
    Choose(Box<SubjectRefAst>),
    MoveZone {
        subject: Box<SubjectRefAst>,
        to: String,
    },
    Or(Box<CostAtomAst>, Box<CostAtomAst>),
    /// Opt-in tabletop honor-system physical/social activation cost.
    ManualTabletopAction {
        prompt: String,
    },
}

/// Turn step for `StepBegan` and delayed triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStepAst {
    UntapStep,
    Upkeep,
    DrawStep,
    PrecombatMain,
    PostcombatMain,
    MainPhase,
    BeginningOfCombat,
    EndOfCombat,
    EndStep,
    CleanupStep,
}

/// Parameterized trigger event AST (covers Base Magic, Planechase, Archenemy, and ManualTabletopClaim).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerEventAst {
    StepBegan {
        step: TurnStepAst,
        whose: String,
    },
    ZoneChange {
        from: Option<String>,
        to: Option<String>,
        subject: Box<SubjectRefAst>,
        grouped: bool,
        entering_controller: Option<ControllerRelation>,
    },
    DamageDealt {
        combat_only: bool,
        noncombat_only: bool,
        source: Option<Box<SubjectRefAst>>,
        recipient: Option<Box<SubjectRefAst>>,
        grouped: bool,
    },
    Attacks {
        subject: Option<Box<SubjectRefAst>>,
        actor: Option<PlayerRefAst>,
        qualifier: Option<String>,
        grouped: bool,
    },
    AttacksWithOthers {
        subject: Box<SubjectRefAst>,
        min_others: u8,
        other_filter: Box<ObjectFilterAst>,
    },
    AttacksUnblocked {
        subject: Box<SubjectRefAst>,
    },
    Blocks {
        subject: Box<SubjectRefAst>,
        attacker: Option<Box<SubjectRefAst>>,
    },
    BecomesBlocked {
        subject: Box<SubjectRefAst>,
        blocker: Option<Box<SubjectRefAst>>,
    },
    SpellCast {
        caster: PlayerRefAst,
        spell: Option<Box<ObjectFilterAst>>,
        self_spell: bool,
        targets: Option<Box<SubjectRefAst>>,
        nth_each_turn: Option<u8>,
        include_copy: bool,
    },
    CardDrawn {
        player: PlayerRefAst,
        qualifier: String,
    },
    CardDiscarded {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    PermanentSacrificed {
        player: PlayerRefAst,
        subject: Box<SubjectRefAst>,
    },
    LifeChanged {
        player: PlayerRefAst,
        gained: bool,
        first_each_turn: bool,
    },
    Cycled {
        subject: Box<SubjectRefAst>,
    },
    TapStateChanged {
        subject: Box<SubjectRefAst>,
        tapped: bool,
        for_mana: bool,
    },
    BecomesTargeted {
        subject: Box<SubjectRefAst>,
        by_opponent: bool,
        by_text: Option<String>,
    },
    TurnedFaceUp {
        subject: Box<SubjectRefAst>,
    },
    Transforms {
        subject: Box<SubjectRefAst>,
    },
    CountersChanged {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        added: bool,
        by_you: bool,
    },
    StateTrigger(Box<ConditionAst>),
    // Modern & supplemental Magic keyword/mechanic triggers
    Mutates {
        subject: Box<SubjectRefAst>,
    },
    UnlockDoor {
        subject: Box<SubjectRefAst>,
    },
    CrankContraption,
    BecomesMonstrous {
        subject: Box<SubjectRefAst>,
    },
    BecomesRenowned {
        subject: Box<SubjectRefAst>,
    },
    Exploits {
        subject: Box<SubjectRefAst>,
    },
    BecomesAttached {
        subject: Box<SubjectRefAst>,
        attachment_kind: String,
    },
    Exerted {
        subject: Box<SubjectRefAst>,
    },
    Crewed {
        subject: Box<SubjectRefAst>,
    },
    Explores {
        subject: Box<SubjectRefAst>,
    },
    Connives {
        subject: Box<SubjectRefAst>,
    },
    CrimeCommitted {
        player: PlayerRefAst,
    },
    KeywordActionPerformed {
        player: PlayerRefAst,
        action: String,
    },
    RollDie {
        player: PlayerRefAst,
        exact_result: Option<u16>,
    },
    FlipCoin {
        player: PlayerRefAst,
        won: Option<bool>,
    },
    ClassLevelReached {
        level: u8,
    },
    CaseSolved,
    BattleDefeated,
    LandPlayed {
        player: PlayerRefAst,
    },
    PhaseChanged {
        subject: Box<SubjectRefAst>,
        phased_out: bool,
    },
    Exiled {
        subject: Box<SubjectRefAst>,
    },
    Expend {
        player: PlayerRefAst,
        amount: u16,
    },
    CastFromOutsideHand {
        player: PlayerRefAst,
    },
    RoomFullyUnlocked {
        player: PlayerRefAst,
    },
    LibrarySearched {
        player: PlayerRefAst,
    },
    CardMilled {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    HauntedCreatureDies,
    DayNightChanged,
    PlayerAttacked {
        player: PlayerRefAst,
    },
    AbilityActivated {
        player: PlayerRefAst,
        source_filter: Box<ObjectFilterAst>,
    },
    SpellOrAbilityOnStack,
    // Supplemental Paper Formats: Planechase (207) & Archenemy (102)
    PlaneswalkToSelf,
    PlaneswalkAwayFromSelf,
    ChaosEnsues,
    EncounterPhenomenon,
    SchemeSetInMotion,
    // Tabletop Honor-System Un-cards (Gotcha / voice / social triggers)
    ManualTabletopClaim {
        prompt: String,
        any_player_can_trigger: bool,
    },
}

/// Complete trigger header with compound `AnyOf` events and optional intervening-`if`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerSpecAst {
    pub events: Vec<TriggerEventAst>,
    pub intervening_if: Option<ConditionAst>,
}

/// Modal option body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalModeAst {
    pub body: Vec<InstructionAst>,
}

/// Kind of orthogonal pairwise exchange (`Exchange` instruction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExchangeKind {
    Control,
    LifeTotals,
    LifeWithStat(StatKind),
    Zones,
}

/// Interactive multiplayer / opponent / hidden choice protocol (`InteractiveChoice` instruction).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractiveChoiceKind {
    VillainousChoice {
        chooser: PlayerRefAst,
        options: Vec<Vec<InstructionAst>>,
    },
    SecretVote {
        prompt: String,
    },
    SecretNumberChoice {
        min_value: i32,
    },
    BiddingAction {
        description: String,
    },
    SeparateIntoPiles {
        chooser: PlayerRefAst,
        subject: Box<SubjectRefAst>,
        pile_count: u8,
    },
    ChooseFromGroup {
        chooser: PlayerRefAst,
        description: String,
    },
}

/// Which player's or turn's step fires a delayed step trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelayedStepScope {
    /// "the next <step>" or "at end of combat" (any player's next matching step).
    Next,
    /// "your next <step>" / "your <step>".
    YourNext,
    /// "that player's next <step>" / "their next <step>".
    ThatPlayersNext,
    /// "each <step>".
    Each,
    /// "each player's <step>".
    EachPlayers,
    /// "each opponent's <step>".
    EachOpponents,
    /// "each of your <step>s".
    EachOfYour,
}

/// Core parameterized Magic instruction AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionAst {
    // Control flow & scoping combinators
    BindX {
        x_expr: Box<ValueExprAst>,
        body: Vec<InstructionAst>,
    },
    Optional(Vec<InstructionAst>),
    OptionalPlayer {
        player: PlayerRefAst,
        body: Vec<InstructionAst>,
    },
    IfPreviousSucceeded(Vec<InstructionAst>),
    IfPreviousFailed(Vec<InstructionAst>),
    Otherwise(Vec<InstructionAst>),
    ReplaceEffect(Vec<InstructionAst>),
    ReflexiveTrigger(Vec<InstructionAst>),
    IfCondition {
        condition: Box<ConditionAst>,
        then_body: Vec<InstructionAst>,
    },
    ForEachPlayer {
        player: PlayerRefAst,
        body: Vec<InstructionAst>,
    },
    ForEachObject {
        filter: Box<ObjectFilterAst>,
        body: Vec<InstructionAst>,
    },
    ForEachCount {
        count: Box<ValueExprAst>,
        body: Vec<InstructionAst>,
    },
    Unless {
        body: Vec<InstructionAst>,
        unless_player: Option<PlayerRefAst>,
        unless_cost: Option<CostAtomAst>,
        unless_action: Option<Vec<InstructionAst>>,
        unless_condition: Option<Box<ConditionAst>>,
    },
    /// Disjunctive imperative choice combinator (`sacrifice an artifact or discard a card`).
    Choice(Vec<Vec<InstructionAst>>),
    DelayedTrigger {
        step: TurnStepAst,
        scope: DelayedStepScope,
        body: Vec<InstructionAst>,
    },
    DelayedEventTrigger {
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
    },
    Modal {
        choice: String,
        modes: Vec<ModalModeAst>,
    },
    SpreeMode {
        cost: Vec<CostAtomAst>,
        body: Vec<InstructionAst>,
    },
    AllowRepeatModes,
    AbilityRestriction {
        restriction: String,
        condition: Option<Box<ConditionAst>>,
    },
    ManaSpendRestriction(String),

    // Core Magic actions
    PayCost(CostAtomAst),
    PlayerPayCost {
        player: PlayerRefAst,
        cost: CostAtomAst,
    },
    PayCostAnyNumberOfTimes(CostAtomAst),
    AddMana {
        player: PlayerRefAst,
        mana: ManaSpecAst,
    },
    RetainManaUntilEndOfTurn,
    Draw {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    DealDamage {
        source: Box<SubjectRefAst>,
        target: Box<SubjectRefAst>,
        amount: Box<ValueExprAst>,
    },
    DealDamageDivided {
        source: Box<SubjectRefAst>,
        amount: Box<ValueExprAst>,
        targets_desc: String,
    },
    SwitchPowerToughness {
        subject: Box<SubjectRefAst>,
        duration: DurationAst,
    },
    ExileOrRevealTopUntil {
        exile: bool,
        library: String,
        filter: Box<ObjectFilterAst>,
    },
    LookAtHand {
        player: PlayerRefAst,
    },
    ShuffleGraveyardIntoLibrary {
        player: PlayerRefAst,
    },
    BecomeChosenCharacteristic {
        subject: Box<SubjectRefAst>,
        kind: String,
        duration: DurationAst,
    },
    ModifyPT {
        subject: Box<SubjectRefAst>,
        dx: String,
        dy: String,
        duration: DurationAst,
        scale: Option<Box<ValueExprAst>>,
    },
    ModifyPTChoice {
        subject: Box<SubjectRefAst>,
        choices: Vec<(String, String)>,
        duration: DurationAst,
    },
    GrantKeywords {
        subject: Box<SubjectRefAst>,
        keywords: Vec<String>,
        duration: DurationAst,
    },
    RemoveKeywords {
        subject: Box<SubjectRefAst>,
        keywords: Vec<String>,
        duration: DurationAst,
    },
    RemoveAllAbilities {
        subject: Box<SubjectRefAst>,
        duration: DurationAst,
    },
    GrantAbilityText {
        subject: Box<SubjectRefAst>,
        ability_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compiled: Option<Box<FaceAst>>,
        duration: DurationAst,
    },
    ChangeLife {
        player: PlayerRefAst,
        delta: Box<ValueExprAst>,
        gain: bool,
    },
    CreateToken {
        controller: PlayerRefAst,
        token: TokenSpecAst,
    },
    PutCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        count: Box<ValueExprAst>,
    },
    RemoveCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        all: bool,
        count: Option<Box<ValueExprAst>>,
    },
    MoveCounters {
        from: Box<SubjectRefAst>,
        to: Box<SubjectRefAst>,
        counter_kind: String,
        all: bool,
        any_number: bool,
        count: Option<Box<ValueExprAst>>,
    },
    Proliferate,
    Destroy {
        subject: Box<SubjectRefAst>,
    },
    ModifyPreviousDestroyNoRegen,
    Exile {
        actor: Option<PlayerRefAst>,
        subject: Box<SubjectRefAst>,
    },
    ExileUntilLeaves {
        subject: Box<SubjectRefAst>,
    },
    ExileTopLibrary {
        actor: Option<PlayerRefAst>,
        whose: String,
        count: Box<ValueExprAst>,
        face_down: bool,
    },
    ExileGraveyard {
        whose: String,
    },
    Sacrifice {
        player: PlayerRefAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
        subject: Box<SubjectRefAst>,
    },
    SacrificeAnyNumber {
        player: PlayerRefAst,
        subject: Box<SubjectRefAst>,
    },
    SacrificeUnlessPay {
        subject: Box<SubjectRefAst>,
        cost: CostAtomAst,
    },
    Discard {
        player: PlayerRefAst,
        count: Option<Box<ValueExprAst>>,
        filter: Option<Box<ObjectFilterAst>>,
        anaphora: Option<String>,
        random: bool,
    },
    DiscardHand {
        player: PlayerRefAst,
    },
    ReturnZone {
        subject: Box<SubjectRefAst>,
        from_zone: Option<String>,
        to: String,
        tapped: bool,
        transformed: bool,
        with_counter: Option<String>,
        attached_to: Option<Box<SubjectRefAst>>,
    },
    MoveZone {
        subject: Box<SubjectRefAst>,
        to: String,
    },
    /// Counted, `up to`, or `any number of` non-targeted zone movement (e.g., Brainstorm, Valakut Awakening, Vessel of Malignity).
    MoveCounted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<PlayerRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
        #[serde(default)]
        up_to: bool,
        #[serde(default)]
        any_number: bool,
        subject: Box<SubjectRefAst>,
        to: String,
    },
    ChooseAndKeep {
        pick: String,
        keep_to: String,
        rest_to: String,
    },
    RevealFromLooked {
        filter: Box<ObjectFilterAst>,
        to: String,
    },
    SearchLibrary {
        library: String,
        count: Box<ValueExprAst>,
        #[serde(default)]
        up_to: bool,
        #[serde(default)]
        any_number: bool,
        filter: Box<ObjectFilterAst>,
        reveal: bool,
        to_zone: String,
        shuffle: bool,
    },
    ShuffleLibrary {
        player: PlayerRefAst,
    },
    Scry {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    Surveil {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    Mill {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    LookTopLibrary {
        reveal: bool,
        library: String,
        count: Box<ValueExprAst>,
    },
    RevealHand {
        player: PlayerRefAst,
    },
    Counter {
        target: Box<SubjectRefAst>,
        unless_pay: Option<CostAtomAst>,
        unless_action: Option<Vec<InstructionAst>>,
    },
    TapPermanent {
        subject: Box<SubjectRefAst>,
    },
    UntapPermanent {
        subject: Box<SubjectRefAst>,
    },
    TapOrUntapPermanent {
        subject: Box<SubjectRefAst>,
    },
    SkipNextUntap {
        subject: Box<SubjectRefAst>,
    },
    Regenerate {
        subject: Box<SubjectRefAst>,
    },
    Transform {
        subject: Box<SubjectRefAst>,
    },
    PhaseOut {
        subject: Box<SubjectRefAst>,
    },
    Fight {
        a: Box<SubjectRefAst>,
        b: Box<SubjectRefAst>,
    },
    Attach {
        subject: Box<SubjectRefAst>,
        target: Box<SubjectRefAst>,
    },
    GainControl {
        controller: PlayerRefAst,
        subject: Box<SubjectRefAst>,
        duration: DurationAst,
    },
    PreventDamage {
        amount: Box<ValueExprAst>,
        target: Box<SubjectRefAst>,
        duration: DurationAst,
    },
    PreventAllDamage {
        combat_only: bool,
        target: Option<Box<SubjectRefAst>>,
        duration: DurationAst,
    },
    Copy {
        subject: Box<SubjectRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exception: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
    },
    MayChooseNewTargetsForCopy,
    GrantCastPermission {
        player: PlayerRefAst,
        cast_only: bool,
        subject: Box<SubjectRefAst>,
        free: bool,
        duration: String,
    },
    CombatRuleMod {
        subject: Box<SubjectRefAst>,
        rule: String,
        duration: DurationAst,
    },
    AnimateOrSetBasePT {
        subject: Box<SubjectRefAst>,
        power: String,
        toughness: String,
        description: Option<String>,
        keywords: Vec<String>,
        duration: DurationAst,
    },
    KeepExistingTypes(String),
    OneShotReplacement {
        subject: String,
        event: String,
        replace_with: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instead_body: Option<Vec<InstructionAst>>,
    },
    ChooseOption(String),
    AdditionalLandPlay {
        count: u8,
        duration: DurationAst,
    },
    AdditionalCombat(String),
    ExtraTurn {
        player: PlayerRefAst,
    },
    WinGame {
        player: PlayerRefAst,
    },
    LoseGame {
        player: PlayerRefAst,
    },
    CreateEmblem(String),
    // Named CR 701 keyword actions & player counters
    Investigate {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    BecomeMonarch {
        player: PlayerRefAst,
    },
    TakeInitiative {
        player: PlayerRefAst,
    },
    VentureIntoDungeon {
        player: PlayerRefAst,
    },
    RingTemptsYou,
    GainPlayerCounters {
        player: PlayerRefAst,
        counter_kind: String,
        count: Box<ValueExprAst>,
    },
    Monstrosity {
        count: Box<ValueExprAst>,
    },
    Adapt {
        count: Box<ValueExprAst>,
    },
    Amass {
        subtype: String,
        count: Box<ValueExprAst>,
    },
    Explore {
        subject: Box<SubjectRefAst>,
    },
    Populate {
        count: Box<ValueExprAst>,
    },
    Detain {
        subject: Box<SubjectRefAst>,
    },
    Suspect {
        subject: Box<SubjectRefAst>,
    },
    Connive {
        subject: Box<SubjectRefAst>,
        count: Box<ValueExprAst>,
    },
    Incubate {
        count: Box<ValueExprAst>,
    },
    Manifest {
        count: Box<ValueExprAst>,
        dread: bool,
    },
    Cloak {
        count: Box<ValueExprAst>,
        subject: Option<Box<SubjectRefAst>>,
    },
    Discover {
        value: Box<ValueExprAst>,
    },
    CollectEvidence {
        value: Box<ValueExprAst>,
    },
    Forage,
    Bolster {
        count: Box<ValueExprAst>,
    },
    Support {
        count: Box<ValueExprAst>,
    },
    TimeTravel,
    OpenAttraction,
    RollVisitAttractions,
    Clash {
        with_opponent: bool,
    },
    Fateseal {
        count: Box<ValueExprAst>,
    },
    BecomePrepared {
        subject: Box<SubjectRefAst>,
    },
    Endure {
        count: Box<ValueExprAst>,
    },
    Blight {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
    },
    Exert {
        subject: Box<SubjectRefAst>,
    },
    UnlockDoor {
        subject: Box<SubjectRefAst>,
    },
    Learn,
    Earthbend {
        count: Box<ValueExprAst>,
    },
    Airbend {
        subject: Option<Box<SubjectRefAst>>,
    },
    Empower {
        planeswalker: String,
        count: Box<ValueExprAst>,
    },
    Teamwork {
        count: Box<ValueExprAst>,
    },
    ExileWithCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        count: Box<ValueExprAst>,
    },
    ShuffleIntoLibrary {
        subject: Box<SubjectRefAst>,
    },
    SetLifeTotal {
        player: PlayerRefAst,
        value: Box<ValueExprAst>,
    },
    RemoveFromCombat {
        subject: Box<SubjectRefAst>,
    },
    DiscardUnless {
        player: PlayerRefAst,
        count: Box<ValueExprAst>,
        unless_filter: Box<ObjectFilterAst>,
    },
    PutSticker {
        kind: String,
        target: Box<SubjectRefAst>,
    },
    PreventNextSourceDamage {
        source_desc: String,
        target: Box<SubjectRefAst>,
    },
    ChangeTargets {
        target: Box<SubjectRefAst>,
    },
    SeparateIntoPiles {
        chooser: PlayerRefAst,
        pile_count: u8,
    },
    ChoosePile {
        chooser: PlayerRefAst,
    },
    WheelShuffleHandAndGraveyard {
        player: PlayerRefAst,
        draw: Box<ValueExprAst>,
    },
    AddManaLandProduced {
        player: PlayerRefAst,
    },
    ReorderTopLibrary,
    Recruit,
    EndTurn,
    SkipNextTurn {
        player: PlayerRefAst,
    },
    SkipNextStep {
        player: PlayerRefAst,
        step: TurnStepAst,
    },
    CopyForCommanderCasts,
    ChaosEnsues,
    ChangeText {
        target: Box<SubjectRefAst>,
        description: String,
    },
    ExileAllNamedFromZones {
        subject: Box<SubjectRefAst>,
    },
    BecomeCopy {
        subject: Box<SubjectRefAst>,
        source: Box<SubjectRefAst>,
        duration: DurationAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exception: Option<String>,
    },
    RedirectDamage {
        amount: Box<ValueExprAst>,
        from: Box<SubjectRefAst>,
        to: Box<SubjectRefAst>,
    },
    AbilityCostReduction {
        delta: String,
        per: Box<ObjectFilterAst>,
    },
    DoubleStatOrCounters {
        subject: Box<SubjectRefAst>,
        kind: String,
        duration: DurationAst,
    },
    Vote {
        options: String,
    },
    RevealRandomFromHand {
        player: PlayerRefAst,
    },
    RollDiceChooseOne {
        count: u8,
        sides: u16,
    },
    Planeswalk,
    SetNextSchemeInMotion,
    AssembleContraption,
    // Archenemy Ongoing Scheme abandon action
    AbandonSourceScheme,
    // Tabletop Honor-System standalone physical/social action
    ManualTabletopAction {
        prompt: String,
    },
    // Deterministic RNG (Un-cards & d20/coin-flip cards)
    RollDie {
        sides: u16,
    },
    FlipCoin,
    // Pass C orthogonal engine primitives
    ControlPlayerTurn {
        player: PlayerRefAst,
        duration: DurationAst,
    },
    Exchange {
        kind: ExchangeKind,
        left: Box<SubjectRefAst>,
        right: Option<Box<SubjectRefAst>>,
    },
    WishFromOutsideGame {
        filter: Box<ObjectFilterAst>,
        count: Box<ValueExprAst>,
        to_zone: String,
        allow_exile: bool,
    },
    Equalize {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        action: String,
    },
    InteractiveChoice(InteractiveChoiceKind),
    DistributeCounters {
        counter_kind: String,
        count: Box<ValueExprAst>,
        targets: Box<SubjectRefAst>,
    },
    RepeatProcess {
        condition: Option<Box<ConditionAst>>,
        max_times: Option<u8>,
        any_number: bool,
    },
    LookAtExiled {
        subject: Box<SubjectRefAst>,
        also_play: bool,
        condition: Box<ConditionAst>,
    },
    /// Turn-scoped or condition-scoped temporary continuous static effect created during resolution.
    TemporaryStatic {
        static_ability: Box<StaticAbilityAst>,
        duration: DurationAst,
    },
    /// CR 603.7 delayed / duration-scoped triggered ability created during resolution.
    TemporaryTrigger {
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
        duration: DurationAst,
    },
    /// CR 614 duration-scoped replacement effect created during resolution.
    TemporaryReplacement {
        replacement: Box<ReplacementEffectAst>,
        duration: DurationAst,
    },
    /// CR 701.16 reveal cards from the top of a library until matching card(s) are revealed.
    RevealUntil {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        count: Box<ValueExprAst>,
    },
    /// CR 608.2h / CR 613.4b dynamic power/toughness assignment using live or LKI `ValueExprAst`.
    SetDynamicPT {
        subject: Box<SubjectRefAst>,
        power: Option<Box<ValueExprAst>>,
        toughness: Option<Box<ValueExprAst>>,
        duration: DurationAst,
    },
    /// CR 702.26 duration-scoped phase-out (`Oubliette`: phases out until `~` leaves the battlefield).
    PhaseOutUntil {
        subject: Box<SubjectRefAst>,
        until: DurationAst,
    },
}

/// Consolidated static ability AST (5 core categories + Conspiracy command-zone agenda).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticAbilityAst {
    AttachmentRestriction {
        filter: Option<Box<ObjectFilterAst>>,
        target_player: Option<String>,
    },
    ContinuousModifyPT {
        subject: Box<SubjectRefAst>,
        dx: String,
        dy: String,
        grant_keywords: Vec<String>,
        grant_ability_text: Option<String>,
        scale: Option<Box<ValueExprAst>>,
        condition: Option<Box<ConditionAst>>,
        paired_rule: Option<Box<StaticAbilityAst>>,
    },
    GrantKeywords {
        subject: Box<SubjectRefAst>,
        keywords: Vec<String>,
        condition: Option<Box<ConditionAst>>,
        paired_rule: Option<Box<StaticAbilityAst>>,
    },
    GrantAbilityText {
        subject: Box<SubjectRefAst>,
        ability_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compiled: Option<Box<FaceAst>>,
        condition: Option<Box<ConditionAst>>,
    },
    CdaPowerToughness {
        power: Option<Box<ValueExprAst>>,
        toughness: Option<Box<ValueExprAst>>,
    },
    SetOrAddType {
        subject: Box<SubjectRefAst>,
        type_or_subtype: String,
        additive: bool,
        condition: Option<Box<ConditionAst>>,
    },
    ControlAttachedHost,
    CostModifier {
        subject: Box<SubjectRefAst>,
        delta: String,
        less: bool,
        #[serde(default)]
        next_only: bool,
        per: Option<Box<ObjectFilterAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale_expr: Option<Box<ValueExprAst>>,
        condition: Option<Box<ConditionAst>>,
    },
    StrivePerExtraTarget {
        delta: String,
    },
    AdditionalCastCost {
        cost: CostAtomAst,
    },
    AlternativeCastCost {
        cost: Vec<CostAtomAst>,
    },
    FlashbackCostModifier {
        cost: String,
        delta: String,
    },
    PlayOrCastPermission {
        cast_only: bool,
        filter: Box<ObjectFilterAst>,
        permission: String,
        condition: Option<Box<ConditionAst>>,
    },
    RuleOrCombatRestriction {
        subject: Option<Box<SubjectRefAst>>,
        rule: String,
        filter: Option<Box<ObjectFilterAst>>,
        condition: Option<Box<ConditionAst>>,
    },
    PreventAllDamageStatic {
        combat_only: bool,
        #[serde(default)]
        noncombat_only: bool,
        #[serde(default)]
        both_ways: bool,
        subject: Box<SubjectRefAst>,
        by: Option<Box<SubjectRefAst>>,
    },
    /// Conspiracy command-zone active rule (`Hidden agenda` / `Double agenda` / face-up).
    ConspiracyAgenda {
        hidden: bool,
        named_slots: u8,
    },
    /// Multi-section card layout metadata (Leveler bands, Sticker options, Deck construction).
    LevelerBand {
        min_level: u8,
        max_level: Option<u8>,
    },
    LevelerStats {
        power: String,
        toughness: String,
    },
    StickerOption {
        ticket_cost: u8,
        payload: String,
    },
    StationBand {
        min_charge: u8,
        payload: String,
    },
    DieRollBand {
        roll: String,
        payload: String,
    },
    DeckConstructionRule(String),
    /// Continuous ability inheritance (`Necrotic Ooze`, `Trazyn`, `Mirran Safehouse`, `Marvin`, `Kasmina`).
    InheritAbilities {
        subject: Box<SubjectRefAst>,
        source_filter: Box<ObjectFilterAst>,
        activated_only: bool,
        condition: Option<Box<ConditionAst>>,
    },
    /// Unified Layers 4/5/6/7b continuous characteristic modification (`Ensoul Artifact`, `Darksteel Mutation`, `Thran Lens`, `Sky Tether`).
    ContinuousLayerEffect {
        subject: Box<SubjectRefAst>,
        set_types: Option<String>,
        additive_types: bool,
        base_pt: Option<(String, String)>,
        set_colors: Option<String>,
        grant_keywords: Vec<String>,
        lose_keywords: Vec<String>,
        lose_all_abilities: bool,
        condition: Option<Box<ConditionAst>>,
    },
    /// Trigger-doubling static (`Panharmonicon`, `Elesh Norn`, `Yarok`, `Isshin`, `Teysa Karlov`).
    TriggerMultiplier {
        event_desc: String,
        multiplier: u8,
        condition: Option<Box<ConditionAst>>,
    },
    /// Keyword grant paired with derived cost rule (`Dream Devourer`, `The Cave of Skulls`, `Varolz`).
    GrantKeywordWithCost {
        subject: Box<SubjectRefAst>,
        keyword: String,
        cost_rule: String,
        condition: Option<Box<ConditionAst>>,
    },
    /// CR 613.4b dynamic base P/T static (`Opalescence`, `Starfield of Nyx`, `March of the Machines`).
    DynamicBasePT {
        subject: Box<SubjectRefAst>,
        power: Box<ValueExprAst>,
        toughness: Box<ValueExprAst>,
        condition: Option<Box<ConditionAst>>,
    },
    /// Composed conjunction of two or more static abilities (`Tamiyo's Compleation`, `Hold for Questioning`).
    Compound(Vec<StaticAbilityAst>),
    /// Once-per-turn frequency wrapper on a static permission or rule (`Once during each of your turns, ...`).
    OncePerTurn(Box<StaticAbilityAst>),
    /// Negated branch following a conditional static (`Otherwise, it gets -2/-1.`).
    Otherwise(Box<StaticAbilityAst>),
    /// Follow-up imperative instructions in a multi-sentence static/replacement paragraph.
    FollowUp(Vec<InstructionAst>),
    /// Opt-in tabletop honor-system continuous rule (e.g. attire / social condition).
    ManualTabletopStatic {
        prompt: String,
    },
}

/// Consolidated replacement effect AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacementEffectAst {
    EntersTapped {
        unless: Option<Box<ConditionAst>>,
    },
    EntersTappedUnlessPayLife {
        life: u16,
    },
    EntersTappedUnlessReveal {
        filter: Box<ObjectFilterAst>,
    },
    EntersPrepared,
    EntersAsCopy {
        optional: bool,
        filter: Box<ObjectFilterAst>,
        exception: Option<String>,
    },
    OthersEnterTapped {
        subject: Box<SubjectRefAst>,
    },
    OthersEnterWithCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        count: Box<ValueExprAst>,
        additional: bool,
    },
    EntersWithCounters {
        counter_kind: String,
        count: Box<ValueExprAst>,
        per: Option<Box<ObjectFilterAst>>,
        x_expr: Option<Box<ValueExprAst>>,
        condition: Option<Box<ConditionAst>>,
    },
    EntersTappedWithCounters {
        counter_kind: String,
        count: Box<ValueExprAst>,
    },
    EscapesWithCounters {
        counter_kind: String,
        count: Box<ValueExprAst>,
    },
    AsEntersChoose {
        choice: String,
    },
    AsEntersAction(Vec<InstructionAst>),
    EventInstead {
        subject_or_event: String,
        would: String,
        instead: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instead_body: Option<Vec<InstructionAst>>,
    },
}

/// Parsed CR 702 keyword with optional desugared IR representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeywordAst {
    pub name: String,
    pub parameter: Option<String>,
    pub desugared_rule: Option<String>,
}

/// Compiled ability container on a card face.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbilityAst {
    Activated {
        ability_word: Option<String>,
        zone: Option<String>,
        cost: Vec<CostAtomAst>,
        body: Vec<InstructionAst>,
    },
    Triggered {
        ability_word: Option<String>,
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
    },
    Loyalty {
        delta: String,
        body: Vec<InstructionAst>,
    },
    SagaChapter {
        chapters: Vec<String>,
        body: Vec<InstructionAst>,
    },
    ClassLevel {
        cost: Vec<CostAtomAst>,
        level: u8,
    },
}

/// Strict parse failure diagnostic recorded when a clause cannot be lowered with zero leftover tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseFailureAst {
    pub stage: String,
    pub text: String,
    pub reason: String,
}

/// Structured Leveler band (`LEVEL N-M` or `LEVEL N+`) preserving band-scoped stats and abilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelerBandAst {
    pub min_level: u8,
    pub max_level: Option<u8>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub keywords: Vec<KeywordAst>,
    pub statics: Vec<StaticAbilityAst>,
    pub abilities: Vec<AbilityAst>,
    pub replacements: Vec<ReplacementEffectAst>,
}

/// Structured Class level section (`{cost}: Level N`) preserving level-scoped abilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassLevelSectionAst {
    pub level: u8,
    pub cost: Vec<CostAtomAst>,
    pub keywords: Vec<KeywordAst>,
    pub statics: Vec<StaticAbilityAst>,
    pub abilities: Vec<AbilityAst>,
    pub replacements: Vec<ReplacementEffectAst>,
}

/// Structured Case `Solved — ...` section preserving solved-only abilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseSolvedSectionAst {
    pub keywords: Vec<KeywordAst>,
    pub statics: Vec<StaticAbilityAst>,
    pub abilities: Vec<AbilityAst>,
    pub replacements: Vec<ReplacementEffectAst>,
}

impl CaseSolvedSectionAst {
    /// True when this face has no `Solved — ...` abilities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
            && self.statics.is_empty()
            && self.abilities.is_empty()
            && self.replacements.is_empty()
    }
}

/// Compiled representation of one card face.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceAst {
    pub name: String,
    pub type_line: String,
    pub keywords: Vec<KeywordAst>,
    pub abilities: Vec<AbilityAst>,
    pub spell_effects: Vec<InstructionAst>,
    pub statics: Vec<StaticAbilityAst>,
    pub replacements: Vec<ReplacementEffectAst>,
    #[serde(default)]
    pub level_bands: Vec<LevelerBandAst>,
    #[serde(default)]
    pub class_levels: Vec<ClassLevelSectionAst>,
    #[serde(default)]
    pub case_solved: CaseSolvedSectionAst,
    pub failures: Vec<ParseFailureAst>,
}

/// Compiled representation of an entire Magic card across all faces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAst {
    pub oracle_id: String,
    pub name: String,
    pub layout: String,
    pub faces: Vec<FaceAst>,
    pub strictly_compiled: bool,
    pub uses_tabletop_honor_system: bool,
}
