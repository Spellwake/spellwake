//! Orthogonal static ability rules unifying cost modifiers, casting permissions,
//! damage/draw/untap rules, restrictions, battlefield modifiers, and Conspiracy command-zone agendas.
use super::{
    BlockingRestriction, CharacteristicDefinitionError, CreatureTypeTextChange,
    DefiningCreatureTypes, DefiningPart, DefiningParts, PermanentSelector, StaticAbilityId,
    StaticPart, StaticParts, StaticSubtypeOperand, StaticTypeChange, SubtypeOperation,
    TypeOperation, validate_battlefield_types, validate_buckets, validate_subtypes,
};
use crate::{
    land::NonlandCardNames,
    mana::ManaType,
    program::{CardType, SpellFilter},
};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::{collections::BTreeSet, num::NonZeroU8, sync::Arc};

/// Magnitude of a generic spell cost reduction granted by a permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellDiscount {
    SourcePlusOneCounters,
    PerExperienceCounter(NonZeroU8),
    Fixed(NonZeroU8),
    ControlledCreaturesWithPlusOneCounters,
}

/// Which kinds of cards a permission lets its controller play from their library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryTopPermission {
    All,
    Land,
    Creature,
    Artifact,
    Nonland,
    InstantOrSorcery,
    ArtifactOrColorlessSpell,
    CreatureOfSourceEntryType,
}

/// Conspiracy command-zone agenda metadata (14 face-up, 14 Hidden Agenda, 1 Double Agenda).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgendaSpec {
    pub hidden: bool,
    pub named_slots: u8,
}

/// Static spell cost modifier or alternative-cost rule.
// Keep SpellFilter inline for value semantics on static rule definitions without extra boxing churn.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellCostModifier {
    /// Reduce generic cost of qualifying spells (either controller's spells or all players' spells).
    Discount {
        any_player: bool,
        matching: SpellFilter,
        amount: SpellDiscount,
    },
    /// Increase spell costs by `amount` except for spells with card type `exempt`.
    IncreaseExceptType { exempt: CardType, amount: NonZeroU8 },
    /// Increase generic cost of qualifying spells (for all players, only opponents, or only controller).
    Increase {
        any_player: bool,
        opponents_only: bool,
        matching: SpellFilter,
        amount: NonZeroU8,
    },
    /// Reduce cost of other spells of `subtype` from battlefield or command zone (eminence).
    SubtypeDiscountFromCommandOrBattlefield {
        subtype: CreatureTypeId,
        amount: NonZeroU8,
    },
    /// Spells of the source's chosen creature type cost one less of each color.
    ChosenTypeOneLessOfEachColor,
    /// Controller may pay zero instead of mana cost for creature spells of `subtype`.
    FreeSubtypeCreatureSpells(CreatureTypeId),
    /// Once per turn, controller may cast a colorless spell from hand without paying its mana cost.
    FreeColorlessHandSpellOnce,
    /// Once on controller's turn, controller may pay life instead of mana for an enchantment spell.
    PayLifeForEnchantmentOwnTurnOnce,
    /// Nonartifact spells controller casts have improvise.
    NonartifactSpellsHaveImprovise,
    /// Spells with mana value at least `minimum` cost `reduction` less and cannot be countered.
    HighManaValueProtected {
        minimum: NonZeroU8,
        reduction: NonZeroU8,
    },
}

/// Static casting, library-top, graveyard-cast, and land-play permission rule.
// Keep SpellFilter inline for value semantics on static permission rules.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastPermissionRule {
    /// Cast qualifying spells as though they had flash.
    Flash {
        any_player: bool,
        matching: SpellFilter,
    },
    /// Continuously reveal top library card publicly or privately to controller.
    SeeLibraryTop { public: bool },
    /// Every player plays with the top card of their library revealed (Field of Dreams, Lantern of Insight).
    AllPlayersRevealLibraryTop,
    /// Players play with their hands revealed (`opponents_only`: Telepathy; false: Revelation / Wandering Eye).
    RevealHands { opponents_only: bool },
    /// Play qualifying cards from the top of controller's library.
    PlayLibraryTop(LibraryTopPermission),
    /// Play qualifying cards matching an orthogonal object filter from the top of controller's library.
    PlayLibraryTopMatching {
        lands_allowed: bool,
        filter: Box<crate::program::ObjectFilter>,
    },
    /// Once per turn, cast a creature spell from controller's graveyard.
    GraveyardCreatureCast,
    /// Play land cards from controller's graveyard (Crucible of Worlds).
    GraveyardLandPlay,
    /// Creature cards of `subtype` in controller's graveyard have encore.
    GraveyardSubtypeEncore(CreatureTypeId),
    /// Spells of `subtype` cast by controller have replicate.
    SubtypeSpellsHaveReplicate(CreatureTypeId),
    /// Colorless spells cast from hand with mana value >= `minimum_mana_value` have `count` cascade instances.
    ColorlessHandSpellsHaveCascade {
        minimum_mana_value: u16,
        count: NonZeroU8,
    },
    /// Additional land plays available to controller each turn.
    AdditionalLandPlays(NonZeroU8),
    /// Additional land plays available to each player on their turn (Rites of Flourishing, Ghirapur Orrery).
    AllPlayersAdditionalLandPlays(NonZeroU8),
    /// Controlled spells with any listed card type cannot be countered.
    ControllerSpellsCannotBeCountered(BTreeSet<CardType>),
    /// Controlled spells matching `SpellFilter` cannot be countered.
    ControllerSpellsCannotBeCounteredFiltered(Box<SpellFilter>),
    /// Spells matching `SpellFilter` cast by any player cannot be countered (Leyline of Lifeforce, Gaea's Herald).
    AllSpellsCannotBeCounteredFiltered(Box<SpellFilter>),
    /// Controller may look at face-down cards exiled with this source permanent (Bane Alley Broker, Grimoire Thief).
    LookAtSourceExiledCards,
    /// Cast qualifying spells (and optionally play lands) matching `filter` from controller's graveyard.
    GraveyardMatching {
        lands_allowed: bool,
        filter: Box<crate::program::ObjectFilter>,
        once_each_turn: bool,
    },
    /// Self-permission to cast this card from its owner's graveyard when `condition` holds.
    GraveyardSelfConditional {
        condition: Box<crate::program::ResolutionCondition>,
    },
    /// Cast qualifying cards matching `filter` from exile (optionally linked to this source).
    ExileMatching {
        filter: Box<crate::program::ObjectFilter>,
        linked_to_source: bool,
        free: bool,
        any_color: bool,
    },
}

/// Which damage recipients a generalized [`StaticDamageRule::ModifyDamage`] matches.
// Kept inline so `StaticDamageRecipient` remains `Copy` alongside zero-allocation `ObjectFilter`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticDamageRecipient {
    Any,
    Opponents,
    OpponentOrOpponentPermanent,
    Controller,
    ControllerOrControlledPermanent,
    ControllerOrControlledCreatures,
    ControlledCreatures,
    Creatures,
    OpponentCreatures,
    Players,
    SelfObject,
    AttachedHost,
    /// Single targeted permanent captured when this temporary rule was created.
    TargetObject,
    /// Single targeted permanent or player captured when this temporary rule was created.
    TargetAny,
    MatchingPermanent(crate::program::ObjectFilter),
}

