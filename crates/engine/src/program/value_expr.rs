//! Unified composable numeric expression used by both effect amounts and branch conditions.
use super::{
    AttackTriggerMode, BodyContext, CardTypeMask, CastEntryKind, Comparison, CreatureTypeSlot,
    DamageTriggerRecipient, DefinitionError, DiesTriggerMode, FlowFacts, ObjectFilter,
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
}

/// Object statistic read by [`ValueExpr::ObjectStat`] or [`ValueExpr::Aggregate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStatKind {
    Power,
    Toughness,
    ManaValue,
}

/// Aggregation operator over matching battlefield objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatAggregate {
    Max,
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
    DestructionTarget,
    SacrificedCreature,
    LastSacrificedCreature,
    SacrificedPermanent,
    ExiledCostCreature,
    TriggeringSpell,
    FinalSaga,
}

/// Subject whose counters are counted by [`ValueExpr::CounterCount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterSubjectRef {
    AbilitySource,
    TriggerSource,
    DyingSource,
    ControllerExperience,
}

/// Attached permanents counted by [`ValueExpr::AttachedCount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachedCountKind {
    EquipmentOnSource,
    AurasAndEquipmentOnSource,
    CursesOnRelatedPlayer,
}

/// Player-level scalar statistic read by [`ValueExpr::PlayerStat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatExpr {
    ControllerLife,
    ControllerLifeAboveStarting,
    OpponentsLifeLostThisTurn,
    CardsDrawnThisTurn,
    CardsShortOfHandSize(u8),
    LargestHandSize,
    ControllerIsMonarch,
    ControlsGreatestCreature,
}

/// Subject tested by [`ValueExpr::ObjectMatches`] (evaluates to `1` if matched, `0` otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchObjectRef {
    RevealedTop,
    DiscardedCard,
    MovedLibraryCard,
    DestructionTarget,
    DyingCreature,
    SacrificedCreature,
    TargetObject,
}

/// Runtime event, stack, or resolution binding read by [`ValueExpr::EventValue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventValueKind {
    ChosenX,
    EnteredKickerX,
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
    SelectedGraveyardExileCreatures,
    SourceAttackedThisTurn,
    TriggerSourceOnBattlefield,
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

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        match self {
            Self::Count { filter, .. }
            | Self::Aggregate { filter, .. }
            | Self::ObjectMatches { filter, .. } => filter.rewrite_creature_type(from, to),
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.rewrite_creature_type(from, to);
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
                if filter.excludes_ability_source()
                    && !matches!(
                        context,
                        BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                    )
                {
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
                if filter.excludes_ability_source()
                    && !matches!(context, BodyContext::TriggerBound { .. })
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Ok(())
            }
            Self::ObjectStat { subject, stat } => match subject {
                ValueObjectRef::AbilitySource => {
                    if matches!(
                        context,
                        BodyContext::Activated { .. } | BodyContext::TriggerBound { .. }
                    ) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::TriggerSource => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                ValueObjectRef::TargetObject => match stat {
                    ObjectStatKind::Power | ObjectStatKind::Toughness => {
                        if facts.exact_creature_target {
                            Ok(())
                        } else {
                            Err(DefinitionError::OperandFamily)
                        }
                    }
                    ObjectStatKind::ManaValue => Ok(()),
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
                CounterSubjectRef::AbilitySource => {
                    if kind.is_some_and(|k| CounterKind::PERMANENT.contains(&k))
                        && matches!(
                            context,
                            BodyContext::Activated { .. } | BodyContext::TriggerBound { .. }
                        )
                    {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::TriggerSource => {
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
                CounterSubjectRef::ControllerExperience => Ok(()),
            },
            Self::AttachedCount(kind) => match kind {
                AttachedCountKind::EquipmentOnSource
                | AttachedCountKind::AurasAndEquipmentOnSource => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
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
                                    event: TriggerEvent::SpellCast { self_cast: true, .. },
                                    intervening_if: Some(TriggerCondition::HandSizeBelow(required)),
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
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
                | PlayerStatExpr::OpponentsLifeLostThisTurn
                | PlayerStatExpr::CardsDrawnThisTurn
                | PlayerStatExpr::LargestHandSize
                | PlayerStatExpr::ControllerIsMonarch
                | PlayerStatExpr::ControlsGreatestCreature => Ok(()),
            },
            Self::ObjectMatches { filter, .. } => filter
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::EventValue(kind) => match kind {
                EventValueKind::ChosenX => {
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        Err(DefinitionError::InvalidInstruction)
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
                EventValueKind::EnteredKickerX => {
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
                                event: TriggerEvent::DealsDamage {
                                    source,
                                    recipient: DamageTriggerRecipient::AnyPlayer,
                                    combat_only: true,
                                },
                                intervening_if: None,
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *source == ObjectFilter::self_object()
                            || *source == ObjectFilter::creature().controlled()
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
                                    subject,
                                    linked_player: false,
                                },
                                ..
                            },
                            ..
                        } if *subject == ObjectFilter::self_object()
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
                | EventValueKind::SelectedGraveyardExileCreatures
                | EventValueKind::SourceAttackedThisTurn
                | EventValueKind::TriggerSourceOnBattlefield => Ok(()),
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
            Self::Literal(_) | Self::PlayerStat(_) | Self::CountAttackingController => Ok(()),
            Self::Count { filter, .. } | Self::Aggregate { filter, .. } => filter
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
                    MatchObjectRef::DestructionTarget
                    | MatchObjectRef::TargetObject
                    | MatchObjectRef::RevealedTop
                    | MatchObjectRef::DiscardedCard
                    | MatchObjectRef::MovedLibraryCard => {
                        if vocab_ok {
                            Ok(())
                        } else {
                            Err(DefinitionError::InvalidInstruction)
                        }
                    }
                }
            }
            Self::CounterCount { subject, kind } => match subject {
                CounterSubjectRef::AbilitySource => {
                    if kind.is_some_and(|k| CounterKind::PERMANENT.contains(&k))
                        && matches!(context, BodyContext::Activated { .. })
                    {
                        Ok(())
                    } else {
                        Err(DefinitionError::InvalidInstruction)
                    }
                }
                CounterSubjectRef::ControllerExperience => Ok(()),
                CounterSubjectRef::TriggerSource | CounterSubjectRef::DyingSource => {
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
                | EventValueKind::SelectedGraveyardExileCreatures
                | EventValueKind::EnteredKickerX
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
                | EventValueKind::DamageReceived => Ok(()),
            },
            Self::Scale { expr, .. } | Self::DivConst { expr, .. } => {
                expr.validate_condition(context, vocabulary)
            }
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
