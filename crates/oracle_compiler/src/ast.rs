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
    /// Matches cards with distinct names within a search/selection.
    pub const DIFFERENT_NAMES: Self = Self(1 << 17);
    /// Matches cards with distinct mana values within a search/selection.
    pub const DIFFERENT_MANA_VALUES: Self = Self(1 << 18);
    /// Matches cards with distinct colors within a search/selection.
    pub const DIFFERENT_COLORS: Self = Self(1 << 19);
    /// Matches cards with distinct card types within a search/selection.
    pub const DIFFERENT_CARD_TYPES: Self = Self(1 << 20);
    /// Preserves past-tense last-known-information controller/owner phrasing.
    pub const LKI_PAST_TENSE: Self = Self(1 << 21);
    /// Matches face-up permanents or exiled cards (`Ashiok, Nightmare Muse`, `Expose the Culprit`).
    pub const FACE_UP: Self = Self(1 << 22);
    /// Matches creatures that attacked or blocked this turn (`Gideon's Triumph`).
    pub const ATTACKED_OR_BLOCKED_THIS_TURN: Self = Self(1 << 23);
    /// Matches a commander (`Campfire`, `Opal Palace`, `Stinging Study`).
    pub const IS_COMMANDER: Self = Self(1 << 24);
    /// Matches a non-commander object (`The Peregrine Dynamo`).
    pub const NOT_COMMANDER: Self = Self(1 << 25);

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
    ActivePlayer,
    ChosenPlayer,
    ThatPlayerOrPlaneswalkerController,
    NextPlayerInDirection,
    PlayerToLeft,
    PlayerToRight,
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
    TargetOpponentGraveyard,
    DefendingPlayerGraveyard,
    SingleGraveyard,
    AnyGraveyard,
    AllGraveyards,
    ThatPlayerGraveyard,
    TargetPlayerGraveyard,
    YourHand,
    ThatPlayerHand,
    TargetPlayerHand,
    TargetOpponentHand,
    OpponentHand,
    OpponentsHands,
    EachPlayerHand,
    AnyHand,
    YourLibrary,
    TargetPlayerLibrary,
    TargetOpponentLibrary,
    ThatPlayerLibrary,
    OpponentLibrary,
    EachPlayerLibrary,
    AnyLibrary,
    CommandZone,
    OutsideTheGame,
    YourGraveyardOrExile,
    AnyGraveyardOrExile,
    YourHandOrGraveyard,
    YourLibraryOrGraveyard,
    YourGraveyardHandOrLibrary,
    ItsOwnerGraveyard,
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
    SameAs(Box<SubjectRefAst>),
}

/// Typed reference to objects affected by an earlier action in the same resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriorActionRefKind {
    SacrificedThisWay,
    DestroyedThisWay,
    ExiledThisWay,
    DiscardedThisWay,
    RevealedThisWay,
    PutIntoGraveyardThisWay,
    ReturnedToHandThisWay,
    TappedThisWay,
    ReceivedCounterThisWay(String),
    ChosenThisWay { negated: bool },
}

/// Structured relational, superlative, or characteristic predicate on an `ObjectFilterAst`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterPredicateAst {
    SuperlativeStat {
        stat: StatKind,
        greatest: bool,
    },
    SuperlativeAmong {
        stat: StatKind,
        greatest: bool,
        scope: Box<ObjectFilterAst>,
    },
    OfEachCategory(String),
    ManaValueParity {
        even: bool,
    },
    HasManaSymbolInCost(String),
    HasAbilityKind {
        kind: String,
        negated: bool,
    },
    CouldProduce(String),
    CouldAttachTo(Box<SubjectRefAst>),
    SharesPropertyWith {
        property: String,
        reference: Box<SubjectRefAst>,
        negated: bool,
    },
    DifferentNameFrom(Box<SubjectRefAst>),
    ManaValueIn(Vec<i32>),
    StatRelationTo {
        stat: StatKind,
        cmp: CompareOp,
        expr: Box<ValueExprAst>,
    },
    SameTotalPowerAndToughness,
    HasBasicLandType,
    AttachedBy(Box<ObjectFilterAst>),
    AttachedTo(Box<SubjectRefAst>),
    AnyOf(Vec<ObjectFilterAst>),
    AffectedByPriorAction(PriorActionRefKind),
    AttackingTarget {
        target: Box<SubjectRefAst>,
        this_turn: bool,
    },
    Blocking(Box<SubjectRefAst>),
    Exclude(Box<SubjectRefAst>),
    MatchesChosen {
        negated: bool,
    },
    TargetsSubject(Box<SubjectRefAst>),
    NotOwnedByController,
    FromSource(Box<ObjectFilterAst>),
    NthSpellCastThisTurn(u8),
    /// Matches the Nth spell a player casts in the entire game (`Chancellor of the Annex`).
    NthSpellCastThisGame(u8),
    /// Matches an object that a referenced spell or ability could legally target (`Exterminator Magmarch`).
    CouldBeTargetedBy(Box<SubjectRefAst>),
    /// Matches an object that is not currently targeted by an object matching the filter (`Goblin Artisans`).
    NotTargetedBy(Box<ObjectFilterAst>),
    /// Matches an object whose color count satisfies the comparison (`Evolving Door`).
    ColorCountCompare {
        op: CompareOp,
        expr: Box<ValueExprAst>,
    },
    /// Matches creatures blocked by the referenced creature (`Glyph of Doom`, `Glyph of Delusion`, `Glyph of Reincarnation`).
    BlockedBy {
        blocker: Box<SubjectRefAst>,
        #[serde(default)]
        this_turn: bool,
    },
    /// Matches creatures continuously controlled (or not continuously controlled) since turn start (`Total War`).
    ControlledSinceTurnStart {
        #[serde(default)]
        negated: bool,
    },
    /// Matches the most recent object in turn history satisfying the surrounding filter (`Suffocation`).
    LastMatchingThisTurn,
    /// Matches permanents on which the source put `counter_kind` and has not removed one (`Cyclopean Tomb`).
    CounterPlacedNotRemovedBySelf {
        counter_kind: String,
    },
    /// Matches cards with the same name as a card spliced onto the referenced spell (`Minamo's Meddling`).
    SameNameAsSplicedOnto(Box<SubjectRefAst>),
    /// Matches permanents of the Nth type from a prior ordered type choice (`Rite of Ruin`).
    NthChosenType(u8),
    /// Matches permanents that do not have a specific kind of counter (`Coiling Stalker`).
    WithoutCounterKind(String),
    /// Matches permanents that did not enter the battlefield this turn (`The Fifth Doctor`).
    DidNotEnterThisTurn,
    /// Matches creatures that blocked or were blocked by objects matching `other` (`You Cannot Pass!`).
    BlockedOrBlockedBy {
        other: Box<ObjectFilterAst>,
        #[serde(default)]
        this_turn: bool,
    },
    /// Matches permanents whose controller was dealt combat damage by the source this turn (`Steel Hellkite`).
    ControllerDealtCombatDamageBySelfThisTurn,
    /// Matches cards put into their current zone from anywhere this turn (`Reenact the Crime`).
    PutIntoZoneThisTurn,
    /// Matches permanents with a sticker of the specified kind (`Proficient Pyrodancer`).
    HasStickerKind(String),
    /// Matches permanents whose loyalty or activated ability was activated this turn (`Cut Short`).
    ActivatedThisTurn,
    /// Matches a set of targets that must have pairwise different controllers (`Cloud's Limit Break`).
    DifferentControllers,
    /// Matches a permanent controlled by the same player as the referenced permanent (`Glamer Spinners`).
    WithSameControllerAs(Box<SubjectRefAst>),
    /// Matches permanents that have (or lack) abilities (`Jasmine Boreal of the Seven`, `Muraganda Petroglyphs`).
    HasAbilities {
        has_abilities: bool,
    },
    /// Matches permanents controlled by the player who is currently the monarch (`Azure Fleet Admiral`).
    ControlledByMonarch,
    StateQualification(String),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ControllerRelation>,
    pub zone: Option<ZoneFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_zones: Vec<ZoneFilter>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicates: Vec<FilterPredicateAst>,
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
            && self.owner.is_none()
            && self.zone.is_none()
            && self.extra_zones.is_empty()
            && self.combat.is_none()
            && self.tapped.is_none()
            && self.token.is_none()
            && self.required_keywords.is_empty()
            && self.forbidden_keywords.is_empty()
            && self.stat_bounds.is_empty()
            && self.counter_kind.is_none()
            && self.named.is_none()
            && self.flags.is_empty()
            && self.predicates.is_empty()
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
    ThatPlayerOrPlaneswalkerController,
    EachPlayerOnYourTeam,
    ChosenPlayer,
    TargetOpponentWhoseTurnItIs,
    TargetOpponentAtRandom,
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

