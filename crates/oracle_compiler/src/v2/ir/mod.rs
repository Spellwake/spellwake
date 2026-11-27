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

use crate::ast::{CardTypeMask, ColorMask, SupertypeMask, TurnStepAst};

mod atoms;
pub use atoms::*;

/// Strictly-typed effect duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duration {
    Instant,
    UntilEndOfTurn,
    UntilEndOfCombat,
    UntilYourNextTurn,
    UntilYourNextEndStep,
    UntilYourNextUpkeep,
    UntilEndOfNextTurn,
    UntilSourceLeaves,
    ForAsLongAs(Box<ConditionExpr>),
    ForAsLongAsExiled,
    ForTheRestOfTheGame,
    Permanent,
}

/// Typed mana production specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSpec {
    Exact(ManaCost),
    Choice(Vec<ManaCost>),
    AnyColor {
        amount: ValueExpr,
        single_color: bool,
    },
    DifferentColors(ValueExpr),
    ChosenColor(ValueExpr),
    ThatColor(ValueExpr),
    Either(Box<ManaSpec>, Box<ManaSpec>),
    Scaled {
        base: Box<ManaSpec>,
        scale: ValueExpr,
    },
}

/// Typed mana spend purpose for `Spend this mana only to ...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSpendPurpose {
    CastSpells(Option<ObjectFilter>),
    CastFromZone(ZoneKind),
    ActivateAbilities(Option<ObjectFilter>),
    Foretell,
    AnyOf(Vec<ManaSpendPurpose>),
}

/// Typed mana spend restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaSpendRule {
    pub only_for: bool,
    pub cast_from_zone: Option<ZoneKind>,
    pub spell_filter: Option<ObjectFilter>,
}

/// Typed copy-token exception clause (`, except it has haste`, `, except it isn't legendary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyException {
    AddKeywords(Vec<KeywordSpec>),
    NotLegendary,
    AddTypes(CardTypeMask),
    AddAbility(Box<CompiledFaceV2>),
    HasThisAbility,
    KeepOwnName,
    EntersWithCounters {
        kind: CounterKind,
        count: ValueExpr,
        condition: Option<ConditionExpr>,
    },
    AddKeywordsConditional {
        keywords: Vec<KeywordSpec>,
        condition: ConditionExpr,
    },
    AddSupertypes {
        supertypes: SupertypeMask,
        keep_existing_types: bool,
    },
    SetColor(ColorMask),
    KeepOwnOtherAbilities,
    SetBasePTAndTypes {
        power: Option<i32>,
        toughness: Option<i32>,
        subtypes: Vec<String>,
        types: CardTypeMask,
        keep_existing_types: bool,
    },
}

/// Typed token specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenSpec {
    Predefined(PredefinedToken),
    Creature {
        power: Option<ValueExpr>,
        toughness: Option<ValueExpr>,
        colors: ColorMask,
        card_types: CardTypeMask,
        subtypes: Vec<String>,
        keywords: Vec<KeywordSpec>,
        abilities: Vec<CompiledFaceV2>,
    },
    CopyOf {
        source: Box<SubjectRef>,
        exceptions: Vec<CopyException>,
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
    Lander,
    Junk,
    Gold,
    Incubator,
    Role,
    Walker,
    Mutagen,
    WickedRole,
    MonsterRole,
    RoyalRole,
    SorcererRole,
    CursedRole,
    YoungHeroRole,
    Tarmogoyf,
    Heartwood,
    Vibranium,
    Mutavault,
}