/// Whether a generalized [`StaticDamageRule::ModifyDamage`] matches combat, noncombat, or any damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticDamageKind {
    Any,
    CombatOnly,
    NoncombatOnly,
}

/// Replacement operation applied by [`StaticDamageRule::ModifyDamage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticDamageOp {
    Add(u8),
    /// Dynamic additive damage modifier evaluated from world state (CR 614.1, e.g. Fated Firepower).
    AddExpr(Box<crate::program::ValueExpr>),
    Subtract(u8),
    CapAt(u8),
    Multiply(u8),
    PreventAll,
    PreventUpTo(u8),
    /// Prevent all damage from an attempt if its remaining amount is at most `max`.
    PreventAtMost(u8),
    /// Prevent half of an attempt's remaining damage (rounded up when `round_up` is true).
    PreventHalf {
        round_up: bool,
    },
    ReplaceWithMinusOneCounters,
}

/// Destination recipient for generalized damage redirection (CR 614.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageRedirectTarget {
    AbilitySource,
    AttachedHost,
    Controller,
    TargetObject,
    /// Single targeted permanent or player captured when this temporary rule was created.
    TargetAny,
    /// Second targeted permanent or player in a multi-target spell or ability.
    SecondTargetObject,
    DamageSourceController,
}

/// Static damage prevention, doubling, replacement, or lethality rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticDamageRule {
    /// Prevent noncombat damage to controller and other controlled permanents.
    PreventNoncombat,
    /// Prevent all combat damage dealt by or to source.
    PreventSourceCombat,
    /// Prevent all combat damage that would be dealt to source (incoming only).
    PreventIncomingCombatToSource,
    /// Prevent combat damage to attacking artifact creatures controlled by source's controller.
    PreventCombatToAttackingArtifacts,
    /// Prevent damage dealt by (and if `combat_only`, dealt to) enchanted creature.
    PreventEnchantedCreature { combat_only: bool },
    /// Prevent combat damage to source and exile that many cards from controller's library.
    PreventIncomingCombatAndExile,
    /// Prevent incoming damage to source by removing one +1/+1 counter (Phantom cycle).
    PreventIncomingRemovePlusOne,
    /// Prevent incoming damage to source only while it has at least one +1/+1 counter, removing one (Oathsworn Knight).
    PreventIncomingRemovePlusOneIfCounter,
    /// Prevent incoming damage to source by removing that many +1/+1 counters.
    PreventIncomingRemoveNPlusOne,
    /// Redirect damage that would be dealt to controller (and optionally other controlled permanents) to source.
    RedirectPlayerDamageToSource { include_other_permanents: bool },
    /// Redirect damage that would be dealt to controller to the permanent this Aura/Equipment is attached to.
    RedirectPlayerDamageToEnchanted,
    /// Damage that would reduce controller's life total below `floor` reduces it to `floor` instead.
    MinimumLifeTotal { floor: u8, require_creature: bool },
    /// Prevent half of incoming damage rounded up.
    PreventHalfIncoming,
    /// Double damage dealt to opponents.
    DoubleToOpponents,
    /// Double combat damage dealt by equipped creature.
    DoubleEquippedCombat,
    /// Double damage dealt to enchanted player.
    DoubleToEnchantedPlayer,
    /// Replace combat damage dealt by controlled `CreatureTypeId` to a player with mill.
    ReplaceSubtypeCombatWithMill(CreatureTypeId),
    /// Positive marked damage is lethal against power instead of toughness for controlled creatures.
    LethalUsesPower,
    /// Damage can't be prevented while this permanent is on the battlefield.
    DamageCannotBePrevented,
    /// Generalized static damage replacement or modification rule (CR 614.1, 616.1).
    ModifyDamage {
        source: Option<crate::program::ObjectFilter>,
        source_controller: crate::program::PlayerRelation,
        recipient: StaticDamageRecipient,
        kind: StaticDamageKind,
        op: StaticDamageOp,
    },
    /// Generalized static damage redirection rule (CR 614.9).
    RedirectDamage {
        source: Option<crate::program::ObjectFilter>,
        from: StaticDamageRecipient,
        to: DamageRedirectTarget,
        combat_only: bool,
    },
}

/// Event kind intercepted by [`StaticContinuousAbility::EventReplacement`] (CR 614.1).
// Kept inline so `ReplaceableEvent` remains `Copy` alongside zero-allocation `ObjectFilter`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceableEvent {
    Draw {
        player: crate::program::AffectedPlayers,
    },
    LifeGain {
        player: crate::program::AffectedPlayers,
    },
    LifeLoss {
        player: crate::program::AffectedPlayers,
    },
    Mill {
        player: crate::program::AffectedPlayers,
    },
    Explore {
        filter: crate::program::ObjectFilter,
    },
    Scry {
        player: crate::program::AffectedPlayers,
    },
    ExtraTurn {
        player: crate::program::AffectedPlayers,
    },
}

/// Replacement transformation applied by [`StaticContinuousAbility::EventReplacement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticReplacementAction {
    /// Modify the numeric count of the event: `(raw + add) * multiply`.
    ModifyAmount {
        add: Box<crate::program::ValueExpr>,
        multiply: u8,
    },
    /// Skip / prevent the event entirely (e.g. skip extra turn, prevent life loss).
    Skip,
    /// Replace the event with an executable body (optionally prompting the affected player).
    ReplaceWithBody {
        optional: bool,
        body: Box<crate::program::ExecutableBody>,
    },
}

/// Static draw replacement, draw limit, or hand-size rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticDrawRule {
    OpponentsAtMostOne,
    EmptyDrawWins,
    /// Instead of drawing from an empty library, put `amount` counters of `kind` on source.
    EmptyDrawPutCounters {
        kind: crate::effects::CounterKind,
        amount: u8,
    },
    /// Modify the number of cards controller draws (Teferi's Ageless Insight, Thought Reflection, Phial of Galadriel).
    ModifyDraw {
        add: u8,
        multiply: u8,
        skip_first_draw_step: bool,
        require_empty_hand: bool,
    },
    SplitOpponentsMultipleDraw,
    ReplaceWithLandKind,
    SkipControllerDrawStep,
    PlayersCannotDraw,
    OpponentsCannotDraw,
    NoMaximumHandSize,
    AllPlayersNoMaximumHandSize,
    /// Controller's maximum hand size is reduced/set to this value (Null Profusion, Recycle).
    MaxHandSize(u8),
}

/// Static untap-step modification rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticUntapRule {
    UntapDuringOtherPlayersUntap,
    /// Untap all permanents matching `selector` during each other player's untap step (Seedborn Muse).
    UntapMatchingDuringOtherPlayersUntap(Box<PermanentSelector>),
    EnchantedCreatureUntapsOnlyForMonarch,
    EnchantedCreatureSkipsUntap,
    SourceSkipsUntap,
    /// CR 502.3: Controller may choose not to untap this permanent during their untap step.
    MayChooseNotToUntapSource,
}

