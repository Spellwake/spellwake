//! Strictly-typed, compact compositional Intermediate Representation (`v2::ir`) for Spellwake's
//! ground-up Oracle parser.
//!
//! Design invariants:
//! - Mirror Spellwake's Universal Rules Toolbox (~45 orthogonal primitives).
//! - ZERO raw-prose `String` escape hatches: every instruction, trigger, replacement, static,
//!   filter, value, condition, duration, and restriction is represented with typed enums/structs.
//! - Explicit distinction between conjunctive (`all_types`, e.g. `artifact creature`) and
//!   disjunctive (`any_types`, e.g. `artifact or creature`) card-type predicates.
//! - Fail-closed parsing: unparsed tokens return `Err(ParseError)` instead of silent dropping.

use serde::{Deserialize, Serialize};

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation,
    PlayerStatKind, Rounding, StatKind, SupertypeMask, TurnStepAst,
};

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
}

/// Optional typed parameter on a keyword (e.g. `Equip {2}`, `Ward {1}`, `Protection from red`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeywordParam {
    Cost(Vec<Cost>),
    Number(u16),
    Filter(Box<ObjectFilter>),
    Color(ColorMask),
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
    ChosenPlayer,
    LowestLifePlayer,
    HighestLifePlayer,
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
    EnergyPaidThisWay,
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
    AmongPriorCards,
    OfChosenPlayer(PlayerRef),
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
    Each(Box<ObjectFilter>),
    All(Box<ObjectFilter>),
    Group(Box<ObjectFilter>),
    Compound(Vec<SubjectRef>),
    PlayersDealtCombatDamageBy {
        player: PlayerRef,
        source: Box<ObjectFilter>,
        this_game: bool,
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
    HasKeyword {
        subject: Box<SubjectRef>,
        keyword: KeywordKind,
    },
    ManaCostContains {
        subject: Box<SubjectRef>,
        symbol: ManaSymbol,
    },
    IsFirstCombatPhase,
    ControlsSelf,
    IsYourTurn,
    Not(Box<ConditionExpr>),
    And(Box<ConditionExpr>, Box<ConditionExpr>),
    Or(Box<ConditionExpr>, Box<ConditionExpr>),
}

/// Strictly-typed effect duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duration {
    Instant,
    UntilEndOfTurn,
    UntilEndOfCombat,
    UntilYourNextTurn,
    UntilEndOfNextTurn,
    UntilSourceLeaves,
    ForAsLongAs(Box<ConditionExpr>),
    ForAsLongAsExiled,
    Permanent,
}

/// Typed mana production specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSpec {
    Exact(ManaCost),
    AnyColor {
        amount: ValueExpr,
        single_color: bool,
    },
}

/// Typed mana spend restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaSpendRule {
    pub only_for: bool,
    pub cast_from_zone: Option<ZoneKind>,
    pub spell_filter: Option<ObjectFilter>,
}

/// Typed token specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenSpec {
    Predefined(PredefinedToken),
    Creature {
        power: ValueExpr,
        toughness: ValueExpr,
        colors: ColorMask,
        card_types: CardTypeMask,
        subtypes: Vec<String>,
        keywords: Vec<KeywordSpec>,
    },
    CopyOf {
        source: Box<SubjectRef>,
    },
}

/// Well-known MTG predefined tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredefinedToken {
    Treasure,
    Clue,
    Food,
    Blood,
    Map,
    Powerstone,
    Shard,
}

/// Typed cost atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cost {
    Mana(ManaCost),
    TapSelf,
    UntapSelf,
    PayLife(ValueExpr),
    SacrificeSelf,
    Sacrifice {
        count: ValueExpr,
        filter: ObjectFilter,
    },
    Discard {
        count: ValueExpr,
        filter: Option<ObjectFilter>,
    },
    ExileSelf,
    RemoveCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
}

/// Typed activation/cast timing restriction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationRestriction {
    AsSorcery,
    AsInstant,
    DuringYourTurn,
    DuringCombat,
    OnceEachTurn,
    Once,
}

/// Typed life adjustment kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifeOp {
    Gain,
    Lose,
    Set,
}

/// Typed zone operation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneOpKind {
    Move,
    Destroy {
        can_regenerate: bool,
    },
    Exile {
        face_down: bool,
        counters: Option<(CounterKind, ValueExpr)>,
    },
    Sacrifice,
    Discard {
        hand: bool,
        random: bool,
    },
    Draw,
    Mill,
    SearchLibrary {
        reveal: bool,
        shuffle: bool,
        exact: bool,
    },
    Shuffle,
    LookTop {
        reveal: bool,
    },
    Reveal,
    CounterSpell {
        unless_pay: Option<Box<Cost>>,
    },
    SeparateIntoPiles {
        pile_count: u8,
        face_down_piles: u8,
    },
    ChoosePile,
}

