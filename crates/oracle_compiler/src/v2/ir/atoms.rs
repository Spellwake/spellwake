//! Core atomic IR types (keywords, counters, zones, filters, subjects, values, conditions)
//! re-exported from [`super`].

use serde::{Deserialize, Serialize};

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation,
    PlayerStatKind, Rounding, StatKind, SupertypeMask, TurnStepAst,
};

use super::{Ability, Cost, CountSpec, Effect, Replacement, StaticAbility};

/// Source byte span inside a normalized clause or face.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Structured fail-closed parse error carrying exact source span and diagnostic reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    pub span: Span,
    pub stage: ParseStage,
    pub message: String,
    pub text: String,
}

/// Pipeline stage where a `v2` parse error occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseStage {
    Lexer,
    Cost,
    Trigger,
    Effect,
    Static,
    Replacement,
    Face,
}

/// Top-level compiled card in `v2::ir`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledCardV2 {
    pub oracle_id: String,
    pub name: String,
    pub layout: String,
    pub faces: Vec<CompiledFaceV2>,
    pub strictly_compiled: bool,
}

/// Compiled single card face in `v2::ir`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledFaceV2 {
    pub name: String,
    pub type_line: String,
    pub keywords: Vec<KeywordSpec>,
    pub abilities: Vec<Ability>,
    pub spell_effects: Vec<Effect>,
    pub statics: Vec<StaticAbility>,
    pub replacements: Vec<Replacement>,
    pub errors: Vec<ParseError>,
}

impl CompiledFaceV2 {
    /// True when every line/clause on this face parsed fail-closed with zero leftover tokens.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Typed mana symbol atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSymbol {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
    Snow,
    Generic(u16),
    VariableX,
    Tap,
    Untap,
    Energy,
    Ticket,
    Pawprint,
    AnyPhyrexian,
    Hybrid(Box<ManaSymbol>, Box<ManaSymbol>),
    Phyrexian(Box<ManaSymbol>),
}

/// Sequence of mana symbols (e.g. `{1}{R}`, `{U}{B}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

/// Typed keyword ability or keyword action identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeywordKind {
    Flying,
    FirstStrike,
    DoubleStrike,
    Deathtouch,
    Haste,
    Hexproof,
    Indestructible,
    Lifelink,
    Menace,
    Reach,
    Trample,
    Vigilance,
    Defender,
    Flash,
    Shroud,
    Ward,
    Equip,
    Kicker,
    Multikicker,
    Flashback,
    Cycling,
    Suspend,
    Spree,
    Protection,
    Enchant,
    Toxic,
    Afflict,
    Annihilator,
    Cascade,
    Convoke,
    Delve,
    Prowess,
    Wither,
    Infect,
    Undying,
    Persist,
    Exalted,
    Extort,
    Fear,
    Intimidate,
    Skulk,
    Shadow,
    Horsemanship,
    Flanking,
    Banding,
    Changeling,
    Devoid,
    Storm,
    SplitSecond,
    Rebound,
    Retrace,
    Buyback,
    Entwine,
    Overload,
    Miracle,
    Foretell,
    Plot,
    Warp,
    Escape,
    Unearth,
    Blitz,
    Dash,
    Evoke,
    Bestow,
    Ninjutsu,
    Morph,
    Disguise,
    Crew,
    Saddle,
    Station,
    Partner,
    Companion,
    Fuse,
    Plainswalk,
    Islandwalk,
    Swampwalk,
    Mountainwalk,
    Forestwalk,
    Landwalk,
    CumulativeUpkeep,
    Affinity,
    Madness,
    Echo,
    Megamorph,
    Vanishing,
    Fading,
    Soulbond,
    Evolve,
    Outlast,
    Exploit,
    Renown,
    Awaken,
    Surge,
    Emerge,
    Escalate,
    Fabricate,
    Improvise,
    Aftermath,
    Embalm,
    Eternalize,
    Ascend,
    Assist,
    JumpStart,
    Mentor,
    Riot,
    Spectacle,
    Afterlife,
    Mutate,
    Encore,
    Boast,
    Disturb,
    Decayed,
    Daybound,
    Nightbound,
    Cleave,
    Training,
    Reconfigure,
    Compleated,
    Casualty,
    Myriad,
    ReadAhead,
    Enlist,
    Squad,
    Prototype,
    ForMirrodin,
    Backup,
    Bargain,
    Craft,
    Offspring,
    Impending,
    StartYourEngines,
    Harmonize,
    Mobilize,
    Phasing,
    Rampage,
    Bushido,
    Modular,
    Sunburst,
    Dredge,
    Bloodthirst,
    Graft,
    Ripple,
    Recover,
    Frenzy,
    Absorb,
    AuraSwap,
    Fortify,
    Gravestorm,
    Poisonous,
    Transfigure,
    Champion,
    Hideaway,
    Prowl,
    Reinforce,
    Conspire,
    Devour,
    LevelUp,
    UmbraArmor,
    BattleCry,
    LivingWeapon,
    Scavenge,
    Unleash,
    Cipher,
    Tribute,
    Dethrone,
    HiddenAgenda,
    Melee,
    Undaunted,
    Ingest,
    Meld,
    Ravenous,
    Demonstrate,
    Freerunning,
    Splice,
    NinjutsuCommander,
    Typecycling,
    BasicLandcycling,
    ChooseABackground,
    DoctorsCompanion,
    FriendsForever,
    JobSelect,
    MoreThanMeetsTheEye,
    LivingMetal,
    Soulshift,
    Firebending,
    Replicate,
    Sneak,
    Teamwork,
    Mayhem,
    Amplify,
    Gift,
    Provoke,
    Transmute,
    Increment,
    Paradigm,
    Offering,
    Haunt,
    Storied,
    Tiered,
    WebSlinging,
    Epic,
}

