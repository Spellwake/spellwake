//! Unified composable numeric expression used by both effect amounts and branch conditions.
use super::{
    AttackTriggerMode, BodyContext, CardTypeMask, CastEntryKind, ColorMask, Comparison,
    CreatureTypeSlot, DefinitionError, DiesTriggerMode, FlowFacts, ObjectFilter,
    PlayerTriggerAction, ResolvingPlayer, TriggerCondition, TriggerEvent, TriggerGrouping,
    TriggerPlayerScope, TriggerSpec,
};
use crate::effects::CounterKind;
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::{collections::BTreeSet, num::NonZeroU16};

/// Zone scanned by a [`ValueExpr::Count`] expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueZone {
    Battlefield,
    Graveyard(ResolvingPlayer),
    Hand(ResolvingPlayer),
    /// All players' graveyards combined.
    AllGraveyards,
    /// Exile (either all cards in exile, or cards owned by a specific resolving player).
    Exile(Option<ResolvingPlayer>),
    /// Cards in a resolving player's library.
    Library(ResolvingPlayer),
}

/// Object statistic read by [`ValueExpr::ObjectStat`] or [`ValueExpr::Aggregate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStatKind {
    Power,
    Toughness,
    ManaValue,
    Loyalty,
}

/// Aggregation operator over matching battlefield objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatAggregate {
    Max,
    Min,
    Sum,
}

/// Subject whose statistic is read by [`ValueExpr::ObjectStat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueObjectRef {
    AbilitySource,
    TriggerSource,
    TargetObject,
    DyingSource,
    DyingCreature,
    EnteringPermanent,
    DestructionTarget,
    SacrificedCreature,
    LastSacrificedCreature,
    SacrificedPermanent,
    ExiledCostCreature,
    /// Card revealed as an additional cost to cast the resolving spell.
    RevealedCostCard,
    TriggeringSpell,
    FinalSaga,
    /// Top card revealed by `LookTopChoose { reveal_only: true }` or `MayRevealLibraryTop`.
    RevealedTop,
    /// Object that caused the current triggered ability to fire (e.g., entering/attacking/targeted/dying permanent).
    TriggerSubject,
    /// Permanent to which the ability source Aura or Equipment is attached.
    AttachedHost,
    /// Army token most recently created or grown by `Instruction::Amass` in this resolution.
    LastAmassedArmy,
}

/// Subject whose counters are counted by [`ValueExpr::CounterCount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterSubjectRef {
    AbilitySource,
    TriggerSource,
    DyingSource,
    TargetObject,
    TriggerSubject,
    AttachedHost,
    ControllerExperience,
    ControllerEnergy,
    ControllerPoison,
}

/// Attached permanents counted by [`ValueExpr::AttachedCount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachedCountKind {
    EquipmentOnSource,
    AurasAndEquipmentOnSource,
    CursesOnRelatedPlayer,
}

/// Player-agnostic zone kind for [`SourceStateExpr::InZone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceZoneKind {
    Battlefield,
    Graveyard,
    Hand,
    Exile,
    Command,
    Stack,
    Library,
}

/// Source-permanent state or turn-history scalar read by [`ValueExpr::SourceState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStateExpr {
    /// `1` if the ability source entered the battlefield this turn, else `0`.
    EnteredThisTurn,
    /// `1` if the ability source attacked this turn, else `0`.
    AttackedThisTurn,
    /// `1` if the ability source attacked or blocked this turn, else `0`.
    AttackedOrBlockedThisTurn,
    /// `1` if the ability source is attacking or blocking in the current combat, else `0`.
    AttackedOrBlockedThisCombat,
    /// `1` if the ability source is currently the only attacking creature, else `0`.
    AttackingAlone,
    /// `1` if the ability source is saddled, else `0`.
    Saddled,
    /// `1` if the ability source is renowned, else `0`.
    Renowned,
    /// `1` if the ability source is currently paired with another creature (soulbond), else `0`.
    Paired,
    /// `1` if the ability source is monstrous (CR 701.31), else `0`.
    Monstrous,
    /// `1` if tribute was paid as the ability source entered the battlefield (CR 702.104), else `0`.
    TributePaid,
    /// `1` if the ability source is suspected (CR 701.58), else `0`.
    Suspected,
    /// `1` if the ability source Case is solved (CR 719), else `0`.
    Solved,
    /// `1` if the ability source is prepared, else `0`.
    Prepared,
    /// `1` if the ability source is goaded, else `0`.
    Goaded,
    /// `1` if the ability source was exerted this turn, else `0`.
    ExertedThisTurn,
    /// `1` if the ability source is currently in `zone`, else `0`.
    InZone(SourceZoneKind),
    /// Total damage dealt to the ability source this turn.
    DamageDealtThisTurn,
    /// Number of times any activated ability of the ability source was activated this turn.
    AbilityActivatedThisTurn,
    /// Number of times abilities of the ability source have resolved this turn.
    AbilityResolvedCountThisTurn,
    /// Total mana spent to cast the ability source.
    ManaSpentToCast,
    /// Number of distinct colors of mana spent to cast the ability source.
    ColorsSpentToCast,
    /// Amount of a specific mana type spent to cast the ability source.
    ManaTypeSpentToCast(crate::mana::ManaType),
    /// Amount of mana from snow sources spent to cast the ability source.
    SnowManaSpentToCast,
    /// Amount of mana from Treasure sources spent to cast the ability source.
    TreasureManaSpentToCast,
    /// Amount of mana produced by creatures spent to cast the ability source.
    CreatureManaSpentToCast,
    /// `1` if the ability source was cast from anywhere other than its caster's hand.
    CastFromOutsideHand,
    /// `1` if the ability source was cast from exile.
    CastFromExile,
    /// `1` if the ability source was cast from a graveyard.
    CastFromGraveyard,
    /// `1` if the ability source was cast during its controller's main phase.
    CastDuringMainPhase,
    /// `1` if the ability source was bargained.
    Bargained,
    /// `1` if the gift was promised for the ability source.
    GiftPromised,
    /// `1` if the ability source was cleaved.
    Cleaved,
    /// `1` if evidence was collected for the ability source.
    EvidenceCollected,
    /// `1` if the ability source is currently its controller's Ring-bearer (CR 701.52).
    IsRingBearer,
    /// `1` if the ability source entered the battlefield from a graveyard.
    EnteredFromGraveyard,
    /// `1` if an alternative or keyword casting cost was paid for the ability source.
    PaidAlternative,
}

/// Turn-history statistic checked across active opponents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpponentTurnStat {
    Discarded,
    Sacrificed,
    CastSpells,
    DrewCards,
    Attacked,
    GainedLife,
    LostLife,
    PlayedLand,
    CreatureEntered,
    PermanentLeft,
}