/// Typed cost atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cost {
    Mana(ManaCost),
    ScaledMana {
        base: ManaCost,
        scale: ValueExpr,
    },
    TapSelf,
    UntapSelf,
    PayLife(ValueExpr),
    PayEnergy(ValueExpr),
    SacrificeSelf,
    Sacrifice {
        count: ValueExpr,
        filter: ObjectFilter,
    },
    Discard {
        count: ValueExpr,
        filter: Option<ObjectFilter>,
        random: bool,
    },
    DiscardHand,
    ExileSelf,
    Exile {
        count: ValueExpr,
        filter: ObjectFilter,
        from: Vec<ZoneRef>,
    },
    TapUntapped {
        count: ValueExpr,
        filter: ObjectFilter,
    },
    ReturnToHand {
        subject: SubjectRef,
    },
    PutCounters {
        kind: CounterKind,
        count: ValueExpr,
        subject: SubjectRef,
    },
    RemoveCounters {
        kind: CounterKind,
        count: ValueExpr,
        subject: Option<SubjectRef>,
    },
    Mill(ValueExpr),
    RevealFromHand {
        count: ValueExpr,
        filter: ObjectFilter,
    },
    ExertSelf,
    CollectEvidence(ValueExpr),
    ExileTop {
        zone: ZoneRef,
        count: ValueExpr,
    },
    UnattachSelf,
    Blight(ValueExpr),
    Waterbend(ManaCost),
    PutZone {
        subject: SubjectRef,
        dest: ZoneDest,
    },
    ChooseObject(SubjectRef),
    Forage,
    PayAnyAmountLife,
    PayAnyAmountMana,
    UntapTapped {
        count: ValueExpr,
        filter: ObjectFilter,
    },
    RemoveEitherCounter {
        kind1: CounterKind,
        kind2: CounterKind,
        subject: Option<SubjectRef>,
    },
    Behold(ObjectFilter),
    Or(Box<Cost>, Box<Cost>),
}

/// Specific combat sub-step for timing restrictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatStepKind {
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
}

/// Typed activation/cast timing restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationRestriction {
    AsSorcery,
    AsInstant,
    DuringYourTurn,
    DuringYourUpkeep,
    DuringCombat,
    DuringEndOfCombat,
    BeforeAttackers,
    OnceEachTurn,
    Once,
    OnlyIf(Box<ConditionExpr>),
    AllowedPlayer(PlayerRef),
    ControlledContinuously,
    DuringCombatStep(CombatStepKind),
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
    LookAt,
    Reveal,
    CounterSpell {
        unless_pay: Option<Box<Cost>>,
    },
    SeparateIntoPiles {
        pile_count: u8,
        face_down_piles: u8,
    },
    ChoosePile,
    ExileUntilReveal {
        filter: Box<ObjectFilter>,
    },
}

/// Zero-argument state transition actions on an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectStateAction {
    Transform,
    TurnFaceUp,
    TurnFaceDown,
    Flip,
    Goad,
    Regenerate,
    DoesntUntapNextUntap,
    MustAttackEachCombat,
    Exert,
    Suspect,
    Detain,
    PhaseOut,
    Prepare,
    AssignsNoCombatDamage,
    SwitchPowerAndToughness,
    RemoveFromCombat,
    PutSticker,
}

/// Scaled keyword actions applied to an object (`Adapt N`, `Monstrosity N`, `Endure N`, `Explore N`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKeywordAction {
    Adapt,
    Monstrosity,
    Endure,
    Explore,
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
    DoubleCounters {
        kind: Option<CounterKind>,
    },
    DoubleStat {
        power: bool,
        toughness: bool,
    },
    BecomeCopy {
        source: Box<SubjectRef>,
        exceptions: Vec<CopyException>,
    },
    BecomeColors {
        colors: ColorMask,
        keep_existing_colors: bool,
    },
    BecomeCreatureTypes {
        subtypes: Vec<String>,
        add_types: CardTypeMask,
        keep_existing_types: bool,
        base_pt: Option<(i32, i32)>,
        keywords: Vec<KeywordSpec>,
    },
    BecomeChosen(CharacteristicKind),
    GainControl {
        controller: PlayerRef,
    },
    AttachTo(Box<SubjectRef>),
    ApplyRule(RuleRestrictionKind),
    CantBeRegenerated,
    StateAction(ObjectStateAction),
    KeywordAction {
        kind: ObjectKeywordAction,
        amount: ValueExpr,
    },
    CanBlockAdditional(u8),
    Fight(Box<SubjectRef>),
    RetainTypes(CardTypeMask),
    MustBlockThisTurn(Option<Box<SubjectRef>>),
    ChangeText(CharacteristicKind),
    RetargetTo(Box<SubjectRef>),
}