/// Typed Bloomburrow Gift kind (`Gift a card`, `Gift a Treasure`, `Gift a tapped Fish`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GiftKind {
    Card,
    Treasure,
    Food,
    TappedFish,
    Octopus,
    ExtraTurn,
}

/// Optional typed parameter on a keyword (e.g. `Equip {2}`, `Ward {1}`, `Protection from red`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeywordParam {
    Cost(Vec<Cost>),
    Number(u16),
    NumberAndCost(u16, Vec<Cost>),
    CostAndBasePt(Vec<Cost>, i32, i32),
    FilterAndCost(Box<ObjectFilter>, Vec<Cost>),
    Filter(Box<ObjectFilter>),
    Color(ColorMask),
    ChosenColor,
    ChosenPlayer,
    ChosenCardName,
    Everything,
    Gift(GiftKind),
    VariableX,
    VariableXBound(Box<ValueExpr>),
    PartnerName(String),
}

/// Typed keyword ability specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeywordSpec {
    pub kind: KeywordKind,
    pub param: Option<KeywordParam>,
}

/// Typed counter kind (eliminates `"a"`, `"an"`, `""`, and raw prose).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CounterKind {
    PlusOnePlusOne,
    MinusOneMinusOne,
    Loyalty,
    Time,
    Energy,
    Poison,
    Experience,
    Finality,
    Stun,
    Charge,
    Spore,
    Slumber,
    Lore,
    Shield,
    Oil,
    StunOrKeyword(KeywordKind),
    /// Arbitrary P/T counter (e.g. `-2/-2`, `+0/+1`, `+2/+2`).
    Pt(i8, i8),
    /// Explicitly unspecified counter kind (e.g. `Move a counter from ...`).
    Any,
    /// Single-word named custom counter (validated non-article, no spaces).
    Named(String),
}

/// Typed player reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerRef {
    You,
    TargetPlayer,
    TargetOpponent,
    EachPlayer,
    EachOpponent,
    EachOtherPlayer,
    EachOtherOpponent,
    AnyOpponent,
    AnyPlayer,
    ThatPlayer,
    DefendingPlayer,
    ActivePlayer,
    EnchantedPlayer,
    ItsController,
    ItsOwner,
    ThatObjectController,
    ThatSpellController,
    ChosenPlayer,
    LowestLifePlayer,
    HighestLifePlayer,
    MostObjectsPlayer,
    SearchedLibraryPlayer,
}