/// Static prohibition, cast limit, targeting restriction, or transform restriction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticRestriction {
    PreventLoyaltyActivations,
    PreventArtifactActivations,
    PreventControlledTransform {
        subtype: CreatureTypeId,
        except: CreatureTypeId,
    },
    ProhibitLifeAndCreatureCosts,
    ProhibitOpponentsCausingSacrifice,
    OpponentsCannotGainLife,
    PlayersCannotGainLife,
    OthersEnterTapped(Box<PermanentSelector>),
    PreventMatchingActivations(Box<PermanentSelector>),
    RestrictOpponentsDuringOwnTurn,
    EnchantedPlayerCastLimit,
    ProhibitCastingChosenName(Arc<NonlandCardNames>),
    RestrictGraveyardAndLibrary,
    CombatRequiresCityBlessing,
    ControllerHasHexproof,
    /// Controller cannot be the target of spells or abilities from any player (CR 702.18, Ivory Mask).
    ControllerHasShroud,
    /// Controller cannot lose the game and opponents cannot win the game (Platinum Angel).
    ControllerCantLoseOpponentsCantWin,
    /// Controller does not lose the game for having 0 or less life (Phyrexian Unlife, Lich's Mastery).
    ControllerCantLoseForZeroLife,
    /// Controller cannot win the game and opponents cannot lose the game (Abyssal Persecutor).
    ControllerCantWinOpponentsCantLose,
    /// Players matching this relation cannot play lands.
    CantPlayLands(crate::program::PlayerRelation),
    /// Counters (optionally restricted to `counter_kind`) cannot be placed on matching permanents and/or players (Solemnity, Melira).
    PreventCounters {
        selector: Option<Box<PermanentSelector>>,
        players: Option<crate::program::PlayerRelation>,
        counter_kind: Option<crate::effects::CounterKind>,
    },
    /// Each player can't cast more than one spell each turn (CR 101.2 / Rule of Law).
    OneSpellPerTurn,
    /// Creatures entering the battlefield don't cause abilities to trigger (Torpor Orb).
    SuppressEntersTriggers,
    /// Players matching `player` cannot cast spells matching `filter`.
    ProhibitCasting {
        player: crate::program::PlayerRelation,
        filter: Box<SpellFilter>,
    },
    /// Matching permanents enter the battlefield untapped.
    EnterUntapped(Box<PermanentSelector>),
    /// Matching permanents cannot enter the battlefield from non-cast sources.
    ProhibitEntering(Box<PermanentSelector>),
    /// Controller may cast this spell as though it had flash; if cast any time a sorcery couldn't be cast, sacrifice it at the beginning of the next cleanup step.
    FlashSacrificeAtCleanup,
    /// Permanents matching `selector` can be targeted by the source controller as though they didn't have hexproof (Glaring Spotlight, Shadowspear).
    IgnoreOpponentsHexproof(Box<PermanentSelector>),
    /// Controller may activate abilities of matching creatures as though those creatures had haste (Thousand-Year Elixir, Tyvar).
    ActivateAbilitiesAsThoughHaste(Box<PermanentSelector>),
    /// Activated abilities of matching permanents can't be activated unless they're mana abilities (Faith's Fetters, Damping Matrix).
    PreventMatchingNonManaActivations(Box<PermanentSelector>),
    /// Cards in graveyards can't be the targets of spells or abilities (Ground Seal, Silent Gravestone).
    PreventGraveyardTargeting,
    /// Matching permanents can't be sacrificed (Hithlain Rope, Alexios, Assault Suit).
    CannotBeSacrificed(Box<PermanentSelector>),
    /// Matching permanents can't become untapped by untap steps or effects (Blossombind, Frozen in Ice).
    CannotBeUntapped(Box<PermanentSelector>),
    /// Matching permanents can't be the targets of Aura spells and/or spells or abilities of the specified colors (Bartel Runeaxe, Raiding Party).
    CantBeTargetedBy {
        selector: Box<PermanentSelector>,
        auras_only: bool,
        colors: crate::program::ColorMask,
    },
    /// Source controller can't cast more than one spell each turn (Moderation).
    ControllerOneSpellPerTurn,
    /// Opponents of source controller can't cast spells during combat (Marisi).
    OpponentsCantCastDuringCombat,
    /// Source controller's life total can't change (Platinum Emperion, Teferi's Protection, Flare of Fortitude).
    ControllerLifeTotalCantChange,
    /// Players matching this relation can't search libraries (Stranglehold, Ashiok, Shadow of Doubt).
    PlayersCantSearchLibraries(crate::program::PlayerRelation),
}

/// Static battlefield replacement, doubling, mana retention, or structural modifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticModifierRule {
    DoubleCreatedTokens,
    DoublePlacedCounters,
    AdditionalSelfPlusOneCounter,
    OtherCreaturesEnterWithSourceCounters,
    /// Matching permanents enter with `count` additional `kind` counters (Good-Fortune Unicorn, Grumgully).
    OthersEnterWithCounters {
        selector: Box<PermanentSelector>,
        kind: crate::effects::CounterKind,
        count: u8,
    },
    /// Matching permanents enter with a dynamic world-evaluable number of additional `kind` counters.
    OthersEnterWithCountersExpr {
        selector: Box<PermanentSelector>,
        kind: crate::effects::CounterKind,
        amount: Box<crate::program::ValueExpr>,
    },
    /// Modify counters placed on matching permanents (Hardened Scales, Benevolent Hydra, Branching Evolution).
    ModifyCounters {
        selector: Box<PermanentSelector>,
        counter_kind: Option<crate::effects::CounterKind>,
        add: u8,
        multiply: u8,
    },
    /// Modify the number of tokens created by players matching `controller` (Anointed Procession, Ojer Taq, Primal Vigor).
    ModifyTokens {
        controller: crate::program::PlayerRelation,
        add: u8,
        multiply: u8,
    },
    /// Modify the number of tokens with `subtype` created by players matching `controller` (Xorn, Peregrin Took).
    ModifyTokenSubtype {
        subtype: crate::program::ArtifactSubtype,
        controller: crate::program::PlayerRelation,
        add: u8,
        multiply: u8,
    },
    /// Whenever matching players would create one or more tokens, also create `count` bonus tokens (or `count` per created token when `per_created` is true; Peregrin Took, Chatterfang).
    BonusTokenOnCreate {
        controller: crate::program::PlayerRelation,
        token: Box<crate::token::TokenDefinition>,
        count: u8,
        per_created: bool,
    },
    /// Halve (rounded down) counters placed on opponents or permanents opponents control (Vorinclex).
    HalveOpponentCounters,
    /// Modify or invert life gained by players matching `player` (Boon Reflection, Angel of Vitality, Tainted Remedy).
    ModifyLifeGain {
        player: crate::program::PlayerRelation,
        add: u8,
        multiply: u8,
        lose_instead: bool,
    },
    /// Conditionally modify life gained by players matching `player` (Phial of Galadriel).
    ConditionalModifyLifeGain {
        condition: Box<crate::program::ResolutionCondition>,
        player: crate::program::PlayerRelation,
        add: u8,
        multiply: u8,
    },
    /// If it is neither day nor night as this permanent enters / is on battlefield, it becomes day.
    BecomesDayIfNeither,
    ExtraEntryLoyalty,
    TwiceLoyaltyPerTurn,
    AdditionalCreatureDeathTrigger,
    /// Generalized CR 603.2d trigger multiplier (Panharmonicon, Teysa, Isshin, Yarok, Roaming Throne, etc.).
    AdditionalTrigger(Box<TriggerMultiplierRule>),
    EnterControlledCreaturesAsEnchantedCopy,
    EnterControlledCreaturesAsSourceCopy,
    IgnoreLegendForSubtype(CreatureTypeId),
    /// The "legend rule" doesn't apply to permanents any player controls (Mirror Gallery).
    IgnoreLegendRule,
    /// The "legend rule" doesn't apply to permanents this permanent's controller controls.
    IgnoreLegendRuleForController,
    RetainMana(ManaType),
    Ascend,
    AdditionalBlockWhileMonarch,
}