/// Zero-argument player keyword actions and game-state rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerActionKind {
    Proliferate,
    Populate,
    Manifest,
    ManifestDread,
    VentureIntoDungeon,
    RingTemptsYou,
    Forage,
    FlipCoin,
    Learn,
    Clash,
    BecomeMonarch,
    ExtraTurn,
    SkipNextTurn,
    WinGame,
    LoseGame,
    TakeInitiative,
    OpenAttraction,
    KeepManaAcrossSteps,
    RepeatProcessOnce,
    Recruit,
    FlashbackCostEqualsManaCost,
    SpendManaAsAnyType,
    NoMaxHandSize,
    ChooseSameModeMultiple,
    XCantBeZero,
    LifeTotalCantChange,
}

/// Scaled numeric player keyword actions (`Scry N`, `Surveil N`, `Investigate N`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaledPlayerAction {
    Scry,
    Surveil,
    Investigate,
    Connive,
    Incubate,
    Discover,
    Bolster,
    Support,
    Fateseal,
    CollectEvidence,
    Earthbend,
    Blight,
}

/// Typed mana spending or retention rule applied to a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaRuleKind {
    SpendRestriction(ManaSpendRule),
    SpendPurpose(ManaSpendPurpose),
    SpendForbidden(ManaSpendPurpose),
    SpendOnlyColorOnX(ColorMask),
    KeepColorAcrossSteps(ColorMask),
}

/// Typed player mutation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerMutation {
    AddMana(ManaSpec),
    ManaRule(ManaRuleKind),
    AddManaSpendRider {
        spell_filter: ObjectFilter,
        rider: Vec<Effect>,
    },
    GainCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
    Action(PlayerActionKind),
    ScaledAction {
        kind: ScaledPlayerAction,
        amount: ValueExpr,
    },
    Amass {
        subtype: String,
        amount: ValueExpr,
    },
    Retarget {
        subject: Box<SubjectRef>,
        new_target: Option<Box<SubjectRef>>,
    },
    RollDie {
        sides: u8,
        count: u8,
    },
    SkipNextStep(TurnStepAst),
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
    Empower {
        planeswalker: String,
        amount: ValueExpr,
    },
    Behold(ObjectFilter),
    XMaxBound(ValueExpr),
    GameIsDraw,
}

/// Typed modal choice count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceCount {
    Exact(u8),
    UpTo(u8),
    OneOrBoth,
    OneOrMore,
    VariableX,
    AnyNumber,
}

/// Typed characteristic choice category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharacteristicKind {
    CreatureType,
    CardType,
    Color,
    CardName,
    NonlandCardName,
    BasicLandType,
    BasicLandTypes(u8),
    OddOrEven,
    BinaryMode,
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
    EachPlayerWhoCant(PlayerRef),
    EachPlayerWhoDoes(PlayerRef),
    GraveyardWith(Box<ObjectFilter>),
    Object(Box<ObjectFilter>),
    ColorAmong(Box<ObjectFilter>),
    CardType,
    Count(ValueExpr),
    EachUnitOfValue(ValueExpr),
    EachOpponentWithCondition(ConditionExpr),
}