/// Typed zone category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    Battlefield,
    Hand,
    Graveyard,
    Library,
    Exile,
    CommandZone,
    Stack,
    OutsideTheGame,
}

/// Typed zone ownership qualifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneOwner {
    Default,
    You,
    Opponent,
    Opponents,
    TargetOpponent,
    TargetPlayer,
    DefendingPlayer,
    ThatPlayer,
    EachPlayer,
    Any,
    All,
    ItsOwner,
    TheirOwners,
    ThatCreatureOwner,
}

/// Typed zone reference (zone + owner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneRef {
    pub zone: ZoneKind,
    pub owner: ZoneOwner,
}

/// Position inside a library destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryPosition {
    Top,
    Bottom,
    TopOrBottom,
    Shuffled,
    FromTop(u16),
}

/// Order when placing multiple cards into a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryOrder {
    AnyOrder,
    RandomOrder,
}

/// Typed destination for zone moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneDest {
    pub zone: ZoneKind,
    pub owner: ZoneOwner,
    pub library_position: Option<LibraryPosition>,
    pub library_order: Option<LibraryOrder>,
    pub under_control: Option<ZoneOwner>,
}

/// Typed entry modifiers for battlefield/exile/token creation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryModifiers {
    pub tapped: bool,
    pub attacking: bool,
    pub face_down: bool,
    pub transformed: bool,
    pub with_counter: Option<(CounterKind, u16)>,
    pub attached_to: Option<Box<SubjectRef>>,
}

/// Bitflags for `ObjectFilter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterFlags(pub u32);

impl FilterFlags {
    pub const EMPTY: Self = Self(0);
    pub const EXCLUDE_SELF: Self = Self(1 << 0);
    pub const DIFFERENT_NAMES: Self = Self(1 << 1);
    pub const ATTACKED_THIS_TURN: Self = Self(1 << 2);
    pub const DEALT_DAMAGE_THIS_TURN: Self = Self(1 << 3);
    pub const DIED_THIS_TURN: Self = Self(1 << 4);
    pub const ENTERED_THIS_TURN: Self = Self(1 << 5);
    pub const MODIFIED: Self = Self(1 << 6);
    pub const EQUIPPED: Self = Self(1 << 7);
    pub const ENCHANTED: Self = Self(1 << 8);
    pub const FACE_DOWN: Self = Self(1 << 9);
    pub const FACE_UP: Self = Self(1 << 10);
    pub const IS_COMMANDER: Self = Self(1 << 11);
    pub const NOT_COMMANDER: Self = Self(1 << 12);
    pub const WITH_ANY_COUNTER: Self = Self(1 << 13);
    pub const WITHOUT_COUNTERS: Self = Self(1 << 14);
    pub const EXILED_WITH_SELF: Self = Self(1 << 15);
    pub const LKI_PAST_TENSE: Self = Self(1 << 16);
    pub const TRANSFORMED: Self = Self(1 << 17);
    pub const KICKED: Self = Self(1 << 18);
    pub const GOADED: Self = Self(1 << 19);
    pub const SUSPECTED: Self = Self(1 << 20);
    pub const WARPED: Self = Self(1 << 21);
    pub const SUSPENDED: Self = Self(1 << 22);
    pub const DIFFERENT_POWERS: Self = Self(1 << 23);
    pub const HAS_ADVENTURE: Self = Self(1 << 24);
    pub const HAS_X_IN_MANA_COST: Self = Self(1 << 25);
    pub const AT_RANDOM: Self = Self(1 << 26);
    pub const SINGLE_TARGET: Self = Self(1 << 27);
    pub const BOTH_COLORS: Self = Self(1 << 28);
    pub const STICKERED: Self = Self(1 << 29);
    pub const RENOWNED: Self = Self(1 << 30);
    pub const NOT_ON_BATTLEFIELD: Self = Self(1 << 31);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Prior-action reference kind within the same resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriorActionKind {
    ExiledThisWay,
    SacrificedThisWay,
    DestroyedThisWay,
    DiscardedThisWay,
    RevealedThisWay,
    ReturnedThisWay,
    MilledThisWay,
    TappedThisWay,
    DealtDamageThisWay,
    DamagePreventedThisWay,
    EnergyPaidThisWay,
    SearchedThisWay,
    CreatedThisWay,
    CastThisWay,
    ExcessDamageThisWay,
    LookedAtScryingThisWay,
    CountersRemovedThisWay,
    LifePaidAsEntered,
    ThatMany,
}

/// Shared characteristic property for relational predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharedProperty {
    Color,
    CardType,
    PermanentType,
    CreatureType,
    LandType,
    Name,
    ManaValue,
    Power,
    Toughness,
}