/// Player-level scalar statistic read by [`ValueExpr::PlayerStat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatExpr {
    ControllerLife,
    ControllerLifeAboveStarting,
    ControllerStartingLife,
    OpponentMaxLife,
    /// Minimum life total among active opponents (falls back to controller life if no opponents).
    OpponentMinLife,
    /// Maximum count of `CounterKind` across active opponents (e.g. Corrupted poison check).
    OpponentMaxCounter(CounterKind),
    /// Nonzero (`1`) if any active opponent performed the tracked turn action this turn.
    OpponentStatThisTurn(OpponentTurnStat),
    /// Global day/night state: `0 = Neither`, `1 = Day`, `2 = Night`.
    DayNightState,
    /// Minimum number of cards in any active participant's library.
    MinLibrarySize,
    OpponentsLifeLostThisTurn,
    CardsDrawnThisTurn,
    CardsShortOfHandSize(u8),
    LargestHandSize,
    ControllerIsMonarch,
    OpponentIsMonarch,
    NoMonarch,
    ControlsGreatestCreature,
    ControllerIsActivePlayer,
    ControllerIsAttacking,
    ControllerIsDefending,
    ControllerSpeed,
    OwnGraveyardDistinctCardTypes,
    AllGraveyardsDistinctCardTypes,
    ControllerLifeLostThisTurn,
    ControllerLifeGainedThisTurn,
    ControllerSpellsCastThisTurn,
    CastSpellTypeThisTurn(super::CardType),
    CastNoncreatureSpellsThisTurn,
    CastSpellsFromHandThisTurn,
    ControllerAttackedThisTurn,
    AttackingCreaturesThisTurn,
    ControllerDiscardedThisTurn,
    ControllerSacrificedThisTurn,
    PermanentLeftBattlefieldThisTurn,
    NonlandPermanentLeftBattlefieldThisTurn,
    EnteredPermanentThisTurn,
    EnteredTypeThisTurn(super::CardType),
    CreatureEnteredUnderControlThisTurn,
    PlayedLandThisTurn,
    CardsLeftGraveyardThisTurn,
    CreaturesDiedThisTurn,
    ControlledCreatureDiedThisTurn,
    CreatedTokensThisTurn,
    OpponentDealtDamageThisTurn {
        noncombat_only: bool,
    },
    DescendedThisTurn,
    CommittedCrimeThisTurn,
    ControllerHasCityBlessing,
    AnyOpponentPoisoned,
    /// Current life total of any resolving player.
    PlayerLife(ResolvingPlayer),
    /// Number of cards in any resolving player's hand.
    PlayerHandSize(ResolvingPlayer),
    /// Number of cards in any resolving player's library.
    PlayerLibrarySize(ResolvingPlayer),
    /// Number of cards in any resolving player's graveyard.
    PlayerGraveyardSize(ResolvingPlayer),
    /// `1` if the controller activated a loyalty ability this turn, else `0`.
    LoyaltyActivatedThisTurn,
    /// `1` if it is currently the controller's main phase, else `0`.
    IsOwnMainPhase,
    /// `1` if the controller has activated an exhaust ability on the source permanent, else `0`.
    ExhaustActivatedThisTurn,
    /// Number of dungeons the controller has completed this game (CR 309.7).
    CompletedDungeons,
    /// Number of times the Ring has tempted the controller (0..=4, CR 701.52).
    RingLevel,
    /// `1` if the controller currently has the initiative (CR 725), else `0`.
    HasInitiative,
    /// Number of combat phases that have begun this turn (1 during the first combat phase).
    CombatPhasesThisTurn,
    /// Number of cards put into the controller's graveyard from anywhere this turn.
    CardsEnteredGraveyardThisTurn,
    /// Number of counters placed on creatures by the controller this turn.
    CountersPlacedOnCreaturesThisTurn,
    /// Number of lands that entered the battlefield under `ResolvingPlayer`'s control this turn.
    LandsEnteredThisTurn(ResolvingPlayer),
    /// Number of dice rolled by the controller this turn.
    DiceRolledThisTurn,
    /// Total mana value of other spells the controller cast this turn.
    OtherSpellsCastManaValueThisTurn,
    /// Number of other spells the controller cast this turn (excluding the source if on the stack).
    OtherSpellsCastThisTurn,
    /// Maximum life lost by any single player this turn (0 if no player lost life).
    MaxPlayerLifeLostThisTurn,
}

/// Subject tested by [`ValueExpr::ObjectMatches`] (evaluates to `1` if matched, `0` otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchObjectRef {
    AbilitySource,
    RevealedTop,
    DiscardedCard,
    MovedLibraryCard,
    DestructionTarget,
    DyingCreature,
    SacrificedCreature,
    TargetObject,
    TriggerSubject,
    AttachedHost,
    /// Any object affected/moved by the immediately preceding instruction in this resolution.
    LastAffected,
    /// Any object moved by the immediately preceding instruction that landed in a graveyard.
    LastMovedToGraveyard,
    /// Any object moved by the immediately preceding instruction that landed in exile.
    LastMovedToExile,
    /// Any object moved by the immediately preceding instruction that landed in hand.
    LastMovedToHand,
    /// Any object moved by the immediately preceding instruction that entered the battlefield.
    LastMovedToBattlefield,
}

/// Runtime event, stack, or resolution binding read by [`ValueExpr::EventValue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventValueKind {
    ChosenX,
    EnteredKickerX,
    EnteredKickerCount(usize),
    KickerCount(usize),
    PaidAlternative,
    Bargained,
    CastDuringOwnMain,
    CastFromGraveyard,
    MatchedOccurrences,
    PaidCreatureTaps,
    PaidNamedCounters,
    LargestDiscard,
    DestroyedPermanents,
    ExiledMilledCreaturePower,
    MilledCardsManaValueSum,
    MilledCardsManaValueMax,
    DeclaredSubtypeAttackers,
    DeclaredAttackersAtController,
    TriggeringLifeChange,
    CombatDamageToPlayer,
    DamageReceived,
    CreaturesDiedThisTurn,
    LastSacrificeSucceeded,
    LastActionSucceeded,
    LastActionCount,
    SelectedGraveyardExileCreatures,
    SourceAttackedThisTurn,
    TriggerSourceOnBattlefield,
    ManaSpentToCast,
    ColorsSpentToCast,
    ManaTypeSpentToCast(crate::mana::ManaType),
    SnowManaSpentToCast,
    TreasureManaSpentToCast,
    CreatureManaSpentToCast,
    CastFromOutsideHand,
    CastFromExile,
    GiftPromised,
    Cleaved,
    EvidenceCollected,
    /// Final chosen or combined die roll result from the most recent `Instruction::RollDie`.
    LastDieRoll,
    /// Number of coin flips won during the most recent `Instruction::FlipCoins`.
    CoinFlipsWon,
    /// `1` if the most recent coin flip was won, else `0`.
    LastCoinFlipWon,
    /// Number chosen by `Instruction::ChooseValue(Number)`.
    ChosenNumber,
    /// 0-based option index chosen by `Instruction::ChooseValue(LabeledOption)`.
    ChosenOptionIndex,
    /// Vote count received by option `u8` in the most recent `Instruction::Vote`.
    VoteCount(u8),
    /// Winning option index from the most recent `Instruction::Vote`.
    WinningVoteOption,
    /// Count of nonland cards discarded during the most recent `Instruction::Connive`.
    LastNonlandDiscarded,
    /// Total mana spent to cast the spell that caused this triggered ability to trigger.
    TriggeringSpellManaSpent,
    /// `1` if an optional additional cost (kicker, bargain, buyback, entwine, etc.) was paid when casting this spell.
    AdditionalCostPaid,
}

/// Composable numeric expression unifying effect amounts and resolution-condition operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueExpr {
    /// Exact signed integer literal.
    Literal(RulesInteger),
    /// Count objects matching `filter` in `zone`, optionally scoped to a resolving player's battlefield permanents.
    Count {
        zone: ValueZone,
        filter: Box<ObjectFilter>,
        controller_scope: Option<ResolvingPlayer>,
    },
    /// Maximum count of objects matching `filter` in `zone` across all active opponents.
    OpponentMaxCount {
        zone: ValueZone,
        filter: Box<ObjectFilter>,
    },
    /// Minimum count of objects matching `filter` in `zone` across all active opponents.
    OpponentMinCount {
        zone: ValueZone,
        filter: Box<ObjectFilter>,
    },
    /// CR 700.8: number of creatures in controller's party (0..=4) for `[Cleric, Rogue, Warrior, Wizard]`.
    PartyCount(Box<[CreatureTypeId; 4]>),
    /// Number of distinct WUBRG colors among battlefield permanents matching `filter` (0..=5).
    DistinctColors { filter: Box<ObjectFilter> },
    /// Number of distinct mana values among cards in `player`'s graveyard.
    DistinctManaValuesInGraveyard(ResolvingPlayer),
    /// Source-permanent state or turn-history scalar.
    SourceState(SourceStateExpr),
    /// Count other battlefield creatures controlled by the resolving player sharing a subtype with the trigger source.
    CountOtherSharingSourceSubtype,
    /// Count creatures currently attacking the resolving controller directly (not a planeswalker).
    CountAttackingController,
    /// Aggregate (max or sum) a stat across battlefield objects matching `filter`.
    Aggregate {
        filter: Box<ObjectFilter>,
        stat: ObjectStatKind,
        op: StatAggregate,
    },
    /// Read a stat from a specific resolving or bound object.
    ObjectStat {
        subject: ValueObjectRef,
        stat: ObjectStatKind,
    },
    /// Count counters of `kind` (`None` = all counters) on `subject`.
    CounterCount {
        subject: CounterSubjectRef,
        kind: Option<CounterKind>,
    },
    /// Count permanents attached to the source or related player.
    AttachedCount(AttachedCountKind),
    /// Read a player-level statistic.
    PlayerStat(PlayerStatExpr),
    /// Evaluates to `1` if `subject` matches `filter`, else `0`.
    ObjectMatches {
        subject: MatchObjectRef,
        filter: Box<ObjectFilter>,
    },
    /// Read a stack, trigger, or cursor runtime binding.
    EventValue(EventValueKind),
    /// Multiply a sub-expression by an integer factor.
    Scale {
        expr: Box<ValueExpr>,
        factor: RulesInteger,
    },
    /// Integer division of a sub-expression by a positive constant (rounding down or up).
    DivConst {
        expr: Box<ValueExpr>,
        divisor: NonZeroU16,
        round_up: bool,
    },
    /// Sum of two sub-expressions.
    Add(Box<ValueExpr>, Box<ValueExpr>),
    /// Difference of two sub-expressions (`left - right`).
    Sub(Box<ValueExpr>, Box<ValueExpr>),
    /// CR 305.6: number of distinct basic land types among lands the resolving player controls (0..=5).
    DomainCount,
    /// Number of opponents of the resolving player.
    OpponentsCount,
    /// CR 700.5: controller's devotion to the specified color mask.
    Devotion(ColorMask),
    /// Conditional numeric selection (`if left <cmp> right { then_value } else { else_value }`).
    Conditional {
        left: Box<ValueExpr>,
        comparison: Comparison,
        right: Box<ValueExpr>,
        then_value: Box<ValueExpr>,
        else_value: Box<ValueExpr>,
    },
}