/// Step owner for step-based triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOwner {
    Next,
    NextTurn,
    YourNext,
    Your,
    Each,
    The,
    EachPlayer,
    EachOpponent,
    ThatPlayer,
    DefendingPlayer,
    EnchantedController,
    ChosenPlayer,
    ThatCombat,
    ThatTurn,
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
    PreventDamage {
        amount: Option<ValueExpr>,
        combat_only: bool,
        source: Option<SubjectRef>,
        target: Option<SubjectRef>,
        duration: Duration,
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
        exceptions: Vec<CopyException>,
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
    ReflexiveTrigger(Vec<Effect>),
    ReplacePrevious(Vec<Effect>),
    Unless {
        body: Vec<Effect>,
        payer: Option<PlayerRef>,
        cost: Option<Cost>,
        condition: Option<ConditionExpr>,
    },
    UnlessEffect {
        body: Vec<Effect>,
        unless: Vec<Effect>,
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
    StartingWith {
        player: PlayerRef,
        body: Vec<Effect>,
    },
    ExchangeControl {
        subject: SubjectRef,
    },
    Restriction(Vec<ActivationRestriction>),
}

/// Typed static rule restriction kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleRestrictionKind {
    CantAttack,
    CantBlock,
    CantBlockSubject(Box<SubjectRef>),
    CantBlockOrBeBlockedBy(Box<ObjectFilter>),
    CantAttackOrBlock,
    CantBeBlocked,
    CantBeCountered,
    CantBeCast,
    CantBeTargetedBy(Box<ObjectFilter>),
    CantBeBlockedBy(Box<ObjectFilter>),
    CantBeBlockedExceptBy(Box<ObjectFilter>),
    CantBeBlockedByMoreThan(u8),
    CantBeBlockedExceptByCount(u8),
    CantAttackTarget(Box<SubjectRef>),
    CantAttackUnlessDefendingControls(Box<ObjectFilter>),
    CantAttackUnless(Box<ConditionExpr>),
    CantBlockUnless(Box<ConditionExpr>),
    CantAttackOrBlockUnless(Box<ConditionExpr>),
    CanBlockOnly(Box<ObjectFilter>),
    DamageCantBePrevented,
    ActivatedAbilitiesCantBeActivated,
    MustBeBlocked,
    AreGoaded,
    CantAttackAlone,
    CantBlockAlone,
    CantAttackOrBlockAlone,
    CantBecomeUntapped,
    CantBeEnchantedBy(Box<ObjectFilter>),
    CantPhaseOut,
}

/// Typed player-level static restriction kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerRestrictionKind {
    CantCastSpells,
    CantActivateAbilities,
    CastOnlyAsSorcery,
    PlaysWithHandRevealed,
    CantGainLife,
    CantSearchLibraries,
    CantDrawMoreThanOne,
    CantCastMoreThanOnePerTurn,
    CantUntapMoreThan(u8),
    NoMaxHandSize,
    CantWinGame,
    CantLoseGame,
    CantPlayFromHand,
}

/// Zone/card visibility rule kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisibilityKind {
    LookAtTop(ZoneRef),
    LookAtExiledWithSelf,
    PlayWithTopRevealed,
}

/// Play/cast permission mode across zones, timing, and alternative costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastPermissionMode {
    PlayFromZone {
        top_only: bool,
        lands: bool,
        spells: bool,
    },
    CastFromZone,
    AsThoughFlash {
        extra_cost: Option<Box<Cost>>,
    },
    OncePerTurn {
        free: bool,
        alt_cost: Option<Vec<Cost>>,
    },
    FirstSpellFree,
    WithoutPaying,
    AlternativeCost(Vec<Cost>),
}

/// Trigger-count modification rule kind (doublers and suppression).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerModifierKind {
    AdditionalTriggers,
    EntersDontTrigger,
    AdditionalOnAbility,
}

/// Hand-size modification rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandSizeRule {
    Delta(i16),
    Set(ValueExpr),
}

/// Subject-scoped static rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectStaticRule {
    CanAttackWithDefender,
    DoesntUntap,
    MustAttack,
    CanBlockAdditional(u8),
    CanBlockAnyNumber,
    EntersTapped,
    EntersUntapped,
    MayNotUntap,
    AllCreatureTypes,
    UntapDuringEachOtherUntap,
    AdditionalTypesChosen,
    IsChosenColor,
    IsChosenType,
    AssignDamageByToughness,
    AssignDamageAsUnblocked,
    AllMustBlock,
    IgnoreKeywordForBlocking(KeywordKind),
    CopyActivatedAbilities(ObjectFilter),
    RemoveTypes(CardTypeMask),
    SetColors {
        colors: ColorMask,
        keep_existing: bool,
    },
}