/// Stat bound predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatBound {
    pub stat: StatKind,
    pub op: CompareOp,
    pub value: Box<ValueExpr>,
}

/// Structured relational predicate on `ObjectFilter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterPredicate {
    AnyOf(Vec<ObjectFilter>),
    AttachedTo(Box<SubjectRef>),
    SharesPropertyWith {
        property: SharedProperty,
        reference: Box<SubjectRef>,
        negated: bool,
    },
    AffectedByPriorAction(PriorActionKind),
    NthSpellCastThisTurn(u8),
    Exclude(Box<SubjectRef>),
    TargetsSubject(Box<SubjectRef>),
    AttackedPlayerThisTurn(PlayerRef),
    FromOutsideHand,
    NotOwnedByController,
    NotManaAbility,
    HasCounterKind(CounterKind),
    HasNamedAbility(KeywordKind),
    HasManaAbility,
    HasBasicLandType,
    HasNameSticker,
    AmongPriorCards,
    OfChosenPlayer(PlayerRef),
    OfChosenColor,
    OfChosenCreatureType,
    OfChosenName,
    ManaCostIn(Vec<ManaCost>),
    InChosenPile,
    DrawnThisTurn,
    NamedSelf,
    DealtDamageBySelfThisTurn,
    KickedWith(ManaCost),
    GreatestStat {
        stat: StatKind,
        scope: Box<ObjectFilter>,
    },
    LeastStat {
        stat: StatKind,
        scope: Box<ObjectFilter>,
    },
    SameController,
    OwnedAndControlledByYou,
    OfThatColor,
    LesserManaValue {
        or_equal: bool,
    },
}

/// Strictly-typed orthogonal object filter with explicit AND (`all_types`) vs OR (`any_types`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectFilter {
    /// Conjunctive card-type mask (e.g. `artifact creature` -> ARTIFACT | CREATURE).
    pub all_types: CardTypeMask,
    /// Disjunctive card-type mask (e.g. `artifact or creature` -> ARTIFACT | CREATURE).
    pub any_types: CardTypeMask,
    pub forbidden_types: CardTypeMask,
    pub required_supertypes: SupertypeMask,
    pub forbidden_supertypes: SupertypeMask,
    pub subtypes: Vec<String>,
    pub subtypes_match_any: bool,
    pub forbidden_subtypes: Vec<String>,
    pub any_colors: ColorMask,
    pub forbidden_colors: ColorMask,
    pub color_arity: Option<ColorArity>,
    pub controller: Option<ControllerRelation>,
    pub owner: Option<ControllerRelation>,
    pub zones: Vec<ZoneRef>,
    pub combat: Option<CombatState>,
    pub tapped: Option<bool>,
    pub token: Option<bool>,
    pub required_keywords: Vec<KeywordKind>,
    pub keywords_match_any: bool,
    pub forbidden_keywords: Vec<KeywordKind>,
    pub stat_bounds: Vec<StatBound>,
    pub flags: FilterFlags,
    pub predicates: Vec<FilterPredicate>,
    pub any_permanent: bool,
    pub any_card: bool,
    pub any_spell: bool,
    pub is_ability: bool,
}

impl ObjectFilter {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.all_types.is_empty()
            && self.any_types.is_empty()
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
            && self.zones.is_empty()
            && self.combat.is_none()
            && self.tapped.is_none()
            && self.token.is_none()
            && self.required_keywords.is_empty()
            && self.forbidden_keywords.is_empty()
            && self.stat_bounds.is_empty()
            && self.flags.is_empty()
            && self.predicates.is_empty()
            && !self.any_permanent
            && !self.any_card
            && !self.any_spell
            && !self.is_ability
    }
}