impl ValueExpr {
    /// Exact integer literal.
    #[must_use]
    pub fn literal(value: impl Into<RulesInteger>) -> Self {
        Self::Literal(value.into())
    }

    /// Announced X of the resolving spell or activation.
    #[must_use]
    pub const fn chosen_x() -> Self {
        Self::EventValue(EventValueKind::ChosenX)
    }

    /// Count battlefield permanents matching `filter`.
    #[must_use]
    pub fn count_battlefield(filter: ObjectFilter) -> Self {
        Self::Count {
            zone: ValueZone::Battlefield,
            filter: Box::new(filter),
            controller_scope: None,
        }
    }

    /// Count battlefield permanents controlled by `player` matching `filter`.
    #[must_use]
    pub fn count_player_battlefield(player: ResolvingPlayer, filter: ObjectFilter) -> Self {
        Self::Count {
            zone: ValueZone::Battlefield,
            filter: Box::new(filter),
            controller_scope: Some(player),
        }
    }

    /// Count cards in `player`'s graveyard matching `filter`.
    #[must_use]
    pub fn count_graveyard(player: ResolvingPlayer, filter: ObjectFilter) -> Self {
        Self::Count {
            zone: ValueZone::Graveyard(player),
            filter: Box::new(filter),
            controller_scope: None,
        }
    }

    /// Count cards in `player`'s hand.
    #[must_use]
    pub fn count_hand(player: ResolvingPlayer) -> Self {
        Self::Count {
            zone: ValueZone::Hand(player),
            filter: Box::new(ObjectFilter::any()),
            controller_scope: None,
        }
    }

    /// Aggregate a stat over battlefield objects matching `filter`.
    #[must_use]
    pub fn aggregate(filter: ObjectFilter, stat: ObjectStatKind, op: StatAggregate) -> Self {
        Self::Aggregate {
            filter: Box::new(filter),
            stat,
            op,
        }
    }

    /// Read a stat from a bound or resolving object.
    #[must_use]
    pub const fn object_stat(subject: ValueObjectRef, stat: ObjectStatKind) -> Self {
        Self::ObjectStat { subject, stat }
    }

    /// Count counters on `subject`.
    #[must_use]
    pub const fn counters(subject: CounterSubjectRef, kind: Option<CounterKind>) -> Self {
        Self::CounterCount { subject, kind }
    }

    /// Test whether `subject` matches `filter` (`1` or `0`).
    #[must_use]
    pub fn object_matches(subject: MatchObjectRef, filter: ObjectFilter) -> Self {
        Self::ObjectMatches {
            subject,
            filter: Box::new(filter),
        }
    }

    /// Read a runtime event or stack binding.
    #[must_use]
    pub const fn event(kind: EventValueKind) -> Self {
        Self::EventValue(kind)
    }

    /// Multiply this expression by `factor`.
    #[must_use]
    pub fn scaled(self, factor: impl Into<RulesInteger>) -> Self {
        Self::Scale {
            expr: Box::new(self),
            factor: factor.into(),
        }
    }

    /// Divide this expression by `divisor`, rounding down.
    #[must_use]
    pub fn div_down(self, divisor: NonZeroU16) -> Self {
        Self::DivConst {
            expr: Box::new(self),
            divisor,
            round_up: false,
        }
    }

    /// Divide this expression by 2, rounding down.
    #[must_use]
    pub fn half_down(self) -> Self {
        const TWO: NonZeroU16 = match NonZeroU16::new(2) {
            Some(v) => v,
            None => unreachable!(),
        };
        self.div_down(TWO)
    }

    /// Conditional numeric branch (`if left <cmp> right { then_value } else { else_value }`).
    #[must_use]
    pub fn conditional(
        left: Self,
        comparison: Comparison,
        right: Self,
        then_value: Self,
        else_value: Self,
    ) -> Self {
        Self::Conditional {
            left: Box::new(left),
            comparison,
            right: Box::new(right),
            then_value: Box::new(then_value),
            else_value: Box::new(else_value),
        }
    }

    /// Convenience: count permanents controlled by the resolving player that have `kind`.
    #[must_use]
    pub fn controlled_type(kind: super::CardType) -> Self {
        Self::count_battlefield(
            ObjectFilter::any_types(CardTypeMask::from_card_type(kind)).controlled(),
        )
    }

    /// Convenience: count permanents controlled by the resolving player that have creature subtype `kind`.
    #[must_use]
    pub fn controlled_subtype(kind: CreatureTypeId) -> Self {
        Self::count_battlefield(ObjectFilter::any().controlled().with_subtype(kind))
    }

    /// Return the exact literal value if this expression is a literal.
    #[must_use]
    pub const fn as_literal(&self) -> Option<&RulesInteger> {
        match self {
            Self::Literal(value) => Some(value),
            _ => None,
        }
    }

    /// True if this expression is `ChosenX`.
    #[must_use]
    pub const fn is_chosen_x(&self) -> bool {
        matches!(self, Self::EventValue(EventValueKind::ChosenX))
    }

    /// True if this expression reads `AbilitySource` counters of some kind.
    #[must_use]
    pub const fn ability_source_counter_kind(&self) -> Option<CounterKind> {
        match self {
            Self::CounterCount {
                subject: CounterSubjectRef::AbilitySource,
                kind: Some(kind),
            } => Some(*kind),
            _ => None,
        }
    }