/// Event predicate matched by a generalized trigger multiplier (CR 603.2d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerMultiplierEvent {
    /// Any non-mana triggered ability of a matching permanent (Roaming Throne, Delney, Annie Joins Up).
    Any,
    /// A permanent matching `ObjectFilter` enters the battlefield (Panharmonicon, Yarok, Elesh Norn MoM).
    Enters(crate::program::ObjectFilter),
    /// A permanent matching `ObjectFilter` dies (Teysa Karlov, Drivnod).
    Dies(crate::program::ObjectFilter),
    /// A permanent matching `ObjectFilter` leaves the battlefield (Gandalf the White).
    LeavesBattlefield(crate::program::ObjectFilter),
    /// A creature matching `ObjectFilter` attacks (Isshin, Wulfgar).
    Attacks(crate::program::ObjectFilter),
    /// Casting or copying a spell matching `SpellFilter` (Veyran).
    SpellCastOrCopy(crate::program::SpellFilter),
    /// Dealing combat or any damage (Felix Five-Boots).
    DealsDamage {
        combat_only: bool,
        to_player_only: bool,
    },
    /// Disjunction of multiplier cause events (Gandalf the White: enters OR leaves).
    AnyOf(Vec<TriggerMultiplierEvent>),
}

impl TriggerMultiplierEvent {
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        match self {
            Self::Any | Self::DealsDamage { .. } => {}
            Self::Enters(f) | Self::Dies(f) | Self::LeavesBattlefield(f) | Self::Attacks(f) => {
                f.rewrite_creature_type(from, to)
            }
            Self::SpellCastOrCopy(f) => f.rewrite_creature_type(from, to),
            Self::AnyOf(items) => {
                for item in items {
                    item.rewrite_creature_type(from, to);
                }
            }
        }
    }

    pub(crate) fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), CharacteristicDefinitionError> {
        match self {
            Self::Any | Self::DealsDamage { .. } => Ok(()),
            Self::Enters(f) | Self::Dies(f) | Self::LeavesBattlefield(f) | Self::Attacks(f) => f
                .validate_vocabulary(vocabulary)
                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType),
            Self::SpellCastOrCopy(f) => f
                .validate_vocabulary(vocabulary)
                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType),
            Self::AnyOf(items) => {
                if items.is_empty() {
                    return Err(CharacteristicDefinitionError::EmptyParts);
                }
                for item in items {
                    item.validate(vocabulary)?;
                }
                Ok(())
            }
        }
    }
}

/// Orthogonal CR 603.2d trigger multiplier specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerMultiplierRule {
    /// Additive extra triggers (typically 1 for "triggers an additional time").
    pub additional: u8,
    /// Cause event that must trigger the ability.
    pub event: TriggerMultiplierEvent,
    /// Optional filter on the permanent whose ability triggered.
    pub triggering_source: Option<PermanentSelector>,
    /// True for "another triggered ability" / non-self (Roaming Throne, Harmonic Prodigy).
    pub exclude_self: bool,
    /// True for death-trigger doublers (Teysa Karlov, Drivnod) where sacrifice triggers do not double.
    pub exclude_sacrifice_triggers: bool,
}

/// Orthogonal static continuous ability replacing the flat 69-variant enum.
// `DamageRule` and `EventReplacement` store `Copy` filter specs inline; kept unboxed to preserve existing value ownership.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticContinuousAbility {
    /// Ordinary continuous effects generated by a battlefield source.
    Battlefield {
        id: StaticAbilityId,
        selector: PermanentSelector,
        parts: StaticParts,
    },
    /// Printed self-only definitions in every represented physical zone (CR 604.3a, 613.6).
    CharacteristicDefining {
        id: StaticAbilityId,
        parts: DefiningParts,
    },
    /// Spell cost discounts, taxes, and alternative-cost permissions.
    CostModifier {
        id: StaticAbilityId,
        modifier: SpellCostModifier,
    },
    /// Flash timing, library-top access, graveyard casting, extra land plays, and uncounterability.
    CastPermission {
        id: StaticAbilityId,
        permission: CastPermissionRule,
    },
    /// Static damage prevention, doubling, replacement, and lethality rules.
    DamageRule {
        id: StaticAbilityId,
        rule: StaticDamageRule,
    },
    /// Static draw replacements, limits, skips, and hand-size overrides.
    DrawRule {
        id: StaticAbilityId,
        rule: StaticDrawRule,
    },
    /// Static untap-step overrides.
    UntapRule {
        id: StaticAbilityId,
        rule: StaticUntapRule,
    },
    /// Static activation, casting, targeting, life-gain, and combat prohibitions.
    Restriction {
        id: StaticAbilityId,
        rule: StaticRestriction,
    },
    /// Static token/counter/loyalty/death-trigger/legend/mana/copy/ascend modifiers.
    Modifier {
        id: StaticAbilityId,
        rule: StaticModifierRule,
    },
    /// Conspiracy command-zone active ability with optional hidden named card slots.
    CommandZoneActive {
        id: StaticAbilityId,
        agenda: AgendaSpec,
    },
    /// Generalized event replacement rule (CR 614.1a: draw, life gain/loss, mill, explore, scry, extra turns).
    EventReplacement {
        id: StaticAbilityId,
        event: ReplaceableEvent,
        condition: Option<Box<crate::characteristics::StaticSourceCondition>>,
        action: StaticReplacementAction,
    },
}

impl StaticContinuousAbility {
    /// The printed identity is independent of the effect's operating zone.
    #[must_use]
    pub fn id(&self) -> StaticAbilityId {
        match self {
            Self::Battlefield { id, .. }
            | Self::CharacteristicDefining { id, .. }
            | Self::CostModifier { id, .. }
            | Self::CastPermission { id, .. }
            | Self::DamageRule { id, .. }
            | Self::DrawRule { id, .. }
            | Self::UntapRule { id, .. }
            | Self::Restriction { id, .. }
            | Self::Modifier { id, .. }
            | Self::CommandZoneActive { id, .. }
            | Self::EventReplacement { id, .. } => *id,
        }
    }

    /// Assign an identity within the combined text of a merged permanent.
    pub(crate) fn with_component_id(mut self, new_id: StaticAbilityId) -> Self {
        match &mut self {
            Self::Battlefield { id, .. }
            | Self::CharacteristicDefining { id, .. }
            | Self::CostModifier { id, .. }
            | Self::CastPermission { id, .. }
            | Self::DamageRule { id, .. }
            | Self::DrawRule { id, .. }
            | Self::UntapRule { id, .. }
            | Self::Restriction { id, .. }
            | Self::Modifier { id, .. }
            | Self::CommandZoneActive { id, .. }
            | Self::EventReplacement { id, .. } => *id = new_id,
        }
        self
    }