/// Target cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetCount {
    Exact(u16),
    UpTo(u16),
    VariableX,
    UpToX,
    AnyNumber,
}

/// Strictly-typed anaphoric reference (zero arbitrary prose strings!).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnaphoraRef {
    It,
    Them,
    ThatCreature,
    ThatPermanent,
    ThatCard,
    ThatSpell,
    ThatToken,
    ThatLand,
    ThatObject,
    ThoseCreatures,
    ThosePermanents,
    ThoseCards,
    TheRest,
    TheExiledCard,
    OnePile,
    OtherPile,
    EquippedCreature,
    EnchantedCreature,
    EnchantedPermanent,
    SacrificedObject,
    ExiledObject,
    DestroyedObject,
    DiscardedObject,
    TheCopy,
    TheCopies,
    RevealedCard,
    SameTarget,
    BlightedCreature,
    ThatSource,
    ThatDamage,
}

/// Strictly-typed subject/target reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectRef {
    SelfObject,
    Anaphora(AnaphoraRef),
    AnyTarget,
    TargetPlayer {
        opponent_only: bool,
        count: TargetCount,
    },
    TargetObject {
        filter: Box<ObjectFilter>,
        count: TargetCount,
        another: bool,
    },
    Player(PlayerRef),
    BothPairedCreatures,
    HandOf(PlayerRef),
    OneOfThem(ValueExpr),
    Each(Box<ObjectFilter>),
    All(Box<ObjectFilter>),
    Group(Box<ObjectFilter>),
    Counted {
        filter: Box<ObjectFilter>,
        count: CountSpec,
    },
    Compound(Vec<SubjectRef>),
    PlayersDealtCombatDamageBy {
        player: PlayerRef,
        source: Box<ObjectFilter>,
        this_game: bool,
    },
    PlayerWhoControls {
        player: PlayerRef,
        filter: Box<ObjectFilter>,
        min_count: u16,
    },
}

/// Dynamic variable identifier (`X`, `Y`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarName {
    X,
    Y,
}

/// Aggregation operator for stat summation/extrema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregateOp {
    Sum,
    Max,
    Min,
}

/// Turn-history event kind counted in `ValueExpr::EventCountThisTurn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnEventKind {
    SpellCast,
    CardDrawn,
    CardDiscarded,
    LifeGained,
    LifeLost,
    CreatureDied,
}

/// Strictly-typed numeric expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueExpr {
    Const(i32),
    Var(VarName),
    PriorActionCount(PriorActionKind),
    Count(Box<ObjectFilter>),
    CounterCount {
        subject: Box<SubjectRef>,
        kind: CounterKind,
    },
    PlayerCounterCount {
        player: PlayerRef,
        kind: CounterKind,
    },
    DistinctCardTypes(ZoneRef),
    DomainCount(PlayerRef),
    Devotion(ColorMask),
    PartySize(PlayerRef),
    TimesKicked,
    ColorCount(Box<SubjectRef>),
    ObjectStat {
        subject: Box<SubjectRef>,
        stat: StatKind,
    },
    AggregateStat {
        op: AggregateOp,
        stat: StatKind,
        filter: Box<ObjectFilter>,
    },
    PlayerStat {
        player: PlayerRef,
        stat: PlayerStatKind,
    },
    OpponentsCount,
    EventCountThisTurn {
        event: TurnEventKind,
        player: Option<PlayerRef>,
        exclude_first: bool,
    },
    ManaSpentFromFilter(Box<ObjectFilter>),
    Speed(PlayerRef),
    SnowManaSpent,
    DistinctManaValues(ZoneRef),
    ManaPaidThisWay,
    ColorsOfManaSpent,
    TargetsBeyondFirst,
    CommanderCastCount(PlayerRef),
    DamageDealtToOpponentsThisTurn {
        noncombat_only: bool,
    },
    MutatedCount,
    DistinctCounterKinds(Box<ObjectFilter>),
    DistinctCardTypesAmong(Box<ObjectFilter>),
    PlayersWhoDidThisTurn {
        event: TurnEventKind,
        opponents_only: bool,
    },
    DamageDealtToPlayerThisTurn(PlayerRef),
    StatDifference(Box<SubjectRef>),
    HighestLifeTotal,
    ConvokedCount,
    VisitedAttractionsThisTurn,
    Add(Box<ValueExpr>, Box<ValueExpr>),
    Sub(Box<ValueExpr>, Box<ValueExpr>),
    Mul(Box<ValueExpr>, Box<ValueExpr>),
    Div {
        expr: Box<ValueExpr>,
        denom: i32,
        rounding: Rounding,
    },
}