/// Typed object mutation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectMutation {
    Tap,
    Untap,
    ModifyPT {
        power: PtDelta,
        toughness: PtDelta,
        scale: Option<Box<ValueExpr>>,
    },
    SetDynamicBasePT {
        power: ValueExpr,
        toughness: ValueExpr,
        add_types: CardTypeMask,
        keep_existing_types: bool,
    },
    GrantKeywords(Vec<KeywordSpec>),
    RemoveKeywords(Vec<KeywordSpec>),
    RemoveAllAbilities,
    GrantAbility(Box<CompiledFaceV2>),
    PutCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
    RemoveCounters {
        kind: CounterKind,
        count: Option<ValueExpr>,
        all: bool,
    },
    MoveCounters {
        from: Box<SubjectRef>,
        to: Box<SubjectRef>,
        kind: CounterKind,
        count: ValueExpr,
    },
    DistributeCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
    GainControl {
        controller: PlayerRef,
    },
    AttachTo(Box<SubjectRef>),
    Transform,
    TurnFaceUp,
    TurnFaceDown,
    Goad,
    CantAttack,
    CantBlock,
    CantAttackOrBlock,
    CantBeBlocked,
}

/// Typed player mutation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerMutation {
    AddMana(ManaSpec),
    AddManaSpendRestriction(ManaSpendRule),
    AddManaSpendRider {
        spell_filter: ObjectFilter,
        rider: Vec<Effect>,
    },
    GainCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
    Scry(ValueExpr),
    Surveil(ValueExpr),
    Investigate(ValueExpr),
    Proliferate,
    BecomeMonarch,
    ExtraTurn,
    SkipNextTurn,
    SkipNextStep(TurnStepAst),
    WinGame,
    LoseGame,
    AdditionalLandPlay(u8),
    AdditionalCombat {
        followed_by_main: bool,
    },
    PayCost(Cost),
    GrantPlayPermission {
        cast_only: bool,
        subject: Box<SubjectRef>,
        free: bool,
    },
    CreateEmblem(Box<CompiledFaceV2>),
}

/// Typed modal choice count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceCount {
    Exact(u8),
    UpTo(u8),
    AnyNumber,
}

/// Typed characteristic choice category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharacteristicKind {
    CreatureType,
    CardType,
    Color,
    CardName,
    BasicLandType,
    Number,
}

/// One mode in a modal spell/ability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalMode {
    pub body: Vec<Effect>,
}

/// One mode in a Spree spell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreeMode {
    pub cost: Vec<Cost>,
    pub body: Vec<Effect>,
}

/// Typed interactive choice kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceKind {
    Modal {
        count: ChoiceCount,
        modes: Vec<ModalMode>,
    },
    Spree {
        modes: Vec<SpreeMode>,
    },
    VillainousChoice {
        options: Vec<Vec<Effect>>,
    },
    ChooseBranch(Vec<Vec<Effect>>),
    ChooseCharacteristic(CharacteristicKind),
    ChooseObject(SubjectRef),
    ChooseKeywordAndGrant {
        subject: SubjectRef,
        keywords: Vec<KeywordSpec>,
        duration: Duration,
    },
}

/// Typed iteration domain for `Effect::ForEach`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForEachDomain {
    Player(PlayerRef),
    PlayersMatching(Box<SubjectRef>),
    GraveyardWith(Box<ObjectFilter>),
    Object(Box<ObjectFilter>),
    Count(ValueExpr),
}

/// Step owner for step-based triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOwner {
    Next,
    YourNext,
    Your,
    EachPlayer,
    EachOpponent,
    ThatPlayer,
}

/// Delayed trigger specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelayedTriggerKind {
    Step { step: TurnStepAst, owner: StepOwner },
    Event(TriggerSpec),
}

/// Count specification for zone moves (`count`, `up_to`, `any_number`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountSpec {
    pub count: ValueExpr,
    pub up_to: bool,
    pub any_number: bool,
}

/// Core orthogonal `v2` effect instruction (~20 primitives, zero raw-prose Strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    ZoneOp {
        actor: PlayerRef,
        op: ZoneOpKind,
        subject: SubjectRef,
        from: Vec<ZoneRef>,
        to: Option<ZoneDest>,
        modifiers: EntryModifiers,
        except: Option<Box<SubjectRef>>,
        count: Option<CountSpec>,
    },
    DealDamage {
        source: SubjectRef,
        target: SubjectRef,
        amount: ValueExpr,
        divided_by: Option<PlayerRef>,
    },
    AdjustLife {
        player: PlayerRef,
        amount: ValueExpr,
        op: LifeOp,
    },
    MutateObject {
        subject: SubjectRef,
        mutation: ObjectMutation,
        duration: Duration,
    },
    MutatePlayer {
        player: PlayerRef,
        mutation: PlayerMutation,
        duration: Duration,
    },
    CreateToken {
        controller: PlayerRef,
        count: ValueExpr,
        spec: TokenSpec,
        modifiers: EntryModifiers,
    },
    CopyObject {
        actor: PlayerRef,
        subject: SubjectRef,
        count: ValueExpr,
        modifiers: EntryModifiers,
    },
    ApplyContinuous {
        effect: Box<StaticAbility>,
        duration: Duration,
    },
    ApplyReplacement {
        replacement: Box<Replacement>,
        duration: Duration,
    },
    PromptChoice {
        chooser: PlayerRef,
        kind: ChoiceKind,
    },
    Optional {
        actor: PlayerRef,
        body: Vec<Effect>,
    },
    BranchCondition {
        condition: ConditionExpr,
        then_body: Vec<Effect>,
        else_body: Vec<Effect>,
    },
    IfPrevious {
        succeeded: bool,
        body: Vec<Effect>,
    },
    ReplacePrevious(Vec<Effect>),
    Unless {
        body: Vec<Effect>,
        payer: Option<PlayerRef>,
        cost: Option<Cost>,
        condition: Option<ConditionExpr>,
    },
    ForEach {
        over: ForEachDomain,
        body: Vec<Effect>,
    },
    BindVariable {
        var: VarName,
        expr: ValueExpr,
        body: Vec<Effect>,
    },
    DelayedTrigger {
        trigger: DelayedTriggerKind,
        body: Vec<Effect>,
    },
    ScopedDuration {
        duration: Duration,
        body: Vec<Effect>,
    },
    Restriction(Vec<ActivationRestriction>),
}