    /// Whether this static ability requires a permanent program definition (rejected on instant/sorcery spells).
    #[must_use]
    pub(crate) fn requires_permanent(&self) -> bool {
        match self {
            Self::CharacteristicDefining { .. } | Self::CommandZoneActive { .. } => false,
            Self::Battlefield { .. }
            | Self::UntapRule { .. }
            | Self::DamageRule { .. }
            | Self::EventReplacement { .. } => true,
            Self::CostModifier { modifier, .. } => !matches!(
                modifier,
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { .. }
                    | SpellCostModifier::HighManaValueProtected { .. }
            ),
            Self::CastPermission { permission, .. } => !matches!(
                permission,
                CastPermissionRule::GraveyardCreatureCast
                    | CastPermissionRule::GraveyardSelfConditional { .. }
                    | CastPermissionRule::ColorlessHandSpellsHaveCascade { .. }
                    | CastPermissionRule::ControllerSpellsCannotBeCountered(_)
                    | CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(_)
                    | CastPermissionRule::AllSpellsCannotBeCounteredFiltered(_)
            ),
            Self::DrawRule { rule, .. } => matches!(
                rule,
                StaticDrawRule::NoMaximumHandSize
                    | StaticDrawRule::AllPlayersNoMaximumHandSize
                    | StaticDrawRule::MaxHandSize(_)
            ),
            Self::Restriction { rule, .. } => !matches!(
                rule,
                StaticRestriction::ProhibitLifeAndCreatureCosts
                    | StaticRestriction::ProhibitOpponentsCausingSacrifice
            ),
            Self::Modifier { rule, .. } => {
                !matches!(rule, StaticModifierRule::AdditionalBlockWhileMonarch)
            }
        }
    }

