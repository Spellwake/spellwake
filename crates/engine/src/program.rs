//! Authored characteristics and executable capabilities under one definition owner.
//! Constructors assemble authored input. EffectWindow validates the complete
//! record before accepting it into the sole program map.
use crate::{
    activated::ActivatedAbility,
    characteristics::{
        CharacteristicDefinitionError, Color, KeywordAbility, PowerToughness,
        PrintedPowerToughness, ResolvedDuration, ResolvedPartsSpec, StaticContinuousAbility,
    },
    land::LandType,
    mana::{ManaAbility, ManaCost},
    turn::Step,
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::collections::{BTreeMap, BTreeSet};
/// The alternative price offered by a Miracle trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiracleCost {
    pub mana: ManaCost,
    pub x_symbols: u8,
}

/// An optional additional mana cost identified by its position in the printed definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdditionalCastCost {
    /// If paid once, select every printed mode.
    Entwine(ManaCost),
    Kicker(ManaCost),
    /// An optional kicker cost with one generic X in addition to its fixed component.
    KickerX(ManaCost),
    /// An optional kicker cost with non-mana extra costs (e.g. sacrifice/discard/pay life) and optional mana.
    KickerExtra {
        mana: ManaCost,
        extra: Vec<crate::activated::ExtraActivationCost>,
    },
    Multikicker(ManaCost),
    Replicate(ManaCost),
    Buyback(ManaCost),
}
impl AdditionalCastCost {
    pub(crate) fn mana(&self) -> &ManaCost {
        match self {
            Self::Entwine(cost)
            | Self::KickerX(cost)
            | Self::Kicker(cost)
            | Self::KickerExtra { mana: cost, .. }
            | Self::Multikicker(cost)
            | Self::Replicate(cost)
            | Self::Buyback(cost) => cost,
        }
    }
}
/// A permission on the physical card in its owner's graveyard (or graveyard/exile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraveyardSelfCast {
    Unconditional,
    WhileControllingSubtype(CreatureTypeId),
    /// Cast from owner's graveyard by paying these additional non-mana costs in addition to mana cost.
    WithExtraCost(Vec<crate::activated::ExtraActivationCost>),
    /// Cast from owner's graveyard or exile (e.g. Eternal Scourge / Squee, the Immortal).
    GraveyardOrExile,
    /// Cast from owner's graveyard for an alternative mana cost plus optional extra costs.
    WithAlternativeCost {
        mana: ManaCost,
        extra: Vec<crate::activated::ExtraActivationCost>,
    },
}

/// Printed special action from hand and the corresponding exile permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExileAction {
    /// CR 702.143. Pay two to foretell; the printed price is for casting later.
    Foretell(ManaCost),
    /// CR 702.62. Pay the printed price to suspend with this many time counters.
    Suspend { counters: u16, price: ManaCost },
}
impl ExileAction {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Foretell(cost) => cost.valid(),
            Self::Suspend { counters, price } => *counters > 0 && price.valid(),
        }
    }
}
/// Permanent spell alternatives which create a delayed end-step trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndStepCast {
    Dash,
    Blitz,
    Warp,
}
/// Permanents eligible to tap for an alternative casting cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastTapQuality {
    Artifact,
    FlyingCreature,
}
/// Conditional alternative mana costs available in an ordinary individual-turn game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalCastCost {
    /// Alternative price paid by exiling exactly this many cards of the given color from hand.
    ExileHandCards {
        mana: ManaCost,
        count: u8,
        color: crate::characteristics::Color,
    },
    /// Fixed mana and a single cost tapping multiple controlled permanents.
    TapPermanents {
        mana: ManaCost,
        count: u8,
        quality: CastTapQuality,
    },
    /// Alternative cost while the caster controls any designated commander on the battlefield.
    ControlsCommander(ManaCost),
    /// The caster has cast another spell this turn.
    Surge(ManaCost),
    /// Combat damage to a player this turn by a source sharing one of this spell's creature types.
    Prowl(ManaCost),
    /// Combat damage to a player by an Assassin creature or commander controlled by the caster.
    Freerunning(ManaCost),
    /// Pay the alternative price in the graveyard after discarding this card this turn.
    Mayhem(ManaCost),
    /// Return a tapped creature the caster controls as part of paying the alternative price.
    WebSlinging(ManaCost),
    /// CR 702.103. Cast an enchantment creature as an Aura with enchant creature.
    Bestow(ManaCost),
    /// Alternative cost to cast this creature as a mutating creature spell.
    Mutate(ManaCost),
    /// CR 702.190. Return an unblocked attacker during your declare blockers step.
    Sneak(ManaCost),
    /// CR 702.74 alternative followed by a separately triggered sacrifice.
    Evoke(ManaCost),
    /// Pay the alternative price to replace each occurrence of “target” with “each”.
    Overload {
        mana: ManaCost,
        each: Box<ExecutableBody>,
    },
    /// Fixed-price cleave and the executable rules remaining after bracket removal.
    Cleave {
        mana: ManaCost,
        unbracketed: Box<ExecutableBody>,
    },
    /// Sacrifice a permanent of this type and reduce the generic cost by its mana value.
    Emerge { mana: ManaCost, from: CardType },
    /// A permanent cast this way is tracked for a delayed ability at the next end step.
    EndStep { kind: EndStepCast, mana: ManaCost },
    /// An opponent of the caster has lost life this turn.
    Spectacle(ManaCost),
    /// CR 702.138. Pay mana and exile other cards to cast from the owner's graveyard.
    Escape {
        mana: ManaCost,
        exile: u16,
        /// Printed number of +1/+1 counters the creature escapes with.
        entry_counters: u16,
    },
    /// Alternative mana cost (including `{0}` for "without paying its mana cost") available when `condition` holds.
    Condition {
        condition: Box<ResolutionCondition>,
        mana: ManaCost,
    },
    /// CR 702.113 Awaken N - {cost}: cast for alternative cost and animate target land with N +1/+1 counters.
    Awaken { mana: ManaCost, counters: u8 },
    /// CR 702.176 Impending N - {cost}: cast for alternative cost, enters with N time counters and isn't a creature.
    Impending { mana: ManaCost, counters: u8 },
    /// CR 702.180 Harmonize {cost}: cast from graveyard; may tap a creature to reduce generic cost by its power.
    Harmonize(ManaCost),
    /// CR 702.48 <Subtype> offering: cast sacrificing a matching permanent to reduce cost by its mana cost.
    Offering {
        mana: ManaCost,
        subtype: CreatureTypeId,
    },
    /// Generalized alternative cost (mana plus composable non-mana extra costs, optionally gated by a world condition).
    Alternative {
        condition: Option<Box<ResolutionCondition>>,
        mana: ManaCost,
        extra: Vec<crate::activated::ExtraActivationCost>,
    },
}
impl ConditionalCastCost {
    pub(crate) fn mana(&self) -> &ManaCost {
        match self {
            Self::ExileHandCards { mana: price, .. }
            | Self::TapPermanents { mana: price, .. }
            | Self::EndStep { mana: price, .. }
            | Self::Emerge { mana: price, .. }
            | Self::Overload { mana: price, .. }
            | Self::Cleave { mana: price, .. }
            | Self::Evoke(price)
            | Self::Sneak(price)
            | Self::Mutate(price)
            | Self::Bestow(price)
            | Self::Prowl(price)
            | Self::Freerunning(price)
            | Self::Mayhem(price)
            | Self::WebSlinging(price)
            | Self::ControlsCommander(price)
            | Self::Surge(price)
            | Self::Spectacle(price)
            | Self::Escape { mana: price, .. }
            | Self::Condition { mana: price, .. }
            | Self::Awaken { mana: price, .. }
            | Self::Impending { mana: price, .. }
            | Self::Harmonize(price)
            | Self::Offering { mana: price, .. }
            | Self::Alternative { mana: price, .. } => price,
        }
    }
}
/// Kind of gift promised by CR 702.174 Gift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftKind {
    Card,
    Fish,
    Food,
    Treasure,
    TappedFish,
    ExtraTurn,
}
/// Static counters applied as a permanent enters (CR 702.44 and 702.54).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryCounterAbility {
    /// Sacrifice any number of controlled creatures as this permanent enters.
    Devour(u8),
    /// Put the value chosen for X on the resolving spell in +1/+1 counters.
    ChosenXPlusOne,
    /// Unconditional +1/+1 counters from a printed modular or graft instance.
    FixedPlusOne(RulesInteger),
    /// Unconditional -1/-1 counters with which this permanent enters.
    FixedMinusOne(RulesInteger),
    /// Charge counters with which this permanent enters.
    FixedCharge(RulesInteger),
    /// Additional entry loyalty placed by a printed ability, on any permanent type.
    FixedLoyalty(RulesInteger),
    /// Named counters of any non-P/T permanent counter kind with which this permanent enters.
    FixedNamed {
        kind: crate::effects::CounterKind,
        amount: RulesInteger,
    },
    /// Dynamically counted entry counters of any permanent counter kind (`+1/+1`, `-1/-1`, or named/keyword).
    DynamicCounter {
        kind: crate::effects::CounterKind,
        amount: ValueExpr,
    },
    /// Dynamically counted entry counters placed only if `condition` holds on entry.
    ConditionalCounter {
        condition: Box<ResolutionCondition>,
        kind: crate::effects::CounterKind,
        amount: ValueExpr,
    },
    /// Suspect counters equal to the most creatures controlled by any one player before entry.
    SuspectForGreatestCreatureCount,
    /// One charge counter for each payment of the indicated kicker cost.
    ChargePerKicker(usize),
    /// If the specified kicker cost was paid, count opposing nonbasic lands before entry.
    KickedOpposingNonbasicLands(usize),
    /// Each color of mana spent gives one +1/+1 counter to a creature or one charge counter otherwise.
    Sunburst,
    /// A fixed number of counters if any opponent was dealt damage this turn.
    Bloodthirst(RulesInteger),
    /// One counter for each point of damage all opponents were dealt this turn.
    BloodthirstX,
    /// Place these additional +1/+1 counters if a creature died this turn.
    MorbidPlusOne(RulesInteger),
    /// Count other permanents and own graveyard cards with this creature subtype.
    ControlledAndGraveyardSubtypePlusOne(CreatureTypeId),
    /// While in your graveyard, give each creature of this subtype you control
    /// an additional +1/+1 counter as it enters.
    GraveyardControlledSubtypePlusOne(CreatureTypeId),
    /// CR 702.38 Amplify N: reveal any number of cards from hand sharing a creature type; enter with N counters each.
    Amplify(u8),
}
/// Objects counted by an ordinary spell's printed generic cost reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCostCount {
    BattlefieldCreatures,
    AttackingCreatures,
    ControlledCreaturesWithPlusOneCounters,
    OwnGraveyardCreatures,
    OwnGraveyardInstantsOrSorceries,
}
/// Battlefield value used for a spell's own generic cost reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCostValue {
    TotalCreaturePower,
    GreatestCreaturePower,
    NoncreatureArtifactManaValue,
    NoncreatureEnchantmentManaValue,
}
/// Each printed instance further reduces the generic component of a casting cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastReduction {
    /// Optional additional graveyard creature exiles, reducing generic mana per selected card.
    ExileGraveyardCreatures { per_card: u8 },
    /// Reduce the generic cost by one per matching object.
    PerObject(CastCostCount),
    /// Compute the reduction from permanents the caster currently controls.
    BattlefieldValue(CastCostValue),
    /// CR 702.41 counts matching permanents controlled by the caster.
    Affinity(Box<crate::characteristics::PermanentSelector>),
    /// CR 702.125 counts the caster's opponents in the current game.
    Undaunted,
    /// Reduce generic mana when the target spell or ability targets a sufficiently powerful controlled creature.
    StackTargetThreatensCreature { minimum_power: u8, discount: u8 },
    /// Reduce generic cost by `per_card` for each object matching `filter` in `zone`.
    CountZone {
        zone: ValueZone,
        filter: Box<ObjectFilter>,
        per_card: u8,
    },
    /// Reduce generic cost by `discount` when `condition` holds at cast time.
    Conditional {
        condition: Box<ResolutionCondition>,
        discount: u8,
    },
    /// Reduce generic cost by `discount` when this spell targets an object matching `filter`.
    IfTargets {
        filter: Box<ObjectFilter>,
        discount: u8,
    },
    /// Reduce generic cost by a world-evaluable expression at cast time.
    Dynamic(Box<ValueExpr>),
}
mod growth;
mod transfers;
pub use growth::GrowthKeyword;
pub use transfers::TransferKeyword;
mod control;
#[cfg(test)]
mod control_tests;
mod object_filter;
mod spell_filter;
mod trigger_spec;
mod value_expr;
use control::FlowFacts;
pub use control::{Comparison, ResolutionCondition};
pub use object_filter::{
    CardTypeMask, ColorMask, CombatFilter, ControllerFilter, DynamicStatAnchor, EffectSubject,
    ExactNameKey, ManaValueFilter, ObjectClause, ObjectColorFilter, ObjectFilter,
    ObjectFilterFlags, PowerFilter, SingleSubtype, SourceRelation, SubtypeFilter, SubtypeList,
    SupertypeMask, ToughnessFilter,
};
pub use spell_filter::{SpellCastZoneFilter, SpellFilter, SpellSpecialFilter, SpellTargetShape};
pub use trigger_spec::{
    AttackTriggerMode, BlockTriggerKind, CastEntryKind, CounterTriggerKind, DamageTriggerRecipient,
    DiesTriggerMode, PlanarTriggerEvent, PlayerTriggerAction, StateThresholdKind,
    StatusTriggerKind, TriggerCondition, TriggerEvent, TriggerFromZone, TriggerKeywordEvent,
    TriggerPlayerScope, TriggerSpec, TriggerZone,
};
pub use value_expr::{
    AttachedCountKind, CounterSubjectRef, EventValueKind, MatchObjectRef, ObjectStatKind,
    OpponentTurnStat, PhaseCheck, PlayerStatExpr, SourceStateExpr, SourceZoneKind, StatAggregate,
    ValueExpr, ValueObjectRef, ValueZone,
};
#[cfg(test)]
mod cast_trigger_tests;
#[cfg(test)]
#[path = "program/characteristics_tests.rs"]
mod characteristics_tests;
#[cfg(test)]
mod color_definition_tests;
#[cfg(test)]
mod defining_types_pt_tests;
#[cfg(test)]
mod optional_payment_tests;
#[cfg(test)]
mod permanent_copy_tests;
#[cfg(test)]
mod resolution_selection_tests;
#[cfg(test)]
mod spell_role_lifetime_tests;

/// A body-local value name; admission checks its producer before each use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatureTypeSlot(u16);
impl CreatureTypeSlot {
    pub fn new(ordinal: u16) -> Self {
        Self(ordinal)
    }
    pub fn ordinal(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BodyContext<'a> {
    TapCreatureCost,
    Unbound,
    Activated {
        named_counter_cost: bool,
    },
    SacrificedCreature,
    SacrificedPermanent,
    RevealedCreature,
    ExiledCreatureCost,
    TriggerBound {
        condition: &'a TriggerSpec,
        grouping: TriggerGrouping,
    },
}
impl BodyContext<'_> {
    pub(crate) const ACTIVATED: Self = Self::Activated {
        named_counter_cost: false,
    };

    /// Whether this body executes in the context of a battlefield/permanent ability with a source object.
    pub(crate) const fn has_source(self) -> bool {
        matches!(
            self,
            Self::TriggerBound { .. }
                | Self::Activated { .. }
                | Self::SacrificedCreature
                | Self::SacrificedPermanent
                | Self::RevealedCreature
                | Self::ExiledCreatureCost
        )
    }
}

/// A player derived from the actual resolving entry, never supplied by a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvingPlayer {
    /// The resolving entry's captured controller, including a triggered ability.
    Controller,
    /// The single actual participant in the entry's complete retained causes.
    /// Admission requires a participant-bearing trigger and singular grouping.
    UniqueRelatedPlayer,
    /// Current or last known controller of the permanent that dealt combat damage.
    CombatDamageSourceController,
    HighestEntrantController,
    TriggeringAttackerController,
    /// The exact bound current spell entry's controller, including spell copies.
    /// This consumes the body's required [`BodyTarget::single_spell`] target.
    /// A departed target spell supplies its controller through last known information.
    TargetSpellController,
    /// The controller captured from the targeted permanent immediately before its exile.
    ExiledTargetController,
    /// The controller observed immediately before the preceding destruction instruction.
    DestructionTargetController,
    /// The body's exact bound player, consuming [`BodyTarget::SINGLE_PLAYER`].
    /// A creature-or-player target does not guarantee this player capability.
    TargetPlayer,
    /// The controller of the body's target (permanent, spell, or planeswalker), or the targeted
    /// player itself if the target is a player.
    TargetController,
    /// The owner of the body's targeted permanent, graveyard card, or spell.
    TargetOwner,
    /// The active player of the current turn.
    ActivePlayer,
    /// The player currently enchanted by the ability source.
    EnchantedPlayer,
    /// The defending player in the current combat phase.
    DefendingPlayer,
    /// The player chosen earlier during this resolution (via `Instruction::PromptChoice`).
    ChosenPlayer,
    /// The current controller of the permanent to which the ability source is attached.
    AttachedHostController,
    /// The current or last-known controller of the triggering object.
    TriggerSubjectController,
    /// The owner of the triggering object.
    TriggerSubjectOwner,
    /// The player chosen as the ability source entered (via `EntryTap::ChoosePlayer` / `ChooseOpponent`).
    SourceChosenPlayer,
    /// The current player bound by an enclosing `Instruction::ForEachPlayerQuery` or `OfferAnyPlayer`.
    LoopPlayer,
    /// An opponent of the resolving controller chosen at random during resolution.
    RandomOpponent,
    /// The owner of the resolving spell or ability's source object.
    SourceOwner,
}
impl ResolvingPlayer {
    pub(super) fn validate(
        self,
        context: BodyContext<'_>,
        target: BodyTarget,
        definitely_exiled_target: bool,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        match self {
            Self::Controller => Ok(None),
            Self::DestructionTargetController => Err(DefinitionError::InvalidInstruction),
            Self::ExiledTargetController => {
                if definitely_exiled_target {
                    Ok(None)
                } else {
                    Err(DefinitionError::InvalidInstruction)
                }
            }
            Self::TriggeringAttackerController => {
                if matches!(
                    context,
                    BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::Attacks {
                                mode: AttackTriggerMode::AtControllerOrPlaneswalker,
                                ..
                            },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    }
                ) {
                    Ok(None)
                } else {
                    Err(DefinitionError::UnguaranteedRelatedPlayer)
                }
            }
            Self::HighestEntrantController => {
                if matches!(
                    context,
                    BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::Enters {
                                subject,
                                from_zone: TriggerFromZone::Any,
                                cast_origin: None,
                                attached_to_creature: false,
                            },
                            intervening_if: None,
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *subject == ObjectFilter::creature().other_than_ability_source()
                ) {
                    Ok(None)
                } else {
                    Err(DefinitionError::UnguaranteedRelatedPlayer)
                }
            }
            Self::CombatDamageSourceController => {
                if matches!(
                    context,
                    BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::DealsDamage {
                                source: ObjectFilter::Clause(c),
                                recipient: DamageTriggerRecipient::AnyPlayer,
                                combat_only: true,
                            },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if c.controller == ControllerFilter::Any && matches!(c.subtypes, SubtypeFilter::Has(_))
                ) {
                    Ok(None)
                } else {
                    Err(DefinitionError::UnguaranteedRelatedPlayer)
                }
            }
            Self::UniqueRelatedPlayer => match context {
                BodyContext::Unbound
                | BodyContext::Activated { .. }
                | BodyContext::SacrificedPermanent
                | BodyContext::RevealedCreature
                | BodyContext::ExiledCreatureCost
                | BodyContext::SacrificedCreature
                | BodyContext::TapCreatureCost => Err(DefinitionError::UnboundResolvingPlayer),
                BodyContext::TriggerBound {
                    condition,
                    grouping,
                } => {
                    let singular = match grouping {
                        TriggerGrouping::EachOccurrence
                        | TriggerGrouping::OneOrMoreByPlayer
                        | TriggerGrouping::OneOrMoreOnceEachTurn => true,
                        TriggerGrouping::OneOrMoreTogether => false,
                    };
                    if singular && condition.captures_related_player() {
                        Ok(None)
                    } else {
                        Err(DefinitionError::UnguaranteedRelatedPlayer)
                    }
                }
            },
            Self::TargetSpellController => match target {
                BodyTarget::Spell {
                    include_abilities: false,
                    ..
                } => Ok(Some(OperandFamily::Spell)),
                BodyTarget::Spell {
                    include_abilities: true,
                    ..
                }
                | BodyTarget::StackEntry(_) => Ok(Some(OperandFamily::StackEntry)),
                BodyTarget::None
                | BodyTarget::Object { .. }
                | BodyTarget::Player { .. }
                | BodyTarget::AnyTarget(_) => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::TargetPlayer => match target {
                BodyTarget::Player { .. } => Ok(Some(OperandFamily::Player)),
                BodyTarget::None
                | BodyTarget::Object { .. }
                | BodyTarget::AnyTarget(_)
                | BodyTarget::Spell { .. }
                | BodyTarget::StackEntry(_) => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::TargetController => match target {
                BodyTarget::Player { .. } => Ok(Some(OperandFamily::Player)),
                BodyTarget::Spell {
                    include_abilities: false,
                    ..
                } => Ok(Some(OperandFamily::Spell)),
                BodyTarget::Spell {
                    include_abilities: true,
                    ..
                }
                | BodyTarget::StackEntry(StackTargetKind::AnyEntry) => {
                    Ok(Some(OperandFamily::StackEntry))
                }
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                } => Ok(Some(OperandFamily::Permanent)),
                BodyTarget::AnyTarget(
                    AnyTargetKind::Any
                    | AnyTargetKind::CreatureOrPlayer
                    | AnyTargetKind::PlayerOrPlaneswalker
                    | AnyTargetKind::OpponentOrPlaneswalker,
                ) => Ok(Some(OperandFamily::DamageRecipient)),
                _ => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::TargetOwner => match target {
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                } => Ok(Some(OperandFamily::Permanent)),
                BodyTarget::Object {
                    zone: TargetZone::OwnGraveyard | TargetZone::AnyGraveyard { .. },
                    ..
                } => Ok(Some(OperandFamily::GraveyardCard)),
                BodyTarget::Object {
                    zone: TargetZone::Exile { .. },
                    ..
                } => Ok(Some(OperandFamily::ExiledCard)),
                BodyTarget::Spell {
                    include_abilities: false,
                    ..
                } => Ok(Some(OperandFamily::Spell)),
                BodyTarget::AnyTarget(
                    AnyTargetKind::Any
                    | AnyTargetKind::CreatureOrPlayer
                    | AnyTargetKind::PlayerOrPlaneswalker
                    | AnyTargetKind::OpponentOrPlaneswalker,
                ) => Ok(Some(OperandFamily::DamageRecipient)),
                _ => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::ActivePlayer
            | Self::DefendingPlayer
            | Self::ChosenPlayer
            | Self::LoopPlayer
            | Self::RandomOpponent => Ok(None),
            Self::EnchantedPlayer
            | Self::AttachedHostController
            | Self::SourceChosenPlayer
            | Self::SourceOwner => {
                if context.has_source() {
                    Ok(None)
                } else {
                    Err(DefinitionError::UnboundResolvingPlayer)
                }
            }
            Self::TriggerSubjectController | Self::TriggerSubjectOwner => {
                if matches!(context, BodyContext::TriggerBound { .. }) {
                    Ok(None)
                } else {
                    Err(DefinitionError::UnboundResolvingPlayer)
                }
            }
        }
    }
}

/// Players instructed to perform one selected-object effect during resolution.
/// Runtime derives participants from the resolving entry and active-player order;
/// neither a decision reply nor a caller-authored seat list supplies this scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedPlayers {
    /// One player derived through the same native role as other resolving work.
    One(ResolvingPlayer),
    /// Every current participant, with choices made in actual APNAP order.
    EachPlayer,
    /// All current participants except the resolving controller.
    EachOtherPlayer,
    /// The resolving controller’s opponents choose in active-player, nonactive-player order.
    EachOpponent,
}
impl AffectedPlayers {
    fn validate(
        self,
        context: BodyContext<'_>,
        target: BodyTarget,
        definitely_exiled_target: bool,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        match self {
            Self::One(player) => player.validate(context, target, definitely_exiled_target),
            Self::EachPlayer | Self::EachOtherPlayer | Self::EachOpponent => Ok(None),
        }
    }
}

/// Source-independent current-type restrictions for an otherwise eligible permanent.
/// Empty sets impose no type restriction. Whole-body admission rejects an overlap
/// with [`DefinitionError::InvalidInstruction`]; physical zone/control eligibility
/// remains a separate runtime requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentTypeFilter {
    /// When populated, at least one of these evaluated types must be present.
    pub any_types: BTreeSet<CardType>,
    /// Every listed type must occur in the permanent's evaluated current types.
    pub required_types: BTreeSet<CardType>,
    /// None of these types may occur in its evaluated current types.
    pub forbidden_types: BTreeSet<CardType>,
    /// Optional additional `ObjectFilter` predicate (subtypes, colors, tapped, token, stat bounds, keywords).
    pub object_filter: Option<ObjectFilter>,
}

/// Bounded eligibility and destination policies for a controller's library search.
/// Source of a fixed player attachment for a searched Curse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraSearchHost {
    TargetPlayer,
    EnchantedPlayerWithDifferentCurseNames,
}

/// A specified set of cards from one library, distinct from the keyword action mill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryCohort {
    BottomCardOfTargetPlayer,
    DefendingPlayerRevealsThroughLand,
}

/// Zone(s) inspected by a library search instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSource {
    /// The searching player's own library.
    OwnLibrary,
    /// The searching player's own library and graveyard.
    OwnLibraryOrGraveyard,
    /// The targeted opponent's library.
    TargetOpponentLibrary,
    /// The targeted player's library.
    TargetPlayerLibrary,
    /// Runtime-bound library of a specific seat (not statically authored).
    PlayerLibrary(crate::priority::SeatId),
}

/// Card-characteristic predicate for a library search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchCardMatcher {
    /// Any card in the searched zone.
    Any,
    /// Any nonland card.
    Nonland,
    /// A basic land card.
    BasicLand,
    /// A land card matching any listed land subtype.
    LandSubtypes {
        subtypes: BTreeSet<LandType>,
        /// When true, the card must currently have card type Land in addition to the subtype.
        require_land_type: bool,
    },
    /// Any card having at least one of the listed card types.
    CardTypes(BTreeSet<CardType>),
    /// A card having the specified creature subtype.
    CreatureSubtype(CreatureTypeId),
    /// An Equipment card.
    Equipment,
    /// A Curse aura eligible to attach to the resolved host.
    CurseForHost(AuraSearchHost),
    /// A card having any of the specified English names.
    AnyOfNames(Vec<spellwake_contracts::names::EnglishCardName>),
    /// A card sharing a name with a runtime-resolved reference object (CR 201.3),
    /// optionally further filtered by `extra`.
    SameNameAs {
        reference: MoveSubjectRef,
        extra: Option<Box<ObjectFilter>>,
    },
    /// Any card matching a general orthogonal object filter.
    Filtered(Box<ObjectFilter>),
}

/// Mana-value constraint composed onto a [`SearchCardMatcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchManaValue {
    /// Exact mana value equality (e.g. transmute, transfigure).
    Exact(RulesInteger),
    /// Mana value greater than or equal to the given bound.
    AtLeast(u32),
    /// Mana value less than or equal to the given bound.
    AtMost(u32),
    /// Mana value less than or equal to the resolving entry's announced X.
    AtMostAnnouncedX,
    /// Mana value equal to the resolving source's current or last-known counter count.
    EqualToSourceCounters(crate::effects::CounterKind),
    /// Card's printed mana cost exactly matches one of the listed mana costs.
    ExactManaCostOneOf(Vec<ManaCost>),
}

/// Cardinality policy for a library search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchCount {
    /// Find up to `n` matching cards (fail-to-find permitted).
    UpTo(u8),
    /// Find up to a dynamically evaluated number of matching cards (fail-to-find permitted).
    UpToExpr(Box<ValueExpr>),
    /// Unrestricted required search: find exactly `n` cards (or the whole library if smaller).
    Exact(std::num::NonZeroU8),
    /// Find up to the number of players controlling more lands than the searcher.
    UpToPlayersWithMoreLands,
}

/// Cross-card constraint when selecting a pair of cards in one search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchPairConstraint {
    /// Both chosen cards must share at least one land subtype.
    ShareLandType,
    /// When two cards are chosen, one must have the first land subtype and the other the second.
    DistinctLandTypes(LandType, LandType),
}

/// Post-entry rule attached to cards put onto the battlefield by a library search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBattlefieldPostEntry {
    /// Ordinary battlefield entry.
    None,
    /// Grant haste and schedule return to hand at the next end step.
    HasteAndReturnAtEndStep,
    /// Enter attached to the resolved Curse host player.
    AttachToCurseHost(AuraSearchHost),
    /// Transfigure entry (retains `ZoneReason::TransfigureEntry` and source mana-value binding).
    Transfigure,
}

/// Exile linkage policy when a search exiles its selected cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchExileLink {
    /// Ordinary exile without source linkage.
    None,
    /// Link the exiled cards to the resolving trigger source.
    LinkedToTriggerSource,
    /// Append exiled cards to the resolving cursor's free-cast found-spell cohort.
    FreeCastCohort,
}

/// Destination policy for cards found by a [`LibrarySearch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDestination {
    /// Put found card(s) into the searching player's hand.
    Hand,
    /// Put found card(s) onto the battlefield under the searching player's control.
    Battlefield {
        tapped: bool,
        post_entry: SearchBattlefieldPostEntry,
    },
    /// Shuffle the library, then put the found card on top of it.
    LibraryTop,
    /// Exile the found card(s).
    Exile { link: SearchExileLink },
    /// Put found card(s) into the searching player's graveyard.
    Graveyard,
    /// Put one found card onto the battlefield tapped and the other into hand.
    SplitBattlefieldTappedAndHand,
    /// Put into hand, or optionally onto the battlefield if a creature died this turn (Morbid).
    HandOrBattlefieldIfCreatureDied,
    /// Put one found card into hand and the other into graveyard.
    SplitHandAndGraveyard,
}

/// Composable library search specification replacing per-card search variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySearch {
    /// Player performing the search and receiving its result.
    pub searcher: ResolvingPlayer,
    /// Zone(s) inspected by the search.
    pub source: SearchSource,
    /// Characteristic filter for eligible cards.
    pub matcher: SearchCardMatcher,
    /// Optional mana-value bound composed onto `matcher`.
    pub mana_value: Option<SearchManaValue>,
    /// Number of cards that may or must be found.
    pub count: SearchCount,
    /// Optional cross-card constraint when selecting a pair.
    pub pair_constraint: Option<SearchPairConstraint>,
    /// Destination for found cards.
    pub destination: SearchDestination,
    /// Whether selected cards are publicly revealed.
    pub reveal: bool,
}

impl LibrarySearch {
    /// Search your own library for up to one matching card, reveal it, and put it into hand.
    #[must_use]
    pub fn to_hand(matcher: SearchCardMatcher) -> Self {
        Self {
            searcher: ResolvingPlayer::Controller,
            source: SearchSource::OwnLibrary,
            matcher,
            mana_value: None,
            count: SearchCount::UpTo(1),
            pair_constraint: None,
            destination: SearchDestination::Hand,
            reveal: true,
        }
    }

    /// Search your own library for up to one matching card and put it onto the battlefield.
    #[must_use]
    pub fn to_battlefield(matcher: SearchCardMatcher, tapped: bool) -> Self {
        Self {
            searcher: ResolvingPlayer::Controller,
            source: SearchSource::OwnLibrary,
            matcher,
            mana_value: None,
            count: SearchCount::UpTo(1),
            pair_constraint: None,
            destination: SearchDestination::Battlefield {
                tapped,
                post_entry: SearchBattlefieldPostEntry::None,
            },
            reveal: false,
        }
    }

    /// Search your own library for one or more cards without restriction and put them into hand.
    #[must_use]
    pub fn any_to_hand(count: std::num::NonZeroU8) -> Self {
        Self {
            searcher: ResolvingPlayer::Controller,
            source: SearchSource::OwnLibrary,
            matcher: SearchCardMatcher::Any,
            mana_value: None,
            count: SearchCount::Exact(count),
            pair_constraint: None,
            destination: SearchDestination::Hand,
            reveal: false,
        }
    }

    /// Search your own library for a single card type and put it into hand.
    #[must_use]
    pub fn card_type_to_hand(kind: CardType) -> Self {
        Self::to_hand(SearchCardMatcher::CardTypes([kind].into()))
    }

    /// Search your own library for a land subtype and put it into hand.
    #[must_use]
    pub fn land_subtype_to_hand(kind: LandType) -> Self {
        Self::to_hand(SearchCardMatcher::LandSubtypes {
            subtypes: [kind].into(),
            require_land_type: false,
        })
    }

    /// Search your own library for any listed land subtype and put it onto the battlefield.
    #[must_use]
    pub fn land_subtypes_to_battlefield(subtypes: BTreeSet<LandType>, tapped: bool) -> Self {
        Self::to_battlefield(
            SearchCardMatcher::LandSubtypes {
                subtypes,
                require_land_type: true,
            },
            tapped,
        )
    }

    /// Override the player performing the search.
    #[must_use]
    pub fn with_searcher(mut self, searcher: ResolvingPlayer) -> Self {
        self.searcher = searcher;
        self
    }

    /// Override the searched zone(s).
    #[must_use]
    pub fn with_source(mut self, source: SearchSource) -> Self {
        self.source = source;
        self
    }

    /// Attach a mana-value constraint.
    #[must_use]
    pub fn with_mana_value(mut self, mana_value: SearchManaValue) -> Self {
        self.mana_value = Some(mana_value);
        self
    }

    /// Override the search cardinality.
    #[must_use]
    pub fn with_count(mut self, count: SearchCount) -> Self {
        self.count = count;
        self
    }

    /// Attach a pair selection constraint.
    #[must_use]
    pub fn with_pair_constraint(mut self, constraint: SearchPairConstraint) -> Self {
        self.pair_constraint = Some(constraint);
        self
    }

    /// Override the search destination.
    #[must_use]
    pub fn with_destination(mut self, destination: SearchDestination) -> Self {
        self.destination = destination;
        self
    }

    /// Override whether selected cards are publicly revealed.
    #[must_use]
    pub fn with_reveal(mut self, reveal: bool) -> Self {
        self.reveal = reveal;
        self
    }

    /// Whether this search can put cards onto the battlefield.
    #[must_use]
    pub fn can_enter_battlefield(&self) -> bool {
        matches!(
            self.destination,
            SearchDestination::Battlefield { .. }
                | SearchDestination::SplitBattlefieldTappedAndHand
                | SearchDestination::HandOrBattlefieldIfCreatureDied
        )
    }

    /// Whether battlefield entries from this search enter tapped.
    #[must_use]
    pub fn enters_tapped(&self) -> bool {
        matches!(
            self.destination,
            SearchDestination::Battlefield { tapped: true, .. }
                | SearchDestination::SplitBattlefieldTappedAndHand
        )
    }

    /// Whether this search exiles its selected cards.
    #[must_use]
    pub fn is_exile_destination(&self) -> bool {
        matches!(self.destination, SearchDestination::Exile { .. })
    }

    /// Whether this search links exiled cards to its trigger source.
    #[must_use]
    pub fn is_linked_exile(&self) -> bool {
        matches!(
            self.destination,
            SearchDestination::Exile {
                link: SearchExileLink::LinkedToTriggerSource
            }
        )
    }

    /// Whether this search is a transfigure search.
    #[must_use]
    pub fn is_transfigure(&self) -> bool {
        matches!(
            self.destination,
            SearchDestination::Battlefield {
                post_entry: SearchBattlefieldPostEntry::Transfigure,
                ..
            }
        )
    }

    /// Whether this search grants haste and schedules an end-step return.
    #[must_use]
    pub fn is_haste_and_return(&self) -> bool {
        matches!(
            self.destination,
            SearchDestination::Battlefield {
                post_entry: SearchBattlefieldPostEntry::HasteAndReturnAtEndStep,
                ..
            }
        )
    }

    /// Resolved Curse host policy if this search attaches a searched Curse.
    #[must_use]
    pub fn curse_host(&self) -> Option<AuraSearchHost> {
        if let SearchCardMatcher::CurseForHost(host) = &self.matcher {
            return Some(*host);
        }
        if let SearchDestination::Battlefield {
            post_entry: SearchBattlefieldPostEntry::AttachToCurseHost(host),
            ..
        } = self.destination
        {
            return Some(host);
        }
        None
    }

    /// Counter kind read from the source at resolution, if any.
    #[must_use]
    pub fn source_counter_kind(&self) -> Option<crate::effects::CounterKind> {
        match self.mana_value {
            Some(SearchManaValue::EqualToSourceCounters(kind)) => Some(kind),
            _ => None,
        }
    }

    /// Whether this search may be used as a typecycling body (CR 702.29e).
    #[must_use]
    pub fn is_typecycling_compatible(&self) -> bool {
        self.searcher == ResolvingPlayer::Controller
            && self.source == SearchSource::OwnLibrary
            && self.mana_value.is_none()
            && self.count == SearchCount::UpTo(1)
            && self.pair_constraint.is_none()
            && self.destination == SearchDestination::Hand
            && self.reveal
            && !matches!(
                self.matcher,
                SearchCardMatcher::Any
                    | SearchCardMatcher::Nonland
                    | SearchCardMatcher::CurseForHost(_)
            )
    }

    /// Whether finding a card is mandatory when the library is nonempty.
    #[must_use]
    pub fn is_required(&self) -> bool {
        matches!(self.count, SearchCount::Exact(_))
    }

    /// Whether this search uses the multi-card battlefield batch reply.
    #[must_use]
    pub fn is_battlefield_batch(&self) -> bool {
        matches!(self.destination, SearchDestination::Battlefield { .. })
            && matches!(
                self.count,
                SearchCount::UpTo(n) if n > 1
            )
            || matches!(self.destination, SearchDestination::Battlefield { .. })
                && matches!(self.count, SearchCount::UpToExpr(_))
    }

    /// Whether this search uses the multi-card exile batch reply.
    #[must_use]
    pub fn is_exile_batch(&self) -> bool {
        matches!(self.destination, SearchDestination::Exile { .. })
            && (matches!(self.count, SearchCount::UpTo(n) if n > 1)
                || matches!(self.count, SearchCount::UpToExpr(_)))
    }

    /// Fixed maximum selection count for bounded hand, graveyard, or exile batch searches.
    #[must_use]
    pub fn fixed_batch_limit(&self) -> Option<usize> {
        match self.destination {
            SearchDestination::Hand
            | SearchDestination::Graveyard
            | SearchDestination::Exile { .. } => match self.count {
                SearchCount::UpTo(n) if n > 1 => Some(usize::from(n)),
                SearchCount::Exact(n) if n.get() > 1 => Some(usize::from(n.get())),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether this search may be wrapped directly in an `OptionalSequence`.
    #[must_use]
    pub fn supports_optional_wrapper(&self) -> bool {
        !matches!(
            self.source,
            SearchSource::PlayerLibrary(_)
                | SearchSource::TargetOpponentLibrary
                | SearchSource::TargetPlayerLibrary
        ) && !self.is_required()
            && !self.is_transfigure()
            && !self.is_haste_and_return()
            && matches!(
                self.destination,
                SearchDestination::Hand
                    | SearchDestination::Graveyard
                    | SearchDestination::LibraryTop
                    | SearchDestination::Battlefield { .. }
                    | SearchDestination::Exile { .. }
            )
    }
}

/// Stack-functioning trigger that copies its own instruction spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCopyKeyword {
    /// A permanent instructs the caster to copy the observed spell once.
    CopyObservedSpell,
    /// Each player other than the caster may copy the observed spell.
    OfferCopyToOtherPlayers,
    CopyThisPermanentSpell,
    Storm,
    Gravestorm,
    Replicate(usize),
}

/// Recipients of one simultaneous nontargeted damage event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealedHandSelection {
    AllLands,
    OneCreature,
}

/// One simultaneous damage event with a group of recipients.
// `DamageGroup` is `Copy`; embedding `ObjectFilter` by value preserves `Copy` semantics.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageGroup {
    Controller,
    ControllerAndTargetCreature,
    DestroyedTargetController,
    RelatedPlayer,
    RelatedPlayerOrTheirPlaneswalker,
    VolunteeringOpponent,
    CapturedAttackedPlayerOrPlaneswalker,
    /// The resolving activated or triggered ability's own source permanent.
    AbilitySource,
    /// Creatures whose current types do not include the vocabulary-bound subtype.
    CreaturesExcept(CreatureTypeId),
    CreaturesOfSubtype(CreatureTypeId),
    CreaturesWithoutFlying,
    /// Battlefield permanents matching a general object filter (creatures, planeswalkers, battles).
    Filtered(ObjectFilter),
    /// Battlefield permanents matching `filter` plus every player in `players`.
    FilteredAndPlayers {
        filter: ObjectFilter,
        players: AffectedPlayers,
    },
    /// Every player resolved by `AffectedPlayers` (e.g., active player, defending player, each other player).
    AffectedPlayers(AffectedPlayers),
    Opponents,
    Creatures,
    Players,
    CreaturesAndPlayers,
    CreaturesAndPlaneswalkers,
}

/// One simultaneous action applied to the still-legal targets of a single printed clause.
/// Battlefield subjects identified from one announced target as the effect resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedExileGroup {
    /// The target creature and every other creature currently sharing its name.
    CreaturesSharingTargetName,
    /// All nonlands the target player controls, with a delayed return to their owners.
    TargetPlayerNonlandsUntilNextEndStep,
}

/// Printed linkage shared by a set of exile effects and their independent return trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkedExileGroup {
    FiendHunter,
    Helvault,
    /// Generalized source-linked exile returned when the source leaves the battlefield.
    SourceLinked,
}

/// Position in the physical card owner's library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashbackSubject {
    Target,
    ControllerGraveyard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryPlacement {
    Top,
    Bottom,
    /// Put on the bottom of the owner's library in a random order.
    BottomRandom,
    /// Zero-based index from the top of the owner's library (1 = second from the top).
    FromTop(u8),
    /// The designated player chooses top or bottom of the owner's library during resolution.
    ChoiceTopOrBottom {
        chooser: ResolvingPlayer,
    },
}

/// Subject resolved dynamically by `Instruction::MoveSubject`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveSubjectRef {
    TargetObject,
    AbilitySource,
    TriggeringObject,
    AttachedHost,
    RevealedCard,
    CreatedTokens,
    LastMoved,
    RememberedFirst,
    RememberedSecond,
}

/// Optional delayed-return schedule when `MoveSubject` exiles an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedReturnTiming {
    /// Flicker immediately back to the battlefield during this instruction.
    Immediately,
    /// Return to the battlefield at the beginning of the next end step.
    NextEndStep,
    /// Return to the battlefield when the ability source leaves the battlefield.
    UntilSourceLeaves,
    /// Record exiled card(s) under an explicit `LinkedExileGroup` for explicit LTB return triggers (e.g. Champion).
    LinkedGroup(LinkedExileGroup),
}

/// Orthogonal destination for unified zone-movement instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectDestination {
    Battlefield {
        tapped: bool,
        attacking: bool,
        transformed: bool,
        under_owner_control: bool,
        with_counter: Option<(crate::effects::CounterKind, u8)>,
        attach_to_self: bool,
    },
    Hand,
    Graveyard,
    Exile {
        delayed_return: Option<DelayedReturnTiming>,
    },
    Library(LibraryPlacement),
}

/// Source zone scanned by `Instruction::SelectZoneCardsToZone`, `ZoneCastPermission`, and `ExileGroupFromZone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceZoneFilter {
    Hand,
    Graveyard,
    /// Either the player's hand or their graveyard (e.g. Swift Warkite).
    HandOrGraveyard,
    Battlefield,
    /// Any battlefield permanent regardless of controller (filter may narrow controller).
    AnyBattlefield,
    Exile,
    OpponentGraveyard,
    AnyGraveyard,
    YourLibrary,
    OpponentExile,
    LinkedExile,
    /// Objects moved by the immediately preceding instruction in this resolution (for example milled cards).
    LastMoved,
    /// The hand of the resolving spell or ability's target player/opponent.
    TargetPlayerHand,
    /// The graveyard of the resolving spell or ability's target player/opponent.
    TargetPlayerGraveyard,
    /// Any opponent's hand.
    OpponentHand,
    /// The command zone (e.g. commander cards).
    CommandZone,
}

/// How multiple die rolls are combined into a single numeric result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DieRollMode {
    /// Sum all rolled dice.
    Sum,
    /// Keep the highest rolled die.
    KeepHighest,
    /// Prompt the roller to choose one of the rolled results via `OptionIndexFrame`.
    ChooseOne,
}

/// One numeric result band attached to a die roll (CR 706.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DieRollBand {
    pub min: u16,
    pub max: u16,
    pub body: ExecutableBody,
}

/// How coin flips are performed in `Instruction::FlipCoins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoinFlipMode {
    /// Flip a single coin.
    Single,
    /// Flip a fixed or dynamic number of coins.
    Count(ValueExpr),
    /// Flip coins until losing a flip.
    UntilLose,
}

/// Damage source resolved dynamically by `Instruction::DealDamage`.
// Inline `ObjectFilter` matches `Instruction` value semantics; boxed at the `Instruction` boundary.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageSourceRef {
    AbilitySource,
    TargetObject,
    TriggeringObject,
    AttachedHost,
    Matching(ObjectFilter),
    /// Objects moved earlier during this same resolution.
    LastMoved,
    /// Tokens created earlier during this same resolution.
    CreatedTokens,
    /// Every still-legal permanent target announced for this spell or ability.
    AllPermanentTargets,
}

/// Damage recipients resolved dynamically by `Instruction::DealDamage`.
// Inline `DamageGroup` matches `Instruction` value semantics; boxed at the `Instruction` boundary.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageRecipientsRef {
    Target,
    AbilitySource,
    TriggeringObject,
    AttachedHost,
    TargetController,
    Group(DamageGroup),
    /// Objects moved earlier during this same resolution.
    LastMoved,
    /// Tokens created earlier during this same resolution.
    CreatedTokens,
    /// Every other creature (excluding the damage source) and each player.
    OtherCreaturesAndEachPlayer,
    /// Every player other than the damage source's controller.
    EachOtherPlayer,
    /// Every opponent of the damage source's controller.
    EachOtherOpponent,
    /// Each damage source deals damage to itself.
    EachSourceToItself,
    /// Each damage source deals damage to its controller.
    EachSourceController,
}

/// Damage amount specification for `Instruction::DealDamage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageAmountSpec {
    Expr(ValueExpr),
    SourcePower,
    SourceToughness,
}

/// Combat obligation or removal action applied by `Instruction::ApplyContinuous` (`ContinuousPayload::CombatAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatActionKind {
    MustAttack,
    MustBlock,
    MustBeBlocked,
    Goad,
    RemoveFromCombat,
    UntapAndRemoveFromCombat,
    PreventCombatDamageByAndTo,
    /// Cause an attacking creature to become blocked without adding a blocker (CR 509.1h).
    BecomeBlocked,
}

/// Triggered cast keyword granted temporarily to the next matching spell cast by the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextSpellCastKeyword {
    /// CR 702.85 Cascade cast trigger.
    Cascade,
    /// CR 702.40 Storm cast trigger.
    Storm,
}

/// Event that triggers a [`TemporaryRuleKind::ExileReplacement`] rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporaryExileTrigger {
    Dies,
    LeavesBattlefield,
    SpellResolves,
}

/// Temporary game-rule modification installed by `Instruction::ApplyContinuous` (`ContinuousPayload::TemporaryRule`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemporaryRuleKind {
    /// Spells matching optional `filter` cannot be countered (or cannot be countered by spells
    /// or abilities of any color in `colors` when `colors` is non-empty).
    CantBeCountered {
        colors: ColorMask,
        filter: Option<ObjectFilter>,
        controller_only: bool,
        next_only: bool,
    },
    PlayerCantGainLife {
        players: AffectedPlayers,
    },
    /// Prevent `players` from casting spells (optionally restricted to `zone` and/or `filter`).
    CantCastSpells {
        players: AffectedPlayers,
        zone: Option<SourceZoneFilter>,
        filter: Option<ObjectFilter>,
    },
    /// Affected players can't activate abilities (or non-mana abilities when `nonmana_only` is true).
    CantActivateAbilities {
        players: AffectedPlayers,
        nonmana_only: bool,
    },
    /// Affected players can't play lands while this temporary rule lasts (e.g. Solfatara, Turf Wound).
    CantPlayLands {
        players: AffectedPlayers,
    },
    /// Grant player-applicable keywords (Hexproof, Shroud, Protection) to affected players (e.g. Veil of Summer, Teferi's Protection).
    PlayerGrantKeyword {
        players: AffectedPlayers,
        keywords: BTreeSet<KeywordAbility>,
    },
    /// Modifier applied to the next matching spell cast by `player` this turn
    /// (free cast, keyword grant, cast-trigger keyword grant, or affinity).
    NextSpellModifier {
        player: ResolvingPlayer,
        filter: Option<ObjectFilter>,
        free: bool,
        keyword: Option<KeywordAbility>,
        cast_keyword: Option<NextSpellCastKeyword>,
        affinity_filter: Option<ObjectFilter>,
    },
    /// Duration-scoped zone replacement effect (e.g. Yawgmoth's Will, Hallowed Moonlight, Cosmic Intervention).
    TemporaryZoneReplacement(Box<ZoneReplacementAbility>),
    DamageCantBePrevented,
    /// Temporary exile replacement when matching permanents die or leave the battlefield,
    /// or when matching spells cast by `players` would be put into a graveyard on resolution.
    ExileReplacement {
        subject: Option<Box<EffectSubject>>,
        players: AffectedPlayers,
        filter: Option<ObjectFilter>,
        trigger: TemporaryExileTrigger,
    },
    /// Temporary spell cost reduction (`increase == false`) or increase (`increase == true`)
    /// evaluated when `ApplyContinuous` resolves (CR 611.2c).
    SpellCostModifier {
        players: AffectedPlayers,
        filter: ObjectFilter,
        amount: Box<ValueExpr>,
        increase: bool,
        next_only: bool,
    },
    AdditionalLandPlay {
        player: ResolvingPlayer,
        count: u8,
    },
    /// Affected players can cast at most `max_more` additional spells this turn (e.g. Irencrag Feat).
    CastMaxMoreSpellsThisTurn {
        players: AffectedPlayers,
        max_more: u8,
    },
    /// Temporary flash permission for spells matching `filter` cast by `player`.
    FilteredFlash {
        player: ResolvingPlayer,
        filter: ObjectFilter,
        next_only: bool,
    },
    /// Temporary zone-wide play/cast permission for cards matching `filter` in `zone`.
    ZoneCastPermission {
        player: ResolvingPlayer,
        zone: SourceZoneFilter,
        filter: Option<ObjectFilter>,
        cast_only: bool,
        free: bool,
        any_color: bool,
        once: bool,
        sorcery_only: bool,
    },
    /// Duration-scoped non-layer static ability installed by a resolving spell or ability (CR 611.2a),
    /// optionally gated by `condition` or consumed on first application when `one_shot` is true.
    TemporaryStatic {
        ability: Box<crate::characteristics::StaticContinuousAbility>,
        condition: Option<Box<crate::characteristics::StaticSourceCondition>>,
        one_shot: bool,
    },
    /// Duration-scoped damage prevention shield (all combat damage, finite or dynamic next-N
    /// prevention, or one-shot next-time prevention per CR 615.4).
    PreventDamageShield {
        amount: Option<Box<ValueExpr>>,
        source: Option<ObjectFilter>,
        source_chosen_color: bool,
        source_chosen_type: bool,
        source_is_target: bool,
        half_round_down: bool,
        recipient: crate::characteristics::StaticDamageRecipient,
        combat_only: bool,
        one_shot: bool,
    },
    /// Duration-scoped damage redirection shield (CR 614.9, e.g. Shield Dancer, Blood of the Martyr, Harm's Way).
    RedirectNextDamage {
        source: Option<ObjectFilter>,
        source_is_target: bool,
        from: crate::characteristics::StaticDamageRecipient,
        to: crate::characteristics::DamageRedirectTarget,
        combat_only: bool,
        amount: Option<Box<ValueExpr>>,
        one_shot: bool,
    },
}

impl TemporaryRuleKind {
    /// Construct a shield preventing all combat damage this turn.
    #[must_use]
    pub fn prevent_all_combat_damage() -> Self {
        Self::PreventDamageShield {
            amount: None,
            source: None,
            source_chosen_color: false,
            source_chosen_type: false,
            source_is_target: false,
            half_round_down: false,
            recipient: crate::characteristics::StaticDamageRecipient::Any,
            combat_only: true,
            one_shot: false,
        }
    }

    /// Construct an unconditional multi-use temporary static ability.
    #[must_use]
    pub fn temporary_static(ability: crate::characteristics::StaticContinuousAbility) -> Self {
        Self::TemporaryStatic {
            ability: Box::new(ability),
            condition: None,
            one_shot: false,
        }
    }

    /// Construct a one-shot duration-scoped event replacement ability.
    #[must_use]
    pub fn one_shot_event_replacement(
        ability: crate::characteristics::StaticContinuousAbility,
    ) -> Self {
        Self::TemporaryStatic {
            ability: Box::new(ability),
            condition: None,
            one_shot: true,
        }
    }

    /// Construct a conditional temporary static ability active while `condition` holds.
    #[must_use]
    pub fn conditional_temporary_static(
        condition: crate::characteristics::StaticSourceCondition,
        ability: crate::characteristics::StaticContinuousAbility,
    ) -> Self {
        Self::TemporaryStatic {
            ability: Box::new(ability),
            condition: Some(Box::new(condition)),
            one_shot: false,
        }
    }

    /// Construct a temporary replacement exiling `subject` if it would die.
    #[must_use]
    pub fn exile_if_dies(subject: EffectSubject) -> Self {
        Self::ExileReplacement {
            subject: Some(Box::new(subject)),
            players: AffectedPlayers::EachPlayer,
            filter: None,
            trigger: TemporaryExileTrigger::Dies,
        }
    }

    /// Construct a temporary replacement exiling `subject` if it would leave the battlefield.
    #[must_use]
    pub fn exile_if_leaves_battlefield(subject: EffectSubject) -> Self {
        Self::ExileReplacement {
            subject: Some(Box::new(subject)),
            players: AffectedPlayers::EachPlayer,
            filter: None,
            trigger: TemporaryExileTrigger::LeavesBattlefield,
        }
    }

    /// Construct a temporary replacement exiling matching spells cast by `players` on resolution.
    #[must_use]
    pub fn exile_spell_on_resolve(players: AffectedPlayers, filter: Option<ObjectFilter>) -> Self {
        Self::ExileReplacement {
            subject: None,
            players,
            filter,
            trigger: TemporaryExileTrigger::SpellResolves,
        }
    }

    /// Construct a rule letting the controller cast their next matching spell without paying its mana cost.
    #[must_use]
    pub fn next_spell_free(filter: Option<ObjectFilter>) -> Self {
        Self::NextSpellModifier {
            player: ResolvingPlayer::Controller,
            filter,
            free: true,
            keyword: None,
            cast_keyword: None,
            affinity_filter: None,
        }
    }

    /// Construct a rule granting `keyword` to the controller's next matching spell.
    #[must_use]
    pub fn next_spell_keyword(filter: ObjectFilter, keyword: KeywordAbility) -> Self {
        Self::NextSpellModifier {
            player: ResolvingPlayer::Controller,
            filter: Some(filter),
            free: false,
            keyword: Some(keyword),
            cast_keyword: None,
            affinity_filter: None,
        }
    }

    /// Construct a rule granting `keyword` (cascade or storm) to the controller's next matching spell.
    #[must_use]
    pub fn next_spell_cast_keyword(filter: ObjectFilter, keyword: NextSpellCastKeyword) -> Self {
        Self::NextSpellModifier {
            player: ResolvingPlayer::Controller,
            filter: Some(filter),
            free: false,
            keyword: None,
            cast_keyword: Some(keyword),
            affinity_filter: None,
        }
    }

    /// Construct a rule granting affinity for `affinity_filter` to `player`'s next matching spell.
    #[must_use]
    pub fn next_spell_affinity(
        player: ResolvingPlayer,
        spell_filter: ObjectFilter,
        affinity_filter: ObjectFilter,
    ) -> Self {
        Self::NextSpellModifier {
            player,
            filter: Some(spell_filter),
            free: false,
            keyword: None,
            cast_keyword: None,
            affinity_filter: Some(affinity_filter),
        }
    }

    /// Whether this temporary rule references an announced target of the resolving entry.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        let is_target_player = |p: ResolvingPlayer| {
            matches!(
                p,
                ResolvingPlayer::TargetPlayer
                    | ResolvingPlayer::TargetController
                    | ResolvingPlayer::TargetOwner
                    | ResolvingPlayer::TargetSpellController
                    | ResolvingPlayer::ExiledTargetController
            )
        };
        let players_use_target = |players: AffectedPlayers| matches!(players, AffectedPlayers::One(p) if is_target_player(p));
        let filter_uses_target =
            |f: &ObjectFilter| f.has_target_player() || f.uses_target_permanent();
        let recipient_uses_target = |r: &crate::characteristics::StaticDamageRecipient| {
            matches!(
                r,
                crate::characteristics::StaticDamageRecipient::TargetObject
                    | crate::characteristics::StaticDamageRecipient::TargetAny
            )
        };
        match self {
            Self::PlayerCantGainLife { players }
            | Self::CantPlayLands { players }
            | Self::CantActivateAbilities { players, .. }
            | Self::PlayerGrantKeyword { players, .. }
            | Self::CastMaxMoreSpellsThisTurn { players, .. } => players_use_target(*players),
            Self::CantCastSpells {
                players, filter, ..
            } => players_use_target(*players) || filter.as_ref().is_some_and(filter_uses_target),
            Self::CantBeCountered { filter, .. } => {
                filter.as_ref().is_some_and(filter_uses_target)
            }
            Self::NextSpellModifier {
                player,
                filter,
                affinity_filter,
                ..
            } => {
                is_target_player(*player)
                    || filter.as_ref().is_some_and(filter_uses_target)
                    || affinity_filter.as_ref().is_some_and(filter_uses_target)
            }
            Self::SpellCostModifier {
                players,
                filter,
                amount,
                ..
            } => players_use_target(*players) || filter_uses_target(filter) || amount.uses_target(),
            Self::FilteredFlash { player, filter, .. } => {
                is_target_player(*player) || filter_uses_target(filter)
            }
            Self::ZoneCastPermission { player, filter, .. } => {
                is_target_player(*player) || filter.as_ref().is_some_and(filter_uses_target)
            }
            Self::AdditionalLandPlay { player, .. } => is_target_player(*player),
            Self::ExileReplacement {
                subject,
                players,
                filter,
                ..
            } => {
                players_use_target(*players)
                    || filter.as_ref().is_some_and(filter_uses_target)
                    || subject.as_ref().is_some_and(|subject| {
                        matches!(
                            subject.as_ref(),
                            EffectSubject::TargetObject
                                | EffectSubject::TargetAndSameNameMatching(_)
                                | EffectSubject::MatchingOtherThanTarget(_)
                                | EffectSubject::TargetAndSharingColorMatching(_)
                        ) || matches!(subject.as_ref(), EffectSubject::Matching(f) if filter_uses_target(f))
                    })
            }
            Self::PreventDamageShield {
                amount,
                source,
                source_is_target,
                recipient,
                ..
            } => {
                amount.as_deref().is_some_and(ValueExpr::uses_target)
                    || *source_is_target
                    || source.as_ref().is_some_and(filter_uses_target)
                    || recipient_uses_target(recipient)
            }
            Self::RedirectNextDamage {
                amount,
                source,
                source_is_target,
                from,
                to,
                ..
            } => {
                amount.as_deref().is_some_and(ValueExpr::uses_target)
                    || *source_is_target
                    || source.as_ref().is_some_and(filter_uses_target)
                    || recipient_uses_target(from)
                    || matches!(
                        to,
                        crate::characteristics::DamageRedirectTarget::TargetObject
                            | crate::characteristics::DamageRedirectTarget::TargetAny
                            | crate::characteristics::DamageRedirectTarget::SecondTargetObject
                    )
            }
            Self::TemporaryStatic { .. }
            | Self::TemporaryZoneReplacement(_)
            | Self::DamageCantBePrevented => false,
        }
    }
}

/// Which stack object is copied by `Instruction::StackOp` (CR 707.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackCopyTarget {
    TargetSpell,
    TargetAbility,
    TargetSpellOrAbility,
    TriggeringSpell,
    TriggeringAbility,
    ResolvingSpell,
}

/// Uniqueness restriction across repeated resolutions of a modal ability or spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalUniqueness {
    #[default]
    None,
    OncePerTurn,
    OncePerGame,
}

/// Casting timing and world-condition restrictions printed on a spell or castable card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastTimingRestriction {
    Sorcery,
    DuringCombat,
    DuringCombatBeforeBlockers,
    BeforeCombatDamage,
    DuringCombatAfterBlockers,
    DuringDeclareAttackers,
    DuringDeclareBlockers,
    DuringDeclareAttackersIfAttacked,
    DuringCombatOnOpponentsTurn,
    DuringYourTurn,
    DuringOpponentsTurn,
    DuringStep(crate::turn::Step),
    DuringOwnStep(crate::turn::Step),
    DuringOpponentsStep(crate::turn::Step),
    /// Cast only if announced X is strictly greater than zero.
    XNonZero,
    /// Cast only after combat on your turn (postcombat main phase / end step).
    AfterCombat,
    Condition(Box<ResolutionCondition>),
    All(Vec<CastTimingRestriction>),
}

impl CastTimingRestriction {
    pub(crate) fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Condition(cond) => cond.validate_world(vocabulary),
            Self::All(items) => {
                if items.is_empty() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                for item in items {
                    item.validate(vocabulary)?;
                }
                Ok(())
            }
            Self::Sorcery
            | Self::DuringCombat
            | Self::DuringCombatBeforeBlockers
            | Self::BeforeCombatDamage
            | Self::DuringCombatAfterBlockers
            | Self::DuringDeclareAttackers
            | Self::DuringDeclareBlockers
            | Self::DuringDeclareAttackersIfAttacked
            | Self::DuringCombatOnOpponentsTurn
            | Self::DuringYourTurn
            | Self::DuringOpponentsTurn
            | Self::DuringStep(_)
            | Self::DuringOwnStep(_)
            | Self::DuringOpponentsStep(_)
            | Self::XNonZero
            | Self::AfterCombat => Ok(()),
        }
    }
}

/// Printed restrictions on how many creatures may be declared in combat.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CombatDeclarationLimits {
    pub one_attacker: bool,
    pub one_blocker: bool,
}

/// Combat declaration rules contributed by this permanent's active printed ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackRequirement {
    /// Pay this amount for each creature attacking a protected defender.
    PayPerAttacker {
        protect_player: bool,
        symbol: AttackCostSymbol,
    },
    LimitAttacksAtTappedController,
    /// Flying creatures cannot attack this permanent's controller or their planeswalkers.
    FlyingCannotAttackController,
    /// The enchanted creature cannot attack its Aura's controller or block their creatures.
    EnchantedCannotAttackOrBlockController,
    EquippedCannotAttackControllerOrBeSacrificed,
    SourceAttacks,
    SourceAttacksExceptOwner,
    AllCreaturesAttack,
    EnchantedCreatureIsGoaded,
    AttachedCreatureAttacks,
    /// Creatures controlled by the attached player must attack if able.
    EnchantedPlayerCreaturesAttack,
    /// Source creature cannot attack its controller's opponents' controller (wait: cannot attack you or planeswalkers you control).
    SourceCannotAttackController,
    /// Attached creature cannot attack this permanent's controller or planeswalkers they control.
    AttachedCannotAttackController,
}
/// A resolving attack trigger alters declaration choices for this combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeclarationRule {
    EachOpponentOneBlocker,
    ControllerChooses,
}
/// Recipients of a resolving instruction that grants protection from one chosen color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionRecipients {
    TargetPermanent,
    ControllerAndPermanents,
    AbilitySource,
    Matching(Box<ObjectFilter>),
}

/// A unit of cost applied to each creature attacking the affected defender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackCostSymbol {
    OneGeneric,
    Generic(u8),
    WhitePhyrexian,
}
/// Which permanent deals damage using its own power at resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerDamageSource {
    AbilitySource,
    CreatureDealtDamage,
    /// Exact creature observed by a trigger as it entered from a graveyard.
    GraveyardEntrant,
    TargetCreature,
    /// The targeted creature deals its power to the exact ability source.
    TargetCreatureToAbilitySource,
}
/// Creatures untapped when a resolving effect adds a combat phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatUntapGroup {
    None,
    ControlledCreatures,
    SourceAndControlledSubtype(CreatureTypeId),
    AttackersIfSourceAttacksHighestLife,
}
/// Zone searched by [`EntryCopyRule::Matching`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCopyZone {
    Battlefield,
    OwnGraveyard,
    AnyGraveyard,
    OpponentGraveyard,
}

/// Generalized clone replacement rule (CR 707.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryCopySpec {
    pub zone: EntryCopyZone,
    pub filter: ObjectFilter,
    pub nonlegendary: bool,
    pub extra_types: BTreeSet<CardType>,
    pub extra_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub extra_keywords: BTreeSet<KeywordAbility>,
    pub extra_counters: Option<(crate::effects::CounterKind, u8)>,
    pub extra_activated: Option<Box<crate::activated::ActivatedAbility>>,
    pub extra_triggered: Option<Box<TriggeredAbility>>,
}

/// Extended clone replacement rule supporting multiple abilities, conditional counters,
/// P/T overrides, extra colors, and name retention (CR 707.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryCopySpecExt {
    pub zone: EntryCopyZone,
    pub filter: ObjectFilter,
    pub nonlegendary: bool,
    pub extra_types: BTreeSet<CardType>,
    pub extra_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub extra_keywords: BTreeSet<KeywordAbility>,
    pub extra_counters: Vec<(crate::effects::CounterKind, u8)>,
    pub conditional_counters: Vec<(ResolutionCondition, crate::effects::CounterKind, u8)>,
    pub extra_activated: Vec<crate::activated::ActivatedAbility>,
    pub extra_triggered: Vec<TriggeredAbility>,
    pub extra_static: Vec<StaticContinuousAbility>,
    pub override_pt: Option<(RulesInteger, RulesInteger)>,
    pub extra_colors: BTreeSet<Color>,
    pub override_colors: Option<BTreeSet<Color>>,
    pub extra_artifact_subtypes: BTreeSet<ArtifactSubtype>,
    pub extra_enchantment_subtypes: BTreeSet<EnchantmentSubtype>,
    pub extra_supertypes: BTreeSet<Supertype>,
    pub keep_name: bool,
    /// Whether copying is optional ("You may have ... enter as a copy"); defaults to `true`.
    pub optional: bool,
    /// Whether the permanent enters tapped when copying.
    pub enters_tapped: bool,
    /// Additional printed mana abilities granted as part of the copy exception.
    pub extra_mana: Vec<ManaAbility>,
}

impl EntryCopySpecExt {
    /// Construct an extended clone spec with standard defaults (`optional: true`, `enters_tapped: false`, empty `extra_mana`).
    #[must_use]
    pub fn new(zone: EntryCopyZone, filter: ObjectFilter) -> Self {
        Self {
            zone,
            filter,
            nonlegendary: false,
            extra_types: BTreeSet::new(),
            extra_creature_subtypes: BTreeSet::new(),
            extra_keywords: BTreeSet::new(),
            extra_counters: Vec::new(),
            conditional_counters: Vec::new(),
            extra_activated: Vec::new(),
            extra_triggered: Vec::new(),
            extra_static: Vec::new(),
            override_pt: None,
            extra_colors: BTreeSet::new(),
            override_colors: None,
            extra_artifact_subtypes: BTreeSet::new(),
            extra_enchantment_subtypes: BTreeSet::new(),
            extra_supertypes: BTreeSet::new(),
            keep_name: false,
            optional: true,
            enters_tapped: false,
            extra_mana: Vec::new(),
        }
    }

    /// Validate vocabulary references, counters, conditions, and granted abilities on this clone spec.
    pub(crate) fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.filter.validate_vocabulary(vocabulary)?;
        if self
            .extra_creature_subtypes
            .iter()
            .any(|ct| vocabulary.label(*ct).is_none())
        {
            return Err(DefinitionError::ForeignCreatureType);
        }
        for (kind, _) in &self.extra_counters {
            if !kind.is_permanent() {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        for (cond, kind, _) in &self.conditional_counters {
            if !kind.is_permanent() {
                return Err(DefinitionError::InvalidInstruction);
            }
            cond.validate_world(vocabulary)?;
        }
        for ability in &self.extra_activated {
            if !ability.mana_cost.valid() {
                return Err(DefinitionError::InvalidInstruction);
            }
            ability
                .body
                .validate_in(vocabulary, BodyContext::ACTIVATED)?;
        }
        for trigger in &self.extra_triggered {
            trigger.validate_body(vocabulary)?;
        }
        for stat in &self.extra_static {
            stat.validate(vocabulary)
                .map_err(|_| DefinitionError::ContinuousEffect)?;
        }
        for mana in &self.extra_mana {
            if !mana.output.valid(vocabulary) {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        Ok(())
    }
}

/// Tie-breaking policy for [`crate::toolbox::PlayerMutation::SelectExtremeLife`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtremeLifeTieMode {
    /// Resolving controller chooses one of the tied players.
    ControllerChooses,
    /// If tied, the game ends in a draw.
    DrawGame,
    /// If tied, no player is selected.
    Skip,
}

/// Endpoint specification for permanent-attachment selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachSlot {
    /// Fixed object reference resolved without player selection.
    Fixed(MoveSubjectRef),
    /// Player chooses matching battlefield permanent(s) during resolution.
    ChooseMatching {
        filter: Box<ObjectFilter>,
        exclude_current_host: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryCopyRule {
    GraveyardCreature,
    ControlledCreatureOrPlaneswalker,
    BattlefieldCreature,
    ControlledCreature,
    BattlefieldArtifact,
    BattlefieldEnchantment,
    BattlefieldCreatureWithAbility(Box<crate::activated::ActivatedAbility>),
    Matching(Box<EntryCopySpec>),
    /// Extended CR 707.9 clone rule with full multi-ability, counter, color, P/T, and name exceptions.
    MatchingExt(Box<EntryCopySpecExt>),
}

/// Destination for kept/matched cards in [`crate::toolbox::zone_op::look_top_choose_instructions`]
/// and [`crate::toolbox::zone_op::reveal_until_match_instructions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookTopKeepDest {
    Hand,
    Battlefield {
        tapped: bool,
        attacking: bool,
    },
    LibraryTop,
    Graveyard,
    Exile {
        playable: bool,
        free: bool,
        cast_only: bool,
        any_color: bool,
        duration: ResolvedDuration,
    },
    /// Exile kept cards linked to the ability source (optionally face down and/or playable).
    ExileLinked {
        face_down: bool,
        playable: bool,
        free: bool,
        cast_only: bool,
        any_color: bool,
        duration: ResolvedDuration,
    },
}

/// Destination for remaining/unkept cards in [`crate::toolbox::zone_op::look_top_choose_instructions`]
/// and [`crate::toolbox::zone_op::reveal_until_match_instructions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookTopRestDest {
    BottomRandom,
    BottomOrder,
    TopOrder,
    Graveyard,
    Exile,
    Hand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelayedGraveyardReturn {
    DyingCreatureWhileSourceRemains,
    DyingSourceToHand,
    DyingSourceTransformed,
    AuraAfterEnchantedCreatureDies,
}

/// Permanents that a resolving effect phases out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhasingSubject {
    Target,
    Source,
    ControlledLands,
}

/// Intervening-if or resolution-time condition guard checked by [`Instruction::RequireCondition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterveningCondition {
    /// Controller (or an opponent when `opponent: true`) is the monarch.
    Monarch { opponent: bool },
    /// An opponent controls more lands than the controller.
    OpponentMoreLands,
    /// Opponents have lost at least this much life this turn.
    OpponentLifeLoss(u8),
    /// Trigger source has at least this many quest counters (using LKI if departed).
    SourceQuestCounters(u8),
    /// Trigger source has at least one loyalty counter available to transfer.
    SourceLoyalty,
    /// Controller controls the artifact with the greatest mana value.
    GreatestArtifact,
    /// Controller controls exactly this many creatures.
    ControlledCreatureCount(u16),
    /// Controller controls no other creatures besides the trigger source.
    NoOtherControlledCreatures,
    /// Controller has not activated a loyalty ability this turn.
    NoLoyaltyActivation,
    /// Controller controls their own commander.
    OwnCommander,
    /// The resolving spell was cast from a graveyard (skips remainder if false).
    CastFromGraveyard,
    /// The delayed trigger's originating source is still on the battlefield (skips remainder if false).
    ReturnSourceOnBattlefield,
}

impl InterveningCondition {
    /// Corresponding trigger intervening-if condition, if this guard mirrors one.
    #[must_use]
    pub const fn expected_trigger_condition(self) -> Option<TriggerCondition> {
        match self {
            Self::Monarch { opponent } => Some(TriggerCondition::Monarch { opponent }),
            Self::OpponentMoreLands => Some(TriggerCondition::OpponentHasMoreLands),
            Self::OpponentLifeLoss(n) => Some(TriggerCondition::OpponentLostLifeAtLeast(n)),
            Self::SourceQuestCounters(minimum) => Some(TriggerCondition::SourceCounterAtLeast {
                kind: crate::effects::CounterKind::Quest,
                minimum,
            }),
            Self::SourceLoyalty => Some(TriggerCondition::SourceHasLoyalty),
            Self::GreatestArtifact => Some(TriggerCondition::ControlsGreatestArtifact),
            Self::ControlledCreatureCount(count) => {
                Some(TriggerCondition::ControlledCreatureCount(count))
            }
            Self::NoOtherControlledCreatures => Some(TriggerCondition::NoOtherControlledCreatures),
            Self::NoLoyaltyActivation => Some(TriggerCondition::NoLoyaltyActivationThisTurn),
            Self::OwnCommander => Some(TriggerCondition::ControlsOwnCommander),
            Self::CastFromGraveyard | Self::ReturnSourceOnBattlefield => None,
        }
    }

    /// Corresponding guard instruction for an intervening-if trigger condition, if required.
    #[must_use]
    pub fn from_trigger_condition(cond: &TriggerCondition) -> Option<Self> {
        match *cond {
            TriggerCondition::Monarch { opponent } => Some(Self::Monarch { opponent }),
            TriggerCondition::OpponentHasMoreLands => Some(Self::OpponentMoreLands),
            TriggerCondition::OpponentLostLifeAtLeast(n) => Some(Self::OpponentLifeLoss(n)),
            TriggerCondition::SourceCounterAtLeast {
                kind: crate::effects::CounterKind::Quest,
                minimum,
            } => Some(Self::SourceQuestCounters(minimum)),
            TriggerCondition::SourceHasLoyalty => Some(Self::SourceLoyalty),
            TriggerCondition::ControlsGreatestArtifact => Some(Self::GreatestArtifact),
            TriggerCondition::ControlledCreatureCount(count) => {
                Some(Self::ControlledCreatureCount(count))
            }
            TriggerCondition::NoOtherControlledCreatures => Some(Self::NoOtherControlledCreatures),
            TriggerCondition::NoLoyaltyActivationThisTurn => Some(Self::NoLoyaltyActivation),
            TriggerCondition::ControlsOwnCommander => Some(Self::OwnCommander),
            _ => None,
        }
    }
}

/// Mana produced during spell or ability resolution by [`crate::toolbox::PlayerMutation::AddMana`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectManaYield {
    /// Add a fixed mana bundle.
    Fixed(BTreeMap<crate::mana::ManaType, RulesInteger>),
    /// Add mana equal to the triggering combat damage; only those units survive step changes this turn.
    CombatDamageUntilCleanup(crate::mana::ManaType),
    /// Add `amount` mana of `kind` that persists across combat steps until combat ends (CR 702.189 Firebending).
    RetainedUntilEndOfCombat {
        kind: crate::mana::ManaType,
        amount: ValueExpr,
    },
    /// Add one mana of `kind` per resolved `amount`.
    Counted {
        kind: crate::mana::ManaType,
        amount: ValueExpr,
    },
    /// Add `amount` total mana, distributing it between two distinct colors.
    Split {
        first: crate::mana::ManaType,
        second: crate::mana::ManaType,
        amount: ValueExpr,
    },
    /// Choose a color from `colors` and add `amount` mana restricted to `restriction`.
    RestrictedChoice {
        colors: BTreeSet<crate::mana::ManaType>,
        amount: ValueExpr,
        restriction: crate::mana::ManaRestriction,
    },
    /// Add a fixed mana bundle restricted to `restriction`.
    RestrictedFixed {
        bundle: BTreeMap<crate::mana::ManaType, RulesInteger>,
        restriction: crate::mana::ManaRestriction,
    },
    /// Add `amount` mana of `kind` restricted to `restriction`.
    RestrictedCounted {
        kind: crate::mana::ManaType,
        amount: ValueExpr,
        restriction: crate::mana::ManaRestriction,
    },
    /// Choose a color from `colors` and add `amount` unrestricted mana of that color.
    Choice {
        colors: BTreeSet<crate::mana::ManaType>,
        amount: ValueExpr,
    },
    /// Choose one of `options` fixed mana bundles to add.
    ChoiceBundle {
        options: Vec<BTreeMap<crate::mana::ManaType, RulesInteger>>,
    },
    /// Add `amount` mana of any color that a land controlled by `controller` could produce.
    LandProducible {
        controller: AffectedPlayers,
        amount: ValueExpr,
    },
    /// Add `amount` mana of any color in the resolving controller's commanders' color identity.
    CommanderIdentity { amount: ValueExpr },
    /// Add `amount` mana of the color chosen for the ability source permanent.
    SourceChosenColor { amount: ValueExpr },
    /// Add `amount` mana of distinct colors (up to 5).
    DistinctColors { amount: ValueExpr },
    /// Wrap any mana yield so the added mana is not lost as steps and phases end until cleanup.
    RetainedUntilCleanup(Box<EffectManaYield>),
}

impl EffectManaYield {
    pub(crate) fn uses_target(&self) -> bool {
        match self {
            Self::RetainedUntilCleanup(inner) => inner.uses_target(),
            Self::Counted { amount, .. }
            | Self::RestrictedCounted { amount, .. }
            | Self::RestrictedChoice { amount, .. }
            | Self::RetainedUntilEndOfCombat { amount, .. }
            | Self::Split { amount, .. }
            | Self::Choice { amount, .. }
            | Self::CommanderIdentity { amount }
            | Self::DistinctColors { amount }
            | Self::SourceChosenColor { amount } => amount.uses_target(),
            _ => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        let inner = match self {
            Self::RetainedUntilCleanup(inner) => inner.as_ref(),
            other => other,
        };
        match inner {
            Self::Counted { amount, .. }
            | Self::RestrictedCounted { amount, .. }
            | Self::RestrictedChoice { amount, .. }
            | Self::RetainedUntilEndOfCombat { amount, .. }
            | Self::Split { amount, .. }
            | Self::Choice { amount, .. }
            | Self::LandProducible { amount, .. }
            | Self::CommanderIdentity { amount }
            | Self::SourceChosenColor { amount }
            | Self::DistinctColors { amount } => amount
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            _ => Ok(()),
        }
    }
}

/// Duration window for exiled impulse-play permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpulsePlayDuration {
    ThisTurn,
    UntilEndOfNextTurn,
    /// Until the beginning of the grantee's next end step (CR 611.2a).
    UntilNextEndStep,
    /// Until the end of the grantee's next end step.
    UntilEndOfNextEndStep,
    /// Until the beginning of the grantee's next upkeep step (CR 611.2a).
    UntilNextUpkeep,
    WhileExiled,
    /// Until another card is exiled with the same source permanent.
    UntilNextFromSource,
}

/// Exceptions applied when an object becomes a copy of another object (CR 707.9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyExceptions {
    pub nonlegendary: bool,
    pub keep_name: bool,
    pub keep_abilities: bool,
    pub keep_types: bool,
    pub override_pt: Option<crate::characteristics::PowerToughness>,
    pub extra_types: BTreeSet<CardType>,
    pub extra_artifact_subtypes: BTreeSet<ArtifactSubtype>,
    pub extra_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub extra_keywords: BTreeSet<KeywordAbility>,
    pub extra_triggered: Vec<TriggeredAbility>,
    pub extra_activated: Vec<ActivatedAbility>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
// Remaining inline control/search variants stay inline while consolidation migrates them onto boxed toolbox specs.
#[allow(clippy::large_enum_variant)]
pub enum Instruction {
    /// Schedule a delayed event-driven triggered ability (CR 603.7) that expires after `expires`.
    ScheduleDelayedEventTrigger {
        trigger: Box<TriggerSpec>,
        body: Box<ExecutableBody>,
        expires: ResolvedDuration,
        once: bool,
    },
    /// Execute `body` sequentially `count` times (evaluated once on loop entry).
    RepeatLoop {
        count: ValueExpr,
        body: Box<ExecutableBody>,
    },
    /// CR 603.7. Schedule a delayed step trigger executing `body` at the next matching step.
    ScheduleDelayedStepTrigger {
        step: Step,
        own_step_only: bool,
        body: Box<ExecutableBody>,
    },
    /// CR 603.12. Emit a reflexive triggered ability executing `body` (separately targeted on stack).
    EmitReflexiveTrigger { body: Box<ExecutableBody> },
    /// One mandatory target clause in a fixed sequence; the enclosed body uses that clause's target.
    FixedTargetClause(Box<ExecutableBody>),
    /// Search a player's library (and optionally graveyard) and shuffle.
    SearchLibrary { filter: Box<LibrarySearch> },
    /// Continue at a strictly forward absolute destination, including body end.
    Jump { destination: usize },
    /// Offer Take at the next instruction or Decline after `instructions` (`>= 1`) operations.
    /// Admission rejects a missing next instruction or an unsupported control wrapper.
    /// One choice governs the complete following sequence, including any later decisions.
    OptionalSequence {
        chooser: ResolvingPlayer,
        instructions: usize,
    },
    CreateTokens {
        creations: Vec<crate::token::TokenCreation>,
    },
    /// Universal composable zone-operation pipeline (`Source -> Select -> Move/Reveal/Look -> Bind/Tag`).
    ZoneOp(Box<crate::toolbox::ZoneOpSpec>),
    /// Universal card-draw primitive (`PlayerSelector`, `ValueExpr`, optional `ExecEnv` bind).
    DrawCards(Box<crate::toolbox::DrawCardsSpec>),
    /// Universal permanent/object state mutation primitive (`Query -> Select -> Mutate -> Bind`).
    MutateObject(Box<crate::toolbox::MutateObjectSpec>),
    /// Universal composable life adjustment primitive (gain, lose, set, pay, drain, lose half).
    AdjustLife(Box<crate::toolbox::AdjustLifeSpec>),
    /// Universal composable damage primitive (entry or object source(s), arbitrary targets, optional exile-if-dies).
    DealDamage(Box<crate::toolbox::DealDamageSpec>),
    /// Universal composable player state mutation primitive (counters, win/lose/draw, extra turn, end turn, monarch, ascend, speed, day/night).
    MutatePlayer(Box<crate::toolbox::MutatePlayerSpec>),
    /// Universal continuous effect, restriction, replacement, cost modifier, or emblem applicator.
    ApplyContinuous(Box<crate::toolbox::ApplyContinuousSpec>),
    /// Universal interactive player choice with automatic `ExecEnv` binding, history, and `ChoiceMade` event.
    PromptChoice(Box<crate::toolbox::PromptChoiceSpec>),
    /// Branch forward to `destination` if `condition` evaluates to `false`.
    BranchCondition {
        condition: Box<crate::toolbox::ConditionExpr>,
        destination: usize,
    },
    /// Execute `body` once per object returned by `query`, binding each item in `ExecEnv` at `bind_item`.
    ForEach {
        query: Box<crate::toolbox::ObjectQuery>,
        bind_item: crate::toolbox::BindingId,
        body: Box<ExecutableBody>,
    },
    /// Execute `body` once per player returned by `selector`, binding each seat in `ExecEnv` at `bind_player`.
    ForEachPlayerQuery {
        selector: Box<crate::toolbox::PlayerSelector>,
        bind_player: crate::toolbox::BindingId,
        body: Box<ExecutableBody>,
    },
    /// Bind a typed value directly into `ExecEnv` at `binding`.
    BindEnv {
        binding: crate::toolbox::BindingId,
        value: Box<crate::toolbox::ExecValue>,
    },
    /// Set (`Some`) or remove (`None`) a metadata tag or partition membership on queried objects.
    SetObjectTag {
        query: Box<crate::toolbox::ObjectQuery>,
        key: crate::toolbox::TagKey,
        value: Option<crate::toolbox::TagValue>,
    },
    /// Update a generic global, player, or object state track in `StateTrackStore`.
    UpdateStateTrack {
        key: Box<crate::toolbox::StateTrackKeySpec>,
        op: crate::toolbox::StateTrackOp,
    },
    /// Advance, cycle, or recycle a supplemental side-deck in `SideDeckStore`.
    SideDeckOp(Box<crate::toolbox::SideDeckOpSpec>),
    /// Universal stack manipulation primitive (counter, copy, retarget, gain control).
    StackOp(Box<crate::toolbox::StackOpSpec>),
}
impl Instruction {
    /// Relocate absolute instruction jump destinations `>= min_dest` by `delta`.
    /// Exhaustive over every `Instruction` variant so new jump-carrying operations
    /// cannot silently be missed when prepending or splicing instructions.
    pub fn shift_jumps(&mut self, delta: isize, min_dest: usize) {
        let adjust = |dest: &mut usize| {
            if *dest >= min_dest {
                *dest = dest.saturating_add_signed(delta);
            }
        };
        match self {
            Self::BranchCondition { destination, .. } | Self::Jump { destination } => {
                adjust(destination);
            }
            Self::ZoneOp(_)
            | Self::ApplyContinuous(_)
            | Self::ForEach { .. }
            | Self::ForEachPlayerQuery { .. }
            | Self::BindEnv { .. }
            | Self::SetObjectTag { .. }
            | Self::UpdateStateTrack { .. }
            | Self::SideDeckOp(_)
            | Self::StackOp(_) => {}
            Self::PromptChoice(spec) => {
                if let Some((_, paid_destination, declined_destination)) =
                    spec.as_mana_payment_mut()
                {
                    adjust(paid_destination);
                    adjust(declined_destination);
                }
            }
            Self::MutatePlayer(spec) => match &mut spec.op {
                crate::toolbox::PlayerMutation::RandomEvent(
                    crate::toolbox::RandomEventOp::FlipCoins {
                        lose_destination: Some(dest),
                        ..
                    },
                ) => adjust(dest),
                crate::toolbox::PlayerMutation::OfferSourceCounters {
                    placed_destination,
                    declined_destination,
                    ..
                } => {
                    adjust(placed_destination);
                    adjust(declined_destination);
                }
                _ => {}
            },
            Self::ScheduleDelayedEventTrigger { .. }
            | Self::RepeatLoop { .. }
            | Self::ScheduleDelayedStepTrigger { .. }
            | Self::EmitReflexiveTrigger { .. }
            | Self::FixedTargetClause(..)
            | Self::SearchLibrary { .. }
            | Self::OptionalSequence { .. }
            | Self::CreateTokens { .. }
            | Self::DrawCards(_)
            | Self::MutateObject(_)
            | Self::AdjustLife(_)
            | Self::DealDamage(_) => {}
        }
    }
    /// Whether an instruction in a chosen mode requires that mode's target to be legal.
    /// Exhaustive over every `Instruction` variant so newly added targeted instructions
    /// cannot silently be treated as untargeted in modal spells.
    pub fn uses_mode_target(&self) -> bool {
        use ResolvingPlayer::{
            ExiledTargetController, TargetController, TargetOwner, TargetPlayer,
            TargetSpellController,
        };
        let is_target_player = |p: ResolvingPlayer| {
            matches!(
                p,
                TargetPlayer
                    | TargetController
                    | TargetOwner
                    | TargetSpellController
                    | ExiledTargetController
            )
        };
        let filter_uses_target =
            |f: &ObjectFilter| f.has_target_player() || f.uses_target_permanent();
        match self {
            Self::CreateTokens { creations } => {
                (!creations.is_empty()
                    && creations.iter().all(|creation| {
                        matches!(
                            creation.recipients,
                            crate::token::TokenRecipients::ExiledTargetController
                                | crate::token::TokenRecipients::RememberedTargetController
                        )
                    })) || creations.iter().any(|c| {
                    matches!(
                        c.attacking_defender,
                        Some(
                            crate::token::AttackingDefenderScope::Player(p)
                                | crate::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(p)
                        ) if is_target_player(p)
                    ) || c.amount.uses_target()
                        || match &c.base_size {
                            Some(crate::token::TokenSize::AtCreation(e)) => e.uses_target(),
                            Some(crate::token::TokenSize::AtCreationPair { power, toughness }) => {
                                power.uses_target() || toughness.uses_target()
                            }
                            _ => false,
                        }
                })
            }
            Self::SearchLibrary { filter } => {
                matches!(
                    filter.source,
                    SearchSource::TargetOpponentLibrary | SearchSource::TargetPlayerLibrary
                ) || filter.curse_host() == Some(AuraSearchHost::TargetPlayer)
                    || is_target_player(filter.searcher)
                    || matches!(&filter.count, SearchCount::UpToExpr(e) if e.uses_target())
                    || match &filter.matcher {
                        SearchCardMatcher::Filtered(f) => filter_uses_target(f),
                        SearchCardMatcher::SameNameAs { reference, extra } => {
                            matches!(reference, MoveSubjectRef::TargetObject)
                                || extra.as_deref().is_some_and(filter_uses_target)
                        }
                        _ => false,
                    }
            }
            Self::OptionalSequence { chooser, .. } => is_target_player(*chooser),
            Self::RepeatLoop { body, .. } => body.instructions.iter().any(Self::uses_mode_target),
            Self::ScheduleDelayedEventTrigger { body, .. } => {
                body.target == BodyTarget::None
                    && body.instructions.iter().any(Self::uses_mode_target)
            }
            Self::ZoneOp(spec) => spec.uses_target(),
            Self::DrawCards(spec) => spec.uses_target(),
            Self::MutateObject(spec) => spec.uses_target(),
            Self::AdjustLife(spec) => spec.uses_target(),
            Self::DealDamage(spec) => spec.uses_target(),
            Self::MutatePlayer(spec) => spec.uses_target(),
            Self::ApplyContinuous(spec) => spec.uses_target(),
            Self::PromptChoice(spec) => spec.uses_target(),
            Self::BranchCondition { condition, .. } => condition.uses_target(),
            Self::ForEach { query, body, .. } => {
                query.uses_target() || body.instructions.iter().any(Self::uses_mode_target)
            }
            Self::ForEachPlayerQuery { selector, body, .. } => {
                selector.uses_target() || body.instructions.iter().any(Self::uses_mode_target)
            }
            Self::BindEnv { .. } => false,
            Self::SetObjectTag { query, .. } => query.uses_target(),
            Self::UpdateStateTrack { key, op } => {
                let key_uses = match &key.scope {
                    crate::toolbox::StateTrackScope::Global => false,
                    crate::toolbox::StateTrackScope::Player(p) => p.uses_target(),
                    crate::toolbox::StateTrackScope::Object(q) => q.uses_target(),
                };
                let op_uses = match op {
                    crate::toolbox::StateTrackOp::SetInt(e)
                    | crate::toolbox::StateTrackOp::AddInt(e) => e.uses_target(),
                    crate::toolbox::StateTrackOp::SetSeat(p) => p.uses_target(),
                    _ => false,
                };
                key_uses || op_uses
            }
            Self::SideDeckOp(spec) => spec
                .owner
                .as_ref()
                .is_some_and(crate::toolbox::PlayerSelector::uses_target),
            Self::StackOp(spec) => spec.uses_target(),
            Self::ScheduleDelayedStepTrigger { .. }
            | Self::EmitReflexiveTrigger { .. }
            | Self::FixedTargetClause(..)
            | Self::Jump { .. } => false,
        }
    }
    /// Whether this operation can be the single effect of an optional wrapper.
    /// Admission and runtime availability share this exhaustive language boundary.
    pub(crate) fn is_optional_primitive(&self) -> bool {
        match self {
            Self::ZoneOp(spec) => {
                if let Some((players, _)) = spec.as_group_discard() {
                    return matches!(players, AffectedPlayers::One(_));
                }
                if let Some((players, _, _, _)) = spec.as_group_sacrifice() {
                    return matches!(players, AffectedPlayers::One(_));
                }
                match &spec.destination {
                    crate::toolbox::ZoneDest::Delayed(action) => !matches!(
                        action,
                        crate::toolbox::DelayedZoneAction::ReturnChosenXGraveyardCreatures
                            | crate::toolbox::DelayedZoneAction::ReturnGraveyardToHand(_)
                            | crate::toolbox::DelayedZoneAction::ReturnExiledTargetCohort { .. }
                            | crate::toolbox::DelayedZoneAction::ExileObjects(_)
                    ),
                    _ => true,
                }
            }
            Self::MutateObject(spec) => !matches!(
                spec.op,
                crate::toolbox::ObjectMutation::KeywordZoneAction(
                    crate::toolbox::KeywordZoneOp::ReturnEarthbentCard
                        | crate::toolbox::KeywordZoneOp::SacrificeEvoked
                        | crate::toolbox::KeywordZoneOp::ExileDefeatedSiege
                ) | crate::toolbox::ObjectMutation::ResolveGrowth(_)
                    | crate::toolbox::ObjectMutation::ResolveCounterKeyword(_)
                    | crate::toolbox::ObjectMutation::StateMarker(
                        crate::toolbox::ObjectStateMarkerOp::RememberPermanentTarget
                    )
            ),
            Self::MutatePlayer(spec) => !matches!(
                spec.op,
                crate::toolbox::PlayerMutation::LibraryTop(
                    crate::toolbox::LibraryTopAction::MayRevealTop
                ) | crate::toolbox::PlayerMutation::ChooseRevealedHandCard { .. }
                    | crate::toolbox::PlayerMutation::KeywordAction(
                        crate::toolbox::PlayerKeywordAction::ExtortDrain
                    )
                    | crate::toolbox::PlayerMutation::OfferSourceCounters { .. }
                    | crate::toolbox::PlayerMutation::SacrificeRandomUntilLand
                    | crate::toolbox::PlayerMutation::ExileOnePerPlayerUntilSourceLeaves
            ),
            Self::StackOp(spec) => !matches!(
                spec.op,
                crate::toolbox::StackOpKind::ChooseFoundSpell
                    | crate::toolbox::StackOpKind::CastFoundSpell(_)
                    | crate::toolbox::StackOpKind::Encore
            ),
            Self::DealDamage(spec) => !matches!(
                spec.recipients,
                crate::toolbox::DamageTargetQuery::FromGrantingPermanent
            ),
            Self::PromptChoice(spec) => !matches!(
                spec.domain,
                crate::toolbox::PromptDomain::ManaPayment { .. }
            ),
            Self::FixedTargetClause(_)
            | Self::DrawCards(_)
            | Self::AdjustLife(_)
            | Self::ApplyContinuous(_)
            | Self::ForEach { .. }
            | Self::ForEachPlayerQuery { .. }
            | Self::BindEnv { .. }
            | Self::SetObjectTag { .. }
            | Self::UpdateStateTrack { .. }
            | Self::SideDeckOp(_)
            | Self::ScheduleDelayedEventTrigger { .. }
            | Self::RepeatLoop { .. }
            | Self::ScheduleDelayedStepTrigger { .. }
            | Self::EmitReflexiveTrigger { .. }
            | Self::CreateTokens { .. } => true,
            Self::SearchLibrary { filter } if filter.supports_optional_wrapper() => true,
            Self::SearchLibrary { .. }
            | Self::BranchCondition { .. }
            | Self::Jump { .. }
            | Self::OptionalSequence { .. } => false,
        }
    }
}
/// Disturb casts a transforming card from its own graveyard using the reverse face and this price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disturb {
    pub cost: ManaCost,
    pub back: Program,
}

/// The half proposed for a split spell (CR 709.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitHalf {
    Left,
    Right,
}

/// Two spell programs printed on one physical card. Aftermath is on the right half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCard {
    pub left: Program,
    pub right: Program,
    pub aftermath: bool,
    pub fuse: bool,
}
impl SplitCard {
    pub fn half(&self, half: SplitHalf) -> &Program {
        match half {
            SplitHalf::Left => &self.left,
            SplitHalf::Right => &self.right,
        }
    }

    /// Whether this split card is a Duskmourn Room permanent with two Room doors (CR 709.5).
    #[must_use]
    pub fn is_room(&self) -> bool {
        !self.aftermath
            && self.left.permanent().is_some()
            && self.right.permanent().is_some()
            && self
                .left
                .enchantment_subtypes
                .contains(&EnchantmentSubtype::Room)
            && self
                .right
                .enchantment_subtypes
                .contains(&EnchantmentSubtype::Room)
    }

    /// Combine the two names, mana costs and spell/permanent characteristics outside the stack (CR 709.4, 709.5).
    pub fn into_program(self) -> Option<Program> {
        match (&self.left.kind, &self.right.kind) {
            (ProgramKind::Spell(left), ProgramKind::Spell(right)) => {
                let mut combined = self.left.clone();
                let ProgramKind::Spell(spell) = &mut combined.kind else {
                    return None;
                };
                spell.cost.generic = left.cost.generic.plus(&right.cost.generic);
                spell.uncastable_mana_cost =
                    left.uncastable_mana_cost && right.uncastable_mana_cost;
                for (mana, amount) in &right.cost.required {
                    let total = spell.cost.required.entry(*mana).or_insert_with(|| 0.into());
                    *total = total.plus(amount);
                }
                combined.names.add(&self.right.names);
                combined
                    .hybrid_symbols
                    .extend_from_slice(&self.right.hybrid_symbols);
                combined
                    .card_types
                    .extend(self.right.card_types.iter().copied());
                combined
                    .spell_subtypes
                    .extend(self.right.spell_subtypes.iter().copied());
                combined
                    .printed_keywords
                    .extend(self.right.printed_keywords.iter().copied());
                combined.cannot_be_countered |= self.right.cannot_be_countered;
                match (&self.left.color_indicator, &self.right.color_indicator) {
                    (Some(left_colors), Some(right_colors)) => {
                        let mut colors = left_colors.clone();
                        colors.extend(right_colors.iter().copied());
                        combined.color_indicator = Some(colors);
                    }
                    (Some(colors), None) | (None, Some(colors)) => {
                        combined.color_indicator = Some(colors.clone());
                    }
                    (None, None) => {}
                }
                combined.x_mana_symbols = self
                    .left
                    .x_mana_symbols
                    .checked_add(self.right.x_mana_symbols)?;
                combined.split = Some(Box::new(self));
                Some(combined)
            }
            (ProgramKind::Permanent(left_perm), ProgramKind::Permanent(right_perm))
                if self.is_room() =>
            {
                let left_cost = left_perm.casting_cost.clone()?;
                let right_cost = right_perm.casting_cost.clone()?;
                let mut combined = self.left.clone();
                let ProgramKind::Permanent(perm) = &mut combined.kind else {
                    return None;
                };
                let mut total_cost = left_cost.clone();
                total_cost.generic = total_cost.generic.plus(&right_cost.generic);
                for (mana, amount) in &right_cost.required {
                    let entry = total_cost.required.entry(*mana).or_insert_with(|| 0.into());
                    *entry = entry.plus(amount);
                }
                perm.casting_cost = Some(total_cost);
                if perm.entry_tap.is_none() {
                    perm.entry_tap = right_perm.entry_tap;
                }
                combined.names.add(&self.right.names);
                combined
                    .hybrid_symbols
                    .extend_from_slice(&self.right.hybrid_symbols);
                combined
                    .supertypes
                    .extend(self.right.supertypes.iter().copied());
                combined
                    .card_types
                    .extend(self.right.card_types.iter().copied());
                combined
                    .creature_subtypes
                    .extend(self.right.creature_subtypes.iter().copied());
                combined
                    .land_subtypes
                    .extend(self.right.land_subtypes.iter().copied());
                combined
                    .spell_subtypes
                    .extend(self.right.spell_subtypes.iter().copied());
                combined
                    .artifact_subtypes
                    .extend(self.right.artifact_subtypes.iter().copied());
                combined
                    .enchantment_subtypes
                    .extend(self.right.enchantment_subtypes.iter().copied());
                combined
                    .planeswalker_subtypes
                    .extend(self.right.planeswalker_subtypes.iter().copied());
                combined
                    .printed_keywords
                    .extend(self.right.printed_keywords.iter().copied());
                combined.x_mana_symbols = self
                    .left
                    .x_mana_symbols
                    .checked_add(self.right.x_mana_symbols)?;

                let gate_static =
                    |existing: Option<crate::characteristics::StaticSourceCondition>,
                     half: SplitHalf| {
                        let door =
                            crate::characteristics::StaticSourceCondition::RoomDoorUnlocked(half);
                        match existing {
                            Some(prev) => crate::characteristics::StaticSourceCondition::And(
                                Box::new(door),
                                Box::new(prev),
                            ),
                            None => door,
                        }
                    };
                let gate_trigger = |mut trig: TriggeredAbility, half: SplitHalf| {
                    if let TriggerEvent::DoorUnlocked {
                        subject,
                        fully_unlocked_only: false,
                        door,
                    } = &mut trig.condition.event
                        && *subject == ObjectFilter::self_object()
                    {
                        *door = Some(half);
                    } else if trig.condition.zone == TriggerZone::Battlefield {
                        let req = TriggerCondition::RoomDoorUnlocked(half);
                        trig.condition.intervening_if = match trig.condition.intervening_if.take() {
                            Some(prev) => {
                                Some(TriggerCondition::And(Box::new(req), Box::new(prev)))
                            }
                            None => Some(req),
                        };
                    }
                    trig
                };
                let gate_activation =
                    |existing: Option<crate::activated::ActivationRestriction>,
                     req: crate::activated::ActivationRestriction| {
                        match existing {
                            Some(crate::activated::ActivationRestriction::All(mut items)) => {
                                items.insert(0, req);
                                crate::activated::ActivationRestriction::All(items)
                            }
                            Some(prev) => {
                                crate::activated::ActivationRestriction::All(vec![req, prev])
                            }
                            None => req,
                        }
                    };

                // Gate Left static abilities, triggers, and activated abilities on Left door unlocked.
                let left_static_ids: Vec<_> = combined
                    .continuous
                    .iter()
                    .chain(&combined.additional_battlefield_abilities)
                    .map(|a| a.id())
                    .collect();
                for id in &left_static_ids {
                    let prev = combined.static_conditions.remove(id);
                    combined
                        .static_conditions
                        .insert(*id, gate_static(prev, SplitHalf::Left));
                }
                let mut next_static = left_static_ids
                    .iter()
                    .map(|id| id.0)
                    .max()
                    .map_or(1, |m| m.saturating_add(1));

                for ability in self
                    .right
                    .continuous
                    .iter()
                    .chain(&self.right.additional_battlefield_abilities)
                {
                    let old_id = ability.id();
                    let new_id = crate::characteristics::StaticAbilityId(next_static);
                    next_static = next_static.saturating_add(1);
                    let shifted = ability.clone().with_component_id(new_id);
                    if combined.continuous.is_none() {
                        combined.continuous = Some(shifted);
                    } else if shifted.allowed_as_additional() {
                        combined.additional_battlefield_abilities.push(shifted);
                    } else {
                        return None;
                    }
                    let prev = self.right.static_conditions.get(&old_id).cloned();
                    combined
                        .static_conditions
                        .insert(new_id, gate_static(prev, SplitHalf::Right));
                }

                // Gate Left triggers and shift + gate Right triggers.
                combined.triggered = combined
                    .triggered
                    .into_iter()
                    .map(|t| gate_trigger(t, SplitHalf::Left))
                    .collect();
                let mut next_trig = combined
                    .triggered
                    .iter()
                    .map(|t| t.id.0)
                    .max()
                    .map_or(1, |m| m.saturating_add(1));
                for mut trig in self.right.triggered.clone() {
                    trig.id = TriggeredAbilityId(next_trig);
                    next_trig = next_trig.saturating_add(1);
                    combined
                        .triggered
                        .push(gate_trigger(trig, SplitHalf::Right));
                }

                // Shift Right mana abilities.
                let mut next_mana = combined
                    .mana
                    .iter()
                    .map(|m| m.id.0)
                    .max()
                    .map_or(1, |m| m.saturating_add(1));
                for mut mana in self.right.mana.clone() {
                    mana.id = crate::mana::ManaAbilityId(next_mana);
                    next_mana = next_mana.saturating_add(1);
                    combined.mana.push(mana);
                }

                // Shift Right zone replacements.
                let mut next_zr = combined
                    .zone_replacements
                    .iter()
                    .map(|z| z.id.0)
                    .max()
                    .map_or(1, |m| m.saturating_add(1));
                for mut zr in self.right.zone_replacements.clone() {
                    let old_id = zr.id;
                    let new_id = ZoneReplacementAbilityId(next_zr);
                    next_zr = next_zr.saturating_add(1);
                    zr.id = new_id;
                    combined.zone_replacements.push(zr);
                    if let Some(extra) = self.right.zone_replacement_extras.get(&old_id) {
                        combined
                            .zone_replacement_extras
                            .insert(new_id, extra.clone());
                    }
                }

                // Gate Left activated abilities and shift + gate Right activated abilities.
                let left_act_ids: Vec<_> = combined.activated.iter().map(|a| a.id).collect();
                for id in left_act_ids {
                    let prev = combined.activation_restrictions.remove(&id);
                    combined.activation_restrictions.insert(
                        id,
                        gate_activation(
                            prev,
                            crate::activated::ActivationRestriction::RoomDoorUnlocked(
                                SplitHalf::Left,
                            ),
                        ),
                    );
                }
                let mut next_act = combined
                    .activated
                    .iter()
                    .map(|a| a.id.0)
                    .max()
                    .map_or(1, |m| m.saturating_add(1));
                for mut act in self.right.activated.clone() {
                    let old_id = act.id;
                    let new_id = crate::activated::ActivatedAbilityId(next_act);
                    next_act = next_act.saturating_add(1);
                    act.id = new_id;
                    combined.activated.push(act);
                    let prev = self.right.activation_restrictions.get(&old_id).cloned();
                    combined.activation_restrictions.insert(
                        new_id,
                        gate_activation(
                            prev,
                            crate::activated::ActivationRestriction::RoomDoorUnlocked(
                                SplitHalf::Right,
                            ),
                        ),
                    );
                    if let Some(v) = self.right.activation_modes.get(&old_id) {
                        combined.activation_modes.insert(new_id, v.clone());
                    }
                    if let Some(v) = self.right.activation_hybrid_symbols.get(&old_id) {
                        combined.activation_hybrid_symbols.insert(new_id, v.clone());
                    }
                    if let Some(v) = self.right.extra_activation_costs.get(&old_id) {
                        combined.extra_activation_costs.insert(new_id, v.clone());
                    }
                    if let Some(v) = self.right.activation_cost_reductions.get(&old_id) {
                        combined
                            .activation_cost_reductions
                            .insert(new_id, v.clone());
                    }
                    if let Some(v) = self.right.activation_target_cost_reductions.get(&old_id) {
                        combined
                            .activation_target_cost_reductions
                            .insert(new_id, *v);
                    }
                    if let Some(v) = self.right.loyalty_costs.get(&old_id) {
                        combined.loyalty_costs.insert(new_id, *v);
                    }
                    if self.right.variable_loyalty_costs.contains(&old_id) {
                        combined.variable_loyalty_costs.insert(new_id);
                    }
                }

                // Append sorcery-speed door unlock abilities for both doors (CR 709.5e).
                for (half, cost, hybrids) in [
                    (SplitHalf::Left, left_cost, &self.left.hybrid_symbols),
                    (SplitHalf::Right, right_cost, &self.right.hybrid_symbols),
                ] {
                    let unlock_id = crate::activated::ActivatedAbilityId(next_act);
                    next_act = next_act.saturating_add(1);
                    combined.activated.push(crate::activated::ActivatedAbility {
                        id: unlock_id,
                        mana_cost: cost,
                        source_cost: crate::activated::SourceCost::None,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::MutateObject(Box::new(
                                crate::toolbox::MutateObjectSpec::unlock_door(
                                    crate::toolbox::ObjectQuery::Source,
                                    Some(half),
                                    false,
                                ),
                            ))],
                        ),
                    });
                    if !hybrids.is_empty() {
                        combined
                            .activation_hybrid_symbols
                            .insert(unlock_id, hybrids.clone());
                    }
                    combined.activation_restrictions.insert(
                        unlock_id,
                        crate::activated::ActivationRestriction::All(vec![
                            crate::activated::ActivationRestriction::Sorcery,
                            crate::activated::ActivationRestriction::RoomDoorLocked(half),
                        ]),
                    );
                }

                combined.split = Some(Box::new(self));
                Some(combined)
            }
            _ => None,
        }
    }
}

/// Characteristics a card must have before being returned from a graveyard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveyardReturnFilter {
    Permanent,
    CardType(CardType),
    CreatureSubtype(CreatureTypeId),
}

/// Additive continuous effects applied as a card enters the battlefield.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryAdditions {
    pub creature_subtype: Option<CreatureTypeId>,
    pub color: Option<crate::characteristics::Color>,
}

/// Independent selection, destination control and entry choices for a graveyard cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraveyardCohortReturn {
    pub filter: GraveyardReturnFilter,
    pub from_all_graveyards: bool,
    pub under_resolving_players_control: bool,
    pub tapped: bool,
    pub additions: EntryAdditions,
}
impl GraveyardCohortReturn {
    /// Return qualifying cards from the resolving player's graveyard under their control.
    pub fn own(filter: GraveyardReturnFilter) -> Self {
        Self {
            filter,
            from_all_graveyards: false,
            under_resolving_players_control: true,
            tapped: false,
            additions: EntryAdditions::default(),
        }
    }
}

/// Zone scope for a single-object [`BodyTarget::Object`] target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetZone {
    Battlefield,
    OwnGraveyard,
    AnyGraveyard { opponents_only: bool },
    Exile { owned_flashback_only: bool },
    RecentlyDiscardedOrFromLibrary,
}

/// Recipient scope for a [`BodyTarget::AnyTarget`] target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyTargetKind {
    Any,
    CreatureOrPlayer,
    PlayerOrPlaneswalker,
    OpponentOrPlaneswalker,
    OpponentOrPlaneswalkerThenCreature,
}

/// Stack-entry scope for a [`BodyTarget::StackEntry`] target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackTargetKind {
    AnyEntry,
    TargetingControlledPermanent,
    ControlledAbility { colorless_source: bool },
}

/// The body's admission gate is independent of its instruction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyTarget {
    None,
    Object {
        zone: TargetZone,
        filter: ObjectFilter,
    },
    Player {
        opponents_only: bool,
    },
    AnyTarget(AnyTargetKind),
    Spell {
        filter: SpellFilter,
        include_abilities: bool,
    },
    StackEntry(StackTargetKind),
}

impl BodyTarget {
    pub const SINGLE_PERMANENT: Self = Self::Object {
        zone: TargetZone::Battlefield,
        filter: ObjectFilter::any(),
    };
    pub const SINGLE_CREATURE: Self = Self::Object {
        zone: TargetZone::Battlefield,
        filter: ObjectFilter::creature(),
    };
    pub const SINGLE_PLANESWALKER: Self = Self::Object {
        zone: TargetZone::Battlefield,
        filter: ObjectFilter::planeswalker(),
    };
    pub const SINGLE_OWN_GRAVEYARD_CARD: Self = Self::Object {
        zone: TargetZone::OwnGraveyard,
        filter: ObjectFilter::any(),
    };
    pub const ANY_GRAVEYARD_CARD: Self = Self::Object {
        zone: TargetZone::AnyGraveyard {
            opponents_only: false,
        },
        filter: ObjectFilter::any(),
    };
    pub const ANY_GRAVEYARD_CREATURE: Self = Self::Object {
        zone: TargetZone::AnyGraveyard {
            opponents_only: false,
        },
        filter: ObjectFilter::creature(),
    };
    pub const ANY_GRAVEYARD_CREATURE_OR_PLANESWALKER: Self = Self::Object {
        zone: TargetZone::AnyGraveyard {
            opponents_only: false,
        },
        filter: ObjectFilter::creature_or_planeswalker(),
    };
    pub const OWNED_EXILED_FLASHBACK_CARD: Self = Self::Object {
        zone: TargetZone::Exile {
            owned_flashback_only: true,
        },
        filter: ObjectFilter::any(),
    };
    pub const RECENTLY_DISCARDED_OR_FROM_LIBRARY: Self = Self::Object {
        zone: TargetZone::RecentlyDiscardedOrFromLibrary,
        filter: ObjectFilter::any(),
    };
    pub const SINGLE_PLAYER: Self = Self::Player {
        opponents_only: false,
    };
    pub const SINGLE_OPPONENT: Self = Self::Player {
        opponents_only: true,
    };
    pub const SINGLE_ANY_TARGET: Self = Self::AnyTarget(AnyTargetKind::Any);
    pub const SINGLE_CREATURE_OR_PLAYER: Self = Self::AnyTarget(AnyTargetKind::CreatureOrPlayer);
    pub const SINGLE_PLAYER_OR_PLANESWALKER: Self =
        Self::AnyTarget(AnyTargetKind::PlayerOrPlaneswalker);
    pub const SINGLE_OPPONENT_OR_PLANESWALKER: Self =
        Self::AnyTarget(AnyTargetKind::OpponentOrPlaneswalker);
    pub const OPPONENT_OR_PLANESWALKER_THEN_CREATURE: Self =
        Self::AnyTarget(AnyTargetKind::OpponentOrPlaneswalkerThenCreature);
    pub const SINGLE_STACK_ENTRY: Self = Self::StackEntry(StackTargetKind::AnyEntry);
    pub const STACK_ENTRY_TARGETING_CONTROLLED_PERMANENT: Self =
        Self::StackEntry(StackTargetKind::TargetingControlledPermanent);

    #[must_use]
    pub const fn filtered_permanent(filter: ObjectFilter) -> Self {
        Self::Object {
            zone: TargetZone::Battlefield,
            filter,
        }
    }

    #[must_use]
    pub const fn filtered_own_graveyard(filter: ObjectFilter) -> Self {
        Self::Object {
            zone: TargetZone::OwnGraveyard,
            filter,
        }
    }

    #[must_use]
    pub const fn filtered_any_graveyard(filter: ObjectFilter, opponents_only: bool) -> Self {
        Self::Object {
            zone: TargetZone::AnyGraveyard { opponents_only },
            filter,
        }
    }

    #[must_use]
    pub const fn filtered_exiled(filter: ObjectFilter) -> Self {
        Self::Object {
            zone: TargetZone::Exile {
                owned_flashback_only: false,
            },
            filter,
        }
    }

    #[must_use]
    pub const fn single_spell(filter: SpellFilter) -> Self {
        Self::Spell {
            filter,
            include_abilities: false,
        }
    }

    #[must_use]
    pub const fn single_stack_object(filter: SpellFilter) -> Self {
        Self::Spell {
            filter,
            include_abilities: true,
        }
    }

    #[must_use]
    pub const fn controlled_ability(colorless_source: bool) -> Self {
        Self::StackEntry(StackTargetKind::ControlledAbility { colorless_source })
    }

    pub fn guarantees_creature(self) -> bool {
        matches!(
            self,
            Self::Object {
                zone: TargetZone::Battlefield,
                filter,
            } if filter.guarantees_creature()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableBody {
    pub target: BodyTarget,
    pub instructions: Vec<Instruction>,
    pub(crate) triggered_modes: Option<TriggeredModes>,
    graveyard_target_bounds: Option<(usize, usize)>,
    pub permanent_target_bounds: Option<(u16, u16)>,
    x_target_count: bool,
    up_to_x_target_count: bool,
    up_to_one_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependentTargets {
    CreatureControlledByPrimary,
}

impl DependentTargets {
    pub(crate) fn maximum(self) -> usize {
        match self {
            Self::CreatureControlledByPrimary => 1,
        }
    }
}

/// Modal abilities choose the printed modes and their targets during trigger placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredModes {
    pub minimum: u8,
    pub maximum: u8,
    pub distinct_targets: bool,
    pub allow_repeat: bool,
    pub uniqueness: ModalUniqueness,
    pub prefix: Option<Box<ExecutableBody>>,
    pub suffix: Option<Box<ExecutableBody>>,
    pub options: Vec<ExecutableBody>,
}
fn toolbox_merge_family(
    current: Option<OperandFamily>,
    next: Option<OperandFamily>,
) -> Result<Option<OperandFamily>, DefinitionError> {
    match (current, next) {
        (None, other) | (other, None) => Ok(other),
        (Some(left), Some(right)) if left == right => Ok(Some(left)),
        (Some(_), Some(_)) => Err(DefinitionError::OperandFamily),
    }
}

fn toolbox_object_target_family(target: BodyTarget) -> Result<OperandFamily, DefinitionError> {
    match target {
        BodyTarget::Object {
            zone: TargetZone::Battlefield,
            ..
        } => Ok(OperandFamily::Permanent),
        BodyTarget::Object {
            zone:
                TargetZone::OwnGraveyard
                | TargetZone::AnyGraveyard { .. }
                | TargetZone::RecentlyDiscardedOrFromLibrary,
            ..
        } => Ok(OperandFamily::GraveyardCard),
        BodyTarget::Object {
            zone: TargetZone::Exile { .. },
            ..
        } => Ok(OperandFamily::ExiledCard),
        BodyTarget::Spell {
            include_abilities: false,
            ..
        } => Ok(OperandFamily::Spell),
        BodyTarget::Spell {
            include_abilities: true,
            ..
        }
        | BodyTarget::StackEntry(_) => Ok(OperandFamily::StackEntry),
        BodyTarget::AnyTarget(_) => Ok(OperandFamily::DamageRecipient),
        BodyTarget::None | BodyTarget::Player { .. } => Err(DefinitionError::OperandFamily),
    }
}

fn inherited_target_family(target: BodyTarget) -> Option<OperandFamily> {
    match target {
        BodyTarget::Player { .. } => Some(OperandFamily::Player),
        BodyTarget::Object {
            zone: TargetZone::Battlefield,
            ..
        } => Some(OperandFamily::Permanent),
        BodyTarget::AnyTarget(
            AnyTargetKind::Any
            | AnyTargetKind::CreatureOrPlayer
            | AnyTargetKind::PlayerOrPlaneswalker
            | AnyTargetKind::OpponentOrPlaneswalker,
        ) => Some(OperandFamily::DamageRecipient),
        BodyTarget::Spell {
            include_abilities: false,
            ..
        } => Some(OperandFamily::Spell),
        BodyTarget::Spell {
            include_abilities: true,
            ..
        }
        | BodyTarget::StackEntry(_) => Some(OperandFamily::StackEntry),
        _ => Some(OperandFamily::GraveyardCard),
    }
}

fn validate_toolbox_player_selector(
    selector: &crate::toolbox::PlayerSelector,
    context: BodyContext,
    target: BodyTarget,
    definitely_exiled_target: bool,
) -> Result<Option<OperandFamily>, DefinitionError> {
    use crate::toolbox::PlayerSelector;
    match selector {
        PlayerSelector::Resolving(role) => role.validate(context, target, definitely_exiled_target),
        PlayerSelector::Group(players) => {
            players.validate(context, target, definitely_exiled_target)
        }
        PlayerSelector::OwnersOf(query) | PlayerSelector::ControllersOf(query) => {
            validate_toolbox_object_query(query, context, target, definitely_exiled_target)
        }
        PlayerSelector::Bound(_) | PlayerSelector::Fixed(_) => Ok(None),
    }
}

fn validate_toolbox_filter(
    filter: &ObjectFilter,
    target: BodyTarget,
) -> Result<Option<OperandFamily>, DefinitionError> {
    let mut family = None;
    if filter.uses_target_permanent() {
        family = toolbox_merge_family(family, Some(OperandFamily::Permanent))?;
    }
    if filter.has_target_player() {
        let f = match target {
            BodyTarget::Player { .. } => OperandFamily::Player,
            _ => toolbox_object_target_family(target)?,
        };
        family = toolbox_merge_family(family, Some(f))?;
    }
    Ok(family)
}

fn validate_toolbox_zone(
    zone: &crate::toolbox::QueryZone,
    context: BodyContext,
    target: BodyTarget,
    definitely_exiled_target: bool,
) -> Result<Option<OperandFamily>, DefinitionError> {
    use crate::toolbox::QueryZone;
    match zone {
        QueryZone::Hand(p)
        | QueryZone::Library(p)
        | QueryZone::Graveyard(p)
        | QueryZone::ExileOwnedBy(p)
        | QueryZone::BattlefieldControlledBy(p) => {
            validate_toolbox_player_selector(p, context, target, definitely_exiled_target)
        }
        QueryZone::Battlefield
        | QueryZone::Exile
        | QueryZone::Stack
        | QueryZone::Command
        | QueryZone::AllZones => Ok(None),
    }
}

fn validate_toolbox_object_query(
    query: &crate::toolbox::ObjectQuery,
    context: BodyContext,
    target: BodyTarget,
    definitely_exiled_target: bool,
) -> Result<Option<OperandFamily>, DefinitionError> {
    use crate::toolbox::{ObjectQuery, QuerySlice};
    match query {
        ObjectQuery::Targets | ObjectQuery::TargetsWithPairedPartner => {
            Ok(Some(toolbox_object_target_family(target)?))
        }
        ObjectQuery::LinkedTarget { .. } => Ok(None),
        ObjectQuery::EffectSubject(subject) => match subject.as_ref() {
            EffectSubject::TargetObject
            | EffectSubject::TargetAndSameNameMatching(_)
            | EffectSubject::MatchingOtherThanTarget(_)
            | EffectSubject::TargetAndSharingColorMatching(_) => {
                Ok(Some(toolbox_object_target_family(target)?))
            }
            EffectSubject::Matching(filter) => validate_toolbox_filter(filter, target),
            _ => Ok(None),
        },
        ObjectQuery::ZoneScan {
            zone,
            filter,
            slice,
        } => {
            let mut family =
                validate_toolbox_zone(zone, context, target, definitely_exiled_target)?;
            family = toolbox_merge_family(family, validate_toolbox_filter(filter, target)?)?;
            let slice_fam = match slice {
                QuerySlice::All => None,
                QuerySlice::Top(expr) | QuerySlice::Bottom(expr) => {
                    expr.required_operand_family(target)?
                }
                QuerySlice::UntilMatch { filter, count, .. } => toolbox_merge_family(
                    validate_toolbox_filter(filter, target)?,
                    count.required_operand_family(target)?,
                )?,
            };
            toolbox_merge_family(family, slice_fam)
        }
        ObjectQuery::Filtered { source, filter } => toolbox_merge_family(
            validate_toolbox_object_query(source, context, target, definitely_exiled_target)?,
            validate_toolbox_filter(filter, target)?,
        ),
        ObjectQuery::Take { source, count } => toolbox_merge_family(
            validate_toolbox_object_query(source, context, target, definitely_exiled_target)?,
            count.required_operand_family(target)?,
        ),
        ObjectQuery::BoundFiltered { filter, .. } => validate_toolbox_filter(filter, target),
        ObjectQuery::Tagged { zone, filter, .. } => {
            let zf = match zone {
                Some(z) => validate_toolbox_zone(z, context, target, definitely_exiled_target)?,
                None => None,
            };
            let ff = match filter {
                Some(f) => validate_toolbox_filter(f, target)?,
                None => None,
            };
            toolbox_merge_family(zf, ff)
        }
        ObjectQuery::Union(parts) => {
            let mut family = None;
            for part in parts {
                family = toolbox_merge_family(
                    family,
                    validate_toolbox_object_query(part, context, target, definitely_exiled_target)?,
                )?;
            }
            Ok(family)
        }
        ObjectQuery::Difference(left, right) | ObjectQuery::Intersect(left, right) => {
            toolbox_merge_family(
                validate_toolbox_object_query(left, context, target, definitely_exiled_target)?,
                validate_toolbox_object_query(right, context, target, definitely_exiled_target)?,
            )
        }
        ObjectQuery::Empty
        | ObjectQuery::Fixed(_)
        | ObjectQuery::Bound(_)
        | ObjectQuery::Source
        | ObjectQuery::TriggerSubject
        | ObjectQuery::AttachedHost
        | ObjectQuery::LastMoved
        | ObjectQuery::DestroyedThisWay
        | ObjectQuery::CreatedTokens
        | ObjectQuery::Revealed
        | ObjectQuery::LinkedExile { .. } => Ok(None),
    }
}

fn validate_toolbox_zone_op(
    spec: &crate::toolbox::ZoneOpSpec,
    context: BodyContext,
    target: BodyTarget,
    definitely_exiled_target: bool,
) -> Result<Option<OperandFamily>, DefinitionError> {
    use crate::toolbox::{LibraryOrderPos, ZoneController, ZoneDest, ZoneSelection};
    let mut family =
        validate_toolbox_object_query(&spec.source, context, target, definitely_exiled_target)?;
    let sel_fam = match &spec.selection {
        ZoneSelection::All => None,
        ZoneSelection::Random { count, chooser } => toolbox_merge_family(
            count.required_operand_family(target)?,
            validate_toolbox_player_selector(chooser, context, target, definitely_exiled_target)?,
        )?,
        ZoneSelection::Prompt {
            chooser, min, max, ..
        } => {
            let f1 = validate_toolbox_player_selector(
                chooser,
                context,
                target,
                definitely_exiled_target,
            )?;
            let f2 = toolbox_merge_family(f1, min.required_operand_family(target)?)?;
            toolbox_merge_family(f2, max.required_operand_family(target)?)?
        }
    };
    family = toolbox_merge_family(family, sel_fam)?;
    let dest_fam = match &spec.destination {
        ZoneDest::Battlefield {
            controller,
            entering_counters,
            attach_to,
            ..
        } => {
            let mut f = match controller {
                ZoneController::Owner => None,
                ZoneController::Player(p) => {
                    validate_toolbox_player_selector(p, context, target, definitely_exiled_target)?
                }
            };
            for (_, amount) in entering_counters {
                f = toolbox_merge_family(f, amount.required_operand_family(target)?)?;
            }
            if let Some(attach) = attach_to {
                if matches!(attach.as_ref(), crate::toolbox::ObjectQuery::Source)
                    && !context.has_source()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                f = toolbox_merge_family(
                    f,
                    validate_toolbox_object_query(
                        attach,
                        context,
                        target,
                        definitely_exiled_target,
                    )?,
                )?;
            }
            f
        }
        ZoneDest::Exile {
            viewer,
            delayed_return,
            ..
        } => {
            if matches!(
                delayed_return,
                Some(DelayedReturnTiming::UntilSourceLeaves | DelayedReturnTiming::LinkedGroup(_))
            ) && !context.has_source()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            match viewer {
                Some(p) => {
                    validate_toolbox_player_selector(p, context, target, definitely_exiled_target)?
                }
                None => None,
            }
        }
        ZoneDest::LookOnly { viewer } => {
            validate_toolbox_player_selector(viewer, context, target, definitely_exiled_target)?
        }
        ZoneDest::Library { position } => match position {
            LibraryOrderPos::Top
            | LibraryOrderPos::Bottom
            | LibraryOrderPos::BottomRandom
            | LibraryOrderPos::ShuffleInto => None,
            LibraryOrderPos::NthFromTop(expr) => expr.required_operand_family(target)?,
            LibraryOrderPos::ChoiceTopOrBottom { chooser } => validate_toolbox_player_selector(
                chooser,
                context,
                target,
                definitely_exiled_target,
            )?,
        },
        ZoneDest::SwapZones { other } => {
            if let crate::toolbox::ObjectQuery::ZoneScan { zone: from, .. } = &spec.source
                && from == other
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            validate_toolbox_zone(other, context, target, definitely_exiled_target)?
        }
        ZoneDest::None
        | ZoneDest::Graveyard { .. }
        | ZoneDest::Hand
        | ZoneDest::RevealOnly
        | ZoneDest::Delayed(_) => None,
    };
    family = toolbox_merge_family(family, dest_fam)?;
    if let Some(shuffler) = &spec.shuffle_library {
        family = toolbox_merge_family(
            family,
            validate_toolbox_player_selector(shuffler, context, target, definitely_exiled_target)?,
        )?;
    }
    Ok(family)
}

fn toolbox_query_scans_graveyard(query: &crate::toolbox::ObjectQuery) -> bool {
    use crate::toolbox::{ObjectQuery, QueryZone};
    match query {
        ObjectQuery::ZoneScan {
            zone: QueryZone::Graveyard(_),
            ..
        } => true,
        ObjectQuery::Filtered { source, .. } | ObjectQuery::Take { source, .. } => {
            toolbox_query_scans_graveyard(source)
        }
        ObjectQuery::Union(parts) => parts.iter().any(toolbox_query_scans_graveyard),
        ObjectQuery::Difference(left, _) | ObjectQuery::Intersect(left, _) => {
            toolbox_query_scans_graveyard(left)
        }
        _ => false,
    }
}

fn toolbox_query_scans_library(query: &crate::toolbox::ObjectQuery) -> bool {
    use crate::toolbox::{ObjectQuery, QueryZone};
    match query {
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(_),
            ..
        } => true,
        ObjectQuery::Filtered { source, .. } | ObjectQuery::Take { source, .. } => {
            toolbox_query_scans_library(source)
        }
        ObjectQuery::Union(parts) => parts.iter().any(toolbox_query_scans_library),
        ObjectQuery::Difference(left, _) | ObjectQuery::Intersect(left, _) => {
            toolbox_query_scans_library(left)
        }
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OperandFamily {
    GraveyardCard,
    ExiledCard,
    Permanent,
    Player,
    DamageRecipient,
    StackEntry,
    Spell,
}
impl BodyTarget {
    /// Bind source-relative targeting to the exact permanent incarnation.
    pub(crate) fn with_ability_source(self, source: crate::object_identity::ObjectId) -> Self {
        let Self::Object {
            zone: TargetZone::Battlefield,
            filter,
        } = self
        else {
            return self;
        };
        Self::Object {
            zone: TargetZone::Battlefield,
            filter: filter.with_ability_source(source),
        }
    }
}

impl ExecutableBody {
    pub(crate) fn dependent_targets(&self) -> Option<DependentTargets> {
        match (self.target, self.instructions.first()) {
            (BodyTarget::AnyTarget(AnyTargetKind::OpponentOrPlaneswalkerThenCreature), _) => {
                Some(DependentTargets::CreatureControlledByPrimary)
            }
            _ => None,
        }
    }

    /// Bounds of the one printed variable-cardinality target clause, if present.
    pub fn ordered_any_target_bounds(&self) -> Option<(usize, usize)> {
        match self.instructions.as_slice() {
            [Instruction::DealDamage(spec)]
                if matches!(
                    spec.recipients,
                    crate::toolbox::DamageTargetQuery::OrderedTargets { .. }
                ) && self.target == BodyTarget::OPPONENT_OR_PLANESWALKER_THEN_CREATURE =>
            {
                Some((1, 2))
            }
            [Instruction::DealDamage(spec)]
                if matches!(
                    spec.recipients,
                    crate::toolbox::DamageTargetQuery::OrderedTargets { .. }
                        | crate::toolbox::DamageTargetQuery::DivideThree
                ) && self.target == BodyTarget::SINGLE_ANY_TARGET =>
            {
                Some((1, 3))
            }
            _ => None,
        }
    }
    /// Total amount that must be assigned in positive shares while choosing targets.
    pub fn target_division_total(&self) -> Option<u16> {
        self.instructions.iter().find_map(|op| match op {
            Instruction::MutateObject(spec) => match spec.op {
                crate::toolbox::ObjectMutation::Counters(
                    crate::toolbox::CounterOp::Distribute { total, .. },
                ) => Some(total),
                _ => None,
            },
            _ => None,
        })
    }

    /// Dynamic damage total expression for `DamageTargetQuery::Divided`.
    pub fn target_division_expr(&self) -> Option<&ValueExpr> {
        self.instructions.iter().find_map(|op| match op {
            Instruction::DealDamage(spec) => match &spec.recipients {
                crate::toolbox::DamageTargetQuery::Divided { total, .. } => Some(total.as_ref()),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn permanent_target_bounds(&self) -> Option<(usize, usize)> {
        self.graveyard_target_bounds
            .or_else(|| self.non_graveyard_target_bounds())
    }

    fn non_graveyard_target_bounds(&self) -> Option<(usize, usize)> {
        if let Some((minimum, maximum)) = self.permanent_target_bounds {
            return Some((usize::from(minimum), usize::from(maximum)));
        }
        if let Some(total) = self.target_division_total() {
            return Some((1, usize::from(total)));
        }
        if let Some(max_t) = self.instructions.iter().find_map(|op| match op {
            Instruction::DealDamage(spec) => match &spec.recipients {
                crate::toolbox::DamageTargetQuery::Divided { max_targets, .. } => {
                    Some(*max_targets)
                }
                _ => None,
            },
            _ => None,
        }) {
            return Some((0, max_t.map_or(usize::MAX, usize::from)));
        }
        if self.target == BodyTarget::None {
            return None;
        }
        if self.up_to_one_target {
            return Some((0, 1));
        }
        match self.instructions.as_slice() {
            [Instruction::ZoneOp(spec)]
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Delayed(
                        crate::toolbox::DelayedZoneAction::ReturnChosenXGraveyardCreatures
                    )
                ) && self.target
                    == BodyTarget::filtered_own_graveyard(ObjectFilter::creature()) =>
            {
                Some((0, usize::MAX))
            }
            [Instruction::MutatePlayer(spec)]
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::ExileOnePerPlayerUntilSourceLeaves
                ) =>
            {
                Some((0, usize::MAX))
            }
            [Instruction::MutateObject(spec)]
                if matches!(spec.source, crate::toolbox::ObjectQuery::Targets)
                    && matches!(
                        spec.op,
                        crate::toolbox::ObjectMutation::Counters(
                            crate::toolbox::CounterOp::Double { .. }
                        )
                    ) =>
            {
                Some((0, usize::MAX))
            }
            [Instruction::ApplyContinuous(spec)]
                if matches!(
                    spec.payload,
                    crate::toolbox::ContinuousPayload::ModifyAnyNumber { .. }
                ) =>
            {
                Some((0, usize::MAX))
            }
            _ => self.instructions.iter().find_map(|op| match op {
                Instruction::DealDamage(spec) => match spec.recipients {
                    crate::toolbox::DamageTargetQuery::AnnouncedTargets {
                        min_targets,
                        max_targets,
                    } => Some((usize::from(min_targets), usize::from(max_targets))),
                    _ => None,
                },
                Instruction::ApplyContinuous(spec)
                    if matches!(
                        &spec.payload,
                        crate::toolbox::ContinuousPayload::TemporaryRule(rule)
                            if matches!(
                                rule.as_ref(),
                                TemporaryRuleKind::RedirectNextDamage {
                                    to: crate::characteristics::DamageRedirectTarget::SecondTargetObject,
                                    ..
                                }
                            )
                    ) =>
                {
                    Some((2, 2))
                }
                Instruction::MutateObject(spec)
                    if matches!(spec.source, crate::toolbox::ObjectQuery::Targets)
                        && matches!(
                            spec.op,
                            crate::toolbox::ObjectMutation::TokenAttach(
                                crate::toolbox::TokenAttachOp::CreateAuraAttached {
                                    up_to_one: true,
                                    ..
                                }
                            )
                        ) =>
                {
                    Some((0, 1))
                }
                _ => None,
            }),
        }
    }

    pub fn new(target: BodyTarget, instructions: Vec<Instruction>) -> Self {
        Self {
            target,
            instructions,
            triggered_modes: None,
            graveyard_target_bounds: None,
            permanent_target_bounds: None,
            x_target_count: false,
            up_to_x_target_count: false,
            up_to_one_target: false,
        }
    }

    /// Set explicit `(minimum, maximum)` bounds for multi-permanent target clauses.
    pub fn with_permanent_target_bounds(mut self, minimum: u16, maximum: u16) -> Self {
        self.permanent_target_bounds = Some((minimum, maximum));
        self
    }

    /// Allow 0 or 1 permanent target to be chosen ("up to one target ...").
    pub fn with_up_to_one_target(mut self) -> Self {
        self.up_to_one_target = true;
        self
    }

    /// True when 0 or 1 permanent target may be chosen ("up to one target ...").
    #[must_use]
    pub fn is_up_to_one_target(&self) -> bool {
        self.up_to_one_target
    }

    /// Require the spell's chosen X to equal the number of permanent/graveyard targets chosen.
    pub fn with_x_target_count(mut self) -> Self {
        self.x_target_count = true;
        self
    }

    /// True when the spell's chosen X must equal the number of selected targets.
    pub fn has_x_target_count(&self) -> bool {
        self.x_target_count
    }

    /// Allow 0..=X distinct targets to be chosen ("up to X target ...").
    pub fn with_up_to_x_target_count(mut self) -> Self {
        self.up_to_x_target_count = true;
        self
    }

    /// True when the number of selected targets must be at most the spell/ability's chosen X.
    pub fn has_up_to_x_target_count(&self) -> bool {
        self.up_to_x_target_count
    }

    /// Set the number of distinct cards in this body's graveyard target clause.
    pub fn with_graveyard_target_bounds(mut self, minimum: usize, maximum: usize) -> Self {
        self.graveyard_target_bounds = Some((minimum, maximum));
        self
    }

    pub fn with_triggered_modes(modes: TriggeredModes) -> Self {
        let mut body = Self::new(BodyTarget::None, Vec::new());
        body.triggered_modes = Some(modes);
        body
    }

    pub fn triggered_modes(&self) -> Option<&TriggeredModes> {
        self.triggered_modes.as_ref()
    }

    /// Mutable access to triggered-ability modes for lowering rewrites.
    pub fn triggered_modes_mut(&mut self) -> Option<&mut TriggeredModes> {
        self.triggered_modes.as_mut()
    }

    /// Check every scalar and operand against the explicit whole-body contract.
    ///
    /// # Errors
    /// Returns a definition error for invalid instructions, operands, resolving
    /// player contexts, costs, forward destinations or local-slot assignments.
    /// Trigger-dependent amounts and related players require a trigger binding.
    pub fn validate(&self, vocabulary: &CreatureTypeVocabulary) -> Result<(), DefinitionError> {
        self.validate_in(vocabulary, BodyContext::Unbound)
    }

    pub(crate) fn validate_in(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        context: BodyContext<'_>,
    ) -> Result<(), DefinitionError> {
        self.validate_in_with_x(vocabulary, context, false)
    }

    pub(crate) fn validate_in_with_x(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        context: BodyContext<'_>,
        initial_defined_x: bool,
    ) -> Result<(), DefinitionError> {
        self.validate_in_with_facts(
            vocabulary,
            context,
            FlowFacts {
                defined_resolution_x: initial_defined_x,
                ..FlowFacts::default()
            },
        )
    }

    fn validate_in_with_facts(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        context: BodyContext<'_>,
        initial_facts: FlowFacts,
    ) -> Result<(), DefinitionError> {
        if self.up_to_one_target {
            let valid_target = matches!(
                self.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            );
            if !valid_target
                || self.graveyard_target_bounds.is_some()
                || self
                    .permanent_target_bounds
                    .is_some_and(|bounds| bounds != (0, 1))
                || self.x_target_count
                || self.up_to_x_target_count
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        if let Some((minimum, maximum)) = self.permanent_target_bounds {
            let valid_target = matches!(
                self.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            );
            if minimum > maximum
                || maximum == 0
                || !valid_target
                || self.graveyard_target_bounds.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        if self.x_target_count || self.up_to_x_target_count {
            let valid_bounds = self
                .permanent_target_bounds()
                .is_some_and(|(min, max)| min == 0 && max >= usize::from(u16::MAX));
            if !valid_bounds
                || !matches!(
                    context,
                    BodyContext::Unbound
                        | BodyContext::Activated { .. }
                        | BodyContext::SacrificedCreature
                        | BodyContext::SacrificedPermanent
                )
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        if let Some((minimum, maximum)) = self.graveyard_target_bounds {
            let has_multi_gy_move = self.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ZoneOp(spec)
                        if matches!(spec.source, crate::toolbox::ObjectQuery::Targets)
                            && matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Battlefield { .. }
                                    | crate::toolbox::ZoneDest::Hand
                                    | crate::toolbox::ZoneDest::Exile { .. }
                                    | crate::toolbox::ZoneDest::Library { .. }
                            )
                )
            });
            let valid_gy_target_and_ops = matches!(
                self.target,
                BodyTarget::Object {
                    zone: TargetZone::OwnGraveyard
                        | TargetZone::AnyGraveyard { .. }
                        | TargetZone::Exile {
                            owned_flashback_only: false
                        },
                    ..
                }
            ) && has_multi_gy_move;
            if minimum > maximum
                || maximum == 0
                || self.non_graveyard_target_bounds().is_some()
                || !valid_gy_target_and_ops
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        if let Some(modes) = &self.triggered_modes {
            if !matches!(context, BodyContext::TriggerBound { .. })
                || self.target != BodyTarget::None
                || !self.instructions.is_empty()
                || modes.options.len() < 2
                || modes.options.len() > usize::from(u8::MAX)
                || (modes.distinct_targets && modes.minimum > 1)
                || modes.maximum < modes.minimum
                || (!modes.allow_repeat && usize::from(modes.maximum) > modes.options.len())
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let Some(prefix) = &modes.prefix {
                if prefix.target != BodyTarget::None
                    || prefix.triggered_modes.is_some()
                    || prefix
                        .instructions
                        .iter()
                        .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                prefix.validate_in(vocabulary, context)?;
            }
            if let Some(suffix) = &modes.suffix {
                if suffix.target != BodyTarget::None
                    || suffix.triggered_modes.is_some()
                    || suffix
                        .instructions
                        .iter()
                        .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                suffix.validate_in(vocabulary, context)?;
            }
            for option in &modes.options {
                if option.triggered_modes.is_some()
                    || option.instructions.is_empty()
                    || option.ordered_any_target_bounds().is_some()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                option.validate_in(vocabulary, context)?;
            }
            return Ok(());
        }
        if let BodyTarget::Object { filter, .. } = self.target {
            filter.validate_vocabulary(vocabulary)?;
        }
        if let BodyTarget::Object {
            zone: TargetZone::Battlefield,
            filter,
        } = self.target
        {
            if filter.has_bound_exclusion() {
                return Err(DefinitionError::InvalidInstruction);
            }
            if filter.is_creature_or_other_planeswalker() {
                if !context.has_source() {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if filter.has_unbound_trigger_exclusion()
                && !matches!(context, BodyContext::TriggerBound { .. })
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if filter.requires_same_name_as_source()
                && !matches!(context, BodyContext::Activated { .. })
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if (filter.has_related_player()
                && !matches!(
                    context,
                    BodyContext::TriggerBound {
                        condition,
                        grouping: TriggerGrouping::EachOccurrence
                            | TriggerGrouping::OneOrMoreByPlayer
                            | TriggerGrouping::OneOrMoreOnceEachTurn,
                    } if condition.captures_related_player()
                ))
                || filter.has_concrete_player()
            {
                return Err(DefinitionError::UnguaranteedRelatedPlayer);
            }
        }
        if self.instructions.is_empty() {
            return Err(DefinitionError::EmptyBody);
        }
        let mut consuming = false;
        let length = self.instructions.len();
        let points = length
            .checked_add(1)
            .ok_or(DefinitionError::InvalidControlFlow)?;
        let mut incoming = vec![None; points];
        incoming[0] = Some(initial_facts);
        // Unreachable instructions still validate scalar/context/operand syntax.
        // Their local reads do not contribute facts or make a target reachable.
        let declared_slots: BTreeSet<_> = self
            .instructions
            .iter()
            .filter_map(|op| {
                if let Instruction::PromptChoice(spec) = op
                    && let crate::toolbox::PromptDomain::Subtype {
                        legacy_slot: Some(slot),
                        ..
                    } = &spec.domain
                {
                    Some(*slot)
                } else {
                    None
                }
            })
            .collect();
        let permanent_target = matches!(
            self.target,
            BodyTarget::Object {
                zone: TargetZone::Battlefield,
                ..
            }
        );
        let player_target = matches!(self.target, BodyTarget::Player { .. });
        let merge_family = |a: Option<OperandFamily>,
                            b: Option<OperandFamily>|
         -> Result<Option<OperandFamily>, DefinitionError> {
            match (a, b) {
                (Some(OperandFamily::DamageRecipient), Some(OperandFamily::Permanent))
                | (Some(OperandFamily::Permanent), Some(OperandFamily::DamageRecipient))
                    if permanent_target =>
                {
                    Ok(Some(OperandFamily::Permanent))
                }
                (Some(OperandFamily::DamageRecipient), Some(OperandFamily::Player))
                | (Some(OperandFamily::Player), Some(OperandFamily::DamageRecipient))
                    if player_target =>
                {
                    Ok(Some(OperandFamily::Player))
                }
                (Some(x), Some(y)) if x != y => Err(DefinitionError::OperandFamily),
                (Some(x), _) | (_, Some(x)) => Ok(Some(x)),
                (None, None) => Ok(None),
            }
        };
        for (index, instruction) in self.instructions.iter().enumerate() {
            let reachable = incoming[index].is_some();
            let mut facts = incoming[index].take().unwrap_or_else(|| FlowFacts {
                definitely: declared_slots.clone(),
                ..FlowFacts::default()
            });
            facts.exact_creature_target = (self.target.guarantees_creature()
                || matches!(
                    self.target,
                    BodyTarget::Object {
                        zone: TargetZone::OwnGraveyard | TargetZone::AnyGraveyard { .. },
                        filter,
                    } if filter.guarantees_creature()
                )
                || matches!(
                    self.target,
                    BodyTarget::Spell {
                        filter,
                        include_abilities: false,
                    } if filter.object.guarantees_creature()
                ))
                && self
                    .permanent_target_bounds()
                    .is_none_or(|bounds| bounds == (1, 1) || bounds == (0, 1));
            facts.has_object_or_spell_target =
                !matches!(self.target, BodyTarget::None | BodyTarget::Player { .. });
            let mut defined_slots = facts.definitely.clone();
            let mut donor_moved = facts.donor_moved;
            if let Instruction::PromptChoice(spec) = instruction
                && let crate::toolbox::PromptDomain::Subtype {
                    legacy_slot: Some(into),
                    ..
                } = &spec.domain
            {
                if reachable && facts.possibly.contains(into) {
                    return Err(DefinitionError::DuplicateCreatureTypeSlot);
                }
                // Unreachable code has no incoming assignment history.
                if !reachable {
                    defined_slots.remove(into);
                }
            }
            let exclusion = match instruction {
                Instruction::ApplyContinuous(spec) => match &spec.selector {
                    Some(crate::toolbox::ObjectQuery::EffectSubject(group)) => {
                        group.chosen_type_slot()
                    }
                    _ => None,
                },
                _ => None,
            };
            if exclusion.is_some_and(|slot| !defined_slots.contains(&slot)) {
                return Err(DefinitionError::UndefinedCreatureTypeSlot);
            }
            let family = match instruction {
                Instruction::ZoneOp(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    if let crate::toolbox::ZoneDest::Delayed(action) = &spec.destination {
                        match action {
                            crate::toolbox::DelayedZoneAction::PutHandPermanentsOntoBattlefield {
                                player,
                                types,
                                maximum,
                                ..
                            } => {
                                if *maximum == 0
                                    || types.is_empty()
                                    || types.iter().any(|kind| !kind.is_permanent())
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                player.validate(context, self.target, facts.definitely_exiled_target)?
                            }
                            crate::toolbox::DelayedZoneAction::ReturnChosenXGraveyardCreatures => {
                                if self.target
                                    == BodyTarget::filtered_own_graveyard(ObjectFilter::creature())
                                    && self.instructions.len() == 1
                                {
                                    Some(OperandFamily::GraveyardCard)
                                } else {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                            }
                            crate::toolbox::DelayedZoneAction::ReturnExiledTargetCohort { .. } => {
                                let is_cohort = |op: &Instruction, next_end_step_false: bool| {
                                    matches!(
                                        op,
                                        Instruction::ZoneOp(s)
                                            if matches!(
                                                s.destination,
                                                crate::toolbox::ZoneDest::Delayed(
                                                    crate::toolbox::DelayedZoneAction::ReturnExiledTargetCohort {
                                                        next_end_step,
                                                        ..
                                                    }
                                                ) if !next_end_step_false || !next_end_step
                                            )
                                    )
                                };
                                let is_target_exile = |op: &Instruction| {
                                    matches!(
                                        op,
                                        Instruction::ZoneOp(s)
                                            if (matches!(
                                                s.source,
                                                crate::toolbox::ObjectQuery::Targets
                                            ) || matches!(
                                                &s.source,
                                                crate::toolbox::ObjectQuery::EffectSubject(subj)
                                                    if matches!(
                                                        subj.as_ref(),
                                                        EffectSubject::TargetObject
                                                    )
                                            )) && matches!(
                                                s.destination,
                                                crate::toolbox::ZoneDest::Exile { .. }
                                            )
                                    )
                                };
                                let optional_return = index == 2
                                    && matches!(
                                        self.instructions.as_slice(),
                                        [
                                            Instruction::OptionalSequence {
                                                chooser: ResolvingPlayer::Controller,
                                                instructions: 2
                                            },
                                            second,
                                            third,
                                        ] if is_target_exile(second) && is_cohort(third, false)
                                    );
                                let returning_source = index == 1
                                    && self.target == BodyTarget::None
                                    && matches!(
                                        self.instructions.as_slice(),
                                        [Instruction::ZoneOp(_), second] if is_cohort(second, true)
                                    );
                                let direct_return = index == 1
                                    && matches!(
                                        self.instructions.as_slice(),
                                        [first, second, ..] if is_target_exile(first) && is_cohort(second, false)
                                    );
                                if !optional_return && !returning_source && !direct_return {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            crate::toolbox::DelayedZoneAction::ReturnGraveyardToHand(_)
                            | crate::toolbox::DelayedZoneAction::ReturnPermanentToHand(_)
                            | crate::toolbox::DelayedZoneAction::ReturnGraveyardCard(_)
                            | crate::toolbox::DelayedZoneAction::ExileObjects(_)
                            | crate::toolbox::DelayedZoneAction::SacrificeObjects(_)
                            | crate::toolbox::DelayedZoneAction::ReturnExile(_)
                            | crate::toolbox::DelayedZoneAction::ReturnExileWithCounters(_)
                            | crate::toolbox::DelayedZoneAction::ReturnGraveyardCreatureWithCorpse(_) => {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                        }
                    } else {
                        if matches!(spec.source, crate::toolbox::ObjectQuery::DestroyedThisWay)
                            && !index
                                .checked_sub(1)
                                .and_then(|previous| self.instructions.get(previous))
                                .is_some_and(|op| {
                                    matches!(
                                        op,
                                        Instruction::ZoneOp(prev)
                                            if matches!(
                                                prev.destination,
                                                crate::toolbox::ZoneDest::Graveyard {
                                                    semantics: crate::toolbox::GraveyardSemantics::Destroy { .. }
                                                }
                                            )
                                    )
                                })
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if let crate::toolbox::ObjectQuery::ZoneScan {
                            slice:
                                crate::toolbox::QuerySlice::Top(expr)
                                | crate::toolbox::QuerySlice::Bottom(expr)
                                | crate::toolbox::QuerySlice::UntilMatch { count: expr, .. },
                            ..
                        } = &spec.source
                        {
                            expr.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                        }
                        match &spec.selection {
                            crate::toolbox::ZoneSelection::Prompt { min, max, .. } => {
                                min.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                                if max != min {
                                    max.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                }
                            }
                            crate::toolbox::ZoneSelection::Random { count, .. } => {
                                count.validate_amount(
                                    &defined_slots,
                                    context,
                                    &facts,
                                    vocabulary,
                                )?;
                            }
                            crate::toolbox::ZoneSelection::All => {}
                        }
                        if let Some((_, _, primary, _)) = spec.as_group_sacrifice()
                            && let ObjectFilter::Clause(c) = primary
                            && ((c.required_types.bits() & c.forbidden_types.bits()) != 0
                                || (!c.any_types.is_empty()
                                    && (c.any_types.bits() & !c.forbidden_types.bits()) == 0))
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        let family = validate_toolbox_zone_op(
                            spec,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?;
                        let targets_source =
                            matches!(&spec.source, crate::toolbox::ObjectQuery::Targets)
                                || matches!(
                                    &spec.source,
                                    crate::toolbox::ObjectQuery::EffectSubject(s)
                                        if matches!(s.as_ref(), EffectSubject::TargetObject)
                                );
                        if targets_source {
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Graveyard {
                                    semantics: crate::toolbox::GraveyardSemantics::Destroy { .. }
                                }
                            ) {
                                facts.checked_destruction_target = true;
                                facts.maybe_checked_destruction_target = true;
                            }
                            if matches!(spec.destination, crate::toolbox::ZoneDest::Exile { .. })
                                && !donor_moved
                            {
                                facts.definitely_exiled_target = true;
                                facts.maybe_exiled_target = true;
                            }
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Battlefield { .. }
                                    | crate::toolbox::ZoneDest::Hand
                            ) && matches!(
                                toolbox_object_target_family(self.target),
                                Ok(OperandFamily::GraveyardCard)
                            ) {
                                facts.returned_graveyard_target = true;
                            }
                            donor_moved = true;
                        }
                        if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Destroy { .. }
                            }
                        ) {
                            facts.definitely_destroyed = true;
                            facts.maybe_destroyed = true;
                        }
                        if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Mill
                            }
                        ) {
                            facts.definitely_milled = true;
                        }
                        if let Some((players, amount, _, _)) = spec.as_group_sacrifice() {
                            let single_sac = matches!(players, AffectedPlayers::One(_))
                                && matches!(amount, ValueExpr::Literal(value) if *value == 1.into());
                            facts.definitely_sacrificed |= single_sac;
                            if !matches!(amount, ValueExpr::Literal(value) if *value == 0.into()) {
                                facts.maybe_sacrificed = true;
                            }
                            // A generic selection can move the later copy donor. Until
                            // copiable LKI exists, preserve the existing conservative
                            // CFG boundary. Exact zero adds no possible departure but
                            // must never clear an incoming true fact.
                            donor_moved |=
                                !matches!(amount, ValueExpr::Literal(value) if *value == 0.into());
                        } else if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Sacrifice
                            }
                        ) {
                            facts.definitely_sacrificed = true;
                            facts.maybe_sacrificed = true;
                        }
                        if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Discard
                            }
                        ) {
                            facts.definitely_discarded = true;
                            facts.maybe_discarded = true;
                        }
                        if matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Mill
                            }
                        ) {
                            facts.definitely_milled = true;
                            facts.definitely_milled_top = true;
                            facts.maybe_milled_top = true;
                        }
                        if matches!(spec.destination, crate::toolbox::ZoneDest::Exile { .. })
                            && toolbox_query_scans_graveyard(&spec.source)
                        {
                            facts.definitely_selected_graveyard_exile = true;
                            facts.maybe_selected_graveyard_exile = true;
                        }
                        if toolbox_query_scans_library(&spec.source) {
                            facts.definitely_checked_top = true;
                            facts.maybe_checked_top = true;
                            facts.moved_library_cards = true;
                            facts.maybe_moved_library_cards = true;
                        }
                        family
                    }
                }
                Instruction::DrawCards(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let f1 = validate_toolbox_player_selector(
                        &spec.players,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    spec.amount
                        .validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    toolbox_merge_family(f1, spec.amount.required_operand_family(self.target)?)?
                }
                Instruction::AdjustLife(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let mut fam = validate_toolbox_player_selector(
                        &spec.players,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    let op_fam = match &spec.op {
                        crate::toolbox::LifeOp::Gain(e)
                        | crate::toolbox::LifeOp::Lose(e)
                        | crate::toolbox::LifeOp::Set(e)
                        | crate::toolbox::LifeOp::Pay(e) => {
                            e.required_operand_family(self.target)?
                        }
                        crate::toolbox::LifeOp::Drain {
                            amount, gain_to, ..
                        } => toolbox_merge_family(
                            amount.required_operand_family(self.target)?,
                            validate_toolbox_player_selector(
                                gain_to,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?,
                        )?,
                        crate::toolbox::LifeOp::LoseHalf { .. } => None,
                        crate::toolbox::LifeOp::ExchangeTotals { other } => {
                            validate_toolbox_player_selector(
                                other,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::LifeOp::ExchangeWithStat { object, .. } => {
                            match object.to_move_subject() {
                                Some(MoveSubjectRef::TargetObject) => {
                                    if !matches!(
                                        self.target,
                                        BodyTarget::Object {
                                            zone: TargetZone::Battlefield,
                                            ..
                                        }
                                    ) {
                                        return Err(DefinitionError::OperandFamily);
                                    }
                                    Some(OperandFamily::Permanent)
                                }
                                Some(
                                    MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost,
                                ) => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TriggeringObject) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(_) => None,
                                None => validate_toolbox_object_query(
                                    object,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            }
                        }
                    };
                    fam = toolbox_merge_family(fam, op_fam)?;
                    fam
                }
                Instruction::DealDamage(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let mut fam = match &spec.source {
                        crate::toolbox::DamageSourceQuery::EntrySource => None,
                        crate::toolbox::DamageSourceQuery::Objects(q) => {
                            validate_toolbox_object_query(
                                q,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                    };
                    let rec_fam = match &spec.recipients {
                        crate::toolbox::DamageTargetQuery::Targets => Some(match self.target {
                            BodyTarget::Player { .. } => OperandFamily::Player,
                            _ => toolbox_object_target_family(self.target)?,
                        }),
                        crate::toolbox::DamageTargetQuery::Objects(q) => {
                            validate_toolbox_object_query(
                                q,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::DamageTargetQuery::Players(p) => {
                            validate_toolbox_player_selector(
                                p,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::DamageTargetQuery::Both { objects, players } => {
                            toolbox_merge_family(
                                validate_toolbox_object_query(
                                    objects,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                                validate_toolbox_player_selector(
                                    players,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?
                        }
                        crate::toolbox::DamageTargetQuery::AnnouncedTargets {
                            min_targets,
                            max_targets,
                        } => {
                            if *max_targets == 0 || min_targets > max_targets {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            match self.target {
                                BodyTarget::AnyTarget(
                                    AnyTargetKind::Any
                                    | AnyTargetKind::CreatureOrPlayer
                                    | AnyTargetKind::PlayerOrPlaneswalker
                                    | AnyTargetKind::OpponentOrPlaneswalker,
                                )
                                | BodyTarget::Player { .. }
                                | BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                } => Some(OperandFamily::DamageRecipient),
                                _ => return Err(DefinitionError::OperandFamily),
                            }
                        }
                        crate::toolbox::DamageTargetQuery::FromPower(source_kind) => {
                            match source_kind {
                                PowerDamageSource::AbilitySource => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    Some(OperandFamily::DamageRecipient)
                                }
                                PowerDamageSource::CreatureDealtDamage
                                | PowerDamageSource::GraveyardEntrant => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    Some(OperandFamily::DamageRecipient)
                                }
                                PowerDamageSource::TargetCreature => match self.target {
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } => Some(OperandFamily::Permanent),
                                    _ => Some(OperandFamily::DamageRecipient),
                                },
                                PowerDamageSource::TargetCreatureToAbilitySource => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    match self.target {
                                        BodyTarget::Object {
                                            zone: TargetZone::Battlefield,
                                            ..
                                        } => Some(OperandFamily::Permanent),
                                        _ => return Err(DefinitionError::OperandFamily),
                                    }
                                }
                            }
                        }
                        crate::toolbox::DamageTargetQuery::CreaturePowerToOther { .. } => {
                            let [
                                Instruction::FixedTargetClause(dealer),
                                Instruction::FixedTargetClause(recipient),
                                Instruction::DealDamage(_),
                                ..,
                            ] = self.instructions.as_slice()
                            else {
                                return Err(DefinitionError::InvalidInstruction);
                            };
                            let dealer_ok = dealer.target.guarantees_creature();
                            let recipient_ok = matches!(
                                &recipient.target,
                                BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                } | BodyTarget::Player { .. }
                                    | BodyTarget::AnyTarget(
                                        AnyTargetKind::Any
                                            | AnyTargetKind::CreatureOrPlayer
                                            | AnyTargetKind::PlayerOrPlaneswalker
                                            | AnyTargetKind::OpponentOrPlaneswalker,
                                    )
                            );
                            if self.target != BodyTarget::None || !dealer_ok || !recipient_ok {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::DamageTargetQuery::TargetControllersCreatures {
                            ..
                        } => {
                            if !matches!(
                                self.target,
                                BodyTarget::Player { .. }
                                    | BodyTarget::AnyTarget(
                                        AnyTargetKind::PlayerOrPlaneswalker
                                            | AnyTargetKind::OpponentOrPlaneswalker,
                                    )
                            ) {
                                return Err(DefinitionError::OperandFamily);
                            }
                            Some(OperandFamily::DamageRecipient)
                        }
                        crate::toolbox::DamageTargetQuery::OrderedTargets { first, others } => {
                            let dependent = self.target
                                == BodyTarget::OPPONENT_OR_PLANESWALKER_THEN_CREATURE
                                && matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        grouping: TriggerGrouping::EachOccurrence,
                                        ..
                                    }
                                );
                            if !(dependent || self.target == BodyTarget::SINGLE_ANY_TARGET)
                                || self.instructions.len() != 1
                                || !first.is_positive()
                                || !others.is_positive()
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::DamageRecipient)
                        }
                        crate::toolbox::DamageTargetQuery::FromGrantingPermanent => {
                            let valid_amt = matches!(
                                &spec.amount,
                                DamageAmountSpec::Expr(ValueExpr::Literal(amt)) if amt.is_positive()
                            );
                            if !valid_amt
                                || self.target != BodyTarget::SINGLE_ANY_TARGET
                                || self.instructions.len() != 1
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::DamageRecipient)
                        }
                        crate::toolbox::DamageTargetQuery::DivideThree => {
                            if self.target != BodyTarget::SINGLE_ANY_TARGET
                                || self.instructions.len() != 1
                                || !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        grouping: TriggerGrouping::EachOccurrence,
                                        ..
                                    }
                                )
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::DamageRecipient)
                        }
                        crate::toolbox::DamageTargetQuery::Fight { first, second } => {
                            let mut fight_fam = None;
                            for query in [first.as_ref(), second.as_ref()] {
                                let qf = match query.to_move_subject() {
                                    Some(MoveSubjectRef::TargetObject) => {
                                        if !matches!(
                                            self.target,
                                            BodyTarget::Object {
                                                zone: TargetZone::Battlefield,
                                                ..
                                            }
                                        ) {
                                            return Err(DefinitionError::OperandFamily);
                                        }
                                        Some(OperandFamily::Permanent)
                                    }
                                    Some(
                                        MoveSubjectRef::AbilitySource
                                        | MoveSubjectRef::AttachedHost,
                                    ) => {
                                        if !context.has_source() {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                        None
                                    }
                                    Some(MoveSubjectRef::TriggeringObject) => {
                                        if !matches!(context, BodyContext::TriggerBound { .. }) {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                        None
                                    }
                                    Some(_) => None,
                                    None => validate_toolbox_object_query(
                                        query,
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?,
                                };
                                fight_fam = merge_family(fight_fam, qf)?;
                            }
                            fight_fam
                        }
                        crate::toolbox::DamageTargetQuery::Divided { total, .. } => {
                            total.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            match self.target {
                                BodyTarget::AnyTarget(
                                    AnyTargetKind::Any
                                    | AnyTargetKind::CreatureOrPlayer
                                    | AnyTargetKind::PlayerOrPlaneswalker
                                    | AnyTargetKind::OpponentOrPlaneswalker,
                                )
                                | BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                } => Some(OperandFamily::DamageRecipient),
                                _ => return Err(DefinitionError::OperandFamily),
                            }
                        }
                        crate::toolbox::DamageTargetQuery::Relative(rel) => match rel.as_ref() {
                            DamageRecipientsRef::Target | DamageRecipientsRef::TargetController => {
                                Some(match self.target {
                                    BodyTarget::Player { .. } => OperandFamily::Player,
                                    _ => toolbox_object_target_family(self.target)?,
                                })
                            }
                            DamageRecipientsRef::Group(g) => match g {
                                DamageGroup::ControllerAndTargetCreature => {
                                    Some(OperandFamily::Permanent)
                                }
                                DamageGroup::Filtered(f) => {
                                    if f.uses_target_permanent() {
                                        Some(OperandFamily::Permanent)
                                    } else if f.has_target_player() {
                                        Some(OperandFamily::Player)
                                    } else {
                                        None
                                    }
                                }
                                DamageGroup::FilteredAndPlayers { filter, players } => {
                                    let f_fam = if filter.uses_target_permanent() {
                                        Some(OperandFamily::Permanent)
                                    } else if filter.has_target_player() {
                                        Some(OperandFamily::Player)
                                    } else {
                                        None
                                    };
                                    let p_fam = validate_toolbox_player_selector(
                                        &crate::toolbox::PlayerSelector::Group(*players),
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?;
                                    toolbox_merge_family(f_fam, p_fam)?
                                }
                                DamageGroup::AffectedPlayers(players) => {
                                    validate_toolbox_player_selector(
                                        &crate::toolbox::PlayerSelector::Group(*players),
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?
                                }
                                _ => None,
                            },
                            _ => None,
                        },
                    };
                    fam = toolbox_merge_family(fam, rec_fam)?;
                    if let DamageAmountSpec::Expr(e) = &spec.amount {
                        fam = toolbox_merge_family(fam, e.required_operand_family(self.target)?)?;
                    }
                    fam
                }
                Instruction::MutatePlayer(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let fam = validate_toolbox_player_selector(
                        &spec.players,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    let op_fam = match &spec.op {
                        crate::toolbox::PlayerMutation::Counter { amount, .. } => {
                            amount.required_operand_family(self.target)?
                        }
                        crate::toolbox::PlayerMutation::TurnFlow(
                            crate::toolbox::TurnFlowOp::AdditionalCombat { untap },
                        ) => {
                            let admitted = match untap {
                                CombatUntapGroup::None | CombatUntapGroup::ControlledCreatures => {
                                    self.target == BodyTarget::None
                                }
                                CombatUntapGroup::AttackersIfSourceAttacksHighestLife => matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        condition: TriggerSpec {
                                            event: TriggerEvent::Attacks {
                                                mode: AttackTriggerMode::FirstThisTurn {
                                                    highest_life: true
                                                },
                                                ..
                                            },
                                            ..
                                        },
                                        ..
                                    }
                                ),
                                CombatUntapGroup::SourceAndControlledSubtype(kind) => {
                                    vocabulary.label(*kind).is_some()
                                        && matches!(
                                            context,
                                            BodyContext::TriggerBound {
                                                condition: TriggerSpec {
                                                    event: TriggerEvent::Attacks {
                                                        mode: AttackTriggerMode::FirstThisTurn {
                                                            highest_life: false
                                                        },
                                                        ..
                                                    },
                                                    ..
                                                },
                                                ..
                                            }
                                        )
                                }
                            };
                            if !admitted || self.target != BodyTarget::None {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::SelectExtremeLife { .. } => {
                            facts.chose_player = true;
                            None
                        }
                        crate::toolbox::PlayerMutation::Vote { options, .. } => {
                            if options.len() < 2 || options.len() > usize::from(u8::MAX) {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::Ripple(n),
                        ) => {
                            if *n == 0 {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::Incubate { count },
                        ) => {
                            count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            None
                        }
                        crate::toolbox::PlayerMutation::LibraryTop(
                            crate::toolbox::LibraryTopAction::ManifestTop(amount)
                            | crate::toolbox::LibraryTopAction::Scry(amount)
                            | crate::toolbox::LibraryTopAction::Surveil(amount)
                            | crate::toolbox::LibraryTopAction::Mill(amount),
                        )
                        | crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::Blight { amount }
                            | crate::toolbox::PlayerKeywordAction::CollectEvidence {
                                minimum: amount,
                            },
                        ) => {
                            if matches!(
                                spec.op,
                                crate::toolbox::PlayerMutation::LibraryTop(
                                    crate::toolbox::LibraryTopAction::Mill(_)
                                )
                            ) {
                                facts.definitely_milled_top = true;
                            }
                            if matches!(
                                spec.op,
                                crate::toolbox::PlayerMutation::KeywordAction(
                                    crate::toolbox::PlayerKeywordAction::CollectEvidence { .. }
                                )
                            ) {
                                facts.definitely_selected_graveyard_exile = true;
                            }
                            amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            amount.required_operand_family(self.target)?
                        }
                        crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::Bolster(amount),
                        ) => {
                            if *amount == 0 {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::Amass { amount, .. },
                        ) => {
                            if *amount == 0 {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::AddMana(mana_yield) => {
                            let inner_yield = match mana_yield {
                                EffectManaYield::RetainedUntilCleanup(inner) => {
                                    if matches!(
                                        inner.as_ref(),
                                        EffectManaYield::RetainedUntilCleanup(_)
                                    ) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    inner.as_ref()
                                }
                                other => other,
                            };
                            match inner_yield {
                                EffectManaYield::RetainedUntilCleanup(_) => {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                EffectManaYield::RestrictedChoice {
                                    colors,
                                    amount,
                                    restriction,
                                } => {
                                    if colors.is_empty()
                                        || colors.contains(&crate::mana::ManaType::Colourless)
                                        || !restriction.valid_authored(vocabulary)
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    amount.required_operand_family(self.target)?
                                }
                                EffectManaYield::CombatDamageUntilCleanup(_) => {
                                    ValueExpr::event(EventValueKind::CombatDamageToPlayer)
                                        .validate_amount(
                                            &defined_slots,
                                            context,
                                            &facts,
                                            vocabulary,
                                        )?;
                                    if self.target != BodyTarget::None {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                EffectManaYield::Split { first, second, .. } if first == second => {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                EffectManaYield::RestrictedFixed {
                                    bundle,
                                    restriction,
                                } => {
                                    if bundle.is_empty()
                                        || bundle.values().any(|amt| !amt.is_positive())
                                        || !restriction.valid_authored(vocabulary)
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                EffectManaYield::RestrictedCounted {
                                    amount,
                                    restriction,
                                    ..
                                } => {
                                    if !restriction.valid_authored(vocabulary) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    amount.required_operand_family(self.target)?
                                }
                                EffectManaYield::Counted { amount, .. }
                                | EffectManaYield::RetainedUntilEndOfCombat { amount, .. }
                                | EffectManaYield::Split { amount, .. } => {
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    amount.required_operand_family(self.target)?
                                }
                                EffectManaYield::Choice { colors, amount } => {
                                    if colors.is_empty() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    amount.required_operand_family(self.target)?
                                }
                                EffectManaYield::ChoiceBundle { options } => {
                                    if options.len() < 2
                                        || options.iter().any(|bundle| {
                                            bundle.is_empty()
                                                || bundle.values().any(|a| !a.is_positive())
                                        })
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                EffectManaYield::LandProducible { controller, amount } => {
                                    let f = controller.validate(
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?;
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    f
                                }
                                EffectManaYield::CommanderIdentity { amount }
                                | EffectManaYield::DistinctColors { amount } => {
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    None
                                }
                                EffectManaYield::SourceChosenColor { amount } => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    None
                                }
                                EffectManaYield::Fixed(produced) => {
                                    if produced.is_empty()
                                        || produced.values().any(|a| !a.is_positive())
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                            }
                        }
                        crate::toolbox::PlayerMutation::OfferSourceCounters {
                            amount,
                            placed_destination,
                            declined_destination,
                        } => {
                            if amount < &0.into()
                                || !matches!(context, BodyContext::TriggerBound { .. })
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Self::validate_destination(index, *placed_destination, length)?;
                            Self::validate_destination(index, *declined_destination, length)?;
                            None
                        }
                        crate::toolbox::PlayerMutation::ExileOnePerPlayerUntilSourceLeaves => {
                            if !matches!(
                                self.target,
                                BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                }
                            ) || !matches!(context, BodyContext::TriggerBound { .. })
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::Permanent)
                        }
                        crate::toolbox::PlayerMutation::LibraryTop(
                            crate::toolbox::LibraryTopAction::MayRevealTop,
                        ) => {
                            if self.target != BodyTarget::None {
                                return Err(DefinitionError::OperandFamily);
                            }
                            facts.definitely_checked_top = true;
                            None
                        }
                        crate::toolbox::PlayerMutation::RandomEvent(
                            crate::toolbox::RandomEventOp::RollDie { count, bands, .. },
                        ) => {
                            count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            let mut rf = count.required_operand_family(self.target)?;
                            for band in bands {
                                if band.min > band.max {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                if band.body.validate_in(vocabulary, context).is_err() {
                                    if self.target == BodyTarget::None {
                                        band.body.validate_in(vocabulary, context)?;
                                    } else {
                                        let mut inherited = band.body.clone();
                                        inherited.target = self.target;
                                        inherited.validate_in(vocabulary, context)?;
                                        rf =
                                            merge_family(rf, inherited_target_family(self.target))?;
                                    }
                                }
                            }
                            rf
                        }
                        crate::toolbox::PlayerMutation::RandomEvent(
                            crate::toolbox::RandomEventOp::FlipCoins {
                                mode,
                                win_body,
                                lose_body,
                                lose_destination,
                            },
                        ) => {
                            if let Some(dest) = lose_destination {
                                Self::validate_destination(index, *dest, length)?;
                            }
                            if let CoinFlipMode::Count(v) = mode {
                                v.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            }
                            if let Some(b) = win_body {
                                b.validate_in(vocabulary, context)?;
                            }
                            if let Some(b) = lose_body {
                                b.validate_in(vocabulary, context)?;
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::OfferAnyPlayer {
                            cost_body,
                            paid_body,
                            declined_body,
                        } => {
                            let mut ofam = None;
                            let validate_paid = !(paid_body.instructions.is_empty()
                                && (!cost_body.instructions.is_empty() || declined_body.is_some()));
                            if !validate_paid && paid_body.target != BodyTarget::None {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            for sub in std::iter::once(cost_body.as_ref())
                                .chain(validate_paid.then_some(paid_body.as_ref()))
                                .chain(declined_body.as_deref())
                            {
                                if sub.target != BodyTarget::None {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                if sub.validate_in(vocabulary, context).is_err() {
                                    if self.target == BodyTarget::None {
                                        sub.validate_in(vocabulary, context)?;
                                    } else {
                                        let mut inherited = sub.clone();
                                        inherited.target = self.target;
                                        inherited.validate_in(vocabulary, context)?;
                                        ofam = merge_family(
                                            ofam,
                                            inherited_target_family(self.target),
                                        )?;
                                    }
                                }
                            }
                            ofam
                        }
                        crate::toolbox::PlayerMutation::RevealTopTwoPiles {
                            count,
                            target_opponent,
                            ..
                        } => {
                            count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            facts.moved_library_cards = true;
                            facts.definitely_checked_top = true;
                            let tf = if *target_opponent {
                                if self.target != BodyTarget::SINGLE_OPPONENT {
                                    return Err(DefinitionError::OperandFamily);
                                }
                                Some(OperandFamily::Player)
                            } else {
                                None
                            };
                            let af = count.required_operand_family(self.target)?;
                            merge_family(tf, af)?
                        }
                        crate::toolbox::PlayerMutation::BindResolutionX(expr) => {
                            expr.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            facts.defined_resolution_x = true;
                            expr.required_operand_family(self.target)?
                        }
                        crate::toolbox::PlayerMutation::SacrificeRandomUntilLand => {
                            if self.target != BodyTarget::SINGLE_OPPONENT {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::Player)
                        }
                        crate::toolbox::PlayerMutation::ChooseRevealedHandCard {
                            filter, ..
                        } => {
                            if !matches!(self.target, BodyTarget::Player { .. }) {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            if let Some(f) = filter {
                                f.validate_vocabulary(vocabulary)
                                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                            }
                            Some(OperandFamily::Player)
                        }
                        crate::toolbox::PlayerMutation::RequireCondition(cond) => {
                            match cond {
                                InterveningCondition::ReturnSourceOnBattlefield => {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                InterveningCondition::CastFromGraveyard => {
                                    if !matches!(context, BodyContext::Unbound) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                InterveningCondition::SourceLoyalty => {
                                    if !matches!(
                                        context,
                                        BodyContext::TriggerBound {
                                            condition: TriggerSpec {
                                                event: TriggerEvent::Enters { subject: ObjectFilter::Clause(c), .. },
                                                intervening_if: Some(TriggerCondition::SourceHasLoyalty),
                                                ..
                                            },
                                            grouping: TriggerGrouping::EachOccurrence,
                                        } if !c.any_types.is_empty() && (c.any_types.bits() & !CardTypeMask::PERMANENT.bits()) == 0
                                    ) || self.target != BodyTarget::None
                                        || index != 0
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                InterveningCondition::SourceQuestCounters(minimum)
                                | InterveningCondition::OpponentLifeLoss(minimum) => {
                                    let expected = cond.expected_trigger_condition();
                                    if index != 0
                                        || *minimum == 0
                                        || !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if condition.intervening_if == expected)
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                InterveningCondition::Monarch { .. } => {
                                    let expected = cond.expected_trigger_condition();
                                    if index != 0
                                        || !matches!(context, BodyContext::TriggerBound { condition, .. } if condition.intervening_if == expected)
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                other => {
                                    let expected = other.expected_trigger_condition();
                                    if index != 0
                                        || !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if condition.intervening_if == expected)
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                            }
                            None
                        }
                        crate::toolbox::PlayerMutation::KeywordAction(
                            crate::toolbox::PlayerKeywordAction::ExtortDrain,
                        ) => {
                            let definition = TriggeredAbility::extort(TriggeredAbilityId(0));
                            if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == definition.condition)
                                || *self != definition.body
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        _ => None,
                    };
                    toolbox_merge_family(fam, op_fam)?
                }
                Instruction::MutateObject(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let mut family = match &spec.op {
                        crate::toolbox::ObjectMutation::StateMarker(
                            crate::toolbox::ObjectStateMarkerOp::RememberPermanentTarget,
                        )
                        | crate::toolbox::ObjectMutation::TokenAttach(
                            crate::toolbox::TokenAttachOp::CreateCopy(_)
                            | crate::toolbox::TokenAttachOp::CreateAuraAttached { .. },
                        )
                        | crate::toolbox::ObjectMutation::Connive { .. }
                        | crate::toolbox::ObjectMutation::Control(
                            crate::toolbox::ControlOp::Exchange { .. },
                        )
                        | crate::toolbox::ObjectMutation::Attach(
                            crate::toolbox::AttachOp::ToObject {
                                ability: Some(_), ..
                            },
                        ) => None,
                        _ => validate_toolbox_object_query(
                            &spec.source,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?,
                    };
                    let sel_fam = match &spec.selection {
                        crate::toolbox::ZoneSelection::All => None,
                        crate::toolbox::ZoneSelection::Random { count, chooser } => {
                            toolbox_merge_family(
                                count.required_operand_family(self.target)?,
                                validate_toolbox_player_selector(
                                    chooser,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?
                        }
                        crate::toolbox::ZoneSelection::Prompt {
                            chooser, min, max, ..
                        } => {
                            let f1 = validate_toolbox_player_selector(
                                chooser,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?;
                            let f2 = toolbox_merge_family(
                                f1,
                                min.required_operand_family(self.target)?,
                            )?;
                            toolbox_merge_family(f2, max.required_operand_family(self.target)?)?
                        }
                    };
                    family = toolbox_merge_family(family, sel_fam)?;
                    let op_fam = match &spec.op {
                        crate::toolbox::ObjectMutation::Goad { by, .. } => {
                            validate_toolbox_player_selector(
                                by,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::ObjectMutation::Attach(
                            crate::toolbox::AttachOp::ToObject {
                                ability: Some(ability),
                                ..
                            },
                        ) => {
                            let valid = match context {
                                BodyContext::Unbound
                                | BodyContext::Activated { .. }
                                | BodyContext::SacrificedPermanent
                                | BodyContext::RevealedCreature
                                | BodyContext::ExiledCreatureCost
                                | BodyContext::SacrificedCreature
                                | BodyContext::TapCreatureCost => {
                                    self.target == ability.target()
                                        || (*ability == crate::attachment::AttachAbility::Equip
                                            && matches!(
                                                self.target,
                                                BodyTarget::Object {
                                                    zone: TargetZone::Battlefield,
                                                    ..
                                                }
                                            ))
                                }
                                BodyContext::TriggerBound { .. } => {
                                    *ability == crate::attachment::AttachAbility::Equip
                                        && matches!(
                                            self.target,
                                            BodyTarget::Object {
                                                zone: TargetZone::Battlefield,
                                                ..
                                            }
                                        )
                                }
                            };
                            let single_only = !matches!(context, BodyContext::TriggerBound { .. });
                            if !valid || (single_only && self.instructions.len() != 1) {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::Permanent)
                        }
                        crate::toolbox::ObjectMutation::Attach(
                            crate::toolbox::AttachOp::ToObject {
                                host,
                                ability: None,
                            },
                        ) => validate_toolbox_object_query(
                            host,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?,
                        crate::toolbox::ObjectMutation::Control(
                            crate::toolbox::ControlOp::Exchange { other },
                        ) => {
                            let mut ex_fam = None;
                            for query in [&spec.source, other.as_ref()] {
                                let qf = match query.to_move_subject() {
                                    Some(MoveSubjectRef::TargetObject) => {
                                        if !matches!(
                                            self.target,
                                            BodyTarget::Object {
                                                zone: TargetZone::Battlefield,
                                                ..
                                            }
                                        ) {
                                            return Err(DefinitionError::OperandFamily);
                                        }
                                        Some(OperandFamily::Permanent)
                                    }
                                    Some(
                                        MoveSubjectRef::AbilitySource
                                        | MoveSubjectRef::AttachedHost,
                                    ) => {
                                        if !context.has_source() {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                        None
                                    }
                                    Some(MoveSubjectRef::TriggeringObject) => {
                                        if !matches!(context, BodyContext::TriggerBound { .. }) {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                        None
                                    }
                                    Some(_) => None,
                                    None => validate_toolbox_object_query(
                                        query,
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?,
                                };
                                ex_fam = merge_family(ex_fam, qf)?;
                            }
                            ex_fam
                        }
                        crate::toolbox::ObjectMutation::Attach(
                            crate::toolbox::AttachOp::ToPlayer { player },
                        ) => validate_toolbox_player_selector(
                            player,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?,
                        crate::toolbox::ObjectMutation::Counters(
                            crate::toolbox::CounterOp::Add { amount, .. }
                            | crate::toolbox::CounterOp::Remove { amount, .. },
                        ) => amount.required_operand_family(self.target)?,
                        crate::toolbox::ObjectMutation::StateMarker(
                            crate::toolbox::ObjectStateMarkerOp::BecomeMonstrous { amount },
                        ) => {
                            if !context.has_source() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            amount.required_operand_family(self.target)?
                        }
                        crate::toolbox::ObjectMutation::Earthbend { amount } => {
                            amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            amount.required_operand_family(self.target)?
                        }
                        crate::toolbox::ObjectMutation::Combat(
                            crate::toolbox::ObjectCombatOp::Enlist,
                        ) => {
                            if !context.has_source() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::Counters(
                            crate::toolbox::CounterOp::Distribute { kind, total },
                        ) => {
                            if *total == 0 || kind.is_player_counter() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            match self.target {
                                BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                } => Some(OperandFamily::Permanent),
                                _ => return Err(DefinitionError::OperandFamily),
                            }
                        }
                        crate::toolbox::ObjectMutation::Combat(
                            crate::toolbox::ObjectCombatOp::ReselectAttackDefender,
                        ) => {
                            if self.target
                                != BodyTarget::filtered_permanent(
                                    ObjectFilter::creature().attacking(),
                                )
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::Permanent)
                        }
                        crate::toolbox::ObjectMutation::Combat(
                            crate::toolbox::ObjectCombatOp::SoulbondPair { entrant_only },
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(context, BodyContext::TriggerBound {
                                    condition: TriggerSpec {
                                        event: TriggerEvent::Keyword(
                                            TriggerKeywordEvent::Soulbond { entrant_only: only }
                                        ),
                                        ..
                                    },
                                    grouping: TriggerGrouping::EachOccurrence,
                                } if *only == *entrant_only)
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::Counters(
                            crate::toolbox::CounterOp::Move { to, amount, .. },
                        ) => {
                            let f1 = validate_toolbox_object_query(
                                to,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?;
                            let f2 = match amount {
                                Some(expr) => expr.required_operand_family(self.target)?,
                                None => None,
                            };
                            toolbox_merge_family(f1, f2)?
                        }
                        crate::toolbox::ObjectMutation::Control(
                            crate::toolbox::ControlOp::Gain { controller, .. },
                        ) => validate_toolbox_player_selector(
                            controller,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?,
                        crate::toolbox::ObjectMutation::TokenAttach(
                            crate::toolbox::TokenAttachOp::CreateAndAttach { definition },
                        ) => {
                            if !matches!(
                                context,
                                BodyContext::TriggerBound {
                                    condition,
                                    grouping: TriggerGrouping::EachOccurrence
                                } if *condition == TriggerSpec::self_enters()
                            ) || self.target != BodyTarget::None
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            definition
                                .validate(vocabulary)
                                .map_err(|_| DefinitionError::TokenDefinition)?;
                            None
                        }
                        crate::toolbox::ObjectMutation::StateMarker(
                            crate::toolbox::ObjectStateMarkerOp::RememberPermanentTarget,
                        ) => {
                            if index != 0 {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            facts.remembered_permanent = true;
                            Some(
                                if matches!(
                                    self.target,
                                    BodyTarget::Player { .. }
                                        | BodyTarget::AnyTarget(
                                            AnyTargetKind::Any
                                                | AnyTargetKind::CreatureOrPlayer
                                                | AnyTargetKind::PlayerOrPlaneswalker
                                                | AnyTargetKind::OpponentOrPlaneswalker,
                                        )
                                ) {
                                    OperandFamily::DamageRecipient
                                } else {
                                    OperandFamily::Permanent
                                },
                            )
                        }
                        crate::toolbox::ObjectMutation::Connive { count } => {
                            if let Some(count) = count {
                                count.validate_amount(
                                    &defined_slots,
                                    context,
                                    &facts,
                                    vocabulary,
                                )?;
                            }
                            match spec.source.to_move_subject() {
                                Some(
                                    MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost,
                                ) => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TriggeringObject) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TargetObject) => match self.target {
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } => Some(OperandFamily::Permanent),
                                    _ => return Err(DefinitionError::OperandFamily),
                                },
                                Some(MoveSubjectRef::RevealedCard) | None => {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                Some(
                                    MoveSubjectRef::CreatedTokens
                                    | MoveSubjectRef::LastMoved
                                    | MoveSubjectRef::RememberedFirst
                                    | MoveSubjectRef::RememberedSecond,
                                ) => None,
                            }
                        }
                        crate::toolbox::ObjectMutation::TokenAttach(
                            crate::toolbox::TokenAttachOp::CreateCopy(copy),
                        ) => {
                            copy.count.validate_amount(
                                &defined_slots,
                                context,
                                &facts,
                                vocabulary,
                            )?;
                            let mut fam = match spec.source.to_move_subject() {
                                Some(MoveSubjectRef::TargetObject) => match self.target {
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } => Some(OperandFamily::Permanent),
                                    BodyTarget::Object {
                                        zone:
                                            TargetZone::OwnGraveyard
                                            | TargetZone::AnyGraveyard { .. }
                                            | TargetZone::RecentlyDiscardedOrFromLibrary,
                                        ..
                                    } => Some(OperandFamily::GraveyardCard),
                                    BodyTarget::Object {
                                        zone: TargetZone::Exile { .. },
                                        ..
                                    } => Some(OperandFamily::ExiledCard),
                                    _ => return Err(DefinitionError::OperandFamily),
                                },
                                Some(
                                    MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost,
                                ) => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TriggeringObject) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(
                                    MoveSubjectRef::RevealedCard
                                    | MoveSubjectRef::CreatedTokens
                                    | MoveSubjectRef::LastMoved
                                    | MoveSubjectRef::RememberedFirst
                                    | MoveSubjectRef::RememberedSecond,
                                ) => None,
                                None => validate_toolbox_object_query(
                                    &spec.source,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            };
                            if let crate::token::TokenRecipients::Player(p) = &copy.recipients {
                                fam = merge_family(
                                    fam,
                                    p.validate(
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?,
                                )?;
                            }
                            if let crate::token::AttackingDefenderScope::Player(p)
                            | crate::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                                p,
                            ) = &copy.defender_scope
                            {
                                fam = merge_family(
                                    fam,
                                    p.validate(
                                        context,
                                        self.target,
                                        facts.definitely_exiled_target,
                                    )?,
                                )?;
                            }
                            merge_family(fam, copy.count.required_operand_family(self.target)?)?
                        }
                        crate::toolbox::ObjectMutation::TokenAttach(
                            crate::toolbox::TokenAttachOp::CreateAuraAttached {
                                definition, ..
                            },
                        ) => {
                            definition
                                .validate(vocabulary)
                                .map_err(|_| DefinitionError::InvalidInstruction)?;
                            match spec.source.to_move_subject() {
                                Some(MoveSubjectRef::TargetObject) => match self.target {
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } => Some(OperandFamily::Permanent),
                                    _ if facts.returned_graveyard_target => {
                                        Some(OperandFamily::GraveyardCard)
                                    }
                                    _ => return Err(DefinitionError::OperandFamily),
                                },
                                Some(
                                    MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost,
                                ) => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TriggeringObject) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::RevealedCard) | None => {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                Some(
                                    MoveSubjectRef::CreatedTokens
                                    | MoveSubjectRef::LastMoved
                                    | MoveSubjectRef::RememberedFirst
                                    | MoveSubjectRef::RememberedSecond,
                                ) => None,
                            }
                        }
                        crate::toolbox::ObjectMutation::KeywordZoneAction(
                            crate::toolbox::KeywordZoneOp::ExileDefeatedSiege,
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        condition: TriggerSpec {
                                            event: TriggerEvent::CountersChanged {
                                                kind: CounterTriggerKind::LastDefenseRemoved,
                                                ..
                                            },
                                            ..
                                        },
                                        grouping: TriggerGrouping::EachOccurrence,
                                    }
                                )
                                || self.instructions
                                    != [
                                        Instruction::MutateObject(Box::new(
                                            crate::toolbox::MutateObjectSpec::exile_defeated_siege(
                                            ),
                                        )),
                                        Instruction::StackOp(Box::new(
                                            crate::toolbox::StackOpSpec::cast_defeated_siege(),
                                        )),
                                    ]
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::ResolveKeywordTrigger(keyword) => {
                            if matches!(keyword, CombatTriggerKeyword::Mobilize(_))
                                && vocabulary.resolve_label("Warrior").is_none()
                            {
                                return Err(DefinitionError::TokenDefinition);
                            }
                            if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == keyword.condition())
                                || !keyword.valid()
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::Ninjutsu
                        | crate::toolbox::ObjectMutation::Unearth => {
                            if self.target != BodyTarget::None {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::Myriad => {
                            if self.target == BodyTarget::None
                                && matches!(context, BodyContext::TriggerBound { condition: TriggerSpec { event: TriggerEvent::Attacks { subject, mode: AttackTriggerMode::Individual { capture_defender: true } }, .. }, grouping: TriggerGrouping::EachOccurrence } if *subject == ObjectFilter::self_object())
                            {
                                None
                            } else {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                        }
                        crate::toolbox::ObjectMutation::ResolveTransfer(keyword) => {
                            if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == keyword.condition())
                                || self.target != keyword.target()
                                || self.instructions.as_slice()
                                    != keyword.ability(TriggeredAbilityId(0)).body.instructions
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            match keyword {
                                TransferKeyword::Modular(_) => Some(OperandFamily::Permanent),
                                TransferKeyword::Soulshift(_) => Some(OperandFamily::GraveyardCard),
                                TransferKeyword::Graft(_) => None,
                            }
                        }
                        crate::toolbox::ObjectMutation::ResolveGrowth(keyword) => {
                            if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == keyword.condition())
                                || self.target != keyword.target()
                                || self.instructions.len() != 1
                                || matches!(keyword, GrowthKeyword::Backup { amount, .. } if amount < &0.into())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            (self.target != BodyTarget::None).then_some(OperandFamily::Permanent)
                        }
                        crate::toolbox::ObjectMutation::ResolveCounterKeyword(keyword) => {
                            if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == TriggerSpec::counter_keyword(*keyword))
                                || self.instructions.len() != 1
                                || self.target != BodyTarget::None
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::KeywordZoneAction(
                            crate::toolbox::KeywordZoneOp::SacrificeStateTriggerSource,
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        grouping: TriggerGrouping::EachOccurrence,
                                        ..
                                    } | BodyContext::Activated { .. }
                                )
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::KeywordZoneAction(
                            crate::toolbox::KeywordZoneOp::SacrificeEvoked,
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        condition: TriggerSpec {
                                            event: TriggerEvent::Enters {
                                                cast_origin: Some(CastEntryKind::Evoked),
                                                ..
                                            },
                                            ..
                                        },
                                        grouping: TriggerGrouping::EachOccurrence,
                                    }
                                )
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::ResolvePowerAbility(_) => {
                            if !matches!(
                                context,
                                BodyContext::Unbound
                                    | BodyContext::Activated { .. }
                                    | BodyContext::SacrificedPermanent
                                    | BodyContext::RevealedCreature
                                    | BodyContext::ExiledCreatureCost
                                    | BodyContext::SacrificedCreature
                            ) || self.target != BodyTarget::None
                                || self.instructions.len() != 1
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ObjectMutation::KeywordZoneAction(
                            crate::toolbox::KeywordZoneOp::ExileUnearthed(_)
                            | crate::toolbox::KeywordZoneOp::ScheduleArtifactReturn(_),
                        ) => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        crate::toolbox::ObjectMutation::KeywordZoneAction(
                            crate::toolbox::KeywordZoneOp::ReturnEarthbentCard,
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(context, BodyContext::TriggerBound { .. })
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        _ => None,
                    };
                    toolbox_merge_family(family, op_fam)?
                }
                Instruction::ApplyContinuous(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    let mut family = match &spec.selector {
                        Some(q) => validate_toolbox_object_query(
                            q,
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?,
                        None => None,
                    };
                    let pay_fam = match &spec.payload {
                        crate::toolbox::ContinuousPayload::DynamicBoost {
                            power,
                            toughness,
                            ..
                        } => toolbox_merge_family(
                            power.required_operand_family(self.target)?,
                            toughness.required_operand_family(self.target)?,
                        )?,
                        crate::toolbox::ContinuousPayload::PlayPermission { player, .. } => {
                            validate_toolbox_player_selector(
                                player,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::ContinuousPayload::SkipTurns { player, count } => {
                            toolbox_merge_family(
                                validate_toolbox_player_selector(
                                    player,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                                count.required_operand_family(self.target)?,
                            )?
                        }
                        crate::toolbox::ContinuousPayload::TemporaryRule(rule) => self
                            .validate_temporary_rule(
                                rule,
                                context,
                                &defined_slots,
                                &facts,
                                vocabulary,
                            )?,
                        crate::toolbox::ContinuousPayload::PreventDamage(prev_spec) => {
                            self.validate_prevent_damage_spec(prev_spec, context)?
                        }
                        crate::toolbox::ContinuousPayload::DamageMultiplier { factor } => {
                            if !factor.is_positive() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            match self.target {
                                BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                } => Some(OperandFamily::Permanent),
                                _ => return Err(DefinitionError::OperandFamily),
                            }
                        }
                        crate::toolbox::ContinuousPayload::BlockDeclarations(_) => {
                            if self.target != BodyTarget::None
                                || !matches!(context, BodyContext::TriggerBound {
                                    condition: TriggerSpec {
                                        event: TriggerEvent::Attacks {
                                            subject,
                                            mode: AttackTriggerMode::Individual { capture_defender: false }
                                                | AttackTriggerMode::WithAtLeastOthers(_)
                                        },
                                        intervening_if: None,
                                        ..
                                    },
                                    ..
                                } if *subject == ObjectFilter::self_object())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        crate::toolbox::ContinuousPayload::ChosenColorProtection { recipients } => {
                            match recipients {
                                ProtectionRecipients::TargetPermanent => match self.target {
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } => Some(OperandFamily::Permanent),
                                    _ => return Err(DefinitionError::OperandFamily),
                                },
                                ProtectionRecipients::AbilitySource => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                ProtectionRecipients::ControllerAndPermanents => None,
                                ProtectionRecipients::Matching(filter) => {
                                    filter.validate_vocabulary(vocabulary)?;
                                    let mut fam = None;
                                    if filter.has_target_player() {
                                        fam = merge_family(fam, Some(OperandFamily::Player))?;
                                    }
                                    if filter.uses_target_permanent() {
                                        fam = merge_family(fam, Some(OperandFamily::Permanent))?;
                                    }
                                    fam
                                }
                            }
                        }
                        crate::toolbox::ContinuousPayload::ModifyAnyNumber { parts } => {
                            if !matches!(
                                self.target,
                                BodyTarget::Object {
                                    zone: TargetZone::Battlefield,
                                    ..
                                }
                            ) || parts.0.iter().any(|part| {
                                matches!(
                                    part,
                                    crate::characteristics::ResolvedPartSpec::Control(
                                        crate::characteristics::ControlRecipient::TargetOwner
                                    )
                                )
                            }) {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            parts
                                .validate(vocabulary, &defined_slots)
                                .map_err(|_| DefinitionError::ContinuousEffect)?;
                            for part in &parts.0 {
                                if let crate::characteristics::ResolvedPartSpec::SetPower(expr)
                                | crate::characteristics::ResolvedPartSpec::SetToughness(
                                    expr,
                                ) = part
                                {
                                    expr.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                }
                            }
                            Some(OperandFamily::Permanent)
                        }
                        crate::toolbox::ContinuousPayload::ResolvedParts(parts) => {
                            if spec.duration.is_materialized_source_bound()
                                || (spec.duration.requires_ability_source()
                                    && !context.has_source())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            parts.validate(vocabulary, &defined_slots).map_err(
                                |error| match error {
                                    CharacteristicDefinitionError::UndefinedCreatureTypeSlot => {
                                        DefinitionError::UndefinedCreatureTypeSlot
                                    }
                                    _ => DefinitionError::ContinuousEffect,
                                },
                            )?;
                            for part in &parts.0 {
                                if let crate::characteristics::ResolvedPartSpec::SetPower(expr)
                                | crate::characteristics::ResolvedPartSpec::SetToughness(
                                    expr,
                                ) = part
                                {
                                    expr.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                }
                            }
                            let is_pair = matches!(
                                spec.selector,
                                Some(crate::toolbox::ObjectQuery::TargetsWithPairedPartner)
                            );
                            if is_pair
                                && self
                                    .permanent_target_bounds()
                                    .is_some_and(|bounds| bounds != (1, 1))
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            if (is_pair
                                || (matches!(
                                    spec.selector,
                                    Some(crate::toolbox::ObjectQuery::Targets)
                                ) && self
                                    .permanent_target_bounds()
                                    .is_some_and(|(_, max)| max > 1)))
                                && parts.0.iter().any(|part| {
                                    matches!(
                                        part,
                                        crate::characteristics::ResolvedPartSpec::Control(
                                            crate::characteristics::ControlRecipient::TargetOwner
                                        )
                                    )
                                })
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            if matches!(
                                spec.selector,
                                Some(
                                    crate::toolbox::ObjectQuery::Targets
                                        | crate::toolbox::ObjectQuery::TargetsWithPairedPartner
                                )
                            ) {
                                Some(OperandFamily::Permanent)
                            } else {
                                None
                            }
                        }
                        crate::toolbox::ContinuousPayload::SkipStep { player, .. } => {
                            validate_toolbox_player_selector(
                                player,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?
                        }
                        crate::toolbox::ContinuousPayload::Emblem(definition) => {
                            match definition.as_ref() {
                                crate::emblem::EmblemDefinition::ReturnFallenArtifacts
                                | crate::emblem::EmblemDefinition::UnlimitedHandWithGraveyardReturn
                                | crate::emblem::EmblemDefinition::SacrificeCreatureGainLifeDraw
                                | crate::emblem::EmblemDefinition::LoyaltyAnyTurn
                                | crate::emblem::EmblemDefinition::PreventAllButOne => {}
                                crate::emblem::EmblemDefinition::ControlledCreatureBonus {
                                    power_toughness,
                                    keywords,
                                } => {
                                    let parts = crate::characteristics::ResolvedPartsSpec(vec![
                                        crate::characteristics::ResolvedPartSpec::Abilities(
                                            crate::characteristics::AbilityChange::grant_keywords(
                                                keywords.iter().copied(),
                                            ),
                                        ),
                                        crate::characteristics::ResolvedPartSpec::ModifyPT(
                                            power_toughness.clone(),
                                        ),
                                    ]);
                                    parts
                                        .validate(vocabulary, &defined_slots)
                                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                                }
                                crate::emblem::EmblemDefinition::SpellcastDamage { amount, .. }
                                | crate::emblem::EmblemDefinition::UpkeepDamage { amount } => {
                                    if !amount.is_positive() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                crate::emblem::EmblemDefinition::Authored {
                                    label,
                                    continuous,
                                    triggered,
                                } => {
                                    if label.trim().is_empty()
                                        || (continuous.is_empty() && triggered.is_empty())
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    let mut static_ids = BTreeSet::new();
                                    for ability in continuous {
                                        if !static_ids.insert(ability.id()) {
                                            return Err(DefinitionError::DuplicateAbility);
                                        }
                                        ability
                                            .validate(vocabulary)
                                            .map_err(|_| DefinitionError::InvalidInstruction)?;
                                        use crate::characteristics::{
                                            ControllerSelector, NameFilter,
                                            StaticContinuousAbility, StaticDamageRecipient,
                                            StaticDamageRule, StaticPart, StaticSourceCondition,
                                        };
                                        let ok = match ability {
                                            StaticContinuousAbility::Battlefield {
                                                selector,
                                                parts,
                                                ..
                                            } => {
                                                !selector.attached_to_source
                                                    && !selector.exclude_source
                                                    && !selector.matches_source_entry_type
                                                    && !selector.source_has_imprinted_creature
                                                    && selector.station_at.is_none()
                                                    && !matches!(
                                                        selector.controller,
                                                        ControllerSelector::SourceItself
                                                            | ControllerSelector::SourcePair
                                                            | ControllerSelector::SelfAtMaxSpeed
                                                            | ControllerSelector::EnchantedPlayer
                                                    )
                                                    && !matches!(
                                                        selector.name,
                                                        NameFilter::SharesWithSource
                                                            | NameFilter::DifferentFromSource
                                                    )
                                                    && !matches!(
                                                        selector.source_condition,
                                                        Some(StaticSourceCondition::HasKeyword(_))
                                                    )
                                                    && !parts.0.iter().any(|p| {
                                                        matches!(
                                                            p,
                                                            StaticPart::InheritExiledCreatureAbilities
                                                                | StaticPart::InheritCounteredCreatureAbilities
                                                                | StaticPart::InheritZoneAbilities { .. }
                                                                | StaticPart::GrantLoyalty { .. }
                                                        )
                                                    })
                                            }
                                            StaticContinuousAbility::DamageRule { rule, .. } => {
                                                !matches!(
                                                    rule,
                                                    StaticDamageRule::ModifyDamage {
                                                        recipient: StaticDamageRecipient::SelfObject
                                                            | StaticDamageRecipient::AttachedHost,
                                                        ..
                                                    } | StaticDamageRule::RedirectPlayerDamageToSource { .. }
                                                        | StaticDamageRule::RedirectPlayerDamageToEnchanted
                                                        | StaticDamageRule::PreventIncomingRemovePlusOne
                                                )
                                            }
                                            StaticContinuousAbility::CharacteristicDefining { .. }
                                            | StaticContinuousAbility::CommandZoneActive { .. } => {
                                                false
                                            }
                                            _ => true,
                                        };
                                        if !ok {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                    }
                                    let mut trig_ids = BTreeSet::new();
                                    for ability in triggered {
                                        if !trig_ids.insert(ability.id) {
                                            return Err(DefinitionError::DuplicateAbility);
                                        }
                                        ability.validate_body(vocabulary)?;
                                    }
                                }
                            }
                            None
                        }
                        crate::toolbox::ContinuousPayload::TargetPreventionShield { amount } => {
                            if !amount.is_positive() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            Some(OperandFamily::DamageRecipient)
                        }
                        crate::toolbox::ContinuousPayload::BecomeCopy { donor, .. } => {
                            if spec.duration.is_materialized_source_bound()
                                || (spec.duration.requires_ability_source()
                                    && !context.has_source())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            if let Some(receiver) = &spec.selector {
                                match receiver {
                                    crate::toolbox::ObjectQuery::TriggerSubject => {
                                        if !matches!(context, BodyContext::TriggerBound { .. }) {
                                            return Err(DefinitionError::InvalidInstruction);
                                        }
                                    }
                                    crate::toolbox::ObjectQuery::EffectSubject(subj) => {
                                        match subj.as_ref() {
                                            EffectSubject::TriggerSubject
                                            | EffectSubject::TriggeringAttacker
                                            | EffectSubject::TriggeringAttackGroup
                                            | EffectSubject::TriggeringTarget => {
                                                if !matches!(
                                                    context,
                                                    BodyContext::TriggerBound { .. }
                                                ) {
                                                    return Err(
                                                        DefinitionError::InvalidInstruction,
                                                    );
                                                }
                                            }
                                            EffectSubject::TargetObject
                                            | EffectSubject::TargetAndSameNameMatching(_)
                                            | EffectSubject::MatchingOtherThanTarget(_)
                                            | EffectSubject::TargetAndSharingColorMatching(_)
                                                if !matches!(
                                                    self.target,
                                                    BodyTarget::Object {
                                                        zone: TargetZone::Battlefield,
                                                        ..
                                                    }
                                                ) =>
                                            {
                                                return Err(DefinitionError::OperandFamily);
                                            }
                                            _ => {}
                                        }
                                    }
                                    crate::toolbox::ObjectQuery::Targets
                                        if !matches!(
                                            self.target,
                                            BodyTarget::Object {
                                                zone: TargetZone::Battlefield,
                                                ..
                                            }
                                        ) =>
                                    {
                                        return Err(DefinitionError::OperandFamily);
                                    }
                                    _ => {}
                                }
                            }
                            match donor.to_move_subject() {
                                Some(
                                    MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost,
                                ) => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TriggeringObject) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    None
                                }
                                Some(MoveSubjectRef::TargetObject) => {
                                    if donor_moved {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                    match self.target {
                                        BodyTarget::Object {
                                            zone: TargetZone::Battlefield,
                                            ..
                                        } => Some(OperandFamily::Permanent),
                                        _ => return Err(DefinitionError::OperandFamily),
                                    }
                                }
                                Some(_) => None,
                                None => validate_toolbox_object_query(
                                    donor,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            }
                        }
                        _ => None,
                    };
                    family = toolbox_merge_family(family, pay_fam)?;
                    if let Some(p) = &spec.as_emblem {
                        family = toolbox_merge_family(
                            family,
                            validate_toolbox_player_selector(
                                p,
                                context,
                                self.target,
                                facts.definitely_exiled_target,
                            )?,
                        )?;
                    }
                    family
                }
                Instruction::PromptChoice(spec) => {
                    spec.validate_vocabulary(vocabulary)?;
                    if let crate::toolbox::PromptDomain::Subtype {
                        legacy_slot: Some(into),
                        ..
                    } = &spec.domain
                        && !defined_slots.insert(*into)
                    {
                        return Err(DefinitionError::DuplicateCreatureTypeSlot);
                    }
                    let mut family = validate_toolbox_player_selector(
                        &spec.chooser,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    match &spec.domain {
                        crate::toolbox::PromptDomain::Objects {
                            query,
                            min,
                            max,
                            max_total_power,
                            ..
                        } => {
                            min.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            max.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            if let Some(limit) = max_total_power {
                                limit.validate_amount(
                                    &defined_slots,
                                    context,
                                    &facts,
                                    vocabulary,
                                )?;
                            }
                            family = toolbox_merge_family(
                                family,
                                validate_toolbox_object_query(
                                    query,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?;
                            family = toolbox_merge_family(
                                family,
                                min.required_operand_family(self.target)?,
                            )?;
                            family = toolbox_merge_family(
                                family,
                                max.required_operand_family(self.target)?,
                            )?;
                            if let Some(limit) = max_total_power {
                                family = toolbox_merge_family(
                                    family,
                                    limit.required_operand_family(self.target)?,
                                )?;
                            }
                        }
                        crate::toolbox::PromptDomain::Players { selector, .. } => {
                            facts.chose_player = true;
                            family = toolbox_merge_family(
                                family,
                                validate_toolbox_player_selector(
                                    selector,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?;
                        }
                        crate::toolbox::PromptDomain::Number { min, max } => {
                            min.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            max.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            if let (ValueExpr::Literal(lo), ValueExpr::Literal(hi)) =
                                (min.as_ref(), max.as_ref())
                                && (lo > hi || hi.minus(lo) > 100.into())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            family = toolbox_merge_family(
                                family,
                                min.required_operand_family(self.target)?,
                            )?;
                            family = toolbox_merge_family(
                                family,
                                max.required_operand_family(self.target)?,
                            )?;
                        }
                        crate::toolbox::PromptDomain::Partition { query, .. } => {
                            family = toolbox_merge_family(
                                family,
                                validate_toolbox_object_query(
                                    query,
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?;
                        }
                        crate::toolbox::PromptDomain::ManaPayment {
                            fixed,
                            extra_generic,
                            paid_destination,
                            declined_destination,
                        } => {
                            if !fixed.valid() {
                                return Err(DefinitionError::InvalidResolutionPaymentCost);
                            }
                            let is_chosen_x_payment = **extra_generic == ValueExpr::chosen_x()
                                || matches!(
                                    extra_generic.as_ref(),
                                    ValueExpr::Scale { expr, factor }
                                        if **expr == ValueExpr::chosen_x() && factor.is_positive()
                                );
                            if !is_chosen_x_payment {
                                extra_generic.validate_amount(
                                    &defined_slots,
                                    context,
                                    &facts,
                                    vocabulary,
                                )?;
                            }
                            Self::validate_destination(index, *paid_destination, length)?;
                            Self::validate_destination(index, *declined_destination, length)?;
                        }
                        crate::toolbox::PromptDomain::Options { .. }
                        | crate::toolbox::PromptDomain::Color { .. }
                        | crate::toolbox::PromptDomain::CardType { .. }
                        | crate::toolbox::PromptDomain::Subtype { .. }
                        | crate::toolbox::PromptDomain::CardName { .. } => {}
                    }
                    for branch in &spec.option_branches {
                        if branch.target != BodyTarget::None {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if branch.validate_in(vocabulary, context).is_err() {
                            if self.target == BodyTarget::None {
                                branch.validate_in(vocabulary, context)?;
                            } else {
                                let mut inherited = branch.clone();
                                inherited.target = self.target;
                                inherited.validate_in(vocabulary, context)?;
                                family =
                                    merge_family(family, inherited_target_family(self.target))?;
                            }
                        }
                    }
                    family
                }
                Instruction::BranchCondition {
                    condition,
                    destination,
                } => {
                    if let crate::toolbox::ConditionExpr::Resolution(cond) = condition.as_ref() {
                        cond.validate(context, vocabulary)?;
                        if !(facts.moved_library_cards || facts.maybe_moved_library_cards)
                            && [&cond.left, &cond.right].iter().any(|value| {
                                matches!(
                                    value,
                                    ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::MovedLibraryCard,
                                        ..
                                    }
                                )
                            })
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if !(facts.checked_destruction_target
                            || facts.maybe_checked_destruction_target)
                            && [&cond.left, &cond.right].iter().any(|value| {
                                matches!(
                                    value,
                                    ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::DestructionTarget,
                                        ..
                                    }
                                )
                            })
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if !(facts.definitely_discarded || facts.maybe_discarded)
                            && [&cond.left, &cond.right].iter().any(|value| {
                                matches!(
                                    value,
                                    ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::DiscardedCard,
                                        ..
                                    }
                                )
                            })
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if !(facts.definitely_selected_graveyard_exile
                            || facts.maybe_selected_graveyard_exile)
                            && [&cond.left, &cond.right].contains(&&ValueExpr::event(
                                EventValueKind::SelectedGraveyardExileCreatures,
                            ))
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if !(facts.definitely_sacrificed || facts.maybe_sacrificed)
                            && [&cond.left, &cond.right].contains(&&ValueExpr::event(
                                EventValueKind::LastSacrificeSucceeded,
                            ))
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if !(facts.definitely_checked_top || facts.maybe_checked_top)
                            && [&cond.left, &cond.right].iter().any(|value| {
                                matches!(
                                    value,
                                    ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::RevealedTop,
                                        ..
                                    }
                                )
                            })
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if [&cond.left, &cond.right].iter().any(|value| {
                            matches!(
                                value,
                                ValueExpr::ObjectStat {
                                    subject: ValueObjectRef::TargetObject,
                                    stat: ObjectStatKind::ManaValue,
                                }
                            )
                        }) && !matches!(
                            self.target,
                            BodyTarget::Object {
                                zone: TargetZone::Battlefield
                                    | TargetZone::OwnGraveyard
                                    | TargetZone::AnyGraveyard { .. }
                                    | TargetZone::Exile {
                                        owned_flashback_only: false
                                    },
                                ..
                            } | BodyTarget::Spell {
                                include_abilities: false,
                                ..
                            }
                        ) {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        Self::validate_destination(index, *destination, length)?;
                        let left_fam = cond.left.required_operand_family(self.target)?;
                        let right_fam = cond.right.required_operand_family(self.target)?;
                        merge_family(left_fam, right_fam)?
                    } else {
                        Self::validate_destination(index, *destination, length)?;
                        condition.validate_vocabulary(vocabulary)?;
                        None
                    }
                }
                Instruction::ForEach {
                    query, body: inner, ..
                } => {
                    query.validate_vocabulary(vocabulary)?;
                    let mut family = validate_toolbox_object_query(
                        query,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    if inner.validate_in(vocabulary, context).is_err() {
                        if self.target == BodyTarget::None || inner.target != BodyTarget::None {
                            inner.validate_in(vocabulary, context)?;
                        } else {
                            let mut inherited = inner.as_ref().clone();
                            inherited.target = self.target;
                            inherited.validate_in(vocabulary, context)?;
                            family = merge_family(family, inherited_target_family(self.target))?;
                        }
                    }
                    family
                }
                Instruction::ForEachPlayerQuery {
                    selector,
                    body: inner,
                    ..
                } => {
                    selector.validate_vocabulary(vocabulary)?;
                    let mut family = validate_toolbox_player_selector(
                        selector,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?;
                    for sub in &inner.instructions {
                        if let Instruction::ZoneOp(spec) = sub {
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Graveyard {
                                    semantics: crate::toolbox::GraveyardSemantics::Destroy { .. }
                                }
                            ) {
                                facts.definitely_destroyed = true;
                                facts.maybe_destroyed = true;
                            }
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Graveyard {
                                    semantics: crate::toolbox::GraveyardSemantics::Mill
                                }
                            ) {
                                facts.definitely_milled = true;
                            }
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Graveyard {
                                    semantics: crate::toolbox::GraveyardSemantics::Sacrifice
                                }
                            ) {
                                facts.definitely_sacrificed = true;
                                facts.maybe_sacrificed = true;
                            }
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Graveyard {
                                    semantics: crate::toolbox::GraveyardSemantics::Discard
                                }
                            ) {
                                facts.definitely_discarded = true;
                                facts.maybe_discarded = true;
                            }
                            if matches!(spec.destination, crate::toolbox::ZoneDest::Exile { .. })
                                && toolbox_query_scans_graveyard(&spec.source)
                            {
                                facts.definitely_selected_graveyard_exile = true;
                                facts.maybe_selected_graveyard_exile = true;
                            }
                            if toolbox_query_scans_library(&spec.source) {
                                facts.definitely_checked_top = true;
                                facts.maybe_checked_top = true;
                                facts.moved_library_cards = true;
                                facts.maybe_moved_library_cards = true;
                            }
                        }
                    }
                    if inner.validate_in(vocabulary, context).is_err() {
                        if self.target == BodyTarget::None || inner.target != BodyTarget::None {
                            inner.validate_in(vocabulary, context)?;
                        } else {
                            let mut inherited = inner.as_ref().clone();
                            inherited.target = self.target;
                            inherited.validate_in(vocabulary, context)?;
                            family = merge_family(family, inherited_target_family(self.target))?;
                        }
                    }
                    family
                }
                Instruction::BindEnv { .. } => None,
                Instruction::SetObjectTag { query, .. } => {
                    query.validate_vocabulary(vocabulary)?;
                    validate_toolbox_object_query(
                        query,
                        context,
                        self.target,
                        facts.definitely_exiled_target,
                    )?
                }
                Instruction::UpdateStateTrack { key, op } => {
                    match &key.scope {
                        crate::toolbox::StateTrackScope::Global => {}
                        crate::toolbox::StateTrackScope::Player(p) => {
                            p.validate_vocabulary(vocabulary)?;
                        }
                        crate::toolbox::StateTrackScope::Object(q) => {
                            q.validate_vocabulary(vocabulary)?;
                        }
                    }
                    match op {
                        crate::toolbox::StateTrackOp::SetInt(e)
                        | crate::toolbox::StateTrackOp::AddInt(e) => {
                            e.validate_vocabulary(vocabulary)?;
                        }
                        crate::toolbox::StateTrackOp::SetSeat(p) => {
                            p.validate_vocabulary(vocabulary)?;
                        }
                        _ => {}
                    }
                    None
                }
                Instruction::SideDeckOp(spec) => {
                    if let Some(p) = &spec.owner {
                        p.validate_vocabulary(vocabulary)?;
                    }
                    None
                }
                Instruction::StackOp(spec) => {
                    use crate::toolbox::{CounterDestination, StackOpKind, StackTargetSelector};
                    spec.validate_vocabulary(vocabulary)?;
                    if let StackOpKind::Copy { count, .. }
                    | StackOpKind::Discover { value: count } = &spec.op
                    {
                        count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    }
                    let count_family = if let StackOpKind::Copy { count, .. } = &spec.op {
                        count.required_operand_family(self.target)?
                    } else {
                        None
                    };
                    let target_family = match (&spec.target, &spec.op) {
                        (
                            StackTargetSelector::Targeted,
                            StackOpKind::Counter { destination, .. },
                        ) => {
                            if !matches!(destination, CounterDestination::Graveyard)
                                && !matches!(
                                    self.target,
                                    BodyTarget::Spell {
                                        include_abilities: false,
                                        ..
                                    }
                                )
                            {
                                return Err(DefinitionError::OperandFamily);
                            }
                            Some(
                                if matches!(
                                    self.target,
                                    BodyTarget::Spell {
                                        include_abilities: true,
                                        ..
                                    } | BodyTarget::StackEntry(_)
                                ) {
                                    OperandFamily::StackEntry
                                } else {
                                    OperandFamily::Spell
                                },
                            )
                        }
                        (StackTargetSelector::Targeted, StackOpKind::GainControl) => {
                            Some(OperandFamily::Spell)
                        }
                        (
                            StackTargetSelector::Targeted,
                            StackOpKind::Retarget {
                                single_target_only: false,
                            },
                        ) => Some(
                            if matches!(
                                self.target,
                                BodyTarget::Spell {
                                    include_abilities: false,
                                    ..
                                }
                            ) {
                                OperandFamily::Spell
                            } else {
                                OperandFamily::StackEntry
                            },
                        ),
                        (
                            StackTargetSelector::Targeted,
                            StackOpKind::Retarget {
                                single_target_only: true,
                            },
                        ) => match self.target {
                            BodyTarget::Spell {
                                include_abilities: false,
                                ..
                            } => Some(OperandFamily::Spell),
                            BodyTarget::Spell {
                                include_abilities: true,
                                ..
                            }
                            | BodyTarget::StackEntry(_) => Some(OperandFamily::StackEntry),
                            _ => return Err(DefinitionError::InvalidInstruction),
                        },
                        (
                            StackTargetSelector::Targeted,
                            StackOpKind::Copy {
                                ability_mode: true, ..
                            },
                        ) => {
                            if matches!(
                                self.target,
                                BodyTarget::StackEntry(StackTargetKind::ControlledAbility { .. })
                            ) {
                                Some(OperandFamily::StackEntry)
                            } else {
                                return Err(DefinitionError::OperandFamily);
                            }
                        }
                        (
                            StackTargetSelector::Targeted,
                            StackOpKind::Copy {
                                ability_mode: false,
                                ..
                            },
                        ) => Some(OperandFamily::Spell),
                        (StackTargetSelector::Targeted, _) => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        (StackTargetSelector::Matching { .. }, _) => None,
                        (
                            StackTargetSelector::Triggering,
                            StackOpKind::Counter {
                                ward_source: true, ..
                            },
                        ) => {
                            if self.target != BodyTarget::None
                                || !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        condition: TriggerSpec {
                                            event: TriggerEvent::Targeted { .. },
                                            ..
                                        },
                                        grouping: TriggerGrouping::EachOccurrence,
                                    }
                                )
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        (StackTargetSelector::Triggering, _) => {
                            if !matches!(context, BodyContext::TriggerBound { .. }) {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        (StackTargetSelector::Source, op) => match op {
                            StackOpKind::Discover { .. } => None,
                            StackOpKind::Encore => {
                                if !matches!(
                                    context,
                                    BodyContext::Unbound | BodyContext::Activated { .. }
                                ) || self.target != BodyTarget::None
                                    || self.instructions.len() != 1
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            StackOpKind::CastDefeatedSiege => {
                                if self.target != BodyTarget::None
                                    || !matches!(
                                        context,
                                        BodyContext::TriggerBound {
                                            condition: TriggerSpec {
                                                event: TriggerEvent::CountersChanged {
                                                    kind: CounterTriggerKind::LastDefenseRemoved,
                                                    ..
                                                },
                                                ..
                                            },
                                            grouping: TriggerGrouping::EachOccurrence,
                                        }
                                    )
                                    || self.instructions
                                        != [
                                            Instruction::MutateObject(Box::new(
                                                crate::toolbox::MutateObjectSpec::exile_defeated_siege(),
                                            )),
                                            Instruction::StackOp(Box::new(
                                                crate::toolbox::StackOpSpec::cast_defeated_siege(),
                                            )),
                                        ]
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            StackOpKind::Cascade | StackOpKind::CascadeBottom => {
                                if !matches!(
                                    context,
                                    BodyContext::TriggerBound {
                                        condition,
                                        grouping: TriggerGrouping::EachOccurrence
                                    } if *condition == TriggerSpec::self_cast()
                                ) || self.target != BodyTarget::None
                                    || self.instructions
                                        != [
                                            Instruction::StackOp(Box::new(
                                                crate::toolbox::StackOpSpec::cascade(),
                                            )),
                                            Instruction::StackOp(Box::new(
                                                crate::toolbox::StackOpSpec::cascade_bottom(),
                                            )),
                                        ]
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            StackOpKind::CopyOnCast(kind) => {
                                let valid_source = match kind {
                                    CastCopyKeyword::OfferCopyToOtherPlayers => {
                                        matches!(context, BodyContext::TriggerBound {
                                            condition: TriggerSpec {
                                                event: TriggerEvent::SpellCast {
                                                    caster: TriggerPlayerScope::EnchantedPlayer,
                                                    filter,
                                                    ..
                                                },
                                                ..
                                            },
                                            grouping: TriggerGrouping::EachOccurrence,
                                        } if *filter == SpellFilter::instant_or_sorcery())
                                    }
                                    CastCopyKeyword::CopyObservedSpell => {
                                        matches!(context, BodyContext::TriggerBound {
                                            condition: TriggerSpec {
                                                event: TriggerEvent::SpellCast {
                                                    caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                                    filter,
                                                    self_cast: false,
                                                    ..
                                                },
                                                ..
                                            },
                                            grouping: TriggerGrouping::EachOccurrence,
                                        } if *filter == SpellFilter::instant_or_sorcery().from_own_library())
                                    }
                                    _ => {
                                        matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == TriggerSpec::self_cast())
                                    }
                                };
                                if !valid_source
                                    || self.instructions.len() != 1
                                    || self.target != BodyTarget::None
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            StackOpKind::CopyCapturedAbility
                            | StackOpKind::EndStepCast { .. }
                            | StackOpKind::MadnessCast
                            | StackOpKind::MiracleCast(_)
                            | StackOpKind::ReboundCast(_)
                            | StackOpKind::ChooseFoundSpell
                            | StackOpKind::CastFoundSpell(_) => {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            _ => None,
                        },
                        (StackTargetSelector::CopyTarget(ct), _) => match ct {
                            StackCopyTarget::TargetSpell => match self.target {
                                BodyTarget::Spell {
                                    include_abilities: false,
                                    ..
                                } => Some(OperandFamily::Spell),
                                BodyTarget::Spell {
                                    include_abilities: true,
                                    ..
                                }
                                | BodyTarget::StackEntry(StackTargetKind::AnyEntry) => {
                                    Some(OperandFamily::StackEntry)
                                }
                                _ => return Err(DefinitionError::OperandFamily),
                            },
                            StackCopyTarget::TargetAbility => match self.target {
                                BodyTarget::Spell {
                                    include_abilities: true,
                                    ..
                                }
                                | BodyTarget::StackEntry(_) => Some(OperandFamily::StackEntry),
                                _ => return Err(DefinitionError::OperandFamily),
                            },
                            StackCopyTarget::TargetSpellOrAbility => match self.target {
                                BodyTarget::Spell {
                                    include_abilities: false,
                                    ..
                                } => Some(OperandFamily::Spell),
                                BodyTarget::Spell {
                                    include_abilities: true,
                                    ..
                                }
                                | BodyTarget::StackEntry(_) => Some(OperandFamily::StackEntry),
                                _ => return Err(DefinitionError::OperandFamily),
                            },
                            StackCopyTarget::TriggeringSpell
                            | StackCopyTarget::TriggeringAbility => {
                                if !matches!(context, BodyContext::TriggerBound { .. }) {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                            StackCopyTarget::ResolvingSpell => None,
                        },
                    };
                    merge_family(target_family, count_family)?
                }
                Instruction::Jump { destination } => {
                    Self::validate_destination(index, *destination, length)?;
                    None
                }
                Instruction::OptionalSequence {
                    chooser,
                    instructions,
                } => {
                    let next = index
                        .checked_add(1)
                        .ok_or(DefinitionError::InvalidControlFlow)?;
                    let effect = self
                        .instructions
                        .get(next)
                        .ok_or(DefinitionError::MissingOptionalEffect)?;
                    let allowed_head = effect.is_optional_primitive()
                        || (*instructions >= 2
                            && matches!(
                                effect,
                                Instruction::PromptChoice(spec)
                                    if matches!(
                                        spec.domain,
                                        crate::toolbox::PromptDomain::ManaPayment { .. }
                                    )
                            ));
                    if !allowed_head {
                        return Err(DefinitionError::UnsupportedOptionalEffect);
                    }
                    if *instructions == 0 {
                        return Err(DefinitionError::InvalidControlFlow);
                    }
                    let declined = next
                        .checked_add(*instructions)
                        .ok_or(DefinitionError::InvalidControlFlow)?;
                    Self::validate_destination(index, declined, length)?;
                    if *chooser == ResolvingPlayer::DestructionTargetController {
                        if !facts.checked_destruction_target
                            || !matches!(effect, Instruction::SearchLibrary { filter } if filter.searcher == ResolvingPlayer::DestructionTargetController)
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        None
                    } else {
                        chooser.validate(context, self.target, facts.definitely_exiled_target)?
                    }
                }
                Instruction::CreateTokens { creations } => {
                    if creations.is_empty() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    let mut token_family = None;
                    for creation in creations {
                        if let Some(
                            crate::token::AttackingDefenderScope::Player(role)
                            | crate::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(role),
                        ) = creation.attacking_defender
                        {
                            token_family = merge_family(
                                token_family,
                                role.validate(
                                    context,
                                    self.target,
                                    facts.definitely_exiled_target,
                                )?,
                            )?;
                        }
                        if let crate::token::TokenRecipients::Player(role) = creation.recipients
                            && let Some(fam) = role.validate(context, self.target, false)?
                        {
                            token_family = Some(fam);
                        }
                        if creation.recipients
                            == crate::token::TokenRecipients::CombatDamageSourceController
                        {
                            ResolvingPlayer::CombatDamageSourceController.validate(
                                context,
                                self.target,
                                false,
                            )?;
                        }
                        if creation.recipients
                            == crate::token::TokenRecipients::RememberedTargetController
                            && !facts.remembered_permanent
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if creation.recipients
                            == crate::token::TokenRecipients::ExiledTargetController
                            && !facts.definitely_exiled_target
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        creation
                            .definition
                            .validate(vocabulary)
                            .map_err(|_| DefinitionError::TokenDefinition)?;
                        if let Some(size) = &creation.base_size {
                            let definition = creation.definition.program();
                            if !definition.has_type(CardType::Creature) {
                                return Err(DefinitionError::TokenDefinition);
                            }
                            match size {
                                crate::token::TokenSize::AtCreation(amount) => {
                                    amount.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    token_family = merge_family(
                                        token_family,
                                        amount.required_operand_family(self.target)?,
                                    )?;
                                }
                                crate::token::TokenSize::AtCreationPair { power, toughness } => {
                                    power.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    toughness.validate_amount(
                                        &defined_slots,
                                        context,
                                        &facts,
                                        vocabulary,
                                    )?;
                                    token_family = merge_family(
                                        token_family,
                                        power.required_operand_family(self.target)?,
                                    )?;
                                    token_family = merge_family(
                                        token_family,
                                        toughness.required_operand_family(self.target)?,
                                    )?;
                                }
                                crate::token::TokenSize::AbilitySourceCounters(kind) => {
                                    if !matches!(context, BodyContext::TriggerBound { .. })
                                        || !kind.tracks_linked_values()
                                        || definition.continuous.is_some()
                                    {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                            }
                        }
                        creation.amount.validate_amount(
                            &defined_slots,
                            context,
                            &facts,
                            vocabulary,
                        )?;
                        token_family = merge_family(
                            token_family,
                            creation.amount.required_operand_family(self.target)?,
                        )?;
                    }
                    token_family
                }
                Instruction::FixedTargetClause(body) => {
                    let only_second_target_bounds = body.permanent_target_bounds() == Some((2, 2))
                        && body.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::ApplyContinuous(spec)
                                    if matches!(
                                        &spec.payload,
                                        crate::toolbox::ContinuousPayload::TemporaryRule(rule)
                                            if matches!(
                                                rule.as_ref(),
                                                TemporaryRuleKind::RedirectNextDamage {
                                                    to: crate::characteristics::DamageRedirectTarget::SecondTargetObject,
                                                    ..
                                                }
                                            )
                                    )
                            )
                        });
                    if self.target != BodyTarget::None
                        || body.target == BodyTarget::None
                        || body.instructions.is_empty()
                        || (body.permanent_target_bounds().is_some() && !only_second_target_bounds)
                        || !body.instructions.iter().any(Instruction::uses_mode_target)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    body.validate_in(vocabulary, context)?;
                    None
                }
                Instruction::SearchLibrary { filter } => {
                    if matches!(filter.source, SearchSource::PlayerLibrary(_)) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if filter.is_linked_exile()
                        && !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition,
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *condition == TriggerSpec::self_enters()
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if filter.searcher == ResolvingPlayer::DestructionTargetController
                        && (!facts.checked_destruction_target
                            || index == 0
                            || self.instructions[index - 1]
                                != (Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::DestructionTargetController,
                                    instructions: 1,
                                }))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if filter.searcher == ResolvingPlayer::ExiledTargetController
                        && !facts.definitely_exiled_target
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    match &filter.matcher {
                        SearchCardMatcher::LandSubtypes { subtypes, .. } if subtypes.is_empty() => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        SearchCardMatcher::CardTypes(types) if types.is_empty() => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        SearchCardMatcher::AnyOfNames(names)
                            if names.is_empty()
                                || names.iter().collect::<BTreeSet<_>>().len() != names.len() =>
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        SearchCardMatcher::SameNameAs { reference, extra } => {
                            match reference {
                                MoveSubjectRef::AbilitySource | MoveSubjectRef::AttachedHost => {
                                    if !context.has_source() {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                MoveSubjectRef::TriggeringObject => {
                                    if !matches!(context, BodyContext::TriggerBound { .. }) {
                                        return Err(DefinitionError::InvalidInstruction);
                                    }
                                }
                                MoveSubjectRef::TargetObject => {
                                    if !matches!(
                                        self.target,
                                        BodyTarget::Object {
                                            zone: TargetZone::Battlefield,
                                            ..
                                        }
                                    ) {
                                        return Err(DefinitionError::OperandFamily);
                                    }
                                }
                                MoveSubjectRef::RevealedCard
                                | MoveSubjectRef::CreatedTokens
                                | MoveSubjectRef::LastMoved
                                | MoveSubjectRef::RememberedFirst
                                | MoveSubjectRef::RememberedSecond => {}
                            }
                            if let Some(obj_filter) = extra {
                                if obj_filter.uses_outside_named_slots() {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                obj_filter
                                    .validate_vocabulary(vocabulary)
                                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                            }
                        }
                        SearchCardMatcher::CreatureSubtype(kind)
                            if vocabulary.label(*kind).is_none() =>
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        SearchCardMatcher::Filtered(obj_filter) => {
                            if obj_filter.uses_outside_named_slots() {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            obj_filter
                                .validate_vocabulary(vocabulary)
                                .map_err(|_| DefinitionError::InvalidInstruction)?;
                        }
                        _ => {}
                    }
                    match &filter.mana_value {
                        Some(SearchManaValue::Exact(value)) if value < &0.into() => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        Some(SearchManaValue::ExactManaCostOneOf(costs))
                            if costs.is_empty() || !costs.iter().all(ManaCost::valid) =>
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        _ => {}
                    }
                    if let Some(host) = filter.curse_host() {
                        match host {
                            AuraSearchHost::TargetPlayer => ResolvingPlayer::TargetPlayer
                                .validate(context, self.target, false)?,
                            AuraSearchHost::EnchantedPlayerWithDifferentCurseNames => {
                                if self.target != BodyTarget::None
                                    || !matches!(
                                        context,
                                        BodyContext::TriggerBound {
                                            condition,
                                            grouping: TriggerGrouping::EachOccurrence,
                                        } if *condition == TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller)
                                    )
                                {
                                    return Err(DefinitionError::InvalidInstruction);
                                }
                                None
                            }
                        }
                    } else if filter.source == SearchSource::TargetOpponentLibrary {
                        if self.target != BodyTarget::SINGLE_OPPONENT {
                            return Err(DefinitionError::OperandFamily);
                        }
                        Some(OperandFamily::Player)
                    } else if filter.source == SearchSource::TargetPlayerLibrary {
                        if !matches!(self.target, BodyTarget::Player { .. }) {
                            return Err(DefinitionError::OperandFamily);
                        }
                        Some(OperandFamily::Player)
                    } else if filter.searcher == ResolvingPlayer::TargetPlayer {
                        filter.searcher.validate(
                            context,
                            self.target,
                            facts.definitely_exiled_target,
                        )?
                    } else if matches!(
                        &filter.matcher,
                        SearchCardMatcher::SameNameAs {
                            reference: MoveSubjectRef::TargetObject,
                            ..
                        }
                    ) {
                        Some(OperandFamily::Permanent)
                    } else {
                        None
                    }
                }
                Instruction::ScheduleDelayedStepTrigger {
                    step,
                    own_step_only,
                    body,
                } => {
                    let player = if *own_step_only {
                        TriggerPlayerScope::Relation(PlayerRelation::Controller)
                    } else {
                        TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant)
                    };
                    let mut delayed_spec = TriggerSpec::new(TriggerEvent::StepBegan {
                        step: *step,
                        player,
                    });
                    if matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                zone: TriggerZone::Graveyard,
                                ..
                            } | TriggerSpec {
                                event: TriggerEvent::PlayerAction {
                                    action: PlayerTriggerAction::DiscardedByOpponent(_),
                                    ..
                                },
                                ..
                            },
                            ..
                        }
                    ) {
                        delayed_spec.zone = TriggerZone::Graveyard;
                    }
                    body.validate_in(
                        vocabulary,
                        BodyContext::TriggerBound {
                            condition: &delayed_spec,
                            grouping: TriggerGrouping::EachOccurrence,
                        },
                    )?;
                    None
                }
                Instruction::EmitReflexiveTrigger { body } => {
                    let inherited_facts = FlowFacts {
                        remembered_permanent: facts.remembered_permanent,
                        definitely_exiled_target: facts.definitely_exiled_target,
                        maybe_exiled_target: facts.maybe_exiled_target,
                        definitely_discarded: facts.definitely_discarded,
                        maybe_discarded: facts.maybe_discarded,
                        definitely_destroyed: facts.definitely_destroyed,
                        maybe_destroyed: facts.maybe_destroyed,
                        checked_destruction_target: facts.checked_destruction_target,
                        maybe_checked_destruction_target: facts.maybe_checked_destruction_target,
                        moved_library_cards: facts.moved_library_cards,
                        maybe_moved_library_cards: facts.maybe_moved_library_cards,
                        definitely_milled: facts.definitely_milled,
                        definitely_milled_top: facts.definitely_milled_top,
                        maybe_milled_top: facts.maybe_milled_top,
                        definitely_exiled_milled: facts.definitely_exiled_milled,
                        maybe_exiled_milled: facts.maybe_exiled_milled,
                        definitely_checked_top: facts.definitely_checked_top,
                        maybe_checked_top: facts.maybe_checked_top,
                        definitely_sacrificed: facts.definitely_sacrificed,
                        maybe_sacrificed: facts.maybe_sacrificed,
                        definitely_selected_graveyard_exile: facts
                            .definitely_selected_graveyard_exile,
                        maybe_selected_graveyard_exile: facts.maybe_selected_graveyard_exile,
                        returned_graveyard_target: facts.returned_graveyard_target,
                        defined_resolution_x: facts.defined_resolution_x,
                        maybe_defined_resolution_x: facts.maybe_defined_resolution_x,
                        chose_player: facts.chose_player,
                        maybe_chose_player: facts.maybe_chose_player,
                        ..FlowFacts::default()
                    };
                    if matches!(context, BodyContext::TriggerBound { .. }) {
                        body.validate_in_with_facts(vocabulary, context, inherited_facts)?;
                    } else {
                        let reflexive_spec = TriggerSpec::new(TriggerEvent::StepBegan {
                            step: crate::turn::Step::End,
                            player: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
                        });
                        body.validate_in_with_facts(
                            vocabulary,
                            BodyContext::TriggerBound {
                                condition: &reflexive_spec,
                                grouping: TriggerGrouping::EachOccurrence,
                            },
                            inherited_facts,
                        )?;
                    }
                    None
                }
                Instruction::ScheduleDelayedEventTrigger { trigger, body, .. } => {
                    let temp = TriggeredAbility {
                        id: TriggeredAbilityId(0),
                        condition: trigger.as_ref().clone(),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: body.as_ref().clone(),
                    };
                    temp.validate_body(vocabulary)?;
                    None
                }
                Instruction::RepeatLoop { count, body } => {
                    if body.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    let loop_facts = FlowFacts {
                        definitely_exiled_target: facts.definitely_exiled_target,
                        maybe_exiled_target: facts.maybe_exiled_target,
                        remembered_permanent: facts.remembered_permanent,
                        definitely_discarded: facts.definitely_discarded,
                        maybe_discarded: facts.maybe_discarded,
                        definitely_destroyed: facts.definitely_destroyed,
                        maybe_destroyed: facts.maybe_destroyed,
                        checked_destruction_target: facts.checked_destruction_target,
                        maybe_checked_destruction_target: facts.maybe_checked_destruction_target,
                        moved_library_cards: facts.moved_library_cards,
                        maybe_moved_library_cards: facts.maybe_moved_library_cards,
                        definitely_milled: facts.definitely_milled,
                        definitely_milled_top: facts.definitely_milled_top,
                        maybe_milled_top: facts.maybe_milled_top,
                        definitely_exiled_milled: facts.definitely_exiled_milled,
                        maybe_exiled_milled: facts.maybe_exiled_milled,
                        definitely_checked_top: facts.definitely_checked_top,
                        maybe_checked_top: facts.maybe_checked_top,
                        definitely_sacrificed: facts.definitely_sacrificed,
                        maybe_sacrificed: facts.maybe_sacrificed,
                        definitely_selected_graveyard_exile: facts
                            .definitely_selected_graveyard_exile,
                        maybe_selected_graveyard_exile: facts.maybe_selected_graveyard_exile,
                        returned_graveyard_target: facts.returned_graveyard_target,
                        defined_resolution_x: facts.defined_resolution_x,
                        maybe_defined_resolution_x: facts.maybe_defined_resolution_x,
                        chose_player: facts.chose_player,
                        maybe_chose_player: facts.maybe_chose_player,
                        ..FlowFacts::default()
                    };
                    if body
                        .validate_in_with_facts(vocabulary, context, loop_facts.clone())
                        .is_ok()
                    {
                        None
                    } else if self.target != BodyTarget::None {
                        let mut inherited = body.as_ref().clone();
                        inherited.target = self.target;
                        inherited.validate_in_with_facts(vocabulary, context, loop_facts)?;
                        match self.target {
                            BodyTarget::Player { .. } => Some(OperandFamily::Player),
                            BodyTarget::Object {
                                zone: TargetZone::Battlefield,
                                ..
                            } => Some(OperandFamily::Permanent),
                            BodyTarget::AnyTarget(
                                AnyTargetKind::Any
                                | AnyTargetKind::CreatureOrPlayer
                                | AnyTargetKind::PlayerOrPlaneswalker
                                | AnyTargetKind::OpponentOrPlaneswalker,
                            ) => Some(OperandFamily::DamageRecipient),
                            BodyTarget::Spell {
                                include_abilities: false,
                                ..
                            } => Some(OperandFamily::Spell),
                            _ => Some(OperandFamily::GraveyardCard),
                        }
                    } else {
                        body.validate_in_with_facts(vocabulary, context, loop_facts)?;
                        None
                    }
                }
            };
            if let Some(family) = family {
                consuming |= reachable;
                let returned_permanent_followup = facts.returned_graveyard_target
                    && matches!(family, OperandFamily::Permanent)
                    && matches!(
                        self.target,
                        BodyTarget::Object {
                            zone: TargetZone::OwnGraveyard
                                | TargetZone::AnyGraveyard { .. }
                                | TargetZone::RecentlyDiscardedOrFromLibrary,
                            ..
                        }
                    );
                if !returned_permanent_followup
                    && !matches!(
                        (self.target, family),
                        (
                            BodyTarget::Object {
                                zone: TargetZone::Battlefield,
                                ..
                            },
                            OperandFamily::Permanent | OperandFamily::DamageRecipient
                        ) | (
                            BodyTarget::Object {
                                zone: TargetZone::OwnGraveyard
                                    | TargetZone::AnyGraveyard { .. }
                                    | TargetZone::RecentlyDiscardedOrFromLibrary,
                                ..
                            },
                            OperandFamily::GraveyardCard
                        ) | (
                            BodyTarget::Object {
                                zone: TargetZone::Exile { .. },
                                ..
                            },
                            OperandFamily::ExiledCard
                        ) | (
                            BodyTarget::StackEntry(_)
                                | BodyTarget::Spell {
                                    include_abilities: true,
                                    ..
                                },
                            OperandFamily::StackEntry
                        ) | (
                            BodyTarget::Spell {
                                include_abilities: false,
                                ..
                            },
                            OperandFamily::Spell
                        ) | (
                            BodyTarget::Player { .. },
                            OperandFamily::Player | OperandFamily::DamageRecipient
                        ) | (BodyTarget::AnyTarget(_), OperandFamily::DamageRecipient)
                    )
                {
                    return Err(DefinitionError::OperandFamily);
                }
            }
            if reachable {
                facts.definitely = defined_slots;
                facts.donor_moved = donor_moved;
                facts.record_maybe_from_definite();
                if let Instruction::PromptChoice(spec) = instruction
                    && let crate::toolbox::PromptDomain::Subtype {
                        legacy_slot: Some(into),
                        ..
                    } = &spec.domain
                {
                    facts.possibly.insert(*into);
                }
                let next = index
                    .checked_add(1)
                    .ok_or(DefinitionError::InvalidControlFlow)?;
                match instruction {
                    Instruction::Jump { destination } => {
                        facts.merge_into(&mut incoming[*destination])
                    }
                    Instruction::BranchCondition {
                        condition,
                        destination,
                    } => {
                        let guards_last_action = matches!(
                            condition.as_ref(),
                            crate::toolbox::ConditionExpr::Resolution(cond)
                                if matches!(
                                    (&cond.left, cond.comparison, &cond.right),
                                    (
                                        ValueExpr::EventValue(
                                            EventValueKind::LastActionSucceeded
                                                | EventValueKind::LastSacrificeSucceeded
                                                | EventValueKind::LastActionCount
                                        ),
                                        Comparison::GreaterOrEqual | Comparison::Equal,
                                        ValueExpr::Literal(v)
                                    ) if *v == 1.into()
                                ) || matches!(
                                    (&cond.left, cond.comparison, &cond.right),
                                    (
                                        ValueExpr::EventValue(
                                            EventValueKind::LastActionSucceeded
                                                | EventValueKind::LastSacrificeSucceeded
                                                | EventValueKind::LastActionCount
                                        ),
                                        Comparison::NotEqual | Comparison::Greater,
                                        ValueExpr::Literal(v)
                                    ) if *v == 0.into()
                                ) || matches!(
                                    (&cond.left, cond.comparison, &cond.right),
                                    (
                                        ValueExpr::EventValue(EventValueKind::LastActionCount),
                                        Comparison::GreaterOrEqual | Comparison::Greater,
                                        ValueExpr::Literal(v)
                                    ) if v.is_positive()
                                )
                        );
                        if guards_last_action {
                            let mut next_facts = facts.clone();
                            next_facts.promote_maybe_to_definite();
                            next_facts.merge_into(&mut incoming[next]);
                        } else {
                            facts.merge_into(&mut incoming[next]);
                        }
                        facts.merge_into(&mut incoming[*destination]);
                    }
                    Instruction::PromptChoice(spec)
                        if matches!(
                            spec.domain,
                            crate::toolbox::PromptDomain::ManaPayment { .. }
                        ) =>
                    {
                        if let crate::toolbox::PromptDomain::ManaPayment {
                            extra_generic,
                            paid_destination,
                            declined_destination,
                            ..
                        } = &spec.domain
                        {
                            let is_chosen_x_payment = **extra_generic == ValueExpr::chosen_x()
                                || matches!(
                                    extra_generic.as_ref(),
                                    ValueExpr::Scale { expr, factor }
                                        if **expr == ValueExpr::chosen_x() && factor.is_positive()
                                );
                            let mut paid_facts = facts.clone();
                            if is_chosen_x_payment {
                                paid_facts.defined_resolution_x = true;
                                paid_facts.maybe_defined_resolution_x = true;
                            }
                            paid_facts.merge_into(&mut incoming[*paid_destination]);
                            facts.merge_into(&mut incoming[*declined_destination]);
                        }
                    }
                    Instruction::ZoneOp(_)
                    | Instruction::ApplyContinuous(_)
                    | Instruction::PromptChoice(_)
                    | Instruction::ForEach { .. }
                    | Instruction::ForEachPlayerQuery { .. }
                    | Instruction::BindEnv { .. }
                    | Instruction::SetObjectTag { .. }
                    | Instruction::UpdateStateTrack { .. }
                    | Instruction::SideDeckOp(_) => {
                        facts.merge_into(&mut incoming[next]);
                    }
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            crate::toolbox::PlayerMutation::RandomEvent(
                                crate::toolbox::RandomEventOp::FlipCoins {
                                    lose_destination: Some(_),
                                    ..
                                }
                            )
                        ) =>
                    {
                        facts.merge_into(&mut incoming[next]);
                        if let crate::toolbox::PlayerMutation::RandomEvent(
                            crate::toolbox::RandomEventOp::FlipCoins {
                                lose_destination: Some(destination),
                                ..
                            },
                        ) = &spec.op
                        {
                            facts.merge_into(&mut incoming[*destination]);
                        }
                    }
                    Instruction::OptionalSequence { instructions, .. } => {
                        let declined = next
                            .checked_add(*instructions)
                            .ok_or(DefinitionError::InvalidControlFlow)?;
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[declined]);
                    }
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            crate::toolbox::PlayerMutation::OfferSourceCounters { .. }
                        ) =>
                    {
                        if let crate::toolbox::PlayerMutation::OfferSourceCounters {
                            placed_destination: paid_destination,
                            declined_destination,
                            ..
                        } = &spec.op
                        {
                            facts.merge_into(&mut incoming[*paid_destination]);
                            facts.merge_into(&mut incoming[*declined_destination]);
                        }
                    }
                    Instruction::CreateTokens { .. }
                    | Instruction::ScheduleDelayedStepTrigger { .. }
                    | Instruction::EmitReflexiveTrigger { .. }
                    | Instruction::ScheduleDelayedEventTrigger { .. }
                    | Instruction::RepeatLoop { .. }
                    | Instruction::FixedTargetClause(_)
                    | Instruction::SearchLibrary { .. }
                    | Instruction::DrawCards(_)
                    | Instruction::MutateObject(_)
                    | Instruction::AdjustLife(_)
                    | Instruction::DealDamage(_)
                    | Instruction::MutatePlayer(_)
                    | Instruction::StackOp(_) => facts.merge_into(&mut incoming[next]),
                }
            }
        }
        if self.target != BodyTarget::None && !consuming {
            return Err(DefinitionError::PhantomTarget);
        }
        Ok(())
    }
    fn validate_temporary_rule(
        &self,
        rule: &TemporaryRuleKind,
        context: BodyContext<'_>,
        defined_slots: &BTreeSet<CreatureTypeSlot>,
        facts: &FlowFacts,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        let fam = match rule {
            TemporaryRuleKind::CantBeCountered { filter, .. } => {
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                None
            }
            TemporaryRuleKind::PlayerCantGainLife { players }
            | TemporaryRuleKind::CantActivateAbilities { players, .. }
            | TemporaryRuleKind::CantPlayLands { players }
            | TemporaryRuleKind::CastMaxMoreSpellsThisTurn { players, .. } => {
                players.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::CantCastSpells {
                players, filter, ..
            } => {
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                players.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::PlayerGrantKeyword { players, keywords } => {
                if keywords.is_empty() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                players.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::NextSpellModifier {
                player,
                filter,
                free,
                keyword,
                cast_keyword,
                affinity_filter,
            } => {
                if !*free
                    && keyword.is_none()
                    && cast_keyword.is_none()
                    && affinity_filter.is_none()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if let Some(keyword) = keyword
                    && !matches!(
                        keyword,
                        KeywordAbility::Convoke
                            | KeywordAbility::Improvise
                            | KeywordAbility::Delve
                            | KeywordAbility::Flash
                            | KeywordAbility::Bargain
                            | KeywordAbility::Retrace
                            | KeywordAbility::JumpStart
                            | KeywordAbility::Rebound
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if let Some(f) = affinity_filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                player.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::TemporaryZoneReplacement(ability) => {
                zone_replacement::validate_ability(ability, true, vocabulary)?;
                None
            }
            TemporaryRuleKind::DamageCantBePrevented => None,
            TemporaryRuleKind::ExileReplacement {
                subject,
                players,
                filter,
                trigger,
            } => {
                if matches!(
                    trigger,
                    TemporaryExileTrigger::Dies | TemporaryExileTrigger::LeavesBattlefield
                ) && subject.is_none()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let mut fam =
                    players.validate(context, self.target, facts.definitely_exiled_target)?;
                if let Some(subject) = subject {
                    subject
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                    if matches!(
                        subject.as_ref(),
                        EffectSubject::TargetObject
                            | EffectSubject::TargetAndSameNameMatching(_)
                            | EffectSubject::MatchingOtherThanTarget(_)
                            | EffectSubject::TargetAndSharingColorMatching(_)
                    ) {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                    }
                }
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                fam
            }
            TemporaryRuleKind::SpellCostModifier {
                players,
                filter,
                amount,
                ..
            } => {
                if matches!(amount.as_ref(), ValueExpr::Literal(v) if !v.is_positive()) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                amount.validate_amount(defined_slots, context, facts, vocabulary)?;
                let fam = players.validate(context, self.target, facts.definitely_exiled_target)?;
                toolbox_merge_family(fam, amount.required_operand_family(self.target)?)?
            }
            TemporaryRuleKind::AdditionalLandPlay { player, count } => {
                if *count == 0 {
                    return Err(DefinitionError::InvalidInstruction);
                }
                player.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::FilteredFlash { player, filter, .. } => {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                player.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::ZoneCastPermission { player, filter, .. } => {
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                player.validate(context, self.target, facts.definitely_exiled_target)?
            }
            TemporaryRuleKind::TemporaryStatic {
                ability,
                condition,
                one_shot,
            } => {
                if *one_shot
                    && !matches!(
                        ability.as_ref(),
                        crate::characteristics::StaticContinuousAbility::EventReplacement { .. }
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if let Some(condition) = condition {
                    crate::characteristics::validate_static_source_condition(condition, vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                ability
                    .validate(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                use crate::characteristics::{
                    CastPermissionRule, LibraryTopPermission, SpellCostModifier, SpellDiscount,
                    StaticContinuousAbility, StaticDamageRecipient, StaticDamageRule,
                    StaticDrawRule, StaticModifierRule, StaticRestriction, StaticUntapRule,
                };
                let selector_allowed = |selector: &crate::characteristics::PermanentSelector| {
                    context.has_source()
                        || (!selector.attached_to_source
                            && !selector.exclude_source
                            && !matches!(
                                selector.controller,
                                crate::characteristics::ControllerSelector::SourceItself
                                    | crate::characteristics::ControllerSelector::SelfAtMaxSpeed
                                    | crate::characteristics::ControllerSelector::SourcePair
                            ))
                };
                let allowed = match ability.as_ref() {
                    StaticContinuousAbility::DamageRule { rule, .. } => match rule {
                        StaticDamageRule::PreventNoncombat
                        | StaticDamageRule::PreventHalfIncoming
                        | StaticDamageRule::DoubleToOpponents
                        | StaticDamageRule::ReplaceSubtypeCombatWithMill(_)
                        | StaticDamageRule::LethalUsesPower
                        | StaticDamageRule::DamageCannotBePrevented
                        | StaticDamageRule::MinimumLifeTotal { .. } => true,
                        StaticDamageRule::ModifyDamage { recipient, .. }
                        | StaticDamageRule::ConditionalModifyDamage { recipient, .. } => {
                            context.has_source()
                                || !matches!(
                                    recipient,
                                    StaticDamageRecipient::SelfObject
                                        | StaticDamageRecipient::AttachedHost
                                )
                        }
                        StaticDamageRule::RedirectDamage { from, to, .. } => {
                            context.has_source()
                                || (!matches!(
                                    from,
                                    StaticDamageRecipient::SelfObject
                                        | StaticDamageRecipient::AttachedHost
                                ) && !matches!(
                                    to,
                                    crate::characteristics::DamageRedirectTarget::AbilitySource
                                        | crate::characteristics::DamageRedirectTarget::AttachedHost
                                ))
                        }
                        _ => false,
                    },
                    StaticContinuousAbility::DrawRule { rule, .. } => {
                        !matches!(rule, StaticDrawRule::EmptyDrawPutCounters { .. })
                    }
                    StaticContinuousAbility::UntapRule { rule, .. } => match rule {
                        StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(selector) => {
                            selector_allowed(selector)
                        }
                        _ => false,
                    },
                    StaticContinuousAbility::Restriction { rule, .. } => !matches!(
                        rule,
                        StaticRestriction::EnchantedPlayerCastLimit
                            | StaticRestriction::FlashSacrificeAtCleanup
                    ),
                    StaticContinuousAbility::Modifier { rule, .. } => matches!(
                        rule,
                        StaticModifierRule::DoubleCreatedTokens
                            | StaticModifierRule::DoublePlacedCounters
                            | StaticModifierRule::OthersEnterWithCounters { .. }
                            | StaticModifierRule::OthersEnterWithCountersExpr { .. }
                            | StaticModifierRule::ModifyCounters { .. }
                            | StaticModifierRule::ModifyTokens { .. }
                            | StaticModifierRule::ModifyTokenSubtype { .. }
                            | StaticModifierRule::ModifyTokensFiltered { .. }
                            | StaticModifierRule::StealCreatedTokens { .. }
                            | StaticModifierRule::CrewAndSaddlePowerBonus { .. }
                            | StaticModifierRule::ModifyMaxHandSize { .. }
                            | StaticModifierRule::ConvertUnspentMana { .. }
                            | StaticModifierRule::BonusTokenOnCreate { .. }
                            | StaticModifierRule::HalveOpponentCounters
                            | StaticModifierRule::ModifyLifeGain { .. }
                            | StaticModifierRule::ConditionalModifyLifeGain { .. }
                            | StaticModifierRule::IgnoreLegendRule
                            | StaticModifierRule::IgnoreLegendRuleForController
                            | StaticModifierRule::IgnoreLegendForSubtype(_)
                            | StaticModifierRule::RetainMana(_)
                            | StaticModifierRule::TwiceLoyaltyPerTurn
                            | StaticModifierRule::AdditionalCreatureDeathTrigger
                    ),
                    StaticContinuousAbility::CostModifier { modifier, .. } => matches!(
                        modifier,
                        SpellCostModifier::Discount {
                            amount: SpellDiscount::Fixed(_)
                                | SpellDiscount::PerExperienceCounter(_)
                                | SpellDiscount::ControlledCreaturesWithPlusOneCounters
                                | SpellDiscount::Dynamic(_),
                            ..
                        } | SpellCostModifier::IncreaseExceptType { .. }
                            | SpellCostModifier::Increase { .. }
                            | SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { .. }
                            | SpellCostModifier::FreeSubtypeCreatureSpells(_)
                            | SpellCostModifier::FreeColorlessHandSpellOnce
                            | SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce
                            | SpellCostModifier::NonartifactSpellsHaveImprovise
                            | SpellCostModifier::HighManaValueProtected { .. }
                            | SpellCostModifier::FirstSpellRule { .. }
                            | SpellCostModifier::AlternativeForMatching { .. }
                    ),
                    StaticContinuousAbility::CastPermission { permission, .. } => !matches!(
                        permission,
                        CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::CreatureOfSourceEntryType
                        )
                    ),
                    StaticContinuousAbility::Battlefield { selector, .. } => {
                        selector_allowed(selector)
                    }
                    StaticContinuousAbility::EventReplacement { .. } => true,
                    StaticContinuousAbility::CharacteristicDefining { .. }
                    | StaticContinuousAbility::CommandZoneActive { .. } => false,
                };
                if !allowed {
                    return Err(DefinitionError::InvalidInstruction);
                }
                None
            }
            TemporaryRuleKind::PreventDamageShield {
                amount,
                source,
                source_chosen_color,
                source_chosen_type,
                source_is_target,
                recipient,
                ..
            } => {
                if (*source_chosen_color || *source_chosen_type) && !context.has_source() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let mut fam = None;
                if let Some(amt) = amount {
                    if matches!(amt.as_ref(), ValueExpr::Literal(v) if !v.is_positive()) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    amt.validate_amount(defined_slots, context, facts, vocabulary)?;
                    fam = toolbox_merge_family(fam, amt.required_operand_family(self.target)?)?;
                }
                if let Some(f) = source {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if let crate::characteristics::StaticDamageRecipient::MatchingPermanent(f) =
                    recipient
                {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if matches!(
                    recipient,
                    crate::characteristics::StaticDamageRecipient::SelfObject
                        | crate::characteristics::StaticDamageRecipient::AttachedHost
                ) && !context.has_source()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if *source_is_target {
                    fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                }
                match recipient {
                    crate::characteristics::StaticDamageRecipient::TargetObject => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                    }
                    crate::characteristics::StaticDamageRecipient::TargetAny => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::DamageRecipient))?;
                    }
                    _ => {}
                }
                fam
            }
            TemporaryRuleKind::RedirectNextDamage {
                source,
                source_is_target,
                from,
                to,
                amount,
                ..
            } => {
                let mut fam = None;
                if let Some(amt) = amount {
                    if matches!(amt.as_ref(), ValueExpr::Literal(v) if !v.is_positive()) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    amt.validate_amount(defined_slots, context, facts, vocabulary)?;
                    fam = toolbox_merge_family(fam, amt.required_operand_family(self.target)?)?;
                }
                if let Some(f) = source {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if let crate::characteristics::StaticDamageRecipient::MatchingPermanent(f) = from {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if matches!(
                    from,
                    crate::characteristics::StaticDamageRecipient::SelfObject
                        | crate::characteristics::StaticDamageRecipient::AttachedHost
                ) && !context.has_source()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if matches!(
                    to,
                    crate::characteristics::DamageRedirectTarget::AbilitySource
                        | crate::characteristics::DamageRedirectTarget::AttachedHost
                ) && !context.has_source()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if *source_is_target {
                    fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                }
                match from {
                    crate::characteristics::StaticDamageRecipient::TargetObject => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                    }
                    crate::characteristics::StaticDamageRecipient::TargetAny => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::DamageRecipient))?;
                    }
                    _ => {}
                }
                match to {
                    crate::characteristics::DamageRedirectTarget::TargetObject
                    | crate::characteristics::DamageRedirectTarget::SecondTargetObject => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::Permanent))?;
                    }
                    crate::characteristics::DamageRedirectTarget::TargetAny => {
                        fam = toolbox_merge_family(fam, Some(OperandFamily::DamageRecipient))?;
                    }
                    _ => {}
                }
                fam
            }
        };
        Ok(fam)
    }

    fn validate_prevent_damage_spec(
        &self,
        spec: &crate::damage::TurnPreventionSpec,
        context: BodyContext<'_>,
    ) -> Result<Option<OperandFamily>, DefinitionError> {
        let fam = {
            use crate::damage::{PreventionRecipientSpec, PreventionSourceSpec};
            let needs_source =
                matches!(
                    spec.source,
                    PreventionSourceSpec::AbilitySource
                        | PreventionSourceSpec::CreaturesOtherThanAbilitySource
                ) || matches!(spec.recipient, PreventionRecipientSpec::AbilitySource);
            if needs_source && !context.has_source() {
                return Err(DefinitionError::InvalidInstruction);
            }
            let source_family = match spec.source {
                PreventionSourceSpec::TargetPermanent
                | PreventionSourceSpec::AllPermanentTargets
                | PreventionSourceSpec::CreaturesOtherThanTarget => Some(OperandFamily::Permanent),
                PreventionSourceSpec::TargetSpell => Some(OperandFamily::Spell),
                _ => None,
            };
            let recipient_family = match spec.recipient {
                PreventionRecipientSpec::Target => match self.target {
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    } => Some(OperandFamily::Permanent),
                    _ => Some(OperandFamily::DamageRecipient),
                },
                _ => None,
            };
            match (source_family, recipient_family) {
                (Some(_), Some(_)) => return Err(DefinitionError::InvalidInstruction),
                (Some(f), None) | (None, Some(f)) => Some(f),
                (None, None) => None,
            }
        };
        Ok(fam)
    }

    fn validate_destination(
        from: usize,
        destination: usize,
        length: usize,
    ) -> Result<(), DefinitionError> {
        if destination <= from || destination > length {
            return Err(DefinitionError::InvalidControlFlow);
        }
        Ok(())
    }
}
/// Admitted card-type labels. Other real types and their mechanics remain open.
/// Executable shape and creature-family membership are independent properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CardType {
    Battle,
    /// A planeswalker permanent. Execution remains gated pending full loyalty rules.
    Planeswalker,
    Land,
    Artifact,
    Creature,
    Enchantment,
    Kindred,
    Instant,
    Sorcery,
}
impl CardType {
    /// Whether this admitted type is itself a permanent type (CR 110.4).
    pub const fn is_permanent(self) -> bool {
        matches!(
            self,
            Self::Battle
                | Self::Land
                | Self::Artifact
                | Self::Creature
                | Self::Enchantment
                | Self::Planeswalker
        )
    }
    /// Creature and Kindred share one source-qualified subtype family.
    pub const fn carries_creature_subtypes(self) -> bool {
        matches!(self, Self::Creature | Self::Kindred)
    }
}
/// CR 205.4a's independent supertype axis. Carrying a value does not qualify
/// its associated rules: World/legend checks, Snow costs and schemes have their
/// own admission and execution requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Supertype {
    Basic,
    Legendary,
    Ongoing,
    Snow,
    World,
}
impl Supertype {
    /// Stable printed supertype word.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Legendary => "Legendary",
            Self::Ongoing => "Ongoing",
            Self::Snow => "Snow",
            Self::World => "World",
        }
    }
    /// Parse an exact supertype label.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        match label {
            "Basic" => Some(Self::Basic),
            "Legendary" => Some(Self::Legendary),
            "Ongoing" => Some(Self::Ongoing),
            "Snow" => Some(Self::Snow),
            "World" => Some(Self::World),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentDefinition {
    /// Copiable self-replacement for the status of this permanent as it enters.
    pub entry_tap: Option<crate::land::EntryTap>,
    /// Copiable Aura or Equipment information.
    pub attachment: Option<crate::attachment::AttachmentKind>,
    pub printed_pt: Option<PrintedPowerToughness>,
    pub casting_cost: Option<ManaCost>,
}
/// Printed mode selection limits and the remaining modes in printed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellModes {
    pub minimum: u8,
    pub maximum: u8,
    /// Optional world condition that raises the mode maximum (e.g. "If you control a commander, you may choose both").
    pub conditional_maximum: Option<(Box<ResolutionCondition>, u8)>,
    pub allow_repeat: bool,
    pub distinct_targets: bool,
    pub uniqueness: ModalUniqueness,
    pub mode_costs: Vec<ManaCost>,
    pub escalate_mana: Option<ManaCost>,
    pub prefix: Option<Box<ExecutableBody>>,
    pub suffix: Option<Box<ExecutableBody>>,
    pub additional: Vec<ExecutableBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellDefinition {
    /// On a modal spell, `body` is the first printed mode and this field contains the rest.
    pub modes: Option<SpellModes>,
    pub cost: ManaCost,
    /// True when this spell has no printed mana cost (CR 118.6, unpayable except via alternative costs).
    pub uncastable_mana_cost: bool,
    pub body: ExecutableBody,
}
// ProgramKind carries SpellDefinition and PermanentDefinition inline as the top-level program payload.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramKind {
    Permanent(PermanentDefinition),
    Spell(SpellDefinition),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TriggeredAbilityId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviousTurnCastCondition {
    NoSpells,
    OnePlayerCastAtLeastTwo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRelation {
    Controller,
    Opponent,
    AnyParticipant,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub id: TriggeredAbilityId,
    pub condition: TriggerSpec,
    /// Partition actual matches within one publication, source and definition.
    pub grouping: TriggerGrouping,
    pub body: ExecutableBody,
}
/// Keyword abilities with individual instances that use the stack (CR 702).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatTriggerKeyword {
    Decayed,
    Mobilize(RulesInteger),
    Ingest,
    Afflict(RulesInteger),
    Dethrone,
    Melee,
    Renown(RulesInteger),
    Training,
    Prowess,
    BattleCry,
    Bushido(RulesInteger),
    Frenzy(RulesInteger),
    Poisonous(RulesInteger),
    Rampage(RulesInteger),
    Flanking,
}

impl CombatTriggerKeyword {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Mobilize(amount)
            | Self::Afflict(amount)
            | Self::Renown(amount)
            | Self::Poisonous(amount)
            | Self::Bushido(amount)
            | Self::Frenzy(amount)
            | Self::Rampage(amount) => amount >= &0.into(),
            _ => true,
        }
    }
    fn condition(&self) -> TriggerSpec {
        match self {
            Self::Prowess => {
                TriggerSpec::spell_cast(PlayerRelation::Controller, SpellFilter::noncreature())
            }
            Self::Afflict(_) => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BecomesBlocked {
                    capture_defender: true,
                },
            ),
            Self::Dethrone => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::HighestLifeDefender,
            ),
            Self::Decayed | Self::Mobilize(_) | Self::Melee | Self::BattleCry => {
                TriggerSpec::self_attacks()
            }
            Self::Renown(_) => TriggerSpec::self_deals_combat_damage_to_player()
                .only_if(TriggerCondition::UnrenownedSource),
            Self::Training => TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::WithStrongerCreature,
            ),
            Self::Bushido(_) => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlocksOrBecomesBlocked,
            ),
            Self::Ingest | Self::Poisonous(_) => TriggerSpec::self_deals_combat_damage_to_player(),
            Self::Frenzy(_) => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::Unblocked,
            ),
            Self::Rampage(_) => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BecomesBlocked {
                    capture_defender: false,
                },
            ),
            Self::Flanking => TriggerSpec::blocks_or_blocked(
                ObjectFilter::self_object(),
                BlockTriggerKind::BlockedByEach {
                    without_flanking_only: true,
                },
            ),
        }
    }
}
/// Each declared instance owns an independent stack trigger. Numbered fading
/// and vanishing also define the corresponding number of counters on entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterKeyword {
    Fading(u32),
    Vanishing(Option<u32>),
    VanishingLastCounter,
    SuspendUpkeep,
    SuspendLastCounter,
    Persist,
    Undying,
}
impl CounterKeyword {
    /// Construct one trigger. A vanishing instance uses both its upkeep and its last-counter trigger.
    pub fn ability(self, id: TriggeredAbilityId) -> TriggeredAbility {
        TriggeredAbility {
            id,
            condition: TriggerSpec::counter_keyword(self),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::resolve_counter_keyword(self),
                ))],
            ),
        }
    }
    pub(crate) fn death_return(self) -> bool {
        matches!(self, Self::Persist | Self::Undying)
    }
    pub(crate) fn eligible_at_death(self, source: &crate::occurrence::SupportedAppearance) -> bool {
        source
            .battlefield_counters
            .as_ref()
            .is_some_and(|counts| match self {
                Self::Persist => !counts.minus_one.is_positive(),
                Self::Undying => !counts.plus_one.is_positive(),
                _ => false,
            })
    }
}
/// Upkeep-triggered keyword abilities with an all-or-nothing mana payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpkeepKeyword {
    Echo,
    CumulativeUpkeep,
    /// Pay the printed cost for each other creature controlled on resolution.
    OtherCreaturesUpkeep,
}
impl UpkeepKeyword {
    /// Build one independently stackable upkeep ability with the printed base cost.
    pub fn ability(self, id: TriggeredAbilityId, cost: ManaCost) -> TriggeredAbility {
        TriggeredAbility {
            id,
            condition: TriggerSpec::upkeep_keyword(self),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PromptChoice(Box::new(
                        crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::Controller,
                            cost,
                            ValueExpr::Literal(0.into()),
                            2,
                            1,
                        ),
                    )),
                    Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Source,
                        crate::toolbox::ZoneDest::Graveyard {
                            semantics: crate::toolbox::GraveyardSemantics::Sacrifice,
                        },
                    ))),
                ],
            ),
        }
    }
}
impl TriggeredAbility {
    /// A separate CR 702.21a instance with a mana payment offered on resolution.
    pub fn ward(id: TriggeredAbilityId, cost: ManaCost) -> Self {
        Self::counter_unless_targeter_pays(
            id,
            TriggerSpec::new(TriggerEvent::Targeted {
                subject: ObjectFilter::self_object(),
                by_opponent: true,
                spell_only: false,
                instant_or_sorcery_only: false,
            }),
            cost,
        )
    }

    /// A separate counter-unless-paid trigger for each targeted controlled creature of a subtype.
    pub fn protect_controlled_subtype(
        id: TriggeredAbilityId,
        subtype: CreatureTypeId,
        cost: ManaCost,
    ) -> Self {
        Self::counter_unless_targeter_pays(
            id,
            TriggerSpec::new(TriggerEvent::Targeted {
                subject: ObjectFilter::creature().controlled().with_subtype(subtype),
                by_opponent: true,
                spell_only: false,
                instant_or_sorcery_only: false,
            }),
            cost,
        )
    }

    fn counter_unless_targeter_pays(
        id: TriggeredAbilityId,
        condition: TriggerSpec,
        cost: ManaCost,
    ) -> Self {
        Self {
            id,
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PromptChoice(Box::new(
                        crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::UniqueRelatedPlayer,
                            cost,
                            ValueExpr::Literal(0.into()),
                            2,
                            1,
                        ),
                    )),
                    Instruction::StackOp(Box::new(
                        crate::toolbox::StackOpSpec::counter_ward_source(),
                    )),
                ],
            ),
        }
    }

    /// Each instance of cascade triggers independently when its spell is actually cast.
    pub fn cascade(id: TriggeredAbilityId) -> Self {
        Self {
            id,
            condition: TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::StackOp(Box::new(crate::toolbox::StackOpSpec::cascade())),
                    Instruction::StackOp(Box::new(crate::toolbox::StackOpSpec::cascade_bottom())),
                ],
            ),
        }
    }
    /// Storm, gravestorm and the triggered half of a printed replicate cost.
    pub fn spell_copies(id: TriggeredAbilityId, kind: CastCopyKeyword) -> Self {
        Self {
            id,
            condition: TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::copy_on_cast(kind),
                ))],
            ),
        }
    }
    pub fn cast_copy_keyword(&self) -> Option<CastCopyKeyword> {
        let [Instruction::StackOp(spec)] = self.body.instructions.as_slice() else {
            return None;
        };
        let crate::toolbox::StackOpKind::CopyOnCast(kind) = &spec.op else {
            return None;
        };
        (self == &Self::spell_copies(self.id, *kind)).then_some(*kind)
    }
    /// CR 702.86 captures the attacked player independently of later combat changes.
    pub fn annihilator(id: TriggeredAbilityId, amount: RulesInteger) -> Self {
        Self {
            id,
            condition: TriggerSpec::attacks(
                ObjectFilter::self_object(),
                AttackTriggerMode::Individual {
                    capture_defender: true,
                },
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::sacrifice(
                        AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                        ValueExpr::Literal(amount),
                        PermanentTypeFilter {
                            any_types: Default::default(),
                            required_types: BTreeSet::new(),
                            forbidden_types: BTreeSet::new(),
                            object_filter: None,
                        },
                    ),
                ))],
            ),
        }
    }
    /// CR 702.101. Each instance offers its controller a separate white-or-black payment.
    pub fn extort(id: TriggeredAbilityId) -> Self {
        Self {
            id,
            condition: TriggerSpec::spell_cast(PlayerRelation::Controller, SpellFilter::any()),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::PromptChoice(Box::new(
                        crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::Controller,
                            ManaCost::generic(1.into()),
                            ValueExpr::Literal(0.into()),
                            1,
                            2,
                        ),
                    )),
                    Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::extort_drain(),
                    )),
                ],
            ),
        }
    }

    /// Construct one instance with its own definition ID and captured trigger source.
    pub fn combat_keyword(id: TriggeredAbilityId, keyword: CombatTriggerKeyword) -> Self {
        Self {
            id,
            condition: keyword.condition(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::resolve_keyword_trigger(keyword),
                ))],
            ),
        }
    }

    /// Identify a printed keyword only when the entire executable definition matches it.
    pub fn as_combat_keyword(&self) -> Option<&CombatTriggerKeyword> {
        let [Instruction::MutateObject(spec)] = self.body.instructions.as_slice() else {
            return None;
        };
        let crate::toolbox::ObjectMutation::ResolveKeywordTrigger(keyword) = &spec.op else {
            return None;
        };
        (self.condition == keyword.condition()
            && self.grouping == TriggerGrouping::EachOccurrence
            && self.body.target == BodyTarget::None)
            .then_some(keyword)
    }

    fn exalted_instruction() -> Instruction {
        Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec::resolved_parts(
                crate::toolbox::ObjectQuery::TriggerSubject,
                ResolvedPartsSpec(vec![crate::characteristics::ResolvedPartSpec::ModifyPT(
                    PowerToughness {
                        power: 1.into(),
                        toughness: 1.into(),
                    },
                )]),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ))
    }

    /// Whether this definition has the exact rules text of exalted.
    pub fn is_exalted(&self) -> bool {
        self.condition == TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::Alone)
            && self.grouping == TriggerGrouping::EachOccurrence
            && self.body.target == BodyTarget::None
            && self.body.instructions.as_slice() == [Self::exalted_instruction()]
    }
    /// Construct one independent instance of exalted (CR 702.83).
    pub fn exalted(id: TriggeredAbilityId) -> Self {
        Self {
            id,
            condition: TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::Alone),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![Self::exalted_instruction()]),
        }
    }
}
/// Logical trigger-event boundaries within one actual publication (CR 700.1).
/// History restrictions and additional-trigger modifiers are separate mechanisms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerGrouping {
    /// One instance for each matching occurrence.
    EachOccurrence,
    /// One instance owning every match in this publication.
    OneOrMoreTogether,
    OneOrMoreOnceEachTurn,
    /// One instance for each actual participant captured by the condition.
    OneOrMoreByPlayer,
}
/// Class of spells that may receive the printed text of a splice card (CR 702.47).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceQuality {
    Arcane,
    InstantOrSorcery,
}

/// Splice currently admits mana prices and straight-line spell effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpliceAbility {
    pub quality: SpliceQuality,
    pub cost: ManaCost,
}

/// Instant and sorcery subtype supported by the authored characteristics model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpellSubtype {
    Adventure,
    /// Arcane (CR 205.3k).
    Arcane,
    /// Lesson (CR 205.3k).
    Lesson,
    /// Trap (CR 205.3k).
    Trap,
    /// Omen (CR 205.3k).
    Omen,
}
/// Artifact subtypes represented in the printed and evaluated characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactSubtype {
    Attraction,
    Blood,
    Bobblehead,
    Book,
    Clue,
    Contraption,
    Equipment,
    Food,
    Fortification,
    Gold,
    Incubator,
    Junk,
    Lander,
    Map,
    Mutagen,
    Powerstone,
    Shard,
    Spacecraft,
    Treasure,
    Vehicle,
}
impl ArtifactSubtype {
    /// Exact printed subtype word.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Attraction => "Attraction",
            Self::Blood => "Blood",
            Self::Bobblehead => "Bobblehead",
            Self::Book => "Book",
            Self::Clue => "Clue",
            Self::Contraption => "Contraption",
            Self::Equipment => "Equipment",
            Self::Food => "Food",
            Self::Fortification => "Fortification",
            Self::Gold => "Gold",
            Self::Incubator => "Incubator",
            Self::Junk => "Junk",
            Self::Lander => "Lander",
            Self::Map => "Map",
            Self::Mutagen => "Mutagen",
            Self::Powerstone => "Powerstone",
            Self::Shard => "Shard",
            Self::Spacecraft => "Spacecraft",
            Self::Treasure => "Treasure",
            Self::Vehicle => "Vehicle",
        }
    }
    /// Resolve an exact supported subtype.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        [
            Self::Attraction,
            Self::Blood,
            Self::Bobblehead,
            Self::Book,
            Self::Clue,
            Self::Contraption,
            Self::Equipment,
            Self::Food,
            Self::Fortification,
            Self::Gold,
            Self::Incubator,
            Self::Junk,
            Self::Lander,
            Self::Map,
            Self::Mutagen,
            Self::Powerstone,
            Self::Shard,
            Self::Spacecraft,
            Self::Treasure,
            Self::Vehicle,
        ]
        .into_iter()
        .find(|kind| kind.wire_label() == label)
    }
}
/// Enchantment subtype supported by the current characteristic model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnchantmentSubtype {
    Background,
    Cartouche,
    Case,
    Class,
    Curse,
    Role,
    Room,
    Rune,
    /// Saga (CR 205.3h).
    Saga,
    Shard,
    Shrine,
}
impl EnchantmentSubtype {
    /// Stable subtype label for authored definitions.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Cartouche => "Cartouche",
            Self::Case => "Case",
            Self::Class => "Class",
            Self::Curse => "Curse",
            Self::Role => "Role",
            Self::Room => "Room",
            Self::Rune => "Rune",
            Self::Saga => "Saga",
            Self::Shard => "Shard",
            Self::Shrine => "Shrine",
        }
    }
    /// Parse the supported exact printed subtype label.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        match label {
            "Background" => Some(Self::Background),
            "Cartouche" => Some(Self::Cartouche),
            "Case" => Some(Self::Case),
            "Class" => Some(Self::Class),
            "Curse" => Some(Self::Curse),
            "Role" => Some(Self::Role),
            "Room" => Some(Self::Room),
            "Rune" => Some(Self::Rune),
            "Saga" => Some(Self::Saga),
            "Shard" => Some(Self::Shard),
            "Shrine" => Some(Self::Shrine),
            _ => None,
        }
    }
}
impl SpellSubtype {
    /// Stable printed label used by external definitions and projections.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Adventure => "Adventure",
            Self::Arcane => "Arcane",
            Self::Lesson => "Lesson",
            Self::Trap => "Trap",
            Self::Omen => "Omen",
        }
    }
    /// Parse an exact supported subtype label.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        match label {
            "Adventure" => Some(Self::Adventure),
            "Arcane" => Some(Self::Arcane),
            "Lesson" => Some(Self::Lesson),
            "Trap" => Some(Self::Trap),
            "Omen" => Some(Self::Omen),
            _ => None,
        }
    }
}

// CR 205.3j. The same closed vocabulary is used for printed types and resolution choices.
macro_rules! planeswalker_types {
    ($($kind:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum PlaneswalkerSubtype { $($kind),+ }
        impl PlaneswalkerSubtype {
            pub const ALL: &'static [Self] = &[$(Self::$kind),+];
            pub const fn label(self) -> &'static str { match self { $(Self::$kind => stringify!($kind)),+ } }
            pub fn from_label(value:&str) -> Option<Self> { match value { $(stringify!($kind)=>Some(Self::$kind)),+, _=>None } }
        }
    };
}
planeswalker_types!(
    Ajani,
    Aminatou,
    Angrath,
    Arlinn,
    Arzakon,
    Ashiok,
    Bahamut,
    Basri,
    Bolas,
    Calix,
    Chandra,
    Comet,
    Dack,
    Dakkon,
    Daretti,
    Davriel,
    Dellian,
    Dihada,
    Domri,
    Dovin,
    Dyfed,
    Ellywick,
    Elminster,
    Elspeth,
    Estrid,
    Feroz,
    Freyalise,
    Garruk,
    Gideon,
    Greensleeves,
    Grist,
    Guff,
    Huatli,
    Jace,
    Jared,
    Jaya,
    Jeska,
    Kaito,
    Karn,
    Kasmina,
    Kaya,
    Kiora,
    Koth,
    Liliana,
    Lolth,
    Lukka,
    Minsc,
    Mordenkainen,
    Nahiri,
    Narset,
    Niko,
    Nissa,
    Nixilis,
    Oko,
    Quintorius,
    Ral,
    Rowan,
    Saheeli,
    Samut,
    Sarkhan,
    Serra,
    Sifa,
    Sivitri,
    Sorin,
    Szat,
    Tamiyo,
    Tasha,
    Teferi,
    Teyo,
    Tezzeret,
    Thomil,
    Tibalt,
    Tyvar,
    Ugin,
    Urza,
    Venser,
    Vivien,
    Vraska,
    Vronos,
    Will,
    Windgrace,
    Worzel,
    Wrenn,
    Xenagos,
    Yanggu,
    Yanling,
    Zariel,
);
/// Conditional self-flash permission printed on a spell or permanent card (CR 601.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfFlashPermission {
    /// May be cast as though it had flash while `condition` is satisfied.
    Condition(Box<ResolutionCondition>),
    /// May be cast as though it had flash if the caster pays this additional mana cost (CR 601.2f).
    PayExtraMana(ManaCost),
}

/// Authored partial definition. Only validated definitions enter EffectWindow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub splice: Option<SpliceAbility>,
    /// Whether this spell has a printed ability preventing it from being countered.
    pub cannot_be_countered: bool,
    /// Whether this spell has a printed ability preventing it from being copied (CR 707.10).
    pub cannot_be_copied: bool,
    /// Optional self-flash permission (world condition or additional mana cost to cast at instant timing).
    pub conditional_flash: Option<SelfFlashPermission>,
    /// Number of {X} symbols in the printed mana cost, each sharing the announced value.
    pub x_mana_symbols: u8,
    /// Hybrid and Phyrexian symbols in printed order. Their first colors are in the base price.
    pub hybrid_symbols: Vec<crate::mana::HybridSymbol>,
    pub attack_requirement: Option<AttackRequirement>,
    /// Additional printed attack requirements beyond the primary `attack_requirement` slot.
    pub additional_attack_requirements: Vec<AttackRequirement>,
    pub entry_copy: Option<EntryCopyRule>,
    /// Optional world condition under which this permanent enters untapped; enters tapped when false (CR 614.1c).
    pub entry_tap_unless: Option<Box<ResolutionCondition>>,
    /// Pregame action allowing this permanent to begin the game on the battlefield from an opening hand (CR 103.6).
    pub opening_hand_battlefield: bool,
    /// Limits contributed while these printed abilities are active.
    pub combat_limits: CombatDeclarationLimits,
    /// This spell can be cast only during combat on its controller’s turn.
    pub cast_during_own_combat: bool,
    /// Printed cast-timing or world-condition restriction for this card.
    pub cast_timing: Option<CastTimingRestriction>,
    /// Mandatory life payment on casting. Fixed and announced X prices are admitted.
    pub additional_life_cost: Option<ValueExpr>,
    /// Physical reverse face of a transforming double-faced card.
    pub nightbound_back: Option<Box<Program>>,
    /// Physical reverse face of a transforming card without daybound.
    pub transform_back: Option<Box<Program>>,
    /// Independent back face of a modal double-faced card (CR 712.1b).
    pub modal_back: Option<Box<Program>>,
    /// Flipped half of a Kamigawa flip card (CR 710).
    pub flip_back: Option<Box<Program>>,
    /// Alternative prototyped characteristics (CR 702.160).
    pub prototype: Option<Box<Program>>,
    /// Independent alternative characteristics used when casting an Adventure.
    pub adventure: Option<Box<Program>>,
    /// Prepared spell face whose copy may be cast while this permanent is prepared.
    pub prepare_spell: Option<Box<Program>>,
    /// Whether this permanent enters the battlefield prepared.
    pub enters_prepared: bool,
    /// Printed halves of a split instant or sorcery, combined outside the stack.
    pub split: Option<Box<SplitCard>>,
    /// Printed or copied keyword abilities before continuous effects are applied.
    pub printed_keywords: BTreeSet<KeywordAbility>,
    /// Printed permissions for face-down casting and special-action costs.
    pub face_up_abilities: Vec<crate::face_down::FaceUpAbility>,
    /// Restrictions keyed by the printed activated ability instance.
    pub activation_restrictions:
        BTreeMap<crate::activated::ActivatedAbilityId, crate::activated::ActivationRestriction>,
    /// Modal selection specs keyed by the printed activated ability instance.
    pub activation_modes: BTreeMap<crate::activated::ActivatedAbilityId, SpellModes>,
    /// Hybrid and Phyrexian symbols in printed order for activated abilities.
    pub activation_hybrid_symbols:
        BTreeMap<crate::activated::ActivatedAbilityId, Vec<crate::mana::HybridSymbol>>,
    /// Additional composable non-mana activation costs keyed by activated ability identity.
    pub extra_activation_costs:
        BTreeMap<crate::activated::ActivatedAbilityId, Vec<crate::activated::ExtraActivationCost>>,
    /// World-evaluable generic mana cost reductions keyed by activated ability identity.
    pub activation_cost_reductions: BTreeMap<crate::activated::ActivatedAbilityId, ValueExpr>,
    /// Generic mana cost reduction when activating an ability targeting a permanent matching `ObjectFilter`.
    pub activation_target_cost_reductions:
        BTreeMap<crate::activated::ActivatedAbilityId, (ObjectFilter, u8)>,
    /// Optional extra side effects (exile counters, follow-up body) keyed by zone replacement identity.
    pub zone_replacement_extras: BTreeMap<ZoneReplacementAbilityId, ZoneReplacementExtra>,
    /// Additional composable non-mana mandatory casting costs.
    pub extra_cast_costs: Vec<crate::activated::ExtraActivationCost>,
    /// Signed loyalty costs keyed by activated ability; represented but admission is gated.
    pub loyalty_costs: BTreeMap<crate::activated::ActivatedAbilityId, i32>,
    /// Loyalty abilities whose printed cost is −X, keyed by ability identity.
    pub variable_loyalty_costs: BTreeSet<crate::activated::ActivatedAbilityId>,
    /// Sum of printed toxic instances. A printed toxic 0 remains distinguishable from absence.
    pub printed_toxic: Option<RulesInteger>,
    /// Copiable printed starting loyalty, distinct from physical loyalty counters.
    pub printed_loyalty: Option<RulesInteger>,
    /// Defense printed on a battle card (CR 310.4).
    pub printed_defense: Option<RulesInteger>,
    /// Physical reverse face of a double-faced Siege.
    pub siege_back: Option<Box<Program>>,
    /// Graveyard permission and reverse face of a transforming double-faced card (CR 702.146).
    pub disturb: Option<Box<Disturb>>,
    /// Printed name of the other physical component of this meld pair.
    pub meld_partner: Option<spellwake_contracts::names::EnglishCardName>,

    /// Static casting reductions in copiable rules text. Repeated instances stack.
    pub cast_reductions: Vec<CastReduction>,
    /// Entry-counter abilities in printed order, including repeated instances.
    pub entry_counter_abilities: Vec<EntryCounterAbility>,
    /// Mana price for plotting this card from hand (CR 702.170).
    pub plot: Option<ManaCost>,
    /// Alternative mana cost offered by a discard replaced by madness.
    pub madness: Option<ManaCost>,
    pub miracle: Option<MiracleCost>,
    /// This exact face can be cast from its owner’s face-up exile.
    pub cast_from_own_exile: bool,
    pub exile_action: Option<ExileAction>,
    /// Mana component of the flashback alternative cost (CR 702.34).
    pub flashback: Option<ManaCost>,
    pub flashback_x_mana_symbols: u8,
    /// Number of controlled creatures sacrificed as part of a flashback cost.
    pub flashback_sacrifice_creatures: u8,
    /// Count of untapped white creatures required as the printed flashback price.
    pub flashback_tap_white_creatures: u8,
    /// Life paid only when casting using this flashback cost.
    pub flashback_life: u16,
    /// This spell requires discarding one other card as an additional cost.
    pub discard_cast_cost: bool,
    /// Optional filter restricting the card discarded for `discard_cast_cost`.
    pub discard_cast_cost_filter: Option<ObjectFilter>,
    /// Creature cards the caster must exile from their graveyard to cast this spell.
    pub graveyard_creature_exile_cost: u8,
    /// Exile X cards from the casting player's graveyard as an additional cost.
    pub graveyard_exile_x_cost: bool,
    /// Printed permission for a creature to be cast from its owner's graveyard.
    pub graveyard_self_cast: Option<GraveyardSelfCast>,
    /// Casting this spell requires sacrificing one permanent (creature by default) the caster controls.
    pub sacrifice_creature_cast_cost: bool,
    /// Optional filter restricting the permanent sacrificed for `sacrifice_creature_cast_cost`.
    pub sacrifice_cast_cost_filter: Option<ObjectFilter>,
    /// The caster may tap any number of untapped creatures they control as an additional cost.
    pub tap_creatures_cast_cost: bool,
    /// Reveal a colorless creature card in hand as an additional casting cost.
    pub reveal_colorless_creature_cast_cost: bool,
    /// Printed alternative price which depends on an event earlier this turn.
    pub conditional_cost: Option<ConditionalCastCost>,
    /// Optional mana costs on an instruction spell.
    pub additional_cast_costs: Vec<AdditionalCastCost>,
    /// Mandatory additional mana cost paid for each chosen target beyond the first (Strive / Fireball).
    pub per_extra_target_cost: Option<ManaCost>,
    /// Copiable battlefield replacement abilities; admitted action scope is explicit.
    pub zone_replacements: Vec<ZoneReplacementAbility>,
    /// Authored copiable types, independent of the executable body shape.
    pub card_types: BTreeSet<CardType>,
    /// The shared Creature/Kindred family; IDs belong to the game vocabulary.
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    /// Authored land family, independent of the Basic supertype and names.
    pub land_subtypes: BTreeSet<LandType>,
    /// Authored instant/sorcery subtypes, independent of card type.
    pub spell_subtypes: BTreeSet<SpellSubtype>,
    /// Copiable artifact subtypes.
    pub artifact_subtypes: BTreeSet<ArtifactSubtype>,
    /// Copiable enchantment subtypes.
    pub enchantment_subtypes: BTreeSet<EnchantmentSubtype>,
    /// Copiable planeswalker subtype family, independent of names.
    pub planeswalker_subtypes: BTreeSet<PlaneswalkerSubtype>,
    pub names: spellwake_contracts::names::ObjectNames,
    /// Printed copiable supertypes, independent of kind, subtype and name.
    pub supertypes: BTreeSet<Supertype>,
    pub kind: ProgramKind,
    /// Printed color indicator, when present. Engine admission requires at least
    /// one color. Without an indicator ordinary colors come from the mana cost.
    pub color_indicator: Option<BTreeSet<Color>>,
    /// Colors defined by the creating effect (CR 111.3), independently of an
    /// indicator or mana cost. An empty set explicitly defines colorless.
    pub effect_defined_colors: Option<BTreeSet<Color>>,
    pub mana: Vec<ManaAbility>,
    pub activated: Vec<ActivatedAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub continuous: Option<StaticContinuousAbility>,
    /// Further printed battlefield abilities, each with an independent identity and selector.
    pub additional_battlefield_abilities: Vec<StaticContinuousAbility>,
    /// Optional source-condition gates keyed by static ability identity (Class levels, Solved Cases, "As long as ...").
    pub static_conditions: BTreeMap<
        crate::characteristics::StaticAbilityId,
        crate::characteristics::StaticSourceCondition,
    >,
    /// CR 702.174 Gift: optional additional cost promising a gift to an opponent.
    pub gift: Option<GiftKind>,
    /// CR 702.153 Casualty N: optional additional cost sacrificing a creature with power >= N to copy this spell.
    pub casualty: Option<u8>,
    /// CR 702.78 Conspire: optional additional cost tapping 2 untapped creatures sharing a color to copy this spell.
    pub conspire: bool,
    /// CR 702.52 Dredge N: replace a draw while in graveyard by milling N cards and returning this card to hand.
    pub dredge: Option<u8>,
    /// CR 702.132 Assist: another player may pay part of this spell's generic cost.
    pub assist: bool,
    /// CR 702.99 Cipher: exile this spell encoded on a creature you control as it resolves.
    pub cipher: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionError {
    /// Planeswalker execution is gated until loyalty activation and combat are supported.
    UnsupportedPlaneswalker,
    /// A replacement predicate requires a movement adapter outside this slice.
    UnsupportedZoneReplacementScope,
    /// A replacement has an empty action domain or no destination transformation.
    InvalidZoneReplacementDefinition,
    TokenDefinition,
    ColorIndicator,
    CardTypes,
    CastingCost,
    ManaSource,
    ActivatedSource,
    TriggeredSource,
    DuplicateAbility,
    EmptyBody,
    InvalidInstruction,
    /// A control successor is not a strictly forward point within its body.
    InvalidControlFlow,
    /// A condition requires a captured trigger but its body has no such binding.
    UnboundTriggerCondition,
    /// UniqueRelatedPlayer is used outside a captured triggered-ability body.
    UnboundResolvingPlayer,
    /// The trigger condition/grouping does not guarantee one related participant.
    UnguaranteedRelatedPlayer,
    /// A targeted resolving role lacks its exact whole-body target capability:
    /// TargetSpellController requires single_spell; TargetPlayer requires SINGLE_PLAYER.
    ResolvingPlayerTarget,
    /// An optional wrapper has no following primitive instruction.
    MissingOptionalEffect,
    /// An optional wrapper attempts an unsupported control or selected-object effect.
    UnsupportedOptionalEffect,
    /// A resolution payment has an invalid authored mana cost.
    InvalidResolutionPaymentCost,
    OperandFamily,
    PhantomTarget,
    TargetlessActivation,
    InvalidCreatureSubtypes,
    InvalidLandSubtypes,
    ForeignCreatureType,
    DuplicateCreatureTypeSlot,
    UndefinedCreatureTypeSlot,
    /// A body without a trigger binding uses a trigger-dependent amount.
    UnboundTriggerAmount,
    EmptyCreatureTypeDomain,
    ContinuousEffect,
}
mod zone_replacement;
pub use zone_replacement::*;

impl Program {
    /// Whether this spell's complete copied effect uses a supported splice instruction.
    pub fn supports_splice_text(&self) -> bool {
        let ProgramKind::Spell(spell) = &self.kind else {
            return false;
        };
        spell.modes.is_none()
            && (matches!(
                spell.body.instructions.as_slice(),
                [Instruction::DealDamage(_)
                    | Instruction::DrawCards(_)
                    | Instruction::AdjustLife(_)
                    | Instruction::CreateTokens { .. }]
            ) || matches!(
                spell.body.instructions.as_slice(),
                [Instruction::MutatePlayer(spec)]
                    if matches!(
                        spec.op,
                        crate::toolbox::PlayerMutation::AddMana(EffectManaYield::Fixed(_))
                            | crate::toolbox::PlayerMutation::LibraryTop(
                                crate::toolbox::LibraryTopAction::Mill(_)
                            )
                    )
            ) || matches!(
                spell.body.instructions.as_slice(),
                [Instruction::ApplyContinuous(spec)]
                    if matches!(
                        spec.payload,
                        crate::toolbox::ContinuousPayload::ChosenColorProtection { .. }
                    )
            ) || matches!(
                spell.body.instructions.as_slice(),
                [Instruction::ZoneOp(spec)]
                    if matches!(spec.source, crate::toolbox::ObjectQuery::Targets)
                        && matches!(
                            spec.destination,
                            crate::toolbox::ZoneDest::Graveyard {
                                semantics: crate::toolbox::GraveyardSemantics::Destroy { .. }
                            }
                        )
            ))
    }

    /// The physical card's chosen color at admission, if it has a pregame color definition.
    pub fn commander_color_choice_mut(
        &mut self,
    ) -> Option<&mut Option<crate::characteristics::Color>> {
        let crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
            parts, ..
        } = self.continuous.as_mut()?
        else {
            return None;
        };
        parts.0.iter_mut().find_map(|part| match part {
            crate::characteristics::DefiningPart::CommanderChosenColor(choice) => Some(choice),
            _ => None,
        })
    }
    /// Reverse definition of a physical transforming double-faced card.
    pub fn reverse_face(&self) -> Option<&Program> {
        self.transform_back
            .as_deref()
            .or(self.nightbound_back.as_deref())
            .or(self.siege_back.as_deref())
            .or_else(|| self.disturb.as_deref().map(|disturb| &disturb.back))
    }
    /// Back face of any double-faced or flip card (transforming, modal, or Kamigawa flip).
    pub fn back_face(&self) -> Option<&Program> {
        self.reverse_face()
            .or(self.modal_back.as_deref())
            .or(self.flip_back.as_deref())
    }
    /// Printed mana cost of this face, including the distinction between an absent and a zero cost.
    pub fn printed_mana_cost(&self) -> Option<&ManaCost> {
        match &self.kind {
            ProgramKind::Spell(spell) => (!spell.uncastable_mana_cost).then_some(&spell.cost),
            ProgramKind::Permanent(permanent) => permanent.casting_cost.as_ref(),
        }
    }
    /// Mana value of the represented face in a hidden zone; an absent cost has value zero.
    pub fn mana_value(&self) -> RulesInteger {
        self.printed_mana_cost()
            .map(ManaCost::total)
            .unwrap_or_else(|| 0.into())
    }

    /// Construct authored data with explicit traits; the owning engine must call
    /// validate before admission. Invalid definitions remain representable for
    /// typed rejection tests and syntax-only qualification boundaries.
    pub fn new(
        card_types: BTreeSet<CardType>,
        creature_subtypes: BTreeSet<CreatureTypeId>,
        land_subtypes: BTreeSet<LandType>,
        kind: ProgramKind,
    ) -> Self {
        Self {
            cannot_be_countered: false,
            cannot_be_copied: false,
            conditional_flash: None,
            card_types,
            creature_subtypes,
            land_subtypes,
            spell_subtypes: BTreeSet::new(),
            artifact_subtypes: BTreeSet::new(),
            enchantment_subtypes: BTreeSet::new(),
            planeswalker_subtypes: BTreeSet::new(),
            splice: None,
            adventure: None,
            prepare_spell: None,
            enters_prepared: false,
            split: None,
            nightbound_back: None,
            transform_back: None,
            modal_back: None,
            flip_back: None,
            prototype: None,
            printed_keywords: BTreeSet::new(),
            face_up_abilities: Vec::new(),
            activation_restrictions: BTreeMap::new(),
            activation_modes: BTreeMap::new(),
            activation_hybrid_symbols: BTreeMap::new(),
            extra_activation_costs: BTreeMap::new(),
            activation_cost_reductions: BTreeMap::new(),
            activation_target_cost_reductions: BTreeMap::new(),
            zone_replacement_extras: BTreeMap::new(),
            extra_cast_costs: Vec::new(),
            loyalty_costs: BTreeMap::new(),
            variable_loyalty_costs: BTreeSet::new(),
            printed_toxic: None,
            x_mana_symbols: 0,
            hybrid_symbols: Vec::new(),
            attack_requirement: None,
            additional_attack_requirements: Vec::new(),
            entry_copy: None,
            entry_tap_unless: None,
            opening_hand_battlefield: false,
            combat_limits: Default::default(),
            cast_during_own_combat: false,
            cast_timing: None,
            additional_life_cost: None,
            siege_back: None,
            disturb: None,
            meld_partner: None,
            printed_defense: None,
            printed_loyalty: None,
            cast_reductions: Vec::new(),
            entry_counter_abilities: vec![],
            conditional_cost: None,
            plot: None,
            madness: None,
            miracle: None,
            cast_from_own_exile: false,
            exile_action: None,
            flashback: None,
            flashback_x_mana_symbols: 0,
            flashback_sacrifice_creatures: 0,
            flashback_tap_white_creatures: 0,
            flashback_life: 0,
            sacrifice_creature_cast_cost: false,
            sacrifice_cast_cost_filter: None,
            tap_creatures_cast_cost: false,
            reveal_colorless_creature_cast_cost: false,
            discard_cast_cost: false,
            discard_cast_cost_filter: None,
            graveyard_creature_exile_cost: 0,
            graveyard_exile_x_cost: false,
            graveyard_self_cast: None,
            additional_cast_costs: vec![],
            per_extra_target_cost: None,
            names: spellwake_contracts::names::ObjectNames::nameless(),
            supertypes: BTreeSet::new(),
            kind,
            color_indicator: None,
            effect_defined_colors: None,
            mana: vec![],
            zone_replacements: vec![],
            activated: vec![],
            triggered: vec![],
            continuous: None,
            additional_battlefield_abilities: Vec::new(),
            static_conditions: BTreeMap::new(),
            gift: None,
            casualty: None,
            conspire: false,
            dredge: None,
            assist: false,
            cipher: false,
        }
    }
    /// Add a fixed-price dash, warp or blitz alternative. For blitz the separate
    /// death ability is available while the permanent has its paid blitz ability.
    #[must_use]
    pub fn with_end_step_cast(
        mut self,
        kind: EndStepCast,
        mana: ManaCost,
        blitz_id: TriggeredAbilityId,
    ) -> Self {
        self.conditional_cost = Some(ConditionalCastCost::EndStep { kind, mana });
        if kind == EndStepCast::Blitz {
            self.triggered.push(TriggeredAbility {
                id: blitz_id,
                condition: TriggerSpec::new(TriggerEvent::Dies {
                    subject: ObjectFilter::self_object(),
                    mode: DiesTriggerMode::Blitz,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawCards(Box::new(
                        crate::toolbox::DrawCardsSpec::simple(
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            ValueExpr::Literal(1.into()),
                        ),
                    ))],
                ),
            });
        }
        self
    }
    /// Add the two abilities represented by a fixed-price evoke keyword (CR 702.74).
    #[must_use]
    pub fn with_evoke(mut self, cost: ManaCost, id: TriggeredAbilityId) -> Self {
        self.conditional_cost = Some(ConditionalCastCost::Evoke(cost));
        self.triggered.push(TriggeredAbility {
            id,
            condition: TriggerSpec::new(TriggerEvent::Enters {
                subject: ObjectFilter::self_object(),
                from_zone: TriggerFromZone::Any,
                cast_origin: Some(CastEntryKind::Evoked),
                attached_to_creature: false,
            }),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::sacrifice_evoked(),
                ))],
            ),
        });
        self
    }
    /// Set the copiable keyword abilities of this definition.
    #[must_use]
    pub fn with_printed_keywords(
        mut self,
        keywords: impl IntoIterator<Item = KeywordAbility>,
    ) -> Self {
        self.printed_keywords = keywords.into_iter().collect();
        self
    }

    pub fn creature(
        casting_cost: Option<ManaCost>,
        power: RulesInteger,
        toughness: RulesInteger,
        creature_subtypes: BTreeSet<CreatureTypeId>,
    ) -> Self {
        Self::new(
            BTreeSet::from([CardType::Creature]),
            creature_subtypes,
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: None,
                attachment: None,
                printed_pt: Some(PowerToughness { power, toughness }.into()),
                casting_cost,
            }),
        )
    }
    /// Single station symbol which grants the printed abilities, type and base P/T at its threshold.
    pub fn station_spacecraft(
        casting_cost: ManaCost,
        minimum: std::num::NonZeroU16,
        base: crate::characteristics::PowerToughness,
        grants: std::collections::BTreeSet<crate::characteristics::KeywordAbility>,
    ) -> Self {
        use crate::characteristics::{
            AbilityChange, NumericExpression, NumericPair, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts, StaticTypeChange, SubtypeOperation,
            TypeOperation,
        };
        let mut program = Self::artifact(
            Some(casting_cost),
            vec![crate::activated::ActivatedAbility::tap_for_power(
                crate::activated::ActivatedAbilityId(1),
                crate::activated::PowerAbility::Station,
                0,
            )],
        );
        let mut parts = vec![StaticPart::Type(StaticTypeChange {
            types: TypeOperation::Add([CardType::Creature].into()),
            supertypes: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
        })];
        if !grants.is_empty() {
            parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(grants)));
        }
        parts.push(StaticPart::SetPT(NumericPair {
            power: NumericExpression::Literal(base.power),
            toughness: NumericExpression::Literal(base.toughness),
        }));
        program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                station_at: Some(minimum),
                ..PermanentSelector::default()
            },
            parts: StaticParts(parts),
        });
        program
    }
    pub fn artifact(casting_cost: Option<ManaCost>, activated: Vec<ActivatedAbility>) -> Self {
        Self {
            activated,
            ..Self::new(
                BTreeSet::from([CardType::Artifact]),
                BTreeSet::new(),
                BTreeSet::new(),
                ProgramKind::Permanent(PermanentDefinition {
                    entry_tap: None,
                    attachment: None,
                    printed_pt: None,
                    casting_cost,
                }),
            )
        }
    }
    /// An Aura permanent spell with one explicit enchant restriction.
    pub fn aura(
        casting_cost: Option<ManaCost>,
        restriction: crate::attachment::EnchantRestriction,
    ) -> Self {
        let mut program = Self::enchantment(casting_cost);
        if let ProgramKind::Permanent(permanent) = &mut program.kind {
            permanent.attachment = Some(crate::attachment::AttachmentKind::Aura(restriction));
        }
        program
    }
    /// Equipment with a single sorcery-speed equip activation (ability ID 1).
    pub fn equipment(casting_cost: Option<ManaCost>, equip_cost: ManaCost) -> Self {
        Self::attachment_artifact(
            casting_cost,
            equip_cost,
            crate::attachment::AttachAbility::Equip,
        )
    }
    /// Fortification with one sorcery-speed fortify ability (ability ID 1).
    pub fn fortification(casting_cost: Option<ManaCost>, fortify_cost: ManaCost) -> Self {
        Self::attachment_artifact(
            casting_cost,
            fortify_cost,
            crate::attachment::AttachAbility::Fortify,
        )
    }
    fn attachment_artifact(
        casting_cost: Option<ManaCost>,
        ability_cost: ManaCost,
        attach: crate::attachment::AttachAbility,
    ) -> Self {
        let mut program = Self::artifact(
            casting_cost,
            vec![ActivatedAbility {
                id: crate::activated::ActivatedAbilityId(1),
                mana_cost: ability_cost,
                source_cost: crate::activated::SourceCost::None,
                body: ExecutableBody::new(
                    attach.target(),
                    vec![Instruction::MutateObject(Box::new(
                        crate::toolbox::MutateObjectSpec::attach_ability(attach),
                    ))],
                ),
            }],
        );
        if let ProgramKind::Permanent(permanent) = &mut program.kind {
            permanent.attachment = Some(attach.kind());
        }
        if attach.kind() == crate::attachment::AttachmentKind::Equipment {
            program.artifact_subtypes.insert(ArtifactSubtype::Equipment);
        }
        program
    }
    /// Artifact creature Equipment with both sorcery-speed Reconfigure abilities (CR 702.151).
    /// Ability 1 attaches to another creature and ability 2 unattaches this Equipment.
    pub fn reconfigure(
        casting_cost: Option<ManaCost>,
        power: RulesInteger,
        toughness: RulesInteger,
        subtypes: BTreeSet<CreatureTypeId>,
        cost: ManaCost,
    ) -> Self {
        use crate::{
            activated::{ActivatedAbilityId, SourceCost},
            attachment::{AttachAbility, AttachmentKind},
        };
        let mut program = Self::creature(casting_cost, power, toughness, subtypes);
        program.card_types.insert(CardType::Artifact);
        program.artifact_subtypes.insert(ArtifactSubtype::Equipment);
        if let ProgramKind::Permanent(def) = &mut program.kind {
            def.attachment = Some(AttachmentKind::Equipment);
        }
        program.activated = vec![
            ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: cost.clone(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![Instruction::MutateObject(Box::new(
                        crate::toolbox::MutateObjectSpec::attach_ability(
                            AttachAbility::Reconfigure,
                        ),
                    ))],
                ),
            },
            ActivatedAbility {
                id: ActivatedAbilityId(2),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        crate::toolbox::MutateObjectSpec::all(
                            crate::toolbox::ObjectQuery::Source,
                            crate::toolbox::ObjectMutation::Attach(
                                crate::toolbox::AttachOp::Unattach,
                            ),
                        ),
                    ))],
                ),
            },
        ];
        program
    }

    /// Construct a non-Aura enchantment using the ordinary permanent body.
    pub fn enchantment(casting_cost: Option<ManaCost>) -> Self {
        Self::new(
            BTreeSet::from([CardType::Enchantment]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: None,
                attachment: None,
                printed_pt: None,
                casting_cost,
            }),
        )
    }
    /// Construct a land with explicit subtypes and any authored mana abilities.
    /// Intrinsic basic-land abilities are derived by the characteristic evaluator.
    pub fn land(land_subtypes: BTreeSet<LandType>, mana: Vec<ManaAbility>) -> Self {
        Self {
            mana,
            ..Self::new(
                BTreeSet::from([CardType::Land]),
                BTreeSet::new(),
                land_subtypes,
                ProgramKind::Permanent(PermanentDefinition {
                    entry_tap: None,
                    attachment: None,
                    printed_pt: None,
                    casting_cost: None,
                }),
            )
        }
    }
    pub fn instant(cost: ManaCost, body: ExecutableBody) -> Self {
        Self::new(
            BTreeSet::from([CardType::Instant]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition {
                cost,
                uncastable_mana_cost: false,
                body,
                modes: None,
            }),
        )
    }
    pub fn sorcery(cost: ManaCost, body: ExecutableBody) -> Self {
        Self::new(
            BTreeSet::from([CardType::Sorcery]),
            BTreeSet::new(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition {
                cost,
                uncastable_mana_cost: false,
                body,
                modes: None,
            }),
        )
    }
    pub fn permanent(&self) -> Option<&PermanentDefinition> {
        match &self.kind {
            ProgramKind::Permanent(value) => Some(value),
            _ => None,
        }
    }
    /// Defined single-face colors before continuous effects. Additional costs,
    /// activation costs and produced mana do not contribute colors (CR 202.2).
    /// Color-defining abilities are applied separately in layer 5.
    pub fn defined_colors(&self) -> BTreeSet<Color> {
        if let Some(colors) = &self.effect_defined_colors {
            return colors.clone();
        }
        if let Some(indicator) = &self.color_indicator {
            return indicator.clone();
        }
        self.colored_mana_symbols().into_keys().collect()
    }

    /// Number of printed mana symbols of each color, including both sides of colored hybrids.
    pub fn colored_mana_symbols(&self) -> BTreeMap<Color, RulesInteger> {
        let mut colors: BTreeMap<_, _> = self
            .printed_mana_cost()
            .into_iter()
            .flat_map(|cost| &cost.required)
            .filter(|(_, n)| n.is_positive())
            .filter_map(|(kind, n)| Color::from_mana(*kind).map(|color| (color, n.clone())))
            .collect();
        for symbol in &self.hybrid_symbols {
            if let crate::mana::HybridAlternative::Mana(other) = symbol.alternative
                && let Some(color) = Color::from_mana(other)
            {
                let amount = colors.entry(color).or_insert_with(|| 0.into());
                *amount = amount.plus(&1.into());
            }
        }
        colors
    }
    /// The qualified name domain on this exact prospective definition.
    pub fn nonland_card_names(&self) -> Option<&std::sync::Arc<crate::land::NonlandCardNames>> {
        self.continuous
            .iter()
            .chain(&self.additional_battlefield_abilities)
            .find_map(|rule| match rule {
                crate::characteristics::StaticContinuousAbility::Restriction {
                    rule:
                        crate::characteristics::StaticRestriction::ProhibitCastingChosenName(names),
                    ..
                } => Some(names),
                _ => None,
            })
    }

    pub fn has_type(&self, kind: CardType) -> bool {
        self.card_types.contains(&kind)
    }
    /// Admit represented permanent types independently of their printed capabilities.
    pub fn validate(&self, vocabulary: &CreatureTypeVocabulary) -> Result<(), DefinitionError> {
        self.validate_definition(vocabulary, false)
    }

    fn validate_definition(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        checking_mode: bool,
    ) -> Result<(), DefinitionError> {
        if self
            .miracle
            .as_ref()
            .is_some_and(|price| !price.mana.valid())
        {
            return Err(DefinitionError::CastingCost);
        }
        if self.cast_during_own_combat && !matches!(self.kind, ProgramKind::Spell(_)) {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(timing) = &self.cast_timing {
            timing.validate(vocabulary)?;
        }
        if !self.hybrid_symbols.is_empty() {
            let mut required = self
                .printed_mana_cost()
                .ok_or(DefinitionError::InvalidInstruction)?
                .required
                .clone();
            for symbol in &self.hybrid_symbols {
                if (Color::from_mana(symbol.first).is_none()
                    && symbol.first != crate::mana::ManaType::Colourless)
                    || matches!(symbol.alternative,
                    crate::mana::HybridAlternative::Mana(other) if symbol.first == other || (Color::from_mana(other).is_none() && other != crate::mana::ManaType::Colourless))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let first = required
                    .get_mut(&symbol.first)
                    .filter(|n| n.is_positive())
                    .ok_or(DefinitionError::InvalidInstruction)?;
                *first = first.minus(&1.into());
            }
        }
        if self.has_type(CardType::Battle) != self.printed_defense.is_some()
            || self.printed_defense.as_ref().is_some_and(|n| *n < 0.into())
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(disturb) = &self.disturb {
            if !disturb.cost.valid()
                || self.permanent().is_none()
                || disturb.back.permanent().is_none()
                || disturb.back.reverse_face().is_some()
                || self.siege_back.is_some()
                || self.nightbound_back.is_some()
                || disturb.back.printed_mana_cost().is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            disturb.back.validate(vocabulary)?;
        }
        if let Some(back) = &self.siege_back {
            if self.permanent().and_then(|p| p.entry_tap)
                != Some(crate::land::EntryTap::SiegeProtector)
                || !self.has_type(CardType::Battle)
                || back.siege_back.is_some()
                || !self.triggered.iter().any(|t| {
                    matches!(
                        t.condition.event,
                        TriggerEvent::CountersChanged {
                            kind: CounterTriggerKind::LastDefenseRemoved,
                            ..
                        }
                    )
                })
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            back.validate(vocabulary)?;
        }
        let planeswalker = self.has_type(CardType::Planeswalker);
        if (planeswalker
            && (self.permanent().is_none()
                || self
                    .printed_loyalty
                    .as_ref()
                    .is_none_or(|value| value < &0.into())))
            || (!planeswalker
                && (self.printed_loyalty.is_some()
                    || !self.loyalty_costs.is_empty()
                    || !self.planeswalker_subtypes.is_empty()))
            || self
                .variable_loyalty_costs
                .iter()
                .any(|id| self.loyalty_costs.get(id) != Some(&0))
            || self.loyalty_costs.keys().any(|id| {
                !self.activated.iter().any(|ability| {
                    ability.id == *id
                        && ability.source_cost == crate::activated::SourceCost::None
                        && ability.mana_cost == ManaCost::free()
                })
            })
        {
            return Err(DefinitionError::UnsupportedPlaneswalker);
        }
        if let Some(split) = &self.split {
            let is_room = split.is_room();
            if split.fuse && (split.aftermath || is_room) {
                return Err(DefinitionError::InvalidInstruction);
            }
            for half in [&split.left, &split.right] {
                if half.split.is_some()
                    || half.adventure.is_some()
                    || (!is_room && half.permanent().is_some())
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                half.validate(vocabulary)?;
            }
            let canonical = split
                .as_ref()
                .clone()
                .into_program()
                .ok_or(DefinitionError::InvalidInstruction)?;
            if self != &canonical {
                return Err(DefinitionError::InvalidInstruction);
            }
            return Ok(());
        }
        if let Some(splice) = &self.splice
            && (!splice.cost.valid() || !self.supports_splice_text())
        {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(adventure) = &self.adventure {
            if self.permanent().is_none()
                || adventure.permanent().is_some()
                || adventure.adventure.is_some()
                || !adventure.spell_subtypes.contains(&SpellSubtype::Adventure)
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            adventure.validate(vocabulary)?;
        }
        if let Some(prepared) = &self.prepare_spell {
            if self.permanent().is_none()
                || prepared.permanent().is_some()
                || self.back_face().is_some()
                || self.split.is_some()
                || self.adventure.is_some()
                || self.prototype.is_some()
                || prepared.prepare_spell.is_some()
                || prepared.back_face().is_some()
                || prepared.split.is_some()
                || prepared.adventure.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            prepared.validate(vocabulary)?;
        }
        if let Some(back) = &self.transform_back {
            if self.permanent().is_none()
                || back.permanent().is_none()
                || back.printed_mana_cost().is_some()
                || self.nightbound_back.is_some()
                || self.siege_back.is_some()
                || self.disturb.is_some()
                || back.reverse_face().is_some()
                || back.disturb.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            back.validate(vocabulary)?;
        }
        if let Some(back) = &self.nightbound_back {
            if self.permanent().is_none()
                || back.permanent().is_none()
                || !self.printed_keywords.contains(&KeywordAbility::Daybound)
                || !back.printed_keywords.contains(&KeywordAbility::Nightbound)
                || back.reverse_face().is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            back.validate(vocabulary)?;
        }
        if let Some(back) = &self.modal_back {
            if self.reverse_face().is_some()
                || self.split.is_some()
                || self.adventure.is_some()
                || self.prototype.is_some()
                || back.back_face().is_some()
                || back.split.is_some()
                || back.adventure.is_some()
                || back.prototype.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            back.validate(vocabulary)?;
        }
        if let Some(back) = &self.flip_back {
            if self.permanent().is_none()
                || back.permanent().is_none()
                || self.reverse_face().is_some()
                || self.modal_back.is_some()
                || self.split.is_some()
                || self.adventure.is_some()
                || self.prototype.is_some()
                || back.back_face().is_some()
                || back.split.is_some()
                || back.adventure.is_some()
                || back.prototype.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            back.validate(vocabulary)?;
        }
        if let Some(proto) = &self.prototype {
            if self.permanent().is_none()
                || proto.permanent().is_none()
                || proto.printed_mana_cost().is_none()
                || self.back_face().is_some()
                || self.split.is_some()
                || self.adventure.is_some()
                || proto.prototype.is_some()
                || proto.back_face().is_some()
                || proto.split.is_some()
                || proto.adventure.is_some()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            proto.validate(vocabulary)?;
        }

        if self
            .printed_toxic
            .as_ref()
            .is_some_and(|amount| amount < &0.into())
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        for (i, cost) in self.additional_cast_costs.iter().enumerate() {
            if !checking_mode
                && matches!(cost, AdditionalCastCost::Entwine(_))
                && (!matches!(&self.kind,ProgramKind::Spell(spell) if spell.modes.as_ref().is_some_and(|modes|!modes.allow_repeat))
                    || self.additional_cast_costs[..i]
                        .iter()
                        .any(|cost| matches!(cost, AdditionalCastCost::Entwine(_))))
            {
                return Err(DefinitionError::CastingCost);
            }
            if matches!(cost, AdditionalCastCost::Replicate(_))
                && self
                    .triggered
                    .iter()
                    .filter(|t| t.cast_copy_keyword() == Some(CastCopyKeyword::Replicate(i)))
                    .count()
                    != 1
            {
                return Err(DefinitionError::CastingCost);
            }
        }
        for copy in self
            .triggered
            .iter()
            .filter_map(TriggeredAbility::cast_copy_keyword)
        {
            if (match copy {
                CastCopyKeyword::CopyThisPermanentSpell => {
                    self.permanent().is_none() || self.has_type(CardType::Land)
                }
                CastCopyKeyword::Replicate(_) | CastCopyKeyword::Storm => {
                    self.has_type(CardType::Land)
                }
                _ => !matches!(self.kind, ProgramKind::Spell(_)),
            }) || matches!(copy,CastCopyKeyword::Replicate(i) if !matches!(self.additional_cast_costs.get(i),Some(AdditionalCastCost::Replicate(_))))
            {
                return Err(DefinitionError::TriggeredSource);
            }
        }
        zone_replacement::validate(self, vocabulary)?;
        if self
            .exile_action
            .as_ref()
            .is_some_and(|kind| !kind.valid() || self.has_type(CardType::Land))
        {
            return Err(DefinitionError::CastingCost);
        }
        if self
            .madness
            .as_ref()
            .is_some_and(|cost| !cost.valid() || self.has_type(CardType::Land))
        {
            return Err(DefinitionError::CastingCost);
        }
        if self
            .plot
            .as_ref()
            .is_some_and(|price| !price.valid() || self.has_type(CardType::Land))
        {
            return Err(DefinitionError::CastingCost);
        }
        if self
            .conditional_cost
            .as_ref()
            .is_some_and(|c| !c.mana().valid() || self.has_type(CardType::Land))
        {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(ConditionalCastCost::Escape {
            exile,
            entry_counters,
            ..
        }) = &self.conditional_cost
            && (*exile == 0 || (*entry_counters > 0 && !self.has_type(CardType::Creature)))
        {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(price) = &self.additional_life_cost
            && (self.has_type(CardType::Land)
                || price
                    .validate_amount(
                        &BTreeSet::new(),
                        BodyContext::Unbound,
                        &FlowFacts::default(),
                        vocabulary,
                    )
                    .is_err())
        {
            return Err(DefinitionError::CastingCost);
        }
        for additional in &self.additional_cast_costs {
            let valid_kind = match &self.kind {
                ProgramKind::Spell(_) => true,
                ProgramKind::Permanent(permanent) => {
                    permanent.casting_cost.is_some()
                        && !self.has_type(CardType::Land)
                        && matches!(
                            additional,
                            AdditionalCastCost::KickerX(_)
                                | AdditionalCastCost::Kicker(_)
                                | AdditionalCastCost::KickerExtra { .. }
                                | AdditionalCastCost::Multikicker(_)
                                | AdditionalCastCost::Replicate(_)
                        )
                }
            };
            if !valid_kind || !additional.mana().valid() {
                return Err(DefinitionError::CastingCost);
            }
            if let AdditionalCastCost::KickerExtra { extra, .. } = additional {
                if extra.is_empty() {
                    return Err(DefinitionError::CastingCost);
                }
                for cost in extra {
                    cost.validate(vocabulary)?;
                }
            }
        }
        if self
            .flashback
            .as_ref()
            .is_some_and(|cost| !cost.valid() || !matches!(self.kind, ProgramKind::Spell(_)))
        {
            return Err(DefinitionError::CastingCost);
        }
        if self.tap_creatures_cast_cost
            && (!matches!(self.kind, ProgramKind::Spell(_))
                || self.sacrifice_creature_cast_cost
                || self.reveal_colorless_creature_cast_cost)
        {
            return Err(DefinitionError::CastingCost);
        }
        if self.reveal_colorless_creature_cast_cost
            && (!matches!(self.kind, ProgramKind::Spell(_)) || self.sacrifice_creature_cast_cost)
        {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(permission) = &self.graveyard_self_cast {
            let allows_no_mana_cost =
                matches!(permission, GraveyardSelfCast::WithAlternativeCost { .. });
            if self.has_type(CardType::Land)
                || (!allows_no_mana_cost && self.printed_mana_cost().is_none())
            {
                return Err(DefinitionError::CastingCost);
            }
            match permission {
                GraveyardSelfCast::Unconditional | GraveyardSelfCast::GraveyardOrExile => {}
                GraveyardSelfCast::WhileControllingSubtype(kind) => {
                    if vocabulary.label(*kind).is_none() {
                        return Err(DefinitionError::CastingCost);
                    }
                }
                GraveyardSelfCast::WithExtraCost(costs) => {
                    if costs.is_empty() {
                        return Err(DefinitionError::CastingCost);
                    }
                    for cost in costs {
                        cost.validate(vocabulary)?;
                    }
                }
                GraveyardSelfCast::WithAlternativeCost { mana, extra } => {
                    if !mana.valid() {
                        return Err(DefinitionError::CastingCost);
                    }
                    for cost in extra {
                        cost.validate(vocabulary)?;
                    }
                }
            }
        }
        if self.graveyard_exile_x_cost && !matches!(self.kind, ProgramKind::Spell(_)) {
            return Err(DefinitionError::InvalidInstruction);
        }
        if self.graveyard_creature_exile_cost > 0 && self.printed_mana_cost().is_none() {
            return Err(DefinitionError::CastingCost);
        }
        if self.discard_cast_cost && self.has_type(CardType::Land) {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(filter) = self.discard_cast_cost_filter {
            if !self.discard_cast_cost {
                return Err(DefinitionError::CastingCost);
            }
            filter.validate_vocabulary(vocabulary)?;
        }
        if let Some(filter) = self.sacrifice_cast_cost_filter {
            if !self.sacrifice_creature_cast_cost {
                return Err(DefinitionError::CastingCost);
            }
            filter.validate_vocabulary(vocabulary)?;
        }
        for extra_cost in &self.extra_cast_costs {
            if self.has_type(CardType::Land) {
                return Err(DefinitionError::CastingCost);
            }
            extra_cost.validate(vocabulary)?;
        }
        if let Some(extra_cost) = &self.per_extra_target_cost {
            let valid_kind = match &self.kind {
                ProgramKind::Spell(_) => !self.has_type(CardType::Land),
                ProgramKind::Permanent(permanent) => {
                    permanent.casting_cost.is_some() && !self.has_type(CardType::Land)
                }
            };
            if !valid_kind || !extra_cost.valid() {
                return Err(DefinitionError::CastingCost);
            }
        }
        for (ability_id, extra_costs) in &self.extra_activation_costs {
            let Some(ability) = self.activated.iter().find(|ab| ab.id == *ability_id) else {
                return Err(DefinitionError::InvalidInstruction);
            };
            let needs_objects = extra_costs
                .iter()
                .any(crate::activated::ExtraActivationCost::uses_selected_objects);
            let has_fixed = ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
            if needs_objects
                && (ability
                    .body
                    .permanent_target_bounds()
                    .is_some_and(|bounds| bounds != (1, 1))
                    || has_fixed
                    || ability.source_cost.uses_selected_cost_origin())
            {
                return Err(DefinitionError::CastingCost);
            }
            for cost in extra_costs {
                cost.validate(vocabulary)?;
            }
        }
        if (self.flashback_sacrifice_creatures != 0
            || self.flashback_tap_white_creatures != 0
            || self.flashback_life != 0
            || self.flashback_x_mana_symbols != 0)
            && self.flashback.is_none()
        {
            return Err(DefinitionError::CastingCost);
        }
        if self.entry_counter_abilities.iter().any(|ability| matches!(ability,EntryCounterAbility::ChargePerKicker(index) | EntryCounterAbility::KickedOpposingNonbasicLands(index)
            if !matches!(self.additional_cast_costs.get(*index),Some(AdditionalCastCost::KickerX(_) | AdditionalCastCost::Kicker(_) | AdditionalCastCost::KickerExtra { .. } | AdditionalCastCost::Multikicker(_))))) {return Err(DefinitionError::InvalidInstruction);}
        if self.entry_counter_abilities.iter().any(|ability| {
            matches!(ability,
                EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(subtype)
                | EntryCounterAbility::GraveyardControlledSubtypePlusOne(subtype)
                    if vocabulary.label(*subtype).is_none()
            )
        }) {
            return Err(DefinitionError::InvalidInstruction);
        }
        if !self.entry_counter_abilities.is_empty()
            && (!matches!(self.kind, ProgramKind::Permanent(_))
                || self
                    .entry_counter_abilities
                    .iter()
                    .any(|ability| match ability {
                        EntryCounterAbility::Bloodthirst(n)
                        | EntryCounterAbility::FixedCharge(n)
                        | EntryCounterAbility::FixedLoyalty(n)
                        | EntryCounterAbility::FixedPlusOne(n)
                        | EntryCounterAbility::FixedMinusOne(n)
                        | EntryCounterAbility::MorbidPlusOne(n) => n < &0.into(),
                        EntryCounterAbility::FixedNamed { kind, amount } => {
                            !kind.is_named() || amount < &0.into()
                        }
                        EntryCounterAbility::DynamicCounter { kind, amount } => {
                            !kind.is_permanent()
                                || amount
                                    .validate_amount(
                                        &BTreeSet::new(),
                                        BodyContext::ACTIVATED,
                                        &FlowFacts::default(),
                                        vocabulary,
                                    )
                                    .is_err()
                        }
                        EntryCounterAbility::ConditionalCounter {
                            condition,
                            kind,
                            amount,
                        } => {
                            !kind.is_permanent()
                                || condition
                                    .validate(BodyContext::ACTIVATED, vocabulary)
                                    .is_err()
                                || amount
                                    .validate_amount(
                                        &BTreeSet::new(),
                                        BodyContext::ACTIVATED,
                                        &FlowFacts::default(),
                                        vocabulary,
                                    )
                                    .is_err()
                        }
                        _ => false,
                    }))
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        for reduction in &self.cast_reductions {
            if matches!(
                reduction,
                CastReduction::ExileGraveyardCreatures { per_card: 0 }
                    | CastReduction::CountZone { per_card: 0, .. }
                    | CastReduction::Conditional { discount: 0, .. }
                    | CastReduction::IfTargets { discount: 0, .. }
            ) {
                return Err(DefinitionError::CastingCost);
            }
            if let CastReduction::CountZone { filter, .. }
            | CastReduction::IfTargets { filter, .. } = reduction
            {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
            }
            if let CastReduction::Conditional { condition, .. } = reduction {
                condition
                    .validate(BodyContext::Unbound, vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
            }
            if let CastReduction::Dynamic(expr) = reduction {
                expr.validate_world_expr(vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
            }
            if let CastReduction::Affinity(selector) = reduction {
                if selector.controller
                    != crate::characteristics::ControllerSelector::SourceController
                {
                    return Err(DefinitionError::CastingCost);
                }
                selector
                    .validate(vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
            }
        }
        if matches!(self.conditional_cost, Some(ConditionalCastCost::Mutate(_)))
            && (!self.has_type(CardType::Creature) || self.permanent().is_none())
        {
            return Err(DefinitionError::CastingCost);
        }
        if matches!(self.conditional_cost, Some(ConditionalCastCost::Bestow(_)))
            && !(self.has_type(CardType::Creature)
                && self.has_type(CardType::Enchantment)
                && matches!(self.kind, ProgramKind::Permanent(_)))
        {
            return Err(DefinitionError::CastingCost);
        }
        if let Some(
            ConditionalCastCost::Overload {
                each: unbracketed, ..
            }
            | ConditionalCastCost::Cleave { unbracketed, .. },
        ) = &self.conditional_cost
        {
            if matches!(
                self.conditional_cost,
                Some(ConditionalCastCost::Overload { .. })
            ) && unbracketed.target != BodyTarget::None
            {
                return Err(DefinitionError::CastingCost);
            }
            let mut alternative = self.clone();
            let ProgramKind::Spell(definition) = &mut alternative.kind else {
                return Err(DefinitionError::CastingCost);
            };
            definition.body = unbracketed.as_ref().clone();
            alternative.conditional_cost = None;
            alternative.validate(vocabulary)?;
        }
        let permanent = self.card_types.iter().copied().any(CardType::is_permanent);
        match &self.conditional_cost {
            Some(
                ConditionalCastCost::ExileHandCards { count: 0, .. }
                | ConditionalCastCost::TapPermanents { count: 0, .. },
            ) => return Err(DefinitionError::CastingCost),
            Some(ConditionalCastCost::Emerge { from, .. }) if !from.is_permanent() => {
                return Err(DefinitionError::CastingCost);
            }
            Some(ConditionalCastCost::Condition { condition, .. }) => {
                condition
                    .left
                    .validate_world_condition(vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
                condition
                    .right
                    .validate_world_condition(vocabulary)
                    .map_err(|_| DefinitionError::CastingCost)?;
            }
            Some(ConditionalCastCost::Alternative {
                condition, extra, ..
            }) => {
                if let Some(cond) = condition {
                    cond.validate_world(vocabulary)
                        .map_err(|_| DefinitionError::CastingCost)?;
                }
                for cost in extra {
                    cost.validate(vocabulary)?;
                }
            }
            Some(ConditionalCastCost::Evoke(_))
                if !permanent
                    || !self.triggered.iter().any(|a| {
                        matches!(
                            a.condition.event,
                            TriggerEvent::Enters {
                                cast_origin: Some(CastEntryKind::Evoked),
                                ..
                            }
                        ) && matches!(
                            a.body.instructions.as_slice(),
                            [Instruction::MutateObject(spec)]
                                if matches!(
                                    spec.op,
                                    crate::toolbox::ObjectMutation::KeywordZoneAction(
                                        crate::toolbox::KeywordZoneOp::SacrificeEvoked
                                    )
                                )
                        )
                    }) =>
            {
                return Err(DefinitionError::CastingCost);
            }
            Some(ConditionalCastCost::EndStep { kind, .. })
                if !permanent
                    || (*kind == EndStepCast::Blitz
                        && !self.triggered.iter().any(|a| {
                            matches!(
                                a.condition.event,
                                TriggerEvent::Dies {
                                    mode: DiesTriggerMode::Blitz,
                                    ..
                                }
                            ) && a.body.instructions
                                == [Instruction::DrawCards(Box::new(
                                    crate::toolbox::DrawCardsSpec::simple(
                                        crate::toolbox::PlayerSelector::Resolving(
                                            ResolvingPlayer::Controller,
                                        ),
                                        ValueExpr::Literal(1.into()),
                                    ),
                                ))]
                        })) =>
            {
                return Err(DefinitionError::CastingCost);
            }
            _ => {}
        }
        if self.entry_copy.is_some() && self.permanent().is_none() {
            return Err(DefinitionError::InvalidInstruction);
        }
        if self.opening_hand_battlefield && self.permanent().is_none() {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(cond) = &self.entry_tap_unless {
            if self.permanent().is_none() || !cond.is_world_evaluable() {
                return Err(DefinitionError::InvalidInstruction);
            }
            cond.validate_world(vocabulary)?;
        }
        if let Some(EntryCopyRule::BattlefieldCreatureWithAbility(ability)) = &self.entry_copy {
            if !ability.mana_cost.valid()
                || ability.source_cost != crate::activated::SourceCost::Tap
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            ability
                .body
                .validate_in(vocabulary, BodyContext::ACTIVATED)?;
        }
        if let Some(EntryCopyRule::Matching(spec)) = &self.entry_copy {
            spec.filter.validate_vocabulary(vocabulary)?;
            if spec
                .extra_creature_subtypes
                .iter()
                .any(|ct| vocabulary.label(*ct).is_none())
            {
                return Err(DefinitionError::ForeignCreatureType);
            }
            if let Some(ability) = &spec.extra_activated {
                if !ability.mana_cost.valid() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                ability
                    .body
                    .validate_in(vocabulary, BodyContext::ACTIVATED)?;
            }
            if let Some(trigger) = &spec.extra_triggered {
                trigger.validate_body(vocabulary)?;
            }
        }
        if let Some(EntryCopyRule::MatchingExt(spec)) = &self.entry_copy {
            spec.validate(vocabulary)?;
        }
        let instruction = self.card_types.contains(&CardType::Instant)
            ^ self.card_types.contains(&CardType::Sorcery);
        if self.card_types.is_empty()
            || self.card_types == BTreeSet::from([CardType::Kindred])
            || match &self.kind {
                ProgramKind::Permanent(_) => {
                    !permanent
                        || self
                            .card_types
                            .iter()
                            .any(|ty| matches!(ty, CardType::Instant | CardType::Sorcery))
                }
                ProgramKind::Spell(_) => !instruction || permanent,
            }
        {
            return Err(DefinitionError::CardTypes);
        }
        if !self.spell_subtypes.is_empty()
            && !(self.has_type(CardType::Instant) || self.has_type(CardType::Sorcery))
        {
            return Err(DefinitionError::CardTypes);
        }
        if !self.artifact_subtypes.is_empty() && !self.has_type(CardType::Artifact) {
            return Err(DefinitionError::CardTypes);
        }
        if !self.enchantment_subtypes.is_empty() && !self.has_type(CardType::Enchantment) {
            return Err(DefinitionError::CardTypes);
        }
        if !self.has_type(CardType::Land) && !self.land_subtypes.is_empty() {
            return Err(DefinitionError::InvalidLandSubtypes);
        }
        if !self
            .card_types
            .iter()
            .copied()
            .any(CardType::carries_creature_subtypes)
            && !self.creature_subtypes.is_empty()
        {
            return Err(DefinitionError::InvalidCreatureSubtypes);
        }
        if self
            .creature_subtypes
            .iter()
            .any(|id| vocabulary.label(*id).is_none())
        {
            return Err(DefinitionError::ForeignCreatureType);
        }
        if self
            .color_indicator
            .as_ref()
            .is_some_and(BTreeSet::is_empty)
        {
            return Err(DefinitionError::ColorIndicator);
        }
        let mut static_ids = BTreeSet::new();
        if self
            .additional_battlefield_abilities
            .iter()
            .any(|ability| !ability.allowed_as_additional())
        {
            return Err(DefinitionError::ContinuousEffect);
        }
        for ability in self
            .continuous
            .iter()
            .chain(&self.additional_battlefield_abilities)
        {
            if !static_ids.insert(ability.id()) {
                return Err(DefinitionError::DuplicateAbility);
            }
            if ability.requires_permanent() && !matches!(self.kind, ProgramKind::Permanent(_)) {
                return Err(DefinitionError::ContinuousEffect);
            }
            ability
                .validate(vocabulary)
                .map_err(|_| DefinitionError::ContinuousEffect)?;
        }
        for (id, cond) in &self.static_conditions {
            if !static_ids.contains(id) {
                return Err(DefinitionError::ContinuousEffect);
            }
            crate::characteristics::validate_static_source_condition(cond, vocabulary)
                .map_err(|_| DefinitionError::ContinuousEffect)?;
        }
        match &self.kind {
            ProgramKind::Permanent(value) => {
                use crate::attachment::AttachmentKind;
                if value.attachment.is_some_and(|kind| {
                    !self.has_type(match kind {
                        AttachmentKind::Aura(_) => CardType::Enchantment,
                        AttachmentKind::Equipment | AttachmentKind::Fortification => {
                            CardType::Artifact
                        }
                    })
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if let Some(AttachmentKind::Aura(
                    crate::attachment::EnchantRestriction::Matching(filter),
                )) = value.attachment
                {
                    filter.validate_vocabulary(vocabulary)?;
                }
                if value
                    .casting_cost
                    .as_ref()
                    .is_some_and(|cost| !cost.valid())
                    || self.has_type(CardType::Land) && value.casting_cost.is_some()
                {
                    return Err(DefinitionError::CastingCost);
                }
                if let Some(crate::land::EntryTap::ChooseOptions(labels)) = value.entry_tap
                    && (labels.len() < 2 || labels.len() > usize::from(u8::MAX))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if matches!(
                    value.entry_tap,
                    Some(
                        crate::land::EntryTap::AlwaysChooseColorExcept(
                            crate::mana::ManaType::Colourless
                        ) | crate::land::EntryTap::ChooseColorExcept(
                            crate::mana::ManaType::Colourless
                        )
                    )
                ) || self.mana.iter().any(|ability| {
                    ability
                        .output
                        .entry_chosen_fixed_color()
                        .is_some_and(|fixed| {
                            !matches!(
                                value.entry_tap,
                                Some(
                                    crate::land::EntryTap::AlwaysChooseColorExcept(c)
                                        | crate::land::EntryTap::ChooseColorExcept(c)
                                ) if c == fixed
                            )
                        })
                }) {
                    return Err(DefinitionError::ManaSource);
                }
                if self.mana.iter().any(|ability| !ability.valid(vocabulary)) {
                    return Err(DefinitionError::ManaSource);
                }
            }
            ProgramKind::Spell(spell) => {
                fn requires_permanent_ability(instruction: &Instruction) -> bool {
                    match instruction {
                        Instruction::StackOp(spec)
                            if matches!(spec.op, crate::toolbox::StackOpKind::Encore) =>
                        {
                            true
                        }
                        Instruction::MutatePlayer(spec)
                            if matches!(
                                spec.op,
                                crate::toolbox::PlayerMutation::LibraryTop(
                                    crate::toolbox::LibraryTopAction::MayRevealTop
                                )
                            ) =>
                        {
                            true
                        }
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                crate::toolbox::ObjectMutation::KeywordZoneAction(
                                    crate::toolbox::KeywordZoneOp::ExileDefeatedSiege
                                )
                            ) =>
                        {
                            true
                        }
                        Instruction::DealDamage(spec)
                            if matches!(
                                spec.recipients,
                                crate::toolbox::DamageTargetQuery::FromPower(
                                    PowerDamageSource::AbilitySource
                                )
                            ) =>
                        {
                            true
                        }
                        Instruction::ApplyContinuous(spec)
                            if matches!(
                                &spec.payload,
                                crate::toolbox::ContinuousPayload::PreventDamage(prev)
                                    if matches!(
                                        prev.source,
                                        crate::damage::PreventionSourceSpec::AbilitySource
                                            | crate::damage::PreventionSourceSpec::CreaturesOtherThanAbilitySource
                                    ) || matches!(
                                        prev.recipient,
                                        crate::damage::PreventionRecipientSpec::AbilitySource
                                    )
                            ) =>
                        {
                            true
                        }
                        Instruction::ScheduleDelayedStepTrigger { body, .. }
                        | Instruction::EmitReflexiveTrigger { body }
                        | Instruction::FixedTargetClause(body) => {
                            body.instructions.iter().any(requires_permanent_ability)
                        }
                        _ => false,
                    }
                }
                if std::iter::once(&spell.body)
                    .chain(spell.modes.iter().flat_map(|m| &m.additional))
                    .any(|body| body.instructions.iter().any(requires_permanent_ability))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if !spell.cost.valid() {
                    return Err(DefinitionError::CastingCost);
                }
                if spell.body.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::DealDamage(spec)
                            if matches!(
                                spec.recipients,
                                crate::toolbox::DamageTargetQuery::FromGrantingPermanent
                                    | crate::toolbox::DamageTargetQuery::DivideThree
                                    | crate::toolbox::DamageTargetQuery::OrderedTargets { .. }
                            )
                    )
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let fixed = spell
                    .body
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
                if fixed
                    && (spell.body.target != BodyTarget::None
                        || spell.body.permanent_target_bounds().is_some())
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let has_reveal_cost = self.reveal_colorless_creature_cast_cost
                    || self.extra_cast_costs.iter().any(|cost| {
                        matches!(
                            cost,
                            crate::activated::ExtraActivationCost::RevealHand { .. }
                        )
                    });
                let has_exile_gy_cost = self.graveyard_creature_exile_cost == 1
                    || self.extra_cast_costs.iter().any(|cost| {
                        matches!(
                            cost,
                            crate::activated::ExtraActivationCost::ExileObjects {
                                zone: crate::activated::CostExileZone::Graveyard,
                                count: Some(n),
                                ..
                            } if n.get() == 1
                        )
                    });
                let cost_context = if has_reveal_cost {
                    BodyContext::RevealedCreature
                } else if has_exile_gy_cost {
                    BodyContext::ExiledCreatureCost
                } else if self.sacrifice_creature_cast_cost {
                    BodyContext::SacrificedCreature
                } else if self.tap_creatures_cast_cost {
                    BodyContext::TapCreatureCost
                } else {
                    BodyContext::Unbound
                };
                spell.body.validate_in(vocabulary, cost_context)?;
                if let Some(modes) = &spell.modes {
                    if std::iter::once(&spell.body)
                        .chain(&modes.additional)
                        .any(|body| body.dependent_targets().is_some())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if modes.maximum < modes.minimum
                        || modes.additional.is_empty()
                        || (!modes.allow_repeat
                            && usize::from(modes.maximum) > modes.additional.len() + 1)
                        || (!modes.mode_costs.is_empty()
                            && (modes.mode_costs.len() != modes.additional.len() + 1
                                || !modes.mode_costs.iter().all(ManaCost::valid)))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if let Some((cond, cond_max)) = &modes.conditional_maximum {
                        cond.validate_world(vocabulary)?;
                        if *cond_max < modes.minimum
                            || (!modes.allow_repeat
                                && usize::from(*cond_max) > modes.additional.len() + 1)
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                    }
                    if let Some(prefix) = &modes.prefix {
                        if prefix.target != BodyTarget::None {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        prefix.validate_in(vocabulary, cost_context)?;
                    }
                    if let Some(suffix) = &modes.suffix {
                        if suffix.target != BodyTarget::None {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        suffix.validate_in(vocabulary, cost_context)?;
                    }
                    // Apply the same spell-only operand, cost and ability checks to every mode.
                    let mut sibling = self.clone();
                    for body in &modes.additional {
                        let ProgramKind::Spell(mode) = &mut sibling.kind else {
                            return Err(DefinitionError::InvalidInstruction);
                        };
                        mode.modes = None;
                        mode.body.clone_from(body);
                        sibling.validate_definition(vocabulary, true)?;
                    }
                }
                if spell.body.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                crate::toolbox::ObjectMutation::Unearth
                                    | crate::toolbox::ObjectMutation::Ninjutsu
                            )
                    )
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if spell.body.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::SearchLibrary { filter }
                            if filter.is_haste_and_return() || filter.is_transfigure()
                    ) || matches!(
                        instruction,
                        Instruction::ApplyContinuous(spec)
                            if matches!(
                                spec.payload,
                                crate::toolbox::ContinuousPayload::BecomeCopy { .. }
                            ) && match &spec.selector {
                                Some(crate::toolbox::ObjectQuery::Source) => true,
                                Some(crate::toolbox::ObjectQuery::EffectSubject(subj)) => {
                                    **subj == EffectSubject::AbilitySource
                                }
                                _ => false,
                            }
                    ) || matches!(
                        instruction,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                crate::toolbox::ObjectMutation::Attach(
                                    crate::toolbox::AttachOp::ToObject {
                                        ability: Some(_), ..
                                    }
                                ) | crate::toolbox::ObjectMutation::ResolvePowerAbility(_)
                                    | crate::toolbox::ObjectMutation::StateMarker(
                                        crate::toolbox::ObjectStateMarkerOp::BecomeMonstrous { .. }
                                            | crate::toolbox::ObjectStateMarkerOp::AdvanceClassLevel(_)
                                            | crate::toolbox::ObjectStateMarkerOp::SolveCase
                                            | crate::toolbox::ObjectStateMarkerOp::SetPrepared
                                    )
                                    | crate::toolbox::ObjectMutation::Combat(
                                        crate::toolbox::ObjectCombatOp::Enlist
                                    )
                            )
                    )
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if !self.mana.is_empty() {
                    return Err(DefinitionError::ManaSource);
                }
                if self.activated.iter().any(|ability| {
                    !ability.source_cost.discards_source()
                        && !ability.source_cost.is_from_graveyard()
                        && !self
                            .extra_activation_costs
                            .get(&ability.id)
                            .is_some_and(|costs| {
                                costs
                                    .iter()
                                    .any(|c| c.is_from_graveyard() || c.is_from_hand())
                            })
                }) {
                    return Err(DefinitionError::ActivatedSource);
                }
                if self.triggered.iter().any(|ability| {
                    !matches!(
                        ability.condition.zone,
                        TriggerZone::Graveyard | TriggerZone::Any
                    ) && !matches!(
                        ability.condition.event,
                        TriggerEvent::SpellCast {
                            self_cast: true,
                            ..
                        } | TriggerEvent::PlayerAction {
                            action: PlayerTriggerAction::Cycled { self_only: true },
                            ..
                        }
                    )
                }) {
                    return Err(DefinitionError::TriggeredSource);
                }
            }
        }
        if self
            .mana
            .iter()
            .map(|a| a.id)
            .collect::<BTreeSet<_>>()
            .len()
            != self.mana.len()
            || self
                .activated
                .iter()
                .map(|a| a.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.activated.len()
            || self
                .triggered
                .iter()
                .map(|a| a.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.triggered.len()
        {
            return Err(DefinitionError::DuplicateAbility);
        }
        if self
            .activation_restrictions
            .keys()
            .any(|id| !self.activated.iter().any(|ability| ability.id == *id))
            || self
                .activation_modes
                .keys()
                .any(|id| !self.activated.iter().any(|ability| ability.id == *id))
            || self
                .activation_cost_reductions
                .keys()
                .any(|id| !self.activated.iter().any(|ability| ability.id == *id))
            || self
                .activation_target_cost_reductions
                .keys()
                .any(|id| !self.activated.iter().any(|ability| ability.id == *id))
            || self
                .zone_replacement_extras
                .keys()
                .any(|id| !self.zone_replacements.iter().any(|zr| zr.id == *id))
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        for expr in self.activation_cost_reductions.values() {
            expr.validate_world_expr(vocabulary)?;
        }
        for (filter, discount) in self.activation_target_cost_reductions.values() {
            if *discount == 0 {
                return Err(DefinitionError::InvalidInstruction);
            }
            filter.validate_vocabulary(vocabulary)?;
        }
        for extra in self.zone_replacement_extras.values() {
            if let Some((kind, _)) = &extra.exile_counters
                && !kind.is_permanent()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let Some(body) = &extra.followup_body {
                body.validate_in(vocabulary, BodyContext::ACTIVATED)?;
            }
        }
        if let Some(flash) = &self.conditional_flash {
            match flash {
                SelfFlashPermission::Condition(cond) => cond.validate_world(vocabulary)?,
                SelfFlashPermission::PayExtraMana(cost) => {
                    if !cost.valid() {
                        return Err(DefinitionError::CastingCost);
                    }
                }
            }
        }
        if self
            .face_up_abilities
            .iter()
            .any(|ability| !ability.cost().valid())
        {
            return Err(DefinitionError::CastingCost);
        }
        for ability in &self.activated {
            use crate::activated::{
                CounterCostDelta, DiscardCostMode, GraveyardExileCost, HandDiscardKeyword,
                LifeCostAmount, SelectedSacrificeFilter, SourceCost, TapCostCount, TapCostFilter,
            };
            if matches!(
                ability.source_cost,
                SourceCost::SacrificeSelected {
                    filter: SelectedSacrificeFilter::GrantingPermanent,
                    ..
                }
            ) || ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::DealDamage(spec)
                        if matches!(
                            spec.recipients,
                            crate::toolbox::DamageTargetQuery::FromGrantingPermanent
                        )
                )
            }) {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let Some(restriction) = self.activation_restrictions.get(&ability.id) {
                restriction.validate(vocabulary)?;
                if (restriction.is_any_player() || restriction.is_only_opponents())
                    && (!matches!(
                        ability.source_cost,
                        SourceCost::None
                            | SourceCost::GenericX {
                                tap_source: false,
                                ..
                            }
                            | SourceCost::Counter {
                                tap_source: false,
                                sacrifice_source: false,
                                ..
                            }
                            | SourceCost::Discard {
                                tap_source: false,
                                ..
                            }
                            | SourceCost::SacrificeSelected {
                                tap_source: false,
                                from_graveyard: false,
                                ..
                            }
                            | SourceCost::PayLife {
                                tap_source: false,
                                ..
                            }
                            | SourceCost::ExileGraveyard {
                                tap_source: false,
                                ..
                            }
                            | SourceCost::Exile {
                                tap_source: false,
                                from_graveyard: false,
                                ..
                            }
                            | SourceCost::Sacrifice
                    ) || self.loyalty_costs.contains_key(&ability.id))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            }
            if matches!(
                ability.source_cost,
                SourceCost::SacrificeSelected { count: 0, .. }
            ) || matches!(ability.source_cost, SourceCost::GenericX { symbols: 0, .. })
                || matches!(
                    ability.source_cost,
                    SourceCost::PayLife {
                        amount: LifeCostAmount::Fixed(0),
                        ..
                    } | SourceCost::GraveyardPayLife {
                        amount: LifeCostAmount::Fixed(0),
                    }
                )
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            let has_fixed_clauses = ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
            if ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::DealDamage(spec)
                        if matches!(
                            spec.recipients,
                            crate::toolbox::DamageTargetQuery::DivideThree
                                | crate::toolbox::DamageTargetQuery::OrderedTargets { .. }
                        )
                )
            }) || (has_fixed_clauses
                && (ability.source_cost.uses_selected_cost_origin()
                    || ability.source_cost.uses_contributors_origin()
                    || ability.source_cost.power_cost().is_some()))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if matches!(
                ability.body.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    filter,
                } if filter.has_power_at_most_x()
            ) && !matches!(
                ability.source_cost,
                SourceCost::TapPermanents {
                    count: TapCostCount::ChosenX,
                    ..
                } | SourceCost::GenericX { .. }
                    | SourceCost::Counter {
                        delta: CounterCostDelta::RemoveAll,
                        ..
                    }
            ) && !self.loyalty_costs.contains_key(&ability.id)
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let SourceCost::TapPermanents {
                filter: TapCostFilter::Matching(filter),
                ..
            }
            | SourceCost::SacrificeSelected {
                filter: SelectedSacrificeFilter::Matching(filter),
                ..
            }
            | SourceCost::PayLife {
                sacrifice: Some(SelectedSacrificeFilter::Matching(filter)),
                ..
            }
            | SourceCost::Discard {
                mode: DiscardCostMode::Filtered { filter, .. },
                ..
            }
            | SourceCost::GraveyardDiscard {
                mode: DiscardCostMode::Filtered { filter, .. },
            }
            | SourceCost::ExileGraveyard {
                filter: GraveyardExileCost::Filtered { filter, .. },
                ..
            }
            | SourceCost::GraveyardExileOther {
                filter: GraveyardExileCost::Filtered { filter, .. },
            } = ability.source_cost
            {
                filter.validate_vocabulary(vocabulary)?;
            }
            if matches!(
                ability.source_cost,
                SourceCost::TapPermanents { filter: TapCostFilter::Subtype(kind), .. }
                    | SourceCost::SacrificeSelected { filter: SelectedSacrificeFilter::Subtype(kind), .. }
                    | SourceCost::PayLife { sacrifice: Some(SelectedSacrificeFilter::Subtype(kind)), .. }
                if vocabulary.label(kind).is_none()
            ) {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let [Instruction::MutateObject(spec)] = ability.body.instructions.as_slice()
                && let crate::toolbox::ObjectMutation::Attach(crate::toolbox::AttachOp::ToObject {
                    ability: Some(attach),
                    ..
                }) = &spec.op
                && (self.permanent().and_then(|value| value.attachment) != Some(attach.kind())
                    || (ability.source_cost != SourceCost::None
                        && !(*attach == crate::attachment::AttachAbility::Equip
                            && !matches!(
                                ability.source_cost,
                                SourceCost::HandDiscardSelf(_)
                                    | SourceCost::GraveyardSelf { .. }
                                    | SourceCost::GraveyardDiscard { .. }
                                    | SourceCost::GraveyardExileOther { .. }
                                    | SourceCost::GraveyardPayLife { .. }
                                    | SourceCost::Ninjutsu { .. }
                            ))))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if !ability.mana_cost.valid() {
                return Err(DefinitionError::CastingCost);
            }
            let extra_costs = self
                .extra_activation_costs
                .get(&ability.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let has_reveal_extra = extra_costs.iter().any(|cost| {
                matches!(
                    cost,
                    crate::activated::ExtraActivationCost::RevealHand { count, .. }
                        if count.get() == 1
                )
            });
            let has_exile_gy_extra = extra_costs.iter().any(|cost| {
                matches!(
                    cost,
                    crate::activated::ExtraActivationCost::ExileObjects {
                        zone: crate::activated::CostExileZone::Graveyard,
                        count: Some(n),
                        ..
                    } if n.get() == 1
                )
            });
            let context = match ability.source_cost {
                SourceCost::Counter { kind, .. } if !kind.is_permanent() => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                SourceCost::Counter { delta: CounterCostDelta::Remove(_) | CounterCostDelta::RemoveAll, .. } => {
                    BodyContext::Activated { named_counter_cost: true }
                }
                SourceCost::ExileGraveyard { filter: GraveyardExileCost::Creature { pay_mana_cost: true }, .. }
                | SourceCost::Discard { mode: DiscardCostMode::WholeHand, .. } => BodyContext::ACTIVATED,
                SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Creature { pay_mana_cost: false },
                    ..
                }
                | SourceCost::GraveyardExileOther {
                    filter: GraveyardExileCost::Creature { pay_mana_cost: false },
                } => BodyContext::ExiledCreatureCost,
                SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Filtered { count, .. },
                    ..
                }
                | SourceCost::GraveyardExileOther {
                    filter: GraveyardExileCost::Filtered { count, .. },
                } if count.get() == 1 => BodyContext::ExiledCreatureCost,
                _ if has_reveal_extra => BodyContext::RevealedCreature,
                _ if has_exile_gy_extra => BodyContext::ExiledCreatureCost,
                SourceCost::SacrificeSelected {
                    filter:
                        SelectedSacrificeFilter::Creature
                        | SelectedSacrificeFilter::OtherCreature
                        | SelectedSacrificeFilter::Subtype(_),
                    count: 1,
                    from_graveyard: false,
                    ..
                } => BodyContext::SacrificedCreature,
                SourceCost::SacrificeSelected {
                    filter: SelectedSacrificeFilter::Matching(filter),
                    count: 1,
                    from_graveyard: false,
                    ..
                } if filter.guarantees_creature() => BodyContext::SacrificedCreature,
                SourceCost::PayLife {
                    sacrifice: Some(SelectedSacrificeFilter::Creature | SelectedSacrificeFilter::OtherCreature),
                    ..
                } => BodyContext::SacrificedCreature,
                SourceCost::SacrificeSelected { count: 1, from_graveyard: false, .. } => BodyContext::SacrificedPermanent,
                _ if matches!(
                    ability.body.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        filter,
                    } if filter.requires_same_name_as_source()
                ) =>
                {
                    BodyContext::ACTIVATED
                }
                _ if ability.body.instructions.iter().any(|op| {
                    let expr_uses_source = |e: &ValueExpr| e.references_ability_source();
                    matches!(
                        op,
                        Instruction::RepeatLoop { count: amount, .. }
                            if expr_uses_source(amount)
                    ) || matches!(op, Instruction::StackOp(spec) if matches!(&spec.op, crate::toolbox::StackOpKind::Copy { count, .. } if expr_uses_source(count))) || matches!(
                        op,
                        Instruction::CreateTokens { creations }
                            if creations.iter().any(|c| expr_uses_source(&c.amount))
                    ) || matches!(
                        op,
                        Instruction::BranchCondition { condition, .. }
                            if matches!(
                                condition.as_ref(),
                                crate::toolbox::ConditionExpr::Resolution(rc)
                                    if expr_uses_source(&rc.left) || expr_uses_source(&rc.right)
                            )
                    ) || matches!(
                        op,
                        Instruction::DrawCards(_)
                            | Instruction::MutateObject(_)
                            | Instruction::AdjustLife(_)
                            | Instruction::DealDamage(_)
                            | Instruction::MutatePlayer(_)
                            | Instruction::ApplyContinuous(_)
                            | Instruction::ZoneOp(_)
                            | Instruction::ScheduleDelayedStepTrigger { .. }
                            | Instruction::EmitReflexiveTrigger { .. }
                            | Instruction::RepeatLoop { .. }
                    )
                }) => BodyContext::ACTIVATED,
                _ if matches!(
                    ability.body.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        filter,
                    } if filter.is_creature_or_other_planeswalker()
                ) =>
                {
                    BodyContext::ACTIVATED
                }
                _ => BodyContext::ACTIVATED,
            };
            ability.body.validate_in(vocabulary, context)?;
            if let Some(modes) = self.activation_modes.get(&ability.id) {
                if modes.maximum < modes.minimum
                    || modes.additional.is_empty()
                    || (!modes.allow_repeat
                        && usize::from(modes.maximum) > modes.additional.len() + 1)
                    || (!modes.mode_costs.is_empty()
                        && (modes.mode_costs.len() != modes.additional.len() + 1
                            || !modes.mode_costs.iter().all(ManaCost::valid)))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if let Some(prefix) = &modes.prefix {
                    if prefix.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    prefix.validate_in(vocabulary, context)?;
                }
                if let Some(suffix) = &modes.suffix {
                    if suffix.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    suffix.validate_in(vocabulary, context)?;
                }
                for extra in &modes.additional {
                    extra.validate_in(vocabulary, context)?;
                }
                continue;
            }
            if matches!(ability.source_cost, SourceCost::GenericX { .. }) {
                continue;
            }
            if matches!(ability.source_cost, SourceCost::Counter { .. }) {
                continue;
            }
            if matches!(ability.source_cost, SourceCost::Mill { .. }) {
                continue;
            }
            if matches!(
                ability.source_cost,
                SourceCost::Discard {
                    mode: DiscardCostMode::WholeHand,
                    ..
                }
            ) {
                continue;
            }
            if matches!(
                ability.source_cost,
                SourceCost::Discard {
                    mode: DiscardCostMode::MatchingTargetSpellManaValue,
                    ..
                }
            ) {
                if ability.body.target != BodyTarget::single_spell(SpellFilter::any())
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::StackOp(spec)]
                            if **spec
                                == crate::toolbox::StackOpSpec::counter_targeted(
                                    crate::toolbox::CounterDestination::Graveyard,
                                )
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if matches!(
                ability.body.instructions.as_slice(),
                [Instruction::StackOp(spec)]
                    if matches!(spec.op, crate::toolbox::StackOpKind::Encore)
            ) {
                if !self.has_type(CardType::Creature)
                    || !matches!(
                        ability.source_cost,
                        SourceCost::Exile {
                            from_graveyard: true,
                            ..
                        }
                    )
                    || self.activation_restrictions.get(&ability.id)
                        != Some(&crate::activated::ActivationRestriction::Sorcery)
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if matches!(
                ability.source_cost,
                SourceCost::Exile {
                    from_graveyard: true,
                    ..
                }
            ) {
                continue;
            }
            let extra_from_graveyard =
                self.extra_activation_costs
                    .get(&ability.id)
                    .is_some_and(|costs| {
                        costs
                            .iter()
                            .any(crate::activated::ExtraActivationCost::is_from_graveyard)
                    });
            if (ability.source_cost.is_from_graveyard() || extra_from_graveyard)
                && !matches!(
                    ability.source_cost,
                    SourceCost::GraveyardSelf { unearth: true }
                )
            {
                continue;
            }
            let enters_from_zone = match ability.source_cost {
                SourceCost::GraveyardSelf { unearth: true } => Some(Instruction::MutateObject(
                    Box::new(crate::toolbox::MutateObjectSpec::unearth()),
                )),
                SourceCost::Ninjutsu { .. } => Some(Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::ninjutsu(),
                ))),
                _ => None,
            };
            if let Some(operation) = enters_from_zone {
                if !matches!(self.kind, ProgramKind::Permanent(_))
                    || ability.body.target != BodyTarget::None
                    || ability.body.instructions != [operation]
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutateObject(spec)
                        if matches!(
                            spec.op,
                            crate::toolbox::ObjectMutation::Unearth
                                | crate::toolbox::ObjectMutation::Ninjutsu
                        )
                )
            }) {
                return Err(DefinitionError::InvalidInstruction);
            } else if ability.source_cost
                == SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce)
            {
                if ability.body.target != BodyTarget::SINGLE_CREATURE
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::MutateObject(spec)]
                            if matches!(spec.source, crate::toolbox::ObjectQuery::Targets)
                                && matches!(
                                    spec.op,
                                    crate::toolbox::ObjectMutation::Counters(
                                        crate::toolbox::CounterOp::Add {
                                            kind: crate::effects::CounterKind::PlusOne,
                                            ..
                                        }
                                    )
                                )
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.instructions.iter().any(
                |op| matches!(op, Instruction::SearchLibrary { filter } if filter.is_transfigure()),
            ) {
                if ability.source_cost != SourceCost::Sacrifice
                    || self.activation_restrictions.get(&ability.id)
                        != Some(&crate::activated::ActivationRestriction::Sorcery)
                    || ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::SearchLibrary { filter }]
                            if filter.is_transfigure()
                                && filter.mana_value == Some(SearchManaValue::Exact(self.mana_value()))
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.source_cost
                == SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute)
            {
                if ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::SearchLibrary { filter }]
                            if filter.matcher == SearchCardMatcher::Any
                                && filter.destination == SearchDestination::Hand
                                && filter.mana_value == Some(SearchManaValue::Exact(self.mana_value()))
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.source_cost
                == SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling)
            {
                if !ability.valid_cycling_body() {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.source_cost
                == SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel)
            {
                if ability.body.ordered_any_target_bounds().is_some() {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if let Some((kind, _)) = ability.source_cost.power_cost() {
                if !matches!(
                    ability.body.instructions.as_slice(),
                    [Instruction::MutateObject(spec)]
                        if spec.op == crate::toolbox::ObjectMutation::ResolvePowerAbility(kind)
                ) || ability.mana_cost != ManaCost::free()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if self.activation_restrictions.get(&ability.id)
                == Some(&crate::activated::ActivationRestriction::PowerUp)
            {
                if ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::MutateObject(_)]
                            | [Instruction::MutateObject(_), Instruction::DrawCards(_)]
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.target == BodyTarget::None
                && !has_fixed_clauses
                && !self.loyalty_costs.contains_key(&ability.id)
                && !(self.permanent().is_some()
                    && matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::SearchLibrary { .. }]
                    ))
                && !((self.permanent().is_some()
                    || self
                        .extra_activation_costs
                        .get(&ability.id)
                        .is_some_and(|costs| {
                            costs.contains(&crate::activated::ExtraActivationCost::RevealSource)
                        }))
                    && ability.body.instructions.iter().all(|operation| {
                        let targetless_player = |player: ResolvingPlayer| {
                            matches!(
                                player,
                                ResolvingPlayer::Controller
                                    | ResolvingPlayer::ActivePlayer
                                    | ResolvingPlayer::EnchantedPlayer
                                    | ResolvingPlayer::AttachedHostController
                                    | ResolvingPlayer::SourceChosenPlayer
                                    | ResolvingPlayer::LoopPlayer
                                    | ResolvingPlayer::RandomOpponent
                                    | ResolvingPlayer::ChosenPlayer
                                    | ResolvingPlayer::SourceOwner
                            )
                        };
                        match operation {
                            Instruction::ScheduleDelayedStepTrigger { .. }
                            | Instruction::EmitReflexiveTrigger { .. }
                            | Instruction::SearchLibrary { .. }
                            | Instruction::Jump { .. }
                            | Instruction::CreateTokens { .. }
                            | Instruction::ScheduleDelayedEventTrigger { .. }
                            | Instruction::RepeatLoop { .. } => true,
                            Instruction::ZoneOp(_)
                            | Instruction::DrawCards(_)
                            | Instruction::MutateObject(_)
                            | Instruction::AdjustLife(_)
                            | Instruction::DealDamage(_)
                            | Instruction::MutatePlayer(_)
                            | Instruction::ApplyContinuous(_)
                            | Instruction::PromptChoice(_)
                            | Instruction::BranchCondition { .. }
                            | Instruction::ForEach { .. }
                            | Instruction::ForEachPlayerQuery { .. }
                            | Instruction::BindEnv { .. }
                            | Instruction::SetObjectTag { .. }
                            | Instruction::UpdateStateTrack { .. }
                            | Instruction::SideDeckOp { .. }
                            | Instruction::StackOp(_) => !operation.uses_mode_target(),
                            Instruction::OptionalSequence { chooser, .. } => {
                                targetless_player(*chooser)
                            }
                            _ => false,
                        }
                    }))
            {
                return Err(DefinitionError::TargetlessActivation);
            }
        }
        for ability in &self.triggered {
            ability.validate_body(vocabulary)?;
        }
        Ok(())
    }
}

mod token_keywords;
pub(crate) use token_keywords::mobilize_warrior;
pub use token_keywords::{TokenKeyword, decayed_zombie};

impl TriggeredAbility {
    pub(crate) fn validate_body(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.condition.validate_vocabulary(vocabulary)?;
        if let TriggerEvent::Keyword(TriggerKeywordEvent::Soulbond { entrant_only }) =
            self.condition.event
            && !matches!(
                self.body.instructions.as_slice(),
                [Instruction::MutateObject(spec)]
                    if spec.as_ref()
                        == &crate::toolbox::MutateObjectSpec::soulbond_pair(entrant_only)
            )
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(TriggerCondition::HandSizeBelow(required)) = self.condition.intervening_if
            && required == 0
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let TriggerEvent::PlayerAction {
            action: PlayerTriggerAction::Sacrificed(ObjectFilter::Clause(c)),
            ..
        } = self.condition.event
            && (c.any_types.bits() & !CardTypeMask::PERMANENT.bits()) != 0
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters { kind, .. }) =
            self.condition.event
            && (!kind.is_permanent() || self.grouping != TriggerGrouping::EachOccurrence)
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let TriggerEvent::StateThreshold(StateThresholdKind::ControlledSubtype {
            minimum, ..
        }) = self.condition.event
            && (minimum == 0 || self.grouping != TriggerGrouping::EachOccurrence)
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let TriggerEvent::CountersChanged {
            kind: CounterTriggerKind::SagaChapter(chapter),
            ..
        } = self.condition.event
            && (chapter == 0 || self.grouping != TriggerGrouping::EachOccurrence)
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(cond) = self
            .condition
            .intervening_if
            .as_ref()
            .and_then(InterveningCondition::from_trigger_condition)
        {
            let needs_each_occurrence = matches!(
                cond,
                InterveningCondition::OwnCommander
                    | InterveningCondition::GreatestArtifact
                    | InterveningCondition::OpponentMoreLands
            );
            let is_req = |op: Option<&Instruction>| {
                matches!(
                    op,
                    Some(Instruction::MutatePlayer(spec))
                        if spec.op == crate::toolbox::PlayerMutation::RequireCondition(cond)
                )
            };
            let has_require = is_req(self.body.instructions.first())
                || matches!(
                    self.body.instructions.first(),
                    Some(Instruction::PromptChoice(spec))
                        if !spec.option_branches.is_empty()
                            && spec
                                .option_branches
                                .iter()
                                .all(|branch| is_req(branch.instructions.first()))
                );
            if (needs_each_occurrence && self.grouping != TriggerGrouping::EachOccurrence)
                || !has_require
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        if self.body.instructions.iter().any(
            |op| matches!(op, Instruction::SearchLibrary { filter } if filter.is_transfigure()),
        ) {
            return Err(DefinitionError::InvalidInstruction);
        }
        // The printed suspend action supplies its own two exile abilities.
        if matches!(
            self.condition.event,
            TriggerEvent::Keyword(TriggerKeywordEvent::Counter(
                CounterKeyword::SuspendUpkeep | CounterKeyword::SuspendLastCounter
            ))
        ) {
            return Err(DefinitionError::TriggeredSource);
        }
        if self
            .body
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::MutateObject(spec) if matches!(spec.op, crate::toolbox::ObjectMutation::Unearth | crate::toolbox::ObjectMutation::Ninjutsu)))
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if matches!(
            self.condition.event,
            TriggerEvent::BlocksOrBlocked {
                kind: BlockTriggerKind::BlockedByEach {
                    without_flanking_only: true
                },
                ..
            }
        ) && self.grouping != TriggerGrouping::EachOccurrence
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        self.body.validate_in(
            vocabulary,
            BodyContext::TriggerBound {
                condition: &self.condition,
                grouping: self.grouping,
            },
        )?;
        Ok(())
    }
}

/// Where the controller sends a card selected from an opponent's revealed hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChosenHandAction {
    /// The opponent discards the chosen card, allowing discard replacements.
    Discard,
    /// Exile the chosen card without a discard event.
    Exile,
}

#[cfg(test)]
mod stage_a_validation_and_jump_tests {
    use super::*;
    use crate::activated::{
        ActivatedAbility, ActivatedAbilityId, ExtraActivationCost, LifeCostAmount,
        SelectedSacrificeFilter, SourceCost,
    };
    use crate::mana::ManaCost;
    use spellwake_contracts::creature_types::CreatureTypeVocabulary;
    use std::num::NonZeroU8;

    fn test_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test-v1".to_string(),
            [0x11; 32],
            vec!["Human".to_string(), "Soldier".to_string()],
        )
        .unwrap()
    }

    #[test]
    fn shift_jumps_relocates_all_six_jump_carrying_instructions() {
        let mut ops = vec![
            Instruction::Jump { destination: 5 },
            Instruction::BranchCondition {
                condition: Box::new(crate::toolbox::ConditionExpr::Resolution(Box::new(
                    ResolutionCondition {
                        left: ValueExpr::literal(1),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    },
                ))),
                destination: 4,
            },
            Instruction::MutatePlayer(Box::new(crate::toolbox::MutatePlayerSpec::flip_coins(
                ResolvingPlayer::Controller,
                CoinFlipMode::Single,
                None,
                None,
                Some(6),
            ))),
            Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::Controller,
                    ManaCost::free(),
                    ValueExpr::literal(0),
                    3,
                    7,
                ),
            )),
            Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::Controller,
                    ManaCost::free(),
                    ValueExpr::literal(1),
                    2,
                    8,
                ),
            )),
            Instruction::MutatePlayer(Box::new(
                crate::toolbox::MutatePlayerSpec::offer_source_counters(1.into(), 1, 9),
            )),
        ];
        for op in &mut ops {
            op.shift_jumps(2, 3);
        }
        assert!(matches!(ops[0], Instruction::Jump { destination: 7 }));
        assert!(matches!(
            ops[1],
            Instruction::BranchCondition { destination: 6, .. }
        ));
        assert!(matches!(
            &ops[2],
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::RandomEvent(
                        crate::toolbox::RandomEventOp::FlipCoins {
                            lose_destination: Some(8),
                            ..
                        }
                    )
                )
        ));
        assert!(matches!(
            &ops[3],
            Instruction::PromptChoice(spec)
                if matches!(spec.as_mana_payment(), Some((_, _, _, 5, 9)))
        ));
        assert!(matches!(
            &ops[4],
            Instruction::PromptChoice(spec)
                if matches!(spec.as_mana_payment(), Some((_, _, _, 2, 10)))
        ));
        assert!(matches!(
            &ops[5],
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::OfferSourceCounters {
                        placed_destination: 1,
                        declined_destination: 11,
                        ..
                    }
                )
        ));
    }

    #[test]
    fn unimplemented_choice_instructions_and_extra_costs_fail_validation_closed() {
        let vocab = test_vocab();

        // Interactive choice instructions (Blight, CollectEvidence, RemoveCounters any-kind) now validate cleanly.
        for op in [
            Instruction::MutatePlayer(Box::new(
                crate::toolbox::MutatePlayerSpec::collect_evidence(
                    ResolvingPlayer::Controller,
                    ValueExpr::literal(4),
                ),
            )),
            Instruction::MutatePlayer(Box::new(crate::toolbox::MutatePlayerSpec::blight(
                ResolvingPlayer::Controller,
                ValueExpr::literal(2),
            ))),
        ] {
            let p = Program::sorcery(
                ManaCost::free(),
                ExecutableBody::new(BodyTarget::None, vec![op]),
            );
            assert!(
                p.validate(&vocab).is_ok(),
                "interactive player-choice instruction should validate"
            );
        }
        let mut perm = Program::artifact(None, vec![]);
        perm.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::all(
                        crate::toolbox::ObjectQuery::Source,
                        crate::toolbox::ObjectMutation::Counters(
                            crate::toolbox::CounterOp::Remove {
                                kind: None,
                                amount: ValueExpr::literal(1),
                            },
                        ),
                    ),
                ))],
            ),
        });
        assert!(perm.validate(&vocab).is_ok());

        // ExtraActivationCost now supports random discard, Either, PayMana, and kind: None
        for valid_cost in [
            ExtraActivationCost::DiscardCards {
                count: NonZeroU8::new(1),
                filter: ObjectFilter::any(),
                random: true,
                whole_hand: false,
            },
            ExtraActivationCost::Either(
                Box::new(ExtraActivationCost::PayLife(LifeCostAmount::Fixed(2))),
                Box::new(ExtraActivationCost::SacrificeSource),
            ),
            ExtraActivationCost::RemovePermanentCounters {
                filter: ObjectFilter::any(),
                kind: None,
                count: NonZeroU8::new(1),
                all: false,
            },
            ExtraActivationCost::PayMana(ManaCost::generic(2.into())),
        ] {
            assert!(valid_cost.validate(&vocab).is_ok());
        }

        // Conflicting selected-object ExtraActivationCost with SourceCost::SacrificeSelected fails closed
        let mut p = Program::artifact(None, vec![]);
        p.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::SacrificeSelected {
                count: 1,
                filter: SelectedSacrificeFilter::Creature,
                tap_source: false,
                from_graveyard: false,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawCards(Box::new(
                    crate::toolbox::DrawCardsSpec::simple(
                        crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                        ValueExpr::literal(1),
                    ),
                ))],
            ),
        });
        p.extra_activation_costs
            .insert(ActivatedAbilityId(0), vec![ExtraActivationCost::Forage]);
        assert!(p.validate(&vocab).is_err());
    }
}