/// Typed static rule restriction kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleRestrictionKind {
    CantAttack,
    CantBlock,
    CantAttackOrBlock,
    CantBeBlocked,
    CantBeCountered,
    CantBeTargetedBy(Box<ObjectFilter>),
    AreGoaded,
}

/// Typed static ability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticAbility {
    ContinuousModifyPT {
        subject: SubjectRef,
        power: PtDelta,
        toughness: PtDelta,
        grant_keywords: Vec<KeywordSpec>,
        scale: Option<Box<ValueExpr>>,
        condition: Option<ConditionExpr>,
    },
    GrantKeywords {
        subject: SubjectRef,
        keywords: Vec<KeywordSpec>,
        condition: Option<ConditionExpr>,
    },
    GrantAbility {
        subject: SubjectRef,
        ability: Box<CompiledFaceV2>,
        condition: Option<ConditionExpr>,
    },
    CostModifier {
        subject: ObjectFilter,
        delta: ManaCost,
        less: bool,
        scale: Option<ValueExpr>,
        min_mana: Option<u8>,
        condition: Option<ConditionExpr>,
    },
    RuleRestriction {
        subject: Option<SubjectRef>,
        rule: RuleRestrictionKind,
        condition: Option<ConditionExpr>,
    },
    NextSpellModifier {
        filter: ObjectFilter,
        effect: Box<StaticAbility>,
    },
    AttachmentRestriction(ObjectFilter),
    Compound(Vec<StaticAbility>),
}

/// Typed replacement effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Replacement {
    EntersTapped {
        unless: Option<ConditionExpr>,
    },
    EntersWithCounters {
        subject: Option<SubjectRef>,
        kind: CounterKind,
        count: ValueExpr,
        additional: bool,
    },
    EventInstead {
        event: TriggerEvent,
        instead: Vec<Effect>,
    },
}

/// Typed trigger event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerEvent {
    ZoneChange {
        from: Option<ZoneKind>,
        to: Option<ZoneKind>,
        subject: SubjectRef,
    },
    Dies {
        subject: SubjectRef,
    },
    SpellCast {
        caster: PlayerRef,
        spell: Option<ObjectFilter>,
        from_outside_hand: bool,
    },
    Attacks {
        subject: Option<SubjectRef>,
        player: Option<PlayerRef>,
    },
    DealsDamage {
        combat_only: bool,
        source: Option<SubjectRef>,
        target: Option<SubjectRef>,
    },
    StepBegins {
        step: TurnStepAst,
        owner: StepOwner,
    },
    PermanentSacrificed {
        player: PlayerRef,
        subject: SubjectRef,
    },
    CardDrawn {
        player: PlayerRef,
    },
    LifeChanged {
        player: PlayerRef,
        gained: bool,
    },
    CardDiscarded {
        player: PlayerRef,
    },
    LandPlayed {
        player: PlayerRef,
    },
    CounterPlaced {
        subject: SubjectRef,
        kind: Option<CounterKind>,
    },
}

/// Trigger specification (one or more events + optional intervening-if).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerSpec {
    pub events: Vec<TriggerEvent>,
    pub intervening_if: Option<ConditionExpr>,
}

/// Planeswalker loyalty cost delta (`+1`, `-2`, `0`, `-X`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoyaltyDelta {
    Fixed(i16),
    MinusX,
}

/// Compiled card ability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ability {
    Activated {
        cost: Vec<Cost>,
        body: Vec<Effect>,
        restrictions: Vec<ActivationRestriction>,
    },
    Triggered {
        trigger: TriggerSpec,
        body: Vec<Effect>,
    },
    Loyalty {
        delta: LoyaltyDelta,
        body: Vec<Effect>,
    },
    SagaChapter {
        chapters: Vec<u8>,
        body: Vec<Effect>,
    },
}