/// Signed power/toughness delta atom (`+2`, `-1`, `+X`, `+0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PtDelta {
    Fixed(i16),
    PlusX,
    MinusX,
}

/// Typed object state predicate for conditions (`~ is tapped`, `~ is untapped`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectStateKind {
    Tapped,
    Untapped,
    Attacking,
    Blocking,
    Blocked,
    Unblocked,
    Equipped,
    Enchanted,
    Modified,
    FaceDown,
    Paired,
    Saddled,
    AttackedOrBlockedThisTurn,
    Monstrous,
    AttackingAlone,
}

/// Zero-argument boolean game/spell condition flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionFlag {
    PermanentDiedThisTurn,
    PermanentLeftThisTurn,
    HasCitysBlessing,
    HasInitiative,
    HasEnduringStory,
    GiftWasPromised,
    WasBargained,
    WonCoinFlip,
    CastUsingTeamwork,
    TributeNotPaid,
    AdditionalCostPaid,
    ManaSpentNoColor,
    FirstTimeEachTurn,
    IsFirstCombatPhase,
    ControlsSelf,
    IsYourTurn,
    IsDay,
    IsNight,
    CreatureAttackingYou,
    IsNotManaAbility,
    CardsExiledWithSelf,
    LesserManaValueSpell,
    NoCreaturesAttackedThisTurn,
    NoCreaturesOnBattlefield,
    NoOtherCreatureHasGreaterPower,
    WasKicked,
    WasCastFromHand,
    YouCastIt,
}

/// Player-scoped boolean condition predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerConditionKind {
    AttackedThisTurn,
    AttackedThisStep,
    IsMonarch,
    HasFullParty,
    Poisoned,
    CardsLeftGraveyardThisTurn,
    LandEnteredThisTurn,
    CompletedDungeon,
    DescendedThisTurn,
    CastSpellFromHandThisTurn,
    ControlsCommander,
    CommittedCrimeThisTurn,
    LostLifeLastTurn,
    LostLifeThisTurn,
    PlayedLandThisTurn,
    PlayedFromExileThisTurn,
    NoOpponentHasMoreLifeThan,
    NoOtherCreaturesAttacking,
    ControlsMostCreatures,
    DealtDamageThisTurn { combat_only: bool },
    LifeAboveStarting { margin: i32 },
    AttackedWithCount { min_count: u16 },
    AttackedWithTotalPower { min_power: i32 },
}

/// Subject-scoped boolean condition predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectConditionKind {
    RemainsExiled,
    NotCastOrNoManaSpent,
    EnteredThisTurn,
    HadCountersOnIt,
    HasChosenName,
    WasCast,
    InCommandZoneOrBattlefield,
    AttackedBattleThisTurn,
    DealtDamageThisTurn { combat_only: bool },
    AttackedThisCombat { or_blocked: bool },
    HasntDealtDamageYet { combat_only: bool },
    HasCounters(Option<CounterKind>),
    DamageDealtThisTurn { min_damage: u16 },
}

/// Zone-level diversity or emptiness metric kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneMetricKind {
    DistinctCardTypes,
    DistinctPermanentTypes,
    IsEmpty,
}

/// Filter-level condition metric kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterConditionKind {
    RevealedThisWay,
    ManaSpentFrom,
    ColorsAmong,
    CountersAmong,
    OpponentControlsMoreBy,
}