/// Zero-argument or scalar global static card flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticFlagKind {
    CanBeCommander,
    MayExertOnAttack,
    DontLoseForZeroLife,
    EntersUnderOpponentControl,
    AppliesInAllZones,
    FlashSacrificeAtCleanup,
    NamedSourcesCantActivate,
    LoyaltyAsInstant,
    AllColors,
    EntersPrepared,
    AnyNumberInDeck,
    DraftFaceUp,
    RetainAttachment,
    CostReductionRule,
    CrewPowerBonus(i32),
}

/// Class / Leveler card structural marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LevelMarker {
    ClassLevel(u8),
    Band { min: u8, max: Option<u8> },
    BasePT(i32, i32),
}

/// Filter-scoped static rule kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterStaticKind {
    DoubleDamage,
    AlsoAttacksIfAble,
    ActivateAbilitiesAsHaste,
    SpendManaAsAnyColor,
    AttachmentRestriction,
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
    PlayerRestriction {
        player: PlayerRef,
        restriction: PlayerRestrictionKind,
        condition: Option<ConditionExpr>,
    },
    SkipStep {
        player: PlayerRef,
        step: TurnStepAst,
    },
    SetSubtypes {
        subject: SubjectRef,
        subtypes: Vec<String>,
        keep_existing: bool,
    },
    RemoveAllAbilities {
        subject: SubjectRef,
    },
    SetBasePT {
        subject: SubjectRef,
        power: i32,
        toughness: i32,
    },
    AdditionalCost(Vec<Cost>),
    AlternativeCost(Vec<Cost>),
    DynamicBasePT {
        power: Option<ValueExpr>,
        toughness: Option<ValueExpr>,
    },
    EntersWithCountersStatic {
        subject: SubjectRef,
        kind: CounterKind,
        count: ValueExpr,
        additional: bool,
        condition: Option<ConditionExpr>,
    },
    ControlEnchanted {
        controller: PlayerRef,
    },
    ActivationCostModifier {
        delta: ManaCost,
        less: bool,
        scale: Option<ValueExpr>,
        condition: Option<ConditionExpr>,
    },
    PreventDamageStatic {
        combat_only: bool,
        source: Option<SubjectRef>,
        target: Option<SubjectRef>,
    },
    AdditionalLandPlayStatic {
        player: PlayerRef,
        extra: u8,
    },
    CantCastSpellsMatching {
        player: PlayerRef,
        filter: ObjectFilter,
        condition: Option<ConditionExpr>,
    },
    MaxAttackersPerCombat {
        max: u8,
        target: Option<SubjectRef>,
    },
    RevealOpeningHand {
        effect: Vec<Effect>,
    },
    LegendRuleDoesntApply(Option<ObjectFilter>),
    ConditionalReplacement {
        condition: ConditionExpr,
        replacement: Box<Replacement>,
    },
    AttackTax {
        target: SubjectRef,
        cost: Cost,
        per_creature: bool,
    },
    BindVariable {
        var: VarName,
        expr: ValueExpr,
        inner: Box<StaticAbility>,
    },
    NextSpellModifier {
        filter: ObjectFilter,
        effect: Box<StaticAbility>,
    },
    VisibilityRule {
        player: PlayerRef,
        kind: VisibilityKind,
    },
    ZoneCastPermission {
        player: PlayerRef,
        filter: Option<ObjectFilter>,
        subject: Option<SubjectRef>,
        zone: Option<ZoneRef>,
        mode: CastPermissionMode,
        condition: Option<ConditionExpr>,
    },
    TriggerModifier {
        kind: TriggerModifierKind,
        filter: ObjectFilter,
        permanent_filter: Option<ObjectFilter>,
        extra_count: u8,
    },
    HandSize {
        player: PlayerRef,
        rule: HandSizeRule,
    },
    SubjectRule {
        subject: SubjectRef,
        rule: SubjectStaticRule,
        condition: Option<ConditionExpr>,
    },
    GlobalFlag(StaticFlagKind),
    LevelStructure(LevelMarker),
    FilterRule {
        filter: ObjectFilter,
        kind: FilterStaticKind,
    },
    Compound(Vec<StaticAbility>),
}