    /// Whether this static ability is admitted in `Program::additional_battlefield_abilities`.
    #[must_use]
    pub fn allowed_as_additional(&self) -> bool {
        matches!(
            self,
            Self::Battlefield { .. }
                | Self::CostModifier { .. }
                | Self::CastPermission {
                    permission: CastPermissionRule::Flash { .. }
                        | CastPermissionRule::GraveyardCreatureCast
                        | CastPermissionRule::GraveyardLandPlay
                        | CastPermissionRule::SubtypeSpellsHaveReplicate(_)
                        | CastPermissionRule::GraveyardSubtypeEncore(_)
                        | CastPermissionRule::SeeLibraryTop { .. }
                        | CastPermissionRule::AllPlayersRevealLibraryTop
                        | CastPermissionRule::RevealHands { .. }
                        | CastPermissionRule::PlayLibraryTop(_)
                        | CastPermissionRule::PlayLibraryTopMatching { .. }
                        | CastPermissionRule::AdditionalLandPlays(_)
                        | CastPermissionRule::AllPlayersAdditionalLandPlays(_)
                        | CastPermissionRule::ControllerSpellsCannotBeCountered(_)
                        | CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(_)
                        | CastPermissionRule::AllSpellsCannotBeCounteredFiltered(_)
                        | CastPermissionRule::LookAtSourceExiledCards
                        | CastPermissionRule::GraveyardMatching { .. }
                        | CastPermissionRule::GraveyardSelfConditional { .. }
                        | CastPermissionRule::ExileMatching { .. },
                    ..
                }
                | Self::DamageRule { .. }
                | Self::EventReplacement { .. }
                | Self::DrawRule {
                    rule: StaticDrawRule::NoMaximumHandSize
                        | StaticDrawRule::AllPlayersNoMaximumHandSize
                        | StaticDrawRule::MaxHandSize(_)
                        | StaticDrawRule::OpponentsAtMostOne
                        | StaticDrawRule::SkipControllerDrawStep
                        | StaticDrawRule::ModifyDraw { .. }
                        | StaticDrawRule::EmptyDrawPutCounters { .. }
                        | StaticDrawRule::PlayersCannotDraw
                        | StaticDrawRule::OpponentsCannotDraw,
                    ..
                }
                | Self::UntapRule { .. }
                | Self::Restriction {
                    rule: StaticRestriction::CombatRequiresCityBlessing
                        | StaticRestriction::PreventLoyaltyActivations
                        | StaticRestriction::PreventArtifactActivations
                        | StaticRestriction::PreventControlledTransform { .. }
                        | StaticRestriction::ProhibitLifeAndCreatureCosts
                        | StaticRestriction::ProhibitOpponentsCausingSacrifice
                        | StaticRestriction::OpponentsCannotGainLife
                        | StaticRestriction::PlayersCannotGainLife
                        | StaticRestriction::OthersEnterTapped(_)
                        | StaticRestriction::PreventMatchingActivations(_)
                        | StaticRestriction::PreventMatchingNonManaActivations(_)
                        | StaticRestriction::IgnoreOpponentsHexproof(_)
                        | StaticRestriction::ActivateAbilitiesAsThoughHaste(_)
                        | StaticRestriction::PreventGraveyardTargeting
                        | StaticRestriction::CannotBeSacrificed(_)
                        | StaticRestriction::CannotBeUntapped(_)
                        | StaticRestriction::CantBeTargetedBy { .. }
                        | StaticRestriction::ControllerOneSpellPerTurn
                        | StaticRestriction::OpponentsCantCastDuringCombat
                        | StaticRestriction::ControllerLifeTotalCantChange
                        | StaticRestriction::PlayersCantSearchLibraries(_)
                        | StaticRestriction::RestrictOpponentsDuringOwnTurn
                        | StaticRestriction::RestrictGraveyardAndLibrary
                        | StaticRestriction::ControllerHasHexproof
                        | StaticRestriction::ControllerHasShroud
                        | StaticRestriction::ControllerCantLoseOpponentsCantWin
                        | StaticRestriction::ControllerCantLoseForZeroLife
                        | StaticRestriction::ControllerCantWinOpponentsCantLose
                        | StaticRestriction::CantPlayLands(_)
                        | StaticRestriction::PreventCounters { .. }
                        | StaticRestriction::OneSpellPerTurn
                        | StaticRestriction::SuppressEntersTriggers
                        | StaticRestriction::ProhibitCasting { .. }
                        | StaticRestriction::EnterUntapped(_)
                        | StaticRestriction::ProhibitEntering(_)
                        | StaticRestriction::FlashSacrificeAtCleanup,
                    ..
                }
                | Self::Modifier {
                    rule: StaticModifierRule::IgnoreLegendForSubtype(_)
                        | StaticModifierRule::IgnoreLegendRule
                        | StaticModifierRule::IgnoreLegendRuleForController
                        | StaticModifierRule::Ascend
                        | StaticModifierRule::DoublePlacedCounters
                        | StaticModifierRule::DoubleCreatedTokens
                        | StaticModifierRule::AdditionalSelfPlusOneCounter
                        | StaticModifierRule::OtherCreaturesEnterWithSourceCounters
                        | StaticModifierRule::OthersEnterWithCounters { .. }
                        | StaticModifierRule::OthersEnterWithCountersExpr { .. }
                        | StaticModifierRule::ModifyCounters { .. }
                        | StaticModifierRule::HalveOpponentCounters
                        | StaticModifierRule::ModifyTokens { .. }
                        | StaticModifierRule::ModifyTokenSubtype { .. }
                        | StaticModifierRule::BonusTokenOnCreate { .. }
                        | StaticModifierRule::ModifyLifeGain { .. }
                        | StaticModifierRule::ConditionalModifyLifeGain { .. }
                        | StaticModifierRule::RetainMana(_)
                        | StaticModifierRule::BecomesDayIfNeither
                        | StaticModifierRule::ExtraEntryLoyalty
                        | StaticModifierRule::AdditionalCreatureDeathTrigger
                        | StaticModifierRule::AdditionalTrigger(_),
                    ..
                }
        )
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type_in_rule(&mut self, change: CreatureTypeTextChange) {
        let word = |kind: &mut CreatureTypeId| {
            if *kind == change.from {
                *kind = change.to;
            }
        };
        match self {
            Self::CostModifier { modifier, .. } => match modifier {
                SpellCostModifier::Discount { matching, .. } => {
                    matching.rewrite_creature_type(change.from, change.to);
                }
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, .. }
                | SpellCostModifier::FreeSubtypeCreatureSpells(subtype) => word(subtype),
                _ => {}
            },
            Self::CastPermission { permission, .. } => match permission {
                CastPermissionRule::Flash { matching, .. } => {
                    matching.rewrite_creature_type(change.from, change.to);
                }
                CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(matching)
                | CastPermissionRule::AllSpellsCannotBeCounteredFiltered(matching) => {
                    matching.rewrite_creature_type(change.from, change.to);
                }
                CastPermissionRule::PlayLibraryTopMatching { filter, .. }
                | CastPermissionRule::GraveyardMatching { filter, .. }
                | CastPermissionRule::ExileMatching { filter, .. } => {
                    filter.rewrite_creature_type(change.from, change.to);
                }
                CastPermissionRule::GraveyardSelfConditional { condition } => {
                    condition.rewrite_creature_type(change.from, change.to);
                }
                CastPermissionRule::SubtypeSpellsHaveReplicate(subtype)
                | CastPermissionRule::GraveyardSubtypeEncore(subtype) => word(subtype),
                _ => {}
            },
            Self::DamageRule { rule, .. } => match rule {
                StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype) => word(subtype),
                StaticDamageRule::ModifyDamage {
                    source,
                    recipient,
                    op,
                    ..
                } => {
                    if let Some(filter) = source {
                        filter.rewrite_creature_type(change.from, change.to);
                    }
                    if let StaticDamageRecipient::MatchingPermanent(filter) = recipient {
                        filter.rewrite_creature_type(change.from, change.to);
                    }
                    if let StaticDamageOp::AddExpr(expr) = op {
                        expr.rewrite_creature_type(change.from, change.to);
                    }
                }
                StaticDamageRule::RedirectDamage { source, from, .. } => {
                    if let Some(filter) = source {
                        filter.rewrite_creature_type(change.from, change.to);
                    }
                    if let StaticDamageRecipient::MatchingPermanent(filter) = from {
                        filter.rewrite_creature_type(change.from, change.to);
                    }
                }
                _ => {}
            },
            Self::EventReplacement {
                event,
                condition,
                action,
                ..
            } => {
                if let Some(cond) = condition {
                    crate::characteristics::text::static_source_condition(change, cond);
                }
                if let ReplaceableEvent::Explore { filter } = event {
                    filter.rewrite_creature_type(change.from, change.to);
                }
                match action {
                    StaticReplacementAction::ModifyAmount { add, .. } => {
                        add.rewrite_creature_type(change.from, change.to);
                    }
                    StaticReplacementAction::ReplaceWithBody { body, .. } => {
                        crate::characteristics::text::rewrite_executable_body(body, change);
                    }
                    StaticReplacementAction::Skip => {}
                }
            }
            Self::Restriction { rule, .. } => match rule {
                StaticRestriction::PreventControlledTransform { subtype, except } => {
                    word(subtype);
                    word(except);
                }
                StaticRestriction::ProhibitCasting { filter, .. } => {
                    filter.rewrite_creature_type(change.from, change.to);
                }
                StaticRestriction::OthersEnterTapped(sel)
                | StaticRestriction::PreventMatchingActivations(sel)
                | StaticRestriction::PreventMatchingNonManaActivations(sel)
                | StaticRestriction::IgnoreOpponentsHexproof(sel)
                | StaticRestriction::ActivateAbilitiesAsThoughHaste(sel)
                | StaticRestriction::CannotBeSacrificed(sel)
                | StaticRestriction::CannotBeUntapped(sel)
                | StaticRestriction::CantBeTargetedBy { selector: sel, .. }
                | StaticRestriction::EnterUntapped(sel)
                | StaticRestriction::ProhibitEntering(sel) => {
                    if let Some(st) = &mut sel.creature_subtype {
                        word(st);
                    }
                    if let Some(st) = &mut sel.forbidden_creature_subtype {
                        word(st);
                    }
                    if sel.any_creature_subtypes.remove(&change.from) {
                        sel.any_creature_subtypes.insert(change.to);
                    }
                }
                _ => {}
            },
            Self::Modifier { rule, .. } => match rule {
                StaticModifierRule::IgnoreLegendForSubtype(subtype) => word(subtype),
                StaticModifierRule::ConditionalModifyLifeGain { condition, .. } => {
                    condition.rewrite_creature_type(change.from, change.to);
                }
                StaticModifierRule::AdditionalTrigger(mult) => {
                    mult.event.rewrite_creature_type(change.from, change.to);
                    if let Some(sel) = &mut mult.triggering_source {
                        if let Some(st) = &mut sel.creature_subtype {
                            word(st);
                        }
                        if let Some(st) = &mut sel.forbidden_creature_subtype {
                            word(st);
                        }
                        if sel.any_creature_subtypes.remove(&change.from) {
                            sel.any_creature_subtypes.insert(change.to);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
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
            Self::CostModifier { modifier, .. } => match modifier {
                SpellCostModifier::Discount { matching, .. }
                | SpellCostModifier::Increase { matching, .. } => {
                    return matching
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, .. }
                | SpellCostModifier::FreeSubtypeCreatureSpells(subtype) => {
                    return validate_subtypes(&BTreeSet::from([*subtype]), vocabulary);
                }
                SpellCostModifier::IncreaseExceptType { .. }
                | SpellCostModifier::ChosenTypeOneLessOfEachColor
                | SpellCostModifier::FreeColorlessHandSpellOnce
                | SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce
                | SpellCostModifier::NonartifactSpellsHaveImprovise
                | SpellCostModifier::HighManaValueProtected { .. } => return Ok(()),
            },
            Self::CastPermission { permission, .. } => match permission {
                CastPermissionRule::Flash { matching, .. } => {
                    return matching
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(matching)
                | CastPermissionRule::AllSpellsCannotBeCounteredFiltered(matching) => {
                    return matching
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                CastPermissionRule::PlayLibraryTopMatching { filter, .. }
                | CastPermissionRule::GraveyardMatching { filter, .. }
                | CastPermissionRule::ExileMatching { filter, .. } => {
                    return filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                CastPermissionRule::GraveyardSelfConditional { condition } => {
                    return condition
                        .validate_world(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                CastPermissionRule::SubtypeSpellsHaveReplicate(subtype)
                | CastPermissionRule::GraveyardSubtypeEncore(subtype) => {
                    return validate_subtypes(&BTreeSet::from([*subtype]), vocabulary);
                }
                CastPermissionRule::ControllerSpellsCannotBeCountered(types) => {
                    if types.is_empty() {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    return Ok(());
                }
                CastPermissionRule::SeeLibraryTop { .. }
                | CastPermissionRule::AllPlayersRevealLibraryTop
                | CastPermissionRule::RevealHands { .. }
                | CastPermissionRule::PlayLibraryTop(_)
                | CastPermissionRule::GraveyardCreatureCast
                | CastPermissionRule::GraveyardLandPlay
                | CastPermissionRule::ColorlessHandSpellsHaveCascade { .. }
                | CastPermissionRule::AdditionalLandPlays(_)
                | CastPermissionRule::AllPlayersAdditionalLandPlays(_)
                | CastPermissionRule::LookAtSourceExiledCards => return Ok(()),
            },
            Self::DamageRule { rule, .. } => {
                match rule {
                    StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype) => {
                        validate_subtypes(&BTreeSet::from([*subtype]), vocabulary)?;
                    }
                    StaticDamageRule::ModifyDamage {
                        source,
                        recipient,
                        op,
                        ..
                    } => {
                        if let Some(filter) = source {
                            filter
                                .validate_vocabulary(vocabulary)
                                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                        }
                        if let StaticDamageRecipient::MatchingPermanent(filter) = recipient {
                            filter
                                .validate_vocabulary(vocabulary)
                                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                        }
                        if let StaticDamageOp::AddExpr(expr) = op {
                            expr.validate_world_condition(vocabulary)
                                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                        }
                        if matches!(
                            op,
                            StaticDamageOp::Add(0)
                                | StaticDamageOp::Subtract(0)
                                | StaticDamageOp::Multiply(0 | 1)
                                | StaticDamageOp::PreventUpTo(0)
                                | StaticDamageOp::PreventAtMost(0)
                        ) {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                    }
                    StaticDamageRule::RedirectDamage { source, from, .. } => {
                        if let Some(filter) = source {
                            filter
                                .validate_vocabulary(vocabulary)
                                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                        }
                        if let StaticDamageRecipient::MatchingPermanent(filter) = from {
                            filter
                                .validate_vocabulary(vocabulary)
                                .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                        }
                    }
                    StaticDamageRule::MinimumLifeTotal { floor: 0, .. } => {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    _ => {}
                }
                return Ok(());
            }
            Self::EventReplacement {
                event,
                condition,
                action,
                ..
            } => {
                if let ReplaceableEvent::Explore { filter } = event {
                    filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                }
                if let Some(cond) = condition {
                    crate::characteristics::validate_static_source_condition(cond, vocabulary)?;
                }
                match action {
                    StaticReplacementAction::ModifyAmount { add, multiply } => {
                        if *multiply == 0 {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        add.validate_world_condition(vocabulary)
                            .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                    }
                    StaticReplacementAction::Skip => {}
                    StaticReplacementAction::ReplaceWithBody { body, .. } => {
                        body.validate_in(vocabulary, crate::program::BodyContext::ACTIVATED)
                            .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                        if !crate::effects::event_replacement::is_inline_replacement_body_supported(
                            body,
                        ) {
                            return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                        }
                    }
                }
                return Ok(());
            }
            Self::DrawRule { rule, .. } => {
                match rule {
                    StaticDrawRule::ModifyDraw { add, multiply, .. }
                        if *multiply == 0 || (*add == 0 && *multiply == 1) =>
                    {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    StaticDrawRule::EmptyDrawPutCounters { kind, amount }
                        if *amount == 0 || !kind.is_permanent() =>
                    {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    _ => {}
                }
                return Ok(());
            }
            Self::CommandZoneActive { .. } => {
                return Ok(());
            }
            Self::UntapRule { rule, .. } => {
                if let StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(selector) = rule {
                    return selector.validate(vocabulary);
                }
                return Ok(());
            }
            Self::Restriction { rule, .. } => match rule {
                StaticRestriction::ProhibitCasting { filter, .. } => {
                    return filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                StaticRestriction::ProhibitCastingChosenName(names) => {
                    if names.options().is_empty() {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    return Ok(());
                }
                StaticRestriction::PreventControlledTransform { subtype, except } => {
                    if vocabulary.label(*subtype).is_none() || vocabulary.label(*except).is_none() {
                        return Err(CharacteristicDefinitionError::ForeignCreatureType);
                    }
                    return Ok(());
                }
                StaticRestriction::OthersEnterTapped(selector)
                | StaticRestriction::PreventMatchingActivations(selector)
                | StaticRestriction::PreventMatchingNonManaActivations(selector)
                | StaticRestriction::IgnoreOpponentsHexproof(selector)
                | StaticRestriction::ActivateAbilitiesAsThoughHaste(selector)
                | StaticRestriction::CannotBeSacrificed(selector)
                | StaticRestriction::CannotBeUntapped(selector)
                | StaticRestriction::CantBeTargetedBy { selector, .. }
                | StaticRestriction::EnterUntapped(selector)
                | StaticRestriction::ProhibitEntering(selector) => {
                    return selector.validate(vocabulary);
                }
                StaticRestriction::PreventCounters {
                    selector, players, ..
                } => {
                    if selector.is_none() && players.is_none() {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    if let Some(sel) = selector {
                        sel.validate(vocabulary)?;
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            },
            Self::Modifier { rule, .. } => {
                match rule {
                    StaticModifierRule::IgnoreLegendForSubtype(subtype) => {
                        validate_subtypes(&BTreeSet::from([*subtype]), vocabulary)?;
                    }
                    StaticModifierRule::OthersEnterWithCounters {
                        selector,
                        kind,
                        count,
                    } => {
                        if *count == 0 || !kind.is_permanent() {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        selector.validate(vocabulary)?;
                    }
                    StaticModifierRule::OthersEnterWithCountersExpr {
                        selector,
                        kind,
                        amount,
                    } => {
                        if !kind.is_permanent() {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        selector.validate(vocabulary)?;
                        amount
                            .validate_world_condition(vocabulary)
                            .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                    }
                    StaticModifierRule::BonusTokenOnCreate { token, count, .. } => {
                        if *count == 0 {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        token
                            .validate(vocabulary)
                            .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                    }
                    StaticModifierRule::ModifyCounters {
                        selector,
                        counter_kind,
                        add,
                        multiply,
                    } => {
                        if *multiply == 0
                            || (*add == 0 && *multiply == 1)
                            || counter_kind.is_some_and(|k| !k.is_permanent())
                        {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        selector.validate(vocabulary)?;
                    }
                    StaticModifierRule::ModifyTokens { add, multiply, .. }
                    | StaticModifierRule::ModifyTokenSubtype { add, multiply, .. } => {
                        if *multiply == 0 || (*add == 0 && *multiply == 1) {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                    }
                    StaticModifierRule::ModifyLifeGain {
                        add,
                        multiply,
                        lose_instead,
                        ..
                    } => {
                        if *multiply == 0 || (!*lose_instead && *add == 0 && *multiply == 1) {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                    }
                    StaticModifierRule::ConditionalModifyLifeGain {
                        condition,
                        add,
                        multiply,
                        ..
                    } => {
                        if *multiply == 0 || (*add == 0 && *multiply == 1) {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        condition
                            .validate_world(vocabulary)
                            .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                    }
                    StaticModifierRule::AdditionalTrigger(mult) => {
                        if mult.additional == 0 {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        mult.event.validate(vocabulary)?;
                        if let Some(sel) = &mult.triggering_source {
                            sel.validate(vocabulary)?;
                        }
                    }
                    _ => {}
                }
                return Ok(());
            }
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
                        | DefiningPart::CommanderChosenColor(_)
                        | DefiningPart::Colors(_) => {}
                    }
                }
                return Ok(());
            }
        };
        selector.validate(vocabulary)?;
        if selector.controlled_creatures_and_typed_tokens.is_some()
            && !matches!(
                parts.0.as_slice(),
                [StaticPart::Type(StaticTypeChange {
                    types: TypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    land_subtypes: SubtypeOperation::Keep,
                    subtypes: SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType),
                })]
            )
        {
            return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
        }
        if parts
            .0
            .iter()
            .filter(|part| matches!(part, StaticPart::Abilities(_)))
            .count()
            > 1
        {
            return Err(CharacteristicDefinitionError::DuplicateBucket);
        }
        validate_buckets(parts.0.iter().map(StaticPart::bucket))?;
        for part in &parts.0 {
            if let StaticPart::GrantSubtypeProtection {
                types: StaticSubtypeOperand::Literal(types),
                ..
            } = part
                && (types.is_empty() || types.iter().any(|kind| vocabulary.label(*kind).is_none()))
            {
                return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
            }
            if let StaticPart::Abilities(change) = part {
                if let Some(trigger) = &change.granted_trigger {
                    trigger
                        .validate_body(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                change.validate_grants()?;
            }
            match part {
                StaticPart::BlockingRestriction(
                    BlockingRestriction::BlockerPowerAtLeastPowerOf(_)
                    | BlockingRestriction::CannotBlockObject(_),
                ) => {
                    return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
                }
                StaticPart::BlockingRestriction(
                    BlockingRestriction::OnlyBlockedBySubtype(kind)
                    | BlockingRestriction::CannotBlockSubtype(kind)
                    | BlockingRestriction::CannotBeBlockedBySubtype(kind),
                ) => {
                    validate_subtypes(&BTreeSet::from([*kind]), vocabulary)?;
                }
                StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBlockMatching(filter)
                    | BlockingRestriction::CannotBeBlockedByMatching(filter)
                    | BlockingRestriction::OnlyBlockedByMatching(filter)
                    | BlockingRestriction::CanBlockOnlyMatching(filter)
                    | BlockingRestriction::CannotAttackUnlessDefenderControls(filter)
                    | BlockingRestriction::MustBeBlockedByAllMatching(filter),
                ) => {
                    filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                }
                StaticPart::GrantActivated(ability)
                | StaticPart::GrantActivatedRestricted { ability, .. } => {
                    if let StaticPart::GrantActivatedRestricted { restriction, .. } = part {
                        restriction
                            .validate(vocabulary)
                            .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                    }
                    let is_grantor_sac = matches!(
                        ability.source_cost,
                        crate::activated::SourceCost::SacrificeSelected {
                            filter: crate::activated::SelectedSacrificeFilter::GrantingPermanent,
                            count: 1,
                            tap_source: true,
                            from_graveyard: false,
                        }
                    );
                    if ability.id.0 != 0
                        || !ability.mana_cost.valid()
                        || !matches!(
                            ability.source_cost,
                            crate::activated::SourceCost::PayLife { .. }
                                | crate::activated::SourceCost::None
                                | crate::activated::SourceCost::Tap
                                | crate::activated::SourceCost::Sacrifice
                                | crate::activated::SourceCost::TapSacrifice
                                | crate::activated::SourceCost::Untap
                                | crate::activated::SourceCost::PayEnergy { .. }
                                | crate::activated::SourceCost::ReturnSelfToHand { .. }
                                | crate::activated::SourceCost::Exile {
                                    from_graveyard: false,
                                    ..
                                }
                                | crate::activated::SourceCost::GenericX { .. }
                                | crate::activated::SourceCost::TapPermanents { .. }
                                | crate::activated::SourceCost::Counter { .. }
                                | crate::activated::SourceCost::SacrificeSelected {
                                    count: 1..,
                                    from_graveyard: false,
                                    ..
                                }
                                | crate::activated::SourceCost::Discard {
                                    mode: crate::activated::DiscardCostMode::SelectedOne
                                        | crate::activated::DiscardCostMode::Filtered { .. },
                                    ..
                                }
                                | crate::activated::SourceCost::ExileGraveyard { .. }
                                | crate::activated::SourceCost::Power { .. }
                        )
                    {
                        return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                    }
                    if is_grantor_sac
                        != matches!(
                            ability.body.instructions.as_slice(),
                            [crate::program::Instruction::DamageFromGrantingPermanent { .. }]
                        )
                    {
                        return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                    }
                    let body_context = match ability.source_cost {
                        crate::activated::SourceCost::Counter { kind, .. }
                            if !kind.is_permanent() =>
                        {
                            return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                        }
                        crate::activated::SourceCost::Counter {
                            delta:
                                crate::activated::CounterCostDelta::Remove(_)
                                | crate::activated::CounterCostDelta::RemoveAll,
                            ..
                        } => crate::program::BodyContext::Activated {
                            named_counter_cost: true,
                        },
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
                StaticPart::GrantTrigger(ability) => {
                    ability
                        .validate_body(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                StaticPart::Text(change) => {
                    validate_subtypes(&BTreeSet::from([change.from, change.to]), vocabulary)?;
                }
                StaticPart::Type(change) | StaticPart::TypeWithSubtypes { base: change, .. } => {
                    validate_battlefield_types(&change.types)?;
                    match &change.subtypes {
                        SubtypeOperation::Keep
                        | SubtypeOperation::Add(StaticSubtypeOperand::SourceCurrentSubtypes)
                        | SubtypeOperation::Set(StaticSubtypeOperand::SourceCurrentSubtypes)
                        | SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType)
                        | SubtypeOperation::Set(StaticSubtypeOperand::SourceEntryCreatureType)
                        | SubtypeOperation::Add(StaticSubtypeOperand::AllCreatureTypes)
                        | SubtypeOperation::Set(StaticSubtypeOperand::AllCreatureTypes) => {}
                        SubtypeOperation::Remove(values)
                        | SubtypeOperation::Add(StaticSubtypeOperand::Literal(values))
                        | SubtypeOperation::Add(StaticSubtypeOperand::ImprintedCreatureTypes {
                            keep: values,
                        })
                        | SubtypeOperation::Set(StaticSubtypeOperand::ImprintedCreatureTypes {
                            keep: values,
                        })
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
                | StaticPart::InheritCounteredCreatureAbilities
                | StaticPart::InheritExiledCreatureAbilities
                | StaticPart::ControlSourceController
                | StaticPart::Color(_)
                | StaticPart::Abilities(_)
                | StaticPart::GrantFixedColorMana(_)
                | StaticPart::GrantAnyColorMana
                | StaticPart::GrantMana(_)
                | StaticPart::GrantSubtypeProtection { .. }
                | StaticPart::BlockingRestriction(_)
                | StaticPart::SetPTToOwnManaValue
                | StaticPart::GrantProtectionChosenColor
                | StaticPart::GrantProtectionChosenPlayer
                | StaticPart::SwitchPT => {}
            }
        }
        Ok(())
    }
}