/// Pile reference for pile-separation effects (Fact or Fiction, Make an Example, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PileRefAst {
    ChosenPile,
    OtherPile,
    ThatPile,
    EachPile,
    FilteredInPile {
        filter: Box<ObjectFilterAst>,
        pile: Box<PileRefAst>,
    },
}

/// Subject or target reference inside an effect, trigger, or static clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectRefAst {
    SelfObject,
    Anaphora(String),
    AttachedHost(String),
    /// Explicit zone-qualified self or anaphora subject (e.g. `~ from your graveyard`, `it from exile`).
    InZone {
        subject: Box<SubjectRefAst>,
        zone: ZoneFilter,
    },
    /// Possessive controller of an arbitrary subject (e.g. `target spell's controller`).
    ControllerOf(Box<SubjectRefAst>),
    /// Possessive owner of an arbitrary subject (e.g. `target creature's owner`).
    OwnerOf(Box<SubjectRefAst>),
    /// Craft LKI group (`the exiled cards used to craft ~`).
    CraftExiledCards,
    /// Pile reference subject (`that pile`, `the creatures in their chosen pile`).
    PileRef(PileRefAst),
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
    /// Shared target pool across disjunctive target specifications (e.g. `up to three target creatures
    /// from the battlefield and/or creature cards from graveyards`, `target opponent or battle`).
    TargetOneOf {
        options: Vec<SubjectRefAst>,
        count: TargetCountAst,
    },
    /// Distinct ordinal target slot (`another target creature`, `a third target creature`) preserving
    /// pairwise distinctness across multi-target instructions (`Serpentine Spike`).
    DistinctTarget {
        ordinal: u8,
        filter: Box<ObjectFilterAst>,
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
    /// Exponentiation (e.g. `2^x` in Mathemagics).
    Pow {
        base: Box<ValueExprAst>,
        exp: Box<ValueExprAst>,
    },
    /// Number of colors of mana spent to cast a referenced spell (`that spell`).
    ColorsOfManaSpentOn(Box<SubjectRefAst>),
    /// Count of a specific color's mana symbols in the mana costs of a subject group.
    ColorPipsIn {
        color: String,
        subject: Box<SubjectRefAst>,
    },
    /// Saturating shortfall below a threshold (`for each card less than two a player draws this way`).
    Shortfall {
        threshold: Box<ValueExprAst>,
        actual: Box<ValueExprAst>,
    },
    /// Extreme stat value among filtered objects (e.g. `greatest mana value of a commander you own`).
    ExtremeStatAmong {
        stat: StatKind,
        greatest: bool,
        filter: Box<ObjectFilterAst>,
        #[serde(default)]
        or_toughness: bool,
    },
    /// Distinct count of a characteristic category among filtered objects (`different powers`, `permanent types`, `colors`, etc.).
    DistinctCountAmong {
        category: String,
        filter: Box<ObjectFilterAst>,
    },
    /// Greatest number of objects in a filtered set sharing a single subtype (`Skemfar Shadowsage`, `Basalt Ravager`).
    MaxSharedSubtypeCount {
        filter: Box<ObjectFilterAst>,
    },
    /// Count of players matching a predicate (`players who control more lands than you`, `players being attacked`).
    CountPlayersMatching {
        predicate: String,
    },
    /// Turn-scoped or game-scoped event counter (`permanents you've sacrificed this turn`, `times cast from command zone`).
    EventCountThisTurn {
        event: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<PlayerRefAst>,
    },
    /// Total counters of a given kind across all objects matching a filter (`time counters among permanents you control`).
    CounterCountAmong {
        counter_kind: String,
        filter: Box<ObjectFilterAst>,
    },
    /// Number of unique vowels on the referenced name sticker (`Unfinity` eternal-legal cards).
    StickerUniqueVowels,
    /// Absolute difference between two numeric expressions (`the difference between its power and toughness`).
    AbsDiff(Box<ValueExprAst>, Box<ValueExprAst>),
    /// Maximum of two numeric expressions (`2 or the greatest power among Dinosaurs you control, whichever is greater`).
    Max(Box<ValueExprAst>, Box<ValueExprAst>),
    /// Minimum of two numeric expressions (`X or enchanted creature's toughness minus 1, whichever is smaller`).
    Min(Box<ValueExprAst>, Box<ValueExprAst>),
    /// Mana value of a commander owned by `owner` (battlefield or command zone); `greatest: false` when player chooses (`Stinging Study`).
    CommanderManaValue {
        owner: PlayerRefAst,
        greatest: bool,
    },
    /// Number of listed keywords possessed by a subject (`Akroma, Vision of Ixidor`).
    CountKeywordsIn {
        subject: Box<SubjectRefAst>,
        keywords: Vec<String>,
    },
    /// Mana spent to activate an ability (`Mr. House, President and CEO`).
    ManaSpentToActivate {
        mana_filter: Option<String>,
    },
    /// Number of times a commander has been cast from the command zone this game (`Opal Palace`).
    TimesCastFromCommandZone(Box<SubjectRefAst>),
    /// Number of cards drawn by a player during the current resolution (`Malignant Growth`).
    CountDrawnThisWay(PlayerRefAst),
    /// Amount of energy (`{E}`) paid during the current resolution (`Wheel of Potential`).
    EnergyPaidThisWay,
    /// Conditional numeric expression choosing between two branches (`Snowblind`).
    Conditional {
        condition: Box<ConditionAst>,
        then_expr: Box<ValueExprAst>,
        else_expr: Box<ValueExprAst>,
    },
    /// Pre-damage cap of the damaged target: player life total before damage, planeswalker loyalty before damage, or creature toughness (`Drain Life`, `Soul Burn`).
    TargetPreDamageCap,
    /// Total damage already dealt to the referenced permanent or player this turn (`Final Punishment`, `Whipkeeper`).
    DamageDealtToSubjectThisTurn(Box<SubjectRefAst>),
    /// Amount of damage prevented during the current resolution (`Bone Mask`).
    DamagePreventedThisWay,
    /// Number of counters removed during the current resolution (`Eventide's Shadow`).
    CountersRemovedThisWay,
    /// Total toxic value of a referenced creature (`Goliath Hatchery`).
    TotalToxicValue(Box<SubjectRefAst>),
    /// Total amount of unspent mana currently in a player's mana pool (`Glissa Sunseeker`).
    UnspentManaAmount(PlayerRefAst),
    /// Count of a specific letter in name stickers on the referenced permanent (`Make a _____ Splash`).
    StickerLetterCount {
        subject: Box<SubjectRefAst>,
        letter: char,
    },
    /// Number of basic land types among lands a specific player controls (`Allied Strategies`, `Collapsing Borders`).
    DomainCountFor(PlayerRefAst),
    /// Number of cards exiled during the current resolution (`Trench Gorger`).
    CardsExiledThisWay,
    /// Amount of life paid by a player during the current resolution (`Plague of Vermin`).
    LifePaidThisWay(PlayerRefAst),
    /// Number of creature types noted for a referenced permanent (`Volo, Itinerant Scholar`).
    NotedTypesCount(Box<SubjectRefAst>),
    /// Number of times an additional cost was paid (`Taste of Paradise`).
    AdditionalCostPaidCount(String),
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
    UntilEndOfPlayersNextTurn(PlayerRefAst),
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
    DealtDamageThisWay {
        combat_only: bool,
    },
    DealtDamageBySelfThisTurn {
        combat_only: bool,
    },
    SubjectHarnessed(Box<SubjectRefAst>),
    AdditionalCostPaid {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    PlayerDealtDamageBy {
        player: PlayerRefAst,
        source_filter: Box<ObjectFilterAst>,
        min_sources: u16,
        combat_only: bool,
        this_game: bool,
    },
    PlayerHadCardsPutIntoGraveyardThisTurn {
        player: PlayerRefAst,
        min_count: u16,
    },
    IsNthSpellMatchingThisTurn {
        nth: u8,
        filters: Vec<ObjectFilterAst>,
    },
    TopCardOfZoneMatches {
        zone: ZoneFilter,
        filter: Box<ObjectFilterAst>,
    },
    PlayerActionExtremeStat {
        player: PlayerRefAst,
        action: PriorActionRefKind,
        stat: StatKind,
        greatest: bool,
        allow_tie: bool,
    },
    CycledCardNamedTimesThisGame {
        name: NameFilterAst,
        min_times: u16,
    },
    RevealedDistinctNames {
        min_count: u16,
    },
    EnergyPaidThisWayAtLeast(u16),
    ZoneActionCountThisWay {
        action: PriorActionRefKind,
        min_count: u16,
    },
    DistinctCardTypesAmong {
        filter: Box<ObjectFilterAst>,
        min_count: u16,
    },
    SameManaValueAsTriggerSpell,
    /// All targets of the resolving spell or ability are still legal (`Goblin Welder`).
    AllTargetsLegal,
    /// Subject shares a color with the most common color (or tied for most common) among `scope` (`Barrin's Unmaking`).
    SharesMostCommonColorAmong {
        subject: Box<SubjectRefAst>,
        scope: Box<ObjectFilterAst>,
        allow_tie: bool,
    },
    /// The spell is the first spell its controller has cast this game (`Once Upon a Time`).
    FirstSpellCastThisGame,
    /// You have cast another spell with the same name as the source this game (`Approach of the Second Sun`).
    CastAnotherNamedThisGame,
    /// Referenced object entered from or was cast from `zone` (`Extraordinary Journey`, `Fblthp, the Lost`).
    EnteredOrCastFromZone(ZoneFilter),
    /// Referenced player's speed is strictly greater than each other player's speed (`Spikeshell Harrier`).
    SpeedGreaterThanEachOther(PlayerRefAst),
    /// Player has the greatest or least value of a player statistic (`Sokenzan Renegade`).
    PlayerHasExtremePlayerStat {
        player: PlayerRefAst,
        stat: PlayerStatKind,
        greatest: bool,
    },
    /// Referenced player revealed the exact chosen number of cards of the chosen color (`Scrying Glass`).
    RevealedExactCountAndColor {
        player: PlayerRefAst,
    },
    /// At least `min_count` cards milled this way share a card type (`The Tale of Tamiyo`).
    MilledCardsShareCardType {
        min_count: u8,
    },
    /// One or more permanents were sacrificed to activate the referenced ability (`Ashnod the Uncaring`).
    PermanentSacrificedToActivate,
    /// Exact number of colors of mana spent to activate the current ability (`Paragon of Modernity`).
    ColorsSpentToActivateExact(u8),
    /// A matching permanent under your control was destroyed this turn by an opponent's spell or ability (`Cobra Trap`).
    PermanentsDestroyedByOpponentThisTurn {
        filter: Box<ObjectFilterAst>,
    },
    /// The referenced creature exploited the referenced creature (`Silumgar Scavenger`).
    ExploitedCreature,
    /// The referenced attachment was attached to a matching permanent (`Unforge`).
    WasAttachedTo {
        subject: Box<SubjectRefAst>,
        host_filter: Box<ObjectFilterAst>,
    },
    /// The referenced creature regenerated via the current shield (`Debt of Loyalty`).
    SourceRegeneratedThisWay,
    /// The Ring has tempted you at least `min_times` times this game (`Frodo, Adventurous Hobbit`).
    RingTemptedMinTimesThisGame(u8),
    /// The referenced creature is your Ring-bearer (`Frodo, Adventurous Hobbit`).
    SubjectIsRingBearer(Box<SubjectRefAst>),
    /// Hidden chosen values matched (`Expert-Level Safe`).
    HiddenValuesMatched,
    /// Either of two targets has a color the other does not (`Dead Ringers`).
    EitherHasColorOtherDoesnt,
    /// `attacker` is currently attacking `defender` (`Curse of Bounty`).
    PlayerAttackingPlayer {
        attacker: PlayerRefAst,
        defender: PlayerRefAst,
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
    /// Coin-flip outcome condition (`if you win all the flips`, `if you won N or more flips`).
    CoinFlipOutcome {
        outcome: String,
    },
    /// Player tie comparison (`if two or more players are tied for fewest`).
    PlayersTiedFor {
        metric: String,
        min_tied: u8,
    },
    /// Turn/game history check (`if ~ dealt damage to another creature this turn`, `if you cycled two or more cards this turn`).
    EventOccurredThisTurn {
        event: String,
    },
    /// General numeric expression comparison (`if there are four or more basic land types among lands that player controls`).
    ExprCompare {
        lhs: Box<ValueExprAst>,
        op: CompareOp,
        rhs: Box<ValueExprAst>,
    },
    /// Unspent mana threshold check (`as long as you have six or more unspent mana`).
    UnspentManaAtLeast {
        player: PlayerRefAst,
        amount: i32,
    },
    /// Prime-number cardinality check (`if you control a prime number of lands`).
    PrimeNumberCheck {
        expr: Box<ValueExprAst>,
    },
    /// Quantified check over a referenced object/player group (`if any of those creatures have ...`).
    AnyOfGroupMatches {
        group: Box<SubjectRefAst>,
        predicate: String,
    },
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
    /// Filtered mana-production query (`one mana of any color that a Gate you control could produce`).
    AnyObjectCouldProduce {
        filter: Box<ObjectFilterAst>,
        any_type: bool,
    },
    /// Mana color constrained to colors among a filtered group (`Mox Amber`).
    AnyColorAmong {
        source: Box<SubjectRefAst>,
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
    /// Pay generic mana equal to a dynamic expression (`Repulsive Mutation`: `mana equal to the greatest power ...`).
    PayGenericManaExpr(Box<ValueExprAst>),
    Behold(Box<ObjectFilterAst>),
    TapSelf,
    UntapSelf,
    PayLife(Box<ValueExprAst>),
    PayHalfLife,
    SacrificeSelf,
    Sacrifice {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
        #[serde(default)]
        up_to: bool,
        #[serde(default)]
        min_one: bool,
    },
    SacrificeAll(Box<ObjectFilterAst>),
    DiscardSelf,
    DiscardHand,
    Discard {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
        random: bool,
        #[serde(default)]
        up_to: bool,
        #[serde(default)]
        any_number: bool,
    },
    ExileSelf {
        from_graveyard: bool,
        from_hand: bool,
    },
    Exile {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
        #[serde(default)]
        face_down: bool,
        #[serde(default)]
        face_up: bool,
        #[serde(default)]
        single_graveyard: bool,
        #[serde(default)]
        any_number: bool,
        #[serde(default)]
        min_one: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_mv: Option<(CompareOp, Box<ValueExprAst>)>,
    },
    ExileTopLibrary {
        count: Box<ValueExprAst>,
        face_down: bool,
    },
    /// Exile top card(s) of a non-library zone (e.g. `Exile the top [creature] card of your graveyard`).
    ExileTopZone {
        zone: ZoneFilter,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Box<ObjectFilterAst>>,
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
    /// Remove counters distributed from among matching permanents (e.g. `Hopeful Initiate`, `Ooze Flux`).
    RemoveCountersFromAmong {
        all: bool,
        min_one: bool,
        count: Option<Box<ValueExprAst>>,
        counter_kind: String,
        filter: Box<ObjectFilterAst>,
    },
    PutCounters {
        count: Box<ValueExprAst>,
        counter_kind: String,
        subject: Box<SubjectRefAst>,
    },
    TapPermanent {
        count: Box<ValueExprAst>,
        filter: Box<ObjectFilterAst>,
        #[serde(default)]
        any_number: bool,
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
        #[serde(default)]
        excess_only: bool,
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
    /// Filtered or multi-creature blocking / becoming-blocked trigger (`Serra Inquisitors`, `Lairwatch Giant`, `Godsend`).
    BlocksOrBecomesBlockedBy {
        subject: Box<SubjectRefAst>,
        other_filter: Box<ObjectFilterAst>,
        blocks: bool,
        becomes_blocked: bool,
        min_count: u8,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted: Option<Box<FaceAst>>,
}

/// Modifier on a library search (`instead of one`, `an additional`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchModifierAst {
    InsteadOfOne,
    Additional,
}

/// Optional count limit for cast/play permissions (`up to N of`, `any number of`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CountLimitAst {
    UpTo(Box<ValueExprAst>),
    AnyNumber,
}

/// Alternative casting cost specified on a cast permission (`pay life equal to its mana value`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AltCastCostAst {
    PayLifeEqualToManaValue,
    PayCost(Vec<CostAtomAst>),
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

/// Closed category domain for `InstructionAst::ForEachCategory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CategoryIterationKind {
    /// Each of the ten two-color pairs (`Niv-Mizzet Reborn`).
    ColorPair,
    /// Each distinct kind of counter among matching objects (`Exotic Pets`).
    CounterKindAmong(Box<ObjectFilterAst>),
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
        #[serde(default)]
        up_to: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        divider: Option<PlayerRefAst>,
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
        #[serde(default)]
        random: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        except: Option<Box<SubjectRefAst>>,
    },
    ModifyPreviousDestroyNoRegen,
    Exile {
        actor: Option<PlayerRefAst>,
        subject: Box<SubjectRefAst>,
        #[serde(default)]
        random: bool,
        #[serde(default)]
        face_down: bool,
        #[serde(default)]
        into_pile: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        except: Option<Box<SubjectRefAst>>,
        #[serde(default)]
        half: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rounding: Option<Rounding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chooser: Option<PlayerRefAst>,
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
        #[serde(default)]
        random: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        except: Option<Box<SubjectRefAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
        #[serde(default)]
        up_to: bool,
        #[serde(default)]
        any_number: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<PlayerRefAst>,
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
        #[serde(default)]
        exact: bool,
        filter: Box<ObjectFilterAst>,
        reveal: bool,
        to_zone: String,
        shuffle: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extra_zones: Vec<ZoneFilter>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secondary_filter: Option<Box<ObjectFilterAst>>,
        #[serde(default)]
        secondary_is_or: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        where_x: Option<Box<ValueExprAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rest_to_zone: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modifier: Option<SearchModifierAst>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        follow_up: Vec<InstructionAst>,
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
        #[serde(default)]
        any_number: bool,
    },
    UntapPermanent {
        subject: Box<SubjectRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
        #[serde(default)]
        up_to: bool,
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
    /// CR 702.26 phase phased-out permanent(s) back in (`Time and Tide`, `The Pandorica`).
    PhaseIn {
        subject: Box<SubjectRefAst>,
    },
    Fight {
        a: Box<SubjectRefAst>,
        b: Box<SubjectRefAst>,
    },
    Attach {
        subject: Box<SubjectRefAst>,
        target: Box<SubjectRefAst>,
        #[serde(default)]
        any_number: bool,
        #[serde(default)]
        all: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count_limit: Option<CountLimitAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt_cost: Option<AltCastCostAst>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<Box<SubjectRefAst>>,
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
        #[serde(default)]
        face_down: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_target: Option<Box<SubjectRefAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        must_target: Option<Box<SubjectRefAst>>,
        #[serde(default)]
        single_target_only: bool,
    },
    SeparateIntoPiles {
        chooser: PlayerRefAst,
        pile_count: u8,
        #[serde(default)]
        face_down_piles: u8,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Box<ValueExprAst>>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_choice: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration: Option<DurationAst>,
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
    /// Non-You or filtered-player emblem creation (e.g. `Chandra, Roaring Flame`).
    CreateEmblemFor {
        recipient: Box<SubjectRefAst>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compiled: Option<Box<FaceAst>>,
    },
    /// Generalized action applied to a subject-referenced or filtered player set.
    ForEachSubjectPlayer {
        players: Box<SubjectRefAst>,
        body: Vec<InstructionAst>,
    },
    /// Keyword action: Harness (Infinity Stones / Marvel).
    Harness {
        subject: Box<SubjectRefAst>,
    },
    /// Structured selection from a group (e.g. `choose one of them at random`).
    ChooseFromGroup {
        count: Box<ValueExprAst>,
        random: bool,
        group: Box<SubjectRefAst>,
    },
    /// Temporary special-action timing window (`any time you could activate a mana ability`).
    TemporarySpecialAction {
        timing: String,
        duration: DurationAst,
        body: Vec<InstructionAst>,
    },
    /// Iterate over each color among a subject group (`Sunbird Standard`).
    ForEachColorAmong {
        source: Box<SubjectRefAst>,
        body: Vec<InstructionAst>,
    },
    /// Modal choice modifier or constraint (`choose both instead`, `each mode must target a different player`).
    ModalModifier(String),
    /// Repeat-process loop with explicit dynamic count, keyword list, or until-condition (`Another Round`, `Kathril`, `Timesifter`).
    RepeatProcessExt {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        times: Option<Box<ValueExprAst>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        for_list: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until: Option<String>,
    },
    /// Temporary mana-spending rule (`you may spend colorless mana as though it were mana of any color`).
    TemporaryManaSpendRule {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mana_kind: Option<String>,
        as_though: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        duration: DurationAst,
        #[serde(default)]
        next_only: bool,
    },
    /// Duration-scoped continuous body (`For as long as <cond>, <effect>`).
    ForAsLongAs {
        condition: Box<ConditionAst>,
        body: Vec<InstructionAst>,
    },
    /// Next-action delayed trigger (`When you next cast <spell> this turn, <effect>`).
    WhenYouNext {
        event: String,
        body: Vec<InstructionAst>,
        duration: DurationAst,
    },
    /// Dual variable binding (`where X is <e1> and Y is <e2>`).
    BindXY {
        x_expr: Box<ValueExprAst>,
        y_expr: Box<ValueExprAst>,
        body: Vec<InstructionAst>,
    },
    /// Choose a counter (or kind of counter) on a subject (`Animation Module`, `Clockspinning`, `Contractual Safeguard`).
    ChooseCounter {
        subject: Box<SubjectRefAst>,
        #[serde(default)]
        kind_only: bool,
    },
    /// Move counter(s) from one object to another (`Leech Bonder`).
    MoveCounter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counter_kind: Option<String>,
        from: Box<SubjectRefAst>,
        to: Box<SubjectRefAst>,
    },
    /// Suspect or unsuspect a creature (`Frantic Scapegoat`, `Airtight Alibi`, `Absolving Lammasu`).
    SetSuspected {
        subject: Box<SubjectRefAst>,
        #[serde(default)]
        remove: bool,
    },
    /// Auction/bidding action (`Pain's Reward`, `Mages' Contest`, `Illicit Auction`).
    BiddingAction(String),
    /// Voting modifier/action (`Illusion of Choice`, `Mob Verdict`).
    VotingAction(String),
    /// Symmetric "does the same" / "the same way" repetition (`Balance`, `Restore Balance`, `Juxtapose`, `The Wedding of River Song`).
    RepeatSameWay(String),
    /// Temporary entry modifier (`Lands you control enter tapped this turn`, `They enter tapped and attacking`).
    EntersTappedRule {
        subject: Box<SubjectRefAst>,
        tapped: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attacking: Option<String>,
        duration: DurationAst,
    },
    /// Turn a permanent face down (`Master of the Veil`).
    TurnFaceDown {
        subject: Box<SubjectRefAst>,
    },
    /// Action targeting a pile or cards in a pile (`Phyrexian Portal`, `Bend or Break`, `Abstract Performance`).
    PileAction {
        action: String,
        pile: PileRefAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Box<ObjectFilterAst>>,
    },
    /// Characteristic-only or target-only modal option (`Outlaws' Merriment`, `Wild Shape`, `Genku`, `Call Damage Control`).
    TokenCharacteristicsChoice(String),
    /// Generalized reveal/look-at subject action (`Nightshade Assassin`, `Vault 21`, `Guided Passage`, `Mindblaze`).
    RevealCards {
        subject: Box<SubjectRefAst>,
    },
    /// Mana-spend rider applied when the produced mana is spent to cast a matching spell (`Opal Palace`, `Biophagus`, `Jade Orb`).
    ManaSpendRider {
        cast_filter: Box<ObjectFilterAst>,
        rider: Vec<InstructionAst>,
    },
    /// Turn-order or direction-ordered sequential player selection (`Order of Succession`, `The Legend of Yangchen`, `Manifold Insights`, `Rejoin the Fight`, `Fortunate Few`, `Grenzo's Rebuttal`).
    SequentialChoose {
        start_player: PlayerRefAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<String>,
        each_player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        #[serde(default)]
        distinct: bool,
        count: TargetCountAst,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        body: Vec<InstructionAst>,
    },
    /// Choose one object per category filter (`Cataclysm`, `Selective Adaptation`).
    ChoosePerFilter {
        player: PlayerRefAst,
        categories: Vec<ObjectFilterAst>,
        #[serde(default)]
        sacrifice_rest: bool,
    },
    /// Turn face-down object(s) face up (`Pyxis of Pandemonium`).
    TurnFaceUp {
        player: PlayerRefAst,
        subject: Box<SubjectRefAst>,
    },
    /// Choose one keyword from a fixed list to grant (`Ballroom Brawlers`).
    GrantKeywordChoice {
        subject: Box<SubjectRefAst>,
        choices: Vec<String>,
        duration: DurationAst,
    },
    /// Look at one or more hidden objects/zones (`Spy Network`).
    LookAt {
        player: PlayerRefAst,
        targets: Vec<SubjectRefAst>,
    },
    /// Typed reflexive triggered ability (`Explosion of Riches`, `Saruman of Many Colors`, `Unleash the Inferno`).
    ReflexiveEventTrigger {
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
    },
    /// Exile or reveal from top of library until cumulative stat total reaches a threshold (`Dream Harvest`, `Tasha's Hideous Laughter`).
    ExileOrRevealTopUntilTotalStat {
        exile: bool,
        player: PlayerRefAst,
        stat: StatKind,
        threshold: Box<ValueExprAst>,
    },
    /// Execute two or more atomic instructions simultaneously (`Goblin Welder`, `Sands of Time`).
    Simultaneous(Vec<InstructionAst>),
    /// Iterate over a closed category domain (`Niv-Mizzet Reborn`, `Exotic Pets`).
    ForEachCategory {
        category: CategoryIterationKind,
        body: Vec<InstructionAst>,
    },
    /// A player loses all unspent mana (`Pygmy Hippo`).
    LoseUnspentMana {
        player: PlayerRefAst,
    },
    /// One referenced player controls another referenced player during the controlled player's next turn (`Cruel Entertainment`).
    PlayerControlsPlayerTurn {
        controller: Box<SubjectRefAst>,
        controlled: Box<SubjectRefAst>,
    },
    /// CR 724 restart the game, optionally leaving matching exiled cards in exile (`Karn Liberated`).
    RestartGame {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leave_in_exile: Option<Box<ObjectFilterAst>>,
    },
    /// Reorder a zone in any order or at random (`Fossil Find`, `Tahngarth's Glare`).
    ReorderZone {
        player: PlayerRefAst,
        zone: ZoneFilter,
        #[serde(default)]
        random: bool,
    },
    /// Grant a player additional step(s) after the current step (`Paradox Haze`).
    AdditionalStep {
        player: PlayerRefAst,
        step: TurnStepAst,
        count: u8,
    },
    /// Skip all instances of a named phase on a player's next turn (`Empty City Ruse`).
    SkipAllPhasesNextTurn {
        player: PlayerRefAst,
        phase: String,
    },
    /// Force a player to activate a mana ability of each matching permanent (`Pygmy Hippo`, `Drain Power`).
    ActivateManaAbilities {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    /// Modify a player's speed by a signed delta (`Spikeshell Harrier`).
    ModifyPlayerSpeed {
        player: PlayerRefAst,
        delta: i8,
    },
    /// Sacrifice all matching permanents except `keep` of controller's choice (`Keldon Firebombers`, `Choice of Damnations`).
    SacrificeAllExcept {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        keep: Box<ValueExprAst>,
    },
    /// Exile all cards from a player's library except the bottom `keep` cards (`Doomsday Excruciator`).
    ExileAllExceptBottom {
        player: PlayerRefAst,
        keep: Box<ValueExprAst>,
        #[serde(default)]
        face_down: bool,
    },
    /// A player guesses a hidden property of a referenced object (`Gollum, Scheming Guide`).
    GuessCardProperty {
        guesser: PlayerRefAst,
        subject: Box<SubjectRefAst>,
        property: String,
    },
    /// Distribute stickers among targets (`Roxi, Publicist to the Stars`).
    DistributeStickers {
        sticker_kind: String,
        count: Box<ValueExprAst>,
        #[serde(default)]
        up_to: bool,
        targets: Box<SubjectRefAst>,
    },
    /// Ignore the enclosing effect for permanents matching `filter` (`Siren's Call`).
    IgnoreEffectFor {
        filter: Box<ObjectFilterAst>,
    },
    /// Put a referenced card into a player's library at a specific 1-based position from the top (`Long-Term Plans`).
    PutInLibraryAtPosition {
        player: PlayerRefAst,
        subject: Box<SubjectRefAst>,
        from_top: u8,
    },
    /// Explicitly count matching objects or counters into `PriorResult` (`Rumbling Ruin`).
    CountObjects {
        filter: Box<ObjectFilterAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counter_kind: Option<String>,
    },
    /// Players secretly choose among fixed options and reveal simultaneously (`Prisoner's Dilemma`).
    SecretChoiceAndReveal {
        players: PlayerRefAst,
        options: Vec<String>,
    },
    /// Order all matching permanents sequentially (`Starlight Spectacular`).
    OrderGroup {
        player: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
    },
    /// Two players exchange control of all matching permanents they control (`Reins of Power`).
    ExchangeAllControlled {
        left: PlayerRefAst,
        right: PlayerRefAst,
        filter: Box<ObjectFilterAst>,
        duration: DurationAst,
    },
    /// Replace mana produced by a permanent when tapped for mana (`Quarum Trench Gnomes`).
    ReplaceManaProduction {
        subject: Box<SubjectRefAst>,
        replace_with: ManaSpecAst,
        duration: DurationAst,
    },
    /// Reduce the enclosing cost by `delta` for each unit of `per` (`Draco`).
    DynamicCostReduction {
        delta: String,
        per: Box<ValueExprAst>,
    },
    /// Exchange the text boxes of two permanents (`Exchange of Words`, `Deadpool, Trading Card`).
    ExchangeTextBoxes {
        left: Box<SubjectRefAst>,
        right: Box<SubjectRefAst>,
    },
    /// CR 701.37 meld paired cards into their combined back face (`Vanille, Cheerful l'Cie`, `Gisela`, `Urza`).
    Meld {
        subject: Box<SubjectRefAst>,
        into_name: String,
    },
    /// Double each type of unspent mana in a player's mana pool (`Doubling Cube`).
    DoubleUnspentMana {
        player: PlayerRefAst,
    },
    /// Redistribute life totals among any number of players (`Reverse the Sands`).
    RedistributeLifeTotals {
        player: PlayerRefAst,
    },
    /// Reselect targets or attacked player for a spell, copy, or attacking creature (`Chef's Kiss`, `Capricopian`).
    ReselectTargets {
        subject: Box<SubjectRefAst>,
        #[serde(default)]
        random: bool,
    },
    /// Roll multiple dice with drop-lowest/highest or store-on-source semantics (`Berserker's Frenzy`, `Centaur of Attention`).
    RollDiceAndKeep {
        count: Box<ValueExprAst>,
        sides: u16,
        #[serde(default)]
        drop_lowest: u8,
        #[serde(default)]
        store_on_self: bool,
    },
    /// Modify the number of creatures a blocker may block or an opponent may block with (`Yare`, `Mirri, Weatherlight Duelist`).
    ChangeBlockerLimit {
        subject: Box<SubjectRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extra_blockers: Option<Box<ValueExprAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_blockers: Option<u8>,
        duration: DurationAst,
    },
    /// Lossless structured effect rule preserving full semantics for specialized Magic rules (`Karn Liberated`, `Global Ruin`, `Feather`, `Invert`, etc.).
    StructuredRule {
        kind: String,
        text: String,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        for_spells: Option<Box<ObjectFilterAst>>,
        #[serde(default)]
        next_only: bool,
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
    /// CR 613.4c dynamic signed power/toughness modification using arbitrary `ValueExprAst` expressions (`Snowblind`).
    DynamicModifyPT {
        subject: Box<SubjectRefAst>,
        power_delta: Box<ValueExprAst>,
        toughness_delta: Box<ValueExprAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Prototype alternate mana cost and power/toughness header (`{M} - P/T`).
    PrototypeStats {
        cost: Vec<CostAtomAst>,
        power: String,
        toughness: String,
    },
    /// Typed counter prevention static (`can't have -1/-1 counters put on them`, `can't get poison counters`).
    PreventCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: Option<String>,
        condition: Option<Box<ConditionAst>>,
    },
    /// Damage-as-though keyword proxy static (`all damage is dealt to you as though its source had infect`).
    DamageDealtAsThough {
        damage_filter: String,
        target: Box<SubjectRefAst>,
        source_keyword: String,
        condition: Option<Box<ConditionAst>>,
    },
    /// Leading `For each <per>, <static>` scaling wrapper (preserves quantifier losslessly).
    ScaledBy {
        per: Box<ValueExprAst>,
        inner: Box<StaticAbilityAst>,
    },
    /// Conspiracy / draft-matters static rule (`Reveal ~ as you draft it`, `As you draft a card, ...`).
    DraftAction(String),
    /// Static mana-spending rule (`You may spend white mana as though it were mana of any color`).
    ManaSpendRule {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mana_kind: Option<String>,
        as_though: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Static look/reveal rule (`Play with your hand revealed`, `You may look at face-down creatures ...`).
    LookOrRevealRule {
        rule: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Box<ObjectFilterAst>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        follow_up: Vec<InstructionAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Static rule paired with a player ignore-effect payment (`Leonin Arbiter`, `Lost in Thought`, `Damping Engine`).
    WithIgnoreCost {
        inner: Box<StaticAbilityAst>,
        who: String,
        cost: Vec<CostAtomAst>,
        duration: DurationAst,
    },
    /// Alternative activation/keyword cost (`Bruenor`, `New Perspectives`, `Heart of Kiran`).
    AlternativeAbilityCost {
        cost: Vec<CostAtomAst>,
        ability_kind: String,
        scope: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Damage-doubling static (`Double all damage ~ would deal`).
    DoubleAllDamage {
        source: Box<SubjectRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Fractional damage prevention (`prevent half that damage, rounded up`).
    PreventFractionDamage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<SubjectRefAst>>,
        target: Box<SubjectRefAst>,
        numerator: u8,
        denom: u8,
        rounding: Rounding,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Threshold damage prevention (`If a source would deal 3 or less damage to ~, prevent that damage`).
    PreventThresholdDamage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<SubjectRefAst>>,
        target: Box<SubjectRefAst>,
        max_damage: Box<ValueExprAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Damage prevention paired with replacement follow-up (`Panther Habit`, `Unbreathing Horde`).
    PreventAndReplaceDamage {
        target: Box<SubjectRefAst>,
        follow_up: Vec<InstructionAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Counter-placement scaling replacement (`Vorinclex, Monstrous Raider`).
    CounterModificationReplacement {
        player: PlayerRefAst,
        modifier: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rounding: Option<Rounding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Triggered ability paired inside a multi-sentence static paragraph (`Keranos`, `God-Eternal Kefnet`).
    PairedTrigger {
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
    },
    /// Static step-skipping rule (`Each player skips their untap step`).
    SkipStep {
        player: PlayerRefAst,
        step: TurnStepAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Static control-changing rule (`The monarch controls enchanted creature`).
    ControlChange {
        controller: String,
        subject: Box<SubjectRefAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Maximum counter cap (`~ can't have more than seven dream counters on it`).
    MaxCounters {
        subject: Box<SubjectRefAst>,
        counter_kind: String,
        max: i32,
    },
    /// Additional cost to activate matching activated or loyalty abilities (`Drought`, `Brutal Suppression`, `Carth the Lion`).
    AdditionalActivationCost {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<Box<SubjectRefAst>>,
        cost: CostAtomAst,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        per: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Override activation frequency or instant-speed timing for an ability category (`Oath of Teferi`, `Teferi, Master of Time`, `Birgi`, `Wonder Man`).
    ActivationLimitOverride {
        ability_kind: String,
        subject: Box<SubjectRefAst>,
        extra_times: u8,
        #[serde(default)]
        instant_speed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Override combat damage assignment or lethal-damage determination (`Arcades`, `Zilortha`, `Defensive Formation`, `Butcher Orgg`, `Cunning Giant`, `Loot`).
    CombatDamageAssignmentRule {
        subject: Box<SubjectRefAst>,
        rule: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Static die-roll adjustment or reroll ability (`Night Shift of the Living Dead`, `Xenosquirrels`, `Monitor Monitor`).
    DieRollModifier {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        cost: Vec<CostAtomAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<i8>,
        #[serde(default)]
        reroll: bool,
        #[serde(default)]
        once_each_turn: bool,
    },
    /// Dynamic maximum hand size static (`Winter, Misanthropic Guide`).
    ModifyMaximumHandSize {
        player: PlayerRefAst,
        size: Box<ValueExprAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
    /// Power bonus applied when crewing Vehicles and/or saddling Mounts (`Hotshot Mechanic`, `Deathless Pilot`).
    CrewOrSaddlePowerBonus {
        subject: Box<SubjectRefAst>,
        crews_vehicles: bool,
        saddles_mounts: bool,
        bonus: Box<ValueExprAst>,
    },
    /// Disjunctive blocking requirement where a blocker matching any listed filter may block (`Elven Riders`, `Amrou Seekers`, `Skirk Shaman`, `Serpent of Yawning Depths`).
    CantBeBlockedExceptByAny {
        subject: Box<SubjectRefAst>,
        allowed_filters: Vec<ObjectFilterAst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<ConditionAst>>,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        per: Option<Box<ValueExprAst>>,
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
    /// Compound replacement effect (`~ enters with <A> and <B>`).
    Compound(Vec<ReplacementEffectAst>),
    /// Leading `For each <per>, <replacement>` scaling wrapper.
    ScaledBy {
        per: Box<ValueExprAst>,
        inner: Box<ReplacementEffectAst>,
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