/// Token replacement rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenReplacementKind {
    Multiply(u8),
    Bonus {
        spec: Box<TokenSpec>,
        count: ValueExpr,
    },
    Replace(Box<TokenSpec>),
}

/// Damage replacement rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageReplacement {
    ModifyDynamic {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        add: ValueExpr,
    },
    Cap {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        threshold: i32,
        capped_to: i32,
    },
    UnpreventableBySource(SubjectRef),
    PreventAll {
        target: SubjectRef,
    },
    PreventWithRider {
        target: SubjectRef,
        condition: Option<ConditionExpr>,
        rider: Vec<Effect>,
    },
    PreventAllBut {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        remaining: u16,
    },
    PreventFixed {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        amount: ValueExpr,
    },
    RedirectAll {
        from: SubjectRef,
        to: SubjectRef,
    },
    RedirectNext {
        amount: ValueExpr,
        from: SubjectRef,
        to: SubjectRef,
    },
    ReplaceNext {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        instead: Vec<Effect>,
    },
}

/// Player-targeted replacement rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerReplacementKind {
    SkipStep { step: TurnStepAst, optional: bool },
    SkipExtraTurn,
    LoseLifeMultiply { factor: u8, during_your_turn: bool },
    ModifyLifeGain { add: i16, factor: u8 },
    MultiplyMill { factor: u8 },
}

/// Miscellaneous typed replacement rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacementRuleKind {
    LeaveBattlefieldExileInstead,
    OpeningHandOnBattlefield,
    BecomesDayOnEnter,
    EntersWithChoiceCounter {
        kind1: CounterKind,
        kind2: CounterKind,
    },
    EntersWithKeywords(Vec<KeywordSpec>),
    EscapesWithCounters {
        kind: CounterKind,
        count: ValueExpr,
    },
    ManaOverride(ManaCost),
    LifeFloor(i32),
    ExploreAgain(ObjectFilter),
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
    AsEnters(Vec<Effect>),
    EnterAsCopy {
        optional: bool,
        filter: ObjectFilter,
        exceptions: Vec<CopyException>,
    },
    LoseLifeInsteadOfGain(PlayerRef),
    ModifyDamage {
        source: Option<ObjectFilter>,
        target: Option<SubjectRef>,
        add: i16,
        factor: u8,
    },
    TokenRule {
        controller: PlayerRef,
        filter: Option<ObjectFilter>,
        rule: TokenReplacementKind,
    },
    CounterRule {
        filter: ObjectFilter,
        kind: Option<CounterKind>,
        add: i16,
        factor: u8,
    },
    DamageRule(DamageReplacement),
    PlayerRule {
        player: PlayerRef,
        rule: PlayerReplacementKind,
    },
    Rule(ReplacementRuleKind),
    Compound(Vec<Replacement>),
    ConditionalEnters {
        condition: ConditionExpr,
        inner: Box<Replacement>,
    },
    EventInstead {
        event: TriggerEvent,
        instead: Vec<Effect>,
    },
}

/// Single-player trigger action kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerTriggerKind {
    CardDrawn,
    CardDiscarded,
    LandPlayed,
    CommitCrime,
    ScryOrSurveil,
    Scry,
    Surveil,
    ActivateExhaust,
    CreateOrSacrificeToken,
    SearchesLibrary,
    ExertsCreature,
    CompletesDungeon,
    ShufflesLibrary,
    LosesGame,
    BecomesMonarch,
    IsAttacked,
    WinClash,
    WinCoinFlip,
}

/// Single-subject state-transition trigger kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectTriggerKind {
    AttacksAndNotBlocked,
    BecomesTapped,
    BecomesUntapped,
    TurnedFaceUp,
    Transforms,
    Exploits,
    BecomesMonstrous,
    BecomesCrewed,
    BecomesUnattached,
    DiesOrHauntedDies,
    Mutates,
    CrewsVehicle,
    PhasesIn,
    PhasesOut,
}