    /// True if this expression requires a live or last-known `AbilitySource` binding.
    #[must_use]
    pub fn references_ability_source(&self) -> bool {
        match self {
            Self::ObjectStat {
                subject: ValueObjectRef::AbilitySource | ValueObjectRef::AttachedHost,
                ..
            }
            | Self::CounterCount {
                subject: CounterSubjectRef::AbilitySource | CounterSubjectRef::AttachedHost,
                ..
            }
            | Self::ObjectMatches {
                subject: MatchObjectRef::AbilitySource | MatchObjectRef::AttachedHost,
                ..
            }
            | Self::SourceState(_) => true,
            Self::Count { filter, .. }
            | Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter }
            | Self::Aggregate { filter, .. } => filter.excludes_ability_source(),
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.references_ability_source()
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.references_ability_source() || b.references_ability_source()
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.references_ability_source()
                    || right.references_ability_source()
                    || then_value.references_ability_source()
                    || else_value.references_ability_source()
            }
            _ => false,
        }
    }

    /// True if this expression can be evaluated purely against a world snapshot without resolution/stack state.
    #[must_use]
    pub fn is_world_evaluable(&self) -> bool {
        let world_player = |p: ResolvingPlayer| {
            matches!(
                p,
                ResolvingPlayer::Controller
                    | ResolvingPlayer::ActivePlayer
                    | ResolvingPlayer::EnchantedPlayer
                    | ResolvingPlayer::AttachedHostController
                    | ResolvingPlayer::SourceChosenPlayer
            )
        };
        let world_filter =
            |f: &ObjectFilter| f.chosen_type_slot().is_none() && !f.has_related_player();
        match self {
            Self::Literal(_)
            | Self::DomainCount
            | Self::OpponentsCount
            | Self::Devotion(_)
            | Self::PartyCount(_)
            | Self::CountOtherSharingSourceSubtype
            | Self::CountAttackingController => true,
            Self::SourceState(kind) => !matches!(kind, SourceStateExpr::Goaded),
            Self::Count {
                zone,
                filter,
                controller_scope,
            } => {
                let zone_ok = match *zone {
                    ValueZone::Battlefield | ValueZone::AllGraveyards | ValueZone::Exile(None) => {
                        true
                    }
                    ValueZone::Graveyard(p)
                    | ValueZone::Hand(p)
                    | ValueZone::Library(p)
                    | ValueZone::Exile(Some(p)) => world_player(p),
                };
                zone_ok && controller_scope.is_none_or(world_player) && world_filter(filter)
            }
            Self::OpponentMaxCount { zone, filter } | Self::OpponentMinCount { zone, filter } => {
                matches!(
                    zone,
                    ValueZone::Battlefield
                        | ValueZone::Graveyard(_)
                        | ValueZone::Hand(_)
                        | ValueZone::Library(_)
                        | ValueZone::Exile(_)
                ) && world_filter(filter)
            }
            Self::DistinctColors { filter } | Self::Aggregate { filter, .. } => {
                world_filter(filter)
            }
            Self::DistinctManaValuesInGraveyard(p) => world_player(*p),
            Self::ObjectStat { subject, .. } => matches!(
                subject,
                ValueObjectRef::AbilitySource
                    | ValueObjectRef::TriggerSource
                    | ValueObjectRef::AttachedHost
            ),
            Self::CounterCount { subject, .. } => matches!(
                subject,
                CounterSubjectRef::AbilitySource
                    | CounterSubjectRef::TriggerSource
                    | CounterSubjectRef::AttachedHost
                    | CounterSubjectRef::ControllerExperience
                    | CounterSubjectRef::ControllerEnergy
                    | CounterSubjectRef::ControllerPoison
            ),
            Self::AttachedCount(kind) => matches!(
                kind,
                AttachedCountKind::EquipmentOnSource | AttachedCountKind::AurasAndEquipmentOnSource
            ),
            Self::PlayerStat(stat) => match stat {
                PlayerStatExpr::CardsShortOfHandSize(_) => false,
                PlayerStatExpr::PlayerLife(p)
                | PlayerStatExpr::PlayerHandSize(p)
                | PlayerStatExpr::PlayerLibrarySize(p)
                | PlayerStatExpr::PlayerGraveyardSize(p)
                | PlayerStatExpr::LandsEnteredThisTurn(p) => world_player(*p),
                _ => true,
            },
            Self::ObjectMatches { subject, filter } => {
                matches!(
                    subject,
                    MatchObjectRef::AbilitySource | MatchObjectRef::AttachedHost
                ) && world_filter(filter)
            }
            Self::EventValue(kind) => matches!(
                kind,
                EventValueKind::CreaturesDiedThisTurn
                    | EventValueKind::SourceAttackedThisTurn
                    | EventValueKind::ManaSpentToCast
                    | EventValueKind::ColorsSpentToCast
                    | EventValueKind::ManaTypeSpentToCast(_)
                    | EventValueKind::SnowManaSpentToCast
                    | EventValueKind::TreasureManaSpentToCast
                    | EventValueKind::CreatureManaSpentToCast
                    | EventValueKind::CastFromOutsideHand
                    | EventValueKind::CastFromExile
                    | EventValueKind::CastFromGraveyard
                    | EventValueKind::CastDuringOwnMain
                    | EventValueKind::Bargained
                    | EventValueKind::GiftPromised
                    | EventValueKind::Cleaved
                    | EventValueKind::EvidenceCollected
                    | EventValueKind::PaidAlternative
                    | EventValueKind::KickerCount(_)
                    | EventValueKind::AdditionalCostPaid
            ),
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => expr.is_world_evaluable(),
            Self::Add(a, b) | Self::Sub(a, b) => a.is_world_evaluable() && b.is_world_evaluable(),
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.is_world_evaluable()
                    && right.is_world_evaluable()
                    && then_value.is_world_evaluable()
                    && else_value.is_world_evaluable()
            }
        }
    }

    /// True if this expression can be evaluated at trigger time against a world snapshot plus trigger event context.
    #[must_use]
    pub fn is_trigger_world_evaluable(&self) -> bool {
        let trig_player = |p: ResolvingPlayer| {
            matches!(
                p,
                ResolvingPlayer::Controller
                    | ResolvingPlayer::ActivePlayer
                    | ResolvingPlayer::EnchantedPlayer
                    | ResolvingPlayer::AttachedHostController
                    | ResolvingPlayer::TriggerSubjectController
                    | ResolvingPlayer::TriggerSubjectOwner
                    | ResolvingPlayer::SourceChosenPlayer
                    | ResolvingPlayer::UniqueRelatedPlayer
                    | ResolvingPlayer::DefendingPlayer
                    | ResolvingPlayer::TargetController
                    | ResolvingPlayer::TargetOwner
            )
        };
        let world_filter = |f: &ObjectFilter| f.chosen_type_slot().is_none();
        match self {
            Self::Literal(_)
            | Self::DomainCount
            | Self::OpponentsCount
            | Self::Devotion(_)
            | Self::PartyCount(_)
            | Self::CountOtherSharingSourceSubtype
            | Self::CountAttackingController => true,
            Self::SourceState(kind) => !matches!(kind, SourceStateExpr::Goaded),
            Self::Count {
                zone,
                filter,
                controller_scope,
            } => {
                let zone_ok = match *zone {
                    ValueZone::Battlefield | ValueZone::AllGraveyards | ValueZone::Exile(None) => {
                        true
                    }
                    ValueZone::Graveyard(p)
                    | ValueZone::Hand(p)
                    | ValueZone::Library(p)
                    | ValueZone::Exile(Some(p)) => trig_player(p),
                };
                zone_ok && controller_scope.is_none_or(trig_player) && world_filter(filter)
            }
            Self::OpponentMaxCount { zone, filter } | Self::OpponentMinCount { zone, filter } => {
                matches!(
                    zone,
                    ValueZone::Battlefield
                        | ValueZone::Graveyard(_)
                        | ValueZone::Hand(_)
                        | ValueZone::Library(_)
                        | ValueZone::Exile(_)
                ) && world_filter(filter)
            }
            Self::DistinctColors { filter } | Self::Aggregate { filter, .. } => {
                world_filter(filter)
            }
            Self::DistinctManaValuesInGraveyard(p) => trig_player(*p),
            Self::ObjectStat { subject, .. } => matches!(
                subject,
                ValueObjectRef::AbilitySource
                    | ValueObjectRef::TriggerSource
                    | ValueObjectRef::AttachedHost
                    | ValueObjectRef::TriggerSubject
                    | ValueObjectRef::DyingCreature
                    | ValueObjectRef::EnteringPermanent
                    | ValueObjectRef::TriggeringSpell
            ),
            Self::CounterCount { subject, .. } => matches!(
                subject,
                CounterSubjectRef::AbilitySource
                    | CounterSubjectRef::TriggerSource
                    | CounterSubjectRef::AttachedHost
                    | CounterSubjectRef::TriggerSubject
                    | CounterSubjectRef::ControllerExperience
                    | CounterSubjectRef::ControllerEnergy
                    | CounterSubjectRef::ControllerPoison
            ),
            Self::AttachedCount(kind) => matches!(
                kind,
                AttachedCountKind::EquipmentOnSource | AttachedCountKind::AurasAndEquipmentOnSource
            ),
            Self::PlayerStat(stat) => match stat {
                PlayerStatExpr::CardsShortOfHandSize(_) => false,
                PlayerStatExpr::PlayerLife(p)
                | PlayerStatExpr::PlayerHandSize(p)
                | PlayerStatExpr::PlayerLibrarySize(p)
                | PlayerStatExpr::PlayerGraveyardSize(p)
                | PlayerStatExpr::LandsEnteredThisTurn(p) => trig_player(*p),
                _ => true,
            },
            Self::ObjectMatches { subject, filter } => {
                matches!(
                    subject,
                    MatchObjectRef::AbilitySource
                        | MatchObjectRef::AttachedHost
                        | MatchObjectRef::TriggerSubject
                        | MatchObjectRef::DyingCreature
                ) && world_filter(filter)
            }
            Self::EventValue(kind) => matches!(
                kind,
                EventValueKind::CreaturesDiedThisTurn
                    | EventValueKind::SourceAttackedThisTurn
                    | EventValueKind::ManaSpentToCast
                    | EventValueKind::ColorsSpentToCast
                    | EventValueKind::ManaTypeSpentToCast(_)
                    | EventValueKind::SnowManaSpentToCast
                    | EventValueKind::TreasureManaSpentToCast
                    | EventValueKind::CreatureManaSpentToCast
                    | EventValueKind::CastFromOutsideHand
                    | EventValueKind::CastFromExile
                    | EventValueKind::CastFromGraveyard
                    | EventValueKind::CastDuringOwnMain
                    | EventValueKind::Bargained
                    | EventValueKind::GiftPromised
                    | EventValueKind::Cleaved
                    | EventValueKind::EvidenceCollected
                    | EventValueKind::PaidAlternative
                    | EventValueKind::KickerCount(_)
                    | EventValueKind::TriggeringSpellManaSpent
                    | EventValueKind::AdditionalCostPaid
            ),
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.is_trigger_world_evaluable()
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.is_trigger_world_evaluable() && b.is_trigger_world_evaluable()
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.is_trigger_world_evaluable()
                    && right.is_trigger_world_evaluable()
                    && then_value.is_trigger_world_evaluable()
                    && else_value.is_trigger_world_evaluable()
            }
        }
    }

    /// Validate that this expression is world-evaluable and references only valid creature types.
    pub(crate) fn validate_world_expr(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if !self.is_world_evaluable() {
            return Err(DefinitionError::InvalidInstruction);
        }
        self.validate_trigger_world_condition(vocabulary)
    }

    /// Validate that this expression is trigger-world-evaluable and references only valid creature types.
    pub(crate) fn validate_trigger_world_condition(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if !self.is_trigger_world_evaluable() {
            return Err(DefinitionError::InvalidInstruction);
        }
        match self {
            Self::Count { filter, .. }
            | Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter }
            | Self::Aggregate { filter, .. }
            | Self::ObjectMatches { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::PartyCount(roles) => {
                if roles.iter().any(|ct| vocabulary.label(*ct).is_none()) {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.validate_trigger_world_condition(vocabulary)
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.validate_trigger_world_condition(vocabulary)?;
                b.validate_trigger_world_condition(vocabulary)
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.validate_trigger_world_condition(vocabulary)?;
                right.validate_trigger_world_condition(vocabulary)?;
                then_value.validate_trigger_world_condition(vocabulary)?;
                else_value.validate_trigger_world_condition(vocabulary)
            }
            _ => Ok(()),
        }
    }

    /// Validate that this expression is purely world-evaluable and references only valid creature types.
    pub(crate) fn validate_world_condition(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if !self.is_world_evaluable() {
            return Err(DefinitionError::InvalidInstruction);
        }
        match self {
            Self::Count { filter, .. }
            | Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter }
            | Self::Aggregate { filter, .. }
            | Self::ObjectMatches { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::PartyCount(roles) => {
                if roles.iter().any(|ct| vocabulary.label(*ct).is_none()) {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.validate_world_condition(vocabulary)
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.validate_world_condition(vocabulary)?;
                b.validate_world_condition(vocabulary)
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.validate_world_condition(vocabulary)?;
                right.validate_world_condition(vocabulary)?;
                then_value.validate_world_condition(vocabulary)?;
                else_value.validate_world_condition(vocabulary)
            }
            _ => Ok(()),
        }
    }

    /// True if this expression reads `TargetPlayer` tapped creatures on the battlefield.
    #[must_use]
    pub fn is_target_player_tapped_creatures(&self) -> bool {
        matches!(
            self,
            Self::Count {
                zone: ValueZone::Battlefield,
                controller_scope: Some(ResolvingPlayer::TargetPlayer),
                filter,
            } if **filter == ObjectFilter::creature().tapped()
        )
    }

    /// True if this expression reads `TargetPlayer`'s hand size.
    #[must_use]
    pub fn is_target_player_hand_size(&self) -> bool {
        matches!(
            self,
            Self::Count {
                zone: ValueZone::Hand(ResolvingPlayer::TargetPlayer),
                filter,
                controller_scope: None,
            } if **filter == ObjectFilter::any()
        )
    }

    /// True if this expression references the resolving ability/trigger source directly.
    #[must_use]
    pub fn references_trigger_source_stat(&self) -> bool {
        matches!(
            self,
            Self::ObjectStat {
                subject: ValueObjectRef::TriggerSource,
                stat: ObjectStatKind::Power,
            } | Self::CounterCount {
                subject: CounterSubjectRef::TriggerSource,
                kind: Some(CounterKind::PlusOne),
            }
        )
    }

    /// True if this expression directly reads from the resolving spell or ability's target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Count {
                zone,
                controller_scope,
                ..
            } => {
                let role_uses = |r: ResolvingPlayer| {
                    matches!(
                        r,
                        ResolvingPlayer::TargetPlayer
                            | ResolvingPlayer::TargetController
                            | ResolvingPlayer::TargetOwner
                            | ResolvingPlayer::TargetSpellController
                    )
                };
                controller_scope.is_some_and(role_uses)
                    || matches!(
                        zone,
                        ValueZone::Hand(r) | ValueZone::Graveyard(r) | ValueZone::Library(r)
                            if role_uses(*r)
                    )
            }
            Self::PlayerStat(
                PlayerStatExpr::PlayerLife(r)
                | PlayerStatExpr::PlayerHandSize(r)
                | PlayerStatExpr::PlayerLibrarySize(r)
                | PlayerStatExpr::PlayerGraveyardSize(r)
                | PlayerStatExpr::LandsEnteredThisTurn(r),
            ) => matches!(
                r,
                ResolvingPlayer::TargetPlayer
                    | ResolvingPlayer::TargetController
                    | ResolvingPlayer::TargetOwner
                    | ResolvingPlayer::TargetSpellController
            ),
            Self::ObjectStat {
                subject: ValueObjectRef::TargetObject,
                ..
            }
            | Self::ObjectMatches {
                subject: MatchObjectRef::TargetObject,
                ..
            }
            | Self::CounterCount {
                subject: CounterSubjectRef::TargetObject,
                ..
            } => true,
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => expr.uses_target(),
            Self::Add(a, b) | Self::Sub(a, b) => a.uses_target() || b.uses_target(),
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.uses_target()
                    || right.uses_target()
                    || then_value.uses_target()
                    || else_value.uses_target()
            }
            _ => false,
        }
    }

    /// Determine the target operand family required by any target references inside this expression.
    pub(super) fn required_operand_family(
        &self,
        target: super::BodyTarget,
    ) -> Result<Option<super::OperandFamily>, super::DefinitionError> {
        use super::{BodyTarget, DefinitionError, OperandFamily};
        let merge = |acc: &mut Option<OperandFamily>, next: Option<OperandFamily>| {
            if let Some(f) = next {
                if let Some(existing) = *acc
                    && existing != f
                {
                    return Err(DefinitionError::OperandFamily);
                }
                *acc = Some(f);
            }
            Ok(())
        };
        let role_family = |r: ResolvingPlayer| -> Result<Option<OperandFamily>, DefinitionError> {
            match r {
                ResolvingPlayer::TargetPlayer => match target {
                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                        Ok(Some(OperandFamily::Player))
                    }
                    BodyTarget::SingleAnyTarget
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker => {
                        Ok(Some(OperandFamily::DamageRecipient))
                    }
                    _ => Err(DefinitionError::OperandFamily),
                },
                ResolvingPlayer::TargetController => match target {
                    BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::FilteredPermanent(_) => Ok(Some(OperandFamily::Permanent)),
                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                        Ok(Some(OperandFamily::Player))
                    }
                    BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                    BodyTarget::SingleAnyTarget
                    | BodyTarget::SingleCreatureOrPlayer
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker => {
                        Ok(Some(OperandFamily::DamageRecipient))
                    }
                    _ => Err(DefinitionError::OperandFamily),
                },
                ResolvingPlayer::TargetOwner => match target {
                    BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::FilteredPermanent(_) => Ok(Some(OperandFamily::Permanent)),
                    BodyTarget::SingleOwnGraveyardCard
                    | BodyTarget::AnyGraveyardCard
                    | BodyTarget::AnyGraveyardCreature
                    | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                    | BodyTarget::FilteredOwnGraveyardCard(_)
                    | BodyTarget::FilteredAnyGraveyardCard { .. } => {
                        Ok(Some(OperandFamily::GraveyardCard))
                    }
                    BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                    BodyTarget::SingleAnyTarget
                    | BodyTarget::SingleCreatureOrPlayer
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker => {
                        Ok(Some(OperandFamily::DamageRecipient))
                    }
                    _ => Err(DefinitionError::OperandFamily),
                },
                ResolvingPlayer::TargetSpellController => match target {
                    BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                    _ => Err(DefinitionError::OperandFamily),
                },
                _ => Ok(None),
            }
        };
        match self {
            Self::Count {
                zone,
                filter,
                controller_scope,
            } => {
                let mut out = None;
                if let Some(r) = controller_scope {
                    merge(&mut out, role_family(*r)?)?;
                }
                if let ValueZone::Hand(r)
                | ValueZone::Graveyard(r)
                | ValueZone::Library(r)
                | ValueZone::Exile(Some(r)) = zone
                {
                    merge(&mut out, role_family(*r)?)?;
                }
                if filter.has_target_player() {
                    merge(&mut out, Some(OperandFamily::Player))?;
                }
                Ok(out)
            }
            Self::Aggregate { filter, .. } | Self::DistinctColors { filter } => {
                if filter.has_target_player() {
                    Ok(Some(OperandFamily::Player))
                } else {
                    Ok(None)
                }
            }
            Self::PlayerStat(
                PlayerStatExpr::PlayerLife(r)
                | PlayerStatExpr::PlayerHandSize(r)
                | PlayerStatExpr::PlayerLibrarySize(r)
                | PlayerStatExpr::PlayerGraveyardSize(r)
                | PlayerStatExpr::LandsEnteredThisTurn(r),
            ) => role_family(*r),
            Self::ObjectStat {
                subject: ValueObjectRef::TargetObject,
                ..
            }
            | Self::ObjectMatches {
                subject: MatchObjectRef::TargetObject,
                ..
            }
            | Self::CounterCount {
                subject: CounterSubjectRef::TargetObject,
                ..
            } => match target {
                BodyTarget::SingleCreature
                | BodyTarget::SinglePermanent
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_) => Ok(Some(OperandFamily::Permanent)),
                BodyTarget::SingleAnyTarget
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::SingleOpponentOrPlaneswalker => {
                    Ok(Some(OperandFamily::DamageRecipient))
                }
                BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                BodyTarget::SingleOwnGraveyardCard
                | BodyTarget::AnyGraveyardCard
                | BodyTarget::AnyGraveyardCreature
                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                | BodyTarget::FilteredOwnGraveyardCard(_)
                | BodyTarget::FilteredAnyGraveyardCard { .. } => {
                    Ok(Some(OperandFamily::GraveyardCard))
                }
                _ => Err(DefinitionError::OperandFamily),
            },
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.required_operand_family(target)
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                let mut out = a.required_operand_family(target)?;
                merge(&mut out, b.required_operand_family(target)?)?;
                Ok(out)
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                let mut out = None;
                for sub in [left, right, then_value, else_value] {
                    merge(&mut out, sub.required_operand_family(target)?)?;
                }
                Ok(out)
            }
            _ => Ok(None),
        }
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        match self {
            Self::Count { filter, .. }
            | Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter }
            | Self::Aggregate { filter, .. }
            | Self::ObjectMatches { filter, .. } => filter.rewrite_creature_type(from, to),
            Self::PartyCount(roles) => {
                for role in roles.iter_mut() {
                    if *role == from {
                        *role = to;
                    }
                }
            }
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.rewrite_creature_type(from, to);
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.rewrite_creature_type(from, to);
                b.rewrite_creature_type(from, to);
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.rewrite_creature_type(from, to);
                right.rewrite_creature_type(from, to);
                then_value.rewrite_creature_type(from, to);
                else_value.rewrite_creature_type(from, to);
            }
            Self::Literal(_)
            | Self::CountOtherSharingSourceSubtype
            | Self::CountAttackingController
            | Self::DomainCount
            | Self::OpponentsCount
            | Self::Devotion(_)
            | Self::DistinctManaValuesInGraveyard(_)
            | Self::SourceState(_)
            | Self::ObjectStat { .. }
            | Self::CounterCount { .. }
            | Self::AttachedCount(_)
            | Self::PlayerStat(_)
            | Self::EventValue(_) => {}
        }
    }

    /// Validate a numeric effect-amount expression against the enclosing instruction context.
    pub(super) fn validate_amount(
        &self,
        defined_slots: &BTreeSet<CreatureTypeSlot>,
        context: BodyContext<'_>,
        facts: &FlowFacts,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Literal(value) => {
                if *value < 0.into() {
                    Err(DefinitionError::InvalidInstruction)
                } else {
                    Ok(())
                }
            }
            Self::Scale { expr, factor } => {
                if !factor.is_positive() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                expr.validate_amount(defined_slots, context, facts, vocabulary)
            }
            Self::DivConst { expr, .. } => {
                expr.validate_amount(defined_slots, context, facts, vocabulary)
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.validate_amount(defined_slots, context, facts, vocabulary)?;
                b.validate_amount(defined_slots, context, facts, vocabulary)
            }
            Self::DomainCount
            | Self::OpponentsCount
            | Self::Devotion(_)
            | Self::DistinctManaValuesInGraveyard(_) => Ok(()),
            Self::SourceState(_) => {
                if context.has_source() {
                    Ok(())
                } else {
                    Err(DefinitionError::InvalidInstruction)
                }
            }
            Self::PartyCount(roles) => {
                if roles.iter().any(|ct| vocabulary.label(*ct).is_none()) {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter } => {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                if let Some(slot) = filter.chosen_type_slot()
                    && !defined_slots.contains(&slot)
                {
                    return Err(DefinitionError::UndefinedCreatureTypeSlot);
                }
                Ok(())
            }
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.validate_amount(defined_slots, context, facts, vocabulary)?;
                right.validate_amount(defined_slots, context, facts, vocabulary)?;
                then_value.validate_amount(defined_slots, context, facts, vocabulary)?;
                else_value.validate_amount(defined_slots, context, facts, vocabulary)
            }
            Self::Count {
                zone,
                filter,
                controller_scope: _,
            } => {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                if let Some(slot) = filter.chosen_type_slot()
                    && !defined_slots.contains(&slot)
                {
                    return Err(DefinitionError::UndefinedCreatureTypeSlot);
                }
                if filter.excludes_ability_source() && !context.has_source() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if *zone == ValueZone::Battlefield {
                    let (a, b) = filter.clauses();
                    let invalid_non_perm = |c: super::ObjectClause| {
                        !c.any_types.is_empty()
                            && (c.any_types.bits() & !CardTypeMask::PERMANENT.bits()) != 0
                    };
                    if invalid_non_perm(a) || b.is_some_and(invalid_non_perm) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                }
                Ok(())
            }
            Self::CountOtherSharingSourceSubtype => {
                if matches!(context, BodyContext::TriggerBound { .. }) {
                    Ok(())
                } else {
                    Err(DefinitionError::InvalidInstruction)
                }
            }
            Self::CountAttackingController => Ok(()),
            Self::Aggregate { filter, .. } => {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                if filter.excludes_ability_source() && !context.has_source() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Ok(())
            }
            Self::ObjectStat { subject, stat } => match subject {
                ValueObjectRef::AbilitySource | ValueObjectRef::AttachedHost => {
                    if context.has_source() {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::TriggerSource | ValueObjectRef::TriggerSubject => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::RevealedTop | ValueObjectRef::LastAmassedArmy => Ok(()),
                ValueObjectRef::TargetObject => match stat {
                    ObjectStatKind::Power | ObjectStatKind::Toughness => {
                        if facts.exact_creature_target || facts.has_object_or_spell_target {
                            Ok(())
                        } else {
                            Err(DefinitionError::OperandFamily)
                        }
                    }
                    ObjectStatKind::ManaValue | ObjectStatKind::Loyalty => {
                        if facts.has_object_or_spell_target {
                            Ok(())
                        } else {
                            Err(DefinitionError::OperandFamily)
                        }
                    }
                },
                ValueObjectRef::DyingSource => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Dies { subject, mode: DiesTriggerMode::Creature },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *subject == ObjectFilter::self_object()
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::DyingCreature => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Dies {
                                    mode: DiesTriggerMode::Creature
                                        | DiesTriggerMode::DamagedBySource,
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::EnteringPermanent => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Enters { .. },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::DestructionTarget => {
                    if facts.checked_destruction_target {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::SacrificedCreature => {
                    if matches!(context, BodyContext::SacrificedCreature) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::LastSacrificedCreature => {
                    if facts.definitely_sacrificed {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::SacrificedPermanent => {
                    if matches!(
                        context,
                        BodyContext::SacrificedPermanent | BodyContext::SacrificedCreature
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::ExiledCostCreature => {
                    if matches!(context, BodyContext::ExiledCreatureCost) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::RevealedCostCard => {
                    if matches!(context, BodyContext::RevealedCreature) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::TriggeringSpell => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::SpellCast {
                                    caster: TriggerPlayerScope::Relation(_),
                                    include_copy: false,
                                    min_mana_value: None,
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::FinalSaga => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::PlayerAction {
                                    action: PlayerTriggerAction::SagaFinalChapterResolved,
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
            },
            Self::CounterCount { subject, kind } => match subject {
                CounterSubjectRef::AbilitySource | CounterSubjectRef::AttachedHost => {
                    if kind.is_none_or(CounterKind::is_permanent) && context.has_source() {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::TargetObject => {
                    if kind.is_none_or(CounterKind::is_permanent)
                        && facts.has_object_or_spell_target
                    {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::TriggerSource | CounterSubjectRef::TriggerSubject => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::DyingSource => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Dies { subject, mode: DiesTriggerMode::Creature },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *subject == ObjectFilter::self_object()
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::ControllerExperience
                | CounterSubjectRef::ControllerEnergy
                | CounterSubjectRef::ControllerPoison => Ok(()),
            },
            Self::AttachedCount(kind) => match kind {
                AttachedCountKind::EquipmentOnSource
                | AttachedCountKind::AurasAndEquipmentOnSource => {
                    if context.has_source() {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                AttachedCountKind::CursesOnRelatedPlayer => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::StepBegan {
                                    step: crate::turn::Step::Upkeep,
                                    player: TriggerPlayerScope::EnchantedPlayer,
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
            },
            Self::PlayerStat(stat) => match stat {
                PlayerStatExpr::CardsShortOfHandSize(limit) => {
                    if *limit > 0
                        && matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    intervening_if: Some(TriggerCondition::HandSizeBelow(required)),
                                    ..
                                },
                                ..
                            } if *required == *limit
                        )
                    {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                PlayerStatExpr::ControllerLife
                | PlayerStatExpr::ControllerLifeAboveStarting
                | PlayerStatExpr::ControllerStartingLife
                | PlayerStatExpr::OpponentMaxLife
                | PlayerStatExpr::OpponentMinLife
                | PlayerStatExpr::OpponentMaxCounter(_)
                | PlayerStatExpr::OpponentStatThisTurn(_)
                | PlayerStatExpr::DayNightState
                | PlayerStatExpr::MinLibrarySize
                | PlayerStatExpr::OpponentsLifeLostThisTurn
                | PlayerStatExpr::CardsDrawnThisTurn
                | PlayerStatExpr::LargestHandSize
                | PlayerStatExpr::ControllerIsMonarch
                | PlayerStatExpr::OpponentIsMonarch
                | PlayerStatExpr::NoMonarch
                | PlayerStatExpr::ControlsGreatestCreature
                | PlayerStatExpr::ControllerIsActivePlayer
                | PlayerStatExpr::ControllerIsAttacking
                | PlayerStatExpr::ControllerIsDefending
                | PlayerStatExpr::ControllerSpeed
                | PlayerStatExpr::OwnGraveyardDistinctCardTypes
                | PlayerStatExpr::AllGraveyardsDistinctCardTypes
                | PlayerStatExpr::ControllerLifeLostThisTurn
                | PlayerStatExpr::ControllerLifeGainedThisTurn
                | PlayerStatExpr::ControllerSpellsCastThisTurn
                | PlayerStatExpr::CastSpellTypeThisTurn(_)
                | PlayerStatExpr::CastNoncreatureSpellsThisTurn
                | PlayerStatExpr::CastSpellsFromHandThisTurn
                | PlayerStatExpr::ControllerAttackedThisTurn
                | PlayerStatExpr::AttackingCreaturesThisTurn
                | PlayerStatExpr::ControllerDiscardedThisTurn
                | PlayerStatExpr::ControllerSacrificedThisTurn
                | PlayerStatExpr::PermanentLeftBattlefieldThisTurn
                | PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn
                | PlayerStatExpr::EnteredPermanentThisTurn
                | PlayerStatExpr::EnteredTypeThisTurn(_)
                | PlayerStatExpr::CreatureEnteredUnderControlThisTurn
                | PlayerStatExpr::PlayedLandThisTurn
                | PlayerStatExpr::CardsLeftGraveyardThisTurn
                | PlayerStatExpr::CreaturesDiedThisTurn
                | PlayerStatExpr::ControlledCreatureDiedThisTurn
                | PlayerStatExpr::CreatedTokensThisTurn
                | PlayerStatExpr::OpponentDealtDamageThisTurn { .. }
                | PlayerStatExpr::DescendedThisTurn
                | PlayerStatExpr::CommittedCrimeThisTurn
                | PlayerStatExpr::ControllerHasCityBlessing
                | PlayerStatExpr::AnyOpponentPoisoned
                | PlayerStatExpr::LoyaltyActivatedThisTurn
                | PlayerStatExpr::IsOwnMainPhase
                | PlayerStatExpr::ExhaustActivatedThisTurn
                | PlayerStatExpr::CompletedDungeons
                | PlayerStatExpr::RingLevel
                | PlayerStatExpr::HasInitiative
                | PlayerStatExpr::CombatPhasesThisTurn
                | PlayerStatExpr::CardsEnteredGraveyardThisTurn
                | PlayerStatExpr::CountersPlacedOnCreaturesThisTurn
                | PlayerStatExpr::DiceRolledThisTurn
                | PlayerStatExpr::OtherSpellsCastManaValueThisTurn
                | PlayerStatExpr::OtherSpellsCastThisTurn
                | PlayerStatExpr::MaxPlayerLifeLostThisTurn => Ok(()),
                PlayerStatExpr::PlayerLife(role)
                | PlayerStatExpr::PlayerHandSize(role)
                | PlayerStatExpr::PlayerLibrarySize(role)
                | PlayerStatExpr::PlayerGraveyardSize(role)
                | PlayerStatExpr::LandsEnteredThisTurn(role) => {
                    if matches!(
                        role,
                        ResolvingPlayer::Controller
                            | ResolvingPlayer::TargetPlayer
                            | ResolvingPlayer::TargetController
                            | ResolvingPlayer::TargetOwner
                            | ResolvingPlayer::UniqueRelatedPlayer
                            | ResolvingPlayer::ActivePlayer
                            | ResolvingPlayer::EnchantedPlayer
                            | ResolvingPlayer::AttachedHostController
                            | ResolvingPlayer::TriggerSubjectController
                            | ResolvingPlayer::TriggerSubjectOwner
                            | ResolvingPlayer::SourceChosenPlayer
                            | ResolvingPlayer::DefendingPlayer
                            | ResolvingPlayer::ChosenPlayer
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundResolvingPlayer)
                    }
                }
            },
            Self::ObjectMatches { filter, .. } => filter
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::EventValue(kind) => match kind {
                EventValueKind::ChosenX => {
                    if facts.defined_resolution_x {
                        Ok(())
                    } else if let BodyContext::TriggerBound {
                        condition,
                        grouping,
                    } = context
                    {
                        if grouping == TriggerGrouping::EachOccurrence
                            && (matches!(
                                &condition.event,
                                TriggerEvent::Enters { subject, .. }
                                    if *subject == ObjectFilter::self_object()
                            ) || matches!(
                                &condition.event,
                                TriggerEvent::SpellCast {
                                    self_cast: true,
                                    ..
                                }
                            ))
                        {
                            Ok(())
                        } else {
                            Err(DefinitionError::InvalidInstruction)
                        }
                    } else {
                        Ok(())
                    }
                }
                EventValueKind::MatchedOccurrences => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerAmount)
                    }
                }
                EventValueKind::PaidCreatureTaps => {
                    if matches!(context, BodyContext::TapCreatureCost) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::PaidNamedCounters => {
                    if matches!(
                        context,
                        BodyContext::Activated {
                            named_counter_cost: true
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::TriggeringLifeChange => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::LifeChanged { .. },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::MilledCardsManaValueSum
                | EventValueKind::MilledCardsManaValueMax => {
                    if facts.definitely_milled_top {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::LargestDiscard => {
                    if facts.definitely_discarded {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::DeclaredSubtypeAttackers => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Attacks {
                                    mode: AttackTriggerMode::GroupSubtype(_),
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::EnteredKickerX | EventValueKind::EnteredKickerCount(_) => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Enters {
                                    cast_origin: Some(CastEntryKind::Kicked),
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::ExiledMilledCreaturePower => {
                    if facts.definitely_exiled_milled {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::DestroyedPermanents => {
                    if facts.definitely_destroyed {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::CombatDamageToPlayer => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::DealsDamage { .. },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::DamageReceived => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::DealtDamage {
                                    linked_player: false,
                                    ..
                                },
                                ..
                            },
                            ..
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                EventValueKind::KickerCount(_)
                | EventValueKind::PaidAlternative
                | EventValueKind::Bargained
                | EventValueKind::CastDuringOwnMain
                | EventValueKind::CastFromGraveyard
                | EventValueKind::DeclaredAttackersAtController
                | EventValueKind::CreaturesDiedThisTurn
                | EventValueKind::LastSacrificeSucceeded
                | EventValueKind::LastActionSucceeded
                | EventValueKind::LastActionCount
                | EventValueKind::SelectedGraveyardExileCreatures
                | EventValueKind::SourceAttackedThisTurn
                | EventValueKind::TriggerSourceOnBattlefield
                | EventValueKind::ManaSpentToCast
                | EventValueKind::ColorsSpentToCast
                | EventValueKind::ManaTypeSpentToCast(_)
                | EventValueKind::SnowManaSpentToCast
                | EventValueKind::TreasureManaSpentToCast
                | EventValueKind::CreatureManaSpentToCast
                | EventValueKind::CastFromOutsideHand
                | EventValueKind::CastFromExile
                | EventValueKind::GiftPromised
                | EventValueKind::Cleaved
                | EventValueKind::EvidenceCollected
                | EventValueKind::LastDieRoll
                | EventValueKind::CoinFlipsWon
                | EventValueKind::LastCoinFlipWon
                | EventValueKind::ChosenNumber
                | EventValueKind::ChosenOptionIndex
                | EventValueKind::VoteCount(_)
                | EventValueKind::WinningVoteOption
                | EventValueKind::LastNonlandDiscarded
                | EventValueKind::TriggeringSpellManaSpent
                | EventValueKind::AdditionalCostPaid => Ok(()),
            },
        }
    }

    /// Validate a condition operand against the enclosing branch context.
    pub(super) fn validate_condition(
        &self,
        context: BodyContext<'_>,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Literal(_)
            | Self::PlayerStat(_)
            | Self::CountAttackingController
            | Self::DistinctManaValuesInGraveyard(_) => Ok(()),
            Self::SourceState(_) => {
                if !matches!(context, BodyContext::Unbound) {
                    Ok(())
                } else {
                    Err(DefinitionError::InvalidInstruction)
                }
            }
            Self::PartyCount(roles) => {
                if roles.iter().any(|ct| vocabulary.label(*ct).is_none()) {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::Count { filter, .. }
            | Self::OpponentMaxCount { filter, .. }
            | Self::OpponentMinCount { filter, .. }
            | Self::DistinctColors { filter }
            | Self::Aggregate { filter, .. } => filter
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::CountOtherSharingSourceSubtype => {
                if matches!(context, BodyContext::TriggerBound { .. }) {
                    Ok(())
                } else {
                    Err(DefinitionError::UnboundTriggerCondition)
                }
            }
            Self::ObjectMatches { subject, filter } => {
                let vocab_ok = filter.validate_vocabulary(vocabulary).is_ok();
                match subject {
                    MatchObjectRef::SacrificedCreature => {
                        if matches!(context, BodyContext::SacrificedCreature) && vocab_ok {
                            Ok(())
                        } else {
                            Err(DefinitionError::InvalidInstruction)
                        }
                    }
                    MatchObjectRef::DyingCreature => {
                        if matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Dies {
                                        mode: DiesTriggerMode::Creature,
                                        ..
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            }
                        ) && vocab_ok
                        {
                            Ok(())
                        } else {
                            Err(DefinitionError::UnboundTriggerCondition)
                        }
                    }
                    MatchObjectRef::AbilitySource | MatchObjectRef::AttachedHost => {
                        if vocab_ok && !matches!(context, BodyContext::Unbound) {
                            Ok(())
                        } else {
                            Err(DefinitionError::InvalidInstruction)
                        }
                    }
                    MatchObjectRef::TriggerSubject => {
                        if vocab_ok && matches!(context, BodyContext::TriggerBound { .. }) {
                            Ok(())
                        } else {
                            Err(DefinitionError::UnboundTriggerCondition)
                        }
                    }
                    MatchObjectRef::DestructionTarget
                    | MatchObjectRef::TargetObject
                    | MatchObjectRef::RevealedTop
                    | MatchObjectRef::DiscardedCard
                    | MatchObjectRef::MovedLibraryCard
                    | MatchObjectRef::LastAffected
                    | MatchObjectRef::LastMovedToGraveyard
                    | MatchObjectRef::LastMovedToExile
                    | MatchObjectRef::LastMovedToHand
                    | MatchObjectRef::LastMovedToBattlefield => {
                        if vocab_ok {
                            Ok(())
                        } else {
                            Err(DefinitionError::InvalidInstruction)
                        }
                    }
                }
            }
            Self::CounterCount { subject, kind } => match subject {
                CounterSubjectRef::AbilitySource | CounterSubjectRef::AttachedHost => {
                    if kind.is_none_or(CounterKind::is_permanent) && context.has_source() {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::TargetObject => {
                    if kind.is_none_or(CounterKind::is_permanent) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::ControllerExperience
                | CounterSubjectRef::ControllerEnergy
                | CounterSubjectRef::ControllerPoison => Ok(()),
                CounterSubjectRef::TriggerSource
                | CounterSubjectRef::TriggerSubject
                | CounterSubjectRef::DyingSource => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerCondition)
                    }
                }
            },
            Self::ObjectStat { subject, .. } => match subject {
                ValueObjectRef::TargetObject => Ok(()),
                _ => {
                    let empty_slots = BTreeSet::new();
                    self.validate_amount(&empty_slots, context, &FlowFacts::default(), vocabulary)
                }
            },
            Self::AttachedCount(_) => {
                let empty_slots = BTreeSet::new();
                self.validate_amount(&empty_slots, context, &FlowFacts::default(), vocabulary)
            }
            Self::EventValue(kind) => match kind {
                EventValueKind::TriggerSourceOnBattlefield => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerCondition)
                    }
                }
                EventValueKind::SourceAttackedThisTurn => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                intervening_if: Some(TriggerCondition::SourceDidNotAttackThisTurn),
                                ..
                            },
                            ..
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerCondition)
                    }
                }
                EventValueKind::DeclaredAttackersAtController => {
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Attacks {
                                    mode: AttackTriggerMode::OpponentsAtController { minimum: 2 },
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerCondition)
                    }
                }
                EventValueKind::MatchedOccurrences => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::UnboundTriggerCondition)
                    }
                }
                EventValueKind::ChosenX
                | EventValueKind::CastDuringOwnMain
                | EventValueKind::CastFromGraveyard
                | EventValueKind::PaidAlternative
                | EventValueKind::Bargained
                | EventValueKind::KickerCount(_)
                | EventValueKind::CreaturesDiedThisTurn
                | EventValueKind::LastSacrificeSucceeded
                | EventValueKind::LastActionSucceeded
                | EventValueKind::LastActionCount
                | EventValueKind::SelectedGraveyardExileCreatures
                | EventValueKind::EnteredKickerX
                | EventValueKind::EnteredKickerCount(_)
                | EventValueKind::PaidCreatureTaps
                | EventValueKind::PaidNamedCounters
                | EventValueKind::LargestDiscard
                | EventValueKind::DestroyedPermanents
                | EventValueKind::ExiledMilledCreaturePower
                | EventValueKind::MilledCardsManaValueSum
                | EventValueKind::MilledCardsManaValueMax
                | EventValueKind::DeclaredSubtypeAttackers
                | EventValueKind::TriggeringLifeChange
                | EventValueKind::CombatDamageToPlayer
                | EventValueKind::DamageReceived
                | EventValueKind::ManaSpentToCast
                | EventValueKind::ColorsSpentToCast
                | EventValueKind::ManaTypeSpentToCast(_)
                | EventValueKind::SnowManaSpentToCast
                | EventValueKind::TreasureManaSpentToCast
                | EventValueKind::CreatureManaSpentToCast
                | EventValueKind::CastFromOutsideHand
                | EventValueKind::CastFromExile
                | EventValueKind::GiftPromised
                | EventValueKind::Cleaved
                | EventValueKind::EvidenceCollected
                | EventValueKind::LastDieRoll
                | EventValueKind::CoinFlipsWon
                | EventValueKind::LastCoinFlipWon
                | EventValueKind::ChosenNumber
                | EventValueKind::ChosenOptionIndex
                | EventValueKind::VoteCount(_)
                | EventValueKind::WinningVoteOption
                | EventValueKind::LastNonlandDiscarded
                | EventValueKind::TriggeringSpellManaSpent
                | EventValueKind::AdditionalCostPaid => Ok(()),
            },
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.validate_condition(context, vocabulary)
            }
            Self::Add(a, b) | Self::Sub(a, b) => {
                a.validate_condition(context, vocabulary)?;
                b.validate_condition(context, vocabulary)
            }
            Self::DomainCount | Self::OpponentsCount | Self::Devotion(_) => Ok(()),
            Self::Conditional {
                left,
                right,
                then_value,
                else_value,
                ..
            } => {
                left.validate_condition(context, vocabulary)?;
                right.validate_condition(context, vocabulary)?;
                then_value.validate_condition(context, vocabulary)?;
                else_value.validate_condition(context, vocabulary)
            }
        }
    }
}