/// Spell/cost/turn event condition check kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventCheckKind {
    ManaCostSpent(ManaCost),
    KickedWith(ManaCost),
    KeywordCostPaid(KeywordKind),
    PriorObjectEvent(PriorActionKind),
    DidntPutInZone(ZoneKind),
    MostCommonColor(ColorMask),
    ResolutionCountThisTurn(u8),
    PlayersLostGame(u8),
    SpellsCastLastTurn { op: CompareOp, count: u16 },
    DuringStep(TurnStepAst),
    TiedForExtremeStat { stat: StatKind, greatest: bool },
}

/// Strictly-typed boolean condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionExpr {
    Compare {
        lhs: Box<ValueExpr>,
        op: CompareOp,
        rhs: Box<ValueExpr>,
    },
    Controls {
        player: PlayerRef,
        filter: Box<ObjectFilter>,
        min_count: u16,
    },
    ControlsSubject {
        player: PlayerRef,
        subject: Box<SubjectRef>,
    },
    ControlsMoreThan {
        player: PlayerRef,
        other: PlayerRef,
        filter: Box<ObjectFilter>,
    },
    HasKeyword {
        subject: Box<SubjectRef>,
        keyword: KeywordKind,
    },
    SubjectState {
        subject: Box<SubjectRef>,
        state: ObjectStateKind,
    },
    SubjectMatches {
        subject: Box<SubjectRef>,
        filter: Box<ObjectFilter>,
    },
    SubjectCombatWith {
        subject: Box<SubjectRef>,
        other: Box<ObjectFilter>,
    },
    SubjectAttachedTo {
        subject: Box<SubjectRef>,
        target: Box<ObjectFilter>,
    },
    AttachedCount {
        subject: Box<SubjectRef>,
        filter: Box<ObjectFilter>,
        min_count: u16,
    },
    SharesTypeWith {
        subject: Box<SubjectRef>,
        other: Box<SubjectRef>,
        kind: SharedProperty,
    },
    HasGreaterStatThan {
        subject: Box<SubjectRef>,
        other: Box<SubjectRef>,
        stat: Option<StatKind>,
    },
    TargetsFilter {
        subject: Box<SubjectRef>,
        filter: Box<ObjectFilter>,
    },
    ManaCostContains {
        subject: Box<SubjectRef>,
        symbol: ManaSymbol,
    },
    InZone {
        subject: Box<SubjectRef>,
        zone: ZoneRef,
    },
    CastFromZone {
        subject: Box<SubjectRef>,
        zone: ZoneRef,
    },
    ZoneHasCount {
        filter: Box<ObjectFilter>,
        zone: ZoneRef,
        min_count: u16,
    },
    ZoneMetric {
        zone: ZoneRef,
        kind: ZoneMetricKind,
        min_count: u16,
    },
    PlayerTurnEvent {
        player: PlayerRef,
        event: TurnEventKind,
        filter: Option<Box<ObjectFilter>>,
        min_count: u16,
    },
    PlayerHasCounters {
        player: PlayerRef,
        kind: CounterKind,
        min_count: u16,
    },
    PermanentEnteredThisTurn {
        filter: Box<ObjectFilter>,
        controller: PlayerRef,
        min_count: u16,
    },
    SacrificedPermanentThisTurn {
        player: PlayerRef,
        filter: Box<ObjectFilter>,
    },
    PriorActionMatched {
        action: PriorActionKind,
        filter: Box<ObjectFilter>,
    },
    ColoredManaSpent {
        color: ColorMask,
        min_count: u8,
    },
    Flag(ConditionFlag),
    PlayerCheck {
        player: PlayerRef,
        kind: PlayerConditionKind,
    },
    SubjectCheck {
        subject: Box<SubjectRef>,
        kind: SubjectConditionKind,
    },
    FilterCheck {
        filter: Box<ObjectFilter>,
        kind: FilterConditionKind,
        value: u16,
    },
    EventCheck(EventCheckKind),
    Not(Box<ConditionExpr>),
    And(Box<ConditionExpr>, Box<ConditionExpr>),
    Or(Box<ConditionExpr>, Box<ConditionExpr>),
}