/// Player + filtered-object trigger kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerObjectTriggerKind {
    ControlNo,
    TappedForMana,
    CardDiscarded,
    CardPlayed,
    TapOpponentCreature,
    CreatesTokens,
    MillsCard,
}

/// Counter-related trigger event kind (beyond basic `CounterPlaced` / `LastCounterRemoved`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CounterTriggerKind {
    Removed { while_exiled: bool },
    Threshold { min_count: u16 },
    HasNone,
    NthPut { nth: u8 },
}

/// Zero-argument global trigger event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlobalTriggerKind {
    DayNightChanged,
    RingTemptsYou,
    NoCreaturesOnBattlefield,
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
    SpellCopied {
        caster: PlayerRef,
        spell: Option<ObjectFilter>,
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
    StepEnds {
        step: TurnStepAst,
        owner: StepOwner,
    },
    PermanentSacrificed {
        player: PlayerRef,
        subject: SubjectRef,
    },
    LifeChanged {
        player: PlayerRef,
        gained: bool,
    },
    CounterPlaced {
        subject: SubjectRef,
        kind: Option<CounterKind>,
    },
    CounterEvent {
        subject: SubjectRef,
        kind: CounterKind,
        event: CounterTriggerKind,
    },
    LastCounterRemoved {
        subject: SubjectRef,
        kind: CounterKind,
    },
    BecomesTarget {
        subject: SubjectRef,
        source: ObjectFilter,
    },
    Blocks {
        subject: SubjectRef,
        blocked: Option<Box<SubjectRef>>,
    },
    BecomesBlocked {
        subject: SubjectRef,
        by: Option<Box<SubjectRef>>,
    },
    DealtDamage {
        subject: SubjectRef,
        combat_only: bool,
    },
    Cycled {
        player: PlayerRef,
        self_card: bool,
    },
    NthCardDrawn {
        player: PlayerRef,
        nth: u8,
    },
    UnlockDoor {
        player: PlayerRef,
        fully_unlocked: bool,
    },
    AbilityActivated {
        player: PlayerRef,
        non_mana_only: bool,
    },
    AttacksWithOthers {
        subject: Option<SubjectRef>,
        min_count: u8,
    },
    PlayerAttacksWith {
        player: PlayerRef,
        filter: ObjectFilter,
        min_count: u8,
    },
    RolledDie {
        player: PlayerRef,
        value: u8,
    },
    ClassBecomesLevel {
        level: u8,
    },
    LoseControl {
        player: PlayerRef,
        subject: SubjectRef,
    },
    Expend {
        player: PlayerRef,
        amount: u8,
    },
    BecomesAttached {
        subject: SubjectRef,
        target: ObjectFilter,
    },
    ForcedDiscard {
        player: PlayerRef,
        subject: SubjectRef,
    },
    PlayerAction {
        player: PlayerRef,
        action: PlayerTriggerKind,
    },
    SubjectState {
        subject: SubjectRef,
        event: SubjectTriggerKind,
    },
    PlayerObjectEvent {
        player: PlayerRef,
        kind: PlayerObjectTriggerKind,
        filter: Option<ObjectFilter>,
    },
    Global(GlobalTriggerKind),
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

/// Typed payload for a sticker row (`{TK}{TK} — 1/5` or `{TK}{TK}{TK} — Whenever ...`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StickerPayload {
    BasePt(i32, i32),
    Face(Box<CompiledFaceV2>),
}

/// Typed payload for a die-roll table or Station threshold row (`1—9 | ...`, `9+ | ...`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableRowPayload {
    Effects(Vec<Effect>),
    Face(Box<CompiledFaceV2>),
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
    TableRow {
        min: u16,
        max: Option<u16>,
        open_ended: bool,
        payload: TableRowPayload,
    },
    StickerRow {
        ticket_cost: u8,
        payload: StickerPayload,
    },
    CaseSolveCondition(ConditionExpr),
    CaseSolved(Box<CompiledFaceV2>),
}
